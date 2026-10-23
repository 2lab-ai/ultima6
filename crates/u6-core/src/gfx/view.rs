//! The map window: render 0x102c90 and the refresh 0x102ef0.
//!
//! Geometry (0x102c90 and 0x10e81e; spec 13 C3): a [`VIEW_W`] × [`VIEW_H`] grid of 16-pixel
//! cells (11 × 11 in Original) drawn from frame pixel ([`MAP_X`], [`MAP_Y`]) ((0, 0) in Original),
//! cell (col, row) at pixel (`MAP_X` + 16·col, `MAP_Y` + 16·row); the view centre (A4-0x7b78 /
//! -0x7b76 / -0x7b74, [`crate::map::MapState::centre`]) sits in cell ([`HALF_W`], [`HALF_H`]),
//! (5, 5) in Original, pixels 80..96 on both axes (0x10e81e places an object at
//! `x - centre_x + 5`). Only the pixels of [`SHOWN`] reach the front buffer (0x151f5a, 0x1520e2
//! with (8, 8)..(0xa7, 0xa7) in Original): the full cells and a half cell on each side. The frame
//! is 8-wide border tiles 0x1b0..0x1b7 on the first and last column and row of cells (x / y = 0
//! and 0xa0 in Original).

use u6_render::{Framebuffer, Rgb};

use super::blit::{blit_back, copy_window_rows, plot, set_plot_colour};
use super::cache::{BOXES, WindowKey, cell_box};
use super::tables::GameTables;
use super::{GfxState, anim, cache, seams};
use crate::actors::Calendar;
use crate::engine::Io;
use crate::geom::{HALF_H, HALF_W, MAP_X, MAP_Y, SHOWN, VIEW_H, VIEW_W};
use crate::state::State;

/// Cells per side of the Original map window (`moveq #0xb` loops in 0x102c90). The view's size
/// is [`VIEW_W`] × [`VIEW_H`] (spec 13); this module no longer reads it.
pub const WINDOW_TILES: usize = 11;
/// Cell side in pixels (`asl.w #4`).
const CELL: u16 = 16;
// The cells' boxes are at multiples of 16 from the frame's corner ([`super::cache`]), and the
// blitter takes x down to a multiple of 8.
const _: () = assert!(
    MAP_X.is_multiple_of(CELL) && MAP_Y.is_multiple_of(CELL),
    "the map's corner on a cell boundary"
);
/// Object type whose tile gets clock hands (0x13aa7e = basetile 0x13a940 + 2·0x9f).
const CLOCK_TYPE: usize = 0x9f;

/// What 0x102c90 draws: the window tile cache A4-0x1476 (11×11 words in Original, built by
/// 0x109e74) and per cell the object tile words in draw order (the lists at A4-0x15f0 heads,
/// A4-0x1384 words, A4-0x1577 links, built by 0x10e81e). Words carry the outline bits 11..12.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewWindow {
    /// `tiles[row][col]`: [`VIEW_H`] rows of [`VIEW_W`].
    pub tiles: [[u16; VIEW_W]; VIEW_H],
    /// `objects[row * VIEW_W + col]`, first drawn first.
    pub objects: Vec<Vec<u16>>,
}

impl Default for ViewWindow {
    /// Tile 0 in every cell and no object lists (written out: arrays of Wide's 37 columns have no
    /// derived default).
    fn default() -> Self {
        Self {
            tiles: [[0; VIEW_W]; VIEW_H],
            objects: Vec::new(),
        }
    }
}

impl ViewWindow {
    /// Every cell empty (tile 0, no objects).
    pub fn empty() -> Self {
        Self {
            tiles: [[0; VIEW_W]; VIEW_H],
            objects: vec![Vec::new(); VIEW_W * VIEW_H],
        }
    }
}

/// The frame pixel of view cell (col, row)'s top left: (`MAP_X` + 16·col, `MAP_Y` + 16·row).
pub(crate) fn cell_pixel(col: usize, row: usize) -> (u16, u16) {
    (MAP_X + col as u16 * CELL, MAP_Y + row as u16 * CELL)
}

/// Replaces: 0x162358 (copies the 32 OCS colours into the copper list)
/// Replaces: 0x16236a (installs the copper list: the five bitplane pointers, the pointer sprite)
///
/// The chunky buffer carries its palette; every window draw writes the 32 game colours. The
/// engine's buffers are the bitplanes, and the pointer sprite is its overlay
/// ([`crate::ui::pointer::vblank`]).
fn load_palette(state: &State, fb: &mut Framebuffer) {
    for (dst, c) in fb.palette.iter_mut().zip(&state.tables.palette) {
        *dst = Rgb::new(c.r, c.g, c.b);
    }
}

/// Original: 0x102bf2 (clock hands: three pixels on a clock tile drawn at (x, y))
///
/// Plot colour A4-0x6568, into the back buffer: the pivot (x + 4, y + 14), the hour hand one
/// step from it in direction `hour_dir[hour % 12]`, the minute hand in direction
/// `minute · 2 / 15` (hour A4-0x7b81, minute A4-0x7b82; `divs.w`). The plot colour stays set.
fn clock_hands(cal: &Calendar, gfx: &mut GfxState, fb: &mut Framebuffer, x: u16, y: u16) {
    let points = clock_points(cal, &gfx.tables, x, y);
    set_plot_colour(gfx, gfx.tables.clock_colour);
    gfx.plot_back = true;
    for (hx, hy) in points {
        plot(gfx, fb, hx, hy);
    }
    gfx.plot_back = false;
}

/// The three pixels [`clock_hands`] plots for a clock tile at (x, y): the pivot, the hour
/// hand's tip, the minute hand's tip.
pub(super) fn clock_points(cal: &Calendar, t: &GameTables, x: u16, y: u16) -> [(u16, u16); 3] {
    let (px, py) = (x.wrapping_add(4), y.wrapping_add(0xe));
    let hour = usize::from(t.hour_dir[usize::from(cal.hour % 12)]);
    let minute = usize::from(cal.minute) * 2 / 15;
    let tip = |d: usize| {
        let (dx, dy) = (t.dir_dx.get(d).copied(), t.dir_dy.get(d).copied());
        let step = |v: u16, s: Option<i8>| v.wrapping_add(s.unwrap_or(0) as i16 as u16);
        (step(px, dx), step(py, dy))
    };
    [(px, py), tip(hour), tip(minute)]
}

/// The border of 0x102c90 (`0x00102E16..0x00102EC2`): corners 0x1b0 (0, 0), 0x1b2 (0xa0, 0),
/// 0x1b3 (0, 0xa0), 0x1b5 (0xa0, 0xa0), then for i = 1..10 the edges 0x1b1 (16i, 0),
/// 0x1b4 (16i, 0xa0), 0x1b6 (0, 16i), 0x1b7 (0xa0, 16i) (Original's pixels; [`border_tiles`]).
/// Each tile lies on an edge cell; `only` keeps those of the boxes it marks.
fn draw_border(gfx: &GfxState, fb: &mut Framebuffer, only: Option<&[bool; BOXES]>) {
    for (t, x, y) in border_tiles() {
        if only.is_none_or(|d| cell_box(x, y).is_some_and(|b| d[b])) {
            blit_back(gfx, fb, t, x, y);
        }
    }
}

/// The border's (tile, x, y) in the order [`draw_border`] draws them: the corners on the view's
/// first and last cells, then for i = 1.. the edge cells i of the top, bottom, left and right
/// edges (the top and bottom ones up to `VIEW_W - 2`, the left and right ones up to `VIEW_H - 2`;
/// 1..10 for all four in Original).
pub(crate) fn border_tiles() -> impl Iterator<Item = (u16, u16, u16)> {
    let (left, top) = cell_pixel(0, 0);
    let (right, bottom) = cell_pixel(VIEW_W - 1, VIEW_H - 1);
    let corners = [
        (0x1b0, left, top),
        (0x1b2, right, top),
        (0x1b3, left, bottom),
        (0x1b5, right, bottom),
    ];
    let edges = (1..VIEW_W.max(VIEW_H) - 1).flat_map(move |i| {
        let (x, y) = cell_pixel(i, i);
        let across = i < VIEW_W - 1;
        let down = i < VIEW_H - 1;
        [
            (across, (0x1b1, x, top)),
            (across, (0x1b4, x, bottom)),
            (down, (0x1b6, left, y)),
            (down, (0x1b7, right, y)),
        ]
        .into_iter()
        .filter_map(|(on, t)| on.then_some(t))
    });
    corners.into_iter().chain(edges)
}

/// The border word of each cell's box ([`border_tiles`]).
fn border_of_boxes() -> [Option<u16>; BOXES] {
    let mut at = [None; BOXES];
    for (t, x, y) in border_tiles() {
        if let Some(b) = cell_box(x, y) {
            at[b] = Some(t);
        }
    }
    at
}

/// Original: 0x102c90 (render the map window into the back buffer; with `framed`, add the cursor
/// and border and present it)
///
/// Order: plot colour = A4-0x6570; per cell (rows, then columns) the cache tile, then its
/// objects, each via 0x152ffa (0x14f12c before each cell is os code whose results the blitter
/// never reads, so it is not called); an object whose tile is the clock's gets hands
/// ([`clock_hands`]).
/// With A4-0x7b65 set: 100 XOR pixels of colour 8 into the back buffer at
/// (random 8..=0xa7, random 8..=0xa7) (Original; random within [`SHOWN`] per axis, one draw per
/// axis either way). With `framed` (D0): the ui's target cursor, the border,
/// then the window goes to the front, dissolved when A4-0x6578 is set (cleared after), copied
/// otherwise.
pub fn render_window(state: &mut State, io: &Io, framed: bool) {
    let inputs = window_inputs(state, io, framed);
    draw_window(state, io, framed, &inputs);
}

/// What 0x102c90 takes before it draws: the window contents, the clock's tile word, and with
/// `framed` the target cursor (whose call 0x106932 may redraw the panel cursor). The plot
/// colour is set first.
struct WindowInputs {
    window: ViewWindow,
    clock: Option<u16>,
    cursor: Option<(u16, u16, u16)>,
}

fn window_inputs(state: &mut State, io: &Io, framed: bool) -> WindowInputs {
    let colour = state.gfx.tables.window_colour;
    set_plot_colour(&mut state.gfx, colour);
    let window = seams::view_window(state);
    let clock = state.tables.basetile.base.get(CLOCK_TYPE).copied();
    let cursor = if framed {
        seams::map_cursor(state, io)
    } else {
        None
    };
    WindowInputs {
        window,
        clock,
        cursor,
    }
}

/// The rest of 0x102c90: the palettes, the cells into the back buffer, the sparkle, with
/// `framed` the cursor, the border and the present.
fn draw_window(state: &mut State, io: &Io, framed: bool, inputs: &WindowInputs) {
    io.draw(|fb| load_palette(state, fb));
    io.draw_back(|fb| load_palette(state, fb));
    draw_boxes(state, io, framed, inputs, None);
    if framed {
        let gfx = &mut state.gfx;
        let dissolve = gfx.dissolve != 0;
        seams::present_window(io, gfx, dissolve);
        if dissolve {
            gfx.dissolve = 0;
        }
    }
}

/// The back-buffer draws of 0x102c90: the cells (rows, then columns), the sparkle, with `framed`
/// the cursor and the border. With `only`, the draws in the boxes it marks, in the same order
/// (no sparkle then: [`render_window_cached`]).
fn draw_boxes(
    state: &mut State,
    io: &Io,
    framed: bool,
    inputs: &WindowInputs,
    only: Option<&[bool; BOXES]>,
) {
    let WindowInputs {
        window,
        clock,
        cursor,
    } = inputs;
    let clock = *clock;
    let State {
        gfx, actors, rng, ..
    } = state;
    io.draw_back(|fb| {
        for (row, tiles) in window.tiles.iter().enumerate() {
            for (col, &tile) in tiles.iter().enumerate() {
                if only.is_some_and(|d| !d[row * VIEW_W + col]) {
                    continue;
                }
                let (x, y) = cell_pixel(col, row);
                blit_back(gfx, fb, tile, x, y);
                for &word in &window.objects[row * VIEW_W + col] {
                    blit_back(gfx, fb, word, x, y);
                    if Some(word & 0x7ff) == clock {
                        clock_hands(&actors.calendar, gfx, fb, x, y);
                    }
                }
            }
        }
        if gfx.sparkle != 0 {
            gfx.plot_xor = true;
            gfx.plot_back = true;
            set_plot_colour(gfx, 8);
            let (x0, y0, x1, y1) = SHOWN;
            for _ in 0..100 {
                let x = rng.range(x0, x1);
                let y = rng.range(y0, y1);
                plot(gfx, fb, x, y);
            }
            gfx.plot_xor = false;
            gfx.plot_back = false;
        }
        if framed {
            if let Some((tile, x, y)) = *cursor
                && only.is_none_or(|d| cell_box(x, y).is_some_and(|b| d[b]))
            {
                blit_back(gfx, fb, tile, x, y);
            }
            draw_border(gfx, fb, only);
        }
    });
}

/// Original: 0x102ef0 (post-step refresh: rebuild the view flags if stale, redraw the window,
/// redraw a pending panel)
///
/// A4-0x6a8e set: 0x109e74, then clear. A4-0x7af8 set: the animation tick 0x10202e and
/// 0x102c90 with D0 = 1. A4-0x7afc non-zero: clear it and redraw that panel (0x133a1a with
/// D1 = 1).
///
/// Port (spec 06 O2, [`Io::set_window_cache`], on by default): the render's draws are left out
/// where they would change nothing ([`super::cache`]); the frames and the state are those of
/// the original's every-time render.
pub fn refresh_view(state: &mut State, io: &Io) {
    if state.map.view_dirty != 0 {
        seams::view_flags(state);
        state.map.view_dirty = 0;
    }
    if state.gfx.window_shown != 0 {
        anim::anim_tick(state);
        if io.window_cache() {
            render_window_cached(state, io);
        } else {
            render_window(state, io, true);
        }
    }
    if state.gfx.panel_redraw != 0 {
        let shakes = state.gfx.panel_redraw;
        state.gfx.panel_redraw = 0;
        seams::quake(state, io, shakes);
    }
}

/// [`render_window`] framed, drawing only the boxes whose draws read a changed value, none when
/// none did ([`cache::dirty_boxes`]; the other boxes in the back buffer are what their draws
/// would draw again). Then the palettes are loaded as the render loads them, the plot colour and
/// plane are those the clock hands leave, and the window is presented (the rows of its rect
/// where the screen differs from the back buffer copied: what [`super::blit::copy_window`]
/// changes). Every box is drawn, as by [`render_window`], when nothing is remembered, and the
/// render is not remembered with the sparkle, the dissolve or XOR plots on, or a draw that would
/// leave its box ([`WindowKey::of`]).
fn render_window_cached(state: &mut State, io: &Io) {
    cache::check_static(&mut state.gfx);
    let inputs = window_inputs(state, io, true);
    let border = border_of_boxes();
    let key = WindowKey::of(state, &inputs.window, inputs.clock, inputs.cursor, &border);
    let dirty = key
        .as_ref()
        .and_then(|key| cache::dirty_boxes(&state.gfx, io.back_mark(), key));
    match (&key, dirty) {
        (Some(key), Some(dirty)) => {
            io.draw(|fb| load_palette(state, fb));
            io.draw_back(|fb| load_palette(state, fb));
            let redrawn = dirty.iter().filter(|&&d| d).count();
            if redrawn > 0 {
                draw_boxes(state, io, true, &inputs, Some(&dirty));
            }
            let gfx = &mut state.gfx;
            if key.clock {
                set_plot_colour(gfx, gfx.tables.clock_colour);
                gfx.plot_back = false;
            }
            io.with_buffers(copy_window_rows);
            io.present();
            if redrawn == 0 {
                gfx.cache.window_skips += 1;
            } else {
                gfx.cache.window_partial += 1;
                gfx.cache.boxes_redrawn += redrawn as u64;
            }
        }
        _ => draw_window(state, io, true, &inputs),
    }
    let memo = key.map(|key| (key, io.back_mark()));
    cache::remember_window(&mut state.gfx, memo);
}

/// Original: 0x132338 (ui: flash an actor in the map window: its tile in outline mode over its
/// cell, the window presented dissolving, then the window again without it)
///
/// Nothing for an actor off the map (status A4+0x190c & 0x18) or outside the view (more than five
/// cells from the view centre on either axis in Original, `(x - centre + 5) & 0x3ff < 11`; spec 13
/// Decision 5: [`HALF_W`] and [`HALF_H`] cells, the view cell inside [`VIEW_W`] × [`VIEW_H`]).
/// The tile word is the type's base
/// tile (0x13a940) plus the frame, plus 0x1800 (`bright`, D1 non-zero) or 0x1000; it goes into
/// the cell's current draw list on top (0x10e596 mode 2), then A4-0x6578 = 1 and 0x102c90 with
/// D0 = 1; the draw lists rebuilt (0x10e81e), A4-0x6578 = 1 and 0x102c90 again. The original
/// shows the flash for as long as its second render takes; the port records both windows as
/// frames (the hit flash's way), each after its time ([`crate::combat::present::ACTOR_FLASH_MS`]).
/// The caller plays them ([`crate::combat::play_effects`]) where the original draws, before
/// anything else is drawn or awaited: the async callers right after the call (the spells
/// wrapper `magic::spells::flash_actor`, Ready / Unready, Pickpocket, Drop), the synchronous
/// ones at the next print or the end of the command.
pub fn flash_actor(state: &mut State, io: &Io, actor: u16, bright: bool) {
    let a = usize::from(actor);
    let (Some(&status), Some(&at), Some(&tf)) = (
        state.objects.obj_status.get(a),
        state.objects.location.get(a),
        state.objects.type_frame.get(a),
    ) else {
        return;
    };
    if status & 0x18 != 0 {
        return;
    }
    let c = state.map.centre;
    let col = (at.x & 0x3ff).wrapping_sub(c.x).wrapping_add(HALF_W as u16) & 0x3ff;
    let row = (at.y & 0x3ff).wrapping_sub(c.y).wrapping_add(HALF_H as u16) & 0x3ff;
    let base = state
        .tables
        .basetile
        .base
        .get(usize::from(tf & 0x3ff))
        .copied();
    let word = base
        .unwrap_or(0)
        .wrapping_add(tf >> 10)
        .wrapping_add(if bright { 0x1800 } else { 0x1000 });
    if usize::from(col) >= VIEW_W || usize::from(row) >= VIEW_H {
        return;
    }
    crate::objects::add_tile(state, word, col as i16, row as i16, 2);
    state.gfx.dissolve = 1;
    render_window(state, io, true);
    crate::combat::present::capture_dissolve(state, crate::combat::present::ACTOR_FLASH_MS);
    crate::objects::build_draw_lists(state);
    state.gfx.dissolve = 1;
    render_window(state, io, true);
    crate::combat::present::capture_dissolve(state, crate::combat::present::ACTOR_FLASH_MS);
}

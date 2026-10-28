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
//!
//! Spec 18, port-only ([`Io::set_sight_memory`], off by default): the remembered sight. A cell in
//! view that the window showed while it was in sight and that is out of sight now (base tile
//! 0xff, or 0x1bc unlit) is drawn as it was then, its ground and still objects
//! ([`super::cache::SightMemory`]), in the grey colours [`GREY`].., fading by the game days
//! since ([`super::cache::keeps`], [`blit_memory`]); with the switch off the render is the
//! original's.

use u6_formats::gfx::{ANIM_FRAMES, ANIM_PLANE, TILE_H, TILE_W};
use u6_render::{FLAG_MEMORY, FLAG_OVERLAY, Framebuffer, GREY, Kind, Rgb, Src, grey};

use super::blit::{blit_back, copy_window_rows, plot, set_plot_colour};
use super::cache::{BOXES, Layer, Shown, SightMemory, WindowKey, cell_box, keeps};
use super::tables::GameTables;
use super::{GfxState, anim, cache, motion, seams};
use crate::actors::Calendar;
use crate::engine::Io;
use crate::geom::{HALF_H, HALF_W, MAP_X, MAP_Y, SHOWN, VIEW_H, VIEW_W};
use crate::state::State;

/// Cell side in pixels (`asl.w #4`).
const CELL: u16 = 16;
// The cells' boxes are at multiples of 16 from the frame's corner ([`super::cache`]), and the
// blitter takes x down to a multiple of 8.
const _: () = assert!(
    MAP_X.is_multiple_of(CELL) && MAP_Y.is_multiple_of(CELL),
    "the map's corner on a cell boundary"
);
/// Object type whose tile gets clock hands (0x13aa7e = basetile 0x13a940 + 2·0x9f).
pub(super) const CLOCK_TYPE: usize = 0x9f;

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
/// ([`crate::ui::pointer::vblank`]). Port (spec 18): with the remembered sight on (`memory`),
/// entry [`GREY`] + i takes [`grey`] of colour i too.
fn load_palette(state: &State, fb: &mut Framebuffer, memory: bool) {
    for (dst, c) in fb.palette.iter_mut().zip(&state.tables.palette) {
        *dst = Rgb::new(c.r, c.g, c.b);
    }
    if memory {
        let (game, rest) = fb.palette.split_at_mut(usize::from(GREY));
        for (dst, &c) in rest.iter_mut().zip(game.iter()) {
            *dst = grey(c);
        }
    }
}

/// Spec 18: draws the remembered layer `layer` at (x, y), the top left of a cell, in grey, `fade`
/// whole days after its last sight: each opaque pixel of its pixels (its record's, or the copy of
/// them it keeps: [`Layer::image`]; of the first 16 rows) that the fade keeps ([`keeps`]) takes
/// their colour plus [`GREY`]; with masktype bit 2, the record's overlay at the frame kept ORs
/// colour bit 3 into the pixels under it that the fade keeps (rows within the cell). A pixel the
/// fade does not keep is not written. The sources are the blitter's (0x153008, [`blit_back`])
/// with [`FLAG_MEMORY`]. Every write stays in the cell's box; a map cell is never in the UI's
/// columns in Wide, so no write is dropped there. The one draw of a remembered layer: the window
/// render's into the back buffer, and (spec 20) a slide's at any pixel of its canvas
/// ([`motion::Canvas`]).
pub(super) fn blit_memory(
    gfx: &GfxState,
    fb: &mut impl motion::Canvas,
    layer: Layer,
    (x, y): (i32, i32),
    fade: u8,
) {
    let r = usize::from(layer.record);
    let (Some((pixels, transparent, id, ver)), Some(overlay)) =
        (layer.image(gfx), gfx.overlays.get(r))
    else {
        return;
    };
    let src = |c: usize, row: usize, flags: u8| Src {
        ver,
        id,
        u: c as u16,
        v: row as u16,
        kind: Kind::Tile,
        flags: flags | FLAG_MEMORY,
    };
    let at = |c: usize, row: usize| (x + c as i32, y + row as i32);
    let pixels = pixels.iter().zip(transparent).take(TILE_W * TILE_H);
    for (i, (&p, &clear)) in pixels.enumerate() {
        let (row, c) = (i / TILE_W, i % TILE_W);
        if !clear && keeps(fade, c, row) {
            fb.put(at(c, row), src(c, row, 0), |_| p | GREY);
        }
    }
    if layer.masktype & 4 != 0
        && let Some(ov) = overlay
    {
        let rows = &ov.frames[usize::from(layer.frame) % ANIM_FRAMES];
        for (k, bits) in rows.iter().enumerate() {
            let row = usize::from(ov.first_row) + k;
            if row >= TILE_H {
                break;
            }
            let drawn = |&c: &usize| bits & (0x8000 >> c) != 0 && keeps(fade, c, row);
            for c in (0..TILE_W).filter(drawn) {
                fb.put(at(c, row), src(c, row, FLAG_OVERLAY), |p| {
                    p | 1 << ANIM_PLANE
                });
            }
        }
    }
}

/// Spec 18: draws a remembered cell at (x, y) as the window render does: its own tile (black:
/// 0xff or 0x1bc) under the pixels the fade leaves out, then its remembered layers in grey
/// ([`blit_memory`]).
fn draw_remembered(
    gfx: &GfxState,
    fb: &mut Framebuffer,
    tile: u16,
    at: (u16, u16),
    shown: (u8, &[Layer]),
) {
    let (fade, layers) = shown;
    blit_back(gfx, fb, tile, at.0, at.1);
    for &layer in layers {
        blit_memory(gfx, fb, layer, (i32::from(at.0), i32::from(at.1)), fade);
    }
}

/// Spec 18: draws view cell (col, row)'s remembered sight into `back` ([`draw_remembered`]), what
/// the window render shows there now ([`SightMemory::shown`]); false, drawing nothing, when it
/// shows none (a cell outside the view included). For the flights' redraw over the missiles
/// (0x1325e4, [`crate::combat::present::redraw_over`]), which draws darkness there otherwise; the
/// caller checks the switch ([`Io::sight_memory`]).
pub(crate) fn blit_remembered(state: &State, back: &mut Framebuffer, col: i16, row: i16) -> bool {
    let (Ok(col), Ok(row)) = (usize::try_from(col), usize::try_from(row)) else {
        return false;
    };
    if col >= VIEW_W || row >= VIEW_H {
        return false;
    }
    let base = state.objects.draw.base[row * VIEW_W + col];
    let Some(shown) = state.gfx.cache.memory.shown(state, (col, row), base) else {
        return false;
    };
    draw_remembered(&state.gfx, back, base, cell_pixel(col, row), shown);
    true
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
    let snap = draw_window(state, io, framed, &inputs);
    if let Some(snap) = snap {
        keep_scene(state, io, inputs, snap, false);
    }
}

/// What 0x102c90 takes before it draws: the window contents, the clock's tile word, and with
/// `framed` the target cursor (whose call 0x106932 may redraw the panel cursor). The plot
/// colour is set first. Port (spec 18): with the remembered sight on, per box the layers drawn
/// in grey ([`SightMemory::layers`], which with `framed` remembers what this render shows);
/// empty with it off.
struct WindowInputs {
    window: ViewWindow,
    clock: Option<u16>,
    cursor: Option<(u16, u16, u16)>,
    memory: Vec<Shown>,
}

fn window_inputs(state: &mut State, io: &Io, framed: bool) -> WindowInputs {
    let colour = state.gfx.tables.window_colour;
    set_plot_colour(&mut state.gfx, colour);
    let window = seams::view_window(state);
    let memory = if io.sight_memory() {
        SightMemory::layers(state, &window, framed)
    } else {
        Vec::new()
    };
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
        memory,
    }
}

/// The rest of 0x102c90: the palettes, the cells into the back buffer, the sparkle, with
/// `framed` the cursor, the border and the present. Port (spec 20): framed, whether its scene can
/// slide for smooth motion ([`keep_scene`]): not dissolved, nor with the sparkle's XOR plots.
fn draw_window(state: &mut State, io: &Io, framed: bool, inputs: &WindowInputs) -> Option<bool> {
    let memory = io.sight_memory();
    io.draw(|fb| load_palette(state, fb, memory));
    io.draw_back(|fb| load_palette(state, fb, memory));
    let sparkle = state.gfx.sparkle != 0;
    draw_boxes(state, io, framed, inputs, None);
    if !framed {
        return None;
    }
    let gfx = &mut state.gfx;
    let dissolve = gfx.dissolve != 0;
    seams::present_window(io, gfx, dissolve);
    if dissolve {
        gfx.dissolve = 0;
    }
    Some(dissolve || sparkle)
}

/// Port (spec 20): the framed render of `inputs` just presented keeps its scene for smooth motion
/// ([`motion::record`]), `snap` when it cannot slide; `same` when the window cache drew no box.
fn keep_scene(state: &State, io: &Io, inputs: WindowInputs, snap: bool, same: bool) {
    let cursor = (inputs.cursor, inputs.clock);
    motion::record(state, io, inputs.memory, cursor, snap, same);
}

/// The back-buffer draws of 0x102c90: the cells (rows, then columns), the sparkle, with `framed`
/// the cursor and the border. With `only`, the draws in the boxes it marks, in the same order
/// (no sparkle then: [`render_window_cached`]). A cell with remembered layers draws them in grey
/// in place of its tile, then its objects as the original (spec 18).
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
        memory,
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
                match memory
                    .get(row * VIEW_W + col)
                    .filter(|m| !m.layers.is_empty())
                {
                    Some(m) => draw_remembered(gfx, fb, tile, (x, y), (m.fade, &m.layers)),
                    None => blit_back(gfx, fb, tile, x, y),
                }
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
    let key = WindowKey::of(
        state,
        &inputs.window,
        &inputs.memory,
        inputs.clock,
        inputs.cursor,
        &border,
    );
    let dirty = key
        .as_ref()
        .and_then(|key| cache::dirty_boxes(&state.gfx, io.back_mark(), key));
    match (&key, dirty) {
        (Some(key), Some(dirty)) => {
            let memory = io.sight_memory();
            io.draw(|fb| load_palette(state, fb, memory));
            io.draw_back(|fb| load_palette(state, fb, memory));
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
            let gfx = &mut state.gfx;
            if redrawn == 0 {
                gfx.cache.window_skips += 1;
            } else {
                gfx.cache.window_partial += 1;
                gfx.cache.boxes_redrawn += redrawn as u64;
            }
            keep_scene(state, io, inputs, false, redrawn == 0);
        }
        _ => {
            if let Some(snap) = draw_window(state, io, true, &inputs) {
                keep_scene(state, io, inputs, snap, false);
            }
        }
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

/// Spec 18 on the user's game (skipped without the zip; counts only are printed): the remembered
/// sight after the party's steps, with the real sight flood and draw lists. Each case is found by
/// a search over places round the start and the eight directions (what the user's map holds
/// there is not written here).
#[cfg(test)]
mod sight_memory_tests {
    #![allow(clippy::disallowed_methods)] // tests may read files

    use std::path::Path;

    use u6_render::{FLAG_MEMORY, Framebuffer, GREY, Kind, Rgb};
    use u6_testkit::{Quiet, assert_same};

    use super::cell_pixel;
    use crate::actors::party::{StepResult, player_step};
    use crate::engine::{Engine, Io};
    use crate::files::Files;
    use crate::geom::{HALF_H, HALF_W, VIEW_H, VIEW_W};
    use crate::gfx::blit::blit_back;
    use crate::state::State;

    const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

    fn files() -> Option<Files> {
        if !Path::new(ZIP).exists() {
            eprintln!("skipped: {ZIP} not present");
            return None;
        }
        let data = std::fs::read(ZIP).quiet();
        let r = u6_intake::intake(vec![u6_intake::InputFile {
            name: "ultima-vi.zip".into(),
            data,
        }]);
        Some(Files::new(r.files))
    }

    /// The start: the new game after load state's tail (the region, the window, the view).
    fn start(files: &Files) -> State {
        let mut s = State::new_game(files).quiet();
        crate::main_loop::load_state_tail(&mut s);
        s
    }

    /// An engine for its `Io` (the renders are called directly).
    fn engine() -> Engine {
        Engine::with_game(State::empty(), Files::default(), |_, _| async {})
    }

    type Cell = (usize, usize);
    type World = (u16, u16);

    /// The world cell view cell `at` shows round the view centre.
    fn world(s: &State, (col, row): Cell) -> World {
        let (c, m) = (s.map.centre, s.map.world_mask);
        let x = c.x.wrapping_add(col as u16).wrapping_sub(HALF_W as u16);
        let y = c.y.wrapping_add(row as u16).wrapping_sub(HALF_H as u16);
        (x & m, y & m)
    }

    /// The view cell that shows world cell `w`, if one does.
    fn cell_of(s: &State, w: World) -> Option<Cell> {
        cells().find(|&at| world(s, at) == w)
    }

    fn cells() -> impl Iterator<Item = Cell> {
        (0..VIEW_H).flat_map(|row| (0..VIEW_W).map(move |col| (col, row)))
    }

    /// Not on the view's edge, where the border covers half the cell.
    fn inner((col, row): Cell) -> bool {
        (1..VIEW_W - 1).contains(&col) && (1..VIEW_H - 1).contains(&row)
    }

    fn base(s: &State, (col, row): Cell) -> u16 {
        s.objects.draw.base[row * VIEW_W + col]
    }

    /// In sight and lit: its base tile neither 0xff nor 0x1bc.
    fn in_sight(s: &State, at: Cell) -> bool {
        !matches!(base(s, at), 0xff | 0x1bc)
    }

    /// The nodes of the cell's draw list, each with its still tag.
    fn nodes(s: &State, (col, row): Cell) -> Vec<(u16, bool)> {
        let d = &s.objects.draw;
        let mut out = Vec::new();
        let mut n = d.cell[row * VIEW_W + col];
        while n != 0 && out.len() < crate::objects::DRAW_NODES {
            let i = usize::from(n);
            out.push((d.node_tile[i], d.node_still[i]));
            n = d.node_next[i];
        }
        out
    }

    /// A step of the leader in direction `d` (0 north, clockwise) and the view rebuilt, as the
    /// refresh after a step does; whether the leader moved.
    fn walk(s: &mut State, d: u16) -> bool {
        let l = usize::from(s.actors.party.leader().quiet().0);
        s.actors.movement_points[l] = 30;
        let moved = player_step(s, d) == StepResult::Moved;
        crate::map::view::update_view(s);
        moved
    }

    fn render(s: &mut State, io: &Io) {
        super::render_window(s, io, true);
    }

    /// The back buffer's 16 × 16 pixels of view cell `at`.
    fn box_of(io: &Io, at: Cell) -> Vec<u8> {
        let (x, y) = cell_pixel(at.0, at.1);
        let (x, y) = (usize::from(x), usize::from(y));
        io.peek_back(|b| {
            (0..256)
                .map(|i| b.get(x + i % 16, y + i / 16).quiet())
                .collect()
        })
    }

    /// [`box_of`] after a render of `s` with the remembered sight off, then on again (an off
    /// render neither fills nor drops the memory).
    fn box_off(s: &mut State, io: &Io, at: Cell) -> Vec<u8> {
        io.set_sight_memory(false);
        render(s, io);
        io.set_sight_memory(true);
        box_of(io, at)
    }

    /// Where the searches look, in this order: the start, then the party teleported (0x1092fc,
    /// without drawing) to the cells 8, 12, .. 60 east, west, south, north and diagonally off the
    /// start cell, on its level. (The start's room is lit and has nothing in it that hides a cell
    /// from a step.)
    fn places(start: &State) -> impl Iterator<Item = State> + '_ {
        places_at(start, (8..64).step_by(4))
    }

    /// [`places`] at the distances `radii` off the start cell.
    fn places_at<'a>(
        start: &'a State,
        radii: impl Iterator<Item = i16> + 'a,
    ) -> impl Iterator<Item = State> + 'a {
        const WAYS: [(i16, i16); 8] = [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
        ];
        let off = radii.flat_map(|r| WAYS.map(|(x, y)| (x * r, y * r)));
        std::iter::once(start.clone()).chain(off.map(move |(dx, dy)| {
            let mut s = start.clone();
            let c = s.map.centre;
            let (x, y) = (c.x.wrapping_add_signed(dx), c.y.wrapping_add_signed(dy));
            crate::map::travel::teleport(&mut s, x, y, u16::from(c.z), 0);
            crate::map::view::update_view(&mut s);
            s
        }))
    }

    /// Every place ([`places`]), direction and cell whose step takes the cell out of sight that
    /// was in sight, `before` holding for its view cell before the step and `after` after it, in
    /// that order: the place, the direction, the world cell and its view cells before and after.
    fn hiding_steps<'a>(
        start: &'a State,
        before: impl Fn(&State, Cell) -> bool + 'a,
        after: impl Fn(&State, Cell) -> bool + 'a,
    ) -> impl Iterator<Item = (State, u16, World, Cell, Cell)> + 'a {
        hiding_steps_in(places(start), before, after)
    }

    /// [`hiding_steps`] over the places `places`.
    fn hiding_steps_in<'a>(
        places: impl Iterator<Item = State> + 'a,
        before: impl Fn(&State, Cell) -> bool + 'a,
        after: impl Fn(&State, Cell) -> bool + 'a,
    ) -> impl Iterator<Item = (State, u16, World, Cell, Cell)> + 'a {
        places.flat_map(move |place| {
            let seen: Vec<(World, Cell)> = cells()
                .filter(|&at| in_sight(&place, at) && before(&place, at))
                .map(|at| (world(&place, at), at))
                .collect();
            let mut found = Vec::new();
            for d in 0..8u16 {
                let mut s = place.clone();
                if !walk(&mut s, d) {
                    continue;
                }
                for &(w, at0) in &seen {
                    let hidden = |at: &Cell| !in_sight(&s, *at) && after(&s, *at);
                    if let Some(at1) = cell_of(&s, w).filter(hidden) {
                        found.push((d, w, at0, at1));
                    }
                }
            }
            found
                .into_iter()
                .map(move |(d, w, at0, at1)| (place.clone(), d, w, at0, at1))
        })
    }

    /// The first of [`hiding_steps`].
    fn hiding_step(
        start: &State,
        before: impl Fn(&State, Cell) -> bool,
        after: impl Fn(&State, Cell) -> bool,
    ) -> Option<(State, u16, World, Cell, Cell)> {
        hiding_steps(start, before, after).next()
    }

    /// Spec 18: a cell in sight that a step takes out of sight shows, in the grey colours, what
    /// it showed before with no actor on it: its pixels those of the render before the actor
    /// came, each plus [`GREY`], named the records with [`FLAG_MEMORY`]. The actor stood there
    /// in sight at the render before the step.
    #[test]
    fn real_a_cell_a_step_hides_shows_its_ground_and_still_objects_in_grey_and_no_actor() {
        let Some(files) = files() else { return };
        let start = start(&files);
        let clock = start.tables.basetile.base.get(0x9f).copied();
        // Its ground and at least one still object, nothing else (no clock: its hands are drawn
        // from the time, not remembered).
        let plain = |s: &State, at: Cell| {
            let nodes = nodes(s, at);
            inner(at)
                && !nodes.is_empty()
                && Some(base(s, at) & 0x7ff) != clock
                && nodes
                    .iter()
                    .all(|&(w, still)| still && Some(w & 0x7ff) != clock)
        };
        let (place, d, h, at0, at1) = hiding_step(&start, plain, |_, at| inner(at))
            .expect("a step takes a cell with a still object out of sight");
        let e = engine();
        let io = e.io();
        io.set_sight_memory(true);
        io.enable_src_plane();
        let mut s = place;
        render(&mut s, io);
        let reference = box_of(io, at0);
        // An actor onto the cell, in sight and drawn there.
        let z = u16::from(s.map.centre.z);
        let party = s.actors.party.members[..usize::from(s.actors.party.size)].to_vec();
        let mut placed = false;
        for a in 2..0xe0u8 {
            if party.contains(&a) || s.objects.type_frame[usize::from(a)] == 0 {
                continue;
            }
            let mut t = s.clone();
            crate::objects::move_to(&mut t, i16::from(a), h.0, h.1, z);
            crate::map::view::update_view(&mut t);
            if in_sight(&t, at0) && nodes(&t, at0).iter().any(|&(_, still)| !still) {
                s = t;
                placed = true;
                break;
            }
        }
        assert!(placed, "an actor stands on the cell in sight");
        render(&mut s, io);
        assert!(box_of(io, at0) != reference, "the actor shows on the cell");
        assert!(walk(&mut s, d), "the step");
        assert!(cell_of(&s, h) == Some(at1) && !in_sight(&s, at1));
        render(&mut s, io);
        let want: Vec<u8> = reference.iter().map(|&p| p | GREY).collect();
        assert_same!(
            box_of(io, at1),
            want,
            "the hidden cell: its ground and still objects in grey"
        );
        let (x, y) = cell_pixel(at1.0, at1.1);
        let flagged = io.peek_back(|b| {
            (0..256)
                .filter_map(|i| b.src_at(usize::from(x) + i % 16, usize::from(y) + i / 16))
                .filter(|src| src.kind == Kind::Tile && src.flags & FLAG_MEMORY != 0)
                .count()
        });
        assert!(
            flagged == 256,
            "every pixel named a record of the remembered sight"
        );
        println!("{} cells remembered", s.gfx.cache.memory.len());
    }

    /// A world cell in sight at `place` that a step `d` takes out of sight, a second step `d` off
    /// the view and a step back into the view again, out of sight.
    fn two_steps_and_back(place: &State, d: u16) -> Option<World> {
        let mut s = place.clone();
        let seen: Vec<World> = cells()
            .filter(|&at| in_sight(&s, at))
            .map(|at| world(&s, at))
            .collect();
        let hidden_after = |s: &State, w: World| cell_of(s, w).is_some_and(|at| !in_sight(s, at));
        if !walk(&mut s, d) {
            return None;
        }
        let hidden: Vec<World> = seen.into_iter().filter(|&w| hidden_after(&s, w)).collect();
        if !walk(&mut s, d) {
            return None;
        }
        let gone: Vec<World> = hidden
            .into_iter()
            .filter(|&w| cell_of(&s, w).is_none())
            .collect();
        if !walk(&mut s, (d + 4) % 8) {
            return None;
        }
        gone.into_iter().find(|&w| hidden_after(&s, w))
    }

    /// Spec 18 (the owner's follow-up: "한번이라도 본 지형은 계속 기억"): a cell out of sight that
    /// leaves the view stays remembered. Two steps take it out of sight, then off the view; one
    /// step back brings it into the view again, still out of sight: it shows the grey look it
    /// had before it left (the same game time: no fade), not black.
    #[test]
    fn real_a_cell_that_leaves_the_view_shows_grey_after_one_step_back() {
        let Some(files) = files() else { return };
        let start = start(&files);
        let found = places(&start).find_map(|place| {
            (0..8u16).find_map(|d| two_steps_and_back(&place, d).map(|x| (place.clone(), d, x)))
        });
        let (place, d, x) = found.expect("two steps take a cell out of sight, then off the view");
        let e = engine();
        let io = e.io();
        io.set_sight_memory(true);
        let mut s = place;
        render(&mut s, io);
        assert!(walk(&mut s, d));
        render(&mut s, io);
        let at = cell_of(&s, x).quiet();
        let grey = box_of(io, at);
        assert!(
            grey != box_off(&mut s, io, at),
            "out of sight in the view: grey"
        );
        assert!(walk(&mut s, d));
        render(&mut s, io);
        assert!(cell_of(&s, x).is_none(), "off the view");
        assert!(
            s.gfx.cache.memory.holds(x.0, x.1),
            "off the view: remembered"
        );
        assert!(walk(&mut s, (d + 4) % 8));
        render(&mut s, io);
        let at = cell_of(&s, x).quiet();
        assert!(!in_sight(&s, at));
        assert_same!(box_of(io, at), grey, "back in the view: its grey look");
    }

    /// Spec 18 (the owner's follow-up): the party teleported (0x1092fc) to the level below and
    /// back: the surface cell out of sight is remembered all along and shows its grey look again.
    #[test]
    fn real_another_level_keeps_the_remembered_sight() {
        let Some(files) = files() else { return };
        let start = start(&files);
        let (place, d, h, _, at1) = hiding_step(&start, |_, _| true, |_, at| inner(at))
            .expect("a step takes a cell out of sight");
        let e = engine();
        let io = e.io();
        io.set_sight_memory(true);
        let mut s = place;
        render(&mut s, io);
        assert!(walk(&mut s, d));
        render(&mut s, io);
        let grey = box_of(io, at1);
        assert!(grey != box_off(&mut s, io, at1), "grey on its level");
        let (c, z) = (s.map.centre, s.map.centre.z);
        let (x, y) = (
            (c.x & 7) + ((c.x >> 2) & 0xf8),
            (c.y & 7) + ((c.y >> 2) & 0xf8),
        );
        crate::map::travel::teleport(&mut s, x, y, u16::from(z) + 1, 0);
        render(&mut s, io);
        assert!(s.map.centre.z != z, "another level");
        assert!(
            s.gfx.cache.memory.holds_on(z, h.0, h.1),
            "another level: still remembered"
        );
        crate::map::travel::teleport(&mut s, c.x, c.y, u16::from(z), 0);
        render(&mut s, io);
        let at = cell_of(&s, h).quiet();
        assert!(
            at == at1 && !in_sight(&s, at),
            "back where it stood, the cell out of sight"
        );
        assert_same!(box_of(io, at), grey, "back on its level: its grey look");
    }

    /// Spec 18 (the owner's follow-up): the window reset (0x133a96, which the full redraw 0x133b0e
    /// dissolves in before the view is rebuilt: the party's darkness, an arrest, the healer)
    /// keeps the remembered sight: the reset window shows the remembered cell's grey look, and so
    /// does the view rebuilt after it. A restore 0x1052c8 (its load state 0x105350) forgets it.
    #[test]
    fn real_the_window_reset_keeps_the_remembered_sight_and_load_state_forgets_it() {
        let Some(files) = files() else { return };
        let start = start(&files);
        let (place, d, h, _, at1) = hiding_step(&start, |_, _| true, |_, at| inner(at))
            .expect("a step takes a cell out of sight");
        let e = Engine::with_game(State::empty(), files.clone(), |_, _| async {});
        let io = e.io();
        io.set_sight_memory(true);
        let mut s = place;
        render(&mut s, io);
        assert!(walk(&mut s, d));
        render(&mut s, io);
        let grey = box_of(io, at1);
        assert!(grey != box_off(&mut s, io, at1), "grey before the reset");
        let n = s.gfx.cache.memory.len();
        crate::ui::effects::reset_window(&mut s);
        render(&mut s, io);
        assert!(s.gfx.cache.memory.len() == n, "nothing forgotten");
        assert_same!(box_of(io, at1), grey, "the reset window: its grey look");
        crate::map::view::update_view(&mut s);
        render(&mut s, io);
        assert!(s.gfx.cache.memory.holds(h.0, h.1), "still remembered");
        assert_same!(box_of(io, at1), grey, "the view rebuilt: its grey look");
        crate::saveload::restore(&mut s, io).quiet();
        assert!(
            s.gfx.cache.memory.len() == 0,
            "a restore (load state): forgotten"
        );
    }

    /// The calendar `n` whole days after `cal` (the original's: days 1..=28, months 1..=12).
    fn days_after(cal: crate::actors::Calendar, n: u8) -> crate::actors::Calendar {
        let mut c = cal;
        for _ in 0..n {
            c.day += 1;
            if c.day > 28 {
                c.day = 1;
                c.month += 1;
                if c.month > 12 {
                    c.month = 1;
                    c.year += 1;
                }
            }
        }
        c
    }

    /// The cell `at`'s pixels the remembered sight drew in grey (index past the game's 32), as a
    /// 256-flag mask.
    fn grey_mask(io: &Io, at: Cell) -> Vec<bool> {
        box_of(io, at).iter().map(|&p| p >= GREY).collect()
    }

    /// Spec 18 (the owner's follow-up: fading by game days): a cell of plain ground with a fully
    /// opaque record, out of sight after a step, the calendar set 1, 2, 3, 4 and 5 whole days
    /// later (across a month's and a year's end: the last day of the year at the sight): its
    /// grey pixels are exactly 256, 128, 64, 32, 16 on days 0..4, each day's a subset of the day
    /// before's, and the 50 % a checkerboard; on day 5 none (its tile's black), and the sweep
    /// forgets it. A day less one minute is still the day before. The renders up to day 5 go
    /// through the window cache ([`super::render_window_cached`], each after the one before with
    /// nothing else drawn in between): only the fade changes the box's draw, so a key without it
    /// would leave the box as it was.
    #[test]
    fn real_a_remembered_cell_fades_by_whole_game_days() {
        let Some(files) = files() else { return };
        let start = start(&files);
        let opaque = |s: &State, at: Cell| {
            let t = usize::from(base(s, at) & 0x7ff);
            let rec = &s.gfx.records[usize::from(s.gfx.record_of[t])];
            inner(at) && nodes(s, at).is_empty() && rec.transparent.iter().all(|&c| !c)
        };
        let (place, d, h, _, at1) = hiding_step(&start, opaque, |_, at| inner(at))
            .expect("a step hides a cell of opaque ground");
        let e = engine();
        let io = e.io();
        io.set_sight_memory(true);
        let mut s = place;
        let year_end = crate::actors::Calendar {
            minute: 30,
            hour: 23,
            day: 28,
            month: 12,
            year: s.actors.calendar.year,
        };
        s.actors.calendar = year_end;
        let cached = |s: &mut State| super::render_window_cached(s, io);
        cached(&mut s);
        assert!(walk(&mut s, d));
        cached(&mut s);
        let mut before = grey_mask(io, at1);
        assert!(before.iter().all(|&g| g), "day 0: every pixel");
        let (skips, partial) = (s.gfx.cache.window_skips, s.gfx.cache.window_partial);
        for day in 1..=4u8 {
            let mut almost = days_after(year_end, day);
            almost.minute -= 1;
            s.actors.calendar = almost;
            cached(&mut s);
            assert!(
                grey_mask(io, at1) == before,
                "day {day} less a minute: the day before"
            );
            s.actors.calendar = days_after(year_end, day);
            cached(&mut s);
            let now = grey_mask(io, at1);
            let n = now.iter().filter(|&&g| g).count();
            assert_eq!(n, 256 >> day, "day {day}: kept pixels");
            assert!(
                now.iter().zip(&before).all(|(&a, &b)| !a || b),
                "day {day}: a subset"
            );
            if day == 1 {
                let checker = (0..256).all(|i| now[i] == ((i % 16 + i / 16) % 2 == 0));
                assert!(checker, "day 1: a checkerboard");
            }
            before = now;
        }
        let c = &s.gfx.cache;
        assert!(
            c.window_skips + c.window_partial > skips + partial,
            "the cache drew the days (whole renders left out, or boxes of them)"
        );
        s.actors.calendar = days_after(year_end, 5);
        cached(&mut s);
        assert!(grey_mask(io, at1).iter().all(|&g| !g), "day 5: none");
        assert_same!(box_of(io, at1), box_off(&mut s, io, at1), "day 5: black");
        assert!(!s.gfx.cache.memory.holds(h.0, h.1), "day 5: forgotten");
    }

    /// Spec 18: the age in minutes counts the original clock's rollover (0x10368a: 60 minutes, 24
    /// hours, days 1..=28, months 1..=12): one minute past the year's last, by the clock itself,
    /// is one minute later; a whole day across a month's end is 1440.
    #[test]
    fn the_age_counts_the_calendars_rollover() {
        use crate::actors::Calendar;
        use crate::gfx::cache::minutes;
        let mut s = State::empty();
        let end = Calendar {
            minute: 59,
            hour: 23,
            day: 28,
            month: 12,
            year: 161,
        };
        s.actors.calendar = end;
        crate::actors::clock::tick(&mut s, 1);
        let next = s.actors.calendar;
        assert!(
            (next.day, next.month, next.year) == (1, 1, 162),
            "the clock rolled over"
        );
        assert_eq!(minutes(&next) - minutes(&end), 1);
        let month_end = Calendar { month: 5, ..end };
        assert_eq!(
            minutes(&days_after(month_end, 1)) - minutes(&month_end),
            1440
        );
    }

    /// Spec 18: a cell at 50 % (one game day old) in the HD frame at ×2: each pixel the fade keeps
    /// shows grey sub-pixels and names the remembered sight, each other pixel is black in both
    /// frames (its tile's: the classic black and its HD block).
    #[test]
    fn real_a_half_faded_cell_in_hd_is_grey_on_kept_pixels_and_black_elsewhere() {
        let Some(files) = files() else { return };
        let start = start(&files);
        let opaque = |s: &State, at: Cell| {
            let t = usize::from(base(s, at) & 0x7ff);
            let rec = &s.gfx.records[usize::from(s.gfx.record_of[t])];
            inner(at) && nodes(s, at).is_empty() && rec.transparent.iter().all(|&c| !c)
        };
        let (place, d, _, _, at1) = hiding_step(&start, opaque, |_, at| inner(at))
            .expect("a step hides a cell of opaque ground");
        let e = engine();
        let io = e.io();
        io.set_sight_memory(true);
        io.enable_src_plane();
        let mut s = place;
        let day0 = s.actors.calendar;
        render(&mut s, io);
        assert!(walk(&mut s, d));
        s.actors.calendar = days_after(day0, 1);
        render(&mut s, io);
        let mut c = u6_render::hd::Compositor::new();
        let hd = hd_box(&mut c, &s, io, at1);
        let (x0, y0) = cell_pixel(at1.0, at1.1);
        let (x0, y0) = (usize::from(x0), usize::from(y0));
        let (mut kept, mut black) = (0, 0);
        for i in 0..256 {
            let (u, v) = (i % 16, i / 16);
            let (p, src) = io.draw(|f| (f.get(x0 + u, y0 + v).quiet(), f.src_at(x0 + u, y0 + v)));
            let subs: Vec<[u8; 4]> = (0..4)
                .map(|k| hd[(v * 2 + k / 2) * 32 + u * 2 + k % 2].to_le_bytes())
                .collect();
            let memory = src.is_some_and(|s| s.flags & u6_render::FLAG_MEMORY != 0);
            if crate::gfx::cache::keeps(1, u, v) {
                assert!(
                    p >= GREY && memory,
                    "a kept pixel: grey, the remembered sight"
                );
                assert!(
                    subs.iter().all(|w| w[0] == w[1] && w[1] == w[2]),
                    "kept: grey in HD"
                );
                kept += 1;
            } else {
                let rgb = io.draw(|f| f.palette[usize::from(p)]);
                assert!(
                    !memory && (rgb.r, rgb.g, rgb.b) == (0, 0, 0),
                    "a left-out pixel: black"
                );
                assert!(
                    subs.iter().all(|w| w[..3] == [0, 0, 0]),
                    "left out: black in HD"
                );
                black += 1;
            }
        }
        assert_eq!((kept, black), (128, 128));
    }

    /// Spec 18: with the remembered sight off (the engine's default) nothing is remembered, a
    /// cell a step takes out of sight is the original's tile 0xff, no pixel takes a grey colour
    /// and the palette's entries past the game's 32 are untouched.
    #[test]
    fn real_with_the_switch_off_a_hidden_cell_is_the_originals_black() {
        let Some(files) = files() else { return };
        let start = start(&files);
        let (place, d, _, _, at1) = hiding_step(&start, |_, _| true, |_, at| inner(at))
            .expect("a step takes a cell out of sight");
        let e = engine();
        let io = e.io();
        assert!(!io.sight_memory(), "off by default");
        let mut s = place;
        render(&mut s, io);
        assert!(walk(&mut s, d));
        render(&mut s, io);
        assert!(s.gfx.cache.memory.len() == 0, "nothing remembered");
        let (x, y) = cell_pixel(at1.0, at1.1);
        let mut black = Framebuffer::new();
        blit_back(&s.gfx, &mut black, 0xff, x, y);
        let (x, y) = (usize::from(x), usize::from(y));
        let want: Vec<u8> = (0..256)
            .map(|i| black.get(x + i % 16, y + i / 16).quiet())
            .collect();
        assert_same!(box_of(io, at1), want, "the hidden cell: tile 0xff");
        let untouched = |fb: &Framebuffer| {
            fb.pixels().iter().all(|&p| p < GREY)
                && fb.palette[usize::from(GREY)..]
                    .iter()
                    .all(|&c| c == Rgb::default())
        };
        assert!(io.peek_back(untouched), "the back buffer");
        assert!(io.draw(|fb| untouched(fb)), "the screen");
    }

    /// The game's own steps and commands, as the platforms send them, on both engines: each
    /// command of `commands` after the prompt is up, its keys one at a time, each once the game
    /// took the one before and 30 ticks passed; the frames (pixels and palette) compared after
    /// every tick.
    fn play_both(on: &mut Engine, off: &mut Engine, commands: &[&[crate::Key]]) {
        let prompt = |e: &Engine| e.state().main.reading == 1 && e.io().pending_events() == 0;
        let mut tick = |input: &[crate::InputEvent]| {
            on.tick(input, 20);
            off.tick(input, 20);
            assert_same!(on.frame().pixels(), off.frame().pixels(), "the frame");
            assert!(on.frame().palette == off.frame().palette, "the palette");
            prompt(on) && prompt(off)
        };
        for keys in commands {
            let mut ready = 0;
            for _ in 0..3000 {
                ready = if tick(&[]) { ready + 1 } else { 0 };
                if ready == 3 {
                    break;
                }
            }
            assert!(ready == 3, "the prompt");
            for &k in *keys {
                tick(&[crate::InputEvent::Key(k)]);
                for _ in 0..30 {
                    tick(&[]);
                }
            }
        }
        for _ in 0..200 {
            tick(&[]);
        }
    }

    /// Spec 06 O2 with spec 18: the game played from the start with the remembered sight on, the
    /// idle refresh with the window cache and without it: the same frames after every tick. The
    /// party walks ten cells south from the start and uses the door south of it twice (open, then
    /// close: what it showed is out of sight), so the last frames show the remembered sight; the
    /// cache left out draws (whole renders, or boxes of them).
    #[test]
    fn real_the_window_cache_draws_the_remembered_sight_as_without_it() {
        use crate::Key::{Char, Down, Enter};
        let Some(files) = files() else { return };
        let mut on = Engine::game_first(files.clone(), 0).quiet();
        let mut off = Engine::game_first(files, 0).quiet();
        off.io().set_window_cache(false);
        for e in [&on, &off] {
            e.io().set_sight_memory(true);
        }
        let start = on.state().leader_position().quiet();
        let mut commands: Vec<&[crate::Key]> = vec![&[Down]; 10];
        commands.extend([&[Char('u'), Down, Enter][..], &[Char('u'), Down, Enter]]);
        play_both(&mut on, &mut off, &commands);
        let at = on.state().leader_position().quiet();
        assert!((at.x, at.y) == (start.x, start.y + 10), "ten cells south");
        let grey = on.frame().pixels().iter().filter(|&&p| p >= GREY).count();
        let memory = on.state().gfx.cache.memory.len();
        println!("{grey} grey pixels, {memory} cells remembered");
        assert!(grey >= 256, "a cell's worth of the remembered sight shown");
        let c = &on.state().gfx.cache;
        assert!(
            c.window_skips + c.window_partial > 0,
            "the cache left out draws"
        );
    }

    /// The records the objects in `slots` draw: per slot its tile (type's base tile plus frame)
    /// and the three before it (the parts of a double-size tile, 0x10e73a).
    fn records_of(s: &State, slots: &[usize]) -> Vec<u16> {
        let mut out = Vec::new();
        for &i in slots {
            let tf = s.objects.type_frame[i];
            let base = s.tables.basetile.base[usize::from(tf & 0x3ff)];
            let tile = base.wrapping_add(tf >> 10);
            for k in 0..4 {
                let t = usize::from(tile.wrapping_sub(k) & 0x7ff);
                out.push(s.gfx.record_of[t]);
            }
        }
        out
    }

    /// A monster of `kind` made by 0x131646 at world cell `h` of `place` (for the silver serpent's
    /// head two segments 0x19d in object slots east of it, as 0x10b42e leaves them), the view
    /// rebuilt; with the slots it took.
    fn with_monster(place: &State, kind: u16, h: World) -> Option<(State, Vec<usize>)> {
        let mut s = place.clone();
        let z = u16::from(s.map.centre.z);
        let before = s.objects.type_frame.clone();
        let head = crate::objects::spawn_monster(&mut s, kind, h.0, h.1, z);
        if head < 0 {
            return None;
        }
        if kind == SERPENT {
            for k in 1..3u16 {
                let qq = k << 8 | head as u16;
                let tf = kind | 8 << 10;
                crate::objects::create_on_map(&mut s.objects, tf, 1, qq, h.0 + k, h.1, z);
            }
        }
        let made: Vec<usize> = (0..before.len())
            .filter(|&i| s.objects.type_frame[i] != before[i])
            .collect();
        crate::map::view::update_view(&mut s);
        Some((s, made))
    }

    /// Spec 18 (the review's creature ghost): a monster of `kind` and its parts (a 0x176's eight
    /// 0x1a9, a dragon's four, a tailed type's tail, the serpent's segments) on a cell in sight,
    /// found among the cells a step takes out of sight and checked again with the monster there:
    /// its pixels reach its cell and a part's cell (each box differs from the render without the
    /// monster), the step still walks and still takes its cell out of sight. No layer of the
    /// remembered sight holds a record of the monster's or its parts' tiles, before the step or
    /// after it, and its cell is remembered (without it).
    fn no_ghost(kind: u16) {
        let Some(files) = files() else { return };
        let start = start(&files);
        let mut candidates = hiding_steps(&start, |_, at| inner(at), |_, at| inner(at)).take(400);
        let checked = candidates.find_map(|(place, d, h, at0, _)| {
            let (s, made) = with_monster(&place, kind, h)?;
            let holds = |at: Cell, slots: &[usize]| {
                let records = records_of(&s, slots);
                nodes(&s, at)
                    .iter()
                    .any(|&(w, _)| records.contains(&s.gfx.record_of[usize::from(w & 0x7ff)]))
            };
            let parts: Vec<usize> = made.iter().copied().filter(|&i| i >= 0x100).collect();
            let part_cells: Vec<Cell> = cells()
                .filter(|&at| in_sight(&s, at) && holds(at, &parts))
                .collect();
            if !in_sight(&s, at0) || part_cells.is_empty() {
                return None;
            }
            let e = engine();
            let io = e.io();
            let mut without = place.clone();
            render(&mut without, io);
            let plain: Vec<Vec<u8>> = cells().map(|at| box_of(io, at)).collect();
            let mut with = s.clone();
            render(&mut with, io);
            let reaches = |at: Cell| box_of(io, at) != plain[at.1 * VIEW_W + at.0];
            if !reaches(at0) || !part_cells.iter().any(|&at| reaches(at)) {
                return None;
            }
            let mut after = s.clone();
            let moved = walk(&mut after, d);
            let hidden = cell_of(&after, h).is_some_and(|at| !in_sight(&after, at));
            let records = records_of(&s, &made);
            (moved && hidden).then_some((s, d, h, records))
        });
        let (mut s, d, h, records) =
            checked.expect("a monster in sight whose cell a step takes out of sight");
        let e = engine();
        let io = e.io();
        io.set_sight_memory(true);
        render(&mut s, io);
        let ghost = |s: &State| s.gfx.cache.memory.records().any(|r| records.contains(&r));
        assert!(!ghost(&s), "{kind:#x}: none remembered in sight");
        assert!(walk(&mut s, d), "{kind:#x}: the step");
        let at = cell_of(&s, h).quiet();
        assert!(!in_sight(&s, at), "{kind:#x}: its cell out of sight");
        render(&mut s, io);
        assert!(!ghost(&s), "{kind:#x}: none remembered out of sight");
        assert!(
            s.gfx.cache.memory.holds(h.0, h.1),
            "{kind:#x}: its cell remembered"
        );
    }

    #[test]
    fn real_no_ghost_of_a_0x176_and_its_eight_0x1a9_parts() {
        no_ghost(0x176);
    }

    #[test]
    fn real_no_ghost_of_a_dragon_and_its_parts() {
        no_ghost(0x19b);
    }

    #[test]
    fn real_no_ghost_of_a_silver_serpent_and_its_segments() {
        no_ghost(SERPENT);
    }

    #[test]
    fn real_no_ghost_of_a_tailed_monster_and_its_tail() {
        no_ghost(0x1aa);
    }

    /// The silver serpent's head and its segments (objects' `moves::SERPENT`).
    const SERPENT: u16 = 0x19d;

    /// Ambient light word A4-0x7b79 (`objlist_tail` 0x0b, [`crate::map::seams`]).
    const AMBIENT: usize = 0x0b;

    /// Spec 18 (`ssot.md`: darkness too): the remembered sight with the light gone. A place whose
    /// cells in sight at the start's daylight are in part unlit at ambient 0 (night, no light,
    /// the view rebuilt where the party stands): with darkness on ([`super::cache`]'s
    /// `MEMORY_IN_DARKNESS`, the default) such a cell shows its remembered sight, all grey; off,
    /// the original's black (0x1bc). A cell lit at night shows itself either way.
    fn darkness_case(darkness: bool) {
        let Some(files) = files() else { return };
        let start = start(&files);
        let dark_at_night = |place: &State| {
            let mut t = place.clone();
            t.actors.objlist_tail[AMBIENT] = 0;
            crate::map::view::update_view(&mut t);
            let x = cells().find(|&at| inner(at) && in_sight(place, at) && base(&t, at) == 0x1bc);
            let lit = cells().find(|&at| inner(at) && in_sight(&t, at));
            Some((x?, lit?))
        };
        let (place, (x, lit)) = places(&start)
            .find_map(|p| dark_at_night(&p).map(|c| (p, c)))
            .expect("a place partly unlit at night");
        let e = engine();
        let io = e.io();
        io.set_sight_memory(true);
        let mut s = place;
        s.gfx.cache.memory.set_darkness(darkness);
        render(&mut s, io);
        let day = box_of(io, x);
        s.actors.objlist_tail[AMBIENT] = 0;
        crate::map::view::update_view(&mut s);
        render(&mut s, io);
        let shown = box_of(io, x);
        let lit_shown = box_of(io, lit);
        let black = box_off(&mut s, io, x);
        if darkness {
            assert!(shown != black, "the unlit cell: not black");
            assert!(shown.iter().all(|&p| p >= GREY), "the unlit cell: all grey");
            assert!(shown != day, "the unlit cell: not its daylight colours");
        } else {
            assert_same!(shown, black, "the unlit cell: the original's black");
        }
        assert!(
            lit_shown.iter().all(|&p| p < GREY),
            "the lit cell: none of it grey"
        );
    }

    #[test]
    fn real_an_unlit_cell_shows_its_remembered_sight_with_darkness_on() {
        darkness_case(true);
    }

    #[test]
    fn real_an_unlit_cell_is_black_with_darkness_off() {
        darkness_case(false);
    }

    /// Spec 18: a window whose every cell is unlit (no cell in sight shows itself) that is not
    /// the window reset keeps the remembered sight: with darkness on every remembered cell shows
    /// it (all its pixels grey), off every such cell is the original's black. The window is made
    /// unlit by hand (every base 0x1bc, no object listed): ambient light of the game's range
    /// lights the leader's own cell, so the sight flood never leaves the whole view unlit.
    #[test]
    fn real_an_all_dark_window_that_is_no_reset_keeps_the_remembered_sight() {
        let Some(files) = files() else { return };
        let start = start(&files);
        for darkness in [true, false] {
            let e = engine();
            let io = e.io();
            io.set_sight_memory(true);
            let mut s = start.clone();
            s.gfx.cache.memory.set_darkness(darkness);
            render(&mut s, io);
            // The cells in sight whose ground and still objects are all they show (no clock,
            // whose hands are not remembered), with their pixels in sight.
            let clock = s.tables.basetile.base.get(0x9f).copied();
            let plain = |s: &State, at: Cell| {
                inner(at)
                    && in_sight(s, at)
                    && Some(base(s, at) & 0x7ff) != clock
                    && nodes(s, at)
                        .iter()
                        .all(|&(w, still)| still && Some(w & 0x7ff) != clock)
            };
            let lit: Vec<(Cell, Vec<u8>)> = cells()
                .filter(|&at| plain(&s, at))
                .map(|at| (at, box_of(io, at)))
                .collect();
            assert!(!lit.is_empty(), "cells in sight");
            let n = s.gfx.cache.memory.len();
            s.objects.draw.base.fill(0x1bc);
            s.objects.draw.cell.fill(0);
            render(&mut s, io);
            assert!(s.gfx.cache.memory.len() == n, "nothing forgotten");
            for (at, seen) in &lit {
                let shown = box_of(io, *at);
                if darkness {
                    let want: Vec<u8> = seen.iter().map(|&p| p | GREY).collect();
                    assert_same!(shown, want, "darkness on: the last sight in grey");
                } else {
                    assert_same!(shown, box_off(&mut s, io, *at), "darkness off: black");
                }
            }
        }
    }

    /// The flight frames [`real_a_flight_over_a_remembered_cell_keeps_it_grey_in_every_frame`]
    /// records (its missile's drawn steps).
    const FLIGHT_FRAMES: usize = 8;

    /// Spec 18 (the review's missile trail): a flight (0x132706, one missile of tile 0x22c, no
    /// flags) from a remembered cell outward. Its recorded frames replayed in order over the
    /// screen it started from (each rectangle put where it was taken, as the playback does):
    /// after each of its [`FLIGHT_FRAMES`] step frames (0x1325e4 redraws over the missile before
    /// each) the remembered cell's whole box is as the window render drew it, grey, the missile
    /// covered by it as by the original's darkness.
    #[test]
    fn real_a_flight_over_a_remembered_cell_keeps_it_grey_in_every_frame() {
        let Some(files) = files() else { return };
        let start = start(&files);
        let (place, d, h, _, at1) = hiding_step(&start, |_, at| inner(at), |_, at| inner(at))
            .expect("a step takes a cell out of sight");
        let out = std::rc::Rc::new(std::cell::RefCell::new(None));
        let keep = std::rc::Rc::clone(&out);
        let mut e = Engine::with_game(place, files, move |st, io| async move {
            let mut s = st.borrow_mut();
            io.set_sight_memory(true);
            render(&mut s, &io);
            assert!(walk(&mut s, d));
            render(&mut s, &io);
            let (x0, y0) = cell_pixel(at1.0, at1.1);
            let (x0, y0) = (usize::from(x0), usize::from(y0));
            let mut screen = io.draw(|f| f.clone());
            let cell = |f: &Framebuffer| -> Vec<u8> {
                (0..256)
                    .map(|i| f.get(x0 + i % 16, y0 + i / 16).quiet())
                    .collect()
            };
            let grey = cell(&screen);
            assert!(
                grey.iter().all(|&p| p >= GREY),
                "the cell grey before the flight"
            );
            s.combat.effects.clear();
            let (hx, hy) = (h.0 as i16, h.1 as i16);
            let (mut xs, mut ys) = ([hx + 3], [hy]);
            crate::combat::present::fly(&mut s, hx, hy, &mut xs, &mut ys, 1, 7, 0x22c, 0);
            let mut after = Vec::new();
            for f in s.combat.effects.iter() {
                for (i, &p) in f.pixels.iter().enumerate() {
                    screen.set(f.x + i % f.w, f.y + i / f.w, p);
                }
                if f.cost == crate::combat::present::FLIGHT_STEP_MS {
                    after.push(cell(&screen) == grey);
                }
            }
            *keep.borrow_mut() = Some(after);
        });
        e.tick(&[], 16);
        let after = out.borrow_mut().take().expect("the flight ran");
        println!("{} flight frames", after.len());
        assert_eq!(after.len(), FLIGHT_FRAMES, "the flight's step frames");
        assert!(
            after.iter().all(|&same| same),
            "the cell grey after every frame"
        );
    }

    /// Spec 18 (the review's in-sight identity): the same state rendered at once with the
    /// remembered sight on and off (each on an engine of its own, the pixels with it on read
    /// first): every cell in sight is pixel for pixel the same, the remembered ones differ.
    #[test]
    fn real_a_cell_in_sight_is_the_same_with_the_switch_on_and_off() {
        let Some(files) = files() else { return };
        let start = start(&files);
        let (place, d, _, _, at1) = hiding_step(&start, |_, at| inner(at), |_, at| inner(at))
            .expect("a step takes a cell out of sight");
        let (e_on, e_off) = (engine(), engine());
        let (on, off) = (e_on.io(), e_off.io());
        on.set_sight_memory(true);
        let mut s = place;
        render(&mut s, on);
        assert!(walk(&mut s, d));
        let mut t = s.clone();
        render(&mut s, on);
        let seen: Vec<(Cell, Vec<u8>)> = cells()
            .filter(|&at| in_sight(&s, at))
            .map(|at| (at, box_of(on, at)))
            .collect();
        let remembered = box_of(on, at1);
        render(&mut t, off);
        assert!(!seen.is_empty(), "cells in sight");
        for (at, pixels) in &seen {
            assert_same!(*pixels, box_of(off, *at), "a cell in sight: itself");
        }
        assert!(
            remembered != box_of(off, at1),
            "the remembered cell differs"
        );
    }

    /// The records the shore merge 0x152dac writes now (tile `shore_dst[i] / 4`'s).
    fn merged(s: &State) -> Vec<u16> {
        let g = &s.gfx;
        (g.tables.shore_dst.iter())
            .filter(|&&w| w % 4 == 0 && usize::from(w / 4) < g.record_of.len())
            .map(|&w| g.record_of[usize::from(w / 4)])
            .collect()
    }

    /// The HD frame's block of view cell `at` at ×2, the screen composed from what `s` holds.
    fn hd_box(c: &mut u6_render::hd::Compositor, s: &State, io: &Io, at: Cell) -> Vec<u32> {
        let mut cache = crate::hd::ImageCache::new();
        let mut images = cache.images(s, io);
        io.draw(|f| c.compose(f, &mut images, u6_render::hd::Scale::X2));
        let (x, y) = cell_pixel(at.0, at.1);
        let (x, y) = (usize::from(x) * 2, usize::from(y) * 2);
        let ow = u6_render::WIDTH * 2;
        (0..32 * 32)
            .map(|i| c.rgba()[(y + i / 32) * ow + x + i % 32])
            .collect()
    }

    /// A shore cell (its ground a record the shore merge 0x152dac rewrites in place) in sight by
    /// day and unlit at night, at the first place ([`places_at`], 8 to 248 cells off the start)
    /// that has one (no step hides a shore cell of the moat round the start, open water): the
    /// state rendered by day, then at night (ambient 0, no light), with the remembered sight on
    /// and the source plane, through the window cache; its view cell and world cell.
    fn shore_at_night(start: &State, io: &Io) -> (State, Cell, World) {
        let shore = |s: &State, at: Cell| {
            let t = usize::from(base(s, at) & 0x7ff);
            inner(at) && merged(s).contains(&s.gfx.record_of[t])
        };
        let dark_at_night = |place: &State| {
            let mut t = place.clone();
            t.actors.objlist_tail[AMBIENT] = 0;
            crate::map::view::update_view(&mut t);
            cells().find(|&at| shore(place, at) && base(&t, at) == 0x1bc)
        };
        let (place, at) = places_at(start, (8..256).step_by(8))
            .find_map(|p| dark_at_night(&p).map(|at| (p, at)))
            .expect("a shore cell in sight by day, unlit at night");
        io.set_sight_memory(true);
        io.enable_src_plane();
        let mut s = place;
        super::render_window_cached(&mut s, io);
        let w = world(&s, at);
        s.actors.objlist_tail[AMBIENT] = 0;
        crate::map::view::update_view(&mut s);
        super::render_window_cached(&mut s, io);
        (s, at, w)
    }

    /// Spec 18 (the owner's "안보이게 됐을때의 모습으로": the look from when it went out of
    /// sight): a shore cell (its ground a record the shore merge 0x152dac rewrites in place) seen
    /// in daylight, then no longer seen: night falls (ambient 0, no light; no step hides a shore
    /// cell of the moat round the start, open water). Then the animation runs on (0x10202e)
    /// until the merge changed that record: the cell shows the pixels of its last sight in grey,
    /// on the screen (through the window cache, and the whole render) and in the HD frame at ×2,
    /// unchanged; the record itself did change. The look is kept as one copy per record and
    /// content a remembered cell names.
    #[test]
    fn real_a_shore_cell_keeps_the_look_of_its_last_sight() {
        let Some(files) = files() else { return };
        let e = engine();
        let io = e.io();
        let (mut s, at, w) = shore_at_night(&start(&files), io);
        let r = usize::from(s.gfx.cache.memory.at(w.0, w.1).quiet()[0].record);
        let (shown, ver) = (box_of(io, at), s.gfx.cache.version(r));
        assert!(
            shown.iter().all(|&p| p >= GREY),
            "the cell remembered, grey"
        );
        let pixels = s.gfx.records[r].pixels.clone();
        let mut hd = u6_render::hd::Compositor::new();
        let hd_shown = hd_box(&mut hd, &s, io, at);
        let mut ticks = 0;
        while s.gfx.records[r].pixels == pixels && ticks < 64 {
            crate::gfx::anim::anim_tick(&mut s);
            ticks += 1;
        }
        assert!(s.gfx.cache.version(r) != ver, "the record changed");
        assert!(s.gfx.records[r].pixels != pixels, "its pixels changed");
        super::render_window_cached(&mut s, io);
        assert_same!(box_of(io, at), shown, "the screen: the last sight's look");
        let hd_now = hd_box(&mut hd, &s, io, at);
        assert_same!(hd_now, hd_shown, "the HD frame: the last sight's look");
        let fresh = hd_box(&mut u6_render::hd::Compositor::new(), &s, io, at);
        assert_same!(
            fresh,
            hd_shown,
            "a new compositor's HD frame: the last sight's look"
        );
        render(&mut s, io);
        assert_same!(
            box_of(io, at),
            shown,
            "the whole render: the last sight's look"
        );
        let copies = s.gfx.cache.memory.copies();
        println!("{ticks} animation ticks; {copies} copies of merged records held");
        assert!(copies > 0, "a copy held");
    }

    /// Spec 18 (the review's pack art): with an HD pack holding an image for a shore record,
    /// keyed by the content of the copy the remembered sight keeps of it (a synthetic picture of
    /// one colour), the remembered shore cell's pixels drawn from the copy show that picture in
    /// grey in the HD frame at ×2: the copy is the pack's image of the record it was taken from.
    #[test]
    fn real_a_remembered_shore_cell_shows_the_packs_art_in_grey() {
        use u6_hdpack::{Generator, ImageId, PackImage, Rgba, read_pack, tile_hash, write_pack};
        let Some(files) = files() else { return };
        let e = engine();
        let io = e.io();
        let (s, at, w) = shore_at_night(&start(&files), io);
        let layer = s.gfx.cache.memory.at(w.0, w.1).quiet()[0];
        let id = crate::gfx::cache::FROZEN_BASE + layer.frozen.quiet();
        let (pixels, clear) = s.gfx.cache.frozen_image(id).quiet();
        let colour = u6_render::Rgb::new(250, 20, 240);
        let mut picture = Rgba::new(64, 64);
        for p in picture.pixels.chunks_exact_mut(4) {
            p.copy_from_slice(&[colour.r, colour.g, colour.b, 255]);
        }
        let image = PackImage {
            id: ImageId::Tile(layer.record),
            hash: tile_hash(pixels, clear).quiet(),
            image: picture,
        };
        let g = Generator {
            name: "test".into(),
            version: "0".into(),
            engine: "none".into(),
            date: "2026-10-27".into(),
        };
        let pack = read_pack(&write_pack(&[image], &g, true).quiet()).quiet();
        let mut c = u6_render::hd::Compositor::new();
        c.set_pack(Some(std::rc::Rc::new(crate::hd::HdPack::new(pack))));
        let hd = hd_box(&mut c, &s, io, at);
        let (x0, y0) = cell_pixel(at.0, at.1);
        let (x0, y0) = (usize::from(x0), usize::from(y0));
        let named = io.draw(|f| {
            (0..256)
                .map(|i| {
                    f.src_at(x0 + i % 16, y0 + i / 16)
                        .is_some_and(|p| p.id == id)
                })
                .collect::<Vec<bool>>()
        });
        let want = u6_render::hd::word(u6_render::grey(colour));
        let mut from_copy = 0;
        for (i, _) in named.iter().enumerate().filter(|&(_, &n)| n) {
            let (u, v) = (i % 16, i / 16);
            for k in 0..4 {
                let sub = hd[(v * 2 + k / 2) * 32 + u * 2 + k % 2];
                assert!(
                    sub == want,
                    "a pixel drawn from the copy: the pack's picture in grey"
                );
            }
            from_copy += 1;
        }
        println!("{from_copy} pixels of the cell drawn from the copy");
        assert!(from_copy > 0, "pixels drawn from the copy");
    }

    /// Spec 18 (the review's merged-record set): a record rewritten before the cell showing it
    /// was last in sight (its version not 0; here a ground record outside the shore merge's,
    /// rewritten by hand and reported as the merge reports it) is kept as a copy: rewritten again
    /// while the cell is out of sight, the cell keeps the look of its last sight. On the user's
    /// game, the records the animation rewrites in 64 ticks are counted against the shore set.
    #[test]
    fn real_a_rewritten_record_outside_the_shore_set_keeps_its_look() {
        let Some(files) = files() else { return };
        let start = start(&files);
        let mut ticked = start.clone();
        for _ in 0..64 {
            crate::gfx::anim::anim_tick(&mut ticked);
        }
        let set = merged(&ticked);
        let rewritten = (0..ticked.gfx.records.len()).filter(|&r| ticked.gfx.cache.version(r) != 0);
        let outside = rewritten
            .clone()
            .filter(|&r| !set.contains(&(r as u16)))
            .count();
        println!(
            "64 ticks: {} records rewritten, {outside} outside the shore set",
            rewritten.count()
        );
        assert_eq!(outside, 0, "the animation rewrites only shore records");
        let ground = |s: &State, at: Cell| {
            let r = s.gfx.record_of[usize::from(base(s, at) & 0x7ff)];
            inner(at) && !merged(s).contains(&r) && nodes(s, at).is_empty()
        };
        let (place, d, h, _, at1) = hiding_step(&start, ground, |_, at| inner(at))
            .expect("a step hides a cell of plain ground");
        let e = engine();
        let io = e.io();
        io.set_sight_memory(true);
        let mut s = place;
        let at0 = cell_of(&s, h).quiet();
        let r = usize::from(s.gfx.record_of[usize::from(base(&s, at0) & 0x7ff)]);
        let rewrite = |s: &mut State| {
            for p in s.gfx.records[r].pixels.iter_mut() {
                *p ^= 1;
            }
            s.gfx.cache.record_changed(r);
        };
        rewrite(&mut s);
        super::render_window_cached(&mut s, io);
        assert!(walk(&mut s, d));
        super::render_window_cached(&mut s, io);
        let layer = s.gfx.cache.memory.at(h.0, h.1).quiet()[0];
        assert!(layer.frozen.is_some(), "kept as a copy");
        let shown = box_of(io, at1);
        rewrite(&mut s);
        super::render_window_cached(&mut s, io);
        assert_same!(box_of(io, at1), shown, "the last sight's look");
    }

    /// Spec 18 (the review's item 1: no live record in the remembered sight): the copy store full
    /// ([`crate::gfx::cache::MAX_COPIES`]), a cell of plain ground whose record was rewritten
    /// (its version not 0: kept as a copy) is seen with a copy, then its record is rewritten
    /// again while it is in sight (a new content, which the full store cannot take), then a step
    /// hides it and the record is rewritten once more: the cell is forgotten, the original's
    /// black out of sight, never the record's pixels now (the look of its last sight cannot be
    /// kept, and the older look is not its last sight). Through the window cache.
    #[test]
    fn real_a_full_copy_store_forgets_the_cell_and_never_shows_a_rewrite() {
        let Some(files) = files() else { return };
        let start = start(&files);
        let ground = |s: &State, at: Cell| {
            let r = s.gfx.record_of[usize::from(base(s, at) & 0x7ff)];
            inner(at) && !merged(s).contains(&r) && nodes(s, at).is_empty()
        };
        let (place, d, h, at0, at1) = hiding_step(&start, ground, |_, at| inner(at))
            .expect("a step hides a cell of plain ground");
        let e = engine();
        let io = e.io();
        io.set_sight_memory(true);
        let mut s = place;
        let r = usize::from(s.gfx.record_of[usize::from(base(&s, at0) & 0x7ff)]);
        let rewrite = |s: &mut State, bit: u8| {
            for p in s.gfx.records[r].pixels.iter_mut() {
                *p ^= bit;
            }
            s.gfx.cache.record_changed(r);
        };
        let cached = |s: &mut State| super::render_window_cached(s, io);
        rewrite(&mut s, 1);
        cached(&mut s);
        let layer = s.gfx.cache.memory.at(h.0, h.1).quiet()[0];
        assert!(layer.frozen.is_some(), "seen with a copy");
        s.gfx
            .cache
            .memory
            .fill_copies(crate::gfx::cache::MAX_COPIES, 0);
        rewrite(&mut s, 2);
        cached(&mut s);
        assert!(walk(&mut s, d));
        let black = box_off(&mut s, io, at1);
        cached(&mut s);
        assert_same!(box_of(io, at1), black, "out of sight: the original's black");
        rewrite(&mut s, 4);
        cached(&mut s);
        assert_same!(box_of(io, at1), black, "rewritten again: still black");
        assert!(
            !s.gfx.cache.memory.holds(h.0, h.1),
            "a new content, the store full: forgotten"
        );
    }

    /// Spec 18 (the review's faded overlay): a remembered layer drawn with its record's overlay
    /// (masktype bit 2: the overlay ORs colour bit 3 into the pixels under it), 1..4 days after
    /// its last sight, over a box that held other pixels: each pixel the fade leaves out is the
    /// cell's own black, as the window render draws the cell ([`super::draw_remembered`]) and as
    /// the flights' redraw over the missiles (0x1325e4, [`super::blit_remembered`]) does, and the
    /// overlay ORs into kept pixels only. On the user's game: the tile drawn with the overlay that
    /// covers the most pixels, at an out-of-sight cell of a place's view.
    #[test]
    fn real_a_faded_overlay_leaves_the_pixels_it_drops_black() {
        use crate::gfx::cache::{keeps, minutes};
        use u6_formats::gfx::{ANIM_PLANE, TILE_COUNT};
        let Some(files) = files() else { return };
        let start = start(&files);
        let g = &start.gfx;
        let covers = |t: usize| {
            let ov = g.overlays[usize::from(g.record_of[t])].as_ref();
            g.masktype[t] & 4 != 0 && ov.is_some_and(|ov| ov.frames[0].iter().any(|&b| b != 0))
        };
        let bits = |t: usize| {
            let ov = g.overlays[usize::from(g.record_of[t])].as_ref();
            ov.map_or(0, |ov| {
                ov.frames[0].iter().map(|b| b.count_ones()).sum::<u32>()
            })
        };
        let t = (0..TILE_COUNT)
            .filter(|&t| covers(t))
            .max_by_key(|&t| bits(t))
            .expect("a tile drawn with an overlay");
        let layer = super::Layer {
            record: g.record_of[t],
            masktype: g.masktype[t],
            frame: 0,
            frozen: None,
        };
        let (mut s, at) = places(&start)
            .find_map(|p| {
                let at = cells().find(|&at| inner(at) && base(&p, at) == 0xff)?;
                Some((p, at))
            })
            .expect("an out-of-sight cell");
        let (w, now) = (world(&s, at), minutes(&s.actors.calendar));
        let ov = s.gfx.overlays[usize::from(layer.record)].clone().quiet();
        let under = |c: usize, row: usize| {
            let k = row.wrapping_sub(usize::from(ov.first_row));
            ov.frames[0]
                .get(k)
                .is_some_and(|&bits| bits & (0x8000 >> c) != 0)
        };
        let (x0, y0) = cell_pixel(at.0, at.1);
        let (x, y) = (usize::from(x0), usize::from(y0));
        // A game colour, not the cell's black: what the box held before.
        let fill = 0x0e;
        let filled = || {
            let mut fb = Framebuffer::new();
            for i in 0..256 {
                fb.set(x + i % 16, y + i / 16, fill);
            }
            fb
        };
        let pixels = |fb: &Framebuffer| -> Vec<u8> {
            (0..256)
                .map(|i| fb.get(x + i % 16, y + i / 16).quiet())
                .collect()
        };
        let mut fb = filled();
        blit_back(&s.gfx, &mut fb, 0xff, x0, y0);
        let black = pixels(&fb);
        assert!(
            black.iter().all(|&p| p != fill),
            "the cell's own tile covers its box"
        );
        let (mut dropped, mut ored) = (0, 0);
        for day in 1..=4u8 {
            let seen = now - u64::from(day) * 1440;
            let key = (s.map.centre.z, w.0, w.1);
            s.gfx.cache.memory.remember(key, vec![layer], seen);
            let mut drawn = filled();
            super::draw_remembered(&s.gfx, &mut drawn, 0xff, (x0, y0), (day, &[layer]));
            let mut redrawn = filled();
            let col_row = (at.0 as i16, at.1 as i16);
            assert!(
                super::blit_remembered(&s, &mut redrawn, col_row.0, col_row.1),
                "the redraw over the missiles draws the cell"
            );
            for (name, fb) in [("the window render", &drawn), ("the redraw", &redrawn)] {
                let got = pixels(fb);
                for (i, &p) in got.iter().enumerate() {
                    let (c, row) = (i % 16, i / 16);
                    if !keeps(day, c, row) {
                        assert!(p == black[i], "{name}, day {day}: a dropped pixel black");
                        dropped += usize::from(under(c, row));
                    } else if under(c, row) {
                        assert!(
                            p & 1 << ANIM_PLANE != 0,
                            "{name}, day {day}: the overlay on a kept pixel"
                        );
                        ored += 1;
                    }
                }
            }
        }
        println!("{dropped} dropped pixels under the overlay, {ored} kept pixels ORed");
        assert!(dropped > 0, "the overlay covers dropped pixels");
        assert!(ored > 0, "the overlay ORs kept pixels");
    }

    /// The parity driver (spec 18 review): the same text runs on the renderer before spec 18
    /// (587d973), whose hashes are pinned. On the user's game: the start; ten steps south; the
    /// door south of the party opened and closed (its frame bit 2, as Use flips it); two steps
    /// west; the party teleported (0x1092fc) to level 1 below and back to where it stood; two
    /// steps east. After
    /// each, the view rebuilt (0x109e74) and the window rendered framed (0x102c90). Per render,
    /// the FNV-1a hash of the screen's pixels with its 256 palette entries, and with the game's
    /// 32 only; `toggle(i, io)` runs before render i.
    fn parity_frames(
        files: &crate::files::Files,
        toggle: impl Fn(usize, &crate::engine::Io) + 'static,
    ) -> Vec<(u64, u64)> {
        use u6_testkit::Quiet;
        let mut state = crate::state::State::new_game(files).quiet();
        crate::main_loop::load_state_tail(&mut state);
        let out = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let keep = std::rc::Rc::clone(&out);
        let mut e =
            crate::engine::Engine::with_game(state, files.clone(), move |st, io| async move {
                let mut s = st.borrow_mut();
                let fnv = |h: u64, b: u8| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
                let mut n = 0;
                let mut frame = |s: &mut crate::state::State| {
                    toggle(n, &io);
                    crate::map::view::update_view(s);
                    crate::gfx::render_window(s, &io, true);
                    let hashes = io.draw(|f| {
                        let px = f
                            .pixels()
                            .iter()
                            .fold(0xcbf2_9ce4_8422_2325, |h, &b| fnv(h, b));
                        let rgb = |h: u64, c: &crate::Rgb| fnv(fnv(fnv(h, c.r), c.g), c.b);
                        let all = f.palette.iter().fold(px, rgb);
                        let game = f.palette[..32].iter().fold(px, rgb);
                        (all, game)
                    });
                    keep.borrow_mut().push(hashes);
                    n += 1;
                };
                let step = |s: &mut crate::state::State, d: u16| {
                    let l = usize::from(s.actors.party.leader().quiet().0);
                    s.actors.movement_points[l] = 30;
                    crate::actors::party::player_step(s, d);
                };
                frame(&mut s);
                for _ in 0..10 {
                    step(&mut s, 4);
                    frame(&mut s);
                }
                let c = s.map.centre;
                let door = (0x100..s.objects.type_frame.len()).find(|&i| {
                    let p = s.objects.location[i];
                    let kind = s.objects.type_frame[i] & 0x3ff;
                    (0x12c..=0x12e).contains(&kind) && (p.x, p.y, p.z) == (c.x, c.y + 1, c.z)
                });
                let door = door.quiet();
                for _ in 0..2 {
                    s.objects.type_frame[door] ^= 4 << 10;
                    frame(&mut s);
                }
                for _ in 0..2 {
                    step(&mut s, 6);
                    frame(&mut s);
                }
                let c = s.map.centre;
                let (x, y) = (
                    (c.x & 7) + ((c.x >> 2) & 0xf8),
                    (c.y & 7) + ((c.y >> 2) & 0xf8),
                );
                crate::map::travel::teleport(&mut s, x, y, 1, 0);
                frame(&mut s);
                crate::map::travel::teleport(&mut s, c.x, c.y, u16::from(c.z), 0);
                frame(&mut s);
                for _ in 0..2 {
                    step(&mut s, 2);
                    frame(&mut s);
                }
            });
        e.tick(&[], 16);
        out.take()
    }

    /// [`parity_frames`] on the renderer before spec 18 (587d973, a copy of that tree with the
    /// same driver, the Original build), per render: the hash with the 256 palette entries, then
    /// with the game's 32.
    #[cfg(not(feature = "wide"))]
    const PINNED: [(u64, u64); 19] = [
        (0xbcd6_be96_512f_3ab0, 0x5e99_8275_60bf_02b0),
        (0xaecc_ca6c_487d_2c37, 0xb2a6_5281_a232_6ab7),
        (0xca2f_6357_44de_67a1, 0x32e1_b8d1_9b94_cd21),
        (0x590c_c6dd_a8f6_6806, 0xe17d_ba4c_0419_2906),
        (0x763f_ed28_5892_2705, 0x7803_f308_d178_f285),
        (0xd340_f5a5_8620_abd6, 0xe848_ae2f_4c43_64d6),
        (0x3dde_e571_9525_a48b, 0x413b_5beb_938d_710b),
        (0x1069_6869_aa95_c4c8, 0x83b6_222b_9017_90c8),
        (0xf6e0_17e3_6867_542b, 0x3786_1443_06ab_10ab),
        (0x45dd_0341_18a6_5a08, 0xa2ae_5979_1229_0608),
        (0x539e_feed_7c57_7759, 0x8932_1f2c_6714_d0d9),
        (0x6c39_ba9a_0622_d559, 0x40df_dc2f_3c85_2ed9),
        (0x539e_feed_7c57_7759, 0x8932_1f2c_6714_d0d9),
        (0x184c_127f_f3e2_4f3a, 0xf14c_f0ae_145b_6e3a),
        (0xd701_21de_f033_7826, 0xd3eb_ad13_3efc_e926),
        (0x98bc_0e77_453b_a273, 0x8d34_0047_461d_6af3),
        (0xc783_3582_a2eb_0793, 0x7a42_554b_ef77_0013),
        (0x4f74_df68_89a8_1107, 0x2726_09b7_be42_c787),
        (0x1d3c_1dc0_6aa6_4a51, 0x8243_1b40_d208_77d1),
    ];

    /// [`PINNED`] of the Wide build (`--features wide`, the same copy).
    #[cfg(feature = "wide")]
    const PINNED: [(u64, u64); 19] = [
        (0xdb03_148a_0a3e_4ff7, 0xeab4_d763_c85c_2e77),
        (0x3779_9e64_a325_07b8, 0x62a3_be59_757f_fbb8),
        (0x020a_6f79_0238_51c9, 0xba9c_418e_29d8_1349),
        (0x793b_c26f_9e78_8c15, 0x5965_2f69_1b22_2f95),
        (0x8a9e_04b6_5ecb_2df0, 0xb2f5_34dd_6e00_d5f0),
        (0xd91c_c4d0_5623_637f, 0x57c4_a319_40e8_2dff),
        (0xda34_82d1_a27f_7efa, 0xacf6_d847_eee3_3dfa),
        (0xc2dc_88ea_6b62_1f47, 0x19ae_a384_3307_35c7),
        (0xe1f9_d9fa_2469_a498, 0xe3fd_5373_7605_6898),
        (0x6579_d132_9ccf_d1ed, 0x1dfa_9cd0_3c18_196d),
        (0xc24e_9557_9edb_0436, 0x4a02_5af7_22cd_cd36),
        (0x94d3_a90e_15ad_ebc3, 0xbd47_6029_4115_ec43),
        (0xc24e_9557_9edb_0436, 0x4a02_5af7_22cd_cd36),
        (0xf5d6_1f3f_96d7_be3d, 0x5209_cac5_80f6_bdbd),
        (0x9571_1010_b6b3_47e3, 0xc8aa_e915_8ca3_f863),
        (0x1826_e141_227f_5902, 0xaf83_bb07_fca5_c402),
        (0xd91b_dd39_dcca_990e, 0x3df7_8f65_659e_860e),
        (0x1149_4896_2e9c_dd74, 0x44d8_38a7_ca79_9b74),
        (0x175a_b43b_664d_4052, 0x2f48_5650_ebfe_e352),
    ];

    /// Spec 18 (the review's parity oracle): with the remembered sight off (the engine's default)
    /// every render of the driver hashes as the renderer before spec 18 rendered it, pixels and
    /// all 256 palette entries.
    #[test]
    fn real_with_the_switch_off_the_frames_are_the_base_renderers() {
        let Some(files) = files() else { return };
        let frames = parity_frames(&files, |_, _| {});
        assert!(frames.len() == PINNED.len(), "the renders");
        for (i, (got, want)) in frames.iter().zip(&PINNED).enumerate() {
            assert!(got.0 == want.0, "render {i}: the base renderer's frame");
        }
    }

    /// Spec 18: the remembered sight switched on after the start and off again on the way back
    /// from level 1 (it was on through the walk, the door and the level change): while on the
    /// frames differ (the cells the door's closing hid are grey). From the first render after
    /// the switch went off the frames match the base renderer's visually: the pixels and the 32
    /// game colours are its; palette entries 32..63 keep the last grey, and nothing draws with
    /// them after that render.
    #[test]
    fn real_the_switch_on_then_off_gives_the_base_frames_again() {
        let Some(files) = files() else { return };
        let off_at = 16;
        let frames = parity_frames(&files, move |i, io| {
            if i == 1 {
                io.set_sight_memory(true);
            }
            if i == off_at {
                io.set_sight_memory(false);
            }
        });
        assert!(frames.len() == PINNED.len(), "the renders");
        let on = (1..off_at).filter(|&i| frames[i].1 != PINNED[i].1).count();
        assert!(on > 0, "while on, frames differ");
        for i in off_at..frames.len() {
            assert!(frames[i].1 == PINNED[i].1, "render {i}: the base frame");
        }
    }

    /// Spec 18 cost receipt (run alone, `--ignored`; prints numbers only): the game from the start,
    /// the party ten cells south and the door south of it used twice (the remembered sight on
    /// screen), on four engines with the source plane (as the page with HD): the remembered
    /// sight off; on; on with 50 000 more cells remembered off the view (one layer each, seen
    /// now, injected after the scenario); and that at night (also injected: the hour 23, the
    /// ambient light 0 as the clock sets it then, the view flags recomputed), where the unlit
    /// cells in sight that the scenario remembered draw their remembered sight too (darkness
    /// on; not every cell of the window: those never seen stay black). Then 600 idle ticks of
    /// each, alternating: the median and the 95th percentile of a tick, the cells, the memory's
    /// heap bytes ([`SightMemory::heap_bytes`]) and the grey pixels of the last frame.
    #[test]
    #[ignore = "a cost receipt, run alone"]
    #[allow(clippy::disallowed_types)] // a test may read the clock
    fn receipt_the_tick_with_50k_remembered_cells() {
        use crate::Key::{Char, Down, Enter};
        use std::cell::Cell as Flag;
        use std::future::Future;
        let Some(files) = files() else { return };
        let fills = [(); 2].map(|_| std::rc::Rc::new(Flag::new(false)));
        let engine_of = |memory: bool, fill: Option<std::rc::Rc<Flag<bool>>>, night: bool| {
            let mut state = State::new_game(&files).quiet();
            state.rng.seed(0);
            let mut e = Engine::with_game(state, files.clone(), move |st, io| async move {
                let mut game = Box::pin(crate::main_loop::game_first(st.clone(), io));
                std::future::poll_fn(move |cx| {
                    if let Some(f) = &fill
                        && f.replace(false)
                    {
                        let mut s = st.borrow_mut();
                        let now = crate::gfx::cache::minutes(&s.actors.calendar);
                        let z = s.map.centre.z;
                        s.gfx.cache.memory.fill_synthetic(50_000, z, 600, 0x40, now);
                        if night {
                            s.actors.calendar.hour = 23;
                            s.actors.objlist_tail[AMBIENT] = 0;
                            crate::map::view::update_view(&mut s);
                        }
                    }
                    game.as_mut().poll(cx)
                })
                .await;
            });
            e.io().set_sight_memory(memory);
            e.enable_src_plane();
            e
        };
        let mut engines = [
            engine_of(false, None, false),
            engine_of(true, None, false),
            engine_of(true, Some(fills[0].clone()), false),
            engine_of(true, Some(fills[1].clone()), true),
        ];
        let mut commands: Vec<&[crate::Key]> = vec![&[Down]; 10];
        commands.extend([&[Char('u'), Down, Enter][..], &[Char('u'), Down, Enter]]);
        let prompt = |e: &Engine| e.state().main.reading == 1 && e.io().pending_events() == 0;
        for e in engines.iter_mut() {
            for keys in &commands {
                let mut ready = 0;
                for _ in 0..3000 {
                    e.tick(&[], 20);
                    ready = if prompt(e) { ready + 1 } else { 0 };
                    if ready == 3 {
                        break;
                    }
                }
                for &k in *keys {
                    e.tick(&[crate::InputEvent::Key(k)], 20);
                    for _ in 0..30 {
                        e.tick(&[], 20);
                    }
                }
            }
        }
        for f in &fills {
            f.set(true);
        }
        let mut times = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for _ in 0..600 {
            for (i, e) in engines.iter_mut().enumerate() {
                let t = std::time::Instant::now();
                e.tick(&[], 20);
                times[i].push(t.elapsed().as_secs_f64() * 1e3);
            }
        }
        let names = [
            "off",
            "on",
            "on, 50k more cells",
            "on, 50k more cells, night",
        ];
        for (i, name) in names.iter().enumerate() {
            let t = &mut times[i];
            t.sort_by(f64::total_cmp);
            let m = &engines[i].state().gfx.cache.memory;
            let grey = engines[i]
                .frame()
                .pixels()
                .iter()
                .filter(|&&p| p >= GREY)
                .count();
            let light = engines[i].state().actors.objlist_tail[AMBIENT];
            println!(
                "{}x{} memory {name}: tick median {:.3} ms, p95 {:.3} ms; {} cells, {} heap \
                 bytes, {grey} grey pixels, light {light}",
                u6_render::WIDTH,
                u6_render::HEIGHT,
                t[t.len() / 2],
                t[t.len() * 95 / 100],
                m.len(),
                m.heap_bytes()
            );
        }
    }
}

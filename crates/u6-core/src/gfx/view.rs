//! The map window: render 0x102c90 and the refresh 0x102ef0.
//!
//! Geometry (0x102c90 and 0x10e81e): an 11×11 grid of 16-pixel cells drawn from screen (0, 0),
//! cell (col, row) at pixel (16·col, 16·row); the view centre (A4-0x7b78 / -0x7b76 / -0x7b74,
//! [`crate::map::MapState::centre`]) sits in cell (5, 5), i.e. pixels 80..96 on both axes
//! (0x10e81e places an object at `x - centre_x + 5`). Only pixels 8..168 reach the front buffer
//! (0x151f5a, 0x1520e2 with (8, 8)..(0xa7, 0xa7)): ten full cells and a half cell on each side.
//! The frame is 8-wide border tiles 0x1b0..0x1b7 at x / y = 0 and 0xa0.

use u6_render::{Framebuffer, Rgb};

use super::anim::anim_tick;
use super::blit::{blit_back, plot, set_plot_colour};
use super::{GfxState, seams};
use crate::actors::Calendar;
use crate::engine::Io;
use crate::state::State;

/// Cells per side of the map window (`moveq #0xb` loops in 0x102c90).
pub const WINDOW_TILES: usize = 11;
/// Cell side in pixels (`asl.w #4`).
const CELL: u16 = 16;
/// Object type whose tile gets clock hands (0x13aa7e = basetile 0x13a940 + 2·0x9f).
const CLOCK_TYPE: usize = 0x9f;

/// What 0x102c90 draws: the window tile cache A4-0x1476 (11×11 words, built by 0x109e74) and per
/// cell the object tile words in draw order (the lists at A4-0x15f0 heads, A4-0x1384 words,
/// A4-0x1577 links, built by 0x10e81e). Words carry the outline bits 11..12.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewWindow {
    /// `tiles[row][col]`.
    pub tiles: [[u16; WINDOW_TILES]; WINDOW_TILES],
    /// `objects[row * 11 + col]`, first drawn first.
    pub objects: Vec<Vec<u16>>,
}

impl ViewWindow {
    /// Every cell empty (tile 0, no objects).
    pub fn empty() -> Self {
        Self {
            tiles: [[0; WINDOW_TILES]; WINDOW_TILES],
            objects: vec![Vec::new(); WINDOW_TILES * WINDOW_TILES],
        }
    }
}

/// Replaces: 0x162358 (copies the 32 OCS colours into the copper list)
///
/// The chunky buffer carries its palette; every window draw writes the 32 game colours.
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
    let t = &gfx.tables;
    let (px, py) = (x.wrapping_add(4), y.wrapping_add(0xe));
    let hour = usize::from(t.hour_dir[usize::from(cal.hour % 12)]);
    let minute = usize::from(cal.minute) * 2 / 15;
    let tip = |d: usize| {
        let (dx, dy) = (t.dir_dx.get(d).copied(), t.dir_dy.get(d).copied());
        let step = |v: u16, s: Option<i8>| v.wrapping_add(s.unwrap_or(0) as i16 as u16);
        (step(px, dx), step(py, dy))
    };
    let (hand, mark) = (tip(hour), tip(minute));
    set_plot_colour(gfx, gfx.tables.clock_colour);
    gfx.plot_back = true;
    for (hx, hy) in [(px, py), hand, mark] {
        plot(gfx, fb, hx, hy);
    }
    gfx.plot_back = false;
}

/// The border of 0x102c90 (`0x00102E16..0x00102EC2`): corners 0x1b0 (0, 0), 0x1b2 (0xa0, 0),
/// 0x1b3 (0, 0xa0), 0x1b5 (0xa0, 0xa0), then for i = 1..10 the edges 0x1b1 (16i, 0),
/// 0x1b4 (16i, 0xa0), 0x1b6 (0, 16i), 0x1b7 (0xa0, 16i).
fn draw_border(gfx: &GfxState, fb: &mut Framebuffer) {
    for (t, x, y) in [
        (0x1b0, 0, 0),
        (0x1b2, 0xa0, 0),
        (0x1b3, 0, 0xa0),
        (0x1b5, 0xa0, 0xa0),
    ] {
        blit_back(gfx, fb, t, x, y);
    }
    for i in 1..10 {
        blit_back(gfx, fb, 0x1b1, i * CELL, 0);
        blit_back(gfx, fb, 0x1b4, i * CELL, 0xa0);
        blit_back(gfx, fb, 0x1b6, 0, i * CELL);
        blit_back(gfx, fb, 0x1b7, 0xa0, i * CELL);
    }
}

/// Original: 0x102c90 (render the map window into the back buffer; with `framed`, add the cursor
/// and border and present it)
///
/// Order: plot colour = A4-0x6570; per cell (rows, then columns) the cache tile, then its
/// objects, each via 0x152ffa (0x14f12c before each cell is os code whose results the blitter
/// never reads, so it is not called); an object whose tile is the clock's gets hands
/// ([`clock_hands`]).
/// With A4-0x7b65 set: 100 XOR pixels of colour 8 into the back buffer at
/// (random 8..=0xa7, random 8..=0xa7). With `framed` (D0): the ui's target cursor, the border,
/// then the window goes to the front, dissolved when A4-0x6578 is set (cleared after), copied
/// otherwise.
pub fn render_window(state: &mut State, io: &Io, framed: bool) {
    let colour = state.gfx.tables.window_colour;
    set_plot_colour(&mut state.gfx, colour);
    let window = seams::view_window(state);
    let clock = state.tables.basetile.base.get(CLOCK_TYPE).copied();
    let cursor = if framed {
        seams::map_cursor(state, io)
    } else {
        None
    };
    io.draw(|fb| load_palette(state, fb));
    io.draw_back(|fb| load_palette(state, fb));
    let State {
        gfx, actors, rng, ..
    } = state;
    io.draw_back(|fb| {
        for (row, tiles) in window.tiles.iter().enumerate() {
            for (col, &tile) in tiles.iter().enumerate() {
                let (x, y) = (col as u16 * CELL, row as u16 * CELL);
                blit_back(gfx, fb, tile, x, y);
                for &word in &window.objects[row * WINDOW_TILES + col] {
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
            for _ in 0..100 {
                let x = rng.range(8, 0xa7);
                let y = rng.range(8, 0xa7);
                plot(gfx, fb, x, y);
            }
            gfx.plot_xor = false;
            gfx.plot_back = false;
        }
        if framed {
            if let Some((tile, x, y)) = cursor {
                blit_back(gfx, fb, tile, x, y);
            }
            draw_border(gfx, fb);
        }
    });
    if framed {
        let dissolve = gfx.dissolve != 0;
        seams::present_window(io, gfx, dissolve);
        if dissolve {
            gfx.dissolve = 0;
        }
    }
}

/// Original: 0x102ef0 (post-step refresh: rebuild the view flags if stale, redraw the window,
/// redraw a pending panel)
///
/// A4-0x6a8e set: 0x109e74, then clear. A4-0x7af8 set: the animation tick 0x10202e and
/// 0x102c90 with D0 = 1. A4-0x7afc non-zero: clear it and redraw that panel (0x133a1a with
/// D1 = 1).
pub fn refresh_view(state: &mut State, io: &Io) {
    if state.map.view_dirty != 0 {
        seams::view_flags(state);
        state.map.view_dirty = 0;
    }
    if state.gfx.window_shown != 0 {
        anim_tick(state);
        render_window(state, io, true);
    }
    if state.gfx.panel_redraw != 0 {
        let shakes = state.gfx.panel_redraw;
        state.gfx.panel_redraw = 0;
        seams::quake(state, io, shakes);
    }
}

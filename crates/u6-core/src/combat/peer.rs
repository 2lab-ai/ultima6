//! The map overview of hunk 18 (a peer: the 40 × 40 window cache drawn at 4 × 4 pixels per
//! tile in the map window, then a key). Ported with the hunk-18 effects it shares the border
//! drawing with.
//!
//! Spec 13: the window is [`WINDOW`] cells a side (40 in Original, where its 160 × 160 pixels are
//! the shown map; 60 in Wide, 240 × 240 centred in the shown map, [`AT`]).

use std::cell::RefCell;

use crate::engine::Io;
use crate::geom::{HALF_H, HALF_W, SHOWN, WINDOW};
use crate::gfx::blit::{plot, set_plot_colour};
use crate::state::State;

/// The window cache side (tiles): [`WINDOW`], 0x28 in Original.
const SIDE: i16 = WINDOW as i16;
/// Pixels per overview cell, each way.
const DOT: i16 = 4;
/// The shown map ([`SHOWN`]) as pixel words.
const SHOWN_PX: (i16, i16, i16, i16) = (
    SHOWN.0 as i16,
    SHOWN.1 as i16,
    SHOWN.2 as i16,
    SHOWN.3 as i16,
);
/// The overview's top left: the window at [`DOT`] pixels a cell, centred in the shown map ((8, 8)
/// in Original, where it fills it).
const AT: (i16, i16) = (
    SHOWN_PX.0 + (SHOWN_PX.2 + 1 - SHOWN_PX.0 - SIDE * DOT) / 2,
    SHOWN_PX.1 + (SHOWN_PX.3 + 1 - SHOWN_PX.1 - SIDE * DOT) / 2,
);
const _: () = assert!(
    AT.0 >= SHOWN_PX.0 && AT.1 >= SHOWN_PX.1,
    "the overview fits the shown map"
);
/// Cells of the surface per axis: its coordinates wrap at 0x400.
const SURFACE: i16 = 0x400;

fn flags1(s: &State, tile: u16) -> u8 {
    s.tables
        .tileflags
        .flags1
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
}

fn flags2(s: &State, tile: u16) -> u8 {
    s.tables
        .tileflags
        .flags2
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
}

/// Original: 0x134044 (ui: one 4 × 4 overview cell at (x, y) in the back buffer, a checker of
/// the tile's colour)
///
/// -1 (an actor) is a solid white-ish block (colour 15, both checker phases); 0xff (unseen)
/// black; a tile with flags1 0x08 the A4-0x6576 colour, water (0x02) A4-0x6572 or A4-0x6566 by
/// flags1 0x01; any other map tile A4-0x6574, an object tile without a colour nothing.
pub fn overview_cell(s: &mut State, tile: i16, x: i16, y: i16) {
    let c = &s.combat.tables.peer_colours;
    let mut phase: i16 = 1;
    let mut colour: i16 = if tile < 0 {
        phase = 0;
        0xf
    } else if tile == 0xff {
        0
    } else if flags1(s, tile as u16) & 0x08 != 0 {
        c[0]
    } else if flags1(s, tile as u16) & 0x02 != 0 {
        if flags1(s, tile as u16) & 0x01 != 0 {
            c[2]
        } else {
            c[3]
        }
    } else {
        -1
    };
    if colour < 0 && tile >= 0x100 {
        return;
    }
    if colour < 0 {
        colour = c[1];
    }
    set_plot_colour(&mut s.gfx, colour as u16);
    let gfx = &s.gfx;
    crate::engine::with_current_io(|io| {
        io.draw_back(|fb| {
            for r in 0..4i16 {
                for k in 0..4i16 {
                    if (r + k) & 1 == phase {
                        plot(gfx, fb, (x + r) as u16, (y + k) as u16);
                    }
                }
            }
        })
    });
}

/// Original: 0x13410a (ui: the map overview: the 40 × 40 window cache at 4 pixels per tile,
/// objects and actors over it, the leader's cell marked, border, shown until a key)
///
/// In a dungeon (levels 1..4) only what the flood from the leader (0x109c12) reaches is drawn;
/// on the surface the columns / rows past the world's edge are left out ([`surface_cells`]).
pub async fn overview(state: &RefCell<State>, io: &Io) {
    {
        let mut s = state.borrow_mut();
        draw_overview(&mut s);
    }
    io.present();
    state.borrow_mut().ui.input.idle_refresh = false;
    crate::ui::wait::read_key(state, io).await;
    let mut s = state.borrow_mut();
    s.ui.input.idle_refresh = true;
    crate::map::view::update_view(&mut s);
}

/// The drawing part of [`overview`] (everything before the key).
pub(super) fn draw_overview(s: &mut State) {
    let z = s.map.centre.z;
    let dungeon = z > 0 && z < 5;
    s.map.view_flags.iter_mut().for_each(|f| *f = 0);
    s.map.light.iter_mut().for_each(|f| *f = 0);
    let ((mut col0, mut col1), (mut row0, mut row1)) = ((0i16, SIDE), (0i16, SIDE));
    let [ox, oy] = s.map.window_origin;
    let [vx, vy] = s.map.view_offset;
    // The leader's window cell: the view's centre.
    let lead = [vx + HALF_W as i16, vy + HALF_H as i16];
    if dungeon {
        super::seams::flood_sight(s, lead[0], lead[1]);
    } else if z == 0 {
        ((col0, col1), (row0, row1)) = surface_cells([ox, oy], lead);
    }
    s.gfx.plot_back = true;
    set_plot_colour(&mut s.gfx, 0);
    let (x0, y0, x1, y1) = SHOWN_PX;
    super::seams::fill_rect(s, x0, y0, x1, y1);
    let visible = |s: &State, col: i16, row: i16| {
        let f = s.map.view_flags.get((row * SIDE + col) as usize).copied();
        f.unwrap_or(0) & 0x80 != 0
    };
    for row in row0..row1 {
        for col in col0..col1 {
            let i = (row * SIDE + col) as usize;
            let mut t = i16::from(s.map.window_tiles.get(i).copied().unwrap_or(0));
            if dungeon && !visible(s, col, row) {
                t = 0xff;
            }
            overview_cell(s, t, AT.0 + col * DOT, AT.1 + row * DOT);
        }
    }
    let (ox, oy) = (ox as i16, oy as i16);
    let mut o = crate::objects::begin_area(s, ox, oy, ox + SIDE - 1, oy + SIDE - 1);
    while o >= 0 {
        let skip = o == 0 && s.actors.attrs.status[0] & 0x80 == 0;
        if !skip {
            let p = s.objects.location[o as usize];
            let (col, row) = (p.x as i16 - ox, p.y as i16 - oy);
            if !dungeon || visible(s, col, row) {
                let (px, py) = (AT.0 + col * DOT, AT.1 + row * DOT);
                if o < 0x100 {
                    overview_cell(s, -1, px, py);
                } else {
                    let w = s.objects.type_frame[o as usize];
                    let base = s.tables.basetile.base.get(usize::from(w & 0x3ff)).copied();
                    let t = base.unwrap_or(0).wrapping_add(w >> 10);
                    overview_cell(s, t as i16, px, py);
                    let f2 = flags2(s, t);
                    if f2 & 0x80 != 0 && col != 0 {
                        if f2 & 0x40 != 0 && row != 0 {
                            overview_cell(s, t as i16 - 2, px, py - DOT);
                            overview_cell(s, t as i16 - 3, px - DOT, py - DOT);
                        }
                        overview_cell(s, t as i16 - 1, px - DOT, py);
                    } else if f2 & 0x40 != 0 && row != 0 {
                        overview_cell(s, t as i16 - 1, px, py - DOT);
                    }
                }
            }
        }
        o = crate::objects::next_in_area(&mut s.objects);
    }
    set_plot_colour(&mut s.gfx, 0xf);
    // The leader's cell (`vx * 4 + 0x1c` in Original: the view's centre cell).
    let (lx, ly) = (AT.0 + lead[0] * DOT, AT.1 + lead[1] * DOT);
    super::seams::fill_rect(s, lx, ly, lx + DOT - 1, ly + DOT - 1);
    set_plot_colour(&mut s.gfx, 0);
    s.gfx.plot_back = false;
    super::present::draw_borders(s, x0, y0, x1, y1);
    crate::engine::with_current_io(|io| {
        io.with_buffers(|back, front| crate::gfx::blit::copy_window(back, front))
    });
}

/// The window columns and rows, each `(from, to)`, that the overview draws on the surface, for
/// the window `origin` and the leader's window cell `lead`.
///
/// Original: 0x13410a's tests, one of them at most: an origin x of 0x3f0 leaves out columns
/// 0..8, of 0x3e0 those from 0x20; else an origin y from 0x3f0 leaves out the rows before
/// 0x400 - y, of 0x3e0 or 0x3e8 those from it. Wide (spec 13): on each axis the cells past the
/// world's edge from the leader ([`this_side`]), whatever the other axis does.
pub(crate) fn surface_cells(origin: [u16; 2], lead: [i16; 2]) -> ((i16, i16), (i16, i16)) {
    let [ox, oy] = origin;
    let all = (0, SIDE);
    if cfg!(feature = "wide") {
        return (this_side(ox, lead[0]), this_side(oy, lead[1]));
    }
    if ox == 0x3f0 {
        ((8, SIDE), all)
    } else if ox == 0x3e0 {
        ((0, 0x20), all)
    } else if oy >= 0x3f0 {
        (all, ((SURFACE - oy as i16), SIDE))
    } else if oy == 0x3e0 || oy == 0x3e8 {
        (all, (0, SURFACE - oy as i16))
    } else {
        (all, all)
    }
}

/// The window cells `(from, to)` of one axis on the leader's side of the surface's edge: the
/// window wraps there when its `origin` is less than [`SIDE`] cells before 0x400, at window cell
/// `0x400 - origin`; the cells on the other side of it from the leader's cell `lead` are left out.
/// (The rows of Original's tests, for its leader cells 16..=23.)
pub(crate) fn this_side(origin: u16, lead: i16) -> (i16, i16) {
    let edge = SURFACE - (origin & 0x3ff) as i16;
    if edge >= SIDE {
        (0, SIDE)
    } else if edge <= lead {
        (edge, SIDE)
    } else {
        (0, edge)
    }
}

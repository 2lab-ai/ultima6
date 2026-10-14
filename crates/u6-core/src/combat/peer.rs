//! The map overview of hunk 18 (a peer: the 40 × 40 window cache drawn at 4 × 4 pixels per
//! tile in the map window, then a key). Ported with the hunk-18 effects it shares the border
//! drawing with.

use std::cell::RefCell;

use crate::engine::Io;
use crate::gfx::blit::{plot, set_plot_colour};
use crate::state::State;

/// The window cache side (tiles).
const SIDE: i16 = 0x28;

fn flags1(s: &State, tile: u16) -> u8 {
    s.tables.tileflags.flags1.get(usize::from(tile)).copied().unwrap_or(0)
}

fn flags2(s: &State, tile: u16) -> u8 {
    s.tables.tileflags.flags2.get(usize::from(tile)).copied().unwrap_or(0)
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
/// on the surface the columns / rows past the world's edge are left out.
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
fn draw_overview(s: &mut State) {
    let z = s.map.centre.z;
    let dungeon = z > 0 && z < 5;
    s.map.view_flags.iter_mut().for_each(|f| *f = 0);
    s.map.light.iter_mut().for_each(|f| *f = 0);
    let (mut row0, mut row1, mut col0, mut col1) = (0i16, SIDE, 0i16, SIDE);
    let [ox, oy] = s.map.window_origin;
    if dungeon {
        let [vx, vy] = s.map.view_offset;
        super::seams::flood_sight(s, vx + 5, vy + 5);
    } else if z == 0 {
        if ox == 0x3f0 {
            col0 = 8;
        } else if ox == 0x3e0 {
            col1 = 0x20;
        } else if oy >= 0x3f0 {
            row0 = (0x400 - oy) as i16;
        } else if oy == 0x3e0 || oy == 0x3e8 {
            row1 = (0x400 - oy) as i16;
        }
    }
    s.gfx.plot_back = true;
    set_plot_colour(&mut s.gfx, 0);
    super::seams::fill_rect(s, 8, 8, 0xa7, 0xa7);
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
            overview_cell(s, t, col * 4 + 8, row * 4 + 8);
        }
    }
    let (ox, oy) = (ox as i16, oy as i16);
    let mut o = crate::objects::begin_area(s, ox, oy, ox + 0x27, oy + 0x27);
    while o >= 0 {
        let skip = o == 0 && s.actors.attrs.status[0] & 0x80 == 0;
        if !skip {
            let p = s.objects.location[o as usize];
            let (col, row) = (p.x as i16 - ox, p.y as i16 - oy);
            if !dungeon || visible(s, col, row) {
                let (px, py) = (col * 4 + 8, row * 4 + 8);
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
                            overview_cell(s, t as i16 - 2, px, py - 4);
                            overview_cell(s, t as i16 - 3, px - 4, py - 4);
                        }
                        overview_cell(s, t as i16 - 1, px - 4, py);
                    } else if f2 & 0x40 != 0 && row != 0 {
                        overview_cell(s, t as i16 - 1, px, py - 4);
                    }
                }
            }
        }
        o = crate::objects::next_in_area(&mut s.objects);
    }
    set_plot_colour(&mut s.gfx, 0xf);
    let [vx, vy] = s.map.view_offset;
    super::seams::fill_rect(s, vx * 4 + 0x1c, vy * 4 + 0x1c, vx * 4 + 0x1f, vy * 4 + 0x1f);
    set_plot_colour(&mut s.gfx, 0);
    s.gfx.plot_back = false;
    super::present::draw_borders(s, 8, 8, 0xa7, 0xa7);
    crate::engine::with_current_io(|io| {
        io.with_buffers(|back, front| crate::gfx::blit::copy_window(back, front))
    });
}

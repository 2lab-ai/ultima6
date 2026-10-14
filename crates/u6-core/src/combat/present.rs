//! What the combat code puts on the screen: missile flights and blasts (hunk 18), and the log of
//! frames the synchronous core presented, played back by [`play_effects`].
//!
//! The flight 0x132706 draws into the back buffer and copies each step's changed rectangle to
//! the screen (0x151fe6); the hit flash 0x1230ce presents a whole window (0x102c90). The core
//! does exactly that on the polling engine's buffers ([`with_current_io`]) and records each
//! presented rectangle with the number of messages queued before it ([`capture`]). Playback
//! puts the rectangles back on the screen in order, prints the messages in between, and waits
//! VBlanks so the pictures are seen: the original shows them for as long as its drawing takes,
//! the port for a fixed number of display frames (display time is not game time, spec 03 §1).

use std::cell::RefCell;

use crate::engine::{Io, with_current_io};
use crate::gfx::blit::{blit_back, copy_rect};
use crate::state::State;

use super::tables::at;

/// VBlanks a hit flash stays on the screen.
pub const FLASH_HOLD: u8 = 8;
/// Flight steps per VBlank (the original steps the overlay animation every 4 steps).
const STEPS_PER_FRAME: i16 = 4;
/// The map window with its border, in screen pixels.
const WINDOW: (usize, usize) = (176, 176);

/// One presented rectangle of the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect {
    /// Top-left pixel and size.
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    /// The screen's pixels there, row-major.
    pub pixels: Vec<u8>,
    /// VBlanks to wait after showing it.
    pub hold: u8,
    /// Messages queued (`main.pending_print`) before it was presented.
    pub mark: usize,
}

/// Records the screen rectangle (x0, y0)..=(x1, y1) as it is now (after a present).
pub(crate) fn capture(s: &mut State, x0: usize, y0: usize, x1: usize, y1: usize, hold: u8) {
    if x1 < x0 || y1 < y0 {
        return;
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let pixels = with_current_io(|io| {
        io.draw(|fb| {
            let mut p = Vec::with_capacity(w * h);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    p.push(fb.get(x, y).unwrap_or(0));
                }
            }
            p
        })
    });
    if let Some(pixels) = pixels {
        let mark = s.main.pending_print.len();
        s.combat.effects.push(Effect {
            x: x0,
            y: y0,
            w,
            h,
            pixels,
            hold,
            mark,
        });
    }
}

/// Records the whole map window.
pub(crate) fn capture_window(s: &mut State, hold: u8) {
    capture(s, 0, 0, WINDOW.0 - 1, WINDOW.1 - 1, hold);
}

/// Prints the first `n` queued messages and keeps the rest queued.
async fn flush_first(state: &RefCell<State>, io: &Io, n: usize) {
    if n == 0 {
        return;
    }
    let rest = {
        let mut s = state.borrow_mut();
        let q = &mut s.main.pending_print;
        let n = n.min(q.len());
        q.split_off(n)
    };
    crate::main_loop::flush_prints(state, io).await;
    let mut s = state.borrow_mut();
    let mut q = rest;
    q.append(&mut s.main.pending_print);
    s.main.pending_print = q;
}

/// Plays the recorded frames in order with the messages queued between them, then the rest of
/// the messages. A no-op when nothing was recorded.
pub async fn play_effects(state: &RefCell<State>, io: &Io) {
    let effects = std::mem::take(&mut state.borrow_mut().combat.effects);
    let mut printed = 0;
    for e in effects {
        if e.mark > printed {
            flush_first(state, io, e.mark - printed).await;
            printed = e.mark;
        }
        io.draw(|fb| {
            for r in 0..e.h {
                for c in 0..e.w {
                    fb.set(e.x + c, e.y + r, e.pixels[r * e.w + c]);
                }
            }
        });
        io.present();
        state.borrow_mut().combat.shown += 1;
        for _ in 0..e.hold {
            io.vblank().await;
        }
    }
    crate::main_loop::flush_prints(state, io).await;
    let mut s = state.borrow_mut();
    if s.combat.swallow {
        s.combat.swallow = false;
        s.ui.text.skip_newline = true;
    }
}

/// Original: 0x13242e (ui: grow the flight's changed rectangle A4+0x1900.. to the 16 × 16
/// sprite centred on (x, y), clipped to the window 8..0xa7)
pub fn extend_dirty(s: &mut State, x: i16, y: i16) {
    let d = &mut s.combat.dirty;
    if x < 16 {
        d[0] = 8;
    } else if x - 8 < d[0] {
        d[0] = x - 8;
    }
    if y < 16 {
        d[1] = 8;
    } else if y - 8 < d[1] {
        d[1] = y - 8;
    }
    if x > 0x9f {
        d[2] = 0xa7;
    } else if x + 8 > d[2] {
        d[2] = x + 8;
    }
    if y > 0x9f {
        d[3] = 0xa7;
    } else if y + 8 > d[3] {
        d[3] = y + 8;
    }
}

fn back_blit(s: &State, tile: u16, x: u16, y: u16) {
    with_current_io(|io| io.draw_back(|fb| blit_back(&s.gfx, fb, tile, x, y)));
}

/// Original: 0x1324ae (ui: redraw the window border (tiles 0x1b0..0x1b7) along the edges the
/// rectangle (x0, y0)..(x1, y1) touches)
pub fn draw_borders(s: &State, x0: i16, y0: i16, x1: i16, y1: i16) {
    let (left, top, right, bottom) = (x0 < 16, y0 < 16, x1 > 0x9f, y1 > 0x9f);
    if left && top {
        back_blit(s, 0x1b0, 0, 0);
    }
    if right && top {
        back_blit(s, 0x1b2, 0xa0, 0);
    }
    if left && bottom {
        back_blit(s, 0x1b3, 0, 0xa0);
    }
    if right && bottom {
        back_blit(s, 0x1b5, 0xa0, 0xa0);
    }
    for i in 1..10u16 {
        if top {
            back_blit(s, 0x1b1, i * 16, 0);
        }
        if bottom {
            back_blit(s, 0x1b4, i * 16, 0xa0);
        }
        if left {
            back_blit(s, 0x1b6, 0, i * 16);
        }
        if right {
            back_blit(s, 0x1b7, 0xa0, i * 16);
        }
    }
}

/// Original: 0x1325e4 (ui: redraw what lies over a flight in the rectangle (x0, y0)..(x1, y1):
/// per window cell the draw-list tiles with flags2 bit 4 (on top), darkness (tile 0xff) on a
/// cell whose base is 0xff or 0x1bc, then the border)
pub fn redraw_over(s: &State, x0: i16, y0: i16, x1: i16, y1: i16) {
    let d = &s.objects.draw;
    for col in (x0 >> 4)..=(x1 >> 4) {
        for row in (y0 >> 4)..=(y1 >> 4) {
            let cell = (row * 11 + col) as usize;
            let mut n = d.cell.get(cell).copied().unwrap_or(0);
            let mut guard = 0;
            while n != 0 && guard < 256 {
                let t = d.node_tile[usize::from(n)];
                if t != 0 {
                    let tile = t & 0x7ff;
                    let f2 = s.tables.tileflags.flags2.get(usize::from(tile)).copied();
                    if f2.unwrap_or(0) & 0x10 != 0 {
                        back_blit(s, tile, (col * 16) as u16, (row * 16) as u16);
                    }
                }
                n = d.node_next[usize::from(n)];
                guard += 1;
            }
            let base = d.base.get(cell).copied().unwrap_or(0);
            if base == 0xff || base == 0x1bc {
                back_blit(s, 0xff, (col * 16) as u16, (row * 16) as u16);
            }
        }
    }
    draw_borders(s, x0, y0, x1, y1);
}

/// Per-missile state of a flight (the 16-entry stack arrays of 0x132706).
#[derive(Clone, Copy, Debug, Default)]
struct Missile {
    /// -0x40 / -0x60: position in pixels of the map.
    px: i16,
    py: i16,
    /// -0x1a0 / -0x1c0: the tile it is on.
    tx: i16,
    ty: i16,
    /// -0x80: steps along the major axis (the flight ends for it at that step).
    steps: i16,
    /// -0xa0: steps along the minor axis.
    minor: i16,
    /// -0xe0 / -0x120: x and y of the straight (0) and diagonal (1) step.
    sx: [i16; 2],
    sy: [i16; 2],
    /// -0x140: the error term; -0x180: what each choice adds to it.
    err: i16,
    add: [i16; 2],
    /// -0x1e0: sprite angle.
    angle: i16,
    /// -0x240 / -0x260: where its sprite was last drawn on the screen.
    drawn: (i16, i16),
}

/// Flight flags (the word at 0xe(A5) of 0x132706).
pub mod flight {
    /// The missile flies back from the target to the shooter (boomerang).
    pub const RETURN: u16 = 1;
    /// Sprites are drawn straight into the window without saving what is under them.
    pub const TRAIL: u16 = 2;
    /// One unturned sprite (slot 15) for every missile.
    pub const SHARED: u16 = 8;
}

/// Original: 0x132706 (ui: fly `n` missiles of `tile` from tile (x0, y0) toward (xs[i], ys[i]))
///
/// Bresenham in pixels (16 per tile), one pixel step per round along the major axis; a missile
/// stops at the first tile past `range` (the A4-0x69a0 distance from (x0, y0)), at a map tile
/// with flags2 bit 5 (bit 3 lets it through next to the shooter), or at an object whose tile has
/// flags2 bit 5; stopping one missile ends the round for the others. Actors it passes (not the
/// lying 0x165 frame 0, not swallowed) are listed in A4+0x186e. Sprites (turned by the A4-0x64f4
/// angle table unless [`flight::SHARED`]) go into the back buffer over saved backgrounds and the
/// changed rectangle is copied to the screen each round ([`capture`]d for playback). At the end
/// `xs` / `ys` hold where each missile stopped (a [`flight::RETURN`] flight keeps them), the
/// first one's object (seen by the shooter A4-0x69a2) goes to A4+0x185c; returns 1 when every
/// missile reached its target.
#[allow(clippy::too_many_arguments)]
pub fn fly(
    s: &mut State,
    x0: i16,
    y0: i16,
    xs: &mut [i16],
    ys: &mut [i16],
    n: i16,
    range: i16,
    tile: u16,
    flags: u16,
) -> i16 {
    with_current_io(|io| crate::gfx::render_window(s, io, false));
    let n = n.clamp(0, 16) as usize;
    let mut passed = [0u16; 32];
    let mut m = [Missile::default(); 16];
    m[0].tx = x0;
    m[0].ty = y0;
    s.combat.landing = (x0, y0);
    let mut reached: i16 = 1;
    if flags & flight::RETURN != 0 {
        m[0].tx = xs[0];
        xs[0] = x0;
        std::mem::swap(&mut m[0].ty, &mut ys[0]);
    }
    m[0].px = m[0].tx * 16 + 8;
    m[0].py = m[0].ty * 16 + 8;
    let t = s.combat.tables.clone();
    let kind = t.missile_tiles.iter().position(|&k| k == tile).unwrap_or(0);
    let step = if flags & flight::TRAIL != 0 {
        6
    } else {
        i16::from(at(&t.missile_step, kind))
    };
    let (cx, cy) = (
        u16::from(at(&t.missile_cx, kind)),
        u16::from(at(&t.missile_cy, kind)),
    );
    let (sx0, sy0) = (m[0].tx, m[0].ty);
    if tile != 0 {
        if flags & flight::SHARED != 0 {
            super::seams::sprite_prepare(s, 0xf, tile, 0, 0x7f, 0x7f);
        } else {
            for i in 0..n {
                let a = i16::from(t.angle(xs[i].wrapping_sub(sx0), ys[i].wrapping_sub(sy0)));
                m[i].angle = a.wrapping_sub(i16::from(at(&t.missile_angle, kind)));
                let slot = if flags & flight::TRAIL != 0 { i } else { i + 8 } as u16;
                super::seams::sprite_prepare(s, slot, tile, m[i].angle as u16, cx, cy);
            }
        }
    }
    let mut longest: i16 = 0;
    for i in 0..n {
        let first = m[0];
        let mi = &mut m[i];
        mi.px = first.px;
        mi.py = first.py;
        mi.tx = first.tx;
        mi.ty = first.ty;
        let mut major = xs[i].wrapping_sub(first.tx).wrapping_mul(16);
        let mut minor = ys[i].wrapping_sub(first.ty).wrapping_mul(16);
        mi.sy[0] = 0;
        let sx = if major < 0 {
            major = major.wrapping_neg();
            -1
        } else {
            1
        };
        mi.sx = [sx, sx];
        mi.sy[1] = if minor < 0 {
            minor = minor.wrapping_neg();
            -1
        } else {
            1
        };
        if major < minor {
            std::mem::swap(&mut major, &mut minor);
            mi.sy[0] = mi.sy[1];
            mi.sx[1] = mi.sx[0];
            mi.sx[0] = 0;
        }
        mi.steps = major;
        mi.minor = minor;
        if major > longest {
            longest = major;
        }
        mi.err = minor.wrapping_add(minor).wrapping_sub(major);
        mi.add = [
            minor.wrapping_add(minor),
            minor.wrapping_sub(major).wrapping_mul(2),
        ];
    }
    let spin = i16::from(at(&t.missile_spin, kind));
    let mut round: i16 = 0;
    while round < longest {
        s.combat.dirty = [0xa8, 0xa8, 7, 7];
        let mut visible = false;
        'missiles: for i in 0..n {
            if round >= m[i].steps {
                if round != 0 {
                    let (dx, dy) = m[i].drawn;
                    extend_dirty(s, dx, dy);
                }
                continue;
            }
            let k = usize::from(m[i].err >= 0);
            m[i].err = m[i].err.wrapping_add(m[i].add[k]);
            m[i].px = m[i].px.wrapping_add(m[i].sx[k]);
            m[i].py = m[i].py.wrapping_add(m[i].sy[k]);
            if tile != 0 && step != 0 && round % step == 0 {
                let c = s.map.centre;
                let sx = m[i].px.wrapping_sub((c.x as i16 - 5) * 16);
                let sy = m[i].py.wrapping_sub((c.y as i16 - 5) * 16);
                if (8..0xa8).contains(&sx) && (8..0xa8).contains(&sy) {
                    visible = true;
                    extend_dirty(s, sx, sy);
                    if flags & flight::TRAIL != 0 {
                        super::seams::sprite_draw(s, i as u16, sx as u16, sy as u16);
                    } else {
                        if round != 0 {
                            let (dx, dy) = m[i].drawn;
                            extend_dirty(s, dx, dy);
                        }
                        m[i].drawn = (sx, sy);
                    }
                }
                if spin != 0 {
                    m[i].angle = m[i].angle.wrapping_add(spin);
                    if flags & (flight::TRAIL | flight::SHARED) == 0 {
                        let a = m[i].angle as u16;
                        super::seams::sprite_prepare(s, i as u16 + 8, tile, a, cx, cy);
                    }
                }
            }
            let (ntx, nty) = (m[i].px >> 4, m[i].py >> 4);
            if ntx == m[i].tx && nty == m[i].ty {
                continue;
            }
            let ddx = ntx.wrapping_sub(x0).wrapping_abs();
            let ddy = nty.wrapping_sub(y0).wrapping_abs();
            if ddx > 7 || ddy > 7 || i16::from(t.distance_at(ddx as usize, ddy as usize)) > range {
                m[i].steps = round;
                break 'missiles;
            }
            m[i].tx = ntx;
            m[i].ty = nty;
            let z = u16::from(s.map.centre.z);
            let mt = crate::actors::seams::tile_at(s, ntx as u16, nty as u16, z);
            let f2 = |s: &State, tile: u16| {
                s.tables
                    .tileflags
                    .flags2
                    .get(usize::from(tile))
                    .copied()
                    .unwrap_or(0)
            };
            let near = ddx <= 1 && ddy <= 1 && f2(s, u16::from(mt)) & 0x08 != 0;
            if !near && f2(s, u16::from(mt)) & 0x20 != 0 {
                m[i].steps = round;
                break 'missiles;
            }
            let mut o =
                crate::objects::begin_at(&mut s.objects, &s.tables, ntx as u16, nty as u16, z);
            while o >= 0 {
                let w = s.objects.type_frame[o as usize];
                if o < 0x100 {
                    let lying = w & 0x3ff == 0x165 && w >> 10 == 0;
                    if !lying && s.actors.attrs.movement_flags[o as usize] & 0x10 == 0 {
                        passed[(o >> 3) as usize] |= 1 << (o & 7);
                    }
                }
                let base = s.tables.basetile.base.get(usize::from(w & 0x3ff)).copied();
                let part_tile = base
                    .unwrap_or(0)
                    .wrapping_add(w >> 10)
                    .wrapping_sub(s.objects.at.part);
                if f2(s, part_tile) & 0x20 != 0 {
                    break;
                }
                o = crate::objects::next_at(&mut s.objects, &s.tables);
            }
            if o >= 0 {
                m[i].steps = round;
                break 'missiles;
            }
            if i == 0 {
                s.combat.landing = (m[0].tx, m[0].ty);
            }
        }
        if visible {
            if round & 3 == 0 {
                crate::main_loop::anim_step(s);
            }
            let draws = flags & flight::TRAIL == 0;
            if draws {
                for (i, mi) in m.iter().enumerate().take(n) {
                    if round < mi.steps || flags & flight::SHARED != 0 {
                        let (dx, dy) = mi.drawn;
                        super::seams::sprite_save(s, i as u16, dx as u16, dy as u16);
                    }
                }
                for (i, mi) in m.iter().enumerate().take(n) {
                    let (dx, dy) = mi.drawn;
                    if flags & flight::SHARED != 0 {
                        super::seams::sprite_draw(s, 0xf, dx as u16, dy as u16);
                    } else if round < mi.steps {
                        super::seams::sprite_draw(s, i as u16 + 8, dx as u16, dy as u16);
                    }
                }
            }
            let [dx0, dy0, dx1, dy1] = s.combat.dirty;
            if dx0 <= dx1 && dy0 <= dy1 {
                redraw_over(s, dx0, dy0, dx1, dy1);
                with_current_io(|io| {
                    io.with_buffers(|back, front| {
                        let w = |v: i16| v as u16;
                        copy_rect(back, front, w(dx0), w(dy0), w(dx1), w(dy1), w(dx0), w(dy0));
                    })
                });
                let hold = u8::from(round % STEPS_PER_FRAME == 0);
                let (cx0, cx1) = ((dx0 as usize) & !7, (dx1 as usize) | 7);
                capture(s, cx0, dy0 as usize, cx1, dy1 as usize, hold);
            }
            if draws {
                for (i, mi) in m.iter().enumerate().take(n) {
                    if round < mi.steps || flags & flight::SHARED != 0 {
                        let (dx, dy) = mi.drawn;
                        super::seams::sprite_draw(s, i as u16, dx as u16, dy as u16);
                    }
                }
            }
        }
        round += 1;
    }
    if tile != 0 && flags & (flight::TRAIL | flight::SHARED) == 0 {
        with_current_io(|io| crate::gfx::refresh_view(s, io));
        capture_window(s, 0);
    }
    for i in 0..n {
        let moved_x = m[i].tx != xs[i];
        let moved_y = m[i].ty != ys[i];
        if moved_x || (moved_y && flags & flight::RETURN == 0) {
            xs[i] = m[i].tx;
            ys[i] = m[i].ty;
            reached = 0;
            if i == 0 && s.combat.shooter >= 0 {
                let shooter = s.combat.shooter;
                s.combat.hit = super::leaves::actor_at(s, shooter, m[0].tx, m[0].ty);
            }
        }
    }
    let mut list = Vec::new();
    'words: for (w, bits) in passed.iter().enumerate() {
        for b in 0..8 {
            if bits & (1 << b) != 0 {
                list.push((w * 8 + b) as i16);
            }
            if list.len() > 0x3e {
                break 'words;
            }
        }
    }
    list.push(-1);
    s.combat.passed = list;
    reached
}

/// Original: 0x1331b4 (ui: a blast of 16 sprites of `tile` from (x, y) out to the A4-0x63e6 /
/// -0x63d6 offsets, range 3, drawn as trails; returns the actors it reached, A4+0x186e: those
/// the flights passed, then those on (x, y) up to the first object that stops missiles)
pub fn blast(s: &mut State, x: i16, y: i16, tile: u16) -> Vec<i16> {
    let mut xs = [0i16; 16];
    let mut ys = [0i16; 16];
    for i in 0..16 {
        xs[i] = x.wrapping_add(i16::from(at(&s.combat.tables.blast_dx, i) as i8));
        ys[i] = y.wrapping_add(i16::from(at(&s.combat.tables.blast_dy, i) as i8));
    }
    fly(s, x, y, &mut xs, &mut ys, 16, 3, tile, 6);
    let mut list = s.combat.passed.clone();
    let k = list.iter().take(0x3e).take_while(|&&a| a >= 0).count();
    list.truncate(k);
    let z = u16::from(s.map.centre.z);
    let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, x as u16, y as u16, z);
    while list.len() < 0x3f && o >= 0 {
        let w = s.objects.type_frame[o as usize];
        if o < 0x100 {
            let lying = w & 0x3ff == 0x165 && w >> 10 == 0;
            if !lying && s.actors.attrs.movement_flags[o as usize] & 0x10 == 0 {
                list.push(o);
            }
        }
        let base = s.tables.basetile.base.get(usize::from(w & 0x3ff)).copied();
        let t = base
            .unwrap_or(0)
            .wrapping_add(w >> 10)
            .wrapping_sub(s.objects.at.part);
        if s.tables
            .tileflags
            .flags2
            .get(usize::from(t))
            .copied()
            .unwrap_or(0)
            & 0x20
            != 0
        {
            break;
        }
        o = crate::objects::next_at(&mut s.objects, &s.tables);
    }
    list.push(-1);
    s.combat.passed = list.clone();
    list
}

/// A `printf` argument of a combat message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    Str(Vec<u8>),
    Word(i16),
}

/// One combat message as queued: the image address of its format and the arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    pub at: u32,
    pub args: Vec<Text>,
}

/// Messages [`CombatState::said`](super::CombatState) keeps (the last ones).
const SAID_KEPT: usize = 64;

/// [ui] 0x108658 with the string at image address `at` of the user's `game` as the format:
/// queued for the command loop like the actor code's lines, and kept in `combat.said`.
pub(crate) fn say(s: &mut State, at: u32, args: Vec<Text>) {
    let said = &mut s.combat.said;
    if said.len() >= SAID_KEPT {
        said.remove(0);
    }
    said.push(Said {
        at,
        args: args.clone(),
    });
    let fmt = with_current_io(|io| crate::main_loop::game_string(io, at));
    if let Some(mut fmt) = fmt {
        if s.combat.swallow {
            let n = fmt.iter().take_while(|&&b| b == b'\n').count();
            fmt.drain(..n);
            if !fmt.is_empty() {
                s.combat.swallow = false;
            }
        }
        let args = args
            .into_iter()
            .map(|a| match a {
                Text::Str(b) => crate::main_loop::PrintArg::Str(b),
                Text::Word(w) => crate::main_loop::PrintArg::Word(w as u16),
            })
            .collect();
        crate::main_loop::queue_print(s, fmt, args);
    }
}

/// The display name of an object (0x10eef8), as a message argument.
pub(crate) fn name(s: &State, o: i16) -> Text {
    Text::Str(crate::objects::display_name(s, o).into_bytes())
}

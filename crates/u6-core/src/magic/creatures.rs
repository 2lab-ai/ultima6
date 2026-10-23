//! The spells that make, change or strike the creatures around the caster: Slime 0x118382,
//! Conjure / Summon 0x118cec, Insect Swarm 0x1195f2, Tremor 0x119cbe, Wing Strike 0x11a0cc,
//! spell 0x80 (0x11a41e) and Armageddon 0x11a550.

use std::cell::RefCell;

use super::effects::{FMT_NL_S_NL, NEWLINE, print_at, say};
use super::seams;
use super::spells::flash_window;
use super::tables::Msg;
use crate::engine::Io;
use crate::geom::{HALF_W, RULE_HALF, VIEW_W};
use crate::objects::Slot;
use crate::state::State;

/// The slime type Slime turns foes into.
const SLIME: u16 = 0x177;
/// The insect type of Insect Swarm, the daemon of Summon.
const INSECTS: u16 = 0x157;
const DAEMON: u16 = 0x16f;
/// Hunk 11: the line of a daemon summoned hostile, and the line of no foe in view.
const MSG_HOSTILE: u32 = 0x0011_59dc;
pub(super) const MSG_NO_FOES: u32 = 0x0011_5a66;
/// The Wing Strike dragon (two parts) and the A4 rows it reads: the step sign, the frames.
const DRAGON_PART: u16 = 0x19b;

/// Creature flag word of the monster type of `o` (A4-0x7dbe) and its index (-1 not listed).
fn flags_of(s: &State, o: u16) -> (i16, u16) {
    let idx = crate::objects::monster_index(&s.objects, s.objects.kind(o as Slot));
    (idx, s.actors.game.creature_flags(idx))
}

/// Original: 0x118382 (magic: Slime: every evil actor in view becomes a slime)
///
/// "In view" is the original's 11 × 11 screen round the leader in both builds (spec 13
/// Decision 3, [`RULE_HALF`]).
/// For the first object of every tile within 5 of the view centre (columns outside): an actor
/// slot below 0x100 of alignment 0x20 that does not resist (0x122076) becomes type 0x177,
/// unless it is a creature with flag 0x0200 other than index 1; one of the temporary slots
/// 0xe0.. loses everything it carries (0x10c1d6). Then the view dissolves in (no message).
pub async fn slime(state: &RefCell<State>, io: &Io) {
    let caster = super::cast::caster(&state.borrow());
    super::spells::flash_actor(state, io, caster, 0).await;
    let s = &mut *state.borrow_mut();
    let c = s.map.centre;
    let (cx, cy, z) = (c.x as i16, c.y as i16, u16::from(c.z));
    for x in cx - RULE_HALF..=cx + RULE_HALF {
        for y in cy - RULE_HALF..=cy + RULE_HALF {
            let o = crate::objects::begin_at(&mut s.objects, &s.tables, x as u16, y as u16, z);
            if !(0..0x100).contains(&o) {
                continue;
            }
            let i = o as usize;
            if s.actors.attrs.status[i] & 0x60 != 0x20 || seams::resists(s, caster, o as u16) {
                continue;
            }
            let (idx, flags) = flags_of(s, o as u16);
            if idx >= 0 && flags & 0x0200 != 0 && idx != 1 {
                continue;
            }
            s.objects.type_frame[i] = SLIME;
            if o >= 0xe0 {
                for _ in 0..crate::objects::OBJECT_SLOTS {
                    let child = crate::objects::first_child(&mut s.objects, o);
                    if child < 0 {
                        break;
                    }
                    crate::objects::remove(s, child);
                }
            }
        }
    }
    super::effects::redraw_dissolving(s, io);
}

/// Up to eight random spots within 5 of the view centre (two draws 0..10, x first) until `new`
/// may stand on one (0x11b1c0); it is moved there. False when none was found. The port draws
/// the numbers but tests nothing for a missing creature (-1; the original tests slot -1).
fn place_near(s: &mut State, new: Slot) -> bool {
    let c = s.map.centre;
    for _ in 0..8 {
        let dx = s.rng.range(0, 10);
        let dy = s.rng.range(0, 10);
        let (x, y) = ((c.x + dx).wrapping_sub(5), (c.y + dy).wrapping_sub(5));
        if new >= 0 && crate::actors::walk::can_move_to(s, new as u16, x, y) {
            crate::objects::move_to(s, new, x, y, u16::from(c.z));
            return true;
        }
    }
    false
}

/// Alignment bits 0x60 = `align`, worktype 8.
fn enlist(s: &mut State, new: Slot, align: u8) {
    let i = new as usize;
    let a = &mut s.actors.attrs;
    a.status[i] = (a.status[i] & !0x60) | align;
    s.actors.worktype[i] = 8;
}

/// Original: 0x118cec (magic: Conjure (0x31) / Summon (0x77) a creature near the caster)
///
/// A draw 0..15 picks Conjure's creature (≤ 3: 0x173, ≤ 6: 0x156, ≤ 9: 0x169, ≤ 12: 0x158, else
/// 0x166; drawn for Summon too); the window and the caster flash. The creature (Summon: the
/// daemon 0x16f) is made at the centre (0x131646) and put on the first of up to 8 random spots
/// near it where it may stand; none: it goes again, silently. Placed: Summon's daemon turns
/// chaotic (0x60) when it resists the caster (0x122076), the hostile line (0x1159dc); otherwise
/// it takes the caster's alignment and `Msg::Success`; worktype 8, shown dissolving.
pub async fn summon(state: &RefCell<State>, io: &Io, spell: u16, caster: u16) {
    let r = state.borrow_mut().rng.range(0, 0xf);
    let conjured = match r {
        0..=3 => 0x173,
        4..=6 => 0x156,
        7..=9 => 0x169,
        10..=12 => 0x158,
        _ => 0x166,
    };
    flash_window(state, io, spell / 16 + 1).await;
    seams::flash_actor(&mut state.borrow_mut(), io, caster, 0);
    let new = {
        let mut s = state.borrow_mut();
        let c = s.map.centre;
        let kind = match spell {
            0x31 => conjured,
            0x77 => DAEMON,
            _ => return,
        };
        let new = seams::spawn(&mut s, kind, c.x, c.y, u16::from(c.z));
        if !place_near(&mut s, new) {
            crate::objects::remove(&mut s, new);
            return;
        }
        new
    };
    let hostile = spell == 0x77 && seams::resists(&mut state.borrow_mut(), caster, new as u16);
    let align = if hostile {
        print_at(state, io, MSG_HOSTILE, &[]).await;
        0x60
    } else {
        let a = state.borrow().actors.attrs.status[usize::from(caster)] & 0x60;
        say(state, io, FMT_NL_S_NL, Msg::Success).await;
        a
    };
    let mut s = state.borrow_mut();
    enlist(&mut s, new, align);
    super::effects::redraw_dissolving(&mut s, io);
}

/// Original: 0x1195f2 (magic: Insect Swarm: eight swarms (0x157) near the caster on its side)
///
/// The flashes; eight times a swarm is made at the centre (0x131646) and put on the first of up
/// to 8 random spots near it where it may stand, with the caster's alignment and worktype 8;
/// one that finds none goes again. Then `Msg::Success`.
pub async fn insect_swarm(state: &RefCell<State>, io: &Io, spell: u16) {
    flash_window(state, io, spell / 16 + 1).await;
    let caster = super::cast::caster(&state.borrow());
    seams::flash_actor(&mut state.borrow_mut(), io, caster, 0);
    {
        let mut s = state.borrow_mut();
        for _ in 0..8 {
            let c = s.map.centre;
            let new = seams::spawn(&mut s, INSECTS, c.x, c.y, u16::from(c.z));
            if place_near(&mut s, new) {
                let align = s.actors.attrs.status[usize::from(caster)] & 0x60;
                enlist(&mut s, new, align);
            } else {
                crate::objects::remove(&mut s, new);
            }
        }
    }
    say(state, io, FMT_NL_S_NL, Msg::Success).await;
}

/// Original: 0x119cbe (magic: Tremor: the window shakes, every evil actor in view is hurt)
///
/// "In view" is the original's 11 × 11 screen round the leader in both builds (spec 13
/// Decision 3, [`RULE_HALF`]).
/// The caster's flash, the quake (0x133a1a, 20 shakes of 3 pixels); then for the first object
/// of every tile within 5 of the centre (columns outside): an actor slot below 0x100 of
/// alignment 0x20 that is not a creature with flag 0x0100 takes random 1..30 damage (0x1231be),
/// the caster the experience (0x122104), a newline and the wound line (0x12395a). `Msg::Success`.
pub async fn tremor(state: &RefCell<State>, io: &Io) {
    let caster = super::cast::caster(&state.borrow());
    {
        let mut s = state.borrow_mut();
        seams::flash_actor(&mut s, io, caster, 0);
        crate::ui::effects::quake(&mut s, io, 0x14, 3);
    }
    // Both draw before the loop in the original (0x119cd6, 0x119cde): their frames now.
    super::effects::settle(state, io).await;
    let c = state.borrow().map.centre;
    let (cx, cy, z) = (c.x as i16, c.y as i16, u16::from(c.z));
    for x in cx - RULE_HALF..=cx + RULE_HALF {
        for y in cy - RULE_HALF..=cy + RULE_HALF {
            let o = {
                let s = &mut *state.borrow_mut();
                crate::objects::begin_at(&mut s.objects, &s.tables, x as u16, y as u16, z)
            };
            if !(0..0x100).contains(&o) {
                continue;
            }
            {
                let mut s = state.borrow_mut();
                let (_, flags) = flags_of(&s, o as u16);
                if s.actors.attrs.status[o as usize] & 0x60 != 0x20 || flags & 0x0100 != 0 {
                    continue;
                }
                let d = s.rng.range(1, 0x1e) as i16;
                let exp = seams::hurt(&mut s, o as u16, d);
                seams::add_experience(&mut s, caster, exp);
            }
            print_at(state, io, NEWLINE, &[]).await;
            seams::wound_line(state, io, o as u16).await;
        }
    }
    say(state, io, FMT_NL_S_NL, Msg::Success).await;
}

/// Original: 0x11a41e (magic: spell 0x80, outside the eight circles: the caster turns invisible
/// (object status 0x02) with the flash, `Msg::Success`; already invisible: `Msg::NoEffect`)
pub async fn spell_0x80(state: &RefCell<State>, io: &Io) {
    let done = {
        let mut s = state.borrow_mut();
        let c = super::cast::caster(&s);
        let st = &mut s.objects.obj_status[usize::from(c)];
        if *st & 2 == 0 {
            *st |= 2;
            seams::flash_actor(&mut s, io, c, 1);
            true
        } else {
            false
        }
    };
    let m = if done { Msg::Success } else { Msg::NoEffect };
    say(state, io, FMT_NL_S_NL, m).await;
}

/// Original: 0x11a550 (magic: Armageddon: every actor slot 0..0xfe but the hero (1) and the
/// ruler (5) is removed (0x10c1d6); the view flags and refresh; the eggs stop (A4-0x7afb = 1)
/// and the party is the hero alone (A4+0x4b00 = 1))
pub async fn armageddon(state: &RefCell<State>, io: &Io) {
    let mut s = state.borrow_mut();
    for a in 0..0xffi16 {
        if a != 1 && a != 5 {
            crate::objects::remove(&mut s, a);
        }
    }
    crate::map::view::update_view(&mut s);
    crate::gfx::view::refresh_view(&mut s, io);
    s.map.flag_7afb = 1;
    s.actors.party.size = 1;
}

/// Flips frame bit 0 of `o` (the wing beat).
fn flap(s: &mut State, o: Slot) {
    if let Some(w) = s.objects.type_frame.get_mut(o as usize) {
        *w ^= 0x400;
    }
}

/// Original: 0x11a0cc (magic: Wing Strike: a dragon sweeps the row of every foe in view)
///
/// The caster flashes. For each actor slot below 0x100 that exists, is alive (status 0x10
/// clear), visible (object status 0x02 clear), evil or chaotic (alignment 0x20 / 0x60) and
/// within 5 of the centre on both axes: the side it is on (east of the centre: row 0, else 1)
/// picks the step sign and the two frames from the A4-0x6b2c / -0x6b2a / -0x6b28 pairs; a
/// two-part dragon (0x19b) appears at the window's edge on the foe's row (head one row up) and
/// crosses the window in ten steps, beating its wings, the view redrawn each step; the step on
/// the foe's column hurts it by random 1..20 (0x1231be), the caster takes the experience and
/// the wound line follows. The dragon goes. None at all: the no-foes line (0x115a66).
///
/// The port waits one display frame per redraw (as X-ray does), so each step is shown.
///
/// Spec 13: the foes are those of the original's 11 × 11 box round the leader (Decision 3,
/// [`RULE_HALF`]); the dragon enters at the view's edge, [`HALF_W`] columns from the centre,
/// and crosses the view in `VIEW_W - 1` one-cell steps (Decision 5; ten in Original), showing
/// the original's ten frames ([`CROSSING_FRAMES`], [`shows_frame`]; Decision 7).
pub async fn wing_strike(state: &RefCell<State>, io: &Io) {
    let caster = super::cast::caster(&state.borrow());
    super::spells::flash_actor(state, io, caster, 0).await;
    let rows = seams::a4_bytes(io, -0x6b2c, 6);
    sweep(state, io, caster, &rows).await;
}

/// The display frames a dragon's crossing shows after its first: the original's, one per step
/// of its ten.
const CROSSING_FRAMES: i16 = 10;

/// Spec 13 Decision 7 (durations stay, rates scale): step `k` (1..=`steps`) of a crossing of
/// `steps` one-cell steps ends on a display frame when `k * 10 / steps` advances, so a crossing
/// shows [`CROSSING_FRAMES`] frames whatever its steps: every step in Original (ten steps), about
/// every fourth of Wide's 36.
fn shows_frame(k: i16, steps: i16) -> bool {
    k * CROSSING_FRAMES / steps != (k - 1) * CROSSING_FRAMES / steps
}

/// [`wing_strike`] after the caster's flash, with the A4-0x6b2c pairs `rows` (step sign of row 0
/// and 1, then the two frame pairs).
pub(super) async fn sweep(state: &RefCell<State>, io: &Io, caster: u16, rows: &[u8]) {
    let steps = VIEW_W as i16 - 1;
    let mut any = false;
    for a in 0..0x100u16 {
        let (x, y, cx, step, parts) = {
            let mut s = state.borrow_mut();
            let i = usize::from(a);
            let al = s.actors.attrs.status[i] & 0x60;
            let (x, y) = (s.objects.x(a as Slot) as i16, s.objects.y(a as Slot) as i16);
            let c = s.map.centre;
            let (cx, cy) = (c.x as i16, c.y as i16);
            if s.objects.type_frame[i] == 0
                || s.actors.attrs.status[i] & 0x10 != 0
                || s.objects.obj_status[i] & 2 != 0
                || (al != 0x20 && al != 0x60)
                || x < cx - RULE_HALF
                || x > cx + RULE_HALF
                || y < cy - RULE_HALF
                || y > cy + RULE_HALF
            {
                continue;
            }
            let half = HALF_W as i16;
            let row = usize::from(x - cx + half <= cx - x + half);
            let step = i16::from(rows[row] as i8);
            let (fa, fb) = (u16::from(rows[2 + row]), u16::from(rows[4 + row]));
            let z = u16::from(c.z);
            let sx = (cx - half * step) as u16;
            let head =
                super::area::new_object(&mut s, DRAGON_PART | fa << 10, sx, (y - 1) as u16, z);
            let tail = super::area::new_object(&mut s, DRAGON_PART | fb << 10, sx, y as u16, z);
            crate::map::view::update_view(&mut s);
            crate::gfx::view::refresh_view(&mut s, io);
            (x, y, cx, step, (head, tail))
        };
        io.vblank().await;
        for k in 1..=steps {
            let hit = {
                let mut s = state.borrow_mut();
                flap(&mut s, parts.0);
                flap(&mut s, parts.1);
                let z = u16::from(s.map.centre.z);
                let px = cx - (HALF_W as i16 - k) * step;
                crate::objects::move_to(&mut s, parts.0, px as u16, (y - 1) as u16, z);
                crate::objects::move_to(&mut s, parts.1, px as u16, y as u16, z);
                crate::map::view::update_view(&mut s);
                crate::gfx::view::refresh_view(&mut s, io);
                if px == x {
                    let d = s.rng.range(1, 0x14) as i16;
                    let exp = seams::hurt(&mut s, a, d);
                    seams::add_experience(&mut s, caster, exp);
                    true
                } else {
                    false
                }
            };
            if hit {
                seams::wound_line(state, io, a).await;
            }
            if shows_frame(k, steps) {
                io.vblank().await;
            }
        }
        any = true;
        let mut s = state.borrow_mut();
        crate::objects::remove(&mut s, parts.0);
        crate::objects::remove(&mut s, parts.1);
    }
    if !any {
        print_at(state, io, MSG_NO_FOES, &[]).await;
    }
}

//! The actor idle-frame pass of 0x10202e (`0x001020E4..0x00102BE8`).
//!
//! For each actor slot 0..256 with a non-zero type/frame word, on the view centre's level and
//! in the view (less than 6 tiles from the centre on both axes in Original, signed 16-bit
//! differences; spec 13 Decision 5: at most [`HALF_W`] and [`HALF_H`] tiles), while
//! [`GfxState::actor_anim_due`] (A4-0x7afa) is set and the actor's status byte (A4+0x3d0c) has
//! none of bits 0x16: pick a new frame by object type, drawing RNG values in the original's
//! order. `e` is the frame word being built (local -0xa), `base` the frame bits kept (local
//! -0xc, frame & 0xfc unless a type says otherwise); the slot gets
//! `type + ((base + (e & 3)) << 10)` at the end, with the type re-read (two cases rewrite it).

use super::GfxState;
use super::seams;
use crate::geom::{HALF_H, HALF_W};
use crate::objects::ObjectId;
use crate::state::State;

/// Actor slots the pass walks (`cmpi.w #0x100`).
const SLOTS: u16 = 0x100;
/// Bit 7 of the movement-flags byte (0x139588): animation runs upward.
const UP: u8 = 0x80;

/// `rng.range(0, hi)` (0x151f06 called with 0 and `hi`).
fn roll(state: &mut State, hi: u16) -> u16 {
    state.rng.range(0, hi)
}

/// Frame word (6 bits) of `id`.
fn frame_of(state: &State, id: ObjectId) -> u16 {
    seams::type_frame(state, id) >> 10
}

/// Rewrites the frame of `id`, keeping its type.
fn set_frame(state: &mut State, id: ObjectId, frame: u16) {
    let w = seams::type_frame(state, id);
    seams::set_type_frame(state, id, (w & 0x3ff).wrapping_add(frame << 10));
}

/// Runs the pass; true when any frame changed (the caller then rebuilds the window lists).
///
/// The pass reads only the direction tables and the "due" word of [`GfxState`]; it works on a
/// copy of those ([`GfxState::view`]) so the state stays borrowable.
pub(crate) fn actor_idle_frames(state: &mut State) -> bool {
    let gfx = &state.gfx.view();
    let mut changed = false;
    let c = state.map.centre;
    for slot in 0..SLOTS {
        let id = ObjectId(slot);
        let word = seams::type_frame(state, id);
        let Some(p) = state.objects.location(id) else {
            continue;
        };
        if word == 0 || p.z != c.z {
            continue;
        }
        let near = |a: u16, b: u16, half: usize| {
            usize::from((a as i16).wrapping_sub(b as i16).unsigned_abs()) < half + 1
        };
        if !near(p.x, c.x, HALF_W) || !near(p.y, c.y, HALF_H) {
            continue;
        }
        let ty = word & 0x3ff;
        if gfx.actor_anim_due == 0 || seams::actor_status(state, id) & 0x16 != 0 {
            continue;
        }
        let mut e = word >> 10;
        let mut base = e & 0xfc;
        if (0x178..=0x19a).contains(&ty) && ty != 0x184 {
            changed |= person(state, id, ty, &mut e, &mut base);
        } else {
            changed |= creature(state, gfx, id, ty, &mut e, &mut base);
        }
        let cur = seams::type_frame(state, id) & 0x3ff;
        let f = base.wrapping_add(e & 3);
        seams::set_type_frame(state, id, cur.wrapping_add(f << 10));
    }
    changed
}

/// The 3-frame idle cycle shared by people and type 0x16b: from frame 0 or 2 go to 1 (1 in 32),
/// from 1 go to a random 0..=2 (1 in 64).
fn idle_cycle(state: &mut State, e: &mut u16, phase: u16) -> bool {
    match phase {
        0 | 2 if roll(state, 0x1f) == 0 => {
            *e = 1;
            true
        }
        1 if roll(state, 0x3f) == 0 => {
            *e = roll(state, 2);
            true
        }
        _ => false,
    }
}

/// Types 0x178..=0x19a except 0x184 (`0x00102276..0x00102406`).
fn person(state: &mut State, id: ObjectId, ty: u16, e: &mut u16, base: &mut u16) -> bool {
    match ty {
        0x187 => {
            if roll(state, 3) == 0 {
                *e ^= 1;
                return true;
            }
            if roll(state, 0xf) == 0 {
                seams::set_type_frame(state, id, 0x181);
                *base = 8;
                *e = 0;
                return true;
            }
            false
        }
        0x188 => {
            let hit = roll(state, 1) != 0;
            if hit {
                *e ^= 1;
            }
            hit
        }
        0x17f => {
            *e &= 3;
            if roll(state, 1) != 0 && *e < 3 {
                *e += 1;
                if *e > 2 {
                    *e = 0;
                }
                return true;
            }
            false
        }
        _ => {
            let mut changed = idle_cycle(state, e, *e & 3);
            if ty == 0x181 && seams::worktype(state, id) == 0x94 && roll(state, 0xf) == 0 {
                seams::set_type_frame(state, id, 0x187);
                changed = true;
                *e = 0;
                *base = 0;
            }
            changed
        }
    }
}

/// `if roll(0, hi) == 0 { e ^= 1 }`.
fn flicker(state: &mut State, e: &mut u16, hi: u16) -> bool {
    let hit = roll(state, hi) == 0;
    if hit {
        *e ^= 1;
    }
    hit
}

/// Every other type (`0x0010240A..0x00102BA6`).
fn creature(
    state: &mut State,
    gfx: &GfxState,
    id: ObjectId,
    ty: u16,
    e: &mut u16,
    base: &mut u16,
) -> bool {
    let up = |state: &State| seams::movement_flags(state, id) & UP != 0;
    let set_up = |state: &mut State, on: bool| {
        let f = seams::movement_flags(state, id);
        seams::set_movement_flags(state, id, if on { f | UP } else { f & !UP });
    };
    match ty {
        0x16b => {
            let phase = *e % 3;
            *base = *e - phase;
            *e = phase;
            idle_cycle(state, e, phase)
        }
        0x16a => {
            let phase = *e % 12;
            *base = *e - phase;
            *e = phase;
            let mut changed = false;
            match phase {
                3 | 11 if roll(state, 0x1f) == 0 => {
                    *e = 7;
                    changed = true;
                }
                7 if roll(state, 0x3f) == 0 => {
                    *e = if roll(state, 1) != 0 { 3 } else { 0xb };
                    changed = true;
                }
                _ => {}
            }
            *base = base.wrapping_add(*e);
            *e = 0;
            changed
        }
        0x16f..=0x174 => flicker(state, e, 7),
        0x158 | 0x16c => {
            if roll(state, 1) == 0 {
                return false;
            }
            if up(state) {
                *e = e.wrapping_add(1);
                if *e as i16 >= 3 {
                    set_up(state, false);
                    *e = 3;
                }
            } else {
                *e = e.wrapping_sub(1);
                if *e as i16 <= 0 {
                    set_up(state, true);
                    *e = 0;
                }
            }
            true
        }
        0x163 | 0x15b | 0x168 => {
            if roll(state, 1) == 0 {
                return false;
            }
            *e = e.wrapping_add(1);
            if *e as i16 > 3 {
                *e = 0;
            }
            true
        }
        0x157 | 0x160 | 0x161 => {
            if ty != 0x157 && roll(state, 1) == 0 {
                return false;
            }
            *e = roll(state, 3);
            true
        }
        0x175 | 0x159 => {
            if roll(state, 1) == 0 {
                return false;
            }
            *e = if roll(state, 0xf) == 0 {
                3
            } else {
                roll(state, 2)
            };
            true
        }
        0x165 => {
            if *e == 0 || roll(state, 1) == 0 {
                return false;
            }
            *e = if up(state) {
                e.wrapping_sub(1)
            } else {
                e.wrapping_add(1)
            };
            if *e as i16 > 3 {
                set_up(state, true);
                *e = 2;
            }
            if (*e as i16) < 1 {
                set_up(state, false);
                *e = 2;
            }
            true
        }
        0x164 => {
            if *e as i16 >= 3 {
                return false;
            }
            if roll(state, 0x1f) == 0 {
                *e = 3;
                return true;
            }
            if roll(state, 1) == 0 {
                return false;
            }
            *e += 1;
            if *e > 2 {
                *e = 0;
            }
            true
        }
        0x16d => flicker(state, e, 7),
        0x176 => hydra(state, gfx, id, e),
        0x19b => serpent(state, gfx, id, e),
        0x156 | 0x15a | 0x15c..=0x15f | 0x166 | 0x169 | 0x1aa.. => {
            if roll(state, 7) != 0 || (ty == 0x1ac && roll(state, 3) != 0) {
                return false;
            }
            *e ^= 1;
            if let Some(other) = seams::other_half(state, id) {
                let f = frame_of(state, other);
                set_frame(state, other, f ^ 1);
            }
            true
        }
        _ => false,
    }
}

/// Map position (x, y) of `id` as words.
fn xy(state: &State, id: ObjectId) -> (i32, i32) {
    let p = state.objects.location(id).unwrap_or_default();
    (i32::from(p.x), i32::from(p.y))
}

/// Type 0x176 (`0x001027EC..0x001029B4`): first each neighbour in direction d = 0..8 that holds
/// an object of type 0x1a9 whose frame >> 2 is d steps that one's frame (1 in 2); then the actor
/// itself walks its frame up or down, turning at 0 and 3 (and at random in between).
fn hydra(state: &mut State, gfx: &GfxState, id: ObjectId, e: &mut u16) -> bool {
    let mut changed = false;
    let (x, y) = xy(state, id);
    let z = state.map.centre.z;
    for d in 0..8u16 {
        let (dx, dy) = (
            gfx.tables.dir_dx[usize::from(d)],
            gfx.tables.dir_dy[usize::from(d)],
        );
        for other in seams::objects_at(state, x + i32::from(dx), y + i32::from(dy), z) {
            let w = seams::type_frame(state, other);
            if w & 0x3ff != 0x1a9 || w >> 12 != d {
                continue;
            }
            if roll(state, 1) != 0 {
                let f = w >> 10;
                set_frame(state, other, (f & 0x1c) + ((f + 1) & 3));
                changed = true;
            }
            break;
        }
    }
    *e = frame_of(state, id);
    if roll(state, 1) == 0 {
        return changed;
    }
    let set_up = |state: &mut State, on: bool| {
        let f = seams::movement_flags(state, id);
        seams::set_movement_flags(state, id, if on { f | UP } else { f & !UP });
    };
    if *e == 0 || (*e < 3 && roll(state, 4) == 0) {
        set_up(state, true);
    }
    if *e == 3 || (*e as i16 > 0 && roll(state, 4) == 0) {
        set_up(state, false);
    }
    *e = if seams::movement_flags(state, id) & UP != 0 {
        e.wrapping_add(1)
    } else {
        e.wrapping_sub(1)
    };
    true
}

/// Type 0x19b (`0x001029B8..0x00102AEE`): flip frame bit 0 (1 in 2), then for d = 0, 2, 4, 6 the
/// body part 0x10b922 finds: a part with frame ≥ 0x18 takes bits 0..2 of `e`, any other flips
/// its frame bit 0 (1 in 8).
fn serpent(state: &mut State, _gfx: &GfxState, id: ObjectId, e: &mut u16) -> bool {
    let mut changed = flicker_odds(state, e);
    let (x, y) = xy(state, id);
    let z = state.map.centre.z;
    for d in (0..8u16).step_by(2) {
        let own = frame_of(state, id);
        let Some(part) = seams::serpent_part(state, x, y, z, d, own) else {
            continue;
        };
        let f = frame_of(state, part);
        if f >= 0x18 {
            set_frame(state, part, (f & 0x38) | (*e & 7));
        } else if roll(state, 7) == 0 {
            set_frame(state, part, f ^ 1);
            changed = true;
        }
    }
    changed
}

/// `if roll(0, 1) != 0 { e ^= 1 }` (the 0x19b head).
fn flicker_odds(state: &mut State, e: &mut u16) -> bool {
    let hit = roll(state, 1) != 0;
    if hit {
        *e ^= 1;
    }
    hit
}

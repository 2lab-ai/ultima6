//! Object helpers of hunks 5 and 15-17 used by the movement, schedule and talk paths.

use super::iter::{begin_at, next_at};
use super::{Objects, Slot};
use std::cell::RefCell;

use crate::engine::Io;
use crate::state::State;

/// Original: 0x10a4d6 (objects: frame of an actor type for animation step / direction `n`)
///
/// Types without an entry give 0.
pub fn actor_frame(kind: u16, n: i16) -> i16 {
    match kind {
        0xfc | 0xdd | 0x162 | 0x167 | 0x184 => n >> 1,
        0x15c..=0x15f | 0x166 | 0x169 | 0x19d | 0x16f..=0x174 | 0x156 | 0x15a | 0x188 => n,
        k if k >= 0x1aa => n,
        0x16a => n * 2 + n * 4 + 7,
        0x1a8 => n * 4 + 3,
        0x16b => (n >> 1) + n + 1,
        0x178..=0x183 | 0x199 | 0x19a => n * 2 + 1,
        _ => 0,
    }
}

/// Original: 0x1283b8 (objects: actor of party member `i`; member 0 while the leader index
/// equals the party size)
pub fn party_member(s: &State, i: usize) -> u8 {
    let p = &s.actors.party;
    let i = if p.leader_index == p.size { 0 } else { i };
    p.members.get(i).copied().unwrap_or(0)
}

/// Original: 0x1288ee (objects: x of `slot`, of slot 0 while the leader index equals the party
/// size)
pub fn member_x(s: &State, slot: Slot) -> u16 {
    let p = &s.actors.party;
    s.objects.x(if p.leader_index == p.size { 0 } else { slot })
}

/// Original: 0x128942 (objects: y of `slot`, of slot 0 while the leader index equals the party
/// size)
pub fn member_y(s: &State, slot: Slot) -> u16 {
    let p = &s.actors.party;
    s.objects.y(if p.leader_index == p.size { 0 } else { slot })
}

/// Original: 0x12899c (objects: `kind` is one of the 9 types at A4-0x670c)
pub fn in_type_set_670c(o: &Objects, kind: u16) -> bool {
    o.defs.type_set_670c.iter().take(9).any(|&k| k == kind)
}

/// Original: 0x128b0e (objects: mark an object and everything inside it as fine to take)
///
/// Slot -1 does nothing (the original sets bit 0 of the byte before the status table).
pub fn mark_takeable(o: &mut Objects, slot: Slot) {
    if slot < 0 {
        return;
    }
    o.obj_status[slot as usize] |= 0x01;
    let mut c = o.next_of(slot);
    while c >= 0 && o.place(c) != 0 {
        if !o.contained_in(c, slot) {
            return;
        }
        o.obj_status[c as usize] |= 0x01;
        c = o.next_of(c);
    }
}

/// Original: 0x12dcbe (objects: first object of `kind` at (x, y, z), -1)
pub fn object_of_kind_at(s: &mut State, x: u16, y: u16, z: u16, kind: u16) -> Slot {
    let mut c = begin_at(&mut s.objects, &s.tables, x, y, z);
    while c >= 0 && s.objects.kind(c) != kind {
        c = next_at(&mut s.objects, &s.tables);
    }
    c
}

/// Original: 0x1311d2 (objects: `n` varied: n − n/2 + two draws of 0..=n/2)
pub fn vary(s: &mut State, n: i16) -> i16 {
    let half = n >> 1;
    if half == 0 {
        return n;
    }
    let base = n.wrapping_sub(half);
    let a = s.rng.range(0, half as u16) as i16;
    let b = s.rng.range(0, half as u16) as i16;
    base.wrapping_add(a).wrapping_add(b)
}

/// Original: 0x131196 (objects: index of `kind` in the monster type list at A4-0x8000, -1)
pub fn monster_index(o: &Objects, kind: u16) -> i16 {
    o.defs
        .monster_types
        .iter()
        .take_while(|&&k| k != 0)
        .position(|&k| k == kind)
        .map_or(-1, |i| i as i16)
}

/// Original: 0x1301e2 (objects: open or close a passthrough (sliding wall section))
///
/// Closed (frame 0): the section slides one tile — up for type 0x116, else left — takes
/// frame | 2 and the opening line (0x12cf20) is printed. Open: unless the first object where
/// it closes is an actor (the refusal at 0x12cee2), it slides back, keeps frame bit 0 and
/// prints the closing line (0x12cf38). Slot -1 does nothing.
pub fn toggle_passthrough(s: &mut State, slot: Slot) {
    if slot < 0 {
        return;
    }
    let o = &s.objects;
    let tf = o.tf(slot);
    let (x, y) = (o.x(slot), o.y(slot));
    let z = u16::from(s.map.centre.z);
    let vertical = tf & 0x3ff == 0x116;
    let frame = tf >> 10;
    if frame == 0 {
        let (nx, ny) = if vertical {
            (x, y.wrapping_sub(1))
        } else {
            (x.wrapping_sub(1), y)
        };
        super::moves::move_to(s, slot, nx, ny, z);
        let t = &mut s.objects.type_frame[slot as usize];
        *t = (*t & 0x3ff).wrapping_add((frame | 2) << 10);
        super::seams::print(s, STR_OPEN, &[]);
        return;
    }
    let (nx, ny) = if vertical { (x, y + 1) } else { (x + 1, y) };
    let here = begin_at(&mut s.objects, &s.tables, nx, ny, z);
    if here != -1 && here < super::ACTOR_SLOTS as Slot {
        super::seams::print(s, STR_NOT_NOW, &[]);
        return;
    }
    super::moves::move_to(s, slot, nx, ny, z);
    let t = &mut s.objects.type_frame[slot as usize];
    *t = (*t & 0x3ff).wrapping_add((frame & 1) << 10);
    super::seams::print(s, STR_CLOSE, &[]);
}

/// Image addresses of the strings 0x1301e2 prints.
const STR_OPEN: u32 = 0x0012_cf20;
const STR_NOT_NOW: u32 = 0x0012_cee2;
const STR_CLOSE: u32 = 0x0012_cf38;

/// Port-only (spec 08, slice K2): the strings 0x1301e2 passes to `printf` as its format, for the
/// locale layer's catalogue ([`crate::locale::strings`]).
pub(super) const PRINTED: &[u32] = &[STR_OPEN, STR_NOT_NOW, STR_CLOSE];

/// 0x128ce8 (an actor's portrait, equipment and name, then a key unless a conversation runs):
/// ported in [`crate::inventory::show_actor`]; this entry keeps the objects-side signature.
pub async fn show_actor(state: &RefCell<State>, io: &Io, actor: Slot) {
    Box::pin(crate::inventory::show_actor(state, io, actor)).await
}

pub use super::commands::{blast_doors, explode_keg, spring_trap};

pub use super::commands::{dismount, mount_horse, parse_answer};
pub use super::commands::{equip_monster, hatch_egg, spawn_monster};

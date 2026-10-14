//! Object helpers of hunks 5 and 15-17 used by the movement, schedule and talk paths.

use super::iter::{begin_at, next_at};
use super::{Objects, Slot};
use std::cell::RefCell;

use crate::engine::Io;
use crate::state::State;
use crate::todo_stub;

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

/// Original: 0x128b0e (objects: mark an object and everything inside it "ok to take")
pub fn mark_takeable(o: &mut Objects, slot: Slot) {
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
/// frame | 2 and "Open the passthrough" is printed. Open: unless the first object where it
/// closes is an actor ("Not now!"), it slides back, keeps frame bit 0 and prints "Close the
/// passthrough".
pub fn toggle_passthrough(s: &mut State, slot: Slot) {
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

/// Stub of 0x128ce8 (objects: show an actor's portrait, equipment and name, wait for a key)
///
/// Stub contract: draws the actor's portrait (converse 0x133d3e) and equipment panel (ui
/// 0x133fe0 / 0x1113fe), prints its name (0x10eef8 or its converse name) and, unless A4-0x71b5
/// is set, awaits a key (0x10790a) — so the port's signature is async. Fallback: nothing shown,
/// no key consumed; the caller continues. Reached by Talk's start (0x114afa) and its opcode
/// executor (0x11404c), the side panel (0x103f64) and Look: the M1 Talk scenario reaches it,
/// and the missing key wait is a command-boundary difference there.
pub async fn show_actor(_state: &RefCell<State>, _io: &Io, _actor: Slot) {
    todo_stub!("0x128ce8 show an actor's portrait, equipment and name", ())
}

/// Stub of 0x12ba36 (objects: spring a trap: acid, poison, bomb or gas)
///
/// Stub contract: one RNG draw 0..=3 picks the effect, damages or poisons the actor and those in
/// the blast (combat 0x1231be / 0x1230ce, ui animation 0x1331b4), then removes the trap. Fallback:
/// the trap is removed without effect and no RNG draw is made. Not reached by the M1 scenarios.
pub fn spring_trap(s: &mut State, trap: Slot) {
    todo_stub!("0x12ba36 trap effects (acid, poison, bomb, gas)", ());
    super::moves::remove(s, trap);
}

/// Stub of 0x12db7e (objects: explode a powder keg)
///
/// Stub contract: removes the keg, damages the actors in the blast (ui 0x1331b4, combat
/// 0x1231be), chains to 0x12d9ee and refreshes the view. Fallback: the keg is removed only. Not
/// reached by the M1 scenarios.
pub fn explode_keg(s: &mut State, keg: Slot) {
    crate::usecmd::explode_keg(s, keg);
}

/// Stub of 0x12d9ee (objects: blow up kegs and doors around (x, y))
///
/// Stub contract: lights kegs (0xdf) and removes closed doors (0x12c / 0x129 / 0x12a / 0x12b)
/// in x − 2..=x + 2, y − 2..y + 2, printing "The door is blown up!". Fallback: nothing. Not
/// reached by the M1 scenarios.
pub fn blast_doors(s: &mut State, x: u16, y: u16) {
    crate::usecmd::blast_doors(s, x as i16, y as i16);
}

/// Stub of 0x12f6ee (objects: mount / place a horse)
///
/// Stub contract: moves the rider, sets its type to the mounted horse (0x1af) and places or
/// re-types the horse object, then rebuilds the tile cache and refreshes. Fallback: nothing.
/// Not reached by the M1 scenarios (no Use command).
pub fn mount_horse(s: &mut State, rider: Slot, x: u16, y: u16, frame: u16, here: u16) {
    crate::usecmd::mount_horse(s, rider, x, y, frame, here);
}

/// Stub of 0x12f8fe (objects: dismount: put the horse next to the rider, "No room!")
///
/// Stub contract: restores the rider's base type, removes the mounted-horse object and places a
/// horse on a free neighbouring tile (0x11b1c0), else prints "No room!". Fallback: nothing. Not
/// reached by the M1 scenarios.
pub fn dismount(s: &mut State) {
    let rider = s.actors.party.leader().map_or(-1, |l| l.slot());
    crate::usecmd::dismount(s, rider);
}

/// Stub of 0x128b98 (objects: parse a typed number or direction answer)
///
/// Stub contract: digits give a number; with `mode` 1 a trailing N / S (2: E / W) gives the
/// sign, any other letter 100; mode 0 with no digits gives -1 or, with A4-0x7a44 set, the value
/// read so far. Fallback -2 ("no answer"). Reached only from Use handlers (not M1).
pub fn parse_answer(_text: &str, _mode: i16) -> i16 {
    todo_stub!("0x128b98 parse a typed number or direction", -2)
}

/// Stub of 0x131646 (objects: create a monster of `kind` at (x, y, z) with stats)
///
/// Stub contract: takes a temporary-actor slot (0x10bec8), sets stats from the monster tables
/// (0x131196 / 0x1311d2, combat 0x122176), its frame and extra parts, and gives it equipment
/// (0x13121a). Fallback -1 (no monster). Reached by random encounters (0x1319b2) only.
pub fn spawn_monster(_s: &mut State, _kind: u16, _x: u16, _y: u16, _z: u16) -> Slot {
    todo_stub!("0x131646 create a monster with stats and equipment", -1)
}

/// Stub of 0x13121a (objects: give a new monster its equipment from the A4-0x7cbe table)
///
/// Stub contract: RNG-weighted items into the monster's inventory. Fallback: none given. Reached
/// only through 0x131646.
pub fn equip_monster(_s: &mut State, _monster: Slot, _index: i16) {
    todo_stub!("0x13121a give a new monster its equipment", ())
}

/// Stub of 0x1319b2 (objects: hatch an egg: spawn its monsters, "Shamino says, I hear something")
///
/// Stub contract: called by map 0x1321dc for every egg (type 0x14f) on the leader's level in the
/// 40 × 40 window each time the window is rebuilt (0x108d10, i.e. whenever walking shifts the
/// window), unless A4-0x7afb is set; with RNG checks it spawns the egg's monsters out of view
/// (0x131646) and may print Shamino's warning. Fallback: no monster and no RNG draw. Reached on
/// the M1 walk path: RNG parity diverges at the first window shift with an egg in range.
pub fn hatch_egg(_s: &mut State, _egg: Slot) {
    todo_stub!("0x1319b2 hatch an egg (spawn its monsters)", ())
}

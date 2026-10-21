//! Port-only (owner, 2026-10-21; .prd/hd-i18n-voice/ssot.md): Look at a plant by a gatehouse
//! door, next to the leader, gives the key of that door.
//!
//! In the original this key is given in conversation after the copy-protection question; it
//! opens the gatehouse where the cranks that lower the drawbridge are
//! (<https://wiki.ultimacodex.com/wiki/Key_locations_in_Ultima_VI>). The game makes it in code:
//! no object block or conversation script holds a key of that lock's quality. Here a plant by
//! the gatehouse gives one while the port holds none.
//!
//! Everything is read from the user's files at run time: a gatehouse door is a locked door with
//! a lock (a quality) within [`DOOR_REACH`] tiles of a drawbridge crank that has a bridge piece
//! of its quality (as 0x12e3f6 finds the bridge); a gatehouse plant is a plant tile within
//! [`PLANT_REACH`] tiles of such a door, on either side of it.

use super::keys::KEY;
use super::text::{queue, queue_at};
use super::world::{S_DOT, S_SEARCHING, name_prefix, refresh};
use crate::objects::{HEAD, OBJECT_SLOTS, Slot, TEMPORARY, create_on_map, move_to, shown_name};
use crate::state::State;

/// The drawbridge crank and a piece of the bridge (0x12e3f6 looks for a piece of the crank's
/// quality).
const CRANK: u16 = 0x120;
const BRIDGE: u16 = 0x10d;
/// Map tiles of the plants: every map tile one of the two plant entries of the look table
/// names.
pub(super) const PLANT_TILES: [u8; 14] = [
    0x05, 0x30, 0x31, 0x32, 0x33, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x4e,
];
/// A gatehouse door lies at most this many tiles from a crank on either axis.
pub(super) const DOOR_REACH: i32 = 4;
/// A gatehouse plant lies at most this many tiles from its door on either axis.
pub(super) const PLANT_REACH: i32 = 6;
/// Status of the new key: fine to take (bit 0), as the conversation's add-object gives it.
const TAKEABLE: u8 = 0x01;

/// Every slot in use, in list order (the map objects the port holds and what they hold).
fn list(s: &State) -> impl Iterator<Item = Slot> + '_ {
    let o = &s.objects;
    std::iter::successors(Some(o.next_of(HEAD)), move |&c| Some(o.next_of(c)))
        .take_while(|&c| c >= 0)
        .take(OBJECT_SLOTS)
}

/// The map objects on the leader's level.
fn on_level(s: &State) -> impl Iterator<Item = Slot> + '_ {
    let z = s.map.centre.z;
    list(s).filter(move |&c| s.objects.place(c) == 0 && s.objects.z(c) == z)
}

fn quality(s: &State, slot: Slot) -> u8 {
    (s.objects.qq_of(slot) >> 8) as u8
}

/// How far (x, y) lies from `slot`: the larger of the two axes, on words.
fn reach(s: &State, slot: Slot, x: u16, y: u16) -> i32 {
    let o = &s.objects;
    let d = |a: u16, b: u16| i32::from((a as i16).wrapping_sub(b as i16)).abs();
    d(x, o.x(slot)).max(d(y, o.y(slot)))
}

/// The gatehouse doors on the leader's level: a locked door (frame 8..=11, 0x12bbc0) with a
/// lock (quality ≠ 0; 0x12c0c2 opens none of quality 0 with a key) within [`DOOR_REACH`] of a
/// crank that has a drawbridge piece of its quality.
pub(super) fn doors(s: &State) -> Vec<Slot> {
    let level: Vec<Slot> = on_level(s).collect();
    let cranks: Vec<Slot> = level
        .iter()
        .copied()
        .filter(|&c| {
            s.objects.kind(c) == CRANK
                && level
                    .iter()
                    .any(|&b| s.objects.kind(b) == BRIDGE && quality(s, b) == quality(s, c))
        })
        .collect();
    level
        .iter()
        .copied()
        .filter(|&d| {
            crate::magic::is_door(s, s.objects.kind(d))
                && (8..=11).contains(&(s.objects.tf(d) >> 10))
                && quality(s, d) != 0
                && cranks
                    .iter()
                    .any(|&c| reach(s, c, s.objects.x(d), s.objects.y(d)) <= DOOR_REACH)
        })
        .collect()
}

/// The gatehouse door the map tile `tile` at (x, y) is a plant of: a plant tile within
/// [`PLANT_REACH`] of a gatehouse door; the nearest such door (the lower slot on a tie). `None`
/// for any other tile.
pub(super) fn plant_door(s: &State, x: u16, y: u16, tile: u8) -> Option<Slot> {
    if !PLANT_TILES.contains(&tile) {
        return None;
    }
    doors(s)
        .into_iter()
        .map(|d| (reach(s, d, x, y), d))
        .filter(|&(r, _)| r <= PLANT_REACH)
        .min()
        .map(|(_, d)| d)
}

/// A key of quality `q` among the objects the port holds: on the map, in a container or in an
/// inventory.
fn key_held(s: &State, q: u8) -> bool {
    list(s).any(|c| s.objects.kind(c) == KEY && quality(s, c) == q)
}

/// Port-only (module doc): Look at the map tile `tile` at (x, y) within one tile of the leader.
/// At a gatehouse plant ([`plant_door`]), while the port holds no key of its door's quality,
/// one key of that quality is made on the tile (an ordinary map object, taken as any other and
/// kept: not temporary, so 0x10c49c does not drop it) and found as the search finds what it
/// spills (0x129544): the leader re-placed on top, the view refreshed, the search lead-in, the
/// key's count or article and name, the full stop, queued. True when it gave one; otherwise
/// nothing changes.
pub(super) fn give_key(s: &mut State, x: u16, y: u16, tile: u8) -> bool {
    let Some(door) = plant_door(s, x, y, tile) else {
        return false;
    };
    let q = quality(s, door);
    if key_held(s, q) {
        return false;
    }
    let z = u16::from(s.map.centre.z);
    let key = create_on_map(&mut s.objects, KEY, TAKEABLE, u16::from(q) << 8, x, y, z);
    let Some(status) = usize::try_from(key)
        .ok()
        .and_then(|i| s.objects.obj_status.get_mut(i))
    else {
        return false;
    };
    *status &= !TEMPORARY;
    let p = &s.actors.party;
    let leader = Slot::from(p.members[usize::from(p.leader_index).min(15)]);
    let (lx, ly) = (s.objects.x(leader), s.objects.y(leader));
    move_to(s, leader, lx, ly, z);
    refresh(s);
    queue_at(s, S_SEARCHING, Vec::new());
    name_prefix(s, key);
    let name = shown_name(s, key).into_bytes();
    queue(s, name, Vec::new());
    queue_at(s, S_DOT, Vec::new());
    true
}

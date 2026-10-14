//! Moving, removing and creating objects.

use super::iter::{find_child, first_child, next_child};
use super::parts::{move_monster_parts, move_serpent, update_vine_frames};
use super::{
    ACTOR_SLOTS, IN_INVENTORY, NONE, Objects, READIED, Slot, TEMPORARY, WHERE_MASK, seams,
};
use crate::state::State;

/// Type the move routines refuse (0x10c9d2 always, 0x10cda8 unless allowed).
const FIXED_TYPE: u16 = 0x4c;
/// Silver-serpent head (moves one orthogonal step, never onto its own body).
pub(crate) const SERPENT: u16 = 0x19d;
/// Serpent body segment (cannot be removed on its own).
pub(crate) const SERPENT_BODY: u16 = 0x19c;
/// Tangle vine (0x10b2cc re-frames every vine after any vine changes).
pub(crate) const VINE: u16 = 0x177;
/// Types whose parts 0x10b9ca moves with them (plus every type from 0x1aa).
pub(crate) fn has_parts(kind: u16) -> bool {
    matches!(kind, 0x176 | 0x19b | 0x19c) || kind >= 0x1aa
}

/// Sets [`Objects::carried_changed`] when command 0x92 runs and `test` holds for the member
/// the command acts for (the check at the top of 0x10c1d6, 0x10c9d2 and 0x10be02).
fn note_carried(s: &mut State, test: impl Fn(&Objects, Slot) -> bool) {
    if seams::command_code(s) != 0x92 {
        return;
    }
    let i = usize::from(seams::command_member(s));
    let member = s.actors.party.members.get(i).copied().unwrap_or(0);
    if test(&s.objects, Slot::from(member)) {
        s.ui.panel.dirty = true;
    }
}

/// Original: 0x10c9d2 (objects: move an object or actor to map tile (x, y, z))
///
/// Takes the object and its contents out of the list and links them in front of the objects
/// at the new tile. Refuses (-1) type 0x4c, an object not in the list, and a serpent head
/// that would step diagonally, not at all, more than one tile, or back onto its body. Returns
/// `slot` on success. Afterwards multi-tile monsters drag their parts along, and an actor
/// moved after load runs 0x1032b6.
pub fn move_to(s: &mut State, slot: Slot, x: u16, y: u16, z: u16) -> Slot {
    if slot < 0 {
        return NONE;
    }
    note_carried(s, |o, m| o.contained_in(slot, m));
    let o = &s.objects;
    let kind = o.kind(slot);
    if kind == FIXED_TYPE {
        return NONE;
    }
    let dx = (o.x(slot) as i16).wrapping_sub(x as i16);
    let dy = (o.y(slot) as i16).wrapping_sub(y as i16);
    if kind == SERPENT && slot < ACTOR_SLOTS as Slot {
        let f = usize::from(o.tf(slot) >> 10);
        let defs = &o.defs;
        if (dx != 0 && dy != 0)
            || (dx == 0 && dy == 0)
            || dx.abs() > 1
            || dy.abs() > 1
            || (dx == defs.dx(f) && dy == defs.dy(f))
        {
            return NONE;
        }
    }
    let mask = if z != 0 { 0xff } else { 0x3ff };
    let (x, y) = (x & mask, y & mask);
    let (ox, oy, oz) = (o.x(slot), o.y(slot), o.z(slot));
    let pred = o.predecessor(slot);
    if pred < 0 {
        return NONE;
    }
    let c = s.map.centre;
    let near = |px: u16, py: u16| {
        let (px, py, lx, ly) = (px as i16, py as i16, c.x as i16, c.y as i16);
        (lx - 5..=lx + 5).contains(&px) && (ly - 5..=ly + 5).contains(&py)
    };
    if near(ox, oy) || near(x, y) {
        s.map.view_dirty = 1;
    }
    let o = &mut s.objects;
    let last = o.last_inside(slot);
    o.set_next(pred, o.next_of(last));
    o.unlink_cell(ox, oy, u16::from(oz), slot, o.next_of(pred));
    let p = o.list_pos(x, y, true);
    o.set_next(last, o.next_of(p));
    o.set_next(p, slot);
    o.obj_status[slot as usize] &= !WHERE_MASK;
    o.set_xyz(slot, x, y, z);
    if let Some(cell) = o.cell(x, y, 0x3ff) {
        o.grid[cell] = slot;
    }
    let actor = slot < ACTOR_SLOTS as Slot;
    if kind == VINE {
        update_vine_frames(s);
    } else if kind == SERPENT && actor {
        move_serpent(s, slot, ox, oy, false);
    } else if actor && has_parts(kind) {
        move_monster_parts(s, slot, ox, oy, false);
    } else if kind == 0x165 {
        s.objects.type_frame[slot as usize] &= 0x3ff;
    }
    if actor && s.map.flag_784e != 0 {
        if let Some(f) = s.actors.attrs.movement_flags.get_mut(slot as usize) {
            *f &= !0x08;
        }
        seams::after_actor_moved(s, slot);
    }
    slot
}

/// Original: 0x10c1d6 (objects: remove an object from the world)
///
/// Contents drop to where the object was (onto its tile, or into its own container / owner);
/// the slot is cleared, unlinked and returned to its free chain (actor slots to the monster
/// chain). A serpent body segment is never removed; removing a multi-tile monster or a serpent
/// head removes its parts. [`Objects::last_moved_child`] = the last content moved out, or -1.
pub fn remove(s: &mut State, slot: Slot) {
    if slot < 0 {
        return;
    }
    s.objects.last_moved_child = NONE;
    note_carried(s, |o, m| o.contained_in(slot, m));
    let kind = s.objects.kind(slot);
    if kind == SERPENT_BODY {
        return;
    }
    let (x, y) = (s.objects.x(slot), s.objects.y(slot));
    if slot < ACTOR_SLOTS as Slot {
        if has_parts(kind) {
            move_monster_parts(s, slot, x, y, true);
        } else if kind == SERPENT {
            move_serpent(s, slot, x, y, true);
        }
    }
    loop {
        let child = first_child(&mut s.objects, slot);
        // A child that refuses to move (type 0x4c) is found again: the original loops forever
        // here; the port stops.
        if child < 0 || child == s.objects.last_moved_child {
            break;
        }
        s.objects.last_moved_child = child;
        let o = &s.objects;
        if o.place(slot) == 0 {
            let z = u16::from(o.z(slot));
            move_to(s, child, x, y, z);
        } else {
            let (owner, place) = (o.parent(slot), o.place(slot));
            move_into(s, child, owner, place);
        }
    }
    let o = &mut s.objects;
    o.type_frame[slot as usize] = 0;
    let pred = o.predecessor(slot);
    if pred >= 0 {
        o.set_next(pred, o.next_of(slot));
        if slot < ACTOR_SLOTS as Slot {
            o.set_next(slot, o.free_actors.0);
            o.free_actors = (slot, o.free_actors.1 + 1);
        } else {
            o.set_next(slot, o.free.0);
            o.free = (slot, o.free.1 + 1);
        }
    }
    let z = u16::from(o.z(slot));
    o.unlink_cell(x, y, z, slot, o.next_of(pred));
    if kind == VINE {
        update_vine_frames(s);
    }
}

/// Original: 0x10cda8 (objects: move an object into `owner` with location bits `place`)
///
/// `place` = 0x08 into a container, 0x10 into an actor's inventory, 0x18 readied. The object
/// and its contents are linked directly after `owner`. Refuses (-1) an object into itself or
/// into something it contains, and type 0x4c unless [`Objects::allow_type_4c`] is set.
pub fn move_into(s: &mut State, slot: Slot, owner: Slot, place: u8) -> Slot {
    let o = &mut s.objects;
    if slot < 0 || owner < 0 {
        return NONE;
    }
    if o.kind(slot) == FIXED_TYPE && o.allow_type_4c == 0 {
        return NONE;
    }
    if slot == owner || o.contained_in(owner, slot) {
        return NONE;
    }
    let pred = o.predecessor(slot);
    let last = o.last_inside(slot);
    o.set_next(pred, o.next_of(last));
    o.set_next(last, o.next_of(owner));
    o.set_next(owner, slot);
    let (x, y, z) = (o.x(slot), o.y(slot), u16::from(o.z(slot)));
    o.unlink_cell(x, y, z, slot, o.next_of(pred));
    let st = &mut o.obj_status[slot as usize];
    *st = (*st & !WHERE_MASK) | place;
    o.set_parent(slot, owner);
    slot
}

/// Original: 0x10c810 (objects: put the actor a body stands for back where the body lies)
///
/// The body's quality byte is an actor number. The actor moves to the map tile of the body
/// (or of whatever holds it), takes the body's contents (type 0x4c readied, the rest into its
/// inventory; type 0x4c allowed meanwhile) and the body is removed. Returns the actor; -1 for
/// slot -1 or a body whose holders never reach the map (port bound, nothing changes).
pub fn actor_from_body(s: &mut State, body: Slot) -> Slot {
    if body < 0 {
        return NONE;
    }
    s.objects.allow_type_4c = 1;
    let actor = (s.objects.qq_of(body) >> 8) as Slot;
    // The original climbs to the map with no bound (0x10c838-0x10c868); a body whose holders
    // never reach it raises nobody here.
    let root = s.objects.map_holder(body);
    if root < 0 {
        s.objects.allow_type_4c = 0;
        return NONE;
    }
    let (x, y, z) = (s.objects.x(root), s.objects.y(root), s.objects.z(root));
    move_to(s, actor, x, y, u16::from(z));
    let mut last = NONE;
    loop {
        let c = first_child(&mut s.objects, body);
        // As in 0x10c1d6: a child that cannot move would be found forever; the port stops.
        if c < 0 || c == last {
            break;
        }
        last = c;
        let place = if s.objects.kind(c) == FIXED_TYPE {
            READIED
        } else {
            IN_INVENTORY
        };
        move_into(s, c, actor, place);
    }
    remove(s, body);
    s.objects.allow_type_4c = 0;
    actor
}

/// Original: 0x10bcee (objects: create a map object at (x, y, z), -1 when no slot is free)
///
/// The new object gets `status` | 0x20 (temporary), is linked in front of the objects at the
/// tile and heads its grid cell.
pub fn create_on_map(
    o: &mut Objects,
    tf: u16,
    status: u8,
    qq: u16,
    x: u16,
    y: u16,
    z: u16,
) -> Slot {
    let mask = if z != 0 { 0xff } else { 0x3ff };
    let (x, y) = (x & mask, y & mask);
    let slot = o.free_slot(1);
    if slot < 0 {
        return NONE;
    }
    let prev = o.list_pos(x, y, true);
    o.pop_free();
    o.obj_status[slot as usize] = status | TEMPORARY;
    o.set_xyz(slot, x, y, z);
    o.type_frame[slot as usize] = tf;
    o.qq[slot as usize] = qq;
    o.link_after(prev, slot);
    if let Some(c) = o.cell(x, y, 0x3ff) {
        o.grid[c] = slot;
    }
    slot
}

/// Original: 0x10be02 (objects: create an object inside `owner`, -1 when no slot is free)
///
/// `status` carries the location bits; every type + frame but 0x18c also gets 0x20
/// (temporary). The object is linked directly after its owner.
pub fn create_in(s: &mut State, tf: u16, status: u8, qq: u16, owner: Slot) -> Slot {
    note_carried(s, |_, m| m == owner);
    let o = &mut s.objects;
    let slot = o.free_slot(1);
    if slot < 0 {
        return NONE;
    }
    o.pop_free();
    let i = slot as usize;
    o.obj_status[i] = status;
    if tf != 0x18c {
        o.obj_status[i] |= TEMPORARY;
    }
    o.set_parent(slot, owner);
    o.type_frame[i] = tf;
    o.qq[i] = qq;
    o.link_after(owner, slot);
    slot
}

/// Original: 0x10bec8 (objects: create a temporary actor (monster) of `tf` at (x, y, z))
///
/// Takes a slot from 0xe0..0x100 (-1 when none is free): status 0x21, actor status 0,
/// worktype 8, base type = type, quality / quantity 1. A new tangle vine re-frames the vines.
pub fn create_actor(s: &mut State, tf: u16, x: u16, y: u16, z: u16) -> Slot {
    let mask = if z != 0 { 0xff } else { 0x3ff };
    let (x, y) = (x & mask, y & mask);
    let o = &mut s.objects;
    let slot = if o.free_actors.1 != 0 {
        o.free_actors.0
    } else {
        NONE
    };
    if slot < 0 {
        return NONE;
    }
    let prev = o.list_pos(x, y, true);
    o.free_actors.0 = o.next_of(slot);
    o.free_actors.1 -= 1;
    let i = slot as usize;
    o.obj_status[i] = 0x21;
    o.set_xyz(slot, x, y, z);
    o.type_frame[i] = tf;
    o.qq[i] = 1;
    o.link_after(prev, slot);
    if let Some(c) = o.cell(x, y, 0x3ff) {
        o.grid[c] = slot;
    }
    let attrs = &mut s.actors.attrs;
    set(&mut attrs.status, i, 0);
    set(&mut s.actors.worktype, i, 8);
    set(&mut attrs.base_type_frame, i, tf);
    if tf == VINE {
        update_vine_frames(s);
    }
    slot
}

fn set<T: Copy>(v: &mut [T], i: usize, x: T) {
    if let Some(e) = v.get_mut(i) {
        *e = x;
    }
}

/// Original: 0x10bc4c (objects: create an object from an 8-byte record's fields)
///
/// On-map records (location bits 0) go through 0x10bcee at their x, y, z; the others through
/// 0x10be02 with the parent slot in location bytes 0-1.
pub fn create_from_record(s: &mut State, rec: [u8; 8]) -> Slot {
    let status = rec[0];
    let tf = u16::from_be_bytes([rec[4], rec[5]]);
    let qq = u16::from_be_bytes([rec[6], rec[7]]);
    let pos = u6_formats::position::Position::from_packed([rec[1], rec[2], rec[3]]);
    if status & WHERE_MASK == 0 {
        create_on_map(
            &mut s.objects,
            tf,
            status,
            qq,
            pos.x,
            pos.y,
            u16::from(pos.z),
        )
    } else {
        let owner = (u16::from(rec[2]) << 8 | u16::from(rec[1])) as Slot;
        create_in(s, tf, status, qq, owner)
    }
}

/// Original: 0x10cf7c (objects: give `owner` `count` of type `kind`, stacking where it can)
///
/// `count` = quality << 8 | quantity. Gold (kind 4 of 0x10cef2) adds the whole word to a gold
/// object already inside, or to a new one. Quantity stacks (kind 2) fill an existing stack of
/// the same quality up to 0x90 and spill into new ones. Anything else creates `quantity`
/// objects. New objects go into an actor's inventory (readied for type 0x4c) or a container;
/// for type 0x4c an actor's single-slot readied object moves to the inventory first. Returns
/// the last object touched, -1 if none.
pub fn add_to_owner(s: &mut State, owner: Slot, kind: u16, count: u16) -> Slot {
    if owner < 0 {
        return NONE;
    }
    let quality = (count as i16) >> 8;
    let mut qty = count & 0xff;
    let stack = super::names::stack_kind(&s.objects, kind);
    let mut place = super::IN_CONTAINER;
    if owner < ACTOR_SLOTS as Slot {
        place = IN_INVENTORY;
        if kind == FIXED_TYPE {
            place = READIED;
            let mut c = first_child(&mut s.objects, owner);
            while c >= 0 {
                if s.objects.place(c) == READIED && seams::ready_slot(s, c) == 1 {
                    let st = &mut s.objects.obj_status[c as usize];
                    *st = (*st & !WHERE_MASK) | IN_INVENTORY;
                    break;
                }
                c = next_child(&mut s.objects);
            }
        }
    }
    let status = place | 0x01;
    let mut last = NONE;
    match stack {
        4 => {
            last = find_child(&mut s.objects, owner, kind as i16, -1);
            if last < 0 {
                last = create_in(s, kind, status, 0, owner);
            }
            if last >= 0 {
                let w = &mut s.objects.qq[last as usize];
                *w = w.wrapping_add(count);
            }
        }
        2 => {
            s.objects.skip_readied = 1;
            last = find_child(&mut s.objects, owner, kind as i16, quality);
            s.objects.skip_readied = 0;
            while qty != 0 {
                if last < 0 {
                    last = create_in(s, kind, status, (quality as u16) << 8, owner);
                }
                if last < 0 {
                    break;
                }
                let have = s.objects.qq[last as usize] & 0xff;
                let w = &mut s.objects.qq[last as usize];
                if have + qty <= 0x90 {
                    *w = (*w & 0xff00) | ((have + qty) & 0xff);
                    qty = 0;
                } else {
                    qty -= 0x90 - have;
                    *w = (*w & 0xff00) | 0x90;
                    last = NONE;
                }
            }
        }
        _ => {
            for _ in 0..qty {
                last = create_in(s, kind, status, (quality as u16) << 8, owner);
                if last < 0 {
                    break;
                }
            }
        }
    }
    last
}

/// Original: 0x10d16e (objects: take `count` of type `kind` out of `owner`, at any depth)
///
/// `count` = quality << 8 | quantity (gold: the whole word; quality 0 = any). Matching objects
/// are removed while the rest covers them; the next one gives up the remainder (gold from its
/// word, quantity stacks from their quantity byte) and ends the walk. Returns the object the
/// walk ended on, -1 when it ran out.
pub fn take_from_owner(s: &mut State, owner: Slot, kind: u16, count: u16) -> Slot {
    if owner < 0 {
        return NONE;
    }
    let quality = (count as i16) >> 8;
    let stack = super::names::stack_kind(&s.objects, kind);
    let mut left = if stack == 4 {
        count as i16
    } else {
        (count & 0xff) as i16
    };
    let mut changed = false;
    let mut c = first_child(&mut s.objects, owner);
    while c >= 0 && s.objects.place(c) != 0 && s.objects.contained_in(c, owner) {
        let o = &s.objects;
        let quality_ok =
            stack == 4 || quality == 0 || i16::from((o.qq_of(c) >> 8) as u8) == quality;
        if o.kind(c) != kind || !quality_ok {
            c = o.next_of(c);
            continue;
        }
        let n = super::names::quantity(o, c) as i16;
        if left >= n {
            left -= n;
            let next = o.next_of(c);
            remove(s, c);
            changed = true;
            c = next;
            continue;
        }
        let w = &mut s.objects.qq[c as usize];
        if stack == 4 {
            *w = w.wrapping_sub(left as u16);
        } else if stack == 2 {
            *w = (*w & 0xff00) | (w.wrapping_sub(left as u16) & 0xff);
        }
        changed = true;
        break;
    }
    if changed {
        note_carried(s, |_, m| m == owner);
    }
    c
}

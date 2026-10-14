//! Equipment slots and weights (hunk 8): what an object readies into, what it weighs, and the
//! per-actor totals with the equipment cells 0x1112cc fills.

use crate::objects::{Slot, first_child, next_child, stack_kind};
use crate::state::State;

/// Torch: its frame 1 (lit) weighs as one torch whatever its count.
const TORCH: u16 = 0x5a;
/// Weight byte 0xff = cannot be carried; 0x1110f6 answers this instead.
const IMMOVABLE: i16 = 10000;

/// The weight byte of object type `ty` (0x13a540, `tileflag`; 0 past the table).
pub(crate) fn type_weight(s: &State, ty: u16) -> i16 {
    i16::from(
        s.tables
            .tileflags
            .weight
            .get(usize::from(ty))
            .copied()
            .unwrap_or(0),
    )
}

/// Original: 0x110f96 (ui: the equipment slot an object readies into, -1 if none)
///
/// By the object's tile (base tile of its type + frame): boots 7; rings 9; amulets 1; helms 0;
/// armour 4; two-handed weapons 8; shields 5; the 33 tiles of A4-0x720c 2 (the weapon hand).
pub fn ready_slot(s: &State, obj: Slot) -> i16 {
    let o = &s.objects;
    let ty = o.kind(obj);
    let base = s
        .tables
        .basetile
        .base
        .get(usize::from(ty))
        .copied()
        .unwrap_or(0);
    let tile = base.wrapping_add(o.tf(obj) >> 10) as i16;
    let any = |set: &[i16]| set.contains(&tile);
    let within = |a: i16, b: i16| (a..=b).contains(&tile);
    if any(&[0x21a, 0x21b]) {
        7
    } else if tile == 0x258 || within(0x37d, 0x37f) {
        9
    } else if tile == 0x219 || within(0x250, 0x252) || any(&[0x217, 0x101]) {
        1
    } else if within(0x200, 0x207) {
        0
    } else if within(0x210, 0x216) || any(&[0x218, 0x219, 0x28c, 0x28e, 0x29d, 0x257]) {
        4
    } else if any(&[0x228, 0x229, 0x231, 0x235]) || within(0x22b, 0x22e) {
        8
    } else if within(0x208, 0x20f) || tile == 0x222 {
        5
    } else if s
        .inventory
        .tables
        .hand_tiles
        .iter()
        .take(33)
        .any(|&t| t as i16 == tile)
    {
        2
    } else {
        -1
    }
}

/// Original: 0x1110c8 (ui: `ty` is one of the 9 types at A4-0x71ca)
pub fn counted_type(s: &State, ty: i16) -> bool {
    s.inventory
        .tables
        .counted
        .iter()
        .take(9)
        .any(|&t| t as i16 == ty)
}

/// Original: 0x1110f6 (ui: weight of one object in tenths of a stone, its count included)
///
/// The count: gold its whole quality / quantity word, a stack its quantity byte (a lit torch
/// counts 1), else 1; the stack kind is asked of the type alone (frame 0). The types of 0x1110c8
/// weigh a tenth per item. A result of exactly 0xff (an immovable weight byte) is 10000.
pub fn object_weight(s: &State, obj: Slot) -> i16 {
    let o = &s.objects;
    let ty = o.kind(obj);
    let count = match stack_kind(o, ty) {
        4 => o.qq_of(obj) as i16,
        2 if ty == TORCH && o.tf(obj) >> 10 == 1 => 1,
        2 => i16::from(o.qq_of(obj) as u8),
        _ => 1,
    };
    let product = type_weight(s, ty).wrapping_mul(count);
    let w = if counted_type(s, ty as i16) {
        product / 10
    } else {
        product
    };
    if w == 0xff { IMMOVABLE } else { w }
}

/// Original: 0x1111ca (ui: weight of everything inside `obj`)
///
/// The run of list entries after `obj` that are contained and inside it at any depth (contents
/// follow their container in the list), each weighed by 0x1110f6.
pub fn contents_weight(s: &State, obj: Slot) -> i16 {
    let o = &s.objects;
    let mut sum: i16 = 0;
    let mut n = o.next_of(obj);
    while n >= 0 && o.place(n) != 0 && o.contained_in(n, obj) {
        sum = sum.wrapping_add(object_weight(s, n));
        n = o.next_of(n);
    }
    sum
}

/// Original: 0x1112cc (ui: weights, list count and equipment cells of an actor or container)
///
/// Clears the list count (A4+0x1340) and the carried weight, and for an actor the readied
/// weight. Each object directly inside: readied ones add their type's weight byte and take their
/// slot (both hands: slot 2 with the marker 1 in slot 5 and the two-handed flag of the member,
/// or of the open container; a second one-hand weapon or shield goes to the other hand; a second
/// ring to slot 6); the others add their weight with contents and count one list entry. The
/// member index is searched among the first 8 roster slots only (8 when not found).
pub fn carried_weight(s: &mut State, obj: Slot) {
    s.ui.panel.inventory_count = 0;
    s.inventory.carried_weight = 0;
    if obj < 0x100 {
        s.inventory.equip_weight = 0;
    }
    let member = (0..8)
        .find(|&i| i16::from(s.actors.party.members[i]) == obj)
        .unwrap_or(8);
    let mut c = first_child(&mut s.objects, obj);
    while c >= 0 {
        if s.objects.place(c) == crate::objects::READIED {
            let ty = s.objects.kind(c);
            let w = type_weight(s, ty);
            let inv = &mut s.inventory;
            inv.equip_weight = inv.equip_weight.wrapping_add(w);
            let mut slot = ready_slot(s, c);
            let inv = &mut s.inventory;
            if slot == 8 {
                slot = 2;
                inv.set_equip(5, 1);
                if s.ui.panel.open_container == 0 {
                    s.ui.panel.member_flags[member] = 1;
                } else {
                    s.ui.panel.container_flag = true;
                }
            } else if slot == 2 && inv.equip(2) != 0 {
                slot = 5;
            } else if slot == 5 && inv.equip(5) != 0 {
                slot = 2;
            } else if slot == 9 {
                slot = if inv.equip(3) != 0 { 6 } else { 3 };
            }
            s.inventory.set_equip(slot, c);
        } else {
            let w = object_weight(s, c).wrapping_add(contents_weight(s, c));
            let inv = &mut s.inventory;
            inv.carried_weight = inv.carried_weight.wrapping_add(w);
            let p = &mut s.ui.panel;
            p.inventory_count = p.inventory_count.wrapping_add(1);
        }
        c = next_child(&mut s.objects);
    }
}

/// Original: 0x111882 (ui: tenths to the stones shown: rounded half up, but any non-zero weight
/// below one stone shows as 1)
pub fn stones(tenths: i16) -> i16 {
    let (q, r) = (tenths / 10, tenths % 10);
    if q != 0 {
        q + i16::from(r >= 5)
    } else {
        q + i16::from(r != 0)
    }
}

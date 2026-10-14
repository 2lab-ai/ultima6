//! Fixtures shared by the modules' tests (integration, WU-P7).

use crate::objects::{NONE, OBJECT_SLOTS, Slot};
use crate::state::State;

/// Test fixtures place objects by writing the slot tables; this links them into the object list
/// the way load state and the region load leave it, keeping their slot numbers: the slots reset
/// (0x10ed44), the actors linked (0x10edb4), every other placed slot taken off the free chain and
/// inserted at its list position, then the window around the centre (0x108d10's origin, the
/// grid 0x10dea4). Safe to run again after a fixture moves or adds objects.
pub(crate) fn link_placed_objects(s: &mut State) {
    let placed: Vec<Slot> = (0x101..OBJECT_SLOTS as Slot)
        .filter(|&i| s.objects.type_frame[i as usize] != 0)
        .collect();
    crate::objects::init_slots(&mut s.objects);
    crate::objects::link_actors(&mut s.objects);
    let o = &mut s.objects;
    for &slot in &placed {
        let (mut prev, mut c) = (NONE, o.free.0);
        while c >= 0 && c != slot {
            prev = c;
            c = o.next_of(c);
        }
        if c == slot {
            let n = o.next_of(slot);
            if prev < 0 {
                o.free.0 = n;
            } else {
                o.set_next(prev, n);
            }
            o.free.1 -= 1;
        }
        let p = o.list_pos(o.x(slot), o.y(slot), true);
        o.set_next(slot, o.next_of(p));
        o.set_next(p, slot);
    }
    let c = s.map.centre;
    let (ox, oy) = (c.x.wrapping_sub(16) & 0x3f8, c.y.wrapping_sub(16) & 0x3f8);
    s.map.window_origin = [ox, oy];
    s.objects.set_window(ox, oy, c.z);
}

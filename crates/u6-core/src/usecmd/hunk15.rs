//! The hunk-15 handlers the Use switch reaches: doors, chests, keys, torches, fixed lights, the
//! gem and the secret door (0x12bbc0..0x12c68a). They sit in the objects-command hunk but only
//! Use (and the lock spells, through 0x12bbc0 / 0x12bdfa) calls them.

use super::defs::{MSG_NO_EFFECT, MSG_NOTHING, MSG_OUT_OF_RANGE};
use super::{Ux, dist, leader, quality, quantity, seams, set_frame, set_quantity};
use crate::objects::{IN_CONTAINER, IN_INVENTORY, READIED, Slot, WHERE_MASK};
use crate::state::State;

const STR_CLOSED: u32 = 0x0012_87f6;
const STR_CANT_LOCK_OPEN: u32 = 0x0012_8800;
const STR_LOCKED_BANG: u32 = 0x0012_8820;
const STR_MAGIC_LOCKED: u32 = 0x0012_882a;
const STR_OPENED: u32 = 0x0012_883e;
const STR_UNLOCKED: u32 = 0x0012_8848;
const STR_LOCKED: u32 = 0x0012_8854;
const STR_ON: u32 = 0x0012_885c;
const FMT_BANG: u32 = 0x0012_8860; // "%s!\n"
const STR_KEY_BROKE: u32 = 0x0012_8866;
const STR_NO_HAND: u32 = 0x0012_8874;
const STR_TORCH_LIT: u32 = 0x0012_8896;
const STR_FIREPLACE: u32 = 0x0012_88a6;
const STR_BRAZIER: u32 = 0x0012_88b0;
const STR_CANDELABRA: u32 = 0x0012_88b8;
const STR_CAMPFIRE: u32 = 0x0012_88c4;
const STR_CANDLE: u32 = 0x0012_88ce;
const STR_DOUSED: u32 = 0x0012_88d6;
const STR_LIT: u32 = 0x0012_88e4;
const FMT_LINE: u32 = 0x0012_83de; // "%s\n"
const FMT_NL_LINE: u32 = 0x0012_859a; // "\n%s\n"
/// "A torch burned out." (A4 - 0x756f).
const STR_BURNED_OUT: u32 = 0x756f;

/// Trap object (type 0x151) and the quality 0x12bbc0 / 0x12bdfa look for inside.
const TRAP: (i16, i16) = (0x151, 0x16);

/// The trap inside `obj` springs (objects 0x12ba36), else "opened!".
async fn trap_or_opened(ux: Ux<'_>, obj: Slot) {
    let trap = {
        let mut s = ux.s();
        crate::objects::find_child(&mut s.objects, obj, TRAP.0, TRAP.1)
    };
    if trap != -1 {
        crate::objects::spring_trap(&mut ux.s(), trap);
        ux.flush().await;
    } else {
        ux.print(STR_OPENED, &[]).await;
    }
}

/// Toggles frame bits `x` of `obj`.
fn flip(s: &mut State, obj: Slot, x: u16) {
    let f = s.objects.tf(obj) >> 10;
    set_frame(s, obj, f ^ x);
}

/// Original: 0x12bbc0 (objects: open / close / lock / unlock a door)
///
/// Frames 0-3 open, 4-7 closed, 8-11 locked, 12-15 magically locked. Plain use (`key`,
/// `magic_lock`, `magic_unlock` all 0) closes an open door and opens a closed one (a trap
/// inside springs instead of "opened!"); a key locks a closed door and unlocks a locked one;
/// the lock spell turns closed or locked into magically locked, the unlock spell undoes that.
pub async fn door(ux: Ux<'_>, obj: Slot, key: i16, magic_lock: i16, magic_unlock: i16) {
    ux.put(b'\n').await;
    let f = ux.s().objects.tf(obj) >> 10;
    if f < 4 {
        if key == 0 && magic_lock == 0 && magic_unlock == 0 {
            ux.print(STR_CLOSED, &[]).await;
            flip(&mut ux.s(), obj, 4);
        } else {
            ux.print(STR_CANT_LOCK_OPEN, &[]).await;
        }
    } else if f < 8 {
        if key != 0 {
            ux.print(STR_LOCKED_BANG, &[]).await;
            flip(&mut ux.s(), obj, 0xc);
        } else if magic_lock != 0 {
            ux.print(STR_MAGIC_LOCKED, &[]).await;
            flip(&mut ux.s(), obj, 8);
        } else {
            trap_or_opened(ux, obj).await;
            flip(&mut ux.s(), obj, 4);
        }
    } else if f < 12 {
        if key != 0 {
            ux.print(STR_UNLOCKED, &[]).await;
            flip(&mut ux.s(), obj, 0xc);
        } else if magic_lock != 0 {
            ux.print(STR_MAGIC_LOCKED, &[]).await;
            flip(&mut ux.s(), obj, 4);
        } else {
            ux.print(STR_LOCKED, &[]).await;
        }
    } else if magic_unlock != 0 {
        ux.print(STR_UNLOCKED, &[]).await;
        flip(&mut ux.s(), obj, 8);
    } else {
        ux.print(STR_MAGIC_LOCKED, &[]).await;
    }
}

/// Original: 0x12bdfa (objects: open / close / lock / unlock a chest)
///
/// Frame 0 open, 1 closed, 2 locked, 3 magically locked. Opening springs a trap inside or
/// prints "opened!", then searches the chest's tile (0x129544: the contents come out); anything
/// that does not apply prints "No effect". A chest carried by someone redraws the inventory.
pub async fn chest(ux: Ux<'_>, obj: Slot, key: i16, magic_lock: i16, magic_unlock: i16) {
    ux.put(b'\n').await;
    let f = ux.s().objects.tf(obj) >> 10;
    let plain = key == 0 && magic_lock == 0 && magic_unlock == 0;
    let mut set = None;
    match f {
        0 if plain => {
            ux.print(STR_CLOSED, &[]).await;
            set = Some(1);
        }
        1 if plain => {
            trap_or_opened(ux, obj).await;
            set_frame(&mut ux.s(), obj, 0);
            let (x, y) = {
                let s = ux.s();
                (s.objects.x(obj) as i16, s.objects.y(obj) as i16)
            };
            seams::search(ux.state, ux.io, obj, x, y).await;
        }
        1 if key != 0 => {
            ux.print(STR_LOCKED_BANG, &[]).await;
            set = Some(2);
        }
        1 | 2 if key == 0 && magic_lock != 0 => {
            ux.print(STR_MAGIC_LOCKED, &[]).await;
            set = Some(3);
        }
        2 if key != 0 => {
            ux.print(STR_UNLOCKED, &[]).await;
            set = Some(1);
        }
        3.. if magic_unlock != 0 => {
            ux.print(STR_UNLOCKED, &[]).await;
            set = Some(1);
        }
        _ => ux.say(FMT_LINE, MSG_NO_EFFECT).await,
    }
    let carried = {
        let mut s = ux.s();
        if let Some(frame) = set {
            set_frame(&mut s, obj, frame);
        }
        s.objects.place(obj) != 0
    };
    if carried {
        seams::redraw_inventory(&mut ux.s());
    }
}

/// Original: 0x12c062 (objects: a lock pick breaks: 1..=30 below (45 − dexterity) / 2)
pub fn pick_breaks(s: &mut State, dexterity: i16) -> bool {
    let r = s.rng.range(1, 0x1e) as i16;
    r < (0x2d - dexterity) / 2
}

/// Original: 0x12c092 (objects: an intelligence check: 1..=30 below (45 − intelligence) / 2)
pub fn fumbles(s: &mut State, intelligence: i16) -> bool {
    let r = s.rng.range(1, 0x1e) as i16;
    r < (0x2d - intelligence) / 2
}

/// Original: 0x12c0c2 (objects: use a key (0x40) or lock pick (0x3f) on a door or chest)
///
/// "On " and a target (cross-hair); nothing selected → "nothing". A map target must be next to
/// the leader. Only doors 0x129..0x12c and chests 0x62 take keys. A pick breaks on a failed
/// dexterity check (one fewer, or gone); a key fits a door whose quality is its own, a pick
/// one of quality 0; then the door or chest is locked or unlocked ([`door`] / [`chest`] with
/// `key` 1).
pub async fn key(ux: Ux<'_>, key_obj: Slot) {
    let (lead, kind) = {
        let s = ux.s();
        (leader(&s), s.objects.kind(key_obj))
    };
    ux.print(STR_ON, &[]).await;
    {
        let mut s = ux.s();
        if s.ui.input.targeting != 0 {
            crate::ui::cursor::panel_cursor_in(&mut s, ux.io, 0);
            s.ui.input.targeting = 1;
        }
    }
    let k = ux.key().await;
    let sel = ux.s().ui.input.selection;
    if k != crate::ui::wait::REPEAT || sel == -1 {
        ux.say(FMT_LINE, MSG_NOTHING).await;
        return;
    }
    ux.name(sel).await;
    ux.put(b'\n').await;
    let (t, far) = {
        let s = ux.s();
        let l = Slot::from(lead);
        let (lx, ly) = (s.objects.x(l) as i16, s.objects.y(l) as i16);
        let (tx, ty) = s.ui.input.target;
        let far = s.objects.place(sel) == 0 && (dist(tx, lx) > 1 || dist(ty, ly) > 1);
        (s.objects.kind(sel), far)
    };
    if far {
        ux.say(FMT_BANG, MSG_OUT_OF_RANGE).await;
        return;
    }
    if !(0x129..=0x12c).contains(&t) && t != 0x62 {
        ux.say(FMT_LINE, MSG_NO_EFFECT).await;
        return;
    }
    if kind == 0x3f {
        let broke = {
            let mut s = ux.s();
            let dex = crate::actors::leaves::dexterity(&s, u16::from(lead)) as i16;
            pick_breaks(&mut s, dex)
        };
        if broke {
            ux.print(STR_KEY_BROKE, &[]).await;
            let mut s = ux.s();
            if quantity(&s, key_obj) == 1 {
                crate::objects::remove(&mut s, key_obj);
            } else {
                let q = quantity(&s, key_obj).wrapping_sub(1);
                set_quantity(&mut s, key_obj, q);
            }
            if s.ui.panel.mode == 0x92 {
                seams::redraw_inventory(&mut s);
            }
            return;
        }
    }
    let fits = {
        let s = ux.s();
        let door_q = quality(&s, sel);
        (kind == 0x40 && door_q != 0 && quality(&s, key_obj) == door_q)
            || (kind == 0x3f && door_q == 0)
    };
    if !fits {
        ux.say(FMT_NL_LINE, MSG_NO_EFFECT).await;
    } else if t == 0x62 {
        Box::pin(chest(ux, sel, 1, 0, 0)).await;
    } else {
        door(ux, sel, 1, 0, 0).await;
    }
}

/// Original: 0x12c326 (objects: light a torch, or a lit one burns out)
///
/// A lit torch is removed ("A torch burned out."). Otherwise a free hand is needed (A4+0x1364 /
/// A4+0x136a). From a stack one torch is split off (on the map at the stack's tile, else
/// readied by the member the panel shows); a carried torch goes into the free hand (readied,
/// 0x11121e). The torch is lit (status 0x80, frame 1, 200 turns in the quantity byte), the
/// clock runs 0 minutes (0x10368a: the light level), "Torch is lit.".
pub async fn torch(ux: Ux<'_>, obj: Slot) {
    let lit = ux.s().objects.status(obj) & 0x80 != 0;
    if lit {
        crate::objects::remove(&mut ux.s(), obj);
        ux.put(b'\n').await;
        ux.print_a4(STR_BURNED_OUT).await;
        return;
    }
    let busy = {
        let s = ux.s();
        let h = seams::hands(&s);
        h[0] != 0 && h[1] != 0
    };
    if busy {
        ux.print(STR_NO_HAND, &[]).await;
    } else {
        light_torch(&mut ux.s(), obj);
        ux.print(STR_TORCH_LIT, &[]).await;
    }
    let mut s = ux.s();
    if s.ui.panel.mode == 0x92 {
        seams::redraw_inventory(&mut s);
    }
}

/// The state part of 0x12c326 after the hand check.
fn light_torch(s: &mut State, obj: Slot) {
    let mut torch = obj;
    let q = quantity(s, obj);
    if q > 1 {
        let place = s.objects.place(obj);
        torch = if place == 0 {
            let tf = 0x5a | (s.objects.tf(obj) >> 10) << 10;
            let qq = u16::from(quality(s, obj)) << 8 | 1;
            let (x, y) = (s.objects.x(obj), s.objects.y(obj));
            let z = u16::from(s.map.centre.z);
            crate::objects::create_on_map(&mut s.objects, tf, place, qq, x, y, z)
        } else {
            let m = s.actors.party.members[(s.ui.panel.member as usize) & 0xf];
            crate::objects::create_in(s, 0x5a, READIED, 1, Slot::from(m))
        };
        set_quantity(s, obj, q - 1);
    }
    let place = s.objects.place(torch);
    if place == IN_INVENTORY || place == IN_CONTAINER {
        let h = seams::hands(s);
        seams::set_hand(s, usize::from(h[0] != 0), torch);
        if let Some(st) = s.objects.obj_status.get_mut(torch as usize) {
            *st = (*st & !WHERE_MASK) | READIED;
        }
        let m = s.actors.party.members[(s.ui.panel.member as usize) & 0xf];
        seams::ready_changed(s, Slot::from(m));
    }
    if let Some(st) = usize::try_from(torch)
        .ok()
        .and_then(|i| s.objects.obj_status.get_mut(i))
    {
        *st |= 0x80;
    }
    set_frame(s, torch, 1);
    set_quantity(s, torch, 200);
    crate::actors::clock::tick(s, 0);
}

/// Original: 0x12c54a (objects: peer through a gem: the map view (ui 0x13410a), one gem used up)
pub async fn gem(ux: Ux<'_>, obj: Slot) {
    seams::peer_map(ux.state, ux.io).await;
    let mut s = ux.s();
    if obj != 0 {
        let q = quantity(&s, obj);
        if q > 1 {
            set_quantity(&mut s, obj, q - 1);
        } else {
            crate::objects::remove(&mut s, obj);
        }
    }
    if s.ui.panel.mode == 0x92 {
        seams::redraw_inventory(&mut s);
    }
}

/// Original: 0x12c594 (objects: light or douse a candle, candelabra, fireplace, brazier,
/// campfire)
///
/// Prints its name, then frame bit 0 off → lit ("is lit."), on → doused (frame keeps bit 1
/// only, "is doused.").
pub async fn fixed_light(ux: Ux<'_>, obj: Slot) {
    let (kind, f) = {
        let s = ux.s();
        (s.objects.kind(obj), s.objects.tf(obj) >> 10)
    };
    ux.put(b'\n').await;
    let name = match kind {
        0x7a => Some(STR_CANDLE),
        0x91 => Some(STR_CANDELABRA),
        0xa4 => Some(STR_FIREPLACE),
        0xce => Some(STR_BRAZIER),
        0xfd => Some(STR_CAMPFIRE),
        _ => None,
    };
    if let Some(n) = name {
        ux.print(n, &[]).await;
    }
    if f & 1 == 0 {
        set_frame(&mut ux.s(), obj, f | 1);
        ux.print(STR_LIT, &[]).await;
    } else {
        set_frame(&mut ux.s(), obj, f & 2);
        ux.print(STR_DOUSED, &[]).await;
    }
}

/// Original: 0x12c68a (objects: open / close a secret door: frame bit 0 toggles, bit 1 kept)
pub fn secret_door(s: &mut State, obj: Slot) {
    let f = s.objects.tf(obj) >> 10;
    set_frame(s, obj, if f & 1 == 0 { f | 1 } else { f & 2 });
}

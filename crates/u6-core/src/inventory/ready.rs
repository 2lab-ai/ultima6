//! Ready 0x112052 and Unready 0x1123d6 (the panel codes 0xa7 / 0xa8) with their hunk-8 helpers.

use std::cell::RefCell;

use super::panel::panel_contents;
use super::{seams, weight};
use crate::actors::timer;
use crate::engine::Io;
use crate::image::A4;
use crate::main_loop::{game_string, leader_name, pointed_string};
use crate::objects::{
    IN_INVENTORY, READIED, Slot, WHERE_MASK, add_to_owner, create_in, display_name, find_child,
    move_into, remove,
};
use crate::state::State;
use crate::ui::printf::{Arg, printf};

/// Image strings (code hunk 8).
const STR_READY: u32 = 0x0011_0c14;
const STR_TORCH: u32 = 0x0011_0c1c;
const STR_NAME_LINE: u32 = 0x0011_0baa;
const STR_CANT: u32 = 0x0011_0c24;
const STR_HEAVY: u32 = 0x0011_0c38;
const STR_NO_PLACE: u32 = 0x0011_0c46;
const STR_PROMPT: u32 = 0x0011_0c5a;
const STR_LINE: u32 = 0x0011_0c62;
const STR_NEWLINE: u32 = 0x0011_0c68;
const STR_UNREADY: u32 = 0x0011_0c6a;
/// Original: A4-0x6c3c (pointer to "Magical energy prevents you from removing the amulet.")
const PTR_AMULET: u32 = 0x6c3c;
/// Original: A4-0x756f ("A torch burned out.")
const STR_BURNED_OUT: u32 = 0x756f;

/// Object types with special handling.
const TORCH: u16 = 0x5a;
const AMULET: u16 = 0x4c;
const INVISIBILITY_RING: u16 = 0x102;
const SPARKLE_TYPE: u16 = 0x51;
/// Object status bit 1: invisible.
const INVISIBLE: u8 = 0x02;

/// `printf` of the image string at `addr`.
async fn say(state: &RefCell<State>, io: &Io, addr: u32, args: &[Arg<'_>]) {
    let fmt = game_string(io, addr);
    printf(state, io, &fmt, args).await;
}

/// The command prompt with the leader's name (`\n%s:\n%>`, 0x110c5a).
async fn prompt(state: &RefCell<State>, io: &Io) {
    let name = leader_name(&state.borrow());
    say(state, io, STR_PROMPT, &[Arg::Str(&name)]).await;
}

/// The actor of the member the panel shows (A4+0x47ec + A4-0x7a3e).
fn shown_actor(s: &State) -> Slot {
    let m = usize::try_from(s.ui.panel.member).unwrap_or(0).min(15);
    Slot::from(s.actors.party.members[m])
}

/// A4-0x7b65 is one byte of the objlist block (the clock's fade timer) that the window render
/// also reads; the port keeps it twice (`actors.timers`, `gfx.sparkle`) and writes both.
fn set_sparkle(s: &mut State, v: u8) {
    s.actors.timers[timer::FADE] = v;
    s.gfx.sparkle = v;
}

/// Original: 0x112052 (inventory: Ready the selected object (A4-0x2f04) for the member shown)
///
/// "Ready-" and the name (a torch prints "torch"); refused when it has no slot, when its weight
/// byte would take the readied weight past 10 × strength, or when the slot is taken (a second
/// one-hand weapon or shield goes to the free hand, a two-handed one needs both and leaves the
/// marker 1 in slot 5, a second ring goes to slot 6). The object moves readied into the actor
/// that holds it; a torch is taken off its stack and a new single torch is readied. The ring of
/// invisibility (0x102) makes the actor invisible and 0x51 starts the sparkle, both with the
/// actor's flash (0x132338). The panel contents are redrawn; the prompt follows in every case.
pub async fn ready(state: &RefCell<State>, io: &Io) {
    let (actor, obj, ty, strength) = {
        let s = state.borrow();
        let actor = shown_actor(&s);
        let obj = s.ui.input.selection;
        let ty = s.objects.kind(obj);
        (actor, obj, ty, s.actors.attrs.strength[actor as usize])
    };
    say(state, io, STR_READY, &[]).await;
    if ty == TORCH {
        say(state, io, STR_TORCH, &[]).await;
    } else {
        let name = display_name(&state.borrow(), obj);
        say(state, io, STR_NAME_LINE, &[Arg::Str(name.as_bytes())]).await;
    }
    let refusal = place(&mut state.borrow_mut(), obj, ty, strength);
    if let Some(msg) = refusal {
        say(state, io, msg, &[]).await;
    } else {
        panel_contents(state, io, actor).await;
        let mut s = state.borrow_mut();
        match s.objects.kind(obj) {
            INVISIBILITY_RING => {
                s.objects.obj_status[actor as usize] |= INVISIBLE;
                seams::flash_actor(&mut s, io, actor, true);
            }
            SPARKLE_TYPE => {
                set_sparkle(&mut s, 0x14);
                seams::flash_actor(&mut s, io, actor, true);
            }
            _ => {}
        }
    }
    prompt(state, io).await;
}

/// The checks and the move of [`ready`]; the refusal message when it does not happen.
fn place(s: &mut State, obj: Slot, ty: u16, strength: u8) -> Option<u32> {
    let mut slot = weight::ready_slot(s, obj);
    if slot == -1 {
        return Some(STR_CANT);
    }
    let load = weight::type_weight(s, ty).wrapping_add(s.inventory.equip_weight);
    if load > i16::from(strength).wrapping_mul(10) {
        return Some(STR_HEAVY);
    }
    let inv = &s.inventory;
    let (hand, other) = (inv.equip(2) != 0, inv.equip(5) != 0);
    match slot {
        8 if hand => slot = 2,
        8 if other => slot = 5,
        8 => {
            slot = 2;
            s.inventory.set_equip(5, 1);
            let m = usize::try_from(s.ui.panel.member).unwrap_or(0).min(15);
            s.ui.panel.member_flags[m] = 1;
        }
        2 if hand && !other => slot = 5,
        5 if other && !hand => slot = 2,
        9 => slot = if s.inventory.equip(3) != 0 { 6 } else { 3 },
        _ => {}
    }
    if s.inventory.equip(slot) != 0 {
        return Some(STR_NO_PLACE);
    }
    let mut owner = obj;
    while owner >= 0x100 {
        owner = s.objects.parent(owner);
    }
    let readied = if ty == TORCH {
        let q = &mut s.objects.qq[obj as usize];
        let n = (*q as u8).wrapping_sub(1);
        *q = (*q & 0xff00) | u16::from(n);
        if n == 0 {
            remove(s, obj);
        }
        create_in(s, TORCH, READIED, 1, owner)
    } else {
        let r = move_into(s, obj, owner, READIED);
        let p = &mut s.ui.panel;
        p.inventory_count = p.inventory_count.wrapping_sub(1);
        r
    };
    s.inventory.set_equip(slot, readied);
    None
}

/// Original: 0x1122dc (inventory: what Unready meets first: 1 = the amulet of submission (0x4c)
/// stays, with its message; 2 = a lit torch burns out (removed); else 0)
pub async fn unready_check(state: &RefCell<State>, io: &Io, obj: Slot) -> i16 {
    let (place, ty, frame) = {
        let s = state.borrow();
        let o = &s.objects;
        (o.place(obj), o.kind(obj), o.tf(obj) >> 10)
    };
    if place != READIED {
        return 0;
    }
    if ty == AMULET {
        let text = pointed_string(io, A4.wrapping_sub(PTR_AMULET));
        say(state, io, STR_LINE, &[Arg::Str(&text)]).await;
        return 1;
    }
    if ty == TORCH && frame == 1 {
        remove(&mut state.borrow_mut(), obj);
        say(state, io, STR_NEWLINE, &[]).await;
        let text = game_string(io, A4.wrapping_sub(STR_BURNED_OUT));
        printf(state, io, &text, &[]).await;
        return 2;
    }
    0
}

/// Original: 0x11236c (inventory: a readied ring of invisibility or 0x51 leaves its effect on
/// `actor` (visible again / no sparkle) with the flash; 3 when one did, else 0)
pub fn release_effect(s: &mut State, io: &Io, obj: Slot, actor: Slot) -> i16 {
    if s.objects.place(obj) != READIED {
        return 0;
    }
    match s.objects.kind(obj) {
        INVISIBILITY_RING => {
            if let Some(st) = s.objects.obj_status.get_mut(actor as usize) {
                *st &= !INVISIBLE;
            }
            seams::flash_actor(s, io, actor, true);
            3
        }
        SPARKLE_TYPE => {
            set_sparkle(s, 0);
            seams::flash_actor(s, io, actor, true);
            3
        }
        _ => 0,
    }
}

/// Original: 0x111fe0 (inventory: the equipment slot holding `obj`, -1 if none)
pub fn equipped_index(s: &State, obj: Slot) -> i16 {
    (0..8).find(|&i| s.inventory.equip(i) == obj).unwrap_or(-1)
}

/// Original: 0x1123d6 (inventory: Unready the selected object (A4-0x2f04) for the member shown)
///
/// Nothing for the selection 1 (the both-hands marker). "Unready-" and the name; the amulet of
/// submission stays readied; a lit torch burns out; otherwise the slot is cleared, the effects
/// end, a both-hands weapon frees slot 5, and the object goes back to the inventory (a torch
/// onto the actor's torch stack when it has one). The panel contents are redrawn and the prompt
/// printed.
pub async fn unready(state: &RefCell<State>, io: &Io) {
    let obj = state.borrow().ui.input.selection;
    if obj == 1 {
        return;
    }
    let name = display_name(&state.borrow(), obj);
    say(state, io, STR_UNREADY, &[Arg::Str(name.as_bytes())]).await;
    let (ty, actor) = {
        let s = state.borrow();
        (s.objects.kind(obj), shown_actor(&s))
    };
    let r = unready_check(state, io, obj).await;
    if r != 1 {
        let mut s = state.borrow_mut();
        let i = equipped_index(&s, obj);
        s.inventory.set_equip(i, 0);
        if r != 2 {
            release_effect(&mut s, io, obj, actor);
            if weight::ready_slot(&s, obj) == 8 {
                let m = usize::try_from(s.ui.panel.member).unwrap_or(0).min(15);
                s.ui.panel.member_flags[m] = 0;
                s.inventory.set_equip(5, 0);
            }
            let stacked = ty == TORCH && {
                let stack = find_child(&mut s.objects, actor, TORCH as i16, 0);
                s.objects.status(stack) & IN_INVENTORY != 0
            };
            if stacked {
                remove(&mut s, obj);
                add_to_owner(&mut s, actor, TORCH, 1);
            } else {
                let st = &mut s.objects.obj_status[obj as usize];
                *st = (*st & !WHERE_MASK) | IN_INVENTORY;
                let p = &mut s.ui.panel;
                p.inventory_count = p.inventory_count.wrapping_add(1);
            }
        }
    }
    panel_contents(state, io, actor).await;
    prompt(state, io).await;
}

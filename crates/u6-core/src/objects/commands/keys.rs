//! Use helpers of hunk 15: keys and lock picks (0x12c0c2 with 0x12c062), the type list
//! 0x128b6a, the frame toggle 0x12c68a and the dexterity roll 0x12c092.

use std::cell::RefCell;

use super::get::message;
use super::seams;
use super::text::{print, print_at, putc};
use super::world::{S_LINE, chest, door};
use crate::engine::Io;
use crate::objects::moves::remove;
use crate::objects::{Slot, display_name};
use crate::state::State;
use crate::ui::printf::Arg;

const S_ON: u32 = 0x0012_885c;
const S_BANG: u32 = 0x0012_8860; // "%s!\n"
const S_KEY_BROKE: u32 = 0x0012_8866;
const S_NL_LINE_NL: u32 = 0x0012_859a;
const P_NOTHING: i32 = -0x6c58;
const P_OUT_OF_RANGE: i32 = -0x6c60;
const P_NO_EFFECT: i32 = super::world::P_NO_EFFECT;
/// Original: A4-0x66dc (85 words: the types 0x128b6a accepts)
const TYPE_LIST: i32 = -0x66dc;
const LOCK_PICK: u16 = 0x3f;
const KEY: u16 = 0x40;
const CHEST: u16 = 0x62;
const TARGET_DONE: u16 = crate::ui::wait::REPEAT;

/// Original: 0x128b6a (objects: `kind` is one of the 85 types at A4-0x66dc)
pub fn in_type_list(s: &State, kind: u16) -> bool {
    (0..0x55).any(|i| s.objcmds.data.a4_word(TYPE_LIST + 2 * i) == kind)
}

/// Original: 0x12c062 (objects: a lock pick breaks: one draw 1..=30 below (45 − dexterity) / 2)
pub fn pick_breaks(s: &mut State, dexterity: i16) -> bool {
    let r = s.rng.range(1, 30) as i16;
    r < (45 - dexterity) / 2
}

/// Original: 0x12c092 (objects: the same roll for another Use handler (0x12fca2): one draw
/// 1..=30 below (45 − `n`) / 2)
pub fn fails_roll(s: &mut State, n: i16) -> bool {
    let r = s.rng.range(1, 30) as i16;
    r < (45 - n) / 2
}

/// Original: 0x12c68a (objects: toggle `obj` lit: frame bit 0 set, or cleared keeping bit 1)
pub fn toggle_lit(s: &mut State, obj: Slot) {
    let t = &mut s.objects.type_frame[obj as usize];
    let f = *t >> 10;
    let f = if f & 1 == 0 { f | 1 } else { f & 2 };
    *t = (*t & 0x3ff) + (f << 10);
}

/// Original: 0x12c0c2 (objects: use key or lock pick `key` "On" a door or chest)
///
/// "On " and the target key (anything but a confirmed target: "nothing"). The target's name.
/// On the map it must be within one tile of the leader ("Out of range!"); only doors
/// (0x129..=0x12c) and chests take keys ("No effect"). A lock pick may break (0x12c062 against
/// the leader's dexterity, 0x121ff8: "Key broke.", one of the stack gone). A key whose quality
/// matches the lock's (not 0), or a pick on a lock of quality 0, locks or unlocks it (0x12bdfa
/// / 0x12bbc0 with the key flag); otherwise "No effect".
pub async fn use_key(state: &RefCell<State>, io: &Io, key: Slot) {
    let (actor, ktype) = {
        let s = state.borrow();
        let p = &s.actors.party;
        let a = Slot::from(p.members[usize::from(p.leader_index).min(15)]);
        (a, s.objects.kind(key))
    };
    print_at(state, io, S_ON, &[]).await;
    {
        let mut s = state.borrow_mut();
        if s.ui.input.targeting != 0 {
            crate::ui::cursor::panel_cursor_in(&mut s, io, 0);
            s.ui.input.targeting = 1;
        }
    }
    let k = crate::ui::wait::read_key(state, io).await;
    let sel = state.borrow().ui.input.selection;
    if k != TARGET_DONE || sel == -1 {
        let msg = message(&state.borrow(), P_NOTHING);
        print_at(state, io, S_LINE, &[Arg::Str(&msg)]).await;
        return;
    }
    let name = display_name(&state.borrow(), sel).into_bytes();
    print(state, io, &name, &[]).await;
    putc(state, io, b'\n').await;
    let (kind, far) = {
        let s = state.borrow();
        let (tx, ty) = s.ui.input.target;
        let far = |t: i16, a: u16| t.wrapping_sub(a as i16).wrapping_abs() > 1;
        let away = far(tx, s.objects.x(actor)) || far(ty, s.objects.y(actor));
        (s.objects.kind(sel), s.objects.place(sel) == 0 && away)
    };
    if far {
        let msg = message(&state.borrow(), P_OUT_OF_RANGE);
        print_at(state, io, S_BANG, &[Arg::Str(&msg)]).await;
        return;
    }
    if !((0x129..=0x12c).contains(&kind) || kind == CHEST) {
        let msg = message(&state.borrow(), P_NO_EFFECT);
        print_at(state, io, S_LINE, &[Arg::Str(&msg)]).await;
        return;
    }
    let broke = ktype == LOCK_PICK && {
        let mut s = state.borrow_mut();
        let dex = crate::actors::leaves::dexterity(&s, actor as u16) as i16;
        pick_breaks(&mut s, dex)
    };
    if broke {
        print_at(state, io, S_KEY_BROKE, &[]).await;
        let mut s = state.borrow_mut();
        if s.objects.qq_of(key) & 0xff == 1 {
            remove(&mut s, key);
        } else {
            s.objects.qq[key as usize] = s.objects.qq_of(key).wrapping_sub(1);
        }
        if s.ui.panel.mode == 0x92 {
            seams::redraw_inventory(&mut s);
        }
        return;
    }
    let fits = {
        let s = state.borrow();
        let lock = s.objects.qq_of(sel) >> 8;
        (ktype == KEY && lock != 0 && s.objects.qq_of(key) >> 8 == lock)
            || (ktype == LOCK_PICK && lock == 0)
    };
    if fits {
        {
            let mut s = state.borrow_mut();
            if kind == CHEST {
                chest(&mut s, sel, true, false, false);
            } else {
                door(&mut s, sel, true, false, false);
            }
        }
        crate::main_loop::flush_prints(state, io).await;
    } else {
        let msg = message(&state.borrow(), P_NO_EFFECT);
        print_at(state, io, S_NL_LINE_NL, &[Arg::Str(&msg)]).await;
    }
}

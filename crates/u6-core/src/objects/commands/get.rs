//! Get (0x12a7a4) with its test whether the party may take an object (0x128aa4).

use std::cell::RefCell;

use super::seams;
use super::text::{print, print_at, putc, queue_at};
use super::world::{self, P_NO_EFFECT};
use crate::engine::Io;
use crate::objects::iter::tile_of;
use crate::objects::moves::{add_to_owner, move_into, remove};
use crate::objects::{IN_INVENTORY, READIED, Slot, display_name, mark_takeable, stack_kind};
use crate::state::State;
use crate::ui::printf::Arg;

/// Strings (hunk 15) and message pointers (A4-0x6c6c table).
const S_LINE: u32 = super::world::S_LINE;
const S_BANG_LINE: u32 = 0x0012_85ac; // "\n%s!\n"
const S_NL_LINE: u32 = 0x0012_85b2; // "\n%s"
const S_NL_LINE_NL: u32 = 0x0012_859a; // "\n%s\n"
const S_TOO_HEAVY: u32 = 0x0012_85b6;
const S_NO_FREE_HAND: u32 = 0x0012_85d0;
const S_STOP_THIEF: u32 = 0x0012_85de;
const S_STEALING: u32 = 0x0012_85f0;

/// Port-only (spec 08, slice K2): the strings this module passes to `printf` as its format,
/// for the locale layer's catalogue ([`crate::locale::strings`]).
pub(super) const PRINTED: &[u32] = &[
    S_LINE,
    S_BANG_LINE,
    S_NL_LINE,
    S_NL_LINE_NL,
    S_TOO_HEAVY,
    S_NO_FREE_HAND,
    S_STOP_THIEF,
    S_STEALING,
];
const P_NOTHING: i32 = -0x6c58;
const P_OUT_OF_RANGE: i32 = -0x6c60;
pub(super) const P_NOT_POSSIBLE: i32 = -0x6c54;
const _: i32 = P_NO_EFFECT;

/// Object types Get handles apart.
const TORCH: u16 = 0x5a;
const CHEST: u16 = 0x62;
const MOONSTONE: u16 = 0x49;
const DRAGON: u16 = 0x19b;
/// Original: A4-0x7b5a (8 × 3 words: where each moonstone lies; a taken one reads 0, 0, 0)
const MOONSTONES: usize = 0x2a;

/// The message the pointer at `A4 + off` names. Port (spec 08, slice K2): read for display only
/// (the commands print it through `%s`), so the locale pack's label of it is shown in its place
/// ([`crate::locale::shown`]).
pub(super) fn message(s: &State, off: i32) -> Vec<u8> {
    crate::locale::shown(s, s.objcmds.data.pointed(super::GameData::a4(off)))
}

/// Original: 0x128aa4 (objects: `obj` and everything inside it may be taken (status bit 0))
pub fn takeable(s: &State, obj: Slot) -> bool {
    let o = &s.objects;
    if o.status(obj) & 0x01 == 0 {
        return false;
    }
    let mut c = o.next_of(obj);
    while c >= 0 && o.place(c) != 0 && o.contained_in(c, obj) {
        if o.status(c) & 0x01 == 0 {
            return false;
        }
        c = o.next_of(c);
    }
    true
}

fn leader(s: &State) -> Slot {
    let p = &s.actors.party;
    Slot::from(p.members[usize::from(p.leader_index).min(15)])
}

fn shows_leader_inventory(s: &State) -> bool {
    i16::from(s.actors.party.leader_index) == s.ui.panel.member && s.ui.panel.mode == 0x92
}

fn panel_actor(s: &State) -> Slot {
    let m = usize::try_from(s.ui.panel.member).unwrap_or(0).min(15);
    Slot::from(s.actors.party.members[m])
}

/// Original: 0x12a7a4 (objects: the Get command on the confirmed target)
///
/// The leader takes the selected object; past a cover (flags3 bit 4), or for an actor's tile,
/// the object below (0x12948a). Nothing there or not on the map: the nothing message. The
/// leader turns to it and its name is printed. More than one tile away: the out-of-range
/// message. A damaging tile: the not-possible message and the hazard (0x102f3c). Weightless
/// (unless a type weighed in tenths), 10000 or more, an actor or a dragon part: not possible.
/// The weight with what the leader holds (0x11121e) above 20 × strength: the too-heavy line
/// (0x1285b6). Else 3 movement points, and the object goes into the inventory: a counted stack
/// merges (0x10cf7c), a lit torch is readied in a free hand (the no-free-hand line 0x1285d0
/// when both hold something), anything else is carried (a few types get their resting frame
/// back). Taking what is not yours on the surface costs karma, and the first awake neutral
/// townsperson in view cries thief (0x1285de) and turns (combat 0x123aec); unseen: the
/// stealing line (0x1285f0).
pub async fn get(state: &RefCell<State>, io: &Io) {
    let actor = leader(&state.borrow());
    {
        let mut s = state.borrow_mut();
        let sel = s.ui.input.selection;
        let cover = |s: &State, o: Slot| {
            let tile = tile_of(&s.tables, s.objects.tf(o));
            s.tables
                .tileflags
                .flags3
                .get(usize::from(tile))
                .is_some_and(|f| f & 0x10 != 0)
        };
        if (sel <= 0xff && s.ui.input.target.0 != -1) || cover(&s, sel) {
            let under = world::object_under(&mut s, sel);
            s.ui.input.selection = under;
        }
    }
    let sel = state.borrow().ui.input.selection;
    if sel == -1 || state.borrow().objects.place(sel) != 0 {
        let msg = message(&state.borrow(), P_NOTHING);
        print_at(state, io, S_LINE, &[Arg::Str(&msg)]).await;
        return;
    }
    {
        let mut s = state.borrow_mut();
        let (px, py) = s.ui.input.pointer;
        let dir = seams::direction_of(&s, px, py);
        if dir != -1 {
            crate::actors::walk::face(&mut s, actor as u16, dir as u16);
            crate::map::view::update_view(&mut s);
        }
        world::name_prefix(&mut s, sel);
    }
    let name = display_name(&state.borrow(), sel).into_bytes();
    print(state, io, &name, &[]).await;
    putc(state, io, b'\n').await;
    let (ax, ay, (tx, ty)) = {
        let s = state.borrow();
        (s.objects.x(actor), s.objects.y(actor), s.ui.input.target)
    };
    let far = |t: i16, a: u16| t.wrapping_sub(a as i16).wrapping_abs() > 1;
    if far(tx, ax) || far(ty, ay) {
        let msg = message(&state.borrow(), P_OUT_OF_RANGE);
        print_at(state, io, S_BANG_LINE, &[Arg::Str(&msg)]).await;
        return;
    }
    let tile = tile_of(&state.borrow().tables, state.borrow().objects.tf(sel));
    let hazard = state
        .borrow()
        .tables
        .tileflags
        .flags1
        .get(usize::from(tile))
        .is_some_and(|f| f & 0x08 != 0);
    if hazard {
        let msg = message(&state.borrow(), P_NOT_POSSIBLE);
        print_at(state, io, S_NL_LINE, &[Arg::Str(&msg)]).await;
        crate::actors::party::tile_effect(&mut state.borrow_mut(), actor as u16, tile);
        putc(state, io, b'\n').await;
        seams::wound_text(&mut state.borrow_mut(), actor);
        crate::main_loop::flush_prints(state, io).await;
        return;
    }
    if take(state, io, actor, sel).await {
        finish(state, io).await;
    }
}

/// From the weight test on; false when Get ended at the no-free-hand line (no end-of-command
/// bookkeeping then).
async fn take(state: &RefCell<State>, io: &Io, actor: Slot, sel: Slot) -> bool {
    let (wt, strength, kind) = {
        let mut s = state.borrow_mut();
        let wt = seams::weight(&s, sel).wrapping_add(seams::contents_weight(&s, sel));
        let strength = i16::from(s.actors.attrs.strength[actor as usize]);
        if !shows_leader_inventory(&s) {
            seams::sum_inventory(&mut s, actor);
        }
        (wt, strength, s.objects.kind(sel))
    };
    let weightless = wt == 0 && !seams::weighed_in_tenths(&state.borrow(), kind);
    if weightless || wt >= 10000 || sel < 0x100 || kind == DRAGON {
        let msg = message(&state.borrow(), P_NOT_POSSIBLE);
        print_at(state, io, S_NL_LINE_NL, &[Arg::Str(&msg)]).await;
        return true;
    }
    let (readied, carried) = {
        let inv = &state.borrow().inventory;
        (inv.equip_weight, inv.carried_weight)
    };
    if wt.wrapping_add(carried).wrapping_add(readied) > strength.wrapping_mul(20) {
        print_at(state, io, S_TOO_HEAVY, &[]).await;
        return true;
    }
    let stored = store(&mut state.borrow_mut(), actor, sel, kind);
    if !stored {
        print_at(state, io, S_NO_FREE_HAND, &[]).await;
    }
    stored
}

/// Get from the movement points on (synchronous; the theft lines are queued): false when
/// both hands of the shown member are full for a lit torch.
fn store(s: &mut State, actor: Slot, sel: Slot, kind: u16) -> bool {
    crate::actors::walk::spend_mp(s, actor as u16, 3);
    let frame = s.objects.tf(sel) >> 10;
    if stack_kind(&s.objects, kind) != 0 && !(kind == TORCH && frame != 0) {
        let qq = s.objects.qq_of(sel);
        if add_to_owner(s, actor, kind, qq) >= 0 {
            remove(s, sel);
        }
    } else if kind == TORCH && frame == 1 {
        if i16::from(s.actors.party.leader_index) == s.ui.panel.member {
            if s.inventory.equip(2) == 0 {
                s.inventory.set_equip(2, sel);
            } else if s.inventory.equip(5) == 0 {
                s.inventory.set_equip(5, sel);
            } else {
                return false;
            }
        }
        move_into(s, sel, actor, READIED);
    } else {
        move_into(s, sel, actor, IN_INVENTORY);
        let t = &mut s.objects.type_frame[sel as usize];
        match kind {
            0x7a | 0x91 => *t &= 0x3ff,
            CHEST if frame == 0 => *t = (*t & 0x3ff) + 0x400,
            MOONSTONE => {
                let at = MOONSTONES + usize::from(frame) * 6;
                if let Some(b) = s.actors.objlist_tail.get_mut(at..at + 6) {
                    b.fill(0);
                }
            }
            0x1a7 => *t = (*t & 0xfc00) + 0x1a4,
            _ => {}
        }
    }
    if shows_leader_inventory(s) {
        let a = panel_actor(s);
        seams::inventory_contents(s, a);
    }
    if !takeable(s, sel) && s.map.centre.z == 0 {
        mark_takeable(&mut s.objects, sel);
        seams::lose_karma(s, 1);
        let witness = (1..0xe0).any(|a| {
            let st = s.actors.attrs.status[a as usize];
            st & 0x60 == 0 && crate::converse::on_screen(s, a) && st & 0x04 == 0
        });
        if witness {
            queue_at(s, S_STOP_THIEF, Vec::new());
            seams::turn_hostile(s);
        } else {
            queue_at(s, S_STEALING, Vec::new());
        }
    }
    crate::map::view::update_view(s);
    true
}

/// The end of Get past the weight test: the shown member's sums again, no target pending.
async fn finish(state: &RefCell<State>, io: &Io) {
    {
        let mut s = state.borrow_mut();
        if i16::from(s.actors.party.leader_index) != s.ui.panel.member {
            let a = panel_actor(&s);
            seams::sum_inventory(&mut s, a);
        }
        s.ui.input.target_pending = 0;
    }
    crate::main_loop::flush_prints(state, io).await;
}

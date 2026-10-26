//! Move (0x12ae04): push an object or actor one tile on the map (0x12acc0, 0x115c06), or move
//! a carried object to another party member or into or out of a container (0x12adc4).

use std::cell::RefCell;

use super::drop::{fits_in, how_many, surface_at};
use super::get::{P_NOT_POSSIBLE, message};
use super::seams;
use super::text::{print, print_at, putc};
use super::world::{S_DOT, S_LINE};
use crate::engine::Io;
use crate::objects::iter::begin_at;
use crate::objects::moves::{add_to_owner, move_into, move_to, remove, take_from_owner};
use crate::objects::{
    IN_CONTAINER, IN_INVENTORY, READIED, Slot, in_type_set_670c, shown_name, stack_kind,
};
use crate::state::State;
use crate::ui::pointer::LEADER_PIXEL;
use crate::ui::printf::Arg;

const S_BANG_LINE: u32 = 0x0012_85ac; // "\n%s!\n"
const S_NL_LINE_NL: u32 = 0x0012_859a; // "\n%s\n"
const S_TO: u32 = 0x0012_85fe;
const S_NOWHERE: u32 = 0x0012_8602;
const S_DIR_LINE: u32 = 0x0012_860c; // "%s.\n"
const S_NOT_AROUND: u32 = 0x0012_8612;
const S_NL2_LINE_NL: u32 = 0x0012_8630; // "\n\n%s\n"
const S_NOBODY: u32 = 0x0012_8636;
const S_NL2_BANG_LINE: u32 = 0x0012_8640; // "\n\n%s!\n"
const S_ONLY_PARTY: u32 = 0x0012_8648;
const S_CANT_CARRY: u32 = 0x0012_8662;
const S_YOURSELF: u32 = 0x0012_8672;
const S_NOT_CONTAINER: u32 = 0x0012_867e;
const S_OTHERS: u32 = 0x0012_8692;
const S_TOO_BIG: u32 = 0x0012_86ba;
const S_UNREADY_FIRST: u32 = 0x0012_86e6;
const S_INTO_ITSELF: u32 = 0x0012_8708;
const S_ONLY_SPELLS: u32 = 0x0012_8730;
const S_ONLY_MOONSTONES: u32 = 0x0012_875a;
const S_SPELLS_STAY: u32 = 0x0012_878a;
const S_OUT_OF: u32 = 0x0012_87c8;
const S_INTO: u32 = 0x0012_87d0;

/// Port-only (spec 08, slice K2): the strings this module passes to `printf` as its format,
/// for the locale layer's catalogue ([`crate::locale::strings`]).
pub(super) const PRINTED: &[u32] = &[
    S_BANG_LINE,
    S_NL_LINE_NL,
    S_TO,
    S_NOWHERE,
    S_DIR_LINE,
    S_NOT_AROUND,
    S_NL2_LINE_NL,
    S_NOBODY,
    S_NL2_BANG_LINE,
    S_ONLY_PARTY,
    S_CANT_CARRY,
    S_YOURSELF,
    S_NOT_CONTAINER,
    S_OTHERS,
    S_TOO_BIG,
    S_UNREADY_FIRST,
    S_INTO_ITSELF,
    S_ONLY_SPELLS,
    S_ONLY_MOONSTONES,
    S_SPELLS_STAY,
    S_OUT_OF,
    S_INTO,
];
const P_NOTHING: i32 = -0x6c58;
const P_OUT_OF_RANGE: i32 = -0x6c60;
const P_BLOCKED: i32 = -0x6c5c;
const P_FAILED: i32 = -0x6c68;
/// Original: A4-0x6c3c (the line that type 0x4c stays where it is)
const P_FIXED: i32 = -0x6c3c;
/// Original: A4-0x61be (pointers to the 8 direction names)
const DIR_NAMES: i32 = -0x61be;
/// The command code a direction key gives (walk).
const WALK: u16 = 0x80;
/// The code a confirmed target gives (0x10790a).
const TARGETED: u16 = crate::ui::wait::REPEAT;
/// A chair: pushed against something, or not facing the push, it only turns.
const CHAIR: u16 = 0xdd;
/// Carried types Move handles apart: one that never moves, the spellbook and the spells it
/// takes, the vortex cube and the moonstones it takes.
const FIXED: u16 = 0x4c;
const SPELLBOOK: u16 = 0x39;
const SPELL: u16 = 0x3a;
const VORTEX_CUBE: u16 = 0x3e;
const MOONSTONE: u16 = 0x49;

/// The frame pixel of map location (x, y) with the view centred on (cx, cy): the leader's
/// pixel (A4-0x7a26 / -0x7a25 at rest, [`LEADER_PIXEL`]) plus 16 per cell. Words: Wide's map
/// pixels pass 255, where the original's bytes would wrap (spec 13 Design 3).
fn view_pixel(x: u16, y: u16, cx: u16, cy: u16) -> (u16, u16) {
    (
        (x.wrapping_sub(cx) << 4).wrapping_add(LEADER_PIXEL.0),
        (y.wrapping_sub(cy) << 4).wrapping_add(LEADER_PIXEL.1),
    )
}

fn leader(s: &State) -> Slot {
    let p = &s.actors.party;
    Slot::from(p.members[usize::from(p.leader_index).min(15)])
}

/// Original: 0x12acc0 (objects: is the push of `obj` from (x, y) toward `dir` blocked)
///
/// Not blocked when the object may stand on the tile ahead (0x11b1c0) or a surface is there
/// (0x12a050); a diagonal push then also needs one of the two tiles beside the diagonal free.
pub fn push_blocked(s: &mut State, obj: Slot, x: u16, y: u16, dir: usize) -> bool {
    let d = &s.objects.defs;
    let at = |i: usize, d: &crate::objects::ObjectDefs| {
        (
            x.wrapping_add_signed(d.dx(i)),
            y.wrapping_add_signed(d.dy(i)),
        )
    };
    let (nx, ny) = at(dir, d);
    if !seams::may_stand(s, obj, nx, ny) && !surface_at(s, nx, ny) {
        return true;
    }
    if dir & 1 == 0 {
        return false;
    }
    let d = &s.objects.defs;
    let (a, b) = (at(dir.wrapping_sub(1), d), at(dir + 1, d));
    let free =
        |s: &mut State, (x, y): (u16, u16)| seams::may_stand(s, obj, x, y) || surface_at(s, x, y);
    !(free(s, a) || free(s, b))
}

/// The end of a Move that went nowhere or failed: no target pending, the pointer origin back on
/// the centre cell, the pointer shape for it (0x1051a6, [`crate::ui::pointer::update_shape`]).
fn reset_pointer(s: &mut State) {
    s.ui.input.target_pending = 0;
    s.inventory.pointer_origin = LEADER_PIXEL;
    crate::ui::pointer::update_shape(s);
}

/// Original: 0x12ae04 (objects: the Move command on the confirmed target)
///
/// Nothing selected or the shown container: the nothing message (A4-0x6c58). The name. On the
/// map: within one tile of the leader (else the out-of-range message, A4-0x6c60), the leader
/// turns to it, the direction prompt (0x1285fe) and a direction key (any other key: the
/// nowhere line, 0x128602), the direction's name. A weightless thing or a dragon part: the
/// not-possible message. An actor resists (magic 0x115c06), and the leader or an actor lying
/// down (frame & 3 = 3, not type 0x153) cannot be pushed: the failed message (A4-0x6c68). Else
/// 5 movement points, and it goes into a container ahead it fits in (weight below 0xff), or,
/// unless blocked (the blocked message, A4-0x6c5c; 0x12acc0),
/// one tile on; a chair turns to face the push instead when it does not already. A carried
/// object goes to the member or container the next target names ([`move_carried`]).
pub async fn push(state: &RefCell<State>, io: &Io) {
    let sel = state.borrow().ui.input.selection;
    if sel == -1 || sel as u16 == state.borrow().ui.panel.container {
        let msg = message(&state.borrow(), P_NOTHING);
        print_at(state, io, S_LINE, &[Arg::Str(&msg)]).await;
        return;
    }
    let name = shown_name(&state.borrow(), sel).into_bytes();
    print(state, io, &name, &[]).await;
    putc(state, io, b'\n').await;
    if state.borrow().objects.place(sel) != 0 {
        move_carried(state, io, sel).await;
        return;
    }
    let (cx, cy, (tx, ty)) = {
        let s = state.borrow();
        (s.map.centre.x, s.map.centre.y, s.ui.input.target)
    };
    let far = |t: i16, c: u16| t.wrapping_sub(c as i16).wrapping_abs() > 1;
    if far(tx, cx) || far(ty, cy) {
        let msg = message(&state.borrow(), P_OUT_OF_RANGE);
        print_at(state, io, S_BANG_LINE, &[Arg::Str(&msg)]).await;
        return;
    }
    let (ox, oy) = {
        let mut s = state.borrow_mut();
        let (px, py) = s.ui.input.pointer;
        let dir = seams::direction_of(&s, px, py);
        if dir != -1 {
            let l = leader(&s) as u16;
            crate::actors::walk::face(&mut s, l, dir as u16);
            crate::map::view::update_view(&mut s);
        }
        let (ox, oy) = (s.objects.x(sel), s.objects.y(sel));
        s.ui.input.target_pending = 0;
        s.inventory.pointer_origin = view_pixel(ox, oy, cx, cy);
        (ox, oy)
    };
    print_at(state, io, S_TO, &[]).await;
    if crate::ui::wait::read_key(state, io).await != WALK {
        print_at(state, io, S_NOWHERE, &[]).await;
        reset_pointer(&mut state.borrow_mut());
        return;
    }
    let dir = state.borrow().ui.input.direction;
    let dir_name = {
        let s = state.borrow();
        let at = super::GameData::a4(DIR_NAMES).wrapping_add_signed(i32::from(dir) * 4);
        // Port (spec 08, slice K2; 2026-10-24): read for display, so the locale pack's label of
        // it when it has one ([`crate::magic::LABELS`], [`crate::locale::shown`]).
        crate::locale::shown(&s, s.objcmds.data.pointed(at))
    };
    print_at(state, io, S_DIR_LINE, &[Arg::Str(&dir_name)]).await;
    let pusher = leader(&state.borrow());
    let (wt, kind) = {
        let s = state.borrow();
        let wt = seams::weight(&s, sel).wrapping_add(seams::contents_weight(&s, sel));
        (wt, s.objects.kind(sel))
    };
    let weightless = state
        .borrow()
        .tables
        .tileflags
        .weight
        .get(usize::from(kind))
        .copied()
        .unwrap_or(0)
        == 0;
    if weightless || kind == 0x19b {
        let msg = message(&state.borrow(), P_NOT_POSSIBLE);
        print_at(state, io, S_NL_LINE_NL, &[Arg::Str(&msg)]).await;
        reset_pointer(&mut state.borrow_mut());
        finish(state);
        return;
    }
    if sel < 0x100 {
        let failed = {
            let mut s = state.borrow_mut();
            crate::magic::strength_contest(&mut s, pusher as u16, sel as u16)
                || sel == pusher
                || (s.objects.tf(sel) >> 10 & 3 == 3 && kind != 0x153)
        };
        if failed {
            let msg = message(&state.borrow(), P_FAILED);
            print_at(state, io, S_NL_LINE_NL, &[Arg::Str(&msg)]).await;
            reset_pointer(&mut state.borrow_mut());
            return;
        }
    }
    let blocked = {
        let mut s = state.borrow_mut();
        let s = &mut *s;
        crate::actors::walk::spend_mp(s, pusher as u16, 5);
        let d = usize::try_from(dir).unwrap_or(0);
        let nx = ox.wrapping_add_signed(s.objects.defs.dx(d));
        let ny = oy.wrapping_add_signed(s.objects.defs.dy(d));
        let z = u16::from(s.map.centre.z);
        let here = begin_at(&mut s.objects, &s.tables, nx, ny, z);
        if here != -1 && fits_in(s, here, sel) && wt < 0xff {
            move_into(s, sel, here, IN_CONTAINER);
            false
        } else if push_blocked(s, sel, ox, oy, d) {
            true
        } else {
            if !turn_chair(s, sel, dir) {
                move_to(s, sel, nx, ny, z);
            }
            false
        }
    };
    if blocked {
        let msg = message(&state.borrow(), P_BLOCKED);
        print_at(state, io, S_NL_LINE_NL, &[Arg::Str(&msg)]).await;
        let mut s = state.borrow_mut();
        turn_chair(&mut s, sel, dir);
        reset_pointer(&mut s);
        return;
    }
    {
        let mut s = state.borrow_mut();
        crate::map::view::update_view(&mut s);
        reset_pointer(&mut s);
    }
    finish(state);
}

/// A chair not facing the push direction turns to it (frame (dir >> 1) & 3); true when it did.
fn turn_chair(s: &mut State, obj: Slot, dir: i16) -> bool {
    if s.objects.kind(obj) != CHAIR || s.objects.tf(obj) >> 10 == (dir >> 1) as u16 {
        return false;
    }
    let t = &mut s.objects.type_frame[obj as usize];
    *t = (*t & 0x3ff) + ((((dir >> 1) & 3) as u16) << 10);
    true
}

/// 0x12ba24: the panel is not redrawn on its own, no target pending.
fn finish(state: &RefCell<State>) {
    let mut s = state.borrow_mut();
    s.ui.panel.dirty = false;
    s.ui.input.target_pending = 0;
}

/// The actor of party index `m` (the panel's member, A4-0x7a3e).
fn member(s: &State, m: i16) -> Slot {
    Slot::from(s.actors.party.members[usize::try_from(m).unwrap_or(0).min(15)])
}

/// The carried half of 0x12ae04 (0x12b24c..0x12ba34) on `obj`.
///
/// A readied object that resists (0x1122dc) only redraws the panel's container (0x111be0). The
/// member the panel shows must be in view (0x112bf2, else the not-around line 0x128612). How
/// many (0x12a106, 0 ends it), the to prompt (0x1285fe); type 0x4c stays (its line, A4-0x6c3c).
/// A cursor up gives way to the cross-hair (range 7, 0x12b2dc); the key must confirm a target
/// (0x8e, else the nobody line 0x128636). A target on the map must be in view (an actor) or
/// within one tile of the centre (an object), else the out-of-range line (A4-0x6c60). Then a
/// member ([`to_member`]) or a container ([`to_container`]); unless they refused, the panel's
/// container is redrawn when the inventory shows (0x12ba14) and [`finish`] (in Wide the panel is
/// then marked for the views' redraw). A refusal returns with the target still pending
/// (0x12ba2c), as the original does.
async fn move_carried(state: &RefCell<State>, io: &Io, obj: Slot) {
    if seams::cannot_drop(state, io, obj).await != 0 {
        let shown = state.borrow().ui.panel.container as Slot;
        crate::inventory::panel_contents(state, io, shown).await;
        return;
    }
    let (kind, sk, m) = {
        let s = state.borrow();
        let kind = s.objects.kind(obj);
        (kind, stack_kind(&s.objects, kind), s.ui.panel.member)
    };
    let giver = member(&state.borrow(), m);
    if !crate::converse::on_screen(&state.borrow(), giver) {
        print_at(state, io, S_NOT_AROUND, &[]).await;
        return;
    }
    let n = how_many(state, io, obj).await;
    if n == 0 {
        return;
    }
    print_at(state, io, S_TO, &[]).await;
    if kind == FIXED {
        let msg = message(&state.borrow(), P_FIXED);
        print_at(state, io, S_NL2_LINE_NL, &[Arg::Str(&msg)]).await;
        return;
    }
    {
        let mut s = state.borrow_mut();
        if s.ui.input.targeting != 0 {
            s.ui.input.range = 7;
            crate::ui::cursor::panel_cursor_in(&mut s, io, 0);
            s.ui.input.targeting = 1;
        }
    }
    let key = crate::ui::wait::read_key(state, io).await;
    let t = state.borrow().ui.input.selection;
    if key & 0xff != TARGETED || t == -1 {
        print_at(state, io, S_NOBODY, &[]).await;
        return;
    }
    let far = {
        let s = state.borrow();
        let (tx, ty) = s.ui.input.target;
        let c = s.map.centre;
        let off = |t: i16, c: u16| t.wrapping_sub(c as i16).wrapping_abs() > 1;
        s.objects.place(t) == 0
            && if t < 0x100 {
                !crate::converse::on_screen(&s, t)
            } else {
                off(tx, c.x) || off(ty, c.y)
            }
    };
    if far {
        let msg = message(&state.borrow(), P_OUT_OF_RANGE);
        print_at(state, io, S_NL2_BANG_LINE, &[Arg::Str(&msg)]).await;
        return;
    }
    let moved = if t < 0x100 {
        to_member(state, io, obj, (kind, sk, n), m, t).await
    } else {
        to_container(state, io, obj, (kind, sk, n), t).await
    };
    if !moved {
        return;
    }
    let (inventory, shown) = {
        let s = state.borrow();
        (s.ui.panel.mode == 0x92, s.ui.panel.container as Slot)
    };
    if inventory {
        crate::inventory::panel_contents(state, io, shown).await;
    }
    finish(state);
    // Port-only (spec 14 Design 2, Wide): the inventory view stays on the screen in each of the
    // three views' modes, and the draw above leaves its slot as it was; the command loop's
    // 0x103f64 redraws the three views with the object moved.
    #[cfg(feature = "wide")]
    {
        let mut s = state.borrow_mut();
        if crate::ui::panels::slot_of(s.ui.panel.mode).is_some() {
            s.ui.panel.dirty = true;
        }
    }
}

/// 0x12b396..0x12b610: carried `obj` (type, stack kind 0x10cef2, count) from the giver, the
/// member of party index `m` (the panel's as the key read began, kept at 0x12b2be), to actor
/// `t`; false when refused.
///
/// The giver himself: the yourself line (0x128672), nothing moves. Another is named and must
/// be in view (the not-around line) and in the party (status bit 7, else 0x128648), and must
/// carry it ([`can_carry`]), else the can't-carry line (0x128662) and the sums redone of the
/// member the panel shows now (0x12b492 reads A4-0x7a3e again: the target key may have paged
/// the panel). Then its magic ends (0x11236c on the giver, the flash played), a readied one
/// leaves its equipment slot (both hands too, 0x110f96 = 8: the giver's flag), a stack's count
/// goes from its holder to him (0x10cf7c, 0x10d16e), anything else into his inventory
/// (0x10cda8), and the sums of the member the panel shows are redone (0x12b5a6). Either way the
/// leader turns to the pointer (0x105016, 0x11ba5e) and spends 5 movement points.
async fn to_member(
    state: &RefCell<State>,
    io: &Io,
    obj: Slot,
    (kind, sk, n): (u16, u16, i16),
    m: i16,
    t: Slot,
) -> bool {
    let giver = member(&state.borrow(), m);
    if t == giver {
        print_at(state, io, S_YOURSELF, &[]).await;
    } else {
        let name = shown_name(&state.borrow(), t).into_bytes();
        print(state, io, &name, &[]).await;
        print_at(state, io, S_DOT, &[]).await;
        let refusal = {
            let s = state.borrow();
            let party = s.actors.attrs.status.get(t as usize).copied().unwrap_or(0) & 0x80;
            if !crate::converse::on_screen(&s, t) {
                Some(S_NOT_AROUND)
            } else if party == 0 {
                Some(S_ONLY_PARTY)
            } else {
                None
            }
        };
        if let Some(line) = refusal {
            print_at(state, io, line, &[]).await;
            return false;
        }
        let fits = can_carry(&mut state.borrow_mut(), obj, (kind, sk, n), t);
        if !fits {
            print_at(state, io, S_CANT_CARRY, &[]).await;
            let mut s = state.borrow_mut();
            let shown = member(&s, s.ui.panel.member);
            seams::sum_inventory(&mut s, shown);
            return false;
        }
        seams::end_worn_magic(&mut state.borrow_mut(), io, obj, giver);
        crate::combat::play_effects(state, io).await;
        let mut s = state.borrow_mut();
        let s = &mut *s;
        if s.objects.place(obj) == READIED {
            let slot = seams::equipment_slot(s, obj);
            s.inventory.set_equip(slot, 0);
            if super::super::seams::ready_slot(s, obj) == 8 {
                if let Some(f) = usize::try_from(m)
                    .ok()
                    .and_then(|m| s.ui.panel.member_flags.get_mut(m))
                {
                    *f = 0;
                }
                s.inventory.set_equip(5, 0);
            }
        }
        if sk != 0 {
            let q = if sk == 2 {
                (s.objects.qq_of(obj) & 0xff00) | n as u16
            } else {
                n as u16
            };
            add_to_owner(s, t, kind, q);
            let holder = s.objects.parent(obj);
            take_from_owner(s, holder, kind, q);
        } else {
            move_into(s, obj, t, IN_INVENTORY);
        }
        let shown = member(s, s.ui.panel.member);
        seams::sum_inventory(s, shown);
    }
    let mut s = state.borrow_mut();
    let (px, py) = s.ui.input.pointer;
    let dir = seams::direction_of(&s, px, py);
    let l = leader(&s) as u16;
    if dir != -1 {
        crate::actors::walk::face(&mut s, l, dir as u16);
        crate::map::view::update_view(&mut s);
    }
    crate::actors::walk::spend_mp(&mut s, l, 5);
    true
}

/// 0x12b3f4..0x12b488: actor `t` can take carried `obj` (type, stack kind, count): his readied
/// and carried weight (0x11121e, left in A4+0x1342 / +0x1344) plus the object's (a stack: its
/// type's weight byte × count, in tenths for the types of 0x1110c8; else 0x1110f6 + 0x1111ca)
/// at most his strength × 20 (signed words, `ble` at 0x12b488).
fn can_carry(s: &mut State, obj: Slot, (kind, sk, n): (u16, u16, i16), t: Slot) -> bool {
    seams::sum_inventory(s, t);
    let w = if sk != 0 {
        let byte = s.tables.tileflags.weight.get(usize::from(kind));
        let w = i16::from(byte.copied().unwrap_or(0)).wrapping_mul(n);
        if seams::weighed_in_tenths(s, kind) {
            w / 10
        } else {
            w
        }
    } else {
        seams::weight(s, obj).wrapping_add(seams::contents_weight(s, obj))
    };
    let strength = s.actors.attrs.strength.get(t as usize).copied();
    let inv = &s.inventory;
    w.wrapping_add(inv.carried_weight)
        .wrapping_add(inv.equip_weight)
        <= i16::from(strength.unwrap_or(0)) * 20
}

/// The line 0x12b614..0x12b7b0 refuses moving `obj` to object `t` with, if any: `t` not
/// carried (the nobody line), not a container type (0x12899c), held by another actor
/// (0x12adc4), `obj` an actor, readied or `t` itself; into the spellbook only a spell, into
/// the vortex cube only a moonstone; out of the spellbook no spell.
fn container_refusal(s: &State, obj: Slot, t: Slot) -> Option<u32> {
    let o = &s.objects;
    let (into, what) = (o.kind(t), o.kind(obj));
    Some(if o.place(t) == 0 {
        S_NOBODY
    } else if !in_type_set_670c(o, into) {
        S_NOT_CONTAINER
    } else if o.outer_holder(t) != o.outer_holder(obj) {
        // Port-only: two parent chains that never reach the map both give -1 here and pass;
        // the original never returns from 0x12adc4 on such a chain.
        S_OTHERS
    } else if obj < 0x100 {
        S_TOO_BIG
    } else if o.place(obj) == READIED {
        S_UNREADY_FIRST
    } else if t == obj {
        S_INTO_ITSELF
    } else if into == SPELLBOOK && what != SPELL {
        S_ONLY_SPELLS
    } else if into == VORTEX_CUBE && what != MOONSTONE {
        S_ONLY_MOONSTONES
    } else if o.parent(obj) == t && into == SPELLBOOK && what == SPELL {
        S_SPELLS_STAY
    } else {
        return None;
    })
}

/// 0x12b614..0x12ba12: carried `obj` (type, stack kind 0x10cef2, count) into or out of
/// carried container `t`; false when refused ([`container_refusal`]).
///
/// The container holding it takes it out to its own holder (the out-of line 0x1287c8), any
/// other takes it in (the into line 0x1287d0); the leader spends 5 movement points. A stack's
/// count is taken from the stack itself ([`take_count`]) and given (0x10cf7c), anything else
/// moves (0x10cda8, place 0x08 even when the holder is an actor, 0x12b8de). The list count
/// (A4+0x1340) drops by one and the container is named.
async fn to_container(
    state: &RefCell<State>,
    io: &Io,
    obj: Slot,
    (kind, sk, n): (u16, u16, i16),
    t: Slot,
) -> bool {
    let refusal = container_refusal(&state.borrow(), obj, t);
    if let Some(line) = refusal {
        print_at(state, io, line, &[]).await;
        return false;
    }
    let out = state.borrow().objects.parent(obj) == t;
    print_at(state, io, if out { S_OUT_OF } else { S_INTO }, &[]).await;
    {
        let mut s = state.borrow_mut();
        let s = &mut *s;
        let l = leader(s) as u16;
        crate::actors::walk::spend_mp(s, l, 5);
        let to = if out { s.objects.parent(t) } else { t };
        if sk != 0 {
            let q = take_count(s, obj, sk, n);
            add_to_owner(s, to, kind, q);
        } else {
            move_into(s, obj, to, IN_CONTAINER);
        }
        let p = &mut s.ui.panel;
        p.inventory_count = p.inventory_count.wrapping_sub(1);
    }
    let name = shown_name(&state.borrow(), t).into_bytes();
    print(state, io, &name, &[]).await;
    putc(state, io, b'\n').await;
    true
}

/// 0x12b7dc..0x12b880: `n` of stack `obj` taken from it (removed when that is all of it), as
/// the count word 0x10cf7c is given: the stack's quality << 8 | `n` for a quantity stack
/// (stack kind 2, the quantity byte), else `n` (the whole word).
fn take_count(s: &mut State, obj: Slot, sk: u16, n: i16) -> u16 {
    let qq = s.objects.qq_of(obj);
    let (left, all) = if sk == 2 {
        let q = qq & 0xff;
        (
            (qq & 0xff00) | (q.wrapping_sub(n as u16) & 0xff),
            q == n as u16,
        )
    } else {
        (qq.wrapping_sub(n as u16), qq == n as u16)
    };
    if all {
        remove(s, obj);
    } else if let Some(w) = s.objects.qq.get_mut(obj as usize) {
        *w = left;
    }
    if sk == 2 {
        (s.objects.qq_of(obj) & 0xff00) | n as u16
    } else {
        n as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{HALF_H, HALF_W, MAP_X, MAP_Y, VIEW_H, VIEW_W};

    /// Spec 13 Design 3: the pointer origin Move puts on the pushed object is the centre of its
    /// view cell. For the view's east-most and south-most cells that is past 255 in Wide, and no
    /// byte wraps it: from the east-most cell the leader is due west, from the south-most due
    /// north.
    #[test]
    fn the_push_pixel_of_the_east_most_view_cell_does_not_wrap() {
        let (cx, cy) = (500u16, 400u16);
        let (hw, hh) = (HALF_W as u16, HALF_H as u16);
        let east = view_pixel(cx + hw, cy, cx, cy);
        assert_eq!(east, (MAP_X + (VIEW_W as u16 - 1) * 16 + 8, LEADER_PIXEL.1));
        let south = view_pixel(cx, cy + hh, cx, cy);
        assert_eq!(
            south,
            (LEADER_PIXEL.0, MAP_Y + (VIEW_H as u16 - 1) * 16 + 8)
        );
        let north_west = view_pixel(cx - hw, cy - hh, cx, cy);
        assert_eq!(north_west, (MAP_X + 8, MAP_Y + 8), "the view's first cell");
        assert_eq!(view_pixel(cx, cy, cx, cy), LEADER_PIXEL, "the leader's own");
        let (lx, ly) = (LEADER_PIXEL.0 as i16, LEADER_PIXEL.1 as i16);
        assert_eq!(crate::inventory::direction_to(east, lx, ly), 6, "west");
        assert_eq!(crate::inventory::direction_to(south, lx, ly), 0, "north");
        if cfg!(feature = "wide") {
            assert!(
                east.0 > 0xff && east.0 < u6_render::WIDTH as u16,
                "past a byte"
            );
        }
    }

    /// The container rules of Move on a carried object (0x12b614..0x12b7b0) on synthetic
    /// slots: the holder each side climbs to (0x12adc4: at least one step, to the first map
    /// object) must be the same, the vortex cube 0x3e takes only a moonstone 0x49 (the target's
    /// rule), the spellbook 0x39 only a spell 0x3a and gives none back, and a readied object or
    /// the container itself is refused. None when it may go in or out. (The too-big line for an
    /// actor comes after the holder test, which a map actor's parent words never pass.)
    #[test]
    fn a_carried_object_goes_only_into_a_bag_its_own_holder_carries() {
        let mut s = crate::objects::tests::world();
        s.objects.defs.type_set_670c = vec![0x40, 0x3e, SPELLBOOK];
        let (a, b) = (1, 2);
        for (actor, x) in [(a, 500), (b, 501)] {
            s.objects.type_frame[actor as usize] = 0x170;
            crate::objects::move_to(&mut s, actor, x, 500, 0);
        }
        let make = |s: &mut State, t: u16, place: u8, owner: Slot| {
            crate::objects::create_in(s, t, place, 1, owner)
        };
        let gem = make(&mut s, 0x50, IN_INVENTORY, a);
        let bag = make(&mut s, 0x40, IN_INVENTORY, a);
        let inner = make(&mut s, 0x40, IN_CONTAINER, bag);
        let theirs = make(&mut s, 0x40, IN_INVENTORY, b);
        let cube = make(&mut s, 0x3e, IN_INVENTORY, a);
        let stone = make(&mut s, MOONSTONE, IN_INVENTORY, a);
        let book = make(&mut s, SPELLBOOK, IN_INVENTORY, a);
        let spell = make(&mut s, SPELL, IN_CONTAINER, book);
        let worn = make(&mut s, 0x50, READIED, a);
        assert!(
            [gem, bag, inner, theirs, cube, stone, book, spell, worn]
                .iter()
                .all(|&o| o >= 0x100)
        );
        let o = &s.objects;
        assert_eq!(o.outer_holder(inner), a, "two steps up");
        assert_eq!(o.outer_holder(theirs), b);
        let rule = |obj, t| container_refusal(&s, obj, t);
        assert_eq!(rule(gem, bag), None, "into a bag the giver carries");
        assert_eq!(rule(gem, inner), None, "into a bag inside it");
        assert_eq!(rule(gem, theirs), Some(S_OTHERS), "another's bag");
        assert_eq!(rule(gem, gem), Some(S_NOT_CONTAINER));
        assert_eq!(rule(bag, bag), Some(S_INTO_ITSELF));
        assert_eq!(rule(worn, bag), Some(S_UNREADY_FIRST));
        assert_eq!(rule(gem, cube), Some(S_ONLY_MOONSTONES));
        assert_eq!(rule(stone, cube), None, "a moonstone into the cube");
        assert_eq!(rule(cube, bag), None, "the cube itself into a bag");
        assert_eq!(rule(gem, book), Some(S_ONLY_SPELLS));
        assert_eq!(rule(spell, book), Some(S_SPELLS_STAY), "out of the book");
    }

    /// 0x12adc4 climbs the parent words at least once (`do … while`, 0x12adcc..0x12adfa): a
    /// slot on the map is not its own holder, its location bytes are read as a parent. A cycle
    /// of contained slots ends at the port's bound with -1.
    #[test]
    fn the_outer_holder_climbs_at_least_once_and_stops_on_a_cycle() {
        let mut s = crate::objects::tests::world();
        s.objects.type_frame[1] = 0x170;
        crate::objects::move_to(&mut s, 1, 500, 500, 0);
        let bag = crate::objects::create_in(&mut s, 0x40, IN_INVENTORY, 0, 1);
        let o = &s.objects;
        assert_eq!(o.outer_holder(bag), 1);
        let read_as_parent = o.parent(1);
        assert_ne!(read_as_parent, 1, "the location bytes are not the slot");
        assert_ne!(o.outer_holder(1), 1, "a map slot is not its own holder");
        let (a, b) = (0x200, 0x201);
        for (slot, parent) in [(a, b), (b, a)] {
            s.objects.obj_status[slot as usize] = IN_CONTAINER;
            s.objects.set_parent(slot, parent);
        }
        assert_eq!(s.objects.outer_holder(a), -1, "a cycle ends at the bound");
    }
}

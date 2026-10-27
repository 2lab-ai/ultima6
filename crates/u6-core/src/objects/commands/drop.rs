//! Drop (0x12a226) and its helpers: may an object go into a container (0x1289ca), does it
//! break when thrown (0x128a76), is there a surface (0x12a050), how many (0x12a106).

use std::cell::RefCell;

use super::get::{P_NOT_POSSIBLE, message};
use super::seams;
use super::text::{print, print_at, putc, queue_at};
use super::world::S_LINE;
use crate::engine::Io;
use crate::objects::iter::{begin_at, next_at, tile_of};
use crate::objects::moves::{
    add_to_owner, create_from_record, move_into, move_to, remove, take_from_owner,
};
use crate::objects::{
    IN_CONTAINER, READIED, Slot, in_type_set_670c, mark_takeable, pack_xyz, shown_name, stack_kind,
};
use crate::state::State;
use crate::ui::printf::Arg;

const S_LOCATION: u32 = 0x0012_858e;
const S_NL_LINE_NL: u32 = 0x0012_859a;
const S_IT_BROKE: u32 = 0x0012_85a0;
const S_HOW_MANY: u32 = 0x0012_8578;
const S_COUNT_LINE: u32 = 0x0012_8584; // "%d\n"
const S_ZERO: u32 = 0x0012_8588; // "0   \n"

/// Port-only (spec 08, slice K2): the strings this module passes to `printf` as its format,
/// for the locale layer's catalogue ([`crate::locale::strings`]).
pub(super) const PRINTED: &[u32] = &[
    S_LOCATION,
    S_NL_LINE_NL,
    S_IT_BROKE,
    S_HOW_MANY,
    S_COUNT_LINE,
    S_ZERO,
];
const P_NOTHING: i32 = -0x6c58;
/// Original: A4-0x66fa (15 words: types that break when thrown)
const FRAGILE: i32 = -0x66fa;

/// Original: 0x1289ca (objects: may `obj` go into `container`: one of the 9 container types
/// of A4-0x670c, not the spellbook 0x39 or 0x153, an open chest / 0xba / 0xc0, `obj` neither an
/// actor nor one of those chests)
pub fn fits_in(s: &State, container: Slot, obj: Slot) -> bool {
    if container == -1 {
        return false;
    }
    let k = s.objects.kind(container);
    if !in_type_set_670c(&s.objects, k) || obj < 0x100 || k == 0x39 || k == 0x153 {
        return false;
    }
    let chest = |k: u16| matches!(k, 0x62 | 0xba | 0xc0);
    if chest(k) && s.objects.tf(container) >> 10 != 0 {
        return false;
    }
    !chest(s.objects.kind(obj))
}

/// Original: 0x128a76 (objects: `kind` breaks when thrown: one of the 15 types at A4-0x66fa)
pub fn fragile(s: &State, kind: u16) -> bool {
    (0..15).any(|i| s.objcmds.data.a4_word(FRAGILE + 2 * i) == kind)
}

/// Original: 0x12a050 (objects: something at (x, y) on the leader's level offers a surface
/// (flags3 bit 1) before anything impassable)
pub fn surface_at(s: &mut State, x: u16, y: u16) -> bool {
    let z = u16::from(s.map.centre.z);
    let mut c = begin_at(&mut s.objects, &s.tables, x, y, z);
    while c >= 0 {
        let tile = usize::from(tile_of(&s.tables, s.objects.tf(c)));
        let f = &s.tables.tileflags;
        if f.flags3.get(tile).is_some_and(|b| b & 0x02 != 0) {
            return true;
        }
        if f.flags1.get(tile).is_some_and(|b| b & 0x02 != 0) {
            return false;
        }
        c = next_at(&mut s.objects, &s.tables);
    }
    false
}

/// Original: 0x12a106 (objects: how many of stack `obj` to drop)
///
/// Not a counted stack, readied, or a stack of one: 1. Else the how-many prompt (0x128578) and
/// a line of up to 4 characters (0x107cba, the panel cursor put away meanwhile); empty: the
/// whole stack, its count printed over the line; more than the stack, 0 or Escape: a zero
/// printed (0x128588) and 0; else the number.
pub async fn how_many(state: &RefCell<State>, io: &Io, obj: Slot) -> i16 {
    let (kind, n) = {
        let s = state.borrow();
        let k = stack_kind(&s.objects, s.objects.kind(obj));
        if k == 0 || s.objects.place(obj) == READIED {
            return 1;
        }
        let q = s.objects.qq_of(obj);
        (k, if k == 2 { q & 0xff } else { q } as i16)
    };
    let _ = kind;
    if n == 1 {
        return 1;
    }
    let (sel, row) = {
        let s = state.borrow();
        (s.ui.input.selection, s.ui.text.win().row)
    };
    print_at(state, io, S_HOW_MANY, &[]).await;
    let mode = {
        let mut s = state.borrow_mut();
        let mode = s.ui.input.targeting;
        crate::ui::cursor::panel_cursor_in(&mut s, io, 0);
        s.ui.input.targeting = 0;
        mode
    };
    let (_, line) = crate::ui::keyread::read_line(state, io, 4).await;
    {
        let mut s = state.borrow_mut();
        s.ui.input.targeting = mode;
        s.ui.input.selection = sel;
    }
    let v = super::parse_answer(&state.borrow(), &line, 0);
    if v == -1 {
        crate::ui::panels::set_cursor(state, io, 10, i16::from(row)).await;
        print_at(state, io, S_COUNT_LINE, &[Arg::Word(n as u16)]).await;
        return n;
    }
    if v > n || v == 0 || v == -2 {
        crate::ui::panels::set_cursor(state, io, 10, i16::from(row)).await;
        print_at(state, io, S_ZERO, &[]).await;
        return 0;
    }
    putc(state, io, b'\n').await;
    v
}

/// The quality / quantity word Drop moves for `n` of stack `obj`.
fn amount(s: &State, kind: u16, obj: Slot, n: i16) -> u16 {
    if kind == 2 {
        (s.objects.qq_of(obj) & 0xff00) | n as u16
    } else {
        n as u16
    }
}

/// Original: 0x12a226 (objects: Drop's first test, pseudocode l.23-28: nothing selected, an
/// object on the map, or the container the panel shows: the nothing message)
pub(crate) fn nothing_to_drop(s: &State, sel: Slot) -> bool {
    sel == -1 || s.objects.place(sel) == 0 || sel as u16 == s.ui.panel.container
}

/// Original: 0x12a226 (objects: the Drop command on the confirmed inventory object)
///
/// Nothing selected, an object on the map, or the container the panel shows: the nothing
/// message. The name; a readied object that resists (0x1122dc) only redraws the panel. How
/// many (0x12a106, 0 ends it), the location prompt (0x12858e) and the target key. The spot
/// must take it: the owner may stand there (0x11b1c0), or a surface, or a container it fits in
/// (0x1289ca), and the path must reach it (0x1228ba); else the not-possible message. Then it
/// is unreadied (0x11236c, the equipment slots), the owner's sums redone (0x11121e), 3
/// movement points spent, and it flies along the path. Thrown farther than the next tile a
/// fragile type breaks (the broke line, 0x1285a0). Otherwise
/// it lands in the container there, or on the spot (a counted stack splits: the dropped part
/// is a new object).
pub async fn drop_object(state: &RefCell<State>, io: &Io) {
    let sel = state.borrow().ui.input.selection;
    if nothing_to_drop(&state.borrow(), sel) {
        let msg = message(&state.borrow(), P_NOTHING);
        print_at(state, io, S_LINE, &[Arg::Str(&msg)]).await;
        return;
    }
    let name = shown_name(&state.borrow(), sel).into_bytes();
    print(state, io, &name, &[]).await;
    putc(state, io, b'\n').await;
    let (kind, sk, owner) = {
        let s = state.borrow();
        let kind = s.objects.kind(sel);
        let m = usize::try_from(s.ui.panel.member).unwrap_or(0).min(15);
        (
            kind,
            stack_kind(&s.objects, kind),
            Slot::from(s.actors.party.members[m]),
        )
    };
    let o = sel;
    if seams::cannot_drop(state, io, o).await != 0 {
        redraw_container(state, io).await;
        return;
    }
    let n = how_many(state, io, sel).await;
    if n == 0 {
        return;
    }
    print_at(state, io, S_LOCATION, &[]).await;
    {
        let mut s = state.borrow_mut();
        s.ui.input.target_pending = 1;
        if s.ui.input.targeting != 0 {
            crate::ui::cursor::panel_cursor_in(&mut s, io, 0);
            s.ui.input.targeting = 1;
        }
    }
    crate::ui::wait::read_key(state, io).await;
    putc(state, io, b'\n').await;
    let spot = {
        let mut s = state.borrow_mut();
        place_ok(&mut s, o, owner)
    };
    let Some(here) = spot else {
        let msg = message(&state.borrow(), P_NOT_POSSIBLE);
        print_at(state, io, S_NL_LINE_NL, &[Arg::Str(&msg)]).await;
        return;
    };
    land(&mut state.borrow_mut(), io, o, owner, here, kind, sk, n);
    crate::combat::play_effects(state, io).await;
    redraw_container(state, io).await;
}

/// Drop from the unready on (synchronous; the broke line is queued).
#[allow(clippy::too_many_arguments)]
pub(super) fn land(
    s: &mut State,
    io: &Io,
    o: Slot,
    owner: Slot,
    here: Slot,
    kind: u16,
    sk: u16,
    n: i16,
) {
    seams::end_worn_magic(s, io, o, owner);
    if s.objects.place(o) == READIED {
        let slot = seams::equipment_slot(s, o);
        if (0..8).contains(&slot) {
            s.inventory.set_equip(slot, 0);
        }
        if super::super::seams::ready_slot(s, o) == 8 {
            let m = usize::try_from(s.ui.panel.member).unwrap_or(0);
            if let Some(f) = s.ui.panel.member_flags.get_mut(m) {
                *f = 0;
            }
            s.inventory.set_equip(5, 0);
        }
    }
    seams::sum_inventory(s, owner);
    crate::actors::walk::spend_mp(s, owner as u16, 3);
    let (tx, ty) = s.ui.input.target;
    let (mut dx, mut dy) = (tx, ty);
    let tile = tile_of(&s.tables, s.objects.tf(o));
    seams::throw_path(s, owner, 8, &mut dx, &mut dy, tile);
    let z = u16::from(s.map.centre.z);
    let (ox, oy) = (s.objects.x(owner), s.objects.y(owner));
    let far = |t: i16, a: u16| t.wrapping_sub(a as i16).wrapping_abs() > 1;
    if (far(tx, ox) || far(ty, oy)) && fragile(s, kind) {
        if sk != 0 {
            let q = amount(s, sk, o, n);
            take_from_owner(s, owner, kind, q);
        } else if kind == 0x1a1 || kind == 0x7b {
            let f = if kind == 0x1a1 { 1 } else { 2 };
            let t = &mut s.objects.type_frame[o as usize];
            *t = (*t & 0x3ff) + (f << 10);
            move_to(s, o, dx as u16, dy as u16, z);
            mark_takeable(&mut s.objects, o);
        } else {
            remove(s, o);
        }
        queue_at(s, S_IT_BROKE, Vec::new());
    } else {
        if fits_in(s, here, o) && !surface_at(s, tx as u16, ty as u16) {
            if sk != 0 {
                let q = amount(s, sk, o, n);
                let r = add_to_owner(s, here, kind, q);
                if r != -1 {
                    mark_takeable(&mut s.objects, r);
                }
                take_from_owner(s, owner, kind, q);
            } else {
                move_into(s, o, here, IN_CONTAINER);
                mark_takeable(&mut s.objects, o);
            }
        } else if sk != 0 {
            let q = amount(s, sk, o, n);
            let [b0, b1, b2] = pack_xyz(dx as u16, dy as u16, z).to_packed();
            let [t0, t1] = kind.to_be_bytes();
            let [q0, q1] = q.to_be_bytes();
            let r = create_from_record(s, [0, b0, b1, b2, t0, t1, q0, q1]);
            // The original marks without testing (0x12a758) the -1 a full object table gives;
            // it still takes the count from the owner (0x12a766), as the port does.
            if r != -1 {
                mark_takeable(&mut s.objects, r);
            }
            take_from_owner(s, owner, kind, q);
        } else {
            move_to(s, o, dx as u16, dy as u16, z);
            mark_takeable(&mut s.objects, o);
        }
    }
    crate::map::view::update_view(s);
    s.ui.panel.dirty = false;
}

/// The spot test of Drop: the object may stand there, or a surface, or a container it fits
/// in; then the path (0x1228ba) must reach it (it may move the target). Gives the first
/// object at the target as it was before the path, which Drop puts the object into.
fn place_ok(s: &mut State, o: Slot, owner: Slot) -> Option<Slot> {
    let (tx, ty) = s.ui.input.target;
    let z = u16::from(s.map.centre.z);
    let here = begin_at(&mut s.objects, &s.tables, tx as u16, ty as u16, z);
    if tx == -1 || ty == -1 {
        return None;
    }
    let spot = seams::may_stand(s, o, tx as u16, ty as u16)
        || surface_at(s, tx as u16, ty as u16)
        || (here != -1 && fits_in(s, here, o));
    if !spot {
        return None;
    }
    let (mut x, mut y) = (tx, ty);
    let ok = seams::throw_path(s, owner, 8, &mut x, &mut y, 0);
    s.ui.input.target = (x, y);
    ok.then_some(here)
}

/// 0x111be0 on the container the panel shows (A4+0x1346).
async fn redraw_container(state: &RefCell<State>, io: &Io) {
    {
        let mut s = state.borrow_mut();
        let c = s.ui.panel.container as Slot;
        seams::inventory_contents(&mut s, c);
    }
    crate::main_loop::flush_prints(state, io).await;
}

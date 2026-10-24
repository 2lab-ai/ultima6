//! Spells on an object or actor without a missile (0x117d34): Detect Magic / Trap, Trap,
//! Untrap, Unlock Magic, Lock, Disable; with the checks they use (0x115228, 0x1165a4,
//! 0x116526).

use std::cell::RefCell;

use super::book::quality;
use super::effects::{FMT_NL_S_BANG, FMT_NL_S_NL, NEWLINE, print_at, print_name, report, say};
use super::seams;
use super::tables::Msg;
use crate::engine::Io;
use crate::objects::{Objects, Slot};
use crate::state::State;
use crate::ui::printf::Arg;

const MSG_CHARGE: u32 = 0x0011_5978;
const MSG_NOT_MAGICAL: u32 = 0x0011_5994;
pub(super) const MSG_POTION: u32 = 0x0011_59a8;
pub(super) const MSG_MAGICAL: u32 = 0x0011_59ba;
const MSG_TRAPPED: u32 = 0x0011_595e;
const MSG_NO_TRAP: u32 = 0x0011_596e;
/// Port-only (spec 08, slice K2; 2026-10-24): the detections' lines, for the locale layer's
/// catalogue ([`super::PRINTED`]). The charge line puts the spell's name through `%s`, read for
/// display (2026-10-25, [`super::tables::NAME_LABELS`]).
pub(super) const PRINTED: &[u32] = &[
    MSG_CHARGE,
    MSG_NOT_MAGICAL,
    MSG_POTION,
    MSG_MAGICAL,
    MSG_TRAPPED,
    MSG_NO_TRAP,
];
/// Object types.
const CHARGE: u16 = 0x150;
const TRAP: u16 = 0x151;
pub(super) const POTION: u16 = 0x113;
const CHEST: u16 = 0x62;
/// A trap's quality (the spell number of Trap).
const TRAP_QUALITY: i16 = 0x16;

/// Original: 0x115228 (magic: is the tile of `obj` one of the 0x4e magic tiles at A4-0x6c38)
pub fn magic_tile(s: &State, obj: Slot) -> bool {
    let tile = super::effects::tile_of(s, obj) as u16;
    s.magic.tables.magic_tiles.contains(&tile)
}

/// Original: 0x1165a4 (magic: is `kind` one of the four door types at A4-0x6b54)
pub fn is_door(s: &State, kind: u16) -> bool {
    s.magic.tables.doors.contains(&kind)
}

/// Original: 0x116526 (magic: the first object of `kind` (and quality `quality` unless 0)
/// that `actor` carries at any depth, -1 if none)
///
/// The objects after the actor in the list while they are carried (status bits 0x18) and
/// inside it (0x10c928). Converse uses it for its carried-object test.
pub fn carried_object(o: &Objects, actor: i16, kind: i16, quality: i16) -> i16 {
    let mut slot = o.next_of(actor);
    while slot >= 0 && o.status(slot) & 0x18 != 0 && o.contained_in(slot, actor) {
        if o.kind(slot) as i16 == kind
            && (quality == 0 || i16::from(self::quality(o, slot)) == quality)
        {
            return slot;
        }
        slot = o.next_of(slot);
    }
    -1
}

fn set_result(state: &RefCell<State>, r: u8) {
    state.borrow_mut().magic.result = r;
}

/// Original: 0x117d34 (magic: a spell on an object or actor in reach: the prompt and target,
/// the range (not for the detect, trap, untrap, unlock and lock spells), the kind of target,
/// the effect, the result)
pub async fn detect(
    state: &RefCell<State>,
    io: &Io,
    spell: u16,
    range: i16,
    kind: i16,
    chosen: bool,
) {
    super::effects::prompt_target(state, io, kind, range, chosen).await;
    let no_target = |s: &State| {
        let i = &s.ui.input;
        i.selection == -1 || (i.target.0 == -1 && spell != 1 && spell != 2)
    };
    let (missing, caster) = {
        let s = state.borrow();
        (no_target(&s), super::cast::caster(&s))
    };
    if missing {
        say(state, io, super::effects::FMT_S_NL, Msg::Nothing).await;
    } else {
        let sel = state.borrow().ui.input.selection;
        print_name(state, io, sel).await;
        print_at(state, io, NEWLINE, &[]).await;
    }
    seams::flash_actor(&mut state.borrow_mut(), io, caster, 0);
    if no_target(&state.borrow()) {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    let far = {
        let s = state.borrow();
        let at = s
            .objects
            .location(crate::objects::ObjectId(caster))
            .unwrap_or_default();
        let (tx, ty) = s.ui.input.target;
        tx.wrapping_sub(at.x as i16).wrapping_abs() > range
            || ty.wrapping_sub(at.y as i16).wrapping_abs() > range
    };
    if far && !matches!(spell, 1 | 2 | 0x16 | 0x18 | 0x24 | 0x17) {
        say(state, io, FMT_NL_S_BANG, Msg::OutOfRange).await;
        return;
    }
    let sel = state.borrow().ui.input.selection;
    if (kind == 1 && sel > 0xff) || (kind == -1 && sel < 0x100) {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    let (sel, k) = {
        let mut s = state.borrow_mut();
        let sel = seams::target_owner(&mut s, sel);
        s.ui.input.selection = sel;
        s.magic.result = 0;
        (sel, s.objects.kind(Slot::from(sel)))
    };
    match spell {
        1 => detect_magic(state, io, sel).await,
        2 => detect_trap(state, io, caster, sel).await,
        0x16 | 0x18 => {
            let mut s = state.borrow_mut();
            seams::flash_actor(&mut s, io, sel as u16, 1);
            let lockable = k == CHEST || is_door(&s, k);
            let trap = crate::objects::find_child(&mut s.objects, sel, TRAP as i16, TRAP_QUALITY);
            match (spell, lockable, trap) {
                (0x16, true, -1) => {
                    crate::objects::add_to_owner(&mut s, sel, TRAP, 0x1601);
                }
                (0x18, true, t) if t != -1 => {
                    let t = carried_object(&s.objects, sel, TRAP as i16, TRAP_QUALITY);
                    crate::objects::remove(&mut s, t);
                }
                _ => s.magic.result = 2,
            }
        }
        0x17 => {
            let mut s = state.borrow_mut();
            let frame = s.objects.tf(sel) >> 10;
            if is_door(&s, k) && frame & 0xc == 0xc {
                seams::door(&mut s, sel, false);
                s.magic.result = 0xff;
            } else if k == CHEST && frame == 3 {
                seams::chest(&mut s, sel, false);
                s.magic.result = 0xff;
            } else {
                s.magic.result = 2;
            }
        }
        0x24 => {
            let mut s = state.borrow_mut();
            if is_door(&s, k) {
                seams::door(&mut s, sel, true);
            } else if k == CHEST {
                seams::chest(&mut s, sel, true);
                s.magic.result = 0xff;
            } else {
                s.magic.result = 2;
            }
        }
        0x32 => super::effects::harm(state, io, spell).await,
        _ => {}
    }
    report(state, io).await;
}

/// Detect Magic (1): a magic tile gets the magical line (0x1159ba; a potion names its kind);
/// else a charge inside (type 0x150) shows its spell; else the not-magical line (0x115994).
/// Port (spec 08, 2026-10-25): the charge's spell name is read for display
/// ([`crate::locale::shown`]: the pack's form when it has one).
async fn detect_magic(state: &RefCell<State>, io: &Io, sel: Slot) {
    seams::flash_actor(&mut state.borrow_mut(), io, sel as u16, 1);
    let (found, charge) = {
        let mut s = state.borrow_mut();
        if magic_tile(&s, sel) {
            (1, None)
        } else {
            let mut o = crate::objects::first_child(&mut s.objects, sel);
            let mut charge = None;
            while o >= 0 {
                if s.objects.kind(o) == CHARGE {
                    let n = usize::from(quality(&s.objects, o));
                    charge = Some(crate::locale::shown(&s, s.magic.tables.name(n).to_vec()));
                    break;
                }
                o = crate::objects::next_child(&mut s.objects);
            }
            (if charge.is_some() { 2 } else { 0 }, charge)
        }
    };
    match (found, charge) {
        (_, Some(name)) => print_at(state, io, MSG_CHARGE, &[Arg::Str(&name)]).await,
        (0, None) => print_at(state, io, MSG_NOT_MAGICAL, &[]).await,
        _ => {
            let potion = {
                let s = state.borrow();
                let tf = s.objects.tf(sel);
                (tf & 0x3ff == POTION).then(|| {
                    let n = usize::from(tf >> 10);
                    // Port (spec 08, slice K2; 2026-10-24): read for display, so the locale
                    // pack's label of it when it has one ([`super::tables::LABELS`],
                    // [`crate::locale::shown`]).
                    let p = s.magic.tables.potions.get(n).cloned().unwrap_or_default();
                    crate::locale::shown(&s, p)
                })
            };
            match potion {
                Some(p) => print_at(state, io, MSG_POTION, &[Arg::Str(&p)]).await,
                None => print_at(state, io, MSG_MAGICAL, &[]).await,
            }
        }
    }
    set_result(state, 0xff);
}

/// Detect Trap (2): random 1..30 below the caster's intelligence finds each trap inside
/// (the trapped line 0x11595e per trap); otherwise, or with none, the no-trap line 0x11596e.
async fn detect_trap(state: &RefCell<State>, io: &Io, caster: u16, sel: Slot) {
    let traps = {
        let mut s = state.borrow_mut();
        seams::flash_actor(&mut s, io, sel as u16, 1);
        let r = s.rng.range(1, 0x1e);
        if r < crate::actors::leaves::intelligence(&s, caster) {
            let mut n = 0;
            let mut o = crate::objects::first_child(&mut s.objects, sel);
            while o >= 0 {
                if s.objects.kind(o) == TRAP {
                    n += 1;
                }
                o = crate::objects::next_child(&mut s.objects);
            }
            n
        } else {
            0
        }
    };
    for _ in 0..traps {
        print_at(state, io, MSG_TRAPPED, &[]).await;
    }
    if traps == 0 {
        print_at(state, io, MSG_NO_TRAP, &[]).await;
    }
    set_result(state, 0xff);
}

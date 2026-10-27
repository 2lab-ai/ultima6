//! The Use command 0x1305e0: target checks, then one handler by object type.

use std::cell::RefCell;

use super::defs::{self, MSG_NO_EFFECT, MSG_NOT_USABLE, MSG_NOTHING, MSG_OUT_OF_RANGE};
use super::{Ux, aboard, dist, flags3_of, leader, seams};
use crate::engine::Io;
use crate::objects::{ACTOR_SLOTS, READIED, Slot};
use crate::state::State;

/// Image strings 0x1305e0 prints (hunk 16).
pub(super) const FMT_LINE: u32 = 0x0012_c9ee; // format: the string, a newline
pub(super) const FMT_NL_LINE: u32 = 0x0012_c93e; // format: newline, the string, newline
pub(super) const FMT_BANG: u32 = 0x0012_cda6; // format: as above with an exclamation mark
const STR_HORSE: u32 = 0x0012_cfa0;
const STR_NL: u32 = 0x0012_ccfe;
pub(super) const STR_NOT_NOW: u32 = 0x0012_cee2;
const STR_IN_HAND: u32 = 0x0012_cfa8;
const STR_NOT_SOLO: u32 = 0x0012_cfc0;
const STR_NOT_READIED: u32 = 0x0012_cfd4;
/// The aboard-ship refusal (the data-hunk string at A4 - 0x744d).
const STR_ABOARD: u32 = 0x744d;

/// Port-only (spec 08, slice K2): the strings the command's checks pass to `printf` as its
/// format, and the line formats the handlers share, for the locale layer's catalogue
/// ([`super::PRINTED`]; 2026-10-25: the newline before the aboard refusal too, each handler's
/// file listing its own lines); the shared messages it prints through them are the catalogue's
/// labels ([`crate::locale::strings::SHARED_MESSAGES`]). The line formats and the newline hold
/// no word of their own: the catalogue leaves them out.
pub(super) const PRINTED: &[u32] = &[
    FMT_LINE,
    FMT_NL_LINE,
    FMT_BANG,
    STR_HORSE,
    STR_NL,
    STR_NOT_NOW,
    STR_IN_HAND,
    STR_NOT_SOLO,
    STR_NOT_READIED,
    crate::image::A4.wrapping_sub(STR_ABOARD),
];

/// Original: 0x130546 (objects: Use of `obj` is refused while aboard ship)
///
/// Types in the 10-word list at A4-0x65aa are refused, except the vessel the party is in and a
/// ship (0x19c) used from a skiff (0x19e).
pub fn refused_aboard(s: &State, io: &Io, obj: Slot) -> bool {
    let ty = s.objects.kind(obj);
    let (off, n) = defs::ABOARD_REFUSED;
    if !defs::a4_words(io, off, n).contains(&ty) {
        return false;
    }
    let lead = Slot::from(leader(s));
    if aboard(s) && obj == lead {
        return false;
    }
    !(s.objects.kind(lead) == 0x19e && ty == 0x19c)
}

/// 0x1305e0 (pseudocode l.15-34): the target the selection stands for, written back to the
/// selection: its actor (0x1240fa); an actor other than a horse or cow used while not aboard with
/// a map cell selected, or an object whose tile has flags3 bit 4 (a cover), gives the object
/// under it (0x12948a); that one's actor again. Returns it (-1 for none) and the type of the
/// first actor.
pub(crate) fn resolve(s: &mut State) -> (Slot, u16) {
    let cur = s.ui.input.selection;
    let sel = seams::actor_of(s, cur);
    s.ui.input.selection = sel;
    let ty = s.objects.kind(sel);
    let actor_case = sel < ACTOR_SLOTS as Slot
        && s.ui.input.target.0 != -1
        && !matches!(ty, 0x1ae | 0x1af | 0x1ac)
        && !aboard(s);
    let mut sel = sel;
    if sel >= 0 && (actor_case || flags3_of(s, sel) & 0x10 != 0) {
        sel = seams::object_under(s, sel);
        s.ui.input.selection = sel;
    }
    if sel == -1 {
        return (-1, ty);
    }
    let sel = seams::actor_of(s, sel);
    s.ui.input.selection = sel;
    (sel, ty)
}

/// 0x1305e0 (pseudocode l.55-95): `sel` lies on the map more than one tile from the leader on
/// either axis of the selected cell (the out-of-range message).
pub(crate) fn out_of_reach(s: &State, sel: Slot) -> bool {
    if s.objects.place(sel) != 0 {
        return false;
    }
    let lead = Slot::from(leader(s));
    let (lx, ly) = (s.objects.x(lead) as i16, s.objects.y(lead) as i16);
    let (tx, ty) = s.ui.input.target;
    dist(tx, lx) > 1 || dist(ty, ly) > 1
}

/// 0x1305e0 (pseudocode l.96-107): `sel` is a door with an object on top that is neither a door
/// nor a cover, or a secret door (0x14e) that is not on top (the not-now line 0x12cee2).
pub(crate) fn not_now(s: &mut State, sel: Slot) -> bool {
    let ty = s.objects.kind(sel);
    let z = u16::from(s.map.centre.z);
    let (x, y) = (s.objects.x(sel), s.objects.y(sel));
    let top = super::first_at(s, x, y, z);
    let top_ty = s.objects.kind(top);
    let door = seams::is_door(s, ty) && !seams::is_door(s, top_ty) && flags3_of(s, top) & 0x10 == 0;
    door || (ty == 0x14e && top_ty != 0x14e)
}

/// 0x1305e0 (pseudocode l.108-113): `sel` is readied (status 0x18) while the panel shows another
/// member than the leader, and it is not a torch (the in-hand line 0x12cfa8).
pub(crate) fn in_other_hand(s: &State, sel: Slot) -> bool {
    let other = i16::from(s.actors.party.leader_index) != s.ui.panel.member;
    other && s.objects.place(sel) == READIED && s.objects.kind(sel) != 0x5a
}

/// What a case of the type switch refuses before it acts (`cases.rs`, 0x13095e..0x130efa).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaseRefusal {
    /// The not-usable, not-readied or no-effect line of the case.
    Case,
    /// The solo-mode line (the ladder).
    Solo,
}

/// 0x1305e0's switch (pseudocode l.114 on, `cases.rs`): the refusal a case of type `ty` gives
/// `obj` of frame `frame` from the state alone, before any draw or prompt: the vortex cube off
/// the shrine (0x3e, A4-0x7afb set), a wand on the map or not readied (0x4e), an orb on the map
/// (0x57), a sextant or balloon basket off levels 0 and 5 (0x5d, 0x1a4), a chest carried (0x62),
/// a shovel or pick not readied (0x67, 0x68), a brazier of frame bit 1 (0xce), a crate of frame
/// other than 1 (0xba, 0xc0), a vessel carried (0x19c, 0x19e, 0x19f, 0x1a7), a ladder in solo
/// mode (0x131). The switch keeps its own copy of these tests; the menu's test runs the switch
/// to hold the two together.
pub(crate) fn case_refusal(s: &State, obj: Slot, ty: u16, frame: u16) -> Option<CaseRefusal> {
    let place = s.objects.place(obj);
    let off_levels = !matches!(s.map.centre.z, 0 | 5);
    let refused = match ty {
        0x3e => s.map.flag_7afb != 0,
        0x4e | 0x67 | 0x68 => place != READIED,
        0x57 => place == 0,
        0x5d | 0x1a4 => off_levels,
        0x62 | 0x19c | 0x19e | 0x19f | 0x1a7 => place != 0,
        0xce => frame & 2 != 0,
        0xba | 0xc0 => frame != 1,
        0x131 => {
            return (crate::ui::globals::solo_member(&s.actors) != -1).then_some(CaseRefusal::Solo);
        }
        _ => false,
    };
    refused.then_some(CaseRefusal::Case)
}

/// What the checks of 0x1305e0 decided before the switch.
enum Checked {
    /// Refused: the command ends without the end-of-use step.
    Refused,
    /// Go on with the target, its type and frame.
    Use(Slot, u16, u16),
}

/// Original: 0x1305e0 (objects: Use command: target checks, dispatch to per-object handlers)
///
/// Order: the target (A4-0x2f04) becomes the actor it stands for (combat 0x1240fa); an actor
/// other than a horse or cow used while not aboard with a map cell selected, or an object whose
/// tile has flags3 bit 4, is replaced by the object under it (0x12948a); none → the nothing
/// message. The name is printed (the line 0x12cfa0 for a ridden horse); the type must be usable
/// (0x128b6a) and not refused aboard ship (0x130546). A map object turns the leader towards the
/// pointer (0x105016, 0x11ba5e, 0x109e74) and must be next to the leader (the out-of-range
/// message). A door blocked by an object, or a secret door not on top, gives the line 0x12cee2;
/// a readied object of another member than the panel's (except a torch) the line 0x12cfa8.
/// Then the handler; after any
/// handler the leader pays 5 movement points (0x11c650), the view flags and the view are
/// refreshed (0x109e74, 0x102ef0).
pub async fn use_command(state: &RefCell<State>, io: &Io) {
    let ux = Ux::new(state, io);
    let Checked::Use(obj, ty, frame) = check(ux).await else {
        return;
    };
    super::cases::dispatch_type(ux, obj, ty, frame).await;
    ux.flush().await;
    let lead = leader(&ux.s());
    {
        let mut s = ux.s();
        crate::actors::walk::spend_mp(&mut s, u16::from(lead), 5);
        crate::map::view::update_view(&mut s);
    }
    ux.refresh();
    ux.flush().await;
}

/// 0x1305e0 up to the switch.
async fn check(ux: Ux<'_>) -> Checked {
    let (sel, first_ty) = resolve(&mut ux.s());
    if sel == -1 {
        ux.say(FMT_LINE, MSG_NOTHING).await;
        return Checked::Refused;
    }
    if first_ty == 0x1af {
        ux.print(STR_HORSE, &[]).await;
    } else {
        ux.name(sel).await;
        ux.put(b'\n').await;
    }
    let ty = ux.s().objects.kind(sel);
    if !crate::objects::commands::in_type_list(&ux.s(), ty) {
        ux.say(FMT_NL_LINE, MSG_NOT_USABLE).await;
        return Checked::Refused;
    }
    let refused = {
        let s = ux.s();
        aboard(&s) && refused_aboard(&s, ux.io, sel)
    };
    if refused {
        ux.print(STR_NL, &[]).await;
        ux.print_a4(STR_ABOARD).await;
        return Checked::Refused;
    }
    if !in_reach(ux, sel).await {
        return Checked::Refused;
    }
    if not_now(&mut ux.s(), sel) {
        ux.print(STR_NOT_NOW, &[]).await;
        return Checked::Refused;
    }
    if in_other_hand(&ux.s(), sel) {
        ux.print(STR_IN_HAND, &[]).await;
        return Checked::Refused;
    }
    let frame = ux.s().objects.tf(sel) >> 10;
    Checked::Use(sel, ty, frame)
}

/// A map object: face the pointer, then the leader must be next to the selected cell.
async fn in_reach(ux: Ux<'_>, sel: Slot) -> bool {
    let far = {
        let mut s = ux.s();
        if s.objects.place(sel) != 0 {
            return true;
        }
        let (px, py) = s.ui.input.pointer;
        let dir = seams::pointer_direction(&s, px, py);
        let lead = leader(&s);
        if dir != -1 {
            crate::actors::walk::face(&mut s, u16::from(lead), dir as u16);
            crate::map::view::update_view(&mut s);
        }
        out_of_reach(&s, sel)
    };
    if far {
        ux.say(FMT_BANG, MSG_OUT_OF_RANGE).await;
    }
    !far
}

/// The not-usable message the type cases print when a condition fails.
pub(super) async fn not_usable(ux: Ux<'_>) {
    ux.say(FMT_NL_LINE, MSG_NOT_USABLE).await;
}

/// The no-effect message.
pub(super) async fn no_effect(ux: Ux<'_>) {
    ux.say(FMT_NL_LINE, MSG_NO_EFFECT).await;
}

/// The solo-mode refusal 0x12cfc0 (the ladder case).
pub(super) async fn not_solo(ux: Ux<'_>) {
    ux.print(STR_NOT_SOLO, &[]).await;
}

/// The not-readied line 0x12cfd4 (shovels, wands).
pub(super) async fn not_readied(ux: Ux<'_>) {
    ux.print(STR_NOT_READIED, &[]).await;
}

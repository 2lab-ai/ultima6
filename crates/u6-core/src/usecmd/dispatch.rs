//! The Use command 0x1305e0: target checks, then one handler by object type.

use std::cell::RefCell;

use super::defs::{self, MSG_NO_EFFECT, MSG_NOT_USABLE, MSG_NOTHING, MSG_OUT_OF_RANGE};
use super::{Ux, aboard, dist, flags3_of, leader, seams};
use crate::engine::Io;
use crate::objects::{ACTOR_SLOTS, READIED, Slot};
use crate::state::State;

/// Image strings 0x1305e0 prints (hunk 16).
pub(super) const FMT_LINE: u32 = 0x0012_c9ee; // "%s\n"
pub(super) const FMT_NL_LINE: u32 = 0x0012_c93e; // "\n%s\n"
pub(super) const FMT_BANG: u32 = 0x0012_cda6; // "\n%s!\n"
const STR_HORSE: u32 = 0x0012_cfa0;
const STR_NL: u32 = 0x0012_ccfe;
pub(super) const STR_NOT_NOW: u32 = 0x0012_cee2;
const STR_IN_HAND: u32 = 0x0012_cfa8;
const STR_NOT_SOLO: u32 = 0x0012_cfc0;
const STR_NOT_READIED: u32 = 0x0012_cfd4;
/// "Not while aboard ship!" (A4 - 0x744d).
const STR_ABOARD: u32 = 0x744d;

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
/// tile has flags3 bit 4, is replaced by the object under it (0x12948a); none → "nothing". The
/// name is printed ("horse" for a ridden horse); the type must be usable (0x128b6a) and not
/// refused aboard ship (0x130546). A map object turns the leader towards the pointer (0x105016,
/// 0x11ba5e, 0x109e74) and must be next to the leader ("Out of range"). A door blocked by an
/// object, or a secret door not on top, gives "Not now!"; a readied object of another member
/// than the panel's (except a torch) "Must be in your hand.". Then the handler; after any
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
    let first = {
        let mut s = ux.s();
        let cur = s.ui.input.selection;
        let sel = seams::actor_of(&mut s, cur);
        s.ui.input.selection = sel;
        let ty = s.objects.kind(sel);
        let actor_case = sel < ACTOR_SLOTS as Slot
            && s.ui.input.target.0 != -1
            && !matches!(ty, 0x1ae | 0x1af | 0x1ac)
            && !aboard(&s);
        let mut sel = sel;
        if sel >= 0 && (actor_case || flags3_of(&s, sel) & 0x10 != 0) {
            sel = seams::object_under(&mut s, sel);
            s.ui.input.selection = sel;
        }
        (sel, ty)
    };
    let (sel, first_ty) = first;
    if sel == -1 {
        ux.say(FMT_LINE, MSG_NOTHING).await;
        return Checked::Refused;
    }
    let sel = {
        let mut s = ux.s();
        let cur = s.ui.input.selection;
        let sel = seams::actor_of(&mut s, cur);
        s.ui.input.selection = sel;
        sel
    };
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
    let (not_now, in_hand) = {
        let mut s = ux.s();
        let z = u16::from(s.map.centre.z);
        let (x, y) = (s.objects.x(sel), s.objects.y(sel));
        let top = super::first_at(&mut s, x, y, z);
        let top_ty = s.objects.kind(top);
        let door =
            seams::is_door(&s, ty) && !seams::is_door(&s, top_ty) && flags3_of(&s, top) & 0x10 == 0;
        let secret = ty == 0x14e && top_ty != 0x14e;
        let other = i16::from(s.actors.party.leader_index) != s.ui.panel.member;
        let readied = s.objects.place(sel) == READIED;
        (door || secret, other && readied && ty != 0x5a)
    };
    if not_now {
        ux.print(STR_NOT_NOW, &[]).await;
        return Checked::Refused;
    }
    if in_hand {
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
        let lead = Slot::from(lead);
        let (lx, ly) = (s.objects.x(lead) as i16, s.objects.y(lead) as i16);
        let (tx, ty) = s.ui.input.target;
        dist(tx, lx) > 1 || dist(ty, ly) > 1
    };
    if far {
        ux.say(FMT_BANG, MSG_OUT_OF_RANGE).await;
    }
    !far
}

/// The "Not usable" line the type cases print when a condition fails.
pub(super) async fn not_usable(ux: Ux<'_>) {
    ux.say(FMT_NL_LINE, MSG_NOT_USABLE).await;
}

/// "No effect".
pub(super) async fn no_effect(ux: Ux<'_>) {
    ux.say(FMT_NL_LINE, MSG_NO_EFFECT).await;
}

/// "Not in solo mode." (the ladder case).
pub(super) async fn not_solo(ux: Ux<'_>) {
    ux.print(STR_NOT_SOLO, &[]).await;
}

/// "Not readied." (shovels, wands).
pub(super) async fn not_readied(ux: Ux<'_>) {
    ux.print(STR_NOT_READIED, &[]).await;
}

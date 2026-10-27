//! The type switch of 0x1305e0 (0x13095e..0x130efa).

use super::dispatch::{no_effect, not_readied, not_solo, not_usable};
use super::{Ux, food, mech, misc, travel};
use crate::objects::commands as hunk15;
use crate::objects::{READIED, Slot};
use crate::state::State;

/// What a case of the switch refuses from the state alone, before it acts: the line it prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaseRefusal {
    /// The not-usable message.
    NotUsable,
    /// The not-readied line 0x12cfd4.
    NotReadied,
    /// The no-effect message.
    NoEffect,
    /// The solo-mode line 0x12cfc0.
    NotSolo,
}

/// The refusal the case of type `ty` gives `obj` of frame `frame` at its own test, before it acts
/// (0x1305e0 pseudocode l.114 on, the switch 0x13095e..0x130efa), or `None`: the vortex cube with
/// A4-0x7afb set (0x3e, pseudocode l.114-118), a wand on the map (not usable) or carried but not
/// readied (0x4e), an orb on the map (0x57), a sextant or a balloon basket off levels 0 and 5
/// (0x5d, 0x1a4), a chest or a vessel carried (0x62, 0x19c, 0x19e, 0x19f, 0x1a7), a shovel or a
/// pick not readied (0x67, 0x68), a brazier of frame bit 1 (0xce), a ladder in solo mode (0x131).
/// A crate's frame (0xba, 0xc0) is no refusal: its case prints the not-usable line and searches
/// the crate all the same.
pub(crate) fn refusal(s: &State, obj: Slot, ty: u16, frame: u16) -> Option<CaseRefusal> {
    let place = s.objects.place(obj);
    let on_levels = matches!(s.map.centre.z, 0 | 5);
    let refused = match ty {
        0x3e => s.map.flag_7afb != 0,
        0x4e if place == 0 => true,
        0x4e | 0x67 | 0x68 => return (place != READIED).then_some(CaseRefusal::NotReadied),
        0x57 => place == 0,
        0x5d | 0x1a4 => !on_levels,
        0x62 | 0x19c | 0x19e | 0x19f | 0x1a7 => place != 0,
        0xce => frame & 2 != 0,
        0x131 => return (solo_member(s) != -1).then_some(CaseRefusal::NotSolo),
        _ => false,
    };
    match (refused, ty) {
        (false, _) => None,
        (true, 0x3e) => Some(CaseRefusal::NoEffect),
        (true, _) => Some(CaseRefusal::NotUsable),
    }
}

fn solo_member(s: &State) -> i8 {
    crate::ui::globals::solo_member(&s.actors)
}

/// One case of the switch for target `obj` of type `ty` and frame `frame`; the caller runs the
/// end-of-use step afterwards (every case reaches it, refusals inside a case included). A case's
/// refusal ([`refusal`]) is its first test: its line, and nothing else of the case.
pub(super) async fn dispatch_type(ux: Ux<'_>, obj: Slot, ty: u16, frame: u16) {
    let refused = refusal(&ux.s(), obj, ty, frame);
    match refused {
        Some(CaseRefusal::NotUsable) => return not_usable(ux).await,
        Some(CaseRefusal::NotReadied) => return not_readied(ux).await,
        Some(CaseRefusal::NoEffect) => return no_effect(ux).await,
        Some(CaseRefusal::NotSolo) => return not_solo(ux).await,
        None => {}
    }
    match ty {
        0x3e => {
            if !Box::pin(misc::vortex_cube(ux, obj)).await {
                no_effect(ux).await;
            }
        }
        0x3f | 0x40 => Box::pin(hunk15::use_key(ux.state, ux.io, obj)).await,
        0x49 => mech::bury(ux, obj).await,
        0x4d => hunk15::use_up(ux.state, ux.io, obj).await,
        0x4e => Box::pin(misc::wand(ux, obj)).await,
        0x57 => Box::pin(travel::orb(ux)).await,
        0x5a => {
            hunk15::light_torch(&mut ux.s(), obj);
            ux.flush().await;
        }
        0x5d => seams_sextant(ux).await,
        0x61 => food::study_scroll(ux).await,
        0x62 => {
            hunk15::chest(&mut ux.s(), obj, false, false, false);
            ux.flush().await;
        }
        0x67 | 0x68 => Box::pin(mech::dig(ux)).await,
        0x73..=0x75 => food::drink(ux, obj).await,
        0x7a | 0x91 | 0xa4 | 0xce | 0xfd => {
            hunk15::douse_or_light(&mut ux.s(), obj);
            ux.flush().await;
        }
        0x5f | 0x60 | 0x80..=0x85 | 0x87 | 0xb4 | 0xb8 | 0xd1 | 0xd2 | 0x109 => {
            food::eat(ux, obj).await
        }
        0x99 => misc::instrument(ux, 0x13).await,
        0x9a => Box::pin(travel::telescope(ux, obj)).await,
        0x9b => Box::pin(travel::sextant_travel(ux)).await,
        0x9c => misc::instrument(ux, 3).await,
        0x9d => misc::instrument(ux, 2).await,
        0x9e => misc::instrument(ux, 0x12).await,
        0x128 => misc::instrument(ux, 0x14).await,
        0xa9 => food::squeak(ux).await,
        0xae => mech::switch(ux, obj).await,
        0xb5 => food::churn(ux).await,
        0xb6 => food::honey(ux).await,
        0xba | 0xc0 => {
            if frame == 1 {
                let mut s = ux.s();
                let tf = s.objects.tf(obj) & 0x3ff;
                super::set_tf(&mut s, obj, tf);
            } else {
                not_usable(ux).await;
            }
            let (x, y) = ux.s().ui.input.target;
            hunk15::search(&mut ux.s(), obj, x as u16, y as u16);
            ux.flush().await;
        }
        0xd4 => food::fan(ux).await,
        0xdd => mech::cannon(ux, obj).await,
        0xdf => mech::light_keg(ux, obj).await,
        0xe9 => food::fill_bucket(ux, food::Bucket::Water).await,
        0x1ac => food::milk(ux).await,
        0xea => Box::pin(misc::wish(ux)).await,
        0xec | 0x1a3 => misc::machine(ux).await,
        0xf2..=0xf9 => Box::pin(misc::mantra(ux, obj)).await,
        0x108 => misc::fish(ux).await,
        0x10c => mech::lever(ux, obj).await,
        0x10e => Box::pin(travel::balloon(ux)).await,
        0x113 => Box::pin(food::potion(ux, obj)).await,
        0x116 | 0x118 => {
            crate::objects::toggle_passthrough(&mut ux.s(), obj);
            ux.flush().await;
        }
        0x120 => mech::drawbridge(ux, obj).await,
        0x129..=0x12c => {
            hunk15::door(&mut ux.s(), obj, false, false, false);
            ux.flush().await;
        }
        0x131 => {
            if let Some(id) = crate::objects::ObjectId::from_slot(obj) {
                crate::map::travel::climb(&mut ux.s(), id);
            }
            ux.flush().await;
        }
        0x139 => misc::serpents(ux).await,
        0x14e => hunk15::toggle_lit(&mut ux.s(), obj),
        0x19c | 0x19e | 0x19f | 0x1a7 => Box::pin(travel::board(ux, obj)).await,
        0x1a4 => Box::pin(travel::inflate(ux, obj)).await,
        0x1ae => travel::mount(ux, obj).await,
        0x1af => {
            hunk15::dismount(&mut ux.s(), obj);
            ux.flush().await;
        }
        _ => {}
    }
}

/// 0x11a470 through the seam.
async fn seams_sextant(ux: Ux<'_>) {
    super::seams::sextant(ux.state, ux.io).await;
}

//! The type switch of 0x1305e0 (0x13095e..0x130efa).

use super::dispatch::{no_effect, not_readied, not_solo, not_usable};
use super::{Ux, food, hunk15, mech, misc, travel};
use crate::objects::{READIED, Slot};

/// One case of the switch for target `obj` of type `ty` and frame `frame`; the caller runs the
/// end-of-use step afterwards (every case reaches it, refusals inside a case included).
pub(super) async fn dispatch_type(ux: Ux<'_>, obj: Slot, ty: u16, frame: u16) {
    let place = ux.s().objects.place(obj);
    let z = ux.s().map.centre.z;
    match ty {
        0x3e => {
            let shrine = ux.s().map.flag_7afb == 0;
            if !shrine || !Box::pin(misc::vortex_cube(ux, obj)).await {
                no_effect(ux).await;
            }
        }
        0x3f | 0x40 => Box::pin(hunk15::key(ux, obj)).await,
        0x49 => mech::bury(ux, obj).await,
        0x4d => hunk15::gem(ux, obj).await,
        0x4e => match place {
            0 => not_usable(ux).await,
            READIED => Box::pin(misc::wand(ux, obj)).await,
            _ => not_readied(ux).await,
        },
        0x57 if place == 0 => not_usable(ux).await,
        0x57 => Box::pin(travel::orb(ux)).await,
        0x5a => hunk15::torch(ux, obj).await,
        0x5d if z == 0 || z == 5 => seams_sextant(ux).await,
        0x5d => not_usable(ux).await,
        0x61 => food::study_scroll(ux).await,
        0x62 if place != 0 => not_usable(ux).await,
        0x62 => hunk15::chest(ux, obj, 0, 0, 0).await,
        0x67 | 0x68 if place == READIED => Box::pin(mech::dig(ux)).await,
        0x67 | 0x68 => not_readied(ux).await,
        0x73..=0x75 => food::drink(ux, obj).await,
        0x7a | 0x91 | 0xa4 | 0xce | 0xfd => {
            if ty == 0xce && frame & 2 != 0 {
                not_usable(ux).await;
            } else {
                hunk15::fixed_light(ux, obj).await;
            }
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
            super::seams::search(ux.state, ux.io, obj, x, y).await;
        }
        0xd4 => food::fan(ux).await,
        0xdd => mech::cannon(ux, obj).await,
        0xdf => mech::light_keg(ux, obj).await,
        0xe9 => food::fill_bucket(ux, food::Bucket::Water).await,
        0x1ac => food::fill_bucket(ux, food::Bucket::Milk).await,
        0xea => Box::pin(misc::wish(ux)).await,
        0xec | 0x1a3 => misc::machine(ux).await,
        0xf2..=0xf9 => Box::pin(misc::mantra(ux, obj)).await,
        0x108 => misc::take_from_well(ux).await,
        0x10c => mech::lever(ux, obj).await,
        0x10e => Box::pin(travel::balloon(ux)).await,
        0x113 => Box::pin(food::potion(ux, obj)).await,
        0x116 | 0x118 => {
            crate::objects::toggle_passthrough(&mut ux.s(), obj);
            ux.flush().await;
        }
        0x120 => mech::drawbridge(ux, obj).await,
        0x129..=0x12c => hunk15::door(ux, obj, 0, 0, 0).await,
        0x131 => {
            let solo = crate::ui::globals::solo_member(&ux.s().actors);
            if solo == -1 {
                if let Some(id) = crate::objects::ObjectId::from_slot(obj) {
                    crate::map::travel::climb(&mut ux.s(), id);
                }
                ux.flush().await;
            } else {
                not_solo(ux).await;
            }
        }
        0x139 => misc::serpents(ux).await,
        0x14e => hunk15::secret_door(&mut ux.s(), obj),
        0x19c | 0x19e | 0x19f | 0x1a7 if place == 0 => Box::pin(travel::board(ux, obj)).await,
        0x19c | 0x19e | 0x19f | 0x1a7 => not_usable(ux).await,
        0x1a4 if z == 0 || z == 5 => Box::pin(travel::inflate(ux, obj)).await,
        0x1a4 => not_usable(ux).await,
        0x1ae => travel::mount(ux, obj).await,
        0x1af => {
            travel::dismount(&mut ux.s(), obj);
            ux.flush().await;
        }
        _ => {}
    }
}

/// 0x11a470 through the seam.
async fn seams_sextant(ux: Ux<'_>) {
    super::seams::sextant(ux.state, ux.io).await;
}

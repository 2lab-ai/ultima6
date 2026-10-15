//! Eating, drinking, potions and the farmyard: food 0x130380, drinks 0x12f0a4, potions
//! 0x12d526, buckets 0x12d336 / 0x12d3c4, the churn 0x12d452, the beehive 0x12d4bc, the
//! scroll 0x12e912, the mouse 0x13051a and the fan 0x130526.

use super::defs::{self, MSG_DONE, MSG_NO_EFFECT, MSG_NOTHING, MSG_ON_WHOM};
use super::dispatch::{FMT_LINE, FMT_NL_LINE};
use super::{Ux, seams, set_quantity};
use crate::objects::Slot;

const STR_NEED_BUCKET: u32 = 0x0012_c952;
const STR_WATER_HEAVY: u32 = 0x0012_c968;
const STR_MILK_HEAVY: u32 = 0x0012_c994;
const STR_NEED_MILK: u32 = 0x0012_c9c0;
const STR_NEED_JAR: u32 = 0x0012_c9d6;
const FMT_STR: u32 = 0x0012_c94a; // format: the string
const STR_STUDY: u32 = 0x0012_cb4a;
const STR_DRINK: u32 = 0x0012_cc7e;
const STR_EAT: u32 = 0x0012_cf52;
const STR_SQUEAK: u32 = 0x0012_cf80;
const STR_BREEZE: u32 = 0x0012_cf8a;

/// Original: 0x130380 (objects: eat food: one off a stack, or the object goes)
///
/// The eating line (0x12cf52); a counted stack (0x10cef2 ≠ 0) with more than one loses one,
/// anything else is removed; a carried one redraws the inventory.
pub async fn eat(ux: Ux<'_>, obj: Slot) {
    ux.print(STR_EAT, &[]).await;
    let mut s = ux.s();
    let carried = s.objects.place(obj) != 0;
    let kind = s.objects.kind(obj);
    let q = super::quantity(&s, obj);
    if crate::objects::stack_kind(&s.objects, kind) != 0 && q > 1 {
        set_quantity(&mut s, obj, q - 1);
    } else {
        crate::objects::remove(&mut s, obj);
    }
    if carried {
        seams::redraw_inventory(&mut s);
    }
}

/// Original: 0x12f0a4 (objects: drink wine, mead or ale: the object goes, alcohol + 1)
pub async fn drink(ux: Ux<'_>, obj: Slot) {
    {
        let mut s = ux.s();
        crate::objects::remove(&mut s, obj);
        let a = crate::actors::tail::ALCOHOL;
        if let Some(b) = s.actors.objlist_tail.get_mut(a) {
            *b = b.wrapping_add(1);
        }
    }
    ux.print(STR_DRINK, &[]).await;
    let mut s = ux.s();
    if s.ui.panel.mode == 0x92 {
        seams::redraw_inventory(&mut s);
    }
}

/// Which bucket a well or cow fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    Water,
    Milk,
}

/// Original: 0x12d3c4 (objects: milk a cow: an empty bucket 0xb2 becomes a bucket of milk 0xb4)
///
/// The body of 0x12d336 with the milk type and message.
pub async fn milk(ux: Ux<'_>) {
    fill_bucket(ux, Bucket::Milk).await;
}

/// Original: 0x12d336 (objects: fill a bucket at a well: an empty bucket 0xb2 becomes 0xb3)
///
/// [`milk`] (0x12d3c4) is the same routine for a cow. The leader needs an empty bucket and the
/// strength to carry it full (20 × strength at least the carried weight + 0x20); the empty one
/// goes and a full one is given (0x10cf7c), the done message.
pub async fn fill_bucket(ux: Ux<'_>, which: Bucket) {
    let (member, bucket) = {
        let mut s = ux.s();
        let li = usize::from(s.actors.party.leader_index);
        let m = Slot::from(crate::objects::party_member(&s, li));
        (m, crate::objects::find_child(&mut s.objects, m, 0xb2, 0))
    };
    if bucket == -1 {
        ux.print(STR_NEED_BUCKET, &[]).await;
        return;
    }
    let heavy = {
        let mut s = ux.s();
        let w = seams::carried_weight(&mut s, member);
        let strength = i16::from(s.actors.attrs.strength[member as usize & 0xff]);
        w.wrapping_add(0x20) > strength.wrapping_mul(0x14)
    };
    if heavy {
        let msg = match which {
            Bucket::Water => STR_WATER_HEAVY,
            Bucket::Milk => STR_MILK_HEAVY,
        };
        ux.print(msg, &[]).await;
        return;
    }
    {
        let mut s = ux.s();
        crate::objects::remove(&mut s, bucket);
        let full = match which {
            Bucket::Water => 0xb3,
            Bucket::Milk => 0xb4,
        };
        crate::objects::add_to_owner(&mut s, member, full, 1);
    }
    ux.say(FMT_NL_LINE, MSG_DONE).await;
}

/// Original: 0x12d452 (objects: churn: a bucket of milk 0xb4 becomes butter 0x60 and an empty
/// bucket 0xb2)
pub async fn churn(ux: Ux<'_>) {
    let (member, milk) = {
        let mut s = ux.s();
        let li = usize::from(s.actors.party.leader_index);
        let m = Slot::from(crate::objects::party_member(&s, li));
        (m, crate::objects::find_child(&mut s.objects, m, 0xb4, 0))
    };
    if milk == -1 {
        ux.print(STR_NEED_MILK, &[]).await;
        return;
    }
    {
        let mut s = ux.s();
        crate::objects::remove(&mut s, milk);
        crate::objects::add_to_owner(&mut s, member, 0x60, 1);
        crate::objects::add_to_owner(&mut s, member, 0xb2, 1);
    }
    ux.say(FMT_NL_LINE, MSG_DONE).await;
}

/// Original: 0x12d4bc (objects: a beehive fills a honey jar: 0xb7 → jar of honey 0xb8)
pub async fn honey(ux: Ux<'_>) {
    let jar = {
        let mut s = ux.s();
        let li = usize::from(s.actors.party.leader_index);
        let m = Slot::from(crate::objects::party_member(&s, li));
        crate::objects::find_child(&mut s.objects, m, 0xb7, 0)
    };
    if jar == -1 {
        ux.print(STR_NEED_JAR, &[]).await;
        return;
    }
    {
        let mut s = ux.s();
        let tf = s.objects.tf(jar);
        super::set_tf(&mut s, jar, (tf & 0xfc00).wrapping_add(0xb8));
    }
    ux.say(FMT_NL_LINE, MSG_DONE).await;
    seams::redraw_inventory(&mut ux.s());
}

/// Original: 0x12d526 (objects: drink a potion: ask on whom, the effect by its frame)
///
/// Frame 0 wakes (status 0x04 off, the base type back), 1 cures poison (0x08 off), 2 heals
/// (magic 0x116d0e(6)), 3 poisons and 4 puts to sleep unless the monster kind resists (bytes
/// at A4-0x7dbd), 5 protects (status 0x01), 6 makes invisible (object status 0x02), 7 x-ray
/// (0x119200). Only party members take effect; the potion goes and the roster / stats panel
/// is marked for a redraw.
pub async fn potion(ux: Ux<'_>, obj: Slot) {
    ux.say(FMT_STR, MSG_ON_WHOM).await;
    {
        let mut s = ux.s();
        s.ui.input.target_pending = 1;
        if s.ui.input.targeting != 0 {
            crate::ui::cursor::panel_cursor_in(&mut s, ux.io, 0);
            s.ui.input.targeting = 1;
            s.ui.input.range = 7;
        }
    }
    ux.key().await;
    let (who, tx, ty) = {
        let s = ux.s();
        let (x, y) = s.ui.input.target;
        (s.ui.input.selection, x, y)
    };
    if who == -1 || tx == -1 || ty == -1 {
        ux.say(FMT_LINE, MSG_NOTHING).await;
        return;
    }
    ux.name(who).await;
    ux.put(b'\n').await;
    let a = (who as usize) & 0xff;
    if ux.s().actors.attrs.status[a] & 0x80 == 0 {
        ux.say(FMT_LINE, MSG_NO_EFFECT).await;
        return;
    }
    let frame = ux.s().objects.tf(obj) >> 10;
    let resists = |ux: Ux<'_>, bit: u8| {
        let s = ux.s();
        let i = crate::objects::monster_index(&s.objects, s.objects.kind(who));
        defs::a4_byte(ux.io, defs::MONSTER_FLAGS, 2 * (i as usize)) & bit != 0
    };
    let mut no_effect = false;
    match frame {
        0 => {
            let mut s = ux.s();
            if s.actors.attrs.status[a] & 0x04 != 0 {
                s.actors.attrs.status[a] &= !0x04;
                s.objects.type_frame[a] = s.actors.attrs.base_type_frame[a];
            } else {
                no_effect = true;
            }
        }
        1 => {
            let mut s = ux.s();
            if s.actors.attrs.status[a] & 0x08 != 0 {
                s.actors.attrs.status[a] &= !0x08;
            } else {
                no_effect = true;
            }
        }
        2 => seams::heal(&mut ux.s(), 6),
        3 if !resists(ux, 0x04) => {
            let mut s = ux.s();
            s.actors.attrs.status[a] |= 0x08;
            seams::hit_flash(&mut s, who);
        }
        4 if !resists(ux, 0x80) => {
            let mut s = ux.s();
            seams::hit_flash(&mut s, who);
            s.actors.attrs.status[a] |= 0x04;
            seams::status_frame(&mut s, who);
        }
        5 => {
            let mut s = ux.s();
            if s.actors.attrs.status[a] & 0x01 == 0 {
                s.actors.attrs.status[a] |= 0x01;
                seams::flash_actor(&mut s, who);
            } else {
                no_effect = true;
            }
        }
        6 => {
            let mut s = ux.s();
            if s.objects.obj_status[a] & 0x02 == 0 {
                s.objects.obj_status[a] |= 0x02;
                seams::flash_actor(&mut s, who);
            } else {
                no_effect = true;
            }
        }
        7 => seams::xray(ux.state, ux.io).await,
        3 | 4 => no_effect = true,
        _ => {}
    }
    if no_effect {
        ux.say(FMT_NL_LINE, MSG_NO_EFFECT).await;
    }
    let mut s = ux.s();
    crate::objects::remove(&mut s, obj);
    if matches!(s.ui.panel.mode, 0x90 | 0x91) {
        s.ui.panel.dirty = true;
    }
}

/// Original: 0x12e912 (objects: study a scroll: hidden `&` text shows from now on, A4-0x7b26)
pub async fn study_scroll(ux: Ux<'_>) {
    ux.print(STR_STUDY, &[]).await;
    if let Some(w) = ux.s().actors.objlist_tail.get_mut(0x5e..0x60) {
        w.copy_from_slice(&[0, 1]);
    }
}

/// Original: 0x13051a (objects: the mouse squeaks, the line 0x12cf80)
pub async fn squeak(ux: Ux<'_>) {
    ux.print(STR_SQUEAK, &[]).await;
}

/// Original: 0x130526 (objects: a fan turns the wind one step clockwise, the line 0x12cf8a,
/// then the clock runs 0 minutes for the wind line)
pub async fn fan(ux: Ux<'_>) {
    {
        let mut s = ux.s();
        s.actors.wind = s.actors.wind.wrapping_add(1) & 7;
    }
    ux.print(STR_BREEZE, &[]).await;
    crate::actors::clock::tick(&mut ux.s(), 0);
    ux.flush().await;
}

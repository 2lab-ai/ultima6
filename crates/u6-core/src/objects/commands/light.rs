//! Light sources of hunk 15: torches (0x12c326), the gem (0x12c54a: the map overview, then one
//! of the stack used up), lighting or dousing a fixture (0x12c594). Synchronous but for the
//! gem, which waits on the overview's key; their text is queued.

use std::cell::RefCell;

use super::seams;
use super::text::{queue, queue_at};
use crate::engine::Io;
use crate::objects::moves::{create_from_record, create_in, remove};
use crate::objects::{IN_CONTAINER, IN_INVENTORY, READIED, Slot, WHERE_MASK, pack_xyz};
use crate::state::State;

const S_NO_FREE_HAND: u32 = 0x0012_8874;
const S_TORCH_LIT: u32 = 0x0012_8896;
const S_FIREPLACE: u32 = 0x0012_88a6;
const S_BRAZIER: u32 = 0x0012_88b0;
const S_CANDELABRA: u32 = 0x0012_88b8;
const S_CAMPFIRE: u32 = 0x0012_88c4;
const S_CANDLE: u32 = 0x0012_88ce;
const S_DOUSED: u32 = 0x0012_88d6;
const S_LIT: u32 = 0x0012_88e4;
/// Original: A4-0x756f (the burned-out torch line, data hunk)
const MSG_BURNED_OUT: i32 = -0x756f;

/// Port-only (spec 08, slice K2; spec 16 item 1, 2026-10-27): the strings this module passes to
/// `printf` as its format, for the locale layer's catalogue ([`crate::locale::strings`]): the
/// torch's lines (0x12c326 pushes them at 0x12c358, 0x12c370 and 0x12c52a) and a fixture's line,
/// three formats in a row: the newline, the fixture's name (0x12c594: 0x12c5e2..0x12c616) and the
/// ending (0x12c648 doused, 0x12c67a lit).
pub(super) const PRINTED: &[u32] = &[
    super::GameData::a4(MSG_BURNED_OUT),
    S_NO_FREE_HAND,
    S_TORCH_LIT,
    S_FIREPLACE,
    S_BRAZIER,
    S_CANDELABRA,
    S_CAMPFIRE,
    S_CANDLE,
    S_DOUSED,
    S_LIT,
];

const TORCH: u16 = 0x5a;
/// The two hand slots of the equipment table (A4+0x1364, A4+0x136a).
const HANDS: [i16; 2] = [2, 5];

fn panel_actor(s: &State) -> Slot {
    Slot::from(crate::ui::panels::member_actor(s, s.ui.panel.member.max(0)))
}

/// Original: 0x12c326 (objects: use torch `t`: light it in a free hand, or put it out)
///
/// A lit torch (status 0x80) goes out of the world with the burned-out line (A4-0x756f). Both
/// hands full: the no-free-hand line (0x128874). From a stack one torch is split off (on the
/// map a new map object with the stack's frame, carried a new readied torch of the member the
/// panel shows) and the stack loses one. A carried torch is readied in the first free hand
/// (the panel's sums redone, 0x11121e); then it is lit: status 0x80, frame 1, 200 turns of
/// fuel, the light recomputed by a clock step (0x10368a(0)), the lit line (0x128896). The
/// inventory panel is redrawn when open (0x1118c6).
pub fn light_torch(s: &mut State, t: Slot) {
    let q = s.objects.qq_of(t) & 0xff;
    if s.objects.status(t) & 0x80 != 0 {
        remove(s, t);
        queue(s, b"\n".to_vec(), Vec::new());
        let fmt = super::text::a4_string(s, crate::string_registry::text!(MSG_BURNED_OUT));
        queue(s, fmt, Vec::new());
        return;
    }
    if s.inventory.equip(HANDS[0]) != 0 && s.inventory.equip(HANDS[1]) != 0 {
        queue_at(s, crate::string_registry::text!(S_NO_FREE_HAND), Vec::new());
    } else {
        let mut torch = t;
        if q > 1 {
            if s.objects.place(t) == 0 {
                let z = u16::from(s.map.centre.z);
                let [b0, b1, b2] = pack_xyz(s.objects.x(t), s.objects.y(t), z).to_packed();
                let tf = (s.objects.tf(t) >> 10) << 10 | TORCH;
                let [t0, t1] = tf.to_be_bytes();
                let qq = (s.objects.qq_of(t) & 0xff00) | 1;
                let [q0, q1] = qq.to_be_bytes();
                torch = create_from_record(s, [0, b0, b1, b2, t0, t1, q0, q1]);
            } else {
                let owner = panel_actor(s);
                torch = create_in(s, TORCH, READIED, 1, owner);
            }
            s.objects.qq[t as usize] = (s.objects.qq_of(t) & 0xff00) | (q - 1);
        }
        let place = s.objects.place(torch);
        if place == IN_INVENTORY || place == IN_CONTAINER {
            let hand = if s.inventory.equip(HANDS[0]) == 0 {
                HANDS[0]
            } else {
                HANDS[1]
            };
            s.inventory.set_equip(hand, torch);
            let b = &mut s.objects.obj_status[torch as usize];
            *b = (*b & !WHERE_MASK) | READIED;
            let owner = panel_actor(s);
            seams::sum_inventory(s, owner);
        }
        if let Some(i) = usize::try_from(torch)
            .ok()
            .filter(|&i| i < s.objects.qq.len())
        {
            s.objects.obj_status[i] |= 0x80;
            s.objects.type_frame[i] = (s.objects.type_frame[i] & 0x3ff) + 0x400;
            s.objects.qq[i] = (s.objects.qq[i] & 0xff00) | 200;
        }
        crate::actors::clock::tick(s, 0);
        queue_at(s, crate::string_registry::text!(S_TORCH_LIT), Vec::new());
    }
    if s.ui.panel.mode == 0x92 {
        seams::redraw_inventory(s);
    }
}

/// Original: 0x12c54a (objects: peer through gem `obj`: the map overview, then one of the
/// stack used up)
///
/// The overview (ui 0x13410a, [`crate::combat::peer::overview`]: the map window drawn small
/// until a key), then [`use_up_one`]. (The integration kept usecmd's reading of 0x13410a: the
/// dump of 0x12c54a calls it first, and the port of 0x13410a draws and waits for a key.)
pub async fn use_up(state: &RefCell<State>, io: &Io, obj: Slot) {
    crate::combat::peer::overview(state, io).await;
    use_up_one(&mut state.borrow_mut(), obj);
}

/// The part of 0x12c54a after the overview: a quantity above 1 drops by one, else the object
/// goes (`obj` 0 is skipped). The inventory panel is redrawn when open (0x1118c6).
pub fn use_up_one(s: &mut State, obj: Slot) {
    if obj != 0 {
        let q = s.objects.qq_of(obj) & 0xff;
        if q > 1 {
            s.objects.qq[obj as usize] = (s.objects.qq_of(obj) & 0xff00) | (q - 1);
        } else {
            remove(s, obj);
        }
    }
    if s.ui.panel.mode == 0x92 {
        seams::redraw_inventory(s);
    }
}

/// Original: 0x12c594 (objects: light or douse fixture `obj`: candle 0x7a, candelabra 0x91,
/// fireplace 0xa4, brazier 0xce, campfire 0xfd)
///
/// A newline and the fixture's name, then: lit (frame bit 0) it keeps frame bit 1 only, the
/// doused ending (0x1288d6); else frame bit 0 is set, the lit ending (0x1288e4). Another type
/// prints no name.
pub fn douse_or_light(s: &mut State, obj: Slot) {
    let kind = s.objects.kind(obj);
    let frame = s.objects.tf(obj) >> 10;
    queue(s, b"\n".to_vec(), Vec::new());
    let name = match kind {
        0x7a => Some(S_CANDLE),
        0x91 => Some(S_CANDELABRA),
        0xa4 => Some(S_FIREPLACE),
        0xce => Some(S_BRAZIER),
        0xfd => Some(S_CAMPFIRE),
        _ => None,
    };
    if let Some(n) = name {
        queue_at(
            s,
            crate::string_registry::choose(
                (n) as i64,
                &[
                    crate::string_registry::text!(S_CANDLE),
                    crate::string_registry::text!(S_CANDELABRA),
                    crate::string_registry::text!(S_FIREPLACE),
                    crate::string_registry::text!(S_BRAZIER),
                    crate::string_registry::text!(S_CAMPFIRE),
                ],
            ),
            Vec::new(),
        );
    }
    let t = &mut s.objects.type_frame[obj as usize];
    if frame & 1 != 0 {
        *t = (*t & 0x3ff) + ((frame & 2) << 10);
        queue_at(s, crate::string_registry::text!(S_DOUSED), Vec::new());
    } else {
        *t = (*t & 0x3ff) + ((frame | 1) << 10);
        queue_at(s, crate::string_registry::text!(S_LIT), Vec::new());
    }
}

//! The helpers of the main hunk (hunk 1) the command loop and the other modules call.

use crate::engine::{Io, with_current_io};
use crate::objects::ObjectId;
use crate::state::State;

/// Objlist block bytes (A4-0x7b84 + n) these helpers read.
const TAIL_SOLO: usize = 0x79;
const TAIL_SOLO_WORK: usize = 0x7a;
const TAIL_PHASE_1: usize = 0x7d;
const TAIL_PHASE_2: usize = 0x7f;

fn tail(s: &State, i: usize) -> u8 {
    s.actors.objlist_tail.get(i).copied().unwrap_or(0)
}

fn set_tail(s: &mut State, i: usize, v: u8) {
    if let Some(b) = s.actors.objlist_tail.get_mut(i) {
        *b = v;
    }
}

/// Original: 0x101ff6 (main: set the animation flag of the animdata slot that animates `tile`)
///
/// The first of the A4+0x5dca slots whose tile is `tile` gets `value` (A4-0x309b; 0 off,
/// 1 forward, 2 backward); no slot, no write.
pub fn set_tile_animation(s: &mut State, tile: u16, value: u8) {
    let anim = &s.tables.anim;
    let n = usize::from(anim.count).min(anim.slots.len());
    if let Some(i) = anim.slots[..n]
        .iter()
        .position(|a| a.tile_to_animate == tile)
        && let Some(f) = s.map.anim_enabled.get_mut(i)
    {
        *f = value;
    }
}

/// Original: 0x102f2a (main: when the view changed (A4-0x6a8e), recompute the view flags, clear
/// the request and refresh the window 0x102ef0)
pub fn after_turn_refresh(s: &mut State) {
    if s.map.view_dirty == 0 {
        return;
    }
    crate::map::view::update_view(s);
    s.map.view_dirty = 0;
    with_current_io(|io| crate::gfx::refresh_view(s, io));
}

/// Original: 0x1032b6 (main: hazard of the tile under an actor in the window)
///
/// Skipped for the types 0x1a7, 0x19e, 0x19f, 0x19c, for a creature whose flag word (A4-0x7dbe)
/// has bit 8, an actor with movement flag 0x08, one on another level than the leader, or one
/// outside the 40 × 40 window ((x − (leader x − 16) & 0x3f8) & mask < 0x28). A map tile with
/// flags1 bit 3 under it, else the first other object there whose tile has it (that object
/// loses its invisible bit), goes to 0x102f3c.
pub fn actor_status_tick(s: &mut State, actor: u16) {
    let id = ObjectId(actor);
    let ty = s.objects.obj_type(id);
    let index = crate::objects::monster_index(&s.objects, ty);
    if [0x1a7, 0x19e, 0x19f, 0x19c].contains(&ty) {
        return;
    }
    if index >= 0 && s.actors.game.creature_flags(index) & 0x100 != 0 {
        return;
    }
    let a = usize::from(actor);
    if s.actors.attrs.movement_flags.get(a).copied().unwrap_or(0) & 0x08 != 0 {
        return;
    }
    let p = s.objects.location.get(a).copied().unwrap_or_default();
    let c = s.map.centre;
    if p.z != c.z {
        return;
    }
    let mask = if c.z != 0 { 0xff } else { 0x3ff };
    let ox = c.x.wrapping_sub(16) & 0x3f8;
    let oy = c.y.wrapping_sub(16) & 0x3f8;
    if p.x.wrapping_sub(ox) & mask >= 0x28 || p.y.wrapping_sub(oy) & mask >= 0x28 {
        return;
    }
    let flags1 = |s: &State, t: u16| {
        let f = &s.tables.tileflags.flags1;
        f.get(usize::from(t)).copied().unwrap_or(0)
    };
    let (x, y, z) = (usize::from(p.x), usize::from(p.y), usize::from(c.z));
    let tile = u16::from(s.map.tile_at(x, y, z).unwrap_or(0));
    if flags1(s, tile) & 0x08 != 0 {
        crate::actors::party::tile_effect(s, actor, tile);
        return;
    }
    let mut obj = crate::objects::begin_at(&mut s.objects, &s.tables, p.x, p.y, u16::from(c.z));
    while obj >= 0 {
        if obj != actor as i16 {
            let o = ObjectId(obj as u16);
            let base = s
                .tables
                .basetile
                .base
                .get(usize::from(s.objects.obj_type(o)));
            let t = base.copied().unwrap_or(0) + u16::from(s.objects.frame(o));
            if flags1(s, t) & 0x08 != 0 {
                if let Some(f) = s.objects.actor_flags.get_mut(o.index()) {
                    *f &= !0x02;
                }
                crate::actors::party::tile_effect(s, actor, t);
                return;
            }
        }
        obj = crate::objects::next_at(&mut s.objects, &s.tables);
    }
}

/// Original: 0x10348c (main: end of turn: the volcano or fumarole 0x10e81e found in view)
///
/// When A4-0x7af6 holds a slot (above 1): a type 0x133 shakes the view (0x133a1a(10, 2)); a type
/// 0xab shakes it on a random 1 of 0..=1, else a new object 0xab appears on its tile and
/// explodes (0x10bcee, 0x12db7e). The word is cleared.
pub fn end_of_turn_volcano(s: &mut State) {
    let probe = s.objects.draw.volcano_probe;
    if probe <= 1 {
        return;
    }
    let id = ObjectId(probe as u16);
    match s.objects.obj_type(id) {
        0x133 => quake(s, 10, 2),
        0xab => {
            if s.rng.range(0, 1) != 0 {
                quake(s, 10, 2);
            } else {
                let p = s.objects.location(id).unwrap_or_default();
                let slot =
                    crate::objects::create_on_map(&mut s.objects, 0xab, 0, 0, p.x, p.y, p.z.into());
                crate::objects::explode_keg(s, slot);
            }
        }
        _ => {}
    }
    s.objects.draw.volcano_probe = 0;
}

/// 0x133a1a from synchronous code: on the screen of the polling engine.
fn quake(s: &mut State, times: i16, amp: i16) {
    with_current_io(|io| crate::ui::effects::quake(s, io, times, amp));
}

/// Original: 0x10355c (main: show or hide the moongates at the eight moonstones in the window)
///
/// For each moonstone (A4-0x7b5a + 6 n: x, y, z words) on the leader's level inside the 40 × 40
/// window: while either moon phase byte (A4-0x7b07, A4-0x7b05) is below 15 a missing red
/// moongate (type 0x55, frame 1: word 0x455) is created there, otherwise an existing one is
/// removed. Any change recomputes the view flags (0x109e74) and sets A4-0x6a8e.
pub fn moongates(s: &mut State) {
    let mut changed = false;
    for n in 0..8u8 {
        let [x, y, z] = crate::map::seams::moonstone(s, n);
        if z != u16::from(s.map.centre.z) {
            continue;
        }
        let [ox, oy] = s.map.window_origin;
        if x < ox || x >= ox + 0x28 || y < oy || y >= oy + 0x28 {
            continue;
        }
        let gate = crate::objects::object_of_type_at(s, 0x55, x, y);
        let up = tail(s, TAIL_PHASE_1) < 15 || tail(s, TAIL_PHASE_2) < 15;
        if up {
            if gate < 0 {
                crate::objects::create_on_map(&mut s.objects, 0x455, 0, 0, x, y, z);
                changed = true;
            }
        } else if gate >= 0 {
            crate::objects::remove(s, gate);
            changed = true;
        }
    }
    if changed {
        crate::map::view::update_view(s);
        s.map.view_dirty = 1;
    }
}

/// Original: 0x103632 (main: random wind change, `calm` in D0)
///
/// Underground (levels 1..4) a wind blowing (A4-0x7b7b ≥ 0) stops (-1), counting as a change.
/// Without a change and with `calm` 0: one draw of 0..=0x3f; a 0 on the surface or level 5
/// draws the new direction, 0..=8 minus 1 (-1 = calm).
pub fn wind_change(s: &mut State, calm: u16) {
    let mut changed = calm;
    let z = s.map.centre.z;
    if z > 0 && z < 5 && (s.actors.wind as i8) >= 0 {
        s.actors.wind = 0xff;
        changed = 1;
    }
    if changed == 0 && s.rng.range(0, 0x3f) == 0 && (z == 0 || z == 5) {
        s.actors.wind = (s.rng.range(0, 8) as u8).wrapping_sub(1);
    }
}

/// Original: 0x103fd4 (main: make party member `member` the leader)
///
/// A new leader index moves the view centre (A4-0x7b78..-0x7b74) to that member, rebuilds the
/// region around it (0x108f90), and sets A4-0x7af8 (the window is shown) and A4-0x7afd (the
/// loop reprints the prompt).
pub fn switch_leader(s: &mut State, member: u8) {
    if s.actors.party.leader_index == member {
        return;
    }
    s.actors.party.leader_index = member;
    let actor = s.actors.party.members.get(usize::from(member)).copied();
    if let Some(p) = actor.and_then(|a| s.objects.location(ObjectId(u16::from(a)))) {
        s.map.centre = p;
    }
    crate::map::region::refresh_region(s);
    s.gfx.window_shown = 1;
    s.actors.work.prompt = 1;
}

/// Original: 0x104094 (main: back to party mode)
///
/// The solo member's worktype (the leader's while A4-0x7b0b is negative) gets the saved one
/// back (A4-0x7b0a); the first member's worktype is saved there and becomes 2 (the player
/// leads); every other member not flagged 0x04 (A4+0x190c) follows (1). Then member 0 leads
/// (0x103fd4), party mode (A4-0x7b0b = -1), the panel cursor is restored (0x106932(0)) and
/// A4+0x1846 is cleared.
pub fn end_solo(s: &mut State, io: &Io) {
    let members = s.actors.party.members;
    let solo = tail(s, TAIL_SOLO) as i8;
    let saved = tail(s, TAIL_SOLO_WORK);
    let who = if solo < 0 {
        members[0]
    } else {
        members[(solo as usize).min(15)]
    };
    s.actors.worktype[usize::from(who)] = saved;
    let first = usize::from(members[0]);
    let w = s.actors.worktype[first];
    set_tail(s, TAIL_SOLO_WORK, w);
    s.actors.worktype[first] = 2;
    for &m in members
        .iter()
        .take(usize::from(s.actors.party.size))
        .skip(1)
    {
        if s.objects.actor_flags[usize::from(m)] & 0x04 == 0 {
            s.actors.worktype[usize::from(m)] = 1;
        }
    }
    switch_leader(s, 0);
    set_tail(s, TAIL_SOLO, 0xff);
    crate::ui::cursor::panel_cursor_in(s, io, 0);
    s.actors.work.leader_kept = 0;
}

/// Original: 0x135120 (boot: step the animation overlay frame, unless A4-0x7b64 is set)
///
/// The counter A4-0x618e goes up by one and its old value becomes the overlay frame
/// (0x152dca keeps its low three bits).
pub fn anim_step(s: &mut State) {
    if s.gfx.anim_paused != 0 {
        return;
    }
    let v = s.main.overlay_counter;
    s.main.overlay_counter = v.wrapping_add(1);
    crate::gfx::anim::set_frame(&mut s.gfx, v);
}

/// Original: 0x135118 (boot: empty; reached by the debug code 0x119 of the command loop)
pub fn debug_hook() {}

/// Original: 0x13511a (boot: calls the empty 0x152dc8 through its jump stub)
pub fn boot_hook() {
    crate::gfx::blit::empty_hook();
}

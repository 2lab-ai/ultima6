//! Party moves between places: teleport 0x1092fc, party placement 0x10921a, leaving a region
//! 0x108fba, ladders 0x109374, moongates 0x10952e, resurrection 0x1095bc, and the post-step check
//! 0x11d1d4 that the player step (0x11d4ee, `actors/`) calls after every step.
//!
//! The original keeps the view centre (A4-0x7b78..) and the members' object locations apart:
//! these routines set the centre first and then move every member there with 0x10c9d2
//! ([`seams::move_object`]), parking them at z = 15 while the party is between places.

use u6_formats::position::Position;

use super::{region, seams, view};
use crate::objects::ObjectId;
use crate::state::State;

/// Object types the post-step check reacts to (immediates in 0x11d1d4 / 0x1092fc).
const OBJ_RED_GATE: u16 = 0x55;
const OBJ_BLUE_GATE: u16 = 0x54;
const OBJ_LADDER: u16 = 0x134;
const OBJ_HOLE: u16 = 0x146;
/// Type + frame word of a ladder leading up (0x109374 compares the whole word).
const LADDER_UP: u16 = 0x531;
/// A party member of this type triggers 0x12f8fe before leaving (0x108fba).
const OBJ_LEAVES_FIRST: u16 = 0x1af;
/// Body objects 0x1095bc looks for, the type it clears from slots 0xe0..0x100.
const OBJ_BODY_A: u16 = 0x153;
const OBJ_BODY_B: u16 = 0x162;
const OBJ_CLEARED_ON_WAKE: u16 = 0x17e;
/// The level members are parked on between places.
const PARKED: u16 = 15;
/// Shamino (actor 3) names a dungeon entrance within this distance (0x11d1d4).
const SHAMINO: ObjectId = ObjectId(3);
/// Party mode: the solo-member byte holds 0xff.
const PARTY_MODE: u8 = 0xff;

/// Party roster byte `i` as the original reads it: A4+0x47ec + i. Past the 16 members comes one
/// unmodelled byte (A4+0x47fc, read as 0), then the A4+0x47fd array (`unknown_1af1`); 0x1095bc
/// sets the leader index to 0xff, so the next 0x10921a follows actor `unknown_1af1[0xee]`.
fn roster_byte(state: &State, i: u8) -> u8 {
    let p = &state.actors.party;
    let i = usize::from(i);
    match i.checked_sub(p.members.len()) {
        None => p.members[i],
        Some(0) => 0,
        Some(k) => state
            .actors
            .attrs
            .unknown_1af1
            .get(k - 1)
            .copied()
            .unwrap_or(0),
    }
}

fn members(state: &State) -> Vec<ObjectId> {
    (0..state.actors.party.size)
        .map(|i| ObjectId(roster_byte(state, i).into()))
        .collect()
}

fn refill_moves(state: &mut State, id: ObjectId) {
    let a = &mut state.actors;
    if let (Some(&dex), Some(mp)) = (
        a.attrs.dexterity.get(id.index()),
        a.movement_points.get_mut(id.index()),
    ) {
        *mp = dex;
    }
}

fn centre(state: &State) -> (u16, u16, u16) {
    let c = state.map.centre;
    (c.x, c.y, c.z.into())
}

fn set_centre(state: &mut State, x: u16, y: u16, z: u16) {
    state.map.centre = Position { x, y, z: z as u8 };
}

/// Draw the step: object lists then the refresh (the 0x10e81e + 0x102ef0 pair).
fn draw(state: &mut State) {
    seams::build_view_objects(state);
    seams::refresh_view(state);
}

/// Original: 0x1092fc (map: teleport the party to x, y, z)
///
/// Leave the region (0x108fba with `mode`), remove a blue moongate (type 0x54) standing at the
/// old centre, set the centre, refresh the region, offer the window's eggs with the "arriving"
/// flag set, update the view, the UI step 0x133b28, a clock tick (argument 0), then place the
/// party (0x10921a with `mode`). `mode`: 0 = no drawing, 1 = draw each step and redraw all,
/// 2 = draw each step.
pub fn teleport(state: &mut State, x: u16, y: u16, z: u16, mode: u16) {
    leave_region(state, mode);
    let (cx, cy, _) = centre(state);
    if let Some(gate) = seams::find_object_at(state, OBJ_BLUE_GATE, cx, cy) {
        seams::remove_object(state, gate);
    }
    set_centre(state, x, y, z);
    region::refresh_region(state);
    state.map.arriving = 1;
    region::check_eggs(state);
    state.map.arriving = 0;
    view::update_view(state);
    seams::ui_after_teleport(state);
    seams::clock_tick(state, 0);
    place_party(state, x, y, z, mode);
}

/// Original: 0x10921a (map: put the party at x, y, z and let it fan out)
///
/// Every member is moved to (x, y, z) with A4-0x784e cleared around the loop, then 0x104094;
/// with mode 1 everything is redrawn; then 5 rounds of the leader's followers stepping
/// (0x11c910 with the leader, drawn when mode ≠ 0); finally every member's movement points are
/// refilled (A4+0x430c = A4+0x400c).
pub fn place_party(state: &mut State, x: u16, y: u16, z: u16, mode: u16) {
    state.map.flag_784e = 0;
    for m in members(state) {
        seams::move_object(state, m, x, y, z);
    }
    state.map.flag_784e = 1;
    seams::main_after_placing(state);
    if mode == 1 {
        state.gfx.dissolve = 1;
        draw(state);
    }
    for _ in 0..5 {
        let leader = ObjectId(roster_byte(state, state.actors.party.leader_index).into());
        seams::party_follow(state, leader);
        if mode != 0 {
            draw(state);
        }
    }
    for m in members(state) {
        refill_moves(state, m);
    }
}

/// Original: 0x108fba (map: the party leaves the current region)
///
/// Members of type 0x1af first run 0x12f8fe; the region is refreshed; with `mode` ≠ 0 the
/// members on the centre's level gather on the centre for up to 30 rounds (each round: restore
/// a shape-changed member's type, clear status bits 0x06, then a member on the centre is parked
/// at z = 15, any other one steps toward it (0x11f452), joins it when that fails within distance
/// 2, or steps in a random direction 0, 2, 4 or 6 (RNG); movement points refilled; the round
/// ends early once nobody stepped); finally every member not yet parked is parked at the centre.
pub fn leave_region(state: &mut State, mode: u16) {
    for m in members(state) {
        if state.objects.obj_type(m) == OBJ_LEAVES_FIRST {
            seams::before_leaving(state, m);
        }
    }
    region::refresh_region(state);
    if mode != 0 {
        for _ in 0..30 {
            let mut settled = true;
            for m in members(state) {
                settled &= gather_step(state, m, mode);
                refill_moves(state, m);
            }
            draw(state);
            if settled {
                break;
            }
        }
    }
    let (cx, cy, _) = centre(state);
    state.map.flag_784e = 0;
    for m in members(state) {
        if state
            .objects
            .location(m)
            .is_some_and(|p| u16::from(p.z) != PARKED)
        {
            seams::move_object(state, m, cx, cy, PARKED);
        }
    }
    state.map.flag_784e = 1;
}

/// One member's turn of 0x108fba's gathering round; false when it stepped toward the centre.
fn gather_step(state: &mut State, m: ObjectId, mode: u16) -> bool {
    let (cx, cy, cz) = centre(state);
    let i = m.index();
    let attrs = &mut state.actors.attrs;
    if attrs.status[i] & 0x04 != 0 {
        state.objects.type_frame[i] = attrs.base_type_frame[i];
    }
    attrs.status[i] &= 0xf9;
    let p = state.objects.location[i];
    let free = attrs.status[i] & 0x16 == 0 && attrs.movement_flags[i] & 0x10 == 0;
    if u16::from(p.z) != cz || !free {
        return true;
    }
    if (p.x, p.y) == (cx, cy) {
        draw(state);
        state.map.flag_784e = 0;
        seams::move_object(state, m, cx, cy, PARKED);
        state.map.flag_784e = 1;
        if mode == 1 {
            state.gfx.dissolve = 1;
            draw(state);
        }
        return true;
    }
    if seams::step_toward(state, m, cx, cy) != 0 {
        return false;
    }
    if seams::distance(state, m, cx, cy) < 2 {
        seams::move_object(state, m, cx, cy, cz);
    } else {
        let dir = state.rng.range(0, 3) * 2;
        seams::walk(state, m, dir);
    }
    true
}

/// Original: 0x109374 (map: climb the ladder or hole `ladder` to the next level)
///
/// Down from the surface and from levels 1..4 unless the object is an up-ladder (type + frame
/// word 0x531); up from level 5. The centre takes the object's x, y, the party leaves (mode 2)
/// and the level changes. Surface → level 1 keeps the tile inside its chunk and divides the
/// chunk coordinates by 4; level 1 → surface multiplies them by 4 and adds 8 / 16 to x / y for
/// quality bits 0 / 1 / 2 / 3. Then region, view, the UI step and a clock tick (only for the
/// surface, level 5, the first step down or the last step up), the party is placed (mode 2) and
/// the area song starts.
pub fn climb(state: &mut State, ladder: ObjectId) {
    if seams::tail_byte(state, 0x78) != 0 {
        seams::break_off_combat(state);
    }
    let (_, _, z) = centre(state);
    let dir: i16 = match z {
        0 => 1,
        5 => -1,
        _ if state.objects.type_frame[ladder.index()] == LADDER_UP => -1,
        _ => 1,
    };
    let p = state.objects.location[ladder.index()];
    set_centre(state, p.x, p.y, z);
    leave_region(state, 2);
    let z = (z as i16 + dir) as u16;
    let (mut x, mut y) = (p.x, p.y);
    if z == 1 && dir == 1 {
        x = (x & 7) + ((x >> 2) & 0xf8);
        y = (y & 7) + ((y >> 2) & 0xf8);
    } else if z == 0 && dir == -1 {
        x = (x & 7) + ((x << 2) & 0x3e0);
        y = (y & 7) + ((y << 2) & 0x3e0);
        let q = seams::object_quality(state, ladder);
        x += if q & 1 != 0 { 8 } else { 0 } + if q & 2 != 0 { 16 } else { 0 };
        y += if q & 4 != 0 { 8 } else { 0 } + if q & 8 != 0 { 16 } else { 0 };
    }
    set_centre(state, x, y, z);
    region::refresh_region(state);
    view::update_view(state);
    if z == 0 || z == 5 || (z == 1 && dir == 1) || (z == 4 && dir == -1) {
        seams::ui_after_teleport(state);
        seams::clock_tick(state, 0);
    }
    place_party(state, x, y, z, 2);
    seams::play_area_song(state);
}

/// Original: 0x10952e (map: go through a moongate to moonstone `stone`)
///
/// Out of combat first; a stone at (0, 0, 0) (not placed) leaves the party where it is, else
/// it teleports there (mode 1); then the area song.
pub fn moongate(state: &mut State, stone: u8) {
    if seams::tail_byte(state, 0x78) != 0 {
        seams::break_off_combat(state);
    }
    let [x, y, z] = match seams::moonstone(state, stone) {
        [0, 0, 0] => {
            let (x, y, z) = centre(state);
            [x, y, z]
        }
        at => at,
    };
    teleport(state, x, y, z, 1);
    seams::play_area_song(state);
}

/// Original: 0x11d1d4 (map: after a step, use the moongate, ladder or hole under the leader)
///
/// Nothing in combat or when the leader index equals the party size. Otherwise the first object
/// at the centre (not the leader) that is a red moongate (0x55), a blue moongate (0x54) or a
/// ladder / hole (0x134 / 0x146) decides; a moongate counts only with stacking word 0. Outside
/// party mode the party refuses: "You must be in party mode to enter." A red moongate at
/// 00:00..00:09 leads to (0x18, 0x1d, 1) with song 8 and a clock tick (0x0a); else to the stone
/// of the moon whose phase is nearer 7 (a tie goes to moon 2 exactly when "minute < 30" and
/// "phase 1 < phase 2" differ). A blue moongate of quality q leads to the
/// destination tables' entry q - 1 (then the area song), quality 0 nowhere. A ladder or hole
/// climbs (0x109374), Shamino first naming a dungeon entrance (quality 1..=20) when he is in
/// the party, awake and within 6 tiles.
pub fn after_step(state: &mut State) {
    if seams::tail_byte(state, 0x78) != 0 {
        return;
    }
    let party = &state.actors.party;
    if party.leader_index == party.size {
        return;
    }
    let leader = ObjectId(roster_byte(state, state.actors.party.leader_index).into());
    let (cx, cy, cz) = centre(state);
    let party_mode = seams::tail_byte(state, 0x79) == PARTY_MODE;
    let mut refused = false;
    for (id, stack) in seams::objects_at(state, cx, cy, cz) {
        if id == leader {
            continue;
        }
        match state.objects.obj_type(id) {
            OBJ_RED_GATE if stack == 0 => {
                if party_mode {
                    red_moongate(state);
                } else {
                    refused = true;
                }
            }
            OBJ_BLUE_GATE if stack == 0 => {
                if party_mode {
                    blue_moongate(state, id);
                } else {
                    refused = true;
                }
            }
            OBJ_HOLE | OBJ_LADDER => {
                let shamino = state.actors.attrs.status[SHAMINO.index()];
                let q = seams::object_quality(state, id);
                if shamino & 0x80 != 0
                    && shamino & 0x10 == 0
                    && seams::distance(state, SHAMINO, cx, cy) < 6
                    && (1..0x15).contains(&q)
                {
                    seams::say_place_name(state, q);
                    state.actors.work.prompt = 1;
                }
                if party_mode {
                    climb(state, id);
                } else {
                    refused = true;
                }
            }
            _ => continue,
        }
        break;
    }
    if refused {
        seams::say_party_mode_needed(state);
        state.actors.work.prompt = 1;
    }
}

/// 0x11d1d4's red-moongate branch (asm 0x11d260..0x11d33c).
fn red_moongate(state: &mut State) {
    let clock = state.actors.calendar;
    if clock.hour == 0 && clock.minute < 10 {
        teleport(state, 0x18, 0x1d, 1, 1);
        seams::play_song(state, 8);
        seams::clock_tick(state, 0x0a);
        return;
    }
    let (stone1, phase1) = (seams::tail_byte(state, 0x7c), seams::tail_byte(state, 0x7d));
    let (stone2, phase2) = (seams::tail_byte(state, 0x7e), seams::tail_byte(state, 0x7f));
    let off = |p: u8| (7 - i16::from(p)).abs();
    let d = off(phase1) - off(phase2);
    // A tie goes to moon 2 when "first half hour" and "phase 1 < phase 2" differ.
    let tie_to_2 = (clock.minute < 30) != (phase1 < phase2);
    let stone = if d < 0 || (d == 0 && !tie_to_2) {
        stone1
    } else {
        stone2
    };
    moongate(state, stone);
}

/// 0x11d1d4's blue-moongate branch (asm 0x11d364..0x11d3ca).
fn blue_moongate(state: &mut State, gate: ObjectId) {
    let q = seams::object_quality(state, gate);
    let dest = state.map.tables.as_ref().and_then(|t| {
        let i = usize::from(q.checked_sub(1)?);
        Some((*t.gate_x.get(i)?, *t.gate_y.get(i)?, *t.gate_z.get(i)?))
    });
    match (q, dest) {
        (0, _) | (_, None) => {
            let (x, y, z) = centre(state);
            teleport(state, x, y, z, 1);
        }
        (_, Some((x, y, z))) => {
            teleport(state, x, y, z.into(), 1);
            seams::play_area_song(state);
        }
    }
}

/// Original: 0x1095bc (map: the party wakes in Lord British's castle after a total defeat)
///
/// The leader index becomes 0xff. For actors 0..0xe0 with status bits 0x80 and 0x10, the
/// first body object (type 0x153 or 0x162) in the region list whose quality is the actor is
/// stored in A4-0x2f04 and raised: the Avatar through 0x10c810 with its type, magic points
/// (= intelligence) and status restored and, below karma 98, its experience scaled by karma
/// / 100; a companion through 0x11785e. Worktype 0x9b becomes 0x81. Every member then gets its
/// base type back, status bits reset, full hit points (0x122144) and movement points; type
/// 0x17e objects in slots 0xe0..0x100 go; the timers and the table A4+0x186e are cleared; out
/// of combat; teleport to (0x133, 0x160, 0) without drawing; the clock runs (0x0f per tick,
/// 0x120a1c each tick) until 8..12 or 14..18 o'clock; then everything is redrawn and the area
/// song starts.
///
/// Port guard: when a clock tick leaves the time unchanged (the clock not ported yet) the wait
/// stops instead of spinning forever.
pub fn resurrect_party(state: &mut State) {
    state.actors.party.leader_index = 0xff;
    for a in 0..0xe0u16 {
        let st = state.actors.attrs.status[usize::from(a)];
        if st & 0x80 != 0 && st & 0x10 != 0 {
            let body = seams::region_list(state).into_iter().find(|&o| {
                let t = state.objects.obj_type(o);
                (t == OBJ_BODY_A || t == OBJ_BODY_B)
                    && u16::from(seams::object_quality(state, o)) == a
            });
            if let Some(body) = body {
                state.ui.input.selection = body.0 as i16;
                raise(state, ObjectId(a), body);
            }
        }
        let w = &mut state.actors.worktype[usize::from(a)];
        if *w == 0x9b {
            *w = 0x81;
        }
    }
    for m in members(state) {
        let i = m.index();
        let attrs = &mut state.actors.attrs;
        if attrs.status[i] & 0x04 != 0 {
            attrs.status[i] &= !0x04;
            state.objects.type_frame[i] = attrs.base_type_frame[i];
        }
        attrs.status[i] = (attrs.status[i] & 0x9f) | 0x40;
        if let Some(f) = state.objects.actor_flags.get_mut(i) {
            *f &= 0xb9;
        }
        state.actors.attrs.status[i] &= 0xe1;
        state.actors.attrs.hp[i] = seams::max_hp(state, m);
        refill_moves(state, m);
    }
    for s in 0xe0..0x100u16 {
        if state.objects.obj_type(ObjectId(s)) == OBJ_CLEARED_ON_WAKE {
            seams::remove_object(state, ObjectId(s));
        }
    }
    state.actors.timers = [0; 16];
    state.map.table_186e = vec![-1; 64];
    seams::main_after_placing(state);
    seams::break_off_combat(state);
    teleport(state, 0x133, 0x160, 0, 0);
    state.actors.work.force_teleport = 1;
    loop {
        let before = state.actors.calendar;
        let h = before.hour;
        if !(h < 8 || (12..14).contains(&h) || h > 17) {
            break;
        }
        seams::clock_tick(state, 0x0f);
        seams::scan_hostiles(state);
        if state.actors.calendar == before {
            break;
        }
    }
    state.actors.work.force_teleport = 0;
    state.gfx.dissolve = 1;
    draw(state);
    seams::play_area_song(state);
    state.ui.panel.dirty = true;
    state.map.flag_784e = 1;
}

/// 0x1095bc's raise step for actor `a` lying as `body`.
fn raise(state: &mut State, a: ObjectId, body: ObjectId) {
    if a != ObjectId(1) {
        state.map.flag_784e = 0;
        seams::resurrect_body(state, body);
        return;
    }
    seams::raise_avatar_body(state, body);
    let attrs = &mut state.actors.attrs;
    attrs.status[1] &= 0xef;
    state.objects.type_frame[1] = attrs.base_type_frame[1];
    attrs.magic[1] = attrs.intelligence[1];
    attrs.status[1] &= 0xe0;
    let karma = state.actors.karma;
    if karma < 0x62 {
        // 0x101c48 (32-bit multiply) then 0x101c68 (signed 32-bit divide); both operands are
        // non-negative here.
        let exp = u32::from(attrs.exp[1]) * u32::from(karma) / 100;
        attrs.exp[1] = exp as u16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::tests::open_state;

    fn at(x: u16, y: u16, z: u8) -> Position {
        Position { x, y, z }
    }

    /// Open map, party of actors 1 and 2 at (300, 300, 0), party mode, out of combat.
    fn party() -> State {
        let mut s = open_state(at(300, 300, 0));
        s.actors.party.members[..2].copy_from_slice(&[1, 2]);
        s.actors.party.size = 2;
        s.actors.objlist_tail[0x79] = PARTY_MODE;
        for a in [1, 2] {
            s.objects.type_frame[a] = 0x150;
            s.objects.location[a] = at(300, 300, 0);
            s.actors.attrs.dexterity[a] = 20 + a as u8;
        }
        s
    }

    fn put(s: &mut State, slot: u16, type_frame: u16, p: Position) -> ObjectId {
        s.objects.type_frame[usize::from(slot)] = type_frame;
        s.objects.location[usize::from(slot)] = p;
        ObjectId(slot)
    }

    fn quality(s: &mut State, id: ObjectId, q: u8) {
        s.objects.qq[id.index()] = u16::from(q) << 8;
    }

    #[test]
    fn teleport_moves_the_centre_and_every_member_and_refills_moves() {
        let mut s = party();
        teleport(&mut s, 500, 600, 0, 0);
        assert_eq!(s.map.centre, at(500, 600, 0));
        assert_eq!(s.map.window_origin, [480, 584]);
        for a in [1, 2] {
            assert_eq!(s.objects.location[a], at(500, 600, 0));
            assert_eq!(s.actors.movement_points[a], 20 + a as u8);
        }
        assert_eq!((s.map.arriving, s.map.flag_784e), (0, 1));
    }

    #[test]
    fn leaving_gathers_members_on_the_centre_then_parks_them() {
        let mut s = party();
        s.objects.location[2] = at(303, 300, 0);
        // A member on another level and one held by status 0x02 / movement flag 0x10 stay.
        s.actors.party.members[2] = 3;
        s.actors.party.size = 3;
        s.objects.location[3] = at(10, 10, 2);
        s.actors.attrs.status[2] = 0x04;
        s.actors.attrs.base_type_frame[2] = 0x177;
        leave_region(&mut s, 2);
        // Seam fallbacks: no step, distance 0, so member 2 joins at once; all end parked.
        assert_eq!(s.objects.location[1], at(300, 300, 15));
        assert_eq!(s.objects.location[2], at(300, 300, 15));
        assert_eq!(s.objects.location[3], at(300, 300, 15));
        assert_eq!(
            s.objects.type_frame[2], 0x177,
            "shape restored from the base type"
        );
        assert_eq!(s.actors.attrs.status[2], 0);
    }

    #[test]
    fn ladders_scale_coordinates_between_surface_and_level_one() {
        let mut s = party();
        let down = put(&mut s, 300, 0x134, at(300, 300, 0));
        climb(&mut s, down);
        // x: (300 & 7) + ((300 >> 2) & 0xf8) = 4 + 72.
        assert_eq!(s.map.centre, at(76, 76, 1));
        assert_eq!(s.objects.location[1], at(76, 76, 1));
        // Up again from level 1 with quality bits 0 and 3: x + 8, y + 16. Qualities above
        // slot 255 are not held yet (seam fallback 0), so the ladder takes an actor slot.
        let up = put(&mut s, 200, LADDER_UP, at(76, 76, 1));
        quality(&mut s, up, 0b1001);
        climb(&mut s, up);
        // x: (76 & 7) + ((76 << 2) & 0x3e0) = 4 + 288, + 8; y: 4 + 288 + 16.
        assert_eq!(s.map.centre, at(300, 308, 0));
        // Level 2 ladders go down unless they are up-ladders; level 5 always goes up.
        s.map.centre = at(40, 40, 2);
        let l = put(&mut s, 201, 0x134, at(40, 40, 2));
        climb(&mut s, l);
        assert_eq!(s.map.centre.z, 3);
        s.map.centre = at(40, 40, 5);
        climb(&mut s, l);
        assert_eq!(s.map.centre.z, 4);
    }

    #[test]
    fn blue_moongates_use_the_destination_tables_and_red_ones_the_moons() {
        let mut s = party();
        let mut t = s.map.tables.clone().unwrap();
        t.gate_x[2] = 640;
        t.gate_y[2] = 480;
        t.gate_z[2] = 0;
        s.map.tables = Some(t);
        let gate = put(&mut s, 210, OBJ_BLUE_GATE, at(300, 300, 0));
        quality(&mut s, gate, 3);
        after_step(&mut s);
        assert_eq!(s.map.centre, at(640, 480, 0));
        assert_eq!(s.objects.location[2], at(640, 480, 0));
        // Red gate at 00:05: the fixed destination (0x18, 0x1d, 1).
        put(&mut s, 211, OBJ_RED_GATE, at(640, 480, 0));
        s.actors.calendar.hour = 0;
        s.actors.calendar.minute = 5;
        after_step(&mut s);
        assert_eq!(s.map.centre, at(0x18, 0x1d, 1));
        // Red gate by day: moon 1 phase 3 (|7 - 3| = 4) vs moon 2 phase 6 (1): moon 2's stone.
        put(&mut s, 212, OBJ_RED_GATE, at(0x18, 0x1d, 1));
        s.actors.calendar.hour = 9;
        let tail = &mut s.actors.objlist_tail;
        tail[0x7c..0x80].copy_from_slice(&[4, 3, 5, 6]);
        tail[0x2a + 5 * 6..0x2a + 6 * 6].copy_from_slice(&[0x01, 0x00, 0x00, 0xc8, 0x00, 0x00]);
        after_step(&mut s);
        assert_eq!(s.map.centre, at(256, 200, 0));
    }

    #[test]
    fn moon_ties_follow_the_half_hour_and_unset_stones_stay_put() {
        let mut s = party();
        let tail = &mut s.actors.objlist_tail;
        tail[0x7c..0x80].copy_from_slice(&[1, 5, 2, 9]); // |7-5| = |7-9|: a tie, phase 1 < 2
        tail[0x2a + 6..0x2a + 12].copy_from_slice(&[0, 10, 0, 20, 0, 0]);
        tail[0x2a + 12..0x2a + 18].copy_from_slice(&[0, 30, 0, 40, 0, 0]);
        s.actors.calendar.hour = 9;
        s.actors.calendar.minute = 10; // first half hour == phase 1 < 2: moon 1
        put(&mut s, 210, OBJ_RED_GATE, at(300, 300, 0));
        after_step(&mut s);
        assert_eq!(s.map.centre, at(10, 20, 0));
        s.actors.calendar.minute = 40;
        put(&mut s, 211, OBJ_RED_GATE, at(10, 20, 0));
        after_step(&mut s);
        assert_eq!(s.map.centre, at(30, 40, 0));
        // Stone 0 never placed: the party stays.
        moongate(&mut s, 0);
        assert_eq!(s.map.centre, at(30, 40, 0));
    }

    #[test]
    fn solo_mode_refuses_and_the_leaders_own_tile_object_is_skipped() {
        let id = "0x108658 print the party-mode message (string in game)";
        let mut s = party();
        s.actors.objlist_tail[0x79] = 0;
        put(&mut s, 210, OBJ_HOLE, at(300, 300, 0));
        let before = crate::stub::detached_hits().get(id).copied().unwrap_or(0);
        after_step(&mut s);
        assert_eq!(s.map.centre, at(300, 300, 0));
        assert_eq!(
            crate::stub::detached_hits().get(id).copied().unwrap_or(0),
            before + 1
        );
        assert_eq!(s.actors.work.prompt, 1);
        // In combat nothing happens; with leader index == size neither.
        s.actors.objlist_tail[0x79] = PARTY_MODE;
        s.actors.objlist_tail[0x78] = 1;
        after_step(&mut s);
        assert_eq!(s.map.centre.z, 0);
        s.actors.objlist_tail[0x78] = 0;
        s.actors.party.leader_index = 2;
        after_step(&mut s);
        assert_eq!(s.map.centre.z, 0);
        s.actors.party.leader_index = 0;
        after_step(&mut s);
        assert_eq!(s.map.centre.z, 1, "the hole leads down");
    }

    #[test]
    fn resurrection_wakes_the_party_in_the_castle() {
        let mut s = party();
        s.actors.attrs.status[1] = 0x90;
        s.actors.attrs.exp[1] = 1000;
        s.actors.attrs.intelligence[1] = 25;
        s.actors.attrs.base_type_frame[1] = 0x19a;
        s.actors.karma = 50;
        s.actors.worktype[7] = 0x9b;
        let body = put(&mut s, 400, OBJ_BODY_A, at(300, 300, 0));
        // The body's quality (actor 1) comes through the seam; slot 400 reads 0, so no match.
        s.actors.calendar.hour = 3;
        resurrect_party(&mut s);
        assert_eq!(s.map.centre, at(0x133, 0x160, 0));
        assert_eq!(s.objects.location[1], at(0x133, 0x160, 0));
        assert_eq!(s.actors.worktype[7], 0x81);
        assert_eq!(s.actors.party.leader_index, 0xff);
        assert_eq!(s.actors.attrs.exp[1], 1000, "no body matched");
        assert_eq!(s.actors.attrs.status[1], ((0x90 & 0x9f) | 0x40) & 0xe1);
        assert_eq!((s.actors.work.force_teleport, s.ui.panel.dirty), (0, true));
        // The Avatar's raise step itself.
        raise(&mut s, ObjectId(1), body);
        assert_eq!(s.actors.attrs.exp[1], 500);
        assert_eq!(s.actors.attrs.magic[1], 25);
        assert_eq!(s.objects.type_frame[1], 0x19a);
    }

    #[test]
    fn the_roster_byte_reads_past_the_members_like_the_original() {
        let mut s = party();
        s.actors.attrs.unknown_1af1[0xee] = 9;
        assert_eq!(roster_byte(&s, 1), 2);
        assert_eq!(roster_byte(&s, 16), 0);
        assert_eq!(roster_byte(&s, 0xff), 9);
    }
}

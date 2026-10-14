//! The party on the move: the player's step (0x11d4ee) and the followers (0x11c910).

use crate::objects::ObjectId;
use crate::state::State;

use super::leaves;
use super::seams;
use super::walk::{self, DX8, DY8};

/// Why a player step did not happen (for receipts and tests; the original only keeps
/// A4-0x6a90 and the message).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepResult {
    /// The leader moved.
    Moved,
    /// [`walk::can_move_to`] refused the tile (or, diagonally, both orthogonal neighbours).
    Blocked,
    /// A sailing vessel with no wind ("Thou canst not move without wind!").
    NoWind,
    /// 0x10c9d2 refused the move ("Object mismatch").
    Mismatch,
}

fn leader(s: &State) -> u16 {
    u16::from(
        s.actors
            .party
            .members
            .get(usize::from(s.actors.party.leader_index))
            .copied()
            .unwrap_or(0),
    )
}

/// Original: 0x11c76e (actors: is (x, y) beside one of the first `rank` actors of the follow
/// list, and not on top of one)
///
/// Beside = within one tile on both axes. Standing on one is "no" unless that one is type
/// 0x162 or the list's head is (a vehicle). Type 0x1af also counts its rear tile.
pub fn beside_earlier(s: &State, rank: usize, x: u16, y: u16) -> bool {
    let list = &s.actors.work.follow;
    let vehicle = walk::obj_type(s, ObjectId(u16::from(list[0]))) == 0x162;
    let mut beside = false;
    for &id in list.iter().take(rank) {
        let id = ObjectId(u16::from(id));
        let t = walk::obj_type(s, id);
        let p = s.objects.location[id.index()];
        let mut dx = (p.x as i16).wrapping_sub(x as i16);
        let mut dy = (p.y as i16).wrapping_sub(y as i16);
        let near = |dx: i16, dy: i16| (-1..=1).contains(&dx) && (-1..=1).contains(&dy);
        if dx == 0 && dy == 0 && t != 0x162 && !vehicle {
            return false;
        }
        if near(dx, dy) {
            beside = true;
        }
        if t == 0x1af {
            let d = usize::from(walk::obj_frame(s, id) & 6);
            dx = dx.wrapping_sub(DX8[d]);
            dy = dy.wrapping_sub(DY8[d]);
            if dx == 0 && dy == 0 && t != 0x162 && !vehicle {
                return false;
            }
            if near(dx, dy) {
                beside = true;
            }
        }
    }
    beside
}

/// The best of the 8 steps for follower `m` toward its formation spot (fx, fy) (0x11c910): an
/// enterable tile scores 0x100 (0x80 if damaging; skipped then when `m` already stands beside
/// someone ahead), +0x100 when beside someone ahead (0 if not, while it stood beside one),
/// minus the distance to the spot; the highest positive score wins.
fn best_step(s: &mut State, m: u16, rank: usize, (mx, my): (u16, u16), (fx, fy): (i16, i16)) {
    let here = beside_earlier(s, rank, mx, my);
    let mut best_d: i16 = -1;
    let mut best: i16 = 0;
    for d in 0..8usize {
        let nx = mx.wrapping_add_signed(DX8[d]);
        let ny = my.wrapping_add_signed(DY8[d]);
        if !walk::can_move_to(s, m, nx, ny) {
            continue;
        }
        let mut score: i16 = if s.actors.work.saw_damage == 0 {
            0x100
        } else if here {
            continue;
        } else {
            0x80
        };
        if beside_earlier(s, rank, nx, ny) {
            score += 0x100;
        } else if here {
            score = 0;
        }
        score -= (nx as i16).wrapping_sub(fx).wrapping_abs();
        score -= (ny as i16).wrapping_sub(fy).wrapping_abs();
        if score > best {
            best_d = d as i16;
            best = score;
        }
    }
    if best_d >= 0 {
        let d = best_d as usize;
        let (nx, ny) = (
            mx.wrapping_add_signed(DX8[d]),
            my.wrapping_add_signed(DY8[d]),
        );
        seams::move_object(s, ObjectId(m), nx, ny, walk::party_z(s));
        s.actors.work.stepped = 1;
        walk::face(s, m, d as u16);
    }
}

/// Original: 0x11c910 (party: the followers keep formation behind the leader)
///
/// Not while a vehicle carries the party. Two passes over the members in worktype 1 on the
/// party's level who can act: each has a spot (rank r: `back(r)` behind the leader along its
/// facing, `side(r)` across). One not beside anyone ahead in the list steps (in both passes);
/// one beside someone ahead steps only in the second pass, and then only when `strict` and off
/// its spot, or (not strict) when its spot is ahead of it along the facing. Each such member
/// pays 5 points per pass.
pub fn follow(s: &mut State, leader: u16, strict: bool) {
    if s.actors.party.leader_index == s.actors.party.size {
        return;
    }
    s.actors.work.follow[0] = leader as u8;
    let lp = s.objects.location[usize::from(leader)];
    let (lx, ly) = (lp.x as i16, lp.y as i16);
    let lt = walk::obj_type(s, ObjectId(leader));
    let f = if (0x178..=0x19a).contains(&lt) || lt == 0x16b {
        usize::from((walk::obj_frame(s, ObjectId(leader)) >> 1) & 6)
    } else {
        usize::from(s.actors.attrs.movement_flags[usize::from(leader)] & 6)
    };
    let (fdx, fdy) = (DX8[f], DY8[f]);
    for pass in 0..2 {
        let mut rank = 1usize;
        for i in 0..usize::from(s.actors.party.size) {
            let m = s.actors.party.members[i];
            if let Some(slot) = s.actors.work.follow.get_mut(rank) {
                *slot = m;
            }
            let g = &s.actors.game;
            let fx = lx - fdx * g.back(rank) - fdy * g.side(rank);
            let fy = ly + fdx * g.side(rank) - fdy * g.back(rank);
            let m = u16::from(m);
            let mi = usize::from(m);
            if m == leader
                || u16::from(s.objects.location[mi].z) != walk::party_z(s)
                || s.actors.worktype[mi] != 1
            {
                continue;
            }
            if super::turn::swallowed(s, m) {
                continue;
            }
            if s.actors.attrs.status[mi] & 0x16 != 0
                || s.actors.attrs.movement_flags[mi] & 0x10 != 0
            {
                continue;
            }
            let saved = s.actors.movement_points[mi];
            let p = s.objects.location[mi];
            let go = if beside_earlier(s, rank, p.x, p.y) {
                pass == 1
                    && if strict {
                        (p.x as i16, p.y as i16) != (fx, fy)
                    } else {
                        let v = (fx - p.x as i16)
                            .wrapping_mul(fdx)
                            .wrapping_add((fy - p.y as i16).wrapping_mul(fdy));
                        v > 0
                    }
            } else {
                true
            };
            if go {
                best_step(s, m, rank, (p.x, p.y), (fx, fy));
            }
            s.actors.movement_points[mi] = saved;
            walk::spend_mp(s, m, 5);
            rank += 1;
        }
    }
}

/// Map tiles 8..15 are currents / whirlpools [likely]: their tile number − 8 is a direction.
fn current_at(s: &State, x: u16, y: u16) -> Option<u16> {
    let t = u16::from(seams::tile_at(s, x, y, walk::party_z(s)));
    (8..16).contains(&t).then(|| t - 8)
}

/// Original: 0x11d4ee (party: the player's step in direction `dir` (0 = north, clockwise))
///
/// A drunk party (alcohol > 3) staggers on a coin flip ("Hic!", random direction). Sailing
/// vessels (0x19f, 0x1a7) go with the wind (0x19f along a current by the sailing table);
/// without wind nothing moves. The step must be enterable; a diagonal one also needs one of
/// its two orthogonal neighbours (with a clicked target, a blocked diagonal slides along the
/// axis toward it). A party member standing on the tile swaps places (one in worktype 3 is
/// pushed on instead when it can be). Cost by vehicle (ship by angle to the wind, raft by
/// angle to the current) or the tile's cost (diagonals 5 more). Then the view centre and
/// window follow, the leader moves and faces, the followers follow ([`follow`]) and the level
/// transition runs (0x11d1d4).
pub fn player_step(s: &mut State, dir: u16) -> StepResult {
    s.actors.work.hic = 1;
    let l = leader(s);
    let li = usize::from(l);
    let lp = s.objects.location[li];
    s.map.centre.x = lp.x;
    s.map.centre.y = lp.y;
    s.actors.work.step_blocked = 0;
    let mut dir = dir as i16;
    if s.actors.tail(super::tail::ALCOHOL) > 3 && s.rng.range(0, 1) != 0 {
        // "Hic!" (PC-relative string at 0x11b912).
        seams::print_image(s, 0x11b912);
        dir = s.rng.range(0, 7) as i16;
    }
    let t = walk::obj_type(s, ObjectId(l));
    if t == 0x19f || t == 0x1a7 {
        let mut drift = dir;
        dir = i16::from(s.actors.wind as i8) ^ 4;
        if t == 0x19f {
            if let Some(c) = current_at(s, lp.x, lp.y) {
                drift = c as i16;
            }
            dir = if dir >= 0 {
                let i = (drift as usize) * 8 + dir as usize;
                i16::from(s.actors.game.sail.get(i).copied().unwrap_or(0))
            } else {
                drift
            };
        }
        if dir < 0 {
            seams::print(s, -0x73aa, &[]);
            s.actors.work.prompt = 1;
            s.actors.movement_points[li] = 0;
            return StepResult::NoWind;
        }
    }
    let d = (dir as usize) & 7;
    let (cx, cy) = (s.map.centre.x, s.map.centre.y);
    let mut nx = cx.wrapping_add_signed(DX8[d]) & 0x3ff;
    let mut ny = cy.wrapping_add_signed(DY8[d]) & 0x3ff;
    let diagonal = DX8[d] != 0 && DY8[d] != 0;
    if !walk::can_move_to(s, l, nx, ny) {
        s.actors.work.step_blocked = 1;
        if s.actors.work.mouse_walk != 0 && diagonal {
            slide(s, l, (cx, cy), (&mut nx, &mut ny));
        }
    } else if diagonal {
        let ox = cx.wrapping_add_signed(DX8[d]) & 0x3ff;
        let oy = cy.wrapping_add_signed(DY8[d]) & 0x3ff;
        if !walk::can_move_to(s, l, ox, cy) && !walk::can_move_to(s, l, cx, oy) {
            s.actors.work.step_blocked = 1;
        }
    }
    if s.actors.work.step_blocked != 0 {
        let p = s.objects.location[li];
        s.map.centre.x = p.x;
        s.map.centre.y = p.y;
        s.actors.work.hic = 0;
        return StepResult::Blocked;
    }
    if t != 0x162 {
        swap_member(s, (cx, cy), (nx, ny), d);
    }
    pay_step(s, l, d, diagonal, (cx, cy));
    s.map.centre.x = nx;
    s.map.centre.y = ny;
    seams::rebuild_window(s);
    let z = walk::party_z(s);
    let result = if seams::move_object(s, ObjectId(l), nx, ny, z) >= 0 {
        let mf = &mut s.actors.attrs.movement_flags[li];
        *mf = (*mf & 0xf8) | d as u8;
        s.actors.work.stepped = 1;
        walk::face(s, l, d as u16);
        s.map.view_dirty = 1;
        StepResult::Moved
    } else {
        let p = s.objects.location[li];
        s.map.centre.x = p.x;
        s.map.centre.y = p.y;
        seams::rebuild_window(s);
        // "Object mismatch" (PC-relative string at 0x11b918).
        seams::print_image(s, 0x11b918);
        StepResult::Mismatch
    };
    s.actors.work.hic = 0;
    follow(s, l, false);
    seams::level_transition(s);
    s.actors.work.hic = 0;
    result
}

/// 0x11d4ee with a clicked target: a blocked diagonal tries the axis along which the target is
/// further first, then the other; the first enterable one is taken.
fn slide(s: &mut State, l: u16, (cx, cy): (u16, u16), (nx, ny): (&mut u16, &mut u16)) {
    let (tx, ty) = s.actors.work.mouse_target;
    let ddx = tx.wrapping_sub(cx as i16).wrapping_abs();
    let ddy = ty.wrapping_sub(cy as i16).wrapping_abs();
    // Axis 0 keeps the x step (y stays), axis 1 keeps the y step (x stays).
    let order = if ddx > ddy { [0, 1] } else { [1, 0] };
    for axis in order {
        let (x, y) = if axis == 0 { (*nx, cy) } else { (cx, *ny) };
        if walk::can_move_to(s, l, x, y) {
            *nx = x;
            *ny = y;
            s.actors.work.step_blocked = 0;
            return;
        }
    }
}

/// 0x11d4ee: a party member (alive, not 0x162) on the target tile swaps with the leader; one
/// in worktype 3 first tries to step on, away from the leader.
fn swap_member(s: &mut State, (cx, cy): (u16, u16), (nx, ny): (u16, u16), d: usize) {
    let z = walk::party_z(s);
    let li = usize::from(s.actors.party.leader_index);
    for i in 0..usize::from(s.actors.party.size) {
        if i == li {
            continue;
        }
        let m = u16::from(s.actors.party.members[i]);
        let mi = usize::from(m);
        if s.actors.attrs.status[mi] & 0x10 != 0 || walk::obj_type(s, ObjectId(m)) == 0x162 {
            continue;
        }
        let p = s.objects.location[mi];
        if p.x != nx || p.y != ny || u16::from(p.z) != z {
            continue;
        }
        if s.actors.worktype[mi] == 3 {
            let (ax, ay) = (
                nx.wrapping_mul(2).wrapping_sub(cx),
                ny.wrapping_mul(2).wrapping_sub(cy),
            );
            if super::npc::approach(s, m, ax, ay) {
                return;
            }
        }
        seams::move_object(s, ObjectId(m), cx, cy, z);
        s.actors.work.stepped = 1;
        walk::face(s, m, ((d + 4) & 7) as u16);
        return;
    }
}

/// 0x11d4ee's movement cost of the leader's step.
fn pay_step(s: &mut State, l: u16, d: usize, diagonal: bool, (cx, cy): (u16, u16)) {
    let t = walk::obj_type(s, ObjectId(l));
    let wind = s.actors.wind as i8;
    if t == 0x19c && wind >= 0 {
        let i = (d as i16 - i16::from(wind)) as usize & 7;
        let c = s.actors.game.ship_cost[i];
        walk::spend_mp(s, l, i16::from(c));
    } else if t == 0x19e {
        let c = current_at(s, cx, cy).map_or(d as u16, |c| c);
        let i = (d as u16).wrapping_sub(c) as usize & 7;
        let cost = s.actors.game.raft_cost[i];
        walk::spend_mp(s, l, i16::from(cost));
    } else if t == 0x19f || t == 0x1a7 {
        walk::spend_mp(s, l, 5);
    } else if diagonal {
        walk::spend_tile_cost(s, l);
        walk::spend_mp(s, l, 5);
    } else {
        walk::spend_tile_cost(s, l);
    }
}

/// [combat] 0x1230ce (a hit's flash and redraw on `actor`). Stub contract: argument the actor;
/// no result; nothing drawn. Reached only from hazards ([`tile_effect`]).
fn flash(_s: &mut State, _actor: u16) {
    crate::todo_stub!("0x1230ce flash a hit actor (fallback: none)", ())
}

/// [combat] 0x1221da (put an actor to sleep / knock it down). Stub contract: argument the
/// actor; no result; the sleep status bit is already set by the caller.
fn fall_asleep(_s: &mut State, _actor: u16) {
    crate::todo_stub!("0x1221da lie down without a bed (fallback: none)", ())
}

/// [objects] 0x10b18c / 0x10b0ba (the first object of type `t` carried by `actor`, then the
/// next). Stub contract: none carried (inventories are not in the slot tables before
/// 0x10e1ce), so swamp boots never protect.
fn readied(_s: &State, _actor: u16, _t: u16) -> bool {
    crate::todo_stub!("0x10b18c carried object of a type (fallback: none)", false)
}

/// Original: 0x102f3c (party: what the tile under an actor does to it: swamp, fire, poison
/// field, web, sleep field)
///
/// Marks the actor (movement flag 0x08) unless dead. By `tile`: swamp (2..5) poisons unless
/// swamp boots are readied; webs (0x234) paralyse on `random(1, 30)` ≥ strength (not type
/// 0x169); poison fields poison (not the protected or immune); fire and other hazards hurt
/// (1..8 or 1..30, doubled for creatures with flag 0x10, none for the immune or protected);
/// sleep fields put the unprotected to sleep. Damage, flashes and falling asleep are combat
/// stub points.
pub fn tile_effect(s: &mut State, actor: u16, tile: u16) {
    let a = usize::from(actor);
    let me = ObjectId(actor);
    let mut immune = false;
    s.actors.attrs.movement_flags[a] |= 0x08;
    if s.actors.attrs.status[a] & 0x10 != 0 {
        return;
    }
    let ci = seams::creature_index(s, walk::obj_type(s, me));
    // The original tests bits of the flag word's low byte (offset + 1).
    let cflag = |s: &State, bit: u16| s.actors.game.creature_flags(ci) & bit != 0;
    let protected = |s: &State| s.actors.attrs.status[a] & 0x01 != 0;
    let poison = |s: &mut State, mut immune: bool| {
        if protected(s) || cflag(s, 0x04) {
            immune = true;
        }
        if immune || s.actors.attrs.status[a] & 0x08 != 0 {
            return;
        }
        seams::print(s, -0x75af, &[seams::Arg::Name(me)]);
        s.actors.work.prompt = 1;
        seams::after_turn_refresh(s);
        flash(s, actor);
        s.actors.attrs.status[a] |= 0x08;
        s.ui.panel.dirty = true;
    };
    match tile {
        2..=5 => {
            if !readied(s, actor, 0x1c) {
                poison(s, immune);
            }
        }
        0x48d => poison(s, immune),
        0x234 => {
            if protected(s) {
                return;
            }
            let st = leaves::strength(s, actor) as i16;
            if st >= s.rng.range(1, 30) as i16 || walk::obj_type(s, me) == 0x169 {
                return;
            }
            s.actors.attrs.status[a] |= 0x02;
            seams::print(s, -0x72e7, &[seams::Arg::Name(me)]);
            s.actors.work.prompt = 1;
        }
        0xdc..=0xdf | 0x37a | 0x464 | 0x465 | 0x46a | 0x46b | 0x48c | 0x4a9 => {
            if protected(s) || cflag(s, 0x08) {
                immune = true;
            }
            if immune {
                return;
            }
            let mut n = if tile == 0x48c {
                s.rng.range(1, 30)
            } else {
                s.rng.range(1, 8)
            };
            if cflag(s, 0x10) {
                n *= 2;
            }
            seams::after_turn_refresh(s);
            super::clock::hurt(s, actor, n as u8);
        }
        0x5b2..=0x5b7 | 0x232 | 0x2db => {
            let is_16e = walk::obj_type(s, me) == 0x16e;
            if (0x5b2..=0x5b7).contains(&tile) && is_16e || protected(s) {
                return;
            }
            seams::after_turn_refresh(s);
            let n = s.rng.range(1, 8);
            super::clock::hurt(s, actor, n as u8);
        }
        0x2dc | 0x3f2 => {
            if protected(s) {
                return;
            }
            seams::after_turn_refresh(s);
            let n = s.rng.range(1, 30);
            super::clock::hurt(s, actor, n as u8);
        }
        0x48f => {
            if cflag(s, 0x01) || protected(s) || s.actors.attrs.status[a] & 0x04 != 0 {
                return;
            }
            seams::after_turn_refresh(s);
            flash(s, actor);
            s.actors.attrs.status[a] |= 0x04;
            fall_asleep(s, actor);
        }
        _ => {}
    }
}

/// Original: 0x1279d4 (party: member `i` heals (rest): half the missing hit points plus a
/// random part of that half, magic points to the maximum; "has healed." when `announce`)
pub fn heal(s: &mut State, i: usize, announce: bool) {
    let m = s.actors.party.members.get(i).copied().unwrap_or(0);
    let mi = usize::from(m);
    let id = u16::from(m);
    let mut gap = (leaves::max_hp(s, id) as i16).wrapping_sub(i16::from(s.actors.attrs.hp[mi]));
    if gap != 0 {
        if gap == 1 {
            gap = 2;
        }
        let half = gap / 2;
        let gain = half.wrapping_add(s.rng.range(0, half as u16) as i16);
        let hp = i16::from(s.actors.attrs.hp[mi]).wrapping_add(gain);
        let max = leaves::max_hp(s, id);
        s.actors.attrs.hp[mi] = if hp < max as i16 { hp as u8 } else { max as u8 };
        if announce {
            seams::print(s, 0, &[seams::Arg::PartyName(i as u8)]);
            // " has healed." (PC-relative string at 0x1272a8).
            seams::print_image(s, 0x1272a8);
        }
    }
    s.actors.attrs.magic[mi] = leaves::max_magic(s, id) as u8;
}

#[cfg(test)]
mod tests {
    use u6_formats::position::Position;

    use super::*;
    use crate::actors::{GameTables, tail};

    /// Party 1 (leader), 2, 3 in a column going north at (20, 20..22); an NPC 9 at (20, 18).
    fn party() -> State {
        let mut s = State::empty();
        crate::actors::walk::tests::creature_tiles(&mut s);
        s.actors.set_tail(tail::SOLO_MEMBER, 0xff);
        s.actors.party.members[..3].copy_from_slice(&[1, 2, 3]);
        s.actors.party.size = 3;
        s.map.centre = Position { x: 20, y: 20, z: 0 };
        for (i, a) in [1usize, 2, 3].into_iter().enumerate() {
            s.objects.type_frame[a] = 0x17a;
            s.objects.location[a] = Position {
                x: 20,
                y: 20 + i as u16,
                z: 0,
            };
            s.actors.attrs.status[a] = 0x80;
            s.actors.worktype[a] = if i == 0 { 2 } else { 1 };
            s.actors.movement_points[a] = 30;
        }
        s.objects.type_frame[9] = 0x17a;
        s.objects.location[9] = Position { x: 20, y: 18, z: 0 };
        // Ranks 1, 2 one and two tiles behind, no sideways offset.
        let mut formation = vec![0; 28];
        formation[12] = 1;
        formation[13] = 2;
        s.actors.game = GameTables {
            formation,
            ..GameTables::default()
        };
        s
    }

    #[test]
    fn step_moves_the_leader_and_the_view_and_followers_close_up() {
        let mut s = party();
        assert_eq!(player_step(&mut s, 0), StepResult::Moved);
        assert_eq!(s.objects.location[1], Position { x: 20, y: 19, z: 0 });
        assert_eq!((s.map.centre.x, s.map.centre.y), (20, 19));
        assert_eq!(s.objects.location[2].y, 20, "rank 1 one behind");
        assert_eq!(s.objects.location[3].y, 21, "rank 2 two behind");
        assert_eq!(
            player_step(&mut s, 0),
            StepResult::Blocked,
            "NPC 9 stands there"
        );
        assert_eq!(s.objects.location[1].y, 19);
        assert_eq!((s.map.centre.x, s.map.centre.y), (20, 19));
    }

    #[test]
    fn stepping_onto_a_member_swaps() {
        let mut s = party();
        assert_eq!(player_step(&mut s, 4), StepResult::Moved);
        assert_eq!(s.objects.location[1].y, 21);
        assert_eq!(
            s.objects.location[2].y, 20,
            "member 2 took the leader's tile"
        );
    }

    #[test]
    fn diagonal_needs_an_open_side() {
        let mut s = party();
        // NPCs east and north of the leader leave the north-east diagonal shut.
        s.objects.location[9] = Position { x: 21, y: 20, z: 0 };
        s.objects.type_frame[8] = 0x17a;
        s.objects.location[8] = Position { x: 20, y: 19, z: 0 };
        assert_eq!(player_step(&mut s, 1), StepResult::Blocked);
        s.objects.location[8].x = 25;
        assert_eq!(player_step(&mut s, 1), StepResult::Moved);
        assert_eq!(s.objects.location[1], Position { x: 21, y: 19, z: 0 });
    }

    #[test]
    fn no_wind_no_sailing() {
        let mut s = party();
        s.objects.type_frame[1] = 0x1a7; // balloon: not a humanoid frame layout
        s.actors.wind = 0xff;
        assert_eq!(player_step(&mut s, 0), StepResult::NoWind);
        assert_eq!(s.actors.movement_points[1], 0);
        s.actors.wind = 2; // from the east: drifts west
        assert_eq!(player_step(&mut s, 0), StepResult::Moved);
        assert_eq!(s.objects.location[1].x, 19);
    }

    #[test]
    fn heal_restores_part_of_the_missing_points() {
        let mut s = party();
        s.actors.attrs.level[1] = 2;
        s.actors.attrs.hp[1] = 20;
        heal(&mut s, 0, false);
        let hp = s.actors.attrs.hp[1];
        assert!(
            (40..=60).contains(&hp),
            "20 + 20..40 of the 40 missing, got {hp}"
        );
        s.actors.attrs.hp[1] = 59;
        heal(&mut s, 0, false);
        assert_eq!(s.actors.attrs.hp[1], 60, "gap 1 heals as 2, capped");
    }

    #[test]
    fn webs_paralyse_the_weak() {
        let mut s = party();
        s.actors.attrs.strength[2] = 0;
        tile_effect(&mut s, 2, 0x234);
        assert_eq!(s.actors.attrs.status[2] & 0x02, 0x02);
        assert_eq!(s.actors.attrs.movement_flags[2] & 0x08, 0x08);
        s.actors.attrs.status[3] |= 0x01;
        tile_effect(&mut s, 3, 0x234);
        assert_eq!(s.actors.attrs.status[3] & 0x02, 0, "protected");
    }
}

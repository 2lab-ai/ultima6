//! The world turn: the actor scheduler 0x1213de and the per-turn party bookkeeping around it.

use std::cell::RefCell;

use crate::engine::Io;
use crate::state::State;

use super::seams::{self, Arg};
use super::{clock, leaves, npc, path, tail, walk};

/// Refills (clock minutes) without anyone able to act after which the scheduler yields to the
/// host once (port checkpoint, spec 03 §1: a yield changes no game state).
const IDLE_REFILLS_PER_YIELD: u32 = 60;

fn healthy(s: &State, actor: usize) -> bool {
    s.actors.attrs.status[actor] & 0x16 == 0
        && s.actors.attrs.movement_flags[actor] & 0x10 == 0
        && s.objects.obj_status[actor] & 0x04 == 0
}

fn member(s: &State, i: usize) -> usize {
    usize::from(s.actors.party.members.get(i).copied().unwrap_or(0))
}

/// The view centre follows member `i` (x and y only, as the original writes them).
fn centre_on(s: &mut State, i: usize) {
    let p = s.objects.location[member(s, i)];
    s.map.centre.x = p.x;
    s.map.centre.y = p.y;
}

/// Original: 0x121020 (actors: the Avatar has died: "An unending darkness engulfs thee")
///
/// Unless the leader index is 16: music off, the two lines with a full redraw between, then
/// the resurrection 0x1095bc.
pub fn darkness(s: &mut State) {
    if s.actors.party.leader_index == 0x10 {
        return;
    }
    crate::sound::play_song(s, 0);
    seams::print(s, -0x7435, &[]);
    s.gfx.window_shown = 1;
    seams::full_redraw(s);
    seams::print(s, -0x740e, &[]);
    crate::todo_stub!("0x1095bc resurrect the party (fallback: none)", ());
}

/// Original: 0x121056 (party: leader and solo-mode bookkeeping per turn)
///
/// With the leader index at the party size (a vehicle carries the party) actor 0 is cleaned
/// up. A dead Avatar (actor 1) ends in [`darkness`]. In solo mode a leader who cannot act ends
/// it (0x104094). In party mode a leader who cannot act hands over to the first member who
/// can; and member 0 takes the lead back from any other member once it can act (in combat the
/// old leader returns to its combat worktype instead of following).
pub fn leader_bookkeeping(s: &mut State) {
    let size = s.actors.party.size;
    if s.actors.party.leader_index > size {
        return;
    }
    if s.actors.party.leader_index == size {
        s.objects.obj_status[0] &= 0xbb;
        s.actors.attrs.status[0] = (s.actors.attrs.status[0] & 0x81) | 0x40;
        s.actors.attrs.movement_flags[0] &= 0xef;
    }
    if s.actors.attrs.status[1] & 0x10 != 0 {
        darkness(s);
    }
    let li = usize::from(s.actors.party.leader_index);
    if s.actors.solo_member() >= 0 && !healthy(s, member(s, li)) {
        crate::todo_stub!("0x104094 end solo mode (fallback: stays solo)", ());
    }
    if s.actors.solo_member() >= 0 {
        return;
    }
    let leader = member(s, li);
    if !healthy(s, leader) {
        for i in 0..usize::from(size) {
            let m = member(s, i);
            if healthy(s, m) && s.actors.worktype[m] != 0 {
                s.actors.worktype[leader] = 1;
                s.actors.worktype[m] = 2;
                s.actors.work.leader_kept = i as u16;
                s.actors.party.leader_index = i as u8;
                s.actors.work.prompt = 1;
                centre_on(s, i);
                seams::rebuild_window(s);
                break;
            }
        }
    }
    let li = s.actors.party.leader_index;
    if li < size && li != 0 && s.actors.solo_member() < 0 && healthy(s, member(s, 0)) {
        let leader = member(s, usize::from(li));
        if !s.actors.combat_mode() {
            s.actors.worktype[leader] = 1;
            s.actors.work.leader_kept = 0;
            s.actors.party.leader_index = 0;
            s.actors.work.prompt = 1;
            centre_on(s, 0);
            seams::rebuild_window(s);
        } else {
            s.actors.worktype[leader] = s.actors.attrs.combat_mode[leader];
        }
        let first = member(s, 0);
        s.actors.worktype[first] = 2;
    }
}

/// Original: 0x120b64 (party: party centre and the hostiles' front for this turn)
///
/// Party centre = rounded mean position of the members not in worktype 5 or 6 (the leader's
/// position when none). Hostiles = living actors 1..255 with alignment bit 0x20 within 23
/// tiles of the leader (worktype 7 only within 5); their centre likewise (count / 2 rounding),
/// else one step ahead of member 0. Then, along and across the party→hostiles vector, the most
/// advanced party member and the flanks of both sides among the actors in worktype 2 or 3. A
/// hostile centre on the party centre is pushed one step ahead of member 0.
pub fn formation(s: &mut State) {
    let size = usize::from(s.actors.party.size);
    let (mut n, mut sx, mut sy) = (0i32, 0i32, 0i32);
    for i in 0..size {
        let m = member(s, i);
        let wt = s.actors.worktype[m];
        if wt == 5 || wt == 6 {
            continue;
        }
        n += 1;
        let p = s.objects.location[m];
        sx += i32::from(p.x);
        sy += i32::from(p.y);
    }
    let c = s.map.centre;
    s.actors.work.party_centre = if n != 0 {
        let half = i32::from((size as i16) >> 1);
        (((sx + half) / n) as i16, ((sy + half) / n) as i16)
    } else {
        (c.x as i16, c.y as i16)
    };
    let (mut n, mut sx, mut sy) = (0i32, 0i32, 0i32);
    for a in 1..0x100usize {
        let st = s.actors.attrs.status[a];
        if s.objects.type_frame[a] == 0 || st & 0x10 != 0 || st & 0x20 == 0 {
            continue;
        }
        let p = s.objects.location[a];
        let dist = |v: u16, w: u16| (v as i16).wrapping_sub(w as i16).wrapping_abs();
        let (dx, dy) = (dist(p.x, c.x), dist(p.y, c.y));
        if s.actors.worktype[a] == 7 && (dx > 5 || dy > 5) {
            continue;
        }
        if dx < 0x18 && dy < 0x18 {
            n += 1;
            sx += i32::from(p.x);
            sy += i32::from(p.y);
        }
    }
    let w = &mut s.actors.work;
    w.enemies = n as i16;
    let lead_dir = usize::from(s.actors.attrs.movement_flags[member(s, 0)] & 7);
    let ahead = (
        (c.x as i16).wrapping_add(walk::DX8[lead_dir]),
        (c.y as i16).wrapping_add(walk::DY8[lead_dir]),
    );
    if n == 0 {
        s.actors.work.flag_6a88 = 0;
        s.actors.work.enemy_centre = ahead;
    } else {
        let half = n >> 1;
        let e = (((sx + half) / n) as i16, ((sy + half) / n) as i16);
        s.actors.work.enemy_centre = e;
        flanks(s, e);
    }
    let w = &mut s.actors.work;
    if w.party_centre == w.enemy_centre {
        w.enemy_centre.0 = w.enemy_centre.0.wrapping_add(walk::DX8[lead_dir]);
        w.enemy_centre.1 = w.enemy_centre.1.wrapping_add(walk::DY8[lead_dir]);
    }
}

/// 0x120b64's projections of the fighters (worktype 2 or 3) onto the party→hostiles axis.
fn flanks(s: &mut State, e: (i16, i16)) {
    let (px, py) = s.actors.work.party_centre;
    let (vx, vy) = (e.0.wrapping_sub(px), e.1.wrapping_sub(py));
    let mut f = [-1i16; 6];
    f[0] = s.actors.work.front[0];
    f[2] = s.actors.work.front[2];
    f[3] = s.actors.work.front[3];
    let (mut best_along, mut party_left, mut party_right) = (i16::MIN, 0i16, 0i16);
    let (mut foe_left, mut foe_right) = (0i16, 0i16);
    for a in 0..0x100usize {
        let wt = s.actors.worktype[a];
        if s.actors.attrs.status[a] & 0x10 != 0 || (wt != 3 && wt != 2) {
            continue;
        }
        let p = s.objects.location[a];
        let rx = (p.x as i16).wrapping_sub(px);
        let ry = (p.y as i16).wrapping_sub(py);
        let along = rx.wrapping_mul(vx).wrapping_add(ry.wrapping_mul(vy));
        let across = rx.wrapping_mul(vy).wrapping_sub(ry.wrapping_mul(vx));
        if s.actors.attrs.status[a] & 0x80 != 0 {
            if along > best_along {
                f[0] = a as i16;
                best_along = along;
            }
            if across > party_left {
                f[2] = a as i16;
                party_left = across;
            }
            if across < party_right {
                f[3] = a as i16;
                party_right = across;
            }
        } else {
            if f[1] < 0 {
                f[1] = a as i16;
            }
            if across > foe_left {
                f[4] = a as i16;
                foe_left = across;
            }
            if across < foe_right {
                f[5] = a as i16;
                foe_right = across;
            }
        }
    }
    s.actors.work.front = f;
}

/// Original: 0x1217d4 (actors: after a load: dexterity at least 1, actor 0 out of schedule
/// worktypes, leader kept, formation)
pub fn after_load(s: &mut State) {
    for d in s.actors.attrs.dexterity.iter_mut() {
        if *d == 0 {
            *d = 1;
        }
    }
    if s.actors.worktype[0] >= 0x80 {
        s.actors.worktype[0] = 0;
    }
    if s.actors.solo_member() < 0 {
        s.actors.work.leader_kept = u16::from(s.actors.party.leader_index);
    }
    formation(s);
}

/// Original: 0x11c6c2 (actors: a swallowed actor fights its way out: true = still inside)
///
/// Only actors with movement flag 0x10. The player's own character prints "ARGH!". On
/// `random(1, 30)` below its strength it is freed (" regurgitated!", redraw requested);
/// otherwise it loses 1..15 hit points and its turn.
pub fn swallowed(s: &mut State, actor: u16) -> bool {
    let a = usize::from(actor);
    if s.actors.attrs.movement_flags[a] & 0x10 == 0 {
        return false;
    }
    let me = crate::objects::ObjectId(actor);
    if s.actors.worktype[a] == 2 {
        seams::print(s, -0x75e1, &[Arg::Name(me)]);
    }
    let r = s.rng.range(1, 30) as i16;
    if r < leaves::strength(s, actor) as i16 {
        seams::print_name(s, me);
        seams::print(s, -0x75d3, &[]);
        s.actors.attrs.movement_flags[a] &= !0x10;
        s.map.view_dirty = 1;
        return false;
    }
    let damage = s.rng.range(1, 15) as u8;
    clock::hurt(s, actor, damage);
    s.actors.movement_points[a] = 0;
    true
}

/// Original: 0x120a1c (actors: send NPCs to their scheduled places, at most 3 jumps and 3
/// path searches per turn)
///
/// Needs a free path slot. Every NPC 2..0xdf in worktype 0x81 may jump ([`path::teleport_home`])
/// and, still 0x81, gets a path search toward its entry ([`path::seek`]); the slot cursor moves
/// past taken slots, and with all 8 taken the pass ends. When no NPC was waiting for a search,
/// the unreachable ones (0x86) are given another chance (back to 0x81, may jump).
pub fn go_to_schedules(s: &mut State) {
    let mut settled = true;
    let slot = npc::free_slot(s);
    s.actors.work.slot = slot;
    if slot == 8 {
        return;
    }
    s.actors.work.searches = 0;
    s.actors.work.teleports = 0;
    for a in 2..0xe0u16 {
        let i = usize::from(a);
        if s.actors.worktype[i] != 0x81 {
            continue;
        }
        if s.actors.work.teleports < 3 {
            path::teleport_home(s, a);
        }
        if s.actors.worktype[i] != 0x81 || s.actors.work.searches >= 3 {
            continue;
        }
        settled = false;
        let Some(e) = path::schedule_entry(s, a) else {
            continue;
        };
        path::seek(s, a, path::Goal::At(e.pos.x, e.pos.y));
        loop {
            let k = s.actors.work.slot;
            if s.actors.tail(tail::SLOT_ACTOR + usize::from(k)) == 0 || k >= 8 {
                break;
            }
            s.actors.work.slot = k + 1;
        }
        if s.actors.work.slot == 8 {
            break;
        }
    }
    if settled {
        for a in 2..0xe0u16 {
            let i = usize::from(a);
            if s.actors.worktype[i] == 0x86 {
                s.actors.worktype[i] = 0x81;
                if s.actors.work.teleports < 3 {
                    path::teleport_home(s, a);
                }
            }
        }
    }
}

/// Is `actor` a candidate to act this round (0x1213de's filters before the window test)?
fn may_act(s: &State, a: usize, z: u8) -> bool {
    let st = s.actors.attrs.status[a];
    let wt = s.actors.worktype[a];
    s.objects.type_frame[a] != 0
        && wt != 0
        && st & 0x02 == 0
        && (st & 0x04 == 0 || wt == 0x80)
        && st & 0x10 == 0
        && s.actors.mp(a) > 0
        && s.objects.obj_status[a] & 0x18 == 0
        && s.objects.location[a].z == z
}

/// What one pass of the scheduler's actor loop chose.
struct Pick {
    actor: i16,
    mp: i16,
    dex: i16,
}

/// One pass over the 256 actors (0x1213de): actors outside the 40 × 40 window around the party
/// lose their points (walkers there jump home or search again); party followers (worktype 1)
/// never act here; dormant ones (0x80) in the window wake up ([`path::arrive`]). The pick is
/// the actor with the highest points-to-dexterity ratio (ties: higher dexterity); one with
/// points ≥ dexterity is taken at once.
fn pick_actor(s: &mut State, mask: u16, pick: &mut Pick) {
    let c = s.map.centre;
    let wx = c.x.wrapping_sub(16) & 0x3f8;
    let wy = c.y.wrapping_sub(16) & 0x3f8;
    for i in 1..usize::from(s.actors.party.size) {
        let m = member(s, i);
        if s.actors.worktype[m] == 1 && s.actors.mp(m) <= 0 {
            let first = member(s, 0);
            s.actors.movement_points[first] = 0;
        }
    }
    for a in 0..0x100usize {
        if !may_act(s, a, c.z) {
            continue;
        }
        let p = s.objects.location[a];
        let near = (p.x.wrapping_sub(wx) & mask) <= 0x27 && (p.y.wrapping_sub(wy) & mask) <= 0x27;
        if !near {
            let wt = s.actors.worktype[a];
            if (0x83..=0x86).contains(&wt) {
                path::teleport_home(s, a as u16);
            }
            let wt = s.actors.worktype[a];
            if (0x83..0x86).contains(&wt) {
                let slot = usize::from(s.actors.attrs.unknown_18f1[a]);
                s.actors.set_tail(tail::SLOT_ACTOR + slot, 0);
                s.actors.worktype[a] = 0x81;
            }
            s.actors.movement_points[a] = 0;
            continue;
        }
        if s.actors.worktype[a] == 1 {
            continue;
        }
        if s.actors.worktype[a] == 0x80 {
            path::arrive(s, a as u16);
        }
        let mp = i16::from(s.actors.mp(a));
        let dex = i16::from(s.actors.attrs.dexterity[a]);
        let v = mp
            .wrapping_mul(pick.dex)
            .wrapping_sub(dex.wrapping_mul(pick.mp));
        let full = s.actors.mp(a) >= s.actors.attrs.dexterity[a] as i8;
        if full || v > 0 || (v == 0 && dex > pick.dex) {
            *pick = Pick {
                actor: a as i16,
                mp,
                dex,
            };
        }
        if full {
            break;
        }
    }
}

/// Every actor's points become its dexterity, plus any debt (negative points) carried over.
fn refill(s: &mut State) {
    for a in 0..0x100 {
        let mp = s.actors.movement_points[a];
        let dex = s.actors.attrs.dexterity[a];
        s.actors.movement_points[a] = if (mp as i8) < 0 {
            dex.wrapping_add(mp)
        } else {
            dex
        };
    }
}

/// Original: 0x1213de (actors: world turn: let NPCs act until the player's character is next)
///
/// After the bookkeeping ([`leader_bookkeeping`], [`formation`], [`go_to_schedules`]), picks
/// the next actor by movement points again and again; when nobody near the party has points
/// left all actors are refilled and the clock advances one minute ([`clock::tick`]). The pick
/// may be swallowed ([`swallowed`]); an NPC (not worktype 1 or 2) runs [`npc::npc_turn`], then
/// the bookkeeping again. Returns when the pick is the player's character (worktype 2, not
/// swallowed), after making it the leader if it is another party member.
pub async fn world_turn(state: &RefCell<State>, io: &Io) {
    let mask = {
        let mut s = state.borrow_mut();
        leader_bookkeeping(&mut s);
        formation(&mut s);
        go_to_schedules(&mut s);
        let leader = member(&s, usize::from(s.actors.party.leader_index));
        if (2..0x10).contains(&s.actors.worktype[leader]) {
            s.actors.attrs.unknown_18f1[leader] = leader as u8;
        }
        if s.map.centre.z == 0 { 0x3ff } else { 0xff }
    };
    loop {
        let mut pick = Pick {
            actor: -1,
            mp: 0,
            dex: 1,
        };
        let mut idle = 0u32;
        loop {
            {
                let mut s = state.borrow_mut();
                pick_actor(&mut s, mask, &mut pick);
                if pick.mp > 0 {
                    break;
                }
                refill(&mut s);
                clock::tick(&mut s, 1);
            }
            idle += 1;
            if idle.is_multiple_of(IDLE_REFILLS_PER_YIELD) {
                io.vblank().await;
            }
        }
        let actor = pick.actor as u16;
        let a = usize::from(actor);
        let held = swallowed(&mut state.borrow_mut(), actor);
        if !held {
            let wt = state.borrow().actors.worktype[a];
            if wt != 2 && wt != 1 {
                npc::npc_turn(state, io, actor).await;
                let mut s = state.borrow_mut();
                leader_bookkeeping(&mut s);
                if (2..0x10).contains(&s.actors.worktype[a]) {
                    s.actors.attrs.unknown_18f1[a] = actor as u8;
                }
            }
            seams::after_turn_refresh(&mut state.borrow_mut());
        }
        let mut s = state.borrow_mut();
        if s.actors.worktype[a] == 2 && s.actors.attrs.movement_flags[a] & 0x10 == 0 {
            let mut chosen = actor;
            if s.actors.party.leader_index == s.actors.party.size {
                chosen = 0;
            }
            let leader = member(&s, usize::from(s.actors.party.leader_index));
            if usize::from(chosen) != leader {
                for i in 0..usize::from(s.actors.party.size) {
                    if member(&s, i) == usize::from(chosen) {
                        seams::switch_leader(&mut s, i as u8);
                        s.actors.work.prompt = 1;
                        break;
                    }
                }
            }
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use u6_formats::position::Position;

    use super::*;
    use crate::engine::Engine;
    use crate::files::Files;

    /// Leader actor 1 (player, worktype 2) at (20, 20) and NPCs 10, 11, 12 next to it, with
    /// the given points and dexterities.
    fn world(npcs: &[(i8, u8)]) -> State {
        let mut s = State::empty();
        s.actors.party.members[0] = 1;
        s.actors.party.size = 1;
        s.actors.set_tail(tail::SOLO_MEMBER, 0xff);
        let at = Position { x: 20, y: 20, z: 0 };
        s.map.centre = at;
        s.objects.location[1] = at;
        s.objects.type_frame[1] = 0x17a;
        s.actors.worktype[1] = 2;
        s.actors.attrs.status[1] = 0x80;
        s.actors.attrs.dexterity[1] = 10;
        s.actors.movement_points[1] = 10;
        for (i, &(mp, dex)) in npcs.iter().enumerate() {
            let a = 10 + i;
            s.objects.location[a] = Position {
                x: 22 + i as u16,
                y: 20,
                z: 0,
            };
            s.objects.type_frame[a] = 0x17a;
            s.actors.worktype[a] = 0x80 + 0x10; // 0x90: loiter
            s.actors.attrs.dexterity[a] = dex;
            s.actors.movement_points[a] = mp as u8;
        }
        s
    }

    fn pick(s: &mut State) -> i16 {
        let mut p = Pick {
            actor: -1,
            mp: 0,
            dex: 1,
        };
        pick_actor(s, 0x3ff, &mut p);
        p.actor
    }

    #[test]
    fn the_actor_with_the_best_points_to_dexterity_ratio_goes_first() {
        // Leader 10/10 = full: taken at once (it comes first in slot order).
        let mut s = world(&[(5, 20), (9, 10), (3, 3)]);
        assert_eq!(pick(&mut s), 1);
        // Leader spent: actor 10 has 5/20, 11 has 9/10, 12 has 3/3 (full: best ratio).
        s.actors.movement_points[1] = 0;
        assert_eq!(pick(&mut s), 12, "3/3 beats 9/10 and 5/20");
        s.actors.movement_points[12] = 0;
        assert_eq!(pick(&mut s), 11, "9/10 beats 5/20");
        s.actors.movement_points[11] = 2;
        assert_eq!(pick(&mut s), 10, "5/20 beats 2/10");
        // Equal ratios: the higher dexterity wins.
        s.actors.movement_points[10] = 2;
        s.actors.attrs.dexterity[10] = 20;
        s.actors.movement_points[11] = 1;
        assert_eq!(pick(&mut s), 10, "2/20 = 1/10, dexterity 20 > 10");
        // Nobody has points: no pick.
        s.actors.movement_points[10] = 0;
        s.actors.movement_points[11] = 0;
        assert_eq!(pick(&mut s), -1);
    }

    #[test]
    fn actors_outside_the_window_or_unable_lose_their_turn() {
        let mut s = world(&[(5, 10), (5, 10), (5, 10)]);
        s.actors.movement_points[1] = 0;
        s.objects.location[10].x = 200;
        s.actors.attrs.status[11] = 0x02; // paralysed
        assert_eq!(pick(&mut s), 12);
        assert_eq!(
            s.actors.movement_points[10], 0,
            "outside the window: points gone"
        );
        assert_eq!(s.actors.movement_points[11], 5, "paralysed: just skipped");
    }

    #[test]
    fn refill_carries_debt() {
        let mut s = world(&[(-4, 10), (3, 10)]);
        refill(&mut s);
        assert_eq!(s.actors.mp(10), 6);
        assert_eq!(s.actors.mp(11), 10);
    }

    #[test]
    fn world_turn_runs_npcs_then_returns_to_the_player() {
        let mut s = world(&[(6, 10), (6, 10)]);
        s.actors.movement_points[1] = 0;
        s.actors.calendar.minute = 30;
        let rounds = Rc::new(std::cell::Cell::new(0));
        let r = Rc::clone(&rounds);
        let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
            world_turn(&st, &io).await;
            r.set(r.get() + 1);
        });
        for _ in 0..10 {
            e.tick(&[], 16);
        }
        assert!(e.finished());
        assert_eq!(rounds.get(), 1);
        let s = e.state();
        // The NPCs spent their points, the round ended, all were refilled and a minute passed.
        assert_eq!(s.actors.calendar.minute, 31);
        assert!(s.actors.mp(1) > 0, "the player has points again");
    }

    #[test]
    fn a_dead_avatar_brings_the_darkness_once_per_bookkeeping() {
        let mut s = world(&[]);
        s.actors.attrs.status[1] = 0x90;
        s.gfx.window_shown = 0;
        leader_bookkeeping(&mut s);
        assert_eq!(s.gfx.window_shown, 1);
    }

    #[test]
    fn member_zero_takes_the_lead_back() {
        let mut s = world(&[]);
        s.actors.party.members[1] = 2;
        s.actors.party.size = 2;
        s.objects.type_frame[2] = 0x17a;
        s.objects.location[2] = Position { x: 30, y: 31, z: 0 };
        s.actors.party.leader_index = 1;
        s.actors.worktype[1] = 1;
        s.actors.worktype[2] = 2;
        leader_bookkeeping(&mut s);
        assert_eq!(s.actors.party.leader_index, 0);
        assert_eq!((s.actors.worktype[1], s.actors.worktype[2]), (2, 1));
        assert_eq!((s.map.centre.x, s.map.centre.y), (20, 20));
    }

    #[test]
    fn swallowed_actor_breaks_free_or_suffers() {
        let mut s = world(&[(5, 10)]);
        s.actors.attrs.movement_flags[10] = 0x10;
        s.actors.attrs.strength[10] = 31;
        s.actors.attrs.hp[10] = 20;
        assert!(!swallowed(&mut s, 10), "strength 31 always beats 1..30");
        assert_eq!(s.actors.attrs.movement_flags[10] & 0x10, 0);
        assert_eq!(s.map.view_dirty, 1);
        s.actors.attrs.movement_flags[10] = 0x10;
        s.actors.attrs.strength[10] = 1;
        assert!(swallowed(&mut s, 10));
        assert_eq!(s.actors.movement_points[10], 0);
        assert!(s.actors.attrs.hp[10] < 20);
    }

    #[test]
    fn formation_centres() {
        let mut s = world(&[(5, 10)]);
        s.actors.party.members[1] = 2;
        s.actors.party.size = 2;
        s.objects.location[2] = Position { x: 23, y: 25, z: 0 };
        // Hostile actor 10 (alignment 0x20) at (22, 20).
        s.actors.attrs.status[10] = 0x20;
        formation(&mut s);
        // (20 + 23 + 1) / 2 = 22, (20 + 25 + 1) / 2 = 23.
        assert_eq!(s.actors.work.party_centre, (22, 23));
        assert_eq!(s.actors.work.enemy_centre, (22, 20));
        assert_eq!(s.actors.work.enemies, 1);
    }
}

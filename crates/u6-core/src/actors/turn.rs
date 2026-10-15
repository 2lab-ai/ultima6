//! The world turn: the actor scheduler 0x1213de and the per-turn party bookkeeping around it.

use std::cell::RefCell;

use crate::engine::Io;
use crate::state::State;

use super::seams::{self, Arg};
use super::{clock, leaves, npc, path, tail, timer, walk};

/// Refills (clock minutes) without anyone able to act after which the scheduler yields to the
/// host once (port checkpoint, spec 03 §1: a yield changes no game state).
const IDLE_REFILLS_PER_YIELD: u32 = 60;
/// [replaced] Engine milliseconds of the refresh after a turn (0x102f2a with the view changed:
/// the view flags 0x109e74, then 0x102ef0 draws the window and copies it to the screen): 427 ms,
/// 21.336 PAL frames, the median of 73 such calls traced in the oracle (19.86..23.98: the start
/// fight and the castle without it, 7 steps each, two runs of each setup; tools/oracle/trace.mjs
/// on the CPU clock, the reduction tools/oracle/trace.py). With the view unchanged the call
/// returns at once. The original has no wait there: that is the 68000's drawing time, which the
/// port, drawing at once, waits on the engine clock instead.
pub const AFTER_TURN_MS: u16 = 427;
/// [replaced] Engine milliseconds of one pick of the scheduler 0x1213de (its pass over the 256
/// actors, the refills and clock minutes, the formation and schedule passes, spread over the
/// picks): 20 ms, 0.994 PAL frames, the world turn's time outside its NPC turns and refreshes
/// over its picks (each NPC turn and the player's): 359 picks traced in the same runs.
pub const PICK_MS: u16 = 20;
/// [replaced] Engine milliseconds of an NPC turn that runs a combat worktype (3 to 0x7f: the
/// party's combat modes, assault; [`turn_worktype`]) besides what it draws, prints and names:
/// 46 ms, 2.278 PAL frames, the mean of 87 traced in the start fight's two runs (0.89..4.04). A
/// turn in a schedule worktype (0x80 and up) takes 0.033 frames (n = 244) and waits nothing.
pub const COMBAT_NPC_MS: u16 = 46;

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

/// Original: 0x121020 (actors: the hero has died: the darkness lines)
///
/// Unless the leader index is 16: music off, the two lines with a full redraw between, then
/// the resurrection 0x1095bc.
///
/// Port: 0x1095bc ([`crate::map::travel::resurrect_party`]) waits (the rejoin prints), and
/// this runs inside synchronous code (the bookkeeping, a killing blow in 0x1231be), so it only
/// marks the resurrection pending; [`world_turn`] awaits it at its next bookkeeping point,
/// before it picks an actor again. The rest of a killing blow therefore runs before the raise.
/// While it is pending the lines are not printed again (the original's raise has cleared the
/// death by then).
pub fn darkness(s: &mut State) {
    if s.actors.party.leader_index == 0x10 || s.actors.work.resurrect_pending {
        return;
    }
    crate::sound::play_song(s, 0);
    seams::print(s, -0x7435, &[]);
    s.gfx.window_shown = 1;
    seams::full_redraw(s);
    seams::print(s, -0x740e, &[]);
    s.actors.work.resurrect_pending = true;
}

/// The resurrection [`darkness`] left pending, awaited (0x1095bc).
async fn raise_if_dead(state: &RefCell<State>, io: &Io) {
    let pending = std::mem::take(&mut state.borrow_mut().actors.work.resurrect_pending);
    if pending {
        crate::map::travel::resurrect_party(state, io).await;
    }
}

/// Original: 0x121056 (party: leader and solo-mode bookkeeping per turn)
///
/// With the leader index at the party size (a vehicle carries the party) actor 0 is cleaned
/// up. A dead hero (actor 1) ends in [`darkness`]. In solo mode a leader who cannot act ends
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
        for i in 0..s.actors.party.count() {
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
    let size = s.actors.party.count();
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
/// Only actors with movement flag 0x10. The player's own character cries out (A4-0x75e1). On
/// `random(1, 30)` below its strength it is freed (its name and A4-0x75d3, redraw requested);
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
    for i in 1..s.actors.party.count() {
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

/// What the port waits at the pick of `actor` ([`PICK_MS`]), with [`COMBAT_NPC_MS`] more when
/// an NPC turn (`npc`) runs a combat worktype: in a time stop, or on another level than the
/// party, 0x1200b0 ends the turn at once (0x1200e8..0x120112, [`npc::npc_turn_sync`]) and no
/// worktype takes its time. The worktype is the one the turn runs ([`turn_worktype`]), not the
/// one the pick saw.
fn pick_ms(s: &State, actor: usize, npc: bool) -> u16 {
    let runs =
        s.actors.timers[timer::TIME_STOP] == 0 && s.objects.location[actor].z == s.map.centre.z;
    let fights = npc && runs && (3..0x80).contains(&turn_worktype(s, actor));
    PICK_MS + if fights { COMBAT_NPC_MS } else { 0 }
}

/// The worktype 0x1200b0 runs for `actor`. Its first test (0x1200ba..0x1200d4), before it saves
/// the points and before the time-stop and level tests: a party member (status bit 0x80) who is
/// not charmed (object status 0x04) is uncharmed (0x121bce through the thunk 0x121988,
/// [`super::leaves::uncharm`]), which gives it the party's worktype back: its combat mode in
/// combat, else 1 (follow) in party mode or 0 in solo mode. Anyone else runs its own. Below 3
/// (0 wait, 1 follow, 2 the player's character's, also the first combat mode a member can be
/// set to: 0x1061de cycles them 2..8) the turn only spends its points (the default arm) and
/// waits nothing; no traced turn began below 3.
fn turn_worktype(s: &State, actor: usize) -> u8 {
    let member = s.actors.attrs.status[actor] & 0x80 != 0;
    if !member || s.objects.obj_status[actor] & 0x04 != 0 {
        s.actors.worktype[actor]
    } else if s.actors.combat_mode() {
        s.actors.attrs.combat_mode[actor]
    } else if s.actors.solo_member() < 0 {
        1
    } else {
        0
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
///
/// Port: the original draws as it goes and is as slow as that drawing; the port waits the
/// measured time on the engine clock instead, at each pick ([`pick_ms`]: [`PICK_MS`], and
/// [`COMBAT_NPC_MS`] before an NPC turn in a combat worktype that runs) and before each refresh
/// of a changed view ([`AFTER_TURN_MS`]), so the old picture stays up meanwhile.
pub async fn world_turn(state: &RefCell<State>, io: &Io) {
    leader_bookkeeping(&mut state.borrow_mut());
    raise_if_dead(state, io).await;
    let mask = {
        let mut s = state.borrow_mut();
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
        // A death in the clock's ticks above (a hazard tile, 0x1032b6) left the raise pending.
        raise_if_dead(state, io).await;
        let actor = pick.actor as u16;
        let a = usize::from(actor);
        let held = swallowed(&mut state.borrow_mut(), actor);
        let wt = state.borrow().actors.worktype[a];
        let npc = !held && wt != 2 && wt != 1;
        let ms = pick_ms(&state.borrow(), a, npc);
        crate::combat::present::pace(state, io, ms).await;
        if !held {
            if npc {
                npc::npc_turn(state, io, actor).await;
                leader_bookkeeping(&mut state.borrow_mut());
                raise_if_dead(state, io).await;
                let mut s = state.borrow_mut();
                if (2..0x10).contains(&s.actors.worktype[a]) {
                    s.actors.attrs.unknown_18f1[a] = actor as u8;
                }
            }
            if state.borrow().map.view_dirty != 0 {
                crate::combat::present::pace(state, io, AFTER_TURN_MS).await;
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
                for i in 0..s.actors.party.count() {
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
        // Two NPC turns and the refreshes paced on the engine clock (well under 2 s).
        for _ in 0..125 {
            e.tick(&[], 16);
        }
        assert!(e.finished());
        assert_eq!(rounds.get(), 1);
        let s = e.state();
        // The NPCs spent their points, the round ended, all were refilled and a minute passed.
        assert_eq!(s.actors.calendar.minute, 31);
        assert!(s.actors.mp(1) > 0, "the player has points again");
    }

    /// The engine time one world turn of `world(npcs)` takes at host ticks of `tick_ms` (a poll
    /// with no time elapsed after each when `zero_polls`), the view changed before it, and the
    /// NPC turns it ran.
    fn turn_ms(npcs: &[(i8, u8)], tick_ms: u32, zero_polls: bool) -> (u64, u32) {
        let mut s = world(npcs);
        s.actors.movement_points[1] = 0;
        s.map.view_dirty = 1;
        let done = Rc::new(std::cell::Cell::new(None));
        let out = Rc::clone(&done);
        let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
            let t0 = io.now_ms();
            world_turn(&st, &io).await;
            let n = st.borrow().combat.paced.get(&PICK_MS).copied();
            out.set(Some((io.now_ms() - t0, n.unwrap_or(0))));
        });
        for _ in 0..10_000 {
            if done.get().is_some() {
                break;
            }
            e.tick(&[], tick_ms);
            if zero_polls {
                e.tick(&[], 0);
            }
        }
        done.get().expect("the turn ends")
    }

    #[test]
    fn only_a_combat_npc_turn_that_runs_takes_combat_turn_time() {
        // NPC 10 in a combat worktype (3) on the party's level.
        let mut s = world(&[(6, 10)]);
        s.actors.worktype[10] = 3;
        let fights = PICK_MS + COMBAT_NPC_MS;
        assert_eq!(pick_ms(&s, 10, true), fights, "the turn runs");
        assert_eq!(pick_ms(&s, 10, false), PICK_MS, "not an NPC turn");
        s.actors.worktype[10] = 0x90;
        assert_eq!(pick_ms(&s, 10, true), PICK_MS, "a schedule worktype");
        s.actors.worktype[10] = 3;
        s.actors.timers[timer::TIME_STOP] = 5;
        assert_eq!(pick_ms(&s, 10, true), PICK_MS, "time stop");
        s.actors.timers[timer::TIME_STOP] = 0;
        s.objects.location[10].z = 1;
        assert_eq!(pick_ms(&s, 10, true), PICK_MS, "another level");
    }

    #[test]
    fn a_party_members_turn_is_charged_by_the_worktype_it_runs() {
        // NPC 10 is a party member (status 0x80) picked in worktype 8, on the party's level, no
        // time stop. 0x1200b0 first uncharms a member who is not charmed (0x1200ba..0x1200d4),
        // which resets its worktype, and then runs that one.
        let fights = PICK_MS + COMBAT_NPC_MS;
        let mut s = world(&[(6, 10)]);
        s.actors.attrs.status[10] = 0x80;
        s.actors.worktype[10] = 8;
        // Charmed (object status 0x04): not uncharmed, its turn runs the charm's assault.
        s.objects.obj_status[10] = 0x04;
        assert_eq!(pick_ms(&s, 10, true), fights, "charmed: worktype 8 runs");
        // The charm is over but the worktype is still the charm's: out of combat in party mode
        // it follows (1), which only spends its points.
        s.objects.obj_status[10] = 0;
        assert_eq!(
            pick_ms(&s, 10, true),
            PICK_MS,
            "uncharmed to 1 out of combat"
        );
        // In combat it runs its combat mode: front (3) fights, mode 2 is the default arm.
        s.actors.set_tail(tail::COMBAT_MODE, 1);
        s.actors.attrs.combat_mode[10] = 3;
        s.actors.worktype[10] = 0x90;
        assert_eq!(
            pick_ms(&s, 10, true),
            fights,
            "a schedule worktype uncharmed to 3"
        );
        s.actors.attrs.combat_mode[10] = 2;
        s.actors.worktype[10] = 8;
        assert_eq!(pick_ms(&s, 10, true), PICK_MS, "uncharmed to 2 in combat");
        // Not a member: its own worktype, whatever its charm.
        s.actors.attrs.status[10] = 0;
        assert_eq!(pick_ms(&s, 10, true), fights, "an NPC in worktype 8");
    }

    #[test]
    fn the_worktype_a_members_turn_runs_is_the_one_its_uncharm_leaves() {
        // turn_worktype restates 0x121bce's worktype rule; the uncharm itself must agree.
        for (combat, solo, mode) in [
            (false, 0xff, 3),
            (false, 0, 3),
            (true, 0xff, 3),
            (true, 0, 2),
        ] {
            let mut s = world(&[(6, 10)]);
            s.actors.attrs.status[10] = 0x80;
            s.actors.worktype[10] = 8;
            s.actors.attrs.combat_mode[10] = mode;
            s.actors.set_tail(tail::COMBAT_MODE, u8::from(combat));
            s.actors.set_tail(tail::SOLO_MEMBER, solo);
            let mut t = s.clone();
            crate::actors::leaves::uncharm(&mut t, 10);
            assert_eq!(
                turn_worktype(&s, 10),
                t.actors.worktype[10],
                "combat {combat}, solo {solo:#x}, mode {mode}"
            );
        }
    }

    #[test]
    fn a_members_turn_after_its_charm_takes_no_combat_turn_time() {
        // Member 10 (status 0x80, in the party list) still in the charm's worktype 8, its
        // charm over (no object status 0x04), out of combat: its turn runs worktype 1 (party
        // mode), which spends 5 of its 5 points. Two picks (it and the player) and the refresh
        // of the view the turn started with changed; no combat NPC turn.
        let mut s = world(&[(5, 10)]);
        s.actors.party.members[1] = 10;
        s.actors.party.size = 2;
        s.actors.attrs.status[10] = 0x80;
        s.actors.worktype[10] = 8;
        s.actors.movement_points[1] = 0;
        s.map.view_dirty = 1;
        let want = 2 * u64::from(PICK_MS) + u64::from(AFTER_TURN_MS);
        for (tick, zero) in [(16, false), (7, false), (16, true)] {
            let done = Rc::new(std::cell::Cell::new(None));
            let out = Rc::clone(&done);
            let mut e = Engine::with_game(s.clone(), Files::default(), move |st, io| async move {
                let t0 = io.now_ms();
                world_turn(&st, &io).await;
                out.set(Some(io.now_ms() - t0));
            });
            for _ in 0..10_000 {
                if done.get().is_some() {
                    break;
                }
                e.tick(&[], tick);
                if zero {
                    e.tick(&[], 0);
                }
            }
            let ms = done.get().expect("the turn ends");
            assert_eq!(e.state().actors.worktype[10], 1, "the turn uncharmed it");
            assert!(
                ms >= want && ms <= want + u64::from(tick),
                "{ms} ms at {tick} ms ticks (zero polls {zero}), want {want}"
            );
        }
    }

    #[test]
    fn a_world_turn_waits_the_originals_time_at_any_host_tick() {
        // Spec 03 §1: the turn's waits (an NPC turn each, the refresh of the changed view) run
        // on the engine clock: the same time at 16 ms and 8 ms ticks and with polls of no time.
        let (ms, npcs) = turn_ms(&[(6, 10), (6, 10)], 16, false);
        println!("two NPCs: {npcs} NPC turns, {ms} ms");
        assert!(npcs >= 2, "both acted: {npcs}");
        let least = u64::from(npcs) * u64::from(PICK_MS) + u64::from(AFTER_TURN_MS);
        assert!(ms >= least, "{ms} ms, at least {least}");
        for (tick, zero) in [(8, false), (16, true), (5, true)] {
            let (other, n) = turn_ms(&[(6, 10), (6, 10)], tick, zero);
            assert_eq!(n, npcs, "the same turns");
            // Each run ends within its own tick of the same deadline.
            assert!(
                other + 16 >= ms && other <= ms + u64::from(tick),
                "16 ms ticks {ms}, {tick} ms {other}"
            );
        }
    }

    #[test]
    fn a_dead_hero_brings_the_darkness_once_per_bookkeeping() {
        let mut s = world(&[]);
        s.actors.attrs.status[1] = 0x90;
        s.gfx.window_shown = 0;
        leader_bookkeeping(&mut s);
        assert_eq!(s.gfx.window_shown, 1);
    }

    #[test]
    fn a_dead_hero_is_raised_before_the_world_turn_picks_again() {
        let mut s = world(&[]);
        s.actors.attrs.status[1] = 0x90;
        s.actors.calendar.hour = 9;
        let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
            world_turn(&st, &io).await;
        });
        e.tick(&[], 16);
        let s = e.state();
        // The raise ran (bits reset, the view moved to the castle) before any pick; the
        // hero of this bare world is not carried along, so the turn then idles.
        assert!(!s.actors.work.resurrect_pending);
        assert_eq!(s.actors.attrs.status[1] & 0x10, 0, "the hero lives again");
        assert_eq!((s.map.centre.x, s.map.centre.y), (0x133, 0x160));
    }

    #[test]
    fn a_pending_resurrection_is_not_announced_twice() {
        let mut s = world(&[]);
        s.actors.attrs.status[1] = 0x90;
        leader_bookkeeping(&mut s);
        assert!(s.actors.work.resurrect_pending);
        s.gfx.window_shown = 0;
        leader_bookkeeping(&mut s);
        assert_eq!(s.gfx.window_shown, 0, "the darkness lines once per death");
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

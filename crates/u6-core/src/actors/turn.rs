//! The world turn: the actor scheduler 0x1213de and the per-turn party bookkeeping around it.

use std::cell::RefCell;

use crate::engine::Io;
use crate::state::State;

use super::seams::{self, Arg};
use super::{clock, leaves, npc, path, tail, timer, walk};

/// [replaced] Engine milliseconds of the refresh after a turn (0x102f2a with the view changed:
/// the view flags 0x109e74, then 0x102ef0 draws the window and copies it to the screen): 427 ms,
/// 21.336 PAL frames, the median of 73 such calls traced in the oracle (19.86..23.98: the start
/// fight and the castle without it, 7 steps each, two runs of each setup; tools/oracle/trace.mjs
/// on the CPU clock, the reduction tools/oracle/trace.py). With the view unchanged the call
/// returns at once. The original has no wait there: that is the 68000's drawing time, which the
/// port, drawing at once, waits on the engine clock instead.
pub const AFTER_TURN_MS: u16 = 427;
/// [replaced] Engine milliseconds of the world turn's setup before its first pick (0x1213de up to
/// the return of 0x120a1c: the bookkeeping 0x121056, the party centre 0x120b64, the schedules
/// 0x120a1c), once per world turn: 16 ms, 0.811 PAL frames, the mean of 33 traced (0.56..1.23;
/// the start fight and the castle without it, two runs each, and the castle's blocked steps;
/// tools/oracle/trace.mjs on the CPU clock, tools/oracle/trace.py "world turn setup"). The start
/// fight's first world turn after the boot took 8.71 in both runs (0x120a1c alone 8.08): a
/// one-off, not paid.
pub const SETUP_MS: u16 = 16;
/// [replaced] Engine milliseconds of one pass of the scheduler 0x1213de over the 256 actors (a
/// pick's, each NPC turn's and the player's, and the one after a refill; with the refill and the
/// bookkeeping after an NPC turn): 11 ms, 0.531 PAL frames, the world turn's time outside its
/// setup, NPC turns, refreshes and clock minutes over its 386 passes traced in the same runs
/// (tools/oracle/trace.py "scheduler pass"). Before the setup and the minutes were traced apart
/// a pick carried them: 0.994 frames (359 picks, r4 and r5), 0.990 in these runs.
pub const PICK_MS: u16 = 11;
/// [replaced] Engine milliseconds of a clock minute in the world turn (0x10368a with 1, through
/// 0x1219b8, after the refill when no one near has points left: [`clock::tick`]) besides the
/// date line it prints: 186 ms, 9.306 PAL frames, the mean of the 12 traced in the same runs
/// (8.86..9.76; every other step of the castle and of the start fight). The port waits it with
/// the pass the refill begins ([`PICK_MS`]). In a time stop the minute only counts the stop down
/// and returns at its first test: not traced, the pass alone is waited.
pub const MINUTE_MS: u16 = 186;
/// [replaced] Engine milliseconds of an NPC turn that runs a combat routine (the worktype the
/// turn runs, [`turn_worktype`], in [`npc::runs_combat_routine`]: the party's combat modes,
/// assault, retreat) besides what it draws, prints and names: 46 ms, 2.278 PAL frames, the mean
/// of 87 traced in the start fight's two runs (0.89..4.04), every one in a combat routine. The
/// 244 other turns traced (all in schedule worktypes, 0x80 and up, none in 0x9a or 0x9b) took
/// 0.033 frames: any turn outside a combat routine waits nothing.
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

/// The resurrection [`darkness`] left pending, awaited (0x1095bc), after what the world turn
/// `owed` till then.
async fn raise_if_dead(state: &RefCell<State>, io: &Io, owed: &mut u32) {
    let pending = std::mem::take(&mut state.borrow_mut().actors.work.resurrect_pending);
    if pending {
        pay(state, io, owed, 0).await;
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

/// Whether the pick of `actor` takes [`COMBAT_NPC_MS`] besides its pass ([`PICK_MS`]): an NPC
/// turn (`npc`) that runs a combat routine. In a time stop, or on another level than the party,
/// 0x1200b0 ends the turn at once (0x1200e8..0x120112, [`npc::npc_turn_sync`]) and no worktype
/// takes its time. The worktype is the one the turn runs ([`turn_worktype`]), not the one the
/// pick saw.
fn fights(s: &State, actor: usize, npc: bool) -> bool {
    let runs =
        s.actors.timers[timer::TIME_STOP] == 0 && s.objects.location[actor].z == s.map.centre.z;
    npc && runs && npc::runs_combat_routine(turn_worktype(s, actor))
}

/// The worktype 0x1200b0 runs for `actor`. Its first test (0x1200ba..0x1200d4), before it saves
/// the points and before the time-stop and level tests: a party member (status bit 0x80) who is
/// not charmed (object status 0x04) is uncharmed (0x121bce through the thunk 0x121988,
/// [`super::leaves::uncharm`]), which gives it the party's worktype back: its combat mode in
/// combat, else 1 (follow) in party mode or 0 in solo mode. Anyone else runs its own. Below 3
/// (0 wait, 1 follow, 2 the player's character's, also the first combat mode a member can be
/// set to: 0x1061de cycles them 2..8) the turn only spends its points (the default arm), no
/// combat routine; no traced turn began below 3.
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
/// measured time on the engine clock instead: the setup once ([`SETUP_MS`]), each pick
/// ([`PICK_MS`], and [`COMBAT_NPC_MS`] for an NPC turn in a combat worktype that runs,
/// [`fights`]), each refill and clock minute ([`PICK_MS`] + [`MINUTE_MS`]) and each refresh of a
/// changed view ([`AFTER_TURN_MS`]), so the old picture stays up meanwhile. A pick is shorter
/// than a host frame, and [`crate::combat::present::pace`] waits at least one (so a run of them
/// would end later at a coarser tick), so picks are owed and paid with the next wait of the
/// turn: before a combat NPC turn, first in the playback of an NPC turn that shows something
/// (frames or lines it queued, an arrest, a conversation: [`owe_first`], so what its
/// synchronous part drew stays off the screen meanwhile), at a refill, before a refresh, before
/// a raise of the dead party, or at the end of the turn. So whatever the turn shows comes after
/// every pass before it, as in the original. The setup is paid with the end of the turn: that last wait is then
/// never shorter than a frame and makes up what the waits before it ended late, while the
/// turn's pictures come at most the setup's 16 ms (under a frame) early. In a time stop the
/// refills wait nothing (the minute only counts the stop down), so the passes of up to 255
/// refills of up to 256 picks stay owed: hence a u32.
pub async fn world_turn(state: &RefCell<State>, io: &Io) {
    let mut owed: u32 = 0;
    leader_bookkeeping(&mut state.borrow_mut());
    raise_if_dead(state, io, &mut owed).await;
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
        loop {
            let minute = {
                let mut s = state.borrow_mut();
                pick_actor(&mut s, mask, &mut pick);
                if pick.mp > 0 {
                    break;
                }
                refill(&mut s);
                // While a time stop runs past it the minute only counts it down: no minute's
                // time, and the pass stays owed (the stop ends within its count of refills).
                let stopped = s.actors.timers[timer::TIME_STOP] > 1;
                clock::tick(&mut s, 1);
                owed += u32::from(PICK_MS);
                !stopped
            };
            if minute {
                pay(state, io, &mut owed, MINUTE_MS).await;
            }
        }
        // A death in the clock's ticks above (a hazard tile, 0x1032b6) left the raise pending.
        raise_if_dead(state, io, &mut owed).await;
        let actor = pick.actor as u16;
        let a = usize::from(actor);
        let held = swallowed(&mut state.borrow_mut(), actor);
        let wt = state.borrow().actors.worktype[a];
        let npc = !held && wt != 2 && wt != 1;
        owed += u32::from(PICK_MS);
        if fights(&state.borrow(), a, npc) {
            pay(state, io, &mut owed, COMBAT_NPC_MS).await;
        }
        if !held {
            if npc {
                let started = npc::npc_turn_sync(&mut state.borrow_mut(), actor);
                if shows(&state.borrow(), started.as_ref()) {
                    owe_first(&mut state.borrow_mut(), &mut owed);
                }
                npc::npc_turn_rest(state, io, actor, started).await;
                leader_bookkeeping(&mut state.borrow_mut());
                raise_if_dead(state, io, &mut owed).await;
                let mut s = state.borrow_mut();
                if (2..0x10).contains(&s.actors.worktype[a]) {
                    s.actors.attrs.unknown_18f1[a] = actor as u8;
                }
            }
            if state.borrow().map.view_dirty != 0 {
                pay(state, io, &mut owed, AFTER_TURN_MS).await;
            }
            seams::after_turn_refresh(&mut state.borrow_mut());
        }
        if leader_picked(&mut state.borrow_mut(), actor) {
            pay(state, io, &mut owed, SETUP_MS).await;
            return;
        }
    }
}

/// [`world_turn`]'s waits: what is `owed` and `more`, in one wait (none when both are 0; in
/// waits of at most `u16::MAX` ms each when longer, one schedule, so the same total).
async fn pay(state: &RefCell<State>, io: &Io, owed: &mut u32, more: u16) {
    let mut ms = std::mem::take(owed) + u32::from(more);
    while ms > 0 {
        let part = ms.min(u32::from(u16::MAX)) as u16;
        crate::combat::present::pace(state, io, part).await;
        ms -= u32::from(part);
    }
}

/// What the world turn `owed`, put at the front of the NPC turn's playback (pauses: recorded
/// effects of no picture, at most `u16::MAX` ms each, before any line): [`npc::npc_turn_rest`]'s
/// playback waits them first with the window as last published put back, so a picture the
/// turn's synchronous part drew and presented (a guard's strike renders its flash) is not
/// published before the passes and its own time, as a wait between the two parts would.
fn owe_first(s: &mut State, owed: &mut u32) {
    let mut ms = std::mem::take(owed);
    let mut pauses = Vec::new();
    while ms > 0 {
        let part = ms.min(u32::from(u16::MAX)) as u16;
        pauses.push(crate::combat::present::Effect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            pixels: Vec::new(),
            cost: part,
            mark: 0,
            dissolve: false,
        });
        ms -= u32::from(part);
    }
    s.combat.effects.splice(0..0, pauses);
}

/// Whether the NPC turn [`npc::npc_turn_sync`] started (`started`) shows something before it
/// ends: frames or lines queued (played by [`npc::npc_turn_rest`]), an arrest or a
/// conversation.
fn shows(s: &State, started: Option<&(i8, npc::Wait)>) -> bool {
    !s.combat.effects.is_empty()
        || !s.main.pending_print.is_empty()
        || matches!(started, Some((_, npc::Wait::Arrest | npc::Wait::Talk(_))))
}

/// The end of [`world_turn`]'s loop (0x121760..0x1217cc): true when the pick `actor` is the
/// player's character (worktype 2, not swallowed), made the leader if it is another member.
fn leader_picked(s: &mut State, actor: u16) -> bool {
    let a = usize::from(actor);
    if s.actors.worktype[a] != 2 || s.actors.attrs.movement_flags[a] & 0x10 != 0 {
        return false;
    }
    let mut chosen = actor;
    if s.actors.party.leader_index == s.actors.party.size {
        chosen = 0;
    }
    let leader = member(s, usize::from(s.actors.party.leader_index));
    if usize::from(chosen) != leader {
        for i in 0..s.actors.party.count() {
            if member(s, i) == usize::from(chosen) {
                seams::switch_leader(s, i as u8);
                s.actors.work.prompt = 1;
                break;
            }
        }
    }
    true
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

    /// What the port waits for the pick of `actor`: its pass, and the combat turn's time when
    /// it fights ([`fights`]).
    fn pick_ms(s: &State, actor: usize, npc: bool) -> u16 {
        PICK_MS
            + if fights(s, actor, npc) {
                COMBAT_NPC_MS
            } else {
                0
            }
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

    /// The engine time one world turn on `s` takes at host ticks of `tick_ms` (a poll with no
    /// time elapsed after each when `zero_polls`), and the state after it.
    fn turn_ms(s: State, tick_ms: u32, zero_polls: bool) -> (u64, State) {
        let done = Rc::new(std::cell::Cell::new(None));
        let out = Rc::clone(&done);
        let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
            let t0 = io.now_ms();
            world_turn(&st, &io).await;
            out.set(Some(io.now_ms() - t0));
        });
        for _ in 0..MAX_TICKS {
            if done.get().is_some() {
                break;
            }
            e.tick(&[], tick_ms);
            if zero_polls {
                e.tick(&[], 0);
            }
        }
        let ms = done.get().expect("the turn ends");
        (ms, e.state().clone())
    }

    /// [`turn_ms`] with the loop long enough for a turn of minutes.
    const MAX_TICKS: usize = 100_000;

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
    fn the_combat_charge_follows_the_routine_a_worktype_runs() {
        // 0x1200b0's dispatch (0x120184..0x120248): the combat routines run for 3..=11, 13, 14,
        // 0x13, 0x9a and 0x9b; 12 runs the wander of 0x8f, 15 and 16 the random walk of
        // 0x8b..0x8e, 0x12 the arrest, 0x11 the default arm.
        let fights = PICK_MS + COMBAT_NPC_MS;
        let mut s = world(&[(6, 10)]);
        for (wt, want) in [
            (15, PICK_MS),
            (0x9b, fights),
            (3, fights),
            (11, fights),
            (13, fights),
            (14, fights),
            (0x13, fights),
            (0x9a, fights),
            (12, PICK_MS),
            (16, PICK_MS),
            (0x11, PICK_MS),
            (0x12, PICK_MS),
            (0x8b, PICK_MS),
            (0x8f, PICK_MS),
        ] {
            s.actors.worktype[10] = wt;
            assert_eq!(pick_ms(&s, 10, true), want, "worktype {wt:#x}");
        }
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
        // mode), which spends 5 of its 5 points. The setup, three passes (its pick, the one after
        // the refill and the player's pick), one clock minute and the refresh of the view the
        // turn started with changed; no combat NPC turn.
        let mut s = world(&[(5, 10)]);
        s.actors.party.members[1] = 10;
        s.actors.party.size = 2;
        s.actors.attrs.status[10] = 0x80;
        s.actors.worktype[10] = 8;
        s.actors.movement_points[1] = 0;
        s.map.view_dirty = 1;
        s.actors.calendar.minute = 30;
        let want = u64::from(SETUP_MS)
            + 3 * u64::from(PICK_MS)
            + u64::from(MINUTE_MS)
            + u64::from(AFTER_TURN_MS);
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
            assert_eq!(e.state().actors.calendar.minute, 31, "one clock minute");
            assert!(
                ms >= want && ms <= want + u64::from(tick),
                "{ms} ms at {tick} ms ticks (zero polls {zero}), want {want}"
            );
        }
    }

    /// The cases of a world turn's time: the setup, then per pass, per clock minute; each at host
    /// ticks of 16 ms, 7 ms and 16 ms with a poll of no time after each, ending within a tick
    /// of the stated cost; the view unchanged, so no refresh.
    fn assert_turn_ms(name: &str, s: &State, want: u64, minutes: u8) {
        for (tick, zero) in [(16, false), (7, false), (16, true)] {
            let (ms, after) = turn_ms(s.clone(), tick, zero);
            let m = after
                .actors
                .calendar
                .minute
                .wrapping_sub(s.actors.calendar.minute);
            assert_eq!(m, minutes, "{name}: clock minutes");
            assert_eq!(after.map.view_dirty, 0, "{name}: no refresh");
            assert!(
                ms >= want && ms <= want + u64::from(tick),
                "{name}: {ms} ms at {tick} ms ticks (zero polls {zero}), want {want}"
            );
        }
    }

    #[test]
    fn a_one_pick_world_turn_takes_its_setup_and_one_pass() {
        // The player has its points (10 of dexterity 10): picked at once, as after a blocked
        // step. The setup and one pass (the oracle: 1.13..1.72 frames for such a turn, n 18).
        let s = world(&[(5, 10)]);
        let want = u64::from(SETUP_MS) + u64::from(PICK_MS);
        assert_turn_ms("one pick", &s, want, 0);
    }

    #[test]
    fn an_n_pick_world_turn_takes_its_setup_its_passes_and_its_minute() {
        // The player spent; NPCs 10 and 11 (5 of dexterity 10 each) in worktype 0x11, whose
        // turn only spends 5 points: each takes one turn, then no one has points, the refill
        // and a clock minute, then the player. The setup, three picks, and the minute with the
        // pass the refill begins.
        let mut s = world(&[(5, 10), (5, 10)]);
        s.actors.movement_points[1] = 0;
        s.actors.worktype[10] = 0x11;
        s.actors.worktype[11] = 0x11;
        s.actors.calendar.minute = 30;
        let want = u64::from(SETUP_MS) + 4 * u64::from(PICK_MS) + u64::from(MINUTE_MS);
        assert_turn_ms("two NPCs", &s, want, 1);
        // The same in a time stop: the NPC turns end at once and the minute only counts the
        // stop down (no minute's time).
        s.actors.timers[timer::TIME_STOP] = 5;
        let want = u64::from(SETUP_MS) + 4 * u64::from(PICK_MS);
        assert_turn_ms("two NPCs, time stopped", &s, want, 0);
    }

    #[test]
    fn an_idle_stretch_through_an_expiring_time_stop_waits_each_minute_that_passes() {
        // The player in debt (35 points owed, dexterity 10): refilled four times before it has
        // points (-25, -15, -5, 5), no one else. A time stop of 2: the first refill's minute
        // only counts it down (no minute), the second's ends it and passes in full, as the
        // third's and fourth's. The setup, five passes (four refills and the player's pick) and
        // three minutes.
        let mut s = world(&[]);
        s.actors.movement_points[1] = (-35i8) as u8;
        s.actors.timers[timer::TIME_STOP] = 2;
        s.actors.calendar.minute = 30;
        let want = u64::from(SETUP_MS) + 5 * u64::from(PICK_MS) + 3 * u64::from(MINUTE_MS);
        assert_turn_ms("the stop expiring", &s, want, 3);
        let (_, after) = turn_ms(s, 16, false);
        assert_eq!(after.actors.timers[timer::TIME_STOP], 0, "the stop is over");
    }

    #[test]
    fn a_long_time_stop_with_many_actors_near_pays_every_pass() {
        // 250 NPCs (dexterity 5, no points) near the player, who owes 128 points (dexterity 5):
        // 26 refills before it has points (-128 + 5 x 26 = 2), all in a time stop of 100, so no
        // minute passes and nothing is waited until the turn ends. After each refill every NPC
        // is full and is picked once (its turn ends at once in the stop); after the 26th they
        // come before the player (full, it is not). The setup and 26 x 250 + 26 + 1 passes:
        // more than a u16 of milliseconds.
        let mut s = world(&[]);
        s.actors.attrs.dexterity[1] = 5;
        s.actors.movement_points[1] = 0x80;
        s.actors.timers[timer::TIME_STOP] = 100;
        for i in 0..250u16 {
            let a = usize::from(2 + i);
            s.objects.location[a] = Position {
                x: 2 + i % 36,
                y: 2 + i / 36,
                z: 0,
            };
            s.objects.type_frame[a] = 0x17a;
            s.actors.worktype[a] = 0x90;
            s.actors.attrs.dexterity[a] = 5;
        }
        let passes = 26 * 250 + 26 + 1;
        let want = u64::from(SETUP_MS) + passes * u64::from(PICK_MS);
        assert!(want > u64::from(u16::MAX));
        assert_turn_ms("250 NPCs in a time stop", &s, want, 0);
        let (_, after) = turn_ms(s, 16, false);
        assert_eq!(
            after.actors.timers[timer::TIME_STOP],
            74,
            "26 refills counted it down"
        );
    }

    #[test]
    fn an_npc_turn_that_shows_something_comes_after_the_passes_before_it() {
        // Five NPCs (0x11: their turn only spends 5 points) and a guard (0x12) beside the
        // player, all full: picked in slot order, the guard sixth. The guard closes in and
        // asks for surrender (0x11fd20, the flag it needs set): a line and a key read. In the
        // original its turn comes after the six passes before it; the key (N) is read no
        // earlier than their 66 ms, and within a tick of them.
        let mut s = world(&[(10, 10); 5]);
        s.ui = crate::main_loop::test_support::test_ui();
        s.actors.movement_points[1] = 0;
        for a in 10..15 {
            s.actors.worktype[a] = 0x11;
        }
        let guard = 20;
        s.objects.location[guard] = Position { x: 21, y: 20, z: 0 };
        s.objects.type_frame[guard] = 0x17a;
        s.actors.worktype[guard] = 0x12;
        s.actors.attrs.dexterity[guard] = 10;
        s.actors.movement_points[guard] = 10;
        s.actors.attrs.talk_flags[5] |= 0x80;
        crate::main_loop::test_support::link_placed_objects(&mut s);
        let want = 6 * u64::from(PICK_MS);
        for (tick, zero) in [(16, false), (7, false), (16, true)] {
            let start = Rc::new(std::cell::Cell::new(None));
            let done = Rc::new(std::cell::Cell::new(false));
            let (st0, dn) = (Rc::clone(&start), Rc::clone(&done));
            let mut e = Engine::with_game(s.clone(), Files::default(), move |st, io| async move {
                st0.set(Some(io.now_ms()));
                world_turn(&st, &io).await;
                dn.set(true);
            });
            let mut read = None;
            for i in 0..MAX_TICKS {
                if done.get() {
                    break;
                }
                let key = [crate::engine::InputEvent::Key(crate::engine::Key::Char(
                    'N',
                ))];
                e.tick(if i == 0 { &key } else { &[] }, tick);
                if zero {
                    e.tick(&[], 0);
                }
                if read.is_none() && e.io().pending_events() == 0 {
                    read = Some(e.io().now_ms());
                }
            }
            assert!(done.get(), "the turn ends");
            assert_eq!(
                e.state().actors.worktype[guard],
                8,
                "N: the guard turned hostile"
            );
            let ms = read.expect("the key was read") - start.get().expect("the turn began");
            assert!(
                ms >= want && ms <= want + u64::from(tick),
                "the key read {ms} ms in at {tick} ms ticks (zero polls {zero}), want {want}"
            );
        }
    }

    /// The published map window (176 × 176 from the top left) of `e`.
    fn published_window(e: &Engine) -> Vec<u8> {
        e.io().published(|fb| {
            (0..176)
                .flat_map(|y| (0..176).map(move |x| (x, y)))
                .map(|(x, y)| fb.get(x, y).unwrap_or(0))
                .collect()
        })
    }

    #[test]
    fn a_guards_strike_after_owed_passes_shows_its_flash_only_after_them_and_its_time() {
        // A guard at its post (0x99, not a combat routine: no combat turn's wait before it)
        // beside the player, after five NPCs (0x11) that only spend points, each with one
        // turn's points (5 of dexterity 5, the guard too, so it acts once, sixth): six passes
        // owed when its turn strikes the player (0x1230ce: the window rendered and presented, then
        // recorded to show after its FLASH_MS). The window as published before the turn (a
        // marker) must stay up until the flash shows: no earlier than the passes and the
        // flash's time, within a tick. The post is the schedule's (0, 0) here, so the scene is
        // near it; a generator state whose coin strikes (`draws` draws on from the start) is
        // looked for first.
        let scene = |draws: u16| {
            let mut s = world(&[(5, 5); 5]);
            s.ui = crate::main_loop::test_support::test_ui();
            s.gfx.window_shown = 1;
            s.combat.flash = 1;
            let at = Position { x: 2, y: 2, z: 0 };
            s.map.centre = at;
            s.objects.location[1] = at;
            s.actors.movement_points[1] = 0;
            // In the 40 × 40 window around the player (else they lose their points), more than 8
            // tiles from the guard (else it might take one of them for its target).
            for (i, a) in (10..15).enumerate() {
                s.actors.worktype[a] = 0x11;
                s.objects.location[a] = Position {
                    x: 15 + i as u16,
                    y: 12,
                    z: 0,
                };
            }
            let guard = 20;
            s.objects.location[guard] = Position { x: 3, y: 2, z: 0 };
            s.objects.type_frame[guard] = 0x17a;
            s.actors.worktype[guard] = 0x99;
            s.actors.attrs.dexterity[guard] = 5;
            s.actors.movement_points[guard] = 5;
            crate::main_loop::test_support::link_placed_objects(&mut s);
            for _ in 0..draws {
                s.rng.range(0, 3);
            }
            s
        };
        let marker = vec![5u8; 176 * 176];
        // When (ms after the turn began) the published window first differs from the marker,
        // and how many recorded frames were shown.
        let run = |s: State, tick: u32, zero: bool| {
            let start = Rc::new(std::cell::Cell::new(None));
            let done = Rc::new(std::cell::Cell::new(false));
            let (st0, dn) = (Rc::clone(&start), Rc::clone(&done));
            let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
                io.draw(|fb| {
                    for y in 0..176 {
                        for x in 0..176 {
                            fb.set(x, y, 5);
                        }
                    }
                });
                io.present();
                io.vblank().await;
                st0.set(Some(io.now_ms()));
                world_turn(&st, &io).await;
                dn.set(true);
            });
            let mut changed = None;
            for _ in 0..MAX_TICKS {
                if done.get() {
                    break;
                }
                e.tick(&[], tick);
                if zero {
                    e.tick(&[], 0);
                }
                if let Some(t0) = start.get()
                    && changed.is_none()
                    && published_window(&e) != marker
                {
                    changed = Some(e.io().now_ms() - t0);
                }
            }
            assert!(done.get(), "the turn ends");
            (changed, e.state().combat.shown)
        };
        let draws = (0..200)
            .find(|&n| run(scene(n), 16, false).1 > 0)
            .expect("a generator state whose guard strikes");
        let want = 6 * u64::from(PICK_MS) + u64::from(crate::combat::present::FLASH_MS);
        for (tick, zero) in [(16, false), (7, false), (16, true)] {
            let (changed, shown) = run(scene(draws), tick, zero);
            assert!(shown > 0, "the flash was shown");
            let ms = changed.expect("the flash reached the screen");
            assert!(
                ms >= want && ms <= want + u64::from(tick),
                "the window changed {ms} ms in at {tick} ms ticks (zero polls {zero}), want \
                 {want}"
            );
        }
    }

    /// The oracle's trace tool keeps its own copy of the worktypes that run a combat routine
    /// (`COMBAT_ROUTINES` in tools/oracle/trace.mjs: it writes an NPC turn's `combat` from it, so
    /// the traces never hold a worktype). It must be the port's set, every worktype the same.
    #[test]
    #[allow(clippy::disallowed_methods)] // reads the tool's source, not the user's files
    fn the_trace_tools_combat_worktypes_are_the_ports() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/oracle/trace.mjs");
        let src = std::fs::read_to_string(path).expect("tools/oracle/trace.mjs");
        let decl = "const COMBAT_ROUTINES = new Set([";
        let start = src.find(decl).expect("the tool declares COMBAT_ROUTINES") + decl.len();
        let list = &src[start..start + src[start..].find(']').expect("the set's end")];
        let tool: std::collections::BTreeSet<u8> = list
            .split(',')
            .map(|v| {
                let v = v.trim();
                match v.strip_prefix("0x") {
                    Some(h) => u8::from_str_radix(h, 16),
                    None => v.parse(),
                }
                .unwrap_or_else(|_| panic!("a worktype in the tool's set: {v:?}"))
            })
            .collect();
        let port: std::collections::BTreeSet<u8> = (0..=255)
            .filter(|&wt| npc::runs_combat_routine(wt))
            .collect();
        assert_eq!(tool, port);
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

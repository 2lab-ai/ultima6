//! The NPC turn (0x1200b0) and the worktype behaviours it dispatches to.
//!
//! [`npc_turn`] is the one entry point (spec 03 §5): scheduler order, side effects and RNG draws
//! as in the original. Its synchronous part ([`npc_turn_sync`]) runs the worktype; the two
//! behaviours that wait for the player (an arrest prompt, an NPC who starts a conversation)
//! hand back a [`Wait`] that the async part awaits without holding a state borrow.
//!
//! Worktypes implemented in cut combat code (hunk 13: 3..6, 7, 8, 9, 10, 11, 13, 14, 0x13,
//! 0x9a, 0x9b and the 0x99 target search) are stub points. Their contract: argument the actor,
//! no result, no movement; the turn's epilogue then charges the actor 10 movement points
//! (0x1200b0 does that whenever the points did not change), so such an NPC always makes
//! progress toward the end of the round and never loops.

use std::cell::RefCell;

use crate::engine::Io;
use crate::objects::ObjectId;
use crate::state::State;
use crate::todo_stub;

use super::clock;
use super::path::{self, Goal};
use super::seams::{self, Arg};
use super::{leaves, tail, walk};

/// What the synchronous part left for the asynchronous part of an NPC turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// Nothing to wait for.
    None,
    /// 0x11fd20: a guard who caught up with the party asks for surrender.
    Arrest,
    /// 0x120034: the NPC starts a conversation with party member `target`.
    Talk(u16),
}

/// The combat worktype behaviours (hunk 13, `combat::npc`).
fn combat_worktype(s: &mut State, actor: u16, wt: u8) {
    crate::combat::npc::worktype(s, actor, wt);
}

/// [combat] 0x12500a (the 0x99 worktype's target search): a target actor or -1.
fn find_target(s: &mut State, actor: u16) -> i16 {
    crate::combat::npc::find_target(s, actor as i16)
}

/// [combat] 0x1230ce (the hit flash on the target).
fn strike(s: &mut State, target: u16) {
    crate::combat::damage::flash(s, target as i16);
}

/// Original: 0x12005a (actors: first free path slot, freeing slots of actors that stopped
/// walking; 8 = none free)
pub fn free_slot(s: &mut State) -> u8 {
    let mut i = 0u8;
    while i < 8 {
        let k = tail::SLOT_ACTOR + usize::from(i);
        let a = s.actors.tail(k);
        if a != 0 && !(0x83..=0x86).contains(&s.actors.worktype[usize::from(a)]) {
            s.actors.set_tail(k, 0);
        }
        if s.actors.tail(k) == 0 {
            break;
        }
        i += 1;
    }
    i
}

/// Original: 0x11f1d2 (actors: loiter about (tx, ty))
///
/// 7 in 8 turns the NPC waits (5 points); else it takes one step biased toward the spot: the
/// axis by a [`walk::random_offset`] against the difference of the distances, the way along it
/// by another against the distance itself.
pub fn loiter(s: &mut State, actor: u16, tx: u16, ty: u16) {
    if s.rng.range(0, 7) != 0 {
        walk::spend_mp(s, actor, 5);
        return;
    }
    let p = s.objects.location[usize::from(actor)];
    let dx = (p.x as i16).wrapping_sub(tx as i16);
    let dy = (p.y as i16).wrapping_sub(ty as i16);
    let r = walk::random_offset(&mut s.rng);
    let dir = if r > dy.wrapping_abs().wrapping_sub(dx.wrapping_abs()) {
        if walk::random_offset(&mut s.rng) > dx {
            2
        } else {
            6
        }
    } else if walk::random_offset(&mut s.rng) > dy {
        4
    } else {
        0
    };
    walk::step(s, actor, dir, 1);
}

/// Original: 0x11f2b0 (actors: one diagonal step between `hdir` and `vdir`)
///
/// Not for type 0x19d. Pays the current tile; party members may be swapped with while checking
/// that one of the two orthogonal neighbours is enterable; damaging tiles are refused. Then the
/// diagonal itself must be enterable: doors, move, movement flags, facing as in [`walk::step`].
pub fn step_diagonal(s: &mut State, actor: u16, hdir: u16, vdir: u16) -> bool {
    let a = usize::from(actor);
    if s.objects.type_frame[a] & 0x3ff == 0x19d {
        return false;
    }
    let d = if vdir == 0 && hdir == 6 {
        7
    } else {
        (vdir + hdir) >> 1
    };
    walk::spend_tile_cost(s, actor);
    let p = s.objects.location[a];
    let nx = p.x.wrapping_add_signed(walk::DX8[usize::from(d)]);
    let ny = p.y.wrapping_add_signed(walk::DY8[usize::from(d)]);
    s.actors.work.swap_ok = 1;
    s.actors.work.avoid_damage = 1;
    let side = walk::can_move_to(s, actor, nx, p.y) || walk::can_move_to(s, actor, p.x, ny);
    s.actors.work.swap_ok = 0;
    if side && walk::can_move_to(s, actor, nx, ny) {
        s.actors.work.avoid_damage = 0;
        walk::door_at_actor(s, actor, 1);
        seams::move_object(s, ObjectId(actor), nx, ny, walk::party_z(s));
        let mf = &mut s.actors.attrs.movement_flags[a];
        *mf = (*mf & 0xf8) | d as u8;
        s.actors.work.stepped = 1;
        walk::face(s, actor, d);
        walk::door_at_actor(s, actor, 0);
        return true;
    }
    s.actors.work.avoid_damage = 0;
    false
}

/// Original: 0x11f452 (actors: step toward (tx, ty))
///
/// Standing there (or worktype 0x0e): wait, 5 points, false. Else picks the horizontal and
/// vertical directions (a coin flip on a zero axis) and which to try first (nearer than 4 on
/// both axes: the longer axis, a coin on a tie; else a die weighted by the distances), then
/// tries first axis, diagonal, second axis, and on a coin flip the opposite of the first
/// axis; if all fail, pays the tile and returns false. Damaging tiles are refused unless
/// A4-0x6a94 is set.
pub fn approach(s: &mut State, actor: u16, tx: u16, ty: u16) -> bool {
    let a = usize::from(actor);
    let avoid = u16::from(s.actors.work.walk_through_damage == 0);
    let p = s.objects.location[a];
    let dx = (tx as i16).wrapping_sub(p.x as i16);
    let dy = (ty as i16).wrapping_sub(p.y as i16);
    if (dx == 0 && dy == 0) || s.actors.worktype[a] == 0x0e {
        walk::spend_mp(s, actor, 5);
        return false;
    }
    let hdir = match dx {
        0 => {
            if s.rng.range(0, 1) != 0 {
                2
            } else {
                6
            }
        }
        d if d < 0 => 6,
        _ => 2,
    };
    let vdir = match dy {
        0 => {
            if s.rng.range(0, 1) != 0 {
                0
            } else {
                4
            }
        }
        d if d < 0 => 0,
        _ => 4,
    };
    s.actors.work.swap_ok = 0;
    let (ax, ay) = (dx.wrapping_abs(), dy.wrapping_abs());
    let vertical_first = if ax < 4 && ay < 4 {
        ax < ay || (ax == ay && s.rng.range(0, 1) != 0)
    } else {
        s.rng.range(1, ax.wrapping_add(ay) as u16) as i16 > ax
    };
    let (first, second) = if vertical_first {
        (vdir, hdir)
    } else {
        (hdir, vdir)
    };
    let moved = walk::step(s, actor, first, avoid)
        || step_diagonal(s, actor, hdir, vdir)
        || walk::step(s, actor, second, avoid)
        || (s.rng.range(0, 1) == 0 && walk::step(s, actor, first ^ 4, 1));
    if !moved {
        walk::spend_tile_cost(s, actor);
    }
    s.actors.work.swap_ok = 1;
    moved
}

/// Original: 0x11f72e (actors: wander; party members keep near the leader)
///
/// Worktype 0x0e waits. A party member away from the leader heads for it 1 time in 4.
/// Otherwise 1 time in 8 a step in a random straight direction, else wait (5 points).
pub fn wander(s: &mut State, actor: u16) {
    let a = usize::from(actor);
    if s.actors.worktype[a] == 0x0e {
        walk::spend_mp(s, actor, 5);
        return;
    }
    let c = s.map.centre;
    let p = s.objects.location[a];
    if s.actors.attrs.status[a] & 0x80 != 0 && (p.x != c.x || p.y != c.y) && s.rng.range(0, 3) == 0
    {
        approach(s, actor, c.x, c.y);
        return;
    }
    if s.rng.range(0, 7) == 0 {
        let d = s.rng.range(0, 3) * 2;
        walk::step(s, actor, d, 1);
    } else {
        walk::spend_mp(s, actor, 5);
    }
}

/// Original: 0x11f9c6 (actors: close in on the nearest visible party member; the member when
/// adjacent, else -1)
///
/// Nearest by squared distance (16-bit), below 50, ties broken by a coin. With nobody in sight
/// the NPC heads for the party centre (A4+0x1830).
pub fn close_in(s: &mut State, actor: u16) -> i16 {
    let p = s.objects.location[usize::from(actor)];
    let mut best: i16 = -1;
    let mut best_d: i16 = 50;
    for i in 0..usize::from(s.actors.party.size) {
        let m = u16::from(s.actors.party.members[i]);
        let mi = usize::from(m);
        if s.objects.type_frame[mi] == 0 || s.actors.attrs.status[mi] & 0x10 != 0 {
            continue;
        }
        if !leaves::can_see(s, actor, m) {
            continue;
        }
        let q = s.objects.location[mi];
        let ddx = (q.x as i16).wrapping_sub(p.x as i16);
        let ddy = (q.y as i16).wrapping_sub(p.y as i16);
        let d = ddx.wrapping_mul(ddx).wrapping_add(ddy.wrapping_mul(ddy));
        if d < best_d || (d == best_d && s.rng.range(0, 1) != 0) {
            best_d = d;
            best = m as i16;
        }
    }
    if best < 0 {
        let (x, y) = s.actors.work.party_centre;
        approach(s, actor, x as u16, y as u16);
        return -1;
    }
    let q = s.objects.location[best as usize];
    approach(s, actor, q.x, q.y);
    let me = s.objects.location[usize::from(actor)];
    let near = |a: u16, b: u16| (a as i16).wrapping_sub(b as i16).wrapping_abs() <= 1;
    if near(q.x, me.x) && near(q.y, me.y) {
        best
    } else {
        -1
    }
}

/// Original: 0x11f804 (actors: walk one step of the path in the actor's slot (worktypes
/// 0x83..0x85))
///
/// At the path's end the slot is freed; within 1 tile of (tx, ty) the actor [`arrive`]s
/// (path::arrive), else it searches again (0x81); 5 points. A step that works advances the run
/// (worktype back to 0x83); a blocked one escalates 0x83 → 0x84 → 0x85 (the turn ends) and at
/// 0x85 gives up (0x86, slot freed), arriving if the blocked step was the last onto (tx, ty).
pub fn walk_path(s: &mut State, actor: u16, tx: u16, ty: u16) {
    let a = usize::from(actor);
    let slot = usize::from(s.actors.attrs.unknown_18f1[a]);
    let at = usize::from(s.actors.tail(tail::SLOT_POS + slot));
    let b = s
        .actors
        .attrs
        .unknown_1af1
        .get(slot * 32 + at)
        .copied()
        .unwrap_or(0);
    if b == 0 {
        s.actors.set_tail(tail::SLOT_ACTOR + slot, 0);
        if seams::distance(s, actor, tx, ty) < 2 {
            path::arrive(s, actor);
        } else {
            s.actors.worktype[a] = 0x81;
        }
        walk::spend_mp(s, actor, 5);
        return;
    }
    let d = u16::from(b & 3) * 2;
    if walk::step(s, actor, d, 1) {
        let k = tail::SLOT_STEP + slot;
        let n = s.actors.tail(k).wrapping_add(1);
        s.actors.set_tail(k, n);
        if n >= b >> 2 {
            let k = tail::SLOT_POS + slot;
            s.actors.set_tail(k, s.actors.tail(k).wrapping_add(1));
            s.actors.set_tail(tail::SLOT_STEP + slot, 0);
        }
        s.actors.worktype[a] = 0x83;
        return;
    }
    match s.actors.worktype[a] {
        0x83 => {
            s.actors.worktype[a] = 0x84;
            s.actors.movement_points[a] = 0;
        }
        0x84 => {
            s.actors.worktype[a] = 0x85;
            s.actors.movement_points[a] = 0;
        }
        0x85 => {
            s.actors.worktype[a] = 0x86;
            s.actors.movement_points[a] = 0;
            s.actors.set_tail(tail::SLOT_ACTOR + slot, 0);
            let p = s.objects.location[a];
            let g = &s.actors.game;
            let nx = (p.x as i16).wrapping_add(g.delta_byte(usize::from(b)));
            let ny = (p.y as i16).wrapping_add(g.delta_byte(usize::from(b) + 8));
            if nx == tx as i16 && ny == ty as i16 {
                path::arrive(s, actor);
            }
        }
        _ => {}
    }
}

/// [objects] 0x10d2d8 / 0x10d16e (does `holder` carry gold (0x58); take `n` of it). Stub
/// contract: no gold is carried, so nothing is taken and no message prints.
fn carries_gold(_s: &State, _holder: u16) -> bool {
    todo_stub!(
        "0x10d2d8 holder carries an object type (fallback: no)",
        false
    )
}

/// [objects] 0x10d16e: removes `n` gold from `holder`. Unreachable while [`carries_gold`] says
/// no.
fn take_gold(_s: &mut State, _holder: u16, _n: u16) {
    todo_stub!(
        "0x10d16e take objects of a type from a holder (fallback: none)",
        ()
    )
}

/// Original: 0x11ffae (actors: a thief takes gold from party member `target`)
///
/// Only when the target carries gold: 10..30 gold leave it, then on `random(1, 30)` ≥
/// (target intelligence − thief dexterity + 30) / 2 the theft is noticed and printed.
/// (0x1200b0 never reaches it: see [`npc_turn_sync`], worktype 0x97.)
pub fn steal(s: &mut State, actor: u16, target: u16) {
    if !carries_gold(s, target) {
        return;
    }
    let n = s.rng.range(10, 30);
    take_gold(s, target, n);
    let roll = s.rng.range(1, 30) as i16;
    let int = leaves::intelligence(s, target) as i16;
    let dex = leaves::dexterity(s, actor) as i16;
    let mut need = int - dex + 30;
    if need < 0 {
        need += 1;
    }
    if roll >= need >> 1 {
        seams::print(s, -0x7466, &[Arg::Name(ObjectId(actor))]);
    }
}

/// The synchronous part of [`npc_turn`] (0x1200b0 up to its epilogue).
///
/// A party member who is not charmed is first uncharmed ([`leaves::uncharm`]: resets its
/// worktype). During a time stop, or on another level than the party, the NPC loses its turn
/// (0 points). Otherwise its worktype runs, with (tx, ty) = its schedule entry's location.
/// Returns the points it had (for the epilogue) and what the async part must wait for; `None`
/// when the turn ended early.
pub fn npc_turn_sync(s: &mut State, actor: u16) -> Option<(i8, Wait)> {
    let a = usize::from(actor);
    if s.actors.attrs.status[a] & 0x80 != 0 && s.objects.obj_status[a] & 0x04 == 0 {
        leaves::uncharm(s, actor);
    }
    let before = s.actors.mp(a);
    let z = s.objects.location[a].z;
    if s.actors.timers[super::timer::TIME_STOP] != 0 || z != s.map.centre.z {
        s.actors.movement_points[a] = 0;
        return None;
    }
    let (tx, ty) = path::schedule_entry(s, actor).map_or((0, 0), |e| (e.pos.x, e.pos.y));
    let wt = s.actors.worktype[a];
    let mut wait = Wait::None;
    match wt {
        3..=11 | 13 | 14 | 0x13 | 0x9a | 0x9b => combat_worktype(s, actor, wt),
        12 | 0x8f => wander(s, actor),
        15 | 16 | 0x8b..=0x8e => random_walk(s, actor, tx, ty),
        0x12 => {
            if close_in(s, actor) >= 0 {
                wait = Wait::Arrest;
            }
        }
        0x82 => {
            let slot = free_slot(s);
            s.actors.work.slot = slot;
            if slot < 8 {
                let kind = usize::from(s.actors.attrs.unknown_18f1[a]);
                let t = s.actors.game.work_objects.get(kind).copied().unwrap_or(0);
                path::seek(s, actor, Goal::Type(t));
                walk::spend_mp(s, actor, 5);
            } else {
                s.actors.movement_points[a] = 0;
            }
        }
        0x83..=0x85 => walk_path(s, actor, tx, ty),
        0x90 | 0x94 => loiter(s, actor, tx, ty),
        0x96 | 0x97 => {
            let c = s.map.centre;
            let near = |a: u16, b: u16| (a as i16).wrapping_sub(b as i16).wrapping_abs() < 5;
            if near(tx, c.x) && near(ty, c.y) {
                let target = close_in(s, actor);
                if target >= 0 {
                    s.actors.worktype[a] = 0x90;
                    // The original tests for 0x97 (thief) after writing 0x90, so it always
                    // talks; 0x11ffae is never reached from here.
                    if s.actors.worktype[a] == 0x97 {
                        steal(s, actor, target as u16);
                    } else {
                        wait = Wait::Talk(target as u16);
                    }
                }
            } else {
                loiter(s, actor, tx, ty);
            }
        }
        0x98 => {
            if s.actors.tail(tail::BELL_PENDING) != 0 {
                s.actors.set_tail(tail::BELL_PENDING, 0);
                let h = s.actors.calendar.hour % 12;
                s.actors.work.bell_strikes = if h == 0 { 12 } else { h };
                // The word at 0x13ab18 is the base tile of object type 0xec (the bell).
                let bell = s.tables.basetile.base.get(0xec).copied().unwrap_or(0);
                seams::set_tile_animation(s, bell, true);
            }
            walk::spend_mp(s, actor, 5);
        }
        0x99 => guard_post(s, actor, tx, ty),
        _ => walk::spend_mp(s, actor, 5),
    }
    Some((before, wait))
}

/// 0x1200b0 worktypes 15, 16, 0x8b..0x8e: a coin decides between waiting and a step in the
/// remembered direction (0x8b..0x8e on their post: the post's direction); a blocked step is
/// retried the other way at no extra cost.
fn random_walk(s: &mut State, actor: u16, tx: u16, ty: u16) {
    let a = usize::from(actor);
    if s.rng.range(0, 1) == 0 {
        walk::spend_mp(s, actor, 5);
        return;
    }
    let wt = s.actors.worktype[a];
    let p = s.objects.location[a];
    let mut d = if wt >= 0x8b && p.x == tx && p.y == ty {
        u16::from(wt - 0x8b) * 2
    } else {
        u16::from(s.actors.attrs.movement_flags[a] & 7)
    };
    let before = s.actors.movement_points[a];
    if !walk::step(s, actor, d, 1) {
        d ^= 4;
        s.actors.movement_points[a] = before;
        walk::step(s, actor, d, 1);
    }
}

/// 0x1200b0 worktype 0x99: 1 time in 4 look for a target near the post and go for it (strike
/// when adjacent, then back toward the post); otherwise loiter about the post.
fn guard_post(s: &mut State, actor: u16, tx: u16, ty: u16) {
    if s.rng.range(0, 3) == 0 {
        let t = find_target(s, actor);
        if t >= 0 && seams::distance(s, t as u16, tx, ty) < 5 {
            let q = s.objects.location[t as usize];
            if seams::distance(s, actor, q.x, q.y) < 2 {
                strike(s, t as u16);
                approach(s, actor, tx, ty);
            } else {
                approach(s, actor, q.x, q.y);
            }
            return;
        }
    }
    loiter(s, actor, tx, ty);
}

/// Original: 0x1200b0 (schedules: NPC turn: walk to current schedule entry / do worktype)
///
/// The single NPC-turn entry point (spec 03 §5). Runs [`npc_turn_sync`], awaits what it left
/// (arrest prompt, conversation) with no state borrow held, then the epilogue: an NPC whose
/// movement points did not change pays 10, so every turn makes progress.
pub async fn npc_turn(state: &RefCell<State>, io: &Io, actor: u16) {
    let started = npc_turn_sync(&mut state.borrow_mut(), actor);
    crate::combat::play_effects(state, io).await;
    let Some((before, wait)) = started else {
        return;
    };
    match wait {
        Wait::None => {}
        Wait::Arrest => arrest(state, io, actor).await,
        Wait::Talk(target) => talk(state, io, actor, target).await,
    }
    let mut s = state.borrow_mut();
    if s.actors.mp(usize::from(actor)) == before {
        walk::spend_mp(&mut s, actor, 10);
    }
}

/// Original: 0x11fd20 (actors: a guard arrests the party: "Wilt thou come quietly?")
///
/// Only once the flag bit 7 of actor 5's talk flags is set; otherwise the guard just goes back
/// to its schedule (0x81). Waits for Y or N. Yes: the party is knocked out, taken to the jail
/// cell (0xba, 0xe7, 0) and the clock runs in hours to 8 o'clock; the confiscation of
/// weapons is an objects stub point. No: the guard and every other guard (0x12) turn hostile
/// (worktype 8, evil alignment).
pub async fn arrest(state: &RefCell<State>, io: &Io, actor: u16) {
    let a = usize::from(actor);
    {
        let mut s = state.borrow_mut();
        if s.actors.attrs.talk_flags[5] & 0x80 == 0 {
            s.actors.worktype[a] = 0x81;
            return;
        }
        seams::print(&mut s, -0x7503, &[]);
    }
    let key = loop {
        let k = seams::read_key(state, io).await;
        if k == u16::from(b'N') || k == u16::from(b'Y') {
            break k;
        }
    };
    let mut s = state.borrow_mut();
    s.actors.worktype[a] = 0x81;
    if key == u16::from(b'Y') {
        seams::print(&mut s, -0x74cb, &[]);
        seams::full_redraw(&mut s);
        if s.actors.combat_mode() {
            crate::combat::mode::break_off(&mut s);
        }
        seams::jail_party(&mut s, 0xba, 0xe7, 0);
        while s.actors.calendar.hour != 8 {
            clock::tick(&mut s, 60);
        }
        todo_stub!(
            "0x10c928 confiscate the party's weapons (fallback: kept)",
            ()
        );
        s.gfx.dissolve = 1;
        todo_stub!(
            "0x10e81e jail door and objects after the arrest (fallback: none)",
            ()
        );
        seams::area_music(&mut s);
        seams::refresh_view(&mut s);
    } else {
        seams::print(&mut s, -0x7489, &[]);
        crate::combat::damage::rouse_neutrals(&mut s);
        s.actors.worktype[a] = 8;
        s.actors.attrs.status[a] = (s.actors.attrs.status[a] & 0x9f) | 0x20;
        for i in 0..0x100 {
            if s.actors.worktype[i] == 0x12 {
                s.actors.worktype[i] = 8;
                s.actors.attrs.status[i] = (s.actors.attrs.status[i] & 0x9f) | 0x20;
            }
        }
    }
    s.actors.work.prompt = 1;
}

/// Original: 0x120034 (actors: the NPC starts a conversation with party member `target`)
pub async fn talk(state: &RefCell<State>, io: &Io, actor: u16, target: u16) {
    seams::talk(state, io, target, actor, false).await;
    let mut s = state.borrow_mut();
    seams::area_music(&mut s);
    s.actors.work.prompt = 1;
}

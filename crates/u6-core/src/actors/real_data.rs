//! Real-data receipts for the world turn: the user's archive.org zip → u6-intake → `Files` →
//! `State::new_game`, then world turns and party steps. Skips where the zip is missing (never
//! in CI: the data is not ours to ship; it is only read here and the tests print numbers only).
#![allow(clippy::disallowed_methods)] // tests may read files

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use u6_formats::objblk::ObjLocation;
use u6_formats::position::Position;
use u6_intake::{InputFile, intake};

use crate::engine::Engine;
use crate::files::Files;
use crate::objects::{ACTOR_SLOTS, NONE, Slot, find_segment_at, remove, spawn_monster};
use crate::state::State;

use super::party::{self, StepResult};
use super::walk::{DX8, DY8, can_move_to, step};
use super::{GameTables, clock, path, turn};
use u6_testkit::{Quiet, assert_same};

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

/// The start state after load state: `State::new_game` and the object, region and view steps
/// of 0x105350's tail (what `main_loop::finish_load_state` runs, less the command-bar draw).
fn start() -> Option<(State, Files)> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).quiet();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    let files = Files::new(r.files);
    let mut state = State::new_game(&files).quiet();
    state.actors.game = GameTables::from_game(files.get("game").quiet()).quiet();
    crate::objects::load_state_objects(&mut state);
    state.map.flag_784e = 1;
    // The start window's egg stays unhatched (A4-0x7afb, which 0x1321dc / 0x1319b2 check), as
    // in combat's receipt: since the eggs hatch at boot its gargoyles fight the party, and the
    // hero died around pass 70 (resurrected in the castle), so the schedules this receipt
    // watches were no longer the ones around the start.
    state.map.flag_7afb = 1;
    crate::map::region::refresh_region(&mut state);
    state.map.flag_7afb = 0;
    crate::map::view::update_view(&mut state);
    Some((state, files))
}

fn chebyshev(a: Position, b: Position) -> i16 {
    let dx = (a.x as i16 - b.x as i16).abs();
    let dy = (a.y as i16 - b.y as i16).abs();
    dx.max(dy)
}

fn leader(s: &State) -> u16 {
    s.actors.party.leader().quiet().0
}

/// One "pass" command as the command loop 0x1041d8 runs it (space / Esc): the leader's points
/// go to 0, the followers close up (0x11c910 with D1 = 1), then the world turn.
async fn pass(state: &RefCell<State>, io: &crate::engine::Io) {
    {
        let mut s = state.borrow_mut();
        let l = leader(&s);
        s.actors.movement_points[usize::from(l)] = 0;
        party::follow(&mut s, l, true);
    }
    turn::world_turn(state, io).await;
}

/// Per pass, each watched NPC's location and worktype.
type Trace = Vec<Vec<(Position, u8)>>;

/// How an NPC got from one pass (one player turn = one minute, several NPC turns) to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Move {
    Stay,
    /// 1..=4 tiles while walking a path (worktype 0x83..0x85): steps.
    Walk,
    /// Anything else that moved: a jump to the scheduled place (0x11e60c) or a loiter step.
    Other,
}

fn classify(a: Position, b: Position, wt: u8) -> Move {
    match chebyshev(a, b) {
        0 => Move::Stay,
        1..=4 if (0x83..=0x85).contains(&wt) => Move::Walk,
        _ => Move::Other,
    }
}

#[test]
fn world_turns_move_npcs_toward_their_schedules() {
    let Some((state, files)) = start() else {
        return;
    };
    let l = leader(&state);
    assert!(
        state.actors.worktype[usize::from(l)] == 2,
        "leader is the player"
    );
    let lp = state.objects.location[usize::from(l)];
    // The 10 NPCs nearest the leader on its level.
    let mut near: Vec<(i16, u16)> = (2..0xe0u16)
        .filter(|&a| state.objects.type_frame[usize::from(a)] != 0)
        .filter(|&a| state.actors.attrs.status[usize::from(a)] & 0x80 == 0)
        .filter(|&a| state.objects.location[usize::from(a)].z == lp.z)
        .map(|a| (chebyshev(state.objects.location[usize::from(a)], lp), a))
        .collect();
    near.sort();
    let watched: Vec<u16> = near.iter().take(10).map(|&(_, a)| a).collect();
    // The first hour boundary at which an NPC in view (within HALF_W columns and HALF_H rows,
    // where NPCs walk instead of jumping) is sent somewhere else: run the hourly pick on copies.
    let seen = |a: u16| {
        let p = state.objects.location[usize::from(a)];
        let (dx, dy) = (p.x.abs_diff(lp.x), p.y.abs_diff(lp.y));
        usize::from(dx) <= crate::geom::HALF_W && usize::from(dy) <= crate::geom::HALF_H
    };
    let in_view: Vec<u16> = near.iter().map(|&(_, a)| a).filter(|&a| seen(a)).collect();
    let here = |s: &State, a: u16| path::schedule_entry(s, a).map(|e| e.pos);
    let boundary = (1..24u8)
        .map(|k| (state.actors.calendar.hour + k) % 24)
        .find(|&h| {
            let mut next = state.clone();
            next.actors.calendar.hour = h;
            clock::pick_schedules(&mut next);
            in_view.iter().any(|&a| here(&next, a) != here(&state, a))
        })
        .expect("a schedule boundary for the NPCs in view");
    let before = state.clone();
    let c0 = state.actors.calendar;
    let trace: Rc<RefCell<Trace>> = Rc::new(RefCell::new(Vec::new()));
    let lowest = Rc::new(std::cell::Cell::new(state.actors.attrs.hp[1]));
    let (tr, ids, low) = (Rc::clone(&trace), watched.clone(), Rc::clone(&lowest));
    let mut engine = Engine::with_game(state, files, move |st, io| async move {
        let mut after = 0;
        // Pass until the boundary hour, then 60 more passes (about an hour) of walking.
        while after < 60 && tr.borrow().len() < 2000 {
            pass(&st, &io).await;
            let s = st.borrow();
            low.set(low.get().min(s.actors.attrs.hp[1]));
            let row = ids
                .iter()
                .map(|&a| {
                    (
                        s.objects.location[usize::from(a)],
                        s.actors.worktype[usize::from(a)],
                    )
                })
                .collect();
            tr.borrow_mut().push(row);
            if after > 0 || s.actors.calendar.hour == boundary {
                after += 1;
            }
        }
    });
    for _ in 0..100_000 {
        if engine.finished() {
            break;
        }
        engine.tick(&[], 16);
    }
    assert!(engine.finished(), "the world turns returned to the player");
    let after = engine.state().clone();
    assert!(
        lowest.get() == before.actors.attrs.hp[1],
        "no fight at the start: nothing hurts the hero"
    );
    let c1 = after.actors.calendar;
    let trace = trace.borrow();
    assert!(c0.hour != c1.hour, "the clock crossed an hour");
    let mut walked_closer = 0;
    for (k, &a) in watched.iter().enumerate() {
        let i = usize::from(a);
        let e = path::schedule_entry(&after, a);
        let p0 = before.objects.location[i];
        let goal = e.map(|e| e.pos).unwrap_or(p0);
        // Path-walking passes that got closer to the entry (output: none of the places, types
        // or worktypes, which are the user's data).
        let mut closer = 0;
        let mut prev = p0;
        for row in trace.iter() {
            let (p, wt) = row[k];
            if classify(prev, p, wt) == Move::Walk && chebyshev(p, goal) < chebyshev(prev, goal) {
                closer += 1;
            }
            prev = p;
        }
        if closer >= 2 {
            walked_closer += 1;
        }
    }
    for id in engine.stub_hits().keys() {
        println!("stub {id}");
    }
    assert!(
        walked_closer > 0,
        "an NPC walked toward its scheduled place"
    );
}

#[test]
fn party_steps_five_tiles_each_way() {
    let Some((start, _files)) = start() else {
        return;
    };
    // A step moves the leader and the view with it, a refused one leaves both (where it walks
    // is the user's map, so nothing of it is printed).
    for d in 0..8u16 {
        let mut s = start.clone();
        let l = leader(&s);
        for _ in 0..5 {
            s.actors.movement_points[usize::from(l)] = 30;
            let from = s.objects.location[usize::from(l)];
            match party::player_step(&mut s, d) {
                StepResult::Moved => {
                    let to = s.objects.location[usize::from(l)];
                    assert!(
                        (s.map.centre.x, s.map.centre.y) == (to.x, to.y),
                        "direction {d}"
                    );
                    assert!(from != to, "direction {d}");
                }
                _ => {
                    assert!(s.objects.location[usize::from(l)] == from, "direction {d}");
                }
            }
        }
    }
}

#[test]
fn the_ambush_kinds_are_creatures_of_the_game() {
    let Some((s, _files)) = start() else {
        return;
    };
    let g = &s.actors.game;
    for k in 0..4 {
        let kind = g.ambush_kind(k).expect("read from game");
        let index = g.creature_index(kind);
        assert!(index >= 0, "roll {k}: a creature type");
    }
    assert!(g.ambush_kind(4).is_none());
}

/// The dragon: a head actor and the four part objects beside it, five tiles (0x10b922,
/// 0x131646).
const DRAGON: u16 = 0x19b;
/// Egg type (0x1321dc).
const EGG: u16 = 0x14f;
/// The sides 0x10b9ca moves the parts on: N, E, S, W.
const SIDES: [usize; 4] = [0, 2, 4, 6];
/// The status byte 0x131646 leaves on a dragon part: 0x10bcee creates it with 1 plus the
/// temporary bit 0x20, which 0x131646 then clears.
const SPAWNED_PART: u8 = 0x01;

/// A dragon ready to walk: its head (an actor slot), its four part slots (N, E, S, W at the
/// start) and a direction 90° from its facing that `can_move_to` allows twice; the files for
/// an engine.
struct Dragon {
    s: State,
    files: Files,
    head: u16,
    parts: [Slot; 4],
    dir: u16,
}

/// The head's facing, 0..3 = N, E, S, W (frame bits 1-2).
fn facing_of(s: &State, head: u16) -> u16 {
    (s.objects.tf(head as Slot) >> 11) & 3
}

/// How many of the dragon's own parts (`parts`, N, E, S, W) carry, bit 0 aside, the frame
/// 0x11ba5e gives their side for facing `facing` (A4-0x6a70 as the actor tables read it), read
/// off the part slots themselves rather than through 0x10b922.
fn framed_for(s: &State, parts: &[Slot; 4], facing: u16) -> usize {
    let table = &s.actors.game.part_frames;
    (0..4)
        .filter(|&k| {
            let want = u16::from(table[usize::from(facing) * 4 + k]) & 0x3e;
            (s.objects.tf(parts[k]) >> 10) & 0x3e == want
        })
        .count()
}

/// The tile `k` steps from the head in 8-way `dir` (`step`'s offsets).
fn ahead(s: &State, head: u16, dir: u16, k: i16) -> (u16, u16) {
    let p = s.objects.location[usize::from(head)];
    let d = usize::from(dir);
    (
        p.x.wrapping_add_signed(k * DX8[d]),
        p.y.wrapping_add_signed(k * DY8[d]),
    )
}

/// Per side, the part `find_segment_at` finds beside the head with the head's facing, kept only
/// when it stands on that side's tile (one tile N, E, S, W on the head's level), else -1.
fn parts_beside(s: &mut State, head: u16) -> [Slot; 4] {
    let h = head as Slot;
    let (x, y, z) = (s.objects.x(h), s.objects.y(h), s.objects.z(h));
    let facing = (s.objects.tf(h) >> 10) & 6;
    SIDES.map(|d| {
        let p = find_segment_at(s, x, y, u16::from(z), d, facing);
        let side = (
            x.wrapping_add_signed(DX8[d]),
            y.wrapping_add_signed(DY8[d]),
            z,
        );
        let at = (s.objects.x(p), s.objects.y(p), s.objects.z(p));
        if p >= 0 && at == side { p } else { NONE }
    })
}

/// The dragon heads of the start state (actor slots of the kind with a head frame below 8, on
/// the map), then the eggs on the map that hatch one (a template of the kind), in block order.
/// For each in turn the view centre goes onto its tile and the region around it loads the way
/// [`start`] loads the start region (eggs left unhatched); an egg's dragon is made on the egg's
/// tile by 0x131646, as 0x1319b2 makes its first hatchling. The first whose four parts are found
/// beside it and that may step twice 90° from its facing is the one.
fn dragon() -> Option<Dragon> {
    dragon_where("", |_| true)
}

/// [`dragon`] among the heads and eggs below the surface (level above 0), where a block read is
/// 0x10e10c's (one dungeon level) and coordinates would wrap at 0x100.
fn dragon_below() -> Option<Dragon> {
    dragon_where("below the surface ", |at| at.z != 0)
}

/// [`dragon`] among the heads and eggs whose tile `wanted` takes (`label` names the choice in
/// the counts printed). `None` only without the user's files; with them and no such dragon it
/// panics, as [`dragon`] does.
fn dragon_where(label: &str, wanted: impl Fn(Position) -> bool) -> Option<Dragon> {
    let (start, files) = start()?;
    let o = &start.objects;
    let heads: Vec<(Position, Option<u16>)> = (0..ACTOR_SLOTS as u16)
        .filter(|&a| {
            let a = a as Slot;
            o.kind(a) == DRAGON && o.tf(a) >> 10 < 8 && o.place(a) == 0
        })
        .map(|a| (o.location[usize::from(a)], Some(a)))
        .filter(|&(at, _)| wanted(at))
        .collect();
    let mut eggs = Vec::new();
    for (_, b) in &o.blocks {
        for (k, r) in b.objects.iter().enumerate() {
            let on_map = r.obj_n == EGG && r.location() == ObjLocation::Map;
            let holds = |c: &u6_formats::objblk::ObjRecord| {
                c.container_index() == Some(k as u16) && c.obj_n == DRAGON
            };
            if on_map && wanted(r.pos) && b.objects.iter().any(holds) {
                eggs.push((r.pos, None));
            }
        }
    }
    println!(
        "dragon heads in the start state {label}{}, dragon eggs on the map {label}{}",
        heads.len(),
        eggs.len()
    );
    for (n, (at, found)) in heads.into_iter().chain(eggs).enumerate() {
        let mut s = start.clone();
        s.map.centre = at;
        s.map.flag_7afb = 1;
        crate::map::region::refresh_region(&mut s);
        s.map.flag_7afb = 0;
        crate::map::view::update_view(&mut s);
        let head = match found {
            Some(a) => a,
            None => {
                let m = spawn_monster(&mut s, DRAGON, at.x, at.y, u16::from(at.z));
                assert!(
                    (0..ACTOR_SLOTS as Slot).contains(&m),
                    "candidate {n}: the dragon takes an actor slot"
                );
                m as u16
            }
        };
        let f = s.objects.tf(head as Slot) >> 10;
        assert!(f < 8, "candidate {n}: a head frame below 8");
        let parts = parts_beside(&mut s, head);
        let beside = parts.iter().filter(|&&p| p >= 0).count();
        if found.is_some() && beside < 4 {
            continue;
        }
        assert!(
            beside == 4,
            "candidate {n}: the new dragon's parts found beside it {beside} of 4"
        );
        let facing = f & 6;
        for dir in [(facing + 2) & 6, (facing + 6) & 6] {
            let free = (1..=2).all(|k| {
                let (x, y) = ahead(&s, head, dir, k);
                can_move_to(&mut s, head, x, y)
            });
            if free {
                println!(
                    "candidate {n}: head slot {head}, part slots {parts:?}, turning direction \
                     {dir}"
                );
                return Some(Dragon {
                    s,
                    files,
                    head,
                    parts,
                    dir,
                });
            }
        }
    }
    panic!("a dragon that may step twice 90° from its facing");
}

/// After a step: the parts found beside the head (`find_segment_at`, the head's facing, one
/// tile N, E, S, W of it), how many of those are this dragon's, and how many of this dragon's
/// part slots stand anywhere but one tile N, E, S or W of the head.
fn parts_after(s: &mut State, head: u16, parts: &[Slot; 4]) -> (usize, usize, usize) {
    let found = parts_beside(s, head);
    let beside = found.iter().filter(|&&p| p >= 0).count();
    let own = found.iter().filter(|&p| parts.contains(p)).count();
    let h = head as Slot;
    let (hx, hy, hz) = (s.objects.x(h), s.objects.y(h), s.objects.z(h));
    let sides: Vec<(u16, u16, u8)> = SIDES
        .iter()
        .map(|&d| {
            (
                hx.wrapping_add_signed(DX8[d]),
                hy.wrapping_add_signed(DY8[d]),
                hz,
            )
        })
        .collect();
    let elsewhere = parts
        .iter()
        .filter(|&&p| {
            let o = &s.objects;
            let at = (o.x(p), o.y(p), o.z(p));
            o.kind(p) != DRAGON || o.place(p) != 0 || !sides.contains(&at)
        })
        .count();
    (beside, own, elsewhere)
}

/// The step 0x11f0a0 → 0x10c9d2 → 0x10b9ca, then 0x11ba5e: a dragon that turns (a step 90°
/// from its facing) and steps on the same way keeps its four parts beside it, found with its
/// new facing; none of them stays behind.
#[test]
fn a_turned_dragon_keeps_its_parts() {
    let Some(Dragon {
        mut s,
        head,
        parts,
        dir,
        ..
    }) = dragon()
    else {
        return;
    };
    let mut seen = Vec::new();
    for n in 1..=2 {
        let want = ahead(&s, head, dir, 1);
        let moved = step(&mut s, head, dir, 0);
        let p = s.objects.location[usize::from(head)];
        let one = (p.x, p.y) == want;
        let (beside, own, elsewhere) = parts_after(&mut s, head, &parts);
        let turned = facing_of(&s, head) == dir >> 1;
        let framed = framed_for(&s, &parts, dir >> 1);
        seen.push((n, moved, one, turned, framed, beside, own, elsewhere));
    }
    for &(n, moved, one, turned, framed, beside, own, elsewhere) in &seen {
        println!(
            "step {n} (direction {dir}): moved {moved}, one tile that way {one}, faces that way \
             {turned}, own parts framed for it {framed} of 4, parts found beside the head \
             {beside} of 4 ({own} its own), its part slots elsewhere {elsewhere} of 4"
        );
    }
    for &(n, moved, one, turned, framed, beside, own, elsewhere) in &seen {
        assert!(moved, "step {n} (direction {dir}): the dragon moved");
        assert!(
            one,
            "step {n} (direction {dir}): the head moved one tile that way"
        );
        assert!(
            turned,
            "step {n} (direction {dir}): the head faces that way"
        );
        assert!(
            framed == 4,
            "step {n} (direction {dir}): own parts framed for that way {framed} of 4"
        );
        assert!(
            beside == 4 && own == 4 && elsewhere == 0,
            "step {n} (direction {dir}): parts found beside the head {beside} of 4 ({own} its \
             own), its part slots elsewhere {elsewhere} of 4"
        );
    }
    let mut found = parts_beside(&mut s, head).to_vec();
    let mut want = parts.to_vec();
    found.sort();
    want.sort();
    assert_same!(
        found,
        want,
        "the part slots beside the head after two steps"
    );
}

/// The removal 0x10c1d6 → 0x10b9ca with `drop`: a dragon removed after a turning step takes its
/// four parts out of the world with it, as one removed before any step does.
#[test]
fn a_turned_dragon_removed_takes_its_parts() {
    let Some(Dragon {
        mut s,
        head,
        parts,
        dir,
        ..
    }) = dragon()
    else {
        return;
    };
    let left = |s: &State| parts.iter().filter(|&&p| s.objects.tf(p) != 0).count();
    let facing0 = facing_of(&s, head);
    let mut unturned = s.clone();
    remove(&mut unturned, head as Slot);
    println!(
        "removed unturned: part slots still in the world {} of 4",
        left(&unturned)
    );
    assert!(
        left(&unturned) == 0,
        "removed before any step: part slots still in the world {} of 4",
        left(&unturned)
    );
    let want = ahead(&s, head, dir, 1);
    assert!(
        step(&mut s, head, dir, 0),
        "step (direction {dir}): the dragon moved"
    );
    let p = s.objects.location[usize::from(head)];
    let facing = facing_of(&s, head);
    let turned = facing0 != facing && facing == dir >> 1;
    let framed = framed_for(&s, &parts, dir >> 1);
    println!(
        "step (direction {dir}): one tile that way {}, turned that way {turned}, own parts \
         framed for it {framed} of 4",
        (p.x, p.y) == want
    );
    assert!(
        (p.x, p.y) == want,
        "step (direction {dir}): the head moved one tile that way"
    );
    assert!(turned, "step (direction {dir}): the head turned that way");
    assert!(
        framed == 4,
        "step (direction {dir}): own parts framed for that way {framed} of 4"
    );
    remove(&mut s, head as Slot);
    println!(
        "removed after the turn: part slots still in the world {} of 4",
        left(&s)
    );
    assert!(
        left(&s) == 0,
        "removed after a turn (direction {dir}): part slots still in the world {} of 4",
        left(&s)
    );
}

/// Dragon objects (0x19b, object slots) on the map at `at`; with `status`, only those whose
/// status byte is that.
fn dragon_objects_at(s: &State, at: Position, status: Option<u8>) -> usize {
    let o = &s.objects;
    (ACTOR_SLOTS..crate::objects::OBJECT_SLOTS)
        .map(|c| c as Slot)
        .filter(|&c| o.kind(c) == DRAGON && o.place(c) == 0 && o.location[c as usize] == at)
        .filter(|&c| status.is_none_or(|st| o.status(c) == st))
        .count()
}

/// Entries of the object list (walked from its head) with no type: a slot freed but left
/// linked; 0 in a sound list.
fn untyped_in_list(s: &State) -> usize {
    let o = &s.objects;
    let mut c = o.next_of(crate::objects::HEAD);
    let (mut n, mut steps) = (0, 0);
    while c >= 0 && steps < crate::objects::OBJECT_SLOTS {
        if o.tf(c) == 0 {
            n += 1;
        }
        c = o.next_of(c);
        steps += 1;
    }
    n
}

/// The loaded region's objects go back to their block files and are read again: Save's
/// write-back 0x10e4bc (the region's blocks written, committed and read at the view centre; the
/// actor slots stay as they are).
fn reload(s: &mut State) {
    crate::saveload::write_back_region(s);
}

/// The view centre onto `at` and the region around it loaded, as [`dragon`] does.
fn look_at(s: &mut State, at: Position) {
    s.map.centre = at;
    s.map.flag_7afb = 1;
    crate::map::region::refresh_region(s);
    s.map.flag_7afb = 0;
    crate::map::view::update_view(s);
}

/// Port-only repair guard (D2): the load-time sweep of stranded dragon parts takes only a dragon
/// object (0x19b) on the map with exactly the status byte 0x131646 gives a part
/// ([`SPAWNED_PART`]) and no head beside it. The shipped block files hold dragon objects on the
/// map without a head; none of them carries that byte. Every block file of the start state,
/// records on the map (never an egg's template).
#[test]
fn the_shipped_map_has_no_part_the_sweep_would_take() {
    let Some((s, _files)) = start() else {
        return;
    };
    let on_map: Vec<u8> = s
        .objects
        .blocks
        .iter()
        .flat_map(|(_, b)| &b.objects)
        .filter(|r| r.obj_n == DRAGON && r.location() == ObjLocation::Map)
        .map(|r| r.status)
        .collect();
    let taken = on_map.iter().filter(|&&st| st == SPAWNED_PART).count();
    assert!(
        !on_map.is_empty(),
        "the block files hold dragon objects on the map (the guard is not vacuous)"
    );
    println!(
        "dragon objects on the map in the block files {}, with a spawned part's status {taken}",
        on_map.len()
    );
    assert!(
        taken == 0,
        "dragon objects on the map with a spawned part's status {taken}"
    );
}

/// Port-only repair (D2): parts the old build stranded (a turn re-framed the head but not its
/// parts, so its removal found none of them) are dropped when their block is read again. A
/// dragon from an egg as in [`dragon`]; the old turn made by hand (the head's frame written for
/// the new facing, the parts left as they were), then the head removed by 0x10c1d6, which finds
/// no part to take. After the region's blocks are written back and read again, no part with the
/// spawned part's status stands where the four were.
#[test]
fn a_stranded_dragon_part_is_swept_on_load() {
    if let Some(d) = dragon() {
        stranded_parts_swept(d);
    }
}

/// [`a_stranded_dragon_part_is_swept_on_load`] below the surface: the read is 0x10e10c's
/// (`load_level`, then the repair), on a level where the sides would wrap at 0x100. The dragon
/// chosen is not at a level's edge, so this pins that call, not the wrap.
#[test]
fn a_stranded_dragon_part_is_swept_on_load_below_the_surface() {
    if let Some(d) = dragon_below() {
        stranded_parts_swept(d);
    }
}

/// The body of [`a_stranded_dragon_part_is_swept_on_load`] for `dragon`.
fn stranded_parts_swept(dragon: Dragon) {
    let Dragon {
        mut s,
        head,
        parts,
        dir,
        ..
    } = dragon;
    let at: Vec<Position> = parts
        .iter()
        .map(|&p| s.objects.location[p as usize])
        .collect();
    let spawned = parts
        .iter()
        .filter(|&&p| s.objects.status(p) == SPAWNED_PART)
        .count();
    assert!(
        spawned == 4,
        "the new dragon's parts with the spawned part's status {spawned} of 4"
    );
    let h = head as Slot;
    let w = s.objects.type_frame[usize::from(head)];
    s.objects.type_frame[usize::from(head)] = (w & 0x3ff) | ((dir & 6) << 10);
    remove(&mut s, h);
    let count = |s: &State| -> usize {
        at.iter()
            .map(|&a| dragon_objects_at(s, a, Some(SPAWNED_PART)))
            .sum()
    };
    let stranded = count(&s);
    assert!(
        s.objects.tf(h) == 0 && stranded == 4,
        "stranded: the head removed, its parts left {stranded} of 4"
    );
    reload(&mut s);
    let left = count(&s);
    let untyped = untyped_in_list(&s);
    println!(
        "stranded parts where they were: {stranded} of 4 before the reload, {left} after; \
         list entries without a type {untyped}"
    );
    assert!(
        left == 0,
        "stranded parts where they were after the reload {left} of 4"
    );
    assert!(
        untyped == 0,
        "list entries without a type after the reload {untyped}"
    );
}

/// A live dragon's parts survive the reads that would sweep stranded ones: the region's blocks
/// written back and read again (0x10e4bc), and Save then Restore with the party beside it
/// (0x105716, 0x1052c8: the head is an actor slot and comes back with objlist, the parts with
/// the blocks around the leader), before and after a turn.
#[test]
fn a_live_dragon_keeps_its_parts_over_a_reload() {
    let Some(Dragon {
        s: ready,
        files,
        head,
        dir,
        ..
    }) = dragon()
    else {
        return;
    };
    let h = head as Slot;
    let beside = |s: &mut State| parts_beside(s, head).iter().filter(|&&p| p >= 0).count();
    for turned in [false, true] {
        let mut s = ready.clone();
        if turned {
            assert!(step(&mut s, head, dir, 0), "the turning step moved");
        }
        let before = beside(&mut s);
        reload(&mut s);
        let alive = s.objects.kind(h) == DRAGON;
        let after = beside(&mut s);
        println!(
            "write-back and read, turned {turned}: head kept {alive}, parts beside the head \
             {before} of 4 before, {after} of 4 after"
        );
        assert!(
            alive && before == 4 && after == 4,
            "write-back and read, turned {turned}: head kept {alive}, parts beside the head \
             {before} of 4 before, {after} of 4 after"
        );

        let mut s = ready.clone();
        if turned {
            assert!(step(&mut s, head, dir, 0), "the turning step moved");
        }
        let at = s.objects.location[usize::from(head)];
        // Saved with the party at hand (Restore puts the view on the leader, and 0x108d10 drops
        // temporary actors farther than half the window from it): the leader three tiles behind.
        let l = leader(&s);
        let (lx, ly) = ahead(&s, head, (dir + 4) & 6, 3);
        crate::objects::move_to(&mut s, l as Slot, lx, ly, u16::from(at.z));
        let mut engine = Engine::with_game(s, files.clone(), move |st, io| async move {
            let mut s = st.borrow_mut();
            crate::saveload::save(&mut s);
            crate::saveload::restore(&mut s, &io).quiet();
        });
        for _ in 0..1000 {
            if engine.finished() {
                break;
            }
            engine.tick(&[], 16);
        }
        assert!(engine.finished(), "save and restore returned");
        let mut s = engine.state().clone();
        let alive = s.objects.kind(h) == DRAGON && s.objects.location[usize::from(head)] == at;
        let after = beside(&mut s);
        println!(
            "save and restore, turned {turned}: head kept {alive}, parts beside the head {after} \
             of 4"
        );
        assert!(
            alive && after == 4,
            "save and restore, turned {turned}: head kept {alive}, parts beside the head {after} \
             of 4"
        );
    }
}

/// The shipped map's headless dragon objects are not the port's stranded parts: after the
/// blocks that hold them are loaded, written back and read again (0x10e4bc), each tile holds as
/// many as its block file has.
#[test]
fn the_shipped_headless_dragon_objects_stay() {
    let Some((start, _files)) = start() else {
        return;
    };
    let shipped: Vec<Position> = start
        .objects
        .blocks
        .iter()
        .flat_map(|(_, b)| &b.objects)
        .filter(|r| r.obj_n == DRAGON && r.location() == ObjLocation::Map)
        .map(|r| r.pos)
        .collect();
    assert!(
        !shipped.is_empty(),
        "the block files hold dragon objects on the map (the test is not vacuous)"
    );
    let mut tiles: Vec<Position> = Vec::new();
    for &p in &shipped {
        if !tiles.contains(&p) {
            tiles.push(p);
        }
    }
    let mut kept = 0;
    for (n, &at) in tiles.iter().enumerate() {
        let want = shipped.iter().filter(|&&p| p == at).count();
        let mut s = start.clone();
        look_at(&mut s, at);
        let loaded = dragon_objects_at(&s, at, None);
        reload(&mut s);
        let after = dragon_objects_at(&s, at, None);
        println!("tile {n}: in the block file {want}, loaded {loaded}, after the reload {after}");
        assert!(
            loaded == want && after == want,
            "tile {n}: in the block file {want}, loaded {loaded}, after the reload {after}"
        );
        kept += after;
    }
    println!("shipped dragon objects kept {kept} of {}", shipped.len());
    assert!(kept == shipped.len(), "every shipped dragon object kept");
}

/// Port-only repair (D2): a dragon that turned under the old build lives on in a save with its
/// head on the new facing and its parts on the old one, so 0x10b9ca finds none of them. When its
/// block is read again the parts take the frames of the head's facing (A4-0x6a70), are found
/// beside it, and follow its next step. The old turn made by hand: the head's frame written for
/// the new facing, the parts left as they were.
#[test]
fn an_old_turned_dragon_is_reframed_on_load() {
    if let Some(d) = dragon() {
        old_turn_reframed(d);
    }
}

/// [`an_old_turned_dragon_is_reframed_on_load`] below the surface: the read is 0x10e10c's
/// (`load_level`, then the repair), on a level where the sides would wrap at 0x100. The dragon
/// chosen is not at a level's edge, so this pins that call, not the wrap.
#[test]
fn an_old_turned_dragon_is_reframed_on_load_below_the_surface() {
    if let Some(d) = dragon_below() {
        old_turn_reframed(d);
    }
}

/// The body of [`an_old_turned_dragon_is_reframed_on_load`] for `dragon`.
fn old_turn_reframed(dragon: Dragon) {
    let Dragon {
        mut s, head, dir, ..
    } = dragon;
    let beside = |s: &mut State| parts_beside(s, head).iter().filter(|&&p| p >= 0).count();
    let w = s.objects.type_frame[usize::from(head)];
    s.objects.type_frame[usize::from(head)] = (w & 0x3ff) | ((dir & 6) << 10);
    let before = beside(&mut s);
    assert!(
        before == 0,
        "the old turn: parts found beside the head {before} of 4"
    );
    reload(&mut s);
    let after = beside(&mut s);
    let moved = step(&mut s, head, dir, 0);
    let stepped = beside(&mut s);
    println!(
        "old turn (direction {dir}): parts beside the head {before} of 4 before the reload, \
         {after} after, {stepped} after a step (moved {moved})"
    );
    assert!(
        after == 4,
        "old turn (direction {dir}): parts beside the head after the reload {after} of 4"
    );
    assert!(
        moved && stepped == 4,
        "old turn (direction {dir}): moved {moved}, parts beside the head after a step \
         {stepped} of 4"
    );
}

/// Port-only repair (D3): a dragon that turned under the old build and then walked on lives on
/// in a save with its head tiles away from its four parts, since each step's 0x10b9ca found no
/// part of the new facing to move. When its block is read again the four parts stand beside the
/// head with its facing, none stays where they were, and they follow its next step. The old turn
/// made by hand (the head's frame written for the new facing, the parts left as they were), then
/// two steps that way, which move the head alone as the old build's did.
#[test]
fn a_dragon_that_turned_and_walked_is_whole_on_load() {
    if let Some(d) = dragon() {
        turned_and_walked_whole(d);
    }
}

/// [`a_dragon_that_turned_and_walked_is_whole_on_load`] below the surface: the read is
/// 0x10e10c's (`load_level`, then the repair).
#[test]
fn a_dragon_that_turned_and_walked_is_whole_on_load_below_the_surface() {
    if let Some(d) = dragon_below() {
        turned_and_walked_whole(d);
    }
}

/// The body of [`a_dragon_that_turned_and_walked_is_whole_on_load`] for `dragon`. The part
/// slots are read only before the reload (the write-back frees and refills object slots); after
/// it the parts are counted by the tiles they stood on. The step after the reload goes back the
/// way the head came, onto a tile [`dragon_where`] found it may enter and it entered: the tile a
/// third step ahead was never checked. Returns the state after that step.
fn turned_and_walked_whole(dragon: Dragon) -> State {
    let Dragon {
        mut s,
        head,
        parts,
        dir,
        ..
    } = dragon;
    let beside = |s: &mut State| parts_beside(s, head).iter().filter(|&&p| p >= 0).count();
    let at: Vec<Position> = parts
        .iter()
        .map(|&p| s.objects.location[p as usize])
        .collect();
    let spawned = parts
        .iter()
        .filter(|&&p| s.objects.status(p) == SPAWNED_PART)
        .count();
    assert!(
        spawned == 4,
        "the new dragon's parts with the spawned part's status {spawned} of 4"
    );
    let count = |s: &State| -> usize {
        at.iter()
            .map(|&a| dragon_objects_at(s, a, Some(SPAWNED_PART)))
            .sum()
    };
    let w = s.objects.type_frame[usize::from(head)];
    s.objects.type_frame[usize::from(head)] = (w & 0x3ff) | ((dir & 6) << 10);
    let walked = [step(&mut s, head, dir, 0), step(&mut s, head, dir, 0)];
    let walked_beside = beside(&mut s);
    let stayed = count(&s);
    assert!(
        walked == [true, true],
        "turned and walked (direction {dir}): the two steps moved {walked:?}"
    );
    assert!(
        walked_beside == 0,
        "turned and walked (direction {dir}): parts found beside the head {walked_beside} of 4"
    );
    assert!(
        stayed == 4,
        "turned and walked (direction {dir}): spawned parts where they were {stayed} of 4"
    );
    reload(&mut s);
    let after = beside(&mut s);
    // Spawned parts on the recorded tiles, less those found beside the head (a part the repair
    // kept may stand on a tile it stood on before, now one of the head's sides).
    let kept_there = parts_beside(&mut s, head)
        .iter()
        .filter(|&&p| p >= 0)
        .filter(|&&p| {
            let o = &s.objects;
            o.status(p) == SPAWNED_PART && at.contains(&o.location[p as usize])
        })
        .count();
    let left = count(&s).saturating_sub(kept_there);
    let untyped = untyped_in_list(&s);
    let back = (dir + 4) & 7;
    let moved = step(&mut s, head, back, 0);
    let stepped = beside(&mut s);
    println!(
        "turned and walked (direction {dir}): steps moved {walked:?}, parts beside the head \
         {walked_beside} of 4 after them, spawned parts where they were {stayed} of 4; after the \
         reload parts beside the head {after} of 4, spawned parts left where they were and not \
         beside it {left}, list entries without a type {untyped}; one more step (direction \
         {back}) moved {moved}, parts beside the head {stepped} of 4"
    );
    assert!(
        after == 4,
        "turned and walked (direction {dir}): parts beside the head after the reload {after} of 4"
    );
    assert!(
        left == 0,
        "turned and walked (direction {dir}): spawned parts left where they were and not beside \
         the head after the reload {left}"
    );
    assert!(
        untyped == 0,
        "turned and walked (direction {dir}): list entries without a type after the reload \
         {untyped}"
    );
    assert!(
        moved && stepped == 4,
        "turned and walked (direction {dir}): one more step (direction {back}) moved {moved}, \
         parts beside the head after it {stepped} of 4"
    );
    s
}

/// Port-only repair (D2): the re-frame never takes a part another dragon's 0x10b922 already
/// finds. Two dragons made by 0x131646 two tiles apart on a row, so the east part of the west
/// one and the west part of the east one share a tile; each in turn, then both, get the old
/// build's turn (the head's frame for the facing after its own, the parts left), the region's
/// blocks are written back and read again, and both have all four parts beside their heads. With
/// both turned neither part on the shared tile is found before the re-frame, so the second head
/// must leave the one re-framed for the first.
#[test]
fn a_neighbouring_dragon_keeps_its_part_when_another_is_reframed() {
    let Some(Dragon { s: one, head, .. }) = dragon() else {
        return;
    };
    let p = one.objects.location[usize::from(head)];
    let mut pair = one.clone();
    let m = spawn_monster(&mut pair, DRAGON, p.x.wrapping_add(2), p.y, u16::from(p.z));
    assert!(
        (0..ACTOR_SLOTS as Slot).contains(&m),
        "the second dragon takes an actor slot"
    );
    let heads = [head, m as u16];
    let beside = |s: &mut State, h: u16| parts_beside(s, h).iter().filter(|&&p| p >= 0).count();
    let shared = |s: &State| {
        let at = Position {
            x: p.x.wrapping_add(1),
            ..p
        };
        dragon_objects_at(s, at, None)
    };
    for (k, &h) in heads.iter().enumerate() {
        let before = beside(&mut pair, h);
        assert!(before == 4, "dragon {k}: parts beside it {before} of 4");
    }
    assert!(
        shared(&pair) == 2,
        "the shared tile holds both dragons' parts ({})",
        shared(&pair)
    );
    let cases: [(&str, &[usize]); 3] = [("dragon 0", &[0]), ("dragon 1", &[1]), ("both", &[0, 1])];
    for (t, turned) in cases {
        let mut s = pair.clone();
        for &k in turned {
            let i = usize::from(heads[k]);
            let w = s.objects.type_frame[i];
            let f = (w >> 10) & 6;
            s.objects.type_frame[i] = (w & 0x3ff) | (((f + 2) & 6) << 10);
        }
        let lost: usize = turned.iter().map(|&k| beside(&mut s, heads[k])).sum();
        reload(&mut s);
        let after: Vec<usize> = heads.iter().map(|&h| beside(&mut s, h)).collect();
        let kept = shared(&s);
        println!(
            "{t} given the old turn: parts beside the turned {lost} before the reload; after it \
             dragon 0 {} of 4, dragon 1 {} of 4, the shared tile holds {kept}",
            after[0], after[1]
        );
        assert!(
            after == [4, 4] && kept == 2,
            "{t} given the old turn: after the reload dragon 0 {} of 4, dragon 1 {} of 4, the \
             shared tile holds {kept}",
            after[0],
            after[1]
        );
    }
}

/// Port-only repair (D3): a removed dragon's stranded parts are still swept beside a live dragon
/// that lacks no side, so the re-attach (each side still empty takes the nearest part no head
/// owns) never reaches them. Two dragons made by 0x131646 two tiles apart on a row, as in
/// [`a_neighbouring_dragon_keeps_its_part_when_another_is_reframed`]; the west one gets the old
/// build's turn and is removed by 0x10c1d6, which finds no part to take. After the region's
/// blocks are written back and read again the east one has its four parts beside it, the shared
/// tile holds only its part, and no part with the spawned part's status stands on the removed
/// one's other three tiles.
#[test]
fn a_removed_dragon_s_parts_are_swept_beside_a_whole_one() {
    let Some(Dragon {
        mut s,
        head,
        parts,
        dir,
        ..
    }) = dragon()
    else {
        return;
    };
    let p = s.objects.location[usize::from(head)];
    let m = spawn_monster(&mut s, DRAGON, p.x.wrapping_add(2), p.y, u16::from(p.z));
    assert!(
        (0..ACTOR_SLOTS as Slot).contains(&m),
        "the second dragon takes an actor slot"
    );
    let live = m as u16;
    let beside = |s: &mut State, h: u16| parts_beside(s, h).iter().filter(|&&p| p >= 0).count();
    let shared_at = Position {
        x: p.x.wrapping_add(1),
        ..p
    };
    let others: Vec<Position> = parts
        .iter()
        .map(|&c| s.objects.location[c as usize])
        .filter(|&a| a != shared_at)
        .collect();
    let on_others = |s: &State| -> usize {
        others
            .iter()
            .map(|&a| dragon_objects_at(s, a, Some(SPAWNED_PART)))
            .sum()
    };
    let shared = |s: &State| dragon_objects_at(s, shared_at, Some(SPAWNED_PART));
    let (a0, b0) = (beside(&mut s, head), beside(&mut s, live));
    assert!(
        a0 == 4 && b0 == 4 && others.len() == 3 && shared(&s) == 2,
        "the two dragons: parts beside them {a0} and {b0} of 4, the removed one's tiles not \
         shared {}, the shared tile holds {}",
        others.len(),
        shared(&s)
    );
    let h = head as Slot;
    let w = s.objects.type_frame[usize::from(head)];
    s.objects.type_frame[usize::from(head)] = (w & 0x3ff) | ((dir & 6) << 10);
    remove(&mut s, h);
    let (stranded, shared_before, live_before) = (on_others(&s), shared(&s), beside(&mut s, live));
    assert!(
        s.objects.tf(h) == 0 && stranded == 3 && shared_before == 2 && live_before == 4,
        "the west dragon removed after its old turn: its parts left on its own tiles {stranded} \
         of 3, the shared tile holds {shared_before}, parts beside the east one {live_before} of 4"
    );
    reload(&mut s);
    let alive = s.objects.kind(live as Slot) == DRAGON;
    let (left, shared_after, live_after) = (on_others(&s), shared(&s), beside(&mut s, live));
    let untyped = untyped_in_list(&s);
    println!(
        "a removed dragon beside a whole one (direction {dir}): its parts on its own tiles \
         {stranded} of 3 before the reload, {left} after; the shared tile holds {shared_before} \
         before, {shared_after} after; the whole one kept {alive}, parts beside it {live_before} \
         of 4 before, {live_after} after; list entries without a type {untyped}"
    );
    assert!(
        left == 0 && shared_after == 1,
        "after the reload: the removed dragon's parts on its own tiles {left}, the shared tile \
         holds {shared_after}"
    );
    assert!(
        alive && live_after == 4,
        "after the reload: the whole dragon kept {alive}, parts beside it {live_after} of 4"
    );
    assert!(
        untyped == 0,
        "list entries without a type after the reload {untyped}"
    );
}

/// Spawned dragon parts (0x19b object slots on the map on level `z` with the spawned part's
/// status) and the tiles they stand on, in slot order.
fn spawned_parts(s: &State, z: u8) -> Vec<Position> {
    let o = &s.objects;
    (ACTOR_SLOTS..crate::objects::OBJECT_SLOTS)
        .map(|c| c as Slot)
        .filter(|&c| o.kind(c) == DRAGON && o.place(c) == 0 && o.z(c) == z)
        .filter(|&c| o.status(c) == SPAWNED_PART)
        .map(|c| o.location[c as usize])
        .collect()
}

/// Port-only repair (D3): a dragon head outside the loaded region takes no part of a walked
/// dragon. Its side tiles lie in a superchunk not in slots, where 0x10b922 finds nothing only
/// because the parts are in that block's file. The dragon of [`dragon`] is removed and its actor
/// slot goes to the far head (the head first in slot order, which the re-attach serves first) on
/// the centre of the superchunk four columns and four rows from the region's base; its parts are
/// removed as if in their block file. A dragon made again on the first one's tile then turns and
/// walks as in [`a_dragon_that_turned_and_walked_is_whole_on_load`] and is whole on load, and the
/// far head's sides stay empty.
#[test]
fn a_far_dragon_takes_no_part_of_a_walked_one() {
    let Some(Dragon {
        mut s,
        files,
        head,
        parts,
        dir,
    }) = dragon()
    else {
        return;
    };
    let p = s.objects.location[usize::from(head)];
    remove(&mut s, head as Slot);
    let left = parts.iter().filter(|&&c| s.objects.tf(c) != 0).count();
    assert!(
        left == 0,
        "the first dragon removed: parts left {left} of 4"
    );
    let base = s.objects.surface_base;
    assert!(p.z == 0 && base >= 0, "the first dragon is on the surface");
    let k = (((base & 0x38) + 0x20) & 0x38) + (((base & 7) + 4) & 7);
    let keyed = |s: &State| s.objects.loaded_surface.contains(&k);
    assert!(!keyed(&s), "the far superchunk is not in slots");
    let (fx, fy) = ((k & 7) as u16 * 0x80 + 0x40, (k >> 3) as u16 * 0x80 + 0x40);
    let far = spawn_monster(&mut s, DRAGON, fx, fy, 0);
    assert!(
        (0..ACTOR_SLOTS as Slot).contains(&far),
        "the far dragon takes an actor slot"
    );
    let far = far as u16;
    let own = parts_beside(&mut s, far);
    let made = own.iter().filter(|&&c| c >= 0).count();
    assert!(made == 4, "the far dragon's parts made {made} of 4");
    for c in own {
        remove(&mut s, c);
    }
    let walked = spawn_monster(&mut s, DRAGON, p.x, p.y, 0);
    assert!(
        (0..ACTOR_SLOTS as Slot).contains(&walked),
        "the walked dragon takes an actor slot"
    );
    let walked = walked as u16;
    assert!(
        far < walked,
        "the far head comes first in slot order (slots {far} and {walked})"
    );
    let parts = parts_beside(&mut s, walked);
    let beside = parts.iter().filter(|&&c| c >= 0).count();
    assert!(
        beside == 4,
        "the walked dragon's parts beside it {beside} of 4"
    );
    let far_sides: Vec<Position> = SIDES
        .iter()
        .map(|&d| Position {
            x: fx.wrapping_add_signed(DX8[d]),
            y: fy.wrapping_add_signed(DY8[d]),
            z: 0,
        })
        .collect();
    let mut s = turned_and_walked_whole(Dragon {
        s,
        files,
        head: walked,
        parts,
        dir,
    });
    let far_kept = s.objects.kind(far as Slot) == DRAGON;
    let far_beside = parts_beside(&mut s, far)
        .iter()
        .filter(|&&c| c >= 0)
        .count();
    let on_far_sides: usize = far_sides
        .iter()
        .map(|&a| dragon_objects_at(&s, a, Some(SPAWNED_PART)))
        .sum();
    let far_keyed = keyed(&s);
    println!(
        "far head (slot {far}) and walked head (slot {walked}): after the reload the far head kept \
         {far_kept}, its superchunk in slots {far_keyed}, parts beside it {far_beside} of 4, \
         spawned parts on its side tiles {on_far_sides}"
    );
    assert!(
        far_kept && !far_keyed && far_beside == 0 && on_far_sides == 0,
        "after the reload: the far head kept {far_kept}, its superchunk in slots {far_keyed}, \
         parts beside it {far_beside} of 4, spawned parts on its side tiles {on_far_sides}"
    );
}

/// Port-only repair (D3): a walked dragon whose head ends on the region's inner superchunk edge,
/// its sides in two superchunks in slots, is whole on load with no part doubled: the repair runs
/// once after all four reads, when both are keyed. The dragon of [`dragon`] is removed and the
/// view goes two tiles west of the inner vertical edge on its row (the same 2 × 2 stays in slots);
/// a dragon is made two tiles west of the edge on the first row within 16 of that one where it may
/// stand and step twice east, turns east and walks as in
/// [`a_dragon_that_turned_and_walked_is_whole_on_load`], so its head ends on the edge's east
/// column and its west side on the west superchunk. A regression guard: the per-read repair
/// before reached 4 of 4 here too, since a walked dragon has no part of its own in the later read.
#[test]
fn a_walked_dragon_on_a_superchunk_edge_is_whole_on_load() {
    let Some(Dragon {
        mut s, files, head, ..
    }) = dragon()
    else {
        return;
    };
    let p = s.objects.location[usize::from(head)];
    remove(&mut s, head as Slot);
    let base = s.objects.surface_base;
    assert!(p.z == 0 && base >= 0, "the first dragon is on the surface");
    let edge = (((base & 7) + 1) & 7) as u16 * 0x80;
    let x = edge.wrapping_sub(2) & 0x3ff;
    look_at(&mut s, Position { x, ..p });
    assert!(
        s.objects.surface_base == base,
        "the view two tiles from the edge keeps the region"
    );
    let dir = 2u16;
    let rows = (0..=16i16).flat_map(|k| [k, -k]).skip(1);
    let found = rows.map(|k| p.y.wrapping_add_signed(k)).find_map(|y| {
        let mut t = s.clone();
        let m = spawn_monster(&mut t, DRAGON, x, y, 0);
        if !(0..ACTOR_SLOTS as Slot).contains(&m) {
            return None;
        }
        let m = m as u16;
        let free = (0..=2).all(|k| can_move_to(&mut t, m, (x + k) & 0x3ff, y));
        free.then_some((t, m))
    });
    let Some((s, walked)) = found else {
        panic!("a row within 16 of the view where a dragon may stand and step twice east");
    };
    let mut t = s;
    let parts = parts_beside(&mut t, walked);
    let beside = parts.iter().filter(|&&c| c >= 0).count();
    assert!(
        beside == 4,
        "the edge dragon's parts beside it {beside} of 4"
    );
    let block = |px: u16, py: u16| (((py & 0x3ff) >> 7) * 8 + ((px & 0x3ff) >> 7)) as i16;
    let y = t.objects.location[usize::from(walked)].y;
    let (west, east) = (block(edge.wrapping_sub(1), y), block(edge, y));
    let mut s = turned_and_walked_whole(Dragon {
        s: t,
        files,
        head: walked,
        parts,
        dir,
    });
    let keyed = [west, east].map(|b| s.objects.loaded_surface.contains(&b));
    let found: Vec<Position> = parts_beside(&mut s, walked)
        .iter()
        .filter(|&&c| c >= 0)
        .map(|&c| s.objects.location[c as usize])
        .collect();
    let all = spawned_parts(&s, 0);
    let mut tiles = all.clone();
    tiles.sort_by_key(|a| (a.y, a.x));
    tiles.dedup();
    let doubled = all.len() - tiles.len();
    let elsewhere = all.iter().filter(|a| !found.contains(a)).count();
    let h = s.objects.location[usize::from(walked)];
    let sides_across = SIDES
        .iter()
        .map(|&d| block(h.x.wrapping_add_signed(DX8[d]), h.y))
        .filter(|&b| b != block(h.x, h.y))
        .count();
    println!(
        "edge dragon (slot {walked}): both superchunks in slots {keyed:?}, sides in the other \
         superchunk {sides_across}; spawned parts on the level {}, on a tile with another \
         {doubled}, not beside the head {elsewhere}, parts beside the head {} of 4",
        all.len(),
        found.len()
    );
    assert!(
        keyed == [true, true] && sides_across == 1,
        "both superchunks in slots {keyed:?}, sides in the other superchunk {sides_across}"
    );
    assert!(
        all.len() == 4 && doubled == 0 && elsewhere == 0 && found.len() == 4,
        "spawned parts on the level {}, on a tile with another {doubled}, not beside the head \
         {elsewhere}, parts beside the head {} of 4",
        all.len(),
        found.len()
    );
}

/// Port-only repair (D3): a save written after the D2 sweep holds a head with fewer than four
/// parts (the sweep removed those no head found), and the original's live head always has four:
/// 0x131646 makes them and only 0x10c1d6 with the head takes them. When its block is read again
/// the missing parts are made beside it as 0x131646 makes them, and reading it twice more makes
/// no more. The dragon of [`dragon`] takes its turning step (its parts follow), then all its parts
/// but the one behind it are removed by hand: the shape the D2 sweep left of a dragon that turned
/// and walked under the old build.
#[test]
fn a_dragon_saved_with_one_part_is_whole_on_load() {
    if let Some(d) = dragon() {
        missing_parts_made(d, 1);
    }
}

/// [`a_dragon_saved_with_one_part_is_whole_on_load`] below the surface (one read, 0x10e10c's).
#[test]
fn a_dragon_saved_with_one_part_is_whole_on_load_below_the_surface() {
    if let Some(d) = dragon_below() {
        missing_parts_made(d, 1);
    }
}

/// [`a_dragon_saved_with_one_part_is_whole_on_load`] with all four parts removed by hand.
#[test]
fn a_dragon_saved_with_no_part_is_whole_on_load() {
    if let Some(d) = dragon() {
        missing_parts_made(d, 0);
    }
}

/// [`a_dragon_saved_with_no_part_is_whole_on_load`] below the surface (one read, 0x10e10c's).
#[test]
fn a_dragon_saved_with_no_part_is_whole_on_load_below_the_surface() {
    if let Some(d) = dragon_below() {
        missing_parts_made(d, 0);
    }
}

/// The body of [`a_dragon_saved_with_one_part_is_whole_on_load`] for `dragon`, keeping `keep`
/// (0 or 1) of its parts: with 1, the one on the side behind the head. After the reload: the
/// parts beside the head, the spawned parts on the level (only this dragon makes any here), how
/// many of those share a tile with another and how many stand anywhere but beside the head; then
/// one more step the same way ([`dragon_where`] found the tile free) and two more reloads.
fn missing_parts_made(dragon: Dragon, keep: usize) {
    let Dragon {
        mut s, head, dir, ..
    } = dragon;
    let z = s.objects.z(head as Slot);
    let beside = |s: &mut State| parts_beside(s, head).iter().filter(|&&p| p >= 0).count();
    // Spawned parts on the level, those on a tile with another, those not beside the head.
    let tally = |s: &mut State| {
        let found: Vec<Position> = parts_beside(s, head)
            .iter()
            .filter(|&&c| c >= 0)
            .map(|&c| s.objects.location[c as usize])
            .collect();
        let all = spawned_parts(s, z);
        let mut tiles = all.clone();
        tiles.sort_by_key(|a| (a.y, a.x));
        tiles.dedup();
        let elsewhere = all.iter().filter(|a| !found.contains(a)).count();
        (all.len(), all.len() - tiles.len(), elsewhere)
    };
    assert!(
        step(&mut s, head, dir, 0),
        "the turning step (direction {dir}) moved"
    );
    let whole = beside(&mut s);
    assert!(
        whole == 4,
        "after the turning step: parts beside the head {whole} of 4"
    );
    let behind = SIDES
        .iter()
        .position(|&d| d == usize::from((dir + 4) & 6))
        .expect("a side");
    for (k, c) in parts_beside(&mut s, head).into_iter().enumerate() {
        if keep == 0 || k != behind {
            remove(&mut s, c);
        }
    }
    let saved = beside(&mut s);
    let (saved_on_level, ..) = tally(&mut s);
    assert!(
        saved == keep && saved_on_level == keep,
        "the save's shape: parts beside the head {saved}, spawned parts on the level \
         {saved_on_level} (kept {keep})"
    );
    reload(&mut s);
    let after = beside(&mut s);
    let (on_level, doubled, elsewhere) = tally(&mut s);
    let untyped = untyped_in_list(&s);
    let moved = step(&mut s, head, dir, 0);
    let stepped = beside(&mut s);
    let (stepped_on_level, ..) = tally(&mut s);
    let mut again = Vec::new();
    for _ in 0..2 {
        reload(&mut s);
        let b = beside(&mut s);
        let (n, d, e) = tally(&mut s);
        again.push((b, n, d, e));
    }
    println!(
        "kept {keep} of 4 (direction {dir}): after the reload parts beside the head {after} of 4, \
         spawned parts on the level {on_level}, on a tile with another {doubled}, not beside the \
         head {elsewhere}, list entries without a type {untyped}; one more step moved {moved}, \
         parts beside the head {stepped} of 4, on the level {stepped_on_level}; two more reloads \
         (beside, on the level, doubled, not beside) {again:?}"
    );
    assert!(
        after == 4 && on_level == 4 && doubled == 0 && elsewhere == 0 && untyped == 0,
        "kept {keep}: after the reload parts beside the head {after} of 4, spawned parts on the \
         level {on_level}, on a tile with another {doubled}, not beside the head {elsewhere}, \
         list entries without a type {untyped}"
    );
    assert!(
        moved && stepped == 4 && stepped_on_level == 4,
        "kept {keep}: one more step (direction {dir}) moved {moved}, parts beside the head \
         {stepped} of 4, spawned parts on the level {stepped_on_level}"
    );
    assert!(
        again == [(4, 4, 0, 0), (4, 4, 0, 0)],
        "kept {keep}: two more reloads (beside, on the level, doubled, not beside) {again:?}"
    );
}

/// Port-only repair (D3): a part made for a side 0x10b922 cannot find is removed again, so none
/// is left behind and none piles up. On the surface at x 0 the west side's tile is x 0x3ff
/// (0x10bcee masks it), but 0x10b922 adds −1 unmasked and its packed location (0x134e1c) carries
/// into y, so the part there is found only on a row whose low six bits are all set. A dragon made
/// by 0x131646 at x 0 on a row that is not one, the view one tile east of it so the region wraps
/// (the west side's superchunk in slots as well as the head's): three of its parts are found;
/// after each of three reloads it still has those three, and no spawned part stands anywhere
/// else.
#[test]
fn a_part_past_the_world_edge_is_not_left_on_load() {
    let Some((mut s, _files)) = start() else {
        return;
    };
    let c = s.map.centre;
    assert!(c.z == 0, "the start is on the surface");
    let y = if c.y & 0x3f == 0x3f { c.y - 1 } else { c.y };
    // One tile from x 0, closer than the margin 0x10e1ce keeps from a superchunk edge, so the
    // region's base moves west across the world's edge.
    look_at(&mut s, Position { x: 1, y, z: 0 });
    let block = |px: u16| ((y >> 7) * 8 + (px >> 7)) as i16;
    let keyed = |s: &State| [0x3ff, 0].map(|px| s.objects.loaded_surface.contains(&block(px)));
    println!(
        "view near x 0: superchunks of x 0x3ff and x 0 in slots {:?}",
        keyed(&s)
    );
    assert!(
        keyed(&s) == [true, true],
        "the region wraps: superchunks of x 0x3ff and x 0 in slots {:?}",
        keyed(&s)
    );
    let m = spawn_monster(&mut s, DRAGON, 0, y, 0);
    assert!(
        (0..ACTOR_SLOTS as Slot).contains(&m),
        "the dragon takes an actor slot"
    );
    let head = m as u16;
    let beside = |s: &mut State| parts_beside(s, head).iter().filter(|&&p| p >= 0).count();
    let not_beside = |s: &mut State| {
        let found: Vec<Position> = parts_beside(s, head)
            .iter()
            .filter(|&&c| c >= 0)
            .map(|&c| s.objects.location[c as usize])
            .collect();
        spawned_parts(s, 0)
            .iter()
            .filter(|a| !found.contains(a))
            .count()
    };
    let made = (beside(&mut s), not_beside(&mut s));
    let mut seen = Vec::new();
    for _ in 0..3 {
        reload(&mut s);
        seen.push((beside(&mut s), not_beside(&mut s)));
    }
    println!(
        "dragon at x 0: made (beside, not beside) {made:?}; after each of three reloads {seen:?}"
    );
    assert!(
        made == (3, 1),
        "the dragon at x 0 as made: parts beside it and spawned parts elsewhere {made:?}"
    );
    assert!(
        seen == [(3, 0), (3, 0), (3, 0)],
        "after each of three reloads: parts beside it and spawned parts elsewhere {seen:?}"
    );
}

/// 0x102f3c on tile 2 (asm 0x1030b2..0x1030e2) for the new game's leader: a readied type-0x1c
/// child keeps the poison off; the same child carried (status 0x10) does not.
#[test]
fn the_leader_with_a_readied_0x1c_is_not_poisoned_on_tile_2() {
    use crate::objects::{IN_INVENTORY, READIED, WHERE_MASK, create_in, find_child};
    let Some((mut s, _files)) = start() else {
        return;
    };
    let l = leader(&s);
    let a = usize::from(l);
    let held = find_child(&mut s.objects, l as Slot, 0x1c, -1);
    println!(
        "leader slot {l}: alive and not poisoned {}, a type-0x1c child before: {}",
        s.actors.attrs.status[a] & 0x18 == 0,
        held >= 0
    );
    assert!(s.actors.attrs.status[a] & 0x18 == 0, "alive, not poisoned");
    let child = create_in(&mut s, 0x1c, READIED, 0, l as Slot);
    assert!(child >= 0, "a free slot");
    party::tile_effect(&mut s, l, 2);
    let readied = s.actors.attrs.status[a] & 0x08;
    let st = &mut s.objects.obj_status[child as usize];
    *st = *st & !WHERE_MASK | IN_INVENTORY;
    party::tile_effect(&mut s, l, 2);
    let carried = s.actors.attrs.status[a] & 0x08;
    println!("poison bit after tile 2: readied {readied:#04x}, carried {carried:#04x}");
    assert_eq!((readied, carried), (0, 0x08));
}

/// Every object `member` carries at any depth (0x10c928 over the object table; a removed
/// object's slot keeps its location and status, only its type word is 0).
fn carried(s: &State, member: Slot) -> Vec<Slot> {
    (0..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&c| s.objects.tf(c) != 0 && s.objects.contained_in(c, member))
        .collect()
}

/// 0x11fd20 answered Y, in the town of the cell: the party goes to (0xe7, 0xba, 0) (D0 = 0xe7,
/// D1 = 0xba into 0x1092fc, asm 0x11fd8c..0x11fd9a); then each member's type-0x3f objects are
/// removed, its type-0x40 objects of quality 9 go into the first type 0xb1 at (0xeb, 0xb7, 0),
/// and the first type 0x12c at (0xe7, 0xb8, 0) gets frame 9 (asm 0x11fdb0..0x11ff20). Taken
/// objects are given to the first and the last member. A type 0x40 of another quality and
/// everything else the members carry stay, each in its slot with its type and quality words.
#[test]
fn an_arrest_jails_at_the_cell_and_takes_types_3f_and_40_of_quality_9() {
    let Some((mut s, files)) = start() else {
        return;
    };
    crate::map::travel::teleport(&mut s, 0xe7, 0xc2, 0, 0);
    let door = crate::objects::object_of_kind_at(&mut s, 0xe7, 0xb8, 0, 0x12c);
    assert!(
        door >= 0,
        "a type 0x12c at (0xe7, 0xb8, 0) once the town is loaded"
    );
    println!(
        "type 0x12c at (0xe7, 0xb8, 0): frame {} as shipped",
        s.objects.frame(crate::objects::ObjectId(door as u16))
    );
    s.objects.type_frame[door as usize] &= 0x3ff;
    s.actors.attrs.talk_flags[5] |= 0x80;
    let members: Vec<Slot> = s.actors.party.active().map(|m| m.0 as Slot).collect();
    // The arrest only sets the arresting actor's worktype: any actor outside the party.
    let guard = (1..ACTOR_SLOTS as Slot)
        .find(|&a| s.objects.kind(a) != 0 && !members.contains(&a))
        .quiet() as u16;
    let (m0, last) = (members[0], members[members.len() - 1]);
    let add = |s: &mut State, owner: Slot, tf: u16, qq: u16| {
        let c = crate::objects::create_in(s, tf, crate::objects::IN_INVENTORY, qq, owner);
        assert!(c >= 0, "a free slot for the carried object");
        s.objects.obj_status[c as usize] &= !crate::objects::TEMPORARY;
        c
    };
    let taken = |s: &State, c: Slot| {
        let k = s.objects.kind(c);
        k == 0x3f || (k == 0x40 && s.objects.qq_of(c) >> 8 == 9)
    };
    let shipped: Vec<(usize, usize)> = members
        .iter()
        .map(|&m| {
            let c = carried(&s, m);
            (c.len(), c.iter().filter(|&&c| taken(&s, c)).count())
        })
        .collect();
    println!(
        "members {}: carried (all, of 0x3f / 0x40 q9) {shipped:?}",
        members.len()
    );
    add(&mut s, m0, 0x3f, 1);
    let q9 = add(&mut s, m0, 0x40, 9 << 8 | 1);
    let q8 = add(&mut s, m0, 0x40, 8 << 8 | 1);
    add(&mut s, last, 0x3f, 1);
    let pinned: Vec<Vec<(Slot, u16, u16)>> = members
        .iter()
        .map(|&m| {
            carried(&s, m)
                .into_iter()
                .filter(|&c| !taken(&s, c))
                .map(|c| (c, s.objects.tf(c), s.objects.qq_of(c)))
                .collect()
        })
        .collect();
    let kept: Vec<usize> = pinned.iter().map(Vec::len).collect();

    let mut engine = Engine::with_game(s, files, move |st, io| async move {
        super::npc::arrest(&st, &io, guard).await;
    });
    for _ in 0..2000 {
        if engine.finished() {
            break;
        }
        engine.tick(&[crate::InputEvent::Key(crate::Key::Char('y'))], 16);
    }
    assert!(engine.finished(), "the arrest returned");
    let mut s = engine.state().clone();
    let c = s.map.centre;
    let holder = crate::objects::object_of_kind_at(&mut s, 0xeb, 0xb7, 0, 0xb1);
    let door = crate::objects::object_of_kind_at(&mut s, 0xe7, 0xb8, 0, 0x12c);
    let got = (
        (c.x, c.y, c.z),
        members
            .iter()
            .map(|&m| carried(&s, m).len())
            .collect::<Vec<_>>(),
        members
            .iter()
            .map(|&m| carried(&s, m).iter().filter(|&&c| taken(&s, c)).count())
            .collect::<Vec<_>>(),
        members
            .iter()
            .zip(&pinned)
            .map(|(&m, p)| {
                p.iter()
                    .filter(|&&(c, tf, qq)| {
                        s.objects.contained_in(c, m)
                            && s.objects.tf(c) == tf
                            && s.objects.qq_of(c) == qq
                    })
                    .count()
            })
            .collect::<Vec<_>>(),
        holder >= 0 && s.objects.parent(q9) == holder && s.objects.place(q9) == 0x08,
        s.objects.contained_in(q8, m0),
        (door >= 0).then(|| s.objects.frame(crate::objects::ObjectId(door as u16))),
    );
    println!(
        "after: centre, carried per member, of 0x3f / 0x40 q9, of the pinned unchanged, the q9 \
         0x40 in the 0xb1, the q8 0x40 kept, the 0x12c's frame {got:?}"
    );
    let want = (
        (0xe7, 0xba, 0),
        kept.clone(),
        vec![0; members.len()],
        kept,
        true,
        true,
        Some(9),
    );
    assert_eq!(got, want);
}

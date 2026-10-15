//! Real-data receipts for the world turn: the user's archive.org zip → u6-intake → `Files` →
//! `State::new_game`, then world turns and party steps. Skips where the zip is missing (never
//! in CI: the data is not ours to ship; it is only read here and the tests print numbers only).
#![allow(clippy::disallowed_methods)] // tests may read files

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use u6_formats::position::Position;
use u6_intake::{InputFile, intake};

use crate::engine::Engine;
use crate::files::Files;
use crate::state::State;

use super::party::{self, StepResult};
use super::{GameTables, clock, path, turn};
use u6_testkit::Quiet;

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
    // Avatar died around pass 70 (resurrected in the castle), so the schedules this receipt
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
    // The first hour boundary at which an NPC in view (within 5 tiles, where NPCs walk
    // instead of jumping) is sent somewhere else: run the hourly pick on copies.
    let in_view: Vec<u16> = near
        .iter()
        .filter(|&&(d, _)| d <= 5)
        .map(|&(_, a)| a)
        .collect();
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
        "no fight at the start: nothing hurts the Avatar"
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

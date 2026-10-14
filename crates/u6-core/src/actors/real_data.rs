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
use crate::objects::ObjectId;
use crate::state::State;

use super::party::{self, StepResult};
use super::seams::{self, Found};
use super::{GameTables, clock, path, turn, walk};

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

/// The start state with the `game` tables loaded (the wiring WU-C0 adds to `State::new_game`).
fn start() -> Option<(State, Files)> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).unwrap();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    let files = Files::new(r.files);
    let mut state = State::new_game(&files).unwrap_or_else(|e| panic!("{e}"));
    state.actors.game = GameTables::from_game(files.get("game").unwrap()).unwrap();
    Some((state, files))
}

fn chebyshev(a: Position, b: Position) -> i16 {
    let dx = (a.x as i16 - b.x as i16).abs();
    let dy = (a.y as i16 - b.y as i16).abs();
    dx.max(dy)
}

fn leader(s: &State) -> u16 {
    s.actors.party.leader().unwrap().0
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
    assert_eq!(
        state.actors.worktype[usize::from(l)],
        2,
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
    println!("NPCs in view {in_view:?}; first boundary for them {boundary:02}:00");
    let before = state.clone();
    let c0 = state.actors.calendar;
    let trace: Rc<RefCell<Trace>> = Rc::new(RefCell::new(Vec::new()));
    let (tr, ids) = (Rc::clone(&trace), watched.clone());
    let mut engine = Engine::with_game(state, files, move |st, io| async move {
        let mut after = 0;
        // Pass until the boundary hour, then 60 more passes (about an hour) of walking.
        while after < 60 && tr.borrow().len() < 2000 {
            pass(&st, &io).await;
            let s = st.borrow();
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
    let c1 = after.actors.calendar;
    let trace = trace.borrow();
    println!(
        "{} passes: {:02}:{:02} day {} -> {:02}:{:02} day {}",
        trace.len(),
        c0.hour,
        c0.minute,
        c0.day,
        c1.hour,
        c1.minute,
        c1.day
    );
    assert_ne!(c0.hour, c1.hour, "the clock crossed an hour");
    let mut walked_closer = 0;
    for (k, &a) in watched.iter().enumerate() {
        let i = usize::from(a);
        let e = path::schedule_entry(&after, a);
        let (p0, p1) = (before.objects.location[i], after.objects.location[i]);
        let goal = e.map(|e| e.pos).unwrap_or(p0);
        // Path-walking passes that got closer to the entry, and the other moves.
        let (mut walks, mut closer, mut others) = (0, 0, 0);
        let mut prev = p0;
        let mut path_log = Vec::new();
        for (n, row) in trace.iter().enumerate() {
            let (p, wt) = row[k];
            match classify(prev, p, wt) {
                Move::Stay => {}
                Move::Walk => {
                    walks += 1;
                    if chebyshev(p, goal) < chebyshev(prev, goal) {
                        closer += 1;
                    }
                    if path_log.len() < 8 {
                        path_log.push(format!("{n}:({},{})", p.x, p.y));
                    }
                }
                Move::Other => others += 1,
            }
            prev = p;
        }
        if closer >= 2 {
            walked_closer += 1;
        }
        let e_old = path::schedule_entry(&before, a);
        println!(
            "actor {a:3} type {:#05x} wt {:#04x}->{:#04x} ({},{},{})->({},{},{}) \
             entry {:?}->{:?} dist {}->{} path-walk passes {walks} (closer {closer}) \
             other moves {others} walk {}",
            before.objects.type_frame[i] & 0x3ff,
            before.actors.worktype[i],
            after.actors.worktype[i],
            p0.x,
            p0.y,
            p0.z,
            p1.x,
            p1.y,
            p1.z,
            e_old.map(|e| (e.hour, e.worktype, e.pos.x, e.pos.y)),
            e.map(|e| (e.hour, e.worktype, e.pos.x, e.pos.y)),
            chebyshev(p0, goal),
            chebyshev(p1, goal),
            path_log.join(" "),
        );
    }
    println!("{walked_closer} watched NPCs walked a path toward their entry in 2+ passes");
    for (id, n) in engine.stub_hits() {
        println!("stub {n:6} × {id}");
    }
    assert!(
        walked_closer > 0,
        "an NPC walked toward its scheduled place"
    );
}

/// Why the leader cannot step from `from` in direction `d` (what 0x11b1c0 looks at).
fn why_blocked(s: &mut State, l: u16, d: usize) -> String {
    let p = s.objects.location[usize::from(l)];
    let (nx, ny) = (
        p.x.wrapping_add_signed(walk::DX8[d]),
        p.y.wrapping_add_signed(walk::DY8[d]),
    );
    let z = u16::from(p.z);
    let mut why = Vec::new();
    let t = seams::tile_at(s, nx, ny, z);
    let f = walk::flags1(s, u16::from(t));
    if f & 0x02 != 0 {
        why.push(format!("map tile {t:#04x} impassable"));
    }
    for Found { id, part } in seams::objects_at(s, nx, ny, z) {
        let w = seams::obj_word(s, id);
        let tile = walk::word_tile(s, w).wrapping_sub(part);
        let what = if id.0 < 0x100 {
            format!("actor {} (type {:#05x})", id.0, w & 0x3ff)
        } else {
            format!(
                "object type {:#05x} tile {tile:#05x} flags1 {:#04x}",
                w & 0x3ff,
                walk::flags1(s, tile)
            )
        };
        why.push(what);
    }
    if walk::DX8[d] != 0 && walk::DY8[d] != 0 && walk::can_move_to(s, l, nx, ny) {
        why.push("diagonal: both orthogonal neighbours blocked".into());
    }
    if why.is_empty() {
        why.push("nothing found at the target".into());
    }
    format!("({nx},{ny}) {}", why.join("; "))
}

#[test]
fn party_steps_five_tiles_each_way() {
    let Some((start, _files)) = start() else {
        return;
    };
    let names = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    for (d, name) in names.iter().enumerate() {
        let mut s = start.clone();
        let l = leader(&s);
        let p0 = s.objects.location[usize::from(l)];
        let mut log = Vec::new();
        for _ in 0..5 {
            s.actors.movement_points[usize::from(l)] = 30;
            let from = s.objects.location[usize::from(l)];
            match party::player_step(&mut s, d as u16) {
                StepResult::Moved => {
                    let to = s.objects.location[usize::from(l)];
                    assert_eq!((s.map.centre.x, s.map.centre.y), (to.x, to.y));
                    assert_ne!(from, to);
                    log.push(format!("({},{})", to.x, to.y));
                }
                other => {
                    let why = why_blocked(&mut s, l, d);
                    assert_eq!(s.objects.location[usize::from(l)], from);
                    log.push(format!("{other:?} at {why}"));
                }
            }
        }
        let followers: Vec<String> = s
            .actors
            .party
            .active()
            .filter(|&m| m != ObjectId(l))
            .map(|m| {
                let q = s.objects.location[m.index()];
                format!("{}@({},{})", m.0, q.x, q.y)
            })
            .collect();
        println!(
            "{name:2} from ({},{}): {} | followers {}",
            p0.x,
            p0.y,
            log.join(" "),
            followers.join(" ")
        );
    }
}

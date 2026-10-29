//! Spec 21: the travel search on crafted maps, the walk's stops through an engine whose game
//! reads one key a prompt and steps the party with it, and the real-data receipts on the user's
//! zip (counts only; skipped without it).

use std::cell::RefCell;
use std::rc::Rc;

use u6_formats::position::Position;

use super::*;
use crate::actors::party::StepResult;
use crate::actors::tail;
use crate::engine::{Engine, Io, POINTER_LEFT};
use crate::files::Files;
use crate::geom;

/// The leader's cell in the open crafted maps: window origin 0 in both builds (as `path.rs`'s
/// tests).
const C: u16 = geom::LEAD + 4;
/// The room of the walking tests: cells `R..R + 8` on both axes, walled round, where the window's
/// origin stays 0 whatever cell the leader stands on (0x108d10 rebuilds the window cache only
/// when the leader leaves the chunk centred in it), so the crafted tiles stay.
const R: u16 = geom::LEAD;

/// Floor (tile 0), wall, water, a costly floor (cost nibble 4), a damaging floor.
const WALL: u8 = 1;
const WATER: u8 = 2;
const COSTLY: u8 = 3;
const HARM: u8 = 4;
/// The door type the crafted maps use (0x129) and its base tile: frames 0..3 open, 4..7 closed
/// (as the real doors: `door_at_actor` writes 4 × closed + the side).
const DOOR: u16 = 0x129;
const DOOR_TILE: u16 = 200;
/// The object slot of a crafted door, and of a barrier (0x1a0).
const DOOR_SLOT: usize = 0x200;
const BARRIER_SLOT: usize = 0x201;

/// An open window (every tile floor) with leader 1 alone at (C, C), the prompt up.
fn open_map() -> State {
    let mut s = State::empty();
    crate::actors::walk::tests::creature_tiles(&mut s);
    s.map.world.surface = vec![0; u6_formats::map::SURFACE_CHUNKS.pow(2)];
    s.map.window_tiles = vec![0; crate::map::WINDOW_CELLS];
    s.map.window_origin = [0, 0];
    {
        let f = &mut s.tables.tileflags.flags1;
        f[usize::from(WALL)] = 0x06;
        f[usize::from(WATER)] = 0x03;
        f[usize::from(COSTLY)] = 0x40;
        f[usize::from(HARM)] = 0x08;
        f[usize::from(DOOR_TILE) + 4..usize::from(DOOR_TILE) + 8].fill(0x02);
    }
    s.tables.basetile.base[usize::from(DOOR)] = DOOR_TILE;
    s.actors.set_tail(tail::SOLO_MEMBER, 0xff);
    s.actors.party.members[0] = 1;
    s.actors.party.size = 1;
    s.map.centre = Position { x: C, y: C, z: 0 };
    s.objects.type_frame[1] = 0x17a;
    s.objects.location[1] = Position { x: C, y: C, z: 0 };
    s.actors.attrs.status[1] = 0x80;
    s.actors.worktype[1] = 2;
    s.actors.movement_points[1] = 30;
    s.main.program = Program::Game;
    s.main.reading = 1;
    s.ui.input.accept = true;
    s.ui.input.in_wait = true;
    let mut formation = vec![0; 28];
    formation[12] = 1;
    formation[13] = 2;
    s.actors.game = crate::actors::GameTables {
        formation,
        ..crate::actors::GameTables::default()
    };
    crate::actors::walk::tests::link_objects(&mut s);
    s
}

/// [`open_map`] with the leader at `at` in the walled room `R..R + 8`, the window as 0x108d10
/// left it (its key and view offset), so a step does not fill the window again.
fn room(at: (u16, u16)) -> State {
    let mut s = open_map();
    for i in R - 1..=R + 8 {
        for (x, y) in [(i, R - 1), (i, R + 8), (R - 1, i), (R + 8, i)] {
            tile(&mut s, x, y, WALL);
        }
    }
    s.map.window_key = [0, 0, 0];
    put_leader(&mut s, at);
    s
}

fn put_leader(s: &mut State, (x, y): (u16, u16)) {
    s.objects.location[1] = Position { x, y, z: 0 };
    s.map.centre = Position { x, y, z: 0 };
    s.map.view_offset = [
        geom::view_offset(x, 0, geom::HALF_W as u16),
        geom::view_offset(y, 0, geom::HALF_H as u16),
    ];
    crate::actors::walk::tests::link_objects(s);
}

fn tile(s: &mut State, x: u16, y: u16, t: u8) {
    s.map.window_tiles[crate::map::cell(usize::from(x), usize::from(y))] = t;
}

/// Actor `a` (not in the party, type 0x17a) at (x, y).
fn npc(s: &mut State, a: usize, x: u16, y: u16) {
    s.objects.type_frame[a] = 0x17a;
    s.objects.location[a] = Position { x, y, z: 0 };
    crate::actors::walk::tests::link_objects(s);
}

/// A door at (x, y), open (frame 1) or closed (frame 5).
fn door(s: &mut State, x: u16, y: u16, open: bool) {
    let frame: u16 = if open { 1 } else { 5 };
    s.objects.type_frame[DOOR_SLOT] = DOOR | frame << 10;
    s.objects.location[DOOR_SLOT] = Position { x, y, z: 0 };
    crate::actors::walk::tests::link_objects(s);
}

fn leader_at(s: &State) -> (u16, u16) {
    let p = s.objects.location[1];
    (p.x, p.y)
}

/// The planned path from the leader to `to` on `s`.
fn path(s: &State, to: (u16, u16)) -> Option<Vec<u8>> {
    plan(s, 1, leader_at(s), to).path
}

/// Walks `path` with the player's step on a copy of `s`, one step a direction: every step moves
/// the leader, and it ends on `to` (the path is what the keys walk).
fn walk_keys(s: &State, path: &[u8], to: (u16, u16)) {
    let mut c = s.clone();
    for (i, &d) in path.iter().enumerate() {
        let before = leader_at(&c);
        assert_eq!(
            party::player_step(&mut c, u16::from(d)),
            StepResult::Moved,
            "step {i} ({d}) is one the keys take"
        );
        let want = party::step_cell(before, usize::from(d));
        assert_eq!(leader_at(&c), want, "step {i} lands where it aims");
    }
    assert_eq!(leader_at(&c), to, "the keys end on the target");
}

/// The cells `path` visits from `from`, the target last.
fn cells(from: (u16, u16), path: &[u8]) -> Vec<(u16, u16)> {
    let mut at = from;
    path.iter()
        .map(|&d| {
            at = party::step_cell(at, usize::from(d));
            at
        })
        .collect()
}

#[test]
fn the_search_goes_round_a_wall_and_its_path_is_what_the_keys_walk() {
    let mut s = room((R + 1, R + 3));
    for y in R + 1..=R + 6 {
        tile(&mut s, R + 3, y, WALL);
    }
    let to = (R + 5, R + 3);
    let p = path(&s, to).expect("a way round");
    assert!(
        cells((R + 1, R + 3), &p)
            .iter()
            .all(|&(x, y)| x != R + 3 || !(R + 1..=R + 6).contains(&y)),
        "no step into the wall: {p:?}"
    );
    walk_keys(&s, &p, to);
}

#[test]
fn water_is_not_walked_the_gap_is() {
    let mut s = room((R + 1, R + 2));
    for x in R..R + 8 {
        if x != R + 6 {
            tile(&mut s, x, R + 4, WATER);
        }
    }
    let to = (R + 1, R + 6);
    let p = path(&s, to).expect("through the gap");
    assert!(
        cells((R + 1, R + 2), &p).contains(&(R + 6, R + 4)),
        "the gap: {p:?}"
    );
    walk_keys(&s, &p, to);
    tile(&mut s, R + 6, R + 4, WATER);
    assert_eq!(reachable(&s, to), Some(Reason::Unreachable), "no gap");
}

#[test]
fn an_open_door_lets_the_path_through_and_a_shut_one_does_not() {
    let mut s = room((R + 1, R + 3));
    for y in R..R + 8 {
        if y != R + 3 {
            tile(&mut s, R + 3, y, WALL);
        }
    }
    door(&mut s, R + 3, R + 3, true);
    let to = (R + 5, R + 3);
    let p = path(&s, to).expect("through the open door");
    assert!(
        cells((R + 1, R + 3), &p).contains(&(R + 3, R + 3)),
        "the door: {p:?}"
    );
    walk_keys(&s, &p, to);
    door(&mut s, R + 3, R + 3, false);
    assert_eq!(reachable(&s, to), Some(Reason::Unreachable), "shut");
}

#[test]
fn a_diagonal_goes_only_past_an_open_side() {
    let mut s = room((R + 1, R + 3));
    tile(&mut s, R + 2, R + 3, WALL);
    tile(&mut s, R + 1, R + 2, WALL);
    let to = (R + 2, R + 2);
    let mut c = s.clone();
    assert_eq!(
        party::player_step(&mut c, 1),
        StepResult::Blocked,
        "the keys refuse the shut corner"
    );
    assert!(
        c.ui.text == s.ui.text
            && c.ui.messages == s.ui.messages
            && c.main.pending_print == s.main.pending_print,
        "no line printed or queued"
    );
    assert_eq!(
        c.actors.movement_points, s.actors.movement_points,
        "no points spent"
    );
    assert_eq!(leader_at(&c), (R + 1, R + 3), "not moved");
    let p = path(&s, to).expect("a way round");
    assert!(p.len() > 1, "not the corner: {p:?}");
    walk_keys(&s, &p, to);
    tile(&mut s, R + 1, R + 2, 0);
    assert_eq!(path(&s, to), Some(vec![1]), "one side open: the diagonal");
}

/// Spec 21 round 2 (review SHOULD-1): the harm-free pass refuses harm on the cell a step enters
/// only; a diagonal's side cells are tested as the key's step tests them (0x11d4ee, the switch
/// clear), so a diagonal past harm beside it is the one step a key takes.
#[test]
fn a_diagonal_past_harm_beside_it_is_the_keys_one_step() {
    let mut s = room((R + 1, R + 3));
    tile(&mut s, R + 1, R + 2, WALL);
    tile(&mut s, R + 2, R + 3, HARM);
    assert_eq!(
        party::player_step(&mut s.clone(), 1),
        StepResult::Moved,
        "the key's diagonal"
    );
    assert_eq!(path(&s, (R + 2, R + 2)), Some(vec![1]), "the same one step");
}

/// Review opus NIT 1: the search's cost of a cell is 0x11ef84's with the leader standing on it: the
/// leader's own cell as `tile_cost` reads it there (the leader counted), any other cell plus the
/// leader's own tile (cost nibble 4 here).
#[test]
fn a_cells_cost_counts_the_leader_standing_on_it() {
    let mut s = room((R + 1, R + 3));
    let t = walk::word_tile(&s, s.objects.type_frame[1]);
    s.tables.tileflags.flags1[usize::from(t)] = 0x40;
    let w = Window::of(&s);
    let mut grid = Grid::new(&s, w, 1, 0);
    let home = w.cell((R + 1, R + 3)).expect("in the window");
    let other = w.cell((R + 2, R + 3)).expect("in the window");
    assert_eq!(
        walk::tile_cost(&s, R + 1, R + 3, 0),
        9,
        "0x11ef84 counts the leader where it stands"
    );
    assert_eq!(walk::tile_cost(&s, R + 2, R + 3, 0), 5);
    assert_eq!(grid.cost(&s, home), 9, "its own cell");
    assert_eq!(grid.cost(&s, other), 9, "another cell, the leader on it");
}

#[test]
fn diagonals_win_a_tie_of_points_and_cheap_tiles_win_over_steps() {
    let s = room((R, R));
    assert_eq!(
        path(&s, (R + 3, R + 3)),
        Some(vec![3, 3, 3]),
        "three diagonals pay what six straight steps pay, in half the steps"
    );
    let mut s = room((R + 1, R + 3));
    tile(&mut s, R + 2, R + 3, COSTLY);
    assert_eq!(
        path(&s, (R + 3, R + 3)),
        Some(vec![2, 2]),
        "two straight steps over the costly cell pay 14, two diagonals 20 (5 more each)"
    );
    let mut s = room((R + 1, R + 3));
    for x in R + 2..=R + 4 {
        tile(&mut s, x, R + 3, COSTLY);
    }
    let to = (R + 5, R + 3);
    let p = path(&s, to).expect("a way");
    assert!(
        cells((R + 1, R + 3), &p)
            .iter()
            .all(|&(x, y)| y != R + 3 || !(R + 2..=R + 4).contains(&x)),
        "round the costly cells (32 points straight, 30 round): {p:?}"
    );
    walk_keys(&s, &p, to);
}

#[test]
fn harm_is_walked_only_when_there_is_no_other_way() {
    let mut s = room((R + 1, R + 3));
    tile(&mut s, R + 2, R + 3, HARM);
    tile(&mut s, R + 3, R + 3, HARM);
    let to = (R + 4, R + 3);
    let harmed = |p: &[u8]| {
        cells((R + 1, R + 3), p)
            .iter()
            .any(|&(x, y)| y == R + 3 && (x == R + 2 || x == R + 3))
    };
    let p = path(&s, to).expect("a way");
    assert!(!harmed(&p), "round the harm: {p:?}");
    for x in R + 1..=R + 4 {
        tile(&mut s, x, R + 2, WALL);
        tile(&mut s, x, R + 4, WALL);
    }
    tile(&mut s, R, R + 3, WALL);
    tile(&mut s, R + 5, R + 3, WALL);
    let p = path(&s, to).expect("only through");
    assert_eq!(p, vec![2, 2, 2], "through the harm, as a key walks");
    walk_keys(&s, &p, to);
}

#[test]
fn a_walled_off_cell_is_unreachable_after_every_open_cell_of_the_window() {
    let mut s = open_map();
    let to = (C + 5, C + 5);
    for dy in -1i16..=1 {
        for dx in -1i16..=1 {
            if (dx, dy) != (0, 0) {
                tile(
                    &mut s,
                    to.0.wrapping_add_signed(dx),
                    to.1.wrapping_add_signed(dy),
                    WALL,
                );
            }
        }
    }
    let mut c = s.clone();
    c.actors.work.hic = 0;
    let mut grid = Grid::new(&c, Window::of(&c), 1, 0);
    let one = search(&mut c, &mut grid, (C, C), to);
    assert_eq!(one.path, None);
    let cells = geom::WINDOW * geom::WINDOW;
    assert_eq!(
        one.expanded,
        cells - 9,
        "each open cell once: the window less the ring and the target"
    );
    assert_eq!(reachable(&s, to), Some(Reason::Unreachable));
    assert_eq!(
        plan(&s, 1, (C, C), to).expanded,
        2 * (cells - 9),
        "the harmless pass, then the pass a key walks"
    );
}

#[test]
fn the_refusals() {
    let s = room((R + 1, R + 3));
    assert_eq!(reachable(&s, (R + 1, R + 3)), Some(Reason::Here));
    assert_eq!(
        reachable(&s, (geom::WINDOW as u16 + 2, R)),
        Some(Reason::Far),
        "outside the window"
    );
    assert_eq!(reachable(&s, (R + 5, R + 1)), None);
    assert_eq!(
        reachable(&s, (R + 9, R + 3)),
        Some(Reason::Unreachable),
        "outside the room"
    );
    let mut combat = s.clone();
    combat.actors.set_tail(tail::COMBAT_MODE, 1);
    assert_eq!(reachable(&combat, (R + 5, R + 3)), Some(Reason::Combat));
    let mut aboard = s.clone();
    aboard.actors.party.leader_index = aboard.actors.party.size;
    aboard.actors.party.members[1] = 1;
    assert_eq!(reachable(&aboard, (R + 5, R + 3)), Some(Reason::Aboard));
    let mut foe = s.clone();
    in_view_foe(&mut foe, 0x20);
    assert_eq!(reachable(&foe, (R + 5, R + 3)), Some(Reason::Foe));
    let mut none = s.clone();
    none.objects.type_frame[1] = 0;
    assert_eq!(reachable(&none, (R + 5, R + 3)), Some(Reason::NoLeader));
}

/// Review opus N3: no travel starts while the command loop's program is not running.
#[test]
fn no_travel_starts_outside_the_game() {
    let mut s = room((R + 1, R + 3));
    assert_eq!(reachable(&s, (R + 5, R + 3)), None);
    s.main.program = Program::U;
    assert_eq!(reachable(&s, (R + 5, R + 3)), Some(Reason::NoGame));
}

/// Actor 9 two cells west of the leader with status alignment bits `align`, its view cell seen.
fn in_view_foe(s: &mut State, align: u8) {
    let (x, y) = leader_at(s);
    npc(s, 9, x - 2, y);
    s.actors.attrs.status[9] = align;
    s.map.view_flags = vec![0; crate::map::WINDOW_CELLS];
    let cell = s.map.view_cell(geom::HALF_W - 2, geom::HALF_H);
    s.map.view_flags[cell] = crate::map::flags::CELL_VISIBLE;
}

#[test]
fn a_foe_is_evil_or_chaotic_seen_and_alive() {
    for (align, foe) in [(0x20, true), (0x60, true), (0x40, false), (0, false)] {
        let mut s = room((R + 3, R + 3));
        in_view_foe(&mut s, align);
        assert_eq!(foe_in_view(&s, 1), foe, "alignment {align:#x}");
    }
    let mut s = room((R + 3, R + 3));
    in_view_foe(&mut s, 0x20);
    let mut dead = s.clone();
    dead.actors.attrs.status[9] |= 0x10;
    assert!(!foe_in_view(&dead, 1), "dead");
    let mut member = s.clone();
    member.actors.attrs.status[9] |= 0x80;
    assert!(!foe_in_view(&member, 1), "in the party");
    let mut unseen = s.clone();
    unseen.map.view_flags.fill(0);
    assert!(!foe_in_view(&unseen, 1), "a cell the leader does not see");
    let mut hidden = s.clone();
    hidden.objects.obj_status[9] |= 0x02;
    assert!(!foe_in_view(&hidden, 1), "invisible (0x1224ea)");
}

/// Spec 21 (review gpt b1-3, the claim as observed): `reachable` leaves the caller's state as it
/// was, its RNG included, and the copy a plan searches (`plan_on`, which `plan` and `reachable`
/// run on a clone) draws no RNG and queues no line. The clone inside `reachable` is not observed.
#[test]
fn the_search_reads_the_state_only_and_draws_no_rng() {
    let mut s = room((R + 1, R + 3));
    for y in R + 1..=R + 6 {
        tile(&mut s, R + 3, y, WALL);
    }
    tile(&mut s, R + 2, R + 4, HARM);
    // A barrier (0x1a0) the search tests: with the hiccup flag set, 0x11b1c0 would queue its
    // line there.
    s.objects.type_frame[BARRIER_SLOT] = 0x1a0;
    s.objects.location[BARRIER_SLOT] = Position {
        x: R + 2,
        y: R + 2,
        z: 0,
    };
    crate::actors::walk::tests::link_objects(&mut s);
    s.actors.work.hic = 1;
    let before = s.clone();
    assert_eq!(reachable(&s, (R + 5, R + 3)), None);
    assert!(s == before, "the state as it was, its RNG included");
    // The plan's own copy (what `reachable` and `plan` search on): no draw, no line.
    let mut c = s.clone();
    assert!(
        plan_on(&mut c, 1, (R + 1, R + 3), (R + 5, R + 3))
            .path
            .is_some()
    );
    assert_eq!(c.rng, s.rng, "the plan's copy drew no RNG");
    assert!(c.main.pending_print.is_empty(), "and queued no line");
    assert_eq!(
        c.actors.work.prompt, 0,
        "nor took the barrier's line (0x11b1c0 sets A4-0x7afd with it while A4-0x6a9c is set)"
    );
}

#[test]
fn every_reason_and_end_has_its_own_code_and_each_direction_its_key() {
    let mut codes: Vec<&str> = Reason::ALL.iter().map(|r| r.code()).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), Reason::ALL.len());
    let mut codes: Vec<&str> = End::ALL.iter().map(|e| e.code()).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), End::ALL.len());
    for (d, &(key, dx, dy)) in KEYPAD.iter().enumerate() {
        assert_eq!(key_of(d as u8), key, "direction {d}");
        assert_eq!((DX8[d], DY8[d]), (dx, dy), "direction {d} steps as its key");
    }
}

/// The oracle of the direction keys, written apart from [`key_of`]: each key with the step on the
/// map a player reads from it (x east, y south), north first and clockwise, as the PC keypad
/// lays them out (8 up, 9 up-right, 6 right, 3, 2, 1, 4, 7).
const KEYPAD: [(Key, i16, i16); 8] = [
    (Key::Up, 0, -1),
    (Key::UpRight, 1, -1),
    (Key::Right, 1, 0),
    (Key::DownRight, 1, 1),
    (Key::Down, 0, 1),
    (Key::DownLeft, -1, 1),
    (Key::Left, -1, 0),
    (Key::UpLeft, -1, -1),
];

/// The walk direction a key's step takes, by [`KEYPAD`] (not [`key_of`]): the 8-way direction
/// whose step is the key's.
fn dir_of(k: Key) -> Option<u8> {
    let &(_, dx, dy) = KEYPAD.iter().find(|(key, ..)| *key == k)?;
    (0..8u8).find(|&d| (DX8[usize::from(d)], DY8[usize::from(d)]) == (dx, dy))
}

/// What the stand-in game does after step `n` (from 1) it took, or at the read after `n` steps.
type After = Box<dyn Fn(&mut State, u32)>;

/// An engine whose game has the command loop's shape: at the prompt (A4-0x7a2c and A4-0x7a42 set,
/// in the event wait) it reads one key as the prompt does (the drained take,
/// [`crate::ui::keyread::take_drained`]), steps the party with a direction key's direction (the
/// player's step) and runs `after`, then takes 100 ms for the turn.
fn stand_in(s: State, after: After) -> Engine {
    stand_in_with(s, nothing(), 0, after)
}

/// [`stand_in`] with `prompt` run at each read once the prompt is set (with the steps taken so
/// far), and the read itself `lag` ms after it (the travel's key then waits in the queue).
fn stand_in_with(s: State, prompt: After, lag: u32, after: After) -> Engine {
    Engine::with_game(
        s,
        Files::default(),
        move |st: Rc<RefCell<State>>, io: Rc<Io>| async move {
            let mut n = 0;
            loop {
                {
                    let mut s = st.borrow_mut();
                    s.main.reading = 1;
                    s.ui.input.in_wait = true;
                    prompt(&mut s, n);
                }
                if lag > 0 {
                    io.sleep(lag).await;
                }
                // The read as the command prompt's (spec 19 O1c): the drained take, counted,
                // once a frame (`ui/keyread.rs` `poll`).
                let k = loop {
                    if let Some(k) = crate::ui::keyread::take_drained(&st, &io) {
                        break k;
                    }
                    io.vblank().await;
                };
                {
                    let mut s = st.borrow_mut();
                    s.main.reading = 0;
                    s.ui.input.in_wait = false;
                    if let Some(d) = dir_of(k) {
                        party::player_step(&mut s, u16::from(d));
                        n += 1;
                        after(&mut s, n);
                    }
                }
                io.sleep(100).await;
            }
        },
    )
}

/// Ticks of 20 ms with each of `inject` at its tick (from 1), until the travel ends or 400 ticks.
fn run(e: &mut Engine, inject: &[(u64, InputEvent)]) {
    for t in 1..=400u64 {
        let input: Vec<InputEvent> = inject
            .iter()
            .filter(|(at, _)| *at == t)
            .map(|(_, ev)| *ev)
            .collect();
        e.tick(&input, 20);
        if !e.travel_active() {
            return;
        }
    }
}

#[test]
fn the_travel_presses_one_step_a_prompt_and_stops_on_arrival() {
    let mut s = room((R + 1, R + 3));
    for y in R + 1..=R + 6 {
        tile(&mut s, R + 3, y, WALL);
    }
    let to = (R + 5, R + 3);
    let want = path(&s, to).expect("a way");
    let mut e = stand_in(s, Box::new(|_, _| {}));
    e.tick(&[], 20);
    e.travel_to(to).expect("reachable");
    assert!(e.travel_active());
    e.tick(&[], 20);
    assert_eq!(e.travel_log().len(), 1, "the first press at once");
    assert_eq!(e.io().pending_events(), 0, "taken in the tick it went in");
    run(&mut e, &[]);
    assert_eq!(e.travel_end(), Some(End::Arrived));
    assert_eq!(leader_at(&e.state()), to);
    let keys: Vec<Option<u8>> = e.travel_log().iter().map(|&(_, k)| dir_of(k)).collect();
    let want: Vec<Option<u8>> = want.into_iter().map(Some).collect();
    assert_eq!(keys, want, "the planned steps, one key each");
    let ticks: Vec<u64> = e.travel_log().iter().map(|&(t, _)| t).collect();
    assert!(
        ticks.windows(2).all(|w| w[1] - w[0] >= 5),
        "one a prompt, after each 100 ms turn: {ticks:?}"
    );
    let n = e.travel_log().len();
    for _ in 0..30 {
        e.tick(&[], 20);
    }
    assert_eq!(e.travel_log().len(), n, "nothing after the end");
}

/// The stand-in ([`stand_in_with`]: `prompt` at each read, `lag`, `after` after each step) on
/// the room with `setup` run on it, its travel east across the room started, (R + 1, R + 3) to
/// (R + 6, R + 3).
fn east(setup: impl FnOnce(&mut State), prompt: After, lag: u32, after: After) -> Engine {
    let mut s = room((R + 1, R + 3));
    setup(&mut s);
    let mut e = stand_in_with(s, prompt, lag, after);
    e.tick(&[], 20);
    e.travel_to((R + 6, R + 3)).expect("reachable");
    e
}

/// `e` run with each of `inject` at its tick: how its travel ended, and the keys pressed.
fn ended(mut e: Engine, inject: &[(u64, InputEvent)]) -> (Option<End>, usize) {
    run(&mut e, inject);
    (e.travel_end(), e.travel_log().len())
}

/// The walk east with `setup` run on the room, `after` after each step and each of `inject` at
/// its tick: how it ended, and the keys pressed.
fn walk_east(
    setup: impl FnOnce(&mut State),
    after: After,
    inject: &[(u64, InputEvent)],
) -> (Option<End>, usize) {
    ended(east(setup, nothing(), 0, after), inject)
}

/// [`run`] with no input, giving the most plans the travel had made at any tick it walked.
fn run_plans(e: &mut Engine) -> u32 {
    let mut most = 0;
    for _ in 0..400 {
        e.tick(&[], 20);
        match e.travel_progress() {
            Some(t) => most = most.max(t.plans),
            None => return most,
        }
    }
    most
}

fn nothing() -> After {
    Box::new(|_, _| {})
}

#[test]
fn the_walk_east_arrives_in_five_presses() {
    assert_eq!(walk_east(|_| {}, nothing(), &[]), (Some(End::Arrived), 5));
}

/// Spec 21 round 2 (review gpt-d MUST 1): a key, a held key's repeat (spec 19 O1c), text or a
/// press ends the travel in the tick it comes in, with no press of the travel's after it nor in
/// that tick: at the command prompt (where the travel would press) and in the middle of a turn.
#[test]
fn a_key_or_press_stops_it_in_its_tick_at_the_prompt_and_mid_turn() {
    for at_prompt in [true, false] {
        let phase = if at_prompt {
            "at the prompt"
        } else {
            "mid-turn"
        };
        for (ev, name) in [
            (InputEvent::Key(Key::Char('a')), "a key"),
            (InputEvent::Repeat(Key::Left), "a repeat"),
            (InputEvent::Text('a'), "text"),
            (
                InputEvent::Pointer {
                    x: 9,
                    y: 9,
                    buttons: 1,
                },
                "a left press",
            ),
            (
                InputEvent::Pointer {
                    x: 9,
                    y: 9,
                    buttons: 2,
                },
                "a right press",
            ),
        ] {
            let mut e = east(|_| {}, nothing(), 0, nothing());
            for _ in 0..400 {
                if e.travel_log().len() >= 2 && command_prompt(&e.state()) == at_prompt {
                    break;
                }
                e.tick(&[], 20);
            }
            assert!(e.travel_active(), "{name} {phase}: still walking");
            assert!(
                e.travel_log().len() >= 2 && command_prompt(&e.state()) == at_prompt,
                "{name} {phase}: reached"
            );
            let (n, t) = (e.travel_log().len(), e.ticks() + 1);
            e.tick(&[ev], 20);
            assert_eq!(e.travel_end(), Some(End::Input), "{name} {phase}");
            assert_eq!(e.travel_log().len(), n, "{name} {phase}: no press after it");
            assert!(
                e.travel_log().iter().all(|&(at, _)| at != t),
                "{name} {phase}: none in its tick"
            );
        }
    }
}

#[test]
fn a_release_or_a_move_does_not_stop_it() {
    for (ev, name) in [
        (InputEvent::Release(Key::Left), "a release"),
        (
            InputEvent::Pointer {
                x: 9,
                y: 9,
                buttons: 0,
            },
            "a move",
        ),
    ] {
        assert_eq!(
            walk_east(|_| {}, nothing(), &[(8, ev)]),
            (Some(End::Arrived), 5),
            "{name}"
        );
    }
}

/// Review opus NIT 10: a button held down since before the travel started (1 then 1, the
/// pointer moving) is no press; a second button going down is.
#[test]
fn a_held_button_does_not_stop_it_a_new_press_does() {
    let held = InputEvent::Pointer {
        x: 9,
        y: 9,
        buttons: 1,
    };
    for (buttons, want) in [(1u8, End::Arrived), (3, End::Input)] {
        let mut e = stand_in(room((R + 1, R + 3)), nothing());
        e.tick(&[held], 20);
        e.travel_to((R + 6, R + 3)).expect("reachable");
        run(
            &mut e,
            &[(
                8,
                InputEvent::Pointer {
                    x: 10,
                    y: 10,
                    buttons,
                },
            )],
        );
        assert_eq!(e.travel_end(), Some(want), "buttons 1, then {buttons}");
    }
}

/// Review gpt-a SHOULD 1 and gpt-d MUST 2: a key waiting in the queue that the travel did not
/// press (given before it started, the game not reading yet) ends it before its first press,
/// though no input comes with the tick.
#[test]
fn a_waiting_key_not_its_own_stops_it() {
    let mut e = stand_in_with(room((R + 1, R + 3)), nothing(), 200, nothing());
    e.tick(&[], 20);
    e.tick(&[InputEvent::Key(Key::Char('a'))], 20);
    assert_eq!(e.io().pending_events(), 1, "the key waits");
    e.travel_to((R + 6, R + 3)).expect("reachable");
    e.tick(&[], 20);
    assert_eq!(e.travel_end(), Some(End::Input), "a key it did not press");
    assert!(e.travel_log().is_empty(), "no press");
}

/// The travel's own key waiting for a late read (the game reads 60 ms after its prompt: three
/// ticks) only holds it, at each step.
#[test]
fn its_own_key_waiting_for_a_late_read_holds_it() {
    assert_eq!(
        ended(east(|_| {}, nothing(), 60, nothing()), &[]),
        (Some(End::Arrived), 5),
        "its own key, read late"
    );
}

/// Review opus N4: a new travel does not take the last travel's pressed key, still waiting for the
/// game's read, as its own: a key queued before it started stops it at once.
#[test]
fn a_stopped_travels_waiting_key_is_not_the_next_ones_own() {
    let mut e = east(|_| {}, nothing(), 60, nothing());
    for _ in 0..400 {
        if !e.travel_log().is_empty() {
            break;
        }
        e.tick(&[], 20);
    }
    assert_eq!(e.io().pending_events(), 1, "its key waits for the read");
    let press = InputEvent::Pointer {
        x: 9,
        y: 9,
        buttons: 1,
    };
    e.tick(&[press], 20);
    assert_eq!(e.travel_end(), Some(End::Input), "stopped, its key unread");
    e.travel_to((R + 6, R + 3)).expect("reachable");
    e.tick(&[], 20);
    assert_eq!(
        e.travel_end(),
        Some(End::Input),
        "the old key is not the new travel's own"
    );
    assert_eq!(e.travel_log().len(), 1, "no press of its own");
}

/// A stand-in prompt hook that runs `set` at the read after step 2 (the read up: A4-0x7a2c 1, in
/// the event wait).
fn at_read_2(set: fn(&mut State)) -> After {
    Box::new(move |s, n| {
        if n == 2 {
            set(s);
        }
    })
}

/// The walk east with `set` run at the read after step 2: how it ended, and the keys pressed.
fn east_set_at_read_2(set: fn(&mut State)) -> (Option<End>, usize) {
    ended(east(|_| {}, at_read_2(set), 0, nothing()), &[])
}

/// Spec 21 round 2 (review opus MUST-2): a more prompt (A4-0x7a42 clear with the read up) ends
/// the travel there with no further press.
#[test]
fn a_more_prompt_stops_it() {
    assert_eq!(
        east_set_at_read_2(|s| s.ui.input.accept = false),
        (Some(End::Prompt), 2)
    );
}

/// Spec 19 round 6 (the merge): the travel's own command-prompt predicate (A4-0x7a2c, A4-0x7a42,
/// no line editor A4-0x7a57, no cursor A4-0x7a60) is gone for the one in `engine`
/// ([`crate::engine::command_prompt`], without the line editor's flag); over every combination
/// of the five flags (A4-0x7a60 at 0, 1 and 2) the travel decides as before: the other-prompt
/// stop is the old one, and where it does not stop the command-prompt test is the old one too.
#[test]
fn the_one_command_prompt_predicate_decides_the_travel_as_before() {
    let old_prompt = |s: &State| {
        s.main.reading == 1
            && s.ui.input.accept
            && !s.ui.input.line_input
            && s.ui.input.targeting == 0
    };
    let old_other = |s: &State| !old_prompt(s) && (s.ui.input.in_wait || s.ui.input.line_input);
    let mut seen = 0;
    for bits in 0..16u8 {
        for targeting in 0..3u8 {
            let mut s = State::empty();
            s.main.reading = u16::from(bits & 1);
            s.ui.input.accept = bits & 2 != 0;
            s.ui.input.line_input = bits & 4 != 0;
            s.ui.input.in_wait = bits & 8 != 0;
            s.ui.input.targeting = targeting;
            assert_eq!(
                other_prompt(&s),
                old_other(&s),
                "the stop, {bits:#x} {targeting}"
            );
            if !other_prompt(&s) {
                assert_eq!(
                    command_prompt(&s),
                    old_prompt(&s),
                    "the press, {bits:#x} {targeting}"
                );
            }
            seen += 1;
        }
    }
    assert_eq!(seen, 48);
}

/// Review opus MUST-2: the line editor up (A4-0x7a57) ends it there.
#[test]
fn the_line_editor_stops_it() {
    assert_eq!(
        east_set_at_read_2(|s| s.ui.input.line_input = true),
        (Some(End::Prompt), 2)
    );
}

/// Review opus MUST-1: Tab's cross-hair (targeting 1) or the panel cursor (2) up ends it there:
/// no direction key is pressed into the cursor.
#[test]
fn a_cursor_up_stops_it() {
    assert_eq!(
        east_set_at_read_2(|s| s.ui.input.targeting = 1),
        (Some(End::Prompt), 2),
        "the cross-hair"
    );
    assert_eq!(
        east_set_at_read_2(|s| s.ui.input.targeting = 2),
        (Some(End::Prompt), 2),
        "the panel cursor"
    );
}

/// A wait off the command prompt (the read down, in the event wait) ends it.
#[test]
fn a_wait_off_the_prompt_stops_it() {
    let off: After = Box::new(|s, n| {
        if n == 2 {
            s.ui.input.in_wait = true;
            s.ui.input.accept = false;
        }
    });
    assert_eq!(walk_east(|_| {}, off, &[]), (Some(End::Prompt), 2));
}

/// Review opus MUST-2: another member made leader mid-walk ends the travel (Displaced).
#[test]
fn another_leader_ends_it() {
    let switch: After = Box::new(|s, n| {
        if n == 1 {
            s.actors.party.leader_index = 1;
        }
    });
    assert_eq!(
        walk_east(with_member, switch, &[]),
        (Some(End::Displaced), 1),
        "another leader"
    );
}

/// Review opus MUST-2: no leader (the index past the roster's slots and slot 16) ends it (Gone).
#[test]
fn no_leader_ends_it() {
    let none: After = Box::new(|s, n| {
        if n == 1 {
            s.actors.party.leader_index = crate::actors::PARTY_SLOTS as u8 + 1;
        }
    });
    assert_eq!(
        walk_east(|_| {}, none, &[]),
        (Some(End::Gone), 1),
        "no leader"
    );
}

/// Review gpt-d MUST 2: a Restore during the walk that leaves the leader on the cell the last
/// press found it on (as a save made at the start would) ends the travel; it does not read as a
/// blocked step, and nothing presses after it.
#[test]
fn a_restore_during_the_walk_ends_it_at_the_same_cell() {
    let load: After = Box::new(|s, n| {
        if n == 1 {
            seams::move_object(s, crate::objects::ObjectId(1), R + 1, R + 3, 0);
            s.saveload.restores = s.saveload.restores.wrapping_add(1);
        }
    });
    let mut e = east(|_| {}, nothing(), 0, load);
    run(&mut e, &[]);
    assert_eq!(
        (e.travel_end(), e.travel_log().len()),
        (Some(End::Displaced), 1)
    );
    assert_eq!(leader_at(&e.state()), (R + 1, R + 3), "back at the start");
    for _ in 0..30 {
        e.tick(&[], 20);
    }
    assert!(
        !e.travel_active() && e.travel_log().len() == 1,
        "and it stays over"
    );
}

/// Member 2 following (worktype 1) one cell behind the leader.
fn with_member(s: &mut State) {
    s.actors.party.members[1] = 2;
    s.actors.party.size = 2;
    s.objects.type_frame[2] = 0x17a;
    s.objects.location[2] = Position {
        x: R,
        y: R + 3,
        z: 0,
    };
    s.actors.attrs.status[2] = 0x80;
    s.actors.worktype[2] = 1;
    crate::actors::walk::tests::link_objects(s);
}

/// After step 2 and on, member 2 stands `cells` west of the leader.
fn member_west(cells: u16) -> After {
    Box::new(move |s, n| {
        if n >= 2 {
            let p = s.objects.location[1];
            s.objects.location[2] = Position {
                x: p.x - cells,
                ..p
            };
        }
    })
}

#[test]
fn combat_a_foe_or_a_member_left_behind_stops_it() {
    let combat: After = Box::new(|s, n| {
        if n == 2 {
            s.actors.set_tail(tail::COMBAT_MODE, 1);
        }
    });
    assert_eq!(walk_east(|_| {}, combat, &[]), (Some(End::Combat), 2));
    let foe: After = Box::new(|s, n| {
        if n == 2 {
            in_view_foe(s, 0x60);
        }
    });
    assert_eq!(walk_east(|_| {}, foe, &[]), (Some(End::Foe), 2));
    assert_eq!(
        walk_east(with_member, member_west(LEASH + 1), &[]),
        (Some(End::Party), 2)
    );
    assert_eq!(
        walk_east(with_member, member_west(LEASH), &[]),
        (Some(End::Arrived), 5),
        "at the leash"
    );
    assert_eq!(
        walk_east(with_member, nothing(), &[]),
        (Some(End::Arrived), 5),
        "a member that follows"
    );
}

#[test]
fn a_blocked_step_plans_once_then_stops() {
    // After step 1 an NPC stands on the next cell east: one new plan goes round it.
    let ahead = |x: u16| -> After {
        Box::new(move |s, n| {
            if n == 1 {
                npc(s, 9, x, R + 3);
            }
        })
    };
    let s = room((R + 1, R + 3));
    let mut e = stand_in(s.clone(), ahead(R + 3));
    e.tick(&[], 20);
    e.travel_to((R + 6, R + 3)).expect("reachable");
    let most = run_plans(&mut e);
    assert_eq!(e.travel_end(), Some(End::Arrived), "round it");
    assert_eq!(
        most, 2,
        "the first plan and the one round it, while it walked"
    );
    let keys: Vec<Key> = e.travel_log().iter().map(|&(_, k)| k).collect();
    assert_eq!(keys[0], Key::Right, "the first step east");
    assert_ne!(
        keys[1],
        Key::Right,
        "then round the NPC, not into it: {keys:?}"
    );
    // A corridor: the NPC in it leaves no way, so the one plan finds none.
    let mut s = s;
    for x in R..R + 8 {
        tile(&mut s, x, R + 2, WALL);
        tile(&mut s, x, R + 4, WALL);
    }
    let mut e = stand_in(s, ahead(R + 4));
    e.tick(&[], 20);
    e.travel_to((R + 6, R + 3)).expect("reachable");
    run(&mut e, &[]);
    assert_eq!(e.travel_end(), Some(End::Blocked));
    assert_eq!(
        leader_at(&e.state()),
        (R + 3, R + 3),
        "stopped before the NPC"
    );
    assert_eq!(e.travel_log().len(), 2, "no press into it");
}

#[test]
fn a_step_that_went_elsewhere_plans_again_a_jump_ends_it() {
    let moved = |to: (u16, u16), z: u8| -> After {
        Box::new(move |s, n| {
            if n == 1 {
                seams::move_object(s, crate::objects::ObjectId(1), to.0, to.1, u16::from(z));
                s.objects.location[1].z = z;
            }
        })
    };
    assert_eq!(
        walk_east(|_| {}, moved((R + 2, R + 4), 0), &[]).0,
        Some(End::Arrived),
        "a stagger one cell off: planned again"
    );
    let back: After = Box::new(|s, _| {
        seams::move_object(s, crate::objects::ObjectId(1), R + 1, R + 3, 0);
    });
    assert_eq!(
        walk_east(|_| {}, back, &[]),
        (Some(End::Blocked), 2),
        "a step that leaves the leader where it was, twice: the one plan, then the stop"
    );
    assert_eq!(
        walk_east(|_| {}, moved((R + 5, R + 6), 0), &[]),
        (Some(End::Displaced), 1),
        "a jump"
    );
    assert_eq!(
        walk_east(|_| {}, moved((R + 2, R + 3), 1), &[]),
        (Some(End::Displaced), 1),
        "another level"
    );
}

/// Review gpt-d SHOULD 1: each blockage has its own new plan. A step that leaves the leader where
/// it was, after step 1 and again after step 3 with a good step between, plans again each time
/// and the travel arrives.
#[test]
fn each_blockage_has_its_own_new_plan() {
    let twice: After = Box::new(|s, n| {
        if n == 1 || n == 3 {
            let p = s.objects.location[1];
            seams::move_object(s, crate::objects::ObjectId(1), p.x - 1, p.y, 0);
        }
    });
    let mut e = east(|_| {}, nothing(), 0, twice);
    let most = run_plans(&mut e);
    assert_eq!(e.travel_end(), Some(End::Arrived));
    assert_eq!(most, 3, "the first plan and one a blockage");
    assert_eq!(
        e.travel_log().len(),
        7,
        "five steps and the two that came back"
    );
}

/// What a quiet tick polls: no input, no button, nothing queued.
const QUIET: Polled<'static> = Polled {
    input: &[],
    buttons: 0,
    queued: false,
    foreign: false,
    menu: false,
    due: true,
};

/// Review opus SHOULD-2: the first press checks the leader against the plan's start. One cell
/// off (it moved between the start and the first prompt) plans again from where it stands;
/// farther ends the travel.
#[test]
fn the_first_press_checks_the_plans_start() {
    let s = room((R + 1, R + 3));
    let to = (R + 6, R + 3);
    let moved = |cell: (u16, u16)| {
        let mut c = s.clone();
        seams::move_object(&mut c, crate::objects::ObjectId(1), cell.0, cell.1, 0);
        c
    };
    let mut t = Travel::start(&s, to).expect("reachable");
    assert_eq!(
        t.next(&s, &QUIET),
        Next::Press(Key::Right),
        "from the start"
    );
    assert_eq!(t.plans, 1);
    let off = moved((R + 1, R + 4));
    let first = path(&off, to).expect("a way")[0];
    let mut t = Travel::start(&s, to).expect("reachable");
    assert_eq!(
        t.next(&off, &QUIET),
        Next::Press(KEYPAD[usize::from(first)].0),
        "one cell off: the new plan's first step"
    );
    assert_eq!(t.plans, 2, "planned again");
    let mut t = Travel::start(&s, to).expect("reachable");
    assert_eq!(
        t.next(&moved((R + 3, R + 6)), &QUIET),
        Next::Stop(End::Displaced),
        "farther off"
    );
}

/// The level's coordinate mask: the surface's at z 0, a dungeon's else.
fn mask_of(z: u8) -> u16 {
    if z == 0 {
        crate::map::SURFACE_MASK
    } else {
        crate::map::DUNGEON_MASK
    }
}

/// [`open_map`] on level `z`, its window straddling the level's seam on `axis` (0 x, 1 y), the
/// leader on `at` on that axis (window cell C on both axes).
fn seam(z: u8, axis: usize, at: u16) -> State {
    let mut s = open_map();
    let mask = mask_of(z);
    s.map.world_mask = mask;
    let mut origin = [0u16; 2];
    origin[axis] = at.wrapping_sub(C) & mask;
    s.map.window_origin = origin;
    let mut p = [C, C];
    p[axis] = at;
    s.map.centre = Position {
        x: p[0],
        y: p[1],
        z,
    };
    s.objects.location[1] = s.map.centre;
    crate::actors::walk::tests::link_objects(&mut s);
    s
}

/// The cell `along` cells from `at` on `axis` (masked) and `across` from C on the other axis.
fn seam_cell(z: u8, axis: usize, at: u16, along: i16, across: i16) -> (u16, u16) {
    let mut c = [C, C];
    c[axis] = at.wrapping_add_signed(along) & mask_of(z);
    c[1 - axis] = C.wrapping_add_signed(across);
    (c[0], c[1])
}

/// Review gpt-a MUST 1 (opus NIT 9): the distance on a level wraps at its mask, on both axes and
/// both ways: the cells either side of the seam are one apart.
#[test]
fn the_seam_is_one_cell_on_every_level() {
    for z in [0u8, 1] {
        let mask = mask_of(z);
        for (a, b) in [((0, 5), (mask, 5)), ((5, 0), (5, mask))] {
            assert_eq!(chebyshev(a, b, mask), 1, "{mask:#x}: {a:?} to {b:?}");
            assert_eq!(chebyshev(b, a, mask), 1, "{mask:#x}: {b:?} to {a:?}");
        }
        assert_eq!(
            chebyshev((1, 0), (mask - 1, mask), mask),
            3,
            "x 1, 0, mask, mask - 1"
        );
        assert_eq!(
            chebyshev((0, 0), (mask / 2 + 1, 0), mask),
            mask / 2 + 1,
            "half way round"
        );
        assert_eq!(chebyshev((3, 4), (3, 4), mask), 0);
    }
}

/// Review gpt-a MUST 1: on both levels, both axes and both ways, a travel across the seam presses
/// the step across it; a step that lands one cell off across the seam is a stagger (planned
/// again), three cells on is a jump (Displaced); a member beside the leader across the seam is
/// not left behind, one past the leash is.
#[test]
fn a_stagger_across_the_seam_plans_again_a_jump_ends_it() {
    for z in [0u8, 1] {
        let mask = mask_of(z);
        for axis in 0..2 {
            for way in [1i16, -1] {
                let at = if way == 1 { mask } else { 0 };
                let s = seam(z, axis, at);
                let name = format!("z {z}, axis {axis}, way {way}");
                let across = KEYPAD
                    .iter()
                    .find(|&&(_, dx, dy)| (dx, dy) == if axis == 0 { (way, 0) } else { (0, way) })
                    .expect("a key")
                    .0;
                let landed = |cell: (u16, u16)| {
                    let to = seam_cell(z, axis, at, 2 * way, 0);
                    let mut t = Travel::start(&s, to).unwrap_or_else(|r| panic!("{name}: {r:?}"));
                    assert_eq!(t.next(&s, &QUIET), Next::Press(across), "{name}");
                    let mut c = s.clone();
                    seams::move_object(
                        &mut c,
                        crate::objects::ObjectId(1),
                        cell.0,
                        cell.1,
                        u16::from(z),
                    );
                    t.next(&c, &QUIET)
                };
                assert_eq!(
                    landed(seam_cell(z, axis, at, way, 0)),
                    Next::Press(across),
                    "{name}: the step across"
                );
                assert!(
                    matches!(landed(seam_cell(z, axis, at, way, 1)), Next::Press(_)),
                    "{name}: one cell off across the seam, planned again"
                );
                assert_eq!(
                    landed(seam_cell(z, axis, at, 3 * way, 0)),
                    Next::Stop(End::Displaced),
                    "{name}: three cells on"
                );
                for (cells, behind) in [(1i16, false), (LEASH as i16 + 1, true)] {
                    let mut m = s.clone();
                    with_member(&mut m);
                    let (x, y) = seam_cell(z, axis, at, cells * way, 0);
                    m.objects.location[2] = Position { x, y, z };
                    crate::actors::walk::tests::link_objects(&mut m);
                    assert_eq!(party::following(&m), vec![2], "{name}: member 2 follows");
                    assert_eq!(
                        left_behind(&m, 1),
                        behind,
                        "{name}: a member {cells} across the seam"
                    );
                }
            }
        }
    }
}

#[test]
fn the_turn_holds_it_and_a_finished_game_or_another_program_ends_it() {
    let prompt = room((R + 1, R + 3));
    let mut busy = prompt.clone();
    busy.main.reading = 0;
    busy.ui.input.in_wait = false;
    let mut t = Travel::start(&prompt, (R + 4, R + 3)).expect("reachable");
    let quiet = QUIET;
    assert_eq!(t.next(&busy, &quiet), Next::Hold, "the turn runs");
    let queued = Polled {
        queued: true,
        ..quiet
    };
    assert_eq!(t.next(&prompt, &queued), Next::Hold, "its own key waits");
    assert_eq!(t.next(&prompt, &quiet), Next::Press(Key::Right));
    let foreign = Polled {
        queued: true,
        foreign: true,
        ..quiet
    };
    let mut f = t.clone();
    assert_eq!(
        f.next(&prompt, &foreign),
        Next::Stop(End::Input),
        "a key not its own waits"
    );
    let mut u = prompt.clone();
    u.main.program = Program::U;
    assert_eq!(t.next(&u, &quiet), Next::Stop(End::Gone));
    let mut e = Engine::with_game(prompt, Files::default(), |_, _| async {});
    e.tick(&[], 20);
    assert!(e.finished());
    e.travel_to((R + 4, R + 3)).expect("reachable");
    e.tick(&[], 20);
    assert_eq!(e.travel_end(), Some(End::Gone));
}

/// Spec 21 S3: a step not due on the platform's clock (`Polled.due` false) holds the travel at
/// the prompt with nothing read back (the same step pressed once it is due), and the stops stay
/// at once: a key, or new input the context menu took, ends it while no step is due. So it holds
/// with a press outstanding too, the leader on the cell it aimed at: due, that step lands.
#[test]
fn a_step_not_due_holds_it_and_the_stops_stay_at_once() {
    let prompt = room((R + 1, R + 3));
    let mut t = Travel::start(&prompt, (R + 4, R + 3)).expect("reachable");
    let not_due = Polled {
        due: false,
        ..QUIET
    };
    let before = t.clone();
    assert_eq!(t.next(&prompt, &not_due), Next::Hold, "not due: holds");
    assert_eq!(t, before, "nothing read back");
    assert_eq!(
        t.next(&prompt, &QUIET),
        Next::Press(Key::Right),
        "due: the step"
    );
    let key = [InputEvent::Key(Key::Char('a'))];
    let keyed = Polled {
        input: &key,
        ..not_due
    };
    assert_eq!(
        t.clone().next(&prompt, &keyed),
        Next::Stop(End::Input),
        "a key"
    );
    let menu = Polled {
        menu: true,
        ..not_due
    };
    assert_eq!(
        t.clone().next(&prompt, &menu),
        Next::Stop(End::Input),
        "the menu's new input"
    );
    // Round 5 (opus SHOULD 2, gpt c SHOULD 1): with the first press outstanding and the leader on
    // the cell it aimed at, a step not due reads nothing back either (no `pressed` taken, no path
    // popped), however often it is asked; due, the step lands and the next is pressed.
    let mut stepped = prompt.clone();
    seams::move_object(&mut stepped, crate::objects::ObjectId(1), R + 2, R + 3, 0);
    let before = t.clone();
    assert!(
        before.pressed.is_some(),
        "the fixture: the first press outstanding"
    );
    let polls: Vec<(Next, bool)> = (0..5)
        .map(|_| (t.next(&stepped, &not_due), t == before))
        .collect();
    let due = t.next(&stepped, &QUIET);
    assert_eq!(
        (polls, due, t.steps),
        (vec![(Next::Hold, true); 5], Next::Press(Key::Right), 1),
        "not due: held, nothing read back (the press still outstanding), each time; due: the step \
         landed and the next pressed"
    );
}

/// Spec 21's merge contract with unit M3 (the wiring round): the stand-in at its prompt with the
/// context menu on and a menu open over the room (the press at frame pixel (40, 40), a map menu
/// whose one entry is Go here to (R + 7, R + 3), seven steps east of the leader at (R, R + 3)),
/// and the frame pixel of its Go here row. The menu's press, not a query's: the room has no map
/// the menu could read.
fn menu_over_the_room() -> (Engine, (i16, i16)) {
    let s = room((R, R + 3));
    let open = go_here_east(&s);
    let (x0, y0, ..) = open.text_rect();
    let mut e = stand_in(s, nothing());
    e.set_context_menu(true);
    e.tick(&[], 20);
    e.popup_mut().show(open);
    (e, ((x0 + 12) as i16, (y0 + 4) as i16))
}

/// The map menu [`menu_over_the_room`] shows on `s`, laid out for a press at frame pixel (40, 40):
/// its one entry Go here to (R + 7, R + 3).
fn go_here_east(s: &State) -> crate::ui::popup::Open {
    use crate::ui::menu::{Entry, GO_HERE, Kind, Menu};
    let menu = Menu {
        kind: Kind::Map,
        cell: Some(((R + 7) as i16, (R + 3) as i16)),
        underlined: None,
        sections: Vec::new(),
        go_here: Some(Entry {
            code: GO_HERE,
            reason: None,
            theft: false,
        }),
    };
    crate::ui::popup::Open::new(&menu, (40, 40), s, &[])
}

fn ptr((x, y): (i16, i16), buttons: u8) -> InputEvent {
    InputEvent::Pointer { x, y, buttons }
}

/// What [`after_the_tail`] saw.
#[derive(Debug)]
struct Tail {
    /// The leader's cell after each tick of the tail.
    cells: Vec<(u16, u16)>,
    /// The keys the travel pressed during the tail, each with its tick.
    log: Vec<(u64, Key)>,
    /// It still walked after the tail.
    walking: bool,
    /// The presses and the key events the menu let through to the game during the tail
    /// ([`Engine::game_presses`], [`Engine::game_keys`]).
    game: (u64, u64),
    /// The cells Go here asked travel to, after the tail.
    asked: usize,
    /// How it ended at a later left press on the map (or at its end).
    end: Option<End>,
}

/// [`menu_over_the_room`] with Go here chosen, then `tail` given a tick each (an empty list: a
/// tick with none) with the travel walking, then a later left press on the map.
fn after_the_tail(choose: &[InputEvent], tail: &[&[InputEvent]]) -> Tail {
    let (mut e, _) = menu_over_the_room();
    e.tick(choose, 20);
    assert!(e.context_menu().is_none(), "chosen: closed");
    assert!(e.travel_active(), "Go here started the travel");
    assert!(
        e.travel_log().is_empty(),
        "its first step is the next tick's"
    );
    let game = (e.game_presses(), e.game_keys());
    let mut cells = Vec::new();
    for events in tail {
        e.tick(events, 20);
        cells.push(leader_at(&e.state()));
    }
    let log = e.travel_log().to_vec();
    let walking = e.travel_active();
    let game = (e.game_presses() - game.0, e.game_keys() - game.1);
    let asked = e.go_here_asked().asked.len();
    run(&mut e, &[(3, ptr((9, 9), POINTER_LEFT))]);
    Tail {
        cells,
        log,
        walking,
        game,
        asked,
        end: e.travel_end(),
    }
}

/// Round 5 (gpt c SHOULD 2): `tail` (`what`) after the choice `choose` against its control, the
/// same choice and as many ticks with no input: the tail drives nothing, the leader on the same
/// cells and the travel's keys at the same ticks; Go here was chosen once; none of the tail
/// reached the game; still walking, and a later press ends it. The keys pressed during the tail.
fn the_tail_is_inert(choose: &[InputEvent], tail: &[&[InputEvent]], what: &str) -> usize {
    let quiet: Vec<&[InputEvent]> = vec![&[]; tail.len()];
    let (got, control) = (after_the_tail(choose, tail), after_the_tail(choose, &quiet));
    assert!(control.walking, "the fixture: the control walks");
    assert_eq!(
        (&got.cells, &got.log, got.walking),
        (&control.cells, &control.log, true),
        "{what}: the cells walked, the keys pressed and when, still walking"
    );
    assert_eq!(got.asked, 1, "{what}: Go here chosen once");
    assert_eq!(got.game, (0, 0), "{what}: no press or key of it the game's");
    assert_eq!(got.end, Some(End::Input), "{what}: a later press ends it");
    got.log.len()
}

/// Spec 21's merge contract, the wiring round's first test: the choosing press's held moves and
/// its release, which the menu swallows (its handler buttons read 0 after the choice, so a held
/// move fed raw reads as a new press), are no stop: the travel the press started walks on through
/// them, as with no input, and a later press on the map ends it.
#[test]
fn a_travel_the_menu_started_walks_through_the_choosing_presss_held_moves_and_release() {
    let (_, (gx, gy)) = menu_over_the_room();
    let held: Vec<[InputEvent; 1]> = (1..=6)
        .map(|k| [ptr((gx + k, gy), POINTER_LEFT)])
        .chain(std::iter::once([ptr((gx + 6, gy), 0)]))
        .collect();
    let tail: Vec<&[InputEvent]> = held.iter().map(|e| e.as_slice()).collect();
    let during = the_tail_is_inert(
        &[ptr((gx, gy), POINTER_LEFT)],
        &tail,
        "the held moves and the release",
    );
    assert!(
        during >= 1,
        "walked through the held moves and the release: {during} steps"
    );
}

/// The second: a double click on the entry (its second press within the window at the first's
/// pixel, and its release: the menu's, the choice's tail) is no stop.
#[test]
fn a_travel_the_menu_started_walks_through_a_double_click_on_the_entry() {
    let (_, at) = menu_over_the_room();
    let (up, down) = ([ptr(at, 0)], [ptr(at, POINTER_LEFT)]);
    let tail: [&[InputEvent]; 4] = [&up, &[], &down, &up];
    let during = the_tail_is_inert(&[ptr(at, POINTER_LEFT)], &tail, "the double click");
    assert!(
        during >= 1,
        "walked through the double click: {during} steps"
    );
}

/// The third: Enter chose Go here and is held past its repeat delay (500 ms, 25 ticks, then a
/// repeat every other tick and the release): the repeats and the release are the menu's (it
/// took the key's press), no stop.
#[test]
fn a_travel_the_menu_started_walks_through_enter_held_past_its_repeat_delay() {
    let (rep, release) = (
        [InputEvent::Repeat(Key::Enter)],
        [InputEvent::Release(Key::Enter)],
    );
    let mut tail: Vec<&[InputEvent]> = vec![&[]; 24];
    for _ in 0..3 {
        tail.push(&rep);
        tail.push(&[]);
    }
    tail.push(&release);
    let during = the_tail_is_inert(
        &[InputEvent::Key(Key::Enter)],
        &tail,
        "Enter's repeats and release",
    );
    assert!(
        during >= 3,
        "walked through Enter's repeats and release: {during} steps"
    );
}

/// Round 5 (opus NIT): the stop flag reaches the travel without the user's zip. A travel under way
/// and then a menu shown over the room (as no input of the player can, since a menu opens only by
/// new input): a key the menu takes is in none of the events it passes on, so only its flag
/// ([`crate::ui::popup::Popup::took_new_input`] into `Polled.menu`) ends the travel, `Input`,
/// before its first step.
#[test]
fn a_key_the_menu_takes_ends_a_travel_under_way_by_its_flag() {
    let s = room((R, R + 3));
    let open = go_here_east(&s);
    let mut e = stand_in(s, nothing());
    e.set_context_menu(true);
    e.tick(&[], 20);
    e.travel_to((R + 7, R + 3)).expect("reachable");
    e.popup_mut().show(open);
    let keys = e.game_keys();
    e.tick(&[InputEvent::Key(Key::Down)], 20);
    assert_eq!(
        (e.travel_end(), e.travel_log().len()),
        (Some(End::Input), 0),
        "the flag ended it, before its first step"
    );
    assert_eq!(e.game_keys(), keys, "the fixture: the key the menu's");
}

/// Real data: the user's zip, read only (counts printed only). Skips without it.
#[allow(clippy::disallowed_methods)] // tests may read files
mod real {
    use super::*;
    use crate::engine::InputEvent;
    use crate::pacer::STEP_MS;
    use u6_testkit::Quiet;

    const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

    fn files() -> Option<Files> {
        if !std::path::Path::new(ZIP).exists() {
            println!("real receipt: skipped ({ZIP} not present)");
            return None;
        }
        let data = std::fs::read(ZIP).quiet();
        let r = u6_intake::intake(vec![u6_intake::InputFile {
            name: "ultima-vi.zip".into(),
            data,
        }]);
        Some(Files::new(r.files))
    }

    fn at_prompt(e: &Engine) -> bool {
        e.io().pending_events() == 0 && command_prompt(&e.state())
    }

    fn settle(e: &mut Engine) {
        for _ in 0..20_000 {
            if at_prompt(e) && e.ticks() > 10 {
                return;
            }
            e.tick(&[], STEP_MS);
        }
        panic!("the game reaches the prompt");
    }

    /// The castle start state of spec 19's held-key tests (`ui/tests/held_key.rs` `castle`):
    /// `game` at once with seed 0, the actors in the assault worktype 8 passed over (status bit
    /// 0x10), the fight broken off with `b`, at the prompt.
    fn castle(files: &Files) -> Engine {
        castle_with(files).0
    }

    /// [`castle`], with the game's state for the test to change between ticks.
    fn castle_with(files: &Files) -> (Engine, Rc<RefCell<State>>) {
        let mut state = State::new_game(files).quiet();
        state.rng.seed(0);
        let keep = Rc::new(RefCell::new(None));
        let k = Rc::clone(&keep);
        let mut e = Engine::with_game(state, files.clone(), move |st, io| {
            *k.borrow_mut() = Some(Rc::clone(&st));
            crate::main_loop::game_first(st, io)
        });
        settle(&mut e);
        let st = keep.borrow_mut().take().quiet();
        {
            let mut s = st.borrow_mut();
            for a in 0..0x100 {
                if s.actors.worktype[a] == 8 {
                    s.actors.attrs.status[a] |= 0x10;
                }
            }
        }
        e.tick(&[InputEvent::Key(Key::Char('b'))], STEP_MS);
        for _ in 0..480 / STEP_MS {
            e.tick(&[], STEP_MS);
        }
        settle(&mut e);
        (e, st)
    }

    /// A cell two or one steps from the leader (east, west, south, north first) that a travel
    /// may start to.
    fn reachable_near(s: &State) -> Option<(u16, u16)> {
        let p = s.leader_position().quiet();
        [
            (2i16, 0i16),
            (-2, 0),
            (0, 2),
            (0, -2),
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
        ]
        .into_iter()
        .map(|(dx, dy)| (p.x.wrapping_add_signed(dx), p.y.wrapping_add_signed(dy)))
        .find(|&c| reachable(s, c).is_none())
    }

    /// The shown frame's pixels, hashed.
    fn frame_hash(e: &Engine) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        e.frame().pixels().hash(&mut h);
        h.finish()
    }

    /// One tick of `e` with `input`, its frame's hash kept in `frames`.
    fn tick_kept(e: &mut Engine, input: &[InputEvent], frames: &mut Vec<u64>) {
        e.tick(input, STEP_MS);
        frames.push(frame_hash(e));
    }

    /// A door the receipt walks through: its slot, the cell before it (orthogonally beside it)
    /// that the leader travels to across the room, and the cell beyond it on the same axis that
    /// the second travel reaches through it once open.
    struct Door {
        slot: usize,
        cell: (u16, u16),
        before: (u16, u16),
        beyond: (u16, u16),
    }

    /// A closed, unlocked door (types 0x129..=0x12c, frame & 0xc = 4: frames 4..7 closed,
    /// 8.. locked) on the party's level inside the window, nearest the leader first, with a cell
    /// before it at least three steps away by travel and a cell one or two beyond it that a plan
    /// from the cell before reaches through the door once it is open (frame ^ 4, as Use leaves
    /// it): (closed doors in the window, the first such door).
    fn closed_door(s: &State) -> (usize, Option<Door>) {
        let l = s.actors.party.leader().quiet().0;
        let lp = s.objects.location[usize::from(l)];
        let w = Window::of(s);
        let mut doors: Vec<usize> = (0x100..s.objects.type_frame.len())
            .filter(|&i| {
                let t = s.objects.type_frame[i];
                let p = s.objects.location[i];
                (0x129..=0x12c).contains(&(t & 0x3ff))
                    && t >> 10 & 0xc == 4
                    && p.z == lp.z
                    && w.cell((p.x, p.y)).is_some()
                    && s.objects
                        .location(crate::objects::ObjectId(i as u16))
                        .is_some()
            })
            .collect();
        doors.sort_by_key(|&i| {
            let p = s.objects.location[i];
            (chebyshev((lp.x, lp.y), (p.x, p.y), w.mask), i)
        });
        let found = doors.iter().find_map(|&i| {
            let p = s.objects.location[i];
            let mut open = s.clone();
            open.objects.type_frame[i] ^= 4 << 10;
            [(1i16, 0i16), (-1, 0), (0, 1), (0, -1)]
                .into_iter()
                .find_map(|(ax, ay)| {
                    let before = (p.x.wrapping_add_signed(-ax), p.y.wrapping_add_signed(-ay));
                    let far = plan(s, l, (lp.x, lp.y), before).path?;
                    if reachable(s, before).is_some() || far.len() < 3 {
                        return None;
                    }
                    (1..=2).rev().find_map(|k| {
                        let beyond = (
                            p.x.wrapping_add_signed(ax * k),
                            p.y.wrapping_add_signed(ay * k),
                        );
                        let through = plan(&open, l, before, beyond).path?;
                        cells(before, &through)
                            .contains(&(p.x, p.y))
                            .then_some(Door {
                                slot: i,
                                cell: (p.x, p.y),
                                before,
                                beyond,
                            })
                    })
                })
        });
        (doors.len(), found)
    }

    /// The frame pixel at the middle of the view cell holding map cell (x, y) (as
    /// `objects/commands/real_data.rs` `cell_pixel`: the map drawn from the map's pixel origin, 16
    /// pixels a cell).
    fn cell_pixel(s: &State, (x, y): (u16, u16)) -> (i16, i16) {
        let c = s.map.centre;
        let col = x as i16 - c.x as i16 + geom::HALF_W as i16;
        let row = y as i16 - c.y as i16 + geom::HALF_H as i16;
        (
            geom::MAP_X as i16 + col * 16 + 8,
            geom::MAP_Y as i16 + row * 16 + 8,
        )
    }

    /// What one leg of the receipt counted: leader moves and command prompts reached (turns).
    #[derive(Default)]
    struct Counts {
        moves: usize,
        turns: usize,
    }

    /// Ticks `e` until it is back at the command prompt with no travel under way (at most 20000
    /// ticks), `given` (tick, input) fed at the tick it names, counting into `n` and keeping each
    /// tick's frame hash in `frames`.
    fn until_prompt(
        e: &mut Engine,
        given: &[(u64, InputEvent)],
        n: &mut Counts,
        frames: &mut Vec<u64>,
    ) {
        let mut at = e.state().leader_position().quiet();
        let mut prompt = at_prompt(e);
        for _ in 0..20_000 {
            let tick = e.ticks() + 1;
            let input: Vec<InputEvent> = given
                .iter()
                .filter(|&&(t, _)| t == tick)
                .map(|&(_, ev)| ev)
                .collect();
            tick_kept(e, &input, frames);
            let p = e.state().leader_position().quiet();
            if p != at {
                n.moves += 1;
                at = p;
            }
            let now = at_prompt(e);
            if now && !prompt {
                n.turns += 1;
            }
            prompt = now;
            if now && !e.travel_active() && given.iter().all(|&(t, _)| t <= e.ticks()) {
                return;
            }
        }
        panic!("the prompt did not come back");
    }

    fn minutes(s: &State) -> u32 {
        let c = s.actors.calendar;
        u32::from(c.hour) * 60 + u32::from(c.minute)
    }

    /// Spec 21 acceptance 5: in the castle the party travels across a room to a closed door,
    /// opens it with `u` and a press on it, and travels through it to a cell beyond; each leg
    /// arrives with one key a planned step and one move a key, diagonal keys among them. The same
    /// inputs given by hand at the same ticks from the same start show the same frame at every
    /// tick and leave an equal state (no shortcut).
    #[test]
    fn the_party_travels_through_a_door_and_the_keys_walk_the_same() {
        let Some(files) = files() else { return };
        let mut e = castle(&files);
        let (doors, found) = closed_door(&e.state());
        let Some(door) = found else {
            panic!("no closed door with a cell before and beyond it ({doors} closed doors)");
        };
        let (t0, m0, ms0) = (e.ticks(), minutes(&e.state()), e.io().now_ms());
        let mut frames: Vec<u64> = Vec::new();
        let mut given: Vec<(u64, InputEvent)> = Vec::new();
        let mut legs = Vec::new();
        for leg in 0..2 {
            let to = if leg == 0 { door.before } else { door.beyond };
            let planned = {
                let s = e.state();
                let l = s.actors.party.leader().quiet().0;
                let p = s.leader_position().quiet();
                plan(&s, l, (p.x, p.y), to).path.quiet()
            };
            let pressed = e.travel_log().len();
            e.travel_to(to).expect("reachable");
            let mut n = Counts::default();
            until_prompt(&mut e, &[], &mut n, &mut frames);
            assert_eq!(e.travel_end(), Some(End::Arrived), "leg {leg} arrives");
            let p = e.state().leader_position().quiet();
            assert_eq!((p.x, p.y), to, "leg {leg}: on the target");
            let keys = e.travel_log().len() - pressed;
            assert_eq!(keys, planned.len(), "leg {leg}: one key a planned step");
            assert_eq!(n.moves, planned.len(), "leg {leg}: one move a key");
            if leg == 1 {
                assert!(
                    cells(door.before, &planned).contains(&door.cell),
                    "through the door"
                );
            }
            legs.push((planned.len(), n));
            if leg == 0 {
                let closed = e.state().objects.type_frame[door.slot] >> 10;
                let u = InputEvent::Key(Key::Char('u'));
                given.push((e.ticks() + 1, u));
                tick_kept(&mut e, &[u], &mut frames);
                for _ in 0..400 {
                    let waits = {
                        let s = e.state();
                        s.main.reading == 0 && s.ui.input.target_pending == 1
                    };
                    if waits {
                        break;
                    }
                    tick_kept(&mut e, &[], &mut frames);
                }
                let mut n = Counts::default();
                let at = cell_pixel(&e.state(), door.cell);
                let tick = e.ticks() + 1;
                for ev in crate::ui::menu::left_press(at.0, at.1) {
                    given.push((tick, ev));
                }
                until_prompt(&mut e, &given, &mut n, &mut frames);
                let open = e.state().objects.type_frame[door.slot] >> 10;
                assert_eq!(open, closed ^ 4, "Use opened the door");
                legs.push((0, n));
            }
        }
        let log = e.travel_log().to_vec();
        let diagonals = log
            .iter()
            .filter(|&&(_, k)| crate::ui::input::keypad_code(k).is_some())
            .count();
        let total = |f: fn(&Counts) -> usize| legs.iter().map(|(_, n)| f(n)).sum::<usize>();
        println!(
            "real receipt: ran; travel: {doors} closed doors in the window; legs planned {} and {} \
             steps; pressed {} keys ({diagonals} diagonal) and {} by hand; the leader moved {} \
             times; {} turns; {} clock minutes; {} engine ms; {} ticks",
            legs[0].0,
            legs[2].0,
            log.len(),
            given.len(),
            total(|n| n.moves),
            total(|n| n.turns),
            minutes(&e.state()) - m0,
            e.io().now_ms() - ms0,
            e.ticks() - t0
        );
        assert!(diagonals > 0, "the walk took a diagonal key");
        assert_eq!(frames.len() as u64, e.ticks() - t0, "a frame each tick");
        // The same inputs by hand.
        let mut k = castle(&files);
        assert_eq!(k.ticks(), t0, "the same start");
        let mut all: Vec<(u64, InputEvent)> = given;
        all.extend(log.iter().map(|&(t, key)| (t, InputEvent::Key(key))));
        all.sort_by_key(|&(t, _)| t);
        let last = e.ticks();
        while k.ticks() < last {
            let tick = k.ticks() + 1;
            let input: Vec<InputEvent> = all
                .iter()
                .filter(|&&(t, _)| t == tick)
                .map(|&(_, ev)| ev)
                .collect();
            k.tick(&input, STEP_MS);
            let i = (k.ticks() - t0 - 1) as usize;
            assert_eq!(
                frame_hash(&k),
                frames[i],
                "the same frame at tick {}",
                k.ticks()
            );
        }
        assert!(!k.travel_active() && k.travel_log().is_empty(), "no travel");
        assert!(*k.state() == *e.state(), "the inputs leave the same state");
        assert!(
            k.frame().pixels() == e.frame().pixels(),
            "and the same frame"
        );
    }

    /// Review opus MUST-1: with Tab's cross-hair up (targeting 1), and the panel cursor after a
    /// second Tab (2), a travel to a reachable cell ends at its first ticks (Prompt) with no key
    /// pressed, the cursor and the leader where they were.
    #[test]
    fn a_cursor_up_ends_the_travel_and_leaves_the_cursor() {
        let Some(files) = files() else { return };
        let mut e = castle(&files);
        let to = reachable_near(&e.state()).expect("a reachable cell");
        for want in [1u8, 2] {
            e.tick(&[InputEvent::Key(Key::Char('\t'))], STEP_MS);
            for _ in 0..400 {
                let up = {
                    let s = e.state();
                    s.main.reading == 1 && s.ui.input.in_wait && s.ui.input.targeting == want
                };
                if up && e.io().pending_events() == 0 {
                    break;
                }
                e.tick(&[], STEP_MS);
            }
            let (cursor, panel, at) = {
                let s = e.state();
                assert_eq!(s.ui.input.targeting, want, "Tab: targeting {want}");
                (
                    s.ui.input.cursor,
                    s.ui.input.panel_cursor,
                    s.leader_position().quiet(),
                )
            };
            let pressed = e.travel_log().len();
            e.travel_to(to).expect("starts");
            for _ in 0..30 {
                e.tick(&[], STEP_MS);
            }
            assert_eq!(e.travel_end(), Some(End::Prompt), "targeting {want}");
            assert_eq!(e.travel_log().len(), pressed, "targeting {want}: no key");
            let s = e.state();
            assert!(
                s.ui.input.targeting == want
                    && s.ui.input.cursor == cursor
                    && s.ui.input.panel_cursor == panel,
                "targeting {want}: the cursor where it was"
            );
            assert_eq!(
                s.leader_position().quiet(),
                at,
                "targeting {want}: the leader"
            );
        }
        println!("real receipt: ran; a cursor up ends the travel at targeting 1 and 2");
    }

    /// Review gpt-d MUST 2: a Restore at the end of the travel's first turn (save mode 0x52, as
    /// the loop's end runs it, 0x104d98) puts the leader back on the cell it was saved on, the one
    /// the first press left: the travel ends (Displaced), not as a blocked step, and stays over.
    #[test]
    fn a_restore_during_the_travel_ends_it() {
        let Some(files) = files() else { return };
        let (mut e, st) = castle_with(&files);
        crate::saveload::save(&mut st.borrow_mut());
        let start = e.state().leader_position().quiet();
        let to = reachable_near(&e.state()).expect("a reachable cell");
        let restores = e.state().saveload.restores;
        st.borrow_mut().objects.save_mode = 0x52;
        e.travel_to(to).expect("starts");
        let (mut n, mut frames) = (Counts::default(), Vec::new());
        until_prompt(&mut e, &[], &mut n, &mut frames);
        assert_eq!(e.travel_end(), Some(End::Displaced));
        assert_eq!(e.travel_log().len(), 1, "one press");
        assert_eq!(e.state().saveload.restores, restores + 1, "one Restore");
        assert_eq!(
            e.state().leader_position().quiet(),
            start,
            "back on the start"
        );
        assert_eq!(n.moves, 2, "the step, and the Restore back");
        for _ in 0..50 {
            e.tick(&[], STEP_MS);
        }
        assert!(
            !e.travel_active() && e.travel_log().len() == 1,
            "and it stays over"
        );
        println!("real receipt: ran; a Restore during the travel ends it");
    }

    /// Review gpt b1-2 and b2-1: the real Restore (`saveload::restore`) of a save made on the cell
    /// the leader stands on, at the prompt after the travel's first step landed there: the leader
    /// stays put, and the travel ends (Displaced) with no key after it, by the Restore counter
    /// alone (without it the leader on the aimed cell reads as the next step).
    #[test]
    fn a_restore_on_the_leaders_own_cell_ends_the_travel() {
        let Some(files) = files() else { return };
        let (mut e, st) = castle_with(&files);
        let start = e.state().leader_position().quiet();
        let to = reachable_near(&e.state()).expect("a reachable cell");
        let steps = {
            let s = e.state();
            let l = s.actors.party.leader().quiet().0;
            plan(&s, l, (start.x, start.y), to).path.quiet().len()
        };
        assert!(steps >= 2, "a walk of {steps} steps");
        e.travel_to(to).expect("starts");
        for _ in 0..2000 {
            if e.travel_log().len() == 1 && at_prompt(&e) {
                break;
            }
            e.tick(&[], STEP_MS);
        }
        assert!(
            e.travel_active() && e.travel_log().len() == 1 && at_prompt(&e),
            "the first step taken, at the prompt"
        );
        let at = e.state().leader_position().quiet();
        assert_ne!(at, start, "the first step landed");
        let restores = e.state().saveload.restores;
        {
            let mut s = st.borrow_mut();
            crate::saveload::save(&mut s);
            crate::saveload::restore(&mut s, e.io()).quiet();
        }
        assert_eq!(
            e.state().leader_position().quiet(),
            at,
            "the leader stays put"
        );
        e.tick(&[], STEP_MS);
        assert_eq!(e.travel_end(), Some(End::Displaced), "the Restore ends it");
        assert_eq!(
            e.state().saveload.restores,
            restores + 1,
            "one Restore counted"
        );
        for _ in 0..50 {
            e.tick(&[], STEP_MS);
        }
        assert_eq!(e.travel_log().len(), 1, "no key after it");
        assert_eq!(e.state().leader_position().quiet(), at, "nor a step");
        println!("real receipt: ran; a Restore on the leader's own cell ends the travel");
    }

    /// Review opus SHOULD-4 (spec 21 Research): the Amiga raw-key table gives none of the four
    /// diagonal codes in its plain, shift or control column (two of them stand in its Alt
    /// column), so a key of the platform's reaches them only through the travel (counts only).
    #[test]
    fn no_raw_key_gives_a_diagonal_code_plain_shifted_or_with_control() {
        let Some(files) = files() else { return };
        let s = State::new_game(&files).quiet();
        let codes: Vec<u16> = [Key::UpRight, Key::DownRight, Key::DownLeft, Key::UpLeft]
            .into_iter()
            .map(|k| crate::ui::input::keypad_code(k).quiet())
            .collect();
        let map = &s.ui.tables.keymap;
        let columns: Vec<usize> = (0..4)
            .map(|c| map.iter().filter(|e| codes.contains(&e[c])).count())
            .collect();
        let alt: std::collections::BTreeSet<u16> = map
            .iter()
            .map(|e| e[3])
            .filter(|c| codes.contains(c))
            .collect();
        println!(
            "real receipt: ran; {} raw keys; giving a diagonal code (plain, shift, control, alt): \
             {columns:?}; distinct in alt {}",
            map.len(),
            alt.len()
        );
        assert!(!map.is_empty(), "the table read");
        assert_eq!(columns[..3], [0, 0, 0], "plain, shift, control");
        assert_eq!(alt.len(), 2, "two codes in Alt");
    }
}

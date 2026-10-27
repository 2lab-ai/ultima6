//! Spec 21: the travel search on crafted maps, the walk's stops through an engine whose game
//! reads one key a prompt and steps the party with it, and the real-data receipts on the user's
//! zip (counts only; skipped without it).

use std::cell::RefCell;
use std::rc::Rc;

use u6_formats::position::Position;

use super::*;
use crate::actors::party::StepResult;
use crate::actors::tail;
use crate::engine::{Engine, Io};
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
/// The object slot of a crafted door.
const DOOR_SLOT: usize = 0x200;

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
        c.ui.text == s.ui.text && c.ui.messages == s.ui.messages,
        "no line printed"
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
    let mut grid = Grid::new(Window::of(&c), 1, 0);
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

#[test]
fn the_search_reads_the_state_only_and_draws_no_rng() {
    let mut s = room((R + 1, R + 3));
    for y in R + 1..=R + 6 {
        tile(&mut s, R + 3, y, WALL);
    }
    tile(&mut s, R + 2, R + 4, HARM);
    s.actors.work.hic = 1;
    let before = s.clone();
    assert_eq!(reachable(&s, (R + 5, R + 3)), None);
    assert!(s == before, "the state as it was");
    let mut c = s.clone();
    c.actors.work.hic = 0;
    let mut grid = Grid::new(Window::of(&c), 1, 0);
    assert!(
        search(&mut c, &mut grid, (R + 1, R + 3), (R + 5, R + 3))
            .path
            .is_some()
    );
    assert_eq!(c.rng, s.rng, "the copy drew no RNG");
    assert!(c.main.pending_print.is_empty(), "and queued no line");
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
    for d in 0..8u8 {
        assert_eq!(dir_of(key_of(d)), Some(d));
    }
}

/// The direction of a direction key ([`key_of`]'s inverse).
fn dir_of(k: Key) -> Option<u8> {
    (0..8u8).find(|&d| key_of(d) == k)
}

/// What the stand-in game does after step `n` (from 1) it took.
type After = Box<dyn Fn(&mut State, u32)>;

/// An engine whose game has the command loop's shape: at the prompt (A4-0x7a2c and A4-0x7a42 set,
/// in the event wait) it reads one key, steps the party with a direction key's direction (the
/// player's step) and runs `after`, then takes 100 ms for the turn.
fn stand_in(s: State, after: After) -> Engine {
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
                }
                let k = io.next_key().await;
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

/// A walk east across the room, (R + 1, R + 3) to (R + 6, R + 3), with `setup` run on the room,
/// `after` after each step and each of `inject` at its tick: how it ended, and the keys pressed.
fn walk_east(
    setup: impl FnOnce(&mut State),
    after: After,
    inject: &[(u64, InputEvent)],
) -> (Option<End>, usize) {
    let mut s = room((R + 1, R + 3));
    setup(&mut s);
    let mut e = stand_in(s, after);
    e.tick(&[], 20);
    e.travel_to((R + 6, R + 3)).expect("reachable");
    run(&mut e, inject);
    (e.travel_end(), e.travel_log().len())
}

fn nothing() -> After {
    Box::new(|_, _| {})
}

#[test]
fn the_walk_east_arrives_in_five_presses() {
    assert_eq!(walk_east(|_| {}, nothing(), &[]), (Some(End::Arrived), 5));
}

#[test]
fn any_key_or_press_stops_it_a_release_or_a_move_does_not() {
    let at = |ev| vec![(8u64, ev)];
    for (ev, name) in [
        (InputEvent::Key(Key::Char('a')), "a key"),
        (
            InputEvent::Repeat(Key::Left),
            "a held key's repeat (spec 19 O1c)",
        ),
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
        let (end, pressed) = walk_east(|_| {}, nothing(), &at(ev));
        assert_eq!(end, Some(End::Input), "{name}");
        assert!(pressed < 5, "{name}: stopped short ({pressed})");
    }
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
            walk_east(|_| {}, nothing(), &at(ev)),
            (Some(End::Arrived), 5),
            "{name}"
        );
    }
}

#[test]
fn another_prompt_stops_it() {
    let more: After = Box::new(|s, n| {
        if n == 2 {
            s.ui.input.in_wait = true;
            s.ui.input.accept = false;
        }
    });
    assert_eq!(
        walk_east(|_| {}, more, &[]),
        (Some(End::Prompt), 2),
        "a wait off the prompt"
    );
    let line: After = Box::new(|s, n| {
        if n == 2 {
            s.ui.input.line_input = true;
        }
    });
    assert_eq!(
        walk_east(|_| {}, line, &[]),
        (Some(End::Prompt), 2),
        "the line editor"
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
    run(&mut e, &[]);
    assert_eq!(e.travel_end(), Some(End::Arrived), "round it");
    let keys: Vec<Key> = e.travel_log().iter().map(|&(_, k)| k).collect();
    assert_eq!(keys[0], Key::Right, "the first step east");
    assert_ne!(
        keys[1],
        Key::Right,
        "then round the NPC, not into it: {keys:?}"
    );
    assert_eq!(e.travel().map(|t| t.plans), None, "ended");
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

#[test]
fn the_turn_holds_it_and_a_finished_game_or_another_program_ends_it() {
    let prompt = room((R + 1, R + 3));
    let mut busy = prompt.clone();
    busy.main.reading = 0;
    busy.ui.input.in_wait = false;
    let mut t = Travel::start(&prompt, (R + 4, R + 3)).expect("reachable");
    let quiet = Polled {
        input: &[],
        buttons: 0,
        queued: false,
    };
    assert_eq!(t.next(&busy, &quiet), Next::Hold, "the turn runs");
    let queued = Polled {
        queued: true,
        ..quiet
    };
    assert_eq!(t.next(&prompt, &queued), Next::Hold, "a key waits");
    assert_eq!(t.next(&prompt, &quiet), Next::Press(Key::Right));
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
            eprintln!("skipped: {ZIP} not present");
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
        e
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
            (chebyshev((lp.x, lp.y), (p.x, p.y)), i)
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
    /// ticks), `given` (tick, input) fed at the tick it names, counting into `n`.
    fn until_prompt(e: &mut Engine, given: &[(u64, InputEvent)], n: &mut Counts) {
        let mut at = e.state().leader_position().quiet();
        let mut prompt = at_prompt(e);
        for _ in 0..20_000 {
            let tick = e.ticks() + 1;
            let input: Vec<InputEvent> = given
                .iter()
                .filter(|&&(t, _)| t == tick)
                .map(|&(_, ev)| ev)
                .collect();
            e.tick(&input, STEP_MS);
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
    /// arrives with one key a planned step and one move a key. The same inputs given by hand at
    /// the same ticks from the same start leave an equal state and an equal frame (no shortcut).
    #[test]
    fn the_party_travels_through_a_door_and_the_keys_walk_the_same() {
        let Some(files) = files() else { return };
        let mut e = castle(&files);
        let (doors, found) = closed_door(&e.state());
        let Some(door) = found else {
            panic!("no closed door with a cell before and beyond it ({doors} closed doors)");
        };
        let (t0, m0, ms0) = (e.ticks(), minutes(&e.state()), e.io().now_ms());
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
            until_prompt(&mut e, &[], &mut n);
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
                e.tick(&[u], STEP_MS);
                for _ in 0..400 {
                    let waits = {
                        let s = e.state();
                        s.main.reading == 0 && s.ui.input.target_pending == 1
                    };
                    if waits {
                        break;
                    }
                    e.tick(&[], STEP_MS);
                }
                let mut n = Counts::default();
                let at = cell_pixel(&e.state(), door.cell);
                let tick = e.ticks() + 1;
                for ev in crate::ui::menu::left_press(at.0, at.1) {
                    given.push((tick, ev));
                }
                until_prompt(&mut e, &given, &mut n);
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
            "travel: {doors} closed doors in the window; legs planned {} and {} steps; pressed {} \
             keys ({diagonals} diagonal) and {} by hand; the leader moved {} times; {} turns; \
             {} clock minutes; {} engine ms; {} ticks",
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
        }
        assert!(!k.travel_active() && k.travel_log().is_empty(), "no travel");
        assert!(*k.state() == *e.state(), "the inputs leave the same state");
        assert!(
            k.frame().pixels() == e.frame().pixels(),
            "and the same frame"
        );
    }
}

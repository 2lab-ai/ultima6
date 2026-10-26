//! Unit tests of the Use handlers on synthetic state (no `game` file: strings print empty and
//! the type tables read empty).

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use u6_formats::position::Position;

use super::{Ux, food, mech, misc};
use crate::engine::{Engine, Io};
use crate::files::Files;
use crate::objects::{IN_CONTAINER, Slot};
use crate::state::State;

/// Object slots the fixtures use.
const A: Slot = 0x200;
const B: Slot = 0x201;
const C: Slot = 0x202;

fn world() -> State {
    let mut s = State::empty();
    s.map.centre = Position {
        x: 100,
        y: 100,
        z: 0,
    };
    s.objects.location[1] = Position {
        x: 100,
        y: 100,
        z: 0,
    };
    s.objects.type_frame[1] = 0x17a;
    s.actors.party.members[0] = 1;
    s.actors.party.size = 1;
    s.actors.party.names = vec!["Abcdef".into()];
    s.actors.attrs.status[1] = 0x80;
    s.actors.objlist_tail = vec![0; 0x82];
    s
}

fn put(s: &mut State, slot: Slot, tf: u16, x: u16, y: u16) {
    s.objects.type_frame[slot as usize] = tf;
    s.objects.location[slot as usize] = Position { x, y, z: 0 };
}

fn link(s: &mut State) {
    crate::main_loop::test_support::link_placed_objects(s);
}

/// Runs `game` to its end with no input; returns the engine.
fn run<G, F>(s: State, game: G) -> Engine
where
    G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F,
    F: Future<Output = ()> + 'static,
{
    let mut e = Engine::with_game(s, Files::default(), game);
    for _ in 0..64 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    e
}

// The door, chest, fixed-light, secret-door, lock-pick and torch handlers Use reaches are
// objects::commands' ports since the integration; their tests are in objects/commands/tests.rs.

#[test]
fn eating_takes_one_off_a_counted_stack() {
    let mut s = world();
    s.objects.defs.stackable = vec![0x81];
    put(&mut s, A, 0x81, 100, 99);
    s.objects.qq[A as usize] = 3;
    put(&mut s, B, 0x80, 101, 99);
    link(&mut s);
    let e = run(s, |st, io| async move {
        let ux = Ux::new(&st, &io);
        food::eat(ux, A).await;
        food::eat(ux, B).await;
    });
    assert_eq!(e.state().objects.qq_of(A) & 0xff, 2);
    assert_eq!(e.state().objects.tf(B), 0, "an uncounted food goes");
}

#[test]
fn drink_fan_and_scroll_set_their_globals() {
    let mut s = world();
    put(&mut s, A, 0x73, 100, 99);
    s.actors.wind = 7;
    link(&mut s);
    let e = run(s, |st, io| async move {
        let ux = Ux::new(&st, &io);
        food::drink(ux, A).await;
        food::fan(ux).await;
        food::study_scroll(ux).await;
    });
    let s = e.state();
    assert_eq!(s.objects.tf(A), 0);
    assert_eq!(s.actors.objlist_tail[crate::actors::tail::ALCOHOL], 1);
    assert_eq!(s.actors.wind, 0, "7 + 1 wraps to north");
    assert_eq!(&s.actors.objlist_tail[0x5e..0x60], &[0, 1]);
}

#[test]
fn lever_opens_and_closes_its_portcullis_unless_blocked() {
    let mut s = world();
    put(&mut s, A, 0x10c, 100, 99);
    s.objects.qq[A as usize] = 0x0500;
    put(&mut s, B, 0x12d | 2 << 10, 104, 100);
    s.objects.qq[B as usize] = 0x0500;
    link(&mut s);
    let lever = |s: State| {
        run(s, |st, io| async move {
            mech::lever(Ux::new(&st, &io), A).await;
        })
        .state()
        .clone()
    };
    let gate_at = |s: &mut State| crate::objects::object_of_kind_at(s, 104, 100, 0, 0x136);
    let mut s = lever(s);
    assert_eq!(s.objects.tf(A) >> 10, 1);
    let g = gate_at(&mut s);
    assert!(g >= 0, "a portcullis closes on the marker");
    assert_eq!(
        s.objects.tf(g) >> 10,
        3,
        "frame 3 for a marker with frame bit 1"
    );
    let mut s = lever(s);
    assert_eq!(gate_at(&mut s), -1, "it opens again");
    // An actor under the marker keeps it open.
    s.objects.location[1] = Position {
        x: 104,
        y: 100,
        z: 0,
    };
    link(&mut s);
    let mut s = lever(s);
    assert_eq!(gate_at(&mut s), -1);
    assert_eq!(mech::actor_at(&mut s, 104, 100, 0), 1);
}

#[test]
fn keg_lights_once() {
    let mut s = world();
    put(&mut s, A, 0xdf, 100, 99);
    put(&mut s, B, 0xdf, 101, 99);
    link(&mut s);
    let e = run(s, |st, io| async move {
        let ux = Ux::new(&st, &io);
        mech::light_keg(ux, A).await;
        mech::light_keg(ux, B).await;
    });
    let s = e.state();
    assert_eq!(s.objects.tf(A) >> 10, 1);
    assert_eq!(s.objects.tf(B) >> 10, 0, "refused: one keg burns at a time");
    assert_eq!(s.actors.tail_word(crate::actors::tail::KEG_TIMER), 3);
    assert_eq!(
        s.actors.tail_word(crate::actors::tail::KEG_OBJECT),
        A as u16
    );
}

#[test]
fn keg_explosion_blows_up_doors_but_not_sealed_ones() {
    let mut s = world();
    put(&mut s, A, 0xdf | 1 << 10, 100, 99);
    put(&mut s, B, 0x12c | 5 << 10, 101, 99);
    put(&mut s, C, 0x12a | 13 << 10, 99, 99);
    link(&mut s);
    crate::objects::commands::explode_keg(&mut s, A);
    assert_eq!(s.objects.tf(A), 0, "the keg goes");
    assert_eq!(s.objects.tf(B), 0, "the door is blown up");
    assert_eq!(
        s.objects.tf(C) & 0x3ff,
        0x12a,
        "a magically locked door stays"
    );
}

#[test]
fn eggs_of_wisps_and_daemons_go_with_their_contents() {
    let mut s = world();
    put(&mut s, A, 0x14f, 110, 100);
    put(&mut s, B, 0x16a, 110, 100);
    put(&mut s, C, 0x14f, 112, 100);
    link(&mut s);
    crate::objects::move_into(&mut s, B, A, IN_CONTAINER);
    misc::clear_eggs(&mut s, 100, 100);
    assert_eq!(s.objects.tf(A), 0);
    assert_eq!(s.objects.tf(B), 0);
    assert_eq!(s.objects.tf(C), 0x14f, "an empty egg stays");
}

#[test]
fn mount_and_dismount_a_horse() {
    let mut s = world();
    s.actors.attrs.base_type_frame[1] = 0x17a;
    link(&mut s);
    crate::objects::commands::mount_horse(&mut s, 1, 100, 100, 2, false);
    assert_eq!(s.objects.tf(1), 0x1af | 2 << 10);
    let rear = crate::objects::object_of_kind_at(&mut s, 100, 100, 0, 0x1af);
    assert!(rear >= 0);
    crate::objects::commands::dismount(&mut s, 1);
    assert_eq!(s.objects.kind(1), 0x17a, "the rider's own type is back");
}

/// Runs `game` to its end, one `key` per tick while it waits; returns the engine.
fn run_keys<G, F>(s: State, game: G) -> Engine
where
    G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F,
    F: Future<Output = ()> + 'static,
{
    use crate::engine::{InputEvent, Key};
    let mut e = Engine::with_game(s, Files::default(), game);
    for _ in 0..64 {
        if e.finished() {
            break;
        }
        let key = if e.io().pending_events() == 0 {
            vec![InputEvent::Key(Key::Enter)]
        } else {
            Vec::new()
        };
        e.tick(&key, 16);
    }
    assert!(e.finished());
    e
}

/// Every map tile around the centre is `tile`, with `flags1` (bit 0 water, bit 1 impassable).
fn tiles(s: &mut State, tile: u8, flags1: u8) {
    s.map.window_tiles = vec![tile; crate::map::WINDOW_CELLS];
    // A world map, so the walk test reads the window rather than tile 0.
    s.map.world.surface = vec![0; u6_formats::map::SURFACE_CHUNKS.pow(2)];
    let mut f = vec![0u8; 0x800];
    f[usize::from(tile)] = flags1;
    s.tables.tileflags.flags1 = f;
}

#[test]
fn fishing_next_to_water_draws_once_after_the_hourly_test() {
    use crate::actors::tail::HOURLY_B;
    for caught_this_hour in [false, true] {
        let mut s = world();
        link(&mut s);
        tiles(&mut s, 0, 0x01);
        s.actors.objlist_tail[HOURLY_B] = u8::from(caught_this_hour);
        let mut expect = s.rng;
        if !caught_this_hour {
            expect.range(0, 4);
        }
        let e = run(s, |st, io| async move {
            misc::fish(Ux::new(&st, &io)).await;
        });
        assert_eq!(
            e.state().rng,
            expect,
            "0x12eff4 tests the hourly byte, then 0x12f000 draws 0x151f06(0, 4) only when it is 0"
        );
    }
}

/// A vortex cube holding the eight moonstones, the two lenses and the Codex in place at the
/// shrine, the party next to it. Returns the state, the cube and the Codex slots.
fn codex_shrine() -> (State, Slot, Slot) {
    let mut s = world();
    s.ui = crate::main_loop::test_support::test_ui();
    let (cx, cy) = (0x39b, 0x354);
    s.map.centre = Position { x: cx, y: cy, z: 0 };
    s.objects.location[1] = Position { x: cx, y: cy, z: 0 };
    let (cube, codex) = (A, B);
    put(&mut s, cube, 0x3e, cx, cy + 1);
    put(&mut s, codex, 0x3b, 0x39b, 0x353);
    put(&mut s, C, 0x18a, 0x399, 0x353);
    put(&mut s, 0x203, 0x18c, 0x39d, 0x353);
    for f in 0..8u16 {
        put(&mut s, 0x210 + f as Slot, 0x56 | f << 10, cx, cy + 1);
    }
    link(&mut s);
    for f in 0..8 {
        crate::objects::move_into(&mut s, 0x210 + f, cube, IN_CONTAINER);
    }
    (s, cube, codex)
}

#[test]
fn the_vortex_cube_refuses_before_the_codex_goes_while_the_ending_is_unported() {
    let (s, cube, codex) = codex_shrine();
    let done = Rc::new(std::cell::Cell::new(None));
    let d = Rc::clone(&done);
    let e = run_keys(s, move |st, io| async move {
        d.set(Some(misc::vortex_cube(Ux::new(&st, &io), cube).await));
    });
    assert_eq!(
        e.state().objects.kind(codex),
        0x3b,
        "the Codex stays while 0x13513a is a stub"
    );
    assert_eq!(
        done.get(),
        Some(false),
        "refused: the caller prints the cube's own refusal"
    );
    assert!(
        e.stub_hits().keys().any(|k| k.starts_with("0x13513a")),
        "the refusal is counted as the ending's stub hit"
    );
}

/// The party aboard a ship (actor 0, 0x19c) at the centre with the types `hold` in its hold
/// (slots A, B, …, each moved in after the one before, so the last is the first child), and
/// the types `beside` on the map east of the centre (slots from 0x210, quality 0).
fn ship_holding(land: bool, hold: &[u16], beside: &[u16]) -> State {
    let mut s = world();
    s.objects.location[0] = s.map.centre;
    s.objects.type_frame[0] = 0x19c;
    s.actors.attrs.status[0] = 0xc0;
    s.actors.party.members[1] = 0;
    s.actors.party.leader_index = 1;
    // A4-0x7b0b (objlist tail 0x79): no solo member.
    s.actors.objlist_tail[0x79] = 0xff;
    // Land: passable ground; else deep water (walkers blocked). Tile 0: what the walk test
    // reads without a world map.
    tiles(&mut s, 0, if land { 0 } else { 0x03 });
    for (i, &kind) in hold.iter().enumerate() {
        put(&mut s, A + i as Slot, kind, 100, 100);
    }
    for (i, &kind) in beside.iter().enumerate() {
        put(&mut s, 0x210 + i as Slot, kind, 101, 100);
    }
    link(&mut s);
    crate::objects::move_into(&mut s, 1, 0, crate::objects::IN_INVENTORY);
    for i in 0..hold.len() {
        crate::objects::move_into(&mut s, A + i as Slot, 0, crate::objects::IN_INVENTORY);
    }
    s
}

/// [`ship_holding`] with one skiff (0x19e) in the hold; returns the state and the skiff's slot.
fn ship_with_skiff(land: bool) -> (State, Slot) {
    (ship_holding(land, &[0x19e], &[]), A)
}

/// True while `slot` is in actor 0's inventory (the hold of whatever vessel actor 0 is).
fn in_hold(s: &State, slot: Slot) -> bool {
    s.objects.parent(slot) == 0 && s.objects.place(slot) == crate::objects::IN_INVENTORY
}

#[test]
fn a_ship_that_cannot_land_puts_the_party_into_its_skiff() {
    let (s, _) = ship_with_skiff(false);
    let e = run(s, |st, io| async move {
        super::travel::board(Ux::new(&st, &io), 0).await;
    });
    let s = e.state();
    assert_eq!(
        s.objects.kind(0),
        0x19e,
        "0x12f620: 0x11d0a4(0) takes the skiff out and boards it"
    );
    assert_eq!(s.actors.party.leader_index, s.actors.party.size, "aboard");
}

#[test]
fn a_ship_that_can_land_leaves_its_skiff_in_the_hold() {
    let (s, skiff) = ship_with_skiff(true);
    let e = run(s, |st, io| async move {
        super::travel::board(Ux::new(&st, &io), 0).await;
    });
    let s = e.state();
    assert_eq!(s.actors.party.leader_index, 0, "the party is on land");
    assert_ne!(
        s.objects.kind(0),
        0x19e,
        "0x12f5d4: 0x11d0a4(1) keeps the skiff"
    );
    assert_ne!(s.objects.place(skiff), 0, "the skiff is not put on the map");
}

/// As in the original: the cannot-land test looks for a skiff only (0x10b18c(vessel, 0x19e,
/// quality 0) at 0x12f60c..0x12f61e), the extraction takes the first skiff or raft
/// (0x11d118 / 0x11d126). A raft ahead of the skiff is the one boarded; the skiff stays.
#[test]
fn a_ship_that_cannot_land_boards_the_first_skiff_or_raft_of_its_hold() {
    let (raft, skiff) = (B, A);
    let s = ship_holding(false, &[0x19e, 0x19f], &[]);
    assert_eq!(
        crate::objects::first_child(&mut s.clone().objects, 0),
        raft,
        "fixture: the raft is the hold's first child"
    );
    let e = run(s, |st, io| async move {
        super::travel::board(Ux::new(&st, &io), 0).await;
    });
    let s = e.state();
    assert_eq!(s.objects.kind(0), 0x19f, "0x11d0a4(0) takes the raft out");
    assert_eq!(s.actors.party.leader_index, s.actors.party.size, "aboard");
    assert!(
        in_hold(&s, skiff),
        "the skiff moves on with actor 0 (0x11cfa0)"
    );
}

/// The land refusal's string (0x12cd7a): a synthetic marker the test puts at that address.
const LAND_REFUSAL: (u32, &[u8]) = (0x0012_cd7a, b"MARKER-LAND-REFUSAL");

/// Files with a synthetic `game` whose image (one CODE hunk from the image base) holds `text`,
/// NUL-terminated, at image address `addr`: what `game_string` reads for the string a handler
/// prints. The text is the test's own, no game text.
fn game_with_string(part: (u32, &[u8])) -> Files {
    game_with(&[part])
}

/// [`game_with_string`] for several `(addr, bytes)` parts: strings and the small A4 tables a
/// handler reads, the zeros after each ending it. The bytes are the test's own, no game data.
fn game_with(parts: &[(u32, &[u8])]) -> Files {
    let at = |addr: u32| (addr - crate::image::IMAGE_BASE) as usize;
    let end = parts
        .iter()
        .map(|&(a, b)| at(a) + b.len())
        .max()
        .unwrap_or(0);
    let mut hunk = vec![0u8; (end + 4) & !3];
    for &(addr, bytes) in parts {
        hunk[at(addr)..at(addr) + bytes.len()].copy_from_slice(bytes);
    }
    let game = crate::image::SyntheticGame::new()
        .loaded(false, hunk)
        .build();
    Files::new(std::collections::BTreeMap::from([(
        "game".to_string(),
        game,
    )]))
}

/// [`run`] with `files`.
fn run_on<G, F>(s: State, files: Files, game: G) -> Engine
where
    G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F,
    F: Future<Output = ()> + 'static,
{
    let mut e = Engine::with_game(s, files, game);
    for _ in 0..64 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    e
}

/// Everything the message scroll took: its lines and the one being written.
fn scroll_text(s: &State) -> String {
    let t = &s.ui.text;
    let mut all = s.ui.messages.join("\n");
    all.extend(
        t.transcript_line
            .iter()
            .map(|&b| crate::ui::wide::latin_char(b)),
    );
    all.extend(
        t.line[usize::from(t.line_start)..usize::from(t.line_len)]
            .iter()
            .map(|&b| crate::ui::wide::latin_char(b)),
    );
    all
}

/// Board with no land to step on, on [`ship_holding`] (`hold` with `qualities`) and the test
/// text layer, under a `game` that holds the land refusal's marker; returns the state.
fn board_unlanding(hold: &[u16], qualities: &[u8]) -> State {
    let mut s = ship_holding(false, hold, &[]);
    s.ui = crate::main_loop::test_support::test_ui();
    for (i, &q) in qualities.iter().enumerate() {
        s.objects.qq[(A as usize) + i] = u16::from(q) << 8;
    }
    let e = run_on(s, game_with_string(LAND_REFUSAL), |st, io| async move {
        super::travel::board(Ux::new(&st, &io), 0).await;
    });
    e.state().clone()
}

/// A raft alone in the hold does not count (the test at 0x12f612 is for 0x19e): the land
/// refusal 0x12cd7a is printed, the party still aboard its ship.
#[test]
fn a_ship_holding_only_a_raft_cannot_land() {
    let s = board_unlanding(&[0x19f], &[0]);
    assert_eq!(s.objects.kind(0), 0x19c, "still the ship");
    assert_eq!(s.actors.party.leader_index, s.actors.party.size, "aboard");
    assert!(in_hold(&s, A), "the raft stays in the hold");
    let refusal = String::from_utf8_lossy(LAND_REFUSAL.1);
    assert!(
        scroll_text(&s).contains(&*refusal),
        "the land refusal printed"
    );
}

/// A skiff of a quality alone in the hold does not count either: the test looks for quality 0
/// (0x10b18c(vessel, 0x19e, 0) at 0x12f60c..0x12f61e), so the refusal is printed and the skiff
/// stays.
#[test]
fn a_ship_holding_only_a_skiff_of_a_quality_cannot_land() {
    let s = board_unlanding(&[0x19e], &[5]);
    assert_eq!(s.objects.kind(0), 0x19c, "still the ship");
    assert_eq!(s.actors.party.leader_index, s.actors.party.size, "aboard");
    assert!(in_hold(&s, A), "the skiff stays in the hold");
    let refusal = String::from_utf8_lossy(LAND_REFUSAL.1);
    assert!(
        scroll_text(&s).contains(&*refusal),
        "the land refusal printed"
    );
}

/// With a skiff of quality 0 aboard the test passes, and the extraction takes the first skiff
/// or raft of any quality (0x11d118 / 0x11d126): a skiff of quality 5 ahead of it is the one
/// boarded; no refusal.
#[test]
fn a_ship_that_cannot_land_boards_the_first_skiff_of_any_quality() {
    let (plain, marked) = (A, B);
    let s = board_unlanding(&[0x19e, 0x19e], &[0, 5]);
    assert_eq!(s.objects.kind(0), 0x19e, "a skiff boarded");
    assert_eq!(s.actors.party.leader_index, s.actors.party.size, "aboard");
    assert!(
        !in_hold(&s, marked),
        "the first one, of quality 5, is out of the hold"
    );
    assert!(
        in_hold(&s, plain),
        "the skiff of quality 0 moves on with actor 0"
    );
    let refusal = String::from_utf8_lossy(LAND_REFUSAL.1);
    assert!(!scroll_text(&s).contains(&*refusal), "no refusal");
}

/// A raft of a quality ahead of the skiff of quality 0 is the one boarded.
#[test]
fn a_ship_that_cannot_land_boards_a_raft_of_a_quality_ahead_of_its_skiff() {
    let (skiff, raft) = (A, B);
    let s = board_unlanding(&[0x19e, 0x19f], &[0, 9]);
    assert_eq!(s.objects.kind(0), 0x19f, "the raft boarded");
    assert!(!in_hold(&s, raft), "the raft is out of the hold");
    assert!(in_hold(&s, skiff), "the skiff moves on with actor 0");
}

/// As in the original: landing (0x11d0a4(1), the extraction skipped at 0x11d0f2) leaves the
/// skiff in the emptied actor 0, parked at level 14 (0x11d190..0x11d19e, 0x10c9d2 carrying
/// its contents); boarding makes actor 0 a copy of the first member (0x122fbe, attributes
/// only) and moves it with its contents (0x11cfa0), so the next vessel boarded has the skiff.
#[test]
fn the_skiff_left_in_the_hold_goes_with_the_next_vessel_boarded() {
    let raft = 0x210;
    let s = ship_holding(true, &[0x19e], &[0x19f]);
    let e = run(s, move |st, io| async move {
        super::travel::board(Ux::new(&st, &io), 0).await;
        {
            let s = st.borrow();
            assert_eq!(s.actors.party.leader_index, 0, "landed");
            assert_eq!(s.objects.z(0), 0xe, "actor 0 parked at level 14");
            assert!(in_hold(&s, A), "the skiff stays in actor 0");
        }
        super::travel::board(Ux::new(&st, &io), raft).await;
    });
    let s = e.state();
    assert_eq!(s.objects.kind(0), 0x19f, "aboard the raft");
    assert_eq!(s.actors.party.leader_index, s.actors.party.size, "aboard");
    assert!(in_hold(&s, A), "the raft carries the skiff");
}

/// The machine's animation slot: tile 0x180 (the base tile of type 0xec) cycles from 0x1c0;
/// record `0x1000 + k` marks source tile 0x1c0 + k. Song 1 plays with sound on.
fn machine_room() -> State {
    let mut s = world();
    let mut base = vec![0u16; 0x400];
    base[0xec] = 0x180;
    s.tables.basetile.base = base;
    s.tables.anim.count = 1;
    s.tables.anim.slots = vec![u6_formats::animdata::AnimEntry {
        tile_to_animate: 0x180,
        first_anim_frame: 0x1c0,
        and_mask: 0,
        shift: 0,
    }];
    for k in 0..0x10u16 {
        s.gfx.record_of[usize::from(0x1c0 + k)] = 0x1000 + k;
    }
    crate::sound::set_sound_flag(&mut s, true);
    s.sound.current = 1;
    s.sound.loaded = 1;
    link(&mut s);
    s
}

#[test]
fn the_machine_shows_each_frame_and_its_sound_runs_before_the_song_returns() {
    use crate::sound::MedCall;
    let mut e = Engine::with_game(machine_room(), Files::default(), |st, io| async move {
        misc::machine(Ux::new(&st, &io)).await;
    });
    let (mut shown, mut effect_at, mut song_at) = (Vec::new(), None, None);
    for tick in 0..400u32 {
        if e.finished() {
            break;
        }
        e.tick(&[], 20);
        let s = e.state();
        shown.push(s.gfx.record_of[0x180]);
        for (_, call) in &s.sound.calls {
            match call {
                MedCall::PlayEffect if effect_at.is_none() => effect_at = Some(tick),
                MedCall::PlayBuffer if effect_at.is_some() && song_at.is_none() => {
                    song_at = Some(tick)
                }
                _ => {}
            }
        }
    }
    assert!(e.finished());
    // 0x12d018..0x12d024: step k shows frame first + k / 4; each step is presented once.
    for k in 1..4u16 {
        let ticks = shown.iter().filter(|&&r| r == 0x1000 + k).count();
        assert_eq!(ticks, 4, "frame {k} for steps {}..{}", 4 * k, 4 * k + 3);
    }
    assert!(
        shown.iter().all(|&r| r < 0x1004),
        "no frame past first + 3: {shown:x?}"
    );
    let fx = effect_at.expect("the effect plays");
    let song = song_at.expect("the song returns");
    assert!(
        song - fx >= 101,
        "0x1623be(100) waits 101 vertical blanks between the effect and the song: {fx} -> {song}"
    );
}

#[test]
fn the_telescope_shows_each_step_of_its_slide() {
    use crate::engine::{InputEvent, Key};
    let mut s = world();
    s.ui = crate::main_loop::test_support::test_ui();
    // The right arrow (raw 0x4e) gives the east cursor-key code.
    s.ui.tables.keymap[0x4e] = [0x14d, 0x14d, 0x14d, 0];
    put(&mut s, A, 0x9a, 100, 99);
    link(&mut s);
    tiles(&mut s, 0, 0);
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        super::travel::telescope(Ux::new(&st, &io), A).await;
    });
    e.tick(&[InputEvent::Key(Key::Right)], 16);
    let mut ticks = 1;
    while !e.finished() && ticks < 400 {
        e.tick(&[], 16);
        ticks += 1;
    }
    assert!(e.finished());
    // Without the image the direction table reads (0, 0) and no tile stops the slide: 41
    // steps (0x12febc: while the count 40.. stays ≥ 0), each held one vertical blank.
    assert!(ticks > 41, "the slide took {ticks} ticks");
    let c = e.state().map.centre;
    assert_eq!((c.x, c.y), (100, 100), "back on the party");
}

/// A deflated balloon (0x1a4) at A next to the leader; every tile impassable but `open`.
fn balloon_field(open: &[(u16, u16)]) -> State {
    let mut s = world();
    put(&mut s, A, 0x1a4, 100, 101);
    link(&mut s);
    tiles(&mut s, 2, 0x02);
    let [ox, oy] = s.map.window_origin;
    for &(x, y) in open {
        s.map.window_tiles[crate::map::cell(usize::from(x - ox), usize::from(y - oy))] = 0;
    }
    s
}

/// The inflated balloons (0x1a7) and where they are.
fn inflated(s: &State) -> Vec<(u16, u16)> {
    (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&i| s.objects.kind(i) == 0x1a7)
        .map(|i| (s.objects.x(i), s.objects.y(i)))
        .collect()
}

#[test]
fn inflating_a_balloon_tries_the_tiles_as_the_original_and_keeps_it_inflated_without_room() {
    let use_it = |s: State| {
        run(s, |st, io| async move {
            super::travel::inflate(Ux::new(&st, &io), A).await;
        })
        .state()
        .clone()
    };
    // West of the leader (i 0, j 1) is the first tile tried that is open.
    let s = use_it(balloon_field(&[(99, 100), (101, 101)]));
    let deflated = |s: &State| {
        (0x100..0x400)
            .filter(|&i| s.objects.kind(i) == 0x1a4)
            .count()
    };
    assert_eq!(deflated(&s), 0, "the deflated balloon is used up");
    assert_eq!(inflated(&s), [(99, 100)]);
    // Only the northwest tile open: 0x12ec16..0x12ec1c skip (i, j) = (0, 0), the leader's own
    // tile is tried and blocked by the leader, so no room. As in the original (0x12eb22 removes
    // the deflated balloon first, 0x12ebf2 creates the inflated one at the leader's tile, the
    // no-room line leaves it there), the balloon stays inflated at the leader's feet.
    let s = use_it(balloon_field(&[(99, 99)]));
    assert_eq!(deflated(&s), 0);
    assert_eq!(inflated(&s), [(100, 100)]);
}

#[test]
fn fishing_counts_water_only_on_the_diagonal_the_original_reads() {
    use crate::actors::tail::HOURLY_B;
    // Water on one tile only (tile 0, flags1 bit 0); every other tile is dry tile 2.
    let draws = |wet: (u16, u16)| {
        let mut s = world();
        link(&mut s);
        tiles(&mut s, 2, 0);
        s.tables.tileflags.flags1[0] = 0x01;
        let [ox, oy] = s.map.window_origin;
        let c = crate::map::cell(usize::from(wet.0 - ox), usize::from(wet.1 - oy));
        s.map.window_tiles[c] = 0;
        s.actors.objlist_tail[HOURLY_B] = 0;
        let before = s.rng;
        let e = run(s, |st, io| async move {
            misc::fish(Ux::new(&st, &io)).await;
        });
        e.state().rng != before
    };
    // 0x12efc4..0x12efd0 add the outer counter to both x and y: (x-1, y-1), (x, y), (x+1, y+1).
    for wet in [(99, 99), (100, 100), (101, 101)] {
        assert!(draws(wet), "water at {wet:?} is read: the catch is drawn");
    }
    for wet in [(100, 99), (101, 100), (99, 101), (101, 99)] {
        assert!(!draws(wet), "water at {wet:?} is not read: no draw");
    }
}

#[test]
fn the_telescope_pointed_nowhere_leaves_the_view_on_it() {
    use crate::engine::{InputEvent, Key};
    let mut s = world();
    s.ui = crate::main_loop::test_support::test_ui();
    put(&mut s, A, 0x9a, 100, 99);
    link(&mut s);
    tiles(&mut s, 0, 0);
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        super::travel::telescope(Ux::new(&st, &io), A).await;
    });
    e.tick(&[InputEvent::Key(Key::Char('a'))], 16);
    for _ in 0..64 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    let c = e.state().map.centre;
    // 0x12fe80 branches to the exit 0x12ff46 without restoring A4-0x7b78 / -0x7b76; the next
    // player step re-centres on the leader (0x11d536, 0x11d55c).
    assert_eq!((c.x, c.y), (100, 99), "the centre stays on the telescope");
}

/// Dig's and the telescope's direction prompts and the direction's line (`%s.\n`) both print,
/// and the direction names' slots (A4-0x61be + 4 n), the test's own copies of the addresses;
/// where the synthetic `game` keeps north's name (no site there).
const DIG_PROMPT: u32 = 0x0012_cd00;
const SCOPE_PROMPT: u32 = 0x0012_ced6;
const DIR_LINE: u32 = 0x0012_cd16;
const DIR_SLOTS: u32 = crate::image::A4 - 0x61be;
const NORTH_TEXT: u32 = crate::image::IMAGE_BASE + 0x400;

/// The synthetic `game` of a direction's line (the test's own text): the two prompts, the
/// line's format and north's name at the direction table's first slot (an absolute pointer).
fn direction_parts() -> Vec<(u32, Vec<u8>)> {
    vec![
        (DIG_PROMPT, b"Zq1".to_vec()),
        (SCOPE_PROMPT, b"Zq2".to_vec()),
        (DIR_LINE, b"%s.\n".to_vec()),
        (DIR_SLOTS, NORTH_TEXT.to_be_bytes().to_vec()),
        (NORTH_TEXT, b"Zn0".to_vec()),
    ]
}

/// Dig, or (`scope`) the telescope A, answered with the up arrow (north) under the `game` of
/// [`direction_parts`], with a Korean pack of a form for north's name and each prompt installed
/// (`pack`) or no pack: the scroll (each cell as its character) and the counts.
fn direction_line(scope: bool, pack: bool) -> (String, [u32; 2]) {
    use crate::engine::{InputEvent, Key};
    let parts = direction_parts();
    let refs: Vec<(u32, &[u8])> = parts.iter().map(|(a, b)| (*a, b.as_slice())).collect();
    let mut s = world();
    s.ui = crate::main_loop::test_support::test_ui();
    // The up arrow (raw 0x4c) gives north's cursor-key code.
    s.ui.tables.keymap[0x4c] = [0x148; 4];
    put(&mut s, A, 0x9a, 100, 99);
    link(&mut s);
    tiles(&mut s, 0, 0);
    if pack {
        crate::locale::strings::install_test_forms(
            &mut s,
            &refs,
            &[(b"Zn0", "북"), (b"Zq1", "가"), (b"Zq2", "나")],
        );
    }
    let mut e = Engine::with_game(s, game_with(&refs), move |st, io| async move {
        let ux = Ux::new(&st, &io);
        if scope {
            super::travel::telescope(ux, A).await;
        } else {
            mech::dig(ux).await;
        }
    });
    e.tick(&[InputEvent::Key(Key::Up)], 16);
    for _ in 0..400 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    let s = e.state();
    let t = &s.ui.text;
    let mut all = s.ui.messages.join("\n");
    let pending = &t.line[usize::from(t.line_start)..usize::from(t.line_len)];
    let cells = t.transcript_line.iter().chain(pending);
    all.extend(cells.map(|&c| crate::ui::wide::cell_char(c)));
    (all.trim_end().to_string(), s.locale.texts.get())
}

#[test]
fn dig_and_the_telescope_show_the_packs_direction_name_and_without_a_pack_the_english() {
    // The prompt (catalogued since 2026-10-25), then north's name (a label read for display)
    // through the line's format (no word of its own: as it is); the prompt and the name the
    // pack's, both found there, or the English uncounted.
    for (what, scope, prompt, ko) in [
        ("dig", false, "Zq1", "가"),
        ("the telescope", true, "Zq2", "나"),
    ] {
        assert_eq!(
            direction_line(scope, true),
            (format!("{ko}북."), [2, 0]),
            "{what}: the pack's prompt and name"
        );
        assert_eq!(
            direction_line(scope, false),
            (format!("{prompt}Zn0."), [0, 0]),
            "{what}: the English"
        );
    }
}

/// The made-up map tiles of the drawbridge site: water everywhere, one row of a bank tile the
/// made-up end list holds (A4-0x6614 in [`bridge_game`]).
const MOAT: u8 = 2;
const BANK: u8 = 0x43;
const BANK_ROW: u16 = 104;
/// Markers at the crank's line addresses: no bridge (0x12ca2a), then results 0..4 (nothing
/// moved, raised, lowered, something on it, something in the way).
const NO_BRIDGE: &str = "MARK-NO-BRIDGE";
const BRIDGE_LINES: [&str; 5] = [
    "MARK-NOTHING",
    "MARK-RAISED",
    "MARK-LOWERED",
    "MARK-ON-IT",
    "MARK-IN-THE-WAY",
];
/// The crank's quality and the first slot of the bridge's pieces.
const CRANK_Q: u16 = 7;
const PIECES: Slot = 0x210;

/// A synthetic `game` with the markers at the crank's lines and an end list holding [`BANK`].
fn bridge_game() -> Files {
    let (off, _) = super::defs::BRIDGE_ENDS;
    let ends = [0x41, 0x42, BANK, 0x44, 0x45];
    let mut parts: Vec<(u32, &[u8])> = vec![(mech::STR_NO_BRIDGE, NO_BRIDGE.as_bytes())];
    parts.extend(
        mech::STR_BRIDGE
            .iter()
            .zip(BRIDGE_LINES)
            .map(|(&a, m)| (a, m.as_bytes())),
    );
    parts.push((crate::image::A4.wrapping_add_signed(off), &ends));
    game_with(&parts)
}

/// A crank (0x120, quality [`CRANK_Q`]) at A next to the party and its bridge raised as
/// 0x12de3a leaves it: frame 6 of the quality at (103, 101), frames 7, 7, 7 and 8 east of it.
/// Water below down to [`BANK_ROW`]; the two crank tiles have animation slots.
fn drawbridge_site() -> State {
    let mut s = world();
    s.ui = crate::main_loop::test_support::test_ui();
    put(&mut s, A, 0x120, 100, 99);
    s.objects.qq[A as usize] = CRANK_Q << 8;
    for (i, frame) in [6u16, 7, 7, 7, 8].into_iter().enumerate() {
        put(
            &mut s,
            PIECES + i as Slot,
            0x10d | frame << 10,
            103 + i as u16,
            101,
        );
    }
    s.objects.qq[PIECES as usize] = CRANK_Q << 8;
    link(&mut s);
    tiles(&mut s, MOAT, 0);
    let oy = s.map.window_origin[1];
    for x in 0..crate::map::WINDOW {
        s.map.window_tiles[crate::map::cell(x, usize::from(BANK_ROW - oy))] = BANK;
    }
    s.tables.anim.count = 2;
    s.tables.anim.slots = [0x3f1, 0x3fc]
        .map(|tile| u6_formats::animdata::AnimEntry {
            tile_to_animate: tile,
            first_anim_frame: tile,
            and_mask: 0,
            shift: 0,
        })
        .to_vec();
    s.map.anim_enabled = vec![0; 2];
    s
}

/// The drawbridge pieces on the map: (x, y, frame, quality), sorted.
fn bridge_pieces(s: &State) -> Vec<(u16, u16, u16, u16)> {
    let mut v: Vec<_> = (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&i| s.objects.kind(i) == 0x10d && s.objects.place(i) == 0)
        .map(|i| {
            let o = &s.objects;
            (o.x(i), o.y(i), o.tf(i) >> 10, o.qq_of(i) >> 8)
        })
        .collect();
    v.sort();
    v
}

/// The bridge of [`drawbridge_site`] up: one row, the quality on its first piece.
fn raised() -> Vec<(u16, u16, u16, u16)> {
    let mut v = vec![(103, 101, 6, CRANK_Q)];
    v.extend((104..107).map(|x| (x, 101, 7, 0)));
    v.push((107, 101, 8, 0));
    v.sort();
    v
}

/// The bridge of [`drawbridge_site`] down: rows of frames 3 (with the quality), 4, 4, 5 from
/// (104, 102) above the bank row, which gets 0, 1, 1, 2.
fn lowered() -> Vec<(u16, u16, u16, u16)> {
    let mut v = Vec::new();
    for y in 102..BANK_ROW {
        v.extend([
            (104, y, 3, CRANK_Q),
            (105, y, 4, 0),
            (106, y, 4, 0),
            (107, y, 5, 0),
        ]);
    }
    v.extend([
        (104, BANK_ROW, 0, 0),
        (105, BANK_ROW, 1, 0),
        (106, BANK_ROW, 1, 0),
        (107, BANK_ROW, 2, 0),
    ]);
    v.sort();
    v
}

/// Works the crank A to its end under [`bridge_game`]: Use, or the Telekinesis spell (0x15)
/// with the crank chosen. Returns the state and the crank tiles' animation flags the first
/// time any was on (empty: never).
fn work_crank(mut s: State, telekinesis: bool) -> (State, Vec<u8>) {
    if telekinesis {
        s.ui.input.selection = A;
        s.ui.input.target = (s.objects.x(A) as i16, s.objects.y(A) as i16);
    }
    let mut e = Engine::with_game(s, bridge_game(), move |st, io| async move {
        if telekinesis {
            crate::magic::dispatch(&st, &io, 0x15, true).await;
        } else {
            mech::drawbridge(Ux::new(&st, &io), A).await;
        }
    });
    let mut turning = Vec::new();
    for _ in 0..2000 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
        let a = &e.state().map.anim_enabled;
        if turning.is_empty() && a.iter().any(|&f| f != 0) {
            turning = a.clone();
        }
    }
    assert!(e.finished());
    (e.state().clone(), turning)
}

/// The view refreshes paced so far ([`crate::combat::present::REFRESH_MS`] waits).
fn refreshes(s: &State) -> u32 {
    let ms = crate::combat::present::REFRESH_MS;
    s.combat.paced.get(&ms).copied().unwrap_or(0)
}

#[test]
fn the_drawbridge_crank_lowers_its_raised_bridge_and_raises_it_again() {
    let s = drawbridge_site();
    assert_eq!(
        bridge_pieces(&s),
        raised(),
        "fixture: up as 0x12de3a leaves it"
    );
    let (s, turning) = work_crank(s, false);
    assert_eq!(
        bridge_pieces(&s),
        lowered(),
        "frame 6: the row goes, the rows come down to the bank"
    );
    assert_eq!(turning, [1, 1], "0x101ff6: the crank tiles turn forward");
    assert_eq!(s.map.anim_enabled, [0, 0], "and stop after the line");
    assert_eq!(
        refreshes(&s),
        20,
        "20 view refreshes (0x102ef0), each paced"
    );
    let (s, turning) = work_crank(s, false);
    assert_eq!(bridge_pieces(&s), raised(), "frame 3: back up into one row");
    assert_eq!(turning, [2, 2], "backward while it rises");
    assert_eq!(s.map.anim_enabled, [0, 0]);
    assert_eq!(refreshes(&s), 40);
    let t = scroll_text(&s);
    let (down, up) = (t.find(BRIDGE_LINES[2]), t.find(BRIDGE_LINES[1]));
    assert!(
        down.is_some() && up.is_some() && down < up,
        "result 2's line, then result 1's: {t:?}"
    );
    for other in [BRIDGE_LINES[0], BRIDGE_LINES[3], BRIDGE_LINES[4], NO_BRIDGE] {
        assert!(!t.contains(other), "no {other}");
    }
}

#[test]
fn a_drawbridge_with_something_in_its_way_or_on_it_stays() {
    // Up, with an object on the first n − 4 = 2 tiles of a row it would come down on: 4.
    let mut s = drawbridge_site();
    crate::objects::create_on_map(&mut s.objects, 0x77, 0, 1, 105, 103, 0);
    let (s, _) = work_crank(s, false);
    assert_eq!(bridge_pieces(&s), raised(), "result 4: it stays up");
    assert!(scroll_text(&s).contains(BRIDGE_LINES[4]));
    // Down, with an object on it: 3.
    let (mut s, _) = work_crank(drawbridge_site(), false);
    crate::objects::create_on_map(&mut s.objects, 0x77, 0, 1, 107, BANK_ROW, 0);
    let (s, _) = work_crank(s, false);
    assert_eq!(bridge_pieces(&s), lowered(), "result 3: it stays down");
    assert!(scroll_text(&s).contains(BRIDGE_LINES[3]));
    assert!(!scroll_text(&s).contains(BRIDGE_LINES[1]));
}

#[test]
fn a_lowered_drawbridge_covers_what_its_check_leaves_out_and_lies_under_the_bank_row() {
    // 0x12de3a checks the first n − 4 tiles of each row above the bank row only: an object on
    // a row's third tile and one on the bank row do not stop it. The bank row's pieces move
    // what lies on their tiles there again (0x10c9d2), ahead of the piece; above, the new
    // piece comes first.
    let mut s = drawbridge_site();
    let third = crate::objects::create_on_map(&mut s.objects, 0x77, 0, 1, 106, 102, 0);
    let bank = crate::objects::create_on_map(&mut s.objects, 0x77, 0, 1, 105, BANK_ROW, 0);
    let (mut s, _) = work_crank(s, false);
    assert_eq!(bridge_pieces(&s), lowered());
    assert!(scroll_text(&s).contains(BRIDGE_LINES[2]));
    assert_eq!(super::first_at(&mut s, 105, BANK_ROW, 0), bank);
    let first = super::first_at(&mut s, 106, 102, 0);
    assert_eq!(s.objects.kind(first), 0x10d, "the piece ahead of it");
    assert_eq!(super::next_here(&mut s), third);
}

#[test]
fn a_crank_without_its_bridge_or_with_a_piece_of_another_frame_says_so() {
    // No piece of the crank's quality: the no-bridge line, nothing turns.
    let mut s = drawbridge_site();
    s.objects.qq[A as usize] = 9 << 8;
    let (s, turning) = work_crank(s, false);
    assert_eq!(bridge_pieces(&s), raised());
    assert!(scroll_text(&s).contains(NO_BRIDGE));
    assert!(turning.is_empty(), "the crank tiles do not turn");
    assert_eq!(refreshes(&s), 0);
    // The first piece of the quality has frame 7: the crank turns, result 0.
    let mut s = drawbridge_site();
    s.objects.qq[PIECES as usize] = 0;
    s.objects.qq[PIECES as usize + 1] = CRANK_Q << 8;
    let before = bridge_pieces(&s);
    let (s, turning) = work_crank(s, false);
    assert_eq!(bridge_pieces(&s), before, "result 0: nothing moved");
    assert!(scroll_text(&s).contains(BRIDGE_LINES[0]));
    assert_eq!(turning, [1, 1]);
    assert_eq!(refreshes(&s), 20);
}

#[test]
fn telekinesis_on_the_crank_works_the_bridge_as_use_does() {
    let (by_use, _) = work_crank(drawbridge_site(), false);
    let (by_spell, turning) = work_crank(drawbridge_site(), true);
    assert_eq!(bridge_pieces(&by_spell), lowered(), "0x118e98 → 0x12e3f6");
    assert_eq!(bridge_pieces(&by_spell), bridge_pieces(&by_use));
    assert_eq!(turning, [1, 1]);
    assert!(scroll_text(&by_spell).contains(BRIDGE_LINES[2]));
}

/// Every object slot as it is (type and frame, place, quantity and quality, status, list link)
/// and the free slots: what a drawbridge that does not move leaves as it was.
type Objects = (
    Vec<u16>,
    Vec<Position>,
    Vec<u16>,
    Vec<u8>,
    Vec<Slot>,
    (Slot, i16),
);

fn all_objects(s: &State) -> Objects {
    let o = &s.objects;
    (
        o.type_frame.clone(),
        o.location.clone(),
        o.qq.clone(),
        o.obj_status.clone(),
        o.next.clone(),
        o.free,
    )
}

/// Made-up objects away from the bridge until `left` object slots are free.
fn fill_pool(s: &mut State, left: i16) {
    while s.objects.free.1 > left {
        let o = crate::objects::create_on_map(&mut s.objects, 0x77, 0, 1, 90, 110, 0);
        assert!(o >= 0);
    }
}

#[test]
fn a_drawbridge_the_object_pool_cannot_hold_stays_as_it_is() {
    // Lowering frees the raised row's 5 pieces, then makes 3 rows of 4: 7 free slots needed.
    // With 6 nothing is made or removed (result 0); with 7 it comes down.
    let mut s = drawbridge_site();
    fill_pool(&mut s, 6);
    let before = all_objects(&s);
    let (s, turning) = work_crank(s, false);
    assert_eq!(all_objects(&s), before, "every object as it was");
    assert_eq!(bridge_pieces(&s), raised());
    let t = scroll_text(&s);
    assert!(
        t.contains(BRIDGE_LINES[0]) && !t.contains(BRIDGE_LINES[2]),
        "result 0's line: {t:?}"
    );
    assert_eq!(turning, [1, 1], "the crank still turns");
    let mut s = drawbridge_site();
    fill_pool(&mut s, 7);
    let (s, _) = work_crank(s, false);
    assert_eq!(bridge_pieces(&s), lowered(), "7 free slots are enough");
    // Raising makes its frame 6 piece before any piece goes, each later one after one went:
    // 1 free slot needed.
    let (mut s, _) = work_crank(drawbridge_site(), false);
    fill_pool(&mut s, 0);
    let before = all_objects(&s);
    let (s, _) = work_crank(s, false);
    assert_eq!(all_objects(&s), before, "every object as it was");
    assert_eq!(bridge_pieces(&s), lowered());
    let t = scroll_text(&s);
    assert!(
        t.contains(BRIDGE_LINES[0]) && !t.contains(BRIDGE_LINES[1]),
        "result 0's line: {t:?}"
    );
    let (mut s, _) = work_crank(drawbridge_site(), false);
    fill_pool(&mut s, 1);
    let (s, _) = work_crank(s, false);
    assert_eq!(bridge_pieces(&s), raised(), "1 free slot is enough");
}

#[test]
fn a_raised_drawbridge_over_a_column_with_no_end_tile_stays_up() {
    // No bank row: the original would walk down the column for ever; the port stops and
    // leaves everything as it was, result 0.
    let mut s = drawbridge_site();
    s.map.window_tiles.iter_mut().for_each(|t| *t = MOAT);
    let before = all_objects(&s);
    let (s, _) = work_crank(s, false);
    assert_eq!(all_objects(&s), before, "every object as it was");
    let t = scroll_text(&s);
    assert!(
        t.contains(BRIDGE_LINES[0]) && !t.contains(BRIDGE_LINES[2]),
        "result 0's line: {t:?}"
    );
}

/// The made-up mantra of virtue 3 (the test's own text) and where the test's `game` keeps it.
const MANTRA_TEXT: u32 = 0x0013_8000;
const MANTRA: &[u8] = b"Zqv";

/// The shrine of virtue 3 (type 0xf5) at A, north of the leader at (100, 100), and at `stone` a
/// moonstone (0x49) of frame 3 at B with a force field (0x33) at C on its tile.
fn mantra_site(stone: (u16, u16)) -> State {
    let mut s = world();
    s.ui = crate::main_loop::test_support::test_ui();
    put(&mut s, A, 0xf5, 100, 99);
    put(&mut s, B, 0x49 | 3 << 10, stone.0, stone.1);
    put(&mut s, C, 0x33, stone.0, stone.1);
    link(&mut s);
    s
}

/// Use on the shrine at A, the mantra typed and Return, under a `game` whose mantra table
/// (A4-0x660e) names [`MANTRA`] for virtue 3; returns the state.
fn chant(s: State) -> State {
    use crate::engine::{InputEvent, Key};
    let slot = crate::image::A4.wrapping_add_signed(misc::MANTRAS + 3 * 4);
    let game = game_with(&[(slot, &MANTRA_TEXT.to_be_bytes()), (MANTRA_TEXT, MANTRA)]);
    let mut e = Engine::with_game(s, game, |st, io| async move {
        misc::mantra(Ux::new(&st, &io), A).await;
    });
    let mut typed = MANTRA
        .iter()
        .map(|&c| InputEvent::Text(char::from(c)))
        .chain([InputEvent::Key(Key::Enter)]);
    for _ in 0..400 {
        if e.finished() {
            break;
        }
        let input: Vec<InputEvent> = if e.io().pending_events() == 0 {
            typed.next().into_iter().collect()
        } else {
            Vec::new()
        };
        e.tick(&input, 16);
    }
    assert!(e.finished());
    e.state().clone()
}

#[test]
fn the_mantra_frees_a_stone_round_the_leader_not_round_the_shrine() {
    // 0x12edca..0x12ee5a centre the 3 x 3 on the leader (A4+0x47ec[A4-0x7b7a]), whose own
    // tile 0x12ee70..0x12ee78 leave out: the shrine's tile (100, 99) is in it, the leader's
    // (100, 100) and (101, 98), next to the shrine, are not.
    for (stone, freed) in [((100, 99), true), ((100, 100), false), ((101, 98), false)] {
        let s = chant(mantra_site(stone));
        assert_eq!(
            s.objects.tf(C) == 0,
            freed,
            "the field with the stone at {stone:?} goes: {freed}"
        );
    }
}

/// The serpent figure's lines (0x12ff50): the refusal 0x12cee2, none laid 0x12ceee, some laid
/// 0x12cf00, each with the test's own marker (short enough for the room to the next).
const SERPENT_LINES: [(u32, &str); 3] = [
    (0x0012_cee2, "Zs0"),
    (0x0012_ceee, "Zs1"),
    (0x0012_cf00, "Zs2"),
];

/// The party at `at` on open ground (`flags1` 0) or walls (bit 1), with 0x1319b2 kept from
/// hatching (A4-0x7afb set, its first test): the eggs stay as 0x12ff50 lays them.
fn serpent_field(flags1: u8, at: (u16, u16)) -> State {
    let mut s = world();
    s.ui = crate::main_loop::test_support::test_ui();
    (s.map.centre.x, s.map.centre.y) = at;
    (s.objects.location[1].x, s.objects.location[1].y) = at;
    link(&mut s);
    tiles(&mut s, 0, flags1);
    s.map.flag_7afb = 1;
    s
}

/// Use on the serpent figure: the state and the scroll.
fn summon(s: State) -> (State, String) {
    let parts: Vec<(u32, &[u8])> = SERPENT_LINES
        .iter()
        .map(|&(a, m)| (a, m.as_bytes()))
        .collect();
    let e = run_on(s, game_with(&parts), |st, io| async move {
        misc::serpents(Ux::new(&st, &io)).await;
    });
    let s = e.state().clone();
    let t = scroll_text(&s);
    (s, t)
}

/// Which of [`SERPENT_LINES`] the scroll holds, each once at most.
fn serpent_lines(t: &str) -> [bool; 3] {
    SERPENT_LINES.map(|(_, m)| t.contains(m))
}

/// The eggs (0x14f) on the map: (x, y, z, status, qq, its contents' (type, status, qq)), sorted.
type Egg = (u16, u16, u8, u8, u16, Vec<(u16, u8, u16)>);
fn eggs(s: &State) -> Vec<Egg> {
    let mut s = s.clone();
    let mut v = Vec::new();
    for i in 0x100..crate::objects::OBJECT_SLOTS as Slot {
        let o = &s.objects;
        if o.kind(i) != 0x14f || o.place(i) != 0 {
            continue;
        }
        let at = (o.x(i), o.y(i), o.z(i), o.status(i), o.qq_of(i));
        let mut inside = Vec::new();
        let mut c = crate::objects::first_child(&mut s.objects, i);
        while c >= 0 {
            let o = &s.objects;
            inside.push((o.tf(c), o.status(c), o.qq_of(c)));
            c = crate::objects::next_child(&mut s.objects);
        }
        v.push((at.0, at.1, at.2, at.3, at.4, inside));
    }
    v.sort();
    v
}

/// Objects of type 0x19d on the map (the probe 0x12ff50 makes is one).
fn serpents_on_map(s: &State) -> usize {
    let o = &s.objects;
    (0x100..crate::objects::OBJECT_SLOTS as Slot)
        .filter(|&i| o.kind(i) == 0x19d && o.place(i) == 0)
        .count()
}

#[test]
fn the_serpent_figure_refuses_while_a_silver_serpent_holds_a_slot_from_0xe0() {
    // 0x12ff5c..0x12ff90: actor slots 0xe0..0x100, type bits only; no draw, nothing made.
    let mut s = serpent_field(0, (100, 100));
    s.objects.type_frame[0xe0] = 0x19d | 2 << 10;
    let rng = s.rng;
    let (s, t) = summon(s);
    assert_eq!(
        serpent_lines(&t),
        [true, false, false],
        "the refusal: {t:?}"
    );
    assert_eq!(s.rng, rng, "no draw");
    assert!(eggs(&s).is_empty() && serpents_on_map(&s) == 0);
}

#[test]
fn the_serpent_figure_with_no_open_spot_draws_eight_spots_per_egg_and_says_none() {
    // 0x12ffde..0x13012e: three eggs, each 8 tries of a side 0x151f06(0, 3) and an offset
    // 0x151f06(0, 10); the probe 0x19d made at 0x12ffd0 goes at 0x1301b8.
    let s = serpent_field(0x02, (100, 100));
    let mut rng = s.rng;
    for _ in 0..3 * 8 {
        rng.range(0, 3);
        rng.range(0, 10);
    }
    let (s, t) = summon(s);
    assert_eq!(s.rng, rng, "48 draws");
    assert_eq!(serpent_lines(&t), [false, true, false], "none laid: {t:?}");
    assert!(eggs(&s).is_empty());
    assert_eq!(serpents_on_map(&s), 0, "the probe is gone");
}

/// The six cells 0x11b1c0 tests round a spot, in the original's order (0x13007e..0x130126).
const AROUND: [(i16, i16); 6] = [(0, 0), (-1, 0), (0, 1), (-1, 1), (1, 0), (0, -1)];

/// The spot of `side` and offset `d` round `c` (0x130020..0x13007a): (x - 6, y + d - 6),
/// (x + d - 6, y - 7), (x + 7, y + d - 6) or (x + d - 6, y + 6), in words.
fn spot_of(c: Position, side: u16, d: u16) -> (u16, u16) {
    let d = d as i16;
    let (dx, dy) = match side {
        0 => (-6, d - 6),
        1 => (d - 6, -7),
        2 => (7, d - 6),
        _ => (d - 6, 6),
    };
    (c.x.wrapping_add_signed(dx), c.y.wrapping_add_signed(dy))
}

/// What 0x12ff50 lays round `c` on level 0 when only the cells `walls` are closed, drawing from
/// `rng` as the original does: the eggs as [`eggs`] lists them, and the tries each egg took
/// (9: none laid). The egg 0x14f (status 0x20 from 0x10bcee, quantity 100, quality 4: 0x10bc4c
/// at 0x130172) holds a 0x19d in a container (status 8, temporary 0x20) of quality 8 and
/// quantity 0x151f06(4, 8) (0x10be02 at 0x13019a), at the place 0x134e1c packs (a y of -1
/// spills into the level bits) and 0x10bcee keeps (8 bits a coordinate off the surface).
fn laid(rng: &mut crate::rng::Rng, c: Position, walls: &[(u16, u16)]) -> (Vec<Egg>, Vec<u32>) {
    let mut v = Vec::new();
    let mut tries = Vec::new();
    for _ in 0..3 {
        let mut at = None;
        let mut t = 0;
        while at.is_none() && t < 8 {
            t += 1;
            let side = rng.range(0, 3);
            let d = rng.range(0, 10);
            let (x, y) = spot_of(c, side, d);
            let open = AROUND.iter().all(|&(dx, dy)| {
                !walls.contains(&(x.wrapping_add_signed(dx), y.wrapping_add_signed(dy)))
            });
            if open {
                at = Some((x, y));
            }
        }
        tries.push(if at.is_some() { t } else { 9 });
        let Some((x, y)) = at else { continue };
        let p = crate::objects::pack_xyz(x, y, 0);
        let m = if p.z != 0 { 0xff } else { 0x3ff };
        let n = rng.range(4, 8);
        let inside = vec![(0x19d, 0x28, 0x0800 | n)];
        v.push((p.x & m, p.y & m, p.z, 0x20, 0x0464, inside));
    }
    v.sort();
    (v, tries)
}

/// The map cell `at` (in the view window) closed: a tile whose flags1 has bit 1 (impassable).
fn close(s: &mut State, at: (u16, u16)) {
    const WALL: u8 = 1;
    s.tables.tileflags.flags1[usize::from(WALL)] = 0x02;
    let [ox, oy] = s.map.window_origin;
    let cell = crate::map::cell(usize::from(at.0 - ox), usize::from(at.1 - oy));
    s.map.window_tiles[cell] = WALL;
}

#[test]
fn the_serpent_figure_lays_three_eggs_of_a_silver_serpent_on_the_view_s_edges() {
    // Open ground takes each egg's first try. At (6, 6) a spot can lie at -1 (the word 0xffff),
    // its cells wrapping on either side of it.
    for at in [(100, 100), (6, 6)] {
        let s = serpent_field(0, at);
        let mut rng = s.rng;
        let (want, tries) = laid(&mut rng, s.map.centre, &[]);
        assert_eq!(tries, [1, 1, 1]);
        let (s, t) = summon(s);
        assert_eq!(eggs(&s), want, "round {at:?}");
        assert_eq!(s.rng, rng, "two draws per egg and its serpent's quantity");
        assert_eq!(serpent_lines(&t), [false, false, true], "some laid: {t:?}");
        assert_eq!(serpents_on_map(&s), 0, "the probe is gone");
    }
}

#[test]
fn the_serpent_figure_takes_a_spot_only_when_each_of_its_six_cells_is_open() {
    // 0x13007e..0x130126: the probe must be able to stand on (x, y), (x - 1, y), (x, y + 1),
    // (x - 1, y + 1), (x + 1, y) and (x, y - 1). Any one of them closed under the first spot
    // drawn sends the first egg to the second (2 tries of 2 draws, then its serpent's quantity).
    for (k, &(dx, dy)) in AROUND.iter().enumerate() {
        let mut s = serpent_field(0, (100, 100));
        let c = s.map.centre;
        let mut first = s.rng;
        let side = first.range(0, 3);
        let d = first.range(0, 10);
        let (x, y) = spot_of(c, side, d);
        let wall = (x.wrapping_add_signed(dx), y.wrapping_add_signed(dy));
        close(&mut s, wall);
        let mut rng = s.rng;
        let (want, tries) = laid(&mut rng, c, &[wall]);
        assert_eq!(tries[0], 2, "cell {k}: the first egg on the second try");
        let mut five = s.rng;
        for _ in 0..2 {
            five.range(0, 3);
            five.range(0, 10);
        }
        five.range(4, 8);
        let (s, _) = summon(s);
        let e = eggs(&s);
        assert_eq!(e, want, "cell {k} ({dx}, {dy}) closed under the first spot");
        assert!(
            !e.iter().any(|g| (g.0, g.1) == (x, y)),
            "cell {k}: no egg on the first spot"
        );
        assert_eq!(s.rng, rng, "cell {k}: the draws");
        let mut after_first = five;
        for &t in &tries[1..] {
            for _ in 0..t.min(8) {
                after_first.range(0, 3);
                after_first.range(0, 10);
            }
            if t <= 8 {
                after_first.range(4, 8);
            }
        }
        assert_eq!(s.rng, after_first, "cell {k}: 5 draws for the first egg");
    }
}

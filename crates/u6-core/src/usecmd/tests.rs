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
    s.actors.party.names = vec!["Avatar".into()];
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

/// The party aboard a ship (actor 0, 0x19c) at the centre, a skiff (0x19e) in its hold.
/// Returns the state and the skiff's slot.
fn ship_with_skiff(land: bool) -> (State, Slot) {
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
    put(&mut s, A, 0x19e, 100, 100);
    link(&mut s);
    crate::objects::move_into(&mut s, 1, 0, crate::objects::IN_INVENTORY);
    crate::objects::move_into(&mut s, A, 0, crate::objects::IN_INVENTORY);
    (s, A)
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

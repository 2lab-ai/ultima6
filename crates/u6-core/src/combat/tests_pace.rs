//! Every wait the original's drawing time became (spec 03 §1): run through its own call site and
//! the playback at each of [`TICKS`]: 16 ms ticks, 7 ms ticks (no divisor of the costs, so the
//! deadlines chain across ticks), 16 ms ticks with three polls of no time after each, a jittery
//! host (16, 17, 16, 33, 16 ms over and over) and a native-like one (17, 17, 16 ms, one tick of
//! 2 ms early in the run). Each fixture states what it should cost (the constants it charges,
//! counted by the fixture, not read back from the pacing's own tally); the time from the game's
//! first poll is that, to within the run's longest tick, at every tick pattern.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use u6_formats::position::Position;

use super::present::{
    self, ACTOR_FLASH_MS, DISSOLVE_MS, FLASH_MS, FLIGHT_START_MS, FLIGHT_STEP_MS, NAME_MS,
    PRINT_MS, REFRESH_MS, RENDER_MS,
};
use crate::actors::turn::{AFTER_TURN_MS, COMBAT_NPC_MS, PICK_MS};
use crate::engine::{Engine, Io};
use crate::files::Files;
use crate::magic::WINDOW_FLASH_MS;
use crate::state::State;
use crate::ui::effects::SHAKE_MS;

/// A host's ticks: a label, the length (ms) of tick `t` (from 0), and the polls of no time
/// after each.
type Ticks = (&'static str, fn(usize) -> u32, usize);

/// The tick patterns every fixture runs at.
const TICKS: [Ticks; 5] = [
    ("16 ms", |_| 16, 0),
    ("7 ms", |_| 7, 0),
    ("16 ms, 3 zero polls", |_| 16, 3),
    ("16/17/16/33/16 ms", |t| [16, 17, 16, 33, 16][t % 5], 0),
    (
        "17/17/16 ms, one of 2 ms",
        |t| if t == 3 { 2 } else { [17, 17, 16][t % 3] },
        0,
    ),
];

/// Runs `game` at the host ticks `tick` with `zeros` polls of no time after each: the engine
/// time from its first poll to its end, and the longest tick it ran at.
fn timed<G, F>(state: State, tick: fn(usize) -> u32, zeros: usize, game: G) -> (u64, u32)
where
    G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F + 'static,
    F: Future<Output = ()> + 'static,
{
    let done = Rc::new(Cell::new(None));
    let out = Rc::clone(&done);
    let mut e = Engine::with_game(state, Files::default(), move |st, io| async move {
        let t0 = io.now_ms();
        game(Rc::clone(&st), Rc::clone(&io)).await;
        out.set(Some(io.now_ms() - t0));
    });
    let mut longest = 0;
    for t in 0..1_000_000 {
        if done.get().is_some() {
            break;
        }
        let ms = tick(t);
        longest = longest.max(ms);
        e.tick(&[], ms);
        for _ in 0..zeros {
            e.tick(&[], 0);
        }
    }
    (done.get().expect("the game ends"), longest)
}

/// `name`'s waits, made by `game` on a fresh `state()`, take `want` ms (the fixture's own count
/// of the constants it charges) to within the run's longest tick at each of [`TICKS`].
fn same_at_any_tick<G, F>(name: &str, want: u64, state: fn() -> State, game: G)
where
    G: Fn(Rc<RefCell<State>>, Rc<Io>) -> F + Clone + 'static,
    F: Future<Output = ()> + 'static,
{
    let mut seen = Vec::new();
    for (label, tick, zeros) in TICKS {
        let (ms, longest) = timed(state(), tick, zeros, game.clone());
        seen.push((label, ms));
        assert!(
            ms >= want && ms <= want + u64::from(longest),
            "{name}: {ms} ms at ticks of {label}, want {want} + up to {longest}"
        );
    }
    println!("{name}: want {want} ms; (ticks, engine ms) {seen:?}");
}

/// The map window shown, centre (20, 20), actor 2 next to it (its type has a base tile), the
/// party's leader.
fn window_state() -> State {
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    s.gfx.window_shown = 1;
    s.map.centre = Position { x: 20, y: 20, z: 0 };
    s.objects.type_frame[2] = 0x17a;
    s.objects.location[2] = Position { x: 21, y: 20, z: 0 };
    s.actors.party.members[0] = 2;
    s.actors.party.size = 1;
    s
}

#[test]
fn a_hit_flash_waits_its_time_at_any_tick() {
    let want = u64::from(FLASH_MS);
    same_at_any_tick("hit flash", want, window_state, |st, io| async move {
        super::damage::flash(&mut st.borrow_mut(), 2);
        super::play_effects(&st, &io).await;
    });
}

#[test]
fn the_window_flash_waits_its_time_at_any_tick() {
    // Its render and a pass before the XORed window shows, a pass before it is restored.
    let want = u64::from(RENDER_MS) + 2 * u64::from(WINDOW_FLASH_MS);
    same_at_any_tick("window flash", want, window_state, |st, io| async move {
        crate::magic::flash_window(&st, &io, 1).await;
    });
}

#[test]
fn the_actor_flash_dissolves_in_its_time_at_any_tick() {
    // Two dissolving windows: the outlined actor, then the window without it.
    let want = 2 * u64::from(ACTOR_FLASH_MS);
    same_at_any_tick("actor flash", want, window_state, |st, io| async move {
        crate::gfx::view::flash_actor(&mut st.borrow_mut(), &io, 2, true);
        super::play_effects(&st, &io).await;
    });
}

#[test]
fn a_dissolve_takes_its_time_at_any_tick() {
    let want = u64::from(DISSOLVE_MS);
    same_at_any_tick("dissolve", want, window_state, |st, io| async move {
        present::capture_dissolve(&mut st.borrow_mut(), DISSOLVE_MS);
        super::play_effects(&st, &io).await;
    });
}

#[test]
fn a_quake_shakes_in_its_time_at_any_tick() {
    // Three shakes, then the refreshed window.
    let want = 3 * u64::from(SHAKE_MS) + u64::from(REFRESH_MS);
    same_at_any_tick("quake", want, window_state, |st, io| async move {
        crate::ui::effects::quake(&mut st.borrow_mut(), &io, 3, 4);
        super::play_effects(&st, &io).await;
    });
}

/// `window_state` with one missile kind that draws every 4th round.
fn flight_state() -> State {
    let mut s = window_state();
    let t = &mut s.combat.tables;
    t.missile_tiles = vec![0x22c];
    t.missile_step = vec![4];
    t.missile_cx = vec![8];
    t.missile_cy = vec![8];
    t.missile_angle = vec![0];
    t.missile_spin = vec![0];
    t.distance = (0..64).map(|i| ((i / 8).max(i % 8)) as u8).collect();
    s
}

#[test]
fn a_flight_takes_its_time_at_any_tick() {
    // Three tiles east: 48 rounds of a pixel, drawn every 4th (12 steps, all in the window),
    // after the first render, then the closing refresh.
    let want = u64::from(FLIGHT_START_MS) + 12 * u64::from(FLIGHT_STEP_MS) + u64::from(REFRESH_MS);
    same_at_any_tick("flight", want, flight_state, |st, io| async move {
        {
            let mut s = st.borrow_mut();
            let (mut xs, mut ys) = ([23i16], [20i16]);
            present::fly(&mut s, 20, 20, &mut xs, &mut ys, 1, 7, 0x22c, 0);
        }
        super::play_effects(&st, &io).await;
    });
}

#[test]
fn messages_take_their_time_at_any_tick() {
    let want = 3 * u64::from(PRINT_MS);
    same_at_any_tick("messages", want, window_state, |st, io| async move {
        {
            let mut s = st.borrow_mut();
            for line in [&b"one\n"[..], b"two\n", b"three\n"] {
                crate::main_loop::queue_print(&mut s, line.to_vec(), Vec::new());
            }
        }
        super::play_effects(&st, &io).await;
    });
}

#[test]
fn name_reads_take_their_time_at_any_tick() {
    let want = 2 * u64::from(NAME_MS);
    same_at_any_tick("name reads", want, window_state, |st, io| async move {
        {
            let mut s = st.borrow_mut();
            let _ = present::name(&mut s, 2);
            let _ = present::name(&mut s, 2);
        }
        super::play_effects(&st, &io).await;
    });
}

#[test]
fn x_ray_refreshes_take_their_time_at_any_tick() {
    // The window flash (circle 2), the caster's flash (two dissolving windows), the 100 X-ray
    // refreshes and the normal view's, each after a refresh's time.
    let want = u64::from(RENDER_MS)
        + 2 * u64::from(WINDOW_FLASH_MS)
        + 2 * u64::from(ACTOR_FLASH_MS)
        + 101 * u64::from(REFRESH_MS);
    same_at_any_tick("X-ray", want, window_state, |st, io| async move {
        crate::magic::xray(&st, &io, 0x12).await;
    });
}

/// The player (actor 1, no points) and two NPCs in combat worktype 3 near it, in a time stop:
/// each NPC loses its turn at once (0x1200b0), so the turn is two NPC picks and the player's.
fn combat_npcs() -> State {
    let mut s = window_state();
    let at = Position { x: 20, y: 20, z: 0 };
    s.objects.location[1] = at;
    s.objects.type_frame[1] = 0x17a;
    s.actors.party.members[0] = 1;
    s.actors.set_tail(crate::actors::tail::SOLO_MEMBER, 0xff);
    s.actors.worktype[1] = 2;
    s.actors.attrs.status[1] = 0x80;
    s.actors.attrs.dexterity[1] = 10;
    s.actors.movement_points[1] = 0;
    for (i, a) in [10usize, 11].into_iter().enumerate() {
        s.objects.location[a] = Position {
            x: 21 + i as u16,
            y: 21,
            z: 0,
        };
        s.objects.type_frame[a] = 0x17a;
        s.actors.worktype[a] = 3;
        s.actors.attrs.dexterity[a] = 10;
        s.actors.movement_points[a] = 6;
    }
    s.actors.timers[crate::actors::timer::TIME_STOP] = 100;
    s.map.view_dirty = 1;
    s
}

/// [`combat_npcs`] without the time stop, the NPCs in worktype 4 (rear) with 5 points each: on
/// the party's level, with no hostiles near (formation 0x120b64 counts none), a rear fighter
/// outside the party only spends 5 points (0x1259b0), so each takes one turn that runs its
/// worktype and draws nothing.
fn running_combat_npcs() -> State {
    let mut s = combat_npcs();
    s.actors.timers[crate::actors::timer::TIME_STOP] = 0;
    for a in [10usize, 11] {
        s.actors.worktype[a] = 4;
        s.actors.movement_points[a] = 5;
    }
    s
}

#[test]
fn combat_npc_turns_that_run_take_their_turn_time_at_any_tick() {
    // Three picks (the two NPCs, then the player after the refill), a combat NPC turn for each
    // NPC, and the one refresh of the view the turn started with changed.
    let want = 3 * u64::from(PICK_MS) + 2 * u64::from(COMBAT_NPC_MS) + u64::from(AFTER_TURN_MS);
    same_at_any_tick(
        "running combat NPC turns",
        want,
        running_combat_npcs,
        |st, io| async move {
            crate::actors::turn::world_turn(&st, &io).await;
            let s = st.borrow();
            assert_eq!(s.actors.worktype[10..12], [4, 4], "still rear fighters");
            assert_eq!(
                [s.actors.mp(10), s.actors.mp(11)],
                [10, 10],
                "one turn each, refilled"
            );
        },
    );
}

#[test]
fn combat_npc_turns_lost_to_a_time_stop_take_no_turn_time_at_any_tick() {
    // Three picks (the two NPCs, then the player) and the one refresh of the view the turn
    // started with changed. The NPC turns end at once (0x1200e8..0x120112: no worktype runs),
    // so no combat NPC turn time.
    let want = 3 * u64::from(PICK_MS) + u64::from(AFTER_TURN_MS);
    same_at_any_tick(
        "time-stopped NPC turns",
        want,
        combat_npcs,
        |st, io| async move {
            crate::actors::turn::world_turn(&st, &io).await;
        },
    );
}

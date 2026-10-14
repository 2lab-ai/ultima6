//! Every wait the original's drawing time became (spec 03 §1): run through its own call site and
//! the playback, at 16 ms ticks, at 7 ms ticks (no divisor of the costs, so the deadlines chain
//! across ticks) and at 16 ms ticks with three polls of no time after each. The time from the
//! game's first poll is the waits' requested total, to within a tick, and the same at each.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use u6_formats::position::Position;

use super::present;
use crate::engine::{Engine, Io};
use crate::files::Files;
use crate::state::State;

/// Runs `game` at ticks of `tick_ms` with `zeros` polls of no time after each: the engine time
/// from its first poll to its end, and the total of the waits it asked for.
fn timed<G, F>(state: State, tick_ms: u32, zeros: usize, game: G) -> (u64, u64)
where
    G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F + 'static,
    F: Future<Output = ()> + 'static,
{
    let done = Rc::new(Cell::new(None));
    let out = Rc::clone(&done);
    let mut e = Engine::with_game(state, Files::default(), move |st, io| async move {
        let t0 = io.now_ms();
        game(Rc::clone(&st), Rc::clone(&io)).await;
        let asked: u64 = st
            .borrow()
            .combat
            .paced
            .iter()
            .map(|(&ms, &n)| u64::from(ms) * u64::from(n))
            .sum();
        out.set(Some((io.now_ms() - t0, asked)));
    });
    for _ in 0..100_000 {
        if done.get().is_some() {
            break;
        }
        e.tick(&[], tick_ms);
        for _ in 0..zeros {
            e.tick(&[], 0);
        }
    }
    done.get().expect("the game ends")
}

/// `name`'s waits, made by `game` on a fresh `state()`, at the three tick patterns.
fn same_at_any_tick<G, F>(name: &str, state: fn() -> State, game: G)
where
    G: Fn(Rc<RefCell<State>>, Rc<Io>) -> F + Clone + 'static,
    F: Future<Output = ()> + 'static,
{
    let runs: Vec<(u32, usize, u64, u64)> = [(16, 0), (7, 0), (16, 3)]
        .into_iter()
        .map(|(tick, zeros)| {
            let (ms, asked) = timed(state(), tick, zeros, game.clone());
            (tick, zeros, ms, asked)
        })
        .collect();
    println!("{name}: (tick ms, zero polls, engine ms, ms asked) {runs:?}");
    let asked = runs[0].3;
    assert!(asked > 0, "{name}: no wait asked for");
    for &(tick, zeros, ms, a) in &runs {
        assert_eq!(a, asked, "{name}: the same waits at {tick} ms ticks");
        assert!(
            ms >= asked && ms <= asked + 2 * 16,
            "{name}: {ms} ms at {tick} ms ticks ({zeros} zero polls), {asked} asked"
        );
        assert!(ms.abs_diff(runs[0].2) <= 16, "{name}: {runs:?}");
    }
}

/// The map window shown, centre (20, 20), actor 2 next to it (its type has a base tile).
fn window_state() -> State {
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    s.gfx.window_shown = 1;
    s.map.centre = Position { x: 20, y: 20, z: 0 };
    s.objects.type_frame[2] = 0x17a;
    s.objects.location[2] = Position { x: 21, y: 20, z: 0 };
    s
}

#[test]
fn a_hit_flash_waits_its_time_at_any_tick() {
    same_at_any_tick("hit flash", window_state, |st, io| async move {
        super::damage::flash(&mut st.borrow_mut(), 2);
        super::play_effects(&st, &io).await;
    });
}

#[test]
fn the_window_flash_waits_its_time_at_any_tick() {
    same_at_any_tick("window flash", window_state, |st, io| async move {
        crate::magic::flash_window(&st, &io, 1).await;
    });
}

#[test]
fn the_actor_flash_dissolves_in_its_time_at_any_tick() {
    same_at_any_tick("actor flash", window_state, |st, io| async move {
        crate::gfx::view::flash_actor(&mut st.borrow_mut(), &io, 2, true);
        super::play_effects(&st, &io).await;
    });
}

#[test]
fn a_dissolve_takes_its_time_at_any_tick() {
    same_at_any_tick("dissolve", window_state, |st, io| async move {
        present::capture_dissolve(&mut st.borrow_mut(), present::DISSOLVE_MS);
        super::play_effects(&st, &io).await;
    });
}

#[test]
fn a_quake_shakes_in_its_time_at_any_tick() {
    same_at_any_tick("quake", window_state, |st, io| async move {
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
    same_at_any_tick("flight", flight_state, |st, io| async move {
        {
            let mut s = st.borrow_mut();
            let (mut xs, mut ys) = ([23i16], [20i16]);
            present::fly(&mut s, 20, 20, &mut xs, &mut ys, 1, 7, 0x22c, 0);
        }
        super::play_effects(&st, &io).await;
    });
    // The steps drawn are waits of their own.
    let (_, asked) = timed(flight_state(), 16, 0, |st, io| async move {
        {
            let mut s = st.borrow_mut();
            let (mut xs, mut ys) = ([23i16], [20i16]);
            present::fly(&mut s, 20, 20, &mut xs, &mut ys, 1, 7, 0x22c, 0);
        }
        super::play_effects(&st, &io).await;
    });
    let least = u64::from(present::FLIGHT_START_MS + present::FLIGHT_STEP_MS);
    assert!(
        asked >= least,
        "the start and at least one step: {asked} ms"
    );
}

#[test]
fn messages_take_their_time_at_any_tick() {
    same_at_any_tick("messages", window_state, |st, io| async move {
        {
            let mut s = st.borrow_mut();
            for line in [&b"one\n"[..], b"two\n", b"three\n"] {
                crate::main_loop::queue_print(&mut s, line.to_vec(), Vec::new());
            }
        }
        super::play_effects(&st, &io).await;
    });
}

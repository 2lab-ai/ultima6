//! Spec 19 O1b: a key held at a key wait gives one command a poll and none once let go, a tap
//! during a turn runs once, the line editor keeps every key.
//!
//! Real data: the castle start state as the T1 probe ran it (its `held` and `tap` rows): `game`
//! at once with seed 0, the actors in the assault worktype (8) passed over (status bit 0x10)
//! and the fight broken off with `b` (tools/oracle/game.mjs townSetup), fed at the hosts' step
//! ([`STEP_MS`]) as a platform sends a key: the press, its auto-repeat at the owner's macOS rate
//! (`InitialKeyRepeat` 15, `KeyRepeat` 2: the first repeat 225 ms after the press, then one every
//! 30 ms) and the release, each in the tick it falls in, and the press the platform paces at
//! the command prompt (spec 19 O1c, [`Engine::held_press`], which takes the auto-repeat's
//! presses back there; 1×, so engine ms are wall ms). Against it the original's model, the
//! probe's "latest-only" row: one press put in at each prompt the key is down at, so its steps
//! are the prompts reached while the key is down. Skips without the zip; prints step counts and
//! engine ms only. The other tests read a drained key wait and the line editor on the test
//! tables.

use super::real_tests::files;
use super::*;
use crate::engine::KEY_RING;
use crate::main_loop::Program;
use crate::pacer::STEP_MS;
use u6_testkit::Quiet;

/// Engine ms from a press to its first auto-repeat (`InitialKeyRepeat` 15 × 15 ms).
const FIRST_REPEAT_MS: u64 = 225;
/// Engine ms between two auto-repeats (`KeyRepeat` 2 × 15 ms).
const REPEAT_MS: u64 = 30;

/// The command loop waits for a command key and nothing is queued.
fn at_prompt(e: &Engine) -> bool {
    e.io().pending_events() == 0 && e.state().main.reading == 1
}

fn leader(e: &Engine) -> (u16, u16) {
    let p = e.state().leader_position().quiet();
    (p.x, p.y)
}

/// Idle ticks until the prompt is up.
fn settle(e: &mut Engine) {
    for _ in 0..20_000 {
        if at_prompt(e) && e.ticks() > 10 {
            return;
        }
        e.tick(&[], STEP_MS);
    }
    panic!("the game reaches the prompt");
}

/// The castle start state (module doc), at the prompt.
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
    // 480 ms for the break-off.
    for _ in 0..480 / STEP_MS {
        e.tick(&[], STEP_MS);
    }
    settle(&mut e);
    e
}

/// What a hold did: the leader's steps while the key was down and after its release, and the
/// most events the engine queue held after a tick.
#[derive(Debug)]
struct Hold {
    held: u32,
    after: u32,
    most_queued: usize,
}

/// Holds `key` from the prompt for `ms` of engine time, then idles until the prompt with nothing
/// queued. `platform`: the platform's events (the press at 0, its repeats from
/// [`FIRST_REPEAT_MS`] every [`REPEAT_MS`] while down, the release at `ms`, and while down its
/// paced press, [`crate::engine::paced_press_due`] from the last press the prompt took); else
/// the original's model (a press at each prompt while the key is down). A step counts as held
/// when it shows after a tick that ends no later than `ms`.
fn hold(e: &mut Engine, key: Key, ms: u64, platform: bool) -> Hold {
    let t0 = e.io().now_ms();
    let repeats = (FIRST_REPEAT_MS..ms).step_by(REPEAT_MS as usize);
    let mut due = std::iter::once(0).chain(repeats).peekable();
    let mut released = false;
    let mut last_step: Option<u64> = None;
    let mut at = leader(e);
    let mut h = Hold {
        held: 0,
        after: 0,
        most_queued: 0,
    };
    for _ in 0..100_000 {
        let now = e.io().now_ms() - t0;
        let end = now + u64::from(STEP_MS);
        let mut input = Vec::new();
        if platform {
            while let Some(t) = due.next_if(|&t| t < end) {
                input.push(if t == 0 {
                    InputEvent::Key(key)
                } else {
                    InputEvent::Repeat(key)
                });
            }
            if !released && ms < end {
                input.push(InputEvent::Release(key));
                released = true;
            }
            let since = last_step.map(|t| (now - t) as f64);
            if !released
                && crate::engine::paced_press_due(now as f64, since)
                && let Some(press) = e.held_press(&input, Some(key))
            {
                input.push(press);
            }
        } else if now < ms && at_prompt(e) {
            input.push(InputEvent::Key(key));
        }
        let takes = e.prompt_takes();
        e.tick(&input, STEP_MS);
        if e.prompt_takes() != takes {
            last_step = Some(now);
        }
        h.most_queued = h.most_queued.max(e.io().pending_events());
        let p = leader(e);
        let then = e.io().now_ms() - t0;
        if p != at {
            at = p;
            if then <= ms {
                h.held += 1;
            } else {
                h.after += 1;
            }
        }
        if then >= ms && at_prompt(e) {
            return h;
        }
    }
    panic!("the prompt after a hold of {ms} ms");
}

/// The probe's held rows (south, the way open for about 10 cells) for a hold of `ms`: the
/// original's model, then the platform's events. A hold that starts at the prompt has its press
/// taken there, so all that waits at the release is repeats, and the release takes them back:
/// no step after it (with the drain alone the newest repeat would give one). At the prompt the
/// repeats are taken back and the paced press steps (the castle's turns outlast
/// [`crate::engine::MIN_STEP_MS`] at 1×, so once a prompt, as the model).
fn held_south(files: &Files, ms: u64) -> (Hold, Hold) {
    let model = hold(&mut castle(files), Key::Down, ms, false);
    let platform = hold(&mut castle(files), Key::Down, ms, true);
    println!("held {ms} ms south: model {model:?}, platform {platform:?}");
    assert!(
        model.held >= 2 && model.after == 0,
        "the model: one step a prompt while down, none after"
    );
    (model, platform)
}

#[test]
fn a_key_held_1_s_steps_once_a_prompt_and_not_after_its_release() {
    let Some(files) = files() else { return };
    let (model, platform) = held_south(&files, 1000);
    assert!(
        platform.held.abs_diff(model.held) <= 1,
        "steps while held {} against the prompts reached while down {}",
        platform.held,
        model.held
    );
    assert_eq!(platform.after, 0, "steps after the release");
}

#[test]
fn a_key_held_3_s_does_not_fill_the_ring() {
    let Some(files) = files() else { return };
    let (model, platform) = held_south(&files, 3000);
    assert!(
        platform.most_queued < KEY_RING,
        "{} of the ring's {KEY_RING} keys queued",
        platform.most_queued
    );
    assert!(
        platform.held.abs_diff(model.held) <= 1,
        "steps while held {} against the prompts reached while down {}",
        platform.held,
        model.held
    );
    assert_eq!(platform.after, 0, "steps after the release");
}

#[test]
fn a_tap_during_a_turn_gives_exactly_one_more_step() {
    let Some(files) = files() else { return };
    let mut e = castle(&files);
    // A tap at the prompt (released 60 ms later) and a second one 100 ms into its turn, as the
    // probe's `tap gap 100 ms` row.
    let taps = [
        (0, InputEvent::Key(Key::Down)),
        (60, InputEvent::Release(Key::Down)),
        (100, InputEvent::Key(Key::Down)),
        (160, InputEvent::Release(Key::Down)),
    ];
    let t0 = e.io().now_ms();
    let mut at = leader(&e);
    let mut next = 0;
    let mut steps = Vec::new();
    for _ in 0..100_000 {
        let now = e.io().now_ms() - t0;
        let end = now + u64::from(STEP_MS);
        let mut input = Vec::new();
        while next < taps.len() && taps[next].0 < end {
            input.push(taps[next].1);
            next += 1;
        }
        e.tick(&input, STEP_MS);
        let p = leader(&e);
        if p != at {
            at = p;
            steps.push(e.io().now_ms() - t0);
        }
        if next == taps.len() && at_prompt(&e) {
            break;
        }
    }
    println!("two taps 100 ms apart: steps at {steps:?} ms");
    assert!(at_prompt(&e), "the prompt after the taps");
    assert_eq!(steps.len(), 2, "the tap during the turn runs once");
}

/// A state of the test tables in `game` (where a release takes back repeats, while the key
/// waits drain).
fn game_state() -> State {
    let mut s = state();
    s.main.program = Program::Game;
    s
}

/// The game code key `k` gives on the test tables.
fn code(s: &State, k: Key) -> u16 {
    let mut q = s.ui.input.qualifiers;
    input::key_code(&mut q, &s.ui.tables.keymap, k)
}

/// A key wait with the drain flag set (every wait but the line editor's): 0x105c8e hands
/// A4-0x7a46 to the take 0x134e7c, which empties the ring and keeps the last code. A release
/// takes back only its key's repeats: the press stays, so a tap still counts.
#[test]
fn a_drained_key_wait_takes_the_newest_press_and_a_release_takes_back_its_repeats() {
    let s = game_state();
    assert!(s.ui.input.drain, "the image's word is 1");
    let (a, b, c, d) = (Key::Char('a'), Key::Char('s'), Key::Char('1'), Key::Right);
    let want = |k| code(&s, k);
    let (wa, wb, wc, wd) = (want(a), want(b), want(c), want(d));
    let all = [wa, wb, wc, wd];
    assert!(
        (0..4).all(|i| all[i] != 0 && !all[..i].contains(&all[i])),
        "four keys of the test tables, four codes"
    );
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = Rc::clone(&seen);
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        loop {
            let k = keyread::idle_poll(&st, &io).await;
            if k != 0 {
                out.borrow_mut().push(k);
            }
        }
    });
    let mut take = |input: &[InputEvent]| {
        e.tick(input, 16);
        e.tick(&[], 16);
        assert_eq!(e.io().pending_events(), 0, "the poll emptied the queue");
        std::mem::take(&mut *seen.borrow_mut())
    };
    let key = InputEvent::Key;
    let (rep, up) = (InputEvent::Repeat, InputEvent::Release);
    assert!(take(&[]).is_empty(), "nothing queued");
    assert_eq!(take(&[key(a), key(b), key(c)]), [wc], "the newest press");
    assert_eq!(
        take(&[key(d), rep(d), rep(d), up(d)]),
        [wd],
        "a tap held into its repeats"
    );
    assert_eq!(
        take(&[key(a), rep(a), up(a), key(b), rep(b)]),
        [wb],
        "a released key's repeats gone, the held key's newest"
    );
    assert!(
        take(&[rep(a), up(a)]).is_empty(),
        "a repeat released before the poll"
    );
}

/// The line editor 0x107cba reads with the drain flag clear (0x107cd6 clears it, 0x107d5a sets
/// it back): every key and every repeat is a character, a release takes nothing back.
#[test]
fn typing_a_line_keeps_every_character() {
    let mut e = Engine::with_game(game_state(), Files::default(), |st, io| async move {
        let (_, line) = keyread::read_line(&st, &io, 16).await;
        st.borrow_mut().converse.buffers[0] = line;
    });
    let (a, b, c) = (Key::Char('a'), Key::Char('s'), Key::Char('1'));
    let key = InputEvent::Key;
    let (rep, up) = (InputEvent::Repeat, InputEvent::Release);
    e.tick(&[], 16);
    assert!(!e.state().ui.input.drain, "the line editor reads");
    e.tick(&[key(a), rep(a), rep(a), up(a), key(b), key(c)], 16);
    e.tick(&[rep(c), up(c), up(b), key(a)], 16);
    e.tick(&[up(a), key(Key::Enter)], 16);
    for _ in 0..8 {
        e.tick(&[], 16);
    }
    assert!(e.finished(), "the line ended");
    assert_eq!(e.state().converse.buffers[0], b"aaas11a");
    assert!(e.state().ui.input.drain, "the flag set again");
}

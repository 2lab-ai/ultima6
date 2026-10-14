//! Executor acceptance (contract before the module fan-out): a helper three calls deep prints a
//! prompt, awaits a key that is already queued, sleeps 200 ms and awaits two vblanks. The prompt
//! must be in the published frame while the game is suspended, the game must resume without
//! borrow panics, every presented stage must be published (no skipped frames), and the outcome
//! must not depend on the tick size.
//!
//! Runs natively (`cargo test`) and in node under wasm-bindgen-test-runner:
//! `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner scripts/cargo.sh test
//! -p u6-core --test nested_prompt --target wasm32-unknown-unknown`.

use std::cell::RefCell;

use u6_core::{Engine, Files, InputEvent, Io, Key, State};

const PROMPT: &str = "Talk to whom?";
/// Background index per stage; the prompt also draws a block of index 7 at (8, 8).
const STAGE_PROMPT: u8 = 1;
const STAGE_SLEPT: u8 = 2;
const STAGE_VBLANK1: u8 = 3;
const STAGE_DONE: u8 = 4;

fn show(io: &Io, stage: u8) {
    io.draw(|fb| fb.clear(stage));
    io.present();
}

async fn ask(state: &RefCell<State>, io: &Io) -> Key {
    state.borrow_mut().ui.messages.push(PROMPT.to_string());
    io.draw(|fb| {
        fb.clear(STAGE_PROMPT);
        fb.blit(8, 8, 4, 4, &[7; 16], None);
    });
    io.present();
    let key = io.next_key().await;
    io.sleep(200).await;
    show(io, STAGE_SLEPT);
    io.vblank().await;
    show(io, STAGE_VBLANK1);
    io.vblank().await;
    key
}

async fn depth2(state: &RefCell<State>, io: &Io) -> Key {
    ask(state, io).await
}

async fn depth1(state: &RefCell<State>, io: &Io) -> Key {
    depth2(state, io).await
}

#[derive(Debug, PartialEq)]
struct Outcome {
    /// Stage shown after each tick, consecutive repeats collapsed.
    stages: Vec<u8>,
    /// Ticks the prompt frame stayed published.
    prompt_ticks: u64,
    finished_at: u64,
    messages: Vec<String>,
}

fn run(tick_ms: u32) -> Outcome {
    let mut engine = Engine::with_game(State::empty(), Files::default(), |state, io| async move {
        let key = depth1(&state, &io).await;
        state.borrow_mut().ui.messages.push(format!("{key:?}"));
        show(&io, STAGE_DONE);
    });
    let mut stages = Vec::new();
    let mut prompt_ticks = 0;
    // The key is queued before the game ever asks for it.
    let mut input = vec![InputEvent::Key(Key::Char('y'))];
    while !engine.finished() {
        assert!(engine.ticks() < 1000, "game did not finish");
        engine.tick(&input, tick_ms);
        input.clear();
        let frame = engine.frame();
        let stage = frame.get(0, 0).unwrap();
        if stage == STAGE_PROMPT {
            prompt_ticks += 1;
            assert_eq!(frame.get(9, 9), Some(7), "prompt block published");
            // Suspended inside the helper with the prompt printed and the key consumed.
            let state = engine.state();
            assert_eq!(state.ui.messages, [PROMPT]);
            assert_eq!(engine.io().pending_events(), 0);
        }
        if stages.last() != Some(&stage) {
            stages.push(stage);
        }
    }
    Outcome {
        stages,
        prompt_ticks,
        finished_at: engine.ticks(),
        messages: engine.state().ui.messages.clone(),
    }
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn nested_prompt_is_published_resumes_and_skips_no_frame() {
    let outcomes: Vec<(u32, Outcome)> = [1, 16, 50].into_iter().map(|t| (t, run(t))).collect();
    for (t, o) in &outcomes {
        let sleep_ticks = 200u64.div_ceil(u64::from(*t));
        assert_eq!(
            o.stages,
            [STAGE_PROMPT, STAGE_SLEPT, STAGE_VBLANK1, STAGE_DONE],
            "tick {t} ms"
        );
        assert_eq!(
            o.prompt_ticks, sleep_ticks,
            "tick {t} ms: prompt shown for 200 ms"
        );
        // Prompt ticks, then one tick per vblank-separated stage.
        assert_eq!(o.finished_at, sleep_ticks + 3, "tick {t} ms");
        assert_eq!(o.messages, [PROMPT, "Char('y')"], "tick {t} ms");
    }
    // Same observable run for every tick size except how long the 200 ms took in ticks.
    let shape = |o: &Outcome| (o.stages.clone(), o.messages.clone());
    assert!(
        outcomes
            .windows(2)
            .all(|w| shape(&w[0].1) == shape(&w[1].1))
    );
}

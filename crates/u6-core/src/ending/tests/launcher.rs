//! Public boot → original-format save load → Journey → Use → command/game return → End.
//! The fixture prepares the end condition only. It does not demonstrate quest progression.
//! After boot the test supplies input and observes state; it never calls a handoff directly.

use super::*;

struct Run {
    engine: Engine,
    transitions: Vec<Program>,
}

impl Run {
    fn tick(&mut self, input: &[InputEvent]) {
        self.engine.tick(input, 20);
        let program = self.engine.state().main.program;
        if self.transitions.last() != Some(&program) {
            self.transitions.push(program);
        }
    }

    fn key(&mut self, key: Key, settle: usize) {
        self.tick(&[InputEvent::Key(key)]);
        for _ in 0..settle {
            self.tick(&[]);
        }
    }

    fn at_prompt(&self) -> bool {
        self.engine.state().main.program == Program::Game
            && crate::engine::command_prompt(&self.engine.state())
            && self.engine.io().pending_events() == 0
    }
}

#[test]
fn public_engine_journey_restores_fixture_and_use_runs_ending_to_completion() {
    let Some(mut files) = files() else { return };
    let saved = runtime_save(&files);
    let saved_count = saved.len();
    assert!(saved.contains_key(crate::saveload::OBJLIST_FILE));
    assert!(saved_count > 1, "the fixture includes saved object blocks");
    files.overlay("ending-fixture", saved);
    let mut r = Run {
        engine: Engine::new(files).quiet(),
        transitions: Vec::new(),
    };
    for _ in 0..30 {
        r.tick(&[]);
    }
    assert_eq!(r.transitions, [Program::U], "public boot runs the launcher");
    r.key(Key::Escape, 120);
    r.key(Key::Char('j'), 0);
    for _ in 0..600 {
        if r.at_prompt() || r.engine.finished() {
            break;
        }
        r.tick(&[]);
    }
    assert!(
        r.at_prompt(),
        "Journey restores the save and reaches a command prompt"
    );
    assert_eq!(r.transitions, [Program::U, Program::Game]);
    let date = {
        let s = r.engine.state();
        let position = s.leader_position().quiet();
        assert!(
            (position.x, position.y, position.z) == (0x39b, 0x354, 0),
            "the loader restores the fixture's shrine position"
        );
        assert_same!(
            s.actors.party.names[usize::from(s.actors.party.leader_index)],
            "Tester",
            "the loader restores the fixture's character"
        );
        assert_eq!(s.saveload.generation, 0, "boot has not rewritten the save");
        assert!(
            s.main.ending.is_none(),
            "the loaded save has no pending handoff"
        );
        s.actors.calendar
    };
    let elapsed = Elapsed::from_calendar(date);
    assert!(
        elapsed
            == Elapsed {
                years: 1,
                months: 2,
                days: 3
            },
        "the loader restores the fixture's distinct elapsed units"
    );

    // The same ordinary input as the browser receipt: Use, target the cube to the south,
    // confirm, then dismiss the shrine message. No preselected object survives the save.
    for key in [Key::Char('u'), Key::Down, Key::Enter] {
        r.key(key, 30);
    }
    r.key(Key::Space, 0);
    for _ in 0..600 {
        if r.engine.state().main.program == Program::End || r.engine.finished() {
            break;
        }
        r.tick(&[]);
    }
    assert!(
        r.engine.state().main.ending == Some(elapsed),
        "ordinary Use requests the ending through the restored shrine"
    );
    assert_eq!(
        r.engine.state().main.program,
        Program::End,
        "command_loop must return and game_main must return exit code 2"
    );
    assert!(!r.engine.finished(), "End still waits for its scene input");
    // Ordinary target/key waits animate the live map and legitimately draw actor-idle RNG.
    // Ending purity begins at the observed public transition, after those input waits.
    let ending_rng = r.engine.state().rng;
    for tick in 0..8000 {
        if r.engine.finished() {
            break;
        }
        if tick % 47 == 0 {
            r.tick(&[InputEvent::Key(Key::Space)]);
        } else {
            r.tick(&[]);
        }
        assert_eq!(r.engine.state().main.program, Program::End);
        assert!(
            r.engine.sprite().is_none(),
            "End owns an empty hardware sprite"
        );
    }
    assert!(
        r.engine.finished(),
        "the final scene input finishes the public engine"
    );
    assert_eq!(r.transitions, [Program::U, Program::Game, Program::End]);
    let s = r.engine.state();
    assert!(
        s.actors.calendar == date,
        "Use and End do not spend a game turn"
    );
    assert!(s.rng == ending_rng, "End does not draw game RNG");
    assert_eq!(
        s.saveload.generation, 0,
        "ending does not overwrite the save"
    );
    let stubs = r.engine.stub_hits();
    assert!(
        stubs.keys().all(|id| id.starts_with("u ")),
        "game and ending use no stubbed path"
    );
    eprintln!(
        "ending public flow: save_files={saved_count}, ticks={}, transitions={:?}, finished={}, stub_kinds={}, stub_hits={}",
        r.engine.ticks(),
        r.transitions,
        r.engine.finished(),
        stubs.len(),
        stubs.values().sum::<u64>()
    );
}

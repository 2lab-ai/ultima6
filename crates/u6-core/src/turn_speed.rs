//! Presentation-only turn timing (spec 19 O3/O5). No rule or save reads these controls.
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TurnSpeed {
    #[default]
    Original,
    Quick,
    Fast,
    Instant,
}

impl TurnSpeed {
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Instant,
            2 => Self::Quick,
            4 => Self::Fast,
            _ => Self::Original,
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Self::Original => 1,
            Self::Quick => 2,
            Self::Fast => 4,
            Self::Instant => 0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Quick => "quick",
            Self::Fast => "fast",
            Self::Instant => "instant",
        }
    }

    pub fn millis(self, ms: u16) -> u16 {
        let k = self.code();
        if k == 0 { 0 } else { ms.div_ceil(u16::from(k)) }
    }
}

#[derive(Default)]
pub(crate) struct Playback {
    pub speed: Cell<TurnSpeed>,
    active: Cell<u32>,
    waiting: Cell<u32>,
    skipped: Cell<bool>,
    suppressed: Cell<u8>,
}

impl Playback {
    pub fn begin(&self) -> Turn<'_> {
        if self.active.get() == 0 {
            self.skipped.set(false);
        }
        self.active.set(self.active.get() + 1);
        Turn(self)
    }

    pub fn wait(&self) -> Wait<'_> {
        self.waiting.set(self.waiting.get() + 1);
        Wait(self)
    }

    pub fn skip(&self) -> bool {
        if self.active.get() == 0 || self.waiting.get() == 0 {
            return false;
        }
        self.skipped.set(true);
        true
    }

    /// A consumed physical skip owns its repeats through its release, even after the turn.
    pub fn suppress_until_release(&self, key: crate::Key) {
        self.suppressed.set(self.suppressed.get() | key_bit(key));
    }

    pub fn suppress_repeat(&self, event: crate::InputEvent) -> bool {
        use crate::InputEvent;
        match event {
            InputEvent::Release(key) => {
                self.suppressed.set(self.suppressed.get() & !key_bit(key));
                false
            }
            InputEvent::Repeat(key) => self.suppressed.get() & key_bit(key) != 0,
            _ => false,
        }
    }

    pub fn speed(&self) -> TurnSpeed {
        if self.skipped.get() {
            TurnSpeed::Instant
        } else {
            self.speed.get()
        }
    }
}

fn key_bit(key: crate::Key) -> u8 {
    match key {
        crate::Key::Space | crate::Key::Char(' ') => 1,
        crate::Key::Escape => 2,
        _ => 0,
    }
}

pub(crate) struct Turn<'a>(&'a Playback);
impl Drop for Turn<'_> {
    fn drop(&mut self) {
        self.0.active.set(self.0.active.get() - 1);
        if self.0.active.get() == 0 {
            self.0.skipped.set(false);
        }
    }
}

pub(crate) struct Wait<'a>(&'a Playback);
impl Drop for Wait<'_> {
    fn drop(&mut self) {
        self.0.waiting.set(self.0.waiting.get() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_drawing_wait_in_a_turn_accepts_skip_and_it_ends_with_that_turn() {
        let p = Playback::default();
        assert!(!p.skip());
        let turn = p.begin();
        assert!(!p.skip(), "a dialogue inside a turn keeps its keys");
        let wait = p.wait();
        assert!(p.skip());
        assert_eq!(p.speed(), TurnSpeed::Instant);
        drop(wait);
        assert_eq!(
            p.speed(),
            TurnSpeed::Instant,
            "the remaining waits are skipped"
        );
        drop(turn);
        assert_eq!(p.speed(), TurnSpeed::Original);
        let _next = p.begin();
        assert_eq!(p.speed(), TurnSpeed::Original);
    }
    #[test]
    fn skip_repeats_stay_consumed_after_the_turn_until_release() {
        use crate::{InputEvent, Key};
        let p = Playback::default();
        for key in [Key::Space, Key::Escape] {
            let turn = p.begin();
            let wait = p.wait();
            assert!(p.skip());
            p.suppress_until_release(key);
            assert!(p.suppress_repeat(InputEvent::Repeat(key)));
            drop(wait);
            drop(turn);
            assert!(p.suppress_repeat(InputEvent::Repeat(key)));
            assert!(!p.suppress_repeat(InputEvent::Key(Key::Char('x'))));
            assert!(!p.suppress_repeat(InputEvent::Release(key)));
            assert!(!p.suppress_repeat(InputEvent::Repeat(key)));
        }
    }

    #[test]
    fn ordinary_vblank_and_input_waits_keep_their_time_and_keys_at_every_factor() {
        use crate::{Engine, Files, InputEvent, Key, State};
        for speed in [
            TurnSpeed::Original,
            TurnSpeed::Quick,
            TurnSpeed::Fast,
            TurnSpeed::Instant,
        ] {
            let observed = std::rc::Rc::new(Cell::new(None));
            let out = observed.clone();
            let mut state = State::empty();
            state.main.program = crate::main_loop::Program::Game;
            let mut e = Engine::with_game(state, Files::default(), move |_, io| async move {
                let _turn = io.playback.begin();
                let at = io.now_ms();
                for _ in 0..10 {
                    io.vblank().await;
                }
                let elapsed = io.now_ms() - at;
                let key = io.next_key().await;
                out.set(Some((elapsed, key)));
            });
            e.io().set_turn_speed(speed);
            for tick in 0..20 {
                let input = if tick == 15 {
                    vec![InputEvent::Key(Key::Space)]
                } else {
                    vec![]
                };
                e.tick(&input, 10);
            }
            assert_eq!(observed.get(), Some((100, Key::Space)));
        }
    }

    #[test]
    fn a_skipped_turn_resets_and_the_next_prompt_keeps_space_or_escape() {
        use crate::{Engine, Files, InputEvent, Key, State};
        for key in [Key::Space, Key::Escape] {
            let observed = std::rc::Rc::new(Cell::new(None));
            let out = observed.clone();
            let mut state = State::empty();
            state.main.program = crate::main_loop::Program::Game;
            state.ui.input.accept = true;
            let mut e = Engine::with_game(state, Files::default(), move |st, io| async move {
                {
                    let _turn = io.playback.begin();
                    crate::combat::present::pace(&st, &io, 1000).await;
                }
                st.borrow_mut().ui.input.in_wait = true;
                let command = io.next_key().await;
                st.borrow_mut().ui.input.in_wait = false;
                let _next = io.playback.begin();
                let at = io.now_ms();
                crate::combat::present::pace(&st, &io, 1000).await;
                out.set(Some((command, io.now_ms() - at)));
            });
            for tick in 0..200 {
                let input = match tick {
                    5 => vec![InputEvent::Key(key)],
                    10 => vec![InputEvent::Repeat(key)],
                    15 => vec![InputEvent::Release(key)],
                    20 => vec![InputEvent::Key(key)],
                    _ => vec![],
                };
                e.tick(&input, 10);
                if e.finished() {
                    break;
                }
            }
            assert_eq!(
                observed.get(),
                Some((key, 1000)),
                "next prompt owns its key and the next turn has the original duration"
            );
            assert_eq!(e.io().pending_events(), 0);
        }
    }

    fn waits(speed: Option<TurnSpeed>, skip: bool) -> (u64, crate::State, usize) {
        use crate::{Engine, Files, InputEvent, Key, State};
        let done = std::rc::Rc::new(Cell::new(None));
        let out = done.clone();
        let mut state = State::empty();
        state.main.program = crate::main_loop::Program::Game;
        state.ui.input.accept = true;
        let mut e = Engine::with_game(state, Files::default(), move |st, io| async move {
            let _turn = io.playback.begin();
            let start = io.now_ms();
            for ms in [1000, 600, 400] {
                crate::combat::present::pace(&st, &io, ms).await;
            }
            out.set(Some(io.now_ms() - start));
        });
        if let Some(speed) = speed {
            e.io().set_turn_speed(speed);
        }
        for tick in 0..2000 {
            let input = if skip && tick == 5 {
                vec![InputEvent::Key(Key::Space)]
            } else {
                vec![]
            };
            e.tick(&input, 10);
            if e.finished() {
                break;
            }
        }
        let mut state = e.state().clone();
        state.combat.due_ms = 0;
        state.combat.paced_at_ms = 0;
        (
            done.get().expect("waits finish"),
            state,
            e.io().pending_events(),
        )
    }

    #[test]
    fn turn_speed_scales_only_drawing_waits_and_original_is_identical() {
        let baseline = waits(None, false);
        let original = waits(Some(TurnSpeed::Original), false);
        assert_eq!(
            baseline, original,
            "default and explicit original are identical"
        );
        assert_eq!(baseline.0, 2000);
        for (speed, expected) in [
            (TurnSpeed::Quick, 1000),
            (TurnSpeed::Fast, 500),
            (TurnSpeed::Instant, 30),
        ] {
            let got = waits(Some(speed), false);
            assert_eq!(got.0, expected, "{speed:?}: wait durations");
            assert_eq!(
                got.1, baseline.1,
                "{speed:?}: state and RNG identical apart from timing bookkeeping"
            );
            assert_eq!(got.2, 0);
        }
    }

    #[test]
    fn a_skip_press_ends_current_and_remaining_waits_without_a_queued_command() {
        let baseline = waits(None, false);
        let skipped = waits(None, true);
        assert!(
            skipped.0 < 100,
            "skip ends playback at the next display frames"
        );
        assert_eq!(skipped.1, baseline.1, "skip changes no rule or RNG");
        assert_eq!(skipped.2, 0, "the skip press cannot reach the next command");
    }
}

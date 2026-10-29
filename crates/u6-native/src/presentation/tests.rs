//! Native-window presentation receipts: no window or output of the owner's game data.
#![allow(clippy::disallowed_methods)]

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::{HeldKeys, SCALE, Walk};
use std::time::{Duration, Instant};
use u6_core::{Files, HEIGHT, InputEvent, Key, State, WIDTH};
use u6_core::{pacer::Speed, turn_speed::TurnSpeed};
use u6_intake::{InputFile, intake};
use u6_testkit::Quiet;

fn game() -> Option<(Engine, Rc<RefCell<State>>)> {
    let archive = u6_testkit::archive();
    if !archive.is_file() {
        return None;
    }
    let data = std::fs::read(archive).quiet();
    let files = Files::new(
        intake(vec![InputFile {
            name: "ultima-vi.zip".into(),
            data,
        }])
        .files,
    );
    let mut state = State::new_game(&files).quiet();
    state.rng.seed(0);
    let keep = Rc::new(RefCell::new(None));
    let copy = Rc::clone(&keep);
    let mut engine = Engine::with_game(state, files, move |state, io| {
        *copy.borrow_mut() = Some(Rc::clone(&state));
        u6_core::main_loop::game_first(state, io)
    });
    engine.tick(&[], 0);
    let state = keep.borrow().clone().quiet();
    Some((engine, state))
}

fn prompt(engine: &Engine) -> bool {
    engine.io().pending_events() == 0 && u6_core::engine::command_prompt(&engine.state())
}

/// Removing presentation between engine publications must fail this test. It calls exactly
/// the draw helper used before minifb.update_with_buffer, on a 120 Hz display with 20 ms ticks.
#[test]
fn window_draws_intermediate_pixels_without_a_new_engine_frame() {
    let Some((mut engine, state)) = game() else {
        return;
    };
    let mut presentation = Presentation::new(&mut engine);
    let mut pacer = Pacer::new();
    let mut screen = vec![0; WIDTH * HEIGHT * super::super::SCALE.pow(2)];
    let mut before = screen.clone();
    let mut version = None;
    let mut pending = Vec::new();
    let mut stage = 0;
    let mut unchanged_publication_draws = 0;
    let mut zero_step_draws = 0;
    let mut first_picture = false;
    for _ in 0..2400 {
        if prompt(&engine) {
            match stage {
                0 => {
                    for actor in 0..0x100 {
                        let mut state = state.borrow_mut();
                        if state.actors.worktype[actor] == 8 {
                            state.actors.attrs.status[actor] |= 0x10;
                        }
                    }
                    pending.push(InputEvent::Key(Key::Char('b')));
                }
                1..=6 => pending.push(InputEvent::Key(Key::Up)),
                _ => break,
            }
            stage += 1;
        }
        let steps = crate::pace::run_frame(
            &mut pacer,
            1000.0 / 120.0,
            false,
            || 0.0,
            || {
                engine.tick(&pending, u6_core::pacer::STEP_MS);
                pending.clear();
                Ok::<_, ()>(true)
            },
        )
        .unwrap();
        presentation.draw(&mut engine, &pacer, &mut screen);
        if first_picture && version == Some(engine.frame_version()) && before != screen {
            unchanged_publication_draws += 1;
            zero_step_draws += usize::from(steps == 0);
        }
        first_picture |= steps > 0;
        version = Some(engine.frame_version());
        before.clone_from(&screen);
    }
    assert!(stage > 6, "the real game completed the movement fixture");
    println!(
        "native window helper: intermediate={unchanged_publication_draws}, zero-step={zero_step_draws}"
    );
    assert!(
        unchanged_publication_draws > 0,
        "native drawing has no intermediate pixels between publications"
    );
    assert!(
        zero_step_draws > 0,
        "display frames without logical ticks must still advance the slide"
    );
}

/// Drives the native input, logical-step and drawing helpers on an injected display clock.
/// The control side has no presenter and keeps Engine's default motion-off setting.
struct Window {
    engine: Engine,
    state: Rc<RefCell<State>>,
    presentation: Option<Presentation>,
    pacer: Pacer,
    keys: HeldKeys,
    pending: Vec<InputEvent>,
    screen: Vec<u32>,
    now: Instant,
    elapsed: f64,
    modifier: bool,
    buttons: u8,
    takes: Vec<f64>,
    full_budget: bool,
}

impl Window {
    fn new(on: bool) -> Option<Self> {
        let (mut engine, state) = game()?;
        engine.set_context_menu(true);
        let presentation = on.then(|| {
            let mut p = Presentation::new(&mut engine);
            // Existing interpolation receipts retain their exact original-pixel oracle.
            // The gait receipt below enables the shipping B policy separately.
            p.set_footstep_pattern(0);
            p
        });
        Some(Self {
            engine,
            state,
            presentation,
            pacer: Pacer::new(),
            keys: HeldKeys::default(),
            pending: Vec::new(),
            screen: vec![0; WIDTH * HEIGHT * SCALE.pow(2)],
            now: Instant::now(),
            elapsed: 0.0,
            modifier: false,
            buttons: 0,
            takes: Vec::new(),
            full_budget: false,
        })
    }

    fn display(&mut self, dt: f64) -> u32 {
        self.elapsed += dt;
        self.now += Duration::from_secs_f64(dt / 1000.0);
        self.engine.set_wall_clock(self.elapsed as u64);
        let mut walk = Walk {
            keys: &mut self.keys,
            now: self.now,
            modifier: self.modifier,
        };
        if let Some(p) = &mut self.presentation {
            p.frame(dt);
            p.observe(
                &self.engine,
                walk.keys,
                walk.modifier,
                self.pacer.carried_ms(),
            );
        }
        // Max's time budget can allow one step on a slow machine. Keep that actual native
        // path bounded here; pace.rs separately covers the 64-step ceiling.
        let cost = if self.pacer.speed() == Speed::Max && !self.full_budget {
            dt
        } else {
            0.0
        };
        let count = crate::pace::run_frame(
            &mut self.pacer,
            dt,
            self.buttons != 0,
            || cost,
            || {
                let takes = self.engine.prompt_takes();
                let going = crate::window_step_controls(
                    &mut self.engine,
                    &mut self.pending,
                    &mut Vec::new(),
                    &mut walk,
                    None,
                    None,
                )?;
                for _ in takes..self.engine.prompt_takes() {
                    self.takes.push(self.elapsed);
                }
                if let Some(p) = &mut self.presentation {
                    p.observe(&self.engine, walk.keys, walk.modifier, 0.0);
                }
                Ok::<_, String>(going)
            },
        )
        .quiet();
        self.draw();
        assert!(
            self.engine.take_motion().is_empty(),
            "the display consumes every motion publication"
        );
        count
    }

    fn draw(&mut self) {
        if let Some(p) = &mut self.presentation {
            p.observe(
                &self.engine,
                &self.keys,
                self.modifier,
                self.pacer.carried_ms(),
            );
            p.draw(&mut self.engine, &self.pacer, &mut self.screen);
        } else {
            let mut rgba = Vec::new();
            self.engine.frame().write_rgba(&mut rgba);
            crate::upscale(&rgba, &mut self.screen);
        }
    }

    fn press(&mut self, key: Key) {
        let physical = physical(key);
        self.pending.push(
            self.keys
                .press(physical, key, false, self.modifier, self.now),
        );
    }

    fn release(&mut self, key: Key) {
        self.pending.extend(self.keys.release(physical(key)));
    }

    fn tap(&mut self, key: Key) {
        self.press(key);
        self.release(key);
    }

    fn point(&mut self, x: i16, y: i16, buttons: u8) {
        self.buttons = buttons;
        self.pending.push(crate::pointer(x, y, buttons));
    }

    fn slide(&self) -> Option<&View> {
        self.presentation
            .as_ref()?
            .shown
            .as_ref()?
            .slide
            .as_ref()
            .map(|s| &s.0)
    }

    fn at_prompt(&self) -> bool {
        self.pending.is_empty() && prompt(&self.engine)
    }

    fn ready(&mut self) {
        for _ in 0..2000 {
            self.display(1000.0 / 60.0);
            if self.at_prompt() {
                return;
            }
        }
        panic!("the native fixture did not reach its command prompt");
    }

    fn quiet(&mut self) {
        self.ready();
        suppress_assault(&mut self.state.borrow_mut());
        self.tap(Key::Char('b'));
        self.ready();
    }

    /// Find the ordinary game state in which a map slide is still moving at a command prompt.
    fn moving_prompt(&mut self) {
        self.quiet();
        for key in [Key::Down, Key::Up, Key::Right, Key::Left] {
            self.press(key);
            for _ in 0..900 {
                self.display(1000.0 / 60.0);
                if self.at_prompt() && self.slide().is_some_and(off_grid) {
                    self.release(key);
                    self.draw();
                    return;
                }
            }
            self.release(key);
            self.ready();
        }
        panic!("the fixture must have a slide at the prompt");
    }

    fn assert_authoritative(&self) {
        let mut rgba = Vec::new();
        let mut expected = vec![0; self.screen.len()];
        self.engine.frame().write_rgba(&mut rgba);
        if self.engine.state().main.program == u6_core::main_loop::Program::End {
            crate::upscale_ending(&rgba, &mut expected);
        } else {
            crate::upscale(&rgba, &mut expected);
        }
        u6_testkit::assert_same!(self.screen, expected, "native pixels at rest or bypass");
    }
}

fn physical(key: Key) -> minifb::Key {
    match key {
        Key::Up => minifb::Key::Up,
        Key::Down => minifb::Key::Down,
        Key::Left => minifb::Key::Left,
        Key::Right => minifb::Key::Right,
        Key::Space => minifb::Key::Space,
        Key::Char('b') => minifb::Key::B,
        Key::Char('a') => minifb::Key::A,
        Key::Enter => minifb::Key::Enter,
        _ => panic!("fixture key has no physical mapping"),
    }
}

fn suppress_assault(state: &mut State) {
    for actor in 0..0x100 {
        if state.actors.worktype[actor] == 8 {
            state.actors.attrs.status[actor] |= 0x10;
        }
    }
}

fn off_grid(view: &View) -> bool {
    let subcell = |(x, y): (i32, i32)| x % 16 != 0 || y % 16 != 0;
    subcell(view.camera) || view.movers.iter().any(|m| subcell(m.offset))
}

fn same_game(on: &Window, off: &Window) {
    assert!(
        *on.engine.state() == *off.engine.state(),
        "motion changed whole State, including RNG"
    );
    assert!(
        on.engine.state().rng == off.engine.state().rng,
        "motion changed the RNG"
    );
    assert_eq!(on.engine.ticks(), off.engine.ticks(), "same logical ticks");
    assert_eq!(
        on.engine.prompt_takes(),
        off.engine.prompt_takes(),
        "same accepted input"
    );
    u6_testkit::assert_same!(
        on.engine.published_frame().pixels(),
        off.engine.published_frame().pixels(),
        "published pixels on/off"
    );
}

#[test]
fn tap_held_release_and_modifiers_keep_whole_state_and_rng_unchanged() {
    for (speed, turns, should_slide) in [
        (Speed::Finite(1.0), TurnSpeed::Original, true),
        (Speed::Finite(2.0), TurnSpeed::Quick, true),
        (Speed::Finite(8.0), TurnSpeed::Fast, true),
        (Speed::Max, TurnSpeed::Original, false),
        (Speed::Finite(1.0), TurnSpeed::Instant, false),
    ] {
        let Some(mut on) = Window::new(true) else {
            return;
        };
        let Some(mut off) = Window::new(false) else {
            return;
        };
        for w in [&mut on, &mut off] {
            w.pacer.set_speed(speed);
            w.engine.io().set_turn_speed(turns);
            w.quiet();
        }
        same_game(&on, &off);
        let mut intermediate = 0;
        let mut walking = 0;
        let mut minimum = 0;
        for frame in 0..360 {
            for w in [&mut on, &mut off] {
                match frame {
                    0 => w.tap(Key::Down),
                    60 => w.press(Key::Up),
                    180 => w.modifier = true,
                    192 => w.modifier = false,
                    240 => w.release(Key::Up),
                    _ => {}
                }
                w.display(1000.0 / 60.0);
            }
            same_game(&on, &off);
            let p = on.presentation.as_ref().quiet();
            if (60..240).contains(&frame) && !(180..192).contains(&frame) {
                assert!(
                    p.presenter.walking(),
                    "a physical held key supplies party cadence"
                );
                walking += 1;
                for (_, _, duration, _, _) in p.presenter.timings() {
                    if let Some(rate) = on.pacer.pace_rate() {
                        assert!(
                            duration + 1e-6 >= rate * p.interval * 2.0,
                            "held tracks must last at least two display frames"
                        );
                        minimum += 1;
                    }
                }
            } else {
                assert!(
                    !p.presenter.walking(),
                    "tap, release and modifiers end continuous intent"
                );
            }
            intermediate += usize::from(on.slide().is_some_and(off_grid));
        }
        if should_slide {
            assert!(
                intermediate > 0 && minimum > 0,
                "real held movement must draw subcell frames"
            );
        } else {
            assert_eq!(intermediate, 0, "Max and Instant bypass interpolation");
        }
        on.ready();
        off.ready();
        for _ in 0..120 {
            on.display(1000.0 / 60.0);
            off.display(1000.0 / 60.0);
        }
        same_game(&on, &off);
        assert!(
            on.slide().is_none(),
            "release settles without another input"
        );
        on.assert_authoritative();
        println!(
            "native {speed:?}/{turns:?}: intermediate={intermediate}, walking={walking}, minimum-frames-checked={minimum}; whole State/RNG/published pixels equal"
        );
    }
}

#[test]
fn max_and_instant_end_an_existing_slide_without_waiting_for_a_publication() {
    for instant in [false, true] {
        let Some(mut w) = Window::new(true) else {
            return;
        };
        w.moving_prompt();
        let before = w.engine.frame_version();
        if instant {
            w.engine.io().set_turn_speed(TurnSpeed::Instant);
        } else {
            w.pacer.set_speed(Speed::Max);
            w.pacer.due(0.0, false);
        }
        w.draw();
        assert_eq!(w.engine.frame_version(), before, "no new engine frame");
        assert!(
            w.slide().is_none(),
            "Max/Instant must immediately bypass an already running slide"
        );
        w.assert_authoritative();
    }
}

#[test]
fn a_context_menu_snaps_motion_and_closing_it_does_not_resume_the_old_slide() {
    let Some(mut w) = Window::new(true) else {
        return;
    };
    w.moving_prompt();
    let x = u6_core::geom::MAP_X as i16 + u6_core::geom::HALF_W as i16 * 16 + 8;
    let y = u6_core::geom::MAP_Y as i16 + u6_core::geom::HALF_H as i16 * 16 + 8;
    w.pending.push(crate::pointer(x, y, 2));
    for _ in 0..12 {
        w.display(1000.0 / 60.0);
        if w.engine.context_menu_open() {
            break;
        }
    }
    assert!(
        w.engine.context_menu_open(),
        "the real popup opens on the leader"
    );
    assert!(w.slide().is_none(), "no map repaint can cover the popup");
    w.assert_authoritative();
    w.pending.push(crate::pointer(x, y, 0));
    w.pending.push(InputEvent::Key(Key::Escape));
    for _ in 0..12 {
        w.display(1000.0 / 60.0);
    }
    assert!(!w.engine.context_menu_open(), "the popup closed");
    assert!(
        w.slide().is_none(),
        "closing does not revive a snapped slide"
    );
    w.assert_authoritative();
}

#[test]
fn foreign_publications_and_the_ending_own_the_whole_native_frame() {
    for ending in [false, true] {
        let Some(mut w) = Window::new(true) else {
            return;
        };
        w.moving_prompt();
        if ending {
            w.state.borrow_mut().main.program = u6_core::main_loop::Program::End;
        } else {
            // Hold the command prompt's idle repaint: this fixture publishes a foreign
            // overlay through the real engine queue, rather than having it painted away.
            w.state.borrow_mut().ui.input.idle_refresh = false;
            let (x0, y0, _, _) = u6_core::geom::SHOWN;
            w.engine.io().draw(|fb| {
                let (x, y) = (usize::from(x0) + 8, usize::from(y0) + 8);
                fb.set(x, y, fb.get(x, y).quiet() ^ 1);
            });
            w.engine.io().present();
            w.engine.tick(&[], 0);
        }
        w.draw();
        assert!(
            w.slide().is_none(),
            "foreign/ending frame must end the slide (ending={ending})"
        );
        assert!(
            w.engine.take_motion().is_empty(),
            "foreign publication was consumed"
        );
        w.assert_authoritative();
    }
}

#[test]
fn restore_drops_every_preload_scene_before_motion_resumes() {
    let Some(mut w) = Window::new(true) else {
        return;
    };
    w.moving_prompt();
    let before = w.presentation.as_ref().quiet().presenter.history().to_vec();
    assert!(
        !before.is_empty(),
        "a scene was displayed before the restore"
    );
    u6_core::saveload::restore(&mut w.state.borrow_mut(), w.engine.io()).quiet();
    {
        let mut state = w.state.borrow_mut();
        suppress_assault(&mut state);
        if state.actors.combat_mode() {
            u6_core::combat::mode::break_off(&mut state);
        }
    }
    let mut seen_after = false;
    for key in [Key::Down, Key::Right, Key::Up, Key::Left] {
        w.tap(key);
        for _ in 0..1200 {
            w.display(1000.0 / 60.0);
            let p = &w.presentation.as_ref().quiet().presenter;
            let new = p
                .history()
                .first()
                .is_some_and(|a| !before.iter().any(|b| Rc::ptr_eq(a, b)));
            seen_after |= new;
            if seen_after {
                assert!(
                    p.history()
                        .iter()
                        .all(|a| !before.iter().any(|b| Rc::ptr_eq(a, b))),
                    "a pre-restore scene must never feed the new slide"
                );
            }
            if w.at_prompt() {
                break;
            }
        }
        if seen_after {
            break;
        }
    }
    assert!(seen_after, "the restored game published a new scene");
}

#[test]
fn the_pointer_sprite_stays_above_the_intermediate_map_pixels() {
    let Some(mut w) = Window::new(true) else {
        return;
    };
    w.quiet();
    let x = u6_core::geom::MAP_X as i16 + u6_core::geom::HALF_W as i16 * 16 + 8;
    let y = u6_core::geom::MAP_Y as i16 + u6_core::geom::HALF_H as i16 * 16 + 8;
    w.pending.push(crate::pointer(x, y, 0));
    w.ready();
    let mut checked = 0;
    for key in [Key::Down, Key::Up, Key::Right, Key::Left] {
        w.tap(key);
        for _ in 0..1200 {
            w.display(1000.0 / 60.0);
            if w.slide().is_some_and(off_grid) {
                let sprite = w.engine.sprite().quiet();
                let p = w.presentation.as_ref().quiet();
                for (row, values) in sprite.rows.iter().enumerate() {
                    for (col, &value) in values.iter().enumerate() {
                        let (x, y) = (
                            i32::from(sprite.x) + col as i32,
                            i32::from(sprite.y) + row as i32,
                        );
                        if value != 0
                            && (0..WIDTH as i32).contains(&x)
                            && (0..HEIGHT as i32).contains(&y)
                        {
                            assert!(
                                p.frame.get(x as usize, y as usize) == Some(16 + value),
                                "sprite pixel was covered by the moving map"
                            );
                            checked += 1;
                        }
                    }
                }
            }
            if w.at_prompt() {
                break;
            }
        }
    }
    assert!(
        checked > 0,
        "opaque pointer pixels were checked over a subcell map"
    );
    println!("native sprite pixels verified over moving map: {checked}");
}

#[test]
fn release_keeps_the_remaining_finite_slide_when_a_new_tap_would_be_too_short() {
    let Some(mut w) = Window::new(true) else {
        return;
    };
    w.quiet();
    w.pacer.set_speed(Speed::Finite(8.0));
    w.engine.io().set_turn_speed(TurnSpeed::Fast);
    w.press(Key::Down);
    for _ in 0..360 {
        w.display(1000.0 / 60.0);
        if w.slide().is_some_and(off_grid) {
            break;
        }
    }
    let before = w
        .slide()
        .cloned()
        .expect("a measured held stride must animate at 8x/Fast");
    w.release(Key::Down);
    w.draw();
    assert!(
        w.slide() == Some(&before),
        "release at the same presentation time must keep the drawn position and settle its remaining distance"
    );
}

#[test]
fn pointer_and_go_here_use_the_native_presenter_without_changing_game_state() {
    for travel in [false, true] {
        let Some(mut on) = Window::new(true) else {
            return;
        };
        let Some(mut off) = Window::new(false) else {
            return;
        };
        for w in [&mut on, &mut off] {
            w.quiet();
        }
        let x = u6_core::geom::MAP_X as i16 + u6_core::geom::HALF_W as i16 * 16 + 8;
        let y = u6_core::geom::MAP_Y as i16 + u6_core::geom::HALF_H as i16 * 16 + 40;
        if travel {
            let centre = on.state.borrow().map.centre;
            let mut accepted = None;
            for (dx, dy) in [(0i16, 3i16), (3, 0), (-3, 0), (0, -3)] {
                let target = (
                    centre.x.wrapping_add_signed(dx),
                    centre.y.wrapping_add_signed(dy),
                );
                if on.engine.travel_to(target).is_ok() {
                    accepted = Some(target);
                    break;
                }
            }
            off.engine
                .travel_to(accepted.expect("a local travel is accepted"))
                .quiet();
        } else {
            on.point(x, y, 1);
            off.point(x, y, 1);
        }
        let mut walking = 0;
        let mut intermediate = 0;
        for frame in 0..420 {
            if frame == 240 && !travel {
                on.point(x, y, 0);
                off.point(x, y, 0);
            }
            on.display(1000.0 / 60.0);
            off.display(1000.0 / 60.0);
            same_game(&on, &off);
            walking += usize::from(on.presentation.as_ref().quiet().presenter.walking());
            intermediate += usize::from(on.slide().is_some_and(off_grid));
            if travel && frame > 240 && !on.engine.travel_active() {
                break;
            }
        }
        assert!(
            walking > 0,
            "pointer/GoHere must feed continuous cadence to the native presenter (travel={travel})"
        );
        assert!(
            intermediate > 0,
            "the accepted walk draws subcell positions"
        );
        for _ in 0..120 {
            on.display(1000.0 / 60.0);
            off.display(1000.0 / 60.0);
        }
        same_game(&on, &off);
        assert!(
            !on.presentation.as_ref().quiet().presenter.walking(),
            "release/arrival ends continuous intent"
        );
        println!(
            "native travel={travel}: walking={walking}, intermediate={intermediate}; whole State/RNG/published pixels equal"
        );
    }
}

#[test]
fn native_travel_waits_150_wall_ms_at_original_4x_and_max_even_with_instant_turns() {
    use u6_core::actors::travel::{End, reachable};
    for speed in [Speed::Finite(1.0), Speed::Finite(4.0), Speed::Max] {
        let Some(mut w) = Window::new(true) else {
            return;
        };
        w.engine.io().set_turn_speed(TurnSpeed::Instant);
        w.quiet();
        w.pacer.set_speed(speed);
        w.full_budget = true;
        let centre = w.state.borrow().map.centre;
        let start = (centre.x, centre.y);
        let target = [(3i16, 0i16), (-3, 0), (0, 3), (0, -3), (2, 2), (-2, -2)]
            .into_iter()
            .map(|(dx, dy)| {
                (
                    centre.x.wrapping_add_signed(dx),
                    centre.y.wrapping_add_signed(dy),
                )
            })
            .find(|&target| reachable(&w.engine.state(), target).is_none())
            .expect("a local destination at least three steps away");
        let at = w.takes.len();
        w.engine.travel_to(target).quiet();
        for _ in 0..2000 {
            w.display(16.0);
            if !w.engine.travel_active() {
                break;
            }
        }
        assert!(
            w.engine.travel_end() == Some(End::Arrived),
            "native travel arrives"
        );
        let times = w.takes[at..].to_vec();
        assert!(
            times.len() >= 3,
            "at least three actual accepted travel steps"
        );
        let minimum = times
            .windows(2)
            .map(|p| p[1] - p[0])
            .fold(f64::INFINITY, f64::min);
        println!(
            "native travel {speed:?}/Instant: steps={}, minimum wall gap={minimum}",
            times.len()
        );
        assert!(
            minimum >= 150.0,
            "native travel exceeded the shared 150 ms wall-clock cadence"
        );

        w.ready();
        w.tap(Key::Space);
        let before = w.takes.len();
        for _ in 0..300 {
            w.display(16.0);
            if w.takes.len() > before {
                break;
            }
        }
        assert!(
            w.takes.len() > before,
            "the ordinary Pass was actually consumed before travel re-entry"
        );
        let pass = *w.takes.last().quiet();
        w.engine.travel_to(start).quiet();
        let before = w.takes.len();
        for _ in 0..300 {
            w.display(16.0);
            if w.takes.len() > before {
                break;
            }
        }
        assert!(
            w.takes.len() > before,
            "re-entered travel took its first step"
        );
        assert!(
            w.takes[before] - pass >= 150.0,
            "travel re-entry shares the last command's gate"
        );
        // A stop is processed before the cadence gate even during its 150 ms cooldown.
        w.pending.push(InputEvent::Key(Key::Space));
        for _ in 0..4 {
            w.display(16.0);
            if !w.engine.travel_active() {
                break;
            }
        }
        assert!(
            w.engine.travel_end() == Some(End::Input),
            "input stops travel during cooldown"
        );
    }
}

#[test]
fn native_taps_use_360_over_the_o3_factor_on_the_presentation_clock() {
    for (turns, milliseconds) in [
        (TurnSpeed::Original, 360.0),
        (TurnSpeed::Quick, 180.0),
        (TurnSpeed::Fast, 90.0),
    ] {
        let Some(mut w) = Window::new(true) else {
            return;
        };
        w.quiet();
        w.pacer.set_speed(Speed::Finite(2.0));
        w.engine.io().set_turn_speed(turns);
        let mut found = false;
        for key in [Key::Down, Key::Up, Key::Right, Key::Left] {
            let since = w.engine.io().now_ms() as f64;
            w.tap(key);
            for _ in 0..1200 {
                w.display(1000.0 / 60.0);
                if let Some((_, _, duration, _, _)) = w
                    .presentation
                    .as_ref()
                    .quiet()
                    .presenter
                    .timings()
                    .into_iter()
                    .find(|&(slot, start, _, _, _)| slot == -1 && start > since)
                {
                    assert_eq!(
                        duration, milliseconds,
                        "tap duration is 360/O3 on engine time; 2x pacing must not divide it again"
                    );
                    found = true;
                    break;
                }
                if w.at_prompt() {
                    break;
                }
            }
            if found {
                break;
            }
        }
        assert!(found, "a real tap produced a camera track for {turns:?}");
        println!("native tap {turns:?} at 2x: duration={milliseconds} engine ms");
    }
}

#[test]
fn a_publication_after_release_keeps_finite_settling_continuous() {
    let Some(mut w) = Window::new(true) else {
        return;
    };
    w.quiet();
    w.pacer.set_speed(Speed::Finite(4.0));
    w.engine.io().set_turn_speed(TurnSpeed::Fast);
    w.press(Key::Down);
    for _ in 0..360 {
        w.display(1000.0 / 60.0);
        if w.slide()
            .is_some_and(|v| off_grid(v) && v.camera.0.abs().max(v.camera.1.abs()) >= 13)
        {
            break;
        }
    }
    let view = w
        .slide()
        .cloned()
        .expect("a measured finite stride with remaining distance");
    assert!(
        view.camera.0.abs().max(view.camera.1.abs()) >= 13,
        "the slide has more distance than one settle frame"
    );
    w.release(Key::Down);
    w.draw();
    let previous_scene = Rc::clone(&w.presentation.as_ref().quiet().presenter.history()[0]);
    // Publish a changed map word at unchanged positions through the real renderer and
    // Engine::take_motion queue. Use a copy, as an animation/early picture does, so this
    // controlled publication cannot move an actor or advance the authoritative RNG.
    w.state.borrow_mut().ui.input.idle_refresh = false;
    let mut picture = w.state.borrow().clone();
    let cell = u6_core::geom::HALF_H * u6_core::geom::VIEW_W + u6_core::geom::HALF_W + 2;
    picture.objects.draw.base[cell] ^= 1;
    w.engine.io().set_window_cache(false);
    u6_core::gfx::view::render_window(&mut picture, w.engine.io(), true);
    w.engine.tick(&[], 0);
    w.draw();
    let newest = &w.presentation.as_ref().quiet().presenter.history()[0];
    assert!(
        !Rc::ptr_eq(newest, &previous_scene),
        "a fresh actual scene publication was consumed"
    );
    assert!(
        w.slide() == Some(&view),
        "a same-position publication at the release clock must preserve the remaining slide"
    );
    let mut previous = view.camera;
    let mut publications = 0;
    let mut generation = w.engine.published_version();
    let mut maximum = 0;
    for _ in 0..24 {
        w.display(1000.0 / 60.0);
        publications += usize::from(w.engine.published_version() != generation);
        generation = w.engine.published_version();
        let at = w.slide().map_or((0, 0), |v| v.camera);
        let delta = (at.0 - previous.0).abs().max((at.1 - previous.1).abs());
        maximum = maximum.max(delta);
        // Release settles at 16 pixels / 90 engine ms; a 60Hz frame at 4x travels
        // 11.85 pixels, rounded to at most 12. A publication must not snap the rest.
        assert!(
            delta <= 12,
            "a post-release publication snapped the remaining camera distance: {delta} px"
        );
        previous = at;
    }
    assert!(
        publications > 0,
        "actual engine publications occurred while settling"
    );
    assert!(w.slide().is_none(), "release reaches rest");
    println!(
        "native release publications={publications}, maximum camera displacement={maximum}px/frame"
    );
}

#[test]
fn releasing_after_a_pacer_reset_does_not_rewind_the_last_displayed_position() {
    let Some(mut w) = Window::new(true) else {
        return;
    };
    w.quiet();
    w.press(Key::Down);
    for _ in 0..360 {
        w.display(1000.0 / 60.0);
        if w.slide().is_some_and(off_grid) && w.pacer.carried_ms() >= 10.0 {
            break;
        }
    }
    let before = w
        .slide()
        .cloned()
        .expect("a slide shown between logical ticks");
    assert!(
        w.pacer.carried_ms() >= 10.0,
        "the display has advanced beyond the last tick"
    );
    w.release(Key::Down);
    w.pacer.set_speed(Speed::Finite(2.0));
    w.draw();
    assert!(
        w.slide() == Some(&before),
        "settling must start at the last displayed position even when a speed change drops carried time"
    );
}

#[test]
fn a_speed_change_without_a_tick_never_rewinds_a_held_slide() {
    let Some(mut w) = Window::new(true) else {
        return;
    };
    w.quiet();
    w.press(Key::Down);
    for _ in 0..360 {
        w.display(1000.0 / 60.0);
        if w.slide().is_some_and(off_grid) && w.pacer.carried_ms() >= 10.0 {
            break;
        }
    }
    let before = w
        .slide()
        .cloned()
        .expect("a held slide shown between logical ticks");
    let shown_at = w.presentation.as_ref().quiet().presented_ms;
    assert!(
        w.pacer.carried_ms() >= 10.0,
        "the display is ahead of the logical clock"
    );
    w.pacer.set_speed(Speed::Finite(2.0));
    w.draw();
    assert!(
        w.slide() == Some(&before),
        "a held session must keep its last drawn position when speed drops the carried fraction"
    );
    assert!(
        w.presentation.as_ref().quiet().presented_ms >= shown_at,
        "the native presentation clock cannot go backwards"
    );
}

/// Find a natural multi-corner route by asking the real planner on copies. Only the probe
/// acknowledges planned steps; the receipt below moves the live game through normal input.
fn detour_destination(state: &State) -> (u16, u16) {
    use u6_core::actors::travel::{Next, Polled, Travel};
    let leader = state.actors.party.leader().quiet().index();
    let origin = state.objects.location[leader];
    for dy in -5i16..=5 {
        for dx in -5i16..=5 {
            if dx.abs().max(dy.abs()) < 4 {
                continue;
            }
            let target = (
                origin.x.wrapping_add_signed(dx),
                origin.y.wrapping_add_signed(dy),
            );
            let Ok(mut route) = Travel::start(state, target) else {
                continue;
            };
            let mut probe = state.clone();
            let mut directions = Vec::new();
            for _ in 0..24 {
                let Next::Press(key) = route.next(
                    &probe,
                    &Polled {
                        input: &[],
                        buttons: 0,
                        queued: false,
                        foreign: false,
                        menu: false,
                        due: true,
                    },
                ) else {
                    break;
                };
                let (x, y) = match key {
                    Key::Up => (0, -1),
                    Key::UpRight => (1, -1),
                    Key::Right => (1, 0),
                    Key::DownRight => (1, 1),
                    Key::Down => (0, 1),
                    Key::DownLeft => (-1, 1),
                    Key::Left => (-1, 0),
                    Key::UpLeft => (-1, -1),
                    _ => unreachable!("a travel only emits movement"),
                };
                directions.push((x, y));
                let at = &mut probe.objects.location[leader];
                at.x = at.x.wrapping_add_signed(x);
                at.y = at.y.wrapping_add_signed(y);
                probe.map.centre = *at;
            }
            if directions.len() >= 5 && directions.windows(2).filter(|w| w[0] != w[1]).count() >= 2
            {
                return target;
            }
        }
    }
    panic!("the quiet castle has a natural route with at least two corners");
}

#[test]
fn automatic_route_corners_keep_learned_cadence_in_the_native_window() {
    let Some(mut on) = Window::new(true) else {
        return;
    };
    let Some(mut off) = Window::new(false) else {
        return;
    };
    for w in [&mut on, &mut off] {
        w.quiet();
        for actor in 0..256 {
            let mut state = w.state.borrow_mut();
            if state.actors.attrs.status[actor] & 0x80 == 0 {
                state.actors.attrs.status[actor] |= 0x10;
            }
        }
        w.pacer.set_speed(Speed::Finite(4.0));
    }
    let target = detour_destination(&on.state.borrow());
    on.engine.travel_to(target).quiet();
    off.engine.travel_to(target).quiet();
    let centre = on.state.borrow().map.centre;
    let start = (centre.x, centre.y);
    let session = on
        .engine
        .travel_session()
        .expect("an accepted route has a session");
    let mut previous = (i32::from(centre.x), i32::from(centre.y));
    let mut direction = None;
    let mut corners = Vec::new();
    let mut checked = std::collections::BTreeSet::new();
    let mut paths_with_corners = 0;
    let mut previous_drawn = std::collections::BTreeMap::new();
    let mut bounded_frames = 0;
    let party: Vec<i32> = {
        let state = on.state.borrow();
        state.actors.party.members[..state.actors.party.count()]
            .iter()
            .map(|&s| i32::from(s))
            .collect()
    };
    for _ in 0..2400 {
        on.display(1000.0 / 60.0);
        off.display(1000.0 / 60.0);
        same_game(&on, &off);
        let centre = on.state.borrow().map.centre;
        let at = (i32::from(centre.x), i32::from(centre.y));
        if at != previous {
            let next = ((at.0 - previous.0).signum(), (at.1 - previous.1).signum());
            if direction.replace(next).is_some_and(|old| old != next) {
                corners.push(at);
            }
            previous = at;
        }
        let p = &on.presentation.as_ref().quiet().presenter;
        if on.engine.travel_active() {
            assert_eq!(
                on.engine.travel_session(),
                Some(session),
                "automatic corners keep the same accepted route"
            );
        }
        for (slot, distance) in p.path_distances() {
            if p.timings()
                .iter()
                .find(|t| t.0 == slot)
                .is_some_and(|t| distance > t.3.abs().max(t.4.abs()) + 0.01)
            {
                paths_with_corners += 1;
            }
        }
        assert_eq!(
            p.capacity_resets(),
            0,
            "an ordinary route remains within the shared resource ceilings"
        );
        if let Some(scene) = p.history().first() {
            let view = on.slide();
            let mut positions = vec![(
                -1,
                (scene.scene.centre.0 * 16, scene.scene.centre.1 * 16),
                view.map_or((0, 0), |v| v.camera),
            )];
            positions.extend(
                scene
                    .scene
                    .placed
                    .iter()
                    .filter(|at| party.contains(&i32::from(at.slot)))
                    .map(|at| {
                        (
                            i32::from(at.slot),
                            (at.cell.0 * 16, at.cell.1 * 16),
                            view.and_then(|v| v.movers.iter().find(|m| m.slot == at.slot))
                                .map_or((0, 0), |m| m.offset),
                        )
                    }),
            );
            let time = on.presentation.as_ref().quiet().presented_ms;
            for (slot, target, offset) in positions {
                let drawn = (target.0 + offset.0, target.1 + offset.1);
                let velocity = p.timings().iter().find(|t| t.0 == slot).map_or(0.0, |t| {
                    p.path_distances()
                        .iter()
                        .find(|d| d.0 == slot)
                        .map_or(0.0, |d| d.1)
                        / t.2.max(0.001)
                });
                if let Some((old, old_time, old_velocity, old_target)) =
                    previous_drawn.insert(slot, (drawn, time, velocity, target))
                {
                    let logical = (target.0 - old_target.0)
                        .abs()
                        .max((target.1 - old_target.1).abs());
                    let delta = (drawn.0 - old.0).abs().max((drawn.1 - old.1).abs());
                    let bound =
                        (velocity.max(old_velocity).max(16.0 / 360.0) * (time - old_time)).ceil();
                    assert!(
                        logical > 16 || f64::from(delta) <= bound,
                        "native route mover exceeded its actual path-distance/duration bound"
                    );
                    bounded_frames += 1;
                }
            }
            let at = (scene.scene.centre.0, scene.scene.centre.1);
            if corners.contains(&at) && checked.insert(at) {
                let duration = p.timings().into_iter().find(|t| t.0 == -1).map(|t| t.2);
                assert!(
                    duration.is_some_and(|d| d > 360.0),
                    "an automatic route corner must preserve its learned interval instead of restarting at the tap duration"
                );
            }
        }
        if !on.engine.travel_active() {
            break;
        }
    }
    assert!(
        on.engine.travel_end() == Some(u6_core::actors::travel::End::Arrived),
        "the actual detour arrived"
    );
    assert!(
        corners.len() >= 2 && checked.len() >= 2,
        "at least two actual route corners were presented"
    );
    for _ in 0..120 {
        on.display(1000.0 / 60.0);
        off.display(1000.0 / 60.0);
    }
    same_game(&on, &off);
    assert!(
        !on.presentation.as_ref().quiet().presenter.walking() && on.slide().is_none(),
        "arrival drains the route and its slides"
    );
    on.assert_authoritative();
    println!(
        "native automatic route: {} corners retained cadence, {paths_with_corners} paths longer than direct, {bounded_frames} mover frames bounded; whole State/RNG/published pixels equal",
        checked.len(),
    );

    // A newly accepted request, even to the same destination, is a new session. Its transition
    // is read by the native helper at the last displayed position, never as another corner.
    for w in [&mut on, &mut off] {
        w.engine.travel_to(start).quiet();
    }
    let returning = on.engine.travel_session().quiet();
    assert_ne!(
        returning, session,
        "arrival followed by a new request starts a session"
    );
    for _ in 0..1200 {
        on.display(1000.0 / 60.0);
        off.display(1000.0 / 60.0);
        same_game(&on, &off);
        if on.slide().is_some_and(off_grid) && on.engine.travel_active() {
            break;
        }
    }
    let before = on
        .slide()
        .cloned()
        .expect("the returning route is visibly moving");
    let generation = on.presentation.as_ref().quiet().presenter.generation();
    for w in [&mut on, &mut off] {
        w.engine.travel_to(start).quiet();
        w.draw();
    }
    assert_ne!(
        on.engine.travel_session(),
        Some(returning),
        "replacement is accepted as a new session"
    );
    assert!(
        on.presentation.as_ref().quiet().presenter.generation() > generation,
        "native presenter observes route replacement"
    );
    assert!(
        on.slide() == Some(&before),
        "route replacement keeps the displayed position"
    );
    same_game(&on, &off);

    for w in [&mut on, &mut off] {
        w.pacer.set_speed(Speed::Max);
        w.pacer.due(0.0, false);
        w.draw();
    }
    assert!(
        on.slide().is_none() && on.engine.travel_active(),
        "Max bypasses route motion without cancelling its game input"
    );
    for w in [&mut on, &mut off] {
        w.pending.push(InputEvent::Key(Key::Escape));
    }
    for _ in 0..4 {
        on.display(16.0);
        off.display(16.0);
    }
    same_game(&on, &off);
    assert!(
        on.engine.travel_end() == Some(u6_core::actors::travel::End::Input),
        "Stop still cancels the actual replacement route"
    );
    assert!(
        !on.presentation.as_ref().quiet().presenter.walking(),
        "Stop ends the route cadence session"
    );
}

#[test]
fn footsteps_native_changes_display_only_and_off_restores_the_original() {
    let Some(mut a) = Window::new(true) else {
        return;
    };
    let Some(mut b) = Window::new(true) else {
        return;
    };
    a.presentation.as_mut().quiet().set_footstep_pattern(2);
    b.presentation.as_mut().quiet().set_footstep_pattern(0);
    a.quiet();
    b.quiet();
    let mut lifted = false;
    let mut different = false;
    for frame in 0..300 {
        if frame == 60 {
            a.press(Key::Down);
            b.press(Key::Down);
        }
        if frame == 160 {
            a.release(Key::Down);
            b.release(Key::Down);
        }
        a.display(1000.0 / 60.0);
        b.display(1000.0 / 60.0);
        assert!(
            *a.state.borrow() == *b.state.borrow(),
            "gait leaves State/RNG unchanged"
        );
        assert!(a.engine.published_frame().pixels() == b.engine.published_frame().pixels());
        let poses = &a.presentation.as_ref().quiet().shown.as_ref().quiet().poses;
        lifted |= poses.iter().any(|p| p.lift > 0);
        different |= a.screen != b.screen;
        if frame < 60 {
            assert!(poses.iter().all(|p| p.phase == 1 && p.lift == 0));
        }
    }
    assert!(lifted && different);
    assert!(
        a.presentation
            .as_ref()
            .quiet()
            .shown
            .as_ref()
            .quiet()
            .poses
            .iter()
            .any(|p| p.slot == 1 && p.phase == 1 && p.lift == 0),
        "settled avatar has a real neutral pose"
    );
    a.presentation.as_mut().quiet().set_footstep_pattern(0);
    a.draw();
    b.draw();
    assert!(a.screen == b.screen, "Off restores baseline pixels");
}

#[test]
fn attacks_native_lunge_and_recoil_keep_the_whole_game_and_publications_equal() {
    for ranged in [false, true] {
        let Some(mut on) = Window::new(true) else {
            return;
        };
        let Some(mut off) = Window::new(false) else {
            return;
        };
        for w in [&mut on, &mut off] {
            w.quiet();
            let mut s = w.state.borrow_mut();
            for a in 0..256 {
                if s.actors.attrs.status[a] & 0x80 == 0 {
                    s.actors.attrs.status[a] |= 0x10;
                }
            }
            let mut item = u6_core::objects::first_child(&mut s.objects, 1);
            while item >= 0 {
                s.objects.obj_status[item as usize] &= !0x08;
                item = u6_core::objects::next_child(&mut s.objects);
            }
            assert!(
                u6_core::objects::create_in(
                    &mut s,
                    if ranged { 0x29 } else { 0x22 },
                    0x18,
                    0xff00,
                    1
                ) >= 0
            );
            if ranged {
                assert!(u6_core::objects::create_in(&mut s, 0x37, 0x10, 10, 1) >= 0);
            }
            drop(s);
            w.tap(Key::Char('a'));
        }
        let advance = |a: &mut Window, b: &mut Window| {
            a.display(1000.0 / 60.0);
            b.display(1000.0 / 60.0);
            assert!(
                *a.state.borrow() == *b.state.borrow(),
                "State and RNG every frame"
            );
            assert!(a.engine.published_frame().pixels() == b.engine.published_frame().pixels());
            assert_eq!(a.engine.io().now_ms(), b.engine.io().now_ms());
        };
        for _ in 0..200 {
            advance(&mut on, &mut off);
            if on.state.borrow().ui.input.targeting != 0 {
                break;
            }
        }
        assert_ne!(on.state.borrow().ui.input.targeting, 0);
        on.tap(Key::Down);
        off.tap(Key::Down);
        for _ in 0..10 {
            advance(&mut on, &mut off);
        }
        on.tap(Key::Enter);
        off.tap(Key::Enter);
        let mut moved = false;
        for _ in 0..180 {
            advance(&mut on, &mut off);
            let poses = &on
                .presentation
                .as_ref()
                .quiet()
                .shown
                .as_ref()
                .quiet()
                .poses;
            if let Some(p) = poses.iter().find(|p| p.slot == 1 && p.offset != (0, 0)) {
                assert_eq!(p.offset.0, 0);
                assert!(if ranged {
                    p.offset.1 < 0
                } else {
                    p.offset.1 > 0
                });
                moved |= on.screen != off.screen;
            }
        }
        assert!(
            moved,
            "actual native window pixels show the attack (ranged={ranged})"
        );
        assert!(
            on.screen == off.screen,
            "returns to unchanged original pixels"
        );
    }
}

#[test]
fn footsteps_b_never_repaints_an_accepted_preview_after_max_or_instant() {
    for instant in [false, true] {
        let Some(mut w) = Window::new(true) else {
            return;
        };
        w.presentation.as_mut().quiet().set_footstep_pattern(2);
        w.quiet();
        w.press(Key::Down);
        let mut preview = false;
        for _ in 0..600 {
            w.display(1000.0 / 60.0);
            if w.presentation.as_ref().quiet().previewing && w.slide().is_some() {
                preview = true;
                break;
            }
        }
        assert!(preview, "a real accepted early preview is active");
        let unchanged = w.state.borrow().clone();
        let published = w.engine.frame_version();
        if instant {
            w.engine.io().set_turn_speed(TurnSpeed::Instant);
        } else {
            w.pacer.set_speed(Speed::Max);
            w.pacer.due(0.0, false);
        }
        w.draw();
        assert_eq!(w.engine.frame_version(), published);
        assert!(*w.state.borrow() == unchanged);
        assert!(w.slide().is_none());
        assert!(
            w.presentation
                .as_ref()
                .quiet()
                .shown
                .as_ref()
                .quiet()
                .poses
                .is_empty()
        );
        w.assert_authoritative();
    }
}

#[test]
fn footsteps_b_yields_to_actual_menu_and_foreign_pixels() {
    for foreign in [false, true] {
        let Some(mut w) = Window::new(true) else {
            return;
        };
        w.presentation.as_mut().quiet().set_footstep_pattern(2);
        w.moving_prompt();
        if foreign {
            w.state.borrow_mut().ui.input.idle_refresh = false;
            let (x, y, _, _) = u6_core::geom::SHOWN;
            w.engine
                .io()
                .draw(|fb| fb.set(usize::from(x) + 8, usize::from(y) + 8, 7));
            w.engine.io().present();
            w.engine.tick(&[], 0);
            w.draw();
        } else {
            let x = u6_core::geom::MAP_X as i16 + u6_core::geom::HALF_W as i16 * 16 + 8;
            let y = u6_core::geom::MAP_Y as i16 + u6_core::geom::HALF_H as i16 * 16 + 8;
            w.pending.push(crate::pointer(x, y, 2));
            for _ in 0..12 {
                w.display(1000.0 / 60.0);
                if w.engine.context_menu_open() {
                    break;
                }
            }
            assert!(w.engine.context_menu_open());
        }
        assert!(w.slide().is_none());
        assert!(
            w.presentation
                .as_ref()
                .quiet()
                .shown
                .as_ref()
                .quiet()
                .poses
                .is_empty()
        );
        w.assert_authoritative();
    }
}

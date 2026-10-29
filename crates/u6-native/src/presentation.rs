//! The native window's presentation, independently of minifb and the wall clock.
//!
//! The engine remains on the fixed logical step. This module consumes its copied map scenes
//! through the same Presenter and painter as the page, then scales the resulting pixels for
//! the window. Headless receipts call this exact drawing path, including its conversion cache.

use u6_core::{Engine, Key, gfx::motion::Shot, pacer::Pacer};
use u6_render::{
    Framebuffer,
    motion::{MIN_FRAMES, Presenter, SLIDE_MS, View, slides_shown},
};

use super::HeldKeys;
use std::rc::Rc;
use u6_render::motion::footsteps::{Footsteps, Pose};

#[derive(PartialEq)]
struct Drawn {
    version: u64,
    // Include scene identity as on the page: a publication can change the words in place
    // while keeping both the slide's geometry and its generation.
    slide: Option<(View, u64, Option<*const Shot>)>,
    ending: bool,
    poses: Vec<Pose>,
}

pub(super) struct Presentation {
    presenter: Presenter<Shot>,
    feet: Footsteps,
    attacks: u6_render::motion::attacks::Attacks,
    world: bool,
    previewing: bool,
    interval: f64,
    presented_ms: f64,
    frame: Framebuffer,
    rgba: Vec<u8>,
    shown: Option<Drawn>,
}

impl Presentation {
    pub(super) fn new(engine: &mut Engine) -> Self {
        engine.set_motion(true);
        let mut presentation = Self {
            presenter: Presenter::new(),
            feet: Footsteps::default(),
            attacks: Default::default(),
            world: false,
            previewing: false,
            interval: 1000.0 / 60.0,
            presented_ms: 0.0,
            frame: Framebuffer::new(),
            rgba: Vec::new(),
            shown: None,
        };
        presentation.set_footstep_pattern(2);
        presentation
    }

    /// Display policy shared with web: Off / 1px / 2px / 3px.
    pub(super) fn set_footstep_pattern(&mut self, code: u8) {
        self.feet
            .set_pattern(u6_render::motion::footsteps::Pattern::from_code(code));
        self.shown = None;
    }

    /// Follow the display interval, excluding a stalled frame, as the page does.
    pub(super) fn frame(&mut self, dt: f64) {
        if dt > 0.0 && dt <= u6_core::pacer::CAP_MS {
            self.interval += (dt - self.interval) / 8.0;
        }
    }

    /// Observe every logical step, including steps that publish no scene and previews W2
    /// withholds. Also called on display frames without a tick so release/modifiers settle
    /// immediately. This reads physical intent; it never generates or changes game input.
    pub(super) fn observe(
        &mut self,
        engine: &Engine,
        keys: &HeldKeys,
        modifier: bool,
        carried: f64,
    ) {
        let direction = (!modifier)
            .then(|| {
                u6_core::engine::newest_direction(
                    keys.keys.iter().filter(|h| h.walking).map(|h| (h.game, ())),
                )
            })
            .flatten()
            .map(|(key, ())| match key {
                Key::Up => (0, -1),
                Key::Down => (0, 1),
                Key::Left => (-1, 0),
                Key::Right => (1, 0),
                _ => unreachable!("newest_direction returns cardinal keys"),
            });
        let route = direction
            .is_none()
            .then(|| engine.travel_session())
            .flatten();
        let direction = direction.or_else(|| engine.continuous_walk_direction());
        let state = engine.state();
        let party = &state.actors.party;
        let slots: Vec<u16> = if direction.is_some() && !engine.context_menu_open() {
            party.members[..party.count()]
                .iter()
                .map(|&n| u16::from(n))
                .collect()
        } else {
            Vec::new()
        };
        let time = engine.io().now_ms() as f64;
        let transition = (time + carried).max(self.presented_ms);
        if let Some(session) = route {
            self.presenter
                .set_route_walking(&slots, session, transition);
        } else {
            self.presenter
                .set_walking(&slots, direction.unwrap_or((0, 0)), transition);
        }
        self.presenter.observe(
            -1,
            (i32::from(state.map.centre.x), i32::from(state.map.centre.y)),
            time,
        );
        for slot in slots {
            let at = state.objects.location[usize::from(slot)];
            self.presenter
                .observe(i32::from(slot), (i32::from(at.x), i32::from(at.y)), time);
        }
    }

    fn take(&mut self, engine: &mut Engine, pacer: &Pacer) -> Option<View> {
        let factor = engine.io().playback_speed().code();
        self.presenter.set_duration(if factor == 0 {
            0.0
        } else {
            SLIDE_MS / f64::from(factor)
        });
        let rate = pacer.pace_rate();
        self.presenter
            .set_minimum_duration(rate.map_or(0.0, |r| r * self.interval * MIN_FRAMES));
        let rate = if factor == 0 {
            None
        } else if self.presenter.walking() {
            rate
        } else {
            rate.map(|r| r * f64::from(factor))
        };
        // Changing pace discards the carried fraction. Keep the display clock monotonic
        // until logical time catches up, including a continuing held or route session.
        let time = (engine.io().now_ms() as f64 + pacer.carried_ms()).max(self.presented_ms);
        self.presented_ms = time;
        // After release, an unchanged-position publication must preserve the finite settle
        // already running. The minimum duration still gates any newly started short track.
        let slides = if self.presenter.walking() || self.presenter.active(time) {
            rate.is_some_and(|r| r.is_finite() && r > 0.0)
        } else {
            slides_shown(rate, self.interval)
        };
        let mut blocked = None;
        for publication in engine.take_motion() {
            let time = publication.time_ms as f64;
            if publication.shot.as_ref().is_none_or(|s| s.scene.snap) {
                self.feet.clear();
                self.attacks.clear();
                blocked = Some(publication.time_ms);
            }
            let snaps = self.presenter.snaps();
            match publication.shot {
                Some(shot) if publication.preview => {
                    if self.presenter.accept_preview(shot, time, slides) {
                        self.world = true;
                        self.previewing = true;
                    }
                }
                Some(shot) => {
                    self.presenter.publish(shot, time, slides);
                    self.world = true;
                    self.previewing = false;
                }
                None => {
                    self.presenter.foreign();
                    self.world = false;
                }
            }
            if self.presenter.snaps() != snaps {
                self.attacks.clear();
                blocked = Some(publication.time_ms);
            }
        }
        for (at, strike) in engine.io().take_attacks() {
            if blocked.is_none_or(|b| at > b) && self.world && rate.is_some() {
                self.attacks.start(strike, at as f64, factor);
            }
        }
        if u6_core::engine::command_prompt(&engine.state()) {
            self.presenter.prompt();
        }
        // Speed changes also apply on displays with no new publication. In particular Max
        // must not keep a slide which began at a finite pace.
        if rate.is_none() {
            self.presenter.snap();
            self.attacks.clear();
        }
        // A popup or the ending must own the complete frame, with no map repaint over it.
        if engine.context_menu_open()
            || engine.state().main.program == u6_core::main_loop::Program::End
        {
            self.presenter.snap();
            self.attacks.clear();
            return None;
        }
        let view = self
            .presenter
            .view(time)
            .filter(|_| self.presenter.active(time));
        self.presenter.end(time);
        view
    }

    /// Draw the pixels passed to minifb on every display frame, even with no logical tick.
    /// Unchanged views reuse the previous conversion; the first frame after a slide ends
    /// converts the authoritative frame again, even if its version has not changed.
    pub(super) fn draw(&mut self, engine: &mut Engine, pacer: &Pacer, screen: &mut [u32]) {
        let view = self.take(engine, pacer);
        let eligible = self.world
            && (!self.previewing || view.is_some())
            && !engine.context_menu_open()
            && engine.state().main.program == u6_core::main_loop::Program::Game;
        let mut poses = if eligible {
            u6_core::gfx::motion::footstep_poses(&self.presenter, &mut self.feet, view.as_ref())
        } else {
            self.feet.clear();
            Vec::new()
        };
        if eligible {
            self.attacks.apply(self.presented_ms, &mut poses);
        } else {
            self.attacks.clear();
        }
        let ending = engine.state().main.program == u6_core::main_loop::Program::End;
        let shown = Drawn {
            version: engine.frame_version(),
            slide: view.as_ref().map(|v| {
                (
                    v.clone(),
                    self.presenter.generation(),
                    self.presenter.history().first().map(Rc::as_ptr),
                )
            }),
            ending,
            poses: poses.clone(),
        };
        if self.shown.as_ref() == Some(&shown) {
            return;
        }
        let view = view.or_else(|| {
            (!poses.is_empty()).then(|| View {
                camera: (0, 0),
                movers: Vec::new(),
            })
        });
        if let Some(view) = view {
            self.frame.clone_from(engine.frame());
            u6_core::gfx::motion::paint_footsteps(
                &engine.state(),
                &self.presenter,
                &view,
                &poses,
                &mut self.frame,
            );
            if let Some(sprite) = engine.sprite() {
                sprite.draw(&mut self.frame);
            }
            self.frame.write_rgba(&mut self.rgba);
        } else {
            engine.frame().write_rgba(&mut self.rgba);
        }
        if ending {
            super::upscale_ending(&self.rgba, screen);
        } else {
            super::upscale(&self.rgba, screen);
        }
        self.shown = Some(shown);
    }
}

#[cfg(test)]
mod tests;

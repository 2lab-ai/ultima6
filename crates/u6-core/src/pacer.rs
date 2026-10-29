//! The fixed game step and the speed control (spec 06 §Design 1–2). Port-only: the original
//! runs as fast as the 68000 does, so nothing here has an original address.
//!
//! The engine is stepped only in [`STEP_MS`] (one PAL frame), so the display generation counts
//! PAL frames and every [`crate::Io::vblank`] wait lasts 20 ms of engine time on any display.
//! The host owns the clock: once per display frame it asks the [`Pacer`] how many steps the wall
//! time since the previous frame is worth ([`Pacer::due`]), runs up to that many while its frame
//! budget lasts ([`budget_ms`]) and reports how many it ran ([`Pacer::ran`]). The pacer holds no
//! clock and does no I/O; the same wall times give the same steps.
//!
//! A pointer button held paces at 1× whatever the speed: a held walk repeats on the engine
//! clock ([`crate::ui::target`] `HELD_WALK_MS`), so above 1× a press of a moment would walk
//! (at 8×, one held past about 82 ms).

use crate::combat::present::FRAME_MS;

/// Engine milliseconds of one step: one PAL frame.
pub const STEP_MS: u32 = FRAME_MS as u32;
/// Wall milliseconds of one host frame the pacer takes at most; the rest of a longer frame (a
/// stall, a hidden tab) is cut ([`Pacer::clamped_ms`]).
pub const CAP_MS: f64 = 250.0;
/// Steps one host frame runs at most.
pub const MAX_STEPS: u32 = 64;
/// Shortest frame budget in wall milliseconds ([`budget_ms`]).
pub const MIN_BUDGET_MS: f64 = 3.0;
/// The speeds the page offers besides [`Speed::Max`] (spec 06 §Design 4). 1× is the oracle's
/// pace and the default.
pub const PRESETS: [f64; 7] = [1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0];

/// How fast the game runs against wall time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Speed {
    /// `x` engine milliseconds per wall millisecond (a [`PRESETS`] value or any positive finite
    /// number).
    Finite(f64),
    /// As many steps as the host's frame budget holds, up to [`MAX_STEPS`]; never behind.
    Max,
}

impl Speed {
    /// True for [`Speed::Max`] and for a finite speed above 0.
    pub fn is_valid(self) -> bool {
        match self {
            Speed::Finite(x) => x.is_finite() && x > 0.0,
            Speed::Max => true,
        }
    }
}

impl Default for Speed {
    fn default() -> Self {
        Speed::Finite(1.0)
    }
}

/// Converts the wall time between host frames into steps owed, at a [`Speed`]. Commits only on
/// report: [`Pacer::due`] adds a frame's time and says how many steps it is worth, the host runs
/// some of them, [`Pacer::ran`] takes off those it ran.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pacer {
    speed: Speed,
    /// The frame [`Pacer::due`] last added had a pointer button held (it paced at 1×).
    held: bool,
    /// Engine milliseconds owed (at most [`CAP_MS`] × the rate after [`Pacer::ran`]).
    acc: f64,
    /// Wall milliseconds cut by [`CAP_MS`] so far.
    clamped: f64,
    /// Wall milliseconds owed and not run so far.
    dropped: f64,
}

impl Pacer {
    /// A pacer at 1× with nothing owed.
    pub fn new() -> Pacer {
        Pacer::default()
    }

    /// Engine milliseconds per wall millisecond for the frame [`Pacer::due`] last added: 1 while
    /// a button is held, else the speed; none at [`Speed::Max`] with no button held.
    fn rate(&self) -> Option<f64> {
        match (self.speed, self.held) {
            (_, true) => Some(1.0),
            (Speed::Finite(s), false) => Some(s),
            (Speed::Max, false) => None,
        }
    }

    /// Adds a frame of `wall_dt_ms` and returns the steps owed, at most [`MAX_STEPS`]; nothing is
    /// taken off until [`Pacer::ran`]. The frame counts from 0 to [`CAP_MS`] (what lies beyond
    /// goes to [`Pacer::clamped_ms`]; a time that is not finite counts 0), at the speed, or at 1×
    /// while `held` (a pointer button is down), at [`Speed::Max`] too. A press (`held` after a
    /// frame without) at any speed but 1× is a change of rate: what was owed is forgotten,
    /// neither dropped nor clamped, so the hold runs exactly its 1× steps. At 1× the rate stays
    /// and so does what was owed. At [`Speed::Max`] with no button held: [`MAX_STEPS`], nothing
    /// added or cut.
    pub fn due(&mut self, wall_dt_ms: f64, held: bool) -> u32 {
        if held && !self.held && self.speed != Speed::Finite(1.0) {
            self.acc = 0.0;
        }
        self.held = held;
        let Some(rate) = self.rate() else {
            return MAX_STEPS;
        };
        let dt = if wall_dt_ms.is_finite() {
            wall_dt_ms.max(0.0)
        } else {
            0.0
        };
        let taken = dt.min(CAP_MS);
        self.clamped += dt - taken;
        self.acc += taken * rate;
        ((self.acc / f64::from(STEP_MS)).floor() as u32).min(MAX_STEPS)
    }

    /// The host ran `k` steps of those [`Pacer::due`] gave (k may be 0). They are taken off, and
    /// what is still owed past [`CAP_MS`] of wall time at the frame's rate is dropped (counted in
    /// wall milliseconds, the owed engine time ÷ the rate, except at [`Speed::Max`], which is
    /// never behind). At [`Speed::Max`] with no button held nothing stays owed.
    pub fn ran(&mut self, k: u32) {
        let Some(rate) = self.rate() else {
            self.acc = 0.0;
            return;
        };
        self.acc = (self.acc - f64::from(k) * f64::from(STEP_MS)).max(0.0);
        let cap = CAP_MS * rate;
        if self.acc > cap {
            if self.speed != Speed::Max {
                self.dropped += (self.acc - cap) / rate;
            }
            self.acc = cap;
        }
    }

    /// Port-only (spec 20): the engine milliseconds the wall clock is ahead of the steps run, at
    /// most one step ([`STEP_MS`]; 0 at [`Speed::Max`]): what a clock that moves between the steps
    /// adds to the engine's (smooth motion's presentation clock).
    pub fn carried_ms(&self) -> f64 {
        self.acc.min(f64::from(STEP_MS))
    }

    /// The engine milliseconds per wall millisecond the last frame [`Pacer::due`] added was paced
    /// at: 1 while a button was held, else the speed; `None` at [`Speed::Max`].
    pub fn pace_rate(&self) -> Option<f64> {
        self.rate()
    }

    /// Sets the speed; a change of speed or mode forgets what was owed. An invalid speed (a
    /// finite one that is not above 0, NaN or infinite) is ignored.
    pub fn set_speed(&mut self, speed: Speed) {
        if speed.is_valid() && speed != self.speed {
            self.speed = speed;
            self.acc = 0.0;
        }
    }

    pub fn speed(&self) -> Speed {
        self.speed
    }

    /// Wall milliseconds cut by [`CAP_MS`] since the pacer was made: frames longer than the cap
    /// (a hidden tab, a debugger pause), not time the host fell behind by.
    pub fn clamped_ms(&self) -> f64 {
        self.clamped
    }

    /// Wall milliseconds owed and not run since the pacer was made: the steps the host's budget
    /// did not hold, past the cap. The one source of the "behind" indicator; never grows at
    /// [`Speed::Max`].
    pub fn dropped_ms(&self) -> f64 {
        self.dropped
    }
}

/// The host's step budget for one frame, in wall milliseconds: half the measured interval
/// between display frames, at least [`MIN_BUDGET_MS`] (also for an interval that is not a
/// number).
pub fn budget_ms(raf_interval_ms: f64) -> f64 {
    (0.5 * raf_interval_ms).max(MIN_BUDGET_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds frames of `frame_ms` for `total_ms` of wall time (the last one shorter), `held` or
    /// not, running every step due; returns the steps run.
    fn run_held(p: &mut Pacer, frame_ms: f64, total_ms: f64, held: bool) -> u64 {
        let (mut t, mut steps) = (0.0, 0u64);
        while t < total_ms {
            let dt = frame_ms.min(total_ms - t);
            t += dt;
            let n = p.due(dt, held);
            p.ran(n);
            steps += u64::from(n);
        }
        steps
    }

    fn run(p: &mut Pacer, frame_ms: f64, total_ms: f64) -> u64 {
        run_held(p, frame_ms, total_ms, false)
    }

    fn at(speed: Speed) -> Pacer {
        let mut p = Pacer::new();
        p.set_speed(speed);
        p
    }

    #[test]
    fn steps_follow_the_wall_clock_at_every_preset_and_frame_size() {
        const T: f64 = 10_000.0;
        for s in PRESETS {
            for frame in [1.0, 7.0, 1000.0 / 60.0, 20.0, 33.0, 100.0] {
                let mut p = at(Speed::Finite(s));
                let steps = run(&mut p, frame, T);
                let want = (T * s / f64::from(STEP_MS)).floor() as u64;
                println!("{s}× at {frame:.2} ms frames: {steps} steps, want {want}");
                assert!(
                    steps.abs_diff(want) <= 1,
                    "{s}× {frame} ms: {steps} vs {want}"
                );
                assert_eq!(
                    (p.clamped_ms(), p.dropped_ms()),
                    (0.0, 0.0),
                    "{s}× {frame} ms: nothing lost"
                );
            }
        }
    }

    #[test]
    fn a_stall_past_the_cap_is_clamped_not_dropped_then_the_pace_recovers() {
        let mut p = Pacer::new();
        assert_eq!(run(&mut p, 20.0, 1000.0), 50);
        let n = p.due(5_000.0, false);
        assert_eq!(n, 12, "250 ms of the stall: 12 steps, 10 ms owed");
        assert_eq!(p.clamped_ms(), 4_750.0, "the wall time past the cap");
        p.ran(n);
        assert_eq!(p.dropped_ms(), 0.0, "everything owed was run");
        let after = run(&mut p, 1000.0 / 60.0, 1000.0);
        assert!(after.abs_diff(50) <= 1, "{after} steps in the second after");
        assert_eq!((p.clamped_ms(), p.dropped_ms()), (4_750.0, 0.0));
    }

    #[test]
    fn an_overload_drops_what_the_host_cannot_run_and_stays_bounded() {
        let mut p = at(Speed::Finite(8.0));
        let mut last = 0.0;
        for frame in 0..600 {
            let n = p.due(1000.0 / 60.0, false);
            assert!(n <= MAX_STEPS);
            // The host's budget holds two steps a frame; 8× at 60 Hz wants 6.67.
            p.ran(n.min(2));
            if frame > 100 {
                assert!(p.dropped_ms() > last, "frame {frame}: still behind");
            }
            last = p.dropped_ms();
            assert!(p.acc <= CAP_MS * 8.0, "the debt is bounded");
        }
        // 10 s at 8× owe 4000 steps: 1200 ran, 100 stay owed (the cap), 2700 dropped, which
        // are 6750 ms of wall time.
        println!("dropped {:.3} ms after 10 s", p.dropped_ms());
        assert!(
            (p.dropped_ms() - 6_750.0).abs() < 0.01,
            "{}",
            p.dropped_ms()
        );
        assert_eq!(p.clamped_ms(), 0.0, "no frame past the cap");
    }

    #[test]
    fn max_gives_the_step_cap_and_never_drops() {
        let mut p = at(Speed::Max);
        for (dt, k) in [(16.0, 3), (5_000.0, 0), (-1.0, 64), (f64::NAN, 10)] {
            assert_eq!(p.due(dt, false), MAX_STEPS);
            p.ran(k);
            assert_eq!(p.acc, 0.0, "nothing owed");
        }
        assert_eq!((p.clamped_ms(), p.dropped_ms()), (0.0, 0.0), "never behind");
        // Held, a host that runs nothing: owed at 1×, still never behind.
        for _ in 0..100 {
            p.due(100.0, true);
            p.ran(0);
        }
        assert_eq!(p.dropped_ms(), 0.0);
        assert_eq!(p.acc, CAP_MS, "bounded at the cap");
    }

    /// A held button paces at 1× whatever the speed: 150 ms held is the 1× count at 8× and at
    /// Max, at 60 Hz and at 144 Hz; released, the speed again.
    #[test]
    fn a_held_button_paces_at_1x() {
        for frame in [1000.0 / 60.0, 1000.0 / 144.0] {
            let one = run_held(&mut Pacer::new(), frame, 150.0, true);
            assert_eq!(one, 7, "150 ms at 1×");
            for speed in [Speed::Finite(8.0), Speed::Max] {
                let mut p = at(speed);
                assert_eq!(run_held(&mut p, frame, 150.0, true), one, "{speed:?} held");
                let free = run(&mut p, frame, 150.0);
                println!("{speed:?} at {frame:.2} ms frames: 150 ms held {one}, then {free}");
                assert!(free > 50, "{speed:?} released: {free}");
            }
        }
    }

    /// A press while steps are still owed (a frame past the cap, a host that ran none) starts
    /// from nothing: the hold is the 1× count, and none of the old debt is counted lost.
    #[test]
    fn a_press_forgets_the_debt_and_counts_nothing_lost() {
        for speed in [Speed::Finite(8.0), Speed::Max] {
            let mut p = at(speed);
            p.due(400.0, false);
            p.ran(0);
            let (clamped, dropped) = (p.clamped_ms(), p.dropped_ms());
            let held = run_held(&mut p, 1000.0 / 60.0, 150.0, true);
            println!("{speed:?}: 150 ms held after a debt: {held} steps");
            assert_eq!(held, 7, "{speed:?}: the 1× count");
            assert_eq!(
                (p.clamped_ms(), p.dropped_ms()),
                (clamped, dropped),
                "{speed:?}: the press counts nothing lost"
            );
            // The first held frame alone, the host running none of it: still nothing dropped.
            let mut q = at(speed);
            q.due(250.0, false);
            q.ran(0);
            let n = q.due(100.0, true);
            q.ran(0);
            assert_eq!(q.dropped_ms(), 0.0, "{speed:?}: not behind");
            assert_eq!(n, 5, "{speed:?}: 100 ms at 1×");
        }
    }

    /// At 1× a press changes no rate, so it forgets nothing: a step's remainder and a behind
    /// host's debt stay owed, and presses on and off keep the count at `floor(T / 20)` ± 1.
    #[test]
    fn a_press_at_1x_keeps_what_is_owed() {
        let mut p = Pacer::new();
        assert_eq!(p.due(30.0, false), 1);
        p.ran(1);
        assert_eq!(p.due(5.0, true), 0, "the press");
        assert_eq!(p.acc, 15.0, "the remainder kept through the press");
        let mut q = Pacer::new();
        q.due(250.0, false);
        q.ran(0);
        assert_eq!(q.due(0.0, true), 12, "the debt owed before the press");
        assert_eq!((q.clamped_ms(), q.dropped_ms()), (0.0, 0.0));
        const T: f64 = 10_000.0;
        for frame in [1000.0 / 60.0, 1000.0 / 144.0, 7.0, 33.0f64] {
            let mut p = Pacer::new();
            let (mut t, mut steps) = (0.0, 0u64);
            while t < T {
                let dt = frame.min(T - t);
                t += dt;
                // Pressed 150 ms of every 400.
                let n = p.due(dt, t % 400.0 < 150.0);
                p.ran(n);
                steps += u64::from(n);
            }
            let want = (T / f64::from(STEP_MS)).floor() as u64;
            println!("1× with presses at {frame:.2} ms frames: {steps} steps, want {want}");
            assert!(steps.abs_diff(want) <= 1, "{frame} ms: {steps} vs {want}");
        }
    }

    #[test]
    fn a_change_of_speed_forgets_the_debt() {
        let mut p = Pacer::new();
        assert_eq!(p.due(50.0, false), 2);
        p.ran(0);
        p.set_speed(Speed::Finite(1.0));
        assert_eq!(p.due(0.0, false), 2, "the same speed again keeps the debt");
        p.set_speed(Speed::Finite(2.0));
        assert_eq!(p.due(0.0, false), 0, "a new speed starts from nothing");
        assert_eq!(p.due(15.0, false), 1, "30 engine ms");
        p.set_speed(Speed::Max);
        p.set_speed(Speed::Finite(2.0));
        assert_eq!(p.due(0.0, false), 0, "through Max and back");
    }

    #[test]
    fn invalid_speeds_and_wall_times_are_refused() {
        let mut p = at(Speed::Finite(3.0));
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            p.set_speed(Speed::Finite(bad));
            assert_eq!(p.speed(), Speed::Finite(3.0), "{bad} ignored");
        }
        assert_eq!(
            p.due(-40.0, false),
            0,
            "a clock that went back adds nothing"
        );
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(p.due(bad, false), 0, "{bad} counts 0");
            assert_eq!(p.due(bad, true), 0, "{bad} held counts 0");
        }
        assert_eq!((p.clamped_ms(), p.dropped_ms()), (0.0, 0.0));
        assert_eq!(p.due(20.0, false), 3, "the pace goes on after them");
        p.due(10.0, false);
        p.ran(5);
        assert_eq!(
            p.acc, 0.0,
            "more reported than owed: nothing owed, no credit"
        );
    }

    #[test]
    fn the_budget_is_half_the_frame_interval_at_least_3_ms() {
        assert_eq!(budget_ms(1000.0 / 60.0), 1000.0 / 120.0);
        assert_eq!(budget_ms(1000.0 / 144.0), 500.0 / 144.0);
        assert_eq!(budget_ms(4.0), MIN_BUDGET_MS);
        assert_eq!(budget_ms(f64::NAN), MIN_BUDGET_MS);
    }
}

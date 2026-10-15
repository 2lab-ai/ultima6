//! The window on the fixed step (spec 06 §Design 8): each window frame runs the steps of
//! [`u6_core::pacer::STEP_MS`] the wall time since the previous frame is worth at the speed
//! ([`u6_core::pacer::Pacer`], the page's pacer too), while the frame budget lasts; the speed
//! steps through the page's presets and shows in the window title.

use u6_core::pacer::{PRESETS, Pacer, Speed, budget_ms};

/// The window title at `speed`: the plain name at 1×, else the name and the speed
/// (`Ultima VI — 2×`, `Ultima VI — Max`).
pub fn title(speed: Speed) -> String {
    match speed {
        Speed::Finite(1.0) => "Ultima VI".to_string(),
        Speed::Finite(x) => format!("Ultima VI — {x}×"),
        Speed::Max => "Ultima VI — Max".to_string(),
    }
}

/// The next preset above `speed`, [`Speed::Max`] past the last one (and at Max).
pub fn faster(speed: Speed) -> Speed {
    match speed {
        Speed::Finite(x) => PRESETS
            .into_iter()
            .find(|&p| p > x)
            .map_or(Speed::Max, Speed::Finite),
        Speed::Max => Speed::Max,
    }
}

/// The next preset below `speed`, the last one from [`Speed::Max`]; 1× stays.
pub fn slower(speed: Speed) -> Speed {
    match speed {
        Speed::Finite(x) => PRESETS
            .into_iter()
            .rev()
            .find(|&p| p < x)
            .map_or(speed, Speed::Finite),
        Speed::Max => PRESETS.last().map_or(speed, |&p| Speed::Finite(p)),
    }
}

/// One window frame's steps: asks `pacer` what `wall_dt_ms` (the wall time since the previous
/// frame) is worth, at 1× while `held` (a mouse button down), and runs them through `step`. The
/// first always runs, each next one only while `elapsed_ms()` (wall milliseconds since the frame's steps
/// began) is under the frame budget, [`budget_ms`] of `wall_dt_ms` (the page's rule, with its
/// measured frame interval). `step` returns false once the game ended, which stops the frame.
/// The steps run are reported to the pacer, also on an error (the step that failed ran);
/// returns them.
pub fn run_frame<E>(
    pacer: &mut Pacer,
    wall_dt_ms: f64,
    held: bool,
    mut elapsed_ms: impl FnMut() -> f64,
    mut step: impl FnMut() -> Result<bool, E>,
) -> Result<u32, E> {
    let due = pacer.due(wall_dt_ms, held);
    let budget = budget_ms(wall_dt_ms);
    let mut k = 0;
    while k < due && (k == 0 || elapsed_ms() < budget) {
        let going = step();
        k += 1;
        match going {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                pacer.ran(k);
                return Err(e);
            }
        }
    }
    pacer.ran(k);
    Ok(k)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use u6_core::pacer::{MAX_STEPS, STEP_MS};

    const HZ60: f64 = 1000.0 / 60.0;

    /// Runs `frames` window frames of `frame_ms` at `speed`, each step costing `cost_ms` of wall
    /// time on a fake clock; returns the steps of each frame.
    fn frames(speed: Speed, frame_ms: f64, frames: usize, cost_ms: f64) -> (Vec<u32>, Pacer) {
        let mut pacer = Pacer::new();
        pacer.set_speed(speed);
        let mut per = Vec::new();
        for _ in 0..frames {
            let clock = Cell::new(0.0);
            let k = run_frame(
                &mut pacer,
                frame_ms,
                false,
                || clock.get(),
                || {
                    clock.set(clock.get() + cost_ms);
                    Ok::<_, ()>(true)
                },
            );
            per.push(k.unwrap());
        }
        (per, pacer)
    }

    /// The window's frames at 60 Hz, 30 Hz and 10 Hz, at 1× and 4×: the steps follow the wall
    /// clock (exactly where the frame time is a whole number of ms, else within one), each
    /// frame runs the whole or a step short of what its own time is worth, and nothing is lost.
    #[test]
    fn the_frames_run_the_steps_their_wall_time_is_worth() {
        for (speed, frame_ms, lo, hi) in [
            (1.0, HZ60, 0, 1),
            (4.0, HZ60, 3, 4),
            (1.0, 33.0, 1, 2),
            (4.0, 33.0, 6, 7),
            (1.0, 100.0, 5, 5),
            (4.0, 100.0, 20, 20),
        ] {
            let n = 60;
            let (per, pacer) = frames(Speed::Finite(speed), frame_ms, n, 0.0);
            let total: u32 = per.iter().sum();
            let want = (n as f64 * frame_ms * speed / f64::from(STEP_MS)).floor() as u32;
            println!(
                "{speed}× at {frame_ms:.2} ms frames: {total} steps in {n} frames, want {want}"
            );
            if frame_ms.fract() == 0.0 {
                assert_eq!(total, want, "{speed}× {frame_ms} ms");
            } else {
                assert!(total.abs_diff(want) <= 1, "{speed}× {frame_ms} ms: {total}");
            }
            assert!(
                per.iter().all(|k| (lo..=hi).contains(k)),
                "{speed}× {frame_ms} ms: {per:?}"
            );
            assert_eq!((pacer.clamped_ms(), pacer.dropped_ms()), (0.0, 0.0));
        }
    }

    #[test]
    fn the_budget_stops_the_steps_after_the_first() {
        // 100 ms at 4× owes 20 steps; the budget is 50 ms, a step costs 8: steps start at 0,
        // 8, .., 48 ms, so 7 run, and the rest stay owed up to the cap.
        let (per, pacer) = frames(Speed::Finite(4.0), 100.0, 1, 8.0);
        assert_eq!(per, [7]);
        assert_eq!(pacer.dropped_ms(), 0.0, "owed, under the cap");
        // A step slower than the whole budget still runs, one a frame: 1× at 60 Hz, which owes
        // at most one a frame, keeps its pace; 8× goes on, behind.
        let (per, pacer) = frames(Speed::Finite(1.0), HZ60, 120, 40.0);
        assert_eq!(per, frames(Speed::Finite(1.0), HZ60, 120, 0.0).0);
        assert_eq!(pacer.dropped_ms(), 0.0);
        let (per, pacer) = frames(Speed::Finite(8.0), HZ60, 120, 40.0);
        assert!(per.iter().all(|&k| k == 1), "{per:?}");
        assert!(pacer.dropped_ms() > 0.0, "8× falls behind");
    }

    #[test]
    fn max_runs_what_the_budget_holds_up_to_the_step_cap() {
        // 60 Hz: an 8.33 ms budget, 1 ms steps: steps start at 0, 1, .., 8 ms.
        let (per, pacer) = frames(Speed::Max, HZ60, 30, 1.0);
        assert!(per.iter().all(|&k| k == 9), "{per:?}");
        let (per, _) = frames(Speed::Max, HZ60, 3, 0.0);
        assert_eq!(per, [MAX_STEPS; 3], "free steps: the cap");
        assert_eq!(pacer.dropped_ms(), 0.0, "never behind");
    }

    #[test]
    fn a_held_button_runs_the_1x_steps_at_any_speed() {
        for speed in [Speed::Finite(8.0), Speed::Max] {
            let mut pacer = Pacer::new();
            pacer.set_speed(speed);
            let mut steps = 0;
            for _ in 0..9 {
                steps += run_frame(&mut pacer, HZ60, true, || 0.0, || Ok::<_, ()>(true)).unwrap();
            }
            assert_eq!(steps, 7, "{speed:?}: 150 ms held");
        }
    }

    #[test]
    fn the_game_ending_or_failing_stops_the_frame_and_counts_the_step() {
        let mut pacer = Pacer::new();
        let mut left = 3;
        let k = run_frame(
            &mut pacer,
            100.0,
            false,
            || 0.0,
            || {
                left -= 1;
                Ok::<_, ()>(left > 0)
            },
        );
        assert_eq!(k, Ok(3), "the step that ended ran");
        assert_eq!(pacer.due(0.0, false), 2, "the other two stay owed");
        let mut pacer = Pacer::new();
        let k = run_frame(&mut pacer, 100.0, false, || 0.0, || Err::<bool, _>("disk"));
        assert_eq!(k, Err("disk"));
        assert_eq!(pacer.due(0.0, false), 4, "the failed step ran");
    }

    #[test]
    fn the_speed_steps_through_the_presets_and_shows_in_the_title() {
        let mut up = vec![Speed::default()];
        while up.last() != Some(&Speed::Max) {
            up.push(faster(*up.last().unwrap()));
        }
        let want: Vec<Speed> = PRESETS
            .into_iter()
            .map(Speed::Finite)
            .chain([Speed::Max])
            .collect();
        assert_eq!(up, want, "1× up to Max");
        assert_eq!(faster(Speed::Max), Speed::Max);
        let mut down = vec![Speed::Max];
        while down.last() != Some(&Speed::Finite(1.0)) {
            down.push(slower(*down.last().unwrap()));
        }
        assert_eq!(
            down,
            want.into_iter().rev().collect::<Vec<_>>(),
            "Max down to 1×"
        );
        assert_eq!(slower(Speed::Finite(1.0)), Speed::Finite(1.0));
        assert_eq!(
            faster(Speed::Finite(2.5)),
            Speed::Finite(3.0),
            "off the list"
        );
        assert_eq!(slower(Speed::Finite(2.5)), Speed::Finite(2.0));
        let titles: Vec<String> = [1.0, 1.5, 2.0, 8.0]
            .map(Speed::Finite)
            .into_iter()
            .chain([Speed::Max])
            .map(title)
            .collect();
        assert_eq!(
            titles,
            [
                "Ultima VI",
                "Ultima VI — 1.5×",
                "Ultima VI — 2×",
                "Ultima VI — 8×",
                "Ultima VI — Max"
            ]
        );
    }
}

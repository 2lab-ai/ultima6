//! Display-only humanoid gait. Inputs are displayed world pixels, never camera pixels or time.
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pattern {
    Off,
    A1,
    #[default]
    B2,
    C3,
}
impl Pattern {
    pub fn from_code(code: u8) -> Self {
        match code {
            1 => Self::A1,
            2 => Self::B2,
            3 => Self::C3,
            _ => Self::Off,
        }
    }
    pub fn code(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::A1 => 1,
            Self::B2 => 2,
            Self::C3 => 3,
        }
    }
    /// Phase of one eight-pixel step. These are the same curves used by the A/B/C preview.
    pub fn lift(self, phase: f64) -> i32 {
        if !phase.is_finite() || !(0.0..=1.0).contains(&phase) {
            return 0;
        }
        let height = match self {
            Self::Off => 0.0,
            Self::A1 => (std::f64::consts::PI * phase).sin(),
            Self::B2 if phase > 0.15 && phase < 0.85 => {
                2.0 * (std::f64::consts::PI * (phase - 0.15) / 0.7).sin()
            }
            Self::C3 if phase > 0.05 && phase < 0.95 => {
                let q = (phase - 0.05) / 0.9;
                3.0 * 4.0 * q * (1.0 - q)
            }
            _ => 0.0,
        };
        height.round().clamp(0.0, f64::from(self.code())) as i32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pose {
    pub slot: u16,
    pub phase: u16,
    pub lift: i32,
    pub offset: (i32, i32),
}

#[derive(Default)]
pub struct Footsteps {
    pattern: Pattern,
    previous: BTreeMap<u16, ((i32, i32), u32)>,
}
impl Footsteps {
    pub fn pattern(&self) -> Pattern {
        self.pattern
    }
    pub fn set_pattern(&mut self, pattern: Pattern) {
        if self.pattern != pattern {
            self.clear();
            self.pattern = pattern;
        }
    }
    pub fn clear(&mut self) {
        self.previous.clear();
    }
    /// One sample per displayed frame. A subpixel pause within a live track keeps its pose;
    /// actual rest returns to phase 1, the middle of the original 0/1/2 walk cycle.
    pub fn sample(
        &mut self,
        actors: impl IntoIterator<Item = (u16, (i32, i32), bool)>,
    ) -> Vec<Pose> {
        if self.pattern == Pattern::Off {
            self.clear();
            return Vec::new();
        }
        let mut next = BTreeMap::new();
        let out = actors
            .into_iter()
            .filter(|(slot, _, _)| *slot < 256)
            .map(|(slot, at, moving)| {
                let (before, distance) = self.previous.get(&slot).copied().unwrap_or((at, 0));
                let delta = at.0.abs_diff(before.0).max(at.1.abs_diff(before.1));
                let distance = if moving && delta <= 32 {
                    (distance + delta) % 16
                } else {
                    0
                };
                next.insert(slot, (at, distance));
                let phase = [1, 0, 1, 2][distance as usize / 4];
                let phase_in_step = f64::from(distance % 8) / 8.0;
                let lift = self.pattern.lift(phase_in_step);
                Pose {
                    slot,
                    phase,
                    lift,
                    offset: (0, 0),
                }
            })
            .collect();
        self.previous = next;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn footsteps_preview_curves_match_every_integer_pixel_in_a_step() {
        for (pattern, expected) in [
            (Pattern::A1, [0, 0, 1, 1, 1, 1, 1, 0, 0]),
            (Pattern::B2, [0, 0, 1, 2, 2, 2, 1, 0, 0]),
            (Pattern::C3, [0, 1, 2, 3, 3, 3, 2, 1, 0]),
        ] {
            for (distance, height) in expected.into_iter().enumerate() {
                assert_eq!(pattern.lift(distance as f64 / 8.0), height);
            }
        }
    }

    #[test]
    fn footsteps_follow_distance_with_synchronized_lift_and_settle_at_rest() {
        for pattern in [Pattern::A1, Pattern::B2, Pattern::C3] {
            let mut gait = Footsteps::default();
            gait.set_pattern(pattern);
            assert_eq!(
                gait.sample([(1, (0, 0), false)])[0],
                Pose {
                    slot: 1,
                    phase: 1,
                    lift: 0,
                    offset: (0, 0)
                }
            );
            let apex = gait.sample([(1, (4, 0), true)])[0];
            assert_eq!(apex.phase, 0);
            assert_eq!(apex.lift, i32::from(pattern.code()));
            assert_eq!(gait.sample([(1, (4, 0), true)])[0], apex);
            assert_eq!(
                gait.sample([(1, (8, 0), true)])[0],
                Pose {
                    slot: 1,
                    phase: 1,
                    lift: 0,
                    offset: (0, 0)
                }
            );
            assert_eq!(gait.sample([(1, (12, 0), true)])[0].phase, 2);
            assert_eq!(
                gait.sample([(1, (12, 0), false)])[0],
                Pose {
                    slot: 1,
                    phase: 1,
                    lift: 0,
                    offset: (0, 0)
                }
            );
            gait.set_pattern(Pattern::Off);
            assert!(gait.sample([(1, (16, 0), true)]).is_empty());
        }
    }
}

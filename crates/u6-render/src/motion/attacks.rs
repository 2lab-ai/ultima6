//! Display-only strike envelopes. No game position, turn, hit roll or damage is changed.
use std::collections::BTreeMap;

use super::footsteps::Pose;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strike {
    pub slot: u16,
    pub direction: (i8, i8),
    pub ranged: bool,
}

/// B: a brief body lunge; projectiles use a smaller recoil in the opposite direction.
/// Kept separate from the walking policy so either can be tuned or reverted independently.
pub const MELEE_PIXELS: i32 = 6;
pub const RECOIL_PIXELS: i32 = 3;
pub const MELEE_MS: f64 = 180.0;
pub const RECOIL_MS: f64 = 120.0;

#[derive(Default)]
pub struct Attacks(BTreeMap<u16, (Strike, f64, f64)>);

impl Attacks {
    pub fn active(&self, time: f64) -> bool {
        self.0
            .values()
            .any(|(_, start, duration)| time.is_finite() && time < *start + *duration)
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn start(&mut self, strike: Strike, time: f64, speed: u8) {
        if strike.slot >= 256 || !time.is_finite() || speed == 0 {
            return;
        }
        let duration = if strike.ranged { RECOIL_MS } else { MELEE_MS } / f64::from(speed);
        self.0.insert(strike.slot, (strike, time, duration));
    }

    /// Sample on the presentation clock and merge into copied draw poses. A repeat starts
    /// again from rest, and expired attacks leave no offset or persistent saved state.
    pub fn apply(&mut self, time: f64, poses: &mut Vec<Pose>) {
        self.0
            .retain(|_, (_, start, duration)| time.is_finite() && time < *start + *duration);
        for &(strike, start, duration) in self.0.values() {
            let phase = ((time - start) / duration).clamp(0.0, 1.0);
            // Quick impulse, slower return; exact rest at both endpoints.
            let envelope = if phase < 0.3 {
                phase / 0.3
            } else {
                (1.0 - phase) / 0.7
            };
            let distance = (envelope
                * f64::from(if strike.ranged {
                    -RECOIL_PIXELS
                } else {
                    MELEE_PIXELS
                }))
            .round() as i32;
            let offset = (
                i32::from(strike.direction.0.signum()) * distance,
                i32::from(strike.direction.1.signum()) * distance,
            );
            if offset == (0, 0) {
                continue;
            }
            if let Some(pose) = poses.iter_mut().find(|p| p.slot == strike.slot) {
                pose.offset = offset;
            } else {
                poses.push(Pose {
                    slot: strike.slot,
                    phase: 1,
                    lift: 0,
                    offset,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strike_moves_toward_target_and_recoil_away_then_both_return_to_rest() {
        for direction in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, -1)] {
            for ranged in [false, true] {
                let mut attacks = Attacks::default();
                let strike = Strike {
                    slot: 1,
                    direction,
                    ranged,
                };
                let ms = if ranged { RECOIL_MS } else { MELEE_MS };
                attacks.start(strike, 100.0, 1);
                let mut poses = Vec::new();
                attacks.apply(100.0, &mut poses);
                assert!(poses.is_empty());
                attacks.apply(100.0 + ms * 0.3, &mut poses);
                let px = if ranged { -RECOIL_PIXELS } else { MELEE_PIXELS };
                assert_eq!(
                    poses[0].offset,
                    (i32::from(direction.0) * px, i32::from(direction.1) * px)
                );
                poses.clear();
                attacks.apply(100.0 + ms, &mut poses);
                assert!(poses.is_empty());
            }
        }
    }

    #[test]
    fn strike_speed_restart_and_cancellation_are_display_only_and_bounded() {
        let mut attacks = Attacks::default();
        let strike = Strike {
            slot: 1,
            direction: (1, 0),
            ranged: false,
        };
        attacks.start(strike, 0.0, 2);
        attacks.start(strike, 10.0, 2);
        assert_eq!(attacks.0.len(), 1);
        let mut poses = Vec::new();
        attacks.apply(37.0, &mut poses);
        assert_eq!(poses[0].offset, (6, 0));
        attacks.clear();
        poses.clear();
        attacks.apply(40.0, &mut poses);
        assert!(poses.is_empty());
        attacks.start(strike, 0.0, 0);
        attacks.start(
            Strike {
                slot: 256,
                ..strike
            },
            0.0,
            1,
        );
        assert!(attacks.0.is_empty());
    }
}

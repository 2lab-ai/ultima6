//! Amiga 500 audio hardware for the Ultima VI port: Paula's four DMA channels, the CIA timer
//! that paces a music player's interrupt, CIA-A port A bit 1 (the audio filter switch) and the
//! A500 analogue output stage.
//!
//! This crate knows nothing about the game. A player (the game's MED routine, ported in
//! `u6-core::sound`) implements [`TimerInterrupt`]; the host pulls stereo samples with
//! [`Machine::render`], which runs the hardware for the time those samples cover and calls the
//! interrupt at each timer underflow, exactly where the CIA would raise it.
//!
//! Timing: PAL colour clock 3,546,895 Hz (Paula's period unit), CIA E clock = colour clock / 5.
//! Output: each sample is the average of the DAC level over its time span (a box filter), then
//! the selected [`FilterModel`]. Channels 0 and 3 go left, 1 and 2 right. Platform-free: no
//! I/O, no clocks, no threads; the same writes and sample rate give the same samples.

mod cia;
mod filter;
mod paula;

#[cfg(test)]
mod tests;

pub use cia::{CiaTimer, E_CLOCK_DIVIDER};
pub use filter::FilterModel;
pub use paula::{Channel, Paula};

/// PAL colour clock (Hz): Paula's period unit.
pub const PAL_CLOCK: u32 = 3_546_895;

/// Full-scale level of one channel: sample (−128..127) × volume (0..64).
const CHANNEL_FULL: f64 = 128.0 * 64.0;

/// The player side of the CIA interrupt.
pub trait TimerInterrupt {
    /// Runs the interrupt handler; it may write any register of `machine`.
    fn timer_interrupt(&mut self, machine: &mut Machine);
}

/// The audio part of the machine: chip memory, Paula, one CIA timer, the filter switch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Machine {
    /// Chip memory Paula fetches from (addresses are offsets into it).
    pub chip: Vec<u8>,
    pub paula: Paula,
    /// CIA-B timer B (the player's interrupt source).
    pub timer: CiaTimer,
    /// CIA-A port A bit 1 clear: the LED is on and so is the A500 audio filter.
    pub led_filter: bool,
    /// Output stage model.
    pub filter: FilterModel,
    output: filter::Output,
    /// Colour clocks owed to the next sample (fraction of PAL_CLOCK / rate, in units of 1/rate).
    remainder: u64,
}

impl Machine {
    /// A machine with `chip_size` bytes of zeroed chip memory.
    pub fn new(chip_size: usize) -> Self {
        Self {
            chip: vec![0; chip_size],
            ..Self::default()
        }
    }

    /// CIA-A port A bit 1 as the program writes it (set = LED off = filter off).
    pub fn write_led_bit(&mut self, set: bool) {
        self.led_filter = !set;
    }

    /// Fills `out` (interleaved stereo f32, −1..1) at `sample_rate`, running the hardware and
    /// `irq` for the time the samples cover.
    pub fn render<I: TimerInterrupt>(&mut self, irq: &mut I, out: &mut [f32], sample_rate: u32) {
        self.render_taps(irq, out, &mut [], sample_rate);
    }

    /// [`Machine::render`], also writing each channel's DAC level before the output stage into
    /// `taps` (4 values per stereo frame, −1..1) when `taps` is long enough.
    pub fn render_taps<I: TimerInterrupt>(
        &mut self,
        irq: &mut I,
        out: &mut [f32],
        taps: &mut [f32],
        sample_rate: u32,
    ) {
        if sample_rate == 0 {
            return;
        }
        let rate = u64::from(sample_rate);
        let with_taps = taps.len() >= out.len() / 2 * 4;
        for (n, frame) in out.chunks_exact_mut(2).enumerate() {
            let owed = self.remainder + u64::from(PAL_CLOCK);
            let clocks = owed / rate;
            self.remainder = owed % rate;
            let mut sums = [0i64; 4];
            let (l, r) = self.run(irq, clocks, &mut sums);
            let span = clocks.max(1) as f64;
            let side = 2.0 * CHANNEL_FULL * span;
            let (l, r) = self.output.run(
                self.filter,
                sample_rate,
                self.led_filter,
                (l as f64 / side, r as f64 / side),
            );
            frame[0] = l as f32;
            frame[1] = r as f32;
            if with_taps {
                for (c, s) in sums.iter().enumerate() {
                    taps[n * 4 + c] = (*s as f64 / (CHANNEL_FULL * span)) as f32;
                }
            }
        }
    }

    /// Runs `clocks` colour clocks, stopping at each timer underflow for the interrupt.
    fn run<I: TimerInterrupt>(&mut self, irq: &mut I, clocks: u64, taps: &mut [i64]) -> (i64, i64) {
        let (mut l, mut r) = (0i64, 0i64);
        let mut left = clocks;
        while left > 0 {
            let step = self
                .timer
                .clocks_to_underflow()
                .map_or(left, |c| c.min(left));
            let (dl, dr) = self.paula.advance(step as u32, &self.chip, taps);
            l += dl;
            r += dr;
            left -= step;
            if self.timer.advance(step) {
                irq.timer_interrupt(self);
            }
        }
        (l, r)
    }
}

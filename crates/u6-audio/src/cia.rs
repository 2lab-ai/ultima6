//! The CIA pieces the game's music player touches: one 16-bit interval timer (CIA-B timer B,
//! or timer A when B is taken; same behaviour) and CIA-A port A bit 1 (the power LED, which
//! also switches the A500's audio filter).
//!
//! The timer counts E-clock cycles (colour clock / 5 = 709,379 Hz on PAL). In continuous mode
//! it counts down from its latch and interrupts on the underflow after 0, then reloads: one
//! interrupt every latch + 1 E cycles. Writing the high latch byte while the timer is stopped
//! also loads the counter (6526 behaviour); while it runs, a new latch is used from the next
//! reload on.

/// Colour clocks per E-clock cycle.
pub const E_CLOCK_DIVIDER: u64 = 5;

/// A CIA interval timer in continuous mode.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CiaTimer {
    latch: u16,
    counter: u16,
    running: bool,
    /// Colour clocks into the current E cycle (0..5).
    phase: u64,
}

impl CiaTimer {
    /// Write of the low latch byte (TxLO).
    pub fn write_lo(&mut self, value: u8) {
        self.latch = (self.latch & 0xff00) | u16::from(value);
    }

    /// Write of the high latch byte (TxHI); loads the counter while the timer is stopped.
    pub fn write_hi(&mut self, value: u8) {
        self.latch = (self.latch & 0x00ff) | u16::from(value) << 8;
        if !self.running {
            self.counter = self.latch;
        }
    }

    /// Write of the control register (CRA/CRB): bit 0 = START; RUNMODE (bit 3) is taken as 0
    /// (continuous), LOAD (bit 4) loads the counter from the latch.
    pub fn write_control(&mut self, value: u8) {
        if value & 0x10 != 0 {
            self.counter = self.latch;
        }
        self.running = value & 1 != 0;
    }

    /// The control register as last written (START bit only).
    pub fn control(&self) -> u8 {
        u8::from(self.running)
    }

    /// The latch value.
    pub fn latch(&self) -> u16 {
        self.latch
    }

    /// Whether the timer counts.
    pub fn running(&self) -> bool {
        self.running
    }

    /// Colour clocks until the next underflow (None while stopped).
    pub fn clocks_to_underflow(&self) -> Option<u64> {
        self.running
            .then(|| (u64::from(self.counter) + 1) * E_CLOCK_DIVIDER - self.phase)
    }

    /// Advances `clocks` colour clocks, which must not pass the next underflow; returns
    /// whether it was reached (the counter has then been reloaded).
    pub fn advance(&mut self, clocks: u64) -> bool {
        if !self.running {
            return false;
        }
        let total = self.phase + clocks;
        let cycles = total / E_CLOCK_DIVIDER;
        self.phase = total % E_CLOCK_DIVIDER;
        let left = u64::from(self.counter) + 1;
        if cycles >= left {
            self.counter = self.latch;
            self.phase = 0;
            true
        } else {
            self.counter -= cycles as u16;
            false
        }
    }
}

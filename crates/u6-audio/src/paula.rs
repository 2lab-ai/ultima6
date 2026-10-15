//! Paula's four audio DMA channels (custom registers AUD0..AUD3 at 0xDFF0A0 + 0x10 n, DMACON
//! 0xDFF096).
//!
//! Each channel plays 8-bit signed samples from chip memory: on DMA start it copies AUDxLC into
//! its pointer and AUDxLEN (words, 0 = 65536) into its word counter, fetches one word at a time
//! and outputs the high byte, then the low byte, holding each for AUDxPER colour clocks. After the
//! last word of a block it copies AUDxLC / AUDxLEN again, so whatever the program wrote there
//! after the start is the loop. Volume is 0..64 (bit 6 alone means 64). The program's register
//! writes take effect at once; a period write is picked up at the next sample, as on the chip.
//! DMACON's master enable (bit 9) is taken as set: the OS keeps it on and the game never clears
//! it. A channel whose DMA is off outputs 0.

/// One channel: the registers the program writes and the state the DMA engine keeps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Channel {
    /// AUDxLC: start address of the next block (chip memory offset).
    pub location: u32,
    /// AUDxLEN: block length in words (0 = 65536).
    pub length: u16,
    /// AUDxPER: colour clocks per sample.
    pub period: u16,
    /// AUDxVOL: volume as written.
    pub volume: u16,
    on: bool,
    pointer: u32,
    words_left: u32,
    word: u16,
    low_next: bool,
    /// Colour clocks until the next sample; 0 = fetch now.
    countdown: u32,
    sample: i8,
}

impl Channel {
    /// The volume Paula applies: bits 0..5, or 64 when bit 6 is set.
    pub fn effective_volume(&self) -> i32 {
        if self.volume & 0x40 != 0 {
            64
        } else {
            i32::from(self.volume & 0x3f)
        }
    }

    /// Whether the channel's DMA is on.
    pub fn playing(&self) -> bool {
        self.on
    }

    /// The sample byte being output (0 while DMA is off).
    pub fn sample(&self) -> i8 {
        if self.on { self.sample } else { 0 }
    }

    fn start(&mut self) {
        self.on = true;
        self.reload();
        self.low_next = false;
        self.countdown = 0;
    }

    fn reload(&mut self) {
        self.pointer = self.location;
        self.words_left = if self.length == 0 {
            0x1_0000
        } else {
            u32::from(self.length)
        };
    }

    fn next_sample(&mut self, chip: &[u8]) {
        if self.low_next {
            self.sample = self.word.to_be_bytes()[1] as i8;
            self.low_next = false;
        } else {
            let at = |a: u32| chip.get(a as usize).copied().unwrap_or(0);
            self.word = u16::from_be_bytes([at(self.pointer), at(self.pointer.wrapping_add(1))]);
            self.pointer = self.pointer.wrapping_add(2);
            self.words_left -= 1;
            if self.words_left == 0 {
                self.reload();
            }
            self.sample = self.word.to_be_bytes()[0] as i8;
            self.low_next = true;
        }
        self.countdown = u32::from(self.period).max(1);
    }

    /// Advances `clocks` colour clocks; returns the integral of sample × volume over them.
    fn advance(&mut self, mut clocks: u32, chip: &[u8]) -> i64 {
        if !self.on {
            return 0;
        }
        let mut sum = 0i64;
        while clocks > 0 {
            if self.countdown == 0 {
                self.next_sample(chip);
            }
            let step = clocks.min(self.countdown);
            let level = i64::from(self.sample) * i64::from(self.effective_volume());
            sum += level * i64::from(step);
            self.countdown -= step;
            clocks -= step;
        }
        sum
    }
}

/// The four channels and DMACON.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Paula {
    pub channels: [Channel; 4],
}

impl Paula {
    /// A DMACON write: bit 15 set = enable the channels in bits 0..3, clear = disable them. A
    /// channel switched on loads its pointer and counter; one already on is not restarted.
    pub fn write_dmacon(&mut self, value: u16) {
        let set = value & 0x8000 != 0;
        for (n, ch) in self.channels.iter_mut().enumerate() {
            if value & (1 << n) == 0 {
                continue;
            }
            if !set {
                ch.on = false;
            } else if !ch.on {
                ch.start();
            }
        }
    }

    /// DMACON's channel bits as the program would read them back (DMACONR bits 0..3).
    pub fn dma_bits(&self) -> u16 {
        self.channels
            .iter()
            .enumerate()
            .filter(|(_, c)| c.on)
            .fold(0, |bits, (n, _)| bits | 1 << n)
    }

    /// Advances every channel; returns (left, right) integrals: channels 0 and 3 left, 1 and 2
    /// right, as the Amiga wires them. `taps` (if 4 long) receives each channel's integral.
    pub(crate) fn advance(&mut self, clocks: u32, chip: &[u8], taps: &mut [i64]) -> (i64, i64) {
        let mut v = [0i64; 4];
        for (n, ch) in self.channels.iter_mut().enumerate() {
            v[n] = ch.advance(clocks, chip);
        }
        if let Some(t) = taps.get_mut(..4) {
            for (dst, src) in t.iter_mut().zip(v) {
                *dst += src;
            }
        }
        (v[0] + v[3], v[1] + v[2])
    }
}

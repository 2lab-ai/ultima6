//! An MMD0 (OctaMED) module in the player's memory, read the way the MED routine reads it.
//!
//! The original relocates a module once after loading it (0x15355a: every stored pointer gets
//! the module's address added, synth waveform pointers the instrument's address) and from then
//! on follows plain pointers. The port keeps the module bytes as stored and adds the base at
//! each access, which is the same address. Field offsets are the MMD0 format's (OctaMED
//! `MMD0`, `MMD0song`, `MMD0block`, `InstrHdr`, `SynthInstr`, `MMD0exp`) as the routine uses
//! them; every read is bounds-checked and reads 0 outside memory, where the original would read
//! whatever is there. Addresses are formed with [`addr`], which wraps: a malformed module's
//! pointer can hold any value.

/// `base + off` as the 68000 forms an address from a register and an offset (32 bits, wrapping;
/// 0x100fa4 reads `$301(a0)` of whatever the song pointer holds). A plain `+` here would give
/// the same bits in a release build and panic in a debug one; the lint makes the gate's clippy
/// run reject it in every profile.
#[deny(clippy::arithmetic_side_effects)]
pub fn addr(base: u32, off: u32) -> u32 {
    base.wrapping_add(off)
}

/// Byte reader over the player's memory (chip memory of the audio machine).
#[derive(Clone, Copy, Debug)]
pub struct Mem<'a>(pub &'a [u8]);

impl Mem<'_> {
    pub fn b(&self, at: u32) -> u8 {
        self.0.get(at as usize).copied().unwrap_or(0)
    }

    pub fn w(&self, at: u32) -> u16 {
        u16::from_be_bytes([self.b(at), self.b(addr(at, 1))])
    }

    pub fn l(&self, at: u32) -> u32 {
        u32::from(self.w(at)) << 16 | u32::from(self.w(addr(at, 2)))
    }
}

/// MMD0 header: song pointer.
const MMD_SONG: u32 = 0x08;
/// MMD0 header: block array pointer.
const MMD_BLOCKARR: u32 = 0x10;
/// MMD0 header: sample array pointer.
const MMD_SMPLARR: u32 = 0x18;
/// MMD0 header: expansion data pointer.
const MMD_EXPDATA: u32 = 0x20;

/// `MMD0song` offsets (after the 63 eight-byte instrument entries).
pub mod song {
    pub const NUMBLOCKS: u32 = 0x1f8;
    pub const SONGLEN: u32 = 0x1fa;
    pub const PLAYSEQ: u32 = 0x1fc;
    pub const DEFTEMPO: u32 = 0x2fc;
    pub const PLAYTRANSP: u32 = 0x2fe;
    /// Bit 0 filter on, bit 4 hex volumes, bit 5 no effects on a line's first tick.
    pub const FLAGS: u32 = 0x2ff;
    pub const TEMPO2: u32 = 0x301;
    pub const TRKVOL: u32 = 0x302;
    pub const MASTERVOL: u32 = 0x312;
    pub const NUMSAMPLES: u32 = 0x313;
}

/// `InstrHdr` / `SynthInstr` offsets (the sample array's targets).
pub mod instr {
    pub const LENGTH: u32 = 0;
    /// 0 sample, 1..3 IFF multi-octave, −1 synth, −2 hybrid.
    pub const TYPE: u32 = 4;
    pub const DATA: u32 = 6;
    pub const SPEEDS: u32 = 0x12;
    pub const WFORMS: u32 = 0x14;
    pub const VOLTBL: u32 = 0x16;
    pub const WFTBL: u32 = 0x96;
    pub const WF: u32 = 0x116;
}

/// A module at address `base` of the player's memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Module {
    pub base: u32,
}

impl Module {
    /// A stored pointer of the module, made absolute (a stored 0 stays 0, as in 0x153550).
    pub(super) fn ptr(&self, m: Mem, at: u32) -> u32 {
        match m.l(at) {
            0 => 0,
            off => addr(self.base, off),
        }
    }

    /// The song structure (`MMD0song`).
    pub fn song(&self, m: Mem) -> u32 {
        self.ptr(m, addr(self.base, MMD_SONG))
    }

    /// Block `n` (`MMD0block`: tracks, last line index, then 3 bytes per note).
    pub fn block(&self, m: Mem, n: u16) -> u32 {
        let arr = self.ptr(m, addr(self.base, MMD_BLOCKARR));
        self.ptr(m, addr(arr, u32::from(n) * 4))
    }

    /// Instrument `n`'s data (0 = none).
    pub fn sample(&self, m: Mem, n: u8) -> u32 {
        let arr = self.ptr(m, addr(self.base, MMD_SMPLARR));
        self.ptr(m, addr(arr, u32::from(n) * 4))
    }

    /// The expansion structure (0 = none).
    pub fn expdata(&self, m: Mem) -> u32 {
        self.ptr(m, addr(self.base, MMD_EXPDATA))
    }

    /// `MMD0exp.exp_smp` (instrument extensions: hold, decay), 0 = none.
    pub fn exp_samples(&self, m: Mem, exp: u32) -> u32 {
        self.ptr(m, addr(exp, 4))
    }

    /// Waveform `n` of the synth or hybrid instrument at `instr` (relative to the instrument).
    pub fn waveform(m: Mem, instr: u32, n: u8) -> u32 {
        match m.l(addr(instr, instr::WF + u32::from(n) * 4)) {
            0 => 0,
            off => addr(instr, off),
        }
    }
}

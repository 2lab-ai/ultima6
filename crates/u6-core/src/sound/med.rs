//! The MED replay routine linked into hunk 0 ("MED - Player routine", OctaMED's player for
//! MMD0 modules, with synth sounds): its globals, entry points and the CIA tempo.
//!
//! The routine runs from a CIA-B timer interrupt ("MEDTimerInterrupt", 0x10118a, code 0x100890)
//! and talks to Paula and the CIAs directly; in the port it drives [`u6_audio::Machine`], which
//! calls [`MedPlayer::timer_interrupt`] at each timer underflow. Addresses below are the chip
//! memory of that machine (the module, the null sample).

use u6_audio::{CiaTimer, Machine, Paula};

use super::module::{Mem, Module};
use super::tables::MedTables;

/// The 4 zero bytes a one-shot sample repeats after it ends (the original's 0x1535d0; any
/// nonzero address of zeros: 0x10060c skips a repeat pointer of 0).
pub const NULL_SAMPLE: u32 = 4;
/// Tracks the routine keeps state for (0x10064a: 4 Amiga channels of 0x4a bytes, then 12 MIDI
/// tracks of 0x12 bytes; only the first 4 reach Paula).
pub const TRACKS: usize = 16;
/// Amiga channels.
pub const CHANNELS: usize = 4;
/// 0x100e2e value while a song plays.
pub const PLAYING_SONG: u16 = 2;

/// One track's state (the structure at 0x10064a + 0x4a n; offsets in the field docs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    /// +0x00 last note (1..63)
    pub note: u8,
    /// +0x01 instrument (0-based)
    pub instr: u8,
    /// +0x02 volume 0..64
    pub volume: u8,
    /// +0x04 / +0x05 command and argument of the current line
    pub cmd: u8,
    pub arg: u8,
    /// +0x07 hold countdown in ticks (negative = not held)
    pub hold_count: i8,
    /// +0x08 / +0x09 the instrument's hold and decay (from the module's extension, or cmd 8)
    pub hold: u8,
    pub decay: u8,
    /// +0x0a the instrument's transpose
    pub transpose: u8,
    /// +0x0c address of the instrument's song entry (rep, replen, ..., svol, strans)
    pub instr_entry: u32,
    /// +0x10 track volume × master volume / 16
    pub track_vol: u16,
    /// +0x12 current period
    pub period: u16,
    /// +0x18 / +0x1c repeat start and length (words) set after DMA starts (0 = none)
    pub repeat: u32,
    pub repeat_len: u16,
    /// +0x1e portamento target period (0 = none), +0x46 its speed
    pub porta_target: u16,
    pub porta_speed: u8,
    /// +0x20 vibrato phase, +0x21 vibrato command (speed << 4 | depth)
    pub vib_pos: u8,
    pub vib_cmd: u8,
    /// +0x22 synth or hybrid instrument (0 = none)
    pub synth: u32,
    /// +0x26 / +0x28 synth arpeggio position and start
    pub arp_pos: u16,
    pub arp_start: u16,
    /// +0x2a / +0x2b volume and waveform sequence countdowns
    pub vol_count: u8,
    pub wf_count: u8,
    /// +0x2c / +0x2e volume and waveform sequence positions
    pub vol_pos: u16,
    pub wf_pos: u16,
    /// +0x30 / +0x31 sequence waits
    pub vol_wait: u8,
    pub wf_wait: u8,
    /// +0x32 synth vibrato speed, +0x3c phase, +0x3e depth
    pub vib_speed: u16,
    pub vib_phase: u16,
    pub vib_depth: u16,
    /// +0x34 synth period change per step, +0x3a accumulated offset
    pub period_change: u16,
    pub period_offset: u16,
    /// +0x36 synth volume change per step (signed)
    pub vol_change: u8,
    /// +0x37 synth note (0-based, transposed)
    pub synth_note: u8,
    /// +0x38 / +0x39 sequence speeds
    pub vol_speed: u8,
    pub wf_speed: u8,
    /// +0x40 synth volume
    pub synth_vol: u8,
    /// +0x41 0 = sample, 1 = synth, 0xff = hybrid
    pub synth_type: u8,
    /// +0x42 period table in use: index into [`MedTables::periods`] (0 = synth, 24 = sample)
    pub period_table: u16,
    /// +0x48 decay to start when a hold ends, +0x49 decay running (volume step per tick)
    pub release: u8,
    pub fade: u8,
}

/// The routine's globals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MedPlayer {
    pub tables: MedTables,
    pub tracks: [Track; TRACKS],
    /// 0x101186 module playing (None = 0)
    pub module: Option<Module>,
    /// 0x10088a / 0x10088c tracks and last line of the block being read
    pub numtracks: u16,
    pub lines: u16,
    /// 0x10088e tick within the line
    pub counter: u8,
    /// 0x10088f: >0 = F00 (next position), 0xff = B (jump to `pseq`)
    pub next_block: u8,
    /// 0x100648 channels to start at the end of the interrupt
    pub dma_pending: u16,
    /// 0x100e28 / 0x100e2a / 0x100e2c / 0x100e2e line, block, play sequence position, state
    pub line: u16,
    pub block: u16,
    pub pseq: u16,
    pub state: u16,
    /// 0x100e6c timer divisor
    pub timer_div: u32,
    /// 0x101174 player set up (2 = CIA-B timer B)
    pub initialized: u8,
    /// 0x101308 hold and decay per instrument (63 each)
    pub hold: [u8; 63],
    pub decay: [u8; 63],
}

/// The machine registers one interrupt touches.
pub(super) struct Hw<'a> {
    pub chip: &'a mut [u8],
    pub paula: &'a mut Paula,
    pub timer: &'a mut CiaTimer,
    pub led_filter: &'a mut bool,
}

impl<'a> Hw<'a> {
    pub fn of(m: &'a mut Machine) -> Self {
        Hw {
            chip: &mut m.chip,
            paula: &mut m.paula,
            timer: &mut m.timer,
            led_filter: &mut m.led_filter,
        }
    }

    pub fn mem(&self) -> Mem<'_> {
        Mem(self.chip)
    }

    /// Memory to read and Paula to write at the same time.
    pub fn split(&mut self) -> (Mem<'_>, &mut Paula) {
        (Mem(self.chip), self.paula)
    }

    pub fn set_byte(&mut self, addr: u32, v: u8) {
        if let Some(b) = self.chip.get_mut(addr as usize) {
            *b = v;
        }
    }

    /// DMACON write (bit 15 = set).
    pub fn dmacon(&mut self, v: u16) {
        self.paula.write_dmacon(v);
    }

    pub fn location(&mut self, ch: usize, v: u32) {
        if let Some(c) = self.paula.channels.get_mut(ch) {
            c.location = v;
        }
    }

    pub fn length(&mut self, ch: usize, v: u16) {
        if let Some(c) = self.paula.channels.get_mut(ch) {
            c.length = v;
        }
    }

    pub fn period(&mut self, ch: usize, v: u16) {
        if let Some(c) = self.paula.channels.get_mut(ch) {
            c.period = v;
        }
    }

    pub fn volume(&mut self, ch: usize, v: u8) {
        if let Some(c) = self.paula.channels.get_mut(ch) {
            c.volume = u16::from(v);
        }
    }

    /// CIA-A port A bit 1 (set = LED off = filter off).
    pub fn led_bit(&mut self, set: bool) {
        *self.led_filter = !set;
    }
}

impl MedPlayer {
    /// The routine's state as the image holds it before 0x100f7c runs.
    pub fn new(tables: MedTables) -> Self {
        let timer_div = tables.timer_div;
        Self {
            tables,
            tracks: Default::default(),
            module: None,
            numtracks: 0,
            lines: 0,
            counter: 0,
            next_block: 0,
            dma_pending: 0,
            line: 0,
            block: 0,
            pseq: 0,
            state: 0,
            timer_div,
            initialized: 0,
            hold: [0; 63],
            decay: [0; 63],
        }
    }
}

//! Unit tests on synthetic data: tables computed here (not the game's), modules built here.

mod med;
mod songs;
mod synth;

use u6_audio::Machine;

use super::driver::CHIP_BUFFER;
use super::med::MedPlayer;
use super::tables::MedTables;

/// Synthetic player tables: falling periods, a computed sine, made-up timer values.
pub(super) fn tables() -> MedTables {
    let mut periods = [0u16; 96];
    for (i, p) in periods.iter_mut().enumerate() {
        *p = (6848.0 / 2f64.powf(i as f64 / 12.0)).round() as u16;
    }
    let mut sine = [0i8; 32];
    for (i, s) in sine.iter_mut().enumerate() {
        *s = (127.0 * (i as f64 * std::f64::consts::PI / 16.0).sin()).round() as i8;
    }
    MedTables {
        periods,
        sine,
        st_tempos: [1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 10000],
        timer_div: 600_000,
        octave_shift: [4, 3, 2, 1, 1, 0, 2, 2, 1, 1, 0, 0],
        octave_mult: [15, 7, 3, 1, 1, 0, 3, 3, 1, 1, 0, 0],
        octave_note: [12, 12, 12, 12, 24, 24, 0, 12, 12, 24, 24, 36],
    }
}

/// One instrument of a synthetic module.
pub(super) struct Inst {
    pub data: Vec<u8>,
    /// Type word (0 sample, -1 synth).
    pub kind: i16,
    pub rep: u16,
    pub replen: u16,
    pub transpose: i8,
}

impl Inst {
    pub fn sample(len: usize, rep: u16, replen: u16) -> Self {
        Self {
            data: (0..len).map(|i| (i as u8).wrapping_mul(7)).collect(),
            kind: 0,
            rep,
            replen,
            transpose: 0,
        }
    }
}

/// A synthetic MMD0: 4-track blocks of (note byte, instr/cmd byte, arg) cells.
pub(super) struct Mmd {
    pub blocks: Vec<Vec<[[u8; 3]; 4]>>,
    pub insts: Vec<Inst>,
    pub playseq: Vec<u8>,
    pub tempo: u16,
    pub tempo2: u8,
    pub flags: u8,
    pub transpose: i8,
    /// (hold, decay) per instrument in an expansion block.
    pub ext: Vec<(u8, u8)>,
}

impl Mmd {
    pub fn new(blocks: Vec<Vec<[[u8; 3]; 4]>>) -> Self {
        let n = blocks.len() as u8;
        Self {
            blocks,
            insts: vec![Inst::sample(64, 0, 0)],
            playseq: (0..n).collect(),
            tempo: 125,
            tempo2: 3,
            flags: 0,
            transpose: 0,
            ext: Vec::new(),
        }
    }

    pub fn build(&self) -> Vec<u8> {
        let mut m = vec![0u8; 0x348];
        m[..4].copy_from_slice(b"MMD0");
        let put32 =
            |m: &mut Vec<u8>, at: usize, v: u32| m[at..at + 4].copy_from_slice(&v.to_be_bytes());
        let put16 =
            |m: &mut Vec<u8>, at: usize, v: u16| m[at..at + 2].copy_from_slice(&v.to_be_bytes());
        put32(&mut m, 8, 0x34);
        let s = 0x34;
        for (i, inst) in self.insts.iter().enumerate() {
            put16(&mut m, s + i * 8, inst.rep);
            put16(&mut m, s + i * 8 + 2, inst.replen);
            m[s + i * 8 + 6] = 64;
            m[s + i * 8 + 7] = inst.transpose as u8;
        }
        put16(&mut m, s + 0x1f8, self.blocks.len() as u16);
        put16(&mut m, s + 0x1fa, self.playseq.len() as u16);
        m[s + 0x1fc..s + 0x1fc + self.playseq.len()].copy_from_slice(&self.playseq);
        put16(&mut m, s + 0x2fc, self.tempo);
        m[s + 0x2fe] = self.transpose as u8;
        m[s + 0x2ff] = self.flags;
        m[s + 0x301] = self.tempo2;
        m[s + 0x302..s + 0x312].fill(64);
        m[s + 0x312] = 64;
        m[s + 0x313] = self.insts.len() as u8;
        let blockarr = m.len();
        put32(&mut m, 0x10, blockarr as u32);
        m.resize(blockarr + 4 * self.blocks.len(), 0);
        let smplarr = m.len();
        put32(&mut m, 0x18, smplarr as u32);
        m.resize(smplarr + 4 * self.insts.len(), 0);
        for (k, b) in self.blocks.iter().enumerate() {
            let at = m.len();
            put32(&mut m, blockarr + 4 * k, at as u32);
            m.extend([4, (b.len() - 1) as u8]);
            for row in b {
                for cell in row {
                    m.extend(cell);
                }
            }
        }
        for (i, inst) in self.insts.iter().enumerate() {
            let at = m.len();
            put32(&mut m, smplarr + 4 * i, at as u32);
            m.extend((inst.data.len() as u32).to_be_bytes());
            m.extend(inst.kind.to_be_bytes());
            m.extend(&inst.data);
            if m.len() % 2 == 1 {
                m.push(0);
            }
        }
        if !self.ext.is_empty() {
            let exp = m.len();
            put32(&mut m, 0x20, exp as u32);
            m.resize(exp + 16, 0);
            let es = m.len();
            put32(&mut m, exp + 4, es as u32);
            put16(&mut m, exp + 8, self.ext.len() as u16);
            put16(&mut m, exp + 10, 2);
            for &(h, d) in &self.ext {
                m.extend([h, d]);
            }
        }
        let len = m.len() as u32;
        put32(&mut m, 4, len);
        m
    }
}

/// A note cell: note 1..63, instrument 1..15, command, argument.
pub(super) fn cell(note: u8, instr: u8, cmd: u8, arg: u8) -> [u8; 3] {
    [note, instr << 4 | cmd, arg]
}

pub(super) const EMPTY: [u8; 3] = [0, 0, 0];

/// A player set up (InitPlayer) with `module` in chip memory, PlayModule called.
pub(super) fn playing(module: &Mmd) -> (MedPlayer, Machine) {
    let bytes = module.build();
    let mut m = Machine::new(CHIP_BUFFER as usize + bytes.len() + 64);
    m.chip[CHIP_BUFFER as usize..CHIP_BUFFER as usize + bytes.len()].copy_from_slice(&bytes);
    let mut p = MedPlayer::new(tables());
    p.init(&mut m);
    p.play(&mut m, CHIP_BUFFER);
    (p, m)
}

/// Runs `n` timer interrupts.
pub(super) fn ticks(p: &mut MedPlayer, m: &mut Machine, n: usize) {
    for _ in 0..n {
        u6_audio::TimerInterrupt::timer_interrupt(p, m);
    }
}

/// Address of instrument `i`'s sample data in chip memory (after its 6-byte header).
pub(super) fn sample_data(m: &Machine, i: usize) -> u32 {
    let mem = super::module::Mem(&m.chip);
    let md = super::module::Module { base: CHIP_BUFFER };
    md.sample(mem, i as u8) + 6
}

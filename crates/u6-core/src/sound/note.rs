//! Starting a note on a track: instrument lookup, transpose, sample or synth set-up, Paula's
//! start registers (the DMA itself starts at the end of the interrupt).

use super::med::{CHANNELS, Hw, MedPlayer, NULL_SAMPLE};
use super::module::{Module, addr, instr, song};
use super::tables::SAMPLE_PERIODS;

/// What 0x100e70 hands back for a sample note.
struct SampleNote {
    start: u32,
    /// D1 as a long: byte length (IFF octaves: the low word is the octave's length, the high
    /// word the division's remainder, which the caller's `lsr.l` shifts in).
    length: u32,
    rep: u16,
    replen: u16,
    period: u16,
}

impl MedPlayer {
    /// Original: 0x100234 (sound: MED play a note — instrument, transpose, DMA off, registers)
    ///
    /// `note` 1..63 as in the block, `vol` 0..64, `instr` 0-based, `entry` = the instrument's
    /// song entry. Tracks 4..15 (MIDI in OctaMED) produce nothing.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn play_note(
        &mut self,
        hw: &mut Hw,
        md: Module,
        ch: usize,
        note: u8,
        vol: u8,
        instr_no: u8,
        entry: u32,
    ) {
        let bit = 1u16 << (ch & 15);
        let mem = hw.mem();
        let sample = md.sample(mem, instr_no);
        if sample == 0 {
            return;
        }
        let s = md.song(mem);
        let note = note
            .wrapping_add(mem.b(addr(s, song::PLAYTRANSP)))
            .wrapping_add(mem.b(addr(entry, 7)));
        let kind = mem.w(addr(sample, instr::TYPE)) as i16;
        if ch >= CHANNELS {
            return;
        }
        // A synth note following a synth note keeps its DMA running.
        if !((self.tracks[ch].synth_type as i8) > 0 && kind == -1) {
            hw.dmacon(bit);
        }
        let t = &mut self.tracks[ch];
        t.synth = 0;
        t.fade = 0;
        t.release = t.decay;
        t.vib_pos = 0;
        self.dma_pending |= bit;
        let note = note.wrapping_sub(1);
        if kind >= 0 {
            self.tracks[ch].synth_type = 0;
            self.sample_note(hw, ch, sample, entry, note, vol);
            return;
        }
        let t = &mut self.tracks[ch];
        t.synth_note = note;
        t.synth = sample;
        if kind == -2 {
            // Hybrid: waveform 0 is a plain sample, the sequences run on top of it.
            t.synth_type = 0xff;
            let wf = Module::waveform(hw.mem(), sample, 0);
            self.sample_note(hw, ch, wf, entry, note, vol);
            return;
        }
        t.synth_type = 1;
        t.period_table = 0;
        let period = self.period_at(0, u16::from(note));
        let t = &mut self.tracks[ch];
        t.synth_vol = t.volume;
        t.period = period;
        t.repeat = 0;
        hw.period(ch, period);
        self.synth_init(hw, ch);
    }

    /// The sample half of 0x100234 (0x1002a8..0x100320): octave wrap, start, length, repeat.
    fn sample_note(&mut self, hw: &mut Hw, ch: usize, sample: u32, entry: u32, note: u8, vol: u8) {
        let mut note = note;
        while (note as i8) < 0 {
            note = note.wrapping_add(12);
        }
        if (note as i8) > 0x3e {
            note -= 12;
        }
        let sn = self.sample_params(hw, ch, sample, entry, note);
        hw.volume(ch, vol);
        hw.location(ch, sn.start);
        let t = &mut self.tracks[ch];
        if sn.replen <= 1 {
            t.repeat = NULL_SAMPLE;
            t.repeat_len = 1;
            hw.length(ch, (sn.length >> 1) as u16);
        } else {
            hw.length(ch, if sn.rep != 0 { sn.rep } else { sn.replen });
            t.repeat = sn.start.wrapping_add(u32::from(sn.rep) << 1);
            t.repeat_len = sn.replen;
        }
        hw.period(ch, sn.period);
        t.period = sn.period;
        if t.synth_type != 0 {
            self.synth_init(hw, ch);
        }
    }

    /// Original: 0x100e70 (sound: MED sample start, length, loop and period for a note)
    ///
    /// Plain samples use the sample period table and set it as the track's (`$42`); IFF
    /// multi-octave samples (type 1..3; 2 = 3 octaves, the others 5) pick the octave's part.
    fn sample_params(
        &mut self,
        hw: &Hw,
        ch: usize,
        sample: u32,
        entry: u32,
        note: u8,
    ) -> SampleNote {
        let mem = hw.mem();
        let kind = mem.w(addr(sample, instr::TYPE));
        let (rep, replen) = (mem.w(entry), mem.w(addr(entry, 2)));
        if kind == 0 {
            self.tracks[ch].period_table = SAMPLE_PERIODS;
            return SampleNote {
                start: addr(sample, instr::DATA),
                length: mem.l(addr(sample, instr::LENGTH)),
                rep,
                replen,
                period: self.period_at(SAMPLE_PERIODS, u16::from(note.wrapping_mul(2) / 2)),
            };
        }
        self.octave_params(mem.l(sample), kind, sample, rep, replen, note)
    }

    /// Original: 0x100e96 (sound: MED IFF multi-octave sample part for a note)
    fn octave_params(
        &self,
        len: u32,
        kind: u16,
        sample: u32,
        rep: u16,
        replen: u16,
        note: u8,
    ) -> SampleNote {
        let (octave, in_octave) = (u16::from(note) / 12, u16::from(note) % 12);
        let (row, parts) = if kind as u8 == 2 {
            (octave + 6, 7)
        } else {
            (octave, 31)
        };
        // divu.w: quotient in the low word, remainder in the high word; unchanged on overflow.
        let d1 = if len / parts > 0xffff {
            len
        } else {
            ((len % parts) << 16) | (len / parts)
        };
        let at = |t: &[u8; 12]| t.get(usize::from(row)).copied().unwrap_or(0);
        let shift = u32::from(at(&self.tables.octave_shift)) & 63;
        let sh = |v: u16| if shift >= 16 { 0 } else { v << shift };
        let length = (d1 & 0xffff_0000) | u32::from(sh(d1 as u16));
        let start = (u32::from(d1 as u16) * u32::from(at(&self.tables.octave_mult)))
            .wrapping_add(sample)
            .wrapping_add(instr::DATA);
        let n = (in_octave as u8).wrapping_add(at(&self.tables.octave_note));
        SampleNote {
            start,
            length,
            rep: sh(rep),
            replen: sh(replen),
            period: self.period_at(SAMPLE_PERIODS, u16::from(n.wrapping_mul(2) / 2)),
        }
    }
}

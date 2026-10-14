//! The MED timer interrupt 0x100890: one tick of the song. On the line's first tick it reads the
//! block's notes (instrument, volume, the tick-0 commands, note starts), moves to the next line
//! and looks ahead for held notes; then (unless the song's flag bit 5 says otherwise) it runs
//! the per-tick effects (`effects.rs`) and starts the DMA of the channels that got a new note.

use super::med::{CHANNELS, Hw, MedPlayer, PLAYING_SONG, TRACKS};
use super::module::{Module, addr, line_at, song};

/// Song flag bit 5: no effects on a line's first tick.
const FLAG_NO_TICK0_FX: u8 = 0x20;
/// Song flag bit 4: command C takes a hex volume (else two decimal digits).
const FLAG_HEX_VOLUME: u8 = 0x10;

impl MedPlayer {
    /// Original: 0x100890 (sound: MED timer interrupt — one tick of the song)
    pub(super) fn interrupt(&mut self, hw: &mut Hw) {
        let Some(md) = self.module else {
            return;
        };
        self.dma_pending = 0;
        let s = md.song(hw.mem());
        let tempo2 = hw.mem().b(addr(s, song::TEMPO2));
        let tick = self.counter.wrapping_add(1);
        let tick = if (tick as i8) < (tempo2 as i8) {
            self.counter = tick;
            tick
        } else {
            self.counter = 0;
            if !self.read_line(hw, md, s) {
                return;
            }
            self.next_line(hw, md, s);
            if hw.mem().b(addr(s, song::FLAGS)) & FLAG_NO_TICK0_FX != 0 {
                self.start_dma(hw);
                return;
            }
            0
        };
        self.effects(hw, md, s, tick);
        self.start_dma(hw);
    }

    /// The line's notes and tick-0 commands (0x1008c0..0x100afe). False = command FFE stopped
    /// the song (the interrupt then ends without starting DMA).
    fn read_line(&mut self, hw: &mut Hw, md: Module, s: u32) -> bool {
        let block = md.block(hw.mem(), self.block);
        self.numtracks = u16::from(hw.mem().b(block));
        self.lines = u16::from(hw.mem().b(addr(block, 1)));
        let mut at = line_at(block, self.line, self.numtracks);
        for ch in 0..usize::from(self.numtracks).min(TRACKS) {
            let mem = hw.mem();
            let (mut note, b1, arg) = (mem.b(at), mem.b(addr(at, 1)), mem.b(addr(at, 2)));
            at = addr(at, 3);
            self.tracks[ch].arg = arg;
            let (bit4, bit5) = (note & 0x80 != 0, note & 0x40 != 0);
            note &= 0x3f;
            if b1 & 0xf0 != 0 || bit4 || bit5 {
                let instr = ((b1 >> 4) + if bit4 { 0x10 } else { 0 } + if bit5 { 0x20 } else { 0 })
                    .wrapping_sub(1);
                let entry = addr(s, u32::from(instr) * 8);
                let t = &mut self.tracks[ch];
                t.instr = instr;
                t.hold = self.hold.get(usize::from(instr)).copied().unwrap_or(0);
                t.decay = self.decay.get(usize::from(instr)).copied().unwrap_or(0);
                t.instr_entry = entry;
                t.volume = scale(mem.b(addr(entry, 6)), t.track_vol);
                t.transpose = mem.b(addr(entry, 7));
            }
            let cmd = b1 & 0x0f;
            self.tracks[ch].cmd = cmd;
            match cmd {
                0 => {}
                0xf => match arg {
                    0 => self.next_block = self.next_block.wrapping_add(1),
                    1..=0xf0 => self.set_tempo(hw, u16::from(arg)),
                    0xf2 => {
                        self.tracks[ch].note = note;
                        note = 0;
                    }
                    0xfe => {
                        self.state = 0;
                        let crb = hw.timer.control() & !1;
                        hw.timer.write_control(crb);
                        self.all_off(hw);
                        return false;
                    }
                    0xfd if ch < CHANNELS => {
                        let t = &self.tracks[ch];
                        let n = note.wrapping_sub(1);
                        if (n as i8) >= 0 {
                            let p =
                                self.period_at(t.period_table, u16::from(n.wrapping_mul(2) / 2));
                            hw.period(ch, p);
                        }
                        note = 0;
                    }
                    0xff => self.channel_off(hw, ch),
                    _ => {}
                },
                0xe => {
                    let t = &mut self.tracks[ch];
                    t.wf_pos = (t.wf_pos & 0xff00) | u16::from(arg);
                }
                0xc => {
                    let mut v = arg;
                    if hw.mem().b(addr(s, song::FLAGS)) & FLAG_HEX_VOLUME == 0 {
                        v = (arg >> 4).wrapping_mul(10).wrapping_add(arg & 0x0f);
                    }
                    let t = &mut self.tracks[ch];
                    t.volume = scale(v.min(0x40), t.track_vol);
                }
                0x9 => {
                    let v = match arg & 0x1f {
                        0 => 0x20,
                        v => v,
                    };
                    hw.set_byte(addr(s, song::TEMPO2), v);
                }
                0x8 => {
                    let t = &mut self.tracks[ch];
                    t.decay = arg >> 4;
                    t.hold = arg & 0x0f;
                }
                0xb => {
                    if u16::from(arg) <= hw.mem().w(addr(s, song::SONGLEN)) {
                        self.pseq = u16::from(arg);
                        self.next_block = 0xff;
                    }
                }
                0x3 => note = self.portamento_target(hw, s, ch, note, arg),
                _ => {}
            }
            if note != 0 {
                let t = &mut self.tracks[ch];
                t.note = note;
                t.hold_count = if t.hold == 0 { -1 } else { t.hold as i8 };
                let (vol, instr, entry) = (t.volume, t.instr, t.instr_entry);
                self.play_note(hw, md, ch, note, vol, instr, entry);
            }
        }
        true
    }

    /// Command 3 on the line's first tick: a note becomes the slide target instead of starting.
    /// Returns the note left to play (0 = none).
    fn portamento_target(&mut self, hw: &Hw, s: u32, ch: usize, note: u8, arg: u8) -> u8 {
        let n = note.wrapping_sub(1);
        if (n as i8) < 0 {
            if arg != 0 {
                self.tracks[ch].porta_speed = arg;
            }
            return 0;
        }
        if ch >= CHANNELS {
            return 0;
        }
        let idx = n
            .wrapping_add(hw.mem().b(addr(s, song::PLAYTRANSP)))
            .wrapping_add(self.tracks[ch].transpose);
        if (idx as i8) < 0 {
            return 0;
        }
        let t = &self.tracks[ch];
        let target = self.period_at(t.period_table, u16::from(idx));
        let t = &mut self.tracks[ch];
        t.porta_target = target;
        t.porta_speed = arg;
        0
    }

    /// The next line, block and play sequence position, then the hold look-ahead (0x100afe..
    /// 0x100bbe).
    fn next_line(&mut self, hw: &mut Hw, md: Module, s: u32) {
        let mem = hw.mem();
        let mut line = self.line.wrapping_add(1);
        if (line as i16) > (self.lines as i16) || self.next_block != 0 {
            line = 0;
            if self.state == PLAYING_SONG {
                let mut pos = self.pseq;
                if (self.next_block as i8) >= 0 {
                    pos = pos.wrapping_add(1);
                }
                if (pos as i16) >= (mem.w(addr(s, song::SONGLEN)) as i16) {
                    pos = 0;
                }
                self.pseq = (self.pseq & 0xff00) | (pos & 0xff);
                let mut blk = mem.b(addr(s, song::PLAYSEQ + u32::from(pos)));
                if (blk as i8) >= (mem.b(addr(s, song::NUMBLOCKS + 1)) as i8) {
                    blk = 0;
                }
                self.block = (self.block & 0xff00) | u16::from(blk);
            }
            self.next_block = 0;
        }
        self.line = line;
        // A held note goes on while the next line has the instrument without a note, or a
        // portamento note.
        let block = md.block(mem, self.block);
        let tracks = mem.b(block);
        let tempo2 = mem.b(addr(s, song::TEMPO2));
        let mut at = line_at(block, self.line, u16::from(tracks));
        for t in self.tracks.iter_mut().take(usize::from(tracks)) {
            if t.hold_count >= 0 {
                let (note, b1) = (mem.b(at), mem.b(addr(at, 1)));
                let keep = if note == 0 {
                    b1 & 0xf0 != 0
                } else {
                    note & 0x3f == 0 || b1 & 0x0f == 3
                };
                if keep {
                    t.hold_count = t.hold_count.wrapping_add(tempo2 as i8);
                }
            }
            at = addr(at, 3);
        }
    }

    /// The period table entry `n` words after `table` (0 past the extracted table).
    pub(super) fn period_at(&self, table: u16, n: u16) -> u16 {
        let i = usize::from(table) + usize::from(n);
        self.tables.periods.get(i).copied().unwrap_or(0)
    }
}

/// svol × track volume / 256 as the 68000 computes it (mulu.w, lsr.w #8 of the low word).
fn scale(v: u8, track_vol: u16) -> u8 {
    ((u32::from(v) * u32::from(track_vol)) as u16 >> 8) as u8
}

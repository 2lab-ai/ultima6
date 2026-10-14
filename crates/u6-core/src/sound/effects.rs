//! The per-tick part of the MED interrupt 0x100890 (0x100bc0..0x100e1e): hold and decay, the
//! synth step, the effect commands as this player implements them (0x100c46 jump table), and
//! the DMA start at the end (0x100628).

use super::med::{CHANNELS, Hw, MedPlayer};
use super::module::{Module, addr, song};

/// Lowest period the slides leave (0x71 = 113).
const MIN_PERIOD: u16 = 0x71;

impl MedPlayer {
    /// Effects of tick `tick` on every Amiga track (part of 0x100890).
    pub(super) fn effects(&mut self, hw: &mut Hw, md: Module, s: u32, tick: u8) {
        for ch in 0..usize::from(self.numtracks).min(self.tracks.len()) {
            if ch >= CHANNELS {
                continue;
            }
            let (cmd, arg) = (self.tracks[ch].cmd, self.tracks[ch].arg);
            // The synth step runs once per tick for a synth or hybrid note (0x100c12 / 0x100c18).
            let synth = self.tracks[ch].synth;
            let t = &mut self.tracks[ch];
            if t.hold_count >= 0 {
                t.hold_count -= 1;
                if t.hold_count < 0 {
                    if t.synth_type != 0 {
                        // Release: the volume sequence jumps to the decay position.
                        t.vol_pos = (t.vol_pos & 0xff00) | u16::from(t.release);
                        t.vol_wait = 0;
                    } else {
                        t.fade = t.release;
                        if t.fade == 0 {
                            hw.dmacon(1 << ch);
                        }
                    }
                }
            }
            if synth != 0 {
                self.synth_step(hw, ch, synth);
            }
            let t = &mut self.tracks[ch];
            if t.fade != 0 {
                t.volume = t.volume.wrapping_sub(t.fade);
                if (t.volume as i8) < 0 {
                    t.volume = 0;
                    t.fade = 0;
                }
                hw.volume(ch, t.volume);
            }
            let period = match cmd {
                0x0 if arg != 0 => self.arpeggio(hw, s, ch, arg, tick),
                0x1 => {
                    let t = &mut self.tracks[ch];
                    t.period = t.period.wrapping_sub(u16::from(arg));
                    if (t.period as i16) < MIN_PERIOD as i16 {
                        t.period = MIN_PERIOD;
                    }
                    Some(t.period)
                }
                0x2 => {
                    let t = &mut self.tracks[ch];
                    t.period = t.period.wrapping_add(u16::from(arg));
                    Some(t.period)
                }
                0x3 => Some(self.portamento(ch)),
                0x4 => Some(self.vibrato(ch, arg)),
                0x5 => {
                    let p = self.tracks[ch].period;
                    Some(if (tick as i8) >= 3 {
                        p
                    } else {
                        p.wrapping_sub(u16::from(arg))
                    })
                }
                0xa | 0xd => {
                    let t = &mut self.tracks[ch];
                    if arg & 0xf0 == 0 {
                        t.volume = t.volume.wrapping_sub(arg);
                        if (t.volume as i8) < 0 {
                            t.volume = 0;
                        }
                    } else {
                        t.volume = t.volume.wrapping_add(arg >> 4);
                        if (t.volume as i8) > 0x40 {
                            t.volume = 0x40;
                        }
                    }
                    Some(0)
                }
                0xc => Some(0),
                0xf => {
                    self.command_f(hw, md, ch, arg, tick);
                    None
                }
                _ => None,
            };
            // 0x100e02: a nonzero period goes to the channel; the volume always.
            if let Some(p) = period {
                if p != 0 {
                    hw.period(ch, p);
                }
                hw.volume(ch, self.tracks[ch].volume);
            }
        }
    }

    /// Command 0: tick % 3 = 0 → note + low digit, 1 → note + high digit, 2 → the note.
    fn arpeggio(&self, hw: &Hw, s: u32, ch: usize, arg: u8, tick: u8) -> Option<u16> {
        let t = &self.tracks[ch];
        let n = match tick % 3 {
            0 => (arg & 0x0f).wrapping_add(t.note),
            1 => (arg >> 4).wrapping_add(t.note),
            _ => t.note,
        };
        let n = n
            .wrapping_sub(1)
            .wrapping_add(hw.mem().b(addr(s, song::PLAYTRANSP)))
            .wrapping_add(t.transpose);
        Some(self.period_at(t.period_table, u16::from(n.wrapping_mul(2) / 2)))
    }

    /// Command 3: slide the period toward the target by the speed; the target clears on arrival.
    fn portamento(&mut self, ch: usize) -> u16 {
        let t = &mut self.tracks[ch];
        let target = t.porta_target;
        if target == 0 {
            return 0;
        }
        let speed = u16::from(t.porta_speed);
        let mut p = t.period;
        let arrived = if p > target {
            p = p.wrapping_sub(speed);
            p <= target
        } else {
            p = p.wrapping_add(speed);
            (p as i16) >= (target as i16)
        };
        if arrived {
            p = target;
            t.porta_target = 0;
        }
        t.period = p;
        p
    }

    /// Command 4: sine vibrato, depth = low digit, speed = high digit; 0 keeps the last.
    fn vibrato(&mut self, ch: usize, arg: u8) -> u16 {
        let sine = self.tables.sine;
        let t = &mut self.tracks[ch];
        if arg != 0 {
            t.vib_cmd = arg;
        }
        let s = i16::from(sine[usize::from((t.vib_pos >> 2) & 0x1f)]);
        let delta = (s * i16::from(t.vib_cmd & 0x0f)) >> 5;
        t.vib_pos = t.vib_pos.wrapping_add((t.vib_cmd >> 3) & 0x3e);
        t.period.wrapping_add(delta as u16)
    }

    /// Command F on later ticks: F1 / F2 replay at tick 3, F3 at ticks with bit 1 or 2 set;
    /// F8 / F9 filter off / on.
    fn command_f(&mut self, hw: &mut Hw, md: Module, ch: usize, arg: u8, tick: u8) {
        let replay = match arg {
            0xf1 | 0xf2 => tick == 3,
            0xf3 => tick & 6 != 0,
            0xf8 => {
                hw.led_bit(true);
                false
            }
            0xf9 => {
                hw.led_bit(false);
                false
            }
            _ => false,
        };
        if replay {
            let t = &self.tracks[ch];
            let (note, vol, instr, entry) = (t.note, t.volume, t.instr, t.instr_entry);
            self.play_note(hw, md, ch, note, vol, instr, entry);
        }
    }

    /// Original: 0x100628 (sound: MED start the DMA of new notes, then their repeat part)
    ///
    /// The busy waits around the DMACON write (0x1005f2, ~120 colour clocks each) let Paula
    /// latch the old state and then the new start; the emulated Paula latches at once. Then the
    /// loop start and length go in for the next block (0x10060c).
    pub(super) fn start_dma(&mut self, hw: &mut Hw) {
        let bits = self.dma_pending;
        if bits == 0 {
            return;
        }
        hw.dmacon(bits | 0x8000);
        for ch in 0..CHANNELS {
            let t = &self.tracks[ch];
            if bits & (1 << ch) != 0 && t.repeat != 0 {
                hw.location(ch, t.repeat);
                hw.length(ch, t.repeat_len);
            }
        }
    }
}

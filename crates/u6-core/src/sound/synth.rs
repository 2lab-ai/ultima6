//! MED synth sounds: the per-tick volume and waveform sequences of a synth (or hybrid)
//! instrument, its arpeggio and vibrato. The sound-effect module in hunk 28 is a synth sound.

use super::med::{Hw, MedPlayer};
use super::module::{Module, addr, instr};

/// Port bound on sequence commands per step: a JMP onto itself would hang the original's
/// interrupt; the port stops the step instead.
const MAX_COMMANDS: usize = 256;

/// `addq.b #1` / `subq.b #1` on a position word: the low byte wraps on its own.
fn inc(pos: u16) -> u16 {
    (pos & 0xff00) | u16::from((pos as u8).wrapping_add(1))
}

fn dec(pos: u16) -> u16 {
    (pos & 0xff00) | u16::from((pos as u8).wrapping_sub(1))
}

fn low(word: u16, byte: u8) -> u16 {
    (word & 0xff00) | u16::from(byte)
}

impl MedPlayer {
    /// The synth set-up after a note start (0x100368, the tail of 0x100234): sequences from
    /// the start, the instrument's speeds, no pitch offsets; then one step.
    pub(super) fn synth_init(&mut self, hw: &mut Hw, ch: usize) {
        let ins = self.tracks[ch].synth;
        let speeds = hw.mem().w(addr(ins, instr::SPEEDS));
        let t = &mut self.tracks[ch];
        t.arp_pos = 0;
        t.arp_start = 0;
        t.vol_count = 0;
        t.wf_count = 0;
        t.vol_pos = 0;
        t.wf_pos = 0;
        t.vol_wait = 0;
        t.wf_wait = 0;
        t.vib_speed = 0;
        t.period_change = 0;
        t.vol_change = 0;
        [t.vol_speed, t.wf_speed] = speeds.to_be_bytes();
        t.period_offset = 0;
        t.vib_phase = 0;
        t.vib_depth = 0;
        self.synth_step(hw, ch, ins);
    }

    /// Original: 0x10038a (sound: MED synth step — volume and waveform sequences, arpeggio,
    /// vibrato)
    pub(super) fn synth_step(&mut self, hw: &mut Hw, ch: usize, ins: u32) {
        let (mem, paula) = hw.split();
        let periods = &self.tables.periods;
        let sine = &self.tables.sine;
        let t = &mut self.tracks[ch];
        let Some(regs) = paula.channels.get_mut(ch) else {
            return;
        };
        // Sequence bytes: entry `pos` (1-based) and the argument after it.
        let vol_at = |pos: u16| mem.b(addr(ins, instr::VOLTBL - 1 + u32::from(pos)));
        let wf_at = |pos: u16| mem.b(addr(ins, instr::WFTBL - 1 + u32::from(pos)));

        t.vol_count = t.vol_count.wrapping_sub(1);
        if (t.vol_count as i8) <= 0 {
            t.vol_count = t.vol_speed;
            if t.vol_change != 0 {
                // Signed byte compares: below 0 → 0, above 64 → 64.
                let v = (t.vol_change.wrapping_add(t.synth_vol) as i8).clamp(0, 0x40) as u8;
                t.synth_vol = v;
                regs.volume = u16::from(v);
            }
            let waiting = t.vol_wait != 0 && {
                t.vol_wait = t.vol_wait.wrapping_sub(1);
                (t.vol_wait as i8) > 0
            };
            if !waiting {
                let mut pos = t.vol_pos;
                for _ in 0..MAX_COMMANDS {
                    pos = inc(pos);
                    let (v, arg) = (vol_at(pos), vol_at(pos + 1));
                    if (v as i8) >= 0 {
                        t.synth_vol = v;
                        regs.volume = u16::from(v);
                        break;
                    }
                    match v & 0x0f {
                        0x0 => t.vol_speed = arg,
                        0x1 => {
                            t.vol_wait = arg;
                            pos = inc(pos);
                            break;
                        }
                        0x2 => t.vol_change = arg.wrapping_neg(),
                        0x3 => t.vol_change = arg,
                        0xa => {
                            t.wf_pos = low(t.wf_pos, arg);
                            t.wf_wait = 0;
                        }
                        0xb | 0xf => {
                            pos = dec(pos);
                            break;
                        }
                        0xe => {
                            pos = low(pos, arg);
                            continue;
                        }
                        _ => break,
                    }
                    pos = inc(pos);
                }
                t.vol_pos = pos;
            }
        }
        t.volume = t.synth_vol;

        t.wf_count = t.wf_count.wrapping_sub(1);
        if (t.wf_count as i8) <= 0 {
            t.wf_count = t.wf_speed;
            if t.period_change != 0 {
                t.period_offset = t.period_change.wrapping_add(t.period_offset);
                let mut p = t.period_offset.wrapping_add(t.period);
                if (p as i16) < 0x71 {
                    p = 0x71;
                }
                regs.period = p;
            }
            let waiting = t.wf_wait != 0 && {
                t.wf_wait = t.wf_wait.wrapping_sub(1);
                t.wf_wait != 0
            };
            if !waiting {
                let mut pos = t.wf_pos;
                for _ in 0..MAX_COMMANDS {
                    pos = inc(pos);
                    let (v, arg) = (wf_at(pos), wf_at(pos + 1));
                    if (v as i8) >= 0 {
                        let wf = Module::waveform(mem, ins, v);
                        regs.location = wf.wrapping_add(2);
                        regs.length = mem.w(wf);
                        break;
                    }
                    match v & 0x0f {
                        0x0 => t.wf_speed = arg,
                        0x1 => {
                            t.wf_wait = arg;
                            pos = inc(pos);
                            break;
                        }
                        0x2 => t.period_change = u16::from(arg),
                        0x3 => t.period_change = i16::from(arg.wrapping_neg() as i8) as u16,
                        0x4 => t.vib_depth = low(t.vib_depth, arg),
                        0x5 => t.vib_speed = low(t.vib_speed, arg.wrapping_add(1)),
                        0x6 => {
                            t.period_offset = 0;
                            regs.period = t.period;
                            continue;
                        }
                        0xa => {
                            t.vol_pos = low(t.vol_pos, arg);
                            t.vol_wait = 0;
                        }
                        0xb | 0xf => {
                            pos = dec(pos);
                            break;
                        }
                        0xc => {
                            // Arpeggio: its notes follow up to the next negative byte.
                            t.arp_start = pos;
                            t.arp_pos = pos;
                            for _ in 0..MAX_COMMANDS {
                                pos = inc(pos);
                                if (wf_at(pos) as i8) < 0 {
                                    break;
                                }
                            }
                            continue;
                        }
                        0xd => continue,
                        0xe => {
                            pos = low(pos, arg);
                            continue;
                        }
                        _ => break,
                    }
                    pos = inc(pos);
                }
                t.wf_pos = pos;
            }
        }

        if t.arp_pos != 0 {
            let n = wf_at(t.arp_pos + 1).wrapping_add(t.synth_note);
            let i = usize::from(t.period_table) + usize::from(n);
            let p = periods
                .get(i)
                .copied()
                .unwrap_or(0)
                .wrapping_add(t.period_offset);
            t.period = p;
            regs.period = p;
            let next = inc(t.arp_pos);
            t.arp_pos = if (wf_at(next + 1) as i8) < 0 {
                t.arp_start
            } else {
                next
            };
        }
        if t.vib_depth != 0 {
            let s = i32::from(sine[usize::from((t.vib_phase >> 4) & 0x1f)]);
            let d = ((s * i32::from(t.vib_depth as i16)) as i16) >> 8;
            regs.period = t
                .period
                .wrapping_add(d as u16)
                .wrapping_add(t.period_offset);
            t.vib_phase = t.vib_phase.wrapping_add(t.vib_speed);
        }
    }
}

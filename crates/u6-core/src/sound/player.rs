//! The MED routine's entry points (InitPlayer, PlayModule, StopPlayer, RemPlayer), its channel
//! switch-off and the tempo → CIA timer conversion.

use u6_audio::{Machine, TimerInterrupt};

use super::med::{CHANNELS, Hw, MedPlayer, PLAYING_SONG, TRACKS};
use super::module::{Module, song};

/// 0x10102a's result in 0x101174 when CIA-B timer B was free (1 = it fell back to timer A).
const TIMER_B: u8 = 2;
/// 0x100f7c's tempo before any module: 33.
const INIT_TEMPO: u16 = 0x21;

impl MedPlayer {
    /// Original: 0x100f7c (sound: MED InitPlayer — set up, tempo 33)
    ///
    /// 0x10102a's resource work (signal, audio.device channel allocation, ciab.resource timer
    /// vector) is the platform's; the port keeps its effect on the routine: player set up on
    /// timer B, timer stopped (CRB &= 0x80), state 0. 474326 replaces the PAL divisor only on a
    /// 60 Hz machine; the game requires PAL, so the image's value stays.
    pub fn init(&mut self, m: &mut Machine) {
        self.init_player(m);
        self.set_tempo(&mut Hw::of(m), INIT_TEMPO);
    }

    /// Original: 0x10102a (sound: MED init — signal, audio.device, ciab.resource, CIA-B timer)
    /// [partial]
    fn init_player(&mut self, m: &mut Machine) {
        let crb = m.timer.control() & 0x80;
        m.timer.write_control(crb);
        self.state = 0;
        self.initialized = TIMER_B;
    }

    /// Original: 0x100f96 (sound: MED RemPlayer — stop, then shut down)
    pub fn remove(&mut self, m: &mut Machine) {
        if self.initialized != 0 {
            self.stop(m);
        }
        self.shutdown(m);
    }

    /// Original: 0x101126 (sound: MED shutdown — remove the CIA vector, close audio.device)
    /// [partial]
    ///
    /// The OS calls are the platform's; what stays is the DMACON write that silences the four
    /// channels.
    fn shutdown(&mut self, m: &mut Machine) {
        self.initialized = 0;
        m.paula.write_dmacon(0x000f);
    }

    /// Original: 0x101010 (sound: MED StopPlayer — stop the timer and every channel)
    pub fn stop(&mut self, m: &mut Machine) {
        if self.initialized == 0 {
            return;
        }
        let crb = m.timer.control() & !1;
        m.timer.write_control(crb);
        self.state = 0;
        self.all_off(&mut Hw::of(m));
    }

    /// Original: 0x100fa4 (sound: MED PlayModule — track volumes, filter, tempo, start the timer)
    ///
    /// `module` = 0 is ignored after the table setup, as is a call before InitPlayer. Playing
    /// does not reset the channels: notes of a previous module keep sounding until a new note,
    /// as on the Amiga (the instrument Use command restarts the module per key this way).
    pub fn play(&mut self, m: &mut Machine, module: u32) {
        self.set_tables(&mut Hw::of(m), module);
        if self.initialized == 0 || module == 0 {
            return;
        }
        let crb = m.timer.control() & !1;
        m.timer.write_control(crb);
        self.module = None;
        let md = Module { base: module };
        let mut hw = Hw::of(m);
        let mem = hw.mem();
        let s = md.song(mem);
        self.counter = mem.b(s + song::TEMPO2);
        let filter_on = mem.b(s + song::FLAGS) & 1 != 0;
        let tempo = mem.w(s + song::DEFTEMPO);
        let first = mem.b(s + song::PLAYSEQ);
        hw.led_bit(!filter_on);
        self.line = 0;
        self.block = u16::from(first);
        self.pseq = 0;
        self.state = PLAYING_SONG;
        self.module = Some(md);
        self.set_tempo(&mut hw, tempo);
        let crb = m.timer.control() | 1;
        m.timer.write_control(crb);
    }

    /// Original: 0x100f10 (sound: MED track volumes and the hold/decay table of a module)
    ///
    /// Track volume = trkvol × mastervol / 16 for all 16 tracks; hold/decay from the module's
    /// instrument extensions. Entries past the module's count keep the previous module's values,
    /// as in the original; a module without extensions clears all 63.
    pub(super) fn set_tables(&mut self, hw: &mut Hw, module: u32) {
        if module == 0 {
            return;
        }
        let md = Module { base: module };
        let mem = hw.mem();
        let s = md.song(mem);
        let master = i16::from(mem.b(s + song::MASTERVOL) as i8);
        for (n, t) in self.tracks.iter_mut().enumerate() {
            let v = i16::from(mem.b(s + song::TRKVOL + n as u32) as i8);
            t.track_vol = (v as u16).wrapping_mul(master as u16) >> 4;
        }
        let exp = md.expdata(mem);
        let ext = if exp == 0 { 0 } else { md.exp_samples(mem, exp) };
        let entries = if ext == 0 { 0 } else { mem.w(exp + 8) };
        if entries == 0 {
            self.hold = [0; 63];
            self.decay = [0; 63];
            return;
        }
        let size = u32::from(mem.w(exp + 10));
        for i in 0..usize::from(entries) {
            let at = ext + size * i as u32;
            // The original writes past its 63-byte tables for more entries; the port stops.
            if i < 63 {
                self.decay[i] = mem.b(at + 1);
                self.hold[i] = mem.b(at);
            }
        }
    }

    /// Original: 0x100e30 (sound: MED tempo → CIA timer value)
    ///
    /// Tempos 1..10 are SoundTracker speeds (a table of timer values); above, the timer is the
    /// divisor / tempo. A byte tempo of 0 would index before the table; the port then uses the
    /// slowest timer.
    pub(super) fn set_tempo(&mut self, hw: &mut Hw, tempo: u16) {
        let value = if tempo as u8 <= 10 {
            let i = (tempo & 0xff00) | u16::from((tempo as u8).wrapping_sub(1));
            self.tables.st_tempos.get(usize::from(i)).copied().unwrap_or(0xffff)
        } else {
            let q = self.timer_div / u32::from(tempo);
            // divu.w leaves D1 alone when the quotient overflows 16 bits.
            if q > 0xffff { self.timer_div as u16 } else { q as u16 }
        };
        hw.timer.write_lo(value as u8);
        hw.timer.write_hi((value >> 8) as u8);
    }

    /// Original: 0x1001f8 (sound: MED switch a channel off — synth state, DMA)
    pub(super) fn channel_off(&mut self, hw: &mut Hw, ch: usize) {
        if ch >= CHANNELS {
            return;
        }
        let t = &mut self.tracks[ch];
        t.synth = 0;
        t.synth_type = 0;
        hw.dmacon(1 << ch);
    }

    /// Original: 0x10021e (sound: MED every channel off, no module)
    pub(super) fn all_off(&mut self, hw: &mut Hw) {
        for ch in (0..TRACKS).rev() {
            self.channel_off(hw, ch);
        }
        self.module = None;
    }
}

impl TimerInterrupt for MedPlayer {
    fn timer_interrupt(&mut self, m: &mut Machine) {
        self.interrupt(&mut Hw::of(m));
    }
}

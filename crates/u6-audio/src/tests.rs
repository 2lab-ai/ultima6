//! Hardware behaviour on synthetic memory: pitch from the period, the DMA start/loop rules,
//! volume and panning, the timer rate, the filters.

use super::*;

/// Counts interrupts; optionally runs a closure on each.
#[derive(Default)]
struct Count(u32);

impl TimerInterrupt for Count {
    fn timer_interrupt(&mut self, _m: &mut Machine) {
        self.0 += 1;
    }
}

const RATE: u32 = 48_000;

/// A machine whose chip memory holds a 2-word square wave at 0 and a constant +64 block at 8.
fn machine() -> Machine {
    let mut m = Machine::new(64);
    m.chip[..4].copy_from_slice(&[0x7f, 0x7f, 0x80, 0x80]);
    m.chip[8..12].copy_from_slice(&[0x40; 4]);
    m.filter = FilterModel::Raw;
    m
}

fn start(m: &mut Machine, ch: usize, at: u32, words: u16, period: u16, vol: u16) {
    let c = &mut m.paula.channels[ch];
    c.location = at;
    c.length = words;
    c.period = period;
    c.volume = vol;
    m.paula.write_dmacon(0x8000 | 1 << ch);
}

fn render(m: &mut Machine, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    m.render(&mut Count::default(), &mut out, RATE);
    out
}

fn sign_changes(side: impl Iterator<Item = f32>) -> usize {
    let v: Vec<f32> = side.filter(|x| x.abs() > 1e-6).collect();
    v.windows(2).filter(|w| (w[0] > 0.0) != (w[1] > 0.0)).count()
}

#[test]
fn period_sets_the_pitch_at_the_pal_clock() {
    let mut m = machine();
    start(&mut m, 0, 0, 2, 200, 64);
    let out = render(&mut m, RATE as usize);
    let crossings = sign_changes(out.iter().step_by(2).copied());
    // 4 samples × 200 clocks per cycle, two sign changes per cycle.
    let expect = 2.0 * f64::from(PAL_CLOCK) / 800.0;
    assert!((crossings as f64 - expect).abs() < expect * 0.01, "{crossings} vs {expect}");
}

#[test]
fn channels_pan_hard_left_or_right() {
    for (ch, left) in [(0, true), (1, false), (2, false), (3, true)] {
        let mut m = machine();
        start(&mut m, ch, 8, 2, 300, 64);
        let out = render(&mut m, 100);
        let (l, r) = (out[100], out[101]);
        assert_eq!(l > 0.2, left, "channel {ch} left {l}");
        assert_eq!(r > 0.2, !left, "channel {ch} right {r}");
        assert!(l.abs() < 1e-6 || r.abs() < 1e-6);
    }
}

#[test]
fn volume_is_six_bits_with_bit_6_meaning_full() {
    let level = |vol| {
        let mut m = machine();
        start(&mut m, 0, 8, 2, 300, vol);
        render(&mut m, 50)[60]
    };
    assert!((level(64) - level(0x7f)).abs() < 1e-6);
    assert!((level(32) * 2.0 - level(64)).abs() < 1e-4);
    assert_eq!(level(0), 0.0);
}

#[test]
fn dma_off_is_silent_and_on_again_restarts_from_the_location() {
    let mut m = machine();
    start(&mut m, 0, 0, 2, 124, 64);
    render(&mut m, 3);
    m.paula.write_dmacon(0x0001);
    assert!(render(&mut m, 10).iter().all(|&x| x == 0.0));
    m.paula.channels[0].location = 8;
    m.paula.write_dmacon(0x8001);
    assert!(render(&mut m, 4)[2] > 0.2, "restarted on the constant block");
}

#[test]
fn enabling_a_running_channel_does_not_restart_it() {
    let run = |rewrite: bool| {
        let mut m = machine();
        start(&mut m, 0, 0, 2, 124, 64);
        render(&mut m, 3);
        if rewrite {
            m.paula.write_dmacon(0x8001);
        }
        render(&mut m, 50)
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn the_loop_uses_registers_written_after_the_start() {
    let mut m = machine();
    start(&mut m, 0, 0, 1, 500, 64);
    // Repeat part written after the DMA start, as the MED player does (0x10060c).
    m.paula.channels[0].location = 8;
    m.paula.channels[0].length = 2;
    let out = render(&mut m, 400);
    let first = out[0];
    let later: Vec<f32> = out.iter().step_by(2).skip(50).copied().collect();
    assert!(first > 0.4, "first block = 0x7f7f: {first}");
    assert!(later.iter().all(|&x| (x - later[0]).abs() < 1e-6 && x > 0.2));
}

#[test]
fn timer_interrupts_every_latch_plus_one_e_cycles() {
    let mut m = machine();
    m.timer.write_lo(0x42);
    m.timer.write_hi(0x37);
    m.timer.write_control(1);
    let mut irq = Count::default();
    let mut out = vec![0.0; RATE as usize * 2];
    m.render(&mut irq, &mut out, RATE);
    let expect = f64::from(PAL_CLOCK) / 5.0 / f64::from(0x3742 + 1);
    assert!((f64::from(irq.0) - expect).abs() <= 1.0, "{} vs {expect}", irq.0);
}

#[test]
fn a_latch_written_while_running_applies_from_the_next_reload() {
    let mut t = CiaTimer::default();
    t.write_lo(9);
    t.write_hi(0);
    t.write_control(1);
    assert_eq!(t.clocks_to_underflow(), Some(50));
    t.write_lo(19);
    assert!(!t.advance(49));
    assert!(t.advance(1));
    assert_eq!(t.clocks_to_underflow(), Some(100));
    t.write_control(0);
    assert_eq!(t.clocks_to_underflow(), None);
}

#[test]
fn the_led_filter_cuts_high_frequencies() {
    let rms = |led: bool| {
        let mut m = machine();
        m.filter = FilterModel::A500;
        m.led_filter = led;
        start(&mut m, 0, 0, 2, 80, 64); // ~11 kHz square
        let out = render(&mut m, 4800);
        let s: f64 = out.iter().step_by(2).skip(480).map(|&x| f64::from(x).powi(2)).sum();
        (s / 4320.0).sqrt()
    };
    let (off, on) = (rms(false), rms(true));
    assert!(on < off * 0.5, "led on {on}, off {off}");
}

#[test]
fn a_run_is_deterministic_across_buffer_sizes() {
    let run = |chunk: usize| {
        let mut m = machine();
        m.filter = FilterModel::A500;
        start(&mut m, 1, 0, 2, 333, 40);
        m.timer.write_lo(100);
        m.timer.write_hi(0);
        m.timer.write_control(1);
        let mut all = Vec::new();
        let mut irq = Count::default();
        for _ in 0..(4800 / chunk) {
            let mut out = vec![0.0; chunk * 2];
            m.render(&mut irq, &mut out, RATE);
            all.extend(out);
        }
        (all, irq.0)
    };
    assert_eq!(run(160), run(480));
}

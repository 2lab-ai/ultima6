//! The MED routine on synthetic modules: timing, note starts, loops, commands.

use super::*;
use crate::sound::med::{NULL_SAMPLE, PLAYING_SONG};
use crate::sound::module;

fn period(note: u8) -> u16 {
    tables().periods[24 + usize::from(note) - 1]
}

fn one_line(cells: [[u8; 3]; 4]) -> Mmd {
    Mmd::new(vec![vec![cells, [EMPTY; 4]]])
}

#[test]
fn play_module_sets_the_tempo_timer_and_the_first_tick_reads_line_0() {
    let (mut p, mut m) = playing(&one_line([cell(13, 1, 0, 0), EMPTY, EMPTY, EMPTY]));
    assert_eq!(m.timer.latch(), (600_000 / 125) as u16);
    assert!(m.timer.running());
    assert_eq!((p.counter, p.state, p.line), (3, PLAYING_SONG, 0));
    ticks(&mut p, &mut m, 1);
    let c = &m.paula.channels[0];
    assert!(c.playing());
    assert_eq!(c.period, period(13));
    assert_eq!(c.volume, 64);
    // One-shot: after the start the registers already hold the null-sample repeat.
    assert_eq!((c.location, c.length), (NULL_SAMPLE, 1));
    assert_eq!(p.line, 1);
}

#[test]
fn soundtracker_tempos_take_the_table() {
    let mut md = one_line([EMPTY; 4]);
    md.tempo = 3;
    let (_, m) = playing(&md);
    assert_eq!(m.timer.latch(), 3000);
}

#[test]
fn a_line_lasts_tempo2_ticks_and_the_play_sequence_wraps() {
    let mut md = Mmd::new(vec![vec![[EMPTY; 4]; 2], vec![[EMPTY; 4]; 2]]);
    md.playseq = vec![1, 0];
    let (mut p, mut m) = playing(&md);
    assert_eq!(p.block, 1);
    let mut seen = Vec::new();
    for _ in 0..13 {
        ticks(&mut p, &mut m, 1);
        seen.push((p.counter, p.block, p.line));
    }
    // Tick 0 of each line moves on to the next one at once; ticks 1, 2 follow.
    assert_eq!(&seen[..4], &[(0, 1, 1), (1, 1, 1), (2, 1, 1), (0, 0, 0)]);
    assert_eq!(seen[6], (0, 0, 1));
    assert_eq!(
        seen[9],
        (0, 1, 0),
        "end of the sequence wraps to position 0"
    );
}

#[test]
fn a_looped_sample_plays_up_to_the_loop_then_repeats_it() {
    let mut md = one_line([cell(1, 1, 0, 0), cell(1, 2, 0, 0), EMPTY, EMPTY]);
    md.insts = vec![Inst::sample(64, 4, 8), Inst::sample(64, 0, 8)];
    let (mut p, mut m) = playing(&md);
    ticks(&mut p, &mut m, 1);
    let (s0, s1) = (sample_data(&m, 0), sample_data(&m, 1));
    assert_eq!(p.tracks[0].repeat, s0 + 8);
    assert_eq!(
        (m.paula.channels[0].location, m.paula.channels[0].length),
        (s0 + 8, 8)
    );
    assert_eq!(p.tracks[1].repeat, s1);
    assert_eq!(m.paula.channels[1].length, 8);
}

#[test]
fn song_and_instrument_transposes_shift_the_note() {
    let mut md = one_line([cell(5, 1, 0, 0), EMPTY, EMPTY, EMPTY]);
    md.transpose = 12;
    md.insts[0].transpose = -1;
    let (mut p, mut m) = playing(&md);
    ticks(&mut p, &mut m, 1);
    assert_eq!(m.paula.channels[0].period, period(5 + 11));
}

#[test]
fn notes_wrap_into_the_table_by_octaves() {
    let mut md = one_line([cell(63, 1, 0, 0), EMPTY, EMPTY, EMPTY]);
    md.transpose = 12;
    let (mut p, mut m) = playing(&md);
    ticks(&mut p, &mut m, 1);
    // 63 + 12 - 1 = 74 > 0x3e: one octave down.
    assert_eq!(m.paula.channels[0].period, tables().periods[24 + 62]);
}

#[test]
fn f00_goes_to_the_next_position_and_b_jumps() {
    let row = |c: [u8; 3]| vec![[c, EMPTY, EMPTY, EMPTY], [EMPTY; 4], [EMPTY; 4]];
    let mut md = Mmd::new(vec![
        row(cell(0, 0, 0xf, 0)),
        row(cell(0, 0, 0xb, 3)),
        row(EMPTY),
        row(EMPTY),
    ]);
    md.playseq = vec![0, 1, 2, 3];
    let (mut p, mut m) = playing(&md);
    ticks(&mut p, &mut m, 1);
    assert_eq!((p.pseq, p.block, p.line), (1, 1, 0));
    ticks(&mut p, &mut m, 3);
    assert_eq!((p.pseq, p.block, p.line), (3, 3, 0));
}

#[test]
fn command_9_rewrites_the_lines_speed_in_the_module() {
    let (mut p, mut m) = playing(&one_line([cell(0, 0, 9, 0x05), EMPTY, EMPTY, EMPTY]));
    ticks(&mut p, &mut m, 1);
    let s = 0x34 + CHIP_BUFFER as usize;
    assert_eq!(m.chip[s + 0x301], 5);
    ticks(&mut p, &mut m, 4);
    assert_eq!(p.counter, 4, "5 ticks per line now");
}

#[test]
fn command_c_is_decimal_unless_the_song_says_hex() {
    for (flags, vol) in [(0, 32), (0x10, 0x32)] {
        let mut md = one_line([cell(1, 1, 0xc, 0x32), EMPTY, EMPTY, EMPTY]);
        md.flags = flags;
        let (mut p, mut m) = playing(&md);
        ticks(&mut p, &mut m, 1);
        assert_eq!(m.paula.channels[0].volume, vol, "flags {flags:#x}");
    }
}

#[test]
fn command_f_sets_the_tempo_and_ffe_stops_the_song() {
    let (mut p, mut m) = playing(&one_line([cell(0, 0, 0xf, 0x50), EMPTY, EMPTY, EMPTY]));
    ticks(&mut p, &mut m, 1);
    assert_eq!(m.timer.latch(), (600_000 / 0x50) as u16);
    let (mut p, mut m) = playing(&Mmd::new(vec![vec![
        [cell(1, 1, 0, 0), EMPTY, EMPTY, EMPTY],
        [cell(0, 0, 0xf, 0xfe), EMPTY, EMPTY, EMPTY],
    ]]));
    ticks(&mut p, &mut m, 4);
    assert_eq!(p.state, 0);
    assert!(p.module.is_none() && !m.timer.running());
    assert!(!m.paula.channels[0].playing());
}

#[test]
fn fff_switches_the_channel_off_and_ff2_delays_the_note_to_tick_3() {
    let mut md = Mmd::new(vec![vec![
        [cell(1, 1, 0, 0), cell(1, 1, 0xf, 0xf2), EMPTY, EMPTY],
        [cell(0, 0, 0xf, 0xff), EMPTY, EMPTY, EMPTY],
    ]]);
    md.tempo2 = 6;
    let (mut p, mut m) = playing(&md);
    ticks(&mut p, &mut m, 1);
    assert!(m.paula.channels[0].playing() && !m.paula.channels[1].playing());
    ticks(&mut p, &mut m, 3);
    assert!(m.paula.channels[1].playing(), "F2: played at tick 3");
    ticks(&mut p, &mut m, 3);
    assert!(!m.paula.channels[0].playing(), "FFF on the next line");
}

#[test]
fn slides_move_the_period_and_stop_at_113() {
    let md = one_line([cell(40, 1, 1, 0xff), cell(1, 1, 2, 0x10), EMPTY, EMPTY]);
    let (mut p, mut m) = playing(&md);
    ticks(&mut p, &mut m, 2);
    assert_eq!(m.paula.channels[0].period, 0x71);
    assert_eq!(
        m.paula.channels[1].period,
        period(1) + 0x20,
        "ticks 0 and 1"
    );
}

#[test]
fn the_no_tick0_flag_skips_the_first_ticks_effects() {
    for (flags, vol) in [(0, 64 - 12), (0x20, 64 - 8)] {
        let mut md = one_line([cell(1, 1, 0xd, 0x04), EMPTY, EMPTY, EMPTY]);
        md.flags = flags;
        let (mut p, mut m) = playing(&md);
        ticks(&mut p, &mut m, 3);
        assert_eq!(m.paula.channels[0].volume, vol, "flags {flags:#x}");
    }
}

#[test]
fn portamento_slides_to_the_target_and_clears_it() {
    let mut md = Mmd::new(vec![vec![
        [cell(13, 1, 0, 0), EMPTY, EMPTY, EMPTY],
        [cell(14, 0, 3, 0x20), EMPTY, EMPTY, EMPTY],
        [cell(0, 0, 3, 0), EMPTY, EMPTY, EMPTY],
    ]]);
    md.tempo2 = 6;
    let (mut p, mut m) = playing(&md);
    ticks(&mut p, &mut m, 7);
    let (from, to) = (period(13), period(14));
    assert_eq!(p.tracks[0].porta_target, to);
    assert_eq!(m.paula.channels[0].period, from - 0x20);
    ticks(&mut p, &mut m, 11);
    assert_eq!(
        (m.paula.channels[0].period, p.tracks[0].porta_target),
        (to, 0)
    );
}

#[test]
fn arpeggio_steps_low_digit_high_digit_note() {
    let mut md = one_line([cell(10, 1, 0, 0x47), EMPTY, EMPTY, EMPTY]);
    md.tempo2 = 6;
    let (mut p, mut m) = playing(&md);
    let mut seen = Vec::new();
    for _ in 0..4 {
        ticks(&mut p, &mut m, 1);
        seen.push(m.paula.channels[0].period);
    }
    assert_eq!(seen, [period(17), period(14), period(10), period(17)]);
}

#[test]
fn vibrato_swings_around_the_period() {
    let mut md = one_line([cell(20, 1, 4, 0x8f), EMPTY, EMPTY, EMPTY]);
    md.tempo2 = 32;
    let (mut p, mut m) = playing(&md);
    let base = period(20);
    let mut v = Vec::new();
    for _ in 0..16 {
        ticks(&mut p, &mut m, 1);
        v.push(i32::from(m.paula.channels[0].period) - i32::from(base));
    }
    assert_eq!(v[0], 0);
    assert!(
        v.iter().any(|&d| d > 40) && v.iter().any(|&d| d < -40),
        "{v:?}"
    );
}

#[test]
fn a_held_note_ends_after_its_hold_and_an_instrument_alone_extends_it() {
    let mut md = Mmd::new(vec![vec![
        [cell(1, 1, 0, 0), cell(1, 1, 0, 0), EMPTY, EMPTY],
        [cell(0, 1, 0, 0), EMPTY, EMPTY, EMPTY],
        [EMPTY; 4],
    ]]);
    md.ext = vec![(2, 0)];
    let (mut p, mut m) = playing(&md);
    ticks(&mut p, &mut m, 3);
    assert!(
        m.paula.channels[0].playing(),
        "extended by line 1's instrument"
    );
    assert!(!m.paula.channels[1].playing(), "hold 2 over after tick 2");
    ticks(&mut p, &mut m, 3);
    assert!(!m.paula.channels[0].playing());
}

#[test]
fn stop_turns_the_timer_and_every_channel_off() {
    let (mut p, mut m) = playing(&one_line([cell(1, 1, 0, 0); 4]));
    ticks(&mut p, &mut m, 1);
    assert_eq!(m.paula.dma_bits(), 0xf);
    p.stop(&mut m);
    assert_eq!(m.paula.dma_bits(), 0);
    assert!(!m.timer.running() && p.module.is_none() && p.state == 0);
}

#[test]
fn play_reads_track_volumes_and_the_hold_decay_table() {
    let mut md = one_line([EMPTY; 4]);
    md.ext = vec![(3, 5), (7, 9)];
    let bytes = md.build();
    let mut m = Machine::new(CHIP_BUFFER as usize + bytes.len());
    m.chip[CHIP_BUFFER as usize..].copy_from_slice(&bytes);
    m.chip[CHIP_BUFFER as usize + 0x34 + 0x302] = 32;
    let mut p = MedPlayer::new(tables());
    p.hold[5] = 99;
    p.play(&mut m, CHIP_BUFFER);
    assert!(
        p.module.is_none(),
        "PlayModule before InitPlayer does not start"
    );
    assert_eq!((p.tracks[0].track_vol, p.tracks[1].track_vol), (128, 256));
    assert_eq!(
        (&p.hold[..2], &p.decay[..2], p.hold[5]),
        (&[3, 7][..], &[5, 9][..], 99)
    );
}

#[test]
fn iff_octave_samples_take_the_octaves_part_by_the_row_of_their_kind() {
    // Rows of the synthetic tables: shift = row % 3, multiplier = row % 5, note = 12 (row % 4).
    let mut md = one_line([cell(13, 1, 0, 0), cell(25, 2, 0, 0), EMPTY, EMPTY]);
    md.insts = vec![
        Inst {
            kind: 1,
            ..Inst::sample(62, 2, 4)
        },
        Inst {
            kind: 2,
            ..Inst::sample(70, 2, 4)
        },
    ];
    let (mut p, mut m) = playing(&md);
    ticks(&mut p, &mut m, 1);
    let periods = tables().periods;
    // Five octaves: 62 / 31 = 2 units; note 12 = octave 1, row 1: start 2 × 1, loop 2 << 1 words.
    let s0 = sample_data(&m, 0);
    assert_eq!(
        (p.tracks[0].repeat, p.tracks[0].repeat_len),
        (s0 + 2 + 8, 8)
    );
    assert_eq!(m.paula.channels[0].period, periods[24 + 12]);
    // Three octaves: 70 / 7 = 10 units; note 24 = octave 2, row 6 + 2: start 10 × 3, shift 2.
    let s1 = sample_data(&m, 1);
    assert_eq!(
        (p.tracks[1].repeat, p.tracks[1].repeat_len),
        (s1 + 30 + 16, 16)
    );
    assert_eq!(m.paula.channels[1].period, periods[24]);
}

/// A song pointer past the top of the address space: 0x100fa4 (`movea.l 8(a0),a0`, then
/// `move.b $301(a0)`) reads the line speed from where the sum wraps to, inside chip memory, not
/// from past its end (0) as a sum in a wider integer would.
#[test]
fn a_song_pointer_past_the_top_wraps_into_chip_memory() {
    let mut bytes = one_line([cell(13, 1, 0, 0), EMPTY, EMPTY, EMPTY]).build();
    let bad = 0xffff_fe00u32;
    bytes[8..12].copy_from_slice(&bad.to_be_bytes());
    let song = CHIP_BUFFER.wrapping_add(bad);
    let at = song.wrapping_add(module::song::TEMPO2) as usize;
    assert!(at < bytes.len(), "the wrapped address is in the module");
    bytes[at - CHIP_BUFFER as usize] = 0x2a;
    let (p, m) = playing_bytes(&bytes);
    let md = module::Module { base: CHIP_BUFFER };
    assert_eq!(md.song(module::Mem(&m.chip)), song);
    assert_eq!(p.counter, 0x2a, "the line speed from the wrapped address");
}

/// Stored pointers of a malformed module can hold anything: the addresses the routine forms from
/// them wrap as the 68000's do (0x100fa4: `movea.l 8(a0),a0`, then `move.b $301(a0)`), reads
/// outside chip memory get 0, and nothing panics (debug builds check overflow). Each pointer
/// made absolute is checked against the 32-bit wrapping sum, so a sum that saturates, stops or
/// widens fails in a release build too; a plain `+` gives the same bits there, which the lint of
/// [`module`] rejects.
#[test]
fn pointers_anywhere_in_a_malformed_module_play_without_a_panic() {
    let hybrid = synth::synth(&[0x40, 0xff], &[0x00, 0xff], (1, 1), &[vec![1; 8]]);
    let mut md = one_line([
        cell(13, 1, 0, 0),
        cell(1, 2, 3, 4),
        cell(25, 1, 0xc, 0x20),
        cell(2, 2, 0xf, 0xf1),
    ]);
    md.insts = vec![Inst::sample(64, 4, 8), Inst { kind: -2, ..hybrid }];
    md.ext = vec![(1, 2), (3, 4)];
    let good = md.build();
    let ptr = |at: usize| u32::from_be_bytes(good[at..at + 4].try_into().unwrap()) as usize;
    let (blocks, samples, exp) = (ptr(0x10), ptr(0x18), ptr(0x20));
    // The header's four, block 0, both instruments, the extensions, the hybrid's waveform 0.
    let fields = [
        0x08,
        0x10,
        0x18,
        0x20,
        blocks,
        samples,
        samples + 4,
        exp + 4,
        ptr(samples + 4) + 0x116,
    ];
    // The hybrid's waveform pointers are relative to the instrument, the others to the module.
    let (hybrid, wave) = (ptr(samples + 4), fields[8]);
    let md = module::Module { base: CHIP_BUFFER };
    for at in fields {
        for bad in [0xffff_fe00u32, 0xffff_ffe0, 0x7fff_ffff] {
            let mut bytes = good.clone();
            bytes[at..at + 4].copy_from_slice(&bad.to_be_bytes());
            let (mut p, mut m) = playing_bytes(&bytes);
            let mem = module::Mem(&m.chip);
            let (formed, from) = if at == wave {
                let instr = CHIP_BUFFER + hybrid as u32;
                (module::Module::waveform(mem, instr, 0), instr)
            } else {
                (md.ptr(mem, CHIP_BUFFER + at as u32), CHIP_BUFFER)
            };
            assert_eq!(
                formed,
                from.wrapping_add(bad),
                "pointer at {at:#x} = {bad:#x}"
            );
            let mut out = vec![0.0; 4096];
            m.render(&mut p, &mut out, 8000);
            assert!(
                out.iter().all(|v| v.is_finite()),
                "pointer at {at:#x} = {bad:#x}"
            );
        }
    }
}

/// The routine indexes its tables of pointers with a word: `4 × n` in a word register, taken as
/// a sign-extended word (`lsl.w #2` at 0x1008c8, `movea.l 0(a0,d0.w)` at 0x1008ca). Block
/// 0x2001 is the long 0x7ffc bytes before the block array, not 0x8004 bytes after it; block
/// 0x4001 wraps to the array's second long. A synth's waveform byte is widened with `ext.w`
/// (0x1004a4) before the same scaling.
#[test]
fn table_indexes_are_sign_extended_words() {
    assert_eq!(module::long_index(0x1_0000, 0x2001), 0x8004);
    assert_eq!(module::long_index(0x1_0000, 0x4001), 0x1_0004);
    assert_eq!(module::long_index(0, 0xffff), 0xffff_fffc);
    assert_eq!(module::long_index(0x20, 0x1fff), 0x20 + 0x7ffc);

    let md = module::Module { base: CHIP_BUFFER };
    let mut chip = vec![0u8; 0x2_0000];
    let put = |chip: &mut Vec<u8>, at: u32, v: u32| {
        chip[at as usize..at as usize + 4].copy_from_slice(&v.to_be_bytes());
    };
    // The block array at module offset 0x1_0000 (stored relative to the module).
    put(&mut chip, CHIP_BUFFER + 0x10, 0x1_0000);
    put(&mut chip, CHIP_BUFFER + 0x1_0000 - 0x7ffc, 0x1234);
    put(&mut chip, CHIP_BUFFER + 0x1_0000 + 0x8004, 0x5678);
    put(&mut chip, CHIP_BUFFER + 0x1_0000 + 4, 0x9abc);
    let mem = module::Mem(&chip);
    assert_eq!(md.block(mem, 0x2001), CHIP_BUFFER + 0x1234);
    assert_eq!(md.block(mem, 0x4001), CHIP_BUFFER + 0x9abc);

    // Waveform byte 0xff: `ext.w` makes it −1, the long before waveform 0.
    let instr = 0x100;
    put(&mut chip, instr + module::instr::WF - 4, 0x40);
    put(&mut chip, instr + module::instr::WF + 0x3fc, 0x80);
    let mem = module::Mem(&chip);
    assert_eq!(module::Module::waveform(mem, instr, 0xff), instr + 0x40);
}

/// A line's notes start `3 × line × tracks` bytes into the block, a product the interrupt forms
/// in a word and adds as a sign-extended word (`adda.w` at 0x1008e8, `lea 2(a2,d0.w)` at
/// 0x100b72): `3 × line` wraps in 16 bits, and a product of 0x8000 or more reaches back before
/// the block.
#[test]
fn a_line_offset_is_a_sign_extended_word() {
    assert_eq!(module::line_at(0x100, 1, 2), 0x108);
    assert_eq!(module::line_at(0x1_0000, 171, 64), 0x1_0002 - 0x7fc0);
    assert_eq!(
        module::line_at(0, 0x5556, 1),
        2 + 2,
        "3 × 0x5556 wraps to 2"
    );
    assert_eq!(
        module::line_at(0, 0x2aab, 1),
        2u32.wrapping_sub(0x7fff),
        "3 × 0x2aab = 0x8001, the word −0x7fff"
    );
}

/// A malformed block of 64 tracks at line 171: the notes the interrupt reads (0x1008e8) and the
/// next line it looks ahead into for held notes (0x100b72) lie 0x7fc0 and 0x7f00 bytes before
/// the block's notes, not 0x8040 and 0x8100 bytes after them.
#[test]
fn line_171_of_a_64_track_block_reads_before_the_block() {
    let mut md = one_line([EMPTY; 4]);
    md.ext = vec![(2, 0)];
    let mut bytes = md.build();
    let block = 0x9000usize;
    let notes = block + 2;
    bytes.resize(notes + 0x8100 + 0x100, 0);
    let blockarr = u32::from_be_bytes(bytes[0x10..0x14].try_into().unwrap()) as usize;
    bytes[blockarr..blockarr + 4].copy_from_slice(&(block as u32).to_be_bytes());
    bytes[block..block + 2].copy_from_slice(&[64, 255]);
    // Line 171 and the next one where the 68000 reads them, and where 32-bit sums would.
    bytes[notes - 0x7fc0..][..3].copy_from_slice(&cell(13, 1, 0, 0));
    bytes[notes - 0x7f00..][..3].copy_from_slice(&cell(0, 1, 0, 0));
    bytes[notes + 0x8040..][..3].copy_from_slice(&cell(25, 1, 0, 0));
    let (mut p, mut m) = playing_bytes(&bytes);
    p.line = 171;
    ticks(&mut p, &mut m, 1);
    assert_eq!(p.line, 172);
    assert_eq!(m.paula.channels[0].period, period(13), "line 171's note");
    assert_eq!(
        p.tracks[0].hold_count, 4,
        "hold 2, extended by line 172's instrument (+3), one tick spent"
    );
}

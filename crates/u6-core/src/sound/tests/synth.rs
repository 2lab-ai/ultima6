//! Synth sounds: the volume and waveform sequences of a synthetic synth instrument.

use super::*;

/// A synth instrument: sequences (each ≤ 128 bytes), speeds, waveforms.
pub(super) fn synth(voltbl: &[u8], wftbl: &[u8], speeds: (u8, u8), waves: &[Vec<u8>]) -> Inst {
    // Bytes after the 6-byte length/type header.
    let mut d = vec![0u8; 0x116 - 6 + 4 * waves.len()];
    d[0x12 - 6] = speeds.0;
    d[0x13 - 6] = speeds.1;
    d[0x14 - 6..0x16 - 6].copy_from_slice(&(waves.len() as u16).to_be_bytes());
    d[0x16 - 6..0x16 - 6 + voltbl.len()].copy_from_slice(voltbl);
    d[0x96 - 6..0x96 - 6 + wftbl.len()].copy_from_slice(wftbl);
    for (n, w) in waves.iter().enumerate() {
        let at = d.len() + 6;
        d[0x116 - 6 + 4 * n..0x116 - 6 + 4 * n + 4].copy_from_slice(&(at as u32).to_be_bytes());
        d.extend(((w.len() / 2) as u16).to_be_bytes());
        d.extend(w);
    }
    Inst {
        data: d,
        kind: -1,
        rep: 0,
        replen: 0,
        transpose: 0,
    }
}

fn one_note(inst: Inst, note: u8) -> Mmd {
    let mut md = Mmd::new(vec![vec![[cell(note, 1, 0, 0), EMPTY, EMPTY, EMPTY]; 1]]);
    md.insts = vec![inst];
    md.tempo2 = 32;
    md
}

#[test]
fn a_synth_note_runs_its_volume_sequence_on_the_first_waveform() {
    let inst = synth(
        &[0x40, 0x30, 0xff],
        &[0x01, 0xff],
        (1, 1),
        &[vec![1; 8], vec![2; 16]],
    );
    let (mut p, mut m) = playing(&one_note(inst, 13));
    ticks(&mut p, &mut m, 1);
    let c = &m.paula.channels[0];
    assert!(c.playing());
    assert_eq!(c.period, tables().periods[12], "synth period table");
    assert_eq!(c.volume, 0x30, "init step 0x40, tick-0 step 0x30");
    assert_eq!(c.length, 8, "waveform 1: 16 bytes");
    // Waveform 1 follows waveform 0 (2-byte length + 8 bytes) after the two pointers.
    let wf1 = sample_data(&m, 0) - 6 + 0x116 + 8 + 10;
    assert_eq!(c.location, wf1 + 2);
    ticks(&mut p, &mut m, 3);
    assert_eq!(m.paula.channels[0].volume, 0x30, "END holds");
}

#[test]
fn wait_and_change_commands_pace_the_sequences() {
    // Volume: 16, wait 2 (the counter runs out on the second step: `subq.b`, `ble`), 32.
    // Waveform: wf 0, then CHD 4: the change applies from the next step on (0x10046c runs
    // before the sequence is read).
    let inst = synth(
        &[0x10, 0xf1, 0x02, 0x20, 0xff],
        &[0x00, 0xf2, 0x04, 0xff],
        (1, 1),
        &[vec![1; 8]],
    );
    let (mut p, mut m) = playing(&one_note(inst, 13));
    let mut vols = Vec::new();
    let mut pers = Vec::new();
    for _ in 0..5 {
        ticks(&mut p, &mut m, 1);
        vols.push(m.paula.channels[0].volume);
        pers.push(m.paula.channels[0].period);
    }
    assert_eq!(vols, [0x10, 0x10, 0x20, 0x20, 0x20]);
    let base = tables().periods[12];
    assert_eq!(pers, [base, base + 4, base + 8, base + 12, base + 16]);
}

#[test]
fn a_sequence_jumping_onto_itself_ends_the_step() {
    let inst = synth(&[0xfe, 0x00], &[0x00, 0xfe, 0x01], (1, 1), &[vec![1; 8]]);
    let (mut p, mut m) = playing(&one_note(inst, 13));
    ticks(&mut p, &mut m, 3);
    assert!(m.paula.channels[0].playing());
}

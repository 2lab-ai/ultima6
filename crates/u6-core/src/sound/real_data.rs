//! Real-data receipts: the user's archive.org zip → u6-intake → `Files`, then the player's tables
//! from `game`, every song file through the MED routine. Skips where the zip is missing; prints
//! numbers only (no game bytes go into the repo).
#![allow(clippy::disallowed_methods)] // tests may read files

use u6_intake::{InputFile, intake};
use u6_testkit::assert_same;

use super::module::{Mem, Module, instr, song};
use super::tables::{self, MedTables, SAMPLE_PERIODS};
use super::{AudioDriver, CHIP_BUFFER, CHIP_EFFECT, MedCall, SoundState, sound_on};
use crate::files::Files;
use crate::image::GameImage;
use crate::state::State;
use u6_testkit::Quiet;

const RATE: u32 = 48_000;

fn files() -> Option<Files> {
    if !u6_testkit::archive().exists() {
        eprintln!("skipped: the archive (U6_ARCHIVE) not present");
        return None;
    }
    let data = std::fs::read(u6_testkit::archive()).quiet();
    let r = intake(vec![InputFile {
        name: "ultima-vi.zip".into(),
        data,
    }]);
    Some(Files::new(r.files))
}

fn render(d: &mut AudioDriver, secs: usize) -> Vec<f32> {
    let mut out = vec![0f32; secs * RATE as usize * 2];
    for c in out.chunks_mut(960) {
        d.render(c, RATE);
    }
    out
}

#[test]
fn the_players_tables_and_the_effect_module_come_out_of_game() {
    let Some(files) = files() else { return };
    let img = GameImage::parse(files.get("game").quiet()).quiet();
    let t = MedTables::from_image(&img).quiet();
    let s = usize::from(SAMPLE_PERIODS);
    // One octave up halves the period.
    let ratio = f64::from(t.periods[s]) / f64::from(t.periods[s + 12]);
    assert!((ratio - 2.0).abs() < 0.01, "one octave halves the period");
    let fx = tables::effect_module(&img).quiet();
    let m = Mem(&fx);
    let md = Module { base: 0 };
    let kind = m.w(md.sample(m, 0) + instr::TYPE) as i16;
    // The same field read here from the raw module (MMD0: the BE instrument array pointer at
    // 0x18, its first BE pointer, the BE type word at +4): one of MED's instrument types
    // (-2 hybrid, -1 synth, 0 sample, 1..3 IFF octaves).
    let be32 = |at: usize| u32::from_be_bytes(fx[at..at + 4].try_into().quiet()) as usize;
    let first = be32(be32(0x18));
    let raw = i16::from_be_bytes([fx[first + 4], fx[first + 5]]);
    assert_same!([kind], [raw], "the type of instrument 1");
    assert!((-2..=3).contains(&kind), "a MED instrument type");
}

#[test]
fn every_song_name_is_an_mmd0_file_on_the_disks() {
    let Some(files) = files() else { return };
    let snd = SoundState::load(&files).quiet();
    assert!(snd.names.len() == usize::from(tables::SONGS));
    assert!((snd.current, snd.loaded) == (0, 0xffff));
    for n in 1..=tables::SONGS {
        let name = snd.file_name(n).quiet();
        let bytes = files
            .get(name)
            .unwrap_or_else(|| panic!("the file of song {n} missing"));
        assert_same!(bytes[..4], b"MMD0", "the MED tag of song {n}");
        assert!(
            bytes.len() <= super::MODULE_BUFFER,
            "song {n} fits the buffer"
        );
    }
}

#[test]
fn every_song_starts_with_its_first_rows_notes_and_keeps_playing() {
    let Some(files) = files() else { return };
    let snd = SoundState::load(&files).quiet();
    // Each song file once, by the number of its first song (the names are the user's).
    let mut songs: Vec<(u16, &str)> = Vec::new();
    for n in 1..=tables::SONGS {
        let name = snd.file_name(n).quiet();
        if !songs.iter().any(|&(_, m)| m == name) {
            songs.push((n, name));
        }
    }
    for (n, name) in songs {
        let mut d = AudioDriver::new(&files).quiet();
        d.apply(&MedCall::Load(name.into()), &files);
        d.apply(&MedCall::PlayBuffer, &files);
        // First tick = line 0 of the first block: a channel starts where the row has a note.
        let mut out = vec![0f32; 2 * 2000];
        d.render(&mut out, RATE);
        let mem = Mem(&d.machine.chip);
        let md = Module { base: CHIP_BUFFER };
        let s = md.song(mem);
        let blk = md.block(mem, u16::from(mem.b(s + song::PLAYSEQ)));
        for ch in 0..4u32 {
            let has_note = mem.b(blk + 2 + 3 * ch) & 0x3f != 0;
            let cmd = mem.b(blk + 3 + 3 * ch) & 0xf;
            if has_note && cmd != 3 {
                assert!(
                    d.machine.paula.channels[ch as usize].playing(),
                    "song {n} ch{ch}"
                );
            }
        }
        let a = render(&mut d, 5);
        let rms = (a.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / a.len() as f64).sqrt();
        assert!(rms > 0.01, "song {n} is audible");
    }
}

#[test]
fn rendering_is_deterministic_across_buffer_sizes() {
    let Some(files) = files() else { return };
    // Song 1 of the table the user's `game` names.
    let name = SoundState::load(&files).quiet().names[0].clone();
    let run = |chunk: usize| {
        let mut d = AudioDriver::new(&files).quiet();
        d.apply(&MedCall::Load(name.clone()), &files);
        d.apply(&MedCall::PlayBuffer, &files);
        let mut out = vec![0f32; RATE as usize * 4];
        for c in out.chunks_mut(chunk * 2) {
            d.render(c, RATE);
        }
        out
    };
    assert_same!(
        run(128),
        run(1000),
        "the samples rendered in 128- and 1000-frame chunks"
    );
}

#[test]
fn the_instrument_song_jumps_to_the_poked_position() {
    let Some(files) = files() else { return };
    let mut d = AudioDriver::new(&files).quiet();
    // The instrument song (song 13 of the table the user's `game` names).
    let snd = SoundState::load(&files).quiet();
    let name = snd
        .file_name(super::SONG_INSTRUMENT)
        .expect("the instrument song");
    d.apply(&MedCall::Load(name.to_string()), &files);
    // 0x12c8b8 stores the key's number at +0x3d2 (line 0's B command argument) and restarts.
    d.apply(
        &MedCall::Poke {
            offset: 0x3d2,
            value: 3,
        },
        &files,
    );
    d.apply(&MedCall::PlayBuffer, &files);
    let mut out = vec![0f32; 2 * 2000];
    d.render(&mut out, RATE);
    assert!(d.player.pseq == 3);
    let a = render(&mut d, 2);
    assert!(a.iter().any(|x| x.abs() > 0.05), "the note sounds");
}

#[test]
fn the_effect_module_plays_and_stops_itself() {
    let Some(files) = files() else { return };
    let mut d = AudioDriver::new(&files).quiet();
    d.apply(&MedCall::PlayEffect, &files);
    assert!(d.player.module == Some(Module { base: CHIP_EFFECT }));
    let a = render(&mut d, 3);
    let first: f64 = a[..RATE as usize].iter().map(|x| f64::from(*x).abs()).sum();
    assert!(first > 100.0, "audible at first");
    assert!(d.player.state == 0, "FFE stopped it");
    assert!(d.machine.paula.dma_bits() == 0);
}

#[test]
fn the_shipped_save_has_sound_on() {
    let Some(files) = files() else { return };
    let s = State::new_game(&files).quiet();
    assert!(sound_on(&s));
    assert!(s.sound.names.len() == usize::from(tables::SONGS));
}

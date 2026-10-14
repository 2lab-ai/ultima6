//! Real-data receipts: the user's archive.org zip → u6-intake → `Files`, then the player's tables
//! from `game`, every song file through the MED routine. Skips where the zip is missing; prints
//! numbers only (no game bytes go into the repo).
#![allow(clippy::disallowed_methods)] // tests may read files

use std::path::Path;

use u6_intake::{InputFile, intake};
use u6_testkit::assert_same;

use super::module::{Mem, Module, instr, song};
use super::tables::{self, MedTables, SAMPLE_PERIODS};
use super::{AudioDriver, CHIP_BUFFER, CHIP_EFFECT, MedCall, SoundState, sound_on};
use crate::files::Files;
use crate::image::GameImage;
use crate::state::State;

const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";
const RATE: u32 = 48_000;

fn files() -> Option<Files> {
    if !Path::new(ZIP).exists() {
        eprintln!("skipped: {ZIP} not present");
        return None;
    }
    let data = std::fs::read(ZIP).unwrap();
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
    let img = GameImage::parse(files.get("game").unwrap()).unwrap();
    let t = MedTables::from_image(&img).unwrap();
    let s = usize::from(SAMPLE_PERIODS);
    // One octave up halves the period.
    let ratio = f64::from(t.periods[s]) / f64::from(t.periods[s + 12]);
    assert!((ratio - 2.0).abs() < 0.01, "{ratio}");
    let fx = tables::effect_module(&img).unwrap();
    let m = Mem(&fx);
    let md = Module { base: 0 };
    let kind = m.w(md.sample(m, 0) + instr::TYPE) as i16;
    println!(
        "effect module: {} bytes, instrument 1 type {kind}",
        fx.len()
    );
    assert_eq!(kind, -1, "a synth sound");
}

#[test]
fn every_song_name_is_an_mmd0_file_on_the_disks() {
    let Some(files) = files() else { return };
    let snd = SoundState::load(&files).unwrap();
    assert_eq!(snd.names.len(), usize::from(tables::SONGS));
    assert_eq!((snd.current, snd.loaded), (0, 0xffff));
    for n in 1..=tables::SONGS {
        let name = snd.file_name(n).unwrap();
        let bytes = files.get(name).unwrap_or_else(|| panic!("{name} missing"));
        assert_same!(bytes[..4], b"MMD0", "the MED tag of {name}");
        assert!(
            bytes.len() <= super::MODULE_BUFFER,
            "{name} fits the buffer"
        );
    }
}

#[test]
fn every_song_starts_with_its_first_rows_notes_and_keeps_playing() {
    let Some(files) = files() else { return };
    let snd = SoundState::load(&files).unwrap();
    let mut names: Vec<&str> = snd.names.iter().map(String::as_str).collect();
    names.sort_unstable();
    names.dedup();
    for name in names {
        let mut d = AudioDriver::new(&files).unwrap();
        d.apply(&MedCall::Load(name.into()), &files);
        d.apply(&MedCall::PlayBuffer, &files);
        // First tick = line 0 of the first block: a channel starts where the row has a note.
        let mut out = vec![0f32; 2 * 2000];
        d.render(&mut out, RATE);
        let mem = Mem(&d.machine.chip);
        let md = Module { base: CHIP_BUFFER };
        let s = md.song(mem);
        let blk = md.block(mem, u16::from(mem.b(s + song::PLAYSEQ)));
        let tracks = mem.b(blk);
        for ch in 0..4u32 {
            let has_note = mem.b(blk + 2 + 3 * ch) & 0x3f != 0;
            let cmd = mem.b(blk + 3 + 3 * ch) & 0xf;
            if has_note && cmd != 3 {
                assert!(
                    d.machine.paula.channels[ch as usize].playing(),
                    "{name} ch{ch}"
                );
            }
        }
        let a = render(&mut d, 5);
        let rms = (a.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / a.len() as f64).sqrt();
        println!(
            "{name}: {tracks} tracks, 5 s RMS {rms:.4}, state {}, line {}",
            d.player.state, d.player.line
        );
        assert!(rms > 0.01, "{name} is audible");
    }
}

#[test]
fn rendering_is_deterministic_across_buffer_sizes() {
    let Some(files) = files() else { return };
    let run = |chunk: usize| {
        let mut d = AudioDriver::new(&files).unwrap();
        d.apply(&MedCall::Load("ULTIMA_Aa.MOD".into()), &files);
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
    let mut d = AudioDriver::new(&files).unwrap();
    d.apply(&MedCall::Load("ULTIMA_Ha.MOD".into()), &files);
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
    assert_eq!(d.player.pseq, 3);
    let a = render(&mut d, 2);
    assert!(a.iter().any(|x| x.abs() > 0.05), "the note sounds");
}

#[test]
fn the_effect_module_plays_and_stops_itself() {
    let Some(files) = files() else { return };
    let mut d = AudioDriver::new(&files).unwrap();
    d.apply(&MedCall::PlayEffect, &files);
    assert_eq!(d.player.module, Some(Module { base: CHIP_EFFECT }));
    let a = render(&mut d, 3);
    let first: f64 = a[..RATE as usize].iter().map(|x| f64::from(*x).abs()).sum();
    assert!(first > 100.0, "audible at first");
    assert_eq!(d.player.state, 0, "FFE stopped it");
    assert_eq!(d.machine.paula.dma_bits(), 0);
}

#[test]
fn the_shipped_save_has_sound_on() {
    let Some(files) = files() else { return };
    let s = State::new_game(&files).unwrap();
    assert!(sound_on(&s));
    assert_eq!(s.sound.names.len(), 13);
}

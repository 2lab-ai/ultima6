//! The music code (0x110878, 0x110912, the toggle) and the driver's replay of its calls.

use std::collections::BTreeMap;

use super::*;
use crate::files::Files;
use crate::sound::{
    AudioDriver, CALL_LOG, MedCall, SONG_INSTRUMENT, SoundState, play_area_song, play_song,
    set_sound_flag, sound_on, toggle_sound,
};
use crate::state::State;

/// A state with a song table (names "S1".."S13"), the objlist block and one party member.
fn state(sound: bool) -> State {
    let mut s = State::empty();
    s.sound = SoundState {
        loaded: 0xffff,
        names: (1..=13).map(|n| format!("S{n}")).collect(),
        ..SoundState::default()
    };
    s.actors.objlist_tail = vec![0; 0x82];
    s.objects.type_frame = vec![0; 256];
    s.actors.party.size = 1;
    s.actors.party.members[0] = 1;
    set_sound_flag(&mut s, sound);
    s
}

fn calls(s: &State) -> Vec<MedCall> {
    s.sound.calls.iter().map(|(_, c)| c.clone()).collect()
}

fn load(n: u16) -> MedCall {
    MedCall::Load(format!("S{n}"))
}

#[test]
fn the_sound_flag_is_the_objlist_word_at_0x28() {
    let mut s = state(false);
    s.actors.objlist_tail[0x29] = 1;
    assert!(sound_on(&s));
    set_sound_flag(&mut s, false);
    assert_eq!(&s.actors.objlist_tail[0x28..0x2a], &[0, 0]);
}

#[test]
fn play_song_loads_once_plays_and_stops_the_previous_song() {
    let mut s = state(true);
    play_song(&mut s, 2);
    assert_eq!(calls(&s), [load(2), MedCall::PlayBuffer]);
    assert_eq!((s.sound.current, s.sound.loaded), (2, 2));
    play_song(&mut s, 2);
    assert_eq!(s.sound.calls.len(), 2, "same song: nothing");
    play_song(&mut s, 0);
    play_song(&mut s, 2);
    assert_eq!(
        calls(&s)[2..],
        [MedCall::Stop, MedCall::PlayBuffer],
        "file still in the buffer"
    );
    play_song(&mut s, 7);
    assert_eq!(
        calls(&s)[4..],
        [MedCall::Stop, load(7), MedCall::PlayBuffer]
    );
}

#[test]
fn with_sound_off_only_the_instrument_song_loads_and_it_never_starts() {
    let mut s = state(false);
    play_song(&mut s, 3);
    assert!(s.sound.calls.is_empty() && s.sound.current == 0);
    play_song(&mut s, SONG_INSTRUMENT);
    assert_eq!(calls(&s), [load(13)]);
    assert_eq!(s.sound.current, SONG_INSTRUMENT);
    play_song(&mut s, 0);
    assert_eq!(calls(&s)[1..], [MedCall::Stop]);
}

#[test]
fn the_area_song_depends_on_level_vehicle_and_combat() {
    let pick = |z: u8, kind: u16, combat: bool| {
        let mut s = state(true);
        s.map.centre.z = z;
        s.objects.type_frame[1] = kind;
        s.actors.objlist_tail[0x78] = u8::from(combat);
        s.rng = crate::rng::Rng::seeded(0x1234);
        let mut r = s.rng;
        play_area_song(&mut s);
        (s.sound.current, r.range(0, 3), s.rng.state() == r.state())
    };
    let (song, _, drew) = pick(5, 0, false);
    assert_eq!((song, drew), (11, false), "no draw on level 5");
    assert_eq!(pick(2, 0, false).0, 9);
    let (song, draw, drew) = pick(0, 0, false);
    assert!(drew, "one draw 0x151f06(0, 3) on the surface");
    assert_eq!(song, [8, 1, 5, 10][usize::from(draw)]);
    assert_eq!(pick(0, 0x19c, false).0, 6);
    assert_eq!(pick(3, 0x19e, false).0, 6);
    assert_eq!(pick(0, 0x19c, true).0, 7);
}

#[test]
fn the_area_song_with_sound_off_does_nothing_and_draws_nothing() {
    let mut s = state(false);
    let before = s.rng.state();
    play_area_song(&mut s);
    assert!(s.sound.calls.is_empty());
    assert_eq!(s.rng.state(), before);
}

#[test]
fn control_z_toggles_flag_and_music() {
    let mut s = state(false);
    s.map.centre.z = 1;
    assert!(toggle_sound(&mut s));
    assert!(sound_on(&s));
    assert_eq!(
        (s.sound.current, calls(&s)),
        (9, vec![load(9), MedCall::PlayBuffer])
    );
    assert!(!toggle_sound(&mut s));
    assert!(!sound_on(&s) && s.sound.current == 0);
    assert_eq!(calls(&s).last(), Some(&MedCall::Stop));
}

#[test]
fn the_call_log_keeps_the_last_calls() {
    let mut s = SoundState::default();
    for _ in 0..CALL_LOG + 5 {
        s.call(MedCall::Stop);
    }
    assert_eq!(s.calls.len(), CALL_LOG);
    assert_eq!(s.calls.front().map(|c| c.0), Some(5));
    assert_eq!(s.next_call, CALL_LOG as u64 + 5);
}

#[test]
fn the_driver_replays_new_calls_once_into_the_buffer() {
    let md = Mmd::new(vec![vec![[cell(13, 1, 0, 0), EMPTY, EMPTY, EMPTY]; 2]]);
    let bytes = md.build();
    let files = Files::new(BTreeMap::from([("S2".to_string(), bytes.clone())]));
    let mut d = AudioDriver::with_tables(tables(), b"MMD0");
    let mut s = state(true);
    play_song(&mut s, 2);
    s.sound.call(MedCall::Poke {
        offset: 4,
        value: 0xee,
    });
    d.sync(&s.sound, &files);
    let at = CHIP_BUFFER as usize;
    assert_eq!(&d.machine.chip[at..at + 4], b"MMD0");
    assert_eq!(d.machine.chip[at + 4], 0xee);
    assert!(d.player.module.is_some() && d.machine.timer.running());
    d.machine.chip[at + 4] = 0;
    d.sync(&s.sound, &files);
    assert_eq!(d.machine.chip[at + 4], 0, "already applied");
    let mut out = vec![0.0; 9600];
    d.render(&mut out, 48_000);
    assert!(out.iter().any(|&x| x.abs() > 0.01), "the note sounds");
}

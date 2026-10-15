//! The music code of hunk 7 and the other callers' entry points into the MED routine.

use super::MedCall;
use crate::state::State;

/// Song 13 (its file name is the pointer at A4-0x779a + 4 · 13, the 13th of the song table
/// A4-0x7796): the instrument notes. 0x110878 loads it even with sound off and does not start
/// it; the instrument Use step plays it per key.
pub const SONG_INSTRUMENT: u16 = 0xd;
/// 0x110878's read length into the module buffer (`move.l #$c350`).
pub const MODULE_BUFFER: usize = 50_000;
/// A4-0x7b5c, the sound-on word, is objlist block bytes 0x28..0x29 (A4-0x7b84 + 0x28;
/// objlist 0x1C19, saved with the game).
const TAIL_SOUND: usize = 0x28;
/// Leader types that get the travel song: 0x19c ship, 0x19e (the other vehicle 0x110912 tests).
const TRAVEL_TYPES: [u16; 2] = [0x19c, 0x19e];

/// Original: A4-0x7b5c (word: sound on; control-Z toggles it)
pub fn sound_on(s: &State) -> bool {
    s.actors.tail_word(TAIL_SOUND) != 0
}

/// Writes the sound-on word (A4-0x7b5c).
pub fn set_sound_flag(s: &mut State, on: bool) {
    s.actors.set_tail_word(TAIL_SOUND, u16::from(on));
}

/// Original: 0x110878 (sound: play song N, 0 = stop)
///
/// Same song: nothing. Otherwise the playing song stops; song `n` then starts if sound is on
/// (song 13 is loaded regardless and never started here). The file is read only when the
/// buffer does not hold it already. The module buffer A4-0x35cc is allocated at setup (46390
/// bytes at 0x14ef76, only with enough memory free; without one it reads and plays nothing,
/// 0x1108aa); the port always has one, of the read's length ([`MODULE_BUFFER`], 0x1108d6), so
/// a file longer than 46390 bytes does not overflow it as it would the original's.
pub fn play_song(s: &mut State, n: u16) {
    if n == s.sound.current {
        return;
    }
    if s.sound.current != 0 {
        s.sound.call(MedCall::Stop);
        s.sound.current = 0;
    }
    if n == 0 || !(sound_on(s) || n == SONG_INSTRUMENT) {
        return;
    }
    let snd = &mut s.sound;
    if snd.loaded != n {
        // A song past the table (no caller passes one) reads no file; the buffer stays.
        if let Some(name) = snd.file_name(n).map(str::to_owned) {
            snd.call(MedCall::Load(name));
        }
        snd.loaded = n;
    }
    if n != SONG_INSTRUMENT {
        snd.call(MedCall::PlayBuffer);
    }
    snd.current = n;
}

/// Original: 0x110912 (sound: pick a song for the current area and play it)
///
/// With sound on: the gargoyle world (level 5) song 11, the surface one of songs 8, 1, 5, 10
/// (one RNG draw 0x151f06(0, 3)), the dungeons song 9; a leader of type 0x19c / 0x19e song 6;
/// combat mode song 7.
pub fn play_area_song(s: &mut State) {
    if !sound_on(s) {
        return;
    }
    let mut song = match s.map.centre.z {
        5 => 0xb,
        0 => match s.rng.range(0, 3) {
            0 => 8,
            1 => 1,
            2 => 5,
            _ => 0xa,
        },
        _ => 9,
    };
    let kind = s
        .actors
        .party
        .leader()
        .map_or(0, |id| s.objects.obj_type(id));
    if TRAVEL_TYPES.contains(&kind) {
        song = 6;
    }
    if s.actors.combat_mode() {
        song = 7;
    }
    play_song(s, song);
}

/// The control-Z branch of the command loop 0x1041d8 (code 0x1a) without its message: on →
/// flag, area song; off → song 0, flag. Returns whether sound is now on (the caller prints
/// the on or off line).
pub fn toggle_sound(s: &mut State) -> bool {
    if sound_on(s) {
        play_song(s, 0);
        set_sound_flag(s, false);
        false
    } else {
        set_sound_flag(s, true);
        play_area_song(s);
        true
    }
}

/// A byte store into the module buffer at `offset` (the instrument Use step 0x12c8b8 stores
/// the key's note number at +0x3d2 before each restart).
pub fn poke_module_buffer(s: &mut State, offset: u32, value: u8) {
    s.sound.call(MedCall::Poke { offset, value });
}

/// PlayModule on the module buffer (0x13109c with A4-0x35cc, as 0x12c908 calls it).
pub fn play_module_buffer(s: &mut State) {
    s.sound.call(MedCall::PlayBuffer);
}

/// PlayModule on the sound-effect module (0x12d078 `lea $1512c8,a0` + 0x13109c). The caller
/// stops the music first (0x110878(0)) and restores it after its wait, as 0x12cfe4 does.
pub fn play_effect_module(s: &mut State) {
    s.sound.call(MedCall::PlayEffect);
}

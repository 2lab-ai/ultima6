//! Tables the MED player and the music code read, taken from the user's `game` at run time
//! (spec 03 §5): image addresses are hunk 0 (the MED routine) and hunks 7, 28 and 33.

use crate::image::{A4, GameImage};
use u6_formats::FormatError;

/// 0x101248: the synth note → period table (96 words); the sample table 0x101278 is the same
/// memory 24 words on (`$42(a5)` points at one or the other).
const PERIODS: u32 = 0x10_1248;
/// Entries of the synth table before the sample table starts.
pub const SAMPLE_PERIODS: u16 = 24;
const PERIOD_COUNT: usize = 96;
/// 0x1005d0: 32-entry signed sine for vibrato (effect 4 and synth vibrato).
const SINE: u32 = 0x10_05d0;
/// 0x100e58: CIA timer values for the SoundTracker-compatible tempos 1..10.
const ST_TEMPOS: u32 = 0x10_0e58;
/// 0x100e6c: PAL timer divisor (tempo > 10: timer = divisor / tempo); 0x10102a replaces it
/// with 474326 on a machine whose ExecBase says 60 Hz.
const TIMER_DIV: u32 = 0x10_0e6c;
/// 0x100eec / 0x100ef8 / 0x100f04: per-octave shift, unit multiplier and note offset for IFF
/// multi-octave samples (5-octave rows 0..5, 3-octave rows 6..11).
const OCTAVES: u32 = 0x10_0eec;
/// 0x12d07a: the `lea $1512c8,a0` operand before 0x12d07e's PlayModule: the sound-effect module
/// in hunk 28 (chip DATA), relocated at setup (0x14f09c → 0x15355a).
const EFFECT_LEA_OPERAND: u32 = 0x12_d07a;
/// A4-0x7796: song file name pointers for songs 1..; A4-0x779a is the current song word.
const SONG_NAMES: i32 = -0x7796;
/// Songs the table holds (1..=13; 0x110878 never indexes past the largest caller's 13).
pub const SONGS: u16 = 13;

/// The MED routine's constant tables (all from hunk 0 of `game`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MedTables {
    pub periods: [u16; PERIOD_COUNT],
    pub sine: [i8; 32],
    pub st_tempos: [u16; 10],
    pub timer_div: u32,
    pub octave_shift: [u8; 12],
    pub octave_mult: [u8; 12],
    pub octave_note: [u8; 12],
}

fn bad(detail: String) -> FormatError {
    FormatError::Invalid {
        what: "game (MED tables)",
        offset: 0,
        detail,
    }
}

impl MedTables {
    /// Reads and checks the tables: periods fall within each octave, the sine peaks at 127.
    pub fn from_image(img: &GameImage) -> Result<Self, FormatError> {
        let words = |addr: u32, n: usize| -> Result<Vec<u16>, FormatError> {
            let b = img.bytes(addr, n * 2)?;
            Ok(b.chunks(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect())
        };
        let periods: [u16; PERIOD_COUNT] = words(PERIODS, PERIOD_COUNT)?.try_into().unwrap();
        if periods[..60].windows(2).any(|w| w[1] >= w[0]) || periods[95] == 0 {
            return Err(bad(format!(
                "period table at {PERIODS:#x} is not a period table"
            )));
        }
        let sine: [i8; 32] = img
            .bytes(SINE, 32)?
            .iter()
            .map(|&b| b as i8)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        if sine[8] != 127 || sine[0] != 0 {
            return Err(bad(format!("sine table at {SINE:#x} does not peak at 127")));
        }
        let st_tempos: [u16; 10] = words(ST_TEMPOS, 10)?.try_into().unwrap();
        let t = img.bytes(TIMER_DIV, 4)?;
        let timer_div = u32::from_be_bytes([t[0], t[1], t[2], t[3]]);
        let oct = img.bytes(OCTAVES, 36)?;
        let row = |n: usize| -> [u8; 12] { oct[n * 12..n * 12 + 12].try_into().unwrap() };
        Ok(Self {
            periods,
            sine,
            st_tempos,
            timer_div,
            octave_shift: row(0),
            octave_mult: row(1),
            octave_note: row(2),
        })
    }
}

/// The sound-effect module 0x12cfe4 plays (hunk 28): its bytes, `modlen` long.
pub fn effect_module(img: &GameImage) -> Result<Vec<u8>, FormatError> {
    let addr = img.pointer(EFFECT_LEA_OPERAND)?;
    let head = img.bytes(addr, 8)?;
    if &head[..4] != b"MMD0" {
        return Err(bad(format!("no MMD0 module at {addr:#x}")));
    }
    let len = u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as usize;
    Ok(img.bytes(addr, len)?.to_vec())
}

/// File names of songs 1..=13 (index 0 = song 1) from the pointer table at A4-0x7796.
pub fn song_names(img: &GameImage) -> Result<Vec<String>, FormatError> {
    (0..SONGS)
        .map(|n| {
            let at = A4.wrapping_add_signed(SONG_NAMES) + u32::from(n) * 4;
            let name = img.cstr(img.pointer(at)?)?;
            Ok(String::from_utf8_lossy(&name).into_owned())
        })
        .collect()
}

/// The initial words of the current-song and loaded-song globals (A4-0x779a, A4-0x7798).
pub fn song_globals(img: &GameImage) -> Result<(u16, u16), FormatError> {
    Ok((img.a4_word(-0x779a)?, img.a4_word(-0x7798)?))
}

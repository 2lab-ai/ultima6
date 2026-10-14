//! Sound: the game's music code (hunk 7) and the MED replay routine linked into hunk 0, driving
//! an emulated Amiga 500 audio machine (`u6-audio`).
//!
//! Two halves, because the engine never performs I/O:
//!
//! - **Game side, in [`State`](crate::state::State)** ([`SoundState`]): 0x110878 play song N, 0x110912 the area song,
//!   the control-Z toggle, and the Use-command calls into the player. They change the song
//!   globals and append the calls the original makes into the MED routine ([`MedCall`]: stop,
//!   load a file into the module buffer, poke the buffer, play the buffer or the sound-effect
//!   module). That is all the game ever does with sound; nothing reads the player back.
//! - **Player side, host-owned** ([`AudioDriver`]): the MED routine ([`MedPlayer`]) on a
//!   [`u6_audio::Machine`]. The host builds it from the user's files, replays new calls after
//!   each engine tick ([`AudioDriver::sync`]) and pulls samples ([`AudioDriver::render`]).
//!
//! The song file names, the player's tables and the sound-effect module come from the user's
//! `game` (`tables.rs`); the songs are the user's `ULTIMA_?a.MOD` files (MMD0, OctaMED).
//!
//! Original functions this module owns:
//!
//! - 0x1001f8 [sound] MED: one channel off
//! - 0x10021e [sound] MED: all channels off, no module
//! - 0x100234 [sound] MED: play a note
//! - 0x10038a [sound] MED: synth step
//! - 0x100628 [sound] MED: start the DMA of new notes
//! - 0x100890 [sound] MED: timer interrupt (one tick)
//! - 0x100e30 [sound] MED: tempo → CIA timer
//! - 0x100e70 / 0x100e96 [sound] MED: sample part and period for a note
//! - 0x100f10 [sound] MED: track volumes, hold/decay table
//! - 0x100f7c / 0x10102a [sound] MED InitPlayer
//! - 0x100f96 / 0x101126 [sound] MED RemPlayer
//! - 0x100fa4 [sound] MED PlayModule
//! - 0x101010 [sound] MED StopPlayer
//! - 0x110878 [sound] play song N
//! - 0x110912 [sound] area song

use std::collections::VecDeque;

mod driver;
mod effects;
mod interrupt;
pub mod med;
pub mod module;
mod note;
mod player;
mod songs;
mod synth;
pub mod tables;

#[cfg(test)]
mod real_data;
#[cfg(test)]
mod tests;

pub use driver::{AudioDriver, CHIP_BUFFER, CHIP_EFFECT};
pub use med::MedPlayer;
pub use songs::{
    MODULE_BUFFER, SONG_INSTRUMENT, play_area_song, play_effect_module, play_module_buffer,
    play_song, poke_module_buffer, set_sound_flag, sound_on, toggle_sound,
};
pub use u6_audio::FilterModel;

use crate::files::Files;
use crate::image::GameImage;
use crate::state::LoadError;

/// Calls kept for the host (more than any game step makes between two engine ticks).
pub const CALL_LOG: usize = 64;

/// One call of the game into the MED routine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MedCall {
    /// 0x101010 StopPlayer (through 0x1109d2).
    Stop,
    /// 0x110878's load: the file read into the module buffer A4-0x35cc (at most
    /// [`MODULE_BUFFER`] bytes; bytes past the file keep what was there) and relocated
    /// (0x15355a).
    Load(String),
    /// A byte store into the module buffer (the instrument Use step 0x12c8b8 writes the key's
    /// digit at +0x3d2, a position-jump argument).
    Poke { offset: u32, value: u8 },
    /// 0x100fa4 PlayModule on the module buffer (0x134ec4, 0x13109c).
    PlayBuffer,
    /// 0x100fa4 PlayModule on the sound-effect module of hunk 28 (0x12d07e).
    PlayEffect,
}

/// The music code's globals and its calls into the player.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoundState {
    /// Original: A4-0x779a (word: the song playing, 0 = none)
    pub current: u16,
    /// Original: A4-0x7798 (word: the song whose file is in the module buffer, 0xffff = none)
    pub loaded: u16,
    /// Original: A4-0x7796 (pointers to the file names of songs 1..13; index 0 = song 1)
    pub names: Vec<String>,
    /// Port-only: the calls into the MED routine, numbered from 0 in call order; the oldest
    /// drop out past [`CALL_LOG`].
    pub calls: VecDeque<(u64, MedCall)>,
    /// Port-only: the number the next call gets.
    pub next_call: u64,
}

impl SoundState {
    /// The song globals' initial words and the name table, from the user's `game`.
    pub fn load(files: &Files) -> Result<Self, LoadError> {
        let game = files.get("game").ok_or_else(|| LoadError::Missing {
            file: "game".into(),
        })?;
        let format = |error| LoadError::Format {
            file: "game".into(),
            error,
        };
        let img = GameImage::parse(game).map_err(format)?;
        let (current, loaded) = tables::song_globals(&img).map_err(format)?;
        Ok(Self {
            current,
            loaded,
            names: tables::song_names(&img).map_err(format)?,
            ..Self::default()
        })
    }

    /// Appends a call for the host.
    pub fn call(&mut self, call: MedCall) {
        self.calls.push_back((self.next_call, call));
        self.next_call += 1;
        while self.calls.len() > CALL_LOG {
            self.calls.pop_front();
        }
    }

    /// Takes the call log over from `old`, the state of the process before (a new `game` after
    /// the launcher ran `u` again): the numbering goes on, so the host's driver, which counts the
    /// calls it applied, takes the new ones, and calls it has not synced yet still reach it.
    pub fn continue_calls(&mut self, old: &mut SoundState) {
        self.calls = std::mem::take(&mut old.calls);
        self.next_call = old.next_call;
    }

    /// File name of song `n` (1..=13).
    pub fn file_name(&self, n: u16) -> Option<&str> {
        self.names
            .get(usize::from(n).checked_sub(1)?)
            .map(String::as_str)
    }
}

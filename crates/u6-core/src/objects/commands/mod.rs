//! Object commands of hunk 15 (Look, Get, Drop, Move, search, doors, traps, light sources) and
//! the world helpers hunks 16-17 share with them (egg hatching, monster spawn, kegs, horses).
//!
//! Domain state = [`ObjCommands`]: the user's `game` image the commands read their strings and
//! tables from ([`GameData`]) and the few globals only these routines write. Everything else
//! they touch lives in the object, actor, map and ui tables.
//!
//! The command handlers are `async` (they print through the message scroll, which may wait on a
//! more prompt, and Drop / Move read keys); the world helpers on the walk path (egg hatching
//! 0x1319b2 and the monster spawn 0x131646 / 0x13121a) are synchronous and queue their text.
//!
//! Submodules: `data` (the image), `eggs`, `seams` (calls into modules not ported yet).

mod data;
mod eggs;
mod seams;
mod text;

#[cfg(test)]
mod real_data;
#[cfg(test)]
mod tests;

pub use data::GameData;
pub use eggs::{equip_monster, hatch_egg, spawn_monster};

use crate::files::Files;

/// Original: A4-0x7a26 / -0x7a25 initial bytes: the pixel position in the map view the
/// direction lookup 0x105016 measures from (Move 0x12ae04 writes it; the image holds the
/// centre cell's).
const POINTER_REF: i32 = -0x7a26;

/// State of the object commands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjCommands {
    /// The user's `game` as loaded (strings, tables); empty until [`ObjCommands::load`].
    pub data: GameData,
    /// Original: A4-0x7a26 / -0x7a25 (bytes: pixel x, y in the map view that 0x105016 measures
    /// directions from; written by Move 0x12ae04, read by the egg warning of 0x1319b2)
    pub pointer_ref: (u8, u8),
}

impl ObjCommands {
    /// Maps the user's `game` (spec 03 §5) and reads the initial globals from its data hunk.
    pub fn load(files: &Files) -> Result<Self, String> {
        let game = files.get("game").ok_or("not found in the dropped files")?;
        Ok(Self::from_data(GameData::from_game(game)?))
    }

    /// The state over an already mapped image.
    pub fn from_data(data: GameData) -> Self {
        let pointer_ref = (data.a4_byte(POINTER_REF), data.a4_byte(POINTER_REF + 1));
        Self { data, pointer_ref }
    }
}

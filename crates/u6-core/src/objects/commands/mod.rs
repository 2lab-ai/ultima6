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
//! Submodules: `data` (the image), `text` (output), `eggs`, `world` (doors, chests, search,
//! traps, kegs), `light` (torches, fixtures), `horse` (horses, the answer parser), `look`,
//! `get`, `drop`, `push` (the four commands), `seams` (calls into modules not ported
//! yet).

mod data;
mod drop;
mod eggs;
mod get;
mod horse;
mod keys;
mod light;
mod look;
mod push;
mod seams;
mod text;
mod world;

#[cfg(test)]
mod real_data;
#[cfg(test)]
mod tests;

pub use data::GameData;
pub use drop::{drop_object, fits_in, fragile, how_many, surface_at};
pub use eggs::{equip_monster, hatch_egg, spawn_monster};
pub use get::{get, takeable};
pub use horse::{dismount, mount_horse, parse_answer};
pub use keys::{fails_roll, in_type_list, pick_breaks, toggle_lit, use_key};
pub use light::{douse_or_light, light_torch, use_up, use_up_one};
pub use look::{is_book, is_readable, is_sign, look, read_book};
pub use push::{outermost, push, push_blocked};
pub use world::{
    article, blast_doors, chest, door, explode_keg, is_closed_container, name_prefix, object_under,
    search, spring_trap,
};

use crate::files::Files;

/// State of the object commands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjCommands {
    /// The user's `game` as loaded (strings, tables); empty until [`ObjCommands::load`].
    pub data: GameData,
}

impl ObjCommands {
    /// Maps the user's `game` (spec 03 §5) and reads the initial globals from its data hunk.
    pub fn load(files: &Files) -> Result<Self, String> {
        let game = files.get("game").ok_or("not found in the dropped files")?;
        Ok(Self::from_data(GameData::from_game(game)?))
    }

    /// The state over an already mapped image.
    pub fn from_data(data: GameData) -> Self {
        Self { data }
    }
}

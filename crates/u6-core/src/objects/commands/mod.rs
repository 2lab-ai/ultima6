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
//! `get`, `drop`, `push` (the four commands), `gatehouse` (port-only: the plants by the
//! gatehouse give its key on Look), `seams` (calls into modules not ported yet).

mod data;
pub(crate) mod drop;
mod eggs;
mod gatehouse;
pub(crate) mod get;
mod horse;
mod keys;
mod light;
mod look;
pub(crate) mod push;
mod seams;
mod text;
pub(crate) mod world;

#[cfg(test)]
pub(crate) mod real_data;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_gatehouse;

pub use data::GameData;
pub use drop::{drop_object, fits_in, fragile, how_many, surface_at};
pub use eggs::{equip_monster, hatch_egg, spawn_monster};
pub use get::{get, takeable};
pub use horse::{dismount, mount_horse, parse_answer};
pub use keys::{fails_roll, in_type_list, pick_breaks, toggle_lit, use_key};
pub use light::{douse_or_light, light_torch, use_up, use_up_one};
pub use look::{BOOK_BYTES, is_book, is_readable, is_sign, look, read_book};
pub use push::{push, push_blocked};
pub use world::{
    article, blast_doors, chest, door, explode_keg, is_closed_container, name_prefix, object_under,
    search, spring_trap,
};

use crate::files::Files;

/// Port-only (spec 08, slice K2): the strings the object commands pass to `printf` as its
/// format (Look, Get, Drop, Move, the keys, search, doors and traps; spec 16 item 1, 2026-10-27:
/// and the light sources', the horses' and the egg hatch's, which Use and the walk reach), for
/// the locale layer's catalogue ([`crate::locale::strings`]).
pub(crate) const PRINTED: [&[u32]; 9] = [
    look::PRINTED,
    get::PRINTED,
    drop::PRINTED,
    push::PRINTED,
    keys::PRINTED,
    world::PRINTED,
    light::PRINTED,
    horse::PRINTED,
    eggs::PRINTED,
];

/// Port-only (spec 08, 2026-10-25): the pointers of the reagents' names, A4-0x6d4c + 4 n for the
/// 8 reagent bits, labels of the locale layer's catalogue ([`crate::locale::strings`]): Look's
/// description of a spell in an open spellbook reads each it lists for display through
/// [`crate::locale::shown`] and prints it on a line of its own in the message window, which
/// wraps, so none has a cell budget. Nothing else reads the table.
pub(crate) const LABELS: &[u32] = &[
    reagent_pointer(0),
    reagent_pointer(1),
    reagent_pointer(2),
    reagent_pointer(3),
    reagent_pointer(4),
    reagent_pointer(5),
    reagent_pointer(6),
    reagent_pointer(7),
];

/// The pointer slot of reagent `n`'s name (bit n of a spell's reagent mask).
const fn reagent_pointer(n: u32) -> u32 {
    crate::image::A4.wrapping_sub(0x6d4c) + 4 * n
}

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

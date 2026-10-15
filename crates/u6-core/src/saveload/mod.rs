//! Save and restore (hunk 2): Save 0x105716, Restore 0x1052c8, load state 0x105350, and the
//! object-block write-back 0x10e4bc (WU M2-saveload).
//!
//! The original's save is its `savegame/` directory: `savegame/objlist` (the 25 tables of
//! [`objlist`]) and the 69 object blocks `savegame/objblk??`. While playing, a block leaving
//! the region goes to its `.tmp` file (objects 0x10d538, flag A4-0x78d8); Save writes the
//! region's blocks back, renames every current `.tmp` over its block file (0x10e4bc) and
//! writes `objlist`; Restore drops the `.tmp` files and reads the directory again (0x1052c8 →
//! 0x105350). Nothing else is written: the RNG, the screen and every cache are rebuilt or keep
//! running, so a port save is byte-compatible with the original's and the other way round.
//!
//! The port keeps the directory in memory: the block files are [`crate::objects::Objects`]'
//! `blocks` (the `.tmp` copies its `tmp`), `objlist` is [`SaveLoad::objlist`]. The platform
//! supplies the storage through [`SaveStore`]: at boot [`attach`] lays the stored files over
//! the user's (never into them), and after a tick [`SaveSync::flush`] hands it the files a
//! Save committed since the last flush. Saves therefore leave the engine only at the original
//! save boundary (spec 03 §2), as plain files under the names the original writes.
//!
//! Original functions this module owns:
//!
//! - 0x105716 [saveload] Save: write savegame/objlist + object blocks
//! - 0x1052c8 [saveload] Restore: reload saved state
//! - 0x105350 [saveload] load state: objlist, schedules, base tiles; place party
//! - 0x10e4bc [objects] write the region's blocks back and commit every `.tmp` (map unload)

pub mod objlist;
mod restore;
mod save;
mod seams;
mod store;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_real;

pub use restore::{StateFiles, boot_objlist, load_state, read_state_files, restore, restore_game};
pub use save::{commit_blocks, save, save_game, write_back_region};
pub use store::{MemStore, SAVE_LAYER, SaveStore, SaveSync, StoreError, attach, changed_since};

use crate::objects::BLOCK_COUNT;

/// Name of the objlist file (the PC-relative string at 0x104fd4 both 0x105350 and 0x105716 use).
pub const OBJLIST_FILE: &str = "savegame/objlist";

/// The name 0x10e4bc gives block `i` (0..69): the template `savegame/objblkaa` with
/// `'A' + (i & 7)` and `'A' + (i >> 3)` written over its last two letters, so surface
/// superchunk (sx, sy) is `objblk{A+sx}{A+sy}` and dungeon level z is `objblk{A+z-1}I`.
pub fn block_file(i: usize) -> String {
    let x = char::from(b'A' + (i & 7) as u8);
    let y = char::from(b'A' + (i >> 3) as u8);
    format!("savegame/objblk{x}{y}")
}

/// What the save code keeps besides the game globals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveLoad {
    /// Writes of `savegame/` files since boot (0x105716 runs, and the character `u` writes,
    /// [`crate::intro::handoff::apply`]); [`SaveSync`] flushes when it moves.
    pub generation: u32,
    /// The [`SaveLoad::generation`] at which block `i`'s file was last committed (0 = not this
    /// session, the file is still the one the platform supplied).
    pub block_gen: Vec<u32>,
    /// `savegame/objlist` as the last Save (or `u`'s character) wrote it (`None` until then;
    /// 0x105350 then reads the platform's file).
    pub objlist: Option<Vec<u8>>,
    /// The party-name table (objlist 0x0F00, 16 × 14 bytes) as last read: an unchanged name is
    /// written back with the bytes that followed its NUL.
    pub names_raw: Vec<u8>,
}

impl Default for SaveLoad {
    fn default() -> Self {
        Self {
            generation: 0,
            block_gen: vec![0; BLOCK_COUNT],
            objlist: None,
            names_raw: Vec::new(),
        }
    }
}

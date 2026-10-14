//! The platform side of saving: where the `savegame/` files live between sessions.

use std::collections::BTreeMap;
use std::fmt;

use super::{OBJLIST_FILE, block_file};
use crate::engine::Engine;
use crate::files::Files;
use crate::state::State;

/// Name of the layer [`attach`] adds to the game files.
pub const SAVE_LAYER: &str = "save";

/// The platform could not read or write its saves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreError(pub String);

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "save store: {}", self.0)
    }
}

impl std::error::Error for StoreError {}

/// Storage for the `savegame/` files, supplied by the platform (native: a directory; web:
/// IndexedDB). Names are the original's (`savegame/objlist`, `savegame/objblkAA`, ...); a
/// store compares them without regard to ASCII case, as AmigaDOS does.
pub trait SaveStore {
    /// Every stored file (empty when nothing was saved yet).
    fn load(&self) -> Result<BTreeMap<String, Vec<u8>>, StoreError>;
    /// Writes `files` over the stored ones (files not named stay as they are). One call per
    /// save; a platform that can makes it all-or-nothing.
    fn store(&mut self, files: &BTreeMap<String, Vec<u8>>) -> Result<(), StoreError>;
}

/// A store in memory (tests, and platforms that persist it themselves).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemStore {
    pub files: BTreeMap<String, Vec<u8>>,
}

impl SaveStore for MemStore {
    fn load(&self) -> Result<BTreeMap<String, Vec<u8>>, StoreError> {
        Ok(self.files.clone())
    }

    fn store(&mut self, files: &BTreeMap<String, Vec<u8>>) -> Result<(), StoreError> {
        for (name, bytes) in files {
            let old = self
                .files
                .keys()
                .find(|k| k.eq_ignore_ascii_case(name))
                .cloned();
            if let Some(old) = old {
                self.files.remove(&old);
            }
            self.files.insert(name.clone(), bytes.clone());
        }
        Ok(())
    }
}

/// Lays the stored save over the user's files before the engine boots, so load state reads
/// the saved `savegame/objlist` and blocks; names the store does not hold fall through to the
/// user's new-game files. Returns how many files the store supplied.
pub fn attach(files: &mut Files, store: &dyn SaveStore) -> Result<usize, StoreError> {
    let saved = store.load()?;
    let n = saved.len();
    if n > 0 {
        files.overlay(SAVE_LAYER, saved);
    }
    Ok(n)
}

/// The files Saves committed after generation `since`: `savegame/objlist` and each block whose
/// file a Save replaced, under the original's names.
pub fn changed_since(s: &State, since: u32) -> BTreeMap<String, Vec<u8>> {
    let sl = &s.saveload;
    let mut out = BTreeMap::new();
    if sl.generation <= since {
        return out;
    }
    if let Some(objlist) = &sl.objlist {
        out.insert(OBJLIST_FILE.to_string(), objlist.clone());
    }
    for (i, &g) in sl.block_gen.iter().enumerate() {
        if g > since
            && let Some((_, block)) = s.objects.blocks.get(i)
        {
            out.insert(block_file(i), block.to_bytes());
        }
    }
    out
}

/// Tracks what the platform has stored; call [`SaveSync::flush`] after each tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveSync {
    /// The last generation handed to the store.
    pub flushed: u32,
}

impl SaveSync {
    /// Hands the store what a Save committed since the last flush. Returns the number of files
    /// written (0 when there was no Save).
    pub fn flush(
        &mut self,
        engine: &Engine,
        store: &mut dyn SaveStore,
    ) -> Result<usize, StoreError> {
        let (files, generation) = {
            let s = engine.state();
            (changed_since(&s, self.flushed), s.saveload.generation)
        };
        if generation == self.flushed {
            return Ok(0);
        }
        store.store(&files)?;
        self.flushed = generation;
        Ok(files.len())
    }
}

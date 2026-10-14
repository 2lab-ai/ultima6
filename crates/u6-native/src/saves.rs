//! The native save store: the `savegame/` files in a directory the user names
//! (`--save-dir DIR` → `DIR/savegame/objlist`, `DIR/savegame/objblkAA`, ...), the layout of the
//! original's PLAYER disk, so an original `savegame/` directory copied there loads as it is.
//! Nothing is written anywhere else; the user's game files are only read.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use u6_core::saveload::{SaveStore, StoreError};

/// Directory part every save name starts with.
const DIR: &str = "savegame";

pub struct DirStore {
    root: PathBuf,
}

fn err(path: &Path, e: std::io::Error) -> StoreError {
    StoreError(format!("{}: {e}", path.display()))
}

impl DirStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn dir(&self) -> PathBuf {
        self.root.join(DIR)
    }

    /// The file a save name maps to: `savegame/<one plain component>` only.
    fn path(&self, name: &str) -> Result<PathBuf, StoreError> {
        let file = name
            .strip_prefix(DIR)
            .and_then(|r| r.strip_prefix('/'))
            .filter(|f| !f.is_empty() && !f.contains(['/', '\\']) && *f != "." && *f != "..")
            .ok_or_else(|| StoreError(format!("{name}: not a savegame/ file name")))?;
        Ok(self.dir().join(file))
    }
}

impl SaveStore for DirStore {
    fn load(&self) -> Result<BTreeMap<String, Vec<u8>>, StoreError> {
        let dir = self.dir();
        let mut out = BTreeMap::new();
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(err(&dir, e)),
        };
        for entry in entries {
            let entry = entry.map_err(|e| err(&dir, e))?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            // Leftovers of an interrupted write are not save files.
            if !path.is_file() || name.ends_with(".part") {
                continue;
            }
            let bytes = fs::read(&path).map_err(|e| err(&path, e))?;
            out.insert(format!("{DIR}/{name}"), bytes);
        }
        Ok(out)
    }

    /// Each file goes to `name.part` first and is renamed over the old one; a stored file
    /// whose name differs only in case is removed (AmigaDOS names ignore case).
    fn store(&mut self, files: &BTreeMap<String, Vec<u8>>) -> Result<(), StoreError> {
        let dir = self.dir();
        fs::create_dir_all(&dir).map_err(|e| err(&dir, e))?;
        let existing: Vec<String> = fs::read_dir(&dir)
            .map_err(|e| err(&dir, e))?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        for (name, bytes) in files {
            let path = self.path(name)?;
            let part = path.with_extension("part");
            fs::write(&part, bytes).map_err(|e| err(&part, e))?;
            let file = path.file_name().unwrap_or_default().to_string_lossy();
            for old in &existing {
                if old.eq_ignore_ascii_case(&file) && *old != file {
                    let old = dir.join(old);
                    fs::remove_file(&old).map_err(|e| err(&old, e))?;
                }
            }
            fs::rename(&part, &path).map_err(|e| err(&path, e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("u6-native-saves-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn stores_and_loads_the_savegame_directory() {
        let root = scratch("rt");
        let mut store = DirStore::new(&root);
        assert!(store.load().unwrap().is_empty(), "no directory yet");
        fs::create_dir_all(root.join(DIR)).unwrap();
        fs::write(root.join(DIR).join("OBJBLKAA"), b"old").unwrap();
        let mut files = BTreeMap::new();
        files.insert("savegame/objblkAA".to_string(), b"new".to_vec());
        files.insert("savegame/objlist".to_string(), vec![1, 2, 3]);
        store.store(&files).unwrap();
        let got = store.load().unwrap();
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got["savegame/objblkAA"], b"new");
        assert_eq!(got["savegame/objlist"], [1, 2, 3]);
        let mut bad = BTreeMap::new();
        bad.insert("savegame/../x".to_string(), vec![0]);
        assert!(store.store(&bad).is_err());
        bad.clear();
        bad.insert("objlist".to_string(), vec![0]);
        assert!(store.store(&bad).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}

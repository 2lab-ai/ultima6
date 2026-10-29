//! The native save store: the `savegame/` files in a directory the user names
//! (`--save-dir DIR` → `DIR/savegame/objlist`, `DIR/savegame/objblkAA`, ...), the layout of the
//! original's PLAYER disk, so an original `savegame/` directory copied there loads as it is. A
//! Save with something remembered also writes the port's `DIR/savegame/u6sight` there (spec 18,
//! `u6_core::saveload::SIGHT_FILE`), which the original never opens. Nothing is written anywhere
//! else; the user's game files are only read.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use u6_core::gfx::cache::MAX_SIGHT_FILE_BYTES;
use u6_core::saveload::{SIGHT_FILE, SaveStore, StoreError};

/// Directory part every save name starts with.
const DIR: &str = "savegame";

#[cfg(test)]
thread_local! {
    /// The bytes [`DirStore::load`] read on this thread (the tests read it).
    static READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// In the tests, the handle [`read_within`] reads through: each read of it adds the bytes it
/// returned to [`READ`] as it returns them, whatever is done with them after.
#[cfg(test)]
struct Counted<R>(R);

#[cfg(test)]
impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.0.read(buf)?;
        READ.with(|r| r.set(r.get() + n));
        Ok(n)
    }
}

pub struct DirStore {
    root: PathBuf,
}

fn err(path: &Path, e: std::io::Error) -> StoreError {
    StoreError(format!("{}: {e}", path.display()))
}

/// The file at `path` through one open handle: none when the file it opens (a link followed)
/// holds more than `most` bytes, and then nothing of it is read; else its bytes, read through
/// that handle into a buffer of the length it gave, at most `most` and one more (none when there
/// was one more: it grew since).
fn read_within(path: &Path, most: u64) -> Result<Option<Vec<u8>>, StoreError> {
    let file = fs::File::open(path).map_err(|e| err(path, e))?;
    let len = file.metadata().map_err(|e| err(path, e))?.len();
    if len > most {
        return Ok(None);
    }
    #[cfg(test)]
    let file = Counted(file);
    let mut bytes = Vec::with_capacity(len as usize);
    (file.take(most + 1).read_to_end(&mut bytes)).map_err(|e| err(path, e))?;
    Ok((bytes.len() as u64 <= most).then_some(bytes))
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
    /// Every file of the directory (none when it is missing), but the leftovers of an interrupted
    /// write and a remembered sight's file larger than one can be (spec 18,
    /// `MAX_SIGHT_FILE_BYTES`: the size of the file a link names, taken from the handle it would
    /// be read through), which is left unread: the save loads with nothing remembered.
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
            let file = format!("{DIR}/{name}");
            let bytes = if file.eq_ignore_ascii_case(SIGHT_FILE) {
                match read_within(&path, MAX_SIGHT_FILE_BYTES as u64)? {
                    Some(bytes) => bytes,
                    None => continue,
                }
            } else {
                let bytes = fs::read(&path).map_err(|e| err(&path, e))?;
                #[cfg(test)]
                READ.with(|r| r.set(r.get() + bytes.len()));
                bytes
            };
            out.insert(file, bytes);
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

    /// What [`DirStore::load`] of `store` returns, and the bytes it read ([`READ`]).
    fn load_counted(store: &DirStore) -> (BTreeMap<String, Vec<u8>>, usize) {
        let before = READ.with(std::cell::Cell::get);
        let got = store.load().unwrap();
        (got, READ.with(std::cell::Cell::get) - before)
    }

    /// Spec 18 (saved with the game): a remembered sight's file larger than one can be
    /// (`MAX_SIGHT_FILE_BYTES`) is left out and not read, in any case of its name, while another
    /// file past that size is read; a sight file of that size is read, into a buffer of its
    /// length (none grown past it). The bytes counted are those each read of the sight file's
    /// handle returned ([`Counted`]), so a load that read the file and then left it out would
    /// count them: the bytes the load read are the other files' alone.
    #[test]
    fn a_sight_file_past_its_limit_is_not_read() {
        let root = scratch("sight-limit");
        let dir = root.join(DIR);
        fs::create_dir_all(&dir).unwrap();
        let store = DirStore::new(&root);
        let past = MAX_SIGHT_FILE_BYTES + 1;
        fs::write(dir.join("objlist"), vec![0; past]).unwrap();
        fs::write(dir.join("U6Sight"), vec![0; past]).unwrap();
        let (got, read) = load_counted(&store);
        assert!(
            got.keys().eq(["savegame/objlist"]),
            "the sight file past its limit left out"
        );
        assert!(read == past, "and not read: the objlist's bytes alone");
        fs::write(dir.join("U6Sight"), vec![0; MAX_SIGHT_FILE_BYTES]).unwrap();
        let (got, read) = load_counted(&store);
        let sight = &got["savegame/U6Sight"];
        assert!(
            sight.len() == MAX_SIGHT_FILE_BYTES && read == past + MAX_SIGHT_FILE_BYTES,
            "at its limit: read"
        );
        assert!(
            sight.capacity() == MAX_SIGHT_FILE_BYTES,
            "into a buffer of the length its handle gave, not grown past it"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// Spec 18 (saved with the game): a link in the directory to a remembered sight's file larger
    /// than one can be (outside the directory; the link itself is small) is left out and not read
    /// either: the size is the file's the link names, from the handle it would be read through,
    /// and the bytes counted are those each read of that handle returned ([`Counted`]).
    #[cfg(unix)]
    #[test]
    fn a_link_to_a_sight_file_past_its_limit_is_not_read() {
        let root = scratch("sight-link");
        let dir = root.join(DIR);
        fs::create_dir_all(&dir).unwrap();
        let store = DirStore::new(&root);
        let past = MAX_SIGHT_FILE_BYTES + 1;
        fs::write(dir.join("objlist"), vec![0; past]).unwrap();
        let far = root.join("far");
        fs::write(&far, vec![0; past]).unwrap();
        std::os::unix::fs::symlink(&far, dir.join("u6sight")).unwrap();
        let (got, read) = load_counted(&store);
        assert!(
            got.keys().eq(["savegame/objlist"]) && read == past,
            "a link to a sight file past its limit: left out, not read"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// Spec 18 (saved with the game) on the user's files: a Save in the game with the remembered
    /// sight on writes `savegame/u6sight` into the directory beside the original's files (by way
    /// of `u6sight.part`, none left over), and a game booted again from the directory holds the
    /// memory the Save wrote. Prints `ran` or `skipped` and counts only; with
    /// `U6_REQUIRE_ARCHIVE=1` a missing zip fails it.
    #[test]
    fn real_the_remembered_sight_is_stored_in_the_directory_and_read_back() {
        use u6_core::saveload::{OBJLIST_FILE, SIGHT_FILE, SaveSync, attach};
        use u6_core::{Engine, InputEvent, Key};
        use u6_testkit::Quiet;
        let name = "real_the_remembered_sight_is_stored_in_the_directory_and_read_back";
        let zip = Path::new("/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip");
        if !zip.exists() {
            assert_ne!(
                std::env::var("U6_REQUIRE_ARCHIVE").as_deref(),
                Ok("1"),
                "{name}: U6_REQUIRE_ARCHIVE=1 and the archive is missing"
            );
            println!("{name}: skipped");
            return;
        }
        println!("{name}: ran");
        let r = u6_intake::intake(vec![u6_intake::InputFile {
            name: "ultima-vi.zip".into(),
            data: fs::read(zip).quiet(),
        }]);
        let files = u6_core::Files::new(r.files);
        let root = scratch("sight");
        let mut store = DirStore::new(&root);
        let prompt = |e: &Engine| e.io().pending_events() == 0 && e.state().main.reading == 1;
        // Ticks until the command prompt (at least 40, at most 4000); whether it is there.
        let settle = |e: &mut Engine| {
            for t in 0..4000 {
                if t >= 40 && prompt(e) {
                    break;
                }
                e.tick(&[], 16);
            }
            prompt(e)
        };
        let mut e = Engine::game_first(files.clone(), 0).quiet();
        e.io().set_sight_memory(true);
        assert!(settle(&mut e), "the boot reaches the prompt");
        // A step, then the Save's control-S and its question's Y.
        for key in [Key::Down, Key::Char('\u{13}'), Key::Char('y')] {
            e.tick(&[InputEvent::Key(key)], 16);
            settle(&mut e);
        }
        assert!(prompt(&e), "the prompt after the Save");
        assert!(e.state().saveload.generation == 1, "the Save");
        let saved = e.state().saveload.sight.clone().quiet();
        let n = SaveSync::default().flush(&e, &mut store).quiet();
        let on_disk = fs::read(root.join(DIR).join("u6sight")).quiet();
        assert!(
            on_disk == saved.bytes(),
            "the file the Save wrote, in the directory"
        );
        let left: Vec<String> = fs::read_dir(root.join(DIR))
            .quiet()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".part"))
            .collect();
        assert!(left.is_empty(), "no .part file left over");
        let mut again = files;
        let laid = attach(&mut again, &DirStore::new(&root)).quiet();
        assert!(
            laid == n,
            "every file the Save wrote laid over, the sight file too"
        );
        assert!(again.get(SIGHT_FILE).is_some_and(|f| f == saved.bytes()));
        let mut fresh = Engine::game_first(again, 0).quiet();
        assert!(
            settle(&mut fresh),
            "the boot from the directory reaches the prompt"
        );
        let objlist = fs::read(root.join(OBJLIST_FILE)).quiet();
        let held = fresh.state().gfx.cache.sight_file(&objlist);
        assert!(
            held.as_deref() == Some(saved.bytes()),
            "booted from the directory: the memory the Save wrote"
        );
        println!(
            "native store: {n} files, {} cells remembered after the boot",
            fresh.state().gfx.cache.sight_cells()
        );
        fs::remove_dir_all(&root).quiet();
    }
}

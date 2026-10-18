//! The out dir of a `calibrate` run, of either mode ([`run_in`]): the run writes to a fresh
//! directory beside `--out`, which holds its mode's marker file, and that directory is put in
//! place of `--out` at the end. After any run `--out` holds that run's output alone, or what
//! was there before when the run could not replace it. An old `--out` is replaced only when it
//! holds the marker of the mode run: a directory the tool did not write, an empty one, or one
//! the other mode wrote, is left as it is and the run fails.

use std::path::{Path, PathBuf};

use crate::work;

/// A mode of `calibrate`: its name in messages, and the file its out dirs hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub name: &'static str,
    pub marker: &'static str,
}

/// The first pass's sample (`calibrate`).
pub const FIRST: Mode = Mode {
    name: "calibrate",
    marker: ".calibrate-first-pass",
};

/// The read of the final pack (`calibrate --final`).
pub const FINAL: Mode = Mode {
    name: "calibrate --final",
    marker: ".calibrate-final",
};

/// A run's out dir while it is written: a fresh directory beside the out dir, holding the
/// mode's marker, swapped in at the end ([`Staged::swap`]) or removed.
struct Staged {
    mode: Mode,
    dir: PathBuf,
    out: PathBuf,
}

impl Staged {
    /// A fresh directory beside `out` (made absolute, its parent checked and created).
    fn begin(mode: Mode, out: &Path) -> Result<Staged, String> {
        let name = out
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("{}: not a directory name", out.display()))?
            .to_string();
        let parent = match out.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let parent = work::ensure_dir(&parent)?;
        let stamp = format!("{}-{}", std::process::id(), work::now_ns());
        let dir = parent.join(format!(".{name}.staging-{stamp}"));
        std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let staged = Staged {
            mode,
            dir,
            out: parent.join(name),
        };
        let note = format!("{}\n", mode.name);
        work::write_file(&staged.dir.join(mode.marker), note.as_bytes())?;
        Ok(staged)
    }

    /// Puts the staged dir in the out dir's place: an out dir there is removed only when it is
    /// one this mode wrote (it holds the mode's marker); anything else there, an empty dir too,
    /// is left as it is, the staged dir removed, and the run fails.
    fn swap(self) -> Result<(), String> {
        let refuse = |why: &str| -> Result<(), String> {
            let _ = std::fs::remove_dir_all(&self.dir);
            Err(format!(
                "{}: {why}; it is not replaced (give another --out, or move it)",
                self.out.display()
            ))
        };
        match std::fs::symlink_metadata(&self.out) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return refuse(&e.to_string()),
            Ok(m) if !m.file_type().is_dir() => return refuse("not a directory"),
            Ok(_) if self.out.join(self.mode.marker).is_file() => {
                // The old run moved aside, this one put in its place, then the old removed; on a
                // failure the old put back.
                let old = PathBuf::from(format!("{}.old", self.dir.display()));
                if let Err(e) = std::fs::rename(&self.out, &old) {
                    return refuse(&e.to_string());
                }
                if let Err(e) = std::fs::rename(&self.dir, &self.out) {
                    let _ = std::fs::rename(&old, &self.out);
                    return refuse(&e.to_string());
                }
                let _ = std::fs::remove_dir_all(&old);
                return Ok(());
            }
            Ok(_) => {
                let why = format!(
                    "a directory {} did not write (no marker {})",
                    self.mode.name, self.mode.marker
                );
                return refuse(&why);
            }
        }
        match std::fs::rename(&self.dir, &self.out) {
            Ok(()) => Ok(()),
            Err(e) => refuse(&e.to_string()),
        }
    }
}

/// Runs a `calibrate` run of `mode` whose out dir is `out` (module docs): `run` writes to a
/// fresh directory (its first argument; the second is the out dir it is to be, for messages),
/// which is put in place of `out` at the end; when `run` fails, with `refused.txt` (the error)
/// beside what it wrote.
pub fn run_in<T>(
    mode: Mode,
    out: &Path,
    run: impl FnOnce(&Path, &Path) -> Result<T, String>,
) -> Result<T, String> {
    work::check_target(out)?;
    let staged = Staged::begin(mode, out)?;
    let r = run(&staged.dir, &staged.out);
    if let Err(e) = &r
        && let Err(w) =
            work::write_file(&staged.dir.join("refused.txt"), format!("{e}\n").as_bytes())
    {
        let _ = std::fs::remove_dir_all(&staged.dir);
        return Err(format!("{e} ({w})"));
    }
    match (r, staged.swap()) {
        (r, Ok(())) => r,
        (Ok(_), Err(e)) => Err(e),
        (Err(e), Err(w)) => Err(format!("{e}; {w}")),
    }
}

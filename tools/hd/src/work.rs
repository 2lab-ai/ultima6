//! The work dir (default `~/Games/ultima6/hd/`), where every output lives, and the one way the
//! tool writes: never inside a git work tree or the repository this tool was built from (spec 07
//! Rights).
//!
//! ```text
//! atlas.json          the atlas generation every job and output is bound to
//! jobs.jsonl          one line per sheet (atlas, then the re-rolls ingest adds)
//! sheets/<id>.png     what run sends
//! codex/<id>/         the codex call's working dir, its event log and stderr
//! out/<id>.png        what came back, and out/<id>.json its receipt (run::Receipt)
//! runs.jsonl          one line per call: sheet, ok, seconds
//! gates.jsonl         one line per image per attempt: ids and metrics
//! slices/<image>.png  the images that passed: the ×8 RGBA masters (the pack is ×4)
//! ingest.json         the images pack takes: id, hash, size, slice
//! coverage.txt        records by class: generated / fallback / failed / pending (counts)
//! diagnostic.txt      details of a failed game load (0600; the terminal gets a category)
//! pack/u6-hdpack.zip  the pack
//! ```
//!
//! Every write goes through [`write_file`], [`create_temp`] or [`ensure_dir`], which check the
//! actual target when it is written: the path is absolute with no `..`; its directory, resolved
//! through every symlink, lies outside every protected tree (any directory with a `.git` entry
//! above it, which covers the primary checkout and every linked worktree, and the build tree
//! itself); the file itself is not a symlink; the bytes go to a fresh temporary file created
//! with `O_EXCL` (which never follows or reuses an existing name) and are renamed over the
//! target. A directory swapped for a symlink between the check and the write is not caught (the
//! owner's own machine; the window is a few system calls).

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::atlas::Job;

/// The default work dir under the home directory.
pub const DEFAULT_DIR: &str = "Games/ultima6/hd";

pub struct Work {
    pub root: PathBuf,
}

/// The repository this binary was built from.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `path` made absolute; refused with a `..` component.
fn absolute(path: &Path) -> Result<PathBuf, String> {
    let abs = std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if abs.components().any(|c| c == Component::ParentDir) {
        return Err(format!("{}: a path with `..` is refused", path.display()));
    }
    Ok(abs)
}

/// Why `dir` (resolved, existing) is protected, if it is.
pub(crate) fn protected(dir: &Path) -> Option<String> {
    let build = repo_root();
    let build = build.canonicalize().unwrap_or(build);
    if dir.starts_with(&build) {
        return Some("inside the repository".into());
    }
    dir.ancestors()
        .find(|a| std::fs::symlink_metadata(a.join(".git")).is_ok())
        .map(|a| format!("inside the git work tree {}", a.display()))
}

/// Refuses `path` (made absolute) if its nearest existing ancestor, resolved, is protected.
pub fn check_target(path: &Path) -> Result<(), String> {
    let abs = absolute(path)?;
    let existing = abs
        .ancestors()
        .find(|a| std::fs::symlink_metadata(a).is_ok())
        .ok_or_else(|| format!("{}: no existing ancestor", path.display()))?;
    let resolved = existing
        .canonicalize()
        .map_err(|e| format!("{}: {e}", existing.display()))?;
    if let Some(why) = protected(&resolved) {
        return Err(format!(
            "{}: {why}; the tool writes only outside it (spec 07 Rights)",
            path.display()
        ));
    }
    Ok(())
}

/// Creates `dir` and its parents after checking them; returns it resolved (and checked again).
pub fn ensure_dir(dir: &Path) -> Result<PathBuf, String> {
    check_target(dir)?;
    let abs = absolute(dir)?;
    std::fs::create_dir_all(&abs).map_err(|e| format!("{}: {e}", abs.display()))?;
    let resolved = abs
        .canonicalize()
        .map_err(|e| format!("{}: {e}", abs.display()))?;
    if let Some(why) = protected(&resolved) {
        return Err(format!("{}: {why}", dir.display()));
    }
    Ok(resolved)
}

/// [`ensure_dir`] for a directory the tool owns and empties (`slices/`): refused where `dir` is
/// itself a symlink or not a directory, so what is enumerated and removed there is never a
/// directory it points at; the resolved path.
pub fn own_dir(dir: &Path) -> Result<PathBuf, String> {
    let abs = absolute(dir)?;
    match std::fs::symlink_metadata(&abs) {
        Ok(m) if !m.file_type().is_dir() => {
            return Err(format!(
                "{}: not a directory of the work dir (a symlink is not followed)",
                dir.display()
            ));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    }
    ensure_dir(&abs)
}

static TEMP: AtomicU64 = AtomicU64::new(0);

/// A new temporary file for `path` (created with `O_EXCL`, so never an existing name or a
/// symlink) in `path`'s checked directory: the file, its path, and the target to commit to.
pub fn create_temp(path: &Path, mode: u32) -> Result<(std::fs::File, PathBuf, PathBuf), String> {
    let abs = absolute(path)?;
    let (Some(parent), Some(name)) = (abs.parent(), abs.file_name()) else {
        return Err(format!("{}: not a file path", path.display()));
    };
    let dir = ensure_dir(parent)?;
    let target = dir.join(name);
    if let Ok(md) = std::fs::symlink_metadata(&target)
        && !md.file_type().is_file()
    {
        return Err(format!(
            "{}: exists and is not a plain file",
            path.display()
        ));
    }
    let n = TEMP.fetch_add(1, Ordering::SeqCst);
    let tmp = dir.join(format!(
        ".{}.{}.{n}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&tmp)
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    Ok((file, tmp, target))
}

/// Moves a finished temporary file over its target (refused if the target became a symlink).
pub fn commit(tmp: &Path, target: &Path) -> Result<(), String> {
    if let Ok(md) = std::fs::symlink_metadata(target)
        && !md.file_type().is_file()
    {
        let _ = std::fs::remove_file(tmp);
        return Err(format!(
            "{}: exists and is not a plain file",
            target.display()
        ));
    }
    std::fs::rename(tmp, target).map_err(|e| format!("{}: {e}", target.display()))
}

/// Writes `bytes` to `path` with `mode`, the safe way (see the module doc).
pub fn write_file_mode(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let (mut f, tmp, target) = create_temp(path, mode)?;
    let written = f.write_all(bytes).and_then(|()| f.sync_all());
    drop(f);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", tmp.display()));
    }
    commit(&tmp, &target)
}

/// Writes `bytes` to `path` (0644), the safe way.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_file_mode(path, bytes, 0o644)
}

/// Appends `text` to `path` (read, extended, written whole the safe way).
pub fn append_file(path: &Path, text: &str) -> Result<(), String> {
    let mut all = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    all.extend_from_slice(text.as_bytes());
    write_file(path, &all)
}

/// [`append_file`], made durable: the file is synced before it replaces the old one (as every
/// write here), and its directory after, so the line survives a crash once this returns. An
/// error from the directory's sync comes after the line was written: the caller must treat
/// it as written (the run ledger counts it, conservatively) and as not durable.
pub fn append_durable(path: &Path, text: &str) -> Result<(), String> {
    append_file(path, text)?;
    let dir = ensure_dir(
        path.parent()
            .ok_or_else(|| format!("{}: no parent", path.display()))?,
    )?;
    #[cfg(test)]
    if FAIL_DIR_SYNC
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|(d, needle)| *d == dir && text.contains(needle.as_str()))
    {
        return Err(format!("{}: sync failed (injected)", dir.display()));
    }
    std::fs::File::open(&dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| format!("{}: {e}", dir.display()))
}

/// Tests only: the directory whose sync [`append_durable`] fails, after a line holding the
/// text given is written there.
#[cfg(test)]
pub(crate) static FAIL_DIR_SYNC: std::sync::Mutex<Option<(PathBuf, String)>> =
    std::sync::Mutex::new(None);

/// Now, in ns since the epoch (the run ledger's `at`).
pub fn now_ns() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// Test builds only (`debug_assertions`, which the gate profile keeps): appends
/// `<what> <ns since the epoch>` to the file `U6_HD_TEST_NOTES` names, if it is set.
#[cfg(debug_assertions)]
pub(crate) fn test_note(what: &str) {
    let Some(p) = std::env::var_os("U6_HD_TEST_NOTES") else {
        return;
    };
    // One write per line, so lines from threads at once do not interleave (O_APPEND).
    let line = format!("{what} {}\n", now_ns());
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
        let _ = f.write_all(line.as_bytes());
    }
}

#[cfg(not(debug_assertions))]
pub(crate) fn test_note(_: &str) {}

/// FNV-1a 64 of `parts`, each prefixed by its length, as 16 hex digits. Not a cryptographic
/// hash: it tells a file changed by a race or an accident (a re-ingest while a command reads,
/// another file copied over it), not one swapped on purpose for another made to the same
/// digest.
pub fn digest(parts: &[&[u8]]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in (p.len() as u64).to_le_bytes().iter().chain(p.iter()) {
            h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// The work dir's lock, held while a command writes there: an advisory lock (`flock`) on the
/// open handle of `.u6-hd.lock`, released when the handle closes, by drop or by the process
/// ending however it ends. The file stays: removing it would let a waiting command lock the old
/// file while a new one locks a fresh one.
#[derive(Debug)]
pub struct Lock {
    _file: std::fs::File,
}

impl Work {
    /// Takes the work dir's lock: a second command on the same work dir is refused while the
    /// first holds it, and a command that is gone (killed included) holds nothing. The file
    /// holds the holder's pid, for the message only.
    pub fn lock(&self) -> Result<Lock, String> {
        let dir = ensure_dir(&self.root)?;
        let path = dir.join(".u6-hd.lock");
        // A regular file or none (then created); a symlink there is not followed.
        if let Ok(m) = std::fs::symlink_metadata(&path)
            && !m.file_type().is_file()
        {
            return Err(format!("{}: not a regular file", path.display()));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o644)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                let pid = std::fs::read_to_string(&path).unwrap_or_default();
                let pid = pid.trim();
                let who = if pid.is_empty() {
                    String::new()
                } else {
                    format!(" (pid {pid})")
                };
                return Err(format!(
                    "{}: another u6-hd{who} is using this work dir",
                    path.display()
                ));
            }
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(format!("{}: {e}", path.display()));
            }
        }
        // Held: the pid replaces whatever an earlier holder wrote.
        let pid = std::process::id().to_string();
        file.set_len(0)
            .and_then(|()| file.write_all(pid.as_bytes()))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        test_note("locked");
        Ok(Lock { _file: file })
    }

    /// The work dir at `dir`, or the default under `HOME`; refused inside a protected tree.
    pub fn open(dir: Option<&Path>) -> Result<Work, String> {
        let root = match dir {
            Some(d) => d.to_path_buf(),
            None => {
                let home = std::env::var_os("HOME").ok_or("HOME is not set; pass --work")?;
                Path::new(&home).join(DEFAULT_DIR)
            }
        };
        check_target(&root)?;
        // Absolute, since the codex calls run in their own working dirs.
        let root = absolute(&root)?;
        Ok(Work { root })
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn sheet(&self, id: &str) -> PathBuf {
        self.root.join("sheets").join(format!("{id}.png"))
    }

    pub fn out(&self, id: &str) -> PathBuf {
        self.root.join("out").join(format!("{id}.png"))
    }

    pub fn receipt(&self, id: &str) -> PathBuf {
        self.root.join("out").join(format!("{id}.json"))
    }

    pub fn codex_dir(&self, id: &str) -> PathBuf {
        self.root.join("codex").join(id)
    }

    pub fn slice(&self, stem: &str) -> PathBuf {
        self.root.join("slices").join(format!("{stem}.png"))
    }

    pub fn mkdirs(&self) -> Result<(), String> {
        for d in ["sheets", "out", "codex", "slices", "pack"] {
            ensure_dir(&self.root.join(d))?;
        }
        Ok(())
    }

    pub fn read_jobs(&self) -> Result<Vec<Job>, String> {
        let p = self.path("jobs.jsonl");
        let text = std::fs::read_to_string(&p)
            .map_err(|e| format!("{}: {e} (run atlas first)", p.display()))?;
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
            .map(|(i, l)| {
                serde_json::from_str(l).map_err(|e| format!("{} line {}: {e}", p.display(), i + 1))
            })
            .collect()
    }

    fn jobs_text(jobs: &[Job]) -> Result<String, String> {
        let mut text = String::new();
        for j in jobs {
            text.push_str(&serde_json::to_string(j).map_err(|e| e.to_string())?);
            text.push('\n');
        }
        Ok(text)
    }

    pub fn append_jobs(&self, jobs: &[Job]) -> Result<(), String> {
        append_file(&self.path("jobs.jsonl"), &Self::jobs_text(jobs)?)
    }

    /// Replaces `jobs.jsonl` (a new atlas).
    pub fn write_jobs(&self, jobs: &[Job]) -> Result<(), String> {
        write_file(&self.path("jobs.jsonl"), Self::jobs_text(jobs)?.as_bytes())
    }

    /// Writes `bytes` to `rel` under the root.
    pub fn write(&self, rel: &str, bytes: &[u8]) -> Result<(), String> {
        write_file(&self.path(rel), bytes)
    }
}

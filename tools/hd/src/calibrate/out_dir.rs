//! Where a `calibrate` run writes, in either mode ([`run_in`]). Two rules, each held by how the
//! run writes, not by a check:
//!
//! 1. **A run's output is never mixed with another run's.** Once `--out` is there (made if it is
//!    not), the run's first write is a `create_dir` of a dir of its own,
//!    `<--out>/run-<UTC time>-<pid>/`, named from this process's clock and pid alone; when that
//!    dir is there already the run fails, it is never reused. The run then holds that dir open
//!    and makes every file of it through it ([`at`]: `openat` with `O_CREAT | O_EXCL |
//!    O_NOFOLLOW`, no temp file, no rename), never by a path: nothing outside it is written, no
//!    file there is replaced, and a dir removed, or swapped for a link, while the run writes is
//!    never made again or followed; the next write fails, and so does the run.
//! 2. **The tool never deletes what it did not make.** A run that fails deletes only the files
//!    this process made ([`Run::write`] lists each with the device and inode it was made with),
//!    each only while that file is still at its name, through the dir it holds open, one by
//!    one, never a tree; it keeps its diagnostics ([`Mode::keep`] and `refused.txt`). A file
//!    another hand put at a name of the run's is left, and said. Nothing a previous process made
//!    is removed, renamed or rewritten. Not closed: a file swapped in at a name between that
//!    check and the delete (POSIX has no delete-if-this-inode), and a dir swapped in for the
//!    run dir between its `create_dir` and its opening.
//!
//! A run that ends well writes `done` last, in the dir its report names (a run whose dir moved
//! fails instead): that path is the only pointer to it (no link, no pointer file). A run dir with
//! no `done` is an incomplete run (one that failed, was killed, or is still going): a run lists
//! those it finds in `--out` when it starts, and leaves them as they are. Cleaning up old runs is
//! the person's job. On a host that is not unix every run fails ([`at::UNSUPPORTED`]).

use std::cell::RefCell;
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::img::{self, Rgba};
use crate::work;

pub mod at;
#[cfg(test)]
mod tests;

/// A mode of `calibrate`: its name in messages, and the files a run of it that fails keeps (its
/// snapshots, and the details its error points to), with `refused.txt`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub name: &'static str,
    pub keep: &'static [&'static str],
}

/// The first pass's sample (`calibrate`).
pub const FIRST: Mode = Mode {
    name: "calibrate",
    keep: &["gates.jsonl"],
};

/// The read of the final pack (`calibrate --final`).
pub const FINAL: Mode = Mode {
    name: "calibrate --final",
    keep: &[
        "gates.jsonl",
        "ingest.json",
        "jobs.jsonl",
        "disagreements.txt",
        "unshown.txt",
        "changed.txt",
    ],
};

/// The file a run that ends well writes last.
pub const DONE: &str = "done";

/// What a run that fails writes: its error, and any file it could not delete.
pub const REFUSED: &str = "refused.txt";

/// Tests only: a hook at a step of a run ([`seam`]); its error is the step's.
#[cfg(test)]
pub(crate) type Hook = std::sync::Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// Tests only: the hooks, by the `--out` of the run as given and the step.
#[cfg(test)]
pub(crate) static HOOKS: std::sync::Mutex<Vec<(PathBuf, &'static str, Hook)>> =
    std::sync::Mutex::new(Vec::new());

/// Tests only: the clock a run whose `--out` (as given) is the key names its dir by, in ns
/// since the epoch, in place of the system's.
#[cfg(test)]
pub(crate) static CLOCK: std::sync::Mutex<Vec<(PathBuf, u128)>> = std::sync::Mutex::new(Vec::new());

/// The test seam at `step` of the run whose `--out`, as given, is `key` (`what`: the step's
/// detail, a file name): nothing outside tests; in tests, the hooks set for it ([`HOOKS`]) run
/// in order, each outside the table's lock, and the first error is the step's.
pub(crate) fn seam(key: &Path, step: &'static str, what: &str) -> Result<(), String> {
    #[cfg(test)]
    {
        let hooks: Vec<Hook> = HOOKS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(k, s, _)| k == key && *s == step)
            .map(|h| h.2.clone())
            .collect();
        for h in hooks {
            h(what)?;
        }
    }
    #[cfg(not(test))]
    let _ = (key, step, what);
    Ok(())
}

/// Tests only: the hooks of a `--out`, removed when this is dropped.
#[cfg(test)]
pub(crate) struct Hooked(PathBuf);

#[cfg(test)]
impl Drop for Hooked {
    fn drop(&mut self) {
        HOOKS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(k, _, _)| *k != self.0);
    }
}

/// Tests only: sets `h` at `step` of the runs whose `--out` is `key`, until every [`Hooked`]
/// of `key` is dropped.
#[cfg(test)]
pub(crate) fn hook(
    key: &Path,
    step: &'static str,
    h: impl Fn(&str) -> Result<(), String> + Send + Sync + 'static,
) -> Hooked {
    HOOKS.lock().unwrap_or_else(|e| e.into_inner()).push((
        key.to_path_buf(),
        step,
        std::sync::Arc::new(h),
    ));
    Hooked(key.to_path_buf())
}

/// Now, in ns since the epoch (in tests, the clock set for `key`, [`CLOCK`]).
fn now(key: &Path) -> u128 {
    #[cfg(test)]
    if let Some(&(_, ns)) = CLOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(k, _)| k == key)
    {
        return ns;
    }
    let _ = key;
    work::now_ns()
}

/// The name of the run dir of a run started at `ns` (since the epoch) by process `pid`:
/// `run-<YYYYMMDD>T<HHMMSS>.<ns>Z-<pid>`, which sorts by the time.
pub fn run_name(ns: u128, pid: u32) -> String {
    let secs = (ns / 1_000_000_000) as u64;
    let day = secs % 86_400;
    format!(
        "run-{}T{:02}{:02}{:02}.{:09}Z-{pid}",
        crate::pack::date(secs).replace('-', ""),
        day / 3600,
        day / 60 % 60,
        day % 60,
        ns % 1_000_000_000
    )
}

/// The run dirs of `out` with no [`DONE`], sorted (the oldest first), each `(refused)` when it
/// holds [`REFUSED`]. Read only.
fn incomplete(out: &Path) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    for e in std::fs::read_dir(out).map_err(|e| format!("{}: {e}", out.display()))? {
        let e = e.map_err(|e| format!("{}: {e}", out.display()))?;
        let name = e.file_name().to_string_lossy().into_owned();
        let dir = e.path();
        let is_dir = std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir());
        if !name.starts_with("run-") || !is_dir {
            continue;
        }
        if std::fs::symlink_metadata(dir.join(DONE)).is_ok() {
            continue;
        }
        let refused = std::fs::symlink_metadata(dir.join(REFUSED)).is_ok();
        found.push(if refused {
            format!("{name} (refused)")
        } else {
            name
        });
    }
    found.sort();
    Ok(found)
}

/// `name` when it is one plain name of a file in a dir (no separator, not `.` or `..`, not
/// empty, not absolute), checked before any file is touched; else why not.
fn one_name(name: &str) -> Result<(), String> {
    use std::path::Component;
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(n)), None) if n == name && !name.contains('/') => Ok(()),
        _ => Err(format!(
            "{name:?}: not a name of a file in the run dir (one name, no separator, not . or ..)"
        )),
    }
}

/// A run's own dir: its path, the dir itself, open (every file of the run is made, read and
/// deleted through it, [`at`], never by a path), and what this process made in it.
pub struct Run {
    key: PathBuf,
    dir: PathBuf,
    open: File,
    id: (u64, u64),
    written: RefCell<Vec<(String, (u64, u64))>>,
}

impl Run {
    /// The run's dir, by the path it was made at: every file of the run is in it.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes `bytes` to a new file `name` of the run's dir, made by this call through the dir
    /// itself (`O_CREAT | O_EXCL | O_NOFOLLOW`: no temp file, no rename, nothing replaced; a
    /// file there already, or a dir removed meanwhile, fails it), and lists it, with the
    /// (device, inode) it was made with, as this process's (the only files a failed run
    /// deletes).
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        use std::io::Write as _;
        one_name(name)?;
        seam(&self.key, "write", name)?;
        let path = self.dir.join(name);
        let say = |e: std::io::Error| format!("{}: {e}", path.display());
        let mut f = at::create(&self.open, name).map_err(say)?;
        let made = at::id(&f).map_err(say)?;
        self.written.borrow_mut().push((name.to_string(), made));
        f.write_all(bytes).and_then(|()| f.sync_all()).map_err(say)
    }

    /// [`Run::write`] of `img` as a PNG.
    pub fn write_png(&self, name: &str, img: &Rgba) -> Result<(), String> {
        let path = self.dir.join(name);
        let bytes = img::encode_png(img).map_err(|e| format!("{}: {e}", path.display()))?;
        self.write(name, &bytes)
    }

    /// The file `name` of the run's dir, read through the dir itself, at most `cap` bytes.
    pub fn read(&self, name: &str, cap: u64) -> Result<Vec<u8>, String> {
        use std::io::Read as _;
        one_name(name)?;
        let path = self.dir.join(name);
        let say = |e: std::io::Error| format!("{}: {e}", path.display());
        let f = at::open(&self.open, name).map_err(say)?;
        let mut bytes = Vec::new();
        f.take(cap.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(say)?;
        if bytes.len() as u64 > cap {
            return Err(format!("{}: larger than {cap} bytes", path.display()));
        }
        Ok(bytes)
    }

    /// An error `e` of the run's file `name` (a snapshot it read), said of that file by its
    /// full path: a leading `name` in `e` becomes the path; any other `e` follows it.
    pub fn named(&self, name: &str, e: &str) -> String {
        let path = self.dir.join(name);
        let rest = e.strip_prefix(name).unwrap_or(e);
        match rest.chars().next() {
            Some(':' | ' ') => format!("{}{rest}", path.display()),
            _ => format!("{}: {rest}", path.display()),
        }
    }

    /// Whether the run's dir is no longer at its path (removed, renamed, or swapped for a link
    /// or another dir).
    fn moved(&self) -> bool {
        at::path_id(&self.dir) != Some(self.id)
    }

    /// The run failed with `error`: deletes the files this process made in the run's dir but
    /// what `mode` keeps, each only while it is the file the run made (its device and inode),
    /// through the dir itself; then writes [`REFUSED`] (the error, `then` added: each file it
    /// could not delete, and that the dir moved if it did); gives that error.
    fn fail(&self, mode: Mode, error: &str, then: &dyn Fn(String) -> String) -> String {
        let mut left = Vec::new();
        let written = self.written.borrow().clone();
        for (name, made) in written
            .iter()
            .filter(|w| !mode.keep.contains(&w.0.as_str()))
        {
            let path = self.dir.join(name);
            let gone = seam(&self.key, "delete", name).and_then(|()| {
                match at::open(&self.open, name).and_then(|f| at::id(&f)) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(e.to_string()),
                    Ok(now) if now != *made => Err("not the file the run made".into()),
                    Ok(_) => match at::unlink(&self.open, name) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                        _ => Ok(()),
                    },
                }
            });
            if let Err(e) = gone {
                left.push(format!("{} ({e})", path.display()));
            }
        }
        let mut said = error.to_string();
        if !left.is_empty() {
            said = format!(
                "{said}; left, as it could not be deleted: {}",
                left.join(", ")
            );
        }
        if self.moved() {
            said = format!(
                "{said}; the run dir {} was removed or replaced while the run wrote it: nothing \
                 is written or deleted but in the dir it made",
                self.dir.display()
            );
        }
        let said = then(said);
        match self.write(REFUSED, format!("{said}\n").as_bytes()) {
            Ok(()) => said,
            Err(e) => format!("{said}; {REFUSED} not written ({e})"),
        }
    }
}

/// What a run that ends well says of where it wrote: its dir, and the incomplete runs it found
/// in `--out` when it started (left as they are).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub dir: PathBuf,
    pub incomplete: Vec<String>,
}

impl Written {
    /// The report's lines of it.
    pub fn text(&self) -> String {
        let mut s = format!("run dir: {}\n", self.dir.display());
        if !self.incomplete.is_empty() {
            s.push_str(&format!(
                "incomplete runs beside it (no {DONE}; left as they are): {}\n",
                self.incomplete.join(", ")
            ));
        }
        s
    }
}

/// Runs a `calibrate` run of `mode` whose `--out` is `out` (module docs): `out` made if it is
/// not there; the incomplete runs in it listed; the run's own dir made, by `create_dir`, a
/// failure if it is there; `run` writes in it ([`Run`]); then [`DONE`]. A run that fails keeps
/// only its diagnostics ([`Run::fail`]).
pub fn run_in<T>(
    mode: Mode,
    out: &Path,
    run: impl FnOnce(&Run) -> Result<T, String>,
) -> Result<(T, Written), String> {
    if !cfg!(unix) {
        return Err(at::UNSUPPORTED.into());
    }
    work::check_target(out)?;
    let parent = work::ensure_dir(out)?;
    let incomplete = incomplete(&parent)?;
    // Every failure from here says the incomplete runs beside the run.
    let beside = |e: String| match incomplete.as_slice() {
        [] => e,
        v => format!(
            "{e}; incomplete runs beside it (left as they are): {}",
            v.join(", ")
        ),
    };
    let dir = parent.join(run_name(now(out), std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| {
        let why = match e.kind() {
            std::io::ErrorKind::AlreadyExists => " (a run dir is never reused)",
            _ => "",
        };
        beside(format!("{}: {e}{why}", dir.display()))
    })?;
    let say = |e: std::io::Error| beside(format!("{}: {e}", dir.display()));
    let open = at::open_dir(&dir).map_err(say)?;
    let id = at::id(&open).map_err(say)?;
    let r = Run {
        key: out.to_path_buf(),
        dir,
        open,
        id,
        written: RefCell::new(Vec::new()),
    };
    // And, once the run dir is made, that dir first.
    let at = r.dir.clone();
    let with_incomplete = |e: String| beside(format!("{e}; run dir: {}", at.display()));
    // done only in the dir the report names: a run whose dir moved fails.
    let ended = run(&r).and_then(|v| match r.moved() {
        true => Err(format!("{}: the run dir moved", r.dir.display())),
        false => r.write(DONE, b"").map(|()| v),
    });
    match ended {
        Ok(v) => Ok((
            v,
            Written {
                dir: r.dir.clone(),
                incomplete,
            },
        )),
        Err(e) => Err(r.fail(mode, &e, &with_incomplete)),
    }
}

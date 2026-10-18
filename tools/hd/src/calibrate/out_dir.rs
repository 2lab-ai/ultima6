//! Where a `calibrate` run writes, in either mode ([`run_in`]). Two rules, each held by how the
//! run writes, not by a check:
//!
//! 1. **A run's output is never mixed with another run's.** `--out` is made if it is not there
//!    and opened once (a link at `--out` itself is followed). The run's first write in it is a
//!    `mkdirat` of a dir of its own, `run-<UTC time>-<pid>/`, named from this process's clock and
//!    pid alone; when that dir is there already the run fails, it is never reused. The run opens
//!    that dir (`openat`, `O_NOFOLLOW`) and makes every file of it through it ([`at`]: `openat`
//!    with `O_CREAT | O_EXCL | O_NOFOLLOW`, no temp file, no rename), never by a path: nothing
//!    outside it is written and no file there is replaced. A run dir removed while the run
//!    writes is never made again: the next write fails, and so does the run. A run dir renamed
//!    away (a link put in its place or not) is never followed: the run's writes go on in the dir
//!    it made, where that dir is now, and the run fails at its end, when it finds its dir is not
//!    at the path it made it at (checked before `done` and once `done` is written).
//! 2. **The tool never deletes what it did not make.** A run that fails deletes only the files
//!    this process made ([`Run::write`] lists each, and holds it open until the run ends, so its
//!    inode is not freed and its number cannot be given to another file meanwhile), each only
//!    while the entry at its name is that file (`fstatat`, no open), through the dir it holds
//!    open, one by one, never a tree; it keeps its diagnostics ([`Mode::keep`] and
//!    `refused.txt`). A file another hand put at a name of the run's is left, and said; so is a
//!    file of the run's whose identity could not be read when it was made. Nothing a previous
//!    process made is removed, renamed or rewritten.
//!
//! What this cannot close: a file swapped in at a name between the check and the delete (POSIX
//! has no delete-if-this-inode); a dir put in place of the new run dir, by its name in `--out`,
//! between its `mkdirat` and its `openat` (a link there is refused); and a rename of the run dir
//! after the run's last check, once `done` is written: no finite check pins a path, and a run
//! moved after it ended is outside what it reports. When the run dir cannot be opened at all,
//! it is left empty, with no `refused.txt` (there is nowhere to write one), and the next run
//! lists it as incomplete.
//!
//! A run that ends well writes `done` last, and its report names its dir: that path is the only
//! pointer to it (no link, no pointer file). A run dir with no `done`, or with a `refused.txt`,
//! is an incomplete run (one that failed, was killed, or is still going): a run lists those it
//! finds in `--out` when it starts, and leaves them as they are. Cleaning up old runs is the
//! person's job. A run holds each file it makes open until it ends, so it raises its limit on
//! open files first, as far as the system lets it. Unix only (the crate is).

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

/// How the report says the run dirs beside a run that it could not read.
const UNREADABLE: &str = "run dirs beside it that cannot be read (left as they are)";

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

/// A test seam whose error stands for a system error: a hook's `errno:<n>` is that error (as
/// the system would give it), any other text an error of its own.
fn io_seam(key: &Path, step: &'static str, what: &str) -> std::io::Result<()> {
    seam(key, step, what).map_err(|e| match e.strip_prefix("errno:").map(str::parse) {
        Some(Ok(n)) => std::io::Error::from_raw_os_error(n),
        _ => std::io::Error::other(e),
    })
}

/// `names` sorted: by time, the oldest first (a run dir's name sorts by its time,
/// [`run_name`]).
fn oldest_first(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

/// The run dirs of `out` with no [`DONE`], or with a [`REFUSED`] (`(refused)`: a run that failed,
/// whose `done` may be left too), and those that cannot be read (with why), each the oldest
/// first. Read only. `key` is `--out` as given (the test seams').
fn incomplete(key: &Path, out: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    let (mut found, mut unreadable) = (Vec::new(), Vec::new());
    for e in std::fs::read_dir(out).map_err(|e| format!("{}: {e}", out.display()))? {
        let e = e.map_err(|e| format!("{}: {e}", out.display()))?;
        let name = e.file_name().to_string_lossy().into_owned();
        let dir = e.path();
        let is_dir = std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir());
        if !name.starts_with("run-") || !is_dir {
            continue;
        }
        let has = |file: &str| {
            io_seam(key, "stat", &name)?;
            match std::fs::symlink_metadata(dir.join(file)) {
                Ok(_) => Ok(true),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e),
            }
        };
        match (has(DONE), has(REFUSED)) {
            (Ok(_), Ok(true)) => found.push(format!("{name} (refused)")),
            (Ok(true), Ok(false)) => {}
            (Ok(false), Ok(false)) => found.push(name),
            (Err(e), _) | (_, Err(e)) => unreadable.push(format!("{name} ({e})")),
        }
    }
    Ok((oldest_first(found), oldest_first(unreadable)))
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

/// A file the run made: its name, the (device, inode) it was made with (`None` when that could
/// not be read), and the file, held open until the run ends, so that its inode cannot be freed
/// and its number given to another file while the run lives.
struct Made {
    name: String,
    id: Option<(u64, u64)>,
    file: Option<File>,
}

/// A run's own dir: its path, the dir itself, open (every file of the run is made, read and
/// deleted through it, [`at`], never by a path), and what this process made in it.
pub struct Run {
    key: PathBuf,
    dir: PathBuf,
    open: File,
    id: (u64, u64),
    made: RefCell<Vec<Made>>,
}

impl Run {
    /// The run's dir, by the path it was made at: every file of the run is in it.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes `bytes` to a new file `name` of the run's dir, made by this call through the dir
    /// itself (`O_CREAT | O_EXCL | O_NOFOLLOW`: no temp file, no rename, nothing replaced; a
    /// file there already, or a dir removed meanwhile, fails it), and lists it as this process's
    /// (the only files a failed run deletes), held open until the run ends ([`Made`]).
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        use std::io::Write as _;
        one_name(name)?;
        seam(&self.key, "write", name)?;
        let path = self.dir.join(name);
        let say = |e: std::io::Error| match e.raw_os_error() {
            Some(libc::EMFILE) => format!(
                "{}: {e} (a run holds each file it makes open until it ends: raise the limit on \
                 open files, ulimit -n)",
                path.display()
            ),
            _ => format!("{}: {e}", path.display()),
        };
        let mut f = io_seam(&self.key, "create", name)
            .and_then(|()| at::create(&self.open, name))
            .map_err(say)?;
        let id = io_seam(&self.key, "identify", name)
            .and_then(|()| at::id(&f))
            .ok();
        let written = io_seam(&self.key, "fill", name)
            .and_then(|()| f.write_all(bytes))
            .and_then(|()| f.sync_all())
            .map_err(say);
        self.made.borrow_mut().push(Made {
            name: name.to_string(),
            id,
            file: Some(f),
        });
        written
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

    /// What to say of a run dir that moved: from where, and where it is now when the system
    /// says and the dir there is the run's ([`at::where_now`]).
    fn moved_note(&self) -> String {
        let now = match at::where_now(&self.open, self.id) {
            Some(p) => format!("it is now at {}", p.display()),
            None => "where it is now is not known".into(),
        };
        format!(
            "the run dir {} was moved or removed while the run wrote it ({now}): nothing is \
             written or deleted but in the dir it made",
            self.dir.display()
        )
    }

    /// The run failed with `error`: deletes the files this process made in the run's dir but
    /// what `mode` keeps, each only while the entry at its name is the file the run holds open
    /// (`fstatat` of the name, no open), through the dir itself; then writes [`REFUSED`] (the
    /// error, `then` added: each file it could not delete, and that the dir moved if it did);
    /// gives that error.
    fn fail(&self, mode: Mode, error: &str, then: &dyn Fn(String) -> String) -> String {
        let mut left = Vec::new();
        {
            let mut made = self.made.borrow_mut();
            for m in made
                .iter_mut()
                .filter(|m| !mode.keep.contains(&m.name.as_str()))
            {
                let path = self.dir.join(&m.name);
                let name = &m.name;
                let gone = seam(&self.key, "delete", name).and_then(|()| {
                    let Some(made) = m.id else {
                        return Err("could not be identified when it was made".into());
                    };
                    match at::id_at(&self.open, name) {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        Err(e) => Err(e.to_string()),
                        Ok(now) if now != made => Err("not the file the run made".into()),
                        Ok(_) => match at::unlink(&self.open, name) {
                            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                                Err(e.to_string())
                            }
                            _ => Ok(()),
                        },
                    }
                });
                match gone {
                    // Deleted: its fd is let go (for refused.txt, when fds ran short).
                    Ok(()) => m.file = None,
                    Err(e) => left.push(format!("{} ({e})", path.display())),
                }
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
            let note = self.moved_note();
            if !said.contains(&note) {
                said = format!("{said}; {note}");
            }
        }
        let said = then(said);
        match self.write(REFUSED, format!("{said}\n").as_bytes()) {
            Ok(()) => said,
            Err(e) => format!("{said}; {REFUSED} not written ({e})"),
        }
    }
}

/// What a run that ends well says of where it wrote: its dir, and the incomplete runs and the
/// run dirs it could not read that it found in `--out` when it started (left as they are).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub dir: PathBuf,
    pub incomplete: Vec<String>,
    pub unreadable: Vec<String>,
}

impl Written {
    /// The report's lines of it.
    pub fn text(&self) -> String {
        let mut s = format!("run dir: {}\n", self.dir.display());
        if !self.incomplete.is_empty() {
            s.push_str(&format!(
                "incomplete runs beside it (no {DONE}, or refused; left as they are): {}\n",
                self.incomplete.join(", ")
            ));
        }
        if !self.unreadable.is_empty() {
            s.push_str(&format!("{UNREADABLE}: {}\n", self.unreadable.join(", ")));
        }
        s
    }
}

/// Runs a `calibrate` run of `mode` whose `--out` is `out` (module docs): `out` made if it is
/// not there, and opened; the incomplete runs in it listed; the run's own dir made in it
/// (`mkdirat`, a failure if it is there) and opened (`openat`, not a link); `run` writes in it
/// ([`Run`]); then [`DONE`], and the dir checked to be where it was made. A run that fails keeps
/// only its diagnostics ([`Run::fail`]).
pub fn run_in<T>(
    mode: Mode,
    out: &Path,
    run: impl FnOnce(&Run) -> Result<T, String>,
) -> Result<(T, Written), String> {
    at::raise_open_files();
    work::check_target(out)?;
    let parent = work::ensure_dir(out)?;
    let outfd = at::open_dir(&parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let (incomplete, unreadable) = incomplete(out, &parent)?;
    // Every failure from here says the incomplete runs beside the run, and those it could not
    // read.
    let beside = |e: String| {
        let e = match incomplete.as_slice() {
            [] => e,
            v => format!(
                "{e}; incomplete runs beside it (left as they are): {}",
                v.join(", ")
            ),
        };
        match unreadable.as_slice() {
            [] => e,
            v => format!("{e}; {UNREADABLE}: {}", v.join(", ")),
        }
    };
    let name = run_name(now(out), std::process::id());
    let dir = parent.join(&name);
    io_seam(out, "mkdir", &name)
        .and_then(|()| at::mkdir(&outfd, &name))
        .map_err(|e| {
            let why = match e.kind() {
                std::io::ErrorKind::AlreadyExists => " (a run dir is never reused)",
                _ => "",
            };
            beside(format!("{}: {e}{why}", dir.display()))
        })?;
    // And, once the run dir is made, that dir first.
    let at = dir.clone();
    let with_incomplete = |e: String| beside(format!("{e}; run dir: {}", at.display()));
    let unopened = |e: std::io::Error| {
        with_incomplete(format!(
            "{}: {e}: the run dir is left empty, with no {REFUSED} (it could not be opened to \
             write one); the next run lists it as incomplete",
            dir.display()
        ))
    };
    let open = io_seam(out, "open", &name)
        .and_then(|()| at::open_dir_at(&outfd, &name))
        .map_err(unopened)?;
    let id = at::id(&open).map_err(unopened)?;
    let r = Run {
        key: out.to_path_buf(),
        dir,
        open,
        id,
        made: RefCell::new(Vec::new()),
    };
    // done only in the dir the report names: a run whose dir moved, before done or once it is
    // written, fails (and done, then made, is deleted with the rest).
    let here = |v| match r.moved() {
        true => Err(r.moved_note()),
        false => Ok(v),
    };
    let ended = run(&r)
        .and_then(here)
        .and_then(|v| r.write(DONE, b"").map(|()| v))
        .and_then(here);
    match ended {
        Ok(v) => Ok((
            v,
            Written {
                dir: r.dir.clone(),
                incomplete,
                unreadable,
            },
        )),
        Err(e) => Err(r.fail(mode, &e, &with_incomplete)),
    }
}

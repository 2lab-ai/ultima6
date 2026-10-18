//! Where a `calibrate` run writes, in either mode ([`run_in`]). Two rules, each held by how the
//! run writes, not by a check:
//!
//! 1. **A run's output is never mixed with another run's.** Each run's first write to the
//!    filesystem is a `create_dir` of a dir of its own, `<--out>/run-<UTC time>-<pid>/`, named
//!    from this process's clock and pid alone; when that dir is there already the run fails, it
//!    is never reused. Everything the run writes goes into it, and nothing outside it.
//! 2. **The tool never deletes what it did not make.** A run that fails deletes only the files
//!    this process wrote ([`Run::write`] lists each), only in its own run dir, one by one, never
//!    a tree; it keeps its diagnostics ([`Mode::keep`] and `refused.txt`). Nothing a previous
//!    process made is removed, renamed or rewritten.
//!
//! A run that ends well writes `done` last, and its report names its dir: that path is the only
//! pointer to it (no link, no pointer file). A run dir with no `done` is an incomplete run (one
//! that failed, was killed, or is still going): a run lists those it finds in `--out` when it
//! starts, and leaves them as they are. Cleaning up old runs is the person's job.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::img::{self, Rgba};
use crate::work;

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

/// A run's own dir, and what this process wrote in it.
pub struct Run {
    key: PathBuf,
    dir: PathBuf,
    written: RefCell<Vec<String>>,
}

impl Run {
    /// The run's dir: every file of the run is in it.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes `bytes` to the file `name` of the run's dir, and lists it as written by this
    /// process (the only files a failed run deletes).
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        seam(&self.key, "write", name)?;
        work::write_file(&self.dir.join(name), bytes)?;
        let mut w = self.written.borrow_mut();
        if !w.iter().any(|n| n == name) {
            w.push(name.to_string());
        }
        Ok(())
    }

    /// [`Run::write`] of `img` as a PNG.
    pub fn write_png(&self, name: &str, img: &Rgba) -> Result<(), String> {
        let path = self.dir.join(name);
        let bytes = img::encode_png(img).map_err(|e| format!("{}: {e}", path.display()))?;
        self.write(name, &bytes)
    }

    /// The run failed with `error`: deletes the files this process wrote in the run's dir but
    /// what `mode` keeps, then writes [`REFUSED`] (the error, and each file it could not
    /// delete); gives the error to report.
    fn fail(&self, mode: Mode, error: &str) -> String {
        let mut left = Vec::new();
        let written = self.written.borrow().clone();
        for name in written.iter().filter(|n| !mode.keep.contains(&n.as_str())) {
            let path = self.dir.join(name);
            let gone = seam(&self.key, "delete", name)
                .and_then(|()| std::fs::remove_file(&path).map_err(|e| e.to_string()));
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
        if let Err(e) = self.write(REFUSED, format!("{said}\n").as_bytes()) {
            said = format!("{said}; {REFUSED} not written ({e})");
        }
        said
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
    work::check_target(out)?;
    let parent = work::ensure_dir(out)?;
    let incomplete = incomplete(&parent)?;
    let dir = parent.join(run_name(now(out), std::process::id()));
    std::fs::create_dir(&dir)
        .map_err(|e| format!("{}: {e} (a run dir is never reused)", dir.display()))?;
    let r = Run {
        key: out.to_path_buf(),
        dir,
        written: RefCell::new(Vec::new()),
    };
    let with_incomplete = |e: String| match incomplete.as_slice() {
        [] => e,
        v => format!(
            "{e}; incomplete runs beside it (left as they are): {}",
            v.join(", ")
        ),
    };
    match run(&r).and_then(|v| r.write(DONE, b"").map(|()| v)) {
        Ok(v) => Ok((
            v,
            Written {
                dir: r.dir.clone(),
                incomplete,
            },
        )),
        Err(e) => Err(with_incomplete(r.fail(mode, &e))),
    }
}

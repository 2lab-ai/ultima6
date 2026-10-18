//! The out dir of a `calibrate` run, of either mode ([`run_in`]): the run writes to a fresh
//! directory beside `--out`, which holds its mode's marker file, and that directory is put in
//! place of `--out` at the end ([`publish`]). A run that fails publishes none of what it wrote
//! but its refusal: a fresh directory of `refused.txt` and the snapshots and details the mode
//! keeps ([`Mode::keep`]), never a sheet or `samples.tsv`. After any run `--out` holds that
//! run's output alone, or what was there before when the run could not replace it. An old
//! `--out` is replaced only when it holds the marker of the mode run and nothing but what its
//! marker lists (the marker names every entry the run wrote, [`seal`]): a directory the tool
//! did not write, an empty one, one the other mode wrote, or one a file was put in, is left as
//! it is and the run fails, the files it did not write named.

use std::collections::BTreeSet;
use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::work;

#[cfg(test)]
mod tests;

/// A mode of `calibrate`: its name in messages, the file its out dirs hold, and what a failed
/// run's refusal keeps of what it wrote (its snapshots, and the details its error points to).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub name: &'static str,
    pub marker: &'static str,
    pub keep: &'static [&'static str],
}

/// The first pass's sample (`calibrate`).
pub const FIRST: Mode = Mode {
    name: "calibrate",
    marker: ".calibrate-first-pass",
    keep: &["gates.jsonl"],
};

/// The read of the final pack (`calibrate --final`).
pub const FINAL: Mode = Mode {
    name: "calibrate --final",
    marker: ".calibrate-final",
    keep: &[
        "gates.jsonl",
        "ingest.json",
        "jobs.jsonl",
        "disagreements.txt",
        "unshown.txt",
        "changed.txt",
    ],
};

/// Tests only: a hook at a step of a run ([`seam`]); its error is the step's.
#[cfg(test)]
pub(crate) type Hook = std::sync::Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// Tests only: the hooks, by the `--out` of the run as given and the step.
#[cfg(test)]
pub(crate) static HOOKS: std::sync::Mutex<Vec<(PathBuf, &'static str, Hook)>> =
    std::sync::Mutex::new(Vec::new());

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

/// `out`'s directory (made absolute, checked and created) and its name.
fn split(out: &Path) -> Result<(PathBuf, String), String> {
    let name = out
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: not a directory name", out.display()))?
        .to_string();
    let parent = match out.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    Ok((work::ensure_dir(&parent)?, name))
}

/// A name beside the out dir `out` that nothing has: `.<name>.<kind>-<pid>-<ns>`.
fn beside(out: &Path, kind: &str) -> PathBuf {
    let name = out
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let stamp = format!("{}-{}", std::process::id(), work::now_ns());
    out.with_file_name(format!(".{name}.{kind}-{stamp}"))
}

/// A fresh directory beside `out` holding `mode`'s marker (its first line alone: the run's
/// entries are listed when it ends, [`seal`]).
fn fresh(mode: Mode, out: &Path, kind: &str) -> Result<PathBuf, String> {
    let dir = beside(out, kind);
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let note = format!("{}\n", mode.name);
    if let Err(e) = work::write_file(&dir.join(mode.marker), note.as_bytes()) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(dir)
}

/// The entries of `dir` but `mode`'s marker.
fn entries(mode: Mode, dir: &Path) -> io::Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for e in std::fs::read_dir(dir)? {
        let name = e?.file_name().to_string_lossy().into_owned();
        if name != mode.marker {
            out.insert(name);
        }
    }
    Ok(out)
}

/// Writes `dir`'s marker whole: the mode's name, then every entry of `dir` (the files the run
/// wrote), a line each. An out dir is the tool's to replace only with nothing in it but these.
fn seal(mode: Mode, dir: &Path) -> Result<(), String> {
    let names = entries(mode, dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut text = format!("{}\n", mode.name);
    for n in &names {
        text.push_str(n);
        text.push('\n');
    }
    work::write_file(&dir.join(mode.marker), text.as_bytes())
}

/// What `dir`'s marker of `mode` lists (`None`: no marker, a file).
fn listed(mode: Mode, dir: &Path) -> Option<BTreeSet<String>> {
    let path = dir.join(mode.marker);
    if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    Some(text.lines().skip(1).map(str::to_string).collect())
}

/// The entries of `dir` its marker of `mode` does not list (each `mode` did not write), or why
/// they cannot be read.
fn unknown(mode: Mode, dir: &Path, list: &BTreeSet<String>) -> Result<Vec<String>, String> {
    let names = entries(mode, dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(names.difference(list).cloned().collect())
}

/// Why an out dir is not replaced, and how to go on.
struct Kept {
    why: String,
    recover: &'static str,
}

impl Kept {
    fn of(why: impl Into<String>) -> Kept {
        Kept {
            why: why.into(),
            recover: RECOVER,
        }
    }
}

/// Why `dir` is not an out dir `mode` may replace (`None`: it is one, a directory, not a
/// link, holding the mode's marker).
fn not_ours(mode: Mode, dir: &Path) -> Option<Kept> {
    let m = match std::fs::symlink_metadata(dir) {
        Ok(m) => m,
        Err(e) => return Some(Kept::of(e.to_string())),
    };
    if m.file_type().is_symlink() {
        return Some(Kept {
            why: "a symlink (a rename would move the link, not the directory it points to)".into(),
            recover: "pass the directory it points to as --out, or another --out",
        });
    }
    if !m.is_dir() {
        return Some(Kept::of("not a directory"));
    }
    let marked =
        |marker: &str| std::fs::symlink_metadata(dir.join(marker)).is_ok_and(|m| m.is_file());
    if let Some(list) = listed(mode, dir) {
        return match unknown(mode, dir, &list) {
            Ok(v) if v.is_empty() => None,
            Ok(v) => Some(Kept {
                why: format!(
                    "it holds what {} did not write: {}",
                    mode.name,
                    v.join(", ")
                ),
                recover: "move those out of it, or pass another --out",
            }),
            Err(e) => Some(Kept::of(e)),
        };
    }
    let other = [FIRST, FINAL]
        .into_iter()
        .find(|m| *m != mode && marked(m.marker))
        .map_or(String::new(), |m| format!("; it is {}'s output", m.name));
    Some(Kept::of(format!(
        "a directory {} did not write (no marker {}{other})",
        mode.name, mode.marker
    )))
}

/// How to go on when the out dir `out` is not replaced.
const RECOVER: &str = "move it away, or pass another --out";

/// The refusal of `out`: why, so it is not replaced, and how to go on.
fn refusal(out: &Path, k: &Kept) -> String {
    format!(
        "{}: {}, so it is not replaced: {}",
        out.display(),
        k.why,
        k.recover
    )
}

/// Renames `from` to `to` only when nothing is at `to`, in one step that cannot replace
/// (macOS: `renamex_np` with `RENAME_EXCL`); an error (`EEXIST`) when something is.
#[cfg(target_os = "macos")]
fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    let (f, t) = (c_path(from)?, c_path(to)?);
    // SAFETY: two NUL-terminated paths that outlive the call; renamex_np(2) reads them only.
    let r = unsafe { libc::renamex_np(f.as_ptr(), t.as_ptr(), libc::RENAME_EXCL) };
    if r == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Renames `from` to `to` only when nothing is at `to`, in one step that cannot replace
/// (Linux: `renameat2` with `RENAME_NOREPLACE`); an error (`EEXIST`) when something is.
#[cfg(target_os = "linux")]
fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    let (f, t) = (c_path(from)?, c_path(to)?);
    // SAFETY: two NUL-terminated paths that outlive the call; renameat2(2) reads them only.
    let r = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            f.as_ptr(),
            libc::AT_FDCWD,
            t.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if r == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// No rename that cannot replace on this system: the out dir is never replaced.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn rename_new(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no rename that cannot replace on this system",
    ))
}

fn c_path(p: &Path) -> io::Result<CString> {
    CString::new(p.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL in the path"))
}

/// Puts the directory `stage` (holding `mode`'s marker, beside `out`) in place of the out dir
/// `out`; `key` is `--out` as given (the test seams'). The steps:
///
/// 1. `out`, when there is one, is renamed to a name beside it that nothing has (`aside`);
/// 2. `aside` is checked: a directory, not a link, holding `mode`'s marker;
/// 3. when it is not, it is renamed back (never over something at `out` since) and the run
///    refused;
/// 4. `stage` is renamed to `out` by a rename that cannot replace ([`rename_new`]);
/// 5. when something is at `out` by then, the old dir is put back if it can be (else it stays
///    at `aside`, said so) and the run refused;
/// 6. `aside` is emptied of what its marker lists, the marker, and removed ([`remove_old`]):
///    anything else in it by then is left, with it, and said.
///
/// `stage` is never removed here: the caller removes it when this fails. The window this cannot
/// close: from 1 to 4 nothing is at `out`, so a crash there leaves no `--out`, the old output
/// whole at `aside` (`.<name>.old-<pid>-<ns>` beside it), to be moved back by hand. A crash
/// after 4 and before 6 leaves the new output at `out` and the old at `aside`. A swap in one
/// step (`RENAME_SWAP`, `RENAME_EXCHANGE`) would close the first window; it is not used.
fn publish(mode: Mode, stage: &Path, out: &Path, key: &Path) -> Result<Vec<String>, String> {
    let aside = beside(out, "old");
    let moved = match rename_new(out, &aside) {
        Ok(()) => true,
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(refusal(out, &Kept::of(e.to_string()))),
    };
    seam(key, "moved", "")?;
    if moved && let Some(why) = not_ours(mode, &aside) {
        return Err(match rename_new(&aside, out) {
            Ok(()) => refusal(out, &why),
            Err(e) => format!(
                "{}; it is at {} now: something took {} meanwhile ({e}); move it back by hand",
                refusal(out, &why),
                aside.display(),
                out.display()
            ),
        });
    }
    seam(key, "checked", "")?;
    if let Err(e) = rename_new(stage, out) {
        let why = Kept::of(format!("something is at it since the run began ({e})"));
        let back = match moved {
            false => String::new(),
            true => match rename_new(&aside, out) {
                Ok(()) => String::new(),
                Err(_) => format!("; the old output is at {}", aside.display()),
            },
        };
        return Err(format!("{}{back}", refusal(out, &why)));
    }
    seam(key, "published", "")?;
    let mut notes = Vec::new();
    if moved && let Err(e) = remove_old(mode, &aside) {
        notes.push(format!(
            "the previous output is left at {}: {e}",
            aside.display()
        ));
    }
    Ok(notes)
}

/// Removes the old out dir `dir` of `mode`: what its marker lists, the marker, then the dir;
/// nothing else (a dir holding anything else is left, and why said).
fn remove_old(mode: Mode, dir: &Path) -> Result<(), String> {
    let list = listed(mode, dir).ok_or("its marker is gone")?;
    let other = unknown(mode, dir, &list)?;
    if !other.is_empty() {
        return Err(format!(
            "it holds what {} did not write: {}",
            mode.name,
            other.join(", ")
        ));
    }
    for name in list.iter().map(String::as_str).chain([mode.marker]) {
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{name}: {e}")),
        }
    }
    std::fs::remove_dir(dir).map_err(|e| e.to_string())
}

/// Runs a `calibrate` run of `mode` whose out dir is `out` (module docs): an `out` there that
/// this mode may not replace ([`not_ours`]) is refused before the run; `run` writes to a fresh
/// directory (its first argument; the second is the out dir it is to be, for messages), which
/// is put in place of `out` at the end ([`publish`]), its marker listing what it holds
/// ([`seal`]). When `run` fails, its refusal is put in place of `out` instead ([`refuse`]) and
/// the directory it wrote removed. With the run's value, what to say of the out dir (an old
/// one left, and why).
pub fn run_in<T>(
    mode: Mode,
    out: &Path,
    run: impl FnOnce(&Path, &Path) -> Result<T, String>,
) -> Result<(T, Vec<String>), String> {
    let key = out;
    work::check_target(out)?;
    let (parent, name) = split(out)?;
    let out = parent.join(name);
    if std::fs::symlink_metadata(&out).is_ok()
        && let Some(why) = not_ours(mode, &out)
    {
        return Err(refusal(&out, &why));
    }
    let stage = fresh(mode, &out, "staging")?;
    match run(&stage, &out).and_then(|v| seal(mode, &stage).map(|()| v)) {
        Ok(v) => {
            let published = seam(key, "ran", "").and_then(|()| publish(mode, &stage, &out, key));
            match published {
                Ok(notes) => Ok((v, notes)),
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&stage);
                    Err(e)
                }
            }
        }
        Err(e) => {
            let e = as_out(&e, &stage, &out);
            let mut said = e.clone();
            match refuse(mode, &stage, &out, key, &e) {
                Ok(notes) => {
                    for n in notes {
                        said = format!("{said}; {n}");
                    }
                }
                Err(w) => said = format!("{said}; {w}"),
            }
            // What the failed run wrote is never published: removed, whatever it holds.
            let gone = seam(key, "cleanup", &stage.to_string_lossy()).and_then(|()| {
                std::fs::remove_dir_all(&stage).map_err(|w| format!("{}: {w}", stage.display()))
            });
            if let Err(w) = gone {
                said = format!("{said}; what the run wrote is left beside the out dir ({w})");
            }
            Err(said)
        }
    }
}

/// `error` with the directory the run wrote to, `stage`, named as the out dir it was to be,
/// `out` (a file the error names is one of `out` now, or would have been); `stage` said once,
/// at the end, as where the run wrote first.
fn as_out(error: &str, stage: &Path, out: &Path) -> String {
    let at = stage.to_string_lossy();
    if !error.contains(at.as_ref()) {
        return error.to_string();
    }
    format!(
        "{} (the run wrote to {at} first, which is not kept)",
        error.replace(at.as_ref(), &out.to_string_lossy())
    )
}

/// The refusal of a run of `mode` that failed with `error`: a fresh directory holding the
/// mode's marker, `refused.txt` (the error) and, of the files the run wrote to `failed`, those
/// the mode keeps ([`Mode::keep`]: never a sheet, never `samples.tsv`), put in place of `out`
/// ([`publish`]). Nothing is published when it cannot be made whole.
fn refuse(
    mode: Mode,
    failed: &Path,
    out: &Path,
    key: &Path,
    error: &str,
) -> Result<Vec<String>, String> {
    let diag = fresh(mode, out, "refusal")?;
    let made = (|| {
        for &name in mode.keep {
            let from = failed.join(name);
            if !std::fs::symlink_metadata(&from).is_ok_and(|m| m.is_file()) {
                continue;
            }
            seam(key, "refusal", name)?;
            let bytes = std::fs::read(&from).map_err(|e| format!("{}: {e}", from.display()))?;
            work::write_file(&diag.join(name), &bytes)?;
        }
        work::write_file(&diag.join("refused.txt"), format!("{error}\n").as_bytes())?;
        seal(mode, &diag)?;
        seam(key, "ran", "")?;
        publish(mode, &diag, out, key)
    })();
    if made.is_err() {
        let _ = std::fs::remove_dir_all(&diag);
    }
    made
}

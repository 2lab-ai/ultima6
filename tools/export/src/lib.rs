//! `u6-export`: the two downloads the page once offered, made natively from the user's own files,
//! for the tools that read them (`scripts/make-packs.sh` runs it before `tools/i18n/translate.mjs`
//! and `tools/speech/profile.mjs`):
//!
//! ```text
//! u6-export strings --game PATH --out FILE   the game strings (u6.string-bundle/1), as Game strings
//! u6-export bundles --game PATH --out FILE   the NPC bundles (u6.npc-bundle/1), as NPC bundles
//! ```
//!
//! `--game` takes what the page takes (the archive.org zip, the ADFs or HFEs, or a folder of any
//! of them or of extracted files), read through the same intake. Each export keeps the files the
//! page's exporter kept (its `GameStrings::add`, `NpcBundles::add` in `crates/u6-web`, removed
//! 2026-10-21) and calls the same function of `u6-core`, so its JSON is that export byte for byte. It holds the user's
//! game text: the file is written 0600 (the directories it makes, 0700), never inside a git work
//! tree nor over a game file it read or inside a folder of them, and nothing of it is printed:
//! one line names the file and counts its entries. An error names only what failed.

use std::fs::{DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use u6_core::Files;
use u6_core::converse::disasm::bundle;
use u6_core::locale::strings;
use u6_intake::{InputFile, intake};

pub mod synthetic;
#[cfg(test)]
mod tests;

pub const USAGE: &str = "usage:
  u6-export strings --game PATH --out FILE
  u6-export bundles --game PATH --out FILE";

/// Which of the page's exports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Export {
    /// Game strings (`u6_core::locale::strings`).
    Strings,
    /// NPC bundles (`u6_core::converse::disasm::bundle`).
    Bundles,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub export: Export,
    pub game: PathBuf,
    pub out: PathBuf,
}

/// The command line after the program's name; an error is a usage error.
pub fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let export = match it.next().as_deref() {
        Some("strings") => Export::Strings,
        Some("bundles") => Export::Bundles,
        Some(other) => return Err(format!("unknown command {other:?}")),
        None => return Err("no command".into()),
    };
    let (mut game, mut out) = (None, None);
    while let Some(arg) = it.next() {
        let mut value = |what: &str| it.next().ok_or(format!("{arg} needs {what}"));
        match arg.as_str() {
            "--game" => game = Some(PathBuf::from(value("a path")?)),
            "--out" => out = Some(PathBuf::from(value("a file")?)),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Args {
        export,
        game: game.ok_or("give --game PATH (your game files)")?,
        out: out.ok_or("give --out FILE")?,
    })
}

/// Runs the export: the line to print (the file written and its count of entries), or what
/// failed.
pub fn run(a: &Args) -> Result<String, String> {
    let keep = match a.export {
        Export::Strings => strings::reads,
        Export::Bundles => bundle::reads,
    };
    // The target is checked before the game files are read, and against them before anything
    // is written.
    check_target(&a.out)?;
    let target = resolved_target(&a.out)?;
    let (files, sources) = read_files(&a.game, keep)?;
    if sources.holds(&target) {
        return Err(format!(
            "{}: one of the game files read, or inside a folder of them; the export is written \
             only elsewhere",
            a.out.display()
        ));
    }
    let (json, counts) = json(a.export, &files)?;
    let written = write_private(&a.out, json.as_bytes())?;
    Ok(format!("wrote {} ({counts})", written.display()))
}

/// The export's JSON of `files` and its count of entries: what `crates/u6-web`'s
/// `GameStrings::strings` and `NpcBundles::bundles` gave (removed 2026-10-21), their errors' words
/// included.
pub fn json(export: Export, files: &Files) -> Result<(String, String), String> {
    match export {
        Export::Strings => {
            let e = strings::export(files).map_err(|e| format!("Game strings: {e}"))?;
            let counts = format!(
                "{} game strings, {} object names and books, {} strings of u",
                e.strings.len(),
                e.texts.len(),
                e.u.len()
            );
            Ok((e.to_json(), counts))
        }
        Export::Bundles => {
            let b = bundle::export(files).map_err(|e| format!("NPC bundles: {e}"))?;
            Ok((bundle::to_json(&b), format!("{} bundles", b.len())))
        }
    }
}

/// Where the game files were read from: the resolved path of every file read and of every
/// folder walked (through any link).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sources {
    pub files: Vec<PathBuf>,
    pub folders: Vec<PathBuf>,
}

impl Sources {
    /// Whether `target` (resolved) is a file read or lies inside a folder walked.
    pub fn holds(&self, target: &Path) -> bool {
        self.files.iter().any(|f| f == target) || self.folders.iter().any(|d| target.starts_with(d))
    }
}

/// The user's files at `path` (a zip, an HFE or ADF image, or a folder of any of them or of
/// extracted files) through the page's intake, as `u6-hd` reads them (`tools/hd/src/game.rs`),
/// keeping only the files `keep` takes, as the page's exporter does; and where they were read
/// from.
pub fn read_files(path: &Path, keep: fn(&str) -> bool) -> Result<(Files, Sources), String> {
    let failed = |e: String| format!("the game files cannot be read: {e}");
    let mut inputs = Vec::new();
    let mut sources = Sources::default();
    collect(path, path, &mut inputs, &mut sources).map_err(failed)?;
    if inputs.is_empty() {
        return Err(failed(format!("{}: no files", path.display())));
    }
    let mut files = intake(inputs).files;
    files.retain(|name, _| keep(name));
    Ok((Files::new(files), sources))
}

fn collect(
    root: &Path,
    path: &Path,
    out: &mut Vec<InputFile>,
    sources: &mut Sources,
) -> Result<(), String> {
    let resolved = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if path.is_dir() {
        sources.folders.push(resolved);
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for p in entries {
            collect(root, &p, out, sources)?;
        }
        return Ok(());
    }
    sources.files.push(resolved);
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let rel = path.strip_prefix(root).unwrap_or(path);
    let name = if rel.as_os_str().is_empty() {
        path.file_name()
            .map_or("input".into(), |n| n.to_string_lossy().into_owned())
    } else {
        rel.to_string_lossy().into_owned()
    };
    out.push(InputFile { name, data });
    Ok(())
}

/// Refuses `out` when its nearest existing ancestor, resolved, lies inside a git work tree (a
/// directory with a `.git` entry above it): the export holds the user's game text and never
/// goes where a commit could take it.
pub fn check_target(out: &Path) -> Result<(), String> {
    let abs = std::path::absolute(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let existing = abs
        .ancestors()
        .find(|a| std::fs::symlink_metadata(a).is_ok())
        .ok_or_else(|| format!("{}: no existing ancestor", out.display()))?;
    let resolved = existing
        .canonicalize()
        .map_err(|e| format!("{}: {e}", existing.display()))?;
    if let Some(tree) = resolved
        .ancestors()
        .find(|a| std::fs::symlink_metadata(a.join(".git")).is_ok())
    {
        return Err(format!(
            "{}: inside the git work tree {}; the export holds your game text and is written \
             only outside one",
            out.display(),
            tree.display()
        ));
    }
    Ok(())
}

/// `out` as it will be written: its nearest existing ancestor resolved (through every link),
/// then the rest, which does not exist and so holds no link; a `..` in that rest is refused
/// (the path it names is not known before the folders are made).
pub fn resolved_target(out: &Path) -> Result<PathBuf, String> {
    let abs = std::path::absolute(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let existing = abs
        .ancestors()
        .find(|a| std::fs::symlink_metadata(a).is_ok())
        .ok_or_else(|| format!("{}: no existing ancestor", out.display()))?;
    let mut target = existing
        .canonicalize()
        .map_err(|e| format!("{}: {e}", existing.display()))?;
    let rest = abs.strip_prefix(existing).unwrap_or(Path::new(""));
    for c in rest.components() {
        match c {
            Component::Normal(n) => target.push(n),
            Component::CurDir => {}
            _ => {
                return Err(format!(
                    "{}: a `..` after a folder that does not exist is refused",
                    out.display()
                ));
            }
        }
    }
    Ok(target)
}

/// Writes `bytes` to `out` with mode 0600, its missing directories made 0700 (an existing one
/// is left as it is): a fresh temporary file beside it (`O_EXCL`, so never an existing name or a
/// link) renamed over it; refused where `out` exists and is not a plain file. The path written.
pub fn write_private(out: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    let abs = std::path::absolute(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let (Some(parent), Some(name)) = (abs.parent(), abs.file_name()) else {
        return Err(format!("{}: not a file path", out.display()));
    };
    check_target(&abs)?;
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|e| format!("{}: {e}", parent.display()))?;
    let dir = parent
        .canonicalize()
        .map_err(|e| format!("{}: {e}", parent.display()))?;
    let target = dir.join(name);
    // Again, resolved: a link among the directories made is followed now.
    check_target(&target)?;
    let plain = |p: &Path| std::fs::symlink_metadata(p).map_or(true, |m| m.file_type().is_file());
    if !plain(&target) {
        return Err(format!(
            "{}: exists and is not a plain file",
            target.display()
        ));
    }
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    // The mode exactly, whatever the umask.
    let written = f
        .set_permissions(std::fs::Permissions::from_mode(0o600))
        .and_then(|()| f.write_all(bytes))
        .and_then(|()| f.sync_all());
    drop(f);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", tmp.display()));
    }
    if !plain(&target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "{}: exists and is not a plain file",
            target.display()
        ));
    }
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", target.display())
    })?;
    Ok(target)
}

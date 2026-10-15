//! The work dir (default `~/Games/ultima6/hd/`), where every output lives: never inside the
//! repository this tool was built from (spec 07 Rights).
//!
//! ```text
//! jobs.jsonl          one line per sheet (atlas, then the re-rolls ingest adds)
//! sheets/<id>.png     what run sends
//! codex/<id>/         the codex call's working dir and its event log
//! out/<id>.png        what came back
//! runs.jsonl          one line per call: sheet, ok, seconds
//! gates.jsonl         one line per image per attempt: ids and metrics
//! slices/<image>.png  the images that passed: the ×8 RGBA masters (the pack is ×4)
//! ingest.json         the images pack takes: id, hash, slice
//! coverage.txt        records by class: generated / fallback / failed / pending (counts)
//! pack/u6-hdpack.zip  the pack
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};

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

/// `path` with its nearest existing ancestor resolved (the rest appended as given).
fn resolved(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut p = path.to_path_buf();
    loop {
        if let Ok(c) = p.canonicalize() {
            let mut out = c;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (p.file_name().map(|n| n.to_os_string()), p.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                p = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Refuses an output path inside the repository.
pub fn check_outside_repo(path: &Path) -> Result<(), String> {
    let p = resolved(&std::path::absolute(path).map_err(|e| e.to_string())?);
    let repo = resolved(&repo_root());
    if p.starts_with(&repo) {
        return Err(format!(
            "{}: inside the repository; the tool writes only outside it (spec 07 Rights)",
            path.display()
        ));
    }
    Ok(())
}

impl Work {
    /// The work dir at `dir`, or the default; refused inside the repository.
    pub fn open(dir: Option<&Path>) -> Result<Work, String> {
        let root = match dir {
            Some(d) => d.to_path_buf(),
            None => {
                let home = std::env::var_os("HOME").ok_or("HOME is not set; pass --work")?;
                Path::new(&home).join(DEFAULT_DIR)
            }
        };
        check_outside_repo(&root)?;
        // Absolute, since the codex calls run in their own working dirs.
        let root = std::path::absolute(&root).map_err(|e| format!("{}: {e}", root.display()))?;
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

    pub fn codex_dir(&self, id: &str) -> PathBuf {
        self.root.join("codex").join(id)
    }

    pub fn slice(&self, stem: &str) -> PathBuf {
        self.root.join("slices").join(format!("{stem}.png"))
    }

    pub fn mkdirs(&self) -> Result<(), String> {
        for d in ["sheets", "out", "codex", "slices", "pack"] {
            std::fs::create_dir_all(self.root.join(d))
                .map_err(|e| format!("{}: {e}", self.root.join(d).display()))?;
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

    pub fn append_jobs(&self, jobs: &[Job]) -> Result<(), String> {
        let p = self.path("jobs.jsonl");
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .map_err(|e| format!("{}: {e}", p.display()))?;
        for j in jobs {
            let line = serde_json::to_string(j).map_err(|e| e.to_string())?;
            writeln!(f, "{line}").map_err(|e| format!("{}: {e}", p.display()))?;
        }
        Ok(())
    }

    /// Writes `text` to `rel` (through a temporary file).
    pub fn write(&self, rel: &str, text: &[u8]) -> Result<(), String> {
        let p = self.path(rel);
        let tmp = p.with_extension("tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &p).map_err(|e| format!("{}: {e}", p.display()))
    }
}

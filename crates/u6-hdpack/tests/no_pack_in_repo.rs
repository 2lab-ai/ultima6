//! Spec 07 Rights / Acceptance 4: an HD pack is never committed and the site never serves one.
//! Every file git tracks and every file under `site/` (built output included) is checked for a
//! pack: a zip holding a `manifest.json` with the pack's format string, or a loose
//! `manifest.json` / `index.bin` of a pack. The format string itself may appear in source (this
//! crate, the page's drop classifier); a file is a pack by its shape, not by the string.

use std::path::{Path, PathBuf};
use std::process::Command;

use u6_hdpack::{FORMAT, Generator, INDEX_MAGIC, write_pack};
use u6_intake::zip;

fn holds_format(bytes: &[u8]) -> bool {
    bytes.windows(FORMAT.len()).any(|w| w == FORMAT.as_bytes())
}

/// Whether the file `name` with `bytes` is a pack or a part of one.
fn is_pack_file(name: &str, bytes: &[u8]) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name);
    if base == "manifest.json" && holds_format(bytes) {
        return true;
    }
    if base == "index.bin" && bytes.starts_with(INDEX_MAGIC) {
        return true;
    }
    zip::is_zip(bytes)
        && zip::read_zip(bytes, |n| n.rsplit('/').next() == Some("manifest.json"))
            .is_ok_and(|files| files.iter().any(|(_, b)| holds_format(b)))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn the_detector_finds_a_pack() {
    let g = Generator {
        name: "test".into(),
        version: "0".into(),
        engine: "none".into(),
        date: "2026-10-16".into(),
    };
    let pack = write_pack(&[], &g, true).unwrap();
    assert!(is_pack_file("x/anything.bin", &pack));
    let files = zip::read_zip(&pack, |_| true).unwrap();
    for (name, bytes) in &files {
        assert!(is_pack_file(&format!("site/{name}"), bytes), "{name}");
    }
    assert!(!is_pack_file(
        "crates/u6-hdpack/src/lib.rs",
        FORMAT.as_bytes()
    ));
    assert!(!is_pack_file(
        "site/app.js",
        b"if (format === 'u6-hdpack/1') {}"
    ));
}

#[test]
fn no_pack_is_tracked_or_under_site() {
    let root = repo_root();
    let out = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z"])
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git ls-files failed");
    let mut paths: Vec<PathBuf> = out
        .stdout
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| root.join(String::from_utf8_lossy(p).as_ref()))
        .collect();
    assert!(!paths.is_empty(), "no tracked files found");
    walk(&root.join("site"), &mut paths);
    let found: Vec<String> = paths
        .iter()
        .filter(|p| {
            std::fs::read(p)
                .is_ok_and(|b| is_pack_file(&p.to_string_lossy().replace('\\', "/"), &b))
        })
        .map(|p| p.strip_prefix(&root).unwrap_or(p).display().to_string())
        .collect();
    assert!(
        found.is_empty(),
        "HD pack files in the repo or site/: {found:?}"
    );
}

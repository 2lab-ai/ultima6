//! Spec 07 Rights / Acceptance 4: no HD pack, part of one, or output of the HD tool is
//! committed, and the site serves none. Checked: every blob in the git index (what a commit
//! takes, read with `git cat-file`, whatever the working tree holds), and every file under
//! `site/` on disk (built output included). Any error reading them fails the test.
//!
//! A file is caught by its shape, not by a string (the format string itself is in source: this
//! crate, the page's drop classifier):
//! - a `manifest.json` that parses as JSON with `format` equal to the pack's;
//! - an `index.bin` that starts with the index magic;
//! - a file named as a pack entry: `atlas-N.webp`, `portrait/<letter>-<item>.webp`,
//!   `bitmap/paper.webp` or `bitmap/spellbook.webp`;
//! - a zip holding any of these (its entries are checked the same way);
//! - the tool's work files: `jobs.jsonl` lines with `sheet` and `places`, an `ingest.json` list
//!   of `slice`s, an `atlas.json` with `generation` and `plan`, the masters
//!   `slices/{t-N,p-L-N,b-ROLE}.png`, the receipts `out/sNNNN-aN.json`.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use u6_hdpack::{FORMAT, Generator, INDEX_MAGIC, write_pack};
use u6_intake::zip;

fn base(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

fn json(bytes: &[u8]) -> Option<serde_json::Value> {
    serde_json::from_slice(bytes).ok()
}

fn number(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Why the file `name` with `bytes` is a pack, a part of one, or a tool output; or `None`.
fn pack_file(name: &str, bytes: &[u8]) -> Option<&'static str> {
    let file = base(name);
    let dir = name.rsplit('/').nth(1).unwrap_or_default();
    if file == "manifest.json" && json(bytes).is_some_and(|v| v["format"].as_str() == Some(FORMAT))
    {
        return Some("a pack manifest");
    }
    if file == "index.bin" && bytes.starts_with(INDEX_MAGIC) {
        return Some("a pack index");
    }
    if let Some(stem) = file.strip_suffix(".webp") {
        let atlas = stem.strip_prefix("atlas-").is_some_and(number);
        let portrait = dir == "portrait"
            && stem.len() >= 3
            && stem.as_bytes()[0].is_ascii_lowercase()
            && stem.as_bytes()[1] == b'-'
            && number(&stem[2..]);
        let bitmap = dir == "bitmap" && (stem == "paper" || stem == "spellbook");
        if atlas || portrait || bitmap {
            return Some("a pack image");
        }
    }
    if file == "jobs.jsonl"
        && std::str::from_utf8(bytes).is_ok_and(|t| {
            t.lines()
                .filter_map(|l| json(l.as_bytes()))
                .any(|v| v.get("sheet").is_some() && v.get("places").is_some())
        })
    {
        return Some("the tool's jobs");
    }
    if file == "ingest.json"
        && json(bytes).is_some_and(|v| {
            v.as_array()
                .is_some_and(|a| a.iter().any(|e| e.get("slice").is_some()))
        })
    {
        return Some("the tool's accepted list");
    }
    if file == "atlas.json"
        && json(bytes).is_some_and(|v| v.get("generation").is_some() && v.get("plan").is_some())
    {
        return Some("the tool's atlas");
    }
    if dir == "slices"
        && let Some(stem) = file.strip_suffix(".png")
    {
        let tile = stem.strip_prefix("t-").is_some_and(number);
        let portrait = stem.strip_prefix("p-").is_some_and(|r| {
            r.len() >= 3 && r.as_bytes()[0].is_ascii_lowercase() && number(&r[2..])
        });
        let bitmap = stem == "b-paper" || stem == "b-spellbook";
        if tile || portrait || bitmap {
            return Some("a tool master");
        }
    }
    if dir == "out"
        && let Some(stem) = file.strip_suffix(".json")
        && let Some(rest) = stem.strip_prefix('s')
        && rest
            .split_once("-a")
            .is_some_and(|(a, b)| number(a) && number(b))
    {
        return Some("a tool receipt");
    }
    if zip::is_zip(bytes) {
        let entries = zip::read_zip(bytes, |_| true).ok()?;
        return entries
            .iter()
            .find_map(|(n, b)| pack_file(n, b))
            .map(|_| "a zip holding a pack or its parts");
    }
    None
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn git(args: &[&str]) -> Vec<u8> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(args)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?} failed");
    out.stdout
}

/// Every blob in the index, with its path.
fn index_blobs() -> Vec<(String, Vec<u8>)> {
    let list = git(&["ls-files", "-s", "-z"]);
    let mut entries: Vec<(String, String)> = Vec::new();
    for rec in list.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let rec = String::from_utf8(rec.to_vec()).expect("an index path is UTF-8");
        let (meta, path) = rec.split_once('\t').expect("mode sha stage<TAB>path");
        let mut f = meta.split(' ');
        let (mode, sha) = (f.next().unwrap(), f.next().unwrap());
        // Gitlinks (submodules) have no blob here.
        if mode != "160000" {
            entries.push((sha.to_string(), path.to_string()));
        }
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("git cat-file runs");
    let mut stdin = child.stdin.take().unwrap();
    let shas: String = entries.iter().map(|(s, _)| format!("{s}\n")).collect();
    let writer = std::thread::spawn(move || stdin.write_all(shas.as_bytes()));
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut blobs = Vec::with_capacity(entries.len());
    for (sha, path) in &entries {
        let mut head = String::new();
        out.read_line(&mut head).expect("cat-file header");
        let mut f = head.split_whitespace();
        assert_eq!(f.next(), Some(sha.as_str()), "cat-file order");
        assert_eq!(f.next(), Some("blob"), "{path}: not a blob");
        let size: usize = f.next().and_then(|s| s.parse().ok()).expect("a size");
        let mut data = vec![0; size + 1];
        out.read_exact(&mut data).expect("the blob");
        data.pop();
        blobs.push((path.clone(), data));
    }
    writer.join().unwrap().expect("cat-file input");
    assert!(child.wait().unwrap().success());
    blobs
}

/// Every file under `dir`, with its path relative to the repository. A symlink to a file is
/// read (what the site would serve); a symlink to a directory is not followed (no loops).
fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for e in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let e = e.unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        let p = e.path();
        let t = e
            .file_type()
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        if t.is_dir() {
            walk(&p, root, out);
        } else if t.is_symlink() && std::fs::metadata(&p).is_ok_and(|m| m.is_dir()) {
            continue;
        } else {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            let data = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            out.push((rel, data));
        }
    }
}

/// A zip of one deflated entry.
fn deflated_zip(name: &[u8], plain: &[u8]) -> Vec<u8> {
    let data = miniz_oxide::deflate::compress_to_vec(plain, 6);
    let crc = crc32fast::hash(plain);
    let head = |out: &mut Vec<u8>| {
        for v in [20u16, 0, 8, 0, 0x21] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in [crc, data.len() as u32, plain.len() as u32] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    };
    let mut out = 0x0403_4b50u32.to_le_bytes().to_vec();
    head(&mut out);
    out.extend_from_slice(name);
    out.extend_from_slice(&data);
    let cd = out.len() as u32;
    out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    head(&mut out);
    out.extend_from_slice(&[0; 14]);
    out.extend_from_slice(name);
    let size = out.len() as u32 - cd;
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, 1, 1] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&cd.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[test]
fn the_detector_finds_packs_and_tool_outputs() {
    let g = Generator {
        name: "test".into(),
        version: "0".into(),
        engine: "none".into(),
        date: "2026-10-16".into(),
    };
    let pack = write_pack(&[], &g, true).unwrap();
    assert!(pack_file("x/anything.bin", &pack).is_some());
    for (name, bytes) in zip::read_zip(&pack, |_| true).unwrap() {
        assert!(
            pack_file(&format!("site/{name}"), &bytes).is_some(),
            "{name}"
        );
    }
    // The manifest as JSON, an escaped slash included, and deflated in a zip, where the
    // format string is not in the file's bytes at all.
    let escaped = br#"{"format": "u6-hdpack\/1", "derived": true}"#;
    assert!(pack_file("site/manifest.json", escaped).is_some());
    let zipped = deflated_zip(b"manifest.json", escaped);
    assert!(!zipped.windows(FORMAT.len()).any(|w| w == FORMAT.as_bytes()));
    assert!(pack_file("data/x.zip", &zipped).is_some());
    // Loose pack images and the tool's work files.
    for name in [
        "site/atlas-3.webp",
        "site/portrait/a-12.webp",
        "x/bitmap/spellbook.webp",
        "w/slices/t-123.png",
        "w/slices/p-z-0.png",
        "w/slices/b-paper.png",
    ] {
        assert!(pack_file(name, b"").is_some(), "{name}");
    }
    assert!(pack_file("w/out/s0012-a2.json", b"{}").is_some());
    let job = br#"{"sheet":"s0000-a1","places":[]}"#;
    assert!(pack_file("w/jobs.jsonl", job).is_some());
    assert!(pack_file("w/ingest.json", br#"[{"slice":"slices/t-1.png"}]"#).is_some());
    assert!(pack_file("w/atlas.json", br#"{"generation":"a","plan":"b"}"#).is_some());
    // Source naming the format, another manifest, other images and names: not caught.
    assert!(pack_file("crates/u6-hdpack/src/lib.rs", FORMAT.as_bytes()).is_none());
    assert!(pack_file("site/app.js", b"if (format === 'u6-hdpack/1') {}").is_none());
    assert!(pack_file("site/manifest.json", br#"{"format": "webmanifest"}"#).is_none());
    assert!(pack_file("site/manifest.json", b"u6-hdpack/1, not json").is_none());
    assert!(pack_file("site/pkg/u6_web_bg.wasm", b"\0asm").is_none());
    assert!(pack_file("site/icons/atlas.webp", b"").is_none());
    assert!(pack_file("tools/oracle/out/run.json", b"{}").is_none());
}

#[test]
fn the_walk_does_not_follow_a_directory_link() {
    let dir = std::env::temp_dir().join(format!("u6-hdpack-walk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("a")).unwrap();
    std::fs::write(dir.join("a/f.txt"), b"x").unwrap();
    // A loop back to the top, and a link to a file.
    std::os::unix::fs::symlink(&dir, dir.join("a/loop")).unwrap();
    std::os::unix::fs::symlink(dir.join("a/f.txt"), dir.join("link.txt")).unwrap();
    let mut files = Vec::new();
    walk(&dir, &dir, &mut files);
    let mut names: Vec<String> = files.into_iter().map(|(n, _)| n).collect();
    names.sort();
    assert_eq!(names, ["a/f.txt", "link.txt"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_pack_is_in_the_index_or_under_site() {
    let root = repo_root().canonicalize().unwrap();
    let mut files = index_blobs();
    assert!(files.len() > 100, "the index lists the repository");
    assert!(files.iter().any(|(p, _)| p == "Cargo.toml"));
    walk(&root.join("site"), &root, &mut files);
    let found: Vec<String> = files
        .iter()
        .filter_map(|(p, b)| pack_file(p, b).map(|why| format!("{p}: {why}")))
        .collect();
    assert!(
        found.is_empty(),
        "HD pack files in the repo or site/: {found:?}"
    );
}

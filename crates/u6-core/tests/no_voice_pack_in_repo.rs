//! Spec 09 Rights / Acceptance 5: no voice pack, and none of the tool's other outputs (the
//! personality notes, the coverage receipt), is committed or sits under `site/`. They are read
//! from the owner's game (tools/speech/profile.mjs makes them from the NPC bundles): they live in
//! `~/Games/ultima6/voice/` and the owner's browser storage only. Checked: every blob in the git index (what a commit takes, read with `git cat-file`,
//! whatever the working tree holds), and every file under `site/` on disk (built output and the
//! gitignored model parts included). Any error reading them fails the test.
//!
//! A file is caught by its shape, not by a string (the formats' names are in source: the tool,
//! the page): JSON whose root object, or an element of its root array, has `format` equal to one
//! of the three, after any JSON whitespace. The walker is the NPC bundles' guard's
//! (`no_npc_bundle_in_repo.rs`).
// The test reads the repository and runs git: the crate's platform-free rule is for its code.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The voice pack's format (site/voice/profile.js `PACK_FORMAT`), the notes' and the coverage
/// receipt's (tools/speech/profile.mjs).
const FORMATS: [&str; 3] = ["u6.voice-pack/1", "u6.voice-notes/1", "u6.voice-coverage/1"];

/// JSON's whitespace (RFC 8259: space, tab, line feed, carriage return).
fn json_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// Whether `bytes` is one of the tool's outputs, or an array holding one.
fn is_voice_file(bytes: &[u8]) -> bool {
    let start = bytes.iter().position(|&b| !json_space(b));
    if !start.is_some_and(|i| matches!(bytes[i], b'{' | b'[')) {
        return false;
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return false;
    };
    let one = |v: &serde_json::Value| v["format"].as_str().is_some_and(|f| FORMATS.contains(&f));
    one(&v) || v.as_array().is_some_and(|a| a.iter().any(one))
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

/// Whether the file at `p` starts as JSON: its first byte that is not JSON whitespace is `{` or
/// `[`. The whitespace before it is read through however long it is (a chunk at a time, none of
/// it kept); a file of whitespace only does not.
fn starts_as_json(p: &Path) -> bool {
    let mut f = std::fs::File::open(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut chunk = [0u8; 4096];
    loop {
        let n = f
            .read(&mut chunk)
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        if n == 0 {
            return false;
        }
        if let Some(&b) = chunk[..n].iter().find(|&&b| !json_space(b)) {
            return matches!(b, b'{' | b'[');
        }
    }
}

/// The paths under `dir` (relative to `root`) whose file is one of the tool's outputs. A file is read
/// whole only when it starts as JSON ([`starts_as_json`]); a symlink to a directory is not
/// followed (no loops) but listed in `links`.
fn walk(
    dir: &Path,
    root: &Path,
    found: &mut Vec<String>,
    links: &mut Vec<String>,
    files: &mut usize,
) {
    for e in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let e = e.unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        let p = e.path();
        let t = e
            .file_type()
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        if t.is_dir() {
            walk(&p, root, found, links, files);
            continue;
        }
        if t.is_symlink() && std::fs::metadata(&p).is_ok_and(|m| m.is_dir()) {
            // Not followed (no loops), so nothing under it is checked: the guard fails on it.
            links.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            continue;
        }
        *files += 1;
        if !starts_as_json(&p) {
            continue;
        }
        let data = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        if is_voice_file(&data) {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            found.push(rel);
        }
    }
}

/// A pack as the tool writes it (synthetic numbers).
const PACK: &str = r#"{"format":"u6.voice-pack/1","model":"m","revision":"00000000","profiles":{"1":{"mix":[{"style":"M1","w":1}],"speed":1,"pitch":0,"steps":16,"seedSalt":0}}}"#;

/// The shape check catches a pack, notes, a coverage receipt and arrays of them, whitespace first
/// or not, and nothing else of that look.
#[test]
fn the_shape_check_catches_the_tools_outputs_only() {
    let notes = r#"{"format":"u6.voice-notes/1","model":"m","notes":[]}"#;
    let coverage = r#"{"format":"u6.voice-coverage/1","model":"m","failures":[]}"#;
    assert!(
        is_voice_file(PACK.as_bytes())
            && is_voice_file(notes.as_bytes())
            && is_voice_file(coverage.as_bytes())
    );
    assert!(is_voice_file(format!("[{PACK}]").as_bytes()));
    assert!(is_voice_file(format!("\n \t\r{PACK}").as_bytes()));
    let other = PACK.replace("u6.voice-pack/1", "u6.voice-pack/2");
    assert!(!is_voice_file(other.as_bytes()));
    assert!(
        !is_voice_file(b"{\"format\":1}")
            && !is_voice_file(b"[1,2]")
            && !is_voice_file(b"not json")
    );
}

/// The walker over a directory of its own: a pack after far more JSON whitespace than one read
/// takes is found, as is one after a newline; JSON of another kind after the same whitespace, a
/// file of whitespace only, and a pack after a byte JSON does not take as whitespace (it does not
/// parse) are not.
#[test]
fn the_walker_finds_a_voice_pack_after_any_amount_of_whitespace() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-voice-pack-walk");
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("the old test directory goes");
    }
    let dir = root.join("site/deep");
    std::fs::create_dir_all(&dir).expect("the test directory");
    let pad = " \t\r\n".repeat(64 * 1024);
    let write = |name: &str, text: String| std::fs::write(dir.join(name), text).expect("written");
    write("padded.u6vp", format!("{pad}{PACK}"));
    write("short.json", format!("\n{PACK}"));
    write(
        "other.json",
        format!("{pad}{{\"format\":\"u6-converse-catalog/3\"}}"),
    );
    write("blank.txt", pad.clone());
    write("form-feed.u6vp", format!("\u{c}{PACK}"));
    let (mut found, mut links, mut files) = (Vec::new(), Vec::new(), 0);
    walk(
        &root.join("site"),
        &root,
        &mut found,
        &mut links,
        &mut files,
    );
    assert!(links.is_empty());
    found.sort();
    assert_eq!(files, 5);
    assert_eq!(found, ["site/deep/padded.u6vp", "site/deep/short.json"]);
    std::fs::remove_dir_all(&root).expect("the test directory goes");
}

/// The files under `site` (paths relative to `root`), or why the guard fails there: a symlink to
/// a directory (the walker does not follow it, so what it holds is not checked), named by its
/// repository path only, or the tool outputs found.
fn site_check(site: &Path, root: &Path) -> Result<usize, String> {
    let (mut found, mut links, mut files) = (Vec::new(), Vec::new(), 0);
    walk(site, root, &mut found, &mut links, &mut files);
    if !links.is_empty() {
        return Err(format!(
            "directory links under site/, not followed so not checked: {links:?}"
        ));
    }
    if !found.is_empty() {
        return Err(format!("voice tool outputs under site/: {found:?}"));
    }
    Ok(files)
}

/// A symlink to a directory under `site/` fails the guard, named by its repository path only
/// (not what it points at); without it the same tree passes.
#[test]
fn a_directory_link_under_site_fails_the_guard() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-voice-pack-link");
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("the old test directory goes");
    }
    let (site, outside) = (root.join("site"), root.join("outside"));
    std::fs::create_dir_all(site.join("deep")).expect("the test directory");
    std::fs::create_dir_all(&outside).expect("the target directory");
    std::fs::write(site.join("deep/page.txt"), "a page").expect("written");
    std::fs::write(outside.join("voices.u6vp"), PACK).expect("written");
    assert_eq!(site_check(&site, &root), Ok(1), "no link: the tree passes");
    std::os::unix::fs::symlink(&outside, site.join("deep/out")).expect("the link");
    let err = site_check(&site, &root).unwrap_err();
    assert_eq!(
        err,
        "directory links under site/, not followed so not checked: [\"site/deep/out\"]"
    );
    assert!(!err.contains("outside"), "the target is not named");
    std::fs::remove_dir_all(&root).expect("the test directory goes");
}

#[test]
fn no_voice_pack_is_in_git_or_under_site() {
    let root = repo_root();
    let blobs = index_blobs();
    assert!(!blobs.is_empty(), "the index lists files");
    let tracked: Vec<&str> = blobs
        .iter()
        .filter(|(_, b)| is_voice_file(b))
        .map(|(p, _)| p.as_str())
        .collect();
    assert!(
        tracked.is_empty(),
        "voice tool outputs in the index: {tracked:?}"
    );
    let site = root.join("site");
    let files = site_check(&site, &root).unwrap_or_else(|e| panic!("{e}"));
    assert!(files > 0, "site/ holds files");
    println!(
        "index blobs {}, files under site/ {files}: no voice pack, notes or coverage: pass",
        blobs.len()
    );
}

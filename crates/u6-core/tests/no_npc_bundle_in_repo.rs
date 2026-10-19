//! Spec 09 Rights / Acceptance 5: no NPC bundle is committed or sits under `site/`. The bundles
//! hold the user's game text: they live only in the user's browser storage and the file the
//! user downloads. Checked: every blob in the git index (what a commit takes, read with `git
//! cat-file`, whatever the working tree holds), and every file under `site/` on disk (built
//! output and the gitignored model parts included). Any error reading them fails the test.
//!
//! A file is caught by its shape, not by a string (the format's name is in source: the
//! exporter, the page): JSON whose root object, or an element of its root array, has `format`
//! equal to the bundle format.
// The test reads the repository and runs git: the crate's platform-free rule is for its code.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use u6_core::converse::disasm::bundle::{self, Bundle};

/// JSON's whitespace (RFC 8259: space, tab, line feed, carriage return).
fn json_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// Whether `bytes` is a bundle or an array holding one.
fn is_bundle(bytes: &[u8]) -> bool {
    let start = bytes.iter().position(|&b| !json_space(b));
    if !start.is_some_and(|i| matches!(bytes[i], b'{' | b'[')) {
        return false;
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return false;
    };
    let one = |v: &serde_json::Value| v["format"].as_str() == Some(bundle::FORMAT);
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

/// The paths under `dir` (relative to `root`) whose file is a bundle. A file is read whole only
/// when it starts as JSON ([`starts_as_json`]); a symlink to a directory is not followed (no
/// loops).
fn walk(dir: &Path, root: &Path, found: &mut Vec<String>, files: &mut usize) {
    for e in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let e = e.unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        let p = e.path();
        let t = e
            .file_type()
            .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        if t.is_dir() {
            walk(&p, root, found, files);
            continue;
        }
        if t.is_symlink() && std::fs::metadata(&p).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        *files += 1;
        if !starts_as_json(&p) {
            continue;
        }
        let data = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        if is_bundle(&data) {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            found.push(rel);
        }
    }
}

/// The shape check catches a bundle and an array of them, whitespace first or not, and
/// nothing else of that look.
#[test]
fn the_shape_check_catches_bundles_only() {
    let script = b"\xff\x05Nm\xf1look\xf2Hi\xf7\xef*\xf6Ok\xee\xff";
    let b = Bundle::build(b'a', 1, script, &[]);
    let array = bundle::to_json(std::slice::from_ref(&b));
    let one = serde_json::to_string(&b).unwrap();
    assert!(is_bundle(array.as_bytes()) && is_bundle(one.as_bytes()));
    assert!(is_bundle(format!("\n  {array}").as_bytes()));
    let other = one.replace(bundle::FORMAT, "u6-converse-catalog/3");
    assert!(!is_bundle(other.as_bytes()));
    assert!(!is_bundle(b"{\"format\":1}") && !is_bundle(b"[1,2]") && !is_bundle(b"not json"));
}

/// The walker over a directory of its own: a bundle after far more JSON whitespace than one
/// read takes is found, as is one after a newline; JSON of another kind after the same
/// whitespace, a file of whitespace only, and a bundle after a byte JSON does not take as
/// whitespace (it does not parse) are not.
#[test]
fn the_walker_finds_a_bundle_after_any_amount_of_whitespace() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-npc-bundle-walk");
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("the old test directory goes");
    }
    let dir = root.join("site/deep");
    std::fs::create_dir_all(&dir).expect("the test directory");
    let script = b"\xff\x05Nm\xf1look\xf2Hi\xf7\xef*\xf6Ok\xee\xff";
    let array = bundle::to_json(&[Bundle::build(b'a', 1, script, &[])]);
    let pad = " \t\r\n".repeat(64 * 1024);
    let write = |name: &str, text: String| std::fs::write(dir.join(name), text).expect("written");
    write("padded.json", format!("{pad}{array}"));
    write("short.json", format!("\n{array}"));
    write(
        "other.json",
        format!("{pad}{{\"format\":\"u6-converse-catalog/3\"}}"),
    );
    write("blank.txt", pad.clone());
    write("form-feed.json", format!("\u{c}{array}"));
    let (mut found, mut files) = (Vec::new(), 0);
    walk(&root.join("site"), &root, &mut found, &mut files);
    found.sort();
    assert_eq!(files, 5);
    assert_eq!(found, ["site/deep/padded.json", "site/deep/short.json"]);
    std::fs::remove_dir_all(&root).expect("the test directory goes");
}

#[test]
fn no_npc_bundle_is_in_git_or_under_site() {
    let root = repo_root();
    let blobs = index_blobs();
    assert!(!blobs.is_empty(), "the index lists files");
    let tracked: Vec<&str> = blobs
        .iter()
        .filter(|(_, b)| is_bundle(b))
        .map(|(p, _)| p.as_str())
        .collect();
    assert!(tracked.is_empty(), "NPC bundles in the index: {tracked:?}");
    let site = root.join("site");
    let (mut found, mut files) = (Vec::new(), 0);
    walk(&site, &root, &mut found, &mut files);
    assert!(files > 0, "site/ holds files");
    assert!(found.is_empty(), "NPC bundles under site/: {found:?}");
    println!(
        "index blobs {}, files under site/ {files}: no NPC bundle: pass",
        blobs.len()
    );
}

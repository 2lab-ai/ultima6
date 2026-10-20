//! Spec 08 Rights / Acceptance 8: no Korean pack (`ko.u6lp`), and no coverage receipt of the
//! translation tool, is committed or sits under `site/`. They are made from the owner's game
//! (tools/i18n/translate.mjs makes them from the NPC bundles): they live in
//! `~/Games/ultima6/i18n/ko/` and the owner's browser storage, and the pack also behind his key
//! (below). Checked: every blob in the
//! git index (what a commit takes, read with `git cat-file`, whatever the working tree holds),
//! and every file under `site/` on disk (built output and the gitignored model parts included).
//! Any error reading them fails the test.
//!
//! A file is caught by its shape, not by a name: a pack is any file whose first four bytes are
//! the pack magic `U6LP` (u6_core::locale::MAGIC); a coverage receipt is JSON whose root object,
//! or an element of its root array, has `format` equal to the receipt's, after any JSON
//! whitespace. The walker is the voice pack's guard's (`no_voice_pack_in_repo.rs`), with the
//! magic read first.
//!
//! A file that starts as JSON but does not parse (malformed, or nested past the parser's depth)
//! is caught when a string token of it is a format's name, its escapes decoded (`tests/guard`),
//! since the parse cannot rule out what it holds; and `site/` itself a symbolic link fails the
//! guard (the walker would follow it).
//!
//! The one exception (spec 08 K6, the owner's rule of 2026-10-20: the pack is hosted behind his
//! key, as the HD packs are): a pack on disk under `site/packs/`, which is gitignored and served
//! only through the key gate of `functions/_middleware.js` (u6-hdpack's guard checks, while
//! `site/packs/` exists, that git ignores it and the gate still guards `/packs`). A pack in the
//! index fails wherever it is, and a coverage receipt fails under `site/packs/` too.
// The test reads the repository and runs git: the crate's platform-free rule is for its code.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

mod guard;

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use u6_core::locale::MAGIC;

/// The coverage receipt's format (tools/i18n/translate.mjs `COVERAGE_FORMAT`).
const FORMATS: [&str; 1] = ["u6.locale-coverage/1"];

/// JSON's whitespace (RFC 8259: space, tab, line feed, carriage return).
fn json_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// Whether `bytes` is a pack (the magic first) or a coverage receipt, or an array holding one.
fn is_locale_file(bytes: &[u8]) -> bool {
    if bytes.starts_with(MAGIC) {
        return true;
    }
    let start = bytes.iter().position(|&b| !json_space(b));
    if !start.is_some_and(|i| matches!(bytes[i], b'{' | b'[')) {
        return false;
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        // JSON that does not parse (malformed, or nested past the parser's depth) may still hold
        // one: a file with a string token equal to a format, its escapes decoded, fails the guard
        // too (`guard::holds_string`).
        return guard::holds_string(bytes, &FORMATS);
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

/// Whether the file at `p` starts with the pack magic (its first four bytes only are read).
fn starts_with_magic(p: &Path) -> bool {
    let mut f = std::fs::File::open(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut head = Vec::with_capacity(MAGIC.len());
    (&mut f)
        .take(MAGIC.len() as u64)
        .read_to_end(&mut head)
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    head == MAGIC
}

/// Where a pack may lie on disk: the gitignored directory the key gate serves.
const HOSTED: &str = "site/packs/";

/// Whether a file found on disk at `rel` (relative to the repository) that starts with the magic
/// is a hosted pack: one under [`HOSTED`]. A file of the index never is.
fn hosted(rel: &str) -> bool {
    rel.starts_with(HOSTED)
}

/// The paths under `dir` (relative to `root`) whose file is one of the tool's outputs: one that
/// starts with the magic (but a hosted pack), or one read whole because it starts as JSON
/// ([`starts_as_json`]) that is a receipt; a symlink to a directory is not followed (no loops)
/// but listed in `links`.
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
        if starts_with_magic(&p) {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            if !hosted(&rel) {
                found.push(rel);
            }
            continue;
        }
        if !starts_as_json(&p) {
            continue;
        }
        let data = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        if is_locale_file(&data) {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            found.push(rel);
        }
    }
}

/// A pack as the tool writes it (synthetic: no game text).
const PACK: &[u8] = b"U6LP\n{\"format\":\"u6.locale-pack/1\",\"language\":\"ko\",\"tool\":\"t/1\",\"base\":{\"converse\":\"0000000000000000\"},\"converse\":[]}";
/// A coverage receipt as the tool writes it (synthetic numbers).
const COVERAGE: &str = r#"{"format":"u6.locale-coverage/1","model":"m","scripts":{"chosen":1}}"#;

/// The shape check catches a pack by its magic (whatever follows), a receipt and an array of
/// receipts, whitespace first or not, and nothing else of that look.
#[test]
fn the_shape_check_catches_the_tools_outputs_only() {
    assert!(is_locale_file(PACK) && is_locale_file(b"U6LP") && is_locale_file(b"U6LP\x00\xff"));
    assert!(is_locale_file(COVERAGE.as_bytes()));
    assert!(is_locale_file(format!("[{COVERAGE}]").as_bytes()));
    assert!(is_locale_file(format!("\n \t\r{COVERAGE}").as_bytes()));
    assert!(!is_locale_file(b" U6LP") && !is_locale_file(b"U6L") && !is_locale_file(b"u6lp"));
    let other = COVERAGE.replace("u6.locale-coverage/1", "u6.locale-coverage/2");
    assert!(!is_locale_file(other.as_bytes()));
    assert!(
        !is_locale_file(b"{\"format\":1}")
            && !is_locale_file(b"[1,2]")
            && !is_locale_file(b"not json")
    );
}

/// The walker over a directory of its own: a pack (any name) and a receipt after far more JSON
/// whitespace than one read takes are found; a file that only mentions the magic later, JSON of
/// another kind, a file of whitespace only and an empty file are not.
#[test]
fn the_walker_finds_a_pack_by_its_magic_and_a_receipt_after_any_whitespace() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-locale-pack-walk");
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("the old test directory goes");
    }
    let dir = root.join("site/deep");
    std::fs::create_dir_all(&dir).expect("the test directory");
    let pad = " \t\r\n".repeat(64 * 1024);
    let write = |name: &str, bytes: &[u8]| std::fs::write(dir.join(name), bytes).expect("written");
    write("ko.u6lp", PACK);
    write("renamed.bin", PACK);
    write("padded.json", format!("{pad}{COVERAGE}").as_bytes());
    write("later.txt", b"see U6LP here");
    write(
        "other.json",
        format!("{pad}{{\"format\":\"u6-converse-catalog/3\"}}").as_bytes(),
    );
    write("blank.txt", pad.as_bytes());
    write("empty", b"");
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
    assert_eq!(files, 7);
    assert_eq!(
        found,
        [
            "site/deep/ko.u6lp",
            "site/deep/padded.json",
            "site/deep/renamed.bin"
        ]
    );
    std::fs::remove_dir_all(&root).expect("the test directory goes");
}

/// A pack under `site/packs/` on disk is hosted, not found, at any depth; one beside it, in a
/// sibling or a nested `packs` directory is found, and so is a receipt under `site/packs/`.
#[test]
fn only_a_pack_on_disk_under_site_packs_is_hosted() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-locale-pack-hosted");
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("the old test directory goes");
    }
    let write = |rel: &str, bytes: &[u8]| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).expect("the test directory");
        std::fs::write(p, bytes).expect("written");
    };
    write("site/packs/ko-1.u6lp", PACK);
    write("site/packs/sub/ko-2.u6lp", PACK);
    write("site/packs/coverage.json", COVERAGE.as_bytes());
    write("site/packs-x/ko.u6lp", PACK);
    write("site/x/packs/ko.u6lp", PACK);
    write("site/ko.u6lp", PACK);
    let (mut found, mut links, mut files) = (Vec::new(), Vec::new(), 0);
    walk(
        &root.join("site"),
        &root,
        &mut found,
        &mut links,
        &mut files,
    );
    found.sort();
    assert_eq!(files, 6);
    assert_eq!(
        found,
        [
            "site/ko.u6lp",
            "site/packs-x/ko.u6lp",
            "site/packs/coverage.json",
            "site/x/packs/ko.u6lp"
        ]
    );
    std::fs::remove_dir_all(&root).expect("the test directory goes");
}

/// The files under `site` (paths relative to `root`), or why the guard fails there: `site`
/// itself a symbolic link, or a symlink to a directory under it (the walker does not follow it,
/// so what it holds is not checked), named by its repository path only, or the tool outputs found.
fn site_check(site: &Path, root: &Path) -> Result<usize, String> {
    // `site/` itself a link: the walker would follow it (read_dir does), so nothing is checked.
    let meta =
        std::fs::symlink_metadata(site).unwrap_or_else(|e| panic!("{}: {e}", site.display()));
    if meta.file_type().is_symlink() {
        let rel = site
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        return Err(format!(
            "site/ is a symbolic link, not followed so not checked: {rel:?}"
        ));
    }
    let (mut found, mut links, mut files) = (Vec::new(), Vec::new(), 0);
    walk(site, root, &mut found, &mut links, &mut files);
    if !links.is_empty() {
        return Err(format!(
            "directory links under site/, not followed so not checked: {links:?}"
        ));
    }
    if !found.is_empty() {
        return Err(format!(
            "Korean packs or coverage receipts under site/: {found:?}"
        ));
    }
    Ok(files)
}

/// A symlink to a directory under `site/` fails the guard, named by its repository path only
/// (not what it points at); without it the same tree passes.
#[test]
fn a_directory_link_under_site_fails_the_guard() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-locale-pack-link");
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("the old test directory goes");
    }
    let (site, outside) = (root.join("site"), root.join("outside"));
    std::fs::create_dir_all(site.join("deep")).expect("the test directory");
    std::fs::create_dir_all(&outside).expect("the target directory");
    std::fs::write(site.join("deep/page.txt"), "a page").expect("written");
    std::fs::write(outside.join("ko.u6lp"), PACK).expect("written");
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

/// `site/` itself a symbolic link to a directory fails the guard, named by its repository path
/// only (not what it points at), though what it points at holds a file the guard catches.
#[test]
fn a_site_that_is_a_symbolic_link_fails_the_guard() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-locale-pack-site-link");
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("the old test directory goes");
    }
    let outside = root.join("outside");
    std::fs::create_dir_all(&outside).expect("the target directory");
    let sample = COVERAGE.to_string();
    std::fs::write(outside.join("ko.u6lp"), &sample).expect("written");
    std::fs::write(outside.join("page.txt"), "a page").expect("written");
    std::os::unix::fs::symlink(&outside, root.join("site")).expect("the link");
    let err = site_check(&root.join("site"), &root).unwrap_err();
    assert_eq!(
        err,
        "site/ is a symbolic link, not followed so not checked: \"site\""
    );
    assert!(!err.contains("outside"), "the target is not named");
    std::fs::remove_dir_all(&root).expect("the test directory goes");
}

/// A file that starts as JSON and names the format but does not parse (here nested past the
/// parser's depth) is caught by the shape check and under `site/`; one of that depth that names
/// no format is not.
#[test]
fn a_file_too_deep_to_parse_that_names_the_format_is_caught() {
    let sample = COVERAGE.to_string();
    let deep = format!("{}{sample}{}", "[".repeat(200), "]".repeat(200));
    assert!(
        serde_json::from_str::<serde_json::Value>(&deep).is_err(),
        "past the parser's depth"
    );
    assert!(is_locale_file(deep.as_bytes()));
    let plain = format!("{}1{}", "[".repeat(200), "]".repeat(200));
    assert!(!is_locale_file(plain.as_bytes()));
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-locale-pack-deep");
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("the old test directory goes");
    }
    std::fs::create_dir_all(root.join("site")).expect("the test directory");
    std::fs::write(root.join("site/deep.json"), &deep).expect("written");
    std::fs::write(root.join("site/plain.json"), &plain).expect("written");
    let err = site_check(&root.join("site"), &root).unwrap_err();
    assert!(
        err.ends_with("[\"site/deep.json\"]"),
        "only the file that names it"
    );
    std::fs::remove_dir_all(&root).expect("the test directory goes");
}

/// The reviewer's case: a valid output whose format name is spelled with a JSON escape (`\/`, or
/// `\u` for its first character), first in a root array whose second element is nested past the
/// parser's depth. The parse fails and the name is not in the file's bytes, yet the guard
/// catches it, by its shape check and under `site/`, from the string token decoded; a token that
/// only holds the name inside a longer one is not it.
#[test]
fn an_unparsed_file_that_escapes_the_format_name_is_caught() {
    let sample = COVERAGE.to_string();
    let format = FORMATS[0];
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-locale-pack-escaped");
    let slash = format.replace('/', "\\/");
    let unicode = format!("\\u{:04x}{}", format.as_bytes()[0], &format[1..]);
    for spelled in [slash, unicode] {
        let escaped = sample.replacen(format, &spelled, 1);
        let v: serde_json::Value = serde_json::from_str(&escaped).expect("the output parses");
        assert_eq!(v["format"].as_str(), Some(format), "its format, decoded");
        let file = format!("[{escaped},{deep}]");
        assert!(
            serde_json::from_str::<serde_json::Value>(&file).is_err(),
            "past the parser's depth"
        );
        assert!(
            !file.contains(format),
            "the name is not in the file's bytes"
        );
        assert!(
            is_locale_file(file.as_bytes()),
            "the shape check catches it"
        );
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("the old test directory goes");
        }
        std::fs::create_dir_all(root.join("site")).expect("the test directory");
        std::fs::write(root.join("site/escaped.json"), &file).expect("written");
        let err = site_check(&root.join("site"), &root).unwrap_err();
        assert!(err.ends_with("[\"site/escaped.json\"]"), "and under site/");
    }
    let longer = format!("[\"{format}x\",\"x{format}\",{deep}]");
    assert!(
        !is_locale_file(longer.as_bytes()),
        "a longer token is not the name"
    );
    std::fs::remove_dir_all(&root).expect("the test directory goes");
}

#[test]
fn no_locale_pack_is_in_git_or_under_site() {
    let root = repo_root();
    let blobs = index_blobs();
    assert!(!blobs.is_empty(), "the index lists files");
    let tracked: Vec<&str> = blobs
        .iter()
        .filter(|(_, b)| is_locale_file(b))
        .map(|(p, _)| p.as_str())
        .collect();
    assert!(
        tracked.is_empty(),
        "Korean packs or coverage receipts in the index: {tracked:?}"
    );
    let site = root.join("site");
    let files = site_check(&site, &root).unwrap_or_else(|e| panic!("{e}"));
    assert!(files > 0, "site/ holds files");
    println!(
        "index blobs {}, files under site/ {files}: no Korean pack or coverage receipt: pass",
        blobs.len()
    );
}

//! The `u6-export` binary on synthetic files: what it prints is one line naming the file and its
//! count (never a byte of the export), an error names only what failed, and the exit codes.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use u6_export::synthetic;

const BIN: &str = env!("CARGO_BIN_EXE_u6-export");
const SENTINEL: &str = "Zq5CliSentinel";

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("u6-export-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn export(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn text(b: &[u8]) -> &str {
    std::str::from_utf8(b).unwrap()
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn it_prints_one_line_of_counts_and_never_the_text() {
    let dir = temp("ok");
    std::fs::create_dir_all(dir.join("game")).unwrap();
    std::fs::write(dir.join("game/game"), synthetic::game(SENTINEL)).unwrap();
    std::fs::write(
        dir.join("game/converse.a"),
        synthetic::converse(&synthetic::script(SENTINEL)),
    )
    .unwrap();
    for (cmd, counts) in [
        ("bundles", "(1 bundles)"),
        (
            "strings",
            "game strings, 0 object names and books, 0 strings of u)",
        ),
    ] {
        let out = dir.join(format!("work/{cmd}.json"));
        let o = export(&[cmd, "--game", path(&dir.join("game")), "--out", path(&out)]);
        assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
        let stdout = text(&o.stdout);
        assert_eq!(stdout.lines().count(), 1, "{stdout}");
        assert!(
            stdout.starts_with(&format!("wrote {} (", out.display())),
            "{stdout}"
        );
        assert!(stdout.trim_end().ends_with(counts), "{stdout}");
        assert!(o.stderr.is_empty());
        assert!(std::fs::read_to_string(&out).unwrap().contains(SENTINEL));
        assert!(!stdout.contains(SENTINEL));
    }
}

#[test]
fn a_failure_names_only_what_failed_and_a_usage_error_exits_2() {
    let dir = temp("fail");
    std::fs::create_dir_all(dir.join("game")).unwrap();
    std::fs::write(dir.join("game").join(SENTINEL), SENTINEL).unwrap();
    let out = dir.join("x.json");
    let o = export(&[
        "bundles",
        "--game",
        path(&dir.join("game")),
        "--out",
        path(&out),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        text(&o.stderr),
        "u6-export: NPC bundles: the files hold no conversation archive\n"
    );
    assert!(o.stdout.is_empty());
    assert!(!out.exists());
    let o = export(&["bundles", "--game", path(&dir.join("game"))]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        text(&o.stderr).starts_with("u6-export: give --out FILE\nusage:"),
        "{}",
        text(&o.stderr)
    );
}

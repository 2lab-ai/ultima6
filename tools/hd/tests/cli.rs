//! The `u6-hd` binary end to end, on the synthetic set only, with a recording stand-in for
//! `codex` (a shell script that logs each call and answers with the sheet it was given).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_u6-hd");

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("u6-hd-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// A `codex` stand-in: appends a line to `$STUB_LOG` per call, drains its prompt, copies the
/// `-i` sheet into `$CODEX_HOME/generated_images/<id>/` and prints the thread event; with
/// `STUB_FAIL` set it exits 1 instead.
fn stub(dir: &Path) -> PathBuf {
    let p = dir.join("codex");
    std::fs::write(
        &p,
        r#"#!/bin/sh
echo call >> "$STUB_LOG"
img=""
while [ $# -gt 0 ]; do
  if [ "$1" = "-i" ]; then img="$2"; shift; fi
  shift
done
cat > /dev/null
if [ -n "$STUB_FAIL" ]; then exit 1; fi
id="stub-$$"
mkdir -p "$CODEX_HOME/generated_images/$id"
cp "$img" "$CODEX_HOME/generated_images/$id/exec-1.png"
echo "{\"type\":\"thread.started\",\"thread_id\":\"$id\"}"
"#,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

struct Env {
    dir: PathBuf,
    home: PathBuf,
    log: PathBuf,
}

impl Env {
    fn new(name: &str) -> Env {
        let dir = temp(name);
        let home = dir.join("home");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        Env {
            log: dir.join("calls.log"),
            dir,
            home,
        }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(BIN);
        c.args(args)
            .env("HOME", &self.home)
            .env("CODEX_HOME", self.home.join(".codex"))
            .env("STUB_LOG", &self.log)
            .env_remove("STUB_FAIL")
            .current_dir(&self.dir);
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().unwrap()
    }

    fn calls(&self) -> usize {
        std::fs::read_to_string(&self.log).map_or(0, |t| t.lines().count())
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn run_sends_nothing_without_send() {
    let e = Env::new("send");
    let work = e.dir.join("work");
    let w = work.to_str().unwrap();
    let codex = stub(&e.dir);
    let c = codex.to_str().unwrap();
    let o = e.run(&["atlas", "--synthetic", "--work", w]);
    assert!(o.status.success(), "{}", text(&o));
    let jobs = std::fs::read_to_string(work.join("jobs.jsonl"))
        .unwrap()
        .lines()
        .count();
    // Without --send: the notice, no call, twice (a resume sends nothing either).
    for _ in 0..2 {
        let o = e.run(&["run", "--work", w, "--codex", c]);
        assert!(o.status.success(), "{}", text(&o));
        assert!(text(&o).contains("Nothing was sent"));
        assert!(text(&o).contains(&format!("{jobs} sheet(s) to generate")));
        assert_eq!(e.calls(), 0);
    }
    // With it: one as a trial, then the rest, then nothing more.
    let o = e.run(&["run", "--work", w, "--codex", c, "--send", "--limit", "1"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(e.calls(), 1);
    let o = e.run(&["run", "--work", w, "--codex", c, "--send"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(e.calls(), jobs);
    // Each image was moved out of codex's folder, its thread folder with it.
    let left = std::fs::read_dir(e.home.join(".codex/generated_images"))
        .unwrap()
        .count();
    assert_eq!(left, 0);
    let runs = std::fs::read_to_string(work.join("runs.jsonl")).unwrap();
    assert_eq!(runs.lines().count(), jobs);
    let o = e.run(&["run", "--work", w, "--codex", c, "--send"]);
    assert!(text(&o).contains("0 sheet(s) to generate"));
    assert_eq!(e.calls(), jobs);
    // The outputs ingest and pack.
    let o = e.run(&["ingest", "--synthetic", "--work", w]);
    assert!(o.status.success(), "{}", text(&o));
    let o = e.run(&["pack", "--work", w]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(work.join("pack/u6-hdpack.zip").exists());
    // A rebuilt atlas: every sheet is to be sent again.
    let o = e.run(&["atlas", "--synthetic", "--work", w]);
    assert!(!o.status.success());
    let o = e.run(&["atlas", "--synthetic", "--work", w, "--rebuild"]);
    assert!(o.status.success(), "{}", text(&o));
    let o = e.run(&["run", "--work", w, "--codex", c]);
    assert!(text(&o).contains(&format!("{jobs} sheet(s) to generate")));
    assert_eq!(e.calls(), jobs);
    let _ = std::fs::remove_dir_all(&e.dir);
}

#[test]
fn a_failed_call_exits_non_zero() {
    let e = Env::new("fail");
    let work = e.dir.join("work");
    let w = work.to_str().unwrap();
    let codex = stub(&e.dir);
    assert!(
        e.run(&["atlas", "--synthetic", "--work", w])
            .status
            .success()
    );
    let o = e
        .cmd(&[
            "run",
            "--work",
            w,
            "--codex",
            codex.to_str().unwrap(),
            "--send",
            "--limit",
            "2",
        ])
        .env("STUB_FAIL", "1")
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert_eq!(e.calls(), 2);
    assert!(text(&o).contains("2 call(s) failed"), "{}", text(&o));
    let _ = std::fs::remove_dir_all(&e.dir);
}

#[test]
fn the_default_and_relative_work_dirs_are_checked() {
    let e = Env::new("contain");
    // HOME whose default work dir resolves into the repository.
    std::fs::create_dir_all(e.home.join("Games")).unwrap();
    let inside = repo().join("site");
    std::os::unix::fs::symlink(&inside, e.home.join("Games/ultima6")).unwrap();
    let o = e.run(&["atlas", "--synthetic"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("inside"), "{}", text(&o));
    assert!(!inside.join("hd").exists());
    // A relative --work from inside the repository.
    let o = e
        .cmd(&["atlas", "--synthetic", "--work", "u6-hd-relative-work"])
        .current_dir(repo())
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(!repo().join("u6-hd-relative-work").exists());
    // A pack destination in the repository.
    let work = e.dir.join("work");
    let w = work.to_str().unwrap();
    assert!(
        e.run(&["atlas", "--synthetic", "--work", w])
            .status
            .success()
    );
    let dest = repo().join("site/u6-hd-cli.zip");
    let o = e.run(&["pack", "--work", w, "--out", dest.to_str().unwrap()]);
    assert!(!o.status.success());
    assert!(!dest.exists());
    let _ = std::fs::remove_dir_all(&e.dir);
}

#[test]
fn a_failed_game_load_names_no_file() {
    let e = Env::new("errors");
    let files = e.dir.join("files");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(files.join("maptiles.ega"), b"not a game file").unwrap();
    let work = e.dir.join("work");
    let o = e.run(&[
        "inventory",
        "--game",
        files.to_str().unwrap(),
        "--work",
        work.to_str().unwrap(),
    ]);
    assert!(!o.status.success());
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(stderr.contains("did not load"), "{stderr}");
    assert!(!stderr.contains("maptiles"), "{stderr}");
    assert!(!stderr.contains(".ega"), "{stderr}");
    // The details are in a file only the owner reads.
    use std::os::unix::fs::PermissionsExt;
    let diag = work.join("diagnostic.txt");
    let mode = std::fs::metadata(&diag).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let _ = std::fs::remove_dir_all(&e.dir);
}

#[test]
fn export_is_refused() {
    let o = Command::new(BIN).arg("export").output().unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("derived"));
}

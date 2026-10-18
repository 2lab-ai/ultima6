//! `ingest` calls nothing (spec 07 HD2: a re-ingest reads the sheets already generated, with no
//! upload): on the synthetic set, with a `codex` stand-in first on `PATH` that logs any call,
//! an ingest that judges every sheet, refuses one and lays out its re-roll starts no call.

use std::path::{Path, PathBuf};
use std::process::Command;

use u6_hdtool::atlas::SheetKind;
use u6_hdtool::work::Work;
use u6_hdtool::{img, run};

const BIN: &str = env!("CARGO_BIN_EXE_u6-hd");

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("u6-hd-offline-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// `bin/codex`: appends a line to `$STUB_LOG` per call.
fn stub(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let p = bin.join("codex");
    std::fs::write(&p, "#!/bin/sh\necho call >> \"$STUB_LOG\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

#[test]
fn ingest_makes_no_codex_call() {
    let dir = temp("ingest");
    let bin = stub(&dir);
    let home = dir.join("home");
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    let log = dir.join("calls.log");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let cmd = |prog: &str| {
        let mut c = Command::new(prog);
        c.env("PATH", &path)
            .env("HOME", &home)
            .env("CODEX_HOME", home.join(".codex"))
            .env("STUB_LOG", &log)
            .current_dir(&dir);
        c
    };
    let calls = || std::fs::read_to_string(&log).map_or(0, |t| t.lines().count());
    // The stand-in is the `codex` this environment finds.
    assert!(
        cmd("/bin/sh")
            .args(["-c", "codex"])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(calls(), 1);
    std::fs::remove_file(&log).unwrap();

    let work = dir.join("work");
    let w = work.to_str().unwrap();
    let o = cmd(BIN)
        .args(["atlas", "--synthetic", "--work", w])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // Every sheet back as sent, but the tile sheet one pixel narrower: refused, re-rolled.
    let wk = Work::open(Some(&work)).unwrap();
    for job in wk.read_jobs().unwrap() {
        let sent = img::read_png(&wk.sheet(&job.sheet), job.width, job.height).unwrap();
        let back = if job.kind == SheetKind::Tile {
            sent.rect(0, 0, job.width - 1, job.height)
        } else {
            sent
        };
        run::record(&wk, &job, &img::encode_png(&back).unwrap()).unwrap();
    }
    let o = cmd(BIN)
        .args(["ingest", "--synthetic", "--work", w])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(out.contains("re-rolls queued: "), "{out}");
    assert!(!out.contains("re-rolls queued: 0 items"), "{out}");
    assert!(out.contains(
        "neighbour pairs failing (among pairs with a generated side): 0 before the fixed point, \
         0 after"
    ));
    assert_eq!(calls(), 0, "ingest started codex");
    let _ = std::fs::remove_dir_all(&dir);
}

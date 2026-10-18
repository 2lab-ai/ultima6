//! `ingest` and `calibrate` call nothing (spec 07 HD2: a re-ingest reads the sheets already
//! generated, with no upload): on the synthetic set, an ingest that judges every sheet, refuses
//! one and lays out its re-roll starts no `codex`, and neither does a calibrate that writes its
//! sheets, nor its final read.
//!
//! How the tool finds `codex` (`run::Codex::from_env`): the `--codex PATH` argument when given,
//! else the name `codex`, which the process spawn looks up on `PATH`. No environment variable
//! and no config file names the binary; `CODEX_HOME` (else `$HOME/.codex`) is only the folder
//! codex keeps its images in. So the test points both routes at a stand-in that logs every
//! call: its folder first on `PATH`, and `--codex` given to ingest too (the argument parser is
//! shared, so ingest and calibrate take it). `HOME` and `CODEX_HOME` are folders of the test. Afterwards
//! `run` reaches the stand-in by each route, so the routes were live while ingest ran.

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

/// `bin/codex`: appends a line to `$STUB_LOG` per call, and gives no image.
fn stub(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let p = bin.join("codex");
    std::fs::write(
        &p,
        "#!/bin/sh\necho call >> \"$STUB_LOG\"\ncat > /dev/null\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn ingest_makes_no_codex_call() {
    let dir = temp("ingest");
    let codex = stub(&dir);
    let home = dir.join("home");
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    let log = dir.join("calls.log");
    let path = format!(
        "{}:{}",
        codex.parent().unwrap().display(),
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
    // The calls logged so far: no log is none; a log that cannot be read is no answer.
    let calls = || match std::fs::read_to_string(&log) {
        Ok(t) => t.lines().count(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => panic!("the call log: {e}"),
    };
    // The stand-in is the `codex` this environment finds.
    assert!(
        cmd("/bin/sh")
            .args(["-c", "codex < /dev/null"])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(calls(), 1);
    std::fs::remove_file(&log).unwrap();
    assert_eq!(calls(), 0);

    let atlas = |work: &Path| {
        let o = cmd(BIN)
            .args(["atlas", "--synthetic", "--work", work.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    };
    let work = dir.join("work");
    let w = work.to_str().unwrap();
    atlas(&work);
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
    let c = codex.to_str().unwrap();
    let o = cmd(BIN)
        .args(["ingest", "--synthetic", "--work", w, "--codex", c])
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

    // The routes were live: `run` (in a work dir of its own) reaches the stand-in by `PATH`,
    // then by `--codex`.
    let other = dir.join("other");
    atlas(&other);
    let o = other.to_str().unwrap();
    for args in [vec![], vec!["--codex", c]] {
        let before = calls();
        let mut a = vec!["run", "--work", o, "--send", "--limit", "1"];
        a.extend(&args);
        let _ = cmd(BIN).args(&a).output().unwrap();
        assert!(calls() > before, "run {args:?} did not reach the stand-in");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn calibrate_makes_no_codex_call() {
    // On the synthetic set, a made-up first-pass row and an empty slices dir: the sheets and
    // samples.tsv are written, and neither route to `codex` is taken.
    let dir = temp("calibrate");
    let codex = stub(&dir);
    let home = dir.join("home");
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    let log = dir.join("calls.log");
    let path = format!(
        "{}:{}",
        codex.parent().unwrap().display(),
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
    let calls = || match std::fs::read_to_string(&log) {
        Ok(t) => t.lines().count(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => panic!("the call log: {e}"),
    };
    let work = dir.join("work");
    let wk = Work::open(Some(&work)).unwrap();
    let row = r#"{"neighbour":{"a":"t:0","b":"t:2","edge":"east"},"pass":1,"fail":true,"ratio":3.0,"limit":1.5,"sides":"gen-mmpx","source":"map","at":{"z":0,"x":1,"y":1},"gates":"made up"}"#;
    wk.write("gates.jsonl", format!("{row}\n").as_bytes())
        .unwrap();
    std::fs::create_dir_all(dir.join("slices")).unwrap();
    let out = dir.join("calibrate");
    let c = codex.to_str().unwrap();
    let o = cmd(BIN)
        .args(["calibrate", "--synthetic", "--work", work.to_str().unwrap()])
        .args(["--slices", dir.join("slices").to_str().unwrap()])
        .args(["--out", out.to_str().unwrap(), "--per", "2", "--seed", "5"])
        .args(["--at", "0,1,1", "--at", "leader,south", "--codex", c])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(text.contains("fail-gt1.5-gen-mmpx: 1 of 1"), "{text}");
    assert!(
        text.contains("control windows: 1 (the files place no leader)"),
        "{text}"
    );
    for f in [
        "gates.jsonl",
        "samples.tsv",
        "fail-gt1.5-gen-mmpx.png",
        "fail-gt1.5-gen-mmpx-s2.png",
        "control.png",
        "control-s2.png",
    ] {
        assert!(out.join(f).is_file(), "{f}");
    }
    assert_eq!(calls(), 0, "calibrate started codex");

    // The final read, in a work dir of its own that ingest left with nothing generated: it
    // reads the work dir's state, draws no window, and starts no codex either.
    let fin = dir.join("final-work");
    let fw = Work::open(Some(&fin)).unwrap();
    fw.write("gates.jsonl", b"").unwrap();
    fw.write("ingest.json", b"[]").unwrap();
    std::fs::create_dir_all(fin.join("slices")).unwrap();
    let fout = dir.join("calibrate-final");
    let o = cmd(BIN)
        .args(["calibrate", "--final", "--synthetic"])
        .args(["--work", fin.to_str().unwrap()])
        .args(["--out", fout.to_str().unwrap(), "--codex", c])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(text.contains("the record and the slices agree"), "{text}");
    assert!(text.contains("samples.tsv: 0 rows"), "{text}");
    assert!(fout.join("samples.tsv").is_file());
    // What the final read does not take, and what only it takes.
    let refused = [
        vec!["--final", "--slices", "x"],
        vec!["--final", "--at", "leader"],
        vec!["--final", "--per", "2"],
        vec!["--include", "x"],
    ];
    for extra in refused {
        let o = cmd(BIN)
            .args(["calibrate", "--synthetic", "--work", fin.to_str().unwrap()])
            .args(&extra)
            .output()
            .unwrap();
        assert!(!o.status.success(), "{extra:?}");
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.contains("--final"), "{extra:?}: {err}");
    }
    assert_eq!(calls(), 0, "calibrate --final started codex");

    // The routes were live: `run` (in a work dir of its own) reaches the stand-in by `PATH`,
    // then by `--codex`.
    let other = dir.join("other");
    let o = cmd(BIN)
        .args(["atlas", "--synthetic", "--work", other.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = other.to_str().unwrap();
    for args in [vec![], vec!["--codex", c]] {
        let before = calls();
        let mut a = vec!["run", "--work", o, "--send", "--limit", "1"];
        a.extend(&args);
        let _ = cmd(BIN).args(&a).output().unwrap();
        assert!(calls() > before, "run {args:?} did not reach the stand-in");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

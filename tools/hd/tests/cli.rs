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
    // A reservation and a result per call.
    let runs = std::fs::read_to_string(work.join("runs.jsonl")).unwrap();
    assert_eq!(runs.lines().count(), 2 * jobs);
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

/// A `codex` stand-in that takes its prompt, writes its pid to `$STUB_HOLD` and sleeps.
fn slow_stub(dir: &Path) -> PathBuf {
    let p = dir.join("codex-slow");
    std::fs::write(
        &p,
        "#!/bin/sh\ncat > /dev/null\necho $$ > \"$STUB_HOLD.tmp\"\nmv \"$STUB_HOLD.tmp\" \"$STUB_HOLD\"\nexec sleep 60\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn a_running_command_holds_the_work_dir_until_it_is_gone() {
    let e = Env::new("lock");
    let work = e.dir.join("work");
    let w = work.to_str().unwrap();
    let hold = e.dir.join("held");
    assert!(
        e.run(&["atlas", "--synthetic", "--work", w])
            .status
            .success()
    );
    let slow = slow_stub(&e.dir);
    let mut first = e
        .cmd(&[
            "run",
            "--work",
            w,
            "--codex",
            slow.to_str().unwrap(),
            "--send",
            "--limit",
            "1",
        ])
        .env("STUB_HOLD", &hold)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while !hold.exists() {
        assert!(
            start.elapsed().as_secs() < 30,
            "the first command never called codex"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let stub_pid = std::fs::read_to_string(&hold).unwrap().trim().to_string();
    // While it runs, every command on the work dir is refused, naming the holder.
    let codex = stub(&e.dir);
    let c = codex.to_str().unwrap();
    for args in [
        vec!["run", "--work", w, "--codex", c],
        vec!["ingest", "--synthetic", "--work", w],
        vec!["pack", "--work", w],
        vec!["atlas", "--synthetic", "--work", w, "--rebuild"],
    ] {
        let o = e.run(&args);
        assert!(!o.status.success(), "{args:?}: {}", text(&o));
        let pid = format!("another u6-hd (pid {})", first.id());
        assert!(text(&o).contains(&pid), "{args:?}: {}", text(&o));
    }
    // Killed outright (no handler runs): the lock goes with the process, though the file and
    // its codex child (in its own group) are still there.
    first.kill().unwrap();
    first.wait().unwrap();
    assert!(work.join(".u6-hd.lock").exists());
    let o = e.run(&["run", "--work", w, "--codex", c]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("Nothing was sent"), "{}", text(&o));
    assert_eq!(e.calls(), 0);
    let _ = Command::new("/bin/kill")
        .args(["-s", "KILL", "--", &format!("-{stub_pid}")])
        .status();
    let _ = std::fs::remove_dir_all(&e.dir);
}

/// Whether process `pid` is gone (or a zombie, dead and waiting to be reaped).
fn gone(pid: &str) -> bool {
    let o = Command::new("/bin/ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&o.stdout);
    stat.trim().is_empty() || stat.trim().starts_with('Z')
}

/// Kills the process group `pgid` when dropped (unwinding included): a failed assertion leaves
/// no stand-in running.
struct KillOnDrop(String);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if !self.0.is_empty() {
            let _ = Command::new("/bin/kill")
                .args(["-s", "KILL", "--", &format!("-{}", self.0)])
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

/// A `codex` stand-in that appends its pid to `$STUB_PIDS` and writes it to `$STUB_HOLD`
/// first thing, takes its prompt, then sleeps (`slow`) or exits at once with no image.
fn signal_stub(dir: &Path, slow: bool) -> PathBuf {
    let p = dir.join(if slow { "codex-sleeps" } else { "codex-quits" });
    let then = if slow { "exec sleep 60" } else { "exit 0" };
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\necho $$ >> \"$STUB_PIDS\"\necho $$ > \"$STUB_HOLD.tmp\"\n\
             mv \"$STUB_HOLD.tmp\" \"$STUB_HOLD\"\ncat > /dev/null\n{then}\n"
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// Whether process `pid` does not exist at all (not even as a zombie: reaped).
#[cfg(debug_assertions)]
fn reaped(pid: &str) -> bool {
    let o = Command::new("/bin/ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().is_empty()
}

/// The pids in `file` (one per line; none if it is missing).
fn pids(file: &Path) -> Vec<String> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// The stop signals (SIGINT, SIGTERM, SIGHUP, SIGQUIT) at each point of a call's life (test builds hold the hook that sends one
/// at the points in the tool: `U6_HD_TEST_SIGNAL*`; release builds have none): before the
/// spawn, after it (the group owned, before the first turn of its loop, held there 2.5 s,
/// past any budget the tool once had), in the middle of the call (sent by the test to the
/// tool's process group, as a terminal sends Ctrl-C), and while the call's group is reaped.
/// Each time: the tool ends by that signal; every stand-in it started is reaped by then; a
/// second command, trying all along, gets the lock only after that; no call starts once the
/// signal is in.
#[cfg(debug_assertions)]
#[test]
fn a_stop_signal_at_any_point_ends_every_call_before_the_lock_goes() {
    use std::io::BufRead;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    let points = ["before-spawn", "after-spawn", "mid-call", "reap"];
    for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
        for at in points {
            let what = format!("signal {sig} at {at}");
            let e = Env::new(&format!("stop-{sig}-{at}"));
            let work = e.dir.join("work");
            let w = work.to_str().unwrap().to_string();
            let (hold, started) = (e.dir.join("held"), e.dir.join("started"));
            assert!(
                e.run(&["atlas", "--synthetic", "--work", &w])
                    .status
                    .success()
            );
            let bin = signal_stub(&e.dir, at != "reap");
            // Three sheets, two at a time: after the signal, the third must never start.
            let mut cmd = e.cmd(&[
                "run",
                "--work",
                &w,
                "--codex",
                bin.to_str().unwrap(),
                "--send",
                "--limit",
                "3",
                "--parallel",
                "2",
            ]);
            cmd.env("STUB_HOLD", &hold)
                .env("STUB_PIDS", &started)
                .process_group(0)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            if at != "mid-call" {
                cmd.env("U6_HD_TEST_SIGNAL", sig.to_string())
                    .env("U6_HD_TEST_SIGNAL_AT", at);
            }
            if at == "after-spawn" {
                cmd.env("U6_HD_TEST_SPAWN_WAIT", &hold)
                    .env("U6_HD_TEST_DELAY_MS", "2500");
            }
            let mut tool = cmd.spawn().unwrap();
            let pgid = tool.id() as i32;
            // The tool holds the lock once it prints its notice.
            let mut lines = std::io::BufReader::new(tool.stdout.take().unwrap()).lines();
            let notice = lines.next().unwrap().unwrap();
            assert!(notice.contains("sheet(s) to generate"), "{what}: {notice}");
            let rest = std::thread::spawn(move || lines.count());
            // A second command, tried all along: when it gets the lock, every stand-in started
            // must be reaped already.
            let contender = {
                let (bin, w, started) = (BIN.to_string(), w.clone(), started.clone());
                let (home, log) = (e.home.clone(), e.log.clone());
                std::thread::spawn(move || {
                    let t = std::time::Instant::now();
                    loop {
                        assert!(t.elapsed().as_secs() < 30, "the lock never came free");
                        let o = Command::new(&bin)
                            .args(["run", "--work", &w])
                            .env("HOME", &home)
                            .env("CODEX_HOME", home.join(".codex"))
                            .env("STUB_LOG", &log)
                            .output()
                            .unwrap();
                        if o.status.success() {
                            let all: Vec<String> = pids(&started);
                            return (all.iter().all(|p| reaped(p)), all);
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                })
            };
            if at == "mid-call" {
                let t = std::time::Instant::now();
                while !hold.exists() {
                    assert!(t.elapsed().as_secs() < 30, "{what}: the call never started");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                // SAFETY: kill(2) to the tool's process group.
                assert_eq!(unsafe { libc::kill(-pgid, sig) }, 0);
            }
            let status = tool.wait().unwrap();
            let _ = rest.join();
            let all = pids(&started);
            let _cleanup: Vec<KillOnDrop> = all.iter().map(|p| KillOnDrop(p.clone())).collect();
            assert_eq!(status.signal(), Some(sig), "{what}: {status:?}");
            for p in &all {
                assert!(reaped(p), "{what}: stand-in {p} outlived the tool");
            }
            let (all_reaped, seen) = contender.join().unwrap();
            assert!(
                all_reaped,
                "{what}: the lock came free before {seen:?} were reaped"
            );
            // No call starts once the signal is in: none before the spawn, at most the two
            // running ones otherwise.
            let runs = std::fs::read_to_string(work.join("runs.jsonl")).unwrap_or_default();
            let dispatched = runs
                .lines()
                .filter(|l| l.contains("\"dispatched\""))
                .count();
            if at == "before-spawn" {
                assert_eq!((all.len(), dispatched), (0, 0), "{what}");
            } else {
                assert!(
                    dispatched <= 2 && all.len() <= dispatched,
                    "{what}: {dispatched}"
                );
            }
            let _ = std::fs::remove_dir_all(&e.dir);
        }
    }
}

#[test]
fn a_call_starts_with_no_signal_blocked() {
    // Each stop signal sent to the stand-in itself ends it: the tool's mask while it spawns is
    // not the call's.
    for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        let e = Env::new(&format!("mask-{sig}"));
        let work = e.dir.join("work");
        let w = work.to_str().unwrap();
        let hold = e.dir.join("held");
        assert!(
            e.run(&["atlas", "--synthetic", "--work", w])
                .status
                .success()
        );
        let slow = slow_stub(&e.dir);
        let tool = e
            .cmd(&[
                "run",
                "--work",
                w,
                "--codex",
                slow.to_str().unwrap(),
                "--send",
                "--limit",
                "1",
            ])
            .env("STUB_HOLD", &hold)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let start = std::time::Instant::now();
        while !hold.exists() {
            assert!(start.elapsed().as_secs() < 30, "the call never started");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let call = std::fs::read_to_string(&hold).unwrap().trim().to_string();
        let _cleanup = KillOnDrop(call.clone());
        // SAFETY: kill(2) to the stand-in.
        assert_eq!(unsafe { libc::kill(call.parse().unwrap(), sig) }, 0);
        let start = std::time::Instant::now();
        while !gone(&call) {
            assert!(
                start.elapsed().as_secs() < 2,
                "signal {sig}: blocked in the call"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // The call failed as one whose codex was killed by a signal.
        let o = tool.wait_with_output().unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(out.contains("codex exited with signal"), "{out}");
        let _ = std::fs::remove_dir_all(&e.dir);
    }
}

#[test]
fn a_call_killed_before_its_result_still_counts() {
    // The tool killed outright (no handler runs) while its call works, twice: each call was
    // reserved before it was made, so each counts, and the sheet is then given up.
    let e = Env::new("killed-calls");
    let work = e.dir.join("work");
    let w = work.to_str().unwrap();
    let (hold, started) = (e.dir.join("held"), e.dir.join("started"));
    assert!(
        e.run(&["atlas", "--synthetic", "--work", w])
            .status
            .success()
    );
    let slow = signal_stub(&e.dir, true);
    let first = |runs: &str| {
        runs.lines()
            .find(|l| l.contains("\"dispatched\""))
            .and_then(|l| l.split("\"identity\":\"").nth(1))
            .map(|r| r.split('"').next().unwrap().to_string())
            .unwrap()
    };
    let mut ids = Vec::new();
    for round in 0..2 {
        let _ = std::fs::remove_file(&hold);
        let mut tool = e
            .cmd(&[
                "run",
                "--work",
                w,
                "--codex",
                slow.to_str().unwrap(),
                "--send",
                "--limit",
                "1",
            ])
            .env("STUB_HOLD", &hold)
            .env("STUB_PIDS", &started)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let t = std::time::Instant::now();
        while !hold.exists() {
            assert!(
                t.elapsed().as_secs() < 30,
                "round {round}: the call never started"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        tool.kill().unwrap();
        tool.wait().unwrap();
        let runs = std::fs::read_to_string(work.join("runs.jsonl")).unwrap();
        assert_eq!(
            runs.lines()
                .filter(|l| l.contains("\"dispatched\""))
                .count(),
            round + 1
        );
        assert_eq!(runs.lines().filter(|l| l.contains("\"result\"")).count(), 0);
        ids.push(first(&runs));
    }
    for p in pids(&started) {
        drop(KillOnDrop(p));
    }
    // One sheet, called twice, never recorded: given up, and ingest refuses it.
    assert_eq!(ids[0], ids[1]);
    let o = e.run(&["run", "--work", w]);
    assert!(
        text(&o).contains("1 sheet(s) given up after 2 paid calls"),
        "{}",
        text(&o)
    );
    let o = e.run(&["ingest", "--synthetic", "--work", w]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("sheets refused (size, unreadable or source changed): 1"),
        "{}",
        text(&o)
    );
    let _ = std::fs::remove_dir_all(&e.dir);
}

#[test]
fn a_signal_the_tool_was_started_ignoring_lets_its_calls_finish() {
    // Under the real nohup (SIGHUP ignored), and with SIGINT ignored as a script's background
    // job has it: that signal to the tool's group stops nothing; the call runs to its end.
    for (name, how) in [("nohup", libc::SIGHUP), ("ignored-int", libc::SIGINT)] {
        let e = Env::new(name);
        let work = e.dir.join("work");
        let w = work.to_str().unwrap();
        let (hold, done) = (e.dir.join("held"), e.dir.join("done"));
        assert!(
            e.run(&["atlas", "--synthetic", "--work", w])
                .status
                .success()
        );
        let finishes = e.dir.join("codex-finishes");
        std::fs::write(
            &finishes,
            format!(
                "#!/bin/sh\necho $$ > \"{}\"\ncat > /dev/null\nsleep 1\necho done > \"{}\"\n",
                hold.display(),
                done.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&finishes, std::fs::Permissions::from_mode(0o755)).unwrap();
        let tool_args = format!(
            "\"{BIN}\" run --work \"{w}\" --codex \"{}\" --send --limit 1",
            finishes.display()
        );
        let line = if how == libc::SIGHUP {
            format!("exec /usr/bin/nohup {tool_args}")
        } else {
            format!("trap '' INT; exec {tool_args}")
        };
        use std::os::unix::process::{CommandExt, ExitStatusExt};
        let mut tool = Command::new("/bin/sh")
            .args(["-c", &line])
            .env("HOME", &e.home)
            .env("CODEX_HOME", e.home.join(".codex"))
            .current_dir(&e.dir)
            .process_group(0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let t = std::time::Instant::now();
        while !hold.exists() {
            assert!(t.elapsed().as_secs() < 30, "{name}: the call never started");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let call = std::fs::read_to_string(&hold).unwrap().trim().to_string();
        let _cleanup = KillOnDrop(call);
        // SAFETY: kill(2) to the tool's process group.
        assert_eq!(unsafe { libc::kill(-(tool.id() as i32), how) }, 0);
        let status = tool.wait().unwrap();
        // The tool was not ended by it: its call ran to its end (it gave no thread id, so
        // the run fails, as a call, not as a stop).
        assert_eq!(status.signal(), None, "{name}: {status:?}");
        assert!(done.exists(), "{name}: the call was cut off");
        let runs = std::fs::read_to_string(work.join("runs.jsonl")).unwrap();
        assert!(runs.contains("\"stopped\":false"), "{name}: {runs}");
        assert!(runs.contains("no thread id"), "{name}: {runs}");
        let _ = std::fs::remove_dir_all(&e.dir);
    }
}

#[test]
fn a_codex_that_cannot_be_started_spends_nothing() {
    // A mistyped --codex: no process is made, nothing is sent; each reservation is taken back,
    // so however often it is run, no sheet is given up.
    let e = Env::new("no-codex");
    let work = e.dir.join("work");
    let w = work.to_str().unwrap();
    assert!(
        e.run(&["atlas", "--synthetic", "--work", w])
            .status
            .success()
    );
    let missing = e.dir.join("no-such-codex");
    for _ in 0..3 {
        let o = e.run(&[
            "run",
            "--work",
            w,
            "--codex",
            missing.to_str().unwrap(),
            "--send",
        ]);
        assert!(!o.status.success());
        assert!(text(&o).contains("not sent"), "{}", text(&o));
    }
    let o = e.run(&["run", "--work", w]);
    assert!(!text(&o).contains("given up"), "{}", text(&o));
    let runs = std::fs::read_to_string(work.join("runs.jsonl")).unwrap();
    let count = |ev: &str| {
        runs.lines()
            .filter(|l| l.contains(&format!("\"{ev}\"")))
            .count()
    };
    assert!(count("dispatched") > 0);
    assert_eq!(count("dispatched"), count("voided"));
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

/// `inventory --game files`: fails with `category` alone on the terminal, the details in the
/// 0600 diagnostic file; returns the details, stdout and stderr.
fn failed_load(e: &Env, files: &Path, category: &str) -> (String, String, String) {
    let work = e.dir.join("work");
    let o = e.run(&[
        "inventory",
        "--game",
        files.to_str().unwrap(),
        "--work",
        work.to_str().unwrap(),
    ]);
    assert!(!o.status.success());
    let (stdout, stderr) = (
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr),
    );
    assert!(stderr.contains(category), "{stderr}");
    // The details are in a file only the owner reads, and only there.
    use std::os::unix::fs::PermissionsExt;
    let diag = work.join("diagnostic.txt");
    let mode = std::fs::metadata(&diag).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let text = std::fs::read_to_string(&diag).unwrap();
    let (first, detail) = text.split_once('\n').unwrap();
    assert_eq!(first, category);
    let detail = detail.trim_end().to_string();
    assert!(!detail.is_empty());
    for line in detail.lines().filter(|l| !l.trim().is_empty()) {
        assert!(!stdout.contains(line), "on stdout: {line}");
        assert!(!stderr.contains(line), "on stderr: {line}");
    }
    assert!(stdout.is_empty(), "{stdout}");
    (detail, stdout.into_owned(), stderr.into_owned())
}

/// Each of `values` is absent from stdout and from stderr.
fn absent(values: &[&str], stdout: &str, stderr: &str) {
    for v in values {
        assert!(!stdout.contains(v), "{v:?} on stdout: {stdout}");
        assert!(!stderr.contains(v), "{v:?} on stderr: {stderr}");
    }
}

#[test]
fn a_game_that_cannot_be_read_names_no_file() {
    let e = Env::new("unreadable");
    let files = e.dir.join("files");
    std::fs::create_dir_all(&files).unwrap();
    let locked = files.join("owner-disk-name.adf");
    std::fs::write(&locked, b"x").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let (detail, stdout, stderr) = failed_load(&e, &files, "the game files could not be read");
    // The details name the file and the error (positive controls); the terminal shows neither,
    // each value checked on its own, on each stream.
    let error = std::io::Error::from_raw_os_error(13).to_string();
    for v in ["owner-disk-name.adf", error.as_str()] {
        assert!(detail.contains(v), "{v:?} not in {detail}");
    }
    absent(
        &[
            "owner-disk-name.adf",
            "owner-disk-name",
            error.as_str(),
            "ermission denied",
        ],
        &stdout,
        &stderr,
    );
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).unwrap();
    let _ = std::fs::remove_dir_all(&e.dir);
}

#[test]
fn a_game_the_engine_does_not_load_names_no_file() {
    let e = Env::new("errors");
    let files = e.dir.join("files");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(files.join("maptiles.ega"), b"not a game file").unwrap();
    let (detail, stdout, stderr) =
        failed_load(&e, &files, "the game files did not load as Ultima VI");
    // What the engine said is in the details only (its own words, whatever they are), and no
    // name of the files given is on the terminal either (checked in failed_load by line).
    assert_ne!(detail, "the game files did not load as Ultima VI");
    // Positive control: the engine's error names a game file, which the terminal must not.
    assert!(detail.contains(".ega"), "{detail}");
    absent(&[detail.as_str(), ".ega"], &stdout, &stderr);
    let o = e.run(&[
        "inventory",
        "--game",
        files.to_str().unwrap(),
        "--work",
        e.dir.join("work").to_str().unwrap(),
    ]);
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    for name in ["maptiles", ".ega", "not a game file"] {
        assert!(!all.contains(name), "{name}: {all}");
    }
    let _ = std::fs::remove_dir_all(&e.dir);
}

#[test]
fn export_is_refused() {
    let o = Command::new(BIN).arg("export").output().unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("derived"));
}

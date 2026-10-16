//! The `u6-hd` binary end to end, on the synthetic set only, with a recording stand-in for
//! `codex` (a shell script that logs each call and answers with the sheet it was given).

use std::os::unix::process::CommandExt as _;
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
        .process_group(0)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let _first_guard = KillOnDrop(first.id().to_string());
    let _stub_guard = scopeguard_pids(&hold);
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
    wait_within(&mut first, 30, "the killed command");
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

/// Whether process `pid` is gone: not even a zombie (reaped, by its parent or by launchd once
/// orphaned; callers poll).
fn gone(pid: &str) -> bool {
    let o = Command::new("/bin/ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().is_empty()
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

/// Waits for `child` at most `secs`: past that, its process group is killed and the test
/// fails (a hang is a failure, not a stuck gate).
fn wait_within(child: &mut std::process::Child, secs: u64, what: &str) -> std::process::ExitStatus {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    wait_until(child, deadline, what)
}

/// Waits for `child` until `deadline` (absolute): past it, its process group is killed and the
/// test fails.
fn wait_until(
    child: &mut std::process::Child,
    deadline: std::time::Instant,
    what: &str,
) -> std::process::ExitStatus {
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s;
        }
        if std::time::Instant::now() >= deadline {
            drop(KillOnDrop(child.id().to_string()));
            let _ = child.kill();
            let _ = child.wait();
            panic!("{what}: still running at its deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// `cmd`'s output, its process in a group of its own: its exit and both pipes' ends within
/// one deadline of `secs` (a descendant holding a pipe counts); past it, its group is killed
/// and the test fails.
fn output_within(cmd: &mut Command, secs: u64, what: &str) -> Output {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    let mut child = cmd
        .process_group(0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let guard = KillOnDrop(child.id().to_string());
    let (tx, rx) = std::sync::mpsc::channel();
    for (n, mut pipe) in [
        (
            0,
            Box::new(child.stdout.take().unwrap()) as Box<dyn Read + Send>,
        ),
        (1, Box::new(child.stderr.take().unwrap())),
    ] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = pipe.read_to_end(&mut v);
            let _ = tx.send((n, v));
        });
    }
    let left = || deadline.saturating_duration_since(std::time::Instant::now());
    let status = wait_until(&mut child, deadline, what);
    let mut got = [Vec::new(), Vec::new()];
    for _ in 0..2 {
        match rx.recv_timeout(left()) {
            Ok((n, v)) => got[n] = v,
            Err(_) => {
                drop(guard);
                panic!("{what}: its output did not end in {secs} s");
            }
        }
    }
    let [stdout, stderr] = got;
    Output {
        status,
        stdout,
        stderr,
    }
}

/// The `<what> <ns>` notes the tool's test hooks wrote to `file` (`U6_HD_TEST_NOTES`).
fn notes(file: &Path) -> Vec<(String, u128)> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (w, ns) = l.rsplit_once(' ')?;
            Some((w.to_string(), ns.parse().ok()?))
        })
        .collect()
}

/// The run ledger's lines of `event`: (sheet, identity, at).
fn ledger(work: &Path, event: &str) -> Vec<(String, String, u128)> {
    std::fs::read_to_string(work.join("runs.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["event"] == event)
        .map(|v| {
            (
                v["sheet"].as_str().unwrap_or_default().to_string(),
                v["identity"].as_str().unwrap_or_default().to_string(),
                v["at"].as_u64().map_or(0, u128::from),
            )
        })
        .collect()
}

/// A stand-in that starts a descendant of its own (`sleep 60` in its group), records both
/// pids in `$STUB_PIDS` and its own in `$STUB_LEADERS`, takes its prompt, then sleeps; the
/// first one to find `$STUB_QUIT` (a directory it makes) absent exits at once instead.
fn family_stub(dir: &Path) -> PathBuf {
    let p = dir.join("codex-family");
    std::fs::write(
        &p,
        "#!/bin/sh\nsleep 60 &\nkid=$!\necho $$ >> \"$STUB_PIDS\"\necho $kid >> \"$STUB_PIDS\"\n\
         echo $$ >> \"$STUB_LEADERS\"\ncat > /dev/null\n\
         if [ -n \"$STUB_QUIT\" ] && mkdir \"$STUB_QUIT\" 2>/dev/null; then exit 0; fi\n\
         exec sleep 60\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// The stop signals (SIGINT, SIGTERM, SIGHUP, SIGQUIT) at each point of a call's life, with
/// two calls confirmed running and work still queued (four sheets, three at a time): before
/// the third call's admission, inside its reservation (before the admission point), after its
/// spawn (its group owned, held 2.2 s before its first turn, past the old 2 s budget), in the
/// middle of the calls (sent by the test to the tool's process group, as a terminal sends
/// Ctrl-C), and while the first call's group is being reaped (held 1 s, killed, not waited
/// for). Test builds hold the hooks (`U6_HD_TEST_*`); release builds have none. Each time:
/// - the tool ends by that signal;
/// - every process the stand-ins recorded (each one and a descendant of its own) is gone,
///   each stand-in reaped by the tool;
/// - every call the tool spawned is reaped before a second command, trying all along, takes
///   the lock (the times the hooks note);
/// - no reservation that was not voided, and so no spawn, is later than the stop; the fourth
///   sheet is never reserved; inside the reservation the stop voids it and nothing spawns.
#[cfg(debug_assertions)]
#[test]
fn a_stop_signal_at_any_point_ends_every_call_before_the_lock_goes() {
    use std::io::BufRead;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
        for at in ["before-spawn", "reserve", "after-spawn", "mid-call", "reap"] {
            let what = format!("signal {sig} at {at}");
            let e = Env::new(&format!("stop-{sig}-{at}"));
            let work = e.dir.join("work");
            let w = work.to_str().unwrap().to_string();
            let (stub_pids, leaders) = (e.dir.join("pids"), e.dir.join("leaders"));
            let (tool_notes, lock_notes) = (e.dir.join("tool-notes"), e.dir.join("lock-notes"));
            assert!(
                e.run(&["atlas", "--synthetic", "--work", &w])
                    .status
                    .success()
            );
            let sheets: Vec<String> = std::fs::read_to_string(work.join("jobs.jsonl"))
                .unwrap()
                .lines()
                .map(|l| {
                    serde_json::from_str::<serde_json::Value>(l).unwrap()["sheet"]
                        .as_str()
                        .unwrap()
                        .to_string()
                })
                .collect();
            assert!(sheets.len() >= 4);
            let bin = family_stub(&e.dir);
            let mut cmd = e.cmd(&[
                "run",
                "--work",
                &w,
                "--codex",
                bin.to_str().unwrap(),
                "--send",
                "--limit",
                "4",
                "--parallel",
                "3",
            ]);
            cmd.env("STUB_PIDS", &stub_pids)
                .env("STUB_LEADERS", &leaders)
                .env("U6_HD_TEST_NOTES", &tool_notes)
                .env("U6_HD_TEST_WAIT", &leaders)
                .process_group(0)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            let (nth, lines, delay) = match at {
                "before-spawn" | "reserve" => (3, 2, 0),
                "after-spawn" => (3, 3, 2200),
                "reap" => (1, 3, 1000),
                _ => (0, 0, 0),
            };
            if at != "mid-call" {
                cmd.env("U6_HD_TEST_SIGNAL", sig.to_string())
                    .env("U6_HD_TEST_SIGNAL_AT", at)
                    .env("U6_HD_TEST_NTH", nth.to_string())
                    .env("U6_HD_TEST_WAIT_LINES", lines.to_string())
                    .env("U6_HD_TEST_DELAY_MS", delay.to_string());
            }
            if at == "reap" {
                cmd.env("STUB_QUIT", e.dir.join("quit"));
            }
            let mut tool = cmd.spawn().unwrap();
            let _tool_guard = KillOnDrop(tool.id().to_string());
            let _stub_guard = scopeguard_pids(&stub_pids);
            let pgid = tool.id() as i32;
            // The tool holds the lock once it prints its notice (read under a deadline).
            let (tx, rx) = std::sync::mpsc::channel();
            let stdout = tool.stdout.take().unwrap();
            let reader = std::thread::spawn(move || {
                for l in std::io::BufReader::new(stdout)
                    .lines()
                    .map_while(Result::ok)
                {
                    let _ = tx.send(l);
                }
            });
            let notice = rx
                .recv_timeout(std::time::Duration::from_secs(30))
                .unwrap_or_else(|_| panic!("{what}: no notice"));
            assert!(notice.contains("sheet(s) to generate"), "{what}: {notice}");
            // A second command, tried all along; it notes when it takes the lock.
            let contender = {
                let mut c = e.cmd(&["run", "--work", &w]);
                c.env("U6_HD_TEST_NOTES", &lock_notes);
                let what = what.clone();
                std::thread::spawn(move || {
                    let t = std::time::Instant::now();
                    loop {
                        assert!(
                            t.elapsed().as_secs() < 60,
                            "{what}: the lock never came free"
                        );
                        if output_within(&mut c, 20, &what).status.success() {
                            return;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                })
            };
            if at == "mid-call" {
                let t = std::time::Instant::now();
                while pids(&leaders).len() < 3 {
                    assert!(
                        t.elapsed().as_secs() < 30,
                        "{what}: the calls never started"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                let ns = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                std::fs::write(&tool_notes, format!("stop {ns}\n")).unwrap();
                // SAFETY: kill(2) to the tool's process group.
                assert_eq!(unsafe { libc::kill(-pgid, sig) }, 0);
            }
            let status = wait_within(&mut tool, 60, &what);
            let _ = reader.join();
            assert_eq!(status.signal(), Some(sig), "{what}: {status:?}");
            // Every stand-in was reaped by the tool; every descendant is gone.
            let leaders_now = pids(&leaders);
            for p in &leaders_now {
                assert!(
                    reaped(p),
                    "{what}: stand-in {p} not reaped when the tool ended"
                );
            }
            for p in pids(&stub_pids) {
                let t = std::time::Instant::now();
                while !gone(&p) {
                    assert!(t.elapsed().as_secs() < 3, "{what}: {p} outlived the tool");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            contender.join().unwrap();
            // The order: the stop, every reap, then the lock.
            let tool_notes = notes(&tool_notes);
            let stop = tool_notes
                .iter()
                .find(|(w, _)| w == "stop")
                .map(|n| n.1)
                .unwrap();
            let reaps: Vec<u128> = tool_notes
                .iter()
                .filter(|(w, _)| w.starts_with("reaped"))
                .map(|n| n.1)
                .collect();
            let locked = notes(&lock_notes)
                .iter()
                .find(|(w, _)| w == "locked")
                .map(|n| n.1)
                .unwrap();
            let last_reap = reaps.iter().copied().max().unwrap_or(0);
            assert!(
                locked > last_reap,
                "{what}: the lock came free before a reap"
            );
            assert!(locked > stop, "{what}: the lock came free before the stop");
            // Reservations and spawns against the stop.
            let reserved = ledger(&work, "dispatched");
            let voided = ledger(&work, "voided");
            let kept: Vec<&(String, String, u128)> = reserved
                .iter()
                .filter(|r| !voided.iter().any(|v| v.1 == r.1))
                .collect();
            for r in &kept {
                assert!(r.2 < stop, "{what}: {} reserved after the stop", r.0);
            }
            assert!(
                reserved.iter().all(|r| r.0 != sheets[3]),
                "{what}: the queued sheet was reserved"
            );
            assert_eq!(reaps.len(), kept.len(), "{what}: every spawned call reaped");
            assert_eq!(
                leaders_now.len(),
                kept.len(),
                "{what}: every admitted call ran"
            );
            // The reaped pids are the stand-ins', each once.
            let mut reaped_pids: Vec<String> = tool_notes
                .iter()
                .filter_map(|(w, _)| w.strip_prefix("reaped ").map(str::to_string))
                .collect();
            reaped_pids.sort();
            let mut want_pids = leaders_now.clone();
            want_pids.sort();
            assert_eq!(
                reaped_pids, want_pids,
                "{what}: the reaped are not the stand-ins"
            );
            let want = match at {
                "before-spawn" => (2, 0),
                "reserve" => (3, 1),
                _ => (3, 0),
            };
            assert_eq!((reserved.len(), voided.len()), want, "{what}");
            if at == "reserve" {
                assert!(voided[0].2 > stop, "{what}: voided before the stop");
            }
            let _ = std::fs::remove_dir_all(&e.dir);
        }
    }
}

/// Kills, on drop, every pid recorded in `file` and its group (each stand-in leads one).
fn scopeguard_pids(file: &Path) -> impl Drop + use<> {
    struct G(PathBuf);
    impl Drop for G {
        fn drop(&mut self) {
            for p in pids(&self.0) {
                drop(KillOnDrop(p));
            }
        }
    }
    G(file.to_path_buf())
}

/// Waits until `file` lists `n` pids (at most 30 s).
fn wait_for_pids(file: &Path, n: usize, what: &str) {
    let t = std::time::Instant::now();
    while pids(file).len() < n {
        assert!(
            t.elapsed().as_secs() < 30,
            "{what}: the calls never started"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Every pid recorded in `stub_pids` gone within 3 s (the stand-ins and their descendants).
fn all_gone(stub_pids: &Path, what: &str) {
    for p in pids(stub_pids) {
        let t = std::time::Instant::now();
        while !gone(&p) {
            assert!(t.elapsed().as_secs() < 3, "{what}: {p} outlived the tool");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

#[cfg(debug_assertions)]
#[test]
fn two_stop_signals_in_a_row_are_one_stop() {
    // Two of the same stop signal 1 ms apart (a closed terminal sends SIGHUP twice), four
    // calls running: the second changes nothing; every call is reaped, then the tool ends by
    // the signal.
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    for sig in [libc::SIGHUP, libc::SIGTERM, libc::SIGINT] {
        let what = format!("signal {sig} twice");
        let e = Env::new(&format!("twice-{sig}"));
        let work = e.dir.join("work");
        let w = work.to_str().unwrap();
        let (stub_pids, leaders, notes_file) = (
            e.dir.join("pids"),
            e.dir.join("leaders"),
            e.dir.join("notes"),
        );
        assert!(
            e.run(&["atlas", "--synthetic", "--work", w])
                .status
                .success()
        );
        let bin = family_stub(&e.dir);
        let mut tool = e
            .cmd(&[
                "run",
                "--work",
                w,
                "--codex",
                bin.to_str().unwrap(),
                "--send",
                "--parallel",
                "4",
            ])
            .env("STUB_PIDS", &stub_pids)
            .env("STUB_LEADERS", &leaders)
            .env("U6_HD_TEST_NOTES", &notes_file)
            .process_group(0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let _tool_guard = KillOnDrop(tool.id().to_string());
        let _stub_guard = scopeguard_pids(&stub_pids);
        wait_for_pids(&leaders, 4, &what);
        for _ in 0..2 {
            // SAFETY: kill(2) to the tool's process group.
            assert_eq!(unsafe { libc::kill(-(tool.id() as i32), sig) }, 0);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let status = wait_within(&mut tool, 30, &what);
        assert_eq!(status.signal(), Some(sig), "{what}: {status:?}");
        let mut leaders_now = pids(&leaders);
        for p in &leaders_now {
            assert!(reaped(p), "{what}: stand-in {p} not reaped");
        }
        all_gone(&stub_pids, &what);
        let mut reaped_pids: Vec<String> = notes(&notes_file)
            .iter()
            .filter_map(|(w, _)| w.strip_prefix("reaped ").map(str::to_string))
            .collect();
        reaped_pids.sort();
        leaders_now.sort();
        assert_eq!(reaped_pids, leaders_now, "{what}");
        let _ = std::fs::remove_dir_all(&e.dir);
    }
}

#[cfg(debug_assertions)]
#[test]
fn a_stop_that_stalls_yields_to_a_later_signal() {
    // The stop's cleanup held 6 s (a reap that does not return, as a stalled write would):
    // a second signal within the grace changes nothing; one after it (the stop pending 2 s)
    // ends the tool at once, by that signal.
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    let e = Env::new("stalled");
    let work = e.dir.join("work");
    let w = work.to_str().unwrap();
    let (stub_pids, leaders, mark) = (
        e.dir.join("pids"),
        e.dir.join("leaders"),
        e.dir.join("mark"),
    );
    assert!(
        e.run(&["atlas", "--synthetic", "--work", w])
            .status
            .success()
    );
    let bin = family_stub(&e.dir);
    let mut tool = e
        .cmd(&[
            "run",
            "--work",
            w,
            "--codex",
            bin.to_str().unwrap(),
            "--send",
            "--limit",
            "1",
        ])
        .env("STUB_PIDS", &stub_pids)
        .env("STUB_LEADERS", &leaders)
        .env("U6_HD_TEST_SIGNAL_AT", "reap")
        .env("U6_HD_TEST_MARK", &mark)
        .env("U6_HD_TEST_DELAY_MS", "6000")
        .process_group(0)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let _tool_guard = KillOnDrop(tool.id().to_string());
    let _stub_guard = scopeguard_pids(&stub_pids);
    let pgid = tool.id() as i32;
    wait_for_pids(&leaders, 1, "stalled");
    // SAFETY: kill(2) to the tool's process group.
    assert_eq!(unsafe { libc::kill(-pgid, libc::SIGTERM) }, 0);
    let t = std::time::Instant::now();
    while !mark.exists() {
        assert!(t.elapsed().as_secs() < 30, "the cleanup never started");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let stalled = std::time::Instant::now();
    // Within the grace: absorbed.
    std::thread::sleep(std::time::Duration::from_millis(500));
    // SAFETY: kill(2) to the tool's process group.
    assert_eq!(unsafe { libc::kill(-pgid, libc::SIGTERM) }, 0);
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(
        tool.try_wait().unwrap().is_none(),
        "a signal within the grace ended the tool"
    );
    // After it: the default action, at once.
    std::thread::sleep(std::time::Duration::from_millis(2000));
    let sent = std::time::Instant::now();
    // SAFETY: kill(2) to the tool's process group.
    assert_eq!(unsafe { libc::kill(-pgid, libc::SIGTERM) }, 0);
    let status = wait_within(&mut tool, 30, "stalled");
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
    assert!(sent.elapsed().as_millis() < 1500, "{:?}", sent.elapsed());
    assert!(stalled.elapsed().as_secs() < 5, "the stall was waited out");
    all_gone(&stub_pids, "stalled");
    let _ = std::fs::remove_dir_all(&e.dir);
}

#[cfg(debug_assertions)]
#[test]
fn ignored_signals_during_a_stopped_cleanup_change_nothing() {
    // Started with SIGHUP and SIGINT ignored, stopped by SIGTERM: while the call's group is
    // held unreaped (1 s), the ignored signals come to the tool's group; the cleanup ends all
    // the same, after them, and the tool ends by SIGTERM.
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    let e = Env::new("mixed");
    let work = e.dir.join("work");
    let w = work.to_str().unwrap();
    let (stub_pids, leaders, mark, notes_file) = (
        e.dir.join("pids"),
        e.dir.join("leaders"),
        e.dir.join("in-reap"),
        e.dir.join("notes"),
    );
    assert!(
        e.run(&["atlas", "--synthetic", "--work", w])
            .status
            .success()
    );
    let bin = family_stub(&e.dir);
    let line = format!(
        "trap '' HUP INT; exec \"{BIN}\" run --work \"{w}\" --codex \"{}\" --send --limit 1",
        bin.display()
    );
    let mut tool = Command::new("/bin/sh")
        .args(["-c", &line])
        .env("HOME", &e.home)
        .env("CODEX_HOME", e.home.join(".codex"))
        .env("STUB_PIDS", &stub_pids)
        .env("STUB_LEADERS", &leaders)
        .env("U6_HD_TEST_NOTES", &notes_file)
        .env("U6_HD_TEST_SIGNAL_AT", "reap")
        .env("U6_HD_TEST_MARK", &mark)
        .env("U6_HD_TEST_DELAY_MS", "1000")
        .process_group(0)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let _tool_guard = KillOnDrop(tool.id().to_string());
    let _stub_guard = scopeguard_pids(&stub_pids);
    let pgid = tool.id() as i32;
    let t = std::time::Instant::now();
    while pids(&leaders).is_empty() {
        assert!(t.elapsed().as_secs() < 30, "the call never started");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // SAFETY: kill(2) to the tool's process group.
    assert_eq!(unsafe { libc::kill(-pgid, libc::SIGTERM) }, 0);
    while !mark.exists() {
        assert!(t.elapsed().as_secs() < 30, "the cleanup never started");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let sent = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    for s in [libc::SIGHUP, libc::SIGINT] {
        // SAFETY: kill(2) to the tool's process group.
        assert_eq!(unsafe { libc::kill(-pgid, s) }, 0);
    }
    let status = wait_within(&mut tool, 30, "mixed");
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
    let reaped_at = notes(&notes_file)
        .iter()
        .find(|(w, _)| w.starts_with("reaped"))
        .map(|n| n.1)
        .unwrap();
    assert!(
        reaped_at > sent,
        "the cleanup ended before the ignored signals came"
    );
    for p in pids(&leaders) {
        assert!(reaped(&p), "{p} not reaped");
    }
    for p in pids(&stub_pids) {
        let t = std::time::Instant::now();
        while !gone(&p) {
            assert!(t.elapsed().as_secs() < 3, "{p} outlived the tool");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    let _ = std::fs::remove_dir_all(&e.dir);
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
            .process_group(0)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let _tool_guard = KillOnDrop(tool.id().to_string());
        let _stub_guard = scopeguard_pids(&hold);
        let start = std::time::Instant::now();
        while !hold.exists() {
            assert!(start.elapsed().as_secs() < 30, "the call never started");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let call = std::fs::read_to_string(&hold).unwrap().trim().to_string();
        let _cleanup = KillOnDrop(call.clone());
        // Once it is `sleep` (its exec done): a signal to the shell before that, while it waits
        // for its `mv`, is held by the shell until the `mv` ends, then dropped (the `mv` did not
        // die of it), and the stand-in would sleep on through it.
        let start = std::time::Instant::now();
        loop {
            let o = Command::new("/bin/ps")
                .args(["-o", "comm=", "-p", &call])
                .output()
                .unwrap();
            if String::from_utf8_lossy(&o.stdout).trim().ends_with("sleep") {
                break;
            }
            assert!(
                start.elapsed().as_secs() < 10,
                "signal {sig}: never exec'd sleep"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
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
        let mut tool = tool;
        let mut stdout = tool.stdout.take().unwrap();
        let read = std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = std::io::Read::read_to_end(&mut stdout, &mut v);
            v
        });
        wait_within(&mut tool, 30, "the tool");
        let out = String::from_utf8_lossy(&read.join().unwrap()).into_owned();
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
    let (stub_pids, leaders) = (e.dir.join("pids"), e.dir.join("leaders"));
    assert!(
        e.run(&["atlas", "--synthetic", "--work", w])
            .status
            .success()
    );
    let bin = family_stub(&e.dir);
    let _stub_guard = scopeguard_pids(&stub_pids);
    use std::os::unix::process::CommandExt;
    let mut records: Vec<(String, String, u128)> = Vec::new();
    for round in 0..2 {
        let mut tool = e
            .cmd(&[
                "run",
                "--work",
                w,
                "--codex",
                bin.to_str().unwrap(),
                "--send",
                "--limit",
                "1",
            ])
            .env("STUB_PIDS", &stub_pids)
            .env("STUB_LEADERS", &leaders)
            .process_group(0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let _tool_guard = KillOnDrop(tool.id().to_string());
        let t = std::time::Instant::now();
        while pids(&leaders).len() < round + 1 {
            assert!(
                t.elapsed().as_secs() < 30,
                "round {round}: the call never started"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        tool.kill().unwrap();
        wait_within(&mut tool, 30, "killed tool");
        // The earlier records unchanged, one more, no result for either.
        let now = ledger(&work, "dispatched");
        assert_eq!(now.len(), round + 1, "round {round}");
        assert_eq!(
            &now[..round],
            &records[..],
            "round {round}: an earlier record changed"
        );
        records = now;
        assert!(ledger(&work, "result").is_empty());
    }
    // Two distinct reservations (their times differ) of one sheet and identity.
    let (a, b) = (&records[0], &records[1]);
    assert_ne!(a.2, b.2);
    assert_eq!((&a.0, &a.1), (&b.0, &b.1));
    // Given up: another sending run reserves nothing for it, and calls the next sheet.
    let o = e.run(&["run", "--work", w]);
    assert!(
        text(&o).contains("1 sheet(s) given up after 2 reserved calls"),
        "{}",
        text(&o)
    );
    let quits = stub(&e.dir);
    let mut third = e.cmd(&[
        "run",
        "--work",
        w,
        "--codex",
        quits.to_str().unwrap(),
        "--send",
        "--limit",
        "1",
    ]);
    let o = output_within(&mut third, 60, "third run");
    assert!(o.status.success(), "{}", text(&o));
    let after = ledger(&work, "dispatched");
    assert_eq!(after.len(), 3);
    assert!(after[..2] == records[..]);
    assert_ne!(after[2].1, a.1, "the given-up identity was reserved again");
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
        let _tool_guard = KillOnDrop(tool.id().to_string());
        let _stub_guard = scopeguard_pids(&hold);
        let t = std::time::Instant::now();
        while !hold.exists() {
            assert!(t.elapsed().as_secs() < 30, "{name}: the call never started");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let call = std::fs::read_to_string(&hold).unwrap().trim().to_string();
        let _cleanup = KillOnDrop(call);
        // SAFETY: kill(2) to the tool's process group.
        assert_eq!(unsafe { libc::kill(-(tool.id() as i32), how) }, 0);
        let status = wait_within(&mut tool, 30, name);
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
fn run_turns_core_dumps_off_before_anything() {
    // Started with core dumps allowed as far as the inherited hard limit lets a shell raise
    // them (to min(1000, hard); not at all when hard is 0, as on a hardened runner), run sets
    // both limits to 0 first: a SIGQUIT at any time leaves no core holding the sheets. Run as
    // inherited, and with the hard limit lowered to 0 first (the hardened case, here).
    if !cfg!(debug_assertions) {
        return;
    }
    for (name, harden) in [("inherited", ""), ("hardened", "ulimit -Hc 0 || exit 98; ")] {
        let e = Env::new(&format!("core-{name}"));
        let work = e.dir.join("work");
        let w = work.to_str().unwrap();
        assert!(
            e.run(&["atlas", "--synthetic", "--work", w])
                .status
                .success()
        );
        let (notes_file, raised) = (e.dir.join("notes"), e.dir.join("raised"));
        // The shell reads the hard limit first, in its own units, and raises the soft one only
        // where it can, noting which (its check of the raise is the positive control).
        let line = format!(
            "{harden}h=$(ulimit -Hc); \
             if [ \"$h\" = unlimited ]; then n=1000; \
             elif [ \"$h\" -gt 0 ]; then if [ \"$h\" -lt 1000 ]; then n=$h; else n=1000; fi; \
             else n=; fi; \
             if [ -n \"$n\" ]; then ulimit -c \"$n\" && [ \"$(ulimit -c)\" = \"$n\" ] || exit 99; \
             echo \"raised $n\" > \"{}\"; else echo \"hard 0\" > \"{}\"; fi; \
             exec \"{BIN}\" run --work \"{w}\"",
            raised.display(),
            raised.display()
        );
        let mut sh = Command::new("/bin/sh");
        sh.args(["-c", &line])
            .env("HOME", &e.home)
            .env("CODEX_HOME", e.home.join(".codex"))
            .env("U6_HD_TEST_NOTES", &notes_file);
        let o = output_within(&mut sh, 30, name);
        assert!(o.status.success(), "{name}: {:?} {}", o.status, text(&o));
        let how = std::fs::read_to_string(&raised).unwrap();
        if name == "hardened" {
            assert_eq!(how.trim(), "hard 0", "{name}");
        }
        let limits: Vec<String> = notes(&notes_file)
            .into_iter()
            .filter(|(w, _)| w.starts_with("core-limit"))
            .map(|(w, _)| w)
            .collect();
        assert_eq!(limits, ["core-limit 0 0"], "{name} ({})", how.trim());
        let _ = std::fs::remove_dir_all(&e.dir);
    }
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

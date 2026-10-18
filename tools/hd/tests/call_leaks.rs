//! A codex call leaves nothing of itself in the tool's process, and ends in its bound, whatever
//! the call does with its pipes: calls whose helpers leave the call's process group (sessions
//! of their own) and keep the call's output, or its input and output, open past the call, or
//! write to its output without pause, none of them killed by the test. Each call runs under a
//! watchdog (a call that never returns fails the test, not hangs it) and ends in its bound;
//! afterwards the process has as many threads and open descriptors as before the first. One
//! test in its binary, so nothing else moves the counts.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use u6_hdtool::run::{CLEANUP, Codex, Runner};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("u6-hd-leaks-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// Threads of this process.
fn threads() -> usize {
    if let Ok(d) = std::fs::read_dir("/proc/self/task") {
        return d.count();
    }
    let out = Command::new("/bin/ps")
        .args(["-M", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).lines().count() - 1
}

/// Open descriptors of this process (the listing's own included, the same each time).
fn fds() -> usize {
    std::fs::read_dir("/dev/fd").unwrap().count()
}

/// The helpers' mark: they live while it exists. Removed on drop, when the test unwinds too,
/// so a failed assertion leaves no helper running.
struct Mark(PathBuf);

impl Drop for Mark {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A stand-in for `codex`: `before`, then `n` helpers, each in a session of its own and keeping
/// every descriptor it was given (the call's input and output), running `perl` while `mark`
/// exists, then `after`. Each makes its session before its fork, so it is out of the call's
/// group before the stand-in can exit and the group be killed (made after it, a keeper, which
/// does not acknowledge, could die with the group if the loaded machine had not yet run it).
fn stub(
    dir: &Path,
    name: &str,
    before: &str,
    n: usize,
    perl: &str,
    mark: &Path,
    after: &str,
) -> PathBuf {
    let p = dir.join(name);
    // `$ARGV[1]` is the helper's start acknowledgment: its pid is appended to make a file name.
    let helper = format!(
        "/usr/bin/perl -MPOSIX -e 'POSIX::setsid() or die \"setsid: $!\"; exit if fork; \
         {perl} while -e $ARGV[0]' \"{}\" \"{}\"\n",
        mark.display(),
        dir.join(format!("{name}.ack")).display()
    );
    // Helpers that acknowledge (the writers): the stand-in goes on to `after` once every one
    // has, so that one slow to start on a loaded machine does not begin after the call ended
    // and never write (the test could not tell it from one that cannot). What bounds the wait:
    // the call's own deadline first (it kills the stand-in's group; the writers, in sessions
    // of their own, go on), else 1000 tries (each forks ls and wc and sleeps 10 ms: 10 s at
    // the least, more under load), and in any case the test's 30 s watchdog over the call.
    let wait = if perl.contains("$ARGV[1]") {
        format!(
            "i=0; while [ \"$(ls \"{}\".* 2>/dev/null | wc -l)\" -lt {n} ] && [ $i -lt 1000 ]; \
             do sleep 0.01; i=$((i + 1)); done\n",
            dir.join(format!("{name}.ack")).display()
        )
    } else {
        String::new()
    };
    let body = format!("#!/bin/sh\n{before}\n{}{wait}{after}\n", helper.repeat(n));
    std::fs::write(&p, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// `c.generate` on a thread of its own, waited for at most `watchdog`: the error it returned
/// and the time it took.
fn watched(c: Codex, t: &Path, prompt: &str, watchdog: Duration, name: &str) -> (String, Duration) {
    let (sheet, dir, prompt) = (t.join("sheet.png"), t.join("dir"), prompt.to_string());
    let (tx, rx) = mpsc::channel();
    let call = std::thread::spawn(move || {
        let start = Instant::now();
        let r = c.generate(&sheet, &prompt, &dir);
        let _ = tx.send((r, start.elapsed()));
    });
    let Ok((r, took)) = rx.recv_timeout(watchdog) else {
        panic!("watchdog: {name}: the call did not return in {watchdog:?}");
    };
    call.join().unwrap();
    (r.unwrap_err(), took)
}

#[test]
fn a_call_leaves_no_thread_and_no_descriptor() {
    let t = temp("calls");
    let (keep, flood) = (Mark(t.join("keepers-live")), Mark(t.join("flooders-live")));
    std::fs::write(&keep.0, b"").unwrap();
    std::fs::write(&flood.0, b"").unwrap();
    std::fs::create_dir_all(t.join("dir")).unwrap();
    std::fs::write(t.join("sheet.png"), b"x").unwrap();
    let thread = r#"echo '{"type":"thread.started","thread_id":"t1"}'"#;
    let takes = format!("cat > /dev/null\n{thread}");
    let idle = "select(undef, undef, undef, 0.2)";
    // Four writers of 64 KiB at a time keep the output full most of the time (the reader still
    // finds it empty between their writes, and ends a turn there). Each acknowledges its first
    // write that went in (a file `<stub>.ack.<pid>`), so it is known to be writing.
    let spew = r#"do { my $w = syswrite(STDOUT, "y\n" x 32768); if ($w && !$n++) { open(my $f, ">", "$ARGV[1].$$"); close $f } }"#;
    // (name, stub, its prompt is never read, deadline in seconds). The flooders' stand-ins
    // wait for their four writers' acknowledgments, which took 2.75 s at a load of 118
    // (measured by the review): their deadlines, 12 s for the one that exits (its call ends
    // when it exits, so this costs nothing unloaded) and 8 s for the one that hangs (its call
    // runs to it), are meant to fall after that at a load of 200, about 4.7 s if it grows with
    // the load, with room; a writer that starts later still is reported as one that did not
    // write. The keepers do not acknowledge. The one that exits gets 12 s too: its call took
    // 1.97 s in round 0 at a load of 84, its 1 s grace and so up to 0.97 s to its stand-in's
    // exit (0.41-0.59 s in seven more at 58-86), which a 1 s deadline nearly cut off as "timed
    // out". The one that hangs gets 4 s, 7-10x those (10x the 0.97 s would cost 27 s more
    // a run: its deadline is its run time, three rounds), so that its keeper has started, and
    // left the group, before the deadline kills the group: one that had not would be missing
    // from the six counted at the end.
    let shapes = [
        // Keeps the output: takes the prompt, exits; the stream never ends.
        (
            "keeps-out.sh",
            stub(&t, "keeps-out.sh", &takes, 1, idle, &keep.0, "exit 0"),
            false,
            12,
        ),
        // Keeps the input unread and the output: the prompt never goes in; hangs.
        (
            "keeps-both.sh",
            stub(&t, "keeps-both.sh", thread, 1, idle, &keep.0, "sleep 30"),
            true,
            4,
        ),
        // Writes to the output without pause after the call exits, or while it hangs.
        (
            "floods-exit.sh",
            stub(&t, "floods-exit.sh", &takes, 4, spew, &flood.0, "exit 0"),
            false,
            12,
        ),
        (
            "floods-hang.sh",
            stub(&t, "floods-hang.sh", &takes, 4, spew, &flood.0, "sleep 30"),
            false,
            8,
        ),
    ];
    let big = "x".repeat(1 << 20);
    let (threads0, fds0) = (threads(), fds());
    for round in 0..3 {
        for (name, bin, unread, secs) in &shapes {
            if round > 0 && name.starts_with("floods") {
                continue;
            }
            let c = Codex {
                bin: bin.clone(),
                home: t.join("home"),
                timeout: Duration::from_secs(*secs),
                grace: CLEANUP,
            };
            let hangs = *unread || name.ends_with("hang.sh");
            let prompt = if *unread { big.as_str() } else { "PROMPT" };
            let (e, took) = watched(c, &t, prompt, Duration::from_secs(30), name);
            let want = if hangs {
                "timed out"
            } else {
                "the call's output did not end"
            };
            println!("{name} round {round}: {took:?}: {e}");
            assert!(e.contains(want), "{name} round {round}: {e}");
            if name.starts_with("floods") {
                // All four writers were writing: each acknowledged a write that went in.
                let acks = std::fs::read_dir(&t)
                    .unwrap()
                    .filter(|e| {
                        let n = e.as_ref().unwrap().file_name();
                        n.to_string_lossy().starts_with(&format!("{name}.ack."))
                    })
                    .count();
                let events = std::fs::metadata(t.join("dir/events.jsonl")).unwrap().len();
                println!("{name}: {acks} writers acknowledged, {events} bytes of events kept");
                assert_eq!(acks, 4, "{name}: writers that wrote");
                assert!(events > 1 << 16, "{name}: {events} bytes read");
            }
            // Within the deadline and the 1 s grace, with 2 s of slack for a loaded machine.
            let limit = Duration::from_secs(secs + 1 + 2);
            assert!(took < limit, "{name} round {round}: {took:?} ({e})");
        }
    }
    let (threads1, fds1) = (threads(), fds());
    println!("threads {threads0} -> {threads1}, descriptors {fds0} -> {fds1}");
    assert_eq!(threads1, threads0, "threads left behind");
    assert_eq!(fds1, fds0, "descriptors left open");
    // Positive control: the six keepers are still there, holding their ends (measured with
    // them alive); they go with their mark.
    let alive = Command::new("/usr/bin/pgrep")
        .args(["-f", keep.0.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&alive.stdout).lines().count(), 6);
    drop((keep, flood));
    std::thread::sleep(Duration::from_millis(500));
    let _ = std::fs::remove_dir_all(&t);
}

//! A codex call leaves nothing of itself in the tool's process, whatever the call does with its
//! pipes: calls whose helper leaves the call's process group (its own session) and keeps the
//! call's output, or its input and output, open past the call, never to exit while measured.
//! Each call ends in its bound, and afterwards the process has as many threads and open
//! descriptors as before the first. One test in its binary, so nothing else moves the counts.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use u6_hdtool::run::{Codex, Runner};

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

/// A stand-in for `codex`: `before`, then a helper in a session of its own that keeps every
/// descriptor it was given (the call's input and output) until `mark` is removed, then `after`.
fn stub(dir: &Path, name: &str, mark: &Path, before: &str, after: &str) -> PathBuf {
    let p = dir.join(name);
    let body = format!(
        "#!/bin/sh\n{before}\n/usr/bin/perl -MPOSIX -e 'exit if fork; POSIX::setsid(); \
         select(undef, undef, undef, 0.2) while -e $ARGV[0]' \"{}\"\n{after}\n",
        mark.display()
    );
    std::fs::write(&p, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn a_call_leaves_no_thread_and_no_descriptor() {
    let t = temp("calls");
    let mark = t.join("helpers-live");
    std::fs::write(&mark, b"").unwrap();
    let dir = t.join("dir");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(t.join("sheet.png"), b"x").unwrap();
    let thread = r#"echo '{"type":"thread.started","thread_id":"t1"}'"#;
    // Keeps the output: takes the prompt, exits; the stream never ends.
    let out = stub(
        &t,
        "keeps-out.sh",
        &mark,
        &format!("cat > /dev/null\n{thread}"),
        "exit 0",
    );
    // Keeps the input unread and the output: the prompt never goes in; hangs to the deadline.
    let both = stub(&t, "keeps-both.sh", &mark, thread, "sleep 30");
    let big = "x".repeat(1 << 20);
    let (threads0, fds0) = (threads(), fds());
    for round in 0..3 {
        for (bin, prompt, want, limit) in [
            (&out, "PROMPT", "the call's output did not end", 3),
            (&both, big.as_str(), "timed out", 4),
        ] {
            let c = Codex {
                bin: bin.clone(),
                home: t.join("home"),
                timeout: Duration::from_secs(1),
            };
            let start = Instant::now();
            let e = c.generate(&t.join("sheet.png"), prompt, &dir).unwrap_err();
            let took = start.elapsed();
            assert!(e.contains(want), "round {round}: {e}");
            assert!(
                took < Duration::from_secs(limit),
                "round {round}: {took:?} ({e})"
            );
        }
    }
    let (threads1, fds1) = (threads(), fds());
    println!("threads {threads0} -> {threads1}, descriptors {fds0} -> {fds1}");
    assert_eq!(threads1, threads0, "threads left behind");
    assert_eq!(fds1, fds0, "descriptors left open");
    // Positive control: the six helpers are still there, holding their ends (measured with
    // them alive); they go once the mark does.
    let alive = Command::new("/usr/bin/pgrep")
        .args(["-f", mark.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&alive.stdout).lines().count(), 6);
    std::fs::remove_file(&mark).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let _ = std::fs::remove_dir_all(&t);
}

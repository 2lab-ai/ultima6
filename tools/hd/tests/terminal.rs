//! A terminal closed under an interactive zsh, the tool its foreground job: in a test binary
//! of its own, because the pty's master descriptor (which forkpty does not open close-on-exec)
//! would be held open by any process another test spawned meanwhile, and the terminal would
//! never close.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_u6-hd");

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("u6-hd-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
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
        use std::os::unix::process::CommandExt;
        let mut c = Command::new(BIN);
        // SAFETY: only signal(2) in the child before exec: the stop signals at their default.
        unsafe {
            c.pre_exec(|| {
                for s in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
                    libc::signal(s, libc::SIG_DFL);
                }
                Ok(())
            });
        }
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
}

/// Whether process `pid` is gone: not even a zombie (reaped, by its parent or by launchd once
/// orphaned; callers poll under their own deadlines, assuming each `ps` returns).
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

/// The pids in `file` (one per line; none if it is missing).
fn pids(file: &Path) -> Vec<String> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
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

/// An interactive zsh on a pty, the tool its foreground job with four calls running and its
/// stderr the terminal, the terminal closed: the tool ends by SIGHUP (not 101, from a print
/// that panicked on the gone terminal) and every call is reaped by the tool first. (It does
/// not show how many SIGHUPs came; repeated delivery is the cleanup-held test's, in cli.rs.)
#[test]
fn closing_the_terminal_under_zsh_stops_every_call() {
    use std::ffi::CString;
    use std::io::Read;
    use std::os::fd::FromRawFd;
    if !Path::new("/bin/zsh").exists() {
        return;
    }
    for rep in 0..3 {
        let what = format!("zsh close {rep}");
        let e = Env::new(&format!("zsh-{rep}"));
        let work = e.dir.join("work");
        let w = work.to_str().unwrap();
        let (stub_pids, leaders, status_file, notes_file) = (
            e.dir.join("pids"),
            e.dir.join("leaders"),
            e.dir.join("status"),
            e.dir.join("notes"),
        );
        assert!(
            e.run(&["atlas", "--synthetic", "--work", w])
                .status
                .success()
        );
        let bin = family_stub(&e.dir);
        let _stub_guard = scopeguard_pids(&stub_pids);
        let env: Vec<CString> = [
            "PATH=/usr/bin:/bin".to_string(),
            format!("HOME={}", e.home.display()),
            format!("CODEX_HOME={}", e.home.join(".codex").display()),
            format!("STUB_PIDS={}", stub_pids.display()),
            format!("STUB_LEADERS={}", leaders.display()),
            format!("U6_HD_TEST_NOTES={}", notes_file.display()),
            "PS1=$ ".to_string(),
            "TERM=dumb".to_string(),
        ]
        .into_iter()
        .map(|v| CString::new(v).unwrap())
        .collect();
        let argv: Vec<CString> = ["/bin/zsh", "-f", "-i"]
            .iter()
            .map(|a| CString::new(*a).unwrap())
            .collect();
        let mut argv_p: Vec<*const libc::c_char> = argv.iter().map(|a| a.as_ptr()).collect();
        argv_p.push(std::ptr::null());
        let mut env_p: Vec<*const libc::c_char> = env.iter().map(|a| a.as_ptr()).collect();
        env_p.push(std::ptr::null());
        let mut master: libc::c_int = -1;
        // SAFETY: forkpty(3); the child only calls signal(2), execve(2) with prepared arrays,
        // and _exit, all async-signal-safe.
        let zsh = unsafe {
            libc::forkpty(
                &mut master,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert!(zsh >= 0);
        if zsh == 0 {
            // SAFETY: in the child: signal(2) (the stop signals at their default, whatever this
            // test inherited, as for every tool the tests start), execve, then _exit if it
            // failed; all async-signal-safe.
            unsafe {
                for s in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
                    libc::signal(s, libc::SIG_DFL);
                }
                libc::execve(argv_p[0], argv_p.as_ptr(), env_p.as_ptr());
                libc::_exit(127);
            }
        }
        let _zsh_guard = KillOnDrop(zsh.to_string());
        // SAFETY: fcntl on the master descriptor forkpty gave us: non-blocking, and closed on
        // exec (nothing this test spawns later keeps the terminal open).
        unsafe {
            let fl = libc::fcntl(master, libc::F_GETFL);
            libc::fcntl(master, libc::F_SETFL, fl | libc::O_NONBLOCK);
            libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        // SAFETY: the master descriptor is ours alone; File closes it on drop.
        let mut term = unsafe { std::fs::File::from_raw_fd(master) };
        let drain = |term: &mut std::fs::File, ms: u64| {
            let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
            let mut buf = [0u8; 4096];
            while std::time::Instant::now() < end {
                match term.read(&mut buf) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
                }
            }
        };
        drain(&mut term, 500);
        // The job: a shell that outlives the hang-up (its own handler, which the tool does not
        // inherit) to write the tool's exit status, and the tool, its stderr the terminal.
        let line = format!(
            "sh -c 'trap : HUP; \"{BIN}\" run --work \"{w}\" --codex \"{}\" --send \
             --parallel 4; echo $? > \"{}\"'\n",
            bin.display(),
            status_file.display()
        );
        std::io::Write::write_all(&mut term, line.as_bytes()).unwrap();
        let t = std::time::Instant::now();
        while pids(&leaders).len() < 4 {
            assert!(
                t.elapsed().as_secs() < 30,
                "{what}: the calls never started"
            );
            drain(&mut term, 20);
        }
        drain(&mut term, 300);
        let o = Command::new("/usr/bin/pgrep")
            .args(["-f", &format!("u6-hd run --work {w}")])
            .output()
            .unwrap();
        let tools: Vec<String> = String::from_utf8_lossy(&o.stdout)
            .split_whitespace()
            .map(str::to_string)
            .collect();
        assert_eq!(tools.len(), 1, "{what}: {tools:?}");
        // The terminal goes away.
        drop(term);
        let t = std::time::Instant::now();
        while !gone(&tools[0]) {
            assert!(
                t.elapsed().as_secs() < 10,
                "{what}: the tool outlived the terminal"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        all_gone(&stub_pids, &what);
        // Ended by SIGHUP (128 + 1), not 101 from a print that panicked on the gone terminal.
        let t = std::time::Instant::now();
        while !status_file.exists() && t.elapsed().as_secs() < 10 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let code = std::fs::read_to_string(&status_file).unwrap_or_default();
        assert_eq!(code.trim(), "129", "{what}");
        // Test builds: the tool reaped exactly the stand-ins, each once.
        if cfg!(debug_assertions) {
            let mut reaped: Vec<String> = std::fs::read_to_string(&notes_file)
                .unwrap_or_default()
                .lines()
                .filter_map(|l| l.strip_prefix("reaped "))
                .filter_map(|l| l.split(' ').next())
                .map(str::to_string)
                .collect();
            reaped.sort();
            let mut want = pids(&leaders);
            want.sort();
            assert_eq!(reaped, want, "{what}");
        }
        let mut status = 0;
        for _ in 0..200 {
            // SAFETY: waitpid(2) on the zsh this test forked.
            if unsafe { libc::waitpid(zsh, &mut status, libc::WNOHANG) } == zsh {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = std::fs::remove_dir_all(&e.dir);
    }
}

//! `run`: one imagegen call per sheet not yet done, 2–4 at a time, through Codex's built-in
//! `image_gen` tool (`codex exec -m gpt-6-astra`, the imagegen skill's default path: the
//! ChatGPT account Codex is signed in to, no API key, never the CLI fallback). The built-in
//! tool saves under `$CODEX_HOME/generated_images/<thread id>/`; the runner moves the image to
//! `out/<sheet>.png` with a [`Receipt`] (the thread's folder is removed). Nothing is sent without
//! `--send`. One command at a time uses a work dir ([`crate::work::Work::lock`]).
//!
//! A sheet is done when its receipt names the job's [`identity`] (the atlas generation, the
//! sheet, the attempt, the prompt text and the sheet's bytes) and the output's digest, and the
//! output still has that digest and the size the receipt records. Anything else — no receipt,
//! another atlas generation, a changed prompt or sheet, a changed or truncated output — is not
//! done, and is run again. A call whose image is not a PNG, or is of the sheet's size and does
//! not decode, fails, records nothing and leaves the image where the call put it; an image of
//! another size is recorded undecoded and ingest refuses it ([`record`]). A job whose calls
//! failed [`CALLS_MAX`] times (timeouts, no image) is given up: not called again, refused by
//! ingest. Either way its items are re-rolled once, then left to the scaler.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::atlas::Job;
use crate::img;
use crate::prompt;
use crate::work::{self, Work};

/// What makes an image from a sheet and a prompt.
pub trait Runner: Sync {
    /// Runs one call with `sheet` attached and `prompt`, `dir` as its working dir; returns the
    /// generated image's path.
    fn generate(&self, sheet: &Path, prompt: &str, dir: &Path) -> Result<PathBuf, String>;

    /// After the image at `image` was recorded (output and receipt written): removes what the
    /// call left of it.
    fn finish(&self, image: &Path) {
        let _ = image;
    }
}

/// The model of every call.
pub const MODEL: &str = "gpt-6-astra";
/// Most bytes of codex events kept per call (the rest are read and dropped).
pub const EVENTS_MAX: u64 = 4 << 20;
/// Most bytes of codex's stderr kept per call (the rest are read and dropped).
pub const STDERR_MAX: u64 = 1 << 20;

/// `codex exec` with the built-in image tool.
pub struct Codex {
    pub bin: PathBuf,
    /// `$CODEX_HOME` (default `~/.codex`).
    pub home: PathBuf,
    /// One deadline for the call: the process group, its prompt and its output.
    pub timeout: Duration,
}

impl Codex {
    pub fn from_env(bin: Option<PathBuf>) -> Result<Codex, String> {
        let home = match std::env::var_os("CODEX_HOME") {
            Some(h) => PathBuf::from(h),
            None => {
                let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
                Path::new(&home).join(".codex")
            }
        };
        Ok(Codex {
            bin: bin.unwrap_or_else(|| "codex".into()),
            home,
            timeout: Duration::from_secs(15 * 60),
        })
    }
}

/// Most bytes of one codex event line; a longer line is read and dropped whole.
pub const LINE_MAX: usize = 64 << 10;
/// How long the event stream is given, once the call's group is gone, to end.
const CLEANUP: Duration = Duration::from_secs(1);

/// Kills every process of the group `pgid` (the call and anything it started).
fn kill_group(pgid: u32) {
    if let Ok(p) = i32::try_from(pgid)
        && p > 0
    {
        // SAFETY: kill(2) with a negative pid signals that process group; no memory involved.
        unsafe { libc::kill(-p, libc::SIGKILL) };
    }
}

/// The process groups of the calls running now (0 = a free slot; `run` makes at most 4 at
/// once), for [`on_signal`], which may read only what is safe in a signal handler.
static LIVE: [std::sync::atomic::AtomicI32; 16] =
    [const { std::sync::atomic::AtomicI32::new(0) }; 16];

/// Calls being spawned now, whose groups are not in [`LIVE`] yet. A spawning thread blocks
/// the stop signals from before its spawn until its group is in the table, so [`on_signal`]
/// runs on another thread and waits for this to be 0 before it kills the groups.
static SPAWNING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The signals that stop a command with its calls.
const STOP: [libc::c_int; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];

/// SIGINT, SIGTERM or SIGHUP: once no call is between its spawn and its slot in [`LIVE`]
/// (waited for at most about 2 s), every running call's group is killed, then the signal is
/// raised again with its default action, so the tool ends as it would have, its lock freed
/// only after its calls are gone. (Atomics, poll, kill, signal and raise only:
/// async-signal-safe.)
extern "C" fn on_signal(sig: libc::c_int) {
    for _ in 0..2000 {
        if SPAWNING.load(Ordering::SeqCst) == 0 {
            break;
        }
        // SAFETY: poll(2) with no descriptors is a 1 ms sleep, async-signal-safe.
        unsafe { libc::poll(std::ptr::null_mut(), 0, 1) };
    }
    for slot in &LIVE {
        let p = slot.load(Ordering::SeqCst);
        if p > 0 {
            // SAFETY: kill(2) is async-signal-safe.
            unsafe { libc::kill(-p, libc::SIGKILL) };
        }
    }
    // SAFETY: signal(2) and raise(3) are async-signal-safe; the signal stays blocked until
    // this handler returns, and then ends the process with its default action.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

/// Installs [`on_signal`] for SIGINT, SIGTERM and SIGHUP (the terminal's Ctrl-C, a `kill`, a
/// closed terminal): a command stopped so takes its codex calls with it, which run in process
/// groups of their own that those signals do not reach. A signal that comes while a call is
/// being spawned waits for its group to be in the table ([`SPAWNING`]).
pub fn stop_calls_on_signals() {
    for sig in STOP {
        // SAFETY: a zeroed sigaction with only the handler and an empty mask set is valid; the
        // handler is an extern "C" fn of the expected signature.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(sig, &action, std::ptr::null_mut());
        }
    }
}

/// The stop signals blocked in this thread while held (a call is being spawned and
/// registered); the thread's earlier mask restored on drop, when a signal held meanwhile is
/// delivered.
struct Blocked {
    old: libc::sigset_t,
}

impl Blocked {
    fn new() -> Blocked {
        // SAFETY: sigset_t is plain data, filled by sigemptyset/sigaddset and pthread_sigmask.
        unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            for sig in STOP {
                libc::sigaddset(&mut set, sig);
            }
            let mut old: libc::sigset_t = std::mem::zeroed();
            libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut old);
            Blocked { old }
        }
    }
}

impl Drop for Blocked {
    fn drop(&mut self) {
        // SAFETY: restores the mask this thread had, saved by Blocked::new.
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &self.old, std::ptr::null_mut()) };
    }
}

/// Counted in [`SPAWNING`] while held.
struct Spawning;

impl Spawning {
    fn new() -> Spawning {
        SPAWNING.fetch_add(1, Ordering::SeqCst);
        Spawning
    }
}

impl Drop for Spawning {
    fn drop(&mut self) {
        SPAWNING.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Test builds only (`debug_assertions`, which the gate profile keeps): with
/// `U6_HD_TEST_SIGNAL_IN_SPAWN` set to a signal number, the first call's spawn, between the
/// spawn and the group's registration, waits (at most 10 s) for the file
/// `U6_HD_TEST_SPAWN_WAIT` names (the stand-in's sign that it runs), sends that signal to the
/// tool (or, with `U6_HD_TEST_SIGNAL_TO=thread`, to this very thread, which holds it blocked),
/// then waits 200 ms: the signal comes while the group is not in the table yet.
#[cfg(debug_assertions)]
fn signal_in_spawn_for_tests() {
    static FIRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let Some(sig) = std::env::var("U6_HD_TEST_SIGNAL_IN_SPAWN")
        .ok()
        .and_then(|v| v.parse::<libc::c_int>().ok())
    else {
        return;
    };
    if FIRED.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(wait) = std::env::var_os("U6_HD_TEST_SPAWN_WAIT") {
        let start = Instant::now();
        while !Path::new(&wait).exists() && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if std::env::var("U6_HD_TEST_SIGNAL_TO").as_deref() == Ok("thread") {
        // SAFETY: pthread_kill(3) to this thread.
        unsafe { libc::pthread_kill(libc::pthread_self(), sig) };
    } else {
        // SAFETY: kill(2) to this process.
        unsafe { libc::kill(libc::getpid(), sig) };
    }
    std::thread::sleep(Duration::from_millis(200));
}

/// A spawned call: its group in [`LIVE`] while it runs; on drop, on every way out of
/// [`Codex::generate`], its whole process group is killed, the call reaped, its slot freed.
struct Group {
    child: std::process::Child,
    slot: Option<usize>,
}

impl Group {
    fn new(child: std::process::Child) -> Group {
        let pgid = i32::try_from(child.id()).unwrap_or(0);
        let slot = LIVE.iter().position(|s| {
            s.compare_exchange(0, pgid, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        });
        Group { child, slot }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        kill_group(self.child.id());
        let _ = self.child.wait();
        if let Some(i) = self.slot {
            LIVE[i].store(0, Ordering::SeqCst);
        }
    }
}

/// Codex's event stream, line by line as its bytes arrive: each line of at most [`LINE_MAX`]
/// bytes goes to `events` while they hold under [`EVENTS_MAX`] bytes, and the first
/// `thread.started` event's id is kept. A longer line is dropped up to its newline without
/// being held, so memory stays bounded whatever the stream holds. The first error writing to
/// `events` is kept, and no line is written after it ([`Ended::error`]).
pub struct EventLines<W: Write> {
    events: W,
    kept: u64,
    thread: Option<String>,
    error: Option<std::io::Error>,
    line: Vec<u8>,
    dropping: bool,
}

/// What an event stream came to: its first thread id, and the first error writing its lines
/// to the event file or flushing them there, if there was one.
#[derive(Debug)]
pub struct Ended {
    pub thread: Option<String>,
    pub error: Option<std::io::Error>,
}

impl<W: Write> EventLines<W> {
    pub fn new(events: W) -> EventLines<W> {
        EventLines {
            events,
            kept: 0,
            thread: None,
            error: None,
            line: Vec::new(),
            dropping: false,
        }
    }

    fn on_line(&mut self, line: &[u8]) {
        if self.thread.is_none()
            && let Ok(v) = serde_json::from_slice::<serde_json::Value>(line)
            && v["type"] == "thread.started"
            && let Some(id) = v["thread_id"].as_str()
        {
            self.thread = Some(id.to_string());
        }
        if self.error.is_none() && self.kept + (line.len() as u64) < EVENTS_MAX {
            match self
                .events
                .write_all(line)
                .and_then(|()| self.events.write_all(b"\n"))
            {
                Ok(()) => self.kept += line.len() as u64 + 1,
                Err(e) => self.error = Some(e),
            }
        }
    }

    /// The next bytes of the stream.
    pub fn feed(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let (part, ends) = match bytes.iter().position(|&b| b == b'\n') {
                Some(i) => (&bytes[..i], true),
                None => (bytes, false),
            };
            if !self.dropping && self.line.len() + part.len() <= LINE_MAX {
                self.line.extend_from_slice(part);
            } else {
                self.dropping = true;
                self.line = Vec::new();
            }
            if ends {
                if !self.dropping {
                    let line = std::mem::take(&mut self.line);
                    self.on_line(&line);
                    self.line = line;
                }
                self.line.clear();
                self.dropping = false;
                bytes = &bytes[part.len() + 1..];
            } else {
                bytes = &[];
            }
        }
    }

    /// The end of the stream: its last line (without a newline), the lines flushed to
    /// `events`; the first thread id and the first write or flush error.
    pub fn finish(mut self) -> Ended {
        if !self.dropping && !self.line.is_empty() {
            let line = std::mem::take(&mut self.line);
            self.on_line(&line);
        }
        if self.error.is_none()
            && let Err(e) = self.events.flush()
        {
            self.error = Some(e);
        }
        Ended {
            thread: self.thread,
            error: self.error,
        }
    }
}

/// Reads codex's event stream `r` to its end in 8 KiB chunks ([`EventLines`]).
pub fn read_events(mut r: impl Read, events: impl Write) -> Ended {
    let mut lines = EventLines::new(events);
    let mut chunk = [0u8; 8192];
    loop {
        match r.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => lines.feed(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    lines.finish()
}

/// `fd` switched to non-blocking (our end of a pipe to the call; the call's end is its own).
fn nonblocking(fd: i32) -> Result<(), String> {
    // SAFETY: fcntl on a descriptor this function's caller owns, with no pointer arguments.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(format!(
            "a pipe to the call: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Most bytes, and most system calls, one turn of [`Pipes::step`] moves in each direction:
/// a pipe that is never found empty (or never full) still hands the caller back its turn.
const TURN_BYTES: usize = 64 << 10;
const TURN_CALLS: usize = 8;

/// The call's three pipes, driven from the calling thread: the prompt written to its input,
/// its events and its stderr read (each to its file, under its cap), as each is ready.
/// Closing a pipe is dropping it here.
struct Pipes<'a, W: Write, E: Write> {
    stdin: Option<std::process::ChildStdin>,
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
    prompt: &'a [u8],
    written: usize,
    /// The prompt's write failed: the call's input was closed before it took it all.
    refused: Option<std::io::Error>,
    lines: EventLines<W>,
    errs: Capped<E>,
}

/// A file that keeps the first `max` bytes written to it and drops the rest, and the first
/// error writing them.
struct Capped<E: Write> {
    out: E,
    kept: u64,
    max: u64,
    error: Option<std::io::Error>,
}

impl<E: Write> Capped<E> {
    fn put(&mut self, bytes: &[u8]) {
        let room = self.max.saturating_sub(self.kept) as usize;
        let take = &bytes[..bytes.len().min(room)];
        if self.error.is_some() || take.is_empty() {
            return;
        }
        match self.out.write_all(take) {
            Ok(()) => self.kept += take.len() as u64,
            Err(e) => self.error = Some(e),
        }
    }

    /// Flushed: the first error writing or flushing.
    fn finish(mut self) -> Option<std::io::Error> {
        if self.error.is_none()
            && let Err(e) = self.out.flush()
        {
            self.error = Some(e);
        }
        self.error
    }
}

/// Reads what `pipe` has, at most a turn's worth ([`TURN_BYTES`], [`TURN_CALLS`]) and nothing
/// once `until` has passed, handing each chunk to `sink`; closes it at its end.
fn drain<R: Read>(pipe: &mut Option<R>, until: Instant, mut sink: impl FnMut(&[u8])) {
    let Some(p) = pipe.as_mut() else { return };
    let mut chunk = [0u8; 8192];
    let (mut moved, mut end) = (0, false);
    for _ in 0..TURN_CALLS {
        if moved >= TURN_BYTES || Instant::now() >= until {
            break;
        }
        match p.read(&mut chunk) {
            Ok(0) => {
                end = true;
                break;
            }
            Ok(n) => {
                moved += n;
                sink(&chunk[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => {
                end = true;
                break;
            }
        }
    }
    if end {
        *pipe = None;
    }
}

impl<W: Write, E: Write> Pipes<'_, W, E> {
    /// One turn: waits until either pipe is ready or `until` passes, then writes and reads what
    /// it can, at most [`TURN_BYTES`] and [`TURN_CALLS`] each way, starting no read or write
    /// once `until` has passed, so the caller's checks (the call's exit, the deadline, the
    /// grace) run between turns whatever the call does with its pipes. The work between checks
    /// is bounded on the assumption that the event file's writes return: they are synchronous
    /// (buffered, a turn writes at most what it read), so a stalled disk stalls the turn.
    fn step(&mut self, until: Instant) {
        use std::os::fd::AsRawFd;
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        let mut fds: Vec<libc::pollfd> = Vec::with_capacity(2);
        if let Some(i) = &self.stdin {
            fds.push(libc::pollfd {
                fd: i.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            });
        }
        for fd in [
            self.stdout.as_ref().map(|o| o.as_raw_fd()),
            self.stderr.as_ref().map(|e| e.as_raw_fd()),
        ]
        .into_iter()
        .flatten()
        {
            fds.push(libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
        }
        if fds.is_empty() {
            std::thread::sleep(left);
            return;
        }
        // Rounded up, so a wait of under a millisecond still waits.
        let ms = i32::try_from(left.as_micros().div_ceil(1000)).unwrap_or(i32::MAX);
        // SAFETY: `fds` is a live array of `fds.len()` pollfd records for open descriptors.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };
        if n <= 0 {
            return;
        }
        let ready = |fd: i32| fds.iter().any(|p| p.fd == fd && p.revents != 0);
        if let Some(i) = &mut self.stdin
            && ready(i.as_raw_fd())
        {
            let mut moved = 0;
            for _ in 0..TURN_CALLS {
                if moved >= TURN_BYTES || Instant::now() >= until {
                    break;
                }
                let end = self.prompt.len().min(self.written + (TURN_BYTES - moved));
                match i.write(&self.prompt[self.written..end]) {
                    Ok(n) => {
                        self.written += n;
                        moved += n;
                        if self.written == self.prompt.len() {
                            // Taken whole: the call's input ends.
                            self.stdin = None;
                            break;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        self.refused = Some(e);
                        self.stdin = None;
                        break;
                    }
                }
            }
        }
        if self.stdout.as_ref().is_some_and(|o| ready(o.as_raw_fd())) {
            let lines = &mut self.lines;
            drain(&mut self.stdout, until, |b| lines.feed(b));
        }
        if self.stderr.as_ref().is_some_and(|e| ready(e.as_raw_fd())) {
            let errs = &mut self.errs;
            drain(&mut self.stderr, until, |b| errs.put(b));
        }
    }
}

/// How a call ended, its process cleaned up and its files put in place.
pub(crate) struct Settled {
    /// The call's exit status, or why it was stopped (timed out, the prompt refused).
    pub(crate) waited: Result<std::process::ExitStatus, String>,
    /// The prompt went in whole.
    pub(crate) taken: bool,
    pub(crate) refused: Option<std::io::Error>,
    /// The event stream ended.
    pub(crate) ended: bool,
    /// The first failure saving the call's events or output.
    pub(crate) saved: Option<String>,
}

/// The call's thread id, or why the call failed, in this precedence: how it was stopped, its
/// prompt not taken, its stream not ended, its exit status, then a failure saving what it
/// printed (the call itself having gone right), then the thread id missing or malformed.
pub(crate) fn settle(s: Settled, thread: Option<String>) -> Result<String, String> {
    let status = s.waited?;
    if !s.taken {
        return Err(match s.refused {
            Some(e) => format!("the call did not take its prompt: {e}"),
            None => "the call did not take its prompt".into(),
        });
    }
    if !s.ended {
        return Err("the call's output did not end".into());
    }
    if !status.success() {
        return Err(format!("codex exited with {status}"));
    }
    if let Some(e) = s.saved {
        return Err(e);
    }
    let id = thread.ok_or("no thread id in the codex events")?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("an unexpected thread id in the codex events".into());
    }
    Ok(id)
}

impl Runner for Codex {
    /// One deadline covers the call, all of it in this thread: the prompt is written and the
    /// events read as the pipes are ready (both non-blocking, one `poll` for the two), while
    /// the call is watched for its exit, until it exits, the prompt fails or the deadline
    /// passes; then the call's group is killed (on every path) and the stream given [`CLEANUP`]
    /// to end (each wait bounded as [`Pipes::step`] says: the event file's writes are assumed
    /// to return). Then both pipes are closed, whatever still holds their other ends: nothing
    /// of the call outlives it here, no thread, no descriptor. A failure to save the call's
    /// events or output is the call's error when the call itself went right ([`settle`]).
    fn generate(&self, sheet: &Path, prompt: &str, dir: &Path) -> Result<PathBuf, String> {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let deadline = Instant::now() + self.timeout;
        let (stderr_file, stderr_tmp, stderr_path) =
            work::create_temp(&dir.join("stderr.txt"), 0o644)?;
        let (events, events_tmp, events_path) =
            work::create_temp(&dir.join("events.jsonl"), 0o644)?;
        // From before the spawn until the group is in LIVE, a stop signal is held for this
        // thread and waited for by the handler on any other (Blocked, then Spawning, dropped
        // in the reverse order on every way out).
        let blocked = Blocked::new();
        let spawning = Spawning::new();
        let mut command = Command::new(&self.bin);
        // SAFETY: the closure runs in the child between fork and exec and calls only
        // sigemptyset and sigprocmask, which are async-signal-safe. The child inherits the
        // blocked mask; codex starts with none blocked.
        unsafe {
            command.pre_exec(|| {
                let mut none: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut none);
                libc::sigprocmask(libc::SIG_SETMASK, &none, std::ptr::null_mut());
                Ok(())
            });
        }
        let child = command
            .args(["exec", "-m", MODEL, "-c", "model_reasoning_effort=low"])
            .args([
                "-s",
                "workspace-write",
                "--skip-git-repo-check",
                "--json",
                "-C",
            ])
            .arg(dir)
            .arg("-i")
            .arg(sheet)
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|e| format!("{}: {e}", self.bin.display()))?;
        #[cfg(debug_assertions)]
        signal_in_spawn_for_tests();
        let mut group = Group::new(child);
        drop(spawning);
        drop(blocked);
        let (Some(stdout), Some(stdin), Some(stderr)) = (
            group.child.stdout.take(),
            group.child.stdin.take(),
            group.child.stderr.take(),
        ) else {
            return Err("no pipes to the call".into());
        };
        nonblocking(stdin.as_raw_fd())?;
        nonblocking(stdout.as_raw_fd())?;
        nonblocking(stderr.as_raw_fd())?;
        let mut pipes = Pipes {
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr: Some(stderr),
            prompt: prompt.as_bytes(),
            written: 0,
            refused: None,
            // Buffered: a stream of short lines is not a write per line.
            lines: EventLines::new(std::io::BufWriter::new(events)),
            // Read like the events, so a call that writes its stderr without end fills no disk.
            errs: Capped {
                out: std::io::BufWriter::new(stderr_file),
                kept: 0,
                max: STDERR_MAX,
                error: None,
            },
        };
        if pipes.prompt.is_empty() {
            pipes.stdin = None;
        }
        let waited = loop {
            if let Some(e) = &pipes.refused {
                break Err(format!("the call did not take its prompt: {e}"));
            }
            match group.child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => {}
                Err(e) => break Err(e.to_string()),
            }
            let now = Instant::now();
            if now >= deadline {
                break Err(format!("timed out after {} s", self.timeout.as_secs()));
            }
            pipes.step(deadline.min(now + Duration::from_millis(50)));
        };
        // Whatever the call left running goes with it.
        drop(group);
        let grace = Instant::now() + CLEANUP;
        while pipes.stdout.is_some() && Instant::now() < grace {
            pipes.step(grace);
        }
        let ended = pipes.stdout.is_none();
        let taken = pipes.stdin.is_none() && pipes.refused.is_none();
        let refused = pipes.refused.take();
        // Our ends of both pipes close here, whoever holds the others.
        drop(pipes.stdin.take());
        drop(pipes.stdout.take());
        drop(pipes.stderr.take());
        let stream = pipes.lines.finish();
        let errs = pipes.errs.finish();
        // Both files are put in place whatever happened; the first failure of either is kept.
        let mut saved = stream
            .error
            .map(|e| format!("the call's events were not saved: {e}"))
            .or(errs.map(|e| format!("the call's stderr was not saved: {e}")));
        for (tmp, path) in [(&stderr_tmp, &stderr_path), (&events_tmp, &events_path)] {
            if let Err(e) = work::commit(tmp, path) {
                saved.get_or_insert(format!("the call's output was not saved: {e}"));
            }
        }
        let id = settle(
            Settled {
                waited,
                taken,
                refused,
                ended,
                saved,
            },
            stream.thread,
        )?;
        let images = self.home.join("generated_images").join(&id);
        let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&images)
            .map_err(|_| "no image generated".to_string())?
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        found.sort();
        found
            .pop()
            .map(|(_, p)| p)
            .ok_or("no image generated".into())
    }

    /// The image is moved, not copied: its thread's folder under `generated_images` goes once
    /// the image was recorded (codex's own session log, outside the tool's reach, keeps the sheet
    /// that was sent).
    fn finish(&self, image: &Path) {
        let Some(thread) = image.parent() else { return };
        let images = self.home.join("generated_images");
        let inside = match (
            thread.parent().map(Path::canonicalize),
            images.canonicalize(),
        ) {
            (Some(Ok(a)), Ok(b)) => a == b,
            _ => false,
        };
        if inside {
            let _ = std::fs::remove_dir_all(thread);
        }
    }
}

/// What `out/<sheet>.json` says about `out/<sheet>.png`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub sheet: String,
    /// The job's [`identity`] when the call was made.
    pub identity: String,
    pub width: u32,
    pub height: u32,
    /// [`work::digest`] of the PNG bytes.
    pub digest: String,
}

/// The identity of `job` as it is now: its atlas generation, sheet, attempt, prompt text and
/// the bytes of the sheet that is sent.
pub fn identity(work: &Work, job: &Job) -> Result<String, String> {
    let sheet = img::read_capped(&work.sheet(&job.sheet), img::PNG_MAX_BYTES)?;
    Ok(work::digest(&[
        job.generation.as_bytes(),
        job.sheet.as_bytes(),
        &[job.attempt],
        prompt::text(job).as_bytes(),
        &sheet,
    ]))
}

/// Records `png` as `job`'s output, the image then its receipt (its header size included);
/// returns whether it is the sheet's size. It must be a PNG; one of the sheet's size must
/// decode. One of another size is recorded as it is, not decoded: the attempt is spent, and
/// ingest refuses it by its receipt's size (its items are re-rolled once, then left to the
/// scaler), so a model that keeps answering in another size is not paid again and again.
pub fn record(work: &Work, job: &Job, png: &[u8]) -> Result<bool, String> {
    let (width, height) = img::png_size(png).map_err(|e| format!("not a PNG: {e}"))?;
    let fits = (width, height) == (job.width, job.height);
    if fits {
        img::decode_png(png, width, height).map_err(|e| format!("does not decode: {e}"))?;
    }
    let receipt = Receipt {
        sheet: job.sheet.clone(),
        identity: identity(work, job)?,
        width,
        height,
        digest: work::digest(&[png]),
    };
    work::write_file(&work.out(&job.sheet), png)?;
    let json = serde_json::to_vec(&receipt).map_err(|e| e.to_string())?;
    work::write_file(&work.receipt(&job.sheet), &json)?;
    Ok(fits)
}

/// `job`'s recorded output, if it is done (see the module doc): its PNG bytes and size.
pub fn output(work: &Work, job: &Job) -> Option<(Vec<u8>, u32, u32)> {
    let text = img::read_capped(&work.receipt(&job.sheet), 1 << 16).ok()?;
    let r: Receipt = serde_json::from_slice(&text).ok()?;
    if r.sheet != job.sheet || r.identity != identity(work, job).ok()? {
        return None;
    }
    let png = img::read_capped(&work.out(&job.sheet), img::PNG_MAX_BYTES).ok()?;
    if work::digest(&[&png]) != r.digest || img::png_size(&png).ok()? != (r.width, r.height) {
        return None;
    }
    Some((png, r.width, r.height))
}

pub struct RunOptions {
    pub send: bool,
    pub limit: Option<usize>,
    /// Calls at once, 2..=4.
    pub parallel: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunSummary {
    pub pending: usize,
    pub calls: usize,
    pub ok: usize,
    /// Of `ok`: images of another size than their sheet, recorded for ingest to refuse.
    pub other_size: usize,
    pub failed: usize,
    /// Jobs not called again: [`CALLS_MAX`] calls of theirs failed (ingest refuses them).
    pub given_up: usize,
}

/// Failed calls of one job identity after which the job is given up: not called again, and
/// refused by ingest like a sheet of another size (its items re-rolled once, then left to the
/// scaler). Timeouts and calls that bring back no image are paid at most this often.
pub const CALLS_MAX: usize = 2;

/// Failed calls per job identity, from `runs.jsonl` (lines without an identity, written by an
/// earlier tool, count for none).
pub fn failed_calls(work: &Work) -> Result<std::collections::BTreeMap<String, usize>, String> {
    let mut out = std::collections::BTreeMap::new();
    let p = work.path("runs.jsonl");
    let text = match img::read_capped(&p, 256 << 20) {
        Ok(t) => t,
        Err(_) if !p.exists() => return Ok(out),
        Err(e) => return Err(e),
    };
    for line in text.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        if v["ok"] == false
            && let Some(id) = v["identity"].as_str()
        {
            *out.entry(id.to_string()).or_default() += 1;
        }
    }
    Ok(out)
}

/// Whether `job` is given up: [`CALLS_MAX`] failed calls of its identity as it is now.
pub fn given_up(
    work: &Work,
    job: &Job,
    failed: &std::collections::BTreeMap<String, usize>,
) -> bool {
    identity(work, job).is_ok_and(|id| failed.get(&id).copied().unwrap_or(0) >= CALLS_MAX)
}

/// The notice printed before anything is sent (and instead of sending without `--send`).
pub fn notice(n: usize) -> String {
    format!(
        "{n} sheet(s) to generate. Running them sends the owner's game art on those sheets to \
         OpenAI through the ChatGPT account Codex is signed in to (codex exec -m {MODEL}, the \
         built-in image_gen tool); the results stay in the work dir."
    )
}

/// Runs the pending jobs (all, or the first `limit`), printing one line per sheet to `out`.
pub fn run(
    work: &Work,
    runner: &dyn Runner,
    opts: &RunOptions,
    out: &mut (dyn Write + Send),
) -> Result<RunSummary, String> {
    let jobs = work.read_jobs()?;
    let failed = failed_calls(work)?;
    let not_done: Vec<&Job> = jobs.iter().filter(|j| output(work, j).is_none()).collect();
    let (dropped, mut pending): (Vec<&Job>, Vec<&Job>) = not_done
        .into_iter()
        .partition(|j| given_up(work, j, &failed));
    let given_up = dropped.len();
    if let Some(n) = opts.limit {
        pending.truncate(n);
    }
    let mut summary = RunSummary {
        pending: pending.len(),
        given_up,
        ..RunSummary::default()
    };
    let _ = writeln!(out, "{}", notice(pending.len()));
    if given_up > 0 {
        let _ = writeln!(
            out,
            "{given_up} sheet(s) given up after {CALLS_MAX} failed calls each (not sent again; \
             ingest re-rolls their items once, then leaves them to the scaler)."
        );
    }
    if !opts.send {
        let _ = writeln!(out, "Nothing was sent: add --send to run them.");
        return Ok(summary);
    }
    work.mkdirs()?;
    let next = AtomicUsize::new(0);
    let shared = Mutex::new((out, &mut summary, String::new()));
    let parallel = opts.parallel.clamp(2, 4).min(pending.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..parallel {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(job) = pending.get(i) else { break };
                    let t = Instant::now();
                    let id = identity(work, job).unwrap_or_default();
                    let result = call(work, runner, job);
                    let secs = t.elapsed().as_secs();
                    let mut g = shared.lock().unwrap_or_else(|e| e.into_inner());
                    let (out, summary, log) = &mut *g;
                    summary.calls += 1;
                    let line = match &result {
                        Ok(true) => {
                            summary.ok += 1;
                            format!("{}: ok in {secs} s", job.sheet)
                        }
                        Ok(false) => {
                            summary.ok += 1;
                            summary.other_size += 1;
                            format!(
                                "{}: back in {secs} s, not the sheet's size (recorded; ingest \
                                 refuses it)",
                                job.sheet
                            )
                        }
                        Err(e) => {
                            summary.failed += 1;
                            format!("{}: failed in {secs} s: {e}", job.sheet)
                        }
                    };
                    let _ = writeln!(out, "{line}");
                    let rec = serde_json::json!({
                        "sheet": job.sheet, "identity": id, "ok": result.is_ok(),
                        "seconds": secs, "error": result.err(),
                    });
                    // Written after every call, so a run that stops keeps its record.
                    let line = format!("{rec}\n");
                    if let Err(e) = work::append_file(&work.path("runs.jsonl"), &line) {
                        log.push_str(&e);
                    }
                }
            });
        }
    });
    let (_, _, log) = shared.into_inner().unwrap_or_else(|e| e.into_inner());
    if !log.is_empty() {
        return Err(log);
    }
    Ok(summary)
}

/// One call: the generated image recorded as the job's output, its receipt written, and only
/// then what the call left of it removed (whether it is the sheet's size, see [`record`]). A
/// call whose image is not recorded keeps it there.
fn call(work: &Work, runner: &dyn Runner, job: &Job) -> Result<bool, String> {
    let dir = work::ensure_dir(&work.codex_dir(&job.sheet))?;
    let sheet = work.sheet(&job.sheet);
    let got = runner.generate(&sheet, &prompt::text(job), &dir)?;
    let png = img::read_capped(&got, img::PNG_MAX_BYTES)?;
    let fits = record(work, job, &png)?;
    runner.finish(&got);
    Ok(fits)
}

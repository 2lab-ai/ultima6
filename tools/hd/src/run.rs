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
//! another size is recorded undecoded and ingest refuses it ([`record`]). Every call is
//! reserved in the run ledger before it is made ([`Ledger`]); a job with [`CALLS_MAX`]
//! reserved calls (conservative) of its identity that is still not done (timeouts, no image,
//! cut off, its output lost or invalid) is given up: not called again, refused by ingest, as
//! is one whose sheet cannot be read. Either way its items are re-rolled once, then left to the scaler.
//!
//! Stopped by SIGINT, SIGTERM or SIGHUP ([`stop_calls_on_signals`]), the run starts no call,
//! each running call kills and reaps its group at its next turn, and the command ends by the
//! signal once all of them have returned.

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
use crate::work::{self, Work, now_ns, test_note};

/// What makes an image from a sheet and a prompt.
pub trait Runner: Sync {
    /// Runs one call with `sheet` attached and `prompt`, `dir` as its working dir; returns the
    /// generated image's path.
    fn generate(&self, sheet: &Path, prompt: &str, dir: &Path) -> Result<PathBuf, String>;

    /// [`Runner::generate`], reserved: `book(Book::Reserve)` writes the run ledger's line that
    /// the call is made, and the call is made only once it has; `book(Book::Void)` takes it
    /// back when the call is not started after all (nothing was sent). The admission point is
    /// the last look at the stop, after the reservation is durable and before the call: a
    /// stop seen there voids the reservation ([`NOT_STARTED`]); a call past it is admitted, so
    /// a signal that comes after it, even before the spawn itself, finds the call started and
    /// stops it at its first turn.
    fn dispatch(
        &self,
        sheet: &Path,
        prompt: &str,
        dir: &Path,
        book: &mut dyn FnMut(Book) -> Result<(), String>,
    ) -> Result<PathBuf, String> {
        if stopped_by().is_some() {
            return Err(NOT_STARTED.into());
        }
        book(Book::Reserve)?;
        if stopped_by().is_some() {
            return Err(not_started(book));
        }
        self.generate(sheet, prompt, dir)
    }

    /// After the image at `image` was recorded (output and receipt written): removes what the
    /// call left of it.
    fn finish(&self, image: &Path) {
        let _ = image;
    }
}

/// A call not started at its admission point: its reservation voided, [`NOT_STARTED`] (with
/// the void's own failure, if writing it failed: the call then counts as reserved here, in
/// this run's accounting, though a void line that was written before its directory sync
/// failed is replayed from the ledger as voided; only a crash leaves that uncertain, and
/// nothing is overspent either way, since the call never started).
fn not_started(book: &mut dyn FnMut(Book) -> Result<(), String>) -> String {
    match book(Book::Void) {
        Ok(()) => NOT_STARTED.into(),
        Err(v) => format!("{NOT_STARTED} (also: {v})"),
    }
}

/// What [`Runner::dispatch`] asks of the run ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Book {
    /// The call is about to be made (reserved, so counted from here on).
    Reserve,
    /// The call reserved could not be started (its process never existed): nothing was sent.
    Void,
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

/// The signal stopping the tool (0: none). Set by [`on_signal`] only; read before every spawn
/// and on every turn of every call's loop, where the stopping is done.
static SHUTDOWN: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Held around "not stopping? reserve, spawn, own the group" (spawn admission): no call is
/// spawned once [`SHUTDOWN`] is seen.
static ADMIT: Mutex<()> = Mutex::new(());

/// The signals that stop a command with its calls (Ctrl-C, a kill, a closed terminal,
/// `Ctrl-\`).
const STOP: [libc::c_int; 4] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT];

/// The stop signals [`on_signal`] was installed for (bit `1 << sig`): one the tool inherited
/// as ignored (`nohup`, a script's background job) stays ignored, and is not reset either.
static INSTALLED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A call that was not started: the tool is stopping (nothing reserved, the job left unrun).
pub const NOT_STARTED: &str = "not started: the tool is stopping";
/// A call stopped because the tool is stopping (its group killed and reaped).
pub const STOPPED: &str = "stopped: the tool is stopping";

/// How long a stop may be pending before the stop signals take their default action again
/// ([`watch_the_stop`]): a later one then ends the tool at once.
pub const STOP_GRACE: Duration = Duration::from_secs(2);

/// A stop signal: notes it (the first one) for the calls to see, and nothing else, so more
/// stop signals while the calls are being stopped (a closed terminal sends SIGHUP twice, a
/// few ms apart) change nothing. (An atomic: async-signal-safe; the stopping is done by the
/// calls, in normal context.)
extern "C" fn on_signal(sig: libc::c_int) {
    let _ = SHUTDOWN.compare_exchange(0, sig, Ordering::SeqCst, Ordering::SeqCst);
}

/// Starts the thread that, once a stop has been pending [`STOP_GRACE`], puts back the default
/// action of each stop signal [`on_signal`] was installed for (from normal context), so a
/// stop that stalls (a write that does not return) still yields to a later signal.
pub fn watch_the_stop() {
    std::thread::spawn(|| {
        while stopped_by().is_none() {
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(STOP_GRACE);
        let installed = INSTALLED.load(Ordering::SeqCst);
        for s in STOP {
            if installed & (1 << s) != 0 {
                // SAFETY: signal(2) on this process.
                unsafe { libc::signal(s, libc::SIG_DFL) };
            }
        }
    });
}

/// Installs [`on_signal`] for SIGINT, SIGTERM, SIGHUP and SIGQUIT (Ctrl-C, a `kill`, a closed
/// terminal, `Ctrl-\`), each unless the tool was started with it ignored (`nohup`, a script's
/// background job): then it stays ignored, and the calls (which inherit that) run on through
/// it. Pair with [`watch_the_stop`]. The calls run in process groups of their own, which those signals do not reach:
/// stopped so, the tool starts no call, each running call kills and reaps its group at its
/// next turn, and the command, its workers returned, lets go of its lock and ends by the
/// signal ([`reraise`]).
pub fn stop_calls_on_signals() {
    for sig in STOP {
        // SAFETY: sigaction(2) with a zeroed struct to read the current action, then one with
        // only the handler and an empty mask set; the handler is an extern "C" fn of the
        // expected signature.
        unsafe {
            let mut old: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(sig, std::ptr::null(), &mut old) != 0
                || old.sa_sigaction == libc::SIG_IGN
            {
                continue;
            }
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(sig, &action, std::ptr::null_mut()) == 0 {
                INSTALLED.fetch_or(1 << sig, Ordering::SeqCst);
            }
        }
    }
}

/// The signal that is stopping the tool, if one is.
pub fn stopped_by() -> Option<libc::c_int> {
    let s = SHUTDOWN.load(Ordering::SeqCst);
    (s != 0).then_some(s)
}

/// Turns core dumps off for the rest of the tool's life (soft and hard limits 0) and checks it
/// took: a SIGQUIT, whenever it ends the tool (raised again by [`reraise`], or after the stop
/// grace when [`watch_the_stop`] has put its default action back), leaves no core holding the
/// sheets' bytes. Called before any stop machinery is installed.
pub fn no_core_dumps() -> Result<(), String> {
    // SAFETY: setrlimit(2) and getrlimit(2) with live structs.
    let now = unsafe {
        let none = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        let mut now = libc::rlimit {
            rlim_cur: 1,
            rlim_max: 1,
        };
        if libc::setrlimit(libc::RLIMIT_CORE, &none) != 0
            || libc::getrlimit(libc::RLIMIT_CORE, &mut now) != 0
        {
            return Err(format!(
                "core dumps could not be turned off: {}",
                std::io::Error::last_os_error()
            ));
        }
        now
    };
    test_note(&format!("core-limit {} {}", now.rlim_cur, now.rlim_max));
    if (now.rlim_cur, now.rlim_max) != (0, 0) {
        return Err("core dumps could not be turned off".into());
    }
    Ok(())
}

/// Ends the tool by `sig` with its default action (the caller has let go of its lock), with
/// core dumps off first (again: see [`no_core_dumps`]), so a SIGQUIT leaves no core holding
/// the sheets' bytes.
pub fn reraise(sig: libc::c_int) -> ! {
    // SAFETY: setrlimit(2), signal(2) and raise(3) on this process.
    unsafe {
        let none = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        libc::setrlimit(libc::RLIMIT_CORE, &none);
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
    std::process::exit(128 + sig)
}

/// Test builds only (`debug_assertions`, which the gate profile keeps), all from the
/// environment. `U6_HD_TEST_SIGNAL_AT=<point>` names a point of a call's life: `before-spawn`
/// (in spawn admission, before its first look at the stop), `reserve` (the reservation
/// written, before the admission point), `after-spawn` (the group owned, before its first
/// turn) or `reap` (the group killed, before it is waited for). The `U6_HD_TEST_NTH`-th time
/// (default 1) the point is reached: it waits until the file `U6_HD_TEST_WAIT` has
/// `U6_HD_TEST_WAIT_LINES` lines (default 1, at most 15 s), writes the file
/// `U6_HD_TEST_MARK`, sends `U6_HD_TEST_SIGNAL` (if set) to its own process group as a terminal
/// does, noting `stop` first and waiting until it is handled, then waits
/// `U6_HD_TEST_DELAY_MS`.
#[cfg(debug_assertions)]
fn test_point(at: &str) {
    static SEEN: AtomicUsize = AtomicUsize::new(0);
    let var = |k: &str| std::env::var(k).ok();
    if var("U6_HD_TEST_SIGNAL_AT").as_deref() != Some(at) {
        return;
    }
    let nth: usize = var("U6_HD_TEST_NTH")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    if SEEN.fetch_add(1, Ordering::SeqCst) + 1 != nth {
        return;
    }
    if let Some(wait) = var("U6_HD_TEST_WAIT") {
        let lines: usize = var("U6_HD_TEST_WAIT_LINES")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let start = Instant::now();
        while std::fs::read_to_string(&wait).map_or(0, |t| t.lines().count()) < lines
            && start.elapsed() < Duration::from_secs(15)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if let Some(mark) = var("U6_HD_TEST_MARK") {
        let _ = std::fs::write(mark, now_ns().to_string());
    }
    if let Some(sig) = var("U6_HD_TEST_SIGNAL").and_then(|v| v.parse::<libc::c_int>().ok()) {
        test_note("stop");
        // SAFETY: kill(2) to this process's group.
        unsafe { libc::kill(0, sig) };
        let start = Instant::now();
        while stopped_by().is_none() && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    let ms = var("U6_HD_TEST_DELAY_MS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    std::thread::sleep(Duration::from_millis(ms));
}

#[cfg(not(debug_assertions))]
fn test_point(_: &str) {}

/// A spawned call: on drop, on every way out of [`Codex::generate`], its whole process group is
/// killed and the call reaped.
struct Group {
    child: std::process::Child,
}

impl Drop for Group {
    fn drop(&mut self) {
        kill_group(self.child.id());
        test_point("reap");
        let _ = self.child.wait();
        test_note(&format!("reaped {}", self.child.id()));
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
    /// Reading the events or the stderr failed (that pipe is closed, not ended cleanly).
    out_failed: Option<std::io::Error>,
    err_failed: Option<std::io::Error>,
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
/// once `until` has passed, handing each chunk to `sink`; closes it at its end, or on a read
/// error that a retry does not clear, which it returns (an end, but not a clean one).
pub(crate) fn drain<R: Read>(
    pipe: &mut Option<R>,
    until: Instant,
    mut sink: impl FnMut(&[u8]),
) -> Option<std::io::Error> {
    let p = pipe.as_mut()?;
    let mut chunk = [0u8; 8192];
    let (mut moved, mut end, mut failed) = (0, false, None);
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
            Err(e) => {
                end = true;
                failed = Some(e);
                break;
            }
        }
    }
    if end {
        *pipe = None;
    }
    failed
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
            if let Some(e) = drain(&mut self.stdout, until, |b| lines.feed(b)) {
                self.out_failed.get_or_insert(e);
            }
        }
        if self.stderr.as_ref().is_some_and(|e| ready(e.as_raw_fd())) {
            let errs = &mut self.errs;
            if let Some(e) = drain(&mut self.stderr, until, |b| errs.put(b)) {
                self.err_failed.get_or_insert(e);
            }
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
    /// The event stream ended (cleanly: not a read error).
    pub(crate) ended: bool,
    /// Reading the event stream failed.
    pub(crate) unread: Option<std::io::Error>,
    /// The first failure saving (or reading) the call's events, stderr or output.
    pub(crate) saved: Option<String>,
}

/// The call's thread id, or why the call failed, in this precedence: how it was stopped, its
/// prompt not taken, its events unreadable or not ended, its exit status; then a failure
/// saving what it printed, which is the error when the call itself went right and is kept
/// beside the call's own error otherwise; then the thread id missing or malformed.
pub(crate) fn settle(s: Settled, thread: Option<String>) -> Result<String, String> {
    let call = (|| {
        let status = s.waited?;
        if !s.taken {
            return Err(match s.refused {
                Some(e) => format!("the call did not take its prompt: {e}"),
                None => "the call did not take its prompt".into(),
            });
        }
        if let Some(e) = s.unread {
            return Err(format!("the call's output could not be read: {e}"));
        }
        if !s.ended {
            return Err("the call's output did not end".into());
        }
        if !status.success() {
            return Err(format!("codex exited with {status}"));
        }
        Ok(())
    })();
    match (call, s.saved) {
        (Err(e), Some(saved)) => return Err(format!("{e} (also: {saved})")),
        (Err(e), None) => return Err(e),
        (Ok(()), Some(saved)) => return Err(saved),
        (Ok(()), None) => {}
    }
    let id = thread.ok_or("no thread id in the codex events")?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("an unexpected thread id in the codex events".into());
    }
    Ok(id)
}

impl Runner for Codex {
    fn generate(&self, sheet: &Path, prompt: &str, dir: &Path) -> Result<PathBuf, String> {
        self.dispatch(sheet, prompt, dir, &mut |_| Ok(()))
    }

    /// One deadline covers the call, all of it in this thread: the prompt is written and the
    /// events and stderr read as the pipes are ready (non-blocking, one `poll` for the three),
    /// while the call is watched for its exit, until it exits, the prompt fails, the deadline
    /// passes or the tool is stopping ([`STOPPED`]); then the call's group is killed and
    /// reaped (on every path) and its output pipes given [`CLEANUP`] to end, each tracked on
    /// its own (each wait bounded as [`Pipes::step`] says: the files' writes are assumed to
    /// return; no grace when stopping). Then the pipes are closed, whatever still holds their
    /// other ends: nothing of the call outlives it here, no thread, no descriptor. The spawn
    /// is admitted under [`ADMIT`]: its reservation written, the stop looked at once more (the
    /// admission point: a stop seen there voids the reservation), then the spawn; a spawn that
    /// fails (no process made: nothing sent) voids it too.
    fn dispatch(
        &self,
        sheet: &Path,
        prompt: &str,
        dir: &Path,
        book: &mut dyn FnMut(Book) -> Result<(), String>,
    ) -> Result<PathBuf, String> {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let deadline = Instant::now() + self.timeout;
        let (stderr_file, stderr_tmp, stderr_path) =
            work::create_temp(&dir.join("stderr.txt"), 0o644)?;
        let (events, events_tmp, events_path) =
            work::create_temp(&dir.join("events.jsonl"), 0o644)?;
        let mut group = {
            let _admit = ADMIT.lock().unwrap_or_else(|e| e.into_inner());
            test_point("before-spawn");
            let unmade = |e: String| {
                let _ = std::fs::remove_file(&stderr_tmp);
                let _ = std::fs::remove_file(&events_tmp);
                e
            };
            if stopped_by().is_some() {
                return Err(unmade(NOT_STARTED.into()));
            }
            book(Book::Reserve).map_err(unmade)?;
            // The admission point: the reservation durable, the last look at the stop.
            if stopped_by().is_some() {
                return Err(unmade(not_started(book)));
            }
            let child = Command::new(&self.bin)
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
                .spawn();
            let child = match child {
                Ok(c) => c,
                Err(e) => {
                    let e = format!("{}: {e}", self.bin.display());
                    return Err(unmade(match book(Book::Void) {
                        Ok(()) => e,
                        Err(v) => format!("{e} (also: {v})"),
                    }));
                }
            };
            Group { child }
        };
        test_point("after-spawn");
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
            out_failed: None,
            err_failed: None,
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
            if stopped_by().is_some() {
                break Err(STOPPED.to_string());
            }
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
        // Whatever the call left running goes with it: killed and reaped.
        drop(group);
        // Each output pipe is drained until its own end, within the grace (none when stopping).
        let grace = Instant::now() + CLEANUP;
        while (pipes.stdout.is_some() || pipes.stderr.is_some())
            && Instant::now() < grace
            && stopped_by().is_none()
        {
            pipes.step(grace);
        }
        let unread = pipes.out_failed.take();
        let ended = pipes.stdout.is_none() && unread.is_none();
        let taken = pipes.stdin.is_none() && pipes.refused.is_none();
        let refused = pipes.refused.take();
        // Our ends of the pipes close here, whoever holds the others.
        drop(pipes.stdin.take());
        drop(pipes.stdout.take());
        drop(pipes.stderr.take());
        let stream = pipes.lines.finish();
        let errs = pipes.errs.finish();
        // Both files are put in place whatever happened; the first failure of either is kept.
        let mut saved = stream
            .error
            .map(|e| format!("the call's events were not saved: {e}"))
            .or(errs.map(|e| format!("the call's stderr was not saved: {e}")))
            .or(pipes
                .err_failed
                .take()
                .map(|e| format!("the call's stderr could not be read: {e}")));
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
                unread,
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
    /// Calls cut off because the tool is stopping (their reservations count).
    pub stopped: usize,
    /// Jobs not called again: [`CALLS_MAX`] reserved calls of their identity (ingest refuses
    /// them).
    pub given_up: usize,
    /// Jobs whose sheet cannot be read, so not sent (ingest refuses them).
    pub unreadable: usize,
    /// Lines of `runs.jsonl` from before calls were reserved: they count for no job.
    pub legacy: usize,
}

/// Reserved calls of one job identity after which the job is given up: not called again, and
/// refused by ingest like a sheet of another size (its items re-rolled once, then left to the
/// scaler). A call counts once it is reserved (`runs.jsonl`), before it is made: a call that
/// times out, brings back no image, is cut off by a signal or a kill, or whose output is
/// later lost or invalid counts the same. The count is conservative: a reservation whose call
/// was never sent counts too when nothing could take it back (the tool killed between the
/// reservation and the spawn, a directory sync that failed after the line was written).
pub const CALLS_MAX: usize = 2;

/// `runs.jsonl`, the run ledger: a `dispatched` line written and synced before each call is
/// made (the reservation), a `voided` one when the call was not started after all, and a
/// `result` line after it, each with its time (`at`, ns since the epoch). Reserved calls per
/// identity (conservative, see [`CALLS_MAX`]) are its `dispatched` lines less its `voided`
/// ones. Lines without an `event` were written before reservations existed:
/// they count for no job (the bound counts from this version on), and `run` says how many.
#[derive(Debug, Default)]
pub struct Ledger {
    /// Reserved calls (conservative) per job identity.
    pub reserved: std::collections::BTreeMap<String, usize>,
    pub legacy: usize,
}

/// Reads the run ledger (none yet: empty).
pub fn ledger(work: &Work) -> Result<Ledger, String> {
    let mut out = Ledger::default();
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
        match (v["event"].as_str(), v["identity"].as_str()) {
            (Some("dispatched"), Some(id)) => {
                *out.reserved.entry(id.to_string()).or_default() += 1;
            }
            (Some("voided"), Some(id)) => {
                let n = out.reserved.entry(id.to_string()).or_default();
                *n = n.saturating_sub(1);
            }
            (Some(_), _) => {}
            (None, _) => out.legacy += 1,
        }
    }
    Ok(out)
}

/// Where a job that is not done stands.
#[derive(Debug, PartialEq, Eq)]
pub enum Standing {
    /// To be called: its identity.
    Open(String),
    /// [`CALLS_MAX`] reserved calls of its identity: not called again.
    GivenUp,
    /// Its identity cannot be had (the sheet cannot be read): not sent, never with another.
    Unreadable(String),
}

/// Where `job` (not done) stands against `ledger`.
pub fn standing(work: &Work, job: &Job, ledger: &Ledger) -> Standing {
    match identity(work, job) {
        Err(e) => Standing::Unreadable(e),
        Ok(id) if ledger.reserved.get(&id).copied().unwrap_or(0) >= CALLS_MAX => Standing::GivenUp,
        Ok(id) => Standing::Open(id),
    }
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
/// Stops starting calls once the tool is stopping ([`stopped_by`]); returns when every call
/// it started has returned (its group killed and reaped).
pub fn run(
    work: &Work,
    runner: &dyn Runner,
    opts: &RunOptions,
    out: &mut (dyn Write + Send),
) -> Result<RunSummary, String> {
    let jobs = work.read_jobs()?;
    let book = ledger(work)?;
    let mut summary = RunSummary {
        legacy: book.legacy,
        ..RunSummary::default()
    };
    let mut pending: Vec<(&Job, String)> = Vec::new();
    for job in jobs.iter().filter(|j| output(work, j).is_none()) {
        match standing(work, job, &book) {
            Standing::Open(id) => pending.push((job, id)),
            Standing::GivenUp => summary.given_up += 1,
            Standing::Unreadable(e) => {
                summary.unreadable += 1;
                let _ = writeln!(
                    out,
                    "{}: not sent, its sheet cannot be read: {e}",
                    job.sheet
                );
            }
        }
    }
    if let Some(n) = opts.limit {
        pending.truncate(n);
    }
    summary.pending = pending.len();
    let _ = writeln!(out, "{}", notice(pending.len()));
    if summary.given_up > 0 {
        let _ = writeln!(
            out,
            "{} sheet(s) given up after {CALLS_MAX} reserved calls each (conservative: a call \
             reserved and never sent counts; not sent again; ingest re-rolls their items once, \
             then leaves them to the scaler).",
            summary.given_up
        );
    }
    if book.legacy > 0 {
        let _ = writeln!(
            out,
            "runs.jsonl has {} line(s) from before calls were reserved; they count for no \
             sheet.",
            book.legacy
        );
    }
    if !opts.send {
        let _ = writeln!(out, "Nothing was sent: add --send to run them.");
        return Ok(summary);
    }
    work.mkdirs()?;
    let next = AtomicUsize::new(0);
    // The ledger is read and written whole: one writer at a time.
    let books = Mutex::new(());
    let abort = std::sync::atomic::AtomicBool::new(false);
    let shared = Mutex::new((out, &mut summary, String::new()));
    let parallel = opts.parallel.clamp(2, 4).min(pending.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..parallel {
            s.spawn(|| {
                loop {
                    if stopped_by().is_some() || abort.load(Ordering::SeqCst) {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some((job, id)) = pending.get(i) else {
                        break;
                    };
                    let t = Instant::now();
                    let (result, reserved) = call(work, runner, job, id, &books);
                    let secs = t.elapsed().as_secs();
                    let stopping = stopped_by().is_some();
                    let mut g = shared.lock().unwrap_or_else(|e| e.into_inner());
                    let (out, summary, log) = &mut *g;
                    if !reserved {
                        // Nothing was sent: the tool is stopping, or the ledger cannot be
                        // written (then nothing more is sent), or the call's dir is refused.
                        if let Err(e) = &result
                            && !stopping
                        {
                            summary.failed += 1;
                            let _ = writeln!(out, "{}: not sent: {e}", job.sheet);
                            if e.starts_with(LEDGER_FAILED) {
                                abort.store(true, Ordering::SeqCst);
                                if !log.is_empty() {
                                    log.push('\n');
                                }
                                log.push_str(e);
                            }
                        }
                        continue;
                    }
                    summary.calls += 1;
                    let line = match &result {
                        _ if stopping && result.is_err() => {
                            summary.stopped += 1;
                            format!(
                                "{}: stopped after {secs} s (the tool is stopping)",
                                job.sheet
                            )
                        }
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
                        "event": "result", "sheet": job.sheet, "identity": id,
                        "ok": result.is_ok(), "seconds": secs, "at": now_ns(),
                        "stopped": stopping && result.is_err(), "error": result.err(),
                    });
                    let line = format!("{rec}\n");
                    let _books = books.lock().unwrap_or_else(|e| e.into_inner());
                    if let Err(e) = work::append_durable(&work.path("runs.jsonl"), &line) {
                        if !log.is_empty() {
                            log.push('\n');
                        }
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

/// The start of the error of a call not made because its reservation could not be written.
pub const LEDGER_FAILED: &str = "the run ledger could not be written, so nothing was sent";

/// One call of `job` (identity `id`): reserved in the ledger (synced) before it is made, the
/// generated image recorded as the job's output, its receipt written, and only then what the
/// call left of it removed (whether it is the sheet's size, see [`record`]). A call whose
/// image is not recorded keeps it there. Returns the result and whether the call was reserved
/// (and not voided).
fn call(
    work: &Work,
    runner: &dyn Runner,
    job: &Job,
    id: &str,
    books: &Mutex<()>,
) -> (Result<bool, String>, bool) {
    let mut reserved = false;
    let result = (|| {
        let dir = work::ensure_dir(&work.codex_dir(&job.sheet))?;
        let sheet = work.sheet(&job.sheet);
        let mut book = |b: Book| {
            let event = match b {
                Book::Reserve => "dispatched",
                Book::Void => "voided",
            };
            let rec = serde_json::json!({
                "event": event, "sheet": job.sheet, "identity": id, "attempt": job.attempt,
                "at": now_ns(),
            });
            {
                let _books = books.lock().unwrap_or_else(|e| e.into_inner());
                work::append_durable(&work.path("runs.jsonl"), &format!("{rec}\n"))
                    .map_err(|e| format!("{LEDGER_FAILED}: {e}"))?;
            }
            reserved = b == Book::Reserve;
            if b == Book::Reserve {
                test_point("reserve");
            }
            Ok(())
        };
        let got = runner.dispatch(&sheet, &prompt::text(job), &dir, &mut book)?;
        let png = img::read_capped(&got, img::PNG_MAX_BYTES)?;
        let fits = record(work, job, &png)?;
        runner.finish(&got);
        Ok(fits)
    })();
    (result, reserved)
}

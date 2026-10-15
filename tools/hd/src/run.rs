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
//! done, and is run again. A call whose image is not a decodable PNG of the sheet's size fails,
//! records nothing and leaves the image where the call put it.

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
    let _ = Command::new("/bin/kill")
        .args(["-s", "KILL", "--", &format!("-{pgid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// A spawned call: on drop, on every way out of [`Codex::generate`], its whole process group is
/// killed and the call reaped.
struct Group {
    child: std::process::Child,
}

impl Drop for Group {
    fn drop(&mut self) {
        kill_group(self.child.id());
        let _ = self.child.wait();
    }
}

/// Codex's event stream, line by line as its bytes arrive: each line of at most [`LINE_MAX`]
/// bytes goes to `events` while they hold under [`EVENTS_MAX`] bytes, and the first
/// `thread.started` event's id is kept. A longer line is dropped up to its newline without
/// being held, so memory stays bounded whatever the stream holds.
pub struct EventLines<W: Write> {
    events: W,
    kept: u64,
    thread: Option<String>,
    line: Vec<u8>,
    dropping: bool,
}

impl<W: Write> EventLines<W> {
    pub fn new(events: W) -> EventLines<W> {
        EventLines {
            events,
            kept: 0,
            thread: None,
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
        if self.kept + (line.len() as u64) < EVENTS_MAX {
            self.kept += line.len() as u64 + 1;
            let _ = self
                .events
                .write_all(line)
                .and_then(|()| self.events.write_all(b"\n"));
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
    /// `events`, then the first thread id.
    pub fn finish(mut self) -> Option<String> {
        if !self.dropping && !self.line.is_empty() {
            let line = std::mem::take(&mut self.line);
            self.on_line(&line);
        }
        let _ = self.events.flush();
        self.thread
    }
}

/// Reads codex's event stream `r` to its end in 8 KiB chunks ([`EventLines`]); returns the
/// first thread id.
pub fn read_events(mut r: impl Read, events: impl Write) -> Option<String> {
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

/// The call's two pipes, driven from the calling thread: the prompt written to its input and
/// its events read, as each is ready. Closing a pipe is dropping it here.
struct Pipes<'a, W: Write> {
    stdin: Option<std::process::ChildStdin>,
    stdout: Option<std::process::ChildStdout>,
    prompt: &'a [u8],
    written: usize,
    /// The prompt's write failed: the call's input was closed before it took it all.
    refused: Option<std::io::Error>,
    lines: EventLines<W>,
}

impl<W: Write> Pipes<'_, W> {
    /// One turn: waits until either pipe is ready or `until` passes, then writes and reads what
    /// it can, at most [`TURN_BYTES`] and [`TURN_CALLS`] each way and never past `until`, so
    /// the caller's checks (the call's exit, the deadline, the grace) run between turns
    /// whatever the call does with its pipes.
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
        if let Some(o) = &self.stdout {
            fds.push(libc::pollfd {
                fd: o.as_raw_fd(),
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
        if let Some(o) = &mut self.stdout
            && ready(o.as_raw_fd())
        {
            let mut chunk = [0u8; 8192];
            let mut moved = 0;
            for _ in 0..TURN_CALLS {
                if moved >= TURN_BYTES || Instant::now() >= until {
                    break;
                }
                match o.read(&mut chunk) {
                    Ok(0) => {
                        self.stdout = None;
                        break;
                    }
                    Ok(n) => {
                        moved += n;
                        self.lines.feed(&chunk[..n]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => {
                        self.stdout = None;
                        break;
                    }
                }
            }
        }
    }
}

impl Runner for Codex {
    /// One deadline covers the call, all of it in this thread: the prompt is written and the
    /// events read as the pipes are ready (both non-blocking, one `poll` for the two), while
    /// the call is watched for its exit, until it exits, the prompt fails or the deadline
    /// passes; then the call's group is killed (on every path) and the stream given [`CLEANUP`]
    /// to end. Then both pipes are closed, whatever still holds their other ends: nothing of
    /// the call outlives it here, no thread, no descriptor.
    fn generate(&self, sheet: &Path, prompt: &str, dir: &Path) -> Result<PathBuf, String> {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let deadline = Instant::now() + self.timeout;
        let (stderr, stderr_tmp, stderr_path) = work::create_temp(&dir.join("stderr.txt"), 0o644)?;
        let (events, events_tmp, events_path) =
            work::create_temp(&dir.join("events.jsonl"), 0o644)?;
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
            .stderr(stderr)
            .process_group(0)
            .spawn()
            .map_err(|e| format!("{}: {e}", self.bin.display()))?;
        let mut group = Group { child };
        let (Some(stdout), Some(stdin)) = (group.child.stdout.take(), group.child.stdin.take())
        else {
            return Err("no pipes to the call".into());
        };
        nonblocking(stdin.as_raw_fd())?;
        nonblocking(stdout.as_raw_fd())?;
        let mut pipes = Pipes {
            stdin: Some(stdin),
            stdout: Some(stdout),
            prompt: prompt.as_bytes(),
            written: 0,
            refused: None,
            // Buffered: a stream of short lines is not a write per line.
            lines: EventLines::new(std::io::BufWriter::new(events)),
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
        let thread_id = pipes.lines.finish();
        let _ = work::commit(&stderr_tmp, &stderr_path);
        let _ = work::commit(&events_tmp, &events_path);
        let status = waited?;
        if !taken {
            return Err(match refused {
                Some(e) => format!("the call did not take its prompt: {e}"),
                None => "the call did not take its prompt".into(),
            });
        }
        if !ended {
            return Err("the call's output did not end".into());
        }
        if !status.success() {
            return Err(format!("codex exited with {status}"));
        }
        let id = thread_id.ok_or("no thread id in the codex events")?;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err("an unexpected thread id in the codex events".into());
        }
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

/// Records `png` as `job`'s output: refused unless its header is the size of the sheet that was
/// sent and it decodes at that size; the image, then its receipt.
pub fn record(work: &Work, job: &Job, png: &[u8]) -> Result<(), String> {
    let (width, height) = img::png_size(png).map_err(|e| format!("not a PNG: {e}"))?;
    if (width, height) != (job.width, job.height) {
        return Err(format!(
            "the image is {width} x {height}, the sheet {} x {}",
            job.width, job.height
        ));
    }
    record_unchecked(work, job, png)
}

/// [`record`] without the size check: the image as it is, for tests of what ingest does with an
/// output of another size (recorded by an earlier tool, or edited in the work dir).
pub(crate) fn record_unchecked(work: &Work, job: &Job, png: &[u8]) -> Result<(), String> {
    let (width, height) = img::png_size(png).map_err(|e| format!("not a PNG: {e}"))?;
    img::decode_png(png, width, height).map_err(|e| format!("does not decode: {e}"))?;
    let receipt = Receipt {
        sheet: job.sheet.clone(),
        identity: identity(work, job)?,
        width,
        height,
        digest: work::digest(&[png]),
    };
    work::write_file(&work.out(&job.sheet), png)?;
    let json = serde_json::to_vec(&receipt).map_err(|e| e.to_string())?;
    work::write_file(&work.receipt(&job.sheet), &json)
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
    pub failed: usize,
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
    let mut pending: Vec<&Job> = jobs.iter().filter(|j| output(work, j).is_none()).collect();
    if let Some(n) = opts.limit {
        pending.truncate(n);
    }
    let mut summary = RunSummary {
        pending: pending.len(),
        ..RunSummary::default()
    };
    let _ = writeln!(out, "{}", notice(pending.len()));
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
                    let result = call(work, runner, job);
                    let secs = t.elapsed().as_secs();
                    let mut g = shared.lock().unwrap_or_else(|e| e.into_inner());
                    let (out, summary, log) = &mut *g;
                    summary.calls += 1;
                    let line = match &result {
                        Ok(()) => {
                            summary.ok += 1;
                            format!("{}: ok in {secs} s", job.sheet)
                        }
                        Err(e) => {
                            summary.failed += 1;
                            format!("{}: failed in {secs} s: {e}", job.sheet)
                        }
                    };
                    let _ = writeln!(out, "{line}");
                    let rec = serde_json::json!({
                        "sheet": job.sheet, "ok": result.is_ok(), "seconds": secs,
                        "error": result.err(),
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
/// then what the call left of it removed. A call whose image is not recorded keeps it there.
fn call(work: &Work, runner: &dyn Runner, job: &Job) -> Result<(), String> {
    let dir = work::ensure_dir(&work.codex_dir(&job.sheet))?;
    let sheet = work.sheet(&job.sheet);
    let got = runner.generate(&sheet, &prompt::text(job), &dir)?;
    let png = img::read_capped(&got, img::PNG_MAX_BYTES)?;
    record(work, job, &png)?;
    runner.finish(&got);
    Ok(())
}

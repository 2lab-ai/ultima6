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
//! done, and is run again. A call whose image is not a decodable PNG fails and leaves nothing.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
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

    /// After the image at `image` was read: removes what the call left of it.
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
    /// One deadline for the call: the process group, its output and the event reader.
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

/// Kills every process of the group `pgid` (the call and anything it started).
fn kill_group(pgid: u32) {
    let _ = Command::new("/bin/kill")
        .args(["-s", "KILL", "--", &format!("-{pgid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

enum Event {
    Thread(String),
    Done,
}

impl Runner for Codex {
    fn generate(&self, sheet: &Path, prompt: &str, dir: &Path) -> Result<PathBuf, String> {
        use std::os::unix::process::CommandExt;
        let deadline = Instant::now() + self.timeout;
        let (stderr, stderr_tmp, stderr_path) = work::create_temp(&dir.join("stderr.txt"), 0o644)?;
        let (mut events, events_tmp, events_path) =
            work::create_temp(&dir.join("events.jsonl"), 0o644)?;
        let mut child = Command::new(&self.bin)
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
        let pgid = child.id();
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, rx) = mpsc::channel();
        // The reader streams the events to their file (up to EVENTS_MAX) and reports the
        // thread id; it ends when every process of the group has closed the pipe.
        std::thread::spawn(move || {
            let mut kept = 0u64;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line)
                    && v["type"] == "thread.started"
                    && let Some(id) = v["thread_id"].as_str()
                {
                    let _ = tx.send(Event::Thread(id.to_string()));
                }
                if kept + (line.len() as u64) < EVENTS_MAX {
                    kept += line.len() as u64 + 1;
                    let _ = writeln!(events, "{line}");
                }
            }
            let _ = tx.send(Event::Done);
        });
        if let Some(mut stdin) = child.stdin.take()
            && let Err(e) = stdin.write_all(prompt.as_bytes())
        {
            // A call that does not take its prompt is stopped at once, with all it started.
            kill_group(pgid);
            let _ = child.wait();
            let _ = work::commit(&stderr_tmp, &stderr_path);
            let _ = work::commit(&events_tmp, &events_path);
            return Err(format!("the call did not take its prompt: {e}"));
        }
        let status = loop {
            if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
                break Some(s);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        // Whatever the call left running goes with it.
        kill_group(pgid);
        let _ = child.wait();
        let mut thread_id = None;
        let mut ended = false;
        while !ended {
            let left = deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_secs(5));
            match rx.recv_timeout(left) {
                Ok(Event::Thread(id)) => thread_id = Some(id),
                Ok(Event::Done) | Err(mpsc::RecvTimeoutError::Disconnected) => ended = true,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
            }
        }
        let _ = work::commit(&stderr_tmp, &stderr_path);
        let _ = work::commit(&events_tmp, &events_path);
        let Some(status) = status else {
            return Err(format!("timed out after {} s", self.timeout.as_secs()));
        };
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
    /// the image was read (codex's own session log, outside the tool's reach, keeps the sheet
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

/// Records `png` as `job`'s output: refused unless it decodes; the image, then its receipt.
pub fn record(work: &Work, job: &Job, png: &[u8]) -> Result<(), String> {
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

/// One call: the generated image recorded as the job's output.
fn call(work: &Work, runner: &dyn Runner, job: &Job) -> Result<(), String> {
    let dir = work::ensure_dir(&work.codex_dir(&job.sheet))?;
    let sheet = work.sheet(&job.sheet);
    let got = runner.generate(&sheet, &prompt::text(job), &dir)?;
    let png = img::read_capped(&got, img::PNG_MAX_BYTES);
    runner.finish(&got);
    record(work, job, &png?)
}

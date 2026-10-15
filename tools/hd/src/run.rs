//! `run`: one imagegen call per sheet not yet done, 2–4 at a time, through Codex's built-in
//! `image_gen` tool (`codex exec -m gpt-6-astra`, the imagegen skill's default path: the
//! ChatGPT account Codex is signed in to, no API key, never the CLI fallback). The built-in
//! tool saves under `$CODEX_HOME/generated_images/<thread id>/`; the runner copies the image to
//! `out/<sheet>.png`. A sheet is done when that file exists, so a run that stops resumes where
//! it was. Nothing is sent without `--send`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::atlas::Job;
use crate::prompt;
use crate::work::Work;

/// What makes an image from a sheet and a prompt.
pub trait Runner: Sync {
    /// Runs one call with `sheet` attached and `prompt`, `dir` as its working dir; returns the
    /// generated image's path.
    fn generate(&self, sheet: &Path, prompt: &str, dir: &Path) -> Result<PathBuf, String>;
}

/// The model of every call.
pub const MODEL: &str = "gpt-6-astra";

/// `codex exec` with the built-in image tool.
pub struct Codex {
    pub bin: PathBuf,
    /// `$CODEX_HOME` (default `~/.codex`).
    pub home: PathBuf,
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

impl Runner for Codex {
    fn generate(&self, sheet: &Path, prompt: &str, dir: &Path) -> Result<PathBuf, String> {
        let log = std::fs::File::create(dir.join("stderr.txt")).map_err(|e| e.to_string())?;
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
            .stderr(log)
            .spawn()
            .map_err(|e| format!("{}: {e}", self.bin.display()))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .map_err(|e| format!("prompt: {e}"))?;
        }
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let events_path = dir.join("events.jsonl");
        let reader = std::thread::spawn(move || {
            let mut thread_id = None;
            let mut events = String::new();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line)
                    && v["type"] == "thread.started"
                {
                    thread_id = v["thread_id"].as_str().map(str::to_string);
                }
                events.push_str(&line);
                events.push('\n');
            }
            let _ = std::fs::write(events_path, events);
            thread_id
        });
        let start = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
                break s;
            }
            if start.elapsed() > self.timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {} s", self.timeout.as_secs()));
            }
            std::thread::sleep(Duration::from_millis(500));
        };
        let thread_id = reader.join().map_err(|_| "event reader panicked")?;
        if !status.success() {
            return Err(format!("codex exited with {status}"));
        }
        let id = thread_id.ok_or("no thread id in the codex events")?;
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
    let mut pending: Vec<&Job> = jobs
        .iter()
        .filter(|j| !work.out(&j.sheet).exists())
        .collect();
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
                    log.push_str(&rec.to_string());
                    log.push('\n');
                }
            });
        }
    });
    let (_, _, log) = shared.into_inner().unwrap_or_else(|e| e.into_inner());
    let path = work.path("runs.jsonl");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(log.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(summary)
}

/// One call: the generated image copied to `out/<sheet>.png` (through a temporary name).
fn call(work: &Work, runner: &dyn Runner, job: &Job) -> Result<(), String> {
    let dir = work.codex_dir(&job.sheet);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let sheet = work.sheet(&job.sheet);
    let got = runner.generate(&sheet, &prompt::text(job), &dir)?;
    let dest = work.out(&job.sheet);
    let tmp = dest.with_extension("png.tmp");
    std::fs::copy(&got, &tmp).map_err(|e| format!("{}: {e}", got.display()))?;
    std::fs::rename(&tmp, &dest).map_err(|e| format!("{}: {e}", dest.display()))
}

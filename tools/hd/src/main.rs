//! `u6-hd`: the command line of the HD tool (see the library doc for the commands).

use std::path::PathBuf;
use std::process::ExitCode;

use u6_hdtool::atlas::{self, SheetKind};
use u6_hdtool::game::{Game, synthetic};
use u6_hdtool::gate::Thresholds;
use u6_hdtool::group::{self, Class, Item};
use u6_hdtool::run::{Codex, RunOptions};
use u6_hdtool::work::{self, Work};
use u6_hdtool::{img, ingest, pack, run};

const USAGE: &str = "usage:
  u6-hd inventory (--game PATH | --synthetic) [--work DIR]
  u6-hd atlas     (--game PATH | --synthetic) [--work DIR] [--rebuild]
  u6-hd run       [--work DIR] [--send] [--limit N] [--parallel N] [--codex PATH]
  u6-hd ingest    (--game PATH | --synthetic) [--work DIR] [--gate NAME=VALUE]...
  u6-hd pack      [--work DIR] [--out FILE]";

#[derive(Default)]
struct Args {
    game: Option<PathBuf>,
    synthetic: bool,
    work: Option<PathBuf>,
    send: bool,
    limit: Option<usize>,
    parallel: usize,
    codex: Option<PathBuf>,
    out: Option<PathBuf>,
    gates: Thresholds,
    rebuild: bool,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        parallel: 2,
        ..Args::default()
    };
    while let Some(arg) = it.next() {
        let mut value = |what: &str| it.next().ok_or(format!("{arg} needs {what}"));
        match arg.as_str() {
            "--game" => a.game = Some(value("a path")?.into()),
            "--synthetic" => a.synthetic = true,
            "--work" => a.work = Some(value("a directory")?.into()),
            "--send" => a.send = true,
            "--rebuild" => a.rebuild = true,
            "--limit" => {
                let n = value("a number")?;
                a.limit = Some(n.parse().map_err(|_| format!("bad --limit {n:?}"))?);
            }
            "--parallel" => {
                let n = value("2, 3 or 4")?;
                a.parallel = n.parse().map_err(|_| format!("bad --parallel {n:?}"))?;
                if !(2..=4).contains(&a.parallel) {
                    return Err("--parallel is 2, 3 or 4".into());
                }
            }
            "--codex" => a.codex = Some(value("a path")?.into()),
            "--out" => a.out = Some(value("a path")?.into()),
            "--gate" => a.gates.set(&value("name=value")?)?,
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(a)
}

/// The game: the owner's files or the synthetic set. A failed load prints its category only;
/// its details (which can name the owner's files) go to `diagnostic.txt` (0600) in the work dir.
fn game(a: &Args) -> Result<Game, String> {
    match (&a.game, a.synthetic) {
        (Some(p), false) => Game::load(p).map_err(|f| {
            let written = Work::open(a.work.as_deref()).and_then(|w| {
                let text = format!("{}\n{}\n", f.category, f.detail);
                work::write_file_mode(&w.path("diagnostic.txt"), text.as_bytes(), 0o600)?;
                Ok(w.path("diagnostic.txt"))
            });
            match written {
                Ok(p) => format!("{} (details in {})", f.category, p.display()),
                Err(_) => f.category.to_string(),
            }
        }),
        (None, true) => Ok(synthetic::game()),
        _ => Err("give --game PATH (the owner's files) or --synthetic".into()),
    }
}

fn numbered(plan: &group::Plan) -> Vec<(u32, &Item)> {
    plan.items
        .iter()
        .enumerate()
        .map(|(i, it)| (i as u32, it))
        .collect()
}

fn inventory(a: &Args) -> Result<(), String> {
    let g = game(a)?;
    let plan = group::plan(&g);
    println!("inventory (counts only)");
    println!("terrain records: {}", plan.count(Class::Terrain));
    println!(
        "shore records (merged at run time; the scaler draws them): {}",
        plan.shore
    );
    println!(
        "actor frame groups: {} ({} records)",
        plan.groups(Class::Actor),
        plan.count(Class::Actor)
    );
    let largest = plan
        .items
        .iter()
        .filter(|i| i.class == Class::Actor)
        .map(|i| i.images().count())
        .max()
        .unwrap_or(0);
    println!(
        "largest actor frame group: {largest} records (a tile sheet holds {})",
        atlas::GRID.0 * atlas::GRID.1
    );
    println!(
        "multi-record objects: {} ({} records)",
        plan.groups(Class::Multi),
        plan.count(Class::Multi)
    );
    println!("object records: {}", plan.count(Class::Object));
    println!("portraits: {}", plan.count(Class::Portrait));
    println!("bitmaps: {}", plan.count(Class::Bitmap));
    println!("empty records: {}", plan.empty);
    println!(
        "mosaics joined to another that shares a record: {}",
        plan.overlaps
    );
    let unknown = plan
        .items
        .iter()
        .filter_map(|i| i.layout)
        .filter(|l| l.per_facing.is_none())
        .count();
    println!("actor frame groups of no known layout (to the next base tile): {unknown}");
    println!(
        "actor types whose frames overlap another's (joined to its item, paired in order): {}",
        plan.actor_overlaps
    );
    println!("actor spans split over items: {}", plan.split_actors(&g));
    let counts = atlas::sheet_counts(&g, &numbered(&plan));
    let total: usize = counts.iter().map(|(_, n)| n).sum();
    let name = |k: SheetKind| match k {
        SheetKind::Portrait => "portrait",
        SheetKind::Bitmap => "bitmap",
        SheetKind::Terrain => "terrain",
        SheetKind::Tile => "tile",
    };
    let parts: Vec<String> = counts
        .iter()
        .map(|(k, n)| format!("{} {n}", name(*k)))
        .collect();
    println!("sheets: {} (total {total})", parts.join(", "));
    Ok(())
}

fn atlas_cmd(a: &Args) -> Result<(), String> {
    let g = game(a)?;
    let work = Work::open(a.work.as_deref())?;
    let _lock = work.lock()?;
    if work.path("jobs.jsonl").exists() && !a.rebuild {
        return Err(format!(
            "{} exists: the atlas is built (atlas --rebuild makes a new one, and every output \
             made for the old one stops counting)",
            work.path("jobs.jsonl").display()
        ));
    }
    work.mkdirs()?;
    let plan = group::plan(&g);
    let stamp = atlas::Stamp {
        attempt: 1,
        generation: atlas::new_generation(),
        plan: plan.fingerprint(),
    };
    let mut jobs = Vec::new();
    atlas::layout_each(&g, &numbered(&plan), &stamp, 0, |job, sheet| {
        img::write_png(&work.sheet(&job.sheet), &sheet)?;
        jobs.push(job);
        Ok(())
    })?;
    let meta = serde_json::json!({ "generation": stamp.generation, "plan": stamp.plan });
    work.write("atlas.json", meta.to_string().as_bytes())?;
    work.write_jobs(&jobs)?;
    // What an earlier atlas accepted is not this one's.
    work.write("ingest.json", b"[]")?;
    println!(
        "atlas: {} sheets, {} images, in {}",
        jobs.len(),
        jobs.iter().map(|j| j.places.len()).sum::<usize>(),
        work.root.display()
    );
    Ok(())
}

fn run_cmd(a: &Args) -> Result<(), String> {
    let work = Work::open(a.work.as_deref())?;
    let lock = work.lock()?;
    let codex = Codex::from_env(a.codex.clone())?;
    let opts = RunOptions {
        send: a.send,
        limit: a.limit,
        parallel: a.parallel,
    };
    // Ctrl-C, a kill or a closed terminal: no new call, every running one killed and reaped,
    // then the lock let go and the tool ended by that signal.
    run::stop_calls_on_signals();
    let mut out = std::io::stdout();
    let result = run::run(&work, &codex, &opts, &mut out);
    if let Some(sig) = run::stopped_by() {
        drop(lock);
        eprintln!("u6-hd: stopped by signal {sig}; every call it started is ended");
        run::reraise(sig);
    }
    let s = result?;
    if a.send {
        println!(
            "run: {} calls, {} ok ({} not the sheet's size), {} failed",
            s.calls, s.ok, s.other_size, s.failed
        );
    }
    if s.failed > 0 {
        return Err(format!(
            "{} call(s) failed; the sheets that came back are kept; run again to retry the rest \
             (a sheet with {} reserved calls is given up and refused by ingest)",
            s.failed,
            run::CALLS_MAX
        ));
    }
    Ok(())
}

fn ingest_cmd(a: &Args) -> Result<(), String> {
    let g = game(a)?;
    let work = Work::open(a.work.as_deref())?;
    let _lock = work.lock()?;
    let report = ingest::ingest(&g, &work, &a.gates)?;
    print!("{}", report.text());
    Ok(())
}

fn pack_cmd(a: &Args) -> Result<(), String> {
    let work = Work::open(a.work.as_deref())?;
    let _lock = work.lock()?;
    let s = pack::pack(&work, a.out.as_deref())?;
    println!(
        "pack: {} tiles, {} portraits, {} bitmaps, {} bytes (derived: true; never shared)",
        s.tiles, s.portraits, s.bitmaps, s.bytes
    );
    Ok(())
}

fn main() -> ExitCode {
    let mut it = std::env::args().skip(1);
    let Some(cmd) = it.next() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if cmd == "export" {
        eprintln!(
            "export refused: packs made from the game's art are derived (manifest derived: true) \
             and are never exported, hosted or shared (spec 07 Rights)"
        );
        return ExitCode::from(2);
    }
    let result = parse(it).and_then(|a| match cmd.as_str() {
        "inventory" => inventory(&a),
        "atlas" => atlas_cmd(&a),
        "run" => run_cmd(&a),
        "ingest" => ingest_cmd(&a),
        "pack" => pack_cmd(&a),
        _ => Err(format!("unknown command {cmd:?}\n{USAGE}")),
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("u6-hd: {e}");
            ExitCode::FAILURE
        }
    }
}

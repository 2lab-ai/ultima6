//! `ingest`: every sheet that came back is checked for its size (a sheet of another size is
//! refused whole), sliced at the fixed pitch, keyed, colour-transferred and gated image by
//! image; then each item is judged whole (its images, and the terrain seam or the actor motion
//! gate). An item that fails its first attempt is queued for one re-roll (new sheets, attempt
//! 2); one that fails again falls back to the scaler as a whole. The images of passing items
//! are written to `slices/` and listed in `ingest.json` for `pack`; the coverage report counts
//! records by class.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use u6_hdpack::ImageId;

use crate::atlas::{self, Job, MASTER, Place};
use crate::game::Game;
use crate::gate::{self, Metrics, Thresholds};
use crate::group::{self, Class, Item};
use crate::img::{self, Classic, Rgba};
use crate::work::Work;

/// Where an item stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Passed: its images go in the pack.
    Generated,
    /// Failed the gates twice: the scaler draws it.
    Fallback,
    /// No usable output from its last attempt (sheet refused, or the source changed).
    Failed,
    /// Not run yet, or its re-roll not run yet.
    Pending,
}

/// One attempt of one item.
#[derive(Clone, Debug)]
enum Attempt {
    NotRun,
    Refused,
    Judged(Vec<Judged>),
}

#[derive(Clone, Debug)]
struct Judged {
    place: Place,
    id: ImageId,
    classic: Classic,
    hd: Rgba,
    metrics: Metrics,
}

/// A line of `ingest.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accepted {
    pub image: String,
    pub hash: String,
    pub slice: String,
}

/// What ingest found: records per class and status, and what it did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub counts: BTreeMap<(Class, Status), usize>,
    pub shore: usize,
    pub empty: usize,
    pub rerolled_items: usize,
    pub reroll_sheets: usize,
    pub refused_sheets: usize,
    /// Failing gates over every judged image (a name, how many).
    pub failures: BTreeMap<&'static str, usize>,
    /// The thresholds used.
    pub thresholds: String,
}

impl Report {
    pub fn get(&self, class: Class, status: Status) -> usize {
        self.counts.get(&(class, status)).copied().unwrap_or(0)
    }

    /// The coverage report: counts only.
    pub fn text(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "coverage (records, counts only); gates {}",
            self.thresholds
        );
        let _ = writeln!(
            s,
            "{:<30} {:>9} {:>9} {:>7} {:>8}",
            "", "generated", "fallback", "failed", "pending"
        );
        for class in Class::ALL {
            let _ = writeln!(
                s,
                "{:<30} {:>9} {:>9} {:>7} {:>8}",
                class.name(),
                self.get(class, Status::Generated),
                self.get(class, Status::Fallback),
                self.get(class, Status::Failed),
                self.get(class, Status::Pending),
            );
        }
        let _ = writeln!(
            s,
            "shore records (scaler by rule, merged at run time): {}",
            self.shore
        );
        let _ = writeln!(s, "empty records: {}", self.empty);
        let _ = writeln!(
            s,
            "sheets refused (size, unreadable or source changed): {}",
            self.refused_sheets
        );
        let _ = writeln!(
            s,
            "re-rolls queued: {} items in {} sheets",
            self.rerolled_items, self.reroll_sheets
        );
        let fails: Vec<String> = self
            .failures
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect();
        let _ = writeln!(s, "failing gates (images): {}", fails.join(", "));
        s
    }
}

/// The slice of `place` in `out`, at ×8.
pub fn slice(out: &Rgba, place: &Place) -> Rgba {
    let raw = out.rect(
        place.x,
        place.y,
        place.w * place.scale,
        place.h * place.scale,
    );
    if place.scale == MASTER {
        raw
    } else {
        img::resize(&raw, place.w * MASTER, place.h * MASTER)
    }
}

/// Portraits are judged at the layout scale, everything else at the source pixel.
pub fn scale_of(class: Class) -> gate::Scale {
    match class {
        Class::Portrait => gate::Scale::Layout,
        _ => gate::Scale::Source,
    }
}

/// Judges the places of one sheet that came back. `Err` names why the whole sheet is refused.
fn judge_sheet(game: &Game, job: &Job, out: &Rgba) -> Result<Vec<Judged>, &'static str> {
    if (out.width, out.height) != (job.width, job.height) {
        return Err("size");
    }
    let mut v = Vec::with_capacity(job.places.len());
    for p in &job.places {
        let id = atlas::parse_image(&p.image).ok_or("image id")?;
        let classic = game.classic(id).ok_or("source missing")?;
        let hash = game.hash(id).map(|h| format!("{h:016x}"));
        if hash.as_deref() != Some(p.hash.as_str()) {
            return Err("source changed");
        }
        if (classic.width, classic.height) != (p.w, p.h)
            || p.x + p.w * p.scale > out.width
            || p.y + p.h * p.scale > out.height
        {
            return Err("cell map");
        }
        let (hd, metrics) = gate::judge(&slice(out, p), &classic, job.key, scale_of(p.class));
        v.push(Judged {
            place: p.clone(),
            id,
            classic,
            hd,
            metrics,
        });
    }
    Ok(v)
}

/// The classic and HD images of one piece, composed as its block.
fn compose(parts: &[&Judged]) -> (Classic, Rgba) {
    let x0 = parts.iter().map(|j| j.place.x).min().unwrap_or(0);
    let y0 = parts.iter().map(|j| j.place.y).min().unwrap_or(0);
    let cell = parts[0].place.w * parts[0].place.scale;
    let cols = parts
        .iter()
        .map(|j| (j.place.x - x0) / cell + 1)
        .max()
        .unwrap_or(1);
    let rows = parts
        .iter()
        .map(|j| (j.place.y - y0) / cell + 1)
        .max()
        .unwrap_or(1);
    let (w, h) = (parts[0].classic.width, parts[0].classic.height);
    let mut classic = Classic {
        width: w * cols,
        height: h * rows,
        px: vec![None; (w * cols * h * rows) as usize],
    };
    let mut hd = Rgba::new(w * cols * MASTER, h * rows * MASTER);
    for j in parts {
        let (cx, cy) = ((j.place.x - x0) / cell, (j.place.y - y0) / cell);
        for y in 0..h {
            for x in 0..w {
                let to = ((cy * h + y) * classic.width + cx * w + x) as usize;
                classic.px[to] = j.classic.at(x, y);
            }
        }
        hd.paste(&j.hd, cx * w * MASTER, cy * h * MASTER);
    }
    (classic, hd)
}

/// Whether a judged attempt of `item` passes: every image, then its group gate.
fn passes(class: Class, judged: &[Judged], t: &Thresholds) -> bool {
    judged.iter().all(|j| j.metrics.pass(t)) && group_passes(class, judged, t)
}

/// The group gate of an item's images: the terrain seam, the actor motion.
fn group_passes(class: Class, judged: &[Judged], t: &Thresholds) -> bool {
    match class {
        Class::Terrain => judged
            .iter()
            .all(|j| gate::seam_passes(gate::seam(&j.classic, &j.hd, t.seam))),
        Class::Actor => motion_of(judged).is_none_or(|m| m >= t.motion),
        _ => true,
    }
}

/// The motion kept across an actor item's consecutive pieces of one shape.
fn motion_of(judged: &[Judged]) -> Option<f32> {
    let mut pieces: BTreeMap<u32, Vec<&Judged>> = BTreeMap::new();
    for j in judged {
        pieces.entry(j.place.piece).or_default().push(j);
    }
    let composed: Vec<(Classic, Rgba)> = pieces.values().map(|parts| compose(parts)).collect();
    let pairs: Vec<(Classic, Classic, Rgba, Rgba)> = composed
        .windows(2)
        .filter(|w| (w[0].0.width, w[0].0.height) == (w[1].0.width, w[1].0.height))
        .map(|w| {
            (
                w[0].0.clone(),
                w[1].0.clone(),
                w[0].1.clone(),
                w[1].1.clone(),
            )
        })
        .collect();
    gate::motion(&pairs)
}

/// Records of an item that draw something.
fn records(judged_or_planned: impl Iterator<Item = Classic>) -> usize {
    judged_or_planned.filter(Classic::has_opaque).count()
}

/// Runs ingest over the work dir with the thresholds `t`.
pub fn ingest(game: &Game, work: &Work, t: &Thresholds) -> Result<Report, String> {
    let jobs = work.read_jobs()?;
    let plan = group::plan(game);
    let mut report = Report {
        shore: plan.shore,
        empty: plan.empty,
        thresholds: t.text(),
        ..Report::default()
    };
    // item -> attempt -> sheets' results.
    let mut attempts: BTreeMap<u32, BTreeMap<u8, Attempt>> = BTreeMap::new();
    let mut gates_log = String::new();
    for job in &jobs {
        let out = work.out(&job.sheet);
        let result: Result<Vec<Judged>, &'static str> = if !out.exists() {
            for p in &job.places {
                attempts
                    .entry(p.item)
                    .or_default()
                    .insert(job.attempt, Attempt::NotRun);
            }
            continue;
        } else {
            match img::read_png(&out) {
                Ok(o) => judge_sheet(game, job, &o),
                Err(_) => Err("unreadable"),
            }
        };
        match result {
            Err(why) => {
                report.refused_sheets += 1;
                let line =
                    serde_json::json!({"sheet": job.sheet, "attempt": job.attempt, "refused": why});
                let _ = writeln!(gates_log, "{line}");
                for p in &job.places {
                    attempts
                        .entry(p.item)
                        .or_default()
                        .insert(job.attempt, Attempt::Refused);
                }
            }
            Ok(judged) => {
                for j in judged {
                    for f in j.metrics.failures(t) {
                        *report.failures.entry(f).or_default() += 1;
                    }
                    let m = &j.metrics;
                    let seam = (j.place.class == Class::Terrain)
                        .then(|| gate::seam(&j.classic, &j.hd, t.seam));
                    let line = serde_json::json!({
                        "sheet": job.sheet, "attempt": job.attempt, "item": j.place.item,
                        "image": j.place.image, "pass": m.pass(t),
                        "structure": m.structure, "shift": m.shift, "iou": m.iou,
                        "purity": m.purity, "drift_raw": m.drift_raw, "drift": m.drift,
                        "seam": seam, "gates": t.text(),
                        "structure_scale": scale_of(j.place.class).name(),
                    });
                    let _ = writeln!(gates_log, "{line}");
                    let slot = attempts
                        .entry(j.place.item)
                        .or_default()
                        .entry(job.attempt)
                        .or_insert(Attempt::Judged(Vec::new()));
                    match slot {
                        Attempt::Judged(v) => v.push(j),
                        // Another sheet of this attempt did not come back or was refused.
                        Attempt::NotRun | Attempt::Refused => {}
                    }
                }
            }
        }
    }

    let mut accepted: Vec<Accepted> = Vec::new();
    let mut reroll: Vec<u32> = Vec::new();
    for (no, item) in plan.items.iter().enumerate() {
        let no = no as u32;
        let n_records = records(item.images().filter_map(|id| game.classic(id)));
        let tries = attempts.get(&no);
        let status = match tries {
            None => Status::Pending,
            Some(tries) => {
                let mut status = Status::Pending;
                let mut passed: Option<&Vec<Judged>> = None;
                for (&attempt, a) in tries {
                    let Attempt::Judged(v) = a else { continue };
                    if v.len() != item.images().count() {
                        continue;
                    }
                    if item.class == Class::Actor {
                        let line = serde_json::json!({
                            "item": no, "attempt": attempt, "class": "actor",
                            "motion": motion_of(v), "gates": t.text(),
                        });
                        let _ = writeln!(gates_log, "{line}");
                    }
                    if passes(item.class, v, t) {
                        passed = Some(v);
                        break;
                    }
                }
                if let Some(v) = passed {
                    for j in v.iter().filter(|j| j.classic.has_opaque()) {
                        let stem = atlas::slice_name(j.id);
                        img::write_png(&work.slice(&stem), &j.hd)?;
                        accepted.push(Accepted {
                            image: j.place.image.clone(),
                            hash: j.place.hash.clone(),
                            slice: format!("slices/{stem}.png"),
                        });
                    }
                    status = Status::Generated;
                } else if let Some((&last, a)) = tries.iter().next_back() {
                    status = match (last, a) {
                        (_, Attempt::NotRun) => Status::Pending,
                        (1, _) => {
                            reroll.push(no);
                            Status::Pending
                        }
                        (_, Attempt::Refused) => Status::Failed,
                        (_, Attempt::Judged(_)) => Status::Fallback,
                    };
                }
                status
            }
        };
        *report.counts.entry((item.class, status)).or_default() += n_records;
    }

    if !reroll.is_empty() {
        let items: Vec<(u32, &Item)> = reroll
            .iter()
            .map(|&no| (no, &plan.items[no as usize]))
            .collect();
        let mut new_jobs: Vec<Job> = Vec::new();
        atlas::layout_each(game, &items, 2, jobs.len(), |job, sheet| {
            img::write_png(&work.sheet(&job.sheet), &sheet)?;
            new_jobs.push(job);
            Ok(())
        })?;
        work.append_jobs(&new_jobs)?;
        report.rerolled_items = reroll.len();
        report.reroll_sheets = new_jobs.len();
    }
    work.write("gates.jsonl", gates_log.as_bytes())?;
    let json = serde_json::to_vec_pretty(&accepted).map_err(|e| e.to_string())?;
    work.write("ingest.json", &json)?;
    work.write("coverage.txt", report.text().as_bytes())?;
    Ok(report)
}

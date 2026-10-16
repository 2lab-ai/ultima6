//! `ingest`: every sheet that came back is checked for its size (a sheet of another size is
//! refused whole, before it is decoded, as is a sheet given up after `run::CALLS_MAX` paid
//! calls or one whose sheet cannot be read), sliced at the fixed pitch, keyed, colour-transferred and gated image by
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
use crate::run;
use crate::work::{self, Work};

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

/// What the sheets of one attempt of one item came to.
#[derive(Clone, Debug, Default)]
struct Attempt {
    judged: Vec<Judged>,
    /// A sheet of it has no done output yet.
    missing: bool,
    /// A sheet of it was refused (size, decode, source, cell map).
    refused: bool,
}

/// The outcome of an attempt, in this precedence: refused, then not run, then judged.
enum Outcome<'a> {
    Refused,
    NotRun,
    Judged(&'a [Judged]),
}

#[derive(Clone, Debug)]
pub(crate) struct Judged {
    /// The sheet it was on (a piece's images share one).
    pub(crate) sheet: String,
    pub(crate) place: Place,
    pub(crate) id: ImageId,
    pub(crate) classic: Classic,
    pub(crate) hd: Rgba,
    pub(crate) metrics: Metrics,
}

/// A line of `ingest.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accepted {
    pub image: String,
    pub hash: String,
    /// The ×8 master's size.
    pub width: u32,
    pub height: u32,
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
            sheet: job.sheet.clone(),
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
fn passes(item: &Item, judged: &[Judged], t: &Thresholds) -> bool {
    judged.iter().all(|j| j.metrics.pass(t)) && group_passes(item, judged, t)
}

/// The group gate of an item's images: the terrain seam, the actor motion.
fn group_passes(item: &Item, judged: &[Judged], t: &Thresholds) -> bool {
    match item.class {
        Class::Terrain => judged
            .iter()
            .all(|j| gate::seam_passes(gate::seam(&j.classic, &j.hd, t.seam))),
        Class::Actor => motion_of(item, judged).is_none_or(|m| m.is_finite() && m >= t.motion),
        _ => true,
    }
}

/// The motion kept between an actor item's frames of one facing: consecutive pieces of one
/// shape whose frames (anchor record − base tile) fall in the same facing; a turn is not
/// motion. A type of unknown layout pairs every consecutive piece (under the minimum a turn
/// pair can only lower the result).
pub(crate) fn motion_of(item: &Item, judged: &[Judged]) -> Option<f32> {
    let mut pieces: BTreeMap<u32, Vec<&Judged>> = BTreeMap::new();
    for j in judged {
        pieces.entry(j.place.piece).or_default().push(j);
    }
    let facing = |k: u32| -> Option<u32> {
        let l = item.layout?;
        let per = u32::from(l.per_facing?);
        let anchor = item.pieces.get(k as usize)?.images.last()?;
        let ImageId::Tile(r) = *anchor else {
            return None;
        };
        Some(u32::from(r.checked_sub(l.base)?) / per.max(1))
    };
    let known = item.layout.is_some_and(|l| l.per_facing.is_some());
    let composed: Vec<(Option<u32>, Classic, Rgba)> = pieces
        .iter()
        .map(|(&k, parts)| {
            let (c, h) = compose(parts);
            (facing(k), c, h)
        })
        .collect();
    let pairs: Vec<(Classic, Classic, Rgba, Rgba)> = composed
        .windows(2)
        .filter(|w| (w[0].1.width, w[0].1.height) == (w[1].1.width, w[1].1.height))
        .filter(|w| !known || (w[0].0.is_some() && w[0].0 == w[1].0))
        .map(|w| {
            (
                w[0].1.clone(),
                w[1].1.clone(),
                w[0].2.clone(),
                w[1].2.clone(),
            )
        })
        .collect();
    gate::motion(&pairs)
}

/// Records of an item that draw something.
fn records(judged_or_planned: impl Iterator<Item = Classic>) -> usize {
    judged_or_planned.filter(Classic::has_opaque).count()
}

/// Whether `judged` is exactly `item`: every image of every piece once, in its piece, at its
/// place in the piece's block of cells, and nothing else.
fn matches_item(item: &Item, judged: &[Judged]) -> bool {
    if judged.len() != item.images().count() {
        return false;
    }
    for (k, piece) in item.pieces.iter().enumerate() {
        let of: Vec<&Judged> = judged
            .iter()
            .filter(|j| j.place.piece as usize == k)
            .collect();
        if of.len() != piece.images.len() {
            return false;
        }
        let Some(first) = of.iter().find(|j| j.id == piece.images[0]) else {
            return false;
        };
        let (cw, ch) = (
            first.place.w * first.place.scale,
            first.place.h * first.place.scale,
        );
        for (i, &id) in piece.images.iter().enumerate() {
            let (dx, dy) = (i as u32 % piece.w, i as u32 / piece.w);
            let here = of.iter().filter(|j| j.id == id).collect::<Vec<_>>();
            let [j] = here.as_slice() else { return false };
            let at = (first.place.x + dx * cw, first.place.y + dy * ch);
            if (j.place.x, j.place.y) != at || j.sheet != first.sheet {
                return false;
            }
        }
    }
    true
}

/// Runs ingest over the work dir with the thresholds `t`.
pub fn ingest(game: &Game, work: &Work, t: &Thresholds) -> Result<Report, String> {
    let jobs = work.read_jobs()?;
    // Checked before anything is written there, and again before it is emptied.
    work::own_dir(&work.path("slices"))?;
    let plan = group::plan(game);
    let fingerprint = plan.fingerprint();
    let generation = jobs
        .first()
        .map(|j| j.generation.clone())
        .unwrap_or_default();
    if jobs
        .iter()
        .any(|j| j.plan != fingerprint || j.generation != generation)
    {
        return Err(
            "the atlas was built from other files or by another version of the tool; \
             rebuild it (atlas --rebuild)"
                .into(),
        );
    }
    let mut report = Report {
        shore: plan.shore,
        empty: plan.empty,
        thresholds: t.text(),
        ..Report::default()
    };
    let book = run::ledger(work)?;
    // item -> attempt -> what its sheets came to.
    let mut attempts: BTreeMap<u32, BTreeMap<u8, Attempt>> = BTreeMap::new();
    let mut gates_log = String::new();
    for job in &jobs {
        let mark = |attempts: &mut BTreeMap<u32, BTreeMap<u8, Attempt>>,
                    f: &dyn Fn(&mut Attempt)| {
            for p in &job.places {
                f(attempts
                    .entry(p.item)
                    .or_default()
                    .entry(job.attempt)
                    .or_default());
            }
        };
        let Some((png, w, h)) = run::output(work, job) else {
            // Given up after CALLS_MAX paid calls, or a sheet that cannot be read (so never
            // sent): refused as a sheet that came back unusable.
            let why = match run::standing(work, job, &book) {
                run::Standing::Open(_) => None,
                run::Standing::GivenUp => Some("calls"),
                run::Standing::Unreadable(_) => Some("sheet"),
            };
            if let Some(why) = why {
                report.refused_sheets += 1;
                let line =
                    serde_json::json!({"sheet": job.sheet, "attempt": job.attempt, "refused": why});
                let _ = writeln!(gates_log, "{line}");
                mark(&mut attempts, &|a| a.refused = true);
            } else {
                mark(&mut attempts, &|a| a.missing = true);
            }
            continue;
        };
        // Its receipt's size, before any decode: an image of another size is refused unread.
        let result = if (w, h) != (job.width, job.height) {
            Err("size")
        } else {
            match img::decode_png(&png, w, h) {
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
                mark(&mut attempts, &|a| a.refused = true);
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
                        "purity": m.purity, "flat": m.flat, "drift_raw": m.drift_raw,
                        "drift": m.drift, "seam": seam, "gates": t.text(),
                        "structure_scale": scale_of(j.place.class).name(),
                    });
                    let _ = writeln!(gates_log, "{line}");
                    attempts
                        .entry(j.place.item)
                        .or_default()
                        .entry(job.attempt)
                        .or_default()
                        .judged
                        .push(j);
                }
            }
        }
    }

    let mut accepted: Vec<Accepted> = Vec::new();
    let mut reroll: Vec<u32> = Vec::new();
    for (no, item) in plan.items.iter().enumerate() {
        let no = no as u32;
        let n_records = records(item.images().filter_map(|id| game.classic(id)));
        let outcomes: Vec<(u8, Outcome)> = attempts
            .get(&no)
            .map(|tries| {
                tries
                    .iter()
                    .map(|(&k, a)| {
                        let o = if a.refused || (!a.missing && !matches_item(item, &a.judged)) {
                            Outcome::Refused
                        } else if a.missing {
                            Outcome::NotRun
                        } else {
                            Outcome::Judged(&a.judged)
                        };
                        (k, o)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut passed: Option<&[Judged]> = None;
        for (attempt, o) in &outcomes {
            let Outcome::Judged(v) = o else { continue };
            if item.class == Class::Actor {
                let line = serde_json::json!({
                    "item": no, "attempt": attempt, "class": "actor",
                    "motion": motion_of(item, v), "gates": t.text(),
                });
                let _ = writeln!(gates_log, "{line}");
            }
            if passes(item, v, t) {
                passed = Some(v);
                break;
            }
        }
        let status = if let Some(v) = passed {
            for j in v.iter().filter(|j| j.classic.has_opaque()) {
                let stem = atlas::slice_name(j.id);
                img::write_png(&work.slice(&stem), &j.hd)?;
                accepted.push(Accepted {
                    image: j.place.image.clone(),
                    hash: j.place.hash.clone(),
                    width: j.hd.width,
                    height: j.hd.height,
                    slice: format!("slices/{stem}.png"),
                });
            }
            Status::Generated
        } else {
            match outcomes.last() {
                None | Some((_, Outcome::NotRun)) => Status::Pending,
                Some((1, _)) => {
                    reroll.push(no);
                    Status::Pending
                }
                Some((_, Outcome::Refused)) => Status::Failed,
                Some((_, Outcome::Judged(_))) => Status::Fallback,
            }
        };
        *report.counts.entry((item.class, status)).or_default() += n_records;
    }

    if !reroll.is_empty() {
        let items: Vec<(u32, &Item)> = reroll
            .iter()
            .map(|&no| (no, &plan.items[no as usize]))
            .collect();
        let stamp = atlas::Stamp {
            attempt: 2,
            generation,
            plan: fingerprint,
        };
        let mut new_jobs: Vec<Job> = Vec::new();
        atlas::layout_each(game, &items, &stamp, jobs.len(), |job, sheet| {
            img::write_png(&work.sheet(&job.sheet), &sheet)?;
            new_jobs.push(job);
            Ok(())
        })?;
        work.append_jobs(&new_jobs)?;
        report.rerolled_items = reroll.len();
        report.reroll_sheets = new_jobs.len();
    }
    // A master no longer accepted (its group failed since, or the atlas was rebuilt) goes.
    let keep: std::collections::BTreeSet<String> =
        accepted.iter().map(|a| a.slice.clone()).collect();
    let slices = work::own_dir(&work.path("slices"))?;
    let entries = std::fs::read_dir(&slices).map_err(|e| format!("{}: {e}", slices.display()))?;
    for e in entries {
        let e = e.map_err(|e| format!("{}: {e}", slices.display()))?;
        let rel = format!("slices/{}", e.file_name().to_string_lossy());
        // The entry itself (a symlink is not followed, and not removed either).
        let plain = e.file_type().is_ok_and(|t| t.is_file());
        if plain && !keep.contains(&rel) {
            std::fs::remove_file(slices.join(e.file_name()))
                .map_err(|err| format!("{rel}: {err}"))?;
        }
    }
    work.write("gates.jsonl", gates_log.as_bytes())?;
    let json = serde_json::to_vec_pretty(&accepted).map_err(|e| e.to_string())?;
    work.write("ingest.json", &json)?;
    work.write("coverage.txt", report.text().as_bytes())?;
    Ok(report)
}

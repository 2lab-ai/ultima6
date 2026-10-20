//! `ingest`: every sheet that came back is checked for its size (a sheet of another size is
//! refused whole, before it is decoded, as is a sheet given up after `run::CALLS_MAX` reserved
//! calls or one whose sheet cannot be read), sliced at the fixed pitch, keyed, colour-transferred and gated image by
//! image; then each item is judged whole (its images, and the terrain seam or the actor motion
//! gate). An item that fails its first attempt is queued for one re-roll (new sheets, attempt
//! 2); one that fails again falls back to the scaler as a whole. After every item is judged, the
//! neighbour-seam gate ([`crate::neighbour`]) sends the accepted items whose records meet their
//! neighbours in the world with a seam back to the scaler, to a fixed point. The images of the
//! items left accepted are written to `slices/` and listed in `ingest.json` for `pack`; the
//! coverage report counts records by class, the neighbour fallbacks apart, and the pairs: all of
//! them, then those checked, unmeasured and failing, which are only pairs with a generated side
//! (a pair of two scaled records is not checked; an unmeasured pair is checked, but nothing is
//! measured on it, which on the game's data means no row across the join both sides draw, so
//! it cannot fail). `gates.jsonl` has a line per pair
//! checked in the first pass (an unmeasured one with a null ratio) and per failing pair of a
//! later pass. The owner's list (`ingest --exclude FILE`, [`Exclude`]) sends every item that holds
//! a record it names back to the scaler whole, after the items are judged and before the
//! neighbour gate, which sees its records as scaled sides. Ingest reads the sheets that came back
//! and calls nothing: no `codex`, no upload.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};
use u6_hdpack::ImageId;

use crate::atlas::{self, Job, MASTER, Place};
use crate::game::Game;
use crate::gate::{self, Metrics, Thresholds};
use crate::group::{self, Class, Item};
use crate::img::{self, Classic, Rgba};
use crate::work::{self, Work};
use crate::{neighbour, run};

/// Where an item stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Passed: its images go in the pack.
    Generated,
    /// Failed the gates twice: the scaler draws it.
    Fallback,
    /// Passed its gates, then a record of it met a neighbour with a seam (the neighbour gate):
    /// the scaler draws it.
    Neighbour,
    /// No usable output from its last attempt (sheet refused, or the source changed).
    Failed,
    /// Not run yet, or its re-roll not run yet.
    Pending,
    /// Named by the owner's list (`ingest --exclude`), whatever it came to: the scaler draws it,
    /// every record of it, and it is not re-rolled.
    Excluded,
}

/// The owner's list (`ingest --exclude FILE`): the tile records it names, each with its line.
/// Every item that holds one goes back to the scaler whole. The list is the owner's local file,
/// read only outside every repository; its errors name a line, never a record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exclude {
    /// (line, record), in line order.
    pub records: Vec<(usize, u16)>,
}

impl Exclude {
    /// Reads the list at `path`; refused, unread, when `path` or a directory above it (as given
    /// or resolved) holds a `.git` entry.
    pub fn read(path: &Path) -> Result<Exclude, String> {
        if in_a_work_tree(path) {
            return Err(
                "the exclude list is inside a git work tree (a directory above it \
                        holds .git); keep it outside every repository (spec 07 Rights)"
                    .into(),
            );
        }
        let text =
            std::fs::read(path).map_err(|e| format!("the exclude list cannot be read: {e}"))?;
        Exclude::parse(&text)
    }

    /// The list's text: one tile record a line, `t:N` or `N` (decimal); `#` to the line's end is
    /// a comment; blank lines are skipped; a record listed twice is an error.
    pub fn parse(text: &[u8]) -> Result<Exclude, String> {
        let text = text.strip_prefix(b"\xef\xbb\xbf").unwrap_or(text);
        let mut first: BTreeMap<u16, usize> = BTreeMap::new();
        let mut records = Vec::new();
        for (k, raw) in text.split(|&b| b == b'\n').enumerate() {
            let line = k + 1;
            let bad = |why: &str| format!("the exclude list, line {line}: {why}");
            let raw = std::str::from_utf8(raw).map_err(|_| bad("not text"))?;
            let id = raw.split('#').next().unwrap_or_default().trim();
            if id.is_empty() {
                continue;
            }
            let digits = id.strip_prefix("t:").unwrap_or(id);
            let r = Some(digits)
                .filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|d| d.parse::<u16>().ok())
                .ok_or_else(|| bad("not a tile record id (t:N or N)"))?;
            if let Some(was) = first.insert(r, line) {
                return Err(bad(&format!("a record listed again (first on line {was})")));
            }
            records.push((line, r));
        }
        Ok(Exclude { records })
    }

    /// The numbers of the plan items that hold a listed record; a listed record in no item is an
    /// error (the first such line).
    pub fn items(&self, plan: &group::Plan) -> Result<BTreeSet<u32>, String> {
        let mut owner: BTreeMap<u16, u32> = BTreeMap::new();
        for (no, item) in plan.items.iter().enumerate() {
            for id in item.images() {
                if let ImageId::Tile(r) = id {
                    owner.insert(r, no as u32);
                }
            }
        }
        self.records
            .iter()
            .map(|&(line, r)| {
                owner.get(&r).copied().ok_or_else(|| {
                    format!(
                        "the exclude list, line {line}: a record in no item of the plan (the \
                         shore, an empty record or none of the game's)"
                    )
                })
            })
            .collect()
    }
}

/// Whether `path` or a directory above it, as given (made absolute) or resolved through its
/// symlinks, holds a `.git` entry.
fn in_a_work_tree(path: &Path) -> bool {
    let given = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let resolved = given.canonicalize().ok();
    std::iter::once(given).chain(resolved).any(|p| {
        p.ancestors()
            .any(|a| std::fs::symlink_metadata(a.join(".git")).is_ok())
    })
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
    /// Items the owner's list sent back (their records are counted as [`Status::Excluded`]).
    pub excluded_items: usize,
    /// Failing gates over every judged image (a name, how many).
    pub failures: BTreeMap<&'static str, usize>,
    /// The neighbour gate's pairs and passes.
    pub neighbour: neighbour::Counts,
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
            "{:<30} {:>9} {:>9} {:>9} {:>7} {:>8} {:>9}",
            "", "generated", "fallback", "neighbour", "failed", "pending", "excluded"
        );
        for class in Class::ALL {
            let _ = writeln!(
                s,
                "{:<30} {:>9} {:>9} {:>9} {:>7} {:>8} {:>9}",
                class.name(),
                self.get(class, Status::Generated),
                self.get(class, Status::Fallback),
                self.get(class, Status::Neighbour),
                self.get(class, Status::Failed),
                self.get(class, Status::Pending),
                self.get(class, Status::Excluded),
            );
        }
        let excluded: usize = Class::ALL
            .iter()
            .map(|&c| self.get(c, Status::Excluded))
            .sum();
        let _ = writeln!(
            s,
            "excluded by the owner's list (the scaler draws them whole): {} items, {excluded} \
             records",
            self.excluded_items
        );
        let n = &self.neighbour;
        let _ = writeln!(
            s,
            "neighbour pairs (distinct records in edge-adjacent cells): {} (map {}, objects {})",
            n.pairs, n.map, n.objects
        );
        let _ = writeln!(
            s,
            "neighbour pairs checked (only pairs with a generated side): {} before the fixed \
             point, {} after",
            n.checked, n.checked_after
        );
        let _ = writeln!(
            s,
            "neighbour pairs unmeasured (checked, but no row across the join both sides draw): \
             {} before the fixed point, {} after",
            n.unmeasured, n.unmeasured_after
        );
        let _ = writeln!(
            s,
            "neighbour pairs failing (among pairs with a generated side): {} before the fixed \
             point, {} after ({} passes)",
            n.failing_before, n.failing_after, n.passes
        );
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

/// The neighbour gate over the judged items ([`neighbour::settle`]): the pairs of the world, each
/// record's images ([`neighbour::sides`]: an accepted record's ×8 master box-averaged to the
/// pack's ×4, as `pack` writes it; MMPX ×4 of the classic record), then the fixed point. Its
/// counts carry the pairs'; the pairs come back with it for where each occurs.
fn neighbour_gate(
    game: &Game,
    judged: &[(Status, usize, Option<&[Judged]>)],
    t: &Thresholds,
) -> Result<(neighbour::Pairs, neighbour::Settled), String> {
    let pairs = neighbour::pairs(game);
    // The records the pack would hold: the drawing records of the accepted items.
    let mut accepted: BTreeMap<u16, (u32, &Rgba)> = BTreeMap::new();
    for (no, &(status, _, passed)) in judged.iter().enumerate() {
        if status != Status::Generated {
            continue;
        }
        for j in passed.into_iter().flatten() {
            if let ImageId::Tile(r) = j.id
                && j.classic.has_opaque()
            {
                accepted.insert(r, (no as u32, &j.hd));
            }
        }
    }
    let sides = neighbour::sides(game, &pairs.all, &accepted)?;
    let mut settled = neighbour::settle(&pairs.all, &sides, t.seam);
    settled.counts.pairs = pairs.all.len();
    settled.counts.map = pairs.map;
    settled.counts.objects = pairs.objects;
    Ok((pairs, settled))
}

/// Runs ingest over the work dir with the thresholds `t`.
pub fn ingest(game: &Game, work: &Work, t: &Thresholds) -> Result<Report, String> {
    ingest_excluding(game, work, t, None)
}

/// [`ingest`] with the owner's list at `exclude` ([`Exclude`]), read and checked against the
/// plan before anything is written; `None` is [`ingest`].
pub fn ingest_excluding(
    game: &Game,
    work: &Work,
    t: &Thresholds,
    exclude: Option<&Path>,
) -> Result<Report, String> {
    let list = exclude.map(Exclude::read).transpose()?;
    let jobs = work.read_jobs()?;
    let plan = group::plan(game);
    let excluded = match &list {
        Some(l) => l.items(&plan)?,
        None => BTreeSet::new(),
    };
    // Checked before anything is written there, and again before it is emptied.
    work::own_dir(&work.path("slices"))?;
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
        excluded_items: excluded.len(),
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
            // Given up after CALLS_MAX reserved calls, or a sheet that cannot be read (so never
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

    let mut reroll: Vec<u32> = Vec::new();
    // Per item: its status, its records that draw, and the attempt that passed.
    let mut judged: Vec<(Status, usize, Option<&[Judged]>)> = Vec::with_capacity(plan.items.len());
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
        let status = if passed.is_some() {
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
        judged.push((status, n_records, passed));
    }

    // The owner's list, after every item is judged and before the neighbour gate: each item that
    // holds a listed record goes back to the scaler whole, whatever it came to (the gate sees its
    // records as MMPX sides), and is not re-rolled.
    for &no in &excluded {
        judged[no as usize].0 = Status::Excluded;
        let line = serde_json::json!({
            "item": no, "class": plan.items[no as usize].class, "fallback": "excluded",
            "gates": t.text(),
        });
        let _ = writeln!(gates_log, "{line}");
    }
    reroll.retain(|no| !excluded.contains(no));

    // The neighbour gate, once every item is judged: its fallbacks are final for this ingest
    // (not re-rolled), and only the items it leaves accepted reach the pack.
    let (pairs, settled) = neighbour_gate(game, &judged, t)?;
    // Every pair checked in the first pass, and each failing pair of a later pass: its records,
    // edge, ratio and limit (null when nothing is measured), its generated sides, its source and
    // one cell of A where it occurs (the map's when the map has the pair).
    for c in &settled.checked {
        let seen = pairs.seen.get(&c.pair).copied().unwrap_or_default();
        let at = seen
            .example()
            .map(|a| serde_json::json!({"z": a.z, "x": a.x, "y": a.y}));
        let line = serde_json::json!({
            "neighbour": {
                "a": atlas::image_name(ImageId::Tile(c.pair.a)),
                "b": atlas::image_name(ImageId::Tile(c.pair.b)),
                "edge": c.pair.edge,
            },
            "pass": c.pass, "fail": c.fails,
            "ratio": c.result.map(|r| r.0), "limit": c.result.map(|r| r.1),
            "sides": if c.both { "gen-gen" } else { "gen-mmpx" },
            "source": seen.source(), "at": at, "gates": t.text(),
        });
        let _ = writeln!(gates_log, "{line}");
    }
    for (&no, &pass) in &settled.fallen {
        judged[no as usize].0 = Status::Neighbour;
        let line = serde_json::json!({
            "item": no, "class": plan.items[no as usize].class, "fallback": "neighbour",
            "pass": pass, "gates": t.text(),
        });
        let _ = writeln!(gates_log, "{line}");
    }
    report.neighbour = settled.counts;

    let mut accepted: Vec<Accepted> = Vec::new();
    for (item, &(status, n_records, passed)) in plan.items.iter().zip(&judged) {
        if status == Status::Generated
            && let Some(v) = passed
        {
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
        }
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

//! `calibrate --final`: the fixed point's state read back from `gates.jsonl`, `ingest.json` and
//! `slices/`, checked against itself before anything is drawn (the module docs of
//! [`crate::calibrate`] say what is read, how the last pass is rebuilt and what is drawn).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;

use serde::Deserialize;
use u6_formats::gfx::TILE_COUNT;
use u6_hdpack::ImageId;
use u6_render::hd::Scale;

use super::{
    A_CELL, BACK, COLS, Framed, LABELS, Layout, MAX_HEIGHT, PANEL_NAMES, Panels, ROWS, RawAt,
    RawPair, Sides, Slices, Source, Stack, edge_name, frame, framed, labels, mark, masters,
    measure, pair_line, panels, parse_edge, stacks, text,
};
use crate::atlas;
use crate::game::Game;
use crate::gate::{self, Edge};
use crate::group::{self, Class, Piece, Plan};
use crate::img::{self, Rgba};
use crate::ingest::Accepted;
use crate::neighbour::{self, At, Drawn, Pair};
use crate::work::{self, Work};

#[cfg(test)]
mod tests;

/// A pair line of `gates.jsonl`, of any pass.
#[derive(Clone, Debug, PartialEq)]
pub struct Logged {
    /// Its line in the file, 1 the first.
    pub line: usize,
    pub pass: usize,
    pub pair: Pair,
    pub fail: bool,
    /// (ratio, limit); `None` when nothing was measured.
    pub result: Option<(f32, f32)>,
    pub sides: Sides,
    /// Its labels, `+` between them ([`labels`]).
    pub labels: String,
    pub at: Option<At>,
    /// The thresholds it was gated with, as ingest writes them.
    pub gates: Option<String>,
}

/// A neighbour fallback line of `gates.jsonl`: the item the gate sent back to MMPX, its class
/// as ingest names it and the pass that failed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fell {
    pub line: usize,
    pub item: u32,
    pub class: String,
    pub pass: usize,
}

/// What `gates.jsonl` says of the neighbour gate: its pair lines and its fallbacks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub pairs: Vec<Logged>,
    pub fell: Vec<Fell>,
}

#[derive(Deserialize)]
struct RawLogged {
    neighbour: RawPair,
    pass: usize,
    fail: bool,
    ratio: Option<f32>,
    limit: Option<f32>,
    sides: String,
    at: Option<RawAt>,
    gates: Option<String>,
}

#[derive(Deserialize)]
struct RawFell {
    item: u32,
    class: String,
    pass: usize,
}

/// The neighbour gate's lines of `gates.jsonl` (`text`): every pair line, of every pass, and
/// every neighbour fallback; the other lines (the images' gates, the actors' motion, refused
/// sheets: no `neighbour` field) are not read. A `neighbour` field that is not a pair is
/// refused ([`pair_line`]): the run stops before anything is drawn, as on any other pair line
/// it cannot read.
pub fn record(text: &str) -> Result<Record, String> {
    let mut out = Record::default();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let bad = |e: &dyn std::fmt::Display| format!("gates.jsonl line {}: {e}", i + 1);
        let v: serde_json::Value = serde_json::from_str(line).map_err(|e| bad(&e))?;
        if v.get("fallback").and_then(serde_json::Value::as_str) == Some("neighbour") {
            let f: RawFell = serde_json::from_value(v).map_err(|e| bad(&e))?;
            out.fell.push(Fell {
                line: i + 1,
                item: f.item,
                class: f.class,
                pass: f.pass,
            });
            continue;
        }
        if !pair_line(&v).map_err(|e| bad(&e))? {
            continue;
        }
        let labels = labels(&v);
        if labels.is_empty() {
            return Err(bad(&"a pair with no label (source, label or labels)"));
        }
        let r: RawLogged = serde_json::from_value(v).map_err(|e| bad(&e))?;
        let record = |s: &str| match atlas::parse_image(s) {
            Some(ImageId::Tile(t)) => Ok(t),
            _ => Err(bad(&"not a pair of tile records")),
        };
        let sides = match r.sides.as_str() {
            "gen-gen" => Sides::GenGen,
            "gen-mmpx" => Sides::GenMmpx,
            _ => return Err(bad(&"sides is neither gen-gen nor gen-mmpx")),
        };
        let result = match (r.ratio, r.limit) {
            (Some(ratio), Some(limit)) => Some((ratio, limit)),
            (None, None) => None,
            _ => {
                return Err(bad(
                    &"a ratio without its limit, or a limit without its ratio",
                ));
            }
        };
        out.pairs.push(Logged {
            line: i + 1,
            pass: r.pass,
            pair: Pair {
                a: record(&r.neighbour.a)?,
                b: record(&r.neighbour.b)?,
                edge: parse_edge(&r.neighbour.edge).ok_or_else(|| bad(&"no edge"))?,
            },
            fail: r.fail,
            result,
            sides,
            labels,
            at: r.at.map(|a| At {
                z: a.z,
                x: a.x,
                y: a.y,
            }),
            gates: r.gates,
        });
    }
    Ok(out)
}

/// A pair of the last pass: what the fixed point left of a pair checked in pass 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Last {
    pub pair: Pair,
    /// Its sides in the last pass (by the slices) and its measure there.
    pub sides: Sides,
    pub result: Option<(f32, f32)>,
    pub labels: String,
    pub at: Option<At>,
    /// Its sides and measure as pass 1 logged them.
    pub first: (Sides, Option<(f32, f32)>),
}

impl Last {
    /// Whether it has label `label` (one of its labels, `+` between them).
    pub fn from(&self, label: &str) -> bool {
        self.labels.split('+').any(|l| l == label)
    }

    /// Ratio over limit; nothing measured is the least.
    pub fn excess(&self) -> f32 {
        match self.result {
            Some((r, l)) if l > 0.0 => r / l,
            Some(_) => f32::INFINITY,
            None => f32::NEG_INFINITY,
        }
    }
}

/// An item the fixed point left generated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Survivor {
    /// Its number in the plan (ingest's item).
    pub item: u32,
    pub class: Class,
    /// Its images that draw: the ones the pack has of it.
    pub images: Vec<ImageId>,
    /// Of them, the tile records.
    pub records: Vec<u16>,
}

/// The fixed point's state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fixed {
    /// The last pass (the one that failed nothing), and whether `gates.jsonl` logs it (pass 1
    /// when nothing fell; otherwise rebuilt from the slices, and checked against the log when
    /// it has the pass's lines).
    pub passes: usize,
    pub logged_last: bool,
    /// Pairs checked in pass 1.
    pub first: usize,
    /// The pairs of the last pass, in pair order: those of pass 1 with a side still generated.
    pub last: Vec<Last>,
    pub survivors: Vec<Survivor>,
    /// Records with a slice; records sent back to MMPX, with their item and the pass.
    pub generated: BTreeSet<u16>,
    pub fallen: BTreeMap<u16, (u32, usize)>,
    /// Images `ingest.json` lists.
    pub listed: usize,
}

/// One way the record and the slices disagree: its kind and what it is (record ids, line
/// numbers: for the owner's file, never the terminal).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disagreement {
    pub kind: &'static str,
    pub detail: String,
}

/// The class's name as ingest writes it (`terrain`, `multi`, `object`, …).
pub fn class_name(c: Class) -> String {
    serde_json::to_value(c)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Of `images`, the ones that draw.
fn drawing(game: &Game, images: impl Iterator<Item = ImageId>) -> Vec<ImageId> {
    images
        .filter(|&id| game.classic(id).is_some_and(|c| c.has_opaque()))
        .collect()
}

fn tile(id: ImageId) -> Option<u16> {
    match id {
        ImageId::Tile(r) => Some(r),
        _ => None,
    }
}

/// What the fixed point left, from the record `rec` (of `gates.jsonl`), the list `accepted` (of
/// `ingest.json`), the tile masters `files` found in `slices/` and the images `slices` read from
/// them; the pairs measured again with the seam threshold `seam`. Every disagreement, or the
/// state:
///
/// - `gates`: a pair line gated with another seam threshold (its `gates` text, as ingest writes
///   it, says `seam<=` another value; nothing else is checked then: every limit would differ);
/// - `duplicate`: a pair logged twice in one pass (a pass checks each pair once; which line
///   would be the record is not guessed);
/// - `plan`: a fallback naming an item the plan has not, or of another class; an image listed
///   that no item has;
/// - `listed`: a listed image whose id, slice path or content hash is not the game's;
/// - `slice`: a master not listed, or listed and missing, or of a record the game has no image of;
/// - `part`: an item listed in part;
/// - `fallen`: a listed image of an item the gate sent back;
/// - `sides`: a pass-1 line whose sides are not what the slices and the fallbacks make them
///   (generated in pass 1: listed now, or sent back);
/// - `pass`: a failing line whose generated sides were not sent back in its pass, a later pass's
///   failing line in no pass that sent anything back, a pass with no fallback before the last;
/// - `refused`: a slice the gate refuses (not ×4 of its record);
/// - `measure`: a pair of the last pass whose sides did not change since pass 1, measured from
///   the slices otherwise than pass 1 logged it;
/// - `failing`: a pair of the last pass failing (the fixed point does not end there);
/// - `last`: a line that passes in a pass after the last; the last pass's lines, when the log
///   has them, other than the last pass rebuilt (a pass between the first and the last may be
///   logged whole: its passing lines are not checked).
pub fn fixed(
    game: &Game,
    plan: &Plan,
    rec: &Record,
    accepted: &[Accepted],
    files: &BTreeSet<u16>,
    slices: &Slices,
    seam: f32,
) -> Result<Fixed, Vec<Disagreement>> {
    let mut out: Vec<Disagreement> = Vec::new();
    macro_rules! say {
        ($kind:expr, $detail:expr $(,)?) => {
            out.push(Disagreement {
                kind: $kind,
                detail: $detail,
            })
        };
    }
    let want = format!("seam<={seam}");
    for l in &rec.pairs {
        let theirs = l
            .gates
            .as_deref()
            .and_then(|g| g.split(' ').find(|t| t.starts_with("seam<=")));
        if let Some(t) = theirs.filter(|&t| t != want) {
            say!(
                "gates",
                format!(
                    "line {}: gated with {t}; calibrate measures with {want}",
                    l.line
                ),
            );
        }
    }
    if !out.is_empty() {
        return Err(out);
    }

    // A pass checks each pair once: a pair logged twice in one pass is no record of it.
    let mut once: BTreeMap<(usize, Pair), usize> = BTreeMap::new();
    for l in &rec.pairs {
        match once.get(&(l.pass, l.pair)) {
            Some(first) => say!(
                "duplicate",
                format!(
                    "line {}: the pair of line {first} again, in the same pass {}",
                    l.line, l.pass
                ),
            ),
            None => {
                once.insert((l.pass, l.pair), l.line);
            }
        }
    }

    // The items sent back, and their records that draw.
    let mut fallen: BTreeMap<u16, (u32, usize)> = BTreeMap::new();
    let mut fallen_items: BTreeSet<u32> = BTreeSet::new();
    for f in &rec.fell {
        let Some(item) = plan.items.get(f.item as usize) else {
            say!(
                "plan",
                format!("line {}: item {} is not in the plan", f.line, f.item)
            );
            continue;
        };
        if class_name(item.class) != f.class {
            say!(
                "plan",
                format!(
                    "line {}: item {} is not of class {}",
                    f.line, f.item, f.class
                ),
            );
        }
        fallen_items.insert(f.item);
        for r in drawing(game, item.images()).into_iter().filter_map(tile) {
            fallen.insert(r, (f.item, f.pass));
        }
    }

    // The list, against the game and the masters.
    let mut listed: BTreeSet<ImageId> = BTreeSet::new();
    for a in accepted {
        let Some(id) = atlas::parse_image(&a.image) else {
            say!("listed", format!("{:?}: not an image id", a.image));
            continue;
        };
        if a.slice != format!("slices/{}.png", atlas::slice_name(id)) {
            say!(
                "listed",
                format!("{}: {} is not its slice", a.image, a.slice)
            );
        }
        if game.hash(id).map(|h| format!("{h:016x}")).as_deref() != Some(a.hash.as_str()) {
            say!(
                "listed",
                format!("{}: the content hash is not the game's", a.image),
            );
        }
        listed.insert(id);
    }
    let listed_tiles: BTreeSet<u16> = listed.iter().filter_map(|&id| tile(id)).collect();
    for r in files.difference(&listed_tiles) {
        say!(
            "slice",
            format!("t:{r}: a master ingest.json does not list")
        );
    }
    for r in listed_tiles.difference(files) {
        say!("slice", format!("t:{r}: listed, and no master in slices/"));
    }
    for &r in files {
        if slices.get(r).is_none() {
            say!(
                "slice",
                format!("t:{r}: a master of a record the game has no image of")
            );
        }
    }
    let generated: BTreeSet<u16> = listed_tiles.intersection(files).copied().collect();

    // The survivors: the items whose images are listed, whole.
    let mut owner: BTreeMap<ImageId, usize> = BTreeMap::new();
    for (k, item) in plan.items.iter().enumerate() {
        for id in item.images() {
            owner.entry(id).or_insert(k);
        }
    }
    let mut of: BTreeMap<usize, BTreeSet<ImageId>> = BTreeMap::new();
    for &id in &listed {
        match owner.get(&id) {
            Some(&k) => {
                of.entry(k).or_default().insert(id);
            }
            None => say!(
                "plan",
                format!("{}: listed, and no item has it", atlas::image_name(id)),
            ),
        }
    }
    let mut survivors = Vec::new();
    for (k, have) in of {
        let item = &plan.items[k];
        let images = drawing(game, item.images());
        let want: BTreeSet<ImageId> = images.iter().copied().collect();
        if have != want {
            say!(
                "part",
                format!(
                    "item {k}: {} of its {} images that draw are listed",
                    have.intersection(&want).count(),
                    want.len()
                ),
            );
        }
        if fallen_items.contains(&(k as u32)) {
            say!(
                "fallen",
                format!("item {k}: listed, and sent back to MMPX by the neighbour gate"),
            );
        }
        survivors.push(Survivor {
            item: k as u32,
            class: item.class,
            records: images.iter().copied().filter_map(tile).collect(),
            images,
        });
    }

    // Pass 1's generated records: the ones listed now and the ones sent back since.
    let first_gen = |r: u16| generated.contains(&r) || fallen.contains_key(&r);
    let kind = |a: bool, b: bool| match (a, b) {
        (true, true) => Some(Sides::GenGen),
        (true, false) | (false, true) => Some(Sides::GenMmpx),
        (false, false) => None,
    };
    let fell_in = |r: u16, pass: usize| fallen.get(&r).is_some_and(|f| f.1 == pass);
    let last_fell = rec.fell.iter().map(|f| f.pass).max().unwrap_or(0);
    let passes = last_fell + 1;
    for p in 1..=last_fell {
        if !rec.fell.iter().any(|f| f.pass == p) {
            say!(
                "pass",
                format!("pass {p} sent nothing back, and a later pass did")
            );
        }
    }
    let first: Vec<&Logged> = rec.pairs.iter().filter(|l| l.pass == 1).collect();
    for l in &first {
        let (a, b) = (first_gen(l.pair.a), first_gen(l.pair.b));
        if kind(a, b) != Some(l.sides) {
            say!(
                "sides",
                format!(
                    "line {}: {} in pass 1, and the slices and the fallbacks make it {}",
                    l.line,
                    l.sides.name(),
                    kind(a, b).map_or("no generated side", Sides::name)
                ),
            );
        }
    }
    // Generated in pass `k`: generated in pass 1 and not sent back before it.
    let up_at = |r: u16, k: usize| first_gen(r) && fallen.get(&r).is_none_or(|f| f.1 >= k);
    let mut logged_last: BTreeMap<Pair, &Logged> = BTreeMap::new();
    for l in &rec.pairs {
        if l.fail {
            let up: Vec<u16> = [l.pair.a, l.pair.b]
                .into_iter()
                .filter(|&r| up_at(r, l.pass))
                .collect();
            if up.is_empty() || !up.iter().all(|&r| fell_in(r, l.pass)) {
                say!(
                    "pass",
                    format!(
                        "line {}: fails in pass {}, and its generated sides were not sent back then",
                        l.line, l.pass
                    )
                );
            }
        } else if l.pass > passes {
            say!(
                "last",
                format!(
                    "line {}: passes in pass {}, after the last pass {passes}",
                    l.line, l.pass
                )
            );
        } else if l.pass == passes && passes > 1 {
            // A pass between the first and the last, logged whole, is read and not held to
            // anything: only the last pass's lines are.
            logged_last.insert(l.pair, l);
        }
    }

    // The last pass: pass 1's pairs with a side still generated, measured from the slices.
    let mut last: Vec<Last> = Vec::new();
    for l in &first {
        let (a, b) = (generated.contains(&l.pair.a), generated.contains(&l.pair.b));
        let Some(sides) = kind(a, b) else { continue };
        let result = match measure(game, slices, l.pair, seam) {
            Ok(r) => r,
            Err(e) => {
                say!("refused", format!("line {}: {e}", l.line));
                continue;
            }
        };
        let same = a == first_gen(l.pair.a) && b == first_gen(l.pair.b);
        if same && result != l.result {
            say!(
                "measure",
                format!(
                    "line {}: its sides as in pass 1, measured {:?} from the slices, logged {:?}",
                    l.line, result, l.result
                ),
            );
        }
        if result.is_some_and(|r| !gate::seam_passes(r)) {
            say!(
                "failing",
                format!(
                    "line {}: fails in the last pass ({:?}), so the fixed point does not end there",
                    l.line, result
                ),
            );
        }
        last.push(Last {
            pair: l.pair,
            sides,
            result,
            labels: l.labels.clone(),
            at: l.at,
            first: (l.sides, l.result),
        });
    }
    last.sort_by_key(|x| x.pair);
    if !logged_last.is_empty() {
        for x in &last {
            match logged_last.get(&x.pair) {
                Some(l) if l.sides == x.sides && l.result == x.result => {}
                Some(l) => say!(
                    "last",
                    format!(
                        "line {}: logged {} {:?} in the last pass, rebuilt {} {:?}",
                        l.line,
                        l.sides.name(),
                        l.result,
                        x.sides.name(),
                        x.result
                    ),
                ),
                None => say!(
                    "last",
                    format!(
                        "t:{} t:{}: in the last pass rebuilt, not in the one logged",
                        x.pair.a, x.pair.b
                    ),
                ),
            }
        }
        for (p, l) in &logged_last {
            if !last.iter().any(|x| x.pair == *p) {
                say!(
                    "last",
                    format!(
                        "line {}: logged in the last pass, and no side is generated",
                        l.line
                    ),
                );
            }
        }
    }
    if !out.is_empty() {
        return Err(out);
    }
    Ok(Fixed {
        passes,
        logged_last: passes == 1 || !logged_last.is_empty(),
        first: first.len(),
        last,
        survivors,
        generated,
        fallen,
        listed: listed.len(),
    })
}

/// What `calibrate --final` is given.
#[derive(Clone, Debug)]
pub struct FinalOptions {
    pub out: PathBuf,
    /// The seam threshold the pairs are measured again with (ingest's).
    pub seam: f32,
    /// `all`, or the one label whose pairs place the coverage windows.
    pub source: Source,
    /// The owner's list of record ids, each a window of its own.
    pub include: Option<PathBuf>,
}

/// Where a window is placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Placement {
    /// At a pair of the last pass that one of its records meets.
    Pair,
    /// In no pair: where the world first draws one of its records.
    Drawn,
    /// Drawn nowhere in the starting world: the item by itself on an empty window.
    Alone,
}

impl Placement {
    pub fn name(self) -> &'static str {
        match self {
            Placement::Pair => "pair",
            Placement::Drawn => "drawn",
            Placement::Alone => "alone",
        }
    }
}

/// Why a window is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// One per surviving item.
    Coverage,
    /// One per id of `--include`.
    Include,
    /// One per pair of the last pass that no other window draws.
    Pair,
}

impl Origin {
    pub fn name(self) -> &'static str {
        match self {
            Origin::Coverage => "coverage",
            Origin::Include => "include",
            Origin::Pair => "pair",
        }
    }
}

/// The ids of an `--include` file: one record id a line (`t:N`, or `N`), `#` to the line's end
/// a comment, blank lines skipped; each id once, in the file's order.
pub fn include_ids(text: &str) -> Result<Vec<u16>, String> {
    let mut out: Vec<u16> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let id = line.split('#').next().unwrap_or("").trim();
        if id.is_empty() {
            continue;
        }
        let r = match atlas::parse_image(id) {
            Some(ImageId::Tile(r)) => Some(r),
            Some(_) => None,
            None => id.parse::<u16>().ok(),
        };
        let r = r
            .filter(|&r| usize::from(r) < TILE_COUNT)
            .ok_or_else(|| format!("--include line {}: not a tile record id (t:N)", i + 1))?;
        if !out.contains(&r) {
            out.push(r);
        }
    }
    Ok(out)
}

/// Where each record of `wanted` is first drawn in the starting world (level, row, column, the
/// least first): as a map tile or an object's tile or part, through every frame that replaces
/// it.
pub fn first_drawn(game: &Game, wanted: &BTreeSet<u16>) -> BTreeMap<u16, At> {
    let world = &game.world;
    let mut drawn = Drawn::of(game);
    let mut hits: HashMap<u16, Vec<u16>> = HashMap::new();
    let mut first: BTreeMap<u16, (usize, usize, usize)> = BTreeMap::new();
    let mut see = |t: u16, at: (usize, usize, usize), first: &mut BTreeMap<_, _>| {
        let recs = hits.entry(t).or_insert_with(|| {
            let v = drawn.records(t);
            v.into_iter().filter(|r| wanted.contains(r)).collect()
        });
        for &r in recs.iter() {
            let e = first.entry(r).or_insert(at);
            *e = (*e).min(at);
        }
    };
    for (z, level) in world.levels.iter().enumerate() {
        let s = level.tiles();
        for y in 0..s {
            for x in 0..s {
                if let Some(t) = world.tile(z, x, y) {
                    see(t, (z, y, x), &mut first);
                }
            }
        }
    }
    for o in &world.objects {
        let z = usize::from(o.z);
        let s = world.levels.get(z).map_or(0, |l| l.tiles()) as i64;
        if s == 0 {
            continue;
        }
        let wrap = |v: i64| v.rem_euclid(s) as usize;
        for (dx, dy, t) in neighbour::parts(game, o.tile) {
            let at = (
                z,
                wrap(i64::from(o.y) + i64::from(dy)),
                wrap(i64::from(o.x) + i64::from(dx)),
            );
            see(t, at, &mut first);
        }
    }
    first
        .into_iter()
        .map(|(r, (z, y, x))| (r, At { z, x, y }))
        .collect()
}

/// A window of the final read, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub origin: Origin,
    /// The survivor it shows (its index in [`Fixed::survivors`]).
    pub survivor: usize,
    /// The records it is for: the item's, or the one included.
    pub records: Vec<u16>,
    pub placement: Placement,
    /// A's cell and the edge (a placement at a pair: the pair's; else the cell found, east).
    pub at: Option<At>,
    pub edge: Edge,
    /// The pair it is placed at (its index in [`Fixed::last`]).
    pub pair: Option<usize>,
}

/// The pair of the last pass to place a window for `records` at: one that a record of them
/// meets, with a cell, of label `label` when given; the largest ratio over limit, then the
/// first in pair order.
fn at_pair(fixed: &Fixed, records: &[u16], label: Option<&str>) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, l) in fixed.last.iter().enumerate() {
        let meets = records.contains(&l.pair.a) || records.contains(&l.pair.b);
        if !meets || l.at.is_none() || label.is_some_and(|x| !l.from(x)) {
            continue;
        }
        if best.is_none_or(|b| l.excess() > fixed.last[b].excess()) {
            best = Some(i);
        }
    }
    best
}

/// Places a window for `records` of survivor `survivor` (module docs of [`crate::calibrate`]):
/// at a pair (of `label` first when given; with `strict`, only of it), else where the world
/// draws them, else alone. `None` when `strict` finds no pair of the label.
fn place(
    fixed: &Fixed,
    first: &BTreeMap<u16, At>,
    origin: Origin,
    survivor: usize,
    records: Vec<u16>,
    label: Option<&str>,
    strict: bool,
) -> Option<Window> {
    let pair = at_pair(fixed, &records, label)
        .or_else(|| (!strict).then(|| at_pair(fixed, &records, None)).flatten());
    let window = |placement, at, edge, pair| Window {
        origin,
        survivor,
        records: records.clone(),
        placement,
        at,
        edge,
        pair,
    };
    if let Some(i) = pair {
        let l = &fixed.last[i];
        return Some(window(Placement::Pair, l.at, l.pair.edge, Some(i)));
    }
    if strict {
        return None;
    }
    match records
        .iter()
        .filter_map(|r| first.get(r))
        .min_by_key(|a| (a.z, a.y, a.x))
    {
        Some(&at) => Some(window(Placement::Drawn, Some(at), Edge::East, None)),
        None => Some(window(Placement::Alone, None, Edge::East, None)),
    }
}

/// The cells of a window that shows `pieces` alone: each piece's records as its block of cells,
/// the pieces in a row from the second column and the second row with a cell between them,
/// the next row of pieces a cell below; how many pieces fit.
pub fn alone(pieces: &[Piece]) -> (Vec<Stack>, usize) {
    let mut cells = vec![Stack::default(); COLS * ROWS];
    let (mut c0, mut r0, mut tall, mut fit) = (1usize, 1usize, 0usize, 0usize);
    for p in pieces {
        let (w, h) = (p.w as usize, p.h as usize);
        if c0 + w > COLS - 1 {
            (c0, r0, tall) = (1, r0 + tall + 1, 0);
        }
        if c0 + w > COLS - 1 || r0 + h > ROWS {
            break;
        }
        for (i, &id) in p.images.iter().enumerate() {
            if let Some(t) = tile(id) {
                cells[(r0 + i / w) * COLS + c0 + i % w].list.push(t);
            }
        }
        fit += 1;
        c0 += w + 1;
        tall = tall.max(h);
    }
    (cells, fit)
}

/// The pairs of the last pass drawn inside a window (`f`, as [`framed`] drew it): every two
/// edge-adjacent cells of it, each record that shows in the one beside each that shows in the
/// other, drawn across their join ([`Framed::joins`]: each side left along its side of the
/// join), that `by` (pair → its index in the last pass) has. In cell order (row, column), east
/// before south, then the pair's index.
pub fn ticks(f: &Framed, by: &BTreeMap<Pair, usize>) -> Vec<((usize, usize), usize)> {
    let mut out = Vec::new();
    for r in 0..ROWS {
        for c in 0..COLS {
            let here = &f.drawn[r * COLS + c];
            for (edge, (bc, br)) in [(Edge::East, (c + 1, r)), (Edge::South, (c, r + 1))] {
                if bc >= COLS || br >= ROWS {
                    continue;
                }
                let mut found: Vec<usize> = Vec::new();
                for &a in here {
                    for &b in &f.drawn[br * COLS + bc] {
                        let p = Pair { a, b, edge };
                        if let Some(&i) = by.get(&p)
                            && f.joins((c, r), p)
                        {
                            found.push(i);
                        }
                    }
                }
                found.sort_unstable();
                found.dedup();
                out.extend(found.into_iter().map(|i| ((c, r), i)));
            }
        }
    }
    out
}

/// One row of a final sheet: a window's caption, its marks and legend, its panels.
pub struct FinalLine {
    pub caption: String,
    /// The joins marked: A's cell in the window and the edge.
    pub marks: Vec<((usize, usize), Edge)>,
    /// A line per mark: where, its label, sides and ratio / limit.
    pub legend: Vec<String>,
    pub panels: Panels,
}

/// The height of a final row at layout `l`: the sampled mode's row, and a line per legend line
/// under the panels.
fn row_height(l: &Layout, legend: usize) -> u32 {
    l.row_h() + legend as u32 * l.line_h()
}

/// The size of a final sheet of `lines` at scale `s`.
pub fn final_size(lines: &[FinalLine], s: u32) -> (u32, u32) {
    let l = Layout::of(s);
    let h: u32 = lines.iter().map(|x| row_height(&l, x.legend.len())).sum();
    (l.width(), l.margin / 2 + h)
}

/// A final sheet of `lines` at scale `s`: as [`sheet`] draws a row, every join of the window's
/// marks marked in the margins of each panel (never on the art), and the legend under the
/// panels.
pub fn final_sheet(lines: &[FinalLine], s: u32) -> Rgba {
    let l = Layout::of(s);
    let (w, h) = final_size(lines, s);
    let mut out = Rgba::new(w, h);
    img::fill(&mut out, 0, 0, w, h, BACK);
    let mut top = l.margin / 2;
    for line in lines {
        let p = &line.panels;
        let py = top + l.caption_h() + l.margin;
        for (k, panel) in [&p.classic, &p.plain, &p.hd].into_iter().enumerate() {
            let px = l.margin + k as u32 * (l.w + l.margin);
            let cy = py - l.margin - l.caption_h();
            text(&mut out, (px, cy), &line.caption, l.text, l.w);
            text(&mut out, (px, cy + l.line_h()), PANEL_NAMES[k], l.text, l.w);
            out.paste(panel, px, py);
            for &(cell, edge) in &line.marks {
                mark(&mut out, &l, (px, py), cell, edge);
            }
        }
        let ly = py + l.h + l.margin / 2;
        for (i, t) in line.legend.iter().enumerate() {
            let y = ly + i as u32 * l.line_h();
            text(&mut out, (l.margin, y), t, l.text, w - 2 * l.margin);
        }
        top += row_height(&l, line.legend.len());
    }
    out
}

/// The final read's `samples.tsv` header.
pub const FINAL_TSV_HEADER: &str = "index\torigin\titem\trecords\tclass\tplacement\tz\tx\ty\tedge\t\
                                    pairs\tkinds\tticks\tsheet\tsheet_s2\tpanel\tframes\tnote";

/// What the final read drew, in counts and file names.
#[derive(Clone, Debug, PartialEq)]
pub struct FinalSummary {
    pub source: Source,
    /// The last pass, whether `gates.jsonl` logs it, pass 1's pairs and the last pass's.
    pub passes: usize,
    pub logged_last: bool,
    pub first: usize,
    pub last: usize,
    /// Images listed, and tile masters.
    pub listed: usize,
    pub masters: usize,
    /// Per class: survivors (items, images that draw), and coverage windows by placement;
    /// with a label, the survivors that meet no pair of it (no window).
    pub classes: Vec<ClassRow>,
    /// Per label × sides: pairs of the last pass, of them drawn in a window, windows that draw
    /// one.
    pub kinds: Vec<KindRow>,
    /// `--include`: ids, windows, ids not generated.
    pub include: Option<(usize, usize, usize)>,
    /// Windows of a pair of the last pass no other window draws; pairs of the last pass (of the
    /// label, with one) drawn in no window.
    pub pair_windows: usize,
    pub unread: usize,
    pub tsv_rows: usize,
    pub sheets: Vec<String>,
}

/// A class's line of the summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassRow {
    pub class: Class,
    pub items: usize,
    pub images: usize,
    pub pair: usize,
    pub drawn: usize,
    pub alone: usize,
    pub unlabelled: usize,
}

/// A label × sides line of the summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindRow {
    pub label: String,
    pub sides: Sides,
    pub last: usize,
    pub read: usize,
    pub windows: usize,
}

impl FinalSummary {
    /// The report: counts and file names only.
    pub fn text(&self) -> String {
        let mut s = String::new();
        let how = if self.passes == 1 {
            "nothing fell: pass 1 is the last, logged whole"
        } else if self.logged_last {
            "logged, and rebuilt from the slices to the same"
        } else {
            "rebuilt from the slices (gates.jsonl logs pass 1 whole and a later pass's failing \
             pairs only)"
        };
        let _ = writeln!(
            s,
            "calibrate --final (counts only): the fixed point's last pass is pass {}, {how}; {} \
             pairs checked in pass 1, {} in the last pass; ingest.json lists {} images, slices/ \
             has {} tile masters; the record and the slices agree",
            self.passes, self.first, self.last, self.listed, self.masters
        );
        if let Source::Label(l) = &self.source {
            let _ = writeln!(
                s,
                "source {l}: a coverage window only for a survivor that meets a pair of it, \
                 placed there"
            );
        }
        let _ = writeln!(
            s,
            "survivors (generated in the final pack) and their windows, by class:"
        );
        let _ = writeln!(
            s,
            "{:<30} {:>6} {:>7} {:>7} {:>7} {:>7} {:>9}",
            "", "items", "images", "pair", "drawn", "alone", "no label"
        );
        for c in &self.classes {
            let _ = writeln!(
                s,
                "{:<30} {:>6} {:>7} {:>7} {:>7} {:>7} {:>9}",
                c.class.name(),
                c.items,
                c.images,
                c.pair,
                c.drawn,
                c.alone,
                c.unlabelled
            );
        }
        let _ = writeln!(
            s,
            "(pair: placed at a pair it meets; drawn: in no pair, where the world first draws \
             it; alone: drawn nowhere in the starting world, the item by itself; no label: no \
             window, it meets no pair of the source; portraits and bitmaps have no map window)"
        );
        let _ = writeln!(
            s,
            "pairs by label and sides (a pair of two labels counts in both): in the last pass, \
             drawn in a window read, windows that draw one"
        );
        for k in &self.kinds {
            let _ = writeln!(
                s,
                "{} {}: {} in the last pass, {} read, {} windows",
                k.label,
                k.sides.name(),
                k.last,
                k.read,
                k.windows
            );
        }
        let _ = writeln!(
            s,
            "pair windows (a pair of the last pass that no item's or included id's window \
             draws): {}; pairs of the last pass drawn in no window: {}",
            self.pair_windows, self.unread
        );
        if let Some((ids, windows, not)) = self.include {
            let _ = writeln!(
                s,
                "--include: {ids} ids, {windows} windows, {not} not generated in the final pack \
                 (0 windows)"
            );
        }
        let _ = writeln!(s, "samples.tsv: {} rows", self.tsv_rows);
        let _ = writeln!(s, "sheets: {}", self.sheets.join(" "));
        s
    }
}

/// A window drawn: its frame, the pairs it ticks, and how many of an alone item's pieces fit.
struct Drawn1 {
    framed: Framed,
    ticks: Vec<((usize, usize), usize)>,
    fit: Option<(usize, usize)>,
}

/// Draws window `w` (module docs of [`crate::calibrate`]).
fn draw_window(
    game: &Game,
    plan: &Plan,
    fixed: &Fixed,
    by: &BTreeMap<Pair, usize>,
    w: &Window,
) -> Drawn1 {
    let (framed, fit) = match (w.placement, w.at) {
        (Placement::Pair, Some(at)) => {
            let p = w.pair.map(|i| fixed.last[i].pair);
            (frame(game, at, w.edge, p), None)
        }
        (Placement::Drawn, Some(at)) => {
            let shown = w
                .records
                .iter()
                .copied()
                .filter(|r| first_at(game, at, *r))
                .take(1)
                .collect::<Vec<u16>>();
            let want: Vec<((usize, usize), u16)> = shown.iter().map(|&r| (A_CELL, r)).collect();
            (framed(game, &window_at(game, at), &want), None)
        }
        _ => {
            let item = &plan.items[fixed.survivors[w.survivor].item as usize];
            let pieces: Vec<Piece> = if w.origin == Origin::Include {
                w.records
                    .iter()
                    .map(|&r| Piece {
                        w: 1,
                        h: 1,
                        images: vec![ImageId::Tile(r)],
                    })
                    .collect()
            } else {
                item.pieces.clone()
            };
            let (cells, fit) = alone(&pieces);
            (framed(game, &cells, &[]), Some((fit, pieces.len())))
        }
    };
    let ticks = ticks(&framed, by);
    Drawn1 { framed, ticks, fit }
}

/// Whether the cell `at` draws record `r` (some tile there, through its frames).
fn first_at(game: &Game, at: At, r: u16) -> bool {
    let cells = stacks(game, at.z, (at.x, at.y), (1, 1));
    let mut d = Drawn::of(game);
    cells
        .first()
        .is_some_and(|c| c.tiles().any(|t| d.records(t).contains(&r)))
}

/// The stacks of the window whose A's cell is `at`.
fn window_at(game: &Game, at: At) -> Vec<Stack> {
    super::window(game, at)
}

/// `t:a+t:b…`.
fn names(records: &[u16]) -> String {
    let v: Vec<String> = records
        .iter()
        .map(|&r| atlas::image_name(ImageId::Tile(r)))
        .collect();
    v.join("+")
}

/// `ratio/limit` at `digits` decimals, `-` when nothing is measured.
fn measured(result: Option<(f32, f32)>, digits: usize) -> String {
    result.map_or_else(
        || "-".to_string(),
        |(r, l)| format!("{r:.digits$}/{l:.digits$}"),
    )
}

/// Runs `calibrate --final` (module docs of [`crate::calibrate`]) over the work dir.
pub fn calibrate_final(game: &Game, work: &Work, o: &FinalOptions) -> Result<FinalSummary, String> {
    let out = work::ensure_dir(&o.out)?;
    // The owner's list first: a bad line stops the run before anything is written.
    let ids = match &o.include {
        Some(p) => {
            work::check_target(p).map_err(|e| {
                format!("--include: {e} (the list names records: keep it outside the repository)")
            })?;
            let bytes = img::read_capped(p, 1 << 20)?;
            let text =
                String::from_utf8(bytes).map_err(|_| "--include is not UTF-8".to_string())?;
            Some(include_ids(&text)?)
        }
        None => None,
    };
    // Snapshots first; only they are read.
    let snapshot = |name: &str, cap: u64| -> Result<Vec<u8>, String> {
        let bytes = img::read_capped(&work.path(name), cap)
            .map_err(|e| format!("{e} (run ingest first)"))?;
        work::write_file(&out.join(name), &bytes)?;
        img::read_capped(&out.join(name), cap)
    };
    let text = String::from_utf8(snapshot("gates.jsonl", 1 << 30)?)
        .map_err(|_| "gates.jsonl is not UTF-8".to_string())?;
    let list: Vec<Accepted> = serde_json::from_slice(&snapshot("ingest.json", 64 << 20)?)
        .map_err(|e| format!("ingest.json: {e}"))?;
    let rec = record(&text)?;
    let dir = work.path("slices");
    let files: BTreeSet<u16> = masters(&dir)?.into_iter().map(|(r, _)| r).collect();
    let slices = Rc::new(Slices::read(game, &dir)?);
    let plan = group::plan(game);
    let fixed = match fixed(game, &plan, &rec, &list, &files, &slices, o.seam) {
        Ok(f) => f,
        Err(d) => {
            let mut detail = String::new();
            let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
            for x in &d {
                let _ = writeln!(detail, "{}: {}", x.kind, x.detail);
                *kinds.entry(x.kind).or_default() += 1;
            }
            let path = out.join("disagreements.txt");
            work::write_file(&path, detail.as_bytes())?;
            let k: Vec<String> = kinds.iter().map(|(k, n)| format!("{k} {n}")).collect();
            return Err(format!(
                "the record and the slices disagree, nothing drawn: {} (details in {})",
                k.join(", "),
                path.display()
            ));
        }
    };

    // The windows: one per survivor drawn in the map's records, then one per included id.
    let label = match &o.source {
        Source::All => None,
        Source::Label(l) => Some(l.as_str()),
    };
    let mut wanted: BTreeSet<u16> = fixed
        .survivors
        .iter()
        .flat_map(|s| s.records.iter().copied())
        .collect();
    wanted.extend(ids.iter().flatten().copied());
    let first = first_drawn(game, &wanted);
    let mut windows: Vec<Window> = Vec::new();
    let mut unlabelled: BTreeMap<Class, usize> = BTreeMap::new();
    for (k, sv) in fixed.survivors.iter().enumerate() {
        if sv.records.is_empty() {
            continue;
        }
        let w = place(
            &fixed,
            &first,
            Origin::Coverage,
            k,
            sv.records.clone(),
            label,
            label.is_some(),
        );
        match w {
            Some(w) => windows.push(w),
            None => *unlabelled.entry(sv.class).or_default() += 1,
        }
    }
    let mut not_generated: Vec<u16> = Vec::new();
    for &r in ids.iter().flatten() {
        match fixed.survivors.iter().position(|s| s.records.contains(&r)) {
            Some(k) => windows.extend(place(
                &fixed,
                &first,
                Origin::Include,
                k,
                vec![r],
                label,
                false,
            )),
            None => not_generated.push(r),
        }
    }

    // Every pair of the last pass read: one that no window above draws gets a window of its
    // own (with one label, a pair of it), in pair order, each counting what it draws.
    let by: BTreeMap<Pair, usize> = fixed
        .last
        .iter()
        .enumerate()
        .map(|(i, l)| (l.pair, i))
        .collect();
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    for w in &windows {
        let d = draw_window(game, &plan, &fixed, &by, w);
        seen.extend(d.ticks.iter().map(|t| t.1));
    }
    for (i, l) in fixed.last.iter().enumerate() {
        let Some(at) = l.at else { continue };
        if seen.contains(&i) || label.is_some_and(|x| !l.from(x)) {
            continue;
        }
        let records: Vec<u16> = [l.pair.a, l.pair.b]
            .into_iter()
            .filter(|r| fixed.generated.contains(r))
            .collect();
        let Some(k) = records
            .first()
            .and_then(|r| fixed.survivors.iter().position(|s| s.records.contains(r)))
        else {
            continue;
        };
        let w = Window {
            origin: Origin::Pair,
            survivor: k,
            records,
            placement: Placement::Pair,
            at: Some(at),
            edge: l.pair.edge,
            pair: Some(i),
        };
        let d = draw_window(game, &plan, &fixed, &by, &w);
        seen.extend(d.ticks.iter().map(|t| t.1));
        windows.push(w);
    }

    // The sheets: coverage by class, the alone ones apart, the pairs, then the included ids.
    let stem_of = |w: &Window| -> String {
        let class = class_name(fixed.survivors[w.survivor].class);
        let p = o.source.prefix();
        match (w.origin, w.placement) {
            (Origin::Include, _) => format!("{p}final-include"),
            (Origin::Pair, _) => format!("{p}final-pairs"),
            (_, Placement::Alone) => format!("{p}final-{class}-alone"),
            _ => format!("{p}final-{class}"),
        }
    };
    let mut groups: BTreeMap<(u8, String), Vec<&Window>> = BTreeMap::new();
    for w in &windows {
        let order = match (w.origin, w.placement) {
            (Origin::Coverage, Placement::Alone) => 1,
            (Origin::Coverage, _) => 0,
            (Origin::Pair, _) => 2,
            (Origin::Include, _) => 3,
        };
        groups.entry((order, stem_of(w))).or_default().push(w);
    }
    let mut tsv = format!("{FINAL_TSV_HEADER}\n");
    let mut sheets: Vec<String> = Vec::new();
    let mut index = 0usize;
    let mut read: BTreeMap<(String, Sides), (BTreeSet<usize>, usize)> = BTreeMap::new();
    let l4 = Layout::of(4);
    for ((_, stem), list) in groups {
        // Each window drawn once, then cut into parts under the tallest sheet.
        let drawn: Vec<(&Window, Drawn1)> = list
            .into_iter()
            .map(|w| (w, draw_window(game, &plan, &fixed, &by, w)))
            .collect();
        let mut parts: Vec<Vec<usize>> = vec![Vec::new()];
        let mut height = l4.margin / 2;
        for (i, (_, d)) in drawn.iter().enumerate() {
            let h = row_height(&l4, d.ticks.len());
            let part = parts.last_mut().expect("a part");
            if !part.is_empty() && height + h > MAX_HEIGHT {
                parts.push(vec![i]);
                height = l4.margin / 2 + h;
            } else {
                part.push(i);
                height += h;
            }
        }
        let many = parts.len() > 1;
        for (k, part) in parts.iter().enumerate() {
            let name = if many {
                format!("{stem}-p{}", k + 1)
            } else {
                stem.clone()
            };
            let (s4, s2) = (format!("{name}.png"), format!("{name}-s2.png"));
            let (mut lines4, mut lines2) = (Vec::new(), Vec::new());
            for (row, &i) in part.iter().enumerate() {
                let (w, d) = &drawn[i];
                index += 1;
                let sv = &fixed.survivors[w.survivor];
                let class = class_name(sv.class);
                let caption = format!(
                    "{index}: {class} {} {}",
                    w.placement.name(),
                    w.origin.name()
                );
                let mut legend = Vec::new();
                let mut kinds: BTreeMap<(String, Sides), usize> = BTreeMap::new();
                let mut cells = Vec::new();
                for &(cell, li) in &d.ticks {
                    let l = &fixed.last[li];
                    let lead = if w.pair == Some(li) && cell == A_CELL {
                        ">"
                    } else {
                        " "
                    };
                    let e = if l.pair.edge == Edge::East { "e" } else { "s" };
                    legend.push(format!(
                        "{lead}{},{} {e} {} {} {}",
                        cell.0,
                        cell.1,
                        l.labels,
                        l.sides.name(),
                        measured(l.result, 2)
                    ));
                    for lab in l.labels.split('+') {
                        *kinds.entry((lab.to_string(), l.sides)).or_default() += 1;
                        let e = read.entry((lab.to_string(), l.sides)).or_default();
                        e.0.insert(li);
                    }
                    cells.push(format!(
                        "{},{},{},{},{},{},{},{}",
                        cell.0,
                        cell.1,
                        edge_name(l.pair.edge),
                        l.labels,
                        l.sides.name(),
                        atlas::image_name(ImageId::Tile(l.pair.a)),
                        atlas::image_name(ImageId::Tile(l.pair.b)),
                        measured(l.result, 4).replace('/', ",")
                    ));
                }
                for key in kinds.keys() {
                    read.entry(key.clone()).or_default().1 += 1;
                }
                for (s, lines) in [(Scale::X4, &mut lines4), (Scale::X2, &mut lines2)] {
                    lines.push(FinalLine {
                        caption: caption.clone(),
                        marks: d
                            .ticks
                            .iter()
                            .map(|&(cell, li)| (cell, fixed.last[li].pair.edge))
                            .collect(),
                        legend: legend.clone(),
                        panels: panels(game, &d.framed.fb, &slices, s),
                    });
                }
                let note = match (w.placement, d.fit) {
                    (Placement::Pair, _) if w.origin == Origin::Pair => {
                        "a pair of the last pass no other window draws".to_string()
                    }
                    (Placement::Pair, _) => "-".to_string(),
                    (Placement::Drawn, _) => {
                        "in no pair: where the world first draws it".to_string()
                    }
                    (Placement::Alone, Some((fit, n))) if fit < n => format!(
                        "drawn nowhere in the starting world: the item alone ({fit} of {n} pieces fit)"
                    ),
                    (Placement::Alone, _) => {
                        "drawn nowhere in the starting world: the item alone".to_string()
                    }
                };
                let kinds_text: Vec<String> = kinds
                    .iter()
                    .map(|((lab, sd), n)| format!("{lab}/{}={n}", sd.name()))
                    .collect();
                let dash = |v: Vec<String>, sep: &str| {
                    if v.is_empty() {
                        "-".to_string()
                    } else {
                        v.join(sep)
                    }
                };
                let at =
                    |f: fn(&At) -> usize| w.at.as_ref().map_or("-".into(), |a| f(a).to_string());
                let _ = writeln!(
                    tsv,
                    "{index}\t{}\t{}\t{}\t{class}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{s4}\t{s2}\t{}\t{}\t{note}",
                    w.origin.name(),
                    sv.item,
                    names(&w.records),
                    w.placement.name(),
                    at(|a| a.z),
                    at(|a| a.x),
                    at(|a| a.y),
                    if w.at.is_some() {
                        edge_name(w.edge)
                    } else {
                        "-"
                    },
                    d.ticks.len(),
                    dash(kinds_text, ","),
                    dash(cells, ";"),
                    row + 1,
                    d.framed.frames(),
                );
            }
            img::write_png(&out.join(&s4), &final_sheet(&lines4, 4))?;
            img::write_png(&out.join(&s2), &final_sheet(&lines2, 2))?;
            sheets.push(s4);
            sheets.push(s2);
        }
    }
    let rows = index + not_generated.len();
    for &r in &not_generated {
        index += 1;
        let _ = writeln!(
            tsv,
            "{index}\tinclude\t-\t{}\t-\t-\t-\t-\t-\t-\t0\t-\t-\t-\t-\t-\t-\tnot generated in the final pack (0)",
            atlas::image_name(ImageId::Tile(r))
        );
    }
    work::write_file(&out.join("samples.tsv"), tsv.as_bytes())?;

    // The summary.
    let mut classes = Vec::new();
    for class in Class::ALL {
        let of: Vec<(usize, &Survivor)> = fixed
            .survivors
            .iter()
            .enumerate()
            .filter(|(_, s)| s.class == class)
            .collect();
        let count = |p: Placement| {
            windows
                .iter()
                .filter(|w| w.origin == Origin::Coverage && w.placement == p)
                .filter(|w| fixed.survivors[w.survivor].class == class)
                .count()
        };
        classes.push(ClassRow {
            class,
            items: of.len(),
            images: of.iter().map(|(_, s)| s.images.len()).sum(),
            pair: count(Placement::Pair),
            drawn: count(Placement::Drawn),
            alone: count(Placement::Alone),
            unlabelled: unlabelled.get(&class).copied().unwrap_or(0),
        });
    }
    let drawn_any: BTreeSet<usize> = read
        .values()
        .flat_map(|(set, _)| set.iter().copied())
        .collect();
    let mut labels: Vec<String> = LABELS.iter().map(|l| l.to_string()).collect();
    for l in &fixed.last {
        for lab in l.labels.split('+') {
            if !labels.iter().any(|x| x == lab) {
                labels.push(lab.to_string());
            }
        }
    }
    let mut kinds = Vec::new();
    for lab in &labels {
        for sides in Sides::ALL {
            let last = fixed
                .last
                .iter()
                .filter(|l| l.sides == sides && l.from(lab))
                .count();
            let (r, w) = read
                .get(&(lab.clone(), sides))
                .map_or((0, 0), |(set, n)| (set.len(), *n));
            kinds.push(KindRow {
                label: lab.clone(),
                sides,
                last,
                read: r,
                windows: w,
            });
        }
    }
    Ok(FinalSummary {
        source: o.source.clone(),
        passes: fixed.passes,
        logged_last: fixed.logged_last,
        first: fixed.first,
        last: fixed.last.len(),
        listed: fixed.listed,
        masters: files.len(),
        classes,
        kinds,
        include: ids.as_ref().map(|v| {
            let windows = windows
                .iter()
                .filter(|w| w.origin == Origin::Include)
                .count();
            (v.len(), windows, not_generated.len())
        }),
        pair_windows: windows.iter().filter(|w| w.origin == Origin::Pair).count(),
        unread: fixed
            .last
            .iter()
            .enumerate()
            .filter(|(i, l)| !drawn_any.contains(i) && label.is_none_or(|x| l.from(x)))
            .count(),
        tsv_rows: rows,
        sheets,
    })
}

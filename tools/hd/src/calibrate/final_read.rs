//! `calibrate --final`: the fixed point's state read back from `gates.jsonl`, `ingest.json` and
//! `slices/`, checked against itself before anything is drawn (the module docs of
//! [`crate::calibrate`] say what is read, how the last pass is rebuilt and what is drawn).

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use u6_hdpack::ImageId;

use super::{RawAt, RawPair, Sides, Slices, labels, measure, parse_edge};
use crate::atlas;
use crate::game::Game;
use crate::gate;
use crate::group::{Class, Plan};
use crate::ingest::Accepted;
use crate::neighbour::{At, Pair};

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
/// sheets) are not read.
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
        if !v.get("neighbour").is_some_and(serde_json::Value::is_object) {
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
/// - `last`: lines of a later pass that do not fail, other than the last pass rebuilt.
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
        } else if l.pass > 1 {
            if l.pass != passes {
                say!(
                    "last",
                    format!(
                        "line {}: passes in pass {}, and the last pass is {passes}",
                        l.line, l.pass
                    )
                );
            }
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

//! `calibrate --final` on synthetic records and worlds, and on `gates.jsonl` lines,
//! `ingest.json` lists and masters made by the tests from the synthetic game's records.

use std::collections::BTreeMap;

use super::*;
use crate::game::{Level, World, synthetic};
use crate::gate::{Edge, Thresholds};
use crate::group;
use crate::img::{self, Rgba};

/// Opaque terrain records of the synthetic game, each an item of its own: X, A and B in a row,
/// east of each other; grass around them.
const X: u16 = 3;
const A: u16 = 4;
const B: u16 = 5;
const GRASS: u16 = 0;

pub(super) fn seam() -> f32 {
    Thresholds::default().seam
}

pub(super) fn gates() -> String {
    Thresholds::default().text()
}

/// A chunk of grass but for the cells given (x, y, tile).
fn chunk(cells: &[(usize, usize, u16)]) -> [[u8; 8]; 8] {
    let mut c = [[GRASS as u8; 8]; 8];
    for &(x, y, t) in cells {
        c[y][x] = t as u8;
    }
    c
}

fn hash(i: u32, seed: u32) -> u32 {
    let mut h = i.wrapping_mul(0x9e37_79b9) ^ seed.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 12)
}

/// The first of the five palette entries the tests' grain is drawn in: a made-up green, ±24
/// levels in steps of 12.
const GRAIN: u8 = 200;

/// One level of 2 × 2 chunks, grass, with X, A and B in a row at (2, 3), (3, 3), (4, 3); the four
/// records are made-up green grains (each a seed of its own), which the gate takes as meeting
/// cleanly when each is drawn as it is (×4, nearest, or MMPX).
pub(super) fn game() -> Game {
    let mut g = synthetic::game();
    g.palette.resize(256, [0, 0, 0]);
    for k in 0..5u8 {
        let off = i32::from(k) * 12 - 24;
        g.palette[usize::from(GRAIN + k)] = [60, 120, 50].map(|c: i32| (c + off) as u8);
    }
    for (seed, r) in [GRASS, X, A, B].into_iter().enumerate() {
        let rec = &mut g.records[usize::from(r)];
        for i in 0..256 {
            rec.pixels[i] = GRAIN + (hash(i as u32, seed as u32 + 1) % 5) as u8;
            rec.transparent[i] = false;
        }
    }
    g.world = World {
        chunks: vec![chunk(&[(2, 3, X), (3, 3, A), (4, 3, B)]), chunk(&[])],
        levels: vec![Level {
            side: 2,
            refs: vec![0, 1, 1, 1],
        }],
        objects: Vec::new(),
    };
    g
}

/// Record `r` at ×4, nearest.
pub(super) fn near(g: &Game, r: u16) -> Rgba {
    g.classic(ImageId::Tile(r)).unwrap().scaled(4, [0, 0, 0])
}

/// `img` with its west column of classic pixels painted red.
fn red_west(img: &Rgba) -> Rgba {
    let mut out = img.clone();
    for y in 0..img.height {
        for x in 0..4 {
            img::put(&mut out, x, y, [200, 30, 30, 255]);
        }
    }
    out
}

/// The item of record `r` in the plan.
pub(super) fn item_of(plan: &Plan, r: u16) -> u32 {
    plan.items
        .iter()
        .position(|i| i.images().any(|id| id == ImageId::Tile(r)))
        .unwrap() as u32
}

/// A pair line as ingest writes it.
#[allow(clippy::too_many_arguments)]
pub(super) fn pair_line(
    pass: usize,
    p: Pair,
    fail: bool,
    result: Option<(f32, f32)>,
    sides: Sides,
    source: &str,
    at: Option<At>,
    gates: &str,
) -> String {
    let name = |r: u16| atlas::image_name(ImageId::Tile(r));
    serde_json::json!({
        "neighbour": {"a": name(p.a), "b": name(p.b), "edge": p.edge},
        "pass": pass, "fail": fail,
        "ratio": result.map(|r| r.0), "limit": result.map(|r| r.1),
        "sides": sides.name(), "source": source,
        "at": at.map(|a| serde_json::json!({"z": a.z, "x": a.x, "y": a.y})),
        "gates": gates,
    })
    .to_string()
}

/// A neighbour fallback line as ingest writes it.
pub(super) fn fell_line(plan: &Plan, item: u32, pass: usize) -> String {
    serde_json::json!({
        "item": item, "class": plan.items[item as usize].class, "fallback": "neighbour",
        "pass": pass, "gates": gates(),
    })
    .to_string()
}

/// The pass-1 line of `p` as the gate logs it with the images `first` (pass 1's), in `source`
/// at A's cell `at`.
pub(super) fn logged(
    g: &Game,
    first: &BTreeMap<u16, Rgba>,
    p: Pair,
    source: &str,
    at: Option<At>,
) -> String {
    let s = Slices::new(g, first.clone());
    let result = measure(g, &s, p, seam()).unwrap();
    let both = first.contains_key(&p.a) && first.contains_key(&p.b);
    let sides = if both { Sides::GenGen } else { Sides::GenMmpx };
    let fail = result.is_some_and(|r| !gate::seam_passes(r));
    pair_line(1, p, fail, result, sides, source, at, &gates())
}

/// The list ingest writes for the records `rs` (×8 masters of 128 × 128).
pub(super) fn listed(g: &Game, rs: impl IntoIterator<Item = u16>) -> Vec<Accepted> {
    rs.into_iter()
        .map(|r| {
            let id = ImageId::Tile(r);
            Accepted {
                image: atlas::image_name(id),
                hash: format!("{:016x}", g.hash(id).unwrap()),
                width: 128,
                height: 128,
                slice: format!("slices/{}.png", atlas::slice_name(id)),
            }
        })
        .collect()
}

pub(super) const AB: Pair = Pair {
    a: A,
    b: B,
    edge: Edge::East,
};
const XA: Pair = Pair {
    a: X,
    b: A,
    edge: Edge::East,
};
const BG: Pair = Pair {
    a: B,
    b: GRASS,
    edge: Edge::East,
};

/// X–A–B–grass, east, A and B generated in pass 1: A red on its west side, so it meets X (drawn
/// by MMPX) with a seam and falls in pass 1; A beside B meets cleanly while both are generated;
/// B meets the grass cleanly. The lines of pass 1 and A's fallback, and the images left: B's.
pub(super) fn chain(g: &Game) -> (String, BTreeMap<u16, Rgba>) {
    let plan = group::plan(g);
    let first = BTreeMap::from([(A, red_west(&near(g, A))), (B, near(g, B))]);
    let at = |x: usize| Some(At { z: 0, x, y: 3 });
    let lines = [
        logged(g, &first, XA, "map", at(2)),
        logged(g, &first, AB, "map", at(3)),
        logged(g, &first, BG, "map", at(4)),
        fell_line(&plan, item_of(&plan, A), 1),
    ];
    let left = BTreeMap::from([(B, near(g, B))]);
    (lines.join("\n") + "\n", left)
}

/// The state of `text` with the images `left` (each listed, each a master), or its kinds of
/// disagreement.
fn state(g: &Game, text: &str, left: &BTreeMap<u16, Rgba>) -> Result<Fixed, Vec<&'static str>> {
    let files: BTreeSet<u16> = left.keys().copied().collect();
    let list = listed(g, files.iter().copied());
    state_of(g, text, left, &list, &files)
}

fn state_of(
    g: &Game,
    text: &str,
    left: &BTreeMap<u16, Rgba>,
    list: &[Accepted],
    files: &BTreeSet<u16>,
) -> Result<Fixed, Vec<&'static str>> {
    let plan = group::plan(g);
    let rec = record(text).unwrap();
    let slices = Slices::new(g, left.clone());
    fixed(g, &plan, &rec, list, files, &slices, seam()).map_err(|d| {
        let mut k: Vec<&'static str> = d.iter().map(|d| d.kind).collect();
        k.dedup();
        k
    })
}

#[test]
fn the_last_pass_is_rebuilt_from_the_slices() {
    let g = game();
    let (text, left) = chain(&g);
    // Pass 1 as designed: A fails beside X, A and B pass together, B passes beside the grass.
    let rec = record(&text).unwrap();
    let fails: Vec<(Pair, bool, Sides)> = rec
        .pairs
        .iter()
        .map(|l| (l.pair, l.fail, l.sides))
        .collect();
    assert_eq!(
        fails,
        [
            (XA, true, Sides::GenMmpx),
            (AB, false, Sides::GenGen),
            (BG, false, Sides::GenMmpx)
        ]
    );
    let f = state(&g, &text, &left).unwrap();
    // A fell in pass 1; pass 2 failed nothing and is not logged: it is rebuilt.
    assert_eq!((f.passes, f.logged_last, f.first), (2, false, 3));
    assert_eq!(f.generated, BTreeSet::from([B]));
    let plan = group::plan(&g);
    assert_eq!(f.fallen, BTreeMap::from([(A, (item_of(&plan, A), 1))]));
    // The pairs left: those with B. A beside B is now generated beside MMPX, measured from the
    // slices; B beside the grass as pass 1 logged it.
    let pairs: Vec<Pair> = f.last.iter().map(|l| l.pair).collect();
    assert_eq!(pairs, [AB, BG]);
    let slices = Slices::new(&g, left.clone());
    let ab = &f.last[0];
    assert_eq!((ab.sides, ab.first.0), (Sides::GenMmpx, Sides::GenGen));
    assert_eq!(ab.result, measure(&g, &slices, AB, seam()).unwrap());
    assert!(ab.result.is_some() && ab.result != ab.first.1);
    let bg = &f.last[1];
    assert_eq!((bg.sides, bg.first.0), (Sides::GenMmpx, Sides::GenMmpx));
    assert_eq!(bg.result, bg.first.1);
    assert_eq!(
        (ab.labels.as_str(), ab.at),
        ("map", Some(At { z: 0, x: 3, y: 3 }))
    );
    // The one survivor: B's item.
    assert_eq!(
        f.survivors,
        [Survivor {
            item: item_of(&plan, B),
            class: Class::Terrain,
            images: vec![ImageId::Tile(B)],
            records: vec![B],
        }]
    );
    assert_eq!(f.listed, 1);

    // A log that has the last pass (lines of pass 2 that pass) is held to the rebuilt one.
    let last = |p: &Last| pair_line(2, p.pair, false, p.result, p.sides, "map", p.at, &gates());
    let with = text.clone() + &last(ab) + "\n" + &last(bg) + "\n";
    let f2 = state(&g, &with, &left).unwrap();
    assert!(f2.logged_last);
    assert_eq!(f2.last, f.last);
    // Logged otherwise: the gen-gen of pass 1 kept, or a pair missing.
    let mut stale = ab.clone();
    stale.sides = Sides::GenGen;
    let wrong = text.clone() + &last(&stale) + "\n" + &last(bg) + "\n";
    assert_eq!(state(&g, &wrong, &left).unwrap_err(), ["last"]);
    let short = text.clone() + &last(bg) + "\n";
    assert_eq!(state(&g, &short, &left).unwrap_err(), ["last"]);
    // No fallback: pass 1 is the last pass, logged whole.
    let only = logged(&g, &left, BG, "map", None) + "\n";
    let f3 = state(&g, &only, &left).unwrap();
    assert_eq!((f3.passes, f3.logged_last, f3.last.len()), (1, true, 1));
}

#[test]
fn the_record_and_the_slices_must_agree() {
    let g = game();
    let plan = group::plan(&g);
    let (text, left) = chain(&g);
    assert!(state(&g, &text, &left).is_ok());
    let lines: Vec<&str> = text.lines().collect();
    let edit = |k: usize, f: &dyn Fn(&mut serde_json::Value)| -> String {
        let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        let mut v: serde_json::Value = serde_json::from_str(lines[k]).unwrap();
        f(&mut v);
        out[k] = v.to_string();
        out.join("\n") + "\n"
    };
    // B beside the grass logged with another ratio than its slice gives.
    let other = edit(2, &|v| v["ratio"] = serde_json::json!(0.25));
    assert_eq!(state(&g, &other, &left).unwrap_err(), ["measure"]);
    // ... or as gen-gen, though the grass is not generated.
    let gg = edit(2, &|v| v["sides"] = serde_json::json!("gen-gen"));
    assert_eq!(state(&g, &gg, &left).unwrap_err(), ["sides"]);
    // A's master left in place (and listed), though A was sent back: and A then fails beside X
    // in the last pass.
    let mut kept = left.clone();
    kept.insert(A, red_west(&near(&g, A)));
    assert_eq!(state(&g, &text, &kept).unwrap_err(), ["fallen", "failing"]);
    // Listed, and no master; a master not listed.
    let files = BTreeSet::new();
    let none = BTreeMap::new();
    let err = state_of(&g, &text, &none, &listed(&g, [B]), &files);
    assert_eq!(err.unwrap_err(), ["slice", "sides"]);
    let err = state_of(&g, &text, &left, &[], &BTreeSet::from([B]));
    assert_eq!(err.unwrap_err(), ["slice", "sides"]);
    // A listed hash that is not the game's.
    let mut list = listed(&g, [B]);
    list[0].hash = "0".repeat(16);
    let err = state_of(&g, &text, &left, &list, &BTreeSet::from([B]));
    assert_eq!(err.unwrap_err(), ["listed"]);
    // A master the gate refuses: not ×4 of its record.
    let small = BTreeMap::from([(B, Rgba::new(32, 32))]);
    assert_eq!(state(&g, &text, &small).unwrap_err(), ["refused"]);
    // A fallback of an item of another class, or one the plan has not.
    let class = edit(3, &|v| v["class"] = serde_json::json!("object"));
    assert_eq!(state(&g, &class, &left).unwrap_err(), ["plan"]);
    let far = edit(3, &|v| v["item"] = serde_json::json!(plan.items.len()));
    assert!(state(&g, &far, &left).unwrap_err().starts_with(&["plan"]));
    // A failing line whose generated side was not sent back in its pass.
    let late = edit(3, &|v| v["pass"] = serde_json::json!(2));
    assert!(state(&g, &late, &left).unwrap_err().contains(&"pass"));
    // Gated with other thresholds: said alone.
    let t = Thresholds {
        seam: 2.0,
        ..Thresholds::default()
    };
    let other_gates = text.replace(&gates(), &t.text());
    assert_eq!(state(&g, &other_gates, &left).unwrap_err(), ["gates"]);
}

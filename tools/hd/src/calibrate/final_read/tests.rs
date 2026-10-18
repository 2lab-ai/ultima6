//! `calibrate --final` on synthetic records and worlds, and on `gates.jsonl` lines,
//! `ingest.json` lists and masters made by the tests from the synthetic game's records.

use std::collections::BTreeMap;
use std::path::Path;

use super::super::out_dir::{Hooked, hook};
use super::*;
use crate::game::{Level, World, synthetic};
use crate::gate::{Edge, Thresholds};
use crate::group;

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

/// The ×8 master of the ×4 image `img`: each pixel a 2 × 2 block whose box average is the
/// pixel, one channel of it up by `d` in the block's top-left and bottom-right subpixels and
/// down by `d` in the other two (`d` as much as the channel allows, to 8: 0 where it is 0 or
/// 255). It tells `pack`'s box average from nearest (the top-left subpixel, the pixel plus
/// `d`) wherever `d` is not 0, which [`the_test_masters_tell_the_box_average_from_nearest`]
/// checks of the tests' images; it does not tell it from every other rule (a row's mean is the
/// pixel too).
pub(super) fn master(img: &Rgba) -> Rgba {
    let mut m = Rgba::new(img.width * 2, img.height * 2);
    for y in 0..img.height {
        for x in 0..img.width {
            let p = img::get(img, x, y);
            for (k, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                let mut q = p;
                let c = (x + y) as usize % 3;
                let d = p[c].min(255 - p[c]).min(8);
                q[c] = if k == 0 || k == 3 { p[c] + d } else { p[c] - d };
                img::put(&mut m, 2 * x + dx, 2 * y + dy, q);
            }
        }
    }
    m
}

#[test]
fn the_test_masters_tell_the_box_average_from_nearest() {
    // Each ×4 image the tests make masters of: its master box-averaged is the image, its
    // master reduced to the top-left subpixel is not, wherever the channel moved has room.
    let g = game();
    let mut images = vec![red_west(&near(&g, A))];
    for r in [
        GRASS,
        X,
        A,
        B,
        synthetic::OBJECTS,
        synthetic::OBJECTS + 2,
        synthetic::TREE,
    ] {
        images.push(near(&g, r));
    }
    for (k, i) in images.iter().enumerate() {
        let m = master(i);
        assert_eq!(&img::shrink(&m, 2), i, "{k}");
        let mut nearest = Rgba::new(i.width, i.height);
        for y in 0..i.height {
            for x in 0..i.width {
                img::put(&mut nearest, x, y, img::get(&m, 2 * x, 2 * y));
            }
        }
        // Exactly where the master's channel had room (d not 0: neither 0 nor 255), and there
        // is such a pixel.
        let (mut room, mut differ) = (0, 0);
        for y in 0..i.height {
            for x in 0..i.width {
                let v = img::get(i, x, y)[((x + y) % 3) as usize];
                let has = v != 0 && v != 255;
                let moved = img::get(&nearest, x, y) != img::get(i, x, y);
                assert_eq!(moved, has, "{k} ({x}, {y})");
                room += usize::from(has);
                differ += usize::from(moved);
            }
        }
        assert!(room > 0 && differ == room, "{k}");
    }
}

/// The file of [`master`] of `img`, as ingest writes it.
pub(super) fn master_png(img: &Rgba) -> Vec<u8> {
    img::encode_png(&master(img)).unwrap()
}

/// The masters of the ×4 images `images` as read: each record's image with its file's digest.
pub(super) fn digests(images: &BTreeMap<u16, Rgba>) -> BTreeMap<ImageId, String> {
    images
        .iter()
        .map(|(&r, i)| (ImageId::Tile(r), work::digest(&[&master_png(i)])))
        .collect()
}

/// The list ingest writes for the ×4 images `images`: each ×8 master ([`master`]) of 128 × 128,
/// with its file's digest.
pub(super) fn listed(g: &Game, images: &BTreeMap<u16, Rgba>) -> Vec<Accepted> {
    images
        .iter()
        .map(|(&r, i)| {
            let id = ImageId::Tile(r);
            Accepted {
                image: atlas::image_name(id),
                hash: format!("{:016x}", g.hash(id).unwrap()),
                width: 128,
                height: 128,
                slice: format!("slices/{}.png", atlas::slice_name(id)),
                digest: Some(work::digest(&[&master_png(i)])),
            }
        })
        .collect()
}

/// The neighbour gate's lines of `gates.jsonl` for the world of `g` with the records of `first`
/// generated in pass 1 (their ×4 images; their items accepted), written by the gate's own path
/// as ingest runs it: the world's pairs ([`neighbour::pairs`]), each record's sides from its
/// ×8 master ([`neighbour::sides`]), the fixed point ([`neighbour::settle`]) and ingest's writer
/// ([`crate::ingest::neighbour_lines`]); and the images left (those whose items it kept).
pub(super) fn gate_log(g: &Game, first: &BTreeMap<u16, Rgba>) -> (String, BTreeMap<u16, Rgba>) {
    let plan = group::plan(g);
    let pairs = neighbour::pairs(g);
    let masters: BTreeMap<u16, Rgba> = first.iter().map(|(&r, i)| (r, master(i))).collect();
    let accepted: BTreeMap<u16, (u32, &Rgba)> = masters
        .iter()
        .map(|(&r, m)| (r, (item_of(&plan, r), m)))
        .collect();
    let sides = neighbour::sides(g, &pairs.all, &accepted).unwrap();
    let settled = neighbour::settle(&pairs.all, &sides, seam());
    let text = crate::ingest::neighbour_lines(&plan, &pairs, &settled, &Thresholds::default());
    let left = first
        .iter()
        .filter(|(r, _)| !settled.fallen.contains_key(&item_of(&plan, **r)))
        .map(|(&r, i)| (r, i.clone()))
        .collect();
    (text, left)
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
/// B over the grass, the grass over B, A over the grass and the grass over A: south.
const BG_S: Pair = Pair {
    a: B,
    b: GRASS,
    edge: Edge::South,
};
const GB_S: Pair = Pair {
    a: GRASS,
    b: B,
    edge: Edge::South,
};
const AG_S: Pair = Pair {
    a: A,
    b: GRASS,
    edge: Edge::South,
};
const GA_S: Pair = Pair {
    a: GRASS,
    b: A,
    edge: Edge::South,
};

/// X–A–B–grass, east, in the grass of [`game`], A and B generated in pass 1: A red on its west
/// side, so it meets X (drawn by MMPX) with a seam and falls in pass 1; A beside B meets cleanly
/// while both are generated; B meets the grass cleanly on every side. The gate's lines
/// ([`gate_log`]), and the images left: B's.
pub(super) fn chain(g: &Game) -> (String, BTreeMap<u16, Rgba>) {
    let first = BTreeMap::from([(A, red_west(&near(g, A))), (B, near(g, B))]);
    gate_log(g, &first)
}

/// The state of `text` with the images `left` (each listed, each a master), or its kinds of
/// disagreement.
fn state(g: &Game, text: &str, left: &BTreeMap<u16, Rgba>) -> Result<Fixed, Vec<&'static str>> {
    let list = listed(g, left);
    state_of(g, text, left, &list, &digests(left))
}

fn state_of(
    g: &Game,
    text: &str,
    left: &BTreeMap<u16, Rgba>,
    list: &[Accepted],
    files: &BTreeMap<ImageId, String>,
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
    // Pass 1 as the gate logs it: every pair of the world with A or B, in pair order. A fails
    // beside X, and nothing else fails: A and B together, A and B beside the grass.
    let rec = record(&text).unwrap();
    let fails: Vec<(Pair, bool, Sides)> = rec
        .pairs
        .iter()
        .map(|l| (l.pair, l.fail, l.sides))
        .collect();
    let mmpx = |p: Pair| (p, false, Sides::GenMmpx);
    assert_eq!(
        fails,
        [
            mmpx(GA_S),
            mmpx(GB_S),
            (XA, true, Sides::GenMmpx),
            mmpx(AG_S),
            (AB, false, Sides::GenGen),
            mmpx(BG),
            mmpx(BG_S),
        ]
    );
    let f = state(&g, &text, &left).unwrap();
    // A fell in pass 1; pass 2 failed nothing and is not logged: it is rebuilt.
    assert_eq!((f.passes, f.logged_last, f.first), (2, false, 7));
    assert_eq!(f.generated, BTreeSet::from([B]));
    let plan = group::plan(&g);
    assert_eq!(f.fallen, BTreeMap::from([(A, (item_of(&plan, A), 1))]));
    // The pairs left: those with B. A beside B is now generated beside MMPX, measured from the
    // slices; B beside the grass as pass 1 logged it.
    let pairs: Vec<Pair> = f.last.iter().map(|l| l.pair).collect();
    assert_eq!(pairs, [GB_S, AB, BG, BG_S]);
    let slices = Slices::new(&g, left.clone());
    let of = |p: Pair| f.last.iter().find(|l| l.pair == p).unwrap();
    let ab = of(AB);
    assert_eq!((ab.sides, ab.first.0), (Sides::GenMmpx, Sides::GenGen));
    assert_eq!(ab.result, measure(&g, &slices, AB, seam()).unwrap());
    assert!(ab.result.is_some() && ab.result != ab.first.1);
    for p in [GB_S, BG, BG_S] {
        let x = of(p);
        assert_eq!((x.sides, x.first.0), (Sides::GenMmpx, Sides::GenMmpx));
        assert!(x.result.is_some() && x.result == x.first.1, "{p:?}");
    }
    assert_eq!(
        (ab.labels.as_str(), ab.at, ab.line),
        ("map", Some(At { z: 0, x: 3, y: 3 }), 5)
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
    let pass2 = |x: &[&Last]| -> String { x.iter().map(|l| last(l) + "\n").collect() };
    let (gb_s, bg, bg_s) = (of(GB_S), of(BG), of(BG_S));
    let with = text.clone() + &pass2(&[gb_s, ab, bg, bg_s]);
    let f2 = state(&g, &with, &left).unwrap();
    assert!(f2.logged_last);
    assert_eq!(f2.last, f.last);
    // Logged otherwise: the gen-gen of pass 1 kept, or pairs missing.
    let mut stale = ab.clone();
    stale.sides = Sides::GenGen;
    let wrong = text.clone() + &pass2(&[gb_s, &stale, bg, bg_s]);
    assert_eq!(state(&g, &wrong, &left).unwrap_err(), ["last"]);
    // The sides the rebuilt pass's, the measure another (still passing).
    let mut other = ab.clone();
    let (r, lim) = ab.result.unwrap();
    other.result = Some((r * 0.5, lim));
    let measured = text.clone() + &pass2(&[gb_s, &other, bg, bg_s]);
    assert_eq!(state(&g, &measured, &left).unwrap_err(), ["last"]);
    let short = text.clone() + &pass2(&[bg]);
    assert_eq!(state(&g, &short, &left).unwrap_err(), ["last"]);
    // A pair logged twice in one pass: the stale line before the one that agrees, or a pass-1
    // line again. Neither line is taken for the record.
    let twice = text.clone() + &pass2(&[gb_s, &stale, ab, bg, bg_s]);
    assert_eq!(state(&g, &twice, &left).unwrap_err(), ["duplicate"]);
    let again = text.clone() + text.lines().nth(1).unwrap() + "\n";
    assert_eq!(state(&g, &again, &left).unwrap_err(), ["duplicate"]);
    // The same pair in two passes is no duplicate (the log of the last pass above).
    assert!(state(&g, &with, &left).is_ok());
    // A pair passing in a pass after the last.
    let after = pair_line(3, BG, false, bg.result, bg.sides, "map", bg.at, &gates());
    let late = with.clone() + &after + "\n";
    assert_eq!(state(&g, &late, &left).unwrap_err(), ["last"]);
    // A line logged in the last pass with no side generated there (A beside X).
    let x = XA;
    let none = pair_line(2, x, false, None, Sides::GenMmpx, "map", None, &gates());
    let extra = with.clone() + &none + "\n";
    assert_eq!(state(&g, &extra, &left).unwrap_err(), ["last"]);
    // No fallback: pass 1 is the last pass, logged whole.
    let (only, kept) = gate_log(&g, &left);
    assert_eq!(kept, left);
    let f3 = state(&g, &only, &left).unwrap();
    assert_eq!((f3.passes, f3.logged_last, f3.last.len()), (1, true, 4));
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
    let line_of = |p: Pair| {
        let r = record(&text).unwrap();
        r.pairs.iter().position(|l| l.pair == p).unwrap()
    };
    let (bg, fell) = (line_of(BG), lines.len() - 1);
    // B beside the grass logged with another ratio than its slice gives.
    let other = edit(bg, &|v| v["ratio"] = serde_json::json!(0.25));
    assert_eq!(state(&g, &other, &left).unwrap_err(), ["measure"]);
    // ... or as gen-gen, though the grass is not generated.
    let gg = edit(bg, &|v| v["sides"] = serde_json::json!("gen-gen"));
    assert_eq!(state(&g, &gg, &left).unwrap_err(), ["sides"]);
    // A's master left in place (and listed), though A was sent back: and A then fails beside X
    // in the last pass.
    let mut kept = left.clone();
    kept.insert(A, red_west(&near(&g, A)));
    assert_eq!(state(&g, &text, &kept).unwrap_err(), ["fallen", "failing"]);
    // Listed, and no master; a master not listed.
    let none = BTreeMap::new();
    let err = state_of(&g, &text, &none, &listed(&g, &left), &BTreeMap::new());
    assert_eq!(err.unwrap_err(), ["slice", "sides"]);
    let err = state_of(&g, &text, &left, &[], &digests(&left));
    assert_eq!(err.unwrap_err(), ["slice", "sides"]);
    // A listed hash that is not the game's.
    let mut list = listed(&g, &left);
    list[0].hash = "0".repeat(16);
    let err = state_of(&g, &text, &left, &list, &digests(&left));
    assert_eq!(err.unwrap_err(), ["listed"]);
    // A listed slice that is not the image's own file.
    let mut list = listed(&g, &left);
    list[0].slice = format!("slices/{}.png", atlas::slice_name(ImageId::Tile(A)));
    let err = state_of(&g, &text, &left, &list, &digests(&left));
    assert_eq!(err.unwrap_err(), ["listed"]);
    // A master the gate refuses: not ×4 of its record.
    let small = BTreeMap::from([(B, Rgba::new(32, 32))]);
    assert_eq!(state(&g, &text, &small).unwrap_err(), ["refused"]);
    // A fallback of an item of another class, or one the plan has not.
    let class = edit(fell, &|v| v["class"] = serde_json::json!("object"));
    assert_eq!(state(&g, &class, &left).unwrap_err(), ["plan"]);
    let far = edit(fell, &|v| v["item"] = serde_json::json!(plan.items.len()));
    assert!(state(&g, &far, &left).unwrap_err().starts_with(&["plan"]));
    // A failing line whose generated side was not sent back in its pass.
    let late = edit(fell, &|v| v["pass"] = serde_json::json!(2));
    assert!(state(&g, &late, &left).unwrap_err().contains(&"pass"));
    // A pair of the last pass with no cell: no window could be placed at it, and the world has
    // it at one.
    let nowhere = edit(bg, &|v| v["at"] = serde_json::Value::Null);
    assert_eq!(state(&g, &nowhere, &left).unwrap_err(), ["world", "cell"]);
    // Gated with other thresholds: said alone.
    let t = Thresholds {
        seam: 2.0,
        ..Thresholds::default()
    };
    let other_gates = text.replace(&gates(), &t.text());
    assert_eq!(state(&g, &other_gates, &left).unwrap_err(), ["gates"]);
    // A pair line with no gates field, or one that says no seam threshold: gated with what no
    // line says.
    let no_gates = edit(bg, &|v| {
        v.as_object_mut().unwrap().remove("gates");
    });
    assert_eq!(state(&g, &no_gates, &left).unwrap_err(), ["gates"]);
    let no_seam = edit(bg, &|v| v["gates"] = serde_json::json!("structure>=0.5"));
    assert_eq!(state(&g, &no_seam, &left).unwrap_err(), ["gates"]);
}

#[test]
fn the_fallbacks_and_the_failing_lines_hold_each_other() {
    let g = game();
    let plan = group::plan(&g);
    let (text, left) = chain(&g);
    let lines: Vec<&str> = text.lines().collect();
    let k_of = |p: Pair| {
        record(&text)
            .unwrap()
            .pairs
            .iter()
            .position(|l| l.pair == p)
    };
    let (xa, bg) = (k_of(XA).unwrap(), k_of(BG).unwrap());
    let edit = |k: usize, f: &dyn Fn(&mut serde_json::Value)| -> String {
        let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        let mut v: serde_json::Value = serde_json::from_str(lines[k]).unwrap();
        f(&mut v);
        out[k] = v.to_string();
        out.join("\n") + "\n"
    };
    // A line failing in pass 1 whose generated side was not sent back then (B beside the
    // grass): that rule alone.
    let failing = edit(bg, &|v| v["fail"] = serde_json::json!(true));
    assert_eq!(state(&g, &failing, &left).unwrap_err(), ["pass"]);
    // A fallback with no failing line of its item's in its pass (A beside X passing).
    let no_line = edit(xa, &|v| v["fail"] = serde_json::json!(false));
    assert_eq!(state(&g, &no_line, &left).unwrap_err(), ["pass"]);
    // An item sent back twice.
    let twice = text.clone() + lines.last().unwrap() + "\n";
    assert_eq!(state(&g, &twice, &left).unwrap_err(), ["duplicate"]);
    // B sent back too, in a later pass, by its failing line there (none left generated): its
    // sides there are those of that pass (gen-mmpx: the grass is not generated).
    let b = item_of(&plan, B);
    let later = |pass: usize, sides: Sides| -> String {
        let bg_line = pair_line(
            pass,
            BG,
            true,
            Some((2.0, 1.5)),
            sides,
            "map",
            Some(At { z: 0, x: 4, y: 3 }),
            &gates(),
        );
        let fell = serde_json::json!({
            "item": b, "class": "terrain", "fallback": "neighbour", "pass": pass,
            "gates": gates(),
        });
        format!("{text}{bg_line}\n{fell}\n")
    };
    let none = BTreeMap::new();
    let f = state(&g, &later(2, Sides::GenMmpx), &none).unwrap();
    assert_eq!((f.passes, f.last.len(), f.survivors.len()), (3, 0, 0));
    // ... logged as gen-gen in pass 2: that rule alone.
    let gg = later(2, Sides::GenGen);
    assert_eq!(state(&g, &gg, &none).unwrap_err(), ["sides"]);
    // ... in pass 3, with nothing sent back in pass 2: that rule alone.
    let gap = later(3, Sides::GenMmpx);
    assert_eq!(state(&g, &gap, &none).unwrap_err(), ["pass"]);
}

#[test]
fn the_pairs_of_pass_1_are_the_worlds() {
    let g = game();
    let (text, left) = chain(&g);
    let lines: Vec<&str> = text.lines().collect();
    let without = |k: usize| -> String {
        lines
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != k)
            .map(|(_, l)| format!("{l}\n"))
            .collect()
    };
    let edit = |k: usize, f: &dyn Fn(&mut serde_json::Value)| -> String {
        let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        let mut v: serde_json::Value = serde_json::from_str(lines[k]).unwrap();
        f(&mut v);
        out[k] = v.to_string();
        out.join("\n") + "\n"
    };
    let world = neighbour::pairs(&g);
    // A line of pass 1 dropped: B beside the grass, east, which the world has with B generated
    // (the reviewer's probe A), or the grass over A, whose A fell since.
    for p in [BG, GA_S] {
        assert!(world.all.contains(&p));
        let k = record(&text)
            .unwrap()
            .pairs
            .iter()
            .position(|l| l.pair == p);
        let e = state(&g, &without(k.unwrap()), &left).unwrap_err();
        assert_eq!(e, ["world"], "{p:?}");
    }
    // A line of a pair the world does not have (A west of X), or logged at another cell or from
    // another source than the world's.
    let ax = Pair {
        a: A,
        b: X,
        edge: Edge::East,
    };
    let at = Some(At { z: 0, x: 3, y: 3 });
    let line = pair_line(
        1,
        ax,
        false,
        Some((1.0, 1.5)),
        Sides::GenMmpx,
        "map",
        at,
        &gates(),
    );
    let extra = text.clone() + &line + "\n";
    assert_eq!(state(&g, &extra, &left).unwrap_err(), ["world"]);
    let k = record(&text)
        .unwrap()
        .pairs
        .iter()
        .position(|l| l.pair == BG)
        .unwrap();
    let moved = edit(k, &|v| v["at"]["x"] = serde_json::json!(5));
    assert_eq!(state(&g, &moved, &left).unwrap_err(), ["world"]);
    let objects = edit(k, &|v| v["source"] = serde_json::json!("objects"));
    assert_eq!(state(&g, &objects, &left).unwrap_err(), ["world"]);
    // A label beside the source (H7's label field) is not the source.
    let labelled = edit(k, &|v| v["label"] = serde_json::json!("odd"));
    assert!(state(&g, &labelled, &left).is_ok());
}

#[test]
fn the_final_read_holds_each_master_to_the_digest_ingest_json_lists() {
    // The final read's side of it: an ingest.json made here, its digests the masters'. That
    // ingest writes the digests is checked where ingest runs
    // (the_read_of_what_ingest_and_its_gate_wrote).
    // A survivor in no pair (the chest, drawn at (12, 12)) and one drawn nowhere (the sword):
    // no pair holds their masters to the log.
    let mut g = game();
    let chest = synthetic::OBJECTS;
    let sword = synthetic::OBJECTS + 2;
    g.world.objects = vec![crate::game::Placed {
        x: 12,
        y: 12,
        z: 0,
        tile: chest,
    }];
    let images: BTreeMap<u16, Rgba> = [chest, sword].iter().map(|&r| (r, near(&g, r))).collect();
    let (dir, work) = work_dir("master", "", &g, &images);
    assert!(calibrate_final(&g, &work, &options(&dir)).is_ok());
    // The sword's master replaced after ingest by a solid image (the reviewer's probe B): the
    // listed digest is not the file's.
    let mut solid = Rgba::new(64, 64);
    img::fill(&mut solid, 0, 0, 64, 64, [255, 0, 255, 255]);
    let path = work.slice(&atlas::slice_name(ImageId::Tile(sword)));
    std::fs::write(&path, master_png(&solid)).unwrap();
    let o = FinalOptions {
        out: dir.join("replaced"),
        ..options(&dir)
    };
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(e.contains("disagree, nothing drawn: master 1"), "{e}");
    assert_eq!(
        files_of(&o.out),
        [
            "disagreements.txt",
            "gates.jsonl",
            "ingest.json",
            "jobs.jsonl",
            "refused.txt"
        ]
    );
    let detail = std::fs::read_to_string(o.out.join("disagreements.txt")).unwrap();
    assert_eq!(
        detail,
        format!("master: t:{sword}: its master is not the file ingest wrote (another digest)\n")
    );
    // An ingest.json of an older ingest, with no digests: refused, before anything is drawn.
    std::fs::write(&path, master_png(&images[&sword])).unwrap();
    let mut list = listed(&g, &images);
    list.iter_mut().for_each(|a| a.digest = None);
    work.write("ingest.json", &serde_json::to_vec(&list).unwrap())
        .unwrap();
    let text = std::fs::read_to_string(work.path("ingest.json")).unwrap();
    assert!(!text.contains("digest"));
    let o = FinalOptions {
        out: dir.join("older"),
        ..options(&dir)
    };
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(
        e.contains("no digest of its masters") && e.contains("run ingest again"),
        "{e}"
    );
    assert_eq!(
        files_of(&o.out),
        ["gates.jsonl", "ingest.json", "jobs.jsonl", "refused.txt"]
    );
    // In the state itself, too: a listed master with no digest is not the one ingest wrote.
    let fixed_of = |list: &[Accepted]| {
        let plan = group::plan(&g);
        let slices = Slices::new(&g, images.clone());
        fixed(
            &g,
            &plan,
            &Record::default(),
            list,
            &digests(&images),
            &slices,
            seam(),
        )
    };
    let e = fixed_of(&list).unwrap_err();
    assert!(
        e.iter().all(|d| d.kind == "master") && e.len() == 2,
        "{e:?}"
    );
    assert!(fixed_of(&listed(&g, &images)).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

/// `text` with `label` in a `label` field of the lines of `pairs`.
fn labelled(text: &str, pairs: &[Pair], label: &str) -> String {
    let rec = record(text).unwrap();
    text.lines()
        .enumerate()
        .map(|(i, l)| {
            let of = rec.pairs.iter().find(|x| x.line == i + 1);
            if of.is_some_and(|x| pairs.contains(&x.pair)) {
                let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
                v["label"] = serde_json::json!(label);
                v.to_string() + "\n"
            } else {
                format!("{l}\n")
            }
        })
        .collect()
}

/// The files of the out dir `out`, sorted, but the [`MARKER`] (which each has).
fn files_of(out: &Path) -> Vec<String> {
    assert!(out.join(MARKER).is_file(), "{}", out.display());
    let mut files: Vec<String> = std::fs::read_dir(out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|f| f != MARKER)
        .collect();
    files.sort();
    files
}

#[test]
fn a_neighbour_field_that_is_not_a_pair_stops_the_read() {
    let g = game();
    let (text, left) = chain(&g);
    let lines: Vec<&str> = text.lines().collect();
    let with = |k: usize, neighbour: serde_json::Value| -> String {
        let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        let mut v: serde_json::Value = serde_json::from_str(lines[k]).unwrap();
        v["neighbour"] = neighbour;
        out[k] = v.to_string();
        out.join("\n") + "\n"
    };
    // A beside X (line 3) with a neighbour that is not a pair: refused by its line, in the final
    // read and in the first pass's rows alike, not read as a line of another kind.
    let name = |r: u16| atlas::image_name(ImageId::Tile(r));
    for bad in [
        serde_json::json!(null),
        serde_json::json!(format!("{} {}", name(B), name(GRASS))),
        serde_json::json!([name(B), name(GRASS), "east"]),
        serde_json::json!(7),
    ] {
        let t = with(2, bad.clone());
        let e = record(&t).unwrap_err();
        assert!(
            e.contains("line 3: a neighbour field that is not a pair"),
            "{bad}: {e}"
        );
        let e = crate::calibrate::rows(&t).unwrap_err();
        assert!(
            e.contains("line 3: a neighbour field that is not a pair"),
            "{bad}: {e}"
        );
    }
    // A fallback line of another kind than the gate's (ingest writes none) is refused too.
    for fallback in [serde_json::json!(null), serde_json::json!("motion")] {
        let mut v: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
        v["fallback"] = fallback.clone();
        let t = text.clone() + &v.to_string() + "\n";
        let e = record(&t).unwrap_err();
        assert!(
            e.contains("a fallback field that is not the neighbour gate's"),
            "{fallback}: {e}"
        );
    }
    // A fallback line with a neighbour field: not a pair (refused as one), or a pair (a line
    // that is both).
    let fell: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    let both: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    for (neighbour, said) in [
        (
            serde_json::json!(null),
            "a neighbour field that is not a pair",
        ),
        (
            both["neighbour"].clone(),
            "a line that is both a pair and a fallback",
        ),
    ] {
        let mut v = fell.clone();
        v["neighbour"] = neighbour;
        let t = text.clone() + &v.to_string() + "\n";
        let e = record(&t).unwrap_err();
        let n = t.lines().count();
        assert!(e.contains(&format!("line {n}: {said}")), "{e}");
    }
    // A line with no neighbour field is another kind of line, and not read.
    let other = text.clone() + r#"{"image":"t:1","pass":1,"fail":false}"# + "\n";
    assert_eq!(record(&other).unwrap(), record(&text).unwrap());
    // The run stops before it draws: no sheet, no samples.tsv.
    let t = with(2, serde_json::json!(null));
    let (dir, work) = work_dir("malformed", &t, &g, &left);
    let o = options(&dir);
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(e.contains("line 3"), "{e}");
    assert_eq!(
        files_of(&o.out),
        ["gates.jsonl", "ingest.json", "jobs.jsonl", "refused.txt"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pair_line_ingest_does_not_write_stops_the_read() {
    let g = game();
    let (text, _) = chain(&g);
    let lines: Vec<&str> = text.lines().collect();
    let edit = |f: &dyn Fn(&mut serde_json::Value)| -> String {
        let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        let mut v: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        f(&mut v);
        out[2] = v.to_string();
        out.join("\n") + "\n"
    };
    // A beside X (line 3) with a ratio and no limit, or a limit and no ratio: not read as
    // nothing measured.
    let x: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
    assert_eq!(x["neighbour"]["a"], atlas::image_name(ImageId::Tile(X)));
    for field in ["limit", "ratio"] {
        let t = edit(&|v| v[field] = serde_json::Value::Null);
        let e = record(&t).unwrap_err();
        assert!(
            e.contains("line 3: a ratio without its limit, or a limit without its ratio"),
            "{field}: {e}"
        );
    }
    // A pair line of pass 0, which ingest never writes: refused, not held to nothing.
    let t = edit(&|v| v["pass"] = serde_json::json!(0));
    let e = record(&t).unwrap_err();
    assert!(e.contains("line 3: a pair line of pass 0"), "{e}");
}

#[test]
fn the_atlas_is_laid_out_from_the_plan_of_these_files() {
    // The jobs of an atlas of another plan (other files, or another version of the tool): the
    // log's item numbers name other items.
    let g = game();
    let (text, left) = chain(&g);
    let (dir, work) = work_dir("plan", &text, &g, &left);
    assert!(calibrate_final(&g, &work, &options(&dir)).is_ok());
    let plan = group::plan(&g);
    let empty = Plan::default();
    work.write_jobs(&[job(&plan, &plan.fingerprint()), job(&empty, "another")])
        .unwrap();
    let o = FinalOptions {
        out: dir.join("other"),
        ..options(&dir)
    };
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(e.contains("disagree, nothing drawn: plan 1"), "{e}");
    let detail = std::fs::read_to_string(o.out.join("disagreements.txt")).unwrap();
    assert!(
        detail.starts_with("plan: jobs.jsonl line 2: laid out from another plan"),
        "{detail}"
    );
    // A jobs.jsonl that names no job, or not every item of the log: B's (a survivor), A's (sent
    // back).
    let (b, a) = (item_of(&plan, B), item_of(&plan, A));
    let mut without_b = job(&plan, &plan.fingerprint());
    without_b.places.retain(|p| p.item != b);
    for (k, (jobs, items)) in [(vec![], vec![a, b]), (vec![without_b], vec![b])]
        .into_iter()
        .enumerate()
    {
        work.write_jobs(&jobs).unwrap();
        let o = FinalOptions {
            out: dir.join(format!("lost-{k}")),
            ..options(&dir)
        };
        let e = calibrate_final(&g, &work, &o).unwrap_err();
        let n = items.len();
        assert!(
            e.contains(&format!("disagree, nothing drawn: plan {n}")),
            "{e}"
        );
        let detail = std::fs::read_to_string(o.out.join("disagreements.txt")).unwrap();
        for item in items {
            assert!(
                detail.contains(&format!("plan: item {item}: in the log")),
                "{detail}"
            );
        }
        assert_eq!(
            files_of(&o.out),
            [
                "disagreements.txt",
                "gates.jsonl",
                "ingest.json",
                "jobs.jsonl",
                "refused.txt"
            ]
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A temp dir with a work dir: `gates` as its `gates.jsonl`, the records of `images` (×4) listed
/// in `ingest.json` and each written to `slices/` as its ×8 master ([`master`]).
fn work_dir(name: &str, gates: &str, g: &Game, images: &BTreeMap<u16, Rgba>) -> (PathBuf, Work) {
    let dir = std::env::temp_dir().join(format!("u6-hdtool-final-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let work = Work::open(Some(&dir.join("work"))).unwrap();
    work.write("gates.jsonl", gates.as_bytes()).unwrap();
    let plan = group::plan(g);
    work.write_jobs(&[job(&plan, &plan.fingerprint())]).unwrap();
    let list = listed(g, images);
    work.write("ingest.json", &serde_json::to_vec(&list).unwrap())
        .unwrap();
    std::fs::create_dir_all(work.path("slices")).unwrap();
    for (&r, img4) in images {
        let stem = atlas::slice_name(ImageId::Tile(r));
        std::fs::write(work.slice(&stem), master_png(img4)).unwrap();
    }
    (dir, work)
}

/// A job of the atlas laid out from the plan of fingerprint `fingerprint`, a place for each
/// item of `plan` (what the final read takes of `jobs.jsonl`: the fingerprint and the items).
fn job(plan: &Plan, fingerprint: &str) -> atlas::Job {
    let places = plan
        .items
        .iter()
        .enumerate()
        .map(|(k, item)| {
            let id = item.images().next().unwrap();
            atlas::Place {
                item: k as u32,
                class: item.class,
                piece: 0,
                image: atlas::image_name(id),
                hash: String::new(),
                x: 0,
                y: 0,
                w: 16,
                h: 16,
                scale: 1,
            }
        })
        .collect();
    atlas::Job {
        sheet: "tile-0001".into(),
        generation: "test".into(),
        plan: fingerprint.into(),
        kind: atlas::SheetKind::Tile,
        attempt: 1,
        prompt: String::new(),
        key: [0, 0, 0],
        width: 1,
        height: 1,
        places,
    }
}

fn options(dir: &Path) -> FinalOptions {
    FinalOptions {
        out: dir.join("final"),
        seam: seam(),
        source: Source::All,
        include: None,
    }
}

/// `samples.tsv` of the out dir: the header's columns, and each row by column name.
fn rows_of(out: &Path) -> Vec<BTreeMap<String, String>> {
    let text = std::fs::read_to_string(out.join("samples.tsv")).unwrap();
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap().split('\t').collect();
    assert_eq!(header.join("\t"), FINAL_TSV_HEADER);
    lines
        .map(|l| {
            let cells: Vec<&str> = l.split('\t').collect();
            assert_eq!(cells.len(), header.len(), "{l}");
            header
                .iter()
                .zip(cells)
                .map(|(h, c)| (h.to_string(), c.to_string()))
                .collect()
        })
        .collect()
}

/// The `cell`'s pixels (column, row of the window) of panel `k` of the first row of a sheet
/// at S 4.
fn cell_of(sheet: &Rgba, k: usize, (c, r): (usize, usize)) -> Rgba {
    let l = Layout::of(4);
    let (px, py) = (
        l.margin + k as u32 * (l.w + l.margin),
        l.margin / 2 + l.caption_h() + l.margin,
    );
    let cell = 16 * 4;
    sheet.rect(px + c as u32 * cell, py + r as u32 * cell, cell, cell)
}

fn read_sheet(path: &Path) -> Rgba {
    let (w, h) = img::png_size(&std::fs::read(path).unwrap()).unwrap();
    img::read_png(path, w, h).unwrap()
}

/// Panel `k` (0 classic, 1 no pack, 2 pack) of the first window of a final sheet at S `s`.
fn panel_of(sheet: &Rgba, s: u32, k: usize) -> Rgba {
    let l = Layout::of(s);
    let (px, py) = l.panel_at(0, k);
    sheet.rect(px, py, l.w, l.h)
}

/// The classic panel at S `s` of a window whose cell (column, row) draws `tiles(c, r)` in that
/// order, by hand: each pixel the colour of the last tile opaque there, the palette's first
/// colour where none is.
fn classic_by_hand(g: &Game, s: u32, tiles: impl Fn(usize, usize) -> Vec<u16>) -> Rgba {
    let (w, h) = super::super::window_px();
    let mut out = Rgba::new(w * s, h * s);
    for r in 0..ROWS {
        for c in 0..COLS {
            let list = tiles(c, r);
            for i in 0..256 {
                let mut colour = g.palette[0];
                for &t in &list {
                    let rec = &g.records[usize::from(t)];
                    if !rec.transparent[i] {
                        colour = g.palette[usize::from(rec.pixels[i])];
                    }
                }
                let (x, y) = ((c * 16 + i % 16) as u32, (r * 16 + i / 16) as u32);
                let [r0, g0, b0] = colour;
                img::fill(&mut out, x * s, y * s, s, s, [r0, g0, b0, 255]);
            }
        }
    }
    out
}

/// Panel `k` of the window in row `row` of a final sheet at S `s` whose rows have no legend.
fn panel_in_row(sheet: &Rgba, s: u32, row: usize, k: usize) -> Rgba {
    let l = Layout::of(s);
    let (px, py) = l.panel_at(row, k);
    sheet.rect(px, py, l.w, l.h)
}

/// Record `r` alone in a cell at S `s`, by hand: each pixel its colour (the palette's first
/// colour where it is clear).
fn record_by_hand(g: &Game, r: u16, s: u32) -> Rgba {
    let mut out = Rgba::new(16 * s, 16 * s);
    let rec = &g.records[usize::from(r)];
    for i in 0..256 {
        let c = if rec.transparent[i] {
            g.palette[0]
        } else {
            g.palette[usize::from(rec.pixels[i])]
        };
        let (x, y) = ((i % 16) as u32 * s, (i / 16) as u32 * s);
        img::fill(&mut out, x, y, s, s, [c[0], c[1], c[2], 255]);
    }
    out
}

/// Cell `(c, r)` of panel `k` of the first window of a final sheet at S `s`.
fn cell_at(sheet: &Rgba, s: u32, k: usize, (c, r): (usize, usize)) -> Rgba {
    let (px, py) = Layout::of(s).panel_at(0, k);
    let cell = 16 * s;
    sheet.rect(px + c as u32 * cell, py + r as u32 * cell, cell, cell)
}

/// Whether each pixel of the pack panel `hd` where a record of `generated` drawn in cell
/// (column, row) by `tiles` is opaque, and in front, is that pixel's colour (the pack's image is
/// the record at ×4, nearest).
fn pack_by_hand(
    g: &Game,
    hd: &Rgba,
    s: u32,
    generated: &[u16],
    tiles: impl Fn(usize, usize) -> Vec<u16>,
) -> usize {
    let mut checked = 0;
    for r in 0..ROWS {
        for c in 0..COLS {
            let list = tiles(c, r);
            for i in 0..256 {
                let front = list
                    .iter()
                    .rev()
                    .find(|&&t| !g.records[usize::from(t)].transparent[i]);
                let Some(&t) = front.filter(|t| generated.contains(t)) else {
                    continue;
                };
                let [r0, g0, b0] = g.palette[usize::from(g.records[usize::from(t)].pixels[i])];
                let (x, y) = ((c * 16 + i % 16) as u32 * s, (r * 16 + i / 16) as u32 * s);
                for d in 0..s * s {
                    assert_eq!(
                        img::get(hd, x + d % s, y + d / s),
                        [r0, g0, b0, 255],
                        "t:{t} ({c}, {r}) {i}"
                    );
                }
                checked += 1;
            }
        }
    }
    checked
}

#[test]
fn the_last_pass_is_what_the_windows_show() {
    let g = game();
    let (text, left) = chain(&g);
    let (dir, work) = work_dir("last", &text, &g, &left);
    let o = options(&dir);
    let s = calibrate_final(&g, &work, &o).unwrap();
    assert_eq!((s.passes, s.logged_last, s.first, s.last), (2, false, 7, 4));
    // One survivor (B's item), placed at a pair it meets.
    let rows = rows_of(&o.out);
    assert_eq!(rows.len(), 1);
    let w = &rows[0];
    assert_eq!(
        (
            w["origin"].as_str(),
            w["class"].as_str(),
            w["placement"].as_str()
        ),
        ("coverage", "terrain", "pair")
    );
    assert_eq!(w["records"], atlas::image_name(ImageId::Tile(B)));
    // Every pair of the last pass drawn in the window, each ticked as the last pass has it: A
    // beside B is gen-mmpx now (A fell), with the measure of the slices, not pass 1's gen-gen.
    let name = |r: u16| atlas::image_name(ImageId::Tile(r));
    let slices = Slices::new(&g, left.clone());
    let ticks: Vec<Vec<&str>> = w["ticks"]
        .split(';')
        .map(|t| t.split(',').collect())
        .collect();
    assert_eq!(ticks.len(), 4);
    assert_eq!(w["pairs"], "4");
    assert_eq!(w["kinds"], "map/gen-mmpx=4");
    let ab = ticks.iter().find(|t| t[5] == name(A)).unwrap();
    let (ratio, limit) = measure(&g, &slices, AB, seam()).unwrap().unwrap();
    let want = [
        "east".to_string(),
        "map".into(),
        "gen-mmpx".into(),
        name(A),
        name(B),
        format!("{ratio:.4}"),
        format!("{limit:.4}"),
    ];
    assert_eq!(ab[2..], want);
    let rec = record(&text).unwrap();
    let first = rec.pairs.iter().find(|l| l.pair == AB).unwrap();
    assert_ne!(Some((ratio, limit)), first.result);
    // The sheet: at A's cell the pack panel is the no-pack panel (A is MMPX in the final pack);
    // at B's cell it is not (B's slice).
    let a_cell: (usize, usize) = (ab[0].parse().unwrap(), ab[1].parse().unwrap());
    let b_cell = (a_cell.0 + 1, a_cell.1);
    let sheet = read_sheet(&o.out.join(&w["sheet"]));
    assert_eq!(cell_of(&sheet, 2, a_cell), cell_of(&sheet, 1, a_cell));
    assert_ne!(cell_of(&sheet, 2, b_cell), cell_of(&sheet, 1, b_cell));
    assert_eq!(s.sheets, ["final-terrain.png", "final-terrain-s2.png"]);
    // Placed at the pair of B's with the largest ratio / limit of the last pass, ranked here
    // from the rebuilt pass's ratios and limits; the least is somewhere else.
    let f = state(&g, &text, &left).unwrap();
    let q = |l: &Last| l.result.map(|(r, lim)| r / lim).unwrap();
    let mut ranked: Vec<&Last> = f.last.iter().collect();
    ranked.sort_by(|a, b| q(b).total_cmp(&q(a)));
    assert!(q(ranked[0]) > q(ranked[1]));
    let place = |l: &Last| {
        let at = l.at.unwrap();
        (at.x.to_string(), at.y.to_string(), edge_name(l.pair.edge))
    };
    let placed = (w["x"].clone(), w["y"].clone(), w["edge"].as_str());
    assert_eq!(placed, place(ranked[0]));
    assert_ne!(placed, place(ranked[3]));
    // The legend under the panels: a line per tick, in their order, with the last pass's
    // measure (A beside B's is not pass 1's), `>` before the pair placed at in A's cell.
    let l = Layout::of(4);
    let ly = l.panel_at(0, 0).1 + l.h + l.margin / 2;
    for (i, t) in ticks.iter().enumerate() {
        let x = f
            .last
            .iter()
            .find(|x| {
                name(x.pair.a) == t[5] && name(x.pair.b) == t[6] && edge_name(x.pair.edge) == t[2]
            })
            .unwrap();
        let (r, lim) = x.result.unwrap();
        let cell: (usize, usize) = (t[0].parse().unwrap(), t[1].parse().unwrap());
        let lead = if std::ptr::eq(x, ranked[0]) && cell == A_CELL {
            ">"
        } else {
            " "
        };
        let e = &t[2][..1];
        let line = format!("{lead}{},{} {e} map gen-mmpx {r:.2}/{lim:.2}", t[0], t[1]);
        let width = l.width() - 2 * l.margin;
        let mut want = Rgba::new(width, l.line_h());
        img::fill(&mut want, 0, 0, width, l.line_h(), BACK);
        super::super::text(&mut want, (0, 0), &line, l.text, width);
        let y = ly + i as u32 * l.line_h();
        assert_eq!(sheet.rect(l.margin, y, width, l.line_h()), want, "{line}");
    }
    // A mark in the margins of each panel at each tick's own cell and edge.
    let cell = 16 * 4;
    for k in 0..3 {
        let (px, py) = l.panel_at(0, k);
        for t in &ticks {
            let (c, r): (u32, u32) = (t[0].parse().unwrap(), t[1].parse().unwrap());
            let (tick, bar) = match t[2] {
                "east" => (
                    (px + (c + 1) * cell - 1, py - 3),
                    (px - 4, py + r * cell + 2),
                ),
                _ => (
                    (px - 3, py + (r + 1) * cell - 1),
                    (px + c * cell + 2, py - 4),
                ),
            };
            assert_eq!(img::get(&sheet, tick.0, tick.1), MARK, "{t:?}");
            assert_eq!(img::get(&sheet, bar.0, bar.1), MARK, "{t:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_window_at_a_south_pair_of_level_1_is_listed_there() {
    // B on level 1 alone, between two records opaque in their middle columns only (the gate
    // measures nothing across B's east or west join): B's window is placed at a south pair, on
    // level 1, and samples.tsv says so.
    const MID: u16 = 6;
    let mut g = game();
    let rec = &mut g.records[usize::from(MID)];
    for i in 0..256 {
        let mid = (4..12).contains(&(i % 16));
        rec.pixels[i] = if mid {
            GRAIN + (hash(i as u32, 7) % 5) as u8
        } else {
            0
        };
        rec.transparent[i] = !mid;
    }
    g.world = World {
        chunks: vec![chunk(&[]), chunk(&[(3, 3, MID), (4, 3, B), (5, 3, MID)])],
        levels: vec![
            Level {
                side: 2,
                refs: vec![0, 0, 0, 0],
            },
            Level {
                side: 2,
                refs: vec![1, 0, 0, 0],
            },
        ],
        objects: Vec::new(),
    };
    let images = BTreeMap::from([(B, near(&g, B))]);
    let (text, left) = gate_log(&g, &images);
    assert_eq!(left, images);
    let (dir, work) = work_dir("south-1", &text, &g, &left);
    let o = options(&dir);
    calibrate_final(&g, &work, &o).unwrap();
    let f = state(&g, &text, &left).unwrap();
    // The first pair in pair order with the largest ratio over limit.
    let most = f
        .last
        .iter()
        .map(Last::excess)
        .fold(f32::NEG_INFINITY, f32::max);
    let placed = f.last.iter().find(|l| l.excess() == most).unwrap();
    assert_eq!(placed.pair.edge, Edge::South);
    assert!(f.last.iter().all(|l| l.at.unwrap().z == 1));
    let at = placed.at.unwrap();
    let rows = rows_of(&o.out);
    let w = rows.iter().find(|r| r["origin"] == "coverage").unwrap();
    assert_eq!(w["placement"], "pair");
    let row = |k: &str| w[k].as_str();
    assert_eq!(
        [row("z"), row("x"), row("y"), row("edge")],
        ["1", &at.x.to_string(), &at.y.to_string(), "south"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_mark_before_the_pair_placed_at_is_at_as_cell_alone() {
    // B at (4, 3) and again at (4, 5), the grass east of both: a window placed at B over the
    // grass draws that pair twice, and its legend leads with `>` only the line at A's cell.
    let mut g = game();
    g.world.chunks[0] = chunk(&[(4, 3, B), (4, 5, B)]);
    let images = BTreeMap::from([(B, near(&g, B))]);
    let (text, left) = gate_log(&g, &images);
    assert_eq!(left, images);
    let f = state(&g, &text, &left).unwrap();
    let li = f.last.iter().position(|l| l.pair == BG).unwrap();
    let by: BTreeMap<Pair, usize> = f
        .last
        .iter()
        .enumerate()
        .map(|(i, l)| (l.pair, i))
        .collect();
    let w = Window {
        origin: Origin::Coverage,
        survivor: 0,
        records: vec![B],
        placement: Placement::Pair,
        at: f.last[li].at,
        edge: Edge::East,
        pair: Some(li),
        anchor: None,
        pieces: Vec::new(),
        part: (1, 1),
        rest: false,
    };
    let d = draw_window(&g, &f, &by, &w);
    let cells: Vec<(usize, usize)> = d.ticks.iter().filter(|t| t.1 == li).map(|t| t.0).collect();
    assert_eq!(cells, [A_CELL, (A_CELL.0, A_CELL.1 + 2)]);
    let legend = legend_of(&f, &w, &d);
    let led: Vec<&String> = legend.iter().filter(|l| l.starts_with('>')).collect();
    let (c, r) = A_CELL;
    assert_eq!(led.len(), 1, "{legend:?}");
    assert!(
        led[0].starts_with(&format!(">{c},{r} e map ")),
        "{legend:?}"
    );
    let other = format!(" {c},{} e map ", r + 2);
    assert!(legend.iter().any(|l| l.starts_with(&other)), "{legend:?}");
}

#[test]
fn a_window_is_placed_at_the_largest_ratio_over_limit() {
    // Three pairs of record 9 whose limits differ: by ratio alone the third is the largest, by
    // ratio / limit the second and the third tie (2.0) and the second is first in pair order;
    // the first is the least (1.5). A fourth, of another label, is larger still (3.0).
    let last = |b: u16, ratio: f32, limit: f32, labels: &str| Last {
        line: usize::from(b),
        pair: Pair {
            a: 9,
            b,
            edge: Edge::East,
        },
        sides: Sides::GenMmpx,
        result: Some((ratio, limit)),
        labels: labels.into(),
        at: Some(At { z: 0, x: 0, y: 0 }),
        first: (Sides::GenMmpx, Some((ratio, limit))),
    };
    let fixed = Fixed {
        last: vec![
            last(1, 3.0, 2.0, "map"),
            last(2, 2.0, 1.0, "map"),
            last(3, 4.0, 2.0, "map"),
            last(4, 4.5, 1.5, "objects"),
        ],
        ..Fixed::default()
    };
    assert_eq!(at_pair(&fixed, &[9], Some("map")), Some(1));
    assert_eq!(at_pair(&fixed, &[9], None), Some(3));
    // A pair the records do not meet is not one to place at.
    assert_eq!(at_pair(&fixed, &[8], None), None);
}

#[test]
fn one_window_per_surviving_item_and_a_multi_record_item_is_one() {
    let mut g = game();
    // The tree (four records, one item) and a chest drawn in the world; a sword drawn nowhere.
    let tree = synthetic::TREE;
    g.world.objects = vec![
        crate::game::Placed {
            x: 10,
            y: 10,
            z: 0,
            tile: tree + 3,
        },
        crate::game::Placed {
            x: 12,
            y: 12,
            z: 0,
            tile: synthetic::OBJECTS,
        },
        // The chest again, later in the lists and earlier in the world (row 5).
        crate::game::Placed {
            x: 4,
            y: 5,
            z: 0,
            tile: synthetic::OBJECTS,
        },
    ];
    let sword = synthetic::OBJECTS + 2;
    let records = [
        tree,
        tree + 1,
        tree + 2,
        tree + 3,
        synthetic::OBJECTS,
        sword,
    ];
    let images: BTreeMap<u16, Rgba> = records.iter().map(|&r| (r, near(&g, r))).collect();
    let (text, left) = gate_log(&g, &images);
    assert_eq!(left, images);
    let (dir, work) = work_dir("items", &text, &g, &images);
    let o = options(&dir);
    let s = calibrate_final(&g, &work, &o).unwrap();
    let rows = rows_of(&o.out);
    // Three items, three windows: the tree once, with its four records.
    assert_eq!(rows.len(), 3);
    let by_class = |c: &str| rows.iter().filter(|r| r["class"] == c).collect::<Vec<_>>();
    let trees = by_class("multi");
    assert_eq!(trees.len(), 1);
    let names: Vec<String> = (0..4)
        .map(|k| atlas::image_name(ImageId::Tile(tree + k)))
        .collect();
    assert_eq!(trees[0]["records"], names.join("+"));
    // At a pair of its parts (the gate checks them, both generated), the tree's top-left part
    // in A's cell.
    assert_eq!(
        (
            trees[0]["placement"].as_str(),
            trees[0]["x"].as_str(),
            trees[0]["y"].as_str()
        ),
        ("pair", "9", "9")
    );
    // The chest in no pair: where the world first draws it (the least cell, not the first in
    // the lists), and said; the sword alone.
    let objects = by_class("object");
    let places: Vec<&str> = objects.iter().map(|r| r["placement"].as_str()).collect();
    assert_eq!(places, ["drawn", "alone"]);
    assert_eq!(
        (objects[0]["x"].as_str(), objects[0]["y"].as_str()),
        ("4", "5")
    );
    assert!(objects[0]["note"].starts_with("in no pair"));
    assert!(objects[1]["note"].starts_with("drawn nowhere"));
    // The window of a multi-record item draws all of it: each of its records in the frame.
    let class = |c: Class| s.classes.iter().find(|x| x.class == c).unwrap().clone();
    let m = class(Class::Multi);
    assert_eq!(
        (m.items, m.images, m.pair, m.drawn, m.alone),
        (1, 4, 1, 0, 0)
    );
    let ob = class(Class::Object);
    assert_eq!(
        (ob.items, ob.images, ob.pair, ob.drawn, ob.alone),
        (2, 2, 0, 1, 1)
    );
    // Each window as the sheet has it, against its pixels by hand, at both scales: the classic
    // panel whole, the pack panel where a generated record is in front. The tree's window
    // (A's cell at (9, 9)): its four parts on the grass, and the chest at (12, 12); the
    // chest's (at (4, 5)): X, A and B in its first row; the sword's alone, in the second
    // column and row of an empty window.
    let chest = synthetic::OBJECTS;
    let generated: Vec<u16> = records.to_vec();
    let world = |x0: usize, y0: usize| {
        move |c: usize, r: usize| -> Vec<u16> {
            let (x, y) = (x0 + c, y0 + r);
            let base = match (x, y) {
                (2, 3) => X,
                (3, 3) => A,
                (4, 3) => B,
                _ => GRASS,
            };
            let mut list = vec![base];
            match (x, y) {
                (9, 9) => list.push(tree),
                (10, 9) => list.push(tree + 1),
                (9, 10) => list.push(tree + 2),
                (10, 10) => list.push(tree + 3),
                (12, 12) | (4, 5) => list.push(chest),
                _ => {}
            }
            list
        }
    };
    let alone_cells = |c: usize, r: usize| {
        if (c, r) == (1, 1) {
            vec![sword]
        } else {
            vec![]
        }
    };
    for (sheet, cells) in [
        (
            "final-multi",
            Box::new(world(6, 7)) as Box<dyn Fn(usize, usize) -> Vec<u16>>,
        ),
        ("final-object", Box::new(world(1, 3))),
        ("final-object-alone", Box::new(alone_cells)),
    ] {
        for (s, file) in [(4, format!("{sheet}.png")), (2, format!("{sheet}-s2.png"))] {
            let png = read_sheet(&o.out.join(&file));
            assert_eq!(
                panel_of(&png, s, 0),
                classic_by_hand(&g, s, &cells),
                "{file}"
            );
            let n = pack_by_hand(&g, &panel_of(&png, s, 2), s, &generated, &cells);
            assert!(n > 0, "{file}: {n}");
        }
    }
    // The sheets: drawn windows by class, the alone one apart.
    assert_eq!(
        s.sheets,
        [
            "final-multi.png",
            "final-multi-s2.png",
            "final-object.png",
            "final-object-s2.png",
            "final-object-alone.png",
            "final-object-alone-s2.png"
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_included_id_gets_its_window_or_a_row_of_zero() {
    let g = game();
    let (text, left) = chain(&g);
    let (dir, work) = work_dir("include", &text, &g, &left);
    let list = dir.join("ids.txt");
    let ids = format!(
        "# the owner's list\n{}\n\n  {}  # sent back\nt:{}\n{}\n",
        atlas::image_name(ImageId::Tile(B)),
        A,
        X,
        atlas::image_name(ImageId::Tile(B)),
    );
    std::fs::write(&list, ids).unwrap();
    let o = FinalOptions {
        include: Some(list),
        ..options(&dir)
    };
    let s = calibrate_final(&g, &work, &o).unwrap();
    // B once (each id once), A and X not generated.
    assert_eq!(s.include, Some((3, 1, 2)));
    let rows = rows_of(&o.out);
    let inc: Vec<&BTreeMap<String, String>> =
        rows.iter().filter(|r| r["origin"] == "include").collect();
    assert_eq!(inc.len(), 3);
    assert_eq!(inc[0]["records"], atlas::image_name(ImageId::Tile(B)));
    assert_eq!(inc[0]["placement"], "pair");
    assert_eq!(inc[0]["sheet"], "final-include.png");
    for (r, id) in inc[1..].iter().zip([A, X]) {
        assert_eq!(r["records"], atlas::image_name(ImageId::Tile(id)));
        assert_eq!(r["note"], "not generated in the final pack (0)");
        assert_eq!((r["sheet"].as_str(), r["pairs"].as_str()), ("-", "0"));
    }
    assert!(
        s.text()
            .contains("--include: 3 ids, 1 windows, 2 not generated")
    );
    // The class counts are the coverage windows': B's include window is not one of them.
    let t = s
        .classes
        .iter()
        .find(|c| c.class == Class::Terrain)
        .unwrap();
    assert_eq!((t.pair, t.drawn, t.alone), (1, 0, 0));
    // With one label, an included id still takes a pair of any label (B meets none of the
    // objects): it is drawn though no coverage window is.
    let o = FinalOptions {
        source: Source::Label("objects".into()),
        out: dir.join("objects"),
        ..o
    };
    let s = calibrate_final(&g, &work, &o).unwrap();
    assert_eq!(s.include, Some((3, 1, 2)));
    let rows = rows_of(&o.out);
    let inc: Vec<&BTreeMap<String, String>> =
        rows.iter().filter(|r| r["origin"] == "include").collect();
    assert_eq!(inc[0]["placement"], "pair");
    assert_eq!(rows.iter().filter(|r| r["origin"] == "coverage").count(), 0);
    // The list's lines: an id, a bare number, a comment; anything else is refused by line.
    assert_eq!(include_ids("t:5\n7 # x\n#t:9\n\nt:5\n"), Ok(vec![5, 7]));
    for bad in ["p:a:1", "t:x", "seven", "t:99999", "t:5 t:6"] {
        let e = include_ids(&format!("t:1\n{bad}\n")).unwrap_err();
        assert!(e.contains("line 2"), "{bad}: {e}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_summary_prints_every_label_and_sides_and_class_with_its_zero() {
    let g = game();
    let (text, left) = chain(&g);
    // A label the gate does not write yet (in a label field, as H7 writes its labels), on the
    // grass over B and B beside the grass.
    let text = labelled(&text, &[GB_S, BG], "odd");
    let (dir, work) = work_dir("zeros", &text, &g, &left);
    let o = options(&dir);
    let s = calibrate_final(&g, &work, &o).unwrap();
    let kinds: Vec<(&str, Sides, usize, usize, usize)> = s
        .kinds
        .iter()
        .map(|k| (k.label.as_str(), k.sides, k.last, k.read, k.windows))
        .collect();
    let mut want = Vec::new();
    for l in ["map", "objects", "replace", "object-terrain", "odd"] {
        let n = match l {
            "map" => 4,
            "odd" => 2,
            _ => 0,
        };
        want.push((l, Sides::GenGen, 0, 0, 0));
        want.push((l, Sides::GenMmpx, n, n, n.min(1)));
    }
    assert_eq!(kinds, want);
    // Every label × sides printed, its numbers those above.
    let text = s.text();
    for (l, sides, last, read, windows) in &want {
        let line = format!(
            "\n{l} {}: {last} in the last pass, {read} read, {windows} windows\n",
            sides.name()
        );
        assert!(text.contains(&line), "{line}\n{text}");
    }
    // Every class, survivors or not, each printed with its numbers: B's item placed at a pair,
    // its record drawn; every other class 0 in every column.
    assert_eq!(
        s.classes.iter().map(|c| c.class).collect::<Vec<_>>(),
        Class::ALL
    );
    for class in Class::ALL {
        let line = text
            .lines()
            .find(|l| l.starts_with(class.name()))
            .unwrap_or_else(|| panic!("{}\n{text}", class.name()));
        let fields: Vec<&str> = line[class.name().len()..].split_whitespace().collect();
        let want: [&str; 8] = if class == Class::Terrain {
            ["1", "1", "1", "0", "0", "1/1", "0", "0"]
        } else {
            ["0", "0", "0", "0", "0", "0/0", "0", "0"]
        };
        assert_eq!(fields, want, "{line}");
    }
    // One label: only a survivor that meets a pair of it gets a window; the others are counted.
    let o = FinalOptions {
        source: Source::Label("objects".into()),
        out: dir.join("objects"),
        ..options(&dir)
    };
    let s = calibrate_final(&g, &work, &o).unwrap();
    let t = s
        .classes
        .iter()
        .find(|c| c.class == Class::Terrain)
        .unwrap();
    assert_eq!((t.items, t.pair, t.unlabelled), (1, 0, 1));
    // Its record drawn in no window: counted so, 0 of 1.
    assert_eq!((t.records, t.shown), (1, 0));
    let line = s.text();
    let line = line
        .lines()
        .find(|l| l.starts_with(Class::Terrain.name()))
        .unwrap();
    assert!(line.contains(" 0/1 "), "{line}");
    assert!(s.sheets.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The details of the disagreements of `text` with the images `left`, `list` listed and the
/// masters `files`.
fn details_of(
    g: &Game,
    text: &str,
    left: &BTreeMap<u16, Rgba>,
    list: &[Accepted],
    files: &BTreeMap<ImageId, String>,
) -> Vec<String> {
    let plan = group::plan(g);
    let slices = Slices::new(g, left.clone());
    let d = fixed(
        g,
        &plan,
        &record(text).unwrap(),
        list,
        files,
        &slices,
        seam(),
    )
    .unwrap_err();
    d.iter()
        .map(|d| format!("{}: {}", d.kind, d.detail))
        .collect()
}

#[test]
fn an_item_listed_in_part_or_an_image_of_no_item_is_a_disagreement() {
    // The tree with one of its four records left out of the list and of slices/ (an ingest
    // that wrote only part of it).
    let mut g = game();
    let tree = synthetic::TREE;
    g.world.objects = vec![crate::game::Placed {
        x: 10,
        y: 10,
        z: 0,
        tile: tree + 3,
    }];
    let images: BTreeMap<u16, Rgba> = (tree..tree + 3).map(|r| (r, near(&g, r))).collect();
    let (text, left) = gate_log(&g, &images);
    assert_eq!(left, images);
    let plan = group::plan(&g);
    let item = item_of(&plan, tree);
    assert_eq!(
        details_of(&g, &text, &images, &listed(&g, &images), &digests(&images)),
        [format!(
            "part: item {item}: 3 of its 4 images that draw are listed"
        )]
    );
    let (dir, work) = work_dir("part", &text, &g, &images);
    let o = options(&dir);
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(e.contains("disagree, nothing drawn: part 1"), "{e}");
    assert_eq!(
        files_of(&o.out),
        [
            "disagreements.txt",
            "gates.jsonl",
            "ingest.json",
            "jobs.jsonl",
            "refused.txt"
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
    // A listed image no item has: a shore record (the scaler draws the shore), listed with
    // its master; and a master of a record the game has no image of (past its records).
    let g = game();
    let (text, left) = chain(&g);
    let mut with = left.clone();
    with.insert(synthetic::SHORE, near(&g, synthetic::SHORE));
    let d = details_of(&g, &text, &with, &listed(&g, &with), &digests(&with));
    let shore = atlas::image_name(ImageId::Tile(synthetic::SHORE));
    assert_eq!(d, [format!("plan: {shore}: listed, and no item has it")]);
    let past = g.records.len() as u16 + 1;
    let mut files = digests(&left);
    files.insert(ImageId::Tile(past), "0".repeat(16));
    let d = details_of(&g, &text, &left, &listed(&g, &left), &files);
    assert_eq!(
        d,
        [
            format!("slice: t:{past}: a master ingest.json does not list"),
            format!("slice: t:{past}: a master of an image the game has not"),
        ]
    );
}

#[test]
fn a_window_at_a_pair_draws_the_pairs_records() {
    // A made-up tile that animates through two grains of its own, at (6, 6) in the grass, its
    // second frame generated: placed at a pair of that frame, the window draws the tile as that
    // frame, not at frame 0.
    let mut g = game();
    let (tile, f0, f1) = (6u16, 7u16, 8u16);
    for (seed, r) in [(11, f0), (12, f1)] {
        let rec = &mut g.records[usize::from(r)];
        for i in 0..256 {
            rec.pixels[i] = GRAIN + (hash(i as u32, seed) % 5) as u8;
            rec.transparent[i] = false;
        }
    }
    g.anim.push(u6_formats::animdata::AnimEntry {
        tile_to_animate: tile,
        first_anim_frame: f0,
        and_mask: 1,
        shift: 0,
    });
    g.world.chunks[0][6][6] = tile as u8;
    let images = BTreeMap::from([(f1, near(&g, f1))]);
    let (text, left) = gate_log(&g, &images);
    assert_eq!(left, images);
    let (dir, work) = work_dir("pair-frame", &text, &g, &images);
    let o = options(&dir);
    let s = calibrate_final(&g, &work, &o).unwrap();
    assert_eq!(s.unread, 0);
    let rows = rows_of(&o.out);
    let w = rows.iter().find(|r| r["origin"] == "coverage").unwrap();
    assert_eq!(w["placement"], "pair");
    assert_eq!(w["frames"], "pair");
    let name = atlas::image_name(ImageId::Tile(f1));
    assert!(w["ticks"].contains(&name), "{}", w["ticks"]);
    // The sheet draws the tile's cell as the second frame, not the first, in the classic and the
    // pack panels, at both scales.
    let (x, y): (usize, usize) = (w["x"].parse().unwrap(), w["y"].parse().unwrap());
    let cell = (6 + A_CELL.0 - x, 6 + A_CELL.1 - y);
    for (s, file) in [(4, w["sheet"].clone()), (2, w["sheet_s2"].clone())] {
        let sheet = read_sheet(&o.out.join(&file));
        let (want, frame0) = (record_by_hand(&g, f1, s), record_by_hand(&g, f0, s));
        assert_ne!(want, frame0);
        for k in [0, 2] {
            assert_eq!(cell_at(&sheet, s, k, cell), want, "{file} {k}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_disagreement_fails_and_draws_nothing() {
    let g = game();
    let (text, left) = chain(&g);
    // A's master left in slices/ and listed, though the gate sent A back.
    let mut kept = left.clone();
    kept.insert(A, red_west(&near(&g, A)));
    let (dir, work) = work_dir("disagree", &text, &g, &kept);
    let o = options(&dir);
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(
        e.contains("disagree, nothing drawn: failing 1, fallen 1"),
        "{e}"
    );
    // No sheet, no samples.tsv; the details (record ids) in a file, not in the message.
    assert_eq!(
        files_of(&o.out),
        [
            "disagreements.txt",
            "gates.jsonl",
            "ingest.json",
            "jobs.jsonl",
            "refused.txt"
        ]
    );
    assert!(!e.contains("t:"));
    let detail = std::fs::read_to_string(o.out.join("disagreements.txt")).unwrap();
    assert_eq!(detail.lines().count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Made-up records: HALF, a grain opaque on its lower half; COVER, opaque on its east
/// column's lower half alone.
const HALF: u16 = 201;
const COVER: u16 = 202;

fn with_half_and_cover(mut g: Game) -> Game {
    for i in 0..256 {
        let (u, v) = (i % 16, i / 16);
        let h = &mut g.records[usize::from(HALF)];
        h.pixels[i] = GRAIN + (hash(i as u32, 11) % 5) as u8;
        h.transparent[i] = v < 8;
        let c = &mut g.records[usize::from(COVER)];
        c.pixels[i] = GRAIN + 4;
        c.transparent[i] = !(u == 15 && v >= 8);
    }
    g
}

#[test]
fn a_pair_is_drawn_only_where_a_row_the_gate_measures_shows() {
    // B, HALF east of it: the gate measures the join on HALF's lower rows alone, where both
    // are opaque on their lines. COVER over B covers B's east column on those rows: each side
    // still shows somewhere on its line, and no measured row of the join is left.
    let g = with_half_and_cover(game());
    let a = A_CELL.1 * COLS + A_CELL.0;
    let cells = |list: &[u16]| {
        let mut cells = vec![Stack::default(); COLS * ROWS];
        cells[a] = Stack {
            base: Some(B),
            list: list.to_vec(),
        };
        cells[a + 1].base = Some(HALF);
        cells
    };
    let p = Pair {
        a: B,
        b: HALF,
        edge: Edge::East,
    };
    let slices = Slices::new(&g, BTreeMap::from([(B, near(&g, B))]));
    assert!(
        measure(&g, &slices, p, seam())
            .unwrap()
            .is_some_and(gate::seam_passes)
    );
    let f = framed(&g, &cells(&[COVER]), &[]);
    assert!(f.shows(A_CELL, B) && f.shows((A_CELL.0 + 1, A_CELL.1), HALF));
    assert!(!f.joins(A_CELL, p));
    // Uncovered, it is drawn; and COVER beside HALF, whose measured rows (the lower ones) are
    // left on both sides, is drawn.
    assert!(framed(&g, &cells(&[]), &[]).joins(A_CELL, p));
    let cover = Pair { a: COVER, ..p };
    assert!(f.joins(A_CELL, cover));
}

#[test]
fn a_pair_is_drawn_only_where_a_line_the_gate_measures_shows_on_either_edge_and_side() {
    // As above, east and south, the cover on the first side and on the second: one record
    // (`half`) opaque on half of its line at the join, the other (B) opaque on all of it, so
    // the gate measures that half alone; a cover over B hides B's pixels there. Each side still
    // shows on its line, and no measured row (or column) of the join is left.
    let mut g = game();
    let made = |g: &mut Game, r: u16, opaque: &dyn Fn(usize, usize) -> bool| {
        let rec = &mut g.records[usize::from(r)];
        for i in 0..256 {
            rec.pixels[i] = GRAIN + (hash(i as u32, u32::from(r)) % 5) as u8;
            rec.transparent[i] = !opaque(i % 16, i / 16);
        }
    };
    // Opaque on the lower rows; on the right columns.
    let (lower, right) = (203u16, 204u16);
    made(&mut g, lower, &|_, v| v >= 8);
    made(&mut g, right, &|u, _| u >= 8);
    let a = A_CELL.1 * COLS + A_CELL.0;
    let slices = Slices::new(&g, BTreeMap::from([(B, near(&g, B))]));
    // (edge, B first or second, the half record, the cover's opaque pixels over B's cell).
    type Cover = fn(usize, usize) -> bool;
    let cases: [(Edge, bool, u16, Cover); 4] = [
        (Edge::East, false, lower, |u, v| u == 0 && v >= 8),
        (Edge::South, true, right, |u, v| v == 15 && u >= 8),
        (Edge::South, false, right, |u, v| v == 0 && u >= 8),
        (Edge::East, true, lower, |u, v| u == 15 && v >= 8),
    ];
    for (k, (edge, b_first, half, cover)) in cases.into_iter().enumerate() {
        let c = 205 + k as u16;
        made(&mut g, c, &cover);
        let p = if b_first {
            Pair {
                a: B,
                b: half,
                edge,
            }
        } else {
            Pair {
                a: half,
                b: B,
                edge,
            }
        };
        let other = match edge {
            Edge::East => a + 1,
            Edge::South => a + COLS,
        };
        let (b_at, half_at) = if b_first { (a, other) } else { (other, a) };
        let cells = |list: &[u16]| {
            let mut cells = vec![Stack::default(); COLS * ROWS];
            cells[b_at] = Stack {
                base: Some(B),
                list: list.to_vec(),
            };
            cells[half_at].base = Some(half);
            cells
        };
        let m = measure(&g, &slices, p, seam()).unwrap();
        assert!(m.is_some_and(gate::seam_passes), "{k}: {m:?}");
        let at = |i: usize| (i % COLS, i / COLS);
        let f = framed(&g, &cells(&[c]), &[]);
        assert!(f.shows(at(b_at), B) && f.shows(at(half_at), half), "{k}");
        assert!(!f.joins(A_CELL, p), "{k}");
        assert!(framed(&g, &cells(&[]), &[]).joins(A_CELL, p), "{k}");
    }
}

#[test]
fn a_pair_with_nothing_measured_and_a_side_covered_in_its_cell_is_hidden() {
    // The reviewer's P16: T clear on its west line, so the gate measures nothing across B's
    // east join and the cell rule holds; B covered whole in its cell (then T instead): the
    // pair is not drawn there, and is listed hidden, not dropped.
    const T: u16 = 206;
    const ALL: u16 = 207;
    let mut g = game();
    for i in 0..256 {
        let east = i % 16 >= 8;
        let t = &mut g.records[usize::from(T)];
        t.pixels[i] = if east {
            GRAIN + (hash(i as u32, 7) % 5) as u8
        } else {
            0
        };
        t.transparent[i] = !east;
        let c = &mut g.records[usize::from(ALL)];
        c.pixels[i] = GRAIN + 1;
        c.transparent[i] = false;
    }
    let a = A_CELL.1 * COLS + A_CELL.0;
    let p = Pair {
        a: B,
        b: T,
        edge: Edge::East,
    };
    let by = BTreeMap::from([(p, 0usize)]);
    for covered in [a, a + 1] {
        let mut cells = vec![Stack::default(); COLS * ROWS];
        cells[a].base = Some(B);
        cells[a + 1].base = Some(T);
        cells[covered].list = vec![ALL];
        let f = framed(&g, &cells, &[]);
        let (b, t) = (f.shows(A_CELL, B), f.shows((A_CELL.0 + 1, A_CELL.1), T));
        assert_eq!((b, t), (covered != a, covered == a), "{covered}");
        assert!(!f.joins(A_CELL, p), "{covered}");
        assert_eq!(hidden(&f, &by), [(A_CELL, 0)], "{covered}");
        assert!(ticks(&f, &by).is_empty(), "{covered}");
    }
}

#[test]
fn lines_opaque_on_no_common_row_fall_back_to_the_cell_rule() {
    // The reviewer's P11: UPPER's east column opaque on rows 0-7 alone, HALF's west on 8-15:
    // both lines opaque, on no common row, so the gate measures nothing across the join and the
    // pair is drawn where each side shows in its cell.
    const UPPER: u16 = 203;
    let mut g = with_half_and_cover(game());
    for i in 0..256 {
        let (u, v) = (i % 16, i / 16);
        let h = &mut g.records[usize::from(UPPER)];
        h.pixels[i] = GRAIN + (hash(i as u32, 5) % 5) as u8;
        h.transparent[i] = u == 15 && v >= 8;
    }
    let a = A_CELL.1 * COLS + A_CELL.0;
    let mut cells = vec![Stack::default(); COLS * ROWS];
    cells[a].base = Some(UPPER);
    cells[a + 1].base = Some(HALF);
    let p = Pair {
        a: UPPER,
        b: HALF,
        edge: Edge::East,
    };
    let slices = Slices::new(&g, BTreeMap::from([(UPPER, near(&g, UPPER))]));
    assert_eq!(measure(&g, &slices, p, seam()).unwrap(), None);
    let f = framed(&g, &cells, &[]);
    assert!(f.shows(A_CELL, UPPER) && f.shows((A_CELL.0 + 1, A_CELL.1), HALF));
    assert!(f.joins(A_CELL, p));
}

#[test]
fn the_second_side_of_a_south_join_is_read_on_its_north_line() {
    // The reviewer's P12b: HALFN opaque on its upper half's right columns alone (its south row
    // clear, its north row opaque on columns 8-15), south of B; COVERS over B on B's south
    // row's right columns: no measured column of the join left, not drawn. Read on its south
    // row, HALFN's line would have nothing measured, and the cell rule would draw it. (The
    // reviewer's P12, B over a record opaque on its right half, is the south case of
    // `a_pair_is_drawn_only_where_a_line_the_gate_measures_shows_on_either_edge_and_side`.)
    const HALFN: u16 = 208;
    const COVERS: u16 = 209;
    let mut g = game();
    for i in 0..256 {
        let (u, v) = (i % 16, i / 16);
        let h = &mut g.records[usize::from(HALFN)];
        h.pixels[i] = GRAIN + (hash(i as u32, 17) % 5) as u8;
        h.transparent[i] = !(u >= 8 && v < 8);
        let c = &mut g.records[usize::from(COVERS)];
        c.pixels[i] = GRAIN + 4;
        c.transparent[i] = !(v == 15 && u >= 8);
    }
    let a = A_CELL.1 * COLS + A_CELL.0;
    let cells = |list: &[u16]| {
        let mut cells = vec![Stack::default(); COLS * ROWS];
        cells[a] = Stack {
            base: Some(B),
            list: list.to_vec(),
        };
        cells[a + COLS].base = Some(HALFN);
        cells
    };
    let p = Pair {
        a: B,
        b: HALFN,
        edge: Edge::South,
    };
    let f = framed(&g, &cells(&[COVERS]), &[]);
    assert!(f.shows(A_CELL, B) && f.shows((A_CELL.0, A_CELL.1 + 1), HALFN));
    assert!(!f.joins(A_CELL, p));
    assert!(framed(&g, &cells(&[]), &[]).joins(A_CELL, p));
}

#[test]
fn a_pair_covered_on_every_measured_row_is_not_read() {
    // The chain, and B again at (12, 12) with HALF east of it, COVER on B there: the pair's one
    // cell has no measured row left, so no window reads it, and the run fails.
    let mut g = with_half_and_cover(game());
    g.world.chunks.push(chunk(&[(4, 4, B), (5, 4, HALF)]));
    g.world.levels[0].refs = vec![0, 1, 1, 2];
    g.world.objects = vec![crate::game::Placed {
        x: 12,
        y: 12,
        z: 0,
        tile: COVER,
    }];
    let (text, left) = chain(&g);
    assert!(left.contains_key(&B));
    let (dir, work) = work_dir("measured-row", &text, &g, &left);
    let o = options(&dir);
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(e.contains("nothing drawn: unread 1"), "{e}");
    let detail = std::fs::read_to_string(o.out.join("unshown.txt")).unwrap();
    assert!(detail.contains(&format!("t:{B} t:{HALF} east")), "{detail}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pair_covered_where_the_world_draws_it_is_not_read_there() {
    // The chain, and an object of X's opaque record on the grass east of B: the only cell the
    // world has B beside the grass draws the grass covered.
    let mut g = game();
    let over = |tile: u16| crate::game::Placed {
        x: 5,
        y: 3,
        z: 0,
        tile,
    };
    // A made-up record opaque on its west column alone (an object of it below).
    const WEST: u16 = 200;
    let rec = &mut g.records[usize::from(WEST)];
    for v in 0..16 {
        rec.pixels[v * 16] = GRAIN;
        rec.transparent[v * 16] = false;
    }
    g.world.objects = vec![over(X)];
    let (text, left) = chain(&g);
    let (dir, work) = work_dir("covered", &text, &g, &left);
    let o = options(&dir);
    // B's window does not read it, nor the pair's own window at its cell: the run fails before
    // a sheet or samples.tsv is written, the pair's line in unshown.txt.
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(
        e.contains("every pair of the last pass, nothing drawn: unread 1"),
        "{e}"
    );
    assert!(!e.contains("t:"), "{e}");
    assert_eq!(
        files_of(&o.out),
        [
            "gates.jsonl",
            "ingest.json",
            "jobs.jsonl",
            "refused.txt",
            "unshown.txt"
        ]
    );
    let detail = std::fs::read_to_string(o.out.join("unshown.txt")).unwrap();
    assert_eq!(
        detail,
        format!(
            "unread: line 6: t:{B} t:{GRASS} east (map gen-mmpx) at 0,4,3: drawn in no window\n"
        )
    );
    // An object opaque on its west column alone: the grass shows in its cell, and not along
    // the join, so the pair is not drawn there either.
    g.world.objects = vec![over(WEST)];
    let o = FinalOptions {
        out: dir.join("west"),
        ..options(&dir)
    };
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(e.contains("nothing drawn: unread 1"), "{e}");
    // An object that leaves the grass's west side (the chest's record): read in B's window.
    g.world.objects = vec![over(synthetic::OBJECTS)];
    let o = FinalOptions {
        out: dir.join("chest"),
        ..options(&dir)
    };
    let s = calibrate_final(&g, &work, &o).unwrap();
    assert_eq!((s.pair_windows, s.unread), (0, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pair_covered_in_a_window_is_marked_hidden_there() {
    // B again at (4, 5), and an object of X's opaque record on the grass east of it: B beside
    // the grass shows at (4, 3), where the gate found it, and is covered at (4, 5), which B's
    // window draws too.
    let mut g = game();
    g.world.chunks[0][5][4] = B as u8;
    g.world.objects = vec![crate::game::Placed {
        x: 5,
        y: 5,
        z: 0,
        tile: X,
    }];
    let (text, left) = chain(&g);
    let (dir, work) = work_dir("hidden-tick", &text, &g, &left);
    let o = options(&dir);
    let s = calibrate_final(&g, &work, &o).unwrap();
    assert_eq!(s.unread, 0);
    let rows = rows_of(&o.out);
    let w = &rows[0];
    let (x, y): (usize, usize) = (w["x"].parse().unwrap(), w["y"].parse().unwrap());
    let cell = (4 + A_CELL.0 - x, 5 + A_CELL.1 - y);
    assert_eq!(
        w["hidden"],
        format!("{},{},east,map,gen-mmpx,t:{B},t:{GRASS}", cell.0, cell.1)
    );
    // Read where it shows: its tick there, not the hidden one.
    let shown = format!(
        "{},{},east,map,gen-mmpx,t:{B},t:{GRASS}",
        4 + A_CELL.0 - x,
        3 + A_CELL.1 - y
    );
    assert!(w["ticks"].contains(&shown), "{}", w["ticks"]);
    // Marked in its own colour, dashed, over the read ones (the two share a column, and the
    // hidden one a row with other read pairs): both colours along its row and above its join,
    // only the read one's beside the row of the one read.
    let sheet = read_sheet(&o.out.join(&w["sheet"]));
    let l = Layout::of(4);
    let py = l.margin / 2 + l.caption_h() + l.margin;
    let px = |k: u32| l.margin + k * (l.w + l.margin);
    let bar = |k: u32, r: usize| -> BTreeSet<[u8; 4]> {
        (0..64)
            .map(|v| img::get(&sheet, px(k) - 4, py + r as u32 * 64 + v))
            .collect()
    };
    let tick = |k: u32, c: usize| -> BTreeSet<[u8; 4]> {
        let x = px(k) + (c as u32 + 1) * 64 - 1;
        (3..l.margin - 2)
            .map(|d| img::get(&sheet, x, py - d))
            .collect()
    };
    for k in 0..3 {
        assert_eq!(bar(k, cell.1), BTreeSet::from([MARK, MARK_HIDDEN]));
        assert_eq!(tick(k, cell.0), BTreeSet::from([MARK, MARK_HIDDEN]));
        assert_eq!(bar(k, 3 + A_CELL.1 - y), BTreeSet::from([MARK]));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The pairs a window's row of `samples.tsv` marks.
fn n_pairs(row: &BTreeMap<String, String>) -> usize {
    row["pairs"].parse().unwrap()
}

#[test]
fn a_long_legend_line_goes_on_and_is_not_cut() {
    let g = game();
    let (text, left) = chain(&g);
    // A label long enough that its legend line is wider than the sheet at S 2.
    let long = "a-label-the-gate-does-not-write-yet-and-whose-name-runs-on-past-a-sheet-at-s2";
    let text = labelled(&text, &[BG], long);
    let (dir, work) = work_dir("legend", &text, &g, &left);
    let o = options(&dir);
    let s = calibrate_final(&g, &work, &o).unwrap();
    for (scale, sheet) in [(4, &s.sheets[0]), (2, &s.sheets[1])] {
        let l = Layout::of(scale);
        let fits = |t: &str| (t.chars().count() as u32 - 1) * 4 * l.text + 3 * l.text;
        let legend = vec![
            format!(" 4,2 e map+{long} gen-mmpx 0.95/1.50"),
            "short".into(),
        ];
        let lines = wrapped(&l, &legend);
        // Every line fits the sheet; joined again, the lines are the legend.
        assert!(
            lines.iter().all(|t| fits(t) <= l.width() - 2 * l.margin),
            "{lines:?}"
        );
        let back: String = lines
            .iter()
            .map(|t| t.strip_prefix("  ").unwrap_or(t))
            .collect();
        assert_eq!(back, legend.concat());
        assert_eq!(lines.len() > legend.len(), scale == 2, "{scale}");
        // The sheet is as tall as the lines it writes: at S 4 a line per pair of the window, at
        // S 2 more, the long one going on.
        let (w, h) = img::png_size(&std::fs::read(o.out.join(sheet)).unwrap()).unwrap();
        let rows = rows_of(&o.out);
        let n = rows[0]["pairs"].parse::<u32>().unwrap();
        let base = l.margin / 2 + l.row_h() + n * l.line_h();
        assert_eq!(w, l.width());
        assert_eq!(h == base, scale == 4, "{scale}: {h} {base}");
        assert!(
            h >= base && (h - base).is_multiple_of(l.line_h()),
            "{scale}: {h} {base}"
        );
        // The legend as the sheet writes it, glyph by glyph: each tick's line, the long one
        // going on, indented, in lines of its own as many characters as fit (the last
        // characters of the line on the last of them).
        let png = read_sheet(&o.out.join(sheet));
        let f = state(&g, &text, &left).unwrap();
        let w = &rows[0];
        let width = l.width() - 2 * l.margin;
        let n = (1..200)
            .take_while(|&k| (k - 1) * 4 * l.text + 3 * l.text <= width)
            .count();
        let mut want: Vec<String> = Vec::new();
        for t in w["ticks"].split(';') {
            let t: Vec<&str> = t.split(',').collect();
            let x = f
                .last
                .iter()
                .find(|x| {
                    let name = |r: u16| atlas::image_name(ImageId::Tile(r));
                    (name(x.pair.a), name(x.pair.b), edge_name(x.pair.edge))
                        == (t[5].to_string(), t[6].to_string(), t[2])
                })
                .unwrap();
            let (r, lim) = x.result.unwrap();
            let anchor = (t[2] == w["edge"]) && (t[0], t[1]) == ("3", "2");
            let lead = if anchor { ">" } else { " " };
            let e = &t[2][..1];
            let line = format!(
                "{lead}{},{} {e} {} {} {r:.2}/{lim:.2}",
                t[0], t[1], t[3], t[4]
            );
            let chars: Vec<char> = line.chars().collect();
            want.push(chars.iter().take(n).collect());
            for rest in chars[n.min(chars.len())..].chunks(n - 2) {
                want.push(format!("  {}", rest.iter().collect::<String>()));
            }
            // At S 2 the long line ends on a line of its own, its measure last.
            if t[3].contains(long) && scale == 2 {
                let last = want.last().unwrap();
                assert!(
                    last.starts_with("  ") && line.ends_with(&last[2..]),
                    "{last}"
                );
                assert!(last.ends_with(&format!("{lim:.2}")), "{last}");
            }
        }
        assert_eq!(want.len() > n_pairs(&rows[0]), scale == 2);
        let ly = l.panel_at(0, 0).1 + l.h + l.margin / 2;
        for (i, line) in want.iter().enumerate() {
            let mut glyphs = Rgba::new(width, l.line_h());
            img::fill(&mut glyphs, 0, 0, width, l.line_h(), BACK);
            super::super::text(&mut glyphs, (0, 0), line, l.text, width);
            let y = ly + i as u32 * l.line_h();
            assert_eq!(
                png.rect(l.margin, y, width, l.line_h()),
                glyphs,
                "{scale}: {line}"
            );
        }
        // Without the renderer: on every line, a continuation line too, ink only in the cells
        // of its characters (a character every 4 × the text scale, 3 wide, 5 high) and some in
        // each cell that is not a space; none in the gaps, the blank row under, or past the
        // line's end.
        let t = l.text;
        for (i, line) in want.iter().enumerate() {
            let y0 = ly + i as u32 * l.line_h();
            let chars: Vec<char> = line.chars().collect();
            let inked = |x: u32, y: u32| img::get(&png, l.margin + x, y0 + y) != BACK;
            for x in 0..width {
                let k = (x / (4 * t)) as usize;
                let cell = x % (4 * t) < 3 * t && k < chars.len() && chars[k] != ' ';
                for y in 0..l.line_h() {
                    if !(cell && y < 5 * t) {
                        assert!(!inked(x, y), "{scale}: {line:?}: ink at {x},{y}");
                    }
                }
            }
            for (k, &ch) in chars.iter().enumerate().filter(|(_, c)| **c != ' ') {
                let x0 = k as u32 * 4 * t;
                let any = (x0..x0 + 3 * t).any(|x| (0..5 * t).any(|y| inked(x, y)));
                assert!(any, "{scale}: {line:?}: no ink for {ch:?} at {k}");
            }
            if line.starts_with("  ") {
                assert!((0..8 * t).all(|x| (0..l.line_h()).all(|y| !inked(x, y))));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_window_that_shows_none_of_its_item_fails_the_run() {
    // A chest at (12, 12), and an object of X's opaque record on it later in the lists, so
    // drawn in front of it: the chest's window, where the world first draws it, shows none of
    // it.
    let mut g = game();
    let chest = synthetic::OBJECTS;
    let at = |tile: u16| crate::game::Placed {
        x: 12,
        y: 12,
        z: 0,
        tile,
    };
    g.world.objects = vec![at(chest), at(X)];
    let images = BTreeMap::from([(chest, near(&g, chest))]);
    let (dir, work) = work_dir("hidden", "", &g, &images);
    let o = options(&dir);
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(e.contains("nothing drawn: hidden 1"), "{e}");
    assert!(!e.contains("t:"), "{e}");
    assert_eq!(
        files_of(&o.out),
        [
            "gates.jsonl",
            "ingest.json",
            "jobs.jsonl",
            "refused.txt",
            "unshown.txt"
        ]
    );
    let plan = group::plan(&g);
    let detail = std::fs::read_to_string(o.out.join("unshown.txt")).unwrap();
    assert_eq!(
        detail,
        format!(
            "hidden: item {}: its coverage window (drawn) shows none of t:{chest}: covered where \
             it is drawn\n",
            item_of(&plan, chest)
        )
    );
    // The chest in front of X instead: its window shows it.
    g.world.objects.reverse();
    let o = FinalOptions {
        out: dir.join("front"),
        ..options(&dir)
    };
    let s = calibrate_final(&g, &work, &o).unwrap();
    let ob = s.classes.iter().find(|c| c.class == Class::Object).unwrap();
    assert_eq!((ob.items, ob.drawn), (1, 1));
    // An alone window fits its pieces from the first: a first piece wider than the window fits
    // none, and that window would show none of its item.
    let piece = |w: usize| Piece {
        w: w as u32,
        h: 1,
        images: (0..w).map(|k| ImageId::Tile(k as u16)).collect(),
    };
    assert_eq!(alone(&[piece(1), piece(2)]).1, 2);
    let (cells, fit) = alone(&[piece(COLS), piece(1)]);
    assert_eq!(fit, 0);
    assert!(cells.iter().all(|c| c.tiles().next().is_none()));
    // ... which the final read refuses rather than draw a window of none of it; a piece that
    // fits after others goes on in the next window.
    let w = Window {
        origin: Origin::Coverage,
        survivor: 0,
        records: vec![0],
        placement: Placement::Alone,
        at: None,
        edge: Edge::East,
        pair: None,
        anchor: None,
        pieces: Vec::new(),
        part: (1, 1),
        rest: false,
    };
    assert_eq!(alone_parts(&w, &[piece(1), piece(COLS)]).unwrap_err(), 1);
    let parts = alone_parts(&w, &vec![piece(1); 10]).unwrap();
    let shape: Vec<(usize, (usize, usize))> =
        parts.iter().map(|p| (p.pieces.len(), p.part)).collect();
    assert_eq!(shape, [(9, (1, 2)), (1, (2, 2))]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `g` with an actor type of `frames` frames from record 400 (its layout not known: its frames
/// run to the next type's base tile), each frame a figure of its own: single pieces.
fn with_actor(mut g: Game, frames: u16) -> (Game, u16) {
    let base = 400u16;
    let sample = g.records[usize::from(synthetic::ACTOR_A)].clone();
    for f in 0..frames {
        let mut r = sample.clone();
        let at = 5 * 16 + 6 + usize::from(f % 5);
        r.pixels[at] = (f % 20) as u8 + 1;
        r.transparent[at] = false;
        g.records[usize::from(base + f)] = r;
    }
    let ty = g
        .basetile
        .iter()
        .position(|&b| b == synthetic::END)
        .unwrap();
    g.basetile[ty] = base;
    g.basetile[ty + 1] = base + frames;
    g.actor_types.push(ty as u16);
    (g, base)
}

#[test]
fn every_record_of_an_item_is_drawn_in_a_window() {
    // An actor of twelve frames, each a piece of its own: more than an alone window holds (the
    // reviewer's probe C: nine).
    let (g, base) = with_actor(game(), 12);
    let plan = group::plan(&g);
    let k = item_of(&plan, base) as usize;
    assert_eq!(plan.items[k].class, Class::Actor);
    assert_eq!(plan.items[k].pieces.len(), 12);
    let (cells, fit) = alone(&plan.items[k].pieces);
    assert_eq!(fit, 9);
    assert_eq!(cells.iter().map(|c| c.list.len()).sum::<usize>(), 9);
    let images: BTreeMap<u16, Rgba> = (base..base + 12).map(|r| (r, near(&g, r))).collect();
    let name = |r: u16| atlas::image_name(ImageId::Tile(r));
    let all: Vec<String> = (base..base + 12).map(name).collect();
    let run = |g: &Game, name: &str| {
        let (text, left) = gate_log(g, &images);
        assert_eq!(left, images);
        let (dir, work) = work_dir(name, &text, g, &images);
        let o = options(&dir);
        let s = calibrate_final(g, &work, &o).unwrap();
        (dir, rows_of(&o.out), s, o.out)
    };
    // The alone windows as the sheet has them, at both scales, against the frames by hand:
    // `frames` in order, nine a window, at columns 1, 3, 5 of rows 1, 3, 5.
    assert_ne!(record_by_hand(&g, base, 4), record_by_hand(&g, base + 1, 4));
    let alone_sheet = |out: &Path, frames: &[u16]| {
        for (s, file) in [
            (4, "final-actor-alone.png"),
            (2, "final-actor-alone-s2.png"),
        ] {
            let sheet = read_sheet(&out.join(file));
            for (row, page) in frames.chunks(9).enumerate() {
                let cells = |c: usize, r: usize| -> Vec<u16> {
                    let (odd_c, odd_r) = (c % 2 == 1 && c < 7, r % 2 == 1);
                    let j = (r / 2) * 3 + c / 2;
                    match page.get(j).filter(|_| odd_c && odd_r) {
                        Some(&f) => vec![f],
                        None => vec![],
                    }
                };
                let want = classic_by_hand(&g, s, cells);
                assert_eq!(panel_in_row(&sheet, s, row, 0), want, "{file} {row}");
                let hd = panel_in_row(&sheet, s, row, 2);
                assert!(pack_by_hand(&g, &hd, s, frames, cells) > 0, "{file} {row}");
            }
        }
    };
    // Drawn nowhere in the starting world: alone, in two windows, every frame in one.
    let (dir, rows, s, out) = run(&g, "actor-alone");
    let frames: Vec<u16> = (base..base + 12).collect();
    alone_sheet(&out, &frames);
    let notes: Vec<&str> = rows.iter().map(|r| r["note"].as_str()).collect();
    assert_eq!(
        notes,
        [
            "drawn nowhere in the starting world: the item alone (window 1 of 2)",
            "drawn nowhere in the starting world: the item alone (window 2 of 2)",
        ]
    );
    assert!(rows.iter().all(|r| r["records"] == all.join("+")));
    let actor = s.classes.iter().find(|c| c.class == Class::Actor).unwrap();
    assert_eq!(
        (actor.items, actor.alone, actor.records, actor.shown),
        (1, 2, 12, 12)
    );
    assert!(s.text().contains("12/12"), "{}", s.text());
    let _ = std::fs::remove_dir_all(&dir);
    // Its first frame placed in the world: its window there shows that frame, and the eleven
    // others are drawn alone, in two windows more.
    let mut g2 = g.clone();
    g2.world.objects = vec![crate::game::Placed {
        x: 12,
        y: 12,
        z: 0,
        tile: base,
    }];
    let (dir, rows, s, out) = run(&g2, "actor-rest");
    alone_sheet(&out, &frames[1..]);
    // ... and the window where it stands draws its first frame in A's cell, on the grass.
    for (sc, file) in [(4, "final-actor.png"), (2, "final-actor-s2.png")] {
        let sheet = read_sheet(&out.join(file));
        let cells = |c: usize, r: usize| {
            let mut v = vec![GRASS];
            if (c, r) == A_CELL {
                v.push(base);
            }
            v
        };
        assert_eq!(
            panel_of(&sheet, sc, 0),
            classic_by_hand(&g2, sc, cells),
            "{file}"
        );
    }
    let places: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r["placement"].as_str(), r["note"].as_str()))
        .collect();
    assert_eq!(
        places,
        [
            ("drawn", "in no pair: where the world first draws it"),
            (
                "alone",
                "the rest of the item alone: 11 records no other window shows (window 1 of 2)"
            ),
            (
                "alone",
                "the rest of the item alone: 11 records no other window shows (window 2 of 2)"
            ),
        ]
    );
    assert_eq!(rows[1]["records"], all[1..].join("+"));
    let actor = s.classes.iter().find(|c| c.class == Class::Actor).unwrap();
    assert_eq!(
        (actor.items, actor.drawn, actor.alone, actor.shown),
        (1, 1, 2, 12)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_window_where_the_world_draws_it_shows_its_record() {
    // The chest animates through two made-up frames (each a chest of its own); the second one
    // survives, in no pair, drawn where the chest stands (12, 12). Its window draws the chest's
    // tile as that frame, not at frame 0.
    let mut g = game();
    let chest = synthetic::OBJECTS;
    let (f0, f1) = (160, 161);
    for (k, f) in [f0, f1].into_iter().enumerate() {
        let mut r = g.records[usize::from(chest)].clone();
        r.pixels[9 * 16 + 5 + k] = 4;
        g.records[usize::from(f)] = r;
    }
    g.anim.push(u6_formats::animdata::AnimEntry {
        tile_to_animate: chest,
        first_anim_frame: f0,
        and_mask: 1,
        shift: 0,
    });
    let at = |x, y, tile| crate::game::Placed { x, y, z: 0, tile };
    g.world.objects = vec![at(12, 12, chest)];
    let images = BTreeMap::from([(f1, near(&g, f1))]);
    let (text, left) = gate_log(&g, &images);
    assert_eq!(left, images);
    let (dir, work) = work_dir("anchor", &text, &g, &images);
    let o = options(&dir);
    calibrate_final(&g, &work, &o).unwrap();
    let rows = rows_of(&o.out);
    let w = &rows[0];
    assert_eq!(
        (w["placement"].as_str(), w["x"].as_str(), w["y"].as_str()),
        ("drawn", "12", "12")
    );
    assert_eq!(w["frames"], "pair");
    // The sheet draws the chest's cell as the second frame, on the grass, at both scales.
    for (s, file) in [(4, w["sheet"].clone()), (2, w["sheet_s2"].clone())] {
        let sheet = read_sheet(&o.out.join(&file));
        let world = |f: u16| {
            move |c: usize, r: usize| {
                let mut v = vec![GRASS];
                if (c, r) == A_CELL {
                    v.push(f);
                }
                v
            }
        };
        let (want, frame0) = (
            classic_by_hand(&g, s, world(f1)),
            classic_by_hand(&g, s, world(f0)),
        );
        assert_ne!(want, frame0);
        assert_eq!(panel_of(&sheet, s, 0), want, "{file}");
        assert!(pack_by_hand(&g, &panel_of(&sheet, s, 2), s, &[f1], world(f1)) > 0);
    }
    // A map tile whose cell's base an object replaces is not drawn there: where the world first
    // draws the grass is past the first cell when a replacing object stands on it.
    let wanted = BTreeSet::from([GRASS]);
    assert_eq!(first_drawn(&g, &wanted)[&GRASS], At { z: 0, x: 0, y: 0 });
    g.flags3[usize::from(X)] = REPLACES_BASE;
    g.world.objects.push(at(0, 0, X));
    assert_eq!(first_drawn(&g, &wanted)[&GRASS], At { z: 0, x: 1, y: 0 });
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pair_with_nothing_measured_is_read_where_its_sides_show() {
    // The chain, and B again at (4, 5) beside a record T east of it whose west half is clear:
    // the gate measures nothing across that join, and the pair is still one of the last pass.
    const T: u16 = 6;
    let mut g = game();
    let rec = &mut g.records[usize::from(T)];
    for i in 0..256 {
        let east = i % 16 >= 8;
        rec.pixels[i] = if east {
            GRAIN + (hash(i as u32, 7) % 5) as u8
        } else {
            0
        };
        rec.transparent[i] = !east;
    }
    g.world.chunks[0][5][4] = B as u8;
    g.world.chunks[0][5][5] = T as u8;
    let (text, left) = chain(&g);
    let bt = Pair {
        a: B,
        b: T,
        edge: Edge::East,
    };
    let (dir, work) = work_dir("unmeasured", &text, &g, &left);
    let o = options(&dir);
    let s = calibrate_final(&g, &work, &o).unwrap();
    let f = state(&g, &text, &left).unwrap();
    let x = f.last.iter().find(|l| l.pair == bt).unwrap();
    assert_eq!((x.result, x.first.1), (None, None));
    // Drawn in a window: B shows along its east side, T, with nothing on its west line, in its
    // cell.
    assert_eq!((s.last, s.unread), (6, 0));
    let name = |r: u16| atlas::image_name(ImageId::Tile(r));
    let rows = rows_of(&o.out);
    assert!(
        rows.iter()
            .any(|r| r["ticks"].contains(&format!("{},{}", name(B), name(T)))),
        "{rows:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_change_to_the_work_dir_while_it_is_read_fails_the_run() {
    let g = game();
    let (text, left) = chain(&g);
    let (dir, work) = work_dir("changed", &text, &g, &left);
    let master = work.slice(&atlas::slice_name(ImageId::Tile(B)));
    let other = work.slice(&atlas::slice_name(ImageId::Tile(A)));
    let (gates_path, list_path) = (work.path("gates.jsonl"), work.path("ingest.json"));
    let jobs_path = work.path("jobs.jsonl");
    let originals: Vec<(PathBuf, Vec<u8>)> = [&master, &gates_path, &list_path, &jobs_path]
        .into_iter()
        .map(|p| (p.clone(), std::fs::read(p).unwrap()))
        .collect();
    let mut blank = Rgba::new(128, 128);
    img::fill(&mut blank, 0, 0, 128, 128, [9, 9, 9, 255]);
    let png = img::encode_png(&blank).unwrap();
    // A re-ingest, once the run has read the state: a master rewritten, one added, gates.jsonl
    // or ingest.json rewritten. Each fails the run, the names and a count in the message.
    let changes: Vec<(&str, Touch)> = vec![
        ("1 masters of slices/", {
            let (p, b) = (master.clone(), png.clone());
            Box::new(move || std::fs::write(&p, &b).unwrap())
        }),
        ("1 masters of slices/", {
            let (p, b) = (other.clone(), png.clone());
            Box::new(move || std::fs::write(&p, &b).unwrap())
        }),
        ("(gates.jsonl)", {
            let p = gates_path.clone();
            Box::new(move || std::fs::write(&p, b"").unwrap())
        }),
        ("(ingest.json)", {
            let p = list_path.clone();
            Box::new(move || std::fs::write(&p, b"[]").unwrap())
        }),
        ("(jobs.jsonl)", {
            let p = jobs_path.clone();
            Box::new(move || std::fs::write(&p, b"").unwrap())
        }),
    ];
    for (k, (said, change)) in changes.into_iter().enumerate() {
        let o = FinalOptions {
            out: dir.join(format!("out-{k}")),
            ..options(&dir)
        };
        AFTER_READ.lock().unwrap().push((o.out.clone(), change));
        let e = calibrate_final(&g, &work, &o).unwrap_err();
        assert!(
            e.contains("changed while calibrate --final read it"),
            "{k}: {e}"
        );
        assert!(e.contains(said), "{k}: {e}");
        assert!(!e.contains("t:"), "{k}: {e}");
        AFTER_READ.lock().unwrap().retain(|(key, _)| *key != o.out);
        for (p, b) in &originals {
            std::fs::write(p, b).unwrap();
        }
        let _ = std::fs::remove_file(&other);
    }
    // Nothing changed: the run reports success, and what it read is the work dir's.
    let o = options(&dir);
    assert!(calibrate_final(&g, &work, &o).is_ok());
    let slices = Slices::read(&g, &work.path("slices")).unwrap();
    let digests: Vec<(ImageId, String)> = slices
        .digests()
        .iter()
        .map(|(&id, d)| (id, d.clone()))
        .collect();
    assert_eq!(
        digests,
        [(ImageId::Tile(B), work::digest(&[&originals[0].1]))]
    );
    assert!(slices.changed(&work.path("slices")).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_portrait_and_bitmap_masters_are_held_like_the_tiles() {
    // The chain, and a portrait and a bitmap generated: each listed with its master's digest,
    // its master in slices/ (not decoded: nothing is drawn from it).
    let g = game();
    let (text, left) = chain(&g);
    let others = [
        ImageId::Portrait {
            letter: b'a',
            item: 0,
        },
        ImageId::Bitmap(u6_hdpack::Role::Paper),
    ];
    let png = |id: ImageId| {
        let c = g.classic(id).unwrap();
        img::encode_png(&Rgba::new(c.width * 8, c.height * 8)).unwrap()
    };
    let mut list = listed(&g, &left);
    let mut files = digests(&left);
    for id in others {
        let c = g.classic(id).unwrap();
        list.push(Accepted {
            image: atlas::image_name(id),
            hash: format!("{:016x}", g.hash(id).unwrap()),
            width: c.width * 8,
            height: c.height * 8,
            slice: format!("slices/{}.png", atlas::slice_name(id)),
            digest: Some(work::digest(&[&png(id)])),
        });
        files.insert(id, work::digest(&[&png(id)]));
    }
    assert!(state_of(&g, &text, &left, &list, &files).is_ok());
    // For the portrait and for the bitmap: its master not the file ingest wrote; listed, and
    // no master; a master not listed.
    for p in others {
        let mut swapped = files.clone();
        swapped.insert(p, "0".repeat(16));
        let err = state_of(&g, &text, &left, &list, &swapped);
        assert_eq!(err.unwrap_err(), ["master"], "{p:?}");
        let mut gone = files.clone();
        gone.remove(&p);
        let err = state_of(&g, &text, &left, &list, &gone);
        assert_eq!(err.unwrap_err(), ["slice"], "{p:?}");
        let unlisted: Vec<Accepted> = list
            .iter()
            .filter(|a| a.image != atlas::image_name(p))
            .cloned()
            .collect();
        let err = state_of(&g, &text, &left, &unlisted, &files);
        assert_eq!(err.unwrap_err(), ["slice"], "{p:?}");
    }
    // The run reads them from slices/, each once, and reads them again at the end: either one
    // rewritten while the run reads fails it, nothing drawn kept.
    let (dir, work) = work_dir("others", &text, &g, &left);
    work.write("ingest.json", &serde_json::to_vec(&list).unwrap())
        .unwrap();
    for id in others {
        std::fs::write(work.slice(&atlas::slice_name(id)), png(id)).unwrap();
    }
    let s = calibrate_final(&g, &work, &options(&dir)).unwrap();
    assert_eq!((s.listed, s.masters), (3, 3));
    for (k, p) in others.into_iter().enumerate() {
        let o = FinalOptions {
            out: dir.join(format!("changed-{k}")),
            ..options(&dir)
        };
        let path = work.slice(&atlas::slice_name(p));
        let (at, was) = (path.clone(), png(p));
        AFTER_READ.lock().unwrap().push((
            o.out.clone(),
            Box::new(move || std::fs::write(&at, b"another").unwrap()),
        ));
        let e = calibrate_final(&g, &work, &o).unwrap_err();
        AFTER_READ.lock().unwrap().retain(|(k, _)| *k != o.out);
        assert!(e.contains("(1 masters of slices/)"), "{p:?}: {e}");
        assert!(!o.out.join("samples.tsv").exists(), "{p:?}");
        std::fs::write(&path, was).unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_out_dir_holds_one_runs_output() {
    // The chest at (12, 12) and the sword alone, then a re-ingest that leaves the chest only.
    let mut g = game();
    let (chest, sword) = (synthetic::OBJECTS, synthetic::OBJECTS + 2);
    g.world.objects = vec![crate::game::Placed {
        x: 12,
        y: 12,
        z: 0,
        tile: chest,
    }];
    let images: BTreeMap<u16, Rgba> = [chest, sword].iter().map(|&r| (r, near(&g, r))).collect();
    let (dir, work) = work_dir("one-run", "", &g, &images);
    let o = options(&dir);
    let pngs = |out: &Path| -> Vec<String> {
        files_of(out)
            .into_iter()
            .filter(|f| f.ends_with(".png"))
            .collect()
    };
    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    let s1 = calibrate_final(&g, &work, &o).unwrap();
    assert_eq!(pngs(&o.out), sorted(s1.sheets.clone()));
    let one = BTreeMap::from([(chest, images[&chest].clone())]);
    work.write(
        "ingest.json",
        &serde_json::to_vec(&listed(&g, &one)).unwrap(),
    )
    .unwrap();
    std::fs::remove_file(work.slice(&atlas::slice_name(ImageId::Tile(sword)))).unwrap();
    // A second run: its sheets alone, not the first run's alone sheet (the reviewer's P2).
    let s2 = calibrate_final(&g, &work, &o).unwrap();
    assert_eq!(pngs(&o.out), sorted(s2.sheets.clone()));
    assert!(s1.sheets.len() > s2.sheets.len());
    // A refused run: its refusal alone, no sheet, no samples.tsv (P3).
    let mut solid = Rgba::new(64, 64);
    img::fill(&mut solid, 0, 0, 64, 64, [255, 0, 255, 255]);
    let master = work.slice(&atlas::slice_name(ImageId::Tile(chest)));
    let kept = std::fs::read(&master).unwrap();
    std::fs::write(&master, master_png(&solid)).unwrap();
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(e.contains("master"), "{e}");
    assert_eq!(
        files_of(&o.out),
        [
            "disagreements.txt",
            "gates.jsonl",
            "ingest.json",
            "jobs.jsonl",
            "refused.txt"
        ]
    );
    let refused = std::fs::read_to_string(o.out.join("refused.txt")).unwrap();
    assert_eq!(refused, format!("{e}\n"));
    std::fs::write(&master, &kept).unwrap();
    // A run whose work dir changes while it is read: what changed, no sheet drawn kept (P4).
    let p = work.path("gates.jsonl");
    AFTER_READ.lock().unwrap().push((
        o.out.clone(),
        Box::new(move || std::fs::write(&p, b"\n").unwrap()),
    ));
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    AFTER_READ.lock().unwrap().retain(|(k, _)| *k != o.out);
    assert!(e.contains("nothing drawn is kept"), "{e}");
    assert_eq!(
        files_of(&o.out),
        [
            "changed.txt",
            "gates.jsonl",
            "ingest.json",
            "jobs.jsonl",
            "refused.txt"
        ]
    );
    assert_eq!(
        std::fs::read_to_string(o.out.join("changed.txt")).unwrap(),
        "gates.jsonl\n"
    );
    work.write("gates.jsonl", b"").unwrap();
    // A dir the tool did not write is not replaced, an empty one neither, and nothing is left
    // beside it.
    let theirs = dir.join("theirs");
    std::fs::create_dir_all(&theirs).unwrap();
    std::fs::write(theirs.join("notes.txt"), b"the owner's").unwrap();
    let o2 = FinalOptions {
        out: theirs.clone(),
        ..options(&dir)
    };
    let e = calibrate_final(&g, &work, &o2).unwrap_err();
    assert!(
        e.contains(
            "calibrate --final did not write (no marker .calibrate-final), so it is not replaced: move it away, or pass another --out"
        ),
        "{e}"
    );
    let left: Vec<String> = std::fs::read_dir(&theirs)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["notes.txt"]);
    let beside: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|f| f.starts_with('.'))
        .collect();
    assert!(beside.is_empty(), "{beside:?}");
    std::fs::remove_file(theirs.join("notes.txt")).unwrap();
    let e = calibrate_final(&g, &work, &o2).unwrap_err();
    assert!(e.contains("(no marker .calibrate-final)"), "{e}");
    assert_eq!(std::fs::read_dir(&theirs).unwrap().count(), 0);
    // The first pass's out dir (its marker, not this mode's) is not replaced either.
    let first = super::super::out_dir::FIRST.marker;
    std::fs::write(theirs.join(first), b"calibrate\n").unwrap();
    let e = calibrate_final(&g, &work, &o2).unwrap_err();
    assert!(
        e.contains("(no marker .calibrate-final; it is calibrate's output)"),
        "{e}"
    );
    let left: Vec<String> = std::fs::read_dir(&theirs)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, [first]);
    std::fs::remove_dir_all(&theirs).unwrap();
    assert!(calibrate_final(&g, &work, &o2).is_ok());
    assert!(theirs.join(MARKER).is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A run of the chain that fails as `fail` makes it (the hooks it sets, kept while it runs),
/// at the out dir `name`: its error, and the files of the out dir after it.
fn failed_run(name: &str, fail: impl FnOnce(&Path) -> Vec<Hooked>) -> (String, Vec<String>) {
    let g = game();
    let (text, left) = chain(&g);
    let (dir, work) = work_dir(&format!("failed-{name}"), &text, &g, &left);
    let o = options(&dir);
    let hooks = fail(&o.out);
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    drop(hooks);
    AFTER_READ.lock().unwrap().retain(|(k, _)| *k != o.out);
    let files = files_of(&o.out);
    let _ = std::fs::remove_dir_all(&dir);
    (e, files)
}

/// What a failed run of the chain leaves at the out dir: its refusal and the snapshots read.
const REFUSAL: [&str; 4] = ["gates.jsonl", "ingest.json", "jobs.jsonl", "refused.txt"];

/// A hook that fails the second sheet written (the first is written).
fn second_sheet_fails(out: &Path) -> Hooked {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let n = AtomicUsize::new(0);
    hook(out, "write", move |name| {
        match n.fetch_add(1, Ordering::SeqCst) {
            1 => Err(format!("{name}: no room left")),
            _ => Ok(()),
        }
    })
}

#[test]
fn a_run_whose_masters_cannot_be_read_again_publishes_its_refusal_alone() {
    // slices/ gone once the masters are read: they cannot be read again at the end, after the
    // sheets are drawn. The out dir: the refusal, no sheet, no samples.tsv.
    let (e, files) = failed_run("recheck", |out| {
        let slices = out.parent().unwrap().join("work").join("slices");
        let moved = out.parent().unwrap().join("slices-moved");
        AFTER_READ.lock().unwrap().push((
            out.to_path_buf(),
            Box::new(move || std::fs::rename(&slices, &moved).unwrap()),
        ));
        Vec::new()
    });
    assert!(e.contains("slices"), "{e}");
    assert_eq!(files, REFUSAL);
}

#[test]
fn a_run_whose_master_cannot_be_read_again_publishes_its_refusal_alone() {
    // The reviewer's P7b: B's master unreadable once it is read, so the re-check at the end
    // cannot read it. The out dir: the refusal, no sheet, no samples.tsv.
    use std::os::unix::fs::PermissionsExt;
    let master: std::sync::Arc<std::sync::Mutex<Option<PathBuf>>> = Default::default();
    let at = master.clone();
    let (e, files) = failed_run("unreadable", |out| {
        let m = out
            .parent()
            .unwrap()
            .join("work")
            .join("slices")
            .join(format!("{}.png", atlas::slice_name(ImageId::Tile(B))));
        *at.lock().unwrap() = Some(m.clone());
        AFTER_READ.lock().unwrap().push((
            out.to_path_buf(),
            Box::new(move || {
                std::fs::set_permissions(&m, std::fs::Permissions::from_mode(0o000)).unwrap()
            }),
        ));
        Vec::new()
    });
    let m = master.lock().unwrap().take().unwrap();
    let _ = std::fs::set_permissions(&m, std::fs::Permissions::from_mode(0o644));
    assert!(e.contains("Permission denied"), "{e}");
    assert_eq!(files, REFUSAL);
}

#[test]
fn a_run_whose_second_sheet_cannot_be_written_publishes_its_refusal_alone() {
    let (e, files) = failed_run("write", |out| vec![second_sheet_fails(out)]);
    assert!(e.contains("final-terrain-s2.png: no room left"), "{e}");
    assert_eq!(files, REFUSAL);
}

#[test]
fn a_failed_run_whose_dir_cannot_be_removed_whole_publishes_its_refusal_alone() {
    // The second sheet not written, then what the run wrote not removable whole: a dir in it
    // whose file cannot leave, so the removal stops part of the way. The out dir is still the
    // refusal alone; what is left is said.
    use std::os::unix::fs::PermissionsExt;
    let stuck: std::sync::Arc<std::sync::Mutex<Option<PathBuf>>> = Default::default();
    let at = stuck.clone();
    let (e, files) = failed_run("cleanup", |out| {
        let h = hook(out, "cleanup", move |stage| {
            let sub = Path::new(stage).join("stuck");
            std::fs::create_dir(&sub).unwrap();
            std::fs::write(sub.join("x"), b"x").unwrap();
            std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o500)).unwrap();
            *at.lock().unwrap() = Some(sub);
            Ok(())
        });
        vec![second_sheet_fails(out), h]
    });
    let sub = stuck.lock().unwrap().take().unwrap();
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o700)).unwrap();
    let _ = std::fs::remove_dir_all(sub.parent().unwrap().parent().unwrap());
    assert!(e.contains("no room left"), "{e}");
    assert!(
        e.contains("what the run wrote is left beside the out dir"),
        "{e}"
    );
    assert_eq!(files, REFUSAL);
}

#[test]
fn an_error_of_the_run_names_the_out_dir() {
    // A sheet that cannot be written (a dir where it goes) and a snapshot that cannot be read
    // back (gone once written): each error names the file of the out dir, as refused.txt
    // does, and the dir the run writes first only once, at the end.
    let g = game();
    let (text, left) = chain(&g);
    let (dir, work) = work_dir("named", &text, &g, &left);
    let real = dir.canonicalize().unwrap();
    let staging = |out: &Path| -> PathBuf {
        let prefix = format!(".{}.staging-", out.file_name().unwrap().to_string_lossy());
        std::fs::read_dir(out.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&prefix)
            })
            .unwrap()
    };
    for (k, step, file, said) in [
        (
            "write",
            "write",
            "final-terrain.png",
            "exists and is not a plain file",
        ),
        (
            "snapshot",
            "snapshot",
            "gates.jsonl",
            "No such file or directory",
        ),
    ] {
        let o = FinalOptions {
            out: dir.join(k),
            ..options(&dir)
        };
        let at = o.out.clone();
        let _h = hook(&o.out, step, move |name| {
            if name == file {
                let p = staging(&at).join(name);
                if step == "write" {
                    std::fs::create_dir(&p).unwrap();
                } else {
                    std::fs::remove_file(&p).unwrap();
                }
            }
            Ok(())
        });
        let e = calibrate_final(&g, &work, &o).unwrap_err();
        let want = format!("{}: {said}", real.join(k).join(file).display());
        assert!(e.starts_with(&want), "{k}: {e}");
        assert!(e.ends_with(", which is not kept)"), "{k}: {e}");
        assert_eq!(e.matches(".staging-").count(), 1, "{k}: {e}");
        let refused = std::fs::read_to_string(o.out.join("refused.txt")).unwrap();
        assert_eq!(refused, format!("{e}\n"), "{k}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_include_list_put_in_the_out_dir_is_not_removed_with_it() {
    // The reviewer's P15: the owner's --include list saved in a marked --out. The next run is
    // refused, the list named, and the out dir left as it is, the list in it.
    let g = game();
    let (text, left) = chain(&g);
    let (dir, work) = work_dir("include-inside", &text, &g, &left);
    let o = options(&dir);
    calibrate_final(&g, &work, &o).unwrap();
    let had = files_of(&o.out);
    let mine = o.out.join("owner-ids.txt");
    let ids = format!("{}\n", atlas::image_name(ImageId::Tile(B)));
    std::fs::write(&mine, &ids).unwrap();
    let o2 = FinalOptions {
        include: Some(mine.clone()),
        ..options(&dir)
    };
    let e = calibrate_final(&g, &work, &o2).unwrap_err();
    assert!(
        e.contains("it holds what calibrate --final did not write: owner-ids.txt"),
        "{e}"
    );
    assert_eq!(std::fs::read_to_string(&mine).unwrap(), ids);
    let mut now = had.clone();
    now.push("owner-ids.txt".into());
    now.sort();
    assert_eq!(files_of(&o.out), now);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_run_after_a_crashed_swap_puts_the_old_output_back_and_removes_the_rest() {
    // The reviewer's P8, by the names this protocol gives: a run killed between moving the old
    // output aside and putting its own in place (no --out; the old output at .final.old-…, the
    // new one in .final.staging-…, of a process gone). The next run puts the old one back,
    // removes the staging dir, says both, then replaces --out with its own output alone.
    let g = game();
    let (text, left) = chain(&g);
    let (dir, work) = work_dir("crashed-swap", &text, &g, &left);
    let o = options(&dir);
    calibrate_final(&g, &work, &o).unwrap();
    let old = dir.join(".final.old-99999999-1");
    let staged = dir.join(".final.staging-99999999-2");
    std::fs::rename(&o.out, &old).unwrap();
    std::fs::create_dir(&staged).unwrap();
    std::fs::write(staged.join(MARKER), b"calibrate --final\n").unwrap();
    std::fs::write(staged.join("final-stale.png"), b"x").unwrap();
    let s = calibrate_final(&g, &work, &o).unwrap();
    let mut want = s.sheets.clone();
    want.sort();
    let pngs: Vec<String> = files_of(&o.out)
        .into_iter()
        .filter(|f| f.ends_with(".png"))
        .collect();
    assert_eq!(pngs, want);
    let beside: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|f| f.starts_with('.'))
        .collect();
    assert!(beside.is_empty(), "{beside:?}");
    let report = s.text();
    let line = |what: &str, p: &Path| {
        format!(
            "out dir: {what} {}",
            p.canonicalize().unwrap_or(p.to_path_buf()).display()
        )
    };
    let real = dir.canonicalize().unwrap();
    assert!(
        report.contains(&line("put back from", &real.join(".final.old-99999999-1"))),
        "{report}"
    );
    assert!(
        report.contains(&line("removed", &real.join(".final.staging-99999999-2"))),
        "{report}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_pair_of_the_last_pass_is_drawn_in_a_window() {
    // The chain, and B again far from it, beside a record Y (a grain of its own) east of it:
    // B's window, wherever it is placed, cannot draw both its pairs by the chain and its pair
    // with Y.
    const Y: u16 = 6;
    let mut g = game();
    let rec = &mut g.records[usize::from(Y)];
    for i in 0..256 {
        rec.pixels[i] = GRAIN + (hash(i as u32, 9) % 5) as u8;
        rec.transparent[i] = false;
    }
    g.world.chunks.push(chunk(&[(4, 4, B), (5, 4, Y)]));
    g.world.levels[0].refs = vec![0, 1, 1, 2];
    let (text, left) = chain(&g);
    let by = Pair {
        a: B,
        b: Y,
        edge: Edge::East,
    };
    let (dir, work) = work_dir("every", &text, &g, &left);
    let o = options(&dir);
    let s = calibrate_final(&g, &work, &o).unwrap();
    // B's pairs: by the chain the four of the chain, and here the grass west of B and B beside
    // Y.
    assert_eq!(s.last, 6);
    // Each of the six is drawn in a window: B's own, and one pair window for what it misses.
    for k in &s.kinds {
        assert_eq!(k.read, k.last, "{} {}", k.label, k.sides.name());
    }
    assert_eq!((s.pair_windows, s.unread), (1, 0));
    let map = s
        .kinds
        .iter()
        .find(|k| k.label == "map" && k.sides == Sides::GenMmpx);
    assert_eq!(map.map(|k| (k.last, k.windows)), Some((6, 2)));
    let rows = rows_of(&o.out);
    let pairs: Vec<&BTreeMap<String, String>> =
        rows.iter().filter(|r| r["origin"] == "pair").collect();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0]["placement"], "pair");
    assert_eq!(pairs[0]["records"], atlas::image_name(ImageId::Tile(B)));
    assert_eq!(pairs[0]["sheet"], "final-pairs.png");
    assert_eq!(
        pairs[0]["note"],
        "a pair of the last pass no other window draws"
    );
    // Every pair of the last pass is in some window's ticks.
    let ticked: BTreeSet<(String, String)> = rows
        .iter()
        .flat_map(|r| {
            r["ticks"]
                .split(';')
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|t| t != "-")
        .map(|t| {
            let f: Vec<&str> = t.split(',').collect();
            (f[5].to_string(), f[6].to_string())
        })
        .collect();
    let name = |r: u16| atlas::image_name(ImageId::Tile(r));
    for p in [AB, BG, by] {
        assert!(ticked.contains(&(name(p.a), name(p.b))), "{p:?}");
    }
    assert!(s.sheets.contains(&"final-pairs.png".to_string()));
    assert!(s.sheets.contains(&"final-pairs-s2.png".to_string()));
    assert!(
        s.text()
            .contains("pairs of the last pass drawn in no window: 0")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_read_of_what_ingest_and_its_gate_wrote() {
    // The synthetic set, end to end: the table (two records, one item), the sword east of its
    // anchor and the chest east of the sword, all three made opaque grains, on a grass chunk.
    // Every sheet comes back as sent but the table's anchor with its east column white: the
    // gate fails it beside the sword in pass 1 (gen-gen), sends the table and the sword back,
    // and the sword beside the chest, gen-gen in pass 1, is gen-mmpx in pass 2.
    use crate::game::Placed;
    use crate::ingest::{self, Status};
    let mut g = synthetic::game();
    let grain = |seed: u32| crate::game::Record {
        pixels: (0..256).map(|i| 13 + (hash(i, seed) % 2) as u8).collect(),
        transparent: vec![false; 256],
        map: false,
    };
    let (table, sword, chest) = (synthetic::TABLE, synthetic::OBJECTS + 2, synthetic::OBJECTS);
    for (k, t) in [table, table + 1, sword, chest].into_iter().enumerate() {
        g.records[usize::from(t)] = grain(k as u32 + 1);
    }
    let at = |x, tile| Placed {
        x,
        y: 3,
        z: 0,
        tile,
    };
    g.world = World {
        chunks: vec![[[0u8; 8]; 8]],
        levels: vec![Level {
            side: 1,
            refs: vec![0],
        }],
        objects: vec![at(3, table + 1), at(4, sword), at(5, chest)],
    };
    let dir = std::env::temp_dir().join(format!("u6-hdtool-final-{}-e2e", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let work = Work::open(Some(&dir.join("work"))).unwrap();
    work.mkdirs().unwrap();
    let plan = group::plan(&g);
    let items: Vec<(u32, &group::Item)> = plan
        .items
        .iter()
        .enumerate()
        .map(|(i, it)| (i as u32, it))
        .collect();
    let stamp = atlas::Stamp {
        attempt: 1,
        generation: "test".into(),
        plan: plan.fingerprint(),
    };
    let mut jobs = Vec::new();
    atlas::layout_each(&g, &items, &stamp, 0, |job, sheet| {
        img::write_png(&work.sheet(&job.sheet), &sheet)?;
        jobs.push(job);
        Ok(())
    })
    .unwrap();
    work.append_jobs(&jobs).unwrap();
    let anchor = atlas::image_name(ImageId::Tile(table + 1));
    for job in &jobs {
        let mut back = img::read_png(&work.sheet(&job.sheet), job.width, job.height).unwrap();
        for p in job.places.iter().filter(|p| p.image == anchor) {
            let k = p.scale;
            img::fill(
                &mut back,
                p.x + 15 * k,
                p.y,
                k,
                16 * k,
                [250, 250, 250, 255],
            );
        }
        crate::run::record(&work, job, &img::encode_png(&back).unwrap()).unwrap();
    }
    let mut t = Thresholds::default();
    t.set("structure=-1").unwrap();
    let r = ingest::ingest(&g, &work, &t).unwrap();
    assert_eq!((r.neighbour.passes, r.neighbour.failing_after), (2, 0));
    assert_eq!(r.get(Class::Multi, Status::Neighbour), 2);
    assert_eq!(r.get(Class::Object, Status::Neighbour), 1);

    // The final read of that work dir: it agrees, and reads every pair of the last pass.
    let o = FinalOptions {
        out: dir.join("final"),
        seam: t.seam,
        source: Source::All,
        include: None,
    };
    let s = calibrate_final(&g, &work, &o).unwrap();
    let log = std::fs::read_to_string(work.path("gates.jsonl")).unwrap();
    let rec = record(&log).unwrap();
    let first = rec.pairs.iter().filter(|l| l.pass == 1).count();
    assert_eq!((s.passes, s.logged_last, s.first), (2, false, first));
    assert_eq!(s.unread, 0);
    // The last pass: the grass's two pairs and the sword beside the chest, now gen-mmpx; the
    // table's pairs and the failing one have no side generated any more.
    let grass = |edge| Pair { a: 0, b: 0, edge };
    let sc = Pair {
        a: sword,
        b: chest,
        edge: Edge::East,
    };
    assert_eq!(s.last, 3);
    let objects = s
        .kinds
        .iter()
        .find(|k| k.label == "objects" && k.sides == Sides::GenMmpx)
        .unwrap();
    assert_eq!((objects.last, objects.read), (1, 1));
    let map = s
        .kinds
        .iter()
        .find(|k| k.label == "map" && k.sides == Sides::GenGen)
        .unwrap();
    assert_eq!((map.last, map.read), (2, 2));
    // The chest's window: at its one pair, ticked gen-mmpx with the measure of the slices,
    // which is not pass 1's gen-gen line.
    let rows = rows_of(&o.out);
    let name = |r: u16| atlas::image_name(ImageId::Tile(r));
    let w = rows
        .iter()
        .find(|w| w["origin"] == "coverage" && w["records"] == name(chest))
        .unwrap();
    assert_eq!(
        (w["placement"].as_str(), w["x"].as_str(), w["y"].as_str()),
        ("pair", "4", "3")
    );
    let tick = format!("east,objects,gen-mmpx,{},{}", name(sword), name(chest));
    assert!(w["ticks"].contains(&tick), "{}", w["ticks"]);
    let logged = rec
        .pairs
        .iter()
        .find(|l| l.pair == sc && l.pass == 1)
        .unwrap();
    assert_eq!(logged.sides, Sides::GenGen);
    let measure_of = |w: &BTreeMap<String, String>| -> String {
        let t = w["ticks"].split(';').find(|t| t.contains(&tick)).unwrap();
        t.split(',').skip(7).collect::<Vec<_>>().join(",")
    };
    // The rebuilt measure is the gate's for the last pass: the chest's master as ingest wrote
    // it beside the sword drawn by MMPX, one pair through the gate's sides and fixed point.
    let master = img::read_png(
        &work.slice(&atlas::slice_name(ImageId::Tile(chest))),
        128,
        128,
    )
    .unwrap();
    let plan = group::plan(&g);
    let accepted = BTreeMap::from([(chest, (item_of(&plan, chest), &master))]);
    let sides = neighbour::sides(&g, &[sc], &accepted).unwrap();
    let last = neighbour::settle(&[sc], &sides, t.seam);
    let (ratio, limit) = last.checked[0].result.unwrap();
    assert_eq!(measure_of(w), format!("{ratio:.4},{limit:.4}"));
    // ... which is not pass 1's (the fixture changes the pair's sides).
    let (ratio, limit) = logged.result.unwrap();
    assert_ne!(measure_of(w), format!("{ratio:.4},{limit:.4}"));
    // The grass's pairs read where the grass's window is placed.
    let gw = rows
        .iter()
        .find(|w| w["origin"] == "coverage" && w["records"] == name(0))
        .unwrap();
    for edge in [Edge::East, Edge::South] {
        let p = grass(edge);
        let tick = format!("{},map,gen-gen,{},{}", edge_name(p.edge), name(0), name(0));
        assert!(gw["ticks"].contains(&tick), "{}", gw["ticks"]);
    }
    // Ingest listed each master with the digest of the file it wrote.
    let list: Vec<Accepted> =
        serde_json::from_slice(&std::fs::read(work.path("ingest.json")).unwrap()).unwrap();
    assert!(!list.is_empty());
    for a in &list {
        let bytes = std::fs::read(work.path(&a.slice)).unwrap();
        assert_eq!(a.digest, Some(work::digest(&[&bytes])), "{}", a.image);
    }
    // Every survivor's record drawn in a window; the fallen have none.
    for c in &s.classes {
        assert_eq!(c.shown, c.records, "{:?}", c.class);
    }
    assert_eq!(
        s.classes
            .iter()
            .find(|c| c.class == Class::Object)
            .unwrap()
            .items,
        r.get(Class::Object, Status::Generated)
    );
    assert!(rows.iter().all(|w| w["records"] != name(sword)));
    // The water's record, which the animation always replaces and the world never draws as
    // itself: alone, drawn as itself (said so, not as a pair's frame).
    let water = rows.iter().find(|w| w["records"] == name(1)).unwrap();
    assert_eq!(
        (water["placement"].as_str(), water["frames"].as_str()),
        ("alone", "itself")
    );
    // jobs.jsonl emptied after that ingest: the log's items are bound to no plan any more.
    work.write("jobs.jsonl", b"").unwrap();
    let o = FinalOptions {
        out: dir.join("no-jobs"),
        ..o
    };
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(e.contains("disagree, nothing drawn: plan "), "{e}");
    assert_eq!(
        files_of(&o.out),
        [
            "disagreements.txt",
            "gates.jsonl",
            "ingest.json",
            "jobs.jsonl",
            "refused.txt"
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

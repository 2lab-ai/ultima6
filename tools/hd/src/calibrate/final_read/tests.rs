//! `calibrate --final` on synthetic records and worlds, and on `gates.jsonl` lines,
//! `ingest.json` lists and masters made by the tests from the synthetic game's records.

use std::collections::BTreeMap;
use std::path::Path;

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

/// The ×8 master of the ×4 image `img`: each pixel a 2 × 2 block whose subpixels differ (one
/// channel up and down by as much as it can, to 8) and whose box average is the pixel, so a
/// master reduced by any other rule than `pack`'s is not `img`.
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

/// The file of [`master`] of `img`, as ingest writes it.
pub(super) fn master_png(img: &Rgba) -> Vec<u8> {
    img::encode_png(&master(img)).unwrap()
}

/// The masters of the ×4 images `images` as read: each record with its file's digest.
pub(super) fn digests(images: &BTreeMap<u16, Rgba>) -> BTreeMap<u16, String> {
    images
        .iter()
        .map(|(&r, i)| (r, work::digest(&[&master_png(i)])))
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
    files: &BTreeMap<u16, String>,
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
fn a_master_is_the_one_ingest_wrote() {
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
        ["disagreements.txt", "gates.jsonl", "ingest.json"]
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
    assert_eq!(files_of(&o.out), ["gates.jsonl", "ingest.json"]);
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

/// The files of the out dir `out`, sorted.
fn files_of(out: &Path) -> Vec<String> {
    let mut files: Vec<String> = std::fs::read_dir(out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
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
    // B beside the grass (line 3) with a neighbour that is not a pair: refused by its line, in
    // the final read and in the first pass's rows alike, not read as a line of another kind.
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
    // A line with no neighbour field is another kind of line, and not read.
    let other = text.clone() + r#"{"image":"t:1","pass":1,"fail":false}"# + "\n";
    assert_eq!(record(&other).unwrap(), record(&text).unwrap());
    // The run stops before it draws: no sheet, no samples.tsv.
    let t = with(2, serde_json::json!(null));
    let (dir, work) = work_dir("malformed", &t, &g, &left);
    let o = options(&dir);
    let e = calibrate_final(&g, &work, &o).unwrap_err();
    assert!(e.contains("line 3"), "{e}");
    assert_eq!(files_of(&o.out), ["gates.jsonl", "ingest.json"]);
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
    let _ = std::fs::remove_dir_all(&dir);
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
    // The chest in no pair: where the world first draws it, and said; the sword alone.
    let objects = by_class("object");
    let places: Vec<&str> = objects.iter().map(|r| r["placement"].as_str()).collect();
    assert_eq!(places, ["drawn", "alone"]);
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
    let at = At { z: 0, x: 9, y: 9 };
    let f = framed(&g, &super::super::window(&g, at), &[]);
    let drawn: BTreeSet<u16> = f.drawn.iter().flatten().copied().collect();
    assert!((0..4).all(|k| drawn.contains(&(tree + k))));
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
    let text = s.text();
    for line in [
        "replace gen-gen: 0 in the last pass, 0 read, 0 windows",
        "object-terrain gen-mmpx: 0 in the last pass, 0 read, 0 windows",
        "odd gen-mmpx: 2 in the last pass, 2 read, 1 windows",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    // Every class, survivors or not.
    assert_eq!(
        s.classes.iter().map(|c| c.class).collect::<Vec<_>>(),
        Class::ALL
    );
    assert!(text.contains("actor records"), "{text}");
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
    assert!(s.sheets.is_empty());
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
    let files: Vec<String> = std::fs::read_dir(&o.out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let mut files = files;
    files.sort();
    assert_eq!(files, ["disagreements.txt", "gates.jsonl", "ingest.json"]);
    assert!(!e.contains("t:"));
    let detail = std::fs::read_to_string(o.out.join("disagreements.txt")).unwrap();
    assert_eq!(detail.lines().count(), 2);
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
        ["gates.jsonl", "ingest.json", "unshown.txt"]
    );
    let detail = std::fs::read_to_string(o.out.join("unshown.txt")).unwrap();
    assert_eq!(
        detail,
        format!(
            "unread: line 6: t:{B} t:{GRASS} east (map gen-mmpx) at 0,4,3: drawn in no window\n"
        )
    );
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
        ["gates.jsonl", "ingest.json", "unshown.txt"]
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
    let originals: Vec<(PathBuf, Vec<u8>)> = [&master, &gates_path, &list_path]
        .into_iter()
        .map(|p| (p.clone(), std::fs::read(p).unwrap()))
        .collect();
    let mut blank = Rgba::new(128, 128);
    img::fill(&mut blank, 0, 0, 128, 128, [9, 9, 9, 255]);
    let png = img::encode_png(&blank).unwrap();
    // A re-ingest, once the run has read the state: a master rewritten, one added, gates.jsonl
    // or ingest.json rewritten. Each fails the run, the names and a count in the message.
    let changes: Vec<(&str, Box<dyn Fn() + Send>)> = vec![
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
    let digests: Vec<(u16, String)> = slices
        .digests()
        .iter()
        .map(|(&r, d)| (r, d.clone()))
        .collect();
    assert_eq!(digests, [(B, work::digest(&[&originals[0].1]))]);
    assert!(slices.changed(&work.path("slices")).unwrap().is_empty());
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

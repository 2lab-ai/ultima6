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
    // A pair logged twice in one pass: the stale line before the one that agrees, or a pass-1
    // line again. Neither line is taken for the record.
    let twice = text.clone() + &last(&stale) + "\n" + &last(ab) + "\n" + &last(bg) + "\n";
    assert_eq!(state(&g, &twice, &left).unwrap_err(), ["duplicate"]);
    let again = text.clone() + text.lines().nth(1).unwrap() + "\n";
    assert_eq!(state(&g, &again, &left).unwrap_err(), ["duplicate"]);
    // The same pair in two passes is no duplicate (the log of the last pass above).
    assert!(state(&g, &with, &left).is_ok());
    // A pair passing in a pass after the last.
    let after = pair_line(3, BG, false, bg.result, bg.sides, "map", bg.at, &gates());
    let late = with.clone() + &after + "\n";
    assert_eq!(state(&g, &late, &left).unwrap_err(), ["last"]);
    // No fallback: pass 1 is the last pass, logged whole.
    let only = logged(&g, &left, BG, "map", Some(At { z: 0, x: 4, y: 3 })) + "\n";
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
    // A pair of the last pass with no cell: no window could be placed at it.
    let nowhere = edit(2, &|v| v["at"] = serde_json::Value::Null);
    assert_eq!(state(&g, &nowhere, &left).unwrap_err(), ["cell"]);
    // ... nor a pass-1 pair that no side of is generated any more: it is no pair of the last pass.
    let gone = edit(0, &|v| v["at"] = serde_json::Value::Null);
    assert!(state(&g, &gone, &left).is_ok());
    // Gated with other thresholds: said alone.
    let t = Thresholds {
        seam: 2.0,
        ..Thresholds::default()
    };
    let other_gates = text.replace(&gates(), &t.text());
    assert_eq!(state(&g, &other_gates, &left).unwrap_err(), ["gates"]);
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
/// in `ingest.json` and each written to `slices/` as its ×8 master (each ×4 pixel 2 × 2).
fn work_dir(name: &str, gates: &str, g: &Game, images: &BTreeMap<u16, Rgba>) -> (PathBuf, Work) {
    let dir = std::env::temp_dir().join(format!("u6-hdtool-final-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let work = Work::open(Some(&dir.join("work"))).unwrap();
    work.write("gates.jsonl", gates.as_bytes()).unwrap();
    let list = listed(g, images.keys().copied());
    work.write("ingest.json", &serde_json::to_vec(&list).unwrap())
        .unwrap();
    std::fs::create_dir_all(work.path("slices")).unwrap();
    for (&r, img4) in images {
        let mut m = Rgba::new(img4.width * 2, img4.height * 2);
        for y in 0..m.height {
            for x in 0..m.width {
                img::put(&mut m, x, y, img::get(img4, x / 2, y / 2));
            }
        }
        let stem = atlas::slice_name(ImageId::Tile(r));
        img::write_png(&work.slice(&stem), &m).unwrap();
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
    assert_eq!((s.passes, s.logged_last, s.first, s.last), (2, false, 3, 2));
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
    // Both pairs of the last pass drawn in the window, each ticked as the last pass has it: A
    // beside B is gen-mmpx now (A fell), with the measure of the slices, not pass 1's gen-gen.
    let name = |r: u16| atlas::image_name(ImageId::Tile(r));
    let slices = Slices::new(&g, left.clone());
    let ticks: Vec<Vec<&str>> = w["ticks"]
        .split(';')
        .map(|t| t.split(',').collect())
        .collect();
    assert_eq!(ticks.len(), 2);
    assert_eq!(w["pairs"], "2");
    assert_eq!(w["kinds"], "map/gen-mmpx=2");
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
    let first = record(&text).unwrap().pairs[1].clone();
    assert_eq!(first.pair, AB);
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
    let (dir, work) = work_dir("items", "", &g, &images);
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
    // In no pair: where the world first draws it (the tree's top-left part), and said.
    assert_eq!(
        (
            trees[0]["placement"].as_str(),
            trees[0]["x"].as_str(),
            trees[0]["y"].as_str()
        ),
        ("drawn", "9", "9")
    );
    assert!(trees[0]["note"].starts_with("in no pair"));
    let objects = by_class("object");
    let places: Vec<&str> = objects.iter().map(|r| r["placement"].as_str()).collect();
    assert_eq!(places, ["drawn", "alone"]);
    assert!(objects[1]["note"].starts_with("drawn nowhere"));
    // The window of a multi-record item draws all of it: each of its records in the frame.
    let class = |c: Class| s.classes.iter().find(|x| x.class == c).unwrap().clone();
    let m = class(Class::Multi);
    assert_eq!(
        (m.items, m.images, m.pair, m.drawn, m.alone),
        (1, 4, 0, 1, 0)
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
    // A label the gate does not write yet, on B beside the grass.
    let text = text.replacen(r#""source":"map""#, r#""source":"map+odd""#, 3);
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
        let n = if l == "map" || l == "odd" { 2 } else { 0 };
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
            "unread: line 3: t:{B} t:{GRASS} east (map gen-mmpx) at 0,4,3: drawn in no window\n"
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
    let first = BTreeMap::from([(A, red_west(&near(&g, A))), (B, near(&g, B))]);
    let bt = Pair {
        a: B,
        b: T,
        edge: Edge::East,
    };
    let line = logged(&g, &first, bt, "map", Some(At { z: 0, x: 4, y: 5 }));
    let text = text + &line + "\n";
    let (dir, work) = work_dir("unmeasured", &text, &g, &left);
    let o = options(&dir);
    let s = calibrate_final(&g, &work, &o).unwrap();
    let f = state(&g, &text, &left).unwrap();
    let x = f.last.iter().find(|l| l.pair == bt).unwrap();
    assert_eq!((x.result, x.first.1), (None, None));
    // Drawn in a window: B shows along its east side, T, with nothing on its west line, in its
    // cell.
    assert_eq!((s.last, s.unread), (3, 0));
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
    let first = BTreeMap::from([(A, red_west(&near(&g, A))), (B, near(&g, B))]);
    let by = Pair {
        a: B,
        b: Y,
        edge: Edge::East,
    };
    let text = text + &logged(&g, &first, by, "map", Some(At { z: 0, x: 12, y: 12 })) + "\n";
    let (dir, work) = work_dir("every", &text, &g, &left);
    let o = options(&dir);
    let s = calibrate_final(&g, &work, &o).unwrap();
    assert_eq!(s.last, 3);
    // Each of the three is drawn in a window: B's own, and one pair window for what it misses.
    for k in &s.kinds {
        assert_eq!(k.read, k.last, "{} {}", k.label, k.sides.name());
    }
    assert_eq!((s.pair_windows, s.unread), (1, 0));
    let map = s
        .kinds
        .iter()
        .find(|k| k.label == "map" && k.sides == Sides::GenMmpx);
    assert_eq!(map.map(|k| (k.last, k.windows)), Some((3, 2)));
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

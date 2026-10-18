//! `calibrate` on synthetic records, worlds and `gates.jsonl` lines only: the synthetic game's
//! records placed by the tests, rows written by the tests, images drawn by the tests.

use std::collections::BTreeMap;
use std::rc::Rc;

use u6_formats::position::Position;
use u6_hdpack::tile_hash;
use u6_render::hd::{Compositor, Image, Images, Pack, PackHd, Scale};
use u6_render::{Framebuffer, Kind, Src, WIDTH};

use super::*;
use crate::game::{Level, Placed, World, synthetic};
use crate::gate::Thresholds;

/// A chunk of one map tile everywhere but the cells given (x, y, tile).
fn chunk(fill: u8, cells: &[(usize, usize, u8)]) -> [[u8; 8]; 8] {
    let mut c = [[fill; 8]; 8];
    for &(x, y, t) in cells {
        c[y][x] = t;
    }
    c
}

fn placed(x: u16, y: u16, z: u8, tile: u16) -> Placed {
    Placed { x, y, z, tile }
}

const GRASS: u16 = 0;
const STONE: u16 = 2;
const WATER: u16 = 1;
const CHEST: u16 = synthetic::OBJECTS;
const POTION: u16 = synthetic::OBJECTS + 1;
const SWORD: u16 = synthetic::OBJECTS + 2;

/// A level of 2 × 2 chunks, grass and stone alternating, water in two cells of the first chunk;
/// a second level of one chunk. Objects: a 2 × 2 tree whose anchor is east and south of the
/// window below and its west parts inside it, three objects in one cell (one a top tile), a tile
/// that replaces its cell's base, and an object on the second level. The leader at (1, 1).
fn game() -> Game {
    let mut g = synthetic::game();
    g.world = World {
        chunks: vec![
            chunk(GRASS as u8, &[(1, 1, WATER as u8), (5, 1, WATER as u8)]),
            chunk(STONE as u8, &[]),
            chunk(3, &[]),
        ],
        levels: vec![
            Level {
                side: 2,
                refs: vec![0, 1, 1, 0],
            },
            Level {
                side: 1,
                refs: vec![2],
            },
        ],
        objects: vec![
            placed(6, 3, 0, synthetic::TREE + 3),
            placed(4, 0, 0, CHEST),
            placed(4, 0, 0, SWORD),
            placed(4, 0, 0, POTION),
            placed(2, 3, 0, synthetic::LEFT_HALF),
            placed(1, 1, 1, SWORD),
        ],
    };
    g.flags2[usize::from(CHEST)] |= TOP;
    g.flags3[usize::from(synthetic::LEFT_HALF)] = REPLACES_BASE;
    g.leader = Some(Position { x: 1, y: 1, z: 0 });
    g
}

/// The water's frame the pair below is measured with (not its frame 0).
const WATER_PAIR: u16 = synthetic::WATER_FRAMES + 2;

/// Water at (1, 1) beside grass, east: in the window of (1, 1), which wraps across the level's
/// first column and row.
fn the_pair() -> (At, Pair) {
    let at = At { z: 0, x: 1, y: 1 };
    let pair = Pair {
        a: WATER_PAIR,
        b: GRASS,
        edge: Edge::East,
    };
    (at, pair)
}

/// The record drawn at its place, by hand: its opaque pixels, each naming it.
fn put(fb: &mut Framebuffer, g: &Game, r: u16, (c, row): (usize, usize)) {
    let rec = &g.records[usize::from(r)];
    for i in 0..TILE_W * TILE_W {
        if !rec.transparent[i] {
            let (u, v) = (i % TILE_W, i / TILE_W);
            let src = Src {
                ver: 0,
                id: r,
                u: u as u16,
                v: v as u16,
                kind: Kind::Tile,
                flags: 0,
            };
            fb.set_src(c * TILE_W + u, row * TILE_W + v, rec.pixels[i], src);
        }
    }
}

/// The frame of [`the_pair`]'s window, written cell by cell from what the test placed.
fn by_hand(g: &Game) -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.enable_src();
    for (i, c) in synthetic::PALETTE.iter().enumerate() {
        fb.palette[i] = Rgb::new(c[0], c[1], c[2]);
    }
    for r in 0..ROWS {
        for c in 0..COLS {
            // The window starts at column 14, row 15 of the 16-tile level.
            let (x, y) = ((14 + c) % 16, (15 + r) % 16);
            let stone = (x >= 8) != (y >= 8);
            let water = y == 1 && (x == 1 || x == 5);
            let tiles: Vec<u16> = match (x, y) {
                // The tree's west parts (its anchor at (6, 3) is outside the window).
                (5, 3) => vec![GRASS, synthetic::TREE + 2],
                (5, 2) => vec![GRASS, synthetic::TREE],
                // The later of the starting lists first, each in front of the list; the top
                // tile behind them.
                (4, 0) => vec![GRASS, SWORD, POTION, CHEST],
                // The tile that replaces the base.
                (2, 3) => vec![synthetic::LEFT_HALF],
                // The pair's water: its record measured; the other water at frame 0.
                (1, 1) => vec![WATER_PAIR],
                _ if water => vec![synthetic::WATER_FRAMES],
                _ if stone => vec![STONE],
                _ => vec![GRASS],
            };
            for t in tiles {
                put(&mut fb, g, t, (c, r));
            }
        }
    }
    fb
}

/// The test's images and pack for the compositor (kept apart from the module's).
struct TestImages<'a>(&'a Game);

impl Images for TestImages<'_> {
    fn image(&mut self, kind: Kind, id: u16) -> Option<Image<'_>> {
        let r = &self.0.records[usize::from(id)];
        (kind == Kind::Tile).then_some(Image {
            ver: 0,
            width: 16,
            height: 16,
            pixels: &r.pixels,
            transparent: Some(&r.transparent),
        })
    }

    fn key(&mut self, _: Kind, id: u16) -> Option<u64> {
        let r = &self.0.records[usize::from(id)];
        tile_hash(&r.pixels, &r.transparent)
    }
}

struct TestPack(BTreeMap<u16, (u64, Rgba)>);

impl Pack for TestPack {
    fn hd(&self, _: Kind, id: u16, key: u64) -> Option<PackHd<'_>> {
        let (k, img) = self.0.get(&id)?;
        (*k == key).then_some(PackHd {
            index: u32::from(id) + 1000,
            width: 64,
            height: 64,
            rgba: &img.pixels,
        })
    }
}

/// Made-up ×4 images: a gradient with opaque, clear and part-transparent pixels.
fn hd4(seed: u32) -> Rgba {
    let mut out = Rgba::new(64, 64);
    for y in 0..64 {
        for x in 0..64 {
            let a = [255, 255, 0, 120][((x + y + seed) % 4) as usize];
            let c = [(x * 4) as u8, (y * 4) as u8, (seed * 40) as u8, a];
            img::put(&mut out, x, y, c);
        }
    }
    out
}

/// The pack of the tests: the pair's two records and a tree part.
fn pack_images() -> BTreeMap<u16, Rgba> {
    BTreeMap::from([
        (WATER_PAIR, hd4(1)),
        (GRASS, hd4(2)),
        (synthetic::TREE, hd4(3)),
    ])
}

/// What a compositor of its own writes for `fb` at `scale`, the window's part of it.
fn composed(g: &Game, fb: &Framebuffer, pack: Option<Rc<dyn Pack>>, scale: Scale) -> Rgba {
    let mut c = Compositor::new();
    c.set_pack(pack);
    c.compose(fb, &mut TestImages(g), scale);
    let s = scale.factor();
    let (w, h) = (COLS * TILE_W * s, ROWS * TILE_W * s);
    let mut out = Rgba::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let p = c.rgba()[y * WIDTH * s + x].to_le_bytes();
            img::put(&mut out, x as u32, y as u32, p);
        }
    }
    out
}

#[test]
fn the_window_is_drawn_as_the_world_places_the_map_and_the_objects() {
    let g = game();
    let (at, pair) = the_pair();
    let f = frame(&g, at, Edge::East, Some(pair));
    let want = by_hand(&g);
    assert_eq!(f.fb.palette, want.palette);
    assert_eq!(f.fb.pixels(), want.pixels());
    assert_eq!(f.fb.src(), want.src());
    // The other water at frame 0, the pair's at its own frame.
    assert_eq!(
        (f.frame0, f.pair_frame, f.frames()),
        (true, true, "frame0+pair")
    );
    // Without the pair, the pair's water is at frame 0 too.
    let plain = frame(&g, at, Edge::East, None);
    assert_eq!(plain.frames(), "frame0");
    assert_ne!(plain.fb.pixels(), want.pixels());
    // Off the levels: nothing drawn.
    let off = frame(&g, At { z: 5, x: 0, y: 0 }, Edge::East, None);
    assert!(off.fb.src().unwrap().iter().all(|s| *s == Src::CLASSIC));
}

#[test]
fn the_pack_panel_is_the_compositors_frame_at_s4_and_s2() {
    let g = game();
    let (at, pair) = the_pair();
    let f = frame(&g, at, Edge::East, Some(pair));
    let want = by_hand(&g);
    let slices = Rc::new(Slices::new(&g, pack_images()));
    let test_pack: Rc<dyn Pack> = Rc::new(TestPack(
        pack_images()
            .into_iter()
            .map(|(r, img)| {
                let rec = &g.records[usize::from(r)];
                (r, (tile_hash(&rec.pixels, &rec.transparent).unwrap(), img))
            })
            .collect(),
    ));
    for scale in [Scale::X4, Scale::X2] {
        let p = panels(&g, &f.fb, &slices, scale);
        assert_eq!(
            p.hd,
            composed(&g, &want, Some(test_pack.clone()), scale),
            "{scale:?}"
        );
        assert_eq!(p.plain, composed(&g, &want, None, scale), "{scale:?}");
        assert_ne!(p.hd, p.plain);
        // The pair's cells are the images the gate measures: the pack's image over the classic
        // record where it draws (at S 2 the pack's ×4 box-averaged).
        let s = scale.factor() as u32;
        for (r, (c, row)) in [(pair.a, A_CELL), (pair.b, b_cell(Edge::East))] {
            let classic = g.classic(ImageId::Tile(r)).unwrap();
            let hd = pack_images()[&r].clone();
            let hd = if s == 4 { hd } else { img::shrink(&hd, 2) };
            let shown = gate::shown(&classic, &hd);
            let cell = TILE_W as u32 * s;
            for y in 0..cell {
                for x in 0..cell {
                    let Some(w) = shown[(y * cell + x) as usize] else {
                        continue;
                    };
                    let px = img::get(&p.hd, c as u32 * cell + x, row as u32 * cell + y);
                    assert_eq!([px[0], px[1], px[2]].map(f32::from), w, "{r} ({x}, {y})");
                }
            }
        }
    }
}

#[test]
fn the_classic_panel_is_the_classic_frame_nearest() {
    let g = game();
    let (at, pair) = the_pair();
    let f = frame(&g, at, Edge::East, Some(pair));
    let slices = Rc::new(Slices::new(&g, pack_images()));
    for (scale, s) in [(Scale::X4, 4), (Scale::X2, 2)] {
        let p = panels(&g, &f.fb, &slices, scale);
        let (w, h) = window_px();
        assert_eq!((p.classic.width, p.classic.height), (w * s, h * s));
        for y in 0..h * s {
            for x in 0..w * s {
                let i = f.fb.pixels()[(y / s) as usize * WIDTH + (x / s) as usize];
                let c = synthetic::PALETTE[usize::from(i)];
                assert_eq!(img::get(&p.classic, x, y), [c[0], c[1], c[2], 255]);
            }
        }
    }
}

/// Panels of made-up images with no mark colour in them.
fn made_panels(s: u32, seed: u32) -> Panels {
    let (w, h) = window_px();
    let image = |k: u32| {
        let mut out = Rgba::new(w * s, h * s);
        for y in 0..h * s {
            for x in 0..w * s {
                let v = (x * 7 + y * 3 + k * 50 + seed * 11) % 200;
                img::put(&mut out, x, y, [v as u8, (v / 2) as u8, 30, 255]);
            }
        }
        out
    };
    Panels {
        classic: image(0),
        plain: image(1),
        hd: image(2),
    }
}

#[test]
fn the_join_marks_sit_outside_the_panels() {
    for s in [4, 2] {
        let lines: Vec<Line> = [Edge::East, Edge::South]
            .into_iter()
            .enumerate()
            .map(|(i, edge)| Line {
                caption: format!("#{i} >1.5 gen-mmpx 1.62/1.50"),
                edge,
                panels: made_panels(s, i as u32),
            })
            .collect();
        let out = sheet(&lines, s);
        let l = Layout::of(s);
        assert_eq!((out.width, out.height), (l.width(), l.height(2)));
        let is_mark = |x: u32, y: u32| img::get(&out, x, y) == MARK;
        let cell = TILE_W as u32 * s;
        for (i, line) in lines.iter().enumerate() {
            let p = &line.panels;
            for (k, panel) in [&p.classic, &p.plain, &p.hd].into_iter().enumerate() {
                let (px, py) = l.panel_at(i, k);
                // The art as composed, untouched.
                assert_eq!(&out.rect(px, py, l.w, l.h), panel, "{s} {i} {k}");
                match line.edge {
                    Edge::East => {
                        // Across the join, above the panel: the two columns that meet.
                        let jx = px + 4 * cell;
                        assert!(is_mark(jx - 1, py - 3) && is_mark(jx, py - 3));
                        assert!(!is_mark(jx + 1, py - 3) && !is_mark(jx - 2, py - 3));
                        // Along the edge, left of the panel: A's row only.
                        assert!(is_mark(px - 4, py + 2 * cell));
                        assert!(is_mark(px - 4, py + 3 * cell - 1));
                        assert!(!is_mark(px - 4, py + 3 * cell));
                    }
                    Edge::South => {
                        let jy = py + 3 * cell;
                        assert!(is_mark(px - 3, jy - 1) && is_mark(px - 3, jy));
                        assert!(!is_mark(px - 3, jy + 1));
                        assert!(is_mark(px + 3 * cell, py - 4));
                        assert!(!is_mark(px + 4 * cell, py - 4));
                    }
                }
            }
        }
        // Nothing else is marked: the two marks of each panel, all in its margins.
        let m = l.margin;
        let per_panel = 2 * (m - 4) + 3 * cell;
        let marked = out.pixels.chunks_exact(4).filter(|p| *p == MARK).count();
        assert_eq!(marked as u32, 2 * 3 * per_panel, "{s}");
    }
}

/// A `gates.jsonl` line of a pair checked in pass `pass`.
#[allow(clippy::too_many_arguments)]
fn line(
    pass: usize,
    (a, b, edge): (u16, u16, &str),
    fail: bool,
    ratio: Option<f32>,
    limit: f32,
    sides: &str,
    source: &str,
    (z, x, y): (usize, usize, usize),
) -> String {
    let name = |r: u16| atlas::image_name(ImageId::Tile(r));
    serde_json::json!({
        "neighbour": {"a": name(a), "b": name(b), "edge": edge},
        "pass": pass, "fail": fail, "ratio": ratio, "limit": ratio.map(|_| limit),
        "sides": sides, "source": source, "at": {"z": z, "x": x, "y": y},
        "gates": "made up",
    })
    .to_string()
}

/// Rows of every bucket × sides but fail-le1.1 gen-gen, some of them off the surface map.
fn many_rows() -> String {
    let mut text = String::new();
    let mut n = 0u16;
    let mut add = |count: usize, fail: bool, excess: f32, sides: &str, off: usize| {
        for i in 0..count {
            n += 1;
            // Every `off`-th row on another level or of the objects only.
            let (z, source) = match (off > 0 && i % off == 0, i % 2) {
                (true, 0) => (1, "map"),
                (true, _) => (0, "objects"),
                _ => (0, "map"),
            };
            let at = (z, i % 16, i / 16);
            let ratio = Some(1.5 * excess);
            let l = line(1, (n, n + 1, "east"), fail, ratio, 1.5, sides, source, at);
            text.push_str(&l);
            text.push('\n');
        }
    };
    add(10, false, 0.7, "gen-gen", 3);
    add(4, false, 0.8, "gen-mmpx", 0);
    add(1, true, 1.05, "gen-mmpx", 0);
    add(3, true, 1.3, "gen-gen", 1);
    add(7, true, 1.3, "gen-mmpx", 2);
    add(2, true, 1.8, "gen-gen", 0);
    add(5, true, 2.5, "gen-mmpx", 4);
    text
}

#[test]
fn sampling_is_set_by_the_seed_covers_every_cell_with_rows_and_takes_per() {
    let mut text = many_rows();
    // Lines that are not first-pass rows with a ratio: skipped.
    let pair = (1, 2, "south");
    let skip = [
        line(1, pair, true, None, 1.5, "gen-mmpx", "map", (0, 0, 0)),
        line(2, pair, true, Some(9.0), 1.5, "gen-mmpx", "map", (0, 0, 0)),
        r#"{"item":3,"class":"terrain","fallback":"neighbour","pass":1}"#.to_string(),
        String::new(),
    ];
    for l in skip {
        text.push_str(&l);
        text.push('\n');
    }
    let (rows, skipped) = rows(&text).unwrap();
    assert_eq!((rows.len(), skipped), (32, 1));
    let per = 3;
    let a = sample(&rows, per, 7);
    assert_eq!(a, sample(&rows, per, 7));
    // Another seed draws another sample somewhere.
    assert!((8..16).any(|seed| sample(&rows, per, seed) != a));
    // Every cell that has rows, and only those.
    let mut cells: Vec<(Bucket, Sides)> = rows.iter().map(|r| (Bucket::of(r), r.sides)).collect();
    cells.sort();
    cells.dedup();
    assert_eq!(a.keys().copied().collect::<Vec<_>>(), cells);
    assert_eq!(cells.len(), 7);
    assert!(!a.contains_key(&(Bucket::Le11, Sides::GenGen)));
    let sum: usize = a.values().map(|t| t.had).sum();
    assert_eq!(sum, rows.len());
    for (key, t) in &a {
        assert_eq!(t.rows.len(), per.min(t.had), "{key:?}");
        assert!(t.rows.iter().all(|r| (Bucket::of(r), r.sides) == *key));
        // The surface map first: a cell with enough takes only those; a short one takes them
        // all and fills with the others.
        let surface = t.rows.iter().filter(|r| r.surface_map()).count();
        assert_eq!(surface, t.surface.min(per), "{key:?}");
        // In sheet order.
        assert!(t.rows.windows(2).all(|w| w[0].excess() <= w[1].excess()));
    }
    let take = |b, s| &a[&(b, s)];
    assert_eq!(take(Bucket::Pass, Sides::GenGen).had, 10);
    assert_eq!(take(Bucket::Le11, Sides::GenMmpx).rows.len(), 1);
    // All three of fail-1.1-1.5 gen-gen are off the surface map: taken to fill.
    assert_eq!(take(Bucket::To15, Sides::GenGen).surface, 0);
    assert_eq!(take(Bucket::To15, Sides::GenGen).rows.len(), 3);
    // `per` larger than every cell takes every row.
    let all = sample(&rows, 100, 7);
    assert_eq!(
        all.values().map(|t| t.rows.len()).sum::<usize>(),
        rows.len()
    );
}

#[test]
fn the_buckets_go_by_the_verdict_then_ratio_over_limit() {
    let row = |fail: bool, ratio: f32| Row {
        pair: Pair {
            a: 1,
            b: 2,
            edge: Edge::East,
        },
        fail,
        ratio,
        limit: 2.0,
        sides: Sides::GenGen,
        source: "map".into(),
        at: At { z: 0, x: 0, y: 0 },
    };
    assert_eq!(Bucket::of(&row(false, 9.0)), Bucket::Pass);
    assert_eq!(Bucket::of(&row(true, 2.2)), Bucket::Le11);
    assert_eq!(Bucket::of(&row(true, 2.21)), Bucket::To15);
    assert_eq!(Bucket::of(&row(true, 3.0)), Bucket::To15);
    assert_eq!(Bucket::of(&row(true, 3.01)), Bucket::Gt15);
}

#[test]
fn an_at_names_a_cell_or_the_leader() {
    let cell = |z, x, y, edge| Spot::Cell {
        at: At { z, x, y },
        edge,
    };
    assert_eq!(Spot::parse("0,12,34"), Ok(cell(0, 12, 34, Edge::East)));
    assert_eq!(
        Spot::parse("2, 3, 4, south"),
        Ok(cell(2, 3, 4, Edge::South))
    );
    assert_eq!(Spot::parse("leader"), Ok(Spot::Leader { edge: Edge::East }));
    assert_eq!(
        Spot::parse("leader,south"),
        Ok(Spot::Leader { edge: Edge::South })
    );
    for bad in [
        "",
        "1,2",
        "1,2,3,west",
        "1,2,3,east,5",
        "a,b,c",
        "leader,1,2",
    ] {
        assert!(Spot::parse(bad).is_err(), "{bad:?}");
    }
}

/// A temp dir with a work dir whose `gates.jsonl` is `text`, and a slices dir with the ×8
/// masters of the test's pack (each the ×4 image at ×2, nearest: box-averaged back, the ×4
/// image where it is not clear), a portrait's master and a file that is not a master.
fn setup(name: &str, text: &str) -> (PathBuf, Work, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("u6-hdtool-calibrate-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let work = Work::open(Some(&dir.join("work"))).unwrap();
    work.write("gates.jsonl", text.as_bytes()).unwrap();
    let slices = dir.join("slices");
    std::fs::create_dir_all(&slices).unwrap();
    for (r, img4) in pack_images() {
        let mut m = Rgba::new(128, 128);
        for y in 0..128 {
            for x in 0..128 {
                img::put(&mut m, x, y, img::get(&img4, x / 2, y / 2));
            }
        }
        let stem = atlas::slice_name(ImageId::Tile(r));
        img::write_png(&slices.join(format!("{stem}.png")), &m).unwrap();
    }
    img::write_png(&slices.join("p-a-1.png"), &Rgba::new(8, 8)).unwrap();
    std::fs::write(slices.join("notes.txt"), "not a master").unwrap();
    (dir, work, slices)
}

/// Rows of the test world: the pair at (1, 1), grass and stone across the chunk edges, the
/// wrap's stone beside grass, a failing gen-gen and a passing pair (neither a near control).
fn world_rows() -> String {
    let rows = [
        (
            (WATER_PAIR, GRASS, "east"),
            true,
            3.0,
            "gen-mmpx",
            (0, 1, 1),
        ),
        ((GRASS, STONE, "east"), true, 2.0, "gen-mmpx", (0, 7, 0)),
        ((STONE, GRASS, "east"), true, 1.8, "gen-mmpx", (0, 15, 0)),
        ((GRASS, STONE, "south"), true, 2.0, "gen-gen", (0, 0, 7)),
        ((STONE, GRASS, "south"), false, 1.0, "gen-mmpx", (0, 8, 7)),
    ];
    rows.into_iter()
        .map(|(p, fail, ratio, sides, at)| line(1, p, fail, Some(ratio), 1.5, sides, "map", at))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// `samples.tsv`'s header and rows, split.
fn tsv(path: &Path) -> (String, Vec<Vec<String>>) {
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = text.lines();
    let header = lines.next().unwrap().to_string();
    let rows = lines
        .map(|l| l.split('\t').map(str::to_string).collect())
        .collect();
    (header, rows)
}

#[test]
fn an_at_adds_a_control_panel_and_the_leader_its_near_pairs() {
    let g = game();
    let (dir, work, slices) = setup("control", &world_rows());
    let out = dir.join("calibrate");
    let opts = Options {
        slices,
        out: out.clone(),
        per: 2,
        seed: 1,
        at: vec![
            Spot::Cell {
                at: At { z: 0, x: 7, y: 3 },
                edge: Edge::East,
            },
            Spot::Cell {
                at: At { z: 0, x: 3, y: 3 },
                edge: Edge::South,
            },
            Spot::Leader { edge: Edge::East },
        ],
        seam: Thresholds::default().seam,
    };
    let s = calibrate(&g, &work, &opts).unwrap();
    assert_eq!((s.rows, s.skipped, s.slices), (5, 0, 3));
    // Two cells, the leader's window and the two nearest failing gen-mmpx pairs (of three).
    assert_eq!((s.controls, s.leader), (5, Some(true)));
    assert!(s.sheets.contains(&"control.png".to_string()));
    assert!(s.sheets.contains(&"control-s2.png".to_string()));
    let (_, rows) = tsv(&out.join("samples.tsv"));
    let controls: Vec<&Vec<String>> = rows.iter().filter(|r| r[1] == "control").collect();
    assert_eq!(controls.len(), 5);
    assert!(controls.iter().all(|r| r[13] == "control.png"));
    let name = |r: u16| atlas::image_name(ImageId::Tile(r));
    let pair_of = |r: &Vec<String>| (r[3].clone(), r[4].clone(), r[5].clone());
    // The grass beside the stone east of (7, 3), measured at another cell.
    let c = controls[0];
    assert_eq!(pair_of(c), (name(GRASS), name(STONE), "east".into()));
    assert_eq!((c[6].as_str(), c[7].as_str()), ("2.0000", "1.5000"));
    assert_eq!((c[11].as_str(), c[12].as_str()), ("7", "3"));
    // Grass beside grass: not in the rows.
    let c = controls[1];
    assert_eq!(pair_of(c), ("-".into(), "-".into(), "south".into()));
    assert_eq!(c[6], "-");
    // The leader's cell: the water's frame measured beside the grass, failing.
    let c = controls[2];
    assert_eq!(pair_of(c), (name(WATER_PAIR), name(GRASS), "east".into()));
    assert_eq!(c[16], "frame0+pair");
    // Near it, nearest first: the same pair, then the stone beside the grass across the wrap.
    assert_eq!(pair_of(controls[3]), pair_of(controls[2]));
    assert_eq!(
        pair_of(controls[4]),
        (name(STONE), name(GRASS), "east".into())
    );
    // Its ratio measured again from the slices, as the gate measures it.
    let again = remeasured(
        &g,
        &Slices::read(&g, &opts.slices).unwrap(),
        the_pair().1,
        opts.seam,
    )
    .unwrap();
    assert_eq!(controls[2][8], format!("{again:.4}"));
    // Without a leader in the files, `--at leader` adds nothing.
    let mut none = g.clone();
    none.leader = None;
    let opts = Options {
        at: vec![Spot::Leader { edge: Edge::East }],
        ..opts
    };
    let s = calibrate(&none, &work, &opts).unwrap();
    assert_eq!((s.controls, s.leader), (0, Some(false)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn samples_tsv_names_every_sheet_row_and_a_tall_sheet_is_split() {
    let g = game();
    let mut text = many_rows();
    // 18 more rows of fail-gt1.5 gen-mmpx: more than a sheet at S 4 holds.
    let rows_max = Layout::of(4).rows_max();
    assert_eq!(rows_max, 17);
    for i in 0..18u16 {
        let ratio = Some(3.0 + f32::from(i) / 100.0);
        let at = (0, usize::from(i % 16), 2);
        let l = line(
            1,
            (200 + i, 201 + i, "south"),
            true,
            ratio,
            1.5,
            "gen-mmpx",
            "map",
            at,
        );
        text.push_str(&l);
        text.push('\n');
    }
    let (dir, work, slices) = setup("tsv", &text);
    let out = dir.join("calibrate");
    let opts = Options {
        slices,
        out: out.clone(),
        per: 18,
        seed: 3,
        at: vec![Spot::Leader { edge: Edge::South }],
        seam: Thresholds::default().seam,
    };
    let s = calibrate(&g, &work, &opts).unwrap();
    // The snapshot read is the work dir's file as it was.
    assert_eq!(
        std::fs::read(out.join("gates.jsonl")).unwrap(),
        text.as_bytes()
    );
    let (header, rows) = tsv(&out.join("samples.tsv"));
    assert_eq!(header, TSV_HEADER);
    let columns = header.split('\t').count();
    assert_eq!(columns, 17);
    assert!(rows.iter().all(|r| r.len() == columns));
    assert_eq!(rows.len(), s.tsv_rows);
    let sampled: usize = s.cells.iter().map(|c| c.4).sum();
    assert_eq!(rows.len(), sampled + s.controls);
    // The index runs 1.. over every row.
    for (i, r) in rows.iter().enumerate() {
        assert_eq!(r[0], (i + 1).to_string());
    }
    // Per sheet: its rows numbered 1..n in order, its height n rows, at both scales; a sheet's
    // name is its rows' bucket and sides.
    let mut by_sheet: BTreeMap<String, Vec<&Vec<String>>> = BTreeMap::new();
    for r in &rows {
        by_sheet.entry(r[13].clone()).or_default().push(r);
    }
    let mut named: Vec<String> = Vec::new();
    for (sheet, of) in &by_sheet {
        let panels: Vec<String> = of.iter().map(|r| r[15].clone()).collect();
        let want: Vec<String> = (1..=of.len()).map(|i| i.to_string()).collect();
        assert_eq!(panels, want, "{sheet}");
        let s2 = &of[0][14];
        assert!(of.iter().all(|r| &r[14] == s2));
        assert_eq!(s2, &sheet.replace(".png", "-s2.png"));
        for (file, scale) in [(sheet, 4), (s2, 2)] {
            let bytes = std::fs::read(out.join(file)).unwrap();
            let l = Layout::of(scale);
            assert_eq!(
                img::png_size(&bytes).unwrap(),
                (l.width(), l.height(of.len())),
                "{file}"
            );
            assert!(l.height(of.len()) <= MAX_HEIGHT);
            named.push(file.clone());
        }
        let stem = if of[0][1] == "control" {
            "control".to_string()
        } else {
            format!("{}-{}", of[0][1], of[0][2])
        };
        assert!(of.iter().all(|r| r[1] == of[0][1] && r[2] == of[0][2]));
        assert!(sheet.starts_with(&stem), "{sheet} {stem}");
    }
    let mut written = s.sheets.clone();
    written.sort();
    named.sort();
    assert_eq!(written, named);
    // The tall cell in two parts, at both scales.
    let tall: Vec<&String> = s
        .sheets
        .iter()
        .filter(|f| f.starts_with("fail-gt1.5-gen-mmpx"))
        .collect();
    assert_eq!(
        tall,
        [
            "fail-gt1.5-gen-mmpx-p1.png",
            "fail-gt1.5-gen-mmpx-p1-s2.png",
            "fail-gt1.5-gen-mmpx-p2.png",
            "fail-gt1.5-gen-mmpx-p2-s2.png"
        ]
    );
    assert_eq!(by_sheet["fail-gt1.5-gen-mmpx-p1.png"].len(), rows_max);
    assert_eq!(by_sheet["fail-gt1.5-gen-mmpx-p2.png"].len(), 18 - rows_max);
    // The report: counts and the sheets' names.
    let report = s.text();
    assert!(
        report.contains("fail-gt1.5-gen-mmpx: 18 of 23 (21 of the surface map)"),
        "{report}"
    );
    assert!(report.contains(&format!("samples.tsv: {} rows", rows.len())));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn calibrate_calls_nothing() {
    // It starts no process, `codex` or any other.
    let src = include_str!("../calibrate.rs");
    for word in ["Command", "Codex", "run::", "process::", "spawn"] {
        assert!(!src.contains(word), "calibrate.rs names {word}");
    }
}

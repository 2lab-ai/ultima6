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
use crate::gate::{self, Thresholds};

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
/// window below and its west parts inside it, two objects that overlap in one cell, a top tile
/// and an object it overlaps in another, a tile that replaces its cell's base, and an object on
/// the second level. The leader at (1, 1).
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
            placed(4, 0, 0, SWORD),
            placed(4, 0, 0, POTION),
            placed(3, 0, 0, CHEST),
            placed(3, 0, 0, POTION),
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
                // The later of the starting lists first, each in front of the list.
                (4, 0) => vec![GRASS, SWORD, POTION],
                // A top tile behind the others, though the later in the lists.
                (3, 0) => vec![GRASS, POTION, CHEST],
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
        (f.frame0, f.pair_frame, f.missing, f.frames().as_str()),
        (true, true, false, "frame0+pair")
    );
    // Without the pair, the pair's water is at frame 0 too.
    let plain = frame(&g, at, Edge::East, None);
    assert_eq!(plain.frames(), "frame0");
    assert_ne!(plain.fb.pixels(), want.pixels());
    // A pair whose record no tile of its cell draws: said.
    let stone = Pair {
        a: STONE,
        b: GRASS,
        edge: Edge::East,
    };
    let f = frame(&g, at, Edge::East, Some(stone));
    assert_eq!((f.missing, f.frames().as_str()), (true, "frame0+missing"));
    assert_eq!(f.fb.pixels(), plain.fb.pixels());
    // Off the levels: nothing drawn.
    let off = frame(&g, At { z: 5, x: 0, y: 0 }, Edge::East, None);
    assert!(off.fb.src().unwrap().iter().all(|s| *s == Src::CLASSIC));
}

#[test]
fn a_covered_record_does_not_show_and_draws_no_pair() {
    let mut g = game();
    // Made-up records: one opaque on its east column only, one that draws nothing.
    const EDGE: u16 = 200;
    const EMPTY: u16 = 201;
    let rec = &mut g.records[usize::from(EDGE)];
    for v in 0..TILE_W {
        rec.pixels[v * TILE_W + TILE_W - 1] = 5;
        rec.transparent[v * TILE_W + TILE_W - 1] = false;
    }
    assert!(g.records[usize::from(EMPTY)].is_empty());
    let a = A_CELL.1 * COLS + A_CELL.0;
    let south = (A_CELL.0, A_CELL.1 + 1);
    let cells = |list: &[u16]| {
        let mut cells = vec![Stack::default(); COLS * ROWS];
        cells[a] = Stack {
            base: Some(STONE),
            list: list.to_vec(),
        };
        cells[a + 1].base = Some(GRASS);
        cells[south.1 * COLS + south.0].base = Some(GRASS);
        cells
    };
    let pair = |a: u16, edge: Edge| Pair { a, b: GRASS, edge };
    // The stone under the grass, which is opaque: drawn there, shown nowhere, no pair.
    let f = framed(&g, &cells(&[GRASS]), &[(A_CELL, STONE)]);
    assert_eq!(
        (f.stacked[a].as_slice(), f.drawn[a].as_slice()),
        (&[STONE, GRASS][..], &[GRASS][..])
    );
    assert!(f.missing && !f.shows(A_CELL, STONE));
    assert!(!f.joins(A_CELL, pair(STONE, Edge::East)));
    assert!(!f.joins(A_CELL, pair(STONE, Edge::South)));
    assert!(f.joins(A_CELL, pair(GRASS, Edge::East)));
    // Under the chest, which covers its middle: the stone shows, and meets the grass on both
    // sides; the chest, with no pixel on its east or south line, draws its pairs where it shows.
    let f = framed(&g, &cells(&[CHEST]), &[(A_CELL, STONE)]);
    assert_eq!(f.drawn[a], [STONE, CHEST]);
    assert!(!f.missing);
    assert!(f.joins(A_CELL, pair(STONE, Edge::East)));
    assert!(f.joins(A_CELL, pair(STONE, Edge::South)));
    assert!(f.joins(A_CELL, pair(CHEST, Edge::East)));
    // Covered along its east side only: it shows, it meets the grass south, not east.
    let f = framed(&g, &cells(&[EDGE]), &[(A_CELL, STONE)]);
    assert!(!f.missing && f.shows(A_CELL, STONE));
    assert!(!f.joins(A_CELL, pair(STONE, Edge::East)));
    assert!(f.joins(A_CELL, pair(STONE, Edge::South)));
    assert!(f.joins(A_CELL, pair(EDGE, Edge::East)));
    // A record that draws nothing counts where it is drawn, and only there.
    let f = framed(&g, &cells(&[EMPTY]), &[]);
    assert_eq!(f.drawn[a], [STONE, EMPTY]);
    assert!(f.joins(A_CELL, pair(EMPTY, Edge::East)));
    assert!(!f.joins(A_CELL, pair(WATER, Edge::East)));
    let f = framed(&g, &cells(&[]), &[]);
    assert!(!f.joins(A_CELL, pair(EMPTY, Edge::East)));
    // In the world: an opaque object on the pair's water hides it, and the window says so.
    let (at, p) = the_pair();
    g.world.objects.push(placed(1, 1, 0, STONE));
    let f = frame(&g, at, Edge::East, Some(p));
    assert_eq!(f.drawn[a], [STONE]);
    assert_eq!(
        (f.missing, f.frames().as_str()),
        (true, "frame0+pair+missing")
    );
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
            let shown = gate::shown(&classic, &hd).unwrap();
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
                caption: format!("{i}: >1.5 gen-mmpx 1.62/1.50"),
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
    let a = sample(&rows, per, 7, Source::All);
    assert_eq!(a, sample(&rows, per, 7, Source::All));
    // Another seed draws another sample somewhere.
    assert!((8..16).any(|seed| sample(&rows, per, seed, Source::All) != a));
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
    // `per` 0 takes nothing, with every row or one label (a cell with surface rows keeps no
    // place for them).
    assert!(sample(&rows, 0, 7, Source::All).is_empty());
    assert!(sample(&rows, 0, 7, Source::Label("map".into())).is_empty());
    // `per` larger than every cell takes every row.
    let all = sample(&rows, 100, 7, Source::All);
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
        source: Source::All,
    };
    let s = calibrate(&g, &work, &opts).unwrap();
    assert_eq!((s.rows, s.skipped, s.slices), (5, 0, 3));
    // Two cells, the leader's window and the two nearest failing gen-mmpx pairs (of three).
    assert_eq!((s.controls, s.leader), (5, Some(true)));
    assert!(s.sheets.contains(&"control.png".to_string()));
    assert!(s.sheets.contains(&"control-s2.png".to_string()));
    let (_, rows) = tsv(&run_dir(&out).join("samples.tsv"));
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
        source: Source::All,
    };
    let s = calibrate(&g, &work, &opts).unwrap();
    // The snapshot read is the work dir's file as it was.
    assert_eq!(
        std::fs::read(run_dir(&out).join("gates.jsonl")).unwrap(),
        text.as_bytes()
    );
    let (header, rows) = tsv(&run_dir(&out).join("samples.tsv"));
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
            let bytes = std::fs::read(run_dir(&out).join(file)).unwrap();
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
fn the_default_samples_as_before_the_selector() {
    // The draw of every row (`--source all`), pinned as it was before `--source` existed: the
    // record A and level of each row taken, cell by cell.
    let (rows, _) = rows(&many_rows()).unwrap();
    let taken = |seed, per| -> Vec<(u16, usize)> {
        sample(&rows, per, seed, Source::default())
            .values()
            .flat_map(|t| t.rows.iter().map(|r| (r.pair.a, r.at.z)))
            .collect()
    };
    assert_eq!(Source::default(), Source::All);
    let z = |v: &[u16]| v.iter().map(|&a| (a, 0)).collect::<Vec<_>>();
    let mut want = z(&[2, 3, 9, 12, 13, 14, 15]);
    want.extend([(16, 1), (17, 0), (18, 1)]);
    want.extend(z(&[20, 22, 24, 26, 27, 29, 30, 31]));
    assert_eq!(taken(7, 3), want);
    let mut want = z(&[2, 3, 5, 6, 8, 9, 11, 12, 13, 14, 15]);
    want.extend([
        (16, 1),
        (17, 0),
        (18, 1),
        (19, 1),
        (20, 0),
        (21, 1),
        (22, 0),
        (23, 1),
    ]);
    want.extend(z(&[24, 26, 27]));
    want.extend([(28, 1), (29, 0), (30, 0), (31, 0), (32, 1)]);
    assert_eq!(taken(1, 6), want);
}

/// Rows found in the map, among the objects and in both, on several levels:
/// - fail-gt1.5 gen-mmpx: objects 8 on the surface, 2 on level 3, 1 on 1, 1 on 5; both 2 on the
///   surface, 1 on 2; map 6 on the surface, 2 on 4;
/// - pass gen-gen: objects 2 on level 2, 1 on 4 (none on the surface); map 3 on the surface;
/// - fail-le1.1 gen-gen: map 3 on the surface.
fn sourced_rows() -> Vec<Row> {
    let mut text = String::new();
    let mut n = 0u16;
    let mut add = |count: usize, fail: bool, excess: f32, sides: &str, source: &str, z: usize| {
        for i in 0..count {
            n += 1;
            let ratio = Some(1.5 * excess);
            let at = (z, i, usize::from(n));
            let l = line(1, (n, n + 1, "east"), fail, ratio, 1.5, sides, source, at);
            text.push_str(&l);
            text.push('\n');
        }
    };
    for (count, source, z) in [
        (8, "objects", 0),
        (2, "objects", 3),
        (1, "objects", 1),
        (1, "objects", 5),
        (2, "map+objects", 0),
        (1, "map+objects", 2),
        (6, "map", 0),
        (2, "map", 4),
    ] {
        add(count, true, 2.0, "gen-mmpx", source, z);
    }
    add(2, false, 0.5, "gen-gen", "objects", 2);
    add(1, false, 0.5, "gen-gen", "objects", 4);
    add(3, false, 0.5, "gen-gen", "map", 0);
    add(3, true, 1.05, "gen-gen", "map", 0);
    rows(&text).unwrap().0
}

/// The levels of a cell's rows taken, in level order.
fn levels(t: &Taken) -> Vec<usize> {
    let mut v: Vec<usize> = t.rows.iter().map(|r| r.at.z).collect();
    v.sort_unstable();
    v
}

#[test]
fn a_source_samples_only_its_rows_and_every_level_it_reaches() {
    let rows = sourced_rows();
    let gt = (Bucket::Gt15, Sides::GenMmpx);
    let pass = (Bucket::Pass, Sides::GenGen);
    let le = (Bucket::Le11, Sides::GenGen);
    for seed in [1, 2, 3] {
        let objects = sample(&rows, 6, seed, Source::Label("objects".into()));
        assert_eq!(
            objects,
            sample(&rows, 6, seed, Source::Label("objects".into()))
        );
        // Only the cells with rows found among the objects; only those rows, both kinds.
        assert_eq!(objects.keys().copied().collect::<Vec<_>>(), [pass, gt]);
        for t in objects.values() {
            assert!(t.rows.iter().all(|r| r.source.contains("objects")));
        }
        let t = &objects[&gt];
        assert_eq!((t.had, t.surface), (15, 10));
        // One of each other level, then the surface.
        assert_eq!(levels(t), [0, 0, 1, 2, 3, 5]);
        // No surface rows: the levels first, then the rest of them.
        let t = &objects[&pass];
        assert_eq!((t.had, t.surface), (3, 0));
        assert_eq!(levels(t), [2, 2, 4]);
        // Fewer places than levels: the lower levels, and one place kept for the surface.
        let few = sample(&rows, 3, seed, Source::Label("objects".into()));
        assert_eq!(levels(&few[&gt]), [0, 1, 2]);
        assert_eq!(
            levels(&sample(&rows, 1, seed, Source::Label("objects".into()))[&gt]),
            [0]
        );
        // The map: its rows and those of both.
        let map = sample(&rows, 6, seed, Source::Label("map".into()));
        assert_eq!(map.keys().copied().collect::<Vec<_>>(), [pass, le, gt]);
        for t in map.values() {
            assert!(t.rows.iter().all(|r| r.source.starts_with("map")));
        }
        let t = &map[&gt];
        assert_eq!((t.had, t.surface), (11, 8));
        assert_eq!(levels(t), [0, 0, 0, 0, 2, 4]);
        assert_eq!(map[&le].rows.len(), 3);
        // Every row: the map's surface rows first, nothing else while they last.
        let all = sample(&rows, 6, seed, Source::All);
        let t = &all[&gt];
        assert_eq!((t.had, t.surface), (23, 8));
        assert!(t.rows.iter().all(|r| r.surface_map()));
    }
    assert_eq!(
        Source::parse("objects"),
        Ok(Source::Label("objects".into()))
    );
    assert_eq!(Source::parse("map"), Ok(Source::Label("map".into())));
    assert_eq!(Source::parse("all"), Ok(Source::All));
}

#[test]
fn a_source_is_any_label_of_gates_jsonl() {
    // Every label the gate writes, and one it does not know: each is a source by its name.
    for l in LABELS.iter().copied().chain(["object", "new_kind-2"]) {
        let s = Source::parse(l).unwrap();
        assert_eq!((s.name(), s.prefix()), (l, format!("{l}-")));
    }
    // A name that cannot name a sheet is refused.
    for bad in ["", "a/b", "map objects", "../x", "map+objects"] {
        assert!(Source::parse(bad).is_err(), "{bad:?}");
    }
    // A row's labels: its source split at +, then a label and labels when it has them.
    let pair = (1, 2, "east");
    let with = |extra: serde_json::Value| -> String {
        let mut v: serde_json::Value = serde_json::from_str(&line(
            1,
            pair,
            false,
            Some(1.0),
            1.5,
            "gen-mmpx",
            "map+objects",
            (0, 0, 0),
        ))
        .unwrap();
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        v.to_string()
    };
    let text = [
        with(serde_json::json!({})),
        with(serde_json::json!({"label": "replace"})),
        with(serde_json::json!({"source": "map", "labels": ["object-terrain", "map", "odd"]})),
        with(serde_json::json!({"source": null, "label": "object-terrain"})),
    ]
    .join("\n");
    let (rows, _) = rows(&text).unwrap();
    let sources: Vec<&str> = rows.iter().map(|r| r.source.as_str()).collect();
    assert_eq!(
        sources,
        [
            "map+objects",
            "map+objects+replace",
            "map+object-terrain+odd",
            "object-terrain"
        ]
    );
    // A label takes the rows that have it, the known ones and the others alike; one no row has
    // takes none.
    let took = |l: &str| -> Vec<usize> {
        let s = Source::parse(l).unwrap();
        (0..rows.len()).filter(|&i| s.takes(&rows[i])).collect()
    };
    assert_eq!(took("map"), [0, 1, 2]);
    assert_eq!(took("objects"), [0, 1]);
    assert_eq!(took("replace"), [1]);
    assert_eq!(took("object-terrain"), [2, 3]);
    assert_eq!(took("odd"), [2]);
    assert!(took("object").is_empty());
    assert!(sample(&rows, 6, 1, Source::parse("object").unwrap()).is_empty());
    // A pair line with no label at all is refused.
    let none = with(serde_json::json!({"source": null}));
    assert!(super::rows(&none).is_err());
}

#[test]
fn a_source_names_its_sheets_apart() {
    let g = game();
    // The test world's rows (all of the map), one found among the objects and one in both.
    let text = world_rows()
        + &line(
            1,
            (CHEST, POTION, "east"),
            true,
            Some(3.0),
            1.5,
            "gen-mmpx",
            "objects",
            (0, 3, 0),
        )
        + "\n"
        + &line(
            1,
            (SWORD, POTION, "south"),
            false,
            Some(1.0),
            1.5,
            "gen-gen",
            "map+objects",
            (0, 4, 0),
        )
        + "\n";
    let (dir, work, slices) = setup("source", &text);
    let out = dir.join("calibrate-objects");
    let opts = Options {
        slices,
        out: out.clone(),
        per: 6,
        seed: 1,
        at: vec![Spot::Cell {
            at: At { z: 0, x: 1, y: 1 },
            edge: Edge::East,
        }],
        seam: Thresholds::default().seam,
        source: Source::Label("objects".into()),
    };
    let s = calibrate(&g, &work, &opts).unwrap();
    assert_eq!(s.rows, 7);
    assert_eq!(
        s.sheets,
        [
            "objects-pass-gen-gen.png",
            "objects-pass-gen-gen-s2.png",
            "objects-fail-gt1.5-gen-mmpx.png",
            "objects-fail-gt1.5-gen-mmpx-s2.png",
            "objects-control.png",
            "objects-control-s2.png",
        ]
    );
    let (_, rows) = tsv(&run_dir(&out).join("samples.tsv"));
    let sources: Vec<&str> = rows.iter().map(|r| r[9].as_str()).collect();
    assert_eq!(sources, ["map+objects", "objects", "map"]);
    assert!(rows.iter().all(|r| r[13].starts_with("objects-")));
    let report = s.text();
    assert!(report.contains("source objects: "), "{report}");
    assert!(
        report.contains("objects-fail-gt1.5-gen-mmpx: 1 of 1 (1 on the surface)"),
        "{report}"
    );
    // Every row, in the same out dir: the default names, none a name of the source's, in a run
    // dir of its own; the source's sheets left in theirs.
    let (first, first_dir) = (s.sheets.clone(), s.written.dir.clone());
    let opts = Options {
        source: Source::All,
        ..opts
    };
    let s = calibrate(&g, &work, &opts).unwrap();
    assert!(s.sheets.iter().all(|f| !f.starts_with("objects-")));
    assert!(s.sheets.iter().all(|f| !first.contains(f)));
    assert!(s.sheets.contains(&"control.png".to_string()));
    for f in ["objects-pass-gen-gen.png", "objects-control-s2.png"] {
        assert!(!s.written.dir.join(f).exists(), "{f}");
        assert!(first_dir.join(f).is_file(), "{f}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The entries of `dir`, sorted.
fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// The run dir of the last run under the out dir `out` (by its name, which sorts by time).
fn run_dir(out: &Path) -> PathBuf {
    files_in(out)
        .into_iter()
        .rfind(|f| f.starts_with("run-"))
        .map(|f| out.join(f))
        .unwrap_or_else(|| panic!("no run dir in {}", out.display()))
}

/// The sheets a first pass of the world's rows draws at `--per 2 --seed 1` with no control
/// window: each sampled cell's, at ×4 and ×2.
const SHEETS_PER_2: [&str; 8] = [
    "pass-gen-mmpx.png",
    "pass-gen-mmpx-s2.png",
    "fail-1.1-1.5-gen-gen.png",
    "fail-1.1-1.5-gen-gen-s2.png",
    "fail-1.1-1.5-gen-mmpx.png",
    "fail-1.1-1.5-gen-mmpx-s2.png",
    "fail-gt1.5-gen-mmpx.png",
    "fail-gt1.5-gen-mmpx-s2.png",
];

#[test]
fn a_first_pass_writes_its_own_run_dir_alone() {
    // The cases of the final read's run dir, for the first pass: a second run's dir holds its
    // own sheets alone, and the first run's dir is as it was; a refused run keeps its
    // diagnostics alone; an out dir of the owner's is written in, its files left.
    let g = game();
    let (dir, work, slices) = setup("one-run", &world_rows());
    let out = dir.join("calibrate");
    let opts = Options {
        slices,
        out: out.clone(),
        per: 2,
        seed: 1,
        at: vec![Spot::Cell {
            at: At { z: 0, x: 7, y: 3 },
            edge: Edge::East,
        }],
        seam: Thresholds::default().seam,
        source: Source::All,
    };
    let pngs = |run: &Path| -> Vec<String> {
        files_in(run)
            .into_iter()
            .filter(|f| f.ends_with(".png"))
            .collect()
    };
    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    let contents = |d: &Path| -> Vec<(String, Vec<u8>)> {
        files_in(d)
            .into_iter()
            .map(|f| {
                let b = std::fs::read(d.join(&f)).unwrap();
                (f, b)
            })
            .collect()
    };
    let s1 = calibrate(&g, &work, &opts).unwrap();
    assert!(s1.sheets.contains(&"control.png".to_string()));
    assert_eq!(pngs(&s1.written.dir), sorted(s1.sheets.clone()));
    assert!(s1.written.dir.join(out_dir::DONE).is_file());
    let kept = contents(&s1.written.dir);
    // A second run with no control window: its sheets alone, in its own dir.
    let opts2 = Options {
        at: Vec::new(),
        ..opts.clone()
    };
    let s2 = calibrate(&g, &work, &opts2).unwrap();
    assert_ne!(s2.written.dir, s1.written.dir);
    assert!(!s2.sheets.contains(&"control.png".to_string()));
    assert_eq!(pngs(&s2.written.dir), sorted(s2.sheets.clone()));
    assert_eq!(contents(&s1.written.dir), kept);
    // A refused run (a line of the log that is not a pair): its diagnostics alone, no sheet, no
    // samples.tsv, no done.
    let bad = r#"{"neighbour":7,"pass":1,"fail":false}"#;
    work.write("gates.jsonl", format!("{bad}\n").as_bytes())
        .unwrap();
    let e = calibrate(&g, &work, &opts2).unwrap_err();
    assert!(e.contains("not a pair"), "{e}");
    let run = run_dir(&out);
    assert_eq!(files_in(&run), ["gates.jsonl", "refused.txt"]);
    let refused = std::fs::read_to_string(run.join("refused.txt")).unwrap();
    assert_eq!(refused, format!("{e}\n"));
    work.write("gates.jsonl", world_rows().as_bytes()).unwrap();
    // The next run lists the refused one as incomplete, and leaves it, and the second run's
    // dir, as they were, byte for byte.
    let (refused_had, second_had) = (contents(&run), contents(&s2.written.dir));
    let s3 = calibrate(&g, &work, &opts2).unwrap();
    let name = run.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(s3.written.incomplete, [format!("{name} (refused)")]);
    assert_eq!(contents(&run), refused_had);
    assert_eq!(contents(&s2.written.dir), second_had);
    assert_eq!(contents(&s1.written.dir), kept);
    // A run that fails late, its sheets and samples.tsv written (its done not: no room): its
    // dir holds the snapshot and refused.txt alone; every earlier run's as it was.
    let third_had = contents(&s3.written.dir);
    let wrote: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
    let w = wrote.clone();
    let o = out.clone();
    // The files a run of these options makes before done: the snapshot, its sheets (named here,
    // not taken from a run's report) and samples.tsv.
    let mut expected: Vec<String> = SHEETS_PER_2.iter().map(|s| s.to_string()).collect();
    expected.extend(["gates.jsonl".to_string(), "samples.tsv".to_string()]);
    let expected = sorted(expected);
    assert_eq!(
        sorted(s3.sheets.clone()),
        sorted(SHEETS_PER_2.map(String::from).to_vec())
    );
    let late = out_dir::hook(&out, "write", move |n| {
        w.lock().unwrap().push(n.to_string());
        if n == out_dir::DONE {
            // Those files are there, each with bytes, before done fails.
            let run = run_dir(&o);
            assert_eq!(files_in(&run), expected);
            for f in &expected {
                let n = std::fs::metadata(run.join(f)).unwrap().len();
                assert!(n > 0, "{f}: {n} bytes before done");
            }
            return Err("done: no room left".into());
        }
        Ok(())
    });
    let e = calibrate(&g, &work, &opts2).unwrap_err();
    drop(late);
    let wrote = wrote.lock().unwrap().clone();
    assert!(wrote.iter().any(|n| n == "samples.tsv"), "{wrote:?}");
    assert!(wrote.iter().any(|n| n.ends_with(".png")), "{wrote:?}");
    let failed = run_dir(&out);
    assert_eq!(files_in(&failed), ["gates.jsonl", "refused.txt"]);
    let refused = std::fs::read_to_string(failed.join("refused.txt")).unwrap();
    assert_eq!(refused, format!("{e}\n"));
    assert!(e.starts_with("done: no room left; run dir: "), "{e}");
    assert_eq!(contents(&run), refused_had);
    assert_eq!(contents(&s2.written.dir), second_had);
    assert_eq!(contents(&s3.written.dir), third_had);
    assert_eq!(contents(&s1.written.dir), kept);
    // An out dir of the owner's, a file of theirs in it: written in, the file left.
    let theirs = dir.join("theirs");
    std::fs::create_dir_all(&theirs).unwrap();
    std::fs::write(theirs.join("notes.txt"), b"the owner's").unwrap();
    let o3 = Options {
        out: theirs.clone(),
        ..opts2.clone()
    };
    let s = calibrate(&g, &work, &o3).unwrap();
    let run = s
        .written
        .dir
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(files_in(&theirs), ["notes.txt", run.as_str()]);
    assert_eq!(
        std::fs::read(theirs.join("notes.txt")).unwrap(),
        b"the owner's"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_first_pass_snapshot_is_read_through_the_run_dir_not_by_its_path() {
    // The run dir renamed away once gates.jsonl is written, before it is read back: read
    // through the dir the run holds open, the snapshot is there (by its path it is not), so the
    // run goes on in the dir where it is now, drawing each sheet and samples.tsv, and fails at
    // its end, at the check of where its dir is, and there alone, saying where that is.
    let g = game();
    let (dir, work, slices) = setup("read-through", &world_rows());
    let out = dir.join("calibrate");
    let opts = Options {
        slices,
        out: out.clone(),
        per: 2,
        seed: 1,
        at: Vec::new(),
        seam: Thresholds::default().seam,
        source: Source::All,
    };
    let away = dir.join("away");
    let (o, to) = (out.clone(), away.clone());
    let was: std::sync::Arc<std::sync::Mutex<Option<PathBuf>>> = Default::default();
    let w = was.clone();
    let _h = out_dir::hook(&out, "snapshot", move |name| {
        if name == "gates.jsonl" {
            let run = run_dir(&o).canonicalize().unwrap();
            std::fs::rename(&run, &to).unwrap();
            *w.lock().unwrap() = Some(run);
        }
        Ok(())
    });
    let wrote: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
    let seen = wrote.clone();
    let _w = out_dir::hook(&out, "write", move |name| {
        seen.lock().unwrap().push(name.to_string());
        Ok(())
    });
    let e = calibrate(&g, &work, &opts).unwrap_err();
    let run = was.lock().unwrap().take().unwrap();
    assert_eq!(
        e,
        format!(
            "the run dir {} was moved or removed while the run wrote it (it was at {} when \
             checked): nothing is written or deleted but in the dir it made; run dir: {}",
            run.display(),
            away.canonicalize().unwrap().display(),
            run.display()
        )
    );
    // Every sheet and samples.tsv written after the rename, then no done: the check before it.
    let mut want = vec!["gates.jsonl".to_string()];
    want.extend(SHEETS_PER_2.map(String::from));
    want.extend(["samples.tsv".to_string(), "refused.txt".to_string()]);
    assert_eq!(*wrote.lock().unwrap(), want);
    assert_eq!(files_in(&away), ["gates.jsonl", "refused.txt"]);
    assert!(files_in(&out).is_empty(), "{:?}", files_in(&out));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_first_pass_whose_log_cannot_be_read_says_why_with_its_own_hint() {
    // The work dir's gates.jsonl not there: the error says to run ingest first. In a child
    // under a limit of 64 open files, with every fd but the run dir's own taken (by the test):
    // its read meets "too many open files", and the error says how to go on, not to ingest.
    const NAME: &str = "a_first_pass_whose_log_cannot_be_read_says_why_with_its_own_hint";
    let test = out_dir::tests::test_path(module_path!(), NAME);
    let g = game();
    let (dir, work, slices) = setup(
        &format!("first-read-{}", out_dir::tests::is_child(&test)),
        &world_rows(),
    );
    let out = dir.join("calibrate");
    let opts = Options {
        slices,
        out: out.clone(),
        per: 2,
        seed: 1,
        at: Vec::new(),
        seam: Thresholds::default().seam,
        source: Source::All,
    };
    let log = work.path("gates.jsonl");
    if !out_dir::tests::is_child(&test) {
        std::fs::remove_file(&log).unwrap();
        let e = calibrate(&g, &work, &opts).unwrap_err();
        let run = run_dir(&out).canonicalize().unwrap();
        assert_eq!(
            e,
            format!(
                "{}: {} (run ingest first); run dir: {}",
                log.display(),
                std::io::Error::from_raw_os_error(libc::ENOENT),
                run.display()
            )
        );
        let _ = std::fs::remove_dir_all(&dir);
        return out_dir::tests::in_child(&test, "ulimit -n 64", |_| {});
    }
    let taken: std::sync::Arc<std::sync::Mutex<Vec<std::fs::File>>> = Default::default();
    let (t, probe) = (taken.clone(), log.clone());
    let _h = out_dir::hook(&out, "open", move |_| {
        *t.lock().unwrap() = out_dir::tests::take_fds(&probe, 1);
        Ok(())
    });
    let e = calibrate(&g, &work, &opts).unwrap_err();
    taken.lock().unwrap().clear();
    let run = run_dir(&out).canonicalize().unwrap();
    let (emfile, hint) = (
        std::io::Error::from_raw_os_error(libc::EMFILE),
        "(too many files open: raise the limit on open files, ulimit -n)",
    );
    assert_eq!(
        e,
        format!(
            "{}: {emfile} {hint}; run dir: {}; refused.txt not written ({}: {emfile} {hint})",
            log.display(),
            run.display(),
            run.join("refused.txt").display()
        )
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_malformed_first_pass_snapshot_is_said_by_its_full_path_with_the_run_dir() {
    // An older incomplete run in --out, then a line of the log that is not a pair: the error
    // names the snapshot by its path in the run's dir, then the run dir and the incomplete run;
    // refused.txt says the same.
    let g = game();
    let bad = r#"{"neighbour":7,"pass":1,"fail":false}"#;
    for (k, log, said) in [
        (
            "pair",
            format!("{bad}\n").into_bytes(),
            "gates.jsonl line 1: a neighbour field that is not a pair",
        ),
        ("utf8", b"\xff\n".to_vec(), "gates.jsonl is not UTF-8"),
    ] {
        malformed_first_pass(&g, k, &log, said);
    }
}

/// The first pass over the log `log` (an older incomplete run beside it) fails with `said`
/// after the snapshot's full path, then the run dir and the incomplete run; refused.txt the same.
fn malformed_first_pass(g: &Game, k: &str, log: &[u8], said: &str) {
    let (dir, work, slices) = setup(&format!("malformed-{k}"), "");
    work.write("gates.jsonl", log).unwrap();
    let out = dir.join("calibrate");
    std::fs::create_dir_all(&out).unwrap();
    let older = out_dir::run_name(1, 7);
    std::fs::create_dir(out.join(&older)).unwrap();
    let opts = Options {
        slices,
        out: out.clone(),
        per: 2,
        seed: 1,
        at: Vec::new(),
        seam: Thresholds::default().seam,
        source: Source::All,
    };
    let e = calibrate(g, &work, &opts).unwrap_err();
    let run = run_dir(&out).canonicalize().unwrap();
    assert!(
        e.starts_with(&format!("{}/{said}", run.display())),
        "{k}: {e}"
    );
    let tail = format!(
        "; run dir: {}; incomplete runs beside it (left as they are): {older}",
        run.display()
    );
    assert!(e.ends_with(&tail), "{e}");
    let refused = std::fs::read_to_string(run.join("refused.txt")).unwrap();
    assert_eq!(refused, format!("{e}\n"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A sheet of the last run under the out dir, decoded.
fn sheet_of(out: &Path, name: &str) -> Rgba {
    let path = run_dir(out).join(name);
    let (w, h) = img::png_size(&std::fs::read(&path).unwrap()).unwrap();
    img::read_png(&path, w, h).unwrap()
}

#[test]
fn a_saved_sheet_shows_the_three_panels_and_marks_its_join() {
    // The test world's rows: fail-1.1-1.5 gen-gen has one row, the grass over the stone at
    // (0, 7), south.
    let g = game();
    let (dir, work, slices) = setup("saved", &world_rows());
    let out = dir.join("calibrate");
    let opts = Options {
        slices,
        out: out.clone(),
        per: 2,
        seed: 1,
        at: Vec::new(),
        seam: Thresholds::default().seam,
        source: Source::All,
    };
    calibrate(&g, &work, &opts).unwrap();
    let at = At { z: 0, x: 0, y: 7 };
    let pair = Pair {
        a: GRASS,
        b: STONE,
        edge: Edge::South,
    };
    let f = frame(&g, at, Edge::South, Some(pair));
    let pack = Rc::new(Slices::read(&g, &opts.slices).unwrap());
    for (s, name) in [
        (4, "fail-1.1-1.5-gen-gen.png"),
        (2, "fail-1.1-1.5-gen-gen-s2.png"),
    ] {
        let sheet = sheet_of(&out, name);
        let l = Layout::of(s);
        assert_eq!((sheet.width, sheet.height), (l.width(), l.height(1)));
        // The three panels as the compositor makes them for the window, each where it goes,
        // and each another picture: the pack's grass is not MMPX's.
        let want = panels(&g, &f.fb, &pack, scale_of(s));
        let crops: Vec<Rgba> = (0..3)
            .map(|k| {
                let (px, py) = l.panel_at(0, k);
                sheet.rect(px, py, l.w, l.h)
            })
            .collect();
        assert_eq!(crops, [want.classic, want.plain, want.hd], "{name}");
        assert!(crops[0] != crops[1] && crops[1] != crops[2], "{name}");
        // The south join marked: across it, left of each panel, on the two rows that meet; along
        // it, above the panel, A's columns only. Nothing marks an east join.
        let cell = TILE_W as u32 * s;
        let is_mark = |x: u32, y: u32| img::get(&sheet, x, y) == MARK;
        for k in 0..3 {
            let (px, py) = l.panel_at(0, k);
            let jy = py + 3 * cell;
            assert!(is_mark(px - 3, jy - 1) && is_mark(px - 3, jy), "{name} {k}");
            assert!(
                !is_mark(px - 3, jy + 1) && !is_mark(px - 3, jy - 2),
                "{name} {k}"
            );
            assert!(is_mark(px + 3 * cell, py - 4) && is_mark(px + 4 * cell - 1, py - 4));
            assert!(!is_mark(px + 4 * cell, py - 4) && !is_mark(px + 3 * cell - 1, py - 4));
            // An east join's tick (above, on the columns that meet) and bar (left, A's row).
            assert!(!is_mark(px + 4 * cell, py - 3), "{name} {k}");
            assert!(!is_mark(px - 4, py + 2 * cell), "{name} {k}");
        }
        let per_panel = 2 * (l.margin - 4) + 3 * cell;
        let marked = sheet.pixels.chunks_exact(4).filter(|p| *p == MARK).count();
        assert_eq!(marked as u32, 3 * per_panel, "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The compositor's scale of a sheet's `s`.
fn scale_of(s: u32) -> Scale {
    if s == 4 { Scale::X4 } else { Scale::X2 }
}

#[test]
fn samples_tsv_rows_are_the_samples() {
    // The test world's rows and one on the second level, at a cell whose x and y differ.
    let g = game();
    let below = line(
        1,
        (3, 3, "east"),
        true,
        Some(3.0),
        1.5,
        "gen-mmpx",
        "map",
        (1, 2, 5),
    );
    let (dir, work, slices) = setup("rows", &(world_rows() + &below + "\n"));
    let out = dir.join("calibrate");
    let opts = Options {
        slices,
        out: out.clone(),
        per: 2,
        seed: 1,
        at: Vec::new(),
        seam: Thresholds::default().seam,
        source: Source::All,
    };
    calibrate(&g, &work, &opts).unwrap();
    let (_, rows) = tsv(&run_dir(&out).join("samples.tsv"));
    let name = |r: u16| atlas::image_name(ImageId::Tile(r));
    let pack = Slices::read(&g, &opts.slices).unwrap();
    let pair = |a: u16, b: u16, edge: Edge| Pair { a, b, edge };
    // A whole row: index, bucket, sides, the pair, its ratio and limit, measured again, its
    // source, z, x, y, the sheet, its row there and its frames.
    #[allow(clippy::too_many_arguments)]
    let row = |index: usize,
               bucket: &str,
               sides: &str,
               p: Pair,
               ratio: &str,
               again: String,
               (z, x, y): (usize, usize, usize),
               panel: usize,
               frames: &str| {
        let stem = format!("{bucket}-{sides}");
        vec![
            index.to_string(),
            bucket.to_string(),
            sides.to_string(),
            name(p.a),
            name(p.b),
            edge_name(p.edge).to_string(),
            ratio.to_string(),
            "1.5000".to_string(),
            again,
            "map".to_string(),
            z.to_string(),
            x.to_string(),
            y.to_string(),
            format!("{stem}.png"),
            format!("{stem}-s2.png"),
            panel.to_string(),
            frames.to_string(),
        ]
    };
    let again = |p: Pair| format!("{:.4}", remeasured(&g, &pack, p, opts.seam).unwrap());
    let (stone_grass, grass_stone) = (
        pair(STONE, GRASS, Edge::East),
        pair(GRASS, STONE, Edge::East),
    );
    let water = pair(WATER_PAIR, GRASS, Edge::East);
    // In cell order, each sheet's rows by ratio / limit then pair: pass gen-mmpx (the stone
    // over the grass), fail-1.1-1.5 gen-gen (the grass over the stone), its gen-mmpx two (1.2,
    // 1.33), then fail-gt1.5 gen-mmpx: the pair below the surface (record 3 beside itself, which
    // the slices have no image of) before the water by pair order, their ratio / limit the same.
    // A window with a water other than the pair's (the level's first chunk has two, and the
    // level has it twice: (1, 1), (5, 1), (9, 9), (13, 9)) draws it at frame 0.
    let want = [
        row(
            1,
            "pass",
            "gen-mmpx",
            pair(STONE, GRASS, Edge::South),
            "1.0000",
            again(pair(STONE, GRASS, Edge::South)),
            (0, 8, 7),
            1,
            "frame0",
        ),
        row(
            2,
            "fail-1.1-1.5",
            "gen-gen",
            pair(GRASS, STONE, Edge::South),
            "2.0000",
            again(pair(GRASS, STONE, Edge::South)),
            (0, 0, 7),
            1,
            "frame0",
        ),
        row(
            3,
            "fail-1.1-1.5",
            "gen-mmpx",
            stone_grass,
            "1.8000",
            again(stone_grass),
            (0, 15, 0),
            1,
            "frame0",
        ),
        row(
            4,
            "fail-1.1-1.5",
            "gen-mmpx",
            grass_stone,
            "2.0000",
            again(grass_stone),
            (0, 7, 0),
            2,
            "frame0",
        ),
        row(
            5,
            "fail-gt1.5",
            "gen-mmpx",
            pair(3, 3, Edge::East),
            "3.0000",
            "-".into(),
            (1, 2, 5),
            1,
            "-",
        ),
        row(
            6,
            "fail-gt1.5",
            "gen-mmpx",
            water,
            "3.0000",
            again(water),
            (0, 1, 1),
            2,
            "frame0+pair",
        ),
    ];
    assert_eq!(rows, want);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sheet_orders_its_rows_by_ratio_over_limit_then_pair() {
    // One bucket × sides whose limits differ: ordered by ratio / limit, the order of neither the
    // ratios nor the pairs, and two of one ratio / limit (2.4) by pair.
    let row = |a: u16, ratio: f32, limit: f32| {
        line(
            1,
            (a, a + 1, "east"),
            true,
            Some(ratio),
            limit,
            "gen-mmpx",
            "map",
            (0, 0, 0),
        )
    };
    let fixture = [(1, 6.0, 3.0), (3, 4.0, 1.0), (5, 3.0, 1.25), (7, 4.8, 2.0)];
    let text: Vec<String> = fixture.iter().map(|&(a, r, l)| row(a, r, l)).collect();
    let (rows, _) = rows(&text.join("\n")).unwrap();
    let taken = sample(&rows, 4, 1, Source::All);
    let t = &taken[&(Bucket::Gt15, Sides::GenMmpx)];
    let order: Vec<u16> = t.rows.iter().map(|r| r.pair.a).collect();
    // By hand: 2.0 (1), 2.4 (5), 2.4 (7), 4.0 (3); by ratio alone 5, 3, 7, 1.
    assert_eq!(order, [1, 5, 7, 3]);
}

#[test]
fn a_master_is_box_averaged_to_x4() {
    // A master whose 2 × 2 blocks are not uniform: the ×4 image is each block's average,
    // weighted by alpha (`pack`'s reduction), worked out here by hand.
    let g = game();
    let dir = std::env::temp_dir().join(format!("u6-hdtool-calibrate-{}-box", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let blocks: [[[u8; 4]; 4]; 3] = [
        // Opaque: the plain mean, rounded.
        [
            [0, 0, 0, 255],
            [100, 40, 8, 255],
            [20, 200, 60, 255],
            [40, 1, 4, 255],
        ],
        // Two clear subpixels: the opaque two's colour, half the alpha.
        [
            [255, 0, 0, 255],
            [0, 0, 255, 0],
            [0, 255, 0, 255],
            [9, 9, 9, 0],
        ],
        // Part-transparent: weighted by alpha.
        [
            [200, 0, 0, 200],
            [0, 100, 0, 100],
            [0, 0, 50, 50],
            [0, 0, 0, 0],
        ],
    ];
    let want: [[u8; 4]; 3] = [[40, 60, 18, 255], [128, 128, 0, 128], [114, 29, 7, 88]];
    let mut m = Rgba::new(128, 128);
    for y in 0..64 {
        for x in 0..64 {
            let b = &blocks[((x + y) % 3) as usize];
            for (k, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                img::put(&mut m, 2 * x + dx, 2 * y + dy, b[k]);
            }
        }
    }
    let stem = atlas::slice_name(ImageId::Tile(GRASS));
    img::write_png(&dir.join(format!("{stem}.png")), &m).unwrap();
    let s = Slices::read(&g, &dir).unwrap();
    let x4 = s.get(GRASS).unwrap();
    assert_eq!((x4.width, x4.height), (64, 64));
    for y in 0..64 {
        for x in 0..64 {
            assert_eq!(
                img::get(x4, x, y),
                want[((x + y) % 3) as usize],
                "({x}, {y})"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

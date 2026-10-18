//! The neighbour gate on synthetic records and synthetic worlds only: images drawn by the tests
//! (textures of made-up colours), the synthetic game's records and worlds placed by the tests.

use std::rc::Rc;

use u6_hdpack::ImageId;
use u6_render::hd::{Compositor, Image, Images, Pack, PackHd};
use u6_render::{Framebuffer, Kind, Rgb, Src, WIDTH};

use super::*;
use crate::atlas::{self, Job};
use crate::game::{Level, Placed, Record, World, synthetic};
use crate::gate::Thresholds;
use crate::group::{self, Class};
use crate::img;
use crate::ingest::{self, Status};
use crate::run;
use crate::work::Work;

const SIDE: u32 = 16;

fn hash(i: u32, seed: u32) -> u32 {
    let mut h = i.wrapping_mul(0x9e37_79b9) ^ seed.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 12)
}

/// A made-up opaque texture: `base` with a grain of up to ±24 levels.
fn texture(seed: u32, base: [u8; 3]) -> Classic {
    Classic {
        width: SIDE,
        height: SIDE,
        px: (0..SIDE * SIDE)
            .map(|i| {
                let off = (hash(i, seed) % 5) as i32 * 12 - 24;
                Some(base.map(|c| (i32::from(c) + off).clamp(0, 255) as u8))
            })
            .collect(),
    }
}

/// A "generated" image that is the classic one at ×4, nearest.
fn near(c: &Classic) -> Rgba {
    c.scaled(K, [0, 0, 0])
}

/// The image with its classic pixels along one side (one classic pixel deep) painted `colour`:
/// the side toward a neighbour on `edge` (`far` false), or the opposite side (`far` true).
fn band(img: &Rgba, edge: Edge, far: bool, colour: [u8; 3]) -> Rgba {
    let mut out = img.clone();
    let (w, h) = (img.width, img.height);
    for y in 0..h {
        for x in 0..w {
            let inside = match (edge, far) {
                (Edge::East, false) => x >= w - K,
                (Edge::East, true) => x < K,
                (Edge::South, false) => y >= h - K,
                (Edge::South, true) => y < K,
            };
            if inside {
                img::put(&mut out, x, y, [colour[0], colour[1], colour[2], 255]);
            }
        }
    }
    out
}

const GREEN: [u8; 3] = [60, 120, 50];
const SAND: [u8; 3] = [210, 190, 130];
const WHITE: [u8; 3] = [250, 250, 250];
const RED: [u8; 3] = [200, 30, 30];
const BLACK: [u8; 3] = [0, 0, 0];

fn pair(a: u16, b: u16, edge: Edge) -> Pair {
    Pair { a, b, edge }
}

/// A record, its classic image and, when generated, its item and image.
type Made = (u16, Classic, Option<(u32, Rgba)>);

/// Sides from what the test made.
fn sides(list: Vec<Made>) -> BTreeMap<u16, Side> {
    list.into_iter()
        .map(|(r, c, g)| (r, Side::new(c, g.as_ref().map(|(i, h)| (*i, h)))))
        .collect()
}

fn seam() -> f32 {
    Thresholds::default().seam
}

#[test]
fn two_generated_records_that_meet_cleanly_are_kept() {
    let (a, b) = (texture(1, GREEN), texture(2, GREEN));
    let s = sides(vec![
        (1, a.clone(), Some((10, near(&a)))),
        (2, b.clone(), Some((20, near(&b)))),
    ]);
    let pairs = [pair(1, 2, Edge::East), pair(1, 2, Edge::South)];
    let out = settle(&pairs, &s, seam());
    assert!(out.fallen.is_empty(), "{:?}", out.failing);
    assert_eq!(
        (
            out.counts.checked,
            out.counts.failing_before,
            out.counts.passes
        ),
        (2, 0, 1)
    );
}

#[test]
fn a_generated_record_whose_edge_differs_from_its_scaled_neighbour_falls_back() {
    let (a, b) = (texture(3, GREEN), texture(4, GREEN));
    for edge in [Edge::East, Edge::South] {
        // A's side toward B repainted white; B is drawn by MMPX.
        let s = sides(vec![
            (
                1,
                a.clone(),
                Some((10, band(&near(&a), edge, false, WHITE))),
            ),
            (2, b.clone(), None),
        ]);
        let out = settle(&[pair(1, 2, edge)], &s, seam());
        assert_eq!(out.fallen, BTreeMap::from([(10, 1)]), "{edge:?}");
        let c = &out.counts;
        assert_eq!((c.failing_before, c.failing_after, c.passes), (1, 0, 2));
        assert_eq!((c.checked, c.checked_after), (1, 0));
        // The same image with its band on the far side meets B cleanly.
        let s = sides(vec![
            (1, a.clone(), Some((10, band(&near(&a), edge, true, WHITE)))),
            (2, b.clone(), None),
        ]);
        assert!(settle(&[pair(1, 2, edge)], &s, seam()).fallen.is_empty());
    }
}

/// A–B–C, east: A and B generated with a matching white band where they meet, so they meet
/// cleanly; B's east side red against C, which MMPX draws. B falls in the first pass; A then
/// meets B's MMPX image with its white band and falls in the second.
fn chain() -> (Vec<Pair>, BTreeMap<u16, Side>) {
    let (a, b, c) = (texture(5, SAND), texture(6, SAND), texture(7, SAND));
    let a_hd = band(&near(&a), Edge::East, false, WHITE);
    let b_hd = band(
        &band(&near(&b), Edge::East, true, WHITE),
        Edge::East,
        false,
        RED,
    );
    let s = sides(vec![
        (1, a, Some((10, a_hd))),
        (2, b, Some((20, b_hd))),
        (3, c, None),
    ]);
    (vec![pair(1, 2, Edge::East), pair(2, 3, Edge::East)], s)
}

#[test]
fn a_fallback_that_makes_its_neighbour_fail_takes_it_too() {
    let (pairs, s) = chain();
    let out = settle(&pairs, &s, seam());
    assert_eq!(out.fallen, BTreeMap::from([(20, 1), (10, 2)]));
    let c = &out.counts;
    assert_eq!((c.checked, c.failing_before), (2, 1));
    assert_eq!((c.failing_after, c.passes), (0, 3));
    // While B is generated, A and B meet cleanly: only the pair with C fails at first.
    assert_eq!(
        out.failing.iter().map(|f| (f.0, f.1)).collect::<Vec<_>>(),
        [(1, pair(2, 3, Edge::East)), (2, pair(1, 2, Edge::East))]
    );
}

#[test]
fn the_result_does_not_depend_on_the_pair_order() {
    let (mut pairs, mut s) = chain();
    // More records and pairs around the chain: a clean record two pairs touch, a failing
    // pair apart from the chain.
    let (d, e, f) = (texture(8, SAND), texture(9, GREEN), texture(10, GREEN));
    s.extend(sides(vec![
        (4, d.clone(), Some((40, near(&d)))),
        (
            5,
            e.clone(),
            Some((50, band(&near(&e), Edge::South, false, RED))),
        ),
        (6, f, None),
    ]));
    pairs.extend([
        pair(4, 1, Edge::South),
        pair(5, 6, Edge::South),
        pair(3, 4, Edge::East),
    ]);
    let first = settle(&pairs, &s, seam());
    assert!(first.fallen.contains_key(&50) && first.fallen.contains_key(&10));
    assert!(!first.fallen.contains_key(&40));
    for k in 1..pairs.len() {
        let mut turned = pairs.clone();
        turned.rotate_left(k);
        let out = settle(&turned, &s, seam());
        assert_eq!((&out.fallen, &out.counts), (&first.fallen, &first.counts));
    }
    pairs.reverse();
    let out = settle(&pairs, &s, seam());
    assert_eq!((&out.fallen, &out.counts), (&first.fallen, &first.counts));
}

#[test]
fn a_classic_pair_that_already_steps_is_not_held_to_more() {
    // Green beside sand: the original steps across their edge. Both drawn as they are (×4,
    // nearest) step as much as the original, which is more than the threshold.
    let (a, b) = (texture(11, GREEN), texture(12, SAND));
    let (ratio, limit) = gate::neighbour_seam(
        (&a, &gate::shown(&a, &near(&a))),
        (&b, &gate::shown(&b, &near(&b))),
        K,
        Edge::East,
        seam(),
    )
    .unwrap();
    assert!(ratio > seam(), "the step is above the threshold: {ratio}");
    assert!(
        limit > seam() && gate::seam_passes((ratio, limit)),
        "{ratio} / {limit}"
    );
    let s = sides(vec![
        (1, a.clone(), Some((10, near(&a)))),
        (2, b.clone(), Some((20, near(&b)))),
    ]);
    assert!(
        settle(&[pair(1, 2, Edge::East)], &s, seam())
            .fallen
            .is_empty()
    );
    // A step beyond the original's (a black edge against sand) is still a seam.
    let s = sides(vec![
        (
            1,
            a.clone(),
            Some((10, band(&near(&a), Edge::East, false, BLACK))),
        ),
        (2, b, None),
    ]);
    assert_eq!(
        settle(&[pair(1, 2, Edge::East)], &s, seam()).fallen,
        BTreeMap::from([(10, 1)])
    );
}

#[test]
fn shore_and_empty_neighbours_are_mmpx_or_nothing() {
    let a = texture(14, GREEN);
    let a_hd = band(&near(&a), Edge::East, false, WHITE);
    // The shore record is in no item: MMPX draws it, and a generated record that does not meet
    // its MMPX image falls back.
    let shore = texture(15, GREEN);
    let s = sides(vec![
        (1, a.clone(), Some((10, a_hd.clone()))),
        (11, shore, None),
    ]);
    assert_eq!(
        settle(&[pair(1, 11, Edge::East)], &s, seam()).fallen,
        BTreeMap::from([(10, 1)])
    );
    // An empty record draws nothing: there is no edge to step across; the pair is checked (it
    // has a generated side) and passes.
    let empty = Classic {
        width: SIDE,
        height: SIDE,
        px: vec![None; (SIDE * SIDE) as usize],
    };
    let s = sides(vec![(1, a, Some((10, a_hd))), (12, empty, None)]);
    let out = settle(&[pair(1, 12, Edge::East)], &s, seam());
    assert!(out.fallen.is_empty());
    assert_eq!((out.counts.checked, out.counts.failing_before), (1, 0));
}

/// A chunk of one map tile everywhere but the cells given.
fn chunk(fill: u8, cells: &[(usize, usize, u8)]) -> [[u8; 8]; 8] {
    let mut c = [[fill; 8]; 8];
    for &(x, y, t) in cells {
        c[y][x] = t;
    }
    c
}

fn has(p: &Pairs, a: u16, b: u16, edge: Edge) -> bool {
    p.all.contains(&pair(a, b, edge))
}

#[test]
fn the_map_pairs_cross_chunk_edges_and_the_wrap_and_take_every_frame() {
    let mut g = synthetic::game();
    let (shore, empty) = (synthetic::SHORE, 12);
    assert!(g.records[usize::from(empty)].is_empty());
    g.world = World {
        chunks: vec![
            chunk(0, &[]),
            chunk(2, &[]),
            // Placed nowhere: its tile meets nothing.
            chunk(5, &[]),
            // The water tile (animated) in a corner, the shore and an empty record.
            chunk(3, &[(7, 7, 1), (0, 0, empty as u8), (1, 0, shore as u8)]),
        ],
        levels: vec![
            // 2 x 2 chunks: grass and stone, alternating.
            Level {
                side: 2,
                refs: vec![0, 1, 1, 0],
            },
            Level {
                side: 1,
                refs: vec![3],
            },
        ],
        objects: Vec::new(),
    };
    let p = pairs(&g);
    assert_eq!((p.map, p.objects), (p.all.len(), 0));
    // Grass meets stone only across chunk edges (and the wrap).
    for edge in [Edge::East, Edge::South] {
        assert!(has(&p, 0, 2, edge) && has(&p, 2, 0, edge), "{edge:?}");
        assert!(has(&p, 0, 0, edge) && has(&p, 2, 2, edge), "{edge:?}");
    }
    assert!(p.all.iter().all(|q| q.a != 5 && q.b != 5));
    // The water tile is each of its frames, never itself; its corner meets the next chunk
    // across the level's wrap.
    assert!(p.all.iter().all(|q| q.a != 1 && q.b != 1));
    for f in synthetic::WATER_FRAMES..synthetic::WATER_FRAMES + 4 {
        for edge in [Edge::East, Edge::South] {
            assert!(has(&p, f, 3, edge) && has(&p, 3, f, edge), "{f} {edge:?}");
        }
    }
    // The shore and the empty record count as any other.
    assert!(has(&p, empty, shore, Edge::East));
    assert!(has(&p, 3, empty, Edge::East) && has(&p, shore, 3, Edge::East));
    assert!(has(&p, empty, 3, Edge::South) && has(&p, 3, shore, Edge::South));
    // Every one once.
    let mut all = p.all.clone();
    all.dedup();
    assert_eq!(all.len(), p.all.len());
}

/// An object record: opaque, but only `west` opaque pixels in its west column.
fn object(west: usize) -> Record {
    let mut transparent = vec![false; 256];
    for y in west..16 {
        transparent[y * 16] = true;
    }
    Record {
        pixels: (0..256).map(|i| 13 + (hash(i, 99) % 2) as u8).collect(),
        transparent,
        map: false,
    }
}

#[test]
fn object_pairs_follow_the_object_draw_and_the_half_opaque_rule() {
    let mut g = synthetic::game();
    let full = object(16);
    for t in [300, 310, 311, 320, 321, 322, 323, 330, 331] {
        g.records[t] = full.clone();
    }
    // West columns opaque on 7 and on 8 of their 16 pixels.
    g.records[301] = object(7);
    g.records[302] = object(8);
    g.flags2[311] = DOUBLE_WIDTH;
    g.flags2[323] = DOUBLE_WIDTH | DOUBLE_HEIGHT;
    g.flags2[331] = DOUBLE_HEIGHT;
    let at = |x, y, tile| Placed { x, y, z: 0, tile };
    g.world = World {
        chunks: vec![chunk(0, &[])],
        levels: vec![Level {
            side: 2,
            refs: vec![0; 4],
        }],
        objects: vec![
            at(0, 0, 300),
            at(1, 0, 301),
            at(0, 2, 300),
            at(1, 2, 302),
            // A 2 x 1, a 2 x 2 and a 1 x 2 object at their anchors.
            at(5, 5, 311),
            at(9, 7, 323),
            at(12, 3, 331),
            // An object off the map's levels is not drawn.
            Placed {
                x: 0,
                y: 0,
                z: 9,
                tile: 300,
            },
        ],
    };
    let p = pairs(&g);
    assert_eq!(p.map, 2);
    assert!(has(&p, 300, 302, Edge::East));
    assert!(!has(&p, 300, 301, Edge::East));
    // The parts where the object draw puts them: − 1 west, − 2 north and − 3 north-west, or
    // − 1 north.
    assert!(has(&p, 310, 311, Edge::East));
    assert!(has(&p, 320, 321, Edge::East) && has(&p, 322, 323, Edge::East));
    assert!(has(&p, 320, 322, Edge::South) && has(&p, 321, 323, Edge::South));
    assert!(has(&p, 330, 331, Edge::South));
    assert_eq!(p.objects, 7, "{:?}", p.all);
}

/// One tile record for the compositor.
struct One {
    record: Record,
}

impl Images for One {
    fn image(&mut self, kind: Kind, _: u16) -> Option<Image<'_>> {
        (kind == Kind::Tile).then_some(Image {
            ver: 0,
            width: 16,
            height: 16,
            pixels: &self.record.pixels,
            transparent: Some(&self.record.transparent),
        })
    }

    fn key(&mut self, _: Kind, _: u16) -> Option<u64> {
        Some(7)
    }
}

struct OnePack(Rgba);

impl Pack for OnePack {
    fn hd(&self, kind: Kind, _: u16, key: u64) -> Option<PackHd<'_>> {
        (kind == Kind::Tile && key == 7).then_some(PackHd {
            index: 0,
            width: 64,
            height: 64,
            rgba: &self.0.pixels,
        })
    }
}

#[test]
fn the_images_measured_are_the_ones_the_page_composes() {
    // A figure (transparent around it) drawn at (40, 24) over a fill, through the plane.
    let record = synthetic::figure(1, 0, 10);
    let classic = Classic::from_record(&record);
    let mut fb = Framebuffer::new();
    fb.enable_src();
    for (i, c) in synthetic::PALETTE.iter().enumerate() {
        fb.palette[i] = Rgb::new(c[0], c[1], c[2]);
    }
    fb.clear(5);
    let (x0, y0) = (40, 24);
    for v in 0..16 {
        for u in 0..16 {
            let i = v * 16 + u;
            if !record.transparent[i] {
                let src = Src {
                    ver: 0,
                    id: 200,
                    u: u as u16,
                    v: v as u16,
                    kind: Kind::Tile,
                    flags: 0,
                };
                fb.set_src(x0 + u, y0 + v, record.pixels[i], src);
            }
        }
    }
    let mut images = One { record };
    let check = |c: &Compositor, want: &[Option<[f32; 3]>]| {
        let out = c.rgba();
        let ow = WIDTH * 4;
        let mut n = 0;
        for y in 0..64usize {
            for x in 0..64usize {
                let Some(w) = want[y * 64 + x] else { continue };
                let p = out[(y0 * 4 + y) * ow + x0 * 4 + x].to_le_bytes();
                assert_eq!([p[0], p[1], p[2]].map(f32::from), w, "({x}, {y})");
                n += 1;
            }
        }
        assert!(n > 0);
    };
    // No pack: MMPX x4 of the record.
    let mut c = Compositor::new();
    c.compose(&fb, &mut images, Scale::X4);
    check(&c, &gate::shown(&classic, &mmpx4(&classic)));
    // A pack image, opaque, clear and in between, over the classic colours.
    let mut generated = Rgba::new(64, 64);
    for y in 0..64 {
        for x in 0..64 {
            let a = [255, 0, 128, 40][((x + y) % 4) as usize];
            img::put(&mut generated, x, y, [(x * 4) as u8, (y * 4) as u8, 90, a]);
        }
    }
    c.set_pack(Some(Rc::new(OnePack(generated.clone()))));
    c.compose(&fb, &mut images, Scale::X4);
    check(&c, &gate::shown(&classic, &generated));
}

// Ingest end to end on the synthetic set, in a synthetic world.

fn work(name: &str) -> Work {
    let dir =
        std::env::temp_dir().join(format!("u6-hdtool-neighbour-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let w = Work::open(Some(&dir)).unwrap();
    w.mkdirs().unwrap();
    w
}

fn atlas_into(g: &Game, w: &Work) -> Vec<Job> {
    let plan = group::plan(g);
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
    atlas::layout_each(g, &items, &stamp, 0, |job, sheet| {
        img::write_png(&w.sheet(&job.sheet), &sheet)?;
        jobs.push(job);
        Ok(())
    })
    .unwrap();
    w.append_jobs(&jobs).unwrap();
    jobs
}

#[test]
fn ingest_sends_a_multi_record_item_back_whole_and_counts_it_apart() {
    let mut g = synthetic::game();
    // The table (2 x 1) and the sword made opaque: a brown grain of two palette colours.
    let grain = |seed: u32| Record {
        pixels: (0..256).map(|i| 13 + (hash(i, seed) % 2) as u8).collect(),
        transparent: vec![false; 256],
        map: false,
    };
    let (table, sword) = (synthetic::TABLE, synthetic::OBJECTS + 2);
    for (k, t) in [table, table + 1, sword].into_iter().enumerate() {
        g.records[usize::from(t)] = grain(k as u32 + 1);
    }
    // The table's anchor with the sword east of it, on a grass chunk.
    let at = |x, tile| Placed {
        x,
        y: 3,
        z: 0,
        tile,
    };
    g.world = World {
        chunks: vec![chunk(0, &[])],
        levels: vec![Level {
            side: 1,
            refs: vec![0],
        }],
        objects: vec![at(3, table + 1), at(4, sword)],
    };
    let w = work("multi");
    let jobs = atlas_into(&g, &w);
    // Every sheet comes back as sent, but the table's anchor with its east column white.
    let anchor = atlas::image_name(ImageId::Tile(table + 1));
    for job in &jobs {
        let mut back = img::read_png(&w.sheet(&job.sheet), job.width, job.height).unwrap();
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
        run::record(&w, job, &img::encode_png(&back).unwrap()).unwrap();
    }
    // The painted cell's own gates are not what this tests: its structure is let pass.
    let mut t = Thresholds::default();
    t.set("structure=-1").unwrap();
    let r = ingest::ingest(&g, &w, &t).unwrap();
    // The pair (anchor, sword) fails: both generated sides fall, the table with both records.
    assert_eq!(r.get(Class::Multi, Status::Neighbour), 2);
    assert_eq!(r.get(Class::Object, Status::Neighbour), 1);
    assert_eq!(r.get(Class::Multi, Status::Generated), 6);
    assert_eq!(r.get(Class::Terrain, Status::Neighbour), 0);
    let n = &r.neighbour;
    assert_eq!((n.map, n.objects, n.pairs), (2, 2, 4));
    assert_eq!((n.failing_before, n.failing_after, n.passes), (1, 0, 2));
    let text = r.text();
    assert!(text.contains("neighbour pairs failing: 1 before the fixed point, 0 after (2 passes)"));
    assert!(text.contains(
        "neighbour pairs (distinct records in edge-adjacent cells): 4 (map 2, objects 2)"
    ));
    assert_eq!(
        std::fs::read_to_string(w.path("coverage.txt")).unwrap(),
        text
    );
    // Neither table record nor the sword reaches the pack's list or the slices.
    let accepted: Vec<ingest::Accepted> =
        serde_json::from_slice(&std::fs::read(w.path("ingest.json")).unwrap()).unwrap();
    for t in [table, table + 1, sword] {
        let id = ImageId::Tile(t);
        assert!(accepted.iter().all(|a| a.image != atlas::image_name(id)));
        assert!(!w.slice(&atlas::slice_name(id)).exists());
    }
    let tree = atlas::image_name(ImageId::Tile(synthetic::TREE));
    assert!(accepted.iter().any(|a| a.image == tree));
    // gates.jsonl: the failing pair, and each fallback with its reason.
    let log = std::fs::read_to_string(w.path("gates.jsonl")).unwrap();
    let lines: Vec<serde_json::Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let fell: Vec<&serde_json::Value> = lines
        .iter()
        .filter(|l| l["fallback"] == "neighbour")
        .collect();
    assert_eq!(fell.len(), 2);
    assert!(fell.iter().any(|l| l["class"] == "multi"));
    assert!(fell.iter().any(|l| l["class"] == "object"));
    let failing: Vec<&serde_json::Value> = lines
        .iter()
        .filter(|l| l["neighbour"].is_object())
        .collect();
    assert_eq!(failing.len(), 1);
    assert_eq!(failing[0]["neighbour"]["edge"], "east");
    assert_eq!(failing[0]["neighbour"]["a"], anchor.as_str());
    // A second ingest of the same sheets comes to the same.
    assert_eq!(ingest::ingest(&g, &w, &t).unwrap(), r);
    let _ = std::fs::remove_dir_all(&w.root);
}

#[test]
fn ingest_and_the_gate_call_nothing() {
    // Neither starts a process, `codex` or any other.
    for (name, src) in [
        ("ingest.rs", include_str!("../ingest.rs")),
        ("neighbour.rs", include_str!("../neighbour.rs")),
    ] {
        for word in ["Command", "Codex", "run::run", "process::", "spawn"] {
            assert!(!src.contains(word), "{name} names {word}");
        }
    }
}

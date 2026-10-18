//! The neighbour gate on synthetic records and synthetic worlds only: images drawn by the tests
//! (textures of made-up colours), the synthetic game's records and worlds placed by the tests.

use std::rc::Rc;

use u6_hdpack::{ImageId, read_pack};
use u6_render::hd::{Compositor, Image, Images, Pack, PackHd};
use u6_render::{Framebuffer, Kind, Rgb, Src, WIDTH};

use super::*;
use crate::atlas::{self, Job};
use u6_formats::animdata::AnimEntry;

use crate::game::{Level, Placed, Record, World, synthetic};
use crate::gate::Thresholds;
use crate::group::{self, Class};
use crate::img;
use crate::ingest::{self, Status};
use crate::work::Work;
use crate::{pack, run};

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
        .map(|(r, c, g)| (r, Side::new(c, g.as_ref().map(|(i, h)| (*i, h))).unwrap()))
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
    assert!(out.fallen.is_empty(), "{:?}", out.checked);
    // Each checked pair once, passing, measured, both sides generated.
    assert_eq!(out.checked.len(), 2);
    assert!(
        out.checked
            .iter()
            .all(|c| c.pass == 1 && c.both && !c.fails && c.result.is_some())
    );
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
        out.failing().map(|c| (c.pass, c.pair)).collect::<Vec<_>>(),
        [(1, pair(2, 3, Edge::East)), (2, pair(1, 2, Edge::East))]
    );
    // The first pass has every pair checked, failing or not; a later pass its failing ones.
    let rows: Vec<(usize, Pair, bool, bool)> = out
        .checked
        .iter()
        .map(|c| (c.pass, c.pair, c.both, c.fails))
        .collect();
    assert_eq!(
        rows,
        [
            (1, pair(1, 2, Edge::East), true, false),
            (1, pair(2, 3, Edge::East), false, true),
            (2, pair(1, 2, Edge::East), false, true),
        ]
    );
}

#[test]
fn an_item_falls_back_whole_and_its_other_records_bring_their_neighbours() {
    // Item 30 is records 1 and 2; item 40 is record 3. Record 1's east side is red against
    // record 4, which MMPX draws: it fails. Records 2 and 3 share a white band where they meet,
    // so they meet cleanly while both are generated; record 3's band against record 2 drawn
    // by MMPX is a seam.
    let (r1, r2, r3, r4) = (
        texture(20, SAND),
        texture(21, SAND),
        texture(22, SAND),
        texture(23, SAND),
    );
    let s = sides(vec![
        (
            1,
            r1.clone(),
            Some((30, band(&near(&r1), Edge::East, false, RED))),
        ),
        (
            2,
            r2.clone(),
            Some((30, band(&near(&r2), Edge::East, false, WHITE))),
        ),
        (
            3,
            r3.clone(),
            Some((40, band(&near(&r3), Edge::East, true, WHITE))),
        ),
        (4, r4, None),
    ]);
    let pairs = [pair(1, 4, Edge::East), pair(2, 3, Edge::East)];
    let out = settle(&pairs, &s, seam());
    // Record 1 takes record 2 with it (its whole item), and record 2 drawn by MMPX then takes
    // item 40 in the second pass; falling back record by record would have kept both.
    assert_eq!(out.fallen, BTreeMap::from([(30, 1), (40, 2)]));
    let rows: Vec<(usize, Pair, bool, bool)> = out
        .checked
        .iter()
        .map(|c| (c.pass, c.pair, c.both, c.fails))
        .collect();
    assert_eq!(
        rows,
        [
            (1, pair(1, 4, Edge::East), false, true),
            (1, pair(2, 3, Edge::East), true, false),
            (2, pair(2, 3, Edge::East), false, true),
        ]
    );
    let c = &out.counts;
    assert_eq!((c.checked, c.failing_before), (2, 1));
    assert_eq!((c.checked_after, c.failing_after, c.passes), (0, 0, 3));
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
        (&a, &gate::shown(&a, &near(&a)).unwrap()),
        (&b, &gate::shown(&b, &near(&b)).unwrap()),
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
fn the_image_helpers_refuse_images_that_do_not_fit() {
    let c = texture(30, GREEN);
    let ok = near(&c);
    assert!(gate::shown(&c, &ok).is_some());
    // No classic pixel: no scale to take, nothing to scale.
    let none = Classic {
        width: 0,
        height: 0,
        px: Vec::new(),
    };
    assert_eq!(gate::shown(&none, &ok), None);
    assert_eq!(mmpx4(&none), None);
    // An HD image narrower than the classic one (k = 0), not a whole multiple of it, or of
    // another height.
    assert_eq!(gate::shown(&c, &Rgba::new(8, 8)), None);
    assert_eq!(gate::shown(&c, &Rgba::new(63, 64)), None);
    assert_eq!(gate::shown(&c, &Rgba::new(64, 32)), None);
    // Buffers shorter than their sizes.
    let mut short = ok.clone();
    short.pixels.pop();
    assert_eq!(gate::shown(&c, &short), None);
    let mut few = c.clone();
    few.px.pop();
    assert_eq!(gate::shown(&few, &ok), None);
    assert_eq!(mmpx4(&few), None);
    // Larger than the screen: the compositor does not scale it.
    let wide = Classic {
        width: 321,
        height: 1,
        px: vec![Some([1, 2, 3]); 321],
    };
    assert_eq!(mmpx4(&wide), None);
    let m = mmpx4(&c).unwrap();
    assert_eq!((m.width, m.height, m.pixels.len()), (64, 64, 64 * 64 * 4));
    // The measure: k = 0, classic images of other sizes or short, shown images short.
    let px = gate::shown(&c, &ok).unwrap();
    let s = seam();
    for edge in [Edge::East, Edge::South] {
        assert!(gate::neighbour_seam((&c, &px), (&c, &px), K, edge, s).is_some());
        assert_eq!(gate::neighbour_seam((&c, &px), (&c, &px), 0, edge, s), None);
        assert_eq!(
            gate::neighbour_seam((&c, &px[1..]), (&c, &px), K, edge, s),
            None
        );
        assert_eq!(
            gate::neighbour_seam((&c, &px), (&c, &px[1..]), K, edge, s),
            None
        );
        assert_eq!(
            gate::neighbour_seam((&few, &px), (&c, &px), K, edge, s),
            None
        );
        let small = Classic {
            width: 8,
            height: 8,
            px: vec![Some([1, 2, 3]); 64],
        };
        let small_px = gate::shown(&small, &small.scaled(K, [0, 0, 0])).unwrap();
        assert_eq!(
            gate::neighbour_seam((&c, &px), (&small, &small_px), K, edge, s),
            None
        );
    }
    // A side: a generated image not x4 of its classic image is refused, as is a classic image
    // the scaler does not take.
    assert!(Side::new(c.clone(), Some((1, &ok))).is_ok());
    assert!(Side::new(c.clone(), Some((1, &c.scaled(8, [0, 0, 0])))).is_err());
    assert!(Side::new(few, None).is_err());
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
    // has a generated side), unmeasured, and passes.
    let empty = Classic {
        width: SIDE,
        height: SIDE,
        px: vec![None; (SIDE * SIDE) as usize],
    };
    let s = sides(vec![(1, a, Some((10, a_hd))), (12, empty, None)]);
    let out = settle(&[pair(1, 12, Edge::East)], &s, seam());
    assert!(out.fallen.is_empty());
    let n = &out.counts;
    assert_eq!((n.checked, n.unmeasured, n.failing_before), (1, 1, 0));
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
    // Each has where it first occurs, A's cell in level, row, column order: grass meets stone
    // first at the first chunk edge, stone meets grass first across the wrap of the top row,
    // and the water's frames meet stone across the second level's wrap.
    let at = |a: u16, b: u16, edge| p.seen[&pair(a, b, edge)];
    let cell = |z, x, y| Some(At { z, x, y });
    assert_eq!(at(0, 2, Edge::East).map, cell(0, 7, 0));
    assert_eq!(at(2, 0, Edge::East).map, cell(0, 15, 0));
    assert_eq!(at(0, 2, Edge::South).map, cell(0, 0, 7));
    let water = at(synthetic::WATER_FRAMES, 3, Edge::East);
    assert_eq!((water.map, water.objects), (cell(1, 7, 7), None));
    assert_eq!((water.source(), water.example()), ("map", cell(1, 7, 7)));
    assert_eq!(p.seen.len(), p.all.len());
}

#[test]
fn two_animated_tiles_side_by_side_meet_in_every_pair_of_their_frames() {
    let mut g = synthetic::game();
    // Beside the water (tile 1, frames 64..=67): tile 2 shows frame 80 or 81, and frame 81 is
    // replaced in turn by 90 or 91; tile 4's frames are all at or past 0x200, which the
    // substitution skips, so it draws itself.
    let slot = |tile_to_animate, first_anim_frame| AnimEntry {
        tile_to_animate,
        first_anim_frame,
        and_mask: 1,
        shift: 0,
    };
    g.anim.extend([slot(2, 80), slot(81, 90), slot(4, 0x200)]);
    // The water west of tile 2, tile 4 south of tile 2, on grass.
    g.world = World {
        chunks: vec![chunk(0, &[(3, 3, 1), (4, 3, 2), (4, 4, 4)])],
        levels: vec![Level {
            side: 1,
            refs: vec![0],
        }],
        objects: Vec::new(),
    };
    let p = pairs(&g);
    let water: Vec<u16> = (synthetic::WATER_FRAMES..synthetic::WATER_FRAMES + 4).collect();
    let two = [80, 90, 91];
    // Every frame of one beside every frame of the other, and nothing more between them.
    let east: BTreeSet<(u16, u16)> = p
        .all
        .iter()
        .filter(|q| q.edge == Edge::East && water.contains(&q.a) && two.contains(&q.b))
        .map(|q| (q.a, q.b))
        .collect();
    let want: BTreeSet<(u16, u16)> = water
        .iter()
        .flat_map(|&f| two.iter().map(move |&t| (f, t)))
        .collect();
    assert_eq!(east, want);
    let south: BTreeSet<u16> = p
        .all
        .iter()
        .filter(|q| q.edge == Edge::South && q.b == 4)
        .map(|q| q.a)
        .collect();
    assert_eq!(south, BTreeSet::from([80, 90, 91]));
    // No animated tile is drawn as itself, nor the frame replaced in turn.
    for t in [1, 2, 81] {
        assert!(p.all.iter().all(|q| q.a != t && q.b != t), "{t}");
    }
    // Each frame also meets the grass around it.
    for f in water.iter().chain(&two) {
        assert!(
            has(&p, 0, *f, Edge::East) || has(&p, 0, *f, Edge::South),
            "{f}"
        );
    }
}

#[test]
fn a_map_tile_is_read_only_inside_its_level() {
    let world = World {
        chunks: vec![
            chunk(0, &[]),
            chunk(1, &[]),
            chunk(2, &[]),
            chunk(3, &[(7, 7, 9)]),
        ],
        levels: vec![
            // 2 x 2 chunks: 16 x 16 tiles.
            Level {
                side: 2,
                refs: vec![0, 1, 2, 3],
            },
            // Shorter than its side.
            Level {
                side: 1,
                refs: Vec::new(),
            },
            // A chunk the chunks do not have.
            Level {
                side: 1,
                refs: vec![4],
            },
        ],
        objects: Vec::new(),
    };
    assert_eq!(world.tile(0, 0, 0), Some(0));
    assert_eq!(world.tile(0, 8, 0), Some(1));
    assert_eq!(world.tile(0, 0, 8), Some(2));
    assert_eq!(world.tile(0, 15, 15), Some(9));
    // At or past the side: nothing (x at the side would otherwise name the next chunk row).
    assert_eq!(world.tile(0, 16, 0), None);
    assert_eq!(world.tile(0, 0, 16), None);
    assert_eq!(world.tile(0, 16, 16), None);
    assert_eq!(world.tile(0, usize::MAX, 0), None);
    for z in [1, 2, 3] {
        assert_eq!(world.tile(z, 0, 0), None, "level {z}");
    }
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
    // Where each occurs: A's cell, the part's own for a part.
    let seen = |a: u16, b: u16, edge| p.seen[&pair(a, b, edge)];
    let cell = |x, y| Some(At { z: 0, x, y });
    let s = seen(300, 302, Edge::East);
    assert_eq!(
        (s.map, s.objects, s.source()),
        (None, cell(0, 2), "objects")
    );
    assert_eq!(s.example(), cell(0, 2));
    assert_eq!(seen(322, 323, Edge::East).objects, cell(8, 7));
    assert_eq!(seen(321, 323, Edge::South).objects, cell(9, 6));
    assert_eq!(seen(330, 331, Edge::South).objects, cell(12, 2));
    // A pair the map has too: the map's cell first.
    let both = Seen {
        map: cell(1, 1),
        objects: cell(2, 2),
    };
    assert_eq!((both.source(), both.example()), ("map+objects", cell(1, 1)));
}

/// An opaque object record but for its edges: `west`, `east`, `north` and `south` opaque
/// pixels along each, from the north-west corner.
fn edged(west: usize, east: usize, north: usize, south: usize) -> Record {
    let mut r = object(16);
    for i in 0..16 {
        let t = &mut r.transparent;
        t[i * 16] |= i >= west;
        t[i * 16 + 15] |= i >= east;
        t[i] |= i >= north;
        t[15 * 16 + i] |= i >= south;
    }
    r
}

#[test]
fn objects_meet_only_with_both_edges_half_opaque_and_on_a_level() {
    let mut g = synthetic::game();
    let full = 300;
    g.records[full] = object(16);
    // A's east edge opaque on 7 and on 8 pixels; A's south edge, B's west edge and B's north
    // edge, the same.
    let (e7, e8, s7, s8, n7, n8, w7, w8) = (340, 341, 342, 343, 344, 345, 346, 347);
    g.records[e7] = edged(16, 7, 16, 16);
    g.records[e8] = edged(16, 8, 16, 16);
    g.records[s7] = edged(16, 16, 16, 7);
    g.records[s8] = edged(16, 16, 16, 8);
    g.records[n7] = edged(16, 16, 7, 16);
    g.records[n8] = edged(16, 16, 8, 16);
    g.records[w7] = edged(7, 16, 16, 16);
    g.records[w8] = edged(8, 16, 16, 16);
    // Two opaque objects side by side, but on a level the map does not have.
    let (off_a, off_b) = (350, 351);
    g.records[off_a] = object(16);
    g.records[off_b] = object(16);
    let at = |x, y, tile: usize| Placed {
        x,
        y,
        z: 0,
        tile: tile as u16,
    };
    let off = |x, tile: usize| Placed {
        x,
        y: 12,
        z: 9,
        tile: tile as u16,
    };
    g.world = World {
        chunks: vec![chunk(0, &[])],
        levels: vec![Level {
            side: 2,
            refs: vec![0; 4],
        }],
        objects: vec![
            at(0, 4, e7),
            at(1, 4, full),
            at(0, 6, e8),
            at(1, 6, full),
            at(4, 8, s7),
            at(4, 9, full),
            at(6, 8, s8),
            at(6, 9, full),
            at(8, 8, full),
            at(8, 9, n7),
            at(10, 8, full),
            at(10, 9, n8),
            at(12, 4, full),
            at(13, 4, w7),
            at(12, 6, full),
            at(13, 6, w8),
            off(12, off_a),
            off(13, off_b),
        ],
    };
    let p = pairs(&g);
    let objects: BTreeSet<Pair> = p
        .all
        .iter()
        .copied()
        .filter(|q| p.seen[q].objects.is_some())
        .collect();
    let (full, e8, s8, n8, w8) = (full as u16, e8 as u16, s8 as u16, n8 as u16, w8 as u16);
    assert_eq!(
        objects,
        BTreeSet::from([
            pair(e8, full, Edge::East),
            pair(s8, full, Edge::South),
            pair(full, n8, Edge::South),
            pair(full, w8, Edge::East),
        ])
    );
    assert_eq!(p.objects, 4);
}

#[test]
fn a_pair_whose_edges_draw_in_different_rows_is_checked_and_counted_unmeasured() {
    let mut g = synthetic::game();
    // A's east edge opaque on its first 8 pixels; B's west edge on its last 8 (a control B on
    // its first 8). Each edge is half opaque, so the objects meet, but no row across the join
    // has both drawn: nothing is measured there.
    let (a, b, control) = (360u16, 361u16, 362u16);
    g.records[usize::from(a)] = edged(16, 8, 16, 16);
    let mut last = object(16);
    for y in 0..8 {
        last.transparent[y * 16] = true;
    }
    g.records[usize::from(b)] = last;
    g.records[usize::from(control)] = edged(8, 16, 16, 16);
    let report = |counts: Counts| {
        ingest::Report {
            neighbour: counts,
            ..Default::default()
        }
        .text()
    };
    for (b, unmeasured) in [(b, 1), (control, 0)] {
        let at = |x, tile| Placed {
            x,
            y: 4,
            z: 0,
            tile,
        };
        g.world = World {
            chunks: vec![chunk(0, &[])],
            levels: vec![Level {
                side: 1,
                refs: vec![0],
            }],
            objects: vec![at(2, a), at(3, b)],
        };
        let p = pairs(&g);
        let objects: Vec<Pair> = p
            .all
            .iter()
            .copied()
            .filter(|q| p.seen[q].objects.is_some())
            .collect();
        assert_eq!(objects, [pair(a, b, Edge::East)], "{b}");
        // A generated (its master the classic record at x8), B drawn by MMPX, the grass of
        // the map pairs too: only the pair with A is checked.
        let master = g.classic(ImageId::Tile(a)).unwrap().scaled(8, BLACK);
        let accepted = BTreeMap::from([(a, (10u32, &master))]);
        let s = super::sides(&g, &p.all, &accepted).unwrap();
        let out = settle(&p.all, &s, seam());
        assert!(out.fallen.is_empty(), "{b}");
        assert_eq!(out.checked.len(), 1, "{b}");
        let c = &out.checked[0];
        assert_eq!(
            (c.pair, c.both, c.fails),
            (pair(a, b, Edge::East), false, false)
        );
        assert_eq!(c.result.is_none(), unmeasured == 1, "{b}");
        let n = &out.counts;
        assert_eq!(
            (n.checked, n.unmeasured, n.failing_before),
            (1, unmeasured, 0),
            "{b}"
        );
        assert_eq!(
            (
                n.checked_after,
                n.unmeasured_after,
                n.failing_after,
                n.passes
            ),
            (1, unmeasured, 0, 1),
            "{b}"
        );
        // The coverage report says so.
        assert!(report(out.counts.clone()).contains(&format!(
            "neighbour pairs unmeasured (checked, but no row across the join both sides draw): \
             {unmeasured} before the fixed point, {unmeasured} after"
        )));
    }
    // Before and after are each their own count.
    let counts = Counts {
        unmeasured: 3,
        unmeasured_after: 2,
        ..Default::default()
    };
    assert!(report(counts).contains(
        "neighbour pairs unmeasured (checked, but no row across the join both sides draw): 3 \
         before the fixed point, 2 after"
    ));
}

/// Tile records for the compositor, each keyed by its record number.
struct Tiles(BTreeMap<u16, Record>);

impl Images for Tiles {
    fn image(&mut self, kind: Kind, id: u16) -> Option<Image<'_>> {
        let r = self.0.get(&id).filter(|_| kind == Kind::Tile)?;
        Some(Image {
            ver: 0,
            width: 16,
            height: 16,
            pixels: &r.pixels,
            transparent: Some(&r.transparent),
        })
    }

    fn key(&mut self, kind: Kind, id: u16) -> Option<u64> {
        (kind == Kind::Tile).then_some(u64::from(id))
    }
}

/// A pack of x4 tile images, by record number.
struct TilePack(BTreeMap<u16, Rgba>);

impl Pack for TilePack {
    fn hd(&self, kind: Kind, id: u16, key: u64) -> Option<PackHd<'_>> {
        if kind != Kind::Tile || key != u64::from(id) {
            return None;
        }
        let (index, img) = self
            .0
            .iter()
            .enumerate()
            .find(|(_, (r, _))| **r == id)
            .map(|(i, (_, img))| (i as u32, img))?;
        Some(PackHd {
            index,
            width: img.width as usize,
            height: img.height as usize,
            rgba: &img.pixels,
        })
    }
}

/// The colour index the frames are cleared to: what the page shows where no record draws.
const BACKGROUND: u8 = 5;

/// A frame of the synthetic palette cleared to [`BACKGROUND`], with each (record number,
/// record, x, y) blitted through the source plane: its opaque pixels name it, its transparent
/// ones leave the frame as it is.
fn frame(tiles: &[(u16, &Record, usize, usize)]) -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.enable_src();
    for (i, c) in synthetic::PALETTE.iter().enumerate() {
        fb.palette[i] = Rgb::new(c[0], c[1], c[2]);
    }
    fb.clear(BACKGROUND);
    for &(id, r, x0, y0) in tiles {
        for v in 0..16 {
            for u in 0..16 {
                let i = v * 16 + u;
                if !r.transparent[i] {
                    let src = Src {
                        ver: 0,
                        id,
                        u: u as u16,
                        v: v as u16,
                        kind: Kind::Tile,
                        flags: 0,
                    };
                    fb.set_src(x0 + u, y0 + v, r.pixels[i], src);
                }
            }
        }
    }
    fb
}

/// The composed frame's colour at HD pixel (x, y) of S 4.
fn composed(c: &Compositor, x: usize, y: usize) -> [u8; 3] {
    let p = c.rgba()[y * WIDTH * 4 + x].to_le_bytes();
    [p[0], p[1], p[2]]
}

#[test]
fn the_images_measured_are_the_ones_the_page_composes() {
    // A figure (transparent around it) drawn at (40, 24) over the background, through the
    // plane.
    let record = synthetic::figure(1, 0, 10);
    let classic = Classic::from_record(&record);
    let (id, x0, y0) = (200, 40, 24);
    let fb = frame(&[(id, &record, x0, y0)]);
    let mut images = Tiles(BTreeMap::from([(id, record.clone())]));
    // What is measured, from the classic record alone: the x4 sub-pixels of its opaque
    // pixels; the page shows the background at every other one.
    let mask: Vec<bool> = (0..64 * 64)
        .map(|i| !record.transparent[(i / 64 / 4) * 16 + (i % 64) / 4])
        .collect();
    assert!(mask.iter().any(|&m| m) && mask.iter().any(|&m| !m));
    let background = synthetic::PALETTE[usize::from(BACKGROUND)];
    // Every sub-pixel: the coverage exactly the mask, each measured colour the page's, each
    // other one the background; the measured ones counted by their HD image's alpha.
    let check = |c: &Compositor, want: &[Option<[f32; 3]>], hd: &Rgba| {
        assert_eq!(want.len(), 64 * 64);
        let mut by_alpha: BTreeMap<u8, usize> = BTreeMap::new();
        for y in 0..64usize {
            for x in 0..64usize {
                let i = y * 64 + x;
                let got = composed(c, x0 * 4 + x, y0 * 4 + y);
                assert_eq!(want[i].is_some(), mask[i], "coverage at ({x}, {y})");
                match want[i] {
                    Some(w) => {
                        assert_eq!(got.map(f32::from), w, "({x}, {y})");
                        *by_alpha.entry(hd.pixels[i * 4 + 3]).or_default() += 1;
                    }
                    None => assert_eq!(got, background, "below at ({x}, {y})"),
                }
            }
        }
        assert_eq!(
            by_alpha.values().sum::<usize>(),
            mask.iter().filter(|&&m| m).count()
        );
        by_alpha
    };
    // No pack: MMPX x4 of the record.
    let mut c = Compositor::new();
    c.compose(&fb, &mut images, Scale::X4);
    let scaled = mmpx4(&classic).unwrap();
    check(&c, &gate::shown(&classic, &scaled).unwrap(), &scaled);
    // A pack image whose sub-pixels are opaque, half, a little and not at all, in turn, over
    // the classic colours.
    let mut generated = Rgba::new(64, 64);
    for y in 0..64 {
        for x in 0..64 {
            let a = [255, 0, 128, 40][((x + y) % 4) as usize];
            img::put(&mut generated, x, y, [(x * 4) as u8, (y * 4) as u8, 90, a]);
        }
    }
    c.set_pack(Some(Rc::new(TilePack(BTreeMap::from([(
        id,
        generated.clone(),
    )])))));
    c.compose(&fb, &mut images, Scale::X4);
    let by_alpha = check(&c, &gate::shown(&classic, &generated).unwrap(), &generated);
    assert_eq!(
        by_alpha.keys().copied().collect::<Vec<_>>(),
        [0, 40, 128, 255]
    );
}

/// Mean steps across the lattice lines `pitch` apart along x of a `w × h` picture whose pixels
/// may be undefined: (across the line before column `join`, across the others), each over the
/// steps both of whose pixels are defined (the measure written out again, for a reference).
fn reference_steps(
    px: &dyn Fn(usize, usize) -> Option<[f64; 3]>,
    (w, h): (usize, usize),
    pitch: usize,
    join: usize,
) -> (f64, f64) {
    let (mut j, mut nj, mut o, mut no) = (0.0, 0.0, 0.0, 0.0);
    for y in 0..h {
        for x in (pitch..w).step_by(pitch) {
            if let (Some(a), Some(b)) = (px(x - 1, y), px(x, y)) {
                let d = (0..3).map(|i| (a[i] - b[i]).abs()).sum::<f64>() / 3.0;
                if x == join {
                    (j, nj) = (j + d, nj + 1.0);
                } else {
                    (o, no) = (o + d, no + 1.0);
                }
            }
        }
    }
    assert!(nj > 0.0 && no > 0.0);
    (j / nj, o / no)
}

#[test]
fn a_part_transparent_pair_is_measured_as_the_page_composes_it() {
    // A beside B (east): two records with holes, along their shared edge too, in a grain of
    // three palette colours.
    let (a, b) = (400u16, 401u16);
    let holes = |seed: u32, every: u32| Record {
        pixels: (0..256)
            .map(|i| [13u8, 14, 21][(hash(i, seed) % 3) as usize])
            .collect(),
        transparent: (0..256)
            .map(|i| hash(i, seed + 7).is_multiple_of(every))
            .collect(),
        map: false,
    };
    let mut g = synthetic::game();
    g.records[usize::from(a)] = holes(1, 5);
    g.records[usize::from(b)] = holes(2, 6);
    let (ra, rb) = (
        g.records[usize::from(a)].clone(),
        g.records[usize::from(b)].clone(),
    );
    // Holes on both sides of the shared edge: A's east column, B's west column.
    assert!((0..16).any(|y| ra.transparent[y * 16 + 15]));
    assert!((0..16).any(|y| rb.transparent[y * 16]));
    // A's x8 master: a gradient whose x4 sub-pixels are opaque, clear, half and a little opaque
    // in turn. Within each 2 x 2 block the half and the little opaque pixels are 20 above and
    // below their class on a checkerboard (the block's mean alpha is the class), and red is 30
    // higher on the same squares: a block's pixels differ, and their colour follows their
    // alpha, so taking one pixel of the block, or its unweighted mean, gives another x4 than
    // the pack's alpha-weighted box average.
    let alpha = |u: u32, v: u32| [255u8, 0, 128, 40][((u + v) % 4) as usize];
    let mut master = Rgba::new(128, 128);
    for y in 0..128 {
        for x in 0..128 {
            let (u, v) = (x / 2, y / 2);
            let odd = (x + y) % 2;
            let a = match alpha(u, v) {
                128 | 40 if odd == 1 => alpha(u, v) + 20,
                128 | 40 => alpha(u, v) - 20,
                a => a,
            };
            let c = [
                (u * 3 + odd * 30) as u8,
                (v * 2 + 40) as u8,
                (200 - u - v) as u8,
            ];
            img::put(&mut master, x, y, [c[0], c[1], c[2], a]);
        }
    }
    // The real path: ingest's side preparation, then the fixed point's first pass.
    let pairs = [pair(a, b, Edge::East)];
    let accepted = BTreeMap::from([(a, (10u32, &master))]);
    let s = super::sides(&g, &pairs, &accepted).unwrap();
    let out = settle(&pairs, &s, seam());
    assert_eq!(out.counts.checked, 1);
    let (ratio, limit) = out.checked[0].result.unwrap();

    // The reference: A's image read back from the pack `pack` writes, both records composed
    // by the page's compositor at S 4, the steps taken off the composed frame.
    let w = work("measured");
    let id = ImageId::Tile(a);
    let stem = atlas::slice_name(id);
    img::write_png(&w.slice(&stem), &master).unwrap();
    let listed = [ingest::Accepted {
        image: atlas::image_name(id),
        hash: format!("{:016x}", g.hash(id).unwrap()),
        width: 128,
        height: 128,
        slice: format!("slices/{stem}.png"),
    }];
    w.write("ingest.json", &serde_json::to_vec(&listed).unwrap())
        .unwrap();
    pack::pack(&w, None).unwrap();
    let p = read_pack(&std::fs::read(w.path("pack/u6-hdpack.zip")).unwrap()).unwrap();
    let in_pack = p.get(id).unwrap().image.clone();
    assert_eq!(
        s[&a].generated.as_ref().unwrap().1,
        gate::shown(&s[&a].classic, &in_pack).unwrap(),
        "the gate's side is the pack's image"
    );
    let _ = std::fs::remove_dir_all(&w.root);
    let (x0, y0) = (40, 24);
    let fb = frame(&[(a, &ra, x0, y0), (b, &rb, x0 + 16, y0)]);
    let mut images = Tiles(BTreeMap::from([(a, ra.clone()), (b, rb.clone())]));
    let mut c = Compositor::new();
    c.set_pack(Some(Rc::new(TilePack(BTreeMap::from([(
        a,
        in_pack.clone(),
    )])))));
    c.compose(&fb, &mut images, Scale::X4);
    // A record's pixel under HD pixel (x, y) of the pair, and whether it draws there.
    let under = |x: usize, y: usize| -> (&Record, usize) {
        let (r, x) = if x < 64 { (&ra, x) } else { (&rb, x - 64) };
        (r, (y / 4) * 16 + x / 4)
    };
    let hd = |x: usize, y: usize| -> Option<[f64; 3]> {
        let (r, i) = under(x, y);
        (!r.transparent[i]).then(|| composed(&c, x0 * 4 + x, y0 * 4 + y).map(f64::from))
    };
    let (j, o) = reference_steps(&hd, (128, 64), 4, 64);
    assert!(o >= 0.5, "no floor: {o}");
    let classic = |x: usize, y: usize| -> Option<[f64; 3]> {
        let (r, i) = under(x * 4, y * 4);
        (!r.transparent[i]).then(|| synthetic::PALETTE[usize::from(r.pixels[i])].map(f64::from))
    };
    let (cj, co) = reference_steps(&classic, (32, 16), 1, 16);
    assert!(co >= 0.5, "no floor: {co}");
    let close = |x: f32, y: f64| (f64::from(x) - y).abs() <= 1e-4 * y.abs().max(1.0);
    assert!(close(ratio, j / o), "{ratio} vs {}", j / o);
    assert!(
        close(limit, (cj / co).max(1.5)),
        "{limit} vs {}",
        (cj / co).max(1.5)
    );
    // A's measured sub-pixels include every alpha class of the pack image.
    let mut classes = BTreeSet::new();
    for y in 0..64usize {
        for x in 0..64usize {
            if hd(x, y).is_some() {
                classes.insert(in_pack.pixels[(y * 64 + x) * 4 + 3]);
            }
        }
    }
    assert_eq!(classes.into_iter().collect::<Vec<_>>(), [0, 40, 128, 255]);
    // And the measure is the generated image's: A drawn by MMPX measures otherwise.
    let (sa, sb) = (&s[&a], &s[&b]);
    let scaled = gate::neighbour_seam(
        (&sa.classic, &sa.scaled),
        (&sb.classic, &sb.scaled),
        K,
        Edge::East,
        seam(),
    )
    .unwrap();
    assert!(
        (scaled.0 - ratio).abs() > 0.01 * ratio,
        "{} vs {ratio}",
        scaled.0
    );
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

/// Every line of a `gates.jsonl` parsed and checked against the row kinds ingest writes: its
/// keys exactly one kind's, and a neighbour pair row's and a fallback row's fields of their
/// types and values. Returns how many of each kind.
fn gates_rows(log: &str) -> BTreeMap<&'static str, usize> {
    let keys = |k: &[&str]| -> BTreeSet<String> { k.iter().map(|s| s.to_string()).collect() };
    let kinds = [
        (
            "image",
            keys(&[
                "sheet",
                "attempt",
                "item",
                "image",
                "pass",
                "structure",
                "shift",
                "iou",
                "purity",
                "flat",
                "drift_raw",
                "drift",
                "seam",
                "gates",
                "structure_scale",
            ]),
        ),
        ("refused", keys(&["sheet", "attempt", "refused"])),
        (
            "actor",
            keys(&["item", "attempt", "class", "motion", "gates"]),
        ),
        (
            "pair",
            keys(&[
                "neighbour",
                "pass",
                "fail",
                "ratio",
                "limit",
                "sides",
                "source",
                "at",
                "gates",
            ]),
        ),
        (
            "fallback",
            keys(&["item", "class", "fallback", "pass", "gates"]),
        ),
    ];
    let tile = |v: &serde_json::Value| {
        v.as_str()
            .and_then(atlas::parse_image)
            .is_some_and(|id| matches!(id, ImageId::Tile(_)))
    };
    let mut out = BTreeMap::new();
    for line in log.lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let have: BTreeSet<String> = row.as_object().unwrap().keys().cloned().collect();
        let (kind, _) = kinds
            .iter()
            .find(|(_, k)| *k == have)
            .unwrap_or_else(|| panic!("a row of no kind: {have:?}"));
        match *kind {
            "pair" => {
                let n = row["neighbour"].as_object().unwrap();
                assert_eq!(n.len(), 3, "{line}");
                assert!(tile(&n["a"]) && tile(&n["b"]), "{line}");
                assert!(n["edge"] == "east" || n["edge"] == "south", "{line}");
                assert!(row["pass"].as_u64().is_some_and(|p| p >= 1), "{line}");
                assert!(row["fail"].is_boolean(), "{line}");
                let (ratio, limit) = (&row["ratio"], &row["limit"]);
                assert!(
                    (ratio.is_f64() && limit.is_f64()) || (ratio.is_null() && limit.is_null()),
                    "{line}"
                );
                if row["fail"] == true {
                    assert!(ratio.as_f64().unwrap() > limit.as_f64().unwrap(), "{line}");
                }
                assert!(["gen-gen", "gen-mmpx"].contains(&row["sides"].as_str().unwrap()));
                let source = row["source"].as_str().unwrap();
                assert!(
                    ["map", "objects", "map+objects"].contains(&source),
                    "{line}"
                );
                let at = row["at"].as_object().unwrap();
                assert_eq!(at.len(), 3, "{line}");
                assert!(["z", "x", "y"].iter().all(|k| at[*k].is_u64()), "{line}");
                assert!(row["gates"].is_string());
            }
            "fallback" => {
                assert_eq!(row["fallback"], "neighbour");
                assert!(row["item"].is_u64() && row["class"].is_string(), "{line}");
                assert!(row["pass"].as_u64().is_some_and(|p| p >= 1), "{line}");
            }
            _ => {}
        }
        *out.entry(*kind).or_default() += 1;
    }
    out
}

/// What ingest leaves in a work dir: `ingest.json`, `gates.jsonl`, `coverage.txt` and every
/// file in `slices/` (name and bytes).
type Left = (Vec<u8>, Vec<u8>, Vec<u8>, BTreeMap<String, Vec<u8>>);

/// [`Left`] of `w`.
fn left(w: &Work) -> Left {
    let read = |rel: &str| std::fs::read(w.path(rel)).unwrap();
    let slices = std::fs::read_dir(w.path("slices"))
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            let name = e.file_name().to_string_lossy().into_owned();
            (name, std::fs::read(e.path()).unwrap())
        })
        .collect();
    (
        read("ingest.json"),
        read("gates.jsonl"),
        read("coverage.txt"),
        slices,
    )
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
    // Grass beside grass (map), the table's two parts and the table beside the sword (objects):
    // every pair has a generated side in the first pass.
    assert_eq!(n.checked, 4);
    let text = r.text();
    assert!(text.contains(
        "neighbour pairs (distinct records in edge-adjacent cells): 4 (map 2, objects 2)"
    ));
    assert!(text.contains(
        "neighbour pairs checked (only pairs with a generated side): 4 before the fixed point, \
         2 after"
    ));
    // Every pair checked is measured: the grass pairs and the objects' fully opaque edges.
    assert_eq!((n.unmeasured, n.unmeasured_after), (0, 0));
    assert!(text.contains(
        "neighbour pairs unmeasured (checked, but no row across the join both sides draw): 0 \
         before the fixed point, 0 after"
    ));
    assert!(text.contains(
        "neighbour pairs failing (among pairs with a generated side): 1 before the fixed \
         point, 0 after (2 passes)"
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
    // A line per pair checked in the first pass, failing or not (no pair fails in the second).
    let pairs: Vec<&serde_json::Value> = lines
        .iter()
        .filter(|l| l["neighbour"].is_object())
        .collect();
    assert_eq!(pairs.len(), 4);
    assert!(
        pairs
            .iter()
            .all(|l| l["pass"] == 1 && l["gates"] == t.text())
    );
    let row = |a: u16, b: u16| -> Vec<&serde_json::Value> {
        let (a, b) = (
            atlas::image_name(ImageId::Tile(a)),
            atlas::image_name(ImageId::Tile(b)),
        );
        pairs
            .iter()
            .copied()
            .filter(|l| l["neighbour"]["a"] == a.as_str() && l["neighbour"]["b"] == b.as_str())
            .collect()
    };
    let failing = row(table + 1, sword);
    assert_eq!(failing.len(), 1);
    let f = failing[0];
    assert_eq!(f["neighbour"]["a"], anchor.as_str());
    assert_eq!(f["neighbour"]["edge"], "east");
    assert_eq!((&f["fail"], &f["sides"]), (&true.into(), &"gen-gen".into()));
    assert_eq!(f["source"], "objects");
    assert_eq!(f["at"], serde_json::json!({"z": 0, "x": 3, "y": 3}));
    assert!(f["ratio"].as_f64().unwrap() > f["limit"].as_f64().unwrap());
    let parts = row(table, table + 1);
    assert_eq!(parts.len(), 1);
    assert_eq!(
        (&parts[0]["fail"], &parts[0]["source"]),
        (&false.into(), &"objects".into())
    );
    assert_eq!(parts[0]["at"], serde_json::json!({"z": 0, "x": 2, "y": 3}));
    // The grass pairs: east and south, at the level's first cell.
    let grass = row(0, 0);
    assert_eq!(grass.len(), 2);
    for l in grass {
        assert_eq!(
            (&l["fail"], &l["sides"]),
            (&false.into(), &"gen-gen".into())
        );
        assert_eq!(l["source"], "map");
        assert_eq!(l["at"], serde_json::json!({"z": 0, "x": 0, "y": 0}));
        assert!(l["ratio"].is_number() && l["limit"].is_number());
    }
    // Every row of gates.jsonl is of a kind ingest writes, its fields checked.
    let kinds = gates_rows(&log);
    assert_eq!((kinds["pair"], kinds["fallback"]), (4, 2));
    // A second ingest of the same sheets comes to the same, and leaves the same: the accepted
    // list, every slice (the fallen masters still absent), the coverage and gates.jsonl, which
    // is written whole each time (not appended to).
    let before = left(&w);
    let names: BTreeSet<String> = accepted.iter().map(|a| a.image.clone()).collect();
    assert_eq!(
        before.3.keys().cloned().collect::<BTreeSet<_>>(),
        accepted
            .iter()
            .map(|a| a.slice.trim_start_matches("slices/").to_string())
            .collect()
    );
    assert_eq!(ingest::ingest(&g, &w, &t).unwrap(), r);
    let after = left(&w);
    assert!(after == before, "the second ingest left something else");
    let again: Vec<ingest::Accepted> = serde_json::from_slice(&after.0).unwrap();
    assert_eq!(
        again
            .iter()
            .map(|a| a.image.clone())
            .collect::<BTreeSet<_>>(),
        names
    );
    for t in [table, table + 1, sword] {
        assert!(!w.slice(&atlas::slice_name(ImageId::Tile(t))).exists());
    }
    let again_rows = gates_rows(std::str::from_utf8(&after.1).unwrap());
    assert_eq!(again_rows, kinds);
    // Conservation: with no world, nothing meets and nothing falls back; then each class's
    // generated records are this ingest's generated and neighbour records together.
    let mut alone = g.clone();
    alone.world = World::default();
    let r0 = ingest::ingest(&alone, &w, &t).unwrap();
    assert_eq!(r0.neighbour.pairs, 0);
    for class in Class::ALL {
        assert_eq!(r0.get(class, Status::Neighbour), 0, "{class:?}");
        assert_eq!(
            r0.get(class, Status::Generated),
            r.get(class, Status::Generated) + r.get(class, Status::Neighbour),
            "{class:?}"
        );
        for st in [Status::Fallback, Status::Failed, Status::Pending] {
            assert_eq!(r0.get(class, st), r.get(class, st), "{class:?} {st:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&w.root);
}

//! Synthetic art only: the game here is [`synthetic::game`], drawn by code; every "generated"
//! sheet is made by the test from the sheet that was sent.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use u6_hdpack::{ImageId, Kind, Role, read_pack};

use crate::atlas::{
    self, BORDER, CELL, Job, PORTRAIT, SHEET_H, SHEET_W, SheetKind, TERRAIN_CELL, choose_key,
};
use crate::game::{Game, synthetic};
use crate::gate::{self, Thresholds};
use crate::group::{self, Class, Item};
use crate::img::{self, Classic, Rgba};
use crate::ingest::{self, Status};
use crate::run::{self, RunOptions, Runner};
use crate::work::{Work, check_outside_repo};
use crate::{pack, prompt};

/// A fresh work dir outside the repository.
fn work(name: &str) -> Work {
    let dir = std::env::temp_dir().join(format!("u6-hdtool-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let w = Work::open(Some(&dir)).unwrap();
    w.mkdirs().unwrap();
    w
}

fn numbered(plan: &group::Plan) -> Vec<(u32, &Item)> {
    plan.items
        .iter()
        .enumerate()
        .map(|(i, it)| (i as u32, it))
        .collect()
}

/// The synthetic game's atlas, written to `w`.
fn atlas_into(g: &Game, w: &Work) -> Vec<Job> {
    let plan = group::plan(g);
    let mut jobs = Vec::new();
    atlas::layout_each(g, &numbered(&plan), 1, 0, |job, sheet| {
        img::write_png(&w.sheet(&job.sheet), &sheet)?;
        jobs.push(job);
        Ok(())
    })
    .unwrap();
    w.append_jobs(&jobs).unwrap();
    jobs
}

/// "Generates" every pending sheet with `f` (sheet sent -> sheet back).
fn generate(w: &Work, f: impl Fn(&Job, Rgba) -> Rgba) {
    for job in w.read_jobs().unwrap() {
        if w.out(&job.sheet).exists() {
            continue;
        }
        let sent = img::read_png(&w.sheet(&job.sheet)).unwrap();
        img::write_png(&w.out(&job.sheet), &f(&job, sent)).unwrap();
    }
}

fn tile(r: u16) -> ImageId {
    ImageId::Tile(r)
}

fn of_class(plan: &group::Plan, class: Class) -> Vec<&Item> {
    plan.items.iter().filter(|i| i.class == class).collect()
}

#[test]
fn the_plan_classifies_from_the_tables() {
    let g = synthetic::game();
    let plan = group::plan(&g);
    // 12 chunk tiles less the shore record, plus the 4 animation frames of the water tile.
    assert_eq!(plan.count(Class::Terrain), 15);
    assert_eq!(plan.shore, 1);
    let terrain: Vec<ImageId> = of_class(&plan, Class::Terrain)
        .iter()
        .flat_map(|i| i.images())
        .collect();
    assert!(terrain.contains(&tile(synthetic::WATER_FRAMES + 3)));
    assert!(!terrain.contains(&tile(synthetic::SHORE)));
    // Two actor types: 8 frames of one record, 2 frames of a wide two-record beast.
    let actors = of_class(&plan, Class::Actor);
    assert_eq!(actors.len(), 2);
    assert_eq!(actors[0].pieces.len(), 8);
    assert!(actors[0].pieces.iter().all(|p| (p.w, p.h) == (1, 1)));
    assert_eq!(actors[1].pieces.len(), 2);
    let b = synthetic::ACTOR_B;
    assert_eq!((actors[1].pieces[0].w, actors[1].pieces[0].h), (2, 1));
    assert_eq!(actors[1].pieces[0].images, [tile(b), tile(b + 1)]);
    // Multi-record objects as the object draw composes them: 2 x 2 (t - 3 .. t), 2 x 1, 1 x 2.
    let shapes: Vec<(u32, u32, ImageId)> = of_class(&plan, Class::Multi)
        .iter()
        .map(|i| (i.pieces[0].w, i.pieces[0].h, i.pieces[0].images[0]))
        .collect();
    assert_eq!(
        shapes,
        [
            (2, 2, tile(synthetic::TREE)),
            (2, 1, tile(synthetic::TABLE)),
            (1, 2, tile(synthetic::BANNER))
        ]
    );
    // The rest one by one, the 8-wide halves included; portraits and bitmaps first.
    assert_eq!(plan.count(Class::Object), 5);
    assert_eq!(plan.count(Class::Portrait), 2);
    assert_eq!(plan.count(Class::Bitmap), 2);
    assert_eq!(plan.items[0].class, Class::Portrait);
    // Every record that draws is in exactly one item, or is the shore.
    let mut seen: Vec<ImageId> = plan.items.iter().flat_map(|i| i.images()).collect();
    let n = seen.len();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), n);
    let drawn = g.records.iter().filter(|r| !r.is_empty()).count();
    let tiles: usize = [Class::Terrain, Class::Actor, Class::Multi, Class::Object]
        .iter()
        .map(|&c| plan.count(c))
        .sum();
    assert_eq!(tiles + plan.shore, drawn);
}

#[test]
fn the_atlas_lays_out_cells_by_kind() {
    let g = synthetic::game();
    let plan = group::plan(&g);
    let sheets = atlas::layout(&g, &numbered(&plan), 1, 0);
    let kinds: Vec<SheetKind> = sheets.iter().map(|(j, _)| j.kind).collect();
    assert_eq!(
        kinds,
        [
            SheetKind::Portrait,
            SheetKind::Bitmap,
            SheetKind::Bitmap,
            SheetKind::Terrain,
            SheetKind::Tile
        ]
    );
    for (job, sheet) in &sheets {
        assert_eq!((sheet.width, sheet.height), (SHEET_W, SHEET_H));
        assert_eq!((job.width, job.height), (SHEET_W, SHEET_H));
        assert_eq!(job.prompt, job.kind.prompt_id());
        // The key is the sheet's background, and no image on it uses the key.
        let k = job.key;
        assert_eq!(
            img::get(sheet, SHEET_W - 1, SHEET_H - 1),
            [k[0], k[1], k[2], 255]
        );
        for p in &job.places {
            let c = g.classic(atlas::parse_image(&p.image).unwrap()).unwrap();
            assert!(c.px.iter().flatten().all(|&px| px != k));
            assert_eq!((c.width, c.height), (p.w, p.h));
            // The sheet holds the classic image at its place, nearest scaled.
            for (x, y) in [(0, 0), (p.w - 1, p.h - 1), (p.w / 2, p.h / 3)] {
                let want = c.at(x, y).unwrap_or(k);
                let got = img::get(sheet, p.x + x * p.scale + p.scale / 2, p.y + y * p.scale);
                assert_eq!(got[..3], want, "{} at {x},{y}", p.image);
            }
        }
    }
    // Portraits: 448 x 512 in a 3 x 2 grid.
    let (pj, _) = &sheets[0];
    assert_eq!((pj.places[1].x, pj.places[1].y), (PORTRAIT.0, 0));
    // A bitmap alone at the largest whole scale up to 8: 64 x 40 -> 8.
    let (bj, _) = &sheets[1];
    assert_eq!((bj.places.len(), bj.places[0].scale), (1, 8));
    // Terrain: 192 px cells, the tile 32 px in, the border wrapping it.
    let (tj, ts) = &sheets[3];
    assert_eq!(
        (tj.places[1].x, tj.places[1].y),
        (TERRAIN_CELL + BORDER * 8, BORDER * 8)
    );
    let p = &tj.places[0];
    let c = g.classic(atlas::parse_image(&p.image).unwrap()).unwrap();
    // The border's first source pixel is the tile's pixel (16 - 4, 16 - 4).
    let want = c.at(12, 12).unwrap();
    assert_eq!(img::get(ts, p.x - BORDER * 8, p.y - BORDER * 8)[..3], want);
    // Tiles: every frame of one actor in one sheet, a mosaic in adjacent cells.
    let (job, _) = &sheets[4];
    let at = |id: ImageId| {
        let p = job
            .places
            .iter()
            .find(|p| p.image == atlas::image_name(id))
            .unwrap();
        (p.x / CELL, p.y / CELL)
    };
    let t = synthetic::TREE;
    let (c0, r0) = at(tile(t));
    assert_eq!(
        [at(tile(t + 1)), at(tile(t + 2)), at(tile(t + 3))],
        [(c0 + 1, r0), (c0, r0 + 1), (c0 + 1, r0 + 1)]
    );
    let b = synthetic::BANNER;
    assert_eq!(at(tile(b + 1)), (at(tile(b)).0, at(tile(b)).1 + 1));
    let a = synthetic::ACTOR_A;
    assert!((0..8).all(|f| {
        job.places
            .iter()
            .any(|p| p.image == atlas::image_name(tile(a + f)))
    }));
    // The same plan gives the same sheets.
    let again = atlas::layout(&g, &numbered(&plan), 1, 0);
    assert!(sheets.iter().zip(&again).all(|(a, b)| a == b));
}

#[test]
fn an_actor_goes_whole_to_a_new_sheet_when_the_current_one_is_full() {
    let g = synthetic::game();
    let plan = group::plan(&g);
    let objects: Vec<(u32, &Item)> = numbered(&plan)
        .into_iter()
        .filter(|(_, i)| i.class == Class::Object)
        .collect();
    let actor = numbered(&plan)
        .into_iter()
        .find(|(_, i)| i.class == Class::Actor)
        .unwrap();
    // 92 single cells (the objects repeated), then the 8-frame actor: 4 cells are left.
    let mut items: Vec<(u32, &Item)> = (0..92).map(|k| objects[k % objects.len()]).collect();
    items.push(actor);
    let sheets = atlas::layout(&g, &items, 1, 0);
    assert_eq!(sheets.len(), 2);
    assert_eq!(sheets[0].0.places.len(), 92);
    assert!(sheets[1].0.places.iter().all(|p| p.item == actor.0));
    assert_eq!(sheets[1].0.places.len(), 8);
}

#[test]
fn the_key_is_the_candidate_farthest_from_the_colours_used() {
    assert_eq!(choose_key([[10, 200, 20]]), [255, 0, 255]);
    assert_eq!(choose_key([[250, 10, 250], [240, 0, 230]]), [0, 255, 0]);
    // Magenta and green both used: another candidate.
    let k = choose_key([[255, 0, 255], [0, 255, 0], [0, 0, 0]]);
    assert!(k != [255, 0, 255] && k != [0, 255, 0]);
    assert_eq!(choose_key(std::iter::empty()), [255, 0, 255]);
}

#[test]
fn image_names_round_trip() {
    for id in [
        tile(0),
        tile(2047),
        ImageId::Portrait {
            letter: b'z',
            item: 12,
        },
        ImageId::Bitmap(Role::Spellbook),
    ] {
        assert_eq!(atlas::parse_image(&atlas::image_name(id)), Some(id));
    }
    for bad in ["t:", "t:x", "p:ab:1", "b:map", "t:1:2", "q:1"] {
        assert_eq!(atlas::parse_image(bad), None, "{bad}");
    }
}

/// A 16 x 16 classic sprite: a filled box with a one-pixel dark frame on a transparent ground.
fn sprite() -> Classic {
    let mut px = vec![None; 256];
    for y in 3..13 {
        for x in 4..12 {
            let frame = x == 4 || x == 11 || y == 3 || y == 12;
            px[y * 16 + x] = Some(if frame { [20, 20, 30] } else { [200, 80, 40] });
        }
    }
    Classic {
        width: 16,
        height: 16,
        px,
    }
}

const KEY: [u8; 3] = [255, 0, 255];

/// A perfect "repaint" of `c` at x8: the classic image as it was sent.
fn repaint(c: &Classic) -> Rgba {
    c.scaled(8, KEY)
}

/// Moves an image by (dx, dy) HD pixels, filling with the key.
fn moved(img: &Rgba, dx: i32, dy: i32) -> Rgba {
    let mut out = img.clone();
    for y in 0..img.height as i32 {
        for x in 0..img.width as i32 {
            let (sx, sy) = (x - dx, y - dy);
            let inside = sx >= 0 && sy >= 0 && sx < img.width as i32 && sy < img.height as i32;
            let c = if inside {
                img::get(img, sx as u32, sy as u32)
            } else {
                [KEY[0], KEY[1], KEY[2], 255]
            };
            img::put(&mut out, x as u32, y as u32, c);
        }
    }
    out
}

fn flat() -> Classic {
    Classic {
        width: 16,
        height: 16,
        px: vec![Some([90, 90, 90]); 256],
    }
}

#[test]
fn keying_is_bounded_by_the_grown_mask() {
    let c = sprite();
    let mut raw = repaint(&c);
    // A stray mark far from the sprite, and a half-key edge pixel next to it.
    img::fill(&mut raw, 8, 8, 8, 8, [10, 200, 10, 255]);
    img::put(&mut raw, 4 * 8 - 3, 6 * 8, [200, 60, 200, 255]);
    let hd = gate::key_out(&raw, &c, KEY);
    // Outside the mask grown by one source pixel: transparent whatever was painted.
    assert_eq!(img::get(&hd, 10, 10)[3], 0);
    // Pure key inside the grown mask: transparent; the sprite: opaque, colours kept.
    assert_eq!(img::get(&hd, 3 * 8 + 4, 6 * 8)[3], 0);
    assert_eq!(img::get(&hd, 6 * 8, 6 * 8), [200, 80, 40, 255]);
    // The half-key pixel: partly transparent and despilled (no key channel above the other).
    let p = img::get(&hd, 4 * 8 - 3, 6 * 8);
    assert!(p[3] > 0 && p[3] < 255, "{p:?}");
    assert!(p[0] <= p[1] + 1 && p[2] <= p[1] + 1, "{p:?}");
    // An opaque classic image is not keyed at all, whatever its colours.
    let opaque = Classic {
        width: 16,
        height: 16,
        px: vec![Some(KEY); 256],
    };
    let hd = gate::key_out(&opaque.scaled(8, KEY), &opaque, KEY);
    assert!(hd.pixels.chunks(4).all(|p| p[3] == 255));
}

#[test]
fn structure_tells_a_repaint_from_another_shape() {
    let c = sprite();
    assert!(gate::structure(&c, &repaint(&c), KEY).unwrap() > 0.99);
    // The same colours in another shape and place.
    let mut other = c.clone();
    other.px = vec![None; 256];
    for p in &mut other.px[..6 * 16] {
        *p = Some([200, 80, 40]);
    }
    assert!(gate::structure(&c, &repaint(&other), KEY).unwrap() < 0.5);
    // A flat image has no structure and no offset to measure.
    let f = flat();
    assert_eq!(gate::structure(&f, &f.scaled(8, KEY), KEY), None);
    assert_eq!(gate::shift(&f, &f.scaled(8, KEY), KEY), None);
}

#[test]
fn shift_is_measured_in_source_pixels() {
    let c = sprite();
    let good = repaint(&c);
    assert_eq!(gate::shift(&c, &good, KEY), Some(0.0));
    assert_eq!(gate::shift(&c, &moved(&good, 2, 0), KEY), Some(0.25));
    assert_eq!(gate::shift(&c, &moved(&good, 0, -8), KEY), Some(1.0));
    assert_eq!(gate::shift(&c, &moved(&good, 12, 4), KEY), Some(1.5));
    // A periodic texture moved by its period matches as well as unmoved: no shift.
    let stripes = Classic {
        width: 16,
        height: 16,
        px: (0..256)
            .map(|i| {
                Some(if i % 2 == 0 {
                    [40, 40, 160]
                } else {
                    [120, 160, 220]
                })
            })
            .collect(),
    };
    let s = stripes.scaled(8, KEY);
    let mut wrapped = s.clone();
    for y in 0..128 {
        for x in 0..128 {
            img::put(&mut wrapped, x, y, img::get(&s, (x + 112) % 128, y));
        }
    }
    assert_eq!(gate::shift(&stripes, &wrapped, KEY), Some(0.0));
    let m = gate::Metrics {
        shift: Some(0.75),
        ..Default::default()
    };
    assert_eq!(m.failures(&Thresholds::default()), ["shift"]);
}

#[test]
fn iou_and_purity_catch_lost_silhouettes_and_dirty_keys() {
    let c = sprite();
    let good = repaint(&c);
    let hd = gate::key_out(&good, &c, KEY);
    assert!(gate::iou(&c, &hd).unwrap() > 0.99);
    assert!(gate::purity(&good, &c, KEY).unwrap() <= 1.0);
    // The top half of the sprite painted over with the key: the silhouette is lost.
    let mut eaten = good.clone();
    img::fill(&mut eaten, 0, 0, 128, 64, [255, 0, 255, 255]);
    let hd = gate::key_out(&eaten, &c, KEY);
    assert!(gate::iou(&c, &hd).unwrap() < 0.85);
    // A shadowed background: the key is no longer flat.
    let mut dirty = good.clone();
    img::fill(&mut dirty, 0, 112, 128, 16, [225, 30, 225, 255]);
    assert!(gate::purity(&dirty, &c, KEY).unwrap() > 12.0);
    // An opaque image has neither.
    let f = flat();
    assert_eq!(gate::iou(&f, &f.scaled(8, KEY)), None);
    assert_eq!(gate::purity(&f.scaled(8, KEY), &f, KEY), None);
}

#[test]
fn colour_transfer_pulls_drift_back() {
    let c = sprite();
    let mut hd = gate::key_out(&repaint(&c), &c, KEY);
    for p in hd.pixels.chunks_exact_mut(4) {
        if p[3] > 0 {
            p[0] = p[0].saturating_sub(40);
            p[2] = p[2].saturating_add(30);
        }
    }
    let before = gate::drift(&c, &hd).unwrap();
    gate::colour_transfer(&mut hd, &c);
    let after = gate::drift(&c, &hd).unwrap();
    assert!(before > 20.0 && after < 2.0, "{before} -> {after}");
}

/// A 16 x 16 opaque texture of diagonal bands, which tiles with itself.
fn bands() -> Classic {
    Classic {
        width: 16,
        height: 16,
        px: (0..256)
            .map(|i| {
                let (x, y) = (i % 16, i / 16);
                Some(if (x + y) % 8 < 3 {
                    [40, 90, 40]
                } else {
                    [70, 140, 60]
                })
            })
            .collect(),
    }
}

/// A smooth x8 version of a texture: bilinear between source pixel centres, wrapping.
fn smooth(c: &Classic) -> Rgba {
    let mut out = Rgba::new(128, 128);
    for y in 0..128u32 {
        for x in 0..128u32 {
            let fx = (x as f32 + 0.5) / 8.0 - 0.5;
            let fy = (y as f32 + 0.5) / 8.0 - 0.5;
            let (x0, y0) = (fx.floor(), fy.floor());
            let (tx, ty) = (fx - x0, fy - y0);
            let at = |dx: f32, dy: f32| {
                let xx = ((x0 + dx) as i32).rem_euclid(16) as u32;
                let yy = ((y0 + dy) as i32).rem_euclid(16) as u32;
                c.at(xx, yy).unwrap().map(f32::from)
            };
            let (a, b, cc, d) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0), at(1.0, 1.0));
            let mix = |i: usize| {
                let top = a[i] * (1.0 - tx) + b[i] * tx;
                let bot = cc[i] * (1.0 - tx) + d[i] * tx;
                (top * (1.0 - ty) + bot * ty).round() as u8
            };
            img::put(&mut out, x, y, [mix(0), mix(1), mix(2), 255]);
        }
    }
    out
}

#[test]
fn the_seam_gate_wants_a_tile_that_tiles() {
    let c = bands();
    let t = Thresholds::default();
    let good = smooth(&c);
    let (r, limit) = gate::seam(&c, &good, t.seam);
    assert!(gate::seam_passes((r, limit)), "{r} / {limit}");
    // The same tile with a hard band along its wrap edge: a visible seam.
    let mut bad = good.clone();
    img::fill(&mut bad, 0, 0, 4, 128, [200, 200, 40, 255]);
    let (r, limit) = gate::seam(&c, &bad, t.seam);
    assert!(!gate::seam_passes((r, limit)), "{r} / {limit}");
    // The classic tile scaled has the classic's own ratio, which is then its limit.
    assert!(gate::seam_passes(gate::seam(&c, &c.scaled(8, KEY), t.seam)));
}

#[test]
fn motion_is_the_share_of_the_frame_change_kept() {
    let a = sprite();
    let mut b = a.clone();
    // Frame b lifts a "leg": three pixels change.
    for y in 13..16 {
        b.px[y * 16 + 6] = Some([20, 20, 30]);
    }
    // As ingest has them: keyed.
    let (ha, hb) = (
        gate::key_out(&repaint(&a), &a, KEY),
        gate::key_out(&repaint(&b), &b, KEY),
    );
    let full = gate::motion(&[(a.clone(), b.clone(), ha.clone(), hb.clone())]).unwrap();
    assert!((full - 1.0).abs() < 0.01, "{full}");
    // Both frames repainted alike: the motion is gone.
    let none = gate::motion(&[(a.clone(), b.clone(), ha.clone(), ha.clone())]).unwrap();
    assert!(none < 0.01, "{none}");
    // Equal classic frames make no pair.
    assert_eq!(gate::motion(&[(a.clone(), a, ha.clone(), ha)]), None);
}

#[test]
fn ingest_slices_gates_rerolls_and_reports() {
    let g = synthetic::game();
    let w = work("ingest");
    let jobs = atlas_into(&g, &w);
    let t = Thresholds::default();
    // Nothing back yet: everything pending, nothing re-rolled.
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert_eq!(r.get(Class::Terrain, Status::Pending), 15);
    assert_eq!(r.rerolled_items, 0);
    // Every sheet comes back as sent, but the tile sheet one pixel narrower: refused whole.
    generate(&w, |job, sent| {
        if job.kind == SheetKind::Tile {
            sent.rect(0, 0, SHEET_W - 1, SHEET_H)
        } else {
            sent
        }
    });
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert_eq!(r.refused_sheets, 1);
    assert_eq!(r.get(Class::Portrait, Status::Generated), 2);
    assert_eq!(r.get(Class::Bitmap, Status::Generated), 2);
    assert_eq!(r.get(Class::Terrain, Status::Generated), 15);
    // The refused sheet's items are queued for their one re-roll.
    assert_eq!(r.get(Class::Actor, Status::Pending), 12);
    assert_eq!(r.get(Class::Object, Status::Pending), 5);
    assert_eq!(r.reroll_sheets, 1);
    let all = w.read_jobs().unwrap();
    assert_eq!(all.len(), jobs.len() + 1);
    let reroll = all.last().unwrap();
    assert_eq!((reroll.attempt, reroll.kind), (2, SheetKind::Tile));
    assert!(w.sheet(&reroll.sheet).exists());
    // The re-roll comes back with one frame of the first actor painted over: that actor
    // falls back whole (none of its frames is kept), nothing else does.
    let a = synthetic::ACTOR_A;
    let victim = reroll
        .places
        .iter()
        .find(|p| p.image == atlas::image_name(tile(a + 3)))
        .unwrap()
        .clone();
    generate(&w, |_, mut sent| {
        img::fill(
            &mut sent,
            victim.x,
            victim.y,
            CELL,
            CELL / 2,
            [10, 10, 10, 255],
        );
        sent
    });
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert_eq!(r.get(Class::Actor, Status::Fallback), 8);
    assert_eq!(r.get(Class::Actor, Status::Generated), 4);
    assert_eq!(r.get(Class::Object, Status::Generated), 5);
    assert_eq!(r.get(Class::Multi, Status::Generated), 8);
    assert_eq!(r.reroll_sheets, 0, "a second failure is not re-rolled");
    let text = r.text();
    assert!(text.contains("actor records"));
    // The first tile sheet is still the wrong size: counted on every ingest.
    assert!(text.contains("sheets refused (size, unreadable or source changed): 1"));
    assert_eq!(
        std::fs::read_to_string(w.path("coverage.txt")).unwrap(),
        text
    );
    // No frame of the fallen-back actor is in the list pack takes.
    let accepted: Vec<ingest::Accepted> =
        serde_json::from_slice(&std::fs::read(w.path("ingest.json")).unwrap()).unwrap();
    let name = |r| atlas::image_name(tile(r));
    assert!((0..8).all(|f| accepted.iter().all(|x| x.image != name(a + f))));
    assert!(accepted.iter().any(|x| x.image == name(synthetic::ACTOR_B)));
    // The pack holds what ingest accepted, with the classic hashes, derived.
    let s = pack::pack(&w, None).unwrap();
    let p = read_pack(&std::fs::read(w.path("pack/u6-hdpack.zip")).unwrap()).unwrap();
    assert!(p.manifest.derived);
    assert_eq!(p.images.len(), accepted.len());
    let tiles = p
        .images
        .iter()
        .filter(|i| i.id.kind() == Kind::Tile)
        .count();
    assert_eq!((s.tiles, s.portraits, s.bitmaps), (tiles, 2, 2));
    for img in &p.images {
        assert_eq!(Some(img.hash), g.hash(img.id), "{:?}", img.id);
    }
    let paper = p.get(ImageId::Bitmap(Role::Paper)).unwrap();
    // The pack is x4: the x8 masters box-averaged.
    assert_eq!((paper.image.width, paper.image.height), (64 * 4, 40 * 4));
    let t0 = p.get(tile(synthetic::OBJECTS)).unwrap();
    assert_eq!((t0.image.width, t0.image.height), (64, 64));
    let master = img::read_png(&w.slice(&atlas::slice_name(tile(synthetic::OBJECTS)))).unwrap();
    assert_eq!(t0.image, img::shrink(&master, 2));
    // A pack is never written into the repository.
    let inside = crate::work::repo_root().join("site/u6-hdpack.zip");
    assert!(pack::pack(&w, Some(&inside)).is_err());
    assert!(!inside.exists());
    let _ = std::fs::remove_dir_all(&w.root);
}

#[test]
fn ingest_refuses_a_sheet_made_from_other_art() {
    let g = synthetic::game();
    let w = work("changed");
    atlas_into(&g, &w);
    generate(&w, |_, sent| sent);
    // The owner's files changed between atlas and ingest.
    let mut other = g.clone();
    other.portraits[0].2[100] ^= 1;
    let r = ingest::ingest(&other, &w, &Thresholds::default()).unwrap();
    assert_eq!(r.refused_sheets, 1);
    assert_eq!(r.get(Class::Portrait, Status::Pending), 2);
    let _ = std::fs::remove_dir_all(&w.root);
}

#[test]
fn a_bitmap_below_scale_8_is_resampled() {
    let p = atlas::Place {
        item: 0,
        class: Class::Bitmap,
        piece: 0,
        image: "b:paper".into(),
        hash: String::new(),
        x: 0,
        y: 0,
        w: 320,
        h: 200,
        scale: 4,
    };
    let mut out = Rgba::new(SHEET_W, SHEET_H);
    img::fill(&mut out, 0, 0, 1280, 800, [10, 20, 30, 255]);
    let s = ingest::slice(&out, &p);
    assert_eq!((s.width, s.height), (2560, 1600));
    assert_eq!(img::get(&s, 2559, 1599), [10, 20, 30, 255]);
}

struct Counting {
    calls: AtomicUsize,
}

impl Runner for Counting {
    fn generate(&self, sheet: &Path, _prompt: &str, dir: &Path) -> Result<PathBuf, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let to = dir.join("made.png");
        std::fs::copy(sheet, &to).map_err(|e| e.to_string())?;
        Ok(to)
    }
}

#[test]
fn run_sends_nothing_without_send_and_resumes() {
    let g = synthetic::game();
    let w = work("run");
    let jobs = atlas_into(&g, &w);
    let fake = Counting {
        calls: AtomicUsize::new(0),
    };
    let mut out = Vec::new();
    let opts = |send, limit| RunOptions {
        send,
        limit,
        parallel: 2,
    };
    let s = run::run(&w, &fake, &opts(false, None), &mut out).unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    assert_eq!((s.pending, s.calls), (jobs.len(), 0));
    let text = String::from_utf8(out.clone()).unwrap();
    assert!(text.contains("sends the owner's game art"));
    assert!(text.contains("Nothing was sent"));
    assert!(!w.path("runs.jsonl").exists());
    // A trial of 2, then the rest: a done sheet is never sent again.
    let s = run::run(&w, &fake, &opts(true, Some(2)), &mut out).unwrap();
    assert_eq!((s.calls, s.ok), (2, 2));
    let s = run::run(&w, &fake, &opts(true, None), &mut out).unwrap();
    assert_eq!(s.calls, jobs.len() - 2);
    assert_eq!(fake.calls.load(Ordering::SeqCst), jobs.len());
    let s = run::run(&w, &fake, &opts(true, None), &mut out).unwrap();
    assert_eq!(s.calls, 0);
    assert!(jobs.iter().all(|j| w.out(&j.sheet).exists()));
    let runs = std::fs::read_to_string(w.path("runs.jsonl")).unwrap();
    assert_eq!(runs.lines().count(), jobs.len());
    let _ = std::fs::remove_dir_all(&w.root);
}

#[test]
fn prompts_name_the_layout_and_the_key() {
    let g = synthetic::game();
    let plan = group::plan(&g);
    for (job, _) in atlas::layout(&g, &numbered(&plan), 1, 0) {
        let p = prompt::text(&job);
        let key = format!("#{:02x}{:02x}{:02x}", job.key[0], job.key[1], job.key[2]);
        assert!(p.contains(&key), "{:?}", job.kind);
        assert!(p.contains("built-in image_gen"));
        assert!(p.contains("1536x1024"));
    }
}

#[test]
fn outputs_stay_outside_the_repository() {
    let repo = crate::work::repo_root();
    assert!(check_outside_repo(&repo.join("site/pack.zip")).is_err());
    assert!(check_outside_repo(&repo.join("target/../x/y")).is_err());
    assert!(Work::open(Some(&repo.join("hd"))).is_err());
    assert!(check_outside_repo(&std::env::temp_dir().join("u6-hd-elsewhere")).is_ok());
}

#[test]
fn thresholds_take_name_value() {
    let mut t = Thresholds::default();
    assert_eq!(t.structure, 0.9);
    t.set("structure=0.75").unwrap();
    t.set("motion=0.4").unwrap();
    assert_eq!((t.structure, t.motion), (0.75, 0.4));
    assert!(t.set("colour=1").is_err());
    assert!(t.set("iou").is_err());
    assert!(t.set("iou=x").is_err());
    assert!(t.text().contains("structure>=0.75"));
}

#[test]
fn the_civil_date() {
    assert_eq!(pack::date(0), "1970-01-01");
    assert_eq!(pack::date(1_792_108_800), "2026-10-16");
    assert_eq!(pack::date(951_782_400), "2000-02-29");
}

/// A stand-in for imagegen's repaint of a portrait, made in code: the classic image smoothed
/// (bilinear at x8, then a 3 x 3 box), its colours drifted, a light gradient, strands and folds
/// a few source pixels apart, and a fine deterministic texture added. It keeps where things are and in which colours, and changes
/// every edge, as the repaints on the synthetic sheets did.
fn painted(c: &Classic) -> Rgba {
    let (w, h) = (c.width * 8, c.height * 8);
    let mut s = Rgba::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let fx = ((x as f32 + 0.5) / 8.0 - 0.5).clamp(0.0, (c.width - 1) as f32);
            let fy = ((y as f32 + 0.5) / 8.0 - 0.5).clamp(0.0, (c.height - 1) as f32);
            let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
            let (x1, y1) = ((x0 + 1).min(c.width - 1), (y0 + 1).min(c.height - 1));
            let (tx, ty) = (fx.fract(), fy.fract());
            let at = |x, y| c.at(x, y).unwrap_or(KEY).map(f32::from);
            let (a, b, cc, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
            let mix = |i: usize| {
                let top = a[i] * (1.0 - tx) + b[i] * tx;
                let bot = cc[i] * (1.0 - tx) + d[i] * tx;
                top * (1.0 - ty) + bot * ty
            };
            // Brush strands and painted-in shading: edges two to four source pixels apart that
            // the classic image never had (the repaints add a nose, brows, folds and strands).
            let strand = ((x as f32 * 0.28 + y as f32 * 0.11).sin() * 30.0).round();
            let fold = ((y as f32 * 0.19).sin() * (x as f32 * 0.07).cos() * 24.0).round();
            let texture = ((x * 7 + y * 13) % 11) as f32 - 5.0 + strand + fold;
            let light = 10.0 - 20.0 * y as f32 / h as f32;
            let drift = [12.0, -8.0, 6.0];
            let px: [u8; 3] = std::array::from_fn(|i| {
                (mix(i) + drift[i] + light + texture)
                    .round()
                    .clamp(0.0, 255.0) as u8
            });
            img::put(&mut s, x, y, [px[0], px[1], px[2], 255]);
        }
    }
    let mut out = s.clone();
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let mut acc = [0u32; 3];
            for dy in -1..=1i64 {
                for dx in -1..=1i64 {
                    let xx = (x + dx).clamp(0, w as i64 - 1) as u32;
                    let yy = (y + dy).clamp(0, h as i64 - 1) as u32;
                    let p = img::get(&s, xx, yy);
                    for i in 0..3 {
                        acc[i] += u32::from(p[i]);
                    }
                }
            }
            let px = acc.map(|v| (v / 9) as u8);
            img::put(&mut out, x as u32, y as u32, [px[0], px[1], px[2], 255]);
        }
    }
    out
}

fn mirrored(img: &Rgba) -> Rgba {
    let mut out = img.clone();
    for y in 0..img.height {
        for x in 0..img.width {
            img::put(&mut out, x, y, img::get(img, img.width - 1 - x, y));
        }
    }
    out
}

/// Moved right by `dx` pixels, wrapping around (the colours stay the same).
fn rolled(img: &Rgba, dx: u32) -> Rgba {
    let mut out = img.clone();
    for y in 0..img.height {
        for x in 0..img.width {
            img::put(&mut out, (x + dx) % img.width, y, img::get(img, x, y));
        }
    }
    out
}

#[test]
fn portraits_are_judged_at_the_layout_scale() {
    let g = synthetic::game();
    let t = Thresholds::default();
    let id = |item| ImageId::Portrait { letter: b'a', item };
    let classic = [g.classic(id(0)).unwrap(), g.classic(id(1)).unwrap()];
    let out = [painted(&classic[0]), painted(&classic[1])];
    let judge = |c: &Classic, raw: &Rgba| gate::judge(raw, c, KEY, gate::Scale::Layout).1;
    for i in 0..2 {
        let real = judge(&classic[i], &out[i]);
        let mirror = judge(&classic[i], &mirrored(&out[i]));
        let shifted = judge(&classic[i], &rolled(&out[i], out[i].width / 8));
        let swapped = judge(&classic[i], &out[1 - i]);
        let source = gate::structure(&classic[i], &out[i], KEY);
        println!(
            "portrait {i}: layout real {:.3} (source scale {:.3}), mirrored {:.3} {:?}, \
             shifted 1/8 {:.3} {:?}, swapped {:.3} {:?}",
            real.structure.unwrap(),
            source.unwrap(),
            mirror.structure.unwrap(),
            mirror.failures(&t),
            shifted.structure.unwrap(),
            shifted.failures(&t),
            swapped.structure.unwrap(),
            swapped.failures(&t),
        );
        assert!(real.pass(&t), "{real:?}");
        // The source-scale edge measure fails the same repaint.
        assert!(source.unwrap() < t.structure);
        // Each control fails, and the layout correlation alone would catch it.
        for (what, m) in [
            ("mirrored", mirror),
            ("shifted", shifted),
            ("swapped", swapped),
        ] {
            assert!(!m.pass(&t), "{what} {i}: {m:?}");
            assert!(m.failures(&t).contains(&"structure"), "{what} {i}: {m:?}");
        }
    }
    // Bitmaps keep the source-scale gate.
    let paper = g.classic(ImageId::Bitmap(Role::Paper)).unwrap();
    let m = gate::judge(&paper.scaled(8, KEY), &paper, KEY, gate::Scale::Source).1;
    assert!(m.structure.unwrap() > 0.99);
    assert_eq!(ingest::scale_of(Class::Portrait), gate::Scale::Layout);
    assert_eq!(ingest::scale_of(Class::Bitmap), gate::Scale::Source);
}

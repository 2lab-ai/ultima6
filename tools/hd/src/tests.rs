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
use crate::work::{Work, check_target};
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

/// The stamp of a first attempt of `plan`.
fn stamp(plan: &group::Plan) -> atlas::Stamp {
    atlas::Stamp {
        attempt: 1,
        generation: "test".into(),
        plan: plan.fingerprint(),
    }
}

/// `g`'s atlas, written to `w`.
fn atlas_into(g: &Game, w: &Work) -> Vec<Job> {
    let plan = group::plan(g);
    let mut jobs = Vec::new();
    atlas::layout_each(g, &numbered(&plan), &stamp(&plan), 0, |job, sheet| {
        img::write_png(&w.sheet(&job.sheet), &sheet)?;
        jobs.push(job);
        Ok(())
    })
    .unwrap();
    w.append_jobs(&jobs).unwrap();
    jobs
}

/// The sheet `job` sends.
fn sent(w: &Work, job: &Job) -> Rgba {
    img::read_png(&w.sheet(&job.sheet), job.width, job.height).unwrap()
}

/// "Generates" every sheet not done with `f` (sheet sent -> sheet back), recorded as run
/// records a call's image (one of another size than its sheet included, for ingest to refuse).
fn generate(w: &Work, f: impl Fn(&Job, Rgba) -> Rgba) {
    for job in w.read_jobs().unwrap() {
        if run::output(w, &job).is_none() {
            let back = f(&job, sent(w, &job));
            run::record(w, &job, &img::encode_png(&back).unwrap()).unwrap();
        }
    }
}

/// Records `f`'s picture for `job` again (a changed output, as from another call).
fn regenerate(w: &Work, job: &Job, f: impl Fn(Rgba) -> Rgba) {
    let back = f(sent(w, job));
    run::record(w, job, &img::encode_png(&back).unwrap()).unwrap();
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
    let sheets = atlas::layout(&g, &numbered(&plan), &stamp(&plan), 0);
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
    let again = atlas::layout(&g, &numbered(&plan), &stamp(&plan), 0);
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
    let sheets = atlas::layout(&g, &items, &stamp(&plan), 0);
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
    let master = img::read_png(
        &w.slice(&atlas::slice_name(tile(synthetic::OBJECTS))),
        128,
        128,
    )
    .unwrap();
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
    // Each call: its reservation, synced before it was made, and its result.
    let runs = std::fs::read_to_string(w.path("runs.jsonl")).unwrap();
    let count = |event: &str| {
        runs.lines()
            .filter(|l| l.contains(&format!("\"event\":\"{event}\"")))
            .count()
    };
    assert_eq!(
        (count("dispatched"), count("result")),
        (jobs.len(), jobs.len())
    );
    assert_eq!(runs.lines().count(), 2 * jobs.len());
    let _ = std::fs::remove_dir_all(&w.root);
}

#[test]
fn prompts_name_the_layout_and_the_key() {
    let g = synthetic::game();
    let plan = group::plan(&g);
    for (job, _) in atlas::layout(&g, &numbered(&plan), &stamp(&plan), 0) {
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
    assert!(check_target(&repo.join("site/pack.zip")).is_err());
    assert!(check_target(&repo.join("target/../x/y")).is_err());
    assert!(Work::open(Some(&repo.join("hd"))).is_err());
    assert!(check_target(&std::env::temp_dir().join("u6-hd-elsewhere")).is_ok());
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

#[test]
fn motion_is_gated_on_every_moving_pair() {
    // Grey frames: classic b is 20 levels lighter than a; an HD b at +40 keeps 2x the change,
    // one equal to its a keeps none.
    let grey = |v: u8| Classic {
        width: 16,
        height: 16,
        px: vec![Some([v, v, v]); 256],
    };
    let hd = |v: u8| grey(v).scaled(8, KEY);
    let (a, b) = (grey(100), grey(120));
    let kept_twice = (a.clone(), b.clone(), hd(100), hd(140));
    let kept_none = (a.clone(), b.clone(), hd(100), hd(100));
    let pairs = vec![
        kept_twice.clone(),
        kept_none.clone(),
        kept_none.clone(),
        kept_none,
    ];
    // The mean of 2, 0, 0, 0 would pass a 0.5 gate; the minimum does not.
    assert_eq!(gate::motion(&pairs), Some(0.0));
    assert!((gate::motion(&[kept_twice]).unwrap() - 2.0).abs() < 1e-3);
    assert!(gate::motion(&pairs).unwrap() < Thresholds::default().motion);
}

#[test]
fn nonfinite_gates_and_metrics_fail_closed() {
    let mut t = Thresholds::default();
    for bad in [
        "structure=NaN",
        "structure=inf",
        "shift=-1",
        "iou=1.5",
        "purity=-0.1",
        "motion=3",
        "flat=NaN",
        "seam=-inf",
    ] {
        assert!(t.set(bad).is_err(), "{bad}");
    }
    assert_eq!(t, Thresholds::default());
    for m in [
        gate::Metrics {
            structure: Some(f32::NAN),
            ..Default::default()
        },
        gate::Metrics {
            shift: Some(f32::NAN),
            ..Default::default()
        },
        gate::Metrics {
            iou: Some(f32::INFINITY),
            ..Default::default()
        },
        gate::Metrics {
            purity: Some(f32::NAN),
            ..Default::default()
        },
        gate::Metrics {
            flat: Some(f32::NAN),
            ..Default::default()
        },
    ] {
        assert!(!m.pass(&t), "{m:?}");
    }
    assert!(!gate::seam_passes((f32::NAN, 1.5)));
}

#[test]
fn a_flat_source_must_stay_flat() {
    let f = flat();
    let t = Thresholds::default();
    // Grain inside each source pixel (every 8 x 8 block keeps its mean): passes.
    let mut grain = f.scaled(8, KEY);
    for y in 0..128 {
        for x in 0..128 {
            let d: i32 = if (x + y) % 2 == 0 { 20 } else { -20 };
            let v = (90 + d) as u8;
            img::put(&mut grain, x, y, [v, v, v, 255]);
        }
    }
    let m = gate::judge(&grain, &f, KEY, gate::Scale::Source).1;
    assert_eq!(
        (m.structure, m.shift, m.iou, m.purity),
        (None, None, None, None)
    );
    // Grain of ±20 inside each pixel: 5 levels (a quarter of its own spread), passes.
    assert!(m.flat.unwrap() <= 5.0 && m.pass(&t), "{m:?}");
    // An invented checker of source pixels on the flat tile: fails.
    let mut checker = f.scaled(8, KEY);
    for y in 0..128 {
        for x in 0..128 {
            let v = if (x / 8 + y / 8) % 2 == 0 { 60 } else { 120 };
            img::put(&mut checker, x, y, [v, v, v, 255]);
        }
    }
    let m = gate::judge(&checker, &f, KEY, gate::Scale::Source).1;
    assert_eq!(m.failures(&t), ["flat"], "{m:?}");
    // Any image with something to measure has no flatness metric.
    assert_eq!(gate::flatness(&sprite(), &repaint(&sprite())), None);
}

#[test]
fn hashes_are_the_engines() {
    use u6_core::converse::{ConverseText, Msg, portrait_pixels, portrait_ref};
    use u6_formats::bitmap::Bitmap;
    use u6_formats::lzw;
    use u6_formats::portrait::PORTRAIT_LEN;
    use u6_hdpack::{image_hash, tile_hash};
    // Records: the tool's copy of an engine record hashes as the engine record does, after a
    // change to it in place (a merged shore).
    let mut engine = u6_core::gfx::GfxState::default();
    engine.records[5].pixels[17] = 9;
    engine.records[5].transparent[17] = false;
    engine.records[5].transparent[18] = false;
    let mut g = synthetic::game();
    g.records = engine.records.iter().map(crate::game::Record::of).collect();
    for r in [0u16, 5, 2047] {
        let e = &engine.records[usize::from(r)];
        assert_eq!(
            g.hash(tile(r)),
            tile_hash(&e.pixels, &e.transparent),
            "record {r}"
        );
    }
    // Portraits: the tool lists an archive's items and hashes the pixels the engine's
    // portrait_pixels reads for each, by the (letter, item) the engine's portrait_ref names.
    let images: Vec<Vec<u8>> = (0..3u8)
        .map(|k| {
            (0..PORTRAIT_LEN)
                .map(|i| (i as u8).wrapping_mul(k + 3))
                .collect()
        })
        .collect();
    let items: Vec<Vec<u8>> = images.iter().map(|i| lzw::encode(i).unwrap()).collect();
    let mut lib = Vec::new();
    let mut at = 4 * items.len() as u32;
    for it in &items {
        lib.extend_from_slice(&at.to_le_bytes());
        at += it.len() as u32;
    }
    for it in &items {
        lib.extend_from_slice(it);
    }
    let name = b"facefile.?".to_vec();
    let files = u6_core::Files::new([("DISK/facefile.a".to_string(), lib)].into_iter().collect());
    let mut state = u6_core::State::empty();
    state.converse.text = Some(ConverseText::from_pairs([(Msg::PortraitFile, name)]));
    let ours = crate::game::portraits(&state, &files);
    assert_eq!(ours.len(), 3);
    g.portraits = ours;
    for item in 0..3u16 {
        // Slot s reads item s - 1 of archive a (slot 0 reads item 0; slot 1 is the hero's).
        let npc = if item == 0 { 0 } else { item as i16 + 1 };
        assert_eq!(portrait_ref(&state, npc), Some((b'a', item)));
        let theirs = portrait_pixels(&state, &files, b'a', item).unwrap();
        assert_eq!(theirs, images[usize::from(item)]);
        let id = ImageId::Portrait { letter: b'a', item };
        assert_eq!(
            g.hash(id),
            Some(image_hash(56, 64, theirs.iter().copied())),
            "{item}"
        );
    }
    // Bitmaps: the bytes the engine's reader returns.
    let mut raw = vec![5, 0, 3, 0];
    raw.extend((0..15u8).map(|i| i * 7));
    let file = lzw::encode(&raw).unwrap();
    let bmp = Bitmap::from_lzw(&file).unwrap();
    g.bitmaps = vec![(Role::Paper, bmp.width, bmp.height, bmp.pixels.clone())];
    assert_eq!(
        g.hash(ImageId::Bitmap(Role::Paper)),
        Some(image_hash(5, 3, bmp.pixels.iter().copied()))
    );
}

/// A temp dir for one test, outside the repository.
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("u6-hdtool-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

#[test]
fn writes_are_checked_where_they_land() {
    use crate::work::{ensure_dir, write_file};
    use std::os::unix::fs::symlink;
    let repo = crate::work::repo_root().canonicalize().unwrap();
    let t = temp("contain");
    let decoy = repo
        .join("site")
        .join(format!("u6-hd-decoy-{}.zip", std::process::id()));
    // A symlinked output file pointing into the repository: refused, nothing written there.
    symlink(&decoy, t.join("out.zip")).unwrap();
    assert!(write_file(&t.join("out.zip"), b"x").is_err());
    assert!(!decoy.exists());
    // A symlinked ancestor into the repository with a suffix that does not exist yet.
    symlink(repo.join("site"), t.join("link")).unwrap();
    assert!(write_file(&t.join("link/new/deeper/file.png"), b"x").is_err());
    assert!(ensure_dir(&t.join("link/new")).is_err());
    assert!(!repo.join("site/new").exists());
    // `..` anywhere.
    assert!(write_file(&t.join("a/../b.png"), b"x").is_err());
    assert!(Work::open(Some(&t.join("w/../w"))).is_err());
    // A work dir's child symlinked into the repository (`sheets`).
    let w = t.join("work");
    std::fs::create_dir_all(&w).unwrap();
    symlink(repo.join("site"), w.join("sheets")).unwrap();
    let work = Work::open(Some(&w)).unwrap();
    assert!(img::write_png(&work.sheet("s0000-a1"), &Rgba::new(2, 2)).is_err());
    assert!(work.mkdirs().is_err());
    assert!(!repo.join("site/s0000-a1.png").exists());
    // Any git work tree: a directory with a `.git` entry above the target.
    std::fs::create_dir_all(t.join("other/repo")).unwrap();
    std::fs::write(t.join("other/repo/.git"), "gitdir: elsewhere\n").unwrap();
    assert!(Work::open(Some(&t.join("other/repo/hd"))).is_err());
    assert!(write_file(&t.join("other/repo/x/y.png"), b"x").is_err());
    // The primary checkout and every linked worktree of this repository.
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8(out.stdout).unwrap()
    };
    let common =
        PathBuf::from(git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]).trim());
    let mut trees = vec![common.parent().unwrap().to_path_buf()];
    trees.extend(
        git(&["worktree", "list", "--porcelain"])
            .lines()
            .filter_map(|l| l.strip_prefix("worktree "))
            .map(PathBuf::from),
    );
    assert!(trees.len() >= 2);
    for tree in &trees {
        assert!(
            Work::open(Some(&tree.join("hd-out"))).is_err(),
            "{}",
            tree.display()
        );
        assert!(check_target(&tree.join("deep/new/file.zip")).is_err());
    }
    // A plain write outside: lands, and a planted temp-name symlink is never followed.
    let target = repo
        .join("site")
        .join(format!("u6-hd-planted-{}", std::process::id()));
    symlink(&target, t.join("free.zip.tmp")).unwrap();
    write_file(&t.join("free.zip"), b"ok").unwrap();
    assert_eq!(std::fs::read(t.join("free.zip")).unwrap(), b"ok");
    assert!(!target.exists());
    let _ = std::fs::remove_dir_all(&t);
}

#[test]
fn resume_is_bound_to_the_job_and_a_valid_output() {
    let g = synthetic::game();
    let w = work("resume");
    let jobs = atlas_into(&g, &w);
    generate(&w, |_, s| s);
    assert!(jobs.iter().all(|j| run::output(&w, j).is_some()));
    let j0 = &jobs[0];
    // A changed output (bytes no longer the receipt's): not done.
    let mut png = std::fs::read(w.out(&j0.sheet)).unwrap();
    png.truncate(png.len() / 2);
    std::fs::write(w.out(&j0.sheet), &png).unwrap();
    assert!(run::output(&w, j0).is_none());
    // A receipt for another attempt, sheet or generation: not done.
    let j1 = &jobs[1];
    let mut other = j1.clone();
    other.attempt = 2;
    assert!(run::output(&w, &other).is_none());
    other = j1.clone();
    other.generation = "rebuilt".into();
    assert!(run::output(&w, &other).is_none());
    // A changed sheet (what is sent): not done.
    let mut sheet = sent(&w, j1);
    img::put(&mut sheet, 0, 0, [1, 2, 3, 255]);
    img::write_png(&w.sheet(&j1.sheet), &sheet).unwrap();
    assert!(run::output(&w, j1).is_none());
    // A call whose image is not a PNG, or is of the sheet's size and does not decode, records
    // nothing: the earlier output stays.
    let before = std::fs::read(w.out(&jobs[2].sheet)).unwrap();
    let (jw, jh) = (jobs[2].width, jobs[2].height);
    // A PNG whose data is damaged past its header (the IDAT bytes before IEND).
    let damaged = |img: &Rgba| {
        let mut b = img::encode_png(img).unwrap();
        let n = b.len();
        for x in &mut b[n - 24..n - 12] {
            *x ^= 0x5a;
        }
        b
    };
    assert!(run::record(&w, &jobs[2], b"\x89PNG\r\n\x1a\nnot really").is_err());
    let e = run::record(&w, &jobs[2], &damaged(&Rgba::new(jw, jh))).unwrap_err();
    assert!(e.contains("does not decode"), "{e}");
    assert_eq!(std::fs::read(w.out(&jobs[2].sheet)).unwrap(), before);
    assert!(
        run::output(&w, &jobs[2]).is_some(),
        "the earlier output stays"
    );
    // One of another size is recorded as it is, not decoded (damaged data and all), its
    // receipt holding its header size: the attempt is spent, and ingest refuses it.
    for (dw, dh) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
        let (ow, oh) = (jw.saturating_add_signed(dw), jh.saturating_add_signed(dh));
        let png = damaged(&Rgba::new(ow, oh));
        assert_eq!(run::record(&w, &jobs[2], &png), Ok(false));
        let (got, gw, gh) = run::output(&w, &jobs[2]).unwrap();
        assert_eq!((got, gw, gh), (png, ow, oh));
    }
    // The fake runner sees exactly the jobs that are not done.
    let fake = Counting {
        calls: AtomicUsize::new(0),
    };
    let opts = RunOptions {
        send: true,
        limit: None,
        parallel: 2,
    };
    let s = run::run(&w, &fake, &opts, &mut Vec::new()).unwrap();
    assert_eq!((s.pending, s.calls), (2, 2));
    let _ = std::fs::remove_dir_all(&w.root);
}

/// The names `ingest.json` lists.
fn accepted(w: &Work) -> std::collections::BTreeSet<String> {
    let list: Vec<ingest::Accepted> =
        serde_json::from_slice(&std::fs::read(w.path("ingest.json")).unwrap()).unwrap();
    list.into_iter().map(|a| a.image).collect()
}

/// The ids in the decoded pack, after `pack`.
fn packed(w: &Work) -> std::collections::BTreeSet<String> {
    pack::pack(w, None).unwrap();
    let p = read_pack(&std::fs::read(w.path("pack/u6-hdpack.zip")).unwrap()).unwrap();
    p.images.iter().map(|i| atlas::image_name(i.id)).collect()
}

/// The names of `item`'s records that draw something.
fn members(g: &Game, item: &Item) -> std::collections::BTreeSet<String> {
    item.images()
        .filter(|&id| g.classic(id).is_some_and(|c| c.has_opaque()))
        .map(atlas::image_name)
        .collect()
}

#[test]
fn a_group_accepted_before_is_revoked_whole() {
    let g = synthetic::game();
    let plan = group::plan(&g);
    let w = work("revoke");
    let jobs = atlas_into(&g, &w);
    generate(&w, |_, s| s);
    let t = Thresholds::default();
    ingest::ingest(&g, &w, &t).unwrap();
    let all = accepted(&w);
    assert_eq!(packed(&w), all);
    let actor = of_class(&plan, Class::Actor)[0];
    let tree = of_class(&plan, Class::Multi)[0];
    let (actor_ids, tree_ids) = (members(&g, actor), members(&g, tree));
    assert!(actor_ids.is_subset(&all) && tree_ids.is_subset(&all));
    // One frame of the actor and one part of the tree come back wrong from another call.
    let tile_job = jobs.iter().find(|j| j.kind == SheetKind::Tile).unwrap();
    let at = |id: ImageId| {
        let p = tile_job
            .places
            .iter()
            .find(|p| p.image == atlas::image_name(id))
            .unwrap();
        (p.x, p.y)
    };
    let frame = at(tile(synthetic::ACTOR_A + 5));
    let part = at(tile(synthetic::TREE + 2));
    regenerate(&w, tile_job, |mut s| {
        img::fill(&mut s, frame.0, frame.1, CELL, CELL / 2, [10, 10, 10, 255]);
        img::fill(&mut s, part.0, part.1, CELL, CELL, [250, 250, 250, 255]);
        s
    });
    ingest::ingest(&g, &w, &t).unwrap();
    let now = accepted(&w);
    let want: std::collections::BTreeSet<String> = all
        .difference(&actor_ids)
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .difference(&tree_ids)
        .cloned()
        .collect();
    assert_eq!(now, want);
    assert_eq!(packed(&w), want);
    // Their masters are gone from the work dir too.
    for name in actor_ids.iter().chain(&tree_ids) {
        let id = atlas::parse_image(name).unwrap();
        assert!(!w.slice(&atlas::slice_name(id)).exists(), "{name}");
    }
    // A motion-only failure: the same outputs as at first, a stricter motion gate.
    let w2 = work("revoke-motion");
    atlas_into(&g, &w2);
    generate(&w2, |_, s| s);
    ingest::ingest(&g, &w2, &t).unwrap();
    let before = accepted(&w2);
    let mut strict = t;
    strict.set("motion=1.5").unwrap();
    ingest::ingest(&g, &w2, &strict).unwrap();
    // The walker's frame pairs move and fail; the facing-only beast has no moving pair.
    let walker = members(&g, of_class(&plan, Class::Actor)[0]);
    let want: std::collections::BTreeSet<String> = before.difference(&walker).cloned().collect();
    assert!(members(&g, of_class(&plan, Class::Actor)[1]).is_subset(&want));
    assert_eq!(accepted(&w2), want);
    assert_eq!(packed(&w2), want);
    let _ = std::fs::remove_dir_all(&w.root);
    let _ = std::fs::remove_dir_all(&w2.root);
}

/// The synthetic game with one more actor type of `frames` frames (records from 400).
fn with_big_actor(frames: u16) -> Game {
    let mut g = synthetic::game();
    let base = 400u16;
    let sample = g.records[usize::from(synthetic::ACTOR_A)].clone();
    for f in 0..frames {
        let mut r = sample.clone();
        // Each frame its own: one pixel of the body moves along.
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
    g
}

#[test]
fn an_actor_over_two_sheets_is_one_group() {
    let g = with_big_actor(100);
    let plan = group::plan(&g);
    let big = plan
        .items
        .iter()
        .position(|i| i.class == Class::Actor && i.pieces.len() == 100)
        .unwrap() as u32;
    let ids = members(&g, &plan.items[big as usize]);
    assert_eq!(ids.len(), 100);
    let w = work("big-actor");
    let jobs = atlas_into(&g, &w);
    let sheets: Vec<&Job> = jobs
        .iter()
        .filter(|j| j.places.iter().any(|p| p.item == big))
        .collect();
    assert_eq!(sheets.len(), 2);
    // Every frame on exactly one place.
    let mut placed: Vec<&str> = sheets
        .iter()
        .flat_map(|j| {
            j.places
                .iter()
                .filter(|p| p.item == big)
                .map(|p| p.image.as_str())
        })
        .collect();
    placed.sort_unstable();
    assert_eq!(placed.len(), 100);
    placed.dedup();
    assert_eq!(placed.len(), 100);
    let t = Thresholds::default();
    // Only the first sheet back: pending, nothing accepted, nothing re-rolled.
    let (first, second) = (sheets[0].clone(), sheets[1].clone());
    let others: Vec<Job> = jobs
        .iter()
        .filter(|j| j.sheet != second.sheet)
        .cloned()
        .collect();
    for j in &others {
        regenerate(&w, j, |s| s);
    }
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert!(accepted(&w).is_disjoint(&ids));
    assert_eq!(r.rerolled_items, 0);
    // The second comes back the wrong size: refused, the whole group queued for its re-roll
    // (with every other item that sheet held).
    let small = img::encode_png(&Rgba::new(SHEET_W - 2, SHEET_H)).unwrap();
    assert!(!run::record(&w, &second, &small).unwrap());
    let r = ingest::ingest(&g, &w, &t).unwrap();
    let on_second: std::collections::BTreeSet<u32> = second.places.iter().map(|p| p.item).collect();
    assert_eq!(r.rerolled_items, on_second.len());
    assert!(accepted(&w).is_disjoint(&ids));
    let rerolls: Vec<Job> = w
        .read_jobs()
        .unwrap()
        .into_iter()
        .filter(|j| j.attempt == 2)
        .collect();
    assert!(rerolls.len() >= 2);
    let again: usize = rerolls
        .iter()
        .map(|j| j.places.iter().filter(|p| p.item == big).count())
        .sum();
    assert_eq!(again, 100);
    // The re-roll with one frame painted over: the group falls back whole.
    let victim = rerolls
        .iter()
        .flat_map(|j| j.places.iter().map(move |p| (j, p)))
        .find(|(_, p)| p.item == big)
        .map(|(j, p)| (j.sheet.clone(), p.x, p.y))
        .unwrap();
    for j in &rerolls {
        regenerate(&w, j, |mut s| {
            if j.sheet == victim.0 {
                img::fill(
                    &mut s,
                    victim.1,
                    victim.2,
                    CELL,
                    CELL / 2,
                    [10, 10, 10, 255],
                );
            }
            s
        });
    }
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert!(accepted(&w).is_disjoint(&ids));
    assert_eq!(r.get(Class::Actor, Status::Fallback), 100);
    let _ = first;
    // A clean run of the same atlas: the group is generated, each of its frames listed once
    // (and no image listed twice), and every one of them in the pack as it decodes.
    let w2 = work("big-actor-clean");
    atlas_into(&g, &w2);
    generate(&w2, |_, s| s);
    let r = ingest::ingest(&g, &w2, &t).unwrap();
    assert_eq!(r.get(Class::Actor, Status::Generated), 100 + 12);
    let list: Vec<ingest::Accepted> =
        serde_json::from_slice(&std::fs::read(w2.path("ingest.json")).unwrap()).unwrap();
    let mut all: Vec<&str> = list.iter().map(|a| a.image.as_str()).collect();
    all.sort_unstable();
    let n = all.len();
    all.dedup();
    assert_eq!(all.len(), n, "an image listed twice");
    let mine: Vec<&str> = all.into_iter().filter(|n| ids.contains(*n)).collect();
    let want: Vec<&str> = ids.iter().map(String::as_str).collect();
    assert_eq!(mine, want);
    let in_pack = packed(&w2);
    assert!(
        ids.is_subset(&in_pack),
        "{:?}",
        ids.difference(&in_pack).count()
    );
    let _ = std::fs::remove_dir_all(&w.root);
    let _ = std::fs::remove_dir_all(&w2.root);
}

#[test]
fn a_moved_place_in_the_jobs_is_refused() {
    let g = synthetic::game();
    let w = work("topology");
    let mut jobs = atlas_into(&g, &w);
    generate(&w, |_, s| s);
    // Two parts of the tree swapped in the cell map (the sheet and its output unchanged).
    let tile_job = jobs.iter_mut().find(|j| j.kind == SheetKind::Tile).unwrap();
    let find = |j: &Job, id: ImageId| {
        j.places
            .iter()
            .position(|p| p.image == atlas::image_name(id))
            .unwrap()
    };
    let (a, b) = (
        find(tile_job, tile(synthetic::TREE)),
        find(tile_job, tile(synthetic::TREE + 3)),
    );
    let (pa, pb) = (tile_job.places[a].clone(), tile_job.places[b].clone());
    tile_job.places[a].x = pb.x;
    tile_job.places[a].y = pb.y;
    tile_job.places[b].x = pa.x;
    tile_job.places[b].y = pa.y;
    w.write_jobs(&jobs).unwrap();
    ingest::ingest(&g, &w, &Thresholds::default()).unwrap();
    let plan = group::plan(&g);
    let tree = members(&g, of_class(&plan, Class::Multi)[0]);
    assert!(accepted(&w).is_disjoint(&tree));
    // Jobs of another plan: refused whole.
    let mut other = w.read_jobs().unwrap();
    other[0].plan = "another".into();
    w.write_jobs(&other).unwrap();
    assert!(ingest::ingest(&g, &w, &Thresholds::default()).is_err());
    let _ = std::fs::remove_dir_all(&w.root);
}

#[test]
fn a_rearranged_bitmap_falls_back() {
    let g = synthetic::game();
    let w = work("wrong-picture");
    atlas_into(&g, &w);
    let paper = g.classic(ImageId::Bitmap(Role::Paper)).unwrap();
    let (pw, ph) = (paper.width * 8, paper.height * 8);
    // The paper comes back with its left and right halves exchanged: the same size, colours
    // and histogram, the layout wrong. Everything else comes back as sent.
    let swap = |job: &Job, s: Rgba| {
        if !job.places.iter().any(|p| p.image == "b:paper") {
            return s;
        }
        let mut o = s.clone();
        let half = pw / 2;
        o.paste(&s.rect(half, 0, half, ph), 0, 0);
        o.paste(&s.rect(0, 0, half, ph), half, 0);
        o
    };
    let t = Thresholds::default();
    generate(&w, swap);
    ingest::ingest(&g, &w, &t).unwrap();
    generate(&w, swap);
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert_eq!(r.get(Class::Bitmap, Status::Fallback), 1);
    assert_eq!(
        r.get(Class::Bitmap, Status::Generated),
        1,
        "the spellbook passes"
    );
    assert!(!accepted(&w).contains("b:paper"));
    assert!(accepted(&w).contains("b:spellbook"));
    let p = packed(&w);
    assert!(!p.contains("b:paper") && p.contains("b:spellbook"));
    let _ = std::fs::remove_dir_all(&w.root);
}

#[test]
fn overlapping_mosaics_are_one_item() {
    let mut g = synthetic::game();
    // Tiles 300, 301, 302 drawn as two double-width objects that share 301 (anchor 302 takes
    // 301, anchor 301 takes 300).
    let rec = g.records[usize::from(synthetic::OBJECTS)].clone();
    for r in [300usize, 301, 302] {
        g.records[r] = rec.clone();
        g.records[r].pixels[0] = r as u8 % 31;
        g.records[r].transparent[0] = false;
    }
    g.flags2[301] = crate::game::DOUBLE_WIDTH;
    g.flags2[302] = crate::game::DOUBLE_WIDTH;
    // An empty part named by two anchors: 320 is empty, 321 and 322 are double height over it
    // (322's part is 321, 321's part is 320).
    g.records[321] = rec.clone();
    g.records[322] = rec;
    g.flags2[321] = crate::game::DOUBLE_HEIGHT;
    g.flags2[322] = crate::game::DOUBLE_HEIGHT;
    let plan = group::plan(&g);
    assert_eq!(plan.overlaps, 2);
    let find = |r: u16| {
        plan.items
            .iter()
            .position(|i| i.images().any(|id| id == tile(r)))
            .unwrap()
    };
    assert_eq!(find(300), find(302));
    assert_eq!(find(320), find(322));
    let item = &plan.items[find(300)];
    assert_eq!(item.class, Class::Multi);
    let mut ids: Vec<ImageId> = item.images().collect();
    ids.sort();
    assert_eq!(ids, [tile(300), tile(301), tile(302)]);
    // No record in two pieces anywhere.
    let mut all: Vec<ImageId> = plan.items.iter().flat_map(|i| i.images()).collect();
    let n = all.len();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), n);
}

/// A `codex` stand-in that leaves `sleep 100` running with the event pipe open (its pid in
/// `pid`), then waits `wait` seconds (or prints a thread event and exits at once when 0).
fn lingering_stub(dir: &Path, wait: u32) -> PathBuf {
    let p = dir.join("codex");
    let body = format!(
        "#!/bin/sh\ncat > /dev/null\nsleep 100 &\necho $! > \"{}\"\n\
         echo '{{\"type\":\"thread.started\",\"thread_id\":\"t-1\"}}'\nsleep {wait}\n",
        dir.join("pid").display()
    );
    std::fs::write(&p, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn alive(pid_file: &Path) -> bool {
    let pid = std::fs::read_to_string(pid_file).unwrap();
    std::process::Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[test]
fn one_deadline_covers_the_call_and_what_it_started() {
    use std::time::{Duration, Instant};
    let t = temp("deadline");
    let codex = |wait, timeout| run::Codex {
        bin: lingering_stub(&t, wait),
        home: t.join("codex-home"),
        timeout: Duration::from_secs(timeout),
        grace: run::CLEANUP,
    };
    let dir = crate::work::ensure_dir(&t.join("call")).unwrap();
    std::fs::write(t.join("sheet.png"), b"x").unwrap();
    // The call's child, killed with it, is gone in a moment: looked for up to 10 s, slack for
    // a loaded machine (one that outlived the call would sleep on for its 100 s).
    let child_gone = || {
        let start = Instant::now();
        while alive(&t.join("pid")) {
            if start.elapsed() > Duration::from_secs(10) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    };
    // A call that hangs: stopped at the deadline, its whole group killed. Its stand-in must
    // have started its child, and written its pid, before the deadline kills the group: 4 s,
    // as keeps-both's in the leak test, is slack for a loaded machine, not a bound on its load.
    let secs = 4;
    let start = Instant::now();
    let r = codex(100, secs).generate(&t.join("sheet.png"), "prompt", &dir);
    assert!(r.unwrap_err().contains("timed out"));
    assert!(
        start.elapsed() < Duration::from_secs(15),
        "{:?}",
        start.elapsed()
    );
    assert!(
        t.join("pid").exists(),
        "its stand-in had not written its child's pid by the {secs} s deadline"
    );
    assert!(child_gone(), "the call's child outlived it");
    // A call that exits but leaves a child holding its output: the child goes with it and
    // the event stream ends (here with no image, so the call fails, promptly).
    let start = Instant::now();
    let r = codex(0, 30).generate(&t.join("sheet.png"), "prompt", &dir);
    assert_eq!(r.unwrap_err(), "no image generated");
    assert!(
        start.elapsed() < Duration::from_secs(15),
        "{:?}",
        start.elapsed()
    );
    assert!(child_gone());
    // The events were kept, with the thread line.
    let events = std::fs::read_to_string(dir.join("events.jsonl")).unwrap();
    assert!(events.contains("thread.started"));
    let _ = std::fs::remove_dir_all(&t);
}

#[test]
fn an_actor_span_is_its_frame_count() {
    let mut g = synthetic::game();
    // Another type's base tile among the walker's frames (as an object sharing its tiles).
    let free = g
        .basetile
        .iter()
        .position(|&b| b == synthetic::END)
        .unwrap();
    g.basetile[free] = synthetic::ACTOR_A + 4;
    let plan = group::plan(&g);
    let actor = of_class(&plan, Class::Actor)[0];
    assert_eq!(actor.pieces.len(), 8, "the walker's 8 frames stay one item");
    assert_eq!(
        actor.layout,
        Some(group::Layout {
            base: synthetic::ACTOR_A,
            frames: 8,
            per_facing: Some(2)
        })
    );
    assert_eq!(plan.split_actors(&g), 0);
    assert_eq!(group::actor_layout(synthetic::WALKER), Some((8, 2)));
    assert_eq!(group::actor_layout(0x178), Some((16, 4)));
    assert_eq!(group::actor_layout(0x1b0), Some((16, 2)));
    assert_eq!(group::actor_layout(0x100), None);
    // A record of the span taken by another item first is a split, and is counted.
    let mut g2 = synthetic::game();
    g2.in_chunks[usize::from(synthetic::ACTOR_A + 2)] = true;
    g2.records[usize::from(synthetic::ACTOR_A + 2)].map = true;
    let plan2 = group::plan(&g2);
    assert_eq!(plan2.split_actors(&g2), 1);
}

#[test]
fn actor_types_whose_frames_overlap_are_one_item() {
    let base = |g: &Game| {
        let plan = group::plan(g);
        let actors: Vec<Item> = of_class(&plan, Class::Actor).into_iter().cloned().collect();
        (plan, actors)
    };
    let (plan, actors) = base(&synthetic::game());
    assert_eq!((actors.len(), plan.actor_overlaps), (2, 0));
    // A type of four facings of one (0x167) based at the walker's seventh frame: its frames
    // run past the walker's into the beast's. All three are one item of their 12 records.
    let mut g = synthetic::game();
    g.basetile[0x167] = synthetic::ACTOR_A + 6;
    g.actor_types.push(0x167);
    let (plan, actors) = base(&g);
    assert_eq!(plan.actor_overlaps, 2);
    assert_eq!(actors.len(), 1);
    let mut images: Vec<ImageId> = actors[0].images().collect();
    images.sort_by_key(|id| format!("{id:?}"));
    let want: Vec<ImageId> = (synthetic::ACTOR_A..synthetic::ACTOR_B + 4)
        .map(tile)
        .collect();
    assert_eq!(images.len(), want.len());
    assert!(want.iter().all(|id| images.contains(id)));
    assert_eq!(
        actors[0].layout,
        Some(group::Layout {
            base: synthetic::ACTOR_A,
            frames: 12,
            per_facing: None
        })
    );
    assert_eq!(plan.split_actors(&g), 0);
    // The beast's double-width frames keep their composition.
    assert!(actors[0].pieces.iter().any(|p| p.w == 2));
    // A type inside the walker's frames only (0x164, two frames): joined to the walker.
    let mut g = synthetic::game();
    g.basetile[0x164] = synthetic::ACTOR_A + 2;
    g.actor_types.push(0x164);
    let (plan, actors) = base(&g);
    assert_eq!((actors.len(), plan.actor_overlaps), (2, 1));
    assert_eq!(actors[0].images().count(), 8);
    assert_eq!(actors[0].layout.unwrap().per_facing, None);
    assert_eq!(actors[1].layout, base(&synthetic::game()).1[1].layout);
}

/// The ingest-side record of one judged image, for the group gates.
fn judged(piece: u32, id: ImageId, classic: Classic, hd: Rgba) -> ingest::Judged {
    ingest::Judged {
        sheet: "s".into(),
        place: atlas::Place {
            item: 0,
            class: Class::Actor,
            piece,
            image: atlas::image_name(id),
            hash: String::new(),
            x: piece * CELL,
            y: 0,
            w: 16,
            h: 16,
            scale: 8,
        },
        id,
        classic,
        hd,
        metrics: gate::Metrics::default(),
    }
}

#[test]
fn a_turn_is_not_motion() {
    let g = synthetic::game();
    let plan = group::plan(&g);
    let walker = of_class(&plan, Class::Actor)[0];
    let a = synthetic::ACTOR_A;
    let classic = |f: u16| g.classic(tile(a + f)).unwrap();
    // Every frame repainted facing the first way with its own step: each facing's step is kept
    // whole, every turn is lost.
    let facing_lost: Vec<ingest::Judged> = (0..8u16)
        .map(|f| {
            let drawn = Classic::from_record(&synthetic::figure(0, usize::from(f % 2), 10));
            let hd = gate::key_out(&drawn.scaled(8, KEY), &classic(f), KEY);
            judged(u32::from(f), tile(a + f), classic(f), hd)
        })
        .collect();
    let m = ingest::motion_of(walker, &facing_lost).unwrap();
    assert!(m > 0.95, "within-facing motion {m}");
    // The same frames paired in order, turns included, would fail the gate.
    let mut in_order = walker.clone();
    in_order.layout = in_order.layout.map(|l| group::Layout {
        per_facing: None,
        ..l
    });
    let all = ingest::motion_of(&in_order, &facing_lost).unwrap();
    assert!(
        all < Thresholds::default().motion,
        "consecutive pairs {all}"
    );
    // Each step kept at 30%: fails either way (the review's probe).
    let damped: Vec<ingest::Judged> = (0..8u16)
        .map(|f| {
            let c = classic(f);
            let base = classic(f - f % 2);
            let mut hd = base.scaled(8, KEY);
            if f % 2 == 1 {
                let full = c.scaled(8, KEY);
                for (o, (b, s)) in hd
                    .pixels
                    .iter_mut()
                    .zip(base.scaled(8, KEY).pixels.iter().zip(&full.pixels))
                {
                    *o = (f32::from(*b) + 0.3 * (f32::from(*s) - f32::from(*b))).round() as u8;
                }
            }
            let hd = gate::key_out(&hd, &c, KEY);
            judged(u32::from(f), tile(a + f), c, hd)
        })
        .collect();
    let m = ingest::motion_of(walker, &damped).unwrap();
    assert!(m < Thresholds::default().motion, "damped {m}");
}

#[test]
fn a_flat_tile_takes_no_other_picture() {
    // The review's probe: a flat opaque tile against noise and a checker.
    let t = Thresholds::default();
    let f = Classic {
        width: 16,
        height: 16,
        px: vec![Some([20, 20, 30]); 256],
    };
    let mut noise = Rgba::new(128, 128);
    let mut s: u32 = 7;
    for p in noise.pixels.chunks_exact_mut(4) {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        p.copy_from_slice(&[s as u8, (s >> 8) as u8, (s >> 16) as u8, 255]);
    }
    let mut checker = Rgba::new(128, 128);
    for y in 0..128u32 {
        for x in 0..128u32 {
            let on = ((x / 32) + (y / 32)) % 2 == 0;
            let v = if on { 250 } else { 0 };
            img::put(&mut checker, x, y, [v, v, v, 255]);
        }
    }
    for (what, raw) in [("noise", noise), ("checker", checker)] {
        let m = gate::judge(&raw, &f, KEY, gate::Scale::Source).1;
        assert!(!m.pass(&t), "{what}: {m:?}");
    }
}

#[test]
fn the_review_paths_are_refused() {
    let repo = crate::work::repo_root().canonicalize().unwrap();
    let parent = repo.parent().unwrap().to_path_buf();
    let name = repo.file_name().unwrap().to_str().unwrap().to_string();
    let refused = |p: &Path| check_target(p).is_err();
    assert!(refused(&repo.join("site/hd")));
    assert!(refused(&repo.join("nonexist/../site/hd")));
    assert!(refused(
        &parent
            .join("zz-nonexist")
            .join("..")
            .join(&name)
            .join("site/hd")
    ));
    // Another case of the same path (APFS ignores case): the git work tree still holds it.
    let upper = PathBuf::from(repo.to_str().unwrap().to_uppercase());
    if upper.exists() {
        assert!(refused(&upper.join("site/hd")));
        assert!(refused(&upper.join("newdir/x")));
    }
    let t = temp("review-paths");
    std::os::unix::fs::symlink(&repo, t.join("link-to-repo")).unwrap();
    assert!(refused(&t.join("link-to-repo/site/hd")));
    let _ = std::fs::remove_dir_all(&t);
}

/// A `codex` stand-in from the review: `body` after reading the prompt or not.
fn review_stub(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/bash\n{body}\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn the_review_codex_shapes_end_in_time() {
    use std::time::{Duration, Instant};
    let t = temp("review-codex");
    let home = t.join("home");
    std::fs::create_dir_all(home.join("generated_images/t1")).unwrap();
    let dir = crate::work::ensure_dir(&t.join("dir")).unwrap();
    std::fs::write(t.join("sheet.png"), b"x").unwrap();
    let call = |bin: PathBuf, secs: u64, prompt: &str| {
        let c = run::Codex {
            bin,
            home: home.clone(),
            timeout: Duration::from_secs(secs),
            grace: run::CLEANUP,
        };
        let start = Instant::now();
        (
            c.generate(&t.join("sheet.png"), prompt, &dir),
            start.elapsed(),
        )
    };
    let thread = r#"echo '{"type":"thread.started","thread_id":"t1"}'"#;
    // Reads the prompt to its end, prints a thread id, no image: fails promptly, prompt whole.
    let seen = t.join("prompt-seen.txt");
    let ok = review_stub(
        &t,
        "ok.sh",
        &format!("cat > \"{}\"\n{thread}", seen.display()),
    );
    let (r, took) = call(ok, 5, "PROMPT");
    assert_eq!(r.unwrap_err(), "no image generated");
    assert!(took < Duration::from_secs(4), "{took:?}");
    assert_eq!(std::fs::read_to_string(&seen).unwrap(), "PROMPT");
    // A helper left running with the output open: ends with the call, not 12 s later.
    let bg = review_stub(
        &t,
        "bg.sh",
        &format!("cat > /dev/null\n{thread}\n( sleep 12 ) &\nexit 0"),
    );
    let (r, took) = call(bg, 5, "PROMPT");
    assert!(r.is_err());
    assert!(took < Duration::from_secs(4), "{took:?}");
    // Hangs: stopped at the deadline.
    let hang = review_stub(
        &t,
        "hang.sh",
        &format!("cat > /dev/null\n{thread}\n( sleep 12 ) &\nsleep 30"),
    );
    let (r, took) = call(hang, 2, "PROMPT");
    assert!(r.unwrap_err().contains("timed out"));
    assert!(took < Duration::from_secs(5), "{took:?}");
    // Does not take its prompt: stopped at once, long before the deadline.
    let deaf = review_stub(&t, "deaf.sh", "exec 0<&-\nsleep 30");
    let big = "x".repeat(1 << 20);
    let (r, took) = call(deaf, 30, &big);
    // The write to its closed input fails with EPIPE (std ignores SIGPIPE at startup, so the
    // signal does not end this process instead).
    let e = r.unwrap_err();
    assert!(e.contains("did not take its prompt"), "{e}");
    let epipe = std::io::Error::from_raw_os_error(libc::EPIPE).to_string();
    assert!(e.ends_with(&epipe), "{e}");
    assert!(took < Duration::from_secs(5), "{took:?}");
    let _ = std::fs::remove_dir_all(&t);
}

#[test]
fn the_generated_image_is_moved_out_of_codex_home() {
    let t = temp("move");
    let home = t.join("home");
    let thread = home.join("generated_images/t-9");
    std::fs::create_dir_all(&thread).unwrap();
    let image = thread.join("exec-1.png");
    std::fs::write(&image, b"png").unwrap();
    let c = run::Codex {
        bin: PathBuf::from("/nonexistent"),
        home: home.clone(),
        timeout: std::time::Duration::from_secs(1),
        grace: run::CLEANUP,
    };
    c.finish(&image);
    assert!(!thread.exists());
    assert!(home.join("generated_images").exists());
    // Anything not under generated_images/<thread>/ is left alone.
    let elsewhere = t.join("other/exec.png");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    std::fs::write(&elsewhere, b"png").unwrap();
    c.finish(&elsewhere);
    assert!(elsewhere.exists());
    let _ = std::fs::remove_dir_all(&t);
}

#[test]
fn the_deadline_holds_whatever_the_call_does_with_its_pipes() {
    use std::time::{Duration, Instant};
    let t = temp("pipes");
    let home = t.join("home");
    let dir = crate::work::ensure_dir(&t.join("dir")).unwrap();
    std::fs::write(t.join("sheet.png"), b"x").unwrap();
    let call = |bin: PathBuf, secs: u64, prompt: &str| {
        let c = run::Codex {
            bin,
            home: home.clone(),
            timeout: Duration::from_secs(secs),
            grace: run::CLEANUP,
        };
        let start = Instant::now();
        (
            c.generate(&t.join("sheet.png"), prompt, &dir),
            start.elapsed(),
        )
    };
    let big = "x".repeat(1 << 20);
    // Keeps its input open and never reads it, so the prompt's write blocks: the deadline
    // still stops it (the write is not what the call waits on).
    let stuck = review_stub(&t, "stuck.sh", "sleep 30");
    let (r, took) = call(stuck, 2, &big);
    assert!(r.as_ref().unwrap_err().contains("timed out"), "{r:?}");
    assert!(
        took < Duration::from_secs(6),
        "{took:?} (before: 30 s, the helper's sleep)"
    );
    // A helper that left the call's group (its own session) and holds the output open: the
    // call ends, and the stream is given a bounded grace, not the rest of the deadline. The
    // session is made before the fork, so the helper is out of the group before the call can
    // exit and the group be killed at that exit. A deadline that falls before the helper has
    // made its session still kills it with the group: the hang's 4 s, as keeps-both's in the
    // leak test, is slack for a loaded machine, not a bound on its load.
    let thread = r#"echo '{"type":"thread.started","thread_id":"t1"}'"#;
    for (name, then, secs) in [
        ("escaped-exit.sh", "exit 0", 30),
        ("escaped-hang.sh", "sleep 30", 4),
    ] {
        let pid = t.join(format!("{name}.pid"));
        let body = format!(
            "cat > /dev/null\n{thread}\n/usr/bin/perl -MPOSIX -e 'POSIX::setsid() > 0 or die \
             \"setsid: $!\"; exit if fork; open(my $f, \">\", $ARGV[0]); print $f $$; close $f; \
             sleep 20' \"{}\"\n{then}",
            pid.display()
        );
        let (r, took) = call(review_stub(&t, name, &body), secs, "PROMPT");
        let e = r.unwrap_err();
        // The grace is 1 s: the exit ends a grace after its stand-in's exit, the hang a grace
        // after its deadline (in both the helper holds the output, so the whole grace is waited
        // out); 8 s for both, slack for a loaded machine (before the grace, the hang took its
        // deadline + 5 s, here 9 s, and the exit 20 s, the helper's life).
        let limit = Duration::from_secs(8);
        assert!(took < limit, "{name}: {took:?} ({e})");
        if name == "escaped-hang.sh" {
            assert!(e.contains("timed out"), "{name}: {e}");
            // Its helper holds the call's output past the deadline, so the grace is waited out;
            // a call that ended at its deadline lost its helper with the group (not yet in a
            // session of its own).
            assert!(
                took >= Duration::from_secs(secs) + run::CLEANUP,
                "{name}: {took:?}: its helper had not left the call's group by the {secs} s \
                 deadline, or the grace was cut short"
            );
        } else {
            assert_eq!(e, "the call's output did not end", "{name}");
        }
        let start = Instant::now();
        while !pid.exists() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let pid = std::fs::read_to_string(&pid).unwrap();
        let _ = std::process::Command::new("/bin/kill")
            .args(["-s", "KILL", pid.trim()])
            .status();
    }
    let _ = std::fs::remove_dir_all(&t);
}

#[test]
fn the_event_reader_holds_one_line_at_most() {
    // A line over the cap is dropped up to its newline; the lines around it are kept, and
    // only the first thread id is reported.
    let mut stream = Vec::new();
    stream.extend_from_slice(b"{\"type\":\"turn.started\"}\n");
    stream.extend(std::iter::repeat_n(b'x', run::LINE_MAX + 1));
    stream.extend_from_slice(b"{\"type\":\"thread.started\",\"thread_id\":\"hidden\"}\n");
    stream.extend_from_slice(b"{\"type\":\"thread.started\",\"thread_id\":\"t-1\"}\n");
    stream.extend_from_slice(b"{\"type\":\"thread.started\",\"thread_id\":\"t-2\"}\n");
    let exact = format!("{{\"pad\":\"{}\"}}", "y".repeat(run::LINE_MAX - 10));
    assert_eq!(exact.len(), run::LINE_MAX);
    stream.extend_from_slice(exact.as_bytes());
    stream.extend_from_slice(b"\n{\"last\":1}");
    let mut kept = Vec::new();
    let ended = run::read_events(stream.as_slice(), &mut kept);
    assert_eq!(ended.thread.as_deref(), Some("t-1"));
    assert!(ended.error.is_none());
    let lines: Vec<&[u8]> = kept.split(|&b| b == b'\n').collect();
    assert_eq!(lines.len(), 6, "five lines, each with its newline");
    assert_eq!(lines[0], b"{\"type\":\"turn.started\"}");
    assert!(lines[1].ends_with(b"\"t-1\"}"));
    assert_eq!(lines[3], exact.as_bytes());
    assert_eq!(lines[4], b"{\"last\":1}");
    // The file keeps at most EVENTS_MAX bytes of a longer stream.
    let line = b"{\"type\":\"item\"}\n";
    let n = (run::EVENTS_MAX as usize / line.len()) * 2;
    let long: Vec<u8> = line.iter().copied().cycle().take(n * line.len()).collect();
    let mut kept = Vec::new();
    run::read_events(long.as_slice(), &mut kept);
    assert!(kept.len() as u64 <= run::EVENTS_MAX && kept.len() as u64 > run::EVENTS_MAX - 64);
}

/// A runner that leaves each image outside the work dir (as codex does) and, when told to
/// remove it, checks the output and its receipt are already written, byte for byte.
struct Leaves {
    at: PathBuf,
    out: PathBuf,
    /// The sheet whose "image" is not a PNG at all (a call that fails to record).
    broken: String,
    finished: std::sync::Mutex<Vec<String>>,
}

impl Runner for Leaves {
    fn generate(&self, sheet: &Path, _prompt: &str, _dir: &Path) -> Result<PathBuf, String> {
        let stem = sheet.file_stem().unwrap().to_str().unwrap();
        let to = self.at.join(format!("{stem}.png"));
        if stem == self.broken {
            std::fs::write(&to, b"not a png").map_err(|e| e.to_string())?;
        } else {
            std::fs::copy(sheet, &to).map_err(|e| e.to_string())?;
        }
        Ok(to)
    }

    fn finish(&self, image: &Path) {
        let stem = image.file_stem().unwrap().to_str().unwrap().to_string();
        let out = std::fs::read(self.out.join(format!("{stem}.png"))).unwrap();
        assert_eq!(out, std::fs::read(image).unwrap(), "{stem}: not the image");
        assert!(
            self.out.join(format!("{stem}.json")).exists(),
            "{stem}: no receipt"
        );
        std::fs::remove_file(image).unwrap();
        self.finished.lock().unwrap().push(stem);
    }
}

#[test]
fn the_call_s_image_is_removed_only_once_recorded() {
    let g = synthetic::game();
    let w = work("leaves");
    let jobs = atlas_into(&g, &w);
    let at = temp("leaves-codex");
    let fake = Leaves {
        at: at.clone(),
        out: w.path("out"),
        broken: jobs[1].sheet.clone(),
        finished: std::sync::Mutex::new(Vec::new()),
    };
    let opts = RunOptions {
        send: true,
        limit: None,
        parallel: 2,
    };
    let s = run::run(&w, &fake, &opts, &mut Vec::new()).unwrap();
    assert_eq!((s.ok, s.failed), (jobs.len() - 1, 1));
    let mut finished = fake.finished.into_inner().unwrap();
    finished.sort();
    let mut want: Vec<String> = jobs.iter().map(|j| j.sheet.clone()).collect();
    want.retain(|s| *s != jobs[1].sheet);
    want.sort();
    assert_eq!(finished, want);
    // The image that was not recorded (not a PNG) is where the call left it.
    assert!(at.join(format!("{}.png", jobs[1].sheet)).exists());
    assert!(run::output(&w, &jobs[1]).is_none());
    let left: Vec<_> = std::fs::read_dir(&at).unwrap().collect();
    assert_eq!(left.len(), 1);
    let _ = std::fs::remove_dir_all(&at);
    let _ = std::fs::remove_dir_all(&w.root);
}

#[test]
fn ingest_never_empties_a_directory_slices_points_at() {
    let g = synthetic::game();
    let t = temp("slices-link");
    // A repository (a `.git` entry above it) and a plain directory, each with a file in it.
    std::fs::create_dir_all(t.join("repo/.git")).unwrap();
    std::fs::create_dir_all(t.join("repo/site")).unwrap();
    std::fs::create_dir_all(t.join("plain")).unwrap();
    for target in [t.join("repo/site"), t.join("plain")] {
        std::fs::write(target.join("t-1.png"), b"planted").unwrap();
        std::fs::write(target.join("index.html"), b"planted").unwrap();
        let w = work("slices-link");
        atlas_into(&g, &w);
        std::fs::remove_dir(w.path("slices")).unwrap();
        std::os::unix::fs::symlink(&target, w.path("slices")).unwrap();
        let e = ingest::ingest(&g, &w, &Thresholds::default()).unwrap_err();
        assert!(e.contains("a symlink is not followed"), "{e}");
        for f in ["t-1.png", "index.html"] {
            assert_eq!(std::fs::read(target.join(f)).unwrap(), b"planted", "{f}");
        }
        let _ = std::fs::remove_dir_all(&w.root);
    }
    // A file in place of the directory: refused too, left as it is.
    let w = work("slices-file");
    atlas_into(&g, &w);
    std::fs::remove_dir(w.path("slices")).unwrap();
    std::fs::write(w.path("slices"), b"x").unwrap();
    assert!(ingest::ingest(&g, &w, &Thresholds::default()).is_err());
    assert_eq!(std::fs::read(w.path("slices")).unwrap(), b"x");
    let _ = std::fs::remove_dir_all(&w.root);
    let _ = std::fs::remove_dir_all(&t);
}

/// A runner whose answer per sheet is set between runs: the sheet as sent (passes), noise of
/// the sheet's size (fails its gates), or an image two pixels narrower (another size). Each
/// image is left outside the work dir under a name of its own, and removed when finished.
struct Script {
    at: PathBuf,
    wrong: std::sync::Mutex<std::collections::BTreeSet<String>>,
    noise: std::sync::Mutex<std::collections::BTreeSet<String>>,
    /// Sheets whose calls fail outright (no image).
    fail: std::sync::Mutex<std::collections::BTreeSet<String>>,
    calls: std::sync::Mutex<Vec<String>>,
    n: AtomicUsize,
}

fn script(name: &str) -> Script {
    Script {
        at: temp(name),
        wrong: Default::default(),
        noise: Default::default(),
        fail: Default::default(),
        calls: Default::default(),
        n: AtomicUsize::new(0),
    }
}

/// Sets one of a [`Script`]'s sheet sets.
fn set(m: &std::sync::Mutex<std::collections::BTreeSet<String>>, v: &[&String]) {
    *m.lock().unwrap() = v.iter().map(|s| (*s).clone()).collect();
}

/// One run with `s`: the sheets it called, sorted, its ok and failed counts.
fn run_once(w: &Work, s: &Script) -> (Vec<String>, usize, usize) {
    s.calls.lock().unwrap().clear();
    let opts = RunOptions {
        send: true,
        limit: None,
        parallel: 2,
    };
    let r = run::run(w, s, &opts, &mut Vec::new()).unwrap();
    let mut calls = s.calls.lock().unwrap().clone();
    calls.sort();
    (calls, r.ok, r.failed)
}

fn sorted(v: &[&Job]) -> Vec<String> {
    let mut v: Vec<String> = v.iter().map(|j| j.sheet.clone()).collect();
    v.sort();
    v
}

/// The images left where a [`Script`]'s calls put them.
fn kept(s: &Script) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(&s.at)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

impl Runner for Script {
    fn generate(&self, sheet: &Path, _prompt: &str, _dir: &Path) -> Result<PathBuf, String> {
        let stem = sheet.file_stem().unwrap().to_str().unwrap().to_string();
        let k = self.n.fetch_add(1, Ordering::SeqCst);
        let to = self.at.join(format!("{stem}~{k}.png"));
        let (w, h) = img::png_size(&std::fs::read(sheet).unwrap())?;
        if self.fail.lock().unwrap().contains(&stem) {
            self.calls.lock().unwrap().push(stem);
            return Err("no image generated".into());
        }
        if self.wrong.lock().unwrap().contains(&stem) {
            img::write_png(&to, &Rgba::new(w - 2, h))?;
        } else if self.noise.lock().unwrap().contains(&stem) {
            let mut out = Rgba::new(w, h);
            for (i, p) in out.pixels.chunks_exact_mut(4).enumerate() {
                let v = (i as u32).wrapping_mul(0x9e37_79b9).rotate_left(7);
                p.copy_from_slice(&[v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]);
            }
            img::write_png(&to, &out)?;
        } else {
            std::fs::copy(sheet, &to).map_err(|e| e.to_string())?;
        }
        self.calls.lock().unwrap().push(stem);
        Ok(to)
    }

    fn finish(&self, image: &Path) {
        std::fs::remove_file(image).unwrap();
    }
}

#[test]
fn a_wrong_size_output_end_to_end() {
    use std::collections::BTreeSet;
    let g = synthetic::game();
    let plan = group::plan(&g);
    let w = work("wrong-size");
    let jobs = atlas_into(&g, &w);
    let tile: Vec<&Job> = jobs.iter().filter(|j| j.kind == SheetKind::Tile).collect();
    assert_eq!(tile.len(), 1);
    let x = tile[0].sheet.clone();
    let on_x: BTreeSet<u32> = tile[0].places.iter().map(|p| p.item).collect();
    let s = script("wrong-size-codex");
    let t = Thresholds::default();
    // The model always answers that sheet, and its re-rolls, two pixels narrower.
    set(&s.wrong, &[&x]);
    // Run: every sheet is called once; that one is recorded as it came back, undecoded, and
    // its image moved out of the call's folder like any other.
    assert_eq!(
        run_once(&w, &s),
        (sorted(&jobs.iter().collect::<Vec<_>>()), jobs.len(), 0)
    );
    assert_eq!(
        run::output(&w, tile[0]).map(|(_, a, b)| (a, b)),
        Some((SHEET_W - 2, SHEET_H))
    );
    assert_eq!(kept(&s), Vec::<String>::new());
    // Resume: nothing is called again (the attempt is spent).
    assert_eq!(run_once(&w, &s), (vec![], 0, 0));
    // Ingest refuses it by its size, and re-rolls each item it held, once.
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert_eq!(r.refused_sheets, 1);
    assert_eq!(r.rerolled_items, on_x.len());
    let jobs2 = w.read_jobs().unwrap();
    let rerolls: Vec<&Job> = jobs2.iter().filter(|j| j.attempt == 2).collect();
    assert_eq!(rerolls.len(), r.reroll_sheets);
    let rerolled: BTreeSet<u32> = rerolls
        .iter()
        .flat_map(|j| j.places.iter().map(|p| p.item))
        .collect();
    assert_eq!(rerolled, on_x);
    // Run: only the re-rolls are called, and come back the wrong size too: recorded.
    let rr: Vec<&String> = rerolls.iter().map(|j| &j.sheet).collect();
    set(&s.wrong, &rr);
    assert_eq!(run_once(&w, &s), (sorted(&rerolls), rerolls.len(), 0));
    assert_eq!(run_once(&w, &s), (vec![], 0, 0));
    // Ingest: refused again; each group is left to the scaler whole ("failed": no usable
    // output from its last attempt), with no third attempt.
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert_eq!(r.refused_sheets, 1 + rerolls.len());
    assert_eq!((r.rerolled_items, r.reroll_sheets), (0, 0));
    assert_eq!(w.read_jobs().unwrap().len(), jobs2.len());
    for class in Class::ALL {
        let records: usize = on_x
            .iter()
            .map(|&i| &plan.items[i as usize])
            .filter(|i| i.class == class)
            .map(|i| members(&g, i).len())
            .sum();
        assert_eq!(r.get(class, Status::Failed), records, "{class:?}");
        assert_eq!(r.get(class, Status::Pending), 0, "{class:?}");
    }
    assert!(r.get(Class::Actor, Status::Failed) > 0);
    assert_eq!(run_once(&w, &s), (vec![], 0, 0));
    // Every wrong-size image kept its receipt, with its own size, and is nowhere else.
    for job in [tile[0]].into_iter().chain(rerolls.iter().copied()) {
        let (_, a, b) = run::output(&w, job).unwrap();
        assert_eq!((a, b), (SHEET_W - 2, SHEET_H), "{}", job.sheet);
    }
    assert_eq!(kept(&s), Vec::<String>::new());
    let _ = std::fs::remove_dir_all(&s.at);
    let _ = std::fs::remove_dir_all(&w.root);
}

#[test]
fn calls_that_keep_failing_are_given_up_after_two() {
    use std::collections::BTreeSet;
    let g = synthetic::game();
    let plan = group::plan(&g);
    let w = work("given-up");
    let jobs = atlas_into(&g, &w);
    let tile: Vec<&Job> = jobs.iter().filter(|j| j.kind == SheetKind::Tile).collect();
    let x = tile[0].sheet.clone();
    let on_x: BTreeSet<u32> = tile[0].places.iter().map(|p| p.item).collect();
    let s = script("given-up-codex");
    let t = Thresholds::default();
    let all = sorted(&jobs.iter().collect::<Vec<_>>());
    // The tile sheet's calls always fail (as a timeout or no image does).
    set(&s.fail, &[&x]);
    assert_eq!(run_once(&w, &s), (all.clone(), jobs.len() - 1, 1));
    // Not given up after one failure: called again, fails again.
    assert_eq!(run_once(&w, &s), (vec![x.clone()], 0, 1));
    // Given up after two: not called again, the run says so.
    let mut out = Vec::new();
    let opts = RunOptions {
        send: true,
        limit: None,
        parallel: 2,
    };
    s.calls.lock().unwrap().clear();
    let r = run::run(&w, &s, &opts, &mut out).unwrap();
    assert_eq!((r.calls, r.given_up), (0, 1));
    assert!(String::from_utf8_lossy(&out).contains("1 sheet(s) given up after 2 reserved calls"));
    // Ingest refuses it and re-rolls its items once; the re-rolls get their own two calls.
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert_eq!((r.refused_sheets, r.rerolled_items), (1, on_x.len()));
    let jobs2 = w.read_jobs().unwrap();
    let rerolls: Vec<&Job> = jobs2.iter().filter(|j| j.attempt == 2).collect();
    let rr: Vec<&String> = rerolls.iter().map(|j| &j.sheet).collect();
    set(&s.fail, &rr);
    let n = rerolls.len();
    assert_eq!(run_once(&w, &s), (sorted(&rerolls), 0, n));
    assert_eq!(run_once(&w, &s), (sorted(&rerolls), 0, n));
    assert_eq!(run_once(&w, &s), (vec![], 0, 0));
    // Then the items are left to the scaler whole, with no third attempt.
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert_eq!((r.rerolled_items, r.reroll_sheets), (0, 0));
    assert_eq!(w.read_jobs().unwrap().len(), jobs2.len());
    let records: usize = on_x
        .iter()
        .map(|&i| members(&g, &plan.items[i as usize]).len())
        .sum();
    let failed: usize = Class::ALL.iter().map(|&c| r.get(c, Status::Failed)).sum();
    assert_eq!(failed, records);
    // One failure then a success is not given up: a fresh atlas, the tile sheet failing once.
    let w2 = work("given-up-once");
    let jobs = atlas_into(&g, &w2);
    let x = jobs
        .iter()
        .find(|j| j.kind == SheetKind::Tile)
        .unwrap()
        .sheet
        .clone();
    set(&s.fail, &[&x]);
    assert_eq!(run_once(&w2, &s).2, 1);
    set(&s.fail, &[]);
    assert_eq!(run_once(&w2, &s), (vec![x], 1, 0));
    let r = ingest::ingest(&g, &w2, &t).unwrap();
    assert_eq!((r.refused_sheets, r.rerolled_items), (0, 0));
    let _ = std::fs::remove_dir_all(&s.at);
    let _ = std::fs::remove_dir_all(&w.root);
    let _ = std::fs::remove_dir_all(&w2.root);
}

/// An event file that takes `room` bytes, then fails every write with ENOSPC (counting the
/// writes tried after that); its flush fails when `flush_fails`.
struct Full {
    room: usize,
    got: Vec<u8>,
    tried_after: usize,
    flush_fails: bool,
}

impl std::io::Write for Full {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self.got.len() >= self.room {
            self.tried_after += 1;
            return Err(std::io::Error::from_raw_os_error(libc::ENOSPC));
        }
        let n = b.len().min(self.room - self.got.len());
        self.got.extend_from_slice(&b[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.flush_fails {
            return Err(std::io::Error::other("flush refused"));
        }
        Ok(())
    }
}

#[test]
fn an_event_file_that_fails_is_reported_not_lost() {
    let full = |room, flush_fails| Full {
        room,
        got: Vec::new(),
        tried_after: 0,
        flush_fails,
    };
    let mut stream = b"{\"type\":\"thread.started\",\"thread_id\":\"t-1\"}\n".to_vec();
    for i in 0..10 {
        stream.extend_from_slice(
            format!("{{\"n\":{i},\"pad\":\"{}\"}}\n", "z".repeat(80)).as_bytes(),
        );
    }
    // Out of room after 250 bytes: the first error is kept, and nothing is written after it
    // (one write tried and refused); the thread id is still read.
    let mut file = full(250, false);
    let ended = run::read_events(stream.as_slice(), &mut file);
    assert_eq!(ended.thread.as_deref(), Some("t-1"));
    assert_eq!(
        ended.error.map(|e| e.raw_os_error()),
        Some(Some(libc::ENOSPC))
    );
    assert_eq!(file.tried_after, 1);
    assert_eq!(file.got.len(), 250);
    // Through a buffer, as a call writes its events: the error comes out when the buffer is
    // written, at the end at the latest.
    let mut lines = run::EventLines::new(std::io::BufWriter::new(full(250, false)));
    lines.feed(&stream);
    assert_eq!(
        lines.finish().error.map(|e| e.raw_os_error()),
        Some(Some(libc::ENOSPC))
    );
    // Room enough, the flush refused: that is the error.
    let mut lines = run::EventLines::new(full(1 << 20, true));
    lines.feed(&stream);
    let ended = lines.finish();
    assert_eq!(
        ended.error.map(|e| e.to_string()).as_deref(),
        Some("flush refused")
    );
    // Nothing wrong: no error.
    let mut file = full(1 << 20, false);
    assert!(
        run::read_events(stream.as_slice(), &mut file)
            .error
            .is_none()
    );
    assert_eq!(file.got, stream);
}

#[test]
fn a_call_s_save_failure_is_its_error_after_the_call_s_own() {
    use std::os::unix::process::ExitStatusExt;
    let ok = std::process::ExitStatus::from_raw(0);
    let exit1 = std::process::ExitStatus::from_raw(1 << 8);
    let why = "the call's events were not saved: No space left on device".to_string();
    let saved = Some(why.clone());
    let settled = |waited, taken, ended, saved: &Option<String>| run::Settled {
        waited,
        taken,
        refused: None,
        ended,
        unread: None,
        saved: saved.clone(),
    };
    let id = || Some("t-1".to_string());
    // The call went right: the save failure is its error; without one, the thread id.
    assert_eq!(
        run::settle(settled(Ok(ok), true, true, &saved), id()),
        Err(why.clone())
    );
    assert_eq!(
        run::settle(settled(Ok(ok), true, true, &None), id()),
        Ok("t-1".into())
    );
    // The call's own failure comes first.
    for (s, want) in [
        (
            settled(Err("timed out after 1 s".into()), true, true, &saved),
            "timed out",
        ),
        (
            settled(Ok(ok), false, true, &saved),
            "did not take its prompt",
        ),
        (settled(Ok(ok), true, false, &saved), "output did not end"),
        (settled(Ok(exit1), true, true, &saved), "codex exited with"),
    ] {
        let e = run::settle(s, id()).unwrap_err();
        assert!(e.contains(want), "{e}");
        // The save failure is kept beside it, as context.
        assert!(e.ends_with(&format!("(also: {why})")), "{e}");
    }
    // A save failure before a missing thread id.
    assert_eq!(
        run::settle(settled(Ok(ok), true, true, &saved), None),
        Err(why)
    );
}

#[test]
fn a_capped_read_reads_no_more_than_its_cap() {
    let t = temp("capped");
    let p = t.join("f");
    std::fs::write(&p, vec![7u8; 100]).unwrap();
    assert_eq!(img::read_capped(&p, 100).unwrap().len(), 100);
    assert!(
        img::read_capped(&p, 99)
            .unwrap_err()
            .contains("larger than 99 bytes")
    );
    // A file that is not a plain file of known size (a FIFO, endless while written): still
    // refused after max + 1 bytes, not read to its end.
    let fifo = t.join("fifo");
    assert!(
        std::process::Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let writer = std::thread::spawn({
        let fifo = fifo.clone();
        move || {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
            let chunk = vec![1u8; 1 << 16];
            // Writes until the reader closes its end (EPIPE), at most 64 MiB.
            for _ in 0..1024 {
                if f.write_all(&chunk).is_err() {
                    break;
                }
            }
        }
    });
    let e = img::read_capped(&fifo, 1000).unwrap_err();
    assert!(e.contains("larger than 1000 bytes"), "{e}");
    writer.join().unwrap();
    let _ = std::fs::remove_dir_all(&t);
}

#[test]
fn a_call_s_stderr_is_kept_to_its_cap() {
    use std::time::{Duration, Instant};
    let t = temp("stderr-cap");
    let dir = crate::work::ensure_dir(&t.join("dir")).unwrap();
    std::fs::write(t.join("sheet.png"), b"x").unwrap();
    // 64 MiB to stderr as fast as it goes, then the thread event and a clean exit.
    let loud = review_stub(
        &t,
        "loud.sh",
        "cat > /dev/null\nhead -c 67108864 /dev/zero >&2\n\
         echo '{\"type\":\"thread.started\",\"thread_id\":\"t1\"}'",
    );
    let c = run::Codex {
        bin: loud,
        home: t.join("home"),
        timeout: Duration::from_secs(30),
        grace: run::CLEANUP,
    };
    let start = Instant::now();
    let e = c
        .generate(&t.join("sheet.png"), "PROMPT", &dir)
        .unwrap_err();
    let took = start.elapsed();
    // It ran to its end (its thread id read after all of it), and no image: the usual error.
    assert_eq!(e, "no image generated");
    assert!(took < Duration::from_secs(20), "{took:?}");
    let kept = std::fs::metadata(dir.join("stderr.txt")).unwrap().len();
    println!("64 MiB of stderr in {took:?}: {kept} bytes kept");
    assert_eq!(kept, run::STDERR_MAX);
    let _ = std::fs::remove_dir_all(&t);
}

#[test]
fn a_read_error_on_a_call_s_pipe_is_kept_not_taken_for_its_end() {
    use std::io::{Error, ErrorKind};
    use std::time::{Duration, Instant};
    /// Hands out `steps` in order, then a clean end.
    struct Steps(Vec<std::io::Result<Vec<u8>>>);
    impl std::io::Read for Steps {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            let b = self.0.remove(0)?;
            buf[..b.len()].copy_from_slice(&b);
            Ok(b.len())
        }
    }
    let until = Instant::now() + Duration::from_secs(5);
    let mut got = Vec::new();
    // Interrupted is retried; a failure that is not ends the pipe with that error.
    let mut pipe = Some(Steps(vec![
        Ok(b"ab".to_vec()),
        Err(Error::from(ErrorKind::Interrupted)),
        Ok(b"c".to_vec()),
        Err(Error::other("the disk went away")),
        Ok(b"never".to_vec()),
    ]));
    let e = run::drain(&mut pipe, until, |b| got.extend_from_slice(b)).unwrap();
    assert_eq!(e.to_string(), "the disk went away");
    assert!(pipe.is_none());
    assert_eq!(got, b"abc");
    // Would-block leaves it open; a clean end closes it with no error.
    let mut pipe = Some(Steps(vec![Err(Error::from(ErrorKind::WouldBlock))]));
    assert!(run::drain(&mut pipe, until, |_| {}).is_none());
    assert!(pipe.is_some());
    assert!(run::drain(&mut pipe, until, |_| {}).is_none());
    assert!(pipe.is_none());
    // In the call: an unreadable event stream is the call's error, not a clean end.
    let s = run::Settled {
        waited: Ok(std::os::unix::process::ExitStatusExt::from_raw(0)),
        taken: true,
        refused: None,
        ended: false,
        unread: Some(Error::other("the disk went away")),
        saved: None,
    };
    let e = run::settle(s, Some("t-1".into())).unwrap_err();
    assert_eq!(e, "the call's output could not be read: the disk went away");
}

#[test]
fn stderr_written_after_the_events_end_is_still_kept() {
    use std::time::{Duration, Instant};
    let t = temp("late-stderr");
    let dir = crate::work::ensure_dir(&t.join("dir")).unwrap();
    std::fs::write(t.join("sheet.png"), b"x").unwrap();
    // The call prints its thread and exits; a helper of its own session, holding only its
    // stderr, writes there 300 ms later: the events have ended long before. The session is
    // made before the fork, so the helper has left the call's group before the call can exit
    // and the group be killed (made after it, a helper the loaded machine had not yet run
    // was killed with the group, and never wrote).
    let late = review_stub(
        &t,
        "late.sh",
        "cat > /dev/null\necho '{\"type\":\"thread.started\",\"thread_id\":\"t1\"}'\n\
         /usr/bin/perl -MPOSIX -e 'POSIX::setsid() > 0 or die \"setsid: $!\"; exit if fork; \
         select(undef, undef, undef, 0.3); print STDERR \"late\\n\"' > /dev/null\nexit 0",
    );
    // A grace far past the helper's 300 ms, 100 times it, as slack for a loaded machine (a
    // longer pause would still outlast it): the call still ends before the grace does, when
    // the helper does (its stderr's end).
    let c = run::Codex {
        bin: late,
        home: t.join("home"),
        timeout: Duration::from_secs(10),
        grace: Duration::from_secs(30),
    };
    let start = Instant::now();
    let e = c
        .generate(&t.join("sheet.png"), "PROMPT", &dir)
        .unwrap_err();
    let took = start.elapsed();
    assert!(took < c.grace, "{took:?}");
    assert_eq!(e, "no image generated");
    assert_eq!(
        std::fs::read_to_string(dir.join("stderr.txt")).unwrap(),
        "late\n"
    );
    let _ = std::fs::remove_dir_all(&t);
}

/// The clock `Instant` reads (std's: CLOCK_UPTIME_RAW on Apple, CLOCK_MONOTONIC elsewhere),
/// by its id, so that a call's stand-in can read it too and the two readings compare.
#[cfg(target_vendor = "apple")]
const INSTANT_CLOCK: libc::clockid_t = libc::CLOCK_UPTIME_RAW;
#[cfg(not(target_vendor = "apple"))]
const INSTANT_CLOCK: libc::clockid_t = libc::CLOCK_MONOTONIC;

/// The time on [`INSTANT_CLOCK`].
fn instant_clock() -> std::time::Duration {
    // Zeroed, not a literal: on some targets the struct has private padding beside its fields.
    // SAFETY: a timespec is integers (and padding), for which all zero bytes are a value.
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    // SAFETY: clock_gettime(2) writes the one timespec it is given; no other memory involved.
    assert_eq!(unsafe { libc::clock_gettime(INSTANT_CLOCK, &mut ts) }, 0);
    std::time::Duration::new(
        u64::try_from(ts.tv_sec).unwrap(),
        u32::try_from(ts.tv_nsec).unwrap(),
    )
}

#[test]
fn stderr_written_after_the_grace_is_not_waited_for() {
    use std::time::Duration;
    let t = temp("later-stderr");
    let dir = crate::work::ensure_dir(&t.join("dir")).unwrap();
    std::fs::write(t.join("sheet.png"), b"x").unwrap();
    // As above, but the helper holds the call's stderr while `hold` exists (at most 30 s)
    // and writes there only then; the test removes it once the call has returned, so the
    // write comes after the grace whatever the machine's load. Just before it exits, the call
    // marks the time on the clock its grace is timed on.
    let (hold, exited) = (t.join("hold"), t.join("exited"));
    std::fs::write(&hold, b"").unwrap();
    let later = review_stub(
        &t,
        "later.sh",
        &format!(
            "cat > /dev/null\necho '{{\"type\":\"thread.started\",\"thread_id\":\"t1\"}}'\n\
             /usr/bin/perl -MPOSIX -e 'POSIX::setsid() > 0 or die \"setsid: $!\"; exit if fork; \
             for (1 .. 3000) {{ last unless -e $ARGV[0]; select(undef, undef, undef, 0.01) }} \
             print STDERR \"later\\n\"' \"{}\" > /dev/null\n\
             /usr/bin/perl -MTime::HiRes -e 'printf \"%.9f\", Time::HiRes::clock_gettime($ARGV[0])' \
             {INSTANT_CLOCK} > \"{}\"\nexit 0",
            hold.display(),
            exited.display()
        ),
    );
    // Twice the tool's own grace, so a call that waited this long was given this one.
    let grace = run::CLEANUP * 2;
    let c = run::Codex {
        bin: later,
        home: t.join("home"),
        timeout: Duration::from_secs(10),
        grace,
    };
    let e = c.generate(&t.join("sheet.png"), "PROMPT", &dir);
    let end = instant_clock();
    std::fs::remove_file(&hold).unwrap();
    assert_eq!(e.unwrap_err(), "no image generated");
    // Not waited for: nothing of it kept.
    assert_eq!(std::fs::read_to_string(dir.join("stderr.txt")).unwrap(), "");
    // From the mark to the call's return, on one clock (an end before the mark fails, it is
    // not taken for no time): at least the grace, which the call waited out (the stderr still
    // open), and less than 3 s more, then it ended (slack for a loaded machine). A drain
    // bounded by the call's deadline instead ends 10 s from the call's start: 10 s after its
    // exit less the stand-in's run, past these 5 s unless that run took over 5 s.
    let mark = std::fs::read_to_string(&exited).unwrap();
    let exit = Duration::from_secs_f64(mark.parse().unwrap_or_else(|e| panic!("{mark:?}: {e}")));
    let after = end
        .checked_sub(exit)
        .unwrap_or_else(|| panic!("the call ended at {end:?}, before its exit at {exit:?}"));
    assert!(after >= grace, "{after:?} after its exit");
    assert!(
        after < grace + Duration::from_secs(3),
        "{after:?} after its exit"
    );
    let _ = std::fs::remove_dir_all(&t);
}

#[test]
fn the_tool_s_calls_are_given_the_cleanup_grace() {
    // The tool's calls are made by `Codex::from_env` (its only maker outside the tests), with
    // CLEANUP; the tests above give theirs other graces.
    assert_eq!(run::Codex::from_env(None).unwrap().grace, run::CLEANUP);
}

#[test]
fn every_reserved_call_counts_whatever_became_of_it() {
    let g = synthetic::game();
    let t = Thresholds::default();
    let opts = RunOptions {
        send: true,
        limit: None,
        parallel: 2,
    };
    let dispatched = |w: &Work, sheet: &str| {
        std::fs::read_to_string(w.path("runs.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("\"dispatched\"") && l.contains(&format!("\"{sheet}\"")))
            .count()
    };
    // A successful output later lost, then later corrupt: each call counted, then given up.
    let w = work("paid-lost");
    let jobs = atlas_into(&g, &w);
    let x = jobs[0].sheet.clone();
    let s = script("paid-lost-codex");
    assert_eq!(run_once(&w, &s).1, jobs.len());
    std::fs::remove_file(w.out(&x)).unwrap();
    assert_eq!(run_once(&w, &s), (vec![x.clone()], 1, 0));
    let mut png = std::fs::read(w.out(&x)).unwrap();
    png.truncate(png.len() / 2);
    std::fs::write(w.out(&x), &png).unwrap();
    assert_eq!(run_once(&w, &s), (vec![], 0, 0));
    assert_eq!(dispatched(&w, &x), 2);
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert_eq!(r.refused_sheets, 1);
    assert!(r.rerolled_items > 0);
    let _ = std::fs::remove_dir_all(&w.root);
    // A sheet that cannot be read: never sent (no identity, so no call), refused by ingest.
    let w = work("paid-unreadable");
    let jobs = atlas_into(&g, &w);
    let x = jobs[0].sheet.clone();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(w.sheet(&x), std::fs::Permissions::from_mode(0o000)).unwrap();
    let mut out = Vec::new();
    s.calls.lock().unwrap().clear();
    let r = run::run(&w, &s, &opts, &mut out).unwrap();
    assert_eq!((r.unreadable, r.calls), (1, jobs.len() - 1));
    assert!(!s.calls.lock().unwrap().contains(&x));
    assert!(
        String::from_utf8_lossy(&out).contains(&format!("{x}: not sent, its sheet cannot be read"))
    );
    assert_eq!(dispatched(&w, &x), 0);
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert_eq!(r.refused_sheets, 1);
    std::fs::set_permissions(w.sheet(&x), std::fs::Permissions::from_mode(0o644)).unwrap();
    let _ = std::fs::remove_dir_all(&w.root);
    // A ledger that cannot be read, or cannot be written: nothing is sent.
    let w = work("paid-ledger");
    atlas_into(&g, &w);
    std::fs::create_dir_all(w.path("runs.jsonl")).unwrap();
    s.calls.lock().unwrap().clear();
    assert!(run::run(&w, &s, &opts, &mut Vec::new()).is_err());
    assert!(s.calls.lock().unwrap().is_empty());
    std::fs::remove_dir(w.path("runs.jsonl")).unwrap();
    // Readable (absent), but its directory takes no new file: each reservation fails, so no
    // call is made, and the run says why.
    std::fs::set_permissions(&w.root, std::fs::Permissions::from_mode(0o555)).unwrap();
    let e = run::run(&w, &s, &opts, &mut Vec::new()).unwrap_err();
    std::fs::set_permissions(&w.root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(e.contains(run::LEDGER_FAILED), "{e}");
    // One line per failure.
    assert!(e.lines().all(|l| l.starts_with(run::LEDGER_FAILED)), "{e}");
    assert!(s.calls.lock().unwrap().is_empty());
    assert!(!w.path("runs.jsonl").exists());
    let _ = std::fs::remove_dir_all(&w.root);
    // The line written, then its directory's sync failing: not sent (the reservation is not
    // durable), and counted all the same (conservative: it is in the file).
    let w = work("paid-dirsync");
    let jobs = atlas_into(&g, &w);
    *crate::work::FAIL_DIR_SYNC.lock().unwrap() =
        Some((w.root.canonicalize().unwrap(), "\"dispatched\"".into()));
    s.calls.lock().unwrap().clear();
    let e = run::run(&w, &s, &opts, &mut Vec::new()).unwrap_err();
    *crate::work::FAIL_DIR_SYNC.lock().unwrap() = None;
    assert!(
        e.contains(run::LEDGER_FAILED) && e.contains("sync failed"),
        "{e}"
    );
    assert!(s.calls.lock().unwrap().is_empty());
    let book = run::ledger(&w).unwrap();
    let reserved: usize = book.reserved.values().sum();
    assert!((1..=2).contains(&reserved), "{reserved}");
    let id = run::identity(&w, &jobs[0]).unwrap();
    let r = run::run(&w, &s, &opts, &mut Vec::new()).unwrap();
    assert_eq!(r.given_up, 0);
    assert!(run::ledger(&w).unwrap().reserved[&id] >= 1);
    let _ = std::fs::remove_dir_all(&w.root);
    // A void written, then its directory's sync failing (a codex that cannot be started):
    // this run counts the call as reserved (it could not be sure the void is durable), the
    // ledger replays it as voided; either way nothing was sent.
    let w = work("paid-void-sync");
    let jobs = atlas_into(&g, &w);
    *crate::work::FAIL_DIR_SYNC.lock().unwrap() =
        Some((w.root.canonicalize().unwrap(), "\"voided\"".into()));
    let missing = run::Codex {
        bin: w.root.join("no-such-codex"),
        home: w.root.join("home"),
        timeout: std::time::Duration::from_secs(5),
        grace: run::CLEANUP,
    };
    let one = RunOptions {
        send: true,
        limit: Some(1),
        parallel: 2,
    };
    let mut out = Vec::new();
    let r = run::run(&w, &missing, &one, &mut out).unwrap();
    *crate::work::FAIL_DIR_SYNC.lock().unwrap() = None;
    assert_eq!(
        (r.calls, r.failed),
        (1, 1),
        "{}",
        String::from_utf8_lossy(&out)
    );
    assert!(String::from_utf8_lossy(&out).contains("sync failed"));
    let id = run::identity(&w, &jobs[0]).unwrap();
    assert_eq!(run::ledger(&w).unwrap().reserved.get(&id).copied(), Some(0));
    let _ = std::fs::remove_dir_all(&w.root);
    // Lines from before reservations existed count for no sheet, and the run says so.
    let w = work("paid-legacy");
    let jobs = atlas_into(&g, &w);
    let old = format!(
        "{{\"sheet\":\"{}\",\"ok\":false,\"seconds\":1,\"error\":\"timed out\"}}\n",
        jobs[0].sheet
    );
    std::fs::write(w.path("runs.jsonl"), old.repeat(3)).unwrap();
    let mut out = Vec::new();
    let r = run::run(
        &w,
        &s,
        &RunOptions {
            send: false,
            limit: None,
            parallel: 2,
        },
        &mut out,
    )
    .unwrap();
    assert_eq!((r.legacy, r.given_up, r.pending), (3, 0, jobs.len()));
    assert!(String::from_utf8_lossy(&out).contains("3 line(s) from before calls were reserved"));
    let _ = std::fs::remove_dir_all(&w.root);
    let _ = std::fs::remove_dir_all(&s.at);
}

#[test]
fn a_run_makes_at_most_four_calls_at_once() {
    /// Counts the calls in flight, each 30 ms long.
    struct Busy {
        now: AtomicUsize,
        most: AtomicUsize,
    }
    impl Runner for Busy {
        fn generate(&self, sheet: &Path, _: &str, dir: &Path) -> Result<PathBuf, String> {
            let n = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(30));
            self.now.fetch_sub(1, Ordering::SeqCst);
            let to = dir.join("made.png");
            std::fs::copy(sheet, &to).map_err(|e| e.to_string())?;
            Ok(to)
        }
    }
    let g = synthetic::game();
    for (asked, most) in [(9, 4), (4, 4), (1, 2)] {
        let w = work(&format!("parallel-{asked}"));
        let jobs = atlas_into(&g, &w);
        assert!(jobs.len() >= 4);
        let b = Busy {
            now: AtomicUsize::new(0),
            most: AtomicUsize::new(0),
        };
        let opts = RunOptions {
            send: true,
            limit: None,
            parallel: asked,
        };
        run::run(&w, &b, &opts, &mut Vec::new()).unwrap();
        assert!(b.most.load(Ordering::SeqCst) <= most, "asked {asked}");
        assert!(b.most.load(Ordering::SeqCst) >= 1);
        let _ = std::fs::remove_dir_all(&w.root);
    }
}

#[test]
fn a_turn_of_drain_is_bounded_whatever_the_pipe_holds() {
    use std::cell::Cell;
    use std::time::{Duration, Instant};
    /// A pipe that always has `each` bytes ready: never empty (no WouldBlock), never at its
    /// end, until `cap` reads, then an end, so a drain that is not bounded ends too. The counts
    /// live outside, so they are still there when the drain drops a pipe at its end: the reads,
    /// and the largest buffer a read was given (the drain's chunk).
    struct Endless<'a> {
        each: usize,
        reads: &'a Cell<usize>,
        chunk: &'a Cell<usize>,
        cap: usize,
    }
    impl std::io::Read for Endless<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.reads.set(self.reads.get() + 1);
            self.chunk.set(self.chunk.get().max(buf.len()));
            if self.reads.get() > self.cap {
                return Ok(0);
            }
            let n = self.each.min(buf.len());
            buf[..n].fill(b'y');
            Ok(n)
        }
    }
    for each in [1, 100, 8192, 1 << 20] {
        let (reads, chunk) = (Cell::new(0), Cell::new(0));
        let mut pipe = Some(Endless {
            each,
            reads: &reads,
            chunk: &chunk,
            cap: 100_000,
        });
        let mut got = 0;
        // Each case its own time, far ahead, so it is the turn's bound that ends the turn.
        let until = Instant::now() + Duration::from_secs(60);
        assert!(run::drain(&mut pipe, until, |b| got += b.len()).is_none());
        // Data always ready and time to spare: one turn makes exactly TURN_CALLS reads, moves
        // what they read, at most TURN_BYTES, and leaves the pipe open. Neither a drain that
        // reads on (the bound dropped) nor one that returns without reading passes.
        let (reads, open) = (reads.get(), pipe.is_some());
        assert!(
            reads == run::TURN_CALLS && open,
            "{each}-byte reads: {reads} in one turn, the pipe {}",
            if open { "open" } else { "read to its end" }
        );
        assert_eq!(
            got,
            each.min(chunk.get()) * run::TURN_CALLS,
            "{each}-byte reads"
        );
        assert!(
            got <= run::TURN_BYTES,
            "{each}-byte reads: {got} bytes in one turn"
        );
    }
    // A turn whose time is up reads nothing.
    let (reads, chunk) = (Cell::new(0), Cell::new(0));
    let mut pipe = Some(Endless {
        each: 8,
        reads: &reads,
        chunk: &chunk,
        cap: 10,
    });
    assert!(run::drain(&mut pipe, Instant::now(), |_| {}).is_none());
    assert_eq!((reads.get(), pipe.is_some()), (0, true));
}

// The owner's list (`ingest --exclude`), on the synthetic set: every record named is made up.

/// What ingest leaves in a work dir: `ingest.json`, `gates.jsonl`, `coverage.txt` and every file
/// in `slices/` (name and bytes).
type Left = (
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    std::collections::BTreeMap<String, Vec<u8>>,
);

/// [`Left`] of `w`.
fn left_in(w: &Work) -> Left {
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

/// A list file holding `text`, in a temp dir of its own outside the repository.
fn list(name: &str, text: &[u8]) -> PathBuf {
    let path = temp(&format!("list-{name}")).join("exclude.txt");
    std::fs::write(&path, text).unwrap();
    path
}

/// The number of the item of `plan` that holds record `r`.
fn item_of(plan: &group::Plan, r: u16) -> u32 {
    plan.items
        .iter()
        .position(|i| i.images().any(|id| id == tile(r)))
        .unwrap() as u32
}

/// The rows of `gates.jsonl` that the list wrote.
fn excluded_rows(w: &Work) -> Vec<serde_json::Value> {
    std::fs::read_to_string(w.path("gates.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .filter(|l| l["fallback"] == "excluded")
        .collect()
}

/// `e` with every "line N" taken out names no number (so no record).
fn names_no_number(e: &str) -> bool {
    let mut parts = e.split("line ");
    let mut rest = parts.next().unwrap_or_default().to_string();
    for p in parts {
        rest.push_str(p.trim_start_matches(|c: char| c.is_ascii_digit()));
    }
    !rest.chars().any(|c| c.is_ascii_digit())
}

#[test]
fn the_owner_s_list_sends_its_items_back_to_the_scaler_whole() {
    use std::collections::BTreeSet;
    let g = synthetic::game();
    let plan = group::plan(&g);
    let w = work("exclude");
    atlas_into(&g, &w);
    generate(&w, |_, s| s);
    let t = Thresholds::default();
    let r0 = ingest::ingest(&g, &w, &t).unwrap();
    let all = accepted(&w);
    let before = left_in(&w);
    let (table_no, walker_no) = (
        item_of(&plan, synthetic::TABLE),
        item_of(&plan, synthetic::ACTOR_A),
    );
    let table = members(&g, &plan.items[table_no as usize]);
    let walker = members(&g, &plan.items[walker_no as usize]);
    assert_eq!((table.len(), walker.len()), (2, 8));
    assert!(table.is_subset(&all) && walker.is_subset(&all));
    // The table's west part (not its anchor) and one frame of the walker, in both forms, among
    // comments, blank lines and spaces.
    let text = format!(
        "\u{feff}# made-up records of the synthetic set\n\n  t:{}  # the west part\n\t{}\r\n# end",
        synthetic::TABLE,
        synthetic::ACTOR_A + 5
    );
    let file = list("whole", text.as_bytes());
    let r = ingest::ingest_excluding(&g, &w, &t, Some(&file)).unwrap();
    // Every record of both items goes (the anchor, all eight frames), and nothing else.
    let gone: BTreeSet<String> = table.union(&walker).cloned().collect();
    let now = accepted(&w);
    assert_eq!(now, all.difference(&gone).cloned().collect());
    assert_eq!(packed(&w), now);
    for name in &gone {
        let id = atlas::parse_image(name).unwrap();
        assert!(!w.slice(&atlas::slice_name(id)).exists(), "{name}");
    }
    // What stays is what it was: its lines of ingest.json and its masters, byte for byte.
    let kept = |json: &[u8]| -> Vec<ingest::Accepted> {
        let v: Vec<ingest::Accepted> = serde_json::from_slice(json).unwrap();
        v.into_iter().filter(|a| !gone.contains(&a.image)).collect()
    };
    let after = left_in(&w);
    assert_eq!(kept(&after.0), kept(&before.0));
    for (name, bytes) in &after.3 {
        assert_eq!(before.3.get(name), Some(bytes), "{name}");
    }
    assert_eq!(after.3.len(), before.3.len() - gone.len());
    // Counted apart, as records of their classes; every other count as before.
    assert_eq!(r.get(Class::Multi, Status::Excluded), 2);
    assert_eq!(r.get(Class::Actor, Status::Excluded), 8);
    assert_eq!(
        r.get(Class::Multi, Status::Generated),
        r0.get(Class::Multi, Status::Generated) - 2
    );
    assert_eq!(
        r.get(Class::Actor, Status::Generated),
        r0.get(Class::Actor, Status::Generated) - 8
    );
    let statuses = [
        Status::Generated,
        Status::Fallback,
        Status::Neighbour,
        Status::Failed,
        Status::Pending,
        Status::Excluded,
    ];
    for class in [
        Class::Portrait,
        Class::Bitmap,
        Class::Terrain,
        Class::Object,
    ] {
        for st in statuses {
            assert_eq!(r.get(class, st), r0.get(class, st), "{class:?} {st:?}");
        }
    }
    assert_eq!((r0.excluded_items, r.excluded_items), (0, 2));
    let text = r.text();
    let header = text.lines().nth(1).unwrap();
    assert_eq!(header.split_whitespace().last(), Some("excluded"));
    let row = |name: &str| -> Vec<String> {
        let line = text.lines().find(|l| l.starts_with(name)).unwrap();
        line[name.len()..]
            .split_whitespace()
            .map(String::from)
            .collect()
    };
    assert_eq!(row("multi-record object records")[5], "2");
    assert_eq!(row("actor records")[5], "8");
    assert_eq!(row("object records")[5], "0");
    assert!(text.contains(
        "excluded by the owner's list (the scaler draws them whole): 2 items, 10 records"
    ));
    assert_eq!(
        std::fs::read_to_string(w.path("coverage.txt")).unwrap(),
        text
    );
    // gates.jsonl: one line per item the list sent back, like a neighbour fallback's.
    let rows = excluded_rows(&w);
    let items: BTreeSet<u64> = rows.iter().map(|l| l["item"].as_u64().unwrap()).collect();
    assert_eq!(
        items,
        [u64::from(table_no), u64::from(walker_no)]
            .into_iter()
            .collect()
    );
    for l in &rows {
        let keys: BTreeSet<&str> = l.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["class", "fallback", "gates", "item"].into_iter().collect()
        );
        assert_eq!(l["gates"], t.text());
    }
    assert!(rows.iter().any(|l| l["class"] == "multi"));
    assert!(rows.iter().any(|l| l["class"] == "actor"));
    // Without the list, the next ingest brings both back as they were.
    assert_eq!(ingest::ingest(&g, &w, &t).unwrap(), r0);
    assert!(left_in(&w) == before);
    let _ = std::fs::remove_dir_all(&w.root);
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn an_item_the_list_names_is_not_re_rolled() {
    let g = synthetic::game();
    let plan = group::plan(&g);
    let w = work("exclude-reroll");
    let jobs = atlas_into(&g, &w);
    // The tile sheet comes back one pixel narrower: its items are queued for their re-roll, all
    // but the walker the list names.
    generate(&w, |job, sent| {
        if job.kind == SheetKind::Tile {
            sent.rect(0, 0, SHEET_W - 1, SHEET_H)
        } else {
            sent
        }
    });
    let walker = members(&g, &plan.items[item_of(&plan, synthetic::ACTOR_A) as usize]);
    let file = list("reroll", format!("t:{}\n", synthetic::ACTOR_A).as_bytes());
    let t = Thresholds::default();
    let r = ingest::ingest_excluding(&g, &w, &t, Some(&file)).unwrap();
    assert_eq!(r.get(Class::Actor, Status::Excluded), 8);
    assert_eq!(r.get(Class::Actor, Status::Pending), 4);
    assert_eq!(r.reroll_sheets, 1);
    let all = w.read_jobs().unwrap();
    assert_eq!(all.len(), jobs.len() + 1);
    let reroll = all.last().unwrap();
    assert_eq!(reroll.attempt, 2);
    assert!(reroll.places.iter().all(|p| !walker.contains(&p.image)));
    let beast = atlas::image_name(tile(synthetic::ACTOR_B));
    assert!(reroll.places.iter().any(|p| p.image == beast));
    // Off the list, it is queued then.
    let r = ingest::ingest(&g, &w, &t).unwrap();
    assert_eq!(r.reroll_sheets, 1);
    let last = w.read_jobs().unwrap().pop().unwrap();
    let on: std::collections::BTreeSet<String> =
        last.places.iter().map(|p| p.image.clone()).collect();
    assert_eq!(on, walker);
    let _ = std::fs::remove_dir_all(&w.root);
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn a_bad_list_is_refused_and_nothing_is_written() {
    use std::os::unix::fs::symlink;
    let g = synthetic::game();
    let w = work("exclude-bad");
    atlas_into(&g, &w);
    generate(&w, |_, s| s);
    let t = Thresholds::default();
    ingest::ingest(&g, &w, &t).unwrap();
    let before = left_in(&w);
    let refused = |name: &str, path: &Path| -> String {
        let e = ingest::ingest_excluding(&g, &w, &t, Some(path)).unwrap_err();
        assert!(left_in(&w) == before, "{name}: ingest wrote something");
        e
    };
    // Each list names the tree first: a list taken in part would change what ingest leaves.
    let tree = synthetic::TREE;
    let mut cases: Vec<(&str, Vec<u8>, usize)> = [
        ("no item", format!("t:{tree}\nt:{}\n", synthetic::SHORE), 2),
        ("past the records", format!("t:{tree}\n# past\n9999\n"), 3),
        ("twice", format!("# twice\nt:{tree}\n\n{tree}\n"), 4),
        ("twice, one form each", format!("{tree}\nt:{tree}\n"), 2),
        ("empty id", format!("t:{tree}\nt:\n"), 2),
        ("letters", format!("t:{tree}\nt:x7\n"), 2),
        ("another kind", format!("t:{tree}\np:a:0\n"), 2),
        ("signed", format!("t:{tree}\n+7\n"), 2),
        ("negative", format!("t:{tree}\nt:-7\n"), 2),
        ("past u16", format!("t:{tree}\nt:70000\n"), 2),
        ("two on a line", format!("t:{tree}\n7 8\n"), 2),
        ("a space inside", format!("t:{tree}\nt: 7\n"), 2),
    ]
    .into_iter()
    .map(|(n, s, l)| (n, s.into_bytes(), l))
    .collect();
    let mut not_text = format!("t:{tree}\n\n").into_bytes();
    not_text.extend_from_slice(b"t:\xff7\n");
    cases.push(("not text", not_text, 3));
    for (name, text, line) in &cases {
        let file = list("bad", text);
        let e = refused(name, &file);
        assert!(e.contains(&format!("line {line}:")), "{name}: {e}");
        assert!(names_no_number(&e), "{name}: {e}");
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }
    // A list in a git work tree, by its path or through a symlink, is refused unread; so is one
    // that cannot be read.
    let d = temp("list-git");
    let good = format!("t:{tree}\n");
    std::fs::create_dir_all(d.join("repo/.git")).unwrap();
    std::fs::create_dir_all(d.join("repo/sub/deeper")).unwrap();
    std::fs::write(d.join("repo/sub/deeper/exclude.txt"), &good).unwrap();
    std::fs::create_dir_all(d.join("linked")).unwrap();
    std::fs::write(d.join("linked/.git"), "gitdir: elsewhere\n").unwrap();
    std::fs::write(d.join("linked/exclude.txt"), &good).unwrap();
    std::fs::create_dir_all(d.join("free")).unwrap();
    symlink(
        d.join("repo/sub/deeper/exclude.txt"),
        d.join("free/exclude.txt"),
    )
    .unwrap();
    for rel in [
        "repo/sub/deeper/exclude.txt",
        "linked/exclude.txt",
        "free/exclude.txt",
    ] {
        let e = refused(rel, &d.join(rel));
        assert!(e.contains("git work tree"), "{rel}: {e}");
    }
    let e = refused("missing", &d.join("free/none.txt"));
    assert!(e.contains("cannot be read"), "{e}");
    // The same list outside any work tree is taken.
    std::fs::write(d.join("free/plain.txt"), &good).unwrap();
    let r = ingest::ingest_excluding(&g, &w, &t, Some(&d.join("free/plain.txt"))).unwrap();
    assert_eq!(r.get(Class::Multi, Status::Excluded), 4);
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&w.root);
}

#[test]
fn no_list_or_one_of_comments_only_changes_nothing() {
    let g = synthetic::game();
    let w = work("exclude-none");
    atlas_into(&g, &w);
    generate(&w, |_, s| s);
    let t = Thresholds::default();
    let r = ingest::ingest(&g, &w, &t).unwrap();
    let before = left_in(&w);
    assert!(excluded_rows(&w).is_empty());
    assert_eq!(r.excluded_items, 0);
    assert!(Class::ALL.iter().all(|&c| r.get(c, Status::Excluded) == 0));
    assert!(r.text().contains(
        "excluded by the owner's list (the scaler draws them whole): 0 items, 0 records"
    ));
    let file = list(
        "comments",
        b"# only comments\n\n   \n\t# and blank lines\r\n",
    );
    assert_eq!(
        ingest::ingest_excluding(&g, &w, &t, Some(&file)).unwrap(),
        r
    );
    assert!(left_in(&w) == before);
    assert_eq!(ingest::ingest_excluding(&g, &w, &t, None).unwrap(), r);
    assert!(left_in(&w) == before);
    let _ = std::fs::remove_dir_all(&w.root);
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

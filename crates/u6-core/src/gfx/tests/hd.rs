//! Spec 07 HD0, the compositor over the game's routines on synthetic art: the final S× RGBA of
//! a merged shore record, a pixel left from a record rewritten since, overlapping records, an
//! outline flash and an overlay, the pointer sprite, a combat replay and one that ends on equal
//! colours (composed on its sources alone), the HD switch ([`HdView`], the platform's) off and on
//! over a persistent panel and a skipped render and a scale change, and a portrait read from its
//! archive; with an HD pack set (spec 07 H2), a shore record the merge rewrote after the pack was
//! made from its earlier bytes. Every frame is checked against [`oracle`], the frame by the
//! definition with no cache and no memory of an earlier frame. No game data.

use std::collections::BTreeMap;

use u6_formats::gfx::{AnimOverlay, Span};
use u6_render::hd::{Compositor, Scale, mmpx, word};
use u6_render::{FLAG_OUTLINE, FLAG_OVERLAY, Framebuffer, HEIGHT, Kind, WIDTH};

use super::plane::{kind, plane, tile};
use super::view::world;
use super::{at_map, at_map16, install, map_tile, object_tile, shown, view_rect};
use crate::Files;
use crate::engine::{Engine, InputEvent, Io, Key};
use crate::geom::{VIEW_H, VIEW_W};
use crate::gfx::GfxState;
use crate::gfx::anim::{merge_shores, set_frame};
use crate::gfx::blit::{blit_back, blit_front, copy_window};
use crate::hd::{HdView, ImageCache};
use crate::state::State;

const SCALES: [Scale; 2] = [Scale::X2, Scale::X4];

/// Colours 1..=31 in diagonal bands with steps, from `seed`: MMPX reshapes their edges.
fn pattern(seed: usize) -> impl Fn(usize, usize) -> u8 {
    move |r, c| (((r + 2 * c + seed) / 5 + (r / 4) * (seed % 3)) % 4 * 7 + 1 + seed % 3) as u8
}

/// The S × S block of screen pixel (x, y) in a frame at `scale`.
fn block(out: &[u32], scale: Scale, (x, y): (usize, usize)) -> Vec<u32> {
    let s = scale.factor();
    (0..s * s)
        .map(|k| out[(y * s + k / s) * WIDTH * s + x * s + k % s])
        .collect()
}

/// Block (u, v) of MMPX ×S of record `id` as `gfx` holds it now, in `fb`'s palette, its
/// transparent sub-pixels `classic`.
fn record_block(
    gfx: &GfxState,
    fb: &Framebuffer,
    id: u16,
    (u, v): (usize, usize),
    scale: Scale,
    classic: u32,
) -> Vec<u32> {
    let rec = &gfx.records[usize::from(id)];
    let input: Vec<u32> = (0..256)
        .map(|i| {
            if rec.transparent[i] {
                0
            } else {
                word(fb.palette[usize::from(rec.pixels[i])])
            }
        })
        .collect();
    let mut hd = Vec::new();
    mmpx(&input, 16, 16, scale, &mut hd);
    let s = scale.factor();
    (0..s * s)
        .map(|k| hd[(v * s + k / s) * 16 * s + u * s + k % s])
        .map(|c| if c == 0 { classic } else { c })
        .collect()
}

/// The HD frame of `fb` by the definition (spec 07 HD0), pixel by pixel: a tile pixel without
/// flags whose version is its record's now takes its block of MMPX ×S of the record as `gfx`
/// holds it now, every other pixel its classic colour S × S. Tiles only.
fn oracle(fb: &Framebuffer, gfx: &GfxState, scale: Scale) -> Vec<u32> {
    let s = scale.factor();
    let mut out = vec![0; WIDTH * s * HEIGHT * s];
    let mut blocks: BTreeMap<(u16, usize, usize, u32), Vec<u32>> = BTreeMap::new();
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let src = fb.src_at(x, y).unwrap();
            assert!(matches!(src.kind, Kind::Classic | Kind::Tile), "tiles only");
            let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
            let (u, v) = (usize::from(src.u), usize::from(src.v));
            let hd = src.kind == Kind::Tile
                && src.flags == 0
                && src.ver == gfx.cache.version(usize::from(src.id))
                && u < 16
                && v < 16;
            let b = if hd {
                blocks
                    .entry((src.id, u, v, classic))
                    .or_insert_with(|| record_block(gfx, fb, src.id, (u, v), scale, classic))
                    .clone()
            } else {
                vec![classic; s * s]
            };
            for (k, c) in b.into_iter().enumerate() {
                out[(y * s + k / s) * WIDTH * s + x * s + k % s] = c;
            }
        }
    }
    out
}

/// `c` composes `fb` from `s` (and `io`) at `scale` and must give the oracle's frame.
fn check(
    c: &mut Compositor,
    images: &mut ImageCache,
    fb: &Framebuffer,
    (s, io): (&State, &Io),
    scale: Scale,
    what: &str,
) {
    c.compose(fb, &mut images.images(s, io), scale);
    assert!(
        c.rgba() == oracle(fb, &s.gfx, scale).as_slice(),
        "{what}: the definition's frame"
    );
}

/// A palette whose 32 colours differ.
fn palette(fb: &mut Framebuffer) {
    for (i, p) in fb.palette.iter_mut().enumerate() {
        *p = u6_render::Rgb::new((i * 8) as u8, (i * 37 % 256) as u8, 255 - (i * 5) as u8);
    }
}

/// Blocks of `out` at the pixels of `pixels` that differ from the pixel's classic colour scaled:
/// MMPX changed the art there.
fn reshaped(
    out: &[u32],
    fb: &Framebuffer,
    scale: Scale,
    pixels: impl Iterator<Item = (usize, usize)>,
) -> usize {
    pixels
        .filter(|&(x, y)| {
            let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
            block(out, scale, (x, y)).iter().any(|&c| c != classic)
        })
        .count()
}

const WATER: usize = 20;
const SHORE: usize = 21;

/// As `plane`'s shore state, with patterned water: its shore merge 0 takes every pixel of
/// [`WATER`] into [`SHORE`] (one rewrite per colour plane).
fn shore_state() -> State {
    let mut s = State::empty();
    install(&mut s.gfx, WATER, map_tile(0, 16, pattern(4)));
    install(&mut s.gfx, SHORE, map_tile(0, 16, |_, _| 0b01010));
    s.gfx.tables.shore_src[0] = (WATER * 4) as u16;
    s.gfx.tables.shore_dst[0] = (SHORE * 4) as u16;
    let mut b2 = vec![0u8; 64];
    for i in 1..32 {
        b2[2 * i..2 * i + 2].copy_from_slice(&0xF000u16.to_le_bytes());
    }
    let rec = b2.len() as u16;
    b2.extend([0, 16]);
    for _ in 0..16 {
        b2.extend([0xFF, 0xFF]);
    }
    b2[0..2].copy_from_slice(&rec.wrapping_sub(0x400).to_le_bytes());
    s.gfx.animmask.block2 = b2;
    s
}

/// The shore record merged, then drawn, copied to the screen and presented: its pixels are
/// current, and the HD frame shows MMPX of its bytes now (the water's), not of the bytes it had.
#[test]
fn a_merged_shore_record_shows_mmpx_of_its_current_bytes() {
    let original = shore_state().gfx;
    let mut e = Engine::with_game(shore_state(), Files::default(), |st, io| async move {
        {
            let mut s = st.borrow_mut();
            merge_shores(&mut s.gfx);
            let (x, y) = at_map16(16, 16);
            io.draw_back(|b| blit_back(&s.gfx, b, SHORE as u16, x, y));
            io.with_buffers(|b, f| copy_window(b, f));
        }
        io.present();
        io.sleep(u32::MAX).await;
    });
    e.enable_src_plane();
    e.tick(&[], 16);
    let s = e.state();
    assert!(s.gfx.cache.version(SHORE) > 0 && s.gfx.records[SHORE] != original.records[SHORE]);
    // The window's cell at (16, 16) from the map's corner.
    let (x0, y0) = at_map(16, 16);
    let cell = || (y0..y0 + 16).flat_map(move |y| (x0..x0 + 16).map(move |x| (x, y)));
    for scale in SCALES {
        let mut c = Compositor::new();
        let mut fb = e.frame().clone();
        palette(&mut fb);
        check(
            &mut c,
            &mut ImageCache::new(),
            &fb,
            (&*s, e.io()),
            scale,
            "merged",
        );
        let ver = s.gfx.cache.version(SHORE);
        for (x, y) in cell() {
            assert_eq!(
                fb.src_at(x, y),
                Some(tile(
                    SHORE as u16,
                    ver,
                    ((x - x0) as u16, (y - y0) as u16),
                    0
                ))
            );
            let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
            let now = record_block(&s.gfx, &fb, SHORE as u16, (x - x0, y - y0), scale, classic);
            assert_eq!(
                block(c.rgba(), scale, (x, y)),
                now,
                "({x}, {y}): MMPX of its bytes now"
            );
        }
        assert!(
            reshaped(c.rgba(), &fb, scale, cell()) > 20,
            "MMPX reshaped the merged shore"
        );
        let then = cell().filter(|&(x, y)| {
            let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
            block(c.rgba(), scale, (x, y))
                == record_block(
                    &original,
                    &fb,
                    SHORE as u16,
                    (x - x0, y - y0),
                    scale,
                    classic,
                )
        });
        assert_eq!(then.count(), 0, "not the bytes it had");
    }
}

/// Spec 07 acceptance 2 with a pack loaded (H2): a pack whose images of the shore and the water
/// records were made from their bytes before the merge (the test's own pictures, keyed by
/// `u6_hdpack::tile_hash` of those bytes). Before the merge both cells show the pack's images;
/// once the merge rewrote the shore record, its pack entry (made from its earlier bytes) is not
/// used: the redrawn shore cell shows MMPX of its bytes now, the water (unchanged) still the
/// pack's image.
#[test]
fn with_a_pack_a_merged_shore_record_shows_mmpx_of_its_current_bytes() {
    use std::rc::Rc;
    use u6_hdpack::{Generator, ImageId, PackImage, Rgba, read_pack, tile_hash, write_pack};
    use u6_render::hd::{over, pack_words};

    let original = shore_state().gfx;
    let key = |r: usize| {
        tile_hash(
            &original.records[r].pixels,
            &original.records[r].transparent,
        )
    };
    let picture = |seed: u8| {
        let mut img = Rgba::new(64, 64);
        for (i, p) in img.pixels.chunks_exact_mut(4).enumerate() {
            let a = if i % 13 == 0 { 0 } else { 255 };
            p.copy_from_slice(&[(i % 64) as u8 * 4, seed, (i / 64) as u8 * 3, a]);
        }
        img
    };
    let images = [SHORE, WATER].map(|r| PackImage {
        id: ImageId::Tile(r as u16),
        hash: key(r).unwrap(),
        image: picture(r as u8),
    });
    let g = Generator {
        name: "test".into(),
        version: "0".into(),
        engine: "none".into(),
        date: "2026-10-16".into(),
    };
    let read = read_pack(&write_pack(&images, &g, true).unwrap()).unwrap();
    let pack = Rc::new(crate::hd::HdPack::new(read));
    let mut e = Engine::with_game(shore_state(), Files::default(), |st, io| async move {
        {
            let s = st.borrow();
            io.draw_back(|b| {
                let ((sx, sy), (wx, wy)) = (at_map16(16, 16), at_map16(48, 16));
                blit_back(&s.gfx, b, SHORE as u16, sx, sy);
                blit_back(&s.gfx, b, WATER as u16, wx, wy);
            });
            io.with_buffers(|b, f| copy_window(b, f));
        }
        io.present();
        let _ = io.next_key().await;
        {
            let mut s = st.borrow_mut();
            merge_shores(&mut s.gfx);
            let (x, y) = at_map16(16, 16);
            io.draw_back(|b| blit_back(&s.gfx, b, SHORE as u16, x, y));
            io.with_buffers(|b, f| copy_window(b, f));
        }
        io.present();
        io.sleep(u32::MAX).await;
    });
    e.enable_src_plane();
    e.tick(&[], 16);
    // The window's cells at (16, 16) and (48, 16) from the map's corner, by their left column.
    let (shore_x, top) = at_map(16, 16);
    let water_x = at_map(48, 16).0;
    let cell = |x0: usize| (top..top + 16).flat_map(move |y| (x0..x0 + 16).map(move |x| (x, y)));
    // Pack image `i`'s block (u, v) at `scale` over `classic`.
    let pack_block = |i: usize, (u, v): (usize, usize), scale: Scale, classic: u32| {
        let mut words = Vec::new();
        pack_words(&images[i].image.pixels, 64, 64, scale, &mut words);
        let s = scale.factor();
        (0..s * s)
            .map(|k| over(words[(v * s + k / s) * 16 * s + u * s + k % s], classic))
            .collect::<Vec<u32>>()
    };
    let mut fb = e.frame().clone();
    palette(&mut fb);
    let mut kept = [Compositor::new(), Compositor::new()];
    for (c, scale) in kept.iter_mut().zip(SCALES) {
        c.set_pack(Some(pack.clone()));
        let s = e.state();
        c.compose(&fb, &mut ImageCache::new().images(&s, e.io()), scale);
        for (i, x0) in [(0, shore_x), (1, water_x)] {
            for (x, y) in cell(x0) {
                let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
                let want = pack_block(i, (x - x0, y - top), scale, classic);
                assert_eq!(
                    block(c.rgba(), scale, (x, y)),
                    want,
                    "before the merge: the pack's"
                );
            }
        }
    }
    e.tick(&[InputEvent::Key(Key::Char('m'))], 16);
    let s = e.state();
    assert!(s.gfx.cache.version(SHORE) > 0 && s.gfx.records[SHORE] != original.records[SHORE]);
    assert_eq!(
        key(WATER),
        tile_hash(
            &s.gfx.records[WATER].pixels,
            &s.gfx.records[WATER].transparent
        )
    );
    let mut fb = e.frame().clone();
    palette(&mut fb);
    for (c, scale) in kept.iter_mut().zip(SCALES) {
        let mut fresh = Compositor::new();
        fresh.set_pack(Some(pack.clone()));
        for c in [&mut *c, &mut fresh] {
            c.compose(&fb, &mut ImageCache::new().images(&s, e.io()), scale);
            for (x, y) in cell(shore_x) {
                let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
                let at = (x - shore_x, y - top);
                let now = record_block(&s.gfx, &fb, SHORE as u16, at, scale, classic);
                assert_eq!(
                    block(c.rgba(), scale, (x, y)),
                    now,
                    "the shore: MMPX of its bytes now"
                );
                let packed = pack_block(0, at, scale, classic);
                assert_ne!(block(c.rgba(), scale, (x, y)), packed, "not its pack image");
            }
            for (x, y) in cell(water_x) {
                let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
                let want = pack_block(1, (x - water_x, y - top), scale, classic);
                assert_eq!(
                    block(c.rgba(), scale, (x, y)),
                    want,
                    "the water: still the pack's"
                );
            }
        }
        assert!(
            c.rgba() == fresh.rgba(),
            "the kept compositor equals a new one"
        );
    }
}

/// The shore record drawn into the back buffer, then the merge rewrites it, then the window is
/// copied and presented: the pixels name a version the record has left, and the HD frame shows
/// their classic colours, which MMPX of the record now would not.
#[test]
fn a_pixel_left_while_its_record_is_rewritten_is_its_classic_colour() {
    let mut e = Engine::with_game(shore_state(), Files::default(), |st, io| async move {
        {
            let mut s = st.borrow_mut();
            let (x, y) = at_map16(16, 16);
            io.draw_back(|b| blit_back(&s.gfx, b, SHORE as u16, x, y));
            merge_shores(&mut s.gfx);
            io.with_buffers(|b, f| copy_window(b, f));
        }
        io.present();
        io.sleep(u32::MAX).await;
    });
    e.enable_src_plane();
    e.tick(&[], 16);
    let s = e.state();
    let mut fb = e.frame().clone();
    palette(&mut fb);
    for scale in SCALES {
        let mut c = Compositor::new();
        check(
            &mut c,
            &mut ImageCache::new(),
            &fb,
            (&*s, e.io()),
            scale,
            "stale",
        );
        let mut differ = 0;
        let (x0, y0) = at_map(16, 16);
        for y in y0..y0 + 16 {
            for x in x0..x0 + 16 {
                let src = fb.src_at(x, y).unwrap();
                assert!(
                    src.kind == Kind::Tile && src.ver != s.gfx.cache.version(SHORE),
                    "stale"
                );
                let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
                let got = block(c.rgba(), scale, (x, y));
                assert!(got.iter().all(|&g| g == classic), "({x}, {y}): classic");
                differ += usize::from(
                    got != record_block(
                        &s.gfx,
                        &fb,
                        SHORE as u16,
                        (x - x0, y - y0),
                        scale,
                        classic,
                    ),
                );
            }
        }
        assert_eq!(
            differ, 256,
            "the record now would show other colours everywhere"
        );
    }
}

/// An object record with transparent pixels over a map record, an outline flash and an animated
/// overlay, in the final frame: an opaque object pixel shows the object's block, a transparent
/// one the map record's block under it, a flagged pixel its classic colour.
#[test]
fn overlapping_records_an_outline_and_an_overlay_in_the_final_frame() {
    let mut s = State::empty();
    install(&mut s.gfx, 10, map_tile(0, 16, pattern(10)));
    let holes = pattern(3);
    install(
        &mut s.gfx,
        3,
        object_tile(Span::Full, |r, c| {
            ((r * 3 + c) % 7 != 0).then(|| holes(r, c) + 1)
        }),
    );
    // Colour 0 where the outline paints.
    install(
        &mut s.gfx,
        4,
        object_tile(Span::Full, |r, c| {
            (r > 0).then_some(if c < 8 { 0 } else { 9 })
        }),
    );
    let mut t = map_tile(0, 16, pattern(6));
    t.masktype = 4;
    t.anim_ref = Some(0);
    install(&mut s.gfx, 6, t);
    s.gfx.overlays[6] = Some(AnimOverlay {
        first_row: 3,
        rows: 2,
        frames: (0..8).map(|f| vec![0x8000 >> f, 0x0f00]).collect(),
    });
    set_frame(&mut s.gfx, 2);
    let mut fb = plane();
    palette(&mut fb);
    fb.clear(1);
    blit_back(&s.gfx, &mut fb, 10, 64, 64);
    blit_back(&s.gfx, &mut fb, 3, 64, 64);
    blit_back(&s.gfx, &mut fb, 6, 16, 0);
    blit_back(&s.gfx, &mut fb, 4 | 0x0800, 128, 32);
    // Who owns each pixel of the overlap, from the object's transparency formula alone: the
    // object where it is opaque, the map record under it elsewhere, each at the pixel's place.
    let owner = |c: usize, r: usize| {
        if !(r * 3 + c).is_multiple_of(7) {
            3u16
        } else {
            10
        }
    };
    for r in 0..16 {
        for c in 0..16 {
            let want = tile(owner(c, r), 0, (c as u16, r as u16), 0);
            assert_eq!(
                fb.src_at(64 + c, 64 + r),
                Some(want),
                "({c}, {r}): its owner"
            );
        }
    }
    let io = Io::new(Files::default());
    for scale in SCALES {
        let mut c = Compositor::new();
        check(
            &mut c,
            &mut ImageCache::new(),
            &fb,
            (&s, &io),
            scale,
            "overlapping",
        );
        for r in 0..16 {
            for col in 0..16 {
                let (x, y) = (64 + col, 64 + r);
                let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
                let want = record_block(&s.gfx, &fb, owner(col, r), (col, r), scale, classic);
                assert_eq!(
                    block(c.rgba(), scale, (x, y)),
                    want,
                    "({col}, {r}): its owner's block"
                );
            }
        }
        let mut seen = BTreeMap::new();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let src = fb.src_at(x, y).unwrap();
                let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
                let got = block(c.rgba(), scale, (x, y));
                let what = match (src.kind, src.flags) {
                    (Kind::Tile, 0) => {
                        let uv = (usize::from(src.u), usize::from(src.v));
                        assert_eq!(
                            got,
                            record_block(&s.gfx, &fb, src.id, uv, scale, classic),
                            "({x}, {y})"
                        );
                        if (64..80).contains(&x) && (64..80).contains(&y) {
                            if src.id == 3 { "object" } else { "under" }
                        } else {
                            "map"
                        }
                    }
                    (Kind::Tile, f) => {
                        assert!(
                            got.iter().all(|&g| g == classic),
                            "({x}, {y}): flagged, classic"
                        );
                        if f == FLAG_OVERLAY {
                            "overlay"
                        } else {
                            assert_eq!(f, FLAG_OUTLINE);
                            "outline"
                        }
                    }
                    _ => "classic",
                };
                *seen.entry(what).or_insert(0) += 1;
            }
        }
        for what in ["object", "under", "map", "overlay", "outline", "classic"] {
            assert!(
                seen.get(what).copied().unwrap_or(0) > 0,
                "{what} pixels: {seen:?}"
            );
        }
        let object = (64..80).flat_map(|y| (64..80).map(move |x| (x, y)));
        assert!(
            reshaped(c.rgba(), &fb, scale, object) > 10,
            "MMPX reshaped the object"
        );
    }
}

/// The pointer sprite over HD art: its pixels the classic colours, the art beside it HD.
#[test]
fn the_pointer_sprite_stays_classic_over_hd_art() {
    let mut s = State::empty();
    s.ui.pointer =
        crate::ui::pointer::PointerState::from_tables(crate::ui::pointer::PointerTables {
            shapes: vec![crate::ui::pointer::Shape {
                dx: 0,
                dy: 0,
                rows: vec![[1; 16]; 4],
            }],
            shown: true,
            ..Default::default()
        });
    install(&mut s.gfx, 7, map_tile(0, 16, pattern(7)));
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        io.draw(|f| {
            for x in (192..240).step_by(16) {
                blit_front(&st.borrow().gfx, f, 7, x, 96);
            }
        });
        io.present();
        io.sleep(u32::MAX).await;
    });
    e.enable_src_plane();
    let at = InputEvent::Pointer {
        x: 200,
        y: 100,
        buttons: 0,
    };
    e.tick(&[at], 16);
    e.tick(&[], 16);
    let s = e.state();
    let fb = e.frame();
    for scale in SCALES {
        let mut c = Compositor::new();
        let mut fb = fb.clone();
        palette(&mut fb);
        let fb = &fb;
        check(
            &mut c,
            &mut ImageCache::new(),
            fb,
            (&*s, e.io()),
            scale,
            "the pointer",
        );
        let sprite = (96..112)
            .flat_map(|y| (192..240).map(move |x| (x, y)))
            .filter(|&(x, y)| kind(fb, x, y) == Kind::Classic);
        let mut n = 0;
        for (x, y) in sprite {
            let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
            assert!(block(c.rgba(), scale, (x, y)).iter().all(|&g| g == classic));
            n += 1;
        }
        assert!(n > 0, "the sprite is shown");
        let art = (96..112)
            .flat_map(|y| (192..240).map(move |x| (x, y)))
            .filter(|&(x, y)| kind(fb, x, y) == Kind::Tile);
        assert!(
            reshaped(c.rgba(), fb, scale, art) > 10,
            "the art beside it is HD"
        );
    }
}

/// Combat playback over the plane (as `plane`'s playback test, the records patterned): every
/// frame shown during the replay, composed by one kept compositor, is the definition's frame,
/// and the replayed window shows HD art.
#[test]
fn a_combat_replay_composes_the_hd_art_it_put_back() {
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    for n in 1..=4 {
        install(&mut s.gfx, n, map_tile(0, 16, pattern(n)));
        for _ in 0..n {
            s.gfx.cache.record_changed(n);
        }
    }
    let fill = |gfx: &GfxState, f: &mut Framebuffer, n: u16| {
        for row in 0..VIEW_H {
            for col in 0..VIEW_W {
                let (x, y) = at_map16(col * 16, row * 16);
                blit_front(gfx, f, n, x, y);
            }
        }
    };
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        io.draw(|f| fill(&st.borrow().gfx, f, 1));
        io.present();
        io.vblank().await;
        {
            let mut s = st.borrow_mut();
            io.draw(|f| fill(&s.gfx, f, 2));
            io.present();
            crate::combat::present::capture_window(&mut s, 40);
            io.draw(|f| fill(&s.gfx, f, 3));
            io.present();
            crate::combat::present::capture_dissolve(&mut s, 200);
            io.draw(|f| fill(&s.gfx, f, 4));
            io.present();
        }
        crate::combat::play_effects(&st, &io).await;
        io.sleep(u32::MAX).await;
    });
    e.enable_src_plane();
    let (mut c, mut images) = (Compositor::new(), ImageCache::new());
    let (mut frames, mut version, mut records) = (0, 0, Vec::new());
    for _ in 0..200 {
        e.tick(&[], 16);
        if e.frame_version() == version {
            continue;
        }
        version = e.frame_version();
        let s = e.state();
        let mut fb = e.frame().clone();
        palette(&mut fb);
        check(
            &mut c,
            &mut images,
            &fb,
            (&*s, e.io()),
            Scale::X4,
            "the replay",
        );
        let (xs, ys) = shown();
        let window = ys.flat_map(|y| xs.clone().map(move |x| (x, y)));
        assert!(
            reshaped(c.rgba(), &fb, Scale::X4, window) > 1000,
            "HD art in the window"
        );
        let (cx, cy) = at_map(88, 88);
        records.push(fb.src_at(cx, cy).unwrap().id);
        frames += 1;
    }
    records.dedup();
    assert_eq!(
        records,
        [1, 2, 3, 4],
        "the published window, the two frames, the last picture"
    );
    assert!(frames >= 4);
}

/// The platform's HD switch ([`HdView`], what the page's `Game::set_hd` and `Game::present`
/// run) over the engine's shown frame: a panel drawn once and never again, and a map window
/// refreshed every tick (spec 06 O2: a refresh with nothing changed draws nothing). On, every
/// frame is the definition's; off, ticks with skipped renders and text drawn on the panel run
/// and nothing is composed; on again, the frame is the definition's at once, with no tick, and a
/// new compositor's; then the scale changes over the same pixels and back. The panel and the
/// window show HD art throughout.
#[test]
fn the_hd_switch_over_a_persistent_panel_and_a_skipped_render_is_exact() {
    let (mut s, mut g) = world();
    for n in 0..64 {
        install(&mut g, n, map_tile(0, 16, pattern(n)));
    }
    s.gfx = g;
    for (i, c) in s.tables.palette.iter_mut().enumerate() {
        *c = u6_formats::gfx::Rgb {
            r: (i * 8) as u8,
            g: (i * 37 % 256) as u8,
            b: 255 - (i * 5) as u8,
        };
    }
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        io.draw(|f| {
            let gfx = &st.borrow().gfx;
            for k in 0..8u16 {
                blit_front(gfx, f, 40 + k, 176 + 16 * (k % 4), 8 + 16 * (k / 4));
            }
        });
        loop {
            while io.key_ready() {
                if io.next_key().await == Key::Char('t') {
                    io.draw(|f| {
                        for x in 180..200 {
                            f.set(x, 12, 3);
                        }
                    });
                }
            }
            crate::gfx::refresh_view(&mut st.borrow_mut(), &io);
            io.vblank().await;
        }
    });
    e.enable_src_plane();
    let blank = Framebuffer::new();
    let panel = || (8..40).flat_map(|y| (176..240).map(move |x| (x, y)));
    let window = || {
        let (xs, ys) = shown();
        ys.flat_map(move |y| xs.clone().map(move |x| (x, y)))
    };
    let exact = |view: &HdView, e: &Engine, scale, what: &str| {
        let fb = e.frame();
        assert!(
            view.rgba() == oracle(fb, &e.state().gfx, scale).as_slice(),
            "{what}: the definition's frame"
        );
        assert!(
            reshaped(view.rgba(), fb, scale, panel()) > 50,
            "{what}: the panel is HD"
        );
        assert!(
            reshaped(view.rgba(), fb, scale, window()) > 1000,
            "{what}: the window is HD"
        );
    };
    let mut view = HdView::new();
    assert!(view.set(true, Scale::X4));
    for t in 0..4 {
        e.tick(&[], 16);
        view.present(Some(&e), &blank);
        exact(&view, &e, Scale::X4, &format!("on, tick {t}"));
    }
    // Off: the refreshes left out, the text drawn on the panel, presents; nothing composed.
    assert!(view.set(false, Scale::X4));
    let held = view.rgba().to_vec();
    let skips = e.state().gfx.cache.window_skips;
    e.tick(&[InputEvent::Key(Key::Char('t'))], 16);
    for _ in 0..5 {
        e.tick(&[], 16);
        assert!(!view.present(Some(&e), &blank), "off: nothing composed");
    }
    assert!(
        e.state().gfx.cache.window_skips >= skips + 5,
        "the renders were left out"
    );
    assert!(e.frame().get(185, 12) == Some(3), "the text is shown");
    assert!(
        view.rgba() == held.as_slice(),
        "off: the HD frame as it was"
    );
    // On again: composed at once, with no tick.
    assert!(view.set(true, Scale::X4));
    assert!(
        view.present(Some(&e), &blank),
        "on: the text's blocks changed"
    );
    exact(&view, &e, Scale::X4, "on again");
    let mut fresh = Compositor::new();
    let st = e.state();
    fresh.compose(
        e.frame(),
        &mut ImageCache::new().images(&st, e.io()),
        Scale::X4,
    );
    drop(st);
    assert!(fresh.rgba() == view.rgba(), "a new compositor's frame");
    let text = word(e.frame().palette[3]);
    assert!(
        block(view.rgba(), Scale::X4, (185, 12))
            .iter()
            .all(|&c| c == text),
        "the text classic"
    );
    // The scale changes over the same pixels, and back.
    for scale in [Scale::X2, Scale::X4] {
        assert!(view.set(true, scale));
        view.present(Some(&e), &blank);
        exact(&view, &e, scale, &format!("{scale:?}"));
    }
    // And the ticks after it.
    for t in 0..3 {
        e.tick(&[], 16);
        view.present(Some(&e), &blank);
        exact(&view, &e, Scale::X4, &format!("after, tick {t}"));
    }
}

/// A replay whose recorded frame has the colours of the screen drawn classic and whose last
/// picture is the record again (equal colours, after a pause): the end puts back the record's
/// sources with no new frame. The platform's switch ([`HdView`]) composes on the source version
/// alone there, and the HD frame changes (classic blocks back to the record's MMPX ones); every
/// frame is the definition's.
#[test]
fn a_replay_ending_on_equal_colours_is_composed_on_its_sources_alone() {
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    install(&mut s.gfx, 1, map_tile(0, 16, pattern(1)));
    let tiles = |gfx: &GfxState, f: &mut Framebuffer| {
        for row in 0..VIEW_H {
            for col in 0..VIEW_W {
                let (x, y) = at_map16(col * 16, row * 16);
                blit_front(gfx, f, 1, x, y);
            }
        }
    };
    let classic = |gfx: &GfxState, f: &mut Framebuffer| {
        let rec = &gfx.records[1];
        let (xs, ys) = view_rect();
        for y in ys {
            for x in xs.clone() {
                f.set(x, y, rec.pixels[y % 16 * 16 + x % 16]);
            }
        }
    };
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        io.draw(|f| {
            palette(f);
            tiles(&st.borrow().gfx, f);
        });
        io.present();
        io.vblank().await;
        {
            let mut s = st.borrow_mut();
            io.draw(|f| classic(&s.gfx, f));
            io.present();
            crate::combat::present::capture_window(&mut s, 40);
            crate::combat::present::capture_pause(&mut s, 60);
            io.draw(|f| tiles(&s.gfx, f));
            io.present();
        }
        crate::combat::play_effects(&st, &io).await;
        io.sleep(u32::MAX).await;
    });
    e.enable_src_plane();
    let blank = Framebuffer::new();
    let mut view = HdView::new();
    view.set(true, Scale::X4);
    let (mut last, mut sources_alone, mut composed) = ((0, 0), 0, 0);
    for t in 0..300 {
        e.tick(&[], 16);
        let now = (e.frame_version(), e.src_version());
        let before = view.rgba().to_vec();
        composed += usize::from(view.present(Some(&e), &blank));
        assert!(
            view.rgba() == oracle(e.frame(), &e.state().gfx, Scale::X4).as_slice(),
            "tick {t}: the definition's frame"
        );
        if now.0 == last.0 && now.1 != last.1 {
            sources_alone += 1;
            assert!(
                view.rgba() != before.as_slice(),
                "tick {t}: the sources alone changed the HD frame"
            );
        }
        last = now;
    }
    assert!(e.state().combat.effects.is_empty(), "all played");
    assert_eq!(sources_alone, 1, "one change of the sources alone");
    assert!(
        composed >= 3,
        "the record, the classic frame, the record again"
    );
    let fb = e.frame();
    let (xs, ys) = view_rect();
    let window = ys.flat_map(|y| xs.clone().map(move |x| (x, y)));
    assert!(
        reshaped(view.rgba(), fb, Scale::X4, window) > 1000,
        "HD art at the end"
    );
}

/// A portrait drawn from its archive (synthetic: a one-item `portrait.a`-style library of an LZW
/// stream) through the bitmap colour table: the HD frame shows MMPX of the portrait as drawn.
#[test]
fn a_portrait_from_its_archive_shows_mmpx_of_its_colours() {
    use crate::converse::text::{ConverseText, MESSAGES, Msg};
    let face: Vec<u8> = (0..56 * 64)
        .map(|i| pattern(5)(i / 56, i % 56) + 40)
        .collect();
    let stream = u6_formats::lzw::encode(&face).unwrap();
    let mut lib = 4u32.to_le_bytes().to_vec();
    lib.extend(stream);
    let files = Files::new(BTreeMap::from([("facefile.a".to_string(), lib)]));
    let mut s = State::empty();
    s.converse.text = Some(ConverseText::from_pairs(MESSAGES.iter().map(|&(m, _)| {
        let t: &[u8] = if m == Msg::PortraitFile {
            b"facefile.?"
        } else {
            b""
        };
        (m, t.to_vec())
    })));
    s.gfx.tables.bitmap_colours = std::array::from_fn(|i| (i as u8).wrapping_sub(40) | 0x20);
    let mut e = Engine::with_game(s, files, |st, io| async move {
        crate::converse::draw_portrait(&st.borrow(), &io, 0, 0xd8, 0x18);
        io.present();
        io.sleep(u32::MAX).await;
    });
    e.enable_src_plane();
    e.tick(&[], 16);
    let s = e.state();
    let mut fb = e.frame().clone();
    palette(&mut fb);
    let s = &*s;
    let (x0, y0) = (0xd8, 0x18);
    assert_eq!(
        fb.src_at(x0 + 3, y0 + 2).map(|s| (s.kind, s.u, s.v)),
        Some((Kind::Portrait, 3, 2))
    );
    let drawn: Vec<u32> = (0..56 * 64)
        .map(|i| word(fb.palette[usize::from(fb.get(x0 + i % 56, y0 + i / 56).unwrap())]))
        .collect();
    assert!(drawn.iter().zip(&face).all(|(&d, &f)| d
        == word(fb.palette[usize::from(s.gfx.tables.bitmap_colours[usize::from(f)] & 0x1f)])));
    for scale in SCALES {
        let mut hd = Vec::new();
        mmpx(&drawn, 56, 64, scale, &mut hd);
        let mut c = Compositor::new();
        let mut images = ImageCache::new();
        c.compose(&fb, &mut images.images(s, e.io()), scale);
        let sf = scale.factor();
        for v in 0..64 {
            for u in 0..56 {
                let want: Vec<u32> = (0..sf * sf)
                    .map(|k| hd[(v * sf + k / sf) * 56 * sf + u * sf + k % sf])
                    .collect();
                assert_eq!(block(c.rgba(), scale, (x0 + u, y0 + v)), want, "({u}, {v})");
            }
        }
        let face = (0..64).flat_map(|v| (0..56).map(move |u| (x0 + u, y0 + v)));
        assert!(
            reshaped(c.rgba(), &fb, scale, face) > 50,
            "MMPX reshaped the portrait"
        );
        // Composed again from the decoded cache: the same frame.
        let before = c.rgba().to_vec();
        c.forget();
        c.compose(&fb, &mut images.images(s, e.io()), scale);
        assert!(c.rgba() == before.as_slice());
        // A portrait pixel naming another version (portraits stay at 0 while loaded): its HD
        // block before, its classic colour then.
        let (x, y) = (0..64)
            .flat_map(|v| (0..56).map(move |u| (x0 + u, y0 + v)))
            .find(|&(x, y)| {
                let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
                block(c.rgba(), scale, (x, y)).iter().any(|&g| g != classic)
            })
            .unwrap();
        let mut stale = fb.clone();
        let src = stale.src_at(x, y).unwrap();
        let p = stale.get(x, y).unwrap();
        stale.set_src(x, y, p, u6_render::Src { ver: 1, ..src });
        c.take_dirty();
        c.compose(&stale, &mut images.images(s, e.io()), scale);
        let classic = word(fb.palette[usize::from(p)]);
        assert!(
            block(c.rgba(), scale, (x, y)).iter().all(|&g| g == classic),
            "another version: classic"
        );
        assert_eq!(
            c.take_dirty().map(|d| d[2] * d[3]),
            Some(1),
            "only that block"
        );
    }
}

/// A game for the content keys (spec 07 HD1), every file synthetic: a one-item portrait archive
/// `facefile.a` of `face`, the paper and spellbook bitmaps `paper.bmp` and `book.bmp` (named in a
/// synthetic `game` at A4-0x61e2 and A4-0x61de, as the original names them), and a bitmap colour
/// table that every raw byte goes through (`colours`), so the drawn pixels are not the bytes.
/// With `portrait` the game draws the portrait (letter `a`, item 0) at (0xd8, 0x18) and presents.
fn keyed_game(face: &[u8], paper: (u16, u16, &[u8]), colours: [u8; 256], portrait: bool) -> Engine {
    use crate::converse::text::{ConverseText, MESSAGES, Msg};
    use crate::image::{A4, SyntheticGame};
    let mut lib = 4u32.to_le_bytes().to_vec();
    lib.extend(u6_formats::lzw::encode(face).unwrap());
    let bitmap = |(w, h, px): (u16, u16, &[u8])| {
        let mut raw = w.to_le_bytes().to_vec();
        raw.extend(h.to_le_bytes());
        raw.extend(px);
        u6_formats::lzw::encode(&raw).unwrap()
    };
    let book: Vec<u8> = (0..12).map(|i| (i * 7) as u8).collect();
    let base = (A4 - 0x61e2) & !15;
    let mut data = vec![0u8; 64];
    let at = |a: u32| (a - base) as usize;
    let (name_paper, name_book) = (base + 32, base + 48);
    data[at(A4 - 0x61e2)..][..4].copy_from_slice(&name_paper.to_be_bytes());
    data[at(A4 - 0x61de)..][..4].copy_from_slice(&name_book.to_be_bytes());
    data[at(name_paper)..][..10].copy_from_slice(b"paper.bmp\0");
    data[at(name_book)..][..9].copy_from_slice(b"book.bmp\0");
    let game = SyntheticGame::new()
        .place_at(1, base)
        .loaded(true, data)
        .build();
    let files = Files::new(BTreeMap::from([
        ("game".to_string(), game),
        ("facefile.a".to_string(), lib),
        ("paper.bmp".to_string(), bitmap(paper)),
        ("book.bmp".to_string(), bitmap((4, 3, &book))),
    ]));
    let mut s = State::empty();
    s.converse.text = Some(ConverseText::from_pairs(MESSAGES.iter().map(|&(m, _)| {
        let t: &[u8] = if m == Msg::PortraitFile {
            b"facefile.?"
        } else {
            b""
        };
        (m, t.to_vec())
    })));
    s.gfx.tables.bitmap_colours = colours;
    // Record 7: transparent on a diagonal, with bytes of its own under the transparency.
    install(&mut s.gfx, 7, map_tile(0, 16, pattern(2)));
    let rec = &mut s.gfx.records[7];
    for i in 0..256 {
        rec.transparent[i] = i % 17 == 0;
        if rec.transparent[i] {
            rec.pixels[i] = 9;
        }
    }
    let mut e = Engine::with_game(s, files, move |st, io| async move {
        if portrait {
            crate::converse::draw_portrait(&st.borrow(), &io, 0, 0xd8, 0x18);
            io.present();
        }
        io.sleep(u32::MAX).await
    });
    e.enable_src_plane();
    e.tick(&[], 16);
    e
}

/// SF3: the engine's content keys are those the pack is keyed by (`u6_hdpack`): a tile's of its
/// pixels and transparency (the mask is part of it), a portrait's of the 56 × 64 bytes
/// `portrait_pixels` reads, a bitmap's of the bytes the file holds, both before the colour
/// table (not of the pixels drawn).
#[test]
fn the_engines_content_keys_are_the_packs() {
    use u6_hdpack::{image_hash, tile_hash};
    use u6_render::{BITMAP_PAPER, BITMAP_SPELLBOOK, portrait_id};
    let face: Vec<u8> = (0..56 * 64).map(|i| (i % 97) as u8 + 40).collect();
    let paper: Vec<u8> = (0..10 * 6).map(|i| (i * 3) as u8).collect();
    let colours: [u8; 256] = std::array::from_fn(|i| (i as u8).wrapping_sub(40) | 0x20);
    let e = keyed_game(&face, (10, 6, &paper), colours, false);
    let key = |kind, id| crate::hd::image_key(&e, kind, id);
    let drawn = |raw: &[u8]| {
        raw.iter()
            .map(|&b| colours[usize::from(b)] & 0x1f)
            .collect::<Vec<u8>>()
    };
    // The tile: the mask is in its key.
    let s = e.state();
    let rec = &s.gfx.records[7];
    let tile = key(Kind::Tile, 7).unwrap();
    assert_eq!(Some(tile), tile_hash(&rec.pixels, &rec.transparent));
    assert_ne!(
        tile,
        image_hash(16, 16, rec.pixels.iter().copied()),
        "the mask counts"
    );
    // The portrait: its bytes before the colour table.
    let portrait = key(Kind::Portrait, portrait_id(b'a', 0).unwrap()).unwrap();
    assert_eq!(portrait, image_hash(56, 64, face.iter().copied()));
    assert_ne!(
        portrait,
        image_hash(56, 64, drawn(&face).into_iter()),
        "not the pixels drawn"
    );
    // The paper and the spellbook: their bytes as the files hold them.
    let p = key(Kind::Bitmap, BITMAP_PAPER).unwrap();
    assert_eq!(p, image_hash(10, 6, paper.iter().copied()));
    assert_ne!(p, image_hash(10, 6, drawn(&paper).into_iter()));
    let book: Vec<u8> = (0..12).map(|i| (i * 7) as u8).collect();
    assert_eq!(
        key(Kind::Bitmap, BITMAP_SPELLBOOK),
        Some(image_hash(4, 3, book.iter().copied()))
    );
    // No such image: no key. The archives hold one portrait (the HD tool's and the page's count).
    assert_eq!(key(Kind::Portrait, portrait_id(b'b', 3).unwrap()), None);
    assert_eq!(
        crate::converse::portraits(&s, e.io().files()),
        [(b'a', 0, face.clone())]
    );
    assert_eq!(key(Kind::Classic, 0), None);
}

/// A portrait file name too short to carry the archive letter (the original patches its tenth
/// byte) lists no portraits, as the HD tool before it did, even when a file of that very name
/// holds one.
#[test]
fn a_portrait_file_name_too_short_for_its_letter_lists_no_portraits() {
    use crate::converse::text::{ConverseText, MESSAGES, Msg};
    let mut lib = 4u32.to_le_bytes().to_vec();
    lib.extend(u6_formats::lzw::encode(&vec![7u8; 56 * 64]).unwrap());
    let files = Files::new(BTreeMap::from([("face.?".to_string(), lib)]));
    let mut s = State::empty();
    s.converse.text = Some(ConverseText::from_pairs(MESSAGES.iter().map(|&(m, _)| {
        let t: &[u8] = if m == Msg::PortraitFile {
            b"face.?"
        } else {
            b""
        };
        (m, t.to_vec())
    })));
    assert!(
        crate::converse::portrait_pixels(&s, &files, b'a', 0).is_some(),
        "the file reads"
    );
    assert!(
        crate::converse::portraits(&s, &files).is_empty(),
        "but no portrait is listed"
    );
}

/// A new game whose portrait has other bytes that look the same through its colour table (every
/// byte to one colour) does not show the pack image of the first game's bytes: the view's
/// restart makes the compositor look every image up again ([`Compositor::forget`]), so the
/// portrait's key is asked again.
#[test]
fn after_a_restart_a_portrait_of_other_bytes_that_look_the_same_is_not_the_packs() {
    use std::rc::Rc;
    use u6_hdpack::{Generator, ImageId, PackImage, Rgba, image_hash, read_pack, write_pack};
    let flat: [u8; 256] = [0x20 | 5; 256];
    let (a, b): (Vec<u8>, Vec<u8>) = (
        (0..56 * 64).map(|i| (i % 7) as u8).collect(),
        (0..56 * 64).map(|i| (i % 11) as u8).collect(),
    );
    let mut picture = Rgba::new(224, 256);
    for (i, p) in picture.pixels.chunks_exact_mut(4).enumerate() {
        p.copy_from_slice(&[(i % 224) as u8, (i / 224) as u8, 77, 255]);
    }
    let image = PackImage {
        id: ImageId::Portrait {
            letter: b'a',
            item: 0,
        },
        hash: image_hash(56, 64, a.iter().copied()),
        image: picture,
    };
    let g = Generator {
        name: "test".into(),
        version: "0".into(),
        engine: "none".into(),
        date: "2026-10-16".into(),
    };
    let pack = Rc::new(crate::hd::HdPack::new(
        read_pack(&write_pack(&[image], &g, true).unwrap()).unwrap(),
    ));
    let draw = |face: &[u8]| keyed_game(face, (2, 2, &[1, 2, 3, 4]), flat, true);
    let (first, second) = (draw(&a), draw(&b));
    assert!(
        first.frame().pixels() == second.frame().pixels(),
        "the two games look the same"
    );
    let mut view = HdView::new();
    view.set(true, Scale::X4);
    view.set_pack(Some(pack));
    let blank = Framebuffer::new();
    view.present(Some(&first), &blank);
    let block_at = |view: &HdView| block(view.rgba(), Scale::X4, (0xd8 + 5, 0x18 + 7));
    let classic =
        word(first.frame().palette[usize::from(first.frame().get(0xd8 + 5, 0x18 + 7).unwrap())]);
    assert!(
        block_at(&view).iter().any(|&c| c != classic),
        "the first game: the pack's image"
    );
    view.restart();
    view.present(Some(&second), &blank);
    assert!(
        block_at(&view).iter().all(|&c| c == classic),
        "the second: MMPX of a flat portrait"
    );
}

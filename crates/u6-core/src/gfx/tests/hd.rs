//! Spec 07 HD0, the compositor over the game's routines on synthetic art: the final S× RGBA of
//! a merged shore record, a pixel left from a record rewritten since, overlapping records, an
//! outline flash and an overlay, the pointer sprite, a combat replay, a persistent panel and a
//! skipped render with HD off and on and a scale change, and a portrait read from its archive.
//! Every frame is checked against [`oracle`], the frame by the definition with no cache and no
//! memory of an earlier frame. No game data.

use std::collections::BTreeMap;

use u6_formats::gfx::{AnimOverlay, Span};
use u6_render::hd::{Compositor, Scale, mmpx, word};
use u6_render::{FLAG_OUTLINE, FLAG_OVERLAY, Framebuffer, HEIGHT, Kind, WIDTH};

use super::plane::{kind, plane, tile};
use super::view::world;
use super::{install, map_tile, object_tile};
use crate::Files;
use crate::engine::{Engine, InputEvent, Io};
use crate::gfx::GfxState;
use crate::gfx::anim::{merge_shores, set_frame};
use crate::gfx::blit::{blit_back, blit_front, copy_window};
use crate::hd::ImageCache;
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
            io.draw_back(|b| blit_back(&s.gfx, b, SHORE as u16, 16, 16));
            io.with_buffers(|b, f| copy_window(b, f));
        }
        io.present();
        io.sleep(u32::MAX).await;
    });
    e.enable_src_plane();
    e.tick(&[], 16);
    let s = e.state();
    assert!(s.gfx.cache.version(SHORE) > 0 && s.gfx.records[SHORE] != original.records[SHORE]);
    let cell = || (16..32).flat_map(|y| (16..32).map(move |x| (x, y)));
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
                    ((x - 16) as u16, (y - 16) as u16),
                    0
                ))
            );
            let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
            let now = record_block(&s.gfx, &fb, SHORE as u16, (x - 16, y - 16), scale, classic);
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
                    (x - 16, y - 16),
                    scale,
                    classic,
                )
        });
        assert_eq!(then.count(), 0, "not the bytes it had");
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
            io.draw_back(|b| blit_back(&s.gfx, b, SHORE as u16, 16, 16));
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
        for y in 16..32 {
            for x in 16..32 {
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
                        (x - 16, y - 16),
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
        for row in 0..11 {
            for col in 0..11 {
                blit_front(gfx, f, n, col * 16, row * 16);
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
        let window = (8..168).flat_map(|y| (8..168).map(move |x| (x, y)));
        assert!(
            reshaped(c.rgba(), &fb, Scale::X4, window) > 1000,
            "HD art in the window"
        );
        records.push(fb.src_at(88, 88).unwrap().id);
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

/// A panel drawn once on the screen and a map window whose second refresh draws nothing (spec
/// 06 O2): a kept compositor, run (HD on), then left out over refreshes and presents (HD off),
/// then run again (on), makes the definition's frame each time, and so does a change of scale
/// over the same pixels. The panel stays HD throughout.
#[test]
fn hd_off_and_on_over_a_persistent_panel_and_a_skipped_render_is_exact() {
    let (mut s, mut g) = world();
    for n in 0..64 {
        install(&mut g, n, map_tile(0, 16, pattern(n)));
    }
    s.gfx = g;
    let io = Io::new(Files::default());
    io.enable_src_plane();
    io.draw(|f| {
        for k in 0..8u16 {
            blit_front(&s.gfx, f, 40 + k, 176 + 16 * (k % 4), 8 + 16 * (k / 4));
        }
    });
    let panel = || (8..40).flat_map(|y| (176..240).map(move |x| (x, y)));
    let (mut c, mut images) = (Compositor::new(), ImageCache::new());
    let shown = || {
        let mut fb = io.draw(|f| f.clone());
        palette(&mut fb);
        fb
    };
    let compose = |c: &mut Compositor, images: &mut ImageCache, s: &State, scale, what: &str| {
        let fb = shown();
        check(c, images, &fb, (s, &io), scale, what);
        assert!(
            reshaped(c.rgba(), &fb, scale, panel()) > 50,
            "{what}: the panel is HD"
        );
    };
    crate::gfx::refresh_view(&mut s, &io);
    compose(&mut c, &mut images, &s, Scale::X4, "on");
    // Off: a refresh that draws nothing, text on the panel, a present; nothing composed.
    let skips = s.gfx.cache.window_skips;
    crate::gfx::refresh_view(&mut s, &io);
    assert_eq!(
        s.gfx.cache.window_skips,
        skips + 1,
        "the render was left out"
    );
    io.draw(|f| {
        for x in 180..200 {
            f.set(x, 12, 3);
        }
    });
    io.present();
    crate::gfx::refresh_view(&mut s, &io);
    compose(&mut c, &mut images, &s, Scale::X4, "on again");
    let fb = shown();
    let mut fresh = Compositor::new();
    fresh.compose(&fb, &mut ImageCache::new().images(&s, &io), Scale::X4);
    assert!(fresh.rgba() == c.rgba(), "a new compositor's frame");
    // The scale changes over the same pixels, and back.
    compose(&mut c, &mut images, &s, Scale::X2, "2x");
    compose(&mut c, &mut images, &s, Scale::X4, "4x again");
    assert!(
        (8..168).all(|y| (8..168).all(|x| kind(&fb, x, y) == Kind::Tile)),
        "the window is attributed"
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
    }
}

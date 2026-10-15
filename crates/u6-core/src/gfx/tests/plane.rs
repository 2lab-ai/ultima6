//! Spec 07 HD0: the source plane on synthetic art. What names each pixel (records, overlays,
//! outlines, portraits, panel bitmaps), how the copies, the dissolve, the quake and the presents
//! carry it, what stays classic (text, clock hands, the sparkle, a missile, the pointer sprite),
//! a pixel left from a record rewritten since (stale), the coverage of a scene, and the colours
//! with and without the plane over the adversarial run of the window cache. No game data.

use u6_formats::bitmap::Bitmap;
use u6_formats::font::BitFont;
use u6_formats::gfx::{AnimOverlay, Span};
use u6_render::{
    BITMAP_PAPER, FLAG_OUTLINE, FLAG_OVERLAY, Framebuffer, HEIGHT, Kind, Src, WIDTH, portrait_id,
};

use super::adv::{self, Lists, X};
use super::view::{HERO_BASE, world};
use super::{full_period_taps, install, map_tile, object_tile};
use crate::Files;
use crate::engine::{Engine, InputEvent, Io};
use crate::gfx::GfxState;
use crate::gfx::anim::{merge_shores, set_frame, substitute};
use crate::gfx::blit::{
    blit_back, blit_bitmap, blit_bmp, blit_front, copy_rect, copy_window, copy_window_rows,
    dissolve, dissolve_order,
};
use crate::state::State;

/// A buffer with the plane.
fn plane() -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.enable_src();
    fb
}

/// The source of pixel (u, v) of record `id` at version `ver`.
fn tile(id: u16, ver: u32, (u, v): (u16, u16), flags: u8) -> Src {
    Src {
        ver,
        id,
        u,
        v,
        kind: Kind::Tile,
        flags,
    }
}

fn kind(fb: &Framebuffer, x: usize, y: usize) -> Kind {
    fb.src_at(x, y).expect("a plane, on the screen").kind
}

/// The identity colour table of the bitmap blit.
fn colours() -> [u8; 256] {
    std::array::from_fn(|i| i as u8)
}

/// A 56 × 64 portrait whose pixel (u, v) has colour (u + v) % 31 + 1.
fn portrait() -> Vec<u8> {
    (0..56 * 64)
        .map(|i| ((i % 56 + i / 56) % 31 + 1) as u8)
        .collect()
}

#[test]
fn native_and_test_buffers_have_no_plane() {
    let e = Engine::with_game(State::empty(), Files::default(), |_s, _io| async {});
    assert!(e.frame().src().is_none());
    assert!(e.io().published(|p| p.src().is_none()));
    assert!(e.io().draw(|f| f.src().is_none()));
    assert!(e.io().peek_back(|b| b.src().is_none()));
}

/// A record's pixel names the record (not the tile number a substitution drew it under), the
/// record's version and its (column, row), across the row wrap and for a right-span record; the
/// colours are those without the plane.
#[test]
fn record_pixels_name_their_record_version_and_place() {
    let mut gfx = GfxState::default();
    install(
        &mut gfx,
        5,
        map_tile(2, 3, |r, c| (r * 16 + c) as u8 % 29 + 1),
    );
    install(&mut gfx, 11, object_tile(Span::Right, |_, _| Some(7)));
    substitute(&mut gfx, 9, 5);
    gfx.cache.record_changed(5);
    gfx.cache.record_changed(5);
    let (mut fb, mut plain) = (plane(), Framebuffer::new());
    for f in [&mut fb, &mut plain] {
        f.clear(31);
        blit_back(&gfx, f, 9, 85, 40);
        blit_back(&gfx, f, 11, 64, 0);
        blit_back(&gfx, f, 5, 312, 120);
    }
    assert!(
        fb.pixels() == plain.pixels(),
        "the colours without the plane"
    );
    for r in 2..5u16 {
        for c in 0..16u16 {
            let (x, y) = (80 + usize::from(c), 40 + usize::from(r));
            assert_eq!(fb.src_at(x, y), Some(tile(5, 2, (c, r), 0)));
        }
    }
    assert_eq!(
        kind(&fb, 80, 41),
        Kind::Classic,
        "row 1 is outside the record"
    );
    assert_eq!(fb.src_at(72, 3), Some(tile(11, 0, (8, 3), 0)), "right span");
    assert_eq!(kind(&fb, 71, 3), Kind::Classic, "its left half is clear");
    // Columns 8..16 of the tile at x = 312 continue on the next row at x = 0.
    assert_eq!(fb.src_at(319, 122), Some(tile(5, 2, (7, 2), 0)));
    assert_eq!(fb.src_at(0, 123), Some(tile(5, 2, (8, 2), 0)));
}

/// The overlay OR and the outline change the colour the record has: those pixels are flagged.
#[test]
fn overlay_and_outline_pixels_are_flagged() {
    let mut gfx = GfxState::default();
    let mut t = map_tile(0, 16, |_, _| 2);
    t.masktype = 4;
    t.anim_ref = Some(0);
    install(&mut gfx, 6, t);
    gfx.overlays[6] = Some(AnimOverlay {
        first_row: 3,
        rows: 1,
        frames: (0..8).map(|f| vec![0x8000 >> f]).collect(),
    });
    set_frame(&mut gfx, 2);
    let sprite = |r: usize, c: usize| (r > 0).then_some(if c < 8 { 0 } else { 9 });
    install(&mut gfx, 3, object_tile(Span::Full, sprite));
    install(&mut gfx, 4, object_tile(Span::Right, |_, _| Some(0)));
    let mut fb = plane();
    fb.clear(1);
    blit_back(&gfx, &mut fb, 6, 16, 0);
    assert_eq!(fb.get(18, 3), Some(2 | 8));
    assert_eq!(fb.src_at(18, 3), Some(tile(6, 0, (2, 3), FLAG_OVERLAY)));
    assert_eq!(
        fb.src_at(17, 3),
        Some(tile(6, 0, (1, 3), 0)),
        "no overlay bit"
    );
    blit_back(&gfx, &mut fb, 3 | 0x0800, 0, 32);
    assert_eq!(fb.get(0, 33), Some(11));
    assert_eq!(fb.src_at(0, 33), Some(tile(3, 0, (0, 1), FLAG_OUTLINE)));
    assert_eq!(
        kind(&fb, 8, 33),
        Kind::Classic,
        "a coloured pixel is not drawn"
    );
    // A right-span outline lands 8 pixels left: it names the record pixel it outlines.
    blit_back(&gfx, &mut fb, 4 | 0x1800, 32, 32);
    assert_eq!(fb.src_at(32, 32), Some(tile(4, 0, (8, 0), FLAG_OUTLINE)));
}

/// The back buffer of a window of map records 1..=4 (pixel (r, c) of record n has colour
/// (r·16 + c + n) % 31 + 1) over its 11 × 11 cells.
fn window_back() -> (GfxState, Framebuffer) {
    let mut gfx = GfxState::default();
    for n in 1..=4 {
        install(
            &mut gfx,
            n,
            map_tile(0, 16, move |r, c| ((r * 16 + c + n) % 31 + 1) as u8),
        );
    }
    let mut back = plane();
    for row in 0..11u16 {
        for col in 0..11u16 {
            blit_back(
                &gfx,
                &mut back,
                (row * 11 + col) % 4 + 1,
                col * 16,
                row * 16,
            );
        }
    }
    (gfx, back)
}

/// Pixel `(x, y)` of `fb` has the colour and the source of `(bx, by)` of `back`.
fn same(fb: &Framebuffer, (x, y): (usize, usize), back: &Framebuffer, (bx, by): (usize, usize)) {
    let at = |f: &Framebuffer, x, y| (f.get(x, y), f.src_at(x, y));
    assert_eq!(
        at(fb, x, y),
        at(back, bx, by),
        "({x}, {y}) from ({bx}, {by})"
    );
}

/// The window copy, the row copy of the cached present, a rect copy to an offset, the dissolve
/// and the quake's shifted copy move each pixel's source with its colour.
#[test]
fn the_plane_follows_the_copies_the_dissolve_and_the_quake() {
    let (_, back) = window_back();
    let mut front = plane();
    copy_window(&back, &mut front);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if (8..168).contains(&x) && (8..168).contains(&y) {
                same(&front, (x, y), &back, (x, y));
                assert_eq!(kind(&front, x, y), Kind::Tile);
            } else {
                assert_eq!(kind(&front, x, y), Kind::Classic);
            }
        }
    }
    // The cached present: a row whose colours are the back buffer's but whose pixels were put
    // back classic (a replay) is copied for its sources.
    let mut rows = front.clone();
    for x in 8..168 {
        let p = rows.get(x, 40).unwrap();
        rows.set(x, 40, p);
    }
    assert!(rows.pixels() == front.pixels());
    copy_window_rows(&back, &mut rows);
    assert!(rows.src() == front.src(), "the row's sources are copied");
    // Bytes 2..=5 of rows 16..=31 to (200, 100).
    let mut rect = plane();
    copy_rect(&back, &mut rect, 16, 16, 47, 31, 200, 100);
    for dy in 0..16 {
        for dx in 0..32 {
            same(&rect, (200 + dx, 100 + dy), &back, (16 + dx, 16 + dy));
        }
    }
    assert_eq!(kind(&rect, 232, 100), Kind::Classic);
    // The dissolve over the window, in its LFSR order.
    let taps = vec![0xb400u16; 15];
    let mut dis = plane();
    dissolve(&taps, &back, &mut dis, (8, 8), (0xa7, 0xa7));
    let order = dissolve_order(160, 160, &taps);
    assert!(order.len() > 1000);
    for &(dx, dy) in &order {
        let p = (8 + usize::from(dx), 8 + usize::from(dy));
        same(&dis, p, &back, p);
    }
    let raster = dissolve_order(160, 160, &full_period_taps());
    let mut all = plane();
    dissolve(&full_period_taps(), &back, &mut all, (8, 8), (0xa7, 0xa7));
    assert_eq!(raster.len(), 160 * 160);
    assert!(
        all.src() == front.src(),
        "a dissolve of every pixel is the window copy"
    );
    // One shake of the quake (ui/effects.rs: the window rect from (dx, dy) = 16 ± amp copied to
    // (16, 16)): x 13 is byte 1, so pixel 8 + k lands at 16 + k.
    let (dx, dy) = (13i16, 19i16);
    let mut quake = plane();
    crate::ui::seams::copy_rect(&back, &mut quake, dx, dy, dx + 0x90, dy + 0x90, 0x10, 0x10);
    for r in 0..=0x90 {
        for k in 0..152 {
            same(&quake, (16 + k, 16 + r), &back, (8 + k, 19 + r));
            assert_eq!(kind(&quake, 16 + k, 16 + r), Kind::Tile);
        }
    }
}

/// A tile drawn on the screen once stays attributed through later presents that draw elsewhere
/// (the front buffer persists), in the published frame and the shown one.
#[test]
fn a_front_pixel_keeps_its_source_across_presents() {
    let mut s = State::empty();
    install(
        &mut s.gfx,
        7,
        map_tile(0, 16, |r, c| ((r + c) % 31 + 1) as u8),
    );
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        io.draw(|f| blit_front(&st.borrow().gfx, f, 7, 200, 100));
        io.present();
        for i in 0..4u8 {
            io.vblank().await;
            io.draw(|f| f.set(10 + usize::from(i), 190, 3));
            io.present();
        }
    });
    e.enable_src_plane();
    for _ in 0..6 {
        e.tick(&[], 16);
    }
    assert_eq!(
        e.frame().get(13, 190),
        Some(3),
        "the later presents were shown"
    );
    assert_eq!(kind(e.frame(), 13, 190), Kind::Classic);
    assert_eq!(e.frame().src_at(203, 104), Some(tile(7, 0, (3, 4), 0)));
    assert!(e.io().published(|p| p.src_at(203, 104)) == Some(tile(7, 0, (3, 4), 0)));
}

/// Text drawn over a portrait is classic in the glyph's cell; the rest of the portrait keeps
/// its (letter, item) and (u, v).
#[test]
fn text_over_a_portrait_is_classic() {
    let id = portrait_id(b'a', 3).unwrap();
    let mut fb = plane();
    let pixels = portrait();
    blit_bitmap(
        &colours(),
        &mut fb,
        (56, 64),
        &pixels,
        0xd8,
        0x18,
        (Kind::Portrait, id),
    );
    let font = BitFont {
        glyphs: vec![[0xAA; 8]; 256],
    };
    crate::ui::text::draw_glyph(&mut fb, &font, 0xe0, 0x20, b'A', 1, 2);
    for v in 0..64u16 {
        for u in 0..56u16 {
            let (x, y) = (0xd8 + usize::from(u), 0x18 + usize::from(v));
            let glyph = (0xe0..0xe8).contains(&x) && (0x20..0x28).contains(&y);
            if glyph {
                assert_eq!(kind(&fb, x, y), Kind::Classic);
            } else {
                let want = Src {
                    ver: 0,
                    id,
                    u,
                    v,
                    kind: Kind::Portrait,
                    flags: 0,
                };
                assert_eq!(fb.src_at(x, y), Some(want));
            }
        }
    }
}

/// The world of the view tests in `s.gfx`, with the clock's hands on the hero's tile and its
/// tables.
fn clock_world() -> State {
    let (mut s, mut g) = world();
    g.tables.clock_colour = 6;
    g.tables.dir_dx = [0, 1, 1, 1, 0, -1, -1, -1];
    g.tables.dir_dy = [-1, -1, 0, 1, 1, 1, 0, -1];
    g.tables.hour_dir = [0, 1, 1, 2, 3, 3, 4, 5, 5, 6, 7, 7];
    s.tables.basetile.base[0x9f] = HERO_BASE;
    s.actors.calendar.hour = 4;
    s.actors.calendar.minute = 30;
    s.gfx = g;
    s
}

/// The clock hands and the sparkle plot pixels: classic; every other pixel of the window's back
/// buffer names a record.
#[test]
fn clock_hands_and_the_sparkle_are_classic() {
    let mut s = clock_world();
    let io = Io::new(Files::default());
    io.enable_src_plane();
    crate::gfx::render_window(&mut s, &io, false);
    let hands = crate::gfx::view::clock_points(&s.actors.calendar, &s.gfx.tables, 80, 80);
    io.peek_back(|b| {
        for &(x, y) in &hands {
            let (x, y) = (usize::from(x), usize::from(y));
            assert_eq!((b.get(x, y), kind(b, x, y)), (Some(6), Kind::Classic));
        }
        let classic = (0..176)
            .flat_map(|y| (0..176).map(move |x| (x, y)))
            .filter(|&(x, y)| kind(b, x, y) == Kind::Classic)
            .count();
        assert_eq!(classic, 3, "only the hands");
    });
    let mut s = clock_world();
    s.tables.basetile.base[0x9f] = 0;
    s.gfx.sparkle = 1;
    let mut dice = s.rng;
    let io = Io::new(Files::default());
    io.enable_src_plane();
    crate::gfx::render_window(&mut s, &io, false);
    let plotted: std::collections::BTreeSet<(usize, usize)> = (0..100)
        .map(|_| {
            let x = dice.range(8, 0xa7);
            let y = dice.range(8, 0xa7);
            (usize::from(x), usize::from(y))
        })
        .collect();
    assert!(dice == s.rng, "the sparkle's draws");
    io.peek_back(|b| {
        let classic: std::collections::BTreeSet<(usize, usize)> = (0..176)
            .flat_map(|y| (0..176).map(move |x| (x, y)))
            .filter(|&(x, y)| kind(b, x, y) == Kind::Classic)
            .collect();
        assert_eq!(classic, plotted, "the sparkle's pixels, and only they");
    });
}

/// A missile's sprite and the block it puts back are pixel writes: classic.
#[test]
fn a_missile_is_classic() {
    let (mut gfx, mut back) = window_back();
    gfx.tables.sine = std::array::from_fn(|n| {
        (64.0 * (n as f64 * std::f64::consts::TAU / 128.0).sin()).round() as i8
    });
    install(
        &mut gfx,
        7,
        object_tile(Span::Full, |r, c| {
            (c < 15).then_some(((r * 16 + c) % 31 + 1) as u8)
        }),
    );
    crate::gfx::sprite::prepare(&mut gfx, 2, 7, 0, 128, 128);
    crate::gfx::sprite::draw(&gfx, &mut back, 2, 40, 40);
    for r in 0..16 {
        for c in 0..16 {
            let want = if c < 15 { Kind::Classic } else { Kind::Tile };
            assert_eq!(kind(&back, 32 + c, 32 + r), want, "({c}, {r})");
        }
    }
    // The flight's next step puts the saved block back (the save marks the slot).
    let (_, mut under) = window_back();
    crate::gfx::sprite::save(&mut gfx, &under, 2, 40, 40);
    crate::gfx::sprite::draw(&gfx, &mut under, 2, 40, 40);
    assert_eq!(kind(&under, 40, 40), Kind::Classic, "the block put back");
}

/// The pointer sprite is drawn over the published frame into the shown one: classic there, the
/// tile under it still named in the published frame.
#[test]
fn the_pointer_sprite_is_classic() {
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
    install(&mut s.gfx, 7, map_tile(0, 16, |_, _| 5));
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        io.draw(|f| blit_front(&st.borrow().gfx, f, 7, 200, 100));
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
    let (x, y) = (204, 102);
    assert_eq!(e.frame().get(x, y), Some(17), "the sprite is shown there");
    assert_eq!(kind(e.frame(), x, y), Kind::Classic);
    assert!(e.io().published(|p| p.src_at(x, y)) == Some(tile(7, 0, (4, 2), 0)));
    assert_eq!(kind(e.frame(), 204, 110), Kind::Tile, "below the sprite");
}

/// Tile 21's record drawn into the back buffer, then the shore merge rewrites it, then the
/// window is copied and presented: the shown pixel names version 0 while the record is at 5
/// (one rewrite per plane), and its colour is one the record no longer holds.
#[test]
fn a_pixel_left_while_its_record_is_rewritten_is_stale() {
    const WATER: usize = 20;
    const SHORE: usize = 21;
    let mut s = State::empty();
    install(&mut s.gfx, WATER, map_tile(0, 16, |_, _| 0b10101));
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
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
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
    assert_eq!(
        s.gfx.cache.version(SHORE),
        5,
        "the merge rewrote all five planes"
    );
    assert_eq!(s.gfx.records[SHORE].pixels[4 * 16 + 3], 0b10101);
    assert_eq!(
        e.frame().src_at(19, 20),
        Some(tile(SHORE as u16, 0, (3, 4), 0))
    );
    assert_eq!(
        e.frame().get(19, 20),
        Some(0b01010),
        "the bytes it was drawn from"
    );
}

/// Each attributed, unflagged pixel of `fb` against its image: the record's pixel at (u, v),
/// the portrait's or the paper's through the colour table. Returns the pixels checked by kind
/// (tile, portrait, bitmap).
fn check_against_images(
    fb: &Framebuffer,
    gfx: &GfxState,
    portrait: &[u8],
    paper: &Bitmap,
) -> [usize; 3] {
    let mut n = [0; 3];
    for (i, src) in fb.src().unwrap().iter().enumerate() {
        if src.flags != 0 {
            continue;
        }
        let (u, v) = (usize::from(src.u), usize::from(src.v));
        let want = match src.kind {
            Kind::Classic => continue,
            Kind::Tile => {
                n[0] += 1;
                gfx.records[usize::from(src.id)].pixels[v * 16 + u]
            }
            Kind::Portrait => {
                n[1] += 1;
                portrait[v * 56 + u]
            }
            Kind::Bitmap => {
                n[2] += 1;
                paper.pixels[v * usize::from(paper.width) + u]
            }
        };
        assert_eq!(fb.pixels()[i], want, "pixel {i} against its image");
    }
    n
}

/// A synthetic screen of the paper panel, the map window and a portrait: every pixel of each
/// is attributed to its image, none is classic, and each equals its image's pixel at (u, v).
#[test]
fn a_scene_of_tiles_a_portrait_and_a_panel_is_attributed() {
    let (mut s, g) = world();
    s.gfx = g;
    let io = Io::new(Files::default());
    io.enable_src_plane();
    let paper = Bitmap {
        width: 320,
        height: 200,
        pixels: (0..WIDTH * HEIGHT).map(|i| (i % 7 + 1) as u8).collect(),
    };
    let face = portrait();
    let id = portrait_id(b'z', 0).unwrap();
    io.draw(|f| blit_bmp(&colours(), f, &paper, 0, 0, BITMAP_PAPER));
    crate::gfx::render_window(&mut s, &io, true);
    io.draw(|f| {
        blit_bitmap(
            &colours(),
            f,
            (56, 64),
            &face,
            0xd8,
            0x18,
            (Kind::Portrait, id),
        )
    });
    io.draw(|f| {
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let want = if (8..168).contains(&x) && (8..168).contains(&y) {
                    Kind::Tile
                } else if (0xd8..0xd8 + 56).contains(&x) && (0x18..0x18 + 64).contains(&y) {
                    Kind::Portrait
                } else {
                    Kind::Bitmap
                };
                assert_eq!(kind(f, x, y), want, "({x}, {y})");
            }
        }
        let n = check_against_images(f, &s.gfx, &face, &paper);
        assert_eq!(
            n,
            [160 * 160, 56 * 64, WIDTH * HEIGHT - 160 * 160 - 56 * 64]
        );
    });
}

/// A plane enabled while the window cache remembers a render (the page starts the plane before
/// the first step, but any later enable must be as good): the next refresh draws the whole
/// window again, so the back buffer's window is attributed, not left blank.
#[test]
fn enabling_the_plane_redraws_what_the_window_cache_remembers() {
    let (mut s, g) = world();
    s.gfx = g;
    let io = Io::new(Files::default());
    crate::gfx::refresh_view(&mut s, &io);
    crate::gfx::refresh_view(&mut s, &io);
    assert_eq!(
        s.gfx.cache.window_skips, 1,
        "the second refresh drew nothing"
    );
    io.enable_src_plane();
    crate::gfx::refresh_view(&mut s, &io);
    assert_eq!(
        s.gfx.cache.window_skips, 1,
        "the refresh after the enable drew"
    );
    io.peek_back(|b| {
        let tiles = (0..176)
            .flat_map(|y| (0..176).map(move |x| (x, y)))
            .filter(|&(x, y)| kind(b, x, y) == Kind::Tile)
            .count();
        assert_eq!(tiles, 176 * 176);
    });
    assert!(io.draw(|f| (8..168).all(|y| (8..168).all(|x| kind(f, x, y) == Kind::Tile))));
}

/// The three runs of [`plane_parity`]: the plane off with the window cache, on with it, on
/// without it.
fn three_ios() -> [Io; 3] {
    let ios = [0, 1, 2].map(|_| Io::new(Files::default()));
    ios[1].enable_src_plane();
    ios[2].enable_src_plane();
    ios[2].set_window_cache(false);
    ios
}

/// `steps` refreshes of the adversarial edits of seed `seed` on the three runs, compared after
/// each; returns the attributed pixels the screen of the cached plane run showed.
fn plane_parity(seed: u64, steps: u32) -> usize {
    let (s0, words) = adv::scene();
    let mut states = [s0.clone(), s0.clone(), s0];
    let mut ios = three_ios();
    let mut rng = X(seed | 1);
    let mut lists = [0, 1, 2].map(|_| Lists {
        base: vec![0; 121],
        lists: vec![Vec::new(); 121],
    });
    let mut attributed = 0;
    for i in 0..steps {
        let start = rng.clone();
        for k in 0..3 {
            let mut x = start.clone();
            adv::edit(&mut states[k], &ios[k], &mut x, &mut lists[k], &words, i);
            rng = x;
        }
        if rng.pm(5) {
            // A clone of the State takes over (an empty cache, versions from 0).
            states = states.map(|s| s.clone());
        }
        if rng.pm(3) {
            ios = three_ios();
        }
        if rng.pm(5) {
            for (s, io) in states.iter().zip(&ios) {
                let mut other = s.clone();
                other.objects.draw.base[60] ^= 1;
                crate::gfx::refresh_view(&mut other, io);
            }
        }
        for (s, io) in states.iter_mut().zip(&ios) {
            crate::gfx::refresh_view(s, io);
        }
        let screen = |io: &Io| io.draw(|f| (f.pixels().to_vec(), f.palette));
        let back = |io: &Io| io.peek_back(|b| (b.pixels().to_vec(), b.palette));
        let planes = |io: &Io| {
            let front = io.draw(|f| f.src().map(<[Src]>::to_vec));
            (front, io.peek_back(|b| b.src().map(<[Src]>::to_vec)))
        };
        let (s0, b0) = (screen(&ios[0]), back(&ios[0]));
        for (k, io) in ios.iter().enumerate().skip(1) {
            assert!(
                screen(io) == s0 && back(io) == b0,
                "seed {seed} step {i}: run {k}'s colours differ from the run without the plane"
            );
        }
        assert!(planes(&ios[0]) == (None, None));
        assert!(
            planes(&ios[1]) == planes(&ios[2]),
            "seed {seed} step {i}: the cached plane differs from the every-time render's"
        );
        assert!(
            states[0] == states[1] && states[1] == states[2],
            "seed {seed} step {i}: the State"
        );
        attributed += ios[1].draw(|f| {
            f.src()
                .unwrap()
                .iter()
                .filter(|s| s.kind == Kind::Tile)
                .count()
        });
    }
    attributed
}

/// Spec 07 Acceptance 1: the plane changes no colour, no palette and no State over the
/// adversarial edits of the window cache, and the cache leaves it as the full render writes it.
#[test]
fn the_plane_changes_no_frame_and_follows_the_window_cache() {
    let mut attributed = 0;
    for seed in 0..2 {
        attributed += plane_parity(0x51_7CC1_B727_220A ^ (seed * 0x1000_0001), 400);
    }
    assert!(attributed > 0, "the plane was written");
}

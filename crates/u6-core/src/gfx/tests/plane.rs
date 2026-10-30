//! Spec 07 HD0: the source plane on synthetic art. What names each pixel (records, overlays,
//! outlines, portraits, panel bitmaps, glyphs), how the copies, the dissolve, the quake, a text
//! scroll, a flight's put-back block, a combat playback and the presents carry it, what stays
//! classic (clock hands, the sparkle, a missile, the pointer sprite), a pixel left from a record
//! rewritten since (stale, also in a clone), and a scene whose every pixel is checked against its
//! image. The parity runs are in `plane_parity`. No game data.
//!
//! The map window lies at the map's corner and its sizes come from [`crate::geom`] (spec 13:
//! [`at_map`], [`shown`], [`view_rect`]); the UI's paper, panels and portraits stay on the UI's
//! 320 × 200 ([`UI`]). Tests over [`world`] go through the map's window and the draw lists (C1,
//! C2).

use u6_formats::bitmap::Bitmap;
use u6_formats::font::BitFont;
use u6_formats::gfx::{AnimOverlay, Span};
use u6_render::{
    BITMAP_PAPER, BITMAP_SPELLBOOK, FLAG_INK, FLAG_OUTLINE, FLAG_OVERLAY, FLAG_PAPER,
    FLAG_PAPER_OFF, Framebuffer, HEIGHT, Kind, Src, WIDTH,
};

use super::view::{HERO_BASE, world};
use super::{
    at_map, at_map16, full_period_taps, in_shown, install, map_tile, object_tile, shown, view_rect,
};
use crate::Files;
use crate::engine::{Engine, InputEvent, Io};
use crate::geom::{HALF_H, HALF_W, SHOWN, VIEW_H, VIEW_W};
use crate::gfx::GfxState;
use crate::gfx::anim::{merge_shores, set_frame, substitute};
use crate::gfx::blit::{
    BitmapSource, MapDissolve, UI, blit_back, blit_bitmap, blit_bmp, blit_front, copy_rect,
    copy_window, copy_window_rows, dissolve_map,
};
use crate::state::State;

/// A buffer with the plane.
pub(super) fn plane() -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.enable_src();
    fb
}

/// The source of pixel (u, v) of record `id` at version `ver`.
pub(super) fn tile(id: u16, ver: u32, (u, v): (u16, u16), flags: u8) -> Src {
    Src {
        ver,
        id,
        u,
        v,
        kind: Kind::Tile,
        flags,
    }
}

/// The source of pixel (u, v) of portrait or bitmap `id` (version 0, no flags).
fn picture(kind: Kind, id: u16, (u, v): (usize, usize)) -> Src {
    Src {
        ver: 0,
        id,
        u: u as u16,
        v: v as u16,
        kind,
        flags: 0,
    }
}

/// [`glyph`] drawn over the paper ([`FLAG_PAPER`]).
fn glyph_on_paper(code: u8, row: u8, at: (usize, usize)) -> Src {
    let g = glyph(code, row, at);
    Src {
        flags: g.flags | FLAG_PAPER,
        ..g
    }
}

/// The source of pixel (u, v) of the glyph of `code` whose row bytes are all `row` (spec 11).
fn glyph(code: u8, row: u8, (u, v): (usize, usize)) -> Src {
    Src {
        id: u16::from(code),
        kind: Kind::Glyph,
        flags: if row & (0x80 >> u) != 0 { FLAG_INK } else { 0 },
        ..picture(Kind::Glyph, 0, (u, v))
    }
}

pub(super) fn kind(fb: &Framebuffer, x: usize, y: usize) -> Kind {
    fb.src_at(x, y).expect("a plane, on the screen").kind
}

/// The identity colour table of the bitmap blit.
fn colours() -> [u8; 256] {
    std::array::from_fn(|i| i as u8)
}

/// A `w` × `h` synthetic image of colours 1..=31 hashed from (`seed`, index): no shift of it
/// matches it (not periodic), and images of other seeds differ.
fn image(seed: u32, w: usize, h: usize) -> Vec<u8> {
    (0..w * h)
        .map(|i| {
            let mut x = (i as u32 ^ seed.wrapping_mul(0x85EB_CA6B)).wrapping_mul(0x9E37_79B1);
            x ^= x >> 15;
            x = x.wrapping_mul(0x2C1B_3C6D);
            x ^= x >> 12;
            (x % 31 + 1) as u8
        })
        .collect()
}

/// A 56 × 64 portrait ([`image`] of `seed`).
fn portrait(seed: u32) -> Vec<u8> {
    image(seed, 56, 64)
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
    // Columns 8..16 of the tile at x = 312 continue on the next row at x = 0 on the UI's own
    // screen (Original); in Wide the map lies past x = 319 and they are dropped (spec 13
    // Design 4).
    assert_eq!(fb.src_at(319, 122), Some(tile(5, 2, (7, 2), 0)));
    if (WIDTH, HEIGHT) == UI {
        assert_eq!(fb.src_at(0, 123), Some(tile(5, 2, (8, 2), 0)));
    } else {
        assert_eq!(kind(&fb, 0, 123), Kind::Classic);
        assert_eq!(kind(&fb, 320, 122), Kind::Classic, "not into the map");
    }
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
/// (r·16 + c + n) % 31 + 1, and record n is at version n) over its view cells (11 × 11 in
/// Original) from the map's corner.
fn window_back() -> (GfxState, Framebuffer) {
    let mut gfx = GfxState::default();
    for n in 1..=4 {
        install(
            &mut gfx,
            n,
            map_tile(0, 16, move |r, c| ((r * 16 + c + n) % 31 + 1) as u8),
        );
        for _ in 0..n {
            gfx.cache.record_changed(n);
        }
    }
    let mut back = plane();
    for row in 0..VIEW_H {
        for col in 0..VIEW_W {
            let n = ((row * VIEW_W + col) % 4 + 1) as u16;
            let (x, y) = at_map16(col * 16, row * 16);
            blit_back(&gfx, &mut back, n, x, y);
        }
    }
    (gfx, back)
}

/// The record [`window_back`] draws in the cell of frame pixel (x, y).
fn window_record(x: usize, y: usize) -> u16 {
    let (x0, y0) = at_map(0, 0);
    (((y - y0) / 16 * VIEW_W + (x - x0) / 16) % 4 + 1) as u16
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
    let (_, mut back) = window_back();
    let mut front = plane();
    copy_window(&back, &mut front);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if in_shown(x, y) {
                same(&front, (x, y), &back, (x, y));
                let n = window_record(x, y);
                let uv = ((x % 16) as u16, (y % 16) as u16);
                assert_eq!(front.src_at(x, y), Some(tile(n, u32::from(n), uv, 0)));
            } else {
                assert_eq!(kind(&front, x, y), Kind::Classic);
            }
        }
    }
    // The cached present: the window rect synced, then a row of it put back classic in its own
    // colours (a replay); the next present copies that row's sources again.
    let mut rows = plane();
    copy_window_rows(&mut back, &mut rows);
    assert!(rows.src() == front.src() && rows.pixels() == front.pixels());
    let (xs, _) = shown();
    let row = at_map(0, 40).1;
    for x in xs {
        let p = rows.get(x, row).unwrap();
        rows.set(x, row, p);
    }
    assert!(rows.pixels() == front.pixels() && rows.src() != front.src());
    copy_window_rows(&mut back, &mut rows);
    assert!(rows.src() == front.src(), "the row's sources are copied");
    // Bytes 2..=5 of the window's rows 16..=31 to (200, 100).
    let mut rect = plane();
    let (sx, sy) = at_map16(16, 16);
    copy_rect(&back, &mut rect, sx, sy, sx + 31, sy + 15, 200, 100);
    let (sx, sy) = (usize::from(sx), usize::from(sy));
    for dy in 0..16 {
        for dx in 0..32 {
            same(&rect, (200 + dx, 100 + dy), &back, (sx + dx, sy + dy));
        }
    }
    assert_eq!(kind(&rect, 232, 100), Kind::Classic);
    // The dissolve over the window, in its LFSR order (spec 13: the steps over the shown map).
    let taps = vec![0xb400u16; 15];
    let mut dis = plane();
    dissolve_map(&taps, &back, &mut dis);
    let order = MapDissolve::new(&taps);
    assert!(order.steps() > 1000);
    for k in 0..order.steps() {
        for (x, y) in order.step(k) {
            let p = (usize::from(x), usize::from(y));
            same(&dis, p, &back, p);
        }
    }
    let raster = MapDissolve::new(&full_period_taps());
    let mut all = plane();
    dissolve_map(&full_period_taps(), &back, &mut all);
    assert_eq!(raster.steps(), 160 * 160);
    assert!(
        all.src() == front.src(),
        "a dissolve of every pixel is the window copy"
    );
    // One shake of the quake (ui/effects.rs: the window rect from (dx, dy) = 16 ± amp copied to
    // (16, 16), from the map's corner): x 13 is byte 1, so pixel 8 + k lands at 16 + k.
    let (dx, dy) = at_map16(13, 19);
    let (to_x, to_y) = at_map16(0x10, 0x10);
    let (dx, dy, to_x, to_y) = (dx as i16, dy as i16, to_x as i16, to_y as i16);
    let mut quake = plane();
    crate::ui::seams::copy_rect(&back, &mut quake, dx, dy, dx + 0x90, dy + 0x90, to_x, to_y);
    let (q, b) = (at_map(16, 16), at_map(8, 19));
    for r in 0..=0x90 {
        for k in 0..152 {
            same(&quake, (q.0 + k, q.1 + r), &back, (b.0 + k, b.1 + r));
            assert_eq!(kind(&quake, q.0 + k, q.1 + r), Kind::Tile);
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

/// The published frame copies only what changed of the screen's plane (spec 07 HD0 cost): after
/// a present of a changed screen, after an idle tick that changed nothing and presented again,
/// and after a present of a single new pixel, the published plane is the screen's, and the shown
/// frame's is the published one's but for the pointer sprite.
#[test]
fn a_present_after_an_idle_tick_leaves_the_published_plane_the_screens() {
    let mut s = State::empty();
    install(
        &mut s.gfx,
        7,
        map_tile(0, 16, |r, c| ((r + c) % 31 + 1) as u8),
    );
    s.ui.pointer =
        crate::ui::pointer::PointerState::from_tables(crate::ui::pointer::PointerTables {
            shapes: vec![crate::ui::pointer::Shape {
                dx: 0,
                dy: 0,
                rows: vec![[1; 16]; 2],
            }],
            shown: true,
            ..Default::default()
        });
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        io.draw(|f| {
            for x in (0..320).step_by(16) {
                blit_front(&st.borrow().gfx, f, 7, x, 100);
            }
        });
        io.present();
        io.vblank().await;
        // An idle tick: nothing drawn, nothing presented.
        io.vblank().await;
        // A present with nothing changed since the last one.
        io.present();
        io.vblank().await;
        io.draw(|f| f.set(3, 104, 9));
        io.present();
        io.sleep(u32::MAX).await;
    });
    e.enable_src_plane();
    let at = |x: i16| InputEvent::Pointer {
        x,
        y: 105,
        buttons: 0,
    };
    for (n, input) in [vec![at(40)], vec![], vec![at(90)], vec![], vec![]]
        .into_iter()
        .enumerate()
    {
        e.tick(&input, 16);
        let front = e.io().draw(|f| f.src().unwrap().to_vec());
        assert!(
            e.io().published(|p| p.src() == Some(&front[..])),
            "tick {n}: published"
        );
        let (published, shown) = (e.io().published(|p| p.src().unwrap().to_vec()), e.frame());
        for (i, (p, s)) in published.iter().zip(shown.src().unwrap()).enumerate() {
            let (x, y) = (i % WIDTH, i / WIDTH);
            let sprite = (105..107).contains(&y) && shown.get(x, y) == Some(17);
            assert!(
                p == s || (sprite && s.kind == Kind::Classic),
                "tick {n}: shown ({x}, {y})"
            );
        }
    }
    assert_eq!(
        e.frame().src_at(3, 104).map(|s| s.kind),
        Some(Kind::Classic)
    );
    assert_eq!(e.frame().src_at(4, 104), Some(tile(7, 0, (4, 4), 0)));
    assert_eq!(
        e.frame().src_at(41, 105),
        Some(tile(7, 0, (9, 5), 0)),
        "the sprite moved off"
    );
    assert_eq!(
        e.frame().src_at(91, 105).map(|s| s.kind),
        Some(Kind::Classic),
        "and on"
    );
}

/// Text drawn over a portrait names its glyph in the glyph's cell (spec 11: the code, (u, v) in
/// the cell, the ink flag on set pixels); the rest of the portrait keeps its (letter, item) and
/// (u, v).
#[test]
fn text_over_a_portrait_names_its_glyph() {
    // b'a' << 8 | 3: portrait.a, item 3.
    let id = 0x6103;
    let mut fb = plane();
    let pixels = portrait(3);
    let src = BitmapSource::Portrait(id);
    blit_bitmap(&colours(), &mut fb, (56, 64), &pixels, 0xd8, 0x18, src);
    let font = BitFont {
        glyphs: vec![[0xAA; 8]; 256],
    };
    crate::ui::text::draw_glyph(&mut fb, &font, 0xe0, 0x20, b'A', 1, 2);
    for v in 0..64 {
        for u in 0..56 {
            let (x, y) = (0xd8 + u, 0x18 + v);
            let cell = (0xe0..0xe8).contains(&x) && (0x20..0x28).contains(&y);
            if cell {
                let at = (x - 0xe0, y - 0x20);
                assert_eq!(fb.src_at(x, y), Some(glyph(b'A', 0xAA, at)));
            } else {
                assert_eq!(fb.src_at(x, y), Some(picture(Kind::Portrait, id, (u, v))));
            }
        }
    }
}

/// A crafted paper (identity colour table): all of colour 2, the test windows' paper colour,
/// but two pixels of colour 7 under the window [`text_window`]: (300, 183) in its last pixel
/// row, which its scroll moves up to (300, 175), and (290, 175), where the pixel of colour 2 from
/// (290, 183) arrives. A pixel written onto the paper of its own colour names the paper there;
/// these two branches are the others. The UI's 320 × 200, as the game's paper ([`UI`]).
fn crafted_paper() -> Bitmap {
    let (w, h) = UI;
    let mut pixels = vec![2u8; w * h];
    pixels[183 * w + 300] = 7;
    pixels[175 * w + 290] = 7;
    Bitmap {
        width: w as u16,
        height: h as u16,
        pixels,
    }
}

/// The text window of the scroll tests: columns 22..=37, rows 13..=22 (x 176..304, y 104..184),
/// ink 1, paper 2.
fn text_window() -> crate::ui::text::TextWindow {
    crate::ui::text::TextWindow {
        x: 22,
        y: 13,
        width: 16,
        height: 10,
        col: 0,
        row: 0,
        fg: 1,
        bg: 2,
        flags: 0,
    }
}

/// The source of a pixel of `colour` that the scroll or the clear writes at (x, y) over `paper`
/// (drawn over the whole screen): the paper's there when that is its colour, else the paper's
/// flagged [`FLAG_PAPER_OFF`].
fn over_paper(paper: &Bitmap, colour: u8, (x, y): (usize, usize)) -> Src {
    let named = picture(Kind::Bitmap, BITMAP_PAPER, (x, y));
    if paper.pixels[y * usize::from(paper.width) + x] == colour {
        named
    } else {
        Src {
            flags: FLAG_PAPER_OFF,
            ..named
        }
    }
}

/// A text window scrolled over the paper: a glyph moved up stays that glyph (drawn over the
/// paper); a paper pixel moved up names the paper at the place it moves to, flagged
/// [`FLAG_PAPER_OFF`] where its colour is not the paper's there; every colour moves up; the row
/// the scroll clears names the paper, flagged where the paper is not of the window's paper
/// colour; nothing outside the window changes (and in Wide nothing is drawn past the UI's
/// screen). Checked on a copy synced from the screen before and after the scroll (as the
/// published frame is), against sources and colours computed from the layout: a write that
/// forgot its mark would leave the copy as it was.
#[test]
fn a_text_scroll_moves_the_glyphs_over_the_paper() {
    let mut fb = plane();
    let paper = crafted_paper();
    blit_bmp(&colours(), &mut fb, &paper, 0, 0, BITMAP_PAPER);
    let kept = crate::ui::text::Paper::drawn(&colours(), &paper);
    let font = BitFont {
        glyphs: vec![[0x81; 8]; 256],
    };
    // A glyph in the window's second row: ink in its first and last columns, paper colour 2
    // between.
    crate::ui::text::draw_glyph(&mut fb, &font, 192, 112, b'A', 1, 2);
    let w = text_window();
    let mut copy = plane();
    copy.sync_from(&mut fb);
    crate::ui::text::scroll_window(&mut fb, &w, Some(&kept));
    copy.sync_from(&mut fb);
    // What the screen showed at (x, y) before the scroll.
    let was = |x: usize, y: usize| {
        if (192..200).contains(&x) && (112..120).contains(&y) {
            let ink = if x == 192 || x == 199 { 1 } else { 2 };
            (ink, glyph_on_paper(b'A', 0x81, (x - 192, y - 112)))
        } else {
            (paper.pixels[y * UI.0 + x], picture(Kind::Bitmap, 0, (x, y)))
        }
    };
    // A pixel the scroll moved to (x, y): the paper's named in place (flagged on another colour),
    // any other as it was.
    let in_place = |(colour, src): (u8, Src), x, y| match src {
        Src {
            kind: Kind::Bitmap,
            id: BITMAP_PAPER,
            ..
        } => (colour, over_paper(&paper, colour, (x, y))),
        _ => (colour, src),
    };
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let on_ui = x < UI.0 && y < UI.1;
            let inside = (176..304).contains(&x) && (104..184).contains(&y);
            let want = match (on_ui, inside, y < 176) {
                (false, _, _) => (0, Src::CLASSIC),
                (true, false, _) => was(x, y),
                (true, true, true) => in_place(was(x, y + 8), x, y),
                (true, true, false) => (2, over_paper(&paper, 2, (x, y))),
            };
            assert_eq!(
                (copy.get(x, y), copy.src_at(x, y)),
                (Some(want.0), Some(want.1)),
                "({x}, {y})"
            );
        }
    }
    assert!(copy.src() == fb.src() && copy.pixels() == fb.pixels());
    let at = |x, y| (copy.get(x, y), copy.src_at(x, y));
    assert_eq!(
        at(180, 104),
        (Some(2), Some(picture(Kind::Bitmap, 0, (180, 104)))),
        "the paper's colour moved up, its source stayed"
    );
    let off = |x, y| Src {
        flags: FLAG_PAPER_OFF,
        ..picture(Kind::Bitmap, 0, (x, y))
    };
    assert_eq!(
        at(300, 175),
        (Some(7), Some(off(300, 175))),
        "a pixel of the paper's other colour moved onto its plain part is flagged"
    );
    assert_eq!(
        at(290, 175),
        (Some(2), Some(off(290, 175))),
        "a pixel of the plain part moved onto the other colour is flagged"
    );
    assert_eq!(
        at(301, 183),
        (Some(2), Some(picture(Kind::Bitmap, 0, (301, 183)))),
        "the cleared row names the paper of its colour"
    );
    assert_eq!(
        at(300, 183),
        (Some(2), Some(off(300, 183))),
        "the cleared row is flagged over the paper's other colour"
    );
    assert_eq!(
        at(199, 111),
        (Some(1), Some(glyph_on_paper(b'A', 0x81, (7, 7)))),
        "the glyph moved up"
    );
}

/// The paper is the background and stays where it is drawn when the text scrolls over it: a paper
/// pixel the scroll moves up takes the paper's source of the place it moves to ((u, v) = its new
/// (x, y), as the paper is drawn at (0, 0)), so the HD compositor shows the paper's own place
/// there, not the place the pixel came from; a pixel of another colour than the paper's there
/// names it flagged [`FLAG_PAPER_OFF`] (shown classic). Every other source moves with its pixel as
/// before: a panel bitmap, a portrait, a tile and a glyph under the text. The colours move as they
/// did, equal to the same scroll on a buffer without a plane. Without the paper's colours (no
/// `paper.bmp`) every pixel keeps its source and the cleared row is classic, as before.
#[test]
fn a_text_scroll_leaves_the_paper_in_place() {
    let paper = crafted_paper();
    let kept = crate::ui::text::Paper::drawn(&colours(), &paper);
    let book = image(7, 8, 8);
    let face = image(11, 8, 8);
    let face_id = u6_render::portrait_id(b'a', 3).expect("an item below 256");
    let font = BitFont {
        glyphs: vec![[0x81; 8]; 256],
    };
    let draw = |fb: &mut Framebuffer| {
        blit_bmp(&colours(), fb, &paper, 0, 0, BITMAP_PAPER);
        // In the window's third row: a spellbook block at (200, 120), a portrait block at
        // (216, 120), a row of tile pixels at (232..248, 124) and a glyph at (256, 120).
        let book_src = BitmapSource::Bitmap(BITMAP_SPELLBOOK);
        blit_bitmap(&colours(), fb, (8, 8), &book, 200, 120, book_src);
        let face_src = BitmapSource::Portrait(face_id);
        blit_bitmap(&colours(), fb, (8, 8), &face, 216, 120, face_src);
        for u in 0..16 {
            fb.set_src(232 + u, 124, 9, tile(0x123, 2, (u as u16, 5), 0));
        }
        crate::ui::text::draw_glyph(fb, &font, 256, 120, b'B', 1, 2);
    };
    let w = text_window();
    let mut fb = plane();
    draw(&mut fb);
    let before = fb.clone();
    let mut without = fb.clone();
    crate::ui::text::scroll_window(&mut fb, &w, Some(&kept));
    crate::ui::text::scroll_window(&mut without, &w, None);
    let mut bare = Framebuffer::new();
    draw(&mut bare);
    crate::ui::text::scroll_window(&mut bare, &w, Some(&kept));
    assert!(
        fb.pixels() == bare.pixels() && without.pixels() == bare.pixels(),
        "the colours are the plane-less scroll's"
    );
    let src_before = |x: usize, y: usize| before.src_at(x, y).expect("on the screen");
    for y in 104..184 {
        for x in 176..304 {
            let colour = before.get(x, y + 8).filter(|_| y < 176).unwrap_or(2);
            let from = (y < 176).then(|| src_before(x, y + 8));
            let want = match from {
                Some(s) if s.kind == Kind::Bitmap && s.id == BITMAP_PAPER => {
                    over_paper(&paper, colour, (x, y))
                }
                Some(s) => s,
                None => over_paper(&paper, 2, (x, y)),
            };
            assert_eq!(
                (fb.get(x, y), fb.src_at(x, y)),
                (Some(colour), Some(want)),
                "({x}, {y})"
            );
            let old = from.unwrap_or(Src::CLASSIC);
            assert_eq!(without.src_at(x, y), Some(old), "({x}, {y}) without");
        }
    }
    // The paper pixel moved from (180, 112) to (180, 104) names the paper at (180, 104); the
    // others name what they named one text row lower.
    let at = |x, y| fb.src_at(x, y).expect("on the screen");
    assert_eq!(
        at(180, 104),
        picture(Kind::Bitmap, BITMAP_PAPER, (180, 104))
    );
    let off = |x, y| Src {
        flags: FLAG_PAPER_OFF,
        ..picture(Kind::Bitmap, BITMAP_PAPER, (x, y))
    };
    assert_eq!((fb.get(300, 175), at(300, 175)), (Some(7), off(300, 175)));
    assert_eq!((fb.get(290, 175), at(290, 175)), (Some(2), off(290, 175)));
    assert_eq!(
        at(203, 114),
        picture(Kind::Bitmap, BITMAP_SPELLBOOK, (3, 2))
    );
    assert_eq!(at(221, 119), picture(Kind::Portrait, face_id, (5, 7)));
    assert_eq!(at(240, 116), tile(0x123, 2, (8, 5), 0));
    assert_eq!(at(263, 113), glyph_on_paper(b'B', 0x81, (7, 1)));
}

/// A fill of the UI over the paper ([`crate::ui::text::fill_rect`], the panels' clears and the
/// command underline): each pixel filled names the paper, flagged [`FLAG_PAPER_OFF`] where the
/// paper's colour there is not the colour filled; with the paper's colour 7 filled over its pixel
/// of colour 7 only that pixel names it unflagged; without the paper's colours, or without a
/// plane, the fill is classic as before. The colours are the fill's either way. A colour past 15
/// writes nothing (`plot_in`), so it names nothing either: pixels of the paper's colour drawn
/// there classic before stay classic.
#[test]
fn a_fill_over_the_paper_names_it_where_it_is_its_colour() {
    let paper = crafted_paper();
    let kept = crate::ui::text::Paper::drawn(&colours(), &paper);
    let rect = (280, 170, 303, 183);
    let inside = |x: usize, y: usize| (280..=303).contains(&x) && (170..=183).contains(&y);
    for (colour, given) in [(2, true), (7, true), (2, false)] {
        let mut fb = plane();
        blit_bmp(&colours(), &mut fb, &paper, 0, 0, BITMAP_PAPER);
        let mut bare = Framebuffer::new();
        blit_bmp(&colours(), &mut bare, &paper, 0, 0, BITMAP_PAPER);
        let (x0, y0, x1, y1) = rect;
        let map = given.then_some(&kept);
        crate::ui::text::fill_rect(&mut fb, x1, y1, x0, y0, colour, map);
        crate::ui::text::fill_rect(&mut bare, x0, y0, x1, y1, colour, map);
        assert!(fb.pixels() == bare.pixels(), "the fill's colours");
        for y in 160..190 {
            for x in 270..310 {
                let want = match (inside(x, y), given) {
                    (false, _) => picture(Kind::Bitmap, BITMAP_PAPER, (x, y)),
                    (true, true) => over_paper(&paper, colour, (x, y)),
                    (true, false) => Src::CLASSIC,
                };
                assert_eq!(
                    fb.src_at(x, y),
                    Some(want),
                    "colour {colour}, {given}: ({x}, {y})"
                );
            }
        }
    }
    let mut fb = plane();
    blit_bmp(&colours(), &mut fb, &paper, 0, 0, BITMAP_PAPER);
    for (x, y) in (170..=183).flat_map(|y| (280..=303).map(move |x| (x, y))) {
        fb.set(x, y, 2);
    }
    let before = fb.clone();
    crate::ui::text::fill_rect(&mut fb, 280, 170, 303, 183, 0x12, Some(&kept));
    assert!(
        fb.pixels() == before.pixels(),
        "a colour past 15: nothing filled"
    );
    assert!(fb.src() == before.src(), "a colour past 15: nothing named");
}

/// A paper pixel the scroll moves keeps its place's name as it goes, flagged
/// [`FLAG_PAPER_OFF`] over the paper of another colour and unflagged again where it meets the
/// paper of its own: on a paper of colour 2 with colour 7 at (300, 183), (290, 175) and
/// (290, 167), the cleared row's pixel at (300, 183) (colour 2) is flagged, and names the paper
/// one text row up at the next scroll; the pixel of colour 2 that leaves (290, 183) crosses two
/// places of colour 7, flagged at each, and names the paper at the third scroll.
#[test]
fn a_moved_paper_pixel_is_named_again_where_it_meets_its_colour() {
    let mut pixels = vec![2u8; UI.0 * UI.1];
    for (x, y) in [(300, 183), (290, 175), (290, 167)] {
        pixels[y * UI.0 + x] = 7;
    }
    let paper = Bitmap {
        width: UI.0 as u16,
        height: UI.1 as u16,
        pixels,
    };
    let kept = crate::ui::text::Paper::drawn(&colours(), &paper);
    let mut fb = plane();
    blit_bmp(&colours(), &mut fb, &paper, 0, 0, BITMAP_PAPER);
    let w = text_window();
    let named = |x, y| picture(Kind::Bitmap, BITMAP_PAPER, (x, y));
    let off = |x, y| Src {
        flags: FLAG_PAPER_OFF,
        ..named(x, y)
    };
    let at = |fb: &Framebuffer, x, y| (fb.get(x, y).unwrap(), fb.src_at(x, y).unwrap());
    crate::ui::text::scroll_window(&mut fb, &w, Some(&kept));
    assert_eq!(
        at(&fb, 300, 183),
        (2, off(300, 183)),
        "cleared over colour 7"
    );
    assert_eq!(at(&fb, 290, 175), (2, off(290, 175)), "moved onto colour 7");
    crate::ui::text::scroll_window(&mut fb, &w, Some(&kept));
    assert_eq!(at(&fb, 300, 175), (2, named(300, 175)), "one row up: named");
    assert_eq!(at(&fb, 290, 167), (2, off(290, 167)), "onto colour 7 again");
    crate::ui::text::scroll_window(&mut fb, &w, Some(&kept));
    assert_eq!(
        at(&fb, 290, 159),
        (2, named(290, 159)),
        "back on colour 2: named"
    );
    assert_eq!(at(&fb, 300, 167), (2, named(300, 167)), "named as it goes");
}

/// A glyph names the paper under it ([`FLAG_PAPER`]) where the pixel it replaces is on the paper
/// ([`Src::on_paper`]): over the paper, over the paper flagged [`FLAG_PAPER_OFF`] and over a
/// glyph drawn over the paper; not over the spellbook, a portrait or a classic pixel. The flag
/// moves with the glyph in a scroll.
#[test]
fn a_glyph_carries_the_paper_only_where_it_is_drawn_over_it() {
    let paper = crafted_paper();
    let kept = crate::ui::text::Paper::drawn(&colours(), &paper);
    let mut fb = plane();
    blit_bmp(&colours(), &mut fb, &paper, 0, 0, BITMAP_PAPER);
    let book = BitmapSource::Bitmap(BITMAP_SPELLBOOK);
    blit_bitmap(&colours(), &mut fb, (8, 8), &[2; 64], 200, 120, book);
    let face_id = u6_render::portrait_id(b'a', 3).expect("an item below 256");
    let face = BitmapSource::Portrait(face_id);
    blit_bitmap(&colours(), &mut fb, (8, 8), &[2; 64], 216, 120, face);
    for (x, y) in (120..128).flat_map(|y| (232..240).map(move |x| (x, y))) {
        fb.set(x, y, 2);
    }
    // A fill of colour 3 over the paper at (248..256, 120..128): the paper flagged.
    crate::ui::text::fill_rect(&mut fb, 248, 120, 255, 127, 3, Some(&kept));
    let font = BitFont {
        glyphs: vec![[0x81; 8]; 256],
    };
    for x in [184, 200, 216, 232, 248] {
        crate::ui::text::draw_glyph(&mut fb, &font, x, 120, b'A', 1, 2);
    }
    // Again over the first, a glyph drawn over the paper.
    crate::ui::text::draw_glyph(&mut fb, &font, 184, 120, b'B', 1, 2);
    let cell = |fb: &Framebuffer, x0: usize, y0: usize| -> Vec<Src> {
        (0..64)
            .map(|k| fb.src_at(x0 + k % 8, y0 + k / 8).unwrap())
            .collect()
    };
    let over =
        |fb: &Framebuffer, x0, y0| cell(fb, x0, y0).iter().all(|s| s.flags & FLAG_PAPER != 0);
    let not = |fb: &Framebuffer, x0, y0| cell(fb, x0, y0).iter().all(|s| s.flags & FLAG_PAPER == 0);
    assert!(over(&fb, 184, 120), "over a glyph over the paper");
    assert!(not(&fb, 200, 120), "over the spellbook");
    assert!(not(&fb, 216, 120), "over a portrait");
    assert!(not(&fb, 232, 120), "over classic pixels");
    assert!(over(&fb, 248, 120), "over the paper flagged");
    crate::ui::text::scroll_window(&mut fb, &text_window(), Some(&kept));
    assert!(over(&fb, 184, 112), "moved up with the scroll");
    assert!(
        not(&fb, 200, 112),
        "moved up with the scroll, not over the paper"
    );
    let s = fb.src_at(191, 119).unwrap();
    assert_eq!(s, glyph_on_paper(b'B', 0x81, (7, 7)), "the glyph whole");
}

/// The key wait's symbol ([`crate::ui::text::draw_symbol`]) over the paper names it by the rule
/// of the fills where the paper shows through: its clear pixels of the paper's colour name the
/// paper; its ink pixels, which that rule flags [`FLAG_PAPER_OFF`] (shown classic), name it a
/// symbol (spec 11 Design 2 rev 3: [`Kind::Glyph`] of its code, its place in the cell,
/// [`FLAG_INK`], [`FLAG_PAPER`], [`FLAG_SYMBOL`]; shown classic too), so the HD font mode lists
/// the cell as a symbol, with its rows, for the platform to draw; over the spellbook, or without
/// the paper's colours, its pixels are classic as before and nothing is listed. Never a
/// character of the HD font. One of spec 11 Design 2's receipts.
#[test]
fn the_key_wait_symbol_names_the_paper_under_it() {
    use u6_render::hd::{Compositor, HdGlyph, NoImages, SYMBOL, Scale};
    use u6_render::{FLAG_SYMBOL, Rgb};
    let paper = crafted_paper();
    let kept = crate::ui::text::Paper::drawn(&colours(), &paper);
    let font = BitFont {
        glyphs: vec![[0x81; 8]; 256],
    };
    let w = text_window();
    let mut fb = plane();
    for (i, c) in fb.palette.iter_mut().enumerate() {
        *c = Rgb::new(i as u8, 255 - i as u8, 7);
    }
    blit_bmp(&colours(), &mut fb, &paper, 0, 0, BITMAP_PAPER);
    let book = BitmapSource::Bitmap(BITMAP_SPELLBOOK);
    blit_bitmap(&colours(), &mut fb, (8, 8), &[2; 64], 200, 120, book);
    let fonts = (Some(&font), None);
    let mut bare = fb.clone();
    crate::ui::text::draw_symbol(&mut fb, fonts, &w, 5, (23, 15), Some(&kept));
    crate::ui::text::draw_symbol(&mut fb, fonts, &w, 5, (25, 15), Some(&kept));
    crate::ui::text::draw_symbol(&mut bare, fonts, &w, 5, (23, 15), None);
    for k in 0..64 {
        let (u, v) = (k % 8, k / 8);
        let ink = u == 0 || u == 7;
        let (x, y) = (184 + u, 120 + v);
        let want = if ink {
            Src {
                id: 5,
                flags: FLAG_INK | FLAG_PAPER | FLAG_SYMBOL,
                ..picture(Kind::Glyph, 0, (u, v))
            }
        } else {
            picture(Kind::Bitmap, BITMAP_PAPER, (x, y))
        };
        let colour = if ink { 1 } else { 2 };
        assert_eq!(
            (fb.get(x, y), fb.src_at(x, y)),
            (Some(colour), Some(want)),
            "({x}, {y})"
        );
        assert_eq!(
            fb.src_at(200 + u, y),
            Some(Src::CLASSIC),
            "over the spellbook"
        );
        assert_eq!(
            bare.src_at(x, y),
            Some(Src::CLASSIC),
            "without the paper's colours"
        );
    }
    // Glyph diagnostics: the symbol over the paper listed, its rows given; the one over the
    // spellbook and the one without the paper's colours not.
    let listed = |fb: &Framebuffer| {
        let mut c = Compositor::new();

        c.compose(fb, &mut NoImages, Scale::X2);
        (c.glyphs().to_vec(), c.symbol_rows().to_vec())
    };
    let symbol = HdGlyph {
        x: 184,
        y: 120,
        code: SYMBOL + 5,
        ink: fb.palette[1],
        paper: fb.palette[2],
        halftone: false,
    };
    assert_eq!(
        listed(&fb),
        (vec![symbol], vec![[0x81; 8]]),
        "over the paper: a symbol"
    );
    assert_eq!(
        listed(&bare),
        (vec![], vec![]),
        "without the paper's colours"
    );
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
    // The hero's cell, the leader's ((80, 80) in Original).
    let (hx, hy) = at_map16(HALF_W * 16, HALF_H * 16);
    let hands = crate::gfx::view::clock_points(&s.actors.calendar, &s.gfx.tables, hx, hy);
    let window = || {
        let (xs, ys) = view_rect();
        ys.flat_map(move |y| xs.clone().map(move |x| (x, y)))
    };
    io.peek_back(|b| {
        for &(x, y) in &hands {
            let (x, y) = (usize::from(x), usize::from(y));
            assert_eq!((b.get(x, y), kind(b, x, y)), (Some(6), Kind::Classic));
        }
        let classic = window()
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
    let (x0, y0, x1, y1) = SHOWN;
    let plotted: std::collections::BTreeSet<(usize, usize)> = (0..100)
        .map(|_| {
            let x = dice.range(x0, x1);
            let y = dice.range(y0, y1);
            (usize::from(x), usize::from(y))
        })
        .collect();
    assert!(dice == s.rng, "the sparkle's draws");
    io.peek_back(|b| {
        let classic: std::collections::BTreeSet<(usize, usize)> = window()
            .filter(|&(x, y)| kind(b, x, y) == Kind::Classic)
            .collect();
        assert_eq!(classic, plotted, "the sparkle's pixels, and only they");
    });
}

/// A flight: the block under the sprite saved (slot 3), the sprite drawn (slot 2: its pixels
/// classic), the block put back: every pixel of the block has its colour and its source again,
/// those the sprite covered and those it did not (its clear last column, the block's third byte).
/// Checked in the back buffer and on the screen synced from it over the window rect (as the
/// present does) before the flight, with the sprite up and after the put-back, against sources
/// and colours computed from the layout: a write that forgot its mark would leave the screen as
/// it was.
#[test]
fn a_missile_is_classic_and_the_block_it_put_back_is_named_again() {
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
    // Places from the map's corner: the sprite at (40, 40), its block x 32..56, y 32..48.
    let (ox, oy) = at_map(0, 0);
    let (sx, sy) = at_map16(40, 40);
    let block = || (oy + 32..oy + 48).flat_map(move |y| (ox + 32..ox + 56).map(move |x| (x, y)));
    // The window as `window_back` draws it, and with the sprite over it.
    let ground = |x: usize, y: usize| {
        let n = window_record(x, y);
        let (r, c) = (y % 16, x % 16);
        let colour = ((r * 16 + c + usize::from(n)) % 31 + 1) as u8;
        (colour, tile(n, u32::from(n), (c as u16, r as u16), 0))
    };
    let flying = |x: usize, y: usize| match (x.checked_sub(ox + 32), y.checked_sub(oy + 32)) {
        (Some(c), Some(r)) if c < 15 && r < 16 => (((r * 16 + c) % 31 + 1) as u8, Src::CLASSIC),
        _ => ground(x, y),
    };
    let mut screen = plane();
    let window = |screen: &Framebuffer, want: &dyn Fn(usize, usize) -> (u8, Src)| {
        let (xs, ys) = shown();
        for y in ys {
            for x in xs.clone() {
                let (colour, src) = want(x, y);
                let got = (screen.get(x, y), screen.src_at(x, y));
                assert_eq!(got, (Some(colour), Some(src)), "the screen at ({x}, {y})");
            }
        }
    };
    let (xs, ys) = shown();
    screen.sync_rect_from(&mut back, xs.clone(), ys.clone());
    window(&screen, &ground);
    let before = back.clone();
    crate::gfx::sprite::save(&mut gfx, &back, 3, sx, sy);
    crate::gfx::sprite::draw(&gfx, &mut back, 2, sx, sy);
    screen.sync_rect_from(&mut back, xs.clone(), ys.clone());
    window(&screen, &flying);
    // The block is bytes 4..=6 of rows 32..48 (x 32..56); the sprite covers x 32..47.
    for (x, y) in block() {
        if x < ox + 47 {
            assert_eq!(kind(&back, x, y), Kind::Classic, "the sprite at ({x}, {y})");
        } else {
            same(&back, (x, y), &before, (x, y));
        }
    }
    crate::gfx::sprite::draw(&gfx, &mut back, 3, sx, sy);
    screen.sync_rect_from(&mut back, xs, ys);
    window(&screen, &ground);
    for (x, y) in block() {
        same(&back, (x, y), &before, (x, y));
        let n = window_record(x, y);
        let uv = ((x % 16) as u16, (y % 16) as u16);
        assert_eq!(back.src_at(x, y), Some(tile(n, u32::from(n), uv, 0)));
    }
    // Outside the sprite's opaque footprint: its clear column and the block's third byte.
    let (x, y) = at_map(47, 33);
    let n = window_record(x, y);
    assert_eq!(back.src_at(x, y), Some(tile(n, u32::from(n), (15, 1), 0)));
    assert_eq!(
        back.get(x, y),
        Some(((16 + 15 + n as usize) % 31 + 1) as u8)
    );
    let (x, y) = at_map(53, 46);
    let n = window_record(x, y);
    assert_eq!(back.src_at(x, y), Some(tile(n, u32::from(n), (5, 14), 0)));
    // Saved from a buffer without a plane, the block is put back classic, in its colours.
    let mut plain = Framebuffer::new();
    for (x, y) in block() {
        plain.set(x, y, before.get(x, y).unwrap());
    }
    crate::gfx::sprite::save(&mut gfx, &plain, 3, sx, sy);
    assert!(
        gfx.sprites.under_src[3].0.is_none(),
        "no plane: no sources saved"
    );
    crate::gfx::sprite::draw(&gfx, &mut back, 3, sx, sy);
    let (x, y) = at_map(40, 40);
    assert_eq!(
        (back.get(x, y), kind(&back, x, y)),
        (before.get(x, y), Kind::Classic)
    );
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

/// Combat playback over the plane: the screen shows record 1, then the synchronous code presents
/// records 2 (recorded), 3 (recorded dissolving) and 4 (its last picture). The playback puts the
/// published window back, shows the recorded frames (the dissolve revealed over its time) and
/// ends on the last picture: on every frame shown, each window pixel names the record its colour
/// came from, with its (u, v) and version.
#[test]
fn combat_playback_puts_back_the_sources_it_recorded() {
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    for n in 1..=4 {
        install(
            &mut s.gfx,
            n,
            map_tile(0, 16, move |r, c| ((r * 16 + c + 7 * n) % 31 + 1) as u8),
        );
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
    let mut shown = Vec::new();
    let mut version = 0;
    for _ in 0..200 {
        e.tick(&[], 16);
        if e.frame_version() == version {
            continue;
        }
        version = e.frame_version();
        let fb = e.frame();
        let gfx = &e.state().gfx;
        let (mut all, mut inner) = (Vec::new(), Vec::new());
        let (xs, ys) = view_rect();
        for y in ys {
            for x in xs.clone() {
                let src = fb.src_at(x, y).unwrap();
                let n = src.id;
                let want = tile(n, u32::from(n), ((x % 16) as u16, (y % 16) as u16), 0);
                assert!((1..=4).contains(&n) && src == want, "({x}, {y}): {src:?}");
                let colour = gfx.records[usize::from(n)].pixels[y % 16 * 16 + x % 16];
                assert_eq!(fb.get(x, y), Some(colour), "({x}, {y})");
                all.push(n);
                if in_shown(x, y) {
                    inner.push(n);
                }
            }
        }
        let set = |mut v: Vec<u16>| {
            v.sort_unstable();
            v.dedup();
            v
        };
        shown.push((set(all), set(inner)));
    }
    // The dissolve reveals the window's inside over its time (its border keeps the frame before
    // until the whole recorded rect is put up).
    assert!(
        shown.iter().any(|(all, _)| all == &[2, 3]),
        "the reveal: {shown:?}"
    );
    assert!(
        shown.iter().any(|(_, inner)| inner == &[3]),
        "revealed: {shown:?}"
    );
    let mut whole: Vec<u16> = shown
        .iter()
        .filter(|(a, _)| a.len() == 1)
        .map(|(a, _)| a[0])
        .collect();
    whole.dedup();
    assert_eq!(
        whole,
        [1, 2, 3, 4],
        "the published window, the two frames, the last picture"
    );
    assert!(e.state().combat.effects.is_empty(), "all played");
}

/// A playback of [`equal_colours`].
#[derive(Clone, Copy)]
struct Playback {
    plane: bool,
    /// A pause is recorded after the frame.
    pause: bool,
    /// The record of the recorded frame in window cell (col, row) (1 is the last picture's).
    shot: fn(u16, u16) -> u16,
    /// The pointer at (x, y) from the map's corner ([`at_map`]): a shape of 16 rows whose opaque
    /// columns are the bits of each word (bit 15 the first column).
    pointer: Option<(i16, i16, [u16; 16])>,
}

/// The pointer's frame pixel: (x, y) of [`Playback::pointer`] from the map's corner. In Wide the
/// engine holds the pointer to the UI's 320 × 200 (`engine.rs` `Pointer::apply`), off the map.
fn pointer_at(x: i16, y: i16) -> (i16, i16) {
    let (mx, my) = at_map(0, 0);
    (x + mx as i16, y + my as i16)
}

/// The frame pixels the pointer of `p` covers.
fn pointer_covers(p: &Playback, x: usize, y: usize) -> bool {
    p.pointer.is_some_and(|(px, py, rows)| {
        let (px, py) = pointer_at(px, py);
        let (c, r) = (x as i32 - i32::from(px), y as i32 - i32::from(py));
        (0..16).contains(&c) && (0..16).contains(&r) && rows[r as usize] & (0x8000 >> c) != 0
    })
}

/// Records 1 and 5 have the same pixels. The screen shows record 1, the synchronous code presents
/// the recorded frame ([`Playback::shot`], recorded), then record 1 again (its last picture); then
/// the playback. Returns the engine after it and what each tick left ([`Tick`]).
fn equal_colours(p: Playback) -> (Engine, Vec<Tick>) {
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    let same = |r: usize, c: usize| ((r * 16 + c) % 31 + 1) as u8;
    install(&mut s.gfx, 1, map_tile(0, 16, same));
    install(&mut s.gfx, 5, map_tile(0, 16, same));
    if let Some((_, _, rows)) = p.pointer {
        let rows = rows
            .iter()
            .map(|&w| std::array::from_fn(|c| u8::from(w & (0x8000 >> c) != 0)))
            .collect();
        let shape = crate::ui::pointer::Shape { dx: 0, dy: 0, rows };
        s.ui.pointer =
            crate::ui::pointer::PointerState::from_tables(crate::ui::pointer::PointerTables {
                shapes: vec![shape],
                shown: true,
                ..Default::default()
            });
    }
    let fill = |gfx: &GfxState, f: &mut Framebuffer, n: &dyn Fn(u16, u16) -> u16| {
        for row in 0..VIEW_H {
            for col in 0..VIEW_W {
                let (x, y) = at_map16(col * 16, row * 16);
                blit_front(gfx, f, n(col as u16, row as u16), x, y);
            }
        }
    };
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        io.draw(|f| fill(&st.borrow().gfx, f, &|_, _| 1));
        io.present();
        io.vblank().await;
        {
            let mut s = st.borrow_mut();
            io.draw(|f| fill(&s.gfx, f, &p.shot));
            io.present();
            crate::combat::present::capture_window(&mut s, 40);
            if p.pause {
                crate::combat::present::capture_pause(&mut s, 60);
            }
            io.draw(|f| fill(&s.gfx, f, &|_, _| 1));
            io.present();
        }
        crate::combat::play_effects(&st, &io).await;
        io.sleep(u32::MAX).await;
    });
    if p.plane {
        e.enable_src_plane();
    }
    let versions = (0..40)
        .map(|t| {
            let at = p.pointer.filter(|_| t == 0).map(|(x, y, _)| {
                let (x, y) = pointer_at(x, y);
                InputEvent::Pointer { x, y, buttons: 0 }
            });
            e.tick(at.as_slice(), 16);
            let of = |f: &Framebuffer| (f.pixels().to_vec(), f.palette);
            Tick {
                frame: e.frame_version(),
                src: e.src_version(),
                buffers: [
                    e.io().peek_back(of),
                    e.io().draw(|f| of(f)),
                    e.io().published(of),
                    of(e.frame()),
                ],
            }
        })
        .collect();
    (e, versions)
}

/// What a tick of [`equal_colours`] left: the frame and source versions, and the colours and
/// palette of the back buffer, the screen, the published frame and the shown one.
struct Tick {
    frame: u64,
    src: u64,
    buffers: [(Vec<u8>, [u6_render::Rgb; 256]); 4],
}

/// [`equal_colours`] with the plane off and on: after every tick the frame versions, and the
/// colours and palettes of the four buffers (back, screen, published, shown), are the same; the
/// run without the plane has no source version. Then checks that the shown frame names record 1
/// off the pointer and nothing under it, and that the published frame and the screen name record
/// 1. Returns the last tick's source version.
fn equal_colours_pair(p: Playback, what: &str) -> u64 {
    let (off, plain) = equal_colours(Playback { plane: false, ..p });
    let (on, ticks) = equal_colours(Playback { plane: true, ..p });
    assert_eq!(ticks.len(), plain.len());
    let names = ["back buffer", "screen", "published frame", "shown frame"];
    for (t, (a, b)) in plain.iter().zip(&ticks).enumerate() {
        assert_eq!(a.frame, b.frame, "{what}: tick {t}: the frame version");
        assert_eq!(a.src, 0, "{what}: tick {t}: no plane, no source version");
        for (k, name) in names.iter().enumerate() {
            let ((pa, la), (pb, lb)) = (&a.buffers[k], &b.buffers[k]);
            assert!(pa == pb, "{what}: tick {t}: the {name}'s colours");
            assert!(la == lb, "{what}: tick {t}: the {name}'s palette");
        }
    }
    assert!(on.state().combat.effects.is_empty(), "{what}: all played");
    assert!(
        off.frame().pixels() == on.frame().pixels(),
        "{what}: the colours"
    );
    let at = |x: usize, y: usize| tile(1, 0, ((x % 16) as u16, (y % 16) as u16), 0);
    let (xs, ys) = view_rect();
    let window = ys.flat_map(move |y| xs.clone().map(move |x| (x, y)));
    for (x, y) in window.clone() {
        let want = if pointer_covers(&p, x, y) {
            Src::CLASSIC
        } else {
            at(x, y)
        };
        assert_eq!(
            on.frame().src_at(x, y),
            Some(want),
            "{what}: shown at ({x}, {y})"
        );
    }
    let named = |fb: &Framebuffer| {
        window
            .clone()
            .all(|(x, y)| fb.src_at(x, y) == Some(at(x, y)))
    };
    assert!(on.io().published(named), "{what}: published");
    assert!(on.io().draw(|f| named(f)), "{what}: the screen");
    on.src_version()
}

/// A playback whose last picture has the colours of the frame put up last (records 1 and 5 have
/// the same pixels): the last picture's sources go back all the same, in the published and the
/// shown frame, with no new frame: right after the frame (its present publishes them; no source
/// version), and after a pause (published with the plane alone; the source version moves once).
/// The frame versions are those of the run without the plane.
#[test]
fn a_playback_ending_on_equal_colours_puts_back_the_last_sources() {
    for pause in [false, true] {
        let p = Playback {
            plane: true,
            pause,
            shot: |_, _| 5,
            pointer: None,
        };
        let what = format!("pause {pause}");
        assert_eq!(equal_colours_pair(p, &what), u64::from(pause), "{what}");
    }
}

/// The same after a pause with the pointer standing over the window: two opaque columns of four
/// rows at (36, 40), inside the chunks x 32..48 of rows 40..44, whose other pixels change their
/// sources. The pixels beside the pointer get the last picture's sources (a source-only publish
/// that took only chunks of equal colours would leave those chunks behind for good), the
/// pointer's own stay classic, and the source version moves once.
#[test]
fn a_stationary_pointer_does_not_hold_back_the_sources_beside_it() {
    let mut rows = [0u16; 16];
    rows[..4].fill(0xC000);
    let p = Playback {
        plane: true,
        pause: true,
        shot: |_, _| 5,
        pointer: Some((36, 40, rows)),
    };
    assert_eq!(equal_colours_pair(p, "beside the pointer"), 1);
}

/// The source version moves only when the shown frame's sources changed: not when the change
/// lies wholly under the pointer (record 5 recorded in cell (2, 2) only, a solid pointer over
/// that cell: the published frame changes, the shown one does not), and not when the last
/// picture's sources are those the frame already has.
#[test]
fn the_source_version_moves_only_when_the_shown_sources_change() {
    let hidden = Playback {
        plane: true,
        pause: true,
        shot: |col, row| if (col, row) == (2, 2) { 5 } else { 1 },
        pointer: Some((32, 32, [0xFFFF; 16])),
    };
    assert_eq!(equal_colours_pair(hidden, "under the pointer"), 0);
    let same = Playback {
        shot: |_, _| 1,
        pointer: None,
        ..hidden
    };
    assert_eq!(equal_colours_pair(same, "the same sources"), 0);
}

/// The water record and the shore record it merges into ([`shore_state`]).
const WATER: usize = 20;
const SHORE: usize = 21;

/// A state whose shore merge 0 takes every pixel of [`WATER`] into [`SHORE`].
fn shore_state() -> State {
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
    s
}

/// The shore record drawn into the back buffer, then the shore merge rewrites it, then the
/// window is copied and presented: the shown pixel names version 0 while the record is at 5
/// (one rewrite per plane), and its colour is one the record no longer holds.
#[test]
fn a_pixel_left_while_its_record_is_rewritten_is_stale() {
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
    assert_eq!(
        s.gfx.cache.version(SHORE),
        5,
        "the merge rewrote all five planes"
    );
    assert_eq!(s.gfx.records[SHORE].pixels[4 * 16 + 3], 0b10101);
    let (x, y) = at_map(19, 20);
    assert_eq!(
        e.frame().src_at(x, y),
        Some(tile(SHORE as u16, 0, (3, 4), 0))
    );
    assert_eq!(
        e.frame().get(x, y),
        Some(0b01010),
        "the bytes it was drawn from"
    );
}

/// Spec 07 HD0 with `RenderCache::clone`: a state cloned mid-run keeps its records' versions, so
/// a pixel drawn before the clone is current for the clone as for the original, and a merge in
/// the clone after it makes that pixel stale there only.
#[test]
fn a_clone_keeps_the_record_versions() {
    let s = shore_state();
    let mut g = s.gfx.clone();
    merge_shores(&mut g);
    let ver = g.cache.version(SHORE);
    assert_eq!(ver, 5, "one rewrite per colour plane");
    let mut back = plane();
    blit_back(&g, &mut back, SHORE as u16, 16, 16);
    let drawn = back.src_at(19, 20).unwrap();
    assert_eq!(drawn, tile(SHORE as u16, ver, (3, 4), 0));
    let mut copy = g.clone();
    assert_eq!(copy.cache.version(SHORE), drawn.ver, "current in the clone");
    // Other water: the clone's next merge rewrites the shore.
    copy.records[WATER].pixels.fill(0b00011);
    merge_shores(&mut copy);
    assert!(
        copy.cache.version(SHORE) != drawn.ver,
        "stale in the clone after its merge"
    );
    assert_eq!(
        g.cache.version(SHORE),
        drawn.ver,
        "still current in the original"
    );
    // Cloned with the whole State, as the parity runs and the tools clone it.
    let mut st = State::empty();
    st.gfx = g;
    assert_eq!(st.clone().gfx.cache.version(SHORE), drawn.ver);
}

/// The relaunch of `game` after `u` (main_loop's `take_over`): the new process loads the records
/// again (versions from 0) while the buffers keep the old pixels. Its shore merge from other
/// water rewrites as many planes as the old one did, so an old pixel's version is the new
/// record's again with other bytes. The relaunch resets every plane to classic, the colours
/// untouched: no old pixel is current in the back buffer, the screen, the published frame or the
/// shown one.
#[test]
fn a_new_game_process_leaves_no_old_pixel_current() {
    let row = |gfx: &GfxState, f: &mut Framebuffer| {
        for x in (0..320).step_by(16) {
            blit_front(gfx, f, SHORE as u16, x, 16);
        }
    };
    let mut e = Engine::with_game(shore_state(), Files::default(), move |st, io| async move {
        {
            let mut s = st.borrow_mut();
            merge_shores(&mut s.gfx);
            io.draw_back(|b| row(&s.gfx, b));
            io.draw(|f| row(&s.gfx, f));
        }
        io.present();
        io.vblank().await;
        {
            let mut fresh = shore_state();
            install(&mut fresh.gfx, SHORE, map_tile(0, 16, |_, _| 0));
            install(&mut fresh.gfx, WATER, map_tile(0, 16, |_, _| 0b11111));
            let mut s = st.borrow_mut();
            crate::main_loop::take_over(&mut s, fresh, &io);
            merge_shores(&mut s.gfx);
            assert_eq!(
                s.gfx.cache.version(SHORE),
                5,
                "the new versions met the old"
            );
        }
        io.present();
        io.sleep(u32::MAX).await;
    });
    e.enable_src_plane();
    e.tick(&[], 16);
    let old = e.frame().clone();
    assert_eq!(old.src_at(19, 20).map(|p| p.ver), Some(5));
    e.tick(&[], 16);
    let s = e.state();
    let current = |fb: &Framebuffer| {
        fb.src()
            .unwrap()
            .iter()
            .filter(|p| p.kind == Kind::Tile && p.ver == s.gfx.cache.version(usize::from(p.id)))
            .count()
    };
    assert_eq!(current(e.frame()), 0, "shown");
    assert_eq!(e.io().published(current), 0, "published");
    assert_eq!(e.io().draw(|f| current(f)), 0, "screen");
    assert_eq!(e.io().peek_back(current), 0, "back");
    assert!(e.frame().pixels() == old.pixels(), "the colours untouched");
    assert_eq!(kind(e.frame(), 19, 20), Kind::Classic);
}

/// The spellbook of the coverage scene: (x, y, width, height).
const BOOK: (usize, usize, usize, usize) = (0xa8, 6, 144, 98);
/// Its two portraits: (id, x, y, image seed). b'a' << 8 | 3 is portrait.a item 3, b'z' << 8 | 0
/// portrait.z item 0.
const FACES: [(u16, usize, usize, u32); 2] = [(0x6103, 176, 112, 3), (0x7a00, 240, 112, 4)];

/// What the coverage scene must hold at (x, y), from its layout alone: the colour and the source.
/// In the shown map, view cell (cx, cy) shows world (19 - HALF_W + cx, 10 - HALF_H + cy)
/// ((14 + cx, 5 + cy) in Original), whose tile and record is (y % 8)·8 + x % 8 (colour
/// n % 32 + 1, version n % 3 + 1); the hero's one opaque pixel is at the leader's cell plus
/// (8, 8) ((88, 88) in Original), record [`HERO_BASE`] at version 2. Elsewhere on the UI's
/// screen the portraits, the spellbook (id 1) and the paper (id 0), each pixel its image's at
/// (u, v); in Wide the rest of the frame is not drawn.
fn scene_expect(x: usize, y: usize) -> (u8, Src) {
    if in_shown(x, y) {
        let (mx, my) = at_map(0, 0);
        let (vx, vy) = (x - mx, y - my);
        if (vx, vy) == (HALF_W * 16 + 8, HALF_H * 16 + 8) {
            return (30, tile(HERO_BASE, 2, (8, 8), 0));
        }
        let (wx, wy) = (19 - HALF_W + vx / 16, 10 - HALF_H + vy / 16);
        let n = wy % 8 * 8 + wx % 8;
        let uv = ((x % 16) as u16, (y % 16) as u16);
        return (
            (n % 32 + 1) as u8,
            tile(n as u16, (n % 3 + 1) as u32, uv, 0),
        );
    }
    for (id, fx, fy, seed) in FACES {
        if (fx..fx + 56).contains(&x) && (fy..fy + 64).contains(&y) {
            let (u, v) = (x - fx, y - fy);
            return (
                portrait(seed)[v * 56 + u],
                picture(Kind::Portrait, id, (u, v)),
            );
        }
    }
    let (bx, by, bw, bh) = BOOK;
    if (bx..bx + bw).contains(&x) && (by..by + bh).contains(&y) {
        let (u, v) = (x - bx, y - by);
        return (
            image(2, bw, bh)[v * bw + u],
            picture(Kind::Bitmap, 1, (u, v)),
        );
    }
    if x >= UI.0 || y >= UI.1 {
        return (0, Src::CLASSIC);
    }
    (
        image(1, UI.0, UI.1)[y * UI.0 + x],
        picture(Kind::Bitmap, 0, (x, y)),
    )
}

/// The pixels of `fb` whose colour or source is not [`scene_expect`]'s.
fn scene_mismatches(fb: &Framebuffer) -> usize {
    (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let (colour, src) = scene_expect(x, y);
            (fb.get(x, y), fb.src_at(x, y)) != (Some(colour), Some(src))
        })
        .count()
}

/// A synthetic screen of the paper, the map window (64 records at versions 1..=3, the hero's at
/// 2), the spellbook and two portraits, drawn by the game's routines: every pixel of the UI's
/// screen and the shown map is attributed (none classic; the rest of the Wide frame is not
/// drawn) and has exactly its image's colour and source at (u, v), including the id and the
/// version. The check is the layout's ([`scene_expect`]), not the plane's helpers, and it
/// catches a wrong panel id, a wrong version and a shifted (u, v).
#[test]
fn a_scene_of_tiles_portraits_and_panels_is_attributed_exactly() {
    let (mut s, mut g) = world();
    for n in 0..64 {
        for _ in 0..n % 3 + 1 {
            g.cache.record_changed(n);
        }
    }
    g.cache.record_changed(usize::from(HERO_BASE));
    g.cache.record_changed(usize::from(HERO_BASE));
    s.gfx = g;
    let io = Io::new(Files::default());
    io.enable_src_plane();
    let bitmap = |seed, (w, h): (usize, usize)| Bitmap {
        width: w as u16,
        height: h as u16,
        pixels: image(seed, w, h),
    };
    let paper = bitmap(1, UI);
    let (bx, by, bw, bh) = BOOK;
    let book = bitmap(2, (bw, bh));
    io.draw(|f| blit_bmp(&colours(), f, &paper, 0, 0, BITMAP_PAPER));
    crate::gfx::render_window(&mut s, &io, true);
    io.draw(|f| blit_bmp(&colours(), f, &book, bx as u16, by as u16, BITMAP_SPELLBOOK));
    for (id, fx, fy, seed) in FACES {
        let (x, y) = (fx as u16, fy as u16);
        let face = portrait(seed);
        io.draw(|f| {
            blit_bitmap(
                &colours(),
                f,
                (56, 64),
                &face,
                x,
                y,
                BitmapSource::Portrait(id),
            )
        });
    }
    let fb = io.draw(|f| f.clone());
    let drawn = |i: usize| {
        let (x, y) = (i % WIDTH, i / WIDTH);
        (x < UI.0 && y < UI.1) || in_shown(x, y)
    };
    assert!(
        fb.src()
            .unwrap()
            .iter()
            .enumerate()
            .all(|(i, s)| (s.kind != Kind::Classic) == drawn(i)),
        "none classic"
    );
    assert_eq!(scene_mismatches(&fb), 0);
    // The check fails on a wrong panel id, a wrong version, a shifted (u, v) (the source alone,
    // or with the neighbour's colour), and a portrait's id swapped for the other's.
    let wrong = |x: usize, y: usize, f: &dyn Fn(Src) -> Src, colour: Option<u8>| {
        let mut bad = fb.clone();
        let src = f(bad.src_at(x, y).unwrap());
        bad.set_src(x, y, colour.unwrap_or(bad.get(x, y).unwrap()), src);
        scene_mismatches(&bad)
    };
    assert_eq!(
        wrong(200, 50, &|s| Src { id: 0, ..s }, None),
        1,
        "the book named the paper"
    );
    let (vx, vy) = at_map(100, 60);
    assert_eq!(
        wrong(
            vx,
            vy,
            &|s| Src {
                ver: s.ver + 1,
                ..s
            },
            None
        ),
        1,
        "a wrong version"
    );
    let (vx, vy) = at_map(40, 100);
    assert_eq!(
        wrong(vx, vy, &|s| Src { ver: 0, ..s }, None),
        1,
        "version 0"
    );
    assert_eq!(
        wrong(5, 190, &|s| Src { u: s.u + 1, ..s }, None),
        1,
        "a shifted u"
    );
    let right = fb.get(6, 190);
    assert_eq!(
        wrong(5, 190, &|s| Src { u: s.u + 1, ..s }, right),
        1,
        "shifted, its colour"
    );
    assert_eq!(
        wrong(300, 150, &|s| Src { v: s.v + 1, ..s }, None),
        1,
        "a shifted v"
    );
    assert_eq!(
        wrong(250, 150, &|s| Src { id: 0x6103, ..s }, None),
        1,
        "the other portrait"
    );
    assert!(right != fb.get(5, 190), "the paper is not flat there");
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
    let (xs, ys) = view_rect();
    io.peek_back(|b| {
        let tiles = ys
            .clone()
            .flat_map(|y| xs.clone().map(move |x| (x, y)))
            .filter(|&(x, y)| kind(b, x, y) == Kind::Tile)
            .count();
        assert_eq!(tiles, xs.len() * ys.len());
    });
    let (xs, ys) = shown();
    let all_tiles = |f: &mut Framebuffer| {
        ys.clone()
            .all(|y| xs.clone().all(|x| kind(f, x, y) == Kind::Tile))
    };
    assert!(io.draw(all_tiles));
}

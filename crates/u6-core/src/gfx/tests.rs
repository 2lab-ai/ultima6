//! Unit tests on synthetic tiles, and a real-data test that skips without the user's zip.

use u6_formats::gfx::{AnimOverlay, Span, Tile, TileKind};
use u6_render::Framebuffer;

use super::GfxState;
use super::anim::{merge_shores, set_frame, substitute};
use super::blit::{
    blit_back, copy_window, dissolve, dissolve_order, outline_colour, plot, set_plot_colour,
};

mod adv;
mod cache;
mod idle;
mod plane;
mod plane_parity;
mod real;
mod sprite;
mod view;

/// A map record: rows `first..first+rows` opaque with colour `colour(r, c)`.
pub(super) fn map_tile(first: u8, rows: u8, colour: impl Fn(usize, usize) -> u8) -> Tile {
    let mut t = Tile {
        masktype: 0,
        kind: TileKind::Map,
        first_row: first,
        rows,
        pixels: vec![0; 256],
        transparent: vec![true; 256],
        unused_byte: None,
        anim_ref: None,
    };
    for r in usize::from(first)..usize::from(first + rows) {
        for c in 0..16 {
            t.pixels[r * 16 + c] = colour(r, c);
            t.transparent[r * 16 + c] = false;
        }
    }
    t
}

/// An object record of `span` with all 16 rows; `pixel(r, c)` = `Some(colour)` or transparent.
pub(super) fn object_tile(span: Span, pixel: impl Fn(usize, usize) -> Option<u8>) -> Tile {
    let mut t = map_tile(0, 16, |_, _| 0);
    t.masktype = match span {
        Span::Full => 3,
        Span::Left => 2,
        Span::Right => 1,
    };
    t.kind = TileKind::Object(span);
    for r in 0..16 {
        for c in 0..16 {
            let inside = match span {
                Span::Full => true,
                Span::Left => c < 8,
                Span::Right => c >= 8,
            };
            let p = if inside { pixel(r, c) } else { None };
            t.pixels[r * 16 + c] = p.unwrap_or(0);
            t.transparent[r * 16 + c] = p.is_none();
        }
    }
    t
}

/// Installs `tile` as the record of tile number `n`.
pub(super) fn install(gfx: &mut GfxState, n: usize, tile: Tile) {
    gfx.masktype[n] = tile.masktype;
    gfx.records[n] = tile;
}

fn count(fb: &Framebuffer, v: u8) -> usize {
    fb.pixels().iter().filter(|&&p| p == v).count()
}

#[test]
fn map_tile_writes_only_its_rows_on_8_pixel_steps() {
    let mut gfx = GfxState::default();
    install(
        &mut gfx,
        5,
        map_tile(2, 3, |r, c| (r * 16 + c) as u8 % 29 + 1),
    );
    let mut fb = Framebuffer::new();
    fb.clear(31);
    // x = 85 is taken down to the byte boundary 80.
    blit_back(&gfx, &mut fb, 5, 85, 40);
    assert_eq!(fb.get(80, 41), Some(31), "row 1 is outside the record");
    assert_eq!(fb.get(80, 42), Some((2 * 16) as u8 % 29 + 1));
    assert_eq!(fb.get(95, 44), Some((4 * 16 + 15) as u8 % 29 + 1));
    assert_eq!(fb.get(80, 45), Some(31), "row 5 is past the record");
    assert_eq!(fb.get(79, 42), Some(31));
    assert_eq!(fb.get(96, 42), Some(31));
    assert_eq!(count(&fb, 31), 320 * 200 - 3 * 16);
}

#[test]
fn tiles_wrap_into_the_next_row_and_drop_past_the_screen() {
    let mut gfx = GfxState::default();
    install(&mut gfx, 1, map_tile(0, 16, |_, c| c as u8 + 1));
    let mut fb = Framebuffer::new();
    blit_back(&gfx, &mut fb, 1, 312, 10);
    // Columns 0..8 on row 10 at x 312..320, columns 8..16 on row 11 at x 0..8.
    assert_eq!(fb.get(312, 10), Some(1));
    assert_eq!(fb.get(319, 10), Some(8));
    assert_eq!(fb.get(0, 11), Some(9));
    assert_eq!(fb.get(7, 11), Some(16));
    let mut fb = Framebuffer::new();
    blit_back(&gfx, &mut fb, 1, 0, 192);
    assert_eq!(
        count(&fb, 0),
        320 * 200 - 8 * 16,
        "rows past 199 are dropped"
    );
    let mut fb = Framebuffer::new();
    blit_back(&gfx, &mut fb, 1, 0, 200);
    assert_eq!(count(&fb, 0), 320 * 200, "y = 200 is past the row table");
}

#[test]
fn object_tile_keeps_masked_pixels_and_spans_place_halves() {
    let mut gfx = GfxState::default();
    install(
        &mut gfx,
        7,
        object_tile(Span::Full, |r, c| ((r + c) % 2 == 0).then_some(5)),
    );
    install(&mut gfx, 8, object_tile(Span::Left, |_, _| Some(6)));
    install(&mut gfx, 9, object_tile(Span::Right, |_, _| Some(7)));
    let mut fb = Framebuffer::new();
    fb.clear(2);
    blit_back(&gfx, &mut fb, 7, 0, 0);
    assert_eq!(fb.get(0, 0), Some(5));
    assert_eq!(fb.get(1, 0), Some(2), "a set mask bit keeps the screen");
    blit_back(&gfx, &mut fb, 8, 32, 0);
    blit_back(&gfx, &mut fb, 9, 64, 0);
    assert_eq!((fb.get(32, 3), fb.get(40, 3)), (Some(6), Some(2)));
    assert_eq!((fb.get(71, 3), fb.get(72, 3)), (Some(2), Some(7)));
    assert_eq!(fb.get(79, 15), Some(7));
}

#[test]
fn outline_mode_paints_opaque_black_pixels_in_the_mode_colour() {
    assert_eq!(
        [0x0800, 0x1000, 0x1800, 0x2800].map(outline_colour),
        [11, 4, 3, 3]
    );
    let mut gfx = GfxState::default();
    // Colour 0 on the left half, colour 9 on the right, transparent row 0.
    let sprite = |r: usize, c: usize| (r > 0).then_some(if c < 8 { 0 } else { 9 });
    install(&mut gfx, 3, object_tile(Span::Full, sprite));
    install(&mut gfx, 4, object_tile(Span::Right, |_, _| Some(0)));
    let mut fb = Framebuffer::new();
    fb.clear(1);
    blit_back(&gfx, &mut fb, 3 | 0x0800, 0, 0);
    assert_eq!(fb.get(0, 1), Some(11));
    assert_eq!(fb.get(8, 1), Some(1), "coloured pixels are not drawn");
    assert_eq!(fb.get(0, 0), Some(1), "transparent pixels stay");
    blit_back(&gfx, &mut fb, 3 | 0x1000, 16, 0);
    assert_eq!(fb.get(16, 5), Some(4));
    // A right-span outline is not moved by the extra byte: it lands on columns 0..8.
    blit_back(&gfx, &mut fb, 4 | 0x1800, 32, 0);
    assert_eq!((fb.get(32, 0), fb.get(39, 15)), (Some(3), Some(3)));
    assert_eq!(fb.get(40, 0), Some(1));
}

#[test]
fn overlay_ors_colour_bit_3_into_the_screen_for_the_current_frame() {
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
    let mut fb = Framebuffer::new();
    set_frame(&mut gfx, 8 + 2);
    assert_eq!(gfx.frame, 2);
    blit_back(&gfx, &mut fb, 6, 16, 0);
    assert_eq!(fb.get(18, 3), Some(2 | 8));
    assert_eq!(fb.get(17, 3), Some(2));
    // Outline mode applies only to object records; on an object it skips the overlay.
    let mut o = object_tile(Span::Full, |_, _| Some(0));
    o.masktype = 7;
    install(&mut gfx, 6, o);
    let mut fb = Framebuffer::new();
    blit_back(&gfx, &mut fb, 6 | 0x1000, 16, 0);
    assert_eq!(fb.get(18, 3), Some(4), "colour 4 without bit 3: no overlay");
}

#[test]
fn substitution_copies_masktype_and_record_unless_source_is_high() {
    let mut gfx = GfxState::default();
    gfx.masktype[5] = 3;
    substitute(&mut gfx, 10, 5);
    assert_eq!((gfx.masktype[10], gfx.record_of[10]), (3, 5));
    // Chained: 20 takes what 10 draws now, i.e. record 5.
    substitute(&mut gfx, 20, 10);
    assert_eq!(gfx.record_of[20], 5);
    substitute(&mut gfx, 30, 0x200);
    assert_eq!(gfx.record_of[30], 30, "sources from 0x200 up are ignored");
    substitute(&mut gfx, 4000, 5);
    assert_eq!(gfx.record_of.len(), 2048);
}

#[test]
fn shore_merge_moves_masked_pixels_from_the_water_record() {
    let mut gfx = GfxState::default();
    install(&mut gfx, 9, map_tile(0, 16, |_, _| 0b10101));
    install(&mut gfx, 16, map_tile(0, 16, |_, _| 0b01010));
    install(&mut gfx, 17, map_tile(4, 2, |_, _| 1));
    gfx.tables.shore_src[0] = 9 * 4;
    gfx.tables.shore_dst[0] = 16 * 4;
    gfx.tables.shore_src[1] = 9 * 4;
    gfx.tables.shore_dst[1] = 17 * 4;
    // Block 2: two LE offsets (stored as offset - 0x400), then the records.
    let mut b2 = vec![0u8; 64];
    let rec0 = b2.len() as u16;
    b2.extend([4, 2, 0xF0, 0x00, 0x00, 0x0F]); // skip 2 rows, mask rows 2 and 3
    let rec1 = b2.len() as u16;
    b2.extend([1, 1, 0xFF, 0xFF]); // odd skip: ignored
    b2[0..2].copy_from_slice(&rec0.wrapping_sub(0x400).to_le_bytes());
    b2[2..4].copy_from_slice(&rec1.wrapping_sub(0x400).to_le_bytes());
    // The other 30 merges point at a record past the block: ignored.
    for i in 2..32 {
        b2[2 * i..2 * i + 2].copy_from_slice(&0xF000u16.to_le_bytes());
    }
    gfx.animmask.block2 = b2;
    merge_shores(&mut gfx);
    let px = |t: usize, r: usize, c: usize| gfx.records[t].pixels[r * 16 + c];
    assert_eq!(px(16, 2, 0), 0b10101);
    assert_eq!(px(16, 2, 4), 0b01010);
    assert_eq!(px(16, 3, 15), 0b10101);
    assert_eq!(px(16, 3, 11), 0b01010);
    assert_eq!(px(16, 1, 0), 0b01010);
    assert_eq!(px(17, 4, 0), 1);
}

#[test]
fn shore_merge_follows_substituted_records_and_skips_rows_past_them() {
    let mut gfx = GfxState::default();
    install(&mut gfx, 300, map_tile(0, 16, |_, _| 7));
    install(&mut gfx, 9, map_tile(16, 0, |_, _| 0));
    install(&mut gfx, 17, map_tile(4, 2, |_, _| 1));
    substitute(&mut gfx, 9, 300);
    gfx.tables.shore_src[0] = 9 * 4;
    gfx.tables.shore_dst[0] = 17 * 4;
    let mut b2 = vec![0u8; 64];
    for i in 1..32 {
        b2[2 * i..2 * i + 2].copy_from_slice(&0xF000u16.to_le_bytes());
    }
    let rec = b2.len() as u16;
    b2.extend([0, 3, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    b2[0..2].copy_from_slice(&rec.wrapping_sub(0x400).to_le_bytes());
    gfx.animmask.block2 = b2;
    merge_shores(&mut gfx);
    // Stored rows 0 and 1 of tile 17 are pixel rows 4 and 5; stored row 2 does not exist.
    let px = |r: usize| gfx.records[17].pixels[r * 16 + 3];
    assert_eq!((px(4), px(5), px(6)), (7, 7, 0));
}

#[test]
fn plot_sets_or_xors_colours_below_16() {
    let mut gfx = GfxState::default();
    let mut fb = Framebuffer::new();
    fb.clear(0b10011);
    set_plot_colour(&mut gfx, 6);
    plot(&gfx, &mut fb, 9, 9);
    assert_eq!(fb.get(9, 9), Some(6), "set clears plane 4");
    gfx.plot_xor = true;
    plot(&gfx, &mut fb, 10, 9);
    assert_eq!(fb.get(10, 9), Some(0b10011 ^ 6));
    set_plot_colour(&mut gfx, 16);
    plot(&gfx, &mut fb, 11, 9);
    assert_eq!(fb.get(11, 9), Some(0b10011));
}

/// Feedback words with the full period 2^k - 1 for k = 2..=8, found by search (not game data).
fn full_period_taps() -> Vec<u16> {
    (2..=8u32)
        .map(|k| {
            (1u16..1 << k)
                .find(|&fb| {
                    let (mut s, mut n) = (1u16, 0u32);
                    loop {
                        let carry = s & 1 != 0;
                        s >>= 1;
                        if carry {
                            s ^= fb;
                        }
                        n += 1;
                        if s == 1 || n > 1 << k {
                            break;
                        }
                    }
                    n == (1 << k) - 1
                })
                .unwrap()
        })
        .collect()
}

#[test]
fn dissolve_visits_every_pixel_of_the_rect_once() {
    let taps = full_period_taps();
    let order = dissolve_order(10, 12, &taps);
    let mut seen = std::collections::BTreeSet::new();
    assert!(order.iter().all(|&p| seen.insert(p)), "no pixel twice");
    assert_eq!(seen.len(), 120);
    assert_eq!(*order.last().unwrap(), (0, 0));
    assert_eq!(order[0], (1, 0), "starts at register value 1");
    let mut back = Framebuffer::new();
    back.clear(9);
    let mut front = Framebuffer::new();
    dissolve(&taps, &back, &mut front, (20, 30), (29, 41));
    assert_eq!(count(&front, 9), 120);
    assert_eq!((front.get(20, 30), front.get(29, 41)), (Some(9), Some(9)));
    assert_eq!(front.get(30, 41), Some(0));
    // Without a feedback word for the width the rect is copied in raster order.
    assert_eq!(dissolve_order(2, 1, &[]), [(0, 0), (1, 0)]);
}

#[test]
fn dissolve_ends_on_any_feedback_word_and_on_wrapped_corners() {
    // Tampered feedback words (the top bit clear, odd and even ones): every non-zero word brings
    // the register back to 1 (only 0 takes it to 0; see `dissolve_order`), so the order ends
    // and holds each pixel of the rect at most once.
    for feedback in [
        0x0001u16, 0x0002, 0x0006, 0x00ff, 0x1234, 0x4001, 0x7ffe, 0x8000,
    ] {
        let taps = vec![feedback; 15];
        let order = dissolve_order(160, 160, &taps);
        let mut seen = std::collections::BTreeSet::new();
        assert!(
            order
                .iter()
                .all(|&p| seen.insert(p) && p.0 < 160 && p.1 < 160)
        );
        assert!(order.len() <= 160 * 160, "{feedback:#06x}: {}", order.len());
    }
    // Corners that wrap (x1 = x0 - 1 makes the width 0x10000, 0 as a word): nothing is copied.
    // The table has the 15 words of the game's (0x151f3c), so the 16-bit width finds its word.
    let taps = vec![0xb400u16; 15];
    let back = Framebuffer::new();
    let mut front = Framebuffer::new();
    front.clear(3);
    dissolve(&taps, &back, &mut front, (20, 30), (19, 41));
    assert_eq!(count(&front, 3), 320 * 200);
}

#[test]
fn window_copy_moves_exactly_pixels_8_to_168() {
    let mut back = Framebuffer::new();
    back.clear(4);
    let mut front = Framebuffer::new();
    copy_window(&back, &mut front);
    assert_eq!(count(&front, 4), 160 * 160);
    assert_eq!((front.get(8, 8), front.get(167, 167)), (Some(4), Some(4)));
    assert_eq!((front.get(7, 8), front.get(168, 167)), (Some(0), Some(0)));
}

#[test]
fn bitmaps_map_their_bytes_through_the_colour_table_and_copy_rects_by_bytes() {
    let mut colours = [0u8; 256];
    for (i, c) in colours.iter_mut().enumerate() {
        *c = (i as u8).wrapping_mul(3) | 0x20;
    }
    let mut fb = Framebuffer::new();
    let pixels: Vec<u8> = (0..16 * 2).map(|i| i as u8).collect();
    let classic = super::blit::BitmapSource::Classic;
    super::blit::blit_bitmap(&colours, &mut fb, (16, 2), &pixels, 13, 5, classic);
    // x is taken down to the byte column 8; bit 5 of the table byte is not a plane.
    assert_eq!(fb.get(8, 5), Some(0));
    assert_eq!(fb.get(9, 5), Some(3));
    assert_eq!(fb.get(8 + 15, 6), Some((31u8 * 3) & 0x1f));
    assert_eq!(fb.get(8 + 16, 5), Some(0), "16 pixels per row");
    // copy_rect: bytes x0/8..=x1/8 of rows y0..=y1 to dx/8, dy.
    let mut front = Framebuffer::new();
    super::blit::copy_rect(&fb, &mut front, 9, 5, 17, 6, 100, 50);
    assert_eq!(front.get(96 + 1, 50), Some(3), "column 9 lands at 96 + 1");
    assert_eq!(front.get(96 + 15, 51), fb.get(8 + 15, 6));
    assert_eq!(front.get(96 + 16, 50), Some(0), "two bytes copied");
    assert_eq!(front.get(97, 52), Some(0), "two rows copied");
}

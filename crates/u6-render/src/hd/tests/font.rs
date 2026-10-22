//! Spec 11, the HD font mode, on synthetic glyph cells (row bytes and codes written for these
//! tests): which cells it lists, what it writes for them, that the mode off is the composition
//! before it, and that a kept compositor's frame and list are a new one's after every change,
//! with every cell whose listing changed inside the rect it reported. No game data.

use super::*;
use crate::FLAG_INK;

/// Row bytes with ink and paper on every row (bit 7 = leftmost): four 4 × 4 squares, ink at the
/// top left and the bottom right. A pattern, not a letter.
const SHAPE: [u8; 8] = [0xf0, 0xf0, 0xf0, 0xf0, 0x0f, 0x0f, 0x0f, 0x0f];

/// The ink and paper indices of most test cells.
const INK: u8 = 1;
const PAPER: u8 = 2;

/// Draws the glyph of `code` with row bytes `rows` at screen pixel (x, y) as the text path does:
/// set pixels in `ink` with [`FLAG_INK`], clear ones in `paper`, each naming the glyph at its
/// place in the glyph.
fn glyph_px(
    fb: &mut Framebuffer,
    code: u16,
    rows: [u8; 8],
    (x, y): (usize, usize),
    (ink, paper): (u8, u8),
) {
    for (v, r) in rows.into_iter().enumerate() {
        for u in 0..CELL {
            let set = r & (0x80 >> u) != 0;
            let src = Src {
                ver: 0,
                id: code,
                u: u as u16,
                v: v as u16,
                kind: Kind::Glyph,
                flags: if set { FLAG_INK } else { 0 },
            };
            fb.set_src(x + u, y + v, if set { ink } else { paper }, src);
        }
    }
}

/// [`glyph_px`] in cell (col, row), in [`INK`] and [`PAPER`].
fn glyph(fb: &mut Framebuffer, code: u16, (col, row): (usize, usize)) {
    glyph_px(fb, code, SHAPE, (col * CELL, row * CELL), (INK, PAPER));
}

/// No row byte set: the rows of a blank glyph (the space's here).
const BLANK: [u8; 8] = [0; 8];

/// A whole space (code 0x20, [`BLANK`]) in cell (col, row): all 64 pixels [`PAPER`].
fn space(fb: &mut Framebuffer, (col, row): (usize, usize)) {
    glyph_px(fb, 0x20, BLANK, (col * CELL, row * CELL), (INK, PAPER));
}

/// The composition of a new compositor in the font mode `font`: its frame and its list.
fn fresh_font(fb: &Framebuffer, pics: &Pics, scale: Scale, font: bool) -> (Vec<u32>, Vec<HdGlyph>) {
    let mut c = Compositor::new();
    c.set_font_mode(font);
    c.compose(fb, &mut pics.clone(), scale);
    (c.rgba().to_vec(), c.glyphs().to_vec())
}

/// The listing of cell (col, row) as the compositor gives it, in `fb`'s palette.
fn listed(
    fb: &Framebuffer,
    code: u16,
    (col, row): (usize, usize),
    (ink, paper): (u8, u8),
) -> HdGlyph {
    HdGlyph {
        x: (col * CELL) as u16,
        y: (row * CELL) as u16,
        code,
        ink: fb.palette[usize::from(ink)],
        paper: fb.palette[usize::from(paper)],
    }
}

/// The screen pixels of cell (col, row).
fn cell_pixels((col, row): (usize, usize)) -> impl Iterator<Item = (usize, usize)> {
    (0..CELL * CELL).map(move |k| (col * CELL + k % CELL, row * CELL + k / CELL))
}

/// Spec 11 Acceptance 1: a letter and a syllable (and the ends of both ranges) are listed with
/// their ink and paper and composed as their paper; a whole space (no ink pixel, all of one
/// colour) is listed as 0x20 with that colour as its ink and its paper, and composed as it was;
/// a code under the space, codes past 0x7e (a rune) and past the syllables are not; nor is a cell
/// of a listed code that is not one whole glyph: all ink, no ink, half under a tile, under one
/// classic write (the pointer), a pixel of another paper colour, a pixel flagged otherwise, drawn
/// one pixel off the grid, a pixel of another code, a pixel at another version, an ink pixel of
/// another colour, its lower half written again classic in the colours it had (only the sources
/// tell); nor a space with an ink pixel (in the ink's colour, or in the paper's: only the flag
/// tells), with a pixel of another colour, half under a tile, or its lower half written again
/// classic in the colour it had. Every cell not listed is the mode off's.
#[test]
fn the_hd_font_lists_whole_letters_syllables_and_spaces_only() {
    let mut pics = Pics::default();
    pics.put(Kind::Tile, 9, (16, 16), art(9, 16, 16, 4), None);
    let letters = [
        (0x41u16, (1, 1)),
        (0xac00, (3, 1)),
        (0x21, (5, 1)),
        (0x7e, (7, 1)),
        (0xd7a3, (9, 1)),
    ];
    // Whole spaces after the letters in raster order: each cell and its one colour.
    let spaces = [((11, 1), PAPER), ((13, 1), 3)];
    let others = [
        (0x05u16, (3, 3)),
        (0x7f, (5, 3)),
        (0xc1, (7, 3)),
        (0xabff, (9, 3)),
        (0xd7a4, (11, 3)),
    ];
    for scale in [Scale::X2, Scale::X4] {
        let mut fb = plane();
        fb.palette = palette(5);
        fb.clear(30);
        for &(code, at) in letters.iter().chain(&others) {
            glyph(&mut fb, code, at);
        }
        for &((col, row), colour) in &spaces {
            glyph_px(
                &mut fb,
                0x20,
                BLANK,
                (col * CELL, row * CELL),
                (INK, colour),
            );
        }
        // The broken spaces, row 9: one ink pixel, in the ink's colour and in the paper's.
        let dot = [0, 0, 0x10, 0, 0, 0, 0, 0];
        glyph_px(&mut fb, 0x20, dot, (8, 72), (INK, PAPER));
        glyph_px(&mut fb, 0x20, dot, (24, 72), (PAPER, PAPER));
        // A pixel of another colour, its source kept.
        space(&mut fb, (5, 9));
        let s = fb.src_at(43, 75).unwrap();
        fb.set_src(43, 75, 3, s);
        // Its lower half under a tile.
        space(&mut fb, (7, 9));
        draw(&mut fb, &pics, Kind::Tile, 9, (56, 76));
        // Its lower half written again classic in the colour it had.
        space(&mut fb, (10, 9));
        for (x, y) in cell_pixels((10, 9)).filter(|&(_, y)| y >= 76) {
            fb.set(x, y, fb.get(x, y).unwrap());
        }
        // The broken cells, all of a listed code, row 5.
        glyph_px(&mut fb, 0x42, [0xff; 8], (8, 40), (INK, PAPER));
        glyph_px(&mut fb, 0x43, [0; 8], (24, 40), (INK, PAPER));
        glyph(&mut fb, 0x44, (5, 5));
        draw(&mut fb, &pics, Kind::Tile, 9, (40, 44));
        glyph(&mut fb, 0x45, (8, 5));
        fb.set(66, 43, 17);
        glyph(&mut fb, 0x46, (10, 5));
        let s = fb.src_at(80, 47).unwrap();
        assert_eq!(s.flags, 0, "a paper pixel");
        fb.set_src(80, 47, 3, s);
        glyph(&mut fb, 0x47, (12, 5));
        let s = fb.src_at(97, 41).unwrap();
        fb.set_src(
            97,
            41,
            fb.get(97, 41).unwrap(),
            Src {
                flags: s.flags | 2,
                ..s
            },
        );
        glyph_px(&mut fb, 0x48, SHAPE, (113, 40), (INK, PAPER));
        glyph(&mut fb, 0x49, (16, 5));
        let s = fb.src_at(130, 44).unwrap();
        fb.set_src(130, 44, fb.get(130, 44).unwrap(), Src { id: 0x4a, ..s });
        glyph(&mut fb, 0x4b, (18, 5));
        let s = fb.src_at(147, 45).unwrap();
        fb.set_src(147, 45, fb.get(147, 45).unwrap(), Src { ver: 1, ..s });
        glyph(&mut fb, 0x4c, (20, 5));
        let s = fb.src_at(160, 40).unwrap();
        assert_eq!(s.flags, FLAG_INK, "an ink pixel");
        fb.set_src(160, 40, 3, s);
        glyph(&mut fb, 0x4d, (22, 5));
        for (x, y) in cell_pixels((22, 5)).filter(|&(_, y)| y >= 44) {
            fb.set(x, y, fb.get(x, y).unwrap());
        }
        let mut c = Compositor::new();
        c.set_font_mode(true);
        c.compose(&fb, &mut pics, scale);
        let want: Vec<HdGlyph> = letters
            .iter()
            .map(|&(code, at)| listed(&fb, code, at, (INK, PAPER)))
            .chain(
                spaces
                    .iter()
                    .map(|&(at, colour)| listed(&fb, 0x20, at, (colour, colour))),
            )
            .collect();
        assert_eq!(
            c.glyphs(),
            want.as_slice(),
            "{scale:?}: the letters, the syllables and the whole spaces"
        );
        let (off, none) = fresh_font(&fb, &pics, scale, false);
        assert!(none.is_empty(), "off: nothing listed");
        let paper = word(fb.palette[usize::from(PAPER)]);
        let mut cells = vec![false; WIDTH * HEIGHT];
        for &(_, at) in &letters {
            for (x, y) in cell_pixels(at) {
                cells[y * WIDTH + x] = true;
                assert!(
                    block(c.rgba(), scale, (x, y)).iter().all(|&g| g == paper),
                    "{scale:?}: a listed cell is its paper"
                );
            }
        }
        // A listed space is its paper, which is every pixel it had: the mode off's blocks (and
        // left out of `cells`, so the check below holds it to them too).
        for &(at, colour) in &spaces {
            let paper = word(fb.palette[usize::from(colour)]);
            for p in cell_pixels(at) {
                let b = block(c.rgba(), scale, p);
                assert!(
                    b.iter().all(|&g| g == paper) && b == block(&off, scale, p),
                    "{scale:?}: a listed space is its paper, the pixels it had"
                );
            }
        }
        let s = scale.factor();
        let same = (0..c.rgba().len()).all(|i| {
            let (x, y) = (i % (WIDTH * s) / s, i / (WIDTH * s) / s);
            cells[y * WIDTH + x] || c.rgba()[i] == off[i]
        });
        assert!(same, "{scale:?}: every other cell as with the mode off");
    }
}

/// The mode off (the default) composes each glyph pixel as the classic pixel it was before
/// spec 11 (whole spaces, which the mode on lists, included): the frame is that of the same
/// colours drawn classic, byte for byte, with the same images asked for (a glyph is never asked
/// as one) and nothing listed; and a compositor switched on and off again writes every block and
/// is a new one's.
#[test]
fn with_the_font_mode_off_glyphs_are_the_classic_pixels_they_were() {
    let mut pics = Pics::default();
    pics.put(Kind::Tile, 4, (16, 16), art(4, 16, 16, 5), None);
    let mut fb = plane();
    fb.palette = palette(7);
    for k in 0..20 {
        draw(&mut fb, &pics, Kind::Tile, 4, (k * 16, 64));
    }
    for (col, code) in (0..40).zip([0x41u16, 0xac00, 0x20, 0x7f, 0xc1].into_iter().cycle()) {
        glyph(&mut fb, code, (col, 8 + col % 3));
    }
    space(&mut fb, (0, 12));
    space(&mut fb, (1, 12));
    let mut classic = fb.clone();
    for (i, src) in fb.src().unwrap().iter().enumerate() {
        if src.kind == Kind::Glyph {
            classic.set(i % WIDTH, i / WIDTH, fb.pixels()[i]);
        }
    }
    assert!(classic.src().unwrap().iter().all(|s| s.kind != Kind::Glyph));
    for scale in [Scale::X2, Scale::X4] {
        let (mut a, mut b) = (pics.clone(), pics.clone());
        let mut c = Compositor::new();
        assert!(!c.font_mode(), "off by default");
        c.compose(&fb, &mut a, scale);
        let mut d = Compositor::new();
        d.compose(&classic, &mut b, scale);
        assert!(c.rgba() == d.rgba(), "{scale:?}: byte for byte");
        assert_eq!(a.asked, b.asked, "{scale:?}: no glyph asked as an image");
        assert!(c.glyphs().is_empty());
        c.take_dirty();
        for font in [true, false] {
            c.set_font_mode(font);
            c.compose(&fb, &mut a, scale);
            assert_eq!(
                c.take_dirty(),
                Some([0, 0, WIDTH, HEIGHT]),
                "{scale:?}: a mode change writes every block"
            );
            let (want, list) = fresh_font(&fb, &pics, scale, font);
            assert!(c.rgba() == want.as_slice() && c.glyphs() == list.as_slice());
            assert_eq!(list.is_empty(), !font);
        }
        assert!(
            c.rgba() == d.rgba(),
            "{scale:?}: off again, the classic frame"
        );
    }
}

/// A change of the mode empties the list at once, before any composition (off, it is empty, as
/// the mode off's is; on, it waits for a composition in the mode), and the next composition
/// writes every block and is a new one's, also after a change and back with none between;
/// setting the mode it is in keeps the list and makes the next composition write nothing.
#[test]
fn a_change_of_the_font_mode_empties_the_list_at_once() {
    let pics = Pics::default();
    let mut fb = plane();
    fb.palette = palette(6);
    fb.clear(PAPER);
    glyph(&mut fb, 0x41, (4, 6));
    glyph(&mut fb, 0xac00, (5, 6));
    let want = [
        listed(&fb, 0x41, (4, 6), (INK, PAPER)),
        listed(&fb, 0xac00, (5, 6), (INK, PAPER)),
    ];
    let mut c = Compositor::new();
    let compose = |c: &mut Compositor, font: bool, what: &str| {
        c.compose(&fb, &mut pics.clone(), Scale::X2);
        let (frame, list) = fresh_font(&fb, &pics, Scale::X2, font);
        assert!(c.rgba() == frame.as_slice(), "{what}: a new one's frame");
        assert_eq!(c.glyphs(), list.as_slice(), "{what}: a new one's list");
        c.take_dirty()
    };
    c.set_font_mode(true);
    compose(&mut c, true, "on");
    assert_eq!(c.glyphs(), want, "on: both listed");
    c.set_font_mode(true);
    assert_eq!(c.glyphs(), want, "on again: no change, the list kept");
    assert_eq!(compose(&mut c, true, "on again"), None, "nothing written");
    assert_eq!(c.glyphs(), want);
    c.set_font_mode(false);
    assert!(c.glyphs().is_empty(), "off: empty at once");
    c.set_font_mode(true);
    assert!(c.glyphs().is_empty(), "on again: empty until a composition");
    let full = Some([0, 0, WIDTH, HEIGHT]);
    assert_eq!(compose(&mut c, true, "off and on"), full, "every block");
    assert_eq!(c.glyphs(), want, "off and on: listed again");
    c.set_font_mode(false);
    assert!(c.glyphs().is_empty(), "off: empty at once");
    assert_eq!(compose(&mut c, false, "off"), full, "every block");
    assert!(c.glyphs().is_empty(), "off: empty after it");
    c.set_font_mode(false);
    assert_eq!(compose(&mut c, false, "off again"), None, "nothing written");
    c.set_font_mode(true);
    c.set_font_mode(false);
    assert_eq!(compose(&mut c, false, "on and off"), full, "every block");
}

/// A scroll moves the glyphs with their cells (`copy_px_within`, as the text scroll does): the
/// cells moved up are listed at their new place with their codes and colours, a whole space
/// among them (a space with ink is not, before or after), the row the scroll clears is not; a
/// composition of the same frame writes nothing and keeps the list.
#[test]
fn a_scroll_keeps_the_listing() {
    let pics = Pics::default();
    let mut fb = plane();
    fb.palette = palette(1);
    fb.clear(9);
    // A window of columns 2..=5, rows 10..=12: a letter and a syllable on each of rows 11 and 12.
    for (row, codes) in [(11, [0x41u16, 0xac00]), (12, [0x62, 0xd7a3])] {
        for (col, code) in (2..).zip(codes) {
            glyph(&mut fb, code, (col, row));
        }
    }
    space(&mut fb, (4, 12));
    glyph(&mut fb, 0x20, (5, 12));
    let mut c = Compositor::new();
    c.set_font_mode(true);
    c.compose(&fb, &mut pics.clone(), Scale::X2);
    assert_eq!(c.glyphs().len(), 5, "four letters and the whole space");
    let (xs, ys) = (2 * CELL..6 * CELL, 10 * CELL..13 * CELL);
    for y in ys.start..ys.end - CELL {
        for x in xs.clone() {
            fb.copy_px_within((y + CELL) * WIDTH + x, y * WIDTH + x);
        }
    }
    for y in ys.end - CELL..ys.end {
        for x in xs.clone() {
            fb.set(x, y, PAPER);
        }
    }
    c.take_dirty();
    c.compose(&fb, &mut pics.clone(), Scale::X2);
    let want = [
        listed(&fb, 0x41, (2, 10), (INK, PAPER)),
        listed(&fb, 0xac00, (3, 10), (INK, PAPER)),
        listed(&fb, 0x62, (2, 11), (INK, PAPER)),
        listed(&fb, 0xd7a3, (3, 11), (INK, PAPER)),
        listed(&fb, 0x20, (4, 11), (PAPER, PAPER)),
    ];
    assert_eq!(
        c.glyphs(),
        want,
        "moved up a row, with their codes and colours"
    );
    let (frame, list) = fresh_font(&fb, &pics, Scale::X2, true);
    assert!(c.rgba() == frame.as_slice() && c.glyphs() == list.as_slice());
    assert_eq!(c.take_dirty(), Some([16, 80, 32, 24]), "the window's rows");
    c.compose(&fb, &mut pics.clone(), Scale::X2);
    assert_eq!(c.take_dirty(), None, "the same frame writes nothing");
    assert_eq!(c.glyphs(), want, "and keeps its list");
}

/// A cell under the pointer (a classic write over part of it, as the pointer sprite is drawn)
/// is not listed and shows the pixel font's colours; it leaves the list with every block of it
/// written, and comes back to it the same way when the glyph is shown whole again.
#[test]
fn a_cell_under_the_pointer_is_not_listed() {
    let pics = Pics::default();
    let mut fb = plane();
    fb.palette = palette(3);
    fb.clear(PAPER);
    glyph(&mut fb, 0x41, (10, 10));
    glyph(&mut fb, 0x42, (11, 10));
    let mut c = Compositor::new();
    c.set_font_mode(true);
    let mut under = fb.clone();
    under.blit(83, 85, 3, 3, &[17; 9], None);
    for scale in [Scale::X2, Scale::X4] {
        c.compose(&fb, &mut pics.clone(), scale);
        assert_eq!(c.glyphs().len(), 2, "{scale:?}: both letters");
        for (frame, n, what) in [(&under, 1, "under the pointer"), (&fb, 2, "whole again")] {
            c.take_dirty();
            c.compose(frame, &mut pics.clone(), scale);
            assert_eq!(c.glyphs().len(), n, "{scale:?}: {what}");
            assert_eq!(
                c.take_dirty(),
                Some([80, 80, 8, 8]),
                "{scale:?}: {what}: the cell's every block"
            );
            let (want, list) = fresh_font(frame, &pics, scale, true);
            assert!(c.rgba() == want.as_slice() && c.glyphs() == list.as_slice());
        }
        let (off, _) = fresh_font(&under, &pics, scale, false);
        let mut c2 = Compositor::new();
        c2.set_font_mode(true);
        c2.compose(&under, &mut pics.clone(), scale);
        assert!(
            cell_pixels((10, 10)).all(|p| block(c2.rgba(), scale, p) == block(&off, scale, p)),
            "{scale:?}: the pixel font's colours"
        );
    }
}

/// Random glyph cells (listed codes and others, blank ones among them so that some spaces are
/// whole, in changing colours), classic writes over them, tiles, scrolls, cells copied
/// elsewhere, palette changes, mode and scale switches: after each, a kept compositor's frame
/// and list are a new one's in the same mode, every block that changed is in the rect it
/// reported, and so is every cell that entered, left or changed in the list (with a composition
/// that says it wrote), a whole space's too, whose blocks keep their colours.
#[test]
fn a_kept_compositor_in_the_font_mode_equals_a_new_one_after_every_change() {
    let mut pics = Pics::default();
    for id in 0..4u16 {
        pics.put(
            Kind::Tile,
            id,
            (16, 16),
            art(50 + u64::from(id), 16, 16, 4),
            None,
        );
    }
    let codes = [0x41u16, 0x7a, 0xac00, 0xd7a3, 0x20, 0x05, 0xc1];
    let rows = [
        SHAPE,
        [0x81; 8],
        [0x3c, 0, 0x3c, 0, 0x3c, 0, 0x3c, 0],
        BLANK,
    ];
    let mut fb = plane();
    fb.palette = palette(4);
    let mut x = X(0x0bad_cafe_f00d_1234);
    let mut c = Compositor::new();
    let (mut scale, mut font, mut last_font) = (Scale::X2, true, true);
    let (mut listed_steps, mut space_steps, mut space_moves) = (0, 0, 0);
    for step in 0..300 {
        for _ in 0..1 + x.below(5) {
            let (col, row) = (x.below(COLS as u64) as usize, x.below(ROWS as u64) as usize);
            match x.below(12) {
                0..=4 => {
                    let code = codes[x.below(codes.len() as u64) as usize];
                    let r = rows[x.below(rows.len() as u64) as usize];
                    let colours = (1 + x.below(3) as u8, 1 + x.below(3) as u8);
                    glyph_px(&mut fb, code, r, (col * CELL, row * CELL), colours);
                }
                5 => {
                    let (px, py) = (x.below(WIDTH as u64 - 4), x.below(HEIGHT as u64 - 4));
                    fb.blit(px as i32, py as i32, 3, 4, &[17; 12], None);
                }
                6 => {
                    let at = (col.min(COLS - 2) * CELL, row.min(ROWS - 2) * CELL);
                    draw(&mut fb, &pics, Kind::Tile, x.below(4) as u16, at);
                }
                7 | 8 => {
                    // A window scroll: columns col..col+w, rows row..row+h up one cell row.
                    let (w, h) = (1 + x.below(8) as usize, 2 + x.below(6) as usize);
                    let (c0, r0) = (col.min(COLS - w), row.min(ROWS - h));
                    for y in r0 * CELL..(r0 + h - 1) * CELL {
                        for px in c0 * CELL..(c0 + w) * CELL {
                            fb.copy_px_within((y + CELL) * WIDTH + px, y * WIDTH + px);
                        }
                    }
                    for y in (r0 + h - 1) * CELL..(r0 + h) * CELL {
                        for px in c0 * CELL..(c0 + w) * CELL {
                            fb.set(px, y, PAPER);
                        }
                    }
                }
                9 => {
                    // A cell copied to another (a sprite's put-back block, a pixel replay).
                    let to_col = x.below(COLS as u64) as usize;
                    let to_row = x.below(ROWS as u64) as usize;
                    for (k, (px, py)) in cell_pixels((col, row)).enumerate() {
                        let to = (to_row * CELL + k / CELL) * WIDTH + to_col * CELL;
                        fb.copy_px_within(py * WIDTH + px, to + k % CELL);
                    }
                }
                10 => {
                    let i = 1 + x.below(3) as usize;
                    fb.palette[i] = Rgb::new(x.below(256) as u8, 40, 90);
                }
                _ => {
                    // A pixel recoloured in place, its source kept (no writer does this; the
                    // rule must still hold).
                    let (px, py) = (col * CELL + x.below(8) as usize, row * CELL);
                    let s = fb.src_at(px, py).unwrap();
                    fb.set_src(px, py, 1 + x.below(3) as u8, s);
                }
            }
        }
        match x.below(25) {
            0 => font = !font,
            1 => {
                scale = if scale == Scale::X2 {
                    Scale::X4
                } else {
                    Scale::X2
                }
            }
            _ => {}
        }
        let before = (c.rgba().to_vec(), c.glyphs().to_vec());
        let same = c.scale() == scale && last_font == font && step > 0;
        c.set_font_mode(font);
        last_font = font;
        let wrote = c.compose(&fb, &mut pics.clone(), scale);
        let (want, list) = fresh_font(&fb, &pics, scale, font);
        assert!(c.rgba() == want.as_slice(), "step {step}: the kept frame");
        assert_eq!(c.glyphs(), list.as_slice(), "step {step}: the kept list");
        listed_steps += usize::from(!list.is_empty());
        space_steps += usize::from(list.iter().any(|g| g.code == 0x20));
        let rect = c.take_dirty();
        let inside = |bx: usize, by: usize| {
            rect.is_some_and(|[x0, y0, w, h]| {
                (x0..x0 + w).contains(&bx) && (y0..y0 + h).contains(&by)
            })
        };
        if !same {
            continue;
        }
        let s = scale.factor();
        for i in (0..want.len()).filter(|&i| want[i] != before.0[i]) {
            let (bx, by) = (i % (WIDTH * s) / s, i / (WIDTH * s) / s);
            assert!(inside(bx, by), "step {step}: a changed block in the rect");
        }
        let at = |l: &[HdGlyph], g: &HdGlyph| l.iter().find(|o| (o.x, o.y) == (g.x, g.y)).copied();
        let moved: Vec<HdGlyph> = list
            .iter()
            .filter(|g| at(&before.1, g) != Some(**g))
            .chain(before.1.iter().filter(|g| at(&list, g) != Some(**g)))
            .copied()
            .collect();
        space_moves += moved.iter().filter(|g| g.code == 0x20).count();
        for g in &moved {
            let cell = (usize::from(g.x) / CELL, usize::from(g.y) / CELL);
            assert!(
                cell_pixels(cell).all(|(px, py)| inside(px, py)),
                "step {step}: a cell whose listing changed, whole in the rect"
            );
        }
        assert!(
            wrote || moved.is_empty(),
            "step {step}: a list change is a write"
        );
    }
    assert!(listed_steps > 50, "the list was exercised: {listed_steps}");
    assert!(space_steps > 50, "whole spaces were listed: {space_steps}");
    assert!(space_moves > 20, "spaces entered and left: {space_moves}");
}

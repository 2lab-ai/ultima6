//! Spec 08 §1b: the text of `u` with `wide` on, on a synthetic font and a synthetic Hangul table
//! (sample strings written for these tests).

use std::rc::Rc;

use u6_formats::font::{HANGUL_GLYPHS, HangulFont, WouFont};

use crate::intro::mem::{Chip, Ptr, Region, View};
use crate::intro::text::{self, Arg, TextCtx};

/// A font `h` rows high: every glyph but NUL 4 px wide, the space blank, the others ink in
/// their first column.
fn font(h: u8) -> WouFont {
    let mut raw = vec![0u8; 0x304 + 0x100];
    raw[0] = h;
    raw[2] = 0xf;
    for c in 0..256 {
        raw[4 + c] = if c == 0 { 0 } else { 4 };
        raw[0x104 + c] = if c == 0x20 || c == 0 { 0x84 } else { 0x04 };
        raw[0x204 + c] = 0x03;
    }
    for r in 0..usize::from(h) {
        raw[0x304 + r * 4] = 0xf;
    }
    WouFont::parse(&raw).unwrap()
}

/// Syllable i: column 0 of row 0, its index in rows 1 and 2, column 7 of row 6.
fn rows(i: usize) -> [u8; 8] {
    [0x80, i as u8, (i >> 8) as u8, 0, 0, 0, 0x01, 0]
}

fn table() -> Rc<HangulFont> {
    Rc::new(HangulFont {
        glyphs: (0..HANGUL_GLYPHS).map(rows).collect(),
    })
}

fn view() -> View {
    View {
        planes: Ptr::at(Region::Screen0),
        x1: 40,
        x0: 0,
        y0: 0,
        y1: 100,
    }
}

fn ctx(wide: bool) -> TextCtx {
    let v = view();
    TextCtx {
        view: v,
        x: v.x0,
        y: v.y0,
        align: 0,
        wide,
        hangul: Some(table()),
        ..TextCtx::default()
    }
}

const INK: u8 = text::glyph_colour(0xf);

/// Row `r` of the 8 pixels from (x, y): bit 7 = x, set where the ink colour is.
fn bits(chip: &Chip, x: i32, y: i32) -> u8 {
    (0..8).fold(0, |b, dx| {
        (b << 1) | u8::from(chip.pixel(view().planes, x + dx, y) == INK)
    })
}

/// The syllable drawn with its cell's row 0 at (x, y), from its index rows.
fn syllable(chip: &Chip, x: i32, y: i32) -> Option<char> {
    if bits(chip, x, y) != 0x80 || bits(chip, x, y + 6) != 0x01 {
        return None;
    }
    let i = u32::from(bits(chip, x, y + 1)) | u32::from(bits(chip, x, y + 2)) << 8;
    char::from_u32(0xac00 + i)
}

fn wrapped(wide: bool, text: &str) -> Chip {
    let mut chip = Chip::new();
    let mut c = ctx(wide);
    text::wrap(&mut chip, &mut c, &font(8), text.as_bytes());
    chip
}

#[test]
fn a_korean_line_wraps_by_glyph() {
    // 가나 (16 px) + space (4) + 다라 (16) = 36 px fits the 41 px line: one line, by glyph.
    // By byte (12 bytes of 4 px) it would not.
    let chip = wrapped(true, "가나 다라");
    let at = |x, y| syllable(&chip, x, y);
    assert_eq!(
        [at(0, 0), at(8, 0), at(20, 0), at(28, 0)],
        ['가', '나', '다', '라'].map(Some)
    );
    assert_eq!(bits(&chip, 0, 8), 0, "nothing on the second line");
    // 가나다 (24) + space + 라마바 (24) = 52: the second word goes to the next line.
    let chip = wrapped(true, "가나다 라마바");
    let at = |x, y| syllable(&chip, x, y);
    assert_eq!(
        [at(0, 0), at(8, 0), at(16, 0)],
        ['가', '나', '다'].map(Some)
    );
    assert_eq!(
        [at(0, 8), at(8, 8), at(16, 8)],
        ['라', '마', '바'].map(Some)
    );
    assert_eq!(bits(&chip, 24, 0), 0);
    // One word wider than the line: each glyph advances 8, the sixth goes to the next line.
    let chip = wrapped(true, "가나다라마바사");
    let at = |x, y| syllable(&chip, x, y);
    assert_eq!(at(32, 0), Some('마'));
    assert_eq!([at(0, 8), at(8, 8)], [Some('바'), Some('사')]);
    // Latin and Hangul in one line: 4 px and 8 px glyphs.
    let chip = wrapped(true, "a가b");
    let ink = |x| chip.pixel(view().planes, x, 0) == INK;
    assert!(ink(0) && ink(12) && !ink(16));
    assert_eq!(syllable(&chip, 4, 0), Some('가'));
}

#[test]
fn a_syllable_is_a_column_of_the_line_with_its_ink_rows_centred() {
    let paper = text::glyph_colour(0x1e);
    for (h, top) in [(8u8, 0), (10, 1), (11, 2), (7, 0)] {
        let mut chip = Chip::new();
        let mut c = TextCtx {
            bg: 0x1e,
            ..ctx(true)
        };
        text::print(&mut chip, &mut c, &font(h), "가".as_bytes(), &[]);
        assert_eq!(syllable(&chip, 0, top), Some('가'), "height {h}");
        let p = |x, y| chip.pixel(view().planes, x, y);
        for y in 0..i32::from(h) {
            for x in 0..8 {
                let ink = (x, y) == (0, top) || (x, y) == (7, top + 6);
                assert_eq!(p(x, y), if ink { INK } else { paper }, "h {h} ({x}, {y})");
            }
        }
        assert_eq!(p(0, i32::from(h)), 0, "below the line, h {h}");
        assert_eq!(c.x, 8, "an 8-px advance");
    }
    // No table: the column is paper.
    let mut chip = Chip::new();
    let mut c = TextCtx {
        bg: 0x1e,
        hangul: None,
        ..ctx(true)
    };
    text::print(&mut chip, &mut c, &font(8), "가".as_bytes(), &[]);
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(chip.pixel(view().planes, x, y), paper);
        }
    }
    assert_eq!(c.x, 8);
}

#[test]
fn printf_assembles_across_the_format_and_its_arguments() {
    let mut chip = Chip::new();
    let mut c = ctx(true);
    text::print(
        &mut chip,
        &mut c,
        &font(8),
        "가%s".as_bytes(),
        &[Arg::Str("나".as_bytes().to_vec())],
    );
    assert_eq!(syllable(&chip, 0, 0), Some('가'));
    assert_eq!(syllable(&chip, 8, 0), Some('나'));
    // Into the buffer as bytes, laid out as codes by `%P`.
    let mut chip = Chip::new();
    let mut c = ctx(true);
    text::print_new(
        &mut chip,
        &mut c,
        &font(8),
        "다 %s".as_bytes(),
        &[Arg::Str("라".as_bytes().to_vec())],
    );
    assert_eq!(c.buf, "다 라".as_bytes());
    text::print(&mut chip, &mut c, &font(8), b"%P", &[]);
    assert_eq!(
        [syllable(&chip, 0, 0), syllable(&chip, 12, 0)],
        [Some('다'), Some('라')]
    );
}

#[test]
fn wide_on_ascii_and_latin_1_draw_as_with_it_off() {
    let f = font(8);
    for t in [
        b"ab cd ef gh ij kl mn op".as_slice(),
        b"caf\xe9 \x8f ok",
        b"x\xea",
        b"y\xed\x9e",
        b"\xea\xea\xb0 z",
    ] {
        for align in 0..4 {
            let draw = |wide: bool| {
                let mut chip = Chip::new();
                let mut c = TextCtx { align, ..ctx(wide) };
                text::wrap(&mut chip, &mut c, &f, t);
                text::print(&mut chip, &mut c, &f, t, &[]);
                text::print(
                    &mut chip,
                    &mut c,
                    &f,
                    b"%X%s",
                    &[Arg::Word(3), Arg::Str(t.to_vec())],
                );
                (chip, c.x, c.y)
            };
            assert!(draw(true) == draw(false), "{t:02x?} align {align}");
        }
    }
}

//! Glyph cells over the paper (the text windows show the HD paper in place, spec 07 HD0): a
//! glyph's clear pixel drawn over the paper ([`FLAG_PAPER`]) whose colour is the paper's there
//! shows the HD paper's block of its own place, with the font mode off and in a cell the mode
//! does not list; a cell the mode lists shows the HD paper at each pixel drawn over the paper
//! where the paper's colour is the cell's paper colour, and its flat paper colour elsewhere; ink
//! pixels, other colours, pixels past the paper and glyphs not drawn over the paper (over the
//! spellbook, in the paper's colour) are as before. On a synthetic paper with a synthetic pack
//! image; no game data.

use super::font::{INK, PAPER, SHAPE, glyph_px, listed};
use super::*;
use crate::{BITMAP_PAPER, BITMAP_SPELLBOOK, FLAG_INK, FLAG_PAPER};

/// The test paper's size: smaller than the screen, so a cell past it has no paper under it.
const PW: usize = 96;
const PH: usize = 64;

/// The paper's other colour, at two pixels of cell (1, 1): (13, 10), a clear pixel of
/// [`SHAPE`] there, and (9, 9), an ink pixel.
const OTHER: u8 = 5;

/// A cell drawn: code, cell, the glyph's rows, its paper colour, whether the HD font mode lists
/// it, and whether it is drawn over the paper ([`FLAG_PAPER`] on its pixels).
type Cell = (u16, (usize, usize), [u8; 8], u8, bool, bool);

/// The cells drawn.
const CELLS: [Cell; 8] = [
    // A letter in the paper's colour.
    (0x41, (1, 1), SHAPE, PAPER, true, true),
    // A letter in another paper colour than the paper's.
    (0x42, (3, 1), SHAPE, 3, true, true),
    // A code the mode does not list (a control code, as the key wait's cursor), in the paper's
    // colour.
    (0x05, (5, 1), SHAPE, PAPER, false, true),
    // A whole space in the paper's colour.
    (0x20, (7, 1), [0; 8], PAPER, true, true),
    // A letter and a code the mode does not list, in the paper's colour, over the spellbook
    // ([`spellbook`]): not over the paper.
    (0x44, (9, 1), SHAPE, PAPER, true, false),
    (0x06, (10, 1), SHAPE, PAPER, false, false),
    // A letter in the paper's colour past the paper's width.
    (0x43, (14, 1), SHAPE, PAPER, true, true),
    // A letter in the paper's colour on the paper's last row of cells.
    (0xac00, (2, 7), SHAPE, PAPER, true, true),
];

/// The spellbook drawn under cells (9, 1) and (10, 1): all of the paper's colour, at (72, 8).
const SPELLBOOK: ((usize, usize), (usize, usize)) = ((72, 8), (16, 8));

/// The paper: [`PAPER`] everywhere but [`OTHER`] at (13, 10) and (9, 9).
fn paper_pixels() -> Vec<u8> {
    let mut px = vec![PAPER; PW * PH];
    px[10 * PW + 13] = OTHER;
    px[9 * PW + 9] = OTHER;
    px
}

/// The images (the paper alone), a pack with a patterned image for it, and the frame: colour
/// 30, the paper drawn at (0, 0) naming itself, the cells of [`CELLS`] over it.
fn scene() -> (Pics, TestPack, Framebuffer) {
    let mut pics = Pics::default();
    pics.put(Kind::Bitmap, BITMAP_PAPER, (PW, PH), paper_pixels(), None);
    let mut pack = TestPack::default();
    let key = key_of(&pics, Kind::Bitmap, BITMAP_PAPER);
    pack.put(Kind::Bitmap, BITMAP_PAPER, key, (PW * 4, PH * 4), 21);
    let mut fb = plane();
    fb.palette = palette(9);
    fb.clear(30);
    draw(&mut fb, &pics, Kind::Bitmap, BITMAP_PAPER, (0, 0));
    let ((bx, by), (bw, bh)) = SPELLBOOK;
    let mut book = pics.clone();
    book.put(
        Kind::Bitmap,
        BITMAP_SPELLBOOK,
        (bw, bh),
        vec![PAPER; bw * bh],
        None,
    );
    draw(&mut fb, &book, Kind::Bitmap, BITMAP_SPELLBOOK, (bx, by));
    for (code, (col, row), rows, paper, _, on) in CELLS {
        draw_cell(&mut fb, code, rows, (col, row), paper, on);
    }
    (pics, pack, fb)
}

/// Draws a glyph cell in [`INK`] and `paper` as the text path does, its pixels carrying
/// [`FLAG_PAPER`] when it is drawn over the paper (`on`).
fn draw_cell(
    fb: &mut Framebuffer,
    code: u16,
    rows: [u8; 8],
    (col, row): (usize, usize),
    paper: u8,
    on: bool,
) {
    let (x0, y0) = (col * CELL, row * CELL);
    glyph_px(fb, code, rows, (x0, y0), (INK, paper));
    for (x, y) in (y0..y0 + CELL).flat_map(|y| (x0..x0 + CELL).map(move |x| (x, y))) {
        let s = fb.src_at(x, y).unwrap();
        let flags = if on { s.flags | FLAG_PAPER } else { s.flags };
        fb.set_src(x, y, fb.get(x, y).unwrap(), Src { flags, ..s });
    }
}

/// The frame by the rule, from the layout (no compositor): a paper pixel its block of the HD
/// paper (the pack's image when given, else MMPX); a glyph pixel in a cell the mode lists (with
/// `font`) the HD paper at its own place over the cell's paper colour where it is drawn over the
/// paper and the paper's colour there is that colour, else that colour flat; any other glyph
/// pixel without ink drawn over the paper the HD paper at its own place where its colour is the
/// paper's there; every other pixel its colour.
fn want(
    fb: &Framebuffer,
    pics: &Pics,
    pack: Option<&TestPack>,
    scale: Scale,
    font: bool,
) -> Vec<u32> {
    let s = scale.factor();
    let ow = WIDTH * s;
    let pal = |i: u8| word(fb.palette[usize::from(i)]);
    let paper = paper_pixels();
    let paper_at = |x: usize, y: usize| (x < PW && y < PH).then(|| pal(paper[y * PW + x]));
    let hd: Vec<[u8; 4]> = match pack {
        Some(p) => {
            let a = &p.images[0];
            pack_ref(&a.5, a.3, a.4, scale)
        }
        None => {
            let (_, words) = hd_of(pics, Kind::Bitmap, BITMAP_PAPER, &fb.palette, scale);
            words.iter().map(|w| w.to_le_bytes()).collect()
        }
    };
    let hd_block = |(u, v): (usize, usize), under: u32| -> Vec<u32> {
        (0..s * s)
            .map(|k| hd[(v * s + k / s) * PW * s + u * s + k % s])
            .map(|c| u32::from_le_bytes(over_ref(c, under.to_le_bytes())))
            .collect()
    };
    // The paper colour of the listed cell holding (x, y): the colour of its clear pixels.
    let listed_paper = |x: usize, y: usize| {
        let (col, row) = (x / CELL, y / CELL);
        CELLS.iter().find(|c| c.4 && c.1 == (col, row))?;
        let clear = (0..CELL * CELL)
            .map(|k| (col * CELL + k % CELL, row * CELL + k / CELL))
            .find(|&(x, y)| fb.src_at(x, y).unwrap().flags & FLAG_INK == 0)?;
        fb.get(clear.0, clear.1).map(pal)
    };
    let mut out = vec![0u32; ow * HEIGHT * s];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let src = fb.src_at(x, y).unwrap();
            let classic = pal(fb.get(x, y).unwrap());
            let on = src.flags & FLAG_PAPER != 0;
            let block = match (src.kind, listed_paper(x, y).filter(|_| font)) {
                (Kind::Bitmap, _) => hd_block((usize::from(src.u), usize::from(src.v)), classic),
                (Kind::Glyph, Some(p)) if on && paper_at(x, y) == Some(p) => hd_block((x, y), p),
                (Kind::Glyph, Some(p)) => vec![p; s * s],
                (Kind::Glyph, None)
                    if on && src.flags & FLAG_INK == 0 && paper_at(x, y) == Some(classic) =>
                {
                    hd_block((x, y), classic)
                }
                _ => vec![classic; s * s],
            };
            for (k, c) in block.into_iter().enumerate() {
                out[(y * s + k / s) * ow + x * s + k % s] = c;
            }
        }
    }
    out
}

/// A new compositor's frame in mode `font` with `pack`.
fn fresh_in(
    fb: &Framebuffer,
    pics: &Pics,
    pack: Option<&TestPack>,
    scale: Scale,
    font: bool,
) -> Vec<u32> {
    let mut c = Compositor::new();
    c.set_font_mode(font);
    c.set_pack(pack.map(|p| Rc::new(p.clone()) as Rc<dyn Pack>));
    c.compose(fb, &mut pics.clone(), scale);
    c.rgba().to_vec()
}

/// With the font mode off: the clear pixels of the glyphs in the paper's colour show the HD
/// paper's block of their own place; the one over the paper's other colour, the ink pixels, the
/// cell of another paper colour and the cell past the paper are their classic colours. A kept
/// compositor follows a pack set and a cell redrawn in another paper colour and back as a new one
/// would.
#[test]
fn a_glyphs_clear_pixels_of_the_papers_colour_show_the_hd_paper_in_place() {
    let (pics, pack, mut fb) = scene();
    for scale in [Scale::X2, Scale::X4] {
        let mut c = Compositor::new();
        c.compose(&fb, &mut pics.clone(), scale);
        assert!(
            c.rgba() == want(&fb, &pics, None, scale, false),
            "{scale:?}: MMPX"
        );
        c.set_pack(Some(Rc::new(pack.clone())));
        c.compose(&fb, &mut pics.clone(), scale);
        let with_pack = want(&fb, &pics, Some(&pack), scale, false);
        assert!(c.rgba() == with_pack, "{scale:?}: the pack's paper");
        let classic = |x: usize, y: usize| {
            vec![word(fb.palette[usize::from(fb.get(x, y).unwrap())]); scale.factor().pow(2)]
        };
        assert!(
            block(c.rgba(), scale, (12, 10)) != classic(12, 10),
            "{scale:?}: a clear pixel shows HD"
        );
        for (p, what) in [
            ((13, 10), "over the paper's other colour"),
            ((8, 8), "ink"),
            ((28, 10), "another paper colour"),
            ((116, 10), "past the paper"),
        ] {
            assert!(
                block(c.rgba(), scale, p) == classic(p.0, p.1),
                "{scale:?}: {what}: classic"
            );
        }
        for paper in [3, PAPER] {
            draw_cell(&mut fb, 0x41, SHAPE, (1, 1), paper, true);
            c.compose(&fb, &mut pics.clone(), scale);
            assert!(
                c.rgba() == fresh_in(&fb, &pics, Some(&pack), scale, false),
                "{scale:?}: redrawn in {paper}"
            );
            assert!(c.rgba() == want(&fb, &pics, Some(&pack), scale, false));
        }
    }
}

/// With the font mode on: each cell it lists shows the HD paper at the pixels where the paper's
/// colour is the cell's paper colour (its ink pixels too: the platform draws the character over
/// it) and its paper colour flat at the paper's other colour, past the paper and in the cell of
/// another paper colour; the cell it does not list is as with the mode off; the list is as before.
/// A kept compositor follows a pack set, a mode change and a cell redrawn as a new one would.
#[test]
fn a_listed_cell_over_the_paper_shows_the_hd_paper_where_its_paper_is_the_papers() {
    let (pics, pack, mut fb) = scene();
    for scale in [Scale::X2, Scale::X4] {
        let mut c = Compositor::new();
        c.set_font_mode(true);
        c.compose(&fb, &mut pics.clone(), scale);
        assert!(
            c.rgba() == want(&fb, &pics, None, scale, true),
            "{scale:?}: MMPX"
        );
        let list: Vec<HdGlyph> = CELLS
            .iter()
            .filter(|c| c.4)
            .map(|&(code, at, _, paper, _, _)| {
                let ink = if code == 0x20 { paper } else { INK };
                listed(&fb, code, at, (ink, paper))
            })
            .collect();
        assert_eq!(
            c.glyphs(),
            list.as_slice(),
            "{scale:?}: the cells listed as before"
        );
        c.set_pack(Some(Rc::new(pack.clone())));
        c.compose(&fb, &mut pics.clone(), scale);
        assert!(
            c.rgba() == want(&fb, &pics, Some(&pack), scale, true),
            "{scale:?}: the pack's paper"
        );
        let flat = |i: u8| vec![word(fb.palette[usize::from(i)]); scale.factor().pow(2)];
        assert!(
            block(c.rgba(), scale, (8, 8)) != flat(PAPER),
            "{scale:?}: an ink pixel shows HD"
        );
        assert!(
            block(c.rgba(), scale, (12, 10)) != flat(PAPER),
            "{scale:?}: a clear pixel shows HD"
        );
        for (p, colour, what) in [
            ((13, 10), PAPER, "the paper's other colour"),
            ((9, 9), PAPER, "the other colour under ink"),
            ((28, 10), 3, "another paper colour"),
            ((116, 10), PAPER, "past the paper"),
        ] {
            assert!(
                block(c.rgba(), scale, p) == flat(colour),
                "{scale:?}: {what}: flat"
            );
        }
        for font in [false, true] {
            c.set_font_mode(font);
            c.compose(&fb, &mut pics.clone(), scale);
            assert!(
                c.rgba() == fresh_in(&fb, &pics, Some(&pack), scale, font),
                "{scale:?}: mode {font}"
            );
        }
        for paper in [3, PAPER] {
            draw_cell(&mut fb, 0x41, SHAPE, (1, 1), paper, true);
            c.compose(&fb, &mut pics.clone(), scale);
            assert!(
                c.rgba() == fresh_in(&fb, &pics, Some(&pack), scale, true),
                "{scale:?}: redrawn in {paper}"
            );
            assert!(c.rgba() == want(&fb, &pics, Some(&pack), scale, true));
        }
    }
}

/// Text not drawn over the paper (no [`FLAG_PAPER`]: over the spellbook here) stays as it was
/// before the paper showed behind text, though its paper colour is the paper's there: a listed
/// cell flat in its paper colour with the font mode on, every pixel classic with it off; the same
/// cells drawn over the paper show the HD paper.
#[test]
fn a_glyph_not_drawn_over_the_paper_shows_no_paper_in_its_colour() {
    let (pics, pack, fb) = scene();
    let cells = [(9, 1), (10, 1)];
    let pixels = || {
        cells.into_iter().flat_map(|(col, row)| {
            (0..CELL * CELL).map(move |k| (col * CELL + k % CELL, row * CELL + k / CELL))
        })
    };
    for scale in [Scale::X2, Scale::X4] {
        let n = scale.factor().pow(2);
        for font in [false, true] {
            let frame = fresh_in(&fb, &pics, Some(&pack), scale, font);
            for (x, y) in pixels() {
                let index = if font && x < 80 {
                    PAPER
                } else {
                    fb.get(x, y).unwrap()
                };
                let flat = vec![word(fb.palette[usize::from(index)]); n];
                assert!(
                    block(&frame, scale, (x, y)) == flat,
                    "{scale:?}, font {font}: ({x}, {y}) not over the paper"
                );
            }
        }
        let mut over = fb.clone();
        for (code, at) in [(0x44, (9, 1)), (0x06, (10, 1))] {
            draw_cell(&mut over, code, SHAPE, at, PAPER, true);
        }
        let frame = fresh_in(&over, &pics, Some(&pack), scale, false);
        let shown = pixels()
            .filter(|&(x, y)| over.src_at(x, y).unwrap().flags & FLAG_INK == 0)
            .filter(|&(x, y)| {
                block(&frame, scale, (x, y)) != vec![word(over.palette[usize::from(PAPER)]); n]
            })
            .count();
        assert!(
            shown > 0,
            "{scale:?}: the same cells over the paper show it"
        );
    }
}

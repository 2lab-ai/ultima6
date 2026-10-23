//! Glyph cells over the paper (the text windows show the HD paper in place, spec 07 HD0): a
//! glyph's clear pixel whose colour is the paper's there shows the HD paper's block of its own
//! place, with the font mode off and in a cell the mode does not list; a cell the mode lists shows
//! the HD paper at each pixel where the paper's colour is the cell's paper colour, and its flat
//! paper colour elsewhere; ink pixels, other colours and pixels past the paper are as before. On a
//! synthetic paper with a synthetic pack image; no game data.

use super::font::{INK, PAPER, SHAPE, glyph_px, listed};
use super::*;
use crate::{BITMAP_PAPER, FLAG_INK};

/// The test paper's size: smaller than the screen, so a cell past it has no paper under it.
const PW: usize = 96;
const PH: usize = 64;

/// The paper's other colour, at two pixels of cell (1, 1): (13, 10), a clear pixel of
/// [`SHAPE`] there, and (9, 9), an ink pixel.
const OTHER: u8 = 5;

/// A cell drawn over the paper: code, cell, the glyph's rows, its paper colour, and whether the
/// HD font mode lists it.
type Cell = (u16, (usize, usize), [u8; 8], u8, bool);

/// The cells drawn over the paper.
const CELLS: [Cell; 6] = [
    // A letter in the paper's colour.
    (0x41, (1, 1), SHAPE, PAPER, true),
    // A letter in another paper colour than the paper's.
    (0x42, (3, 1), SHAPE, 3, true),
    // A code the mode does not list (a control code, as the key wait's cursor), in the paper's
    // colour.
    (0x05, (5, 1), SHAPE, PAPER, false),
    // A whole space in the paper's colour.
    (0x20, (7, 1), [0; 8], PAPER, true),
    // A letter in the paper's colour past the paper's width.
    (0x43, (14, 1), SHAPE, PAPER, true),
    // A letter in the paper's colour on the paper's last row of cells.
    (0xac00, (2, 7), SHAPE, PAPER, true),
];

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
    for (code, (col, row), rows, paper, _) in CELLS {
        glyph_px(&mut fb, code, rows, (col * CELL, row * CELL), (INK, paper));
    }
    (pics, pack, fb)
}

/// The frame by the rule, from the layout (no compositor): a paper pixel its block of the HD
/// paper (the pack's image when given, else MMPX); a glyph pixel in a cell the mode lists (with
/// `font`) the HD paper at its own place over the cell's paper colour where the paper's colour
/// there is that colour, else that colour flat; any other glyph pixel without ink the HD paper
/// at its own place where its colour is the paper's there; every other pixel its colour.
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
            let block = match (src.kind, listed_paper(x, y).filter(|_| font)) {
                (Kind::Bitmap, _) => hd_block((usize::from(src.u), usize::from(src.v)), classic),
                (Kind::Glyph, Some(p)) if paper_at(x, y) == Some(p) => hd_block((x, y), p),
                (Kind::Glyph, Some(p)) => vec![p; s * s],
                (Kind::Glyph, None)
                    if src.flags & FLAG_INK == 0 && paper_at(x, y) == Some(classic) =>
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
            glyph_px(&mut fb, 0x41, SHAPE, (8, 8), (INK, paper));
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
            .map(|&(code, at, _, paper, _)| {
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
            glyph_px(&mut fb, 0x41, SHAPE, (8, 8), (INK, paper));
            c.compose(&fb, &mut pics.clone(), scale);
            assert!(
                c.rgba() == fresh_in(&fb, &pics, Some(&pack), scale, true),
                "{scale:?}: redrawn in {paper}"
            );
            assert!(c.rgba() == want(&fb, &pics, Some(&pack), scale, true));
        }
    }
}

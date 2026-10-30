//! Pixel glyphs over HD paper: only clear pixels with matching paper colour use the HD
//! background. Ink remains the original pixel shape, including Hangul. Synthetic data only.

use super::font::{INK, PAPER, SHAPE, glyph_px, listed};
use super::*;
use crate::{BITMAP_PAPER, BITMAP_SPELLBOOK, FLAG_INK, FLAG_PAPER, FLAG_PAPER_OFF};

/// The test paper's size: smaller than the screen, so a cell past it has no paper under it.
const PW: usize = 96;
const PH: usize = 64;

/// The paper's other colour, at two pixels of cell (1, 1): (13, 10), a clear pixel of
/// [`SHAPE`] there, and (9, 9), an ink pixel.
const OTHER: u8 = 5;

/// A cell drawn: code, cell, the glyph's rows, its paper colour, whether diagnostics list
/// it, and whether it is drawn over the paper ([`FLAG_PAPER`] on its pixels).
type Cell = (u16, (usize, usize), [u8; 8], u8, bool, bool);

/// The cells drawn.
const CELLS: [Cell; 8] = [
    // A letter in the paper's colour.
    (0x41, (1, 1), SHAPE, PAPER, true, true),
    // A letter in another paper colour than the paper's.
    (0x42, (3, 1), SHAPE, 3, true, true),
    // A code diagnostics do not list (a control code, as the key wait's cursor), in the paper's
    // colour.
    (0x05, (5, 1), SHAPE, PAPER, false, true),
    // A whole space in the paper's colour.
    (0x20, (7, 1), [0; 8], PAPER, true, true),
    // A letter and a code diagnostics do not list, in the paper's colour, over the spellbook
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

/// Independent pixel oracle: images use their HD blocks, clear glyph pixels matching the
/// paper use that background, and every ink pixel keeps its original colour.
fn want(fb: &Framebuffer, pics: &Pics, pack: Option<&TestPack>, scale: Scale) -> Vec<u32> {
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
    let mut out = vec![0u32; ow * HEIGHT * s];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let src = fb.src_at(x, y).unwrap();
            let classic = pal(fb.get(x, y).unwrap());
            let on = src.flags & FLAG_PAPER != 0;
            let block = match src.kind {
                Kind::Bitmap => hd_block((usize::from(src.u), usize::from(src.v)), classic),
                Kind::Glyph
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

/// A new compositor's pixel-text frame with `pack`.
fn fresh_in(fb: &Framebuffer, pics: &Pics, pack: Option<&TestPack>, scale: Scale) -> Vec<u32> {
    let mut c = Compositor::new();
    c.set_pack(pack.map(|p| Rc::new(p.clone()) as Rc<dyn Pack>));
    c.compose(fb, &mut pics.clone(), scale);
    c.rgba().to_vec()
}

/// The clear pixels of the glyphs in the paper's colour show the HD
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
        assert!(c.rgba() == want(&fb, &pics, None, scale), "{scale:?}: MMPX");
        c.set_pack(Some(Rc::new(pack.clone())));
        c.compose(&fb, &mut pics.clone(), scale);
        let with_pack = want(&fb, &pics, Some(&pack), scale);
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
                c.rgba() == fresh_in(&fb, &pics, Some(&pack), scale),
                "{scale:?}: redrawn in {paper}"
            );
            assert!(c.rgba() == want(&fb, &pics, Some(&pack), scale));
        }
    }
}

/// The diagnostic list coexists with original ink over the pack's HD background.
#[test]
fn listed_latin_and_hangul_ink_stays_visible_over_hd_paper() {
    let (pics, pack, mut fb) = scene();
    for scale in [Scale::X2, Scale::X4] {
        let mut c = Compositor::new();
        c.set_pack(Some(Rc::new(pack.clone())));
        c.compose(&fb, &mut pics.clone(), scale);
        let list: Vec<HdGlyph> = CELLS
            .iter()
            .filter(|c| c.4)
            .map(|&(code, at, _, paper, _, _)| {
                listed(
                    &fb,
                    code,
                    at,
                    (if code == 0x20 { paper } else { INK }, paper),
                )
            })
            .collect();
        assert_eq!(c.glyphs(), list);
        assert_eq!(c.rgba(), want(&fb, &pics, Some(&pack), scale));
        let ink = vec![word(fb.palette[usize::from(INK)]); scale.factor().pow(2)];
        for at in [(8, 8), (9, 9), (16, 56)] {
            assert_eq!(
                block(c.rgba(), scale, at),
                ink,
                "{scale:?}: Latin/Hangul ink remains"
            );
        }
        for paper in [3, PAPER] {
            draw_cell(&mut fb, 0x41, SHAPE, (1, 1), paper, true);
            c.compose(&fb, &mut pics.clone(), scale);
            assert_eq!(c.rgba(), fresh_in(&fb, &pics, Some(&pack), scale));
            assert_eq!(c.rgba(), want(&fb, &pics, Some(&pack), scale));
        }
    }
}

/// A layer with the same source and pixels can reuse the published pixel-text block,
/// including its HD paper. Listing the glyph for diagnostics must not disable that reuse.
#[test]
fn a_listed_pixel_text_layer_reuses_its_original_ink_and_hd_paper() {
    let (pics, pack, fb) = scene();
    let mut layer = crate::motion::LayerPixels::clear(8, 8, 8, 8);
    for y in 0..8 {
        for x in 0..8 {
            let i = y * 8 + x;
            layer.pixels[i] = fb.get(x + 8, y + 8).unwrap();
            layer.src[i] = fb.src_at(x + 8, y + 8).unwrap();
            layer.drawn[i] = true;
        }
    }
    for scale in [Scale::X2, Scale::X4] {
        let mut c = Compositor::new();
        c.set_pack(Some(Rc::new(pack.clone())));
        c.compose(&fb, &mut pics.clone(), scale);
        let expected = want(&fb, &pics, Some(&pack), scale);
        let s = scale.factor();
        let mut out = Vec::new();
        for observe in [false, true] {
            if observe {
                assert!(c.glyphs().iter().any(|g| g.x == 8 && g.y == 8));
            }
            c.compose_layer(&layer, &fb.palette, &mut pics.clone(), scale, &mut out);
            for y in 0..8 * s {
                for x in 0..8 * s {
                    assert_eq!(
                        out[y * 8 * s + x],
                        expected[(8 * s + y) * WIDTH * s + 8 * s + x],
                        "{scale:?}, diagnostics {observe}: layer pixel ({x}, {y})"
                    );
                }
            }
        }
    }
}

/// Text outside the paper retains its original pixels, even where its paper colour matches
/// the paper bitmap. The same clear glyph pixels over the paper can show its HD image.
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
        {
            let frame = fresh_in(&fb, &pics, Some(&pack), scale);
            for (x, y) in pixels() {
                let index = fb.get(x, y).unwrap();
                let flat = vec![word(fb.palette[usize::from(index)]); n];
                assert!(
                    block(&frame, scale, (x, y)) == flat,
                    "{scale:?}: ({x}, {y}) not over the paper"
                );
            }
        }
        let mut over = fb.clone();
        for (code, at) in [(0x44, (9, 1)), (0x06, (10, 1))] {
            draw_cell(&mut over, code, SHAPE, at, PAPER, true);
        }
        let frame = fresh_in(&over, &pics, Some(&pack), scale);
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

/// A pixel naming the paper flagged [`FLAG_PAPER_OFF`] (written on the paper in another colour)
/// is composed as its classic block, never the paper's HD image, even where its classic colour is
/// the paper's at its place; the same pixel unflagged shows the paper's block.
#[test]
fn a_paper_pixel_flagged_off_colour_is_its_classic_block() {
    let (pics, pack, mut fb) = scene();
    let (x, y) = (40, 40);
    assert_eq!(
        paper_pixels()[y * PW + x],
        PAPER,
        "the paper's colour at the pixel"
    );
    let named = fb.src_at(x, y).unwrap();
    assert_eq!(
        (named.kind, named.id, named.flags),
        (Kind::Bitmap, BITMAP_PAPER, 0)
    );
    for scale in [Scale::X2, Scale::X4] {
        let classic = vec![word(fb.palette[usize::from(PAPER)]); scale.factor().pow(2)];
        let frame = fresh_in(&fb, &pics, Some(&pack), scale);
        assert!(
            block(&frame, scale, (x, y)) != classic,
            "{scale:?}: unflagged, the paper's block"
        );
        let off = Src {
            flags: FLAG_PAPER_OFF,
            ..named
        };
        fb.set_src(x, y, PAPER, off);
        {
            let frame = fresh_in(&fb, &pics, Some(&pack), scale);
            assert!(
                block(&frame, scale, (x, y)) == classic,
                "{scale:?}: flagged off-colour, the classic block"
            );
        }
        fb.set_src(x, y, PAPER, named);
    }
}

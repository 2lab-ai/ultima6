//! The MMPX port against the reference's vectors, and the compositor on synthetic frames and
//! images: the blocks it writes for each kind of pixel, a composition kept across changes equal to
//! a new one, the rect it reports, the bound of its cache. No game data.

use super::*;
use crate::{FLAG_OUTLINE, FLAG_OVERLAY};

/// One case of `mmpx-vectors.txt`: name, width, height, scale, input and expected output words.
type Vector = (String, usize, usize, Scale, Vec<u32>, Vec<u32>);

/// The vectors.
fn vectors() -> Vec<Vector> {
    let text = include_str!("mmpx-vectors.txt");
    let mut lines = text.lines().filter(|l| !l.starts_with('#')).peekable();
    let mut out = Vec::new();
    while let Some(head) = lines.next() {
        let f: Vec<&str> = head.split(' ').collect();
        assert_eq!(f[0], "case", "a case line");
        let (w, h): (usize, usize) = (f[2].parse().unwrap(), f[3].parse().unwrap());
        let scale = Scale::from_factor(f[4].parse().unwrap());
        let pal: Vec<u32> = lines
            .next()
            .unwrap()
            .split(' ')
            .skip(1)
            .map(|c| u32::from_str_radix(c, 16).unwrap())
            .collect();
        let digit = |c: char| pal[c.to_digit(36).unwrap() as usize];
        let rows = |lines: &mut std::iter::Peekable<_>, n: usize| -> Vec<u32> {
            (0..n)
                .flat_map(|_| {
                    let l: &str = Iterator::next(lines).unwrap();
                    l.chars().map(digit).collect::<Vec<_>>()
                })
                .collect()
        };
        let input = rows(&mut lines, h);
        assert_eq!(lines.next(), Some("="));
        let output = rows(&mut lines, h * scale.factor());
        assert_eq!(input.len(), w * h, "{}: the input", f[1]);
        assert_eq!(
            output.len(),
            w * h * scale.factor().pow(2),
            "{}: the output",
            f[1]
        );
        out.push((f[1].to_string(), w, h, scale, input, output));
    }
    out
}

#[test]
fn mmpx_matches_the_reference_vectors() {
    let cases = vectors();
    assert!(cases.len() >= 30, "{} cases", cases.len());
    let mut beyond_nearest = 0;
    for (name, w, h, scale, input, want) in &cases {
        let mut got = Vec::new();
        mmpx(input, *w, *h, *scale, &mut got);
        assert!(got == *want, "{name}: the reference's output");
        assert!(
            got.iter().all(|c| input.contains(c)),
            "{name}: no new colour"
        );
        let s = scale.factor();
        let nearest = |i: usize| input[(i / (w * s) / s) * w + (i % (w * s)) / s];
        beyond_nearest += usize::from((0..got.len()).any(|i| got[i] != nearest(i)));
    }
    assert!(
        beyond_nearest * 4 >= cases.len() * 3,
        "the vectors exercise the rules"
    );
}

#[test]
fn mmpx_of_an_empty_image_is_empty_and_4x_is_2x_twice() {
    let mut out = vec![7];
    mmpx2x(&[], 0, 3, &mut out);
    assert!(out.is_empty());
    let img: Vec<u32> = (0..48)
        .map(|i| [0xff00_0000, 0xffff_ffff, 0][(i * 7 / 5) % 3])
        .collect();
    let (mut a, mut b, mut c) = (Vec::new(), Vec::new(), Vec::new());
    mmpx2x(&img, 8, 6, &mut a);
    mmpx2x(&a, 16, 12, &mut b);
    mmpx(&img, 8, 6, Scale::X4, &mut c);
    assert_eq!(b, c);
}

/// A xorshift64 stream.
struct X(u64);

impl X {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// A synthetic image: kind, id, version, width, height, pixels, transparency.
type Pic = (Kind, u16, u32, usize, usize, Vec<u8>, Option<Vec<bool>>);

/// Synthetic images by (kind, id), and how often they were asked for.
#[derive(Clone, Default)]
struct Pics {
    images: Vec<Pic>,
    asked: usize,
}

impl Pics {
    fn put(
        &mut self,
        kind: Kind,
        id: u16,
        (w, h): (usize, usize),
        pixels: Vec<u8>,
        clear: Option<Vec<bool>>,
    ) {
        self.images.retain(|e| (e.0, e.1) != (kind, id));
        self.images.push((kind, id, 0, w, h, pixels, clear));
    }

    /// Image (kind, id) rewritten in place: its version moves.
    fn rewrite(&mut self, kind: Kind, id: u16, pixels: Vec<u8>) {
        let e = self
            .images
            .iter_mut()
            .find(|e| (e.0, e.1) == (kind, id))
            .unwrap();
        e.2 += 1;
        e.5 = pixels;
    }

    /// Image (kind, id) given transparency `clear` with its pixels and version kept (a change
    /// of the mask alone, which the compositor must not take for the same image).
    fn remask(&mut self, kind: Kind, id: u16, clear: Option<Vec<bool>>) {
        let e = self
            .images
            .iter_mut()
            .find(|e| (e.0, e.1) == (kind, id))
            .unwrap();
        e.6 = clear;
    }

    fn ver(&self, kind: Kind, id: u16) -> u32 {
        self.images
            .iter()
            .find(|e| (e.0, e.1) == (kind, id))
            .unwrap()
            .2
    }
}

impl Images for Pics {
    fn image(&mut self, kind: Kind, id: u16) -> Option<Image<'_>> {
        self.asked += 1;
        let e = self.images.iter().find(|e| (e.0, e.1) == (kind, id))?;
        Some(Image {
            ver: e.2,
            width: e.3,
            height: e.4,
            pixels: &e.5,
            transparent: e.6.as_deref(),
        })
    }
}

/// A `w` × `h` image of colours 1..`k` with runs (so MMPX changes it), seeded.
fn art(seed: u64, w: usize, h: usize, k: u64) -> Vec<u8> {
    let mut x = X(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut px = vec![0u8; w * h];
    for i in 0..w * h {
        px[i] = match x.below(10) {
            0..=3 if i % w > 0 => px[i - 1],
            4..=6 if i >= w => px[i - w],
            _ => 1 + x.below(k) as u8,
        };
    }
    px
}

/// A palette whose colours all differ.
fn palette(seed: u8) -> [Rgb; 256] {
    std::array::from_fn(|i| Rgb::new(i as u8, (i as u8).wrapping_mul(7) ^ seed, 255 - i as u8))
}

fn tile(id: u16, ver: u32, (u, v): (usize, usize), flags: u8) -> Src {
    Src {
        ver,
        id,
        u: u as u16,
        v: v as u16,
        kind: Kind::Tile,
        flags,
    }
}

/// Draws image (kind, id) of `pics` at (x, y), opaque pixels only, naming it at version `ver`.
fn draw(fb: &mut Framebuffer, pics: &Pics, kind: Kind, id: u16, (x, y): (usize, usize)) {
    let e = pics
        .images
        .iter()
        .find(|e| (e.0, e.1) == (kind, id))
        .unwrap();
    for v in 0..e.4 {
        for u in 0..e.3 {
            if e.6.as_ref().is_some_and(|t| t[v * e.3 + u]) {
                continue;
            }
            let src = Src {
                ver: e.2,
                id,
                u: u as u16,
                v: v as u16,
                kind,
                flags: 0,
            };
            fb.set_src(x + u, y + v, e.5[v * e.3 + u], src);
        }
    }
}

/// The MMPX ×S image of (kind, id) as the compositor makes it, transparent words 0.
fn hd_of(pics: &Pics, kind: Kind, id: u16, pal: &[Rgb; 256], scale: Scale) -> (usize, Vec<u32>) {
    let e = pics
        .images
        .iter()
        .find(|e| (e.0, e.1) == (kind, id))
        .unwrap();
    let input: Vec<u32> = (0..e.3 * e.4)
        .map(|i| match &e.6 {
            Some(t) if t[i] => 0,
            _ => word(pal[usize::from(e.5[i])]),
        })
        .collect();
    let mut hd = Vec::new();
    mmpx(&input, e.3, e.4, scale, &mut hd);
    (e.3 * scale.factor(), hd)
}

/// The S × S block of screen pixel (x, y) in `out`.
fn block(out: &[u32], scale: Scale, (x, y): (usize, usize)) -> Vec<u32> {
    let s = scale.factor();
    let ow = WIDTH * s;
    (0..s * s)
        .map(|k| out[(y * s + k / s) * ow + x * s + k % s])
        .collect()
}

fn fresh(fb: &Framebuffer, pics: &Pics, scale: Scale) -> Vec<u32> {
    let mut c = Compositor::new();
    c.compose(fb, &mut pics.clone(), scale);
    c.rgba().to_vec()
}

fn plane() -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.enable_src();
    fb
}

#[test]
fn a_frame_without_a_plane_or_without_images_is_its_classic_pixels_scaled() {
    for scale in [Scale::X2, Scale::X4] {
        let mut fb = Framebuffer::new();
        fb.palette = palette(3);
        for (i, p) in art(1, WIDTH, HEIGHT, 30).into_iter().enumerate() {
            fb.set(i % WIDTH, i / WIDTH, p);
        }
        let mut rgba = Vec::new();
        fb.write_rgba(&mut rgba);
        let mut c = Compositor::new();
        c.compose(&fb, &mut NoImages, scale);
        let s = scale.factor();
        assert_eq!(c.rgba().len(), WIDTH * s * HEIGHT * s);
        let nearest = (0..c.rgba().len()).all(|i| {
            let (x, y) = (i % (WIDTH * s) / s, i / (WIDTH * s) / s);
            c.rgba()[i].to_le_bytes() == rgba[(y * WIDTH + x) * 4..][..4]
        });
        assert!(nearest, "{s}x: the classic RGBA, each pixel S x S");
        // With a plane naming tiles the images do not have, the same.
        let mut named = fb.clone();
        named.enable_src();
        for i in 0..WIDTH * HEIGHT {
            let p = named.pixels()[i];
            named.set_src(i % WIDTH, i / WIDTH, p, tile((i % 7) as u16, 0, (0, 0), 0));
        }
        let mut d = Compositor::new();
        d.compose(&named, &mut NoImages, scale);
        assert!(d.rgba() == c.rgba(), "{s}x: images missing: classic");
    }
}

/// The index under a test tile's transparent pixels: its colour appears nowhere else.
const UNDER_CLEAR: u8 = 200;

#[test]
fn a_current_unflagged_pixel_shows_its_block_of_the_hd_image() {
    let mut pics = Pics::default();
    // Transparent outside a diamond (MMPX rounds its edges into the transparency) and on a
    // diagonal lattice inside it.
    let mut clear = vec![false; 256];
    for (i, c) in clear.iter_mut().enumerate() {
        let (r, col) = ((i / 16) as i32, (i % 16) as i32);
        *c = (2 * r - 15).abs() + (2 * col - 15).abs() > 16 || (r + col) % 9 == 0;
    }
    // Under each transparent pixel the record holds a colour of its own, which must never show.
    let mut pixels = art(5, 16, 16, 4);
    for (p, &c) in pixels.iter_mut().zip(&clear) {
        if c {
            *p = UNDER_CLEAR;
        }
    }
    pics.put(Kind::Tile, 5, (16, 16), pixels, Some(clear.clone()));
    pics.put(Kind::Portrait, 0x6103, (56, 64), art(6, 56, 64, 6), None);
    pics.put(Kind::Bitmap, 1, (40, 30), art(7, 40, 30, 5), None);
    for scale in [Scale::X2, Scale::X4] {
        let mut fb = plane();
        fb.palette = palette(9);
        fb.clear(31);
        draw(&mut fb, &pics, Kind::Bitmap, 1, (100, 100));
        draw(&mut fb, &pics, Kind::Portrait, 0x6103, (200, 20));
        draw(&mut fb, &pics, Kind::Tile, 5, (16, 16));
        let mut c = Compositor::new();
        c.compose(&fb, &mut pics.clone(), scale);
        let s = scale.factor();
        let (mut hd_blocks, mut mixed) = (0, 0);
        for (kind, id, at, (w, h)) in [
            (Kind::Tile, 5, (16, 16), (16, 16)),
            (Kind::Portrait, 0x6103, (200, 20), (56, 64)),
            (Kind::Bitmap, 1, (100, 100), (40, 30)),
        ] {
            let (hw, hd) = hd_of(&pics, kind, id, &fb.palette, scale);
            for v in 0..h {
                for u in 0..w {
                    let (x, y) = (at.0 + u, at.1 + v);
                    let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
                    let got = block(c.rgba(), scale, (x, y));
                    if fb.src_at(x, y).unwrap().kind == Kind::Classic {
                        assert!(
                            got.iter().all(|&g| g == classic),
                            "a transparent tile pixel: what is under it"
                        );
                        continue;
                    }
                    let raw: Vec<u32> = (0..s * s)
                        .map(|k| hd[(v * s + k / s) * hw + u * s + k % s])
                        .collect();
                    let want: Vec<u32> = raw
                        .iter()
                        .map(|&c| if c == 0 { classic } else { c })
                        .collect();
                    assert!(got == want, "{kind:?} ({u}, {v}): its block");
                    hd_blocks += usize::from(got.iter().any(|&g| g != classic));
                    // A block that takes the classic colour where MMPX left transparency and
                    // the HD words elsewhere.
                    let fallback = raw.iter().zip(&got).any(|(&r, &g)| r == 0 && g == classic);
                    let opaque = raw.iter().zip(&got).any(|(&r, &g)| r != 0 && g == r);
                    mixed += usize::from(fallback && opaque);
                }
            }
        }
        assert!(hd_blocks > 50, "{s}x: MMPX changed blocks");
        assert!(
            mixed > 0,
            "{s}x: a block with both transparent fallback and HD sub-pixels"
        );
        let under = word(fb.palette[usize::from(UNDER_CLEAR)]);
        assert!(
            fb.pixels().iter().all(|&p| p != UNDER_CLEAR),
            "the colour under the transparency is on no screen pixel"
        );
        assert!(
            c.rgba().iter().all(|&g| g != under),
            "{s}x: and in no HD block"
        );
    }
}

/// Tile pixels whose block at `scale` is not their classic colour S × S (MMPX changed it), in
/// raster order.
fn reshaped(
    c: &Compositor,
    fb: &Framebuffer,
    scale: Scale,
    area: (usize, usize, usize, usize),
) -> Vec<(usize, usize)> {
    let (x0, y0, w, h) = area;
    (y0..y0 + h)
        .flat_map(|y| (x0..x0 + w).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let classic = word(fb.palette[usize::from(fb.get(x, y).unwrap())]);
            block(c.rgba(), scale, (x, y)).iter().any(|&g| g != classic)
        })
        .collect()
}

/// A labelled change of a pixel's source.
type Change = (&'static str, fn(Src) -> Src);

/// Each kind of pixel shown classic, one at a time at a pixel whose block is HD before: the
/// overlay and outline flags, a version not current, u or v past the image, an image the images
/// do not have, a classic source. The colour stays; only that block changes, to the classic
/// colour.
#[test]
fn classic_flagged_stale_and_unknown_pixels_are_their_classic_colour() {
    let mut pics = Pics::default();
    pics.put(Kind::Tile, 2, (16, 16), art(2, 16, 16, 3), None);
    let mut base = plane();
    base.palette = palette(1);
    base.clear(12);
    draw(&mut base, &pics, Kind::Tile, 2, (32, 32));
    let cases: [Change; 8] = [
        ("overlay", |s| Src {
            flags: FLAG_OVERLAY,
            ..s
        }),
        ("outline", |s| Src {
            flags: FLAG_OUTLINE,
            ..s
        }),
        ("stale", |s| Src {
            ver: s.ver + 1,
            ..s
        }),
        ("u past the image", |s| Src { u: 16, ..s }),
        ("v past the image", |s| Src { v: 16, ..s }),
        ("both past", |s| Src { u: 16, v: 16, ..s }),
        ("an unknown image", |s| Src { id: 9, ..s }),
        ("classic", |_| Src::CLASSIC),
    ];
    for scale in [Scale::X2, Scale::X4] {
        let mut c = Compositor::new();
        c.compose(&base, &mut pics.clone(), scale);
        let before = c.rgba().to_vec();
        let hd = reshaped(&c, &base, scale, (32, 32, 16, 16));
        assert!(hd.len() >= cases.len(), "{scale:?}: enough HD blocks");
        for ((what, change), &(x, y)) in cases.iter().zip(&hd) {
            let classic = word(base.palette[usize::from(base.get(x, y).unwrap())]);
            assert!(
                block(&before, scale, (x, y)).iter().any(|&g| g != classic),
                "{what}: HD before"
            );
            let mut fb = base.clone();
            let src = change(fb.src_at(x, y).unwrap());
            let p = fb.get(x, y).unwrap();
            fb.set_src(x, y, p, src);
            let mut kept = Compositor::new();
            kept.compose(&base, &mut pics.clone(), scale);
            kept.take_dirty();
            kept.compose(&fb, &mut pics.clone(), scale);
            assert_eq!(
                kept.take_dirty(),
                Some([x, y, 1, 1]),
                "{what}: only its block"
            );
            for got in [kept.rgba(), fresh(&fb, &pics, scale).as_slice()] {
                assert!(
                    block(got, scale, (x, y)).iter().all(|&g| g == classic),
                    "{what} ({x}, {y}): classic"
                );
                let (s, ow) = (scale.factor(), WIDTH * scale.factor());
                let elsewhere = (0..got.len())
                    .filter(|&i| (i % ow / s, i / ow / s) != (x, y))
                    .all(|i| got[i] == before[i]);
                assert!(elsewhere, "{what}: the other blocks as before");
            }
        }
    }
}

/// Changes one at a time over a kept compositor, each compared with a new compositor's frame:
/// a pixel's source alone flagged then current again (its colour the same), the record
/// rewritten with no pixel drawn (every pixel of it stale, classic) then drawn again at its new
/// version, and a pixel naming a valid column but a row past the image.
#[test]
fn isolated_transitions_change_exactly_their_blocks() {
    let mut pics = Pics::default();
    pics.put(Kind::Tile, 2, (16, 16), art(2, 16, 16, 3), None);
    let mut fb = plane();
    fb.palette = palette(4);
    fb.clear(12);
    draw(&mut fb, &pics, Kind::Tile, 2, (64, 48));
    for scale in [Scale::X2, Scale::X4] {
        let mut c = Compositor::new();
        let mut fb = fb.clone();
        c.compose(&fb, &mut pics.clone(), scale);
        c.take_dirty();
        let first = c.rgba().to_vec();
        let (x, y) = reshaped(&c, &fb, scale, (64, 48, 16, 16))[0];
        let current = fb.src_at(x, y).unwrap();
        let p = fb.get(x, y).unwrap();
        let classic = word(fb.palette[usize::from(p)]);
        let step = |c: &mut Compositor, fb: &Framebuffer, pics: &Pics, what: &str| {
            c.compose(fb, &mut pics.clone(), scale);
            assert!(
                c.rgba() == fresh(fb, pics, scale).as_slice(),
                "{what}: a new compositor's frame"
            );
            c.take_dirty()
        };
        fb.set_src(
            x,
            y,
            p,
            Src {
                flags: FLAG_OVERLAY,
                ..current
            },
        );
        assert_eq!(step(&mut c, &fb, &pics, "flagged"), Some([x, y, 1, 1]));
        assert!(block(c.rgba(), scale, (x, y)).iter().all(|&g| g == classic));
        fb.set_src(x, y, p, current);
        assert_eq!(
            step(&mut c, &fb, &pics, "current again"),
            Some([x, y, 1, 1])
        );
        assert!(c.rgba() == first.as_slice(), "the first frame again");
        // The record rewritten in place: nothing drawn, every pixel of it now stale.
        let mut pics = pics.clone();
        pics.rewrite(Kind::Tile, 2, art(77, 16, 16, 3));
        assert_eq!(step(&mut c, &fb, &pics, "stale"), Some([64, 48, 16, 16]));
        let rgba = fb_rgba(&fb, scale);
        assert!(
            c.rgba() == rgba.as_slice(),
            "stale: the classic frame scaled"
        );
        draw(&mut fb, &pics, Kind::Tile, 2, (64, 48));
        assert_eq!(
            step(&mut c, &fb, &pics, "drawn again"),
            Some([64, 48, 16, 16])
        );
        let (hw, hd) = hd_of(&pics, Kind::Tile, 2, &fb.palette, scale);
        let s = scale.factor();
        let want: Vec<u32> = (0..s * s)
            .map(|k| hd[(3 * s + k / s) * hw + 5 * s + k % s])
            .collect();
        assert_eq!(
            block(c.rgba(), scale, (69, 51)),
            want,
            "the new version's block"
        );
        // A valid column, a row past the image.
        let (x, y) = (70, 50);
        let src = fb.src_at(x, y).unwrap();
        let p = fb.get(x, y).unwrap();
        fb.set_src(x, y, p, Src { v: 16, ..src });
        assert_eq!(step(&mut c, &fb, &pics, "v past"), Some([x, y, 1, 1]));
        let classic = word(fb.palette[usize::from(p)]);
        assert!(block(c.rgba(), scale, (x, y)).iter().all(|&g| g == classic));
    }
}

/// `fb`'s classic colours, each S × S.
fn fb_rgba(fb: &Framebuffer, scale: Scale) -> Vec<u32> {
    let s = scale.factor();
    (0..WIDTH * s * HEIGHT * s)
        .map(|i| {
            let (x, y) = (i % (WIDTH * s) / s, i / (WIDTH * s) / s);
            word(fb.palette[usize::from(fb.get(x, y).unwrap())])
        })
        .collect()
}

/// Literal goldens: a 4 × 4 tile (a staircase of three colours, one transparent corner) over a
/// background, composed at 2× and 4×. The expected bytes (r, g, b, a per sub-pixel) were made by
/// the MMPX reference C++ on the tile's words (the corner alpha 0), with the transparent pixel
/// showing the background and every other sub-pixel the reference's word, by a separate script:
/// none of this crate's code made them.
#[test]
fn a_tile_composes_to_the_reference_bytes() {
    const X2: [&str; 8] = [
        "102030ff102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0ff",
        "102030ff102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0ff",
        "102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0ff",
        "102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff",
        "102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0ff800040ff800040ff",
        "102030ff102030fff0e0d0fff0e0d0fff0e0d0ff800040ff800040ff800040ff",
        "006600ff006600fff0e0d0fff0e0d0ff800040ff800040ff800040ff800040ff",
        "006600ff006600fff0e0d0fff0e0d0ff800040ff800040ff800040ff800040ff",
    ];
    const X4: [&str; 16] = [
        "102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff",
        "102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff",
        "102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff",
        "102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff",
        "102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff",
        "102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff",
        "102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff",
        "102030ff102030ff102030ff102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff",
        "102030ff102030ff102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff800040ff800040ff800040ff800040ff",
        "102030ff102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff800040ff800040ff800040ff800040ff800040ff",
        "102030ff102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff800040ff800040ff800040ff800040ff800040ff800040ff",
        "102030ff102030ff102030ff102030fff0e0d0fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff800040ff800040ff800040ff800040ff800040ff800040ff800040ff",
        "006600ff006600ff006600ff006600fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff800040ff800040ff800040ff800040ff800040ff800040ff800040ff800040ff",
        "006600ff006600ff006600ff006600fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff800040ff800040ff800040ff800040ff800040ff800040ff800040ff800040ff",
        "006600ff006600ff006600ff006600fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff800040ff800040ff800040ff800040ff800040ff800040ff800040ff800040ff",
        "006600ff006600ff006600ff006600fff0e0d0fff0e0d0fff0e0d0fff0e0d0ff800040ff800040ff800040ff800040ff800040ff800040ff800040ff800040ff",
    ];
    let mut pics = Pics::default();
    let (t, a, b, c) = (0u8, 1u8, 2u8, 3u8);
    let pixels = vec![a, a, a, b, a, a, b, b, a, b, b, c, t, b, c, c];
    let clear: Vec<bool> = pixels.iter().map(|&p| p == t).collect();
    pics.put(Kind::Tile, 1, (4, 4), pixels, Some(clear));
    let mut fb = plane();
    fb.palette[1] = Rgb::new(0x10, 0x20, 0x30);
    fb.palette[2] = Rgb::new(0xf0, 0xe0, 0xd0);
    fb.palette[3] = Rgb::new(0x80, 0x00, 0x40);
    fb.palette[4] = Rgb::new(0x00, 0x66, 0x00);
    fb.clear(4);
    draw(&mut fb, &pics, Kind::Tile, 1, (10, 10));
    for (scale, golden) in [(Scale::X2, &X2[..]), (Scale::X4, &X4[..])] {
        let mut comp = Compositor::new();
        comp.compose(&fb, &mut pics.clone(), scale);
        let s = scale.factor();
        let ow = WIDTH * s;
        for (r, row) in golden.iter().enumerate() {
            let got: String = (0..4 * s)
                .flat_map(|k| comp.rgba()[(10 * s + r) * ow + 10 * s + k].to_le_bytes())
                .map(|b| format!("{b:02x}"))
                .collect();
            assert_eq!(got, *row, "{s}x: row {r}");
        }
    }
}

/// Random edits between compositions (pixels drawn from images and classic ones, an image
/// rewritten, an image's transparency alone changed (pixels, version and palette kept), the
/// palette changed, the scale switched, the plane reset): after each, the kept
/// compositor's frame is a new compositor's, and every block that changed lies in the rect it
/// reported.
#[test]
fn a_kept_compositor_equals_a_new_one_after_every_change() {
    let mut pics = Pics::default();
    for id in 0..12u16 {
        let clear: Vec<bool> = (0..256)
            .map(|i| (i * (id as usize + 3)).is_multiple_of(11))
            .collect();
        pics.put(
            Kind::Tile,
            id,
            (16, 16),
            art(u64::from(id), 16, 16, 3 + u64::from(id % 4)),
            Some(clear),
        );
    }
    pics.put(Kind::Portrait, 0x7a01, (56, 64), art(99, 56, 64, 5), None);
    let mut fb = plane();
    fb.palette = palette(0);
    let mut x = X(0x1234_5678_9abc_def1);
    let mut c = Compositor::new();
    let mut scale = Scale::X2;
    let mut composed = 0;
    for step in 0..120 {
        for _ in 0..x.below(6) {
            let (px, py) = (
                x.below(WIDTH as u64 - 16) as usize,
                x.below(HEIGHT as u64 - 16) as usize,
            );
            match x.below(11) {
                10 => {
                    let id = x.below(12) as u16;
                    let k = 2 + x.below(9) as usize;
                    let clear = (0..256).map(|i| (i * k).is_multiple_of(13)).collect();
                    pics.remask(Kind::Tile, id, Some(clear));
                }
                0..=4 => draw(&mut fb, &pics, Kind::Tile, x.below(12) as u16, (px, py)),
                5 => draw(
                    &mut fb,
                    &pics,
                    Kind::Portrait,
                    0x7a01,
                    (px.min(WIDTH - 56), py.min(HEIGHT - 64)),
                ),
                6 => fb.blit(px as i32, py as i32, 5, 3, &[17; 15], None),
                7 => {
                    let id = x.below(12) as u16;
                    pics.rewrite(Kind::Tile, id, art(1000 + step, 16, 16, 4));
                }
                8 => {
                    let i = x.below(256) as usize;
                    fb.palette[i] = Rgb::new(x.below(256) as u8, 3, 9);
                }
                _ => {
                    let p = fb.get(px, py).unwrap();
                    fb.set_src(
                        px,
                        py,
                        p,
                        tile(3, pics.ver(Kind::Tile, 3), (2, 2), FLAG_OVERLAY),
                    );
                }
            }
        }
        match x.below(20) {
            0 => {
                scale = if scale == Scale::X2 {
                    Scale::X4
                } else {
                    Scale::X2
                }
            }
            1 => fb.reset_src(),
            _ => {}
        }
        let before = c.rgba().to_vec();
        let same_scale = c.scale() == scale && !before.is_empty();
        c.compose(&fb, &mut pics, scale);
        composed += 1;
        let want = fresh(&fb, &pics, scale);
        assert!(
            c.rgba() == want.as_slice(),
            "step {step}: the kept compositor's frame"
        );
        let rect = c.take_dirty();
        if same_scale {
            let s = scale.factor();
            for i in (0..want.len()).filter(|&i| want[i] != before[i]) {
                let (bx, by) = (i % (WIDTH * s) / s, i / (WIDTH * s) / s);
                let [x0, y0, w, h] = rect.expect("a rect for a change");
                assert!(
                    (x0..x0 + w).contains(&bx) && (y0..y0 + h).contains(&by),
                    "step {step}: the change in the rect"
                );
            }
        }
        assert_eq!(c.take_dirty(), None, "a rect is taken once");
    }
    assert_eq!(composed, 120);
}

#[test]
fn an_unchanged_frame_writes_nothing_and_asks_for_each_image_once() {
    let mut pics = Pics::default();
    pics.put(Kind::Tile, 1, (16, 16), art(1, 16, 16, 4), None);
    pics.put(Kind::Tile, 2, (16, 16), art(2, 16, 16, 4), None);
    let mut fb = plane();
    fb.palette = palette(4);
    for k in 0..40 {
        draw(
            &mut fb,
            &pics,
            Kind::Tile,
            1 + (k % 2) as u16,
            (k % 20 * 16, k / 20 * 16),
        );
    }
    let mut c = Compositor::new();
    c.compose(&fb, &mut pics, Scale::X4);
    assert_eq!(pics.asked, 2, "one ask per image shown");
    assert_eq!(
        c.take_dirty(),
        Some([0, 0, WIDTH, HEIGHT]),
        "the first composition writes all"
    );
    assert_eq!(c.cache().made(), 2);
    c.compose(&fb, &mut pics, Scale::X4);
    assert_eq!(c.take_dirty(), None, "the same frame writes nothing");
    assert_eq!(
        (pics.asked, c.cache().made()),
        (4, 2),
        "asked again, nothing made"
    );
    fb.set(100, 150, 3);
    c.compose(&fb, &mut pics, Scale::X4);
    assert_eq!(c.take_dirty(), Some([100, 150, 1, 1]));
    // The same pixels drawn as another record with equal content share one HD image.
    pics.put(Kind::Tile, 3, (16, 16), art(1, 16, 16, 4), None);
    draw(&mut fb, &pics, Kind::Tile, 3, (0, 100));
    c.compose(&fb, &mut pics, Scale::X4);
    assert_eq!(c.cache().made(), 2, "equal content, one HD image");
    assert_eq!(
        c.take_dirty(),
        Some([0, 100, 16, 16]),
        "the record changed under the pixels"
    );
}

#[test]
fn a_palette_change_recolours_the_hd_blocks_exactly() {
    let mut pics = Pics::default();
    pics.put(Kind::Tile, 4, (16, 16), art(4, 16, 16, 5), None);
    let mut fb = plane();
    fb.palette = palette(2);
    draw(&mut fb, &pics, Kind::Tile, 4, (48, 48));
    let mut c = Compositor::new();
    c.compose(&fb, &mut pics, Scale::X2);
    // A fade step: every colour halved.
    for p in fb.palette.iter_mut() {
        *p = Rgb::new(p.r / 2, p.g / 2, p.b / 2);
    }
    c.compose(&fb, &mut pics, Scale::X2);
    assert!(c.rgba() == fresh(&fb, &pics, Scale::X2).as_slice());
    let (hw, hd) = hd_of(&pics, Kind::Tile, 4, &fb.palette, Scale::X2);
    for v in 0..16 {
        for u in 0..16 {
            let want: Vec<u32> = (0..4)
                .map(|k| hd[(v * 2 + k / 2) * hw + u * 2 + k % 2])
                .collect();
            assert_eq!(block(c.rgba(), Scale::X2, (48 + u, 48 + v)), want);
        }
    }
    assert_eq!(c.cache().made(), 2, "the faded tile is another HD image");
}

#[test]
fn the_cache_stays_under_its_bound_and_the_frame_is_the_same() {
    let mut pics = Pics::default();
    for id in 0..60u16 {
        pics.put(
            Kind::Tile,
            id,
            (16, 16),
            art(500 + u64::from(id), 16, 16, 4),
            None,
        );
    }
    // Room for about 10 tiles at 4x (input 1 KiB, output 16 KiB each).
    let cap = 10 * 17 * 1024 + 100;
    let mut small = Compositor::with_cap(cap);
    let mut big = Compositor::new();
    let mut fb = plane();
    fb.palette = palette(5);
    let mut x = X(77);
    let mut over = 0;
    for step in 0..40 {
        for _ in 0..1 + x.below(30) {
            let id = x.below(60) as u16;
            let at = (x.below(20) as usize * 16, x.below(12) as usize * 16);
            draw(&mut fb, &pics, Kind::Tile, id, at);
        }
        small.compose(&fb, &mut pics, Scale::X4);
        big.compose(&fb, &mut pics, Scale::X4);
        assert!(small.rgba() == big.rgba(), "step {step}: the same frame");
        assert!(small.cache().bytes() <= cap, "step {step}: under the bound");
        over += usize::from(big.cache().bytes() > cap);
    }
    assert!(over > 0, "the screens needed more than the bound");
    assert!(
        small.cache().made() > big.cache().made(),
        "evicted images were made again"
    );
    assert!(big.cache().bytes() <= HD_CACHE_BYTES && !big.cache().is_empty());
}

/// Every image hashed alike (a forced collision): each still gets its own HD image, by its
/// content and its shape; the same flattened words as 2 × 8 and as 4 × 4 are two images; the
/// frames are those of the real hash.
#[test]
fn colliding_hashes_and_equal_words_of_other_shapes_keep_their_own_images() {
    let words = art(31, 4, 4, 4);
    let mut pics = Pics::default();
    pics.put(Kind::Tile, 1, (2, 8), words.clone(), None);
    pics.put(Kind::Tile, 2, (4, 4), words, None);
    for id in 3..9u16 {
        pics.put(
            Kind::Tile,
            id,
            (16, 16),
            art(u64::from(id), 16, 16, 4),
            None,
        );
    }
    let mut fb = plane();
    fb.palette = palette(6);
    draw(&mut fb, &pics, Kind::Tile, 1, (0, 0));
    draw(&mut fb, &pics, Kind::Tile, 2, (8, 0));
    for id in 3..9u16 {
        draw(&mut fb, &pics, Kind::Tile, id, (16 * usize::from(id), 32));
    }
    for scale in [Scale::X2, Scale::X4] {
        let mut collide =
            Compositor::with_cache(HdImages::with_hash(HD_CACHE_BYTES, |_, _, _, _| 7));
        collide.compose(&fb, &mut pics.clone(), scale);
        assert_eq!(collide.cache().len(), 8, "one image each");
        assert!(
            collide.rgba() == fresh(&fb, &pics, scale).as_slice(),
            "the real hash's frame"
        );
        // And by the definition, block by block, for the two shapes.
        let s = scale.factor();
        for (id, at, (w, h)) in [(1u16, (0, 0), (2, 8)), (2, (8, 0), (4, 4))] {
            let (hw, hd) = hd_of(&pics, Kind::Tile, id, &fb.palette, scale);
            for v in 0..h {
                for u in 0..w {
                    let want: Vec<u32> = (0..s * s)
                        .map(|k| hd[(v * s + k / s) * hw + u * s + k % s])
                        .collect();
                    assert_eq!(block(collide.rgba(), scale, (at.0 + u, at.1 + v)), want);
                }
            }
        }
        let (_, two) = hd_of(&pics, Kind::Tile, 1, &fb.palette, scale);
        let (_, four) = hd_of(&pics, Kind::Tile, 2, &fb.palette, scale);
        assert!(two != four, "the shapes give other HD images");
        // Composed again: every image found again among the colliding ones, none made again.
        let made = collide.cache().made();
        collide.forget();
        collide.compose(&fb, &mut pics.clone(), scale);
        assert_eq!(collide.cache().made(), made);
    }
}

/// A bound smaller than one image: each composition makes what it shows and the cache is empty
/// after it; the frame is the same. The bytes counted are the words in and out and the indices
/// the image was reached from.
#[test]
fn a_bound_smaller_than_one_image_and_the_bytes_counted() {
    let mut pics = Pics::default();
    let clear: Vec<bool> = (0..256).map(|i| i % 5 == 0).collect();
    pics.put(Kind::Tile, 1, (16, 16), art(1, 16, 16, 4), Some(clear));
    pics.put(Kind::Bitmap, 0, (40, 30), art(2, 40, 30, 4), None);
    let mut fb = plane();
    fb.palette = palette(2);
    draw(&mut fb, &pics, Kind::Tile, 1, (16, 16));
    draw(&mut fb, &pics, Kind::Bitmap, 0, (100, 100));
    let mut tiny = Compositor::with_cap(100);
    for scale in [Scale::X2, Scale::X4, Scale::X2] {
        tiny.compose(&fb, &mut pics.clone(), scale);
        assert!(tiny.rgba() == fresh(&fb, &pics, scale).as_slice());
        assert_eq!(tiny.cache().bytes(), 0, "nothing kept");
        assert!(
            tiny.cache().peak() > tiny.cache().cap(),
            "the composition held what it showed"
        );
    }
    let mut c = Compositor::new();
    c.compose(&fb, &mut pics.clone(), Scale::X2);
    // The tile: 256 words in, 1024 out, 256 indices and 256 transparency flags; the bitmap:
    // 1200 words in, 4800 out, 1200 indices.
    let want = (256 + 1024) * 4 + 256 + 256 + (1200 + 4800) * 4 + 1200;
    assert_eq!(c.cache().bytes(), want);
    assert_eq!(c.cache().peak(), want);
}

/// The transparency alone changed (the pixels, the version and the palette the same): a new HD
/// image, not the one the memo holds; equal to a new compositor's frame at each step, and
/// different from the frame before.
#[test]
fn a_change_of_the_transparency_alone_is_another_image() {
    let mut pics = Pics::default();
    let pixels = art(12, 16, 16, 4);
    pics.put(Kind::Tile, 4, (16, 16), pixels, None);
    let mut fb = plane();
    fb.palette = palette(7);
    draw(&mut fb, &pics, Kind::Tile, 4, (32, 32));
    for scale in [Scale::X2, Scale::X4] {
        let mut c = Compositor::new();
        let mut pics = pics.clone();
        c.compose(&fb, &mut pics, scale);
        let mut last = c.rgba().to_vec();
        for (k, mask) in [
            Some(
                (0..256)
                    .map(|i| (i / 16 + i % 16) % 4 == 0)
                    .collect::<Vec<_>>(),
            ),
            Some(vec![false; 256]),
            Some((0..256).map(|i| i % 3 == 0).collect()),
            None,
        ]
        .into_iter()
        .enumerate()
        {
            let changes = mask.as_ref().is_some_and(|m| m.iter().any(|&t| t));
            pics.remask(Kind::Tile, 4, mask);
            c.compose(&fb, &mut pics, scale);
            let want = fresh(&fb, &pics, scale);
            assert!(
                c.rgba() == want.as_slice(),
                "{scale:?} mask {k}: a new compositor's frame"
            );
            if changes {
                assert!(
                    c.rgba() != last.as_slice(),
                    "{scale:?} mask {k}: the frame changed"
                );
            }
            last = c.rgba().to_vec();
        }
    }
}

/// One HD image reached from both shapes of an opaque mask (`None`, and `Some` of all false:
/// the same words, so the same entry), alternately and in one composition: the bytes counted
/// are always the sum of the entries', with the source last reached counted.
#[test]
fn an_image_reached_from_another_mask_shape_is_counted_again() {
    let mut pics = Pics::default();
    let pixels = art(21, 16, 16, 4);
    pics.put(Kind::Tile, 1, (16, 16), pixels.clone(), None);
    pics.put(Kind::Tile, 2, (16, 16), pixels, Some(vec![false; 256]));
    let (none, some) = ((256 + 1024) * 4 + 256, (256 + 1024) * 4 + 256 + 256);
    for (cells, want) in [
        (&[1u16][..], none),
        (&[2][..], some),
        (&[1][..], none),
        (&[1, 2][..], some),
        (&[2, 1][..], none),
    ] {
        let mut fb = plane();
        fb.palette = palette(3);
        for (k, &id) in cells.iter().enumerate() {
            draw(&mut fb, &pics, Kind::Tile, id, (16 * k, 0));
        }
        let mut c = Compositor::new();
        c.compose(&fb, &mut pics.clone(), Scale::X2);
        assert_eq!(c.cache().len(), 1, "{cells:?}: one entry");
        assert_eq!(
            c.cache().bytes(),
            c.cache().entries_bytes(),
            "{cells:?}: the sum"
        );
        assert_eq!(
            c.cache().bytes(),
            want,
            "{cells:?}: the last reached counted"
        );
    }
    // One compositor across compositions that alternate the shapes.
    let mut c = Compositor::new();
    for k in 0..6 {
        let id = 1 + (k % 2) as u16;
        let mut fb = plane();
        fb.palette = palette(3);
        draw(&mut fb, &pics, Kind::Tile, id, (0, 0));
        c.compose(&fb, &mut pics.clone(), Scale::X2);
        assert_eq!(
            c.cache().bytes(),
            c.cache().entries_bytes(),
            "composition {k}: the sum"
        );
        assert_eq!(
            c.cache().bytes(),
            if id == 1 { none } else { some },
            "composition {k}"
        );
    }
}

#[test]
#[should_panic(expected = "an image past")]
fn mmpx_refuses_an_image_past_twice_the_screen() {
    mmpx2x(&[], 2 * WIDTH + 1, 1, &mut Vec::new());
}

#[test]
#[should_panic(expected = "an image past the screen")]
fn mmpx_refuses_an_image_past_the_screen() {
    mmpx(&[], WIDTH, HEIGHT + 1, Scale::X4, &mut Vec::new());
}

#[test]
fn an_image_past_the_screen_is_shown_classic() {
    let mut pics = Pics::default();
    pics.put(
        Kind::Bitmap,
        0,
        (WIDTH + 1, 1),
        art(3, WIDTH + 1, 1, 4),
        None,
    );
    let mut fb = plane();
    fb.palette = palette(8);
    for x in 0..WIDTH {
        fb.set_src(
            x,
            0,
            1 + (x % 3) as u8,
            Src {
                u: x as u16,
                kind: Kind::Bitmap,
                ..Src::CLASSIC
            },
        );
    }
    let mut c = Compositor::new();
    c.compose(&fb, &mut pics, Scale::X4);
    assert!(c.rgba() == fb_rgba(&fb, Scale::X4).as_slice());
    assert_eq!(c.cache().made(), 0);
}

/// Timings only (run with `--ignored --nocapture`): a walk-like scene, the paper bitmap under a
/// map window of 11 × 11 cells of 300 synthetic records that scrolls one cell each composition.
#[test]
#[ignore]
fn bench_a_walk() {
    let mut pics = Pics::default();
    for id in 0..300u16 {
        let clear: Vec<bool> = (0..256)
            .map(|i| id % 3 == 0 && (i % 16 + i / 16) % 5 == 0)
            .collect();
        pics.put(
            Kind::Tile,
            id,
            (16, 16),
            art(u64::from(id), 16, 16, 6),
            Some(clear),
        );
    }
    pics.put(Kind::Bitmap, 0, (320, 200), art(4242, 320, 200, 8), None);
    for scale in [Scale::X4, Scale::X2] {
        let mut fb = plane();
        fb.palette = palette(1);
        draw(&mut fb, &pics, Kind::Bitmap, 0, (0, 0));
        let mut c = Compositor::new();
        let t = std::time::Instant::now();
        c.compose(&fb, &mut pics, scale);
        let cold = t.elapsed();
        let mut times = Vec::new();
        for step in 0..200usize {
            for cy in 0..11 {
                for cx in 0..11 {
                    let id = ((cx + step) * 7 + cy * 13) % 300;
                    draw(
                        &mut fb,
                        &pics,
                        Kind::Tile,
                        id as u16,
                        (8 + cx * 16, 8 + cy * 16),
                    );
                }
            }
            let t = std::time::Instant::now();
            c.compose(&fb, &mut pics, scale);
            times.push(t.elapsed().as_secs_f64() * 1e3);
        }
        times.sort_by(f64::total_cmp);
        let mut full = Compositor::new();
        full.compose(&fb, &mut pics, scale);
        let t = std::time::Instant::now();
        full.forget();
        full.compose(&fb, &mut pics, scale);
        let whole = t.elapsed();
        println!(
            "{}x: cold {:.2} ms, walk median {:.3} ms p95 {:.3} ms, whole frame (warm cache) {:.3} ms, cache {} images {} KiB",
            scale.factor(),
            cold.as_secs_f64() * 1e3,
            times[100],
            times[190],
            whole.as_secs_f64() * 1e3,
            c.cache().len(),
            c.cache().bytes() >> 10
        );
    }
}

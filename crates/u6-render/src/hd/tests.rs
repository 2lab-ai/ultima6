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

#[test]
fn a_current_unflagged_pixel_shows_its_block_of_the_hd_image() {
    let mut pics = Pics::default();
    let mut clear = vec![false; 256];
    for (i, c) in clear.iter_mut().enumerate() {
        *c = (i / 16 + i % 16) % 9 == 0; // a diagonal lattice of transparent pixels
    }
    pics.put(
        Kind::Tile,
        5,
        (16, 16),
        art(5, 16, 16, 4),
        Some(clear.clone()),
    );
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
        let mut hd_blocks = 0;
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
                    let want: Vec<u32> = (0..s * s)
                        .map(|k| hd[(v * s + k / s) * hw + u * s + k % s])
                        .map(|c| if c == 0 { classic } else { c })
                        .collect();
                    assert!(got == want, "{kind:?} ({u}, {v}): its block");
                    hd_blocks += usize::from(got.iter().any(|&g| g != classic));
                }
            }
        }
        assert!(hd_blocks > 50, "{s}x: MMPX changed blocks");
        // A transparent pixel's colour (index 0 under it in the record) never shows in the tile.
        let zero = word(fb.palette[0]);
        assert!(
            (16..32).all(
                |y| (16..32).all(|x| block(c.rgba(), scale, (x, y)).iter().all(|&g| g != zero))
            )
        );
    }
}

#[test]
fn classic_flagged_stale_and_unknown_pixels_are_their_classic_colour() {
    let mut pics = Pics::default();
    pics.put(Kind::Tile, 2, (16, 16), art(2, 16, 16, 3), None);
    let mut fb = plane();
    fb.palette = palette(1);
    fb.clear(12);
    draw(&mut fb, &pics, Kind::Tile, 2, (32, 32));
    let px = |fb: &Framebuffer, x, y| fb.get(x, y).unwrap();
    let cases = [
        ((33, 33), tile(2, 0, (1, 1), FLAG_OVERLAY)),
        ((34, 33), tile(2, 0, (2, 1), FLAG_OUTLINE)),
        ((35, 33), tile(2, 1, (3, 1), 0)), // drawn at a version that is not current
        ((36, 33), tile(2, 0, (16, 1), 0)), // outside the image
        ((37, 33), tile(9, 0, (5, 1), 0)), // an image the images do not have
        ((38, 33), Src::CLASSIC),
    ];
    for ((x, y), src) in cases {
        let p = px(&fb, x, y);
        fb.set_src(x, y, p, src);
    }
    for scale in [Scale::X2, Scale::X4] {
        let mut c = Compositor::new();
        c.compose(&fb, &mut pics.clone(), scale);
        for ((x, y), _) in cases {
            let classic = word(fb.palette[usize::from(px(&fb, x, y))]);
            assert!(
                block(c.rgba(), scale, (x, y)).iter().all(|&g| g == classic),
                "({x}, {y}): classic"
            );
        }
        let (hw, hd) = hd_of(&pics, Kind::Tile, 2, &fb.palette, scale);
        let s = scale.factor();
        let want: Vec<u32> = (0..s * s)
            .map(|k| hd[(4 * s + k / s) * hw + 4 * s + k % s])
            .collect();
        assert_eq!(
            block(c.rgba(), scale, (36, 36)),
            want,
            "its neighbour stays HD"
        );
    }
}

/// Random edits between compositions (pixels drawn from images and classic ones, an image
/// rewritten, the palette changed, the scale switched, the plane reset): after each, the kept
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
            match x.below(10) {
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

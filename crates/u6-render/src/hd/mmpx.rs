//! MMPX, the style-preserving 2× pixel-art magnifier of Morgan McGuire and Mara Gagiu, "MMPX
//! Style-Preserving Pixel Art Magnification", Journal of Graphics Techniques 2021, ported from
//! the MMPXAlgorithm of their reference C++ (`code/source/cppPerf.cpp` of the paper's
//! supplement, MIT licence; see `NOTICE`). Every rule, its order and the border clamp are the
//! reference's; `tests` checks the port against vectors the reference made
//! (`mmpx-vectors.txt`, `tools/mmpx`).
//!
//! A pixel is a packed RGBA word (`u32::from_le_bytes([r, g, b, a])`, alpha in the top byte, as
//! the reference reads its `Color4unorm8` pixels). The output holds only input words: MMPX adds no
//! colours.

/// The reference's luma: the sum of the colour channels plus one, times 256 less alpha (a
/// transparent pixel weighs as a bright one, so curves against transparency are kept).
#[inline]
fn luma(c: u32) -> u32 {
    let alpha = c >> 24;
    (((c >> 16) & 0xff) + ((c >> 8) & 0xff) + (c & 0xff) + 1) * (256 - alpha)
}

#[inline]
fn all_eq2(b: u32, a0: u32, a1: u32) -> bool {
    b == a0 && b == a1
}

#[inline]
fn all_eq3(b: u32, a0: u32, a1: u32, a2: u32) -> bool {
    b == a0 && b == a1 && b == a2
}

#[inline]
fn all_eq4(b: u32, a0: u32, a1: u32, a2: u32, a3: u32) -> bool {
    b == a0 && b == a1 && b == a2 && b == a3
}

#[inline]
fn any_eq3(b: u32, a0: u32, a1: u32, a2: u32) -> bool {
    b == a0 || b == a1 || b == a2
}

#[inline]
fn none_eq2(b: u32, a0: u32, a1: u32) -> bool {
    b != a0 && b != a1
}

#[inline]
fn none_eq4(b: u32, a0: u32, a1: u32, a2: u32, a3: u32) -> bool {
    b != a0 && b != a1 && b != a2 && b != a3
}

/// The largest image [`mmpx2x`] takes: the largest image the compositor magnifies at 2× (the
/// second pass of a 4× magnification of it, [`super::IMAGE_MAX`]). The bound keeps every size
/// and coordinate below in range.
pub const MAX_2X: (usize, usize) = (2 * super::IMAGE_MAX.0, 2 * super::IMAGE_MAX.1);

/// `src` (`w` × `h` words, row-major) magnified 2× into `dst`, which is resized to `2w × 2h`.
/// Samples past the image take the nearest border pixel. An empty image gives an empty one.
///
/// Panics for an image past [`MAX_2X`] or smaller than `w` × `h` words.
pub fn mmpx2x(src: &[u32], w: usize, h: usize, dst: &mut Vec<u32>) {
    assert!(w <= MAX_2X.0 && h <= MAX_2X.1, "an image past {MAX_2X:?}");
    let n = w.checked_mul(h).expect("bounded above");
    assert!(src.len() >= n, "an image smaller than {w}x{h}");
    dst.clear();
    dst.resize(n.checked_mul(4).expect("bounded above"), 0);
    if w == 0 || h == 0 {
        return;
    }
    let (max_x, max_y) = (w as isize - 1, h as isize - 1);
    let at = |x: isize, y: isize| src[y.clamp(0, max_y) as usize * w + x.clamp(0, max_x) as usize];
    let dw = 2 * w;
    for sy in 0..h {
        let y = sy as isize;
        for sx in 0..w {
            let x = sx as isize;
            let (a, b, c) = (at(x - 1, y - 1), at(x, y - 1), at(x + 1, y - 1));
            let (d, e, f) = (at(x - 1, y), at(x, y), at(x + 1, y));
            let (g, h, i) = (at(x - 1, y + 1), at(x, y + 1), at(x + 1, y + 1));
            let (mut j, mut k, mut l, mut m) = (e, e, e, e);
            if (a ^ e) | (b ^ e) | (c ^ e) | (d ^ e) | (f ^ e) | (g ^ e) | (h ^ e) | (i ^ e) != 0 {
                let (p, s) = (at(x, y - 2), at(x, y + 2));
                let (q, r) = (at(x - 2, y), at(x + 2, y));
                let (bl, dl, el, fl, hl) = (luma(b), luma(d), luma(e), luma(f), luma(h));

                // 1:1 slope rules
                if (d == b && d != h && d != f)
                    && (el >= dl || e == a)
                    && any_eq3(e, a, c, g)
                    && (el < dl || a != d || e != p || e != q)
                {
                    j = d;
                }
                if (b == f && b != d && b != h)
                    && (el >= bl || e == c)
                    && any_eq3(e, a, c, i)
                    && (el < bl || c != b || e != p || e != r)
                {
                    k = b;
                }
                if (h == d && h != f && h != b)
                    && (el >= hl || e == g)
                    && any_eq3(e, a, g, i)
                    && (el < hl || g != h || e != s || e != q)
                {
                    l = h;
                }
                if (f == h && f != b && f != d)
                    && (el >= fl || e == i)
                    && any_eq3(e, c, g, i)
                    && (el < fl || i != h || e != r || e != s)
                {
                    m = f;
                }

                // Intersection rules
                if (e != f && all_eq4(e, c, i, d, q) && all_eq2(f, b, h)) && f != at(x + 3, y) {
                    k = f;
                    m = f;
                }
                if (e != d && all_eq4(e, a, g, f, r) && all_eq2(d, b, h)) && d != at(x - 3, y) {
                    j = d;
                    l = d;
                }
                if (e != h && all_eq4(e, g, i, b, p) && all_eq2(h, d, f)) && h != at(x, y + 3) {
                    l = h;
                    m = h;
                }
                if (e != b && all_eq4(e, a, c, h, s) && all_eq2(b, d, f)) && b != at(x, y - 3) {
                    j = b;
                    k = b;
                }
                if bl < el && all_eq4(e, g, h, i, s) && none_eq4(e, a, d, c, f) {
                    j = b;
                    k = b;
                }
                if hl < el && all_eq4(e, a, b, c, p) && none_eq4(e, d, g, i, f) {
                    l = h;
                    m = h;
                }
                if fl < el && all_eq4(e, a, d, g, q) && none_eq4(e, b, c, i, h) {
                    k = f;
                    m = f;
                }
                if dl < el && all_eq4(e, c, f, i, r) && none_eq4(e, b, a, g, h) {
                    j = d;
                    l = d;
                }

                // 2:1 slope rules
                if h != b {
                    if h != a && h != e && h != c {
                        if all_eq3(h, g, f, r) && none_eq2(h, d, at(x + 2, y - 1)) {
                            l = m;
                        }
                        if all_eq3(h, i, d, q) && none_eq2(h, f, at(x - 2, y - 1)) {
                            m = l;
                        }
                    }
                    if b != i && b != g && b != e {
                        if all_eq3(b, a, f, r) && none_eq2(b, d, at(x + 2, y + 1)) {
                            j = k;
                        }
                        if all_eq3(b, c, d, q) && none_eq2(b, f, at(x - 2, y + 1)) {
                            k = j;
                        }
                    }
                }
                if f != d {
                    if d != i && d != e && d != c {
                        if all_eq3(d, a, h, s) && none_eq2(d, b, at(x + 1, y + 2)) {
                            j = l;
                        }
                        if all_eq3(d, g, b, p) && none_eq2(d, h, at(x + 1, y - 2)) {
                            l = j;
                        }
                    }
                    if f != e && f != a && f != g {
                        if all_eq3(f, c, h, s) && none_eq2(f, b, at(x - 1, y + 2)) {
                            k = m;
                        }
                        if all_eq3(f, i, b, p) && none_eq2(f, h, at(x - 1, y - 2)) {
                            m = k;
                        }
                    }
                }
            }
            let o = 2 * sy * dw + 2 * sx;
            dst[o] = j;
            dst[o + 1] = k;
            dst[o + dw] = l;
            dst[o + dw + 1] = m;
        }
    }
}

/// `src` (`w` × `h`) magnified by `scale` (2: MMPX once; 4: MMPX twice) into `dst`, which is
/// resized to `scale·w × scale·h`.
///
/// Panics for an image past the screen ([`super::IMAGE_MAX`]).
pub fn mmpx(src: &[u32], w: usize, h: usize, scale: super::Scale, dst: &mut Vec<u32>) {
    assert!(
        w <= super::IMAGE_MAX.0 && h <= super::IMAGE_MAX.1,
        "an image past the screen"
    );
    match scale {
        super::Scale::X2 => mmpx2x(src, w, h, dst),
        super::Scale::X4 => {
            let mut half = Vec::new();
            mmpx2x(src, w, h, &mut half);
            mmpx2x(&half, 2 * w, 2 * h, dst);
        }
    }
}

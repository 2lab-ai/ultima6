//! Keying, colour transfer and the gates (spec 07 HD2), all on one image at a time against its
//! classic image. Each metric is `None` where it cannot be measured, and an unmeasurable metric
//! passes:
//! - structure: a flat classic image has no edges to correlate;
//! - shift: a flat image has no defined offset (a periodic one takes the nearest of the
//!   shifts that match alike, [`shift`]);
//! - silhouette IoU: only an image with both transparent and opaque pixels has a silhouette;
//! - key purity: only an image with background (pixels outside its mask grown by one source
//!   pixel) shows the key.

use crate::img::{self, Classic, GREY, Plane, Rgba};

/// The gate thresholds. The defaults are spec 07's; `--gate name=value` changes one for a run
/// (the values used are written to `coverage.txt` and every `gates.jsonl` line).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    /// Structure correlation at least this ([`structure`]).
    pub structure: f32,
    /// Best-fit shift at most this, in source pixels.
    pub shift: f32,
    /// Silhouette IoU at least this.
    pub iou: f32,
    /// Key purity: the 99th percentile of the background's deviation from the key (largest
    /// channel difference), at most this many levels.
    pub purity: f32,
    /// Terrain: the wrap seam's step at most this times the interior step.
    pub seam: f32,
    /// Actor groups: every moving frame pair keeps at least this share of its motion.
    pub motion: f32,
    /// A flat classic image (no structure to correlate): the slice's spread at the source
    /// pixel, at most this many levels ([`flatness`]).
    pub flat: f32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            structure: 0.9,
            shift: 0.5,
            iou: 0.85,
            purity: 12.0,
            seam: 1.5,
            motion: 0.5,
            flat: 12.0,
        }
    }
}

impl Thresholds {
    /// Sets one from `name=value`.
    pub fn set(&mut self, spec: &str) -> Result<(), String> {
        let (name, value) = spec
            .split_once('=')
            .ok_or_else(|| format!("--gate {spec:?}: want name=value"))?;
        let v: f32 = value
            .parse()
            .map_err(|_| format!("--gate {spec:?}: not a number"))?;
        // Each gate's domain; NaN and the infinities are outside every one.
        let (slot, lo, hi) = match name {
            "structure" => (&mut self.structure, -1.0, 1.0),
            "shift" => (&mut self.shift, 0.0, 16.0),
            "iou" => (&mut self.iou, 0.0, 1.0),
            "purity" => (&mut self.purity, 0.0, 255.0),
            "seam" => (&mut self.seam, 0.0, 1000.0),
            "motion" => (&mut self.motion, 0.0, 2.0),
            "flat" => (&mut self.flat, 0.0, 255.0),
            _ => return Err(format!("--gate {spec:?}: unknown gate")),
        };
        if !v.is_finite() || v < lo || v > hi {
            return Err(format!("--gate {spec:?}: outside {lo}..={hi}"));
        }
        *slot = v;
        Ok(())
    }

    /// `structure>=0.9 shift<=0.5 ...`
    pub fn text(&self) -> String {
        format!(
            "structure>={} shift<={} iou>={} purity<={} seam<={} motion>={} flat<={}",
            self.structure, self.shift, self.iou, self.purity, self.seam, self.motion, self.flat
        )
    }
}

/// The keying of the imagegen skill's `remove_chroma_key.py` (soft matte 12..220 on the largest
/// channel difference, the key-dominance alpha, a noise floor of 8, spill clean-up), bounded by
/// the classic mask grown by one source pixel: outside it everything is transparent. An image
/// without transparent pixels is not keyed.
pub fn key_out(raw: &Rgba, classic: &Classic, key: [u8; 3]) -> Rgba {
    let mut out = raw.clone();
    if !classic.has_clear() {
        for p in out.pixels.chunks_exact_mut(4) {
            p[3] = 255;
        }
        return out;
    }
    let k = raw.width / classic.width;
    let mask = classic.dilated(1);
    for y in 0..raw.height {
        for x in 0..raw.width {
            let inside = mask[((y / k) * classic.width + x / k) as usize];
            let p = img::get(raw, x, y);
            let rgb = [p[0], p[1], p[2]];
            let d = distance(rgb, key);
            let key_like = d <= 32 || dominance(rgb, key) >= 16.0;
            let mut a = if !inside {
                0
            } else if key_like {
                soft_alpha(d).min(dominance_alpha(rgb, key))
            } else {
                255
            };
            if a <= 8 {
                a = 0;
            }
            let c = if a == 0 {
                [0, 0, 0, 0]
            } else if key_like && a < 252 {
                let c = despill(rgb, key);
                [c[0], c[1], c[2], a]
            } else {
                [rgb[0], rgb[1], rgb[2], a]
            };
            img::put(&mut out, x, y, c);
        }
    }
    out
}

fn distance(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0)
}

fn soft_alpha(d: u8) -> u8 {
    let (lo, hi) = (12.0, 220.0);
    let t = ((f32::from(d) - lo) / (hi - lo)).clamp(0.0, 1.0);
    (255.0 * t * t * (3.0 - 2.0 * t)).round() as u8
}

/// The key's channels: within 16 of its largest and at least 128.
fn spill(key: [u8; 3]) -> Vec<usize> {
    let m = key.iter().copied().max().unwrap_or(0);
    if m < 128 {
        return Vec::new();
    }
    (0..3)
        .filter(|&i| key[i] >= m.saturating_sub(16) && key[i] >= 128)
        .collect()
}

fn dominance(rgb: [u8; 3], key: [u8; 3]) -> f32 {
    let s = spill(key);
    if s.is_empty() {
        return 0.0;
    }
    let strong = s
        .iter()
        .map(|&i| f32::from(rgb[i]))
        .fold(f32::MAX, f32::min);
    let other = (0..3)
        .filter(|i| !s.contains(i))
        .map(|i| f32::from(rgb[i]))
        .fold(0.0, f32::max);
    strong - other
}

fn dominance_alpha(rgb: [u8; 3], key: [u8; 3]) -> u8 {
    let s = spill(key);
    if s.is_empty() {
        return 255;
    }
    let d = dominance(rgb, key);
    if d <= 0.0 {
        return 255;
    }
    let other = (0..3)
        .filter(|i| !s.contains(i))
        .map(|i| f32::from(rgb[i]))
        .fold(0.0, f32::max);
    let den = (f32::from(key.iter().copied().max().unwrap_or(0)) - other).max(1.0);
    ((1.0 - (d / den).min(1.0)) * 255.0).round() as u8
}

fn despill(rgb: [u8; 3], key: [u8; 3]) -> [u8; 3] {
    let s = spill(key);
    let other = (0..3).filter(|i| !s.contains(i)).map(|i| rgb[i]).max();
    let mut out = rgb;
    if let Some(anchor) = other {
        let cap = anchor.saturating_sub(1);
        for &i in &s {
            out[i] = out[i].min(cap);
        }
    }
    out
}

/// Key purity of the raw slice (before keying): see [`Thresholds::purity`].
pub fn purity(raw: &Rgba, classic: &Classic, key: [u8; 3]) -> Option<f32> {
    let k = raw.width / classic.width;
    let mask = classic.dilated(1);
    let mut dev: Vec<u8> = Vec::new();
    for y in 0..raw.height {
        for x in 0..raw.width {
            if !mask[((y / k) * classic.width + x / k) as usize] {
                let p = img::get(raw, x, y);
                dev.push(distance([p[0], p[1], p[2]], key));
            }
        }
    }
    if dev.is_empty() {
        return None;
    }
    dev.sort_unstable();
    Some(f32::from(dev[(dev.len() - 1) * 99 / 100]))
}

/// Moves the image's colours toward its classic image's: per channel, the mean and spread of
/// the opaque pixels are matched to those of the classic opaque pixels (the spread ratio held
/// to 0.5..2).
pub fn colour_transfer(hd: &mut Rgba, classic: &Classic) {
    let stats = |vals: &mut dyn Iterator<Item = [f32; 3]>| {
        let (mut n, mut s, mut q) = (0.0f64, [0.0f64; 3], [0.0f64; 3]);
        for v in vals {
            n += 1.0;
            for i in 0..3 {
                s[i] += f64::from(v[i]);
                q[i] += f64::from(v[i]).powi(2);
            }
        }
        (n > 0.0).then(|| {
            let m: [f64; 3] = std::array::from_fn(|i| s[i] / n);
            let sd: [f64; 3] = std::array::from_fn(|i| (q[i] / n - m[i] * m[i]).max(0.0).sqrt());
            (m, sd)
        })
    };
    let Some((ms, ss)) = stats(&mut classic.px.iter().flatten().map(|c| c.map(f32::from))) else {
        return;
    };
    let Some((mh, sh)) = stats(
        &mut hd
            .pixels
            .chunks_exact(4)
            .filter(|p| p[3] >= 128)
            .map(|p| [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])]),
    ) else {
        return;
    };
    let ratio: [f64; 3] = std::array::from_fn(|i| {
        if sh[i] < 1.0 {
            1.0
        } else {
            (ss[i] / sh[i]).clamp(0.5, 2.0)
        }
    });
    for p in hd.pixels.chunks_exact_mut(4) {
        if p[3] == 0 {
            continue;
        }
        for i in 0..3 {
            let v = (f64::from(p[i]) - mh[i]) * ratio[i] + ms[i];
            p[i] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Mean absolute colour error (levels, all channels) at source resolution over the classic
/// opaque pixels: the HD block's alpha-weighted mean against the classic colour.
pub fn drift(classic: &Classic, hd: &Rgba) -> Option<f32> {
    let k = hd.width / classic.width;
    let (mut sum, mut n) = (0.0f64, 0usize);
    for y in 0..classic.height {
        for x in 0..classic.width {
            let Some(c) = classic.at(x, y) else { continue };
            let (mut acc, mut wsum) = ([0.0f64; 3], 0.0f64);
            for yy in y * k..(y + 1) * k {
                for xx in x * k..(x + 1) * k {
                    let p = img::get(hd, xx, yy);
                    let a = f64::from(p[3]);
                    wsum += a;
                    for i in 0..3 {
                        acc[i] += f64::from(p[i]) * a;
                    }
                }
            }
            if wsum == 0.0 {
                sum += 3.0 * 255.0;
            } else {
                sum += (0..3)
                    .map(|i| (acc[i] / wsum - f64::from(c[i])).abs())
                    .sum::<f64>();
            }
            n += 3;
        }
    }
    (n > 0).then(|| (sum / n as f64) as f32)
}

/// A separable Gaussian blur of `sigma` pixels (clamped edges).
fn gaussian(p: &Plane, sigma: f32) -> Plane {
    let r = (3.0 * sigma).ceil() as i64;
    let k: Vec<f32> = (-r..=r)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let sum: f32 = k.iter().sum();
    let k: Vec<f32> = k.iter().map(|v| v / sum).collect();
    let pass = |src: &Plane, horizontal: bool| {
        let mut v = Vec::with_capacity(src.w * src.h);
        for y in 0..src.h as i64 {
            for x in 0..src.w as i64 {
                let mut acc = 0.0;
                for (j, w) in k.iter().enumerate() {
                    let d = j as i64 - r;
                    acc += w * if horizontal {
                        src.clamped(x + d, y)
                    } else {
                        src.clamped(x, y + d)
                    };
                }
                v.push(acc);
            }
        }
        Plane {
            w: src.w,
            h: src.h,
            v,
        }
    };
    pass(&pass(p, true), false)
}

/// The 3 × 3 edge filter (8 at the centre, −1 around), negative responses cut to 0.
fn edges(p: &Plane) -> Plane {
    let mut v = Vec::with_capacity(p.w * p.h);
    for y in 0..p.h as i64 {
        for x in 0..p.w as i64 {
            let mut acc = 9.0 * p.clamped(x, y);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    acc -= p.clamped(x + dx, y + dy);
                }
            }
            v.push(acc.max(0.0));
        }
    }
    Plane { w: p.w, h: p.h, v }
}

/// Box-averages `p` by `k`.
fn shrink(p: &Plane, k: usize) -> Plane {
    let (w, h) = (p.w / k, p.h / k);
    let mut v = vec![0.0; w * h];
    for y in 0..h * k {
        for x in 0..w * k {
            v[(y / k) * w + x / k] += p.at(x, y);
        }
    }
    let n = (k * k) as f32;
    v.iter_mut().for_each(|x| *x /= n);
    Plane { w, h, v }
}

/// Normalised cross-correlation of two planes of one size; `None` if `a` is flat.
fn ncc(a: &Plane, b: &Plane) -> Option<f32> {
    let n = a.v.len() as f64;
    let ma = a.v.iter().map(|&x| f64::from(x)).sum::<f64>() / n;
    let mb = b.v.iter().map(|&x| f64::from(x)).sum::<f64>() / n;
    let (mut va, mut vb, mut cov) = (0.0f64, 0.0f64, 0.0f64);
    for (&x, &y) in a.v.iter().zip(&b.v) {
        let (dx, dy) = (f64::from(x) - ma, f64::from(y) - mb);
        va += dx * dx;
        vb += dy * dy;
        cov += dx * dy;
    }
    if va / n < 1e-6 {
        return None;
    }
    if vb / n < 1e-6 {
        return Some(0.0);
    }
    Some((cov / (va * vb).sqrt()) as f32)
}

/// Blurs of the edge map, in HD pixels at ×8: one source pixel before the edge filter and one
/// and a half after. r1 blurred by 2 and 3 HD pixels; at that scale the correlation measures
/// sub-pixel edges, which a repaint changes on purpose (on the synthetic sheets: sprites 0.78–0.93,
/// textures −0.4–0.6), while at a source pixel it measures the shapes the prompt asks to keep
/// (sprites 0.95–0.99, a checker repainted flat −0.15).
const EDGE_BLUR: (f32, f32) = (8.0, 12.0);

/// A plane's blurred edge map: Gaussian, the edge filter, Gaussian ([`EDGE_BLUR`]), at ×8,
/// then box-averaged to ×4. Every filter clamps at the image's edges (r1's measurement left the
/// filter's one-pixel frame unfiltered, which inflated its correlations to 0.97–0.99).
fn edge_map(p: &Plane) -> Plane {
    shrink(&gaussian(&edges(&gaussian(p, EDGE_BLUR.0)), EDGE_BLUR.1), 2)
}

/// Luminance of an ×8 image as it is on the sheet.
fn sheet_luma(img: &Rgba) -> Plane {
    img::hd_luma(img, 1)
}

/// Structure correlation (r1 §2.2, `measure3.py`): NCC of the blurred edge maps of the classic
/// image (×8 nearest, transparent pixels in the key, as on the sheet) and of the slice that came
/// back; `None` for a flat classic image.
pub fn structure(classic: &Classic, raw: &Rgba, key: [u8; 3]) -> Option<f32> {
    let k = raw.width / classic.width;
    let a = edge_map(&sheet_luma(&classic.scaled(k, key)));
    let b = edge_map(&sheet_luma(raw));
    ncc(&a, &b)
}

/// Portraits are judged at the layout scale: a quarter of the source size.
pub const LAYOUT_SHRINK: u32 = 4;

/// The colour image at 1 / [`LAYOUT_SHRINK`] of the classic size: box averages, transparent
/// pixels as grey.
fn layout_classic(c: &Classic) -> Vec<[f64; 3]> {
    let k = LAYOUT_SHRINK;
    let (w, h) = (c.width / k, c.height / k);
    let mut out = vec![[0.0; 3]; (w * h) as usize];
    for y in 0..h * k {
        for x in 0..w * k {
            let p = c.at(x, y).map_or([GREY as f64; 3], |p| p.map(f64::from));
            let o = &mut out[((y / k) * w + x / k) as usize];
            for i in 0..3 {
                o[i] += p[i] / f64::from(k * k);
            }
        }
    }
    out
}

/// The same for an HD image `s` times the classic size (alpha composited over grey).
fn layout_hd(img: &Rgba, s: u32) -> Vec<[f64; 3]> {
    let k = LAYOUT_SHRINK * s;
    let (w, h) = (img.width / k, img.height / k);
    let mut out = vec![[0.0; 3]; (w * h) as usize];
    for y in 0..h * k {
        for x in 0..w * k {
            let p = img::get(img, x, y);
            let a = f64::from(p[3]) / 255.0;
            let o = &mut out[((y / k) * w + x / k) as usize];
            for i in 0..3 {
                let v = f64::from(p[i]) * a + f64::from(GREY) * (1.0 - a);
                o[i] += v / f64::from(k * k);
            }
        }
    }
    out
}

/// Layout correlation (portraits): the classic image and the slice, both box-averaged to a
/// quarter of the source size, correlated as colour images: each channel centred on its own
/// mean, the three channels' products and variances pooled. At that scale the measure is where
/// the face, hair, clothes and background sit and in which colours; a repaint keeps that and
/// changes every edge, which the source-scale [`structure`] counts against it (synthetic
/// portraits: 0.55–0.66 there, 0.904–0.913 here). `None` for a flat image.
pub fn layout(classic: &Classic, raw: &Rgba) -> Option<f32> {
    let a = layout_classic(classic);
    let b = layout_hd(raw, raw.width / classic.width);
    let n = a.len() as f64;
    let (mut cov, mut va, mut vb) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..3 {
        let ma = a.iter().map(|p| p[i]).sum::<f64>() / n;
        let mb = b.iter().map(|p| p[i]).sum::<f64>() / n;
        for (p, q) in a.iter().zip(&b) {
            let (x, y) = (p[i] - ma, q[i] - mb);
            cov += x * y;
            va += x * x;
            vb += y * y;
        }
    }
    if va / n < 1e-6 {
        return None;
    }
    if vb / n < 1e-6 {
        return Some(0.0);
    }
    Some((cov / (va * vb).sqrt()) as f32)
}

/// Reach of the shift search in HD pixels, and its step (r1 `measure.py`: 2 HD px steps).
const SHIFT_REACH: i64 = 16;
const SHIFT_STEP: i64 = 2;
/// A shift is taken only when it matches better than no shift by this many levels; of the
/// shifts within this of the best match, the nearest to none is taken (a periodic texture
/// matches several shifts alike, r1 §2.2: the planks).
const SHIFT_MARGIN: f32 = 1.0;

/// Best-fit shift, in source pixels (r1 `measure.py`): the slice's 8 × 8 blocks moved by
/// (dx, dy) HD pixels, box-averaged, against the classic image (transparent as the key) by mean
/// absolute RGB error over the blocks that stay inside; `None` for a flat classic image.
pub fn shift(classic: &Classic, raw: &Rgba, key: [u8; 3]) -> Option<f32> {
    let k = i64::from(raw.width / classic.width);
    let first = classic.px[0];
    if classic.px.iter().all(|p| *p == first) {
        return None;
    }
    let mad = |dx: i64, dy: i64| -> Option<f32> {
        let (mut sum, mut n) = (0.0f64, 0u64);
        for by in 0..i64::from(classic.height) {
            for bx in 0..i64::from(classic.width) {
                let (x0, y0) = (bx * k + dx, by * k + dy);
                if x0 < 0
                    || y0 < 0
                    || x0 + k > i64::from(raw.width)
                    || y0 + k > i64::from(raw.height)
                {
                    continue;
                }
                let c = classic.at(bx as u32, by as u32).unwrap_or(key);
                let mut acc = [0u32; 3];
                for y in y0..y0 + k {
                    for x in x0..x0 + k {
                        let p = img::get(raw, x as u32, y as u32);
                        for i in 0..3 {
                            acc[i] += u32::from(p[i]);
                        }
                    }
                }
                let kk = (k * k) as f64;
                for i in 0..3 {
                    sum += (f64::from(acc[i]) / kk - f64::from(c[i])).abs();
                }
                n += 3;
            }
        }
        (n > 0).then(|| (sum / n as f64) as f32)
    };
    let mut all = Vec::new();
    for dy in (-SHIFT_REACH..=SHIFT_REACH).step_by(SHIFT_STEP as usize) {
        for dx in (-SHIFT_REACH..=SHIFT_REACH).step_by(SHIFT_STEP as usize) {
            if let Some(e) = mad(dx, dy) {
                all.push((e, dx.abs().max(dy.abs())));
            }
        }
    }
    let best = all.iter().map(|a| a.0).fold(f32::MAX, f32::min);
    let near = all
        .iter()
        .filter(|a| a.0 <= best + SHIFT_MARGIN)
        .map(|a| a.1)
        .min()?;
    Some(near as f32 / k as f32)
}

/// Silhouette IoU: the classic opaque mask at ×k against the HD alpha ≥ 128.
pub fn iou(classic: &Classic, hd: &Rgba) -> Option<f32> {
    if !classic.has_clear() || !classic.has_opaque() {
        return None;
    }
    let k = hd.width / classic.width;
    let (mut inter, mut union) = (0u64, 0u64);
    for y in 0..hd.height {
        for x in 0..hd.width {
            let a = classic.at(x / k, y / k).is_some();
            let b = img::get(hd, x, y)[3] >= 128;
            inter += u64::from(a && b);
            union += u64::from(a || b);
        }
    }
    Some(if union == 0 {
        1.0
    } else {
        inter as f32 / union as f32
    })
}

/// The per-image metrics.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    pub structure: Option<f32>,
    pub shift: Option<f32>,
    pub iou: Option<f32>,
    pub purity: Option<f32>,
    /// Only for a flat classic image ([`flatness`]).
    pub flat: Option<f32>,
    /// Colour error before and after the transfer (reported, not gated).
    pub drift_raw: Option<f32>,
    pub drift: Option<f32>,
}

impl Metrics {
    pub fn pass(&self, t: &Thresholds) -> bool {
        self.failures(t).is_empty()
    }

    /// The names of the gates it fails. A metric that is not a finite number fails.
    pub fn failures(&self, t: &Thresholds) -> Vec<&'static str> {
        let below = |v: Option<f32>, min: f32| v.is_some_and(|v| !v.is_finite() || v < min);
        let above = |v: Option<f32>, max: f32| v.is_some_and(|v| !v.is_finite() || v > max);
        let mut out = Vec::new();
        if below(self.structure, t.structure) {
            out.push("structure");
        }
        if above(self.shift, t.shift) {
            out.push("shift");
        }
        if below(self.iou, t.iou) {
            out.push("iou");
        }
        if above(self.purity, t.purity) {
            out.push("purity");
        }
        if above(self.flat, t.flat) {
            out.push("flat");
        }
        out
    }
}

/// Flat-source fidelity: for a classic image of one colour, opaque (nothing to correlate, no
/// edge to shift, no silhouette, no background), the largest of three spreads, in levels:
/// the slice box-averaged to the source pixel (a repaint may add grain inside a pixel; at the
/// source pixel it stays flat), and a quarter of each of the slice's own spread and of its mean
/// colour's distance from the classic colour (texture and colour may move, up to 4 × the gate:
/// 48 levels by default). Each is the largest over the three channels. `None` for any other
/// image.
pub fn flatness(classic: &Classic, raw: &Rgba) -> Option<f32> {
    let first = classic.px[0]?;
    if classic.px.iter().any(|p| *p != Some(first)) {
        return None;
    }
    let k = raw.width / classic.width;
    let n = f64::from(k * k);
    let mut blocks: Vec<[f64; 3]> = Vec::with_capacity(classic.px.len());
    for by in 0..classic.height {
        for bx in 0..classic.width {
            let mut acc = [0.0f64; 3];
            for y in by * k..(by + 1) * k {
                for x in bx * k..(bx + 1) * k {
                    let p = img::get(raw, x, y);
                    for i in 0..3 {
                        acc[i] += f64::from(p[i]) / n;
                    }
                }
            }
            blocks.push(acc);
        }
    }
    let spread = |vals: &mut dyn Iterator<Item = [f64; 3]>| -> ([f64; 3], f64) {
        let v: Vec<[f64; 3]> = vals.collect();
        let m = v.len() as f64;
        let mean: [f64; 3] = std::array::from_fn(|i| v.iter().map(|b| b[i]).sum::<f64>() / m);
        let sd = (0..3)
            .map(|i| (v.iter().map(|b| (b[i] - mean[i]).powi(2)).sum::<f64>() / m).sqrt())
            .fold(0.0, f64::max);
        (mean, sd)
    };
    let (_, block_sd) = spread(&mut blocks.into_iter());
    let (mean, pixel_sd) = spread(
        &mut raw
            .pixels
            .chunks_exact(4)
            .map(|p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]),
    );
    let offset = (0..3)
        .map(|i| (mean[i] - f64::from(first[i])).abs())
        .fold(0.0, f64::max);
    Some(block_sd.max(pixel_sd / 4.0).max(offset / 4.0) as f32)
}

/// The scale structure is measured at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    /// Edge maps at the source pixel ([`structure`]): tiles and bitmaps.
    Source,
    /// Colour layout at a quarter of the source size ([`layout`]): portraits.
    Layout,
}

impl Scale {
    pub fn name(self) -> &'static str {
        match self {
            Scale::Source => "source",
            Scale::Layout => "layout",
        }
    }
}

/// Keys, transfers and measures one slice: the HD image to keep and its metrics.
///
/// Structure (at `scale`), shift and purity are measured on the slice as it came back, against
/// the classic image as it was sent (r1's measurements); IoU and the colour error on the image
/// kept.
pub fn judge(raw: &Rgba, classic: &Classic, key: [u8; 3], scale: Scale) -> (Rgba, Metrics) {
    let mut hd = key_out(raw, classic, key);
    let drift_raw = drift(classic, &hd);
    colour_transfer(&mut hd, classic);
    let m = Metrics {
        structure: match scale {
            Scale::Source => structure(classic, raw, key),
            Scale::Layout => layout(classic, raw),
        },
        shift: shift(classic, raw, key),
        iou: iou(classic, &hd),
        purity: purity(raw, classic, key),
        flat: flatness(classic, raw),
        drift_raw,
        drift: drift(classic, &hd),
    };
    (hd, m)
}

/// Mean step across the lines of a lattice of `pitch` pixels, horizontally and vertically: the
/// wrap line (last column to first, last row to first) and the inner lines.
fn lattice_steps(rgb: &dyn Fn(u32, u32) -> [f32; 3], w: u32, h: u32, pitch: u32) -> [f32; 4] {
    let step = |a: [f32; 3], b: [f32; 3]| (0..3).map(|i| (a[i] - b[i]).abs()).sum::<f32>() / 3.0;
    let (mut wrap_h, mut inner_h, mut n_inner_h) = (0.0, 0.0, 0.0);
    for y in 0..h {
        wrap_h += step(rgb(w - 1, y), rgb(0, y));
        for x in (pitch..w).step_by(pitch as usize) {
            inner_h += step(rgb(x - 1, y), rgb(x, y));
            n_inner_h += 1.0;
        }
    }
    let (mut wrap_v, mut inner_v, mut n_inner_v) = (0.0, 0.0, 0.0);
    for x in 0..w {
        wrap_v += step(rgb(x, h - 1), rgb(x, 0));
        for y in (pitch..h).step_by(pitch as usize) {
            inner_v += step(rgb(x, y - 1), rgb(x, y));
            n_inner_v += 1.0;
        }
    }
    [
        wrap_h / h as f32,
        inner_h / f32::max(n_inner_h, 1.0),
        wrap_v / w as f32,
        inner_v / f32::max(n_inner_v, 1.0),
    ]
}

fn ratio(wrap: f32, inner: f32) -> f32 {
    if inner < 0.5 {
        if wrap < 0.5 { 1.0 } else { wrap / 0.5 }
    } else {
        wrap / inner
    }
}

/// Terrain seams: the tile tiled 3 × 3 against itself, the step across its wrap lines over the
/// step across the other lines of the same lattice (every source pixel, 8 HD pixels apart), the
/// larger of the two axes. Returns (the HD ratio, its limit): the limit is `max_ratio`, or the
/// classic tile's own ratio where that is higher (a tile that does not tile with itself in the
/// original is not held to more).
pub fn seam(classic: &Classic, hd: &Rgba, max_ratio: f32) -> (f32, f32) {
    let k = hd.width / classic.width;
    let f = |c: [u8; 4]| [f32::from(c[0]), f32::from(c[1]), f32::from(c[2])];
    let s = lattice_steps(&|x, y| f(img::get(hd, x, y)), hd.width, hd.height, k);
    let hd_ratio = ratio(s[0], s[1]).max(ratio(s[2], s[3]));
    let cl = |x, y| classic.at(x, y).map_or([GREY; 3], |c| c.map(f32::from));
    let c = lattice_steps(&cl, classic.width, classic.height, 1);
    let classic_ratio = ratio(c[0], c[1]).max(ratio(c[2], c[3]));
    (hd_ratio, max_ratio.max(classic_ratio))
}

/// Whether a [`seam`] result passes; a hair over the limit is rounding (an HD tile that is
/// the classic one scaled has the classic ratio).
pub fn seam_passes((ratio, limit): (f32, f32)) -> bool {
    ratio <= limit * 1.001
}

/// Motion kept across consecutive frames: for each pair (classic a, classic b, HD a, HD b) of
/// one shape, the regression of the HD change on the classic change at source resolution
/// (luminance over grey), Σ(ΔH·ΔC) / Σ(ΔC²): 1 when the change is repainted at full size, 0
/// when the frames came out alike. Pairs whose classic frames are equal are skipped. The
/// minimum over the pairs (each held to 0..2; not a number counts as 0): every moving pair must
/// keep its share. `None` without a pair.
pub fn motion(pairs: &[(Classic, Classic, Rgba, Rgba)]) -> Option<f32> {
    let mut kept = Vec::new();
    for (ca, cb, ha, hb) in pairs {
        let k = ha.width / ca.width;
        let (la, lb) = (img::classic_luma(ca, 1), img::classic_luma(cb, 1));
        let (ma, mb) = (img::hd_luma(ha, k), img::hd_luma(hb, k));
        let (mut num, mut den) = (0.0f64, 0.0f64);
        for i in 0..la.v.len() {
            let dc = f64::from(lb.v[i] - la.v[i]);
            let dh = f64::from(mb.v[i] - ma.v[i]);
            num += dh * dc;
            den += dc * dc;
        }
        if den > 1.0 {
            let r = num / den;
            kept.push(if r.is_finite() {
                r.clamp(0.0, 2.0) as f32
            } else {
                0.0
            });
        }
    }
    kept.into_iter().reduce(f32::min)
}

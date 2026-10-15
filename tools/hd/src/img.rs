//! Image plumbing: PNG files, nearest scaling, the classic image type, luminance planes.

use std::path::Path;

pub use u6_hdpack::Rgba;

/// A classic image in true colour: `None` where it is transparent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classic {
    pub width: u32,
    pub height: u32,
    pub px: Vec<Option<[u8; 3]>>,
}

impl Classic {
    pub fn at(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        self.px[(y * self.width + x) as usize]
    }

    pub fn has_clear(&self) -> bool {
        self.px.iter().any(Option::is_none)
    }

    pub fn has_opaque(&self) -> bool {
        self.px.iter().any(Option::is_some)
    }

    /// The opaque mask grown by `n` pixels (8-neighbourhood).
    pub fn dilated(&self, n: u32) -> Vec<bool> {
        let (w, h) = (self.width as i64, self.height as i64);
        let n = i64::from(n);
        let mut out = vec![false; self.px.len()];
        for y in 0..h {
            for x in 0..w {
                if self.px[(y * w + x) as usize].is_none() {
                    continue;
                }
                for dy in -n..=n {
                    for dx in -n..=n {
                        let (xx, yy) = (x + dx, y + dy);
                        if (0..w).contains(&xx) && (0..h).contains(&yy) {
                            out[(yy * w + xx) as usize] = true;
                        }
                    }
                }
            }
        }
        out
    }

    /// Nearest ×`k`, transparent pixels as `key`.
    pub fn scaled(&self, k: u32, key: [u8; 3]) -> Rgba {
        let mut out = Rgba::new(self.width * k, self.height * k);
        for y in 0..out.height {
            for x in 0..out.width {
                let c = self.at(x / k, y / k).unwrap_or(key);
                let i = ((y * out.width + x) * 4) as usize;
                out.pixels[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        out
    }
}

pub fn get(img: &Rgba, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.width + x) * 4) as usize;
    [
        img.pixels[i],
        img.pixels[i + 1],
        img.pixels[i + 2],
        img.pixels[i + 3],
    ]
}

pub fn put(img: &mut Rgba, x: u32, y: u32, c: [u8; 4]) {
    let i = ((y * img.width + x) * 4) as usize;
    img.pixels[i..i + 4].copy_from_slice(&c);
}

/// Fills a rectangle (clipped to the image).
pub fn fill(img: &mut Rgba, x: u32, y: u32, w: u32, h: u32, c: [u8; 4]) {
    for yy in y..(y + h).min(img.height) {
        for xx in x..(x + w).min(img.width) {
            put(img, xx, yy, c);
        }
    }
}

/// Bilinear resize (bitmaps whose sheet scale is below 8).
pub fn resize(img: &Rgba, w: u32, h: u32) -> Rgba {
    let mut out = Rgba::new(w, h);
    let (sx, sy) = (img.width as f32 / w as f32, img.height as f32 / h as f32);
    for y in 0..h {
        let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (img.height - 1) as f32);
        let (y0, ty) = (fy.floor() as u32, fy.fract());
        let y1 = (y0 + 1).min(img.height - 1);
        for x in 0..w {
            let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (img.width - 1) as f32);
            let (x0, tx) = (fx.floor() as u32, fx.fract());
            let x1 = (x0 + 1).min(img.width - 1);
            let (a, b, c, d) = (
                get(img, x0, y0),
                get(img, x1, y0),
                get(img, x0, y1),
                get(img, x1, y1),
            );
            let mut p = [0u8; 4];
            for ch in 0..4 {
                let top = f32::from(a[ch]) * (1.0 - tx) + f32::from(b[ch]) * tx;
                let bot = f32::from(c[ch]) * (1.0 - tx) + f32::from(d[ch]) * tx;
                p[ch] = (top * (1.0 - ty) + bot * ty).round() as u8;
            }
            put(&mut out, x, y, p);
        }
    }
    out
}

/// Box-averages `img` by `k` (alpha-weighted colour, mean alpha), as `pack` turns the ×8
/// masters into the pack's ×4.
pub fn shrink(img: &Rgba, k: u32) -> Rgba {
    let mut out = Rgba::new(img.width / k, img.height / k);
    for y in 0..out.height {
        for x in 0..out.width {
            let (mut c, mut a) = ([0u32; 3], 0u32);
            for yy in y * k..(y + 1) * k {
                for xx in x * k..(x + 1) * k {
                    let p = get(img, xx, yy);
                    let w = u32::from(p[3]);
                    for i in 0..3 {
                        c[i] += u32::from(p[i]) * w;
                    }
                    a += w;
                }
            }
            let n = k * k;
            let px = if a == 0 {
                [0, 0, 0, 0]
            } else {
                let ch = |v: u32| ((v + a / 2) / a) as u8;
                [ch(c[0]), ch(c[1]), ch(c[2]), ((a + n / 2) / n) as u8]
            };
            put(&mut out, x, y, px);
        }
    }
    out
}

/// Luminance (0..255) of a colour.
pub fn luma(c: [u8; 3]) -> f32 {
    0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2])
}

/// The grey transparent pixels are composited over for the luminance planes.
pub const GREY: f32 = 128.0;

/// A plane of floats.
#[derive(Clone, Debug)]
pub struct Plane {
    pub w: usize,
    pub h: usize,
    pub v: Vec<f32>,
}

impl Plane {
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.v[y * self.w + x]
    }

    /// Clamped read.
    pub fn clamped(&self, x: i64, y: i64) -> f32 {
        let x = x.clamp(0, self.w as i64 - 1) as usize;
        let y = y.clamp(0, self.h as i64 - 1) as usize;
        self.at(x, y)
    }
}

/// The classic image's luminance at ×`k`, transparent pixels as [`GREY`].
pub fn classic_luma(c: &Classic, k: u32) -> Plane {
    let (w, h) = ((c.width * k) as usize, (c.height * k) as usize);
    let mut v = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            v.push(c.at(x as u32 / k, y as u32 / k).map_or(GREY, luma));
        }
    }
    Plane { w, h, v }
}

/// The HD image's luminance composited over [`GREY`] by its alpha, box-averaged by `k`.
pub fn hd_luma(img: &Rgba, k: u32) -> Plane {
    let (w, h) = ((img.width / k) as usize, (img.height / k) as usize);
    let mut v = vec![0f32; w * h];
    for y in 0..img.height {
        for x in 0..img.width {
            let p = get(img, x, y);
            let a = f32::from(p[3]) / 255.0;
            let l = luma([p[0], p[1], p[2]]) * a + GREY * (1.0 - a);
            let (bx, by) = ((x / k) as usize, (y / k) as usize);
            if bx < w && by < h {
                v[by * w + bx] += l;
            }
        }
    }
    let n = (k * k) as f32;
    for p in &mut v {
        *p /= n;
    }
    Plane { w, h, v }
}

/// Most bytes of PNG data the tool reads (a 1536 × 1024 RGBA sheet is 6 MiB raw).
pub const PNG_MAX_BYTES: u64 = 64 << 20;

/// A PNG's size from its header, without decoding it.
pub fn png_size(bytes: &[u8]) -> Result<(u32, u32), String> {
    let dec = png::Decoder::new(std::io::Cursor::new(bytes));
    let reader = dec.read_info().map_err(|e| e.to_string())?;
    let info = reader.info();
    Ok((info.width, info.height))
}

/// Decodes PNG `bytes` that must be `width × height` (checked from the header before anything
/// is allocated for the pixels) as RGBA; the decoder is held to the bytes that size needs.
pub fn decode_png(bytes: &[u8], width: u32, height: u32) -> Result<Rgba, String> {
    if bytes.len() as u64 > PNG_MAX_BYTES {
        return Err("PNG file too large".into());
    }
    let n = (width as usize)
        .checked_mul(height as usize)
        .ok_or("size overflows")?;
    let rgba = n.checked_mul(4).ok_or("size overflows")?;
    let mut dec = png::Decoder::new_with_limits(
        std::io::Cursor::new(bytes),
        png::Limits {
            bytes: rgba.checked_add(1 << 20).ok_or("size overflows")?,
        },
    );
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let (w, h) = (reader.info().width, reader.info().height);
    if (w, h) != (width, height) {
        return Err(format!("{w}x{h}, expected {width}x{height}"));
    }
    let size = reader
        .output_buffer_size()
        .ok_or("PNG output size overflows")?;
    if size > rgba {
        return Err("PNG output larger than RGBA".into());
    }
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let take = |k: usize| buf.get(..n * k).ok_or("PNG frame shorter than its size");
    let pixels = match info.color_type {
        png::ColorType::Rgba => take(4)?.to_vec(),
        png::ColorType::Rgb => take(3)?
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => take(1)?.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => take(2)?
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => return Err("palette not expanded".into()),
    };
    Ok(Rgba {
        width,
        height,
        pixels,
    })
}

/// Reads a PNG file that must be `width × height`.
pub fn read_png(path: &Path, width: u32, height: u32) -> Result<Rgba, String> {
    let bytes = read_capped(path, PNG_MAX_BYTES)?;
    decode_png(&bytes, width, height).map_err(|e| format!("{}: {e}", path.display()))
}

/// Reads a file of at most `max` bytes.
pub fn read_capped(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let len = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if len > max {
        return Err(format!("{}: larger than {max} bytes", path.display()));
    }
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The PNG encoding of `img`.
pub fn encode_png(img: &Rgba) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.width, img.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&img.pixels).map_err(|e| e.to_string())?;
        w.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Writes `img` as PNG to `path` through the tool's one safe write ([`crate::work`]).
pub fn write_png(path: &Path, img: &Rgba) -> Result<(), String> {
    let bytes = encode_png(img).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::work::write_file(path, &bytes)
}

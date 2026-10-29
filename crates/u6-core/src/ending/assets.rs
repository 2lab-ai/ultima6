//! Runtime-only resources of `end`. No original strings or pictures are compiled in.
use crate::intro::{
    load,
    mem::{Chip, Ptr, Region, item_header},
};
use crate::{
    Files,
    image::{GameImage, ImageLayout},
};
use u6_formats::font::WouFont;

pub const A4: u32 = 0x0010_b8f0;

/// Preflight completes before the cube removes anything. Errors contain no asset contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Executable,
    Pictures,
    Font,
}

pub struct Assets {
    file: Vec<u8>,
    layout: ImageLayout,
    pub pictures: Vec<u8>,
    pub font: WouFont,
    pub palette: [u16; 32],
    pub songs: [String; 3],
    pub dissolve: Vec<(u16, u16)>,
}
impl Assets {
    pub fn load(files: &Files) -> Result<Self, Error> {
        let file = files.get("end").ok_or(Error::Executable)?;
        let img = GameImage::parse(file).map_err(|_| Error::Executable)?;
        if img.segment_base(14) != Some(A4) {
            return Err(Error::Executable);
        }
        let bytes = img.bytes(A4 + 0x4f2, 64).map_err(|_| Error::Executable)?;
        let palette = std::array::from_fn(|i| u16::from_be_bytes([bytes[2 * i], bytes[2 * i + 1]]));
        let mut songs = Vec::new();
        for n in 10..=12 {
            let p = img
                .pointer(A4 + 0x246 + n * 4)
                .map_err(|_| Error::Executable)?;
            let name = checked_string(&img, p)?;
            songs.push(String::from_utf8(name).map_err(|_| Error::Executable)?);
        }
        for &addr in super::passages::STRINGS
            .iter()
            .chain(&[0x103848, 0x10384e, 0x103850, 0x103858])
        {
            checked_string(&img, addr)?;
        }
        // Number words have 13-byte stride. Main uses the signed elapsed values literally.
        img.bytes(A4 + 0xc4, 29 * 13)
            .map_err(|_| Error::Executable)?;
        let feedback = img
            .bytes(0x10a15a + 12 * 4, 4)
            .map_err(|_| Error::Executable)?;
        let feedback = u32::from_be_bytes(feedback.try_into().map_err(|_| Error::Executable)?);
        let dissolve = dissolve_order(113, 94, feedback).ok_or(Error::Executable)?;
        let pictures = load::load_lzw(files, b"end.new").ok_or(Error::Pictures)?;
        validate_pictures(&pictures)?;
        let font = load::load_lzw(files, b"u6.set")
            .and_then(|b| WouFont::parse(&b).ok())
            .ok_or(Error::Font)?;
        if font.height == 0 || font.height > 32 || !(32..127).all(|c| font.glyphs[c].width > 0) {
            return Err(Error::Font);
        }
        Ok(Self {
            file: file.to_vec(),
            layout: img.into_layout(),
            pictures,
            font,
            palette,
            songs: songs.try_into().map_err(|_| Error::Executable)?,
            dissolve,
        })
    }
    // End has a separate executable/address space; B13's game registry excludes it.
    #[allow(clippy::disallowed_methods)]
    pub fn string(&self, addr: u32) -> Vec<u8> {
        GameImage::with_layout(&self.file, &self.layout)
            .cstr(addr)
            .unwrap_or_default()
    }
    pub fn duration(&self, n: i16, unit: u32, optional: bool) -> Vec<u8> {
        if optional && n == 0 {
            return Vec::new();
        }
        let addr = A4
            .wrapping_add(0xc4)
            .wrapping_add((i32::from(n) * 13) as u32);
        let mut text = self.string(addr);
        text.extend(self.string(unit));
        if n != 1 {
            text.extend(self.string(0x10384e));
        }
        text
    }
}
// End has a separate executable/address space; this is its validated Assets loader.
#[allow(clippy::disallowed_methods)]
fn checked_string(img: &GameImage<'_>, addr: u32) -> Result<Vec<u8>, Error> {
    let text = img.cstr(addr).map_err(|_| Error::Executable)?;
    if text.is_empty() || text.iter().any(|b| !matches!(b, 9 | 10 | 13 | 32..=127)) {
        return Err(Error::Executable);
    }
    Ok(text)
}
fn validate_pictures(bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() < 60 || bytes.len() > Region::Files.size() {
        return Err(Error::Pictures);
    }
    let long = |o: usize| u32::from_be_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
    let table = long(0);
    if table < 60 || table % 4 != 0 || table > bytes.len() {
        return Err(Error::Pictures);
    }
    let mut chip = Chip::new();
    chip.write(Ptr::at(Region::Files), bytes);
    for i in 0..15 {
        let at = long(i * 4);
        if at < table || at + 10 > bytes.len() {
            return Err(Error::Pictures);
        }
        let h = item_header(&chip, Ptr::at(Region::Files).add(at as u32));
        let size = h.row_bytes() * h.rows() * ((h.flags & 31).count_ones() as usize + 1);
        if h.x1 >= 320 || h.y1 >= 200 || at + 10 + size > bytes.len() {
            return Err(Error::Pictures);
        }
    }
    Ok(())
}
/// Original: end 0x10a1d6. The 32-bit feedback register partitions its bits into columns
/// and rows, unlike game's 16-bit linear-index dissolve. A corrupt feedback table is refused.
fn dissolve_order(w: u16, h: u16, feedback: u32) -> Option<Vec<(u16, u16)>> {
    let xb = 16 - w.leading_zeros();
    let yb = 16 - h.leading_zeros();
    let mask = (1u32 << xb) - 1;
    let mut r = (1u32 << (xb + yb)) - 1;
    let mut out = Vec::new();
    let mut seen = vec![false; usize::from(w) * usize::from(h)];
    for _ in 0..(1u32 << (xb + yb)) {
        r = (r >> 1) ^ if r & 1 != 0 { feedback } else { 0 };
        let (x, y) = ((r & mask) as u16, (r >> xb) as u16);
        if x < w && y < h {
            let i = usize::from(y) * usize::from(w) + usize::from(x);
            if seen[i] || (x, y) == (0, 0) {
                return None;
            }
            seen[i] = true;
            out.push((x, y));
            if out.len() == usize::from(w) * usize::from(h) - 1 {
                out.push((0, 0));
                return Some(out);
            }
        }
    }
    None
}

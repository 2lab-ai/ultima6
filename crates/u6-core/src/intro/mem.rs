//! The chip-memory buffers of `u` and the blitter routines that draw into them.
//!
//! `u` draws into planar bitmaps the way the hardware displays them: 5 bitplanes of 320×200,
//! 40 bytes a row, one plane after the other (8000 bytes apart, 40000 bytes a screen). The port
//! keeps those bytes as they are (raw images load straight into them, several routines write
//! plane bytes) and reads them back as 5-bit palette indices when it shows a frame. The
//! original addresses buffers by pointer; the port names each allocation (0x113bfe) as a
//! [`Region`] and a pointer as a region plus an offset.
//!
//! A [`View`] is the original's 16-byte rectangle record: planes pointer, row-offset table
//! (always the `y × 40` table at A4+0x2436 that 0x11547e builds, so the port computes it), then
//! the words x1, x0, y0, y1 (inclusive).

/// Bytes in one bitplane row.
pub const ROW: usize = 40;
/// Bytes in one 320×200 bitplane.
pub const PLANE: usize = 8000;
/// Bytes in a 5-plane screen.
pub const SCREEN: usize = 5 * PLANE;
/// Screen width and height in pixels.
pub const W: i32 = 320;
pub const H: i32 = 200;

/// One allocation of 0x113bfe (sizes in the doc of each variant).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Region {
    /// Original: A4+0x2d46 (= A4+0x2e32[0]; screen buffer 0, 40000 bytes, chip, cleared)
    #[default]
    Screen0,
    /// Original: A4+0x2d4a (= A4+0x2e32[1]; screen buffer 1, 40000 bytes)
    Screen1,
    /// Original: A4+0x2e1e (work bitmap, 40000 bytes)
    Work,
    /// Original: A4+0x2e22 (48000 bytes: 5 planes, then a mask plane at +40000)
    Composite,
    /// Original: A4+0x2e2e (40000 bytes)
    Spare,
    /// Original: A4+0x2e26 (249100 bytes: the `.new` files of a scene)
    Files,
    /// Original: A4+0x2e2a (36056 bytes: `blocks.new`, the scroll frames of the passages)
    Blocks,
}

impl Region {
    const ALL: [Region; 7] = [
        Region::Screen0,
        Region::Screen1,
        Region::Work,
        Region::Composite,
        Region::Spare,
        Region::Files,
        Region::Blocks,
    ];

    /// Allocated size (0x113bfe's AllocMem calls).
    pub const fn size(self) -> usize {
        match self {
            Region::Screen0 | Region::Screen1 | Region::Work | Region::Spare => SCREEN,
            Region::Composite => 48_000,
            Region::Files => 0x3_cd0c,
            Region::Blocks => 0x8cd8,
        }
    }

    fn index(self) -> usize {
        Region::ALL.iter().position(|&r| r == self).unwrap_or(0)
    }
}

/// A pointer into chip memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Ptr {
    pub region: Region,
    pub offset: u32,
}

impl Ptr {
    pub const fn at(region: Region) -> Self {
        Self { region, offset: 0 }
    }

    pub const fn add(self, n: u32) -> Self {
        Self {
            region: self.region,
            offset: self.offset + n,
        }
    }
}

/// The original's rectangle record (16 bytes, copied by value everywhere).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct View {
    /// +0: the bitmap drawn into or read from
    pub planes: Ptr,
    /// +8: right edge (inclusive)
    pub x1: i16,
    /// +0xa: left edge
    pub x0: i16,
    /// +0xc: top edge
    pub y0: i16,
    /// +0xe: bottom edge (inclusive)
    pub y1: i16,
}

impl View {
    /// The full screen 0..319 × 0..199 of `planes` (main's A4+0x1784 setup).
    pub const fn screen(planes: Ptr) -> Self {
        Self {
            planes,
            x1: 0x13f,
            x0: 0,
            y0: 0,
            y1: 0xc7,
        }
    }

    pub const fn width(&self) -> i16 {
        self.x1 - self.x0 + 1
    }

    pub const fn height(&self) -> i16 {
        self.y1 - self.y0 + 1
    }
}

/// All allocations, zero-filled (AllocMem with MEMF_CLEAR).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    mem: Vec<Vec<u8>>,
}

impl Default for Chip {
    fn default() -> Self {
        Self::new()
    }
}

impl Chip {
    pub fn new() -> Self {
        Self {
            mem: Region::ALL.iter().map(|r| vec![0; r.size()]).collect(),
        }
    }

    /// The bytes from `p` to the end of its region (empty past the end).
    pub fn bytes(&self, p: Ptr) -> &[u8] {
        let m = &self.mem[p.region.index()];
        m.get(p.offset as usize..).unwrap_or(&[])
    }

    /// Mutable bytes from `p` to the end of its region.
    pub fn bytes_mut(&mut self, p: Ptr) -> &mut [u8] {
        let m = &mut self.mem[p.region.index()];
        let o = (p.offset as usize).min(m.len());
        &mut m[o..]
    }

    /// Byte at `p + i` (0 past the end).
    pub fn byte(&self, p: Ptr, i: usize) -> u8 {
        self.bytes(p).get(i).copied().unwrap_or(0)
    }

    /// Big-endian word at `p + i`.
    pub fn word(&self, p: Ptr, i: usize) -> u16 {
        u16::from_be_bytes([self.byte(p, i), self.byte(p, i + 1)])
    }

    /// Big-endian longword at `p + i`.
    pub fn long(&self, p: Ptr, i: usize) -> u32 {
        (u32::from(self.word(p, i)) << 16) | u32::from(self.word(p, i + 2))
    }

    /// Copies `data` to `p` (clipped to the region).
    pub fn write(&mut self, p: Ptr, data: &[u8]) {
        let dst = self.bytes_mut(p);
        let n = data.len().min(dst.len());
        dst[..n].copy_from_slice(&data[..n]);
    }

    /// The 5-bit colour of pixel (x, y) of the bitmap at `planes`.
    pub fn pixel(&self, planes: Ptr, x: i32, y: i32) -> u8 {
        if !(0..W).contains(&x) || !(0..H).contains(&y) {
            return 0;
        }
        let (i, bit) = ((y * 40 + x / 8) as usize, 7 - (x % 8) as u32);
        let b = self.bytes(planes);
        (0..5).fold(0, |c, p| {
            let v = b.get(p * PLANE + i).copied().unwrap_or(0);
            c | (((v >> bit) & 1) << p)
        })
    }

    /// Sets pixel (x, y) of the bitmap at `planes` to `colour` (bits 0..4 = planes 0..4).
    pub fn set_pixel(&mut self, planes: Ptr, x: i32, y: i32, colour: u8) {
        if !(0..W).contains(&x) || !(0..H).contains(&y) {
            return;
        }
        let (i, bit) = ((y * 40 + x / 8) as usize, 7 - (x % 8) as u32);
        let b = self.bytes_mut(planes);
        for p in 0..5 {
            if let Some(v) = b.get_mut(p * PLANE + i) {
                if colour >> p & 1 != 0 {
                    *v |= 1 << bit;
                } else {
                    *v &= !(1 << bit);
                }
            }
        }
    }
}

/// One item of a `.new` file as 0x1163fc reads it: the 10-byte header (words x0, x1, y0, y1,
/// plane flags), the planes whose flag bit is set (row = `((x1 >> 4) + 1) × 2` bytes, `y1 + 1`
/// rows each), then one mask plane of the same size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemHeader {
    /// Hot spot: the item is drawn at (x − x0, y − y0).
    pub x0: i16,
    pub x1: u16,
    pub y0: i16,
    pub y1: u16,
    /// Bit p set = plane p is stored; a clear bit clears that plane under the mask.
    pub flags: u16,
}

impl ItemHeader {
    pub const fn row_bytes(&self) -> usize {
        (((self.x1 >> 4) + 1) * 2) as usize
    }

    pub const fn rows(&self) -> usize {
        self.y1.wrapping_add(1) as usize
    }
}

/// Where item `index` of the `.new` file loaded at `file` starts, or `None` when the index is
/// past the offset table (0x1163fc: `index × 4 >= ` the low word of the first offset, which is
/// the table's own size).
pub fn item_at(chip: &Chip, file: Ptr, index: u16) -> Option<Ptr> {
    let limit = chip.word(file, 2) as i16;
    let i4 = index.wrapping_shl(2) as i16;
    if i4 >= limit {
        return None;
    }
    let off = chip.long(file, usize::from(index) * 4);
    Some(file.add(off))
}

/// The header of the item at `item`.
pub fn item_header(chip: &Chip, item: Ptr) -> ItemHeader {
    ItemHeader {
        x0: chip.word(item, 0) as i16,
        x1: chip.word(item, 2),
        y0: chip.word(item, 4) as i16,
        y1: chip.word(item, 6),
        flags: chip.word(item, 8),
    }
}

/// Original: u 0x1163fc (intro graphics: draw item `index` of the `.new` file at `file` into
/// `dst` at (x, y))
///
/// Finds the item, takes its hot spot off the position and runs the masked blit 0x1199ba
/// ([`blit_masked`]) with the clip height `dst.y1 + 1`. An index past the table draws nothing.
pub fn draw_item(chip: &mut Chip, dst: &View, x: i16, y: i16, file: Ptr, index: u16) {
    let Some(item) = item_at(chip, file, index) else {
        return;
    };
    let h = item_header(chip, item);
    let dx = x.wrapping_sub(h.x0);
    let dy = y.wrapping_sub(h.y0);
    blit_masked(chip, dst.planes, item, &h, dx, dy, dst.y1.wrapping_add(1));
}

/// Original: u 0x1199ba (intro graphics: masked blit of an item's planes, clipped to the
/// screen width and to rows 0..ymax)
///
/// Where the mask bit is set, each destination plane takes the item's plane when its flag bit
/// is set and is cleared when it is not; elsewhere the destination is kept. Nothing is drawn
/// when the item starts at or below `ymax`, at or right of x = 320, or ends left of x = 0.
pub fn blit_masked(
    chip: &mut Chip,
    dst: Ptr,
    item: Ptr,
    h: &ItemHeader,
    dx: i16,
    dy: i16,
    ymax: i16,
) {
    let row_bytes = h.row_bytes();
    let rows = h.rows();
    let width = (row_bytes * 8) as i32;
    let (dx, dy, ymax) = (i32::from(dx), i32::from(dy), i32::from(ymax));
    if dy >= ymax || dx + width < 0 || dx >= W {
        return;
    }
    let plane_size = row_bytes * rows;
    let stored: Vec<usize> = (0..5).filter(|p| h.flags >> p & 1 != 0).collect();
    let data = item.add(10);
    let mask_at = stored.len() * plane_size;
    let src = chip.bytes(data).to_vec();
    let bit = |plane_off: usize, r: usize, c: i32| -> bool {
        let i = plane_off + r * row_bytes + (c / 8) as usize;
        src.get(i).is_some_and(|b| b >> (7 - c % 8) & 1 != 0)
    };
    for r in 0..rows {
        let y = dy + r as i32;
        if y < 0 {
            continue;
        }
        if y >= ymax {
            break;
        }
        for c in 0..width {
            let x = dx + c;
            if !(0..W).contains(&x) || !bit(mask_at, r, c) {
                continue;
            }
            let mut colour = 0u8;
            for (k, &p) in stored.iter().enumerate() {
                if bit(k * plane_size, r, c) {
                    colour |= 1 << p;
                }
            }
            chip.set_pixel(dst, x, y, colour);
        }
    }
}

/// Original: u 0x116180 (intro graphics: blitter copy of a w×h rectangle of 5 planes)
///
/// `and` set (the last argument) ANDs the source into the destination instead of copying, except
/// in the aligned fast path (both x on a 16-pixel boundary and w a multiple of 16), which always
/// copies.
#[allow(clippy::too_many_arguments)]
pub fn copy_rect(
    chip: &mut Chip,
    src: Ptr,
    (sx, sy): (i16, i16),
    (w, h): (i16, i16),
    dst: Ptr,
    (dx, dy): (i16, i16),
    and: bool,
) {
    let fast = sx & 15 == 0 && dx & 15 == 0 && w & 15 == 0;
    let and = and && !fast;
    let mut px = Vec::with_capacity(usize::try_from(i32::from(w) * i32::from(h)).unwrap_or(0));
    for r in 0..i32::from(h) {
        for c in 0..i32::from(w) {
            px.push(chip.pixel(src, i32::from(sx) + c, i32::from(sy) + r));
        }
    }
    let mut it = px.into_iter();
    for r in 0..i32::from(h) {
        for c in 0..i32::from(w) {
            let v = it.next().unwrap_or(0);
            let (x, y) = (i32::from(dx) + c, i32::from(dy) + r);
            let v = if and { v & chip.pixel(dst, x, y) } else { v };
            chip.set_pixel(dst, x, y, v);
        }
    }
}

/// Original: u 0x115588 (intro graphics: copy the rectangle of `src` to `dst`, the smaller of
/// the two sizes)
pub fn copy_view(chip: &mut Chip, src: &View, dst: &View) {
    let w = (src.width() as u16).min(dst.width() as u16) as i16;
    let h = (src.height() as u16).min(dst.height() as u16) as i16;
    copy_rect(
        chip,
        src.planes,
        (src.x0, src.y0),
        (w, h),
        dst.planes,
        (dst.x0, dst.y0),
        false,
    );
}

/// Original: u 0x116326 (intro graphics: fill a w×h rectangle with `colour`; nothing when w or
/// h is not positive)
pub fn fill_rect(chip: &mut Chip, planes: Ptr, (x, y): (i16, i16), (w, h): (i16, i16), colour: u8) {
    if w <= 0 || h <= 0 {
        return;
    }
    for r in 0..i32::from(h) {
        for c in 0..i32::from(w) {
            chip.set_pixel(planes, i32::from(x) + c, i32::from(y) + r, colour);
        }
    }
}

/// Original: u 0x10f898 (intro graphics: fill the whole rectangle of `view` with `colour`)
pub fn fill_view(chip: &mut Chip, view: &View, colour: u8) {
    fill_rect(
        chip,
        view.planes,
        (view.x0, view.y0),
        (view.width(), view.height()),
        colour,
    );
}

/// Bytes of the composite bitmap before its mask plane (A4+0x2e22: 5 planes, then the mask).
const MASK_AT: usize = SCREEN;

/// Original: u 0x1165c2 (intro graphics: the composite's mask plane = the OR of its five planes,
/// so every pixel that is not colour 0 is opaque)
pub fn composite_mask(chip: &mut Chip) {
    let b = chip.bytes_mut(Ptr::at(Region::Composite));
    for i in 0..PLANE {
        let m = (0..5).fold(0u8, |m, p| m | b[p * PLANE + i]);
        b[MASK_AT + i] = m;
    }
}

/// The composite overlay of 0x11653c / 0x116490: per byte, where the mask is set the
/// destination planes are cleared and the composite's planes ORed in; source column `col + skip`
/// lands on destination column `col`.
fn overlay(chip: &mut Chip, dst: Region, skip: usize) {
    let src = chip.bytes(Ptr::at(Region::Composite)).to_vec();
    let d = chip.bytes_mut(Ptr::at(dst));
    for row in 0..200 {
        for col in 0..(ROW - skip) {
            let s = row * ROW + col + skip;
            let t = row * ROW + col;
            let keep = !src[MASK_AT + s];
            for p in 0..5 {
                let at = p * PLANE + t;
                d[at] = (d[at] & keep) | src[p * PLANE + s];
            }
        }
    }
}

/// Original: u 0x11653c (intro graphics: overlay the composite bitmap on the buffer drawn into,
/// A4+0x2d52)
pub fn overlay_composite(chip: &mut Chip, dst: Region) {
    overlay(chip, dst, 0);
}

/// Original: u 0x116490 (intro graphics: as 0x11653c with the composite moved `x` pixels left,
/// in whole bytes; nothing from x = 320 on)
pub fn overlay_composite_left(chip: &mut Chip, dst: Region, x: i16) {
    let skip = usize::from((x as u16) >> 3);
    if skip >= ROW {
        return;
    }
    overlay(chip, dst, skip);
}

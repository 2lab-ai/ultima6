//! Spec 07 HD0: the HD compositor. From the frame shown (its colours, its palette and its source
//! plane) and the images the plane names, [`Compositor::compose`] writes the frame at S× (S = 2
//! or 4, [`Scale`]): each screen pixel becomes an S × S block,
//!
//! - for a pixel of a tile record, a portrait or a panel bitmap without flags, drawn at the
//!   image's current version, the block `(u·S.., v·S..)` of that image's HD version, where an HD
//!   sub-pixel that is transparent takes the pixel's classic colour;
//! - for a glyph pixel drawn over the paper ([`FLAG_PAPER`]), without ink ([`FLAG_INK`]), whose
//!   colour is the paper's at its own place (the paper bitmap, [`BITMAP_PAPER`], is drawn at
//!   (0, 0)), the block `(x·S.., y·S..)` of the paper's HD version, so the text windows show the
//!   HD paper in place behind the text (and text over a portrait or the spellbook does not);
//! - for every other pixel (classic, glyph, flagged, stale), its classic colour.
//!
//! Spec 11, the HD font mode ([`Compositor::set_font_mode`]): on, each 8 × 8 cell of the screen
//! grid whose 64 pixels are the glyph of one code drawn whole ([`Kind::Glyph`], (u, v) its place
//! in the cell, one ink and one paper colour, both present) and whose code is printable ASCII
//! but the space or a Hangul syllable ([`hd_font_code`]) is filled with its paper colour and
//! listed ([`Compositor::glyphs`]): the platform draws the character over it in its HD font.
//! The fill is the paper's HD block at each of its pixels drawn over the paper ([`FLAG_PAPER`])
//! where the paper's colour there is the cell's paper colour, as a clear glyph pixel's is.
//! A whole space is listed too (code 0x20, its ink its paper, [`HdGlyph::code`]): the glyph of
//! the space drawn whole, no pixel of it ink, all 64 of one colour. It is filled with its paper
//! like a listed letter, which leaves its pixels as they were, and nothing is drawn for it: it
//! tells the platform that the text writer drew a space there, which no other blank cell is, so
//! a run of text drawn at the font's own width may cross it. Every other cell is composed as
//! with the mode off.
//!
//! An image's HD version (spec 07 "HD image per id"):
//!
//! - the image of the pack set ([`Compositor::set_pack`], spec 07 H2) made from the image's
//!   content now: [`Images::key`] gives the content key, [`Pack::hd`] the pack's ×4 RGBA for it,
//!   taken as it is at 4× and 2 × 2 box-averaged at 2× ([`pack_words`]), alpha included: a
//!   sub-pixel with alpha between 0 and 255 is mixed over the pixel's classic colour ([`over`]);
//! - otherwise MMPX ×S ([`mmpx`]) of its current pixels in the frame's palette, a transparent
//!   pixel transparent.
//!
//! Either is a pure function of those pixels, the palette, S and the pack, cached by their
//! content ([`HdImages`], at most [`HD_CACHE_BYTES`]); an entry made from a pack image is keyed by
//! that image (the pack's generation and the image's index) as well as by the content, so a pack
//! set or removed never shows an entry of another. The compositor keeps the inputs of its last
//! composition and writes again only the blocks whose inputs changed; what it writes is what a
//! new compositor would write from the same frame and the same pack.
//!
//! Memory: the HD images within the bound (each with the indices it was last reached from, which
//! [`HdImages::bytes`] counts; [`HdImages::peak`] is the retained cache payload peak, the most it
//! held, also during a composition whose images alone exceed the bound); outside it, the 2×
//! intermediate of a 4× magnification while it is made, the frame (`WIDTH·S × HEIGHT·S`
//! words), the last composition's inputs (a [`Src`], an index and a token per screen pixel, the
//! glyph per cell) and its list of glyphs, and a few words per image named so far: per tile
//! record up to the largest record id named (the game's records are 2048), per portrait (letter,
//! item) and per panel bitmap (two) named. The pack's own images are the platform's ([`Pack`]);
//! the compositor holds a reference to it.
//!
//! The output is packed RGBA words, `u32::from_le_bytes([r, g, b, a])`: in wasm memory (little
//! endian) the bytes the page's `ImageData` takes.

mod mmpx;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::rc::Rc;

use crate::{BITMAP_PAPER, FLAG_INK, FLAG_PAPER, Framebuffer, HEIGHT, Kind, Rgb, Src, WIDTH};

pub use mmpx::{mmpx, mmpx2x};

/// The compositor's scales (spec 07 rev 5): MMPX is a 2× scaler, 4× is MMPX applied twice; there
/// is no 3×.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scale {
    X2,
    X4,
}

impl Scale {
    /// 2 or 4.
    pub const fn factor(self) -> usize {
        match self {
            Scale::X2 => 2,
            Scale::X4 => 4,
        }
    }

    /// [`Scale::X4`] for 4, [`Scale::X2`] for any other value.
    pub const fn from_factor(s: u32) -> Scale {
        if s == 4 { Scale::X4 } else { Scale::X2 }
    }
}

/// The largest image the compositor magnifies, width × height: the paper bitmap, the largest
/// image the game draws, the game's 320 × 200 screen. In both builds: the frame of Wide mode
/// (spec 13, [`WIDTH`] × [`HEIGHT`] 912 × 336) holds no larger image.
pub const IMAGE_MAX: (usize, usize) = (320, 200);

/// An image as the game holds it now: palette indices, row-major, at most [`IMAGE_MAX`] (a
/// larger one is shown classic).
pub struct Image<'a> {
    /// The image's version now ([`Src::ver`]): a pixel drawn at another version is stale.
    pub ver: u32,
    pub width: usize,
    pub height: usize,
    /// `width * height` palette indices.
    pub pixels: &'a [u8],
    /// Per pixel, true where the blit leaves the screen as it is (a tile record's
    /// transparency); `None` for an opaque image.
    pub transparent: Option<&'a [bool]>,
}

/// The images a source plane names ([`Src::kind`], [`Src::id`]), as the game holds them now.
/// [`Compositor::compose`] asks once per composition for each image it shows; `None` shows the
/// image's pixels classic.
pub trait Images {
    fn image(&mut self, kind: Kind, id: u16) -> Option<Image<'_>>;

    /// The content key a pack keys image (kind, id) by, as the image is now (spec 07 HD1: the
    /// content hash of the classic image as the engine holds it, `u6_hdpack`'s); `None` when it
    /// has none. Asked only with a pack set, right after [`Images::image`] gave the image, and
    /// only when the image's HD version is not the one it had last: while the image gives the
    /// same pixels and transparency its key is taken to be the same, so a key must change only
    /// with them, or the platform calls [`Compositor::forget`] when it may change otherwise (a
    /// tile's key is of its pixels and transparency; a portrait's or a bitmap's is of its bytes
    /// before the colour table, which within one game are those of one file).
    fn key(&mut self, _kind: Kind, _id: u16) -> Option<u64> {
        None
    }
}

/// HD pixels per classic pixel of a pack image (`u6_hdpack::SCALE`).
pub const PACK_SCALE: usize = 4;

/// Spec 07 H2: the HD images of a pack, by the classic image they were made from. The platform
/// implements it over the pack it loaded and gives it to [`Compositor::set_pack`].
pub trait Pack {
    /// The pack's image for image (kind, id) whose content key is `key` now ([`Images::key`]),
    /// when the pack has one made from that content. One whose size is not [`PACK_SCALE`]× the
    /// image's is not used.
    fn hd(&self, kind: Kind, id: u16, key: u64) -> Option<PackHd<'_>>;
}

/// One image of a pack: RGBA8, row-major, `width × height` (×4 of its classic image), alpha
/// included.
#[derive(Clone, Copy, Debug)]
pub struct PackHd<'a> {
    /// The image's place in the pack: no two of its images share it.
    pub index: u32,
    pub width: usize,
    pub height: usize,
    /// `width * height * 4` bytes.
    pub rgba: &'a [u8],
}

/// No images: every pixel classic.
pub struct NoImages;

impl Images for NoImages {
    fn image(&mut self, _: Kind, _: u16) -> Option<Image<'_>> {
        None
    }
}

/// The bound on the HD images kept ([`HdImages`]): their input and output words. The images of
/// one screen at 4× (the paper bitmap and a few hundred tiles) take about half of it.
pub const HD_CACHE_BYTES: usize = 24 << 20;

/// The packed word of an opaque colour.
pub const fn word(c: Rgb) -> u32 {
    u32::from_le_bytes([c.r, c.g, c.b, 255])
}

/// The word of a transparent pixel in an image given to MMPX (alpha 0).
const CLEAR: u32 = 0;

/// The HD sub-pixel `c` over the pixel's classic colour `classic` (both packed words): `c` when
/// it is opaque, `classic` when it is transparent, else each channel mixed by `c`'s alpha `a`,
/// `(c·a + classic·(255 − a) + 127) / 255`, opaque. MMPX's sub-pixels are opaque or transparent;
/// a pack image's can be in between (the tool box-averages its ×8 masters).
#[inline]
pub fn over(c: u32, classic: u32) -> u32 {
    let a = c >> 24;
    if a == 255 {
        return c;
    }
    if a == 0 {
        return classic;
    }
    let mix = |shift: u32| {
        let (x, y) = ((c >> shift) & 255, (classic >> shift) & 255);
        ((x * a + y * (255 - a) + 127) / 255) << shift
    };
    mix(0) | mix(8) | mix(16) | 0xff00_0000
}

/// A pack image (`width × height` RGBA8 bytes, ×4 of its classic image) as the HD image at
/// `scale`, in packed words: its own pixels at 4×; at 2× each 2 × 2 block box-averaged, the colour
/// weighted by alpha and the alpha the mean, each rounded to nearest (a block with no alpha is
/// transparent): the rule the HD tool uses for its ×8 masters (`tools/hd` `img::shrink`).
pub fn pack_words(rgba: &[u8], width: usize, height: usize, scale: Scale, out: &mut Vec<u32>) {
    out.clear();
    debug_assert_eq!(rgba.len(), width * height * 4);
    let px = |x: usize, y: usize| -> [u8; 4] {
        let at = (y * width + x) * 4;
        [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
    };
    match scale {
        Scale::X4 => out.extend(
            rgba.chunks_exact(4)
                .map(|p| u32::from_le_bytes([p[0], p[1], p[2], p[3]])),
        ),
        Scale::X2 => {
            let (w, h) = (width / 2, height / 2);
            out.reserve(w * h);
            for y in 0..h {
                for x in 0..w {
                    let (mut c, mut a) = ([0u32; 3], 0u32);
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let p = px(2 * x + dx, 2 * y + dy);
                        let weight = u32::from(p[3]);
                        for (sum, &v) in c.iter_mut().zip(&p[..3]) {
                            *sum += u32::from(v) * weight;
                        }
                        a += weight;
                    }
                    out.push(if a == 0 {
                        CLEAR
                    } else {
                        let ch = |v: u32| ((v + a / 2) / a) as u8;
                        u32::from_le_bytes([ch(c[0]), ch(c[1]), ch(c[2]), ((a + 2) / 4) as u8])
                    });
                }
            }
        }
    }
}

/// The palette indices and transparency an HD image was last reached from, under the palette of
/// composition generation `palette`: an image whose pixels are these again, under the same
/// palette, has this HD image with no lookup by content (with a pack, only for an image whose
/// memo was made under the same pack: [`Slot::memo`]).
struct Source {
    pixels: Vec<u8>,
    transparent: Option<Vec<bool>>,
    palette: u64,
}

impl Source {
    fn of(img: &Image<'_>, palette: u64) -> Source {
        Source {
            pixels: img.pixels.to_vec(),
            transparent: img.transparent.map(<[bool]>::to_vec),
            palette,
        }
    }

    fn bytes(&self) -> usize {
        self.pixels.len() + self.transparent.as_ref().map_or(0, Vec::len)
    }
}

/// What an HD image was made from: MMPX of its content, or the image of the pack set at its index
/// ([`PackHd::index`]). Every entry made from a pack is dropped when another pack is set or the
/// pack is removed ([`HdImages::drop_packed`], which also frees them), so an index names an image
/// of the pack set now: no generation is kept here. The image's memo keeps one
/// ([`Slot::memo`]), since MMPX entries outlive the pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    Mmpx,
    Pack(u32),
}

/// One HD image: the content it was made from and the result.
struct Entry {
    /// Never reused: an equal token is the same image ([`Compositor`] compares them).
    token: u32,
    hash: u64,
    origin: Origin,
    scale: Scale,
    width: usize,
    height: usize,
    /// The image's pixels as words ([`CLEAR`] where transparent).
    input: Vec<u32>,
    /// `scale·width × scale·height` words.
    hd: Vec<u32>,
    /// What it was last reached from.
    source: Source,
    /// The composition that used it last ([`HdImages::stamp`]).
    used: u64,
}

impl Entry {
    fn bytes(&self) -> usize {
        (self.input.len() + self.hd.len()) * 4 + self.source.bytes()
    }
}

/// The lookup hash of an image's words, its shape and the scale.
type Hasher = fn(&[u32], usize, usize, Scale) -> u64;

/// HD images by content: the image's pixels as words and the scale give the HD version, made
/// once and kept until the bound needs room for another. Least recently used goes first; one in
/// use by the composition running stays, so a screen whose images alone exceed the bound keeps
/// them until the composition ends, then the cache shrinks back under it.
pub struct HdImages {
    slab: Vec<Option<Entry>>,
    free: Vec<usize>,
    by_hash: HashMap<u64, Vec<usize>>,
    bytes: usize,
    /// The most `bytes` was.
    peak: usize,
    cap: usize,
    next_token: u32,
    /// The composition running (or the last one).
    stamp: u64,
    /// HD images made so far by MMPX, and from a pack's images.
    made: u64,
    packed: u64,
    hash: Hasher,
}

/// FNV-1a over the words and the shape: a hash for the lookup (equal content is compared too).
fn content_hash(input: &[u32], width: usize, height: usize, scale: Scale) -> u64 {
    let mut x = 0xcbf2_9ce4_8422_2325u64 ^ ((width as u64) << 40 | (height as u64) << 8);
    x ^= scale.factor() as u64;
    for &c in input {
        x = (x ^ u64::from(c)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    x
}

impl HdImages {
    /// An empty cache that keeps at most `cap` bytes.
    pub fn new(cap: usize) -> Self {
        Self {
            slab: Vec::new(),
            free: Vec::new(),
            by_hash: HashMap::new(),
            bytes: 0,
            peak: 0,
            cap,
            next_token: 1,
            stamp: 0,
            made: 0,
            packed: 0,
            hash: content_hash,
        }
    }

    /// As [`HdImages::new`] with another lookup hash (tests force collisions with it).
    #[cfg(test)]
    fn with_hash(cap: usize, hash: Hasher) -> Self {
        Self {
            hash,
            ..Self::new(cap)
        }
    }

    /// Bytes held now: each image's input and output words and the indices it was last reached
    /// from.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The retained cache payload peak: the most [`HdImages::bytes`] held so far, within a
    /// composition included (it keeps the images it shows even past the bound, and is back under
    /// it when it ends). Scratch outside the entries is not in it: the 2× intermediate of a 4×
    /// magnification while it is made, the compositor's frame and its last inputs.
    pub fn peak(&self) -> usize {
        self.peak
    }

    /// The bytes of the entries held, summed one by one ([`HdImages::bytes`] keeps the total).
    #[cfg(test)]
    fn entries_bytes(&self) -> usize {
        self.slab.iter().flatten().map(Entry::bytes).sum()
    }

    /// The bound.
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Images held now.
    pub fn len(&self) -> usize {
        self.slab.len() - self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// HD images made so far by MMPX (each a run of it).
    pub fn made(&self) -> u64 {
        self.made
    }

    /// HD images made so far from a pack's images ([`pack_words`]).
    pub fn packed(&self) -> u64 {
        self.packed
    }

    fn entry(&self, slot: usize) -> &Entry {
        self.slab[slot].as_ref().expect("a live entry")
    }

    /// Whether the entry in `slot` is still the one with `token`, at `scale`, last reached from
    /// `img`'s pixels under palette generation `palette`; marked used now if so.
    fn reached(
        &mut self,
        (slot, token): (usize, u32),
        img: &Image<'_>,
        palette: u64,
        scale: Scale,
    ) -> bool {
        let stamp = self.stamp;
        match self.slab.get_mut(slot).and_then(Option::as_mut) {
            Some(e)
                if e.token == token
                    && e.scale == scale
                    && (e.width, e.height) == (img.width, img.height)
                    && e.source.palette == palette
                    && e.source.pixels == img.pixels
                    && e.source.transparent.as_deref() == img.transparent =>
            {
                e.used = stamp;
                true
            }
            _ => false,
        }
    }

    /// The slot of the HD image of `input` (`width` × `height` words) at `scale`, made now if it
    /// is not held: from the pack image `art` when given (spec 07 H2: the pack's image in front
    /// of MMPX), else by MMPX. An entry made from a pack image is keyed by that image as well as
    /// by `input`; a pack set or removed drops them all ([`HdImages::drop_packed`]), so none of
    /// another pack is ever shown.
    ///
    /// `source` is what the image was reached from now (its indices under the palette now): the
    /// entry keeps it for [`HdImages::reached`].
    fn get(
        &mut self,
        input: &[u32],
        (width, height): (usize, usize),
        scale: Scale,
        source: Source,
        art: Option<PackHd<'_>>,
    ) -> usize {
        let origin = art.map_or(Origin::Mmpx, |a| Origin::Pack(a.index));
        let hash = (self.hash)(input, width, height, scale);
        if let Some(slots) = self.by_hash.get(&hash) {
            for &slot in slots {
                let e = self.slab[slot].as_mut().expect("an indexed entry");
                if e.scale == scale
                    && e.width == width
                    && e.height == height
                    && e.origin == origin
                    && e.input == input
                {
                    e.used = self.stamp;
                    self.bytes = self.bytes - e.source.bytes() + source.bytes();
                    e.source = source;
                    self.peak = self.peak.max(self.bytes);
                    return slot;
                }
            }
        }
        let mut hd = Vec::new();
        match art {
            Some(a) => {
                pack_words(a.rgba, a.width, a.height, scale, &mut hd);
                self.packed += 1;
            }
            None => {
                mmpx(input, width, height, scale, &mut hd);
                self.made += 1;
            }
        }
        let token = self.next_token;
        self.next_token = self.next_token.wrapping_add(1).max(1);
        let entry = Entry {
            token,
            hash,
            origin,
            scale,
            width,
            height,
            input: input.to_vec(),
            hd,
            source,
            used: self.stamp,
        };
        while self.bytes + entry.bytes() > self.cap && self.evict(self.stamp) {}
        self.bytes += entry.bytes();
        self.peak = self.peak.max(self.bytes);
        let slot = match self.free.pop() {
            Some(s) => {
                self.slab[s] = Some(entry);
                s
            }
            None => {
                self.slab.push(Some(entry));
                self.slab.len() - 1
            }
        };
        self.by_hash.entry(hash).or_default().push(slot);
        slot
    }

    /// Drops the least recently used entry used before composition `before`; false when there
    /// is none.
    fn evict(&mut self, before: u64) -> bool {
        let oldest = self
            .slab
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.as_ref().map(|e| (e.used, i)))
            .filter(|&(used, _)| used < before)
            .min();
        let Some((_, slot)) = oldest else {
            return false;
        };
        self.remove(slot);
        true
    }

    fn remove(&mut self, slot: usize) {
        let e = self.slab[slot].take().expect("a live entry");
        self.bytes -= e.bytes();
        if let Some(slots) = self.by_hash.get_mut(&e.hash) {
            slots.retain(|&s| s != slot);
            if slots.is_empty() {
                self.by_hash.remove(&e.hash);
            }
        }
        self.free.push(slot);
    }

    /// Drops every entry made from a pack image (the pack was set or removed: no later
    /// composition can reach one).
    fn drop_packed(&mut self) {
        for slot in 0..self.slab.len() {
            if self.slab[slot]
                .as_ref()
                .is_some_and(|e| e.origin != Origin::Mmpx)
            {
                self.remove(slot);
            }
        }
    }

    /// After a composition: back under the bound.
    fn shrink(&mut self) {
        while self.bytes > self.cap && self.evict(u64::MAX) {}
    }
}

/// The side of a character cell in screen pixels: the screen is [`COLS`] × [`ROWS`] cells.
pub const CELL: usize = 8;
/// Cells per row of the screen.
pub const COLS: usize = WIDTH / CELL;
/// Rows of cells of the screen.
pub const ROWS: usize = HEIGHT / CELL;
const _: () = assert!(
    WIDTH.is_multiple_of(CELL) && HEIGHT.is_multiple_of(CELL),
    "the screen is whole cells"
);
/// Cells of the screen.
const CELLS: usize = COLS * ROWS;

/// The Hangul syllables U+AC00..=U+D7A3 (`u6_formats::font::HANGUL_FIRST`..=`HANGUL_LAST`).
const HANGUL: std::ops::RangeInclusive<u16> = 0xac00..=0xd7a3;

/// Spec 11: whether a glyph cell of code `code` is drawn in the HD font ([`Compositor::glyphs`]):
/// printable ASCII but the space (0x21..=0x7e) and the Hangul syllables. Not the space, the
/// control codes the game draws as symbols (its key-wait cursor), and the codes past 0x7e of its
/// font (the runes). A whole space is listed apart from these and not drawn ([`HdGlyph::code`]).
pub fn hd_font_code(code: u16) -> bool {
    (0x21..=0x7e).contains(&code) || HANGUL.contains(&code)
}

/// The space's code: listed when whole, not drawn in the HD font ([`glyph_cell`]).
const SPACE: u16 = 0x20;

/// Spec 11: one cell the HD font mode lists ([`Compositor::glyphs`]): a cell it shows in the HD
/// font, or a whole space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HdGlyph {
    /// The cell's top-left screen pixel (multiples of [`CELL`]).
    pub x: u16,
    pub y: u16,
    /// Its code: a code drawn in the HD font ([`hd_font_code`]), or 0x20, a space the text
    /// writer drew whole (no pixel of it ink, all of one colour), for which nothing is drawn: a
    /// run of text may cross it.
    pub code: u16,
    /// The colour of its set pixels and of its clear ones; a space's ink is its paper.
    pub ink: Rgb,
    pub paper: Rgb,
}

/// A cell listed: its code and its ink and paper indices (a space's ink is its paper).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellGlyph {
    code: u16,
    ink: u8,
    paper: u8,
}

/// Whether cell (col, row) is listed, and as what. Each of its 64 pixels is a [`Kind::Glyph`]
/// source of one code at version 0, (u, v) its place in the cell, flags [`FLAG_INK`] or none,
/// its set pixels of one colour and its clear ones of one colour; and either the code is drawn
/// in the HD font ([`hd_font_code`]) with at least one set pixel and one clear one, or it is the
/// space ([`SPACE`], not drawn in the HD font, listed on its own) with no set pixel, listed with
/// its one colour as its ink and its paper.
fn glyph_cell(plane: &[Src], pixels: &[u8], col: usize, row: usize) -> Option<CellGlyph> {
    let (x0, y0) = (col * CELL, row * CELL);
    let code = plane[y0 * WIDTH + x0].id;
    if plane[y0 * WIDTH + x0].kind != Kind::Glyph || !(hd_font_code(code) || code == SPACE) {
        return None;
    }
    let (mut ink, mut paper) = (None, None);
    for v in 0..CELL {
        for u in 0..CELL {
            let i = (y0 + v) * WIDTH + x0 + u;
            let s = plane[i];
            let at = Src {
                ver: 0,
                id: code,
                u: u as u16,
                v: v as u16,
                kind: Kind::Glyph,
                flags: s.flags & (FLAG_INK | FLAG_PAPER),
            };
            if s != at {
                return None;
            }
            let colour = if s.flags & FLAG_INK != 0 {
                &mut ink
            } else {
                &mut paper
            };
            if *colour.get_or_insert(pixels[i]) != pixels[i] {
                return None;
            }
        }
    }
    if code == SPACE {
        return match (ink, paper) {
            (None, Some(paper)) => Some(CellGlyph {
                code,
                ink: paper,
                paper,
            }),
            _ => None,
        };
    }
    Some(CellGlyph {
        code,
        ink: ink?,
        paper: paper?,
    })
}

/// No HD image: the pixel is shown classic.
const NONE: u32 = 0;

/// What one image is in the running composition.
#[derive(Clone, Copy)]
struct Resolved {
    /// The HD image's token, or [`NONE`].
    token: u32,
    ver: u32,
    width: usize,
    height: usize,
    /// Its slot in [`HdImages`].
    slot: usize,
}

impl Resolved {
    const CLASSIC: Resolved = Resolved {
        token: NONE,
        ver: 0,
        width: 0,
        height: 0,
        slot: 0,
    };
}

/// One image (kind, id): what it is in the composition `stamp`, and the slot and token of the HD
/// image it had when last resolved, under the pack of the generation beside them (an image shown
/// again unchanged under the same pack needs no lookup: [`HdImages::reached`]). The pack's
/// generation is the image's own, not its entry's: an MMPX entry is shared by every image of its
/// content, and whether the pack has an image for this one is a matter of its (kind, id).
#[derive(Default)]
struct Slot {
    stamp: u64,
    resolved: Option<Resolved>,
    memo: Option<(usize, u32, u64)>,
}

/// The inputs of the last composition, per screen pixel, and per cell the glyph it showed in the
/// HD font.
struct Last {
    palette: [Rgb; 256],
    scale: Scale,
    src: Vec<Src>,
    pixels: Vec<u8>,
    tokens: Vec<u32>,
    cells: Vec<Option<CellGlyph>>,
}

/// The HD compositor (the module docs).
pub struct Compositor {
    hd: HdImages,
    scale: Scale,
    /// `WIDTH·S × HEIGHT·S` packed RGBA words.
    out: Vec<u32>,
    last: Option<Last>,
    /// Per tile record id, up to the largest named.
    tiles: Vec<Slot>,
    /// Portraits and bitmaps.
    others: HashMap<(Kind, u16), Slot>,
    /// Counts the palettes the compositions saw (a memo of another palette is not used).
    palette: u64,
    /// The pack set ([`Compositor::set_pack`]) and the count of packs set or removed so far (a
    /// memo of another pack is not used).
    pack: Option<Rc<dyn Pack>>,
    pack_gen: u64,
    /// Scratch: an image's pixels as words.
    input: Vec<u32>,
    /// The screen rect written since [`Compositor::take_dirty`]: x0, y0, x1, y1 (exclusive).
    dirty: Option<[usize; 4]>,
    /// Spec 11: the HD font mode ([`Compositor::set_font_mode`]).
    font: bool,
    /// The mode changed since the last composition: the next writes every block.
    font_changed: bool,
    /// Scratch: per cell, the glyph the running composition shows in the HD font.
    cells: Vec<Option<CellGlyph>>,
    /// The cells the last composition showed in the HD font ([`Compositor::glyphs`]).
    glyphs: Vec<HdGlyph>,
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

impl Compositor {
    /// A compositor whose HD images take at most [`HD_CACHE_BYTES`].
    pub fn new() -> Self {
        Self::with_cap(HD_CACHE_BYTES)
    }

    /// A compositor whose HD images take at most `cap` bytes.
    pub fn with_cap(cap: usize) -> Self {
        Self::with_cache(HdImages::new(cap))
    }

    fn with_cache(hd: HdImages) -> Self {
        Self {
            hd,
            scale: Scale::X2,
            out: Vec::new(),
            last: None,
            tiles: Vec::new(),
            others: HashMap::new(),
            palette: 0,
            pack: None,
            pack_gen: 0,
            input: Vec::new(),
            dirty: None,
            font: false,
            font_changed: false,
            cells: Vec::new(),
            glyphs: Vec::new(),
        }
    }

    /// The HD images held.
    pub fn cache(&self) -> &HdImages {
        &self.hd
    }

    /// The scale of the last composition.
    pub fn scale(&self) -> Scale {
        self.scale
    }

    /// The last composition: `WIDTH·S × HEIGHT·S` packed RGBA words, row-major (empty before
    /// the first).
    pub fn rgba(&self) -> &[u32] {
        &self.out
    }

    /// The screen rect (x, y, width, height, in screen pixels) the compositions since the last
    /// call wrote; `None` when they wrote nothing.
    pub fn take_dirty(&mut self) -> Option<[usize; 4]> {
        self.dirty
            .take()
            .map(|[x0, y0, x1, y1]| [x0, y0, x1 - x0, y1 - y0])
    }

    /// The next composition writes every block and looks every image up again: with no last
    /// composition it takes a new palette generation, which no image's memo was made under
    /// ([`HdImages::reached`]), so each is found by its content and, with a pack set, its key is
    /// asked again ([`Images::key`]). A platform calls it when the images may change in a way
    /// their pixels do not show (a new game: another file can give a portrait other bytes that
    /// look the same through the colour table).
    pub fn forget(&mut self) {
        self.last = None;
    }

    /// Spec 07 H2: sets the pack whose images go in front of MMPX (`None`: no pack). The next
    /// composition looks every image up again, writes again the blocks whose HD image changed,
    /// and is what a new compositor with this pack would write; the HD images made from the
    /// pack set before are dropped.
    pub fn set_pack(&mut self, pack: Option<Rc<dyn Pack>>) {
        self.pack = pack;
        self.pack_gen += 1;
        self.hd.drop_packed();
    }

    /// Whether a pack is set.
    pub fn has_pack(&self) -> bool {
        self.pack.is_some()
    }

    /// Spec 11: the HD font mode on (`hd`) or off (the pixel font). On, each cell of the glyph of
    /// one code drawn whole whose code is drawn in the HD font ([`hd_font_code`]), and each whole
    /// space, is filled with its paper colour (the paper's HD block where that is the paper's
    /// colour) and listed ([`Compositor::glyphs`]); off, glyph pixels are composed as the module
    /// docs say for every glyph pixel (classic, or the paper's HD block behind a clear pixel
    /// drawn over the paper in its colour), as every composition before the mode did. A change
    /// empties the list at once and makes the next composition write every block, as a change of
    /// scale does, also when the mode is changed back before it; setting the mode it is in
    /// changes nothing.
    pub fn set_font_mode(&mut self, hd: bool) {
        if self.font != hd {
            self.font = hd;
            self.font_changed = true;
            self.glyphs.clear();
        }
    }

    /// Whether the HD font mode is on ([`Compositor::set_font_mode`]).
    pub fn font_mode(&self) -> bool {
        self.font
    }

    /// The cells the last composition listed, in raster order: those it showed in the HD font and
    /// the whole spaces, for which nothing is drawn but which a run of text may cross
    /// ([`HdGlyph::code`]). All of them, the cells it left as they were included (a platform that
    /// copies a rect of the frame draws every listed glyph over it again). Empty with the mode
    /// off, and after a change of the mode until the next composition. A cell enters, leaves or
    /// changes in this list only with every block of it written (a space's too, whose blocks
    /// keep their colours), or leaves it at a change of the mode, whose next composition writes
    /// every block: so the rect the compositions wrote ([`Compositor::take_dirty`]) holds the
    /// whole cell.
    pub fn glyphs(&self) -> &[HdGlyph] {
        &self.glyphs
    }

    /// Composes `frame` at `scale` from the images `images` names ([`Compositor::rgba`]).
    /// Returns whether it wrote any block.
    pub fn compose(&mut self, frame: &Framebuffer, images: &mut dyn Images, scale: Scale) -> bool {
        let s = scale.factor();
        let ow = WIDTH * s;
        let n = WIDTH * HEIGHT;
        let mut full = self.out.len() != ow * HEIGHT * s;
        if full {
            self.out = vec![0; ow * HEIGHT * s];
        }
        let same_palette = self
            .last
            .as_ref()
            .is_some_and(|l| l.palette == frame.palette);
        if !same_palette {
            self.palette += 1;
        }
        let font = self.font;
        full |= std::mem::take(&mut self.font_changed);
        full |= !same_palette || self.last.as_ref().is_none_or(|l| l.scale != scale);
        let last = self.last.get_or_insert_with(|| Last {
            palette: frame.palette,
            scale,
            src: vec![Src::CLASSIC; n],
            pixels: vec![0; n],
            tokens: vec![NONE; n],
            cells: vec![None; CELLS],
        });
        last.palette = frame.palette;
        last.scale = scale;
        self.scale = scale;
        let plane = frame.src();
        let pixels = frame.pixels();
        // Spec 11: the cells listed now (shown in the HD font, or whole spaces).
        self.cells.clear();
        match plane.filter(|_| font) {
            Some(p) => self
                .cells
                .extend((0..CELLS).map(|c| glyph_cell(p, pixels, c % COLS, c / COLS))),
            None => self.cells.resize(CELLS, None),
        }
        let cells = &self.cells;
        self.hd.stamp += 1;
        let pal: [u32; 256] = std::array::from_fn(|i| word(frame.palette[i]));
        let mut res = Resolver {
            hd: &mut self.hd,
            tiles: &mut self.tiles,
            others: &mut self.others,
            input: &mut self.input,
            images,
            pack: self.pack.as_deref(),
            pal: &pal,
            palette: self.palette,
            pack_gen: self.pack_gen,
            scale,
        };
        let out = &mut self.out;
        let mut dirty = [usize::MAX, usize::MAX, 0, 0];
        let mut key: Option<(Kind, u16)> = None;
        let mut cur = Resolved::CLASSIC;
        // The paper, resolved at the first glyph pixel that may show it.
        let mut paper: Option<Resolved> = None;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let i = y * WIDTH + x;
                let src = plane.map_or(Src::CLASSIC, |p| p[i]);
                let index = pixels[i];
                let c = y / CELL * COLS + x / CELL;
                let glyph = cells[c];
                let classic = pal[usize::from(index)];
                // The colour of a pixel shown flat: a listed cell's paper colour, else its own.
                let flat = glyph.map_or(classic, |g| pal[usize::from(g.paper)]);
                let mut shown = None;
                // The place in the image shown whose HD block the pixel takes.
                let mut at = (usize::from(src.u), usize::from(src.v));
                if !matches!(src.kind, Kind::Classic | Kind::Glyph) && src.flags == 0 {
                    if key != Some((src.kind, src.id)) {
                        key = Some((src.kind, src.id));
                        cur = res.resolve(src.kind, src.id);
                    }
                    let (u, v) = at;
                    if cur.token != NONE && src.ver == cur.ver && u < cur.width && v < cur.height {
                        shown = Some(cur);
                    }
                } else if src.kind == Kind::Glyph
                    && src.flags & FLAG_PAPER != 0
                    && (glyph.is_some() || src.flags & FLAG_INK == 0)
                {
                    // A glyph names no image: a pixel drawn over the paper ([`FLAG_PAPER`]) of a
                    // listed cell (filled with its paper colour) or a clear one shows the paper's
                    // HD block of its own place where that colour is the paper's there (the paper
                    // is drawn at (0, 0)), else it is flat.
                    let p = *paper.get_or_insert_with(|| res.resolve(Kind::Bitmap, BITMAP_PAPER));
                    let under = (p.token != NONE && x < p.width && y < p.height)
                        .then(|| res.hd.entry(p.slot).input[y * p.width + x]);
                    if under == Some(flat) {
                        shown = Some(p);
                        at = (x, y);
                    }
                }
                let token = shown.map_or(NONE, |r| r.token);
                if !full
                    && last.cells[c] == glyph
                    && last.src[i] == src
                    && last.pixels[i] == index
                    && last.tokens[i] == token
                {
                    continue;
                }
                last.src[i] = src;
                last.pixels[i] = index;
                last.tokens[i] = token;
                dirty = [
                    dirty[0].min(x),
                    dirty[1].min(y),
                    dirty[2].max(x + 1),
                    dirty[3].max(y + 1),
                ];
                let o = y * s * ow + x * s;
                match shown {
                    None => {
                        for r in 0..s {
                            out[o + r * ow..o + r * ow + s].fill(flat);
                        }
                    }
                    Some(r) => {
                        let hd = &res.hd.entry(r.slot).hd;
                        let hw = r.width * s;
                        let b = at.1 * s * hw + at.0 * s;
                        for k in 0..s {
                            let from = &hd[b + k * hw..b + k * hw + s];
                            let to = &mut out[o + k * ow..o + k * ow + s];
                            for (d, &c) in to.iter_mut().zip(from) {
                                *d = over(c, flat);
                            }
                        }
                    }
                }
            }
        }
        last.cells.copy_from_slice(cells);
        self.glyphs.clear();
        self.glyphs
            .extend(cells.iter().enumerate().filter_map(|(c, g)| {
                g.map(|g| HdGlyph {
                    x: (c % COLS * CELL) as u16,
                    y: (c / COLS * CELL) as u16,
                    code: g.code,
                    ink: frame.palette[usize::from(g.ink)],
                    paper: frame.palette[usize::from(g.paper)],
                })
            }));
        self.hd.shrink();
        if dirty[2] == 0 {
            return false;
        }
        let d = self.dirty.get_or_insert(dirty);
        *d = [
            d[0].min(dirty[0]),
            d[1].min(dirty[1]),
            d[2].max(dirty[2]),
            d[3].max(dirty[3]),
        ];
        true
    }
}

/// Resolves the images of one composition ([`Compositor::compose`]).
struct Resolver<'a> {
    hd: &'a mut HdImages,
    tiles: &'a mut Vec<Slot>,
    others: &'a mut HashMap<(Kind, u16), Slot>,
    input: &'a mut Vec<u32>,
    images: &'a mut dyn Images,
    /// The pack set, of generation `pack_gen`.
    pack: Option<&'a dyn Pack>,
    pal: &'a [u32; 256],
    palette: u64,
    pack_gen: u64,
    scale: Scale,
}

impl Resolver<'_> {
    /// What image (kind, id) is in this composition: asked of the images once, its HD image
    /// found by its content (through the memo when the content is the memo's), the pack's image
    /// for that content in front of MMPX.
    fn resolve(&mut self, kind: Kind, id: u16) -> Resolved {
        let stamp = self.hd.stamp;
        let slot = if kind == Kind::Tile {
            let i = usize::from(id);
            if self.tiles.len() <= i {
                self.tiles.resize_with(i + 1, Slot::default);
            }
            &mut self.tiles[i]
        } else {
            self.others.entry((kind, id)).or_default()
        };
        if slot.stamp == stamp
            && let Some(r) = slot.resolved
        {
            return r;
        }
        slot.stamp = stamp;
        let resolved = match self.images.image(kind, id) {
            Some(img) => {
                let (w, h, ver) = (img.width, img.height, img.ver);
                let shaped = (1..=IMAGE_MAX.0).contains(&w)
                    && (1..=IMAGE_MAX.1).contains(&h)
                    && img.pixels.len() == w * h
                    && img.transparent.is_none_or(|t| t.len() == w * h);
                if !shaped {
                    Resolved::CLASSIC
                } else {
                    let memo = slot.memo.filter(|&(at, token, pack)| {
                        pack == self.pack_gen
                            && self.hd.reached((at, token), &img, self.palette, self.scale)
                    });
                    let (at, token) = match memo {
                        Some((at, token, _)) => (at, token),
                        None => {
                            self.input.clear();
                            self.input
                                .extend(img.pixels.iter().enumerate().map(|(i, &p)| {
                                    match img.transparent {
                                        Some(t) if t[i] => CLEAR,
                                        _ => self.pal[usize::from(p)],
                                    }
                                }));
                            let source = Source::of(&img, self.palette);
                            let art = self.pack.and_then(|pack| {
                                let key = self.images.key(kind, id)?;
                                pack.hd(kind, id, key).filter(|a| {
                                    a.width == w * PACK_SCALE
                                        && a.height == h * PACK_SCALE
                                        && a.rgba.len() == a.width * a.height * 4
                                })
                            });
                            let at = self.hd.get(self.input, (w, h), self.scale, source, art);
                            (at, self.hd.entry(at).token)
                        }
                    };
                    slot.memo = Some((at, token, self.pack_gen));
                    Resolved {
                        token,
                        ver,
                        width: w,
                        height: h,
                        slot: at,
                    }
                }
            }
            None => Resolved::CLASSIC,
        };
        slot.resolved = Some(resolved);
        resolved
    }
}

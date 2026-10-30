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
//! - for every other pixel (classic, glyph, flagged, stale), its classic colour;
//! - spec 18: a pixel of the remembered sight ([`FLAG_MEMORY`], no other flag) as the same pixel
//!   without the flag, from the grey copy of the image's HD version ([`crate::grey`] of each
//!   sub-pixel, alpha kept; made once per HD image and kept with it); its classic colour is
//!   already grey (its index is past the game's colours, [`crate::GREY`]).
//!
//! Text always keeps its original 8 × 8 pixel shapes, including Hangul. Whole glyph cells
//! are also listed by [`Compositor::glyphs`] for diagnostics and accessibility; this metadata
//! never erases their ink or changes rendering. Spaces, typed spaces and paper symbols keep
//! their distinct codes, with symbol rows exposed by [`Compositor::symbol_rows`].
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
//! words), the last composition's inputs (a [`Src`], an index and a token per screen pixel),
//! one image dependency per 16-pixel row chunk, lazily requested diagnostic glyphs, and a few
//! words per image named so far: per tile
//! record up to the largest record id named (the game's records are 2048), per portrait (letter,
//! item) and per panel bitmap (two) named. The pack's own images are the platform's ([`Pack`]);
//! the compositor holds a reference to it.
//!
//! The output is packed RGBA words, `u32::from_le_bytes([r, g, b, a])`: in wasm memory (little
//! endian) the bytes the page's `ImageData` takes.
//!
//! Spec 20 Design 5: [`Compositor::compose_layer`] composes a layer of a slide (a rect of pixels
//! of its own size, [`crate::motion::LayerPixels`]) the way a composition writes the frame's
//! pixels outside the text, its clear pixels transparent, with the images and the palette
//! generation of the frame composed last; it keeps no inputs (a slide's layers are made once a
//! publication).

mod mmpx;
#[cfg(test)]
mod tests;

use std::cell::OnceCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::{
    BITMAP_PAPER, FLAG_HALFTONE, FLAG_INK, FLAG_MEMORY, FLAG_PAPER, FLAG_PAPER_OFF, FLAG_SYMBOL,
    FLAG_TYPED, Framebuffer, HEIGHT, Kind, Rgb, Src, WIDTH, grey,
};

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
/// (spec 13 and 14, [`WIDTH`] × [`HEIGHT`] 912 × 408) holds no larger image.
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

    /// Spec 18: the (kind, id) a pack keys image (kind, id) under ([`Pack::hd`]): the image's own
    /// unless it is a copy of another's pixels (the remembered sight's copy of a record), then
    /// that one's, so a copy shows the pack's image of what it was taken from when its content
    /// key is the pack image's.
    fn pack_id(&self, kind: Kind, id: u16) -> (Kind, u16) {
        (kind, id)
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
    /// Spec 18: `hd` in grey ([`grey_word`]), made at the first pixel of the remembered sight
    /// that shows it ([`HdImages::grey`]).
    grey: Option<Vec<u32>>,
    /// What it was last reached from.
    source: Source,
    /// The composition that used it last ([`HdImages::stamp`]).
    used: u64,
}

impl Entry {
    fn bytes(&self) -> usize {
        let grey = self.grey.as_ref().map_or(0, Vec::len);
        (self.input.len() + self.hd.len() + grey) * 4 + self.source.bytes()
    }
}

/// Spec 18: the packed word `w` in the remembered sight's grey ([`crate::grey`]), its alpha kept.
fn grey_word(w: u32) -> u32 {
    let [r, g, b, a] = w.to_le_bytes();
    let c = grey(Rgb::new(r, g, b));
    u32::from_le_bytes([c.r, c.g, c.b, a])
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

    /// Spec 18: the grey copy of the HD image in `slot`, made now if it has none (its words
    /// counted in [`HdImages::bytes`], the bound kept at the composition's end as for any image).
    fn grey(&mut self, slot: usize) -> &[u32] {
        let e = self.slab[slot].as_mut().expect("a live entry");
        if e.grey.is_none() {
            e.grey = Some(e.hd.iter().map(|&w| grey_word(w)).collect());
            self.bytes += e.hd.len() * 4;
            self.peak = self.peak.max(self.bytes);
        }
        e.grey.as_deref().unwrap_or_default()
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
            grey: None,
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

/// Whether a whole character cell has a text code exposed by [`Compositor::glyphs`]:
/// printable ASCII and Hangul. Spaces and paper symbols are classified separately.
pub fn text_code(code: u16) -> bool {
    (0x21..=0x7e).contains(&code) || HANGUL.contains(&code)
}

/// The space's code: listed when whole ([`glyph_cell`]).
const SPACE: u16 = 0x20;

/// Spec 11 Design 2 (rev 3): [`HdGlyph::code`] of a listed symbol is this plus its glyph's code (a
/// code of the game's font, under 0x100): codes no listed character has (private use).
pub const SYMBOL: u16 = 0xe000;

/// Spec 11 Design 3 (rev 3): [`HdGlyph::code`] of a whole space the player typed
/// ([`crate::FLAG_TYPED`]): it never ends a run of text (a private-use code no listed character
/// has).
pub const TYPED_SPACE: u16 = 0xe120;

/// Spec 11 rev 3: the glyph of a listed symbol's [`HdGlyph::code`] ([`SYMBOL`]), `None` for any
/// other code.
pub fn symbol_glyph(code: u16) -> Option<u8> {
    (SYMBOL..SYMBOL + 0x100)
        .contains(&code)
        .then(|| (code - SYMBOL) as u8)
}

/// One whole source-plane glyph cell, exposed as diagnostics without changing its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HdGlyph {
    /// The cell's top-left screen pixel (multiples of [`CELL`]).
    pub x: u16,
    pub y: u16,
    /// A printable text code ([`text_code`]), a whole space (0x20), [`TYPED_SPACE`], or
    /// [`SYMBOL`] plus the original glyph number of a symbol drawn over the paper.
    pub code: u16,
    /// The colour of its set pixels and of its clear ones; a space's ink is its paper.
    pub ink: Rgb,
    pub paper: Rgb,
    /// Drawn half-toned in the original pixel frame ([`crate::FLAG_HALFTONE`]).
    pub halftone: bool,
}

/// A cell listed: its code and its ink and paper indices (a space's ink is its paper); a
/// symbol's rows (bit 7 = leftmost; none for a character or a space).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellGlyph {
    code: u16,
    ink: u8,
    paper: u8,
    rows: [u8; CELL],
    halftone: bool,
}

/// Whether cell (col, row) is listed, and as what. Each of its 64 pixels is a [`Kind::Glyph`]
/// source of one code at version 0, (u, v) its place in the cell, flags [`FLAG_INK`] or none
/// (with [`FLAG_PAPER`] where drawn over the paper, and [`FLAG_TYPED`] and [`FLAG_HALFTONE`]
/// each on all of them or on none), its set pixels of one colour and its clear ones of one
/// colour; and either the code is
/// printable text ([`text_code`]) with at least one set pixel and one clear one, or it
/// is the space ([`SPACE`], listed on its own; [`TYPED_SPACE`] typed)
/// with no set pixel, listed with its one colour as its ink and its paper. Or it is a symbol
/// drawn over the paper ([`symbol_cell`]).
fn glyph_cell(plane: &[Src], pixels: &[u8], col: usize, row: usize) -> Option<CellGlyph> {
    let (x0, y0) = (col * CELL, row * CELL);
    let first = plane[y0 * WIDTH + x0];
    if first.kind != Kind::Glyph || first.flags & FLAG_SYMBOL != 0 {
        return symbol_cell(plane, pixels, (x0, y0));
    }
    let code = first.id;
    if !(text_code(code) || code == SPACE) {
        return None;
    }
    let typed = first.flags & (FLAG_TYPED | FLAG_HALFTONE);
    let halftone = first.flags & FLAG_HALFTONE != 0;
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
                flags: s.flags & (FLAG_INK | FLAG_PAPER) | typed,
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
                code: if typed & FLAG_TYPED != 0 {
                    TYPED_SPACE
                } else {
                    SPACE
                },
                ink: paper,
                paper,
                rows: [0; CELL],
                halftone,
            }),
            _ => None,
        };
    }
    Some(CellGlyph {
        code,
        ink: ink?,
        paper: paper?,
        rows: [0; CELL],
        halftone,
    })
}

/// Spec 11 Design 2 (rev 3): whether the cell at screen pixel (x0, y0) is a symbol the game drew
/// over the paper (`draw_symbol` of the text layer: the key wait's cursor, the more prompt's
/// mark), and as what. Its ink pixels are [`Kind::Glyph`] sources of one code under 0x100 at
/// version 0, (u, v) their place in the cell, flags [`FLAG_INK`], [`FLAG_PAPER`] and
/// [`FLAG_SYMBOL`], all of one colour (its ink); every other pixel names the paper at its own
/// place (flagged [`FLAG_PAPER_OFF`] or not), of one other colour (its paper), or of the ink's
/// colour unflagged (an ink pixel the paper's own colour there, which the symbol named the
/// paper: an ink pixel too). At least one ink pixel and one of the paper. Listed as [`SYMBOL`]
/// plus its code, with its rows. A cell that does not start with a pixel naming the paper or a
/// symbol's is none (the cheap test that keeps this off every other cell).
fn symbol_cell(plane: &[Src], pixels: &[u8], (x0, y0): (usize, usize)) -> Option<CellGlyph> {
    let names_paper = |s: &Src, x: usize, y: usize| {
        s.kind == Kind::Bitmap
            && s.id == BITMAP_PAPER
            && s.ver == 0
            && (usize::from(s.u), usize::from(s.v)) == (x, y)
            && s.flags & !FLAG_PAPER_OFF == 0
    };
    let symbol = |s: &Src| s.kind == Kind::Glyph && s.flags & FLAG_SYMBOL != 0;
    let first = plane[y0 * WIDTH + x0];
    if !(names_paper(&first, x0, y0) || symbol(&first)) {
        return None;
    }
    let (mut code, mut ink) = (None, None);
    let mut rows = [0u8; CELL];
    for (v, row) in rows.iter_mut().enumerate() {
        for u in 0..CELL {
            let (x, y) = (x0 + u, y0 + v);
            let i = y * WIDTH + x;
            let s = plane[i];
            if symbol(&s) {
                let at = Src {
                    ver: 0,
                    id: *code.get_or_insert(s.id),
                    u: u as u16,
                    v: v as u16,
                    kind: Kind::Glyph,
                    flags: FLAG_INK | FLAG_PAPER | FLAG_SYMBOL,
                };
                if s != at || *ink.get_or_insert(pixels[i]) != pixels[i] {
                    return None;
                }
                *row |= 0x80 >> u;
            } else if !names_paper(&s, x, y) {
                return None;
            }
        }
    }
    let (glyph, ink) = (u8::try_from(code?).ok()?, ink?);
    let mut paper = None;
    for (v, row) in rows.iter_mut().enumerate() {
        let marked = *row;
        for u in (0..CELL).filter(|&u| marked & (0x80 >> u) == 0) {
            let i = (y0 + v) * WIDTH + x0 + u;
            if pixels[i] == ink {
                if plane[i].flags != 0 {
                    return None;
                }
                *row |= 0x80 >> u;
            } else if *paper.get_or_insert(pixels[i]) != pixels[i] {
                return None;
            }
        }
    }
    Some(CellGlyph {
        code: SYMBOL + u16::from(glyph),
        ink,
        paper: paper?,
        rows,
        halftone: false,
    })
}

/// No HD image: the pixel is shown classic.
const NONE: u32 = 0;

/// What one image is in the running composition.
#[derive(Clone, Copy, PartialEq, Eq)]
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

const CHUNK_PIXELS: usize = 16;
const _: () = assert!(WIDTH.is_multiple_of(CHUNK_PIXELS));

/// Images consulted by a row chunk, even when missing, stale or hidden by classic colour.
/// A single dependency can be checked without visiting the chunk's unchanged pixels.
#[derive(Clone, Copy)]
enum ChunkImages {
    Flat,
    One(Kind, u16, Resolved),
    Mixed,
}

impl ChunkImages {
    fn record(&mut self, kind: Kind, id: u16, resolved: &Resolved) {
        match self {
            Self::Flat => *self = Self::One(kind, id, *resolved),
            Self::One(k, i, previous) if (*k, *i) == (kind, id) && previous == resolved => {}
            _ => *self = Self::Mixed,
        }
    }

    fn unchanged(self, resolver: &mut Resolver<'_>) -> bool {
        match self {
            Self::Flat => true,
            // Resolve in this composition too: refresh the image and pin its cache entry.
            Self::One(kind, id, previous) => resolver.resolve(kind, id) == previous,
            Self::Mixed => false,
        }
    }
}

/// The inputs of the last composition, per screen pixel. They also supply lazy diagnostics.
struct Last {
    palette: [Rgb; 256],
    scale: Scale,
    src: Vec<Src>,
    pixels: Vec<u8>,
    tokens: Vec<u32>,
    chunks: Vec<ChunkImages>,
    /// The next composition must resolve images again, but diagnostics still describe this
    /// last published frame until that composition replaces it.
    invalidated: bool,
}

#[derive(Default)]
struct Diagnostics {
    glyphs: Vec<HdGlyph>,
    symbol_rows: Vec<[u8; CELL]>,
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
    /// Computed only when a diagnostic getter is used, never by rendering a frame or layer.
    diagnostics: OnceCell<Diagnostics>,
    /// Spec 20: a layer was composed in a palette the last composition had not: the next writes
    /// every block ([`Compositor::compose_layer`]).
    palette_changed: bool,
    /// The palette generation `palette` was taken for (by a composition or a layer's).
    generation_of: Option<[Rgb; 256]>,
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
            diagnostics: OnceCell::new(),
            palette_changed: false,
            generation_of: None,
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

    /// The next composition writes every block and looks every image up again: with no reusable
    /// last composition it takes a new palette generation, which no image's memo was made under
    /// ([`HdImages::reached`]), so each is found by its content and, with a pack set, its key is
    /// asked again ([`Images::key`]). A platform calls it when the images may change in a way
    /// their pixels do not show (a new game: another file can give a portrait other bytes that
    /// look the same through the colour table).
    pub fn forget(&mut self) {
        if let Some(last) = &mut self.last {
            last.invalidated = true;
        }
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

    /// Whole glyph cells in raster order, including spaces and symbols. This is diagnostic
    /// metadata, computed on first request after a composition. Their original ink and paper
    /// remain in the frame; observing or changing this list does not enlarge its dirty rect.
    pub fn glyphs(&self) -> &[HdGlyph] {
        &self.diagnostics().glyphs
    }

    /// Spec 11 Design 2 (rev 3): the rows of each symbol [`Compositor::glyphs`] lists ([`SYMBOL`]),
    /// in its order: its 8 rows of 8 pixels (bit 7 = leftmost), a set bit an ink pixel of the
    /// cell. The frame already contains the original symbol pixels.
    pub fn symbol_rows(&self) -> &[[u8; CELL]] {
        &self.diagnostics().symbol_rows
    }

    fn diagnostics(&self) -> &Diagnostics {
        self.diagnostics.get_or_init(|| {
            let mut diagnostics = Diagnostics::default();
            if let Some(last) = &self.last {
                for c in 0..CELLS {
                    if let Some(g) = glyph_cell(&last.src, &last.pixels, c % COLS, c / COLS) {
                        diagnostics.glyphs.push(HdGlyph {
                            x: (c % COLS * CELL) as u16,
                            y: (c / COLS * CELL) as u16,
                            code: g.code,
                            ink: last.palette[usize::from(g.ink)],
                            paper: last.palette[usize::from(g.paper)],
                            halftone: g.halftone,
                        });
                        if symbol_glyph(g.code).is_some() {
                            diagnostics.symbol_rows.push(g.rows);
                        }
                    }
                }
            }
            diagnostics
        })
    }

    /// Composes `frame` at `scale` from the images `images` names ([`Compositor::rgba`]).
    /// Returns whether it wrote any block.
    pub fn compose(&mut self, frame: &Framebuffer, images: &mut dyn Images, scale: Scale) -> bool {
        self.diagnostics.take();
        let s = scale.factor();
        let ow = WIDTH * s;
        let n = WIDTH * HEIGHT;
        let mut full = self.out.len() != ow * HEIGHT * s;
        if full {
            self.out = vec![0; ow * HEIGHT * s];
        }
        let same_palette = !std::mem::take(&mut self.palette_changed)
            && self
                .last
                .as_ref()
                .is_some_and(|l| !l.invalidated && l.palette == frame.palette);
        if !same_palette || self.generation_of != Some(frame.palette) {
            self.palette += 1;
            self.generation_of = Some(frame.palette);
        }
        full |= !same_palette || self.last.as_ref().is_none_or(|l| l.scale != scale);
        let last = self.last.get_or_insert_with(|| Last {
            palette: frame.palette,
            scale,
            src: vec![Src::CLASSIC; n],
            pixels: vec![0; n],
            tokens: vec![NONE; n],
            chunks: vec![ChunkImages::Flat; n / CHUNK_PIXELS],
            invalidated: false,
        });
        last.palette = frame.palette;
        last.scale = scale;
        last.invalidated = false;
        self.scale = scale;
        let plane = frame.src();
        let pixels = frame.pixels();
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
            for x0 in (0..WIDTH).step_by(CHUNK_PIXELS) {
                let start = y * WIDTH + x0;
                let end = start + CHUNK_PIXELS;
                let chunk = start / CHUNK_PIXELS;
                if !full
                    && last.pixels[start..end] == pixels[start..end]
                    && plane.map_or_else(
                        || last.src[start..end].iter().all(|&src| src == Src::CLASSIC),
                        |src| last.src[start..end] == src[start..end],
                    )
                    && last.chunks[chunk].unchanged(&mut res)
                {
                    continue;
                }
                let mut dependencies = ChunkImages::Flat;
                for x in x0..x0 + CHUNK_PIXELS {
                    let i = y * WIDTH + x;
                    let src = plane.map_or(Src::CLASSIC, |p| p[i]);
                    let index = pixels[i];
                    // Only pixels that cannot sample an image may skip image resolution.
                    if !full
                        && (src.kind == Kind::Classic
                            || src.kind == Kind::Glyph
                                && (src.flags & FLAG_PAPER == 0 || src.flags & FLAG_INK != 0))
                        && last.src[i] == src
                        && last.pixels[i] == index
                        && last.tokens[i] == NONE
                    {
                        continue;
                    }
                    let classic = pal[usize::from(index)];
                    // Text is never cleared for a platform font overlay.
                    let flat = classic;
                    let mut shown: Option<&Resolved> = None;
                    // The place in the image shown whose HD block the pixel takes.
                    let mut at = (usize::from(src.u), usize::from(src.v));
                    if !matches!(src.kind, Kind::Classic | Kind::Glyph)
                        && src.flags & !FLAG_MEMORY == 0
                    {
                        if key != Some((src.kind, src.id)) {
                            key = Some((src.kind, src.id));
                            cur = res.resolve(src.kind, src.id);
                        }
                        // Record the branch, not just a resolve call: cur spans chunk boundaries.
                        dependencies.record(src.kind, src.id, &cur);
                        let (u, v) = at;
                        if cur.token != NONE
                            && src.ver == cur.ver
                            && u < cur.width
                            && v < cur.height
                        {
                            shown = Some(&cur);
                        }
                    } else if src.kind == Kind::Glyph
                        && src.flags & FLAG_PAPER != 0
                        && src.flags & FLAG_INK == 0
                    {
                        // A clear glyph pixel drawn over the paper uses the HD background only
                        // where its colour matches the paper's there (drawn at (0, 0)). Ink stays
                        // flat in its original colour.
                        let p =
                            paper.get_or_insert_with(|| res.resolve(Kind::Bitmap, BITMAP_PAPER));
                        dependencies.record(Kind::Bitmap, BITMAP_PAPER, p);
                        let under = (p.token != NONE && x < p.width && y < p.height)
                            .then(|| res.hd.entry(p.slot).input[y * p.width + x]);
                        if under == Some(flat) {
                            shown = Some(p);
                            at = (x, y);
                        }
                    }
                    let token = shown.map_or(NONE, |r| r.token);
                    if !full
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
                            let hd = if src.flags & FLAG_MEMORY != 0 {
                                res.hd.grey(r.slot)
                            } else {
                                res.hd.entry(r.slot).hd.as_slice()
                            };
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
                last.chunks[chunk] = dependencies;
            }
        }
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

impl Compositor {
    /// Spec 20 Design 5: composes `layer` (`palette` its colours) at `scale` into `out`:
    /// `layer.width·S × layer.height·S` packed words, each pixel its S × S block as
    /// [`Compositor::compose`] writes a pixel that is no glyph's (the image's HD block over its
    /// classic colour, a remembered one's from the grey copy, else the classic colour), each
    /// pixel no draw wrote transparent. A palette other than the one the palette generation was
    /// taken for (by the last composition or layer) takes a new one, as a composition does, and
    /// one other than the last composition's has the next composition write every block. A pixel whose
    /// place in the frame (the layer's top left at frame pixel (`layer.x`, `layer.y`)) the last
    /// composition, at this scale and palette, wrote from the same colour and source takes
    /// that composition's block as it is (the same block: the images and the
    /// pack are those it was resolved under).
    pub fn compose_layer(
        &mut self,
        layer: &crate::motion::LayerPixels,
        palette: &[Rgb; 256],
        images: &mut dyn Images,
        scale: Scale,
        out: &mut Vec<u32>,
    ) {
        let reuse = self.last.as_ref().is_some_and(|l| {
            !l.invalidated && l.palette == *palette && l.scale == scale && !self.palette_changed
        });
        if self
            .last
            .as_ref()
            .is_none_or(|l| l.invalidated || l.palette != *palette)
        {
            self.palette_changed = true;
        }
        if self.generation_of != Some(*palette) {
            self.palette += 1;
            self.generation_of = Some(*palette);
        }
        let s = scale.factor();
        let (w, h) = (layer.width, layer.height);
        let ow = w * s;
        let fw = WIDTH * s;
        let last = self.last.as_ref().filter(|_| reuse);
        let frame = &self.out;
        out.clear();
        out.resize(ow * h * s, CLEAR);
        self.hd.stamp += 1;
        let pal: [u32; 256] = std::array::from_fn(|i| word(palette[i]));
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
        let mut key: Option<(Kind, u16)> = None;
        let mut cur = Resolved::CLASSIC;
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if !layer.drawn[i] {
                    continue;
                }
                let src = layer.src[i];
                let o = y * s * ow + x * s;
                let (fx, fy) = (layer.x + x as i32, layer.y + y as i32);
                if let Some(l) = last
                    && (0..WIDTH as i32).contains(&fx)
                    && (0..HEIGHT as i32).contains(&fy)
                {
                    let (fx, fy) = (fx as usize, fy as usize);
                    let f = fy * WIDTH + fx;
                    if l.src[f] == src && l.pixels[f] == layer.pixels[i] {
                        let b = fy * s * fw + fx * s;
                        for k in 0..s {
                            out[o + k * ow..o + k * ow + s]
                                .copy_from_slice(&frame[b + k * fw..b + k * fw + s]);
                        }
                        continue;
                    }
                }
                let flat = pal[usize::from(layer.pixels[i])];
                let mut shown = None;
                if !matches!(src.kind, Kind::Classic | Kind::Glyph) && src.flags & !FLAG_MEMORY == 0
                {
                    if key != Some((src.kind, src.id)) {
                        key = Some((src.kind, src.id));
                        cur = res.resolve(src.kind, src.id);
                    }
                    let (u, v) = (usize::from(src.u), usize::from(src.v));
                    if cur.token != NONE && src.ver == cur.ver && u < cur.width && v < cur.height {
                        shown = Some((cur, (u, v)));
                    }
                }
                match shown {
                    None => {
                        for r in 0..s {
                            out[o + r * ow..o + r * ow + s].fill(flat);
                        }
                    }
                    Some((r, (u, v))) => {
                        let hd = if src.flags & FLAG_MEMORY != 0 {
                            res.hd.grey(r.slot)
                        } else {
                            res.hd.entry(r.slot).hd.as_slice()
                        };
                        let hw = r.width * s;
                        let b = v * s * hw + u * s;
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
        self.hd.shrink();
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
                                let (kind, id) = self.images.pack_id(kind, id);
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

/// Spec 18: the remembered sight in the compositor, on a synthetic tile (no game data).
#[cfg(test)]
mod memory_tests {
    use super::*;
    use crate::{FLAG_OVERLAY, GREY};

    /// One 16 × 16 tile record, id 7: two colours split by a staircase diagonal (MMPX smooths
    /// it, so its HD block is not the pixels scaled), a third in the corner.
    struct Tile7(Vec<u8>);

    impl Images for Tile7 {
        fn image(&mut self, kind: Kind, id: u16) -> Option<Image<'_>> {
            (kind == Kind::Tile && id == 7).then(|| Image {
                ver: 0,
                width: 16,
                height: 16,
                pixels: &self.0,
                transparent: None,
            })
        }
    }

    fn tile7() -> Tile7 {
        Tile7(
            (0..256)
                .map(|i: usize| match (i % 16, i / 16) {
                    (u, v) if u < 3 && v < 3 => 5,
                    (u, v) if u > v => 1,
                    _ => 2,
                })
                .collect(),
        )
    }

    /// The tile drawn at (32, 48), each pixel named the record with `flags` and its colour plus
    /// `add`; a palette of distinct colours, entry [`GREY`] + i the grey of entry i.
    fn frame(t: &Tile7, flags: u8, add: u8) -> Framebuffer {
        frame_at(t, flags, add, (32, 48))
    }

    /// [`frame`] with the tile drawn at `at`.
    fn frame_at(t: &Tile7, flags: u8, add: u8, at: (usize, usize)) -> Framebuffer {
        let mut fb = Framebuffer::new();
        fb.enable_src();
        for (i, c) in fb.palette.iter_mut().enumerate().take(32) {
            *c = Rgb::new(i as u8 * 8, 255 - i as u8 * 5, (i as u8).wrapping_mul(37));
        }
        for i in 0..32 {
            fb.palette[usize::from(GREY) + i] = grey(fb.palette[i]);
        }
        for (i, &p) in t.0.iter().enumerate() {
            let (u, v) = (i % 16, i / 16);
            let src = Src {
                ver: 0,
                id: 7,
                u: u as u16,
                v: v as u16,
                kind: Kind::Tile,
                flags,
            };
            fb.set_src(at.0 + u, at.1 + v, p | add, src);
        }
        fb
    }

    /// The composed words of the tile's place.
    fn block(c: &Compositor, s: usize) -> Vec<u32> {
        block_at(c, s, (32, 48))
    }

    /// The composed words of the 16 × 16 pixels from `at`.
    fn block_at(c: &Compositor, s: usize, at: (usize, usize)) -> Vec<u32> {
        let ow = WIDTH * s;
        (at.1 * s..(at.1 + 16) * s)
            .flat_map(|y| (at.0 * s..(at.0 + 16) * s).map(move |x| y * ow + x))
            .map(|i| c.rgba()[i])
            .collect()
    }

    /// A layer of the 16 × 16 pixels of `fb` from `from`, at frame pixel `at`, every pixel drawn.
    fn layer_of(
        fb: &Framebuffer,
        from: (usize, usize),
        at: (i32, i32),
    ) -> crate::motion::LayerPixels {
        let mut layer = crate::motion::LayerPixels::clear(at.0, at.1, 16, 16);
        for v in 0..16 {
            for u in 0..16 {
                layer.pixels[v * 16 + u] = fb.get(from.0 + u, from.1 + v).unwrap();
                layer.src[v * 16 + u] = fb.src_at(from.0 + u, from.1 + v).unwrap();
                layer.drawn[v * 16 + u] = true;
            }
        }
        layer
    }

    /// A pixel of the remembered sight shows the grey of the HD block the same pixel shows
    /// without the flag, at both scales; the grey copy is made once and counted in the cache's
    /// bytes. With the flag gone the block is the HD block again; an overlay pixel of the
    /// remembered sight is its classic colour, as a live overlay pixel is.
    #[test]
    fn a_remembered_pixel_takes_the_grey_copy_of_its_hd_block() {
        for scale in [Scale::X2, Scale::X4] {
            let s = scale.factor();
            let mut t = tile7();
            let (mut live, mut grey_c) = (Compositor::new(), Compositor::new());
            live.compose(&frame(&t, 0, 0), &mut t, scale);
            grey_c.compose(&frame(&t, FLAG_MEMORY, GREY), &mut t, scale);
            let want: Vec<u32> = block(&live, s).into_iter().map(grey_word).collect();
            let fb = frame(&t, FLAG_MEMORY, GREY);
            let flat: Vec<u32> = (48 * s..64 * s)
                .flat_map(|y| (32 * s..48 * s).map(move |x| (x / s, y / s)))
                .map(|(x, y)| word(fb.palette[usize::from(fb.get(x, y).unwrap())]))
                .collect();
            assert!(
                want != flat,
                "×{s}: the grey HD block is not the grey pixels scaled"
            );
            assert!(block(&grey_c, s) == want, "×{s}: the grey of the HD block");
            let hd_words = 16 * s * 16 * s;
            assert_eq!(
                grey_c.cache().bytes(),
                live.cache().bytes() + hd_words * 4,
                "×{s}: the grey copy counted"
            );
            assert_eq!(grey_c.cache().bytes(), grey_c.cache().entries_bytes());
            let bytes = grey_c.cache().bytes();
            grey_c.compose(&frame(&t, 0, 0), &mut t, scale);
            assert!(block(&grey_c, s) == block(&live, s), "×{s}: live again");
            grey_c.compose(&frame(&t, FLAG_MEMORY, GREY), &mut t, scale);
            assert!(block(&grey_c, s) == want && grey_c.cache().bytes() == bytes);
            grey_c.compose(&frame(&t, FLAG_MEMORY | FLAG_OVERLAY, GREY), &mut t, scale);
            assert!(block(&grey_c, s) == flat, "×{s}: an overlay pixel classic");
        }
    }

    /// Spec 20 Design 5: a layer holding the frame's pixels of the tile's place (and a clear
    /// column beside it) composes as the frame does there, at both scales: the live tile, the
    /// remembered tile's grey copy, a flagged pixel classic; its clear pixels transparent; and the
    /// next composition of the frame writes every block again, under a new palette generation,
    /// when the layer's palette was another.
    #[test]
    fn a_layer_composes_its_pixels_as_the_frame_does_and_its_clear_ones_transparent() {
        for scale in [Scale::X2, Scale::X4] {
            let s = scale.factor();
            for (flags, add) in [(0, 0), (FLAG_MEMORY, GREY), (FLAG_OVERLAY, 0)] {
                let mut t = tile7();
                let fb = frame(&t, flags, add);
                let mut c = Compositor::new();
                c.compose(&fb, &mut t, scale);
                let mut layer = crate::motion::LayerPixels::clear(32, 48, 17, 16);
                for v in 0..16 {
                    for u in 0..16 {
                        let i = v * 17 + u;
                        layer.pixels[i] = fb.get(32 + u, 48 + v).unwrap();
                        layer.src[i] = fb.src_at(32 + u, 48 + v).unwrap();
                        layer.drawn[i] = true;
                    }
                }
                let mut out = Vec::new();
                c.compose_layer(&layer, &fb.palette, &mut t, scale, &mut out);
                assert_eq!(out.len(), 17 * s * 16 * s);
                let mine: Vec<u32> = (0..16 * s)
                    .flat_map(|y| (0..16 * s).map(move |x| (x, y)))
                    .map(|(x, y)| out[y * 17 * s + x])
                    .collect();
                assert!(
                    mine == block(&c, s),
                    "×{s} flags {flags}: the frame's block"
                );
                let clear = (0..16 * s).all(|y| (16 * s..17 * s).all(|x| out[y * 17 * s + x] == 0));
                assert!(clear, "×{s}: the clear column transparent");
            }
            let mut t = tile7();
            let fb = frame(&t, 0, 0);
            let mut c = Compositor::new();
            c.compose(&fb, &mut t, scale);
            let mut other = fb.palette;
            other[1] = Rgb::new(1, 2, 3);
            let mut layer = crate::motion::LayerPixels::clear(0, 0, 1, 1);
            layer.drawn[0] = true;
            layer.pixels[0] = 1;
            let mut out = Vec::new();
            c.compose_layer(&layer, &other, &mut t, scale, &mut out);
            assert!(
                out.iter().all(|&w| w == word(Rgb::new(1, 2, 3))),
                "×{s}: in its palette"
            );
            assert!(c.take_dirty().is_some());
            assert!(
                c.compose(&fb, &mut t, scale),
                "×{s}: the frame written again"
            );
            assert!(
                block(&c, s)
                    == block(
                        &{
                            let mut fresh = Compositor::new();
                            fresh.compose(&fb, &mut t, scale);
                            fresh
                        },
                        s
                    ),
                "×{s}: as a new compositor"
            );
        }
    }

    /// Spec 20 Design 5: a layer composed where no composition of the frame drew its pixels (a
    /// new compositor, or the layer elsewhere than the tile in the frame composed) is the block the
    /// frame composes with the tile at the layer's place, at both scales: the live tile, the
    /// remembered one's grey copy, a flagged pixel classic.
    #[test]
    fn a_layer_composed_without_the_frames_blocks_is_the_block_of_its_pixels() {
        for scale in [Scale::X2, Scale::X4] {
            let s = scale.factor();
            for (flags, add) in [(0, 0), (FLAG_MEMORY, GREY), (FLAG_OVERLAY, 0)] {
                let mut t = tile7();
                let there = (96, 16);
                let want = {
                    let mut c = Compositor::new();
                    c.compose(&frame_at(&t, flags, add, there), &mut t, scale);
                    block_at(&c, s, there)
                };
                let fb = frame(&t, flags, add);
                let layer = layer_of(&fb, (32, 48), (there.0 as i32, there.1 as i32));
                let mut out = Vec::new();
                Compositor::new().compose_layer(&layer, &fb.palette, &mut t, scale, &mut out);
                assert!(out == want, "×{s} flags {flags}: a new compositor");
                let mut c = Compositor::new();
                c.compose(&fb, &mut t, scale);
                c.compose_layer(&layer, &fb.palette, &mut t, scale, &mut out);
                assert!(out == want, "×{s} flags {flags}: the layer elsewhere");
            }
        }
    }

    /// Spec 20 Design 5: a layer in palette A after one in palette B after the frame's in A shows
    /// A's colours (the images of B's generation are not A's), at both scales; the next
    /// composition of the frame is that of a new compositor.
    #[test]
    fn a_layer_in_the_frames_palette_after_one_in_another_takes_its_own_colours() {
        for scale in [Scale::X2, Scale::X4] {
            let s = scale.factor();
            let mut t = tile7();
            let fb = frame(&t, 0, 0);
            let mut b = fb.palette;
            for k in [1, 2, 5] {
                b[k] = Rgb::new(200, 10 * k as u8, 3);
            }
            let layer = layer_of(&fb, (32, 48), (96, 16));
            let fresh = |palette: &[Rgb; 256], t: &mut Tile7| {
                let mut out = Vec::new();
                Compositor::new().compose_layer(&layer, palette, t, scale, &mut out);
                out
            };
            let (in_a, in_b) = (fresh(&fb.palette, &mut t), fresh(&b, &mut t));
            assert!(in_a != in_b, "×{s}: the palettes show apart");
            let mut c = Compositor::new();
            c.compose(&fb, &mut t, scale);
            let mut out = Vec::new();
            c.compose_layer(&layer, &b, &mut t, scale, &mut out);
            assert!(out == in_b, "×{s}: B");
            c.compose_layer(&layer, &fb.palette, &mut t, scale, &mut out);
            assert!(out == in_a, "×{s}: A again");
            c.compose(&fb, &mut t, scale);
            let mut new = Compositor::new();
            new.compose(&fb, &mut t, scale);
            assert!(
                c.rgba() == new.rgba(),
                "×{s}: the frame as a new compositor's"
            );
        }
    }

    /// The grey of `live`'s block of the tile, composed fresh from `fb` without the flag (its
    /// pixels named the record unflagged, their colours below [`GREY`]).
    fn grey_of_live(t: &mut Tile7, fb: &Framebuffer, scale: Scale) -> Vec<u32> {
        let mut plain = fb.clone();
        for (i, &p) in t.0.iter().enumerate() {
            let src = Src {
                ver: 0,
                id: 7,
                u: (i % 16) as u16,
                v: (i / 16) as u16,
                kind: Kind::Tile,
                flags: 0,
            };
            plain.set_src(32 + i % 16, 48 + i / 16, p, src);
        }
        let mut live = Compositor::new();
        live.compose(&plain, t, scale);
        block(&live, scale.factor())
            .into_iter()
            .map(grey_word)
            .collect()
    }

    /// Once a grey copy exists: new pixels of the image under the same palette, then a new
    /// palette over the same pixels, each make a new HD image whose grey copy the remembered
    /// pixel shows. The cache's bytes stay the sum of its entries'.
    #[test]
    fn the_grey_copy_follows_the_image_and_the_palette() {
        let scale = Scale::X2;
        let s = scale.factor();
        let mut t = tile7();
        let mut c = Compositor::new();
        let fb = frame(&t, FLAG_MEMORY, GREY);
        c.compose(&fb, &mut t, scale);
        let first = block(&c, s);
        t.0[17] = 3;
        let fb = frame(&t, FLAG_MEMORY, GREY);
        c.compose(&fb, &mut t, scale);
        let new_pixels = grey_of_live(&mut t, &fb, scale);
        assert!(
            block(&c, s) == new_pixels && new_pixels != first,
            "the new pixels"
        );
        assert_eq!(c.cache().bytes(), c.cache().entries_bytes());
        let mut fb = frame(&t, FLAG_MEMORY, GREY);
        fb.palette[1] = Rgb::new(250, 10, 10);
        fb.palette[usize::from(GREY) + 1] = grey(fb.palette[1]);
        c.compose(&fb, &mut t, scale);
        let new_palette = grey_of_live(&mut t, &fb, scale);
        assert!(
            block(&c, s) == new_palette && new_palette != new_pixels,
            "the new palette"
        );
        assert_eq!(c.cache().bytes(), c.cache().entries_bytes());
    }

    /// A bound that holds the HD image but not with its grey copy: a composition with remembered
    /// pixels shows the grey copy, then evicts the image with it at its end; one without keeps
    /// the image; the next with them makes the grey copy again. The bytes stay the entries' sum.
    #[test]
    fn a_bound_short_of_the_grey_copy_evicts_it_with_its_image() {
        let scale = Scale::X2;
        let s = scale.factor();
        let mut t = tile7();
        let live = frame(&t, 0, 0);
        let remembered = frame(&t, FLAG_MEMORY, GREY);
        let mut probe = Compositor::new();
        probe.compose(&live, &mut t, scale);
        let image = probe.cache().bytes();
        let mut c = Compositor::with_cap(image);
        for round in 0..2 {
            c.compose(&remembered, &mut t, scale);
            let want = grey_of_live(&mut t, &remembered, scale);
            assert!(block(&c, s) == want, "round {round}: the grey copy shown");
            assert!(
                c.cache().is_empty(),
                "round {round}: evicted with its image"
            );
            c.compose(&live, &mut t, scale);
            assert!(
                block(&c, s) == block(&probe, s),
                "round {round}: the image shown"
            );
            assert_eq!(c.cache().len(), 1, "round {round}: the image kept");
            assert_eq!(c.cache().bytes(), image);
            assert_eq!(c.cache().bytes(), c.cache().entries_bytes());
        }
    }

    /// One 16 × 16 tile image under id 2048 (past the game's records, as the remembered sight
    /// names a copy), version 0 whatever its pixels.
    struct Copy2048(Vec<u8>);

    impl Images for Copy2048 {
        fn image(&mut self, kind: Kind, id: u16) -> Option<Image<'_>> {
            (kind == Kind::Tile && id == 2048).then(|| Image {
                ver: 0,
                width: 16,
                height: 16,
                pixels: &self.0,
                transparent: None,
            })
        }
    }

    /// Spec 18 (the review's slot reuse): a copy's id freed and named again for other pixels,
    /// still at version 0: the compositor shows the new pixels (the memo of the id compares the
    /// pixels it was reached from: a new HD image), in grey, not the image it held under the id.
    #[test]
    fn an_id_named_again_for_other_pixels_at_version_0_shows_them() {
        let scale = Scale::X2;
        let s = scale.factor();
        let at = |t: &Copy2048| {
            let mut fb = frame(&Tile7(t.0.clone()), FLAG_MEMORY, GREY);
            for (i, &p) in t.0.iter().enumerate() {
                let src = Src {
                    ver: 0,
                    id: 2048,
                    u: (i % 16) as u16,
                    v: (i / 16) as u16,
                    kind: Kind::Tile,
                    flags: FLAG_MEMORY,
                };
                fb.set_src(32 + i % 16, 48 + i / 16, p | GREY, src);
            }
            fb
        };
        let grey_of = |t: &Copy2048| {
            let mut live = Compositor::new();
            let mut plain = frame(&Tile7(t.0.clone()), 0, 0);
            for (i, &p) in t.0.iter().enumerate() {
                let src = Src {
                    ver: 0,
                    id: 2048,
                    u: (i % 16) as u16,
                    v: (i / 16) as u16,
                    kind: Kind::Tile,
                    flags: 0,
                };
                plain.set_src(32 + i % 16, 48 + i / 16, p, src);
            }
            live.compose(&plain, &mut Copy2048(t.0.clone()), scale);
            block(&live, s)
                .into_iter()
                .map(grey_word)
                .collect::<Vec<u32>>()
        };
        let mut t = Copy2048(tile7().0);
        let mut c = Compositor::new();
        c.compose(&at(&t), &mut t, scale);
        let first = grey_of(&t);
        assert!(block(&c, s) == first, "the first pixels");
        let made = c.cache().made();
        for p in t.0.iter_mut().step_by(5) {
            *p = 4;
        }
        c.compose(&at(&t), &mut t, scale);
        let second = grey_of(&t);
        assert!(second != first, "other pixels");
        assert!(block(&c, s) == second, "the new pixels shown");
        assert_eq!(c.cache().made(), made + 1, "a new HD image");
    }

    #[test]
    fn grey_word_keeps_the_alpha_and_greys_the_colour() {
        let w = u32::from_le_bytes([200, 100, 50, 128]);
        let [r, g, b, a] = grey_word(w).to_le_bytes();
        let c = grey(Rgb::new(200, 100, 50));
        assert_eq!([r, g, b, a], [c.r, c.g, c.b, 128]);
        assert_eq!(grey_word(0), 0, "a transparent sub-pixel stays transparent");
    }
}

//! Spec 07 HD0: the HD compositor. From the frame shown (its colours, its palette and its source
//! plane) and the images the plane names, [`Compositor::compose`] writes the frame at S× (S = 2
//! or 4, [`Scale`]): each screen pixel becomes an S × S block,
//!
//! - for a pixel of a tile record, a portrait or a panel bitmap without flags, drawn at the
//!   image's current version, the block `(u·S.., v·S..)` of that image's HD version, where an HD
//!   sub-pixel that is transparent takes the pixel's classic colour;
//! - for every other pixel (classic, flagged, stale), its classic colour.
//!
//! An image's HD version is MMPX ×S ([`mmpx`]) of its current pixels in the frame's palette, a
//! transparent pixel transparent: a pure function of those pixels, the palette and S, cached by
//! their content ([`HdImages`], at most [`HD_CACHE_BYTES`]; spec 07 H2 puts the pack's image in
//! front of MMPX there). The compositor keeps the inputs of its last composition and writes again
//! only the blocks whose inputs changed; what it writes is what a new compositor would write
//! from the same frame.
//!
//! Memory: the HD images within the bound (each with the indices it was last reached from, which
//! [`HdImages::bytes`] counts; [`HdImages::peak`] is the retained cache payload peak, the most it
//! held, also during a composition whose images alone exceed the bound); outside it, the 2×
//! intermediate of a 4× magnification while it is made, the frame (`WIDTH·S × HEIGHT·S`
//! words), the last composition's inputs (a [`Src`], an index and a token per screen pixel) and
//! a few words per image named so far: per tile record up to the largest record id named (the
//! game's records are 2048), per portrait (letter, item) and per panel bitmap (two) named.
//!
//! The output is packed RGBA words, `u32::from_le_bytes([r, g, b, a])`: in wasm memory (little
//! endian) the bytes the page's `ImageData` takes.

mod mmpx;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use crate::{Framebuffer, HEIGHT, Kind, Rgb, Src, WIDTH};

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

/// An image as the game holds it now: palette indices, row-major, at most [`WIDTH`] ×
/// [`HEIGHT`] (the largest image the game draws, the paper bitmap; a larger one is shown
/// classic).
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

/// The palette indices and transparency an HD image was last reached from, under the palette of
/// composition generation `palette`: an image whose pixels are these again, under the same
/// palette, has this HD image with no lookup by content.
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

/// One HD image: the content it was made from and the result.
struct Entry {
    /// Never reused: an equal token is the same image ([`Compositor`] compares them).
    token: u32,
    hash: u64,
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
    /// HD images made so far.
    made: u64,
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

    /// HD images made so far (each a run of MMPX).
    pub fn made(&self) -> u64 {
        self.made
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
    /// is not held.
    ///
    /// The seam for spec 07 H2: the pack's image goes in front of MMPX here. [`Resolver::resolve`]
    /// has what its lookup needs (the image's kind, id, palette indices and transparency); it
    /// would pass the pack image found, and an entry made from a pack image is keyed by that image
    /// as well as by `input`, so a pack loaded or forgotten (which moves the platform's HD
    /// version) never shows an entry of the other.
    ///
    /// `source` is what the image was reached from now (its indices under the palette now): the
    /// entry keeps it for [`HdImages::reached`].
    fn get(
        &mut self,
        input: &[u32],
        (width, height): (usize, usize),
        scale: Scale,
        source: Source,
    ) -> usize {
        let hash = (self.hash)(input, width, height, scale);
        if let Some(slots) = self.by_hash.get(&hash) {
            for &slot in slots {
                let e = self.slab[slot].as_mut().expect("an indexed entry");
                if e.scale == scale && e.width == width && e.height == height && e.input == input {
                    e.used = self.stamp;
                    self.bytes = self.bytes - e.source.bytes() + source.bytes();
                    e.source = source;
                    self.peak = self.peak.max(self.bytes);
                    return slot;
                }
            }
        }
        let mut hd = Vec::new();
        mmpx(input, width, height, scale, &mut hd);
        self.made += 1;
        let token = self.next_token;
        self.next_token = self.next_token.wrapping_add(1).max(1);
        let entry = Entry {
            token,
            hash,
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
        let e = self.slab[slot].take().expect("a live entry");
        self.bytes -= e.bytes();
        if let Some(slots) = self.by_hash.get_mut(&e.hash) {
            slots.retain(|&s| s != slot);
            if slots.is_empty() {
                self.by_hash.remove(&e.hash);
            }
        }
        self.free.push(slot);
        true
    }

    /// After a composition: back under the bound.
    fn shrink(&mut self) {
        while self.bytes > self.cap && self.evict(u64::MAX) {}
    }
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
/// image it had when last resolved (an image shown again unchanged needs no lookup by content:
/// [`HdImages::reached`]).
#[derive(Default)]
struct Slot {
    stamp: u64,
    resolved: Option<Resolved>,
    memo: Option<(usize, u32)>,
}

/// The inputs of the last composition, per screen pixel.
struct Last {
    palette: [Rgb; 256],
    scale: Scale,
    src: Vec<Src>,
    pixels: Vec<u8>,
    tokens: Vec<u32>,
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
    /// Scratch: an image's pixels as words.
    input: Vec<u32>,
    /// The screen rect written since [`Compositor::take_dirty`]: x0, y0, x1, y1 (exclusive).
    dirty: Option<[usize; 4]>,
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
            input: Vec::new(),
            dirty: None,
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

    /// The next composition writes every block.
    pub fn forget(&mut self) {
        self.last = None;
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
        full |= !same_palette || self.last.as_ref().is_none_or(|l| l.scale != scale);
        let last = self.last.get_or_insert_with(|| Last {
            palette: frame.palette,
            scale,
            src: vec![Src::CLASSIC; n],
            pixels: vec![0; n],
            tokens: vec![NONE; n],
        });
        last.palette = frame.palette;
        last.scale = scale;
        self.scale = scale;
        self.hd.stamp += 1;
        let pal: [u32; 256] = std::array::from_fn(|i| word(frame.palette[i]));
        let plane = frame.src();
        let pixels = frame.pixels();
        let mut res = Resolver {
            hd: &mut self.hd,
            tiles: &mut self.tiles,
            others: &mut self.others,
            input: &mut self.input,
            images,
            pal: &pal,
            palette: self.palette,
            scale,
        };
        let out = &mut self.out;
        let mut dirty = [usize::MAX, usize::MAX, 0, 0];
        let mut key: Option<(Kind, u16)> = None;
        let mut cur = Resolved::CLASSIC;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let i = y * WIDTH + x;
                let src = plane.map_or(Src::CLASSIC, |p| p[i]);
                let index = pixels[i];
                let mut shown = None;
                if src.kind != Kind::Classic && src.flags == 0 {
                    if key != Some((src.kind, src.id)) {
                        key = Some((src.kind, src.id));
                        cur = res.resolve(src.kind, src.id);
                    }
                    let (u, v) = (usize::from(src.u), usize::from(src.v));
                    if cur.token != NONE && src.ver == cur.ver && u < cur.width && v < cur.height {
                        shown = Some(cur);
                    }
                }
                let token = shown.map_or(NONE, |r| r.token);
                if !full && last.src[i] == src && last.pixels[i] == index && last.tokens[i] == token
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
                let classic = pal[usize::from(index)];
                let o = y * s * ow + x * s;
                match shown {
                    None => {
                        for r in 0..s {
                            out[o + r * ow..o + r * ow + s].fill(classic);
                        }
                    }
                    Some(r) => {
                        let hd = &res.hd.entry(r.slot).hd;
                        let hw = r.width * s;
                        let b = usize::from(src.v) * s * hw + usize::from(src.u) * s;
                        for k in 0..s {
                            let from = &hd[b + k * hw..b + k * hw + s];
                            let to = &mut out[o + k * ow..o + k * ow + s];
                            for (d, &c) in to.iter_mut().zip(from) {
                                *d = if c >> 24 == 0 { classic } else { c };
                            }
                        }
                    }
                }
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

/// Resolves the images of one composition ([`Compositor::compose`]).
struct Resolver<'a> {
    hd: &'a mut HdImages,
    tiles: &'a mut Vec<Slot>,
    others: &'a mut HashMap<(Kind, u16), Slot>,
    input: &'a mut Vec<u32>,
    images: &'a mut dyn Images,
    pal: &'a [u32; 256],
    palette: u64,
    scale: Scale,
}

impl Resolver<'_> {
    /// What image (kind, id) is in this composition: asked of the images once, its HD image
    /// found by its content (through the memo when the content is the memo's).
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
                let (w, h) = (img.width, img.height);
                let shaped = (1..=WIDTH).contains(&w)
                    && (1..=HEIGHT).contains(&h)
                    && img.pixels.len() == w * h
                    && img.transparent.is_none_or(|t| t.len() == w * h);
                if !shaped {
                    Resolved::CLASSIC
                } else {
                    let memo = slot
                        .memo
                        .filter(|&m| self.hd.reached(m, &img, self.palette, self.scale));
                    let (at, token) = match memo {
                        Some(m) => m,
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
                            let at = self.hd.get(self.input, (w, h), self.scale, source);
                            (at, self.hd.entry(at).token)
                        }
                    };
                    slot.memo = Some((at, token));
                    Resolved {
                        token,
                        ver: img.ver,
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

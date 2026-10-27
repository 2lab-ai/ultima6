//! Indexed 8-bit framebuffer for the Ultima VI port.
//!
//! The engine draws palette indices into a [`Framebuffer`]; platforms convert the
//! result to RGBA8 with [`Framebuffer::write_rgba`]. No I/O, no time, no platform code.
//!
//! Spec 07 HD0: a framebuffer can also carry a source plane, one [`Src`] per pixel naming the
//! image the pixel was drawn from ([`Framebuffer::enable_src`]). It is off unless asked for; off,
//! every write does what it did without it, plus one check that the plane is absent. On, each
//! write also marks the 16-pixel chunk it wrote, so that a buffer kept as a copy of another
//! ([`Framebuffer::sync_from`], [`Framebuffer::sync_rect_from`]) copies only the chunks of the
//! plane that may differ; the colours are copied as before. A sync takes the marks it reads, so
//! the first sync between two planes links them, and a later sync of either with another plane
//! panics, with or without debug assertions.
//!
//! [`hd`]: the HD compositor (spec 07 HD0), the frame at 2× or 4× from the source plane. It only
//! reads a plane ([`Framebuffer::src`]), never syncs from one.

pub mod hd;
pub mod motion;

use std::sync::atomic::{AtomicU64, Ordering};

/// Screen width in pixels: the Amiga low-res screen.
#[cfg(not(feature = "wide"))]
pub const WIDTH: usize = 320;
/// Screen height in pixels: the Amiga low-res screen.
#[cfg(not(feature = "wide"))]
pub const HEIGHT: usize = 200;
/// Frame width in pixels, spec 13 Wide mode: the UI's 320 columns, then the 37-cell map.
#[cfg(feature = "wide")]
pub const WIDTH: usize = 912;
/// Frame height in pixels, Wide mode: the 21-cell map's 336 rows (spec 13; the UI's 200 rows at
/// the top), and below the UI the slots of the side panel's three views to row 407 (spec 14
/// Design 1), whole 8-row text cells ([`hd::CELL`]).
#[cfg(feature = "wide")]
pub const HEIGHT: usize = 408;

/// One palette entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// What kind of image a pixel was drawn from ([`Src::kind`]).
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Drawn directly (fills, plots, sprites, pixel replays) or by a write that names no image:
    /// shown as the classic pixel.
    #[default]
    Classic = 0,
    /// A pixel of a 16 × 16 tile record; [`Src::id`] is the record number.
    Tile = 1,
    /// A pixel of a portrait; [`Src::id`] is [`portrait_id`].
    Portrait = 2,
    /// A pixel of a panel bitmap; [`Src::id`] is its role ([`BITMAP_PAPER`], [`BITMAP_SPELLBOOK`]).
    Bitmap = 3,
    /// Spec 11: a pixel of an 8 × 8 character cell; [`Src::id`] is the cell's code (a code of the
    /// game's font, or a Hangul syllable's code point), (u, v) the pixel's place in the cell,
    /// [`FLAG_INK`] on its set pixels, [`FLAG_PAPER`] on those drawn over the paper,
    /// [`FLAG_TYPED`] on a typed cell's, [`FLAG_SYMBOL`] on a symbol's ink pixels. No image:
    /// shown classic (a clear pixel drawn over the paper in the paper's colour at its place shows
    /// the HD paper there, [`hd`]), or by the HD font mode ([`hd::Compositor::set_font_mode`]) as
    /// a listed glyph.
    Glyph = 4,
}

/// [`Src::id`] of the paper bitmap drawn over the whole screen at the boot.
pub const BITMAP_PAPER: u16 = 0;
/// [`Src::id`] of the spellbook bitmap drawn in the side panel.
pub const BITMAP_SPELLBOOK: u16 = 1;

/// [`Src::flags`]: the pixel's colour is the overlay OR over what was there (spec 07 Measured
/// facts: `masktype & 4`); the pixel names the record whose overlay it is.
pub const FLAG_OVERLAY: u8 = 1;
/// [`Src::flags`]: the pixel is an outline pixel, painted in the outline colour over an opaque
/// colour-0 pixel of the record.
pub const FLAG_OUTLINE: u8 = 2;
/// [`Src::flags`] of a [`Kind::Glyph`] pixel: a set pixel of the glyph, in the ink colour (the
/// clear ones are in the paper colour, no flag).
pub const FLAG_INK: u8 = 4;
/// [`Src::flags`] of a [`Kind::Glyph`] pixel: drawn over the paper, the pixel it replaced naming
/// the paper ([`Src::on_paper`]). Only such a pixel may show the HD paper behind the text
/// ([`hd`]); text over a portrait or the spellbook does not.
pub const FLAG_PAPER: u8 = 8;
/// [`Src::flags`] of a pixel naming the paper ([`BITMAP_PAPER`]): written on the paper in
/// another colour than the paper's there (a fill's, a scroll's or a symbol's). Shown classic,
/// as every flagged pixel; named the paper so that a scroll that moves it onto the paper of its
/// colour names the paper there again.
pub const FLAG_PAPER_OFF: u8 = 16;
/// Spec 11 Design 2 (rev 3) [`Src::flags`] of a [`Kind::Glyph`] pixel, with [`FLAG_INK`] and
/// [`FLAG_PAPER`]: an ink pixel of a glyph the game draws as a symbol over the paper (the key
/// wait's cursor, the more prompt's mark), [`Src::id`] that glyph's code. Shown classic, as the
/// ink pixel of a cell the HD font mode does not list; the mode lists the cell as a symbol
/// ([`hd::SYMBOL`]).
pub const FLAG_SYMBOL: u8 = 32;
/// Spec 11 Design 3 (rev 3) [`Src::flags`] of a [`Kind::Glyph`] pixel: of a cell the line editor
/// echoed (a character the player typed), on each pixel of the cell. Shown as without it; the HD
/// font mode lists a whole space typed apart from the game's own ([`hd::TYPED_SPACE`]).
pub const FLAG_TYPED: u8 = 64;
/// Spec 18 [`Src::flags`] of a [`Kind::Tile`] pixel: drawn from the remembered sight (a cell in
/// view that was seen and is out of sight now), its colour index the record's plus [`GREY`]. The
/// HD compositor shows such a pixel as an unflagged one, from the grey copy of the HD image
/// ([`hd`]).
pub const FLAG_MEMORY: u8 = 0x80;

/// Spec 18: the first palette entry of the remembered sight's colours: entry `GREY + i` is
/// [`grey`] of game colour `i` (the game's 32, below `GREY`), written by the map window render
/// while the remembered sight is on.
pub const GREY: u8 = 0x20;

/// Spec 18: the brightness of the remembered sight's grey, out of 256 of the colour's luma.
pub const GREY_LEVEL: u32 = 115;

/// Spec 18: `c` as the remembered sight shows it, in every channel its luma
/// `(77r + 150g + 29b + 128) >> 8` times [`GREY_LEVEL`] / 256, rounded. The classic colours of
/// the remembered sight ([`GREY`]) and the HD compositor's grey copies both take it.
pub const fn grey(c: Rgb) -> Rgb {
    let y = (77 * c.r as u32 + 150 * c.g as u32 + 29 * c.b as u32 + 128) >> 8;
    let v = ((y * GREY_LEVEL + 128) >> 8) as u8;
    Rgb::new(v, v, v)
}

/// [`Src::id`] of portrait `item` of the archive `portrait.<letter>` (the pair the portrait
/// loader computes): the letter's byte in the high byte, the item in the low one. `None` for
/// an item past 255, which no slot reaches; such a portrait is drawn classic.
pub fn portrait_id(letter: u8, item: u16) -> Option<u16> {
    u8::try_from(item)
        .ok()
        .map(|i| u16::from(letter) << 8 | u16::from(i))
}

/// The (letter, item) pair of a portrait [`Src::id`] ([`portrait_id`]).
pub const fn portrait_of(id: u16) -> (u8, u8) {
    ((id >> 8) as u8, id as u8)
}

/// The source of one screen pixel: the image it was drawn from and where in it.
///
/// Layout (`repr(C)`, 12 bytes, alignment 4): `ver` at byte 0 (u32), `id` at 4, `u` at 6, `v`
/// at 8 (u16 each), `kind` at 10 and `flags` at 11 (u8 each). A plane of 320 × 200 is 750 KiB
/// (912 × 408 in Wide mode, 4.3 MiB).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Src {
    /// The image's version when the pixel was drawn: a tile record's count of in-place rewrites
    /// so far (the shore merge), 0 for portraits and bitmaps (they never change while loaded).
    pub ver: u32,
    /// Which image of its kind: the record, the portrait ([`portrait_id`]) or the bitmap role.
    pub id: u16,
    /// The pixel's column inside the image.
    pub u: u16,
    /// The pixel's row inside the image.
    pub v: u16,
    pub kind: Kind,
    /// [`FLAG_OVERLAY`], [`FLAG_OUTLINE`]: the blit changed the image's colour here;
    /// [`FLAG_INK`]: a glyph's set pixel; [`FLAG_PAPER`]: a glyph's pixel over the paper;
    /// [`FLAG_PAPER_OFF`]: the paper's pixel of another colour than the paper's;
    /// [`FLAG_SYMBOL`]: a symbol's ink pixel; [`FLAG_TYPED`]: a typed cell's pixel;
    /// [`FLAG_MEMORY`]: a pixel of the remembered sight.
    pub flags: u8,
}

const _: () = assert!(std::mem::size_of::<Src>() == 12);

impl Src {
    /// The source of every pixel written without one: kind classic, all fields 0.
    pub const CLASSIC: Src = Src {
        ver: 0,
        id: 0,
        u: 0,
        v: 0,
        kind: Kind::Classic,
        flags: 0,
    };

    /// Whether a pixel of this source is on the paper: it names the paper ([`BITMAP_PAPER`],
    /// with or without [`FLAG_PAPER_OFF`]), or it is a glyph's drawn over it ([`FLAG_PAPER`]).
    pub fn on_paper(&self) -> bool {
        match self.kind {
            Kind::Bitmap => self.id == BITMAP_PAPER,
            Kind::Glyph => self.flags & FLAG_PAPER != 0,
            _ => false,
        }
    }
}

/// Pixels per chunk of the change marks: a row is [`ROW_CHUNKS`] chunks.
const CHUNK: usize = 16;
/// Chunks per row.
const ROW_CHUNKS: usize = WIDTH / CHUNK;
const _: () = assert!(WIDTH.is_multiple_of(CHUNK), "a row is whole chunks");
/// Chunks of the screen.
const CHUNKS: usize = ROW_CHUNKS * HEIGHT;
/// Words of a [`Marks`].
const MARK_WORDS: usize = CHUNKS.div_ceil(64);

/// One bit per chunk (16 pixels of a row): chunk k is bit k % 64 of word k / 64.
type Marks = [u64; MARK_WORDS];

/// Every chunk marked.
const ALL: Marks = {
    let mut m = [u64::MAX; MARK_WORDS];
    if !CHUNKS.is_multiple_of(64) {
        m[MARK_WORDS - 1] = (1 << (CHUNKS % 64)) - 1;
    }
    m
};

/// The number of the next [`Plane`] made ([`Plane::id`]).
static NEXT_PLANE: AtomicU64 = AtomicU64::new(1);

/// The source plane with the marks of what changed in it, which let a sync copy only the
/// chunks that may differ ([`Framebuffer::sync_from`]).
struct Plane {
    /// One [`Src`] per pixel, row-major.
    src: Vec<Src>,
    /// Chunks written since the buffer that syncs from this one last did.
    out: Marks,
    /// Chunks written here by anything but a sync into this buffer, since it last synced.
    own: Marks,
    /// This plane's number, distinct for every plane made ([`NEXT_PLANE`]; a clone is a new one).
    id: u64,
    /// The [`Plane::id`] of the one plane synced from this one, 0 before its first sync: `out`
    /// holds that plane's marks ([`Plane::link`]).
    consumer: u64,
    /// The [`Plane::id`] of the one plane this one syncs from, 0 before its first sync: `own`
    /// holds the marks for that plane.
    producer: u64,
}

impl Plane {
    fn classic() -> Plane {
        Plane {
            src: vec![Src::CLASSIC; WIDTH * HEIGHT],
            out: ALL,
            own: ALL,
            id: NEXT_PLANE.fetch_add(1, Ordering::Relaxed),
            consumer: 0,
            producer: 0,
        }
    }

    /// Before a sync from this plane into `to`: links the two at their first sync, and checks the
    /// link at every later one. A sync takes the marks of what changed in both planes since the
    /// last one, so a second plane synced from this one, or `to` synced from a second plane,
    /// would miss changes and keep sources the colours no longer have.
    ///
    /// Panics when either plane is already linked to another (a bug of the caller), with or
    /// without debug assertions.
    fn link(&mut self, to: &mut Plane) {
        if self.consumer == 0 && to.producer == 0 {
            self.consumer = to.id;
            to.producer = self.id;
        }
        assert!(
            self.consumer == to.id && to.producer == self.id,
            "one sync consumer per plane: plane {} syncs into {}, plane {} from {}, not {} into {}",
            self.id,
            self.consumer,
            to.id,
            to.producer,
            self.id,
            to.id
        );
    }

    /// Writes the source of pixel (x, y), at index `i`, and marks its chunk written.
    fn put(&mut self, (x, y): (usize, usize), i: usize, src: Src) {
        self.src[i] = src;
        let k = y * ROW_CHUNKS + x / CHUNK;
        let bit = 1 << (k % 64);
        self.out[k / 64] |= bit;
        self.own[k / 64] |= bit;
    }
}

impl Clone for Plane {
    /// The same sources, every chunk marked: the copy is a new plane, linked to none.
    fn clone(&self) -> Self {
        Plane {
            src: self.src.clone(),
            out: ALL,
            own: ALL,
            id: NEXT_PLANE.fetch_add(1, Ordering::Relaxed),
            consumer: 0,
            producer: 0,
        }
    }
}

/// The first pixel of chunk `k`.
fn chunk_start(k: usize) -> usize {
    k / ROW_CHUNKS * WIDTH + k % ROW_CHUNKS * CHUNK
}

/// A [`WIDTH`] × [`HEIGHT`] grid of palette indices plus the 256-entry palette they refer to,
/// and an optional source plane ([`Framebuffer::enable_src`]).
pub struct Framebuffer {
    pixels: Vec<u8>,
    pub palette: [Rgb; 256],
    /// The source plane, when enabled.
    src: Option<Plane>,
}

impl Clone for Framebuffer {
    fn clone(&self) -> Self {
        Self {
            pixels: self.pixels.clone(),
            palette: self.palette,
            src: self.src.clone(),
        }
    }

    /// Copies into the buffers already held (no allocation). The plane comes along, or goes when
    /// `source` has none; every chunk of it counts as written. A plane held here keeps its sync
    /// links ([`Framebuffer::sync_from`]); one made here is a new plane.
    fn clone_from(&mut self, source: &Self) {
        self.pixels.clone_from(&source.pixels);
        self.palette = source.palette;
        match (&mut self.src, &source.src) {
            (Some(to), Some(from)) => {
                to.src.copy_from_slice(&from.src);
                to.out = ALL;
                to.own = ALL;
            }
            (to, from) => to.clone_from(from),
        }
    }
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Framebuffer {
    /// All pixels index 0, all palette entries black.
    pub fn new() -> Self {
        Self {
            pixels: vec![0; WIDTH * HEIGHT],
            palette: [Rgb::default(); 256],
            src: None,
        }
    }

    /// Adds the source plane, every pixel [`Src::CLASSIC`]; nothing when it is there already.
    pub fn enable_src(&mut self) {
        if self.src.is_none() {
            self.src = Some(Plane::classic());
        }
    }

    /// Every pixel of the source plane classic, the colours untouched; nothing without a plane.
    pub fn reset_src(&mut self) {
        if let Some(plane) = &mut self.src {
            plane.src.fill(Src::CLASSIC);
            plane.out = ALL;
            plane.own = ALL;
        }
    }

    /// The source plane, row-major like [`Framebuffer::pixels`]; `None` when not enabled.
    pub fn src(&self) -> Option<&[Src]> {
        self.src.as_ref().map(|p| p.src.as_slice())
    }

    /// The source of pixel (x, y); `None` outside the screen or without a plane.
    pub fn src_at(&self, x: usize, y: usize) -> Option<Src> {
        let plane = self.src.as_ref()?;
        (x < WIDTH && y < HEIGHT).then(|| plane.src[y * WIDTH + x])
    }

    /// Sets one pixel and, with a plane, its source; out-of-range coordinates are ignored.
    /// Without a plane this is [`Framebuffer::set`].
    pub fn set_src(&mut self, x: usize, y: usize, index: u8, src: Src) {
        if x < WIDTH && y < HEIGHT {
            let i = y * WIDTH + x;
            self.pixels[i] = index;
            if let Some(plane) = &mut self.src {
                plane.put((x, y), i, src);
            }
        }
    }

    /// Copies pixel `i_from` of `from` to pixel `i_to` of this buffer (linear indices), the
    /// colour and the source together: with no plane in `from` the pixel becomes classic, with
    /// none here only the colour is copied. An index past the screen copies nothing.
    pub fn copy_px(&mut self, from: &Framebuffer, i_from: usize, i_to: usize) {
        let n = WIDTH * HEIGHT;
        if i_from >= n || i_to >= n {
            return;
        }
        self.pixels[i_to] = from.pixels[i_from];
        if let Some(plane) = &mut self.src {
            let src = from.src.as_ref().map_or(Src::CLASSIC, |p| p.src[i_from]);
            plane.put((i_to % WIDTH, i_to / WIDTH), i_to, src);
        }
    }

    /// [`Framebuffer::copy_px`] inside this buffer: pixel `i_from` to pixel `i_to`.
    pub fn copy_px_within(&mut self, i_from: usize, i_to: usize) {
        let n = WIDTH * HEIGHT;
        if i_from >= n || i_to >= n {
            return;
        }
        self.pixels[i_to] = self.pixels[i_from];
        if let Some(plane) = &mut self.src {
            let src = plane.src[i_from];
            plane.put((i_to % WIDTH, i_to / WIDTH), i_to, src);
        }
    }

    /// Makes this buffer a copy of `from`, as [`Clone::clone_from`] does, copying of the source
    /// plane only the chunks that may differ: those written in `from` since this buffer last
    /// synced from it, and those written here since. For a buffer that is the one buffer synced
    /// from `from` (the engine's published frame from the screen, the shown frame from the
    /// published one), and synced from no other: another buffer syncing from `from` would take
    /// the marks this one needs. The first sync between two planes links them, and a sync of
    /// either with another plane after that panics (with or without debug assertions; a plane
    /// made here is a new one, and reads, clones and pixel copies of `from` are no syncs; a
    /// refused sync changes neither buffer). The colours and the palette are copied whole, as by
    /// `clone_from`.
    ///
    /// Returns the chunks of the plane it copied (all of them for a plane it had to create).
    pub fn sync_from(&mut self, from: &mut Framebuffer) -> usize {
        let mut copied = 0;
        // The plane first: its link is checked before anything is copied.
        match (&mut self.src, &mut from.src) {
            (Some(to), Some(src)) => {
                src.link(to);
                for w in 0..MARK_WORDS {
                    let mut bits = src.out[w] | to.own[w];
                    to.out[w] |= bits;
                    src.out[w] = 0;
                    to.own[w] = 0;
                    while bits != 0 {
                        let i = chunk_start(w * 64 + bits.trailing_zeros() as usize);
                        bits &= bits - 1;
                        to.src[i..i + CHUNK].copy_from_slice(&src.src[i..i + CHUNK]);
                        copied += 1;
                    }
                }
            }
            (to @ None, Some(src)) => {
                let mut plane = src.clone();
                src.link(&mut plane);
                plane.own = [0; MARK_WORDS];
                *to = Some(plane);
                src.out = [0; MARK_WORDS];
                copied = CHUNKS;
            }
            (to, None) => *to = None,
        }
        self.pixels.clone_from(&from.pixels);
        self.palette = from.palette;
        debug_assert!(self.src() == from.src(), "a sync leaves the planes equal");
        copied
    }

    /// [`Framebuffer::sync_from`] for the source plane alone, the colours left as they are: of
    /// the chunks that may differ, those whose colours here already equal `from`'s are copied
    /// (their sources are those of the pixels shown here); a chunk whose colours differ keeps its
    /// marks for the next `sync_from`, which copies it with its colours. For the same one buffer
    /// synced from `from`, and checked as `sync_from` checks it. Returns the chunks it copied.
    pub fn sync_src_from(&mut self, from: &mut Framebuffer) -> usize {
        let (Some(to), Some(src)) = (&mut self.src, &mut from.src) else {
            return 0;
        };
        src.link(to);
        let mut copied = 0;
        for w in 0..MARK_WORDS {
            let mut bits = src.out[w] | to.own[w];
            while bits != 0 {
                let bit = bits & bits.wrapping_neg();
                bits &= bits - 1;
                let i = chunk_start(w * 64 + bit.trailing_zeros() as usize);
                if self.pixels[i..i + CHUNK] != from.pixels[i..i + CHUNK] {
                    continue;
                }
                to.src[i..i + CHUNK].copy_from_slice(&src.src[i..i + CHUNK]);
                src.out[w] &= !bit;
                to.own[w] &= !bit;
                to.out[w] |= bit;
                copied += 1;
            }
        }
        copied
    }

    /// The rect `xs` × `ys` of `from` copied here: each row of it whose colours differ is copied,
    /// and of the source plane the chunks of the rect that may differ (as [`Framebuffer::sync_from`]
    /// marks them, for the one buffer synced from `from`, always over the same rect, and checked as
    /// `sync_from` checks it, before anything is copied). With no plane in `from` the rect's
    /// sources become classic. A rect reaching past the screen copies nothing, and so does an
    /// empty one. Returns the chunks of the plane it copied.
    pub fn sync_rect_from(
        &mut self,
        from: &mut Framebuffer,
        xs: std::ops::Range<usize>,
        ys: std::ops::Range<usize>,
    ) -> usize {
        if xs.start > xs.end || ys.start > ys.end || xs.end > WIDTH || ys.end > HEIGHT {
            return 0;
        }
        if xs.is_empty() || ys.is_empty() {
            return 0;
        }
        let mut copied = 0;
        let chunks = xs.start / CHUNK..(xs.end - 1) / CHUNK + 1;
        let span = |k: usize| {
            let x = k % ROW_CHUNKS * CHUNK;
            let row = k / ROW_CHUNKS * WIDTH;
            row + x.max(xs.start)..row + (x + CHUNK).min(xs.end)
        };
        match (&mut self.src, &mut from.src) {
            (Some(to), Some(src)) => {
                src.link(to);
                let words = ys.start * ROW_CHUNKS / 64..(ys.end * ROW_CHUNKS).div_ceil(64);
                for w in words {
                    let mut bits = src.out[w] | to.own[w];
                    while bits != 0 {
                        let k = w * 64 + bits.trailing_zeros() as usize;
                        bits &= bits - 1;
                        if !ys.contains(&(k / ROW_CHUNKS)) || !chunks.contains(&(k % ROW_CHUNKS)) {
                            continue;
                        }
                        let bit = 1 << (k % 64);
                        src.out[w] &= !bit;
                        to.own[w] &= !bit;
                        to.out[w] |= bit;
                        let r = span(k);
                        to.src[r.clone()].copy_from_slice(&src.src[r]);
                        copied += 1;
                    }
                }
            }
            (Some(to), None) => {
                for y in ys.clone() {
                    for c in chunks.clone() {
                        let k = y * ROW_CHUNKS + c;
                        to.src[span(k)].fill(Src::CLASSIC);
                        to.out[k / 64] |= 1 << (k % 64);
                        copied += 1;
                    }
                }
            }
            (None, _) => {}
        }
        for y in ys.clone() {
            let row = y * WIDTH + xs.start..y * WIDTH + xs.end;
            if self.pixels[row.clone()] != from.pixels[row.clone()] {
                self.pixels[row.clone()].copy_from_slice(&from.pixels[row]);
            }
        }
        #[cfg(debug_assertions)]
        if let Some(to) = &self.src {
            for y in ys {
                let row = y * WIDTH + xs.start..y * WIDTH + xs.end;
                let want = from.src.as_ref().map(|p| &p.src[row.clone()]);
                let same = match want {
                    Some(w) => to.src[row] == *w,
                    None => to.src[row].iter().all(|s| *s == Src::CLASSIC),
                };
                debug_assert!(same, "a rect sync leaves the rect's sources equal");
            }
        }
        copied
    }

    pub const fn width(&self) -> usize {
        WIDTH
    }

    pub const fn height(&self) -> usize {
        HEIGHT
    }

    /// Row-major palette indices, `WIDTH * HEIGHT` bytes.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Every pixel `index` (and classic).
    pub fn clear(&mut self, index: u8) {
        self.pixels.fill(index);
        if let Some(plane) = &mut self.src {
            plane.src.fill(Src::CLASSIC);
            plane.out = ALL;
            plane.own = ALL;
        }
    }

    /// Palette index at (x, y), or `None` outside the screen.
    pub fn get(&self, x: usize, y: usize) -> Option<u8> {
        (x < WIDTH && y < HEIGHT).then(|| self.pixels[y * WIDTH + x])
    }

    /// Sets one pixel (classic); out-of-range coordinates are ignored.
    pub fn set(&mut self, x: usize, y: usize, index: u8) {
        if x < WIDTH && y < HEIGHT {
            let i = y * WIDTH + x;
            self.pixels[i] = index;
            if let Some(plane) = &mut self.src {
                plane.put((x, y), i, Src::CLASSIC);
            }
        }
    }

    /// Copies a `w`x`h` row-major indexed sprite to (x, y), clipped to the screen; the pixels
    /// written are classic. Pixels equal to `transparent` are skipped.
    pub fn blit(
        &mut self,
        x: i32,
        y: i32,
        w: usize,
        h: usize,
        sprite: &[u8],
        transparent: Option<u8>,
    ) {
        assert!(sprite.len() >= w * h, "sprite smaller than {w}x{h}");
        for sy in 0..h {
            let dy = y as i64 + sy as i64;
            if !(0..HEIGHT as i64).contains(&dy) {
                continue;
            }
            for sx in 0..w {
                let dx = x as i64 + sx as i64;
                if !(0..WIDTH as i64).contains(&dx) {
                    continue;
                }
                let index = sprite[sy * w + sx];
                if transparent != Some(index) {
                    let (px, py) = (dx as usize, dy as usize);
                    let i = py * WIDTH + px;
                    self.pixels[i] = index;
                    if let Some(plane) = &mut self.src {
                        plane.put((px, py), i, Src::CLASSIC);
                    }
                }
            }
        }
    }

    /// Converts the indexed image to RGBA8 (alpha 255), resizing `out` to
    /// `WIDTH * HEIGHT * 4` bytes.
    pub fn write_rgba(&self, out: &mut Vec<u8>) {
        out.resize(WIDTH * HEIGHT * 4, 0);
        let (quads, _) = out.as_chunks_mut::<4>();
        for (dst, &index) in quads.iter_mut().zip(&self.pixels) {
            let c = self.palette[index as usize];
            *dst = [c.r, c.g, c.b, 255];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec 18: the grey of a colour is its luma at [`GREY_LEVEL`] / 256 in every channel: black
    /// stays black, white is the level's grey, a green brighter than a blue of the same value.
    #[test]
    fn grey_is_the_dimmed_luma() {
        assert_eq!(grey(Rgb::new(0, 0, 0)), Rgb::new(0, 0, 0));
        let w = ((255 * GREY_LEVEL + 128) >> 8) as u8;
        assert_eq!(grey(Rgb::new(255, 255, 255)), Rgb::new(w, w, w));
        let (g, b) = (grey(Rgb::new(0, 200, 0)), grey(Rgb::new(0, 0, 200)));
        assert!(g.r == g.g && g.g == g.b && g.r > b.r);
        assert!(usize::from(GREY) == 32, "past the game's 32 colours");
    }

    #[test]
    fn rgba_uses_palette_and_opaque_alpha() {
        let mut fb = Framebuffer::new();
        fb.palette[7] = Rgb::new(10, 20, 30);
        fb.set(1, 0, 7);
        let mut out = Vec::new();
        fb.write_rgba(&mut out);
        assert_eq!(out.len(), WIDTH * HEIGHT * 4);
        assert_eq!(&out[0..4], &[0, 0, 0, 255]);
        assert_eq!(&out[4..8], &[10, 20, 30, 255]);
    }

    #[test]
    fn blit_clips_at_every_edge() {
        let mut fb = Framebuffer::new();
        let sprite = [9u8; 4];
        fb.blit(-1, -1, 2, 2, &sprite, None);
        fb.blit(WIDTH as i32 - 1, HEIGHT as i32 - 1, 2, 2, &sprite, None);
        assert_eq!(fb.get(0, 0), Some(9));
        assert_eq!(fb.get(1, 0), Some(0));
        assert_eq!(fb.get(WIDTH - 1, HEIGHT - 1), Some(9));
        assert_eq!(fb.pixels().iter().filter(|&&p| p == 9).count(), 2);
        fb.blit(i32::MIN, i32::MAX, 2, 2, &sprite, None);
        assert_eq!(fb.pixels().iter().filter(|&&p| p == 9).count(), 2);
    }

    #[test]
    fn blit_skips_transparent_index() {
        let mut fb = Framebuffer::new();
        fb.clear(3);
        fb.blit(0, 0, 2, 1, &[0, 5], Some(0));
        assert_eq!(fb.get(0, 0), Some(3));
        assert_eq!(fb.get(1, 0), Some(5));
    }

    fn tile(id: u16, u: u16, v: u16) -> Src {
        Src {
            ver: 3,
            id,
            u,
            v,
            kind: Kind::Tile,
            flags: 0,
        }
    }

    fn kind(fb: &Framebuffer, x: usize, y: usize) -> Option<Kind> {
        fb.src_at(x, y).map(|s| s.kind)
    }

    #[test]
    fn the_plane_is_off_until_enabled_and_then_classic() {
        let mut fb = Framebuffer::new();
        assert!(fb.src().is_none() && fb.src_at(0, 0).is_none());
        fb.set_src(4, 5, 9, tile(7, 1, 2));
        assert_eq!(fb.get(4, 5), Some(9), "without a plane set_src is set");
        assert!(fb.src().is_none(), "and allocates nothing");
        fb.enable_src();
        assert_eq!(fb.src().map(<[Src]>::len), Some(WIDTH * HEIGHT));
        assert!(fb.src().unwrap().iter().all(|s| *s == Src::CLASSIC));
        fb.set_src(4, 5, 9, tile(7, 1, 2));
        fb.enable_src();
        assert_eq!(
            fb.src_at(4, 5),
            Some(tile(7, 1, 2)),
            "a second enable keeps it"
        );
        assert_eq!(fb.src_at(WIDTH, 0), None);
        fb.set_src(WIDTH, 0, 1, tile(1, 0, 0));
        fb.set_src(0, HEIGHT, 1, tile(1, 0, 0));
        assert_eq!(fb.pixels().iter().filter(|&&p| p == 1).count(), 0);
        assert_eq!(std::mem::size_of::<Src>(), 12);
    }

    #[test]
    fn set_clear_and_blit_mark_their_pixels_classic() {
        let mut fb = Framebuffer::new();
        fb.enable_src();
        for x in 0..4 {
            fb.set_src(x, 0, 2, tile(5, x as u16, 0));
        }
        fb.set(0, 0, 3);
        fb.blit(1, 0, 2, 1, &[0, 6], Some(0));
        assert_eq!(kind(&fb, 0, 0), Some(Kind::Classic), "set");
        assert_eq!(
            kind(&fb, 1, 0),
            Some(Kind::Tile),
            "a transparent pixel is left"
        );
        assert_eq!(kind(&fb, 2, 0), Some(Kind::Classic), "blit");
        assert_eq!(fb.src_at(3, 0), Some(tile(5, 3, 0)));
        fb.clear(0);
        assert!(fb.src().unwrap().iter().all(|s| s.kind == Kind::Classic));
    }

    #[test]
    fn copy_px_moves_the_colour_and_the_source_together() {
        let mut a = Framebuffer::new();
        a.enable_src();
        a.set_src(10, 1, 8, tile(9, 4, 5));
        let mut b = Framebuffer::new();
        b.enable_src();
        b.set_src(0, 0, 1, tile(2, 0, 0));
        b.copy_px(&a, WIDTH + 10, 7);
        assert_eq!(
            (b.get(7, 0), b.src_at(7, 0)),
            (Some(8), Some(tile(9, 4, 5)))
        );
        // From a buffer without a plane the pixel is classic; into one only the colour moves.
        let plain = Framebuffer::new();
        b.copy_px(&plain, 0, 0);
        assert_eq!(
            (b.get(0, 0), kind(&b, 0, 0)),
            (Some(0), Some(Kind::Classic))
        );
        let mut c = Framebuffer::new();
        c.copy_px(&a, WIDTH + 10, 3);
        assert_eq!((c.get(3, 0), c.src()), (Some(8), None));
        // Past the screen nothing is copied.
        b.copy_px(&a, WIDTH * HEIGHT, 1);
        b.copy_px(&a, 0, WIDTH * HEIGHT);
        assert_eq!(b.get(1, 0), Some(0));
    }

    #[test]
    fn copy_px_within_moves_a_pixel_and_its_source() {
        let mut a = Framebuffer::new();
        a.enable_src();
        a.set_src(3, 9, 7, tile(2, 3, 1));
        a.copy_px_within(9 * WIDTH + 3, 1);
        assert_eq!(
            (a.get(1, 0), a.src_at(1, 0)),
            (Some(7), Some(tile(2, 3, 1)))
        );
        a.copy_px_within(WIDTH * HEIGHT, 0);
        a.copy_px_within(0, WIDTH * HEIGHT);
        assert_eq!(
            (a.get(0, 0), kind(&a, 0, 0)),
            (Some(0), Some(Kind::Classic))
        );
        let mut b = Framebuffer::new();
        b.set(3, 0, 4);
        b.copy_px_within(3, 5);
        assert_eq!((b.get(5, 0), b.src()), (Some(4), None));
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

    /// A random write: a source pixel, a classic one, a classic blit, a copy from `other`, a copy
    /// inside the buffer; rarely a clear.
    fn scribble(fb: &mut Framebuffer, other: &Framebuffer, x: &mut X) {
        let (px, py) = (
            x.below(WIDTH as u64) as usize,
            x.below(HEIGHT as u64) as usize,
        );
        let i = py * WIDTH + px;
        match x.below(100) {
            0..=49 => {
                let s = tile(x.below(2048) as u16, x.below(16) as u16, x.below(16) as u16);
                fb.set_src(px, py, x.below(32) as u8, s);
            }
            50..=64 => fb.set(px, py, x.below(32) as u8),
            65..=74 => fb.blit(px as i32 - 4, py as i32 - 4, 9, 9, &[3; 81], Some(0)),
            75..=89 => fb.copy_px(other, x.below((WIDTH * HEIGHT) as u64) as usize, i),
            90..=98 => fb.copy_px_within(x.below((WIDTH * HEIGHT) as u64) as usize, i),
            _ => fb.clear(x.below(32) as u8),
        }
    }

    fn rect_equal(a: &Framebuffer, b: &Framebuffer, xs: std::ops::Range<usize>) -> bool {
        (8..168).all(|y| {
            let row = y * WIDTH + xs.start..y * WIDTH + xs.end;
            a.pixels[row.clone()] == b.pixels[row.clone()]
                && a.src().unwrap()[row.clone()] == b.src().unwrap()[row]
        })
    }

    /// The engine's chain: back to the screen over the window rect, the screen to the published
    /// frame, that to the shown frame with a sprite drawn over it; random writes on every buffer
    /// between the syncs. After each sync the buffers are equal where it copies, and a sync with
    /// nothing written since copies no chunk of the plane.
    #[test]
    fn syncs_copy_what_changed_and_leave_the_buffers_equal() {
        let mut bufs: [Framebuffer; 4] = std::array::from_fn(|_| Framebuffer::new());
        bufs.iter_mut().for_each(Framebuffer::enable_src);
        let [back, front, published, shown] = &mut bufs;
        let mut x = X(0x9E37_79B9_7F4A_7C15);
        let mut copied = [0usize; 3];
        for step in 0..400 {
            for _ in 0..x.below(40) {
                scribble(back, front, &mut x);
            }
            for _ in 0..x.below(20) {
                scribble(front, back, &mut x);
            }
            if x.below(10) == 0 {
                scribble(published, back, &mut x);
            }
            copied[0] += front.sync_rect_from(back, 8..168, 8..168);
            assert!(
                rect_equal(front, back, 8..168),
                "step {step}: the window rect"
            );
            let publish = x.below(4) != 0;
            if publish {
                copied[1] += published.sync_from(front);
                assert!(published.src() == front.src(), "step {step}: published");
                assert!(published.pixels == front.pixels && published.palette == front.palette);
            }
            copied[2] += shown.sync_from(published);
            assert!(shown.src() == published.src(), "step {step}: shown");
            assert_eq!(front.sync_rect_from(back, 8..168, 8..168), 0, "nothing new");
            if publish {
                assert_eq!(published.sync_from(front), 0, "step {step}: nothing new");
            }
            assert_eq!(shown.sync_from(published), 0, "step {step}: nothing new");
            let (sx, sy) = (x.below(300) as i32, x.below(190) as i32);
            shown.blit(sx, sy, 16, 10, &[17; 160], None);
        }
        let all = CHUNKS * 400;
        assert!(
            copied.iter().all(|&c| c > 0 && c < all),
            "copied {copied:?} of {all}"
        );
    }

    /// A buffer with the plane.
    fn planed() -> Framebuffer {
        let mut fb = Framebuffer::new();
        fb.enable_src();
        fb
    }

    /// Whether `f` panics on a sync link ([`Plane::link`]).
    fn refused(f: impl FnOnce()) -> bool {
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        caught.is_err_and(|e| {
            e.downcast_ref::<String>()
                .is_some_and(|m| m.starts_with("one sync consumer per plane"))
        })
    }

    /// The first sync from a plane links the two. A second buffer synced from it (by any of the
    /// three syncs, or by the sync that makes its plane) and the linked buffer synced from a
    /// second plane panic at that sync: their first, whose full copy (a new plane has every chunk
    /// marked) leaves nothing for the equality check to see, and without debug assertions. A
    /// clone is a new plane. Reads, clones, pixel copies, a reset, a clear and a clone_from are
    /// no syncs, and the linked pair goes on as before, each in its own link the other way.
    #[test]
    fn a_plane_has_one_sync_consumer_and_a_consumer_one_source() {
        let (mut from, mut to) = (planed(), planed());
        to.sync_from(&mut from);
        assert!(from.src().is_some() && from.src_at(3, 3).is_some());
        let mut copy = from.clone();
        copy.copy_px(&from, 0, 0);
        to.copy_px(&from, 0, 1);
        from.reset_src();
        to.clear(2);
        to.clone_from(&copy);
        from.set_src(40, 9, 6, tile(9, 1, 2));
        assert!(!refused(|| {
            to.sync_from(&mut from);
            to.sync_src_from(&mut from);
            to.sync_rect_from(&mut from, 8..168, 8..168);
        }));
        assert!(to.src() == from.src(), "the pair goes on");
        let mut next = planed();
        next.sync_from(&mut to);
        assert!(
            next.src() == to.src(),
            "a consumer also feeds one of its own"
        );
        let second = [
            refused(|| _ = planed().sync_from(&mut from)),
            refused(|| _ = Framebuffer::new().sync_from(&mut from)),
            refused(|| _ = planed().sync_src_from(&mut from)),
            refused(|| _ = planed().sync_rect_from(&mut from, 8..168, 8..168)),
            refused(|| _ = to.clone().sync_from(&mut from)),
            refused(|| _ = to.sync_from(&mut planed())),
            refused(|| _ = to.sync_src_from(&mut planed())),
            refused(|| _ = to.sync_rect_from(&mut planed(), 8..168, 8..168)),
        ];
        assert_eq!(second, [true; 8], "a second consumer, then a second source");
        // The plane a first sync makes is linked too; a buffer without a plane takes no marks.
        let (mut made, mut plain) = (Framebuffer::new(), Framebuffer::new());
        made.sync_from(&mut copy);
        assert!(refused(|| _ = planed().sync_from(&mut copy)));
        assert_eq!(made.sync_from(&mut copy), 0);
        assert!(!refused(|| {
            planed().sync_rect_from(&mut plain, 8..168, 8..168);
            Framebuffer::new().sync_from(&mut plain);
        }));
        from.set_src(41, 9, 7, tile(9, 2, 2));
        to.sync_from(&mut from);
        assert!(to.src() == from.src() && to.pixels == from.pixels);
    }

    /// Everything a buffer holds: its colours, its palette, and its sources, marks and links.
    type Whole = (
        Vec<u8>,
        Vec<Rgb>,
        Option<(Vec<Src>, Marks, Marks, [u64; 3])>,
    );

    fn whole(fb: &Framebuffer) -> Whole {
        let plane = fb.src.as_ref().map(|p| {
            let links = [p.id, p.consumer, p.producer];
            (p.src.clone(), p.out, p.own, links)
        });
        (fb.pixels.clone(), fb.palette.to_vec(), plane)
    }

    /// A refused sync changes neither buffer: by each of the three syncs, a second consumer (and
    /// for the full sync one without a plane, which it would make) and a second source leave the
    /// colours, the palette, the sources, the marks and the links of both as they were. The
    /// linked pair then goes on with the marks it left.
    #[test]
    fn a_refused_sync_changes_neither_buffer() {
        let (mut from, mut to, mut other) = (planed(), planed(), planed());
        to.sync_from(&mut from);
        from.set_src(40, 9, 6, tile(9, 1, 2));
        from.palette[6] = Rgb::new(9, 9, 9);
        other.set_src(41, 9, 7, tile(8, 1, 2));
        other.palette[7] = Rgb::new(7, 7, 7);
        let mut second = planed();
        second.set_src(12, 12, 5, tile(7, 0, 0));
        let mut bare = Framebuffer::new();
        bare.set(12, 12, 2);
        type Sync = fn(&mut Framebuffer, &mut Framebuffer);
        let syncs: [(&str, Sync); 3] = [
            ("sync_from", |a, b| _ = a.sync_from(b)),
            ("sync_src_from", |a, b| _ = a.sync_src_from(b)),
            ("sync_rect_from", |a, b| {
                _ = a.sync_rect_from(b, 8..168, 8..168)
            }),
        ];
        for (name, sync) in syncs {
            let check = |what: &str, dst: &mut Framebuffer, src: &mut Framebuffer| {
                let before = (whole(dst), whole(src));
                assert!(refused(|| sync(dst, src)), "{name}: {what} is refused");
                assert!(
                    (whole(dst), whole(src)) == before,
                    "{name}: {what} changes nothing"
                );
            };
            check("a second consumer", &mut second, &mut from);
            check("a second source", &mut to, &mut other);
            if name == "sync_from" {
                check("a second consumer without a plane", &mut bare, &mut from);
            }
        }
        assert_eq!(to.sync_from(&mut from), 1, "the chunk written since");
        assert!(to.src() == from.src() && to.pixels == from.pixels && to.palette == from.palette);
    }

    /// The plane alone: the chunks whose colours are already the source's are copied and their
    /// marks taken; a chunk whose colours differ keeps them, and the next full sync copies it.
    #[test]
    fn a_sync_of_the_plane_alone_copies_only_chunks_of_equal_colours() {
        let (mut from, mut to) = (Framebuffer::new(), Framebuffer::new());
        from.enable_src();
        to.enable_src();
        assert_eq!(to.sync_from(&mut from), CHUNKS);
        from.set_src(3, 2, 0, tile(5, 3, 2));
        from.set_src(40, 2, 6, tile(5, 8, 2));
        assert_eq!(to.sync_src_from(&mut from), 1, "the chunk of equal colours");
        assert_eq!(to.src_at(3, 2), Some(tile(5, 3, 2)));
        assert_eq!(
            (to.get(40, 2), kind(&to, 40, 2)),
            (Some(0), Some(Kind::Classic))
        );
        assert_eq!(
            to.sync_src_from(&mut from),
            0,
            "the other waits for its colours"
        );
        assert_eq!(to.sync_from(&mut from), 1, "and the next sync copies it");
        assert!(to.src() == from.src() && to.pixels == from.pixels);
        let mut plain = Framebuffer::new();
        assert_eq!(plain.sync_src_from(&mut from), 0);
        assert!(plain.src().is_none());
    }

    #[test]
    fn syncs_without_a_plane_and_rects_past_the_screen() {
        let mut a = Framebuffer::new();
        a.set(5, 5, 9);
        a.palette[9] = Rgb::new(1, 2, 3);
        let mut b = Framebuffer::new();
        assert_eq!(b.sync_from(&mut a), 0);
        assert!(b.pixels == a.pixels && b.palette == a.palette && b.src().is_none());
        // A plane appears and goes with the source's.
        a.enable_src();
        a.set_src(5, 5, 9, tile(4, 1, 1));
        assert_eq!(b.sync_from(&mut a), CHUNKS);
        assert_eq!(b.src_at(5, 5), Some(tile(4, 1, 1)));
        assert_eq!(b.sync_from(&mut a), 0, "the new plane's marks were taken");
        let mut plain = Framebuffer::new();
        assert_eq!(b.sync_from(&mut plain), 0);
        assert!(b.src().is_none() && b.get(5, 5) == Some(0));
        // A rect from a buffer without a plane: the rect's sources classic.
        let mut c = Framebuffer::new();
        c.enable_src();
        c.set_src(20, 20, 2, tile(1, 0, 0));
        c.set_src(40, 20, 2, tile(1, 0, 0));
        c.sync_rect_from(&mut plain, 8..32, 8..168);
        assert_eq!(
            (c.get(20, 20), kind(&c, 20, 20)),
            (Some(0), Some(Kind::Classic))
        );
        assert_eq!(kind(&c, 40, 20), Some(Kind::Tile), "outside the rect");
        // Empty rects at the screen's end and rects past it or upside down copy nothing.
        let (last_x, last_y) = (WIDTH - 1, HEIGHT - 1);
        let mut d = Framebuffer::new();
        d.enable_src();
        d.set_src(last_x, last_y, 6, tile(2, 0, 0));
        let mut e = Framebuffer::new();
        e.enable_src();
        assert_eq!(e.sync_rect_from(&mut d, WIDTH..WIDTH, 0..HEIGHT), 0);
        assert_eq!(e.sync_rect_from(&mut d, 0..WIDTH, HEIGHT..HEIGHT), 0);
        assert_eq!(
            e.sync_rect_from(&mut d, WIDTH - 20..WIDTH + 1, HEIGHT - 10..HEIGHT),
            0
        );
        assert_eq!(
            e.sync_rect_from(&mut d, WIDTH - 20..WIDTH, HEIGHT - 10..HEIGHT + 1),
            0
        );
        #[allow(clippy::reversed_empty_ranges)]
        let upside = e.sync_rect_from(&mut d, WIDTH - 10..WIDTH - 20, HEIGHT - 10..HEIGHT);
        assert_eq!(upside, 0);
        assert_eq!(
            (e.get(last_x, last_y), kind(&e, last_x, last_y)),
            (Some(0), Some(Kind::Classic))
        );
        assert_eq!(
            e.sync_rect_from(&mut d, WIDTH - CHUNK..WIDTH, last_y..HEIGHT),
            1,
            "the last chunk"
        );
        assert_eq!(
            (e.get(last_x, last_y), e.src_at(last_x, last_y)),
            (Some(6), Some(tile(2, 0, 0)))
        );
    }

    /// A clone_from brings the plane or takes it away; over a plane already there it marks every
    /// chunk, for the buffer synced from this one and for this one's next sync.
    #[test]
    fn clone_from_carries_the_plane_and_its_absence() {
        let mut on = Framebuffer::new();
        on.enable_src();
        on.set_src(1, 1, 4, tile(3, 2, 1));
        on.palette[4] = Rgb::new(1, 2, 3);
        let mut dst = Framebuffer::new();
        dst.clone_from(&on);
        assert_eq!(dst.src_at(1, 1), Some(tile(3, 2, 1)), "none to some");
        assert_eq!(
            (dst.get(1, 1), dst.palette[4]),
            (Some(4), Rgb::new(1, 2, 3))
        );
        // Some to some between syncs: `dst` synced from `feed` and into `copy`, then made `on`
        // again. Every chunk counts as written, so the next syncs copy it all, both ways.
        let (mut feed, mut copy) = (planed(), planed());
        feed.set_src(7, 7, 2, tile(8, 1, 1));
        dst.sync_from(&mut feed);
        copy.sync_from(&mut dst);
        on.set_src(2, 1, 6, tile(4, 0, 0));
        dst.clone_from(&on);
        assert_eq!(dst.src_at(2, 1), Some(tile(4, 0, 0)), "some to some");
        copy.sync_from(&mut dst);
        assert!(
            copy.src() == dst.src() && dst.src() == on.src(),
            "the buffer synced from it copies the clone"
        );
        dst.sync_from(&mut feed);
        assert!(
            dst.src() == feed.src(),
            "and it copies again the buffer it syncs from"
        );
        copy.sync_from(&mut dst);
        assert!(copy.src() == feed.src());
        dst.clone_from(&Framebuffer::new());
        assert!(
            dst.src().is_none() && dst.get(1, 1) == Some(0),
            "some to none"
        );
        assert_eq!(on.clone().src(), on.src());
    }

    #[test]
    fn portrait_ids_pack_the_letter_over_the_item() {
        assert_eq!(portrait_id(b'b', 0x9c), Some(0x629c));
        assert_eq!(portrait_id(b'a', 0), Some(0x6100));
        assert_eq!(portrait_id(b'z', 255), Some(0x7aff));
        assert_eq!(portrait_of(0x629c), (b'b', 0x9c));
        assert_eq!(portrait_id(b'z', 256), None);
        assert_eq!(portrait_id(b'a', u16::MAX), None);
    }
}

//! Indexed 8-bit framebuffer for the Ultima VI port.
//!
//! The engine draws palette indices into a [`Framebuffer`]; platforms convert the
//! result to RGBA8 with [`Framebuffer::write_rgba`]. No I/O, no time, no platform code.
//!
//! Spec 07 HD0: a framebuffer can also carry a source plane, one [`Src`] per pixel naming the
//! image the pixel was drawn from ([`Framebuffer::enable_src`]). It is off unless asked for; off,
//! every write does what it did without it, plus one check that the plane is absent.

/// Screen width in pixels (Amiga low-res).
pub const WIDTH: usize = 320;
/// Screen height in pixels.
pub const HEIGHT: usize = 200;

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
    /// Drawn directly (text, fills, plots, sprites, pixel replays) or by a write that names no
    /// image: shown as the classic pixel.
    #[default]
    Classic = 0,
    /// A pixel of a 16 × 16 tile record; [`Src::id`] is the record number.
    Tile = 1,
    /// A pixel of a portrait; [`Src::id`] is [`portrait_id`].
    Portrait = 2,
    /// A pixel of a panel bitmap; [`Src::id`] is its role ([`BITMAP_PAPER`], [`BITMAP_SPELLBOOK`]).
    Bitmap = 3,
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
/// at 8 (u16 each), `kind` at 10 and `flags` at 11 (u8 each). A plane of 320 × 200 is 750 KiB.
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
    /// [`FLAG_OVERLAY`], [`FLAG_OUTLINE`]: the blit changed the image's colour here.
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
}

/// A 320x200 grid of palette indices plus the 256-entry palette they refer to, and an optional
/// source plane ([`Framebuffer::enable_src`]).
pub struct Framebuffer {
    pixels: Vec<u8>,
    pub palette: [Rgb; 256],
    /// One [`Src`] per pixel, row-major, when enabled.
    src: Option<Vec<Src>>,
}

impl Clone for Framebuffer {
    fn clone(&self) -> Self {
        Self {
            pixels: self.pixels.clone(),
            palette: self.palette,
            src: self.src.clone(),
        }
    }

    /// Copies into the buffers already held (the published frame and the shown frame are copied
    /// once per change; no allocation then). The plane comes along, or goes when `source` has
    /// none.
    fn clone_from(&mut self, source: &Self) {
        self.pixels.clone_from(&source.pixels);
        self.palette = source.palette;
        self.src.clone_from(&source.src);
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
            self.src = Some(vec![Src::CLASSIC; WIDTH * HEIGHT]);
        }
    }

    /// The source plane, row-major like [`Framebuffer::pixels`]; `None` when not enabled.
    pub fn src(&self) -> Option<&[Src]> {
        self.src.as_deref()
    }

    /// The source of pixel (x, y); `None` outside the screen or without a plane.
    pub fn src_at(&self, x: usize, y: usize) -> Option<Src> {
        let src = self.src.as_ref()?;
        (x < WIDTH && y < HEIGHT).then(|| src[y * WIDTH + x])
    }

    /// Sets one pixel and, with a plane, its source; out-of-range coordinates are ignored.
    /// Without a plane this is [`Framebuffer::set`].
    pub fn set_src(&mut self, x: usize, y: usize, index: u8, src: Src) {
        if x < WIDTH && y < HEIGHT {
            let i = y * WIDTH + x;
            self.pixels[i] = index;
            if let Some(plane) = &mut self.src {
                plane[i] = src;
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
            plane[i_to] = from.src.as_ref().map_or(Src::CLASSIC, |s| s[i_from]);
        }
    }

    /// [`Framebuffer::copy_px`] over `len` consecutive pixels from `i_from` to `i_to`; a span
    /// that would reach past the screen copies nothing.
    pub fn copy_span(&mut self, from: &Framebuffer, i_from: usize, i_to: usize, len: usize) {
        let n = WIDTH * HEIGHT;
        if i_from.checked_add(len).is_none_or(|e| e > n)
            || i_to.checked_add(len).is_none_or(|e| e > n)
        {
            return;
        }
        self.pixels[i_to..i_to + len].copy_from_slice(&from.pixels[i_from..i_from + len]);
        if let Some(plane) = &mut self.src {
            let to = &mut plane[i_to..i_to + len];
            match &from.src {
                Some(s) => to.copy_from_slice(&s[i_from..i_from + len]),
                None => to.fill(Src::CLASSIC),
            }
        }
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
            plane.fill(Src::CLASSIC);
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
                plane[i] = Src::CLASSIC;
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
                    let i = dy as usize * WIDTH + dx as usize;
                    self.pixels[i] = index;
                    if let Some(plane) = &mut self.src {
                        plane[i] = Src::CLASSIC;
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
    fn spans_copy_colours_and_sources() {
        let mut a = Framebuffer::new();
        a.enable_src();
        for x in 0..4 {
            a.set_src(x, 0, 5, tile(1, x as u16, 0));
        }
        let mut b = Framebuffer::new();
        b.enable_src();
        b.blit(0, 1, 4, 1, &[5; 4], None);
        b.copy_span(&a, 0, WIDTH, 4);
        assert_eq!(
            b.src_at(3, 1),
            Some(tile(1, 3, 0)),
            "equal colours, the sources copied"
        );
        assert_eq!(kind(&b, 4, 1), Some(Kind::Classic), "four pixels");
        let plain = Framebuffer::new();
        b.copy_span(&plain, 0, WIDTH, 2);
        assert_eq!(
            (b.get(1, 1), kind(&b, 1, 1)),
            (Some(0), Some(Kind::Classic)),
            "classic"
        );
        assert_eq!(kind(&b, 2, 1), Some(Kind::Tile));
        let mut d = Framebuffer::new();
        d.copy_span(&a, 0, 0, 4);
        assert_eq!(
            (d.get(3, 0), d.src()),
            (Some(5), None),
            "no plane here: the colours"
        );
        // A span reaching past the screen copies nothing.
        let n = WIDTH * HEIGHT;
        d.copy_span(&plain, n - 2, 0, 4);
        d.copy_span(&plain, 0, n - 2, 4);
        assert_eq!(d.get(0, 0), Some(5));
    }

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
        on.set_src(2, 1, 6, tile(4, 0, 0));
        dst.clone_from(&on);
        assert_eq!(dst.src_at(2, 1), Some(tile(4, 0, 0)), "some to some");
        dst.clone_from(&Framebuffer::new());
        assert!(
            dst.src().is_none() && dst.get(1, 1) == Some(0),
            "some to none"
        );
        assert_eq!(on.clone().src(), on.src());
    }

    #[test]
    fn portrait_ids_pack_the_letter_over_the_item() {
        let id = portrait_id(b'b', 0x9c).unwrap();
        assert_eq!(id, 0x629c);
        assert_eq!(portrait_of(id), (b'b', 0x9c));
        assert_eq!(portrait_id(b'z', 256), None);
    }
}

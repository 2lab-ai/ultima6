//! Indexed 8-bit framebuffer for the Ultima VI port.
//!
//! The engine draws palette indices into a [`Framebuffer`]; platforms convert the
//! result to RGBA8 with [`Framebuffer::write_rgba`]. No I/O, no time, no platform code.

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

/// A 320x200 grid of palette indices plus the 256-entry palette they refer to.
#[derive(Clone)]
pub struct Framebuffer {
    pixels: Vec<u8>,
    pub palette: [Rgb; 256],
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

    pub fn clear(&mut self, index: u8) {
        self.pixels.fill(index);
    }

    /// Palette index at (x, y), or `None` outside the screen.
    pub fn get(&self, x: usize, y: usize) -> Option<u8> {
        (x < WIDTH && y < HEIGHT).then(|| self.pixels[y * WIDTH + x])
    }

    /// Sets one pixel; out-of-range coordinates are ignored.
    pub fn set(&mut self, x: usize, y: usize, index: u8) {
        if x < WIDTH && y < HEIGHT {
            self.pixels[y * WIDTH + x] = index;
        }
    }

    /// Copies a `w`x`h` row-major indexed sprite to (x, y), clipped to the screen.
    /// Pixels equal to `transparent` are skipped.
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
                    self.pixels[dy as usize * WIDTH + dx as usize] = index;
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
}

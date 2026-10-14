//! Bitmaps `paper.bmp`, `newmagic.bmp`, `worldmap.bmp` — formats.md §4.6 (N: files/u6_bmp.cpp:41-62;
//! worldmap.bmp has no Nuvie reader). Whole-file U6-LZW → `u16 LE width`, `u16 LE height`, then
//! `width × height` u8 palette indices, row-major; decoded size = 4 + w·h.

use crate::bytes;
use crate::error::Result;
use crate::lzw;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// An indexed bitmap.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Bitmap {
    pub width: u16,
    pub height: u16,
    /// `width × height` palette indices, row-major.
    pub pixels: Vec<u8>,
}

impl Bitmap {
    /// Decodes a `.bmp` file (LZW) and parses it.
    pub fn from_lzw(file: &[u8]) -> Result<Self> {
        Self::parse(&lzw::decode(file)?)
    }

    /// Parses an LZW-decoded bitmap. Fails unless the size is exactly `4 + w·h`.
    pub fn parse(decoded: &[u8]) -> Result<Self> {
        let width = bytes::u16_le(decoded, 0, "bitmap width")?;
        let height = bytes::u16_le(decoded, 2, "bitmap height")?;
        let n = usize::from(width) * usize::from(height);
        bytes::expect_len(decoded, 4 + n, "bitmap (4 + width * height)")?;
        Ok(Bitmap {
            width,
            height,
            pixels: decoded[4..].to_vec(),
        })
    }

    /// Palette index at (x, y), or `None` out of range.
    pub fn pixel(&self, x: usize, y: usize) -> Option<u8> {
        (x < usize::from(self.width) && y < usize::from(self.height))
            .then(|| self.pixels[y * usize::from(self.width) + x])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_le_header() {
        let mut d = vec![3, 0, 2, 0];
        d.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        let b = Bitmap::from_lzw(&lzw::encode(&d).unwrap()).unwrap();
        assert_eq!((b.width, b.height), (3, 2));
        assert_eq!(b.pixel(2, 1), Some(6));
        assert_eq!(b.pixel(3, 0), None);
        assert!(Bitmap::parse(&d[..9]).is_err());
        assert!(Bitmap::parse(&[1, 0]).is_err());
    }
}

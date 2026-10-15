//! Fonts — formats.md §4.5.
//!
//! - `u6.ch` (N: fonts/font_manager.cpp:76-97, u6_font.cpp:62-70): 256 glyphs × 8 bytes, one
//!   byte per row, bit 7 = leftmost pixel. Glyphs 0-127 English, 128-255 the runic and the
//!   gargoyles' script.
//! - `u6.set` (N: script/script_cutscene.cpp:1183, fonts/wou_font.cpp:45-59,105-118): U6-LZW →
//!   `[0]` height, `[2]` ink value, `[4+c]` width, `[0x104+c]` offset lo, `[0x204+c]` offset hi;
//!   glyph = width×height bytes at `hi*256+lo`, a byte equal to ink = set pixel.

use crate::bytes;
use crate::error::{FormatError, Result};
use crate::lzw;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// File size of `u6.ch`.
pub const U6CH_LEN: usize = 2048;
/// Glyphs per font.
pub const GLYPHS: usize = 256;

/// The 8×8 1-bpp font from `u6.ch`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BitFont {
    /// 256 glyphs, 8 row bytes each (bit 7 = leftmost).
    pub glyphs: Vec<[u8; 8]>,
}

impl BitFont {
    /// Parses `u6.ch`. Fails unless the file is exactly 2048 bytes.
    pub fn parse(data: &[u8]) -> Result<Self> {
        bytes::expect_len(data, U6CH_LEN, "u6.ch")?;
        let glyphs = data
            .chunks_exact(8)
            .map(|g| std::array::from_fn(|i| g[i]))
            .collect();
        Ok(BitFont { glyphs })
    }

    /// True when pixel (x, y) of glyph `c` is set; false out of range.
    pub fn pixel(&self, c: u8, x: usize, y: usize) -> bool {
        x < 8 && y < 8 && self.glyphs[usize::from(c)][y] & (0x80 >> x) != 0
    }
}

/// One glyph of a WOU font.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct WouGlyph {
    pub width: u8,
    /// Byte offset of the pixels in the decoded font.
    pub offset: u16,
    /// `width × height` bytes, row-major; `== ink` = set pixel.
    pub pixels: Vec<u8>,
}

/// The proportional font from `u6.set`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct WouFont {
    /// Decoded byte 0.
    pub height: u8,
    /// Decoded byte 2: pixel value that means "set".
    pub ink: u8,
    pub glyphs: Vec<WouGlyph>,
    /// The whole decoded font (bytes 1 and 3 are not named by formats.md).
    pub raw: Vec<u8>,
}

impl WouFont {
    /// Decodes `u6.set` (LZW) and parses it.
    pub fn from_lzw(file: &[u8]) -> Result<Self> {
        Self::parse(&lzw::decode(file)?)
    }

    /// Parses an LZW-decoded WOU font.
    pub fn parse(decoded: &[u8]) -> Result<Self> {
        bytes::slice(decoded, 0, 0x304, "wou font header")?;
        let height = decoded[0];
        let ink = decoded[2];
        let mut glyphs = Vec::with_capacity(GLYPHS);
        for c in 0..GLYPHS {
            let width = decoded[4 + c];
            let offset = u16::from_le_bytes([decoded[0x104 + c], decoded[0x204 + c]]);
            let n = usize::from(width) * usize::from(height);
            let pixels = bytes::slice(decoded, usize::from(offset), n, "wou glyph pixels")
                .map_err(|e| FormatError::in_item("wou font glyph", c, 0x104 + c, e))?
                .to_vec();
            glyphs.push(WouGlyph {
                width,
                offset,
                pixels,
            });
        }
        Ok(WouFont {
            height,
            ink,
            glyphs,
            raw: decoded.to_vec(),
        })
    }

    /// True when pixel (x, y) of glyph `c` equals the ink value.
    pub fn pixel(&self, c: u8, x: usize, y: usize) -> bool {
        let g = &self.glyphs[usize::from(c)];
        x < usize::from(g.width)
            && y < usize::from(self.height)
            && g.pixels[y * usize::from(g.width) + x] == self.ink
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_font_bit7_is_leftmost() {
        let mut d = vec![0u8; U6CH_LEN];
        // A synthetic glyph for 'A': row 0 lit from x = 3, row 6 from x = 0.
        d[0x41 * 8..0x42 * 8].copy_from_slice(&[0x18, 0x3c, 0x66, 0x66, 0x7e, 0x66, 0xc3, 0x00]);
        let f = BitFont::parse(&d).unwrap();
        assert_eq!(f.glyphs.len(), 256);
        assert!(!f.pixel(b'A', 0, 0) && f.pixel(b'A', 3, 0) && f.pixel(b'A', 0, 6));
        assert!(!f.pixel(b'A', 8, 0));
        assert!(BitFont::parse(&d[..8]).is_err());
    }

    #[test]
    fn wou_font_glyphs() {
        let mut d = vec![0u8; 0x304];
        d[0] = 2; // height
        d[2] = 15; // ink
        d[4 + 65] = 3; // 'A' width 3 at 0x304
        d[0x104 + 65] = 0x04;
        d[0x204 + 65] = 0x03;
        d.extend_from_slice(&[15, 0, 15, 0, 15, 0]);
        let f = WouFont::from_lzw(&lzw::encode(&d).unwrap()).unwrap();
        assert_eq!((f.height, f.ink), (2, 15));
        let a = &f.glyphs[65];
        assert_eq!((a.width, a.offset, a.pixels.len()), (3, 0x304, 6));
        assert!(f.pixel(b'A', 0, 0) && !f.pixel(b'A', 1, 0) && f.pixel(b'A', 1, 1));
        assert!(f.glyphs[66].pixels.is_empty());
        let mut bad = d.clone();
        bad[4 + 66] = 9; // 'B': 9 x 2 = 18 bytes at 0x300 runs past the end (0x30a)
        bad[0x204 + 66] = 0x03;
        assert!(matches!(
            WouFont::parse(&bad),
            Err(FormatError::InItem { index: 66, .. })
        ));
        assert!(WouFont::parse(&d[..0x200]).is_err());
    }
}

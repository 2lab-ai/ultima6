//! `portrait.a` / `.b` / `.z` — formats.md §2 "Portrait" (N: portraits/portrait_u6.cpp:55-69,
//! 148-151). U6Lib with u32 LE offsets; each item is one U6-LZW stream → 56×64 = 3584 bytes,
//! 1 byte per pixel, row-major. Pixel values are 256-colour (VGA) palette indices.

use crate::error::{FormatError, Result};
use crate::lzw;
use crate::u6lib::{EntryWidth, U6Lib};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

pub const PORTRAIT_WIDTH: usize = 56;
pub const PORTRAIT_HEIGHT: usize = 64;
/// Decoded bytes per portrait.
pub const PORTRAIT_LEN: usize = PORTRAIT_WIDTH * PORTRAIT_HEIGHT;

/// One 56×64 indexed image.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Portrait {
    /// `PORTRAIT_LEN` palette indices, row-major.
    pub pixels: Vec<u8>,
}

impl Portrait {
    /// Palette index at (x, y), or `None` out of range.
    pub fn pixel(&self, x: usize, y: usize) -> Option<u8> {
        (x < PORTRAIT_WIDTH && y < PORTRAIT_HEIGHT).then(|| self.pixels[y * PORTRAIT_WIDTH + x])
    }
}

/// A portrait library: `images[i]` is item `i` (`None` = empty item).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Portraits {
    pub images: Vec<Option<Portrait>>,
}

impl Portraits {
    /// Splits a portrait library and decodes every image.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let lib = U6Lib::parse(data, EntryWidth::U32)?;
        let mut images = Vec::with_capacity(lib.items.len());
        for (i, it) in lib.items.iter().enumerate() {
            if it.is_empty() {
                images.push(None);
                continue;
            }
            let wrap = |e| FormatError::in_item("portrait", i, it.offset as usize, e);
            let pixels = lzw::decode(&it.data).map_err(wrap)?;
            if pixels.len() != PORTRAIT_LEN {
                return Err(wrap(FormatError::bad_length(
                    "portrait image",
                    PORTRAIT_LEN.to_string(),
                    pixels.len(),
                )));
            }
            images.push(Some(Portrait { pixels }));
        }
        Ok(Portraits { images })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_images() {
        let img: Vec<u8> = (0..PORTRAIT_LEN).map(|i| (i % 240) as u8).collect();
        let item = lzw::encode(&img).unwrap();
        let mut d = 8u32.to_le_bytes().to_vec();
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&item);
        let p = Portraits::parse(&d).unwrap();
        assert_eq!(p.images.len(), 2);
        let first = p.images[0].as_ref().unwrap();
        assert_eq!(first.pixels, img);
        assert_eq!(first.pixel(1, 1), Some(57));
        assert_eq!(first.pixel(56, 0), None);
        assert!(p.images[1].is_none());
    }

    #[test]
    fn rejects_wrong_image_size() {
        let mut d = 4u32.to_le_bytes().to_vec();
        d.extend_from_slice(&lzw::encode(&[1, 2, 3]).unwrap());
        assert!(matches!(
            Portraits::parse(&d),
            Err(FormatError::InItem { index: 0, .. })
        ));
    }
}

//! `tileflag` — formats.md §4.2 (N: core/tile_manager.cpp:404-449, obj_manager.cpp:139-141).
//! 7168 bytes of u8 arrays:
//! `[0x0000,0x0800)` flags1[tile], `[0x0800,0x1000)` flags2[tile], `[0x1000,0x1400)`
//! weight[obj_n], `[0x1400,0x1C00)` flags3[tile].

use crate::bytes;
use crate::error::Result;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// File size of `tileflag`.
pub const TILEFLAG_LEN: usize = 7168;
/// Number of tiles covered by the per-tile arrays.
pub const TILE_COUNT: usize = 2048;
/// Number of object types covered by `weight`.
pub const OBJ_COUNT: usize = 1024;

/// Article stored in flags3 bits 6-7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Article {
    None,
    A,
    An,
    The,
}

/// The four tileflag arrays, kept whole so that unnamed bits survive.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TileFlags {
    /// Per tile: bit0 water, bit1 impassable, bit3 damaging.
    pub flags1: Vec<u8>,
    /// Per tile: bit2|bit3 boundary, bit4 top tile, bit6 double height, bit7 double width.
    pub flags2: Vec<u8>,
    /// Per object type (obj_n): weight.
    pub weight: Vec<u8>,
    /// Per tile: bits 6-7 = article (none/a/an/the).
    pub flags3: Vec<u8>,
}

impl TileFlags {
    /// Parses `tileflag`. Fails unless the file is exactly 7168 bytes.
    pub fn parse(data: &[u8]) -> Result<Self> {
        bytes::expect_len(data, TILEFLAG_LEN, "tileflag")?;
        Ok(TileFlags {
            flags1: data[0x0000..0x0800].to_vec(),
            flags2: data[0x0800..0x1000].to_vec(),
            weight: data[0x1000..0x1400].to_vec(),
            flags3: data[0x1400..0x1C00].to_vec(),
        })
    }

    fn f1(&self, tile: usize, bit: u8) -> bool {
        self.flags1.get(tile).is_some_and(|f| f & bit != 0)
    }

    fn f2(&self, tile: usize, bits: u8) -> bool {
        self.flags2.get(tile).is_some_and(|f| f & bits != 0)
    }

    pub fn is_water(&self, tile: usize) -> bool {
        self.f1(tile, 0x01)
    }

    pub fn is_impassable(&self, tile: usize) -> bool {
        self.f1(tile, 0x02)
    }

    pub fn is_damaging(&self, tile: usize) -> bool {
        self.f1(tile, 0x08)
    }

    pub fn is_boundary(&self, tile: usize) -> bool {
        self.f2(tile, 0x0C)
    }

    pub fn is_top(&self, tile: usize) -> bool {
        self.f2(tile, 0x10)
    }

    pub fn is_double_height(&self, tile: usize) -> bool {
        self.f2(tile, 0x40)
    }

    pub fn is_double_width(&self, tile: usize) -> bool {
        self.f2(tile, 0x80)
    }

    /// Article for `tile` (flags3 bits 6-7); `None` for an out-of-range tile.
    pub fn article(&self, tile: usize) -> Option<Article> {
        let f = self.flags3.get(tile)?;
        Some([Article::None, Article::A, Article::An, Article::The][usize::from(f >> 6)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_arrays_and_decodes_bits() {
        let mut d = vec![0u8; TILEFLAG_LEN];
        d[5] = 0x0B; // water, impassable, damaging
        d[0x0800 + 5] = 0xD4; // boundary(bit2), top, double height, double width
        d[0x1000 + 1023] = 42;
        d[0x1400 + 5] = 0xC0; // "the"
        d[0x1400 + 6] = 0x80; // "an"
        let t = TileFlags::parse(&d).unwrap();
        assert_eq!(
            (
                t.flags1.len(),
                t.flags2.len(),
                t.weight.len(),
                t.flags3.len()
            ),
            (2048, 2048, 1024, 2048)
        );
        assert!(t.is_water(5) && t.is_impassable(5) && t.is_damaging(5));
        assert!(!t.is_water(4) && !t.is_water(4096));
        assert!(t.is_boundary(5) && t.is_top(5) && t.is_double_height(5) && t.is_double_width(5));
        assert_eq!(t.weight[1023], 42);
        assert_eq!(t.article(5), Some(Article::The));
        assert_eq!(t.article(6), Some(Article::An));
        assert_eq!(t.article(7), Some(Article::None));
        assert_eq!(t.article(2048), None);
        assert!(TileFlags::parse(&d[..7167]).is_err());
    }
}

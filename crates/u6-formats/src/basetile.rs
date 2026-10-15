//! `basetile` — formats.md §4.2 (N: core/obj_manager.cpp:117-125). `u16 BE[1024]`, obj_n → base
//! tile; `tile = basetile[obj_n] + frame` (obj_manager.cpp:417,1302-1304). Amiga is BE (DOS LE).

use crate::bytes;
use crate::error::Result;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Number of object types.
pub const BASETILE_COUNT: usize = 1024;
/// File size of `basetile`.
pub const BASETILE_LEN: usize = BASETILE_COUNT * 2;

/// Base tile per object type.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BaseTiles {
    pub base: Vec<u16>,
}

impl BaseTiles {
    /// Parses `basetile`. Fails unless the file is exactly 2048 bytes.
    pub fn parse(data: &[u8]) -> Result<Self> {
        bytes::expect_len(data, BASETILE_LEN, "basetile")?;
        let base = data
            .chunks_exact(2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .collect();
        Ok(BaseTiles { base })
    }

    /// Tile for `obj_n` shown at `frame`, or `None` for an out-of-range obj_n.
    pub fn tile(&self, obj_n: u16, frame: u8) -> Option<u16> {
        let b = *self.base.get(usize::from(obj_n))?;
        Some(b.wrapping_add(u16::from(frame)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_big_endian() {
        // Made-up words `00 00 01 10 01 11 03 05` -> 0, 272, 273, 773.
        let mut d = vec![0u8; BASETILE_LEN];
        d[..8].copy_from_slice(&[0x00, 0x00, 0x01, 0x10, 0x01, 0x11, 0x03, 0x05]);
        let b = BaseTiles::parse(&d).unwrap();
        assert_eq!(&b.base[..4], &[0, 272, 273, 773]);
        assert_eq!(b.tile(2, 3), Some(276));
        assert_eq!(b.tile(1024, 0), None);
        assert!(BaseTiles::parse(&d[..2046]).is_err());
    }
}

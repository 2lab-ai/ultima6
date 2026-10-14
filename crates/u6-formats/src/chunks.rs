//! `chunks` — formats.md §4.1 (N: core/map.cpp:588-599). 65536 bytes = 1024 chunks × 64 bytes;
//! chunk `c` = `chunks[c*64..+64]`, 8 rows × 8 u8 map-tile numbers, row-major.

use crate::bytes;
use crate::error::Result;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Number of chunks.
pub const CHUNK_COUNT: usize = 1024;
/// Chunk side in tiles.
pub const CHUNK_SIZE: usize = 8;
/// File size of `chunks`.
pub const CHUNKS_LEN: usize = CHUNK_COUNT * CHUNK_SIZE * CHUNK_SIZE;

/// 8 × 8 map-tile numbers, `tiles[row][col]`.
pub type Chunk = [[u8; CHUNK_SIZE]; CHUNK_SIZE];

/// The 1024 map chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Chunks {
    pub chunks: Vec<Chunk>,
}

impl Chunks {
    /// Parses `chunks`. Fails unless the file is exactly 65536 bytes.
    pub fn parse(data: &[u8]) -> Result<Self> {
        bytes::expect_len(data, CHUNKS_LEN, "chunks")?;
        let chunks = data
            .chunks_exact(CHUNK_SIZE * CHUNK_SIZE)
            .map(|c| std::array::from_fn(|r| std::array::from_fn(|x| c[r * CHUNK_SIZE + x])))
            .collect();
        Ok(Chunks { chunks })
    }

    /// Map-tile number at (x, y) inside chunk `c`, or `None` out of range.
    pub fn tile(&self, c: usize, x: usize, y: usize) -> Option<u8> {
        self.chunks.get(c)?.get(y)?.get(x).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{MAP_LEN, WorldMap};

    #[test]
    fn chunks_are_row_major() {
        let data: Vec<u8> = (0..CHUNKS_LEN).map(|i| (i % 251) as u8).collect();
        let c = Chunks::parse(&data).unwrap();
        assert_eq!(c.chunks.len(), CHUNK_COUNT);
        assert_eq!(c.tile(0, 1, 0), Some(1));
        assert_eq!(c.tile(0, 0, 1), Some(8));
        assert_eq!(c.tile(3, 7, 7), Some(((3 * 64 + 63) % 251) as u8));
        assert_eq!(c.tile(1024, 0, 0), None);
        assert!(Chunks::parse(&data[1..]).is_err());
    }

    #[test]
    fn map_unpacks_triplets_and_layout() {
        let mut d = vec![0u8; MAP_LEN];
        // Superchunk 9 (sx 1, sy 1), row 2, triplet 3: c1 = 0x3AB, c2 = 0x1C5.
        // b0 = 0xAB, b1 = (c2 & 0xF) << 4 | c1 >> 8 = 0x53, b2 = c2 >> 4 = 0x1C.
        let at = 9 * 384 + 2 * 24 + 3 * 3;
        d[at..at + 3].copy_from_slice(&[0xAB, 0x53, 0x1C]);
        // Dungeon level 2 (z 3), row 31, last triplet -> refs at cx 30, 31.
        let at = 24576 + 2 * 1536 + 31 * 48 + 15 * 3;
        d[at..at + 3].copy_from_slice(&[0x01, 0x20, 0x00]);
        let m = WorldMap::parse(&d).unwrap();
        let (cy, cx) = (16 + 2, 16 + 6);
        assert_eq!(m.surface[cy * 128 + cx], 0x3AB);
        assert_eq!(m.surface[cy * 128 + cx + 1], 0x1C5);
        assert_eq!(m.chunk_at(cx * 8 + 7, cy * 8, 0), Some(0x3AB));
        assert_eq!(m.dungeons[2][31 * 32 + 30], 1);
        assert_eq!(m.dungeons[2][31 * 32 + 31], 2);
        assert_eq!(m.chunk_at(255, 255, 3), Some(2));
        assert_eq!(m.chunk_at(256, 0, 3), None);
        assert_eq!(m.chunk_at(0, 0, 6), None);
        assert_eq!(m.all_refs().count(), 128 * 128 + 5 * 32 * 32);
    }

    #[test]
    fn map_rejects_bad_input() {
        assert!(WorldMap::parse(&[0u8; 100]).is_err());
        let mut d = vec![0u8; MAP_LEN];
        d[1] = 0x04; // c1 = 0x400 = 1024, out of range
        assert!(matches!(
            WorldMap::parse(&d),
            Err(crate::FormatError::Invalid { offset: 0, .. })
        ));
    }
}

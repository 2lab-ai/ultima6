//! `map` — formats.md §4.1 (N: core/map.cpp:564-616). 32256 bytes = 64 × 384 + 5 × 1536.
//!
//! Surface superchunk `s` (0..64) at `s*384`: `sx = s%8`, `sy = s/8`; 16 rows × 8 triplets, row
//! `i` = chunk row `16·sy + i`. A triplet `(b0,b1,b2)` holds two 12-bit chunk refs:
//! `c1 = b0 | (b1&0xF)<<8` (left), `c2 = b2<<4 | b1>>4` (right). Dungeon level `L` (0..5, z =
//! L+1) at `24576 + L*1536`: 32 rows × 16 triplets. Each ref indexes [`crate::chunks`].

use crate::bytes;
use crate::chunks::{CHUNK_COUNT, CHUNK_SIZE};
use crate::error::{FormatError, Result};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// File size of `map`.
pub const MAP_LEN: usize = 32256;
/// Surface side in chunks (1024 tiles / 8).
pub const SURFACE_CHUNKS: usize = 128;
/// Dungeon side in chunks (256 tiles / 8).
pub const DUNGEON_CHUNKS: usize = 32;
/// Number of dungeon levels (z = 1..=5).
pub const DUNGEON_LEVELS: usize = 5;
const SUPERCHUNK_LEN: usize = 384;
const DUNGEON_BASE: usize = 64 * SUPERCHUNK_LEN;
const DUNGEON_LEN: usize = 1536;

/// Chunk-reference grids for the surface and the 5 dungeon levels.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct WorldMap {
    /// 128 × 128 chunk refs, row-major (`[cy * 128 + cx]`); tile (x, y) is in chunk (x/8, y/8).
    pub surface: Vec<u16>,
    /// 5 levels (index = z − 1), each 32 × 32 chunk refs, row-major.
    pub dungeons: Vec<Vec<u16>>,
}

fn unpack_triplet(b: &[u8]) -> (u16, u16) {
    let [b0, b1, b2] = [b[0], b[1], b[2]].map(u16::from);
    (b0 | (b1 & 0xF) << 8, b2 << 4 | b1 >> 4)
}

fn check_ref(c: u16, offset: usize) -> Result<u16> {
    if usize::from(c) < CHUNK_COUNT {
        Ok(c)
    } else {
        Err(FormatError::invalid(
            "map chunk ref",
            offset,
            format!("chunk {c} >= {CHUNK_COUNT}"),
        ))
    }
}

/// Decodes a row-major run of triplets into `out` (2 refs per triplet).
fn read_refs(data: &[u8], start: usize, triplets: usize, out: &mut Vec<u16>) -> Result<()> {
    for t in 0..triplets {
        let at = start + t * 3;
        let (c1, c2) = unpack_triplet(bytes::slice(data, at, 3, "map triplet")?);
        out.push(check_ref(c1, at)?);
        out.push(check_ref(c2, at)?);
    }
    Ok(())
}

impl WorldMap {
    /// Parses `map`. Fails unless the file is 32256 bytes and every ref is < 1024.
    pub fn parse(data: &[u8]) -> Result<Self> {
        bytes::expect_len(data, MAP_LEN, "map")?;
        let mut surface = vec![0u16; SURFACE_CHUNKS * SURFACE_CHUNKS];
        let mut row = Vec::with_capacity(16);
        for s in 0..64 {
            let (sx, sy) = (s % 8, s / 8);
            for i in 0..16 {
                row.clear();
                read_refs(data, s * SUPERCHUNK_LEN + i * 24, 8, &mut row)?;
                let base = (sy * 16 + i) * SURFACE_CHUNKS + sx * 16;
                surface[base..base + 16].copy_from_slice(&row);
            }
        }
        let mut dungeons = Vec::with_capacity(DUNGEON_LEVELS);
        for level in 0..DUNGEON_LEVELS {
            let mut refs = Vec::with_capacity(DUNGEON_CHUNKS * DUNGEON_CHUNKS);
            read_refs(data, DUNGEON_BASE + level * DUNGEON_LEN, 32 * 16, &mut refs)?;
            dungeons.push(refs);
        }
        Ok(WorldMap { surface, dungeons })
    }

    /// Chunk ref covering tile (x, y) at level z (0 = surface), or `None` out of range.
    pub fn chunk_at(&self, x: usize, y: usize, z: usize) -> Option<u16> {
        let (grid, side) = match z {
            0 => (&self.surface, SURFACE_CHUNKS),
            _ => (self.dungeons.get(z - 1)?, DUNGEON_CHUNKS),
        };
        let (cx, cy) = (x / CHUNK_SIZE, y / CHUNK_SIZE);
        (cx < side && cy < side).then(|| grid[cy * side + cx])
    }

    /// All refs, surface first.
    pub fn all_refs(&self) -> impl Iterator<Item = u16> + '_ {
        self.surface
            .iter()
            .chain(self.dungeons.iter().flatten())
            .copied()
    }
}

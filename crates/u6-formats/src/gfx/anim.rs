//! `animmask.ega` — per-tile overlay animations (graphics.md §6).
//!
//! File = two length-prefixed blocks: `u16 LE n1`, `n1` bytes, `u16 LE n2`, `n2` bytes
//! (`0x00152E50` reads them into the buffers at `0x00152E3C` / `0x00152E40`). A tile whose
//! masktype has bit 2 set carries a `u16 LE` reference after its pixel data. The blitter
//! (`0x00153002` tail) resolves it: a reference below `0xFC00` is `offset - 0x4D00` into block 1,
//! one at or above `0xFC00` is `offset - 0x400` into block 2 (16-bit wrap, then a signed add).
//! The record there is `[first row][row count h]` and 8 frames of `h` big-endian row words. The
//! frame selected by the global counter (`0x00152DCA` stores `D0 & 7` at `0x0015803E`) is OR-ed
//! into **bitplane 3** only, so an animated pixel gains colour bit 3.

use crate::bytes;
use crate::error::{FormatError, Result};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Frames per overlay: the frame counter is masked with 7.
pub const ANIM_FRAMES: usize = 8;
/// The bitplane the overlay is OR-ed into (screen plane offset 24000 = 3 × 8000).
pub const ANIM_PLANE: u8 = 3;

/// The two blocks of `animmask.ega`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AnimMask {
    pub block1: Vec<u8>,
    pub block2: Vec<u8>,
}

/// One resolved overlay: rows `first_row..first_row + rows`, 8 frames of row bitmasks
/// (bit 15 = leftmost pixel).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AnimOverlay {
    pub first_row: u8,
    pub rows: u8,
    /// `frames[f][i]` = plane-3 bits OR-ed into row `first_row + i` at frame `f`.
    pub frames: Vec<Vec<u16>>,
}

impl AnimMask {
    /// Parses `animmask.ega` (PowerPacker-decrunched; not LZW). The two blocks must fill the file.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let n1 = usize::from(bytes::u16_le(data, 0, "animmask block 1 length")?);
        let block1 = bytes::slice(data, 2, n1, "animmask block 1")?.to_vec();
        let n2 = usize::from(bytes::u16_le(data, 2 + n1, "animmask block 2 length")?);
        let block2 = bytes::slice(data, 4 + n1, n2, "animmask block 2")?.to_vec();
        if 4 + n1 + n2 != data.len() {
            return Err(FormatError::bad_length(
                "animmask",
                format!("{} (4 + both blocks)", 4 + n1 + n2),
                data.len(),
            ));
        }
        Ok(AnimMask { block1, block2 })
    }

    /// Resolves a tile's animation reference the way the blitter does.
    pub fn overlay(&self, anim_ref: u16) -> Result<AnimOverlay> {
        let (block, delta, name) = if anim_ref < 0xFC00 {
            (&self.block1, 0x4D00u16, "animmask block 1")
        } else {
            (&self.block2, 0x0400u16, "animmask block 2")
        };
        // `adda.w` sign-extends the 16-bit sum; a negative offset points before the block.
        let signed = anim_ref.wrapping_add(delta) as i16;
        let off = usize::try_from(signed).map_err(|_| {
            FormatError::invalid(
                name,
                0,
                format!("reference {anim_ref:#06x} gives offset {signed}"),
            )
        })?;
        let head = bytes::slice(block, off, 2, name)?;
        let (first_row, rows) = (head[0], head[1]);
        if rows > 0 && usize::from(first_row) + usize::from(rows) > super::TILE_H {
            return Err(FormatError::invalid(
                name,
                off,
                format!("overlay rows {first_row}+{rows} exceed the 16-row tile"),
            ));
        }
        let h = usize::from(rows);
        let data = bytes::slice(block, off + 2, ANIM_FRAMES * 2 * h, name)?;
        let word = |k: usize| u16::from_be_bytes([data[2 * k], data[2 * k + 1]]);
        let frames = (0..ANIM_FRAMES)
            .map(|f| (0..h).map(|i| word(f * h + i)).collect())
            .collect();
        Ok(AnimOverlay {
            first_row,
            rows,
            frames,
        })
    }
}

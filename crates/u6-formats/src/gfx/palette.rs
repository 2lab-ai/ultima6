//! The 32-colour game palette (graphics.md §2).
//!
//! The Amiga data disks hold no palette file. `game` keeps 32 OCS colour words (`0RGB`, 4 bits per
//! channel, big-endian) in its merged data hunk (hunk 33, image address `0x0015812C` =
//! `A4 - 0x35C4`). At start-up `0x0014EF06` passes that table to `0x00162358`, which copies the 32
//! words into the `COLOR00..COLOR31` moves of the copper list in CHIP hunk 28. Nothing else writes
//! the colour registers, so the palette never changes in play. The table is read from the user's
//! own `game` file at run time; this module holds only its location.

use crate::bytes;
use crate::error::{FormatError, Result};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Number of colours: 5 bitplanes (as the game's copper list sets `BPLCON0`).
pub const PALETTE_LEN: usize = 32;
/// File offset of hunk 33's `HUNK_DATA` block header in the decrunched `game`.
pub const GAME_HUNK33_BLOCK: usize = 243_036;
/// Offset of the palette inside hunk 33's data (the block's data starts 8 bytes after its header).
pub const GAME_PALETTE_IN_HUNK: usize = 0x4A3C;
/// File offset of the palette in the decrunched `game`: `243036 + 8 + 0x4A3C`.
pub const GAME_PALETTE_OFFSET: usize = GAME_HUNK33_BLOCK + 8 + GAME_PALETTE_IN_HUNK;

/// `HUNK_HEADER` id (AmigaDOS hunk format) that starts every executable.
const HUNK_HEADER: u32 = 0x3F3;
/// `HUNK_DATA` id (AmigaDOS hunk format); the longword after it is the block's length in
/// longwords, then that many longwords of data.
const HUNK_DATA: u32 = 0x3EA;

/// One colour, 8 bits per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// Expands an OCS colour word `0RGB`: each 4-bit channel `x` becomes `x * 17` (`0xF` → 255).
    /// Fails if the top nibble is set (OCS colour registers have 12 bits).
    pub fn from_ocs(word: u16) -> Option<Self> {
        if word > 0x0FFF {
            return None;
        }
        let ch = |shift: u16| ((word >> shift) & 0xF) as u8 * 17;
        Some(Rgb {
            r: ch(8),
            g: ch(4),
            b: ch(0),
        })
    }
}

/// Parses 32 big-endian OCS colour words (64 bytes).
pub fn palette_from_ocs_words(words: &[u8]) -> Result<[Rgb; PALETTE_LEN]> {
    bytes::expect_len(words, PALETTE_LEN * 2, "palette words")?;
    let mut out = [Rgb::default(); PALETTE_LEN];
    for (i, slot) in out.iter_mut().enumerate() {
        let w = bytes::u16_be(words, i * 2, "palette word")?;
        *slot = Rgb::from_ocs(w).ok_or_else(|| {
            FormatError::invalid(
                "palette word",
                i * 2,
                format!("{w:#06x} has bits above 0x0FFF"),
            )
        })?;
    }
    Ok(out)
}

/// Reads the game palette from the decrunched `game` executable (`d1/INSTALL/game`, after
/// PowerPacker; the disk copy is PP20-crunched). Checks the hunk-file id, then hunk 33's block
/// header at its offset: the `HUNK_DATA` id and a length field whose data lies inside the file
/// and holds the palette at its offset, before it reads.
pub fn palette(game: &[u8]) -> Result<[Rgb; PALETTE_LEN]> {
    if bytes::u32_be(game, 0, "game hunk header")? != HUNK_HEADER {
        return Err(FormatError::invalid(
            "game hunk header",
            0,
            "not an AmigaDOS executable (HUNK_HEADER 0x3F3)",
        ));
    }
    if bytes::u32_be(game, GAME_HUNK33_BLOCK, "game hunk 33 header")? != HUNK_DATA {
        return Err(FormatError::invalid(
            "game hunk 33 header",
            GAME_HUNK33_BLOCK,
            "expected HUNK_DATA (0x3EA); unknown game version",
        ));
    }
    let longs = bytes::u32_be(game, GAME_HUNK33_BLOCK + 4, "game hunk 33 length")?;
    let data = GAME_HUNK33_BLOCK + 8;
    let end = usize::try_from(longs)
        .ok()
        .and_then(|n| n.checked_mul(4))
        .and_then(|n| n.checked_add(data));
    if end.is_none_or(|e| e > game.len()) {
        return Err(FormatError::invalid(
            "game hunk 33 length",
            GAME_HUNK33_BLOCK + 4,
            "the block's data runs past the end of the file",
        ));
    }
    if (u64::from(longs) * 4) < (GAME_PALETTE_IN_HUNK + PALETTE_LEN * 2) as u64 {
        return Err(FormatError::invalid(
            "game hunk 33 length",
            GAME_HUNK33_BLOCK + 4,
            "the block is too short to hold the palette; unknown game version",
        ));
    }
    let words = bytes::slice(game, GAME_PALETTE_OFFSET, PALETTE_LEN * 2, "game palette")?;
    palette_from_ocs_words(words).map_err(|e| match e {
        FormatError::Invalid {
            what,
            offset,
            detail,
        } => FormatError::Invalid {
            what,
            offset: GAME_PALETTE_OFFSET + offset,
            detail,
        },
        other => other,
    })
}

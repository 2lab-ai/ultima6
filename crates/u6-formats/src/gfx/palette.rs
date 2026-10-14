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

/// Number of colours: 5 bitplanes (copper `BPLCON0 = 0x5200`).
pub const PALETTE_LEN: usize = 32;
/// Size of the PowerPacker-decrunched `game` executable the offsets below are measured on.
pub const GAME_EXE_LEN: usize = 267_332;
/// File offset of hunk 33's `HUNK_DATA` block header in the decrunched `game`.
pub const GAME_HUNK33_BLOCK: usize = 243_036;
/// Offset of the palette inside hunk 33's data (the block's data starts 8 bytes after its header).
pub const GAME_PALETTE_IN_HUNK: usize = 0x4A3C;
/// File offset of the palette in the decrunched `game`: `243036 + 8 + 0x4A3C`.
pub const GAME_PALETTE_OFFSET: usize = GAME_HUNK33_BLOCK + 8 + GAME_PALETTE_IN_HUNK;

/// `HUNK_HEADER` magic that starts every AmigaDOS executable.
const HUNK_HEADER: [u8; 4] = [0x00, 0x00, 0x03, 0xF3];
/// `HUNK_DATA` id, then hunk 33's loaded length in longwords (0x13C3 × 4 = 20236 bytes).
const HUNK33_DATA_BLOCK: [u8; 8] = [0x00, 0x00, 0x03, 0xEA, 0x00, 0x00, 0x13, 0xC3];

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
/// PowerPacker; the disk copy is PP20-crunched). Checks the file length, the hunk-file magic and
/// hunk 33's `HUNK_DATA` header before trusting the fixed offset.
pub fn palette(game: &[u8]) -> Result<[Rgb; PALETTE_LEN]> {
    bytes::expect_len(
        game,
        GAME_EXE_LEN,
        "game executable (PowerPacker-decrunched)",
    )?;
    if bytes::slice(game, 0, 4, "game hunk header")? != HUNK_HEADER {
        return Err(FormatError::invalid(
            "game hunk header",
            0,
            "not an AmigaDOS executable (HUNK_HEADER 0x3F3)",
        ));
    }
    if bytes::slice(game, GAME_HUNK33_BLOCK, 8, "game hunk 33 header")? != HUNK33_DATA_BLOCK {
        return Err(FormatError::invalid(
            "game hunk 33 header",
            GAME_HUNK33_BLOCK,
            "expected HUNK_DATA (0x3EA) of 0x13C3 longwords; unknown game version",
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

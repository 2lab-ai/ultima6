//! Amiga tile graphics and palette — `re/notes/graphics.md` (REA of `game`).
//!
//! The screen is 320×200 lores with 5 bitplanes, 32 colours ([`palette`]). Each of the 2048 tiles
//! is 16×16 and is one of two storage kinds, chosen by `masktype & 3` exactly as the blitter
//! `0x00153002` chooses:
//!
//! - **map tile** (`& 3 == 0`): opaque rows in `maptiles.ega`, all 5 planes, copied over the screen;
//! - **object tile** (`& 3 != 0`): a masked sprite in `objtiles.ega` — an AND mask, then only the
//!   planes that have bits, OR-ed in. `3` = 16 wide, `2` = left 8 columns, `1` = right 8 columns.
//!
//! `masktype & 4` adds an overlay animation from `animmask.ega` ([`AnimMask`]). Pixels the blitter
//! never writes (rows outside a tile's row range, mask bits set) are transparent.

mod anim;
mod palette;
mod tiles;

#[cfg(test)]
mod tests;

pub use anim::{ANIM_FRAMES, ANIM_PLANE, AnimMask, AnimOverlay};
pub use palette::{
    GAME_EXE_LEN, GAME_HUNK33_BLOCK, GAME_PALETTE_IN_HUNK, GAME_PALETTE_OFFSET, PALETTE_LEN, Rgb,
    palette, palette_from_ocs_words,
};
pub use tiles::decode_tiles;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Number of tiles (`tileindx.ega` = 2048 × `u16 LE`).
pub const TILE_COUNT: usize = 2048;
/// Tile width in pixels.
pub const TILE_W: usize = 16;
/// Tile height in pixels.
pub const TILE_H: usize = 16;
/// Pixels per tile.
pub const TILE_PIXELS: usize = TILE_W * TILE_H;
/// Bitplanes (colour bits) per pixel.
pub const PLANES: usize = 5;

/// Columns an object tile covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Span {
    /// masktype & 3 == 3: all 16 columns, 2 bytes per row.
    Full,
    /// masktype & 3 == 2: columns 0..8, 1 byte per row.
    Left,
    /// masktype & 3 == 1: columns 8..16, 1 byte per row (the blitter adds 1 to the screen byte).
    Right,
}

/// Where a tile's pixels are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TileKind {
    /// `maptiles.ega`, opaque copy.
    Map,
    /// `objtiles.ega`, masked sprite.
    Object(Span),
}

/// One decoded 16×16 tile. `pixels` and `transparent` are row-major, 256 entries each.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Tile {
    /// Raw `masktype.ega` value.
    pub masktype: u8,
    pub kind: TileKind,
    /// First row the blitter writes.
    pub first_row: u8,
    /// Number of rows it writes (0 = draws nothing).
    pub rows: u8,
    /// Colour index 0..31 per pixel; 0 where `transparent`.
    pub pixels: Vec<u8>,
    /// True where the blitter leaves the screen unchanged.
    pub transparent: Vec<bool>,
    /// Object tiles only: header byte 2, which no reader in `game` uses (graphics.md §9).
    pub unused_byte: Option<u8>,
    /// `masktype & 4` only: reference into `animmask.ega` ([`AnimMask::overlay`]).
    pub anim_ref: Option<u16>,
}

impl Tile {
    /// The tile as drawn at animation `frame` (taken mod 8, like the game's counter): `pixels`
    /// with the overlay's bits OR-ed in as colour bit 3. Tiles without `anim_ref` are unchanged.
    pub fn frame(&self, anim: &AnimMask, frame: usize) -> crate::Result<Vec<u8>> {
        let mut out = self.pixels.clone();
        let Some(r) = self.anim_ref else {
            return Ok(out);
        };
        let ov = anim.overlay(r)?;
        let rows = &ov.frames[frame % ANIM_FRAMES];
        for (i, bits) in rows.iter().enumerate() {
            let y = usize::from(ov.first_row) + i;
            for x in 0..TILE_W {
                if bits & (0x8000 >> x) != 0 {
                    out[y * TILE_W + x] |= 1 << ANIM_PLANE;
                }
            }
        }
        Ok(out)
    }
}

/// All 2048 tiles, indexed by tile number.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Tileset {
    pub tiles: Vec<Tile>,
}

//! Tile-flag and view-cell bits, named from the code that tests them.
//!
//! `tileflag` arrays (u6-formats [`TileFlags`]): flags1 = image 0x13b140, flags2 = 0x13b940,
//! flags3 = 0x13c140 (loaded by 0x134f08). Each constant cites a routine that tests the bit.

use u6_formats::tileflag::TileFlags;

use super::MapState;

/// flags1 bit 0: water (0x11b1c0: swimmers need it, boats too).
pub const TF1_WATER: u8 = 0x01;
/// flags1 bit 1: blocks walkers (0x11b1c0: a normal walker may enter only without it).
pub const TF1_BLOCKING: u8 = 0x02;
/// flags1 bit 2: wall (0x109e74: wall objects mark the cell 0x08; lights on wall tiles shine
/// only when the cell below is visible).
pub const TF1_WALL: u8 = 0x04;
/// flags1 bit 3: damaging (0x11b1c0 sets A4-0x6a96 when stepping onto it).
pub const TF1_DAMAGING: u8 = 0x08;
/// flags1 low nibble of a wall that 0x109e74 re-tiles by its visible neighbours.
pub const TF1_CONNECTING_WALL: u8 = 0x06;

/// flags2 bits 0-1: light strength of an object tile (0x109e74).
pub const TF2_LIGHT: u8 = 0x03;
/// flags2 bit 2: boundary, stops sight (0x109ab8) and marks object cells 0x20 (0x109e74).
pub const TF2_BOUNDARY: u8 = 0x04;
/// flags2 bit 3: window, see-through only next to the leader (0x109ab8).
pub const TF2_WINDOW: u8 = 0x08;
/// flags2 bit 6: double height, also covers the cell above (0x109e74).
pub const TF2_DOUBLE_HEIGHT: u8 = 0x40;
/// flags2 bit 7: double width, also covers the cell to the left (0x109e74).
pub const TF2_DOUBLE_WIDTH: u8 = 0x80;

/// flags3 bit 5: an object that lets sight through even on a boundary (0x109e74 → cell 0x04).
pub const TF3_SEE_THROUGH: u8 = 0x20;

/// View cell (A4+0x514a) bits 0-1: strongest object light on the cell (0x109e74).
pub const CELL_LIGHT: u8 = 0x03;
/// View cell bit 2: a see-through object stands here (0x109e74, read by 0x109ab8).
pub const CELL_SEE_THROUGH: u8 = 0x04;
/// View cell bit 3: a wall object stands here (0x109e74).
pub const CELL_WALL: u8 = 0x08;
/// View cell bit 4: a window object stands here (0x109e74, read by 0x109ab8).
pub const CELL_WINDOW: u8 = 0x10;
/// View cell bit 5: a boundary object stands here (0x109e74, read by 0x109ab8).
pub const CELL_BOUNDARY: u8 = 0x20;
/// View cell bit 6: queued by the current flood fill (0x109bae, cleared by 0x109e74).
pub const CELL_QUEUED: u8 = 0x40;
/// View cell bit 7: visible from the leader (0x109c12 in sight mode).
pub const CELL_VISIBLE: u8 = 0x80;

/// flags1 of `tile` (0 past the table).
pub fn flags1(t: &TileFlags, tile: usize) -> u8 {
    t.flags1.get(tile).copied().unwrap_or(0)
}

/// flags2 of `tile` (0 past the table).
pub fn flags2(t: &TileFlags, tile: usize) -> u8 {
    t.flags2.get(tile).copied().unwrap_or(0)
}

/// flags3 of `tile` (0 past the table).
pub fn flags3(t: &TileFlags, tile: usize) -> u8 {
    t.flags3.get(tile).copied().unwrap_or(0)
}

/// Movement impedance of a tile: flags1 >> 4 (0x12061e / 0x1206e2 store it + 1 as the path
/// cost of a cell; the same nibble is the wall-connection mask of 0x109e74).
pub fn impedance(t: &TileFlags, tile: usize) -> u8 {
    flags1(t, tile) >> 4
}

impl MapState {
    /// flags1 of the map tile at (x, y, z) (0x11b1c0 reads it through 0x10996c); `None` when
    /// the map is not loaded.
    pub fn tile_flags1_at(&self, t: &TileFlags, x: u16, y: u16, z: u16) -> Option<u8> {
        let tile = self.tile_at(x.into(), y.into(), z.into())?;
        Some(flags1(t, tile.into()))
    }

    /// The map-tile half of 0x11b1c0's test for a normal walker: the tile at (x, y, z) lacks
    /// [`TF1_BLOCKING`]. The object checks of 0x11b1c0 (stacked objects, doors, boats) are
    /// `actors/`'s.
    pub fn is_passable(&self, t: &TileFlags, x: u16, y: u16, z: u16) -> bool {
        self.tile_flags1_at(t, x, y, z)
            .is_some_and(|f| f & TF1_BLOCKING == 0)
    }
}

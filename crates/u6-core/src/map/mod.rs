//! Map: tile lookup, region and window caches, view-window flags, party moves between places
//! (WU-P2).
//!
//! Domain state = [`MapState`]. The port holds the whole `map` and `chunks` files; the original
//! reads them through two file caches (the 2×2-superchunk region buffer of 0x108ac4 and the
//! 64-chunk LRU of 0x1089d8). Nothing outside those functions reads the cache bookkeeping, so the
//! port replaces both with the whole files (`region.rs`). The 40×40 window tile cache (A4+0x4b0a)
//! is real state: other code reads it directly (0x109ab8, 0x12061e, 0x1206e2). Map-window drawing
//! (0x10202e, 0x102c90) belongs to `gfx/`.
//!
//! API for the other units: [`MapState::tile_at`] (0x10996c), [`MapState::chunk_ref`]
//! (0x10983a), [`MapState::rebuild_window`] (0x108d10), [`region::refresh_region`] (0x108f90),
//! [`view::update_view`] (0x109e74), the tile-flag queries in [`flags`], and the party moves in
//! [`travel`]. Calls into other units go through [`seams`].
//!
//! Original functions this module will own (milestone 1):
//!
//! - 0x1089d8 [map] open the map chunk file
//! - 0x108ac4 [map] load map region around the party [likely]
//! - 0x108d10 [map] unnamed; calls 0x1089d8 (open the map chunk file)
//! - 0x108f66 [map] leaf; callers 2, from 2 subsystems
//! - 0x108f90 [map] rebuild the map-window tile cache [likely]
//! - 0x108fba [map] unnamed; calls 0x102ef0 (post-step refresh: rebuild vi~)
//! - 0x10921a [map] unnamed; calls 0x102ef0 (post-step refresh: rebuild vi~)
//! - 0x1092fc [map] unnamed; calls 0x10368a (advance clock: light/status t~)
//! - 0x109374 [map] unnamed; calls 0x10368a (advance clock: light/status t~)
//! - 0x10952e [map] unnamed; calls 0x110912 (pick a song for the current a~)
//! - 0x1095bc [map] unnamed; calls 0x102ef0 (post-step refresh: rebuild vi~)
//! - 0x10983a [map] unnamed; calls 0x108ac4 (load map region around the pa~)
//! - 0x10996c [map] tile at (x,y): window cache, else from chunk
//! - 0x109ab8 [map] leaf; callers 1, from 1 subsystems
//! - 0x109bae [map] leaf; callers 1, from 1 subsystems
//! - 0x109c12 [map] unnamed; 2 callees
//! - 0x109e74 [map] view-window flags (blocking/light/visible) from objects
//! - 0x11d1d4 [map] dungeon/level transition (dungeon names) [likely]
//! - 0x1321dc [map] unnamed; calls 0x101ea8 (empty stub)

pub mod flags;
pub mod region;
pub mod seams;
pub mod tables;
pub mod travel;
pub mod view;

#[cfg(test)]
mod real_data;
#[cfg(test)]
mod tests;

use u6_formats::chunks::Chunks;
use u6_formats::map::WorldMap;
use u6_formats::position::Position;

use crate::geom::{VIEW_H, VIEW_W};

pub use tables::MapTables;

/// Side of the window tile cache and of its flag / light grids: [`crate::geom::WINDOW`] (0x108d10
/// fills 5 × 5 chunks of its 40; Wide's 60 clips the last column and row of chunks).
pub const WINDOW: usize = crate::geom::WINDOW;
/// Cells in the window grids.
pub const WINDOW_CELLS: usize = WINDOW * WINDOW;
/// Coordinate mask on the surface (1024 tiles); 0x108d10.
pub const SURFACE_MASK: u16 = 0x3ff;
/// Coordinate mask on a dungeon level (256 tiles); 0x108d10.
pub const DUNGEON_MASK: u16 = 0xff;

/// Map data, the window caches and the view grids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapState {
    /// Replaces: 0x108ac4 (region buffer A4-0x1df4; the port keeps the whole `map` file)
    pub world: WorldMap,
    /// Replaces: 0x1089d8 (64-chunk LRU at A4-0x2ef4; the port keeps the whole `chunks` file)
    pub chunks: Chunks,
    /// Original: A4-0x7b78 / -0x7b76 / -0x7b74 (words: x, y, z the map window is centred on;
    /// 0x105350 sets them from the party leader's location)
    ///
    /// A duplicate the original keeps of the leader's object location. Sync rule: whoever moves
    /// the leader (player step 0x11d4ee, the party moves in [`travel`]) rewrites it in the same
    /// step, as the original does; until `actors/` ports the step, only the placeholder view
    /// moves it, and the leader's object location stays authoritative
    /// ([`crate::State::leader_position`]).
    pub centre: Position,
    /// Original: A4-0x7958 / -0x7956 / -0x7954 (words: window-cache origin x, y and level of the
    /// last rebuild by 0x108d10; -1 = none, the image value; 0x105350 resets them to -1)
    pub window_key: [i16; 3],
    /// Original: A4-0x784e (word; 0x108d10 clears it while it re-places actors and sets it to 1
    /// after; also set by 0x105350; meaning not identified)
    pub flag_784e: u16,
    /// Original: A4-0x2ef8 / -0x2ef6 (words: world x, y of window cell (0, 0); 0x108d10)
    pub window_origin: [u16; 2],
    /// Original: A4-0x796c (word: [`SURFACE_MASK`] or [`DUNGEON_MASK`]; 0x108d10)
    pub world_mask: u16,
    /// Original: A4-0x7950 / -0x794e (words: window cell of the view's top-left tile; 0x108d10
    /// sets `centre - origin - 5`, the half view per axis in the port
    /// ([`crate::geom::view_offset`]); the image holds 12, 12)
    pub view_offset: [i16; 2],
    /// Original: A4+0x4b0a (40 × 40 map tiles, row-major; 0x108d10; [`WINDOW`] × [`WINDOW`] in
    /// the port). Empty until the first rebuild; [`MapState::tile_at`] then reads the chunks.
    pub window_tiles: Vec<u8>,
    /// Original: A4+0x514a (40 × 40 cell flags; 0x109e74 / 0x109c12); bits in [`flags`]
    /// (`CELL_*`). Persistent: 0x109e74 clears only the inner block, cells 8..32 of each axis
    /// ([`crate::geom::BLOCK_X`] × [`crate::geom::BLOCK_Y`]).
    pub view_flags: Vec<u8>,
    /// Original: A4+0x578a (40 × 40 light levels summed by 0x109c12; same partial clear)
    pub light: Vec<u8>,
    /// Original: A4-0x1476 (11 × 11 words: the tile 0x102c90 draws per view cell, from 0x109e74;
    /// [`view::NOT_VISIBLE`] or [`view::DARK`] where nothing shows). The port's has
    /// [`crate::geom::VIEW_W`] × [`crate::geom::VIEW_H`] entries, row-major: view cell
    /// (col, row) at `row * VIEW_W + col` (spec 13 C1).
    pub view_tiles: Vec<u16>,
    /// Original: A4-0x794c (word; nonzero makes 0x109e74 show every view tile lit; the image
    /// holds 0)
    pub reveal_all: u16,
    /// Original: A4-0x6a8e (word; "view changed", read by 0x102ef0; 0x108d10 sets it to 1)
    pub view_dirty: u16,
    /// Original: A4-0x6594 (word; 0x1092fc sets it to 1 around its 0x1321dc call, so every egg
    /// in the new window is offered to 0x1319b2)
    pub arriving: u16,
    /// Original: A4-0x6592 (byte; cleared by 0x1321dc before its egg pass, read by 0x1319b2)
    pub flag_6592: u8,
    /// Original: A4-0x7afb (byte; 0x1321dc skips its egg pass while it is set; written by the
    /// command loop 0x1041d8, restore 0x1052c8, converse 0x114afa and others; meaning not
    /// identified; the image holds 0)
    pub flag_7afb: u8,
    /// Original: A4+0x186e (64 words; 0x1095bc sets them all to -1; other users and meaning
    /// not identified; empty until then)
    pub table_186e: Vec<i16>,
    /// The tables 0x109c12 / 0x109e74 / 0x11d1d4 read from the data hunk of `game`; `None`
    /// until [`MapState::load_tables`] runs.
    pub tables: Option<MapTables>,
    /// Original: A4-0x309b (byte per animdata slot: 0 off, 1 forward, 2 backward; 0x105350
    /// clears it for slots animating the base tile of object 0x125, 0x120 or 0xec and sets it
    /// to 1 for the others; 0x101ff6 writes any value)
    pub anim_enabled: Vec<u8>,
}

impl Default for MapState {
    fn default() -> Self {
        Self {
            world: WorldMap {
                surface: Vec::new(),
                dungeons: Vec::new(),
            },
            chunks: Chunks { chunks: Vec::new() },
            centre: Position::default(),
            window_key: [-1; 3],
            flag_784e: 0,
            window_origin: [0; 2],
            world_mask: SURFACE_MASK,
            view_offset: [12; 2],
            window_tiles: Vec::new(),
            view_flags: vec![0; WINDOW_CELLS],
            light: vec![0; WINDOW_CELLS],
            view_tiles: vec![0; VIEW_W * VIEW_H],
            reveal_all: 0,
            view_dirty: 0,
            arriving: 0,
            flag_6592: 0,
            flag_7afb: 0,
            table_186e: Vec::new(),
            tables: None,
            anim_enabled: Vec::new(),
        }
    }
}

/// Window-grid index of cell (x, y).
pub const fn cell(x: usize, y: usize) -> usize {
    y * WINDOW + x
}

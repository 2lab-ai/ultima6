//! Map: tile lookup, region load, view-window flags (WU-P2).
//!
//! Domain state = [`MapState`]: the chunk grids and chunks (the port holds both whole files; the
//! original caches the region around the party, 0x108ac4 / 0x1089d8), the view centre, the
//! region-cache keys and the view flags. Map-window drawing (0x10202e, 0x102c90) belongs to
//! `gfx/`.
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

use u6_formats::chunks::Chunks;
use u6_formats::map::WorldMap;
use u6_formats::position::Position;

/// Map data and view bookkeeping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapState {
    /// Original: region chunk cache filled by 0x108ac4 (the port keeps the whole `map` file)
    pub world: WorldMap,
    /// Original: chunk file opened by 0x1089d8 (the port keeps the whole `chunks` file)
    pub chunks: Chunks,
    /// Original: A4-0x7b78 / -0x7b76 / -0x7b74 (words: x, y, z the map window is centred on;
    /// 0x105350 sets them from the party leader's location)
    pub centre: Position,
    /// Original: A4-0x7958 / -0x7956 / -0x7954 (words; 0x105350 sets all three to -1 before the
    /// region load, so they are the key of the loaded region [likely])
    pub loaded_region: [i16; 3],
    /// Original: A4-0x784e (word; 0x105350 sets it to 1 after the region load; meaning not
    /// identified)
    pub flag_784e: u16,
    /// Original: per-window-tile flags computed by 0x109e74 (blocking / light / visible); empty
    /// until `map/` ports it
    pub view_flags: Vec<u8>,
    /// Original: A4-0x309b (byte per animdata slot; 0x105350 clears it for slots animating the
    /// base tile of object 0x125, 0x120 or 0xec and sets it for the others)
    pub anim_enabled: Vec<bool>,
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
            loaded_region: [-1; 3],
            flag_784e: 0,
            view_flags: Vec::new(),
            anim_enabled: Vec::new(),
        }
    }
}

impl MapState {
    /// Map-tile number at (x, y, z), or `None` off the map or before the data is loaded.
    /// Temporary helper for the placeholder view until 0x10996c is ported.
    pub fn tile_at(&self, x: usize, y: usize, z: usize) -> Option<u8> {
        let chunk = self.world.chunk_at(x, y, z)?;
        self.chunks.tile(usize::from(chunk), x % 8, y % 8)
    }
}

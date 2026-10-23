//! Tile lookup and the window tile cache: 0x10983a, 0x10996c, 0x108d10, 0x108f90, 0x1321dc, and
//! the two file caches the port replaces (0x108ac4, 0x1089d8 / 0x108f66).
//!
//! Why the file caches are replaced, not ported: 0x108ac4 keeps a 2×2-superchunk block (or one
//! dungeon level) of the `map` file in A4-0x1df4 and always ends with the requested superchunk
//! inside it; 0x1089d8 keeps 64 chunks of `chunks` in an LRU (keys A4-0x1ef4, ages A4-0x1e74).
//! Their globals (A4-0x7962, -0x7960..-0x795a, -0x7966, -0x796a, -0x1ef4, -0x1e74) are read only
//! by those functions and their jump stubs (grep over every decompiled procedure, 2026-10-14), so
//! a lookup through them returns exactly what the whole file holds. The port keeps both files
//! whole in [`MapState`].

use u6_formats::chunks::CHUNK_SIZE;
use u6_formats::map::{DUNGEON_CHUNKS, SURFACE_CHUNKS};

use super::seams;
use super::{DUNGEON_MASK, MapState, SURFACE_MASK, WINDOW, WINDOW_CELLS, cell};
use crate::geom::{self, HALF_H, HALF_W};
use crate::objects::{ACTOR_SLOTS, ObjectId};
use crate::state::State;

/// Object type of an egg (0x1321dc).
const OBJ_EGG: u16 = 0x14f;
/// Eggs farther than this from the leader on the x / y axis are offered to 0x1319b2 (0x1321dc):
/// the original's 8 is 3 past its half view of 5; the port keeps the 3 past the half view of
/// each axis, so no egg hatches in sight (spec 13 Decision 5).
pub(super) const EGG_NEAR: [i16; 2] = [HALF_W as i16 + 3, HALF_H as i16 + 3];
/// Window cells from the origin to the window's centre on each axis: 0x108d10 drops the
/// temporaries round it (spec 13 Decision 6, `WINDOW / 2`; the original's 20).
const WINDOW_CENTRE: u16 = (WINDOW / 2) as u16;

impl MapState {
    /// Original: 0x10983a (map: chunk ref at x, y, z; loads the region first)
    ///
    /// x and y are masked to 10 bits. On a dungeon level (z ≠ 0) the chunk column and row are
    /// taken modulo 32 (`(x >> 3) & 0x1f`, `(y << 2) & 0x3e0`), so dungeon coordinates wrap at
    /// 256. The region load 0x108ac4 it calls first is replaced (module doc). `None` when the
    /// map is not loaded or z names no level.
    pub fn chunk_ref(&self, x: u16, y: u16, z: u16) -> Option<u16> {
        let (x, y) = (usize::from(x & 0x3ff), usize::from(y & 0x3ff));
        let (cx, cy) = (x / CHUNK_SIZE, y / CHUNK_SIZE);
        if z == 0 {
            self.world.surface.get(cy * SURFACE_CHUNKS + cx).copied()
        } else {
            let level = self.world.dungeons.get(usize::from(z) - 1)?;
            let (cx, cy) = (cx % DUNGEON_CHUNKS, cy % DUNGEON_CHUNKS);
            level.get(cy * DUNGEON_CHUNKS + cx).copied()
        }
    }

    /// Original: 0x10996c (map: tile at x, y, z; window cache, else from chunk)
    ///
    /// x and y are words masked with [`MapState::world_mask`]. Inside the window cache
    /// ([`WINDOW`] × [`WINDOW`]) the tile comes from [`MapState::window_tiles`]; elsewhere from
    /// the chunk 0x10983a names (the chunk LRU 0x1089d8 it reads through is replaced). The window
    /// cache is a copy of the map, so both paths agree; before the first 0x108d10 the port has no
    /// window and reads the chunks. `None` when the map or the chunks are not loaded.
    pub fn tile_at(&self, x: usize, y: usize, z: usize) -> Option<u8> {
        let mask = self.world_mask;
        let (x, y, z) = (x as u16 & mask, y as u16 & mask, z as u16);
        let wx = x.wrapping_sub(self.window_origin[0]) & mask;
        let wy = y.wrapping_sub(self.window_origin[1]) & mask;
        let inside = usize::from(wx) < WINDOW && usize::from(wy) < WINDOW;
        if inside && self.window_tiles.len() == WINDOW_CELLS {
            return Some(self.window_tiles[cell(wx.into(), wy.into())]);
        }
        let chunk = self.chunk_ref(x, y, z)?;
        self.chunks
            .tile(chunk.into(), usize::from(x & 7), usize::from(y & 7))
    }

    /// Replaces: 0x108ac4 (load the map region around (x, y, z) into the region buffer)
    ///
    /// No-op: the port keeps the whole `map` file (module doc). Kept so the callers port in the
    /// original call order (0x105350, 0x108f90).
    pub fn load_region(&mut self, _x: u16, _y: u16, _z: u16) {}

    /// Replaces: 0x108f66 (empty the chunk LRU of 0x1089d8: keys -1, ages 0..64)
    ///
    /// No-op: the port keeps the whole `chunks` file (module doc). Called at boot and after a
    /// restore in the original (jump stubs 0x1088d6, 0x13550a).
    pub fn reset_chunk_cache(&mut self) {}
}

/// Original: 0x108d10 (map: rebuild the 40×40 window tile cache around the view centre)
///
/// Origin = `(centre - 16) & 0x3f8` per axis (chunk-aligned, the centre in cells 16..24); view
/// offset = `(centre - origin - 5) & 0x1f`; the port's are [`geom::window_origin`] and
/// [`geom::view_offset`] (x with [`HALF_W`], y with [`HALF_H`]). Mask 0xff on a dungeon level,
/// else 0x3ff. When origin and level equal the last rebuild's key the cache is kept; otherwise:
/// object grid rebuild (0x10dea4), every actor on the level inside the window that the grid does
/// not hold at its own cell is re-placed there (0x10c9d2, with A4-0x784e cleared around the
/// loop), temporaries away from the window centre go (0x10c49c), the key is stored, the chunks
/// are copied in and the egg pass 0x1321dc runs. Always: moongates (0x10355c), then "view
/// changed".
pub fn rebuild_window(state: &mut State) {
    let centre = state.map.centre;
    let (lx, ly, lz) = (centre.x, centre.y, u16::from(centre.z));
    let m = &mut state.map;
    let (ox, oy) = (geom::window_origin(lx), geom::window_origin(ly));
    m.window_origin = [ox, oy];
    m.view_offset = [
        geom::view_offset(lx, ox, HALF_W as u16),
        geom::view_offset(ly, oy, HALF_H as u16),
    ];
    m.world_mask = if lz != 0 { DUNGEON_MASK } else { SURFACE_MASK };
    let key = [ox as i16, oy as i16, lz as i16];
    if key != m.window_key {
        seams::rebuild_object_grid(state);
        state.map.flag_784e = 0;
        place_window_actors(state, ox, oy, lz);
        state.map.flag_784e = 1;
        let c = WINDOW_CENTRE;
        seams::drop_far_temporaries(state, ox.wrapping_add(c), oy.wrapping_add(c), lz);
        state.map.window_key = key;
        fill_window(&mut state.map, ox, oy, lz);
        check_eggs(state);
    }
    seams::update_moongates(state);
    state.map.view_dirty = 1;
}

/// 0x108d10's actor loop over slots 0..256.
fn place_window_actors(state: &mut State, ox: u16, oy: u16, lz: u16) {
    let mask = state.map.world_mask;
    for a in 0..ACTOR_SLOTS as u16 {
        let id = ObjectId(a);
        let Some(p) = state.objects.location(id) else {
            continue;
        };
        let wx = p.x.wrapping_sub(ox) & mask;
        let wy = p.y.wrapping_sub(oy) & mask;
        let placed = state.objects.type_frame[id.index()] != 0
            && seams::object_status(state, id) & 0x18 == 0
            && u16::from(p.z) == lz
            && usize::from(wx) < WINDOW
            && usize::from(wy) < WINDOW
            && seams::object_grid_cell(state, wx.into(), wy.into()) != a as i16;
        if placed {
            seams::move_object(state, id, p.x, p.y, lz);
        }
    }
}

/// 0x108d10's copy loop: 5 × 5 chunks, x outer, from (origin + offset) & 0x3ff. A window side
/// that is no multiple of the chunk (Wide's 60) takes the last column and row of chunks clipped
/// to it (spec 13 Geometry).
fn fill_window(m: &mut MapState, ox: u16, oy: u16, lz: u16) {
    let mut tiles = vec![0u8; WINDOW_CELLS];
    for cx in (0..WINDOW).step_by(CHUNK_SIZE) {
        for cy in (0..WINDOW).step_by(CHUNK_SIZE) {
            let x = ox.wrapping_add(cx as u16) & 0x3ff;
            let y = oy.wrapping_add(cy as u16) & 0x3ff;
            let chunk = m
                .chunk_ref(x, y, lz)
                .and_then(|c| m.chunks.chunks.get(usize::from(c)));
            let Some(chunk) = chunk else { continue };
            let w = CHUNK_SIZE.min(WINDOW - cx);
            for (r, row) in chunk.iter().take(WINDOW - cy).enumerate() {
                let at = cell(cx, cy + r);
                tiles[at..at + w].copy_from_slice(&row[..w]);
            }
        }
    }
    m.window_tiles = tiles;
}

/// Original: 0x108f90 (map: refresh the region around the view centre)
///
/// Object blocks of the region (0x10e1ce), the map region (0x108ac4, replaced), then the window
/// cache (0x108d10), all for the centre (A4-0x7b78..-0x7b74).
pub fn refresh_region(state: &mut State) {
    let c = state.map.centre;
    let (x, y, z) = (c.x, c.y, u16::from(c.z));
    seams::load_region_objects(state, x, y, z);
    state.map.load_region(x, y, z);
    rebuild_window(state);
}

/// Original: 0x1321dc (map: offer the eggs of the window to 0x1319b2)
///
/// Skipped while A4-0x7afb is set. Clears A4-0x6592, then walks the objects in the whole window
/// rectangle on the centre's level (0x10b05a / 0x10af74): an egg (type 0x14f) on that level is
/// offered when the party is arriving (A4-0x6594), when it is more than 8 tiles from the centre
/// on either axis (`abs`, 0x101ea8; the port's [`EGG_NEAR`] per axis), or when its status has
/// bit 0x20.
pub fn check_eggs(state: &mut State) {
    if state.map.flag_7afb != 0 {
        return;
    }
    state.map.flag_6592 = 0;
    let [ox, oy] = state.map.window_origin;
    let c = state.map.centre;
    let (x0, y0) = (ox as i16, oy as i16);
    let last = WINDOW as i16 - 1;
    let found = seams::objects_in_rect(state, x0, y0, x0 + last, y0 + last, u16::from(c.z));
    for id in found {
        let Some(p) = state.objects.location(id) else {
            continue;
        };
        if state.objects.obj_type(id) != OBJ_EGG || p.z != c.z {
            continue;
        }
        let far = |a: u16, b: u16, near: i16| (a.wrapping_sub(b) as i16).wrapping_abs() > near;
        let offer = state.map.arriving != 0
            || far(p.x, c.x, EGG_NEAR[0])
            || far(p.y, c.y, EGG_NEAR[1])
            || seams::object_status(state, id) & 0x20 != 0;
        if offer {
            seams::hatch_egg(state, id);
        }
    }
}

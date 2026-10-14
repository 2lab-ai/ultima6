//! View window: sight and light flood (0x109ab8, 0x109bae, 0x109c12) and the per-turn view
//! update 0x109e74 that turns the window cache into the 11 × 11 tiles 0x102c90 draws.
//!
//! Grids are the window's 40 × 40 cells ([`super::cell`]). The view is the 11 × 11 block at
//! [`MapState::view_offset`]; the leader stands at view cell (5, 5).

use super::flags::*;
use super::seams;
use super::tables::MapTables;
use super::{MapState, VIEW, WINDOW, WINDOW_CELLS, cell};
use crate::state::State;

/// View tile of a cell nobody sees (0x109e74).
pub const NOT_VISIBLE: u16 = 0xff;
/// View tile of a seen cell without light (0x109e74).
pub const DARK: u16 = 0x1bc;
/// Ambient light from which 0x109e74 skips the object-light pass.
const FULL_DAYLIGHT: i8 = 7;

impl MapState {
    /// Window cell of view cell (col, row), 0..11 each.
    pub fn view_cell(&self, col: usize, row: usize) -> usize {
        let [vx, vy] = self.view_offset;
        cell(vx as usize + col, vy as usize + row)
    }

    /// The leader sees view cell (col, row) ([`CELL_VISIBLE`]).
    pub fn is_visible(&self, col: usize, row: usize) -> bool {
        self.view_flags[self.view_cell(col, row)] & CELL_VISIBLE != 0
    }

    /// Summed light of view cell (col, row).
    pub fn light_at(&self, col: usize, row: usize) -> u8 {
        self.light[self.view_cell(col, row)]
    }

    /// The tile 0x102c90 draws at view cell (col, row).
    pub fn view_tile(&self, col: usize, row: usize) -> u16 {
        self.view_tiles[row * VIEW + col]
    }
}

/// The tileflag arrays as the image lays them out, back to back: flags1 at 0x13b140, flags2 at
/// 0x13b940, flags3 at 0x13c140 (2048 bytes each). An index past one array reads the next, as
/// the original's `flags2[tile - 1]` and object tiles past 2047 do; past flags3 the port reads 0
/// (the original reads whatever follows it).
struct TileBytes<'a>(&'a u6_formats::tileflag::TileFlags);

impl TileBytes<'_> {
    fn at(&self, off: i32) -> u8 {
        let t = self.0;
        let Ok(mut off) = usize::try_from(off) else {
            return 0;
        };
        for a in [&t.flags1, &t.flags2, &t.flags3] {
            match a.get(off) {
                Some(&b) => return b,
                None => off -= a.len(),
            }
        }
        0
    }
    fn f1(&self, tile: i32) -> u8 {
        self.at(tile)
    }
    fn f2(&self, tile: i32) -> u8 {
        self.at(0x800 + tile)
    }
    fn f3(&self, tile: i32) -> u8 {
        self.at(0x1000 + tile)
    }
}

/// What a flood fills in (the third argument word of 0x109c12). The original also has mode 2
/// (queue every unqueued neighbour); no call site passes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FloodMode {
    /// 0: an object light; spreads while `dist <= light + 3`.
    Light = 0,
    /// 1: the leader's sight; marks cells visible, spreads inside the view plus one cell.
    Sight = 1,
}

/// The ring queue 0x109bae / 0x109c12 share: x at A4-0x17f2, y at A4-0x16f2 (256 bytes each),
/// tail A4-0x790a, head A4-0x7909 (bytes). Every flood ends with it empty.
struct Queue {
    x: [u8; 256],
    y: [u8; 256],
    head: u8,
    tail: u8,
}

/// One 0x109e74 pass over the window grids.
struct Flood<'a> {
    tiles: &'a [u8],
    flags: &'a mut [u8],
    light: &'a mut [u8],
    view: [i16; 2],
    t: &'a MapTables,
    tf: TileBytes<'a>,
    queue: Queue,
}

/// Linear window index of (x, y); the original keeps it in A4-0x17f4 next to x and y.
fn lin(x: i16, y: i16) -> i32 {
    i32::from(y) * WINDOW as i32 + i32::from(x)
}

fn in_grid(x: i16, y: i16) -> bool {
    (0..WINDOW as i16).contains(&x) && (0..WINDOW as i16).contains(&y)
}

impl Flood<'_> {
    fn tile(&self, i: i32) -> i32 {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.tiles.get(i))
            .map_or(0, |&t| t.into())
    }
    fn flag(&self, i: i32) -> u8 {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.flags.get(i))
            .copied()
            .unwrap_or(0)
    }

    /// `dist[|dx|][|dy|]`. Floods in modes 0 and 1 stay within 7 cells of their start; past the
    /// table the original would read the bytes after it, the port reads "too far".
    fn dist(&self, dx: i16, dy: i16) -> i16 {
        let (dx, dy) = (
            usize::from(dx.unsigned_abs()),
            usize::from(dy.unsigned_abs()),
        );
        debug_assert!(dx < 8 && dy < 8, "flood left the distance table");
        self.t
            .dist
            .get(dx)
            .and_then(|r| r.get(dy))
            .map_or(i16::MAX, |&d| d.into())
    }

    /// Original: 0x109ab8 (map: does sight / light pass through window cell (x, y)?)
    ///
    /// No through a boundary object; through a window (map tile flags2 0x08 or a window object)
    /// only when the cell is the leader's or a direct neighbour in the leader's row or column
    /// (`abs`, asm 0x109b12); yes through a see-through object; else no through a boundary map
    /// tile, yes otherwise.
    fn passes(&self, x: i16, y: i16) -> bool {
        let i = lin(x, y);
        let (tile, f) = (self.tile(i), self.flag(i));
        if f & CELL_BOUNDARY != 0 {
            return false;
        }
        if self.tf.f2(tile) & TF2_WINDOW != 0 || f & CELL_WINDOW != 0 {
            let (lx, ly) = (self.view[0] + 5, self.view[1] + 5);
            return (x == lx && (ly - y).abs() < 2) || (y == ly && (lx - x).abs() < 2);
        }
        if f & CELL_SEE_THROUGH != 0 {
            return true;
        }
        self.tf.f2(tile) & TF2_BOUNDARY == 0
    }

    /// Original: 0x109bae (map: queue window cell (x, y) for the flood)
    ///
    /// Off-grid cells are ignored. The cell is stored at the tail first; when the tail then
    /// meets the head (queue full) the tail steps back and the cell is dropped, otherwise it is
    /// marked [`CELL_QUEUED`].
    fn enqueue(&mut self, x: i16, y: i16) {
        if !in_grid(x, y) {
            return;
        }
        let q = &mut self.queue;
        q.x[usize::from(q.tail)] = x as u8;
        q.y[usize::from(q.tail)] = y as u8;
        q.tail = q.tail.wrapping_add(1);
        if q.head == q.tail {
            q.tail = q.tail.wrapping_sub(1);
        } else {
            self.flags[cell(x as usize, y as usize)] |= CELL_QUEUED;
        }
    }
}

impl Flood<'_> {
    /// Original: 0x109c12 (map: flood sight or light from window cell (x0, y0))
    ///
    /// Seeds the start cell, or when it blocks, its east and south neighbours (each if it
    /// passes). Then breadth-first: each dequeued cell gains `clamp(4 - dist + light, 0, 4)`
    /// light (u8 wrapping add), is marked visible in sight mode, and when it passes queues its
    /// unqueued neighbours in the walk order of the tables: in sight mode those inside the view
    /// plus one cell, in light mode those with `dist <= light + 3`.
    fn flood(&mut self, x0: i16, y0: i16, light: i16, mode: FloodMode) {
        if self.passes(x0, y0) {
            self.enqueue(x0, y0);
        } else {
            if self.passes(x0 + 1, y0) {
                self.enqueue(x0 + 1, y0);
            }
            if self.passes(x0, y0 + 1) {
                self.enqueue(x0, y0 + 1);
            }
        }
        let [vx, vy] = self.view;
        while self.queue.head != self.queue.tail {
            let h = usize::from(self.queue.head);
            let (x, y) = (i16::from(self.queue.x[h]), i16::from(self.queue.y[h]));
            self.queue.head = self.queue.head.wrapping_add(1);
            let at = cell(x as usize, y as usize);
            let add = (4 - self.dist(x - x0, y - y0) + light).clamp(0, 4);
            self.light[at] = self.light[at].wrapping_add(add as u8);
            if mode == FloodMode::Sight {
                self.flags[at] |= CELL_VISIBLE;
            }
            if !self.passes(x, y) {
                continue;
            }
            let (mut nx, mut ny, mut i) = (x, y, lin(x, y));
            for k in 0..8 {
                nx += i16::from(self.t.walk_dx[k]);
                ny += i16::from(self.t.walk_dy[k]);
                i += i32::from(self.t.walk_step[k]);
                // Off the grid the original tests a byte of a neighbouring array, but the queue
                // rejects the cell whatever it holds.
                if !in_grid(nx, ny) || self.flag(i) & CELL_QUEUED != 0 {
                    continue;
                }
                let go = match mode {
                    FloodMode::Sight => {
                        (vx - 1..=vx + 11).contains(&nx) && (vy - 1..=vy + 11).contains(&ny)
                    }
                    FloodMode::Light => self.dist(nx - x0, ny - y0) <= light + 3,
                };
                if go {
                    self.enqueue(nx, ny);
                }
            }
        }
    }
}

/// Original: 0x109e74 (map: view-window flags, light and the 11×11 view tiles)
///
/// With A4-0x794c set: the view tiles are the window tiles, the inner 24 × 24 light cells 4.
/// Otherwise: the inner 24 × 24 flag and light cells are cleared (the 8-cell border ring keeps
/// its bytes); every object in that block on the leader's level (0x10b05a / 0x10af74) marks its
/// cell from its tile's flags (window, boundary or see-through; boundary and wall spread to the
/// cell above / left for double-size tiles; strongest light in bits 0-1); the leader's sight is
/// flooded with the ambient light (A4-0x7b79); below daylight (7) every lit visible cell floods
/// its light; then each view cell becomes not-visible, dark, its tile, or a re-tiled wall piece.
/// Ends with the view object lists (0x10e81e). The object pass sees what the
/// `objects_in_rect` seam returns (see `seams.rs`).
///
/// Port fallback when [`MapState::tables`] is not loaded: the A4-0x794c path.
pub fn update_view(state: &mut State) {
    let tables = state.map.tables.clone();
    let (Some(t), 0) = (tables, state.map.reveal_all) else {
        reveal_view(&mut state.map);
        seams::build_view_objects(state);
        return;
    };
    let m = &mut state.map;
    for y in 8..32 {
        let row = cell(8, y)..cell(32, y);
        m.view_flags[row.clone()].fill(0);
        m.light[row].fill(0);
    }
    mark_objects(state);
    let ambient = seams::ambient_light(state);
    let m = &mut state.map;
    if m.window_tiles.len() != WINDOW_CELLS {
        m.window_tiles = vec![0; WINDOW_CELLS];
    }
    let mut f = Flood {
        tiles: &m.window_tiles,
        flags: &mut m.view_flags,
        light: &mut m.light,
        view: m.view_offset,
        t: &t,
        tf: TileBytes(&state.tables.tileflags),
        queue: Queue {
            x: [0; 256],
            y: [0; 256],
            head: 0,
            tail: 0,
        },
    };
    let [vx, vy] = f.view;
    f.flood(vx + 5, vy + 5, ambient.into(), FloodMode::Sight);
    if ambient < FULL_DAYLIGHT {
        flood_lights(&mut f);
    }
    let tiles = view_tiles(&f);
    state.map.view_tiles = tiles;
    seams::build_view_objects(state);
}

/// 0x109e74 with A4-0x794c set (asm 0x109e82..0x109ef6).
fn reveal_view(m: &mut MapState) {
    let [vx, vy] = m.view_offset;
    for row in 0..VIEW {
        for col in 0..VIEW {
            let at = cell(vx as usize + col, vy as usize + row);
            m.view_tiles[row * VIEW + col] = m.window_tiles.get(at).map_or(0, |&t| t.into());
        }
    }
    for y in 8..32 {
        m.light[cell(8, y)..cell(32, y)].fill(4);
    }
}

/// 0x109e74's object pass (asm 0x109f3a..0x10a0f4) over the block `(centre - 16) & !7` + 8..31.
fn mark_objects(state: &mut State) {
    let c = state.map.centre;
    let ox = c.x.wrapping_sub(16) & 0xfff8;
    let oy = c.y.wrapping_sub(16) & 0xfff8;
    let (x0, y0) = (ox.wrapping_add(8) as i16, oy.wrapping_add(8) as i16);
    let (x1, y1) = (ox.wrapping_add(31) as i16, oy.wrapping_add(31) as i16);
    let ids = seams::objects_in_rect(state, x0, y0, x1, y1, c.z.into());
    let State {
        map,
        objects,
        tables,
        ..
    } = state;
    let tf = TileBytes(&tables.tileflags);
    let flags = &mut map.view_flags;
    for id in ids {
        let Some(p) = objects.location(id) else {
            continue;
        };
        let (wx, wy) = (p.x.wrapping_sub(ox) as i16, p.y.wrapping_sub(oy) as i16);
        let i = lin(wx, wy);
        let base = tables.basetile.base.get(usize::from(objects.obj_type(id)));
        let tile = base.map_or(0, |&b| b.wrapping_add(objects.frame(id).into()));
        let tile = i32::from(tile as i16);
        let f2 = tf.f2(tile);
        if f2 & TF2_WINDOW != 0 {
            or_cell(flags, i, CELL_WINDOW);
        } else if f2 & TF2_BOUNDARY != 0 {
            or_cell(flags, i, CELL_BOUNDARY);
        } else if tf.f3(tile) & TF3_SEE_THROUGH != 0 {
            or_cell(flags, i, CELL_SEE_THROUGH);
        }
        // Double-size tiles also cover the cell above / to the left.
        let spread = |flags: &mut [u8], bits: u8| {
            if f2 & TF2_DOUBLE_HEIGHT != 0 && wy != 0 {
                or_cell(flags, i - WINDOW as i32, bits);
            }
            if f2 & TF2_DOUBLE_WIDTH != 0 && wx != 0 {
                or_cell(flags, i - 1, bits);
            }
        };
        if tf.f2(tile - 1) & TF2_BOUNDARY != 0 {
            spread(flags, CELL_BOUNDARY);
        }
        if tf.f1(tile) & TF1_WALL != 0 {
            or_cell(flags, i, CELL_WALL);
            spread(flags, CELL_WALL);
        }
        let strength = f2 & TF2_LIGHT;
        if let Some(b) = usize::try_from(i).ok().and_then(|i| flags.get_mut(i))
            && strength > *b & CELL_LIGHT
        {
            *b = (*b & !CELL_LIGHT) | strength;
        }
    }
}

fn or_cell(flags: &mut [u8], i: i32, bits: u8) {
    if let Some(b) = usize::try_from(i).ok().and_then(|i| flags.get_mut(i)) {
        *b |= bits;
    }
}

/// 0x109e74's object-light pass (asm 0x10a122..0x10a1d4): every cell of the inner block with
/// object light that the leader sees floods its light (`strength - 4`), after clearing the
/// queued marks of the 7 × 7 cells around it.
///
/// The original walks the flag and tile grids with pointers (A2, A3) beside the loop counters.
/// A lit wall tile whose south neighbour is not visible `continue`s past the pointer
/// increments (asm 0x10a15e → 0x10a1c4): the rest of that row re-reads the same cell, and the
/// next row starts as many cells early, while the coordinates handed to the flood keep
/// counting. `p` is that pointer, kept apart from (x, y).
fn flood_lights(f: &mut Flood<'_>) {
    let mut p = cell(8, 8);
    for y in 8..32i16 {
        for x in 8..32i16 {
            let c = f.flags[p];
            if c & CELL_LIGHT == 0 || c & CELL_VISIBLE == 0 {
                p += 1;
                continue;
            }
            let wall = f.tf.f1(f.tiles[p].into()) & TF1_WALL != 0;
            if wall && f.flags[p + WINDOW] & CELL_VISIBLE == 0 {
                continue;
            }
            for cx in x - 3..=x + 3 {
                for cy in y - 3..=y + 3 {
                    f.flags[cell(cx as usize, cy as usize)] &= !CELL_QUEUED;
                }
            }
            let light = i16::from(f.flags[p] & CELL_LIGHT) - 4;
            f.flood(x, y, light, FloodMode::Light);
            p += 1;
        }
        p += WINDOW - 24;
    }
}

/// 0x109e74's output loop (asm 0x10a1d8..0x10a3f8): the 11 × 11 view tiles.
fn view_tiles(f: &Flood<'_>) -> Vec<u16> {
    let [vx, vy] = f.view;
    let mut out = vec![0; VIEW * VIEW];
    for row in 0..VIEW as i16 {
        for col in 0..VIEW as i16 {
            out[row as usize * VIEW + col as usize] = view_tile_at(f, lin(vx + col, vy + row));
        }
    }
    out
}

/// One view cell: [`NOT_VISIBLE`], [`DARK`], the window tile, or for a connecting wall
/// (flags1 low nibble 6) with a hidden neighbour a wall piece chosen by the visible-neighbour
/// mask (N 8, E 4, S 2, W 1) and the wall's own connection nibble (flags1 >> 4) through the
/// table at A4-0x78f0. The connection fix-ups test nibble bit 2 but clear bit 3, and test bit 1
/// but clear bit 0 (asm 0x10a2ee / 0x10a328, 0x10a330 / 0x10a368); ported as written.
fn view_tile_at(f: &Flood<'_>, p: i32) -> u16 {
    if f.flag(p) & CELL_VISIBLE == 0 {
        return NOT_VISIBLE;
    }
    let lit = usize::try_from(p)
        .ok()
        .and_then(|i| f.light.get(i))
        .copied();
    if lit.unwrap_or(0) == 0 {
        return DARK;
    }
    let t = f.tile(p);
    let f1 = f.tf.f1(t);
    if f1 & 0x0f != TF1_CONNECTING_WALL {
        return t as u16;
    }
    let base = (t & 0xf0).max(0x90);
    let w = WINDOW as i32;
    let seen = |i: i32, bit: u8| {
        if f.flag(i) & CELL_VISIBLE != 0 {
            bit
        } else {
            0
        }
    };
    let vis = seen(p - w, 8) | seen(p + 1, 4) | seen(p + w, 2) | seen(p - 1, 1);
    if vis == 0x0f {
        return t as u16;
    }
    let mut conn = f1 >> 4;
    let joins = |i: i32, bit: u8| {
        let n = f.tf.f1(f.tile(i));
        f.flag(i) & CELL_WALL != 0 || (n & TF1_WALL != 0 && n & bit != 0)
    };
    if conn & 4 != 0 && !joins(p - w, 0x20) {
        conn &= !8;
    }
    if conn & 2 != 0 && !joins(p - 1, 0x40) {
        conn &= !1;
    }
    if !(conn == 7 || conn == 11 || conn > 12 || conn == vis) {
        return t as u16;
    }
    let k = conn & vis;
    let piece = i32::from(f.t.wall[usize::from(k)]);
    let tile = if k == 3 || k == 12 {
        (base >> 3) + piece + 0xee
    } else {
        piece + base
    };
    tile as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use u6_formats::tileflag::{TILE_COUNT, TileFlags};

    fn flags() -> TileFlags {
        TileFlags {
            flags1: vec![0; TILE_COUNT],
            flags2: vec![0; TILE_COUNT],
            weight: vec![0; 1024],
            flags3: vec![0; TILE_COUNT],
        }
    }

    fn tables() -> MapTables {
        MapTables {
            dist: std::array::from_fn(|a| std::array::from_fn(|b| a.max(b) as i8)),
            walk_dx: [0; 8],
            walk_dy: [0; 8],
            walk_step: [0; 8],
            wall: std::array::from_fn(|k| k as u8),
            gate_x: [0; 25],
            gate_y: [0; 25],
            gate_z: [0; 25],
        }
    }

    struct Grids {
        tiles: Vec<u8>,
        flags: Vec<u8>,
        light: Vec<u8>,
    }

    fn grids() -> Grids {
        Grids {
            tiles: vec![0; WINDOW_CELLS],
            flags: vec![0; WINDOW_CELLS],
            light: vec![0; WINDOW_CELLS],
        }
    }

    fn flood<'a>(g: &'a mut Grids, t: &'a MapTables, tf: &'a TileFlags) -> Flood<'a> {
        Flood {
            tiles: &g.tiles,
            flags: &mut g.flags,
            light: &mut g.light,
            view: [15, 15],
            t,
            tf: TileBytes(tf),
            queue: Queue {
                x: [0; 256],
                y: [0; 256],
                head: 0,
                tail: 0,
            },
        }
    }

    #[test]
    fn windows_pass_only_next_to_the_leader_in_its_row_or_column() {
        let (t, mut tf) = (tables(), flags());
        tf.flags2[3] = TF2_WINDOW;
        tf.flags2[4] = TF2_BOUNDARY;
        let mut g = grids();
        g.tiles.fill(3);
        g.tiles[cell(30, 30)] = 4;
        g.flags[cell(31, 31)] = CELL_SEE_THROUGH;
        g.tiles[cell(31, 31)] = 4;
        g.flags[cell(32, 32)] = CELL_BOUNDARY | CELL_SEE_THROUGH;
        let f = flood(&mut g, &t, &tf);
        // Leader at (20, 20).
        assert!(f.passes(20, 21) && f.passes(19, 20) && f.passes(20, 20));
        assert!(!f.passes(20, 22) && !f.passes(21, 21) && !f.passes(22, 20));
        assert!(!f.passes(30, 30), "boundary tile");
        assert!(f.passes(31, 31), "see-through object over a boundary tile");
        assert!(!f.passes(32, 32), "boundary object wins");
    }

    #[test]
    fn a_full_queue_drops_the_cell_unmarked() {
        let (t, tf) = (tables(), flags());
        let mut g = grids();
        let mut f = flood(&mut g, &t, &tf);
        for i in 0..255i16 {
            f.enqueue(i % 40, i / 40);
        }
        assert_eq!((f.queue.head, f.queue.tail), (0, 255));
        f.enqueue(7, 7);
        assert_eq!(f.queue.tail, 255);
        assert_eq!(f.flags[cell(7, 7)], 0);
        assert_eq!(f.flags[cell(6, 6)], CELL_QUEUED);
        f.enqueue(-1, 3);
        f.enqueue(3, 40);
        assert_eq!(f.queue.tail, 255);
    }

    #[test]
    fn walls_with_hidden_neighbours_are_re_tiled() {
        let (t, mut tf) = (tables(), flags());
        // Wall 0x95: connecting (low nibble 6), connections N E S W (0xf0).
        tf.flags1[0x95] = 0xf6;
        tf.flags1[0x31] = TF1_WALL | 0x20; // north neighbour joins
        let mut g = grids();
        let p = cell(20, 20);
        g.tiles[p] = 0x95;
        g.tiles[p - WINDOW] = 0x31;
        g.light[p] = 1;
        for q in [p, p - WINDOW, p + 1, p - 1] {
            g.flags[q] = CELL_VISIBLE;
        }
        let f = flood(&mut g, &t, &tf);
        // Seen N, E, W (mask 13); west neighbour is no wall, so connection bit 0 drops: 14.
        // 14 & 13 = 12 -> (0x90 >> 3) + wall[12] + 0xee.
        assert_eq!(view_tile_at(&f, p as i32), 0x12 + 12 + 0xee);
        // All four seen: the tile itself.
        f.flags[p + WINDOW] = CELL_VISIBLE;
        assert_eq!(view_tile_at(&f, p as i32), 0x95);
        // Not lit: dark; not seen: nothing.
        f.light[p] = 0;
        assert_eq!(view_tile_at(&f, p as i32), DARK);
        f.flags[p] = 0;
        assert_eq!(view_tile_at(&f, p as i32), NOT_VISIBLE);
    }
}

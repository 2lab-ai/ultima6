//! View window: sight and light flood (0x109ab8, 0x109bae, 0x109c12) and the per-turn view
//! update 0x109e74 that turns the window cache into the 11 × 11 tiles 0x102c90 draws.
//!
//! Grids are the window's 40 × 40 cells ([`super::cell`]; [`WINDOW`] × [`WINDOW`] in the port).
//! The view is the 11 × 11 block at [`MapState::view_offset`] ([`VIEW_W`] × [`VIEW_H`]); the
//! leader stands at view cell (5, 5) ([`HALF_W`], [`HALF_H`]). The original's cells 8..32 of
//! each axis, the inner block 0x109e74 clears and lights, are [`BLOCK_X`] × [`BLOCK_Y`].

use std::ops::Range;

use super::flags::*;
use super::seams;
use super::tables::MapTables;
use super::{MapState, WINDOW, WINDOW_CELLS, cell};
use crate::geom::{self, BLOCK_X, BLOCK_Y, HALF_H, HALF_W, QUEUE_LEN, VIEW_H, VIEW_W};
use crate::state::State;

/// View tile of a cell nobody sees (0x109e74).
pub const NOT_VISIBLE: u16 = 0xff;
/// View tile of a seen cell without light (0x109e74).
pub const DARK: u16 = 0x1bc;
/// Ambient light from which 0x109e74 skips the object-light pass.
const FULL_DAYLIGHT: i8 = 7;
/// The original's half view: the sight flood reads the distance table as if each axis of the
/// view were this wide (spec 13 Decision 8).
const TABLE_HALF: i16 = geom::RULE_HALF;
/// The view's half on x and y: the leader's view cell.
const HALF: [i16; 2] = [HALF_W as i16, HALF_H as i16];

impl MapState {
    /// Window cell of view cell (col, row), col 0..[`VIEW_W`], row 0..[`VIEW_H`].
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
        self.view_tiles[row * VIEW_W + col]
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

/// What a flood fills in (the third argument word of 0x109c12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FloodMode {
    /// 0: an object light; spreads while `dist <= light + 3`.
    Light = 0,
    /// 1: the leader's sight; marks cells visible, spreads inside the view plus one cell.
    Sight = 1,
    /// 2: the map overview's sight (0x13410a); marks cells visible, spreads to every unqueued
    /// neighbour, with no view or distance bound.
    Overview = 2,
}

/// The ring queue 0x109bae / 0x109c12 share: x at A4-0x17f2, y at A4-0x16f2 (256 bytes each),
/// tail A4-0x790a, head A4-0x7909 (bytes). Every flood ends with it empty. The port's has
/// [`QUEUE_LEN`] entries (Wide: 1024, word indices; spec 13 Geometry).
struct Queue {
    x: [u8; QUEUE_LEN],
    y: [u8; QUEUE_LEN],
    head: QueueIndex,
    tail: QueueIndex,
    /// Test-only: what the queue went through in one [`update_view`].
    #[cfg(test)]
    seen: QueueSeen,
}

/// Test-only: the most cells the queue held at once, and the cells it dropped because it was
/// full, over one [`update_view`] (all its floods).
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct QueueSeen {
    pub peak: usize,
    pub dropped: usize,
}

#[cfg(test)]
thread_local! {
    /// The [`QueueSeen`] of this thread's last [`update_view`] that flooded.
    static LAST_SEEN: std::cell::Cell<QueueSeen> =
        const { std::cell::Cell::new(QueueSeen { peak: 0, dropped: 0 }) };
}

/// Test-only: the queue of this thread's last [`update_view`] that flooded.
#[cfg(test)]
pub(super) fn last_queue() -> QueueSeen {
    LAST_SEEN.with(std::cell::Cell::get)
}

/// A queue index: the original's byte.
#[cfg(not(feature = "wide"))]
type QueueIndex = u8;
/// A queue index: a word, to hold Wide's [`QUEUE_LEN`].
#[cfg(feature = "wide")]
type QueueIndex = u16;
/// The last queue index; indices step round the ring masked with it.
const QUEUE_LAST: QueueIndex = (QUEUE_LEN - 1) as QueueIndex;
const _: () = assert!(
    QUEUE_LEN - 1 == QUEUE_LAST as usize,
    "the index holds the queue"
);

impl Queue {
    const fn new() -> Self {
        Queue {
            x: [0; QUEUE_LEN],
            y: [0; QUEUE_LEN],
            head: 0,
            tail: 0,
            #[cfg(test)]
            seen: QueueSeen {
                peak: 0,
                dropped: 0,
            },
        }
    }
}

/// The queue index after `i`, round the ring (the original's byte wraps at 256).
const fn next(i: QueueIndex) -> QueueIndex {
    i.wrapping_add(1) & QUEUE_LAST
}

/// The queue index before `i`, round the ring.
const fn prev(i: QueueIndex) -> QueueIndex {
    i.wrapping_sub(1) & QUEUE_LAST
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

    /// `dist[|dx|][|dy|]`. Floods in modes 0 and 1 stay within 7 cells of their start in the
    /// original; past the table the original would read the bytes after it, the port reads
    /// "too far" (a light flood stops there; spec 13 Design 2). The overview's flood (mode 2)
    /// goes past it, and the original reads byte `|dx| * 8 + |dy|` from A4-0x794a (asm
    /// 0x109cec..0x109d06): with |dy| >= 8 and an index below 64 a later row of the table
    /// itself; from 64 on the bytes after it, the queue indices A4-0x790a / -0x7909, the walk
    /// tables, the wall table at A4-0x78f0 and what follows, up to A4-0x787b (index 207 =
    /// 23 * 8 + 23, the farthest cell from a leader in window cells 16..23). The port adds no
    /// light there; see [`flood_overview`] for why that light is never read.
    fn dist(&self, dx: i16, dy: i16) -> i16 {
        let (dx, dy) = (
            usize::from(dx.unsigned_abs()),
            usize::from(dy.unsigned_abs()),
        );
        self.t
            .dist
            .get(dx)
            .and_then(|r| r.get(dy))
            .map_or(i16::MAX, |&d| d.into())
    }

    /// The distance a flood in `mode` lights cell (dx, dy) from its start by. A light and the
    /// overview keep the cell's own distance; the leader's sight lays the table over the view,
    /// each axis scaled from its half view to the original's 5 (`dx * 5 / HALF_W`,
    /// `dy * 5 / HALF_H`, toward zero), so daylight and dusk reach the view's edges as they
    /// reached the original's (spec 13 Decision 8; the identity where the half view is 5).
    fn flood_dist(&self, dx: i16, dy: i16, mode: FloodMode) -> i16 {
        match mode {
            FloodMode::Light | FloodMode::Overview => self.dist(dx, dy),
            FloodMode::Sight => self.dist(sight_axis(dx, HALF[0]), sight_axis(dy, HALF[1])),
        }
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
            let (lx, ly) = (self.view[0] + HALF[0], self.view[1] + HALF[1]);
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
        q.tail = next(q.tail);
        if q.head == q.tail {
            q.tail = prev(q.tail);
            #[cfg(test)]
            {
                q.seen.dropped += 1;
            }
        } else {
            #[cfg(test)]
            {
                let held = usize::from(q.tail.wrapping_sub(q.head) & QUEUE_LAST);
                q.seen.peak = q.seen.peak.max(held);
            }
            self.flags[cell(x as usize, y as usize)] |= CELL_QUEUED;
        }
    }
}

impl Flood<'_> {
    /// Original: 0x109c12 (map: flood sight or light from window cell (x0, y0))
    ///
    /// Seeds the start cell, or when it blocks, its east and south neighbours (each if it
    /// passes). Then breadth-first: each dequeued cell gains `clamp(4 - dist + light, 0, 4)`
    /// light (u8 wrapping add), is marked visible in any mode but light (asm 0x109d48), and when
    /// it passes queues its unqueued neighbours in the walk order of the tables: in sight mode
    /// those inside the view plus one cell, in the overview's mode all of them (asm
    /// 0x109e0e..0x109e20), in light mode those with `dist <= light + 3`.
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
            self.queue.head = next(self.queue.head);
            let at = cell(x as usize, y as usize);
            let add = (4 - self.flood_dist(x - x0, y - y0, mode) + light).clamp(0, 4);
            self.light[at] = self.light[at].wrapping_add(add as u8);
            if mode != FloodMode::Light {
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
                        (vx - 1..=vx + VIEW_W as i16).contains(&nx)
                            && (vy - 1..=vy + VIEW_H as i16).contains(&ny)
                    }
                    FloodMode::Overview => true,
                    FloodMode::Light => self.dist(nx - x0, ny - y0) <= light + 3,
                };
                if go {
                    self.enqueue(nx, ny);
                }
            }
        }
    }
}

/// One axis of the sight flood's distance (`Flood::flood_dist`): `d * 5 / half`, rounded toward
/// zero.
fn sight_axis(d: i16, half: i16) -> i16 {
    d * TABLE_HALF / half
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
/// `objects_in_rect` seam returns (see `seams.rs`). The port's inner block is [`BLOCK_X`] ×
/// [`BLOCK_Y`], its view [`VIEW_W`] × [`VIEW_H`].
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
    for y in BLOCK_Y {
        let row = block_row(y);
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
        queue: Queue::new(),
    };
    let [vx, vy] = f.view;
    f.flood(vx + HALF[0], vy + HALF[1], ambient.into(), FloodMode::Sight);
    if ambient < FULL_DAYLIGHT {
        flood_lights(&mut f);
    }
    #[cfg(test)]
    LAST_SEEN.with(|c| c.set(f.queue.seen));
    let tiles = view_tiles(&f);
    state.map.view_tiles = tiles;
    seams::build_view_objects(state);
}

/// Window cells of row `y` in the inner block.
fn block_row(y: usize) -> Range<usize> {
    cell(BLOCK_X.start, y)..cell(BLOCK_X.end, y)
}

/// 0x109e74 with A4-0x794c set (asm 0x109e82..0x109ef6).
fn reveal_view(m: &mut MapState) {
    let [vx, vy] = m.view_offset;
    for row in 0..VIEW_H {
        for col in 0..VIEW_W {
            let at = cell(vx as usize + col, vy as usize + row);
            m.view_tiles[row * VIEW_W + col] = m.window_tiles.get(at).map_or(0, |&t| t.into());
        }
    }
    for y in BLOCK_Y {
        m.light[block_row(y)].fill(4);
    }
}

/// The light 0x13410a floods the overview with (asm 0x134160 `moveq #0x28`, pushed at 0x134184).
const OVERVIEW_LIGHT: i16 = 0x28;

/// 0x109c12 in mode 2 with light 0x28, as the map overview 0x13410a calls it on the grids it has
/// cleared (asm 0x134172..0x13418e): from window cell (x0, y0) every cell reached through
/// passing cells is marked seen, its light summed as in the other modes.
///
/// The light differs from the original's only where nothing reads it. Every cell outside the
/// inner block ([`BLOCK_X`] × [`BLOCK_Y`]) is 8 or more from the leader on one axis, past the
/// distance table ([`Flood::dist`]), so that ring gets no light from the overview in the port
/// where the original adds some. Unobservable: the overview reads bit 7 only; after the key
/// [`update_view`] clears the inner block; the view and its one-cell ring lie inside that block
/// in both builds; and the light's readers, [`view_tile_at`] and 0x10e81e's overlay
/// (`objects/draw.rs` `overlay_view`), read view cells only.
///
/// Port fallback when [`MapState::tables`] is not loaded: every cell seen.
pub(crate) fn flood_overview(state: &mut State, x0: i16, y0: i16) {
    let Some(t) = state.map.tables.clone() else {
        state
            .map
            .view_flags
            .iter_mut()
            .for_each(|f| *f |= CELL_VISIBLE);
        return;
    };
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
        queue: Queue::new(),
    };
    f.flood(x0, y0, OVERVIEW_LIGHT, FloodMode::Overview);
}

/// The window origin on one axis as 0x109e74 takes it, `(c - 16) & 0xfff8` with the port's
/// [`geom::LEAD`]. Not the rebuild's `& 0x3f8` ([`geom::window_origin`]): where the window
/// starts west of (north of) 0 the corner stays negative, so the block's rectangle stops at the
/// map edge (the object walk clamps it at 0) instead of covering the far side.
pub(super) fn block_origin(c: u16) -> u16 {
    c.wrapping_sub(geom::LEAD) & 0xfff8
}

/// 0x109e74's object pass (asm 0x109f3a..0x10a0f4) over the block `(centre - 16) & !7` + 8..31
/// ([`block_origin`] + [`BLOCK_X`], [`BLOCK_Y`]).
fn mark_objects(state: &mut State) {
    let c = state.map.centre;
    let (ox, oy) = (block_origin(c.x), block_origin(c.y));
    let at = |o: u16, cells: usize| o.wrapping_add(cells as u16) as i16;
    let (x0, y0) = (at(ox, BLOCK_X.start), at(oy, BLOCK_Y.start));
    let (x1, y1) = (at(ox, BLOCK_X.end - 1), at(oy, BLOCK_Y.end - 1));
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
    let mut p = cell(BLOCK_X.start, BLOCK_Y.start);
    let (xs, ys) = (block_i16(BLOCK_X), block_i16(BLOCK_Y));
    for y in ys {
        for x in xs.clone() {
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
                    // Off the grid only where the block reaches its edge (Wide's x 0..56); the
                    // sight that lights a cell stays a ring inside the window.
                    if in_grid(cx, cy) {
                        f.flags[cell(cx as usize, cy as usize)] &= !CELL_QUEUED;
                    }
                }
            }
            let light = i16::from(f.flags[p] & CELL_LIGHT) - 4;
            f.flood(x, y, light, FloodMode::Light);
            p += 1;
        }
        p += WINDOW - BLOCK_X.len();
    }
}

/// A block range as the light pass's i16 coordinates.
fn block_i16(r: Range<usize>) -> Range<i16> {
    r.start as i16..r.end as i16
}

/// 0x109e74's output loop (asm 0x10a1d8..0x10a3f8): the 11 × 11 view tiles ([`VIEW_W`] ×
/// [`VIEW_H`], row-major; spec 13 C1).
fn view_tiles(f: &Flood<'_>) -> Vec<u16> {
    let [vx, vy] = f.view;
    let mut out = vec![0; VIEW_W * VIEW_H];
    for row in 0..VIEW_H as i16 {
        for col in 0..VIEW_W as i16 {
            out[row as usize * VIEW_W + col as usize] = view_tile_at(f, lin(vx + col, vy + row));
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

    /// A flood over `g` with the leader at window cell (20, 20).
    fn flood<'a>(g: &'a mut Grids, t: &'a MapTables, tf: &'a TileFlags) -> Flood<'a> {
        Flood {
            tiles: &g.tiles,
            flags: &mut g.flags,
            light: &mut g.light,
            view: [20 - HALF[0], 20 - HALF[1]],
            t,
            tf: TileBytes(tf),
            queue: Queue::new(),
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
        let (w, last) = (WINDOW as i16, QUEUE_LAST);
        for i in 0..last as i16 {
            f.enqueue(i % w, i / w);
        }
        assert_eq!((f.queue.head, f.queue.tail), (0, last));
        assert_eq!(
            f.queue.seen,
            QueueSeen {
                peak: last as usize,
                dropped: 0
            }
        );
        let (dropped, kept) = (last as i16, last as i16 - 1);
        f.enqueue(dropped % w, dropped / w);
        assert_eq!(f.queue.tail, last);
        assert_eq!(f.queue.seen.dropped, 1, "the full queue's drop is counted");
        assert_eq!(
            f.flags[cell((dropped % w) as usize, (dropped / w) as usize)],
            0
        );
        assert_eq!(
            f.flags[cell((kept % w) as usize, (kept / w) as usize)],
            CELL_QUEUED
        );
        f.enqueue(-1, 3);
        f.enqueue(3, w);
        assert_eq!(f.queue.tail, last);
        assert_eq!(f.queue.seen.dropped, 1, "off-grid cells are no drops");
        // Round the ring: one dequeued, one more fits, and the tail wraps to 0.
        f.queue.head = next(f.queue.head);
        f.enqueue(dropped % w, dropped / w);
        assert_eq!((f.queue.head, f.queue.tail), (1, 0));
    }

    /// In the original the queue is 256 entries with byte indices.
    #[cfg(not(feature = "wide"))]
    #[test]
    fn the_original_queue_is_256_bytes_with_byte_indices() {
        assert_eq!((QUEUE_LEN, QUEUE_LAST), (256, 255));
        assert_eq!(std::mem::size_of::<QueueIndex>(), 1);
    }

    /// `map/view.rs` before spec 13, as written there, for every surface coordinate.
    #[cfg(not(feature = "wide"))]
    #[test]
    fn the_original_block_origin_is_the_object_pass_formula() {
        for c in 0..1024u16 {
            assert_eq!(block_origin(c), c.wrapping_sub(16) & 0xfff8, "{c}");
        }
    }

    /// The object pass's block corner is the rebuild's window origin, except where the window
    /// starts west of (north of) 0: there it is negative and the block's rectangle starts at 0.
    #[test]
    fn the_block_origin_is_the_window_origin_unless_the_window_wraps() {
        for c in 0..1024u16 {
            let (block, window) = (block_origin(c), geom::window_origin(c));
            if c >= geom::LEAD {
                assert_eq!(block, window, "{c}");
            } else {
                assert_eq!(block, window.wrapping_sub(0x400), "{c}");
                let x0 = block.wrapping_add(BLOCK_X.start as u16) as i16;
                let x1 = block.wrapping_add(BLOCK_X.end as u16 - 1) as i16;
                assert!(x1 >= c as i16 && x0.max(0) <= c as i16, "{c} in its block");
            }
        }
    }

    /// The sight flood's distance: the view's corner reads the table's (5, 5) and the cell next
    /// to the leader its (1, 0) or less; a light's flood reads the cell's own distance.
    #[test]
    fn sight_scales_the_distance_to_the_original_view_and_light_does_not() {
        let (t, tf) = (tables(), flags());
        let mut g = grids();
        let f = flood(&mut g, &t, &tf);
        let (hx, hy) = (HALF[0], HALF[1]);
        let r = TABLE_HALF;
        assert_eq!(f.flood_dist(hx, hy, FloodMode::Sight), r);
        assert_eq!(f.flood_dist(-hx, -hy, FloodMode::Sight), r);
        assert_eq!(f.flood_dist(hx, 0, FloodMode::Sight), r);
        assert_eq!(f.flood_dist(0, hy, FloodMode::Sight), r);
        assert!(f.flood_dist(1, 0, FloodMode::Sight) <= 1);
        assert_eq!(f.flood_dist(2, 1, FloodMode::Light), 2);
        // Past the table: too far, in both floods.
        assert_eq!(f.flood_dist(8, 0, FloodMode::Light), i16::MAX);
        assert_eq!(f.flood_dist(0, -9, FloodMode::Light), i16::MAX);
        let w = WINDOW as i16;
        assert_eq!(f.flood_dist(w, w, FloodMode::Sight), i16::MAX);
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

    /// The map overview's flood (0x13410a calls 0x109c12 from the leader's cell in mode 2 with
    /// light 0x28, asm 0x134172..0x13418e; mode 2 marks the cell seen at 0x109d48 and queues
    /// every unqueued neighbour at 0x109e0e..0x109e20), through the combat seam on cleared grids
    /// as 0x13410a leaves them: the leader's room, reaching past the view and its ring, and the
    /// walls that close it are seen; nothing else is, a room walled off from it included. Within
    /// the distance table each cell's light is `4 - dist + 0x28` clamped to 0..4 (asm
    /// 0x109d06..0x109d46); the table is raised by 0x26 here so that light covers 0..4.
    #[test]
    fn the_overview_sees_the_leader_s_room_and_not_a_walled_off_one() {
        const WALL: u8 = 1;
        let centre = u6_formats::position::Position {
            x: 0x80,
            y: 0x80,
            z: 1,
        };
        let mut s = crate::map::tests::open_state(centre);
        s.tables.tileflags.flags2[usize::from(WALL)] = TF2_BOUNDARY;
        let t = s.map.tables.as_mut().expect("tables");
        t.dist = std::array::from_fn(|a| std::array::from_fn(|b| a.max(b) as i8 + 0x26));
        let dist = t.dist;
        let [vx, vy] = s.map.view_offset;
        let (lx, ly) = (vx + HALF[0], vy + HALF[1]);
        let (rx, ry) = (HALF[0] + 2, HALF[1] + 2);
        let room =
            |x: i16, y: i16, grow: i16| (x - lx).abs() <= rx + grow && (y - ly).abs() <= ry + grow;
        let sealed = |x: i16, y: i16| (1..=3).contains(&x) && (1..=3).contains(&y);
        let w = WINDOW as i16;
        let cells = || (0..w).flat_map(move |y| (0..w).map(move |x| (x, y)));
        for (x, y) in cells() {
            let open = room(x, y, 0) || sealed(x, y);
            s.map.window_tiles[cell(x as usize, y as usize)] = if open { 0 } else { WALL };
        }
        s.map.view_flags.fill(0);
        s.map.light.fill(0);
        crate::combat::seams::flood_sight(&mut s, lx, ly);
        let seen =
            |x: i16, y: i16| s.map.view_flags[cell(x as usize, y as usize)] & CELL_VISIBLE != 0;
        assert!(
            seen(lx + rx, ly) && seen(lx - rx, ly - ry),
            "past the view's ring"
        );
        assert!(!seen(2, 2), "the walled-off room");
        for (x, y) in cells() {
            assert_eq!(seen(x, y), room(x, y, 1), "({x}, {y})");
        }
        let mut lit = [0; 5];
        for (x, y) in cells() {
            let (dx, dy) = ((x - lx).unsigned_abs(), (y - ly).unsigned_abs());
            let Some(&d) = dist
                .get(usize::from(dx))
                .and_then(|r| r.get(usize::from(dy)))
            else {
                continue;
            };
            let want = (4 - i16::from(d) + 0x28).clamp(0, 4) as u8;
            assert_eq!(
                s.map.light[cell(x as usize, y as usize)],
                want,
                "({x}, {y})"
            );
            lit[usize::from(want)] += 1;
        }
        assert!(
            lit.iter().all(|&n| n > 0),
            "every light level 0..4: {lit:?}"
        );
    }

    /// The overview's flood reaches the whole of an open window, more cells than the queue
    /// holds; what must fit is the cells queued at once. From every leader cell (window cells
    /// LEAD..LEAD + 7 on each axis) the queue never fills.
    #[test]
    fn the_overview_frontier_in_an_open_window_fits_the_queue() {
        let centre = u6_formats::position::Position {
            x: 0x80,
            y: 0x80,
            z: 1,
        };
        let t = crate::map::tests::open_state(centre).map.tables;
        let (t, tf) = (t.expect("tables"), flags());
        let lead = geom::LEAD as i16;
        let mut peak = 0;
        for y0 in lead..lead + 8 {
            for x0 in lead..lead + 8 {
                let mut g = grids();
                let mut f = flood(&mut g, &t, &tf);
                f.flood(x0, y0, OVERVIEW_LIGHT, FloodMode::Overview);
                assert_eq!(f.queue.seen.dropped, 0, "({x0}, {y0})");
                assert!(
                    f.flags.iter().all(|&c| c & CELL_VISIBLE != 0),
                    "({x0}, {y0})"
                );
                peak = peak.max(f.queue.seen.peak);
            }
        }
        println!("overview frontier: peak {peak} of {QUEUE_LEN} entries");
    }
}

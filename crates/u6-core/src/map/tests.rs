//! Unit tests on synthetic maps and tables (no game data).

use u6_formats::chunks::Chunks;
use u6_formats::map::{DUNGEON_CHUNKS, DUNGEON_LEVELS, SURFACE_CHUNKS, WorldMap};
use u6_formats::position::Position;
use u6_formats::tileflag::{TILE_COUNT, TileFlags};

use super::flags::*;
use super::view::{DARK, NOT_VISIBLE};
use super::*;
use crate::geom::{self, BLOCK_X, BLOCK_Y, HALF_H, HALF_W, LEAD, QUEUE_LEN, VIEW_H, VIEW_W};
use crate::state::State;

/// Surface chunk ref = (cx + cy) % 1024 per chunk, dungeon level L ref = 900 + L + (chunk
/// index % 3); chunk c tile (col, row) = (c + row * 8 + col) % 256.
fn world() -> MapState {
    let surface = (0..SURFACE_CHUNKS * SURFACE_CHUNKS)
        .map(|i| ((i % SURFACE_CHUNKS + i / SURFACE_CHUNKS) % 1024) as u16)
        .collect();
    let dungeons = (0..DUNGEON_LEVELS)
        .map(|l| {
            (0..DUNGEON_CHUNKS * DUNGEON_CHUNKS)
                .map(|i| (900 + l + i % 3) as u16)
                .collect()
        })
        .collect();
    let chunks = (0..1024)
        .map(|c| std::array::from_fn(|r| std::array::from_fn(|x| ((c + r * 8 + x) % 256) as u8)))
        .collect();
    MapState {
        world: WorldMap { surface, dungeons },
        chunks: Chunks { chunks },
        ..MapState::default()
    }
}

/// The tile of [`world`] at surface (x, y), from its doc's formulas.
fn world_tile(x: usize, y: usize) -> u8 {
    let chunk = (x / 8 + y / 8) % 1024;
    ((chunk + (y % 8) * 8 + x % 8) % 256) as u8
}

/// The tile of [`world`] at (x, y) of dungeon level `l` (z = l + 1), from its doc's formulas.
fn level_tile(l: usize, x: usize, y: usize) -> u8 {
    let chunk = 900 + l + ((y / 8) * DUNGEON_CHUNKS + x / 8) % 3;
    ((chunk + (y % 8) * 8 + x % 8) % 256) as u8
}

/// The view's half on x and y, as the view offsets' type.
const HALF: [i16; 2] = [HALF_W as i16, HALF_H as i16];

/// Synthetic view tables: Chebyshev distance, the 8-neighbour walk N, NE, E, SE, S, SW, W, NW
/// as steps from the previous neighbour, identity wall pieces.
fn tables() -> MapTables {
    let ring = [
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
    ];
    let mut prev = (0i8, 0i8);
    let (mut dx, mut dy) = ([0i8; 8], [0i8; 8]);
    for (k, &(x, y)) in ring.iter().enumerate() {
        (dx[k], dy[k]) = (x - prev.0, y - prev.1);
        prev = (x, y);
    }
    MapTables {
        dist: std::array::from_fn(|a| std::array::from_fn(|b| a.max(b) as i8)),
        walk_dx: dx,
        walk_dy: dy,
        // As the boot loads them: written with the image's stride, re-encoded for the grids.
        walk_step: tables::walk_steps(&dx, &dy, &image_steps(&dx, &dy)).unwrap(),
        wall: std::array::from_fn(|k| k as u8),
        gate_x: [0; 25],
        gate_y: [0; 25],
        gate_z: [0; 25],
    }
}

/// Walk steps `dy * 40 + dx`, as the image writes them.
fn image_steps(dx: &[i8; 8], dy: &[i8; 8]) -> [i8; 8] {
    std::array::from_fn(|k| (i32::from(dy[k]) * tables::IMAGE_STRIDE + i32::from(dx[k])) as i8)
}

/// A state on an all-zero map (chunk 0 everywhere, tile 0 open) with empty tile flags.
pub(super) fn open_state(centre: Position) -> State {
    let mut s = State::empty();
    s.map.world = WorldMap {
        surface: vec![0; SURFACE_CHUNKS * SURFACE_CHUNKS],
        dungeons: vec![vec![0; DUNGEON_CHUNKS * DUNGEON_CHUNKS]; DUNGEON_LEVELS],
    };
    s.map.chunks = Chunks {
        chunks: vec![[[0; 8]; 8]; 1024],
    };
    s.tables.tileflags = TileFlags {
        flags1: vec![0; TILE_COUNT],
        flags2: vec![0; TILE_COUNT],
        weight: vec![0; 1024],
        flags3: vec![0; TILE_COUNT],
    };
    s.tables.basetile.base = vec![0; 1024];
    s.map.tables = Some(tables());
    s.map.centre = centre;
    s.actors.objlist_tail = vec![0; 0x82];
    region::rebuild_window(&mut s);
    s
}

fn at(x: u16, y: u16) -> Position {
    Position { x, y, z: 0 }
}

#[test]
fn chunk_ref_masks_and_wraps_dungeon_coordinates() {
    let m = world();
    assert_eq!(m.chunk_ref(17, 9, 0), Some(2 + 1));
    // x, y masked to 10 bits.
    assert_eq!(m.chunk_ref(17 + 1024, 9, 0), Some(3));
    // Dungeon chunk column / row modulo 32: x 8 and x 264 are the same chunk.
    assert_eq!(m.chunk_ref(8, 0, 2), m.chunk_ref(264, 256, 2));
    assert_eq!(m.chunk_ref(8, 0, 2), Some(901 + 1));
    assert_eq!(m.chunk_ref(0, 0, 6), None);
    assert_eq!(MapState::default().chunk_ref(0, 0, 0), None);
}

#[test]
fn tile_at_reads_the_window_cache_inside_it_and_the_chunks_outside() {
    let mut m = world();
    // Before any window: the chunks.
    assert_eq!(m.tile_at(17, 9, 0), Some(3 + 8 + 1));
    m.window_tiles = vec![0xaa; WINDOW_CELLS];
    m.window_origin = [16, 8];
    assert_eq!(m.tile_at(17, 9, 0), Some(0xaa));
    assert_eq!(m.tile_at(16 + WINDOW - 1, 8 + WINDOW - 1, 0), Some(0xaa));
    assert_eq!(
        m.tile_at(16 + WINDOW, 9, 0),
        Some(world_tile(16 + WINDOW, 9))
    );
    assert_eq!(
        m.tile_at(17, 8 + WINDOW, 0),
        Some(world_tile(17, 8 + WINDOW))
    );
    // x below the origin wraps (mask 0x3ff) far outside the window.
    assert_eq!(m.tile_at(15, 9, 0), m.chunks.tile(2, 7, 1));
    // The dungeon mask wraps x 256 + 17 onto 17.
    m.world_mask = DUNGEON_MASK;
    assert_eq!(m.tile_at(256 + 17, 9, 1), Some(0xaa));
}

#[test]
fn rebuild_window_aligns_on_chunks_and_keeps_an_unchanged_window() {
    let mut s = State::empty();
    s.map = world();
    s.map.centre = at(339, 400);
    region::rebuild_window(&mut s);
    let m = &s.map;
    // Original: (320, 384), the leader in cells (19, 16).
    let (ox, oy) = (geom::window_origin(339), geom::window_origin(400));
    assert_eq!((ox % 8, oy % 8), (0, 0));
    assert!((LEAD..LEAD + 8).contains(&(339 - ox)) && (LEAD..LEAD + 8).contains(&(400 - oy)));
    assert_eq!(m.window_origin, [ox, oy]);
    let offset = |x: u16, y: u16| [(x - ox) as i16 - HALF[0], (y - oy) as i16 - HALF[1]];
    assert_eq!(m.view_offset, offset(339, 400));
    assert_eq!(m.window_key, [ox as i16, oy as i16, 0]);
    assert_eq!(
        (m.world_mask, m.view_dirty, m.flag_784e),
        (SURFACE_MASK, 1, 1)
    );
    for y in 0..WINDOW {
        for x in 0..WINDOW {
            let (wx, wy) = (usize::from(ox) + x, usize::from(oy) + y);
            let chunk = m.chunk_ref(wx as u16, wy as u16, 0).unwrap();
            let want = m.chunks.tile(chunk.into(), wx % 8, wy % 8).unwrap();
            assert_eq!(m.window_tiles[cell(x, y)], want, "cell ({x}, {y})");
        }
    }
    // Same origin and level: the cache is kept (a poked cell survives). Original (342, 403).
    s.map.window_tiles[0] ^= 0xff;
    let poked = s.map.window_tiles[0];
    let (x1, y1) = (ox + LEAD + 6, oy + LEAD + 3);
    s.map.centre = at(x1, y1);
    region::rebuild_window(&mut s);
    assert_eq!(s.map.window_tiles[0], poked);
    assert_eq!(s.map.view_offset, offset(x1, y1));
    // A new origin refills it. Original (362, 403), origin (344, 384).
    s.map.centre = at(ox + LEAD + 26, y1);
    region::rebuild_window(&mut s);
    assert_eq!(s.map.window_origin, [ox + 24, oy]);
    assert_ne!(s.map.window_tiles[0], poked);
}

#[test]
fn rebuild_window_wraps_near_the_map_edge_and_masks_dungeons() {
    let mut s = State::empty();
    s.map = world();
    s.map.centre = Position { x: 3, y: 20, z: 2 };
    region::rebuild_window(&mut s);
    // (3 - 16) & 0x3f8 = 1008; offset (3 - 1008 - 5) & 0x1f = 14 (the leader in cell 19).
    let ox = geom::window_origin(3);
    assert!(ox > 1024 - WINDOW as u16, "the window starts west of 0");
    assert_eq!(s.map.window_origin, [ox, geom::window_origin(20)]);
    let lead_x = (3 + 1024 - ox) as i16;
    assert_eq!(s.map.view_offset[0], lead_x - HALF[0]);
    assert_eq!(s.map.world_mask, DUNGEON_MASK);
    // Window cell (16 + 3 - 0, ...) is world x 3 through the 0xff mask.
    assert_eq!(s.map.tile_at(3, 20, 2), s.map.chunks.tile(902, 3, 4));
}

#[test]
fn eggs_far_or_active_or_on_arrival_are_offered() {
    // An offered egg is hatched (objects 0x1319b2): with no template it hatches nothing but
    // is marked (status 0x40 | 0x02); the marks show which eggs were offered.
    let mut s = open_state(at(300, 300));
    let [ox, oy] = s.map.window_origin;
    let egg = |s: &mut State, slot: u16, x: u16, y: u16| {
        s.objects.type_frame[usize::from(slot)] = 0x14f;
        s.objects.location[usize::from(slot)] = at(x, y);
        crate::main_loop::test_support::link_placed_objects(s);
    };
    // An egg is near up to 3 past the half view of its axis (spec 13 Decision 5): Original 8
    // each way, Wide 21 east-west and 13 north-south. Taken from geom, not from the pass.
    let (near_x, near_y) = (HALF_W as u16 + 3, HALF_H as u16 + 3);
    assert_eq!(region::EGG_NEAR, [near_x as i16, near_y as i16]);
    egg(&mut s, 5, 300 + near_x, 300); // at the threshold east: near
    egg(&mut s, 6, 300 + near_x + 1, 300); // one past it east: far
    egg(&mut s, 7, 300, 300 - near_y); // at the threshold north: near
    egg(&mut s, 8, 300, 300 - near_y - 1); // one past it north: far
    // The window's south-east corner cell: far, and in the rectangle the pass walks.
    let last = WINDOW as u16 - 1;
    egg(&mut s, 9, ox + last, oy + last);
    let offered = |s: &mut State, status5: u8| {
        for e in 5..10 {
            s.objects.obj_status[e] = 0;
        }
        s.objects.obj_status[5] = status5;
        region::check_eggs(s);
        (5..10)
            .map(|e| s.objects.obj_status[e] & 0x40 != 0)
            .collect::<Vec<_>>()
    };
    assert_eq!(offered(&mut s, 0), [false, true, false, true, true]);
    assert_eq!(offered(&mut s, 0x20), [true, true, false, true, true]);
    s.map.arriving = 1;
    assert_eq!(offered(&mut s, 0), [true; 5]);
    s.map.flag_7afb = 1;
    assert_eq!(offered(&mut s, 0), [false; 5]);
}

#[test]
fn tables_reject_a_file_that_is_not_the_game() {
    assert!(MapTables::from_game(&[0; 64]).is_err());
}

#[test]
fn passable_is_the_map_tile_blocking_bit() {
    let mut s = open_state(at(300, 300));
    s.tables.tileflags.flags1[0] = TF1_BLOCKING;
    assert!(!s.map.is_passable(&s.tables.tileflags, 300, 300, 0));
    s.tables.tileflags.flags1[0] = TF1_WATER | TF1_WALL;
    assert!(s.map.is_passable(&s.tables.tileflags, 300, 300, 0));
    assert_eq!(
        s.map.tile_flags1_at(&s.tables.tileflags, 300, 300, 0),
        Some(5)
    );
    s.tables.tileflags.flags1[0] = 0x30;
    assert_eq!(impedance(&s.tables.tileflags, 0), 3);
}

/// Window cell of world (x, y) for a state from `open_state`.
fn wcell(s: &State, x: u16, y: u16) -> usize {
    let [ox, oy] = s.map.window_origin;
    cell(usize::from(x - ox), usize::from(y - oy))
}

/// Puts an object of a fresh type at world (x, y) whose tile has the given flags.
fn object(s: &mut State, slot: u16, (x, y): (u16, u16), tile: u16, [f1, f2, f3]: [u8; 3]) {
    let ty = slot; // one type per slot
    s.tables.basetile.base[usize::from(ty)] = tile;
    s.tables.tileflags.flags1[usize::from(tile)] = f1;
    s.tables.tileflags.flags2[usize::from(tile)] = f2;
    s.tables.tileflags.flags3[usize::from(tile)] = f3;
    s.objects.type_frame[usize::from(slot)] = ty;
    s.objects.location[usize::from(slot)] = at(x, y);
    crate::main_loop::test_support::link_placed_objects(s);
}

fn set_ambient(s: &mut State, light: i8) {
    s.actors.objlist_tail[0x0b] = light as u8;
}

#[test]
fn daylight_open_field_shows_every_view_tile_lit() {
    let mut s = open_state(at(300, 300));
    set_ambient(&mut s, 7);
    view::update_view(&mut s);
    for row in 0..VIEW_H {
        for col in 0..VIEW_W {
            assert!(s.map.is_visible(col, row));
            assert_eq!(s.map.light_at(col, row), 4);
            assert_eq!(s.map.view_tile(col, row), 0);
        }
    }
    // The corners, farthest from the leader (spec 13 Decision 8).
    let (w, h) = (VIEW_W - 1, VIEW_H - 1);
    for (col, row) in [(0, 0), (w, 0), (0, h), (w, h)] {
        assert_eq!(s.map.light_at(col, row), 4, "corner ({col}, {row})");
    }
}

/// The scaled sight distance of view cell (col, row) under the Chebyshev table of `tables`:
/// each axis laid over the original's half view of 5 (spec 13 Decision 8).
fn sight_dist(col: usize, row: usize) -> usize {
    let r = geom::RULE_HALF as usize;
    (col.abs_diff(HALF_W) * r / HALF_W).max(row.abs_diff(HALF_H) * r / HALF_H)
}

/// For every cell the leader can stand in within its chunk (the window keeps its origin over
/// eight cells), the view and its one-cell ring lie in the inner block, the sight flood of an
/// open field queues exactly the view and its ring, the queue never fills (no cell dropped, the
/// most it held below its size), and every view cell is seen.
#[test]
fn every_leader_cell_sees_its_whole_view_and_its_ring_lies_in_the_block() {
    let area = (VIEW_W + 2) * (VIEW_H + 2);
    assert!(area < QUEUE_LEN, "the sight area fits the queue");
    let mut cells = [[false; 8]; 2];
    for dy in 0..8 {
        for dx in 0..8 {
            let mut s = open_state(at(296 + dx, 296 + dy));
            set_ambient(&mut s, 7);
            view::update_view(&mut s);
            let queue = view::last_queue();
            let [ox, oy] = s.map.window_origin;
            let lead = [296 + dx - ox, 296 + dy - oy].map(usize::from);
            assert_eq!(queue.dropped, 0, "no cell dropped at {lead:?}");
            assert!(queue.peak > 0 && queue.peak < QUEUE_LEN, "{lead:?}");
            cells[0][lead[0] - usize::from(LEAD)] = true;
            cells[1][lead[1] - usize::from(LEAD)] = true;
            let [vx, vy] = s.map.view_offset.map(|v| v as usize);
            assert_eq!([vx + HALF_W, vy + HALF_H], lead, "the leader's view cell");
            for (lo, hi, block) in [(vx, vx + VIEW_W, BLOCK_X), (vy, vy + VIEW_H, BLOCK_Y)] {
                assert!(
                    lo >= 1 && block.contains(&(lo - 1)) && block.contains(&hi),
                    "ring {}..={hi} in {block:?}",
                    lo - 1
                );
            }
            let queued = s
                .map
                .view_flags
                .iter()
                .filter(|&&f| f & CELL_QUEUED != 0)
                .count();
            assert_eq!(queued, area, "leader cell {lead:?}");
            for row in 0..VIEW_H {
                for col in 0..VIEW_W {
                    assert!(s.map.is_visible(col, row), "({col}, {row}) at {lead:?}");
                    assert_eq!(s.map.light_at(col, row), 4);
                }
            }
        }
    }
    assert_eq!(cells, [[true; 8]; 2], "every leader cell reached");
}

/// The view is `VIEW_W` columns across and `VIEW_H` rows down: the view tiles are row-major
/// with `VIEW_W` per row (spec 13 C1), sight reaches one cell past the view on each axis and
/// no farther, and the view drawn at 16 pixels a cell fits the frame. Fails with `VIEW_W` and
/// `VIEW_H` swapped in Wide, in `geom` or at a site here.
#[test]
fn the_view_runs_view_w_across_and_view_h_down() {
    let mut s = open_state(at(300, 300));
    set_ambient(&mut s, 7);
    let [vx, vy] = s.map.view_offset.map(|v| v as usize);
    // Tile 1 down the view's last column, tile 2 along its last row, 3 at that corner.
    for row in 0..VIEW_H {
        s.map.window_tiles[cell(vx + VIEW_W - 1, vy + row)] = 1;
    }
    for col in 0..VIEW_W {
        s.map.window_tiles[cell(vx + col, vy + VIEW_H - 1)] = 2;
    }
    s.map.window_tiles[cell(vx + VIEW_W - 1, vy + VIEW_H - 1)] = 3;
    view::update_view(&mut s);
    let t = &s.map.view_tiles;
    assert_eq!(t.len(), VIEW_W * VIEW_H);
    for (i, &tile) in t.iter().enumerate() {
        let (col, row) = (i % VIEW_W, i / VIEW_W);
        let want = match (col == VIEW_W - 1, row == VIEW_H - 1) {
            (true, true) => 3,
            (true, false) => 1,
            (false, true) => 2,
            (false, false) => 0,
        };
        assert_eq!(tile, want, "view cell ({col}, {row})");
        assert_eq!(s.map.view_tile(col, row), tile);
    }
    let seen = |x: usize, y: usize| s.map.view_flags[cell(x, y)] & CELL_VISIBLE != 0;
    let (lx, ly) = (vx + HALF_W, vy + HALF_H);
    assert!(seen(vx - 1, ly) && seen(vx + VIEW_W, ly), "the ring across");
    assert!(seen(lx, vy - 1) && seen(lx, vy + VIEW_H), "the ring down");
    assert!(!seen(vx - 2, ly) && !seen(vx + VIEW_W + 1, ly));
    assert!(!seen(lx, vy - 2) && !seen(lx, vy + VIEW_H + 1));
    let (fw, fh) = (u6_render::WIDTH, u6_render::HEIGHT);
    assert!(
        usize::from(geom::MAP_X) + VIEW_W * 16 <= fw,
        "columns across the frame"
    );
    assert!(
        usize::from(geom::MAP_Y) + VIEW_H * 16 <= fh,
        "rows down the frame"
    );
}

/// 0x108d10's copy: with a window side that is no multiple of the chunk the last column and row
/// of chunks are cut at the window (Wide: cells 56..59 from the chunk at origin + 56).
#[test]
fn fill_window_takes_the_last_chunks_clipped_to_the_window() {
    let mut s = State::empty();
    s.map = world();
    s.map.centre = at(339, 400);
    region::rebuild_window(&mut s);
    let [ox, oy] = s.map.window_origin.map(usize::from);
    // The last chunk's first cell: Original 32 (the window ends with a whole chunk), Wide 56.
    let last = (WINDOW - 1) / 8 * 8;
    assert_eq!(s.map.window_tiles.len(), WINDOW_CELLS);
    // Tile (col, row) of the chunk whose top-left cell is window cell (x, y).
    let chunk_tile = |(x, y): (usize, usize), col: usize, row: usize| {
        let chunk = ((ox + x) / 8 + (oy + y) / 8) % 1024;
        ((chunk + row * 8 + col) % 256) as u8
    };
    for i in 0..WINDOW {
        let i0 = i / 8 * 8;
        for j in last..WINDOW {
            // The last column of chunks, then the last row.
            let column = chunk_tile((last, i0), j - last, i - i0);
            assert_eq!(s.map.window_tiles[cell(j, i)], column, "({j}, {i})");
            let row = chunk_tile((i0, last), i - i0, j - last);
            assert_eq!(s.map.window_tiles[cell(i, j)], row, "({i}, {j})");
        }
    }
}

/// A leader within 3 cells of a dungeon level's edge (256 wide): the window starts where the
/// rebuild formulas put it, wraps at 256 through the level's mask, and every cell is the
/// level's tile.
#[test]
fn a_leader_near_a_dungeon_level_edge_rebuilds_the_window_from_the_level() {
    let edges = [0u16, 1, 2, 3, 252, 253, 254, 255];
    for (i, &x) in edges.iter().enumerate() {
        let y = edges[(i + 3) % edges.len()];
        let mut s = State::empty();
        s.map = world();
        s.map.centre = Position { x, y, z: 1 };
        region::rebuild_window(&mut s);
        let m = &s.map;
        let (ox, oy) = (geom::window_origin(x), geom::window_origin(y));
        assert_eq!(m.window_origin, [ox, oy], "at ({x}, {y})");
        assert_eq!(
            m.view_offset,
            [
                geom::view_offset(x, ox, HALF_W as u16),
                geom::view_offset(y, oy, HALF_H as u16)
            ]
        );
        assert_eq!(m.world_mask, DUNGEON_MASK);
        for wy in 0..WINDOW {
            for wx in 0..WINDOW {
                let lx = usize::from(ox.wrapping_add(wx as u16) & DUNGEON_MASK);
                let ly = usize::from(oy.wrapping_add(wy as u16) & DUNGEON_MASK);
                assert_eq!(
                    m.window_tiles[cell(wx, wy)],
                    level_tile(0, lx, ly),
                    "cell ({wx}, {wy}) at ({x}, {y})"
                );
            }
        }
        let [vx, vy] = m.view_offset.map(|v| v as usize);
        let leader = m.window_tiles[cell(vx + HALF_W, vy + HALF_H)];
        assert_eq!(m.tile_at(x.into(), y.into(), 1), Some(leader));
        assert_eq!(leader, level_tile(0, x.into(), y.into()));
    }
}

/// The image's walk steps are checked at its stride of 40 and re-encoded for the port's grids.
#[test]
fn walk_steps_are_checked_at_the_image_stride_and_re_encoded_with_the_window() {
    let t = tables();
    let w = WINDOW as i32;
    for k in 0..8 {
        let (dx, dy) = (i32::from(t.walk_dx[k]), i32::from(t.walk_dy[k]));
        assert_eq!(i32::from(t.walk_step[k]), dy * w + dx, "step {k}");
    }
    let image = image_steps(&t.walk_dx, &t.walk_dy);
    let mut bad = image;
    bad[3] = bad[3].wrapping_add(1);
    assert_eq!(tables::walk_steps(&t.walk_dx, &t.walk_dy, &bad), Err(3));
    // Steps already in a window side other than 40 are no image's.
    let first_row_step = t.walk_dy.iter().position(|&d| d != 0).unwrap();
    let port = tables::walk_steps(&t.walk_dx, &t.walk_dy, &t.walk_step);
    if w == tables::IMAGE_STRIDE {
        assert_eq!(port, Ok(t.walk_step));
    } else {
        assert_eq!(port, Err(first_row_step));
    }
}

#[test]
fn a_boundary_wall_hides_what_is_behind_it() {
    let mut s = open_state(at(300, 300));
    set_ambient(&mut s, 7);
    s.tables.tileflags.flags2[1] = TF2_BOUNDARY;
    let [vx, vy] = s.map.view_offset.map(|v| v as usize);
    // A wall two columns east of the leader, across the whole sight area.
    let wall = HALF_W + 2;
    for y in vy - 1..=vy + VIEW_H {
        s.map.window_tiles[cell(vx + wall, y)] = 1;
    }
    view::update_view(&mut s);
    for row in 0..VIEW_H {
        assert_eq!(s.map.view_tile(wall - 1, row), 0);
        assert_eq!(s.map.view_tile(wall, row), 1, "the wall itself is seen");
        for col in wall + 1..VIEW_W {
            assert_eq!(s.map.view_tile(col, row), NOT_VISIBLE);
        }
    }
}

#[test]
fn night_sight_lights_only_near_the_leader() {
    let mut s = open_state(at(300, 300));
    set_ambient(&mut s, 0);
    view::update_view(&mut s);
    for row in 0..VIEW_H {
        for col in 0..VIEW_W {
            let d = sight_dist(col, row);
            let want = if d < 4 { 0 } else { DARK };
            assert_eq!(s.map.view_tile(col, row), want, "({col}, {row})");
            assert!(s.map.is_visible(col, row));
        }
    }
}

#[test]
fn an_object_light_floods_its_radius_at_night() {
    let mut s = open_state(at(300, 300));
    set_ambient(&mut s, 0);
    let mut dark = open_state(at(300, 300));
    set_ambient(&mut dark, 0);
    object(&mut s, 300, (302, 300), 50, [0, 3, 0]);
    view::update_view(&mut s);
    view::update_view(&mut dark);
    let l = |s: &State, x, y| s.map.light[wcell(s, x, y)];
    // The leader's sight, by the view cell's scaled distance (Original: 4 - 2 = 2 at 302).
    let [vx, vy] = s.map.view_offset;
    let [ox, oy] = s.map.window_origin;
    let sight = |x: u16, y: u16| {
        let (col, row) = ((x - ox) as i16 - vx, (y - oy) as i16 - vy);
        4u8.saturating_sub(sight_dist(col as usize, row as usize) as u8)
    };
    // Plus the light's own 4 - 0 - 1 = 3.
    assert_eq!(l(&s, 302, 300), sight(302, 300) + 3);
    // Two cells past the light: its 4 - 2 - 1 = 1; the mirrored side gets none of it.
    assert_eq!(l(&s, 304, 300), sight(304, 300) + 1);
    assert_eq!(l(&s, 296, 300), sight(296, 300));
    assert_eq!(s.map.view_flags[wcell(&s, 302, 300)] & CELL_LIGHT, 3);
    // The light reaches the cells within 2 of it, real distance, whatever the view's size.
    for y in 300 - HALF_H as u16..=300 + HALF_H as u16 {
        for x in 300 - HALF_W as u16..=300 + HALF_W as u16 {
            let near = x.abs_diff(302).max(y.abs_diff(300));
            let want = 3u8.saturating_sub(near as u8);
            assert_eq!(l(&s, x, y) - l(&dark, x, y), want, "({x}, {y})");
        }
    }
}

#[test]
fn a_lit_wall_above_a_hidden_cell_stalls_the_rest_of_its_row() {
    // asm 0x10a15e -> 0x10a1c4: the light pass stops advancing its pointer on that row.
    let mut s = open_state(at(300, 300));
    set_ambient(&mut s, 0);
    let [ox, oy] = s.map.window_origin;
    let w = |x: u16, y: u16| (ox + x, oy + y);
    s.tables.tileflags.flags1[2] = TF1_WALL;
    // The bottom row of the sight area, the cell below never seen; the leader's column 20 and
    // the row 26 in the original.
    let [vx, vy] = s.map.view_offset.map(|v| v as usize);
    let (lx, bottom) = (vx + HALF_W, vy + VIEW_H);
    let (wall, before, after) = (lx - 3, lx - 5, lx + 1);
    let at = |x: usize| w(x as u16, bottom as u16);
    s.map.window_tiles[cell(wall, bottom)] = 2;
    object(&mut s, 300, at(wall), 60, [0, 3, 0]);
    object(&mut s, 301, at(after), 61, [0, 3, 0]);
    object(&mut s, 302, at(before), 62, [0, 3, 0]);
    view::update_view(&mut s);
    let l = |x: usize| s.map.light[cell(x, bottom)];
    assert_eq!(l(before), 3, "a light before the wall floods");
    assert_eq!(l(wall), 1, "the wall gets only its neighbour's light");
    assert_eq!(
        l(after),
        0,
        "a light after the wall on the same row never floods"
    );
    // Past the stall the pointer lags: the next row starts `WINDOW - BLOCK_X.len()` cells after
    // the wall on the wall's own row (Original 16, Wide 4). `after` is 4 cells past the wall, so
    // in Wide the next row's first pass reads it and floods its light with that pass's
    // coordinates, (BLOCK_X.start, bottom + 1) = (0, bottom + 1), its own 4 - 0 - 1 = 3 there;
    // in Original the lag passes it by and nothing floods on that row.
    let lag = WINDOW - BLOCK_X.len();
    assert_eq!(after - wall == lag, cfg!(feature = "wide"));
    let below = s.map.light[cell(BLOCK_X.start, bottom + 1)];
    assert_eq!(
        below,
        if after - wall == lag { 3 } else { 0 },
        "the lagging pointer"
    );
    assert_eq!(
        s.map.light[cell(0, bottom + 1)],
        if cfg!(feature = "wide") { 3 } else { 0 }
    );
}

#[test]
fn objects_mark_their_cells_and_double_size_neighbours() {
    let mut s = open_state(at(300, 300));
    set_ambient(&mut s, 7);
    // Window object, boundary object, see-through object.
    object(&mut s, 300, (298, 298), 70, [0, TF2_WINDOW, 0]);
    object(&mut s, 301, (299, 298), 71, [0, TF2_BOUNDARY, 0]);
    object(&mut s, 302, (301, 298), 72, [0, 0, TF3_SEE_THROUGH]);
    // Double width + height wall whose tile - 1 is a boundary: covers left and above too.
    s.tables.tileflags.flags2[79] = TF2_BOUNDARY;
    let double = TF2_DOUBLE_WIDTH | TF2_DOUBLE_HEIGHT;
    object(&mut s, 303, (303, 303), 80, [TF1_WALL, double, 0]);
    view::update_view(&mut s);
    let f = |x, y| s.map.view_flags[wcell(&s, x, y)];
    assert_eq!(f(298, 298) & 0x3f, CELL_WINDOW);
    assert_eq!(f(299, 298) & 0x3f, CELL_BOUNDARY);
    assert_eq!(f(301, 298) & 0x3f, CELL_SEE_THROUGH);
    assert_eq!(f(303, 303) & 0x3f, CELL_WALL);
    assert_eq!(f(302, 303) & 0x3f, CELL_WALL | CELL_BOUNDARY);
    assert_eq!(f(303, 302) & 0x3f, CELL_WALL | CELL_BOUNDARY);
}

#[test]
fn reveal_all_and_missing_tables_show_the_window_lit() {
    let mut s = open_state(at(300, 300));
    let [vx, vy] = s.map.view_offset.map(|v| v as usize);
    s.map.window_tiles[cell(vx + 3, vy + 4)] = 9;
    s.map.reveal_all = 1;
    s.map.view_flags[cell(vx, vy)] = CELL_BOUNDARY;
    view::update_view(&mut s);
    assert_eq!(s.map.view_tile(3, 4), 9);
    assert_eq!(s.map.light_at(0, 0), 4);
    assert_eq!(
        s.map.view_flags[cell(vx, vy)],
        CELL_BOUNDARY,
        "flags untouched"
    );
    s.map.reveal_all = 0;
    s.map.tables = None;
    s.map.light.fill(0);
    view::update_view(&mut s);
    assert_eq!(s.map.light_at(HALF_W, HALF_H), 4);
    assert_eq!(s.map.light_at(VIEW_W - 1, VIEW_H - 1), 4);
}

#[test]
fn the_border_ring_of_the_grids_is_never_cleared() {
    let mut s = open_state(at(300, 300));
    // Original: (3, 3) and (39, 0) outside the block 8..32, (8, 8) its first cell.
    let (x0, y0) = (BLOCK_X.start, BLOCK_Y.start);
    let outside = [(x0, y0 - 1), (WINDOW - 1, 0), (x0, BLOCK_Y.end), (3, 3)];
    for &(x, y) in &outside {
        assert!(!(BLOCK_X.contains(&x) && BLOCK_Y.contains(&y)));
        s.map.view_flags[cell(x, y)] = 0x5a;
        s.map.light[cell(x, y)] = 0x77;
    }
    let inside = [(x0, y0), (BLOCK_X.end - 1, BLOCK_Y.end - 1)];
    for &(x, y) in &inside {
        s.map.view_flags[cell(x, y)] = 0x5a;
        s.map.light[cell(x, y)] = 0x77;
    }
    view::update_view(&mut s);
    for &(x, y) in &outside {
        assert_eq!(s.map.view_flags[cell(x, y)], 0x5a, "({x}, {y})");
        assert_eq!(s.map.light[cell(x, y)], 0x77, "({x}, {y})");
    }
    for &(x, y) in &inside {
        assert_eq!(
            s.map.view_flags[cell(x, y)],
            0,
            "inner block cleared ({x}, {y})"
        );
        assert_eq!(s.map.light[cell(x, y)], 0, "inner block cleared ({x}, {y})");
    }
}

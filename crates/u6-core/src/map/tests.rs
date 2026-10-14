//! Unit tests on synthetic maps and tables (no game data).

use u6_formats::chunks::Chunks;
use u6_formats::map::{DUNGEON_CHUNKS, DUNGEON_LEVELS, SURFACE_CHUNKS, WorldMap};
use u6_formats::position::Position;
use u6_formats::tileflag::{TILE_COUNT, TileFlags};

use super::flags::*;
use super::view::{DARK, NOT_VISIBLE};
use super::*;
use crate::state::State;
use crate::stub;

/// Surface chunk ref = (cx + cy) % 1024 per chunk, dungeon level L ref = 900 + L; chunk c tile
/// (col, row) = (c + row * 8 + col) % 256.
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
    let (mut dx, mut dy, mut step) = ([0i8; 8], [0i8; 8], [0i8; 8]);
    for (k, &(x, y)) in ring.iter().enumerate() {
        (dx[k], dy[k]) = (x - prev.0, y - prev.1);
        step[k] = dy[k] * 40 + dx[k];
        prev = (x, y);
    }
    MapTables {
        dist: std::array::from_fn(|a| std::array::from_fn(|b| a.max(b) as i8)),
        walk_dx: dx,
        walk_dy: dy,
        walk_step: step,
        wall: std::array::from_fn(|k| k as u8),
        gate_x: [0; 25],
        gate_y: [0; 25],
        gate_z: [0; 25],
    }
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

fn hits(id: &str) -> u64 {
    stub::detached_hits().get(id).copied().unwrap_or(0)
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
    assert_eq!(m.tile_at(55, 47, 0), Some(0xaa));
    assert_eq!(m.tile_at(56, 9, 0), Some(8 + 8));
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
    s.map.centre = at(307, 352);
    region::rebuild_window(&mut s);
    let m = &s.map;
    assert_eq!(m.window_origin, [288, 336]);
    assert_eq!(m.view_offset, [307 - 288 - 5, 352 - 336 - 5]);
    assert_eq!(m.window_key, [288, 336, 0]);
    assert_eq!(
        (m.world_mask, m.view_dirty, m.flag_784e),
        (SURFACE_MASK, 1, 1)
    );
    for y in 0..WINDOW {
        for x in 0..WINDOW {
            let (wx, wy) = (288 + x, 336 + y);
            let chunk = m.chunk_ref(wx as u16, wy as u16, 0).unwrap();
            let want = m.chunks.tile(chunk.into(), wx % 8, wy % 8).unwrap();
            assert_eq!(m.window_tiles[cell(x, y)], want, "cell ({x}, {y})");
        }
    }
    // Same origin and level: the cache is kept (a poked cell survives).
    s.map.window_tiles[0] ^= 0xff;
    let poked = s.map.window_tiles[0];
    s.map.centre = at(310, 355);
    region::rebuild_window(&mut s);
    assert_eq!(s.map.window_tiles[0], poked);
    assert_eq!(s.map.view_offset, [310 - 288 - 5, 355 - 336 - 5]);
    // A new origin refills it.
    s.map.centre = at(330, 355);
    region::rebuild_window(&mut s);
    assert_eq!(s.map.window_origin, [312, 336]);
    assert_ne!(s.map.window_tiles[0], poked);
}

#[test]
fn rebuild_window_wraps_near_the_map_edge_and_masks_dungeons() {
    let mut s = State::empty();
    s.map = world();
    s.map.centre = Position { x: 3, y: 20, z: 2 };
    region::rebuild_window(&mut s);
    // (3 - 16) & 0x3f8 = 1008; offset (3 - 1008 - 5) & 0x1f = 14.
    assert_eq!(s.map.window_origin, [1008, 0]);
    assert_eq!(s.map.view_offset, [14, 15]);
    assert_eq!(s.map.world_mask, DUNGEON_MASK);
    // Window cell (16 + 3 - 0, ...) is world x 3 through the 0xff mask.
    assert_eq!(s.map.tile_at(3, 20, 2), s.map.chunks.tile(902, 3, 4));
}

#[test]
fn eggs_far_or_active_or_on_arrival_are_offered() {
    let id = "0x1319b2 hatch an egg";
    let mut s = open_state(at(300, 300));
    let egg = |s: &mut State, slot: u16, x: u16, y: u16| {
        s.objects.type_frame[usize::from(slot)] = 0x14f;
        s.objects.location[usize::from(slot)] = at(x, y);
    };
    egg(&mut s, 5, 305, 300); // near
    egg(&mut s, 6, 309, 300); // 9 tiles east: far
    egg(&mut s, 7, 300, 291); // 9 tiles north: far
    let before = hits(id);
    region::check_eggs(&mut s);
    assert_eq!(hits(id) - before, 2);
    s.objects.actor_flags[5] = 0x20;
    region::check_eggs(&mut s);
    assert_eq!(hits(id) - before, 2 + 3);
    s.objects.actor_flags[5] = 0;
    s.map.arriving = 1;
    region::check_eggs(&mut s);
    assert_eq!(hits(id) - before, 5 + 3);
    s.map.flag_7afb = 1;
    region::check_eggs(&mut s);
    assert_eq!(hits(id) - before, 8);
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
}

fn set_ambient(s: &mut State, light: i8) {
    s.actors.objlist_tail[0x0b] = light as u8;
}

#[test]
fn daylight_open_field_shows_every_view_tile_lit() {
    let mut s = open_state(at(300, 300));
    set_ambient(&mut s, 7);
    view::update_view(&mut s);
    for row in 0..VIEW {
        for col in 0..VIEW {
            assert!(s.map.is_visible(col, row));
            assert_eq!(s.map.light_at(col, row), 4);
            assert_eq!(s.map.view_tile(col, row), 0);
        }
    }
}

#[test]
fn a_boundary_wall_hides_what_is_behind_it() {
    let mut s = open_state(at(300, 300));
    set_ambient(&mut s, 7);
    s.tables.tileflags.flags2[1] = TF2_BOUNDARY;
    let [vx, vy] = s.map.view_offset.map(|v| v as usize);
    for y in vy - 1..=vy + 11 {
        s.map.window_tiles[cell(vx + 7, y)] = 1;
    }
    view::update_view(&mut s);
    for row in 0..VIEW {
        assert_eq!(s.map.view_tile(6, row), 0);
        assert_eq!(s.map.view_tile(7, row), 1, "the wall itself is seen");
        for col in 8..VIEW {
            assert_eq!(s.map.view_tile(col, row), NOT_VISIBLE);
        }
    }
}

#[test]
fn night_sight_lights_only_near_the_leader() {
    let mut s = open_state(at(300, 300));
    set_ambient(&mut s, 0);
    view::update_view(&mut s);
    for row in 0..VIEW {
        for col in 0..VIEW {
            let d = col.abs_diff(5).max(row.abs_diff(5));
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
    object(&mut s, 300, (302, 300), 50, [0, 3, 0]);
    view::update_view(&mut s);
    let l = |s: &State, x, y| s.map.light[wcell(s, x, y)];
    // Leader sight 4 - 2 = 2 plus the light's own 4 - 0 - 1 = 3.
    assert_eq!(l(&s, 302, 300), 2 + 3);
    // Two cells past the light: sight 0, light 4 - 2 - 1 = 1; mirrored side stays dark.
    assert_eq!(l(&s, 304, 300), 1);
    assert_eq!(l(&s, 296, 300), 0);
    assert_eq!(s.map.view_flags[wcell(&s, 302, 300)] & CELL_LIGHT, 3);
}

#[test]
fn a_lit_wall_above_a_hidden_cell_stalls_the_rest_of_its_row() {
    // asm 0x10a15e -> 0x10a1c4: the light pass stops advancing its pointer on that row.
    let mut s = open_state(at(300, 300));
    set_ambient(&mut s, 0);
    let [ox, oy] = s.map.window_origin;
    let w = |x: u16, y: u16| (ox + x, oy + y);
    s.tables.tileflags.flags1[2] = TF1_WALL;
    let (ax, ay) = w(17, 26); // bottom row of the sight area; the cell below is never seen
    s.map.window_tiles[cell(17, 26)] = 2;
    object(&mut s, 300, (ax, ay), 60, [0, 3, 0]);
    let (bx, by) = w(21, 26);
    object(&mut s, 301, (bx, by), 61, [0, 3, 0]);
    let (dx, dy) = w(15, 26);
    object(&mut s, 302, (dx, dy), 62, [0, 3, 0]);
    view::update_view(&mut s);
    let l = |x: usize, y: usize| s.map.light[cell(x, y)];
    assert_eq!(l(15, 26), 3, "a light before the wall floods");
    assert_eq!(l(17, 26), 1, "the wall gets only its neighbour's light");
    assert_eq!(
        l(21, 26),
        0,
        "a light after the wall on the same row never floods"
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
    assert_eq!(s.map.light_at(5, 5), 4);
}

#[test]
fn the_border_ring_of_the_grids_is_never_cleared() {
    let mut s = open_state(at(300, 300));
    s.map.view_flags[cell(3, 3)] = 0x5a;
    s.map.light[cell(39, 0)] = 0x77;
    s.map.view_flags[cell(8, 8)] = 0x5a;
    view::update_view(&mut s);
    assert_eq!(s.map.view_flags[cell(3, 3)], 0x5a);
    assert_eq!(s.map.light[cell(39, 0)], 0x77);
    assert_eq!(s.map.view_flags[cell(8, 8)], 0, "inner block cleared");
}

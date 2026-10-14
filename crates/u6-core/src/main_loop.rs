//! Boot (`main()` 0x1351d8) and the command loop 0x1041d8 (WU-P7, last).
//!
//! Current content is a placeholder that makes the end-to-end path (zip → intake → `State` →
//! `Engine` → frame) visible: it runs the rest of 0x105350 as stub points, then draws the map
//! around the view centre with one flat colour per map tile (palette index = tile number % 32)
//! and moves the view with the arrow keys. The real map window is `gfx/`'s 0x10202e.
//!
//! Original functions this module will own (milestone 1):
//!
//! - 0x101ff6 [main] leaf; callers 2, from 2 subsystems
//! - 0x102bf2 [main] step of 0x102c90 (present/refresh map window [likel~)
//! - 0x102f2a [main] unnamed; calls 0x102ef0 (post-step refresh: rebuild vi~)
//! - 0x1032b6 [main] unnamed; calls 0x102f3c (per-turn party status effects~)
//! - 0x10348c [main] step of 0x1041d8 (command loop + A-Z key dispatcher~)
//! - 0x10355c [main] unnamed; calls 0x109e74 (view-window flags (blocking/l~)
//! - 0x103632 [main] unnamed; calls 0x151f06 (random number in [lo, hi])
//! - 0x103fd4 [main] unnamed; calls 0x108f90 (rebuild the map-window tile c~)
//! - 0x104094 [main] unnamed; 2 callees
//! - 0x104140 [main] step of 0x1041d8 (command loop + A-Z key dispatcher~)
//! - 0x1041d8 [main] command loop + A-Z key dispatcher, end-of-turn
//! - 0x1350d8 [boot] step of 0x1351d8 (main(): setup, load data, charact~)
//! - 0x135118 [boot] step of 0x1041d8 (command loop + A-Z key dispatcher~)
//! - 0x13511a [boot] step of 0x1351d8 (main(): setup, load data, charact~)
//! - 0x135120 [boot] unnamed; 1 callees
//! - 0x1351d8 [boot] main(): setup, load data, character check, music, command loop

use std::cell::RefCell;
use std::rc::Rc;

use u6_formats::position::Position;
use u6_render::{HEIGHT, Rgb, WIDTH};

use crate::engine::{Io, Key};
use crate::objects::{ACTOR_SLOTS, ObjectId};
use crate::state::State;
use crate::{Framebuffer, todo_stub};

/// Tile side in pixels.
const TILE: usize = 16;
/// Placeholder view: whole screen, 20 × 13 tiles (the last row is cut at 200 pixels).
const VIEW_COLS: usize = WIDTH / TILE;
const VIEW_ROWS: usize = HEIGHT.div_ceil(TILE);
/// Surface side in tiles; dungeon levels are 256.
const SURFACE_TILES: u16 = 1024;
const DUNGEON_TILES: u16 = 256;

/// Original: 0x1351d8 (main(): setup, load data, character check, music, command loop) [placeholder]
///
/// The engine already did steps 1-5 (platform setup, tables, `State::new_game`). This runs the
/// rest of the load-state tail, the music start and then the command loop.
pub async fn run(state: Rc<RefCell<State>>, io: Rc<Io>) {
    finish_load_state(&mut state.borrow_mut());
    // Stub contract: no result; the real call starts the area song (MED player, hunk 0). M1 has
    // no sound, so silence is the accepted behaviour; every boot reaches it.
    todo_stub!("0x110878 start area music (MED player)", ());
    command_loop(&state, &io).await;
}

/// The calls 0x105350 makes after its reads, in its order, as stub points until the owning
/// modules port them. Contract of every stub here: no arguments the port passes yet, result
/// `()`, the caller continues; every boot reaches all of them except 0x10cda8 (only when the
/// leader index equals the party size, not the case in the shipped objlist). Expected side
/// effects once ported, and what is visibly missing meanwhile:
///
/// - 0x10edb4 objects step [role not identified]; nothing visible known.
/// - 0x106506 ui step (called with 0x56aa) [role not identified]; nothing visible known.
/// - 0x10e1ce loads the object blocks of the leader's region into object slots 256..; until
///   then no map objects exist in the slot tables (they stay in `objects.blocks`).
/// - 0x108ac4 loads the map region around the party into the chunk cache; the port holds the
///   whole map, so nothing is missing.
/// - 0x108f90 rebuilds the map-window tile cache; the placeholder view reads tiles directly.
/// - 0x10cda8 per-member object step [role not identified].
/// - 0x109e74 computes the view flags (blocking / light / visible); until then `view_flags`
///   stays empty, so nothing is hidden or darkened.
fn finish_load_state(state: &mut State) {
    todo_stub!("0x10edb4 objects step of load state", ());
    todo_stub!("0x106506 ui step of load state", ());
    todo_stub!("0x10e1ce load object blocks for the region", ());
    todo_stub!("0x108ac4 load map region around the party", ());
    state.map.flag_784e = 1;
    todo_stub!("0x108f90 rebuild the map-window tile cache", ());
    let party = &state.actors.party;
    if party.leader_index == party.size {
        for _ in 0..party.size {
            todo_stub!("0x10cda8 per-member object step of load state", ());
        }
    }
    todo_stub!("0x109e74 view-window flags", ());
}

/// Original: 0x1041d8 (command loop + A-Z key dispatcher, end-of-turn) [placeholder]
///
/// Placeholder: draw, present, wait one display generation, wait for a key, move the view.
async fn command_loop(state: &RefCell<State>, io: &Io) {
    // WU-P1 wiring: tile graphics (0x152e50, boot step 2) until `State` carries `GfxState`; the
    // placeholder view stays only for files that do not load.
    let mut gfx = crate::gfx::GfxState::load(io.files()).ok();
    loop {
        match gfx.as_mut() {
            Some(gfx) => crate::gfx::refresh_view(&mut state.borrow_mut(), gfx, io),
            None => {
                todo_stub!("0x152e50 tile graphics not loaded (placeholder view)", ());
                io.draw(|fb| draw_placeholder_view(&state.borrow(), fb));
            }
        }
        io.present();
        io.vblank().await;
        let key = io.next_key().await;
        let centre = state.borrow().map.centre;
        state.borrow_mut().map.centre = step_view(centre, key);
    }
}

/// The view centre after an arrow key, kept on the map; other keys leave it.
fn step_view(at: Position, key: Key) -> Position {
    let side = if at.z == 0 {
        SURFACE_TILES
    } else {
        DUNGEON_TILES
    };
    let (dx, dy): (i32, i32) = match key {
        Key::Up => (0, -1),
        Key::Down => (0, 1),
        Key::Left => (-1, 0),
        Key::Right => (1, 0),
        _ => (0, 0),
    };
    let clamp = |v: u16, d: i32| (i32::from(v) + d).clamp(0, i32::from(side) - 1) as u16;
    Position {
        x: clamp(at.x, dx),
        y: clamp(at.y, dy),
        z: at.z,
    }
}

/// Palette index with the highest and the lowest r + g + b among the 32 game colours.
fn extremes(palette: &[u6_formats::gfx::Rgb]) -> (u8, u8) {
    let lum = |i: &usize| {
        let c = palette[*i];
        u16::from(c.r) + u16::from(c.g) + u16::from(c.b)
    };
    let bright = (0..palette.len()).max_by_key(lum).unwrap_or(0);
    let dark = (0..palette.len()).min_by_key(lum).unwrap_or(0);
    (bright as u8, dark as u8)
}

fn fill(fb: &mut Framebuffer, x: i32, y: i32, w: usize, h: usize, index: u8) {
    fb.blit(x, y, w, h, &vec![index; w * h], None);
}

/// Placeholder map view: map tiles as flat colours (tile % 32), actors on the level as small
/// squares (party members dark with a bright rim), the leader's tile outlined.
fn draw_placeholder_view(state: &State, fb: &mut Framebuffer) {
    for (dst, c) in fb.palette.iter_mut().zip(&state.tables.palette) {
        *dst = Rgb::new(c.r, c.g, c.b);
    }
    let (bright, dark) = extremes(&state.tables.palette);
    fb.clear(0);
    let c = state.map.centre;
    let x0 = i32::from(c.x) - (VIEW_COLS / 2) as i32;
    let y0 = i32::from(c.y) - (VIEW_ROWS / 2) as i32;
    let screen = |x: u16, y: u16| {
        let (col, row) = (i32::from(x) - x0, i32::from(y) - y0);
        let inside = (0..VIEW_COLS as i32).contains(&col) && (0..VIEW_ROWS as i32).contains(&row);
        inside.then(|| (col * TILE as i32, row * TILE as i32))
    };
    for row in 0..VIEW_ROWS as i32 {
        for col in 0..VIEW_COLS as i32 {
            let (x, y) = (x0 + col, y0 + row);
            if x < 0 || y < 0 {
                continue;
            }
            if let Some(tile) = state.map.tile_at(x as usize, y as usize, usize::from(c.z)) {
                let (px, py) = (col * TILE as i32, row * TILE as i32);
                fill(fb, px, py, TILE, TILE, tile % 32);
            }
        }
    }
    let party: Vec<ObjectId> = state.actors.party.active().collect();
    for a in 1..ACTOR_SLOTS as u16 {
        let id = ObjectId(a);
        let Some(p) = state.objects.location(id) else {
            continue;
        };
        if state.objects.obj_type(id) == 0 || p.z != c.z {
            continue;
        }
        let Some((px, py)) = screen(p.x, p.y) else {
            continue;
        };
        if party.contains(&id) {
            fill(fb, px + 3, py + 3, 10, 10, bright);
            fill(fb, px + 5, py + 5, 6, 6, dark);
        } else {
            fill(fb, px + 5, py + 5, 6, 6, bright);
        }
    }
    if let Some((px, py)) = state.leader_position().and_then(|p| screen(p.x, p.y)) {
        fill(fb, px, py, TILE, 1, bright);
        fill(fb, px, py + TILE as i32 - 1, TILE, 1, bright);
        fill(fb, px, py, 1, TILE, bright);
        fill(fb, px + TILE as i32 - 1, py, 1, TILE, bright);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use u6_formats::chunks::Chunks;
    use u6_formats::map::{SURFACE_CHUNKS, WorldMap};

    /// Surface of chunk 1 everywhere; chunk 1 tile (col, row) = row * 8 + col.
    fn world_state() -> State {
        let mut s = State::empty();
        s.map.world = WorldMap {
            surface: vec![1; SURFACE_CHUNKS * SURFACE_CHUNKS],
            dungeons: Vec::new(),
        };
        let mut chunks = vec![[[0u8; 8]; 8]; 2];
        for (r, row) in chunks[1].iter_mut().enumerate() {
            for (c, t) in row.iter_mut().enumerate() {
                *t = (r * 8 + c) as u8;
            }
        }
        s.map.chunks = Chunks { chunks };
        for (i, c) in s.tables.palette.iter_mut().enumerate() {
            let v = (i * 8) as u8;
            *c = u6_formats::gfx::Rgb { r: v, g: v, b: v };
        }
        s
    }

    #[test]
    fn view_steps_with_arrows_and_stays_on_the_map() {
        let p = |x, y, z| Position { x, y, z };
        assert_eq!(step_view(p(5, 5, 0), Key::Up), p(5, 4, 0));
        assert_eq!(step_view(p(5, 5, 0), Key::Right), p(6, 5, 0));
        assert_eq!(step_view(p(0, 0, 0), Key::Left), p(0, 0, 0));
        assert_eq!(step_view(p(1023, 9, 0), Key::Right), p(1023, 9, 0));
        assert_eq!(step_view(p(255, 9, 2), Key::Right), p(255, 9, 2));
        assert_eq!(step_view(p(5, 5, 0), Key::Char('a')), p(5, 5, 0));
    }

    #[test]
    fn placeholder_view_colours_tiles_and_marks_the_leader() {
        let mut s = world_state();
        s.map.centre = Position { x: 19, y: 10, z: 0 };
        s.objects.location[1] = s.map.centre;
        s.objects.type_frame[1] = 0x150;
        s.actors.party.members[0] = 1;
        s.actors.party.size = 1;
        let mut fb = Framebuffer::new();
        draw_placeholder_view(&s, &mut fb);
        // Top-left tile is world (9, 4): chunk tile row 4, col 1 = 33 -> index 1.
        assert_eq!(fb.get(1, 1), Some(33 % 32));
        // Tile right of the leader is world (20, 10): row 2, col 4 = 20.
        assert_eq!(fb.get(176 + 8, 96 + 8), Some(20));
        // Leader at the view centre (col 10, row 6): outline and party square in the
        // brightest colour (31), dark core (0).
        assert_eq!(fb.get(160, 96), Some(31));
        assert_eq!(fb.get(160 + 4, 96 + 4), Some(31));
        assert_eq!(fb.get(160 + 8, 96 + 8), Some(0));
        assert_eq!(fb.palette[31], Rgb::new(248, 248, 248));
    }
}

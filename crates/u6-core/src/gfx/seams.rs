//! What gfx needs from other modules and from the engine, behind the narrowest signatures
//! (03 §6). Each seam names its producer; the dispatcher replaces the fallback at merge.

use u6_formats::objblk::ObjLocation;

use super::GfxState;
#[cfg(doc)]
use super::blit;
use super::view::{ViewWindow, WINDOW_TILES};
use crate::engine::Io;
use crate::objects::{ACTOR_SLOTS, ObjectId};
use crate::state::State;
use crate::todo_stub;

/// The window contents as last built by map 0x109e74 (tile cache A4-0x1476: map tile per cell,
/// 0xff for an unseen cell, 0x1bc for an unlit one, wall shapes fixed up from their neighbours)
/// and objects 0x10e81e → 0x10e73a / 0x10e596 (per-cell object lists, multi-tile parts, light
/// fringe tiles 0x1bd..0x1bf). Owners: map (WU-P2) and objects (WU-P3).
///
/// Fallback (until they land): every cell visible and lit, the raw map tile at
/// `centre - 5 + (col, row)` (coordinates wrap at the map size like 0x10996c's mask), and the
/// objects of [`fallback_objects`].
pub(crate) fn view_window(state: &State) -> ViewWindow {
    let tiles = todo_stub!("0x109e74 view-window flags", fallback_tiles(state));
    let mut window = ViewWindow {
        tiles,
        ..ViewWindow::empty()
    };
    todo_stub!(
        "0x10e81e window object lists",
        fallback_objects(state, &mut window)
    );
    window
}

/// Map size in tiles on level `z` (surface 1024, dungeons 256).
fn map_side(z: u8) -> i32 {
    if z == 0 { 1024 } else { 256 }
}

fn fallback_tiles(state: &State) -> [[u16; WINDOW_TILES]; WINDOW_TILES] {
    let c = state.map.centre;
    let side = map_side(c.z);
    let mut tiles = [[0u16; WINDOW_TILES]; WINDOW_TILES];
    for (row, line) in tiles.iter_mut().enumerate() {
        for (col, t) in line.iter_mut().enumerate() {
            let x = (i32::from(c.x) - 5 + col as i32).rem_euclid(side) as usize;
            let y = (i32::from(c.y) - 5 + row as i32).rem_euclid(side) as usize;
            *t = state
                .map
                .tile_at(x, y, usize::from(c.z))
                .map_or(0, u16::from);
        }
    }
    tiles
}

/// Fallback for 0x10e81e: actors 0..256 with a type, then the map objects of `objects.blocks`
/// (0x10e1ce has not put them into slots yet) whose location is the map, each on the centre's
/// level in cells 0..=11 (`x - centre_x + 5`), in that order [추정: the original walks its region
/// list 0x10b05a / 0x10af74; actors first is what puts people over carpets under the head
/// insertion of [`insert`]; the order of two map objects in one cell is not verified]. Tile =
/// `basetile[type] + frame`. As in 0x10e81e: an object with flag bit 1 (invisible) is skipped
/// unless it is an actor with status (A4+0x3d0c) bit 7; an actor with movement flag 0x10 is
/// skipped; an actor gets an outline first (its word plus 0x800 when invisible and below type
/// 0x1aa, plus 0x1000 with status bit 0, plus 0x1800 with flag 0x40), then its sprite unless
/// invisible. Each
/// goes through [`place`] (0x10e73a). Not reproduced: the 0x7b frame fix-up, the 0x18a / 0x18c
/// cases, the light fringe tiles 0x1bd..0x1bf, the -0x7af6 / -0x78e0 bookkeeping.
fn fallback_objects(state: &State, window: &mut ViewWindow) {
    let c = state.map.centre;
    let base = &state.tables.basetile.base;
    let cell = |x: u16, y: u16| {
        let (cx, cy) = (
            i32::from(x) - i32::from(c.x) + 5,
            i32::from(y) - i32::from(c.y) + 5,
        );
        ((0..=11).contains(&cx) && (0..=11).contains(&cy)).then_some((cx, cy))
    };
    let tile = |t: u16, frame: u8| base.get(usize::from(t)).map(|b| b + u16::from(frame));
    let mut nodes = 0usize;
    for a in 0..ACTOR_SLOTS as u16 {
        let id = ObjectId(a);
        let (ty, Some(p)) = (state.objects.obj_type(id), state.objects.location(id)) else {
            continue;
        };
        if ty == 0 || p.z != c.z {
            continue;
        }
        let (Some((cx, cy)), Some(t)) = (cell(p.x, p.y), tile(ty, state.objects.frame(id))) else {
            continue;
        };
        let flags = state.objects.actor_flags[usize::from(a)];
        let status = actor_status(state, id);
        if (flags & 2 != 0 && status & 0x80 == 0) || movement_flags(state, id) & 0x10 != 0 {
            continue;
        }
        let mut bits = 0;
        if flags & 2 != 0 && ty < 0x1aa {
            bits = 0x800;
        }
        if status & 1 != 0 {
            bits = 0x1000;
        }
        if flags & 0x40 != 0 {
            bits = 0x1800;
        }
        if bits != 0 {
            place(state, window, &mut nodes, t.wrapping_add(bits), cx, cy);
        }
        if flags & 2 == 0 {
            place(state, window, &mut nodes, t, cx, cy);
        }
    }
    for (_, block) in &state.objects.blocks {
        for r in &block.objects {
            if r.location() != ObjLocation::Map || r.pos.z != c.z {
                continue;
            }
            let (Some((cx, cy)), Some(t)) = (cell(r.pos.x, r.pos.y), tile(r.obj_n, r.frame)) else {
                continue;
            };
            if r.status & 2 == 0 {
                place(state, window, &mut nodes, t, cx, cy);
            }
        }
    }
}

/// 0x10e73a: the object's word at its cell, and for a wide tile (flags2 bit 7) `word - 1` to the
/// left, for a tall one (bit 6) `word - 1` above, for both `word - 2` above and `word - 3`
/// above-left; parts outside cells 0..11 are dropped. Parts are inserted as kind 1.
fn place(state: &State, window: &mut ViewWindow, nodes: &mut usize, word: u16, cx: i32, cy: i32) {
    let f = flags2(state, word);
    let mut put = |w: u16, x: i32, y: i32, part: bool| insert(state, window, nodes, w, x, y, part);
    if cx < 11 && cy < 11 {
        put(word, cx, cy, false);
    }
    if f & 0x80 != 0 {
        if cx != 0 && cy < 11 {
            put(word.wrapping_sub(1), cx - 1, cy, true);
        }
        if f & 0x40 != 0 {
            if cx < 11 && cy != 0 {
                put(word.wrapping_sub(2), cx, cy - 1, true);
            }
            if cx != 0 && cy != 0 {
                put(word.wrapping_sub(3), cx - 1, cy - 1, true);
            }
        }
    } else if f & 0x40 != 0 && cx < 11 && cy != 0 {
        put(word.wrapping_sub(1), cx, cy - 1, true);
    }
}

fn flags2(state: &State, word: u16) -> u8 {
    let flags = &state.tables.tileflags.flags2;
    flags.get(usize::from(word & 0x7ff)).copied().unwrap_or(0)
}

/// Object list nodes of 0x10e81e (bytes 1..=255 at A4-0x1577; node 0 ends a list).
const LIST_NODES: usize = 255;

/// 0x10e596 for a cell whose cache tile is a normal map tile (the fallback never makes unseen
/// 0xff or unlit 0x1bc cells): the basetile of type 0x14f (0x13abde) is never listed; a tile
/// with flags3 bit 5 replaces the cache tile; otherwise, while list nodes are left, a tile without
/// flags2 bit 4 goes to the head of the cell's list (drawn first), a main tile with it goes in
/// front of the first listed tile with it, a part with it to the end (A4-0x7b71 taken as 0).
#[allow(clippy::too_many_arguments)]
fn insert(
    state: &State,
    window: &mut ViewWindow,
    nodes: &mut usize,
    word: u16,
    cx: i32,
    cy: i32,
    part: bool,
) {
    let (Ok(x), Ok(y)) = (usize::try_from(cx), usize::try_from(cy)) else {
        return;
    };
    if x >= WINDOW_TILES || y >= WINDOW_TILES {
        return;
    }
    let t = usize::from(word & 0x7ff);
    if state.tables.basetile.base.get(0x14f).copied() == Some(word & 0x7ff) {
        return;
    }
    if state.tables.tileflags.flags3.get(t).copied().unwrap_or(0) & 0x20 != 0 {
        window.tiles[y][x] = word;
        return;
    }
    if *nodes >= LIST_NODES {
        return;
    }
    *nodes += 1;
    let list = &mut window.objects[y * WINDOW_TILES + x];
    let at = if flags2(state, word) & 0x10 == 0 {
        0
    } else if part {
        list.len()
    } else {
        list.iter()
            .position(|&w| flags2(state, w) & 0x10 != 0)
            .unwrap_or(list.len())
    };
    list.insert(at, word);
}

/// Animation flag of animdata slot `slot` (A4-0x309b, byte per slot: 0 off, 1 forward,
/// 2 backward). Owner: map ([`crate::map::MapState::anim_enabled`]); it stores a `bool`, so the
/// value 2 cannot be represented yet and reads as 1.
pub(crate) fn anim_mode(state: &State, slot: usize) -> u8 {
    state
        .map
        .anim_enabled
        .get(slot)
        .map_or(0, |&on| u8::from(on))
}

/// 0x102c90's target cursor: when the ui is idle (A4-0x7a32 == 0, A4-0x7a5b == 0) it calls
/// 0x106932(0) and clears A4-0x7a60; when A4-0x7a60 == 1 it draws tile 0x16c (A4-0x7a5d < 0) or
/// 0x16d at (16·A4-0x7a5f, 16·A4-0x7a5e). Returns that (tile, x, y). Owner: ui (WU-P4).
/// Fallback: no cursor.
pub(crate) fn map_cursor(_state: &mut State) -> Option<(u16, u16, u16)> {
    todo_stub!("0x106932 map-window target cursor", None)
}

/// 0x109e74 when 0x102ef0 finds the view flags stale. Owner: map (WU-P2). Fallback: nothing;
/// [`view_window`] recomputes the window on every draw.
pub(crate) fn view_flags(_state: &mut State) {
    todo_stub!("0x109e74 view-window flags", ());
}

/// 0x133a1a(`panel`, 1): redraw a side panel after the window. Owner: ui (WU-P4). Fallback:
/// nothing.
pub(crate) fn redraw_panel(_state: &mut State, _panel: u8) {
    todo_stub!("0x133a1a redraw a side panel", ());
}

/// Present the map window: [`blit::dissolve`] over (8, 8)..(0xa7, 0xa7) when `dissolve`, else
/// [`blit::copy_window`], from the back buffer into the front. Owner: engine (WU-C0): `Io` lets
/// the game draw only into the back buffer and copy all of it; these copies need both buffers
/// (e.g. `Io::with_buffers(|back, front| ..)`). Fallback: the whole back buffer is presented,
/// the same picture as long as nothing draws into the front directly.
pub(crate) fn present_window(io: &Io, _gfx: &GfxState, dissolve: bool) {
    if dissolve {
        todo_stub!(
            "0x1520e2 dissolve the window to the front buffer",
            io.present()
        );
    } else {
        todo_stub!("0x151f5a copy the window to the front buffer", io.present());
    }
}

/// Original 0x10ad1a + 0x10aaca: the objects at (x, y, z) in the original's chain order
/// (0x10aaca also returns multi-tile objects whose other parts cover the location). Owner:
/// objects (WU-P3). Fallback: every slot whose location is exactly (x, y, z) and whose type/frame
/// word is not 0, in slot order; actor slots only with actor flags bits 3-4 clear (on the map).
pub(crate) fn objects_at(state: &State, x: i32, y: i32, z: u8) -> Vec<ObjectId> {
    todo_stub!(
        "0x10ad1a objects at a location",
        (0..state.objects.location.len() as u16)
            .map(ObjectId)
            .filter(|&id| {
                let p = state.objects.location[id.index()];
                let flags = state
                    .objects
                    .actor_flags
                    .get(id.index())
                    .copied()
                    .unwrap_or(0);
                (i32::from(p.x), i32::from(p.y), p.z) == (x, y, z)
                    && state.objects.type_frame[id.index()] != 0
                    && flags & 0x18 == 0
            })
            .collect()
    )
}

/// Original 0x10b922(x, y, z, dir, frame): the body part of type 0x19b at (x, y, z) whose frame
/// & 0x3e matches the byte A4-0x6a70[(frame & 6) * 2 + dir / 2] [from the decompile; arguments
/// not checked in the assembly]. Owner: objects (WU-P3). Fallback: none found (the 0x19b case
/// then draws one RNG value less per part).
pub(crate) fn serpent_part(
    _state: &State,
    _x: i32,
    _y: i32,
    _z: u8,
    _dir: u16,
    _frame: u16,
) -> Option<ObjectId> {
    todo_stub!("0x10b922 serpent body part", None)
}

/// Original 0x10a5b0(id): the other half of a two-tile creature (types above 0x1a9, 0x19b,
/// 0x19c), found in the direction its tile flags1 high nibble names. Owner: objects (WU-P3).
/// Fallback: none (the other half keeps its frame).
pub(crate) fn other_half(_state: &State, _id: ObjectId) -> Option<ObjectId> {
    todo_stub!("0x10a5b0 other half of a two-tile creature", None)
}

/// Original 0x10e81e: rebuild the window object lists after frames changed. Owner: objects
/// (WU-P3). Fallback: nothing; [`view_window`] rebuilds the lists on every draw.
pub(crate) fn rebuild_window_objects(_state: &mut State) {
    todo_stub!("0x10e81e window object lists", ());
}

/// Type/frame word of `id` (0x137988; objects owns the table).
pub(crate) fn type_frame(state: &State, id: ObjectId) -> u16 {
    state
        .objects
        .type_frame
        .get(id.index())
        .copied()
        .unwrap_or(0)
}

/// Writes the type/frame word of `id` (0x10202e writes the table directly, as here).
pub(crate) fn set_type_frame(state: &mut State, id: ObjectId, word: u16) {
    if let Some(w) = state.objects.type_frame.get_mut(id.index()) {
        *w = word;
    }
}

/// Status byte of actor `id` (A4+0x3d0c; actors owns it).
pub(crate) fn actor_status(state: &State, id: ObjectId) -> u8 {
    state
        .actors
        .attrs
        .status
        .get(id.index())
        .copied()
        .unwrap_or(0)
}

/// Movement-flags byte of actor `id` (0x139588; actors owns it).
pub(crate) fn movement_flags(state: &State, id: ObjectId) -> u8 {
    state
        .actors
        .attrs
        .movement_flags
        .get(id.index())
        .copied()
        .unwrap_or(0)
}

/// Writes the movement-flags byte of actor `id` (0x10202e writes bit 7 directly, as here).
pub(crate) fn set_movement_flags(state: &mut State, id: ObjectId, flags: u8) {
    if let Some(f) = state.actors.attrs.movement_flags.get_mut(id.index()) {
        *f = flags;
    }
}

/// Worktype byte of actor `id` (0x139188; actors owns it).
pub(crate) fn worktype(state: &State, id: ObjectId) -> u8 {
    state.actors.worktype.get(id.index()).copied().unwrap_or(0)
}

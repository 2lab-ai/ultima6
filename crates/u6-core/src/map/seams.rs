//! Calls from `map/` into routines other units own, with the narrowest signature `map/` needs.
//!
//! Each stub states its contract (arguments, result, side effects of the real routine), the
//! fallback and whether a milestone-1 boot reaches it. The dispatcher replaces the bodies with
//! the owners' functions at merge; the signatures are the request.

use crate::objects::{OBJECT_SLOTS, ObjectId};
use crate::state::State;
use crate::todo_stub;

/// Status byte of object `id` (A4+0x190c, byte per object: 0x18 = not on the map, 0x20 = temporary,
/// 0x40 = egg active, ...). `objects/` holds it for the 256 actor slots so far; other slots read 0
/// until it covers all slots.
pub(crate) fn object_status(state: &State, id: ObjectId) -> u8 {
    state
        .objects
        .actor_flags
        .get(id.index())
        .copied()
        .unwrap_or(0)
}

/// Ambient light (A4-0x7b79, signed byte; 0x105350 loads it with the objlist block at A4-0x7b84,
/// objlist 0x1BFC; the clock 0x10368a rewrites it: 0 at night and on levels 1..4, 1..7 at dawn
/// and dusk, 7 by day). Read from that block until `actors/` names the field.
pub(crate) fn ambient_light(state: &State) -> i8 {
    state.actors.objlist_tail.get(0x0b).map_or(0, |&b| b as i8)
}

/// Objects in the rectangle x0..=x1, y0..=y1 on level `z`, in iteration order (owner `objects/`:
/// 0x10b05a begins the walk, x0 / y0 clamped at 0, at the leader's level; 0x10af74 yields the
/// next object of the region list on the map (status & 0x18 = 0) whose masked x, y and z match).
/// Reached by every view update (0x109e74) and window rebuild (0x1321dc).
///
/// Fallback: the same filter over the object slots in index order, skipping empty slots. Until
/// 0x10e1ce places the region's object blocks into slots, that finds actors only; the view then
/// marks no doors, windows or lights carried by objects. The caller takes a snapshot; the
/// original walks lazily, so an object the walk's own callee adds may or may not be visited.
pub(crate) fn objects_in_rect(
    state: &State,
    x0: i16,
    y0: i16,
    x1: i16,
    y1: i16,
    z: u16,
) -> Vec<ObjectId> {
    let (x0, y0) = (x0.max(0), y0.max(0));
    let mask = state.map.world_mask;
    let scan = || {
        (0..OBJECT_SLOTS as u16)
            .map(ObjectId)
            .filter(|&id| {
                let Some(p) = state.objects.location(id) else {
                    return false;
                };
                let (x, y) = ((p.x & mask) as i16, (p.y & mask) as i16);
                state.objects.type_frame[id.index()] != 0
                    && object_status(state, id) & 0x18 == 0
                    && (x0..=x1).contains(&x)
                    && (y0..=y1).contains(&y)
                    && u16::from(p.z) == z
            })
            .collect()
    };
    todo_stub!("0x10b05a objects in a rectangle (0x10af74 walk)", scan())
}

/// Rebuild the window object grid A4+0x67c (40 × 40 words: first object on each window cell, -1
/// = none) from the region list (owner `objects/`, 0x10dea4). Reached by every window rebuild.
/// Fallback: the grid is not kept.
pub(crate) fn rebuild_object_grid(_state: &mut State) {
    todo_stub!("0x10dea4 rebuild the window object grid", ())
}

/// Object in window cell (wx, wy) of the grid A4+0x67c, -1 = none (owner `objects/`, built by
/// 0x10dea4). Read by 0x108d10 for each actor in the window. Fallback: -1, so every such actor
/// is offered to [`move_object`].
pub(crate) fn object_grid_cell(_state: &State, _wx: usize, _wy: usize) -> i16 {
    todo_stub!("0x10dea4 window object grid cell (A4+0x67c)", -1)
}

/// Move object `id` to (x, y, z), updating the object grid and the "view changed" flag (owner
/// `objects/`, 0x10c9d2). 0x108d10 re-places actors at their own location. Fallback: nothing
/// (the location tables already hold the position).
pub(crate) fn move_object(_state: &mut State, _id: ObjectId, _x: u16, _y: u16, _z: u16) {
    todo_stub!("0x10c9d2 move an object to a map location", ())
}

/// Remove temporary objects (status 0x20) farther than 20 tiles from (x, y) or on another level
/// than `z`, and clear the active bit of such eggs (owner `objects/`, 0x10c49c). Reached by
/// every window rebuild. Fallback: nothing removed.
pub(crate) fn drop_far_temporaries(_state: &mut State, _x: u16, _y: u16, _z: u16) {
    todo_stub!("0x10c49c remove far temporary objects", ())
}

/// Show or hide the moongates at the 8 moonstone locations inside the window by the moon phases,
/// then update the view if one changed (owner `main_loop`, 0x10355c). Reached by every window
/// rebuild. Fallback: no moongates.
pub(crate) fn update_moongates(_state: &mut State) {
    todo_stub!("0x10355c moongates in the window", ())
}

/// Load the object blocks of the region around (x, y, z) into the object slots (owner `objects/`,
/// 0x10e1ce). Reached by every region refresh (0x108f90). Fallback: no map objects in slots.
pub(crate) fn load_region_objects(_state: &mut State, _x: u16, _y: u16, _z: u16) {
    todo_stub!("0x10e1ce load object blocks for the region", ())
}

/// Build the per-view-cell object draw lists (A4-0x15f0, 11 × 11) for 0x102c90; draws RNG
/// (owner `objects/`, 0x10e81e). Reached by every view update. Fallback: no objects drawn.
pub(crate) fn build_view_objects(_state: &mut State) {
    todo_stub!("0x10e81e build the view object lists", ())
}

/// Offer egg `id` to hatch (owner `objects/`, 0x1319b2: may create monsters). Reached when an
/// egg is in the window. Fallback: nothing hatches.
pub(crate) fn hatch_egg(_state: &mut State, _id: ObjectId) {
    todo_stub!("0x1319b2 hatch an egg", ())
}

//! Calls from `map/` into routines other units own, with the narrowest signature `map/` needs.
//!
//! Each stub states its contract (arguments, result, side effects of the real routine), the
//! fallback and whether a milestone-1 boot reaches it. The dispatcher replaces the bodies with
//! the owners' functions at merge; the signatures are the request.

use u6_formats::position::Position;

use crate::objects::{ACTOR_SLOTS, OBJECT_SLOTS, ObjectId};
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

/// Move object `id` to (x, y, z), updating the region list, the object grid and the "view
/// changed" flag (owner `objects/`, 0x10c9d2). 0x108d10 re-places actors at their own location;
/// the party moves put members on the centre or park them at z = 15. Fallback: only the
/// location table is written, so the party follows the centre.
pub(crate) fn move_object(state: &mut State, id: ObjectId, x: u16, y: u16, z: u16) {
    todo_stub!("0x10c9d2 move an object to a map location", ());
    if let Some(p) = state.objects.location.get_mut(id.index()) {
        *p = Position {
            x: x & 0x3ff,
            y: y & 0x3ff,
            z: (z & 0xf) as u8,
        };
    }
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

/// A byte of the objlist block A4-0x7b84 (`actors.objlist_tail`, objlist 0x1BF1..0x1C73) that
/// `actors/` has not named yet; 0 past it. Offsets used here: 0x0b ambient light (A4-0x7b79),
/// 0x78 combat mode (A4-0x7b0c), 0x79 solo member, 0xff = party mode (A4-0x7b0b), 0x7c..0x7f
/// moon state (A4-0x7b08..-0x7b05: stone of moon 1, phase 1, stone of moon 2, phase 2).
pub(crate) fn tail_byte(state: &State, off: usize) -> u8 {
    state.actors.objlist_tail.get(off).copied().unwrap_or(0)
}

/// Moonstone `n` (0..8) location: A4-0x7b5a + 6n, three big-endian words x, y, z (objlist
/// 0x1C1B). Read from the objlist block until `actors/` names it.
pub(crate) fn moonstone(state: &State, n: u8) -> [u16; 3] {
    let at = 0x2a + usize::from(n) * 6;
    let w = |i: usize| u16::from_be_bytes([tail_byte(state, at + i), tail_byte(state, at + i + 1)]);
    [w(0), w(2), w(4)]
}

/// Quality byte of object `id` (the high byte of the word at A4+0x250c + 2 id: a ladder's
/// destination bits, a moongate's destination, a dungeon entrance's name; `objects.qq`).
pub(crate) fn object_quality(state: &State, id: ObjectId) -> u8 {
    (state.objects.qq_of(id.slot()) >> 8) as u8
}

/// Objects at (x, y, z) in iteration order, each with the stacking word A4-0x78dc the walk
/// leaves while yielding it (owner `objects/`: 0x10ad1a begins, 0x10aaca steps). Reached on every
/// player step (0x11d1d4). Fallback: the object slots at that location in index order, word 0.
pub(crate) fn objects_at(state: &State, x: u16, y: u16, z: u16) -> Vec<(ObjectId, u16)> {
    let scan = || {
        (0..OBJECT_SLOTS as u16)
            .map(ObjectId)
            .filter(|&id| {
                let p = state.objects.location(id);
                state.objects.type_frame[id.index()] != 0
                    && object_status(state, id) & 0x18 == 0
                    && p.is_some_and(|p| (p.x, p.y, u16::from(p.z)) == (x, y, z))
            })
            .map(|id| (id, 0))
            .collect()
    };
    todo_stub!("0x10ad1a objects at a location (0x10aaca walk)", scan())
}

/// First object of type `ty` at (x, y) on the leader's level (owner `objects/`, 0x10ad66).
/// Fallback: the slot scan of [`objects_at`].
pub(crate) fn find_object_at(state: &State, ty: u16, x: u16, y: u16) -> Option<ObjectId> {
    let z = u16::from(state.map.centre.z);
    let found = objects_at(state, x, y, z)
        .into_iter()
        .map(|(id, _)| id)
        .find(|&id| state.objects.obj_type(id) == ty);
    todo_stub!("0x10ad66 find an object of a type at a location", found)
}

/// Remove object `id` from the map (owner `objects/`, 0x10c1d6). Fallback: nothing removed.
pub(crate) fn remove_object(_state: &mut State, _id: ObjectId) {
    todo_stub!("0x10c1d6 remove an object from the map", ())
}

/// Advance the clock (owner `actors/`, 0x10368a; the argument word is passed in D0: 0 from the
/// teleports, 0x0a after the midnight moongate, 0x0f per step of the resurrection wait; its
/// meaning is the owner's). Fallback: the clock stands still, so 0x1095bc's wait for morning
/// gives up (see [`super::travel::resurrect_party`]).
pub(crate) fn clock_tick(_state: &mut State, _arg: u16) {
    todo_stub!("0x10368a advance the clock", ())
}

/// UI step after a teleport (owner `ui/`, 0x133b28; draws with the tile blitter). Fallback:
/// nothing drawn.
pub(crate) fn ui_after_teleport(_state: &mut State) {
    todo_stub!("0x133b28 ui step after a teleport", ())
}

/// Main-loop step after the party is placed (owner `main_loop`, 0x104094). Fallback: nothing.
pub(crate) fn main_after_placing(_state: &mut State) {
    todo_stub!("0x104094 main step after placing the party", ())
}

/// Post-step refresh: rebuild the view if dirty and redraw (owner `gfx/`, 0x102ef0). The party
/// moves call it between the object lists (0x10e81e) and the next step, so each step is drawn.
/// Fallback: nothing drawn.
pub(crate) fn refresh_view(_state: &mut State) {
    todo_stub!("0x102ef0 post-step view refresh", ())
}

/// Party member `id` follows the leader once (owner `actors/` party code, 0x11c910; D1 = 1 from
/// 0x10921a). Fallback: the member stays where 0x10921a put it (on the leader's tile).
pub(crate) fn party_follow(_state: &mut State, _id: ObjectId) {
    todo_stub!("0x11c910 party member follows the leader", ())
}

/// Step actor `id` toward (x, y); nonzero when it moved (owner `actors/`, 0x11f452; draws RNG).
/// Fallback: 0 (did not move).
pub(crate) fn step_toward(_state: &mut State, _id: ObjectId, _x: u16, _y: u16) -> i16 {
    todo_stub!("0x11f452 step an actor toward a location", 0)
}

/// Distance from actor `id` to (x, y) (owner: combat leaf 0x121b08, an M1 stub point; uses
/// `abs`). Fallback: 0, so a gathering member joins the leader at once (0x108fba).
pub(crate) fn distance(_state: &State, _id: ObjectId, _x: u16, _y: u16) -> i16 {
    todo_stub!("0x121b08 distance from an actor to a location", 0)
}

/// Walk actor `id` one step in direction `dir` (0, 2, 4, 6) with flag 1 (owner `actors/`,
/// 0x11f0a0). Fallback: no step.
pub(crate) fn walk(_state: &mut State, _id: ObjectId, _dir: u16) {
    todo_stub!("0x11f0a0 walk an actor one step", ())
}

/// The routine 0x108fba runs for a party member whose type is 0x1af before leaving (owner
/// `objects/`, 0x12f8fe, uses "No room!"). Fallback: nothing.
pub(crate) fn before_leaving(_state: &mut State, _id: ObjectId) {
    todo_stub!("0x12f8fe party member of type 0x1af leaves", ())
}

/// End combat mode (0x121d78, an M1 stub point: the B command handler). Fallback: nothing; M1
/// starts out of combat.
pub(crate) fn break_off_combat(_state: &mut State) {
    todo_stub!("0x121d78 break off combat", ())
}

/// Start the song for the current area (0x110912, an M1 stub point). Fallback: silence.
pub(crate) fn play_area_song(_state: &mut State) {
    todo_stub!("0x110912 pick and play the area song", ())
}

/// Play song `n` (0x110878, an M1 stub point). Fallback: silence.
pub(crate) fn play_song(_state: &mut State, _n: u16) {
    todo_stub!("0x110878 play a song", ())
}

/// Print the place a dungeon entrance of quality `q` (1..=20) leads to, as Shamino's line
/// (owner `ui/`: printf 0x108658 of the string at A4-0x7303, then 0x11b8f8 "dungeon " for
/// q < 8 or 0x11b902 "shrine of " for q 9..=11, then the name the pointer table A4-0x6a60 + 4q
/// points at (strings in hunk 12), then 0x11b90e; all to be read from the user's `game`).
/// Fallback: no message.
pub(crate) fn say_place_name(_state: &mut State, _q: u8) {
    todo_stub!("0x108658 print Shamino's place name (strings in game)", ())
}

/// Print A4-0x72cf, "You must be in party mode to enter." (owner `ui/`, printf 0x108658; the
/// string has to be read from the user's `game`). Fallback: no message.
pub(crate) fn say_party_mode_needed(_state: &mut State) {
    todo_stub!("0x108658 print the party-mode message (string in game)", ())
}

/// Maximum hit points of actor `id` (combat leaf 0x122144, ported with the actor model).
/// Fallback: the current hit points.
pub(crate) fn max_hp(state: &State, id: ObjectId) -> u8 {
    let hp = state.actors.attrs.hp.get(id.index()).copied().unwrap_or(0);
    todo_stub!("0x122144 maximum hit points", hp)
}

/// Scan for nearby hostiles and toggle the combat state (owner `actors/`, 0x120a1c). Fallback:
/// nothing.
pub(crate) fn scan_hostiles(_state: &mut State) {
    todo_stub!("0x120a1c scan nearby hostiles", ())
}

/// Put the Avatar back from its body object (owner `objects/`, 0x10c810). Fallback: nothing.
pub(crate) fn raise_avatar_body(_state: &mut State, _body: ObjectId) {
    todo_stub!("0x10c810 raise the Avatar from its body", ())
}

/// Resurrect the companion lying as body object `body` (0x11785e, an M1 stub point in the
/// magic code). Fallback: nothing.
pub(crate) fn resurrect_body(_state: &mut State, _body: ObjectId) {
    todo_stub!("0x11785e resurrect a companion's body", ())
}

/// Objects of the region list (A4-0xf84 head, A4-0x1184 next links) in list order (owner
/// `objects/`). Fallback: every non-empty object slot from 256 up.
pub(crate) fn region_list(state: &State) -> Vec<ObjectId> {
    let scan = || {
        (ACTOR_SLOTS as u16..OBJECT_SLOTS as u16)
            .map(ObjectId)
            .filter(|&id| state.objects.type_frame[id.index()] != 0)
            .collect()
    };
    todo_stub!(
        "0x10d376 the region object list (A4-0xf84 / A4-0x1184)",
        scan()
    )
}

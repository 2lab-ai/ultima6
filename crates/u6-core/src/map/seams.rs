//! Calls from `map/` into routines other units own, with the narrowest signature `map/` needs.
//!
//! Since the integration (WU-P7) each forwards to its owner; the targets outside milestone 1
//! (eggs, combat, music, magic, the hunk-12 place names) keep a `todo_stub!` with the contract
//! and the fallback.

use u6_formats::position::Position;

use crate::objects::{OBJECT_SLOTS, ObjectId};
use crate::state::State;
use crate::todo_stub;

/// Status byte of object `id` (A4+0x190c, byte per object slot: 0x18 = not on the map, 0x20 =
/// temporary, 0x40 = egg active, ...).
pub(crate) fn object_status(state: &State, id: ObjectId) -> u8 {
    state.objects.status(id.slot())
}

/// Ambient light (A4-0x7b79, signed byte; 0x105350 loads it with the objlist block at A4-0x7b84,
/// objlist 0x1BFC; the clock 0x10368a rewrites it: 0 at night and on levels 1..4, 1..7 at dawn
/// and dusk, 7 by day). Read from that block until `actors/` names the field.
pub(crate) fn ambient_light(state: &State) -> i8 {
    state.actors.objlist_tail.get(0x0b).map_or(0, |&b| b as i8)
}

/// Objects in the rectangle x0..=x1, y0..=y1 on level `z`, in iteration order (objects 0x10b05a
/// begins the walk at the leader's level, 0x10af74 yields the next object on the map whose
/// masked x, y and z match). The caller takes a snapshot; the original walks lazily.
pub(crate) fn objects_in_rect(
    state: &mut State,
    x0: i16,
    y0: i16,
    x1: i16,
    y1: i16,
    z: u16,
) -> Vec<ObjectId> {
    let _ = z;
    let mut out = Vec::new();
    let mut c = crate::objects::begin_area(state, x0, y0, x1, y1);
    while let Some(id) = ObjectId::from_slot(c) {
        out.push(id);
        c = crate::objects::next_in_area(&mut state.objects);
    }
    out
}

/// Window origin and coordinate mask for the objects (A4-0x2ef8 / -0x2ef6 / -0x796c: one copy,
/// written here as 0x108d10 writes it), then the window object grid A4+0x67c (0x10dea4).
pub(crate) fn rebuild_object_grid(state: &mut State) {
    let [ox, oy] = state.map.window_origin;
    let z = state.map.centre.z;
    state.objects.set_window(ox, oy, z);
}

/// Object in window cell (wx, wy) of the grid A4+0x67c, -1 = none (built by 0x10dea4).
pub(crate) fn object_grid_cell(state: &State, wx: usize, wy: usize) -> i16 {
    state
        .objects
        .grid
        .get(wy * crate::objects::GRID_SIDE + wx)
        .copied()
        .unwrap_or(-1)
}

/// Move object `id` to (x, y, z), updating the object list, the grid and the "view changed"
/// flag (objects 0x10c9d2).
pub(crate) fn move_object(state: &mut State, id: ObjectId, x: u16, y: u16, z: u16) {
    crate::objects::move_to(state, id.slot(), x, y, z);
}

/// Remove temporary objects (status 0x20) farther than 20 tiles from (x, y) or on another level
/// than `z`, and clear the active bit of such eggs (objects 0x10c49c).
pub(crate) fn drop_far_temporaries(state: &mut State, x: u16, y: u16, z: u16) {
    crate::objects::prune_far(state, x, y, z);
}

/// Show or hide the moongates at the 8 moonstone locations inside the window (main 0x10355c).
pub(crate) fn update_moongates(state: &mut State) {
    crate::main_loop::moongates(state);
}

/// Load the object blocks of the region around (x, y, z) into the object slots (objects
/// 0x10e1ce).
pub(crate) fn load_region_objects(state: &mut State, x: u16, y: u16, z: u16) {
    crate::objects::load_region(state, x, y, z);
}

/// Build the per-view-cell object draw lists for 0x102c90 (objects 0x10e81e; draws RNG). The
/// view tiles 0x109e74 just wrote are A4-0x1476, which the lists' builder overwrites with flat
/// object tiles: one array in the original, kept in `objects.draw.base` (sync rule: copied from
/// `map.view_tiles` right here, the only place 0x109e74 hands it over).
pub(crate) fn build_view_objects(state: &mut State) {
    let n = state
        .objects
        .draw
        .base
        .len()
        .min(state.map.view_tiles.len());
    let tiles = state.map.view_tiles[..n].to_vec();
    state.objects.draw.base[..n].copy_from_slice(&tiles);
    crate::objects::build_draw_lists(state);
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
/// leaves while yielding it (objects 0x10ad1a begins, 0x10aaca steps).
pub(crate) fn objects_at(state: &mut State, x: u16, y: u16, z: u16) -> Vec<(ObjectId, u16)> {
    let at = Position { x, y, z: z as u8 };
    crate::objects::objects_at(state, at)
}

/// First object of type `ty` at (x, y) on the leader's level (objects 0x10ad66).
pub(crate) fn find_object_at(state: &State, ty: u16, x: u16, y: u16) -> Option<ObjectId> {
    ObjectId::from_slot(crate::objects::object_of_type_at(state, ty, x, y))
}

/// Remove object `id` from the map (objects 0x10c1d6).
pub(crate) fn remove_object(state: &mut State, id: ObjectId) {
    crate::objects::remove(state, id.slot());
}

/// Advance the clock (actors 0x10368a; the argument word is passed in D0: 0 from the
/// teleports, 0x0a after the midnight moongate, 0x0f per step of the resurrection wait).
pub(crate) fn clock_tick(state: &mut State, arg: u16) {
    crate::actors::clock::tick(state, arg as i16);
}

/// UI step after a teleport (ui 0x133b28, the sky bar), on the polling engine's screen.
pub(crate) fn ui_after_teleport(state: &mut State) {
    crate::engine::with_current_io(|io| crate::ui::panels::sky_bar(state, io));
}

/// Main-loop step after the party is placed (main 0x104094: back to party mode).
pub(crate) fn main_after_placing(state: &mut State) {
    crate::engine::with_current_io(|io| crate::main_loop::end_solo(state, io));
}

/// Post-step refresh: rebuild the view if dirty and redraw (gfx 0x102ef0), on the polling
/// engine's screen; the party moves call it between steps, so each step is drawn.
pub(crate) fn refresh_view(state: &mut State) {
    crate::engine::with_current_io(|io| crate::gfx::refresh_view(state, io));
}

/// Party member `id` follows the leader once (actors 0x11c910; D1 = 1 from 0x10921a).
pub(crate) fn party_follow(state: &mut State, id: ObjectId) {
    crate::actors::party::follow(state, id.0, true);
}

/// Step actor `id` toward (x, y); nonzero when it moved (actors 0x11f452; draws RNG).
pub(crate) fn step_toward(state: &mut State, id: ObjectId, x: u16, y: u16) -> i16 {
    i16::from(crate::actors::npc::approach(state, id.0, x, y))
}

/// Distance from actor `id` to (x, y) (owner: combat leaf 0x121b08, an M1 stub point; uses
/// `abs`). Fallback: 0, so a gathering member joins the leader at once (0x108fba).
pub(crate) fn distance(_state: &State, _id: ObjectId, _x: u16, _y: u16) -> i16 {
    todo_stub!("0x121b08 distance from an actor to a location", 0)
}

/// Walk actor `id` one step in direction `dir` (0, 2, 4, 6) with flag 1 (actors 0x11f0a0).
pub(crate) fn walk(state: &mut State, id: ObjectId, dir: u16) {
    crate::actors::walk::step(state, id.0, dir, 1);
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

/// Shamino names the place a dungeon entrance of quality `q` (1..=20) leads to (0x11d1d4):
/// printf of the string at A4-0x7303, then "dungeon " (0x11b8f8) for q < 8 or "shrine of "
/// (0x11b902) for q 9..=11, then the name the pointer table A4-0x6a60 + 4q points at, then the
/// string at 0x11b90e; all read from the user's `game`, queued for the loop to print.
pub(crate) fn say_place_name(state: &mut State, q: u8) {
    use crate::image::A4;
    use crate::main_loop::{game_string, pointed_string, queue_print};
    let lines = crate::engine::with_current_io(|io| {
        let mut lines = vec![crate::main_loop::a4_string(io, 0x7303)];
        if q < 8 {
            lines.push(game_string(io, 0x0011_b8f8));
        } else if (9..=11).contains(&q) {
            lines.push(game_string(io, 0x0011_b902));
        }
        lines.push(pointed_string(
            io,
            A4.wrapping_sub(0x6a60) + 4 * u32::from(q),
        ));
        lines.push(game_string(io, 0x0011_b90e));
        lines
    });
    for line in lines.unwrap_or_default() {
        queue_print(state, line, Vec::new());
    }
}

/// Print A4-0x72cf, "You must be in party mode to enter." (printf 0x108658 of the string in
/// the user's `game`, queued for the loop to print).
pub(crate) fn say_party_mode_needed(state: &mut State) {
    let line = crate::engine::with_current_io(|io| crate::main_loop::a4_string(io, 0x72cf));
    crate::main_loop::queue_print(state, line.unwrap_or_default(), Vec::new());
}

/// Maximum hit points of actor `id` (hunk-13 leaf 0x122144, ported with the actor model).
pub(crate) fn max_hp(state: &State, id: ObjectId) -> u8 {
    crate::actors::leaves::max_hp(state, id.0) as u8
}

/// Scan for nearby hostiles and toggle the combat state (actors 0x120a1c).
pub(crate) fn scan_hostiles(state: &mut State) {
    crate::actors::turn::go_to_schedules(state);
}

/// Put the Avatar back from its body object (objects 0x10c810).
pub(crate) fn raise_avatar_body(state: &mut State, body: ObjectId) {
    crate::objects::actor_from_body(state, body.slot());
}

/// Objects of the region list (A4-0xf84 head = slot 0x100, A4-0x1184 next links) in list order.
pub(crate) fn region_list(state: &State) -> Vec<ObjectId> {
    let o = &state.objects;
    let mut out = Vec::new();
    let mut c = o.next_of(crate::objects::HEAD);
    while let Some(id) = ObjectId::from_slot(c) {
        out.push(id);
        if out.len() > OBJECT_SLOTS {
            break;
        }
        c = o.next_of(c);
    }
    out
}

//! Calls from object code into modules that are not ported yet (WU-P3 seams).
//!
//! Each seam is the narrowest signature the object code needs, with a stub contract: what the
//! original routine does at that call, the fallback the port uses meanwhile, and whether a
//! milestone-1 path reaches it. The dispatcher points each at the owning module at merge.

use crate::state::State;
use crate::todo_stub;

/// Owner: main / ui (the command loop 0x1041d8 keeps the code at A4-0x7a30).
///
/// Contract: the code of the command being executed (0x92 while a party member's inventory is
/// open for a command, 0x90 / 0x91 for attack modes). Fallback 0 = no command, so "an object
/// the selected member carries changed" is never flagged. Reached by every object move.
pub(crate) fn command_code(_s: &State) -> u16 {
    todo_stub!("0x1041d8 current command code (A4-0x7a30)", 0)
}

/// Owner: main / ui (A4-0x7a3e, index into the party of the member a command acts for).
///
/// Contract: read only while the command code is 0x92. Fallback 0 (the leader slot).
pub(crate) fn command_member(_s: &State) -> u16 {
    todo_stub!("0x1041d8 command party member (A4-0x7a3e)", 0)
}

/// Owner: ui. Original: 0x108658 (printf to the message scroll).
///
/// Contract: prints the game string at image address `fmt` (format directives filled from
/// `args`) to the message scroll. Fallback: nothing printed; the caller continues. Reached
/// only on error and effect paths (too many objects in a region, traps, blown doors).
pub(crate) fn print(_s: &mut State, _fmt: u32, _args: &[i32]) {
    todo_stub!("0x108658 printf to the message scroll", ())
}

/// Owner: main. Original: 0x1032b6 via 0x10f03c (after an actor moved while the view is live).
///
/// Contract: per-actor follow-up of a move (party status effects of the new tile). Fallback:
/// nothing. Reached by every actor move after load (flag A4-0x784e set).
pub(crate) fn after_actor_moved(_s: &mut State, _actor: i16) {
    todo_stub!("0x1032b6 per-actor step after a move", ())
}

/// Owner: ui. Original: 0x110f96 (equipment slot an object is readied in: 1 = one hand, 2 =
/// head, 5 = other hand, 8 = both hands, 9 = ring...).
///
/// Contract: pure lookup by type. Fallback 0 = "no slot", so a readied object is never
/// treated as needing a free hand. Reached by 0x10cf7c (adding to an actor) and 0x128ce8.
pub(crate) fn ready_slot(_s: &State, _obj: i16) -> i16 {
    todo_stub!("0x110f96 ready slot of an object", 0)
}

/// Owner: map / ui (A4-0x7b71, byte: dark cells show tiles with flags3 bit 0 — a "see
/// through" mode). Fallback false. Read by every draw-list build.
pub(crate) fn see_through(_s: &State) -> bool {
    todo_stub!("0x10e596 see-through flag (A4-0x7b71)", false)
}

/// Owner: map (A4-0x794c, word: skip the view-flag / light pass of 0x10e81e). Fallback 0, so the
/// pass runs on the fallbacks below (everything visible, no light overlay).
pub(crate) fn view_pass_off(_s: &State) -> u16 {
    todo_stub!("0x10e81e view pass switch (A4-0x794c)", 0)
}

/// Owner: map. Original: A4+0x514a (40 × 40 bytes of view flags from 0x109e74, bit 7 = lit /
/// visible) at window (col, row). Fallback 0x80 (visible), so no cell loses its objects.
pub(crate) fn view_flag(_s: &State, _col: i16, _row: i16) -> u8 {
    todo_stub!("0x109e74 view flag at a window tile (A4+0x514a)", 0x80)
}

/// Owner: map. Original: A4+0x578a (40 × 40 bytes: light level 1..3 draws overlay tile
/// 0x1bc + level). Fallback 0 (no overlay).
pub(crate) fn view_light(_s: &State, _col: i16, _row: i16) -> u8 {
    todo_stub!("0x109e74 light level at a window tile (A4+0x578a)", 0)
}

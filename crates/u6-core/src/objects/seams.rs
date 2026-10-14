//! Calls from object code into other modules (WU-P3 seams), forwarding to their owners since
//! the integration (WU-P7). The one target outside milestone 1 keeps its `todo_stub!`.

use crate::state::State;
use crate::todo_stub;

/// The panel mode A4-0x7a30 (0x92 while a party member's inventory is open for a command,
/// 0x90 / 0x91 the stats / roster panels; ui owns it, `ui.panel.mode`).
pub(crate) fn command_code(s: &State) -> u16 {
    s.ui.panel.mode
}

/// The party index the panel shows (A4-0x7a3e, `ui.panel.member`).
pub(crate) fn command_member(s: &State) -> u16 {
    s.ui.panel.member as u16
}

/// Printf 0x108658 of the game string at image address `fmt` with word arguments, queued for
/// the command loop to print (object code is synchronous; ui printf may wait on a more prompt).
pub(crate) fn print(s: &mut State, fmt: u32, args: &[i32]) {
    let text = crate::engine::with_current_io(|io| crate::main_loop::game_string(io, fmt));
    let args = args
        .iter()
        .map(|&a| crate::main_loop::PrintArg::Word(a as u16))
        .collect();
    crate::main_loop::queue_print(s, text.unwrap_or_default(), args);
}

/// Main 0x1032b6 via 0x10f03c (after an actor moved while the view is live): the hazard of the
/// actor's new tile.
pub(crate) fn after_actor_moved(s: &mut State, actor: i16) {
    if actor >= 0 {
        crate::main_loop::actor_status_tick(s, actor as u16);
    }
}

/// Owner: ui. Original: 0x110f96 (equipment slot an object is readied in: 1 = one hand, 2 =
/// head, 5 = other hand, 8 = both hands, 9 = ring...).
///
/// Contract: pure lookup by type. Fallback 0 = "no slot", so a readied object is never
/// treated as needing a free hand. Reached by 0x10cf7c (adding to an actor) and 0x128ce8.
pub(crate) fn ready_slot(_s: &State, _obj: i16) -> i16 {
    todo_stub!("0x110f96 ready slot of an object", 0)
}

/// A4-0x7b71 (byte 0x13 of the objlist block at A4-0x7b84): dark cells show tiles with flags3
/// bit 0 (a "see through" mode).
pub(crate) fn see_through(s: &State) -> bool {
    s.actors.objlist_tail.get(0x13).is_some_and(|&b| b != 0)
}

/// A4-0x794c (word: skip the view-flag / light pass of 0x10e81e; `map.reveal_all`).
pub(crate) fn view_pass_off(s: &State) -> u16 {
    s.map.reveal_all
}

/// A4+0x514a (40 × 40 bytes of view flags from 0x109e74, bit 7 = lit / visible) at window
/// (col, row); 0 outside the grid.
pub(crate) fn view_flag(s: &State, col: i16, row: i16) -> u8 {
    grid_byte(&s.map.view_flags, col, row)
}

/// A4+0x578a (40 × 40 bytes: light level 1..3 draws overlay tile 0x1bc + level).
pub(crate) fn view_light(s: &State, col: i16, row: i16) -> u8 {
    grid_byte(&s.map.light, col, row)
}

fn grid_byte(grid: &[u8], col: i16, row: i16) -> u8 {
    let side = crate::map::WINDOW as i16;
    if !(0..side).contains(&col) || !(0..side).contains(&row) {
        return 0;
    }
    grid.get(crate::map::cell(col as usize, row as usize))
        .copied()
        .unwrap_or(0)
}

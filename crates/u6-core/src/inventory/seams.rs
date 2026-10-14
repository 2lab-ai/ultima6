//! Calls from the inventory and Rest code into original functions no unit owns yet. Each keeps a
//! `todo_stub!` with its contract and an explicit fallback; the dispatcher reconciles them.

use crate::engine::Io;
use crate::objects::Slot;
use crate::state::State;
use crate::todo_stub;

/// 0x132338 (ui, hunk 17: flash `actor` in the map window: its tile plus 0x1800 (`bright`) or
/// plus 0x1000 drawn over its cell (0x10e596), the window presented dissolving, the draw lists
/// rebuilt (0x10e81e) and presented again). Owner: ui / magic (the invisibility spells reach it
/// too). Contract: visual only, no RNG draw besides the rebuild's. Fallback: nothing drawn; the
/// next view refresh shows the actor's new state.
pub(crate) fn flash_actor(_s: &mut State, _io: &Io, _actor: Slot, _bright: bool) {
    todo_stub!("0x132338 flash an actor in the map window", ())
}

//! Calls from Restore into functions other modules own: combat's ports are called directly
//! (integration M2); the area song keeps its contract and an explicit fallback. Owner in
//! brackets.

use crate::state::State;
use crate::todo_stub;

/// 0x110912: pick the song of the party's area and play it [sound].
pub(crate) fn play_area_song(_s: &mut State) {
    // Contract: start the area song. Fallback: silence (M1 has no sound).
    todo_stub!("0x110912 pick and play the area song", ());
}

/// 0x121c64: enter combat mode (the saved combat byte A4-0x7b0c is set) [combat]:
/// [`crate::combat::mode::begin_combat`].
pub(crate) fn begin_combat(s: &mut State) {
    crate::combat::mode::begin_combat(s);
}

/// 0x121d78: leave combat mode (the saved combat byte is clear) [combat]:
/// [`crate::combat::mode::break_off`].
pub(crate) fn break_off_combat(s: &mut State) {
    crate::combat::mode::break_off(s);
}

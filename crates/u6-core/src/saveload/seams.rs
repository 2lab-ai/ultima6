//! Calls from Restore into functions other modules own and have not ported. Each keeps the
//! contract and an explicit fallback; the owner replaces the body with the call. Proposed
//! owner in brackets.

use crate::state::State;
use crate::todo_stub;

/// 0x110912: pick the song of the party's area and play it [sound].
pub(crate) fn play_area_song(_s: &mut State) {
    // Contract: start the area song. Fallback: silence (M1 has no sound).
    todo_stub!("0x110912 pick and play the area song", ());
}

/// 0x121c64: enter combat mode (the saved combat byte A4-0x7b0c is set) [combat].
pub(crate) fn begin_combat(_s: &mut State) {
    // Contract: party worktypes to combat, the combat panel. Fallback: not in combat.
    todo_stub!("0x121c64 begin combat mode", ());
}

/// 0x121d78: leave combat mode (the saved combat byte is clear) [combat].
pub(crate) fn break_off_combat(_s: &mut State) {
    // Contract: party worktypes back to following. Fallback: worktypes as saved.
    todo_stub!("0x121d78 break off combat", ());
}

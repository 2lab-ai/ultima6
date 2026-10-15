//! Calls from Restore into functions other modules own (integration M2: combat's and sound's
//! ports are called directly). Owner in brackets.

use crate::state::State;

/// 0x110912: pick the song of the party's area and play it [sound]:
/// [`crate::sound::play_area_song`].
pub(crate) fn play_area_song(s: &mut State) {
    crate::sound::play_area_song(s);
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

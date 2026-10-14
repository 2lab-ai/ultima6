//! Peer and the gems: 0x12c54a.
//!
//! 0x12c54a sits in hunk 15 with the object commands (its callers are the Peer spell through
//! the hunk-11 stub 0x11b0e4 and Use on a gem, 0x131048); it is ported here, its only spell
//! side, for the dispatcher to move with the Use command.

use std::cell::RefCell;

use super::seams;
use crate::engine::Io;
use crate::objects::Slot;
use crate::state::State;

/// Original: 0x12c54a (objects: the map overview (0x13410a); `gem` ≠ 0 spends one of the
/// gem's quantity, the last one removes it (0x10c1d6); with the inventory panel up it is
/// redrawn (0x1118c6))
///
/// The Peer spell (0x27) calls it with 0, after the flashes; the dispatcher prints "Done".
pub async fn peer(state: &RefCell<State>, io: &Io, gem: Slot) {
    seams::overview(state, io).await;
    let mut s = state.borrow_mut();
    if gem != 0 {
        let i = gem as usize;
        let n = s.objects.qq[i] & 0xff;
        if n > 1 {
            s.objects.qq[i] = (s.objects.qq[i] & 0xff00) | (n - 1);
        } else {
            crate::objects::remove(&mut s, gem);
        }
    }
    if s.ui.panel.mode == 0x92 {
        seams::redraw_inventory(&mut s);
    }
}

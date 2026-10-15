//! Calls from the inventory and Rest code into original functions other units own, each the
//! narrowest signature the inventory needs, forwarding to its owner.

use crate::engine::Io;
use crate::objects::Slot;
use crate::state::State;

/// 0x132338 (ui, hunk 17: flash `actor` in the map window): [`crate::gfx::flash_actor`].
pub(crate) fn flash_actor(s: &mut State, io: &Io, actor: Slot, bright: bool) {
    crate::gfx::flash_actor(s, io, actor as u16, bright);
}

//! Inventory: the panel contents (hunk 8), equipment slots and weights, Ready / Unready, the
//! portrait and equipment panel 0x128ce8 (M2-inventory).
//!
//! Domain state = [`InventoryState`]: the weight words and the 20 panel cells the hunk-8 code
//! shares (A4+0x1342..0x136f), the pointer origin of 0x105016 and the tables read from the
//! user's `game`. The panel globals that ui already keeps (`ui.panel`: list count A4+0x1340,
//! first row A4-0x722a, container A4+0x1346, the two-handed flags A4+0x1370 / 0x1382, the open
//! container A4+0x1380) are written there, not duplicated.
//!
//! The equipment slots, as 0x1113fe draws them (paper-doll cells around the figure):
//! 0 head, 1 neck, 2 weapon hand, 3 first ring, 4 body, 5 other hand, 6 second ring, 7 feet.
//! 0x110f96 also answers 8 (both hands: kept in slot 2, slot 5 holds the marker 1) and 9 (a ring:
//! slot 3, else 6).
//!
//! Original functions this module owns:
//!
//! - 0x110f96 [ui] equipment slot of an object ([`ready_slot`])
//! - 0x1110c8 [ui] type weighed per ten ([`counted_type`])
//! - 0x1110f6 [ui] weight of one object ([`object_weight`])
//! - 0x1111ca [ui] weight of an object's contents ([`contents_weight`])
//! - 0x11121e [ui] list count and weights of an actor or container ([`weights`]; ported at the
//!   M2 integration from objects::commands' fallback, which Get, Drop and Use call)
//! - 0x1112cc [ui] weights and equipment of an actor or container ([`carried_weight`])
//! - 0x111882 [ui] tenths to whole stones ([`stones`])
//! - 0x111be0 [ui] panel contents ([`panel_contents`])
//! - 0x1118c6 [ui] inventory list, figure and weight lines ([`draw_list`])
//! - 0x1113fe [ui] equipment cells ([`draw_equipment`])
//! - 0x105b14 [ui] one inventory icon with its count ([`draw_icon`])
//! - 0x105a3e [ui] a tile with a number ([`tile_with_number`])
//! - 0x1059d4 [ui] one 3 × 5 digit ([`digit`])
//! - 0x111dca / 0x111e0a [ui] scroll the list down / up ([`list_down`], [`list_up`])
//! - 0x111e3c / 0x111eec [ui] the list / equipment slot under the pointer ([`list_slot_at`],
//!   [`equipment_slot_at`])
//! - 0x112052 [inventory] Ready ([`ready`]); 0x1123d6 [inventory] Unready ([`unready`]) with
//!   0x1122dc, 0x11236c, 0x111fe0
//! - 0x128ce8 [objects] portrait / equipment panel ([`show_actor`])
//! - 0x105016 [ui] direction of a pixel from the pointer origin ([`direction_to`])

mod panel;
mod portrait;
mod ready;
#[cfg(test)]
mod real_data;
mod seams;
mod tables;
#[cfg(test)]
mod tests;
mod weight;

pub use panel::{
    digit, draw_equipment, draw_icon, draw_list, equipment_slot_at, list_down, list_slot_at,
    list_up, panel_contents, recount, tile_with_number,
};
pub use portrait::{direction_to, face_pointer, show_actor};
pub use ready::{equipped_index, ready, release_effect, unready, unready_check};
pub use tables::InventoryTables;
pub use weight::{
    carried_weight, contents_weight, counted_type, object_weight, ready_slot, stones, weights,
};

use crate::files::Files;
use crate::state::LoadError;

/// Cells of the panel (A4+0x1348..0x136f): the 12 list cells, then the 8 equipment slots.
pub const CELLS: usize = 20;
/// Index of equipment slot 0 in [`InventoryState::cells`] (A4+0x1360).
pub const EQUIP: usize = 12;

/// Hunk-8 inventory globals and the tables they use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryState {
    /// Original: A4+0x1342 (word: weight of the readied objects, tenths of a stone)
    pub equip_weight: i16,
    /// Original: A4+0x1344 (word: weight of the rest of the inventory, tenths)
    pub carried_weight: i16,
    /// Original: A4+0x1348 (12 words: object in each list cell, 0 = empty) and A4+0x1360
    /// (8 words: object readied in each equipment slot, 0 = none, 1 = taken by a two-handed
    /// weapon). One array, so the original's slot -1 writes land in list cell 11 as there.
    pub cells: [i16; CELLS],
    /// Original: A4-0x7a26 / -0x7a25 (bytes: pixel 0x105016 measures directions from; the
    /// Move command (objcmds, 0x12ae04) moves it to the object being pushed and back to
    /// (0x58, 0x58), the centre of the map window; initial value from the data hunk)
    pub pointer_origin: (u8, u8),
    /// Tables read from the user's `game`.
    pub tables: InventoryTables,
}

impl Default for InventoryState {
    fn default() -> Self {
        Self {
            equip_weight: 0,
            carried_weight: 0,
            cells: [0; CELLS],
            pointer_origin: (0x58, 0x58),
            tables: InventoryTables::default(),
        }
    }
}

impl InventoryState {
    /// Reads the tables from the user's `game` (State::new_game calls this).
    pub fn load(files: &Files) -> Result<Self, LoadError> {
        let game = files.get("game").ok_or_else(|| LoadError::Missing {
            file: "game".into(),
        })?;
        let tables = InventoryTables::from_game(game).map_err(|detail| LoadError::Table {
            file: "game".into(),
            detail,
        })?;
        Ok(Self {
            pointer_origin: tables.pointer_origin,
            tables,
            ..Self::default()
        })
    }

    /// Equipment slot `i` (A4+0x1360 + 2 i; `i` may be -1, which is list cell 11).
    pub fn equip(&self, i: i16) -> i16 {
        usize::try_from(EQUIP as i16 + i)
            .ok()
            .and_then(|i| self.cells.get(i))
            .copied()
            .unwrap_or(0)
    }

    /// Sets equipment slot `i` (same indexing as [`InventoryState::equip`]).
    pub fn set_equip(&mut self, i: i16, obj: i16) {
        if let Some(c) = usize::try_from(EQUIP as i16 + i)
            .ok()
            .and_then(|i| self.cells.get_mut(i))
        {
            *c = obj;
        }
    }
}

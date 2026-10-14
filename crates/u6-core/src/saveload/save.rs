//! Save 0x105716 and its block write-back 0x10e4bc.

use std::cell::RefCell;

use u6_formats::objblk::ObjBlock;

use super::objlist;
use crate::engine::Io;
use crate::objects::{BLOCK_COUNT, Slot};
use crate::state::State;

/// Save mode A4-0x637a while playing (0x105716 and 0x1052c8 set it).
pub(crate) const MODE_PLAY: u16 = 0x20;
/// Location bits of a member carried by the vehicle the party is aboard (0x10cda8's place).
const IN_INVENTORY: u8 = 0x10;

/// The rename loop of 0x10e4bc: every block whose `.tmp` file is current (A4-0x78d8) replaces
/// its block file (0x134b48 deletes `savegame/objblk??`, 0x134b9e renames the `.tmp` over it;
/// read back as 0x10ddd2 / 0x10e10c do, [`crate::objects::parse_lenient`]), and the flag clears. The block is stamped with the running save generation so the platform
/// stores it ([`super::changed_since`]).
///
/// A flag without a `.tmp` file leaves no block file behind (the delete happens, the rename
/// finds nothing); the port reads that as an empty block. 0x10d538 writes the file whenever it
/// sets the flag, so this does not happen in play.
pub fn commit_blocks(s: &mut State) {
    let generation = s.saveload.generation;
    for i in 0..BLOCK_COUNT {
        if s.objects.tmp_flag.get(i).copied().unwrap_or(0) == 0 {
            continue;
        }
        let tmp = s.objects.tmp.get_mut(i).and_then(Option::take);
        if let Some((_, block)) = s.objects.blocks.get_mut(i) {
            *block = tmp.as_deref().map_or_else(
                || ObjBlock {
                    objects: Vec::new(),
                },
                crate::objects::parse_lenient,
            );
        }
        s.objects.tmp_flag[i] = 0;
        if let Some(g) = s.saveload.block_gen.get_mut(i) {
            *g = generation;
        }
    }
}

/// Original: 0x10e4bc (objects: write the region's blocks back, commit every `.tmp`, reload)
///
/// In a dungeon (view level ≠ 0) the level goes back to its `.tmp` file (0x10dfb4), on the
/// surface each of the four superchunks in slots (0x10d6ac); then [`commit_blocks`], the
/// region keys emptied (A4-0x784c ×4, A4-0x7844, A4-0x7842 = -1), the region read again at the
/// view centre (0x10e1ce) and the window grid rebuilt (0x10dea4).
pub fn write_back_region(s: &mut State) {
    let z = s.map.centre.z;
    if z != 0 {
        crate::objects::write_back_level(s, i16::from(z));
    } else {
        for i in 0..4 {
            let b = s.objects.loaded_surface[i];
            crate::objects::write_back_surface(s, b);
        }
    }
    commit_blocks(s);
    let o = &mut s.objects;
    o.loaded_surface = [-1; 4];
    o.surface_base = -1;
    o.loaded_level = -1;
    let c = s.map.centre;
    crate::objects::load_region(s, c.x, c.y, u16::from(c.z));
    s.objects.rebuild_grid();
}

/// The party is aboard a vehicle: the leader index is the party size (the vehicle is the
/// actor the view follows and the members ride in its inventory).
fn aboard(s: &State) -> bool {
    s.actors.party.leader_index == s.actors.party.size
}

fn members(s: &State) -> Vec<Slot> {
    let p = &s.actors.party;
    p.members
        .iter()
        .take(usize::from(p.size))
        .map(|&m| Slot::from(m))
        .collect()
}

/// Original: 0x105716 (saveload: Save: write back the region, commit the blocks, write objlist)
///
/// Save mode back to play (A4-0x637a = 0x20), the window-cache key cleared (A4-0x7958 ..
/// -0x7954 = -1). Aboard a vehicle, every member first steps out onto the view centre
/// (0x10c9d2), so the saved members stand on the vehicle's tile. Then the region goes back to
/// the block files ([`write_back_region`]), the quest flag takes the copy 0x105350 made
/// (A4-0x7b84 = A4-0x2fd2), and `savegame/objlist` is written (disk check 0x1349a0 and open
/// 0x134a54 are the platform's; the 25 tables of [`objlist::encode`]). After: the window cache
/// rebuilt (0x108f90), the members back into the vehicle (0x10cda8, place 0x10), and the panel
/// redrawn (A4-0x7afe + 1).
pub fn save(s: &mut State) {
    s.objects.save_mode = MODE_PLAY;
    s.map.window_key = [-1; 3];
    s.saveload.generation += 1;
    if aboard(s) {
        let c = s.map.centre;
        for m in members(s) {
            crate::objects::move_to(s, m, c.x, c.y, u16::from(c.z));
        }
    }
    write_back_region(s);
    s.actors.quest_flag = s.actors.quest_flag_copy as u8;
    s.saveload.objlist = Some(objlist::encode(s));
    crate::map::region::refresh_region(s);
    if aboard(s) {
        for m in members(s) {
            crate::objects::move_into(s, m, 0, IN_INVENTORY);
        }
    }
    s.ui.panel.dirty = true;
}

/// The Save command's handler as the command loop calls it (code 0xaa after the save question's
/// answer Y). The save itself does not wait; the warnings a block write may queue (0x10d376)
/// are printed by the loop's next flush.
pub async fn save_game(state: &RefCell<State>, _io: &Io) {
    save(&mut state.borrow_mut());
}

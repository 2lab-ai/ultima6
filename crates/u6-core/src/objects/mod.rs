//! Objects: the object tables, iteration, move/remove, names (WU-P3).
//!
//! Domain state = [`Objects`]: the slot tables of the original (identity = slot index
//! [`ObjectId`], location, type/frame, status, quality/quantity), the one list that links every
//! slot in use, the 40 × 40 window grid that indexes it, the iteration cursors and the object
//! blocks. Actors are slots 0..256; `actors/` refers to them by [`ObjectId`] and never keeps its
//! own copy of a position.
//!
//! The model, read off 0x10a7b2, 0x10c9d2, 0x10cda8 and 0x10db82:
//!
//! - `next` chains every slot in use into one list headed by slot 0x100, a sentinel that holds
//!   no object. Map objects are sorted by (y, x); z takes no part. Every object is followed
//!   directly by its contents (pre-order), so an object and everything inside it move as one
//!   run of the list.
//! - A contained object (status bits 3-4 set: 0x08 container, 0x10 inventory, 0x18 readied)
//!   keeps its parent's slot in location bytes 0-1, `x | (y & 0x3f) << 10`.
//! - `grid[dy * 40 + dx]` is the first list entry at window tile (dx, dy), or -1. The window
//!   origin is [`Objects::window`], written by map 0x108d10 when the view moves.
//! - Free slots form two more chains through `next`: 0x101.. for objects and 0xe0..0x100 for
//!   temporary actors (monsters).
//! - Only the 2 × 2 surface superchunks around the party, or one dungeon level, live in slots.
//!   0x10e1ce writes the others back to their `.tmp` block and reads the new ones.
//!
//! Submodules: `defs` (tables read from the user's `game`), `list` (list, grid, free slots),
//! `iter` (cursors), `moves` (move, remove, create), `parts` (multi-tile monsters), `blocks`
//! (region load and write-back), `names`, `draw` (map-window draw lists), `effects` (hunk 15-17
//! helpers), `seams` (calls into modules not ported yet).
//!
//! Original functions this module will own (milestone 1; subsystem in brackets; roles from the
//! inventory, "[likely]" as classified):
//!
//! - 0x10a4d6 [objects] leaf; callers 5, from 3 subsystems
//! - 0x10a5b0 [objects] unnamed; calls 0x10aaca (next object of the iteration)
//! - 0x10a79a [objects] leaf; callers 2, from 1 subsystems
//! - 0x10a7b2 [objects] unnamed; 1 callees
//! - 0x10aaca [objects] next object of the iteration
//! - 0x10ad1a [objects] begin iterating objects at a location
//! - 0x10ad66 [objects] unnamed; 1 callees
//! - 0x10ae52 [objects] object stacked on / contained in an object
//! - 0x10aed6 [objects] leaf; callers 19, from 6 subsystems
//! - 0x10af74 [objects] leaf; callers 10, from 4 subsystems
//! - 0x10b05a [objects] unnamed; 2 callees
//! - 0x10b0ba [objects] leaf; callers 2, from 2 subsystems
//! - 0x10b18c [objects] unnamed; 1 callees
//! - 0x10b20e [objects] leaf; callers 3, from 2 subsystems
//! - 0x10b2cc [objects] unnamed; 1 callees
//! - 0x10b42e [objects] unnamed; calls 0x10aaca (next object of the iteration)
//! - 0x10b922 [objects] unnamed; calls 0x10aaca (next object of the iteration)
//! - 0x10b9ca [objects] unnamed; calls 0x10aaca (next object of the iteration)
//! - 0x10bc4c [objects] unnamed; 2 callees
//! - 0x10bcee [objects] unnamed; 3 callees
//! - 0x10be02 [objects] unnamed; 2 callees
//! - 0x10bec8 [objects] unnamed; 3 callees
//! - 0x10c00a [objects] unnamed; 1 callees
//! - 0x10c0c4 [objects] leaf; callers 3, from 1 subsystems
//! - 0x10c1d6 [objects] remove an object from the map
//! - 0x10c49c [objects] unnamed; calls 0x10c1d6 (remove an object from the map)
//! - 0x10c810 [objects] unnamed; calls 0x10ae52 (object stacked on / contained~)
//! - 0x10c928 [objects] leaf; callers 12, from 5 subsystems
//! - 0x10c970 [objects] unnamed; 1 callees
//! - 0x10c9d2 [objects] move an object/actor to a map location
//! - 0x10cda8 [objects] unnamed; 5 callees
//! - 0x10cef2 [objects] leaf; callers 16, from 4 subsystems
//! - 0x10cf2c [objects] unnamed; 1 callees
//! - 0x10cf7c [objects] unnamed; calls 0x10ae52 (object stacked on / contained~)
//! - 0x10d16e [objects] unnamed; calls 0x10ae52 (object stacked on / contained~)
//! - 0x10d2d8 [objects] unnamed; calls 0x10ae52 (object stacked on / contained~)
//! - 0x10d376 [objects] add object to the region list (fails when full)
//! - 0x10d4e8 [objects] uses "savegame/objblkaa.tmp"
//! - 0x10d538 [objects] uses "savegame/objblkaa"
//! - 0x10d6ac [objects] uses "savegame/objblkaa.tmp"
//! - 0x10d86e [objects] leaf; callers 2, from 1 subsystems
//! - 0x10da94 [objects] step of 0x10e1ce (load object blocks for a region [~)
//! - 0x10db82 [objects] unnamed; 2 callees
//! - 0x10ddd2 [objects] uses "savegame/objblkaa"
//! - 0x10dea4 [objects] leaf; callers 2, from 2 subsystems
//! - 0x10dfb4 [objects] uses "savegame/objblkaa.tmp"
//! - 0x10e10c [objects] uses "savegame/objblkaa"
//! - 0x10e1ce [objects] load object blocks for a region [likely]
//! - 0x10e596 [objects] leaf; callers 4, from 3 subsystems
//! - 0x10e73a [objects] unnamed; 1 callees
//! - 0x10e81e [objects] unnamed; calls 0x151f06 (random number in [lo, hi])
//! - 0x10ed44 [objects] leaf; callers 2, from 2 subsystems
//! - 0x10edb4 [objects] step of 0x105350 (load state: objlist, schedules, b~)
//! - 0x10ee76 [objects] object name from look.lzd
//! - 0x10eef8 [objects] display name of an object
//! - 0x1283b8 [objects] leaf; callers 8, from 1 subsystems
//! - 0x1288ee [objects] leaf; callers 2, from 1 subsystems
//! - 0x128942 [objects] leaf; callers 2, from 1 subsystems
//! - 0x12899c [objects] leaf; callers 3, from 2 subsystems
//! - 0x128b0e [objects] unnamed; 1 callees
//! - 0x128b98 [objects] leaf; callers 4, from 3 subsystems
//! - 0x128ce8 [objects] unnamed; calls 0x10790a (read a command key; handles p~)
//! - 0x12ba36 [objects] trap effects (acid, poison, bomb, gas)
//! - 0x12d9ee [objects] uses "The door is blown up!"
//! - 0x12db7e [objects] unnamed; calls 0x102ef0 (post-step refresh: rebuild vi~)
//! - 0x12dcbe [objects] unnamed; calls 0x10aaca (next object of the iteration)
//! - 0x12f6ee [objects] unnamed; calls 0x102ef0 (post-step refresh: rebuild vi~)
//! - 0x12f8fe [objects] uses "No room!"
//! - 0x1301e2 [objects] uses "Not now!"
//! - 0x131196 [unknown] index of a value in a small-data word list
//! - 0x1311d2 [unknown] unnamed; calls 0x151f06 (random number in [lo, hi])
//! - 0x13121a [unknown] unnamed; calls 0x151f06 (random number in [lo, hi])
//! - 0x131646 [unknown] unnamed; calls 0x131196 (index of a value in a small-d~)
//! - 0x1319b2 [objects] uses "Shamino says, "I hear somet~" (hatch an egg; stub)

mod blocks;
pub mod commands;
mod defs;
mod draw;
mod effects;
mod iter;
mod list;
mod moves;
mod names;
mod parts;
#[cfg(test)]
mod real_data;
mod seams;
#[cfg(test)]
mod tests;

pub use blocks::{
    BLOCK_COUNT, MAX_RECORDS, block_index, delete_tmp_files, load_block, load_level, load_region,
    load_surface, prune_far, write_back_block, write_back_level, write_back_surface,
};
#[allow(unused_imports)]
pub(crate) use blocks::{load_region_objects, unload_region_objects};
pub use defs::{DefsError, ObjectDefs};
pub use draw::{DRAW_CELLS, DRAW_NODES, DrawLists, build_draw_lists};
pub use effects::*;
pub use iter::{
    AreaCursor, ChildCursor, LocationCursor, actor_at, begin_area, begin_at, find_child,
    first_child, next_at, next_child, next_child_of_kind, next_in_area, object_of_type_at,
    objects_at, objects_at_view,
};
pub use list::{init_slots, link_actors, load_state_objects};
pub use moves::{
    actor_from_body, add_to_owner, create_actor, create_from_record, create_in, create_on_map,
    move_into, move_to, remove, take_from_owner,
};
pub use names::{
    article_text, count_of_kind, described_name, display_name, look_text, name_prefix_text,
    quantity, stack_kind,
};
pub use parts::{
    find_segment_at, move_monster_parts, move_serpent, owner_of_part, update_vine_frames,
};

use u6_formats::objblk::{ObjBlock, ObjBlockId};
use u6_formats::position::Position;

/// Object slots in the location / type tables: 0x137988 − 0x135588 = 0x2400 bytes = 3072 × 3,
/// and 0x10ed44 links `next` through slot 0xbff.
pub const OBJECT_SLOTS: usize = 3072;
/// Slots 0..256 are actors.
pub const ACTOR_SLOTS: usize = 256;
/// Window grid side in tiles (A4+0x67c, 40 × 40 words).
pub const GRID_SIDE: usize = 40;

/// A slot number as the original passes it: 0..0xc00, or -1 for "none".
pub type Slot = i16;
/// "No object" (-1 in every slot word of the original).
pub const NONE: Slot = -1;
/// The list head: slot 0x100 holds no object; `next[0x100]` is the first object of the list.
pub const HEAD: Slot = 0x100;
/// First free object slot after 0x10ed44 (0x101) and the free count (0xaff).
pub const FIRST_OBJECT_SLOT: Slot = 0x101;
/// First temporary-actor slot (0xe0); slots 0xe0..0x100 are monsters.
pub const FIRST_TEMP_ACTOR: Slot = 0xe0;

/// Status bits 3-4: where the object is.
pub const WHERE_MASK: u8 = 0x18;
pub const IN_CONTAINER: u8 = 0x08;
pub const IN_INVENTORY: u8 = 0x10;
pub const READIED: u8 = 0x18;
/// Status bit 5: temporary (created during play; 0x10c49c removes it when out of range).
pub const TEMPORARY: u8 = 0x20;

/// An object slot (index into the object tables); actors are 0..256.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub u16);

impl ObjectId {
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// The slot word the original passes (-1 never names an `ObjectId`).
    pub const fn slot(self) -> Slot {
        self.0 as Slot
    }

    /// `slot` as an id, or `None` for a negative slot.
    pub fn from_slot(slot: Slot) -> Option<Self> {
        u16::try_from(slot).ok().map(ObjectId)
    }
}

/// The object tables, the object list and its cursors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Objects {
    /// Original: A4+0x190c (status byte per slot: bits 3-4 where, 0x01 ok to take, 0x02
    /// invisible, 0x20 temporary, 0x40 broken / egg active, 0x80 lit; objlist 0x0000 fills the
    /// actor slots)
    pub obj_status: Vec<u8>,
    /// Original: 0x135588 (3 bytes per slot: x 10 bits, y 10 bits, z 4 bits; for a contained
    /// object bytes 0-1 hold the parent slot; objlist 0x0100 fills the actor slots)
    pub location: Vec<Position>,
    /// Original: 0x137988 (word per slot: type 10 bits, frame 6 bits; 0 = free slot; objlist
    /// 0x0400 fills the actor slots)
    pub type_frame: Vec<u16>,
    /// Original: A4+0x250c (word per slot: high byte quality, low byte quantity; a gold or
    /// other word-count stack uses the whole word; objlist 0x0600 fills the actor slots)
    ///
    /// The aggregate's loader writes objlist 0x0600 into the actor slots.
    pub qq: Vec<u16>,
    /// Original: A4-0x1184 (word per slot: next slot in the object list or a free chain, -1 =
    /// end; slot 0x100 is the list head)
    pub next: Vec<Slot>,
    /// Original: A4+0x67c (40 × 40 words: first list entry at each window tile, -1 = none)
    pub grid: Vec<Slot>,
    /// Original: A4-0x2ef8 / A4-0x2ef6 (words: x, y of the window's top-left tile)
    ///
    /// Written by map 0x108d10 (window rebuild) and read here; map calls
    /// [`Objects::set_window`] instead of keeping its own copy.
    pub window: [u16; 2],
    /// Original: A4-0x796c (word: 0x3ff on the surface, 0xff in a dungeon; set with the window)
    pub coord_mask: u16,
    /// Original: A4+0x12fc / A4+0x12fe (first free object slot, free object count)
    pub free: (Slot, i16),
    /// Original: A4+0x1300 / A4+0x1302 (first free temporary-actor slot, free count)
    pub free_actors: (Slot, i16),
    /// Location cursor of 0x10ad1a / 0x10aaca.
    pub at: LocationCursor,
    /// Child cursor of 0x10ae52 / 0x10aed6 / 0x10b18c / 0x10b0ba.
    pub children: ChildCursor,
    /// Area cursor of 0x10b05a / 0x10af74.
    pub area: AreaCursor,
    /// Original: A4-0x78de (word: while set, 0x10aed6 / 0x10b0ba skip readied objects)
    pub skip_readied: u16,
    /// Original: A4-0x78da (word: while set, 0x10cda8 may move type 0x4c)
    pub allow_type_4c: u16,
    /// Original: A4-0x782c (word: last child 0x10c1d6 moved out of the removed object, -1)
    pub last_moved_child: Slot,
    /// Original: A4-0x784c (4 words: surface superchunks in slots, -1 = empty)
    pub loaded_surface: [i16; 4],
    /// Original: A4-0x7844 (word: top-left superchunk of the loaded 2 × 2, -1 = none)
    pub surface_base: i16,
    /// Original: A4-0x7842 (word: level in slots, 0 = surface, -1 = none)
    pub loaded_level: i16,
    /// The `savegame/objblk??` files (surface `AA..HH`, dungeons `AI..EI`) in
    /// [`ObjBlockId::all`] order; 0x10ddd2 / 0x10e10c read one when its `.tmp` flag is clear.
    pub blocks: Vec<(ObjBlockId, ObjBlock)>,
    /// The `savegame/objblk??.tmp` files 0x10d538 writes (file bytes), same order (`None` = no
    /// file).
    pub tmp: Vec<Option<Vec<u8>>>,
    /// Original: 0x13c940 (0xc00 × 8 bytes: the record buffer block files are read into and
    /// written from)
    pub rec: Vec<u8>,
    /// Original: 0x142940 (0xc00 words: slot of each record in the buffer)
    pub rec_slots: Vec<Slot>,
    /// Original: A4-0x78d8 (69 words: the block's `.tmp` file is current; dungeon z at 63 + z)
    pub tmp_flag: Vec<u16>,
    /// Original: A4-0x637a (word: 0x53 'S' = saving, 0x52 'R' = block files frozen, else play;
    /// initial value from the data hunk)
    pub save_mode: u16,
    /// Original: A4-0x7af8 (word: 0x10d538 sets it after a save wrote the blocks)
    pub saved_blocks: u16,
    /// Map-window draw lists built by 0x10e81e.
    pub draw: DrawLists,
    /// Tables read from the user's `game` file ([`ObjectDefs::from_game`]).
    pub defs: ObjectDefs,
}

impl Default for Objects {
    fn default() -> Self {
        Self {
            obj_status: vec![0; OBJECT_SLOTS],
            location: vec![Position::default(); OBJECT_SLOTS],
            type_frame: vec![0; OBJECT_SLOTS],
            qq: vec![0; OBJECT_SLOTS],
            next: vec![NONE; OBJECT_SLOTS],
            grid: vec![NONE; GRID_SIDE * GRID_SIDE],
            window: [0; 2],
            coord_mask: 0x3ff,
            free: (NONE, 0),
            free_actors: (NONE, 0),
            at: LocationCursor::default(),
            children: ChildCursor::default(),
            area: AreaCursor::default(),
            skip_readied: 0,
            allow_type_4c: 0,
            last_moved_child: NONE,
            loaded_surface: [-1; 4],
            surface_base: -1,
            loaded_level: -1,
            blocks: Vec::new(),
            tmp: vec![None; BLOCK_COUNT],
            rec: vec![0; blocks::MAX_RECORDS * 8],
            rec_slots: vec![0; blocks::MAX_RECORDS],
            tmp_flag: vec![0; BLOCK_COUNT],
            save_mode: 0,
            saved_blocks: 0,
            draw: DrawLists::default(),
            defs: ObjectDefs::default(),
        }
    }
}

/// Index of a slot word into the tables, `None` for -1 or past the end.
fn ix(slot: Slot) -> Option<usize> {
    usize::try_from(slot).ok().filter(|&i| i < OBJECT_SLOTS)
}

impl Objects {
    /// Location of `id`, or `None` past the table.
    pub fn location(&self, id: ObjectId) -> Option<Position> {
        self.location.get(id.index()).copied()
    }

    /// Object type (`obj_n`, 10 bits) of `id`.
    pub fn obj_type(&self, id: ObjectId) -> u16 {
        self.type_frame.get(id.index()).map_or(0, |w| w & 0x3ff)
    }

    /// Frame (6 bits) of `id`.
    pub fn frame(&self, id: ObjectId) -> u8 {
        self.type_frame
            .get(id.index())
            .map_or(0, |w| (w >> 10) as u8)
    }

    /// Status byte of `slot` (0 for -1).
    pub fn status(&self, slot: Slot) -> u8 {
        ix(slot).map_or(0, |i| self.obj_status[i])
    }

    /// Status bits 3-4 of `slot`: 0 = on the map.
    pub fn place(&self, slot: Slot) -> u8 {
        self.status(slot) & WHERE_MASK
    }

    /// `next` of `slot` (-1 for -1).
    pub fn next_of(&self, slot: Slot) -> Slot {
        ix(slot).map_or(NONE, |i| self.next[i])
    }

    /// Sets `next` of `slot`; a write through -1 is dropped.
    pub(crate) fn set_next(&mut self, slot: Slot, to: Slot) {
        if let Some(i) = ix(slot) {
            self.next[i] = to;
        }
    }

    /// Type + frame word of `slot` (0 for -1).
    pub fn tf(&self, slot: Slot) -> u16 {
        ix(slot).map_or(0, |i| self.type_frame[i])
    }

    /// Type (10 bits) of `slot`.
    pub fn kind(&self, slot: Slot) -> u16 {
        self.tf(slot) & 0x3ff
    }

    /// Quality / quantity word of `slot`.
    pub fn qq_of(&self, slot: Slot) -> u16 {
        ix(slot).map_or(0, |i| self.qq[i])
    }

    /// The three location bytes of `slot` as the original stores them.
    pub fn packed(&self, slot: Slot) -> [u8; 3] {
        ix(slot).map_or([0; 3], |i| self.location[i].to_packed())
    }

    /// Location bytes 0-1 as a word, `b1 << 8 | b0`: the parent slot of a contained object.
    pub fn parent(&self, slot: Slot) -> Slot {
        let [b0, b1, _] = self.packed(slot);
        (u16::from(b1) << 8 | u16::from(b0)) as Slot
    }

    /// x (10 bits) of `slot`.
    pub fn x(&self, slot: Slot) -> u16 {
        ix(slot).map_or(0, |i| self.location[i].x)
    }

    /// y (10 bits) of `slot`.
    pub fn y(&self, slot: Slot) -> u16 {
        ix(slot).map_or(0, |i| self.location[i].y)
    }

    /// z (4 bits) of `slot`.
    pub fn z(&self, slot: Slot) -> u8 {
        ix(slot).map_or(0, |i| self.location[i].z)
    }

    /// Original: 0x134e62 (store a parent slot in location bytes 0-1, byte 2 kept)
    pub(crate) fn set_parent(&mut self, slot: Slot, parent: Slot) {
        if let Some(i) = ix(slot) {
            let [_, _, b2] = self.location[i].to_packed();
            let [lo, hi] = (parent as u16).to_le_bytes();
            self.location[i] = Position::from_packed([lo, hi, b2]);
        }
    }

    /// Original: 0x134e1c (store x, y, z in the location bytes)
    pub(crate) fn set_xyz(&mut self, slot: Slot, x: u16, y: u16, z: u16) {
        if let Some(i) = ix(slot) {
            self.location[i] = pack_xyz(x, y, z);
        }
    }

    /// Reads [`Objects::defs`] from the user's `game` executable (`files`), for the aggregate's
    /// loader to call.
    pub fn load_defs(&mut self, files: &crate::files::Files) -> Result<(), DefsError> {
        let game = files
            .get("game")
            .ok_or_else(|| DefsError("game: not found in the dropped files".into()))?;
        self.defs = ObjectDefs::from_game(game)?;
        Ok(())
    }

    /// Writes the window origin and coordinate mask the way map 0x108d10 does, then rebuilds
    /// the grid (0x10dea4). Map's 0x108d10 port calls this instead of keeping a copy.
    pub fn set_window(&mut self, x: u16, y: u16, z: u8) {
        self.window = [x, y];
        self.coord_mask = if z == 0 { 0x3ff } else { 0xff };
        self.rebuild_grid();
    }
}

/// Original: 0x134e1c (pack x, y, z into the 3 location bytes with the 68000's byte arithmetic)
///
/// Byte 0 = x low byte, byte 1 = x >> 8 | y << 2, byte 2 = y >> 6 | z << 4, each truncated to a
/// byte: an x of 1024 sets y's lowest bit, as on the original.
pub fn pack_xyz(x: u16, y: u16, z: u16) -> Position {
    let b0 = x as u8;
    let b1 = ((x >> 8) | (y << 2)) as u8;
    let b2 = ((y >> 6) | (z << 4)) as u8;
    Position::from_packed([b0, b1, b2])
}

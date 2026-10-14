//! Objects: the object tables, iteration, move/remove, names; commands L/G/D/M/U (WU-P3).
//!
//! Domain state = [`Objects`]: identity (slot index [`ObjectId`]), location, type/frame, and the
//! object-block records that carry containment until the region loader (0x10e1ce) places them
//! into slots. Actors are objects 0..256; `actors/` refers to them by [`ObjectId`] and never
//! keeps its own copy of a position.
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
//! - 0x1319b2 [objects] uses "Shamino says, "I hear somet~"

use u6_formats::objblk::{ObjBlock, ObjBlockId};
use u6_formats::position::Position;

/// Object slots in the location / type tables [likely]: 0x137988 − 0x135588 = 0x2400 bytes =
/// 3072 × 3, and 0x139188 − 0x137988 = 0x1800 bytes = 3072 × 2.
pub const OBJECT_SLOTS: usize = 3072;
/// Slots 0..256 are actors.
pub const ACTOR_SLOTS: usize = 256;

/// An object slot (index into the object tables); actors are 0..256.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub u16);

impl ObjectId {
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// The object tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Objects {
    /// Original: A4+0x190c (byte per actor slot; objlist 0x0000)
    pub actor_flags: Vec<u8>,
    /// Original: 0x135588 (3 bytes per object: x 10 bits, y 10 bits, z 4 bits; objlist 0x0100
    /// fills the actor slots)
    pub location: Vec<Position>,
    /// Original: 0x137988 (word per object: type 10 bits, frame 6 bits; objlist 0x0400 fills the
    /// actor slots)
    pub type_frame: Vec<u16>,
    /// The `savegame/objblk??` files as last read or written (surface `AA..HH`, dungeons
    /// `AI..EI`), in [`ObjBlockId::all`] order. The original reads them per region (0x10e1ce →
    /// 0x10ddd2 / 0x10e10c); each record carries its own containment (status bits + position).
    pub blocks: Vec<(ObjBlockId, ObjBlock)>,
}

impl Default for Objects {
    fn default() -> Self {
        Self {
            actor_flags: vec![0; ACTOR_SLOTS],
            location: vec![Position::default(); OBJECT_SLOTS],
            type_frame: vec![0; OBJECT_SLOTS],
            blocks: Vec::new(),
        }
    }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_frame_word_splits_10_and_6_bits() {
        let mut o = Objects::default();
        o.type_frame[3] = 0x3ff | (0x2a << 10);
        assert_eq!(o.obj_type(ObjectId(3)), 0x3ff);
        assert_eq!(o.frame(ObjectId(3)), 0x2a);
        assert_eq!(o.obj_type(ObjectId(OBJECT_SLOTS as u16)), 0);
        assert_eq!(o.location(ObjectId(OBJECT_SLOTS as u16)), None);
    }
}

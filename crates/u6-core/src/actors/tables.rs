//! Game tables the actor code reads from the data hunk of `game` (spec 03 §5: extracted from the
//! user's executable at run time, never compiled in).
//!
//! The data hunk (hunk 33) is the SAS/C small-data section: A4 = hunk start + 0x8000; the tables
//! are read at A4 + `off` through the shared hunk loader [`crate::image::GameImage`].
//! [`GameTables::from_game`] validates the file with the palette check (length, hunk magic,
//! hunk 33 header) first; `State::new_game` loads it. [`GameTables::default`] is empty: every
//! lookup then behaves as "not listed" (see each accessor).

use u6_formats::FormatError;
use u6_formats::gfx;

use crate::image::GameImage;

/// A4 offset of the creature-type word list (0-terminated).
const CREATURE_TYPES: i32 = -0x8000;
/// A4 offset of the creature flag words, parallel to the type list.
const CREATURE_FLAGS: i32 = -0x7dbe;
/// Room for the type list before the flag table starts.
const CREATURE_MAX: usize = ((CREATURE_FLAGS - CREATURE_TYPES) / 2) as usize;
/// A4 offset of the four object types worktypes look for (0x11dd56, 0x1200b0 worktype 0x82).
const WORK_OBJECTS: i32 = -0x6aa6;
/// A4 offset of the moon table: two bytes per day of the month, indexed by day × 2.
const MOONS: i32 = -0x7ac5;
/// A4 offset of the frames of the four parts of the 0x19b creature, 4 per facing.
const PART_FRAMES: i32 = -0x6a70;
/// A4 offset of the four tile-flag bits that mark the ship's bow / stern parts by facing.
const SHIP_SIDES: i32 = -0x6a60;
/// A4 offset of the 8-way x deltas (y deltas follow 8 bytes later); 0x11f804 indexes it with a
/// whole path byte, so 256 + 8 bytes are kept.
const DELTAS: i32 = -0x61ce;
/// A4 offset of the follow formation: 11 sideways offsets, then (from -0x6a7b) the
/// distances behind the leader, indexed by follower rank (the original reads up to rank 16,
/// so the sideways table runs into the second one).
const FORMATION: i32 = -0x6a86;
/// A4 offset of the sailing table: 8 headings per current direction (8 × 8 bytes).
const SAIL: i32 = -0x69fc;
/// A4 offsets of the move costs of the ship (by angle to the wind) and of the raft (by angle
/// to the current).
const SHIP_COST: i32 = -0x6a0c;
const RAFT_COST: i32 = -0x6a04;
/// Days in a month (the clock wraps after 28).
const DAYS: usize = 28;

/// Tables the actor code reads, as extracted from `game`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameTables {
    /// Original: A4-0x8000 (0-terminated word list of creature object types; 0x131196 returns
    /// an index into it)
    pub creature_types: Vec<u16>,
    /// Original: A4-0x7dbe (big-endian flag word per creature index; bits read by the actor
    /// code: 0x0100 flies, 0x0200 swims, 0x0400 moves over anything but its own kind, 0x4000
    /// opens doors; low byte (hazards, 0x102f3c): 0x01 sleep-proof, 0x04 poison-proof, 0x08
    /// fire-proof, 0x10 takes double fire damage)
    pub creature_flags: Vec<u16>,
    /// Original: A4-0x6aa6 (four object types: what a worktype looks for at its location, in
    /// the order 0x11dd56's kind argument selects)
    pub work_objects: [u16; 4],
    /// Original: A4-0x7ac5 (two moon-phase bytes per day, entry `day` at byte `day * 2`)
    pub moons: Vec<u8>,
    /// Original: A4-0x6a70 (16 bytes: part frames of the 0x19b creature, index (dir & 6) × 2 +
    /// part)
    pub part_frames: [u8; 16],
    /// Original: A4-0x6a60 (4 bytes: flags1 bit of a ship end, by facing)
    pub ship_sides: [u8; 4],
    /// Original: A4-0x61ce (x deltas of the 8 directions, y deltas at +8, then whatever data
    /// follows: 264 bytes, as 0x11f804 can read them)
    pub deltas: Vec<u8>,
    /// Original: A4-0x6a86 (28 bytes: sideways offset of follower rank r at r, distance behind
    /// at 11 + r)
    pub formation: Vec<u8>,
    /// Original: A4-0x69fc (64 bytes: heading of a sailing boat, index current × 8 + wind)
    pub sail: Vec<u8>,
    /// Original: A4-0x6a0c (8 bytes: ship move cost by (heading − wind) & 7)
    pub ship_cost: [u8; 8],
    /// Original: A4-0x6a04 (8 bytes: raft move cost by (heading − current) & 7)
    pub raft_cost: [u8; 8],
}

impl GameTables {
    /// Extracts the tables from the PowerPacker-decrunched `game` executable through the shared
    /// hunk loader [`GameImage`].
    pub fn from_game(game: &[u8]) -> Result<Self, FormatError> {
        // Same version check as the palette: length, hunk magic, hunk 33 header.
        gfx::palette(game)?;
        let img = GameImage::parse(game)?;
        let bytes = |a4: i32, n: usize| img.a4_bytes(a4, n);
        let word = |a4: i32, i: usize| -> Result<u16, FormatError> {
            let b = img.a4_bytes(a4 + 2 * i as i32, 2)?;
            Ok(u16::from_be_bytes([b[0], b[1]]))
        };
        let mut creature_types = Vec::new();
        for i in 0..CREATURE_MAX {
            let t = word(CREATURE_TYPES, i)?;
            if t == 0 {
                break;
            }
            creature_types.push(t);
        }
        let creature_flags = (0..creature_types.len())
            .map(|i| word(CREATURE_FLAGS, i))
            .collect::<Result<_, _>>()?;
        let work_objects = [
            word(WORK_OBJECTS, 0)?,
            word(WORK_OBJECTS, 1)?,
            word(WORK_OBJECTS, 2)?,
            word(WORK_OBJECTS, 3)?,
        ];
        let moons = bytes(MOONS, 2 * (DAYS + 1))?.to_vec();
        let mut part_frames = [0u8; 16];
        part_frames.copy_from_slice(bytes(PART_FRAMES, 16)?);
        let mut ship_sides = [0u8; 4];
        ship_sides.copy_from_slice(bytes(SHIP_SIDES, 4)?);
        let deltas = bytes(DELTAS, 264)?.to_vec();
        let formation = bytes(FORMATION, 28)?.to_vec();
        let sail = bytes(SAIL, 64)?.to_vec();
        let mut ship_cost = [0u8; 8];
        ship_cost.copy_from_slice(bytes(SHIP_COST, 8)?);
        let mut raft_cost = [0u8; 8];
        raft_cost.copy_from_slice(bytes(RAFT_COST, 8)?);
        Ok(Self {
            deltas,
            formation,
            sail,
            ship_cost,
            raft_cost,
            creature_types,
            creature_flags,
            work_objects,
            moons,
            part_frames,
            ship_sides,
        })
    }

    /// What 0x131196 (objects/) computes on this list: the index of `obj_type`, -1 when it is
    /// not listed (also when the tables were not loaded). Used as the fallback of
    /// [`super::seams::creature_index`].
    pub fn creature_index(&self, obj_type: u16) -> i16 {
        self.creature_types
            .iter()
            .position(|&t| t == obj_type)
            .map_or(-1, |i| i as i16)
    }

    /// Flag word of creature `index`; the original reads A4-0x7dbe + 2 × index with no bound,
    /// so index -1 reads the word before the table (the list's terminator, 0): 0 here as well.
    pub fn creature_flags(&self, index: i16) -> u16 {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.creature_flags.get(i).copied())
            .unwrap_or(0)
    }

    /// Byte `i` of the delta window (signed); falls back to the 8-way table for the first 16
    /// bytes when not loaded, 0 past them.
    pub fn delta_byte(&self, i: usize) -> i16 {
        if let Some(&b) = self.deltas.get(i) {
            return i16::from(b as i8);
        }
        let dirs = [0, 1, 1, 1, 0, -1, -1, -1, -1, -1, 0, 1, 1, 1, 0, -1];
        dirs.get(i).copied().unwrap_or(0)
    }

    /// Sideways offset of follower rank `r` (0 when not loaded).
    pub fn side(&self, r: usize) -> i16 {
        self.formation.get(r).map_or(0, |&b| i16::from(b as i8))
    }

    /// Distance behind the leader of follower rank `r` (0 when not loaded).
    pub fn back(&self, r: usize) -> i16 {
        self.formation
            .get(11 + r)
            .map_or(0, |&b| i16::from(b as i8))
    }

    /// Moon byte `k` (0 or 1) of `day`; 0 when not loaded.
    pub fn moon(&self, day: u8, k: usize) -> u8 {
        self.moons
            .get(usize::from(day) * 2 + k)
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups_on_empty_tables_say_not_listed() {
        let t = GameTables::default();
        assert_eq!(t.creature_index(0x17a), -1);
        assert_eq!(t.creature_flags(-1), 0);
        assert_eq!(t.moon(3, 1), 0);
    }

    #[test]
    fn creature_index_is_the_list_position() {
        let t = GameTables {
            creature_types: vec![0x10, 0x20, 0x30],
            creature_flags: vec![1, 2, 4],
            ..GameTables::default()
        };
        assert_eq!(t.creature_index(0x30), 2);
        assert_eq!(t.creature_flags(t.creature_index(0x20)), 2);
        assert_eq!(t.creature_index(0x40), -1);
    }

    #[test]
    fn a_non_game_file_is_rejected() {
        assert!(GameTables::from_game(&[0u8; 64]).is_err());
    }
}

//! Game tables the combat code reads from the data hunk of `game` (spec 03 §5: extracted from
//! the user's executable at run time, never compiled in).
//!
//! Every table is read at `A4 + off` through the shared hunk loader
//! [`crate::image::GameImage`] (BSS reads 0). [`CombatTables::default`] is empty: lookups then
//! answer "not listed" (no range bonus, no damage, no armour, distance 9), so a state built
//! without `game` still runs.

use u6_formats::FormatError;

use crate::geom::RULE_HALF;
use crate::image::{A4, GameImage};

/// Original: A4-0x69a0 (8 × 8 bytes: distance by |dx| row, |dy| column; 9 past it)
const DISTANCE: i32 = -0x69a0;
/// Original: A4-0x6836 (18 words: object types with a range) / A4-0x6812 (18 bytes: the range)
const RANGE_TYPES: i32 = -0x6836;
const RANGE_VALUES: i32 = -0x6812;
const RANGE_COUNT: usize = 0x12;
/// Original: A4-0x6800 (37 words: weapon types) / A4-0x67b6 (37 bytes: their damage)
const WEAPON_TYPES: i32 = -0x6800;
const WEAPON_DAMAGE: i32 = -0x67b6;
const WEAPON_COUNT: usize = 0x25;
/// Original: A4-0x6884 (26 words: armour types) / A4-0x6850 (26 bytes: their protection)
const ARMOUR_TYPES: i32 = -0x6884;
const ARMOUR_VALUES: i32 = -0x6850;
const ARMOUR_COUNT: usize = 0x1a;
/// Original: A4-0x7f7e, -0x7f3e, -0x7efe, -0x7ebe, -0x7e7e, -0x7e3e (64 bytes each, by creature
/// index: the six creature stats; -0x7e7e is the natural attack damage)
const CREATURE_STATS: [i32; 6] = [-0x7f7e, -0x7f3e, -0x7efe, -0x7ebe, -0x7e7e, -0x7e3e];
const CREATURES: usize = 0x40;
/// Original: A4-0x7dfe (64 bytes: a new creature's status (alignment bits 0x60), 0x131646)
const CREATURE_STATUS: i32 = -0x7dfe;
/// Original: A4-0x7dbe (64 big-endian words: creature flags)
const CREATURE_FLAGS: i32 = -0x7dbe;
/// Original: A4-0x6790 / A4-0x6768 (0-terminated word lists: what creature 0 and creature 0x17
/// take from a victim, 0x123a7c)
const TAKE_LISTS: [i32; 2] = [-0x6790, -0x6768];
/// Original: A4-0x68e2 (word pairs, -1-terminated: actor type → the type + frame it lies as,
/// 0x1221da)
const LYING: i32 = -0x68e2;
/// Original: A4-0x64f4 - 0x80 (17 rows × 16 bytes: missile angle by dy (-8..8) and dx (-7..8))
const ANGLES: i32 = -0x64f4 - 0x80;
/// Original: A4-0x6464 (18 words: missile tiles that have a flight kind) and the per-kind
/// bytes A4-0x63f8 (frame step), -0x6440 (angle offset), -0x641c / -0x640a (sprite centre x /
/// y), -0x642e (angle added per step: spinning)
const MISSILE_TILES: i32 = -0x6464;
const MISSILE_STEP: i32 = -0x63f8;
const MISSILE_ANGLE: i32 = -0x6440;
const MISSILE_CX: i32 = -0x641c;
const MISSILE_CY: i32 = -0x640a;
const MISSILE_SPIN: i32 = -0x642e;
const MISSILE_KINDS: usize = 0x12;
/// Original: A4-0x6576, -0x6574, -0x6572, -0x6566 (words: the overview colours of 0x134044)
const PEER_COLOURS: [i32; 4] = [-0x6576, -0x6574, -0x6572, -0x6566];
/// Original: A4-0x6752 (bytes: tangle vine frames by facing × 2 + direction / 2, 0x126de8)
const VINE_FRAMES: i32 = -0x6752;
/// Original: A4-0x63e6 / A4-0x63d6 (16 signed bytes each: where the 16 sprites of a blast fly,
/// 0x1331b4)
const BLAST_DX: i32 = -0x63e6;
const BLAST_DY: i32 = -0x63d6;
/// Original: A4-0x6960 (11 × 11 bytes: the triple crossbow's side bolts by target cell (dy + 5,
/// dx + 5); high and low nibble = the directions of the two side bolts)
const SPREAD: i32 = -0x6960;
/// Side of the spread table (11): the original's targeting box round the leader, ±[`RULE_HALF`]
/// (spec 13 Decision 4), in both builds.
pub const SPREAD_SIDE: usize = 2 * RULE_HALF as usize + 1;

/// Tables the combat code reads, as extracted from `game`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CombatTables {
    /// A4-0x69a0: 64 bytes, `distance[dx * 8 + dy]`.
    pub distance: Vec<u8>,
    /// A4-0x6836 / A4-0x6812: (type, range) of the objects with a range.
    pub ranges: Vec<(u16, u8)>,
    /// A4-0x6800 / A4-0x67b6: (type, damage) of the weapons.
    pub weapons: Vec<(u16, u8)>,
    /// A4-0x6884 / A4-0x6850: (type, protection) of the armour.
    pub armour: Vec<(u16, u8)>,
    /// A4-0x7f7e.. -0x7e3e: six stat bytes per creature index.
    pub creature_stats: [Vec<u8>; 6],
    /// A4-0x7dfe: a new creature's status byte.
    pub creature_status: Vec<u8>,
    /// A4-0x7dbe: creature flag words.
    pub creature_flags: Vec<u16>,
    /// A4-0x6790 / A4-0x6768: the two take lists (without the terminating 0).
    pub take: [Vec<u16>; 2],
    /// A4-0x68e2: (type, lying type + frame) pairs.
    pub lying: Vec<(u16, u16)>,
    /// A4-0x64f4 - 0x80: 0x110 angle bytes.
    pub angles: Vec<u8>,
    /// A4-0x6464 and the per-kind bytes.
    pub missile_tiles: Vec<u16>,
    pub missile_step: Vec<u8>,
    pub missile_angle: Vec<u8>,
    pub missile_cx: Vec<u8>,
    pub missile_cy: Vec<u8>,
    pub missile_spin: Vec<u8>,
    /// A4-0x6929: 121 bytes.
    pub spread: Vec<u8>,
    /// A4-0x6576 / -0x6574 / -0x6572 / -0x6566: overview colours (special ground, other ground,
    /// shallow and deep water).
    pub peer_colours: [i16; 4],
    /// A4-0x6752: tangle vine frames (16 + 3 bytes as 0x126de8 indexes them).
    pub vine_frames: Vec<u8>,
    /// A4-0x63e6 / A4-0x63d6: the blast offsets.
    pub blast_dx: Vec<u8>,
    pub blast_dy: Vec<u8>,
}

fn words(img: &GameImage<'_>, off: i32, n: usize) -> Result<Vec<u16>, FormatError> {
    let b = img.read_zero_filled(A4.wrapping_add_signed(off), n * 2)?;
    Ok(b.chunks(2)
        .map(|w| u16::from_be_bytes([w[0], w[1]]))
        .collect())
}

fn bytes(img: &GameImage<'_>, off: i32, n: usize) -> Result<Vec<u8>, FormatError> {
    img.read_zero_filled(A4.wrapping_add_signed(off), n)
}

fn zero_list(img: &GameImage<'_>, off: i32) -> Result<Vec<u16>, FormatError> {
    let w = words(img, off, 0x20)?;
    Ok(w.into_iter().take_while(|&t| t != 0).collect())
}

impl CombatTables {
    /// Reads every table out of the user's `game` file.
    pub fn from_game(game: &[u8]) -> Result<Self, FormatError> {
        let img = GameImage::parse(game)?;
        let pairs = |t: i32, v: i32, n: usize| -> Result<Vec<(u16, u8)>, FormatError> {
            let t = words(&img, t, n)?;
            let v = bytes(&img, v, n)?;
            Ok(t.into_iter().zip(v).collect())
        };
        let lying_raw = words(&img, LYING, 0x40)?;
        let lying = lying_raw
            .chunks(2)
            .take_while(|p| (p[0] as i16) >= 0)
            .map(|p| (p[0], p[1]))
            .collect();
        let stat = |i: usize| bytes(&img, CREATURE_STATS[i], CREATURES);
        Ok(Self {
            distance: bytes(&img, DISTANCE, 64)?,
            ranges: pairs(RANGE_TYPES, RANGE_VALUES, RANGE_COUNT)?,
            weapons: pairs(WEAPON_TYPES, WEAPON_DAMAGE, WEAPON_COUNT)?,
            armour: pairs(ARMOUR_TYPES, ARMOUR_VALUES, ARMOUR_COUNT)?,
            creature_stats: [stat(0)?, stat(1)?, stat(2)?, stat(3)?, stat(4)?, stat(5)?],
            creature_status: bytes(&img, CREATURE_STATUS, CREATURES)?,
            creature_flags: words(&img, CREATURE_FLAGS, CREATURES)?,
            take: [
                zero_list(&img, TAKE_LISTS[0])?,
                zero_list(&img, TAKE_LISTS[1])?,
            ],
            lying,
            angles: bytes(&img, ANGLES, 0x110)?,
            missile_tiles: words(&img, MISSILE_TILES, MISSILE_KINDS)?,
            missile_step: bytes(&img, MISSILE_STEP, MISSILE_KINDS)?,
            missile_angle: bytes(&img, MISSILE_ANGLE, MISSILE_KINDS)?,
            missile_cx: bytes(&img, MISSILE_CX, MISSILE_KINDS)?,
            missile_cy: bytes(&img, MISSILE_CY, MISSILE_KINDS)?,
            missile_spin: bytes(&img, MISSILE_SPIN, MISSILE_KINDS)?,
            spread: bytes(&img, SPREAD, SPREAD_SIDE * SPREAD_SIDE)?,
            peer_colours: [
                words(&img, PEER_COLOURS[0], 1)?[0] as i16,
                words(&img, PEER_COLOURS[1], 1)?[0] as i16,
                words(&img, PEER_COLOURS[2], 1)?[0] as i16,
                words(&img, PEER_COLOURS[3], 1)?[0] as i16,
            ],
            vine_frames: bytes(&img, VINE_FRAMES, 0x14)?,
            blast_dx: bytes(&img, BLAST_DX, 16)?,
            blast_dy: bytes(&img, BLAST_DY, 16)?,
        })
    }

    /// A4-0x69a0[|dx|][|dy|]: 0 when the table is empty.
    pub fn distance_at(&self, ax: usize, ay: usize) -> u8 {
        self.distance.get(ax * 8 + ay).copied().unwrap_or(0)
    }

    /// Stat `n` (0..6) of creature index `i`; 0 past the tables.
    pub fn creature_stat(&self, n: usize, i: i16) -> u8 {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.creature_stats.get(n)?.get(i).copied())
            .unwrap_or(0)
    }

    /// Flag word of creature index `i`. For -1 (not a creature) the original reads the word
    /// before the table: the last two status bytes (A4-0x7dc0); 0 elsewhere off the table.
    pub fn flags(&self, i: i16) -> u16 {
        if i == -1 {
            let b = |k: usize| u16::from(self.creature_status.get(k).copied().unwrap_or(0));
            return b(CREATURES - 2) << 8 | b(CREATURES - 1);
        }
        usize::try_from(i)
            .ok()
            .and_then(|i| self.creature_flags.get(i).copied())
            .unwrap_or(0)
    }

    /// Missile angle of a (dx, dy) step, 0 outside the table.
    pub fn angle(&self, dx: i16, dy: i16) -> u8 {
        let i = (i32::from(dy) + 8) * 16 + i32::from(dx) + 7;
        usize::try_from(i)
            .ok()
            .and_then(|i| self.angles.get(i).copied())
            .unwrap_or(0)
    }
}

/// A byte of a per-kind table, 0 past it.
pub(crate) fn at(t: &[u8], i: usize) -> u8 {
    t.get(i).copied().unwrap_or(0)
}

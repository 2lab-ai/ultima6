//! Tables the inventory code reads from the original's data hunk, taken from the user's own
//! `game` at run time (spec 03 §5): only the A4 offsets live here. Strings are read when printed
//! ([`crate::main_loop::game_string`]).

use crate::image::GameImage;

/// Hunks of the measured `game` and the base of hunk 33 (the merged data hunk).
const GAME_HUNKS: usize = 35;
const HUNK33_BASE: u32 = 0x0015_36f0;

/// Original: A4-0x720c (33 words: the tiles 0x110f96 readies in the weapon hand, slot 2)
const HAND_TILES: (i32, usize) = (-0x720c, 33);
/// Original: A4-0x71ca (9 words: object types 0x1110c8 names; 0x1110f6 weighs them per ten)
const COUNTED: (i32, usize) = (-0x71ca, 9);
/// Original: A4-0x6740 (14 words: food types, 0x127278)
const FOOD: (i32, usize) = (-0x6740, 14);
/// Original: A4-0x7a0a (24 words: 3 × 5 glyph bits, top row first, of the digits 0x1059d4 draws;
/// entries past 9 are the circle numbers of type 0x40)
const GLYPHS: (i32, usize) = (-0x7a0a, 24);
/// Original: A4-0x79da (5 × 4 bytes: x of each digit, units first, for 0..4 digits, 0x105a3e)
const COLUMNS: (i32, usize) = (-0x79da, 20);
/// Original: A4-0x6724 / -0x671c / -0x6714 (8 bytes each: camp place dx, dy and facing of party
/// member `i`, 0x127b1a). Read as one block of 48 bytes: the original indexes each table by
/// member up to the party size, so members past 8 read the next table.
const CAMP: (i32, usize) = (-0x6724, 48);
/// Original: A4-0x7a26 (2 bytes: x, y of the pointer origin, initial value)
const POINTER_ORIGIN: i32 = -0x7a26;

/// The tables, typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InventoryTables {
    pub hand_tiles: Vec<u16>,
    pub counted: Vec<u16>,
    pub food: Vec<u16>,
    pub glyphs: Vec<u16>,
    pub columns: Vec<u8>,
    pub camp: Vec<u8>,
    pub pointer_origin: (u8, u8),
}

impl InventoryTables {
    /// Reads every table from the decrunched `game` executable; a file whose hunk layout differs
    /// from the measured one is refused (the offsets would not hold).
    pub fn from_game(game: &[u8]) -> Result<Self, String> {
        let img = GameImage::parse(game).map_err(|e| e.to_string())?;
        if img.segment_count() != GAME_HUNKS || img.segment_base(33) != Some(HUNK33_BASE) {
            return Err("unknown game version (hunk layout differs from the measured one)".into());
        }
        let bytes = |(off, n): (i32, usize)| -> Result<Vec<u8>, String> {
            img.a4_bytes(off, n)
                .map(<[u8]>::to_vec)
                .map_err(|e| e.to_string())
        };
        let words = |(off, n): (i32, usize)| -> Result<Vec<u16>, String> {
            Ok(bytes((off, 2 * n))?
                .chunks_exact(2)
                .map(|w| u16::from_be_bytes([w[0], w[1]]))
                .collect())
        };
        let origin = bytes((POINTER_ORIGIN, 2))?;
        Ok(Self {
            hand_tiles: words(HAND_TILES)?,
            counted: words(COUNTED)?,
            food: words(FOOD)?,
            glyphs: words(GLYPHS)?,
            columns: bytes(COLUMNS)?,
            camp: bytes(CAMP)?,
            pointer_origin: (origin[0], origin[1]),
        })
    }

    /// Byte `i` of the camp block (0 past its end).
    pub fn camp_byte(&self, i: usize) -> i8 {
        self.camp.get(i).map_or(0, |&b| b as i8)
    }
}

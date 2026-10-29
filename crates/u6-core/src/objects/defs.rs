//! Tables the object code reads from the original's data, taken from the user's own `game`
//! executable at run time (spec 03 §5 "data, not constants"). Only the image addresses live here.
//!
//! Locating them: the decrunched `game` is an AmigaDOS hunk file, mapped by the shared hunk
//! loader [`crate::image::GameImage`] (hunks back to back from image address 0x100000, each base
//! rounded up to 8 bytes; game-layout.md). Small-data offsets `A4 + d` are image address
//! 0x15b6f0 + d (hunk 33 + 0x8000). Bytes past a hunk's loaded length are BSS and read as 0. A
//! file whose hunk layout differs from the measured one is refused: the addresses would not hold.

use std::fmt;

use crate::image::GameImage;

/// Small-data base register A4 (hunk 33 + 0x8000).
pub const A4: u32 = crate::image::A4;
/// Hunks in the measured `game` and the image address of hunk 33 (the merged data hunk).
const GAME_HUNKS: usize = 35;
const HUNK33_BASE: u32 = 0x0015_36f0;

/// The `game` file does not have the layout the table addresses were measured on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefsError(pub String);

impl fmt::Display for DefsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "game executable: {}", self.0)
    }
}

impl std::error::Error for DefsError {}

fn err<T>(what: impl Into<String>) -> Result<T, DefsError> {
    Err(DefsError(what.into()))
}

/// The loaded image of a `game` file, read as the program sees it (BSS reads 0).
pub(crate) struct Image<'a> {
    img: GameImage<'a>,
}

impl<'a> Image<'a> {
    /// Maps the hunk file (the shared loader); any malformed block is refused.
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self, DefsError> {
        let img = GameImage::parse(bytes).map_err(|e| DefsError(e.to_string()))?;
        Ok(Self { img })
    }

    /// `len` bytes at image address `addr` (BSS bytes read as 0). Fails outside every hunk.
    pub(crate) fn read(&self, addr: u32, len: usize) -> Result<Vec<u8>, DefsError> {
        self.img
            .read_zero_filled(addr, len)
            .map_err(|e| DefsError(e.to_string()))
    }

    /// The NUL-terminated string at `addr` (at most 256 bytes, Latin-1).
    pub(crate) fn string(&self, addr: crate::string_registry::Text) -> Result<String, DefsError> {
        addr.latin1(|at, n| self.read(at, n))
    }

    /// Checks the layout the addresses below were measured on.
    pub(crate) fn check_game(&self) -> Result<(), DefsError> {
        let i = &self.img;
        if i.segment_count() != GAME_HUNKS || i.segment_base(33) != Some(HUNK33_BASE) {
            return err("unknown game version (hunk layout differs from the measured one)");
        }
        Ok(())
    }

    /// Image base of hunk `n`.
    #[cfg(test)]
    fn base(&self, n: usize) -> Option<u32> {
        self.img.segment_base(n)
    }
}

/// `A4 + d` as an image address.
pub(crate) const fn a4(d: i32) -> u32 {
    (A4 as i32 + d) as u32
}

/// Original: A4-0x77ce (21 words: object types 0x10cef2 counts by the quantity byte)
const STACKABLE: i32 = -0x77ce;
/// Original: A4-0x61ce / A4-0x61c6 (8 signed bytes each: dx / dy of the 8 directions N, NE, E,
/// SE, S, SW, W, NW). Read 64 bytes each because callers index them by a 6-bit frame.
const DIR_DX: i32 = -0x61ce;
const DIR_DY: i32 = -0x61c6;
/// Original: A4-0x77ec (bytes, indexed by direction 0..7: the frame-flag mask a silver-serpent
/// segment keeps, 0x10b42e), A4-0x77e5 (bytes, indexed by a 4-bit flag set: the segment frame),
/// A4-0x77d5 (bytes, indexed by direction: the flag bit pointing that way, 0x10b9ca)
const SEG_MASK: i32 = -0x77ec;
const SEG_FRAME: i32 = -0x77e5;
const DIR_BIT: i32 = -0x77d5;
/// Original: A4-0x6a70 (16 bytes: frame of the dragon part at each direction, 0x10b922)
const PART_FRAME: i32 = -0x6a70;
/// Original: A4-0x670c (9 words: object types 0x12899c accepts)
const TYPE_SET_670C: i32 = -0x670c;
/// Original: A4-0x8000 (words: monster types, 0-terminated; 0x131196)
const MONSTER_TYPES: i32 = -0x8000;
/// Original: A4-0x637a (word: save mode, initial value)
const SAVE_MODE: i32 = -0x637a;
/// Image addresses of the strings 0x10ee76 / 0x10eef8 return and the article / count prefixes
/// of 0x12904c / 0x129382 (code hunks 5 and 15).
const STR_NOTHING_LOWER: u32 = 0x0010_a4c6;
const STR_NOTHING: u32 = 0x0010_a4ce;
const STR_A: u32 = 0x0012_83e2;
const STR_AN: u32 = 0x0012_83e6;
const STR_THE: u32 = 0x0012_83ea;
const STR_COUNT: u32 = 0x0012_83fe;

/// The tables and strings, typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectDefs {
    /// Types counted by their quantity byte (0x10cef2 kind 2); type 0x58 (gold) is coded apart.
    pub stackable: Vec<u16>,
    pub dir_dx: Vec<i8>,
    pub dir_dy: Vec<i8>,
    pub seg_mask: Vec<u8>,
    pub seg_frame: Vec<u8>,
    pub dir_bit: Vec<u8>,
    pub part_frame: Vec<u8>,
    pub type_set_670c: Vec<u16>,
    /// Monster types 0x131196 indexes (0-terminated list at A4-0x8000).
    pub monster_types: Vec<u16>,
    /// Initial value of the save-mode word.
    pub save_mode: u16,
    /// The nothing word in lower case (0x10ee76 for an empty look string) and capitalised
    /// (0x10eef8 for slot -1).
    pub nothing_lower: String,
    pub nothing: String,
    /// The article prefixes of 0x12904c (flags3 bits 6-7 = 1, 2, 3) and the count format of
    /// 0x129382.
    pub articles: [String; 3],
    pub count_format: String,
}

impl ObjectDefs {
    /// Reads every table from the decrunched `game` executable.
    pub fn from_game(game: &[u8]) -> Result<Self, DefsError> {
        let img = Image::parse(game)?;
        img.check_game()?;
        let bytes = |d: i32, n: usize| img.read(a4(d), n);
        let words = |d: i32, n: usize| -> Result<Vec<u16>, DefsError> {
            Ok(bytes(d, 2 * n)?
                .chunks_exact(2)
                .map(|w| u16::from_be_bytes([w[0], w[1]]))
                .collect())
        };
        let signed = |v: Vec<u8>| v.into_iter().map(|b| b as i8).collect();
        Ok(Self {
            stackable: words(STACKABLE, 21)?,
            dir_dx: signed(bytes(DIR_DX, 64)?),
            dir_dy: signed(bytes(DIR_DY, 64)?),
            seg_mask: bytes(SEG_MASK, 32)?,
            seg_frame: bytes(SEG_FRAME, 16)?,
            dir_bit: bytes(DIR_BIT, 16)?,
            part_frame: bytes(PART_FRAME, 16)?,
            type_set_670c: words(TYPE_SET_670C, 9)?,
            monster_types: words(MONSTER_TYPES, 64)?,
            save_mode: words(SAVE_MODE, 1)?[0],
            nothing_lower: img.string(crate::string_registry::text!(STR_NOTHING_LOWER))?,
            nothing: img.string(crate::string_registry::text!(STR_NOTHING))?,
            articles: [
                img.string(crate::string_registry::text!(STR_A))?,
                img.string(crate::string_registry::text!(STR_AN))?,
                img.string(crate::string_registry::text!(STR_THE))?,
            ],
            count_format: img.string(crate::string_registry::text!(STR_COUNT))?,
        })
    }

    /// dx of direction `d` (the original indexes past 8 with a frame; 0 past the read window).
    pub fn dx(&self, d: usize) -> i16 {
        self.dir_dx.get(d).map_or(0, |&v| i16::from(v))
    }

    /// dy of direction `d`.
    pub fn dy(&self, d: usize) -> i16 {
        self.dir_dy.get(d).map_or(0, |&v| i16::from(v))
    }
}

/// Byte `i` of a table, 0 past its end.
pub(crate) fn at(table: &[u8], i: usize) -> u8 {
    table.get(i).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE_BASE: u32 = crate::image::IMAGE_BASE;

    /// A two-hunk executable: CODE (8 bytes) and BSS (4 longwords).
    fn tiny() -> Vec<u8> {
        let mut code = vec![0x41, 0x42, 0x43, 0];
        code.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        crate::image::SyntheticGame::new()
            .loaded(false, code)
            .bss(16)
            .build()
    }

    #[test]
    fn hunks_are_placed_back_to_back_and_bss_reads_zero() {
        let f = tiny();
        let img = Image::parse(&f).unwrap();
        assert_eq!(
            img.string(crate::string_registry::fixture((IMAGE_BASE) as i64, false))
                .unwrap(),
            "ABC"
        );
        assert_eq!(
            img.read(IMAGE_BASE + 4, 4).unwrap(),
            [0x11, 0x22, 0x33, 0x44]
        );
        assert_eq!(img.read(IMAGE_BASE + 8, 2).unwrap(), [0, 0]);
        assert!(img.read(IMAGE_BASE + 8 + 16, 1).is_err());
        assert_eq!(img.base(1), Some(IMAGE_BASE + 8));
        assert!(img.check_game().is_err());
        let mut bad = f.clone();
        bad[3] = 0xf4;
        assert!(Image::parse(&bad).is_err());
    }
}

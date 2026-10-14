//! The spell tables of the data hunk, read from the user's `game` at load (spec 03 §5): names,
//! incantations, syllables, reagents, circle names and the result messages. Each is a table of
//! longword pointers (or words) at an A4 offset; the pointers are resolved through the hunk
//! relocations and the strings read where they point (hunk 11, 0x1153d0..0x115876).

use crate::image::{A4, GameImage};

/// Spell numbers: circle × 16 + index, circles 0..7.
pub const SPELLS: usize = 0x80;

/// A4-0x6f4c: name pointer per spell number.
const NAMES: u32 = 0x6f4c;
/// A4-0x71b4: incantation pointer per spell (letters `A`..`Z`, one per syllable).
const WORDS: u32 = 0x71b4;
/// A4-0x70b8: syllable pointer per character code (`A` = 0x41 .. `Z`).
const SYLLABLES: u32 = 0x70b8;
/// A4-0x6d1c: reagent bit mask per spell (bit n = reagent n).
const MASKS: u32 = 0x6d1c;
/// A4-0x6d2c: object type of each of the 8 reagents (words).
const REAGENTS: u32 = 0x6d2c;
/// A4-0x6c9c: circle name per circle (the ordinals 1..8).
const CIRCLES: u32 = 0x6c9c;
/// A4-0x6c7c: the four alignment names.
const ALIGNMENTS: u32 = 0x6c7c;
/// A4-0x6c6c: the result messages, `Msg` order.
const MESSAGES: u32 = 0x6c6c;
/// A4-0x6c38: 0x4e words, the tiles 0x115228 counts as magical.
const MAGIC_TILES: u32 = 0x6c38;
/// A4-0x6b54: four words, the door types 0x1165a4 accepts.
const DOORS: u32 = 0x6b54;
/// A4-0x6b4c: potion description per potion frame (8 pointers).
const POTIONS: u32 = 0x6b4c;

/// The result messages at A4-0x6c6c + 4 n.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    Success = 0,
    Failed,
    NoEffect,
    OutOfRange,
    Blocked,
    Nothing,
    NotPossible,
    Done,
    OnWhat,
    OnWhom,
    Location,
    NotUsable,
    Amulet,
}

/// The tables, decoded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MagicTables {
    pub names: Vec<Vec<u8>>,
    pub words: Vec<Vec<u8>>,
    /// Syllable of each letter `A`..`Z` (index letter - 0x41).
    pub syllables: Vec<Vec<u8>>,
    pub masks: Vec<u8>,
    pub reagents: [u16; 8],
    pub circles: Vec<Vec<u8>>,
    pub alignments: Vec<Vec<u8>>,
    pub messages: Vec<Vec<u8>>,
    pub magic_tiles: Vec<u16>,
    pub doors: [u16; 4],
    pub potions: Vec<Vec<u8>>,
}

fn strings(img: &GameImage<'_>, off: u32, n: usize) -> Result<Vec<Vec<u8>>, String> {
    (0..n)
        .map(|i| {
            let at = A4.wrapping_sub(off) + 4 * i as u32;
            let p = img.pointer(at).map_err(|e| format!("{at:#x}: {e}"))?;
            img.cstr(p).map_err(|e| format!("{p:#x}: {e}"))
        })
        .collect()
}

fn words<const N: usize>(img: &GameImage<'_>, off: u32) -> Result<[u16; N], String> {
    let mut out = [0u16; N];
    for (i, w) in out.iter_mut().enumerate() {
        *w = img
            .a4_word(-(off as i32) + 2 * i as i32)
            .map_err(|e| format!("A4-{off:#x}: {e}"))?;
    }
    Ok(out)
}

impl MagicTables {
    /// Reads every table from the user's `game` (the decrunched hunk file).
    pub fn from_game(game: &[u8]) -> Result<Self, String> {
        let img = GameImage::parse(game).map_err(|e| format!("game: {e}"))?;
        let syllables = strings(&img, SYLLABLES - 4 * 0x41, 26)?;
        let masks = img
            .a4_bytes(-(MASKS as i32), SPELLS)
            .map_err(|e| format!("A4-{MASKS:#x}: {e}"))?
            .to_vec();
        let magic_tiles = (0..0x4e)
            .map(|i| img.a4_word(-(MAGIC_TILES as i32) + 2 * i))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("A4-{MAGIC_TILES:#x}: {e}"))?;
        Ok(Self {
            names: strings(&img, NAMES, SPELLS)?,
            words: strings(&img, WORDS, SPELLS)?,
            syllables,
            masks,
            reagents: words::<8>(&img, REAGENTS)?,
            circles: strings(&img, CIRCLES, 8)?,
            alignments: strings(&img, ALIGNMENTS, 4)?,
            messages: strings(&img, MESSAGES, 13)?,
            magic_tiles,
            doors: words::<4>(&img, DOORS)?,
            potions: strings(&img, POTIONS, 8)?,
        })
    }

    /// The name of spell `n` (empty past the table).
    pub fn name(&self, n: usize) -> &[u8] {
        self.names.get(n).map_or(&[], Vec::as_slice)
    }

    /// The incantation letters of spell `n`.
    pub fn word(&self, n: usize) -> &[u8] {
        self.words.get(n).map_or(&[], Vec::as_slice)
    }

    /// The syllable of letter `c` (`A`..`Z`).
    pub fn syllable(&self, c: u8) -> &[u8] {
        let i = usize::from(c.wrapping_sub(0x41));
        self.syllables.get(i).map_or(&[], Vec::as_slice)
    }

    /// The reagent mask of spell `n` (0 past the table).
    pub fn mask(&self, n: usize) -> u8 {
        self.masks.get(n).copied().unwrap_or(0)
    }

    pub fn message(&self, m: Msg) -> &[u8] {
        self.messages.get(m as usize).map_or(&[], Vec::as_slice)
    }
}

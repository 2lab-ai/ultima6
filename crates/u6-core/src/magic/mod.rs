//! Spellcasting (hunk 11, 0x115228..0x11b1c0, and the spellbook code of hunk 2): the Cast
//! command, the spellbook panel, the spell syllable input, the reagents, mana and level checks,
//! the spell dispatcher and the spell effects.
//!
//! Flow (read off the code): `C` starts the spell cursor (mode 3) and calls [`cast`] (0x11af10),
//! which finds the readied spellbooks and runs [`book_cast`] (0x11abd0): the book's spells are
//! collected ([`load_book`], 0x115da4), the book is drawn in the side panel ([`draw_book`],
//! 0x116028 / 0x116218, panel mode 0xa1) and one key read follows. Keys typed meanwhile go to
//! the spell input ([`spell_key`], part of 0x106bbc, with [`find_typed_spell`] 0x10674e) or the
//! panel keys ([`book_panel_key`], [`page_key`], [`book_cursor_step`] 0x106812). Return chooses
//! the spell; the level, magic points and reagents are checked, the reagents and points spent
//! (0x11615a, 0x11c650) and [`dispatch`] (0x11a586) runs the spell, which prompts for its
//! target through the cross-hair where it needs one.
//!
//! The spells: one target (effects.rs, 0x116f2c), the detections (detect.rs), the routines of
//! their own (spells.rs), the location spells (area.rs), the creature spells (creatures.rs),
//! the object spells (items.rs) and the travel and view spells (travel.rs); Peer's overview
//! 0x12c54a is objects::commands' port (shared with Use on a gem).
//!
//! Domain state = [`MagicState`] (the hunk-11 BSS and the spell-input bytes of the ui BSS that
//! only this code reads); the tables are [`MagicTables`], read from the user's `game` at load
//! (the few small A4 tables the spells read at run time come through `seams::a4_bytes`).
//! Calls into code that other units own (combat damage and missiles, the flights, a few ui,
//! inventory and object routines) go through [`seams`].
//!
//! Original functions this module owns: every function of hunk 11 (see `re/notes/port-map.md`
//! for each one's status) plus 0x10674e, 0x1067d8, 0x106812, 0x1078d2 and the spell-input
//! block of 0x106bbc; ported here because the spells are their only callers on the spell
//! side: 0x13331c, 0x1334a6 and 0x133562 (hunk 18).

mod area;
mod book;
mod cast;
mod creatures;
mod detect;
mod dispatch;
mod effects;
mod input;
mod items;
pub(crate) mod seams;
mod spells;
pub mod tables;
mod travel;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_guards;
#[cfg(test)]
mod tests_real;
#[cfg(test)]
mod tests_spells;
#[cfg(test)]
mod tests_units;

pub(crate) use book::{book_bitmap, book_file};
pub use book::{draw_book, draw_book_page, load_book, set_current_spell};
pub use cast::{book_cast, cast};
pub use detect::{carried_object, is_door};
pub use dispatch::dispatch;
pub use effects::{heal, sleep, strength_contest};
pub use input::{
    book_cursor_step, book_panel_key, find_typed_spell, page_key, spell_key, takes_book_key,
};
pub use spells::{WINDOW_FLASH_MS, flash_window, resurrect, resurrect_and_rejoin, xray};
pub use tables::{MagicTables, Msg};
pub use travel::{locate, wizard_eye};

/// Port-only (spec 08, slice K2; 2026-10-24): the strings the spell code passes to `printf` as
/// its format (`print_at`, the direct `printf`s, the refusals), each file's list next to its
/// addresses, for the locale layer's catalogue ([`crate::locale::strings`]). A format of no word
/// of its own (the result formats, the newlines, the announcement's, the book's number, the
/// direction's and Locate's) and a string under three bytes the catalogue leaves out: they stay
/// as they are. The incantation's syllables (printed as a format, the game's magic words) are
/// not listed: they stay English. The spellbook page's level word, drawn at a fixed cell, is
/// listed apart with its cells ([`PAGE_PRINTED`], 2026-10-25), and the names the spells put
/// through `%s` (the spells', the circles') are labels ([`NAME_LABELS`], [`CIRCLE_LABELS`]).
pub(crate) const PRINTED: [&[u32]; 10] = [
    book::PRINTED,
    cast::PRINTED,
    creatures::PRINTED,
    detect::PRINTED,
    dispatch::PRINTED,
    effects::PRINTED,
    input::PRINTED,
    items::PRINTED,
    spells::PRINTED,
    travel::PRINTED,
];

/// Port-only (spec 08, slice K2): the result messages', the potions' and the direction names'
/// labels for the locale layer's catalogue.
pub(crate) use tables::LABELS;

/// Port-only (spec 08, 2026-10-25): the spellbook page's strings for the locale layer's
/// catalogue, each within its field there: the level word, a `printf` format within its
/// English's cells ([`book::PAGE_PRINTED`]); the spells' names, labels within
/// [`tables::NAME_CELLS`], and the circles' names, labels within [`tables::CIRCLE_CELLS`]. Cast's
/// announcement and Detect Magic's charge line read the same names.
pub(crate) use book::PAGE_PRINTED;
pub(crate) use tables::{CIRCLE_CELLS, CIRCLE_LABELS, NAME_CELLS, NAME_LABELS};

/// Spellbook object type.
pub const SPELLBOOK: u16 = 0x39;
/// Spell object type (the spells inside a book; quality = spell number).
pub const SPELL: u16 = 0x3a;
/// The panel mode of the open spellbook.
pub const BOOK_PANEL: u16 = 0xa1;

/// The globals of the spell code.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MagicState {
    pub tables: MagicTables,
    /// Original: A4+0x17f4 / A4+0x17f6 (words: the first and second readied spellbook, 0 =
    /// none)
    pub books: [i16; 2],
    /// Original: A4+0x17f8 (8 bytes: the spell number on each line of the shown page)
    pub page: [u8; 8],
    /// Original: A4+0x1800 (8 bytes: casts the carried reagents allow for each line; bit 7
    /// marks the current spell's line)
    pub page_reagents: [u8; 8],
    /// Original: A4+0x1808 (byte: count of the book's spells before the shown page, 0xff =
    /// unset)
    pub page_first: u8,
    /// Original: A4+0x1809 (byte: lines on the shown page)
    pub page_lines: u8,
    /// Original: A4+0x180a (8 words: bit n of word c = spell c × 16 + n is in the books)
    pub known: [u16; 8],
    /// Original: A4+0x181a (byte: circle of the previous page's last spell, 0xff = none)
    pub prev_circle: u8,
    /// Original: A4+0x181b (byte: previous page: 1 = an earlier circle, 2 = more of this one,
    /// 0xff = none)
    pub prev_page: u8,
    /// Original: A4+0x181c (byte: next page: 1 = a later circle, 2 = more of this one, 0xff =
    /// none)
    pub next_page: u8,
    /// Original: A4+0x181d (byte: circle of the next page's first spell, 0xff = none)
    pub next_circle: u8,
    /// Original: A4+0x181e (byte: result of a spell: 0 success, 1 failed, 2 no effect, 0xff /
    /// 0xfe printed already)
    pub result: u8,
    /// Original: A4+0x181f (byte: the current spell, the books' quality)
    pub current: u8,
    /// Original: A4+0x1822 (word: panel mode to restore after the book closes)
    pub saved_panel: u16,
    /// Original: A4-0x7a3a (long: bit n = letter `A` + n typed as a syllable)
    pub typed_mask: u32,
    /// Original: A4-0x7a3c (byte: syllables typed)
    pub typed: u8,
    /// Original: A4-0x3022 (4 bytes: the letters typed)
    pub typed_letters: [u8; 4],
    /// Original: A4-0x301e / A4-0x301a (4 bytes each: message-window column and row where
    /// each typed syllable starts)
    pub typed_at: [(u8, u8); 4],
    /// Original: A4-0x7a5c (byte: a page step moved the book; the next redraw selects the
    /// first line)
    pub page_moved: u8,
}

impl MagicState {
    /// The current spell number (the byte A4+0x181f, widened as the code compares it).
    pub fn current_spell(&self) -> u16 {
        u16::from(self.current)
    }
}

//! Port-only (spec 08 Design 2, slice K2): the catalogue of the executable's strings that game
//! code shows, read from the user's `game` at run time, and the string export of it.
//!
//! **What is catalogued.** The sites the engine knows (each module lists the strings it prints,
//! next to their addresses): the command loop's prompts and results
//! ([`crate::main_loop::commands::PRINTED`]), Look, Get, Drop, Move and the keys'
//! ([`crate::objects::commands::PRINTED`]), Use's ([`crate::usecmd::PRINTED`]), Rest's
//! ([`crate::actors::rest::PRINTED`]), Attack's ([`crate::combat::attack::PRINTED`]), combat's
//! other lines (2026-10-24: a hit's graze, death line, specials and wound report
//! ([`crate::combat::damage::PRINTED`]), and the refusal of a staff's or a wand's cast
//! ([`crate::combat::seams::PRINTED`])), the inventory's Ready and Unready lines (2026-10-24: the
//! Ready line, a torch's, the three refusals, the Unready line and a burned-out torch's
//! ([`crate::inventory::PRINTED`]), and the locked chest's line of a click in the inventory
//! panel ([`crate::ui::click::PRINTED`])), magic's lines (2026-10-24: Cast's and the books'
//! refusals, the result formats, every spell's own lines and prompts and the typed spell's
//! not-in-book line ([`crate::magic::PRINTED`])), the lines the actors' and objects' own code
//! prints (the clock's with the status area's date line ([`crate::actors::clock::PRINTED`]), the
//! world turn's ([`crate::actors::turn::PRINTED`]), the party's
//! ([`crate::actors::party::PRINTED`]), the NPC turn's ([`crate::actors::npc::PRINTED`]), the
//! step test's ([`crate::actors::walk::PRINTED`]), the passthroughs' and the object blocks'
//! ([`crate::objects::PRINTED`])), the panels' (2026-10-24: the stats view's attribute lines and
//! its magic, health and level headings ([`crate::ui::tables::PRINTED`]), each within its cells;
//! 2026-10-25: the spellbook page's level word ([`crate::magic::PAGE_PRINTED`]), within its
//! English's cells) and the conversation's own messages
//! ([`crate::converse::text::SHOWN`]): each one game code passes to `printf` as its format. And
//! the strings a loader table holds that game code shows through an escape (labels): the command
//! names before their dash ([`crate::main_loop::commands::LABELS`]), the shared messages Use and
//! the object commands print through `%s` ([`SHARED_MESSAGES`]), the amulet's line of that table
//! Unready prints through `%s` (2026-10-24, [`crate::inventory::LABELS`]), the spells' result
//! messages (that table's 13), potion descriptions and direction names they print through `%s`
//! (2026-10-24, [`crate::magic::LABELS`]), the spells' and the circles' names the spellbook
//! page, Cast's announcement, Detect Magic's charge line and Look's description of a spell print
//! through `%s` (2026-10-25, [`crate::magic::NAME_LABELS`] and [`crate::magic::CIRCLE_LABELS`],
//! each within its field on the page), the reagents' names that description prints through `%s`
//! (2026-10-25, [`crate::objects::commands::LABELS`], each on a wrapping line: no budget), the
//! wind's names the date line prints through `%s`
//! ([`crate::actors::clock::LABELS`]), the combat-mode names the panels' line prints through
//! `%s` (2026-10-24, [`crate::ui::tables::LABELS`], each within the line's field), and the
//! conversation's `$G`, `$T` and `$Y` words. And (2026-10-24) the inventory view's two weight
//! labels, which it reads for display at their own addresses and puts with no lookup again
//! ([`crate::ui::tables::SHOWN_AT`]): labels from 1 byte (each is 2 in the original), within
//! its English's cells. So the panels are catalogued (2026-10-24): the stats, inventory and
//! combat-mode lines, and (2026-10-25) the spellbook page's spell names, circle name and level
//! word, each within its cells ([`Entry::cells`]); their name line and values hold no word of
//! their own. Every other string of the executable is not catalogued: the spells' incantation
//! syllables (the game's magic words, which stay English), object names and articles. It is
//! substituted only where its bytes are a catalogued string's, as the lookup is by content: a
//! string two sites print is catalogued once (magic's not-possible line is the string of the
//! staff's refusal, the one address 0x1159f8 in both [`crate::combat::seams::PRINTED`] and
//! [`crate::magic::PRINTED`]); a direction name, read through [`super::shown`] wherever it is
//! shown (the spells', Move's, Dig's and the telescope's lines, a scout's warning), follows the
//! pack everywhere. A string shorter than [`MIN_BYTES`], or with no word of its own (`%s\n`, a
//! name and a space), is not catalogued either; but (2026-10-21) a label read for display
//! through [`super::shown`] (a pointer table's, or a panel label at its own address,
//! [`crate::ui::tables::SHOWN_AT`]) is catalogued from 1 byte (a wind's name is 2, a weight
//! label 2). The conversation's words keep the minimum: its display reads them as the print
//! hooks do.
//!
//! **Lookup.** By content: [`hash64`] of the English bytes, only where game code prints them
//! (`printf`'s format, `puts`, `put_raw_str`; [`super::exe_string`], [`Catalogue::of`]) or reads
//! a label for display (a pointer table's, or a panel label at its own address,
//! [`crate::ui::tables::SHOWN_AT`]; [`super::shown`], [`Catalogue::label_of`]); never on what an
//! escape generates, and never where logic reads a string. A string under [`MIN_BYTES`] is
//! found only at such a label read: the same bytes printed as a format stay English. A string
//! with a cell budget ([`Entry::cells`]) takes the pack's text only within it (2026-10-24, the
//! lookup's check).
//!
//! **The export** ([`Catalogue::to_json`], [`FORMAT`]): every catalogued string once, in the
//! order the sites list them: its hash, its length, its kind and its text (one character per
//! byte, as the NPC bundles write theirs). Slice K3 adds, compatibly (left out when empty, the
//! format's name unchanged), `texts`: the object names of `look.lzd` and the books and signs of
//! `book.dat` ([`super::texts`]), each with its kind (`look`, `book`), its index, hash, length and
//! text. Slice K4 adds, compatibly as well, `u`: the strings the `u` program prints (the opening,
//! the story, the gypsy, character creation and the transfer; [`super::program`]), each with its
//! kind (its role there: `opening`, `story`, `gypsy`, `creation`, `transfer`), hash, length,
//! text and (when a key read follows it) the keys that read takes. A string drawn at a fixed cell
//! (2026-10-24, a panel's: [`Entry::cells`]) carries, compatibly as well, `cells`: the cells its
//! translation takes at most, written only when set, so every other entry is as before. It holds
//! the user's game text: it is made in the user's browser and leaves it only as his own download
//! (`tests/no_string_bundle_in_repo.rs`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::converse::ConverseText;
use crate::converse::disasm::catalog::{hash64, hex64};
use crate::image::{A4, GameImage};

/// The string export's format name.
pub const FORMAT: &str = "u6.string-bundle/1";
/// The locale of the export (the original's English).
pub const LOCALE: &str = "en";
/// Strings shorter than this are never substituted (spec 08 Design 2), but a label read for
/// display ([`Catalogue::label_of`]).
pub const MIN_BYTES: usize = 3;
/// The longest string the game reads ([`GameImage::cstr`]'s bound).
pub const MAX_BYTES: usize = 256;
/// The files the export reads, by bare name (the executable; slice K3: the object names and the
/// books; slice K4: the `u` program).
pub const READS: [&str; 4] = [
    "game",
    super::texts::LOOK_FILE,
    super::texts::BOOK_FILE,
    super::program::FILE,
];

/// The pointers of the shared messages Use and the object commands read for display (A4 - off:
/// success, failed, no effect, out of range, blocked, nothing, not possible, done, on whom, not
/// usable; `crate::usecmd::defs` `MSG_*`, `objects::commands` `P_*`, the entries the code names).
pub const SHARED_MESSAGES: [u32; 10] = [
    A4.wrapping_sub(0x6c6c),
    A4.wrapping_sub(0x6c68),
    A4.wrapping_sub(0x6c64),
    A4.wrapping_sub(0x6c60),
    A4.wrapping_sub(0x6c5c),
    A4.wrapping_sub(0x6c58),
    A4.wrapping_sub(0x6c54),
    A4.wrapping_sub(0x6c50),
    A4.wrapping_sub(0x6c48),
    A4.wrapping_sub(0x6c40),
];

/// How game code shows a catalogued string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// `printf`'s format with no conversion: a line as it is.
    Message,
    /// `printf`'s format with a conversion (`%d`, `%s`, ...): its `%` directives are kept.
    Format,
    /// A label read for display: a word a pointer table holds, shown through an escape inside
    /// other text (`%s`, `$n`), or (2026-10-24) a panel label at its own address
    /// ([`crate::ui::tables::SHOWN_AT`]), put as it is.
    Label,
}

/// Whether `bytes` (a `printf` format) holds a conversion: `%` then flags, digits and one of
/// `c d o s u x` (either case).
fn has_conversion(bytes: &[u8]) -> bool {
    directives(bytes).iter().any(|d| {
        d.last()
            .is_some_and(|c| b"cdosux".contains(&c.to_ascii_lowercase()))
    })
}

/// The `%` directives of `format` in order, each its bytes from the `%` through the byte that
/// ends it, as `printf` reads them with `wide` on (`ui::printf::printf`): `%`, then `N$` (N from
/// 1, its first digit not 0) when it is there, then `-` and `0` flags, then digits, then one more
/// byte (a conversion, or a byte put as it is: `%%`, `%>`, `%/`); a format that ends after the
/// `%` ends the directive there. A NUL ends the format.
pub fn directives(format: &[u8]) -> Vec<Vec<u8>> {
    let f = &format[..format.iter().position(|&b| b == 0).unwrap_or(format.len())];
    let mut out = Vec::new();
    let mut i = 0;
    while i < f.len() {
        if f[i] != b'%' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        // `N$`: digits from 1-9 then `$`, else nothing taken.
        if f.get(i).is_some_and(|b| (b'1'..=b'9').contains(b)) {
            let mut j = i;
            while f.get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            if f.get(j) == Some(&b'$') {
                i = j + 1;
            }
        }
        while f.get(i).is_some_and(|&b| b == b'-' || b == b'0') {
            i += 1;
        }
        while f.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        i = (i + 1).min(f.len());
        out.push(f[start..i].to_vec());
    }
    out
}

/// Whether `text` holds a letter outside its `%` directives ([`directives`]) and the letters an
/// escape takes (`$n`, `#n`, `^c`, the plural letters after `/` and `\\`).
fn has_words(text: &[u8]) -> bool {
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == b'%' {
            i += directives(&text[i..]).first().map_or(1, Vec::len);
            continue;
        }
        if matches!(c, b'$' | b'#' | b'^') {
            i += 2;
            continue;
        }
        if matches!(c, b'/' | b'\\') {
            i += 1;
            while text.get(i).is_some_and(u8::is_ascii_alphabetic) {
                i += 1;
            }
            continue;
        }
        if c.is_ascii_alphabetic() {
            return true;
        }
        i += 1;
    }
    false
}

/// The cells the English `text` takes drawn in place (2026-10-24, [`Entry::cells`]): its bytes, a
/// byte a cell, the `%` directives counted as written.
fn english_cells(text: &[u8]) -> u16 {
    u16::try_from(text.len()).unwrap_or(u16::MAX)
}

/// One catalogued string: its English bytes and how it is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    pub text: Vec<u8>,
    /// Port-only (2026-10-24): the cells its translation takes at most, for a string drawn at a
    /// fixed cell (a panel's: [`crate::ui::tables::PRINTED`]'s and
    /// [`crate::ui::tables::SHOWN_AT`]'s the English's cells, its bytes with the `%` directives
    /// as written; a combat-mode name [`crate::ui::tables::MODE_CELLS`]; 2026-10-25, the
    /// spellbook page's: its level word, [`crate::magic::PAGE_PRINTED`], the English's cells, a
    /// spell's name [`crate::magic::NAME_CELLS`], a circle's [`crate::magic::CIRCLE_CELLS`]);
    /// `None` for one with no such place. Two sites of the same bytes keep the smaller.
    pub cells: Option<u16>,
}

/// The catalogued strings of one `game`, by [`hash64`] of their bytes, in site order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalogue {
    by_hash: BTreeMap<u64, usize>,
    entries: Vec<(u64, Entry)>,
}

/// One string of the export.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportString {
    /// [`hash64`] of its bytes, 16 lower-case hex digits: its key in the pack.
    #[serde(with = "hex64")]
    pub hash: u64,
    /// Its length in bytes.
    pub len: u32,
    pub kind: Kind,
    /// Its bytes, one character per byte (U+0000..U+00FF).
    pub text: String,
    /// 2026-10-24: its cell budget ([`Entry::cells`]), only when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cells: Option<u16>,
}

/// The export ([`FORMAT`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub format: String,
    pub locale: String,
    pub strings: Vec<ExportString>,
    /// Slice K3: the object names and books ([`super::texts::export`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<super::texts::ExportText>,
    /// Slice K4: the strings of the `u` program ([`super::program::export`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub u: Vec<super::program::ExportString>,
}

impl Export {
    /// The export as JSON (one object).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("the export has only serialisable fields")
    }

    /// Reads an export back (another format's name is an error, as is a string or a text whose
    /// text, length and hash disagree, or a text keyed where no pack can key it): the translation
    /// tool's input, read as the tool reads it. Each refusal is a fixed sentence: nothing of the
    /// input (a field's name, a value) is in it.
    pub fn from_json(s: &str) -> Result<Export, &'static str> {
        let e: Export =
            serde_json::from_str(s).map_err(|_| "it is not JSON of the game strings' form")?;
        if e.format != FORMAT {
            return Err("it is not of the game strings' format (u6.string-bundle/1)");
        }
        for t in &e.strings {
            let bytes: Option<Vec<u8>> = t.text.chars().map(|c| u8::try_from(c).ok()).collect();
            let ok = bytes.is_some_and(|b| b.len() == t.len as usize && hash64(&b) == t.hash);
            if !ok {
                return Err("a string's text, length and hash disagree");
            }
        }
        if !e.texts.iter().all(super::texts::ExportText::reads_back) {
            return Err("a text's index, text, length and hash disagree");
        }
        if !e.u.iter().all(super::program::ExportString::reads_back) {
            return Err("a string of the u program's text, length and hash disagree");
        }
        Ok(e)
    }
}

/// Whether the export reads the file at `path` (by its bare name, ignoring case).
pub fn reads(path: &str) -> bool {
    let bare = path.rsplit(['/', '\\', ':']).next().unwrap_or(path);
    READS.iter().any(|r| bare.eq_ignore_ascii_case(r))
}

/// The export of the catalogue of the `game` among `files` (by its bare name), for the page's
/// download, with (slice K3) the object names and books of `files` ([`super::texts::export`])
/// and (slice K4) the strings of its `u` ([`super::program::export`]). Fails, with a fixed
/// sentence (no value of the file), when there is no `game` or it does not read as the
/// executable the engine knows, or a `look.lzd` or a `u` there does not read.
pub fn export(files: &crate::files::Files) -> Result<Export, &'static str> {
    let game = files
        .get("game")
        .ok_or("the files hold no game executable")?;
    if ConverseText::from_game(game).is_err() {
        return Err("the game executable does not read");
    }
    let mut export = Catalogue::from_game(game).export();
    export.texts = super::texts::export(files)?;
    export.u = super::program::export(files)?;
    Ok(export)
}

/// Tests (2026-10-24): installs over `s` a Korean pack of `forms` (an English string and the
/// pack's text for it, each the test's own), checked against the catalogue of a synthetic `game`
/// holding `parts` (one CODE hunk from the image base: each part's bytes at its image address),
/// as the page installs one. For a module's own test of a label read for display
/// ([`super::shown`]).
#[cfg(test)]
pub(crate) fn install_test_forms(
    s: &mut crate::state::State,
    parts: &[(u32, &[u8])],
    forms: &[(&[u8], &str)],
) {
    let at = |addr: u32| (addr - crate::image::IMAGE_BASE) as usize;
    let end = parts.iter().map(|&(a, b)| at(a) + b.len() + 1).max();
    let mut hunk = vec![0u8; end.unwrap_or(0).next_multiple_of(4)];
    for &(addr, bytes) in parts {
        hunk[at(addr)..at(addr) + bytes.len()].copy_from_slice(bytes);
    }
    let game = crate::image::SyntheticGame::new()
        .loaded(false, hunk)
        .build();
    let strings = forms.iter().map(|&(en, ko)| {
        let text = super::StringText {
            len: en.len() as u32,
            text: ko.as_bytes().to_vec(),
        };
        (hash64(en), text)
    });
    let pack = super::Pack::new(
        crate::ui::Language::Korean,
        "test/1",
        super::fingerprint([]),
        [],
    )
    .and_then(|p| p.with_strings(strings))
    .expect("the test's pack reads");
    let files = crate::files::Files::new(BTreeMap::from([("game".to_string(), game.clone())]));
    super::install_catalogued(
        s,
        &files,
        std::rc::Rc::new(pack),
        None,
        Catalogue::from_game(&game),
    )
    .expect("the pack checks");
}

impl Catalogue {
    /// The catalogue of `game` (the decrunched executable): every site's string that reads
    /// (inside the image, NUL-ended) and is [`MIN_BYTES`] or longer, a label read for display (a
    /// pointer table's, or a panel label at its own address, [`crate::ui::tables::SHOWN_AT`]) 1
    /// byte or longer ([`Catalogue::add_label`]). A file that is no hunk file gives an empty
    /// catalogue (nothing is substituted then).
    pub fn from_game(game: &[u8]) -> Catalogue {
        let mut c = Catalogue::default();
        let Ok(img) = GameImage::parse(game) else {
            return c;
        };
        let printed = crate::main_loop::commands::PRINTED
            .iter()
            .chain(
                crate::objects::commands::PRINTED
                    .iter()
                    .flat_map(|s| s.iter()),
            )
            .chain(crate::usecmd::PRINTED)
            .chain(crate::actors::rest::PRINTED)
            .chain(crate::combat::attack::PRINTED)
            .chain(crate::combat::damage::PRINTED)
            .chain(crate::combat::seams::PRINTED)
            .chain(crate::inventory::PRINTED)
            .chain(crate::ui::click::PRINTED)
            .chain(crate::magic::PRINTED.iter().flat_map(|s| s.iter()))
            .chain(crate::actors::clock::PRINTED)
            .chain(crate::actors::turn::PRINTED)
            .chain(crate::actors::party::PRINTED)
            .chain(crate::actors::npc::PRINTED)
            .chain(crate::actors::walk::PRINTED)
            .chain(crate::objects::PRINTED.iter().flat_map(|s| s.iter()))
            .chain(crate::ui::tables::PRINTED)
            .chain(crate::magic::PAGE_PRINTED);
        for &at in printed {
            if let Ok(text) = img.cstr(at) {
                let kind = if has_conversion(&text) {
                    Kind::Format
                } else {
                    Kind::Message
                };
                // A panel's string (the spellbook page's level word too) is drawn at a fixed
                // cell: the English's cells.
                let cells = (crate::ui::tables::PRINTED.contains(&at)
                    || crate::magic::PAGE_PRINTED.contains(&at))
                .then(|| english_cells(&text));
                c.add_within(kind, text, MIN_BYTES, cells);
            }
        }
        let labels = crate::main_loop::commands::LABELS
            .iter()
            .chain(&SHARED_MESSAGES)
            .chain(crate::objects::commands::LABELS)
            .chain(crate::inventory::LABELS)
            .chain(crate::magic::LABELS)
            .chain(crate::actors::clock::LABELS)
            .chain(crate::ui::tables::LABELS)
            .chain(&crate::magic::NAME_LABELS)
            .chain(&crate::magic::CIRCLE_LABELS);
        for &ptr in labels {
            if let Ok(text) = img.pointer(ptr).and_then(|p| img.cstr(p)) {
                // A combat-mode name is centred in its line's field; a spell's and a circle's
                // name each fill a field of the spellbook page.
                let cells = if crate::ui::tables::LABELS.contains(&ptr) {
                    Some(crate::ui::tables::MODE_CELLS)
                } else if crate::magic::NAME_LABELS.contains(&ptr) {
                    Some(crate::magic::NAME_CELLS)
                } else if crate::magic::CIRCLE_LABELS.contains(&ptr) {
                    Some(crate::magic::CIRCLE_CELLS)
                } else {
                    None
                };
                c.add_label(text, cells);
            }
        }
        // The panel labels read for display at their own addresses, each at a fixed cell.
        for &at in crate::ui::tables::SHOWN_AT {
            if let Ok(text) = img.cstr(at) {
                let cells = english_cells(&text);
                c.add_label(text, Some(cells));
            }
        }
        if let Ok(messages) = ConverseText::from_game(game) {
            for (m, how) in crate::converse::text::SHOWN {
                let kind = match how {
                    crate::converse::text::Shown::Printed if has_conversion(messages.get(m)) => {
                        Kind::Format
                    }
                    crate::converse::text::Shown::Printed => Kind::Message,
                    crate::converse::text::Shown::Label => Kind::Label,
                };
                c.add(kind, messages.get(m).to_vec());
            }
        }
        c
    }

    /// A catalogue of `entries` (tests: a synthetic `game`'s strings), each added as a site's (a
    /// label as a pointer table's, [`Catalogue::add_label`]).
    #[cfg(test)]
    pub(crate) fn from_entries(entries: impl IntoIterator<Item = (Kind, Vec<u8>)>) -> Catalogue {
        Catalogue::from_budgeted(entries.into_iter().map(|(kind, text)| (kind, text, None)))
    }

    /// [`Catalogue::from_entries`] with each site's cell budget (tests, 2026-10-24).
    #[cfg(test)]
    pub(crate) fn from_budgeted(
        entries: impl IntoIterator<Item = (Kind, Vec<u8>, Option<u16>)>,
    ) -> Catalogue {
        let mut c = Catalogue::default();
        for (kind, text, cells) in entries {
            if kind == Kind::Label {
                c.add_label(text, cells);
            } else {
                c.add_within(kind, text, MIN_BYTES, cells);
            }
        }
        c
    }

    /// Adds `text` (once per content: the first site's kind, a label's over the others). A string
    /// with no letter but its directives' and escapes' (`%s\n`, `%d `, `\n%s:\n%>`) has nothing
    /// to translate and is not catalogued: such a format is shared with sites the catalogue does
    /// not hold (the panels' name line and values), which keep it as it is. A string under
    /// [`MIN_BYTES`] is not catalogued.
    fn add(&mut self, kind: Kind, text: Vec<u8>) {
        self.add_within(kind, text, MIN_BYTES, None);
    }

    /// [`Catalogue::add`] of a label read for display (a pointer table's, or a panel label at its
    /// own address, [`crate::ui::tables::SHOWN_AT`]), which game code reads through
    /// [`super::shown`] only (2026-10-21): 1 byte or longer. One under [`MIN_BYTES`] is
    /// found only by [`Catalogue::label_of`], never by the print hooks. With the cell budget of
    /// its site (2026-10-24, [`Catalogue::add_within`]).
    fn add_label(&mut self, text: Vec<u8>, cells: Option<u16>) {
        self.add_within(Kind::Label, text, 1, cells);
    }

    /// [`Catalogue::add`] of a string of `min` bytes or longer, with the cell budget of its site
    /// (2026-10-24, [`Entry::cells`]): the same bytes catalogued again keep the smaller budget, a
    /// site with none bounding nothing.
    fn add_within(&mut self, kind: Kind, text: Vec<u8>, min: usize, cells: Option<u16>) {
        if text.len() < min || text.len() > MAX_BYTES || text.contains(&0) || !has_words(&text) {
            return;
        }
        let h = hash64(&text);
        match self.by_hash.get(&h) {
            Some(&i) => {
                let e = &mut self.entries[i].1;
                if e.text == text {
                    if kind == Kind::Label {
                        e.kind = Kind::Label;
                    }
                    e.cells = match (e.cells, cells) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                }
            }
            None => {
                self.by_hash.insert(h, self.entries.len());
                self.entries.push((h, Entry { kind, text, cells }));
            }
        }
    }

    /// The entry whose bytes hash to `hash`.
    pub fn get(&self, hash: u64) -> Option<&Entry> {
        self.by_hash.get(&hash).map(|&i| &self.entries[i].1)
    }

    /// The entry of exactly `bytes`, if catalogued and [`MIN_BYTES`] or longer: the lookup of
    /// the print hooks (`printf`'s format, `puts`, `put_raw_str`; [`super::exe_string`]).
    pub fn of(&self, bytes: &[u8]) -> Option<&Entry> {
        if bytes.len() < MIN_BYTES {
            return None;
        }
        self.get(hash64(bytes)).filter(|e| e.text == bytes)
    }

    /// The entry of exactly `bytes`, if catalogued, for a label read for display (a pointer
    /// table's, or a panel label at its own address, [`crate::ui::tables::SHOWN_AT`];
    /// [`super::shown`]): as [`Catalogue::of`], and a label under [`MIN_BYTES`] too
    /// (2026-10-21).
    pub fn label_of(&self, bytes: &[u8]) -> Option<&Entry> {
        if bytes.is_empty() {
            return None;
        }
        self.get(hash64(bytes))
            .filter(|e| e.text == bytes && (bytes.len() >= MIN_BYTES || e.kind == Kind::Label))
    }

    /// The strings catalogued.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Each (hash, entry), in site order.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &Entry)> {
        self.entries.iter().map(|(h, e)| (*h, e))
    }

    /// The export of the catalogue.
    pub fn export(&self) -> Export {
        Export {
            format: FORMAT.into(),
            locale: LOCALE.into(),
            strings: self
                .iter()
                .map(|(hash, e)| ExportString {
                    hash,
                    len: e.text.len() as u32,
                    kind: e.kind,
                    text: e.text.iter().map(|&b| char::from(b)).collect(),
                    cells: e.cells,
                })
                .collect(),
            texts: Vec::new(),
            u: Vec::new(),
        }
    }

    /// [`Catalogue::export`] as JSON.
    pub fn to_json(&self) -> String {
        self.export().to_json()
    }
}

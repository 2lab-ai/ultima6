//! Port-only (spec 08 Design 2, slice K2): the catalogue of the executable's strings that game
//! code shows, read from the user's `game` at run time, and the string export of it.
//!
//! **What is catalogued.** The sites the engine knows (each module lists the strings it prints,
//! next to their addresses): the command loop's prompts and results
//! ([`crate::main_loop::commands::PRINTED`]), Look, Get, Drop, Move and the keys'
//! ([`crate::objects::commands::PRINTED`]), Use's (its checks' lines; 2026-10-25: and every
//! handler's own lines and prompts, the lever's, the drawbridge crank's and the rest, each file's
//! list, each on a wrapping line: no budget ([`crate::usecmd::PRINTED`])), Rest's
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
//! ([`crate::converse::text::SHOWN`]): each one game code passes to `printf` as its format;
//! 2026-10-27 (spec 16 item 1): and the lines Use and the walk reach in the object code (a
//! torch's and a fixture's, a dismount's, a scout's warning: the object commands' list), `map/`'s
//! (the place-name line and the party-mode line, [`crate::map::seams::PRINTED`], and the place
//! names it prints as formats, read through their pointers, [`crate::map::seams::PLACE_NAMES`]),
//! main's line that no character is made yet (the loop's list), the inventory view's weight
//! values, within the English's cells ([`crate::inventory::PANEL_PRINTED`]), and the display name
//! of no object Use prints as a format ([`NAME_PRINTED`]). And
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
//! ([`crate::actors::clock::LABELS`]), the carried items' and the wand's names the vanished line
//! prints through `%s` (2026-10-27, labels at their own addresses, [`shown_at`]), the combat-mode
//! names the panels' line prints through `%s` (2026-10-24, [`crate::ui::tables::LABELS`], each
//! within the line's field), and the
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
//! lookup's check). Bytes a print hook or a label read finds no entry for are counted
//! (2026-10-27, [`super::Counts::uncatalogued`]) when they equal a complete executable string no
//! entry holds, found by their hash ([`Catalogue::uncatalogued`]).
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

use std::collections::{BTreeMap, BTreeSet};

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
pub(super) fn has_words(text: &[u8]) -> bool {
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
    /// spell's name [`crate::magic::NAME_CELLS`], a circle's [`crate::magic::CIRCLE_CELLS`];
    /// 2026-10-27, the inventory view's weight values, [`crate::inventory::PANEL_PRINTED`], the
    /// English's cells);
    /// `None` for one with no such place. Two sites of the same bytes keep the smaller.
    pub cells: Option<u16>,
}

/// The catalogued strings of one `game`, by [`hash64`] of their bytes, in site order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalogue {
    by_hash: BTreeMap<u64, usize>,
    entries: Vec<(u64, Entry)>,
    /// 2026-10-27: the executable's strings no entry holds ([`Catalogue::uncatalogued`]).
    unlisted: Unlisted,
}

/// The [`hash64`]es of the executable's strings no entry holds and none English on purpose
/// ([`Catalogue::unlisted_of`], 2026-10-27). Its `Debug` gives the count only.
#[derive(Clone, Default, PartialEq, Eq)]
struct Unlisted(BTreeSet<u64>);

impl std::fmt::Debug for Unlisted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Unlisted({} strings)", self.0.len())
    }
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

/// Tests (2026-10-27): a synthetic `game` holding `parts` (one CODE hunk from the image base:
/// each part's bytes NUL-ended at its image address), as [`install_test_forms`] checks against.
#[cfg(test)]
pub(crate) fn test_game(parts: &[(u32, &[u8])]) -> Vec<u8> {
    let at = |addr: u32| (addr - crate::image::IMAGE_BASE) as usize;
    let end = parts.iter().map(|&(a, b)| at(a) + b.len() + 1).max();
    let mut hunk = vec![0u8; end.unwrap_or(0).next_multiple_of(4)];
    for &(addr, bytes) in parts {
        hunk[at(addr)..at(addr) + bytes.len()].copy_from_slice(bytes);
    }
    crate::image::SyntheticGame::new()
        .loaded(false, hunk)
        .build()
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
    let game = test_game(parts);
    install_test_catalogue(s, &game, forms, Catalogue::from_game(&game));
}

/// Tests (2026-10-27): [`install_test_forms`] over `game` with the catalogue given (a string no
/// list names, as a label of the test's own: [`Catalogue::from_entries`]).
#[cfg(test)]
pub(crate) fn install_test_catalogue(
    s: &mut crate::state::State,
    game: &[u8],
    forms: &[(&[u8], &str)],
    catalogue: Catalogue,
) {
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
    let files = crate::files::Files::new(BTreeMap::from([("game".to_string(), game.to_vec())]));
    super::install_catalogued(s, &files, std::rc::Rc::new(pack), None, catalogue)
        .expect("the pack checks");
}

/// Port-only (spec 16 item 1, 2026-10-27): the executable's strings shown in English on purpose
/// at their own addresses, the articles 0x12904c prints before an object name (the object
/// commands' `S_A`, `S_AN`, `S_THE`, which the object tables' loader reads too as `STR_A`,
/// `STR_AN`, `STR_THE`; modules the locale layer cannot name, so the scan's test pins each to
/// those consts), which no list catalogues and the uncatalogued count leaves out
/// ([`Catalogue::uncatalogued`]).
pub(crate) const ENGLISH_AT: [u32; 3] = [0x0012_83e2, 0x0012_83e6, 0x0012_83ea];

/// Port-only (spec 16 item 1, 2026-10-27): the pointer slots of the incantation syllables, the
/// game's magic words, shown in English on purpose (A4-0x70b8 + 4 c for the letters c,
/// [`crate::usecmd::defs::SPELL_NAMES`]; magic's syllable table), which the uncatalogued count
/// leaves out ([`Catalogue::uncatalogued`]).
pub(crate) fn english_slots() -> impl Iterator<Item = u32> {
    (u32::from(b'A')..=u32::from(b'Z'))
        .map(|c| A4.wrapping_add_signed(crate::usecmd::defs::SPELL_NAMES) + 4 * c)
}

/// Port-only (spec 16 item 1, 2026-10-27): the display name of no object (0x10eef8's word for a
/// slot below 0; the string at 0x10a4ce, which the object tables' loader reads, `objects::defs`
/// `STR_NOTHING`, and `objects::names` gives), which the
/// printers that can name a slot below 0 pass to `printf` as its format (Use's
/// `usecmd::Ux::name`, magic's `print_name`, the object commands' `text::print`). The printers
/// that put a name in a `%s` field take a slot of 0 or more: Ready and Unready (`ui::click`
/// gives no code for selection -1), combat's lines, the actors' (`objects::ObjectId`).
pub(crate) const NAME_PRINTED: &[u32] = &[0x0010_a4ce];

/// The image addresses of the strings game code passes to `printf` as its format (or to `puts`,
/// `put_raw_str`), each module's list in site order: the catalogue's printed strings.
pub(crate) fn printed() -> impl Iterator<Item = u32> {
    crate::main_loop::commands::PRINTED
        .iter()
        .chain(
            crate::objects::commands::PRINTED
                .iter()
                .flat_map(|s| s.iter()),
        )
        .chain(crate::usecmd::PRINTED.iter().flat_map(|s| s.iter()))
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
        .chain(crate::magic::PAGE_PRINTED)
        .chain(crate::map::seams::PRINTED)
        .chain(crate::inventory::PANEL_PRINTED)
        .chain(NAME_PRINTED)
        .copied()
}

/// The pointer slots of the strings game code passes to `printf` as its format through a pointer
/// (2026-10-27: the place names, [`crate::map::seams::PLACE_NAMES`]): catalogued as printed
/// strings, not labels.
pub(crate) fn pointed() -> impl Iterator<Item = u32> {
    crate::map::seams::PLACE_NAMES.iter().copied()
}

/// How a printed string is shown: a format when it holds a conversion, else a message.
fn printed_kind(text: &[u8]) -> Kind {
    if has_conversion(text) {
        Kind::Format
    } else {
        Kind::Message
    }
}

/// The pointer slots of the labels game code reads for display ([`super::shown`]): the
/// catalogue's labels.
pub(crate) fn labels() -> impl Iterator<Item = u32> {
    crate::main_loop::commands::LABELS
        .iter()
        .chain(&SHARED_MESSAGES)
        .chain(crate::objects::commands::LABELS)
        .chain(crate::inventory::LABELS)
        .chain(crate::magic::LABELS)
        .chain(crate::actors::clock::LABELS)
        .chain(crate::ui::tables::LABELS)
        .chain(&crate::magic::NAME_LABELS)
        .chain(&crate::magic::CIRCLE_LABELS)
        .copied()
}

/// The labels game code reads for display at their own addresses (no pointer slot holds them):
/// the panels' ([`crate::ui::tables::SHOWN_AT`], each within its English's cells) and
/// (2026-10-27) the vanished line's item words ([`crate::actors::clock::SHOWN_AT`],
/// [`crate::combat::attack::SHOWN_AT`], on a wrapping line: no budget).
pub(crate) fn shown_at() -> impl Iterator<Item = u32> {
    crate::ui::tables::SHOWN_AT
        .iter()
        .chain(crate::actors::clock::SHOWN_AT)
        .chain(crate::combat::attack::SHOWN_AT)
        .copied()
}

impl Catalogue {
    /// The catalogue of `game` (the decrunched executable): every site's string that reads
    /// (inside the image, NUL-ended) and is [`MIN_BYTES`] or longer ([`printed`], then
    /// [`pointed`]'s, each through its pointer), a label read for display (a pointer table's,
    /// [`labels`], or a panel label at its own address, [`crate::ui::tables::SHOWN_AT`]) 1 byte
    /// or longer ([`Catalogue::add_label`]). A file that is no hunk file gives an empty catalogue
    /// (nothing is substituted then).
    pub fn from_game(game: &[u8]) -> Catalogue {
        let mut c = Catalogue::default();
        let Ok(img) = GameImage::parse(game) else {
            return c;
        };
        for at in printed() {
            if let Ok(text) = img.cstr(at) {
                // A panel's string (the spellbook page's level word, the inventory view's weight
                // values too) is drawn at a fixed cell: the English's cells.
                let cells = (crate::ui::tables::PRINTED.contains(&at)
                    || crate::magic::PAGE_PRINTED.contains(&at)
                    || crate::inventory::PANEL_PRINTED.contains(&at))
                .then(|| english_cells(&text));
                c.add_within(printed_kind(&text), text, MIN_BYTES, cells);
            }
        }
        for ptr in pointed() {
            if let Ok(text) = img.pointer(ptr).and_then(|p| img.cstr(p)) {
                c.add_within(printed_kind(&text), text, MIN_BYTES, None);
            }
        }
        for ptr in labels() {
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
        // The labels read for display at their own addresses, a panel's at a fixed cell.
        for at in shown_at() {
            if let Ok(text) = img.cstr(at) {
                let cells = crate::ui::tables::SHOWN_AT
                    .contains(&at)
                    .then(|| english_cells(&text));
                c.add_label(text, cells);
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
        c.unlisted = Unlisted(c.unlisted_of(&img));
        c
    }

    /// The [`hash64`]es of `img`'s strings no entry holds (2026-10-27): each run of every loaded
    /// segment that starts at a string's start (the segment's start or after a NUL) and ends at a
    /// NUL inside the segment (a run the segment ends without one is none), [`MIN_BYTES`] to
    /// [`MAX_BYTES`] long, with a word of its own; but the strings shown in English on purpose
    /// ([`ENGLISH_AT`], [`english_slots`]'s). A string that follows code with no NUL before it
    /// starts no run here (its run starts at the last NUL before it, in the code): such a string
    /// is not in the set, a blind spot of the count.
    fn unlisted_of(&self, img: &GameImage<'_>) -> BTreeSet<u64> {
        let english: BTreeSet<u64> = ENGLISH_AT
            .iter()
            .filter_map(|&at| img.cstr(at).ok())
            .chain(
                english_slots().filter_map(|slot| img.pointer(slot).and_then(|p| img.cstr(p)).ok()),
            )
            .map(|text| hash64(&text))
            .collect();
        let mut out = BTreeSet::new();
        for n in 0..img.segment_count() {
            let segment = img.loaded(n).unwrap_or_default();
            let mut start = 0;
            while let Some(len) = segment[start..].iter().position(|&b| b == 0) {
                let text = &segment[start..start + len];
                let h = hash64(text);
                let fits = (MIN_BYTES..=MAX_BYTES).contains(&len) && has_words(text);
                if fits && self.get(h).is_none_or(|e| e.text != text) && !english.contains(&h) {
                    out.insert(h);
                }
                start += len + 1;
            }
        }
        out
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

    /// Whether `bytes`, which game code prints or reads as a label and the catalogue has no entry
    /// for, are a complete executable string (2026-10-27, [`Catalogue::unlisted_of`]): one hash
    /// and one tree lookup. The set classifies content, not origin: any bytes equal to a complete
    /// executable string count (a name, a typed word or a pack's text equal to one would too). A
    /// string's tail counts only when its bytes are also a complete string of their own somewhere
    /// in the executable; an article does not count.
    pub fn uncatalogued(&self, bytes: &[u8]) -> bool {
        (MIN_BYTES..=MAX_BYTES).contains(&bytes.len()) && self.unlisted.0.contains(&hash64(bytes))
    }

    /// How many of the executable's strings no entry holds ([`Catalogue::unlisted_of`]; counts
    /// only).
    pub fn unlisted_len(&self) -> usize {
        self.unlisted.0.len()
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

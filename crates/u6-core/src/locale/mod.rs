//! Port-only (spec 08 Design 2 and 3, slice K1): the locale layer for conversation text runs,
//! and the pack it reads (`ko.u6lp`).
//!
//! **What it covers.** Only the text runs of the conversation scripts (Design 2's first unit):
//! a run is keyed (archive, item, run offset) as the catalog keys it
//! ([`crate::converse::disasm::catalog`]), and the pack holds each run's length and FNV-1a 64
//! hash beside its translation. Table strings, executable strings, the `u` program's text,
//! `look.lzd` and `book.dat`, keyword aliases and typing are not in this slice: they stay
//! English.
//!
//! **At run time** ([`run_at`], called by `converse::run`'s text runner where a run starts: the
//! first text byte after the runner began or after a byte that is not text): a run the
//! installed pack has, whose bytes in the script buffer are still the ones it was made from
//! (hash and every byte text), is put as its translation instead, byte by byte through the same
//! escape layer, so `$n`, `#n`, `*`, `@` and the rest work as in English; the read position then
//! moves past the run's bytes, which are read (and traced) but not put. No op is moved, merged
//! or skipped: the bytes passed over are text bytes only, which print and do nothing else. Any
//! other run is a miss and prints in English. The runner keeps one checkpoint per source byte,
//! so the bounded-work yields fall where they fall in English; with the identity pack (a
//! translation equal to its source) every put and every yield is English's, in order.
//!
//! The pack's declared language ([`Pack::language`]) gives the grammar rules (josa, the plural
//! letters) of the text the pack puts, and only of it: everything else, the runs the pack does
//! not have included, keeps English's. An installed pack turns the UTF-8 assembler on
//! ([`crate::ui::TextState::wide`], which leaves English bytes as they are) and sets the Hangul
//! table the platform gives.
//!
//! **The pack** ([`Pack::parse`], [`Pack::to_bytes`]): the four bytes [`MAGIC`], a line feed,
//! then one JSON object:
//!
//! ```text
//! {"format":"u6.locale-pack/1","language":"ko","tool":"<name/version>",
//!  "base":{"converse":"<16 hex>"},
//!  "converse":[{"archive":"a","item":5,"offset":10,"len":12,"hash":"<16 hex>","text":"..."}]}
//! ```
//!
//! `base.converse` is the fingerprint of the scripts the converse map names
//! ([`fingerprint`]); each `hash` is [`hash64`] of the run's bytes; `text` is the translation
//! (UTF-8: ASCII and Hangul syllables only, no control character but the line feed). The reader
//! is strict, under spec 04 W6a's package limits: a size cap read before anything else, a cap on
//! the runs and on each text, every field and no other, each run once, a key inside the
//! interpreter's script buffer. Its refusal ([`PackError`]) is a fixed sentence that names the
//! rule broken, never a value read. A pack is checked against the game files once, before it is
//! installed ([`Pack::check`]): the fingerprint, then each run (inside its script, text bytes
//! only, its hash); a mismatch refuses the whole pack and names the part kind only
//! ([`Mismatch`]).
//!
//! **Slice K2: table strings and executable strings.** A pack may also hold two more maps (each
//! left out of the file when empty, so a pack of runs only is K1's file byte for byte):
//!
//! ```text
//!  "tables":[{"archive":"a","item":5,"offset":80,"len":4,"hash":"<16 hex>","capacity":99,"text":"..."}],
//!  "strings":[{"hash":"<16 hex>","len":12,"text":"..."}]
//! ```
//!
//! - **Table strings** are keyed (archive, item, string offset), with the source string's
//!   length (up to its NUL) and hash, and for one an `a6` copies into a string slot the most
//!   bytes that slot holds (`capacity`; the reader refuses a text longer than it). They are
//!   checked with the runs, against the same fingerprint (`base.converse` is over the scripts
//!   the runs and the table strings name). One hook in the table reader serves op `b5 d2` and
//!   op `a6 d2` ([`table_at`]): the string it reads at its start, still the bytes the pack was
//!   made from, is put (`b5`) or shown (`a6`: the slot keeps the English for logic, its shown
//!   text is the pack's, [`crate::converse::vars`]) as the translation; a copy whose slot cannot
//!   hold the translation stays English and is counted missed; a read that starts inside a
//!   string (its rest) is not looked up.
//! - **Executable strings** are keyed by [`hash64`] of their English bytes, with its length;
//!   they need no fingerprint. Only a string the engine catalogues ([`strings::Catalogue`], read
//!   from the user's `game` when the pack is installed) is ever looked up, only where game code
//!   prints it ([`exe_string`]: `printf`'s format, `puts`, `put_raw_str`) or reads a label for
//!   display ([`shown`]), and never on what an escape generates (`$n`, `#n`, `printf`'s `%s`):
//!   logic keeps the English. A string under [`strings::MIN_BYTES`] is never substituted, but a
//!   label [`shown`] reads for display, a pointer table's or (2026-10-24) a panel label at its
//!   own address (`ui::tables::SHOWN_AT`) (2026-10-21: the catalogue holds it from 1 byte,
//!   [`strings::Catalogue::label_of`]; the print hooks never find it). At install every pack
//!   string the catalogue has must hold the same `%` directives, in the same order, as its
//!   English ([`strings::directives`]), else the pack is refused (the strings part). A string
//!   drawn at a fixed cell (2026-10-24, a panel's: [`strings::Entry::cells`]) takes the pack's
//!   text only within its cells and with as many line breaks as its English ([`lookup`]); else
//!   it stays English, counted missed, as a table string over its slot does.
//!
//! **Slice K3: object names and books.** A pack may also hold the object names of `look.lzd`
//! and the books and signs of `book.dat` (each map left out of the file when empty):
//!
//! ```text
//!  "look":[{"index":12,"len":5,"hash":"<16 hex>","text":"..."}],
//!  "books":[{"index":3,"len":800,"hash":"<16 hex>","text":"..."}]
//! ```
//!
//! Spec 08 §3 has the two files replaced whole in a layer of the pack's own; this slice keys them
//! per entry instead, as the runs are: a look entry by its index in the table
//! ([`texts::look_units`]), a book by its index in `book.dat`'s offset table (quality − 1, books
//! and signs alike; [`texts::book_units`]), each with its English's length and hash. They need no
//! fingerprint: at install every entry must be one of the files' units, of that length and hash,
//! with the same `%` directives in the same order as its English (both are printed as `printf`
//! formats), else the pack is refused, the part named (look, books). A book's text must be under
//! [`texts::BOOK_BYTES`], the buffer the game reads a book into (the reader and the read refuse a
//! longer one, as the tool's gate does).
//!
//! At run time an entry is put only where it is read for display ([`look_at`]: the names Look,
//! the object commands, Use, the spells' targets and combat print; [`book_at`]: the book or sign
//! Look reads), its bytes in the game's table still the ones it was made from; logic keeps the
//! English (`crate::objects::display_name` and `look_text`: a name compared, measured for a panel's
//! layout or kept). The plural markers of a look entry follow the pack's language where it is
//! looked up: in Korean they are dropped ([`crate::objects`]'s Korean rule, `/` or `\\` and the
//! letters after it), and the name takes no article; with `en` the text is the English's and the
//! printer resolves it as English. A book is put in the pack's grammar.
//!
//! The counters ([`LocaleState`]) count catalogued units only: a table string read at its start
//! or a catalogued executable string shown, put from the pack (a hit) or not (a miss); a look
//! entry or a book shown (a read past the look table's entries, or of no unit of `book.dat`, is
//! none). And (2026-10-27) an executable string shown with a pack installed that the catalogue
//! does not hold, which prints in English ([`Counts::uncatalogued`]).
//!
//! **Slice K4: the `u` program's strings.** A pack may also hold the formats the `u` program
//! prints (the opening, the introduction story, the gypsy's passages and questions, character
//! creation, the transfer), keyed as the executable strings are, by [`hash64`] of their English
//! with its length (left out of the file when empty):
//!
//! ```text
//!  "u":[{"hash":"<16 hex>","len":40,"text":"..."}]
//! ```
//!
//! Only a string the catalogue of the user's `u` has ([`program::Catalogue`], read when the pack
//! is installed) is looked up, only at the `u` program's print entry, on the format and never on
//! its arguments ([`crate::intro::locale`]); at install every pack string the catalogue has must
//! keep its English's length and the same `printf` codes, in the same order
//! ([`program::directives`]), and the prompt of a key read the keys its English shows standing
//! alone ([`program::keys_kept`]), else the pack is refused (the `u` part). `u` runs on the state
//! the pack is installed on, takes the pack as it starts, and counts its lookups into
//! [`LocaleState::program`].
//!
//! **Slice K5a: keyword aliases.** A pack may also hold, for a conversation script its runs or
//! table strings name (so under the same fingerprint, `base.converse`), the words a player may
//! type for a keyword of it (left out of the file when empty):
//!
//! ```text
//!  "aliases":[{"archive":"a","item":5,"keyword":"name","words":["<1 to 8 characters>"]}]
//! ```
//!
//! `keyword` is the keyword as the keyword block reads it (its bytes up to a NUL, upper case
//! folded to lower, `?` kept; printable ASCII, no `,`), `words` 1 to [`MAX_ALIAS_WORDS`] words
//! of 1 to [`MAX_ALIAS_CHARS`] characters each, Hangul syllables or printable ASCII but the
//! space. The reader refuses an entry of another form, one key twice, or a script none of the
//! pack's runs and table strings name (the aliases part, [`PackError::Aliases`]); at install a
//! word holding one of the game's word delimiters (a word the keyword test never sees whole)
//! refuses the pack, the aliases part named ([`Pack::check_aliases`]). A keyword the script
//! does not hold is kept and never used. At run time the keyword block tries a keyword's
//! aliases after its English ([`crate::converse`]'s `keywords`): a word of the input equal to
//! one, or to one followed by one particle (조사) of a fixed list, matches; the input itself is
//! not changed, so `$` and `f9` keep what was typed.
//!
//! Rights (spec 08): a pack made from the owner's files holds his game text; it lives in
//! `~/Games/ultima6/i18n/ko/` and his browser storage, never in the repository or under `site/`
//! (`tests/no_locale_pack_in_repo.rs`).

#[cfg(test)]
mod identity;
pub mod program;
pub mod strings;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_program;
#[cfg(test)]
mod tests_scan;
#[cfg(test)]
mod tests_strings;
#[cfg(test)]
mod tests_texts;
pub mod texts;

#[cfg(test)]
pub(crate) use identity::identity;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use u6_formats::converse::ConverseLib;
use u6_formats::font::HangulFont;

use crate::converse::SCRIPT_LEN;
use crate::converse::disasm::catalog::hash64;
use crate::converse::disasm::ops::is_text;
use crate::files::Files;
use crate::state::State;
use crate::ui::Language;

/// The first bytes of a pack file.
pub const MAGIC: &[u8; 4] = b"U6LP";
/// The pack format's name (its JSON `format`).
pub const FORMAT: &str = "u6.locale-pack/1";
/// The most bytes a pack file may hold (checked before it is read).
pub const MAX_PACK_BYTES: usize = 16 << 20;
/// The most runs a pack may hold.
pub const MAX_RUNS: usize = 1 << 16;
/// The most bytes one translation may hold (three per syllable of a script-long run).
pub const MAX_TEXT_BYTES: usize = 3 * SCRIPT_LEN;
/// The most characters of the tool's name.
pub const MAX_TOOL_CHARS: usize = 64;
/// The conversation archives a key may name (`converse.a`, `converse.b`).
pub const ARCHIVES: [u8; 2] = [b'a', b'b'];
/// The most table strings, and the most executable strings, a pack may hold.
pub const MAX_STRINGS: usize = 1 << 16;
/// The most bytes one executable string's translation may hold (three per byte of the longest
/// string the game reads).
pub const MAX_STRING_TEXT_BYTES: usize = 3 * strings::MAX_BYTES;
/// Slice K5a: the most keyword aliases (keywords given words) a pack may hold.
pub const MAX_ALIASES: usize = 1 << 16;
/// Slice K5a: the most words one keyword may be given.
pub const MAX_ALIAS_WORDS: usize = 4;
/// Slice K5a: the most characters of one alias word.
pub const MAX_ALIAS_CHARS: usize = 8;
/// Slice K5a: the most bytes of the keyword an alias is keyed by.
pub const MAX_KEYWORD_BYTES: usize = 32;

/// A run's key: (archive letter, item, offset in the script).
pub type RunKey = (u8, u16, u32);

/// Slice K5a: a script's key, (archive letter, item).
pub type ScriptKey = (u8, u16);

/// Slice K5a: a script's keyword aliases: the words (UTF-8) by keyword (its bytes up to a NUL,
/// case-folded).
pub type Aliases = BTreeMap<Vec<u8>, Vec<Vec<u8>>>;

/// One run's translation and what it was made from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunText {
    /// The run's length in the script (its source bytes).
    pub len: u32,
    /// [`hash64`] of those bytes.
    pub hash: u64,
    /// The translation, as the text layer takes it (UTF-8).
    pub text: Vec<u8>,
}

/// One table string's translation and what it was made from (slice K2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableText {
    /// The source string's length in the script (its bytes up to the NUL).
    pub len: u32,
    /// [`hash64`] of those bytes.
    pub hash: u64,
    /// For a string an `a6` copies into a string slot: the most bytes that slot holds (the text
    /// may not be longer).
    pub capacity: Option<u32>,
    /// The translation (UTF-8).
    pub text: Vec<u8>,
}

/// One executable string's translation (slice K2), keyed by [`hash64`] of its English.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringText {
    /// The English's length in bytes.
    pub len: u32,
    /// The translation (UTF-8).
    pub text: Vec<u8>,
}

/// A pack's parts, as a mismatch names them: the conversation part (its runs and table strings,
/// under one fingerprint), the executable strings, (slice K3) the object names and the books,
/// (slice K4) the `u` program's strings and (slice K5a) the keyword aliases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Converse,
    Strings,
    Look,
    Books,
    Program,
    Aliases,
}

impl Part {
    /// The part's name in the pack (`base.converse`; `strings`; `look`; `books`; `u`;
    /// `aliases`).
    pub fn name(self) -> &'static str {
        match self {
            Part::Converse => "converse",
            Part::Strings => "strings",
            Part::Look => "look",
            Part::Books => "books",
            Part::Program => "u",
            Part::Aliases => "aliases",
        }
    }
}

/// A checked pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pack {
    /// The grammar of its text.
    pub language: Language,
    /// The tool that made it (its name and version).
    pub tool: String,
    /// The fingerprint of the scripts its converse map and its table strings name
    /// ([`fingerprint`]).
    pub converse_base: u64,
    runs: BTreeMap<RunKey, RunText>,
    tables: BTreeMap<RunKey, TableText>,
    strings: BTreeMap<u64, StringText>,
    /// Slice K3: object names by look entry index, books by `book.dat` index (each its English's
    /// length and hash, as a run's).
    look: BTreeMap<u32, RunText>,
    books: BTreeMap<u32, RunText>,
    /// Slice K4: the `u` program's strings, by [`hash64`] of their English.
    program: BTreeMap<u64, StringText>,
    /// Slice K5a: the keyword aliases, by script.
    aliases: BTreeMap<ScriptKey, Aliases>,
}

/// Why a pack file is refused. Each is a fixed sentence (no value of the file).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackError {
    /// Over [`MAX_PACK_BYTES`].
    TooLarge,
    /// It does not start with [`MAGIC`] and a line feed.
    NotAPack,
    /// What follows is not JSON of the pack's shape (a field missing, of the wrong kind, or one
    /// the format does not have).
    Shape,
    /// Its `format` is not [`FORMAT`].
    Format,
    /// Its `language` is neither `en` nor `ko`.
    Language,
    /// Its tool name is empty, too long or not printable ASCII.
    Tool,
    /// A hash or the fingerprint is not 16 lower-case hex digits.
    Hash,
    /// Over [`MAX_RUNS`] runs.
    TooManyRuns,
    /// A key outside the script buffer, or an archive other than `a` and `b`.
    Key,
    /// One key twice.
    Duplicate,
    /// A translation over [`MAX_TEXT_BYTES`], or with a character the game cannot show (only
    /// ASCII and Hangul syllables, no control character but the line feed).
    Text,
    /// An executable string whose English is empty or longer than [`strings::MAX_BYTES`] (one
    /// under [`strings::MIN_BYTES`] is read, and found only as a label), or whose translation is
    /// over [`MAX_STRING_TEXT_BYTES`].
    StringKey,
    /// A table string's translation longer than the slot it is copied into holds.
    Capacity,
    /// An object name keyed past the look table ([`texts::LOOK_ENTRIES`]), its English empty or
    /// longer than [`texts::MAX_LOOK_BYTES`], or its translation over
    /// [`texts::MAX_LOOK_TEXT_BYTES`].
    LookEntry,
    /// A book keyed past the qualities that name one ([`texts::BOOK_INDICES`]), or its English
    /// empty or longer than [`texts::BOOK_BYTES`].
    BookEntry,
    /// A book's translation not under [`texts::BOOK_BYTES`] (the buffer the game reads it into).
    BookCapacity,
    /// A string of the `u` program whose English is shorter than [`program::MIN_BYTES`] or
    /// longer than [`program::MAX_BYTES`], or whose translation is over
    /// [`program::MAX_TEXT_BYTES`].
    ProgramKey,
    /// Slice K5a: a keyword alias of another form than the aliases part's (its keyword or a
    /// word), one keyword twice, over [`MAX_ALIASES`], or one for a script none of the pack's
    /// runs and table strings name.
    Aliases,
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PackError::TooLarge => "the file is larger than a Korean pack can be",
            PackError::NotAPack => "the file is not a Korean pack (it does not start with U6LP)",
            PackError::Shape => "its contents are not those of a Korean pack",
            PackError::Format => "it is not of this page's pack format (u6.locale-pack/1)",
            PackError::Language => "it declares a language this page does not have",
            PackError::Tool => "it does not name the tool that made it",
            PackError::Hash => "a hash in it is not 16 hex digits",
            PackError::TooManyRuns => "it holds more lines than a pack can",
            PackError::Key => "a line in it is keyed outside the conversation scripts",
            PackError::Duplicate => "it holds one line twice",
            PackError::Text => "a line in it holds a character the game cannot show",
            PackError::StringKey => "a game string in it is not one the game prints",
            PackError::Capacity => {
                "a table string in it is longer than the place the game copies it to"
            }
            PackError::LookEntry => "an object name in it is not one the game's look table holds",
            PackError::BookEntry => "a book in it is not one the game's book file holds",
            PackError::BookCapacity => "a book in it is longer than the game reads a book",
            PackError::ProgramKey => {
                "an opening or creation string in it is not one the u program prints"
            }
            PackError::Aliases => {
                "its keyword aliases part (aliases) holds an entry of another form or for a script its lines do not name"
            }
        })
    }
}

/// The game files are not the ones a pack was made from: the part that differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub part: Part,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.part {
            Part::Converse => f.write_str("its conversation part was made from other game files"),
            Part::Strings => {
                f.write_str("its game strings part does not keep the game's format directives")
            }
            Part::Look => f.write_str("its object names part does not match the game's look table"),
            Part::Books => f.write_str("its books part does not match the game's books and signs"),
            Part::Program => f.write_str(
                "its opening and creation part does not keep the u program's format codes",
            ),
            Part::Aliases => f.write_str(
                "its keyword aliases part (aliases) holds a word the game splits into words",
            ),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackJson {
    format: String,
    language: String,
    tool: String,
    base: BaseJson,
    converse: Vec<RunJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tables: Vec<TableJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    strings: Vec<StringJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    look: Vec<EntryJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    books: Vec<EntryJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    u: Vec<StringJson>,
    /// Slice K5a: kept as written and read on its own as a list of [`AliasJson`], so anything
    /// of another form is the aliases part's refusal ([`PackError::Aliases`]): a missing, extra
    /// or repeated field (read as written, not through a map that keeps the last of two), a
    /// `null` or anything not a list. `None` only when the field is left out.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    aliases: Option<Box<serde_json::value::RawValue>>,
}

/// A field that is there, whatever its value (`null` included), as `Some`.
fn present<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Box<serde_json::value::RawValue>>, D::Error> {
    Box::<serde_json::value::RawValue>::deserialize(d).map(Some)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AliasJson {
    archive: String,
    item: u16,
    keyword: String,
    words: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BaseJson {
    converse: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunJson {
    archive: String,
    item: u16,
    offset: u32,
    len: u32,
    hash: String,
    text: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TableJson {
    archive: String,
    item: u16,
    offset: u32,
    len: u32,
    hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    capacity: Option<u32>,
    text: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StringJson {
    hash: String,
    len: u32,
    text: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryJson {
    index: u32,
    len: u32,
    hash: String,
    text: String,
}

fn hex(v: u64) -> String {
    format!("{v:016x}")
}

fn read_hex(s: &str) -> Result<u64, PackError> {
    if s.len() != 16 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(PackError::Hash);
    }
    u64::from_str_radix(s, 16).map_err(|_| PackError::Hash)
}

fn language_code(l: Language) -> &'static str {
    match l {
        Language::English => "en",
        Language::Korean => "ko",
    }
}

/// Whether a translation is one the text layer can show: ASCII printable and the line feed, and
/// Hangul syllables (U+AC00..=U+D7A3), within [`MAX_TEXT_BYTES`].
fn text_ok(t: &str) -> bool {
    t.len() <= MAX_TEXT_BYTES
        && t.chars().all(|c| {
            c == '\n' || (' '..='\u{7f}').contains(&c) || ('\u{ac00}'..='\u{d7a3}').contains(&c)
        })
}

/// Slice K5a: whether `keyword` is a key of the aliases: 1 to [`MAX_KEYWORD_BYTES`] bytes of
/// printable ASCII as the keyword block reads a keyword (no `,`, which ends one), upper case
/// folded to lower.
fn keyword_ok(keyword: &[u8]) -> bool {
    (1..=MAX_KEYWORD_BYTES).contains(&keyword.len())
        && keyword
            .iter()
            .all(|&b| (0x20..0x7f).contains(&b) && b != b',' && !b.is_ascii_uppercase())
}

/// Slice K5a: whether `word` is an alias word: 1 to [`MAX_ALIAS_CHARS`] characters, each a
/// Hangul syllable or printable ASCII but the space (which splits the input into words).
fn alias_word_ok(word: &str) -> bool {
    (1..=MAX_ALIAS_CHARS).contains(&word.chars().count())
        && word
            .chars()
            .all(|c| c.is_ascii_graphic() || ('\u{ac00}'..='\u{d7a3}').contains(&c))
}

/// The fingerprint of the scripts a converse map names: [`hash64`] over, for each distinct
/// (archive letter, item) in order, the letter, the item (2 bytes, little-endian) and the
/// script's own [`hash64`] (8 bytes, little-endian; the catalog's and the NPC bundles' `hash`).
pub fn fingerprint(scripts: impl IntoIterator<Item = (u8, u16, u64)>) -> u64 {
    let set: BTreeSet<(u8, u16, u64)> = scripts.into_iter().collect();
    let mut bytes = Vec::with_capacity(set.len() * 11);
    for (letter, item, hash) in set {
        bytes.push(letter);
        bytes.extend_from_slice(&item.to_le_bytes());
        bytes.extend_from_slice(&hash.to_le_bytes());
    }
    hash64(&bytes)
}

impl Pack {
    /// A pack of `runs` (keys checked as [`Pack::parse`] checks them) over the scripts whose
    /// fingerprint is `converse_base`.
    pub fn new(
        language: Language,
        tool: &str,
        converse_base: u64,
        runs: impl IntoIterator<Item = (RunKey, RunText)>,
    ) -> Result<Pack, PackError> {
        let mut map = BTreeMap::new();
        for (key, run) in runs {
            check_key(key, run.len)?;
            let text = std::str::from_utf8(&run.text).map_err(|_| PackError::Text)?;
            if !text_ok(text) {
                return Err(PackError::Text);
            }
            if map.insert(key, run).is_some() {
                return Err(PackError::Duplicate);
            }
            if map.len() > MAX_RUNS {
                return Err(PackError::TooManyRuns);
            }
        }
        check_tool(tool)?;
        Ok(Pack {
            language,
            tool: tool.to_string(),
            converse_base,
            runs: map,
            tables: BTreeMap::new(),
            strings: BTreeMap::new(),
            look: BTreeMap::new(),
            books: BTreeMap::new(),
            program: BTreeMap::new(),
            aliases: BTreeMap::new(),
        })
    }

    /// The pack with `tables` (slice K2), checked as [`Pack::parse`] checks them: keys as the
    /// runs', the text the game can show and no longer than its `capacity`, each key once.
    pub fn with_tables(
        mut self,
        tables: impl IntoIterator<Item = (RunKey, TableText)>,
    ) -> Result<Pack, PackError> {
        for (key, t) in tables {
            check_key(key, t.len)?;
            let text = std::str::from_utf8(&t.text).map_err(|_| PackError::Text)?;
            if !text_ok(text) {
                return Err(PackError::Text);
            }
            if t.capacity.is_some_and(|c| t.text.len() > c as usize) {
                return Err(PackError::Capacity);
            }
            if self.tables.insert(key, t).is_some() {
                return Err(PackError::Duplicate);
            }
            if self.tables.len() > MAX_STRINGS {
                return Err(PackError::TooManyRuns);
            }
        }
        Ok(self)
    }

    /// The pack with the executable strings `strings` (slice K2), keyed by [`hash64`] of their
    /// English, checked as [`Pack::parse`] checks them: an English the game shows (1 to
    /// [`strings::MAX_BYTES`] bytes; the pack holds no kind, so one under [`strings::MIN_BYTES`]
    /// is kept for the catalogue to find as a label only, 2026-10-21), a text the game can show
    /// within [`MAX_STRING_TEXT_BYTES`], each hash once.
    pub fn with_strings(
        mut self,
        strings: impl IntoIterator<Item = (u64, StringText)>,
    ) -> Result<Pack, PackError> {
        for (hash, t) in strings {
            let len = t.len as usize;
            if !(1..=strings::MAX_BYTES).contains(&len) || t.text.len() > MAX_STRING_TEXT_BYTES {
                return Err(PackError::StringKey);
            }
            let text = std::str::from_utf8(&t.text).map_err(|_| PackError::Text)?;
            if !text_ok(text) {
                return Err(PackError::Text);
            }
            if self.strings.insert(hash, t).is_some() {
                return Err(PackError::Duplicate);
            }
            if self.strings.len() > MAX_STRINGS {
                return Err(PackError::TooManyRuns);
            }
        }
        Ok(self)
    }

    /// The pack with the object names `look` (slice K3), keyed by look entry index, checked as
    /// [`Pack::parse`] checks them: an index inside the look table, an English of 1 to
    /// [`texts::MAX_LOOK_BYTES`] bytes, a text the game can show within
    /// [`texts::MAX_LOOK_TEXT_BYTES`], each index once, at most [`texts::LOOK_ENTRIES`].
    pub fn with_look(
        mut self,
        look: impl IntoIterator<Item = (u32, RunText)>,
    ) -> Result<Pack, PackError> {
        for (index, t) in look {
            let len = t.len as usize;
            if index as usize >= texts::LOOK_ENTRIES
                || !(1..=texts::MAX_LOOK_BYTES).contains(&len)
                || t.text.len() > texts::MAX_LOOK_TEXT_BYTES
            {
                return Err(PackError::LookEntry);
            }
            let text = std::str::from_utf8(&t.text).map_err(|_| PackError::Text)?;
            if !text_ok(text) {
                return Err(PackError::Text);
            }
            if self.look.insert(index, t).is_some() {
                return Err(PackError::Duplicate);
            }
        }
        Ok(self)
    }

    /// The pack with the books `books` (slice K3), keyed by `book.dat` index, checked as
    /// [`Pack::parse`] checks them: an index a quality names ([`texts::BOOK_INDICES`]), an English
    /// of 1 to [`texts::BOOK_BYTES`] bytes, a text the game can show and under
    /// [`texts::BOOK_BYTES`] (else [`PackError::BookCapacity`]), each index once.
    pub fn with_books(
        mut self,
        books: impl IntoIterator<Item = (u32, RunText)>,
    ) -> Result<Pack, PackError> {
        for (index, t) in books {
            let len = t.len as usize;
            if index as usize >= texts::BOOK_INDICES || !(1..=texts::BOOK_BYTES).contains(&len) {
                return Err(PackError::BookEntry);
            }
            if t.text.len() >= texts::BOOK_BYTES {
                return Err(PackError::BookCapacity);
            }
            let text = std::str::from_utf8(&t.text).map_err(|_| PackError::Text)?;
            if !text_ok(text) {
                return Err(PackError::Text);
            }
            if self.books.insert(index, t).is_some() {
                return Err(PackError::Duplicate);
            }
        }
        Ok(self)
    }

    /// The pack with the `u` program's strings `program` (slice K4), keyed by [`hash64`] of their
    /// English, checked as [`Pack::parse`] checks them: an English of [`program::MIN_BYTES`] to
    /// [`program::MAX_BYTES`] bytes, a text the game can show within [`program::MAX_TEXT_BYTES`],
    /// each hash once.
    pub fn with_program(
        mut self,
        strings: impl IntoIterator<Item = (u64, StringText)>,
    ) -> Result<Pack, PackError> {
        for (hash, t) in strings {
            let len = t.len as usize;
            if !(program::MIN_BYTES..=program::MAX_BYTES).contains(&len)
                || t.text.len() > program::MAX_TEXT_BYTES
            {
                return Err(PackError::ProgramKey);
            }
            let text = std::str::from_utf8(&t.text).map_err(|_| PackError::Text)?;
            if !text_ok(text) {
                return Err(PackError::Text);
            }
            if self.program.insert(hash, t).is_some() {
                return Err(PackError::Duplicate);
            }
            if self.program.len() > MAX_STRINGS {
                return Err(PackError::TooManyRuns);
            }
        }
        Ok(self)
    }

    /// The pack with the keyword aliases `aliases` (slice K5a): per entry a script, a keyword and
    /// its words (UTF-8), checked as [`Pack::parse`] checks them: a script the pack's runs or
    /// table strings name, a keyword and words of the aliases' form ([`keyword_ok`],
    /// 1..=[`MAX_ALIAS_WORDS`] words, each [`alias_word_ok`]), each keyword of a script once, at
    /// most [`MAX_ALIASES`]. Any other is [`PackError::Aliases`].
    pub fn with_aliases(
        mut self,
        aliases: impl IntoIterator<Item = (ScriptKey, Vec<u8>, Vec<Vec<u8>>)>,
    ) -> Result<Pack, PackError> {
        let scripts = self.script_keys();
        let mut count = 0;
        for (script, keyword, words) in aliases {
            let words_ok = (1..=MAX_ALIAS_WORDS).contains(&words.len())
                && words
                    .iter()
                    .all(|w| std::str::from_utf8(w).is_ok_and(alias_word_ok));
            count += 1;
            if !scripts.contains(&script)
                || !keyword_ok(&keyword)
                || !words_ok
                || count > MAX_ALIASES
            {
                return Err(PackError::Aliases);
            }
            if self
                .aliases
                .entry(script)
                .or_default()
                .insert(keyword, words)
                .is_some()
            {
                return Err(PackError::Aliases);
            }
        }
        Ok(self)
    }

    /// Reads a pack file (see the module doc). Nothing of it is kept unless all of it checks.
    pub fn parse(bytes: &[u8]) -> Result<Pack, PackError> {
        if bytes.len() > MAX_PACK_BYTES {
            return Err(PackError::TooLarge);
        }
        let body = bytes
            .strip_prefix(&MAGIC[..])
            .and_then(|b| b.strip_prefix(b"\n"))
            .ok_or(PackError::NotAPack)?;
        let json: PackJson = serde_json::from_slice(body).map_err(|_| PackError::Shape)?;
        if json.format != FORMAT {
            return Err(PackError::Format);
        }
        let language = match json.language.as_str() {
            "en" => Language::English,
            "ko" => Language::Korean,
            _ => return Err(PackError::Language),
        };
        check_tool(&json.tool)?;
        let converse_base = read_hex(&json.base.converse)?;
        if json.converse.len() > MAX_RUNS {
            return Err(PackError::TooManyRuns);
        }
        if json.tables.len() > MAX_STRINGS || json.strings.len() > MAX_STRINGS {
            return Err(PackError::TooManyRuns);
        }
        if json.look.len() > texts::LOOK_ENTRIES || json.books.len() > texts::BOOK_INDICES {
            return Err(PackError::TooManyRuns);
        }
        if json.u.len() > MAX_STRINGS {
            return Err(PackError::TooManyRuns);
        }
        let alias_entries: Vec<AliasJson> = match &json.aliases {
            None => Vec::new(),
            Some(raw) => serde_json::from_str(raw.get()).map_err(|_| PackError::Aliases)?,
        };
        if alias_entries.len() > MAX_ALIASES {
            return Err(PackError::Aliases);
        }
        let mut runs = Vec::with_capacity(json.converse.len());
        for r in json.converse {
            let archive = archive_of(&r.archive)?;
            let hash = read_hex(&r.hash)?;
            runs.push((
                (archive, r.item, r.offset),
                RunText {
                    len: r.len,
                    hash,
                    text: r.text.into_bytes(),
                },
            ));
        }
        let mut tables = Vec::with_capacity(json.tables.len());
        for t in json.tables {
            let archive = archive_of(&t.archive)?;
            let hash = read_hex(&t.hash)?;
            tables.push((
                (archive, t.item, t.offset),
                TableText {
                    len: t.len,
                    hash,
                    capacity: t.capacity,
                    text: t.text.into_bytes(),
                },
            ));
        }
        let by_hash = |list: Vec<StringJson>| -> Result<Vec<(u64, StringText)>, PackError> {
            list.into_iter()
                .map(|t| {
                    let hash = read_hex(&t.hash)?;
                    Ok((
                        hash,
                        StringText {
                            len: t.len,
                            text: t.text.into_bytes(),
                        },
                    ))
                })
                .collect()
        };
        let strings = by_hash(json.strings)?;
        let entries = |list: Vec<EntryJson>| -> Result<Vec<(u32, RunText)>, PackError> {
            list.into_iter()
                .map(|e| {
                    let hash = read_hex(&e.hash)?;
                    Ok((
                        e.index,
                        RunText {
                            len: e.len,
                            hash,
                            text: e.text.into_bytes(),
                        },
                    ))
                })
                .collect()
        };
        let (look, books) = (entries(json.look)?, entries(json.books)?);
        let program = by_hash(json.u)?;
        let mut aliases = Vec::with_capacity(alias_entries.len());
        for a in alias_entries {
            let archive = archive_of(&a.archive).map_err(|_| PackError::Aliases)?;
            let words = a.words.into_iter().map(String::into_bytes).collect();
            aliases.push(((archive, a.item), a.keyword.into_bytes(), words));
        }
        Pack::new(language, &json.tool, converse_base, runs)?
            .with_tables(tables)?
            .with_strings(strings)?
            .with_look(look)?
            .with_books(books)?
            .with_program(program)?
            .with_aliases(aliases)
    }

    /// The pack file: [`MAGIC`], a line feed and the JSON object, runs in key order.
    pub fn to_bytes(&self) -> Vec<u8> {
        let json = PackJson {
            format: FORMAT.into(),
            language: language_code(self.language).into(),
            tool: self.tool.clone(),
            base: BaseJson {
                converse: hex(self.converse_base),
            },
            converse: self
                .runs
                .iter()
                .map(|(&(a, item, offset), r)| RunJson {
                    archive: char::from(a).to_string(),
                    item,
                    offset,
                    len: r.len,
                    hash: hex(r.hash),
                    text: String::from_utf8(r.text.clone()).expect("checked as UTF-8"),
                })
                .collect(),
            tables: self
                .tables
                .iter()
                .map(|(&(a, item, offset), t)| TableJson {
                    archive: char::from(a).to_string(),
                    item,
                    offset,
                    len: t.len,
                    hash: hex(t.hash),
                    capacity: t.capacity,
                    text: String::from_utf8(t.text.clone()).expect("checked as UTF-8"),
                })
                .collect(),
            strings: strings_json(&self.strings),
            look: entries_json(&self.look),
            books: entries_json(&self.books),
            u: strings_json(&self.program),
            aliases: aliases_json(&self.aliases),
        };
        let mut out = MAGIC.to_vec();
        out.push(b'\n');
        out.extend(serde_json::to_vec(&json).expect("a pack has only serialisable fields"));
        out
    }

    /// The runs it holds.
    pub fn runs(&self) -> usize {
        self.runs.len()
    }

    /// The scripts its runs and table strings are in.
    pub fn scripts(&self) -> usize {
        self.script_keys().len()
    }

    /// The table strings it holds.
    pub fn tables(&self) -> usize {
        self.tables.len()
    }

    /// The executable strings it holds.
    pub fn strings(&self) -> usize {
        self.strings.len()
    }

    /// Slice K3: the object names it holds.
    pub fn look_entries(&self) -> usize {
        self.look.len()
    }

    /// Slice K3: the books (and signs) it holds.
    pub fn books(&self) -> usize {
        self.books.len()
    }

    /// Slice K4: the `u` program's strings it holds.
    pub fn program_strings(&self) -> usize {
        self.program.len()
    }

    /// Slice K5a: the keywords it gives aliases (over every script).
    pub fn aliased_keywords(&self) -> usize {
        self.aliases.values().map(BTreeMap::len).sum()
    }

    /// Slice K5a: the words a player may type for the keyword `keyword` (its bytes up to a NUL,
    /// case-folded) of the script (`letter`, `item`), if the pack gives it any.
    pub fn aliases(&self, (letter, item): ScriptKey, keyword: &[u8]) -> Option<&[Vec<u8>]> {
        self.aliases
            .get(&(letter, item))?
            .get(keyword)
            .map(Vec::as_slice)
    }

    fn script_keys(&self) -> BTreeSet<(u8, u16)> {
        self.runs
            .keys()
            .chain(self.tables.keys())
            .map(|&(a, i, _)| (a, i))
            .collect()
    }

    /// The translation keyed `key`, if the pack has one.
    pub fn run(&self, key: RunKey) -> Option<&RunText> {
        self.runs.get(&key)
    }

    /// The table string keyed `key`, if the pack has one.
    pub fn table(&self, key: RunKey) -> Option<&TableText> {
        self.tables.get(&key)
    }

    /// The executable string whose English hashes to `hash`, if the pack has one.
    pub fn string(&self, hash: u64) -> Option<&StringText> {
        self.strings.get(&hash)
    }

    /// Slice K3: the object name of look entry `index`, if the pack has one.
    pub fn look(&self, index: u32) -> Option<&RunText> {
        self.look.get(&index)
    }

    /// Slice K3: the book of `book.dat` index `index`, if the pack has one.
    pub fn book(&self, index: u32) -> Option<&RunText> {
        self.books.get(&index)
    }

    /// Slice K4: the string of the `u` program whose English hashes to `hash`, if the pack has
    /// one.
    pub fn program(&self, hash: u64) -> Option<&StringText> {
        self.program.get(&hash)
    }

    /// The table string of the script (`letter`, `item`) whose bytes hold `offset` after their
    /// start (a read of its rest), if the pack has one.
    fn table_around(&self, (letter, item, offset): RunKey) -> Option<&TableText> {
        let (&(l, i, at), t) = self.tables.range(..(letter, item, offset)).next_back()?;
        (l == letter && i == item && at < offset && offset < at + t.len).then_some(t)
    }

    /// Checks the pack against the conversation archives `archive` gives by letter (the bytes
    /// of `converse.<letter>` the engine reads): the fingerprint of the scripts it names (by its
    /// runs and its table strings), then each run inside its script, of text bytes only, with its
    /// hash, and each table string inside its script up to a NUL, with its hash. Any difference is
    /// a mismatch of the conversation part.
    pub fn check<'f>(&self, archive: impl Fn(u8) -> Option<&'f [u8]>) -> Result<(), Mismatch> {
        let bad = Mismatch {
            part: Part::Converse,
        };
        let mut libs: BTreeMap<u8, ConverseLib> = BTreeMap::new();
        let keys = self.script_keys();
        for &(letter, _) in &keys {
            if libs.contains_key(&letter) {
                continue;
            }
            let file = archive(letter).ok_or(bad)?;
            libs.insert(letter, ConverseLib::parse(file).map_err(|_| bad)?);
        }
        let script = |letter: u8, item: u16| {
            libs.get(&letter)?
                .scripts
                .get(usize::from(item))?
                .as_ref()
                .map(|s| s.bytecode.as_slice())
        };
        let mut hashes = Vec::with_capacity(keys.len());
        for &(letter, item) in &keys {
            hashes.push((letter, item, hash64(script(letter, item).ok_or(bad)?)));
        }
        if fingerprint(hashes) != self.converse_base {
            return Err(bad);
        }
        for (&(letter, item, offset), r) in &self.runs {
            let bytes = script(letter, item).ok_or(bad)?;
            if !source_ok(bytes, offset, r) {
                return Err(bad);
            }
        }
        for (&(letter, item, offset), t) in &self.tables {
            let bytes = script(letter, item).ok_or(bad)?;
            if !table_ok(bytes, offset, t) {
                return Err(bad);
            }
        }
        Ok(())
    }

    /// Checks the executable strings against the catalogue of the game's `game`
    /// ([`strings::Catalogue`]): every string the catalogue has (by hash and length) holds the
    /// same `%` directives, in the same order, as its English ([`strings::directives`]). A pack
    /// string the catalogue lacks is never looked up (another version's, or one no site shows)
    /// and is not checked. A difference is a mismatch of the strings part.
    pub fn check_strings(&self, catalogue: &strings::Catalogue) -> Result<(), Mismatch> {
        for (&hash, t) in &self.strings {
            let Some(e) = catalogue.get(hash) else {
                continue;
            };
            if e.text.len() == t.len as usize
                && strings::directives(&e.text) != strings::directives(&t.text)
            {
                return Err(Mismatch {
                    part: Part::Strings,
                });
            }
        }
        Ok(())
    }

    /// Slice K4: checks the `u` program's strings against the catalogue of the user's `u`
    /// ([`program::Catalogue`]): every string whose hash the catalogue has keeps its English's
    /// length, the same `printf` codes in the same order ([`program::directives`]; `%B %F %X %Y
    /// %P` included) and, for the prompt of a key read, its keys ([`program::keys_kept`]). A pack
    /// string the catalogue lacks is kept, never looked up and not refused (a pack outlives a
    /// narrower catalogue, as [`Pack::check_strings`] has it). A difference is a mismatch of the
    /// `u` part.
    pub fn check_program(&self, catalogue: &program::Catalogue) -> Result<(), Mismatch> {
        for (&hash, t) in &self.program {
            let Some(e) = catalogue.get(hash) else {
                continue;
            };
            if e.text.len() != t.len as usize
                || program::directives(&e.text) != program::directives(&t.text)
                || !program::keys_kept(&e.text, &t.text, &e.keys)
            {
                return Err(Mismatch {
                    part: Part::Program,
                });
            }
        }
        Ok(())
    }

    /// Slice K5a: checks the keyword aliases against the game's word delimiters (the bytes the
    /// keyword test splits the input at, [`crate::converse::Msg::Delimiters`]): a word that holds
    /// one never comes whole out of the split, so it is a mismatch of the aliases part.
    pub fn check_aliases(&self, delimiters: &[u8]) -> Result<(), Mismatch> {
        let split = self
            .aliases
            .values()
            .flat_map(BTreeMap::values)
            .flatten()
            .any(|w| w.iter().any(|b| delimiters.contains(b)));
        if split {
            return Err(Mismatch {
                part: Part::Aliases,
            });
        }
        Ok(())
    }

    /// Slice K3: checks the object names and the books against the files the engine reads
    /// (`look.lzd` decoded, `book.dat`; a part the pack does not have needs no file): every entry
    /// is one of the file's units ([`texts::look_units`], [`texts::book_units`]), its English of
    /// the entry's length and hash, and its text holds the same `%` directives in the same order as
    /// that English. A difference is a mismatch of that part.
    ///
    /// `book.dat` is read by its bare name here; Look reads the name the game gives
    /// (`objects::commands::look`'s), and checks each book it reads again
    /// ([`book_at`]), so a file other than the one checked shows English.
    pub fn check_texts(&self, files: &Files) -> Result<(), Mismatch> {
        let same = |units: &BTreeMap<u32, Vec<u8>>, entries: &BTreeMap<u32, RunText>| {
            entries.iter().all(|(index, t)| {
                units.get(index).is_some_and(|en| {
                    en.len() == t.len as usize
                        && hash64(en) == t.hash
                        && strings::directives(en) == strings::directives(&t.text)
                })
            })
        };
        if !self.look.is_empty() {
            let bad = Mismatch { part: Part::Look };
            let table = files
                .get(texts::LOOK_FILE)
                .and_then(|f| u6_formats::look::LookTable::from_lzd(f).ok())
                .ok_or(bad)?;
            if !same(&keyed(texts::look_units(&table)), &self.look) {
                return Err(bad);
            }
        }
        if !self.books.is_empty() {
            let bad = Mismatch { part: Part::Books };
            let file = files.get(texts::BOOK_FILE).ok_or(bad)?;
            if !same(&keyed(texts::book_units(file)), &self.books) {
                return Err(bad);
            }
        }
        Ok(())
    }
}

/// Units by index, for [`Pack::check_texts`].
fn keyed(units: Vec<(usize, Vec<u8>)>) -> BTreeMap<u32, Vec<u8>> {
    units
        .into_iter()
        .filter_map(|(i, en)| Some((u32::try_from(i).ok()?, en)))
        .collect()
}

/// The JSON of a map of strings by hash, in hash order.
fn strings_json(map: &BTreeMap<u64, StringText>) -> Vec<StringJson> {
    map.iter()
        .map(|(&hash, t)| StringJson {
            hash: hex(hash),
            len: t.len,
            text: String::from_utf8(t.text.clone()).expect("checked as UTF-8"),
        })
        .collect()
}

/// Slice K5a: the JSON of the keyword aliases, by script then keyword (`None`, left out of the
/// file, when there are none).
fn aliases_json(
    aliases: &BTreeMap<ScriptKey, Aliases>,
) -> Option<Box<serde_json::value::RawValue>> {
    let entries: Vec<AliasJson> = aliases
        .iter()
        .flat_map(|(&(a, item), keywords)| {
            keywords.iter().map(move |(keyword, words)| AliasJson {
                archive: char::from(a).to_string(),
                item,
                keyword: String::from_utf8(keyword.clone()).expect("checked as ASCII"),
                words: words
                    .iter()
                    .map(|w| String::from_utf8(w.clone()).expect("checked as UTF-8"))
                    .collect(),
            })
        })
        .collect();
    (!entries.is_empty()).then(|| {
        serde_json::value::to_raw_value(&entries).expect("an alias has only serialisable fields")
    })
}

/// The JSON of a map of object names or books, in index order.
fn entries_json(map: &BTreeMap<u32, RunText>) -> Vec<EntryJson> {
    map.iter()
        .map(|(&index, t)| EntryJson {
            index,
            len: t.len,
            hash: hex(t.hash),
            text: String::from_utf8(t.text.clone()).expect("checked as UTF-8"),
        })
        .collect()
}

/// The archive letter a key names (`a`, `b`).
fn archive_of(archive: &str) -> Result<u8, PackError> {
    match archive.as_bytes() {
        [l] if ARCHIVES.contains(l) => Ok(*l),
        _ => Err(PackError::Key),
    }
}

/// Whether `script` holds, at `offset`, the string `t` was made from: its bytes, none of them a
/// NUL, then the NUL that ends it, their hash the string's.
fn table_ok(script: &[u8], offset: u32, t: &TableText) -> bool {
    let (at, len) = (offset as usize, t.len as usize);
    let Some(bytes) = at.checked_add(len).and_then(|end| script.get(at..end)) else {
        return false;
    };
    len > 0 && !bytes.contains(&0) && script.get(at + len) == Some(&0) && hash64(bytes) == t.hash
}

/// Whether `script` holds, at `offset`, the bytes `run` was made from: inside it, text bytes
/// only (so passing over them skips no op), their hash the run's.
fn source_ok(script: &[u8], offset: u32, run: &RunText) -> bool {
    let (at, len) = (offset as usize, run.len as usize);
    let Some(bytes) = at.checked_add(len).and_then(|end| script.get(at..end)) else {
        return false;
    };
    len > 0 && bytes.iter().all(|&b| is_text(b)) && hash64(bytes) == run.hash
}

fn check_key((_, _, offset): RunKey, len: u32) -> Result<(), PackError> {
    let end = u64::from(offset) + u64::from(len);
    if len == 0 || end > SCRIPT_LEN as u64 {
        return Err(PackError::Key);
    }
    Ok(())
}

fn check_tool(tool: &str) -> Result<(), PackError> {
    let ok = !tool.is_empty()
        && tool.len() <= MAX_TOOL_CHARS
        && tool.bytes().all(|b| (0x20..0x7f).contains(&b));
    if ok { Ok(()) } else { Err(PackError::Tool) }
}

/// Hits and misses of a part (counts only). Cells, so a read for display that holds the state
/// shared counts too.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Units put or shown from the pack.
    pub hits: Cell<u32>,
    /// Catalogued units shown with the pack installed that it does not have: in English.
    pub misses: Cell<u32>,
    /// 2026-10-27 (spec 16 item 1): executable strings shown with the pack installed that the
    /// catalogue does not hold ([`strings::Catalogue::uncatalogued`]): in English whatever the
    /// pack, and neither a hit nor a miss. Only [`LocaleState::texts`] counts them.
    pub uncatalogued: Cell<u32>,
}

impl Counts {
    fn hit(&self) {
        self.hits.set(self.hits.get().saturating_add(1));
    }

    fn miss(&self) {
        self.misses.set(self.misses.get().saturating_add(1));
    }

    fn not_catalogued(&self) {
        self.uncatalogued
            .set(self.uncatalogued.get().saturating_add(1));
    }

    /// `[hits, misses]`.
    pub fn get(&self) -> [u32; 2] {
        [self.hits.get(), self.misses.get()]
    }

    /// `[hits, misses]`, both set back to 0 (slice K4: what `u` counted, moved to the state's).
    pub fn take(&self) -> [u32; 2] {
        [self.hits.take(), self.misses.take()]
    }

    /// Adds `[hits, misses]`.
    pub fn add(&self, [hits, misses]: [u32; 2]) {
        self.hits.set(self.hits.get().saturating_add(hits));
        self.misses.set(self.misses.get().saturating_add(misses));
    }
}

/// The locale layer's state: the installed pack, the Hangul table the platform gave with it,
/// the catalogue of the game's strings read when it was installed, and what was counted (counts
/// only: never a key or a text).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocaleState {
    pub pack: Option<Rc<Pack>>,
    pub hangul: Option<Rc<HangulFont>>,
    /// Runs put from the pack.
    pub hits: u32,
    /// Runs started with the pack installed that it does not have (or whose bytes changed):
    /// printed in English.
    pub misses: u32,
    /// Slice K2: the executable strings the engine shows ([`strings::Catalogue`]).
    pub catalogue: Option<Rc<strings::Catalogue>>,
    /// Slice K2: table strings read at their start.
    pub tables: Counts,
    /// Slice K2: catalogued executable strings shown (formats, messages and labels).
    pub texts: Counts,
    /// Slice K3: object names shown (a look entry read for display).
    pub look: Counts,
    /// Slice K3: books and signs read.
    pub books: Counts,
    /// Slice K4: the strings the `u` program prints ([`program::Catalogue`]), read from the
    /// user's `u` when the pack was installed.
    pub program_catalogue: Option<Rc<program::Catalogue>>,
    /// Slice K4: catalogued strings of `u` printed (the formats its print entry puts).
    pub program: Counts,
}

/// Installs `pack` on `state` once it checks against the conversation archives of `files`, as
/// the script loader names them ([`crate::converse::archive_name`]; [`Pack::check`]): the text
/// layer takes UTF-8 and draws Hangul from `hangul` ([`apply`]). A mismatch changes nothing.
///
/// Slice K2: the catalogue of the executable's strings is read from `files`' `game` (none there:
/// an empty one), and the pack's strings are checked against it ([`Pack::check_strings`]).
/// Slice K3: then its object names and books against `files`' `look.lzd` and `book.dat`
/// ([`Pack::check_texts`]). Slice K4: then its `u` strings against the catalogue of `files`' `u`
/// ([`program::Catalogue::from_files`], [`Pack::check_program`]). Slice K5a: then its keyword
/// aliases against the game's word delimiters, those of the converse messages `state` holds
/// (none read: no delimiter; [`Pack::check_aliases`]).
pub fn install(
    state: &mut State,
    files: &Files,
    pack: Rc<Pack>,
    hangul: Option<Rc<HangulFont>>,
) -> Result<(), Mismatch> {
    let catalogue = files
        .get("game")
        .map(strings::Catalogue::from_game)
        .unwrap_or_default();
    install_catalogued(state, files, pack, hangul, catalogue)
}

/// [`install`] with the catalogue of the executable's strings given (tests: a synthetic one).
pub(crate) fn install_catalogued(
    state: &mut State,
    files: &Files,
    pack: Rc<Pack>,
    hangul: Option<Rc<HangulFont>>,
    catalogue: strings::Catalogue,
) -> Result<(), Mismatch> {
    let program = program::Catalogue::from_files(files);
    install_parts(state, files, pack, hangul, catalogue, program)
}

/// [`install`] with both catalogues given, the executable's and `u`'s (tests: synthetic ones).
pub(crate) fn install_parts(
    state: &mut State,
    files: &Files,
    pack: Rc<Pack>,
    hangul: Option<Rc<HangulFont>>,
    catalogue: strings::Catalogue,
    program: program::Catalogue,
) -> Result<(), Mismatch> {
    pack.check(|letter| files.get(&crate::converse::archive_name(state, letter)))?;
    pack.check_strings(&catalogue)?;
    pack.check_texts(files)?;
    pack.check_program(&program)?;
    let delimiters = state
        .converse
        .text
        .as_ref()
        .map(|t| t.get(crate::converse::Msg::Delimiters));
    pack.check_aliases(delimiters.unwrap_or_default())?;
    state.locale = LocaleState {
        pack: Some(pack),
        hangul,
        catalogue: Some(Rc::new(catalogue)),
        program_catalogue: Some(Rc::new(program)),
        ..LocaleState::default()
    };
    apply(state);
    Ok(())
}

/// What an installed pack sets in the text layer: the UTF-8 assembler on and the Hangul table.
/// A new process's state ([`crate::main_loop`]'s hand-over) takes it again from here.
pub(crate) fn apply(state: &mut State) {
    if state.locale.pack.is_some() {
        state.ui.text.set_wide(true);
        state.ui.hangul = state.locale.hangul.clone();
    }
}

/// The run that starts at `at` in the script loaded, as the installed pack has it: the pack
/// (the caller reads the run from it) and the run's key. `None`, with a miss counted, when the
/// pack has no such run or the script's bytes there are not the ones it was made from; `None`
/// without a count when no pack is installed or no script is loaded.
pub(crate) fn run_at(state: &mut State, at: i32) -> Option<(Rc<Pack>, RunKey)> {
    let pack = state.locale.pack.clone()?;
    let (letter, item) = state.converse.script_key?;
    let key = (letter, item, u32::try_from(at).ok()?);
    let found = pack
        .run(key)
        .is_some_and(|r| source_ok(&state.converse.script, key.2, r));
    if found {
        state.locale.hits = state.locale.hits.saturating_add(1);
        Some((pack, key))
    } else {
        state.locale.misses = state.locale.misses.saturating_add(1);
        None
    }
}

/// Slice K2: what the installed pack has for `english` (bytes up to a NUL) where game code shows
/// it, counted: `None` without a pack, for a string the catalogue does not have (no hit or miss;
/// a print hook never has one under [`strings::MIN_BYTES`], [`strings::Catalogue::of`], while a
/// `label` read has the catalogue's short labels, [`strings::Catalogue::label_of`]; 2026-10-27:
/// counted uncatalogued when it is an executable string, [`strings::Catalogue::uncatalogued`]),
/// and for a catalogued one the pack lacks or whose text does not keep to its place
/// ([`within_place`]: a miss); else the pack's language and text (a hit).
fn lookup(state: &State, english: &[u8], label: bool) -> Option<(Language, Vec<u8>)> {
    let l = &state.locale;
    let pack = l.pack.as_ref()?;
    let english = &english[..english
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(english.len())];
    let catalogue = l.catalogue.as_ref()?;
    let found = if label {
        catalogue.label_of(english)
    } else {
        catalogue.of(english)
    };
    let Some(entry) = found else {
        if catalogue.uncatalogued(english) {
            l.texts.not_catalogued();
        }
        return None;
    };
    match pack
        .string(hash64(english))
        .filter(|t| t.len as usize == english.len() && within_place(entry, &t.text))
    {
        Some(t) => {
            l.texts.hit();
            Some((pack.language, t.text.clone()))
        }
        None => {
            l.texts.miss();
            None
        }
    }
}

/// 2026-10-24: whether the pack's `text` for catalogued `entry` keeps to the place it is drawn
/// at: for a string with a cell budget ([`strings::Entry::cells`], a panel's), its cells as the
/// text layer counts them ([`crate::ui::wide::decode`]: a Hangul syllable one, every other byte
/// one) at most the budget, and as many line breaks as its English; any text for a string with
/// no budget. Display only: logic and the English are untouched.
fn within_place(entry: &strings::Entry, text: &[u8]) -> bool {
    let breaks = |b: &[u8]| b.iter().filter(|&&c| c == b'\n').count();
    entry.cells.is_none_or(|cells| {
        crate::ui::wide::decode(text).len() <= usize::from(cells)
            && breaks(text) == breaks(&entry.text)
    })
}

/// Slice K2: the translation of the format, string or line `english` game code prints (the
/// outermost call: `printf`'s format, `puts`, `put_raw_str`), with the pack's language for its
/// grammar; `None` (English) as [`lookup`] says. Never called on what an escape generates.
pub(crate) fn exe_string(state: &State, english: &[u8]) -> Option<(Language, Vec<u8>)> {
    lookup(state, english, false)
}

/// Slice K2: a label read for display, a pointer table's (a command's name, a shared message, a
/// wind's name, a combat-mode name) or (2026-10-24) a panel label at its own address (a weight
/// label, `ui::tables::SHOWN_AT`): the pack's text for it, else `english` itself ([`lookup`],
/// counted), a label under [`strings::MIN_BYTES`] included (2026-10-21). Only for reads whose
/// result is shown; logic reads the table as it is.
pub fn shown(state: &State, english: Vec<u8>) -> Vec<u8> {
    match lookup(state, &english, true) {
        Some((_, text)) => text,
        None => english,
    }
}

/// [`shown`] without counting, with the language of the pack's text (`None`: English) and only
/// for a catalogued label: the copy of the escape layer that `printf`'s cell count runs reads a
/// slot as the escape layer will, and counts nothing. It finds what [`exe_string`] finds, not a
/// label under [`strings::MIN_BYTES`] nor a text that does not keep to its place
/// ([`within_place`]): the escape layer reads the slot it copies (`converse::vars::shown_ref`)
/// through [`exe_string`].
pub(crate) fn label_quiet(state: &State, english: &[u8]) -> Option<Vec<u8>> {
    let l = &state.locale;
    let pack = l.pack.as_ref()?;
    let entry = l.catalogue.as_ref()?.of(english)?;
    pack.string(hash64(english))
        .filter(|t| t.len as usize == english.len() && within_place(entry, &t.text))
        .map(|t| t.text.clone())
}

/// Slice K2: the table string read at `at` in the loaded script (the table reader's hook, for
/// op `b5 d2` and op `a6 d2`), as the installed pack has it: its source length, its language and
/// its text. `capacity` is the most bytes the place it goes to holds (`a6`'s slot), `None` for
/// a print. `None`, with a miss counted, when the pack has no string there, the script's bytes
/// are not the ones it was made from, or the text is over `capacity`; `None` without a count
/// with no pack or no script loaded, for an empty string (no unit: nothing to translate), and
/// for a read that starts inside a string the pack has (its rest, which is not a unit).
pub(crate) fn table_at(
    state: &State,
    at: i32,
    capacity: Option<usize>,
) -> Option<(usize, Language, Vec<u8>)> {
    let l = &state.locale;
    let pack = l.pack.as_ref()?;
    let (letter, item) = state.converse.script_key?;
    let key = (letter, item, u32::try_from(at).ok()?);
    if state
        .converse
        .script
        .get(key.2 as usize)
        .is_none_or(|&b| b == 0)
    {
        return None;
    }
    let found = pack.table(key).filter(|t| {
        table_ok(&state.converse.script, key.2, t) && capacity.is_none_or(|c| t.text.len() <= c)
    });
    match found {
        Some(t) => {
            l.tables.hit();
            Some((t.len as usize, pack.language, t.text.clone()))
        }
        None if pack.table_around(key).is_some() => None,
        None => {
            l.tables.miss();
            None
        }
    }
}

/// Slice K3: the object name of look entry `index` (its English `english`, the entry's bytes) as
/// the installed pack has it, for a read whose result is shown: `None` without a pack or for an
/// entry that is no unit ([`texts::is_look_unit`]; not counted), and when the pack has no such entry or the table's bytes there are not the ones it was
/// made from (a miss, when `count`); else the text (a hit, when `count`), its plural markers as
/// the pack's language reads them: in Korean dropped (with the letters after them), in English
/// kept for the printer. `count` off: the article's question, which counts nothing.
pub(crate) fn look_at(state: &State, index: usize, english: &[u8], count: bool) -> Option<Vec<u8>> {
    let l = &state.locale;
    let pack = l.pack.as_ref()?;
    if !texts::is_look_unit(english) {
        return None;
    }
    let found = u32::try_from(index)
        .ok()
        .and_then(|i| pack.look(i))
        .filter(|t| t.len as usize == english.len() && t.hash == hash64(english));
    let Some(t) = found else {
        if count {
            l.look.miss();
        }
        return None;
    };
    if count {
        l.look.hit();
    }
    Some(match pack.language {
        Language::English => t.text.clone(),
        Language::Korean => crate::objects::drop_plural_bytes(&t.text),
    })
}

/// Slice K3: whether the object name of look entry `index` shows from a pack whose language is
/// Korean (no article goes before it then). Counts nothing.
pub(crate) fn look_is_korean(state: &State, index: usize, english: &[u8]) -> bool {
    state
        .locale
        .pack
        .as_ref()
        .is_some_and(|p| p.language == Language::Korean)
        && look_at(state, index, english, false).is_some()
}

/// Slice K3: the book `book.dat` holds at `index` (Look reads it from `file`; `english` is what it
/// read there, [`texts::book_text`]) as the installed pack has it, with the pack's language for
/// its grammar. `None` without a pack or for a read that is no unit (not counted), and when the
/// pack has no such book, the bytes read are not the ones it was made from or its text is not
/// under [`texts::BOOK_BYTES`] (a miss); else a hit.
pub(crate) fn book_at(
    state: &State,
    file: &[u8],
    index: usize,
    english: &[u8],
) -> Option<(Language, Vec<u8>)> {
    let l = &state.locale;
    let pack = l.pack.as_ref()?;
    if english.is_empty() || !texts::is_book_unit(file, index) {
        return None;
    }
    let found = u32::try_from(index)
        .ok()
        .and_then(|i| pack.book(i))
        .filter(|t| {
            t.len as usize == english.len()
                && t.hash == hash64(english)
                && t.text.len() < texts::BOOK_BYTES
        });
    match found {
        Some(t) => {
            l.books.hit();
            Some((pack.language, t.text.clone()))
        }
        None => {
            l.books.miss();
            None
        }
    }
}

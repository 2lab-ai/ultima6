//! Port-only (spec 08 Design 2 and 3, slice K3): the object names of `look.lzd` and the books
//! and signs of `book.dat` as the locale pack's units: what a pack keys them by, what the install
//! checks them against ([`super::Pack::check_texts`]) and what the page's Game strings download
//! lists of them ([`export`]).
//!
//! **Look.** A unit is an entry of the decoded table with a text of 1 to [`MAX_LOOK_BYTES`]
//! bytes (an empty one shows the nothing word, which is not the table's), each a byte a pack's
//! text can hold ([`showable`]): its index, its bytes. The walk past the entries into the
//! table's trailer (`crate::objects::look_text`) reads no unit, and an entry with another byte
//! (a control character) is none: it stays English.
//!
//! **Books.** Look reads a book or a sign as 0x1292d0 does ([`book_text`]): the 16-bit offset of
//! entry quality − 1, then the text there. A unit is an index of the offset table as U6Lib counts
//! it (formats.md §2: the smallest offset that is not 0, over two), whose offset is not 0 and
//! whose text is not empty and [`showable`]. Books and signs share the one key space.
//!
//! Both kinds of text are printed as `printf` formats, so a translation keeps the English's `%`
//! directives in order (checked at install), and a book's is under [`BOOK_BYTES`].

use serde::{Deserialize, Serialize};
use u6_formats::look::LookTable;

use crate::converse::disasm::catalog::{hash64, hex64};

/// The file the object names are read from.
pub const LOOK_FILE: &str = "look.lzd";
/// The file the books and signs are read from (by its bare name; Look reads the name the game
/// gives).
pub const BOOK_FILE: &str = "book.dat";
/// The most entries a look table has (one per tile at most, [`u6_formats::look::LOOK_TILES`]).
pub const LOOK_ENTRIES: usize = u6_formats::look::LOOK_TILES as usize;
/// The longest English object name a pack keys (the longest string the game reads,
/// [`super::strings::MAX_BYTES`]).
pub const MAX_LOOK_BYTES: usize = super::strings::MAX_BYTES;
/// The most bytes an object name's translation may hold (three per byte of the longest English).
pub const MAX_LOOK_TEXT_BYTES: usize = 3 * MAX_LOOK_BYTES;
/// The buffer Look reads a book into (0x1292d0): a book's English is at most this long, and its
/// translation must be shorter (room for its NUL).
pub const BOOK_BYTES: usize = crate::objects::commands::BOOK_BYTES;
/// The `book.dat` indices a quality names (1 to 255, less one).
pub const BOOK_INDICES: usize = 255;

/// The object names of `table` a pack may translate: each entry whose text is a unit
/// ([`is_look_unit`]), its index and its bytes (one per character, as the table was decoded).
pub fn look_units(table: &LookTable) -> Vec<(usize, Vec<u8>)> {
    table
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (i, latin1(&e.text)))
        .filter(|(_, b)| is_look_unit(b))
        .collect()
}

/// Whether a look entry's bytes are a unit: 1 to [`MAX_LOOK_BYTES`] of them, [`showable`].
pub fn is_look_unit(english: &[u8]) -> bool {
    (1..=MAX_LOOK_BYTES).contains(&english.len()) && showable(english)
}

/// Whether every byte of an English text is one a pack's text can hold too (the line feed and
/// printable ASCII, as the reader takes them), so the identity of it is a pack text.
pub fn showable(english: &[u8]) -> bool {
    english
        .iter()
        .all(|&b| b == b'\n' || (0x20..=0x7f).contains(&b))
}

/// The bytes of a decoded look string (one per character, U+0000..U+00FF).
pub(crate) fn latin1(text: &str) -> Vec<u8> {
    text.chars().map(|c| c as u32 as u8).collect()
}

/// The text Look reads for `book.dat` entry `index` (quality − 1; 0x1292d0): the entry's 16-bit
/// little-endian offset at 2 × `index`, then the bytes there, at most [`BOOK_BYTES`], to their
/// NUL; empty when the offset is not in the file.
pub fn book_text(file: &[u8], index: usize) -> Vec<u8> {
    let at = index.wrapping_mul(2);
    file.get(at..at.wrapping_add(2))
        .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
        .map(|off| {
            file.iter()
                .skip(off)
                .take(BOOK_BYTES)
                .copied()
                .take_while(|&b| b != 0)
                .collect()
        })
        .unwrap_or_default()
}

/// The entries of `book.dat`'s offset table as U6Lib counts them: the smallest offset that is not
/// 0, over two, scanning entries until the table reaches it (0 when no offset is set).
fn book_count(file: &[u8]) -> usize {
    let offset = |i: usize| {
        file.get(2 * i..2 * i + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let mut min = file.len();
    let mut i = 0;
    while (i + 1) * 2 <= min {
        match offset(i) {
            Some(o) if o != 0 && usize::from(o) < min => min = usize::from(o),
            Some(_) => {}
            None => break,
        }
        i += 1;
    }
    if min == file.len() { 0 } else { min / 2 }
}

/// Whether `book.dat` entry `index` is a unit: inside the offset table, its offset not 0, its
/// text ([`book_text`]) not empty and [`showable`].
pub fn is_book_unit(file: &[u8], index: usize) -> bool {
    index < book_count(file).min(BOOK_INDICES)
        && file
            .get(2 * index..2 * index + 2)
            .is_some_and(|b| b != [0, 0])
        && {
            let text = book_text(file, index);
            !text.is_empty() && showable(&text)
        }
}

/// The books and signs of `book.dat` a pack may translate: each unit's index and the text Look
/// reads there.
pub fn book_units(file: &[u8]) -> Vec<(usize, Vec<u8>)> {
    (0..book_count(file).min(BOOK_INDICES))
        .filter(|&i| is_book_unit(file, i))
        .map(|i| (i, book_text(file, i)))
        .collect()
}

/// What a text of the export is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    /// An object name (`look.lzd`): a plain noun, with its plural markers.
    Look,
    /// A book or a sign (`book.dat`).
    Book,
}

/// One object name or book of the page's Game strings download (slice K3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportText {
    pub kind: TextKind,
    /// Its key in the pack: the look entry's index, or the `book.dat` index.
    pub index: u32,
    /// [`hash64`] of its bytes, 16 lower-case hex digits.
    #[serde(with = "hex64")]
    pub hash: u64,
    /// Its length in bytes.
    pub len: u32,
    /// Its bytes, one character per byte (U+0000..U+00FF).
    pub text: String,
}

impl ExportText {
    fn of(kind: TextKind, (index, bytes): (usize, Vec<u8>)) -> ExportText {
        ExportText {
            kind,
            index: index as u32,
            hash: hash64(&bytes),
            len: bytes.len() as u32,
            text: bytes.iter().map(|&b| char::from(b)).collect(),
        }
    }

    /// Whether its text, length and hash agree, and its index is one a pack may key.
    pub(super) fn reads_back(&self) -> bool {
        let bytes: Option<Vec<u8>> = self.text.chars().map(|c| u8::try_from(c).ok()).collect();
        let limit = match self.kind {
            TextKind::Look => (LOOK_ENTRIES, MAX_LOOK_BYTES),
            TextKind::Book => (BOOK_INDICES, BOOK_BYTES),
        };
        (self.index as usize) < limit.0
            && bytes.is_some_and(|b| {
                b.len() == self.len as usize
                    && !b.is_empty()
                    && b.len() <= limit.1
                    && hash64(&b) == self.hash
            })
    }
}

/// The object names and the books of the files (by their bare names), for the export: every look
/// unit a pack can key, then every book unit, in index order. A file that is not there gives none;
/// one that does not read fails, with a fixed sentence (no value of the file).
pub fn export(files: &crate::files::Files) -> Result<Vec<ExportText>, &'static str> {
    let mut out = Vec::new();
    if let Some(file) = files.get(LOOK_FILE) {
        let table = LookTable::from_lzd(file).map_err(|_| "look.lzd does not read")?;
        out.extend(
            look_units(&table)
                .into_iter()
                .map(|u| ExportText::of(TextKind::Look, u)),
        );
    }
    if let Some(file) = files.get(BOOK_FILE) {
        out.extend(
            book_units(file)
                .into_iter()
                .map(|u| ExportText::of(TextKind::Book, u)),
        );
    }
    Ok(out)
}

//! Port-only (spec 08 Design 2, slice K4): the catalogue of the strings the `u` program prints
//! (the opening, the introduction story, the walk to the gypsy and her questions, character
//! creation and the transfer), read from the user's `u` at run time, and the export of it.
//!
//! **What is catalogued.** The formats `u`'s code passes to its `printf` (`intro/text.rs`) at the
//! sites the port knows, each listed next to its address: every format a shown passage's case
//! prints through 0x1142f0 ([`crate::intro::passages::SHOWN`], the cases walked as the port lays
//! them out, [`crate::intro::passages::formats`]; the gypsy's questions are the passages of her
//! pair table, [`crate::intro::gypsy::question_of`]), never the player's name buffer a case
//! pushes; the name and portrait screens' ([`crate::intro::create::PRINTED`]); the transfer's
//! ([`crate::intro::transfer::PRINTED`]). Not the acknowledgments (spec 08 Scope keeps the
//! credits English: their code prints without a lookup), and never a string a format prints
//! through `%s` (the player's name, the sex word, a class). A string under [`MIN_BYTES`], one
//! that may be cut short ([`MAX_BYTES`]), one with a byte a pack text cannot hold (a control
//! character but the line feed, a byte from 0x80) or with no letter outside its `%` codes
//! (`%s\n%s\n`) is not catalogued: it is never substituted and stays English.
//!
//! **Keys** ([`Entry::keys`]): a string printed as the prompt of a key read (the gypsy's
//! questions, the sex question, the portrait screen, the transfer's dialog and sheet) carries the
//! letters that read accepts, from the code's constants or the key tables of `u` it reads at run
//! time (Escape and any other byte but an upper-case letter left out). Of those, every letter the
//! English shows standing alone ([`keys_shown`]: `A)`, `(M)`, `N)ew`) must stand alone in the
//! translation too, in the same order ([`keys_kept`]); a key the English shows only inside a word,
//! or as a word of its own (one followed by whitespace, the article A), is not checked.
//!
//! **Lookup** ([`lookup`], [`shown`]): by content, [`hash64`] of the English bytes, at the `u`
//! program's print entry only (the passages' 0x1142f0 calls, the creation and transfer prints;
//! see [`crate::intro::locale`]), on the format and never on its arguments. At install every pack
//! string the catalogue has must keep its English's length, the same `printf` codes in the same
//! order ([`directives`]: the `u` `printf`'s codes, `%B %F %X %Y %P` included) and its keys,
//! else the pack is refused (the `u` part); a pack string the catalogue lacks is kept, never
//! looked up and not refused (a pack outlives a narrower catalogue, as the game strings' do).
//!
//! **The export** ([`export`]): every catalogued string once, in site order, with its role (the
//! export's `kind`: `opening`, `story`, `gypsy`, `creation`, `transfer`, none a kind of the
//! game's strings or texts), hash, length, text and keys, in the page's Game strings download
//! (`u`). It holds the user's game text and leaves his browser only as his own download
//! (`tests/no_string_bundle_in_repo.rs`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::Counts;
use super::Pack;
use crate::converse::disasm::catalog::{hash64, hex64};
use crate::intro::exe::UExe;

/// The file the strings are read from.
pub const FILE: &str = "u";
/// Strings shorter than this are never substituted (spec 08 Design 2).
pub const MIN_BYTES: usize = super::strings::MIN_BYTES;
/// The longest string a unit is: `UExe::cstr` reads at most 512 bytes, so one of 512 may be cut
/// short of its NUL.
pub const MAX_BYTES: usize = 511;
/// The most bytes a translation may hold (three per byte of the longest English).
pub const MAX_TEXT_BYTES: usize = 3 * MAX_BYTES;

/// What a catalogued string is: where `u` prints it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// A passage of the opening (the room, the storm, the moongate).
    Opening,
    /// A passage of the introduction story.
    Story,
    /// A passage of the walk to the gypsy's wagon, her questions or her farewell.
    Gypsy,
    /// A prompt of the name and portrait screens, or the line that a game is under way.
    Creation,
    /// A line of the transfer's dialog or sheet.
    Transfer,
}

/// The `printf` codes of a format of `u` in order, as `intro::text::printf` reads it: each `%`
/// with the byte after it (`%B %F %X %Y %P`, a conversion, or a byte put as it is: `%%`), a `%`
/// that ends the format alone. A NUL ends the format.
pub fn directives(format: &[u8]) -> Vec<Vec<u8>> {
    let f = &format[..format.iter().position(|&b| b == 0).unwrap_or(format.len())];
    let mut out = Vec::new();
    let mut i = 0;
    while i < f.len() {
        if f[i] == b'%' {
            let end = (i + 2).min(f.len());
            out.push(f[i..end].to_vec());
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// Whether `text` holds a letter outside its `%` codes ([`directives`]).
fn has_words(text: &[u8]) -> bool {
    let mut i = 0;
    while i < text.len() {
        match text[i] {
            b'%' => i += 2,
            c if c.is_ascii_alphabetic() => return true,
            _ => i += 1,
        }
    }
    false
}

/// Whether `text` is a unit: [`MIN_BYTES`] to [`MAX_BYTES`] bytes a pack text can hold too
/// ([`super::texts::showable`]: the line feed and printable ASCII), with a letter of its own.
pub fn is_unit(text: &[u8]) -> bool {
    (MIN_BYTES..=MAX_BYTES).contains(&text.len()) && super::texts::showable(text) && has_words(text)
}

/// The keys of a read among `bytes` (a code's constants or a key table of `u`): its upper-case
/// ASCII letters, each once, in order (Escape and the rest left out).
fn letters(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &b in bytes {
        if b.is_ascii_uppercase() && !out.contains(&b) {
            out.push(b);
        }
    }
    out
}

/// The letters of `keys` that the English `text` (up to a NUL) shows standing alone, each once, in
/// the order they first do: an ASCII letter with no ASCII letter right before or after it (`A)`,
/// `(M)`, `N)ew`) and no ASCII whitespace right after it (space, tab, line feed, carriage return:
/// such a letter is a word, the article A, not a key shown). The byte after a `%` is a code
/// ([`directives`]), not a letter.
pub fn keys_shown(text: &[u8], keys: &[u8]) -> Vec<u8> {
    standing(text, keys, true)
}

/// The letters of `keys` that `text` shows standing alone, as [`keys_shown`] reads them; a letter
/// followed by whitespace counts only when `words` is false (a translation: `A 가시오` shows A).
fn standing(text: &[u8], keys: &[u8], words: bool) -> Vec<u8> {
    let f = &text[..text.iter().position(|&b| b == 0).unwrap_or(text.len())];
    let mut glyphs: Vec<Option<u8>> = Vec::with_capacity(f.len());
    let mut i = 0;
    while i < f.len() {
        if f[i] == b'%' {
            glyphs.push(None);
            i += 2;
        } else {
            glyphs.push(Some(f[i]));
            i += 1;
        }
    }
    let letter = |j: Option<usize>| {
        j.and_then(|j| glyphs.get(j).copied().flatten())
            .is_some_and(|b| b.is_ascii_alphabetic())
    };
    let space = |j: usize| {
        glyphs
            .get(j)
            .copied()
            .flatten()
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
    };
    let mut out = Vec::new();
    for (j, g) in glyphs.iter().enumerate() {
        let Some(b) = *g else { continue };
        if keys.contains(&b)
            && !out.contains(&b)
            && !letter(j.checked_sub(1))
            && !letter(Some(j + 1))
            && (!words || !space(j + 1))
        {
            out.push(b);
        }
    }
    out
}

/// Whether `text` keeps the keys of its English `english`: every letter of `keys` the English
/// shows standing alone ([`keys_shown`]) stands alone in `text` too, in the same order (one it
/// shows besides, such as a key added as `(M)`, is not counted). In `text` a key followed by
/// whitespace still counts (the article rule reads the English only). A string without keys keeps
/// them.
pub fn keys_kept(english: &[u8], text: &[u8], keys: &[u8]) -> bool {
    let en = keys_shown(english, keys);
    let kept: Vec<u8> = standing(text, keys, false)
        .into_iter()
        .filter(|k| en.contains(k))
        .collect();
    kept == en
}

/// One catalogued string: its English bytes, where `u` prints it and the keys of the read that
/// follows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub role: Role,
    pub text: Vec<u8>,
    /// The letters the key read after it accepts (upper-case ASCII, each once); empty when no
    /// key read follows it, or one that takes any key or a typed line.
    pub keys: Vec<u8>,
}

/// The catalogued strings of one `u`, by [`hash64`] of their bytes, in site order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalogue {
    by_hash: BTreeMap<u64, usize>,
    entries: Vec<(u64, Entry)>,
}

impl Catalogue {
    /// The catalogue of `exe`: the formats of every shown passage, of the questions (keyed by
    /// the answers the gypsy waits for), of the creation's and the transfer's prints (each keyed
    /// by the read after it, [`crate::intro::create::keys_after`],
    /// [`crate::intro::transfer::keys_after`]; see the module doc), each [`is_unit`] once (the
    /// first site's role, every site's keys).
    pub fn from_u(exe: &UExe) -> Catalogue {
        use crate::intro::{create, gypsy, passages, transfer};
        let mut c = Catalogue::default();
        let passage = |c: &mut Catalogue, n: u16, role: Role, keys: &[u8]| {
            for at in passages::formats(exe, n) {
                c.add(role, exe.cstr(at), keys);
            }
        };
        for &(first, last, role) in &passages::SHOWN {
            for n in first..=last {
                passage(&mut c, n, role, &[]);
            }
        }
        for a in 0..8 {
            for b in (0..8).filter(|&b| b != a) {
                let n = gypsy::question_of(exe, a, b) as u16;
                passage(&mut c, n, Role::Gypsy, &gypsy::ANSWER_KEYS);
            }
        }
        type KeysAfter = fn(&UExe, u32) -> Vec<u8>;
        let sites: [(&[u32], Role, KeysAfter); 2] = [
            (&create::PRINTED, Role::Creation, create::keys_after),
            (&transfer::PRINTED, Role::Transfer, transfer::keys_after),
        ];
        for (sites, role, keys_after) in sites {
            for &at in sites {
                c.add(role, exe.cstr(at), &keys_after(exe, at));
            }
        }
        c
    }

    /// The catalogue of the `u` among `files` (by its bare name); an empty one when there is none
    /// or it does not read (nothing is substituted then).
    pub fn from_files(files: &crate::files::Files) -> Catalogue {
        files
            .get(FILE)
            .and_then(|f| UExe::parse(f).ok())
            .map(|exe| Catalogue::from_u(&exe))
            .unwrap_or_default()
    }

    /// A catalogue of `entries` (tests: a synthetic `u`'s strings), each added as a site's.
    #[cfg(test)]
    pub(crate) fn from_entries(entries: impl IntoIterator<Item = (Role, Vec<u8>)>) -> Catalogue {
        Catalogue::from_keyed(
            entries
                .into_iter()
                .map(|(role, text)| (role, text, Vec::new())),
        )
    }

    /// A catalogue of `entries` with the keys of each site's read (tests).
    #[cfg(test)]
    pub(crate) fn from_keyed(
        entries: impl IntoIterator<Item = (Role, Vec<u8>, Vec<u8>)>,
    ) -> Catalogue {
        let mut c = Catalogue::default();
        for (role, text, keys) in entries {
            c.add(role, text, &keys);
        }
        c
    }

    /// Adds `text` printed at a site whose read takes `keys` (the letters of them,
    /// [`letters`]): once per content, the first site's role, the keys of every site.
    fn add(&mut self, role: Role, text: Vec<u8>, keys: &[u8]) {
        if !is_unit(&text) {
            return;
        }
        let h = hash64(&text);
        let keys = letters(keys);
        match self.by_hash.get(&h) {
            Some(&i) => {
                let e = &mut self.entries[i].1;
                if e.text == text {
                    for k in keys {
                        if !e.keys.contains(&k) {
                            e.keys.push(k);
                        }
                    }
                }
            }
            None => {
                self.by_hash.insert(h, self.entries.len());
                self.entries.push((h, Entry { role, text, keys }));
            }
        }
    }

    /// The entry whose bytes hash to `hash`.
    pub fn get(&self, hash: u64) -> Option<&Entry> {
        self.by_hash.get(&hash).map(|&i| &self.entries[i].1)
    }

    /// The entry of exactly `bytes`, if catalogued.
    pub fn of(&self, bytes: &[u8]) -> Option<&Entry> {
        if bytes.len() < MIN_BYTES {
            return None;
        }
        self.get(hash64(bytes)).filter(|e| e.text == bytes)
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

    /// The export of the catalogue (the Game strings download's `u`).
    pub fn export(&self) -> Vec<ExportString> {
        self.iter()
            .map(|(hash, e)| ExportString {
                kind: e.role,
                hash,
                len: e.text.len() as u32,
                text: e.text.iter().map(|&b| char::from(b)).collect(),
                keys: e.keys.iter().map(|&b| char::from(b)).collect(),
            })
            .collect()
    }
}

/// One string of `u` in the Game strings download.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportString {
    /// Where `u` prints it.
    pub kind: Role,
    /// [`hash64`] of its bytes, 16 lower-case hex digits: its key in the pack.
    #[serde(with = "hex64")]
    pub hash: u64,
    /// Its length in bytes.
    pub len: u32,
    /// Its bytes, one character per byte (U+0000..U+00FF).
    pub text: String,
    /// The keys of the read after it ([`Entry::keys`]): upper-case ASCII letters, each once; left
    /// out when it has none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub keys: String,
}

impl ExportString {
    /// Whether its text, length and hash agree, its length is a unit's and its keys are
    /// upper-case letters, each once.
    pub(super) fn reads_back(&self) -> bool {
        let bytes: Option<Vec<u8>> = self.text.chars().map(|c| u8::try_from(c).ok()).collect();
        let keys = self.keys.as_bytes();
        bytes.is_some_and(|b| {
            b.len() == self.len as usize
                && (MIN_BYTES..=MAX_BYTES).contains(&b.len())
                && hash64(&b) == self.hash
        }) && letters(keys) == keys
    }
}

/// The strings of the `u` among `files` (by its bare name) for the export: none when there is no
/// `u`; one that does not read as the program fails, with a fixed sentence (no value of the
/// file).
pub fn export(files: &crate::files::Files) -> Result<Vec<ExportString>, &'static str> {
    let Some(file) = files.get(FILE) else {
        return Ok(Vec::new());
    };
    let exe = UExe::parse(file).map_err(|_| "the u program does not read")?;
    Ok(Catalogue::from_u(&exe).export())
}

/// What a pack has for a format the `u` program prints ([`lookup`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Lookup {
    /// A string under [`MIN_BYTES`] or one the catalogue does not have: never counted.
    Uncatalogued,
    /// A catalogued string the pack lacks: a miss when shown.
    Missing,
    /// A catalogued string's text in the pack: a hit when shown.
    Found(Vec<u8>),
}

impl Lookup {
    /// Counted in `counts` as shown from the pack: a hit, or a miss for a catalogued string the
    /// pack lacks; the text, if found.
    pub(crate) fn shown(self, counts: &Counts) -> Option<Vec<u8>> {
        match self {
            Lookup::Uncatalogued => None,
            Lookup::Missing => {
                counts.miss();
                None
            }
            Lookup::Found(t) => {
                counts.hit();
                Some(t)
            }
        }
    }

    /// Counted in `counts` as shown in English instead: a miss for a catalogued string.
    pub(crate) fn shown_english(&self, counts: &Counts) {
        if *self != Lookup::Uncatalogued {
            counts.miss();
        }
    }
}

/// What the installed `pack` has for the format `english` (bytes up to a NUL) the `u` program
/// prints, nothing counted (see [`Lookup`]). Only ever called on a format, never on what it
/// prints through `%s`.
pub(crate) fn lookup(pack: &Pack, catalogue: &Catalogue, english: &[u8]) -> Lookup {
    let english = &english[..english
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(english.len())];
    if english.len() < MIN_BYTES || catalogue.of(english).is_none() {
        return Lookup::Uncatalogued;
    }
    match pack
        .program(hash64(english))
        .filter(|t| t.len as usize == english.len())
    {
        Some(t) => Lookup::Found(t.text.clone()),
        None => Lookup::Missing,
    }
}

/// [`lookup`], counted in `counts` as shown from the pack (tests; the print entry puts it so in
/// `crate::intro::locale::format_in`): `None` for a string under [`MIN_BYTES`] or one `catalogue`
/// does not have (not counted), and for a catalogued one the pack lacks (a miss); else its text
/// (a hit).
#[cfg(test)]
pub(crate) fn shown(
    pack: &Pack,
    catalogue: &Catalogue,
    counts: &Counts,
    english: &[u8],
) -> Option<Vec<u8>> {
    lookup(pack, catalogue, english).shown(counts)
}

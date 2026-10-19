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
//! Rights (spec 08): a pack made from the owner's files holds his game text; it lives in
//! `~/Games/ultima6/i18n/ko/` and his browser storage, never in the repository or under `site/`
//! (`tests/no_locale_pack_in_repo.rs`).

#[cfg(test)]
mod tests;

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

/// A run's key: (archive letter, item, offset in the script).
pub type RunKey = (u8, u16, u32);

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

/// A pack's parts, as a mismatch names them (this slice has the conversation runs only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Converse,
}

impl Part {
    /// The part's name in the pack (`base.<name>`).
    pub fn name(self) -> &'static str {
        match self {
            Part::Converse => "converse",
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
    /// The fingerprint of the scripts its converse map names ([`fingerprint`]).
    pub converse_base: u64,
    runs: BTreeMap<RunKey, RunText>,
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
        })
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
        let mut runs = Vec::with_capacity(json.converse.len());
        for r in json.converse {
            let archive = match r.archive.as_bytes() {
                [l] if ARCHIVES.contains(l) => *l,
                _ => return Err(PackError::Key),
            };
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
        Pack::new(language, &json.tool, converse_base, runs)
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

    /// The scripts its runs are in.
    pub fn scripts(&self) -> usize {
        self.script_keys().len()
    }

    fn script_keys(&self) -> BTreeSet<(u8, u16)> {
        self.runs.keys().map(|&(a, i, _)| (a, i)).collect()
    }

    /// The translation keyed `key`, if the pack has one.
    pub fn run(&self, key: RunKey) -> Option<&RunText> {
        self.runs.get(&key)
    }

    /// Checks the pack against the conversation archives `archive` gives by letter (the bytes
    /// of `converse.<letter>` the engine reads): the fingerprint of the scripts it names, then
    /// each run inside its script, of text bytes only, with its hash. Any difference is a
    /// mismatch of the conversation part.
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
        Ok(())
    }
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

/// The locale layer's state: the installed pack, the Hangul table the platform gave with it,
/// and what the runner counted (counts only: never a key or a text).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocaleState {
    pub pack: Option<Rc<Pack>>,
    pub hangul: Option<Rc<HangulFont>>,
    /// Runs put from the pack.
    pub hits: u32,
    /// Runs started with the pack installed that it does not have (or whose bytes changed):
    /// printed in English.
    pub misses: u32,
}

/// Installs `pack` on `state` once it checks against the conversation archives of `files`, as
/// the script loader names them ([`crate::converse::archive_name`]; [`Pack::check`]): the text
/// layer takes UTF-8 and draws Hangul from `hangul` ([`apply`]). A mismatch changes nothing.
pub fn install(
    state: &mut State,
    files: &Files,
    pack: Rc<Pack>,
    hangul: Option<Rc<HangulFont>>,
) -> Result<(), Mismatch> {
    pack.check(|letter| files.get(&crate::converse::archive_name(state, letter)))?;
    state.locale = LocaleState {
        pack: Some(pack),
        hangul,
        hits: 0,
        misses: 0,
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

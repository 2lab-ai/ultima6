//! Port-only (spec 08 rev 3 Design 2 and 6, spec 09 rev 3 Design 5): the catalog of a
//! conversation archive's translation units, built on the disassembler.
//!
//! - **Runs:** each text run, keyed (archive, item, run offset).
//! - **Table strings:** each string of a string table, keyed (archive, item, string offset);
//!   one an `a6` copies into a string slot carries the slots and their capacity.
//! - Also each script's name (bytes 2 up to `f1`) and its keywords, for the keyword aliases and
//!   the bundles' topics.
//! - **Hash:** [`hash64`], FNV-1a 64 (offset basis 0xcbf29ce484222325, prime 0x100000001b3)
//!   over a unit's script bytes, escapes included; the HD pack's content hash uses the same
//!   function (`u6_hdpack::image_hash`). Hashes are written as 16 lower-case hex digits.
//! - **Form:** JSON, [`FORMAT`] `u6-converse-catalog/1` ([`Catalog::to_json`] /
//!   [`Catalog::from_json`]). Unit text is written one character per script byte (U+0000..U+00FF),
//!   so it reads back to the same bytes.
//!
//! A catalog built from the owner's files holds game text: it stays outside the repository.

use serde::{Deserialize, Serialize};

use super::{Assign, Disasm, Kind, Operands, Refusal, Span, Table, disassemble};

/// The catalog format's name.
pub const FORMAT: &str = "u6-converse-catalog/1";
/// The hash every unit carries.
pub const HASH: &str = "fnv1a64";
/// The text a string slot 0..9 holds: its 100-byte buffer (A4+0x140b, `STRING_BUFFERS`) less
/// the NUL.
pub const SLOT_BUFFER_CAPACITY: u32 = 99;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 of `bytes`.
pub fn hash64(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(FNV_OFFSET, |h, &b| (h ^ u64::from(b)).wrapping_mul(FNV_PRIME))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub format: String,
    pub hash: String,
    pub scripts: Vec<ScriptUnits>,
}

/// One script's units, or why it was refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptUnits {
    /// `a` or `b` (`converse.a` / `converse.b`).
    pub archive: char,
    pub item: u16,
    pub len: u32,
    /// [`hash64`] of the whole script.
    #[serde(with = "hex64")]
    pub hash: u64,
    /// The disassembler's refusal; a refused script lists no units.
    pub refused: Option<String>,
    pub name: Option<Unit>,
    pub runs: Vec<RunUnit>,
    pub strings: Vec<StringUnit>,
    pub keywords: Vec<Unit>,
}

/// A byte range of a script with its hash and text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub offset: u32,
    pub len: u32,
    #[serde(with = "hex64")]
    pub hash: u64,
    pub text: String,
}

/// A text run and its escapes, in order (each as its script bytes).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunUnit {
    #[serde(flatten)]
    pub unit: Unit,
    pub placeholders: Vec<String>,
}

/// A table string: its table and entry index, and the string slots an `a6` copies it into.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringUnit {
    #[serde(flatten)]
    pub unit: Unit,
    /// The table's script address and the string's index in it.
    pub table: u32,
    pub index: u32,
    pub slots: Vec<u8>,
    /// The longest text every one of `slots` holds; `None` when a slot's buffer is not known
    /// statically (slots from 10 point where the game sets them) or there is no slot.
    pub capacity: Option<u32>,
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

fn unit(script: &[u8], s: Span) -> Unit {
    let b = s.bytes(script);
    Unit {
        offset: s.offset,
        len: s.len,
        hash: hash64(b),
        text: latin1(b),
    }
}

/// The capacity of a string slot an `a6` writes, when it is known statically.
fn slot_capacity(slot: u8) -> Option<u32> {
    (usize::from(slot) < super::super::STRING_BUFFERS).then_some(SLOT_BUFFER_CAPACITY)
}

impl ScriptUnits {
    /// The units of a decoded script (`script` = the bytes it was decoded from).
    pub fn new(d: &Disasm, script: &[u8]) -> Self {
        let mut out = Self::empty(d.archive, d.item, script, None);
        // String slots each table string is copied into: (table address, index or any).
        let mut copies: Vec<(u32, Option<u32>, u8)> = Vec::new();
        for item in &d.items {
            if let Kind::Op(op) = &item.kind
                && let Operands::Assign(Assign::StrFromTable { var, table, .. }) = &op.operands
            {
                let index = table.index.as_ref().map_or(Some(0), |e| e.constant());
                copies.push((table.addr, index, *var));
            }
        }
        let tables: Vec<(u32, &Table)> = d
            .items
            .iter()
            .filter_map(|it| match &it.kind {
                Kind::Table(t) if !t.words => Some((it.offset, t)),
                _ => None,
            })
            .collect();
        for item in &d.items {
            match &item.kind {
                Kind::Name => out.name = Some(unit(script, item.span())),
                Kind::Text(run) => out.runs.push(RunUnit {
                    unit: unit(script, item.span()),
                    placeholders: run
                        .placeholders()
                        .map(|p| {
                            latin1(
                                Span {
                                    offset: p.offset,
                                    len: p.len,
                                }
                                .bytes(script),
                            )
                        })
                        .collect(),
                }),
                Kind::Keywords(spans) => {
                    out.keywords
                        .extend(spans.iter().map(|&s| unit(script, s)));
                }
                _ => {}
            }
        }
        for (addr, t) in &tables {
            for (index, &e) in t.entries.iter().enumerate() {
                let index = index as u32;
                let mut slots: Vec<u8> = copies
                    .iter()
                    .filter(|&&(a, i, _)| a == *addr && i.is_none_or(|i| i == index))
                    .map(|&(_, _, s)| s)
                    .collect();
                slots.sort_unstable();
                slots.dedup();
                let capacity = if slots.is_empty() {
                    None
                } else {
                    slots
                        .iter()
                        .map(|&s| slot_capacity(s))
                        .try_fold(u32::MAX, |m, c| c.map(|c| m.min(c)))
                };
                out.strings.push(StringUnit {
                    unit: unit(script, e),
                    table: *addr,
                    index,
                    slots,
                    capacity,
                });
            }
        }
        out
    }

    /// A script the disassembler refused: no units, the reason.
    pub fn refused(archive: u8, item: u16, script: &[u8], why: &Refusal) -> Self {
        Self::empty(archive, item, script, Some(why.to_string()))
    }

    fn empty(archive: u8, item: u16, script: &[u8], refused: Option<String>) -> Self {
        Self {
            archive: char::from(archive),
            item,
            len: script.len() as u32,
            hash: hash64(script),
            refused,
            name: None,
            runs: Vec::new(),
            strings: Vec::new(),
            keywords: Vec::new(),
        }
    }
}

impl Catalog {
    /// Disassembles each (archive letter, item, script bytes) and lists its units.
    pub fn build<'a>(scripts: impl IntoIterator<Item = (u8, u16, &'a [u8])>) -> Self {
        let scripts = scripts
            .into_iter()
            .map(|(archive, item, bytes)| match disassemble(archive, item, bytes) {
                Ok(d) => ScriptUnits::new(&d, bytes),
                Err(why) => ScriptUnits::refused(archive, item, bytes, &why),
            })
            .collect();
        Self {
            format: FORMAT.into(),
            hash: HASH.into(),
            scripts,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("the catalog has only serialisable fields")
    }

    /// Reads a catalog back; another format's name is an error.
    pub fn from_json(s: &str) -> Result<Self, String> {
        let c: Catalog = serde_json::from_str(s).map_err(|e| e.to_string())?;
        if c.format != FORMAT || c.hash != HASH {
            return Err(format!("not {FORMAT} with {HASH}"));
        }
        Ok(c)
    }
}

/// `u64` as 16 lower-case hex digits (a JSON number would lose bits in JavaScript).
mod hex64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{v:016x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() != 16 {
            return Err(serde::de::Error::custom("a hash is 16 hex digits"));
        }
        u64::from_str_radix(&s, 16).map_err(serde::de::Error::custom)
    }
}

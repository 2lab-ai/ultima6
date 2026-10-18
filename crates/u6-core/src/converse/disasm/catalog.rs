//! Port-only (spec 08 rev 3 Design 2 and 6, spec 09 rev 3 Design 5): the catalog of a
//! conversation archive's translation units, built on the disassembler.
//!
//! - **Runs:** each text run, keyed (archive, item, run offset). A run a path entered in the
//!   middle was split there ([`super::Disasm::run_splits`]); its parts carry the offset of the
//!   first one ([`RunUnit::part_of`]), so a translator sees one sentence.
//! - **Table strings:** each string of a string table, keyed (archive, item, string offset);
//!   one an `a6` copies into a string slot carries the slots and their capacity. A copy's index
//!   is resolved the way the interpreter reads it, on into the tables that follow its table
//!   without a gap; a computed index marks every string it can reach. One an op reads from
//!   after its start lists those addresses ([`StringUnit::inner`]): such a read prints the
//!   string's rest.
//! - Also each script's name (bytes 2 up to `f1`) and its keywords, for the keyword aliases and
//!   the bundles' topics.
//! - **Hash:** [`hash64`], FNV-1a 64 (offset basis 0xcbf29ce484222325, prime 0x100000001b3)
//!   over a unit's script bytes, escapes included; the HD pack's content hash uses the same
//!   function (`u6_hdpack::image_hash`). Hashes are written as 16 lower-case hex digits.
//! - **Form:** JSON, [`FORMAT`] `u6-converse-catalog/2` ([`Catalog::to_json`] /
//!   [`Catalog::from_json`]). Unit text is written one character per script byte (U+0000..U+00FF),
//!   so it reads back to the same bytes; reading back checks each unit's length, hash and
//!   place in its script, the runs' `part_of` groups and that each string's inner addresses
//!   are inside it.
//!
//! A catalog built from the owner's files holds game text: it stays outside the repository.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::decode::ref_entry;
use super::{Assign, Disasm, Kind, Operands, Refusal, Span, disassemble, reachable};

/// The catalog format's name (`/3`: strings carry the addresses read inside them; `/2`: runs
/// carry `part_of`, copies reach the tables that follow).
pub const FORMAT: &str = "u6-converse-catalog/3";
/// The hash every unit carries.
pub const HASH: &str = "fnv1a64";
/// The text a string slot 0..9 holds: its 100-byte buffer (A4+0x140b, `STRING_BUFFERS`) less
/// the NUL.
pub const SLOT_BUFFER_CAPACITY: u32 = 99;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 of `bytes`.
pub fn hash64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(FNV_PRIME)
    })
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
    /// For a part of a run a path entered in the middle (split there): the offset of the first
    /// part. The parts with one `part_of` are one run of the script, in offset order, read
    /// whole from the first part; a path that enters at a later part reads from there.
    pub part_of: Option<u32>,
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
    /// The script addresses after the string's start, up to its NUL, that an op reads from
    /// ([`super::Table::inner`]), in order: such a read prints the string's rest, so a
    /// translation that changes the bytes before one changes what that read prints.
    pub inner: Vec<u32>,
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

impl Unit {
    /// The unit as [`ScriptUnits::build`] writes it: inside a script of `len` bytes, one
    /// character per byte, its hash that of those bytes.
    fn check(&self, len: u32) -> Result<(), String> {
        let at = self.offset;
        let bytes: Option<Vec<u8>> = self
            .text
            .chars()
            .map(|c| u8::try_from(u32::from(c)).ok())
            .collect();
        let Some(bytes) = bytes else {
            return Err(format!("unit at {at:#x}: a character above U+00FF"));
        };
        if bytes.len() != self.len as usize {
            return Err(format!("unit at {at:#x}: text and length differ"));
        }
        if self.offset.checked_add(self.len).is_none_or(|e| e > len) {
            return Err(format!("unit at {at:#x}: outside the script"));
        }
        if hash64(&bytes) != self.hash {
            return Err(format!("unit at {at:#x}: hash differs"));
        }
        Ok(())
    }
}

impl ScriptUnits {
    /// Disassembles one script (archive letter, item, bytes) and lists its units, or its
    /// refusal.
    pub fn build(archive: u8, item: u16, bytes: &[u8]) -> Self {
        match disassemble(archive, item, bytes) {
            Ok(d) => Self::units(&d, bytes),
            Err(why) => Self::refused(archive, item, bytes, &why),
        }
    }

    /// The units of `d`, decoded from `script` (the spans index it: the same bytes only).
    pub(super) fn units(d: &Disasm, script: &[u8]) -> Self {
        let mut out = Self::empty(d.archive, d.item, script, None);
        let item_at = |a: u32| {
            let i = d.items.partition_point(|it| it.offset <= a);
            d.items.get(i.checked_sub(1)?).filter(|it| a < it.end())
        };
        // The string slots each table string is copied into, by (table, index).
        let mut slots: BTreeMap<(u32, u32), BTreeSet<u8>> = BTreeMap::new();
        for item in &d.items {
            if let Kind::Op(op) = &item.kind
                && let Operands::Assign(Assign::StrFromTable { var, table, .. }) = &op.operands
            {
                let reach = reachable(item_at, table.addr, false);
                let picked = match ref_entry(table, false) {
                    Some(i) => reach.get(i as usize..=i as usize).unwrap_or_default(),
                    None => &reach[..],
                };
                for &(t, i, _) in picked {
                    slots.entry((t, i)).or_default().insert(*var);
                }
            }
        }
        let splits: BTreeSet<u32> = d.run_splits.iter().copied().collect();
        for item in &d.items {
            match &item.kind {
                Kind::Name => out.name = Some(unit(script, item.span())),
                Kind::Text(run) => {
                    let part_of = match out.runs.last_mut() {
                        Some(prev)
                            if splits.contains(&item.offset)
                                && prev.unit.offset + prev.unit.len == item.offset =>
                        {
                            Some(*prev.part_of.get_or_insert(prev.unit.offset))
                        }
                        _ => None,
                    };
                    out.runs.push(RunUnit {
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
                        part_of,
                    });
                }
                Kind::Keywords(spans) => {
                    out.keywords.extend(spans.iter().map(|&s| unit(script, s)));
                }
                _ => {}
            }
        }
        let tables = d.items.iter().filter_map(|it| match &it.kind {
            Kind::Table(t) if !t.words => Some((it.offset, t)),
            _ => None,
        });
        for (addr, t) in tables {
            for (index, &e) in t.entries.iter().enumerate() {
                let index = index as u32;
                let slots: Vec<u8> = slots
                    .get(&(addr, index))
                    .map(|s| s.iter().copied().collect())
                    .unwrap_or_default();
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
                    table: addr,
                    index,
                    slots,
                    capacity,
                    inner: t
                        .inner
                        .iter()
                        .copied()
                        .filter(|&a| e.offset < a && a <= e.end())
                        .collect(),
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

    /// Each unit as [`Unit::check`] has it; a refused script lists none.
    fn check(&self) -> Result<(), String> {
        let mut units = self
            .name
            .iter()
            .chain(self.runs.iter().map(|r| &r.unit))
            .chain(self.strings.iter().map(|s| &s.unit))
            .chain(&self.keywords)
            .peekable();
        let at = format!("script {}{}", self.archive, self.item);
        if self.refused.is_some() && units.peek().is_some() {
            return Err(format!("{at}: refused, with units"));
        }
        for u in units {
            u.check(self.len).map_err(|e| format!("{at}: {e}"))?;
        }
        for s in &self.strings {
            let (start, end) = (s.unit.offset, s.unit.offset + s.unit.len);
            let inside = s.inner.iter().all(|&a| start < a && a <= end);
            if !inside || !s.inner.is_sorted_by(|a, b| a < b) {
                return Err(format!(
                    "{at}: string at {start:#x}: an inner address outside it"
                ));
            }
        }
        check_parts(&self.runs).map_err(|e| format!("{at}: {e}"))
    }
}

/// The runs' `part_of` links as [`ScriptUnits::build`] writes them: runs in offset order, each
/// split run a group of two or more runs that follow each other without a gap, the first
/// linking to itself and every other one to the first; a run outside a group links to
/// nothing.
fn check_parts(runs: &[RunUnit]) -> Result<(), String> {
    for (i, r) in runs.iter().enumerate() {
        let at = r.unit.offset;
        let prev = i.checked_sub(1).map(|j| &runs[j]);
        let next = runs.get(i + 1);
        let follows = |a: &RunUnit, b: &RunUnit| a.unit.offset + a.unit.len == b.unit.offset;
        if prev.is_some_and(|p| p.unit.offset + p.unit.len > at) {
            return Err(format!("run at {at:#x}: runs out of order"));
        }
        let Some(root) = r.part_of else { continue };
        let ok = if root == at {
            // The first part: it starts a group, and the next run carries it on.
            prev.is_none_or(|p| p.part_of != Some(root))
                && next.is_some_and(|n| n.part_of == Some(root) && follows(r, n))
        } else {
            // A later part: the run before it is of the same group, right before it.
            prev.is_some_and(|p| p.part_of == Some(root) && follows(p, r))
        };
        if !ok {
            return Err(format!("run at {at:#x}: a part_of link outside its group"));
        }
    }
    Ok(())
}

impl Catalog {
    /// Disassembles each (archive letter, item, script bytes) and lists its units.
    pub fn build<'a>(scripts: impl IntoIterator<Item = (u8, u16, &'a [u8])>) -> Self {
        let scripts = scripts
            .into_iter()
            .map(|(archive, item, bytes)| ScriptUnits::build(archive, item, bytes))
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

    /// Reads a catalog back; another format's name, or a unit whose text, length, hash or
    /// place disagree, is an error.
    pub fn from_json(s: &str) -> Result<Self, String> {
        let c: Catalog = serde_json::from_str(s).map_err(|e| e.to_string())?;
        if c.format != FORMAT || c.hash != HASH {
            return Err(format!("not {FORMAT} with {HASH}"));
        }
        for s in &c.scripts {
            s.check()?;
        }
        Ok(c)
    }
}

/// `u64` as 16 lower-case hex digits (a JSON number would lose bits in JavaScript); nothing
/// else reads back.
mod hex64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{v:016x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() != 16 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(serde::de::Error::custom(
                "a hash is 16 lower-case hex digits",
            ));
        }
        u64::from_str_radix(&s, 16).map_err(serde::de::Error::custom)
    }
}

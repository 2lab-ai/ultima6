//! Port-only (spec 09 rev 3 Design 5): the NPC bundles, `u6.npc-bundle/1`. One bundle per
//! conversation script, keyed by (archive, item), holds that script's name and dialogue apart
//! from the game, so that a later NpcBrain (spec 04 W7b) can imitate how each NPC speaks.
//!
//! The exporter ([`export`]) reads the game files the page kept (`converse.a`, `converse.b`, and
//! `savegame/objlist` for the actors' numbers when it is there), disassembles every script and
//! builds 08's catalog of it ([`super::catalog`]): a bundle's text is the catalog's, run by run,
//! each text segment carrying its run's key (archive, item, run offset), so a bundle and the
//! Korean pack line up. It runs in wasm in the visitor's browser; the page keeps the bundles in
//! its own storage and downloads them only on a button press.
//!
//! **The schema** (these types are it; the JSON names are the field names unless a field says
//! otherwise):
//!
//! - [`Bundle`]: `format` ([`FORMAT`]), `archive` and `item` (the key), `hash` (the script's
//!   FNV-1a 64, as the catalog has it), `refused` (the disassembler's reason, or `null`), the
//!   `actors` that use the script, their `portraits`, the `name` and `locale`,
//!   `narration.look`, `speech.greeting`, the `turns`, the actors' `traits` (numbers only) and
//!   the script's table `strings` (each with, since spec 08's slice K2 and only when there is one,
//!   the `slots` an `a6` copies it into, their `capacity` and the `inner` addresses an op reads
//!   it from: optional fields, so a bundle without them reads as before).
//! - A [`Turn`] is an input op (`input`, `null` for keyword blocks before any) with the segments
//!   that follow it outside a keyword block and its `topics`: each keyword block, with its
//!   `keywords`, what it `requires` and its `answer`.
//! - A [`Segment`] is one of four kinds (its `kind` field): `text` (one 08 run: its `key`,
//!   `text` and, for a part of a split run, `part_of`), `var` (op `b5`'s printing forms: a
//!   string slot, or a table string chosen at run time), `op` (any other statement op, and the
//!   bytes that end a conversation) and `if` (op `a1` with its condition, `a3`, `a2`).
//! - An expression (an [`Rpn`]) is its tokens in the evaluator's order: numbers for literals
//!   and pushed values, names (`ops::EXPR_OPS`) for operators.
//!
//! Segments are in script order, each where the byte-order walk of the script's items finds it
//! (a jump is an `op` segment with its `target`, not followed), so every run of the catalog is
//! placed exactly once ([`coverage`]).
//!
//! A bundle copies some things more than once (each topic's conditions, each table operand's
//! keys), so a short script could ask for far more than its bytes: every script is built within
//! a budget of items ([`SCRIPT_BUDGET`], [`EXPORT_BUDGET`]) and one that would pass it is
//! refused ([`OVER_BUDGET`]) before the copy that passes it is made.
//!
//! Rights (spec 09): a bundle holds the user's game text. It is built only in the user's
//! browser (or a local test) from the user's files, never committed, logged or uploaded; the
//! errors here name only what failed, never a value of the files.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::catalog::{RunUnit, ScriptUnits, Unit, hash64, hex64};
use super::decode::ref_entry;
use super::ops;
use super::{
    Assign, Disasm, Expr, Input, Item, Kind, Marker, Operands, Print, Stop, TableRef, TokenKind,
    disassemble, reachable,
};
use crate::converse::{portrait_ref, script_item};
use crate::files::Files;
use crate::objects::ACTOR_SLOTS;
use crate::state::State;

/// The bundle format's name.
pub const FORMAT: &str = "u6.npc-bundle/1";
/// The locale of a bundle built from the game files (the original's English).
pub const LOCALE: &str = "en";
/// The archives the exporter reads (`converse.<letter>`).
pub const ARCHIVES: [u8; 2] = [b'a', b'b'];
/// The files the exporter reads, by bare name: the two archives and the objlist.
pub const READS: [&str; 3] = ["converse.a", "converse.b", "objlist"];

/// The most items one script's bundle may take ([`Bundle::build_within`]): each segment, turn
/// and topic, each condition (and its tokens) copied into a topic's `requires`, and each table
/// string key a table operand lists. What a script holds once is bounded by its length; these
/// copies are not (conditions × topics, references × strings), so a script that would pass the
/// budget is refused with [`OVER_BUDGET`] before the copy that passes it is made. Far above
/// what the owner's scripts spend (the real-data receipt prints it), and small enough that a
/// refused script never holds more than a few MiB.
pub const SCRIPT_BUDGET: usize = 1 << 14;
/// The most items one [`export`] spends over all its scripts, refused ones included (each up to
/// the budget it was given): a script gets the smaller of [`SCRIPT_BUDGET`] and what is left,
/// so once the export's budget is spent every script after is refused, and the export goes on.
/// It bounds what an export holds and writes whatever the archives hold (some tens of MiB).
pub const EXPORT_BUDGET: usize = 1 << 18;
/// The refusal of a bundle over its budget (a fixed text: no value of the script).
pub const OVER_BUDGET: &str = "over the bundle budget";

/// Whether [`export`] reads the file at `path` (by its bare name, ignoring case).
pub fn reads(path: &str) -> bool {
    let bare = path.rsplit(['/', '\\', ':']).next().unwrap_or(path);
    READS.iter().any(|r| bare.eq_ignore_ascii_case(r))
}

/// A unit's key: (archive letter, item, offset in the script).
pub type Key = (char, u16, u32);

/// An expression's tokens in the evaluator's order.
pub type Rpn = Vec<Tok>;

/// One token of an [`Rpn`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Tok {
    /// A literal or a pushed byte.
    Num(u32),
    /// An operator, by its name in `ops::EXPR_OPS`.
    Op(String),
}

/// One conversation script's bundle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    /// [`FORMAT`].
    pub format: String,
    /// `a` or `b` (`converse.a` / `converse.b`).
    pub archive: char,
    pub item: u16,
    /// FNV-1a 64 of the whole script (the catalog's `hash`), 16 lower-case hex digits.
    #[serde(with = "hex64")]
    pub hash: u64,
    /// The disassembler's refusal, or [`OVER_BUDGET`]: a refused script has no name, segments,
    /// turns or strings.
    pub refused: Option<String>,
    /// The actor numbers whose conversation loads this script (the engine's own mapping, slots 1
    /// to 255), in order.
    pub actors: Vec<u16>,
    /// The portrait each of those actors shows (`portrait.<archive>` item), where it has one.
    pub portraits: Vec<Portrait>,
    /// The script's name (bytes 2 up to the first `f1`), as the catalog has it.
    pub name: Option<String>,
    /// [`LOCALE`].
    pub locale: String,
    pub narration: Narration,
    pub speech: Speech,
    pub turns: Vec<Turn>,
    /// Numbers about each actor (object type, worktype, alignment), as the speech tap gives
    /// them; empty when the files hold no objlist.
    pub traits: Vec<Traits>,
    /// The script's table strings, which `var` segments and `op` segments name by key.
    pub strings: Vec<TableString>,
}

/// The portrait an actor shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Portrait {
    pub actor: u16,
    /// `portrait.<archive>` (`a`, `b`, or `z` for the hero).
    pub archive: char,
    pub item: u16,
}

/// Numbers about an actor (spec 09 Design 1's traits).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Traits {
    pub actor: u16,
    /// Object type (`type` in JSON).
    #[serde(rename = "type")]
    pub obj_type: u16,
    pub worktype: u8,
    /// Status bits 5..6.
    pub alignment: u8,
}

/// The look text (after `f1`): narration, not speech.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Narration {
    pub look: Vec<Segment>,
}

/// What the NPC says before the first input op (after the `f2`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Speech {
    pub greeting: Vec<Segment>,
}

/// An input op and what follows it up to the next one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    /// The input op; `None` (`null`) for keyword blocks that come before any.
    pub input: Option<InputOp>,
    /// What follows the input op outside a keyword block, in order.
    pub segments: Vec<Segment>,
    pub topics: Vec<Topic>,
}

/// An input op of the input loop.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputOp {
    /// Its offset in the script.
    pub at: u32,
    pub kind: InputKind,
    /// `keys`: the keys allowed, as the script holds them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<String>,
    /// `line`: the string slot the line goes into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u8>,
    /// `number` / `digit`: the word variable the number goes into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var: Option<u8>,
}

/// The input ops (`f7` .. `fc`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    /// `f7`: the player's words, matched against the keyword blocks.
    Ask,
    /// `f8`: one of the allowed keys.
    Keys,
    /// `f9`: a line into a string slot.
    Line,
    /// `fa`: one key.
    Key,
    /// `fb`: a number into a word variable.
    Number,
    /// `fc`: one digit into a word variable.
    Digit,
}

/// A keyword block: the words that select it, what it requires and what the NPC answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Topic {
    /// The offset of its `ef`.
    pub at: u32,
    /// The keywords, as the script holds them (a `*` keyword matches any input).
    pub keywords: Vec<String>,
    /// The conditions of the `if` ops before it in script order, read as the interpreter's skip
    /// loops read them: a false `a1` goes on past the first `a3` / `a2`, and an `a3` reached
    /// goes on past the first `a2`. So an `a2` ends every condition before it, and the segment
    /// after an `a3` needs the conditions since the last `a3` (or `a2`) not all to hold. A jump
    /// into the middle is not followed: this is the order of the bytes.
    pub requires: Vec<Clause>,
    /// From the `f6` up to its `ee` (or the next keyword block or input op).
    pub answer: Vec<Segment>,
}

/// One condition a topic requires.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Clause {
    /// `{"if": rpn}`: the condition holds.
    If(Rpn),
    /// `{"unless": [rpn, ...]}`: these do not all hold (the branch after an `a3`).
    Unless(Vec<Rpn>),
}

/// One piece of a script's flow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Segment {
    Text(TextSegment),
    Var(VarSegment),
    Op(OpSegment),
    If(IfSegment),
}

/// One 08 run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSegment {
    /// (archive, item, run offset): the catalog's key of the run.
    pub key: Key,
    /// The run's bytes, one character per byte, its escapes (`$n`, `#n`, `@`, `*`, ...) kept.
    pub text: String,
    /// For a part of a run a path enters in the middle: the offset of the first part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_of: Option<u32>,
}

/// Op `b5`'s printing forms: a value the NPC says that the script chooses at run time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarSegment {
    pub at: u32,
    pub from: VarFrom,
    /// `slot`: the string slot printed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u8>,
    /// `table`: the table and the strings the index can pick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<TableArg>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VarFrom {
    /// `b5 <kind> <slot> b3`: a string slot, through printf.
    Slot,
    /// `b5 d2 <table> <index>`: a table string.
    Table,
}

/// A table operand: its address, its index and the table strings it can read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableArg {
    pub addr: u32,
    /// The index expression; `None` (absent) for `b1`, the first entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<Rpn>,
    /// The keys of the strings it can read (one for a constant index, every reachable one for a
    /// computed index; none for a word table), as [`Bundle::strings`] lists them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strings: Vec<Key>,
}

/// A statement op other than `b5`'s printing forms and the `if` ops, or a byte that ends the
/// conversation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpSegment {
    pub at: u32,
    /// The op's name in `ops::STATEMENT_OPS` (`print_nothing` for `b5 d5`), or for a byte that
    /// ends the conversation `end` (`ff`), `nul` (0), `control` (a control character) or
    /// `error` (an input op byte the loop does not take).
    pub op: String,
    /// Its expression operands, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<Rpn>,
    /// `jump`: the new read position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<u32>,
    /// `assign`: the word variable or string slot written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var: Option<u8>,
    /// `assign` from a string slot: the slot read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<u8>,
    /// `assign` of a table word, or of a table string into a slot: the table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<TableArg>,
}

/// `a1` (with its condition), `a3` or `a2`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IfSegment {
    pub at: u32,
    pub form: IfForm,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cond: Option<Rpn>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IfForm {
    If,
    Else,
    Endif,
}

/// A table string of the script.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableString {
    /// (archive, item, string offset): the catalog's key of the string.
    pub key: Key,
    /// Its table's address and its index in that table.
    pub table: u32,
    pub index: u32,
    pub text: String,
    /// Spec 08 (slice K2), optional (absent when empty): the string slots an `a6` copies it
    /// into, as the catalog has them ([`super::catalog::StringUnit::slots`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<u8>,
    /// Optional (absent when `None`): the most bytes every one of those slots holds
    /// ([`super::catalog::StringUnit::capacity`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<u32>,
    /// Optional (absent when empty): the script addresses inside the string an op reads from
    /// ([`super::catalog::StringUnit::inner`]; such a read prints the string's rest).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inner: Vec<u32>,
}

/// An actor that uses a script, with what the bundle says of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    pub actor: u16,
    pub portrait: Option<(u8, u16)>,
    pub traits: Option<Traits>,
}

/// Every actor slot 1..=255 whose conversation loads a script, by that script's (archive,
/// item), as `state` maps them (`script_item`, `portrait_ref`); the traits when `with_traits`
/// (the state holds an objlist).
pub fn cast(state: &State, with_traits: bool) -> BTreeMap<(u8, u16), Vec<Member>> {
    let mut out: BTreeMap<(u8, u16), Vec<Member>> = BTreeMap::new();
    for actor in 1..ACTOR_SLOTS as i16 {
        let Some(key) = script_item(state, actor) else {
            continue;
        };
        out.entry(key).or_default().push(Member {
            actor: actor as u16,
            portrait: portrait_ref(state, actor),
            traits: with_traits.then(|| {
                let t = crate::speech::traits(state, actor);
                Traits {
                    actor: actor as u16,
                    obj_type: t.obj_type,
                    worktype: t.worktype,
                    alignment: t.alignment,
                }
            }),
        });
    }
    out
}

/// Why [`export`] made no bundles. Its text names only what failed (never a value of the files).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportError {
    /// The files hold neither `converse.a` nor `converse.b`.
    NoArchive,
    /// An archive (its letter) does not read as a converse library.
    Unreadable(u8),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::NoArchive => write!(f, "the files hold no conversation archive"),
            ExportError::Unreadable(l) => {
                write!(f, "converse.{} does not read", char::from(*l))
            }
        }
    }
}

/// The bundles of every script of `files`' `converse.a` and `converse.b`, in (archive, item)
/// order. The actors' numbers come from `savegame/objlist` when the files hold one the game
/// reads, else only from the actor slots' own mapping (no traits, no slots from 0xe0). Each
/// script is built within its share of [`EXPORT_BUDGET`]; one over it is refused with
/// [`OVER_BUDGET`] and the export goes on.
pub fn export(files: &Files) -> Result<Vec<Bundle>, ExportError> {
    let mut state = State::empty();
    let objlist = crate::saveload::boot_objlist(files).is_ok_and(|o| {
        crate::saveload::objlist::decode(&mut state, o);
        true
    });
    let cast = cast(&state, objlist);
    let mut out = Vec::new();
    let mut found = false;
    let mut left = EXPORT_BUDGET;
    for letter in ARCHIVES {
        let Some(file) = files.get(&format!("converse.{}", char::from(letter))) else {
            continue;
        };
        found = true;
        let lib = u6_formats::converse::ConverseLib::parse(file)
            .map_err(|_| ExportError::Unreadable(letter))?;
        for (item, s) in lib.scripts.iter().enumerate() {
            let (Some(s), Ok(item)) = (s, u16::try_from(item)) else {
                continue;
            };
            let members = cast.get(&(letter, item)).map_or(&[][..], Vec::as_slice);
            let budget = SCRIPT_BUDGET.min(left);
            let (b, spent) = Bundle::build_within(letter, item, &s.bytecode, members, budget);
            left -= spent;
            out.push(b);
        }
    }
    if !found {
        return Err(ExportError::NoArchive);
    }
    Ok(out)
}

/// The bundles as one JSON array.
pub fn to_json(bundles: &[Bundle]) -> String {
    serde_json::to_string(bundles).expect("a bundle has only serialisable fields")
}

/// Reads a JSON array of bundles back; another format's name is an error (one that names only
/// the bundle's place in the array).
pub fn from_json(s: &str) -> Result<Vec<Bundle>, String> {
    let b: Vec<Bundle> = serde_json::from_str(s).map_err(|e| {
        // serde's message can quote the input: its place only.
        format!(
            "not a bundle array (line {}, column {})",
            e.line(),
            e.column()
        )
    })?;
    if let Some(i) = b.iter().position(|b| b.format != FORMAT) {
        return Err(format!("bundle {i}: not {FORMAT}"));
    }
    Ok(b)
}

impl Bundle {
    /// The bundle of one script (archive letter, item, bytes) and the actors that use it, within
    /// [`SCRIPT_BUDGET`].
    pub fn build(archive: u8, item: u16, bytes: &[u8], members: &[Member]) -> Self {
        Self::build_within(archive, item, bytes, members, SCRIPT_BUDGET).0
    }

    /// [`Bundle::build`] within `budget` items ([`SCRIPT_BUDGET`] says what counts), and the
    /// items it spent (at most `budget`). A script that would spend more is refused with
    /// [`OVER_BUDGET`] at the first item past it, before that item is made: it then holds no
    /// name, segments, turns or strings (its actors stay).
    pub fn build_within(
        archive: u8,
        item: u16,
        bytes: &[u8],
        members: &[Member],
        budget: usize,
    ) -> (Self, usize) {
        let (units, disasm) = match disassemble(archive, item, bytes) {
            Ok(d) => (ScriptUnits::units(&d, bytes), Some(d)),
            Err(why) => (ScriptUnits::refused(archive, item, bytes, &why), None),
        };
        let mut b = Bundle {
            format: FORMAT.into(),
            archive: char::from(archive),
            item,
            hash: hash64(bytes),
            refused: units.refused.clone(),
            actors: members.iter().map(|m| m.actor).collect(),
            portraits: members
                .iter()
                .filter_map(|m| {
                    m.portrait.map(|(l, i)| Portrait {
                        actor: m.actor,
                        archive: char::from(l),
                        item: i,
                    })
                })
                .collect(),
            name: units.name.as_ref().map(|u| u.text.clone()),
            locale: LOCALE.into(),
            narration: Narration::default(),
            speech: Speech::default(),
            turns: Vec::new(),
            traits: members.iter().filter_map(|m| m.traits).collect(),
            strings: units
                .strings
                .iter()
                .map(|s| TableString {
                    key: (char::from(archive), item, s.unit.offset),
                    table: s.table,
                    index: s.index,
                    text: s.unit.text.clone(),
                    slots: s.slots.clone(),
                    capacity: s.capacity,
                    inner: s.inner.clone(),
                })
                .collect(),
        };
        let Some(d) = disasm else {
            return (b, 0);
        };
        let mut walk = Walk::new(&d, bytes, &units, budget);
        if walk.run(&mut b).is_ok() {
            return (b, walk.spent);
        }
        let refused = Bundle {
            refused: Some(OVER_BUDGET.into()),
            name: None,
            narration: Narration::default(),
            speech: Speech::default(),
            turns: Vec::new(),
            strings: Vec::new(),
            ..b
        };
        (refused, budget)
    }

    /// Every segment, in the bundle's order: the look, the greeting, then each turn's segments
    /// and its topics' answers.
    pub fn segments(&self) -> impl Iterator<Item = &Segment> {
        self.narration
            .look
            .iter()
            .chain(&self.speech.greeting)
            .chain(self.turns.iter().flat_map(|t| {
                t.segments
                    .iter()
                    .chain(t.topics.iter().flat_map(|p| &p.answer))
            }))
    }
}

/// How a bundle covers its script's catalog (spec 09 Acceptance 5's receipt).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    /// The catalog's runs, and the text segments.
    pub runs: usize,
    pub texts: usize,
    /// Runs no text segment names.
    pub missing: usize,
    /// Runs more than one text segment names (each extra one counted).
    pub repeated: usize,
    /// Text segments whose key names no run of this script, or whose text or `part_of` is not
    /// that run's.
    pub unresolved: usize,
    /// Table strings of the catalog the bundle does not list as the catalog has them.
    pub strings_missing: usize,
    /// The catalog's runs of a script the bundle refuses (its `refused` says why): not gaps.
    pub explained: usize,
}

impl Coverage {
    /// The gaps: every one of them is a run or a string the bundle does not hold as the
    /// catalog does.
    pub fn gaps(&self) -> usize {
        self.missing + self.repeated + self.unresolved + self.strings_missing
    }
}

/// How `b` covers `units` (the catalog of the same script).
pub fn coverage(b: &Bundle, units: &ScriptUnits) -> Coverage {
    let runs: BTreeMap<Key, &RunUnit> = units
        .runs
        .iter()
        .map(|r| ((units.archive, units.item, r.unit.offset), r))
        .collect();
    let mut named: BTreeMap<Key, usize> = BTreeMap::new();
    let mut c = Coverage {
        runs: runs.len(),
        ..Coverage::default()
    };
    if b.refused.is_some() {
        // A refused bundle holds no segment: its runs are explained by its reason.
        c.explained = c.runs;
        return c;
    }
    for s in b.segments() {
        let Segment::Text(t) = s else { continue };
        c.texts += 1;
        match runs.get(&t.key) {
            Some(r) if r.unit.text == t.text && r.part_of == t.part_of => {
                *named.entry(t.key).or_default() += 1;
            }
            _ => c.unresolved += 1,
        }
    }
    c.missing = runs.keys().filter(|k| !named.contains_key(k)).count();
    c.repeated = named.values().map(|n| n - 1).sum();
    c.strings_missing = units
        .strings
        .iter()
        .filter(|s| {
            !b.strings.iter().any(|t| {
                t.key == (units.archive, units.item, s.unit.offset)
                    && t.table == s.table
                    && t.index == s.index
                    && t.text == s.unit.text
            })
        })
        .count();
    c
}

/// Where the walk puts a segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Look,
    Greeting,
    /// The current turn's own segments.
    Turn,
    /// The current topic's answer.
    Answer,
}

/// The byte-order walk of one script's items into a bundle.
struct Walk<'a> {
    d: &'a Disasm,
    script: &'a [u8],
    archive: char,
    runs: BTreeMap<u32, &'a RunUnit>,
    keywords: BTreeMap<u32, &'a Unit>,
    /// The catalog offset of each table string, by (table, index).
    strings: BTreeMap<(u32, u32), u32>,
    place: Place,
    /// The conditions fixed by an `a3` since the last `a2`, and those since the last `a3`.
    prior: Vec<Clause>,
    group: Vec<Rpn>,
    /// What a copy of `prior` and `group` takes: one per condition and one per token.
    conditions: usize,
    /// The items this bundle may take, and those it took ([`SCRIPT_BUDGET`]).
    budget: usize,
    spent: usize,
}

/// The walk would pass its budget.
struct OverBudget;

fn rpn(e: &Expr) -> Rpn {
    e.tokens
        .iter()
        .map(|t| match t.kind {
            TokenKind::Op(c) => Tok::Op(ops::expr_op(c).map_or("?", |o| o.name).into()),
            k => Tok::Num(k.value().unwrap_or(0)),
        })
        .collect()
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

impl<'a> Walk<'a> {
    fn new(d: &'a Disasm, script: &'a [u8], units: &'a ScriptUnits, budget: usize) -> Self {
        Self {
            d,
            script,
            archive: units.archive,
            runs: units.runs.iter().map(|r| (r.unit.offset, r)).collect(),
            keywords: units.keywords.iter().map(|u| (u.offset, u)).collect(),
            strings: units
                .strings
                .iter()
                .map(|s| ((s.table, s.index), s.unit.offset))
                .collect(),
            place: Place::Look,
            prior: Vec::new(),
            group: Vec::new(),
            conditions: 0,
            budget,
            spent: 0,
        }
    }

    /// Takes `n` items of the budget, before they are made.
    fn spend(&mut self, n: usize) -> Result<(), OverBudget> {
        let spent = self.spent.saturating_add(n);
        if spent > self.budget {
            return Err(OverBudget);
        }
        self.spent = spent;
        Ok(())
    }

    fn key(&self, offset: u32) -> Key {
        (self.archive, self.d.item, offset)
    }

    fn item_at(&self, a: u32) -> Option<&'a Item> {
        let items = &self.d.items;
        let i = items.partition_point(|it| it.offset <= a);
        items.get(i.checked_sub(1)?).filter(|it| a < it.end())
    }

    /// A table operand of a string table (`words` false) or a word table, with the strings it
    /// can read as the catalog resolves an `a6` copy's (`catalog.rs`).
    fn table(&mut self, t: &TableRef, words: bool) -> Result<TableArg, OverBudget> {
        let mut strings = Vec::new();
        if !words {
            let reach = reachable(|a| self.item_at(a), t.addr, false);
            let picked = match ref_entry(t, false) {
                Some(i) => reach.get(i as usize..=i as usize).unwrap_or_default(),
                None => &reach[..],
            };
            self.spend(picked.len())?;
            for &(table, index, _) in picked {
                if let Some(&at) = self.strings.get(&(table, index)) {
                    strings.push(self.key(at));
                }
            }
        }
        Ok(TableArg {
            addr: t.addr,
            index: t.index.as_ref().map(rpn),
            strings,
        })
    }

    fn push(&mut self, b: &mut Bundle, s: Segment) {
        let to = match self.place {
            Place::Look => &mut b.narration.look,
            Place::Greeting => &mut b.speech.greeting,
            Place::Turn => match b.turns.last_mut() {
                Some(t) => &mut t.segments,
                None => &mut b.speech.greeting,
            },
            Place::Answer => match b.turns.last_mut().and_then(|t| t.topics.last_mut()) {
                Some(p) => &mut p.answer,
                None => &mut b.speech.greeting,
            },
        };
        to.push(s);
    }

    fn op(at: u32, op: &str) -> OpSegment {
        OpSegment {
            at,
            op: op.into(),
            args: Vec::new(),
            target: None,
            var: None,
            src: None,
            table: None,
        }
    }

    fn run(&mut self, b: &mut Bundle) -> Result<(), OverBudget> {
        let d = self.d;
        for it in &d.items {
            let at = it.offset;
            let seg = match &it.kind {
                Kind::Header | Kind::Name | Kind::Table(_) | Kind::Unreached => None,
                Kind::Marker(Marker::Look) => {
                    self.place = Place::Look;
                    None
                }
                Kind::Marker(Marker::Greeting) => {
                    self.place = Place::Greeting;
                    None
                }
                Kind::Text(_) => self.runs.get(&at).map(|r| {
                    Segment::Text(TextSegment {
                        key: self.key(r.unit.offset),
                        text: r.unit.text.clone(),
                        part_of: r.part_of,
                    })
                }),
                Kind::Op(op) => Some(self.op_segment(at, op)?),
                Kind::Keywords(spans) => {
                    self.spend(1 + self.conditions)?;
                    let keywords = spans
                        .iter()
                        .map(|s| {
                            self.keywords
                                .get(&s.offset)
                                .map_or_else(String::new, |u| u.text.clone())
                        })
                        .collect();
                    let mut requires = self.prior.clone();
                    requires.extend(self.group.iter().cloned().map(Clause::If));
                    if b.turns.is_empty() {
                        b.turns.push(Turn {
                            input: None,
                            segments: Vec::new(),
                            topics: Vec::new(),
                        });
                    }
                    let turn = b.turns.last_mut().expect("a turn");
                    turn.topics.push(Topic {
                        at,
                        keywords,
                        requires,
                        answer: Vec::new(),
                    });
                    self.place = Place::Answer;
                    None
                }
                Kind::BlockEnd => {
                    if self.place == Place::Answer {
                        self.place = Place::Turn;
                    }
                    None
                }
                Kind::Input(input) => {
                    self.spend(1)?;
                    b.turns.push(Turn {
                        input: Some(self.input(at, input)),
                        segments: Vec::new(),
                        topics: Vec::new(),
                    });
                    self.place = Place::Turn;
                    None
                }
                Kind::Stop(stop) => Some(Segment::Op(Self::op(
                    at,
                    match stop {
                        Stop::End => "end",
                        Stop::Nul => "nul",
                        Stop::Control(_) => "control",
                        Stop::Error(_) => "error",
                    },
                ))),
            };
            if let Some(s) = seg {
                self.spend(1)?;
                self.push(b, s);
            }
        }
        Ok(())
    }

    fn input(&self, at: u32, input: &Input) -> InputOp {
        let mut op = InputOp {
            at,
            kind: InputKind::Ask,
            keys: None,
            slot: None,
            var: None,
        };
        match *input {
            Input::Ask => {}
            Input::Keys(span) => {
                op.kind = InputKind::Keys;
                op.keys = Some(latin1(span.bytes(self.script)));
            }
            Input::Line { slot } => {
                op.kind = InputKind::Line;
                op.slot = Some(slot);
            }
            Input::Key => op.kind = InputKind::Key,
            Input::Number { var } => {
                op.kind = InputKind::Number;
                op.var = Some(var);
            }
            Input::Digit { var } => {
                op.kind = InputKind::Digit;
                op.var = Some(var);
            }
        }
        op
    }

    fn op_segment(&mut self, at: u32, op: &super::Op) -> Result<Segment, OverBudget> {
        let name = op.info().name;
        Ok(match (&op.operands, op.code) {
            (Operands::Args(a), 0xa1) => {
                let cond = a.first().map(|o| rpn(&o.expr)).unwrap_or_default();
                self.conditions = self.conditions.saturating_add(1 + cond.len());
                self.group.push(cond.clone());
                Segment::If(IfSegment {
                    at,
                    form: IfForm::If,
                    cond: Some(cond),
                })
            }
            (_, 0xa3) => {
                if !self.group.is_empty() {
                    self.prior
                        .push(Clause::Unless(std::mem::take(&mut self.group)));
                }
                Segment::If(IfSegment {
                    at,
                    form: IfForm::Else,
                    cond: None,
                })
            }
            (_, 0xa2) => {
                self.prior.clear();
                self.group.clear();
                self.conditions = 0;
                Segment::If(IfSegment {
                    at,
                    form: IfForm::Endif,
                    cond: None,
                })
            }
            (Operands::Print(Print::Table(t)), _) => Segment::Var(VarSegment {
                at,
                from: VarFrom::Table,
                slot: None,
                table: Some(self.table(t, false)?),
            }),
            (Operands::Print(Print::Slot { slot, .. }), _) => Segment::Var(VarSegment {
                at,
                from: VarFrom::Slot,
                slot: Some(*slot),
                table: None,
            }),
            (Operands::Print(Print::Nothing), _) => Segment::Op(Self::op(at, "print_nothing")),
            (Operands::Jump { target }, _) => {
                let mut s = Self::op(at, name);
                s.target = Some(*target);
                Segment::Op(s)
            }
            (Operands::Assign(a), _) => {
                let mut s = Self::op(at, name);
                match a {
                    Assign::TableWord { table, value } => {
                        s.table = Some(self.table(table, true)?);
                        s.args = vec![rpn(value)];
                    }
                    Assign::Int { var, value } => {
                        s.var = Some(*var);
                        s.args = vec![rpn(value)];
                    }
                    Assign::StrFromTable { var, table, .. } => {
                        s.var = Some(*var);
                        s.table = Some(self.table(table, false)?);
                    }
                    Assign::StrFromSlot { var, src, .. } => {
                        s.var = Some(*var);
                        s.src = Some(*src);
                    }
                }
                Segment::Op(s)
            }
            (Operands::Args(a), _) => {
                let mut s = Self::op(at, name);
                s.args = a.iter().map(|o| rpn(&o.expr)).collect();
                Segment::Op(s)
            }
        })
    }
}

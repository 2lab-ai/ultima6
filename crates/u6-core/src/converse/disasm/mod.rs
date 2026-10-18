//! Port-only (spec 08 rev 3 Design 2 and 6, spec 09 rev 3 Design 5, spec 04 W3a's disassembler
//! half): the conversation disassembler.
//!
//! [`disassemble`] decodes one script (its archive letter, item and bytes) into an ordered list
//! of [`Item`]s that covers every byte exactly once. It follows the interpreter's control flow
//! from the script's start instead of sweeping the bytes, because scripts keep data in their
//! code (tables the ops address) and bytes no path reads (the tail of an item):
//!
//! - entries: the header, the look text after `f1` (run by `run_text`, then the read position
//!   moves past the first `f2`), the greeting after that `f2`, and from each item its successors
//!   as the interpreter computes them (the next byte, a `b0` target, both ways of an `a1`, the
//!   answer after a keyword block's `f6`, the byte after an input op or an `ee`). These are the
//!   *direct* paths;
//! - the four skip loops (`a1` / `a3`, a keyword block without a match, an answered block, the
//!   scan for `f2`) read raw bytes, not items (stepping over operands with `run.rs`
//!   `operand_len`). Their landing is computed as the interpreter computes it and followed after
//!   every direct path, on trial, one landing at a time: a landing inside an item, or one whose
//!   path would refuse the script, is recorded as a [`Hazard`] and not decoded (a failed trial
//!   is tried again once the landings after it are in, as a table they keep may stop it first).
//!   Most of a conversation is reached this way (each keyword block's successor is a no-match
//!   landing);
//! - every item is decoded from its first byte alone, so a byte has one meaning; two decodes that
//!   disagree on a byte refuse the script ([`Reason::Overlap`]). A path that enters a text run
//!   after its start (a `b0` target or a skip-loop landing inside the text) splits the run
//!   there, so every run is read from its start ([`Disasm::run_splits`]). The split can fall
//!   anywhere in the text, before a trailing newline or in the middle of a sentence; the catalog
//!   links the parts ([`catalog::RunUnit::part_of`]);
//! - tables are the bytes the ops address as data (`b5 d2`, `a6 d2`, and the address operand of
//!   the expression ops `b4`, `b7`, `c3`): NUL-ended strings or words from the address, up to a
//!   `b8` at an entry's start, the next table address, the script's end or direct code. A table
//!   without its `b8` there has no end of its own: it also ends at landing code, but never
//!   before the last entry a constant index reads, its own or one that runs on into it from the
//!   tables before it. A string address an op names inside another table's string (not at a
//!   string's start) is of that table ([`Table::inner`]): a read there reads the string's rest,
//!   so the table never ends before that string; a word address inside a word is not, and its
//!   table ends there. The bytes a direct use reads are closed to landings: its table's
//!   needed entries, of either kind (up to a constant index's, an inner address's, all of them
//!   for a search), and for a direct search of tables with no `b8`, the bytes it reads on past
//!   them. A landing's table address there that does not chain (of the other kind, a word
//!   address inside a word, or past a search's tables anywhere but where they end) drops the
//!   landings whose path makes it (`Dropped(_, TableKinds)`), and landing code in the bytes a
//!   search reads on is closed as a table's bytes are: the direct table is never cut, nor the
//!   script refused, for a landing;
//! - what is left is [`Kind::Unreached`].
//!
//! Tables and the trial. A table use is checked when its op is decoded, inside the path that
//! decodes it: the address must be a literal inside the script, read with one kind, and not in
//! the code of a direct path or of the same path. On a direct path a failed check refuses the
//! script; on a landing's path it drops that landing ([`HazardKind::Dropped`]). A kept use closes
//! its address to code at once, and every byte of its table from the next round on: another
//! landing that reaches any of them stops there ([`HazardKind::IntoData`], its only hazard),
//! while a landing that reaches a table it names itself is dropped (`Dropped(_, TableInCode)`);
//! a direct path that reaches the address refuses the script ([`Reason::TableInCode`]), and
//! direct code after it ends the table (a constant index it cuts off refuses the script,
//! [`Reason::TableIndex`]). Only kept paths reserve bytes. The choices the trial forces:
//!
//! - a table named by a kept path wins over another landing's code inside it (at its address or
//!   past it): the decode runs again with the table's bytes closed to landing paths from the
//!   start (that landing then stops there);
//! - it never wins over direct code or over its own path's code: that landing is dropped
//!   (`Dropped(_, TableInCode)`), and direct code is never read as a table. A path's own code
//!   is every byte its flow reads, the code it joins included (code another landing decoded
//!   first, that its path then runs), not only the code it decoded; and the tables it names are
//!   those of every op it runs, joined or not;
//! - bytes closed that way that no table of a kept path holds in the next round are opened again
//!   and can no longer win: a landing that names a table over other landing code there is
//!   dropped (the intruding landing, when the table is a direct path's);
//! - a constant index is checked against the final tables (its table and the ones that follow
//!   it without a gap): past them, a direct use refuses the script and a landing's use drops,
//!   in the next round, every landing whose path runs that op (the one that decoded it and
//!   those that join it); so does any other read of a table with no entry (a
//!   computed index, a search with no `b8` at the address), which reads past the table. A
//!   search reads, as the interpreter does, from its own address an entry at a time up to a
//!   `b8` at an entry's start: one from a `b8` reads nothing; any other reads every entry of
//!   its tables (so they never end at landing code), and with no `b8` in them it reads on
//!   past the last one, where a `b8` that starts a table of either kind stops it. Onto any
//!   other item that is a read past the table (for a direct search, only onto direct code or
//!   another direct table: landings are closed out of those bytes); past the script's end,
//!   over bytes no item
//!   holds, it is a hazard ([`HazardKind::SearchPastEnd`]). Such a drop is checked again at
//!   the end of each round: the dropped landings are tried on that round's decode, lowest
//!   landing first, and the first whose reads would all hold there (its cause is gone) is
//!   taken again, one per round, so that a later one is tried with it in. A landing dropped
//!   for a read past a table after that stays dropped: its trial can end otherwise than its
//!   round does (its table address in another landing's code, which that round's own table
//!   wins over), and the check would lift it again each time.
//!
//! The rounds end: each one grows the landings taken again, or (with those unchanged) the
//! dropped landings or the settled bytes, or (with those unchanged) the reserved bytes, all
//! bounded: a landing is taken again at most once, and the bytes by the script's length.
//!
//! Order. Conflicts between landings are resolved in ascending landing address: the landings
//! are taken in the order of the address where reading goes on (then by loop, stop byte and
//! context), whatever order they are found in, and a failed trial is tried again in the same
//! order. So the decode and the catalog are functions of the script's bytes alone (a test keeps
//! the waiting landings in other orders and gets the same catalog, byte for byte). The
//! conflicts that rule resolves include:
//!
//! - two landings whose code disagrees on a byte (one would decode an item over bytes the
//!   other decoded otherwise): the lower keeps its code, the other is dropped
//!   (`Dropped(_, Overlap)`);
//! - two landings that read one address with two kinds: the lower keeps its use, the other is
//!   dropped (`Dropped(_, TableKinds)`);
//! - a landing whose code holds the address of a table another landing names: taken after the
//!   namer, it stops there at once ([`HazardKind::IntoData`]), or is dropped when an item of
//!   it would hold the address (`Dropped(_, TableInCode)`); taken first, its code is in for
//!   that round and the table wins over it in the next. Of two landings each naming a table
//!   over the other's code, the lower keeps its table.
//!
//! Operands are decoded with the evaluator's grammar ([`Expr`]), so their bytes are never read
//! as text. A byte the interpreter has no arm for is refused with a named [`Reason`], never
//! guessed. The op tables are in [`ops`]; the catalog of runs and table strings built on this
//! is in [`catalog`]. The catalog's JSON form uses serde: no new crate (u6-hdpack already
//! depends on serde and serde_json) and no measurable weight in the wasm build, and spec 09 V5's
//! bundle exporter, which runs in wasm, reads the same catalog, so it stays in u6-core.

pub mod catalog;
mod decode;
pub mod ops;
#[cfg(test)]
pub(crate) mod trace;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_real;

use std::fmt;

use decode::{Decoder, Plan};
pub use ops::Escape;

/// One decoded script: its items in offset order, covering `0..len` exactly once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disasm {
    /// Archive letter (`b'a'` / `b'b'`) and item, as given.
    pub archive: u8,
    pub item: u16,
    pub len: u32,
    pub items: Vec<Item>,
    /// Places where a skip loop or a path does not meet the items (see the module doc).
    pub hazards: Vec<Hazard>,
    /// Offsets where a path enters text in the middle: the run there was split in two.
    pub run_splits: Vec<u32>,
}

/// A byte range of a script.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    pub offset: u32,
    pub len: u32,
}

impl Span {
    pub fn end(self) -> u32 {
        self.offset + self.len
    }

    /// The bytes of `script` this span covers.
    pub fn bytes(self, script: &[u8]) -> &[u8] {
        &script[self.offset as usize..self.end() as usize]
    }
}

/// One item: a byte range and what the interpreter does with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub offset: u32,
    pub len: u32,
    pub kind: Kind,
}

impl Item {
    pub fn span(&self) -> Span {
        Span {
            offset: self.offset,
            len: self.len,
        }
    }

    pub fn end(&self) -> u32 {
        self.offset + self.len
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `ff` and the byte after it (the script's first two bytes).
    Header,
    /// The NPC's name: from byte 2 up to the first `f1`.
    Name,
    /// `f1` (the look text follows) or `f2` (the greeting follows).
    Marker(Marker),
    /// A text run: bytes 0x20..0x7f and newline between two non-text bytes.
    Text(Run),
    /// A statement op and its operands.
    Op(Op),
    /// `ef`, the keywords separated by `,`, then `f6` (the answer follows as items).
    Keywords(Vec<Span>),
    /// `ee`: ends a keyword block's answer; the input loop goes on after it.
    BlockEnd,
    /// An input op of the input loop (`f7`..`fc`).
    Input(Input),
    /// A byte that ends the conversation.
    Stop(Stop),
    /// Data the ops address.
    Table(Table),
    /// Bytes no path of the interpreter reads.
    Unreached,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Look,
    Greeting,
}

/// A text run, split into plain text and escapes (the placeholders of a translation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub parts: Vec<Part>,
}

/// Part of a run: plain text (`escape` = `None`) or one escape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub offset: u32,
    pub len: u32,
    pub escape: Option<Escape>,
}

impl Run {
    /// The escapes of the run, in order.
    pub fn placeholders(&self) -> impl Iterator<Item = &Part> {
        self.parts.iter().filter(|p| p.escape.is_some())
    }
}

/// A statement op: its byte and operands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Op {
    pub code: u8,
    pub operands: Operands,
}

impl Op {
    /// The row of [`ops::STATEMENT_OPS`] this op was decoded with.
    pub fn info(&self) -> &'static ops::StatementOp {
        ops::statement_op(self.code).expect("decoded ops are in the table")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operands {
    /// Expressions in order; `actor` marks the ones the executor resolves to an actor.
    Args(Vec<Operand>),
    /// `b0`: the new read position (the low word of the operand, sign-extended).
    Jump {
        target: u32,
    },
    Print(Print),
    Assign(Assign),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operand {
    pub actor: bool,
    pub expr: Expr,
}

/// The forms of `b5`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Print {
    /// `d2 <table> (b1 | <index>)`: a table string, byte by byte through the escape layer.
    Table(TableRef),
    /// `d5`: prints nothing.
    Nothing,
    /// `<kind> <slot> b3 <byte>`: string slot `slot` through printf; `kind` and the last byte
    /// are read and not used.
    Slot { kind: u8, slot: u8, skipped: u8 },
}

/// The forms of `a6`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Assign {
    /// `d2 <table> (b1 a8 | <index ending a8>) <value>`: a word of a table is overwritten.
    TableWord { table: TableRef, value: Expr },
    /// `<var> b2 a8 <value>`: a word variable.
    Int { var: u8, value: Expr },
    /// `<var> <kind> a8 d2 <table> (b1 | <index>)`: a table string copied into string slot
    /// `var`.
    StrFromTable { var: u8, kind: u8, table: TableRef },
    /// `<var> <kind> a8 <src> b3`: string slot `src` copied into string slot `var`.
    StrFromSlot { var: u8, kind: u8, src: u8 },
}

/// A table operand: its script address and the index expression (`None` = `b1`, the first
/// element). The address is the read position the interpreter moves to: the operand's low word,
/// sign-extended (so an operand with high bits set reads the table its low word names).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRef {
    pub addr: u32,
    pub index: Option<Expr>,
}

/// An expression: its tokens up to and including the terminator `end` (`a7` / `a8`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expr {
    pub offset: u32,
    pub len: u32,
    pub tokens: Vec<Token>,
    pub end: u8,
}

impl Expr {
    /// The value of a one-token expression of a literal or a pushed value.
    pub fn constant(&self) -> Option<u32> {
        match self.tokens.as_slice() {
            [t] => t.kind.value(),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub offset: u32,
    pub kind: TokenKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Lit32(u32),
    Lit16(u16),
    Lit8(u8),
    /// An op of [`ops::EXPR_OPS`].
    Op(u8),
    /// Any other byte, pushed as its value.
    Value(u8),
}

impl TokenKind {
    /// The bytes the token takes (its prefix and literal).
    pub fn width(self) -> u32 {
        match self {
            TokenKind::Lit32(_) => 5,
            TokenKind::Lit16(_) => 3,
            TokenKind::Lit8(_) => 2,
            TokenKind::Op(_) | TokenKind::Value(_) => 1,
        }
    }

    /// The value a literal or a pushed byte puts on the stack (`None` for an op).
    pub fn value(self) -> Option<u32> {
        match self {
            TokenKind::Lit32(v) => Some(v),
            TokenKind::Lit16(v) => Some(u32::from(v)),
            TokenKind::Lit8(v) | TokenKind::Value(v) => Some(u32::from(v)),
            TokenKind::Op(_) => None,
        }
    }
}

/// An input op of the input loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// `f7`: the player's words.
    Ask,
    /// `f8` and the allowed keys, up to the `ef` that follows (not part of this item).
    Keys(Span),
    /// `f9 <slot> b3`: a line into string slot `slot`.
    Line { slot: u8 },
    /// `fa`: one key.
    Key,
    /// `fb <var> b2`: a number into word variable `var`.
    Number { var: u8 },
    /// `fc <var> b2`: one digit key into word variable `var`.
    Digit { var: u8 },
}

/// Bytes that end the conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// 0: `run_text` stops before it; the input loop prints the logical-error line.
    Nul,
    /// `ff`: the input loop ends.
    End,
    /// A control byte (below 0x20, not newline): the control-character line, then the
    /// logical-error line.
    Control(u8),
    /// `f0`, `f3`..`f6`, `fd`, `fe`: the input loop's logical-error line.
    Error(u8),
}

/// A table: NUL-ended strings or words, with the `b8` that ends it when there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub words: bool,
    pub entries: Vec<Span>,
    /// A `b8` at an entry's start ends the table (the last byte of the item).
    pub terminated: bool,
    /// Addresses ops name inside its strings (a string table only): a read there starts in the
    /// middle of a string and reads its rest; an index from there counts that rest as the
    /// first string.
    pub inner: Vec<u32>,
}

/// A landing or entry that does not meet the items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hazard {
    /// Where the interpreter would go on reading.
    pub at: u32,
    pub kind: HazardKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HazardKind {
    /// A skip loop stops on a byte inside an item, or lands inside one.
    Scan(Scan),
    /// A skip loop finds no stop byte before the script's end.
    ScanPastEnd(Scan),
    /// A search (`b7` / `c3`, at the op) of a table with no `b8`, nor one in the tables that
    /// follow it without a gap: when no entry matches, the interpreter reads on past the last
    /// entry over these bytes, which no item holds, and past the script's end.
    SearchPastEnd(Span),
    /// A landing's path reaches a byte of the range of a table a kept path names (its address
    /// or any byte after it): it stops there.
    IntoData,
    /// The path after a skip loop's landing would refuse the script for this reason (its table
    /// uses included): it is not decoded, and the addresses it names are not tables.
    Dropped(Scan, Reason),
    /// An expression op takes more values than its expression pushed: the evaluator reads 0
    /// below its stack and drops what it writes there (a `b4` / `b7` / `c3` then has no table
    /// address and claims no table).
    Underflow,
    /// An expression puts a value above the evaluator's ten slots: it is dropped, and a read
    /// there gives 0 (a `b4` / `b7` / `c3` whose address is up there claims no table).
    Overflow,
}

/// The interpreter's skip loops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scan {
    /// `a1` with a false condition: past the first `a3` / `a2` (stepping over `b0` / `d3` /
    /// `d4` operands).
    If,
    /// `a3`: past the first `a2`, the same way.
    Else,
    /// A keyword block without a match: to the next `ef` / `ff`, or past the next `ee`, the
    /// same way.
    NoMatch,
    /// A keyword block once the input is answered: past the next `ee` (no stepping).
    Answered,
    /// The move from the look text to the greeting: past the next `f2`.
    Greeting,
}

/// Why a script is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub at: u32,
    pub reason: Reason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The script is longer than the interpreter's script buffer ([`super::SCRIPT_LEN`]).
    TooLong,
    /// The script does not start with `ff` and one more byte.
    NoHeader,
    /// No `f1` after the header.
    NoLookMarker,
    /// No `f2` after the `f1`.
    NoGreetingMarker,
    /// The bytes end inside an item.
    Truncated,
    /// The interpreter's control flow reaches the script's end without a stop byte.
    PastEnd,
    /// A byte the executor's default arm takes (the unknown-command line).
    UnknownOp(u8),
    /// An operand the op's arm rejects (a wrong tag, a missing `a8`, `a6`'s kind 0).
    Malformed(u8),
    /// A `b0` target outside the script.
    JumpOutside,
    /// Two decodes disagree on a byte.
    Overlap,
    /// A table address outside the script.
    TableOutside,
    /// A table address inside decoded code.
    TableInCode,
    /// A table address of `b4` / `b7` / `c3` that is not a literal.
    TableNotLiteral,
    /// One table read both as strings and as words.
    TableKinds,
    /// A constant index past a table's entries (and those of the tables that follow it), or
    /// another read of a table with no entry, which reads past it.
    TableIndex,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::TooLong => write!(f, "longer than the script buffer"),
            Reason::NoHeader => write!(f, "no header"),
            Reason::NoLookMarker => write!(f, "no look marker"),
            Reason::NoGreetingMarker => write!(f, "no greeting marker"),
            Reason::Truncated => write!(f, "truncated"),
            Reason::PastEnd => write!(f, "flow past the end"),
            Reason::UnknownOp(b) => write!(f, "unknown op {b:#04x}"),
            Reason::Malformed(b) => write!(f, "malformed operand of op {b:#04x}"),
            Reason::JumpOutside => write!(f, "jump outside the script"),
            Reason::Overlap => write!(f, "overlapping decodes"),
            Reason::TableOutside => write!(f, "table outside the script"),
            Reason::TableInCode => write!(f, "table inside code"),
            Reason::TableNotLiteral => write!(f, "table address not a literal"),
            Reason::TableKinds => write!(f, "table read as strings and words"),
            Reason::TableIndex => write!(f, "table index past its entries"),
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {:#06x}", self.reason, self.at)
    }
}

/// Decodes one script (see the module doc). Never panics: bytes that end inside an item, or a
/// byte the interpreter has no arm for, refuse the script.
pub fn disassemble(archive: u8, item: u16, bytes: &[u8]) -> Result<Disasm, Refusal> {
    if bytes.len() > super::SCRIPT_LEN {
        return Err(Refusal {
            at: super::SCRIPT_LEN as u32,
            reason: Reason::TooLong,
        });
    }
    // Decode rounds until one needs no other (the module doc says why each one is needed and
    // why they end): each round's plan holds the addresses closed to landing paths from the
    // start, the addresses that can no longer be, and the landings to drop.
    let mut plan = Plan::default();
    // A bound the module doc's argument gives (a landing taken again at most once, each landing
    // dropped at most twice for a read past a table, and the bytes reserved and settled), with
    // room to spare: a test build checks the rounds end.
    let mut rounds = 0usize;
    loop {
        rounds += 1;
        debug_assert!(rounds <= 64 * bytes.len() + 64, "the rounds end");
        let next = {
            let mut d = Decoder::new(bytes, &plan);
            d.run()?;
            match d.revise() {
                Some(next) => next,
                None => {
                    let pre = d.clone();
                    d.claim_tables()?;
                    match d.check_table_indices(&pre)? {
                        Some(next) => next,
                        None => {
                            d.search_hazards();
                            let items = d.finish();
                            return Ok(Disasm {
                                archive,
                                item,
                                len: bytes.len() as u32,
                                items,
                                hazards: d.hazards,
                                run_splits: d.run_splits,
                            });
                        }
                    }
                }
            }
        };
        plan = next;
    }
}

/// The entries a use of the table at `addr` can reach: from the table item over `addr` (from
/// its start, or from an address inside one of its strings, [`Table::inner`]: that string's
/// rest first), and, while a table has no `b8`, the one of the same kind that starts where it
/// ends (an index runs on into it, as `table_element` counts NULs and `b4` / `c3` step over
/// words). As (table offset, index in that table, entry). `containing` gives the item over an
/// offset.
fn reachable<'a>(
    containing: impl Fn(u32) -> Option<&'a Item>,
    addr: u32,
    words: bool,
) -> Vec<(u32, u32, Span)> {
    let mut out = Vec::new();
    let Some(first) = containing(addr) else {
        return out;
    };
    if first.offset != addr
        && !matches!(&first.kind, Kind::Table(t) if !t.words && t.inner.contains(&addr))
    {
        return out;
    }
    let mut at = first.offset;
    while let Some(Item {
        offset,
        kind: Kind::Table(t),
        len,
    }) = containing(at)
    {
        if *offset != at || t.words != words {
            break;
        }
        for (i, &e) in t.entries.iter().enumerate() {
            if e.end() < addr {
                continue;
            }
            // The string `addr` falls in: its rest.
            let e = if e.offset < addr {
                Span {
                    offset: addr,
                    len: e.end() - addr,
                }
            } else {
                e
            };
            out.push((at, i as u32, e));
        }
        if t.terminated {
            break;
        }
        at += len;
    }
    out
}

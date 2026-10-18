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
//!   answer after a keyword block's `f6`, the byte after an input op or an `ee`);
//! - the four skip loops (`a1` / `a3`, a keyword block without a match, an answered block, the
//!   scan for `f2`) read raw bytes, not items. Their landing is computed as the interpreter
//!   computes it; when it does not fall on an item start, or the byte it stopped on is not that
//!   item's own byte, the landing is recorded as a [`Hazard`], not decoded;
//! - every item is decoded from its first byte alone, so a byte has one meaning; two decodes that
//!   disagree on a byte refuse the script ([`Reason::Overlap`]), except a second entry into a
//!   text run, which is a hazard ([`HazardKind::RunEntry`]);
//! - tables are the bytes the ops address as data (`b5 d2`, `a6 d2`, and the address operand of
//!   the expression ops `b4`, `b7`, `c3`): NUL-ended strings or words, up to a `b8` at an
//!   entry's start, the next decoded byte or the script's end. They are never decoded as ops;
//! - what is left is [`Kind::Unreached`].
//!
//! Operands are decoded with the evaluator's grammar ([`Expr`]), so their bytes are never read
//! as text. A byte the interpreter has no arm for is refused with a named [`Reason`], never
//! guessed. The op tables are in [`ops`]; the catalog of runs and table strings built on this
//! is in [`catalog`].

pub mod catalog;
pub mod ops;
#[cfg(test)]
pub(crate) mod trace;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_real;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub use ops::Escape;
use ops::{Arg, Flow, Shape};

/// One decoded script: its items in offset order, covering `0..len` exactly once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disasm {
    /// Archive letter (`b'a'` / `b'b'`) and item, as given.
    pub archive: u8,
    pub item: u16,
    pub len: u32,
    pub items: Vec<Item>,
    /// Places where a skip loop or a second entry does not meet the items (see the module doc).
    pub hazards: Vec<Hazard>,
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
    Jump { target: u32 },
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
/// element).
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
    pub fn len(self) -> u32 {
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
    /// A jump, scan or second path enters a text run after its start.
    RunEntry,
    /// A skip loop stops on a byte inside an item, or lands inside one.
    Scan(Scan),
    /// A skip loop finds no stop byte before the script's end.
    ScanPastEnd(Scan),
    /// A path (in practice one after a skip loop) reaches a table's bytes.
    IntoData,
    /// An expression op takes more values than its expression pushed: the evaluator reads 0
    /// below its stack and drops what it writes there (a `b4` / `b7` / `c3` then has no table
    /// address and claims no table).
    Underflow,
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
    /// The script does not start with `ff` and one more byte.
    NoHeader,
    /// No `f1` after the header.
    NoLookMarker,
    /// No `f2` after the look text.
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
    /// A constant index past a table's entries.
    TableIndex,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
    // Decode, collect the table addresses the ops name, and decode again with those addresses
    // closed to code, until no new address turns up (each round adds one at least).
    let mut reserved = BTreeSet::new();
    let mut d = loop {
        let mut d = Decoder::new(bytes, reserved.clone());
        d.run()?;
        let addrs: BTreeSet<u32> = d.table_uses()?.iter().map(|u| u.addr).collect();
        if addrs.is_subset(&reserved) {
            break d;
        }
        reserved.extend(addrs);
    };
    d.claim_tables()?;
    d.check_table_indices()?;
    let items = d.finish();
    Ok(Disasm {
        archive,
        item,
        len: bytes.len() as u32,
        items,
        hazards: d.hazards,
    })
}

/// The interpreter's two kinds of read position: inside the look text (a stop byte moves to
/// the greeting) and anywhere else (a stop byte goes to the input loop).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ctx {
    Look,
    Main,
}

/// One pending skip loop: where it stopped and where reading goes on.
#[derive(Clone, Copy, Debug)]
struct Landing {
    scan: Scan,
    /// The byte the loop stopped on.
    stop: u32,
    /// Where reading goes on (`stop + 1`, or `stop` for a no-match scan stopping on `ef`/`ff`).
    at: u32,
}

/// What the interpreter does at a stop byte the input loop reads.
const NO_OWNER: u32 = u32::MAX;

/// A table use the ops make: its kind and the constant index (`None` = any).
#[derive(Clone, Copy, Debug)]
struct TableUse {
    addr: u32,
    words: bool,
    index: Option<u32>,
    at: u32,
}

struct Decoder<'a> {
    b: &'a [u8],
    /// Table addresses: no path decodes code there.
    reserved: BTreeSet<u32>,
    items: BTreeMap<u32, Item>,
    /// For each byte, the start of the item that owns it.
    owner: Vec<u32>,
    seen: BTreeSet<(u32, Ctx)>,
    work: Vec<(u32, Ctx)>,
    scans: Vec<(Landing, Ctx)>,
    landed: Vec<Landing>,
    hazards: Vec<Hazard>,
}

fn refuse<T>(at: u32, reason: Reason) -> Result<T, Refusal> {
    Err(Refusal { at, reason })
}

impl<'a> Decoder<'a> {
    fn new(b: &'a [u8], reserved: BTreeSet<u32>) -> Self {
        Self {
            b,
            reserved,
            items: BTreeMap::new(),
            owner: vec![NO_OWNER; b.len()],
            seen: BTreeSet::new(),
            work: Vec::new(),
            scans: Vec::new(),
            landed: Vec::new(),
            hazards: Vec::new(),
        }
    }

    fn len(&self) -> u32 {
        self.b.len() as u32
    }

    /// The byte at `at`, or a [`Reason::Truncated`] refusal.
    fn byte(&self, at: u32) -> Result<u8, Refusal> {
        match self.b.get(at as usize) {
            Some(&b) => Ok(b),
            None => refuse(at, Reason::Truncated),
        }
    }

    fn le32(&self, at: u32) -> Result<u32, Refusal> {
        let mut v = 0;
        for i in 0..4 {
            v |= u32::from(self.byte(at + i)?) << (8 * i);
        }
        Ok(v)
    }

    fn le16(&self, at: u32) -> Result<u16, Refusal> {
        Ok(u16::from(self.byte(at)?) | (u16::from(self.byte(at + 1)?) << 8))
    }

    fn first(&self, from: u32, b: u8) -> Option<u32> {
        let from = from as usize;
        self.b
            .get(from..)?
            .iter()
            .position(|&x| x == b)
            .map(|i| (from + i) as u32)
    }

    // ---- control flow ------------------------------------------------------------------------

    fn run(&mut self) -> Result<(), Refusal> {
        let len = self.len();
        if len < 2 || self.b[0] != ops::HEADER {
            return refuse(0, Reason::NoHeader);
        }
        let Some(look) = self.first(0, ops::LOOK) else {
            return refuse(0, Reason::NoLookMarker);
        };
        if look < 2 {
            return refuse(look, Reason::NoHeader);
        }
        self.insert(Item {
            offset: 0,
            len: 2,
            kind: Kind::Header,
        })?;
        if look > 2 {
            self.insert(Item {
                offset: 2,
                len: look - 2,
                kind: Kind::Name,
            })?;
        }
        self.insert(Item {
            offset: look,
            len: 1,
            kind: Kind::Marker(Marker::Look),
        })?;
        self.work.push((look + 1, Ctx::Look));
        // Reached by spell, the look text is not run: the scan for `f2` starts at 0.
        self.scan_greeting(0, Ctx::Main)?;
        loop {
            while let Some((at, ctx)) = self.work.pop() {
                self.visit(at, ctx)?;
            }
            // Skip-loop landings come last, so that a landing never decodes bytes a direct path
            // owns.
            let Some((landing, ctx)) = self.scans.pop() else {
                break;
            };
            self.land(landing, ctx)?;
        }
        if !self.landed.iter().any(|l| l.scan == Scan::Greeting) {
            return refuse(look, Reason::NoGreetingMarker);
        }
        self.check_landings();
        Ok(())
    }

    /// Goes on reading at `at` in `ctx`: decodes the item there (once) and queues what follows.
    fn visit(&mut self, at: u32, ctx: Ctx) -> Result<(), Refusal> {
        if !self.seen.insert((at, ctx)) {
            return Ok(());
        }
        if at >= self.len() {
            return refuse(at, Reason::PastEnd);
        }
        if self.reserved.contains(&at) {
            self.hazard(at, HazardKind::IntoData);
            return Ok(());
        }
        let start = match self.owner[at as usize] {
            NO_OWNER => {
                let item = self.decode(at)?;
                self.insert(item)?
            }
            s => s,
        };
        if start != at {
            if matches!(self.items[&start].kind, Kind::Text(_)) {
                self.hazard(at, HazardKind::RunEntry);
                let end = self.items[&start].end();
                self.work.push((end, ctx));
                return Ok(());
            }
            return refuse(at, Reason::Overlap);
        }
        self.follow(start, ctx)
    }

    /// Queues the successors of the item at `at` read in `ctx`.
    fn follow(&mut self, at: u32, ctx: Ctx) -> Result<(), Refusal> {
        let item = &self.items[&at];
        let end = item.end();
        // A stop byte inside the look text: `run_text` returns and the read position moves
        // past the next `f2` (the stop byte itself is the first one the scan reads).
        let stops = matches!(
            item.kind,
            Kind::BlockEnd | Kind::Input(_) | Kind::Stop(_) | Kind::Marker(_)
        );
        if stops && ctx == Ctx::Look {
            return self.scan_greeting(at, ctx);
        }
        match &item.kind {
            Kind::Text(_) | Kind::BlockEnd | Kind::Input(_) => self.work.push((end, ctx)),
            Kind::Op(op) => match (op.info().flow, &op.operands) {
                (Flow::Jump, Operands::Jump { target }) => self.work.push((*target, ctx)),
                (Flow::If, _) => {
                    self.work.push((end, ctx));
                    self.scan_ops(Scan::If, end, ctx);
                }
                (Flow::Else, _) => self.scan_ops(Scan::Else, end, ctx),
                (Flow::Bye, _) => {}
                _ => self.work.push((end, ctx)),
            },
            Kind::Keywords(_) => {
                self.work.push((end, ctx));
                self.scan_ops(Scan::NoMatch, end - 1, ctx);
                match self.first(at + 1, ops::BLOCK_END) {
                    Some(stop) => self.scans.push((
                        Landing {
                            scan: Scan::Answered,
                            stop,
                            at: stop + 1,
                        },
                        ctx,
                    )),
                    None => self.hazard(at, HazardKind::ScanPastEnd(Scan::Answered)),
                }
            }
            // The header items are never read as code; stops end the conversation (the
            // logical-error line, the end of the input loop).
            Kind::Header
            | Kind::Name
            | Kind::Marker(_)
            | Kind::Stop(_)
            | Kind::Table(_)
            | Kind::Unreached => {}
        }
        Ok(())
    }

    fn hazard(&mut self, at: u32, kind: HazardKind) {
        let h = Hazard { at, kind };
        if !self.hazards.contains(&h) {
            self.hazards.push(h);
        }
    }

    /// `while (script[pc++] != f2);` from `from`. The greeting is the way into the input loop,
    /// so its landing is followed at once, as a direct path.
    fn scan_greeting(&mut self, from: u32, _ctx: Ctx) -> Result<(), Refusal> {
        match self.first(from, ops::GREETING) {
            Some(stop) => self.land(
                Landing {
                    scan: Scan::Greeting,
                    stop,
                    at: stop + 1,
                },
                Ctx::Main,
            ),
            None => {
                self.hazard(from, HazardKind::ScanPastEnd(Scan::Greeting));
                Ok(())
            }
        }
    }

    /// The skip loops that step over `b0` / `d3` / `d4` operands (`exec.rs` 0xa1 and
    /// `skip_to`, `run.rs` `keywords`).
    fn scan_ops(&mut self, scan: Scan, from: u32, ctx: Ctx) {
        let mut i = from as usize;
        while let Some(&b) = self.b.get(i) {
            let stop = i as u32;
            i += 1;
            let found = match scan {
                Scan::If => (b == 0xa3 || b == 0xa2).then_some(stop + 1),
                Scan::Else => (b == 0xa2).then_some(stop + 1),
                Scan::NoMatch => match b {
                    ops::KEYWORDS | ops::HEADER => Some(stop),
                    ops::BLOCK_END => Some(stop + 1),
                    _ => None,
                },
                Scan::Answered | Scan::Greeting => None,
            };
            if let Some(at) = found {
                self.scans.push((Landing { scan, stop, at }, ctx));
                return;
            }
            i += ops::skip_len(b) as usize;
        }
        self.hazard(from, HazardKind::ScanPastEnd(scan));
    }

    /// Goes on reading where a skip loop landed, unless it stopped on a byte inside an item.
    /// A stop byte no path decoded yet is decoded here (the loop reads it as that byte).
    fn land(&mut self, l: Landing, ctx: Ctx) -> Result<(), Refusal> {
        self.landed.push(l);
        if self.reserved.contains(&l.stop) {
            self.hazard(l.at, HazardKind::IntoData);
            return Ok(());
        }
        match self.owner[l.stop as usize] {
            NO_OWNER if l.at != l.stop => {
                let item = self.decode(l.stop)?;
                self.insert(item)?;
            }
            NO_OWNER => {}
            o if o == l.stop => {}
            _ => return Ok(()),
        }
        if l.at >= self.len() {
            self.hazard(l.at, HazardKind::ScanPastEnd(l.scan));
            return Ok(());
        }
        match self.owner[l.at as usize] {
            o if o == NO_OWNER || o == l.at => self.work.push((l.at, ctx)),
            _ => {}
        }
        Ok(())
    }

    /// After decoding: every skip loop must stop on the first byte of a code item and land on
    /// an item's start; a landing inside a text run is a run entry.
    fn check_landings(&mut self) {
        let landed = std::mem::take(&mut self.landed);
        for l in &landed {
            let code = |o: u32| {
                self.items.get(&o).is_some_and(|it| {
                    !matches!(
                        it.kind,
                        Kind::Header | Kind::Name | Kind::Table(_) | Kind::Unreached
                    )
                })
            };
            let stop_ok = self.owner[l.stop as usize] == l.stop && code(l.stop);
            let at_owner = self.owner.get(l.at as usize).copied();
            if !stop_ok {
                self.hazard(l.at, HazardKind::Scan(l.scan));
            } else if let Some(o) = at_owner
                && o != l.at
            {
                let in_run = self
                    .items
                    .get(&o)
                    .is_some_and(|it| matches!(it.kind, Kind::Text(_)));
                let kind = if in_run {
                    HazardKind::RunEntry
                } else {
                    HazardKind::Scan(l.scan)
                };
                self.hazard(l.at, kind);
            }
        }
        self.landed = landed;
    }

    /// Adds a decoded item; a text run that starts earlier than runs already decoded inside it
    /// absorbs them (each a run entry). Returns the item's start.
    fn insert(&mut self, item: Item) -> Result<u32, Refusal> {
        let (s, e) = (item.offset, item.end());
        if e > self.len() {
            return refuse(self.len(), Reason::Truncated);
        }
        if !matches!(item.kind, Kind::Table(_))
            && let Some(&t) = self.reserved.range(s..e).next()
        {
            return refuse(t, Reason::TableInCode);
        }
        let text = matches!(item.kind, Kind::Text(_));
        let mut absorbed = Vec::new();
        for p in s..e {
            match self.owner[p as usize] {
                NO_OWNER => {}
                o if text
                    && o > s
                    && matches!(self.items[&o].kind, Kind::Text(_))
                    && self.items[&o].end() == e =>
                {
                    if !absorbed.contains(&o) {
                        absorbed.push(o);
                    }
                }
                _ => return refuse(p, Reason::Overlap),
            }
        }
        for o in absorbed {
            self.items.remove(&o);
            self.hazard(o, HazardKind::RunEntry);
        }
        for p in s..e {
            self.owner[p as usize] = s;
        }
        self.items.insert(s, item);
        Ok(s)
    }

    // ---- items -------------------------------------------------------------------------------

    /// Decodes the item that starts at `at` from its first byte.
    fn decode(&self, at: u32) -> Result<Item, Refusal> {
        let b = self.byte(at)?;
        let (len, kind) = if ops::is_text(b) {
            let end = (at..self.len())
                .find(|&i| !ops::is_text(self.b[i as usize]))
                .unwrap_or(self.len());
            (end - at, Kind::Text(self.run_parts(at, end)))
        } else if b == 0 {
            (1, Kind::Stop(Stop::Nul))
        } else if b < 0x20 {
            (1, Kind::Stop(Stop::Control(b)))
        } else if b == ops::KEYWORDS {
            self.keywords(at)?
        } else if b == ops::BLOCK_END {
            (1, Kind::BlockEnd)
        } else if b >= 0xf0 {
            self.input(at, b)?
        } else {
            let op = self.op(at, b)?;
            (op.0, Kind::Op(op.1))
        };
        Ok(Item {
            offset: at,
            len,
            kind,
        })
    }

    /// Splits the run `at..end` into plain text and escapes (`ui/printf.rs` `text_out`).
    fn run_parts(&self, at: u32, end: u32) -> Run {
        let mut parts = Vec::new();
        let mut i = at;
        while i < end {
            let b = self.b[i as usize];
            let part = match Escape::start(b) {
                Some(esc) => {
                    let arg = (i + 1 < end)
                        .then(|| self.b[i as usize + 1])
                        .filter(|&c| Escape::start(c).is_none());
                    match (esc, arg) {
                        (Escape::Str(_), Some(c)) => (2, Some(Escape::Str(Some(ops::var_index(c))))),
                        (Escape::Int(_), Some(c)) => (2, Some(Escape::Int(Some(ops::var_index(c))))),
                        (Escape::Glyph(_), Some(c)) => (2, Some(Escape::Glyph(Some(c)))),
                        (esc, _) => (1, Some(esc)),
                    }
                }
                None => {
                    let n = (i..end)
                        .find(|&j| Escape::start(self.b[j as usize]).is_some())
                        .unwrap_or(end);
                    (n - i, None)
                }
            };
            parts.push(Part {
                offset: i,
                len: part.0,
                escape: part.1,
            });
            i += part.0;
        }
        Run { parts }
    }

    /// `ef kw , kw … f6` (`run.rs` `keywords`).
    fn keywords(&self, at: u32) -> Result<(u32, Kind), Refusal> {
        let mut spans = Vec::new();
        let mut start = at + 1;
        let mut i = at + 1;
        loop {
            let b = self.byte(i)?;
            if b == b',' || b == ops::ANSWER {
                spans.push(Span {
                    offset: start,
                    len: i - start,
                });
                start = i + 1;
                if b == ops::ANSWER {
                    return Ok((i + 1 - at, Kind::Keywords(spans)));
                }
            }
            i += 1;
        }
    }

    /// The input loop's arms (`start.rs` `input_loop`); `f1` / `f2` are the header's markers.
    fn input(&self, at: u32, b: u8) -> Result<(u32, Kind), Refusal> {
        let tagged = |tag: u8| -> Result<u8, Refusal> {
            let var = self.byte(at + 1)?;
            if self.byte(at + 2)? != tag {
                return refuse(at, Reason::Malformed(b));
            }
            Ok(var)
        };
        Ok(match b {
            ops::LOOK => (1, Kind::Marker(Marker::Look)),
            ops::GREETING => (1, Kind::Marker(Marker::Greeting)),
            0xf7 => (1, Kind::Input(Input::Ask)),
            0xf8 => {
                let mut i = at + 1;
                while self.byte(i)? != ops::KEYWORDS {
                    i += 1;
                }
                let keys = Span {
                    offset: at + 1,
                    len: i - at - 1,
                };
                (i - at, Kind::Input(Input::Keys(keys)))
            }
            0xf9 => (
                3,
                Kind::Input(Input::Line {
                    slot: tagged(ops::STR_TAG)?,
                }),
            ),
            0xfa => (1, Kind::Input(Input::Key)),
            0xfb => (
                3,
                Kind::Input(Input::Number {
                    var: tagged(ops::INT_TAG)?,
                }),
            ),
            0xfc => (
                3,
                Kind::Input(Input::Digit {
                    var: tagged(ops::INT_TAG)?,
                }),
            ),
            ops::HEADER => (1, Kind::Stop(Stop::End)),
            _ => (1, Kind::Stop(Stop::Error(b))),
        })
    }

    /// An expression from `at` up to and including its `a7` / `a8` (`eval.rs` `eval`).
    fn expr(&self, at: u32) -> Result<Expr, Refusal> {
        let mut tokens = Vec::new();
        let mut i = at;
        loop {
            let b = self.byte(i)?;
            if b == ops::END || b == ops::END_ASSIGN {
                return Ok(Expr {
                    offset: at,
                    len: i + 1 - at,
                    tokens,
                    end: b,
                });
            }
            let kind = match b {
                ops::LIT32 => TokenKind::Lit32(self.le32(i + 1)?),
                ops::LIT8 => TokenKind::Lit8(self.byte(i + 1)?),
                ops::LIT16 => TokenKind::Lit16(self.le16(i + 1)?),
                _ if ops::expr_op(b).is_some() => TokenKind::Op(b),
                _ => TokenKind::Value(b),
            };
            tokens.push(Token { offset: i, kind });
            i += kind.len();
        }
    }

    /// A table operand after its `d2` (`eval.rs` `table_element`): the address, then `b1` or
    /// an index expression.
    fn table_ref(&self, at: u32) -> Result<(TableRef, u32), Refusal> {
        let addr = self.le32(at)?;
        if self.byte(at + 4)? == ops::TABLE_FIRST {
            return Ok((TableRef { addr, index: None }, at + 5));
        }
        let index = self.expr(at + 4)?;
        let end = index.offset + index.len;
        Ok((
            TableRef {
                addr,
                index: Some(index),
            },
            end,
        ))
    }

    /// A statement op at `at` (`exec.rs` `exec`): its length and decoded operands.
    fn op(&self, at: u32, code: u8) -> Result<(u32, Op), Refusal> {
        let Some(info) = ops::statement_op(code) else {
            return refuse(at, Reason::UnknownOp(code));
        };
        let malformed = || refuse(at, Reason::Malformed(code));
        let (end, operands) = match info.shape {
            Shape::Args(args) => {
                let mut i = at + 1;
                let mut v = Vec::new();
                for a in args {
                    let expr = self.expr(i)?;
                    i = expr.offset + expr.len;
                    v.push(Operand {
                        actor: *a == Arg::Actor,
                        expr,
                    });
                }
                (i, Operands::Args(v))
            }
            Shape::Jump => {
                let raw = self.le32(at + 1)?;
                let target = i32::from(raw as u16 as i16);
                if target < 0 || target as u32 >= self.len() {
                    return refuse(at, Reason::JumpOutside);
                }
                (
                    at + 5,
                    Operands::Jump {
                        target: target as u32,
                    },
                )
            }
            Shape::Print => match self.byte(at + 1)? {
                ops::LIT32 => {
                    let (t, end) = self.table_ref(at + 2)?;
                    (end, Operands::Print(Print::Table(t)))
                }
                ops::PRINT_NOTHING => (at + 2, Operands::Print(Print::Nothing)),
                kind => {
                    let slot = self.byte(at + 2)?;
                    if self.byte(at + 3)? != ops::STR_TAG {
                        return malformed();
                    }
                    let skipped = self.byte(at + 4)?;
                    (
                        at + 5,
                        Operands::Print(Print::Slot {
                            kind,
                            slot,
                            skipped,
                        }),
                    )
                }
            },
            Shape::Assign => self.assign(at, code)?,
        };
        Ok((end - at, Op { code, operands }))
    }

    /// `a6` (`exec.rs` `assign`).
    fn assign(&self, at: u32, code: u8) -> Result<(u32, Operands), Refusal> {
        let malformed = || refuse(at, Reason::Malformed(code));
        let first = self.byte(at + 1)?;
        if first == ops::LIT32 {
            // The word element's index expression ends in the `a8` that separates the value
            // (`table_element` returns its position minus one); after `b1` the `a8` follows.
            let (table, mut end) = self.table_ref(at + 2)?;
            match &table.index {
                None => {
                    if self.byte(end)? != ops::END_ASSIGN {
                        return malformed();
                    }
                    end += 1;
                }
                Some(e) if e.end != ops::END_ASSIGN => return malformed(),
                Some(_) => {}
            }
            let value = self.expr(end)?;
            let end = value.offset + value.len;
            return Ok((end, Operands::Assign(Assign::TableWord { table, value })));
        }
        let (var, kind) = (first, self.byte(at + 2)?);
        if self.byte(at + 3)? != ops::END_ASSIGN || kind == 0 {
            return malformed();
        }
        if kind == ops::INT_TAG {
            let value = self.expr(at + 4)?;
            let end = value.offset + value.len;
            return Ok((end, Operands::Assign(Assign::Int { var, value })));
        }
        if self.byte(at + 4)? == ops::LIT32 {
            let (table, end) = self.table_ref(at + 5)?;
            return Ok((
                end,
                Operands::Assign(Assign::StrFromTable { var, kind, table }),
            ));
        }
        let src = self.byte(at + 4)?;
        if self.byte(at + 5)? != ops::STR_TAG {
            return malformed();
        }
        Ok((
            at + 6,
            Operands::Assign(Assign::StrFromSlot { var, kind, src }),
        ))
    }

    // ---- tables ------------------------------------------------------------------------------

    /// The table uses of the decoded ops.
    fn table_uses(&mut self) -> Result<Vec<TableUse>, Refusal> {
        let mut uses = Vec::new();
        let mut under = Vec::new();
        for item in self.items.values() {
            let Kind::Op(op) = &item.kind else { continue };
            let mut exprs: Vec<&Expr> = Vec::new();
            let mut tables: Vec<(&TableRef, bool)> = Vec::new();
            match &op.operands {
                Operands::Args(v) => exprs.extend(v.iter().map(|o| &o.expr)),
                Operands::Jump { .. } | Operands::Print(_) => {}
                Operands::Assign(Assign::TableWord { table, value }) => {
                    tables.push((table, true));
                    exprs.push(value);
                }
                Operands::Assign(Assign::Int { value, .. }) => exprs.push(value),
                Operands::Assign(Assign::StrFromTable { table, .. }) => tables.push((table, false)),
                Operands::Assign(Assign::StrFromSlot { .. }) => {}
            }
            if let Operands::Print(Print::Table(table)) = &op.operands {
                tables.push((table, false));
            }
            for (t, words) in tables {
                uses.push(TableUse {
                    addr: t.addr,
                    words,
                    index: t.index.as_ref().map_or(Some(0), Expr::constant),
                    at: item.offset,
                });
                exprs.extend(t.index.as_ref());
            }
            for e in exprs {
                expr_tables(e, item.offset, &mut uses, &mut under)?;
            }
        }
        for at in under {
            self.hazard(at, HazardKind::Underflow);
        }
        Ok(uses)
    }

    /// Claims the tables the decoded ops address that no table owns yet (see the module doc):
    /// one table item per address, up to the next address, the next decoded byte or the
    /// script's end. An address inside a table must be one of its entries.
    fn claim_tables(&mut self) -> Result<(), Refusal> {
        let uses = self.table_uses()?;
        let mut by_addr: BTreeMap<u32, Vec<TableUse>> = BTreeMap::new();
        for u in uses {
            by_addr.entry(u.addr).or_default().push(u);
        }
        let addrs: Vec<u32> = by_addr.keys().copied().collect();
        for (n, (&addr, uses)) in by_addr.iter().enumerate() {
            let at = uses[0].at;
            if addr >= self.len() {
                return refuse(at, Reason::TableOutside);
            }
            let words = uses[0].words;
            if uses.iter().any(|u| u.words != words) {
                return refuse(at, Reason::TableKinds);
            }
            match self.owner[addr as usize] {
                NO_OWNER => {}
                o => match &self.items[&o].kind {
                    Kind::Table(t)
                        if t.words == words
                            && (o == addr || t.entries.iter().any(|e| e.offset == addr)) =>
                    {
                        continue;
                    }
                    Kind::Table(_) => return refuse(at, Reason::TableKinds),
                    _ => return refuse(at, Reason::TableInCode),
                },
            }
            let next = addrs.get(n + 1).copied().unwrap_or(self.len());
            let owned = (addr..next)
                .find(|&p| self.owner[p as usize] != NO_OWNER)
                .unwrap_or(next);
            let table = self.table_at(addr, owned, words);
            let end = table
                .entries
                .last()
                .map_or(addr, |e| e.end() + u32::from(!words))
                + u32::from(table.terminated);
            if end > addr {
                self.insert(Item {
                    offset: addr,
                    len: end - addr,
                    kind: Kind::Table(table),
                })?;
            }
        }
        Ok(())
    }

    /// A constant index may run on into the tables that follow its table without a gap; past
    /// them it refuses the script.
    fn check_table_indices(&mut self) -> Result<(), Refusal> {
        for u in &self.table_uses()? {
            let Some(index) = u.index else { continue };
            let mut count = 0usize;
            let mut at = match self.owner.get(u.addr as usize) {
                Some(&o) if o != NO_OWNER => o,
                _ => return refuse(u.at, Reason::TableIndex),
            };
            let mut skip = true;
            while let Some(Item {
                kind: Kind::Table(t),
                len,
                ..
            }) = self.items.get(&at)
            {
                if t.words != u.words {
                    break;
                }
                // Entries before the address are not this use's.
                count += t
                    .entries
                    .iter()
                    .filter(|e| !skip || e.offset >= u.addr)
                    .count();
                skip = false;
                if t.terminated {
                    break;
                }
                at += len;
            }
            if index as usize >= count {
                return refuse(u.at, Reason::TableIndex);
            }
        }
        Ok(())
    }

    /// The entries of a table at `addr` that ends before `limit`.
    fn table_at(&self, addr: u32, limit: u32, words: bool) -> Table {
        let mut entries = Vec::new();
        let mut i = addr;
        let mut terminated = false;
        while i < limit {
            if self.b[i as usize] == ops::TABLE_END {
                terminated = true;
                break;
            }
            if words {
                if i + 2 > limit {
                    break;
                }
                entries.push(Span {
                    offset: i,
                    len: 2,
                });
                i += 2;
            } else {
                let Some(nul) = (i..limit).find(|&j| self.b[j as usize] == 0) else {
                    break;
                };
                entries.push(Span {
                    offset: i,
                    len: nul - i,
                });
                i = nul + 1;
            }
        }
        Table {
            words,
            entries,
            terminated,
        }
    }

    /// The items in offset order, the bytes no item owns as [`Kind::Unreached`].
    fn finish(&mut self) -> Vec<Item> {
        let mut out = Vec::with_capacity(self.items.len());
        let mut at = 0;
        let items = std::mem::take(&mut self.items);
        for (s, item) in items {
            if s > at {
                out.push(Item {
                    offset: at,
                    len: s - at,
                    kind: Kind::Unreached,
                });
            }
            at = item.end();
            out.push(item);
        }
        if self.len() > at {
            out.push(Item {
                offset: at,
                len: self.len() - at,
                kind: Kind::Unreached,
            });
        }
        out
    }
}

/// The table uses of an expression: the address operand of `b4` (words), `b7` (strings) and
/// `c3` (words), found by running the evaluator's ten-slot stack on the tokens (`eval.rs`
/// `Stack`: a read below it gives 0, a write there is dropped). `under` gets each op that reads
/// below the stack.
fn expr_tables(
    e: &Expr,
    at: u32,
    uses: &mut Vec<TableUse>,
    under: &mut Vec<u32>,
) -> Result<(), Refusal> {
    const SLOTS: i32 = 10;
    // `None` = not a constant; outside 0..SLOTS = below / above the array.
    let mut v: [Option<u32>; SLOTS as usize] = [None; SLOTS as usize];
    let mut sp: i32 = 0;
    let get = |v: &[Option<u32>; SLOTS as usize], i: i32| -> Option<Option<u32>> {
        usize::try_from(i).ok().and_then(|i| v.get(i)).copied()
    };
    let set = |v: &mut [Option<u32>; SLOTS as usize], i: i32, x: Option<u32>| {
        if let Some(s) = usize::try_from(i).ok().and_then(|i| v.get_mut(i)) {
            *s = x;
        }
    };
    for t in &e.tokens {
        match t.kind {
            TokenKind::Op(code) => {
                let arity = ops::expr_op(code).map_or(0, |o| i32::from(o.arity));
                if sp - arity < 0 {
                    under.push(t.offset);
                }
                // a = the slot two below the op (the address), b = the one below it. Below the
                // stack there is no address (the evaluator reads 0, the script's start).
                if matches!(code, 0xb4 | 0xb7 | 0xc3)
                    && let Some(a) = get(&v, sp - 2)
                {
                    let Some(addr) = a else {
                        return refuse(t.offset, Reason::TableNotLiteral);
                    };
                    let index = get(&v, sp - 1).flatten();
                    uses.push(TableUse {
                        addr,
                        words: code != 0xb7,
                        // `b7` / `c3` search the whole table; `b4` reads entry `b`.
                        index: if code == 0xb4 { index } else { None },
                        at,
                    });
                }
                if arity == 0 {
                    set(&mut v, sp, None);
                } else {
                    set(&mut v, sp - arity, None);
                    sp -= arity;
                }
            }
            k => set(&mut v, sp, k.value()),
        }
        sp += 1;
    }
    Ok(())
}

//! The decoder behind [`super::disassemble`]: the control-flow walk, the items, the tables.

use std::collections::{BTreeMap, BTreeSet};

use super::Escape;
use super::ops::{self, Arg, Flow, Shape};
use super::{
    Assign, Expr, Hazard, HazardKind, Input, Item, Kind, Marker, Op, Operand, Operands, Part,
    Print, Reason, Refusal, Run, Scan, Span, Stop, Table, TableRef, Token, TokenKind,
};

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

/// `Decoder::owner` of a byte no item holds yet.
const NO_OWNER: u32 = u32::MAX;

/// A table use the ops make: its kind and the constant index (`None` = any).
#[derive(Clone, Copy, Debug)]
pub(super) struct TableUse {
    pub(super) addr: u32,
    words: bool,
    index: Option<u32>,
    at: u32,
}

#[derive(Clone)]
pub(super) struct Decoder<'a> {
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
    pub(super) hazards: Vec<Hazard>,
    pub(super) run_splits: Vec<u32>,
}

fn refuse<T>(at: u32, reason: Reason) -> Result<T, Refusal> {
    Err(Refusal { at, reason })
}

impl<'a> Decoder<'a> {
    pub(super) fn new(b: &'a [u8], reserved: BTreeSet<u32>) -> Self {
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
            run_splits: Vec::new(),
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

    pub(super) fn run(&mut self) -> Result<(), Refusal> {
        let len = self.len();
        if len < 2 || self.b[0] != ops::HEADER {
            return refuse(0, Reason::NoHeader);
        }
        let Some(look) = self.first(0, ops::LOOK) else {
            return refuse(0, Reason::NoLookMarker);
        };
        if self.first(look, ops::GREETING).is_none() {
            return refuse(look, Reason::NoGreetingMarker);
        }
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
        self.drain()?;
        // Skip-loop landings come after the direct paths, so that a landing never decodes
        // bytes a direct path owns; the loops that step over operands first, in script order.
        // A loop may stop on a byte inside an operand, so a landing's path is decoded on trial:
        // when it would refuse the script, it is dropped as a hazard and its bytes stay
        // unreached unless another path reads them.
        while let Some(i) = (0..self.scans.len()).min_by_key(|&i| {
            let (l, _) = self.scans[i];
            (l.scan, l.stop)
        }) {
            let (landing, ctx) = self.scans.remove(i);
            let saved = self.clone();
            if let Err(r) = self.land(landing, ctx).and_then(|()| self.drain()) {
                *self = saved;
                self.hazard(landing.at, HazardKind::Dropped(landing.scan, r.reason));
            }
        }
        if !self.landed.iter().any(|l| l.scan == Scan::Greeting) {
            return refuse(look, Reason::NoGreetingMarker);
        }
        self.check_landings();
        Ok(())
    }

    /// Follows every queued path.
    fn drain(&mut self) -> Result<(), Refusal> {
        while let Some((at, ctx)) = self.work.pop() {
            self.visit(at, ctx)?;
        }
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
                let end = item.end();
                let text = matches!(item.kind, Kind::Text(_));
                let start = self.insert(item)?;
                // A run that stops at a run decoded earlier ends where a path entered.
                if text
                    && self.b.get(end as usize).is_some_and(|&b| ops::is_text(b))
                    && self.owner[end as usize] == end
                {
                    self.run_splits.push(end);
                }
                start
            }
            s => s,
        };
        if start != at {
            if !matches!(self.items[&start].kind, Kind::Text(_)) {
                return refuse(at, Reason::Overlap);
            }
            self.split(start, at);
        }
        self.follow(at, ctx)
    }

    /// A path enters the text run at `start` at `at`: the run becomes two, so that every run
    /// the interpreter reads is read from its start (the paths that read the first part go on
    /// into the second).
    fn split(&mut self, start: u32, at: u32) {
        let end = self.items[&start].end();
        self.items.remove(&start);
        for (s, e) in [(start, at), (at, end)] {
            self.items.insert(
                s,
                Item {
                    offset: s,
                    len: e - s,
                    kind: Kind::Text(self.run_parts(s, e)),
                },
            );
        }
        for p in at..end {
            self.owner[p as usize] = at;
        }
        self.run_splits.push(at);
        for ctx in [Ctx::Look, Ctx::Main] {
            if self.seen.contains(&(start, ctx)) {
                self.work.push((at, ctx));
            }
        }
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
        let o = self.owner[l.at as usize];
        let into_run = o != NO_OWNER && matches!(self.items[&o].kind, Kind::Text(_));
        if o == NO_OWNER || o == l.at || into_run {
            self.work.push((l.at, ctx));
        }
        Ok(())
    }

    /// After decoding: every skip loop must stop on the first byte of a code item and land on
    /// an item's start.
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
            let at_ok = self.owner.get(l.at as usize).is_none_or(|&o| o == l.at);
            if !stop_ok || !at_ok {
                self.hazard(l.at, HazardKind::Scan(l.scan));
            }
        }
        self.landed = landed;
    }

    /// Adds a decoded item. Returns the item's start.
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
        if let Some(p) = (s..e).find(|&p| self.owner[p as usize] != NO_OWNER) {
            return refuse(p, Reason::Overlap);
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
            // Up to the next non-text byte, or the start of a run a path entered before (the
            // run was split there) or of a table.
            let end = (at..self.len())
                .find(|&i| {
                    !ops::is_text(self.b[i as usize])
                        || self.owner[i as usize] != NO_OWNER
                        || self.reserved.contains(&i)
                })
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
                        (Escape::Str(_), Some(c)) => {
                            (2, Some(Escape::Str(Some(ops::var_index(c)))))
                        }
                        (Escape::Int(_), Some(c)) => {
                            (2, Some(Escape::Int(Some(ops::var_index(c)))))
                        }
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
    pub(super) fn expr(&self, at: u32) -> Result<Expr, Refusal> {
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
            i += kind.width();
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
    pub(super) fn op(&self, at: u32, code: u8) -> Result<(u32, Op), Refusal> {
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
    pub(super) fn table_uses(&mut self) -> Result<Vec<TableUse>, Refusal> {
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
    pub(super) fn claim_tables(&mut self) -> Result<(), Refusal> {
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
    pub(super) fn check_table_indices(&mut self) -> Result<(), Refusal> {
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
                entries.push(Span { offset: i, len: 2 });
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
    pub(super) fn finish(&mut self) -> Vec<Item> {
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

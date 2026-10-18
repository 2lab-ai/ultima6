//! The decoder behind [`super::disassemble`]: the control-flow walk, the items, the tables.

use std::collections::{BTreeMap, BTreeSet};

use super::Escape;
use super::ops::{self, Arg, Flow, Shape};
use super::{
    Assign, Expr, Hazard, HazardKind, Input, Item, Kind, Marker, Op, Operand, Operands, Part,
    Print, Reason, Refusal, Run, Scan, Span, Stop, Table, TableRef, Token, TokenKind, reachable,
};

/// The interpreter's two kinds of read position: inside the look text (a stop byte moves to
/// the greeting) and anywhere else (a stop byte goes to the input loop).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Ctx {
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

/// A skip-loop landing as the rounds name it: the loop, the byte it stopped on, the context.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct LandingKey {
    scan: Scan,
    stop: u32,
    ctx: Ctx,
}

/// The path an item or a table use comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    /// The paths that take no skip loop (decoded first, never on trial).
    Direct,
    /// The path after one skip-loop landing (decoded on trial).
    Landing(LandingKey),
}

/// What a decode round hands the next one (see [`super::disassemble`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Plan {
    /// The bytes of tables a kept path names that another landing's code covered: closed to
    /// the landing paths from the round's start, so that the table wins over that code.
    reserved: BTreeSet<u32>,
    /// Bytes taken back out of `reserved` because no table of a kept path holds them any more:
    /// a table can no longer win over landing code there.
    settled: BTreeSet<u32>,
    /// Landings whose path fails a check only the whole decode can make (a constant table
    /// index past its entries, its own code inside a table it names): not decoded, a
    /// [`HazardKind::Dropped`] hazard.
    banned: BTreeMap<LandingKey, Reason>,
}

#[cfg(test)]
thread_local! {
    /// Test-only: take the skip-loop landings in the reverse order (the result must not
    /// change, see the module doc).
    pub(super) static REVERSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `Decoder::owner` of a byte no item holds yet.
const NO_OWNER: u32 = u32::MAX;

/// A constant index that reaches no entry of its table (a word index whose move is negative).
const NO_ENTRY: u32 = u32::MAX;

/// A table use an op makes: its kind and the entry a constant index reads (`None` = any).
#[derive(Clone, Copy, Debug)]
pub(super) struct TableUse {
    pub(super) addr: u32,
    pub(super) words: bool,
    pub(super) index: Option<u32>,
    /// The op's offset.
    pub(super) at: u32,
}

/// The interpreter's read position for a 32-bit script address: its low word, sign-extended
/// (`set_pos` keeps a word, `pos` reads it signed). A negative one comes out at or above
/// 0xffff8000, outside any script.
pub(super) fn read_position(v: u32) -> u32 {
    i32::from(v as u16 as i16) as u32
}

/// Entry `v` of a string table as `table_element` reads it: the index is a word (`as i16`) and
/// a negative one skips no string (the first entry).
pub(super) fn string_entry(v: u32) -> u32 {
    (v as u16 as i16).max(0) as u32
}

/// Entry `v` of a word table as `table_element` reads it: the read position moves by
/// `index * 2` in word arithmetic; a negative move reaches no entry ([`NO_ENTRY`]).
pub(super) fn word_entry(v: u32) -> u32 {
    let off = (v as u16 as i16).wrapping_mul(2);
    if off < 0 { NO_ENTRY } else { off as u32 / 2 }
}

/// The entry a table operand (`d2 <table> (b1 | <index>)`) reads: `b1` the first, a constant
/// index as [`string_entry`] / [`word_entry`] compute it, `None` for a computed index.
pub(super) fn ref_entry(t: &TableRef, words: bool) -> Option<u32> {
    match &t.index {
        None => Some(0),
        Some(e) => e.constant().map(|v| {
            if words {
                word_entry(v)
            } else {
                string_entry(v)
            }
        }),
    }
}

#[derive(Clone)]
pub(super) struct Decoder<'a> {
    b: &'a [u8],
    plan: &'a Plan,
    /// Table addresses: a landing's path stops there, a direct path there refuses the script.
    reserved: BTreeSet<u32>,
    /// The table uses of the direct paths and of the kept landings.
    uses: Vec<(TableUse, Origin)>,
    /// The kind each used address is read as (`true` = words).
    kinds: BTreeMap<u32, bool>,
    /// The addresses the path decoding now named.
    named: BTreeSet<u32>,
    /// The path decoding now.
    phase: Origin,
    items: BTreeMap<u32, Item>,
    /// The path that decoded each item (by its start).
    origins: BTreeMap<u32, Origin>,
    /// For each byte, the start of the item that owns it.
    owner: Vec<u32>,
    seen: BTreeSet<(u32, Ctx)>,
    /// The flow of every path: where reading goes on after the item read at a position, in a
    /// context (a path that joins code another path read goes on along these).
    edges: BTreeMap<(u32, Ctx), Vec<(u32, Ctx)>>,
    /// Where each kept landing's path starts: the byte its skip loop stopped on.
    starts: BTreeMap<LandingKey, (u32, Ctx)>,
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
    pub(super) fn new(b: &'a [u8], plan: &'a Plan) -> Self {
        Self {
            b,
            plan,
            reserved: plan.reserved.clone(),
            uses: Vec::new(),
            kinds: BTreeMap::new(),
            named: BTreeSet::new(),
            phase: Origin::Direct,
            items: BTreeMap::new(),
            origins: BTreeMap::new(),
            owner: vec![NO_OWNER; b.len()],
            seen: BTreeSet::new(),
            edges: BTreeMap::new(),
            starts: BTreeMap::new(),
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
        self.scan_greeting(0, None)?;
        self.drain()?;
        // Skip-loop landings come after the direct paths, so that a landing never decodes
        // bytes a direct path owns; the loops that step over operands first, in script order.
        // A loop may stop on a byte inside an operand, so a landing's path is decoded on trial,
        // its table uses included: when it would refuse the script, it is dropped as a hazard
        // and its bytes and tables are as if it had never been read. Each trial keeps a copy of
        // the decoder to go back to: one copy per landing, O(len²) per round, small for scripts
        // of at most `SCRIPT_LEN` bytes.
        // A trial that fails is tried again once the landings after it are in: a table they
        // keep may stop its path before the bytes that failed it (so which comes first does
        // not decide it). A landing that still fails, with nothing more kept, is dropped.
        let mut failed: Vec<(Landing, Ctx, Reason)> = Vec::new();
        loop {
            while let Some(i) = self.next_landing() {
                let (landing, ctx) = self.scans.remove(i);
                if let Err(r) = self.try_landing(landing, ctx) {
                    failed.push((landing, ctx, r));
                }
            }
            let before = failed.len();
            for (landing, ctx, _) in std::mem::take(&mut failed) {
                if let Err(r) = self.try_landing(landing, ctx) {
                    failed.push((landing, ctx, r));
                }
            }
            if failed.len() == before {
                break;
            }
        }
        for (landing, _, r) in failed {
            self.hazard(landing.at, HazardKind::Dropped(landing.scan, r));
        }
        if !self.landed.iter().any(|l| l.scan == Scan::Greeting) {
            return refuse(look, Reason::NoGreetingMarker);
        }
        self.check_landings();
        Ok(())
    }

    /// Decodes the path after `landing` on trial; on a refusal the decoder is as it was, and
    /// the reason comes back. A banned landing is not decoded.
    fn try_landing(&mut self, landing: Landing, ctx: Ctx) -> Result<(), Reason> {
        let key = LandingKey {
            scan: landing.scan,
            stop: landing.stop,
            ctx,
        };
        if let Some(&r) = self.plan.banned.get(&key) {
            return Err(r);
        }
        let saved = self.clone();
        self.phase = Origin::Landing(key);
        self.named.clear();
        self.starts.insert(key, (landing.stop, ctx));
        let tried = self.land(landing, ctx).and_then(|()| self.drain());
        if let Err(r) = tried {
            *self = saved;
            return Err(r.reason);
        }
        Ok(())
    }

    /// The landing to try next: the loops that step over operands first, in script order (a
    /// test can take them the other way round, [`REVERSED`]).
    fn next_landing(&self) -> Option<usize> {
        let key = |i: &usize| {
            let (l, _) = self.scans[*i];
            (l.scan, l.stop)
        };
        #[cfg(test)]
        if REVERSED.with(std::cell::Cell::get) {
            return (0..self.scans.len()).max_by_key(key);
        }
        (0..self.scans.len()).min_by_key(key)
    }

    /// Queues `to` after the item read at `from`, and keeps the edge (a path's flow).
    fn queue(&mut self, from: (u32, Ctx), to: (u32, Ctx)) {
        self.edges.entry(from).or_default().push(to);
        self.work.push(to);
    }

    /// The bytes the path of landing `k` reads as code: every position its flow reaches, with
    /// the code it joins (read first by another path), and the items there.
    fn reach(&self, k: LandingKey) -> BTreeSet<u32> {
        let mut bytes = BTreeSet::new();
        let mut done = BTreeSet::new();
        let mut todo: Vec<(u32, Ctx)> = self.starts.get(&k).copied().into_iter().collect();
        while let Some(n) = todo.pop() {
            if !done.insert(n) {
                continue;
            }
            let at = n.0;
            bytes.insert(at);
            if self.owner.get(at as usize) == Some(&at)
                && let Some(it) = self.items.get(&at)
            {
                bytes.extend(at..it.end());
            }
            todo.extend(self.edges.get(&n).into_iter().flatten());
        }
        bytes
    }

    /// Follows every queued path.
    fn drain(&mut self) -> Result<(), Refusal> {
        while let Some((at, ctx)) = self.work.pop() {
            self.visit(at, ctx)?;
        }
        Ok(())
    }

    /// A path reaches the table address `addr` (a landing at `at`). A direct path there, or a
    /// landing's path at a table it named itself, reads one byte as code and as data: the
    /// script (or the trial) is refused. Any other landing's path stops there.
    fn reach_table(&mut self, addr: u32, at: u32) -> Result<(), Refusal> {
        if self.phase == Origin::Direct || self.named.contains(&addr) {
            return refuse(addr, Reason::TableInCode);
        }
        self.hazard(at, HazardKind::IntoData);
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
            return self.reach_table(at, at);
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
        if let Some(&o) = self.origins.get(&start) {
            self.origins.insert(at, o);
        }
        for p in at..end {
            self.owner[p as usize] = at;
        }
        self.run_splits.push(at);
        for ctx in [Ctx::Look, Ctx::Main] {
            if self.seen.contains(&(start, ctx)) {
                self.queue((start, ctx), (at, ctx));
            }
        }
    }

    /// Queues the successors of the item at `at` read in `ctx`.
    fn follow(&mut self, at: u32, ctx: Ctx) -> Result<(), Refusal> {
        let item = &self.items[&at];
        let end = item.end();
        // A stop byte inside the look text: `run_text` returns and the read position moves
        // past the next `f2` (the stop byte itself is the first one the scan reads). A `b6`
        // there too: `run_text` returns after it, its read position back on the `b6`.
        let stops = match &item.kind {
            Kind::BlockEnd | Kind::Input(_) | Kind::Stop(_) | Kind::Marker(_) => true,
            Kind::Op(op) => op.info().flow == Flow::Bye,
            _ => false,
        };
        if stops && ctx == Ctx::Look {
            return self.scan_greeting(at, Some((at, ctx)));
        }
        match &item.kind {
            Kind::Text(_) | Kind::BlockEnd | Kind::Input(_) => self.queue((at, ctx), (end, ctx)),
            Kind::Op(op) => match (op.info().flow, &op.operands) {
                (Flow::Jump, Operands::Jump { target }) => self.queue((at, ctx), (*target, ctx)),
                (Flow::If, _) => {
                    self.queue((at, ctx), (end, ctx));
                    self.scan_ops(Scan::If, end, ctx);
                }
                (Flow::Else, _) => self.scan_ops(Scan::Else, end, ctx),
                (Flow::Bye, _) => {}
                _ => self.queue((at, ctx), (end, ctx)),
            },
            Kind::Keywords(_) => {
                self.queue((at, ctx), (end, ctx));
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
    /// so its landing is followed at once, as part of the path that reaches it (`after`: the
    /// item that stopped the look text), or a direct path.
    fn scan_greeting(&mut self, from: u32, after: Option<(u32, Ctx)>) -> Result<(), Refusal> {
        match self.first(from, ops::GREETING) {
            Some(stop) => {
                if let Some(a) = after {
                    self.edges.entry(a).or_default().push((stop, Ctx::Main));
                }
                self.land(
                    Landing {
                        scan: Scan::Greeting,
                        stop,
                        at: stop + 1,
                    },
                    Ctx::Main,
                )
            }
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
            return self.reach_table(l.stop, l.at);
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
            self.queue((l.stop, ctx), (l.at, ctx));
        }
        Ok(())
    }

    /// After decoding: every skip loop must stop on the first byte of a code item and land on
    /// an item's start. A landing that reached a table's bytes has its one hazard already
    /// ([`HazardKind::IntoData`], recorded where it lands): that names what the bytes are.
    fn check_landings(&mut self) {
        let landed = std::mem::take(&mut self.landed);
        for l in &landed {
            let into_data = Hazard {
                at: l.at,
                kind: HazardKind::IntoData,
            };
            if self.hazards.contains(&into_data) {
                continue;
            }
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

    /// Adds a decoded item and, for an op, its table uses. Returns the item's start.
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
        let uses = match &item.kind {
            Kind::Op(op) => {
                let (uses, stack) = op_tables(op, s)?;
                for h in stack {
                    self.hazard(h.at, h.kind);
                }
                uses
            }
            _ => Vec::new(),
        };
        self.items.insert(s, item);
        self.origins.insert(s, self.phase);
        for u in uses {
            self.add_use(u)?;
        }
        Ok(s)
    }

    /// Checks one table use of the path decoding now and closes its address to code. The
    /// address must be inside the script, read with one kind, and outside the code of the
    /// direct paths and of this path; inside an earlier landing's code the round's end decides
    /// ([`Self::revise`]), unless the address was settled.
    fn add_use(&mut self, u: TableUse) -> Result<(), Refusal> {
        if u.addr >= self.len() {
            return refuse(u.at, Reason::TableOutside);
        }
        if *self.kinds.entry(u.addr).or_insert(u.words) != u.words {
            return refuse(u.at, Reason::TableKinds);
        }
        match self.owner[u.addr as usize] {
            NO_OWNER => {}
            o => match self.origins.get(&o) {
                Some(&Origin::Landing(k))
                    if self.phase != Origin::Landing(k) && !self.plan.settled.contains(&u.addr) => {
                }
                _ => return refuse(u.at, Reason::TableInCode),
            },
        }
        self.reserved.insert(u.addr);
        self.named.insert(u.addr);
        self.uses.push((u, self.phase));
        Ok(())
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

    /// Splits the run `at..end` into plain text and escapes ([`run_parts`]).
    fn run_parts(&self, at: u32, end: u32) -> Run {
        run_parts(self.b, at, end)
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

    /// A table operand after its `d2` (`eval.rs` `table_element`): the address (as the read
    /// position it becomes), then `b1` or an index expression.
    fn table_ref(&self, at: u32) -> Result<(TableRef, u32), Refusal> {
        let addr = read_position(self.le32(at)?);
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
                let target = read_position(self.le32(at + 1)?);
                if target >= self.len() {
                    return refuse(at, Reason::JumpOutside);
                }
                (at + 5, Operands::Jump { target })
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

    /// The table each kept use's address takes, in address order: its entries from the address
    /// up to a `b8` at an entry's start, the next table address, the script's end or the first
    /// byte of direct code. A table without its `b8` there has no end of its own: it also ends
    /// at the first byte of landing code, but not before the last entry a constant index of a
    /// use reads, of this table or, through the tables before it that end where it starts (an
    /// index runs on into them, [`reachable`]), of an earlier one.
    fn table_ranges(&self) -> Vec<(u32, Table)> {
        // Per address: the kind and the entries its uses' constant indices need.
        let mut by_addr: BTreeMap<u32, (bool, usize)> = BTreeMap::new();
        for (u, _) in &self.uses {
            let e = by_addr.entry(u.addr).or_insert((u.words, 0));
            if let Some(i) = u.index.filter(|&i| i != NO_ENTRY) {
                e.1 = e.1.max(i as usize + 1);
            }
        }
        let addrs: Vec<u32> = by_addr.keys().copied().collect();
        let mut out = Vec::with_capacity(addrs.len());
        // The entries the tables before still need from this one: (where it must start, its
        // kind, how many).
        let mut carry: Option<(u32, bool, usize)> = None;
        for (n, (&addr, &(words, own))) in by_addr.iter().enumerate() {
            let next = addrs
                .get(n + 1)
                .copied()
                .unwrap_or(self.len())
                .min(self.len());
            let needed = match carry.take() {
                Some((at, w, k)) if at == addr && w == words => own.max(k),
                _ => own,
            };
            let direct = (addr..next).find(|&p| self.direct_at(p)).unwrap_or(next);
            let mut table = self.table_at(addr, direct, words);
            if !table.terminated {
                let owned = (addr..next)
                    .find(|&p| self.owner[p as usize] != NO_OWNER)
                    .unwrap_or(next);
                let cut = self.table_at(addr, owned, words).entries.len();
                table.entries.truncate(cut.max(needed));
                let short = needed.saturating_sub(table.entries.len());
                if short > 0 && !table.entries.is_empty() {
                    carry = Some((table_end(addr, &table), words, short));
                }
            }
            out.push((addr, table));
        }
        out
    }

    /// A byte of an item a direct path decoded.
    fn direct_at(&self, p: u32) -> bool {
        let o = self.owner[p as usize];
        o != NO_OWNER && self.origins.get(&o) == Some(&Origin::Direct)
    }

    /// The plan of the next round, when this one is not final (see the module doc). A landing
    /// that names a table and whose own path reads a byte of it as code (code it decoded or
    /// joined, [`Self::reach`]) is dropped. Any other landing's code inside a table makes the
    /// table's bytes closed to landings from the round's start (the table wins), or, on bytes
    /// settled before, drops the table's landing namers (the code of a direct table's intruder
    /// instead). Then reserved bytes no table holds any more are taken back and settled.
    /// `None` = final.
    pub(super) fn revise(&self) -> Option<Plan> {
        let mut p = self.plan.clone();
        let mut held = BTreeSet::new();
        for (addr, table) in self.table_ranges() {
            // A table with no entry still holds its address: landing code there is inside it.
            let end = table_end(addr, &table).max(addr + 1);
            held.extend(addr..end);
            let namers: BTreeSet<Origin> = self
                .uses
                .iter()
                .filter(|(u, _)| u.addr == addr)
                .map(|&(_, o)| o)
                .collect();
            let mut own = false;
            for o in &namers {
                if let &Origin::Landing(n) = o
                    && self.reach(n).range(addr..end).next().is_some()
                {
                    p.banned.entry(n).or_insert(Reason::TableInCode);
                    own = true;
                }
            }
            if own {
                // The table's other namers, if any, decide in the next round.
                continue;
            }
            // The items over the range: the one over its address, and those that start in it.
            let over = Some(self.owner[addr as usize]).filter(|&o| o != NO_OWNER && o < addr);
            let inside: Vec<u32> = over
                .into_iter()
                .chain(self.items.range(addr..end).map(|(&s, _)| s))
                .collect();
            for s in inside {
                let Some(&Origin::Landing(k)) = self.origins.get(&s) else {
                    continue;
                };
                let settled = (s..self.items[&s].end()).any(|b| self.plan.settled.contains(&b));
                if !settled {
                    let open = (addr..end).filter(|b| !self.plan.settled.contains(b));
                    p.reserved.extend(open);
                } else if namers.contains(&Origin::Direct) {
                    p.banned.entry(k).or_insert(Reason::TableInCode);
                } else {
                    for o in &namers {
                        if let Origin::Landing(n) = o {
                            p.banned.entry(*n).or_insert(Reason::TableInCode);
                        }
                    }
                }
            }
        }
        if p != *self.plan {
            return Some(p);
        }
        let stale: Vec<u32> = self.plan.reserved.difference(&held).copied().collect();
        if stale.is_empty() {
            return None;
        }
        for b in stale {
            p.reserved.remove(&b);
            p.settled.insert(b);
        }
        Some(p)
    }

    /// Claims the tables of the kept uses ([`Self::table_ranges`], see the module doc). Every
    /// address is checked first (inside the script, not in code), before any range is scanned;
    /// the uses were checked when they were made and the rounds keep code out of the ranges,
    /// so this refuses nothing in practice.
    pub(super) fn claim_tables(&mut self) -> Result<(), Refusal> {
        for (u, _) in &self.uses {
            match self.owner.get(u.addr as usize) {
                None => return refuse(u.at, Reason::TableOutside),
                Some(&NO_OWNER) => {}
                Some(_) => return refuse(u.at, Reason::TableInCode),
            }
        }
        for (addr, table) in self.table_ranges() {
            let end = table_end(addr, &table);
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

    /// A constant index may run on into the tables that follow its table without a gap
    /// ([`reachable`]); past them, a direct path's use refuses the script and a landing's use
    /// bans that landing for the next round. `None` = every index holds.
    pub(super) fn check_table_indices(&self) -> Result<Option<Plan>, Refusal> {
        let mut banned = BTreeMap::new();
        for (u, origin) in &self.uses {
            let Some(index) = u.index else { continue };
            let count = reachable(|a| self.items.get(&a), u.addr, u.words).len();
            if index as usize >= count {
                match origin {
                    Origin::Direct => return refuse(u.at, Reason::TableIndex),
                    Origin::Landing(k) => {
                        banned.insert(*k, Reason::TableIndex);
                    }
                }
            }
        }
        if banned.is_empty() {
            return Ok(None);
        }
        let mut p = self.plan.clone();
        p.banned.extend(banned);
        Ok(Some(p))
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

/// Splits the bytes `at..end` of `bytes` into plain text and escapes (`ui/printf.rs` `text_out`):
/// a run's parts, or those of a table string the escape layer prints.
pub(super) fn run_parts(bytes: &[u8], at: u32, end: u32) -> Run {
    let mut parts = Vec::new();
    let mut i = at;
    while i < end {
        let b = bytes[i as usize];
        let part = match Escape::start(b) {
            Some(esc) => {
                let arg = (i + 1 < end)
                    .then(|| bytes[i as usize + 1])
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
                    .find(|&j| Escape::start(bytes[j as usize]).is_some())
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

/// Where a table at `addr` with these entries ends (after the NUL of its last string, or its
/// `b8`).
fn table_end(addr: u32, t: &Table) -> u32 {
    t.entries
        .last()
        .map_or(addr, |e| e.end() + u32::from(!t.words))
        + u32::from(t.terminated)
}

/// The table uses of an op at `at` (the table operands of `b5` / `a6`, and the address operand
/// of the expression ops `b4`, `b7`, `c3`) and the stack hazards of its expressions.
fn op_tables(op: &Op, at: u32) -> Result<(Vec<TableUse>, Vec<Hazard>), Refusal> {
    let mut uses = Vec::new();
    let mut hazards = Vec::new();
    let mut exprs: Vec<&Expr> = Vec::new();
    let table = |t: &TableRef, words: bool, uses: &mut Vec<TableUse>| {
        uses.push(TableUse {
            addr: t.addr,
            words,
            index: ref_entry(t, words),
            at,
        });
    };
    match &op.operands {
        Operands::Args(v) => exprs.extend(v.iter().map(|o| &o.expr)),
        Operands::Jump { .. } | Operands::Print(Print::Nothing | Print::Slot { .. }) => {}
        Operands::Print(Print::Table(t))
        | Operands::Assign(Assign::StrFromTable { table: t, .. }) => {
            table(t, false, &mut uses);
            exprs.extend(t.index.as_ref());
        }
        Operands::Assign(Assign::TableWord { table: t, value }) => {
            table(t, true, &mut uses);
            exprs.extend(t.index.as_ref());
            exprs.push(value);
        }
        Operands::Assign(Assign::Int { value, .. }) => exprs.push(value),
        Operands::Assign(Assign::StrFromSlot { .. }) => {}
    }
    for e in exprs {
        expr_tables(e, at, &mut uses, &mut hazards)?;
    }
    Ok((uses, hazards))
}

/// The table uses of an expression: the address operand of `b4` (words), `b7` (strings) and
/// `c3` (words), found by running the evaluator's ten-slot stack on the tokens (`eval.rs`
/// `Stack`: a read outside it gives 0, a write there is dropped). `hazards` gets each op that
/// reads below the stack ([`HazardKind::Underflow`]) and each value put above it
/// ([`HazardKind::Overflow`]); an address operand outside the stack names no table.
fn expr_tables(
    e: &Expr,
    at: u32,
    uses: &mut Vec<TableUse>,
    hazards: &mut Vec<Hazard>,
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
    let mut hazard = |at: u32, kind: HazardKind| hazards.push(Hazard { at, kind });
    for t in &e.tokens {
        match t.kind {
            TokenKind::Op(code) => {
                let arity = ops::expr_op(code).map_or(0, |o| i32::from(o.arity));
                if arity > 0 && sp - arity < 0 {
                    hazard(t.offset, HazardKind::Underflow);
                }
                // a = the slot two below the op (the address), b = the one below it. Outside
                // the stack there is no address (the evaluator reads 0, the script's start).
                if matches!(code, 0xb4 | 0xb7 | 0xc3)
                    && let Some(a) = get(&v, sp - 2)
                {
                    let Some(addr) = a else {
                        return refuse(t.offset, Reason::TableNotLiteral);
                    };
                    let index = get(&v, sp - 1).flatten();
                    uses.push(TableUse {
                        addr: read_position(addr),
                        words: code != 0xb7,
                        // `b7` / `c3` search the whole table; `b4` reads entry `b as u8`.
                        index: if code == 0xb4 {
                            index.map(|i| u32::from(i as u8))
                        } else {
                            None
                        },
                        at,
                    });
                }
                // The op's value goes where its operands were (where it is, for `c0`).
                let result = if arity == 0 { sp } else { sp - arity };
                if result >= SLOTS {
                    hazard(t.offset, HazardKind::Overflow);
                }
                set(&mut v, result, None);
                sp = result;
            }
            k => {
                if sp >= SLOTS {
                    hazard(t.offset, HazardKind::Overflow);
                }
                set(&mut v, sp, k.value());
            }
        }
        sp += 1;
    }
    Ok(())
}

//! Test-only: what the interpreter reads and prints, recorded at its seams, and the check of
//! those reads against the disassembly (the execution cross-check).
//!
//! The hooks (`#[cfg(test)]` calls in `run.rs`, `exec.rs`, `eval.rs`, `start.rs`, `script.rs`,
//! `ui/print.rs`, `ui/keyread.rs` and `ui/target.rs`) do nothing until a test calls [`start`]
//! on its thread. They record:
//!
//! - every byte `run_text`'s loop reads (text, ops, `ef`, stop bytes), and every op byte the
//!   input loop reads;
//! - every table read: the string bytes `print_op`'s and `assign`'s `d2` forms read and the
//!   strings `b7` searches (each read marked print, copy or search; a print also takes the
//!   escape layer's state when it starts, so the check compares what it put out with its
//!   string filled in), the words `b4` and `c3` read, the `b8` that stops `b7` and `c3`,
//!   and the words `assign`'s `d2` form writes; and for `b4` / `b7` / `c3`, whether the table
//!   address came from inside the evaluator's stack (outside it the evaluator reads 0, the
//!   script's start, which the decoder marks with a stack hazard);
//! - every character handed to the line assembler (`put_raw`'s argument): what the game prints,
//!   before word wrap, spaces and newlines included, with -1 for a flush; and which of them a
//!   key wait put (its flush, `wait_event`, and the newline its cursor ends a full line with,
//!   `idle_poll`), not the escape layer;
//! - which script `load_script` put in the script buffer.
//!
//! With snapshots on, the first text byte of each contiguous read of text takes the escape
//! layer's state and the variables, so the check can compare the characters a visit put out
//! (from its first read to the read after it) with the runs filled in, character for character:
//! spaces and newlines included, as this is before word wrap, and what a key wait inside the
//! visit put left out.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::super::Cx;
use super::super::vars;
use super::decode::run_parts;
use super::ops::{self, Escape};
use super::{Disasm, HazardKind, Input, Kind, Part, Span, Stop};
use crate::state::State;
use crate::ui::text::{RUNIC, itoa};

/// One recorded read.
#[derive(Clone, Debug)]
pub(crate) enum Event {
    /// `load_script` loaded item `item` of `converse.<letter>` into the script buffer.
    Load { letter: u8, item: u16 },
    /// `run_text` read `byte` at `at`; `snap` at the first byte of a text visit.
    Run {
        at: i32,
        byte: u8,
        snap: Option<Box<Snap>>,
    },
    /// The input loop read the op `byte` at `at`.
    Input { at: i32, byte: u8 },
    /// A character `put_raw` was handed (negative = a flush); `wait` = by a key wait, not by the
    /// escape layer.
    Emit { c: i16, wait: bool },
    /// `print_op`'s `d2` form starts printing a table string; `snap` = the state then.
    PrintStart { snap: Option<Box<Snap>> },
    /// A string-table byte read; a 0 ends the string.
    Table { at: i32, byte: u8, read: Read },
    /// A table word read (by `b4` or `c3`), `at` its first byte.
    Word { at: i32, word: u16 },
    /// The `b8` that ends a table, read at an entry's start (by `b7` or `c3`).
    TableEnd { at: i32 },
    /// A table word written (by `assign`'s `d2` form).
    WordWrite { at: i32, word: u16 },
    /// `b4` / `b7` / `c3` took its table address from inside the evaluator's stack, or (not
    /// `inside`) read 0 outside it; its table reads follow.
    Address { inside: bool },
}

/// Who reads a table string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Read {
    /// `print_op` (`b5 d2`): each byte goes through the escape layer.
    Print,
    /// `assign` (`a6 … d2`): the string is copied into a string slot.
    Copy,
    /// `b7`: the string is matched against a slot's words.
    Search,
}

/// The escape layer and the variables at a visit's first byte.
#[derive(Clone, Debug)]
pub(crate) struct Snap {
    pub esc: EscState,
    pub strs: Vec<Vec<u8>>,
    pub ints: Vec<u16>,
}

/// The escape layer's state that decides what a run puts out (`ui/printf.rs` `text_out`).
#[derive(Clone, Debug)]
pub(crate) struct EscState {
    pub prefix: u8,
    pub hidden: bool,
    pub plural: bool,
    pub runic: bool,
    pub show_hidden: bool,
    pub keyword_end: Vec<u8>,
}

struct Trace {
    events: Vec<Event>,
    snapshots: bool,
    /// Offset of the last text byte `run_text` read, while a visit lasts.
    last_text: Option<i32>,
    /// The next `put_raw` call is a key wait's.
    wait: bool,
}

thread_local! {
    static TRACE: RefCell<Option<Trace>> = const { RefCell::new(None) };
}

/// Starts recording on this thread.
pub(crate) fn start(snapshots: bool) {
    TRACE.with(|t| {
        *t.borrow_mut() = Some(Trace {
            events: Vec::new(),
            snapshots,
            last_text: None,
            wait: false,
        });
    });
}

/// Stops recording and returns the reads.
pub(crate) fn stop() -> Vec<Event> {
    TRACE.with(|t| t.borrow_mut().take().map(|t| t.events).unwrap_or_default())
}

fn active() -> bool {
    TRACE.with(|t| t.borrow().is_some())
}

fn push(e: Event) {
    TRACE.with(|t| {
        if let Some(t) = t.borrow_mut().as_mut() {
            t.events.push(e);
        }
    });
}

fn snap(s: &State) -> Snap {
    let t = &s.ui.text;
    let tail = &s.actors.objlist_tail;
    Snap {
        esc: EscState {
            prefix: t.prefix,
            hidden: t.hidden,
            plural: t.plural,
            runic: t.win().flags & RUNIC != 0,
            show_hidden: tail.get(0x5e..0x60).is_some_and(|w| w != [0, 0]),
            keyword_end: s.ui.tables.keyword_end.clone(),
        },
        strs: (0..crate::converse::VAR_COUNT as u8)
            .map(|i| vars::string_var(s, i))
            .collect(),
        ints: (0..crate::converse::VAR_COUNT as u8)
            .map(|i| vars::int_var(s, i))
            .collect(),
    }
}

/// Hook: the top of `run_text`'s loop, `b` just read.
pub(crate) fn run_byte(cx: &Cx<'_>, b: u8) {
    if !active() {
        return;
    }
    let at = cx.cv(|c| c.pos()) - 1;
    let text = ops::is_text(b);
    let (want, last) = TRACE.with(|t| {
        let t = t.borrow();
        let t = t.as_ref().expect("active");
        (t.snapshots, t.last_text)
    });
    let first = text && last != Some(at - 1);
    let snap = (want && first).then(|| Box::new(snap(&cx.state.borrow())));
    TRACE.with(|t| {
        if let Some(t) = t.borrow_mut().as_mut() {
            t.last_text = text.then_some(at);
            t.events.push(Event::Run { at, byte: b, snap });
        }
    });
}

/// Hook: the input loop read the op `b`.
pub(crate) fn input_byte(cx: &Cx<'_>, b: u8) {
    if !active() {
        return;
    }
    let at = cx.cv(|c| c.pos()) - 1;
    TRACE.with(|t| {
        if let Some(t) = t.borrow_mut().as_mut() {
            t.last_text = None;
        }
    });
    push(Event::Input { at, byte: b });
}

/// Hook: `put_raw` was handed `c`.
pub(crate) fn emit(c: i16) {
    TRACE.with(|t| {
        if let Some(t) = t.borrow_mut().as_mut() {
            let wait = std::mem::take(&mut t.wait);
            t.events.push(Event::Emit { c, wait });
        }
    });
}

/// Hook: a key wait calls `put_raw` next (not for the escape layer).
pub(crate) fn wait_put() {
    TRACE.with(|t| {
        if let Some(t) = t.borrow_mut().as_mut() {
            t.wait = true;
        }
    });
}

/// Hook: a string-table byte `b` at `at` was read, by `read`.
pub(crate) fn table_byte(at: i32, b: u8, read: Read) {
    push(Event::Table { at, byte: b, read });
}

/// Hook: `print_op` starts printing a table string.
pub(crate) fn print_start(cx: &Cx<'_>) {
    if !active() {
        return;
    }
    let want = TRACE.with(|t| t.borrow().as_ref().is_some_and(|t| t.snapshots));
    let snap = want.then(|| Box::new(snap(&cx.state.borrow())));
    push(Event::PrintStart { snap });
}

/// Hook: the table word `word` at `at` was read.
pub(crate) fn table_word(at: i32, word: u16) {
    push(Event::Word { at, word });
}

/// Hook: a table's `b8` at `at` was read at an entry's start.
pub(crate) fn table_end(at: i32) {
    push(Event::TableEnd { at });
}

/// Hook: the table word `word` was written at `at`.
pub(crate) fn table_write(at: i32, word: u16) {
    push(Event::WordWrite { at, word });
}

/// Hook: `b4` / `b7` / `c3` took its table address from inside the stack (`inside`) or not.
pub(crate) fn table_address(inside: bool) {
    push(Event::Address { inside });
}

/// Hook: `load_script` filled the script buffer.
pub(crate) fn loaded(letter: u8, item: u16) {
    TRACE.with(|t| {
        if let Some(t) = t.borrow_mut().as_mut() {
            t.last_text = None;
        }
    });
    push(Event::Load { letter, item });
}

// ---- the cross-check -------------------------------------------------------------------------

/// The cross-check's counts. The first six count what was checked; the rest must be 0 for a
/// pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// Bytes checked (text bytes, op and stop bytes, input ops, table bytes and words).
    pub offsets: usize,
    /// Text visits whose output was compared.
    pub filled: usize,
    /// Table strings read (by `print_op`, `assign` and `b7`).
    pub table_strings: usize,
    /// Table prints whose output was compared.
    pub prints: usize,
    /// Table words read (by `b4` and `c3`).
    pub table_words: usize,
    /// Table ends read (by `b7` and `c3`).
    pub table_ends: usize,
    /// Table words written (by `assign`).
    pub table_writes: usize,
    /// `b4` / `b7` / `c3` whose address was outside the evaluator's stack (it reads from the
    /// script's start): each must be an op the decoder marked with a stack hazard, and its
    /// reads are checked against the script's bytes only.
    pub outside_reads: usize,
    /// Reads outside any item, or at an item of the wrong kind or not at its start.
    pub mismatches: usize,
    /// Bytes read that differ from the script's bytes (as the ops have written them).
    pub byte_mismatches: usize,
    /// Text reads that do not start at a run's start and end at a run's end.
    pub partial_visits: usize,
    /// Visits whose output is not the run with its escapes filled in.
    pub fill_mismatches: usize,
    /// Table prints whose output is not the string read with its escapes filled in.
    pub print_mismatches: usize,
    /// Table reads that are not one whole string of a string table, one word of a word table
    /// or a table's `b8`.
    pub table_mismatches: usize,
    /// Reads at or past the item's length (the buffer's leftover bytes).
    pub past_item: usize,
    /// Reads before any script was loaded, or of a script without a disassembly.
    pub unknown_script: usize,
}

impl Counts {
    pub fn add(&mut self, o: &Counts) {
        self.offsets += o.offsets;
        self.filled += o.filled;
        self.table_strings += o.table_strings;
        self.prints += o.prints;
        self.table_words += o.table_words;
        self.table_ends += o.table_ends;
        self.table_writes += o.table_writes;
        self.outside_reads += o.outside_reads;
        self.mismatches += o.mismatches;
        self.byte_mismatches += o.byte_mismatches;
        self.partial_visits += o.partial_visits;
        self.fill_mismatches += o.fill_mismatches;
        self.print_mismatches += o.print_mismatches;
        self.table_mismatches += o.table_mismatches;
        self.past_item += o.past_item;
        self.unknown_script += o.unknown_script;
    }

    pub fn failures(&self) -> usize {
        self.mismatches
            + self.byte_mismatches
            + self.partial_visits
            + self.fill_mismatches
            + self.print_mismatches
            + self.table_mismatches
            + self.past_item
            + self.unknown_script
    }
}

/// A decoded script: its bytes and items.
pub(crate) struct Decoded<'a> {
    pub bytes: &'a [u8],
    pub disasm: &'a Disasm,
}

impl Decoded<'_> {
    fn item_at(&self, at: u32) -> Option<&super::Item> {
        let items = &self.disasm.items;
        let i = items.partition_point(|it| it.offset <= at);
        items.get(i.checked_sub(1)?).filter(|it| at < it.end())
    }

    /// `start..end` is one string of a string table.
    fn string_ok(&self, start: u32, end: u32) -> bool {
        self.entry_at(start).is_some_and(|e| e.end() == end)
    }

    /// The string a read from `at` reads: the string-table entry that starts there, or the
    /// rest of the one an op names `at` inside of ([`super::Table::inner`]).
    fn entry_at(&self, at: u32) -> Option<Span> {
        self.item_at(at).and_then(|it| match &it.kind {
            Kind::Table(t) if !t.words => t
                .entries
                .iter()
                .find(|e| {
                    e.offset == at || (t.inner.contains(&at) && e.offset < at && at <= e.end())
                })
                .map(|e| Span {
                    offset: at,
                    len: e.end() - at,
                }),
            _ => None,
        })
    }

    /// `at` starts one word of a word table.
    fn word_ok(&self, at: u32) -> bool {
        self.item_at(at).is_some_and(|it| match &it.kind {
            Kind::Table(t) if t.words => t.entries.iter().any(|e| e.offset == at),
            _ => false,
        })
    }

    /// The op at `at` has an expression the decoder found reading or writing outside the
    /// evaluator's stack.
    fn stack_hazard(&self, at: u32) -> bool {
        self.item_at(at).is_some_and(|it| {
            it.offset == at
                && matches!(it.kind, Kind::Op(_))
                && self.disasm.hazards.iter().any(|h| {
                    matches!(h.kind, HazardKind::Underflow | HazardKind::Overflow)
                        && it.offset <= h.at
                        && h.at < it.end()
                })
        })
    }

    /// `at` is the `b8` that ends a table.
    fn end_ok(&self, at: u32) -> bool {
        self.item_at(at).is_some_and(|it| {
            matches!(&it.kind, Kind::Table(t) if t.terminated) && it.end() == at + 1
        })
    }
}

/// The script loaded now: its disassembly and its bytes as the ops have written them.
struct Loaded<'a, 'b> {
    d: &'b Decoded<'a>,
    bytes: Vec<u8>,
}

impl Loaded<'_, '_> {
    /// `at` as an offset inside the script.
    fn offset(&self, at: i32, width: u32) -> Option<u32> {
        u32::try_from(at)
            .ok()
            .filter(|&a| (a + width) as usize <= self.bytes.len())
    }
}

/// A text visit in progress: its first offset, the next offset it reads, the state at its first
/// byte and what it put out so far.
struct Visit {
    start: u32,
    next: u32,
    snap: Option<Box<Snap>>,
    out: Vec<i16>,
}

/// A table string read in progress.
struct StrRead {
    start: u32,
    next: u32,
    read: Read,
}

/// A table print in progress: the state when it started and what it put out so far.
struct PrintOut {
    snap: Option<Box<Snap>>,
    out: Vec<i16>,
}

/// Checks `events` of a conversation that ended against the disassemblies (`scripts` by
/// (letter, item)).
pub(crate) fn check(events: &[Event], scripts: &BTreeMap<(u8, u16), Decoded<'_>>) -> Counts {
    check_events(events, scripts, true)
}

/// [`check`] for a conversation stopped while it waits: a text visit or a table print still
/// open at the end is checked as far as it went (its reads a start of whole runs or of one
/// string, its output what those bytes put out, the last one's maybe not all yet).
pub(crate) fn check_interrupted(
    events: &[Event],
    scripts: &BTreeMap<(u8, u16), Decoded<'_>>,
) -> Counts {
    check_events(events, scripts, false)
}

fn check_events(
    events: &[Event],
    scripts: &BTreeMap<(u8, u16), Decoded<'_>>,
    ended: bool,
) -> Counts {
    let mut n = Counts::default();
    let mut cur: Option<Loaded<'_, '_>> = None;
    let mut visit: Option<Visit> = None;
    let mut table: Option<StrRead> = None;
    let mut print: Option<PrintOut> = None;
    // The op `run_text` read last, and whether the table reads now come from an address
    // outside the evaluator's stack.
    let (mut op, mut outside) = (None, false);
    for e in events {
        // Any other read ends a table string read (it must have ended at its NUL) and the
        // print it was for.
        if !matches!(e, Event::Table { .. } | Event::Emit { .. }) {
            n.table_mismatches += usize::from(table.take().is_some());
            print = None;
        }
        if matches!(
            e,
            Event::Load { .. } | Event::Run { .. } | Event::Input { .. }
        ) {
            (op, outside) = (None, false);
        }
        match e {
            Event::Address { inside } => {
                outside = !inside;
                if outside {
                    n.outside_reads += 1;
                    let marked = cur
                        .as_ref()
                        .zip(op)
                        .is_some_and(|(s, at)| s.d.stack_hazard(at));
                    n.table_mismatches += usize::from(!marked);
                }
            }
            Event::Load { letter, item } => {
                close_visit(&mut n, cur.as_ref(), &mut visit);
                cur = scripts.get(&(*letter, *item)).map(|d| Loaded {
                    d,
                    bytes: d.bytes.to_vec(),
                });
            }
            Event::PrintStart { snap } => {
                print = Some(PrintOut {
                    snap: snap.clone(),
                    out: Vec::new(),
                });
            }
            Event::Emit { c, wait: false } => {
                if let Some(v) = &mut visit {
                    v.out.push(*c);
                } else if let Some(p) = &mut print {
                    p.out.push(*c);
                }
            }
            Event::Emit { wait: true, .. } => {}
            Event::Run { at, byte, snap } => {
                n.offsets += 1;
                let Some(s) = &cur else {
                    n.unknown_script += 1;
                    continue;
                };
                let Some(at) = s.offset(*at, 1) else {
                    n.past_item += 1;
                    close_visit(&mut n, cur.as_ref(), &mut visit);
                    continue;
                };
                n.byte_mismatches += usize::from(s.bytes[at as usize] != *byte);
                let Some(item) = s.d.item_at(at) else {
                    n.mismatches += 1;
                    continue;
                };
                if ops::is_text(*byte) {
                    if !matches!(item.kind, Kind::Text(_)) {
                        n.mismatches += 1;
                    }
                    match &mut visit {
                        Some(v) if v.next == at => v.next += 1,
                        _ => {
                            close_visit(&mut n, cur.as_ref(), &mut visit);
                            visit = Some(Visit {
                                start: at,
                                next: at + 1,
                                snap: snap.clone(),
                                out: Vec::new(),
                            });
                        }
                    }
                } else {
                    close_visit(&mut n, cur.as_ref(), &mut visit);
                    op = Some(at);
                    let ok = item.offset == at
                        && match item.kind {
                            Kind::Op(_) => (0x80..0xee).contains(byte) && *byte != ops::KEYWORDS,
                            Kind::Keywords(_) => *byte == ops::KEYWORDS,
                            Kind::BlockEnd | Kind::Input(_) | Kind::Marker(_) => true,
                            Kind::Stop(_) => true,
                            _ => false,
                        };
                    n.mismatches += usize::from(!ok);
                }
            }
            Event::Input { at, byte } => {
                n.offsets += 1;
                close_visit(&mut n, cur.as_ref(), &mut visit);
                let Some(s) = &cur else {
                    n.unknown_script += 1;
                    continue;
                };
                let Some(at) = s.offset(*at, 1) else {
                    n.past_item += 1;
                    continue;
                };
                n.byte_mismatches += usize::from(s.bytes[at as usize] != *byte);
                let ok = s.d.item_at(at).is_some_and(|it| {
                    it.offset == at
                        && match it.kind {
                            Kind::Input(_) | Kind::BlockEnd | Kind::Marker(_) => true,
                            Kind::Stop(s) => !matches!(s, Stop::Control(_)) || *byte < 0x20,
                            Kind::Op(_) => *byte == 0xcb,
                            Kind::Keywords(_) => true,
                            _ => false,
                        }
                });
                n.mismatches += usize::from(!ok);
            }
            Event::Table { at, byte, read } => {
                n.offsets += 1;
                let Some(s) = &cur else {
                    n.unknown_script += 1;
                    continue;
                };
                let Some(at) = s.offset(*at, 1) else {
                    n.past_item += 1;
                    continue;
                };
                n.byte_mismatches += usize::from(s.bytes[at as usize] != *byte);
                if outside {
                    continue;
                }
                if *byte == 0 {
                    // The NUL ends the string; alone, it is an empty string.
                    let r = table.take().unwrap_or(StrRead {
                        start: at,
                        next: at,
                        read: *read,
                    });
                    n.table_strings += 1;
                    let whole = r.next == at && r.read == *read && s.d.string_ok(r.start, at);
                    n.table_mismatches += usize::from(!whole);
                    if *read == Read::Print
                        && let Some(PrintOut {
                            snap: Some(first),
                            out,
                        }) = print.take()
                    {
                        n.prints += 1;
                        let parts = run_parts(&s.bytes, r.start, at).parts;
                        n.print_mismatches +=
                            usize::from(fill_parts(&s.bytes, &parts, &first) != out);
                    }
                    continue;
                }
                match &mut table {
                    Some(r) if r.next == at && r.read == *read => r.next += 1,
                    _ => {
                        n.table_mismatches += usize::from(table.is_some());
                        table = Some(StrRead {
                            start: at,
                            next: at + 1,
                            read: *read,
                        });
                    }
                }
            }
            Event::Word { at, word } => {
                n.offsets += 2;
                let Some(s) = &cur else {
                    n.unknown_script += 1;
                    continue;
                };
                let Some(at) = s.offset(*at, 2) else {
                    n.past_item += 1;
                    continue;
                };
                let i = at as usize;
                n.byte_mismatches += usize::from(s.bytes[i..i + 2] != word.to_le_bytes());
                if outside {
                    continue;
                }
                n.table_words += 1;
                n.table_mismatches += usize::from(!s.d.word_ok(at));
            }
            Event::TableEnd { at } => {
                n.offsets += 1;
                let Some(s) = &cur else {
                    n.unknown_script += 1;
                    continue;
                };
                let Some(at) = s.offset(*at, 1) else {
                    n.past_item += 1;
                    continue;
                };
                n.byte_mismatches += usize::from(s.bytes[at as usize] != ops::TABLE_END);
                if outside {
                    continue;
                }
                n.table_ends += 1;
                n.table_mismatches += usize::from(!s.d.end_ok(at));
            }
            Event::WordWrite { at, word } => {
                n.offsets += 2;
                let Some(s) = &mut cur else {
                    n.unknown_script += 1;
                    continue;
                };
                let Some(at) = s.offset(*at, 2) else {
                    n.past_item += 1;
                    continue;
                };
                n.table_writes += 1;
                n.table_mismatches += usize::from(!s.d.word_ok(at));
                let i = at as usize;
                s.bytes[i..i + 2].copy_from_slice(&word.to_le_bytes());
            }
        }
    }
    if ended {
        // A string read or a visit left open at the end did not reach its end.
        n.table_mismatches += usize::from(table.take().is_some());
        n.partial_visits += usize::from(visit.take().is_some());
        return n;
    }
    // Stopped while it waits: only a print or a text visit can be waiting (on a key, inside
    // the escape layer or the line assembler).
    if let Some(r) = table.take() {
        // A print that has read a start of one string.
        let open = cur.as_ref().filter(|s| {
            r.read == Read::Print && s.d.entry_at(r.start).is_some_and(|e| e.end() >= r.next)
        });
        match (open, print.take()) {
            (
                Some(s),
                Some(PrintOut {
                    snap: Some(first),
                    out,
                }),
            ) => {
                n.prints += 1;
                let upto = |end: u32| {
                    fill_parts(&s.bytes, &run_parts(&s.bytes, r.start, end).parts, &first)
                };
                n.print_mismatches += usize::from(!between(&out, &upto(r.next - 1), &upto(r.next)));
            }
            (Some(_), _) => {}
            (None, _) => n.table_mismatches += 1,
        }
    }
    if let Some(v) = visit.take() {
        open_visit(&mut n, cur.as_ref(), v);
    }
    n
}

/// Output put out while the bytes up to the last one read went through the escape layer, the
/// last one maybe not all yet: it holds what the bytes before the last one put out (`done`) and
/// is a start of what all of them put out (`upto`).
fn between(out: &[i16], done: &[i16], upto: &[i16]) -> bool {
    out.starts_with(done) && upto.starts_with(out)
}

/// A text visit still open where a conversation stopped: its reads must start a run and go on
/// through whole runs; with a snapshot, its output must lie between what the bytes before its
/// last one put out and what the bytes through its last one put out ([`between`]; a cut inside
/// an escape keeps the escape's prefix, as the escape layer does).
fn open_visit(n: &mut Counts, s: Option<&Loaded<'_, '_>>, v: Visit) {
    let Some(s) = s else { return };
    // The runs the reads go through, as (start, end).
    let mut runs = Vec::new();
    let mut at = v.start;
    while at < v.next {
        match s.d.item_at(at) {
            Some(it) if it.offset == at && matches!(it.kind, Kind::Text(_)) => {
                runs.push((it.offset, it.end()));
                at = it.end();
            }
            _ => break,
        }
    }
    if at < v.next {
        n.partial_visits += 1;
        return;
    }
    let Some(first) = v.snap else {
        return;
    };
    n.filled += 1;
    // What the runs' bytes before `cut` put out: each run's parts as the escape layer splits
    // them up to there.
    let upto = |cut: u32| {
        let parts: Vec<Part> = runs
            .iter()
            .filter(|&&(start, _)| start < cut)
            .flat_map(|&(start, end)| run_parts(&s.bytes, start, end.min(cut)).parts)
            .collect();
        fill_parts(&s.bytes, &parts, &first)
    };
    n.fill_mismatches += usize::from(!between(&v.out, &upto(v.next - 1), &upto(v.next)));
}

/// Ends the text visit in progress: its reads must cover whole runs, from a run's start to a
/// run's end (several runs when a path entered text in the middle and split it); with a
/// snapshot, what it put out must be those runs filled in.
fn close_visit(n: &mut Counts, s: Option<&Loaded<'_, '_>>, visit: &mut Option<Visit>) {
    let Some(v) = visit.take() else {
        return;
    };
    let Some(s) = s else { return };
    let mut runs = Vec::new();
    let mut at = v.start;
    while at < v.next {
        match s.d.item_at(at) {
            Some(it) if it.offset == at => match &it.kind {
                Kind::Text(run) => {
                    runs.push(run);
                    at = it.end();
                }
                _ => break,
            },
            _ => break,
        }
    }
    if at != v.next {
        n.partial_visits += 1;
        return;
    }
    let Some(first) = v.snap else {
        return;
    };
    n.filled += 1;
    let want = fill(&s.bytes, &runs, &first);
    n.fill_mismatches += usize::from(want != v.out);
}

fn is_lower(c: i16) -> bool {
    (0x61..=0x7a).contains(&c)
}

fn is_alpha(c: i16) -> bool {
    is_lower(c) || (0x41..=0x5a).contains(&c)
}

/// What runs read one after the other hand the line assembler, from their parts and the state
/// at the first byte: `text_out`'s rules over parts, not bytes. Plain text as it is, each escape
/// filled in or applied, -1 for a flush; spaces and newlines as they are (this is before word
/// wrap). Capitalising and the runic bit of plain text are the line assembler's, after this.
pub(crate) fn fill(script: &[u8], runs: &[&super::Run], s: &Snap) -> Vec<i16> {
    let parts: Vec<Part> = runs.iter().flat_map(|r| r.parts.iter().copied()).collect();
    fill_parts(script, &parts, s)
}

/// [`fill`] over parts (of runs, or of a table string the escape layer prints).
fn fill_parts(script: &[u8], parts: &[Part], s: &Snap) -> Vec<i16> {
    let mut e = s.esc.clone();
    let mut out = Vec::new();
    let string = |out: &mut Vec<i16>, v: u8| {
        let text = s.strs.get(usize::from(v)).cloned().unwrap_or_default();
        out.extend(text.into_iter().take_while(|&b| b != 0).map(i16::from));
    };
    let int = |e: &mut EscState, out: &mut Vec<i16>, v: u8| {
        let w = s.ints.get(usize::from(v)).copied().unwrap_or(0);
        e.plural = w != 1;
        out.extend(itoa(w, 6, -10).into_iter().map(i16::from));
    };
    let glyph = |e: &EscState, c: u8| {
        let c = i16::from(c) & 0x1f;
        if e.runic { c | 0x80 } else { c }
    };
    for p in parts {
        let bytes = &script[p.offset as usize..(p.offset + p.len) as usize];
        // Hidden text drops every character but `&`, escapes included.
        if e.hidden && !e.show_hidden && p.escape != Some(Escape::Hidden) {
            continue;
        }
        match p.escape {
            None => {
                for &b in bytes {
                    let c = i16::from(b);
                    match e.prefix {
                        b'#' => {
                            int(&mut e, &mut out, ops::var_index(b));
                            e.prefix = 0;
                        }
                        b'$' => {
                            string(&mut out, ops::var_index(b));
                            e.prefix = 0;
                        }
                        b'^' => {
                            out.push(glyph(&e, b));
                            e.prefix = 0;
                        }
                        b'/' | b'\\' => {
                            if is_alpha(c) && e.plural == (e.prefix == b'/') {
                                continue;
                            }
                            out.push(c);
                            if !is_alpha(c) {
                                e.prefix = 0;
                            }
                        }
                        b'@' => {
                            if e.keyword_end.contains(&b) {
                                e.prefix = 0;
                            }
                            out.push(c);
                        }
                        _ => out.push(c),
                    }
                }
            }
            Some(Escape::Str(Some(v))) => {
                string(&mut out, v);
                e.prefix = 0;
            }
            Some(Escape::Int(Some(v))) => {
                int(&mut e, &mut out, v);
                e.prefix = 0;
            }
            Some(Escape::Glyph(Some(c))) => {
                out.push(glyph(&e, c));
                e.prefix = 0;
            }
            Some(Escape::Str(None)) => e.prefix = b'$',
            Some(Escape::Int(None)) => e.prefix = b'#',
            Some(Escape::Glyph(None)) => e.prefix = b'^',
            Some(Escape::Wait) => {
                // A flush also ends hidden text (`end_line`).
                out.push(-1);
                e.hidden = false;
            }
            Some(Escape::Keyword) => e.prefix = b'@',
            Some(Escape::Hidden) => e.hidden = !e.hidden,
            Some(Escape::RunicOn) => e.runic = true,
            Some(Escape::RunicOff) => e.runic = false,
            Some(Escape::Capital) => {}
            Some(Escape::Singular) => e.prefix = b'/',
            Some(Escape::Plural) => e.prefix = b'\\',
            Some(Escape::Literal) => e.prefix = b'%',
        }
    }
    out
}

/// Kinds of items, for the receipt's counts.
pub(crate) fn input_kind(i: &Input) -> &'static str {
    match i {
        Input::Ask => "ask",
        Input::Keys(_) => "keys",
        Input::Line { .. } => "line",
        Input::Key => "key",
        Input::Number { .. } => "number",
        Input::Digit { .. } => "digit",
    }
}

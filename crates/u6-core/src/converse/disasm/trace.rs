//! Test-only: what the interpreter reads, recorded at its seams, and the check of those reads
//! against the disassembly (the execution cross-check).
//!
//! The hooks (`#[cfg(test)]` calls in `run.rs`, `exec.rs`, `start.rs` and `script.rs`) do
//! nothing until a test calls [`start`] on its thread. They record:
//!
//! - every byte `run_text`'s loop reads (text, ops, `ef`, stop bytes), and every op byte the
//!   input loop reads;
//! - every byte `print_op`'s `d2` form and `assign`'s `d2` copy read from a table;
//! - which script `load_script` put in the script buffer.
//!
//! With snapshots on, the first text byte of each contiguous read of text, and the first read
//! after it, take the printed scroll (spaces and newlines dropped) and the escape layer's state,
//! so a test can compare what a run printed with the run filled in.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::super::Cx;
use super::super::vars;
use super::ops::{self, Escape};
use super::{Disasm, Input, Kind, Stop};
use crate::state::State;
use crate::ui::text::{CAPITALISE, RUNIC, itoa};

/// One recorded read.
#[derive(Clone, Debug)]
pub(crate) enum Event {
    /// `load_script` loaded item `item` of `converse.<letter>` into the script buffer.
    Load { letter: u8, item: u16 },
    /// `run_text` read `byte` at `at`; `snap` at the start and the end of a text visit.
    Run {
        at: i32,
        byte: u8,
        snap: Option<Box<Snap>>,
    },
    /// The input loop read the op `byte` at `at`.
    Input { at: i32, byte: u8 },
    /// A table string read (`print` = by `print_op`, else by `assign`).
    Table { at: i32, byte: u8, print: bool },
}

/// The printed scroll and the escape layer at a visit boundary.
#[derive(Clone, Debug)]
pub(crate) struct Snap {
    pub printed: Vec<u32>,
    pub esc: EscState,
    pub strs: Vec<Vec<u8>>,
    pub ints: Vec<u16>,
}

/// The escape layer's state that decides what a run prints (`ui/printf.rs` `text_out`,
/// `ui/print.rs` `put_raw`).
#[derive(Clone, Debug)]
pub(crate) struct EscState {
    pub prefix: u8,
    pub hidden: bool,
    pub plural: bool,
    pub capitalise: bool,
    pub runic: bool,
    pub cap_window: bool,
    pub show_hidden: bool,
    pub keyword_end: Vec<u8>,
}

struct Trace {
    events: Vec<Event>,
    snapshots: bool,
    /// Offset of the last text byte `run_text` read, while a visit lasts.
    last_text: Option<i32>,
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

/// The printed scroll as character codes, spaces and newlines dropped (word wrap moves them).
pub(crate) fn printed(s: &State) -> Vec<u32> {
    crate::converse::tests::scroll(s)
        .chars()
        .map(u32::from)
        .filter(|&c| c != 0x20 && c != 0x0a)
        .collect()
}

fn snap(s: &State) -> Snap {
    let t = &s.ui.text;
    let flags = t.win().flags;
    let tail = &s.actors.objlist_tail;
    Snap {
        printed: printed(s),
        esc: EscState {
            prefix: t.prefix,
            hidden: t.hidden,
            plural: t.plural,
            capitalise: t.capitalise,
            runic: flags & RUNIC != 0,
            cap_window: flags & CAPITALISE != 0,
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
    let boundary = if text {
        last != Some(at - 1)
    } else {
        last.is_some()
    };
    let snap = (want && boundary).then(|| Box::new(snap(&cx.state.borrow())));
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

/// Hook: a table string byte at `at` read by `print_op` (`print`) or `assign`.
pub(crate) fn table_byte(at: i32, b: u8, print: bool) {
    push(Event::Table { at, byte: b, print });
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

/// The cross-check's counts. Every field but the first two must be 0 for a pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// Offsets checked (text bytes, op and stop bytes, input ops, table bytes).
    pub offsets: usize,
    /// Text visits whose printed output was compared.
    pub filled: usize,
    /// Reads outside any item, or at an item of the wrong kind or not at its start.
    pub mismatches: usize,
    /// Bytes read that differ from the script's bytes.
    pub byte_mismatches: usize,
    /// Text reads that do not start at a run's start and end at a run's end.
    pub partial_visits: usize,
    /// Visits whose printed output is not the run with its escapes filled in.
    pub fill_mismatches: usize,
    /// Table reads that do not read one whole string of a string table.
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
        self.mismatches += o.mismatches;
        self.byte_mismatches += o.byte_mismatches;
        self.partial_visits += o.partial_visits;
        self.fill_mismatches += o.fill_mismatches;
        self.table_mismatches += o.table_mismatches;
        self.past_item += o.past_item;
        self.unknown_script += o.unknown_script;
    }

    pub fn failures(&self) -> usize {
        self.mismatches
            + self.byte_mismatches
            + self.partial_visits
            + self.fill_mismatches
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
}

/// Checks `events` against the disassemblies (`scripts` by (letter, item)).
pub(crate) fn check(events: &[Event], scripts: &BTreeMap<(u8, u16), Decoded<'_>>) -> Counts {
    let mut n = Counts::default();
    let mut current: Option<&Decoded<'_>> = None;
    let mut known = false;
    // The text visit in progress: (first offset, next expected offset, start snapshot).
    let mut visit: Option<(u32, u32, Option<Box<Snap>>)> = None;
    let mut table: Option<(u32, u32, bool)> = None;
    for e in events {
        if !matches!(e, Event::Table { .. })
            && let Some(t) = table.take()
        {
            n.table_mismatches += usize::from(!table_ok(current, t));
        }
        match e {
            Event::Load { letter, item } => {
                close_visit(&mut n, current, &mut visit, None);
                current = scripts.get(&(*letter, *item));
                known = current.is_some();
            }
            Event::Run { at, byte, snap } => {
                n.offsets += 1;
                let Some(d) = current.filter(|_| known) else {
                    n.unknown_script += 1;
                    continue;
                };
                let Ok(at) = u32::try_from(*at) else {
                    n.mismatches += 1;
                    continue;
                };
                if at as usize >= d.bytes.len() {
                    n.past_item += 1;
                    close_visit(&mut n, current, &mut visit, None);
                    continue;
                }
                n.byte_mismatches += usize::from(d.bytes[at as usize] != *byte);
                let Some(item) = d.item_at(at) else {
                    n.mismatches += 1;
                    continue;
                };
                if ops::is_text(*byte) {
                    if !matches!(item.kind, Kind::Text(_)) {
                        n.mismatches += 1;
                    }
                    match &mut visit {
                        Some((_, next, _)) if *next == at => *next += 1,
                        _ => {
                            close_visit(&mut n, current, &mut visit, None);
                            visit = Some((at, at + 1, snap.clone()));
                        }
                    }
                } else {
                    close_visit(&mut n, current, &mut visit, snap.as_deref());
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
                close_visit(&mut n, current, &mut visit, None);
                let Some(d) = current.filter(|_| known) else {
                    n.unknown_script += 1;
                    continue;
                };
                let Some(at) = u32::try_from(*at).ok().filter(|&a| (a as usize) < d.bytes.len())
                else {
                    n.past_item += 1;
                    continue;
                };
                n.byte_mismatches += usize::from(d.bytes[at as usize] != *byte);
                let ok = d.item_at(at).is_some_and(|it| {
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
            Event::Table { at, byte, print } => {
                let Some(d) = current.filter(|_| known) else {
                    n.offsets += 1;
                    n.unknown_script += 1;
                    continue;
                };
                let Some(at) = u32::try_from(*at).ok().filter(|&a| (a as usize) < d.bytes.len())
                else {
                    n.offsets += 1;
                    n.past_item += 1;
                    continue;
                };
                if *byte == 0 {
                    // The NUL that ends the string: it closes the read.
                    if let Some((s, next, p)) = table.take() {
                        n.table_mismatches += usize::from(next != at || !table_ok(current, (s, at, p)));
                    }
                    continue;
                }
                n.offsets += 1;
                n.byte_mismatches += usize::from(d.bytes[at as usize] != *byte);
                match &mut table {
                    Some((_, next, _)) if *next == at => *next += 1,
                    _ => {
                        if let Some(t) = table.take() {
                            n.table_mismatches += usize::from(!table_ok(current, t));
                        }
                        table = Some((at, at + 1, *print));
                    }
                }
            }
        }
    }
    if let Some(t) = table.take() {
        n.table_mismatches += usize::from(!table_ok(current, t));
    }
    // A visit left open at the end (the run did not reach its stop byte) is partial.
    if visit.take().is_some() {
        n.partial_visits += 1;
    }
    n
}

/// One table read `start..end` must be exactly one entry of a string table.
fn table_ok(d: Option<&Decoded<'_>>, (start, end, _): (u32, u32, bool)) -> bool {
    d.and_then(|d| d.item_at(start)).is_some_and(|it| match &it.kind {
        Kind::Table(t) if !t.words => t
            .entries
            .iter()
            .any(|e| e.offset == start && e.end() == end),
        _ => false,
    })
}

/// Ends the text visit in progress: its reads must cover whole runs, from a run's start to a
/// run's end (several runs when a path entered text in the middle and split it); with both
/// snapshots, what it printed must be those runs filled in.
fn close_visit(
    n: &mut Counts,
    d: Option<&Decoded<'_>>,
    visit: &mut Option<(u32, u32, Option<Box<Snap>>)>,
    end: Option<&Snap>,
) {
    let Some((start, next, first)) = visit.take() else {
        return;
    };
    let Some(d) = d else { return };
    let mut runs = Vec::new();
    let mut at = start;
    while at < next {
        match d.item_at(at) {
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
    if at != next {
        n.partial_visits += 1;
        return;
    }
    let (Some(first), Some(end)) = (first, end) else {
        return;
    };
    n.filled += 1;
    let want = fill(d.bytes, &runs, &first);
    let after = &end.printed;
    let ok = after.len() >= want.len() && after.ends_with(&want) && {
        let head = &after[..after.len() - want.len()];
        first.printed.ends_with(head)
    };
    n.fill_mismatches += usize::from(!ok);
}

fn is_lower(c: u32) -> bool {
    (0x61..=0x7a).contains(&c)
}

fn is_alpha(c: u32) -> bool {
    is_lower(c) || (0x41..=0x5a).contains(&c)
}

/// What runs read one after the other print, from their parts and the state at the first byte:
/// plain text as it is, each escape filled in or applied (`text_out`'s rules over parts, not
/// bytes). Spaces and newlines are dropped, as [`printed`] drops them.
pub(crate) fn fill(script: &[u8], runs: &[&super::Run], s: &Snap) -> Vec<u32> {
    let mut e = s.esc.clone();
    let mut out = Vec::new();
    let raw = |e: &mut EscState, out: &mut Vec<u32>, c: i32| {
        let mut c = c;
        if e.cap_window && e.capitalise && (0x61..=0x7a).contains(&c) {
            c -= 0x20;
        }
        e.capitalise = false;
        if c < 0 {
            // A flush (`*`) also ends hidden text (`end_line`).
            e.hidden = false;
            return;
        }
        if c == 0 {
            c = 0x0a;
        }
        if e.runic && c > 0x20 {
            c |= 0x80;
        }
        if c != 0x20 && c != 0x0a {
            out.push(c as u32);
        }
    };
    for p in runs.iter().flat_map(|r| &r.parts) {
        let bytes = &script[p.offset as usize..(p.offset + p.len) as usize];
        if e.hidden && !e.show_hidden && p.escape != Some(Escape::Hidden) {
            continue;
        }
        match p.escape {
            None => {
                for &b in bytes {
                    if e.hidden && !e.show_hidden {
                        break;
                    }
                    let c = u32::from(b);
                    match e.prefix {
                        b'/' | b'\\' => {
                            if is_alpha(c) && e.plural == (e.prefix == b'/') {
                                continue;
                            }
                            raw(&mut e, &mut out, i32::from(b));
                            if !is_alpha(c) {
                                e.prefix = 0;
                            }
                        }
                        b'@' => {
                            if e.keyword_end.contains(&b) {
                                e.prefix = 0;
                            }
                            raw(&mut e, &mut out, i32::from(b));
                        }
                        _ => raw(&mut e, &mut out, i32::from(b)),
                    }
                }
            }
            Some(Escape::Str(Some(v))) => {
                let text = s.strs.get(usize::from(v)).cloned().unwrap_or_default();
                for b in text.into_iter().take_while(|&b| b != 0) {
                    raw(&mut e, &mut out, i32::from(b));
                }
                e.prefix = 0;
            }
            Some(Escape::Int(Some(v))) => {
                let w = s.ints.get(usize::from(v)).copied().unwrap_or(0);
                e.plural = w != 1;
                for b in itoa(w, 6, -10) {
                    raw(&mut e, &mut out, i32::from(b));
                }
                e.prefix = 0;
            }
            Some(Escape::Glyph(Some(c))) => {
                let g = if e.runic { (c & 0x1f) | 0x80 } else { c & 0x1f };
                raw(&mut e, &mut out, i32::from(g));
                e.prefix = 0;
            }
            Some(Escape::Str(None)) => e.prefix = b'$',
            Some(Escape::Int(None)) => e.prefix = b'#',
            Some(Escape::Glyph(None)) => e.prefix = b'^',
            Some(Escape::Wait) => raw(&mut e, &mut out, -1),
            Some(Escape::Keyword) => e.prefix = b'@',
            Some(Escape::Hidden) => e.hidden = !e.hidden,
            Some(Escape::RunicOn) => e.runic = true,
            Some(Escape::RunicOff) => e.runic = false,
            Some(Escape::Capital) => e.capitalise = true,
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

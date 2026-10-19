//! The escape layer 0x108376 and `printf` 0x108658.
//!
//! Escapes (one character each, consumed by 0x108376): `#n` prints word variable n, `$n` string
//! n (n = `0`-`9`, `A`-`Z`); after `/` letters print only for a singular, after `\` only for a
//! plural (the last `%d` or `#n` decides); `@` starts a keyword in the alert colour that ends at
//! a character of the keyword-end set; `^c` prints control glyph `c & 0x1f`; `&` toggles hidden
//! text; `*` waits for a key; `<` / `>` switch the runic glyphs on / off; a back-quote
//! capitalises the next character. `printf` handles `%[-][0][width]` + `c d o s u x` (case
//! ignored); any other directive character is put out unescaped, which is how strings write a
//! literal escape character (`%/`, `%>`, `%%`).
//!
//! Port (spec 08): in front of the escape layer, with `wide` on, the UTF-8 assembler, and with
//! Korean grammar, the josa resolver ([`super::wide`]); with `wide` on, `printf` also takes
//! `%N$` (argument N, from 1), counts a field's padding in cells, and `@{…}` puts exactly the
//! braced text in the alert colour: `@` then at once `{` opens it (that `{` never starts a josa
//! marker), `}` or the end of the unit of text that opened it closes it ([`end_unit`]). In
//! Korean, `/` and `\` print no letters. Off and English, every byte reaches the escape layer as
//! before.

use std::cell::RefCell;

use super::print::{put_cell, put_cells, put_raw, put_raw_n};
use super::text::{CAPITALISE, RUNIC, SCROLL, WRAP, itoa};
use super::textstate::LINE;
use super::wide::{self, Language};
use crate::engine::Io;
use crate::state::State;

/// One `printf` argument: the original pulls a word (`c d o u x`) or a pointer (`s`) per
/// directive from the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg<'a> {
    Word(u16),
    Str(&'a [u8]),
}

impl Arg<'_> {
    fn word(self) -> u16 {
        match self {
            Arg::Word(w) => w,
            Arg::Str(_) => 0,
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Arg::Str(s) => s,
            Arg::Word(_) => &[],
        }
    }
}

fn ch(b: u8) -> i32 {
    i32::from(b)
}

fn is_lower(c: i32) -> bool {
    (0x61..=0x7a).contains(&c)
}

fn is_alpha(c: i32) -> bool {
    is_lower(c) || (0x41..=0x5a).contains(&c)
}

/// `#n` / `$n` index: digits 0..9, letters (either case) 10..35, anything else 0.
fn var_index(c: i32) -> usize {
    let c = if is_lower(c) { c - 0x20 } else { c };
    match c {
        0x30..=0x39 => (c - 0x30) as usize,
        0x41..=0x5a => (c - 0x37) as usize,
        _ => 0,
    }
}

/// The word at A4-0x7b26: when set, hidden `&` text is printed too. A4-0x7b26 lies inside the
/// 0x82-byte objlist block read to A4-0x7b84, at its byte 0x5e [likely] (the offset is derived
/// from the two addresses, no writer of A4-0x7b26 was read).
fn show_hidden(s: &State) -> bool {
    let tail = &s.actors.objlist_tail;
    tail.get(0x5e..0x60).is_some_and(|w| w != [0, 0])
}

/// Original: 0x108376 (ui: put a character through the escape layer)
///
/// Port (spec 08): `c` is a byte of text (or a negative flush). With `wide` on it goes through
/// the UTF-8 assembler, with Korean grammar the codes through the josa resolver, and what they
/// let through reaches the escape layer ([`escape`]) one code at a time, in order. Off and
/// English (nothing held), `c` goes straight to it.
pub async fn text_out(state: &RefCell<State>, io: &Io, c: i32) {
    let codes = {
        let mut s = state.borrow_mut();
        let t = &mut s.ui.text;
        if !t.on {
            return;
        }
        if !t.wide && t.language == Language::English && t.utf8.is_empty() && t.josa.is_empty() {
            None
        } else {
            let mut out = Vec::new();
            match u8::try_from(c) {
                Ok(b) if t.wide => t.utf8.feed(b, &mut out),
                _ => {
                    t.utf8.drain(&mut out);
                    out.push(c);
                }
            }
            Some(out)
        }
    };
    match codes {
        None => escape(state, io, c).await,
        Some(codes) => {
            for c in codes {
                josa(state, io, c).await;
            }
        }
    }
}

/// Port-only (spec 08): code `c` through the josa resolver (Korean grammar), then what it lets
/// through to the escape layer.
async fn josa(state: &RefCell<State>, io: &Io, c: i32) {
    let codes = {
        let mut s = state.borrow_mut();
        let t = &mut s.ui.text;
        let mut out = Vec::new();
        if t.language == Language::Korean {
            if t.wide && t.after_at && c == ch(b'{') && t.josa.is_empty() {
                // The `{` of `@{` opens the brace ([`escape`]); it starts no marker.
                out.push(c);
            } else {
                let tail = t.tail;
                t.josa.feed(c, &tail, &mut out);
            }
        } else {
            out.append(&mut t.josa.drain());
            out.push(c);
        }
        out
    };
    for c in codes {
        escape(state, io, c).await;
    }
}

/// Port-only (spec 08): lets out what the assembler and the josa resolver hold, unchanged,
/// through the escape layer: a put from outside it ([`put_raw`], [`put_raw_n`],
/// [`super::print::put_raw_str`]) then comes after the bytes before it, as with `wide` off, and
/// the end of a unit of text ([`end_unit`]) leaves nothing to join the next.
pub(super) async fn release(state: &RefCell<State>, io: &Io) {
    let held = {
        let mut s = state.borrow_mut();
        let t = &mut s.ui.text;
        if t.utf8.is_empty() && t.josa.is_empty() {
            return;
        }
        let mut out = Vec::new();
        t.utf8.drain(&mut out);
        out
    };
    for c in held {
        josa(state, io, c).await;
    }
    let rest = state.borrow_mut().ui.text.josa.drain();
    for c in rest {
        escape(state, io, c).await;
    }
}

/// The escape characters that set the pending prefix.
const PREFIXES: [u8; 7] = [b'#', b'$', b'%', b'/', b'@', b'\\', b'^'];

#[cfg(test)]
thread_local! {
    /// Test-only: the codes the escape layer has taken with text output on, which the tests of
    /// `printf`'s cell count compare with it.
    pub(super) static TAKEN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The escape layer of 0x108376 for one code.
async fn escape(state: &RefCell<State>, io: &Io, c: i32) {
    let (prefix, after_at) = {
        let mut s = state.borrow_mut();
        if !s.ui.text.on {
            return;
        }
        #[cfg(test)]
        TAKEN.with(|n| n.set(n.get() + 1));
        // Port (spec 08): whether `c` comes right after an `@`. Every code ends that, one that
        // hidden text drops too, and only an `@` taken as a prefix (below) sets it again.
        let after_at = std::mem::take(&mut s.ui.text.after_at);
        if c != ch(b'&') && s.ui.text.hidden && !show_hidden(&s) {
            return;
        }
        let colours = s.ui.tables.colours;
        let t = &mut s.ui.text;
        let prefix = match c {
            // Port (spec 08): the brace closing `@{…}`.
            _ if t.brace && c == ch(b'}') => {
                t.brace = false;
                t.win_mut().fg = colours.text;
                return;
            }
            _ if PREFIXES.map(ch).contains(&c) => {
                t.prefix = c as u8;
                t.after_at = c == ch(b'@');
                return;
            }
            _ if c == ch(b'&') => {
                t.hidden = !t.hidden;
                return;
            }
            _ if c == ch(b'<') => {
                t.win_mut().flags |= RUNIC;
                return;
            }
            _ if c == ch(b'>') => {
                t.win_mut().flags &= !RUNIC;
                return;
            }
            _ if c == ch(b'`') => {
                t.capitalise = true;
                return;
            }
            _ if c == ch(b'*') => None,
            _ => Some(t.prefix),
        };
        (prefix, after_at)
    };
    let Some(prefix) = prefix else {
        put_cell(state, io, -1).await;
        {
            // Port (spec 09): what the speech tap kept before the wait is flushed.
            let mut s = state.borrow_mut();
            crate::speech::flush(&mut s);
            s.ui.input.cursor_glyph = 1;
        }
        Box::pin(super::wait::read_key(state, io)).await;
        state.borrow_mut().ui.input.cursor_glyph = 5;
        return;
    };
    match prefix {
        b'#' => {
            let v = {
                let mut s = state.borrow_mut();
                let v = crate::converse::int_var(&s, var_index(c) as u8);
                s.ui.text.plural = v != 1;
                v
            };
            put_cells(state, io, &itoa(v, 6, -10)).await;
            state.borrow_mut().ui.text.prefix = 0;
        }
        b'$' => {
            let text = crate::converse::string_var(&state.borrow(), var_index(c) as u8);
            put_cells(state, io, &text).await;
            state.borrow_mut().ui.text.prefix = 0;
        }
        b'/' | b'\\' => {
            let (plural, language) = {
                let t = &state.borrow().ui.text;
                (t.plural, t.language)
            };
            // `/` keeps letters for a singular, `\` for a plural. Port (spec 08): in Korean
            // neither keeps any: the marker and every ASCII letter after it print nothing, and
            // the first other character ends the escape and prints.
            let drop = match language {
                Language::English => plural == (prefix == b'/'),
                Language::Korean => true,
            };
            if is_alpha(c) && drop {
                return;
            }
            put_cell(state, io, c).await;
            if !is_alpha(c) {
                state.borrow_mut().ui.text.prefix = 0;
            }
        }
        b'@' => {
            {
                let mut s = state.borrow_mut();
                if s.ui.text.wide && after_at && c == ch(b'{') {
                    // Port (spec 08): `@` then at once `{` opens a brace highlight, closed by
                    // `}` or the end of the unit of text that opened it ([`end_unit`]); the puts
                    // between take the alert colour ([`put_cell`]). A `{` later in an `@` keyword
                    // is the keyword's.
                    let t = &mut s.ui.text;
                    t.prefix = 0;
                    t.brace = true;
                    t.braces = t.braces.wrapping_add(1);
                    return;
                }
                let ends = c == 0
                    || ((0..0x100).contains(&c) && s.ui.tables.keyword_end.contains(&(c as u8)));
                let colours = s.ui.tables.colours;
                let t = &mut s.ui.text;
                if ends {
                    t.prefix = 0;
                    t.win_mut().fg = colours.text;
                } else if t.keyword_colour {
                    t.win_mut().fg = colours.alert;
                }
            }
            put_cell(state, io, c).await;
        }
        b'^' => {
            let runic = state.borrow().ui.text.win().flags & RUNIC != 0;
            let g = if runic { (c & 0x1f) | 0x80 } else { c & 0x1f };
            put_cell(state, io, g).await;
            state.borrow_mut().ui.text.prefix = 0;
        }
        _ => put_cell(state, io, c).await,
    }
}

/// Original: 0x108642 (ui: put each character of a string through the escape layer)
pub async fn puts(state: &RefCell<State>, io: &Io, s: &[u8]) {
    for &b in s.iter().take_while(|&&b| b != 0) {
        text_out(state, io, i32::from(b)).await;
    }
}

/// `%N$` after a `%` (N from 1, its first digit not 0): N, the bytes taken; else `None`, none.
fn position<I: Iterator<Item = u8> + Clone>(f: &mut I) -> Option<usize> {
    let mut look = f.clone();
    let mut b = look.next()?;
    if !(b'1'..=b'9').contains(&b) {
        return None;
    }
    let mut n = 0usize;
    while b.is_ascii_digit() {
        n = n.saturating_mul(10).saturating_add(usize::from(b - b'0'));
        b = look.next()?;
    }
    if b != b'$' {
        return None;
    }
    *f = look;
    Some(n)
}

/// Port-only (spec 08 §1): the padding of a `printf` field with `wide` on (`width` and `pad`
/// the directive's), the cells the field takes after it, and the copy's last cell after the
/// field. `printf` puts that padding.
///
/// Each code the assembler and the josa resolver let through counts one cell, escape characters
/// included (a field's escapes count as themselves, as its bytes do with `wide` off). The count
/// of a field whose own end is held (a UTF-8 sequence or josa marker it begins and does not
/// finish) is provisional: the held end is counted as if let out, one cell per code. Where the
/// text after the field completes it instead, the field is drawn as completed, and its cells can
/// differ from its count either way: a two-byte lead completed is one syllable, fewer; a
/// one-byte lead completed is one, as counted; a josa marker begun can take its paired form,
/// more (another way the count is not the drawn width, beside the cases below where no
/// padding fits). Such a string, split across a field and the text after it, is not well
/// formed; the translation tool refuses it. The tail the resolver reads takes only the cells
/// [`Escaped`] reports put into the line for a code (none for an escape character, hidden text
/// or a brace; what `#n` and `$n` print; no space that the start of a wrapped row drops), as
/// the engine's tail takes the cells it puts. The padding is no cell of the tail
/// ([`put_raw_n`]), but it is laid out: a padding of spaces that wraps a started row (one pad
/// more than the row has room for) leaves the row after dropping the spaces the field first
/// lays; a padding that only fills the row does not wrap it; where the row then wraps follows
/// from the field's own codes as the line lays them.
///
/// A left-aligned field is put before its padding, counted from what the front holds (a
/// syllable or marker begun before it and finished by it is the field's): its cells do not
/// depend on the padding, so the padding is the width less them (none when they reach it).
///
/// A right-aligned field comes after its padding, and its cells depend on it: a padding of 1
/// or more first lets out what is held (the text before it, so the field no longer finishes
/// it) and ends an `@` just before (so a `{` opens no brace), and it can wrap the row; a
/// padding of 0 puts nothing and lets nothing out, so the field goes on from what is held.
/// With `cells(p)` the cells the field takes after a padding of `p` and `w` the width (at least 0),
/// a padding `p` fits when `p = max(0, w - cells(p))`. The padding is found by iterating
/// `p0 = max(0, w - cells(0))`, `p(k+1) = max(0, w - cells(pk))` and stopping at the first `p` that
/// fits. A `p` that fits meets or exceeds the width (`p + cells(p) >= w`). Every `p` lies in
/// `0..=w`, so the values repeat within `w + 2` steps. When they repeat with none that fits, the
/// padding is 0 if something is held before the field (the assembler or the josa resolver holds a
/// code), and else the smallest `p` of the cycle.
///
/// Either way the field's cells fall short of the width: the padding chosen and the cells after it
/// come to less than `w`. Write `f(p) = max(0, w - cells(p))`, and let `m` be the smallest `p` of
/// the cycle. `f(m)` is in the cycle, so `f(m) >= m`, and no `p` of the cycle fits, so
/// `f(m) > m >= 0`: `w - cells(m) > m`, and `m + cells(m) < w`. A padding of 0 where something is
/// held falls short too: the values reach a cycle only when `p0 = f(0) > 0`, so `cells(0) < w`.
/// That bounds the count, not the line drawn. The line can pass the count in two ways only: `#n`
/// and `$n`, which can put more cells than their codes (above), and a field whose own end is held
/// and completed by the text after it, which is drawn as completed (above). After a held `\xea`,
/// `%5s/가}|` with the field `\xb0\x80a{이` counts four with no padding and five after any, so the
/// paddings go 1, 0, 1 and, something being held, 0 is used; the format's `/가}` then completes the
/// marker after `a`, and the field draws six cells, `가a이(가)`, the `|` after it (the row is seven).
/// The line can also fall below the count, since some codes that are counted draw nothing; among
/// them: escape characters (a back-quote, `<` and `>`, `&`, `%`, `^`, a keyword `@`, and the `@`
/// and braces of `@{…}`), in Korean a `/` or `\` and the letters after it, hidden text while
/// hidden, a newline (it puts no cell, and one the scroll does not skip ends the row), a space the
/// start of a wrapped row drops, the space a wrapping row breaks at, and a short `#n` or `$n` (`#3`
/// holding 2 puts one cell for its two codes, an empty `$1` none).
///
/// A padding of 1 or more, with its first pad and whatever its size, lets out what is held and ends
/// an `@`, a newline to skip and a pending capital; a `0` padding also ends the dropping of spaces
/// at the start of a wrapped row (a `0` is no space). A padding of spaces that wraps a started row
/// (one pad more than the row has room for) leaves the row after dropping the spaces the field
/// first lays (only a code the line lays ends a drop); a padding that only fills the row does not
/// wrap it; where the row then wraps follows from the field's own codes as the line lays them.
///
/// No padding fits where the cells after the paddings tried send the values round a cycle. The
/// cases met so far: a held sequence the field would complete (a UTF-8 lead, a josa marker begun),
/// which a padding lets out first, so the field's codes are put apart; a `0` padding after a row
/// left dropping spaces, whose `0`s end the drop, so the field's leading space is a cell and a
/// marker after it takes the paired form; a newline to skip, which a padding ends, so the field's
/// newline is put and ends a row that drops spaces, with the same effect; an `@` just before the
/// field, which a padding ends, so the field's `{` opens no brace (`가@%5s` with `{가}{이/가}` counts
/// four with no padding and seven after any, and the paddings go 1, 0, 1); digits joined across a
/// dropped space, where a padding of spaces that wraps the row drops the field's leading space, so
/// the field's zeros continue the number before the padding, and twenty or more trailing zeros have
/// no reading, so a marker after them takes the paired form (fifteen cells ending in `1` before
/// `%26s` with a space, twenty `0`s and `{이/가}`: 22 cells after paddings 0 to 2 and 25 after 3 or
/// more, the paddings go 4, 1, 4, and 1 is used); hidden text completing a held sequence, whose
/// completing codes are never laid, so the field's first laid code can be a space that a wrapping
/// padding drops (after `aaaaaaaaa가&` and a held `\xea`, `%8s` with `\xb0\x80& {이/가}`: 7 cells with
/// no padding, 8 after 1 to 7 and 5 after 8, the paddings go 1, 0, 1, and as something is held 0 is
/// used).
///
/// Where something is held, 0 keeps it for the field: short is preferred to broken, since each pad
/// put lets out what is held first, and a padding would show the sequence's bytes or codes apart
/// instead of the syllable or particle. After a held lead, a right-aligned field of width 2 that
/// finishes the syllable takes one cell with no padding and two bytes after a padding of 1: no
/// padding fits, and 0 is used, so the syllable is drawn whole, one cell short. Where nothing is
/// held, the smallest padding of the cycle leaves the field's cells short of the width rather than
/// past it: after 가's padding left a row dropping spaces, `%03s` with ` {이/가}` counts two with no
/// padding (the dropped space and the particle, drawn `가`) and five after any padding of `0`s
/// (` 이(가)`), the paddings go 1, 0, 1, and 0 is used, so the row shows `가|`. Where a padding of 1
/// or more fits, it is the first fixed point and is used, even with something held, though it lets
/// the held sequence out: at width 3 the held-lead field above takes a padding of 1, and `\xea%3s`
/// with the field `\xb0\x80` draws the lead byte, a space and the field's two bytes, apart.
///
/// So the count and the copy's last cell are the engine's at the field's end, for the padding
/// chosen, unless the field's own end is held and what follows the field completes it.
pub(super) fn field_count(
    s: &State,
    field: &[u8],
    left: bool,
    width: i16,
    pad: u8,
) -> (i16, usize, wide::Tail) {
    // Each simulation also gives the branches of the copy's line it took (tests only).
    let count = |p: i16| {
        let before = branches();
        let mut front = wide::Front::new(&s.ui.text, Escaped::new(s));
        if !left {
            front.pad(p, i32::from(pad));
            front.cells = 0;
        }
        front.feed(field);
        front.release();
        (front.cells, front.tail(), since(before))
    };
    let w = i32::from(width.max(0));
    let fit = |cells: usize| (w - i32::try_from(cells).unwrap_or(i32::MAX)).max(0) as i16;
    let first = count(0);
    let mut p = fit(first.0);
    if left || p == 0 {
        // No padding (or a left-aligned field): the count after no padding is the field's.
        returned(first.2);
        return (p, first.0, first.1);
    }
    // Held before the field: what a padding of 1 or more would let out.
    let held = !s.ui.text.utf8.is_empty() || !s.ui.text.josa.is_empty();
    // Each padding tried, with its count, its tail and the branches its simulation took.
    let mut seen: Vec<(i16, usize, wide::Tail, Branches)> = Vec::new();
    // At most `w + 2` passes: each returns or takes a padding not taken before, and `0..=w`
    // has `w + 1`. The bound ends the loop on its own should the check for a repeat break.
    for _ in 0..=w + 1 {
        let (cells, tail, laid) = count(p);
        let next = fit(cells);
        if next == p {
            returned(laid);
            return (p, cells, tail);
        }
        if let Some(i) = seen.iter().position(|q| q.0 == p) {
            // No padding fits: with something held, 0 (no padding lets it out); else the
            // smallest of the cycle, from its own simulation.
            if held {
                returned(first.2);
                return (0, first.0, first.1);
            }
            let (m, cells, tail, laid) = seen[i..]
                .iter()
                .copied()
                .min_by_key(|q| q.0)
                .unwrap_or((p, cells, tail, laid));
            returned(laid);
            return (m, cells, tail);
        }
        seen.push((p, cells, tail, laid));
        p = next;
    }
    // Not reached (above). A debug build, and so every test, fails here; a release build
    // uses no padding.
    debug_assert!(
        false,
        "the paddings {:?} did not repeat within w + 2 passes",
        seen.iter().map(|q| q.0).collect::<Vec<_>>()
    );
    returned(first.2);
    (0, first.0, first.1)
}

/// The branches of [`Escaped::lay`], which the tests check their random corpus reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Laid {
    SkippedNewline,
    Capital,
    Runic,
    DroppedSpace,
    Wrap,
    WrapInWord,
    WrapAtSpace,
    Newline,
    FullRow,
    /// A row that wraps breaks after a hyphen.
    WrapAtHyphen,
    /// A row that wraps keeps all after its start (a flush moved the start).
    WrapAtStart,
    /// A code other than a newline ends a skipped newline.
    SkipEnded,
    /// A newline to skip, in a window that does not scroll: none is skipped.
    SkipNotScrolling,
}

#[cfg(test)]
impl Laid {
    pub(super) const ALL: [Laid; 13] = [
        Laid::SkippedNewline,
        Laid::Capital,
        Laid::Runic,
        Laid::DroppedSpace,
        Laid::Wrap,
        Laid::WrapInWord,
        Laid::WrapAtSpace,
        Laid::Newline,
        Laid::FullRow,
        Laid::WrapAtHyphen,
        Laid::WrapAtStart,
        Laid::SkipEnded,
        Laid::SkipNotScrolling,
    ];
}

#[cfg(test)]
thread_local! {
    /// Test-only: how often each branch of [`Escaped::lay`] was taken, by [`Laid`].
    pub(super) static LAID: std::cell::Cell<[u32; 13]> = const { std::cell::Cell::new([0; 13]) };
}

/// Test-only: counts a branch of [`Escaped::lay`] ([`LAID`]); nothing otherwise.
#[cfg(test)]
fn reached(branch: Laid) {
    LAID.with(|l| {
        let mut v = l.get();
        v[branch as usize] += 1;
        l.set(v);
    });
}

#[cfg(not(test))]
fn reached(_: Laid) {}

/// Test-only: branches of [`Escaped::lay`] taken, by [`Laid`]; nothing otherwise.
#[cfg(test)]
type Branches = [u32; 13];

#[cfg(not(test))]
#[derive(Clone, Copy)]
struct Branches;

#[cfg(test)]
thread_local! {
    /// Test-only: the branches the simulation whose count and tail [`field_count`] last
    /// returned took (one of the simulations it runs: `count(0)` or a padding it tried).
    pub(super) static RETURNED: std::cell::Cell<Branches> = const { std::cell::Cell::new([0; 13]) };
}

/// Test-only: the branches taken so far ([`LAID`]); nothing otherwise.
#[cfg(test)]
fn branches() -> Branches {
    LAID.with(|l| l.get())
}

#[cfg(not(test))]
fn branches() -> Branches {
    Branches
}

/// Test-only: the branches taken since `before`; nothing otherwise.
#[cfg(test)]
fn since(before: Branches) -> Branches {
    let now = branches();
    std::array::from_fn(|i| now[i] - before[i])
}

#[cfg(not(test))]
fn since(_: Branches) -> Branches {
    Branches
}

/// Test-only: records the branches of the simulation [`field_count`] returns ([`RETURNED`]);
/// nothing otherwise.
#[cfg(test)]
fn returned(laid: Branches) {
    RETURNED.with(|r| r.set(laid));
}

#[cfg(not(test))]
fn returned(_: Branches) {}

/// Port-only (spec 08 §1): a copy of the escape layer ([`escape`]) and of the line it puts into
/// ([`put_cell`]), for the cells of a field ([`field_count`]). For each code it reports the cell
/// the line takes, which the josa resolver's last cell follows, and whether a `{` now opens a
/// brace. Of the line it keeps what decides whether a code becomes a cell: the window's capital
/// (a letter after a back-quote), the NUL that prints as a newline, the runic glyphs, a newline
/// a scrolling window skips, and the layout of a wrapping row (its cells, the hyphen and space
/// it breaks at, and that the start of a row wrapped at a space drops spaces until a code that
/// is not one). The copy's last cell is read only in Korean ([`wide::Front`]), so `/` and `\`
/// keep Korean's rule (every letter after them is dropped) and the plural is not kept.
struct Escaped<'a> {
    s: &'a State,
    prefix: u8,
    after_at: bool,
    hidden: bool,
    shown: bool,
    brace: bool,
    runic: bool,
    /// [`super::textstate::TextState::capitalise`], and whether the window capitalises.
    capitalise: bool,
    capitals: bool,
    /// [`super::textstate::TextState::skip_newline`] in a scrolling window.
    skip_newline: bool,
    /// The line: its cell codes, the first not drawn, its length, the window's column, the cells
    /// it holds before it is full, whether it wraps, and whether a code other than a space was
    /// put since a row wrapped at a space (0x108262).
    line: [u16; LINE],
    start: usize,
    len: usize,
    col: usize,
    cap: usize,
    wrap: bool,
    nonspace: bool,
}

impl<'a> Escaped<'a> {
    fn new(s: &'a State) -> Self {
        let t = &s.ui.text;
        let w = t.win();
        if t.skip_newline && w.flags & SCROLL == 0 {
            reached(Laid::SkipNotScrolling);
        }
        Escaped {
            s,
            prefix: t.prefix,
            after_at: t.after_at,
            hidden: t.hidden,
            shown: show_hidden(s),
            brace: t.brace,
            runic: w.flags & RUNIC != 0,
            capitalise: t.capitalise,
            capitals: w.flags & CAPITALISE != 0,
            skip_newline: t.skip_newline && w.flags & SCROLL != 0,
            line: t.line,
            start: usize::from(t.line_start),
            len: usize::from(t.line_len),
            col: usize::from(w.col),
            cap: usize::from(w.width.min(LINE as u8 - 1)),
            wrap: w.flags & WRAP != 0,
            nonspace: t.nonspace,
        }
    }

    /// As [`put_cell`] puts `c` (from 0) into the line: pushes to `cells` the cell the line
    /// takes, if any.
    fn put(&mut self, c: i32, cells: &mut Vec<i32>) {
        if let Some(c) = self.lay(c) {
            cells.push(c);
        }
    }

    /// The line's part of [`put_cell`] and `end_line` for `c` (from 0): the cell the line and the
    /// josa tail take, if any.
    fn lay(&mut self, c: i32) -> Option<i32> {
        const NL: i32 = 0x0a;
        const SPACE: i32 = 0x20;
        let mut c = c;
        if self.skip_newline && c == NL {
            reached(Laid::SkippedNewline);
            return None;
        }
        if self.skip_newline {
            reached(Laid::SkipEnded);
        }
        self.skip_newline = false;
        if self.capitals && self.capitalise && is_lower(c) {
            reached(Laid::Capital);
            c -= 0x20;
        }
        self.capitalise = false;
        if c == 0 {
            c = NL;
        }
        if self.runic && c > SPACE && c < 0x80 {
            reached(Laid::Runic);
            c |= 0x80;
        }
        let code = u16::try_from(c).unwrap_or(u16::MAX);
        let full = self.len >= self.cap;
        let mut cell = None;
        if !full && c != NL {
            // The start of a row wrapped at a space drops spaces.
            if !(self.wrap && c == SPACE && !self.nonspace) {
                self.line[self.len] = code;
                self.len += 1;
                cell = Some(c);
            } else {
                reached(Laid::DroppedSpace);
            }
        } else if self.wrap && full {
            // The row wraps: the line keeps what follows its last space or hyphen.
            let (sp, hyphen) = (0x20u16, 0x2du16);
            let len = self.len;
            self.line[len] = if c == NL { sp } else { code };
            if c != NL {
                cell = Some(c);
            }
            let mut end = len;
            while self.line[end] != sp
                && !(end >= 1 && self.line[end - 1] == hyphen)
                && end > self.start
            {
                end -= 1;
            }
            if self.line[end] != sp && end >= 1 && self.line[end - 1] == hyphen {
                reached(Laid::WrapAtHyphen);
            } else if self.line[end] != sp && end == self.start && end > 0 {
                reached(Laid::WrapAtStart);
            }
            if end == 0 {
                end = len;
            }
            if self.line[end] == sp {
                end += 1;
            }
            self.line.copy_within(end..=len, 0);
            self.col = 0;
            self.start = 0;
            self.len = len + 1 - end;
            reached(Laid::Wrap);
            if self.len > 0 {
                reached(Laid::WrapInWord);
            }
            if c == SPACE {
                reached(Laid::WrapAtSpace);
                self.nonspace = false;
            }
        } else {
            reached(if c == NL {
                Laid::Newline
            } else {
                Laid::FullRow
            });
            self.col = 0;
            self.start = 0;
            self.len = 0;
            if c != NL {
                self.line[0] = code;
                self.len = 1;
                cell = Some(c);
            }
        }
        if c != SPACE {
            self.nonspace = true;
        }
        cell
    }

    /// The cells [`put_cells`] would put for `text`.
    fn put_text(&mut self, text: &[u8], cells: &mut Vec<i32>) {
        let text = &text[..text.iter().position(|&b| b == 0).unwrap_or(text.len())];
        if self.s.ui.text.wide {
            for c in wide::decode(text) {
                self.put(c, cells);
            }
        } else {
            for &b in text {
                self.put(i32::from(b), cells);
            }
        }
    }
}

impl wide::Escapes for Escaped<'_> {
    fn after_at(&self) -> bool {
        self.after_at
    }

    /// As [`put_raw_n`]: `n` copies of `c` laid out in the line (no cell of the tail).
    fn pad(&mut self, n: i16, c: i32) {
        self.after_at = false;
        for _ in 0..n {
            self.lay(c);
        }
    }

    /// As [`escape`] takes `c` (text output is on: `printf` counts nothing when it is off).
    fn take(&mut self, c: i32, cells: &mut Vec<i32>) {
        let after_at = std::mem::take(&mut self.after_at);
        if c != ch(b'&') && self.hidden && !self.shown {
            return;
        }
        match c {
            _ if self.brace && c == ch(b'}') => {
                self.brace = false;
                return;
            }
            _ if PREFIXES.map(ch).contains(&c) => {
                self.prefix = c as u8;
                self.after_at = c == ch(b'@');
                return;
            }
            _ if c == ch(b'&') => {
                self.hidden = !self.hidden;
                return;
            }
            _ if c == ch(b'<') || c == ch(b'>') => {
                self.runic = c == ch(b'<');
                return;
            }
            _ if c == ch(b'`') => {
                self.capitalise = true;
                return;
            }
            // The key wait's flush (0x107e5a with -1): it ends hidden text and the capital, the
            // line is drawn to its end and goes on from the window's column, and a flush is a
            // code other than a space. It puts no cell.
            _ if c == ch(b'*') => {
                self.capitalise = false;
                self.hidden = false;
                if self.start < self.len {
                    self.col = self.len;
                }
                self.start = self.col;
                self.len = self.col;
                self.nonspace = true;
                return;
            }
            _ => {}
        }
        let s = self.s;
        match self.prefix {
            b'#' => {
                let v = crate::converse::int_var(s, var_index(c) as u8);
                self.put_text(&itoa(v, 6, -10), cells);
                self.prefix = 0;
            }
            b'$' => {
                let text = crate::converse::string_var(s, var_index(c) as u8);
                self.put_text(&text, cells);
                self.prefix = 0;
            }
            b'/' | b'\\' => {
                if is_alpha(c) {
                    return;
                }
                self.put(c, cells);
                self.prefix = 0;
            }
            b'@' => {
                if s.ui.text.wide && after_at && c == ch(b'{') {
                    self.prefix = 0;
                    self.brace = true;
                    return;
                }
                let ends = c == 0
                    || ((0..0x100).contains(&c) && s.ui.tables.keyword_end.contains(&(c as u8)));
                if ends {
                    self.prefix = 0;
                }
                self.put(c, cells);
            }
            b'^' => {
                let g = if self.runic {
                    (c & 0x1f) | 0x80
                } else {
                    c & 0x1f
                };
                self.put(g, cells);
                self.prefix = 0;
            }
            _ => self.put(c, cells),
        }
    }
}

/// Original: 0x108658 (ui: formatted output to the current window)
///
/// Plain characters go through the escape layer; a field's text too (so `%s` strings may carry
/// escapes), its padding unescaped. `%c` leaves the field length at 0, so its padding is the
/// whole width. Every number directive sets the plural flag (`value != 1`).
///
/// Port (spec 08): with `wide` on, `%N$` right after the `%` makes the directive take argument
/// N (from 1; the others still take theirs in order), and a field's padding follows the cells
/// it takes where it is put ([`field_count`]) instead of its bytes. The format is a unit of text: a brace
/// `@{…}` it opened (in its fields too) and left open is closed where it ends ([`end_unit`]), so
/// text after it is not in its colour; a brace open before it stays open through it.
pub async fn printf(state: &RefCell<State>, io: &Io, fmt: &[u8], args: &[Arg<'_>]) {
    let (wide, opened) = {
        let t = &state.borrow().ui.text;
        if !t.on {
            return;
        }
        (t.wide, t.braces)
    };
    let all = args;
    let mut args = args.iter().copied();
    let mut f = fmt.iter().copied().take_while(|&b| b != 0);
    while let Some(c) = f.next() {
        if c != b'%' {
            text_out(state, io, i32::from(c)).await;
            continue;
        }
        let pos = if wide { position(&mut f) } else { None };
        let mut next = || f.next().unwrap_or(0);
        let (mut width, mut pad, mut left) = (0i16, b' ', false);
        let mut d = next();
        while d == b'0' || d == b'-' {
            if d == b'-' {
                left = true;
            } else {
                pad = b'0';
            }
            d = next();
        }
        while d.is_ascii_digit() {
            width = width.wrapping_mul(10).wrapping_add(i16::from(d - b'0'));
            d = next();
        }
        let mut field: Vec<u8> = Vec::new();
        let number = |v: u16, max: i16, base: i16| {
            state.borrow_mut().ui.text.plural = v != 1;
            itoa(v, max, base)
        };
        let mut arg = || match pos {
            Some(n) => all.get(n - 1).copied(),
            None => args.next(),
        };
        match d.to_ascii_lowercase() {
            b'c' => {
                let ch = arg().map_or(0, Arg::word) as u8;
                if ch != 0 {
                    field.push(ch);
                }
            }
            b'd' => field = number(arg().map_or(0, Arg::word), 6, -10),
            b'o' => field = number(arg().map_or(0, Arg::word), 6, 8),
            b'u' => field = number(arg().map_or(0, Arg::word), 5, 10),
            b'x' => field = number(arg().map_or(0, Arg::word), 4, 16),
            b's' => {
                let a = arg().unwrap_or(Arg::Str(&[]));
                field = a.bytes().iter().copied().take_while(|&b| b != 0).collect();
            }
            _ => put_raw(state, io, i32::from(d)).await,
        }
        let padding = if d.eq_ignore_ascii_case(&b'c') {
            width
        } else if wide {
            field_count(&state.borrow(), &field, left, width, pad).0
        } else {
            width.wrapping_sub(field.len() as i16)
        };
        if left {
            puts(state, io, &field).await;
        }
        put_raw_n(state, io, padding, i32::from(pad)).await;
        if !left {
            puts(state, io, &field).await;
        }
    }
    end_unit(state, io, opened).await;
}

/// Port-only (spec 08 Design 1): where a unit of text ends (a `printf` format, a conversation's
/// text run, what one conversation op puts). First what the assembler and the josa resolver
/// still hold (a UTF-8 prefix, a marker not complete) is let out unchanged ([`release`]), in the
/// colour in effect there, so nothing joins the next unit, and an `@` that ended it is no
/// longer followed at once ([`TextState::after_at`] ends), so the next unit's `{` opens no
/// brace (with `wide` off nothing reads it). Then a brace still open that the unit opened, one
/// taken since `opened` ([`TextState::braces`] where the unit began), is closed and the text ink
/// comes back. A brace opened before the unit (by text put a byte at a time) is not the unit's
/// and stays open.
///
/// [`TextState::braces`]: super::textstate::TextState::braces
/// [`TextState::after_at`]: super::textstate::TextState::after_at
pub async fn end_unit(state: &RefCell<State>, io: &Io, opened: u32) {
    release(state, io).await;
    let mut s = state.borrow_mut();
    let text = s.ui.tables.colours.text;
    let t = &mut s.ui.text;
    t.after_at = false;
    if t.brace && t.braces != opened {
        t.brace = false;
        t.win_mut().fg = text;
    }
}

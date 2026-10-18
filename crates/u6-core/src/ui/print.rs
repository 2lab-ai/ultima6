//! Text output: the line assembler 0x107e5a, the escape layer 0x108376, `printf` 0x108658 and
//! their helpers. All of them can wait for a key (the more prompt, `*`), so they are async and
//! take the shared state; no borrow is held across an await.
//!
//! Port (spec 08): a character is an `i32` cell code ([`super::wide`]), negative still the one
//! control value (flush without a newline); the line holds `u16` codes.

use std::cell::RefCell;

use super::text::{self, CENTRE, PAGED, RUNIC, SCROLL, WRAP, WindowId};
use super::textstate::TRANSCRIPT;
use super::wide;
use crate::engine::Io;
use crate::state::State;

const NL: i32 = 0x0a;
const SPACE: i32 = 0x20;
/// The space and the hyphen as cell codes (the word-wrap tests).
const SP: u16 = 0x20;
const HYPHEN: u16 = 0x2d;
/// Keys that end the more prompt (0x107f52..0x107f68): space, return, escape, 0x8f.
const MORE_KEYS: [u16; 4] = [0x20, 0x0d, 0x1b, 0x8f];

fn is_lower(c: i32) -> bool {
    (0x61..=0x7a).contains(&c)
}

/// Draws `c` at window-relative cell (`col`, `row`) of the current window, in its colours.
fn cell(s: &mut State, io: &Io, c: u16, col: u8, row: u16) {
    let ui = &s.ui;
    let w = *ui.text.win();
    let x = u16::from(w.x) + u16::from(col);
    let (font, hangul) = (ui.font.as_ref(), ui.hangul.as_deref());
    io.draw(|fb| text::draw_char(fb, font, hangul, &w, c, x, row));
}

/// The code a line cell holds for `c` (codes past `u16` are blank cells; none is put).
fn code(c: i32) -> u16 {
    u16::try_from(c).unwrap_or(u16::MAX)
}

/// Port-side transcript of the message scroll: a finished line goes to `UiState::messages`.
fn transcript_end(s: &mut State) {
    let ui = &mut s.ui;
    if ui.text.current == WindowId::Messages {
        let line: String = ui
            .text
            .transcript_line
            .drain(..)
            .map(wide::cell_char)
            .collect();
        ui.messages.push(line);
        let n = ui.messages.len();
        if n > TRANSCRIPT {
            ui.messages.drain(..n - TRANSCRIPT);
        }
    }
}

/// The more prompt of a paged window (0x107f2e..0x107fb4): idle-cursor glyphs from 1, wait for
/// one of [`MORE_KEYS`], blank the window's last row, rearm the row count.
async fn more(state: &RefCell<State>, io: &Io) {
    let last = {
        let mut s = state.borrow_mut();
        let w = *s.ui.text.win();
        s.ui.input.cursor_glyph = 1;
        s.ui.input.accept = false;
        u16::from(w.height) + u16::from(w.y) - 1
    };
    loop {
        let k = Box::pin(super::wait::read_key(state, io)).await;
        if MORE_KEYS.contains(&k) {
            break;
        }
    }
    let mut s = state.borrow_mut();
    s.ui.input.cursor_glyph = 5;
    s.ui.input.accept = true;
    s.ui.text.drawn = false;
    let w = *s.ui.text.win();
    for col in 0..w.width {
        cell(&mut s, io, SP, col, last);
    }
    s.ui.text.more_rows = i16::from(w.height) - 1;
}

/// Original: 0x107e5a (ui: put a character into the current window's line: append, or on a full
/// line / newline draw the line, word-wrap, move the cursor, scroll; `c` < 0 flushes the
/// pending part of the line without a newline)
///
/// Port (spec 08): `c` is a cell code. First the escape layer lets out what it holds (bytes of
/// a UTF-8 sequence, a josa marker not complete yet; [`super::printf::release`]), so a put from
/// outside it lands after them, as it would with `wide` off.
pub async fn put_raw(state: &RefCell<State>, io: &Io, c: i32) {
    super::printf::release(state, io).await;
    put_cell(state, io, c).await;
}

/// [`put_raw`] without the release: the escape layer's own puts. The runic bit is only for the
/// characters above space below 0x80 (a cell code past them keeps its glyph). Inside `@{…}`
/// every character takes the alert colour (a drawn row gives the window back its text ink).
pub(super) async fn put_cell(state: &RefCell<State>, io: &Io, c: i32) {
    let mut c = c;
    let paging;
    {
        let mut s = state.borrow_mut();
        let alert = s.ui.tables.colours.alert;
        let t = &mut s.ui.text;
        if !t.on {
            return;
        }
        if t.brace && t.keyword_colour {
            t.win_mut().fg = alert;
        }
        if t.skip_newline && t.win().flags & SCROLL != 0 {
            if c == NL {
                return;
            }
            if c >= 0 {
                t.skip_newline = false;
            }
        }
        t.drawn = false;
        if t.win().flags & text::CAPITALISE != 0 && t.capitalise && is_lower(c) {
            c -= 0x20;
        }
        t.capitalise = false;
        if c == 0 {
            c = NL;
        }
        if t.win().flags & RUNIC != 0 && c > SPACE && c < 0x80 {
            c |= 0x80;
        }
        let w = *t.win();
        if !full(t) && c != NL && c >= 0 {
            if !(w.flags & WRAP != 0 && c == SPACE && !t.nonspace) {
                let n = usize::from(t.line_len);
                t.line_ink[n] = w.fg;
                t.line[n] = code(c);
                t.line_len += 1;
                t.tail.put(c);
            }
            finish(t, c);
            return;
        }
        if t.more_rows != 0 && c >= 0 && w.flags & PAGED != 0 {
            t.more_rows -= 1;
        }
        paging = w.flags & PAGED != 0 && t.more_rows == 0 && c >= 0;
    }
    if paging {
        more(state, io).await;
    }
    let mut s = state.borrow_mut();
    end_line(&mut s, io, c);
}

/// 0x107fb8..0x108262: draw the assembled line, keep a wrapped tail, advance or scroll.
fn end_line(s: &mut State, io: &Io, c: i32) {
    let text_ink = s.ui.tables.colours.text;
    let t = &mut s.ui.text;
    let w = *t.win();
    let len = usize::from(t.line_len);
    let start = usize::from(t.line_start);
    let wrapped = w.flags & WRAP != 0 && full(t) && c >= 0;
    let mut end = len;
    if wrapped {
        t.line[len] = if c == NL { SP } else { code(c) };
        t.line_ink[len] = w.fg;
        if c != NL {
            t.tail.put(c);
        }
        while t.line[end] != SP && !(end >= 1 && t.line[end - 1] == HYPHEN) && end > start {
            end -= 1;
        }
        if end == 0 {
            end = len;
        }
    }
    let row = u16::from(w.y) + u16::from(w.row);
    if w.flags & CENTRE != 0 {
        let pad = (i16::from(w.width) - end as i16) >> 1;
        for col in start as i16..start as i16 + pad {
            cell(s, io, SP, col as u8, row);
        }
    }
    let to_transcript = s.ui.text.current == WindowId::Messages;
    for i in start..end {
        let (ch, ink) = (s.ui.text.line[i], s.ui.text.line_ink[i]);
        s.ui.text.win_mut().fg = ink;
        cell(s, io, ch, i as u8, row);
        if to_transcript {
            s.ui.text.transcript_line.push(ch);
        }
    }
    let t = &mut s.ui.text;
    t.win_mut().fg = text_ink;
    if start < end {
        t.win_mut().col = end as u8;
    }
    if wrapped {
        if t.line[end] == SP {
            end += 1;
        }
        let mut i = end;
        while i <= len {
            t.line[i - end] = t.line[i];
            t.line_ink[i - end] = t.line_ink[i];
            i += 1;
        }
        t.line_start = 0;
        t.line_len = (i - end) as u8;
        if c == SPACE {
            t.nonspace = false;
        }
    } else if c >= 0 {
        t.line_len = 0;
        t.line_start = 0;
        if c != NL {
            t.line_ink[0] = t.win().fg;
            t.line_len = 1;
            t.line[0] = code(c);
            t.tail.put(c);
        }
    }
    if c >= 0 {
        transcript_end(s);
        let t = &mut s.ui.text;
        let w = t.win_mut();
        w.col = 0;
        if i16::from(w.row) < i16::from(w.height) - 1 {
            w.row += 1;
        } else if w.flags & SCROLL != 0 {
            let w = *w;
            t.drawn = true;
            io.draw(|fb| text::scroll_window(fb, &w));
        }
    } else {
        let t = &mut s.ui.text;
        let col = t.win().col;
        t.hidden = false;
        t.line_start = col;
        t.line_len = col;
    }
    finish(&mut s.ui.text, c);
}

/// The line holds a whole window row. The original compares the length with the width for
/// equality; the port also stops at the buffer size, which only a window wider than 40 cells
/// (none exists) could reach.
fn full(t: &super::textstate::TextState) -> bool {
    t.line_len >= t.win().width.min(super::textstate::LINE as u8 - 1)
}

/// 0x108262: every character but a space marks the line as started; text was drawn.
fn finish(t: &mut super::textstate::TextState, c: i32) {
    if c != SPACE {
        t.nonspace = true;
    }
    t.drawn = true;
}

/// Original: 0x10829a (ui: put each character of a string through 0x107e5a, unescaped)
///
/// Port (spec 08): with `wide` on, the string's bytes are assembled first ([`wide::decode`]):
/// its Hangul syllables take a cell each, every other byte is put as itself.
pub async fn put_raw_str(state: &RefCell<State>, io: &Io, s: &[u8]) {
    super::printf::release(state, io).await;
    put_cells(state, io, s).await;
}

/// [`put_raw_str`] without the release (the escape layer's `$` and `#`).
pub(super) async fn put_cells(state: &RefCell<State>, io: &Io, s: &[u8]) {
    let s = &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())];
    if state.borrow().ui.text.wide {
        for c in wide::decode(s) {
            put_cell(state, io, c).await;
        }
    } else {
        for &b in s {
            put_cell(state, io, i32::from(b)).await;
        }
    }
}

/// Original: 0x10827c (ui: put `n` copies of `c` through 0x107e5a)
///
/// Port (spec 08): with `n` ≤ 0 it puts nothing and lets nothing out, so the text on either
/// side still joins (a syllable or a josa marker split across `printf` fields). The copies are
/// padding, not text: the last cell the josa resolver reads stays the one before them.
pub async fn put_raw_n(state: &RefCell<State>, io: &Io, n: i16, c: i32) {
    if n <= 0 {
        return;
    }
    super::printf::release(state, io).await;
    let tail = state.borrow().ui.text.tail;
    for _ in 0..n {
        put_cell(state, io, c).await;
    }
    state.borrow_mut().ui.text.tail = tail;
}

//! Keys while the spell cursor or the book is up: the syllable input of 0x106bbc
//! (0x106e9a..0x107182) with 0x10674e, the book panel keys (0x1077ec..0x1078c4), the cursor
//! step 0x106812 and the page codes 0xa2..0xa5 of 0x10790a (0x107b10..0x107c9e).

use std::cell::RefCell;

use super::book::{MARKER, set_current_spell};
use super::book::{draw_book_page, load_book, page_line, quality, redraw_after_page_step};
use crate::engine::Io;
use crate::main_loop::{a4_string, game_string};
use crate::objects::Slot;
use crate::state::State;
use crate::ui::panels::{select_window, set_cursor};
use crate::ui::print::{put_raw, put_raw_n};
use crate::ui::printf::{printf, text_out};
use crate::ui::text::WindowId;
use crate::ui::wait::REPEAT;

const RETURN: u16 = 0x0d;
const ESCAPE: u16 = 0x1b;
const SPACE: u16 = 0x20;
const DELETE: u16 = 0x7f;
const BACKSPACE: u16 = 0x10e;
const WALK: u16 = 0x80;
const NO_COMMAND: u16 = 0x8f;
/// A4-0x7387: the line of a spell not in the book (0x10674e).
const MSG_NOT_IN_BOOK: u32 = 0x7387;
/// The newline 0x10674e prints after a search (hunk 2).
const MSG_NEWLINE: u32 = 0x0010_4ffc;

/// Bit of letter `c` in a syllable set (`1 << (c - 'A')`, a 68000 long shift: 0 from 32 on).
pub(super) fn letter_bit(c: u8) -> u32 {
    1u32.checked_shl(u32::from(c.wrapping_sub(0x41) & 0x3f))
        .unwrap_or(0)
}

/// Original: 0x10674e (ui: the book's spell whose incantation uses exactly the typed
/// syllables, -1 if none)
///
/// Spells 0..0x7f in the books are tried in order; a spell matches when the set of its
/// letters equals the typed set (A4-0x7a3a), so order and repeats do not matter. No match
/// prints that line (A4-0x7387); a newline follows either way, the typed set
/// is cleared and the cursor mode becomes 1 (the cross-hair).
pub async fn find_typed_spell(state: &RefCell<State>, io: &Io) -> i16 {
    let found = {
        let s = state.borrow();
        let m = &s.magic;
        (0..0x80u16)
            .find(|&n| {
                m.known[usize::from(n >> 4)] & (1 << (n & 0xf)) != 0
                    && m.tables
                        .word(usize::from(n))
                        .iter()
                        .fold(0u32, |a, &c| a | letter_bit(c))
                        == m.typed_mask
            })
            .map_or(-1, |n| n as i16)
    };
    if found < 0 {
        let msg = a4_string(io, MSG_NOT_IN_BOOK);
        printf(state, io, &msg, &[]).await;
    }
    let nl = game_string(io, MSG_NEWLINE);
    printf(state, io, &nl, &[]).await;
    let mut s = state.borrow_mut();
    s.magic.typed_mask = 0;
    s.ui.input.targeting = 1;
    found
}

/// The current spell of the books (the first book's quality, else the second's).
fn book_spell(s: &State) -> u8 {
    let [b1, b2] = s.magic.books;
    let book = if b1 != 0 { b1 } else { b2 };
    quality(&s.objects, Slot::from(book))
}

/// `spell` becomes current: the books, the page redrawn, the line selected, no map target;
/// the key is 0x8e (a target confirmed).
async fn choose(state: &RefCell<State>, io: &Io, spell: i16) -> u16 {
    if spell < 0 {
        state.borrow_mut().ui.input.target = (-1, -1);
        return NO_COMMAND;
    }
    {
        let mut s = state.borrow_mut();
        set_current_spell(&mut s, spell as u8);
        load_book(&mut s);
    }
    draw_book_page(state, io).await;
    let mut s = state.borrow_mut();
    let line = page_line(&s.magic, spell as u16);
    s.ui.input.selection = line;
    s.ui.input.target = (-1, -1);
    REPEAT
}

/// A letter: its syllable is printed in the message scroll and remembered (at most 4). The
/// starting cell of each is kept so a delete can erase it; when the print scrolled the window
/// (the cursor came back above the start on the same row), every kept row moves up one.
async fn add_syllable(state: &RefCell<State>, io: &Io, key: u16) {
    let (syl, n) = {
        let mut s = state.borrow_mut();
        let n = s.magic.typed;
        if n >= 4 {
            return;
        }
        let w = *s.ui.text.win();
        let m = &mut s.magic;
        m.typed_letters[usize::from(n)] = key as u8;
        m.typed_at[usize::from(n)] = (w.col, w.row);
        (m.tables.syllable(key as u8).to_vec(), n)
    };
    printf(state, io, &syl, &[]).await;
    text_out(state, io, 0x20).await;
    put_raw(state, io, -1).await;
    let w = *state.borrow().ui.text.win();
    if w.col >= w.width {
        put_raw(state, io, 0).await;
    }
    let mut s = state.borrow_mut();
    let w = *s.ui.text.win();
    let m = &mut s.magic;
    let (col, row) = m.typed_at[usize::from(n)];
    if w.row == row && w.col < col {
        for at in m.typed_at.iter_mut().take(usize::from(n) + 1) {
            at.1 = at.1.wrapping_sub(1);
        }
    }
    m.typed = n + 1;
}

/// Delete: the last syllable is erased (blanks over it, the cursor back at its start).
async fn remove_syllable(state: &RefCell<State>, io: &Io) {
    let (len, start, w) = {
        let mut s = state.borrow_mut();
        if s.magic.typed == 0 {
            return;
        }
        s.magic.typed -= 1;
        let i = usize::from(s.magic.typed);
        let letter = s.magic.typed_letters[i];
        let len = s.magic.tables.syllable(letter).len() as i16;
        (len, s.magic.typed_at[i], *s.ui.text.win())
    };
    let (col, row) = (i16::from(start.0), i16::from(start.1));
    if i16::from(w.col) > len {
        set_cursor(state, io, i16::from(w.col) - len - 1, i16::from(w.row)).await;
    } else {
        set_cursor(state, io, col, row).await;
    }
    put_raw_n(state, io, len, 0x20).await;
    set_cursor(state, io, col, row).await;
}

/// The spell-cursor keys of 0x106bbc (cursor mode 3 under Cast, 0x106e88..0x107182): letters
/// add syllables, Delete removes one, Space / Escape cancel (0x8f, no target), Return casts
/// the typed spell (0x10674e) or, with none typed, the book's current spell (cursor mode 1),
/// a direction steps the book (0x106812); any other key is dropped (0).
pub async fn spell_key(state: &RefCell<State>, io: &Io, key: u16) -> u16 {
    match key {
        0x41..=0x5a => {
            add_syllable(state, io, key).await;
            0
        }
        BACKSPACE | DELETE => {
            remove_syllable(state, io).await;
            0
        }
        SPACE | ESCAPE => {
            let mut s = state.borrow_mut();
            s.magic.typed_mask = 0;
            s.magic.typed = 0;
            s.ui.input.target = (-1, -1);
            NO_COMMAND
        }
        RETURN => {
            let spell = {
                let mut s = state.borrow_mut();
                let m = &mut s.magic;
                m.typed_mask = m
                    .typed_letters
                    .iter()
                    .take(usize::from(m.typed))
                    .fold(0, |a, &c| a | letter_bit(c));
                if m.typed_mask == 0 {
                    s.ui.input.targeting = 1;
                    Some(i16::from(book_spell(&s)))
                } else {
                    None
                }
            };
            let spell = match spell {
                Some(n) => n,
                None => find_typed_spell(state, io).await,
            };
            choose(state, io, spell).await
        }
        WALK => book_cursor_step(state, io).await,
        _ => 0,
    }
}

/// True when key `key` goes to [`book_panel_key`] rather than the ui's panel keys: the book is
/// up (panel mode 0xa1), no conversation runs, and it is Return or a direction (Space /
/// Escape the ui handles the same way).
pub fn takes_book_key(s: &State, key: u16) -> bool {
    s.ui.panel.mode == super::BOOK_PANEL && !s.converse.active && (key == RETURN || key == WALK)
}

/// The keys of panel mode 0xa1 (0x1077ec..0x1078c4, outside a conversation): Space / Escape
/// give 0x8f with no target, Return casts the book's current spell (cursor mode 1), a
/// direction steps the book; other keys pass unchanged.
pub async fn book_panel_key(state: &RefCell<State>, io: &Io, key: u16) -> u16 {
    match key {
        SPACE | ESCAPE => {
            state.borrow_mut().ui.input.target = (-1, -1);
            NO_COMMAND
        }
        RETURN => {
            let spell = {
                let mut s = state.borrow_mut();
                s.ui.input.targeting = 1;
                i16::from(book_spell(&s))
            };
            choose(state, io, spell).await
        }
        WALK => book_cursor_step(state, io).await,
        _ => key,
    }
}

/// Moves the marker from line `from` to line `to` (status window, column 0) and makes that
/// line's spell current.
async fn move_marker(state: &RefCell<State>, io: &Io, from: i16, to: i16) {
    select_window(state, io, WindowId::Status).await;
    set_cursor(state, io, 0, from + 1).await;
    put_raw(state, io, 0x20).await;
    set_cursor(state, io, 0, to + 1).await;
    put_raw(state, io, MARKER).await;
    select_window(state, io, WindowId::Messages).await;
    let mut s = state.borrow_mut();
    let spell = s.magic.page[(to as usize).min(7)];
    set_current_spell(&mut s, spell);
}

/// Original: 0x106812 (ui: a direction with the book up: east / west turn the page when there
/// is one (0xa4 / 0xa3, A4-0x7a5c set), north / south move the marker one line within the
/// page; else 0)
pub async fn book_cursor_step(state: &RefCell<State>, io: &Io) -> u16 {
    let (dir, line, lines, next, prev) = {
        let s = state.borrow();
        let m = &s.magic;
        let line = page_line(m, m.current_spell()).max(0);
        (
            s.ui.input.direction,
            line,
            i16::from(m.page_lines),
            m.next_page,
            m.prev_page,
        )
    };
    match dir {
        2 if next != 0xff => {
            state.borrow_mut().magic.page_moved = 1;
            0xa4
        }
        6 if prev != 0xff => {
            state.borrow_mut().magic.page_moved = 1;
            0xa3
        }
        0 => {
            if line != 0 {
                move_marker(state, io, line, line - 1).await;
            }
            0
        }
        4 => {
            if line < lines - 1 && line < 7 {
                move_marker(state, io, line, line + 1).await;
            }
            0
        }
        _ => 0,
    }
}

/// Spells of the books in the circles before `circle`.
pub(super) fn spells_before(s: &State, circle: u8) -> u8 {
    let mut n = 0u8;
    for c in 0..=usize::from(circle) {
        for b in 0..16 {
            if s.magic.known.get(c).is_some_and(|w| w >> b & 1 != 0) {
                if c == usize::from(circle) {
                    return n;
                }
                n = n.wrapping_add(1);
            }
        }
    }
    n
}

/// The page codes of 0x10790a with the book up (0x107b10..0x107c9e): 0xa2 / 0xa5 show the
/// first page of the previous / next page's circle (redraw 0x116218); 0xa3 the previous page
/// (8 back within the circle, else the last page of the previous circle), 0xa4 the next page
/// (8 on within the circle, else the next circle's first), both redrawn through 0x1078d2. The
/// key read goes on afterwards (the panel redraw 0x103f64 runs as for every panel code).
pub async fn page_key(state: &RefCell<State>, io: &Io, code: u16) {
    match code {
        0xa2 | 0xa5 => {
            {
                let mut s = state.borrow_mut();
                let circle = if code == 0xa2 {
                    s.magic.prev_circle
                } else {
                    s.magic.next_circle
                };
                s.magic.page_first = spells_before(&s, circle);
            }
            draw_book_page(state, io).await;
        }
        0xa3 => {
            {
                let mut s = state.borrow_mut();
                if s.magic.prev_page == 2 {
                    s.magic.page_first = s.magic.page_first.wrapping_sub(8);
                } else {
                    let old = i16::from(s.magic.page_first);
                    let first = spells_before(&s, s.magic.prev_circle);
                    s.magic.page_first = first;
                    if old - i16::from(first) > 8 {
                        s.magic.page_first = first.wrapping_add(8);
                    }
                }
            }
            redraw_after_page_step(state, io).await;
        }
        0xa4 => {
            {
                let mut s = state.borrow_mut();
                s.magic.page_first = if s.magic.next_page == 2 {
                    s.magic.page_first.wrapping_add(8)
                } else {
                    spells_before(&s, s.magic.next_circle)
                };
            }
            redraw_after_page_step(state, io).await;
        }
        _ => {}
    }
}

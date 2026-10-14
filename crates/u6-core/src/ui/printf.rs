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

use std::cell::RefCell;

use super::print::{put_raw, put_raw_n, put_raw_str};
use super::text::{RUNIC, itoa};
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

fn ch(b: u8) -> i16 {
    i16::from(b)
}

fn is_lower(c: i16) -> bool {
    (0x61..=0x7a).contains(&c)
}

fn is_alpha(c: i16) -> bool {
    is_lower(c) || (0x41..=0x5a).contains(&c)
}

/// `#n` / `$n` index: digits 0..9, letters (either case) 10..35, anything else 0.
fn var_index(c: i16) -> usize {
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
pub async fn text_out(state: &RefCell<State>, io: &Io, c: i16) {
    let prefix = {
        let mut s = state.borrow_mut();
        if !s.ui.text.on {
            return;
        }
        if c != i16::from(b'&') && s.ui.text.hidden && !show_hidden(&s) {
            return;
        }
        let t = &mut s.ui.text;
        match c {
            _ if [b'#', b'$', b'%', b'/', b'@', b'\\', b'^']
                .map(ch)
                .contains(&c) =>
            {
                t.prefix = c as u8;
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
        }
    };
    let Some(prefix) = prefix else {
        put_raw(state, io, -1).await;
        state.borrow_mut().ui.input.cursor_glyph = 1;
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
            put_raw_str(state, io, &itoa(v, 6, -10)).await;
            state.borrow_mut().ui.text.prefix = 0;
        }
        b'$' => {
            let text = crate::converse::string_var(&state.borrow(), var_index(c) as u8);
            put_raw_str(state, io, &text).await;
            state.borrow_mut().ui.text.prefix = 0;
        }
        b'/' | b'\\' => {
            let plural = state.borrow().ui.text.plural;
            // `/` keeps letters for a singular, `\` for a plural.
            if is_alpha(c) && plural == (prefix == b'/') {
                return;
            }
            put_raw(state, io, c).await;
            if !is_alpha(c) {
                state.borrow_mut().ui.text.prefix = 0;
            }
        }
        b'@' => {
            {
                let mut s = state.borrow_mut();
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
            put_raw(state, io, c).await;
        }
        b'^' => {
            let runic = state.borrow().ui.text.win().flags & RUNIC != 0;
            let g = if runic { (c & 0x1f) | 0x80 } else { c & 0x1f };
            put_raw(state, io, g).await;
            state.borrow_mut().ui.text.prefix = 0;
        }
        _ => put_raw(state, io, c).await,
    }
}

/// Original: 0x108642 (ui: put each character of a string through the escape layer)
pub async fn puts(state: &RefCell<State>, io: &Io, s: &[u8]) {
    for &b in s.iter().take_while(|&&b| b != 0) {
        text_out(state, io, i16::from(b)).await;
    }
}

/// Original: 0x108658 (ui: formatted output to the current window)
///
/// Plain characters go through the escape layer; a field's text too (so `%s` strings may carry
/// escapes), its padding unescaped. `%c` leaves the field length at 0, so its padding is the
/// whole width. Every number directive sets the plural flag (`value != 1`).
pub async fn printf(state: &RefCell<State>, io: &Io, fmt: &[u8], args: &[Arg<'_>]) {
    if !state.borrow().ui.text.on {
        return;
    }
    let mut args = args.iter().copied();
    let mut f = fmt.iter().copied().take_while(|&b| b != 0);
    while let Some(c) = f.next() {
        if c != b'%' {
            text_out(state, io, i16::from(c)).await;
            continue;
        }
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
        let mut len = 0i16;
        let number = |v: u16, max: i16, base: i16| {
            state.borrow_mut().ui.text.plural = v != 1;
            itoa(v, max, base)
        };
        match d.to_ascii_lowercase() {
            b'c' => {
                let ch = args.next().map_or(0, Arg::word) as u8;
                if ch != 0 {
                    field.push(ch);
                }
            }
            b'd' => field = number(args.next().map_or(0, Arg::word), 6, -10),
            b'o' => field = number(args.next().map_or(0, Arg::word), 6, 8),
            b'u' => field = number(args.next().map_or(0, Arg::word), 5, 10),
            b'x' => field = number(args.next().map_or(0, Arg::word), 4, 16),
            b's' => {
                let a = args.next().unwrap_or(Arg::Str(&[]));
                field = a.bytes().iter().copied().take_while(|&b| b != 0).collect();
            }
            _ => put_raw(state, io, i16::from(d)).await,
        }
        if !d.eq_ignore_ascii_case(&b'c') {
            len = field.len() as i16;
        }
        if left {
            puts(state, io, &field).await;
        }
        put_raw_n(state, io, width.wrapping_sub(len), i16::from(pad)).await;
        if !left {
            puts(state, io, &field).await;
        }
    }
}

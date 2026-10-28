//! Waiting for a key: the idle cursor poll 0x105c8e, the event wait 0x106bbc (keyboard part)
//! and the command-key read 0x10790a with the panel-navigation codes 0x90..0xa9.

use std::cell::RefCell;

use super::print::put_raw;
use super::seams;
use super::text::{self, PAGED};
use crate::engine::{Io, Key, Typed};
use crate::state::State;

/// What a poll took: a game code (never 0), a Hangul syllable typed as text (port, spec 08
/// slice K5a: only the poll of [`read_words`] with a pack takes one), or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Polled {
    None,
    Code(u16),
    Text(char),
}

/// Port-only (spec 08 Design 4, slice K5a): which characters of text
/// ([`crate::engine::InputEvent::Text`]) a poll takes; it drops the others where it meets them,
/// as if they had not come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Text {
    /// None (every key wait but the line editor's).
    Dropped,
    /// Printable ASCII, as the press of its key (the line editor).
    Ascii,
    /// That and the Hangul syllables U+AC00..=U+D7A3 (the player's words with a pack).
    Hangul,
}

/// Replaces: 0x134e7c (take the next game code, 0 when none is queued; with the drain flag
/// A4-0x7a46 it empties the ring and keeps the last code)
///
/// The port reads the engine queue: a key at its front is consumed and mapped through the raw
/// map 0x151d00; anything else gives 0 (a pointer event in front is the event wait's to take,
/// [`crate::engine::Io::take_pointer`]). The drain is the original's (0x134e84..0x134e98 takes
/// until the ring is empty, the last code kept): the idle poll hands the flag over
/// (0x105d06..0x105d0e loads A4-0x7a46 into D0 before the take), whose word is 1 in the image
/// (0x153caa) and clear only while the line editor reads (0x107cd6, set back at 0x107d5a). So
/// with the flag set the take is the newest key press queued, every other press dropped
/// ([`crate::engine::Io::take_newest_key`]): a held key, whose auto-repeat keeps adding
/// presses, gives one command a poll. The original's ring also holds each release, as a 0
/// (0x151c32), so there a key let go before the poll leaves a 0 last and its press is lost;
/// here (spec 19 O1b) a release takes back only the key's queued repeats
/// ([`crate::engine::InputEvent::Release`]): the held key stops at its release, and a tap
/// during a turn still runs once. Without the flag (the line editor) keys are taken in order.
///
/// Port (spec 08 Design 4, slice K5a): a character of text at the front is taken as `text`
/// says: printable ASCII as the press of its key (through the map, as typed), a Hangul syllable
/// as it is ([`Polled::Text`], never a game code); one it does not take is dropped and the poll
/// goes on to what follows ([`crate::engine::Io::next_typed`]; a reader of keys alone drops
/// text, [`crate::engine::Io::key_ready`]).
async fn poll(state: &RefCell<State>, io: &Io, text: Text) -> Polled {
    let key = loop {
        if text == Text::Dropped {
            if !io.key_ready() {
                return Polled::None;
            }
            if state.borrow().ui.input.drain {
                match io.take_newest_key() {
                    Some(key) => break key,
                    None => return Polled::None,
                }
            }
            break io.next_key().await;
        }
        if !io.typed_ready() {
            return Polled::None;
        }
        match io.next_typed().await {
            Typed::Key(key) => break key,
            Typed::Text(' ') => break Key::Space,
            Typed::Text(c) if c.is_ascii_graphic() => break Key::Char(c),
            Typed::Text(c) if text == Text::Hangul && is_syllable(c) => return Polled::Text(c),
            Typed::Text(_) => {}
        }
    };
    let mut s = state.borrow_mut();
    let ui = &mut s.ui;
    match super::input::key_code(&mut ui.input.qualifiers, &ui.tables.keymap, key) {
        0 => Polled::None,
        code => Polled::Code(code),
    }
}

/// Original: 0x105c8e (ui: poll for a key; while none, show one frame of the spinning cursor at the
/// window's cursor cell)
///
/// The original draws the cursor glyph, refreshes the view (0x102ef0, which presents) and
/// erases it again within one call. The port presents and awaits the next display generation
/// between drawing and erasing: that is the guaranteed yield of every key wait.
///
/// Port (spec 11): the cursor is drawn as a symbol ([`text::draw_symbol`]; over the paper it names
/// itself one, Design 2 rev 3, and the HD font mode lists it for the platform to draw after the
/// text it follows).
pub async fn idle_poll(state: &RefCell<State>, io: &Io) -> u16 {
    match idle_wait(state, io, Text::Dropped).await {
        Polled::Code(code) => code,
        Polled::None | Polled::Text(_) => 0,
    }
}

/// [`idle_poll`], taking the text `text` says ([`poll`]; the line editor's).
async fn idle_wait(state: &RefCell<State>, io: &Io, text: Text) -> Polled {
    let (saved, cell) = {
        let mut s = state.borrow_mut();
        let ink = s.ui.tables.colours.text;
        let ui = &mut s.ui;
        let saved = ui.text.win().fg;
        ui.text.win_mut().fg = ink;
        ui.text.more_rows = i16::from(ui.text.win().height);
        if ui.input.unpage {
            ui.text.win_mut().flags &= !PAGED;
            ui.input.unpage = false;
        }
        ui.text.drawn = false;
        let w = *ui.text.win();
        (saved, w.col >= w.width)
    };
    if cell {
        #[cfg(test)]
        crate::converse::disasm::trace::wait_put();
        put_raw(state, io, 0).await;
    }
    let (col, row) = {
        let s = state.borrow();
        let w = s.ui.text.win();
        (
            u16::from(w.col) + u16::from(w.x),
            u16::from(w.row) + u16::from(w.y),
        )
    };
    let key = poll(state, io, text).await;
    if key == Polled::None {
        let refresh = {
            let mut s = state.borrow_mut();
            let ui = &mut s.ui;
            let frame = ui.input.cursor_frame;
            ui.input.cursor_frame = frame.wrapping_add(1);
            let glyph = (u16::from(frame) & 3) + ui.input.cursor_glyph;
            let w = *ui.text.win();
            let (font, hangul) = (ui.font.as_ref(), ui.hangul.as_deref());
            let glyph = u16::from(glyph as u8);
            // Port (spec 11): a symbol, never a character of the HD font.
            let paper = ui.paper.as_ref();
            io.draw(|fb| text::draw_symbol(fb, (font, hangul), &w, glyph, (col, row), paper));
            let refresh = ui.input.idle_refresh;
            s.gfx.actor_anim_due = 1;
            refresh
        };
        // Port: the original drew every frame where it made it, so a wait shows those recorded
        // frames whose lines are printed and keeps the rest for after their lines; the refresh
        // draws here too, a print's more prompt included, and its frames (a shake request) are
        // played in this wait. Both through an interrupt of whatever print waits here
        // (crate::combat::present::interrupt), which leaves its queue and its held newline
        // alone.
        let outer = crate::combat::present::interrupt(state, io).await;
        {
            let mut s = state.borrow_mut();
            if refresh {
                // 0x105d44 / 0x105d48: 0x102ef0 and the animation step.
                seams::refresh_view(&mut s, io);
                seams::anim_step(&mut s);
            }
            // 0x105d4c: the pointer sample (0x14f12c); the event wait takes the events.
            super::pointer::sample(&mut s, io.pointer());
        }
        crate::combat::present::resume(state, io, outer).await;
        io.present();
        io.vblank().await;
    }
    let mut s = state.borrow_mut();
    let ui = &mut s.ui;
    let w = *ui.text.win();
    let (font, hangul) = (ui.font.as_ref(), ui.hangul.as_deref());
    io.draw(|fb| text::draw_char(fb, font, hangul, &w, 0x20, col, row));
    forget(ui, col, row);
    ui.text.drawn = true;
    ui.text.win_mut().fg = saved;
    key
}

/// Port (spec 22 Contract 4): screen cell (`col`, `row`) of the message window was drawn over:
/// the keyword it held is forgotten.
fn forget(ui: &mut super::UiState, col: u16, row: u16) {
    if ui.text.current == super::WindowId::Messages {
        let w = *ui.text.win();
        let (c, r) = (
            col.saturating_sub(u16::from(w.x)),
            row.saturating_sub(u16::from(w.y)),
        );
        ui.text.keys.forget(r as u8, c as u8);
    }
}

/// Original: 0x105d78 (ui: echo a typed character at the cursor of the line editor: backspace (8)
/// steps back and blanks, anything else is drawn and advances, wrapping and scrolling)
///
/// Port (spec 08 Design 4, slice K5a): `c` is a cell's code, a byte of the game's font or a
/// Hangul syllable (U+AC00..=U+D7A3) the wide path draws from the Hangul table, one cell.
///
/// Port (spec 11 Design 3, rev 3): the cell drawn for a character names it typed
/// ([`text::draw_typed`]), so the HD font mode tells a space the player typed from the game's
/// own; the blank Backspace draws is a space as the game's (the line editor takes the same
/// characters as before).
pub fn echo(s: &mut State, io: &Io, c: u16) {
    let ui = &mut s.ui;
    // Port-side: the scroll transcript shows the typed line as the screen does.
    if ui.text.current == super::WindowId::Messages {
        let t = &mut ui.text.transcript_line;
        if c == 8 {
            t.pop();
        } else {
            t.push(c);
        }
    }
    let w = *ui.text.win();
    let mut col = u16::from(w.col) + u16::from(w.x);
    let mut row = u16::from(w.row) + u16::from(w.y);
    ui.text.drawn = false;
    let (font, hangul) = (ui.font.as_ref(), ui.hangul.as_deref());
    if c == 8 {
        let win = ui.text.win_mut();
        if win.col != 0 {
            win.col -= 1;
            col -= 1;
        } else if win.row != 0 {
            win.row -= 1;
            row -= 1;
            win.col = win.width.wrapping_sub(1);
            col = u16::from(win.col) + u16::from(win.x);
        }
        let w = *win;
        io.draw(|fb| text::draw_char(fb, font, hangul, &w, 0x20, col, row));
        forget(ui, col, row);
    } else {
        io.draw(|fb| text::draw_typed(fb, (font, hangul), &w, c, (col, row)));
        forget(ui, col, row);
        let win = ui.text.win_mut();
        win.col = win.col.wrapping_add(1);
        if win.col >= win.width {
            win.col = 0;
            if i16::from(win.row) < i16::from(win.height) - 1 {
                win.row += 1;
            } else if win.flags & text::SCROLL != 0 {
                ui.text.drawn = true;
                if ui.text.current == super::WindowId::Messages {
                    super::talk::scroll(ui);
                }
                let w = *ui.text.win();
                io.draw(|fb| text::scroll_window(fb, &w, ui.paper.as_ref()));
            }
        }
    }
    ui.text.drawn = true;
}

/// Original: 0x107cba (ui: line editor: read up to `max` printable characters into a line with
/// echo, backspace, return; escape erases the echo and gives -1)
///
/// Port (spec 08 Design 4, slice K5a): a character of text ([`crate::engine::InputEvent::Text`])
/// that is printable ASCII is taken as the press of its key would be (through the keymap);
/// any other is dropped where it is met, as if it had not come ([`poll`]). The player's words
/// in a conversation take Hangul too ([`read_words`]).
pub async fn read_line(state: &RefCell<State>, io: &Io, max: i16) -> (i16, Vec<u8>) {
    edit_line(state, io, max, false, false).await
}

/// Port-only (spec 08 Design 4, slice K5a): [`read_line`] for what the player says in a
/// conversation (ops `f7`, `f9`, `fb`). With a locale pack installed, a Hangul syllable typed as
/// text (U+AC00..=U+D7A3) is taken too: the line holds it as UTF-8, its 3 bytes counted against
/// `max`, which stays a byte budget (the most the original's read stores), so a syllable that
/// would pass it is not taken; it echoes as one cell drawn from the Hangul table, and Backspace
/// takes the whole syllable back. Without a pack it is [`read_line`].
pub async fn read_words(state: &RefCell<State>, io: &Io, max: i16) -> (i16, Vec<u8>) {
    let hangul = state.borrow().locale.pack.is_some();
    edit_line(state, io, max, hangul, false).await
}

/// Port-only (spec 22): [`read_words`] for what the player says after "you say" (op `f7`), with
/// the conversation's buttons and keywords to press ([`super::talk`]).
pub async fn read_said(state: &RefCell<State>, io: &Io, max: i16) -> (i16, Vec<u8>) {
    let hangul = state.borrow().locale.pack.is_some();
    edit_line(state, io, max, hangul, true).await
}

/// Whether `c` is a Hangul syllable (U+AC00..=U+D7A3), the one kind of text beyond ASCII the
/// line editor takes ([`read_words`]).
fn is_syllable(c: char) -> bool {
    ('\u{ac00}'..='\u{d7a3}').contains(&c)
}

/// The cells of the line `line` holds (each a byte of ASCII or a UTF-8 syllable).
fn cells(line: &[u8]) -> usize {
    line.iter().filter(|&&b| b & 0xc0 != 0x80).count()
}

/// [`read_line`] (`hangul`: [`read_words`] with a pack; `talk`: [`read_said`]).
async fn edit_line(
    state: &RefCell<State>,
    io: &Io,
    max: i16,
    hangul: bool,
    talk: bool,
) -> (i16, Vec<u8>) {
    {
        let mut s = state.borrow_mut();
        s.ui.input.line_input = true;
        s.ui.input.upcase = false;
    }
    put_raw(state, io, -1).await;
    if talk {
        super::talk::begin(&mut state.borrow_mut(), io);
    }
    let mut line: Vec<u8> = Vec::new();
    let mut n: i16 = 0;
    state.borrow_mut().ui.input.drain = false;
    loop {
        let text = if hangul { Text::Hangul } else { Text::Ascii };
        let typed = loop {
            // Port: nothing here reads the pointer; its queued events are applied (as the
            // handler would) so the keys behind them arrive. Port (spec 22): but the buttons and
            // keywords of `talk`.
            if talk {
                let typed = cells(&line[..n as usize]);
                super::talk::pointer(&mut state.borrow_mut(), io, typed);
            } else {
                io.take_pointer();
            }
            let typed = idle_wait(state, io, text).await;
            if typed != Polled::None {
                break typed;
            }
        };
        let mut s = state.borrow_mut();
        if talk {
            super::talk::cool(&mut s, io);
        }
        let k = match typed {
            Polled::Code(k) => k,
            Polled::Text(c) => {
                let mut utf8 = [0; 4];
                let bytes = c.encode_utf8(&mut utf8).as_bytes();
                if i32::from(n) + bytes.len() as i32 <= i32::from(max) {
                    line.truncate(n as usize);
                    line.extend_from_slice(bytes);
                    n += bytes.len() as i16;
                    echo(&mut s, io, c as u16);
                }
                continue;
            }
            Polled::None => continue,
        };
        match k {
            0x0d => {
                line.truncate(n.max(0) as usize);
                s.ui.input.line_cancelled = false;
                break;
            }
            0x1b => {
                // One cell erased per character typed (a syllable is one cell of three bytes).
                for _ in 0..cells(&line[..n as usize]) {
                    echo(&mut s, io, 8);
                }
                line.clear();
                n = -1;
                s.ui.input.line_cancelled = true;
                break;
            }
            0x10e => {
                if n != 0 {
                    // The whole last character (a syllable's three bytes), one cell.
                    let at = line[..n as usize]
                        .iter()
                        .rposition(|&b| b & 0xc0 != 0x80)
                        .unwrap_or(0);
                    n = at as i16;
                    line.truncate(n as usize);
                    echo(&mut s, io, 8);
                }
            }
            _ => {
                if n < max && (0x20..=0x7e).contains(&k) {
                    line.truncate(n as usize);
                    line.push(k as u8);
                    n += 1;
                    echo(&mut s, io, k);
                }
            }
        }
    }
    let mut s = state.borrow_mut();
    if talk {
        super::talk::end(&mut s, io);
    }
    let ui = &mut s.ui;
    ui.input.drain = true;
    ui.text.more_rows = i16::from(ui.text.win().height);
    ui.input.upcase = true;
    ui.input.line_input = false;
    (n, line)
}

//! Waiting for a key: the idle cursor poll 0x105c8e, the event wait 0x106bbc (keyboard part)
//! and the command-key read 0x10790a with the panel-navigation codes 0x90..0xa9.

use std::cell::RefCell;

use super::print::put_raw;
use super::seams;
use super::text::{self, PAGED};
use crate::engine::Io;
use crate::state::State;

/// Replaces: 0x134e7c (take the next game code, 0 when none is queued; with the drain flag
/// A4-0x7a46 it empties the ring and keeps the last code)
///
/// The port reads the engine queue: one pending event is consumed and mapped through the raw
/// map 0x151d00; nothing pending gives 0. The drain is not applied: the engine contract (spec
/// 03 §1) consumes input in order, and the platform delivers presses only (no key-up codes
/// that the drain would otherwise skip past).
async fn poll(state: &RefCell<State>, io: &Io) -> u16 {
    if io.pending_events() == 0 {
        return 0;
    }
    let key = io.next_key().await;
    let mut s = state.borrow_mut();
    let ui = &mut s.ui;
    super::input::key_code(&mut ui.input.qualifiers, &ui.tables.keymap, key)
}

/// Original: 0x105c8e (ui: poll for a key; while none, show one frame of the spinning cursor at the
/// window's cursor cell)
///
/// The original draws the cursor glyph, refreshes the view (0x102ef0, which presents) and
/// erases it again within one call. The port presents and awaits the next display generation
/// between drawing and erasing: that is the guaranteed yield of every key wait.
pub async fn idle_poll(state: &RefCell<State>, io: &Io) -> u16 {
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
    let key = poll(state, io).await;
    if key == 0 {
        {
            let mut s = state.borrow_mut();
            let ui = &mut s.ui;
            let frame = ui.input.cursor_frame;
            ui.input.cursor_frame = frame.wrapping_add(1);
            let glyph = (u16::from(frame) & 3) + ui.input.cursor_glyph;
            let w = *ui.text.win();
            io.draw(|fb| text::draw_char(fb, ui.font.as_ref(), &w, glyph as u8, col, row));
            let refresh = ui.input.idle_refresh;
            s.gfx.actor_anim_due = 1;
            if refresh {
                seams::refresh_view(&mut s, io);
                seams::anim_step(&mut s);
            }
        }
        io.present();
        io.vblank().await;
    }
    let mut s = state.borrow_mut();
    let ui = &mut s.ui;
    let w = *ui.text.win();
    io.draw(|fb| text::draw_char(fb, ui.font.as_ref(), &w, b' ', col, row));
    ui.text.drawn = true;
    ui.text.win_mut().fg = saved;
    key
}

/// Original: 0x105d78 (ui: echo a typed character at the cursor of the line editor: backspace (8)
/// steps back and blanks, anything else is drawn and advances, wrapping and scrolling)
pub fn echo(s: &mut State, io: &Io, c: u8) {
    let ui = &mut s.ui;
    let w = *ui.text.win();
    let mut col = u16::from(w.col) + u16::from(w.x);
    let mut row = u16::from(w.row) + u16::from(w.y);
    ui.text.drawn = false;
    let font = ui.font.as_ref();
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
        io.draw(|fb| text::draw_char(fb, font, &w, b' ', col, row));
    } else {
        io.draw(|fb| text::draw_char(fb, font, &w, c, col, row));
        let win = ui.text.win_mut();
        win.col = win.col.wrapping_add(1);
        if win.col >= win.width {
            win.col = 0;
            if i16::from(win.row) < i16::from(win.height) - 1 {
                win.row += 1;
            } else if win.flags & text::SCROLL != 0 {
                ui.text.drawn = true;
                let w = *ui.text.win();
                io.draw(|fb| text::scroll_window(fb, &w));
            }
        }
    }
    ui.text.drawn = true;
}

/// Original: 0x107cba (ui: line editor: read up to `max` printable characters into a line with
/// echo, backspace, return; escape erases the echo and gives -1)
pub async fn read_line(state: &RefCell<State>, io: &Io, max: i16) -> (i16, Vec<u8>) {
    {
        let mut s = state.borrow_mut();
        s.ui.input.line_input = true;
        s.ui.input.upcase = false;
    }
    put_raw(state, io, -1).await;
    let mut line: Vec<u8> = Vec::new();
    let mut n: i16 = 0;
    state.borrow_mut().ui.input.drain = false;
    loop {
        let k = loop {
            let k = idle_poll(state, io).await;
            if k != 0 {
                break k;
            }
        };
        let mut s = state.borrow_mut();
        match k {
            0x0d => {
                line.truncate(n.max(0) as usize);
                s.ui.input.line_cancelled = false;
                break;
            }
            0x1b => {
                line.clear();
                while n != 0 {
                    n -= 1;
                    echo(&mut s, io, 8);
                }
                n = -1;
                s.ui.input.line_cancelled = true;
                break;
            }
            0x10e => {
                if n != 0 {
                    n -= 1;
                    line.truncate(n as usize);
                    echo(&mut s, io, 8);
                }
            }
            _ => {
                if n < max && (0x20..=0x7e).contains(&k) {
                    line.truncate(n as usize);
                    line.push(k as u8);
                    n += 1;
                    echo(&mut s, io, k as u8);
                }
            }
        }
    }
    let mut s = state.borrow_mut();
    let ui = &mut s.ui;
    ui.input.drain = true;
    ui.text.more_rows = i16::from(ui.text.win().height);
    ui.input.upcase = true;
    ui.input.line_input = false;
    (n, line)
}

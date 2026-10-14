//! Unit tests of the text layer, input map and panels, on synthetic tables and a synthetic font
//! (glyph `c` = eight rows of the byte `c`, so a drawn cell can be read back).

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use u6_formats::font::BitFont;
use u6_render::Framebuffer;

use super::tables::{Colours, Initial};
use super::text::{CELL, itoa};
use super::*;
use crate::{Engine, Files, InputEvent, Io, Key, State};

mod input_tests;
mod panel_tests;
mod real_tests;
mod text_tests;

pub(super) const INK: u8 = 1;
pub(super) const PAPER: u8 = 2;
pub(super) const ALERT: u8 = 3;

/// Glyph `c` has every row equal to `c`.
pub(super) fn font() -> BitFont {
    BitFont {
        glyphs: (0..=255u8).map(|c| [c; 8]).collect(),
    }
}

/// Synthetic tables: a few raw keys, colours 1..5, synthetic panel strings.
pub(super) fn tables() -> UiTables {
    let mut keymap = vec![[0u16; 4]; tables::RAW_KEYS];
    keymap[0x20] = [0x61, 0x41, 0x01, 0x8e];
    keymap[0x21] = [0x73, 0x53, 0x13, 0x8f];
    keymap[0x01] = [0x31, 0x21, 0, 0x178];
    keymap[0x1d] = [0xbf, 0, 0, 0x14f];
    keymap[0x1e] = [0xc0, 0, 0, 0x150];
    keymap[0x40] = [0x20; 4];
    keymap[0x44] = [0x0d, 0x0d, 0x0d, 0];
    keymap[0x45] = [0x1b; 4];
    keymap[0x41] = [0x10e, 0x10e, 0, 0];
    keymap[0x4c] = [0x148, 0x148, 0x148, 0];
    keymap[0x4e] = [0x14d, 0x14d, 0x14d, 0];
    keymap[0x0b] = [0x2d, 0x5f, 0, 0];
    keymap[0x0c] = [0x3d, 0x2b, 0, 0];
    keymap[0x3a] = [0x2f, 0x3f, 0, 0];
    keymap[0x5d] = [0x2a, 0, 0, 0];
    let mut t = UiTables {
        keymap,
        keyword_end: b" .".to_vec(),
        colours: Colours {
            text: INK,
            paper: PAPER,
            alert: ALERT,
            poisoned: 4,
            command: 5,
        },
        initial: Initial {
            text_on: true,
            cursor_glyph: 5,
            key_drain: true,
            keyword_colour: true,
            window_flags: 0,
            accept: true,
            upcase: true,
            target_command: globals::NO_COMMAND,
            panel_cursor_col: 3,
            idle_refresh: false,
            panel_mode: 0x91,
            panel_dirty: true,
        },
        ..UiTables::default()
    };
    t.step = [
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
    ];
    let p = &mut t.panel;
    p.roster_hp = b"%3d\n".to_vec();
    p.name_line = b"%s\n".to_vec();
    p.plain = b"%s".to_vec();
    p.attributes = [b"a%d\n".to_vec(), b"b%d\n".to_vec(), b"c%d\n".to_vec()];
    p.magic = b"mm".to_vec();
    p.magic_value = b"%2d%/%d".to_vec();
    p.health = b"hh".to_vec();
    p.hp_value = b"%3d".to_vec();
    p.hp_max = b"%/%d".to_vec();
    p.level_exp = b"l%/x".to_vec();
    p.level_exp_value = b"%d%/%d".to_vec();
    p.weights = [b"e".to_vec(), b"i".to_vec()];
    p.combat_modes = (2..=8).map(|m| format!("m{m}").into_bytes()).collect();
    t
}

/// A state with the synthetic ui, main's three windows, the message scroll current.
pub(super) fn state() -> State {
    let mut s = State::empty();
    s.ui = UiState::from_parts(Some(font()), tables());
    let t = s.ui.tables.clone();
    s.ui.text.main_windows(&t);
    // Party mode (no solo member) and no command underlined, as in the shipped objlist.
    s.actors.objlist_tail[0x79] = 0xff;
    s.actors.objlist_tail[0x7b] = 0xff;
    s
}

/// Runs `game` until it finishes (at most `ticks` ticks), queuing `keys` on the first tick.
pub(super) fn run<G, F>(s: State, keys: &[Key], ticks: usize, game: G) -> Engine
where
    G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F,
    F: Future<Output = ()> + 'static,
{
    let mut e = Engine::with_game(s, Files::default(), game);
    let events: Vec<InputEvent> = keys.iter().map(|&k| InputEvent::Key(k)).collect();
    e.tick(&events, 16);
    for _ in 1..ticks {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    e
}

/// The character drawn at screen cell (col, row) in ink `fg` (glyph rows are the code).
pub(super) fn cell(fb: &Framebuffer, col: usize, row: usize, fg: u8) -> u8 {
    (0..CELL).fold(0, |c, dx| {
        let on = fb.get(col * CELL + dx, row * CELL) == Some(fg & 0x0f);
        (c << 1) | u8::from(on)
    })
}

/// The characters of screen row `row`, columns `cols`, in ink `fg` (0 shown as ' ').
pub(super) fn row_text(
    fb: &Framebuffer,
    cols: std::ops::Range<usize>,
    row: usize,
    fg: u8,
) -> String {
    cols.map(|c| match cell(fb, c, row, fg) {
        0 => ' ',
        b => char::from(b),
    })
    .collect()
}

/// Prints `fmt` with `args` into the message scroll, flushes and presents.
pub(super) fn print(s: State, fmt: &'static [u8], args: &'static [Arg<'static>]) -> Engine {
    run(s, &[], 4, move |st, io| async move {
        printf(&st, &io, fmt, args).await;
        print::put_raw(&st, &io, -1).await;
        io.present();
    })
}

/// Screen row `row` of the scroll as drawn (columns 22..39).
pub(super) fn scroll_row(e: &Engine, row: usize) -> String {
    row_text(e.frame(), 22..39, row, INK)
}

#[test]
fn itoa_digits_sign_and_truncation() {
    assert_eq!(itoa(0, 6, -10), b"0");
    assert_eq!(itoa(42, 6, -10), b"42");
    assert_eq!(itoa(0xffff, 6, -10), b"-1");
    assert_eq!(itoa(0x8000, 6, -10), b"-32768");
    // Unsigned base: 0xffff is 65535; three digits keep the low ones.
    assert_eq!(itoa(0xffff, 5, 10), b"65535");
    assert_eq!(itoa(65535, 3, 10), b"535");
    // The sign costs a digit: -12345 with 5 allowed keeps four.
    assert_eq!(itoa(12345u16.wrapping_neg(), 5, -10), b"-2345");
    assert_eq!(itoa(0xbeef, 4, 16), b"BEEF");
    assert_eq!(itoa(8, 6, 8), b"10");
}

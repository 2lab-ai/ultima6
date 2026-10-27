//! Port-only (spec 22): conversation by click. While the conversation reads what the player says
//! (op f7), a click on one of two buttons below the portrait types the script's `job` or `name`
//! keyword, and a click on a keyword the message scroll shows in the alert colour types that
//! keyword: each as the keys the keyboard would send, put at the front of the engine's queue for
//! the line editor ([`crate::Io::push_front`]). The button or keyword under the pointer is drawn
//! inverse. All of it is drawn by the engine, cell by cell with the game's glyph renderer
//! ([`text::draw_char`], 0x105228) in the game's colours; the page only sends the pointer.

use super::text::{self, CELL, TextWindow, WindowId, fill_rect};
use super::wide;
use crate::engine::{InputEvent, Io, Key, POINTER_LEFT};
use crate::state::State;

/// Most cells a button's word is shown in (a pack's word is at most 8 syllables, spec 08 Design
/// 4).
const WORD_CELLS: usize = 8;
/// A blank cell (the space's glyph).
const BLANK: u16 = 0x20;

/// One keyword the message window shows in the alert colour: a run of cells the line assembler
/// drew on one row ([`KeySpans`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeySpan {
    /// The window row (from 0).
    pub row: u8,
    /// The window column of its first cell (from 0).
    pub col: u8,
    /// Its cell codes, left to right, as drawn.
    pub cells: Vec<u16>,
    /// Their ink.
    pub ink: u8,
}

impl KeySpan {
    /// The word it types (UTF-8): its cells as typed ([`typed`]), without the spaces at its end.
    pub fn word(&self) -> Vec<u8> {
        let mut w = typed(&self.cells);
        while w.last() == Some(&b' ') {
            w.pop();
        }
        w
    }

    fn covers(&self, row: u8, col: u8) -> bool {
        self.row == row && col >= self.col && usize::from(col - self.col) < self.cells.len()
    }
}

/// The keywords the message window shows (`TextState::keys`): recorded as the line assembler
/// draws alert cells there ([`KeySpans::draw`]), forgotten where a cell is drawn over them
/// ([`KeySpans::forget`]), moved by its scroll ([`KeySpans::scroll`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeySpans(pub Vec<KeySpan>);

impl KeySpans {
    /// Cell `c` drawn in `ink` at window cell (`row`, `col`), the span under it forgotten
    /// first: it ends the span of that ink just before it on the row, or starts one unless it is
    /// a space (the more prompt blanks its row in the ink of a keyword it broke).
    pub fn draw(&mut self, row: u8, col: u8, c: u16, ink: u8) {
        let next = |s: &&mut KeySpan| {
            s.row == row && s.ink == ink && usize::from(s.col) + s.cells.len() == usize::from(col)
        };
        match self.0.iter_mut().find(next) {
            Some(s) => s.cells.push(c),
            None if c != BLANK => self.0.push(KeySpan {
                row,
                col,
                cells: vec![c],
                ink,
            }),
            None => {}
        }
    }

    /// The cell at window cell (`row`, `col`) is drawn over: the span holding it goes.
    pub fn forget(&mut self, row: u8, col: u8) {
        self.0.retain(|s| !s.covers(row, col));
    }

    /// The window scrolled its cells up a row (0x10525c): every span moves up with them; those
    /// of the top row leave the window and are returned, in order.
    pub fn scroll(&mut self) -> Vec<KeySpan> {
        let (gone, kept): (Vec<KeySpan>, Vec<KeySpan>) = std::mem::take(&mut self.0)
            .into_iter()
            .partition(|s| s.row == 0);
        self.0 = kept
            .into_iter()
            .map(|s| KeySpan {
                row: s.row - 1,
                ..s
            })
            .collect();
        gone
    }

    /// The span holding window cell (`row`, `col`).
    pub fn at(&self, row: u8, col: u8) -> Option<&KeySpan> {
        self.0.iter().find(|s| s.covers(row, col))
    }

    /// Forgets every span (the window set up again).
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// A button below the portrait ([`begin`]), on the Status window's last row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    /// The screen column of its first cell.
    pub col: u8,
    /// Its cells: the word shown, between its blank cells.
    pub cells: Vec<u16>,
    /// The word it types (UTF-8).
    pub word: Vec<u8>,
}

/// What a click can reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// [`TalkClicks::buttons`]`[i]`.
    Button(usize),
    /// The span of the message window that starts at window cell (`row`, `col`).
    Key { row: u8, col: u8 },
}

/// The buttons shown, the target drawn inverse and the one the left button went down on, while
/// the conversation reads the player's words (`UiState::talk`); empty at every other time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TalkClicks {
    pub buttons: Vec<Button>,
    pub hot: Option<Target>,
    pub pressed: Option<Target>,
}

/// The cells `cells` as typed (UTF-8): an ASCII cell as itself, a runic glyph (0x80 | c) as its
/// letter, a Hangul syllable as its character; any other cell is left out.
fn typed(cells: &[u16]) -> Vec<u8> {
    let mut out = String::new();
    for &c in cells {
        match c {
            0x20..=0x7e => out.push(char::from(c as u8)),
            0xa0..=0xfe => out.push(char::from((c & 0x7f) as u8)),
            _ => {
                let ch = char::from_u32(u32::from(c)).filter(|&ch| wide::is_hangul(ch as i32));
                out.extend(ch);
            }
        }
    }
    out.into_bytes()
}

/// The cells a word shows in: its codes through the UTF-8 assembler ([`wide::decode`]), at most
/// [`WORD_CELLS`].
fn shown(word: &[u8]) -> Vec<u16> {
    wide::decode(word)
        .into_iter()
        .filter_map(|c| u16::try_from(c).ok())
        .take(WORD_CELLS)
        .collect()
}

/// The two buttons' words (shown, typed): the first word the locale pack gives the keyword for
/// the script loaded ([`crate::locale::Pack::aliases`]), else the keyword; typed as it is, shown
/// with its first letter a capital when it is a lower-case ASCII one.
fn words(s: &State) -> [(Vec<u16>, Vec<u8>); 2] {
    // The buttons' keywords, as the scripts hold them (and `keywords()` keys a pack's aliases).
    let keywords: [&[u8]; 2] = [b"job", b"name"];
    keywords.map(|k| {
        let word = s
            .locale
            .pack
            .as_ref()
            .zip(s.converse.script_key)
            .and_then(|(p, key)| p.aliases(key, k)?.first().cloned())
            .unwrap_or_else(|| k.to_vec());
        let mut cap = word.clone();
        if let Some(c) = cap.first_mut() {
            *c = c.to_ascii_uppercase();
        }
        (shown(&cap), word)
    })
}

/// The Status window's last row (screen row; 12 for the window main sets up), below the
/// portrait panel's name on its row 10 (0x128ce8), and its first and last pixel columns.
fn button_row(s: &State) -> (u16, i32, i32) {
    let w = s.ui.text.window(WindowId::Status);
    let row = u16::from(w.y) + u16::from(w.height).saturating_sub(1);
    let x0 = i32::from(w.x) * CELL as i32;
    (row, x0, x0 + i32::from(w.width) * CELL as i32 - 1)
}

/// The window record drawing a cell of `ink` on `paper` (only its colours are read).
fn colours(ink: u8, paper: u8) -> TextWindow {
    TextWindow {
        fg: ink,
        bg: paper,
        ..TextWindow::default()
    }
}

/// Draws `cells` from screen cell (`col`, `row`) in `ink` on `paper` with 0x105228.
fn draw_cells(s: &State, io: &Io, cells: &[u16], (col, row): (u16, u16), (ink, paper): (u8, u8)) {
    let w = colours(ink, paper);
    let (font, hangul) = (s.ui.font.as_ref(), s.ui.hangul.as_deref());
    io.draw(|fb| {
        for (i, &c) in cells.iter().enumerate() {
            text::draw_char(fb, font, hangul, &w, c, col + i as u16, row);
        }
    });
}

/// Spec 22 Contract 2: the read of the player's words begins. When every pixel of the Status
/// window's last row is the paper colour (the portrait panel leaves it so), the two buttons are
/// drawn there, centred, one blank cell between them, and recorded; else none.
pub fn begin(s: &mut State, io: &Io) {
    s.ui.talk = TalkClicks::default();
    let (row, x0, x1) = button_row(s);
    let paper = s.ui.tables.colours.paper;
    let y0 = usize::from(row) * CELL;
    let blank = io.draw(|fb| {
        (y0..y0 + CELL).all(|y| (x0..=x1).all(|x| fb.get(x as usize, y) == Some(paper)))
    });
    if !blank || s.ui.font.is_none() {
        return;
    }
    let [(a, ta), (b, tb)] = words(s);
    let width = s.ui.text.window(WindowId::Status).width as usize;
    let pad = usize::from(a.len() + b.len() + 5 <= width);
    let total = a.len() + b.len() + 4 * pad + 1;
    let mut col = (x0 as usize / CELL + width.saturating_sub(total) / 2) as u8;
    for (shown, word) in [(a, ta), (b, tb)] {
        let mut cells = vec![BLANK; pad];
        cells.extend(shown);
        cells.extend(vec![BLANK; pad]);
        let next = col + cells.len() as u8 + 1;
        s.ui.talk.buttons.push(Button { col, cells, word });
        col = next;
    }
    for i in 0..s.ui.talk.buttons.len() {
        draw_target(s, io, Target::Button(i), false);
    }
}

/// Spec 22 Contract 5 and 6, at each pass of the line editor's wait: the pointer events in front
/// of the queue are taken in order ([`Io::take_pointer`], one press or release a take), the
/// target under the pointer drawn inverse after each; a click on one (the left button down on it
/// and up again on it) types its word: one Backspace for each of the `typed` cells the line
/// holds, the word, Enter, at the front of the queue, before any event after the release.
pub fn pointer(s: &mut State, io: &Io, typed: usize) {
    loop {
        let before = io.pointer().buttons & POINTER_LEFT;
        let p = io.take_pointer();
        let target = target_at(s, p.x, p.y);
        hover(s, io, target);
        match (before, p.buttons & POINTER_LEFT) {
            (0, POINTER_LEFT) => s.ui.talk.pressed = target,
            (POINTER_LEFT, 0) => {
                if let Some(t) = s.ui.talk.pressed.take().filter(|&t| Some(t) == target) {
                    let word = word_of(s, t);
                    io.push_front(&keys(typed, &word));
                    return;
                }
            }
            _ => {}
        }
        if !io.pointer_ready() {
            return;
        }
    }
}

/// Spec 22 Contract 6: the target drawn inverse is drawn as it was (before a key is taken).
pub fn cool(s: &mut State, io: &Io) {
    hover(s, io, None);
}

/// Spec 22 Contract 2 and 6: the read ends: the hover goes, the buttons' row is filled with the
/// paper again (0x1521f4, as the portrait panel's clear), and nothing is shown any more.
pub fn end(s: &mut State, io: &Io) {
    cool(s, io);
    if !s.ui.talk.buttons.is_empty() {
        let (row, x0, x1) = button_row(s);
        let y0 = i32::from(row) * CELL as i32;
        let paper = s.ui.tables.colours.paper;
        let map = s.ui.paper.as_ref();
        io.draw(|fb| fill_rect(fb, x0, y0, x1, y0 + CELL as i32 - 1, paper, map));
    }
    s.ui.talk = TalkClicks::default();
}

/// What frame pixel (`x`, `y`) is on: a button's cell, or a keyword's cell in the message window.
pub fn target_at(s: &State, x: i16, y: i16) -> Option<Target> {
    let (col, row) = (u16::try_from(x).ok()? / 8, u16::try_from(y).ok()? / 8);
    if row == button_row(s).0 {
        let i = s.ui.talk.buttons.iter().position(|b| {
            col >= u16::from(b.col) && usize::from(col - u16::from(b.col)) < b.cells.len()
        });
        if let Some(i) = i {
            return Some(Target::Button(i));
        }
    }
    let w = s.ui.text.window(WindowId::Messages);
    let (c, r) = (
        col.checked_sub(u16::from(w.x))?,
        row.checked_sub(u16::from(w.y))?,
    );
    if c >= u16::from(w.width) || r >= u16::from(w.height) {
        return None;
    }
    let span = s.ui.text.keys.at(r as u8, c as u8)?;
    Some(Target::Key {
        row: span.row,
        col: span.col,
    })
}

/// The word target `t` types.
fn word_of(s: &State, t: Target) -> Vec<u8> {
    match t {
        Target::Button(i) => {
            s.ui.talk
                .buttons
                .get(i)
                .map_or_else(Vec::new, |b| b.word.clone())
        }
        Target::Key { row, col } => {
            s.ui.text
                .keys
                .at(row, col)
                .map_or_else(Vec::new, KeySpan::word)
        }
    }
}

/// The keys typing `word` sends after `typed` cells: one Backspace for each, its characters (a
/// space as Space, a Hangul syllable as text, as the page's input method sends it, any other as
/// its key), Enter.
pub fn keys(typed: usize, word: &[u8]) -> Vec<InputEvent> {
    let mut out = vec![InputEvent::Key(Key::Backspace); typed];
    for c in String::from_utf8_lossy(word).chars() {
        out.push(match c {
            ' ' => InputEvent::Key(Key::Space),
            c if wide::is_hangul(c as i32) => InputEvent::Text(c),
            c => InputEvent::Key(Key::Char(c)),
        });
    }
    out.push(InputEvent::Key(Key::Enter));
    out
}

/// Makes `target` the one drawn inverse: the one drawn so before is drawn as it was first.
fn hover(s: &mut State, io: &Io, target: Option<Target>) {
    if s.ui.talk.hot == target {
        return;
    }
    if let Some(old) = s.ui.talk.hot.take() {
        draw_target(s, io, old, false);
    }
    if let Some(t) = target {
        draw_target(s, io, t, true);
        s.ui.talk.hot = Some(t);
    }
}

/// Draws target `t`, inverse when `hot` (its ink and paper swapped): a button's word in the
/// paper colour on cells of the command colour A4-0x6572 (0x106506's mark), a keyword in its ink
/// on the message window's paper.
fn draw_target(s: &State, io: &Io, t: Target, hot: bool) {
    let swap = |(ink, paper): (u8, u8)| if hot { (paper, ink) } else { (ink, paper) };
    match t {
        Target::Button(i) => {
            let Some(b) = s.ui.talk.buttons.get(i) else {
                return;
            };
            let c = s.ui.tables.colours;
            let at = (u16::from(b.col), button_row(s).0);
            draw_cells(s, io, &b.cells, at, swap((c.paper, c.command)));
        }
        Target::Key { row, col } => {
            let Some(k) = s.ui.text.keys.at(row, col) else {
                return;
            };
            let w = s.ui.text.window(WindowId::Messages);
            let at = (
                u16::from(w.x) + u16::from(k.col),
                u16::from(w.y) + u16::from(k.row),
            );
            draw_cells(s, io, &k.cells, at, swap((k.ink, w.bg)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_join_on_a_row_forget_where_drawn_over_and_leave_with_the_top_row() {
        let mut k = KeySpans::default();
        for (i, c) in b"ab".iter().enumerate() {
            k.draw(1, 4 + i as u8, u16::from(*c), 3);
        }
        k.draw(0, 0, u16::from(b'z'), 3);
        k.draw(1, 7, u16::from(b'c'), 3);
        let words: Vec<Vec<u8>> = k.0.iter().map(KeySpan::word).collect();
        assert_eq!(words, [b"ab".to_vec(), b"z".to_vec(), b"c".to_vec()]);
        assert_eq!(k.at(1, 5).map(KeySpan::word), Some(b"ab".to_vec()));
        assert_eq!(k.at(1, 6), None, "between two spans");
        let gone = k.scroll();
        assert_eq!(gone.len(), 1, "the top row's span handed back");
        assert_eq!(k.at(0, 4).map(|s| s.row), Some(0), "a row up");
        k.forget(0, 5);
        assert_eq!(k.at(0, 4), None, "drawn over: forgotten whole");
        assert_eq!(k.0.len(), 1);
        k.clear();
        assert!(k.0.is_empty());
        // Spaces join a span but start none, and its word ends at its last other cell.
        for col in 0..3 {
            k.draw(2, col, BLANK, 3);
        }
        assert!(k.0.is_empty(), "a row of spaces is no keyword");
        for (i, &c) in [0x61, 0x20, 0x62, 0x20].iter().enumerate() {
            k.draw(2, 5 + i as u8, c, 3);
        }
        assert_eq!(k.0.len(), 1);
        assert_eq!(k.0[0].word(), b"a b");
    }

    #[test]
    fn a_span_types_ascii_runic_letters_and_hangul_and_skips_the_rest() {
        let cells = [0x61, 0xe2, 0x20, 0xac00, 0x07, 0x1234];
        assert_eq!(typed(&cells), "ab 가".as_bytes());
        let k = keys(2, "a 가".as_bytes());
        assert_eq!(
            k,
            [
                InputEvent::Key(Key::Backspace),
                InputEvent::Key(Key::Backspace),
                InputEvent::Key(Key::Char('a')),
                InputEvent::Key(Key::Space),
                InputEvent::Text('가'),
                InputEvent::Key(Key::Enter),
            ]
        );
    }
}

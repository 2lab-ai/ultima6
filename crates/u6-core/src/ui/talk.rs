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
/// drew on one row ([`KeySpans`]), or one part of a keyword the word wrap broke over rows.
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
    /// When it is the rest of a keyword the word wrap broke off the row above (it starts at
    /// column 0): the cells the wrap dropped between the two parts, a space or none; `None` for
    /// a keyword's first part.
    pub joined: Option<Vec<u16>>,
    /// The word wrap broke the keyword after this span (it holds its row's last drawn cell): the
    /// span the next row starts with at column 0, when it is a rest ([`KeySpan::joined`]), is
    /// the keyword's next part.
    pub continued: bool,
}

impl KeySpan {
    /// The word its own cells type (UTF-8): its cells as typed ([`typed`]), without the spaces
    /// at its end.
    pub fn word(&self) -> Vec<u8> {
        trimmed(typed(&self.cells))
    }

    fn covers(&self, row: u8, col: u8) -> bool {
        self.row == row && col >= self.col && usize::from(col - self.col) < self.cells.len()
    }
}

/// `w` without the spaces at its end.
fn trimmed(mut w: Vec<u8>) -> Vec<u8> {
    while w.last() == Some(&b' ') {
        w.pop();
    }
    w
}

/// Whether `p` is the rest that follows continued span `s`, `p` on row `next` (`s`'s next).
fn is_rest(p: &KeySpan, s: &KeySpan, next: u8) -> bool {
    p.joined.is_some() && p.ink == s.ink && p.row == next && p.col == 0
}

/// Whether `p` is the continued span rest `s` follows, `p` on row `above` (`s`'s previous).
fn is_first(p: &KeySpan, s: &KeySpan, above: u8) -> bool {
    p.continued && p.ink == s.ink && p.row == above
}

/// The keywords the message window shows (`TextState::keys`): recorded as the line assembler
/// draws its cells there ([`KeySpans::draw`]), forgotten where a cell is drawn over them
/// ([`KeySpans::forget`]), moved by its scroll ([`KeySpans::scroll`]), the parts of one the word
/// wrap broke linked ([`KeySpans::wrapped`]): a part marked [`KeySpan::continued`] and the rest
/// the next row starts with ([`KeySpan::joined`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeySpans {
    /// The spans shown.
    pub spans: Vec<KeySpan>,
    /// The cells the last wrap dropped inside a keyword it broke, until the next row's first
    /// cell is drawn ([`KeySpans::wrapped`]) or the next row is ([`KeySpans::row_drawn`]).
    wrap: Option<Vec<u16>>,
}

impl KeySpans {
    /// Cell `c` drawn in `ink` at window cell (`row`, `col`), `keyword` when that is the alert
    /// colour. The span holding the cell is forgotten first (whatever is drawn), then a keyword
    /// cell ends the span of that ink just before it on the row, or starts one unless it is a
    /// space (the more prompt blanks its row in the ink of a keyword it broke). The first cell
    /// drawn at column 0 after a wrap that broke a keyword, a space aside, takes that wrap: a
    /// keyword cell starts the keyword's rest there, any other ends it.
    pub fn draw(&mut self, row: u8, col: u8, c: u16, ink: u8, keyword: bool) {
        self.forget(row, col);
        let wrap = if col == 0 && c != BLANK {
            self.wrap.take()
        } else {
            None
        };
        if !keyword {
            return;
        }
        let next = |s: &&mut KeySpan| {
            s.row == row && s.ink == ink && usize::from(s.col) + s.cells.len() == usize::from(col)
        };
        match self.spans.iter_mut().find(next) {
            Some(s) => s.cells.push(c),
            None if c != BLANK => self.spans.push(KeySpan {
                row,
                col,
                cells: vec![c],
                ink,
                joined: wrap,
                continued: false,
            }),
            None => {}
        }
    }

    /// The line assembler's word wrap broke row `row` inside a keyword: its last drawn cell, at
    /// column `col`, and the next cell are in the alert colour, `dropped` (a space or none) the
    /// cells it dropped between them. The span holding that last cell is marked continued (no
    /// other span of the row is), and the span the next row starts with at column 0 becomes the
    /// keyword's rest; without a span there (a blank cell of the alert colour starts none),
    /// nothing is.
    pub fn wrapped(&mut self, row: u8, col: u8, dropped: Vec<u16>) {
        let mut marked = false;
        for s in self.spans.iter_mut().filter(|s| s.row == row) {
            s.continued = s.covers(row, col);
            marked |= s.continued;
        }
        self.wrap = marked.then_some(dropped);
    }

    /// Port (spec 22 Contract 4): the line assembler drew a row of the message window: a wrap
    /// its first cell did not take is spent, so it can reach no later row.
    pub fn row_drawn(&mut self) {
        self.wrap = None;
    }

    /// The cell at window cell (`row`, `col`) is drawn over: the span holding it goes, and so
    /// does its link to the part before or after it (that part is a keyword of its own then).
    pub fn forget(&mut self, row: u8, col: u8) {
        while let Some(i) = self.spans.iter().position(|s| s.covers(row, col)) {
            self.unlink(i);
        }
    }

    /// Removes span `i` and its links: its rest is a keyword of its own then, and the part
    /// before it ends its keyword.
    fn unlink(&mut self, i: usize) {
        let gone = self.spans.remove(i);
        if gone.continued
            && let Some(next) = gone.row.checked_add(1)
            && let Some(rest) = self.spans.iter_mut().find(|p| is_rest(p, &gone, next))
        {
            rest.joined = None;
        }
        if gone.joined.is_some()
            && let Some(above) = gone.row.checked_sub(1)
            && let Some(first) = self.spans.iter_mut().find(|p| is_first(p, &gone, above))
        {
            first.continued = false;
        }
    }

    /// The window scrolled its cells up a row (0x10525c): every span moves up with them; those
    /// of the top row leave the window and are returned, in order.
    pub fn scroll(&mut self) -> Vec<KeySpan> {
        let (gone, kept): (Vec<KeySpan>, Vec<KeySpan>) = std::mem::take(&mut self.spans)
            .into_iter()
            .partition(|s| s.row == 0);
        self.spans = kept
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
        self.spans.iter().find(|s| s.covers(row, col))
    }

    /// The parts of the keyword one of whose cells is window cell (`row`, `col`), first to
    /// last: before a rest ([`KeySpan::joined`]) the continued span of its ink on the row above,
    /// after a continued span ([`KeySpan::continued`]) the rest of its ink at column 0 of the row
    /// below. Empty off every span.
    pub fn parts(&self, row: u8, col: u8) -> Vec<&KeySpan> {
        let Some(s) = self.at(row, col) else {
            return Vec::new();
        };
        let mut parts = vec![s];
        while let Some(p) = parts.first().and_then(|&f| self.before(f)) {
            parts.insert(0, p);
        }
        while let Some(p) = parts.last().and_then(|&l| self.after(l)) {
            parts.push(p);
        }
        parts
    }

    /// The part before rest `s`: the continued span of its ink on the row above.
    fn before(&self, s: &KeySpan) -> Option<&KeySpan> {
        s.joined.as_ref()?;
        let above = s.row.checked_sub(1)?;
        self.spans.iter().find(|p| is_first(p, s, above))
    }

    /// The part after continued span `s`: the rest of its ink at column 0 of the row below.
    fn after(&self, s: &KeySpan) -> Option<&KeySpan> {
        if !s.continued {
            return None;
        }
        let next = s.row.checked_add(1)?;
        self.spans.iter().find(|p| is_rest(p, s, next))
    }

    /// The word the keyword holding window cell (`row`, `col`) types (UTF-8): its parts' cells
    /// and the cells the wrap dropped between them, as typed ([`typed`]), without the spaces at
    /// its end; empty off every span.
    pub fn word(&self, row: u8, col: u8) -> Vec<u8> {
        let mut cells = Vec::new();
        for (i, p) in self.parts(row, col).into_iter().enumerate() {
            if i > 0 {
                cells.extend(p.joined.iter().flatten());
            }
            cells.extend(&p.cells);
        }
        trimmed(typed(&cells))
    }

    /// Forgets every span (the window set up again).
    pub fn clear(&mut self) {
        self.spans.clear();
        self.wrap = None;
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
    /// The keyword of the message window whose first part starts at window cell (`row`, `col`).
    Key { row: u8, col: u8 },
}

/// The buttons shown, the target drawn inverse and the one the left button went down on, while
/// the conversation reads the player's words (`UiState::talk`); empty at every other time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TalkClicks {
    /// The read takes clicks ([`begin`] to [`end`]), with buttons or without.
    pub reading: bool,
    pub buttons: Vec<Button>,
    pub hot: Option<Target>,
    pub pressed: Option<Target>,
}

#[cfg(test)]
thread_local! {
    /// Test hook: the keys the last click put at the front of the queue ([`pointer`]), which
    /// the line editor takes in the same pass.
    pub(crate) static INJECTED: std::cell::RefCell<Vec<InputEvent>> =
        const { std::cell::RefCell::new(Vec::new()) };
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
/// drawn there, centred, one blank cell between them, and recorded; else none. Wider than the
/// row with their blank cells, they go without them; wider still (a row narrower than main's
/// 17 cells, which two words of [`WORD_CELLS`] and the cell between fill), there are none.
pub fn begin(s: &mut State, io: &Io) {
    s.ui.talk = TalkClicks {
        reading: true,
        ..TalkClicks::default()
    };
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
    if a.len() + b.len() + 1 > width {
        return;
    }
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
/// of the queue are taken in order, one a take ([`Io::take_pointer_upto`]), each where its own
/// event put the pointer, the target under it drawn inverse after each; a click on one (the left
/// button down on it and up again on it) types its word: one Backspace for each of the `typed`
/// cells the line holds, the word, Enter, at the front of the queue, before any event after the
/// release. A word that types nothing (a keyword of cells no key types) types nothing at all.
pub fn pointer(s: &mut State, io: &Io, typed: usize) {
    loop {
        let before = io.pointer().buttons & POINTER_LEFT;
        let p = io.take_pointer_upto(1);
        let target = target_at(s, p.x, p.y);
        hover(s, io, target);
        match (before, p.buttons & POINTER_LEFT) {
            (0, POINTER_LEFT) => s.ui.talk.pressed = target,
            (POINTER_LEFT, 0) => {
                if let Some(t) = s.ui.talk.pressed.take().filter(|&t| Some(t) == target) {
                    let word = word_of(s, t);
                    if !word.is_empty() {
                        let keys = keys(typed, &word);
                        #[cfg(test)]
                        INJECTED.with(|k| k.replace(keys.clone()));
                        io.push_front(&keys);
                        return;
                    }
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

/// Spec 22 Contract 4 and 6: the message window scrolled its cells up a row (0x10525c): its
/// keywords move up with them ([`KeySpans::scroll`], whose spans of the top row are returned),
/// and so do the keyword drawn inverse and the one the left button went down on: each becomes
/// its first part still in the window, a row up (a keyword whose first part left with the top
/// row is its rest then, drawn back from there; one wholly gone, none).
pub fn scroll(ui: &mut super::UiState) -> Vec<KeySpan> {
    let keys = &ui.text.keys;
    let moved = |t: Option<Target>| match t {
        Some(Target::Key { row, col }) => {
            let part = keys.parts(row, col).into_iter().find(|p| p.row > 0);
            part.map(|p| Target::Key {
                row: p.row - 1,
                col: p.col,
            })
        }
        t => t,
    };
    let (hot, pressed) = (moved(ui.talk.hot), moved(ui.talk.pressed));
    (ui.talk.hot, ui.talk.pressed) = (hot, pressed);
    ui.text.keys.scroll()
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
    let first = *s.ui.text.keys.parts(r as u8, c as u8).first()?;
    Some(Target::Key {
        row: first.row,
        col: first.col,
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
        Target::Key { row, col } => s.ui.text.keys.word(row, col),
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
/// paper colour on cells of the command colour A4-0x6572 (0x106506's mark), a keyword (every
/// part of it) in its ink on the message window's paper.
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
            let w = s.ui.text.window(WindowId::Messages);
            for k in s.ui.text.keys.parts(row, col) {
                let at = (
                    u16::from(w.x) + u16::from(k.col),
                    u16::from(w.y) + u16::from(k.row),
                );
                draw_cells(s, io, &k.cells, at, swap((k.ink, w.bg)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cells` drawn as keyword cells in ink 3 from window cell (`row`, `col`).
    fn put(k: &mut KeySpans, row: u8, col: u8, cells: &[u16]) {
        for (i, &c) in cells.iter().enumerate() {
            k.draw(row, col + i as u8, c, 3, true);
        }
    }

    #[test]
    fn spans_join_on_a_row_forget_where_drawn_over_and_leave_with_the_top_row() {
        let mut k = KeySpans::default();
        put(&mut k, 1, 4, &[0x61, 0x62]);
        put(&mut k, 0, 0, &[0x7a]);
        put(&mut k, 1, 7, &[0x63]);
        let words: Vec<Vec<u8>> = k.spans.iter().map(KeySpan::word).collect();
        assert_eq!(words, [b"ab".to_vec(), b"z".to_vec(), b"c".to_vec()]);
        assert_eq!(k.at(1, 5).map(KeySpan::word), Some(b"ab".to_vec()));
        assert_eq!(k.at(1, 6), None, "between two spans");
        let gone = k.scroll();
        assert_eq!(gone.len(), 1, "the top row's span handed back");
        assert_eq!(k.at(0, 4).map(|s| s.row), Some(0), "a row up");
        k.forget(0, 5);
        assert_eq!(k.at(0, 4), None, "drawn over: forgotten whole");
        assert_eq!(k.spans.len(), 1);
        k.clear();
        assert!(k.spans.is_empty());
        // Spaces join a span but start none, and its word ends at its last other cell.
        put(&mut k, 2, 0, &[BLANK; 3]);
        assert!(k.spans.is_empty(), "a row of spaces is no keyword");
        put(&mut k, 2, 5, &[0x61, 0x20, 0x62, 0x20]);
        assert_eq!(k.spans.len(), 1);
        assert_eq!(k.spans[0].word(), b"a b");
    }

    #[test]
    fn a_cell_drawn_over_a_keyword_replaces_it_whatever_its_ink() {
        let mut k = KeySpans::default();
        put(&mut k, 1, 4, &[0x61, 0x62]);
        k.draw(1, 5, 0x7a, 1, false);
        assert!(
            k.spans.is_empty(),
            "a cell of the text ink over its second cell"
        );
        put(&mut k, 1, 4, &[0x61]);
        k.draw(1, 4, 0x7a, 1, false);
        put(&mut k, 1, 4, &[0x62]);
        let words: Vec<Vec<u8>> = k.spans.iter().map(KeySpan::word).collect();
        assert_eq!(
            words,
            [b"b".to_vec()],
            "a fresh span, not the old one's tail"
        );
        put(&mut k, 1, 5, &[0x63]);
        put(&mut k, 1, 4, &[0x64]);
        assert_eq!(
            k.spans.iter().map(KeySpan::word).collect::<Vec<_>>(),
            [b"d".to_vec()],
            "a keyword cell over a keyword forgets all of it and starts a fresh one"
        );
        println!("drawn over: replaced: pass");
    }

    #[test]
    fn a_keyword_the_wrap_broke_is_one_word_from_either_part() {
        let mut k = KeySpans::default();
        put(&mut k, 3, 10, &[0x62, 0x69, 0x67]);
        k.wrapped(3, 12, vec![BLANK]);
        // The more prompt's blank row between the two parts leaves the wrap.
        k.draw(4, 0, BLANK, 3, true);
        put(&mut k, 4, 0, &[0x63, 0x61, 0x74]);
        for (row, col) in [(3, 11), (4, 1)] {
            assert_eq!(k.word(row, col), b"big cat", "from row {row}");
            let first = k.parts(row, col)[0];
            assert_eq!(
                (first.row, first.col),
                (3, 10),
                "one target: the first part"
            );
        }
        // Scrolled a row up, still one; its first part gone with the top row, the rest alone.
        k.scroll();
        assert_eq!(k.word(3, 1), b"big cat");
        k.scroll();
        k.scroll();
        k.scroll();
        assert_eq!(k.word(0, 1), b"cat", "the first part left the window");
        // Two keywords, the wrap between them dropping no keyword cell: two words.
        k.clear();
        put(&mut k, 3, 14, &[0x61]);
        put(&mut k, 4, 0, &[0x62]);
        assert_eq!(
            (k.word(3, 14), k.word(4, 0)),
            (b"a".to_vec(), b"b".to_vec())
        );
        // A wrap whose next row starts with a cell of another ink is spent.
        k.clear();
        put(&mut k, 3, 14, &[0x61]);
        k.wrapped(3, 14, Vec::new());
        k.draw(4, 0, 0x78, 1, false);
        put(&mut k, 5, 0, &[0x62]);
        assert_eq!(k.word(5, 0), b"b");
        assert_eq!(k.spans[1].joined, None);
        // A wrap at a cell no span holds marks nothing, and a drawn row spends a wrap.
        k.clear();
        put(&mut k, 3, 10, &[0x61]);
        k.wrapped(3, 16, vec![BLANK]);
        put(&mut k, 4, 0, &[0x62]);
        assert_eq!(k.spans[1].joined, None, "no span at the wrap's cell");
        k.clear();
        put(&mut k, 3, 16, &[0x61]);
        k.wrapped(3, 16, vec![BLANK]);
        k.row_drawn();
        put(&mut k, 4, 0, &[0x62]);
        assert_eq!(k.spans[1].joined, None, "spent by the drawn row");
        assert_eq!(k.word(3, 16), b"a");
        println!("wrapped keyword: one word from either part: pass");
    }

    /// Spec 22 Contract 4 (opus M1, gpt a2-1): the parts of a wrapped keyword are linked to each
    /// other only: keywords before and after them on their rows stay their own, and a part
    /// drawn over takes its link with it.
    #[test]
    fn a_wrapped_keyword_s_parts_link_to_each_other_alone() {
        // `b` and `c` on row 3, the wrap breaking `c` into `d`; `e` after `d` on row 4.
        let chained = || {
            let mut k = KeySpans::default();
            put(&mut k, 3, 2, &[0x62]);
            put(&mut k, 3, 10, &[0x63]);
            k.wrapped(3, 10, vec![BLANK]);
            put(&mut k, 4, 0, &[0x64]);
            put(&mut k, 4, 6, &[0x65]);
            k
        };
        let k = chained();
        let words = [(3, 2), (3, 10), (4, 0), (4, 6)].map(|(r, c)| k.word(r, c));
        assert_eq!(words, [&b"b"[..], b"c d", b"c d", b"e"].map(<[u8]>::to_vec));
        let first = |r, c| {
            k.parts(r, c)
                .iter()
                .map(|p| (p.row, p.col))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            first(3, 2),
            [(3, 2)],
            "the keyword before the broken one, alone"
        );
        assert_eq!(first(4, 6), [(4, 6)], "the keyword after its rest, alone");
        assert_eq!(first(4, 0), [(3, 10), (4, 0)], "both parts, first to last");
        // Drawn over, the first part takes the link with it: the rest is a keyword of its own,
        // and joins no other span of the row above.
        let mut k = chained();
        k.forget(3, 10);
        assert_eq!(k.word(4, 0), b"d");
        assert_eq!(k.parts(4, 0).len(), 1);
        assert_eq!(k.spans.iter().filter(|s| s.joined.is_some()).count(), 0);
        // Drawn over, the rest ends the first part's keyword there.
        let mut k = chained();
        k.forget(4, 0);
        assert_eq!(k.word(3, 10), b"c");
        assert!(!k.spans.iter().any(|s| s.continued), "no part continued");
        println!("wrapped keyword: parts linked to each other alone: pass");
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

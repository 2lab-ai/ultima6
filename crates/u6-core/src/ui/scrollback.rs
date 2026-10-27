//! Port-only (spec 23): the message window's scrollback. The original scrolls the window's top
//! row off and forgets it (0x10525c, [`super::text::scroll_window`]); the port keeps it, as the
//! cells the game drew, for a view the player scrolls back with the wheel and a scrollbar the
//! engine draws at the window's right edge. Presentation only: the game never reads it and none
//! of it is saved.
//!
//! [`Scrollback`] (in [`super::UiState`]) holds what the game drew: a shadow of the window's rows,
//! written at each draw of a cell inside it ([`note`]), and the lines that scrolled off
//! ([`scrolled`]). [`View`] (in [`crate::Engine`]) holds what the player looks at: the offset back
//! from the live bottom and the gesture on the bar. [`draw_view`] draws the view over a copy of
//! the published frame; at offset 0 it draws nothing, so the frame is the original's.

use std::collections::VecDeque;
use std::fmt;

use u6_render::{BITMAP_PAPER, Framebuffer, Kind, Src};

use super::UiState;
use super::text::{self, CELL, Paper, TextWindow, WindowId};
use crate::engine::{InputEvent, POINTER_LEFT};

/// The lines kept at most; past it the oldest is dropped first. A 17-cell line holds 102 bytes of
/// cells and three vector headers: about 0.1 MB at the bound ([`Scrollback::heap_bytes`]).
pub const HISTORY: usize = 500;
/// The thumb's least length, in pixels (one cell's height).
pub const MIN_THUMB: usize = 8;
/// The thumb's columns in the window's last cell (x 5..=7 of the cell) and the track's.
const THUMB_X: std::ops::Range<usize> = 5..8;
const TRACK_X: usize = 6;

/// How a cell was drawn: by which of [`text::draw_char`], [`text::draw_typed`] and
/// [`text::draw_symbol`], or not since its row was cleared ([`CellKind::Blank`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellKind {
    #[default]
    Blank,
    Char,
    Typed,
    Symbol,
}

/// One cell as drawn: its code ([`super::wide`]), the window's ink and paper then, its kind.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Cell {
    pub code: u16,
    pub fg: u8,
    pub bg: u8,
    pub kind: CellKind,
}

/// One row of the window: its cells, the paper it was cleared to, and what [`scrolled`] adds when
/// it is kept: the pixels of the row (index in the row, row-major, and colour) the cells do not
/// draw as they were shown, and the keyword spans, the column ranges `[start, end)` drawn in the
/// alert ink (braced words, [`super::print`]).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Line {
    pub cells: Vec<Cell>,
    pub bg: u8,
    pub patch: Vec<(u16, u8)>,
    pub spans: Vec<(u8, u8)>,
}

impl Line {
    fn blank(width: u8, bg: u8) -> Line {
        Line {
            cells: vec![Cell::default(); usize::from(width)],
            bg,
            patch: Vec::new(),
            spans: Vec::new(),
        }
    }
}

/// What the game drew in the message window ([`WindowId::Messages`]): the shadow of its rows and
/// the lines kept, oldest first, at most [`HISTORY`].
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Scrollback {
    rows: Vec<Line>,
    lines: VecDeque<Line>,
    total: u64,
}

impl Scrollback {
    /// The lines kept, oldest first.
    pub fn lines(&self) -> &VecDeque<Line> {
        &self.lines
    }

    /// The lines kept so far, the dropped ones included.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// The shadow of the window's rows (empty before the first cell drawn in it).
    pub fn rows(&self) -> &[Line] {
        &self.rows
    }

    /// The bytes the lines and the shadow hold on the heap, by capacity.
    pub fn heap_bytes(&self) -> usize {
        let line = |l: &Line| {
            l.cells.capacity() * size_of::<Cell>()
                + l.patch.capacity() * size_of::<(u16, u8)>()
                + l.spans.capacity() * size_of::<(u8, u8)>()
        };
        self.lines.capacity() * size_of::<Line>()
            + self.rows.capacity() * size_of::<Line>()
            + self.lines.iter().chain(&self.rows).map(line).sum::<usize>()
    }

    /// The shadow at the window's size, blank where it was not.
    fn fit(&mut self, m: &TextWindow) {
        let fits = self.rows.len() == usize::from(m.height)
            && self
                .rows
                .iter()
                .all(|r| r.cells.len() == usize::from(m.width));
        if !fits {
            self.rows = vec![Line::blank(m.width, m.bg); usize::from(m.height)];
        }
    }
}

/// The lines and the shadow by their counts only (the cells are the user's game text).
impl fmt::Debug for Scrollback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Scrollback({} rows, {} lines, {} kept so far)",
            self.rows.len(),
            self.lines.len(),
            self.total
        )
    }
}

impl fmt::Debug for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Line({} cells, {} patched, {} spans)",
            self.cells.len(),
            self.patch.len(),
            self.spans.len()
        )
    }
}

impl fmt::Debug for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cell({:?})", self.kind)
    }
}

/// The message window's record.
fn messages(ui: &UiState) -> TextWindow {
    *ui.text.window(WindowId::Messages)
}

/// Whether screen cell (col, row) lies in `w`.
fn holds(w: &TextWindow, (col, row): (u16, u16)) -> bool {
    let (x, y) = (u16::from(w.x), u16::from(w.y));
    (x..x + u16::from(w.width)).contains(&col) && (y..y + u16::from(w.height)).contains(&row)
}

/// The cells of `w`'s rectangle, `(x0, y0, x1, y1)` exclusive.
fn rect(w: &TextWindow) -> (u16, u16, u16, u16) {
    let (x, y) = (u16::from(w.x), u16::from(w.y));
    (x, y, x + u16::from(w.width), y + u16::from(w.height))
}

/// Records the cell `c` a draw site just drew at screen cell `at` in window `w`'s colours, when
/// it lies in the message window (with no font nothing is drawn, and nothing is recorded).
pub fn note(ui: &mut UiState, w: &TextWindow, c: u16, at: (u16, u16), kind: CellKind) {
    let m = messages(ui);
    if ui.font.is_none() || !holds(&m, at) {
        return;
    }
    let sb = &mut ui.scrollback;
    sb.fit(&m);
    let (col, row) = (at.0 - u16::from(m.x), at.1 - u16::from(m.y));
    sb.rows[usize::from(row)].cells[usize::from(col)] = Cell {
        code: c,
        fg: w.fg,
        bg: w.bg,
        kind,
    };
}

/// Before `w` scrolls in `fb` ([`text::scroll_window`]): when `w` is the message window, its top
/// row is kept, with the pixels of `fb` its cells do not draw and its keyword spans, the oldest
/// line dropped past [`HISTORY`], and the shadow moves up one row, the new last row blank in
/// `w`'s paper. Another window over part of it clears the shadow.
pub fn scrolled(ui: &mut UiState, fb: &Framebuffer, w: &TextWindow) {
    let m = messages(ui);
    let (a, b) = (rect(&m), rect(w));
    if a != b {
        let apart = b.2 <= a.0 || a.2 <= b.0 || b.3 <= a.1 || a.3 <= b.1;
        if !apart {
            ui.scrollback.rows.clear();
        }
        return;
    }
    if m.width == 0 || m.height == 0 {
        return;
    }
    ui.scrollback.fit(&m);
    let alert = ui.tables.colours.alert;
    let mut line = ui.scrollback.rows.remove(0);
    line.patch = patch(ui, fb, &line, &m, m.y);
    line.spans = spans(&line, alert);
    let sb = &mut ui.scrollback;
    sb.lines.push_back(line);
    if sb.lines.len() > HISTORY {
        sb.lines.pop_front();
    }
    sb.total += 1;
    sb.rows.push(Line::blank(m.width, w.bg));
}

/// The rows of a drawn cell's glyph as [`text::draw_char`] draws them: `u6.ch`'s for a byte, the
/// Hangul table's for a syllable, a blank cell for any other code.
fn glyph(ui: &UiState, code: u16) -> [u8; 8] {
    let Some(font) = ui.font.as_ref() else {
        return [0; 8];
    };
    match u8::try_from(code) {
        Ok(b) => font.glyphs[usize::from(b)],
        Err(_) => ui
            .hangul
            .as_deref()
            .and_then(|h| h.glyph(u32::from(code)))
            .copied()
            .unwrap_or([0; 8]),
    }
}

/// The colour [`draw_line`] draws at pixel (dx, dy) of cell `cell` of a line cleared to `bg`
/// (`None`: a blank cell whose paper the fill does not write, past colour 15).
fn drawn(rows: &[u8; 8], cell: &Cell, bg: u8, (dx, dy): (usize, usize)) -> Option<u8> {
    match cell.kind {
        CellKind::Blank => (bg <= 15).then_some(bg),
        _ if rows[dy] & (0x80 >> dx) != 0 => Some(cell.fg & 0x0f),
        _ => Some(cell.bg & 0x0f),
    }
}

/// The pixels of screen row `row` of `fb` (the line's cells across window `m`) that the cells do
/// not draw as `fb` shows them.
fn patch(ui: &UiState, fb: &Framebuffer, line: &Line, m: &TextWindow, row: u8) -> Vec<(u16, u8)> {
    let (x0, y0) = (usize::from(m.x) * CELL, usize::from(row) * CELL);
    let width = line.cells.len() * CELL;
    let mut out = Vec::new();
    for (k, cell) in line.cells.iter().enumerate() {
        let rows = glyph(ui, cell.code);
        for dy in 0..CELL {
            for dx in 0..CELL {
                let x = k * CELL + dx;
                let shown = fb.get(x0 + x, y0 + dy);
                if let Some(c) = shown
                    && drawn(&rows, cell, line.bg, (dx, dy)) != Some(c)
                {
                    out.push(((dy * width + x) as u16, c));
                }
            }
        }
    }
    out
}

/// The runs of drawn cells in the alert ink, as column ranges.
fn spans(line: &Line, alert: u8) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    let mut start = None;
    for (k, cell) in line.cells.iter().enumerate() {
        let keyword = cell.kind != CellKind::Blank && cell.fg == alert;
        match (keyword, start) {
            (true, None) => start = Some(k),
            (false, Some(s)) => {
                out.push((s as u8, k as u8));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s as u8, line.cells.len() as u8));
    }
    out
}

/// Draws kept line `line` at screen row `row` of window `m` into `out`: the row filled with its
/// paper (naming the paper, [`text::fill_rect`]), each drawn cell through the function that drew
/// it, then the patch (naming the paper where its colour is the paper's there).
fn draw_line(ui: &UiState, line: &Line, m: &TextWindow, row: u16, out: &mut Framebuffer) {
    let paper = ui.paper.as_ref();
    let fonts = (ui.font.as_ref(), ui.hangul.as_deref());
    let (x0, y0) = (usize::from(m.x) * CELL, usize::from(row) * CELL);
    let width = line.cells.len() * CELL;
    let (x1, y1) = ((x0 + width) as i32 - 1, (y0 + CELL) as i32 - 1);
    text::fill_rect(out, x0 as i32, y0 as i32, x1, y1, line.bg, paper);
    for (k, cell) in line.cells.iter().enumerate() {
        let w = TextWindow {
            fg: cell.fg,
            bg: cell.bg,
            ..*m
        };
        let at = (u16::from(m.x) + k as u16, row);
        match cell.kind {
            CellKind::Blank => {}
            CellKind::Char => text::draw_char(out, fonts.0, fonts.1, &w, cell.code, at.0, at.1),
            CellKind::Typed => text::draw_typed(out, fonts, &w, cell.code, at),
            CellKind::Symbol => text::draw_symbol(out, fonts, &w, cell.code, at, paper),
        }
    }
    for &(i, c) in &line.patch {
        let (x, y) = (x0 + usize::from(i) % width, y0 + usize::from(i) / width);
        let src = paper.map_or(Src::CLASSIC, |p| p.over(x, y, c));
        out.set_src(x, y, c, src);
    }
}

/// Copies screen row `from` of window `m` in `live` to screen row `to` of `out`, the pixels with
/// their sources; a pixel that names the paper names it at its new place, as
/// [`text::scroll_rect`] moves it.
fn copy_row(
    live: &Framebuffer,
    out: &mut Framebuffer,
    m: &TextWindow,
    (from, to): (u16, u16),
    paper: Option<&Paper>,
) {
    let x0 = usize::from(m.x) * CELL;
    let is_paper = |s: &Src| s.kind == Kind::Bitmap && s.id == BITMAP_PAPER;
    for dy in 0..CELL {
        let (fy, ty) = (usize::from(from) * CELL + dy, usize::from(to) * CELL + dy);
        for x in x0..x0 + usize::from(m.width) * CELL {
            let Some(c) = live.get(x, fy) else { continue };
            let src = live.src_at(x, fy).unwrap_or(Src::CLASSIC);
            let src = match paper {
                Some(p) if is_paper(&src) => p.over(x, ty, c),
                _ => src,
            };
            out.set_src(x, ty, c, src);
        }
    }
}

/// The thumb's top and length, in pixels down a track of `track` pixels, for a window of `rows`
/// rows over `kept` kept lines at `offset` lines back: its length the visible fraction (at least
/// [`MIN_THUMB`]), its top the fraction of the lines above the view.
pub fn thumb(track: usize, rows: usize, kept: usize, offset: usize) -> (usize, usize) {
    let total = kept + rows;
    if total == 0 || track == 0 {
        return (0, track);
    }
    let len = ((track * rows + total / 2) / total).clamp(MIN_THUMB.min(track), track);
    let span = track - len;
    let above = kept - offset.min(kept);
    let top = (span * above + kept / 2).checked_div(kept).unwrap_or(span);
    (top, len)
}

/// The scrollbar over the view in `out`: in window `m`'s last cell column, in the text ink, a
/// dotted track and the thumb ([`thumb`]), only in the cells that show one colour (no letter), so
/// a letter there stays whole.
fn draw_bar(ui: &UiState, m: &TextWindow, (kept, offset): (usize, usize), out: &mut Framebuffer) {
    let ink = ui.tables.colours.text;
    let x0 = (usize::from(m.x) + usize::from(m.width) - 1) * CELL;
    let rows = usize::from(m.height);
    let (top, len) = thumb(rows * CELL, rows, kept, offset);
    for r in 0..rows {
        let y0 = (usize::from(m.y) + r) * CELL;
        let first = out.get(x0, y0);
        let plain = (0..CELL * CELL).all(|k| out.get(x0 + k % CELL, y0 + k / CELL) == first);
        if !plain {
            continue;
        }
        for dy in 0..CELL {
            let t = r * CELL + dy;
            if (top..top + len).contains(&t) {
                for dx in THUMB_X {
                    out.set(x0 + dx, y0 + dy, ink);
                }
            } else if t.is_multiple_of(2) {
                out.set(x0 + TRACK_X, y0 + dy, ink);
            }
        }
    }
}

/// Draws `view` over `out`, a copy of the published frame `live`, for the message window of `ui`:
/// each of its rows the kept line or the live row the offset puts there, then the scrollbar.
/// Nothing at offset 0.
pub fn draw_view(ui: &UiState, view: &View, live: &Framebuffer, out: &mut Framebuffer) {
    let m = messages(ui);
    let sb = &ui.scrollback;
    let kept = view.kept(sb);
    let offset = view.offset.min(kept);
    if offset == 0 || m.width == 0 || m.height == 0 {
        return;
    }
    let first = kept - offset;
    for r in 0..u16::from(m.height) {
        let i = first + usize::from(r);
        let row = u16::from(m.y) + r;
        match sb.lines.get(i).filter(|_| i < kept) {
            Some(line) => draw_line(ui, line, &m, row, out),
            None => {
                let from = u16::from(m.y) + (i - kept) as u16;
                copy_row(live, out, &m, (from, row), ui.paper.as_ref());
            }
        }
    }
    draw_bar(ui, &m, (kept, offset), out);
}

/// A press the bar took, until every button is up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Gesture {
    #[default]
    None,
    /// The thumb held `grab` pixels below its top.
    Thumb { grab: i32 },
    /// A press on the track (the page turned at the press) or another button.
    Held,
}

/// What the player looks at (the engine's, not the game's): the offset back from the live bottom
/// (0 = live), the kept lines as of the last publish, the gesture on the bar, the pointer's
/// buttons last queued, and a stamp that moves whenever what the view shows changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    offset: usize,
    published: u64,
    gesture: Gesture,
    buttons: u8,
    stamp: u64,
}

impl View {
    /// Lines back from the live bottom (0 = live).
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Moves whenever what the view shows changed.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }

    /// The kept lines as of the last publish (those after it are not on the published frame).
    pub fn kept(&self, sb: &Scrollback) -> usize {
        let pending = sb.total.saturating_sub(self.published);
        sb.lines
            .len()
            .saturating_sub(usize::try_from(pending).unwrap_or(usize::MAX))
    }

    fn set(&mut self, offset: usize) {
        if offset != self.offset {
            self.offset = offset;
            self.stamp += 1;
        }
    }

    /// Back to the live bottom, any gesture ended.
    pub fn leave(&mut self) {
        self.set(0);
        self.gesture = Gesture::None;
    }

    /// At a publish: the lines kept since the last one are on the frame now; scrolled back, the
    /// offset grows by them, so the same lines stay in view.
    pub fn publish(&mut self, sb: &Scrollback) {
        let new = sb.total.saturating_sub(self.published);
        self.published = sb.total;
        if self.offset > 0 {
            let new = usize::try_from(new).unwrap_or(usize::MAX);
            let to = self.offset.saturating_add(new).min(self.kept(sb));
            self.set(to);
        }
    }

    /// The wheel at frame pixel (x, y), `dy` lines (< 0 older, > 0 newer): over the message window
    /// while `game` runs, the offset moves by -`dy` within the kept lines, and the wheel is taken
    /// (true); elsewhere nothing changes.
    pub fn wheel(&mut self, ui: &UiState, game: bool, (x, y): (i16, i16), dy: i32) -> bool {
        let m = messages(ui);
        if !game || !over(&m, x, y) {
            return false;
        }
        let kept = self.kept(&ui.scrollback) as i64;
        let to = (self.offset as i64 - i64::from(dy)).clamp(0, kept);
        self.set(to as usize);
        true
    }

    /// One input event before the engine queues it: a key, a repeat, typed text, and a press
    /// anywhere but on the bar return the view to the live bottom; a press on the bar (shown only
    /// while scrolled back) is the bar's until every button is up, and its events are queued with
    /// no button held (the sprite follows them, the game sees moves).
    pub fn input(&mut self, ui: &UiState, game: bool, e: InputEvent) -> InputEvent {
        let InputEvent::Pointer { x, y, buttons } = e else {
            if !matches!(e, InputEvent::Release(_)) {
                self.leave();
            }
            return e;
        };
        let held = buttons & 3;
        let pressed = held & !self.buttons;
        self.buttons = held;
        let moved = InputEvent::Pointer { x, y, buttons: 0 };
        if self.gesture != Gesture::None {
            if let Gesture::Thumb { grab } = self.gesture
                && held & POINTER_LEFT != 0
            {
                self.drag(ui, y, grab);
            }
            if held == 0 {
                self.gesture = Gesture::None;
            }
            return moved;
        }
        if pressed == 0 {
            return e;
        }
        let m = messages(ui);
        if !(game && self.offset > 0 && on_bar(&m, x, y)) {
            self.set(0);
            return e;
        }
        self.gesture = Gesture::Held;
        if pressed & POINTER_LEFT != 0 {
            let rows = usize::from(m.height);
            let kept = self.kept(&ui.scrollback);
            let (top, len) = thumb(rows * CELL, rows, kept, self.offset);
            let t = i32::from(y) - i32::from(m.y) * CELL as i32;
            if (top as i32..(top + len) as i32).contains(&t) {
                self.gesture = Gesture::Thumb {
                    grab: t - top as i32,
                };
            } else if t < top as i32 {
                self.set((self.offset + rows).min(kept));
            } else {
                self.set(self.offset.saturating_sub(rows));
            }
        }
        moved
    }

    /// The thumb held `grab` pixels below its top, the pointer at frame row `y`: its top follows,
    /// and the offset with it.
    fn drag(&mut self, ui: &UiState, y: i16, grab: i32) {
        let m = messages(ui);
        let rows = usize::from(m.height);
        let kept = self.kept(&ui.scrollback);
        let (_, len) = thumb(rows * CELL, rows, kept, self.offset);
        let span = rows * CELL - len;
        if span == 0 || kept == 0 {
            return;
        }
        let t = i32::from(y) - i32::from(m.y) * CELL as i32 - grab;
        let top = t.clamp(0, span as i32) as usize;
        let above = (top * kept + span / 2) / span;
        self.set(kept - above.min(kept));
    }
}

/// Whether frame pixel (x, y) lies in window `m`.
fn over(m: &TextWindow, x: i16, y: i16) -> bool {
    let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
        return false;
    };
    let (x0, y0) = (usize::from(m.x) * CELL, usize::from(m.y) * CELL);
    (x0..x0 + usize::from(m.width) * CELL).contains(&x)
        && (y0..y0 + usize::from(m.height) * CELL).contains(&y)
}

/// Whether frame pixel (x, y) lies on the bar's place: window `m`'s last cell column.
fn on_bar(m: &TextWindow, x: i16, y: i16) -> bool {
    let last = (usize::from(m.x) + usize::from(m.width)).saturating_sub(1) * CELL;
    over(m, x, y) && usize::try_from(x).is_ok_and(|x| x >= last)
}

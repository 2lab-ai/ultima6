//! Port-only (spec 23): the message window's scrollback. The original scrolls the window's top
//! row off and forgets it (0x10525c, [`super::text::scroll_window`]); the port keeps it, as the
//! cells the game drew, for a view the player scrolls back with the wheel and a scrollbar the
//! engine draws at the window's right edge. Presentation only: the game never reads it and none
//! of it is saved.
//!
//! [`Scrollback`] (in [`super::UiState`]) holds what the game drew: a shadow of the window's rows,
//! written at each draw of a cell inside it ([`note`]), and the lines that scrolled off
//! ([`scrolled`]). [`View`] (in [`crate::Engine`]) holds what the player looks at: the lines kept
//! as of the last publish, the offset back from the live bottom and the gesture on the bar.
//! [`draw_view`] draws the view over a copy of the published frame; at offset 0 it draws nothing,
//! so the frame is the original's.

use std::collections::{HashSet, VecDeque};
use std::fmt;
use std::rc::Rc;

use u6_render::{BITMAP_PAPER, Framebuffer, Kind, Src};

use super::UiState;
use super::talk::KeySpan;
use super::text::{self, CELL, Paper, TextWindow, WindowId};
use crate::engine::{InputEvent, POINTER_LEFT};

/// The lines kept at most; past it the oldest is dropped first. Each is shared ([`Rc`]): a copy of
/// the state, and the view's lines as of a publish, copy a pointer a line. A line of the window's
/// 17 cells as drawn holds 102 bytes of cells and its shared header; with no patch, about 0.1 MB
/// at the bound. The patch adds 4 bytes a pixel: at most every pixel of the row (1088), so a bound
/// of lines all patch is about 2.3 MB ([`Scrollback::heap_bytes`], spec 23 Design 11). A kept
/// keyword's cells take no patch ([`scrolled`]), so its span never adds to that. The view keeps the lines as
/// of the last publish, those dropped since included, so the two together hold at most two bounds
/// ([`View::heap_bytes`]).
pub const HISTORY: usize = 500;
/// The bytes of a token's allocation ([`Generation`]): its two counts.
fn token_bytes() -> usize {
    2 * size_of::<usize>()
}
/// The bytes of a kept line's shared allocation beside what the line holds: its two counts and the
/// line.
fn shared_bytes() -> usize {
    token_bytes() + size_of::<Line>()
}
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
/// draw as they were shown, and the keywords the row showed (spec 22's spans, `row` 0). A patch
/// pixel is drawn back in its colour alone: naming the paper where that colour is the paper's
/// there, else no image (spec 23 Design 11).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Line {
    pub cells: Vec<Cell>,
    pub bg: u8,
    pub patch: Vec<(u16, u8)>,
    /// The row's keyword spans as [`super::talk::KeySpans::scroll`] returned them, left to
    /// right; a rest ([`KeySpan::joined`]) keeps its link only to a continued span of its ink on
    /// the line kept before it (spec 23 Design 10).
    pub spans: Vec<KeySpan>,
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

    /// The bytes the line holds on the heap, by capacity: its cells, its patch, its spans and the
    /// cells each holds.
    fn heap_bytes(&self) -> usize {
        let cells = |v: &Vec<u16>| v.capacity() * size_of::<u16>();
        self.cells.capacity() * size_of::<Cell>()
            + self.patch.capacity() * size_of::<(u16, u8)>()
            + self.spans.capacity() * size_of::<KeySpan>()
            + self
                .spans
                .iter()
                .map(|s| cells(&s.cells) + s.joined.as_ref().map_or(0, cells))
                .sum::<usize>()
    }

    /// Whether cell `col` is one of a kept keyword's.
    fn in_span(&self, col: usize) -> bool {
        self.spans
            .iter()
            .any(|s| (usize::from(s.col)..usize::from(s.col) + s.cells.len()).contains(&col))
    }
}

/// What the game drew in the message window ([`WindowId::Messages`]): the shadow of its rows and
/// the lines kept, oldest first, at most [`HISTORY`], and the history's token ([`Generation`]).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Scrollback {
    rows: Vec<Line>,
    lines: VecDeque<Rc<Line>>,
    total: u64,
    generation: Generation,
}

/// A history's token: every new [`Scrollback`] has its own and so does every copy of one, so the
/// history the game draws into (moved with the state, never copied) keeps one token for as long as
/// it is that history, and any other in its place (a new game's state, as a relaunch makes, or a
/// copy put back) has another. The view compares tokens ([`View::publish`]). Equal whatever it is:
/// two histories of the same lines compare equal.
#[derive(Default)]
struct Generation(Rc<()>);

impl Clone for Generation {
    /// Another token: a copy of a history is not that history.
    fn clone(&self) -> Self {
        Generation::default()
    }
}

impl PartialEq for Generation {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Generation {}

impl Scrollback {
    /// The lines kept, oldest first.
    pub fn lines(&self) -> &VecDeque<Rc<Line>> {
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

    /// The bytes the lines, the shadow and the token hold on the heap, by capacity: each kept
    /// line's shared allocation (its two counts and the line) and what the line holds.
    pub fn heap_bytes(&self) -> usize {
        token_bytes()
            + self.lines.capacity() * size_of::<Rc<Line>>()
            + self.lines.len() * shared_bytes()
            + self.rows.capacity() * size_of::<Line>()
            + self.lines.iter().map(|l| l.heap_bytes()).sum::<usize>()
            + self.rows.iter().map(Line::heap_bytes).sum::<usize>()
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
/// row is kept with `spans`, the keywords it showed ([`super::talk::KeySpans::scroll`]), and the
/// pixels of `fb` its cells do not draw outside them, the oldest line dropped past [`HISTORY`],
/// and the shadow moves up one row, the new last row blank in `w`'s paper. Another window over
/// part of it clears the shadow. A keyword is kept only while each of its cells shows its drawing
/// or that drawing inverse, and its cells take no patch: the one under the pointer is drawn inverse
/// (spec 22 Contract 6), and the history keeps it as drawn, in its ink; a keyword drawn over is not
/// kept, and its cells take their patch as any other. A rest keeps its link ([`KeySpan::joined`])
/// only to a continued span of its ink on the line kept before it ([`linked`]).
pub fn scrolled(ui: &mut UiState, fb: &Framebuffer, w: &TextWindow, spans: Vec<KeySpan>) {
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
    let mut line = ui.scrollback.rows.remove(0);
    // opus N1 (round 4): a span drawn over (a cell that shows neither its drawing nor that drawing
    // inverse, as a window over the message window may draw while the spans are not updated) is
    // not kept, and its cells take their patch.
    let cells = line.cells.len();
    let shown = |s: &KeySpan| {
        let end = usize::from(s.col) + s.cells.len();
        end <= cells && (usize::from(s.col)..end).all(|k| as_drawn(&*ui, fb, &line, &m, m.y, k))
    };
    let spans = spans.into_iter().filter(shown).collect();
    line.spans = linked(ui.scrollback.lines.back().map(Rc::as_ref), spans);
    line.patch = patch(ui, fb, &line, &m, m.y);
    let sb = &mut ui.scrollback;
    sb.lines.push_back(Rc::new(line));
    if sb.lines.len() > HISTORY {
        sb.lines.pop_front();
    }
    sb.total += 1;
    sb.rows.push(Line::blank(m.width, w.bg));
}

/// `spans`, the keywords of a row kept after `above`, made to their length: a rest
/// ([`KeySpan::joined`], at column 0) keeps its link only where `above` holds a continued span of
/// its ink ([`KeySpan::continued`]), as spec 22 links the parts of a keyword on two rows; elsewhere
/// it is a keyword of its own (`joined` on a kept line means linked to the line kept before it).
fn linked(above: Option<&Line>, mut spans: Vec<KeySpan>) -> Vec<KeySpan> {
    for s in &mut spans {
        let ink = s.ink;
        let first = |p: &KeySpan| p.continued && p.ink == ink;
        if s.col != 0 || !above.is_some_and(|a| a.spans.iter().any(first)) {
            s.joined = None;
        }
        s.cells.shrink_to_fit();
        if let Some(j) = &mut s.joined {
            j.shrink_to_fit();
        }
    }
    spans.shrink_to_fit();
    spans
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

/// The colour a keyword's cell shows at pixel (dx, dy) drawn inverse, as spec 22 Contract 6 draws
/// the keyword under the pointer (`talk::draw_target`: [`text::draw_char`] with its ink and paper
/// swapped); `None` for a blank cell, which no keyword draws.
fn inverse(rows: &[u8; 8], cell: &Cell, (dx, dy): (usize, usize)) -> Option<u8> {
    match cell.kind {
        CellKind::Blank => None,
        _ if rows[dy] & (0x80 >> dx) != 0 => Some(cell.bg & 0x0f),
        _ => Some(cell.fg & 0x0f),
    }
}

/// Whether `fb` shows cell `k` of `line` (at screen row `row` of window `m`) as its drawing or that
/// drawing inverse ([`inverse`]), every pixel of it.
fn as_drawn(
    ui: &UiState,
    fb: &Framebuffer,
    line: &Line,
    m: &TextWindow,
    row: u8,
    k: usize,
) -> bool {
    let (x0, y0) = (usize::from(m.x) * CELL + k * CELL, usize::from(row) * CELL);
    let cell = &line.cells[k];
    let rows = glyph(ui, cell.code);
    let all = |draw: &dyn Fn((usize, usize)) -> Option<u8>| {
        (0..CELL * CELL).all(|i| {
            let at = (i % CELL, i / CELL);
            fb.get(x0 + at.0, y0 + at.1)
                .is_none_or(|c| draw(at) == Some(c))
        })
    };
    all(&|at| drawn(&rows, cell, line.bg, at)) || all(&|at| inverse(&rows, cell, at))
}

/// The pixels of screen row `row` of `fb` (the line's cells across window `m`) that the cells do
/// not draw as `fb` shows them, a kept keyword's cells ([`Line::spans`]) aside: each of them shows
/// its drawing or its drawing inverse ([`scrolled`] keeps no other span).
fn patch(ui: &UiState, fb: &Framebuffer, line: &Line, m: &TextWindow, row: u8) -> Vec<(u16, u8)> {
    let (x0, y0) = (usize::from(m.x) * CELL, usize::from(row) * CELL);
    let width = line.cells.len() * CELL;
    let mut out = Vec::new();
    for (k, cell) in line.cells.iter().enumerate() {
        if line.in_span(k) {
            continue;
        }
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
    out.shrink_to_fit();
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
/// each of its rows the line kept as of the publish or the live row the offset puts there, then
/// the scrollbar. Nothing at offset 0.
pub fn draw_view(ui: &UiState, view: &View, live: &Framebuffer, out: &mut Framebuffer) {
    let m = messages(ui);
    let kept = view.kept();
    let offset = view.offset.min(kept);
    if offset == 0 || m.width == 0 || m.height == 0 {
        return;
    }
    let first = kept - offset;
    for r in 0..u16::from(m.height) {
        let i = first + usize::from(r);
        let row = u16::from(m.y) + r;
        match view.lines.get(i) {
            Some(line) => draw_line(ui, line, &m, row, out),
            None => {
                let from = u16::from(m.y) + (i - kept) as u16;
                copy_row(live, out, &m, (from, row), ui.paper.as_ref());
            }
        }
    }
    draw_bar(ui, &m, (kept, offset), out);
}

/// A press the view took, until every button is up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Gesture {
    #[default]
    None,
    /// The thumb held `grab` pixels below its top.
    Thumb { grab: i32 },
    /// A press on the track (the page turned at the press), of another button on the bar, or
    /// inside the window off it (the view live again at the press), or a drag a key, a repeat or
    /// text interrupted.
    Held,
}

/// What the player looks at (the engine's, not the game's): the lines kept as of the last publish
/// (those kept after it are not on the published frame, and those dropped since stay until the
/// next) and the token of their history, the offset back from the live bottom (0 = live, at most
/// the lines), the gesture, the pointer's buttons last queued, and a stamp that moves whenever
/// what the view shows changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    lines: Vec<Rc<Line>>,
    generation: Rc<()>,
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

    /// The lines kept as of the last publish.
    pub fn kept(&self) -> usize {
        self.lines.len()
    }

    /// The bytes the view holds on the heap that `sb` does not, by capacity: its vector of lines,
    /// its token when it is not `sb`'s, and each line it keeps that `sb` no longer holds (dropped
    /// since the last publish), with its shared allocation. With [`Scrollback::heap_bytes`], at
    /// most two bounds of lines: the view keeps at most [`HISTORY`] lines, `sb` at most
    /// [`HISTORY`] others, and the vector is made to the largest publish ([`View::publish`]).
    pub fn heap_bytes(&self, sb: &Scrollback) -> usize {
        let held: HashSet<*const Line> = sb.lines.iter().map(Rc::as_ptr).collect();
        let token = if Rc::ptr_eq(&self.generation, &sb.generation.0) {
            0
        } else {
            token_bytes()
        };
        token
            + self.lines.capacity() * size_of::<Rc<Line>>()
            + self
                .lines
                .iter()
                .filter(|l| !held.contains(&Rc::as_ptr(l)))
                .map(|l| shared_bytes() + l.heap_bytes())
                .sum::<usize>()
    }

    /// Whether a press is the view's ([`View::input`]) until every button is up: the platforms
    /// pace the steps as with a button held (spec 06), though the game sees none.
    pub fn holding(&self) -> bool {
        self.gesture != Gesture::None
    }

    fn set(&mut self, offset: usize) {
        if offset != self.offset {
            self.offset = offset;
            self.stamp += 1;
        }
    }

    /// Back to the live bottom. A gesture stays the view's (as [`Gesture::Held`]) until every
    /// button is up: the game never sees a button the view took.
    pub fn leave(&mut self) {
        self.set(0);
        if self.gesture != Gesture::None {
            self.gesture = Gesture::Held;
        }
    }

    /// At a publish: the lines kept now are those on the frame; scrolled back, the offset grows
    /// by those kept since the last publish, so the same lines stay in view. Another history
    /// (another token: a new game's state, whatever its lines) returns the view to the live
    /// bottom. The vector of lines requests storage only for the lines published (`reserve_exact`):
    /// the most lines one publish took, at most [`HISTORY`]; the allocator may give more than
    /// asked (gpt a, round 4), and the tests measure what it gave on the host.
    pub fn publish(&mut self, sb: &Scrollback) {
        let same = Rc::ptr_eq(&self.generation, &sb.generation.0);
        if same && sb.total == self.published {
            return;
        }
        // The lines kept since the last publish, in the same history only.
        let new = sb.total.checked_sub(self.published).filter(|_| same);
        self.generation = Rc::clone(&sb.generation.0);
        self.published = sb.total;
        self.lines.clear();
        self.lines.reserve_exact(sb.lines.len());
        self.lines.extend(sb.lines.iter().cloned());
        if self.offset > 0 {
            self.offset = match new {
                Some(n) => {
                    let n = usize::try_from(n).unwrap_or(usize::MAX);
                    self.offset.saturating_add(n).min(self.kept())
                }
                None => 0,
            };
            self.stamp += 1;
        }
    }

    /// The wheel at frame pixel (x, y), `dy` lines (< 0 older, > 0 newer): over the message window
    /// while `game` runs, the offset moves by -`dy` within the lines kept, and the wheel is taken
    /// (true); elsewhere nothing changes. `dy` 0 moves nothing: whether the wheel is taken there.
    pub fn wheel(&mut self, ui: &UiState, game: bool, (x, y): (i16, i16), dy: i32) -> bool {
        let m = messages(ui);
        if !game || !over(&m, x, y) {
            return false;
        }
        let kept = self.kept() as i64;
        let to = (self.offset as i64 - i64::from(dy)).clamp(0, kept);
        self.set(to as usize);
        true
    }

    /// One input event before the engine queues it. A key, a repeat and typed text return the view
    /// to the live bottom. A press with no button held before it, inside the message window while
    /// scrolled back, is the view's until every button is up, its events queued with no button
    /// held (the sprite follows them, the game sees moves): on the bar (shown only then) it
    /// scrolls, anywhere else in the window it returns to the live bottom. Any other press is the
    /// game's and returns to the live bottom.
    pub fn input(&mut self, ui: &UiState, game: bool, e: InputEvent) -> InputEvent {
        let InputEvent::Pointer { x, y, buttons } = e else {
            if !matches!(e, InputEvent::Release(_)) {
                self.leave();
            }
            return e;
        };
        let held = buttons & 3;
        let before = self.buttons;
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
        let pressed = held & !before;
        if pressed == 0 {
            return e;
        }
        let m = messages(ui);
        // A button the game holds makes the press the game's too: the game sees every change.
        if before != 0 || !(game && self.offset > 0 && over(&m, x, y)) {
            self.set(0);
            return e;
        }
        self.gesture = Gesture::Held;
        if !on_bar(&m, x, y) {
            self.set(0);
        } else if pressed & POINTER_LEFT != 0 {
            let rows = usize::from(m.height);
            let kept = self.kept();
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
        let kept = self.kept();
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

//! Port-only (spec 17 Presentation, unit M3): the context menu drawn in the game's frame and run by
//! the game's own pointer and keys.
//!
//! [`menu::query`] says what a right press can do; this module lays the answer out as the
//! original's text window (paper, ink, the frame the paper draws round the message scroll), draws
//! it over the published frame ([`Open::draw`], into the engine's screen frame, never into the
//! game's screen buffers) and takes the input that goes to it ([`Popup::input`]): nothing it takes
//! reaches the game's queue, and nothing of it is in [`State`] or the savegame. A choice runs as
//! the original's mouse does ([`menu::Run`]).

use std::fmt;

use u6_render::{Framebuffer, HEIGHT, WIDTH};

use super::menu::{self, Entry, Kind, Menu, Reason, Run, Step};
use super::text::{self, CELL, TextWindow, WindowId};
use super::wide::Language;
use crate::engine::{InputEvent, Io, Key, POINTER_LEFT, POINTER_RIGHT, Pointer};
use crate::objects::Slot;
use crate::state::State;

/// The glyph the original marks the current line of a list with: the spellbook page's marker
/// in column 0 (0x116218, `magic/book.rs`).
const MARKER: u16 = 0x1a;
/// The most lines of the paper's frame drawn round the menu ([`frame_rings`]).
const MAX_RINGS: usize = 6;
/// The inventory view's paper doll ends at panel x 0xf0 (the equipment cells of 0x111eec end
/// there, the list cells of 0x111e3c start at 0xf8, `inventory/panel.rs`), the panel starting at
/// x 0xa8: in Wide the page shows the doll and the list apart.
const DOLL_END: i32 = 0xf0 - 0xa8;
/// The inventory view's name row: panel rows 6..0x10 (the first equipment cell starts at y 0x10).
const NAME_ROWS: i32 = 0x10 - 6;
/// The inventory view's slot among [`crate::geom::SLOTS`] (roster, stats, inventory).
const INVENTORY_SLOT: usize = 2;

/// Unit M4's travel state (spec 21), as the menu holds it. Stub until M4 lands in integ: the
/// targets asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Travel {
    /// The map cells "Go here" asked for, in order.
    pub asked: Vec<(i16, i16)>,
}

/// Unit M4's seam (spec 21): why travel to map cell `target` (world x, y on the leader's level)
/// cannot start now; `None` when it can. Stub until M4 lands in integ: every cell can.
pub fn travel_reason(_s: &State, _target: (i16, i16)) -> Option<Reason> {
    None
}

/// Unit M4's seam (spec 21): starts travel to map cell `target` (world x, y on the leader's
/// level). Stub until M4 lands in integ: the target is only recorded.
pub fn travel_to(travel: &mut Travel, target: (i16, i16)) {
    travel.asked.push(target);
}

/// The port's words of the menu, by language (spec 17: one table, the page has none).
fn command_word(code: u16, korean: bool) -> &'static str {
    let (en, ko) = match code {
        menu::ATTACK => ("Attack", "공격"),
        menu::CAST => ("Cast here", "여기에 주문 시전"),
        menu::TALK => ("Talk", "대화"),
        menu::LOOK => ("Look", "살피기"),
        menu::GET => ("Get", "줍기"),
        menu::DROP => ("Drop", "내려놓기"),
        menu::MOVE => ("Move", "옮기기"),
        menu::USE => ("Use", "사용"),
        menu::REST => ("Rest", "휴식"),
        menu::COMBAT => ("Combat", "전투"),
        menu::OPEN => ("Open", "열기"),
        menu::READY => ("Ready", "장착"),
        menu::UNREADY => ("Take off", "장착 해제"),
        menu::GO_HERE => ("Go here", "여기로 이동"),
        _ => ("?", "?"),
    };
    if korean { ko } else { en }
}

/// The words of a reason (the note row).
fn reason_word(r: Reason, korean: bool) -> &'static str {
    let (en, ko) = match r {
        Reason::Far => ("Too far away", "너무 멂"),
        Reason::Heavy => ("Too heavy to carry", "너무 무거워 들 수 없음"),
        Reason::Aboard => ("Not while aboard", "배에 탄 동안은 안 됨"),
        Reason::Window => ("Too far for this", "명령이 닿지 않는 곳"),
        Reason::Hands => ("No free hand", "빈손이 없음"),
        Reason::Book => ("No spellbook ready", "주문책을 들고 있지 않음"),
        Reason::Negate => ("Magic is negated", "마법이 봉인됨"),
        Reason::Spells => ("No spells in book", "책에 주문이 없음"),
        Reason::Level => ("Level too low", "주문에 비해 레벨이 낮음"),
        Reason::Points => ("Not enough magic", "마력이 모자람"),
        Reason::Reagents => ("No reagents", "주문의 시약이 없음"),
        Reason::Solo => ("Not in solo mode", "단독 행동 중에는 안 됨"),
        Reason::Door => ("Something is on it", "무언가 막고 있음"),
        Reason::Hand => ("Held by another", "다른 일행이 들고 있음"),
        Reason::Case => ("Cannot use it now", "지금은 쓸 수 없음"),
        Reason::Screen => ("Not in view", "보이지 않음"),
        Reason::Silent => ("No answer", "대답이 없음"),
        Reason::Lying => ("Lying down", "누워 있음"),
        Reason::Ammo => ("No arrows or bolts", "화살이나 볼트가 없음"),
        Reason::Blocked => ("A foe is too close", "적이 가로막고 있음"),
        Reason::Stays => ("Will not come off", "벗겨지지 않음"),
        Reason::Locked => ("Locked", "잠겨 있음"),
        Reason::Unreachable => ("No way there", "갈 수 없는 곳"),
    };
    if korean { ko } else { en }
}

/// The theft mark (the note row of Get of what is not the party's).
fn theft_word(korean: bool) -> &'static str {
    if korean { "도둑질" } else { "Stealing" }
}

/// The cell codes of `text` ([`super::wide`]): ASCII as itself, a Hangul syllable as its code,
/// anything else as `?`.
fn cells(text: &str) -> Vec<u16> {
    text.chars()
        .map(|c| {
            let u = u32::from(c);
            if u < 0x80 || super::wide::is_hangul(u as i32) {
                u as u16
            } else {
                u16::from(b'?')
            }
        })
        .collect()
}

/// One command of the drawn menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub code: u16,
    /// The object it resolves to (-1: the cell's tile, or none).
    pub object: Slot,
    pub reason: Option<Reason>,
    pub theft: bool,
}

impl Item {
    fn of(e: Entry, object: Slot) -> Item {
        Item {
            code: e.code,
            object,
            reason: e.reason,
            theft: e.theft,
        }
    }

    pub fn enabled(&self) -> bool {
        self.reason.is_none()
    }

    /// What the note row shows for it: its reason, else its theft mark.
    fn note(&self, korean: bool) -> Option<&'static str> {
        match (self.reason, self.theft) {
            (Some(r), _) => Some(reason_word(r, korean)),
            (None, true) => Some(theft_word(korean)),
            (None, false) => None,
        }
    }
}

/// One row of the drawn menu.
#[derive(Clone, PartialEq, Eq)]
pub enum Row {
    /// A section's heading: its object's display name as cell codes (the user's game text).
    Head(Vec<u16>),
    Entry(Item),
    /// The note of the highlighted entry (its reason or its theft mark).
    Note,
}

/// A heading's length only: its cells are the user's game text, so a failed comparison or a debug
/// print never shows them.
impl fmt::Debug for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Row::Head(c) => write!(f, "Head({} cells)", c.len()),
            Row::Entry(i) => write!(f, "Entry({i:?})"),
            Row::Note => write!(f, "Note"),
        }
    }
}

/// The open menu: what it lists and where it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub kind: Kind,
    /// The pressed frame pixel (the target of a command chosen).
    pub at: (i16, i16),
    /// The pressed map cell (map menus).
    pub cell: Option<(i16, i16)>,
    pub rows: Vec<Row>,
    /// The highlighted row (an entry's).
    pub hover: usize,
    /// The text area in cells: first column, first row, columns, rows.
    pub text: (i32, i32, i32, i32),
    /// The frame's colours, inner line first ([`frame_rings`]).
    pub rings: Vec<u8>,
    pub korean: bool,
}

impl Open {
    /// The menu of `menu` for a press at frame pixel `at`, laid out for `s` (its language, its
    /// paper, its click regions), kept off the frame rects `hidden` where it can ([`place`]).
    pub fn new(menu: &Menu, at: (i16, i16), s: &State, hidden: &[Rect]) -> Open {
        let korean = s.ui.text.language == Language::Korean;
        let mut rows = Vec::new();
        // The underlined command first: Rest and Combat only there, any other as the first entry
        // that has it.
        let mut first = None;
        if let Some(u) = menu.underlined {
            if u == menu::REST || u == menu::COMBAT {
                rows.push(Row::Entry(Item {
                    code: u,
                    object: -1,
                    reason: None,
                    theft: false,
                }));
            } else if let Some((s, e)) = menu
                .sections
                .iter()
                .find_map(|s| s.entries.iter().find(|e| e.code == u).map(|e| (s, *e)))
            {
                first = Some((s.object, u));
                rows.push(Row::Entry(Item::of(e, s.object)));
            }
        }
        for section in &menu.sections {
            let entries: Vec<Item> = section
                .entries
                .iter()
                .filter(|e| first != Some((section.object, e.code)))
                .map(|&e| Item::of(e, section.object))
                .collect();
            if entries.is_empty() {
                continue;
            }
            let head = cells(&section.name);
            if !head.is_empty() {
                rows.push(Row::Head(head));
            }
            rows.extend(entries.into_iter().map(Row::Entry));
        }
        if let Some(e) = menu.go_here {
            rows.push(Row::Entry(Item::of(e, -1)));
        }
        let noted = rows
            .iter()
            .any(|r| matches!(r, Row::Entry(i) if i.note(korean).is_some()));
        let wide = |r: &Row| match r {
            Row::Head(c) => c.len(),
            Row::Entry(i) => 1 + cells(command_word(i.code, korean)).len(),
            Row::Note => 0,
        };
        let mut cols = rows.iter().map(wide).max().unwrap_or(1);
        if noted {
            let note = |r: &Row| match r {
                Row::Entry(i) => i.note(korean).map_or(0, |n| cells(n).len()),
                _ => 0,
            };
            cols = cols.max(rows.iter().map(note).max().unwrap_or(0));
            rows.push(Row::Note);
        }
        let rings = frame_rings(s);
        let want = (cols as i32, rows.len() as i32);
        let ring = rings.len() as i32;
        let whole = (0, 0, WIDTH as i32 - 1, HEIGHT as i32 - 1);
        let text = place(at, want, ring, bounds(s, at), hidden)
            .or_else(|| place(at, want, ring, whole, hidden))
            .unwrap_or((0, 0, 1, 1));
        cut(&mut rows, text.3 as usize, noted);
        let hover = rows
            .iter()
            .position(|r| matches!(r, Row::Entry(i) if i.enabled()))
            .or_else(|| rows.iter().position(|r| matches!(r, Row::Entry(_))))
            .unwrap_or(0);
        Open {
            kind: menu.kind,
            at,
            cell: menu.cell,
            rows,
            hover,
            text,
            rings,
            korean,
        }
    }

    /// The box with its frame, in frame pixels (x0, y0, x1, y1 inclusive).
    pub fn rect(&self) -> (i32, i32, i32, i32) {
        let (x0, y0, x1, y1) = self.text_rect();
        let r = self.rings.len() as i32;
        (x0 - r, y0 - r, x1 + r, y1 + r)
    }

    /// The text area in frame pixels (x0, y0, x1, y1 inclusive).
    pub fn text_rect(&self) -> (i32, i32, i32, i32) {
        let (c, r, w, h) = self.text;
        let px = CELL as i32;
        (c * px, r * px, (c + w) * px - 1, (r + h) * px - 1)
    }

    /// The row at frame pixel (x, y), if it is in the text area.
    pub fn row_at(&self, x: i16, y: i16) -> Option<usize> {
        let (x0, y0, x1, y1) = self.text_rect();
        let (x, y) = (i32::from(x), i32::from(y));
        ((x0..=x1).contains(&x) && (y0..=y1).contains(&y))
            .then(|| ((y - y0) / CELL as i32) as usize)
            .filter(|&i| i < self.rows.len())
    }

    /// Frame pixel (x, y) is in the box, its frame included.
    pub fn contains(&self, x: i16, y: i16) -> bool {
        let (x0, y0, x1, y1) = self.rect();
        (x0..=x1).contains(&i32::from(x)) && (y0..=y1).contains(&i32::from(y))
    }

    /// The item of row `i`, if it is an entry.
    pub fn item(&self, i: usize) -> Option<Item> {
        match self.rows.get(i) {
            Some(Row::Entry(item)) => Some(*item),
            _ => None,
        }
    }

    /// The entry rows, in order.
    fn entries(&self) -> Vec<usize> {
        (0..self.rows.len())
            .filter(|&i| self.item(i).is_some())
            .collect()
    }

    /// Moves the highlight `by` entries (wrapping); true when it moved.
    fn step_hover(&mut self, by: isize) -> bool {
        let entries = self.entries();
        let Some(at) = entries.iter().position(|&i| i == self.hover) else {
            return false;
        };
        let n = entries.len() as isize;
        let next = entries[(at as isize + by).rem_euclid(n) as usize];
        std::mem::replace(&mut self.hover, next) != next
    }

    /// Draws the menu over `fb` as the original draws a text window: the paper filled
    /// ([`super::text::fill_rect`]'s colours, A4-0x656c), the frame of the paper's message scroll
    /// round it, the rows in the game's font (0x105228, [`super::text::draw_char`]): headings in
    /// the alert colour (A4-0x6576, the `@` keywords' of 0x108376) from column 0, entries in ink
    /// (A4-0x656a) from column 1, glyph 0x1a in column 0 of the highlighted one (0x116218's mark),
    /// a disabled entry in 0x134044's checker, the note of the highlighted entry in ink. The fill,
    /// the frame, the mark and the checker are classic pixels; the glyphs drawn in full name their
    /// glyphs (spec 11), so the HD font draws them.
    pub fn draw(&self, fb: &mut Framebuffer, s: &State) {
        let c = s.ui.tables.colours;
        let fonts = (s.ui.font.as_ref(), s.ui.hangul.as_deref());
        let (x0, y0, x1, y1) = self.text_rect();
        fill(fb, (x0, y0, x1, y1), c.paper);
        for (k, &colour) in (1i32..).zip(&self.rings) {
            ring(fb, (x0 - k, y0 - k, x1 + k, y1 + k), colour);
        }
        let (col0, row0, cols, _) = self.text;
        let window = |fg| TextWindow {
            fg,
            bg: c.paper,
            ..TextWindow::default()
        };
        let put = |fb: &mut Framebuffer, fg: u8, at: i32, row: i32, codes: &[u16]| {
            let w = window(fg);
            for (k, &code) in codes.iter().enumerate().take((cols - at).max(0) as usize) {
                let cell = ((col0 + at + k as i32) as u16, row as u16);
                text::draw_char(fb, fonts.0, fonts.1, &w, code, cell.0, cell.1);
            }
        };
        let note = self
            .item(self.hover)
            .and_then(|i| i.note(self.korean))
            .map(cells);
        for (i, r) in self.rows.iter().enumerate() {
            let row = row0 + i as i32;
            match r {
                Row::Head(head) => put(fb, c.alert, 0, row, head),
                Row::Entry(item) => {
                    let label = cells(command_word(item.code, self.korean));
                    if i == self.hover {
                        let w = window(c.text);
                        text::draw_symbol(fb, fonts, &w, MARKER, (col0 as u16, row as u16), None);
                    }
                    if item.enabled() {
                        put(fb, c.text, 1, row, &label);
                    } else {
                        let n = label.len().min((cols - 1).max(0) as usize);
                        for (k, &code) in label[..n].iter().enumerate() {
                            let x = (col0 + 1 + k as i32) * CELL as i32;
                            checker(fb, fonts, code, (x, row * CELL as i32), (c.text, c.paper));
                        }
                    }
                }
                Row::Note => put(fb, c.text, 0, row, note.as_deref().unwrap_or(&[])),
            }
        }
    }

    /// The view of the drawn menu ([`View`]).
    pub fn view(&self) -> View {
        let rows = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let y = (self.text.1 + i as i32) * CELL as i32;
                match r {
                    Row::Head(_) => RowView {
                        kind: RowKind::Head,
                        code: None,
                        reason: None,
                        theft: false,
                        y,
                    },
                    Row::Entry(item) => RowView {
                        kind: RowKind::Entry,
                        code: Some(item.code),
                        reason: item.reason,
                        theft: item.theft,
                        y,
                    },
                    Row::Note => RowView {
                        kind: RowKind::Note,
                        code: None,
                        reason: None,
                        theft: false,
                        y,
                    },
                }
            })
            .collect();
        View {
            rect: self.rect(),
            text: self.text_rect(),
            hover: self.hover,
            rows,
        }
    }
}

/// What a row of the drawn menu is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Head,
    Entry,
    Note,
}

/// One row of [`View`]: no text, codes only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowView {
    pub kind: RowKind,
    pub code: Option<u16>,
    pub reason: Option<Reason>,
    pub theft: bool,
    /// The row's top pixel row.
    pub y: i32,
}

impl RowView {
    pub fn enabled(&self) -> bool {
        self.kind == RowKind::Entry && self.reason.is_none()
    }
}

/// The drawn menu as tests and the web build read it (spec 17 Engine API).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    /// The box with its frame (x0, y0, x1, y1 inclusive).
    pub rect: (i32, i32, i32, i32),
    /// The text area (x0, y0, x1, y1 inclusive), 8 pixels per row.
    pub text: (i32, i32, i32, i32),
    /// The highlighted row.
    pub hover: usize,
    pub rows: Vec<RowView>,
}

/// The rows past `room` cut off, the note kept as the last row when there is one.
fn cut(rows: &mut Vec<Row>, room: usize, noted: bool) {
    if rows.len() <= room {
        return;
    }
    rows.truncate(room.saturating_sub(usize::from(noted)));
    if noted {
        rows.push(Row::Note);
    }
}

/// The frame round the paper's message scroll as the boot drew it (0x1350d8, [`State`]'s
/// `ui.paper`): the colours on the row through the scroll window's middle, from the pixel left of
/// its first column outward, up to the paper's own colour (A4-0x656c) again, at most
/// [`MAX_RINGS`]; one line of ink without the paper or that frame.
pub fn frame_rings(s: &State) -> Vec<u8> {
    let c = s.ui.tables.colours;
    let w = s.ui.text.window(WindowId::Messages);
    let y = usize::from(w.y) * CELL + usize::from(w.height) * CELL / 2;
    let mut out = Vec::new();
    if let Some(p) = &s.ui.paper {
        let mut x = usize::from(w.x) * CELL;
        while x > 0 && out.len() < MAX_RINGS {
            x -= 1;
            match p.at(x, y) {
                Some(colour) if colour != c.paper => out.push(colour),
                _ => break,
            }
        }
    }
    if out.is_empty() {
        out.push(c.text);
    }
    out
}

/// The bounds a menu for a press at frame pixel `at` keeps to, rings included (x0, y0, x1, y1
/// inclusive): the region pressed, as the HD layout and Wide show it apart. The shown map
/// ([`crate::geom::SHOWN`]), the side panel (click region 2), a Wide slot, the inventory view's
/// slot only its doll or its list part below its name row; the whole frame elsewhere.
fn bounds(s: &State, (x, y): (i16, i16)) -> (i32, i32, i32, i32) {
    let whole = (0, 0, WIDTH as i32 - 1, HEIGHT as i32 - 1);
    let Some(i) = super::click::region_at(s, x, y) else {
        return whole;
    };
    if i == 0 {
        let (x0, y0, x1, y1) = crate::geom::SHOWN;
        return (i32::from(x0), i32::from(y0), i32::from(x1), i32::from(y1));
    }
    let Some([x0, y0, x1, y1]) = super::pointer::region(&s.ui.pointer.tables, i) else {
        return whole;
    };
    let (mut x0, mut y0, mut x1, y1) = (i32::from(x0), i32::from(y0), i32::from(x1), i32::from(y1));
    let slots = super::pointer::SLOT_REGIONS;
    if slots.contains(&i) && i - slots.start == INVENTORY_SLOT {
        let split = x0 + DOLL_END;
        y0 += NAME_ROWS;
        if i32::from(x) < split {
            x1 = split - 1;
        } else {
            x0 = split;
        }
    }
    (x0, y0, x1, y1)
}

/// A frame rect, x0, y0, x1, y1 inclusive.
pub type Rect = (i32, i32, i32, i32);

/// The pixels `a` and `b` share.
fn overlap(a: Rect, b: Rect) -> i64 {
    let w = i64::from(a.2.min(b.2) - a.0.max(b.0) + 1).max(0);
    let h = i64::from(a.3.min(b.3) - a.1.max(b.1) + 1).max(0);
    w * h
}

/// The text area (first column, first row, columns, rows; cells) for `want` cells beside the
/// press at frame pixel `at`, its `rings` inside the bounds `b` (x0, y0, x1, y1 inclusive): one
/// cell right of and below the pressed cell, flipped to the other side where it would not fit,
/// then held inside; as much of `want` as fits. `None` when not one cell fits.
///
/// Of those places (beside the press on each side, in that order) the first whose box, rings
/// included, meets none of `hidden` (what the platform shows over the frame there: Wide's page
/// windows, the map's parts off its canvas); with none such, the place in the bounds whose box
/// meets the fewest hidden pixels, the nearest to the first place among those.
fn place(
    at: (i16, i16),
    want: (i32, i32),
    rings: i32,
    b: Rect,
    hidden: &[Rect],
) -> Option<(i32, i32, i32, i32)> {
    let px = CELL as i32;
    // An axis: its first cell and its end in the bounds, the cells it gets, and the starts beside
    // the press (after it, else before it; then before it, held inside).
    let axis = |p: i32, n: i32, lo: i32, hi: i32| {
        let first = (lo + rings + px - 1).div_euclid(px);
        let end = (hi - rings + 1).div_euclid(px);
        let room = end - first;
        if room < 1 {
            return None;
        }
        let n = n.clamp(1, room);
        let cell = p.div_euclid(px);
        let after = if cell + 1 + n > end {
            cell - n
        } else {
            cell + 1
        };
        let before = cell - n;
        let hold = |v: i32| v.clamp(first, end - n);
        Some((first, end, n, [hold(after), hold(before)]))
    };
    let (cx0, cx1, cols, xs) = axis(i32::from(at.0), want.0, b.0, b.2)?;
    let (cy0, cy1, rows, ys) = axis(i32::from(at.1), want.1, b.1, b.3)?;
    let boxed = |col: i32, row: i32| {
        (
            col * px - rings,
            row * px - rings,
            (col + cols) * px - 1 + rings,
            (row + rows) * px - 1 + rings,
        )
    };
    let hides =
        |col: i32, row: i32| -> i64 { hidden.iter().map(|&h| overlap(boxed(col, row), h)).sum() };
    let beside = [
        (xs[0], ys[0]),
        (xs[1], ys[0]),
        (xs[0], ys[1]),
        (xs[1], ys[1]),
    ];
    let (col, row) = beside
        .iter()
        .copied()
        .find(|&(c, r)| hides(c, r) == 0)
        .unwrap_or_else(|| {
            let (c0, r0) = beside[0];
            let mut best = (hides(c0, r0), 0, (c0, r0));
            for r in cy0..=cy1 - rows {
                for c in cx0..=cx1 - cols {
                    let near = (c - c0).abs() + (r - r0).abs();
                    let this = (hides(c, r), near, (c, r));
                    if (this.0, this.1) < (best.0, best.1) {
                        best = this;
                    }
                }
            }
            best.2
        });
    Some((col, row, cols, rows))
}

/// Fills the inclusive rect with `colour`, classic pixels (what lies past the frame skipped).
fn fill(fb: &mut Framebuffer, (x0, y0, x1, y1): (i32, i32, i32, i32), colour: u8) {
    for y in y0.max(0)..=y1 {
        for x in x0.max(0)..=x1 {
            fb.set(x as usize, y as usize, colour);
        }
    }
}

/// The one-pixel outline of the inclusive rect in `colour`, classic pixels.
fn ring(fb: &mut Framebuffer, (x0, y0, x1, y1): (i32, i32, i32, i32), colour: u8) {
    let mut put = |x: i32, y: i32| {
        if x >= 0 && y >= 0 {
            fb.set(x as usize, y as usize, colour);
        }
    };
    for x in x0..=x1 {
        put(x, y0);
        put(x, y1);
    }
    for y in y0..=y1 {
        put(x0, y);
        put(x1, y);
    }
}

/// The rows of cell code `c` as 0x105228 draws it ([`super::text::draw_char`]): a glyph of
/// `u6.ch` below 0x100, a syllable of the Hangul table; `None` without them.
fn glyph_rows(
    (font, hangul): (
        Option<&u6_formats::font::BitFont>,
        Option<&u6_formats::font::HangulFont>,
    ),
    c: u16,
) -> Option<[u8; 8]> {
    match u8::try_from(c) {
        Ok(b) => font.map(|f| f.glyphs[usize::from(b)]),
        Err(_) => hangul.and_then(|h| h.glyph(u32::from(c))).copied(),
    }
}

/// Cell code `c` at frame pixel (x, y) in 0x134044's checker: of its ink pixels only those with
/// `(x + y) & 1` = 0 in `ink`, every other pixel `paper`; classic pixels.
fn checker(
    fb: &mut Framebuffer,
    fonts: (
        Option<&u6_formats::font::BitFont>,
        Option<&u6_formats::font::HangulFont>,
    ),
    c: u16,
    (x, y): (i32, i32),
    (ink, paper): (u8, u8),
) {
    let rows = glyph_rows(fonts, c).unwrap_or([0; 8]);
    for (dy, &bits) in rows.iter().enumerate() {
        for dx in 0..CELL {
            let (px, py) = (x + dx as i32, y + dy as i32);
            let set = bits & (0x80 >> dx) != 0 && (px + py) & 1 == 0;
            if px >= 0 && py >= 0 {
                fb.set(px as usize, py as usize, if set { ink } else { paper });
            }
        }
    }
}

/// An event that changes nothing the game reads (as [`menu::Run::step`]'s quiet asks): a move of
/// a button-less pointer, a key's release (never queued, [`InputEvent::Release`]).
fn inert(ev: &InputEvent) -> bool {
    match ev {
        InputEvent::Pointer { buttons, .. } => buttons & 3 == 0,
        InputEvent::Release(_) => true,
        _ => false,
    }
}

/// The original's command letters at the prompt (`main_loop/commands.rs` `letter_code`).
fn letter_code(c: char) -> Option<u16> {
    Some(match c.to_ascii_uppercase() {
        'A' => menu::ATTACK,
        'B' => menu::COMBAT,
        'C' => menu::CAST,
        'D' => menu::DROP,
        'G' => menu::GET,
        'L' => menu::LOOK,
        'M' => menu::MOVE,
        'R' => menu::REST,
        'T' => menu::TALK,
        'U' => menu::USE,
        _ => return None,
    })
}

/// The engine's context menu: on or off, the open menu, the chosen entry still running, and what
/// of the input is the menu's (spec 17 Presentation, Input).
#[derive(Clone, Debug, Default)]
pub struct Popup {
    on: bool,
    open: Option<Open>,
    /// The chosen command whose target press waits for its target read.
    run: Option<Run>,
    /// The pointer's buttons in the last pointer event of the input.
    buttons: u8,
    /// The pointer's events are the menu's until its buttons are all up.
    swallow: bool,
    /// The key the menu took last: its repeats are the menu's until its release.
    key: Option<Key>,
    /// The pointer the menu took last, its buttons left out: the sprite's place while the game's
    /// own sample stays where the game took it.
    pointer: Option<Pointer>,
    /// Moves at every change of what is drawn.
    version: u64,
    /// Frame rects the platform shows something else over ([`Popup::set_hidden`]).
    hidden: Vec<Rect>,
    /// Unit M4's travel state (spec 21).
    pub travel: Travel,
}

impl Popup {
    /// On or off (off closes it and drops a run).
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
        if !on {
            self.run = None;
            self.close();
        }
    }

    pub fn on(&self) -> bool {
        self.on
    }

    /// The frame rects the platform shows something else over (Wide's page windows over the
    /// map, the map's parts off its canvas): a menu opened after this keeps off them where it
    /// can ([`Open::new`]). None by default.
    pub fn set_hidden(&mut self, rects: Vec<Rect>) {
        self.hidden = rects;
    }

    /// The open menu.
    pub fn open(&self) -> Option<&Open> {
        self.open.as_ref()
    }

    /// The chosen command still waiting for its target read.
    pub fn run(&self) -> Option<Run> {
        self.run
    }

    /// Moves at every change of what is drawn (open, closed, the highlight).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The pointer the menu took last (buttons left out), while its events are the menu's.
    pub fn pointer(&self) -> Option<Pointer> {
        self.pointer
    }

    /// Opens `open` as a right press would (tests: a menu without a query).
    #[cfg(test)]
    pub(crate) fn show(&mut self, open: Open) {
        self.open = Some(open);
        self.version += 1;
    }

    fn close(&mut self) {
        if self.open.take().is_some() {
            self.version += 1;
        }
    }

    /// The input of one tick as the game is to get it: the run stepped first (its target press
    /// after this tick's events, [`Run::step`]), then each event either the menu's or the game's,
    /// with the presses of a choice in its place. `quiet`: nothing is queued for the game.
    pub fn input(
        &mut self,
        s: &State,
        io: &Io,
        input: &[InputEvent],
        quiet: bool,
    ) -> Vec<InputEvent> {
        let mut tail = Vec::new();
        if let Some(run) = self.run {
            match run.step(s, quiet && input.iter().all(inert)) {
                Step::Hold => {}
                Step::Release(press) => {
                    tail.extend(press);
                    self.run = None;
                }
                Step::Drop => self.run = None,
            }
        }
        let mut out = Vec::with_capacity(input.len() + tail.len());
        let mut calm = quiet;
        for &ev in input {
            if !self.take(s, io, ev, calm, &mut out) {
                calm &= inert(&ev);
                out.push(ev);
            }
        }
        out.extend(tail);
        out
    }

    /// After the game's poll: a menu whose prompt went (A4-0x7a2c no longer 1) closes.
    pub fn after_poll(&mut self, s: &State) {
        if self.open.is_some() && s.main.reading != 1 {
            self.close();
            self.pointer = None;
        }
    }

    /// Takes `ev` when it is the menu's (true), else leaves it to the game; the presses of a
    /// choice go to `out`. `calm`: nothing queued for the game, nothing but inert events before.
    fn take(
        &mut self,
        s: &State,
        io: &Io,
        ev: InputEvent,
        calm: bool,
        out: &mut Vec<InputEvent>,
    ) -> bool {
        match ev {
            InputEvent::Pointer { x, y, buttons } => {
                self.pointer_event(s, io, (x, y), buttons, calm, out)
            }
            InputEvent::Key(k) | InputEvent::Repeat(k) => {
                let repeat = matches!(ev, InputEvent::Repeat(_));
                if self.open.is_none() {
                    return repeat && self.key == Some(k);
                }
                self.key = Some(k);
                self.key_event(s, k, repeat, out);
                true
            }
            InputEvent::Release(k) => {
                if self.key == Some(k) {
                    self.key = None;
                }
                false
            }
            InputEvent::Text(_) => self.open.is_some(),
        }
    }

    fn pointer_event(
        &mut self,
        s: &State,
        io: &Io,
        (x, y): (i16, i16),
        buttons: u8,
        calm: bool,
        out: &mut Vec<InputEvent>,
    ) -> bool {
        let b = buttons & (POINTER_LEFT | POINTER_RIGHT);
        let before = std::mem::replace(&mut self.buttons, b);
        let down = b & !before;
        let at = Pointer { x, y, buttons: 0 };
        if self.swallow {
            self.swallow = b != 0;
            if !self.swallow && self.open.is_none() {
                // The release of a press that closed the menu: the game's pointer comes here, a
                // button-less move.
                self.pointer = None;
                return false;
            }
            self.pointer = Some(at);
            self.hover_at(x, y);
            return true;
        }
        if self.open.is_none() {
            let press = b == POINTER_RIGHT && before == 0;
            if press && calm && self.run.is_none() && s.main.reading == 1 {
                let open = menu::query(s, io, x, y).map(|m| Open::new(&m, (x, y), s, &self.hidden));
                if let Some(open) = open.filter(|o| !o.entries().is_empty()) {
                    self.open = Some(open);
                    self.version += 1;
                    self.swallow = true;
                    self.pointer = Some(at);
                    return true;
                }
            }
            self.pointer = None;
            return false;
        }
        self.pointer = Some(at);
        if down & POINTER_RIGHT != 0 {
            self.swallow = true;
            self.close_here(out);
            return true;
        }
        if down & POINTER_LEFT != 0 {
            self.swallow = true;
            let open = self.open.as_ref().expect("open");
            match open.row_at(x, y) {
                Some(i) => {
                    if open.item(i).is_some_and(|item| item.enabled()) {
                        self.choose(s, i, out);
                    }
                }
                None if open.contains(x, y) => {}
                None => self.close_here(out),
            }
            return true;
        }
        self.hover_at(x, y);
        true
    }

    fn key_event(&mut self, s: &State, k: Key, repeat: bool, out: &mut Vec<InputEvent>) {
        let moved = match k {
            Key::Up => self.open.as_mut().is_some_and(|o| o.step_hover(-1)),
            Key::Down => self.open.as_mut().is_some_and(|o| o.step_hover(1)),
            _ if repeat => false,
            Key::Enter | Key::Space => {
                let open = self.open.as_ref().expect("open");
                if open.item(open.hover).is_some_and(|i| i.enabled()) {
                    let i = open.hover;
                    self.choose(s, i, out);
                }
                false
            }
            Key::Escape => {
                self.close_here(out);
                false
            }
            Key::Char(c) => {
                if let Some(code) = letter_code(c) {
                    let open = self.open.as_ref().expect("open");
                    let row = (0..open.rows.len()).find(|&i| {
                        open.item(i)
                            .is_some_and(|it| it.code == code && it.enabled())
                    });
                    if let Some(i) = row {
                        self.choose(s, i, out);
                    }
                }
                false
            }
            _ => false,
        };
        if moved {
            self.version += 1;
        }
    }

    /// The highlight on the entry at frame pixel (x, y), if there is one.
    fn hover_at(&mut self, x: i16, y: i16) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if let Some(i) = open.row_at(x, y).filter(|&i| open.item(i).is_some())
            && std::mem::replace(&mut open.hover, i) != i
        {
            self.version += 1;
        }
    }

    /// Closes the menu with the game's pointer sample brought to where the pointer is now (a
    /// button-less move, so the game draws its shape there).
    fn close_here(&mut self, out: &mut Vec<InputEvent>) {
        self.close();
        if let Some(p) = self.pointer.take() {
            out.push(InputEvent::Pointer {
                x: p.x,
                y: p.y,
                buttons: 0,
            });
        }
    }

    /// Runs row `i`'s entry (spec 17 Running a choice): Go here starts unit M4's travel, a slot's
    /// own action is a left click on the slot, Rest and Combat their icon's press, any other
    /// command its icon's press now and the press at the target once it reads it. The menu closes.
    fn choose(&mut self, s: &State, i: usize, out: &mut Vec<InputEvent>) {
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let Some(item) = open.item(i) else {
            return;
        };
        let (at, cell) = (open.at, open.cell);
        self.close_here(out);
        match item.code {
            menu::GO_HERE => {
                if let Some(cell) = cell {
                    travel_to(&mut self.travel, cell);
                }
            }
            menu::OPEN | menu::READY | menu::UNREADY => out.extend(menu::left_press(at.0, at.1)),
            code => match Run::start(s, code, at) {
                Some((icon, run)) => {
                    out.extend(icon);
                    self.run = Some(run);
                }
                None => out.extend(menu::icon_press(code)),
            },
        }
    }
}

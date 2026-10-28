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

/// Unit M4's request record (spec 21), the stub's: the map cells "Go here" asked travel to, in
/// order ([`travel_to`]; the engine holds it, [`crate::engine::Engine::go_here_asked`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Travel {
    pub asked: Vec<(i16, i16)>,
}

/// Unit M4's refusals (spec 21 `travel::Reason`) as "Go here" reads them ([`go_here_reason`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The leader stands on the cell.
    Here,
    /// A vessel carries the party.
    Aboard,
    /// Combat mode.
    Combat,
    /// A hostile is in view.
    Foe,
    /// The cell lies outside the window the search covers.
    Far,
    /// No walk inside the window reaches the cell.
    Unreachable,
    /// No leader stands on the map.
    NoLeader,
}

/// Unit M4's seam (spec 21 `travel::reachable`): why travel to map cell `target` (world x, y on
/// the leader's level) cannot start now; `None` when it can. Stub until M4 lands in integ: every
/// cell can.
pub fn travel_reason(_s: &State, _target: (i16, i16)) -> Option<Refusal> {
    None
}

/// How "Go here" shows refusal `r`: `Here` and `NoLeader` leave the entry out (`None`); every
/// other refusal disables it with its reason.
pub fn go_here_reason(r: Refusal) -> Option<Reason> {
    match r {
        Refusal::Here | Refusal::NoLeader => None,
        Refusal::Aboard => Some(Reason::Aboard),
        Refusal::Combat => Some(Reason::Combat),
        Refusal::Foe => Some(Reason::Foe),
        Refusal::Far => Some(Reason::Far),
        Refusal::Unreachable => Some(Reason::Unreachable),
    }
}

/// Unit M4's seam (spec 21 `Engine::travel_to`): starts travel to map cell `target` (world x, y
/// on the leader's level). Stub until M4 lands in integ: the target is only recorded. The engine
/// calls it with the cell the menu chose ([`Popup::take_go_here`]) right after the menu's input
/// filter.
pub fn travel_to(travel: &mut Travel, target: (i16, i16)) {
    travel.asked.push(target);
}

/// The longest a press after the press that chose an entry is taken for its double click (the
/// menu's, with its release), on the engine's clock: the common desktop default.
const DOUBLE_CLICK_MS: u64 = 500;
/// How far from the choosing press (frame pixels, each way) a press is its double click.
const DOUBLE_CLICK_PX: i16 = 4;

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
        Reason::Solo => ("Not allowed in solo mode", "단독 행동 중에는 안 됨"),
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
        Reason::Combat => ("Not during combat", "전투 중에는 안 됨"),
        Reason::Foe => ("A foe is in view", "적이 보임"),
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

    /// What the note row shows for it: its reason.
    fn note(&self, korean: bool) -> Option<&'static str> {
        self.reason.map(|r| reason_word(r, korean))
    }

    /// Its row's cells: the mark's column, its label, and its theft mark after a space.
    fn width(&self, korean: bool) -> usize {
        let theft = if self.theft {
            1 + cells(theft_word(korean)).len()
        } else {
            0
        };
        1 + cells(command_word(self.code, korean)).len() + theft
    }
}

/// Cell codes of the user's game text (a section's name): a debug print shows their count only.
#[derive(Clone, PartialEq, Eq)]
pub struct Cells(pub Vec<u16>);

impl fmt::Debug for Cells {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} cells", self.0.len())
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
    /// The name shown beside the first row's entry: the underlined command's section's, when the
    /// menu has more than one section (its own heading comes later).
    pub named: Option<Cells>,
    /// The text area its rows asked for, columns and rows (before the placement cut any).
    pub want: (i32, i32),
    /// What it was laid out from ([`Open::relaid`]).
    menu: Menu,
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
        let mut named = None;
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
                named = Some(Cells(cells(&s.name)))
                    .filter(|c| menu.sections.len() > 1 && !c.0.is_empty());
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
            .any(|r| matches!(r, Row::Entry(i) if i.reason.is_some()));
        if noted {
            rows.push(Row::Note);
        }
        let rings = frame_rings(s);
        let want = (width(&rows, named.as_ref(), korean), rows.len() as i32);
        let ring = rings.len() as i32;
        let whole = (0, 0, WIDTH as i32 - 1, HEIGHT as i32 - 1);
        let (x0, y0, x1, y1) = crate::geom::SHOWN;
        let map = (i32::from(x0), i32::from(y0), i32::from(x1), i32::from(y1));
        let region = bounds(s, at);
        // The region pressed; where it cannot hold the widest row (a Wide inventory slot's part),
        // the map, kept off what the platform hides; then the region as it can, the whole frame.
        let fits = |t: &(i32, i32, i32, i32)| t.2 >= want.0;
        let mut text = place(at, want, ring, region, hidden)
            .filter(fits)
            .or_else(|| place(at, want, ring, map, hidden).filter(fits))
            .or_else(|| place(at, want, ring, region, hidden))
            .or_else(|| place(at, want, ring, whole, hidden))
            .unwrap_or((0, 0, 1, 1));
        cut(&mut rows, text.3 as usize, noted);
        // The note row reserves only the reasons of the entries still shown.
        text.2 = text.2.min(width(&rows, named.as_ref(), korean)).max(1);
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
            named,
            want,
            menu: menu.clone(),
        }
    }

    /// The same menu laid out again for `s` and `hidden` (a change of language or of what the
    /// platform hides while it is open), the same entry highlighted.
    pub fn relaid(&self, s: &State, hidden: &[Rect]) -> Open {
        let mut o = Open::new(&self.menu, self.at, s, hidden);
        if let Some(item) = self.item(self.hover)
            && let Some(i) = (0..o.rows.len()).find(|&i| o.item(i) == Some(item))
        {
            o.hover = i;
        }
        o
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
    /// a disabled entry half-toned in 0x134044's checker ([`super::text::draw_halftone`]), an
    /// entry's theft mark and the first entry's section name after it in the alert colour, the
    /// note of the highlighted entry in ink. The fill, the frame and the mark are classic pixels;
    /// every glyph drawn names its glyph (spec 11; the half-toned ones flagged so), so the HD
    /// font draws them.
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
        // Cells `codes` from column `at` of `row`, as far as the text area reaches; the next
        // column after them.
        let put = |fb: &mut Framebuffer, fg: u8, half: bool, at: i32, row: i32, codes: &[u16]| {
            let w = window(fg);
            for (k, &code) in codes.iter().enumerate().take((cols - at).max(0) as usize) {
                let cell = ((col0 + at + k as i32) as u16, row as u16);
                if half {
                    text::draw_halftone(fb, fonts, &w, code, cell);
                } else {
                    text::draw_char(fb, fonts.0, fonts.1, &w, code, cell.0, cell.1);
                }
            }
            at + codes.len() as i32
        };
        let note = self
            .item(self.hover)
            .and_then(|i| i.note(self.korean))
            .map(cells);
        for (i, r) in self.rows.iter().enumerate() {
            let row = row0 + i as i32;
            match r {
                Row::Head(head) => {
                    put(fb, c.alert, false, 0, row, head);
                }
                Row::Entry(item) => {
                    let label = cells(command_word(item.code, self.korean));
                    if i == self.hover {
                        let w = window(c.text);
                        text::draw_symbol(fb, fonts, &w, MARKER, (col0 as u16, row as u16), None);
                    }
                    let mut next = put(fb, c.text, !item.enabled(), 1, row, &label);
                    if item.theft {
                        next = put(
                            fb,
                            c.alert,
                            false,
                            next + 1,
                            row,
                            &cells(theft_word(self.korean)),
                        );
                    }
                    if let (0, Some(name)) = (i, &self.named) {
                        put(fb, c.alert, false, next + 1, row, &name.0);
                    }
                }
                Row::Note => {
                    put(fb, c.text, false, 0, row, note.as_deref().unwrap_or(&[]));
                }
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

/// The cells the widest of `rows` takes: a heading's name, an entry's mark column, label and
/// theft mark (the first row's also a space and `named`), the note the longest reason of the
/// entries among them.
fn width(rows: &[Row], named: Option<&Cells>, korean: bool) -> i32 {
    let row = |(i, r): (usize, &Row)| match r {
        Row::Head(c) => c.len(),
        Row::Entry(item) => {
            let name = named.filter(|_| i == 0).map_or(0, |n| 1 + n.0.len());
            item.width(korean) + name
        }
        Row::Note => rows
            .iter()
            .filter_map(|r| match r {
                Row::Entry(item) => item.note(korean).map(|n| cells(n).len()),
                _ => None,
            })
            .max()
            .unwrap_or(0),
    };
    rows.iter().enumerate().map(row).max().unwrap_or(1).max(1) as i32
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
    /// The release that ends [`Popup::swallow`] is the menu's too (a press while a chosen command
    /// waits, a double click's second press), not a move given to the game.
    mute: bool,
    /// The keys the menu took, each the menu's (its repeats) until its own release, and whether
    /// it took the key's press too (then the release is the menu's as well; a key pressed for
    /// the game before the menu opened gets its release).
    keys: Vec<(Key, bool)>,
    /// The pointer the menu took last, its buttons left out: the sprite's place while the game's
    /// own sample stays where the game took it.
    pointer: Option<Pointer>,
    /// Moves at every change of what is drawn.
    version: u64,
    /// Frame rects the platform shows something else over ([`Popup::set_hidden`]).
    hidden: Vec<Rect>,
    /// The press that chose an entry: the engine's clock (ms) and its frame pixel, for its
    /// double click ([`DOUBLE_CLICK_MS`], [`DOUBLE_CLICK_PX`]).
    chose: Option<(u64, i16, i16)>,
    /// The map cell "Go here" chose, for the engine ([`Popup::take_go_here`]).
    go_here: Option<(i16, i16)>,
    /// The last [`Popup::input`] took an event that is not the tail of a gesture the menu already
    /// held ([`Popup::took_new_input`]).
    new_input: bool,
}

impl Popup {
    /// On or off. Off closes it, drops a run and forgets what of the input was the menu's (the
    /// pointer it took, a press or keys it held), so turned on again it takes nothing it did not
    /// see.
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
        if !on {
            self.run = None;
            self.close();
            self.pointer = None;
            self.swallow = false;
            self.mute = false;
            self.keys.clear();
            self.buttons = 0;
            self.chose = None;
            self.go_here = None;
        }
    }

    pub fn on(&self) -> bool {
        self.on
    }

    /// The frame rects the platform shows something else over (Wide's page windows over the
    /// map, the parts of the frame off the canvas or the viewport): a menu keeps off them where
    /// it can ([`Open::new`]); one open when they change is laid out again for them (`s`). None
    /// by default.
    pub fn set_hidden(&mut self, rects: Vec<Rect>, s: &State) {
        if rects == self.hidden {
            return;
        }
        self.hidden = rects;
        self.relay(s);
    }

    /// The open menu laid out again for `s` and the hidden rects, drawn again if it moved.
    fn relay(&mut self, s: &State) {
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let again = open.relaid(s, &self.hidden);
        if &again != open {
            self.open = Some(again);
            self.version += 1;
        }
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

    /// The map cell "Go here" chose since the last call, once (unit M4's travel starts there).
    pub fn take_go_here(&mut self) -> Option<(i16, i16)> {
        self.go_here.take()
    }

    /// Whether the last [`Popup::input`] took an event that is not the tail of a gesture the
    /// menu already held: an event while it is open, the press that opens it, a press while a
    /// chosen command waits. The tail of a choice is not: its press's held moves, a double click's
    /// press and release, the repeats and release of a key taken while open (the press's own
    /// release reaches the game as a move with no button). Spec 21's merge contract: a travel
    /// under way ends on it, so the travel Go here starts survives its own gesture.
    pub fn took_new_input(&self) -> bool {
        self.new_input
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
    /// with the presses of a choice in its place. `quiet`: nothing is queued for the game. While
    /// a run was held at the tick's start, every press of its events is the menu's.
    pub fn input(
        &mut self,
        s: &State,
        io: &Io,
        input: &[InputEvent],
        quiet: bool,
    ) -> Vec<InputEvent> {
        let mut tail = Vec::new();
        self.new_input = false;
        let held = self.run.is_some();
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
            if !self.take(s, io, ev, (calm, held), &mut out) {
                calm &= inert(&ev);
                out.push(ev);
            }
        }
        out.extend(tail);
        out
    }

    /// After the game's poll: a menu whose prompt went (A4-0x7a2c no longer 1) closes; one whose
    /// language changed is laid out again.
    pub fn after_poll(&mut self, s: &State) {
        if self.open.is_some() && s.main.reading != 1 {
            self.close();
            self.pointer = None;
        } else if self
            .open
            .as_ref()
            .is_some_and(|o| o.korean != (s.ui.text.language == Language::Korean))
        {
            self.relay(s);
        }
    }

    /// Takes `ev` when it is the menu's (true), else leaves it to the game; the presses of a
    /// choice go to `out`. `calm`: nothing queued for the game, nothing but inert events before;
    /// `held`: a run was held at the tick's start.
    fn take(
        &mut self,
        s: &State,
        io: &Io,
        ev: InputEvent,
        (calm, held): (bool, bool),
        out: &mut Vec<InputEvent>,
    ) -> bool {
        match ev {
            InputEvent::Pointer { x, y, buttons } => {
                self.pointer_event(s, io, (x, y), buttons, (calm, held), out)
            }
            InputEvent::Key(k) | InputEvent::Repeat(k) | InputEvent::Paced(k) => {
                let repeat = matches!(ev, InputEvent::Repeat(_) | InputEvent::Paced(_));
                let taken = self.keys.iter().position(|&(t, _)| t == k);
                if self.open.is_none() {
                    // A press with the menu closed is the game's, and so is the key from then on.
                    if !repeat && let Some(i) = taken {
                        self.keys.remove(i);
                    }
                    return repeat && taken.is_some();
                }
                match taken {
                    Some(i) => self.keys[i].1 |= !repeat,
                    None => self.keys.push((k, !repeat)),
                }
                self.new_input = true;
                self.key_event(s, k, repeat, out);
                true
            }
            InputEvent::Release(k) => {
                let Some(i) = self.keys.iter().position(|&(t, _)| t == k) else {
                    return false;
                };
                let took = self.keys.remove(i).1;
                self.new_input |= took && self.open.is_some();
                took
            }
            InputEvent::Text(_) => {
                self.new_input |= self.open.is_some();
                self.open.is_some()
            }
        }
    }

    fn pointer_event(
        &mut self,
        s: &State,
        io: &Io,
        (x, y): (i16, i16),
        buttons: u8,
        (calm, held): (bool, bool),
        out: &mut Vec<InputEvent>,
    ) -> bool {
        let b = buttons & (POINTER_LEFT | POINTER_RIGHT);
        let before = std::mem::replace(&mut self.buttons, b);
        let down = b & !before;
        let at = Pointer { x, y, buttons: 0 };
        if self.swallow {
            self.swallow = b != 0;
            if !self.swallow && self.open.is_none() {
                if std::mem::take(&mut self.mute) {
                    // A muted press's release: the menu's too; the sprite stays here until the
                    // next event goes to the game.
                    self.pointer = Some(at);
                    return true;
                }
                // The release of a press that closed the menu: the game's pointer comes here, a
                // button-less move.
                self.pointer = None;
                return false;
            }
            self.new_input |= self.open.is_some();
            self.pointer = Some(at);
            self.hover_at(x, y);
            return true;
        }
        if self.open.is_none() {
            if down != 0 && (held || self.run.is_some() || self.double_click(io, x, y)) {
                // A press while a chosen command waits for its target, or the second press of a
                // double click on the entry: the menu's, with its release. The double click is
                // the choice's tail; a press while the command waits is new.
                self.new_input |= !self.double_click(io, x, y);
                self.chose = None;
                self.swallow = true;
                self.mute = true;
                self.pointer = Some(at);
                return true;
            }
            let press = b == POINTER_RIGHT && before == 0;
            if press && calm && self.run.is_none() && s.main.reading == 1 && s.ui.input.accept {
                let open = menu::query(s, io, x, y).map(|m| Open::new(&m, (x, y), s, &self.hidden));
                if let Some(open) = open.filter(|o| !o.entries().is_empty()) {
                    self.open = Some(open);
                    self.new_input = true;
                    self.version += 1;
                    self.swallow = true;
                    self.pointer = Some(at);
                    return true;
                }
            }
            self.pointer = None;
            return false;
        }
        self.new_input = true;
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
                        self.chose = Some((io.now_ms(), x, y));
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

    /// A press at frame pixel (x, y) now is the second press of a double click on the entry
    /// chosen last.
    fn double_click(&self, io: &Io, x: i16, y: i16) -> bool {
        self.chose.is_some_and(|(t, cx, cy)| {
            io.now_ms().saturating_sub(t) <= DOUBLE_CLICK_MS
                && (x - cx).abs() <= DOUBLE_CLICK_PX
                && (y - cy).abs() <= DOUBLE_CLICK_PX
        })
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

    /// Closes an open menu as a press outside it would (the platform's press that reaches no
    /// frame pixel: off the canvas, in a gap or the letterbox); true when one was open.
    pub fn dismiss(&mut self) -> bool {
        let open = self.open.is_some();
        self.close();
        self.pointer = None;
        open
    }

    /// Runs row `i`'s entry (spec 17 Running a choice): Go here asks unit M4's travel to the cell
    /// ([`Popup::take_go_here`]), a slot's own action is a left click on the slot, Rest and Combat
    /// their icon's press, any other command its icon's press now and the press at the target
    /// once it reads it. The menu closes.
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
            menu::GO_HERE => self.go_here = cell,
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

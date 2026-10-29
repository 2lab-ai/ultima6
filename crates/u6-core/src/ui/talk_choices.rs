//! Conversation-local discovered choices (spec 22, owner update 2026-10-29). No save data.

use super::{Button, TalkClicks, Target, draw_cells, trimmed, typed};
use crate::engine::Io;
use crate::state::State;
use crate::ui::text::{CELL, WindowId, fill_rect};
use crate::ui::{Language, UiState};
use u6_render::{Kind, Src};

pub const EXIT: usize = 2;
const PER_PAGE: usize = 8;

#[cfg(test)]
thread_local! {
    /// Paired archive receipt: disable only the port answer presentation, keeping the same VM.
    pub(crate) static ANSWER_UI: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub(crate) fn answers_enabled() -> bool {
    #[cfg(test)]
    {
        ANSWER_UI.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        true
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Topic {
    pub word: Vec<u8>,
    pub label: Vec<u16>,
    pub used: bool,
}

impl std::fmt::Debug for Topic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Topic({} bytes, {} cells, used={})",
            self.word.len(),
            self.label.len(),
            self.used
        )
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Saved {
    rect: (usize, usize, usize, usize),
    pixels: Vec<(u8, Src)>,
}

impl std::fmt::Debug for Saved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Saved({} pixels)", self.pixels.len())
    }
}

impl Saved {
    fn take(io: &Io, rect: (usize, usize, usize, usize)) -> Self {
        let (x, y, w, h) = rect;
        let pixels = io.draw(|fb| {
            (0..w * h)
                .map(|i| {
                    let (x, y) = (x + i % w, y + i / w);
                    (
                        fb.get(x, y).unwrap_or(0),
                        fb.src_at(x, y).unwrap_or(Src::CLASSIC),
                    )
                })
                .collect()
        });
        Self { rect, pixels }
    }
    fn restore(self, io: &Io) {
        let (x, y, w, _) = self.rect;
        io.draw(|fb| {
            for (i, (colour, src)) in self.pixels.into_iter().enumerate() {
                fb.set_src(x + i % w, y + i / w, colour, src);
            }
        });
        io.present();
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choices {
    pub active: bool,
    pub topics: Vec<Topic>,
    pub page: usize,
    fresh: Vec<(u8, u8)>,
    carry: Vec<u16>,
    panel: Option<Saved>,
    answer: Option<Saved>,
    pub yes_no: bool,
    pub arrows: Vec<Button>,
    pub answers: Vec<Button>,
}

fn korean(s: &State) -> bool {
    s.locale
        .pack
        .as_ref()
        .is_some_and(|p| p.language == Language::Korean)
}

fn cells(word: &[u8]) -> Vec<u16> {
    crate::ui::wide::decode(word)
        .into_iter()
        .filter_map(|c| u16::try_from(c).ok())
        .collect()
}

fn canonical(word: &[u8]) -> Vec<u8> {
    word.iter().map(u8::to_ascii_lowercase).collect()
}

pub fn start(s: &mut State) {
    let mut choices = Choices {
        active: true,
        ..Choices::default()
    };
    // Stable IDs retain Job=0 and Name=1 for existing platform instruments. Visual order is
    // Exit, Name, Job; later keywords are appended and never renumbered.
    for keyword in [b"job".as_slice(), b"name".as_slice(), b"bye".as_slice()] {
        let word = s
            .locale
            .pack
            .as_ref()
            .zip(s.converse.script_key)
            .and_then(|(p, key)| p.aliases(key, keyword)?.first().cloned())
            .unwrap_or_else(|| keyword.to_vec());
        let label = if korean(s) {
            cells(match keyword {
                b"job" => "직업".as_bytes(),
                b"name" => "이름".as_bytes(),
                _ => "대화 종료".as_bytes(),
            })
        } else if keyword == b"bye" {
            cells(b"End")
        } else {
            let mut cap = word.clone();
            if let Some(c) = cap.first_mut() {
                *c = c.to_ascii_uppercase();
            }
            cells(&cap)
        };
        choices.topics.push(Topic {
            word,
            label,
            used: false,
        });
    }
    s.ui.choices = choices;
}

pub fn finish(s: &mut State, io: &Io) {
    end(s, io);
    end_answer(s, io);
    s.ui.choices = Choices::default();
    s.ui.talk = TalkClicks::default();
}

/// A newly drawn keyword cell belongs to this conversation. Pre-existing spans from an earlier
/// conversation remain inline targets, but cannot silently populate the new choice list.
pub fn note(ui: &mut UiState, row: u8, col: u8) {
    if !ui.choices.active {
        return;
    }
    if let Some(k) = ui.text.keys.at(row, col) {
        let at = (k.row, k.col);
        if !ui.choices.fresh.contains(&at) {
            ui.choices.fresh.push(at);
        }
    }
}

fn add(ui: &mut UiState, word: Vec<u8>) {
    if word.is_empty() {
        return;
    }
    let key = canonical(&word);
    if ui.choices.topics.iter().any(|t| canonical(&t.word) == key) {
        return;
    }
    ui.choices.topics.push(Topic {
        label: cells(&word),
        word,
        used: false,
    });
}

/// Capture complete visible keywords, including every part of a wrapped keyword. A prefix
/// whose row scrolls out before the rest is printed is carried until that rest is complete.
pub fn discover(ui: &mut UiState, scrolling: bool) {
    if !ui.choices.active {
        return;
    }
    let keys = &ui.text.keys;
    let mut words = Vec::new();
    let mut carry = ui.choices.carry.clone();
    let mut roots = Vec::new();
    for &(row, col) in &ui.choices.fresh {
        let parts = keys.parts(row, col);
        let Some(first) = parts.first() else {
            continue;
        };
        let root = (first.row, first.col);
        if roots.contains(&root) {
            continue;
        }
        roots.push(root);
        if scrolling && first.row != 0 {
            continue;
        }
        let mut word = Vec::new();
        if first.row == 0 && first.col == 0 && first.joined.is_some() {
            word.extend(&carry);
            word.extend(first.joined.iter().flatten());
        }
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                word.extend(part.joined.iter().flatten());
            }
            word.extend(&part.cells);
        }
        if !parts.last().is_some_and(|p| p.continued) {
            words.push(trimmed(typed(&word)));
        }
        if scrolling {
            let top = parts[0];
            if top.continued {
                if top.joined.is_none() {
                    carry.clear();
                } else if top.col == 0 {
                    carry.extend(top.joined.iter().flatten());
                }
                carry.extend(&top.cells);
            } else {
                carry.clear();
            }
        }
    }
    for word in words {
        add(ui, word);
    }
    if scrolling {
        ui.choices.carry = carry;
        ui.choices.fresh = ui
            .choices
            .fresh
            .iter()
            .filter_map(|&(r, c)| r.checked_sub(1).map(|r| (r, c)))
            .collect();
    }
}

fn grid(s: &State) -> (usize, usize, usize, usize) {
    let w = s.ui.text.window(WindowId::Status);
    let last = usize::from(w.y) + usize::from(w.height).saturating_sub(1);
    (
        usize::from(w.x),
        last.saturating_sub(3),
        usize::from(w.width),
        last,
    )
}

fn clear(s: &State, io: &Io, rect: (usize, usize, usize, usize)) {
    let (x, y, w, h) = rect;
    io.draw(|fb| {
        fill_rect(
            fb,
            x as i32,
            y as i32,
            (x + w - 1) as i32,
            (y + h - 1) as i32,
            s.ui.tables.colours.paper,
            s.ui.paper.as_ref(),
        )
    });
}

fn header_rect(s: &State) -> (usize, usize, usize, usize) {
    let (x, row, width, _) = grid(s);
    let y = (usize::from(s.ui.text.window(WindowId::Status).y) + 1) * CELL;
    (x * CELL, y, width * CELL, row * CELL - y)
}

pub fn header(s: &State, io: &Io, preview: Option<usize>) {
    if s.ui.choices.panel.is_none() {
        return;
    }
    let rect = header_rect(s);
    clear(s, io, rect);
    let (x, row, width, _) = grid(s);
    if let Some(topic) = preview.and_then(|i| s.ui.choices.topics.get(i)) {
        // The full keyword replaces the portrait while hovered. Up to the line editor's 50-byte
        // input fits the header's 6 × 17 cells, including ASCII, spaces and Korean syllables.
        for (n, chunk) in topic.label.chunks(width).enumerate() {
            let y = rect.1 / CELL + n;
            if y >= row {
                break;
            }
            draw_cells(
                s,
                io,
                chunk,
                (x as u16, y as u16),
                (s.ui.tables.colours.text, s.ui.tables.colours.paper),
            );
        }
        return;
    }
    if let Some((letter, item)) = crate::converse::portrait_ref(s, s.converse.npc)
        && let Some(pixels) = crate::converse::portrait_pixels(s, io.files(), letter, item)
    {
        let (pw, ph) = (
            42usize.min(rect.2),
            48usize.min(rect.3.saturating_sub(CELL)),
        );
        let x0 = rect.0 + (rect.2 - pw) / 2;
        let colours = &s.gfx.tables.bitmap_colours;
        let id = u6_render::portrait_id(letter, item);
        io.draw(|fb| {
            for y in 0..ph {
                for x in 0..pw {
                    let (u, v) = (x * 56 / pw, y * 64 / ph);
                    let colour = colours
                        .get(usize::from(pixels[v * 56 + u]))
                        .copied()
                        .unwrap_or(0);
                    let src = id.map_or(Src::CLASSIC, |id| Src {
                        kind: Kind::Portrait,
                        id,
                        u: u as u16,
                        v: v as u16,
                        ..Src::CLASSIC
                    });
                    fb.set_src(x0 + x, rect.1 + y, colour, src);
                }
            }
        });
    }
    let label: Vec<u16> = cells(&s.converse.npc_name)
        .into_iter()
        .take(width)
        .collect();
    draw_cells(
        s,
        io,
        &label,
        ((x + (width - label.len()) / 2) as u16, (row - 1) as u16),
        (s.ui.tables.colours.text, s.ui.tables.colours.paper),
    );
}

fn short(label: &[u16], width: usize) -> Vec<u16> {
    let mut out: Vec<_> = label.iter().copied().take(width).collect();
    if label.len() > width && width >= 3 {
        out[width - 3..].fill(u16::from(b'.'));
    }
    out.resize(width, u16::from(b' '));
    out
}

pub fn layout(s: &mut State, io: &Io) {
    let (x, row, width, last) = grid(s);
    let cell = width.saturating_sub(2) / 3;
    s.ui.talk.buttons.clear();
    s.ui.choices.arrows.clear();
    if cell == 0 || row <= usize::from(s.ui.text.window(WindowId::Status).y) + 1 {
        return;
    }
    clear(
        s,
        io,
        (x * CELL, row * CELL, width * CELL, (last - row + 1) * CELL),
    );
    let mut order: Vec<usize> = (0..s.ui.choices.topics.len())
        .filter(|&i| i != EXIT)
        .collect();
    order.sort_by_key(|&i| {
        (
            s.ui.choices.topics[i].used,
            if i == 1 {
                0
            } else if i == 0 {
                1
            } else {
                i
            },
        )
    });
    let pages = order.len().div_ceil(PER_PAGE).max(1);
    s.ui.choices.page = s.ui.choices.page.min(pages - 1);
    let ids = std::iter::once(EXIT).chain(
        order
            .into_iter()
            .skip(s.ui.choices.page * PER_PAGE)
            .take(PER_PAGE),
    );
    for (slot, id) in ids.enumerate() {
        let topic = &s.ui.choices.topics[id];
        s.ui.talk.buttons.push(Button {
            id,
            row: (row + slot / 3) as u8,
            col: (x + (slot % 3) * (cell + 1)) as u8,
            cells: short(&topic.label, cell),
            word: topic.word.clone(),
            used: topic.used,
        });
    }
    // Keep the stable-ID order for callers that inspect Job/Name, independent of visual sorting.
    s.ui.talk.buttons.sort_by_key(|b| b.id);
    if pages > 1 {
        for (next, col, label) in [(false, x, b'<'), (true, x + width - 1, b'>')] {
            s.ui.choices.arrows.push(Button {
                id: usize::from(next),
                row: last as u8,
                col: col as u8,
                cells: vec![u16::from(label)],
                word: Vec::new(),
                used: false,
            });
        }
    }
    for b in &s.ui.talk.buttons {
        super::draw_target(s, io, Target::Button(b.id), false);
    }
    for b in &s.ui.choices.arrows {
        super::draw_target(s, io, Target::Page(b.id != 0), false);
    }
}

pub fn begin(s: &mut State, io: &Io) {
    if !s.ui.choices.active {
        start(s);
    }
    discover(&mut s.ui, false);
    let (x, row, width, last) = grid(s);
    if width < 5 || row <= usize::from(s.ui.text.window(WindowId::Status).y) + 1 {
        return;
    }
    let top = header_rect(s).1;
    s.ui.choices.panel = Some(Saved::take(
        io,
        (x * CELL, top, width * CELL, (last + 1) * CELL - top),
    ));
    header(s, io, None);
    layout(s, io);
}

pub fn end(s: &mut State, io: &Io) {
    if let Some(saved) = s.ui.choices.panel.take() {
        saved.restore(io);
    }
    s.ui.choices.arrows.clear();
}

pub fn used(s: &mut State, word: &[u8]) {
    let key = canonical(word);
    for (i, topic) in s.ui.choices.topics.iter_mut().enumerate() {
        if i != EXIT && canonical(&topic.word) == key {
            topic.used = true;
        }
    }
    s.ui.choices.page = 0;
}

pub fn page(s: &mut State, io: &Io, next: bool) {
    let pages =
        s.ui.choices
            .topics
            .len()
            .saturating_sub(1)
            .div_ceil(PER_PAGE)
            .max(1);
    s.ui.choices.page = if next {
        (s.ui.choices.page + 1) % pages
    } else {
        (s.ui.choices.page + pages - 1) % pages
    };
    s.ui.talk.hot = None;
    s.ui.talk.pressed = None;
    header(s, io, None);
    layout(s, io);
}

pub fn begin_answer(s: &mut State, io: &Io) {
    if !s.ui.choices.yes_no || s.ui.choices.answer.is_some() {
        return;
    }
    let w = *s.ui.text.window(WindowId::Messages);
    let row = w.y + w.height.saturating_sub(1);
    let labels: [&[u8]; 2] = if korean(s) {
        ["예".as_bytes(), "아니오".as_bytes()]
    } else {
        [b"Yes", b"No"]
    };
    let words = labels.map(cells);
    let total = words[0].len() + words[1].len() + 3;
    if total > usize::from(w.width) || s.ui.font.is_none() {
        return;
    }
    s.ui.choices.answer = Some(Saved::take(
        io,
        (
            usize::from(w.x) * CELL,
            usize::from(row) * CELL,
            usize::from(w.width) * CELL,
            CELL,
        ),
    ));
    let mut col = usize::from(w.x) + usize::from(w.width) - total;
    draw_cells(
        s,
        io,
        &[u16::from(b' '), u16::from(b'/'), u16::from(b' ')],
        ((col + words[0].len()) as u16, u16::from(row)),
        (s.ui.tables.colours.text, w.bg),
    );
    s.ui.choices.answers.clear();
    for (id, label) in words.into_iter().enumerate() {
        let len = label.len();
        s.ui.choices.answers.push(Button {
            id,
            row,
            col: col as u8,
            cells: label,
            word: vec![if id == 0 { b'y' } else { b'n' }],
            used: false,
        });
        col += len + 3;
    }
    s.ui.talk = TalkClicks {
        reading: true,
        ..TalkClicks::default()
    };
    for yes in [true, false] {
        super::draw_target(s, io, Target::Answer(yes), false);
    }
}

pub fn end_answer(s: &mut State, io: &Io) {
    s.ui.choices.yes_no = false;
    if let Some(saved) = s.ui.choices.answer.take() {
        saved.restore(io);
    }
    s.ui.choices.answers.clear();
    s.ui.talk = TalkClicks::default();
}

pub fn answering(s: &State) -> bool {
    s.ui.choices.answer.is_some()
}

pub fn grey(io: &Io, paper: u8) -> u8 {
    io.draw(|fb| {
        fb.palette
            .iter()
            .take(16) // draw_char clears plane 4; higher indices would wrap.
            .enumerate()
            .filter(|(i, _)| *i != usize::from(paper))
            .min_by_key(|(_, c)| {
                let high = c.r.max(c.g).max(c.b);
                let low = c.r.min(c.g).min(c.b);
                4 * u16::from(high - low)
                    + (u16::from(c.r) + u16::from(c.g) + u16::from(c.b)).abs_diff(384)
            })
            .map_or(0, |(i, _)| i as u8)
    })
}

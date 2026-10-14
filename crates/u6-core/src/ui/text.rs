//! Text windows and the character-cell drawing under them: the window record 0x107d7c fills,
//! window select / cursor (0x107dd8, 0x107e00), the glyph renderer 0x153490 behind the
//! draw-character pointer A4-0x7a24 (0x105228) and the scroll behind A4-0x7a20 (0x10525c).
//!
//! The screen is 40 × 25 cells of 8 × 8 pixels (the window main sets up first, 0x13533a). The
//! port draws into the engine's chunky back buffer ([`crate::Io::draw`]) where the original
//! writes the bitplanes of the screen at 0x161bde.

use u6_formats::font::BitFont;
use u6_render::{Framebuffer, HEIGHT, WIDTH};

/// Window flag (byte 8) bit: wrap at word boundaries.
pub const WRAP: u8 = 0x01;
/// Window flag: pad the start of each line with spaces (0x108050).
pub const CENTRE: u8 = 0x02;
/// Window flag: the character after a back-quote is capitalised.
pub const CAPITALISE: u8 = 0x04;
/// Window flag: characters above space use the runic glyphs (code + 0x80).
pub const RUNIC: u8 = 0x08;
/// Window flag: wait for a key when the window has filled (more prompt).
pub const PAGED: u8 = 0x10;
/// Window flag: scroll up instead of wrapping to the top row.
pub const SCROLL: u8 = 0x20;

/// Cell size in pixels.
pub const CELL: usize = 8;

/// A text window: the 9-byte record 0x107d7c fills (offsets in brackets).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextWindow {
    /// [0] first column
    pub x: u8,
    /// [1] first row
    pub y: u8,
    /// [2] columns
    pub width: u8,
    /// [3] rows
    pub height: u8,
    /// [4] cursor column (window-relative)
    pub col: u8,
    /// [5] cursor row (window-relative)
    pub row: u8,
    /// [6] ink colour
    pub fg: u8,
    /// [7] paper colour
    pub bg: u8,
    /// [8] flags ([`WRAP`], [`CENTRE`], [`CAPITALISE`], [`RUNIC`], [`PAGED`], [`SCROLL`])
    pub flags: u8,
}

impl TextWindow {
    /// The record as stored in memory (9 bytes).
    pub fn from_bytes(b: &[u8]) -> Self {
        let at = |i: usize| b.get(i).copied().unwrap_or(0);
        TextWindow {
            x: at(0),
            y: at(1),
            width: at(2),
            height: at(3),
            col: at(4),
            row: at(5),
            fg: at(6),
            bg: at(7),
            flags: at(8),
        }
    }

    /// A window over the inclusive corner cells, cursor at the top left (the record
    /// 0x107d7c builds; see `TextState::open`).
    pub fn new(x0: u8, y0: u8, x1: u8, y1: u8, fg: u8, bg: u8, flags: u8) -> Self {
        TextWindow {
            x: x0,
            y: y0,
            width: x1.wrapping_sub(x0).wrapping_add(1),
            height: y1.wrapping_sub(y0).wrapping_add(1),
            col: 0,
            row: 0,
            fg,
            bg,
            flags,
        }
    }
}

/// The windows the game uses (the record each lives in).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WindowId {
    /// A4-0x7a1a: initialised data, current until main selects its own windows.
    #[default]
    Default,
    /// A4-0x2f2e: the whole screen, 40 × 25 (main 0x13533a).
    Screen,
    /// A4-0x2f25: the side panel, columns 22..=38, rows 1..=12 (main 0x135352).
    Status,
    /// A4-0x2f13: the message scroll, columns 22..=38, rows 14..=23 (main 0x135368), selected
    /// by main with [`WRAP`] | [`SCROLL`] | [`CAPITALISE`] (0x13538a..0x1353b6).
    Messages,
}

impl WindowId {
    pub const ALL: [WindowId; 4] = [
        WindowId::Default,
        WindowId::Screen,
        WindowId::Status,
        WindowId::Messages,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Original: 0x153490 (graphics: draw one 8 × 8 glyph of `u6.ch`: planes 0-3 from ink / paper, plane 4
/// cleared)
///
/// `x` is rounded down to a byte (8-pixel) boundary as the planar code does; set pixels get
/// `fg`, clear pixels `bg`, both limited to colours 0..15 because plane 4 is cleared.
pub fn draw_glyph(fb: &mut Framebuffer, font: &BitFont, x: usize, y: usize, c: u8, fg: u8, bg: u8) {
    let x = x & !7;
    let glyph = &font.glyphs[usize::from(c)];
    for (dy, &row) in glyph.iter().enumerate() {
        for dx in 0..CELL {
            let set = row & (0x80 >> dx) != 0;
            fb.set(x + dx, y + dy, if set { fg & 0x0f } else { bg & 0x0f });
        }
    }
}

/// Original: 0x105228 (ui: draw character `c` at screen cell (`col`, `row`) in the current window's
/// colours; the target of the pointer A4-0x7a24)
pub fn draw_char(
    fb: &mut Framebuffer,
    font: Option<&BitFont>,
    w: &TextWindow,
    c: u8,
    col: u16,
    row: u16,
) {
    if let Some(font) = font {
        let (x, y) = (usize::from(col) * CELL, usize::from(row) * CELL);
        draw_glyph(fb, font, x, y, c, w.fg, w.bg);
    }
}

/// Fills the inclusive pixel rectangle with `colour`: 0x152176 selects the colour, 0x1521f4
/// fills (on the screen, set mode; [`crate::gfx::blit::fill_rect`]).
pub fn fill_rect(fb: &mut Framebuffer, x0: i32, y0: i32, x1: i32, y1: i32, colour: u8) {
    let w = |v: i32| v as u16;
    crate::gfx::blit::fill_colour(fb, w(x0), w(y0), w(x1), w(y1), colour);
}

/// Original: 0x1522d6 (graphics: move a byte-aligned screen rectangle up by `-dy` pixel rows)
///
/// Columns are whole bytes (`x >> 3`); the corners are swapped into order. Only a negative
/// `dy` moves anything: the positive branch adjusts its pointers and returns without copying,
/// and so does this port.
pub fn scroll_rect(fb: &mut Framebuffer, x0: u16, y0: u16, x1: u16, y1: u16, dy: i16) {
    let (mut c0, mut c1) = (x0 >> 3, x1 >> 3);
    let (mut r0, mut r1) = (y0, y1);
    if c0 > c1 {
        std::mem::swap(&mut c0, &mut c1);
    }
    if r0 > r1 {
        std::mem::swap(&mut r0, &mut r1);
    }
    if dy >= 0 {
        return;
    }
    let shift = dy.unsigned_abs();
    let rows = (r1 - r0 + 1).saturating_sub(shift);
    let (px0, px1) = (usize::from(c0) * 8, (usize::from(c1) + 1) * 8);
    for r in 0..rows {
        let (dst, src) = (usize::from(r0 + r), usize::from(r0 + r + shift));
        if dst >= HEIGHT || src >= HEIGHT {
            continue;
        }
        for x in px0..px1.min(WIDTH) {
            let p = fb.get(x, src).unwrap_or(0);
            fb.set(x, dst, p);
        }
    }
}

/// Original: 0x10525c (ui: scroll a window's cells up one row and clear the last row to its paper
/// colour; the target of the pointer A4-0x7a20)
pub fn scroll_window(fb: &mut Framebuffer, w: &TextWindow) {
    let x = u16::from(w.x) * 8;
    let y = u16::from(w.y) * 8;
    let wp = u16::from(w.width) * 8;
    let hp = u16::from(w.height) * 8;
    let (x1, y1) = (x + wp - 1, y + hp - 1);
    scroll_rect(fb, x, y, x1, y1, -8);
    fill_rect(
        fb,
        i32::from(x),
        i32::from(y + hp - 8),
        i32::from(x1),
        i32::from(y1),
        w.bg,
    );
}

/// Original: 0x1082b0 (ui: number to text: up to `max` digits in `base`, signed when `base` < 0)
///
/// Digits come out least significant first, so a number longer than `max` keeps its low digits;
/// the sign costs one of the `max`. Zero prints as "0". Hex digits are upper case.
pub fn itoa(value: u16, max: i16, base: i16) -> Vec<u8> {
    let (mut v, mut max, mut base) = (value, max, base);
    let mut neg = false;
    if base < 0 {
        base = -base;
        if v > 0x7fff {
            v = v.wrapping_neg();
            neg = true;
            max -= 1;
        }
    }
    let mut digits = Vec::new();
    while base != 0 && v != 0 && (digits.len() as i16) < max {
        let d = (v % base as u16) as u8;
        digits.push(if d < 10 { b'0' + d } else { d + 0x37 });
        v /= base as u16;
    }
    if digits.is_empty() {
        digits.push(b'0');
    }
    if neg {
        digits.push(b'-');
    }
    digits.reverse();
    digits
}

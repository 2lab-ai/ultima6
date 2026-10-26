//! Text windows and the character-cell drawing under them: the window record 0x107d7c fills,
//! window select / cursor (0x107dd8, 0x107e00), the glyph renderer 0x153490 behind the
//! draw-character pointer A4-0x7a24 (0x105228) and the scroll behind A4-0x7a20 (0x10525c).
//!
//! The screen is 40 × 25 cells of 8 × 8 pixels (the window main sets up first, 0x13533a). The
//! port draws into the engine's chunky back buffer ([`crate::Io::draw`]) where the original
//! writes the bitplanes of the screen at 0x161bde.

use std::fmt;
use std::rc::Rc;

use u6_formats::bitmap::Bitmap;
use u6_formats::font::{BitFont, HangulFont};
use u6_render::{
    BITMAP_PAPER, FLAG_INK, FLAG_PAPER, FLAG_PAPER_OFF, FLAG_SYMBOL, FLAG_TYPED, Framebuffer,
    HEIGHT, Kind, Src, WIDTH,
};

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
///
/// Port (spec 11): each pixel's source names the glyph ([`draw_rows`]).
pub fn draw_glyph(fb: &mut Framebuffer, font: &BitFont, x: usize, y: usize, c: u8, fg: u8, bg: u8) {
    let code = Some(u16::from(c));
    draw_rows(fb, x, y, &font.glyphs[usize::from(c)], code, (fg, bg), 0);
}

/// The pixels of one 8 × 8 cell from its row bytes (bit 7 = leftmost), as [`draw_glyph`] draws
/// them: `x` rounded down to a byte, set pixels `fg`, clear ones `bg`, colours 0..15.
///
/// Port (spec 11): with a `code`, each pixel's source is the glyph of that code ([`Kind::Glyph`],
/// (u, v) its place in the cell, [`FLAG_INK`] on the set pixels, and `flags` on every pixel), so
/// the HD font mode can show the cell's character in an HD font; without one the pixels are
/// classic. The colours are those of either. Port (spec 07 HD0): a glyph's pixel drawn over the
/// paper ([`Src::on_paper`]: the pixel it replaces names the paper or is a glyph's over it)
/// carries [`FLAG_PAPER`], so the HD paper can show behind the text there and nowhere else.
fn draw_rows(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    rows: &[u8; 8],
    code: Option<u16>,
    (fg, bg): (u8, u8),
    flags: u8,
) {
    let x = x & !7;
    for (dy, &row) in rows.iter().enumerate() {
        for dx in 0..CELL {
            let set = row & (0x80 >> dx) != 0;
            let colour = if set { fg & 0x0f } else { bg & 0x0f };
            match code {
                Some(id) => {
                    let under = fb.src_at(x + dx, y + dy);
                    let on_paper = under.is_some_and(|s| s.on_paper());
                    let src = Src {
                        ver: 0,
                        id,
                        u: dx as u16,
                        v: dy as u16,
                        kind: Kind::Glyph,
                        flags: if set { FLAG_INK } else { 0 }
                            | if on_paper { FLAG_PAPER } else { 0 }
                            | flags,
                    };
                    fb.set_src(x + dx, y + dy, colour, src);
                }
                None => fb.set(x + dx, y + dy, colour),
            }
        }
    }
}

/// Original: 0x105228 (ui: draw character `c` at screen cell (`col`, `row`) in the current window's
/// colours; the target of the pointer A4-0x7a24)
///
/// Port (spec 08): `c` is a cell code ([`super::wide`]). A code below 0x100 is the glyph of
/// `u6.ch` as before; a Hangul syllable is its glyph of `hangul`; any other code, and a syllable
/// with no table, is a blank cell (the paper colour only). Without `u6.ch` nothing is drawn.
///
/// Port (spec 11): a glyph's pixels name it (code `c`, [`draw_rows`]); a blank cell's are
/// classic.
pub fn draw_char(
    fb: &mut Framebuffer,
    font: Option<&BitFont>,
    hangul: Option<&HangulFont>,
    w: &TextWindow,
    c: u16,
    col: u16,
    row: u16,
) {
    draw_cell(fb, (font, hangul), w, c, (col, row), Some(0));
}

/// Port-only (spec 11 Design 3, rev 3): [`draw_char`] for the line editor's echo of a character
/// the player typed (`keyread::echo`): each pixel of a glyph drawn also carries [`FLAG_TYPED`], so
/// the HD font mode can tell a space typed from the game's own (a run of text goes on across
/// the first, not across two of the second). The colours are [`draw_char`]'s.
pub fn draw_typed(
    fb: &mut Framebuffer,
    fonts: (Option<&BitFont>, Option<&HangulFont>),
    w: &TextWindow,
    c: u16,
    (col, row): (u16, u16),
) {
    draw_cell(fb, fonts, w, c, (col, row), Some(FLAG_TYPED));
}

/// Port-only (spec 11): [`draw_char`] for a glyph the game draws as a symbol, not a letter (the
/// key wait's cursor, the more prompt's mark): the same pixels, none of them named the glyph as a
/// character, so the HD font mode never shows it as a character of the HD font, whatever its
/// code.
///
/// Port (spec 07 HD0): with `paper` ([`Paper`]), each pixel drawn over the paper
/// ([`Src::on_paper`]) names the paper by the rule of the fills ([`Paper::over`]): the paper
/// where the pixel's colour is the paper's there, else the paper flagged [`FLAG_PAPER_OFF`]; the
/// others, and all of them without `paper` or without a source plane, are classic.
///
/// Port (spec 11 Design 2, rev 3): over the paper, a glyph of the game's font names itself as a
/// symbol on its ink pixels instead: [`Kind::Glyph`] of its code, (u, v) the pixel's place in the
/// cell, [`FLAG_INK`], [`FLAG_PAPER`] and [`FLAG_SYMBOL`]. Those are the pixels the paper's rule
/// flags (or leaves classic, where the paper drew nothing): shown classic either way, so the
/// frame with the HD font off is the same; an ink pixel of the paper's own colour there keeps
/// naming the paper, as do the clear pixels. The HD font mode lists the cell as a symbol, and the
/// platform draws it after the text it follows. Not over the paper, or without `paper`, nothing
/// is named a symbol.
pub fn draw_symbol(
    fb: &mut Framebuffer,
    fonts: (Option<&BitFont>, Option<&HangulFont>),
    w: &TextWindow,
    c: u16,
    (col, row): (u16, u16),
    paper: Option<&Paper>,
) {
    let (x0, y0) = (usize::from(col) * CELL, usize::from(row) * CELL);
    let at = |k: usize| (x0 + k % CELL, y0 + k / CELL);
    let under: [bool; CELL * CELL] = std::array::from_fn(|k| {
        let (x, y) = at(k);
        fb.src_at(x, y).is_some_and(|s| s.on_paper())
    });
    draw_cell(fb, fonts, w, c, (col, row), None);
    let Some(p) = paper.filter(|_| fonts.0.is_some()) else {
        return;
    };
    // The symbol's rows: a glyph of the game's font (the codes the game draws as symbols).
    let rows = u8::try_from(c)
        .ok()
        .and_then(|b| fonts.0.map(|f| f.glyphs[usize::from(b)]));
    for (k, _) in under.iter().enumerate().filter(|(_, on)| **on) {
        let (x, y) = at(k);
        if let Some(colour) = fb.get(x, y) {
            let named = p.over(x, y, colour);
            let ink = rows.is_some_and(|r| r[k / CELL] & (0x80 >> (k % CELL)) != 0);
            let src = if ink && named != Paper::src(x, y) {
                Src {
                    ver: 0,
                    id: c,
                    u: (k % CELL) as u16,
                    v: (k / CELL) as u16,
                    kind: Kind::Glyph,
                    flags: FLAG_INK | FLAG_PAPER | FLAG_SYMBOL,
                }
            } else {
                named
            };
            fb.set_src(x, y, colour, src);
        }
    }
}

/// [`draw_char`] (`named`: its glyph's pixels name it, with these flags; [`draw_typed`]) and
/// [`draw_symbol`] (`None`).
fn draw_cell(
    fb: &mut Framebuffer,
    (font, hangul): (Option<&BitFont>, Option<&HangulFont>),
    w: &TextWindow,
    c: u16,
    (col, row): (u16, u16),
    named: Option<u8>,
) {
    let Some(font) = font else { return };
    let (x, y) = (usize::from(col) * CELL, usize::from(row) * CELL);
    let rows = match u8::try_from(c) {
        Ok(b) => Some(&font.glyphs[usize::from(b)]),
        Err(_) => hangul.and_then(|h| h.glyph(u32::from(c))),
    };
    let code = (named.is_some() && rows.is_some()).then_some(c);
    let flags = named.unwrap_or(0);
    draw_rows(fb, x, y, rows.unwrap_or(&[0; 8]), code, (w.fg, w.bg), flags);
}

/// Fills the inclusive pixel rectangle with `colour`: 0x152176 selects the colour, 0x1521f4
/// fills (on the screen, set mode; [`crate::gfx::blit::fill_rect`]).
///
/// Port (spec 07 HD0): with `paper` ([`Paper`]) each pixel the fill writes names the paper by
/// [`Paper::over`]: the paper where the colour now in the frame is the paper's there, so the HD
/// paper shows wherever the classic frame shows the paper's own colour, else the paper flagged
/// [`FLAG_PAPER_OFF`] (shown classic). Without it, or without a source plane, every pixel filled
/// is classic; a colour past 15 writes nothing and names nothing. The text windows' clears and
/// the UI's (the panels, the command underline) fill through here; the colours are those of the
/// fill either way.
pub fn fill_rect(
    fb: &mut Framebuffer,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    colour: u8,
    paper: Option<&Paper>,
) {
    let w = |v: i32| v as u16;
    crate::gfx::blit::fill_colour(fb, w(x0), w(y0), w(x1), w(y1), colour);
    // The fill writes no colour past 15 (`plot_in`): then no pixel is the fill's to name.
    if colour <= 15 {
        name_paper(fb, (w(x0), w(y0), w(x1), w(y1)), paper);
    }
}

/// Port-only (spec 07 HD0): the paper's colour at each screen pixel, as
/// [`crate::main_loop::draw_paper`] drew it at the boot (the bitmap at (0, 0) through the bitmap
/// colour table), none where it drew nothing. With it the scroll of a text window, the fills of the
/// UI and the key wait's symbol name the paper wherever a pixel they write has the paper's own
/// colour there ([`scroll_window`], [`fill_rect`], [`draw_symbol`]), so the HD paper shows in
/// place behind the text and the panels; a pixel they write on the paper in another colour names
/// it flagged [`FLAG_PAPER_OFF`] (shown classic), so a later scroll can name it again. Shared, so
/// a copy of the state does not copy it; shown by its size only (it is the user's art).
#[derive(Clone, PartialEq, Eq)]
pub struct Paper(Rc<[u8]>);

impl Paper {
    /// A pixel the paper did not draw (the paper's colours are below 32).
    const NONE: u8 = 0xff;

    /// The colours `bmp` puts on the screen drawn at (0, 0) through `colours`, as
    /// [`crate::gfx::blit::blit_bmp`] draws them.
    pub fn drawn(colours: &[u8; 256], bmp: &Bitmap) -> Paper {
        let mut fb = Framebuffer::new();
        fb.clear(Self::NONE);
        crate::gfx::blit::blit_bmp(colours, &mut fb, bmp, 0, 0, BITMAP_PAPER);
        Paper(Rc::from(fb.pixels()))
    }

    /// The paper's colour at (x, y); `None` off the screen and where it drew nothing.
    pub fn at(&self, x: usize, y: usize) -> Option<u8> {
        let c = (x < WIDTH && y < HEIGHT).then(|| self.0[y * WIDTH + x]);
        c.filter(|&c| c != Self::NONE)
    }

    /// The source of the paper's pixel at screen (x, y) (it is drawn at (0, 0)).
    pub fn src(x: usize, y: usize) -> Src {
        Src {
            ver: 0,
            id: BITMAP_PAPER,
            u: x as u16,
            v: y as u16,
            kind: Kind::Bitmap,
            flags: 0,
        }
    }

    /// The source of a pixel of colour `colour` written at (x, y) over the paper: the paper's
    /// there when that is its colour, the paper's flagged [`FLAG_PAPER_OFF`] when it is another,
    /// classic where the paper drew nothing.
    pub fn over(&self, x: usize, y: usize, colour: u8) -> Src {
        match self.at(x, y) {
            Some(c) if c == colour => Self::src(x, y),
            Some(_) => Src {
                flags: FLAG_PAPER_OFF,
                ..Self::src(x, y)
            },
            None => Src::CLASSIC,
        }
    }
}

impl fmt::Debug for Paper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Paper({} pixels)", self.0.len())
    }
}

/// Original: 0x1522d6 (graphics: move a byte-aligned screen rectangle up by `-dy` pixel rows)
///
/// Columns are whole bytes (`x >> 3`); the corners are swapped into order. Only a negative
/// `dy` moves anything: the positive branch adjusts its pointers and returns without copying,
/// and so does this port. The pixels moved keep their sources (spec 07 HD0: a panel bitmap under
/// the text stays named; spec 11: a glyph stays the glyph it was), but the paper's when `paper`
/// is given: the paper is the background, drawn at (0, 0) under the whole screen, and does not
/// move with the text. A paper pixel moved (flagged [`FLAG_PAPER_OFF`] or not) names the paper
/// at the place it moves to by [`Paper::over`]: unflagged when its colour is the paper's there,
/// so the HD paper shows that place, not the one the pixel came from; flagged when it is another
/// (a pixel of the paper's edge moved into its plain middle, shown classic), so that it is named
/// again where it later meets the paper of its colour. The colours move as they did, so the
/// classic frame is unchanged.
pub fn scroll_rect(
    fb: &mut Framebuffer,
    x0: u16,
    y0: u16,
    x1: u16,
    y1: u16,
    dy: i16,
    paper: Option<&Paper>,
) {
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
    let is_paper = |s: &Src| s.kind == Kind::Bitmap && s.id == BITMAP_PAPER;
    for r in 0..rows {
        let (dst, src) = (usize::from(r0 + r), usize::from(r0 + r + shift));
        if dst >= HEIGHT || src >= HEIGHT {
            continue;
        }
        for x in px0..px1.min(WIDTH) {
            let (from, to) = (src * WIDTH + x, dst * WIDTH + x);
            match paper.filter(|_| fb.src_at(x, src).is_some_and(|s| is_paper(&s))) {
                Some(p) => {
                    let colour = fb.pixels()[from];
                    fb.set_src(x, dst, colour, p.over(x, dst, colour));
                }
                None => fb.copy_px_within(from, to),
            }
        }
    }
}

/// Original: 0x10525c (ui: scroll a window's cells up one row and clear the last row to its paper
/// colour; the target of the pointer A4-0x7a20)
///
/// Port (spec 07 HD0): with `paper` ([`Paper`]) the scroll leaves the paper in place
/// ([`scroll_rect`]) and each pixel of the cleared row names the paper ([`fill_rect`]); without
/// it, or without a source plane, the pixels move with their sources and the row is classic.
pub fn scroll_window(fb: &mut Framebuffer, w: &TextWindow, paper: Option<&Paper>) {
    let x = u16::from(w.x) * 8;
    let y = u16::from(w.y) * 8;
    let wp = u16::from(w.width) * 8;
    let hp = u16::from(w.height) * 8;
    let (x1, y1) = (x + wp - 1, y + hp - 1);
    scroll_rect(fb, x, y, x1, y1, -8, paper);
    fill_rect(
        fb,
        i32::from(x),
        i32::from(y + hp - 8),
        i32::from(x1),
        i32::from(y1),
        w.bg,
        paper,
    );
}

/// Port-only (spec 07 HD0): after a fill that wrote every pixel of the inclusive rectangle (its
/// corners put in order and held to the screen as [`crate::gfx::blit::fill_colour`] does), each
/// of them names the paper by [`Paper::over`] with the colour now in the frame. Nothing without
/// `paper` or without a source plane.
pub(crate) fn name_paper(
    fb: &mut Framebuffer,
    (x0, y0, x1, y1): (u16, u16, u16, u16),
    paper: Option<&Paper>,
) {
    let Some(p) = paper.filter(|_| fb.src().is_some()) else {
        return;
    };
    let (xa, xb) = (
        usize::from(x0.min(x1)),
        usize::from(x0.max(x1)).min(WIDTH - 1),
    );
    let (ya, yb) = (
        usize::from(y0.min(y1)),
        usize::from(y0.max(y1)).min(HEIGHT - 1),
    );
    for y in ya..=yb {
        for x in xa..=xb {
            let colour = fb.pixels()[y * WIDTH + x];
            fb.set_src(x, y, colour, p.over(x, y, colour));
        }
    }
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

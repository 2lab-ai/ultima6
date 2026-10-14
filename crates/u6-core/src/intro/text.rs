//! Text of `u`: the proportional font `u6.set`, the text context, glyph drawing, `printf` and
//! the word-wrapping flush (hunks 15 and 19).
//!
//! A text context (main's A4+0x176c, 24 bytes; the current one is A4+0x2d40) holds the view it
//! draws into, the cursor, the font, two colours, a 0x400-byte text buffer with its write
//! pointer, and an alignment. `printf` either draws each character at once (0x1142a6) or
//! appends it to the buffer (0x1142c6 restarts the buffer, 0x1142f0 continues it); `%P` flushes
//! the buffer through the word wrapper 0x115a92, which lays it out left, right, centred or
//! justified. Glyph pixels are one byte each; the font's two key values (ink, background) map
//! to the context's colours, every other value is a colour of its own.

use u6_formats::font::WouFont;

use super::mem::{Chip, View};

/// Original: A4+0x176c (a text context; 0x10f864 makes one current)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextCtx {
    /// +0: the view drawn into (the original keeps a pointer; callers set the fields first)
    pub view: View,
    /// +4: cursor x
    pub x: i16,
    /// +6: cursor y
    pub y: i16,
    /// +0xc: colour of the font's ink pixels
    pub fg: u8,
    /// +0xd: colour of the font's background pixels
    pub bg: u8,
    /// +0xe .. +0x12: the text buffer (the write pointer is its length)
    pub buf: Vec<u8>,
    /// +0x16: 0 left, 1 right, 2 centred, 3 justified (0x115a92)
    pub align: u8,
}

impl Default for TextCtx {
    fn default() -> Self {
        Self {
            view: View::default(),
            x: 0,
            y: 0,
            fg: 0xf,
            bg: 0,
            buf: Vec::new(),
            align: 3,
        }
    }
}

/// One `printf` argument as it sits on the stack (words for `%c %d %u %x %B %F %X %Y`, longs for
/// `%ld %lu`, a string pointer for `%s`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    Word(u16),
    Long(u32),
    Str(Vec<u8>),
}

/// Where `printf` sends characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Out {
    /// 0x115498: draw at the cursor.
    Screen,
    /// 0x10f876: append to the context's buffer.
    Buffer,
}

/// The 5-bit colour a glyph pixel value draws (0x1158ec's 16 plane routines 0x116100: bit 1
/// of the value also sets plane 4).
pub const fn glyph_colour(v: u8) -> u8 {
    (v & 0xf) | ((v & 2) << 3)
}

/// Glyph height (decoded `u6.set` bytes 0..2, little-endian).
fn height(font: &WouFont) -> i16 {
    i16::from_le_bytes([font.raw[0], font.raw[1]])
}

fn width(font: &WouFont, c: u8) -> i16 {
    i16::from(font.raw[4 + usize::from(c)])
}

/// Original: u 0x115544 (text: carriage return, cursor to the view's left edge)
pub fn carriage_return(ctx: &mut TextCtx) {
    ctx.x = ctx.view.x0;
}

/// Original: u 0x115556 (text: new line; the cursor stays on the last line when the next one
/// would reach the view's bottom edge)
pub fn line_feed(ctx: &mut TextCtx, font: &WouFont) {
    carriage_return(ctx);
    let y = ctx.y.wrapping_add(height(font));
    if y < ctx.view.y1 {
        ctx.y = y;
    }
}

/// Original: u 0x115498 (text: draw character `c` at the cursor and advance)
///
/// CR and LF move the cursor. A glyph that would end right of the view's right edge goes to the
/// next line first (the original retries without bound; a glyph wider than the view is drawn
/// after one retry instead of looping). The cursor stops at the right edge.
pub fn put_char(chip: &mut Chip, ctx: &mut TextCtx, font: &WouFont, c: u8) {
    match c {
        0x0d => return carriage_return(ctx),
        0x0a => return line_feed(ctx, font),
        _ => {}
    }
    let w = width(font, c);
    for _ in 0..2 {
        if ctx.x.wrapping_add(w).wrapping_sub(1) <= ctx.view.x1 {
            break;
        }
        line_feed(ctx, font);
    }
    draw_glyph(chip, ctx, font, c);
    ctx.x = ctx.x.wrapping_add(w);
    if ctx.view.x1 <= ctx.x {
        ctx.x = ctx.view.x1;
    }
}

/// Original: u 0x1158ec (text: draw one glyph at the cursor, every pixel, through the colour
/// map of the context)
pub fn draw_glyph(chip: &mut Chip, ctx: &TextCtx, font: &WouFont, c: u8) {
    let (ink, paper) = (font.raw[2], font.raw[3]);
    let h = height(font).max(0) as usize;
    let w = width(font, c).max(0) as usize;
    let off = usize::from(font.raw[0x104 + usize::from(c)])
        | usize::from(font.raw[0x204 + usize::from(c)]) << 8;
    for r in 0..h {
        for col in 0..w {
            let v = font.raw.get(off + r * w + col).copied().unwrap_or(0);
            let v = if v == ink {
                ctx.fg
            } else if v == paper {
                ctx.bg
            } else {
                v
            };
            chip.set_pixel(
                ctx.view.planes,
                i32::from(ctx.x) + col as i32,
                i32::from(ctx.y) + r as i32,
                glyph_colour(v),
            );
        }
    }
}

/// Sends one character to `out`.
fn emit(chip: &mut Chip, ctx: &mut TextCtx, font: &WouFont, out: Out, c: u8) {
    match out {
        Out::Screen => put_char(chip, ctx, font, c),
        Out::Buffer => ctx.buf.push(c),
    }
}

/// Original: u 0x1140a2 (text: `printf` engine)
///
/// Codes: `%B` / `%F` set the background / ink colour, `%X` / `%Y` the cursor, `%P` flushes the
/// buffer (0x115a84), `%c` `%s` `%d` `%u` `%x` `%D`(long `%ld`) `%U`(`%lu`) print their argument,
/// any other `%x` prints `x`. Arguments are taken in order; a missing one reads as 0.
pub fn printf(
    chip: &mut Chip,
    ctx: &mut TextCtx,
    font: &WouFont,
    out: Out,
    fmt: &[u8],
    args: &[Arg],
) {
    let mut args = args.iter();
    let mut word = |args: &mut std::slice::Iter<'_, Arg>| match args.next() {
        Some(Arg::Word(w)) => *w,
        Some(Arg::Long(l)) => (*l >> 16) as u16,
        _ => 0,
    };
    let mut i = 0;
    while i < fmt.len() && fmt[i] != 0 {
        let c = fmt[i];
        i += 1;
        if c != b'%' {
            emit(chip, ctx, font, out, c);
            continue;
        }
        let code = fmt.get(i).copied().unwrap_or(0);
        i += 1;
        let text: Option<String> = match code {
            b'B' => {
                ctx.bg = word(&mut args) as u8;
                None
            }
            b'F' => {
                ctx.fg = word(&mut args) as u8;
                None
            }
            b'X' => {
                ctx.x = word(&mut args) as i16;
                None
            }
            b'Y' => {
                ctx.y = word(&mut args) as i16;
                None
            }
            b'P' => {
                let text = ctx.buf.clone();
                wrap(chip, ctx, font, &text);
                None
            }
            b'c' => {
                emit(chip, ctx, font, out, word(&mut args) as u8);
                None
            }
            b's' => {
                if let Some(Arg::Str(s)) = args.next() {
                    for &b in s.iter().take_while(|&&b| b != 0) {
                        emit(chip, ctx, font, out, b);
                    }
                }
                None
            }
            b'd' => Some((word(&mut args) as i16).to_string()),
            b'u' => Some(word(&mut args).to_string()),
            b'x' => Some(format!("{:X}", word(&mut args))),
            b'D' | b'U' => {
                let l = match args.next() {
                    Some(Arg::Long(l)) => *l,
                    Some(Arg::Word(w)) => u32::from(*w) << 16,
                    _ => 0,
                };
                Some(if code == b'D' {
                    (l as i32).to_string()
                } else {
                    l.to_string()
                })
            }
            0 => break,
            other => {
                emit(chip, ctx, font, out, other);
                None
            }
        };
        if let Some(t) = text {
            for b in t.bytes() {
                emit(chip, ctx, font, out, b);
            }
        }
    }
}

/// Original: u 0x1142a6 (text: `printf` straight to the screen)
pub fn print(chip: &mut Chip, ctx: &mut TextCtx, font: &WouFont, fmt: &[u8], args: &[Arg]) {
    printf(chip, ctx, font, Out::Screen, fmt, args);
}

/// Original: u 0x1142c6 (text: restart the buffer, then `printf` into it)
pub fn print_new(chip: &mut Chip, ctx: &mut TextCtx, font: &WouFont, fmt: &[u8], args: &[Arg]) {
    ctx.buf.clear();
    printf(chip, ctx, font, Out::Buffer, fmt, args);
}

/// Original: u 0x1142f0 (text: `printf` appended to the buffer)
pub fn print_more(chip: &mut Chip, ctx: &mut TextCtx, font: &WouFont, fmt: &[u8], args: &[Arg]) {
    printf(chip, ctx, font, Out::Buffer, fmt, args);
}

/// The byte at `i` of the text (0 past the end, as after the NUL).
fn at(text: &[u8], i: usize) -> u8 {
    text.get(i).copied().unwrap_or(0)
}

/// Original: u 0x115a92 (text: word-wrap `text` into the view from the cursor, in the context's
/// alignment; also reached as 0x115a84)
pub fn wrap(chip: &mut Chip, ctx: &mut TextCtx, font: &WouFont, text: &[u8]) {
    let (right, left) = (ctx.view.x1, ctx.view.x0);
    let mut pos = 0usize;
    loop {
        let start = pos;
        // The first word always goes on the line.
        let mut end = pos;
        if at(text, end) != 0 {
            loop {
                let c = at(text, end);
                end += 1;
                if c == 0 || c == 0x0a {
                    break;
                }
                if c == b' ' {
                    end -= 1;
                    break;
                }
            }
        }
        // Then words while the line still fits (end = the last space before the overflow).
        let mut x = ctx.x as u16;
        let mut d = pos;
        loop {
            let c = at(text, d);
            if c == 0 || c == 0x0a {
                end = d + 1;
                break;
            }
            if c == b' ' {
                end = d;
            }
            d += 1;
            x = x.wrapping_add(width(font, c) as u16);
            if x >= right as u16 {
                break;
            }
        }
        let avail = (right as u16).wrapping_sub(ctx.x as u16).wrapping_add(1);
        let line_w = (start..end).fold(0u16, |s, i| s.wrapping_add(width(font, at(text, i)) as u16));
        match ctx.align {
            0 => put_span(chip, ctx, font, text, start, end),
            1 => align_right(chip, ctx, font, text, (start, end), (line_w, avail), left),
            2 => align_centre(chip, ctx, font, text, (start, end), (line_w, avail)),
            _ => justify(chip, ctx, font, text, (start, end), (line_w, avail)),
        }
        pos = end;
        let last = at(text, end.wrapping_sub(1));
        if last != 0 && last != 0x0a {
            line_feed(ctx, font);
            while at(text, pos) == b' ' {
                pos += 1;
            }
        }
        if last == 0 {
            return;
        }
    }
}

/// Original: u 0x115c42 (text: draw `text[start..end]`, stopping at a NUL)
fn put_span(chip: &mut Chip, ctx: &mut TextCtx, font: &WouFont, text: &[u8], start: usize, end: usize) {
    for i in start..end {
        let c = at(text, i);
        if c == 0 {
            break;
        }
        put_char(chip, ctx, font, c);
    }
}

/// Original: u 0x115bdc (text: centre the line when it fits, then draw it)
fn align_centre(
    chip: &mut Chip,
    ctx: &mut TextCtx,
    font: &WouFont,
    text: &[u8],
    (start, end): (usize, usize),
    (line_w, avail): (u16, u16),
) {
    if at(text, start) != 0 && line_w <= avail {
        let pad = (avail - line_w).wrapping_add(1) >> 1;
        ctx.x = ctx.x.wrapping_add(pad as i16);
    }
    put_span(chip, ctx, font, text, start, end);
}

/// Original: u 0x115c0e (text: right-align the line when it fits, else start at the left edge)
fn align_right(
    chip: &mut Chip,
    ctx: &mut TextCtx,
    font: &WouFont,
    text: &[u8],
    (start, end): (usize, usize),
    (line_w, avail): (u16, u16),
    left: i16,
) {
    ctx.x = if at(text, start) != 0 && line_w <= avail {
        (ctx.view.x1 as u16).wrapping_sub(line_w).wrapping_add(1) as i16
    } else {
        left
    };
    put_span(chip, ctx, font, text, start, end);
}

/// Original: u 0x115c60 (text: justify a full line: leading spaces and the first word as they
/// are, then the slack spread over the remaining spaces, the remainder one pixel each from the
/// left; a short line or the paragraph's last line is drawn as it is)
fn justify(
    chip: &mut Chip,
    ctx: &mut TextCtx,
    font: &WouFont,
    text: &[u8],
    (mut start, end): (usize, usize),
    (mut line_w, avail): (u16, u16),
) {
    let last = at(text, end.wrapping_sub(1));
    if line_w >= avail || last == 0 || last == 0x0a {
        return put_span(chip, ctx, font, text, start, end);
    }
    while at(text, start) == b' ' {
        put_char(chip, ctx, font, b' ');
        line_w = line_w.wrapping_sub(width(font, b' ') as u16);
        start += 1;
    }
    while at(text, start) != b' ' {
        let c = at(text, start);
        put_char(chip, ctx, font, c);
        line_w = line_w.wrapping_sub(width(font, c) as u16);
        start += 1;
    }
    let slack = (ctx.view.x1 as u16)
        .wrapping_sub(ctx.x as u16)
        .wrapping_add(1)
        .wrapping_sub(line_w);
    let spaces = (start..end).filter(|&i| at(text, i) == b' ').count() as u16;
    if spaces == 0 {
        return put_span(chip, ctx, font, text, start, end);
    }
    let (each, mut extra) = ((slack / spaces) as u8, (slack % spaces) as u8);
    for i in start..end {
        let c = at(text, i);
        if c == 0 {
            break;
        }
        put_char(chip, ctx, font, c);
        if c == b' ' {
            ctx.x = ctx.x.wrapping_add(i16::from(each));
            if extra != 0 {
                extra -= 1;
                ctx.x = ctx.x.wrapping_add(1);
            }
        }
    }
}

/// Original: u 0x11568e (text: fill from (x, y) to the view's right edge, one glyph height + 1
/// rows, with the background colour)
pub fn clear_line_from(chip: &mut Chip, ctx: &TextCtx, font: &WouFont, x: i16, y: i16) {
    let v = View {
        planes: ctx.view.planes,
        x1: ctx.view.x1,
        x0: x,
        y0: y,
        y1: y.wrapping_add(height(font)),
    };
    super::mem::fill_view(chip, &v, ctx.bg);
}

/// Original: u 0x115672 (text: clear the rest of the cursor's line)
pub fn clear_eol(chip: &mut Chip, ctx: &TextCtx, font: &WouFont) {
    clear_line_from(chip, ctx, font, ctx.x, ctx.y);
}

/// Original: u 0x11563c (text: clear from the cursor to the view's bottom, line by line; also
/// 0x115630)
///
/// The original steps by `(byte 1 << 1) | byte 0` of the font (a shift of 1 where 8 is meant);
/// for `u6.set` (byte 1 = 0) that is the height.
pub fn clear_eos(chip: &mut Chip, ctx: &TextCtx, font: &WouFont) {
    let step = (i16::from(font.raw[1]) << 1) | i16::from(font.raw[0]);
    let (mut x, mut y) = (ctx.x, ctx.y);
    while y < ctx.view.y1 && step > 0 {
        clear_line_from(chip, ctx, font, x, y);
        x = ctx.view.x0;
        y = y.wrapping_add(step);
    }
}

/// Original: u 0x11560e (text: cursor home, then clear the whole view)
pub fn clear_view(chip: &mut Chip, ctx: &mut TextCtx, font: &WouFont) {
    ctx.x = ctx.view.x0;
    ctx.y = ctx.view.y0;
    clear_eos(chip, ctx, font);
}

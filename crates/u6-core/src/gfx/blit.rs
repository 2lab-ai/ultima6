//! The blitter and pixel routines of hunk 30, on chunky pixels.
//!
//! Addressing is the original's: a plane byte at `row_table[y] + x/8` (the table at 0x157e9e
//! holds `y·40` for y = 0..199) becomes pixels `(that byte)·8 + bit` of the linear 320×200
//! buffer. Nothing clips: a tile at x = 312 continues on the next row at x = 0, as it does on
//! the Amiga. Writes past the last pixel (the original would land in the next plane) are
//! dropped, and so is any call with y ≥ 200 (the original indexes past its row table).
//!
//! Spec 13 Wide mode: the buffer is the [`WIDTH`] × [`HEIGHT`] frame (912 × 408, spec 14), its
//! rows that long; the UI keeps its 320 × 200 in the frame's top-left corner ([`UI`]) and the map
//! lies beside it. A write that starts in the UI's columns stays in the UI (Design 4): its pixels
//! past x = 319, which the Amiga wraps into the next row, would land in the map, and are dropped,
//! as are those past y = 199, where the Amiga's row table ends ([`stays`]: the tile blitter, the
//! fill, the rect copy, the bitmap blit, the plot, the flight sprites). In Original the frame is
//! the UI's and nothing changes.
//!
//! Spec 07 HD0: the record blit and the bitmap blit name the image each pixel comes from on the
//! source plane (when the buffer has one, [`Framebuffer::enable_src`]), and the copies between
//! the buffers carry it ([`Framebuffer::copy_px`]); every other write here (plots, fills) is
//! classic. The colours are the same with and without the plane.

use u6_formats::gfx::{ANIM_FRAMES, ANIM_PLANE, Span, TILE_W, TileKind};
use u6_render::{FLAG_OUTLINE, FLAG_OVERLAY, Framebuffer, HEIGHT, Kind, Src, WIDTH};

use super::GfxState;
use crate::geom::SHOWN;

/// Bytes per plane row.
const ROW_BYTES: usize = WIDTH / 8;
/// The UI's 320 × 200 screen, columns × rows: the frame's top-left corner in both builds (all of
/// the frame in Original; spec 13 Geometry).
pub const UI: (usize, usize) = (320, 200);
/// Side of the dissolve's tile: the original's shown map, 160 × 160 pixels (0x1520e2 with
/// (8, 8)..(0xa7, 0xa7)).
pub const DISSOLVE_SIDE: u16 = 160;

/// Spec 13 Design 4: whether a write that starts at frame column `x0` may write linear pixel `i`.
/// Always in Original (the frame is the UI's screen: the Amiga's wrap into the next row); in Wide
/// not when `x0` is in the UI's columns and `i` is outside the UI's screen ([`UI`]): past its
/// columns, in the map, or past its rows.
pub(super) fn stays(x0: usize, i: usize) -> bool {
    (WIDTH, HEIGHT) == UI || x0 >= UI.0 || (i % WIDTH < UI.0 && i / WIDTH < UI.1)
}

/// Original: 0x157e9e (word per row: y·40), as the byte offset of row `y`.
fn row_base(y: u16) -> Option<usize> {
    (usize::from(y) < HEIGHT).then(|| usize::from(y) * ROW_BYTES)
}

/// Applies `f` to the pixel at linear index `i`, if it is on the screen (classic).
fn update(fb: &mut Framebuffer, i: usize, f: impl FnOnce(u8) -> u8) {
    if i < WIDTH * HEIGHT {
        let (x, y) = (i % WIDTH, i / WIDTH);
        let p = fb.get(x, y).unwrap_or(0);
        fb.set(x, y, f(p));
    }
}

/// [`update`] with the pixel's source `src`.
fn update_src(fb: &mut Framebuffer, i: usize, src: Src, f: impl FnOnce(u8) -> u8) {
    if i < WIDTH * HEIGHT {
        let (x, y) = (i % WIDTH, i / WIDTH);
        let p = fb.get(x, y).unwrap_or(0);
        fb.set_src(x, y, f(p), src);
    }
}

/// The source of pixel (u, v) of tile record `record` at version `ver`, with `flags`.
fn tile_src(record: u16, ver: u32, (u, v): (usize, usize), flags: u8) -> Src {
    Src {
        ver,
        id: record,
        u: u as u16,
        v: v as u16,
        kind: Kind::Tile,
        flags,
    }
}

/// Linear pixel index of column `c` of tile row `r` for a tile whose byte address is `base`.
fn pixel_at(base: usize, r: usize, c: usize) -> usize {
    (base + r * ROW_BYTES) * 8 + c
}

/// Outline colour for tile word bits 11..15 (`0x001530DE..0x0015310A`).
pub fn outline_colour(word: u16) -> u8 {
    match word & 0xF800 {
        0x0800 => 11,
        0x1000 => 4,
        _ => 3,
    }
}

/// Original: 0x152ffa (tile blitter, entry A: draws into the back buffer)
///
/// `word` = tile number in bits 0..10, outline mode in bits 11..12; (x, y) in pixels, x taken
/// down to a multiple of 8. `back` is the buffer the pointer at 0x161be2 names.
pub fn blit_back(gfx: &GfxState, back: &mut Framebuffer, word: u16, x: u16, y: u16) {
    blit(gfx, back, word, x, y);
}

/// Original: 0x153002 (tile blitter, entry B: draws into the front buffer)
///
/// Same body as [`blit_back`] (both run from 0x153008); `front` is the buffer at 0x161bde.
pub fn blit_front(gfx: &GfxState, front: &mut Framebuffer, word: u16, x: u16, y: u16) {
    blit(gfx, front, word, x, y);
}

/// The shared body at 0x153008: masktype of the live tile decides the path.
///
/// - `& 3 == 0`, map record: rows `first..first+h` copied opaque, other rows untouched.
/// - object record: pixels under a set AND-mask bit untouched, every other pixel takes the
///   record's colour (planes not stored stay 0). Right-span records sit one byte to the right.
/// - object record with `word & 0x1800`: outline mode ([`outline`]).
/// - `& 4`: the overlay frame [`GfxState::frame`] is OR-ed into colour bit 3 of the screen pixels
///   under its bits, over all 16 columns from the unshifted byte address (0x152e4c).
///
/// Source plane (spec 07): a pixel the record writes names the record (`record_of`, not the
/// tile number: substitutions share records), its current version
/// ([`super::cache::RenderCache::version`]) and its (column, row) in the record. A pixel under
/// an overlay bit names the same record at its (column, row) with [`FLAG_OVERLAY`], whether the
/// record or something under it drew it (its colour is the OR either way; an overlay whose rows
/// pass the record's 16 names rows past them), and an outline pixel names the record pixel it
/// outlines with [`FLAG_OUTLINE`].
fn blit(gfx: &GfxState, fb: &mut Framebuffer, word: u16, x: u16, y: u16) {
    let Some(row) = row_base(y) else { return };
    let base = row + usize::from(x >> 3);
    let x0 = usize::from(x >> 3) * 8;
    let (rec, overlay, masktype) = gfx.record(word);
    let id = gfx.record_of[usize::from(word & 0x7ff)];
    let ver = gfx.cache.version(usize::from(id));
    if masktype & 3 != 0 && word & 0x1800 != 0 {
        // The original then reads its overlay reference out of the scratch buffer (stale bytes
        // after the coverage rows); the port draws no overlay in outline mode.
        outline(fb, rec, (id, ver), (x0, base), outline_colour(word));
        return;
    }
    for (i, (&p, &clear)) in rec.pixels.iter().zip(&rec.transparent).enumerate() {
        let (r, c) = (i / TILE_W, i % TILE_W);
        if !clear && stays(x0, pixel_at(base, r, c)) {
            let src = tile_src(id, ver, (c, r), 0);
            update_src(fb, pixel_at(base, r, c), src, |_| p);
        }
    }
    if masktype & 4 != 0
        && let Some(ov) = overlay
    {
        // 0x15803e is only written by 0x152dca (`& 7`); larger values would skip past frame 7.
        let rows = &ov.frames[usize::from(gfx.frame) % ANIM_FRAMES];
        for (k, bits) in rows.iter().enumerate() {
            let r = usize::from(ov.first_row) + k;
            for c in 0..TILE_W {
                if bits & (0x8000 >> c) != 0 && stays(x0, pixel_at(base, r, c)) {
                    let src = tile_src(id, ver, (c, r), FLAG_OVERLAY);
                    update_src(fb, pixel_at(base, r, c), src, |p| p | 1 << ANIM_PLANE);
                }
            }
        }
    }
}

/// Outline mode (`0x001530DE..0x001531C2`): paints `colour` on the opaque colour-0 pixels of an
/// object record (NOT mask AND NOT every stored plane, built in the scratch buffer 0x155584),
/// leaving every other pixel. The routine from the table at 0x152bc2 sets the colour's planes and
/// clears the others, plane 4 included. Unlike the normal path this one does not add the byte
/// for right-span records, so their outline lands 8 pixels left of the sprite. `(id, ver)`: the
/// record and its version, for the source plane; `(x0, base)`: the frame column and the byte
/// address the tile starts at.
fn outline(
    fb: &mut Framebuffer,
    rec: &u6_formats::gfx::Tile,
    (id, ver): (u16, u32),
    (x0, base): (usize, usize),
    colour: u8,
) {
    let shift = match rec.kind {
        TileKind::Object(Span::Right) => 8,
        _ => 0,
    };
    for (i, (&p, &clear)) in rec.pixels.iter().zip(&rec.transparent).enumerate() {
        let (r, c) = (i / TILE_W, i % TILE_W);
        if !clear && p == 0 && c >= shift && stays(x0, pixel_at(base, r, c - shift)) {
            let src = tile_src(id, ver, (c, r), FLAG_OUTLINE);
            update_src(fb, pixel_at(base, r, c - shift), src, |_| colour);
        }
    }
}

/// Original: 0x152176 (set the plot colour, word at 0x152174)
pub fn set_plot_colour(gfx: &mut GfxState, colour: u16) {
    gfx.plot_colour = colour;
}

/// Original: 0x152188 (plot one pixel in the plot colour)
///
/// The routine tables hold 16 entries: 0x1524b4 sets the pixel to the colour (planes 0..3 from
/// its bits, plane 4 cleared), 0x152584 XORs the colour's bits into planes 0..3 and is used when
/// [`GfxState::plot_xor`] (0x155574) is set. The original picks the front buffer, or the back one
/// when [`GfxState::plot_back`] (0x155577) is set; the caller passes that buffer as `fb`. A colour
/// above 15 would index past both tables; the port then plots nothing.
pub fn plot(gfx: &GfxState, fb: &mut Framebuffer, x: u16, y: u16) {
    plot_in(fb, x, y, gfx.plot_colour, gfx.plot_xor);
}

/// [`plot`] with the colour and the mode given; in Wide not below the UI's screen in its columns
/// ([`stays`]).
fn plot_in(fb: &mut Framebuffer, x: u16, y: u16, colour: u16, xor: bool) {
    let Some(row) = row_base(y) else { return };
    let Ok(colour) = u8::try_from(colour) else {
        return;
    };
    if colour > 15 {
        return;
    }
    let i = (row + usize::from(x >> 3)) * 8 + usize::from(x & 7);
    if !stays(usize::from(x), i) {
        return;
    }
    if xor {
        update(fb, i, |p| p ^ colour);
    } else {
        update(fb, i, |_| colour);
    }
}

/// Original: 0x1521f4 (graphics: fill the rectangle (x0, y0)..=(x1, y1) in the plot colour,
/// set or XOR as [`plot`] does, into the buffer [`GfxState::plot_back`] names)
///
/// The corners are put in order (unsigned); whole plane bytes are written with the edge masks
/// 0x15802e / 0x158036 on the first and last byte of each row, so exactly the pixels x0..=x1
/// change. The caller passes the buffer as `fb`.
pub fn fill_rect(gfx: &GfxState, fb: &mut Framebuffer, x0: u16, y0: u16, x1: u16, y1: u16) {
    fill(fb, (x0, y0, x1, y1), gfx.plot_colour, gfx.plot_xor);
}

/// [`fill_rect`] in `colour` (set mode), for callers that pass the colour of 0x152176 directly.
pub fn fill_colour(fb: &mut Framebuffer, x0: u16, y0: u16, x1: u16, y1: u16, colour: u8) {
    fill(fb, (x0, y0, x1, y1), u16::from(colour), false);
}

/// The corners are put in order as 0x152244..0x15224e does (unsigned), then held to the screen:
/// the original has no bound and runs its byte pointer on past the row (and the buffer) for
/// corners that wrapped below zero. A fill from the UI's columns is held to the UI's screen
/// (spec 13 Design 4, [`stays`]; the same bound in Original).
fn fill(fb: &mut Framebuffer, (x0, y0, x1, y1): (u16, u16, u16, u16), colour: u16, xor: bool) {
    let (xa, xb) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
    let (ya, yb) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
    let (right, bottom) = if usize::from(xa) < UI.0 {
        UI
    } else {
        (WIDTH, HEIGHT)
    };
    let xb = xb.min(right as u16 - 1);
    let yb = yb.min(bottom as u16 - 1);
    for y in ya..=yb {
        for x in xa..=xb {
            plot_in(fb, x, y, colour, xor);
        }
    }
}

/// Original: 0x15206c (copy one pixel, all 5 planes, from the back buffer to the front)
///
/// The pixel's source comes along ([`Framebuffer::copy_px`]).
pub fn copy_pixel(back: &Framebuffer, front: &mut Framebuffer, x: u16, y: u16) {
    let Some(row) = row_base(y) else { return };
    let i = (row + usize::from(x >> 3)) * 8 + usize::from(x & 7);
    front.copy_px(back, i, i);
}

/// The pixel order of 0x1520e2 over a `w`×`h` rect, as offsets (dx, dy).
///
/// A Galois LFSR as wide as `w·h - 1` (16-bit arithmetic, `subq.w`) steps from 1 until it is 1
/// again; each state `s` names pixel (s mod w, s div w) and is skipped when that row is past the
/// rect. Pixel (0, 0), which the register never holds, comes last. `taps[width - 2]` is the
/// feedback word. A rect of one or two pixels (width < 2) would read before the table in the
/// original; the port then returns the pixels in raster order. A width or height of 0 (corners
/// that wrapped; `divu` by 0 traps in the original) has no pixels.
///
/// The loop ends for every feedback word F. Reading bit i as x^(15 - i), a step multiplies by x
/// modulo x^16 + F = x^m·Q with Q(0) = 1 (m ≤ 15 when F ≠ 0); the register starts at x^15, a
/// multiple of x^m, and on those multiples the step is invertible, so for F ≠ 0 the register
/// returns to 1 within 65535 steps and never meets 0. F = 0 takes it to 0 at the first step
/// (raster order, below). The bound below is never reached.
pub fn dissolve_order(w: u16, h: u16, taps: &[u16]) -> Vec<(u16, u16)> {
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let last = (u32::from(w) * u32::from(h)) as u16;
    let width = 16 - last.wrapping_sub(1).leading_zeros() as usize;
    let Some(&feedback) = width.checked_sub(2).and_then(|i| taps.get(i)) else {
        return (0..h).flat_map(|r| (0..w).map(move |c| (c, r))).collect();
    };
    let mut out = Vec::new();
    let mut s: u16 = 1;
    for _ in 0..=u16::MAX {
        let (row, col) = (s / w, s % w);
        if row < h {
            out.push((col, row));
        }
        let carry = s & 1 != 0;
        s >>= 1;
        if carry {
            s ^= feedback;
        }
        // Only a zero feedback word (not the game's) takes the register to 0, where it would
        // stay: raster order then, rather than spin.
        if s == 0 {
            return (0..h).flat_map(|r| (0..w).map(move |c| (c, r))).collect();
        }
        if s == 1 {
            break;
        }
    }
    out.push((0, 0));
    out
}

/// Original: 0x1520e2 (dissolve: copy the rect (x0, y0)..=(x1, y1) back → front pixel by pixel in
/// LFSR order)
///
/// The original has no waits inside: the reveal is the CPU time of 25,600 pixel copies on the
/// live screen. The port copies in the same order in one call. The map window's dissolve is
/// [`dissolve_map`].
pub fn dissolve(
    taps: &[u16],
    back: &Framebuffer,
    front: &mut Framebuffer,
    (x0, y0): (u16, u16),
    (x1, y1): (u16, u16),
) {
    let (w, h) = (
        x1.wrapping_sub(x0).wrapping_add(1),
        y1.wrapping_sub(y0).wrapping_add(1),
    );
    for (dx, dy) in dissolve_order(w, h, taps) {
        copy_pixel(back, front, x0 + dx, y0 + dy);
    }
}

/// The map's dissolve (spec 13 Decision 7): the original's order over its 160 × 160 shown map
/// ([`dissolve_order`], [`DISSOLVE_SIDE`]) tiles [`SHOWN`] from its top left, each step revealing
/// its pixel in every tile (rows of tiles, then columns), the tiles past the rect clipped. In
/// Original the shown map is the one tile: 0x1520e2 over (8, 8)..(0xa7, 0xa7). Wide reveals its
/// bigger map in as many steps, so in the same time.
#[derive(Clone, Debug)]
pub struct MapDissolve {
    /// Pixel offsets in a tile, in the order they are revealed.
    order: Vec<(u16, u16)>,
    /// Frame pixel of each tile's top left.
    tiles: Vec<(u16, u16)>,
}

impl MapDissolve {
    /// The dissolve with the game's feedback words `taps` (0x151f3c).
    pub fn new(taps: &[u16]) -> MapDissolve {
        let (x0, y0, x1, y1) = SHOWN;
        let side = usize::from(DISSOLVE_SIDE);
        let tiles = (y0..=y1)
            .step_by(side)
            .flat_map(|y| (x0..=x1).step_by(side).map(move |x| (x, y)))
            .collect();
        MapDissolve {
            order: dissolve_order(DISSOLVE_SIDE, DISSOLVE_SIDE, taps),
            tiles,
        }
    }

    /// The steps of the dissolve.
    pub fn steps(&self) -> usize {
        self.order.len()
    }

    /// The frame pixels step `k` reveals: its offset in every tile, those inside [`SHOWN`].
    pub fn step(&self, k: usize) -> impl Iterator<Item = (u16, u16)> + '_ {
        let (_, _, x1, y1) = SHOWN;
        let tiles = &self.tiles;
        let at = self.order.get(k).copied();
        at.into_iter()
            .flat_map(move |(dx, dy)| tiles.iter().map(move |&(x, y)| (x + dx, y + dy)))
            .filter(move |&(x, y)| x <= x1 && y <= y1)
    }
}

/// Original: 0x1520e2 over the map window's shown rect, (8, 8)..(0xa7, 0xa7) (0x102c90 with
/// A4-0x6578 set): [`MapDissolve`]'s steps copied back → front in one call, as [`dissolve`] does.
pub fn dissolve_map(taps: &[u16], back: &Framebuffer, front: &mut Framebuffer) {
    let d = MapDissolve::new(taps);
    for k in 0..d.steps() {
        for (x, y) in d.step(k) {
            copy_pixel(back, front, x, y);
        }
    }
}

/// The shown map's columns and rows, [`SHOWN`] as ranges (8..168 both in Original).
pub fn shown_ranges() -> (std::ops::Range<u16>, std::ops::Range<u16>) {
    let (x0, y0, x1, y1) = SHOWN;
    (x0..x1 + 1, y0..y1 + 1)
}

/// Original: 0x151f5a (copy the map window, rows 8..168 × bytes 1..21 = pixels 8..168, back →
/// front, all 5 planes; the shown map [`SHOWN`], spec 13)
pub fn copy_window(back: &Framebuffer, front: &mut Framebuffer) {
    let (xs, ys) = shown_ranges();
    for y in ys {
        for x in xs.clone() {
            copy_pixel(back, front, x, y);
        }
    }
}

/// [`copy_window`]'s result by rows: each row of its rect whose colours differ between the
/// buffers is copied, the others (which the copy would leave as they are) are not. With a source
/// plane on the screen, the sources of the rect that may differ are copied too: those written in
/// either buffer since the last such copy, including a row of equal colours that a replay put
/// back classic ([`Framebuffer::sync_rect_from`]; the screen is the one buffer synced from the
/// back buffer, always over this rect).
pub fn copy_window_rows(back: &mut Framebuffer, front: &mut Framebuffer) {
    let (xs, ys) = shown_ranges();
    let wide = |r: std::ops::Range<u16>| usize::from(r.start)..usize::from(r.end);
    front.sync_rect_from(back, wide(xs), wide(ys));
}

/// Original: 0x151fe6 (copy the pixel rectangle (x0, y0)..=(x1, y1) of the back buffer to
/// (dx, dy) on the screen: whole bytes of 8 pixels, all five planes)
///
/// Columns are bytes: x0 / 8 ..= x1 / 8 go to dx / 8 onwards (`lsr.w #3`), rows y0..=y1 to
/// dy onwards. A row past the screen is dropped (the original would land in the next plane), and
/// in Wide what a copy to the UI's columns would put outside the UI's screen ([`stays`]). The
/// pixels' sources come along ([`Framebuffer::copy_px`]).
#[allow(clippy::too_many_arguments)]
pub fn copy_rect(
    back: &Framebuffer,
    front: &mut Framebuffer,
    x0: u16,
    y0: u16,
    x1: u16,
    y1: u16,
    dx: u16,
    dy: u16,
) {
    let bytes = (x1 >> 3).wrapping_sub(x0 >> 3);
    let rows = y1.wrapping_sub(y0);
    // A copy to the UI's columns has the UI's screen's bounds (the frame's in Original).
    let (w, h) = if usize::from(dx) < UI.0 {
        UI
    } else {
        (WIDTH, HEIGHT)
    };
    if bytes >= (w / 8) as u16 || rows >= h as u16 {
        return;
    }
    let to = usize::from(dx >> 3) * 8;
    for r in 0..=rows {
        let (Some(src), Some(dst)) = (back_byte(x0, y0 + r), row_base(dy + r)) else {
            continue;
        };
        for b in 0..=bytes {
            let s = (src + usize::from(b)) * 8;
            let d = (dst + usize::from((dx >> 3) + b)) * 8;
            if !stays(to, d) {
                continue;
            }
            for bit in 0..8 {
                front.copy_px(back, s + bit, d + bit);
            }
        }
    }
}

/// Original: 0x152dd8 (graphics: the back-buffer byte address of pixel (x, y): the row table's
/// row `y` plus x / 8)
fn back_byte(x: u16, y: u16) -> Option<usize> {
    row_base(y).map(|row| row + usize::from(x >> 3))
}

/// What a bitmap is on the source plane (spec 07 HD0): a portrait, a panel bitmap, or nothing
/// named (its pixels classic).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitmapSource {
    Classic,
    /// A portrait: its [`u6_render::portrait_id`].
    Portrait(u16),
    /// A panel bitmap: its role ([`u6_render::BITMAP_PAPER`], [`u6_render::BITMAP_SPELLBOOK`]).
    Bitmap(u16),
}

/// Original: 0x153384 (draw a `.bmp` bitmap at (x, y): its header is two little-endian words,
/// width and height, read byte by byte; then the body of 0x1533e0)
///
/// The header is decoded by `u6_formats::bitmap` (the LZW file 0x1346a0 reads). `role`: the
/// bitmap's [`u6_render::Src::id`] ([`u6_render::BITMAP_PAPER`], [`u6_render::BITMAP_SPELLBOOK`]).
pub fn blit_bmp(
    colours: &[u8; 256],
    front: &mut Framebuffer,
    bmp: &u6_formats::bitmap::Bitmap,
    x: u16,
    y: u16,
    role: u16,
) {
    let (size, src) = ((bmp.width, bmp.height), BitmapSource::Bitmap(role));
    blit_bitmap(colours, front, size, &bmp.pixels, x, y, src);
}

/// Original: 0x1533e0 (draw an 8-bit bitmap with a word header on the screen); also the body of
/// 0x153384
///
/// `pixels` is `width × height` bytes, row-major; each byte becomes screen colour
/// `colours[byte] & 0x1f` (the five bits the shifts take into the planes). The bitmap starts
/// at byte column x / 8 of row y; each row is `width` pixels, the next row starts a screen row
/// below. Pixels past the screen are dropped, and in Wide those outside the UI's screen of a
/// bitmap that starts in its columns ([`stays`]).
///
/// `src` names the image for the source plane (spec 07): each pixel gets (column, row) in the
/// bitmap, version 0; [`BitmapSource::Classic`] draws classic pixels.
pub fn blit_bitmap(
    colours: &[u8; 256],
    front: &mut Framebuffer,
    (width, height): (u16, u16),
    pixels: &[u8],
    x: u16,
    y: u16,
    src: BitmapSource,
) {
    let (kind, id) = match src {
        BitmapSource::Classic => (Kind::Classic, 0),
        BitmapSource::Portrait(id) => (Kind::Portrait, id),
        BitmapSource::Bitmap(role) => (Kind::Bitmap, role),
    };
    for r in 0..usize::from(height) {
        let Some(row) = row_base(y.wrapping_add(r as u16)) else {
            continue;
        };
        let start = (row + usize::from(x >> 3)) * 8;
        let x0 = usize::from(x >> 3) * 8;
        for c in 0..usize::from(width) {
            let Some(&b) = pixels.get(r * usize::from(width) + c) else {
                return;
            };
            if !stays(x0, start + c) {
                continue;
            }
            let src = match kind {
                Kind::Classic => Src::CLASSIC,
                _ => Src {
                    ver: 0,
                    id,
                    u: c as u16,
                    v: r as u16,
                    kind,
                    flags: 0,
                },
            };
            update_src(front, start + c, src, |_| colours[usize::from(b)] & 0x1f);
        }
    }
}

/// Original: 0x152dc8 (empty: a bare `rts`, reached from boot through 0x1354b0)
pub fn empty_hook() {}

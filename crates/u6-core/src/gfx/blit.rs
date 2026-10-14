//! The blitter and pixel routines of hunk 30, on chunky pixels.
//!
//! Addressing is the original's: a plane byte at `row_table[y] + x/8` (the table at 0x157e9e
//! holds `y·40` for y = 0..199) becomes pixels `(that byte)·8 + bit` of the linear 320×200
//! buffer. Nothing clips: a tile at x = 312 continues on the next row at x = 0, as it does on
//! the Amiga. Writes past the last pixel (the original would land in the next plane) are
//! dropped, and so is any call with y ≥ 200 (the original indexes past its row table).

use u6_formats::gfx::{ANIM_FRAMES, ANIM_PLANE, Span, TILE_W, TileKind};
use u6_render::{Framebuffer, HEIGHT, WIDTH};

use super::GfxState;

/// Bytes per plane row.
const ROW_BYTES: usize = WIDTH / 8;

/// Original: 0x157e9e (word per row: y·40), as the byte offset of row `y`.
fn row_base(y: u16) -> Option<usize> {
    (usize::from(y) < HEIGHT).then(|| usize::from(y) * ROW_BYTES)
}

/// Applies `f` to the pixel at linear index `i`, if it is on the screen.
fn update(fb: &mut Framebuffer, i: usize, f: impl FnOnce(u8) -> u8) {
    if i < WIDTH * HEIGHT {
        let (x, y) = (i % WIDTH, i / WIDTH);
        let p = fb.get(x, y).unwrap_or(0);
        fb.set(x, y, f(p));
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
fn blit(gfx: &GfxState, fb: &mut Framebuffer, word: u16, x: u16, y: u16) {
    let Some(row) = row_base(y) else { return };
    let base = row + usize::from(x >> 3);
    let (rec, overlay, masktype) = gfx.record(word);
    if masktype & 3 != 0 && word & 0x1800 != 0 {
        // The original then reads its overlay reference out of the scratch buffer (stale bytes
        // after the coverage rows); the port draws no overlay in outline mode.
        outline(fb, rec, base, outline_colour(word));
        return;
    }
    for (i, (&p, &clear)) in rec.pixels.iter().zip(&rec.transparent).enumerate() {
        if !clear {
            update(fb, pixel_at(base, i / TILE_W, i % TILE_W), |_| p);
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
                if bits & (0x8000 >> c) != 0 {
                    update(fb, pixel_at(base, r, c), |p| p | 1 << ANIM_PLANE);
                }
            }
        }
    }
}

/// Outline mode (`0x001530DE..0x001531C2`): paints `colour` on the opaque colour-0 pixels of an
/// object record (NOT mask AND NOT every stored plane, built in the scratch buffer 0x155584),
/// leaving every other pixel. The routine from the table at 0x152bc2 sets the colour's planes and
/// clears the others, plane 4 included. Unlike the normal path this one does not add the byte
/// for right-span records, so their outline lands 8 pixels left of the sprite.
fn outline(fb: &mut Framebuffer, rec: &u6_formats::gfx::Tile, base: usize, colour: u8) {
    let shift = match rec.kind {
        TileKind::Object(Span::Right) => 8,
        _ => 0,
    };
    for (i, (&p, &clear)) in rec.pixels.iter().zip(&rec.transparent).enumerate() {
        let (r, c) = (i / TILE_W, i % TILE_W);
        if !clear && p == 0 && c >= shift {
            update(fb, pixel_at(base, r, c - shift), |_| colour);
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
    let Some(row) = row_base(y) else { return };
    let Ok(colour) = u8::try_from(gfx.plot_colour) else {
        return;
    };
    if colour > 15 {
        return;
    }
    let i = (row + usize::from(x >> 3)) * 8 + usize::from(x & 7);
    if gfx.plot_xor {
        update(fb, i, |p| p ^ colour);
    } else {
        update(fb, i, |_| colour);
    }
}

/// Original: 0x15206c (copy one pixel, all 5 planes, from the back buffer to the front)
pub fn copy_pixel(back: &Framebuffer, front: &mut Framebuffer, x: u16, y: u16) {
    let Some(row) = row_base(y) else { return };
    let i = (row + usize::from(x >> 3)) * 8 + usize::from(x & 7);
    if i < WIDTH * HEIGHT {
        let p = back.get(i % WIDTH, i / WIDTH).unwrap_or(0);
        update(front, i, |_| p);
    }
}

/// The pixel order of 0x1520e2 over a `w`×`h` rect, as offsets (dx, dy).
///
/// A Galois LFSR as wide as `w·h - 1` (16-bit arithmetic, `subq.w`) steps from 1 until it is 1
/// again; each state `s` names pixel (s mod w, s div w) and is skipped when that row is past the
/// rect. Pixel (0, 0), which the register never holds, comes last. `taps[width - 2]` is the
/// feedback word. A rect of one or two pixels (width < 2) would read before the table in the
/// original; the port then returns the pixels in raster order.
pub fn dissolve_order(w: u16, h: u16, taps: &[u16]) -> Vec<(u16, u16)> {
    let last = (u32::from(w) * u32::from(h)) as u16;
    let width = 16 - last.wrapping_sub(1).leading_zeros() as usize;
    let Some(&feedback) = width.checked_sub(2).and_then(|i| taps.get(i)) else {
        return (0..h).flat_map(|r| (0..w).map(move |c| (c, r))).collect();
    };
    let mut out = Vec::new();
    let mut s: u16 = 1;
    loop {
        let (row, col) = (s / w, s % w);
        if row < h {
            out.push((col, row));
        }
        let carry = s & 1 != 0;
        s >>= 1;
        if carry {
            s ^= feedback;
        }
        // A zero register never comes back to 1 (only with a zero feedback word, which the
        // game's table does not hold): raster order then, rather than spin.
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
/// live screen. The port copies in the same order in one call.
pub fn dissolve(
    taps: &[u16],
    back: &Framebuffer,
    front: &mut Framebuffer,
    (x0, y0): (u16, u16),
    (x1, y1): (u16, u16),
) {
    let (w, h) = (x1.wrapping_sub(x0) + 1, y1.wrapping_sub(y0) + 1);
    for (dx, dy) in dissolve_order(w, h, taps) {
        copy_pixel(back, front, x0 + dx, y0 + dy);
    }
}

/// Original: 0x151f5a (copy the map window, rows 8..168 × bytes 1..21 = pixels 8..168, back →
/// front, all 5 planes)
pub fn copy_window(back: &Framebuffer, front: &mut Framebuffer) {
    for y in 8..168 {
        for x in 8..168 {
            copy_pixel(back, front, x, y);
        }
    }
}

/// Original: 0x151fe6 (copy the pixel rectangle (x0, y0)..=(x1, y1) of the back buffer to
/// (dx, dy) on the screen: whole bytes of 8 pixels, all five planes)
///
/// Columns are bytes: x0 / 8 ..= x1 / 8 go to dx / 8 onwards (`lsr.w #3`), rows y0..=y1 to
/// dy onwards. A row past the screen is dropped (the original would land in the next plane).
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
    if bytes >= ROW_BYTES as u16 || rows >= HEIGHT as u16 {
        return;
    }
    for r in 0..=rows {
        let (Some(src), Some(dst)) = (row_base(y0 + r), row_base(dy + r)) else {
            continue;
        };
        for b in 0..=bytes {
            let s = (src + usize::from((x0 >> 3) + b)) * 8;
            let d = (dst + usize::from((dx >> 3) + b)) * 8;
            for bit in 0..8 {
                let i = s + bit;
                if i < WIDTH * HEIGHT {
                    let p = back.get(i % WIDTH, i / WIDTH).unwrap_or(0);
                    update(front, d + bit, |_| p);
                }
            }
        }
    }
}

/// Original: 0x1533e0 (draw an 8-bit bitmap with a word header on the screen); also the body of
/// 0x153384 (the same with a byte-pair header, `paper.bmp`)
///
/// `pixels` is `width × height` bytes, row-major; each byte becomes screen colour
/// `colours[byte] & 0x1f` (the five bits the shifts take into the planes). The bitmap starts
/// at byte column x / 8 of row y; each row is `width` pixels, the next row starts a screen row
/// below. Pixels past the screen are dropped.
pub fn blit_bitmap(
    colours: &[u8; 256],
    front: &mut Framebuffer,
    (width, height): (u16, u16),
    pixels: &[u8],
    x: u16,
    y: u16,
) {
    for r in 0..usize::from(height) {
        let Some(row) = row_base(y.wrapping_add(r as u16)) else {
            continue;
        };
        let start = (row + usize::from(x >> 3)) * 8;
        for c in 0..usize::from(width) {
            let Some(&b) = pixels.get(r * usize::from(width) + c) else {
                return;
            };
            update(front, start + c, |_| colours[usize::from(b)] & 0x1f);
        }
    }
}

/// Original: 0x152dc8 (empty: a bare `rts`, reached from boot through 0x1354b0)
pub fn empty_hook() {}

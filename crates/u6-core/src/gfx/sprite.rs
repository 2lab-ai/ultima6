//! The turned sprites of missile flights (hunk 30): 0x15265a prepares one from an object tile
//! turned by an angle, 0x152800 saves the back-buffer block under it, 0x152876 draws it there
//! (or puts the saved block back). Chunky pixels in the original's byte addressing ([`super::blit`]):
//! a block is 3 plane bytes (24 pixels) wide from byte column `(x - 8) / 8`, rows y - 8 .. y + 7.

use u6_formats::gfx::{Span, TILE_W, TileKind};
use u6_render::{Framebuffer, HEIGHT, WIDTH};

use super::GfxState;

/// Sprite slots (0x15265a ignores `slot >= 16`; 0x14df70 holds 16 blocks of 0xf0 bytes).
pub const SLOTS: usize = 16;
/// Bytes of one sprite: 16 × 16 colours.
const PIXELS: usize = TILE_W * TILE_W;
/// A clear sprite pixel.
const CLEAR: u8 = 0xff;
/// Pixels per row of a saved block (3 plane bytes).
const BLOCK_W: usize = 24;

/// The sprite buffers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sprites {
    /// Original: (0x155584) + 0x6800 (256 bytes: the decoded tile 0x15265a turns; bytes it does
    /// not write keep what the previous tile left)
    pub scratch: [u8; PIXELS],
    /// Original: (0x155584) + 0x6900 (16 × 256 bytes: the prepared sprites, 0xff = clear)
    pub slots: Vec<[u8; PIXELS]>,
    /// Original: 0x1525c4 (16 bytes: the slot holds a saved background; 0x152800 sets it,
    /// 0x15265a clears it)
    pub saved: [bool; SLOTS],
    /// Original: 0x14df70 (16 × 0xf0 bytes: the 24 × 16 pixels under each slot)
    pub under: Vec<[u8; BLOCK_W * TILE_W]>,
    /// Original: 0x1525d4 / 0x1525d6 (words: the turning centre the last call passed, × 8)
    pub centre: (i16, i16),
}

impl Sprites {
    /// No sprite buffers (the record-less view of [`GfxState`]): nothing draws or saves.
    pub(crate) fn none() -> Sprites {
        Sprites {
            scratch: [CLEAR; PIXELS],
            slots: Vec::new(),
            saved: [false; SLOTS],
            under: Vec::new(),
            centre: (0, 0),
        }
    }
}

impl Default for Sprites {
    fn default() -> Self {
        Self {
            scratch: [CLEAR; PIXELS],
            slots: vec![[CLEAR; PIXELS]; SLOTS],
            saved: [false; SLOTS],
            under: vec![[0; BLOCK_W * TILE_W]; SLOTS],
            centre: (0, 0),
        }
    }
}

/// Linear pixel index of pixel `c` of the row at byte address (row `y`, byte column `col`), as
/// the original's byte pointer plus bit; `None` for a row off the 200 (the row table ends).
fn at(y: i32, col: i32, c: usize) -> Option<usize> {
    if !(0..HEIGHT as i32).contains(&y) {
        return None;
    }
    let i = (y as usize * (WIDTH / 8)).checked_add_signed(col as isize)? * 8 + c;
    (i < WIDTH * HEIGHT).then_some(i)
}

fn get(fb: &Framebuffer, i: usize) -> u8 {
    fb.get(i % WIDTH, i / WIDTH).unwrap_or(0)
}

fn set(fb: &mut Framebuffer, i: usize, v: u8) {
    fb.set(i % WIDTH, i / WIDTH, v);
}

/// Original: 0x15265a (graphics: prepare sprite `slot` from object tile `tile` turned by `angle`
/// (128 steps a turn, the sine table 0x1525d9) about (cx, cy); clears the slot's saved flag)
///
/// The tile is decoded into the scratch buffer as 0x15265a does it: rows above the record's
/// first row and after its last mask row clear, the mask rows from the first row (a set bit is
/// clear, else colour 0), one-byte spans (left or right) in columns 0..8 with columns 8..16 of
/// those rows left as they were; then the stored planes are OR-ed in from row 0 (not from the
/// first row: a record that starts lower gets its colours that many rows up, as in the game).
/// The sprite samples the scratch at the turned positions of the 16 × 16 pixel centres; a
/// position off the tile is clear. A map tile or a slot past 15 leaves everything unchanged.
pub fn prepare(gfx: &mut GfxState, slot: u16, tile: u16, angle: u16, cx: i16, cy: i16) {
    gfx.sprites.centre = (cx.wrapping_shl(3), cy.wrapping_shl(3));
    let slot = usize::from(slot);
    let Some(&masktype) = gfx.masktype.get(usize::from(tile)) else {
        return;
    };
    if slot >= SLOTS || masktype & 3 == 0 {
        return;
    }
    decode(gfx, tile);
    gfx.sprites.saved[slot] = false;
    let sine = &gfx.tables.sine;
    let sin = i16::from(sine[usize::from(angle & 0x7f)]);
    let cos = i16::from(sine[usize::from((angle & 0x7f).wrapping_add(0x20) & 0x7f)]);
    let (ox, oy) = gfx.sprites.centre;
    let src = gfx.sprites.scratch;
    let Some(dst) = gfx.sprites.slots.get_mut(slot) else {
        return;
    };
    let mut i = 0;
    for row in (-15i16..16).step_by(2) {
        for col in (-15i16..16).step_by(2) {
            let u = (col.wrapping_mul(cos))
                .wrapping_sub(row.wrapping_mul(sin))
                .wrapping_add(ox);
            let v = (col.wrapping_mul(sin))
                .wrapping_add(row.wrapping_mul(cos))
                .wrapping_add(oy);
            // `lsl.w #1` then `lsr.w #8`: bits 7..14 of the word, compared as a byte.
            let sx = ((u as u16) << 1 >> 8) as u8;
            let sy = ((v as u16) << 1 >> 8) as u8;
            dst[i] = if sx < 16 && sy < 16 {
                src[usize::from(sy << 4 | sx)]
            } else {
                CLEAR
            };
            i += 1;
        }
    }
}

/// 0x1526a6..0x152748: the object record of `tile` into the scratch buffer (0x152c44 finds it).
fn decode(gfx: &mut GfxState, tile: u16) {
    let (rec, _, _) = gfx.record(tile);
    let (first, rows) = (usize::from(rec.first_row), usize::from(rec.rows));
    let (width, col0) = match rec.kind {
        TileKind::Object(Span::Full) | TileKind::Map => (16, 0),
        TileKind::Object(Span::Left) => (8, 0),
        TileKind::Object(Span::Right) => (8, 8),
    };
    let (pixels, transparent) = (rec.pixels.clone(), rec.transparent.clone());
    let scratch = &mut gfx.sprites.scratch;
    // 0x1526c2 clears `first` rows with no bound: a first row past 16 (the loader refuses it)
    // would write past the 256-byte scratch; the port stops at its end.
    scratch[..(first * TILE_W).min(PIXELS)].fill(CLEAR);
    for r in 0..rows.min(TILE_W - first.min(TILE_W)) {
        let y = first + r;
        for c in 0..width {
            let clear = transparent[y * TILE_W + col0 + c];
            scratch[y * TILE_W + c] = if clear { CLEAR } else { 0 };
        }
    }
    scratch[((first + rows) * TILE_W).min(PIXELS)..].fill(CLEAR);
    for r in 0..rows.min(TILE_W - first.min(TILE_W)) {
        let y = first + r;
        for c in 0..width {
            scratch[r * TILE_W + c] |= pixels[y * TILE_W + col0 + c];
        }
    }
}

/// Original: 0x152800 (graphics: save the back-buffer block under a sprite at (x, y): 24 × 16
/// pixels from byte column x / 8 - 1, rows y - 8 .. y + 7; marks the slot as holding it)
pub fn save(gfx: &mut GfxState, back: &Framebuffer, slot: u16, x: u16, y: u16) {
    let slot = usize::from(slot) % SLOTS;
    gfx.sprites.saved[slot] = true;
    let col = i32::from(x >> 3) - 1;
    let Some(under) = gfx.sprites.under.get_mut(slot) else {
        return;
    };
    for r in 0..TILE_W {
        for c in 0..BLOCK_W {
            if let Some(i) = at(i32::from(y) - 8 + r as i32, col, c) {
                under[r * BLOCK_W + c] = get(back, i);
            }
        }
    }
}

/// Original: 0x152876 (graphics: draw sprite `slot` into the back buffer with its top left at
/// (x - 8, y - 8), clear pixels left alone; a slot holding a saved background puts the block
/// back instead)
pub fn draw(gfx: &GfxState, back: &mut Framebuffer, slot: u16, x: u16, y: u16) {
    let slot = usize::from(slot) % SLOTS;
    let left = x.wrapping_sub(8);
    let col = i32::from(left >> 3);
    let top = i32::from(y) - 8;
    let (Some(under), Some(sprite)) = (gfx.sprites.under.get(slot), gfx.sprites.slots.get(slot))
    else {
        return;
    };
    if gfx.sprites.saved[slot] {
        for r in 0..TILE_W {
            for c in 0..BLOCK_W {
                if let Some(i) = at(top + r as i32, col, c) {
                    set(back, i, under[r * BLOCK_W + c]);
                }
            }
        }
        return;
    }
    let bit = usize::from(left & 7);
    for r in 0..TILE_W {
        for c in 0..TILE_W {
            let v = sprite[r * TILE_W + c];
            if v != CLEAR
                && let Some(i) = at(top + r as i32, col, bit + c)
            {
                set(back, i, v & 0x1f);
            }
        }
    }
}

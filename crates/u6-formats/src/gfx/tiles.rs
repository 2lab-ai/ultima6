//! Tile storage decoders (graphics.md §4-§5): `tileindx.ega`, `maptiles.ega`, `objtiles.ega`.

use super::{PLANES, Span, TILE_COUNT, TILE_H, TILE_PIXELS, TILE_W, Tile, TileKind, Tileset};
use crate::bytes;
use crate::error::{FormatError, Result};
use crate::lzw;

/// `tileindx` values from here up are map tiles at `value - 0x6000` (`0x00152E50`).
const MAP_BASE: u16 = 0x6000;
/// `tileindx` values below this are object tiles at file offset `value << 4`.
const OBJ_LIMIT: u16 = 0x5FFF;
/// Bytes the object-tile cache reads per tile (`0x0014F16C`, `0x001623D6`: 0xCC).
const OBJ_RECORD: usize = 0xCC;
/// Plane-list terminator in an object tile.
const PLANE_END: u8 = 0xFF;

/// Decodes all 2048 tiles.
///
/// Inputs are the PowerPacker-decrunched files from `DATA1`. `masktype` may be the file as stored
/// (a U6-LZW stream) or its 2048-byte decoded form. Every tile is bounds-checked against the
/// region the game itself would read.
pub fn decode_tiles(
    maptiles: &[u8],
    objtiles: &[u8],
    tileindx: &[u8],
    masktype: &[u8],
) -> Result<Tileset> {
    bytes::expect_len(tileindx, TILE_COUNT * 2, "tileindx.ega")?;
    let decoded;
    let masktype = if masktype.len() == TILE_COUNT {
        masktype
    } else {
        decoded = lzw::decode(masktype)?;
        bytes::expect_len(&decoded, TILE_COUNT, "masktype.ega (decoded)")?;
        &decoded[..]
    };
    let maps = MapPlanes::new(maptiles)?;
    let mut tiles = Vec::with_capacity(TILE_COUNT);
    for (t, &mt) in masktype.iter().enumerate() {
        let index = bytes::u16_le(tileindx, t * 2, "tileindx entry")?;
        let tile = if mt & 3 == 0 {
            maps.tile(t, index, mt)?
        } else {
            object_tile(objtiles, t, index, mt)?
        };
        tiles.push(tile);
    }
    Ok(Tileset { tiles })
}

/// A blank tile: nothing written yet, every pixel transparent.
fn blank(masktype: u8, kind: TileKind, first_row: u8, rows: u8) -> Tile {
    Tile {
        masktype,
        kind,
        first_row,
        rows,
        pixels: vec![0; TILE_PIXELS],
        transparent: vec![true; TILE_PIXELS],
        unused_byte: None,
        anim_ref: None,
    }
}

/// Rejects a row range that does not fit the 16-row tile. An empty record (`rows == 0`) draws
/// nothing but still names its first row: up to 16 (the file uses `16, 0`); a later one would
/// make the sprite decoder `0x0015265A` clear rows past its 16-row buffer.
fn check_rows(first: u8, rows: u8, what: &'static str, offset: usize) -> Result<()> {
    if usize::from(first) + usize::from(rows) > TILE_H {
        return Err(FormatError::invalid(
            what,
            offset,
            format!("rows {first}+{rows} exceed the 16-row tile"),
        ));
    }
    Ok(())
}

/// `maptiles.ega` = `u16 LE n`, then 5 planes of `n` bytes each. A tile at offset `o` is
/// `[first row][rows]` at `o`, its rows (2 bytes each, plane 0) at `o + 2`, and plane `k`'s rows at
/// the same position `+ k·n` (`0x00153002`: `A0 + D6`, `D6 = n`).
struct MapPlanes<'a> {
    n: usize,
    /// The whole file; plane `k` starts at `2 + k·n`.
    file: &'a [u8],
}

impl<'a> MapPlanes<'a> {
    fn new(file: &'a [u8]) -> Result<Self> {
        let n = usize::from(bytes::u16_le(file, 0, "maptiles.ega plane length")?);
        bytes::slice(file, 2, PLANES * n, "maptiles.ega planes")?;
        Ok(MapPlanes { n, file })
    }

    fn tile(&self, t: usize, index: u16, mt: u8) -> Result<Tile> {
        if index < MAP_BASE {
            return Err(FormatError::invalid(
                "tileindx entry",
                t * 2,
                format!("tile {t} is a map tile (masktype {mt}) but index {index:#06x} < 0x6000"),
            ));
        }
        let o = 2 + usize::from(index - MAP_BASE);
        // Plane 0 holds the header, the rows and the animation reference; keeping them inside
        // plane 0 keeps planes 1..4 (same positions + k·n) inside the buffer.
        let plane0 = &self.file[..2 + self.n];
        let head = bytes::slice(plane0, o, 2, "maptiles.ega tile header")?;
        let (first, rows) = (head[0], head[1]);
        check_rows(first, rows, "maptiles.ega tile header", o)?;
        let h = usize::from(rows);
        bytes::slice(plane0, o + 2, 2 * h, "maptiles.ega tile rows")?;
        let mut tile = blank(mt, TileKind::Map, first, rows);
        for r in 0..h {
            let y = usize::from(first) + r;
            for k in 0..PLANES {
                let at = k * self.n + o + 2 + 2 * r;
                let bits = u16::from_be_bytes([self.file[at], self.file[at + 1]]);
                for x in 0..TILE_W {
                    if bits & (0x8000 >> x) != 0 {
                        tile.pixels[y * TILE_W + x] |= 1 << k;
                    }
                }
            }
            tile.transparent[y * TILE_W..(y + 1) * TILE_W].fill(false);
        }
        if mt & 4 != 0 {
            tile.anim_ref = Some(bytes::u16_le(
                plane0,
                o + 2 + 2 * h,
                "maptiles.ega anim ref",
            )?);
        }
        Ok(tile)
    }
}

/// An object tile at `objtiles.ega[index << 4]`, at most 0xCC bytes (the cache slot size):
/// `[first row][rows][unused]`, the AND mask (`rows × w` bytes, bit set = keep the screen), then
/// planes as `[plane 0..4][rows × w bytes]` until `0xFF`, then the animation reference if
/// `masktype & 4`. `w` = 2 for 16-wide tiles, 1 for 8-wide ones.
fn object_tile(objtiles: &[u8], t: usize, index: u16, mt: u8) -> Result<Tile> {
    const WHAT: &str = "objtiles.ega tile";
    if index >= OBJ_LIMIT {
        return Err(FormatError::invalid(
            "tileindx entry",
            t * 2,
            format!("tile {t} is an object tile (masktype {mt}) but index {index:#06x} >= 0x5FFF"),
        ));
    }
    let base = usize::from(index) << 4;
    if base >= objtiles.len() {
        return Err(FormatError::invalid(
            WHAT,
            base,
            format!("tile {t} starts past the end of the file"),
        ));
    }
    // Every read below goes through `rec`, so a record that needs more than the cached 0xCC
    // bytes is an error, as it would read stale cache memory in the game.
    let rec = &objtiles[base..objtiles.len().min(base + OBJ_RECORD)];
    let err = |at: usize, detail: String| FormatError::invalid(WHAT, base + at, detail);
    let get = |at: usize, n: usize| {
        rec.get(at..at + n)
            .ok_or_else(|| err(at, format!("tile {t} needs bytes {at}..{} of 0xCC", at + n)))
    };
    let head = get(0, 3)?;
    let (first, rows, unused) = (head[0], head[1], head[2]);
    check_rows(first, rows, WHAT, base)?;
    let span = match mt & 3 {
        3 => Span::Full,
        2 => Span::Left,
        _ => Span::Right,
    };
    let (w, col0) = match span {
        Span::Full => (2, 0),
        Span::Left => (1, 0),
        Span::Right => (1, 8),
    };
    let h = usize::from(rows);
    // Row bits widened to 16 columns: bit 15 = leftmost pixel of the tile.
    let row_bits = |b: &[u8], r: usize| -> u16 {
        if w == 2 {
            u16::from_be_bytes([b[2 * r], b[2 * r + 1]])
        } else {
            u16::from(b[r]) << (8 - col0)
        }
    };
    let mut tile = blank(mt, TileKind::Object(span), first, rows);
    tile.unused_byte = Some(unused);
    let mask = get(3, w * h)?;
    let mut opaque = [0u16; TILE_H];
    for r in 0..h {
        let y = usize::from(first) + r;
        // Columns outside the span are never written, so they stay transparent.
        let covered = if w == 2 { 0xFFFF } else { 0xFF00u16 >> col0 };
        opaque[y] = !row_bits(mask, r) & covered;
    }
    let mut at = 3 + w * h;
    loop {
        let plane = get(at, 1)?[0];
        at += 1;
        if plane == PLANE_END {
            break;
        }
        if usize::from(plane) >= super::PLANES {
            return Err(err(at - 1, format!("tile {t}: plane {plane} > 4")));
        }
        let data = get(at, w * h)?;
        for r in 0..h {
            let y = usize::from(first) + r;
            let bits = row_bits(data, r);
            if bits & !opaque[y] != 0 {
                // The game would OR these bits into the background; a flat image cannot say so.
                return Err(err(
                    at,
                    format!("tile {t}: plane {plane} bits under the mask"),
                ));
            }
            for x in 0..TILE_W {
                if bits & (0x8000 >> x) != 0 {
                    tile.pixels[y * TILE_W + x] |= 1 << plane;
                }
            }
        }
        at += w * h;
    }
    for (y, bits) in opaque.iter().enumerate() {
        for x in 0..TILE_W {
            if bits & (0x8000 >> x) != 0 {
                tile.transparent[y * TILE_W + x] = false;
            }
        }
    }
    if mt & 4 != 0 {
        let r = get(at, 2)?;
        tile.anim_ref = Some(u16::from_le_bytes([r[0], r[1]]));
    }
    Ok(tile)
}

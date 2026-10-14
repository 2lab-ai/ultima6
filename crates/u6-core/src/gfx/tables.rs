//! Tables the draw path reads out of the user's `game` executable at run time (spec 03 §5: data
//! is extracted, never compiled in). Addresses are image addresses (base 0x00100000); the file
//! offset of an address is its hunk's header offset + 8 + (address - hunk base) in the
//! PowerPacker-decrunched `game` (267,332 bytes). Each read checks the `HUNK_HEADER` magic, the
//! hunk's type and longword count, like `u6_formats::gfx::palette` does for the palette. The
//! bytes read here carry no relocations (word tables and data words); a real-data test compares
//! them with the flattened image.

use u6_formats::FormatError;
use u6_formats::gfx::GAME_EXE_LEN;

/// Shore merges per pass of 0x152d52 (`moveq #0x1f` + `dbf`).
pub const SHORE_MERGES: usize = 32;
/// LFSR feedback words of 0x1520e2, one per register width 2..=16 bits.
pub const DISSOLVE_TAPS: usize = 15;

const HUNK_HEADER: u32 = 0x3F3;
const HUNK_CODE: u32 = 0x3E9;
const HUNK_DATA: u32 = 0x3EA;

/// One loaded hunk of `game` (re/notes/game-layout.md, `game` segments).
struct Hunk {
    /// File offset of the hunk's type longword.
    header: usize,
    kind: u32,
    /// Longwords of loaded data that follow the 8-byte header.
    longs: u32,
    /// Image address of the first data byte.
    base: u32,
}

/// Hunk 30 (CODE, 0x16f0 bytes at 0x151ee0): the graphics code with its embedded tables.
const HUNK_30: Hunk = Hunk {
    header: 235_908,
    kind: HUNK_CODE,
    longs: 0x5BC,
    base: 0x0015_1EE0,
};

/// Hunk 33 (DATA, 0x4f0c loaded bytes at 0x1536f0): the small-data globals (A4 = 0x15b6f0).
const HUNK_33: Hunk = Hunk {
    header: 243_036,
    kind: HUNK_DATA,
    longs: 0x13C3,
    base: 0x0015_36F0,
};

fn be32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

fn invalid(offset: usize, detail: String) -> FormatError {
    FormatError::Invalid {
        what: "game",
        offset,
        detail,
    }
}

/// `len` bytes at image address `addr` inside `hunk`, after checking the file and hunk header.
fn image_bytes<'a>(
    game: &'a [u8],
    hunk: &Hunk,
    addr: u32,
    len: usize,
) -> Result<&'a [u8], FormatError> {
    if game.len() != GAME_EXE_LEN {
        return Err(FormatError::BadLength {
            what: "game",
            expected: format!("{GAME_EXE_LEN} (decrunched)"),
            actual: game.len(),
        });
    }
    if be32(game, 0) != Some(HUNK_HEADER) {
        return Err(invalid(0, "no HUNK_HEADER magic".into()));
    }
    let head = (be32(game, hunk.header), be32(game, hunk.header + 4));
    if head != (Some(hunk.kind), Some(hunk.longs)) {
        return Err(invalid(
            hunk.header,
            format!(
                "hunk header {head:x?}, expected {:#x} / {:#x}",
                hunk.kind, hunk.longs
            ),
        ));
    }
    let start = addr
        .checked_sub(hunk.base)
        .map(|d| d as usize)
        .filter(|&d| d + len <= hunk.longs as usize * 4)
        .ok_or_else(|| invalid(hunk.header, format!("{addr:#x} outside the hunk")))?;
    let off = hunk.header + 8 + start;
    game.get(off..off + len)
        .ok_or_else(|| invalid(off, "past the end of the file".into()))
}

fn words<const N: usize>(b: &[u8]) -> [u16; N] {
    std::array::from_fn(|i| u16::from_be_bytes([b[2 * i], b[2 * i + 1]]))
}

fn bytes<const N: usize>(b: &[u8]) -> [u8; N] {
    std::array::from_fn(|i| b[i])
}

/// The game tables the world view needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameTables {
    /// Original: 0x152c52 (hunk 30; 32 words: source tile × 4 of each shore merge)
    pub shore_src: [u16; SHORE_MERGES],
    /// Original: 0x152c92 (hunk 30; 32 words: target tile × 4 of each shore merge)
    pub shore_dst: [u16; SHORE_MERGES],
    /// Original: 0x151f3c (hunk 30; 15 words: LFSR feedback for widths 2..=16, 0x1520e2)
    pub dissolve_taps: [u16; DISSOLVE_TAPS],
    /// Original: 0x155180 (A4-0x6570; word: the plot colour 0x102c90 selects first)
    pub window_colour: u16,
    /// Original: 0x155188 (A4-0x6568; word: the plot colour of the clock hands, 0x102bf2)
    pub clock_colour: u16,
    /// Original: 0x153bfc (A4-0x7af4; 12 bytes: direction of the hour hand per hour mod 12)
    pub hour_dir: [u8; 12],
    /// Original: 0x155522 (A4-0x61ce; 8 signed bytes: x step per direction 0..7, N first)
    pub dir_dx: [i8; 8],
    /// Original: 0x15552a (A4-0x61c6; 8 signed bytes: y step per direction 0..7)
    pub dir_dy: [i8; 8],
    /// Original: 0x153bf8 (A4-0x7af8; image value of the "map window shown" word)
    pub window_shown: u16,
    /// Original: 0x155178 (A4-0x6578; image value of the "dissolve the next present" word)
    pub dissolve: u16,
    /// Original: 0x157d9e (A4-0x3952; 256 bytes: screen colour 0..31 of each 8-bit bitmap
    /// colour, the map 0x153384 / 0x1533e0 draw `paper.bmp` and the portraits through)
    pub bitmap_colours: [u8; 256],
}

impl Default for GameTables {
    fn default() -> Self {
        Self {
            shore_src: [0; SHORE_MERGES],
            shore_dst: [0; SHORE_MERGES],
            dissolve_taps: [0; DISSOLVE_TAPS],
            window_colour: 0,
            clock_colour: 0,
            hour_dir: [0; 12],
            dir_dx: [0; 8],
            dir_dy: [0; 8],
            window_shown: 0,
            dissolve: 0,
            bitmap_colours: [0; 256],
        }
    }
}

impl GameTables {
    /// Reads the tables from the decrunched `game`.
    pub fn from_game(game: &[u8]) -> Result<Self, FormatError> {
        let n = SHORE_MERGES * 2;
        let shore_src = words(image_bytes(game, &HUNK_30, 0x0015_2C52, n)?);
        let shore_dst = words(image_bytes(game, &HUNK_30, 0x0015_2C92, n)?);
        let dissolve_taps = words(image_bytes(game, &HUNK_30, 0x0015_1F3C, DISSOLVE_TAPS * 2)?);
        let word = |addr| image_bytes(game, &HUNK_33, addr, 2).map(|b| words::<1>(b)[0]);
        Ok(Self {
            shore_src,
            shore_dst,
            dissolve_taps,
            window_colour: word(0x0015_5180)?,
            clock_colour: word(0x0015_5188)?,
            hour_dir: bytes::<12>(image_bytes(game, &HUNK_33, 0x0015_3BFC, 12)?),
            dir_dx: bytes::<8>(image_bytes(game, &HUNK_33, 0x0015_5522, 8)?).map(|b| b as i8),
            dir_dy: bytes::<8>(image_bytes(game, &HUNK_33, 0x0015_552A, 8)?).map(|b| b as i8),
            window_shown: word(0x0015_3BF8)?,
            dissolve: word(0x0015_5178)?,
            bitmap_colours: bytes::<256>(image_bytes(game, &HUNK_33, 0x0015_7D9E, 256)?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic `game`: right length, header magic, hunk 30 and 33 headers, chosen bytes.
    pub(crate) fn synthetic_game() -> Vec<u8> {
        let mut g = vec![0u8; GAME_EXE_LEN];
        g[..4].copy_from_slice(&HUNK_HEADER.to_be_bytes());
        for h in [&HUNK_30, &HUNK_33] {
            g[h.header..h.header + 4].copy_from_slice(&h.kind.to_be_bytes());
            g[h.header + 4..h.header + 8].copy_from_slice(&h.longs.to_be_bytes());
        }
        let at = |h: &Hunk, a: u32| h.header + 8 + (a - h.base) as usize;
        for i in 0..SHORE_MERGES {
            let s = at(&HUNK_30, 0x0015_2C52) + 2 * i;
            g[s..s + 2].copy_from_slice(&(4 * (i as u16 + 1)).to_be_bytes());
            let d = at(&HUNK_30, 0x0015_2C92) + 2 * i;
            g[d..d + 2].copy_from_slice(&(4 * (i as u16 + 100)).to_be_bytes());
        }
        for i in 0..DISSOLVE_TAPS {
            let o = at(&HUNK_30, 0x0015_1F3C) + 2 * i;
            g[o..o + 2].copy_from_slice(&(0x3u16 << i).to_be_bytes());
        }
        let dirs = at(&HUNK_33, 0x0015_5522);
        g[dirs..dirs + 16]
            .copy_from_slice(&[0, 1, 1, 1, 0, 255, 255, 255, 255, 255, 0, 1, 1, 1, 0, 255]);
        let colours = at(&HUNK_33, 0x0015_7D9E);
        for (i, b) in g[colours..colours + 256].iter_mut().enumerate() {
            *b = (i % 32) as u8;
        }
        let hours = at(&HUNK_33, 0x0015_3BFC);
        g[hours..hours + 12].copy_from_slice(&[0, 0, 1, 1, 2, 3, 3, 4, 5, 5, 6, 7]);
        for (a, v) in [
            (0x0015_5180, 13u16),
            (0x0015_5188, 2),
            (0x0015_3BF8, 1),
            (0x0015_5178, 1),
        ] {
            let o = at(&HUNK_33, a);
            g[o..o + 2].copy_from_slice(&v.to_be_bytes());
        }
        g
    }

    #[test]
    fn reads_tables_at_their_hunk_offsets() {
        let t = GameTables::from_game(&synthetic_game()).unwrap();
        assert_eq!(t.shore_src[0], 4);
        assert_eq!(t.shore_src[31], 128);
        assert_eq!(t.shore_dst[0], 400);
        assert_eq!(t.dissolve_taps[14], 0x3 << 14);
        assert_eq!(t.window_colour, 13);
        assert_eq!((t.window_shown, t.dissolve, t.clock_colour), (1, 1, 2));
        assert_eq!((t.dir_dx[5], t.dir_dy[0], t.dir_dy[2]), (-1, -1, 0));
        assert_eq!(t.hour_dir[11], 7);
        assert_eq!((t.bitmap_colours[33], t.bitmap_colours[255]), (1, 31));
    }

    #[test]
    fn rejects_wrong_length_magic_and_hunk_header() {
        assert!(GameTables::from_game(&[0; 16]).is_err());
        let mut g = synthetic_game();
        g[0] = 0xff;
        assert!(GameTables::from_game(&g).is_err());
        let mut g = synthetic_game();
        g[HUNK_30.header + 7] ^= 1;
        let e = GameTables::from_game(&g).unwrap_err().to_string();
        assert!(e.contains("hunk header"), "{e}");
    }
}

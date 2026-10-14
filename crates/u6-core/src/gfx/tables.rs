//! Tables the draw path reads out of the user's `game` executable at run time (spec 03 §5: data
//! is extracted, never compiled in). Addresses are image addresses (base 0x00100000), read
//! through the shared hunk loader [`crate::image::GameImage`]. The version check is the hunk
//! layout the addresses were measured on: hunk 30 (CODE) at 0x151ee0 with 0x16f0 bytes, hunk
//! 33 (DATA) at 0x1536f0 with 0x4f0c bytes. The bytes read here carry no relocations (word
//! tables and data words); a real-data test compares them with the flattened image.

use u6_formats::FormatError;

use crate::image::GameImage;

/// Shore merges per pass of 0x152d52 (`moveq #0x1f` + `dbf`).
pub const SHORE_MERGES: usize = 32;
/// LFSR feedback words of 0x1520e2, one per register width 2..=16 bits.
pub const DISSOLVE_TAPS: usize = 15;

/// One loaded hunk of `game` (re/notes/game-layout.md, `game` segments).
struct Hunk {
    /// Hunk number.
    n: usize,
    /// Loaded bytes.
    len: usize,
    /// Image address of the first data byte.
    base: u32,
}

/// Hunk 30 (CODE, 0x16f0 bytes at 0x151ee0): the graphics code with its embedded tables.
const HUNK_30: Hunk = Hunk {
    n: 30,
    len: 0x16f0,
    base: 0x0015_1EE0,
};

/// Hunk 33 (DATA, 0x4f0c loaded bytes at 0x1536f0): the small-data globals (A4 = 0x15b6f0).
const HUNK_33: Hunk = Hunk {
    n: 33,
    len: 0x4f0c,
    base: 0x0015_36F0,
};

fn invalid(offset: usize, detail: String) -> FormatError {
    FormatError::Invalid {
        what: "game executable",
        offset,
        detail,
    }
}

/// Checks the two hunks this module reads are where they were measured.
fn check_layout(img: &GameImage<'_>) -> Result<(), FormatError> {
    for h in [&HUNK_30, &HUNK_33] {
        let got = (img.segment_base(h.n), img.loaded(h.n).map(<[u8]>::len));
        if got != (Some(h.base), Some(h.len)) {
            return Err(invalid(
                0,
                format!(
                    "hunk {} layout {got:x?}, expected {:#x} / {:#x}",
                    h.n, h.base, h.len
                ),
            ));
        }
    }
    Ok(())
}

/// `len` bytes at `addr`, which must lie inside hunk `hunk`.
fn image_bytes<'a>(
    img: &GameImage<'a>,
    hunk: &Hunk,
    addr: u32,
    len: usize,
) -> Result<&'a [u8], FormatError> {
    let inside = addr
        .checked_sub(hunk.base)
        .is_some_and(|d| d as usize + len <= hunk.len);
    if !inside {
        return Err(invalid(0, format!("{addr:#x} outside hunk {}", hunk.n)));
    }
    img.bytes(addr, len)
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
    /// Original: 0x1525d9 (hunk 30; 128 signed bytes: 64 × the sine of n / 128 turns, 0x15265a)
    pub sine: [i8; 128],
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
            sine: [0; 128],
        }
    }
}

impl GameTables {
    /// Reads the tables from the decrunched `game`.
    pub fn from_game(game: &[u8]) -> Result<Self, FormatError> {
        let img = GameImage::parse(game)?;
        check_layout(&img)?;
        let game = &img;
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
            sine: bytes::<128>(image_bytes(game, &HUNK_30, 0x0015_25D9, 128)?).map(|b| b as i8),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic `game`: hunk 30 and 33 at their measured bases and sizes, chosen bytes.
    pub(crate) fn synthetic_game() -> Vec<u8> {
        let mut h30 = vec![0u8; HUNK_30.len];
        let mut h33 = vec![0u8; HUNK_33.len];
        let at30 = |a: u32| (a - HUNK_30.base) as usize;
        let at33 = |a: u32| (a - HUNK_33.base) as usize;
        for i in 0..SHORE_MERGES {
            let s = at30(0x0015_2C52) + 2 * i;
            h30[s..s + 2].copy_from_slice(&(4 * (i as u16 + 1)).to_be_bytes());
            let d = at30(0x0015_2C92) + 2 * i;
            h30[d..d + 2].copy_from_slice(&(4 * (i as u16 + 100)).to_be_bytes());
        }
        for i in 0..DISSOLVE_TAPS {
            let o = at30(0x0015_1F3C) + 2 * i;
            h30[o..o + 2].copy_from_slice(&(0x3u16 << i).to_be_bytes());
        }
        let sine = at30(0x0015_25D9);
        for (i, b) in h30[sine..sine + 128].iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(3);
        }
        let dirs = at33(0x0015_5522);
        h33[dirs..dirs + 16]
            .copy_from_slice(&[0, 1, 1, 1, 0, 255, 255, 255, 255, 255, 0, 1, 1, 1, 0, 255]);
        let colours = at33(0x0015_7D9E);
        for (i, b) in h33[colours..colours + 256].iter_mut().enumerate() {
            *b = (i % 32) as u8;
        }
        let hours = at33(0x0015_3BFC);
        h33[hours..hours + 12].copy_from_slice(&[0, 0, 1, 1, 2, 3, 3, 4, 5, 5, 6, 7]);
        for (a, v) in [
            (0x0015_5180, 13u16),
            (0x0015_5188, 2),
            (0x0015_3BF8, 1),
            (0x0015_5178, 1),
        ] {
            let o = at33(a);
            h33[o..o + 2].copy_from_slice(&v.to_be_bytes());
        }
        crate::image::SyntheticGame::new()
            .place_at(HUNK_30.n, HUNK_30.base)
            .loaded(false, h30)
            .place_at(HUNK_33.n, HUNK_33.base)
            .loaded(true, h33)
            .bss(0)
            .build()
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
        assert_eq!((t.sine[1], t.sine[50], t.sine[127]), (3, -106, 125));
    }

    #[test]
    fn rejects_a_non_hunk_file_and_another_layout() {
        assert!(GameTables::from_game(&[0; 16]).is_err());
        let mut g = synthetic_game();
        g[0] = 0xff;
        assert!(GameTables::from_game(&g).is_err());
        let other = crate::image::SyntheticGame::new()
            .place_at(HUNK_30.n, HUNK_30.base + 8)
            .loaded(false, vec![0; HUNK_30.len])
            .build();
        let e = GameTables::from_game(&other).unwrap_err().to_string();
        assert!(e.contains("hunk 30 layout"), "{e}");
    }
}

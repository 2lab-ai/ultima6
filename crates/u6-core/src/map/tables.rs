//! Tables `map/` reads from the data hunk of the user's `game` (spec 03 §5 "data, not
//! constants"): the sight / light tables of 0x109c12 / 0x109e74 and the moongate destinations of
//! 0x11d1d4. Offsets are A4-relative as the code addresses them, read through the shared hunk
//! loader [`crate::image::GameImage`].

use u6_formats::FormatError;
use u6_formats::gfx::palette;

use super::{MapState, WINDOW};
use crate::files::Files;
use crate::image::GameImage;
use crate::state::LoadError;

/// The A4 offset of A4 - `off`.
const fn a4(off: usize) -> i32 {
    -(off as i32)
}
/// Moongate destinations: 25 entries (0x11d1d4 indexes them by quality - 1).
pub const GATES: usize = 25;
/// The row stride the image's walk steps are written with: the original's window side.
pub(super) const IMAGE_STRIDE: i32 = 40;

/// The tables, as read from `game`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapTables {
    /// Original: A4-0x794a (8 × 8 signed bytes, `dist[dx][dy]` = distance of a cell (dx, dy)
    /// away; 0x109c12)
    pub dist: [[i8; 8]; 8],
    /// Original: A4-0x7908 (8 signed bytes: x step of the walk round a cell's 8 neighbours,
    /// from north clockwise, each step from the previous neighbour; 0x109c12)
    pub walk_dx: [i8; 8],
    /// Original: A4-0x7900 (y steps of the same walk)
    pub walk_dy: [i8; 8],
    /// Original: A4-0x78f8 (cell-index steps of the same walk, `dy * 40 + dx`); the port's are
    /// `dy * WINDOW + dx`, re-encoded from the image's (spec 13 Design 2)
    pub walk_step: [i8; 8],
    /// Original: A4-0x78f0 (16 bytes: tile offset of a wall piece per neighbour mask; 0x109e74)
    pub wall: [u8; 16],
    /// Original: A4-0x6b24 / -0x6af2 (25 words each) and A4-0x6ac0 (25 bytes): x, y, z a
    /// moongate of quality q sends the party to (entry q - 1; 0x11d1d4)
    pub gate_x: [u16; GATES],
    pub gate_y: [u16; GATES],
    pub gate_z: [u8; GATES],
}

impl MapTables {
    /// Reads the tables from the decrunched `game`. The file is first checked the way the
    /// palette reader checks it (length, hunk header, hunk 33's HUNK_DATA); the walk tables
    /// must agree with each other (`step = dy * 40 + dx`, the image's stride). The steps are
    /// then re-encoded with [`WINDOW`] (unchanged where it is 40).
    pub fn from_game(game: &[u8]) -> Result<MapTables, FormatError> {
        palette(game)?;
        let img = GameImage::parse(game)?;
        let at = |off: i32, n: usize| img.a4_bytes(off, n);
        let signed = |off: i32| -> Result<[i8; 8], FormatError> {
            let b = at(off, 8)?;
            Ok(std::array::from_fn(|i| b[i] as i8))
        };
        let words = |off: i32| -> Result<[u16; GATES], FormatError> {
            let b = at(off, GATES * 2)?;
            Ok(std::array::from_fn(|i| {
                u16::from_be_bytes([b[i * 2], b[i * 2 + 1]])
            }))
        };
        let dist = at(a4(0x794a), 64)?;
        let wall = at(a4(0x78f0), 16)?;
        let gate_z = at(a4(0x6ac0), GATES)?;
        let mut t = MapTables {
            dist: std::array::from_fn(|dx| std::array::from_fn(|dy| dist[dx * 8 + dy] as i8)),
            walk_dx: signed(a4(0x7908))?,
            walk_dy: signed(a4(0x7900))?,
            walk_step: signed(a4(0x78f8))?,
            wall: std::array::from_fn(|i| wall[i]),
            gate_x: words(a4(0x6b24))?,
            gate_y: words(a4(0x6af2))?,
            gate_z: std::array::from_fn(|i| gate_z[i]),
        };
        t.walk_step =
            walk_steps(&t.walk_dx, &t.walk_dy, &t.walk_step).map_err(|i| FormatError::Invalid {
                what: "game view walk tables",
                offset: img
                    .file_offset(crate::image::A4.wrapping_add_signed(a4(0x78f8)))
                    .unwrap_or(0)
                    + i,
                detail: format!(
                    "step {} != dy * 40 + dx; unknown game version",
                    t.walk_step[i]
                ),
            })?;
        Ok(t)
    }
}

/// The walk's cell-index steps for the port's grids, `dy * WINDOW + dx`, from the image's
/// `image` steps, which must be `dy * 40 + dx` (spec 13 Design 2: decoded with stride 40,
/// re-encoded with [`WINDOW`]). `Err(i)`: step `i` disagrees with its dx, dy, or does not fit
/// a byte at this window side.
pub(super) fn walk_steps(dx: &[i8; 8], dy: &[i8; 8], image: &[i8; 8]) -> Result<[i8; 8], usize> {
    let mut out = [0; 8];
    for i in 0..8 {
        let (dx, dy) = (i32::from(dx[i]), i32::from(dy[i]));
        let agree = dy * IMAGE_STRIDE + dx == i32::from(image[i]);
        out[i] = i8::try_from(dy * WINDOW as i32 + dx)
            .ok()
            .filter(|_| agree)
            .ok_or(i)?;
    }
    Ok(out)
}

impl MapState {
    /// Loads [`MapState::tables`] from `game` (a boot step: `State::new_game` should run it).
    pub fn load_tables(&mut self, files: &Files) -> Result<(), LoadError> {
        let game = files.get("game").ok_or_else(|| LoadError::Missing {
            file: "game".into(),
        })?;
        let tables = MapTables::from_game(game).map_err(|error| LoadError::Format {
            file: "game".into(),
            error,
        })?;
        self.tables = Some(tables);
        Ok(())
    }
}

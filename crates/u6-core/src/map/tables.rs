//! Tables `map/` reads from the data hunk of the user's `game` (spec 03 §5 "data, not
//! constants"): the sight / light tables of 0x109c12 / 0x109e74 and the moongate destinations of
//! 0x11d1d4. Offsets are A4-relative as the code addresses them (A4 = hunk 33 + 0x8000).

use u6_formats::FormatError;
use u6_formats::gfx::{GAME_HUNK33_BLOCK, palette};

use super::{MapState, WINDOW};
use crate::files::Files;
use crate::state::LoadError;

/// Offset of hunk 33's data in the decrunched `game` (its HUNK_DATA header is 8 bytes).
const HUNK33_DATA: usize = GAME_HUNK33_BLOCK + 8;
/// Hunk-33 offset of A4 - `off`.
const fn a4(off: usize) -> usize {
    0x8000 - off
}
/// Moongate destinations: 25 entries (0x11d1d4 indexes them by quality - 1).
pub const GATES: usize = 25;

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
    /// Original: A4-0x78f8 (cell-index steps of the same walk, `dy * 40 + dx`)
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
    /// must agree with each other (`step = dy * 40 + dx`).
    pub fn from_game(game: &[u8]) -> Result<MapTables, FormatError> {
        palette(game)?;
        let at = |off: usize, n: usize| &game[HUNK33_DATA + off..HUNK33_DATA + off + n];
        let signed = |off: usize| -> [i8; 8] { std::array::from_fn(|i| at(off, 8)[i] as i8) };
        let words = |off: usize| -> [u16; GATES] {
            let b = at(off, GATES * 2);
            std::array::from_fn(|i| u16::from_be_bytes([b[i * 2], b[i * 2 + 1]]))
        };
        let dist = at(a4(0x794a), 64);
        let t = MapTables {
            dist: std::array::from_fn(|dx| std::array::from_fn(|dy| dist[dx * 8 + dy] as i8)),
            walk_dx: signed(a4(0x7908)),
            walk_dy: signed(a4(0x7900)),
            walk_step: signed(a4(0x78f8)),
            wall: std::array::from_fn(|i| at(a4(0x78f0), 16)[i]),
            gate_x: words(a4(0x6b24)),
            gate_y: words(a4(0x6af2)),
            gate_z: std::array::from_fn(|i| at(a4(0x6ac0), GATES)[i]),
        };
        for i in 0..8 {
            let step = i32::from(t.walk_dy[i]) * WINDOW as i32 + i32::from(t.walk_dx[i]);
            if step != i32::from(t.walk_step[i]) {
                return Err(FormatError::Invalid {
                    what: "game view walk tables",
                    offset: HUNK33_DATA + a4(0x78f8) + i,
                    detail: format!(
                        "step {} != dy * 40 + dx; unknown game version",
                        t.walk_step[i]
                    ),
                });
            }
        }
        Ok(t)
    }
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

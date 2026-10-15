//! [`GfxState`] and its loader 0x152e50.

use u6_formats::FormatError;
use u6_formats::gfx::{AnimMask, AnimOverlay, TILE_COUNT, Tile, decode_tiles};

use super::tables::GameTables;
use crate::files::Files;
use crate::state::LoadError;

/// The world-view graphics state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GfxState {
    /// Original: the tile records 0x152e50 reads (maptiles buffer at 0x152e38; objtiles records
    /// in the cache slots of 0x14f2c8), decoded, indexed by the tile number they were loaded for.
    /// The shore merge 0x152dac rewrites map records in place.
    pub records: Vec<Tile>,
    /// The animmask overlay each record's reference resolves to (0x153002 tail), resolved at load.
    pub overlays: Vec<Option<AnimOverlay>>,
    /// Original: 0x15559e (longword per tile: pointer to its record); here the record index.
    /// 0x152cd2 copies entries, so several tiles can share one record.
    pub record_of: Vec<u16>,
    /// Original: 0x15759e (byte per tile: masktype, 0x152e50 from `masktype.ega`); 0x152cd2
    /// copies entries along with `record_of`.
    pub masktype: Vec<u8>,
    /// Original: 0x152e3c / 0x152e40 (`animmask.ega` block 1 = overlays, block 2 = shore masks)
    pub animmask: AnimMask,
    /// Tables read out of `game`.
    pub tables: GameTables,
    /// Original: 0x15803e (byte: overlay frame 0..7; written only by 0x152dca)
    pub frame: u8,
    /// Original: 0x152174 (word: colour index of the next plot; 0x152176)
    pub plot_colour: u16,
    /// Original: 0x155574 (A4-0x617c; byte: plots XOR their colour instead of setting it)
    pub plot_xor: bool,
    /// Original: 0x155577 (A4-0x6179; byte: plots go to the back buffer instead of the front)
    pub plot_back: bool,
    /// Original: 0x153bf0 (A4-0x7b00; word: animation counter, +1 per 0x10202e; bit 0 of its low
    /// byte A4-0x7aff skips the shore merge)
    pub anim_counter: u16,
    /// Original: 0x153b8c (A4-0x7b64; byte: non-zero skips the animation tick; the clock and the
    /// NPC turn read it too)
    pub anim_paused: u8,
    /// Original: 0x153bf6 (A4-0x7afa; word: actor idle frames due; 0x10202e clears it)
    pub actor_anim_due: u16,
    /// Original: 0x153bf8 (A4-0x7af8; word: the map window is shown; image value from `game`)
    pub window_shown: u16,
    /// Original: 0x153bf4 (A4-0x7afc; byte: panel 0x102ef0 redraws after the window, 0x133a1a)
    pub panel_redraw: u8,
    /// Original: 0x153b8b (A4-0x7b65; byte: 0x102c90 scatters 100 XOR pixels over the window)
    pub sparkle: u8,
    /// Original: 0x155178 (A4-0x6578; word: the next window present dissolves; image value from
    /// `game`)
    pub dissolve: u16,
    /// The turned sprites of the flights (0x15265a / 0x152800 / 0x152876).
    pub sprites: super::sprite::Sprites,
    /// Port-only (spec 06 O2): what the idle refresh last rendered and merged, so it can leave
    /// out a render or a merge that would change nothing. Not game state: it compares equal
    /// whatever it holds, and a clone starts empty ([`super::cache::RenderCache`]).
    pub cache: super::cache::RenderCache,
}

impl Default for GfxState {
    /// Nothing loaded: 2048 empty map records, every tile on its own record, flags clear.
    fn default() -> Self {
        let empty = Tile {
            masktype: 0,
            kind: u6_formats::gfx::TileKind::Map,
            first_row: 16,
            rows: 0,
            pixels: vec![0; 256],
            transparent: vec![true; 256],
            unused_byte: None,
            anim_ref: None,
        };
        Self {
            records: vec![empty; TILE_COUNT],
            overlays: vec![None; TILE_COUNT],
            record_of: (0..TILE_COUNT as u16).collect(),
            masktype: vec![0; TILE_COUNT],
            animmask: AnimMask {
                block1: Vec::new(),
                block2: Vec::new(),
            },
            tables: GameTables::default(),
            frame: 0,
            plot_colour: 0,
            plot_xor: false,
            plot_back: false,
            anim_counter: 0,
            anim_paused: 0,
            actor_anim_due: 0,
            window_shown: 0,
            panel_redraw: 0,
            sparkle: 0,
            dissolve: 0,
            sprites: super::sprite::Sprites::default(),
            cache: super::cache::RenderCache::default(),
        }
    }
}

fn read<'a>(files: &'a Files, name: &str) -> Result<&'a [u8], LoadError> {
    files.get(name).ok_or_else(|| LoadError::Missing {
        file: name.to_string(),
    })
}

fn format_error(file: &str) -> impl FnOnce(FormatError) -> LoadError + '_ {
    move |error| LoadError::Format {
        file: file.to_string(),
        error,
    }
}

impl GfxState {
    /// A state without tile records (no allocation): what the idle pass reads (tables and the
    /// display globals) without the 2048 records.
    pub(crate) fn view(&self) -> GfxState {
        GfxState {
            records: Vec::new(),
            overlays: Vec::new(),
            record_of: Vec::new(),
            masktype: Vec::new(),
            animmask: AnimMask {
                block1: Vec::new(),
                block2: Vec::new(),
            },
            tables: self.tables.clone(),
            sprites: super::sprite::Sprites::none(),
            cache: super::cache::RenderCache::default(),
            ..*self
        }
    }

    /// Original: 0x152e50 (load tile graphics: maptiles, masktype, tileindx, animmask)
    /// Replaces: 0x14f16c (object-tile cache prefill), 0x1623d6 (cache miss: reload a record)
    /// Replaces: 0x1621c0 (read an LZW file's size), 0x161ec8 (the LZW decoder)
    ///
    /// An LZW file is read by 0x1621c0 (its 4-byte little-endian size) and 0x161ec8 (9..12-bit
    /// codes, 0x100 resets, 0x101 ends, read from the file 0x400 bytes at a time);
    /// `u6_formats::lzw` decodes the whole file (here `masktype.ega`, opened with D0 = 1).
    ///
    /// Reads the files in the original order (`maptiles.ega`, `masktype.ega`, `tileindx.ega`,
    /// `animmask.ega`, then the cache prefill from `objtiles.ega`) and decodes all 2048 records
    /// (the tileindx split and the masktype kinds are checked by the decoder). The port keeps every
    /// object record instead of the LRU cache: the cache only changes what is resident, never what
    /// is drawn; the one draw-visible cache effect, 0x152cd2's pointer copy, is `record_of`. Also
    /// reads the tables of [`GameTables`] from `game` and starts the display globals at their
    /// image values (`window_shown`, `dissolve`, `plot_colour` 0, the rest 0).
    pub fn load(files: &Files) -> Result<GfxState, LoadError> {
        let maptiles = read(files, "maptiles.ega")?;
        let masktype = read(files, "masktype.ega")?;
        let tileindx = read(files, "tileindx.ega")?;
        let animmask = read(files, "animmask.ega")?;
        let objtiles = read(files, "objtiles.ega")?;
        let set = decode_tiles(maptiles, objtiles, tileindx, masktype)
            .map_err(format_error("maptiles.ega / objtiles.ega"))?;
        let animmask = AnimMask::parse(animmask).map_err(format_error("animmask.ega"))?;
        let tables = GameTables::from_game(read(files, "game")?).map_err(format_error("game"))?;
        let overlays = set
            .tiles
            .iter()
            .map(|t| t.anim_ref.map(|r| animmask.overlay(r)).transpose())
            .collect::<Result<Vec<_>, _>>()
            .map_err(format_error("animmask.ega"))?;
        Ok(GfxState {
            masktype: set.tiles.iter().map(|t| t.masktype).collect(),
            records: set.tiles,
            overlays,
            animmask,
            window_shown: tables.window_shown,
            dissolve: tables.dissolve,
            tables,
            ..GfxState::default()
        })
    }

    /// The record tile `t` (low 11 bits) currently draws, with its live masktype.
    pub fn record(&self, t: u16) -> (&Tile, Option<&AnimOverlay>, u8) {
        let t = usize::from(t & 0x7ff);
        let r = usize::from(self.record_of[t]);
        (
            &self.records[r],
            self.overlays[r].as_ref(),
            self.masktype[t],
        )
    }
}

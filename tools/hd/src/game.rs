//! What the tool reads from the owner's files: the classic images (the 2048 tile records, the
//! portraits, the paper and spellbook bitmaps), the palette and the tables that group the
//! records. Everything is loaded the way the engine loads it (`State::new_game`, the public
//! API of `u6-core`), at run time; nothing read here is written anywhere but the work dir.
//!
//! [`Game::synthetic`] is a made-up set of the same shape (drawn by code here), for the tests
//! and for tuning the gates with real imagegen calls without sending any game art.

use std::path::Path;

use u6_core::image::{A4, GameImage};
use u6_core::objects::{ACTOR_SLOTS, ObjectId};
use u6_core::{Files, State};
use u6_formats::animdata::AnimEntry;
use u6_formats::bitmap::Bitmap;
use u6_formats::gfx::{TILE_COUNT, TILE_PIXELS, TILE_W, TileKind};
use u6_formats::portrait::{PORTRAIT_HEIGHT, PORTRAIT_LEN, PORTRAIT_WIDTH};
use u6_hdpack::{ImageId, Role, bitmap_hash, portrait_hash, tile_hash};
use u6_intake::{InputFile, intake};

use crate::img::Classic;

/// Original: A4-0x61e2 (pointer to the paper bitmap's file name, read by 0x1350d8) and
/// A4-0x61de (the spellbook picture's, read by 0x116028).
const BITMAP_NAMES: [(Role, u32); 2] = [(Role::Paper, 0x61e2), (Role::Spellbook, 0x61de)];
/// flags2 bits of a tile that has parts (`u6_core::objects::add_object_tiles`, 0x10e73a).
pub const DOUBLE_WIDTH: u8 = 0x80;
pub const DOUBLE_HEIGHT: u8 = 0x40;

/// One tile record as loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// Colour index per pixel (0 where transparent).
    pub pixels: Vec<u8>,
    pub transparent: Vec<bool>,
    /// A map record (opaque rows, `maptiles.ega`).
    pub map: bool,
}

impl Record {
    /// The record as the engine holds it (`GfxState::records`).
    pub fn of(t: &u6_formats::gfx::Tile) -> Record {
        Record {
            pixels: t.pixels.clone(),
            transparent: t.transparent.clone(),
            map: t.kind == TileKind::Map,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.transparent.iter().all(|&t| t)
    }
}

/// The owner's classic images and the tables that group them.
#[derive(Clone, Debug)]
pub struct Game {
    pub palette: Vec<[u8; 3]>,
    /// The 2048 records, by tile number (at load every tile draws its own record).
    pub records: Vec<Record>,
    /// flags2 per tile.
    pub flags2: Vec<u8>,
    /// Base tile per object type.
    pub basetile: Vec<u16>,
    /// Per tile: some map chunk cell is this tile.
    pub in_chunks: Vec<bool>,
    /// The active animdata slots.
    pub anim: Vec<AnimEntry>,
    /// Tiles the shore merge rewrites on every even tick.
    pub shore: Vec<u16>,
    /// Object types an actor can have: the monster types and the actors' types at the start.
    pub actor_types: Vec<u16>,
    /// Screen colour of each 8-bit bitmap colour.
    pub bitmap_colours: Vec<u8>,
    /// (letter, item, 56×64 bytes).
    pub portraits: Vec<(u8, u16, Vec<u8>)>,
    /// (role, width, height, bytes).
    pub bitmaps: Vec<(Role, u16, u16, Vec<u8>)>,
}

/// A failed load: a fixed category for the terminal, and the details (which may name the
/// owner's game files or carry what the engine read) for the 0600 diagnostic file only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadFailure {
    pub category: &'static str,
    pub detail: String,
}

impl LoadFailure {
    fn new(category: &'static str) -> impl FnOnce(String) -> LoadFailure {
        move |detail| LoadFailure { category, detail }
    }
}

/// The owner's files at `path`: a zip, an HFE or ADF image, or a folder of any of them or of
/// extracted files, through the same intake as the page.
fn read_files(path: &Path) -> Result<Files, String> {
    let mut inputs = Vec::new();
    collect(path, path, &mut inputs)?;
    if inputs.is_empty() {
        return Err(format!("{}: no files", path.display()));
    }
    Ok(Files::new(intake(inputs).files))
}

fn collect(root: &Path, path: &Path, out: &mut Vec<InputFile>) -> Result<(), String> {
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for p in entries {
            collect(root, &p, out)?;
        }
        return Ok(());
    }
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let rel = path.strip_prefix(root).unwrap_or(path);
    let name = if rel.as_os_str().is_empty() {
        path.file_name()
            .map_or("input".into(), |n| n.to_string_lossy().into_owned())
    } else {
        rel.to_string_lossy().into_owned()
    };
    out.push(InputFile { name, data });
    Ok(())
}

impl Game {
    /// Loads the owner's files.
    pub fn load(path: &Path) -> Result<Game, LoadFailure> {
        let files =
            read_files(path).map_err(LoadFailure::new("the game files could not be read"))?;
        let state = State::new_game(&files)
            .map_err(|e| e.to_string())
            .map_err(LoadFailure::new("the game files did not load as Ultima VI"))?;
        let palette = state
            .tables
            .palette
            .iter()
            .map(|c| [c.r, c.g, c.b])
            .collect();
        let records = state.gfx.records.iter().map(Record::of).collect();
        let mut in_chunks = vec![false; TILE_COUNT];
        for chunk in &state.map.chunks.chunks {
            for row in chunk {
                for &t in row {
                    in_chunks[usize::from(t)] = true;
                }
            }
        }
        let anim = state
            .tables
            .anim
            .slots
            .iter()
            .take(usize::from(state.tables.anim.count))
            .copied()
            .collect();
        // The merge's words are offsets into the record pointer table (`gfx::anim`, 0x152d52).
        let shore = state
            .gfx
            .tables
            .shore_dst
            .iter()
            .filter(|w| w.is_multiple_of(4) && usize::from(**w / 4) < TILE_COUNT)
            .map(|w| w / 4)
            .collect();
        let mut actor_types: Vec<u16> = state
            .objects
            .defs
            .monster_types
            .iter()
            .copied()
            .take_while(|&t| t != 0)
            .chain((0..ACTOR_SLOTS as u16).map(|i| state.objects.obj_type(ObjectId(i))))
            .filter(|&t| t != 0)
            .collect();
        actor_types.sort_unstable();
        actor_types.dedup();
        let game = files.get("game").ok_or_else(|| {
            LoadFailure::new("the game files did not load as Ultima VI")("no executable".into())
        })?;
        Ok(Game {
            palette,
            records,
            flags2: state.tables.tileflags.flags2.clone(),
            basetile: state.tables.basetile.base.clone(),
            in_chunks,
            anim,
            shore,
            actor_types,
            bitmap_colours: state.gfx.tables.bitmap_colours.to_vec(),
            portraits: portraits(&state, &files),
            bitmaps: bitmaps(&files, game),
        })
    }

    /// The classic image of `id`, if the game has one.
    pub fn classic(&self, id: ImageId) -> Option<Classic> {
        let colour = |i: u8| self.palette.get(usize::from(i)).copied().unwrap_or([0; 3]);
        let bitmap = |w: u16, h: u16, bytes: &[u8]| Classic {
            width: u32::from(w),
            height: u32::from(h),
            px: bytes
                .iter()
                .map(|&b| Some(colour(self.bitmap_colours[usize::from(b)] & 0x1f)))
                .collect(),
        };
        match id {
            ImageId::Tile(r) => {
                let rec = self.records.get(usize::from(r))?;
                Some(Classic {
                    width: TILE_W as u32,
                    height: TILE_W as u32,
                    px: rec
                        .pixels
                        .iter()
                        .zip(&rec.transparent)
                        .map(|(&p, &t)| (!t).then(|| colour(p)))
                        .collect(),
                })
            }
            ImageId::Portrait { letter, item } => {
                let (_, _, b) = self
                    .portraits
                    .iter()
                    .find(|(l, i, _)| *l == letter && *i == item)?;
                Some(bitmap(PORTRAIT_WIDTH as u16, PORTRAIT_HEIGHT as u16, b))
            }
            ImageId::Bitmap(role) => {
                let (_, w, h, b) = self.bitmaps.iter().find(|(r, ..)| *r == role)?;
                Some(bitmap(*w, *h, b))
            }
        }
    }

    /// The content hash of `id`'s classic image (`u6_hdpack`'s, as the engine will compute it).
    pub fn hash(&self, id: ImageId) -> Option<u64> {
        match id {
            ImageId::Tile(r) => {
                let rec = self.records.get(usize::from(r))?;
                tile_hash(&rec.pixels, &rec.transparent)
            }
            ImageId::Portrait { letter, item } => {
                let (_, _, b) = self
                    .portraits
                    .iter()
                    .find(|(l, i, _)| *l == letter && *i == item)?;
                portrait_hash(b)
            }
            ImageId::Bitmap(role) => {
                let (_, w, h, b) = self.bitmaps.iter().find(|(r, ..)| *r == role)?;
                bitmap_hash(*w, *h, b)
            }
        }
    }
}

/// Every portrait of the three archives, its pixels as the engine reads them
/// (`u6_core::converse::portraits`, `portrait_pixels`: the loading half of 0x133d3e).
pub fn portraits(state: &State, files: &Files) -> Vec<(u8, u16, Vec<u8>)> {
    u6_core::converse::portraits(state, files)
}

/// The paper and spellbook bitmaps, by the file names `game` points to.
fn bitmaps(files: &Files, game: &[u8]) -> Vec<(Role, u16, u16, Vec<u8>)> {
    let Ok(img) = GameImage::parse(game) else {
        return Vec::new();
    };
    BITMAP_NAMES
        .iter()
        .filter_map(|&(role, off)| {
            let name = img.pointer(A4.wrapping_sub(off)).and_then(|p| img.cstr(p));
            let name = String::from_utf8_lossy(&name.ok()?).into_owned();
            let bmp = Bitmap::from_lzw(files.get(&name)?).ok()?;
            Some((role, bmp.width, bmp.height, bmp.pixels))
        })
        .collect()
}

/// The synthetic set: made-up pixel art drawn here, in the shapes the real files have.
pub mod synthetic {
    use super::*;

    /// The made-up palette: 32 colours.
    pub const PALETTE: [[u8; 3]; 32] = [
        [0, 0, 0],
        [24, 24, 32],
        [64, 64, 72],
        [112, 112, 120],
        [168, 168, 176],
        [232, 232, 236],
        [40, 96, 40],
        [64, 136, 48],
        [104, 176, 64],
        [24, 56, 120],
        [40, 96, 168],
        [88, 152, 208],
        [96, 64, 32],
        [144, 96, 48],
        [192, 144, 88],
        [232, 208, 144],
        [128, 32, 32],
        [192, 56, 40],
        [232, 120, 64],
        [248, 200, 64],
        [216, 168, 136],
        [176, 120, 96],
        [72, 40, 88],
        [128, 80, 152],
        [32, 72, 72],
        [56, 128, 120],
        [160, 64, 96],
        [120, 104, 64],
        [80, 72, 48],
        [200, 192, 160],
        [48, 32, 24],
        [248, 248, 248],
    ];
    pub const TERRAIN: [u16; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
    pub const WATER_FRAMES: u16 = 64;
    pub const SHORE: u16 = 11;
    pub const ACTOR_A: u16 = 128;
    /// Actor types (constants of the original's face routine, `group::actor_layout`): four
    /// facings of two frames, and four facings of one.
    pub const WALKER: u16 = 0x15a;
    pub const FACING_ONLY: u16 = 0x162;
    pub const ACTOR_B: u16 = 136;
    pub const TREE: u16 = 144;
    pub const TABLE: u16 = 148;
    pub const BANNER: u16 = 150;
    pub const OBJECTS: u16 = 152;
    pub const LEFT_HALF: u16 = 155;
    pub const END: u16 = 157;

    fn hash(x: u32, y: u32, s: u32) -> u32 {
        let mut h =
            x.wrapping_mul(0x9e37_79b9) ^ y.wrapping_mul(0x85eb_ca6b) ^ s.wrapping_mul(0xc2b2_ae35);
        h ^= h >> 15;
        h = h.wrapping_mul(0x2c1b_3c6d);
        h ^ (h >> 12)
    }

    fn rec(f: impl Fn(usize, usize) -> Option<u8>, map: bool) -> Record {
        let mut pixels = vec![0; TILE_PIXELS];
        let mut transparent = vec![true; TILE_PIXELS];
        for y in 0..TILE_W {
            for x in 0..TILE_W {
                if let Some(c) = f(x, y) {
                    pixels[y * TILE_W + x] = c;
                    transparent[y * TILE_W + x] = false;
                }
            }
        }
        Record {
            pixels,
            transparent,
            map,
        }
    }

    fn terrain(t: u16) -> Record {
        let s = u32::from(t);
        rec(
            |x, y| {
                let (xu, yu) = (x as u32, y as u32);
                let n = hash(xu, yu, s) % 8;
                Some(match t {
                    0 => [7, 7, 6, 8, 7, 7, 6, 7][n as usize],
                    1 | 64..=67 => {
                        let phase = if t >= 64 { (t - 64) as usize } else { 0 };
                        if (x + 2 * y + phase * 2) % 8 < 2 {
                            11
                        } else {
                            10
                        }
                    }
                    2 => {
                        let row = y / 4;
                        let off = if row % 2 == 0 { 0 } else { 4 };
                        if y % 4 == 3 || (x + off) % 8 == 7 {
                            3
                        } else {
                            17 - (n == 0) as u8
                        }
                    }
                    3 => [3, 4, 3, 2, 3, 4, 4, 3][n as usize],
                    4 => [13, 12, 13, 14, 13, 13, 12, 13][n as usize],
                    5 => {
                        if y % 5 == 4 {
                            12
                        } else if x == 7 && y % 10 < 5 {
                            30
                        } else {
                            14
                        }
                    }
                    6 => {
                        if (y + x / 2) % 4 == 0 {
                            16
                        } else {
                            17
                        }
                    }
                    7 => [15, 14, 15, 15, 29, 15, 14, 15][n as usize],
                    8 => {
                        if (x / 4 + y / 4) % 2 == 0 {
                            4
                        } else {
                            3
                        }
                    }
                    9 => {
                        if (x * x + y * y) % 17 < 3 {
                            25
                        } else {
                            24
                        }
                    }
                    10 => {
                        if x % 8 == 0 || y % 8 == 0 {
                            2
                        } else {
                            28
                        }
                    }
                    _ => {
                        if x + y < 16 {
                            15
                        } else {
                            10
                        }
                    }
                })
            },
            true,
        )
    }

    /// A walking figure: `dir` 0..4 turns the face, `step` 0/1 moves the legs one pixel.
    pub fn figure(dir: usize, step: usize, body: u8) -> Record {
        rec(
            |x, y| {
                let (x, y) = (x as i32, y as i32);
                let head = (x - 8).pow(2) + (y - 3).pow(2) <= 5;
                let eye = y == 3
                    && match dir {
                        0 => x == 7 || x == 9,
                        1 => x == 10,
                        2 => false,
                        _ => x == 6,
                    };
                let torso = (5..=10).contains(&y) && (6..=10).contains(&x);
                let arm = (6..=9).contains(&y) && (x == 5 || x == 11);
                let lift = if step == 0 { 0 } else { 1 };
                let leg_l = (11..=14 - lift).contains(&y) && x == 7;
                let leg_r = (11..=14 - (1 - lift)).contains(&y) && x == 9;
                if eye {
                    Some(0)
                } else if head {
                    Some(20)
                } else if torso {
                    Some(body)
                } else if arm {
                    Some(21)
                } else if leg_l || leg_r {
                    Some(30)
                } else {
                    None
                }
            },
            false,
        )
    }

    /// A wide creature in two parts (left, right) for frame `step`.
    fn beast(part: usize, step: usize) -> Record {
        rec(
            |x, y| {
                let gx = (x + part * 16) as i32;
                let y = y as i32;
                let body = (gx - 16).pow(2) / 4 + (y - 9).pow(2) <= 22;
                let head = (gx - 27).pow(2) + (y - 6).pow(2) <= 6;
                let legs = (12..=15).contains(&y)
                    && [8, 13, 19, 24]
                        .iter()
                        .any(|&lx| gx == lx + if step == 1 && lx % 2 == 0 { 1 } else { 0 });
                if head && gx == 28 && y == 5 {
                    Some(0)
                } else if body || head {
                    Some(23)
                } else if legs {
                    Some(22)
                } else {
                    None
                }
            },
            false,
        )
    }

    fn tree(part: usize) -> Record {
        // Parts: 0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right (anchor).
        rec(
            |x, y| {
                let gx = (x + (part % 2) * 16) as i32;
                let gy = (y + (part / 2) * 16) as i32;
                let crown = (gx - 16).pow(2) + (gy - 12).pow(2) <= 120;
                let trunk = (14..=17).contains(&gx) && gy >= 20;
                if crown {
                    Some(if hash(gx as u32, gy as u32, 9).is_multiple_of(5) {
                        8
                    } else {
                        6
                    })
                } else if trunk {
                    Some(12)
                } else {
                    None
                }
            },
            false,
        )
    }

    fn table(part: usize) -> Record {
        rec(
            |x, y| {
                let gx = x + part * 16;
                if (6..=8).contains(&y) && (2..30).contains(&gx) {
                    Some(14)
                } else if y > 8 && (gx == 4 || gx == 27) {
                    Some(12)
                } else {
                    None
                }
            },
            false,
        )
    }

    fn banner(part: usize) -> Record {
        rec(
            |x, y| {
                let gy = y + part * 16;
                if x == 3 {
                    Some(12)
                } else if (4..13).contains(&x) && (2..20).contains(&gy) {
                    Some(if gy % 6 < 3 { 17 } else { 19 })
                } else {
                    None
                }
            },
            false,
        )
    }

    fn object(n: u16) -> Record {
        rec(
            |x, y| {
                let (xi, yi) = (x as i32, y as i32);
                match n {
                    // A chest.
                    0 => {
                        let c = if y == 9 || x == 3 || x == 12 {
                            12
                        } else if x == 8 && y == 10 {
                            19
                        } else {
                            13
                        };
                        ((3..13).contains(&x) && (6..14).contains(&y)).then_some(c)
                    }
                    // A potion.
                    1 => {
                        let bulb = (xi - 8).pow(2) + (yi - 10).pow(2) <= 12;
                        let neck = (7..=8).contains(&x) && (3..7).contains(&y);
                        (bulb || neck).then_some(if neck {
                            4
                        } else if yi < 9 {
                            18
                        } else {
                            17
                        })
                    }
                    // A sword.
                    2 => (x + y == 17 && (3..14).contains(&x))
                        .then_some(4)
                        .or(((x + y == 18 || x + y == 16) && (3..7).contains(&x)).then_some(19)),
                    // An 8-wide left half and its right half.
                    3 => (x < 8 && (2..14).contains(&y) && (x + y) % 3 != 0).then_some(25),
                    _ => (x >= 8 && (2..14).contains(&y) && (x + y) % 3 != 1).then_some(26),
                }
            },
            false,
        )
    }

    fn portrait(seed: u8) -> Vec<u8> {
        let mut out = vec![0u8; PORTRAIT_LEN];
        for y in 0..PORTRAIT_HEIGHT {
            for x in 0..PORTRAIT_WIDTH {
                let (xi, yi) = (x as i32, y as i32);
                // Turned a little to the right, the hair falling on the left, a clasp on the
                // right shoulder: no portrait is its own mirror image.
                let face = (xi - 30).pow(2) * 4 / 3 + (yi - 30).pow(2) <= 300;
                let hair =
                    (xi - 29).pow(2) + (yi - 18).pow(2) <= 280 && (yi < 24 || (xi < 17 && yi < 44));
                let eye = yi == 28 && (xi == 24 || xi == 25 || xi == 36 || xi == 37);
                let mouth = yi == 41 && (28..36).contains(&xi);
                let shoulders = yi > 50 && (xi - 30).abs() < 26;
                let clasp = (xi - 44).pow(2) + (yi - 56).pow(2) <= 6;
                out[y * PORTRAIT_WIDTH + x] = if eye {
                    0
                } else if mouth || clasp {
                    16
                } else if hair {
                    if seed == 0 { 12 } else { 19 }
                } else if face {
                    20
                } else if shoulders {
                    if seed == 0 { 10 } else { 23 }
                } else {
                    2 + ((x + y) / 8 % 2) as u8
                };
            }
        }
        out
    }

    fn panel(w: u16, h: u16, seed: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(usize::from(w) * usize::from(h));
        for y in 0..h {
            for x in 0..w {
                let border = x < 2 || y < 2 || x >= w - 2 || y >= h - 2;
                let line =
                    y % 6 == 3 && (6..w - 6).contains(&x) && (x / 3 + u16::from(seed)) % 7 != 0;
                out.push(if border {
                    13
                } else if line {
                    30
                } else {
                    15
                });
            }
        }
        out
    }

    /// The synthetic game.
    pub fn game() -> Game {
        let empty = rec(|_, _| None, true);
        let mut records = vec![empty; TILE_COUNT];
        for t in TERRAIN {
            records[usize::from(t)] = terrain(t);
        }
        for f in 0..4 {
            records[usize::from(WATER_FRAMES + f)] = terrain(WATER_FRAMES + f);
        }
        for d in 0..4 {
            for s in 0..2 {
                records[usize::from(ACTOR_A) + d * 2 + s] = figure(d, s, 10);
            }
        }
        for s in 0..2 {
            for p in 0..2 {
                records[usize::from(ACTOR_B) + s * 2 + p] = beast(p, s);
            }
        }
        for p in 0..4 {
            records[usize::from(TREE) + p] = tree(p);
        }
        for p in 0..2 {
            records[usize::from(TABLE) + p] = table(p);
            records[usize::from(BANNER) + p] = banner(p);
        }
        for n in 0..5 {
            records[usize::from(OBJECTS + n)] = object(n);
        }
        let mut flags2 = vec![0u8; TILE_COUNT];
        flags2[usize::from(ACTOR_B) + 1] = DOUBLE_WIDTH;
        flags2[usize::from(ACTOR_B) + 3] = DOUBLE_WIDTH;
        flags2[usize::from(TREE) + 3] = DOUBLE_WIDTH | DOUBLE_HEIGHT;
        flags2[usize::from(TABLE) + 1] = DOUBLE_WIDTH;
        flags2[usize::from(BANNER) + 1] = DOUBLE_HEIGHT;
        let mut basetile = vec![END; 1024];
        let bases = [
            TREE,
            TABLE,
            BANNER,
            OBJECTS,
            OBJECTS + 1,
            OBJECTS + 2,
            LEFT_HALF,
        ];
        // Type 0 is no object, as in the files; the objects' types start at 1. The actors have
        // types of known frame layouts (group::actor_layout): a two-frame walker and a
        // facing-only creature.
        basetile[1..=bases.len()].copy_from_slice(&bases);
        basetile[usize::from(WALKER)] = ACTOR_A;
        basetile[usize::from(FACING_ONLY)] = ACTOR_B;
        let mut in_chunks = vec![false; TILE_COUNT];
        for t in TERRAIN {
            in_chunks[usize::from(t)] = true;
        }
        Game {
            palette: PALETTE.to_vec(),
            records,
            flags2,
            basetile,
            in_chunks,
            anim: vec![AnimEntry {
                tile_to_animate: 1,
                first_anim_frame: WATER_FRAMES,
                and_mask: 3,
                shift: 0,
            }],
            shore: vec![SHORE],
            actor_types: vec![WALKER, FACING_ONLY],
            bitmap_colours: (0..=255u8).map(|b| b & 0x1f).collect(),
            portraits: vec![(b'a', 0, portrait(0)), (b'a', 1, portrait(1))],
            bitmaps: vec![
                (Role::Paper, 64, 40, panel(64, 40, 0)),
                (Role::Spellbook, 48, 32, panel(48, 32, 3)),
            ],
        }
    }
}

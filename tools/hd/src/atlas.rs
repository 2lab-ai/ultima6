//! Sheets: the images one imagegen call repaints, and `jobs.jsonl`, one line per sheet with its
//! cell map (which image sits where, at which scale). The jsonl holds ids, positions, content
//! hashes and paths, no art.
//!
//! Layouts, all on the 1536 × 1024 sheet the built-in image tool returns (r1 §2.1):
//! - tiles (actors, multi-record objects, objects): 12 × 8 cells of 128 px (×8), no gutters;
//!   an item's pieces go in one sheet, a mosaic as adjacent cells;
//! - terrain: 8 × 5 cells of 192 px, each the tile (128 px) inside a border of 4 source pixels
//!   that wraps it around (r1 run C), so the repaint continues across its own edges;
//! - portraits: 448 × 512 (×8), 3 × 2 per sheet;
//! - a bitmap alone, at the largest whole scale up to 8 that fits (ingest resamples to ×8).
//!
//! The rest of a sheet, and every transparent pixel, is the sheet's key colour: of
//! [`KEY_COLOURS`], the one farthest from every colour the sheet's images use.

use serde::{Deserialize, Serialize};
use u6_formats::portrait::{PORTRAIT_HEIGHT, PORTRAIT_WIDTH};
use u6_hdpack::{ImageId, Role};

use crate::game::Game;
use crate::group::{Class, Item};
use crate::img::{self, Classic, Rgba};

/// The scale the sheets are drawn and judged at, and of the masters ingest keeps (×8, r1 §2.3:
/// the best grid fit); `pack` writes them at the pack's ×4.
pub const MASTER: u32 = 8;
pub const SHEET_W: u32 = 1536;
pub const SHEET_H: u32 = 1024;
pub const CELL: u32 = 128;
pub const GRID: (u32, u32) = (SHEET_W / CELL, SHEET_H / CELL);
/// Wrap border of a terrain cell, in source pixels.
pub const BORDER: u32 = 4;
pub const TERRAIN_CELL: u32 = CELL + 2 * BORDER * MASTER;
pub const TERRAIN_GRID: (u32, u32) = (SHEET_W / TERRAIN_CELL, SHEET_H / TERRAIN_CELL);
pub const PORTRAIT: (u32, u32) = (
    PORTRAIT_WIDTH as u32 * MASTER,
    PORTRAIT_HEIGHT as u32 * MASTER,
);
pub const PORTRAIT_GRID: (u32, u32) = (SHEET_W / PORTRAIT.0, SHEET_H / PORTRAIT.1);

/// Key candidates: saturated colours, magenta and green first (the imagegen skill's keys).
pub const KEY_COLOURS: [[u8; 3]; 12] = [
    [255, 0, 255],
    [0, 255, 0],
    [0, 255, 255],
    [255, 255, 0],
    [255, 0, 0],
    [0, 0, 255],
    [255, 128, 0],
    [128, 255, 0],
    [0, 255, 128],
    [0, 128, 255],
    [128, 0, 255],
    [255, 0, 128],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SheetKind {
    Portrait,
    Bitmap,
    Terrain,
    Tile,
}

impl SheetKind {
    /// The fixed prompt of the kind (`crate::prompt`).
    pub fn prompt_id(self) -> &'static str {
        match self {
            SheetKind::Portrait => "portrait/1",
            SheetKind::Bitmap => "bitmap/1",
            SheetKind::Terrain => "terrain/1",
            SheetKind::Tile => "tile/1",
        }
    }
}

/// One image on a sheet: where its ×`scale` pixels start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub item: u32,
    pub class: Class,
    /// The piece of the item it belongs to (a mosaic is one piece).
    pub piece: u32,
    pub image: String,
    /// The content hash of the classic image, hex.
    pub hash: String,
    pub x: u32,
    pub y: u32,
    /// The classic image's size; on the sheet it is `w·scale × h·scale`.
    pub w: u32,
    pub h: u32,
    pub scale: u32,
}

/// One line of `jobs.jsonl`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// The sheet's id: `sheets/<id>.png` is sent, `out/<id>.png` is what came back.
    pub sheet: String,
    /// The atlas generation (`atlas.json`): a rebuilt atlas has a new one, and outputs made
    /// for another generation are not this job's ([`crate::run::identity`]).
    pub generation: String,
    /// The fingerprint of the plan the item numbers refer to ([`crate::group::Plan::fingerprint`]).
    pub plan: String,
    pub kind: SheetKind,
    /// 1 for the first call, 2 for the re-roll.
    pub attempt: u8,
    pub prompt: String,
    pub key: [u8; 3],
    pub width: u32,
    pub height: u32,
    pub places: Vec<Place>,
}

pub fn image_name(id: ImageId) -> String {
    match id {
        ImageId::Tile(r) => format!("t:{r}"),
        ImageId::Portrait { letter, item } => format!("p:{}:{item}", char::from(letter)),
        ImageId::Bitmap(role) => format!("b:{}", role.name()),
    }
}

pub fn parse_image(s: &str) -> Option<ImageId> {
    let mut parts = s.split(':');
    let id = match (parts.next()?, parts.next()?, parts.next()) {
        ("t", r, None) => ImageId::Tile(r.parse().ok()?),
        ("p", l, Some(i)) if l.len() == 1 => ImageId::Portrait {
            letter: l.as_bytes()[0],
            item: i.parse().ok()?,
        },
        ("b", role, None) => ImageId::Bitmap(Role::ALL.into_iter().find(|r| r.name() == role)?),
        _ => return None,
    };
    parts.next().is_none().then_some(id)
}

/// The file stem of an image in `slices/`.
pub fn slice_name(id: ImageId) -> String {
    image_name(id).replace(':', "-")
}

fn sheet_kind(class: Class) -> SheetKind {
    match class {
        Class::Portrait => SheetKind::Portrait,
        Class::Bitmap => SheetKind::Bitmap,
        Class::Terrain => SheetKind::Terrain,
        Class::Actor | Class::Multi | Class::Object => SheetKind::Tile,
    }
}

/// The key of a sheet whose images use `colours`: the candidate whose nearest used colour is
/// farthest (squared RGB distance); ties go to the earlier candidate.
pub fn choose_key(colours: impl IntoIterator<Item = [u8; 3]>) -> [u8; 3] {
    let mut used: Vec<[u8; 3]> = colours.into_iter().collect();
    used.sort_unstable();
    used.dedup();
    let d = |a: [u8; 3], b: [u8; 3]| -> i32 {
        (0..3)
            .map(|i| (i32::from(a[i]) - i32::from(b[i])).pow(2))
            .sum()
    };
    let mut best = KEY_COLOURS[0];
    let mut best_d = -1;
    for k in KEY_COLOURS {
        let near = used.iter().map(|&c| d(k, c)).min().unwrap_or(i32::MAX);
        if near > best_d {
            best = k;
            best_d = near;
        }
    }
    best
}

/// A sheet being filled.
struct Draft {
    kind: SheetKind,
    places: Vec<(Place, ImageId)>,
    /// Tile and terrain sheets: occupied cells.
    used: Vec<bool>,
}

impl Draft {
    fn new(kind: SheetKind) -> Draft {
        let cells = match kind {
            SheetKind::Tile => GRID.0 * GRID.1,
            SheetKind::Terrain => TERRAIN_GRID.0 * TERRAIN_GRID.1,
            SheetKind::Portrait => PORTRAIT_GRID.0 * PORTRAIT_GRID.1,
            SheetKind::Bitmap => 1,
        };
        Draft {
            kind,
            places: Vec::new(),
            used: vec![false; cells as usize],
        }
    }

    fn cols(&self) -> u32 {
        match self.kind {
            SheetKind::Tile => GRID.0,
            SheetKind::Terrain => TERRAIN_GRID.0,
            SheetKind::Portrait => PORTRAIT_GRID.0,
            SheetKind::Bitmap => 1,
        }
    }

    /// The first free `w × h` block of cells, scanning rows top down.
    fn find(&self, w: u32, h: u32) -> Option<(u32, u32)> {
        let cols = self.cols();
        let rows = self.used.len() as u32 / cols;
        for r in 0..rows.saturating_sub(h - 1) {
            for c in 0..cols.saturating_sub(w - 1) {
                let free = (0..h)
                    .all(|dy| (0..w).all(|dx| !self.used[((r + dy) * cols + c + dx) as usize]));
                if free {
                    return Some((c, r));
                }
            }
        }
        None
    }

    fn mark(&mut self, c: u32, r: u32, w: u32, h: u32) {
        let cols = self.cols();
        for dy in 0..h {
            for dx in 0..w {
                self.used[((r + dy) * cols + c + dx) as usize] = true;
            }
        }
    }
}

/// Places piece `k` of `item` in the first free block of `draft`.
fn place_piece(draft: &mut Draft, game: &Game, item_no: u32, item: &Item, k: usize) -> bool {
    let piece = &item.pieces[k];
    let Some((c, r)) = draft.find(piece.w, piece.h) else {
        return false;
    };
    draft.mark(c, r, piece.w, piece.h);
    for (i, &id) in piece.images.iter().enumerate() {
        let (dx, dy) = (i as u32 % piece.w, i as u32 / piece.w);
        let classic = game.classic(id).expect("planned images exist");
        let (x, y, scale) = match draft.kind {
            SheetKind::Tile => ((c + dx) * CELL, (r + dy) * CELL, MASTER),
            SheetKind::Terrain => (
                c * TERRAIN_CELL + BORDER * MASTER,
                r * TERRAIN_CELL + BORDER * MASTER,
                MASTER,
            ),
            SheetKind::Portrait => (c * PORTRAIT.0, r * PORTRAIT.1, MASTER),
            SheetKind::Bitmap => (0, 0, bitmap_scale(&classic)),
        };
        let hash = game.hash(id).expect("planned images exist");
        draft.places.push((
            Place {
                item: item_no,
                class: item.class,
                piece: k as u32,
                image: image_name(id),
                hash: format!("{hash:016x}"),
                x,
                y,
                w: classic.width,
                h: classic.height,
                scale,
            },
            id,
        ));
    }
    true
}

/// The sheet scale of a bitmap: the largest whole one up to 8 that fits.
pub fn bitmap_scale(c: &Classic) -> u32 {
    (SHEET_W / c.width).min(SHEET_H / c.height).clamp(1, MASTER)
}

/// Places every piece of `item` in `draft`, or none.
fn place_all(draft: &mut Draft, game: &Game, item_no: u32, item: &Item) -> bool {
    let (n, used) = (draft.places.len(), draft.used.clone());
    for k in 0..item.pieces.len() {
        if !place_piece(draft, game, item_no, item, k) {
            draft.places.truncate(n);
            draft.used = used;
            return false;
        }
    }
    true
}

/// Lays out `items` (with their item numbers): the sheets' drafts. An item goes whole into one
/// sheet (a new one when it does not fit the current one); only an item larger than a sheet is
/// split, piece by piece. A bitmap is alone on its sheet.
fn drafts(game: &Game, items: &[(u32, &Item)]) -> Vec<Draft> {
    let mut drafts: Vec<Draft> = Vec::new();
    for kind in [
        SheetKind::Portrait,
        SheetKind::Bitmap,
        SheetKind::Terrain,
        SheetKind::Tile,
    ] {
        let mut current = Draft::new(kind);
        for &(no, item) in items.iter().filter(|(_, i)| sheet_kind(i.class) == kind) {
            if kind == SheetKind::Bitmap && !current.places.is_empty() {
                drafts.push(std::mem::replace(&mut current, Draft::new(kind)));
            }
            if place_all(&mut current, game, no, item) {
                continue;
            }
            if !current.places.is_empty() {
                drafts.push(std::mem::replace(&mut current, Draft::new(kind)));
                if place_all(&mut current, game, no, item) {
                    continue;
                }
            }
            for k in 0..item.pieces.len() {
                if !place_piece(&mut current, game, no, item, k) {
                    drafts.push(std::mem::replace(&mut current, Draft::new(kind)));
                    // A piece is at most 2 × 2 cells: an empty sheet takes it.
                    place_piece(&mut current, game, no, item, k);
                }
            }
        }
        drafts.push(current);
    }
    drafts.retain(|d| !d.places.is_empty());
    drafts
}

/// The sheets of `items` for `attempt`, ids from `first`, rendered.
/// A new atlas generation: the clock and the process, digested.
pub fn new_generation() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    crate::work::digest(&[&now.to_le_bytes(), &std::process::id().to_le_bytes()])
}

/// What every job of one layout carries: the attempt, the atlas generation, the plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub attempt: u8,
    pub generation: String,
    pub plan: String,
}

pub fn layout(
    game: &Game,
    items: &[(u32, &Item)],
    stamp: &Stamp,
    first: usize,
) -> Vec<(Job, Rgba)> {
    drafts(game, items)
        .into_iter()
        .enumerate()
        .map(|(n, d)| render(game, d, stamp, first + n))
        .collect()
}

/// As [`layout`], handing each sheet to `f` as it is rendered (one sheet in memory at a time).
pub fn layout_each(
    game: &Game,
    items: &[(u32, &Item)],
    stamp: &Stamp,
    first: usize,
    mut f: impl FnMut(Job, Rgba) -> Result<(), String>,
) -> Result<(), String> {
    for (n, d) in drafts(game, items).into_iter().enumerate() {
        let (job, sheet) = render(game, d, stamp, first + n);
        f(job, sheet)?;
    }
    Ok(())
}

/// Sheets per kind for `items`, without rendering them.
pub fn sheet_counts(game: &Game, items: &[(u32, &Item)]) -> [(SheetKind, usize); 4] {
    let d = drafts(game, items);
    [
        SheetKind::Portrait,
        SheetKind::Bitmap,
        SheetKind::Terrain,
        SheetKind::Tile,
    ]
    .map(|k| (k, d.iter().filter(|x| x.kind == k).count()))
}

fn render(game: &Game, d: Draft, stamp: &Stamp, n: usize) -> (Job, Rgba) {
    let attempt = stamp.attempt;
    let classics: Vec<Classic> = d
        .places
        .iter()
        .map(|(_, id)| game.classic(*id).expect("planned images exist"))
        .collect();
    let key = choose_key(classics.iter().flat_map(|c| c.px.iter().flatten().copied()));
    let mut sheet = Rgba::new(SHEET_W, SHEET_H);
    img::fill(
        &mut sheet,
        0,
        0,
        SHEET_W,
        SHEET_H,
        [key[0], key[1], key[2], 255],
    );
    for ((p, _), c) in d.places.iter().zip(&classics) {
        if d.kind == SheetKind::Terrain {
            sheet.paste(
                &wrapped(c, key),
                p.x - BORDER * MASTER,
                p.y - BORDER * MASTER,
            );
        } else {
            sheet.paste(&c.scaled(p.scale, key), p.x, p.y);
        }
    }
    let job = Job {
        sheet: format!("s{n:04}-a{attempt}"),
        generation: stamp.generation.clone(),
        plan: stamp.plan.clone(),
        kind: d.kind,
        attempt,
        prompt: d.kind.prompt_id().into(),
        key,
        width: SHEET_W,
        height: SHEET_H,
        places: d.places.into_iter().map(|(p, _)| p).collect(),
    };
    (job, sheet)
}

/// A tile with its wrap border, ×8: source pixel (x, y) of the cell is the tile's
/// ((x − 4) mod 16, (y − 4) mod 16).
pub fn wrapped(c: &Classic, key: [u8; 3]) -> Rgba {
    let side = c.width + 2 * BORDER;
    let mut src = Classic {
        width: side,
        height: side,
        px: Vec::with_capacity((side * side) as usize),
    };
    for y in 0..side {
        for x in 0..side {
            let tx = (x + c.width - BORDER) % c.width;
            let ty = (y + c.height - BORDER) % c.height;
            src.px.push(c.at(tx, ty));
        }
    }
    src.scaled(MASTER, key)
}

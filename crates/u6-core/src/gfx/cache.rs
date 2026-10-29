//! Spec 06 O2: the idle refresh 0x102ef0 leaves out drawing that would change nothing, with the
//! image and the state as without it.
//!
//! The map window render 0x102c90 ([`super::view::render_window`]) draws into the back buffer
//! only in 16 × 16 boxes at multiples of 16 from the map's corner: each cell's tile and objects
//! (and a clock's hands) at ([`MAP_X`] + 16·col, [`MAP_Y`] + 16·row), the target cursor at its
//! cell, each border tile at an edge cell. The
//! blitter writes a record's 16 × 16 pixels from the box's byte address, the overlay's rows below
//! its first row, the outline 8 pixels further left within the record ([`super::blit`]); so
//! every draw stays in its box, which [`WindowKey::of`] checks where the data decides it (the
//! overlay's rows, the clock hands, the cursor's cell). And it draws with set and OR operations
//! only (the opaque pixels, the overlay OR, the outline, the hands in set mode); only the
//! sparkle XORs. A box's draws run again over their own result with the same inputs therefore
//! change no pixel, and boxes do not touch each other: a box is redrawn only when one of the
//! values its draws read changed ([`WindowKey`]), and none at all when none did. That holds while
//! the back buffer is the one the last render drew into and nothing drew on it since
//! ([`crate::engine::Io::back_mark`]). The values: per word drawn, the word, the record and the
//! masktype it draws (the animation's substitutions change them), the record's version (the
//! shore merge changes its pixels, [`RenderCache::record_changed`]), with an overlay the frame
//! it draws (the overlay frame steps at every idle poll; frames with the same rows count as
//! one), for a clock the hour and the minute; and the clock's tile word. What the render does
//! besides still runs: the palettes, the cursor's side effects, the plot colour and plane the
//! clock hands leave, and the present (the window rect's rows copied where the screen's
//! differ, which is what [`super::blit::copy_window`] would change). Never with the sparkle (it
//! draws from the RNG), the dissolve (it clears its flag) or XOR plots on.
//!
//! Tables the render also reads are loaded once and never written:
//! [`super::tables::GameTables`], the animmask overlays and block 2, and the base tiles. Debug
//! builds check the first three against their fingerprint at the load at every window key
//! ([`check_static`]). The records are written only by the shore merge (`anim::merge_rows`); a
//! writer of [`super::GfxState::records`] elsewhere must report the record
//! ([`RenderCache::record_changed`]).
//!
//! Spec 18: the remembered sight ([`SightMemory`]) is render state of the same kind, kept in the
//! cache: what the window last showed of each cell while it was in sight, drawn in grey where the
//! cell is in the window and out of sight now. A remembered cell's box reads its fade and its
//! layers ([`WindowKey::of`]), which change only with the fade's day while it stays out of
//! sight, so the refresh leaves its box out as any other.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use super::GfxState;
use super::view::{ViewWindow, cell_pixel};
use crate::geom::{HALF_H, HALF_W, MAP_X, MAP_Y, VIEW_H, VIEW_W};
use crate::map::view::{DARK, NOT_VISIBLE};
use crate::objects::DRAW_NODES;
use crate::state::State;
use u6_formats::gfx::{ANIM_FRAMES, TILE_COUNT, TILE_H, TILE_W};
use u6_render::Src;

/// Boxes of the window: its cells.
pub(super) const BOXES: usize = VIEW_W * VIEW_H;

/// Port-only, no original address: what the last map window render of the refresh drew, and the
/// records' versions. A cache, not game state: equal whatever it holds (the same game with and
/// without it is the same `State`). A clone has no window memo (a copy cannot know which back
/// buffer it would describe) and keeps the versions: the records are cloned with it, so a pixel
/// drawn from one before the clone is current or stale for the clone as for the original (spec 07
/// source plane).
#[derive(Debug, Default)]
pub struct RenderCache {
    window: Option<WindowMemo>,
    /// Per record: changes of its pixels so far (empty until the first).
    versions: Vec<u32>,
    /// Debug builds: the fingerprint of the tables that never change ([`check_static`]).
    static_tables: Option<u64>,
    /// Window renders left out whole so far (the tests read them).
    pub(crate) window_skips: u64,
    /// Window renders that redrew some boxes only, and the boxes they redrew.
    pub(crate) window_partial: u64,
    pub(crate) boxes_redrawn: u64,
    /// Spec 18: the remembered sight.
    pub(super) memory: SightMemory,
}

impl Clone for RenderCache {
    /// No window memo, no remembered sight and no counts; the records' versions and the static
    /// tables' fingerprint (the clone holds the same records and tables).
    fn clone(&self) -> Self {
        Self {
            versions: self.versions.clone(),
            static_tables: self.static_tables,
            ..Self::default()
        }
    }
}

impl PartialEq for RenderCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for RenderCache {}

impl RenderCache {
    /// Record `r`'s pixels changed: the boxes drawn from it are drawn again.
    pub fn record_changed(&mut self, r: usize) {
        if self.versions.len() < TILE_COUNT {
            self.versions.resize(TILE_COUNT, 0);
        }
        if let Some(v) = self.versions.get_mut(r) {
            *v = v.wrapping_add(1);
        }
    }

    /// Record `r`'s version: the changes of its pixels so far, 0 before the first. The source
    /// plane names it with each pixel drawn from the record (spec 07; a clone of the cache
    /// starts again at 0).
    pub fn version(&self, r: usize) -> u32 {
        self.versions.get(r).copied().unwrap_or(0)
    }

    /// Spec 18: forgets the remembered sight (load state 0x105350: the view shows black outside
    /// sight until its cells are seen again, as the original's), and the window memo with it: the
    /// memo's key names the store's copies by slot ([`WindowKey::layer`]), and the new store
    /// gives those slots to other contents.
    pub fn forget_sight(&mut self) {
        self.memory = SightMemory::default();
        self.window = None;
    }

    /// Spec 18: the cells the remembered sight holds now (the peak-heap receipt reads it).
    #[doc(hidden)]
    pub fn sight_cells(&self) -> usize {
        self.memory.cells.len()
    }

    /// Whether a window render is memoized (the tests read it).
    #[cfg(test)]
    pub(crate) fn has_window_memo(&self) -> bool {
        self.window.is_some()
    }

    /// Spec 18: the pixels and transparency of the remembered sight's copy with source id `id`
    /// ([`FROZEN_BASE`] + its slot), if it holds one.
    pub(crate) fn frozen_image(&self, id: u16) -> Option<(&[u8], &[bool])> {
        let f = self.frozen(id)?;
        Some((&f.pixels, &f.transparent))
    }

    /// Spec 18: the record the remembered sight's copy with source id `id` was taken from, if it
    /// holds one.
    pub(crate) fn frozen_record(&self, id: u16) -> Option<u16> {
        self.frozen(id).map(|f| f.record)
    }

    fn frozen(&self, id: u16) -> Option<&Frozen> {
        let slot = usize::from(id.checked_sub(FROZEN_BASE)?);
        self.memory.frozen.get(slot)
    }
}

/// Spec 18, port-only, no original address: the remembered sight. Per world cell and level, what
/// the framed window render last drew there while the cell was in sight, and when (the game's
/// calendar, in minutes: [`minutes`]): the base tile's draw, then those of its still objects in
/// draw order (never an actor's or a part that moves with one, a light overlay or a flash:
/// [`crate::objects::DrawLists::node_still`]). A cell stays remembered off the view and while the
/// party is on another level, and shows again when it is back in the window out of sight. Its
/// look fades by the whole game days since that sight ([`fade_of`], [`keeps`]); from
/// [`FORGET_DAYS`] it is black, and forgotten at the next sweep (once a game hour). At most
/// [`MAX_CELLS`] cells are kept, the oldest sight going first (about 24 bytes a cell of one
/// layer: [`Remembered`]). Load state 0x105350 forgets it all, then reads the one saved with its
/// save ([`read_sight`]); the window reset 0x133a96 keeps it. It lives in the [`RenderCache`],
/// outside the game state's equality; a Save writes it beside the save ([`saved`]).
///
/// The look is the one of the last sight: a layer keeps its record and masktype resolved (an
/// animdata substitution, 0x152cd2, changes only which record a tile draws) and the overlay frame
/// then; a record whose pixels the shore merge rewrites in place (0x152dac, the 32 shore tiles'
/// records, the only writer of records after the load: the module docs), and any record
/// rewritten so far (its version not 0, whichever tile draws it now), is kept as a copy of its
/// pixels then ([`Frozen`]). The copies are an append-only store, one per record and content,
/// kept until load state, at most [`MAX_COPIES`]. Its size is a constant of the game data: a
/// merged record's content is its load content with the masked pixels of the water records its
/// sources draw now, and those come from the animdata frames, which repeat with the low byte of
/// the animation counter ([`super::anim::anim_tick`]). No remembered layer draws a record of
/// that set, or one rewritten before its sight, live: when the store is full and a cell in sight
/// needs a new copy, the cell is forgotten instead (black once out of sight, as in the
/// original).
#[derive(Debug)]
pub struct SightMemory {
    /// The remembered cells by [`cell_key`].
    cells: HashMap<u32, Remembered>,
    /// The layers of the remembered cells that have more than one, all of them, by [`cell_key`].
    stacks: HashMap<u32, Vec<Layer>>,
    /// An unlit cell in sight (0x1bc) shows its remembered sight: [`MEMORY_IN_DARKNESS`].
    darkness: bool,
    /// The copies of rewritten records' pixels, by slot ([`Layer::frozen`]): never removed, so a
    /// slot names one content for the store's life (the window cache's key names it).
    frozen: Vec<Frozen>,
    /// The slots by record and content hash (a match compares the pixels too).
    by_content: HashMap<(u16, u64), Vec<u16>>,
    /// Per record, its version and the slot of its content at that version, for the last
    /// version looked up (the one a cell in sight takes again at every render).
    latest: HashMap<u16, (u32, u16)>,
    /// The game hour ([`minutes`] / 60) of the last sweep of the forgotten cells.
    swept: Option<u64>,
    /// Scratch: the layers of a cell being remembered.
    scratch: Vec<Layer>,
    /// The cells the cap forgot so far (the receipts count them).
    #[cfg(test)]
    evicted: usize,
}

impl Default for SightMemory {
    /// Nothing remembered, darkness as [`MEMORY_IN_DARKNESS`].
    fn default() -> Self {
        Self {
            cells: HashMap::new(),
            stacks: HashMap::new(),
            darkness: MEMORY_IN_DARKNESS,
            frozen: Vec::new(),
            by_content: HashMap::new(),
            latest: HashMap::new(),
            swept: None,
            scratch: Vec::new(),
            #[cfg(test)]
            evicted: 0,
        }
    }
}

/// One remembered cell, 16 bytes: the time of its last sight ([`minutes`] as a `u32`), its first
/// layer (the base tile's draw), and whether it has more ([`SightMemory::stacks`] then holds them
/// all). Most cells have one: of the 169 762 cells five game days of a walk over the owner's
/// land remember, 876 have more (spec 18, Cost).
///
/// The minute's horizon is `u32::MAX`, in the calendar's year 8 876, and the time saturates
/// there too ([`age`]). A sight five days or more before it ages through day 5 as any; a sight
/// less than five days before it stops aging at the day it has at the horizon, so it never
/// reaches day 5; a sight at or past it stays at day 0. Such sights go only at load state or by
/// the cap. The game's clock, which starts thousands of years before it, never gets there in
/// play.
#[derive(Clone, Copy, Debug)]
struct Remembered {
    seen: u32,
    first: Layer,
    stacked: bool,
}

/// The key of world cell (x, y) of level `z` in [`SightMemory::cells`]: the level in bits 20 on,
/// x in bits 10..20, y in bits 0..10 (a level is at most 1024 cells a side: the world mask).
fn cell_key((z, x, y): (u8, u16, u16)) -> u32 {
    u32::from(z) << 20 | u32::from(x & 0x3ff) << 10 | u32::from(y & 0x3ff)
}

/// The level and world cell of key `k` ([`cell_key`]).
#[cfg(test)]
fn key_cell(k: u32) -> (u8, u16, u16) {
    (
        (k >> 20) as u8,
        (k >> 10 & 0x3ff) as u16,
        (k & 0x3ff) as u16,
    )
}

/// Minute `now` ([`minutes`]) as a cell keeps it: at most `u32::MAX`, the horizon
/// ([`Remembered`]).
fn minute32(now: u64) -> u32 {
    u32::try_from(now).unwrap_or(u32::MAX)
}

/// The fade of a sight at minute `seen` at minute `now` ([`fade_of`]), both as a cell keeps them
/// ([`minute32`]).
fn age(seen: u32, now: u64) -> Option<u8> {
    fade_of(u64::from(seen), u64::from(minute32(now)))
}

/// A record's pixels and transparency as a remembered layer shows them, and the record they were
/// taken from.
#[derive(Debug)]
struct Frozen {
    record: u16,
    pixels: Vec<u8>,
    transparent: Vec<bool>,
}

/// [`u6_render::Src::id`] of copy slot 0 of the remembered sight ([`SightMemory`]), with
/// [`u6_render::Kind::Tile`]: slot n is `FROZEN_BASE + n`, past every record (the HD
/// compositor's images take it from [`RenderCache::frozen_image`]).
pub const FROZEN_BASE: u16 = TILE_COUNT as u16;

/// Spec 18: the most cells the remembered sight holds at any time (the oldest sight is forgotten
/// first): seven eighths of 2^18, the most a table of 2^18 buckets holds. [`SightMemory::cap`]
/// acts after a render once a further render's cells ([`BOXES`]) could pass it, so there are at
/// most `MAX_CELLS` − 1 before any insert. The table may grow once to 2^19 buckets (21 bytes
/// each, 11 MB retained) and no further: a cap never shrinks it, and with at most `MAX_CELLS`
/// cells, half of 2^19's load, std's hashbrown rehashes in place rather than grow (the assert
/// below). The one growth is the peak: the old and the new table live together (spec 18, Bounds).
/// Five game days of a walk over unseen land in Wide, a light burning at night, hold at most
/// 74 % of it (`receipt_five_days_of_a_wide_walk_fit_the_remembered_cells`).
pub const MAX_CELLS: usize = 7 << 15;

// hashbrown's `reserve_rehash` (behind std's `HashMap`) rehashes in place, allocating nothing,
// while the items after the insert are at most half the table's full capacity (seven eighths
// of its buckets), and grows the table otherwise. So with at most `MAX_CELLS` cells a table of
// 2^19 buckets never grows again. The rule as this toolchain's std carries it (Rust 1.96.1,
// hashbrown 0.16.1): `bucket_mask_to_capacity`, src/raw/mod.rs:184-193, and the test in
// `reserve_rehash_inner`, :2799 (the resize otherwise, :2826); the same in hashbrown 0.17.1,
// src/raw.rs:182 and :2756.
const _: () = assert!(
    MAX_CELLS <= (1 << 19) / 8 * 7 / 2,
    "MAX_CELLS fits a 2^19 table's in-place rehash"
);

/// Spec 18: the most copies of rewritten records it keeps (about 0.6 KiB each with its index,
/// 5.39 MB in all; 32 times the 255 the owner's game makes).
pub const MAX_COPIES: usize = 1 << 13;

/// Spec 18 (the owner's follow-up): whole game days after which a remembered cell is black and
/// forgotten; before, day d shows 2^-d of its pixels ([`keeps`]).
pub const FORGET_DAYS: u64 = 5;

/// The 4 × 4 ordered (Bayer) matrix of the fade, by (row & 3, column & 3): each threshold once,
/// 0..15, so that the thresholds below 8 are a checkerboard and each level below a smaller
/// power of two a subset of the one before.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Spec 18: whether a remembered cell `fade` whole days old ([`fade_of`]) draws its pixel at
/// column `c`, row `row` of the cell: the 4 × 4 ordered pattern on the original's pixel grid,
/// anchored to the world (a cell is 16 pixels, so its own column and row stand for the world's),
/// keeps the pixels whose threshold is below 16 >> `fade`: all of them on day 0, 1 in 2 (a
/// checkerboard), 1 in 4, 1 in 8, 1 in 16 on days 1..4, each a subset of the day before.
pub(crate) fn keeps(fade: u8, c: usize, row: usize) -> bool {
    BAYER[row & 3][c & 3] < 16u8.checked_shr(u32::from(fade)).unwrap_or(0)
}

/// Spec 18: the game calendar `cal` in minutes since its year 0 (the original's: 60 minutes,
/// 24 hours, days 1..=28, months 1..=12, `actors::clock`'s rollover).
pub(crate) fn minutes(cal: &crate::actors::Calendar) -> u64 {
    let months = u64::from(cal.year) * 12 + u64::from(cal.month.saturating_sub(1));
    let days = months * 28 + u64::from(cal.day.saturating_sub(1));
    (days * 24 + u64::from(cal.hour)) * 60 + u64::from(cal.minute)
}

/// Spec 18: the whole game days (1440 minutes) from `seen` to `now`; `None` from
/// [`FORGET_DAYS`] on (black, forgotten).
pub(crate) fn fade_of(seen: u64, now: u64) -> Option<u8> {
    let days = now.saturating_sub(seen) / (24 * 60);
    (days < FORGET_DAYS).then_some(days as u8)
}

/// The layers a remembered cell shows in the render, with its fade ([`keeps`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Shown {
    pub(super) fade: u8,
    pub(super) layers: Vec<Layer>,
}

/// One draw of a remembered cell: the record and the masktype its word drew then, the record's
/// version then, the overlay frame then, and for a rewritten record the slot of the copy of its
/// pixels then ([`SightMemory`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Layer {
    pub(super) record: u16,
    pub(super) masktype: u8,
    pub(super) frame: u8,
    pub(super) frozen: Option<u16>,
}

impl Layer {
    /// Word `w` as the blitter draws it now ([`GfxState::record`]), not yet copied.
    fn of(gfx: &GfxState, w: u16) -> Layer {
        let t = usize::from(w & 0x7ff);
        Layer {
            record: gfx.record_of[t],
            masktype: gfx.masktype[t],
            frame: gfx.frame,
            frozen: None,
        }
    }

    /// The pixels and transparency the layer draws and the source it names them by: the copy's
    /// ([`FROZEN_BASE`] + its slot, version 0: a copy never changes), else the record's at its
    /// version now.
    pub(super) fn image<'a>(&self, gfx: &'a GfxState) -> Option<(&'a [u8], &'a [bool], u16, u32)> {
        match self.frozen {
            Some(slot) => {
                let f = gfx.cache.memory.frozen.get(usize::from(slot))?;
                Some((&f.pixels, &f.transparent, FROZEN_BASE + slot, 0))
            }
            None => {
                let r = usize::from(self.record);
                let rec = gfx.records.get(r)?;
                Some((
                    &rec.pixels,
                    &rec.transparent,
                    self.record,
                    gfx.cache.version(r),
                ))
            }
        }
    }
}

/// Spec 18 (`ssot.md` item 3): darkness too shows the remembered sight, a cell in sight but unlit
/// (0x1bc) as one out of sight (0xff). The owner's veto of darkness is this one constant.
const MEMORY_IN_DARKNESS: bool = true;

/// A view cell whose base tile is `base` shows nothing of itself now: out of sight
/// ([`NOT_VISIBLE`]) or unlit ([`DARK`]). Such a cell is never remembered.
fn unseen(base: u16) -> bool {
    base == NOT_VISIBLE || base == DARK
}

/// FNV-1a over a copy's pixels and transparency.
fn content_hash(pixels: &[u8], transparent: &[bool]) -> u64 {
    let bytes = pixels
        .iter()
        .copied()
        .chain(transparent.iter().map(|&t| u8::from(t)));
    bytes.fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

impl SightMemory {
    /// The world cell of view cell (col, row) round the view centre `c` (0x10e81e's mapping,
    /// `objects::draw`), in `mask`'s world.
    fn world_cell(c: (u16, u16), mask: u16, col: usize, row: usize) -> (u16, u16) {
        let x = c.0.wrapping_add(col as u16).wrapping_sub(HALF_W as u16);
        let y = c.1.wrapping_add(row as u16).wrapping_sub(HALF_H as u16);
        (x & mask, y & mask)
    }

    /// The key of view cell (col, row) in the state's view now: its level and world cell.
    fn key(state: &State, (col, row): (usize, usize)) -> (u8, u16, u16) {
        let c = state.map.centre;
        let (x, y) = Self::world_cell((c.x, c.y), state.map.world_mask, col, row);
        (c.z, x, y)
    }

    /// A view cell whose base tile is `base` shows its remembered sight, if any: out of sight, or
    /// unlit with [`SightMemory::darkness`].
    fn shows(&self, base: u16) -> bool {
        base == NOT_VISIBLE || (self.darkness && base == DARK)
    }

    /// What view cell (col, row), whose base tile is `base`, shows in grey in the state's view
    /// now: its remembered layers when it [`SightMemory::shows`] them and they are younger than
    /// [`FORGET_DAYS`], with their fade.
    pub(super) fn shown(
        &self,
        state: &State,
        at: (usize, usize),
        base: u16,
    ) -> Option<(u8, &[Layer])> {
        if !self.shows(base) {
            return None;
        }
        let k = cell_key(Self::key(state, at));
        let cell = self.cells.get(&k)?;
        let fade = age(cell.seen, minutes(&state.actors.calendar))?;
        Some((fade, self.layers_of(k, cell)?))
    }

    /// The layers of remembered cell `cell`, key `k`: its first, or its stack.
    fn layers_of<'a>(&'a self, k: u32, cell: &'a Remembered) -> Option<&'a [Layer]> {
        match cell.stacked {
            false => Some(std::slice::from_ref(&cell.first)),
            true => self.stacks.get(&k).map(Vec::as_slice),
        }
    }

    /// Remembers cell `k` ([`cell_key`]) as `layers` (the base tile's first), seen at `seen`;
    /// with no layer, forgets it.
    fn put(&mut self, k: u32, layers: &[Layer], seen: u32) {
        let Some(&first) = layers.first() else {
            self.forget(k);
            return;
        };
        let stacked = layers.len() > 1;
        let old = self.cells.insert(
            k,
            Remembered {
                seen,
                first,
                stacked,
            },
        );
        if stacked {
            let stack = self.stacks.entry(k).or_default();
            stack.clear();
            stack.extend_from_slice(layers);
        } else if old.is_some_and(|c| c.stacked) {
            self.stacks.remove(&k);
        }
    }

    /// Forgets cell `k` ([`cell_key`]).
    fn forget(&mut self, k: u32) {
        if self.cells.remove(&k).is_some_and(|c| c.stacked) {
            self.stacks.remove(&k);
        }
    }

    /// The stacks of the cells forgotten since: dropped.
    fn drop_stacks(&mut self) {
        let cells = &self.cells;
        self.stacks.retain(|k, _| cells.contains_key(k));
    }

    /// The slot of the copy of `record`'s pixels now: the one looked up last for the record at
    /// this version, else a copy of the same content, else a new one; `None` when a new one is
    /// needed and the store holds [`MAX_COPIES`].
    fn freeze(&mut self, gfx: &GfxState, record: u16) -> Option<u16> {
        let r = usize::from(record);
        let ver = gfx.cache.version(r);
        if let Some(&(v, slot)) = self.latest.get(&record)
            && v == ver
        {
            return Some(slot);
        }
        let rec = &gfx.records[r];
        let hash = content_hash(&rec.pixels, &rec.transparent);
        let same = self.by_content.get(&(record, hash)).and_then(|slots| {
            slots.iter().copied().find(|&s| {
                let f = &self.frozen[usize::from(s)];
                f.pixels == rec.pixels && f.transparent == rec.transparent
            })
        });
        let slot = match same {
            Some(slot) => slot,
            None if self.frozen.len() >= MAX_COPIES => return None,
            None => {
                self.frozen.push(Frozen {
                    record,
                    pixels: rec.pixels.clone(),
                    transparent: rec.transparent.clone(),
                });
                let slot = (self.frozen.len() - 1) as u16;
                self.by_content
                    .entry((record, hash))
                    .or_default()
                    .push(slot);
                slot
            }
        };
        self.latest.insert(record, (ver, slot));
        Some(slot)
    }

    /// Once a game hour ([`minutes`] / 60), forgets the cells [`FORGET_DAYS`] old at `now` (a scan
    /// of every cell); the copies they named stay in the store.
    fn sweep(&mut self, now: u64) {
        if self.swept == Some(now / 60) {
            return;
        }
        self.swept = Some(now / 60);
        self.cells.retain(|_, c| age(c.seen, now).is_some());
        self.drop_stacks();
    }

    /// Once the next render's cells ([`BOXES`], at most that many new) could pass [`MAX_CELLS`],
    /// forgets the oldest sights down to seven eighths of it (a scan of every cell); of the
    /// sights of the cutoff's own minute, those of the lowest keys ([`cell_key`]), so the cells
    /// kept do not hang on the table's order. The copies they named stay in the store.
    fn cap(&mut self) {
        if self.cells.len() + BOXES <= MAX_CELLS {
            return;
        }
        let target = MAX_CELLS - MAX_CELLS / 8;
        let mut seen: Vec<u32> = self.cells.values().map(|c| c.seen).collect();
        let k = self.cells.len() - target;
        let (_, &mut cutoff, _) = seen.select_nth_unstable(k);
        self.cells.retain(|_, c| c.seen >= cutoff);
        let over = self.cells.len().saturating_sub(target);
        let mut tied: Vec<u32> = (self.cells.iter())
            .filter(|(_, c)| c.seen == cutoff)
            .map(|(&k, _)| k)
            .collect();
        tied.sort_unstable();
        for k in tied.into_iter().take(over) {
            self.cells.remove(&k);
        }
        #[cfg(test)]
        {
            self.evicted += seen.len() - self.cells.len();
        }
        self.drop_stacks();
    }

    /// The remembered sight for the render of `window` (the state's view now): with `framed`,
    /// every cell in sight remembered as the render draws it now, at the time now (in the cell's
    /// own entry; a rewritten record as a copy of its pixels now; a cell whose new copy the full
    /// store cannot take forgotten instead); once a game hour the cells [`FORGET_DAYS`] old
    /// forgotten (a scan of every cell), and past [`MAX_CELLS`] the oldest (a scan of every cell,
    /// then the cells are down to seven eighths of it). Then per box, what the render draws in
    /// grey ([`SightMemory::shown`]).
    pub(super) fn layers(state: &mut State, window: &ViewWindow, framed: bool) -> Vec<Shown> {
        if framed {
            let now = minutes(&state.actors.calendar);
            let keys: Vec<Option<u32>> = cells_of(window)
                .map(|(at, base)| (!unseen(base)).then(|| cell_key(Self::key(state, at))))
                .collect();
            let State { gfx, objects, .. } = &mut *state;
            let d = &objects.draw;
            let mut memory = std::mem::take(&mut gfx.cache.memory);
            // The records the shore merge writes (0x152d52: the record of tile `shore_dst[i] / 4`).
            let merged: Vec<u16> = (gfx.tables.shore_dst.iter())
                .filter(|&&w| w.is_multiple_of(4) && usize::from(w / 4) < TILE_COUNT)
                .map(|&w| gfx.record_of[usize::from(w / 4)])
                .collect();
            for (i, key) in keys.iter().enumerate() {
                let Some(key) = *key else { continue };
                let (row, col) = (i / VIEW_W, i % VIEW_W);
                let mut layers = std::mem::take(&mut memory.scratch);
                layers.clear();
                layers.push(Layer::of(gfx, window.tiles[row][col]));
                let mut n = d.cell[i];
                // At most the pool's nodes, as [`crate::objects::DrawLists::tiles`] reads.
                for _ in 0..DRAW_NODES {
                    if n == 0 {
                        break;
                    }
                    let i = usize::from(n);
                    if d.node_still[i] {
                        layers.push(Layer::of(gfx, d.node_tile[i]));
                    }
                    n = d.node_next[i];
                }
                let mut kept = true;
                for l in layers.iter_mut() {
                    let r = usize::from(l.record);
                    if merged.contains(&l.record) || gfx.cache.version(r) != 0 {
                        l.frozen = memory.freeze(gfx, l.record);
                        kept &= l.frozen.is_some();
                    }
                }
                if kept {
                    debug_assert!(memory.cells.len() < MAX_CELLS, "room for the render's cell");
                    memory.put(key, &layers, minute32(now));
                } else {
                    // The store is full: the look is not kept, nor the older one (not the
                    // cell's last sight).
                    memory.forget(key);
                }
                memory.scratch = layers;
            }
            memory.sweep(now);
            memory.cap();
            gfx.cache.memory = memory;
        }
        state.gfx.cache.memory.shown_in(state, window)
    }

    /// Per box of `window` (the view of `state`), what the render draws in grey there
    /// ([`SightMemory::shown`]: keyed by `state`'s view and faded at its time), this memory
    /// unchanged. Port (spec 20): the early picture reads the state's memory round the view of its
    /// copy, whose cache holds none.
    pub(super) fn shown_in(&self, state: &State, window: &ViewWindow) -> Vec<Shown> {
        cells_of(window)
            .map(|(at, base)| match self.shown(state, at, base) {
                Some((fade, layers)) => Shown {
                    fade,
                    layers: layers.to_vec(),
                },
                None => Shown::default(),
            })
            .collect()
    }

    /// Cells remembered now (the tests read it).
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.cells.len()
    }

    /// The copies of rewritten records' pixels held now (the tests read it).
    #[cfg(test)]
    pub(super) fn copies(&self) -> usize {
        self.frozen.len()
    }

    /// Fills the copy store up to `n` copies of record `record`, of distinct contents (pixels 0
    /// and 1 the slot's two bytes, the rest 0; opaque: the saturation tests).
    #[cfg(test)]
    pub(super) fn fill_copies(&mut self, n: usize, record: u16) {
        for i in self.frozen.len()..n {
            let mut pixels = vec![0; 256];
            pixels[0] = i as u8;
            pixels[1] = (i >> 8) as u8;
            let transparent = vec![false; 256];
            let hash = content_hash(&pixels, &transparent);
            let slot = self.frozen.len() as u16;
            self.frozen.push(Frozen {
                record,
                pixels,
                transparent,
            });
            self.by_content
                .entry((record, hash))
                .or_default()
                .push(slot);
        }
    }

    /// Remembers world cell (x, y) of level `z` as `layers`, seen at `seen` (the tests of a given
    /// layer).
    #[cfg(test)]
    pub(super) fn remember(&mut self, at: (u8, u16, u16), layers: Vec<Layer>, seen: u64) {
        self.put(cell_key(at), &layers, minute32(seen));
    }

    /// Whether world cell (x, y) is remembered now, on any level (the tests read it).
    #[cfg(test)]
    pub(super) fn holds(&self, x: u16, y: u16) -> bool {
        (self.cells.keys()).any(|&k| (key_cell(k).1, key_cell(k).2) == (x, y))
    }

    /// Whether world cell (x, y) of level `z` is remembered now (the tests read it).
    #[cfg(test)]
    pub(super) fn holds_on(&self, z: u8, x: u16, y: u16) -> bool {
        self.cells.contains_key(&cell_key((z, x, y)))
    }

    /// World cell (x, y)'s remembered layers, on any level (the tests read them).
    #[cfg(test)]
    pub(super) fn at(&self, x: u16, y: u16) -> Option<&[Layer]> {
        let (&k, c) =
            (self.cells.iter()).find(|&(&k, _)| (key_cell(k).1, key_cell(k).2) == (x, y))?;
        self.layers_of(k, c)
    }

    /// The records of every remembered layer (the tests read them).
    #[cfg(test)]
    pub(super) fn records(&self) -> impl Iterator<Item = u16> + '_ {
        (self.cells.iter())
            .flat_map(|(&k, c)| self.layers_of(k, c).unwrap_or(&[]))
            .map(|l| l.record)
    }

    /// The keys of the remembered cells, sorted (the tests compare them).
    #[cfg(test)]
    pub(super) fn keys(&self) -> Vec<(u8, u16, u16)> {
        let mut keys: Vec<u32> = self.cells.keys().copied().collect();
        keys.sort_unstable();
        keys.into_iter().map(key_cell).collect()
    }

    /// The remembered cells with more than one layer (the receipts count them).
    #[cfg(test)]
    pub(super) fn stacked(&self) -> usize {
        self.stacks.len()
    }

    /// The cells the cap forgot so far (the receipts count them).
    #[cfg(test)]
    pub(super) fn evicted(&self) -> usize {
        self.evicted
    }

    /// The buckets of the cells' table, from its capacity (a power of two, the capacity at most
    /// seven eighths of it: std's hashbrown; the bound test prints it). A lower bound: deleted
    /// entries take capacity, not buckets.
    #[cfg(test)]
    pub(super) fn cell_buckets(&self) -> usize {
        match self.cells.capacity() {
            0 => 0,
            n => (n * 8 / 7).next_power_of_two(),
        }
    }

    /// The slot of the copy of `record`'s pixels now, as a remembered layer takes it (the
    /// copy-store receipt).
    #[cfg(test)]
    pub(super) fn copy_of(&mut self, gfx: &GfxState, record: u16) -> Option<u16> {
        self.freeze(gfx, record)
    }

    /// Sets whether darkness shows the remembered sight (the tests try both values of
    /// [`MEMORY_IN_DARKNESS`]).
    #[cfg(test)]
    pub(super) fn set_darkness(&mut self, on: bool) {
        self.darkness = on;
    }

    /// `n` synthetic remembered cells on level `z` from row `y0` on (rows of 1024 cells, at most
    /// to row 1023), each one layer of record `record` seen at `seen` (the cost receipts fill the
    /// memory with them).
    #[cfg(test)]
    pub(super) fn fill_synthetic(&mut self, n: usize, z: u8, y0: u16, record: u16, seen: u64) {
        let layer = Layer {
            record,
            masktype: 0,
            frame: 0,
            frozen: None,
        };
        for i in 0..n {
            let y = y0 + (i / 1024) as u16;
            debug_assert!(y < 1024, "a row of the world");
            self.put(
                cell_key((z, (i % 1024) as u16, y)),
                &[layer],
                minute32(seen),
            );
        }
    }

    /// The bytes the memory holds on the heap, from its tables' and buffers' capacities in this
    /// build's layouts (the cost receipts): the cells' table, the stacks' table and their layers,
    /// the copy store and its copies' pixels and transparencies, the index by content and its
    /// slot lists, the latest index, the scratch layers: what it retains, not the peak of a
    /// table's growth (spec 18, Cost). A table of capacity n holds a power of two of buckets, n
    /// at most seven eighths of them (so the buckets are a lower bound: [`Self::cell_buckets`]),
    /// each an entry and a control byte, and a group's width of control bytes more (std's
    /// hashbrown: 16 with SSE2, 8 with NEON or on wasm32; counted as 16). Not the allocator's
    /// own overhead.
    #[cfg(test)]
    pub(super) fn heap_bytes(&self) -> usize {
        use std::mem::size_of;
        let table = |cap: usize, entry: usize| match cap {
            0 => 0,
            _ => (cap * 8 / 7).next_power_of_two() * (entry + 1) + 16,
        };
        let cells = table(self.cells.capacity(), size_of::<(u32, Remembered)>());
        let stacks = table(self.stacks.capacity(), size_of::<(u32, Vec<Layer>)>());
        let layers: usize = self.stacks.values().map(Vec::capacity).sum();
        let store = self.frozen.capacity() * size_of::<Frozen>();
        let copies: usize = (self.frozen.iter())
            .map(|f| f.pixels.capacity() + f.transparent.capacity() * size_of::<bool>())
            .sum();
        let index = table(
            self.by_content.capacity(),
            size_of::<((u16, u64), Vec<u16>)>(),
        );
        let lists: usize = (self.by_content.values()).map(|v| v.capacity() * 2).sum();
        let latest = table(self.latest.capacity(), size_of::<(u16, (u32, u16))>());
        let scratch = self.scratch.capacity();
        cells
            + stacks
            + (layers + scratch) * size_of::<Layer>()
            + store
            + copies
            + index
            + lists
            + latest
    }
}

/// Each view cell of `window` with its base tile, in box order (rows, then columns).
fn cells_of(window: &ViewWindow) -> impl Iterator<Item = ((usize, usize), u16)> + '_ {
    let rows = window.tiles.iter().enumerate();
    rows.flat_map(|(row, tiles)| {
        tiles
            .iter()
            .enumerate()
            .map(move |(col, &b)| ((col, row), b))
    })
}

/// Port-only (spec 07 HD0): the sources of pixels kept beside game state (the block under a
/// flight sprite, a recorded frame), so that pixels put back name their images again. Not game
/// state: equal whatever it holds (a run with the source plane and one without are the same
/// `State`); `None` when the buffer they came from had no plane. Shared: a clone, and a recorded
/// frame whose sources are those of a frame recorded before it and not played yet
/// ([`Sources::shared`]), hold the same allocation; a write copies a shared one first
/// ([`std::rc::Rc::make_mut`]).
///
/// The second field is the hash of the sources ([`Sources::hashed`]) for those kept as they were
/// taken (a recorded frame), `None` for those written in place (the block under a flight
/// sprite): a write would leave it stale.
#[derive(Clone, Debug, Default)]
pub struct Sources(pub Option<Rc<[Src]>>, pub(crate) Option<u64>);

impl PartialEq for Sources {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Sources {}

impl Sources {
    /// Sources taken from a buffer, without their hash.
    pub fn new(src: Option<Vec<Src>>) -> Sources {
        Sources(src.map(Rc::from), None)
    }

    /// The source at `i`, if there are sources.
    pub fn at(&self, i: usize) -> Option<Src> {
        self.0.as_ref().and_then(|s| s.get(i).copied())
    }

    /// The same sources with their hash, to be kept as they are.
    pub fn hashed(self) -> Sources {
        let hash = self.0.as_deref().map(hash_of);
        Sources(self.0, hash)
    }

    /// These sources ([`Sources::hashed`]) in the allocation of the first of `earlier` whose
    /// sources are equal, if any, else in their own. Only sources of the same hash are compared,
    /// so the frames waiting to be played back hold one allocation per distinct sources at the
    /// cost of one hash per frame recorded.
    pub fn shared<'a>(self, earlier: impl IntoIterator<Item = &'a Sources>) -> Sources {
        let (Some(new), Some(hash)) = (&self.0, self.1) else {
            return self;
        };
        let same = |old: &&Sources| {
            old.1 == Some(hash) && old.0.as_ref().is_some_and(|o| o[..] == new[..])
        };
        match earlier.into_iter().find(same) {
            Some(old) => old.clone(),
            None => self,
        }
    }
}

/// A hash of `src`: FxHash's multiply and rotate over one word of each source's fields, in four
/// lanes (a frame's 30,976 sources hash in tens of microseconds).
fn hash_of(src: &[Src]) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mix = |h: u64, w: u64| (h.rotate_left(5) ^ w).wrapping_mul(K);
    let word = |s: &Src| {
        let a = u64::from(s.ver) | u64::from(s.id) << 32 | u64::from(s.u) << 48;
        let b = u64::from(s.v) | u64::from(s.kind as u8) << 16 | u64::from(s.flags) << 24;
        a ^ b.rotate_left(23)
    };
    let mut lanes = [src.len() as u64, 1, 2, 3];
    let (quads, rest) = src.as_chunks::<4>();
    for quad in quads {
        for (h, s) in lanes.iter_mut().zip(quad) {
            *h = mix(*h, word(s));
        }
    }
    for (h, s) in lanes.iter_mut().zip(rest) {
        *h = mix(*h, word(s));
    }
    lanes.into_iter().fold(0, mix)
}

/// A window render: what it read, and the back buffer it drew into as it stood after its
/// present ([`crate::engine::Io::back_mark`]).
#[derive(Debug)]
struct WindowMemo {
    back: (u64, u64),
    key: WindowKey,
}

/// Every value the window render reads, box by box ([`WindowKey::of`]).
#[derive(Debug)]
pub(super) struct WindowKey {
    /// The clock's tile word, then each box's values (cells in order).
    values: Vec<u32>,
    /// Where box `b`'s values end in `values`.
    ends: [u32; BOXES],
    /// A clock is drawn.
    pub(super) clock: bool,
}

/// The values before the first box's: the clock's tile word.
const GLOBAL: usize = 1;

impl WindowKey {
    /// The values the framed render of `window` reads, per box: the cell's base word, the count
    /// of its remembered layers (spec 18, `memory`: per box, empty with the remembered sight off)
    /// and, when it has any, their fade and each layer as [`WindowKey::layer`] has it, the count
    /// of its object words, each word as [`WindowKey::word`] has it (with a clock word the hour
    /// and the minute), then the target cursor's word when it is in the box, then the box's
    /// border word. `None` when the render cannot be remembered: the sparkle, the dissolve or XOR
    /// plots on, the cursor outside the cells, an overlay or clock hands that would leave their
    /// box.
    pub(super) fn of(
        state: &State,
        window: &ViewWindow,
        memory: &[Shown],
        clock: Option<u16>,
        cursor: Option<(u16, u16, u16)>,
        border: &[Option<u16>; BOXES],
    ) -> Option<WindowKey> {
        let gfx = &state.gfx;
        if gfx.sparkle != 0 || gfx.dissolve != 0 || gfx.plot_xor {
            return None;
        }
        let cursor = match cursor {
            None => None,
            Some((tile, x, y)) => Some((tile, cell_box(x, y)?)),
        };
        let mut key = WindowKey {
            values: Vec::with_capacity(GLOBAL + 6 * BOXES),
            ends: [0; BOXES],
            clock: false,
        };
        key.values.push(clock.map_or(u32::MAX, u32::from));
        for (row, tiles) in window.tiles.iter().enumerate() {
            for (col, &tile) in tiles.iter().enumerate() {
                let b = row * VIEW_W + col;
                key.word(gfx, tile)?;
                let shown = memory.get(b);
                let layers = shown.map_or(&[][..], |m| m.layers.as_slice());
                key.values.push(layers.len() as u32);
                if !layers.is_empty() {
                    key.values.push(u32::from(shown.map_or(0, |m| m.fade)));
                }
                for &l in layers {
                    key.layer(gfx, l);
                }
                let objects = &window.objects[b];
                key.values.push(objects.len() as u32);
                for &w in objects {
                    key.word(gfx, w)?;
                    if Some(w & 0x7ff) == clock {
                        let (x, y) = cell_pixel(col, row);
                        let cal = &state.actors.calendar;
                        let points = super::view::clock_points(cal, &gfx.tables, x, y);
                        if points
                            .iter()
                            .any(|&p| cell_box(p.0 & !15, p.1 & !15) != Some(b))
                        {
                            return None;
                        }
                        key.values
                            .extend([u32::from(cal.hour), u32::from(cal.minute)]);
                        key.clock = true;
                    }
                }
                if let Some((tile, _)) = cursor.filter(|&(_, at)| at == b) {
                    key.values.push(1);
                    key.word(gfx, tile)?;
                }
                if let Some(w) = border[b] {
                    key.word(gfx, w)?;
                }
                key.ends[b] = key.values.len() as u32;
            }
        }
        Some(key)
    }

    /// Word `w` drawn in a box: the word, the record and the masktype it draws, the record's
    /// version, and when the blitter ORs its overlay in (masktype bit 2, an overlay, not the
    /// outline path) the first frame with the rows the overlay frame draws. `None` when the
    /// record or those rows would reach past the box.
    fn word(&mut self, gfx: &GfxState, w: u16) -> Option<()> {
        let t = usize::from(w & 0x7ff);
        let r = usize::from(gfx.record_of[t]);
        let (rec, ov, masktype) = gfx.record(w);
        if rec.pixels.len() > TILE_W * TILE_H || rec.transparent.len() > TILE_W * TILE_H {
            return None;
        }
        self.values.extend([
            u32::from(w),
            r as u32,
            u32::from(masktype),
            gfx.cache.version(r),
        ]);
        let outline = masktype & 3 != 0 && w & 0x1800 != 0;
        if let Some(ov) = ov.filter(|_| masktype & 4 != 0 && !outline) {
            let rows = ov.frames.get(usize::from(gfx.frame) % ANIM_FRAMES)?;
            if usize::from(ov.first_row) + rows.len() > TILE_H {
                return None;
            }
            let first = ov.frames.iter().position(|f| f == rows)?;
            self.values.push(first as u32);
        }
        Some(())
    }

    /// Remembered layer `l` drawn in a box ([`super::view`]'s memory blit, which stays in it):
    /// the record, the masktype, whether it draws a copy, and which pixels it draws (the copy's
    /// slot, which names one content for the store's life, else the record's version now), and
    /// when the overlay is drawn (masktype bit 2, an overlay) the first frame with the rows of
    /// the frame kept. The box's fade is read before its layers ([`WindowKey::of`]).
    fn layer(&mut self, gfx: &GfxState, l: Layer) {
        let r = usize::from(l.record);
        let ver = match l.frozen {
            Some(slot) => u32::from(slot),
            None => gfx.cache.version(r),
        };
        self.values.extend([
            r as u32,
            u32::from(l.masktype),
            u32::from(l.frozen.is_some()),
            ver,
        ]);
        let ov = gfx.overlays.get(r).and_then(Option::as_ref);
        if let Some(ov) = ov.filter(|_| l.masktype & 4 != 0) {
            let rows = &ov.frames[usize::from(l.frame) % ANIM_FRAMES];
            let first = ov.frames.iter().position(|f| f == rows).unwrap_or(0);
            self.values.push(first as u32);
        }
    }

    fn of_box(&self, b: usize) -> &[u32] {
        let start = if b == 0 {
            GLOBAL
        } else {
            self.ends[b - 1] as usize
        };
        &self.values[start..self.ends[b] as usize]
    }
}

/// The box of the cell whose top left is (x, y), if it is one.
pub(super) fn cell_box(x: u16, y: u16) -> Option<usize> {
    let (dx, dy) = (x.checked_sub(MAP_X)?, y.checked_sub(MAP_Y)?);
    let (col, row) = (usize::from(dx / 16), usize::from(dy / 16));
    (dx.is_multiple_of(16) && dy.is_multiple_of(16) && col < VIEW_W && row < VIEW_H)
        .then_some(row * VIEW_W + col)
}

/// The boxes the render of `key` must draw over what the back buffer holds: those whose values
/// differ from the last render's. `None` when every box must be drawn: nothing remembered, the
/// back buffer another one or drawn on since (`back`), or the clock's tile word changed.
pub(super) fn dirty_boxes(
    gfx: &GfxState,
    back: (u64, u64),
    key: &WindowKey,
) -> Option<[bool; BOXES]> {
    let memo = gfx.cache.window.as_ref()?;
    if memo.back != back || memo.key.values[..GLOBAL] != key.values[..GLOBAL] {
        return None;
    }
    Some(std::array::from_fn(|b| memo.key.of_box(b) != key.of_box(b)))
}

/// Remembers the render of `key`, its back buffer as `back` left it; `None` forgets the last one
/// (a render the memo cannot stand for).
pub(super) fn remember_window(gfx: &mut GfxState, memo: Option<(WindowKey, (u64, u64))>) {
    gfx.cache.window = memo.map(|(key, back)| WindowMemo { back, key });
}

/// The fingerprint of what the window render reads and nothing writes after the load: the game
/// tables, the overlays and the animmask blocks.
fn static_fingerprint(gfx: &GfxState) -> u64 {
    let mut h = DefaultHasher::new();
    gfx.tables.hash(&mut h);
    gfx.overlays.hash(&mut h);
    gfx.animmask.hash(&mut h);
    h.finish()
}

/// Debug builds: takes the static tables' fingerprint at the load (or, for a state built
/// otherwise, at its first window key), and panics when a later call finds them written: the
/// window cache would then stand for draws of other tables. Nothing in release builds.
pub(super) fn check_static(gfx: &mut GfxState) {
    if !cfg!(debug_assertions) {
        return;
    }
    let now = static_fingerprint(gfx);
    match gfx.cache.static_tables {
        None => gfx.cache.static_tables = Some(now),
        Some(was) => debug_assert!(
            was == now,
            "the game tables, overlays or animmask were written after the load (spec 06 O2)"
        ),
    }
}

#[cfg(test)]
mod sight_tests {
    use u6_testkit::Quiet;

    use super::{
        BOXES, FORGET_DAYS, Frozen, Layer, MAX_CELLS, MAX_COPIES, SightMemory, age, cell_key,
        content_hash, fade_of, keeps, minute32, minutes,
    };
    use crate::gfx::GfxState;

    /// Record `r` rewritten to `colour` on every pixel, opaque, and reported as the shore merge
    /// reports a rewrite (its version one more).
    fn paint(g: &mut GfxState, r: u16, colour: u8) {
        let rec = &mut g.records[usize::from(r)];
        rec.pixels.fill(colour);
        rec.transparent.fill(false);
        g.cache.record_changed(usize::from(r));
    }

    /// The colour copy `slot` holds, if it holds one opaque colour on every pixel.
    fn colour_of(m: &SightMemory, slot: u16) -> Option<u8> {
        let f = m.frozen.get(usize::from(slot))?;
        let c = f.pixels[0];
        let one = f.pixels.iter().all(|&p| p == c) && f.transparent.iter().all(|&t| !t);
        one.then_some(c)
    }

    /// One layer of record `record` drawn from copy `slot`.
    fn layer(record: u16, slot: u16) -> Vec<Layer> {
        vec![Layer {
            record,
            masktype: 0,
            frame: 0,
            frozen: Some(slot),
        }]
    }

    /// Spec 18 (the review's copy ownership; the append-only store): one copy per record and
    /// content. The record at the same version, and rewritten to a content held before (a new
    /// version), takes the copy it took then; another record of the same content its own; a new
    /// content (the transparency included) a new slot, the slots before still holding theirs.
    #[test]
    fn one_copy_per_record_and_content() {
        let mut g = GfxState::default();
        let mut m = SightMemory::default();
        paint(&mut g, 5, 1);
        let a = m.freeze(&g, 5).quiet();
        assert_eq!(m.freeze(&g, 5), Some(a), "the same version: the same copy");
        assert_eq!(m.copies(), 1);
        paint(&mut g, 6, 1);
        let b = m.freeze(&g, 6).quiet();
        assert!(b != a, "another record of the same content: its own copy");
        paint(&mut g, 5, 1);
        assert_eq!(
            m.freeze(&g, 5),
            Some(a),
            "the same content, a new version: no new copy"
        );
        assert_eq!(m.copies(), 2);
        paint(&mut g, 5, 2);
        let c = m.freeze(&g, 5).quiet();
        assert!(c != a && c != b, "a new content: a new slot");
        paint(&mut g, 5, 1);
        assert_eq!(m.freeze(&g, 5), Some(a), "a content held before: its copy");
        g.records[5].transparent[0] = true;
        g.cache.record_changed(5);
        let d = m.freeze(&g, 5).quiet();
        assert!(![a, b, c].contains(&d), "another transparency: a new slot");
        assert_eq!(m.copies(), 4);
        assert_eq!(colour_of(&m, a), Some(1), "slot a: its content");
        assert_eq!(colour_of(&m, c), Some(2), "slot c: its content");
        assert_eq!(
            m.frozen[usize::from(b)].record,
            6,
            "slot b: taken from record 6"
        );
    }

    /// Spec 18 (the review's forgetting, cap and sweep): the sweep forgets the cells
    /// [`FORGET_DAYS`] old, once a game hour (a cell as old after it waits for the next hour);
    /// the cap the oldest; neither drops a copy, and a slot keeps its content after every cell
    /// naming it is gone (so the window cache's key, which names the slot, names that content).
    #[test]
    fn forgetting_cells_keeps_their_copies() {
        let mut g = GfxState::default();
        let mut m = SightMemory::default();
        paint(&mut g, 5, 1);
        let a = m.freeze(&g, 5).quiet();
        let now = FORGET_DAYS * 1440;
        m.remember((0, 1, 1), layer(5, a), 0);
        m.remember((0, 2, 1), layer(5, a), now);
        m.sweep(now);
        assert!(!m.holds_on(0, 1, 1), "five days old: forgotten");
        assert!(m.holds_on(0, 2, 1), "seen now: kept");
        m.remember((0, 3, 1), layer(5, a), 0);
        m.sweep(now + 59);
        assert!(m.holds_on(0, 3, 1), "the same hour: no sweep");
        m.sweep(now + 60);
        assert!(!m.holds_on(0, 3, 1), "the next hour: forgotten");
        assert_eq!(m.copies(), 1, "the sweep drops no copy");
        m.remember((0, 4, 1), layer(5, a), now);
        m.fill_synthetic(MAX_CELLS, 1, 0, 7, now + 1);
        m.cap();
        assert_eq!(m.len(), MAX_CELLS - MAX_CELLS / 8, "the cap: seven eighths");
        assert!(
            !m.holds_on(0, 2, 1) && !m.holds_on(0, 4, 1),
            "the cap: the oldest sights, the copy's owners, forgotten"
        );
        assert!(!m.records().any(|r| r == 5), "no cell names the copy");
        assert_eq!(m.copies(), 1, "the cap drops no copy");
        assert_eq!(colour_of(&m, a), Some(1), "its content");
        assert_eq!(m.freeze(&g, 5), Some(a), "taken again: the same slot");
    }

    /// Spec 18 (the compact cells, the review's lifecycle): one cell, `(0, 2, 1)`, through every
    /// path. Of one layer it keeps it in its entry; seen again with several (1 to many) it holds
    /// them all in its stack; seen again with one, the stack goes; with several again, evicted by
    /// the cap as the oldest sight, it goes with its stack; with several again, forgotten by
    /// [`SightMemory::forget`] (what [`SightMemory::layers`] calls when the copy store is full),
    /// the same; with several again, put with no layer, the same; with several again, forgotten
    /// by the sweep, the same. Another cell of one layer is never stacked.
    #[test]
    fn one_cell_through_one_and_many_layers_and_every_way_out() {
        let l = |record| Layer {
            record,
            masktype: 0,
            frame: 0,
            frozen: None,
        };
        let cell = (0, 2, 1);
        let mut m = SightMemory::default();
        m.remember((0, 1, 1), vec![l(1)], 0);
        m.remember(cell, vec![l(1)], 0);
        assert_eq!(m.stacked(), 0, "cells of one layer: no stack");
        m.remember(cell, vec![l(1), l(2), l(3)], 1);
        assert_eq!(
            m.at(2, 1),
            Some(&[l(1), l(2), l(3)][..]),
            "1 to many: all its layers"
        );
        assert!(m.stacks.contains_key(&cell_key(cell)), "in the side table");
        m.remember(cell, vec![l(4)], 2);
        assert_eq!(m.at(2, 1), Some(&[l(4)][..]), "many to 1: the one");
        assert_eq!(m.stacked(), 0, "many to 1: out of the side table");
        m.remember(cell, vec![l(5), l(6)], 3);
        m.remember((0, 1, 1), vec![l(1)], 10);
        m.fill_synthetic(MAX_CELLS, 1, 0, 7, 10);
        m.cap();
        assert!(!m.holds_on(0, 2, 1), "the cap: the oldest sight, evicted");
        assert!(
            m.at(2, 1).is_none() && m.stacked() == 0,
            "the cap: its stack with it"
        );
        let mut m = SightMemory::default();
        m.remember(cell, vec![l(5), l(6)], 3);
        m.forget(cell_key(cell));
        assert!(!m.holds_on(0, 2, 1), "forget: gone");
        assert!(m.stacks.is_empty(), "forget: its stack with it");
        m.remember(cell, vec![l(5), l(6)], 3);
        m.put(cell_key(cell), &[], 4);
        assert!(!m.holds_on(0, 2, 1), "put with no layer: gone");
        assert!(m.stacks.is_empty(), "put with no layer: its stack with it");
        m.remember(cell, vec![l(5), l(6)], 3);
        m.sweep(3 + FORGET_DAYS * 1440);
        assert!(!m.holds_on(0, 2, 1), "the sweep: five days old, gone");
        assert!(m.stacks.is_empty(), "the sweep: its stack with it");
    }

    /// Spec 18 (the review's u32 minute): the horizon of a cell's minute, `u32::MAX`, falls in the
    /// calendar's year 8 876. A sight five days before it ages through days 0..4 (a minute short
    /// of each next day still the day before) and is forgotten on day 5 as any. A sight less than
    /// five days before it stops at the day it has at the horizon, the time saturating there, and
    /// never reaches day 5; a sight at or past it stays at day 0: the documented limits.
    #[test]
    fn a_sight_before_the_minute_horizon_ages_through_day_5() {
        use crate::actors::Calendar;
        let horizon = u64::from(u32::MAX);
        let year = |year| Calendar {
            minute: 0,
            hour: 0,
            day: 1,
            month: 1,
            year,
        };
        assert!(
            minutes(&year(8876)) < horizon,
            "the year 8 876 starts before it"
        );
        assert!(
            minutes(&year(8877)) > horizon,
            "the year 8 877 starts past it"
        );
        let seen = horizon - FORGET_DAYS * 1440;
        let mut m = SightMemory::default();
        let ground = Layer {
            record: 1,
            masktype: 0,
            frame: 0,
            frozen: None,
        };
        m.remember((0, 1, 1), vec![ground], seen);
        let kept = m.cells[&cell_key((0, 1, 1))].seen;
        assert_eq!(u64::from(kept), seen, "kept exactly");
        for d in 0..FORGET_DAYS {
            let day = seen + d * 1440;
            assert_eq!(age(kept, day), Some(d as u8), "day {d}");
            assert_eq!(
                age(kept, day + 1439),
                Some(d as u8),
                "day {d}, a minute short"
            );
        }
        assert_eq!(age(kept, horizon), None, "day 5: none");
        m.sweep(horizon);
        assert!(!m.holds_on(0, 1, 1), "day 5: forgotten");
        let late = minute32(horizon - 2 * 1440);
        assert_eq!(
            age(late, horizon),
            Some(2),
            "two days before it: day 2 at the horizon"
        );
        assert_eq!(
            age(late, horizon + 10 * 1440),
            Some(2),
            "and day 2 after it"
        );
        let past = minute32(horizon + 10);
        assert_eq!(past, u32::MAX, "past the horizon: saturated");
        assert_eq!(age(past, horizon + 10 * 1440), Some(0), "past it: no age");
    }

    /// Spec 18 (the review's tie-break): two memories of the same cells, all seen in one minute
    /// and put in in opposite orders, keep the same cells past the cap: the cutoff minute's
    /// sights go by key, the lowest first, not by their tables' order.
    #[test]
    fn the_cap_breaks_ties_by_key() {
        let layer = Layer {
            record: 1,
            masktype: 0,
            frame: 0,
            frozen: None,
        };
        let n = MAX_CELLS + 1000;
        let key = |i: usize| cell_key((0, (i % 1024) as u16, (i / 1024) as u16));
        let (mut a, mut b) = (SightMemory::default(), SightMemory::default());
        for i in 0..n {
            a.put(key(i), &[layer], 5);
            b.put(key(n - 1 - i), &[layer], 5);
        }
        a.cap();
        b.cap();
        assert_eq!(a.len(), MAX_CELLS - MAX_CELLS / 8, "seven eighths");
        assert!(a.keys() == b.keys(), "the same cells kept");
        assert!(!a.holds_on(0, 0, 0), "the lowest key forgotten");
        let last = n - 1;
        assert!(
            a.holds_on(0, (last % 1024) as u16, (last / 1024) as u16),
            "the highest kept"
        );
    }

    /// Spec 18 (the review's full compare): a copy of other content filed under the record and
    /// the content hash of the record's pixels now (a forced collision) is not taken: the content
    /// gets its own copy.
    #[test]
    fn a_hash_collision_takes_no_copy_of_other_content() {
        let mut g = GfxState::default();
        let mut m = SightMemory::default();
        paint(&mut g, 5, 1);
        let rec = &g.records[5];
        let hash = content_hash(&rec.pixels, &rec.transparent);
        m.frozen.push(Frozen {
            record: 5,
            pixels: vec![2; 256],
            transparent: vec![false; 256],
        });
        m.by_content.entry((5, hash)).or_default().push(0);
        let a = m.freeze(&g, 5).quiet();
        assert!(a != 0, "the colliding copy of other content: not taken");
        assert_eq!(
            colour_of(&m, a),
            Some(1),
            "a copy of the record's own content"
        );
        assert_eq!(
            m.by_content[&(5, hash)],
            vec![0, a],
            "both under the one hash"
        );
    }

    /// Spec 18 (the review's saturation, item 1): a store of [`MAX_COPIES`] takes no new copy
    /// (the caller forgets the cell instead: no remembered layer draws a rewritten record live),
    /// and still gives the copies it holds, at a new version too.
    #[test]
    fn a_full_store_takes_no_new_copy_and_gives_the_ones_it_holds() {
        let mut g = GfxState::default();
        let mut m = SightMemory::default();
        m.fill_copies(MAX_COPIES - 1, 9);
        paint(&mut g, 5, 1);
        let a = m.freeze(&g, 5).quiet();
        assert_eq!(usize::from(a), MAX_COPIES - 1, "the last slot");
        assert_eq!(m.copies(), MAX_COPIES);
        paint(&mut g, 5, 2);
        assert_eq!(m.freeze(&g, 5), None, "full: no new copy");
        assert_eq!(m.copies(), MAX_COPIES);
        paint(&mut g, 5, 1);
        assert_eq!(m.freeze(&g, 5), Some(a), "a content held: its copy");
        assert_eq!(m.freeze(&g, 5), Some(a), "again at that version");
    }

    /// Past [`MAX_CELLS`] the oldest sights go first, down to seven eighths, also when they all
    /// share one minute; the newest stay.
    #[test]
    fn the_cap_forgets_the_oldest_sights_first() {
        let mut m = SightMemory::default();
        m.fill_synthetic(MAX_CELLS, 0, 0, 1, 10);
        m.fill_synthetic(1000, 1, 0, 1, 20);
        m.cap();
        assert_eq!(m.len(), MAX_CELLS - MAX_CELLS / 8, "seven eighths");
        assert!(
            (0..1000).all(|i| m.holds_on(1, i % 1024, i / 1024)),
            "the newest kept"
        );
    }

    /// One cell in this many has a stack of two layers (its vector's capacity 4) in
    /// [`the_bounds_hold_the_heap_under_16_mib`]: 1 %, about twice the 0.52 % the walk over the
    /// owner's land holds.
    const STACKED_LAND: usize = 100;

    /// One cell in this many, about 1.43 %: 3 275 stacks at the run's high-water (3 268 in Wide),
    /// under the 3 326 [computed] the 16 MiB ceiling holds with [`MAX_COPIES`] copies once the
    /// stacks' table has stepped to 8 192 buckets (spec 18, Bounds); 1 648 bytes under it (1 872
    /// in Wide), as the test prints. The 3 326 is exact for the test's state, the latest index
    /// and the scratch empty; with the latest index and the scratch as play fills them, 0.1 to
    /// 0.9 KB more, it is about 3 300 stacks [computed].
    const STACKED_MOST: usize = 70;

    /// Whether std's hashbrown groups its control bytes by 8 on this target, as it does on
    /// aarch64 (NEON) and wasm32 (the generic 64-bit word); false on x86 and x86_64, whose
    /// groups are SSE2's 16 bytes, and on every other target, not considered here (4-byte
    /// groups on other 32-bit targets, 16 on loongarch64 with LSX). With 16-byte groups an erase
    /// leaves a deleted entry far less often (hashbrown 0.16.1 src/raw/mod.rs:3297-3302), and
    /// what rests on deleted entries piling up (the stacks' table stepping to 8 192 buckets, the
    /// cells' table's rehash at 2^19) may not come within the tests' runs: those asserts are
    /// asserted only with 8-byte groups, and have not been run with other widths.
    const EIGHT_BYTE_GROUPS: bool = cfg!(any(target_arch = "aarch64", target_arch = "wasm32"));

    /// What [`the_bounds_hold_the_heap_under_16_mib`] measures with one cell in `every` stacked:
    /// the most cells held at any time, the cell table's buckets, the highest heap retained over
    /// the run (after each render's puts, before its cap: the most cells and stacks), the stacks
    /// at that sample, and the stacks' table's buckets at the end.
    fn bound_with_stacks(every: usize) -> (usize, usize, usize, usize, usize) {
        let l = |record| Layer {
            record,
            masktype: 0,
            frame: 0,
            frozen: None,
        };
        let mut m = SightMemory::default();
        m.fill_copies(MAX_COPIES, 9);
        let (mut put, mut most, mut minute, mut high, mut stacks) = (0usize, 0, 0, 0, 0);
        while put < 2 * MAX_CELLS {
            minute += 1;
            for _ in 0..BOXES {
                let (z, i) = ((put >> 20) as u8, put & 0xf_ffff);
                let k = cell_key((z, (i & 0x3ff) as u16, (i >> 10) as u16));
                match put % every {
                    0 => m.put(k, &[l(1), l(2)], minute),
                    _ => m.put(k, &[l(1)], minute),
                }
                put += 1;
                most = most.max(m.len());
            }
            let bytes = m.heap_bytes();
            if bytes > high {
                (high, stacks) = (bytes, m.stacked());
            }
            m.cap();
        }
        let stack_buckets = (m.stacks.capacity() * 8 / 7).next_power_of_two();
        println!(
            "one cell in {every} stacked: {put} cells remembered, at most {most} held, {} \
             evicted; {} cell buckets, {stack_buckets} stack buckets; at most {high} heap bytes \
             retained (with {stacks} stacks and {} copies), {} under 16 MiB",
            m.evicted(),
            m.cell_buckets(),
            m.copies(),
            (16usize << 20) as i64 - high as i64
        );
        (most, m.cell_buckets(), high, stacks, stack_buckets)
    }

    /// Spec 18's bounds as retained heap bytes ([`SightMemory::heap_bytes`], this build's
    /// layouts): with [`MAX_COPIES`] copies, renders of a view's worth of new cells each
    /// ([`BOXES`]), each followed by [`SightMemory::cap`] as [`SightMemory::layers`] calls it,
    /// until twice [`MAX_CELLS`] cells were remembered (the cap acting several times); one cell
    /// in [`STACKED_LAND`] with a stack of two layers (its vector's capacity 4), then one in
    /// [`STACKED_MOST`]. The highest heap retained over the run, taken after each render's puts
    /// and before its cap, stays under 16 MiB, spec 18's ceiling; the cells are at most
    /// [`MAX_CELLS`] at any time (taken after each put) and the table at most 2^19 buckets (21
    /// bytes each). At one in [`STACKED_MOST`], with 8-byte control groups ([`EIGHT_BYTE_GROUPS`];
    /// not asserted with other widths), the stacks' table is at 8 192 buckets, and the ceiling's
    /// stack count [computed] from the high-water sample (its bytes but 32 a stack, then 32
    /// bytes a stack up to 16 MiB) is 3 326: the boundary spec 18 states, for the test's state.
    /// A change of either bound, of a cell's size or of the stacks has to move it.
    /// The peak of the table's growth is the integration test `sight_memory_peak`'s.
    #[test]
    fn the_bounds_hold_the_heap_under_16_mib() {
        for every in [STACKED_LAND, STACKED_MOST] {
            let (most, buckets, high, stacks, stack_buckets) = bound_with_stacks(every);
            assert!(high < 16 << 20, "one in {every} stacked: under 16 MiB");
            assert!(stacks <= 3326, "the stacks the ceiling holds");
            assert!(most <= MAX_CELLS, "at most MAX_CELLS cells at any time");
            assert!(buckets <= 1 << 19, "at most 2^19 buckets");
            if every == STACKED_MOST && EIGHT_BYTE_GROUPS {
                assert_eq!(
                    stack_buckets, 8192,
                    "the stacks' table stepped to 8 192 buckets"
                );
                let fixed = high - 32 * stacks;
                let most_stacks = ((16 << 20) - 1 - fixed) / 32;
                assert_eq!(
                    most_stacks, 3326,
                    "the ceiling's stack count, for this state"
                );
            }
        }
    }

    /// Spec 18 (the review's churn; capacity-recovery coverage): renders of a view's worth of new
    /// cells each, each followed by [`SightMemory::cap`], until the cells' table has grown to
    /// 2^19 buckets (its capacity past 2^18's 229 376), its deleted entries have taken some of its
    /// room (its capacity under 2^19's 458 752) and its capacity is back at 458 752 with a rise of
    /// more than [`BOXES`] since the sample before. Since the cap never raises the capacity and
    /// every put is a new key, the render's [`BOXES`] inserts between two samples can take back
    /// at most that many deleted entries, so the larger rise needs a rehash; that it is in place,
    /// allocating nothing, rests on hashbrown's rule cited at [`MAX_CELLS`] (the test sees the
    /// capacity, not allocations). The table never passes 2^19 buckets on the way. Prints the
    /// cells put and the rise: in one run (std's `HashMap` is seeded anew each process), about
    /// 1.76 million cells in Original on this aarch64 machine, whose control groups are 8 bytes
    /// (NEON, as on wasm32). Not run with 16-byte groups (SSE2), where deleted entries form far
    /// less often and the rehash may come past the test's limit: there the rehash is not
    /// asserted ([`EIGHT_BYTE_GROUPS`]), and the print says which phase the run reached.
    #[test]
    fn the_cells_table_rehashes_in_place_at_2_19_buckets() {
        let l = Layer {
            record: 1,
            masktype: 0,
            frame: 0,
            frozen: None,
        };
        let full = (1 << 19) / 8 * 7;
        let mut m = SightMemory::default();
        let (mut put, mut minute, mut phase, mut prev, mut rise) = (0usize, 0, 0, 0, 0);
        while phase < 3 && put < 32 * MAX_CELLS {
            minute += 1;
            for _ in 0..BOXES {
                let (z, i) = (((put >> 20) % 200) as u8, put & 0xf_ffff);
                m.put(
                    cell_key((z, (i & 0x3ff) as u16, (i >> 10) as u16)),
                    &[l],
                    minute,
                );
                put += 1;
            }
            let c = m.cells.capacity();
            assert!(c <= full, "at most 2^19 buckets");
            phase = match phase {
                0 if c > MAX_CELLS => 1,
                1 if c < full => 2,
                2 if c == full && c - prev > BOXES => {
                    rise = c - prev;
                    3
                }
                p => p,
            };
            prev = c;
            m.cap();
        }
        match phase {
            3 => println!(
                "{put} cells put to the first rehash at 2^19 buckets, the capacity up {rise}"
            ),
            p => println!("{put} cells put, no rehash at 2^19 buckets seen (phase {p} of 3)"),
        }
        if EIGHT_BYTE_GROUPS {
            assert_eq!(phase, 3, "grown, deleted entries taken, rehashed");
        }
    }

    /// The fade's days: whole days of 1440 minutes, none from [`FORGET_DAYS`]; the pattern keeps
    /// 16, 8, 4, 2, 1 of each 4 × 4 block on days 0..4, each a subset of the day before's.
    #[test]
    fn the_fade_halves_each_day_in_nested_patterns() {
        assert_eq!(fade_of(0, 1439), Some(0));
        assert_eq!(fade_of(0, 1440), Some(1));
        assert_eq!(fade_of(0, FORGET_DAYS * 1440 - 1), Some(4));
        assert_eq!(fade_of(0, FORGET_DAYS * 1440), None);
        assert_eq!(fade_of(100, 50), Some(0), "a clock set back: no age");
        for d in 0..5u8 {
            let kept = (0..16).filter(|&i| keeps(d, i % 4, i / 4)).count();
            assert_eq!(kept, 16 >> d, "day {d}");
            if d > 0 {
                assert!((0..16).all(|i| !keeps(d, i % 4, i / 4) || keeps(d - 1, i % 4, i / 4)));
            }
        }
        assert!((0..16).all(|i| !keeps(5, i % 4, i / 4)), "day 5: none");
    }
}

// Spec 18 (saved with the game, unit C3): the file a Save writes beside the savegame.
mod saved;
pub use saved::{MAX_SAVED_LAYERS, MAX_SIGHT_FILE_BYTES, SightFileError, read_sight};

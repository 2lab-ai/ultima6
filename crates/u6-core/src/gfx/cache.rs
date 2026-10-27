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
    /// sight until its cells are seen again, as the original's).
    pub fn forget_sight(&mut self) {
        self.memory = SightMemory::default();
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
        self.memory.frozen.get(slot)?.as_ref()
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
/// [`MAX_CELLS`] cells are kept, the oldest sight going first. Load state 0x105350 forgets it all
/// ([`RenderCache::forget_sight`]); the window reset 0x133a96 does not. Never saved: it lives in
/// the [`RenderCache`], outside the game state's equality.
///
/// The look is the one of the last sight: a layer keeps its record and masktype resolved (an
/// animdata substitution, 0x152cd2, changes only which record a tile draws) and the overlay frame
/// then; a record whose pixels the shore merge rewrites in place (0x152dac, the 32 shore tiles'
/// records, the only writer of records after the load: the module docs), and any record
/// rewritten so far (its version not 0, whichever tile draws it now), is kept as a copy of its
/// pixels then ([`Frozen`]). The copies are shared by record and content, counted by the layers
/// that name them, freed with the last, and at most [`MAX_COPIES`]: past it a layer keeps the
/// record itself.
#[derive(Debug)]
pub struct SightMemory {
    cells: HashMap<(u8, u16, u16), Remembered>,
    /// An unlit cell in sight (0x1bc) shows its remembered sight: [`MEMORY_IN_DARKNESS`].
    darkness: bool,
    /// The copies of rewritten records' pixels, by slot ([`Layer::frozen`]).
    frozen: Vec<Option<Frozen>>,
    /// The free slots of `frozen`.
    free: Vec<u16>,
    /// The copies by record and content hash.
    by_content: HashMap<(u16, u64), Vec<u16>>,
    /// Per record, its version and the slot of its copy at that version, for the last copy
    /// taken of it (the copy a cell in sight takes again at every render).
    latest: HashMap<u16, (u32, u16)>,
    /// Copies made so far: the generation of the next ([`Frozen::generation`]).
    made: u32,
    /// The game hour ([`minutes`] / 60) of the last sweep of the forgotten cells.
    swept: Option<u64>,
    /// Scratch: the layers of a cell being remembered.
    scratch: Vec<Layer>,
}

impl Default for SightMemory {
    /// Nothing remembered, darkness as [`MEMORY_IN_DARKNESS`].
    fn default() -> Self {
        Self {
            cells: HashMap::new(),
            darkness: MEMORY_IN_DARKNESS,
            frozen: Vec::new(),
            free: Vec::new(),
            by_content: HashMap::new(),
            latest: HashMap::new(),
            made: 0,
            swept: None,
            scratch: Vec::new(),
        }
    }
}

/// One remembered cell: its layers and the time of its last sight ([`minutes`]).
#[derive(Debug, Default)]
struct Remembered {
    seen: u64,
    layers: Vec<Layer>,
}

/// A record's pixels and transparency as a remembered layer shows them: the record they were
/// taken from, their hash, the layers that name them, and the copy's generation (no two copies
/// share one: the window cache's key names it, [`WindowKey::layer`]).
#[derive(Debug)]
struct Frozen {
    record: u16,
    hash: u64,
    refs: u32,
    generation: u32,
    pixels: Vec<u8>,
    transparent: Vec<bool>,
}

/// [`u6_render::Src::id`] of copy slot 0 of the remembered sight ([`SightMemory`]), with
/// [`u6_render::Kind::Tile`]: slot n is `FROZEN_BASE + n`, past every record (the HD
/// compositor's images take it from [`RenderCache::frozen_image`]).
pub const FROZEN_BASE: u16 = TILE_COUNT as u16;

/// Spec 18: the most cells the remembered sight keeps (the oldest sight is forgotten first).
pub const MAX_CELLS: usize = 1 << 17;

/// Spec 18: the most copies of rewritten records it keeps (about 0.5 KiB each).
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
    pub(super) ver: u32,
    pub(super) frozen: Option<u16>,
}

impl Layer {
    /// Word `w` as the blitter draws it now ([`GfxState::record`]), not yet copied.
    fn of(gfx: &GfxState, w: u16) -> Layer {
        let t = usize::from(w & 0x7ff);
        let record = gfx.record_of[t];
        Layer {
            record,
            masktype: gfx.masktype[t],
            frame: gfx.frame,
            ver: gfx.cache.version(usize::from(record)),
            frozen: None,
        }
    }

    /// The pixels and transparency the layer draws and the source it names them by: the copy's
    /// ([`FROZEN_BASE`] + its slot, version 0: a copy never changes), else the record's at its
    /// version now.
    pub(super) fn image<'a>(&self, gfx: &'a GfxState) -> Option<(&'a [u8], &'a [bool], u16, u32)> {
        match self.frozen {
            Some(slot) => {
                let f = gfx.cache.memory.frozen.get(usize::from(slot))?.as_ref()?;
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
        let cell = self.cells.get(&Self::key(state, at))?;
        let fade = fade_of(cell.seen, minutes(&state.actors.calendar))?;
        (!cell.layers.is_empty()).then_some((fade, cell.layers.as_slice()))
    }

    /// The slot of a copy of `record`'s pixels now, one more layer naming it: the last copy taken
    /// of the record at this version, else one of the same content, else a new one; `None` at
    /// [`MAX_COPIES`].
    fn freeze(&mut self, gfx: &GfxState, record: u16) -> Option<u16> {
        let r = usize::from(record);
        let ver = gfx.cache.version(r);
        let slot = match self.latest.get(&record) {
            Some(&(v, slot)) if v == ver => slot,
            _ => {
                let rec = &gfx.records[r];
                let hash = content_hash(&rec.pixels, &rec.transparent);
                let same = self.by_content.get(&(record, hash)).and_then(|slots| {
                    slots.iter().copied().find(|&s| {
                        let f = self.frozen[usize::from(s)].as_ref();
                        f.is_some_and(|f| {
                            f.pixels == rec.pixels && f.transparent == rec.transparent
                        })
                    })
                });
                let slot = match same {
                    Some(slot) => slot,
                    None => {
                        if self.frozen.len() - self.free.len() >= MAX_COPIES {
                            return None;
                        }
                        let copy = Frozen {
                            record,
                            hash,
                            refs: 0,
                            generation: self.made,
                            pixels: rec.pixels.clone(),
                            transparent: rec.transparent.clone(),
                        };
                        self.made = self.made.wrapping_add(1);
                        let slot = match self.free.pop() {
                            Some(free) => {
                                self.frozen[usize::from(free)] = Some(copy);
                                free
                            }
                            None => {
                                self.frozen.push(Some(copy));
                                (self.frozen.len() - 1) as u16
                            }
                        };
                        self.by_content
                            .entry((record, hash))
                            .or_default()
                            .push(slot);
                        slot
                    }
                };
                self.latest.insert(record, (ver, slot));
                slot
            }
        };
        if let Some(f) = self.frozen[usize::from(slot)].as_mut() {
            f.refs += 1;
        }
        Some(slot)
    }

    /// One layer less names the copies `layers` name; a copy no layer names is freed.
    fn release(&mut self, layers: &[Layer]) {
        for slot in layers.iter().filter_map(|l| l.frozen) {
            let at = usize::from(slot);
            let Some(f) = self.frozen[at].as_mut() else {
                continue;
            };
            f.refs -= 1;
            if f.refs > 0 {
                continue;
            }
            let (record, hash) = (f.record, f.hash);
            self.frozen[at] = None;
            self.free.push(slot);
            if let Some(slots) = self.by_content.get_mut(&(record, hash)) {
                slots.retain(|&s| s != slot);
                if slots.is_empty() {
                    self.by_content.remove(&(record, hash));
                }
            }
            if self.latest.get(&record).is_some_and(|&(_, s)| s == slot) {
                self.latest.remove(&record);
            }
        }
    }

    /// Forgets the cells for which `gone` holds, their copies released.
    fn forget(&mut self, gone: impl Fn(&Remembered) -> bool) {
        let keys: Vec<(u8, u16, u16)> = self
            .cells
            .iter()
            .filter(|(_, c)| gone(c))
            .map(|(&k, _)| k)
            .collect();
        for k in keys {
            if let Some(c) = self.cells.remove(&k) {
                self.release(&c.layers);
            }
        }
    }

    /// Past [`MAX_CELLS`], forgets the oldest sights down to seven eighths of it.
    fn cap(&mut self) {
        if self.cells.len() <= MAX_CELLS {
            return;
        }
        let target = MAX_CELLS - MAX_CELLS / 8;
        let mut seen: Vec<u64> = self.cells.values().map(|c| c.seen).collect();
        let k = self.cells.len() - target;
        let (_, &mut cutoff, _) = seen.select_nth_unstable(k);
        self.forget(|c| c.seen < cutoff);
        // Sights of the cutoff's own minute, as many as still go.
        let over = self.cells.len().saturating_sub(target);
        let tied: Vec<(u8, u16, u16)> = (self.cells.iter())
            .filter(|(_, c)| c.seen == cutoff)
            .map(|(&k, _)| k)
            .take(over)
            .collect();
        for k in tied {
            if let Some(c) = self.cells.remove(&k) {
                self.release(&c.layers);
            }
        }
    }

    /// The remembered sight for the render of `window` (the state's view now): with `framed`,
    /// every cell in sight remembered as the render draws it now, at the time now (in the cell's
    /// own entry; a rewritten record as a copy of its pixels now); once a game hour the cells
    /// [`FORGET_DAYS`] old forgotten, and past [`MAX_CELLS`] the oldest. Then per box, what the
    /// render draws in grey ([`SightMemory::shown`]).
    pub(super) fn layers(state: &mut State, window: &ViewWindow, framed: bool) -> Vec<Shown> {
        if framed {
            let now = minutes(&state.actors.calendar);
            let keys: Vec<Option<(u8, u16, u16)>> = cells_of(window)
                .map(|(at, base)| (!unseen(base)).then(|| Self::key(state, at)))
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
                for l in layers.iter_mut() {
                    let r = usize::from(l.record);
                    if merged.contains(&l.record) || gfx.cache.version(r) != 0 {
                        l.frozen = memory.freeze(gfx, l.record);
                    }
                }
                let cell = memory.cells.entry(key).or_default();
                cell.seen = now;
                std::mem::swap(&mut cell.layers, &mut layers);
                memory.release(&layers);
                memory.scratch = layers;
            }
            if memory.swept != Some(now / 60) {
                memory.swept = Some(now / 60);
                memory.forget(|c| fade_of(c.seen, now).is_none());
            }
            memory.cap();
            gfx.cache.memory = memory;
        }
        let memory = &state.gfx.cache.memory;
        cells_of(window)
            .map(|(at, base)| match memory.shown(state, at, base) {
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
        self.frozen.iter().flatten().count()
    }

    /// Whether world cell (x, y) is remembered now, on any level (the tests read it).
    #[cfg(test)]
    pub(super) fn holds(&self, x: u16, y: u16) -> bool {
        self.cells.keys().any(|&(_, kx, ky)| (kx, ky) == (x, y))
    }

    /// Whether world cell (x, y) of level `z` is remembered now (the tests read it).
    #[cfg(test)]
    pub(super) fn holds_on(&self, z: u8, x: u16, y: u16) -> bool {
        self.cells.contains_key(&(z, x, y))
    }

    /// World cell (x, y)'s remembered layers, on any level (the tests read them).
    #[cfg(test)]
    pub(super) fn at(&self, x: u16, y: u16) -> Option<&[Layer]> {
        let (_, c) = self
            .cells
            .iter()
            .find(|&(&(_, kx, ky), _)| (kx, ky) == (x, y))?;
        Some(c.layers.as_slice())
    }

    /// The records of every remembered layer (the tests read them).
    #[cfg(test)]
    pub(super) fn records(&self) -> impl Iterator<Item = u16> + '_ {
        self.cells
            .values()
            .flat_map(|c| &c.layers)
            .map(|l| l.record)
    }

    /// Sets whether darkness shows the remembered sight (the tests try both values of
    /// [`MEMORY_IN_DARKNESS`]).
    #[cfg(test)]
    pub(super) fn set_darkness(&mut self, on: bool) {
        self.darkness = on;
    }

    /// `n` synthetic remembered cells on level `z` from row `y0` on, each one layer of record
    /// `record` seen at `seen` (the cost receipts fill the memory with them).
    #[cfg(test)]
    pub(super) fn fill_synthetic(&mut self, n: usize, z: u8, y0: u16, record: u16, seen: u64) {
        for i in 0..n {
            let layers = vec![Layer {
                record,
                masktype: 0,
                frame: 0,
                ver: 0,
                frozen: None,
            }];
            let key = (z, (i % 1024) as u16, y0.wrapping_add((i / 1024) as u16));
            self.cells.insert(key, Remembered { seen, layers });
        }
    }

    /// The bytes the memory holds on the heap, from its tables' capacities (the cost receipts).
    #[cfg(test)]
    pub(super) fn heap_bytes(&self) -> usize {
        let entry = std::mem::size_of::<((u8, u16, u16), Remembered)>() + 1;
        let layers: usize = self.cells.values().map(|c| c.layers.capacity()).sum();
        let copies: usize = self
            .frozen
            .iter()
            .flatten()
            .map(|f| f.pixels.capacity() * 2)
            .sum();
        self.cells.capacity() * entry + layers * std::mem::size_of::<Layer>() + copies
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
    /// of its remembered layers (spec 18, `memory`: per box, empty with the remembered sight off),
    /// with some their fade and each as [`WindowKey::layer`] has it, the count of its object words, each word as
    /// [`WindowKey::word`] has it (with a clock word the hour and the minute), then the target
    /// cursor's word when it is in the box, then the box's border word. `None` when the render
    /// cannot be remembered: the sparkle, the dissolve or XOR plots on, the cursor outside the
    /// cells, an overlay or clock hands that would leave their box.
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
    /// the record, the masktype, whether it draws a copy and the version of the pixels it draws
    /// (the copy's generation, which no other copy has, else the record's version now), and when
    /// the overlay is drawn (masktype bit 2, an overlay) the first frame with the rows of the
    /// frame kept. The box's fade is read before its layers ([`WindowKey::of`]).
    fn layer(&mut self, gfx: &GfxState, l: Layer) {
        let r = usize::from(l.record);
        let frozen = l
            .frozen
            .and_then(|s| gfx.cache.memory.frozen.get(usize::from(s))?.as_ref());
        let ver = match (l.frozen, frozen) {
            (Some(_), Some(f)) => f.generation,
            _ => gfx.cache.version(r),
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
    use super::{FORGET_DAYS, MAX_CELLS, SightMemory, fade_of, keeps};

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

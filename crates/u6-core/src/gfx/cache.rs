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

use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use super::GfxState;
use super::view::{ViewWindow, cell_pixel};
use crate::geom::{MAP_X, MAP_Y, VIEW_H, VIEW_W};
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
}

impl Clone for RenderCache {
    /// No window memo and no counts; the records' versions and the static tables' fingerprint
    /// (the clone holds the same records and tables).
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
    /// The values the framed render of `window` reads, per box: the cell's base word and the
    /// count of its object words, each word as [`WindowKey::word`] has it (with a clock word the
    /// hour and the minute), then the target cursor's word when it is in the box, then the
    /// box's border word. `None` when the render cannot be remembered: the sparkle, the
    /// dissolve or XOR plots on, the cursor outside the cells, an overlay or clock hands that
    /// would leave their box.
    pub(super) fn of(
        state: &State,
        window: &ViewWindow,
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

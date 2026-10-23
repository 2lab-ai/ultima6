//! The neighbour-seam gate (spec 07 HD2 "Neighbour seams", Acceptance 4): two records drawn in
//! edge-adjacent cells of the owner's world meet without a seam the original does not have.
//!
//! - **The pairs** ([`pairs`]): every ordered (A, B, edge) of records drawn in edge-adjacent
//!   cells, B east or south of A, counted once:
//!   - in the map, the map tile of every cell as the world places its chunks, on every level,
//!     across chunk edges and across the level's wrap (the window masks its coordinates);
//!   - among the objects of the starting object lists ([`crate::game::World::objects`]), each
//!     drawn where the object draw puts its tile and its parts (0x10e73a: tile − 1 west, − 1 or
//!     − 2 north, − 3 north-west), where both shared edges are opaque on at least half their
//!     length.
//!
//!   A tile the animation replaces is each frame that replaces it (the frames below 0x200, as
//!   0x152cd2 takes them; a frame replaced in turn is its own frames), so a pair of tiles is every
//!   pair of their frames. A shore record is MMPX (no item has it) and an empty record draws
//!   nothing, so their pairs count and measure nothing more than that.
//! - **The images** ([`Side`]): each record as the page shows it at ×4 (`u6_hdpack::SCALE`): the
//!   pack's image (the ×8 master box-averaged, as `pack` writes it) if its item was accepted,
//!   else MMPX ×4 of the classic record, the compositor's own scaler ([`mmpx4`]); both composed
//!   as the compositor composes them ([`gate::shown`]).
//! - **The measure**: [`gate::neighbour_seam`], held to the `seam` threshold or the classic
//!   pair's own ratio.
//! - **The fixed point** ([`settle`]): after every item is judged, every failing pair of a pass
//!   at once sends each generated record in it back to MMPX with its whole item; the passes
//!   repeat until none fails. Fallbacks only add, and each pass judges every pair against the
//!   same images, so the result does not depend on the order of the pairs.
//!
//! Only a pair with a generated side is checked: a pair of two records MMPX draws (or the
//! scaler draws by rule) is not measured, and none may fall back. "No failing pair after the
//! fixed point" therefore says that no pair with a generated side fails, not that every edge
//! the world draws is clean. A checked pair on which nothing is measured cannot fail either;
//! it is counted apart, as unmeasured ([`Counts::unmeasured`]).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use u6_formats::gfx::{TILE_COUNT, TILE_W};
use u6_hdpack::ImageId;
use u6_render::hd::{IMAGE_MAX, Scale, mmpx};

use crate::atlas::MASTER;
use crate::game::{DOUBLE_HEIGHT, DOUBLE_WIDTH, Game};
use crate::gate::{self, Edge};
use crate::img::{self, Classic, Rgba};

#[cfg(test)]
mod tests;

/// HD pixels per classic pixel the pairs are measured at: the pack's scale, the page's S 4.
pub const K: u32 = u6_hdpack::SCALE;

/// Record B drawn on record A's `edge` side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pair {
    pub a: u16,
    pub b: u16,
    pub edge: Edge,
}

/// The pairs of the world: all of them once, and how many each source has (a pair both have is
/// in both counts).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pairs {
    pub all: Vec<Pair>,
    pub map: usize,
    pub objects: usize,
    /// Where each pair occurs: one example cell of A per source.
    pub seen: BTreeMap<Pair, Seen>,
}

/// A cell of the world: level, x, y (tiles).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct At {
    pub z: usize,
    pub x: usize,
    pub y: usize,
}

/// Where a pair occurs: A's cell in the first place each source has it, the cells of both
/// sources taken in level, row, column order (`(z, y, x)`, the least first).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    pub map: Option<At>,
    pub objects: Option<At>,
}

impl Seen {
    /// "map", "objects" or "map+objects".
    pub fn source(&self) -> &'static str {
        match (self.map, self.objects) {
            (Some(_), Some(_)) => "map+objects",
            (None, Some(_)) => "objects",
            _ => "map",
        }
    }

    /// One example cell: the map's when it has one, else the objects'.
    pub fn example(&self) -> Option<At> {
        self.map.or(self.objects)
    }
}

/// The records a tile draws: the frames the animation replaces it with ([`Drawn::records`]).
struct Drawn {
    frames: BTreeMap<u16, Vec<u16>>,
    memo: HashMap<u16, Vec<u16>>,
}

impl Drawn {
    fn of(game: &Game) -> Drawn {
        let n = TILE_COUNT.min(game.records.len());
        let mut frames: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
        for slot in &game.anim {
            for c in 0..=u32::from(slot.and_mask) {
                let f = slot.frame_tile(c);
                if f < 0x200 && usize::from(f) < n {
                    frames.entry(slot.tile_to_animate).or_default().push(f);
                }
            }
        }
        for v in frames.values_mut() {
            v.sort_unstable();
            v.dedup();
        }
        Drawn {
            frames,
            memo: HashMap::new(),
        }
    }

    /// The records tile `t` draws: itself when no frame replaces it, else what each of its
    /// frames draws (a frame that leads back to a tile being expanded draws itself).
    fn records(&mut self, t: u16) -> Vec<u16> {
        if let Some(v) = self.memo.get(&t) {
            return v.clone();
        }
        let mut out = BTreeSet::new();
        self.expand(t, &mut Vec::new(), &mut out);
        let v: Vec<u16> = out.into_iter().collect();
        self.memo.insert(t, v.clone());
        v
    }

    fn expand(&self, t: u16, path: &mut Vec<u16>, out: &mut BTreeSet<u16>) {
        match self.frames.get(&t) {
            Some(fs) if !path.contains(&t) => {
                path.push(t);
                for &f in fs {
                    self.expand(f, path, out);
                }
                path.pop();
            }
            _ => {
                out.insert(t);
            }
        }
    }
}

/// The cells an object's tile `t` is drawn in, relative to the object's, with the tile drawn
/// there (`u6_core::objects::add_object_tiles`, 0x10e73a).
fn parts(game: &Game, t: u16) -> Vec<(i32, i32, u16)> {
    let f = game.flags2.get(usize::from(t)).copied().unwrap_or(0);
    let part = |n: u16| t.wrapping_sub(n) & 0x7ff;
    let mut v = vec![(0, 0, t)];
    if f & DOUBLE_WIDTH != 0 {
        v.push((-1, 0, part(1)));
        if f & DOUBLE_HEIGHT != 0 {
            v.push((0, -1, part(2)));
            v.push((-1, -1, part(3)));
        }
    } else if f & DOUBLE_HEIGHT != 0 {
        v.push((0, -1, part(1)));
    }
    v
}

/// Opaque pixels of record `r` along its side `edge` (true) or the side opposite it (false).
fn opaque_along(game: &Game, r: u16, edge: Edge, own: bool) -> usize {
    let Some(rec) = game.records.get(usize::from(r)) else {
        return 0;
    };
    let last = TILE_W - 1;
    let at = |i: usize| match (edge, own) {
        (Edge::East, true) => i * TILE_W + last,
        (Edge::East, false) => i * TILE_W,
        (Edge::South, true) => last * TILE_W + i,
        (Edge::South, false) => i,
    };
    (0..TILE_W)
        .filter(|&i| rec.transparent.get(at(i)).is_some_and(|t| !t))
        .count()
}

/// Objects meet when both shared edges are opaque on at least half their length.
fn half_opaque(game: &Game, a: u16, b: u16, edge: Edge) -> bool {
    let half = TILE_W.div_ceil(2);
    opaque_along(game, a, edge, true) >= half && opaque_along(game, b, edge, false) >= half
}

/// Every pair of the world (module docs).
pub fn pairs(game: &Game) -> Pairs {
    let world = &game.world;
    let mut drawn = Drawn::of(game);
    let next = |x: usize, y: usize, s: usize| {
        [
            (Edge::East, ((x + 1) % s, y)),
            (Edge::South, (x, (y + 1) % s)),
        ]
    };

    // A's cell, in level, row, column order: the first of a pair's cells is the least.
    let first = |seen: &mut BTreeMap<Pair, (usize, usize, usize)>, p: Pair, at| {
        let e = seen.entry(p).or_insert(at);
        *e = (*e).min(at);
    };

    // The map: the distinct (tile, tile, edge) first, at their first cell, then their records.
    let mut tiles: BTreeMap<(u16, u16, Edge), (usize, usize, usize)> = BTreeMap::new();
    for (z, level) in world.levels.iter().enumerate() {
        let s = level.tiles();
        for y in 0..s {
            for x in 0..s {
                let Some(a) = world.tile(z, x, y) else {
                    continue;
                };
                for (edge, (bx, by)) in next(x, y, s) {
                    if let Some(b) = world.tile(z, bx, by) {
                        tiles.entry((a, b, edge)).or_insert((z, y, x));
                    }
                }
            }
        }
    }
    let mut map: BTreeMap<Pair, (usize, usize, usize)> = BTreeMap::new();
    for (&(a, b, edge), &at) in &tiles {
        let rb = drawn.records(b);
        for ra in drawn.records(a) {
            for &rb in &rb {
                first(&mut map, Pair { a: ra, b: rb, edge }, at);
            }
        }
    }

    // The objects: the tiles drawn in each cell (level, row, column), then the pairs of
    // neighbouring cells.
    let mut cells: BTreeMap<(usize, usize, usize), BTreeSet<u16>> = BTreeMap::new();
    for o in &world.objects {
        let z = usize::from(o.z);
        let Some(level) = world.levels.get(z) else {
            continue;
        };
        let s = level.tiles() as i64;
        if s == 0 {
            continue;
        }
        let wrap = |v: i64| v.rem_euclid(s) as usize;
        for (dx, dy, t) in parts(game, o.tile) {
            let cell = (
                z,
                wrap(i64::from(o.y) + i64::from(dy)),
                wrap(i64::from(o.x) + i64::from(dx)),
            );
            cells.entry(cell).or_default().insert(t);
        }
    }
    let mut objects: BTreeMap<Pair, (usize, usize, usize)> = BTreeMap::new();
    for (&(z, y, x), here) in &cells {
        let s = world.levels[z].tiles();
        for (edge, (bx, by)) in next(x, y, s) {
            let Some(there) = cells.get(&(z, by, bx)) else {
                continue;
            };
            for &ta in here {
                for &tb in there {
                    let rb = drawn.records(tb);
                    for ra in drawn.records(ta) {
                        for &rb in &rb {
                            if half_opaque(game, ra, rb, edge) {
                                first(&mut objects, Pair { a: ra, b: rb, edge }, (z, y, x));
                            }
                        }
                    }
                }
            }
        }
    }
    let at = |(z, y, x): (usize, usize, usize)| At { z, x, y };
    let mut seen: BTreeMap<Pair, Seen> = BTreeMap::new();
    for (&p, &c) in &map {
        seen.entry(p).or_default().map = Some(at(c));
    }
    for (&p, &c) in &objects {
        seen.entry(p).or_default().objects = Some(at(c));
    }
    Pairs {
        all: seen.keys().copied().collect(),
        map: map.len(),
        objects: objects.len(),
        seen,
    }
}

/// MMPX ×4 of a classic image (the compositor's scaler and its input words: the colour opaque,
/// a transparent pixel clear, `u6_render::hd`'s resolve). `None` for an image the compositor
/// does not scale: one of no pixel, larger than the screen ([`IMAGE_MAX`]), or whose pixels are
/// not its `width × height`.
pub fn mmpx4(c: &Classic) -> Option<Rgba> {
    gate::classic_len(c)?;
    if c.width as usize > IMAGE_MAX.0 || c.height as usize > IMAGE_MAX.1 {
        return None;
    }
    let input: Vec<u32> =
        c.px.iter()
            .map(|p| p.map_or(0, |[r, g, b]| u32::from_le_bytes([r, g, b, 255])))
            .collect();
    let mut out = Vec::new();
    mmpx(
        &input,
        c.width as usize,
        c.height as usize,
        Scale::X4,
        &mut out,
    );
    Some(Rgba {
        width: c.width * 4,
        height: c.height * 4,
        pixels: out.iter().flat_map(|w| w.to_le_bytes()).collect(),
    })
}

/// One record's images, as the page shows them ([`gate::shown`]) at ×[`K`]. Made only by
/// [`Side::new`], which checks that they fit.
#[derive(Clone, Debug)]
pub struct Side {
    classic: Classic,
    /// MMPX ×4 of the classic record: what the page draws without a pack image.
    scaled: Vec<Option<[f32; 3]>>,
    /// The item it was accepted with and the pack's image of it; `None` for a record no
    /// accepted item has.
    generated: Option<(u32, Vec<Option<[f32; 3]>>)>,
}

impl Side {
    /// `classic` drawn by MMPX, and `generated` (the item, its ×4 image) when accepted. `Err`
    /// when the classic image is not one MMPX takes ([`mmpx4`]) or the generated image is not
    /// exactly ×[`K`] of it.
    pub fn new(classic: Classic, generated: Option<(u32, &Rgba)>) -> Result<Side, String> {
        let scaled = mmpx4(&classic)
            .and_then(|m| gate::shown(&classic, &m))
            .ok_or("neighbour gate: a classic image the scaler does not take")?;
        let generated = match generated {
            Some((item, hd)) => {
                let fits = (hd.width, hd.height) == (classic.width * K, classic.height * K);
                let shown = fits.then(|| gate::shown(&classic, hd)).flatten();
                Some((
                    item,
                    shown.ok_or("neighbour gate: an accepted image not x4 of its classic image")?,
                ))
            }
            None => None,
        };
        Ok(Side {
            classic,
            scaled,
            generated,
        })
    }
}

/// The records' images the pairs name (ingest's side preparation): each record's classic
/// image, with, for a record of an accepted item (`accepted`: record → its item and ×8 master),
/// the master box-averaged to the pack's ×[`K`] as `pack` writes it. A record the game has no
/// image of has no side, and draws nothing.
pub fn sides(
    game: &Game,
    pairs: &[Pair],
    accepted: &BTreeMap<u16, (u32, &Rgba)>,
) -> Result<BTreeMap<u16, Side>, String> {
    let mut out: BTreeMap<u16, Side> = BTreeMap::new();
    for p in pairs {
        for r in [p.a, p.b] {
            if out.contains_key(&r) {
                continue;
            }
            let Some(classic) = game.classic(ImageId::Tile(r)) else {
                continue;
            };
            let pack = accepted
                .get(&r)
                .map(|&(item, master)| (item, img::shrink(master, MASTER / K)));
            let side = Side::new(classic, pack.as_ref().map(|(item, hd)| (*item, hd)))?;
            out.insert(r, side);
        }
    }
    Ok(out)
}

/// What the fixed point found, in counts only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Distinct pairs, and from which source.
    pub pairs: usize,
    pub map: usize,
    pub objects: usize,
    /// Pairs checked in the first pass: those with a generated side (no other pair is checked).
    pub checked: usize,
    /// Of the pairs checked in the first pass, those on which nothing is measured: [`settle`]
    /// counts every checked pair whose result is `None`, which is a pair with no row across
    /// the join where both sides draw (a side draws nothing along the edge, e.g. an empty
    /// record, or the two draw along it in different rows, e.g. two objects' edges each opaque
    /// on half their length), a pair naming a record with no side, or a pair whose images do
    /// not fit each other ([`gate::neighbour_seam`]). On the game's data only the first
    /// happens: every record a pair names is one the game has an image of (so it has a side),
    /// and every tile image is `TILE_W × TILE_W`, each side checked by [`Side::new`]. Such a
    /// pair cannot fail, so it is counted apart.
    pub unmeasured: usize,
    /// Pairs failing in the first pass, before any neighbour fallback (among those checked).
    pub failing_before: usize,
    /// Pairs checked, unmeasured among them, and failing, in the last pass (none fails there:
    /// it is the pass that ends the fixed point); again only pairs with a generated side.
    pub checked_after: usize,
    pub unmeasured_after: usize,
    pub failing_after: usize,
    /// Passes run (the last one finds no failing pair).
    pub passes: usize,
}

/// One pair checked in one pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Checked {
    /// The pass, 1 the first.
    pub pass: usize,
    pub pair: Pair,
    /// Both sides generated (else one generated, one drawn by MMPX).
    pub both: bool,
    /// (ratio, limit); `None` when nothing is measured, for any of the reasons
    /// [`Counts::unmeasured`] lists (on the game's data, only when no row across the join has
    /// both sides drawing).
    pub result: Option<(f32, f32)>,
    pub fails: bool,
}

/// The fixed point's result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Settled {
    pub counts: Counts,
    /// Items sent back to MMPX, with the pass that failed them (1 is the first).
    pub fallen: BTreeMap<u32, usize>,
    /// Every pair checked in the first pass, failing or not, then each failing pair of a later
    /// pass: by pass, and within a pass in the order of the pairs [`settle`] was given (it
    /// keeps its input's order; ingest gives [`Pairs::all`], which is sorted).
    pub checked: Vec<Checked>,
}

impl Settled {
    /// The failing pairs of every pass.
    pub fn failing(&self) -> impl Iterator<Item = &Checked> {
        self.checked.iter().filter(|c| c.fails)
    }
}

/// Runs the fixed point over `pairs` with the records' images `sides` and the seam threshold
/// `max_ratio` (module docs). A pair is checked only when a side is generated (its item still
/// accepted): a pair of two records MMPX draws is not measured, so no failing pair at the end
/// says that none with a generated side fails. A record without a side draws nothing.
pub fn settle(pairs: &[Pair], sides: &BTreeMap<u16, Side>, max_ratio: f32) -> Settled {
    let mut up: BTreeSet<u32> = sides
        .values()
        .filter_map(|s| s.generated.as_ref().map(|g| g.0))
        .collect();
    let mut out = Settled::default();
    loop {
        out.counts.passes += 1;
        let pass = out.counts.passes;
        let generated = |r: u16| -> Option<u32> {
            let (item, _) = sides.get(&r)?.generated.as_ref()?;
            up.contains(item).then_some(*item)
        };
        let image = |r: u16| -> Option<(&Classic, &[Option<[f32; 3]>])> {
            let s = sides.get(&r)?;
            let px = match &s.generated {
                Some((item, g)) if up.contains(item) => g,
                _ => &s.scaled,
            };
            Some((&s.classic, px))
        };
        let (mut checked, mut unmeasured, mut fails) = (0, 0, Vec::new());
        for &p in pairs {
            let (ga, gb) = (generated(p.a), generated(p.b));
            if ga.is_none() && gb.is_none() {
                continue;
            }
            checked += 1;
            let result = match (image(p.a), image(p.b)) {
                (Some(a), Some(b)) => gate::neighbour_seam(a, b, K, p.edge, max_ratio),
                _ => None,
            };
            if result.is_none() {
                unmeasured += 1;
            }
            let c = Checked {
                pass,
                pair: p,
                both: ga.is_some() && gb.is_some(),
                result,
                fails: result.is_some_and(|r| !gate::seam_passes(r)),
            };
            if c.fails {
                fails.push(p);
            }
            if pass == 1 || c.fails {
                out.checked.push(c);
            }
        }
        if pass == 1 {
            out.counts.checked = checked;
            out.counts.unmeasured = unmeasured;
            out.counts.failing_before = fails.len();
        }
        out.counts.checked_after = checked;
        out.counts.unmeasured_after = unmeasured;
        out.counts.failing_after = fails.len();
        if fails.is_empty() {
            return out;
        }
        let mut fall = BTreeSet::new();
        for &p in &fails {
            fall.extend(generated(p.a));
            fall.extend(generated(p.b));
        }
        for item in fall {
            up.remove(&item);
            out.fallen.insert(item, pass);
        }
    }
}

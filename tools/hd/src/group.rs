//! Which records go together: the plan of items. An item is what is accepted, re-rolled and
//! falls back as one (spec 07 HD2): one terrain record, one object record, one multi-record
//! object (its parts as one mosaic), every frame of one actor type, one portrait, one bitmap.
//!
//! Classes, all derived from the owner's tables at run time:
//! - **terrain**: map records a map chunk cell names, closed over the animdata frames that
//!   replace them (sources below 0x200, as 0x152cd2 takes); the 32 shore records are left out,
//!   since the shore merge rewrites them on every even tick and no pristine image can match;
//! - **actor**: the records of an actor type's frames, as many as its type has from its base
//!   tile ([`actor_layout`]; a type of unknown layout up to the next type's base tile); types
//!   whose frames overlap are one item (types: the monster list and the actors' types at the
//!   start);
//! - **multi**: a tile with the double width / height flags and its parts, composed as the
//!   object draw 0x10e73a composes them (tile − 1 west, − 1 or − 2 north, − 3 north-west);
//! - **object**: every other non-empty record (8-wide `Left` / `Right` records included: their
//!   unwritten columns are transparent like any other).

use serde::{Deserialize, Serialize};
use u6_formats::gfx::TILE_COUNT;
use u6_hdpack::{ImageId, Role};

use crate::game::{DOUBLE_HEIGHT, DOUBLE_WIDTH, Game};

/// Classes in the run order (spec 07: portraits, panel bitmaps, terrain, actors and objects).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Portrait,
    Bitmap,
    Terrain,
    Actor,
    Multi,
    Object,
}

impl Class {
    pub const ALL: [Class; 6] = [
        Class::Portrait,
        Class::Bitmap,
        Class::Terrain,
        Class::Actor,
        Class::Multi,
        Class::Object,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Class::Portrait => "portraits",
            Class::Bitmap => "bitmaps",
            Class::Terrain => "terrain records",
            Class::Actor => "actor records",
            Class::Multi => "multi-record object records",
            Class::Object => "object records",
        }
    }
}

/// A rectangle of records drawn together, row-major (`w × h` of them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub w: u32,
    pub h: u32,
    pub images: Vec<ImageId>,
}

impl Piece {
    fn single(id: ImageId) -> Piece {
        Piece {
            w: 1,
            h: 1,
            images: vec![id],
        }
    }
}

/// What is accepted or falls back as one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub class: Class,
    pub pieces: Vec<Piece>,
    /// Actor items: how the type's frames are laid out.
    pub layout: Option<Layout>,
}

/// The frames of an actor type: its base tile, how many, and how many per facing (a facing's
/// frames are consecutive; `None` when the type's layout is not known, see [`actor_layout`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub base: u16,
    pub frames: u16,
    pub per_facing: Option<u16>,
}

/// Frames and frames per facing of actor type `t`, as the original's face routine 0x11ba5e
/// sets them (`u6_core::actors::walk::face`): humanoids four facings of four, 0x16a of twelve,
/// 0x16b of three, 0x164 two random frames, the two-frame walkers four facings of two (from
/// 0x1aa with the rear half's eight more), facing-only types one per facing, 0x1a8 eight per
/// facing, 0x19b and the ship 0x19c (bow, hull and stern) pairs. `None` for every other type:
/// its frames run to the next type's base tile and are paired in order.
pub fn actor_layout(t: u16) -> Option<(u16, u16)> {
    Some(match t {
        0x178..=0x183 | 0x199 | 0x19a => (16, 4),
        0x16a => (48, 12),
        0x164 => (2, 2),
        0x16b => (12, 3),
        0x15a | 0x156 | 0x15c..=0x15f | 0x166 | 0x169 | 0x16f..=0x174 | 0x188 => (8, 2),
        0x1aa.. => (16, 2),
        0x19c => (24, 2),
        0x162 | 0x167 | 0x19e | 0x184 => (4, 1),
        0x1a8 => (32, 8),
        0x19b => (8, 2),
        _ => return None,
    })
}

impl Item {
    pub fn images(&self) -> impl Iterator<Item = ImageId> + '_ {
        self.pieces.iter().flat_map(|p| p.images.iter().copied())
    }
}

/// The plan: every item, and the records left out.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub items: Vec<Item>,
    /// Shore records (the scaler draws them, HD3).
    pub shore: usize,
    /// Records that draw nothing.
    pub empty: usize,
    /// Mosaics that shared a record with another and joined its item.
    pub overlaps: usize,
    /// Actor types whose frames overlap another type's, joined to its item.
    pub actor_overlaps: usize,
}

impl Plan {
    /// Records (or images) in items of `class`.
    pub fn count(&self, class: Class) -> usize {
        self.items
            .iter()
            .filter(|i| i.class == class)
            .map(|i| i.images().count())
            .sum()
    }

    /// The actor spans split over more than one item (none, by construction; inventory and the
    /// tests check it): an actor's records that draw are all in its item.
    pub fn split_actors(&self, game: &Game) -> usize {
        let owner = |r: u16| {
            self.items
                .iter()
                .position(|i| i.images().any(|id| id == ImageId::Tile(r)))
        };
        self.items
            .iter()
            .enumerate()
            .filter_map(|(k, i)| i.layout.map(|l| (k, l)))
            .filter(|&(k, l)| {
                (l.base..l.base.saturating_add(l.frames)).any(|r| {
                    let draws = game
                        .records
                        .get(usize::from(r))
                        .is_some_and(|rec| !rec.is_empty());
                    let shore = game.shore.contains(&r);
                    draws && !shore && owner(r).is_some_and(|o| o != k)
                })
            })
            .count()
    }

    /// A digest of every item: its class and pieces (shape and images), in order. Jobs carry
    /// it; ingest refuses jobs of another plan, since their item numbers mean other items.
    pub fn fingerprint(&self) -> String {
        let mut text = String::new();
        for item in &self.items {
            text.push_str(item.class.name());
            if let Some(l) = item.layout {
                text.push_str(&format!("|{l:?}"));
            }
            for p in &item.pieces {
                text.push_str(&format!("|{}x{}", p.w, p.h));
                for id in &p.images {
                    text.push_str(&format!(",{id:?}"));
                }
            }
            text.push('\n');
        }
        crate::work::digest(&[text.as_bytes()])
    }

    /// Items of `class`.
    pub fn groups(&self, class: Class) -> usize {
        self.items.iter().filter(|i| i.class == class).count()
    }
}

/// The parts of a tile with the double flags, row-major, the tile itself last; or `None`.
fn mosaic(game: &Game, t: usize) -> Option<Piece> {
    let f = game.flags2.get(t).copied().unwrap_or(0);
    let (w, h, n) = match (f & DOUBLE_WIDTH != 0, f & DOUBLE_HEIGHT != 0) {
        (true, true) => (2, 2, 3),
        (true, false) => (2, 1, 1),
        (false, true) => (1, 2, 1),
        (false, false) => return None,
    };
    let first = t.checked_sub(n)?;
    Some(Piece {
        w,
        h,
        images: (first..=t).map(|r| ImageId::Tile(r as u16)).collect(),
    })
}

fn record(id: ImageId) -> usize {
    match id {
        ImageId::Tile(r) => usize::from(r),
        _ => usize::MAX,
    }
}

/// Classifies everything the game has.
pub fn plan(game: &Game) -> Plan {
    let n = TILE_COUNT.min(game.records.len());
    let mut taken = vec![false; n];
    let mut plan = Plan::default();
    // An empty record is no item, but it may be a part of a mosaic (drawn as nothing).
    let empty: Vec<bool> = game.records.iter().take(n).map(|r| r.is_empty()).collect();
    for (r, &e) in empty.iter().enumerate() {
        if e {
            taken[r] = true;
            plan.empty += 1;
        }
    }
    // A record is in at most one piece: `used` marks the empty records a mosaic took.
    let mut used = vec![false; n];
    let free =
        |taken: &[bool], used: &[bool], r: usize| r < n && (!taken[r] || (empty[r] && !used[r]));
    for &s in &game.shore {
        let s = usize::from(s);
        if s < n && !taken[s] {
            taken[s] = true;
            plan.shore += 1;
        }
    }
    for (letter, item, _) in &game.portraits {
        plan.items.push(Item {
            class: Class::Portrait,
            pieces: vec![Piece::single(ImageId::Portrait {
                letter: *letter,
                item: *item,
            })],
            layout: None,
        });
    }
    for role in Role::ALL {
        if game.bitmaps.iter().any(|(r, ..)| *r == role) {
            plan.items.push(Item {
                class: Class::Bitmap,
                pieces: vec![Piece::single(ImageId::Bitmap(role))],
                layout: None,
            });
        }
    }

    // Terrain: the chunk tiles and the animation frames that stand in for them.
    let mut terrain: Vec<bool> = (0..n)
        .map(|t| game.in_chunks.get(t).copied().unwrap_or(false))
        .collect();
    loop {
        let mut grew = false;
        for slot in &game.anim {
            if !terrain
                .get(usize::from(slot.tile_to_animate))
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            for c in 0..=u32::from(slot.and_mask) {
                let f = usize::from(slot.frame_tile(c));
                if f < 0x200 && f < n && !terrain[f] {
                    terrain[f] = true;
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    for t in 0..n {
        if terrain[t] && !taken[t] && game.records[t].map {
            taken[t] = true;
            plan.items.push(Item {
                class: Class::Terrain,
                pieces: vec![Piece::single(ImageId::Tile(t as u16))],
                layout: None,
            });
        }
    }

    // Spans of the object types: from a base tile up to the next one.
    let mut bases: Vec<usize> = game
        .basetile
        .iter()
        .map(|&b| usize::from(b))
        .filter(|&b| b < n)
        .collect();
    bases.sort_unstable();
    bases.dedup();
    let span = |b: usize| {
        let end = bases.iter().copied().find(|&x| x > b).unwrap_or(n);
        b..end
    };

    // Pieces of a range of tiles, anchors first from the top (a part is below its anchor).
    // A mosaic that would share a record with an earlier one is not composed: its records are
    // single pieces of the same item (an actor is accepted or falls back whole anyway).
    let pieces_in = |range: std::ops::Range<usize>, taken: &mut Vec<bool>, used: &mut Vec<bool>| {
        let mut out: Vec<(usize, Piece)> = Vec::new();
        for t in range.clone().rev() {
            if taken[t] {
                continue;
            }
            if let Some(m) = mosaic(game, t)
                && m.images.iter().all(|&id| {
                    let r = record(id);
                    range.contains(&r) && free(taken, used, r)
                })
            {
                for &id in &m.images {
                    taken[record(id)] = true;
                    used[record(id)] = true;
                }
                out.push((t, m));
            }
        }
        for t in range {
            if !taken[t] {
                taken[t] = true;
                out.push((t, Piece::single(ImageId::Tile(t as u16))));
            }
        }
        out.sort_by_key(|(t, _)| *t);
        out.into_iter().map(|(_, p)| p).collect::<Vec<Piece>>()
    };

    // An actor's frames: as many as its type has, from its base tile (another type's base
    // tile among them does not cut them); a type of unknown layout up to the next base tile.
    let mut actors: Vec<Layout> = Vec::new();
    for &t in &game.actor_types {
        let Some(&b) = game.basetile.get(usize::from(t)) else {
            continue;
        };
        if usize::from(b) >= n {
            continue;
        }
        let (frames, per_facing) = match actor_layout(t) {
            Some((f, p)) => (f, Some(p)),
            None => (span(usize::from(b)).len() as u16, None),
        };
        match actors.iter_mut().find(|l| l.base == b) {
            Some(l) if l.frames < frames => {
                (l.frames, l.per_facing) = (frames, per_facing);
            }
            Some(_) => {}
            None => actors.push(Layout {
                base: b,
                frames,
                per_facing,
            }),
        }
    }
    actors.sort_by_key(|l| l.base);
    // Types whose frames overlap (a base tile among another type's frames, or frames running
    // past the next type's base tile) are one item: their frames together, paired in order,
    // since no one facing layout holds for all of them. Otherwise the later type would lose
    // the shared records to the earlier and be split over two items.
    let mut joined: Vec<Layout> = Vec::new();
    let end = |l: &Layout| usize::from(l.base) + usize::from(l.frames);
    for l in actors {
        match joined.last_mut() {
            Some(j) if usize::from(l.base) < end(j) => {
                let e = end(j).max(end(&l));
                j.frames = (e - usize::from(j.base)) as u16;
                j.per_facing = None;
                plan.actor_overlaps += 1;
            }
            _ => joined.push(l),
        }
    }
    for l in joined {
        let b = usize::from(l.base);
        let range = b..(b + usize::from(l.frames)).min(n);
        let pieces = pieces_in(range, &mut taken, &mut used);
        if !pieces.is_empty() {
            plan.items.push(Item {
                class: Class::Actor,
                pieces,
                layout: Some(l),
            });
        }
    }

    // Multi-record objects anywhere else. Mosaics that share a record (two anchors whose parts
    // overlap, or an empty part both name) are one item, accepted or falling back together: the
    // first found keeps its composition, the records of the others not yet in it join as single
    // pieces. Then the rest one by one.
    let mut groups: Vec<(usize, Vec<Piece>)> = Vec::new();
    let mut owner: Vec<Option<usize>> = vec![None; n];
    for t in (0..n).rev() {
        if taken[t] {
            continue;
        }
        let Some(m) = mosaic(game, t) else { continue };
        let records: Vec<usize> = m.images.iter().map(|&id| record(id)).collect();
        if !records
            .iter()
            .all(|&r| r < n && (!taken[r] || empty[r]) && !(empty[r] && used[r]))
        {
            continue;
        }
        let mut owners: Vec<usize> = records.iter().filter_map(|&r| owner[r]).collect();
        owners.sort_unstable();
        owners.dedup();
        let Some((&into, rest)) = owners.split_first() else {
            let g = groups.len();
            for &r in &records {
                owner[r] = Some(g);
            }
            groups.push((t, vec![m]));
            continue;
        };
        for &other in rest {
            let moved = std::mem::take(&mut groups[other].1);
            for p in &moved {
                for &id in &p.images {
                    owner[record(id)] = Some(into);
                }
            }
            groups[into].1.extend(moved);
        }
        for &r in &records {
            if owner[r].is_none() {
                owner[r] = Some(into);
                groups[into].1.push(Piece::single(ImageId::Tile(r as u16)));
            }
        }
        plan.overlaps += 1;
    }
    for (r, o) in owner.iter().enumerate() {
        if o.is_some() {
            taken[r] = true;
        }
    }
    groups.retain(|(_, pieces)| !pieces.is_empty());
    // Found from the top down; listed from the bottom up.
    groups.reverse();
    for (_, pieces) in groups {
        plan.items.push(Item {
            class: Class::Multi,
            pieces,
            layout: None,
        });
    }
    for (t, done) in taken.iter_mut().enumerate() {
        if !*done {
            *done = true;
            plan.items.push(Item {
                class: Class::Object,
                pieces: vec![Piece::single(ImageId::Tile(t as u16))],
                layout: None,
            });
        }
    }
    plan
}

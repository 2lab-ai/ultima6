//! The three cursors over the object list: objects at a tile, contents of an object, objects in
//! a rectangle.

use super::{IN_CONTAINER, IN_INVENTORY, NONE, ObjectId, Objects, Slot};
use crate::state::{State, Tables};
use u6_formats::position::Position;

/// Original: A4-0x783c..-0x783a (packed target tile), A4-0x7840 (current entry), A4-0x783e
/// (second row reached), A4-0x78dc (which part of a multi-tile object covers the tile).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocationCursor {
    pub pos: [u8; 3],
    pub cur: Slot,
    pub second_row: u16,
    /// 0 = the object stands on the tile; 1 = it is the left / upper half of an object one tile
    /// east or south (2 when that object is also double height), 3 = the top-left quarter of
    /// an object one tile south-east.
    pub part: u16,
}

/// Original: A4-0x7838 (next entry), A4-0x7836 (parent), A4-0x7834 (type), A4-0x7832
/// (quality, -1 = any).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChildCursor {
    pub next: Slot,
    pub parent: Slot,
    pub kind: i16,
    pub quality: i16,
}

/// Original: A4-0x7830 (current entry), A4-0x782e (end entry), A4+0x1304..0x130c (x0, x1, y0,
/// y1, z).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AreaCursor {
    pub cur: Slot,
    pub end: Slot,
    pub x0: i16,
    pub x1: i16,
    pub y0: i16,
    pub y1: i16,
    pub z: i16,
}

/// Original: 0x134d3e (set x in packed location bytes; x >> 8 is OR-ed into byte 1 unmasked)
fn set_x(b: [u8; 3], x: u16) -> [u8; 3] {
    [x as u8, ((u16::from(b[1]) & 0xfc) | (x >> 8)) as u8, b[2]]
}

/// Original: 0x134d6e (set y in packed location bytes)
fn set_y(b: [u8; 3], y: u16) -> [u8; 3] {
    let b1 = (u16::from(b[1] & 0x03) | (y << 2)) as u8;
    let b2 = (u16::from(b[2] & 0xf0) | (y >> 6)) as u8;
    [b[0], b1, b2]
}

/// Tile of a type + frame word: base tile of the type plus the frame (word add).
pub(crate) fn tile_of(t: &Tables, tf: u16) -> u16 {
    let base = t.basetile.base.get(usize::from(tf & 0x3ff)).copied();
    base.unwrap_or(0).wrapping_add(tf >> 10)
}

/// flags2 of a tile (double width 0x80, double height 0x40), 0 past the table.
pub(crate) fn flags2(t: &Tables, tile: u16) -> u8 {
    t.tileflags
        .flags2
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
}

/// Original: 0x10ad1a (objects: begin iterating the objects at (x, y, z))
///
/// In a dungeon (z ≠ 0) x and y wrap at 256. Returns the first object, as 0x10aaca.
pub fn begin_at(o: &mut Objects, t: &Tables, mut x: u16, mut y: u16, z: u16) -> Slot {
    if z != 0 {
        x &= 0xff;
        y &= 0xff;
    }
    o.at.part = 0;
    o.at.second_row = 0;
    o.at.cur = o.list_pos(x, y, false);
    o.at.pos = super::pack_xyz(x, y, z).to_packed();
    next_at(o, t)
}

/// Original: 0x10aaca (objects: next object of the location iteration, -1 at the end)
///
/// Yields, in list order, the map objects on the tile, then those one tile east whose tile is
/// double width, then (after jumping to the next row's list position once) those one tile south
/// that are double height and south-east that are both. Contained objects and other levels are
/// skipped; [`LocationCursor::part`] says which case matched.
pub fn next_at(o: &mut Objects, t: &Tables) -> Slot {
    let mut at = o.at;
    let found = step_at(o, t, &mut at);
    o.at = at;
    found
}

/// The body of 0x10aaca on a cursor held apart from the tables (so a read-only walk can run on a
/// copy of the cursor).
fn step_at(o: &Objects, t: &Tables, cursor: &mut LocationCursor) -> Slot {
    let here = cursor.pos;
    let x = (u16::from(here[1]) << 8 | u16::from(here[0])) & 0x3ff;
    let y = ((u16::from(here[2]) << 8 | u16::from(here[1])) >> 2) & 0x3ff;
    let z = here[2] >> 4;
    let east = set_x(here, x + 1);
    let south = set_y(here, y + 1);
    let south_east = set_y(set_x(here, x + 1), y + 1);
    let mut found = NONE;
    loop {
        let cur = cursor.cur;
        if cur < 0 || found >= 0 {
            return found;
        }
        if o.place(cur) == 0 && o.z(cur) == z {
            let f2 = flags2(t, tile_of(t, o.tf(cur)));
            let at = o.packed(cur);
            if at == here {
                found = cur;
            } else if at == east {
                if f2 & 0x80 != 0 {
                    found = cur;
                    cursor.part = 1;
                }
            } else if at == south {
                if f2 & 0x40 != 0 {
                    found = cur;
                    cursor.part = if f2 & 0x80 != 0 { 2 } else { 1 };
                }
            } else if at == south_east {
                if f2 & 0xc0 == 0xc0 {
                    found = cur;
                    cursor.part = 3;
                }
            } else if cursor.second_row != 0 {
                cursor.cur = NONE;
                return found;
            } else {
                cursor.cur = o.list_pos(x, y + 1, false);
                cursor.second_row = 1;
                continue;
            }
        }
        cursor.cur = o.next_of(cursor.cur);
    }
}

/// [`objects_at`] without moving the state's cursor (for read-only callers; the walk is the same,
/// on a fresh cursor).
pub fn objects_at_view(s: &State, pos: Position) -> Vec<(ObjectId, u16)> {
    let (o, t) = (&s.objects, &s.tables);
    let (mut x, mut y, z) = (pos.x, pos.y, u16::from(pos.z));
    if z != 0 {
        x &= 0xff;
        y &= 0xff;
    }
    let mut cursor = LocationCursor {
        pos: super::pack_xyz(x, y, z).to_packed(),
        cur: o.list_pos(x, y, false),
        second_row: 0,
        part: 0,
    };
    let mut out = Vec::new();
    let mut slot = step_at(o, t, &mut cursor);
    while let Some(id) = ObjectId::from_slot(slot) {
        out.push((id, cursor.part));
        slot = step_at(o, t, &mut cursor);
    }
    out
}

/// Every object 0x10ad1a / 0x10aaca yield at `pos`, with the part each covers.
pub fn objects_at(s: &mut State, pos: Position) -> Vec<(ObjectId, u16)> {
    let (o, t) = (&mut s.objects, &s.tables);
    let mut out = Vec::new();
    let mut slot = begin_at(o, t, pos.x, pos.y, u16::from(pos.z));
    while let Some(id) = ObjectId::from_slot(slot) {
        out.push((id, o.at.part));
        slot = next_at(o, t);
    }
    out
}

/// Original: 0x10ae52 (objects: first object directly inside `parent`, -1 if none)
///
/// Also starts the child cursor for 0x10aed6 (any type, any quality).
pub fn first_child(o: &mut Objects, parent: Slot) -> Slot {
    let c = o.next_of(parent);
    if o.place(c) == 0 || o.parent(c) != parent {
        return NONE;
    }
    o.children = ChildCursor {
        next: o.next_of(c),
        parent,
        kind: o.kind(c) as i16,
        quality: -1,
    };
    c
}

/// The child-cursor walk of 0x10aed6 and 0x10b0ba; `of_kind` adds the type / quality test.
fn walk_children(o: &mut Objects, of_kind: bool) -> Slot {
    let mut found = NONE;
    while o.children.next >= 0 && found < 0 {
        let n = o.children.next;
        if o.place(n) == 0 {
            o.children.next = NONE;
            break;
        }
        let c = o.children;
        let mine = o.parent(n) == c.parent
            && (!of_kind
                || (o.kind(n) as i16 == c.kind
                    && (c.quality < 0 || i16::from((o.qq_of(n) >> 8) as u8) == c.quality)));
        let place = o.place(n);
        if mine && (o.skip_readied == 0 || place == IN_INVENTORY || place == IN_CONTAINER) {
            found = n;
        }
        o.children.next = o.next_of(n);
    }
    found
}

/// Original: 0x10aed6 (objects: next object directly inside the cursor's parent)
///
/// Deeper contents are passed over; the walk ends at the first map object. While
/// [`Objects::skip_readied`] is set, readied objects are skipped.
pub fn next_child(o: &mut Objects) -> Slot {
    walk_children(o, false)
}

/// Original: 0x10b18c (objects: first object of `kind` (and `quality` unless -1) in `parent`)
pub fn find_child(o: &mut Objects, parent: Slot, kind: i16, quality: i16) -> Slot {
    if parent < 0 {
        return NONE;
    }
    let c = o.next_of(parent);
    if o.place(c) == 0 || o.parent(c) != parent {
        return NONE;
    }
    o.children = ChildCursor {
        next: c,
        parent,
        kind,
        quality,
    };
    next_child_of_kind(o)
}

/// Original: 0x10b0ba (objects: next object of the cursor's type and quality in its parent)
pub fn next_child_of_kind(o: &mut Objects) -> Slot {
    walk_children(o, true)
}

/// Original: 0x10b05a (objects: begin iterating map objects in x0..=x1, y0..=y1 on the
/// leader's level)
pub fn begin_area(s: &mut State, x0: i16, y0: i16, x1: i16, y1: i16) -> Slot {
    let (x0, y0) = (x0.max(0), y0.max(0));
    let z = i16::from(s.map.centre.z);
    let o = &mut s.objects;
    let cur = o.list_pos(x0 as u16, y0 as u16, false);
    let end = o.list_pos(x1.wrapping_add(1) as u16, y1 as u16, false);
    o.area = AreaCursor {
        cur,
        end,
        x0,
        x1,
        y0,
        y1,
        z,
    };
    next_in_area(o)
}

/// Original: 0x10af74 (objects: next map object of the area iteration, -1 at the end)
///
/// Positions are masked with [`Objects::coord_mask`] before the test.
pub fn next_in_area(o: &mut Objects) -> Slot {
    let a = o.area;
    if a.cur < 0 {
        return NONE;
    }
    let mut found = NONE;
    let mut cur = a.cur;
    while cur != a.end && cur >= 0 && found < 0 {
        if o.place(cur) == 0 {
            let x = (o.x(cur) & o.coord_mask) as i16;
            let y = (o.y(cur) & o.coord_mask) as i16;
            let z = i16::from(o.z(cur));
            if (a.x0..=a.x1).contains(&x) && (a.y0..=a.y1).contains(&y) && z == a.z {
                found = cur;
            }
        }
        cur = o.next_of(cur);
    }
    o.area.cur = cur;
    if cur == o.area.end {
        o.area.cur = NONE;
        o.area.end = NONE;
    }
    found
}

/// Original: 0x10ad66 (objects: map object of `kind` at (x, y) on the leader's level, -1)
pub fn object_of_type_at(s: &State, kind: u16, x: u16, y: u16) -> Slot {
    let o = &s.objects;
    let z = s.map.centre.z;
    let mut c = o.list_pos(x, y, false);
    while c >= 0 {
        if o.place(c) == 0 {
            if o.x(c) != x || o.y(c) != y {
                return NONE;
            }
            if o.z(c) == z && o.kind(c) == kind {
                return c;
            }
        }
        c = o.next_of(c);
    }
    NONE
}

/// Original: 0x10b20e (objects: first actor from `start` (at least 1) standing at (x, y) on the
/// leader's level and not flagged 0x10 in its actor status, -1)
pub fn actor_at(s: &State, start: i16, x: u16, y: u16) -> Slot {
    let o = &s.objects;
    let z = s.map.centre.z;
    for a in start.max(1)..super::ACTOR_SLOTS as i16 {
        let status = s.actors.attrs.status.get(a as usize).copied().unwrap_or(0);
        if o.tf(a) != 0 && status & 0x10 == 0 && o.x(a) == x && o.y(a) == y && o.z(a) == z {
            return a;
        }
    }
    NONE
}

//! Draw lists of the 11 × 11 map window: per cell, the object tiles to draw over its base tile.
//!
//! Built by 0x10e81e before the window is drawn; gfx (0x10202e) walks them. A list is a chain
//! of node numbers 1..255 through `node_next` (0 ends it, node 0 heads the free chain).

use super::iter::{begin_area, flags2, next_in_area, tile_of};
use super::{ACTOR_SLOTS, Slot, seams};
use crate::state::State;

/// Cells in the map window (11 × 11).
pub const DRAW_CELLS: usize = 11 * 11;
/// Draw-list nodes (node 0 = free-chain head).
pub const DRAW_NODES: usize = 256;
/// Base tile that hides a cell (0x10e596 draws nothing on it).
const HIDDEN: u16 = 0xff;
/// Base tile drawn only with the "see through" flag and for tiles with flags3 bit 0.
const DARK: u16 = 0x1bc;

/// The draw lists and the window's base tiles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawLists {
    /// Original: A4-0x1476 (11 × 11 words: base tile of each window cell; map's tile-cache
    /// rebuild 0x108f90 fills it, 0x10e596 overwrites a cell with a flat object tile)
    pub base: Vec<u16>,
    /// Original: A4-0x15f0 (11 × 11 bytes: first node of each cell's list, 0 = empty)
    pub cell: Vec<u8>,
    /// Original: A4-0x1577 (256 bytes: next node; node 0 = first free node)
    pub node_next: Vec<u8>,
    /// Original: A4-0x1384 (256 words: tile of each node; bits 11-12 = effect: 0x800 invisible
    /// outline, 0x1000 / 0x1800 actor effects)
    pub node_tile: Vec<u16>,
    /// Original: A4-0x7af6 (word: set to 1 to ask for the slot of a volcano / fumarole in view;
    /// 0x10e81e stores the slot or resets it)
    pub volcano_probe: i16,
    /// Original: A4-0x78e0 (word: an object 0x188 was in view at the last build)
    pub saw_188: u16,
    /// Original: A4-0x77a0 / A4-0x779f (bytes: reset when an object 0x188 comes into view; the
    /// second gets a random 0..=1)
    pub anim_188: [u8; 2],
}

impl Default for DrawLists {
    fn default() -> Self {
        Self {
            base: vec![0; DRAW_CELLS],
            cell: vec![0; DRAW_CELLS],
            node_next: vec![0; DRAW_NODES],
            node_tile: vec![0; DRAW_NODES],
            volcano_probe: 0,
            saw_188: 0,
            anim_188: [0; 2],
        }
    }
}

impl DrawLists {
    /// The tiles of cell (cx, cy) in list order.
    pub fn tiles(&self, cx: usize, cy: usize) -> Vec<u16> {
        let mut out = Vec::new();
        let mut n = self.cell.get(cy * 11 + cx).copied().unwrap_or(0);
        while n != 0 && out.len() < DRAW_NODES {
            out.push(self.node_tile[usize::from(n)]);
            n = self.node_next[usize::from(n)];
        }
        out
    }
}

fn flags3(s: &State, tile: u16) -> u8 {
    s.tables
        .tileflags
        .flags3
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
}

/// Where a new node is linked: the head pointer of a cell or the `node_next` of a node.
#[derive(Clone, Copy)]
enum Link {
    Cell(usize),
    Node(u8),
}

fn read(d: &DrawLists, l: Link) -> u8 {
    match l {
        Link::Cell(c) => d.cell[c],
        Link::Node(n) => d.node_next[usize::from(n)],
    }
}

/// Original: 0x10e596 (objects: add a tile to the draw list of cell (cx, cy))
///
/// `mode`: 0 the object's own cell, 1 the other part of a multi-tile object, 2 forced on top, 3
/// a light overlay. Egg tiles are never drawn; nothing is drawn on a hidden cell, or on a dark
/// cell unless "see through" is on and the tile has flags3 bit 0. A flags3 0x20 tile replaces
/// the cell's base tile. Otherwise the node goes in front of the list, except that top tiles
/// (flags2 0x10) and mode 2 go behind the last non-top tile (mode 0/2), at the end (mode 1),
/// or, for mode 3 with "see through", after the first tile without flags3 bit 0.
pub fn add_tile(s: &mut State, word: u16, cx: i16, cy: i16, mode: u16) {
    let t = word & 0x7ff;
    let egg = s.tables.basetile.base.get(0x14f).copied();
    if egg == Some(t) {
        return;
    }
    let idx = cy * 11 + cx;
    let Some(c) = usize::try_from(idx).ok().filter(|&c| c < DRAW_CELLS) else {
        // The original reads the word before the table here; the port draws nothing.
        return;
    };
    let base = s.objects.draw.base[c];
    let see_through = seams::see_through(s);
    if base == HIDDEN || (base == DARK && (!see_through || flags3(s, t) & 1 == 0)) {
        return;
    }
    if flags3(s, t) & 0x20 != 0 {
        s.objects.draw.base[c] = word;
        return;
    }
    let top = flags2(&s.tables, t) & 0x10 != 0;
    let d = &s.objects.draw;
    let node = d.node_next[0];
    if node == 0 {
        return;
    }
    let tile_of_node = |n: u8| d.node_tile[usize::from(n)] & 0x7ff;
    let mut at = Link::Cell(c);
    if top || mode == 2 {
        if mode & 1 != 0 {
            if see_through && mode == 3 {
                let start = at;
                let mut p = start;
                loop {
                    let n = read(d, p);
                    if n == 0 {
                        at = start;
                        break;
                    }
                    if flags3(s, tile_of_node(n)) & 1 == 0 {
                        at = Link::Node(n);
                        break;
                    }
                    p = Link::Node(n);
                }
            } else {
                while read(d, at) != 0 {
                    at = Link::Node(read(d, at));
                }
            }
        } else {
            loop {
                let n = read(d, at);
                if n == 0 || flags2(&s.tables, tile_of_node(n)) & 0x10 != 0 {
                    break;
                }
                at = Link::Node(n);
            }
        }
    }
    let d = &mut s.objects.draw;
    d.node_next[0] = d.node_next[usize::from(node)];
    d.node_tile[usize::from(node)] = word;
    let after = read(d, at);
    d.node_next[usize::from(node)] = after;
    match at {
        Link::Cell(c) => d.cell[c] = node,
        Link::Node(n) => d.node_next[usize::from(n)] = node,
    }
}

/// Original: 0x10e73a (objects: add an object's tile(s) to the draw lists)
///
/// The tile `tile` + `effect` goes to cell (cx, cy) when it is inside the window; a double
/// width tile adds tile − 1 to the cell west, a double height one tile − 1 (or − 2 when also
/// wide) to the cell north, and a double one tile − 3 to the cell north-west.
pub fn add_object_tiles(s: &mut State, tile: u16, effect: u16, cx: i16, cy: i16) {
    let w = tile.wrapping_add(effect);
    if cx < 11 && cy < 11 {
        add_tile(s, w, cx, cy, 0);
    }
    let f2 = flags2(&s.tables, tile);
    if f2 & 0x80 != 0 {
        if cx != 0 && cy < 11 {
            add_tile(s, w.wrapping_sub(1), cx - 1, cy, 1);
        }
        if f2 & 0x40 != 0 {
            if cx < 11 && cy != 0 {
                add_tile(s, w.wrapping_sub(2), cx, cy - 1, 1);
            }
            if cx != 0 && cy != 0 {
                add_tile(s, w.wrapping_sub(3), cx - 1, cy - 1, 1);
            }
        }
    } else if cx < 11 && cy != 0 && f2 & 0x40 != 0 {
        add_tile(s, w.wrapping_sub(1), cx, cy - 1, 1);
    }
}

/// Original: 0x10e81e (objects: build the draw lists of the map window around the leader)
///
/// Every map object in leader − 5 ..= leader + 6 is visited in list order. Invisible objects
/// (status 0x02) are drawn only for party members (actor status 0x80) and then as an outline
/// (+0x800, types below 0x1aa); actors with movement flag 0x10 are skipped; a type-0x7b actor
/// with frame < 2 shows frame 1 when an actor stands below it. Actor effects: actor status
/// 0x01 → +0x1000, status 0x40 → +0x1800. Two fixed doorways (0x18a at (926, 856), 0x18c at
/// (930, 856)) add their side tiles. Then, unless A4-0x794c is set, cells whose base tile has
/// flags1 0x04 and whose view flags (east or south neighbour) are dark lose their objects, and
/// light levels 1..3 overlay tile 0x1bc + level. An object 0x188 coming into view resets its
/// animation with one RNG draw.
pub fn build_draw_lists(s: &mut State) {
    let d = &mut s.objects.draw;
    for (i, n) in d.node_next.iter_mut().enumerate() {
        *n = (i + 1) as u8;
    }
    d.cell.fill(0);
    let mut saw_188 = 0;
    let (lx, ly) = (s.map.centre.x as i16, s.map.centre.y as i16);
    let mut c = begin_area(s, lx - 5, ly - 5, lx + 6, ly + 6);
    while c >= 0 {
        draw_object(s, c, lx, ly, &mut saw_188);
        c = next_in_area(&mut s.objects);
    }
    if seams::view_pass_off(s) == 0 {
        overlay_view(s);
    }
    let d = &mut s.objects.draw;
    if d.volcano_probe == 1 {
        d.volcano_probe = 0;
    }
    if saw_188 != 0 && d.saw_188 == 0 {
        d.anim_188[0] = 0;
        d.anim_188[1] = s.rng.range(0, 1) as u8;
    }
    s.objects.draw.saw_188 = saw_188;
}

fn draw_object(s: &mut State, c: Slot, lx: i16, ly: i16, saw_188: &mut u16) {
    let actor = c < ACTOR_SLOTS as Slot;
    let kind = s.objects.kind(c);
    let astatus = |s: &State| {
        if actor {
            s.actors.attrs.status.get(c as usize).copied().unwrap_or(0)
        } else {
            0
        }
    };
    if s.objects.draw.volcano_probe == 1 && (kind == 0x133 || kind == 0xab) {
        s.objects.draw.volcano_probe = c;
    }
    if kind == 0x188 {
        *saw_188 = 1;
    }
    let o = &s.objects;
    let invisible = o.status(c) & 0x02 != 0;
    if invisible && !(actor && astatus(s) & 0x80 != 0) {
        return;
    }
    if actor {
        let mf = s.actors.attrs.movement_flags.get(c as usize).copied();
        if mf.unwrap_or(0) & 0x10 != 0 {
            return;
        }
    }
    if kind == 0x7b && (o.tf(c) >> 10) < 2 {
        // The start slot is whatever D0 held: location byte 1 of the object.
        let start = i16::from(o.packed(c)[1]);
        let below = super::iter::actor_at(s, start, o.x(c), o.y(c) + 1);
        let t = &mut s.objects.type_frame[c as usize];
        *t = (*t & 0x3ff) + if below >= 0 { 0x400 } else { 0 };
    }
    let o = &s.objects;
    let tile = tile_of(&s.tables, o.tf(c));
    let mut effect = 0;
    if invisible && kind < 0x1aa {
        effect = 0x800;
    }
    if actor {
        if astatus(s) & 0x01 != 0 {
            effect = 0x1000;
        }
        if o.status(c) & 0x40 != 0 {
            effect = 0x1800;
        }
    }
    let cx = (o.x(c) as i16).wrapping_sub(lx) + 5;
    let cy = (o.y(c) as i16).wrapping_sub(ly) + 5;
    let z = o.z(c);
    if effect != 0 {
        add_object_tiles(s, tile, effect, cx, cy);
    }
    if !invisible {
        add_object_tiles(s, tile, 0, cx, cy);
    }
    let doorway = match kind {
        0x18a => Some((0x39e, [0x1ba, 0x1bb])),
        0x18c => Some((0x3a2, [0x1b8, 0x1b9])),
        _ => None,
    };
    if let Some((x, [right, left])) = doorway
        && cx + lx == x
        && cy + ly == 0x358
        && z == 0
        && cy < 11
    {
        if cx != 0 {
            add_tile(s, right, cx + 1, cy, 1);
        }
        if cx < 10 {
            add_tile(s, left, cx - 1, cy, 1);
        }
    }
}

/// The second pass of 0x10e81e over the 11 × 11 cells (view flags and light overlays).
fn overlay_view(s: &mut State) {
    let (vx, vy) = view_offset(s);
    for x in 0..11i16 {
        for y in 0..11i16 {
            let c = (y * 11 + x) as usize;
            let base = s.objects.draw.base[c];
            let f1 = s.tables.tileflags.flags1.get(usize::from(base)).copied();
            if f1.unwrap_or(0) & 0x04 != 0
                && (seams::view_flag(s, vx + x + 1, vy + y) & 0x80 == 0
                    || seams::view_flag(s, vx + x, vy + y + 1) & 0x80 == 0)
            {
                let d = &mut s.objects.draw;
                let first = d.cell[c];
                if first != 0 {
                    d.cell[c] = 0;
                    let mut last = first;
                    while d.node_next[usize::from(last)] != 0 {
                        last = d.node_next[usize::from(last)];
                    }
                    d.node_next[usize::from(last)] = d.node_next[0];
                    d.node_next[0] = first;
                }
            }
            let light = seams::view_light(s, vx + x, vy + y);
            if (1..4).contains(&light) {
                add_tile(s, DARK + u16::from(light), x, y, 3);
            }
        }
    }
}

/// Original: A4-0x7950 / A4-0x794e (words: column and row of the 11 × 11 view inside the
/// 40 × 40 window), as map 0x108d10 computes them from the leader and the window origin.
pub fn view_offset(s: &State) -> (i16, i16) {
    let c = s.map.centre;
    let [wx, wy] = s.objects.window;
    let off = |l: u16, w: u16| (l.wrapping_sub(w).wrapping_sub(5) & 0x1f) as i16;
    (off(c.x, wx), off(c.y, wy))
}

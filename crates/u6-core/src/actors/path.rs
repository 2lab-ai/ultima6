//! Going to the scheduled place: the path finder over the 40 × 40 window, the path slots, the
//! off-screen teleport and the arrival that starts the scheduled work.
//!
//! The search (0x11ea54) is a bucket-queue Dijkstra over the cost map 0x1206e2 builds, run from
//! both ends at once when the goal is a location (grid bit 7 tells the sides apart); the path
//! (0x11e2a6) is stored as at most 31 bytes in one of 8 slots: direction (0 N, 1 E, 2 S, 3 W)
//! in bits 0-1, run length × 4 above. Walkers (worktypes 0x83..0x85) follow it in 0x11f804.

use crate::objects::ObjectId;
use crate::state::State;

use super::seams::{self, Found};
use super::work::{GRID, NODES};
use super::{tail, walk};

/// 4-way deltas, 0 = north, clockwise (read from the image at A4-0x69bc / A4-0x69b8:
/// x 0,1,0,-1 and y -1,0,1,0).
const DX4: [i16; 4] = [0, 1, 0, -1];
const DY4: [i16; 4] = [-1, 0, 1, 0];
/// Window side as a signed coordinate bound.
const LAST: i16 = GRID as i16 - 1;
/// Port safety net for the two back-trace walks (the original has no bound; a well-formed
/// grid always ends them within GRID² steps). A walk that would leave the grid (the original
/// then reads outside it) also ends.
const WALK_LIMIT: usize = GRID * GRID;

fn cell(col: i16, row: i16) -> usize {
    row as usize * GRID + col as usize
}

fn inside(col: i16, row: i16) -> bool {
    (0..=LAST).contains(&col) && (0..=LAST).contains(&row)
}

/// The search window origin: ((leader x − 16) & 0x3f8, (leader y − 16) & 0x3f8).
fn window(s: &State) -> (u16, u16) {
    let c = s.map.centre;
    (c.x.wrapping_sub(16) & 0x3f8, c.y.wrapping_sub(16) & 0x3f8)
}

/// The schedule entry the actor is on (first entry + 0x139388).
pub fn schedule_entry(s: &State, actor: u16) -> Option<u6_formats::schedule::ScheduleEntry> {
    let a = usize::from(actor);
    let first = *s.actors.schedules.first.get(a)?;
    let i = first.wrapping_add(u16::from(s.actors.attrs.unknown_10f1[a]));
    s.actors.schedules.entries.get(usize::from(i)).copied()
}

/// Path byte `i` of slot `slot` (A4+0x47fd + 32 × slot).
fn path_byte(s: &State, slot: usize, i: usize) -> u8 {
    s.actors
        .attrs
        .unknown_1af1
        .get(slot * 32 + i)
        .copied()
        .unwrap_or(0)
}

fn set_path_byte(s: &mut State, slot: usize, i: usize, v: u8) {
    if let Some(b) = s.actors.attrs.unknown_1af1.get_mut(slot * 32 + i) {
        *b = v;
    }
}

/// Original: 0x12061e (actors: add an object tile to a cost-map cell)
///
/// On a blocked cell only a passable tile over water (a bridge) opens it, at its own cost; on
/// an open cell an impassable tile blocks it, a wall leaves it, any other tile adds its cost
/// nibble (at most 0xff).
pub fn add_tile_cost(s: &mut State, tile: u16, col: i16, row: i16) {
    let f = walk::flags1(s, tile);
    let i = cell(col, row);
    let cur = s.actors.path.cost[i];
    if cur == 0xff {
        let under = seams::window_tile(s, col as u16, row as u16);
        if walk::flags1(s, u16::from(under)) & 0x01 != 0 && f & 0x02 == 0 {
            s.actors.path.cost[i] = (f >> 4) + 1;
        }
    } else if f & 0x02 != 0 {
        s.actors.path.cost[i] = 0xff;
    } else if f & 0x04 == 0 {
        s.actors.path.cost[i] = (u16::from(cur) + u16::from(f >> 4)).min(0xff) as u8;
    }
}

/// Original: 0x1206e2 (actors: build the 40 × 40 cost map of the search window)
///
/// Map tiles: impassable = 0xff, else cost nibble + 1. Doors add 1 when open (frame < 8), else
/// block, and block the cell before them (above for a double-height door, left otherwise);
/// portcullises add 1; other non-actor objects add their tiles (all four of a double-size one).
pub fn build_cost_map(s: &mut State, _actor: u16) {
    for row in 0..GRID {
        for col in 0..GRID {
            let t = seams::window_tile(s, col as u16, row as u16);
            let f = walk::flags1(s, u16::from(t));
            s.actors.path.cost[row * GRID + col] = if f & 0x02 != 0 { 0xff } else { (f >> 4) + 1 };
        }
    }
    let (x0, y0) = window(s);
    let z = walk::party_z(s);
    let objs = seams::objects_in_rect(s, x0, y0, x0 + LAST as u16, y0 + LAST as u16, z);
    for id in objs {
        let p = seams::obj_pos(s, id);
        let (c, r) = (p.x.wrapping_sub(x0) as i16, p.y.wrapping_sub(y0) as i16);
        if !inside(c, r) {
            continue;
        }
        let word = seams::obj_word(s, id);
        let t = word & 0x3ff;
        let tile = walk::word_tile(s, word);
        let f2 = walk::flags2(s, tile);
        let cost = &mut s.actors.path.cost;
        if (0x129..=0x12c).contains(&t) {
            let i = cell(c, r);
            if word >> 10 < 8 && cost[i] < 0xff {
                cost[i] += 1;
            } else {
                cost[i] = 0xff;
            }
            if f2 & 0x40 != 0 {
                if r != 0 {
                    cost[cell(c, r - 1)] = 0xff;
                }
            } else if c != 0 {
                cost[cell(c - 1, r)] = 0xff;
            }
        } else if t == 0x116 || t == 0x118 {
            let i = cell(c, r);
            cost[i] = cost[i].saturating_add(1);
        } else if id.0 >= 0x100 {
            add_tile_cost(s, tile, c, r);
            if f2 & 0x80 != 0 {
                if c != 0 {
                    add_tile_cost(s, tile.wrapping_sub(1), c - 1, r);
                }
                if f2 & 0x40 != 0 {
                    if r != 0 {
                        add_tile_cost(s, tile.wrapping_sub(2), c, r - 1);
                    }
                    if c != 0 && r != 0 {
                        add_tile_cost(s, tile.wrapping_sub(3), c - 1, r - 1);
                    }
                }
            } else if f2 & 0x40 != 0 && r != 0 {
                add_tile_cost(s, tile.wrapping_sub(1), c, r - 1);
            }
        }
    }
}

/// Original: 0x11dcc2 (actors: direction + 1 of a 0x77 object north, east, south or west of
/// (x, y, z), 0 if none)
pub fn table_beside(s: &State, x: u16, y: u16, z: u16) -> u16 {
    for d in [0usize, 2, 4, 6] {
        let (nx, ny) = (
            x.wrapping_add_signed(walk::DX8[d]),
            y.wrapping_add_signed(walk::DY8[d]),
        );
        let hit = seams::objects_at(s, nx, ny, z)
            .iter()
            .any(|f| walk::obj_type(s, f.id) == 0x77);
        if hit {
            return d as u16 + 1;
        }
    }
    0
}

/// Original: 0x11dd56 (actors: the object the scheduled work needs at the actor's location)
///
/// `kind` indexes the four types at A4-0x6aa6 (chair, bed, "chair at a table", bell [likely]).
/// A chair also matches part 2 of type 0x147; "chair at a table" is a chair with a 0x77 beside.
pub fn work_object(s: &State, actor: u16, kind: usize) -> Option<ObjectId> {
    let p = s.objects.location[usize::from(actor)];
    let z = u16::from(p.z);
    let want = s.actors.game.work_objects[kind];
    seams::objects_at(s, p.x, p.y, z)
        .into_iter()
        .find(|&Found { id, part }| {
            let t = walk::obj_type(s, id);
            match want {
                0xe4 => t == 0xfc && table_beside(s, p.x, p.y, z) != 0,
                0xfc => t == 0xfc || (t == 0x147 && walk::obj_frame(s, id).wrapping_sub(part) == 2),
                _ => t == want,
            }
        })
        .map(|f| f.id)
}

/// [combat] 0x1221da (a sleeper with no bed lies down [likely]). Stub contract: argument the
/// actor; no result; the actor keeps its frame (sleeping, status bit 0x04 set).
fn lie_down(_s: &mut State, _actor: u16) {
    crate::todo_stub!("0x1221da lie down without a bed (fallback: none)", ())
}

/// Original: 0x11de8c (actors: arrive at the schedule location: start the scheduled work)
///
/// The worktype becomes the entry's. Sleep (0x91): asleep, on a bed's head or foot the sleeping
/// shape 0x92; sit (0x92) / 0x95: on the chair (0x95 turns the chair to frame 2 and the actor
/// into type 0x188 frame 4); eat (0x93): face the table; 0x98: the bell; 0x87..0x8e face a
/// fixed way. Those five work anywhere; an actor outside the window goes dormant (0x80). Any
/// other work needs the exact scheduled tile, else the actor goes back to 0x81 (a 0x188 actor
/// becomes 0x182 again).
pub fn arrive(s: &mut State, actor: u16) {
    let a = usize::from(actor);
    let Some(e) = schedule_entry(s, actor) else {
        return;
    };
    let wt = e.worktype;
    let mut at = s.objects.location[a] == e.pos;
    s.actors.worktype[a] = wt;
    if wt == 0x91 {
        at = true;
        s.actors.attrs.status[a] |= 0x04;
        if walk::outside_window(s, actor) {
            s.actors.worktype[a] = 0x80;
        } else {
            let p = s.objects.location[a];
            let bed = seams::objects_at(s, p.x, p.y, u16::from(p.z))
                .into_iter()
                .find(|&Found { id, part }| {
                    let rel = walk::obj_frame(s, id).wrapping_sub(part);
                    walk::obj_type(s, id) == 0xa3 && (rel == 0 || rel == 6)
                });
            match bed {
                Some(Found { id, part }) => {
                    let head = walk::obj_frame(s, id).wrapping_sub(part) == 0;
                    s.objects.type_frame[a] = if head { 0x92 } else { 0x92 + 0x400 };
                }
                None => lie_down(s, actor),
            }
        }
    } else {
        s.actors.attrs.status[a] &= !0x04;
        s.objects.type_frame[a] = s.actors.attrs.base_type_frame[a];
        if wt == 0x92 || wt == 0x95 {
            at = true;
            if walk::outside_window(s, actor) {
                s.actors.worktype[a] = 0x80;
            } else if let Some(chair) = work_object(s, actor, 0) {
                if wt == 0x95 {
                    let w = seams::obj_word(s, chair);
                    seams::set_obj_word(s, chair, (w & 0x3ff) + 0x800);
                    walk::face(s, actor, 4);
                    s.objects.type_frame[a] = 0x188 + 0x1000;
                } else {
                    walk::face(s, actor, 0);
                }
            }
        } else if wt == 0x93 {
            at = true;
            if walk::outside_window(s, actor) {
                s.actors.worktype[a] = 0x80;
            } else if let Some(chair) = work_object(s, actor, 2) {
                let p = seams::obj_pos(s, chair);
                let d = table_beside(s, p.x, p.y, u16::from(p.z));
                let w = seams::obj_word(s, chair);
                seams::set_obj_word(s, chair, (w & 0x3ff).wrapping_add((d >> 1) << 10));
                walk::face(s, actor, d.wrapping_sub(1));
            } else {
                walk::face(s, actor, 4);
            }
        } else if wt == 0x98 {
            at = true;
            if walk::outside_window(s, actor) {
                s.actors.worktype[a] = 0x80;
            } else {
                let _ = work_object(s, actor, 3);
            }
        } else if (0x87..=0x8e).contains(&wt) {
            let d = (u16::from(wt) - 0x87) & 3;
            let mf = &mut s.actors.attrs.movement_flags[a];
            *mf = (*mf & 0xf8) | (d * 2) as u8;
            walk::face(s, actor, d * 2);
        }
    }
    if !at {
        s.actors.worktype[a] = 0x81;
        if s.objects.type_frame[a] & 0x3ff == 0x188 {
            s.objects.type_frame[a] = 0x182;
            walk::face(s, actor, 4);
        }
    }
}

/// Original: 0x11e60c (actors: teleport to the schedule location when neither end is in view)
///
/// Unless forced (A4-0x6a9e), an actor within 5 tiles of the leader on its level, or whose
/// scheduled place is, stays. Otherwise it frees its path slot (walkers 0x83..0x85), is placed
/// at the scheduled location, [`arrive`]s, pays 10 points; counts toward the 3 per turn.
pub fn teleport_home(s: &mut State, actor: u16) {
    let a = usize::from(actor);
    let p = s.objects.location[a];
    let c = s.map.centre;
    let near = |x: u16, y: u16| {
        let (x, y, cx, cy) = (x as i16, y as i16, c.x as i16, c.y as i16);
        (cx - 5..=cx + 5).contains(&x) && (cy - 5..=cy + 5).contains(&y)
    };
    let forced = s.actors.work.force_teleport != 0;
    let same_level = p.z == c.z;
    if !forced && same_level && near(p.x, p.y) {
        return;
    }
    let Some(e) = schedule_entry(s, actor) else {
        return;
    };
    if !forced && same_level && near(e.pos.x, e.pos.y) {
        return;
    }
    let wt = s.actors.worktype[a];
    if (0x83..0x86).contains(&wt) {
        let slot = usize::from(s.actors.attrs.unknown_18f1[a]);
        s.actors.set_tail(tail::SLOT_ACTOR + (slot & 0xff), 0);
    }
    s.map.flag_784e = 0;
    seams::move_object(s, ObjectId(actor), e.pos.x, e.pos.y, u16::from(e.pos.z));
    s.map.flag_784e = 1;
    arrive(s, actor);
    walk::spend_mp(s, actor, 10);
    s.actors.work.teleports = s.actors.work.teleports.wrapping_add(1);
}

/// Original: 0x11e9d6 (actors: reverse the path in slot `slot` (directions flipped))
pub fn reverse_path(s: &mut State, slot: usize) {
    let n = (0..32).take_while(|&i| path_byte(s, slot, i) != 0).count();
    for i in 0..n.div_ceil(2) {
        let a = path_byte(s, slot, i) ^ 2;
        let b = path_byte(s, slot, n - 1 - i) ^ 2;
        set_path_byte(s, slot, i, b);
        set_path_byte(s, slot, n - 1 - i, a);
    }
}

/// The 4-neighbour of (col, row) with the lowest grid value (`floor` = only values ≥ it), ties
/// kept on `last` (0x11e2a6's two walks).
fn lowest_neighbour(s: &State, col: i16, row: i16, last: usize, floor: u8) -> usize {
    let mut best = 0xffu16;
    let mut best_d = 0;
    for d in 0..4 {
        let (nc, nr) = (col + DX4[d], row + DY4[d]);
        if !inside(nc, nr) {
            continue;
        }
        let v = s.actors.path.grid[cell(nc, nr)];
        if v < floor {
            continue;
        }
        let v = u16::from(v);
        if v < best || (v == best && last == d) {
            best = v;
            best_d = d;
        }
    }
    best_d
}

/// Original: 0x11e2a6 (actors: store the found path into the slot being filled)
///
/// Walks from the meeting cell down to the start (grid value 0), prepending each move reversed;
/// for a two-sided search it then walks from the meeting cell down to the goal (0x80) on the
/// goal side, appending moves. Runs of one direction share a byte; at most 31 bytes, 0-ended.
pub fn store_path(s: &mut State, col: i16, row: i16) {
    let slot = usize::from(s.actors.work.slot);
    set_path_byte(s, slot, 0, 0);
    let mut len = 0usize;
    let mut last = 0usize;
    let (mut c, mut r) = (col, row);
    let mut guard = 0;
    while s.actors.path.grid[cell(c, r)] != 0 && guard < WALK_LIMIT {
        guard += 1;
        let d = lowest_neighbour(s, c, r, last, 0);
        if !inside(c + DX4[d], r + DY4[d]) {
            break;
        }
        c += DX4[d];
        r += DY4[d];
        last = d;
        let back = (d ^ 2) as u8;
        if len != 0 && back == path_byte(s, slot, 0) & 3 {
            let b = path_byte(s, slot, 0);
            set_path_byte(s, slot, 0, b.wrapping_add(4));
            continue;
        }
        if len < 31 {
            len += 1;
        }
        for i in (1..=len).rev() {
            let b = path_byte(s, slot, i - 1);
            set_path_byte(s, slot, i, b);
        }
        set_path_byte(s, slot, 0, back + 4);
    }
    if s.actors.path.seek_type < 0 && s.actors.path.seek_edge < 0 {
        let (mut c, mut r) = (col, row);
        s.actors.path.grid[cell(c, r)] = 0xff;
        let mut guard = 0;
        while s.actors.path.grid[cell(c, r)] > 0x80 && guard < WALK_LIMIT {
            guard += 1;
            let d = lowest_neighbour(s, c, r, last, 0x80);
            if !inside(c + DX4[d], r + DY4[d]) {
                break;
            }
            c += DX4[d];
            r += DY4[d];
            last = d;
            if len >= 31 {
                break;
            }
            if len != 0 && path_byte(s, slot, len - 1) & 3 == d as u8 {
                let b = path_byte(s, slot, len - 1);
                set_path_byte(s, slot, len - 1, b.wrapping_add(4));
            } else {
                set_path_byte(s, slot, len, d as u8 + 4);
                len += 1;
            }
        }
    }
    set_path_byte(s, slot, len, 0);
}

/// Original: 0x11e7a4 (actors: reach a window cell at `cost` from side `side` (0 start, 0x80
/// goal))
///
/// Meeting the other side, the sought object type (0xe4 = a chair beside a table) or the sought
/// window edge ends the search; else a cheaper reach queues the cell in its cost bucket.
pub fn reach(s: &mut State, cost: i16, col: i16, row: i16, side: u8) {
    if s.actors.path.found != 0 || cost > 0x7f || !inside(col, row) {
        return;
    }
    let i = cell(col, row);
    let g = s.actors.path.grid[i];
    if g != 0xff && g & 0x80 != side {
        s.actors.path.found = 1;
        store_path(s, col, row);
        return;
    }
    let want = s.actors.path.seek_type;
    if want >= 0 {
        let (x0, y0) = window(s);
        let (x, y, z) = (
            (col as u16).wrapping_add(x0),
            (row as u16).wrapping_add(y0),
            walk::party_z(s),
        );
        let hit = seams::objects_at(s, x, y, z).into_iter().any(|f| {
            let t = walk::obj_type(s, f.id);
            if want == 0xe4 {
                t == 0xfc && table_beside(s, x, y, z) != 0
            } else {
                t == want as u16
            }
        });
        if hit {
            s.actors.path.found = 1;
            store_path(s, col, row);
            return;
        }
    }
    let edge = s.actors.path.seek_edge;
    if edge >= 0 {
        let at_edge = (col == 0 && edge == 3)
            || (col == LAST && edge == 1)
            || (row == 0 && edge == 0)
            || (row == LAST && edge == 2);
        if at_edge {
            s.actors.path.found = 1;
            s.actors.path.edge_cell = (col, row);
            store_path(s, col, row);
            return;
        }
    }
    if i16::from(s.actors.path.grid[i] & 0x7f) <= cost {
        return;
    }
    let n = s.actors.path.free;
    if n < 0 {
        return;
    }
    let p = &mut s.actors.path;
    if i16::from(p.top) < cost {
        p.top = cost as u8;
    }
    let n = n as usize;
    p.free = p.next[n];
    p.next[n] = p.heads[cost as usize];
    p.col[n] = col as u8;
    p.row[n] = row as u8;
    p.heads[cost as usize] = n as i16;
    p.grid[i] = cost as u8 | side;
}

/// What [`seek`] looks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// A map location (the schedule entry's).
    At(u16, u16),
    /// The nearest object of a type (worktype 0x82; 0xe4 = a chair beside a table).
    Type(u16),
}

/// Original: 0x11ea54 (actors: find a path for `actor` and start walking it)
///
/// Standing on the target: [`arrive`]. If the actor or the target is outside the 40 × 40
/// window (or on another level): at most 3 per turn [`teleport_home`]; if still 0x81 and the
/// actor itself is outside, the search runs from the target toward the window edge facing the
/// actor, which then jumps to where the path meets the edge and walks it reversed. Each search
/// counts toward the 3 per turn. Found: worktype 0x83 in the slot being filled
/// ([`ActorWork::slot`](super::ActorWork)); not found: 0x81 → 0x86 (unreachable), 0x82 → 0x80.
pub fn seek(s: &mut State, actor: u16, goal: Goal) {
    let a = usize::from(actor);
    s.actors.path.found = 0;
    s.actors.path.seek_edge = -1;
    let (x0, y0) = window(s);
    let p = s.objects.location[a];
    let mut ac = p.x.wrapping_sub(x0) as i16;
    let mut ar = p.y.wrapping_sub(y0) as i16;
    let mut swapped = false;
    let (mut gc, mut gr) = (0i16, 0i16);
    match goal {
        Goal::Type(t) => s.actors.path.seek_type = t as i16,
        Goal::At(tx, ty) => {
            gc = tx.wrapping_sub(x0) as i16;
            gr = ty.wrapping_sub(y0) as i16;
            s.actors.path.seek_type = -1;
            if ac == gc && ar == gr {
                arrive(s, actor);
                return;
            }
            let in_window = u16::from(p.z) == walk::party_z(s) && inside(ac, ar) && inside(gc, gr);
            if !in_window {
                if s.actors.work.teleports >= 3 {
                    return;
                }
                teleport_home(s, actor);
                if s.actors.worktype[a] != 0x81 {
                    return;
                }
                if !inside(ac, ar) {
                    std::mem::swap(&mut ac, &mut gc);
                    std::mem::swap(&mut ar, &mut gr);
                    swapped = true;
                }
                let (ex, ey) = ((gc - 20).abs(), (gr - 20).abs());
                s.actors.path.seek_edge = if ex > ey {
                    if gc < 20 { 3 } else { 1 }
                } else if gr < 20 {
                    0
                } else {
                    2
                };
            }
        }
    }
    s.actors.work.searches = s.actors.work.searches.wrapping_add(1);
    build_cost_map(s, actor);
    let p = &mut s.actors.path;
    p.grid.fill(0xff);
    for (i, n) in p.next.iter_mut().enumerate() {
        *n = i as i16 + 1;
    }
    p.heads.fill(-1);
    p.next[NODES - 1] = -1;
    p.found = 0;
    p.free = 0;
    p.top = 0;
    p.bucket = 0;
    reach(s, 0, ac, ar, 0);
    if s.actors.path.seek_type == -1 && s.actors.path.seek_edge == -1 {
        reach(s, 0, gc, gr, 0x80);
    }
    expand(s);
    if s.actors.path.found != 0 {
        let slot = s.actors.work.slot;
        if swapped {
            let (c, r) = s.actors.path.edge_cell;
            let z = u16::from(s.objects.location[a].z);
            s.map.flag_784e = 0;
            let (x, y) = ((c as u16).wrapping_add(x0), (r as u16).wrapping_add(y0));
            seams::move_object(s, ObjectId(actor), x, y, z);
            s.map.flag_784e = 1;
            reverse_path(s, usize::from(slot));
        }
        s.actors.worktype[a] = 0x83;
        s.actors.attrs.unknown_18f1[a] = slot;
        let si = usize::from(slot);
        s.actors.set_tail(tail::SLOT_ACTOR + si, actor as u8);
        s.actors.set_tail(tail::SLOT_POS + si, 0);
        s.actors.set_tail(tail::SLOT_STEP + si, 0);
    } else if s.actors.worktype[a] == 0x81 {
        s.actors.worktype[a] = 0x86;
    } else if s.actors.worktype[a] == 0x82 {
        s.actors.worktype[a] = 0x80;
    }
}

/// The bucket loop of 0x11ea54: pop the lowest bucket, reach the four neighbours at the
/// popped cost plus each neighbour's entry cost, until found or the queue is empty.
fn expand(s: &mut State) {
    loop {
        let b = usize::from(s.actors.path.bucket);
        let head = s.actors.path.heads[b];
        if head < 0 || s.actors.path.found != 0 {
            return;
        }
        let n = head as usize;
        let p = &mut s.actors.path;
        p.heads[b] = p.next[n];
        let (c, r) = (i16::from(p.col[n]), i16::from(p.row[n]));
        p.next[n] = p.free;
        p.free = n as i16;
        let g = p.grid[cell(c, r)];
        let (side, base) = (g & 0x80, i16::from(g & 0x7f));
        let cost_at = |p: &super::PathScratch, dc: i16, dr: i16| {
            let i = (r + dr) * GRID as i16 + c + dc;
            usize::try_from(i)
                .ok()
                .and_then(|i| p.cost.get(i))
                .map_or(0, |&v| i16::from(v))
        };
        let (w, e, n_, s_) = (
            cost_at(p, -1, 0),
            cost_at(p, 1, 0),
            cost_at(p, 0, -1),
            cost_at(p, 0, 1),
        );
        reach(s, base + w, c - 1, r, side);
        reach(s, base + e, c + 1, r, side);
        reach(s, base + n_, c, r - 1, side);
        reach(s, base + s_, c, r + 1, side);
        let p = &mut s.actors.path;
        if p.heads[usize::from(p.bucket)] < 0 {
            while p.bucket < p.top && p.heads[usize::from(p.bucket)] < 0 {
                p.bucket += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use u6_formats::position::Position;
    use u6_formats::schedule::ScheduleEntry;

    use super::*;

    /// An empty map (every tile passable, cost 1) with the view at (20, 20) (window origin 0),
    /// NPC 10 at `from` with one schedule entry at `to`.
    fn walker(from: (u16, u16), to: (u16, u16), worktype: u8) -> State {
        let mut s = State::empty();
        s.map.centre = Position { x: 20, y: 20, z: 0 };
        s.objects.type_frame[10] = 0x17a;
        s.actors.attrs.base_type_frame[10] = 0x17a;
        s.objects.location[10] = Position {
            x: from.0,
            y: from.1,
            z: 0,
        };
        s.actors.worktype[10] = 0x81;
        s.actors.attrs.dexterity[10] = 20;
        s.actors.schedules.first = vec![0; 257];
        for a in 11..257 {
            s.actors.schedules.first[a] = 1;
        }
        s.actors.schedules.entries = vec![ScheduleEntry {
            hour: 8,
            day_of_week: 0,
            worktype,
            pos: Position {
                x: to.0,
                y: to.1,
                z: 0,
            },
        }];
        crate::actors::walk::tests::link_objects(&mut s);
        s
    }

    fn path_bytes(s: &State, slot: usize) -> Vec<u8> {
        (0..32)
            .map(|i| path_byte(s, slot, i))
            .take_while(|&b| b != 0)
            .collect()
    }

    #[test]
    fn seek_finds_a_path_and_stores_runs() {
        let mut s = walker((5, 5), (10, 8), 0x90);
        s.actors.work.slot = 0;
        seek(&mut s, 10, Goal::At(10, 8));
        assert_eq!(s.actors.worktype[10], 0x83, "walking");
        assert_eq!(s.actors.attrs.unknown_18f1[10], 0);
        assert_eq!(s.actors.tail(tail::SLOT_ACTOR), 10);
        let bytes = path_bytes(&s, 0);
        // Runs: (direction 0 N 1 E 2 S 3 W, length) adding up to 5 east and 3 south.
        let (mut dx, mut dy) = (0i16, 0i16);
        for &b in &bytes {
            let n = i16::from(b >> 2);
            dx += DX4[usize::from(b & 3)] * n;
            dy += DY4[usize::from(b & 3)] * n;
        }
        assert_eq!((dx, dy), (5, 3), "path {bytes:?}");
        assert_eq!(s.actors.work.searches, 1);
    }

    #[test]
    fn seek_goes_around_an_impassable_wall() {
        let mut s = walker((5, 5), (12, 5), 0x90);
        // An impassable wall at x = 8, y = 0..9 (via a blocking object tile under each cell).
        let mut flags = vec![0u8; 2048];
        flags[7] = 0x02;
        s.tables.tileflags.flags1 = flags;
        s.tables.tileflags.flags2 = vec![0; 2048];
        s.tables.tileflags.flags3 = vec![0; 2048];
        s.tables.basetile.base = vec![0; 1024];
        s.tables.basetile.base[0x300] = 7;
        for y in 0..10 {
            crate::objects::create_on_map(&mut s.objects, 0x300, 0, 0, 8, y, 0);
        }
        s.actors.work.slot = 0;
        seek(&mut s, 10, Goal::At(12, 5));
        assert_eq!(s.actors.worktype[10], 0x83);
        // Walk it: the NPC must reach (12, 5) without crossing x = 8 above y = 10.
        let mut turns = 0;
        while s.actors.worktype[10] == 0x83 && turns < 100 {
            s.actors.movement_points[10] = 20;
            super::super::npc::walk_path(&mut s, 10, 12, 5);
            let p = s.objects.location[10];
            assert!(!(p.x == 8 && p.y < 10), "walked through the wall at {p:?}");
            turns += 1;
        }
        let p = s.objects.location[10];
        assert_eq!((p.x, p.y), (12, 5), "after {turns} steps");
        assert_eq!(s.actors.worktype[10], 0x90, "arrived: the entry's worktype");
    }

    #[test]
    fn standing_on_the_target_arrives_at_once() {
        let mut s = walker((7, 7), (7, 7), 0x8b);
        seek(&mut s, 10, Goal::At(7, 7));
        assert_eq!(s.actors.worktype[10], 0x8b);
        assert_eq!(s.actors.work.searches, 0);
    }

    #[test]
    fn arrival_needs_the_exact_tile_for_plain_work() {
        let mut s = walker((7, 7), (8, 7), 0x90);
        arrive(&mut s, 10);
        assert_eq!(s.actors.worktype[10], 0x81, "one tile off: go there first");
        s.objects.location[10].x = 8;
        arrive(&mut s, 10);
        assert_eq!(s.actors.worktype[10], 0x90);
    }

    #[test]
    fn teleport_home_when_neither_end_is_in_view() {
        let mut s = walker((100, 100), (300, 300), 0x90);
        teleport_home(&mut s, 10);
        let p = s.objects.location[10];
        assert_eq!((p.x, p.y), (300, 300));
        assert_eq!(s.actors.worktype[10], 0x90);
        assert_eq!(s.actors.work.teleports, 1);
        assert_eq!(s.actors.mp(10), -10);
        // Near the leader: stays.
        let mut s = walker((22, 21), (300, 300), 0x90);
        teleport_home(&mut s, 10);
        assert_eq!(s.objects.location[10].x, 22);
    }

    #[test]
    fn reverse_flips_order_and_directions() {
        let mut s = State::empty();
        for (i, b) in [4 * 3 + 1, 4 + 2, 4 * 2].into_iter().enumerate() {
            set_path_byte(&mut s, 2, i, b);
        }
        reverse_path(&mut s, 2);
        assert_eq!(path_bytes(&s, 2), [4 * 2 + 2, 4, 4 * 3 + 3]);
    }

    #[test]
    fn free_slot_releases_slots_of_actors_that_stopped_walking() {
        let mut s = State::empty();
        s.actors.set_tail(tail::SLOT_ACTOR, 10);
        s.actors.set_tail(tail::SLOT_ACTOR + 1, 11);
        s.actors.worktype[10] = 0x84;
        s.actors.worktype[11] = 0x90;
        assert_eq!(super::super::npc::free_slot(&mut s), 1);
        assert_eq!(s.actors.tail(tail::SLOT_ACTOR + 1), 0);
        for k in 0..8 {
            s.actors.set_tail(tail::SLOT_ACTOR + k, 10);
        }
        assert_eq!(super::super::npc::free_slot(&mut s), 8);
    }
}

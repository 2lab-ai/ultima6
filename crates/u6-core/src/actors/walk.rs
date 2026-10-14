//! One actor step: movement-point costs, the "may stand here" test, doors, facing frames.
//!
//! Tile flags (tileflag, 0x13b140 = flags1): 0x01 water, 0x02 impassable, 0x04 wall, 0x08
//! damaging, high nibble = extra move cost. Creature flags: [`super::GameTables`].

use crate::objects::ObjectId;
use crate::rng::Rng;
use crate::state::State;

use super::seams::{self, Found};
use super::timer;

/// 8-way direction deltas, 0 = north, clockwise (read from the image at A4-0x61ce / A4-0x61c6:
/// x 0,1,1,1,0,-1,-1,-1 and y -1,-1,0,1,1,1,0,-1).
pub const DX8: [i16; 8] = [0, 1, 1, 1, 0, -1, -1, -1];
pub const DY8: [i16; 8] = [-1, -1, 0, 1, 1, 1, 0, -1];

pub(crate) fn flags1(s: &State, tile: u16) -> u8 {
    s.tables
        .tileflags
        .flags1
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
}

pub(crate) fn flags2(s: &State, tile: u16) -> u8 {
    s.tables
        .tileflags
        .flags2
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
}

pub(crate) fn flags3(s: &State, tile: u16) -> u8 {
    s.tables
        .tileflags
        .flags3
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
}

/// Tile of an object word: base tile of the type plus the frame (16-bit add, as the original).
pub(crate) fn word_tile(s: &State, word: u16) -> u16 {
    let base = s
        .tables
        .basetile
        .base
        .get(usize::from(word & 0x3ff))
        .copied();
    base.unwrap_or(0).wrapping_add(word >> 10)
}

/// Type (10 bits) of an object.
pub(crate) fn obj_type(s: &State, id: ObjectId) -> u16 {
    seams::obj_word(s, id) & 0x3ff
}

/// Frame (6 bits) of an object.
pub(crate) fn obj_frame(s: &State, id: ObjectId) -> u16 {
    seams::obj_word(s, id) >> 10
}

/// Writes the frame of an object, keeping its type.
pub(crate) fn set_frame(s: &mut State, id: ObjectId, frame: u16) {
    let w = seams::obj_word(s, id);
    seams::set_obj_word(s, id, (w & 0x3ff).wrapping_add(frame << 10));
}

/// The party's level (A4-0x7b74).
pub(crate) fn party_z(s: &State) -> u16 {
    u16::from(s.map.centre.z)
}

/// Original: 0x11c650 (actors: spend movement points)
///
/// Type 0x1af pays half; party members pay half while the party-haste timer runs, otherwise
/// actors with status bit 0x20 pay double while the slow timer runs; the cost is at least 1.
/// The points are a signed byte and may go negative.
pub fn spend_mp(s: &mut State, actor: u16, cost: i16) {
    let a = usize::from(actor);
    let mut c = cost;
    if s.objects.type_frame[a] & 0x3ff == 0x1af {
        c >>= 1;
    }
    let status = s.actors.attrs.status[a];
    if s.actors.timers[timer::HASTE_PARTY] != 0 && status & 0x80 != 0 {
        c >>= 1;
    } else if s.actors.timers[timer::SLOW_EVIL] != 0 && status & 0x20 != 0 {
        c = c.wrapping_add(c);
    }
    if c < 1 {
        c = 1;
    }
    let mp = i16::from(s.actors.mp(a));
    s.actors.set_mp(a, mp.wrapping_sub(c));
}

/// Original: 0x11ef54 (actors: random offset, geometric magnitude, random sign)
///
/// Counts draws of `random(0, 1)` until one is 0, then one more draw picks the sign.
pub fn random_offset(rng: &mut Rng) -> i16 {
    let mut n: i16 = 0;
    while rng.range(0, 1) != 0 {
        n = n.wrapping_add(1);
    }
    if rng.range(0, 1) != 0 {
        n.wrapping_neg()
    } else {
        n
    }
}

/// Original: 0x11ef84 (actors: pay the move cost of the actor's own tile and what lies there)
///
/// 5, plus the cost nibble of the map tile and of every object tile there that is not
/// impassable.
pub fn spend_tile_cost(s: &mut State, actor: u16) {
    let p = s.objects.location[usize::from(actor)];
    let mut cost: i16 = 5;
    let t = u16::from(seams::tile_at(s, p.x, p.y, u16::from(p.z)));
    let f = flags1(s, t);
    if f & 0x02 == 0 {
        cost += i16::from(f >> 4);
    }
    for Found { id, .. } in seams::objects_at(s, p.x, p.y, u16::from(p.z)) {
        let f = flags1(s, word_tile(s, seams::obj_word(s, id)));
        if f & 0x02 == 0 {
            cost += i16::from(f >> 4);
        }
    }
    spend_mp(s, actor, cost);
}

/// Original: 0x11b92a (actors: is the actor outside the map-window cache)
///
/// True when the actor is on another level than the party or outside the 40 × 40 cache.
pub fn outside_window(s: &State, actor: u16) -> bool {
    let p = s.objects.location[usize::from(actor)];
    if u16::from(p.z) != party_z(s) {
        return true;
    }
    let (x0, y0, mask) = seams::window_origin(s);
    let dx = (p.x.wrapping_sub(x0) & mask) as i16;
    let dy = (p.y.wrapping_sub(y0) & mask) as i16;
    dx > 0x27 || dy > 0x27
}

/// Original: 0x11b9ae (actors: is (x, y) on the party's level impassable)
///
/// The map tile is impassable, or an object there (party members excepted) has an impassable
/// tile.
pub fn blocked_at(s: &State, x: u16, y: u16) -> bool {
    let z = party_z(s);
    if flags1(s, u16::from(seams::tile_at(s, x, y, z))) & 0x02 != 0 {
        return true;
    }
    seams::objects_at(s, x, y, z)
        .into_iter()
        .any(|Found { id, part }| {
            let party = id.0 < 0x100 && s.actors.attrs.status[id.index()] & 0x80 != 0;
            let tile = word_tile(s, seams::obj_word(s, id)).wrapping_sub(part);
            !party && flags1(s, tile) & 0x02 != 0
        })
}

/// Movement abilities 0x11b1c0 derives from the mover's type.
struct Mover {
    /// Type 10 bits of the mover.
    kind: u16,
    /// Creature-list index (-1 = not a listed creature).
    creature: i16,
    /// Its tile is double width and double height (flags2 & 0xc0 == 0xc0).
    big: bool,
    /// Walks on land (not a swimmer / flyer, or creature index 1).
    walks: bool,
    /// Swims (creature flag 0x200, or a boat type 0x19e / 0x19f).
    swims: bool,
    /// Boat type 0x19e / 0x19f.
    boat: bool,
    /// Flies (creature flag 0x100).
    flies: bool,
    /// Moves only where its own kind is (creature flag 0x400).
    own_kind: bool,
}

fn mover(s: &State, actor: u16) -> Mover {
    let word = s.objects.type_frame[usize::from(actor)];
    let kind = word & 0x3ff;
    let creature = seams::creature_index(s, kind);
    let flags = s.actors.game.creature_flags(creature);
    let flies = flags & 0x100 != 0;
    let boat = kind == 0x19e || kind == 0x19f;
    let swims = flags & 0x200 != 0 || boat;
    Mover {
        kind,
        creature,
        big: flags2(s, word_tile(s, word)) & 0xc0 == 0xc0,
        walks: !(swims || flies) || creature == 1,
        swims,
        boat,
        flies,
        own_kind: flags & 0x400 != 0,
    }
}

/// Original: 0x11b1c0 (actors: may the actor stand at (x, y) on the party's level)
///
/// First the map tile: walkers need it passable, swimmers impassable water, boats water,
/// flyers no wall, own-kind movers anything; type 0x168 only damaging tiles. A damaging tile
/// sets A4-0x6a96 and fails while A4-0x6a8a is set. Then every object there may veto or allow
/// it, in the original's order (see the body); party members may swap with healthy fellow
/// members other than the leader while A4-0x6a8c is set.
pub fn can_move_to(s: &mut State, actor: u16, x: u16, y: u16) -> bool {
    s.actors.work.saw_damage = 0;
    let m = mover(s, actor);
    let a = usize::from(actor);
    let leader = s.actors.party.leader().map_or(0, |l| l.0);
    let z = party_z(s);
    let f = flags1(s, u16::from(seams::tile_at(s, x, y, z)));
    let mut ok = false;
    if m.kind == 0x168 {
        ok = f & 0x08 != 0;
    } else {
        if (m.walks && f & 0x02 == 0)
            || (m.swims && f & 0x02 != 0 && f & 0x01 != 0)
            || (m.boat && f & 0x01 != 0)
            || (m.flies && f & 0x04 == 0)
            || m.own_kind
        {
            ok = true;
        }
        if f & 0x08 != 0 {
            s.actors.work.saw_damage = 1;
            if s.actors.work.avoid_damage != 0 {
                return false;
            }
        }
    }
    let me = s.objects.location[a];
    let my_tile_flags = flags1(s, word_tile(s, s.objects.type_frame[a]));
    for Found { id, part } in seams::objects_at(s, x, y, z) {
        if id.0 == actor {
            continue;
        }
        let other = obj_type(s, id);
        if other == 0x1a0 && s.actors.quest_flag_copy == 0 {
            if s.actors.work.hic != 0 {
                seams::print(s, -0x73e0, &[]);
                s.actors.work.hic = 0;
                s.actors.work.prompt = 1;
            }
            return false;
        }
        // The other tiles of a multi-tile mover: only the ones on its far side block.
        if (m.kind >= 0x1aa || m.kind == 0x19c) && id.0 >= 0x100 && other == m.kind {
            let f = my_tile_flags;
            if (f & 0x80 != 0 && y < me.y)
                || (f & 0x40 != 0 && x > me.x)
                || (f & 0x20 != 0 && y > me.y)
                || (f & 0x10 != 0 && x < me.x)
            {
                continue;
            }
        }
        if m.kind == 0x162 && matches!(other, 0xd5 | 0x11a | 0x136) {
            ok = true;
            continue;
        }
        let otile = word_tile(s, seams::obj_word(s, id)).wrapping_sub(part);
        if m.big && part == 0 && flags2(s, otile) & 0xc0 != 0 {
            return false;
        }
        if m.walks && flags3(s, otile) & 0x04 != 0 {
            ok = true;
            if flags3(s, otile) & 0x10 == 0 {
                return ok;
            }
        }
        let of = flags1(s, otile);
        if of & 0x08 != 0 {
            s.actors.work.saw_damage = 1;
            if s.actors.work.avoid_damage != 0 {
                return false;
            }
        }
        let mut keep = true;
        if m.kind == 0x19d && other == 0x19d {
            // Serpent segments: blocked only by the segment right behind (word 0x600 high byte).
            let seg = |i: usize| s.objects.qq.get(i).copied().unwrap_or(0) >> 8;
            if seg(a) + 1 == seg(id.index()) {
                keep = false;
                ok = false;
            }
        } else {
            match object_verdict(s, &m, actor, leader, id, other, of, otile) {
                Verdict::Neutral => {}
                Verdict::Skip => continue,
                Verdict::Block => {
                    keep = false;
                    ok = false;
                }
            }
        }
        if id.0 < 0x100 && id.0 != 0 {
            let small = matches!(other, 0x164 | 0x162 | 0x157 | 0x167 | 0x165);
            if !small || other == m.kind {
                return false;
            }
        }
        if of & 0x04 != 0 && matches!(m.kind, 0x16a | 0x1a8 | 0x19b | 0x1a9) {
            keep = false;
            ok = false;
        }
        if !keep {
            break;
        }
    }
    ok
}

/// What one object at the destination does to a move (0x11b1c0 after the type checks).
enum Verdict {
    /// No effect; the actor and wall checks still run.
    Neutral,
    /// Ignored entirely (a fellow party member to swap with).
    Skip,
    /// Blocks.
    Block,
}

#[allow(clippy::too_many_arguments)]
fn object_verdict(
    s: &State,
    m: &Mover,
    actor: u16,
    leader: u16,
    id: ObjectId,
    other: u16,
    of: u8,
    otile: u16,
) -> Verdict {
    let f3 = flags3(s, otile);
    if m.walks && of & 0x02 == 0 {
        return Verdict::Neutral;
    }
    if m.swims && ((of & 0x02 != 0 && of & 0x01 != 0) || f3 & 0x10 != 0) {
        return Verdict::Neutral;
    }
    if m.boat && (of & 0x01 != 0 || f3 & 0x10 != 0) {
        return Verdict::Neutral;
    }
    if m.flies && of & 0x04 == 0 {
        if id.0 >= 0x100 {
            return Verdict::Neutral;
        }
        let ci = seams::creature_index(s, other);
        if s.actors.game.creature_flags(ci) & 0x100 == 0 {
            return Verdict::Neutral;
        }
    }
    if m.own_kind && id.0 >= 0x100 {
        return Verdict::Neutral;
    }
    let status = &s.actors.attrs.status;
    // The original reads A4+0x3d0c + mover for any mover, an object slot (≥ 0x100: a vessel
    // being boarded, a dropped or pushed object) included, i.e. past the 256 status bytes into
    // the tables after them. The port reads 0 there: an object is not a party member.
    let mover_status = status.get(usize::from(actor)).copied().unwrap_or(0);
    if mover_status & 0x80 != 0 {
        let fellow = id.0 != leader
            && s.actors.work.swap_ok != 0
            && id.0 < 0x100
            && status[id.index()] & 0x80 != 0
            && status[id.index()] & 0x16 == 0
            && s.actors.attrs.movement_flags[id.index()] & 0x10 == 0;
        return if fellow {
            Verdict::Skip
        } else {
            Verdict::Block
        };
    }
    if actor < 0x100 {
        let door = (0x129..=0x12c).contains(&other) && obj_frame(s, id) < 8;
        if (door || other == 0x116 || other == 0x118)
            && s.actors.game.creature_flags(m.creature) & 0x4000 != 0
        {
            return Verdict::Neutral;
        }
    }
    Verdict::Block
}

/// Original: 0x11db7c (actors: a door-opening NPC opens / closes the door it stands in)
///
/// Only creatures with flag 0x4000 outside the party. A door (0x129..0x12c) at the actor's
/// location gets frame `4 × closed + (frame & 3)` and the view flags are recomputed; a
/// portcullis (0x116 / 0x118) is operated through 0x1301e2 with A4-0x7a4c cleared around it.
pub fn door_at_actor(s: &mut State, actor: u16, closed: u16) {
    let a = usize::from(actor);
    let ci = seams::creature_index(s, s.objects.type_frame[a] & 0x3ff);
    if ci < 0 || s.actors.game.creature_flags(ci) & 0x4000 == 0 {
        return;
    }
    if s.actors.attrs.status[a] & 0x80 != 0 {
        return;
    }
    let p = s.objects.location[a];
    for Found { id, .. } in seams::objects_at(s, p.x, p.y, u16::from(p.z)) {
        let t = obj_type(s, id);
        if (0x129..=0x12c).contains(&t) {
            let f = obj_frame(s, id);
            set_frame(s, id, (closed << 2).wrapping_add(f & 3));
            seams::refresh_view_flags(s);
            return;
        }
        if t == 0x116 || t == 0x118 {
            s.actors.work.portcullis_guard = 0;
            operate_portcullis(s, id);
            s.actors.work.portcullis_guard = 1;
        }
    }
}

/// [objects] 0x1301e2 (Use on a portcullis; "Not now!"). Stub contract: argument the
/// portcullis object, no result; the portcullis stays as it is; reached only by door-opening
/// NPCs standing in a portcullis (not in the start area).
fn operate_portcullis(_s: &mut State, _id: ObjectId) {
    crate::todo_stub!("0x1301e2 operate a portcullis (fallback: unchanged)", ())
}

/// Original: 0x11f0a0 (actors: try one step in direction `dir`, paying the current tile)
///
/// Pays [`spend_tile_cost`] first, whether or not the step happens. In combat mode a party
/// member may not end more than 8 tiles from the leader. Then [`can_move_to`] with damaging
/// tiles refused while `avoid_damage`; on success the actor's doors are handled, it moves (on
/// the party's level), its movement flags keep the direction, and its frame turns.
pub fn step(s: &mut State, actor: u16, dir: u16, avoid_damage: u16) -> bool {
    let d = usize::from(dir & 7);
    spend_tile_cost(s, actor);
    let a = usize::from(actor);
    let p = s.objects.location[a];
    let nx = p.x.wrapping_add_signed(DX8[d]);
    let ny = p.y.wrapping_add_signed(DY8[d]);
    if s.actors.attrs.status[a] & 0x80 != 0 && s.actors.combat_mode() {
        let c = s.map.centre;
        let far = |a: u16, b: u16| (a as i16).wrapping_sub(b as i16).unsigned_abs() > 8;
        if far(c.x, nx) || far(c.y, ny) {
            return false;
        }
    }
    s.actors.work.avoid_damage = avoid_damage;
    if !can_move_to(s, actor, nx, ny) {
        s.actors.work.avoid_damage = 0;
        return false;
    }
    s.actors.work.avoid_damage = 0;
    door_at_actor(s, actor, 1);
    seams::move_object(s, ObjectId(actor), nx, ny, party_z(s));
    let mf = &mut s.actors.attrs.movement_flags[a];
    *mf = (*mf & 0xf8) | d as u8;
    s.actors.work.stepped = 1;
    face(s, actor, d as u16);
    door_at_actor(s, actor, 0);
    true
}

/// Facing (0..3 = N, E, S, W) after a step in 8-way `dir`, from the current facing `cur`: a
/// straight step faces its way; a diagonal one keeps `cur` when it is one of the step's two
/// components, else turns around (0x11ba5e's repeated rule).
fn turn(dir: u16, cur: u16) -> u16 {
    let want = dir >> 1;
    if dir & 1 == 0 {
        return want;
    }
    if want.wrapping_sub(cur).wrapping_add(1) & 3 > 1 {
        cur.wrapping_add(2) & 3
    } else {
        cur
    }
}

/// The three-frame walk cycle (frames 0, 1, 2 with movement-flag bit 7 as the direction):
/// `phase` is the current frame within the facing; returns the next.
fn walk_cycle(s: &mut State, a: usize, phase: u16, low: u16, mid: u16, high: u16) -> u16 {
    let mf = &mut s.actors.attrs.movement_flags[a];
    if phase == low {
        *mf &= 0x7f;
        mid
    } else if phase == mid {
        if *mf & 0x80 == 0 { high } else { low }
    } else if phase == high {
        *mf |= 0x80;
        mid
    } else {
        mid
    }
}

/// Original: 0x11ba5e (actors: set the actor's frame for a step / facing `dir`)
///
/// Humanoids (0x178..0x183, 0x199, 0x19a) sit on a chair or bed head in view, else face and
/// cycle three walk frames. Other shapes by type: 12- and 3-frame cycles (0x16a, 0x16b), a
/// random frame (0x164), two-frame walkers (with the other half of a two-tile creature moved
/// behind it), the ship's bow and stern (0x19c), facing-only types, 0x1a8 (with a random byte
/// for A4-0x7afc), the four parts of 0x19b. Clears A4-0x6a9c at the end.
pub fn face(s: &mut State, actor: u16, dir: u16) {
    let a = usize::from(actor);
    let me = ObjectId(actor);
    let mut seat: Option<(u16, ObjectId)> = None;
    if !outside_window(s, actor) {
        let p = s.objects.location[a];
        for Found { id, part } in seams::objects_at(s, p.x, p.y, u16::from(p.z)) {
            let t = obj_type(s, id);
            if t == 0xfc || (t == 0x147 && obj_frame(s, id).wrapping_sub(part) == 2) {
                seat = Some((t, id));
                break;
            }
        }
    }
    let t = s.objects.type_frame[a] & 0x3ff;
    let f = s.objects.type_frame[a] >> 10;
    let mut dir = dir;
    let humanoid = (0x178..=0x183).contains(&t) || (0x199..=0x19a).contains(&t);
    if humanoid {
        let (facing, phase) = match seat {
            Some((0xfc, chair)) => (obj_frame(s, chair), 3),
            Some(_) => (2, 3),
            None => (turn(dir, f >> 2), walk_cycle(s, a, f & 3, 0, 1, 2)),
        };
        set_frame(s, me, facing * 4 + phase);
    } else if t == 0x16a {
        let facing = turn(dir, f / 12);
        let phase = walk_cycle(s, a, f % 12, 3, 7, 11);
        set_frame(s, me, facing * 12 + phase);
    } else if t == 0x164 {
        let r = s.rng.range(0, 2);
        set_frame(s, me, r);
    } else if t == 0x16b {
        let facing = turn(dir, f / 3);
        let phase = walk_cycle(s, a, f % 3, 0, 1, 2);
        set_frame(s, me, facing * 3 + phase);
    } else if matches!(t, 0x15a | 0x156 | 0x15c..=0x15f | 0x166 | 0x169 | 0x16f..=0x174 | 0x188)
        || t >= 0x1aa
    {
        let mut facing = f >> 1;
        let mut phase = f;
        if t < 0x1aa || s.actors.work.stepped != 0 {
            if dir & 1 != 0 {
                let p = s.objects.location[a];
                let d = usize::from(dir);
                if blocked_at(s, p.x, p.y.wrapping_sub(DY8[d] as u16)) {
                    dir = (dir & 4) + 2;
                } else if blocked_at(s, p.x.wrapping_sub(DX8[d] as u16), p.y) {
                    dir = (dir + 1) & 4;
                }
            }
            facing = turn(dir, facing);
            phase = if f & 1 == 0 { 1 } else { 0 };
        }
        let new = facing * 2 + (phase & 1);
        if t >= 0x1aa && s.actors.work.stepped != 0 {
            move_rear_half(s, actor, new, facing);
        }
        set_frame(s, me, new);
    } else if t == 0x19c && s.actors.work.stepped != 0 {
        steer_ship(s, actor, dir, f);
    } else if matches!(t, 0x162 | 0x167 | 0x19e | 0x184) {
        set_frame(s, me, turn(dir, f));
    } else if t == 0x1a8 {
        let facing = turn(dir, f >> 3);
        let phase = if f & 4 == 0 { 7 } else { 3 };
        set_frame(s, me, facing * 8 + phase);
        if !outside_window(s, actor) {
            s.gfx.panel_redraw = s.rng.range(2, 4) as u8;
        }
    } else if t == 0x19b && f < 8 {
        let p = s.objects.location[a];
        for k in 0..4u16 {
            let part = dragon_part(s, p.x, p.y, party_z(s), k * 2, f);
            if part >= 0 {
                let i = usize::from((dir & 6) * 2 + k);
                let fr = s.actors.game.part_frames[i];
                set_frame(s, ObjectId(part as u16), u16::from(fr));
            }
        }
        set_frame(s, me, dir & 6);
    }
    s.actors.work.stepped = 0;
}

/// 0x11ba5e for two-tile creatures (type ≥ 0x1aa) that stepped: the other half (0x10a5b0)
/// takes the matching frame (`new & 7` + 8 for a tail part, `(new & 7) ^ 4` otherwise) and
/// moves behind the head.
fn move_rear_half(s: &mut State, actor: u16, new: u16, facing: u16) {
    let rear = seams::other_half(s, actor);
    if rear < 0 {
        return;
    }
    let rear = ObjectId(rear as u16);
    let rf = obj_frame(s, rear);
    let fr = if rf & 8 != 0 {
        (new & 7) + 8
    } else {
        (new & 7) ^ 4
    };
    set_frame(s, rear, fr);
    let p = s.objects.location[usize::from(actor)];
    let d = usize::from(facing * 2);
    let (x, y) = (
        p.x.wrapping_sub(DX8[d] as u16),
        p.y.wrapping_sub(DY8[d] as u16),
    );
    seams::move_object(s, rear, x, y, party_z(s));
}

/// 0x11ba5e for the ship (0x19c) after a step: bow (frame < 8) and stern (frame > 15) parts in
/// front of / behind the hull by its old facing turn with it and move to the new front / back.
fn steer_ship(s: &mut State, actor: u16, dir: u16, f: u16) {
    let me = ObjectId(actor);
    let facing = turn(dir, (f & 6) >> 1);
    let hull = facing * 2 + 9;
    let p = s.objects.location[usize::from(actor)];
    let z = party_z(s);
    let old = usize::from(f & 6);
    let new = usize::from(facing * 2);
    let ends = [
        (DX8[old], DY8[old], 1i16, (f & 6) >> 1),
        (-DX8[old], -DY8[old], -1i16, ((f & 6) >> 1) ^ 2),
    ];
    for (k, (dx, dy, sign, side)) in ends.into_iter().enumerate() {
        let (x, y) = (p.x.wrapping_add_signed(dx), p.y.wrapping_add_signed(dy));
        for Found { id, .. } in seams::objects_at(s, x, y, z) {
            if id.0 < 0x100 || obj_type(s, id) != 0x19c {
                continue;
            }
            let pf = obj_frame(s, id);
            let wanted = if k == 0 { pf < 8 } else { pf > 0xf };
            let bit = s.actors.game.ship_sides[usize::from(side)];
            if !wanted || flags1(s, word_tile(s, seams::obj_word(s, id))) & bit == 0 {
                continue;
            }
            let fr = if k == 0 { hull - 8 } else { hull + 8 };
            set_frame(s, id, fr);
            let (nx, ny) = (
                p.x.wrapping_add_signed(sign * DX8[new]),
                p.y.wrapping_add_signed(sign * DY8[new]),
            );
            seams::move_object(s, id, nx, ny, z);
            break;
        }
    }
    set_frame(s, me, hull);
}

/// [objects] 0x10b922: part `part` (0, 2, 4, 6) of the four-tile creature at (x, y, z) with
/// head frame `frame`, -1 if none. Stub contract: returns -1, so the parts keep their frames
/// (only the 0x19b creature's turn reaches it; none in the start area).
fn dragon_part(_s: &State, _x: u16, _y: u16, _z: u16, _part: u16, _frame: u16) -> i16 {
    crate::todo_stub!("0x10b922 part of a four-tile creature (fallback: none)", -1)
}

#[cfg(test)]
pub(crate) mod tests {
    use u6_formats::position::Position;

    use super::*;

    #[test]
    fn spend_mp_modifiers() {
        let mut s = State::empty();
        s.actors.movement_points[3] = 20;
        spend_mp(&mut s, 3, 5);
        assert_eq!(s.actors.mp(3), 15);
        s.objects.type_frame[3] = 0x1af;
        spend_mp(&mut s, 3, 5);
        assert_eq!(s.actors.mp(3), 13, "type 0x1af pays half");
        s.objects.type_frame[3] = 0;
        s.actors.timers[timer::SLOW_EVIL] = 1;
        s.actors.attrs.status[3] = 0x20;
        spend_mp(&mut s, 3, 5);
        assert_eq!(s.actors.mp(3), 3, "slowed evil pays double");
        s.actors.timers[timer::HASTE_PARTY] = 1;
        s.actors.attrs.status[3] = 0xa0;
        spend_mp(&mut s, 3, 1);
        assert_eq!(s.actors.mp(3), 2, "hasted party pays half, at least 1");
        spend_mp(&mut s, 3, 10);
        assert_eq!(s.actors.mp(3), -3, "points go negative");
    }

    #[test]
    fn random_offset_draws_until_zero_then_a_sign() {
        let mut a = Rng::new();
        let mut b = Rng::new();
        let r = random_offset(&mut a);
        let mut n = 0i16;
        while b.range(0, 1) != 0 {
            n += 1;
        }
        let expect = if b.range(0, 1) != 0 { -n } else { n };
        assert_eq!(r, expect);
        assert_eq!(a, b);
    }

    /// Tables where type 0x17a (frames 0..63) has impassable tiles 100.., as creature tiles do
    /// in the real tileflag.
    pub(crate) fn creature_tiles(s: &mut State) {
        s.tables.basetile.base = vec![0; 1024];
        s.tables.basetile.base[0x17a] = 100;
        let mut f1 = vec![0u8; 2048];
        f1[100..164].fill(0x02);
        s.tables.tileflags.flags1 = f1;
        s.tables.tileflags.flags2 = vec![0; 2048];
        s.tables.tileflags.flags3 = vec![0; 2048];
    }

    fn two_actors() -> State {
        let mut s = State::empty();
        creature_tiles(&mut s);
        s.map.centre = Position { x: 20, y: 20, z: 0 };
        for (a, x) in [(10usize, 20u16), (11, 21)] {
            s.objects.type_frame[a] = 0x17a;
            s.objects.location[a] = Position { x, y: 20, z: 0 };
        }
        link_objects(&mut s);
        s
    }

    /// The object list over the slots placed so far (load state's 0x10ed44 + 0x10edb4), so the
    /// object code's moves and walks find them.
    pub(crate) fn link_objects(s: &mut State) {
        crate::main_loop::test_support::link_placed_objects(s);
    }

    #[test]
    fn actors_block_each_other_party_members_swap() {
        let mut s = two_actors();
        assert!(
            !can_move_to(&mut s, 10, 21, 20),
            "another actor stands there"
        );
        assert!(can_move_to(&mut s, 10, 20, 21));
        s.actors.attrs.status[10] = 0x80;
        s.actors.attrs.status[11] = 0x80;
        assert!(can_move_to(&mut s, 10, 21, 20), "fellow member: swap");
        s.actors.work.swap_ok = 0;
        assert!(
            !can_move_to(&mut s, 10, 21, 20),
            "not while swapping is off"
        );
    }

    /// 0x11b1c0 asked for an object slot (Use boarding a vessel, Drop and Move): the party test
    /// reads past the 256 actor status bytes (the original's A4+0x3d0c + slot); the port reads
    /// "not in the party" there and goes on to the blocking checks.
    #[test]
    fn an_object_slot_mover_is_not_a_party_member() {
        let mut s = two_actors();
        s.objects.type_frame[0x200] = 0x17a;
        s.objects.location[0x200] = Position { x: 22, y: 20, z: 0 };
        link_objects(&mut s);
        assert!(!can_move_to(&mut s, 0x200, 21, 20), "an actor stands there");
        assert!(can_move_to(&mut s, 0x200, 22, 21));
    }

    #[test]
    fn impassable_tiles_block_walkers_and_damage_is_noted() {
        let mut s = two_actors();
        s.tables.tileflags.flags1[0] = 0x08;
        assert!(can_move_to(&mut s, 10, 20, 22));
        assert_eq!(s.actors.work.saw_damage, 1);
        s.actors.work.avoid_damage = 1;
        assert!(!can_move_to(&mut s, 10, 20, 22), "damaging tiles refused");
        s.tables.tileflags.flags1[0] = 0x02;
        s.actors.work.avoid_damage = 0;
        assert!(!can_move_to(&mut s, 10, 20, 22), "impassable");
    }

    #[test]
    fn step_moves_pays_and_turns() {
        let mut s = two_actors();
        s.actors.movement_points[10] = 20;
        assert!(step(&mut s, 10, 4, 1));
        assert_eq!(s.objects.location[10], Position { x: 20, y: 21, z: 0 });
        assert_eq!(s.actors.attrs.movement_flags[10] & 7, 4);
        assert_eq!(s.actors.mp(10), 15, "5 for the tile it left");
        // Type 0x17a is a humanoid: facing south (2) in frame bits 2-3, walk phase 1.
        assert_eq!(s.objects.type_frame[10] >> 10, 2 * 4 + 1);
        assert!(!step(&mut s, 10, 1, 1), "north-east is actor 11");
        assert_eq!(s.actors.mp(10), 10, "a failed step still pays");
    }

    #[test]
    fn outside_window_by_level_and_distance() {
        let mut s = two_actors();
        assert!(!outside_window(&s, 10));
        s.objects.location[10].x = 60;
        assert!(outside_window(&s, 10));
        s.objects.location[10] = Position { x: 20, y: 20, z: 1 };
        assert!(outside_window(&s, 10));
    }
}

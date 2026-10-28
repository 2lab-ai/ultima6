//! One actor step: movement-point costs, the "may stand here" test, doors, facing frames.
//!
//! Tile flags (tileflag, 0x13b140 = flags1): 0x01 water, 0x02 impassable, 0x04 wall, 0x08
//! damaging, high nibble = extra move cost. Creature flags: [`super::GameTables`].

use std::cell::RefCell;

use crate::engine::Io;
use crate::image::A4;
use crate::objects::ObjectId;
use crate::rng::Rng;
use crate::state::State;

use super::party::StepResult;
use super::seams::{self, Found};
use super::timer;

/// 8-way direction deltas, 0 = north, clockwise (read from the image at A4-0x61ce / A4-0x61c6:
/// x 0,1,1,1,0,-1,-1,-1 and y -1,-1,0,1,1,1,0,-1).
pub const DX8: [i16; 8] = [0, 1, 1, 1, 0, -1, -1, -1];
pub const DY8: [i16; 8] = [-1, -1, 0, 1, 1, 1, 0, -1];

/// [replaced] Engine milliseconds of the player's step 0x11d4ee besides its followers' pass (the
/// hiccup and vessel tests, the blocking tests, a swap, the cost, the window rebuilt, the move
/// and the facing, the level test 0x11d1d4): 24 ms, 1.186 PAL frames, the mean of 27 steps
/// traced in the oracle (0.57..2.26): 21 in the castle without the fight (3 runs of 7) and 6 in
/// the start fight, whose first step also waited on the floppy and is left out
/// (tools/oracle/trace.mjs on the CPU clock, the reduction tools/oracle/trace.py). The original
/// has no wait there: that is the 68000's time, which the port, stepping at once, waits on the
/// engine clock instead.
pub const STEP_MS: u16 = 24;
/// [replaced] Engine milliseconds of the followers' pass 0x11c910 the step ends in, when it has
/// members to move ([`super::party::has_followers`]): 41 ms, 2.065 PAL frames, the mean of the
/// castle's 21 passes (0.16..5.27), traced with three followers only; the port pays it flat for
/// one to three. In the fight every member is in its combat mode and the pass took 0.049
/// frames (n = 7): no wait.
pub const FOLLOW_MS: u16 = 41;
/// [replaced] Engine milliseconds of Pass's followers' pass (0x11c910 with D1 = 1, called by
/// the command loop at 0x104c0a) when it has members to move ([`super::party::has_followers`])
/// and moves one or more of them back to their spots: 95 ms, 4.773 PAL frames to the nearest
/// ms, the mean of the 14 such passes traced in the castle without the fight, three followers at
/// each (1.72..5.41: after a step, or the first two of a run of Passes; 6 runs of 7 keys, Pass
/// repeated and Pass after a step, `pace/trace-town-pass-q1`..`q6`; tools/oracle/trace.mjs on the
/// CPU clock, tools/oracle/trace.py "follow, Pass, moved").
pub const PASS_MOVED_MS: u16 = 95;
/// [replaced] The same pass when no member moves (all on their spots): 4 ms, 0.192 PAL frames,
/// the mean of the other 19 traced (0.16..0.48; trace.py "follow, Pass, none moved"). With no
/// member to move at all (in a fight every member is in its combat mode) it waits nothing, as
/// the step's pass with no one to move.
///
/// The unit tests (`pass_waits_the_originals_followers_pass_at_any_host_tick`) pin which of the
/// two a Pass pays, not their values; the values are pinned by trace.py's reduction of the
/// `q1`..`q6` traces recorded in re/notes/oracle.md §E (14 and 19 passes, the means above).
pub const PASS_STILL_MS: u16 = 4;

/// The A4 string offset of the line the player's step prints when an object of type 0x1a0 bars
/// it ([`can_move_to`]).
const BARRED: i32 = -0x73e0;

/// Port-only (spec 08, slice K2): the string the step test passes to `printf` as its format, for
/// the locale layer's catalogue ([`crate::locale::strings`]).
pub(crate) const PRINTED: &[u32] = &[A4.wrapping_add_signed(BARRED)];

/// Port: the walk command's step ([`super::party::player_step`], 0x11d4ee), then the time the
/// original spends in it, on the engine clock: [`STEP_MS`], and [`FOLLOW_MS`] more when the
/// followers' pass has members to move ([`super::party::has_followers`], asked after the
/// step). Only a step past its blocking tests waits (moved, or refused by 0x10c9d2 after the
/// move was paid: both end in the followers' pass and the level test). A blocked step returns
/// before them and waits nothing: traced, it took 0.082 PAL frames (n 12, 0.06..0.37; the
/// castle, tools/oracle/trace.py "party step, blocked") with the leader's points unchanged, and
/// the command's world turn then picked the leader again at once (its setup and one pass, no
/// NPC turn, the view unchanged: the next key read 1 to 2 frames after the key). A windless step was not
/// traced and waits nothing. Nothing is drawn meanwhile: the world turn's refresh draws the view
/// the step left. A line the step queues (the hiccup, the refused move's) plays after this
/// wait, up to [`STEP_MS`] + [`FOLLOW_MS`] later than the original prints it.
///
/// Port (spec 20 Design 4): a step that moved makes the early picture of smooth motion, on a copy
/// of the state ([`crate::gfx::motion::preview`]; nothing with it off or in combat mode).
pub async fn paced_player_step(state: &RefCell<State>, io: &Io, dir: u16) -> StepResult {
    let result = super::party::player_step(&mut state.borrow_mut(), dir);
    if result == StepResult::Moved {
        crate::gfx::motion::preview(&state.borrow(), io);
    }
    if matches!(result, StepResult::Moved | StepResult::Mismatch) {
        let follow = if super::party::has_followers(&state.borrow()) {
            FOLLOW_MS
        } else {
            0
        };
        crate::combat::present::pace(state, io, STEP_MS + follow).await;
    }
    result
}

/// Port: Pass's followers' pass ([`super::party::follow`] strict, 0x11c910 with D1 = 1 from
/// 0x104c0a) behind `leader`, then its time on the engine clock when the pass has members to
/// move ([`super::party::has_followers`], asked after it, as for the step): [`PASS_MOVED_MS`]
/// when a member stands elsewhere after it, [`PASS_STILL_MS`] when none does; nothing when it
/// has none to move (aboard a vessel the pass returns at its first test and no one is counted).
/// Nothing is drawn meanwhile: the world turn's refresh draws the view it left.
pub async fn paced_pass_follow(state: &RefCell<State>, io: &Io, leader: u16) {
    let places = |s: &State| {
        let p = &s.actors.party;
        p.members[..p.count()]
            .iter()
            .map(|&m| s.objects.location[usize::from(m)])
            .collect::<Vec<_>>()
    };
    let before = places(&state.borrow());
    super::party::follow(&mut state.borrow_mut(), leader, true);
    let ms = {
        let s = state.borrow();
        let moved = places(&s) != before;
        super::party::has_followers(&s).then_some(if moved { PASS_MOVED_MS } else { PASS_STILL_MS })
    };
    if let Some(ms) = ms {
        crate::combat::present::pace(state, io, ms).await;
    }
}

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
    let cost = tile_cost(s, p.x, p.y, u16::from(p.z));
    spend_mp(s, actor, cost);
}

/// 0x11ef84's cost of (x, y, z) before [`spend_mp`]: 5, plus the cost nibble of the map tile and
/// of every object tile there that is not impassable. Port: also spec 21's travel search's cost
/// of a step that leaves the cell.
pub(crate) fn tile_cost(s: &State, x: u16, y: u16, z: u16) -> i16 {
    let mut cost: i16 = 5;
    let f = flags1(s, u16::from(seams::tile_at(s, x, y, z)));
    if f & 0x02 == 0 {
        cost += i16::from(f >> 4);
    }
    for Found { id, .. } in seams::objects_at(s, x, y, z) {
        cost += object_cost(s, seams::obj_word(s, id));
    }
    cost
}

/// What one object of type/frame `word` adds to 0x11ef84's cost of its cell: the cost nibble of
/// its tile, unless the tile is impassable.
pub(crate) fn object_cost(s: &State, word: u16) -> i16 {
    let f = flags1(s, word_tile(s, word));
    if f & 0x02 == 0 { i16::from(f >> 4) } else { 0 }
}

/// Original: 0x11b92a (actors: is the actor outside the map-window cache)
///
/// True when the actor is on another level than the party or outside the window's cache
/// ([`crate::geom::WINDOW`] tiles a side, 40 in Original).
pub fn outside_window(s: &State, actor: u16) -> bool {
    let p = s.objects.location[usize::from(actor)];
    if u16::from(p.z) != party_z(s) {
        return true;
    }
    let (x0, y0, mask) = seams::window_origin(s);
    let dx = (p.x.wrapping_sub(x0) & mask) as i16;
    let dy = (p.y.wrapping_sub(y0) & mask) as i16;
    let last = crate::geom::WINDOW as i16 - 1;
    dx > last || dy > last
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
    let leader = super::party::leader_actor(s);
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
                seams::print(s, BARRED, &[]);
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
/// Only creatures with flag 0x4000 outside the party. The objects at the actor's location are
/// walked on the location cursor (0x10ad1a at asm 0x11dc20, 0x10aaca at 0x11dcb0): a door
/// (0x129..0x12c) gets frame `4 × closed + (frame & 3)` and the view flags are recomputed; a
/// portcullis (0x116 / 0x118) is operated through 0x1301e2
/// ([`crate::objects::toggle_passthrough`]) with the text output (A4-0x7a4c, `ui.text.on`) off
/// around it (asm 0x11dca0..0x11dcae), and the walk goes on from the cursor as 0x1301e2 left it.
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
    let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, p.x, p.y, u16::from(p.z));
    while let Some(id) = ObjectId::from_slot(o) {
        let t = obj_type(s, id);
        if (0x129..=0x12c).contains(&t) {
            let f = obj_frame(s, id);
            set_frame(s, id, (closed << 2).wrapping_add(f & 3));
            seams::refresh_view_flags(s);
            return;
        }
        if t == 0x116 || t == 0x118 {
            s.ui.text.on = false;
            crate::objects::toggle_passthrough(s, id.slot());
            s.ui.text.on = true;
        }
        o = crate::objects::next_at(&mut s.objects, &s.tables);
    }
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
        let z = party_z(s);
        for k in 0..4u16 {
            let part = dragon_part(s, p.x, p.y, z, k * 2, f);
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

/// [objects] 0x10b922: the part of the four-tile creature (0x19b) one tile from (x, y, z) in
/// direction `part` (0, 2, 4, 6) whose frame is the one the head's facing `frame` gives that
/// side (A4-0x6a70), -1 if none. [`face`] re-frames it for the new facing.
fn dragon_part(s: &mut State, x: u16, y: u16, z: u16, part: u16, frame: u16) -> i16 {
    crate::objects::find_segment_at(s, x, y, z, usize::from(part), frame)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use u6_formats::position::Position;

    use super::*;
    use crate::actors::{GameTables, tail};
    use crate::engine::Engine;
    use crate::files::Files;

    /// Leader 1 (worktype 2) with members 2 and 3 following it (worktype 1) in a column going
    /// north at (20, 20..22), ranks one and two tiles behind; NPC 9 at (20, 18).
    fn column() -> State {
        let mut s = State::empty();
        creature_tiles(&mut s);
        s.actors.set_tail(tail::SOLO_MEMBER, 0xff);
        s.actors.party.members[..3].copy_from_slice(&[1, 2, 3]);
        s.actors.party.size = 3;
        s.map.centre = Position { x: 20, y: 20, z: 0 };
        for (i, a) in [1usize, 2, 3].into_iter().enumerate() {
            s.objects.type_frame[a] = 0x17a;
            s.objects.location[a] = Position {
                x: 20,
                y: 20 + i as u16,
                z: 0,
            };
            s.actors.attrs.status[a] = 0x80;
            s.actors.worktype[a] = if i == 0 { 2 } else { 1 };
            s.actors.movement_points[a] = 30;
        }
        s.objects.type_frame[9] = 0x17a;
        s.objects.location[9] = Position { x: 20, y: 18, z: 0 };
        let mut formation = vec![0; 28];
        formation[12] = 1;
        formation[13] = 2;
        s.actors.game = GameTables {
            formation,
            ..GameTables::default()
        };
        link_objects(&mut s);
        s
    }

    /// The walk command (0x80, north) through the command loop's dispatch on `s` at host ticks
    /// of `tick` ms with `zeros` polls of no time after each: the engine time it took and the
    /// state after it.
    fn walk_ms(s: State, tick: u32, zeros: usize) -> (u64, State) {
        command_ms(s, 0x80, tick, zeros)
    }

    /// Command `code` through the command loop's dispatch on `s`, as [`walk_ms`].
    fn command_ms(s: State, code: u16, tick: u32, zeros: usize) -> (u64, State) {
        let done = Rc::new(Cell::new(None));
        let out = Rc::clone(&done);
        let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
            let t0 = io.now_ms();
            crate::main_loop::commands::dispatch(&st, &io, code).await;
            out.set(Some(io.now_ms() - t0));
        });
        for _ in 0..10_000 {
            if done.get().is_some() {
                break;
            }
            e.tick(&[], tick);
            for _ in 0..zeros {
                e.tick(&[], 0);
            }
        }
        let ms = done.get().expect("the step ends");
        let after = e.state().clone();
        (ms, after)
    }

    #[test]
    fn a_step_waits_the_originals_time_at_any_host_tick() {
        // Each case: the setup, the branch its step takes (checked on a copy), what it costs
        // (counted here: the step, its followers' pass when someone follows) and where the
        // leader ends.
        let follow_on: fn(&mut State) = |_| {};
        let in_combat: fn(&mut State) = members_in_combat;
        let asleep: fn(&mut State) = members_asleep;
        let blocked: fn(&mut State) = npc_ahead;
        let unlinked: fn(&mut State) = |s| {
            // The leader linked into the list at a tile before its own, so the move's walk of
            // the list from its tile finds no predecessor and the move is refused after the
            // step was paid.
            s.objects.location[1].y = 10;
            link_objects(s);
            s.objects.location[1].y = 20;
        };
        let calm_balloon: fn(&mut State) = |s| {
            s.objects.type_frame[1] = 0x1a7;
            s.actors.wind = 0xff;
        };
        let cases = [
            (
                "two followers",
                follow_on,
                StepResult::Moved,
                STEP_MS + FOLLOW_MS,
                (20, 19),
            ),
            (
                "no one follows",
                in_combat,
                StepResult::Moved,
                STEP_MS,
                (20, 19),
            ),
            (
                "the followers asleep",
                asleep,
                StepResult::Moved,
                STEP_MS,
                (20, 19),
            ),
            (
                "the move refused",
                unlinked,
                StepResult::Mismatch,
                STEP_MS + FOLLOW_MS,
                (20, 20),
            ),
            ("blocked", blocked, StepResult::Blocked, 0, (20, 20)),
            ("no wind", calm_balloon, StepResult::NoWind, 0, (20, 20)),
        ];
        for (name, setup, branch, want, at) in cases {
            let mut s = column();
            setup(&mut s);
            assert_eq!(
                super::super::party::player_step(&mut s.clone(), 0),
                branch,
                "{name}: the branch"
            );
            let want = u64::from(want);
            for (tick, zeros) in [(16, 0), (7, 0), (16, 3)] {
                let (ms, after) = walk_ms(s.clone(), tick, zeros);
                let p = after.objects.location[1];
                assert_eq!((p.x, p.y), at, "{name}: where the leader ends");
                assert!(
                    ms >= want && ms <= want + u64::from(tick),
                    "{name}: {ms} ms at {tick} ms ticks ({zeros} zero polls), want {want}"
                );
            }
        }
    }

    /// The members of [`column`] in a combat worktype: a followers' pass has no one to move.
    fn members_in_combat(s: &mut State) {
        s.actors.worktype[2] = 3;
        s.actors.worktype[3] = 3;
    }

    /// Both followers of [`column`] asleep (status 0x04): a followers' pass passes them over.
    fn members_asleep(s: &mut State) {
        s.actors.attrs.status[2] |= 0x04;
        s.actors.attrs.status[3] |= 0x04;
    }

    /// NPC 9 of [`column`] moved to (20, 19), the tile north of the leader.
    fn npc_ahead(s: &mut State) {
        s.objects.location[9].y = 19;
        link_objects(s);
    }

    #[test]
    fn pass_waits_the_originals_followers_pass_at_any_host_tick() {
        // Pass (the space key's code) on the column: the leader's points spent, then its
        // followers' strict pass (0x11c910 with D1 = 1). Each case: the setup, what it costs
        // (counted here: the pass's time when it has members to move, by whether one moves;
        // none when no one is eligible), whether a member moved, and where member 3 ends.
        let in_line: fn(&mut State) = |_| {};
        let off_spot: fn(&mut State) = |s| {
            // Member 3 beside member 2 but a tile east of its spot (20, 22): only a strict
            // pass walks it back.
            s.objects.location[3].x = 21;
            link_objects(s);
        };
        let cases = [
            (
                "two followers in line",
                in_line,
                PASS_STILL_MS,
                false,
                (20, 22),
            ),
            (
                "a follower off its spot",
                off_spot,
                PASS_MOVED_MS,
                true,
                (20, 22),
            ),
            ("no one follows", members_in_combat, 0, false, (20, 22)),
            ("the followers asleep", members_asleep, 0, false, (20, 22)),
        ];
        for (name, setup, want, moves, at) in cases {
            let mut s = column();
            setup(&mut s);
            let want = u64::from(want);
            for (tick, zeros) in [(16, 0), (7, 0), (16, 3)] {
                let (ms, after) = command_ms(s.clone(), 0x20, tick, zeros);
                assert_eq!(after.actors.mp(1), 0, "{name}: the leader's points spent");
                let moved = [2, 3]
                    .iter()
                    .any(|&m| after.objects.location[m] != s.objects.location[m]);
                assert_eq!(moved, moves, "{name}: a member moved");
                let p = after.objects.location[3];
                assert_eq!((p.x, p.y), at, "{name}: where member 3 ends");
                assert!(
                    ms >= want && ms <= want + u64::from(tick),
                    "{name}: {ms} ms at {tick} ms ticks ({zeros} zero polls), want {want}"
                );
            }
        }
    }

    /// The command loop on `s` (with the test text layer and the up arrow on its raw key) fed
    /// the up arrow at host ticks of `tick` ms with `zeros` polls of no time after each: the
    /// engine time from the key taken (the loop's reading flag cleared) to the next key read
    /// (set again), and the state then.
    fn up_to_prompt_ms(mut s: State, tick: u32, zeros: usize) -> (u64, State) {
        use crate::engine::{InputEvent, Key};
        s.ui = crate::main_loop::test_support::test_ui();
        s.ui.tables.keymap[0x4c] = [0x148; 4];
        let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
            crate::main_loop::commands::command_loop(&st, &io).await;
        });
        let run = |e: &mut Engine, input: &[InputEvent]| {
            e.tick(input, tick);
            for _ in 0..zeros {
                e.tick(&[], 0);
            }
        };
        run(&mut e, &[]);
        assert_eq!(e.state().main.reading, 1, "the loop waits for a key");
        run(&mut e, &[InputEvent::Key(Key::Up)]);
        let mut taken = None;
        for _ in 0..1000 {
            let reading = e.state().main.reading;
            match (taken, reading) {
                (None, 0) => taken = Some(e.io().now_ms()),
                (Some(t0), 1) => return (e.io().now_ms() - t0, e.state().clone()),
                _ => {}
            }
            run(&mut e, &[]);
        }
        panic!("no next key read (key taken: {taken:?})");
    }

    /// A step into an occupied tile (0x11d4ee's blocked branch, 0x11d800..0x11d856: the centre
    /// back on the leader, no points spent, no message, no followers' pass) and its turn
    /// (0x1048dc, the end of turn 0x104d98: the world turn 0x1213de, which picks the leader
    /// again at once). The oracle (tools/oracle/trace.mjs, the castle without the fight, 12
    /// steps north into a tile that is not enterable): the leader's points unchanged, a world
    /// turn of one pick and no NPC turn (1.13..1.72 frames), the next key read 1 to 2 PAL
    /// frames after the key (mean 1.667, by the frame counter). The port: the world turn's setup
    /// and one pass (no step wait, no refresh: the view did not change) and nothing else.
    #[test]
    fn a_blocked_step_takes_one_pick_and_keeps_the_leaders_points() {
        // The traced key-to-prompt bound, in engine ms (20 a PAL frame).
        let (lo, hi) = (20, 40);
        let want = u64::from(super::super::turn::SETUP_MS + super::super::turn::PICK_MS);
        assert!((lo..=hi).contains(&want), "one pick within 1..2 frames");
        let mut s = column();
        npc_ahead(&mut s);
        for (tick, zeros) in [(16, 0), (7, 0), (16, 3)] {
            let (ms, after) = up_to_prompt_ms(s.clone(), tick, zeros);
            let p = after.objects.location[1];
            assert_eq!((p.x, p.y), (20, 20), "the leader stays");
            assert_eq!(after.actors.mp(1), 30, "the leader's points unchanged");
            for m in [2, 3] {
                assert_eq!(after.actors.mp(m), 30, "member {m} did not follow");
            }
            assert!(
                ms >= want && ms <= want + u64::from(tick),
                "{ms} ms at {tick} ms ticks ({zeros} zero polls), want {want}"
            );
        }
    }

    #[test]
    fn a_swallowed_follower_counts_by_what_the_pass_left_it() {
        // Member 2 swallowed (movement flag 0x10, 100 hit points), member 3 in a combat
        // worktype: 2 is the pass's only follower. The pass runs twice (0x11c910) and rolls the
        // swallowed test 0x11c6c2 for 2 in each while it is still held: random(1, 30) against
        // its strength, then, still held, random(1, 15) for the hurt. The draws are counted
        // here on a copy of the starting generator.
        let swallowed = |strength: u8| {
            let mut s = column();
            s.actors.worktype[3] = 3;
            s.actors.attrs.movement_flags[2] |= 0x10;
            s.actors.attrs.hp[2] = 100;
            s.actors.attrs.strength[2] = strength;
            s
        };
        // Strength 1: no roll of 1..30 is below it, so 2 stays swallowed through both passes
        // (two draws in each) and does not count afterwards: the step alone.
        let held = swallowed(1);
        let mut held_rng = held.rng;
        for _ in 0..2 {
            held_rng.range(1, 30);
            held_rng.range(1, 15);
        }
        // Strength 31: the first roll frees it (one draw), the second pass does not roll, and
        // it counts afterwards: the step and its followers' pass.
        let freed = swallowed(31);
        let mut freed_rng = freed.rng;
        freed_rng.range(1, 30);
        let cases = [
            ("stays swallowed", held, held_rng, 0x10, STEP_MS),
            ("freed", freed, freed_rng, 0, STEP_MS + FOLLOW_MS),
        ];
        for (name, s, rng, flag, want) in cases {
            let want = u64::from(want);
            for (tick, zeros) in [(16, 0), (7, 0), (16, 3)] {
                let (ms, after) = walk_ms(s.clone(), tick, zeros);
                assert_eq!(after.rng, rng, "{name}: the draws");
                let held = after.actors.attrs.movement_flags[2] & 0x10;
                assert_eq!(held, flag, "{name}: swallowed after the step");
                let p = after.objects.location[1];
                assert_eq!((p.x, p.y), (20, 19), "{name}: the leader moved");
                assert!(
                    ms >= want && ms <= want + u64::from(tick),
                    "{name}: {ms} ms at {tick} ms ticks ({zeros} zero polls), want {want}"
                );
            }
        }
    }

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
        // The window's last column, then the one past it.
        let [ox, _] = s.map.window_origin;
        let last = ox.wrapping_add(crate::geom::WINDOW as u16 - 1) & 0x3ff;
        s.objects.location[10].x = last;
        assert!(!outside_window(&s, 10), "the last column");
        s.objects.location[10].x = (last + 1) & 0x3ff;
        assert!(outside_window(&s, 10));
        s.objects.location[10] = Position { x: 20, y: 20, z: 1 };
        assert!(outside_window(&s, 10));
    }

    /// The passthrough of the portcullis tests.
    const PORTCULLIS: usize = 0x200;

    /// NPC 10 (type 0x17a, a creature with flag 0x4000, outside the party) at (20, 20), a
    /// passthrough 0x116 with `frame` there; `more` placed before the list is linked.
    fn portcullis_under_npc(frame: u16, more: fn(&mut State)) -> State {
        let mut s = State::empty();
        creature_tiles(&mut s);
        s.map.centre = Position { x: 20, y: 20, z: 0 };
        s.objects.defs.monster_types = vec![0x17a, 0];
        s.actors.game = GameTables {
            creature_flags: vec![0x4000],
            ..GameTables::default()
        };
        s.objects.type_frame[10] = 0x17a;
        s.objects.location[10] = Position { x: 20, y: 20, z: 0 };
        s.objects.type_frame[PORTCULLIS] = 0x116 | frame << 10;
        s.objects.location[PORTCULLIS] = Position { x: 20, y: 20, z: 0 };
        more(&mut s);
        link_objects(&mut s);
        s
    }

    #[test]
    fn a_door_opening_npc_operates_a_portcullis_without_a_line() {
        // 0x11db7c (asm 0x11dc94..0x11dcae): a 0x116 at the actor's cell goes through 0x1301e2
        // with A4-0x7a4c cleared around it, so the passthrough's line is not printed.
        let at = |s: &State| s.objects.location[PORTCULLIS];
        let frame = |s: &State| s.objects.type_frame[PORTCULLIS] >> 10;
        let silent = |s: &State, case: &str| {
            assert!(s.main.pending_print.is_empty(), "{case}: no line queued");
            assert!(s.ui.text.on, "{case}: the text on again");
        };
        // Closed: it slides one tile up and opens.
        let mut s = portcullis_under_npc(0, |_| {});
        door_at_actor(&mut s, 10, 1);
        assert_eq!(at(&s), Position { x: 20, y: 19, z: 0 }, "opened");
        assert_eq!(frame(&s), 2);
        silent(&s, "opened");
        // Open: it slides one tile down and closes.
        let mut s = portcullis_under_npc(2, |_| {});
        door_at_actor(&mut s, 10, 0);
        assert_eq!(at(&s), Position { x: 20, y: 21, z: 0 }, "closed");
        assert_eq!(frame(&s), 0);
        silent(&s, "closed");
        // Open with an actor where it closes: refused (the refusal not printed either).
        let mut s = portcullis_under_npc(2, |s| {
            s.objects.type_frame[11] = 0x17a;
            s.objects.location[11] = Position { x: 20, y: 21, z: 0 };
        });
        door_at_actor(&mut s, 10, 0);
        assert_eq!(at(&s), Position { x: 20, y: 20, z: 0 }, "refused");
        assert_eq!(frame(&s), 2);
        silent(&s, "refused");
    }

    #[test]
    fn after_a_refused_portcullis_the_walk_goes_on_where_0x1301e2_looked() {
        // 0x1301e2's refusal began the location walk at the cell the passthrough would close
        // into (0x10ad1a there); 0x11db7c's next 0x10aaca (asm 0x11dcb0) goes on from that
        // walk, so a door after the actor in that cell takes the actor's door frame.
        const DOOR: usize = 0x201;
        let mut s = portcullis_under_npc(2, |s| {
            s.objects.type_frame[11] = 0x17a;
            s.objects.location[11] = Position { x: 20, y: 22, z: 0 };
            s.objects.type_frame[DOOR] = 0x129 | 5 << 10;
            s.objects.location[DOOR] = Position { x: 20, y: 21, z: 0 };
        });
        // A move links the actor in front of the door (0x10c9d2).
        assert_eq!(crate::objects::move_to(&mut s, 11, 20, 21, 0), 11);
        let there = crate::objects::objects_at_view(&s, Position { x: 20, y: 21, z: 0 });
        let order: Vec<usize> = there.iter().map(|(id, _)| id.index()).collect();
        assert_eq!(order, [11, DOOR], "the fixture: the actor first");
        door_at_actor(&mut s, 10, 0);
        assert_eq!(s.objects.location[PORTCULLIS].y, 20, "refused");
        assert_eq!(s.objects.type_frame[DOOR] >> 10, 1, "4 × 0 + (5 & 3)");
    }

    #[test]
    fn after_a_closed_portcullis_the_walk_goes_on_after_the_first_object_where_it_closed() {
        // 0x1301e2 closing begins the location walk at the cell it closes into (0x10ad1a) and
        // the move links it in front of that cell's objects (0x10c9d2): 0x11db7c's next
        // 0x10aaca goes on after the first object there, so a door second in that cell takes
        // the actor's door frame and the first one is passed over.
        const FIRST: usize = 0x201;
        const SECOND: usize = 0x202;
        let mut s = portcullis_under_npc(2, |s| {
            for (d, f) in [(FIRST, 6), (SECOND, 5)] {
                s.objects.type_frame[d] = 0x129 | f << 10;
                s.objects.location[d] = Position { x: 20, y: 21, z: 0 };
            }
        });
        let there = crate::objects::objects_at_view(&s, Position { x: 20, y: 21, z: 0 });
        let order: Vec<usize> = there.iter().map(|(id, _)| id.index()).collect();
        let [first, second] = order[..] else {
            panic!("the fixture: two doors, {order:?}")
        };
        let frame = |s: &State, d: usize| s.objects.type_frame[d] >> 10;
        let (first_was, second_was) = (frame(&s, first), frame(&s, second));
        door_at_actor(&mut s, 10, 0);
        assert_eq!(s.objects.location[PORTCULLIS].y, 21, "closed");
        assert_eq!(frame(&s, first), first_was, "the first door passed over");
        assert_eq!(
            frame(&s, second),
            second_was & 3,
            "the second: 4 × 0 + (frame & 3)"
        );
    }
}

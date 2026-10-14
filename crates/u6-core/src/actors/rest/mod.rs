//! Rest (hunk 14, 0x127278..0x1283b8): the R command 0x127b1a with its camp checks, the meal,
//! the guard, the hours of sleep with ambush rolls, healing 0x1279d4 ([`super::party::heal`],
//! ported with the party code) and the ship's hull repair 0x127ac0 (M2-inventory).
//!
//! Original functions this module owns:
//!
//! - 0x127278 [party] a food type ([`is_food`])
//! - 0x127438 [party] an actor eats one food item it carries ([`eat`])
//! - 0x1274b4 [party] a member eats, from its own pack or a fellow's ([`member_eats`])
//! - 0x1274fe [party] not the wilderness ([`not_wilderness`])
//! - 0x12759e [party] foes near ([`foes_near`])
//! - 0x127688 [party] others near ([`others_near`])
//! - 0x1277e4 [party] no room around the leader ([`no_room`])
//! - 0x12783c [party] an ambush: monsters placed around the camp ([`ambush`])
//! - 0x127ac0 [party] repair the ship's hull ([`repair_hull`])
//! - 0x127b1a [party] the Rest command ([`rest`])

mod command;
#[cfg(test)]
mod tests;

pub use command::rest;

use std::cell::RefCell;

use super::walk::{can_move_to, face, flags1};
use super::{seams as actor_seams, tail};
use crate::engine::Io;
use crate::main_loop::{flush_prints, game_string};
use crate::objects::{Slot, pack_xyz};
use crate::state::State;
use crate::ui::printf::{Arg, printf};

/// Image strings 0x127b1a / 0x127ac0 print (code hunk 14).
const STR_REST: u32 = 0x0012_72cc;
const STR_COMBAT: u32 = 0x0012_72d2;
const STR_NO_REPAIR: u32 = 0x0012_72ea;
const STR_WILDERNESS: u32 = 0x0012_7302;
const STR_FOES: u32 = 0x0012_731c;
const STR_OTHERS: u32 = 0x0012_7338;
const STR_SOLO: u32 = 0x0012_7356;
const STR_ROOM: u32 = 0x0012_736a;
const STR_DISMOUNT: u32 = 0x0012_737e;
const STR_HOURS: u32 = 0x0012_7390;
const STR_ZERO: u32 = 0x0012_73a2;
const STR_KEY: u32 = 0x0012_73a6;
const STR_GUARD: u32 = 0x0012_73aa;
const STR_NONE: u32 = 0x0012_73bc;
const STR_NEWLINE: u32 = 0x0012_73c2;
const STR_MEAL: u32 = 0x0012_73c4;
const STR_TUNE: u32 = 0x0012_73d0;
const STR_FOOD: u32 = 0x0012_73e0;
const STR_NO_FOOD: u32 = 0x0012_73ec;
const STR_STANDS_GUARD: u32 = 0x0012_73fc;
const STR_HEAR: u32 = 0x0012_7422;
const STR_HULL: u32 = 0x0012_72b6;

/// Object types.
const SHIP: u16 = 0x19c;
const HORSE_RIDER: u16 = 0x1af;
const CAMPFIRE: u16 = 0xfd;
const BARD: u16 = 0x182;
const BARD_PLAYING: u16 = 0x188;
/// A type 0x127688 does not count as "others" (0x1ae).
const NOT_OTHERS: u16 = 0x1ae;
/// The monsters of an ambush, by a roll of 0..=3 (0x12783c).
const AMBUSHERS: [u16; 4] = [0x15f, 0x166, 0x156, 0x161];
/// Worktypes: the guard's while the party sleeps, an ambusher's.
const GUARDING: u8 = 0x0c;
const ATTACK: u8 = 0x08;
/// No guard (word 0x100).
const NO_GUARD: Slot = 0x100;
/// Command code of a confirmed target (0x8e).
const TARGET_DONE: u16 = 0x8e;

/// `printf` of the image string at `addr`.
async fn say(state: &RefCell<State>, io: &Io, addr: u32, args: &[Arg<'_>]) {
    let fmt = game_string(io, addr);
    printf(state, io, &fmt, args).await;
}

/// `printf` with member `i`'s roster name as the format (the original passes the name itself).
async fn say_name(state: &RefCell<State>, io: &Io, i: usize) {
    let name = state
        .borrow()
        .actors
        .party
        .names
        .get(i)
        .map_or_else(Vec::new, |n| n.as_bytes().to_vec());
    printf(state, io, &name, &[]).await;
}

fn member(s: &State, i: usize) -> Slot {
    Slot::from(s.actors.party.members[i.min(15)])
}

fn size(s: &State) -> usize {
    usize::from(s.actors.party.size)
}

/// The party is aboard (the leader index points past the members, at the vehicle).
fn aboard(s: &State) -> bool {
    s.actors.party.leader_index == s.actors.party.size
}

fn leader(s: &State) -> Slot {
    member(s, usize::from(s.actors.party.leader_index))
}

/// Byte `i` of the camp place tables (dx, dy at +8, facing at +16).
fn camp(s: &State, i: usize) -> i8 {
    s.inventory.tables.camp_byte(i)
}

/// Original: 0x127278 (party: `ty` is one of the 14 food types at A4-0x6740)
pub fn is_food(s: &State, ty: u16) -> bool {
    s.inventory.tables.food.iter().take(14).any(|&f| f == ty)
}

/// Original: 0x127438 (party: `actor` eats the first food item it carries at any depth; one of
/// that type is taken (0x10d16e))
pub fn eat(s: &mut State, actor: Slot) -> bool {
    let mut n = s.objects.next_of(actor);
    while n >= 0 && s.objects.place(n) != 0 && s.objects.contained_in(n, actor) {
        let ty = s.objects.kind(n);
        if is_food(s, ty) {
            crate::objects::take_from_owner(s, actor, ty, 1);
            return true;
        }
        n = s.objects.next_of(n);
    }
    false
}

/// Original: 0x1274b4 (party: member `i` eats from its own pack, else from the first member
/// (in roster order) who has food)
pub fn member_eats(s: &mut State, i: usize) -> bool {
    let me = member(s, i);
    if eat(s, me) {
        return true;
    }
    (0..size(s)).any(|j| {
        let m = member(s, j);
        eat(s, m)
    })
}

/// Original: 0x1274fe (party: not a place to camp: a tile within 5 of the centre has flags1
/// bit 2 and one of bits 4..7 (low byte of the tile number), or the centre tile has flags1
/// bit 0 while the party is not aboard)
pub fn not_wilderness(s: &State) -> bool {
    let c = s.map.centre;
    let (cx, cy) = (c.x as i16, c.y as i16);
    let z = u16::from(c.z);
    for x in cx - 5..=cx + 5 {
        for y in cy - 5..=cy + 5 {
            let f = flags1(s, u16::from(actor_seams::tile_at(s, x as u16, y as u16, z)));
            if f & 0x04 != 0 && f & 0xf0 != 0 {
                return true;
            }
        }
    }
    let f = flags1(s, u16::from(actor_seams::tile_at(s, c.x, c.y, z)));
    f & 0x01 != 0 && !aboard(s)
}

/// Original: 0x12759e (party: a hostile actor (alignment 0x20 or 0x60) on the party's level in
/// the 39 × 39 tiles from the 8-aligned corner 16 tiles up-left of the centre)
pub fn foes_near(s: &State) -> bool {
    let c = s.map.centre;
    let mask: u16 = if c.z != 0 { 0xff } else { 0x3ff };
    let x0 = c.x.wrapping_sub(16) & 0x3f8;
    let y0 = c.y.wrapping_sub(16) & 0x3f8;
    (0..crate::objects::ACTOR_SLOTS).any(|a| {
        let align = s.actors.attrs.status[a] & 0x60;
        let p = s.objects.location[a];
        s.objects.type_frame[a] != 0
            && (align == 0x20 || align == 0x60)
            && (p.x.wrapping_sub(x0) & mask) < 0x27
            && (p.y.wrapping_sub(y0) & mask) < 0x27
            && p.z == c.z
    })
}

/// Original: 0x127688 (party: an actor not in the party (and not of type 0x1ae) within 5 tiles
/// of the centre on its level)
pub fn others_near(s: &State) -> bool {
    let c = s.map.centre;
    (0..crate::objects::ACTOR_SLOTS).any(|a| {
        let p = s.objects.location[a];
        let tf = s.objects.type_frame[a];
        tf != 0
            && s.actors.attrs.status[a] & 0x80 == 0
            && (p.x as i16 - c.x as i16).abs() <= 5
            && (p.y as i16 - c.y as i16).abs() <= 5
            && p.z == c.z
            && tf & 0x3ff != NOT_OTHERS
    })
}

/// Original: 0x1277e4 (party: the leader may not stand on one of the 3 × 3 tiles around the
/// centre (0x11b1c0))
pub fn no_room(s: &mut State) -> bool {
    let l = leader(s) as u16;
    let c = s.map.centre;
    for dx in 0..3u16 {
        for dy in 0..3u16 {
            let (x, y) = ((c.x + dx).wrapping_sub(1), (c.y + dy).wrapping_sub(1));
            if !can_move_to(s, l, x, y) {
                return true;
            }
        }
    }
    false
}

/// Original: 0x12783c (party: an ambush: 1..=8 monsters of one of four kinds, each placed on
/// a free tile at one of the four sides 6 / 7 tiles from the centre (8 tries), hostile and
/// attacking; true when one was placed)
///
/// The monsters come from objects 0x131646 (a declared stub returning -1, no slot): a failed
/// creation is skipped here, where the original would go on with slot -1.
pub fn ambush(s: &mut State) -> bool {
    let kind = AMBUSHERS[usize::from(s.rng.range(0, 3)).min(3)];
    let n = s.rng.range(1, 8);
    let c = s.map.centre;
    let (cx, cy, z) = (c.x as i16, c.y as i16, u16::from(c.z));
    let mut placed = false;
    for _ in 0..n {
        let m = crate::objects::spawn_monster(s, kind, 0, 0, z);
        if m < 0 {
            continue;
        }
        let mut spot = None;
        for _ in 0..8 {
            let side = s.rng.range(0, 3);
            let off = s.rng.range(0, 10) as i16;
            let (x, y) = match side {
                0 => (cx - 6, cy + off - 6),
                1 => (cx + off - 6, cy - 7),
                2 => (cx + 7, cy + off - 6),
                _ => (cx + off - 6, cy + 6),
            };
            if can_move_to(s, m as u16, x as u16, y as u16) {
                spot = Some((x as u16, y as u16));
                break;
            }
        }
        match spot {
            None => crate::objects::remove(s, m),
            Some((x, y)) => {
                crate::objects::move_to(s, m, x, y, z);
                let st = &mut s.actors.attrs.status[m as usize];
                *st = (*st & 0x9f) | 0x20;
                s.actors.worktype[m as usize] = ATTACK;
                placed = true;
            }
        }
    }
    placed
}

/// Original: 0x127ac0 (party: repair the ship: its hit points + 5 up to 100, five minutes,
/// "Hull Strength: n%")
pub async fn repair_hull(state: &RefCell<State>, io: &Io) {
    let hp = {
        let mut s = state.borrow_mut();
        let ship = leader(&s) as usize;
        let hp = &mut s.actors.attrs.hp[ship];
        *hp = if u16::from(*hp) + 5 < 100 { *hp + 5 } else { 100 };
        super::clock::tick(&mut s, 5);
        s.actors.attrs.hp[ship]
    };
    flush_prints(state, io).await;
    say(state, io, STR_HULL, &[Arg::Word(u16::from(hp))]).await;
}

/// The first camp check that fails, in 0x127b1a's order.
fn refusal(s: &mut State) -> Option<u32> {
    if s.actors.tail(tail::COMBAT_MODE) != 0 {
        return Some(STR_COMBAT);
    }
    if aboard(s) && s.objects.kind(leader(s)) != SHIP {
        return Some(STR_NO_REPAIR);
    }
    if not_wilderness(s) {
        return Some(STR_WILDERNESS);
    }
    if foes_near(s) {
        return Some(STR_FOES);
    }
    if others_near(s) {
        return Some(STR_OTHERS);
    }
    if s.actors.tail(tail::SOLO_MEMBER) as i8 != -1 {
        return Some(STR_SOLO);
    }
    if no_room(s) {
        return Some(STR_ROOM);
    }
    None
}

/// One frame of the camp loops: the actor idle frames due (A4-0x7afa), the view refresh, the
/// overlay step (0x14f12c, the pointer copy, has nothing to do in the port).
fn camp_frame(s: &mut State, io: &Io) {
    s.gfx.actor_anim_due = 1;
    crate::gfx::refresh_view(s, io);
    crate::main_loop::anim_step(s);
}

/// 0x1221da (combat, hunk 13: an actor lies down asleep: off its horse (0x12f8fe), the
/// sleeping body tile of its creature, status bits). Owner: combat. Contract: called by Rest for
/// every member but the guard. Fallback: nothing (the members do not visibly lie down; Rest
/// still gives them their types back from 0x139688 when they wake).
fn lie_down(_s: &mut State, _actor: Slot) {
    crate::todo_stub!("0x1221da lie down asleep (Rest)", ())
}

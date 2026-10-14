//! The hunk-13 helpers the combat code is built from: distances, ranges, weapon damage, armour,
//! experience, the actor at a location, the owner of a multi-tile part, the weapon an NPC picks,
//! what a thief takes, and the karma loss.

use crate::objects::{self, Slot};
use crate::state::State;

/// The 16-bit `abs` of the runtime (0x101ea8) as the callers use it.
fn abs16(v: i16) -> i16 {
    v.wrapping_abs()
}

/// Location of a slot. For -1 the original reads the three bytes before the table; the port
/// reads (0, 0).
fn loc(s: &State, o: i16) -> u6_formats::position::Position {
    usize::try_from(o)
        .ok()
        .and_then(|i| s.objects.location.get(i).copied())
        .unwrap_or_default()
}

fn x_of(s: &State, o: i16) -> i16 {
    loc(s, o).x as i16
}

fn y_of(s: &State, o: i16) -> i16 {
    loc(s, o).y as i16
}

/// Object type (10 bits) of a slot.
pub(crate) fn kind(s: &State, o: i16) -> u16 {
    s.objects
        .type_frame
        .get(o as usize)
        .map_or(0, |w| w & 0x3ff)
}

/// Original: 0x121b08 (combat: distance from `actor` to (x, y): the larger axis distance)
pub fn distance(s: &State, actor: i16, x: i16, y: i16) -> i16 {
    let dx = abs16(x_of(s, actor).wrapping_sub(x));
    let dy = abs16(y_of(s, actor).wrapping_sub(y));
    if dx > dy { dx } else { dy }
}

/// Original: 0x124e0e (combat: distance between (x0, y0) and (x1, y1) from the A4-0x69a0 table:
/// the table entry for |dx| < 8 and |dy| < 8, else 9)
pub fn cell_distance(s: &State, x0: i16, y0: i16, x1: i16, y1: i16) -> i16 {
    let dx = abs16(x0.wrapping_sub(x1));
    let dy = abs16(y0.wrapping_sub(y1));
    if (0..8).contains(&dx) && (0..8).contains(&dy) {
        i16::from(s.combat.tables.distance_at(dx as usize, dy as usize))
    } else {
        9
    }
}

/// Original: 0x12271e (combat: range of an object: the A4-0x6836 table, 1 when not listed)
pub fn range(s: &State, obj: i16) -> i16 {
    let t = kind(s, obj);
    s.combat
        .tables
        .ranges
        .iter()
        .find(|&&(k, _)| k == t)
        .map_or(1, |&(_, r)| i16::from(r))
}

/// Original: 0x122f3e (combat: damage of a weapon or an actor's bare hands)
///
/// An actor whose type is a creature hits with the creature's damage byte (A4-0x7e7e); any
/// other object with the weapon table's value (A4-0x6800 / -0x67b6); -1 when neither.
pub fn damage_of(s: &State, obj: i16) -> i16 {
    let t = kind(s, obj);
    let i = if obj < 0x100 {
        objects::monster_index(&s.objects, t)
    } else {
        -1
    };
    if i >= 0 {
        return i16::from(s.combat.tables.creature_stat(4, i));
    }
    s.combat
        .tables
        .weapons
        .iter()
        .find(|&&(k, _)| k == t)
        .map_or(-1, |&(_, d)| i16::from(d))
}

/// Original: 0x12267a (combat: protection of one piece of armour, 0 when not armour)
pub fn armour_of(s: &State, obj: i16) -> i16 {
    let t = kind(s, obj);
    s.combat
        .tables
        .armour
        .iter()
        .find(|&&(k, _)| k == t)
        .map_or(0, |&(_, a)| i16::from(a))
}

/// Original: 0x1226c8 (combat: an actor's armour: everything readied, 3 less when cursed, 3
/// more when protected)
pub fn armour(s: &mut State, actor: i16) -> i16 {
    let mut total: i16 = 0;
    let mut c = objects::first_child(&mut s.objects, actor);
    while c >= 0 {
        if s.objects.obj_status[c as usize] & 0x18 == 0x18 {
            total = total.wrapping_add(armour_of(s, c));
        }
        c = objects::next_child(&mut s.objects);
    }
    if s.objects.obj_status[actor as usize] & 0x40 != 0 {
        total = total.wrapping_sub(3);
    }
    if s.actors.attrs.status[actor as usize] & 0x01 != 0 {
        total = total.wrapping_add(3);
    }
    total
}

/// Original: 0x122104 (combat: add `n` experience points, at most 9999)
pub fn add_experience(s: &mut State, actor: i16, n: i16) {
    let e = &mut s.actors.attrs.exp[actor as usize];
    if *e < 9999u16.wrapping_sub(n as u16) {
        *e = e.wrapping_add(n as u16);
    } else {
        *e = 9999;
    }
}

/// Original: 0x1220c6 (combat: the level an actor's experience earns: 1, plus one per halving
/// until it is below 100)
pub fn level_for_experience(s: &State, actor: i16) -> i16 {
    let mut e = s.actors.attrs.exp[actor as usize];
    let mut level = 1;
    while e > 99 {
        level += 1;
        e >>= 1;
    }
    level
}

/// Original: 0x122076 (combat: intelligence contest: `random(1, 30)` below
/// int(`b`) / 2 − int(`a`) + 15)
pub fn contest(s: &mut State, a: i16, b: i16) -> bool {
    let r = s.rng.range(1, 30) as i16;
    let half = (crate::actors::leaves::intelligence(s, b as u16) as i16) / 2;
    let ia = crate::actors::leaves::intelligence(s, a as u16) as i16;
    r < half.wrapping_sub(ia).wrapping_add(15)
}

/// Original: 0x1225c0 (combat: the object at (x, y) on the leader's level a hit at it lands on)
///
/// The first object `viewer` can see that is a live actor (below 0x100, status without 0x10);
/// else the first object it can see; else the first one it cannot; -1 on an empty tile.
pub fn actor_at(s: &mut State, viewer: i16, x: i16, y: i16) -> i16 {
    let z = u16::from(s.map.centre.z);
    let mut hidden: Slot = -1;
    let mut o = objects::begin_at(&mut s.objects, &s.tables, x as u16, y as u16, z);
    while o >= 0 && !crate::actors::leaves::can_see(s, viewer as u16, o as u16) {
        if hidden < 0 {
            hidden = o;
        }
        o = objects::next_at(&mut s.objects, &s.tables);
    }
    let first_seen = o;
    let mut cur = o;
    while cur >= 0 {
        if crate::actors::leaves::can_see(s, viewer as u16, cur as u16) {
            if cur < 0x100 && s.actors.attrs.status[cur as usize] & 0x10 == 0 {
                break;
            }
        } else if hidden < 0 {
            hidden = cur;
        }
        cur = objects::next_at(&mut s.objects, &s.tables);
    }
    if cur >= 0 {
        cur
    } else if hidden >= 0 && first_seen < 0 {
        hidden
    } else {
        first_seen
    }
}

/// Original: 0x1240fa (combat: the actor a part belongs to)
///
/// The extra tiles of a big creature (types 0x19b..0x19d and from 0x1aa) name their owner
/// (0x10a5b0); the back half of a horse (0x1a9) names the horse (an actor of type 0x176 one
/// tile toward its facing). Anything else is itself.
pub fn owner(s: &mut State, obj: i16) -> i16 {
    if obj < 0 {
        return -1;
    }
    let t = kind(s, obj);
    if t >= 0x1aa || (0x19b..=0x19d).contains(&t) {
        if obj >= 0x100 {
            return objects::owner_of_part(s, obj);
        }
        return obj;
    }
    if t != 0x1a9 {
        return obj;
    }
    let dir = usize::from(s.objects.type_frame[obj as usize] >> 12);
    let dx = i16::from(s.objects.defs.dir_dx.get(dir).copied().unwrap_or(0));
    let dy = i16::from(s.objects.defs.dir_dy.get(dir).copied().unwrap_or(0));
    let x = x_of(s, obj).wrapping_sub(dx);
    let y = y_of(s, obj).wrapping_sub(dy);
    let z = u16::from(s.map.centre.z);
    let mut o = objects::begin_at(&mut s.objects, &s.tables, x as u16, y as u16, z);
    while o >= 0 {
        if o < 0x100 && kind(s, o) == 0x176 {
            return o;
        }
        o = objects::next_at(&mut s.objects, &s.tables);
    }
    obj
}

/// Original: 0x124e74 (combat: the weapon `actor` attacks `target` with)
///
/// A readied staff or wand (0x39, 0x150) is used 1 time in 4 when no A4-0x7b65 effect runs and
/// `random(0, its charge)` is below 16. Otherwise the readied object with the most damage
/// whose range reaches the target; the actor itself (bare hands) when none does.
pub fn pick_weapon(s: &mut State, actor: i16, target: i16) -> i16 {
    let mut best = actor;
    let mut best_damage: i16 = 0;
    let d = cell_distance(
        s,
        x_of(s, target),
        y_of(s, target),
        x_of(s, actor),
        y_of(s, actor),
    );
    let mut c = objects::first_child(&mut s.objects, actor);
    while c >= 0 {
        let readied = s.objects.obj_status[c as usize] & 0x18 == 0x18;
        let t = kind(s, c);
        if readied
            && (t == 0x39 || t == 0x150)
            && s.rng.range(0, 3) == 0
            && s.actors.timers[13] == 0
        {
            let q = s.objects.qq[c as usize] >> 8;
            if (s.rng.range(0, q) as i16) < 16 {
                return c;
            }
        }
        if readied {
            let dmg = damage_of(s, c);
            if dmg > best_damage && range(s, c) >= d {
                best_damage = dmg;
                best = c;
            }
        }
        c = objects::next_child(&mut s.objects);
    }
    best
}

/// Original: 0x123a7c (combat: an object `actor` carries that is on take list `which`; among
/// several, each later one replaces the pick on a coin flip; -1 when none)
pub fn pick_to_take(s: &mut State, actor: i16, which: usize) -> i16 {
    let mut pick: i16 = -1;
    let mut c = objects::first_child(&mut s.objects, actor);
    while c >= 0 {
        let t = kind(s, c);
        let listed = s.combat.tables.take[which].contains(&t);
        if listed && (pick < 0 || s.rng.range(0, 1) != 0) {
            pick = c;
        }
        c = objects::next_child(&mut s.objects);
    }
    pick
}

/// Original: 0x122fbe (combat: actor `to` becomes a copy of `from` (slime divides): flags,
/// type (unless 0x177), quality, stats, experience, hit points, worktypes; no movement points)
pub fn copy_actor(s: &mut State, from: i16, to: i16) {
    let (f, t) = (from as usize, to as usize);
    let o = &mut s.objects;
    o.obj_status[t] = o.obj_status[f];
    if o.type_frame[f] & 0x3ff != 0x177 {
        o.type_frame[t] = o.type_frame[f];
    }
    o.qq[t] = o.qq[f];
    let a = &mut s.actors.attrs;
    a.status[t] = a.status[f];
    a.strength[t] = a.strength[f];
    a.dexterity[t] = a.dexterity[f];
    a.intelligence[t] = a.intelligence[f];
    a.magic[t] = a.magic[f];
    s.actors.movement_points[t] = 0;
    a.exp[t] = a.exp[f];
    a.hp[t] = a.hp[f];
    s.actors.worktype[t] = s.actors.worktype[f];
    a.combat_mode[t] = a.combat_mode[f];
    a.unknown_10f1[t] = a.unknown_10f1[f];
    a.unknown_18f1[t] = a.unknown_18f1[f];
    a.movement_flags[t] = a.movement_flags[f];
}

/// Original: 0x101eb0 (main: karma drops by `n`, not below 0)
///
/// A leaf of hunk 1 only the combat code calls; ported here.
pub fn lose_karma(s: &mut State, n: i16) {
    let k = &mut s.actors.karma;
    if n < i16::from(*k) {
        *k = k.wrapping_sub(n as u8);
    } else {
        *k = 0;
    }
}

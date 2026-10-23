//! The combat worktypes the NPC turn 0x1200b0 dispatches to: front (3), rear (4), flank (5),
//! berserk (6), retreat / flee (7, 9, 0x13, 0x9a), assault (8, 0x9b), the animal moods (10,
//! 11), the tangle vine (13) and stationary attackers (14), and the target search (0x12500a).
//!
//! Formation words (`actors.work`, set by the actor model's 0x120b64): party centre A4+0x1830,
//! hostiles' centre A4+0x1834, `front` A4+0x1838.. (index 1 = A4+0x183a), hostiles counted
//! A4+0x1844.

use crate::actors::npc::{approach, wander};
use crate::actors::walk;
use crate::geom::{RULE_HALF, WINDOW};
use crate::objects;
use crate::state::State;

use super::attack::{hit, shoot};
use super::leaves::{self, distance, pick_weapon};

/// Location of a slot; the original reads whatever precedes the table for -1, the port 0.
fn pos(s: &State, o: i16) -> (i16, i16) {
    let p = usize::try_from(o)
        .ok()
        .and_then(|i| s.objects.location.get(i).copied())
        .unwrap_or_default();
    (p.x as i16, p.y as i16)
}

fn status(s: &State, a: i16) -> u8 {
    s.actors.attrs.status[a as usize]
}

fn align(s: &State, a: i16) -> u8 {
    status(s, a) & 0x60
}

fn party(s: &State, a: i16) -> bool {
    status(s, a) & 0x80 != 0
}

fn wt(s: &State, a: i16) -> u8 {
    s.actors.worktype[a as usize]
}

fn mp(s: &State, a: i16) -> u8 {
    s.actors.movement_points[a as usize]
}

fn set_mp(s: &mut State, a: i16, v: u8) {
    s.actors.movement_points[a as usize] = v;
}

fn spend(s: &mut State, a: i16, n: i16) {
    walk::spend_mp(s, a as u16, n);
}

fn go(s: &mut State, a: i16, x: i16, y: i16) -> bool {
    approach(s, a as u16, x as u16, y as u16)
}

fn centre(s: &State) -> (i16, i16) {
    let c = s.map.centre;
    (c.x as i16, c.y as i16)
}

fn member0(s: &State) -> i16 {
    i16::from(s.actors.party.members[0])
}

/// Within `d` of the leader on both axes.
fn near_leader(s: &State, x: i16, y: i16, d: i16) -> bool {
    let (cx, cy) = centre(s);
    (cx - d..=cx + d).contains(&x) && (cy - d..=cy + d).contains(&y)
}

/// The A4-0x69a0 distance of (dx, dy) is within `range` (and both under 8).
fn in_reach(s: &State, dx: i16, dy: i16, range: i16) -> bool {
    let (ax, ay) = (dx.wrapping_abs(), dy.wrapping_abs());
    (0..8).contains(&ax)
        && (0..8).contains(&ay)
        && i16::from(s.combat.tables.distance_at(ax as usize, ay as usize)) <= range
}

/// The dispatch of 0x1200b0 over the combat worktypes.
pub fn worktype(s: &mut State, actor: u16, wt: u8) {
    let a = actor as i16;
    match wt {
        3 => front(s, a),
        4 => rear(s, a),
        5 => flank(s, a),
        6 => berserk(s, a),
        8 | 0x9b => assault(s, a),
        10 => tame(s, a),
        11 => wary(s, a),
        13 => tangle(s, a),
        14 => lurk(s, a),
        _ => retreat(s, a),
    }
}

/// Original: 0x12500a (combat: the nearest foe `actor` can see within 8 tiles, -1 when none)
///
/// Foes by alignment: good attack evil-bit (0x20) actors, evil attack good-bit (0x40) ones,
/// chaotic anyone aligned but chaotic, neutrals only good ones and only in worktype 0x9b; a
/// fleeing or guarding actor (7, 9, 0x13, 0x99, 0x9a) takes anyone. A fleeing foe counts only
/// near the party. Nearest by squared distance (16-bit), ties by a coin.
pub fn find_target(s: &mut State, actor: i16) -> i16 {
    let me = align(s, actor);
    let (ax, ay) = pos(s, actor);
    let mut best: i16 = -1;
    let mut best_d: i16 = 0x7fff;
    for t in 0..0x100i16 {
        if s.objects.type_frame[t as usize] == 0 || status(s, t) & 0x10 != 0 || t == actor {
            continue;
        }
        if !crate::actors::leaves::can_see(s, actor as u16, t as u16) {
            continue;
        }
        let w = wt(s, actor);
        if ![9, 0x9a, 0x13, 7, 0x99].contains(&w) {
            let ta = align(s, t);
            if me == 0 && (w != 0x9b || ta != 0x40) {
                continue;
            }
            if me == 0x60 && ta == 0x60 {
                continue;
            }
            if ta == 0 {
                continue;
            }
            if me == 0x40 && status(s, t) & 0x20 == 0 {
                continue;
            }
            if me == 0x20 && status(s, t) & 0x40 == 0 {
                continue;
            }
        }
        let (tx, ty) = pos(s, t);
        if distance(s, actor, tx, ty) > 8 {
            continue;
        }
        if wt(s, t) == 7 && !near_leader(s, tx, ty, RULE_HALF) {
            continue;
        }
        let dx = tx.wrapping_sub(ax);
        let dy = ty.wrapping_sub(ay);
        let d = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
        if d < best_d || (d == best_d && s.rng.range(0, 1) != 0) {
            best_d = d;
            best = t;
        }
    }
    best
}

/// Where to step when the line of fire to `target` is blocked: a quarter turn of the vector to
/// it, one way or the other on a coin.
fn sidestep(s: &mut State, ax: i16, ay: i16, target: i16) -> (i16, i16) {
    let (tx, ty) = pos(s, target);
    if s.rng.range(0, 1) != 0 {
        (ax - (ty - ay), (tx - ax) + ay)
    } else {
        ((ty - ay) + ax, ay - (tx - ax))
    }
}

/// Original: 0x125254 (combat: attack the nearest foe (worktypes 8, 0x9b and the fallback of
/// the others))
///
/// No foe: wait (worktype 3) or wander. A foe within the best weapon's reach is shot at with
/// an invisible flight first: clear, the hit ([`hit`]) and 10 points; blocked, a side step.
/// Out of reach: wait (3) or close in.
pub fn assault(s: &mut State, actor: i16) {
    let t = find_target(s, actor);
    s.ui.input.selection = t;
    let weapon = pick_weapon(s, actor, t);
    if t < 0 {
        if wt(s, actor) == 3 {
            spend(s, actor, 5);
        } else {
            wander(s, actor as u16);
        }
        return;
    }
    let (tx, ty) = pos(s, t);
    let (ax, ay) = pos(s, actor);
    let range = leaves::range(s, weapon);
    if in_reach(s, tx - ax, ty - ay, range) {
        let (mut x, mut y) = (tx, ty);
        if shoot(s, actor, &mut x, &mut y, range, 0) != 0 {
            hit(s, actor, x, y, weapon);
            spend(s, actor, 10);
            return;
        }
        let (x, y) = sidestep(s, ax, ay, t);
        go(s, actor, x, y);
    } else if wt(s, actor) == 3 {
        spend(s, actor, 5);
    } else {
        go(s, actor, tx, ty);
    }
}

/// Original: 0x1254d6 (combat: hold the front line: step to the cell on the line through the
/// formation that is level with the front; 1 when it did nothing else)
///
/// The line runs from the first member (or, for a hostile, the party centre) toward the
/// hostiles' centre (or the first hostile); the actor's place on it is its projection, pushed
/// sideways (to its own side) past occupied cells. Outside the window or already there: 1. A
/// step that fails turns into an attack; a party member off the spot tries once more.
pub fn hold_line(s: &mut State, actor: i16) -> i16 {
    let w = s.actors.work.clone();
    if (w.enemies == 0 || w.front[1] == 0) && !party(s, actor) {
        spend(s, actor, 5);
        return 1;
    }
    let (ax, ay) = pos(s, actor);
    let (bx, by, vx, vy) = if party(s, actor) {
        let (bx, by) = pos(s, member0(s));
        (bx, by, w.enemy_centre.0 - bx, w.enemy_centre.1 - by)
    } else {
        let (bx, by) = w.party_centre;
        let (fx, fy) = pos(s, w.front[1]);
        (bx, by, fx - bx, fy - by)
    };
    let cross = (ax - bx)
        .wrapping_mul(vy)
        .wrapping_sub((ay - by).wrapping_mul(vx));
    let mut side = cross > 0;
    if cross == 0 {
        side = s.rng.range(0, 1) != 0;
    }
    let (mut sx, mut sy) = (if vy > 0 { 1 } else { -1 }, if vx > 0 { -1 } else { 1 });
    if !side {
        sx = -sx;
        sy = -sy;
    }
    let reference = if party(s, actor) {
        member0(s)
    } else {
        w.front[1]
    };
    let (rx, ry) = pos(s, reference);
    let mut proj = (rx - bx)
        .wrapping_mul(vx)
        .wrapping_add((ry - by).wrapping_mul(vy));
    if reference == member0(s) {
        proj = proj.wrapping_add(vx.wrapping_abs().wrapping_add(vy.wrapping_abs()));
    }
    let mut len = vx.wrapping_mul(vx).wrapping_add(vy.wrapping_mul(vy));
    if len == 0 {
        len = 1;
    }
    let mut tx = vx.wrapping_mul(proj).wrapping_div(len) + bx;
    let mut ty = vy.wrapping_mul(proj).wrapping_div(len) + by;
    s.actors.work.swap_ok = 0;
    let (ox, oy, _) = crate::actors::seams::window_origin(s);
    let (ox, oy) = (ox as i16, oy as i16);
    let last = WINDOW as i16 - 1;
    loop {
        let inside = (ox..=ox + last).contains(&tx) && (oy..=oy + last).contains(&ty);
        if !inside || (tx == ax && ty == ay) {
            s.actors.work.swap_ok = 1;
            return 1;
        }
        let taken = objects::actor_at(s, 0, tx as u16, ty as u16) >= 0;
        if !taken {
            break;
        }
        let a = (tx + sx - bx)
            .wrapping_mul(vx)
            .wrapping_add((ty - by).wrapping_mul(vy));
        let b = (tx - bx)
            .wrapping_mul(vx)
            .wrapping_add((ty + sy - by).wrapping_mul(vy));
        if a.wrapping_sub(proj).wrapping_abs() < b.wrapping_sub(proj).wrapping_abs() {
            tx += sx;
        } else {
            ty += sy;
        }
    }
    s.actors.work.swap_ok = 1;
    let saved = mp(s, actor);
    if !go(s, actor, tx, ty) {
        set_mp(s, actor, saved);
        assault(s, actor);
    } else if party(s, actor) && pos(s, actor) != (tx, ty) {
        if s.actors.work.enemies == 0 {
            set_mp(s, actor, saved);
        }
        if !go(s, actor, tx, ty) {
            assault(s, actor);
        }
    }
    0
}

/// Original: 0x125938 (combat: worktype 3, front: hold the line ([`hold_line`]); then a
/// hostile attacks; a party member near the party attacks (the first hostile's mirror steps
/// back toward the party 1 time in 8), one farther away waits)
pub fn front(s: &mut State, actor: i16) {
    if hold_line(s, actor) == 0 {
        return;
    }
    if !party(s, actor) {
        assault(s, actor);
        return;
    }
    let (px, py) = s.actors.work.party_centre;
    if distance(s, actor, px, py) < 8 {
        if actor == s.actors.work.front[1] && s.rng.range(0, 7) == 0 {
            go(s, actor, px, py);
        } else {
            assault(s, actor);
        }
    } else {
        spend(s, actor, 5);
    }
}

/// The fighters of `me`'s side in worktype 3 (front): the smallest projection on (vx, vy)
/// from (bx, by).
fn front_line(s: &State, me: u8, b: (i16, i16), v: (i16, i16)) -> i16 {
    let mut least: i16 = 0x7fff;
    for t in 0..0x100i16 {
        if status(s, t) & 0x10 != 0 || wt(s, t) != 3 || align(s, t) != me {
            continue;
        }
        let (tx, ty) = pos(s, t);
        let p = (tx - b.0)
            .wrapping_mul(v.0)
            .wrapping_add((ty - b.1).wrapping_mul(v.1));
        if p < least {
            least = p;
        }
    }
    least
}

/// Original: 0x1259b0 (combat: worktype 4, rear: stay behind the front line, between the
/// flanks; attack when that fails)
///
/// With no hostiles a party member closes in on the leader, a hostile waits. A member more
/// than 3 from the leader closes in. Level with or ahead of the line (projection toward the
/// foe ≥ the front's least): step back; else beyond a flank (the cross product past the
/// flank's): step back in. A step that fails, or no step needed, turns into an attack.
pub fn rear(s: &mut State, actor: i16) {
    let w = s.actors.work.clone();
    let me_party = party(s, actor);
    if w.enemies == 0 {
        if me_party {
            let (cx, cy) = centre(s);
            go(s, actor, cx, cy);
        } else {
            spend(s, actor, 5);
        }
        return;
    }
    let (f1, f2, b, v) = if me_party {
        let b = w.party_centre;
        (
            w.front[2],
            w.front[3],
            b,
            (w.enemy_centre.0 - b.0, w.enemy_centre.1 - b.1),
        )
    } else {
        let f1 = if w.front[5] < 0 { actor } else { w.front[5] };
        let f2 = if w.front[4] < 0 { actor } else { w.front[4] };
        let b = w.enemy_centre;
        (f1, f2, b, (w.party_centre.0 - b.0, w.party_centre.1 - b.1))
    };
    let least = front_line(s, align(s, actor), b, v);
    let (ax, ay) = pos(s, actor);
    let mine = (ax - b.0)
        .wrapping_mul(v.0)
        .wrapping_add((ay - b.1).wrapping_mul(v.1));
    let saved = mp(s, actor);
    let (cx, cy) = centre(s);
    let cross = |p: (i16, i16)| {
        (p.0 - b.0)
            .wrapping_mul(v.1)
            .wrapping_sub((p.1 - b.1).wrapping_mul(v.0))
    };
    let failed = if me_party && distance(s, actor, cx, cy) > 3 {
        !go(s, actor, cx, cy)
    } else if mine >= least {
        !go(s, actor, ax - v.0, ay - v.1)
    } else {
        let me_x = cross((ax, ay));
        if me_x > cross(pos(s, f1)) {
            !go(s, actor, ax - v.1, ay + v.0)
        } else if me_x < cross(pos(s, f2)) {
            !go(s, actor, ax + v.1, ay - v.0)
        } else {
            true
        }
    };
    if failed {
        set_mp(s, actor, saved);
        assault(s, actor);
    }
}

/// Original: 0x125e26 (combat: worktype 5, flank: go round the side of the foes)
///
/// Without hostiles (or a hostile farther than 7 from the leader) it waits; a member farther
/// than 2 closes in first, at no cost. The side is the actor's own of the line between the
/// centres. The foe most to that side (seen, aligned against it, near the party or the actor;
/// a fleeing one only in view) is shot at when in reach (blocked: a side step); out of reach
/// the actor steps toward it and one tile further round. A failed step attacks.
pub fn flank(s: &mut State, actor: i16) {
    let w = s.actors.work.clone();
    let (cx, cy) = centre(s);
    let engage = w.enemies != 0 && (party(s, actor) || distance(s, actor, cx, cy) <= 7);
    if !engage {
        let saved = mp(s, actor);
        if party(s, actor) && distance(s, actor, cx, cy) > 2 {
            go(s, actor, cx, cy);
        }
        set_mp(s, actor, saved);
        spend(s, actor, 5);
        return;
    }
    let me = align(s, actor);
    let (ax, ay) = pos(s, actor);
    let p = w.party_centre;
    let v = (w.enemy_centre.0 - p.0, w.enemy_centre.1 - p.1);
    let cross = (ax - p.0)
        .wrapping_mul(v.1)
        .wrapping_sub((ay - p.1).wrapping_mul(v.0));
    let mut side = cross > 0;
    if cross == 0 {
        side = s.rng.range(0, 1) != 0;
    }
    let round = if side { (v.1, -v.0) } else { (-v.1, v.0) };
    let mut best: i16 = -1;
    let mut best_v: i16 = i16::MIN;
    for t in 0..0x100i16 {
        if s.objects.type_frame[t as usize] == 0 || status(s, t) & 0x10 != 0 {
            continue;
        }
        if align(s, t) == me || !crate::actors::leaves::can_see(s, actor as u16, t as u16) {
            continue;
        }
        if me == 0x40 && status(s, t) & 0x20 == 0 {
            continue;
        }
        if me == 0x20 && status(s, t) & 0x40 == 0 {
            continue;
        }
        let (tx, ty) = pos(s, t);
        if [7, 9, 0x9a].contains(&wt(s, t)) && !near_leader(s, tx, ty, RULE_HALF) {
            continue;
        }
        let far = (tx - cx).wrapping_abs() > 7 || (ty - cy).wrapping_abs() > 7;
        if far && ((tx - ax).wrapping_abs() > 5 || (ty - ay).wrapping_abs() > 5) {
            continue;
        }
        let mut val = (tx - p.0)
            .wrapping_mul(v.1)
            .wrapping_sub((ty - p.1).wrapping_mul(v.0));
        if !side {
            val = val.wrapping_neg();
        }
        if val > best_v {
            best = t;
            best_v = val;
        }
    }
    if best < 0 {
        wander(s, actor as u16);
        return;
    }
    s.ui.input.selection = best;
    let (mut x, mut y) = pos(s, best);
    let weapon = pick_weapon(s, actor, best);
    let range = leaves::range(s, weapon);
    if in_reach(s, x - ax, y - ay, range) {
        if shoot(s, actor, &mut x, &mut y, range, 0) != 0 {
            hit(s, actor, x, y, weapon);
            spend(s, actor, 10);
            return;
        }
        (x, y) = sidestep(s, ax, ay, best);
    } else {
        x += round.0.signum();
        y += round.1.signum();
    }
    let saved = mp(s, actor);
    if !go(s, actor, x, y) {
        set_mp(s, actor, saved);
        assault(s, actor);
    }
}

/// Original: 0x1263a2 (combat: worktype 6, berserk: go for the strongest foe in sight)
///
/// Strongest = the largest strength + dexterity + intelligence among the live actors within 8
/// that it can see and that are against its alignment; none: attack the nearest. In reach a
/// shot (blocked: a side step), else a step toward it; a failed step attacks. Always 5 more
/// points at the end.
pub fn berserk(s: &mut State, actor: i16) {
    let me = align(s, actor);
    let (ax, ay) = pos(s, actor);
    let mut best: i16 = -1;
    let mut best_sum: i16 = 0;
    for t in 0..0x100i16 {
        if s.objects.type_frame[t as usize] == 0 || status(s, t) & 0x10 != 0 || t == actor {
            continue;
        }
        if distance(s, t, ax, ay) > 8 {
            continue;
        }
        if !crate::actors::leaves::can_see(s, actor as u16, t as u16) {
            continue;
        }
        let ta = align(s, t);
        let against = match me {
            0x40 => ta == 0x20,
            0x20 => ta == 0x40,
            0x60 => ta != 0x60 && ta != 0,
            _ => true,
        };
        if !against {
            continue;
        }
        let at = &s.actors.attrs;
        let i = t as usize;
        let sum =
            i16::from(at.strength[i]) + i16::from(at.dexterity[i]) + i16::from(at.intelligence[i]);
        if sum > best_sum {
            best = t;
            best_sum = sum;
        }
    }
    if best < 0 {
        assault(s, actor);
    } else {
        let (mut x, mut y) = pos(s, best);
        let weapon = pick_weapon(s, actor, best);
        let range = leaves::range(s, weapon);
        let mut step = true;
        if in_reach(s, x - ax, y - ay, range) {
            s.ui.input.selection = best;
            if shoot(s, actor, &mut x, &mut y, range, 0) != 0 {
                hit(s, actor, x, y, weapon);
                spend(s, actor, 10);
                step = false;
            } else {
                (x, y) = sidestep(s, ax, ay, best);
            }
        }
        if step {
            let saved = mp(s, actor);
            if !go(s, actor, x, y) {
                set_mp(s, actor, saved);
                assault(s, actor);
            }
        }
    }
    spend(s, actor, 5);
}

/// Original: 0x126738 (combat: worktypes 7, 9, 0x13, 0x9a: retreat / flee)
///
/// Farther than 7 from the leader: the turn is lost. With no foe it wanders. A foe within 8
/// (worktype 7) or 3 tiles: run the other way; cornered, a fleeing creature (not 9 / 0x9a)
/// fights. Otherwise a good one heads for the party, others wander. Worktype 7 recovers 1 hit
/// point 1 time in 4 and goes back to its combat mode once out of the critical quarter.
pub fn retreat(s: &mut State, actor: i16) {
    let (cx, cy) = centre(s);
    if distance(s, actor, cx, cy) > 7 {
        set_mp(s, actor, 0);
        return;
    }
    let t = find_target(s, actor);
    if t < 0 {
        wander(s, actor as u16);
        return;
    }
    let (tx, ty) = pos(s, t);
    let (ax, ay) = pos(s, actor);
    let (dx, dy) = (tx - ax, ty - ay);
    let lim = if wt(s, actor) == 7 { 8 } else { 3 };
    if dx.wrapping_abs() < lim && dy.wrapping_abs() < lim {
        let saved = mp(s, actor);
        if !go(s, actor, ax - dx, ay - dy) {
            set_mp(s, actor, saved);
            let w = wt(s, actor);
            if w != 0x9a && w != 9 {
                assault(s, actor);
            }
        }
    } else if align(s, actor) == 0x40 && wt(s, actor) != 9 && wt(s, actor) != 0x9a {
        let (px, py) = s.actors.work.party_centre;
        go(s, actor, px, py);
    } else {
        wander(s, actor as u16);
    }
    if wt(s, actor) == 7 && s.rng.range(0, 3) == 0 {
        let i = actor as usize;
        let max = crate::actors::leaves::max_hp(s, actor as u16) as i16;
        if i16::from(s.actors.attrs.hp[i]) < max {
            s.actors.attrs.hp[i] = s.actors.attrs.hp[i].wrapping_add(1);
        }
        if (i16::from(s.actors.attrs.hp[i]) * 4) / max > 0 {
            s.actors.worktype[i] = s.actors.attrs.combat_mode[i];
        }
    }
}

/// A party member within 3 tiles of `actor` on both axes.
fn party_near(s: &State, actor: i16) -> bool {
    let (ax, ay) = pos(s, actor);
    s.actors.party.active().any(|m| {
        let (mx, my) = pos(s, m.0 as i16);
        (mx - ax).wrapping_abs() < 3 && (my - ay).wrapping_abs() < 3
    })
}

/// The animal amble of worktypes 10 and 11: on a coin a walk toward a point 10 tiles away on a
/// random axis and way, else a wait.
fn amble(s: &mut State, actor: i16) {
    let (mut x, mut y) = pos(s, actor);
    if s.rng.range(0, 1) == 0 {
        spend(s, actor, 5);
        return;
    }
    if s.rng.range(0, 1) != 0 {
        x += if s.rng.range(0, 1) != 0 { 10 } else { -10 };
    } else {
        y += if s.rng.range(0, 1) != 0 { 10 } else { -10 };
    }
    go(s, actor, x, y);
}

/// Original: 0x126990 (combat: worktype 10, tame: near the party it follows it on a coin,
/// otherwise ambles)
pub fn tame(s: &mut State, actor: i16) {
    if party_near(s, actor) && s.rng.range(0, 1) == 0 {
        let (px, py) = s.actors.work.party_centre;
        go(s, actor, px, py);
        return;
    }
    amble(s, actor);
}

/// Original: 0x126b28 (combat: worktype 11, wary: near the party it turns to assault (8) 1
/// time in 8, otherwise ambles)
pub fn wary(s: &mut State, actor: i16) {
    if party_near(s, actor) && s.rng.range(0, 7) == 0 {
        s.actors.worktype[actor as usize] = 8;
        assault(s, actor);
        return;
    }
    amble(s, actor);
}

/// Original: 0x126cc0 (combat: worktype 14, lurk: 16 tries at a random tile within 4; the
/// first visible aligned actor of another side found there is hit; else a wait)
pub fn lurk(s: &mut State, actor: i16) {
    let me = align(s, actor);
    if me != 0 {
        for _ in 0..16 {
            let (ax, ay) = pos(s, actor);
            let x = ax + s.rng.range(0, 8) as i16 - 4;
            let y = ay + s.rng.range(0, 8) as i16 - 4;
            let t = leaves::actor_at(s, actor, x, y);
            s.ui.input.selection = t;
            if (0..0x100).contains(&t)
                && crate::actors::leaves::can_see(s, actor as u16, t as u16)
                && align(s, t) != me
                && align(s, t) != 0
            {
                let weapon = pick_weapon(s, actor, t);
                hit(s, actor, x, y, weapon);
                spend(s, actor, 10);
                return;
            }
        }
    }
    spend(s, actor, 5);
}

/// Original: 0x126de8 (combat: the tangle vine grows one step toward `dir`, leaving a vine
/// segment (0x16e, quality = the vine) with the frame joining its old and new facing
/// (A4-0x6752); never straight back. 0 when it could not move)
///
/// The segment's quality byte (the high byte of its word, which 0x1231be reads back as the
/// vine) is the actor (asm 0x126e74 `asl.w #8`). Without a free object slot the vine steps back
/// with its old facing and the result is 0 (asm 0x126f20..0x126f4c).
pub fn grow(s: &mut State, actor: i16, dir: u16) -> i16 {
    let a = actor as usize;
    let dir = dir & 6;
    let (x, y) = pos(s, actor);
    let facing = u16::from(s.actors.attrs.movement_flags[a] & 7);
    if facing ^ dir == 4 {
        return 0;
    }
    if !walk::step(s, actor as u16, dir, 1) {
        return 0;
    }
    let z = u16::from(s.map.centre.z);
    let seg = objects::create_on_map(
        &mut s.objects,
        0x16e,
        0x20,
        (actor as u16) << 8,
        x as u16,
        y as u16,
        z,
    );
    if seg >= 0 {
        let frames = s.combat.tables.vine_frames.clone();
        let f = |i: u16| u16::from(super::tables::at(&frames, usize::from(i)));
        let i = facing * 2 + (dir >> 1);
        let w = &mut s.objects.type_frame[seg as usize];
        *w = (*w & 0x3ff) + (f(i) << 10);
        let r = s.rng.range(0, 3);
        let w = &mut s.objects.type_frame[a];
        *w = (*w & 0x3ff) + (f(dir * 2 + r) << 10);
        return 1;
    }
    objects::move_to(s, actor, x as u16, y as u16, z);
    let mf = &mut s.actors.attrs.movement_flags[a];
    *mf = (*mf & 0xf8) | facing as u8;
    0
}

/// Original: 0x126f58 (combat: worktype 13, the tangle vine: strike a foe next to it on a coin;
/// grow toward one within 4 tiles 1 time in 4 (along the longer axis, else the other); then 1
/// time in 4 grow a random way, else rest (0 points))
pub fn tangle(s: &mut State, actor: i16) {
    let t = find_target(s, actor);
    s.ui.input.selection = t;
    if t >= 0 {
        let (tx, ty) = pos(s, t);
        let (ax, ay) = pos(s, actor);
        let (dx, dy) = (tx - ax, ty - ay);
        if dx.wrapping_abs() < 2 && dy.wrapping_abs() < 2 && s.rng.range(0, 1) != 0 {
            hit(s, actor, tx, ty, actor);
            spend(s, actor, 10);
            return;
        }
        if dx.wrapping_abs() < 5 && dy.wrapping_abs() < 5 && s.rng.range(0, 3) == 0 {
            let h = if dx > 0 { 2 } else { 6 };
            let v = if dy > 0 { 4 } else { 0 };
            let (first, second) = if dx.wrapping_abs() > dy.wrapping_abs() {
                (h, v)
            } else {
                (v, h)
            };
            if grow(s, actor, first) == 0 {
                grow(s, actor, second);
                return;
            }
        }
    }
    if s.rng.range(0, 3) == 0 {
        let mut d = s.rng.range(0, 3) * 2;
        let facing = u16::from(s.actors.attrs.movement_flags[actor as usize] & 7);
        if d == facing {
            d ^= 4;
        }
        grow(s, actor, d);
    } else {
        set_mp(s, actor, 0);
    }
}

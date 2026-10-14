//! The Attack command and one hit: weapon list, cross-hair range, to-hit roll, missiles,
//! ammunition, special weapons, damage.

use std::cell::RefCell;

use crate::engine::Io;
use crate::image::A4;
use crate::objects;
use crate::state::State;
use crate::ui::globals::NO_COMMAND;

use super::damage::apply_damage;
use super::leaves::{self, kind};
use super::present::{self, Text, name, say};

const ATTACK_WORD: u32 = A4 - 0x769b;
const NOT_ABOARD: u32 = A4 - 0x744d;
const WHAT: u32 = A4 - 0x7694;
const SHIP_CANNONS: u32 = A4 - 0x7709;
const BARE_HANDS: u32 = A4 - 0x76f6;
const WITH: u32 = A4 - 0x76e5;
const NO_ARROWS: u32 = A4 - 0x76dc;
const NO_BOLTS: u32 = A4 - 0x76cc;
const PASS: u32 = A4 - 0x76bd;
const NOTHING: u32 = A4 - 0x76b6;
const INTERFERES: u32 = A4 - 0x75f2;
const OUT_OF_RANGE: u32 = A4 - 0x7512;
const NEXT_WEAPON: u32 = A4 - 0x76ac;
const SHATTERED: u32 = A4 - 0x760d;
const WISP: u32 = A4 - 0x7545;
const VANISHED: u32 = A4 - 0x7354;
/// PC-relative strings of hunk 13: "-", " ", "%s.\n", "\n", the charm / paralysis lines and
/// "wand".
const DASH: u32 = 0x0012_1bca;
const SPACE: u32 = 0x0012_1bcc;
const NAMED: u32 = 0x0012_1bc2;
const NEWLINE: u32 = 0x0012_1bc8;
const CHARMED: u32 = 0x0012_1ba0;
const PARALYZED: u32 = 0x0012_1bb0;
const WAND: u32 = 0x0012_1b8e;
/// Cross-hair confirmed (the key read's code 0x8e).
const CONFIRMED: u16 = crate::ui::wait::REPEAT;

fn leader(s: &State) -> i16 {
    let p = &s.actors.party;
    i16::from(p.members.get(usize::from(p.leader_index)).copied().unwrap_or(0))
}

fn pos(s: &State, o: i16) -> (i16, i16) {
    let p = s.objects.location[o as usize];
    (p.x as i16, p.y as i16)
}

fn is_bow(t: u16) -> bool {
    t == 0x29 || t == 0x36
}

fn is_crossbow(t: u16) -> bool {
    t == 0x2a || t == 0x32
}

/// The ammunition a bow (0x37 arrows) or crossbow (0x38 bolts) needs, if any.
fn ammo_type(t: u16) -> Option<i16> {
    if is_bow(t) {
        Some(0x37)
    } else if is_crossbow(t) {
        Some(0x38)
    } else {
        None
    }
}

/// The weapon the attack is on: the listed one, or the attacker itself (bare hands).
fn current_weapon(s: &State, attacker: i16) -> i16 {
    let c = &s.combat;
    if c.weapon_count == 0 {
        attacker
    } else {
        c.weapons[c.weapon_index.clamp(0, 7) as usize]
    }
}

/// The cross-hair range for `weapon`; a cursor further out goes back to the centre.
fn aim(s: &mut State, weapon: i16) {
    let r = leaves::range(s, weapon);
    s.ui.input.range = r as i8;
    let (cx, cy) = s.ui.input.cursor;
    if leaves::cell_distance(s, i16::from(cx), i16::from(cy), 5, 5) > i16::from(r as i8) {
        s.ui.input.cursor = (5, 5);
    }
}

/// Original: 0x1249de (combat: list the weapons the leader has readied (up to 8 that do damage)
/// and print the first: "with %s-", "with bare hands-", or "with ship cannons-" for a ship)
pub fn list_weapons(s: &mut State) {
    let attacker = leader(s);
    s.combat.weapon_index = 0;
    s.combat.weapon_count = 0;
    let mut c = objects::first_child(&mut s.objects, attacker);
    while c >= 0 && s.combat.weapon_count < 8 {
        if s.objects.obj_status[c as usize] & 0x18 == 0x18 && leaves::damage_of(s, c) >= 0 {
            let n = s.combat.weapon_count as usize;
            s.combat.weapons[n] = c;
            s.combat.weapon_count += 1;
        }
        c = objects::next_child(&mut s.objects);
    }
    if s.combat.weapon_count == 0 {
        let at = if kind(s, attacker) == 0x19c {
            SHIP_CANNONS
        } else {
            BARE_HANDS
        };
        say(s, at, vec![]);
    } else {
        let n = name(s, s.combat.weapons[0]);
        say(s, WITH, vec![n]);
    }
}

/// Original: 0x124caa (combat: the Attack command)
///
/// "Attack"; refused aboard ship unless the vessel has cannons (0x19c). Otherwise the command
/// waits for a target (A4-0x7a32 = 1, A4-0x7a2e = 0x81), the weapons are listed and the
/// cross-hair limited to the first one's range. A confirmed target that is not an object in a
/// container or inventory is attacked with each weapon in turn ([`attack_with_weapon`]), the
/// cross-hair asked again between weapons; anything else is "what?". The leader pays 10
/// movement points; the cursor goes off.
///
/// The command loop calls this for code 0x81 and then clears A4-0x7a2e (0x8f).
pub async fn attack(state: &RefCell<State>, io: &Io) {
    let aboard = {
        let mut s = state.borrow_mut();
        say(&mut s, ATTACK_WORD, vec![]);
        let p = &s.actors.party;
        let aboard = p.leader_index == p.size && kind(&s, leader(&s)) != 0x19c;
        if aboard {
            say(&mut s, DASH, vec![]);
            say(&mut s, NOT_ABOARD, vec![]);
        } else {
            say(&mut s, SPACE, vec![]);
            s.ui.input.target_pending = 1;
            s.ui.input.target_command = 0x81;
            list_weapons(&mut s);
            let w = current_weapon(&s, leader(&s));
            aim(&mut s, w);
        }
        aboard
    };
    crate::main_loop::flush_prints(state, io).await;
    if aboard {
        return;
    }
    let mut key = crate::ui::wait::read_key(state, io).await;
    let ok = {
        let mut s = state.borrow_mut();
        let sel = s.ui.input.selection;
        s.main.attack_target = sel;
        key == CONFIRMED && (sel < 0 || s.objects.obj_status[sel as usize] & 0x18 == 0)
    };
    if ok {
        loop {
            attack_with_weapon(&mut state.borrow_mut());
            present::play_effects(state, io).await;
            if state.borrow().ui.input.target_pending != 1 {
                break;
            }
            crate::ui::panels::redraw_panel(state, io).await;
            key = crate::ui::wait::read_key(state, io).await;
            if key != CONFIRMED {
                say(&mut state.borrow_mut(), WHAT, vec![]);
                break;
            }
        }
    } else {
        say(&mut state.borrow_mut(), WHAT, vec![]);
    }
    {
        let mut s = state.borrow_mut();
        let l = leader(&s);
        crate::actors::walk::spend_mp(&mut s, l as u16, 10);
        s.ui.input.target_pending = 0;
        s.ui.input.targeting = 0;
    }
    crate::main_loop::flush_prints(state, io).await;
}

/// The code the command loop leaves in A4-0x7a2e after the Attack command.
pub const AFTER_ATTACK: u16 = NO_COMMAND;

/// Original: 0x124a98 (combat: attack the selected target with the current weapon)
///
/// A bow or crossbow without arrows / bolts says so. The target is echoed ("pass." for the
/// attacker itself, the object's name, "nothing!", or the map tile's name); unless it is the
/// attacker: the object there ([`leaves::actor_at`]) and the hit ([`hit`]): "interferes!" when
/// a foe next to the attacker blocks a missile, "Out of range!" (the attack target is
/// forgotten). Then the next weapon: its range on the cross-hair and "Attack with %s-", or,
/// after the last, the cursor off.
pub fn attack_with_weapon(s: &mut State) {
    let attacker = leader(s);
    let weapon = current_weapon(s, attacker);
    let wt = kind(s, weapon);
    let out_of_ammo = match ammo_type(wt) {
        Some(ammo) => objects::find_child(&mut s.objects, attacker, ammo, -1) < 0,
        None => false,
    };
    if out_of_ammo {
        say(s, if is_bow(wt) { NO_ARROWS } else { NO_BOLTS }, vec![]);
    } else {
        let sel = s.ui.input.selection;
        if sel >= 0 {
            if sel == attacker {
                say(s, PASS, vec![]);
            } else {
                let n = name(s, sel);
                say(s, NAMED, vec![n]);
            }
        } else if s.ui.input.target_void {
            say(s, NOTHING, vec![]);
        } else {
            let (x, y) = s.ui.input.target;
            let z = u16::from(s.map.centre.z);
            let t = crate::actors::seams::tile_at(s, x as u16, y as u16, z);
            let look = crate::objects::look_text(s, u16::from(t)).into_bytes();
            say(s, NAMED, vec![Text::Str(look)]);
        }
        if s.ui.input.selection != attacker {
            say(s, NEWLINE, vec![]);
            s.combat.swallow = true;
            let (x, y) = s.ui.input.target;
            if s.ui.input.selection < 0 {
                s.ui.input.selection = leaves::actor_at(s, attacker, x, y);
            }
            match hit(s, attacker, x, y, weapon) {
                1 => {
                    let foe = i16::from(s.actors.attrs.unknown_18f1[attacker as usize]);
                    let n = name(s, foe);
                    say(s, INTERFERES, vec![n]);
                }
                2 => {
                    say(s, OUT_OF_RANGE, vec![]);
                    s.main.attack_target = -1;
                }
                _ => {}
            }
        }
    }
    s.combat.weapon_index += 1;
    if s.combat.weapon_index < s.combat.weapon_count {
        s.ui.input.targeting = 1;
        let w = s.combat.weapons[s.combat.weapon_index as usize];
        aim(s, w);
        let n = name(s, w);
        say(s, NEXT_WEAPON, vec![n]);
    } else {
        s.ui.input.target_pending = 0;
        s.ui.input.targeting = 0;
    }
}

/// Original: 0x12387e (combat: does `attacker` hit `target` with `weapon`)
///
/// Objects are always hit, and so is anything by the glass sword (0x30). The attacker's
/// strength counts for bare hands of a creature with flag 0x20 and for the weapons 0x04, 0x22,
/// 0x27, 0x2c, its dexterity otherwise; hit when `random(1, 30)` ≥ (target dexterity −
/// that + 30) / 2.
pub fn to_hit(s: &mut State, attacker: i16, target: i16, weapon: i16) -> bool {
    if target >= 0x100 {
        return true;
    }
    let wt = kind(s, weapon);
    if wt == 0x30 {
        return true;
    }
    let use_strength = if weapon == attacker {
        let i = objects::monster_index(&s.objects, wt);
        s.combat.tables.flags(i) & 0x0020 != 0
    } else {
        [0x04, 0x22, 0x27, 0x2c].contains(&wt)
    };
    let a = if use_strength {
        crate::actors::leaves::strength(s, attacker as u16)
    } else {
        crate::actors::leaves::dexterity(s, attacker as u16)
    } as i16;
    // A negative target (a triple-crossbow bolt into an empty cell) reads the byte before the
    // table in the original; the port reads 0.
    let d = if target < 0 {
        0
    } else {
        crate::actors::leaves::dexterity(s, target as u16) as i16
    };
    let r = s.rng.range(1, 30) as i16;
    r >= d.wrapping_sub(a).wrapping_add(30) / 2
}

/// Original: 0x12276a (combat: a foe next to a fighting actor gets in the way of its missile)
///
/// The actor fights (worktype 2..0x0f) and its foe (0x139488) is another live actor that is
/// awake, not paralysed, not swallowed, within one tile, and not frozen by a time stop.
pub fn interferes(s: &State, actor: i16) -> bool {
    let a = actor as usize;
    let wt = s.actors.worktype[a];
    if wt <= 1 || wt >= 0x10 {
        return false;
    }
    let foe = s.actors.attrs.unknown_18f1[a];
    if i16::from(foe) == actor {
        return false;
    }
    let f = usize::from(foe);
    if s.objects.type_frame[f] == 0 || s.actors.attrs.status[f] & 0x16 != 0 {
        return false;
    }
    if s.actors.attrs.movement_flags[f] & 0x10 != 0 {
        return false;
    }
    let (ap, fp) = (s.objects.location[a], s.objects.location[f]);
    let far = |u: u16, v: u16| (u as i16).wrapping_sub(v as i16).wrapping_abs() >= 2;
    if far(ap.x, fp.x) || far(ap.y, fp.y) {
        return false;
    }
    let stopped = s.actors.timers[crate::actors::timer::TIME_STOP] != 0;
    !(stopped && s.actors.attrs.status[f] & 0x80 == 0)
}

/// Original: 0x1228ba (combat: one missile of `tile` from `actor` toward (x, y) within
/// `range`; (x, y) becomes where it stopped; 1 when it got there)
pub fn shoot(s: &mut State, actor: i16, x: &mut i16, y: &mut i16, range: i16, tile: u16) -> i16 {
    s.combat.shooter = actor;
    let (ax, ay) = pos(s, actor);
    let (mut xs, mut ys) = ([*x], [*y]);
    let r = present::fly(s, ax, ay, &mut xs, &mut ys, 1, range, tile, 0);
    *x = xs[0];
    *y = ys[0];
    s.combat.shooter = -1;
    r
}

/// Weapons that always fly as missiles (0x124232's list).
const MISSILE_WEAPONS: [u16; 10] = [0x21, 0x25, 0x29, 0x2a, 0x31, 0x53, 0x32, 0x5b, 0x4f, 0x50];

/// Original: 0x124232 (combat: `attacker` hits at (x, y) with `weapon`; 0 done, 1 a foe next
/// to it interferes, 2 out of range)
///
/// Out of the weapon's range ([`leaves::range`] against the A4-0x69a0 distance): 2. A bow or
/// crossbow without ammunition does nothing. The attacker turns toward the target (a ship turns
/// broadside), attack frames for the dragons 0x170..0x174, the view is refreshed. A missile
/// (any weapon but 0x28 / 0x2f used at a distance, or one of the missile weapons) is stopped by
/// an adjacent foe (1). A staff or wand casts the spell its quality names instead (a wisp
/// 0x175 teleports with quality 0x81). Otherwise the to-hit roll ([`to_hit`]); a missed missile
/// lands on a random neighbouring tile; the missile flies ([`missile`]); the damage is the
/// weapon's ([`leaves::damage_of`]), halved against a creature with flag 0x0080 unless the
/// weapon is 0x30, 0x32 or 0x36; the triple crossbow strikes up to three targets. The glass
/// sword shatters after a hit; a boomerang flies back.
pub fn hit(s: &mut State, attacker: i16, x: i16, y: i16, weapon: i16) -> i16 {
    let (mut x, mut y) = (x, y);
    let (ax, ay) = pos(s, attacker);
    let a = attacker as usize;
    if leaves::cell_distance(s, x, y, ax, ay) > leaves::range(s, weapon) {
        return 2;
    }
    let wt = kind(s, weapon);
    if let Some(ammo) = ammo_type(wt)
        && objects::find_child(&mut s.objects, attacker, ammo, -1) < 0
    {
        return 0;
    }
    let ddx = x.wrapping_sub(ax);
    let ddy = y.wrapping_sub(ay);
    let mut dir: u8 = if ddx.wrapping_abs() > ddy.wrapping_abs() {
        if ddx < 0 { 6 } else { 2 }
    } else if ddy < 0 {
        0
    } else {
        4
    };
    let at = kind(s, attacker);
    let mf = s.actors.attrs.movement_flags[a];
    if at == 0x19c {
        if (dir ^ (mf & 6)) & 2 != 0 {
            dir = mf & 7;
        } else {
            dir ^= 2;
        }
    }
    s.actors.attrs.movement_flags[a] = (mf & 0xf8) | dir;
    crate::actors::walk::face(s, attacker as u16, u16::from(dir));
    if (0x170..=0x174).contains(&at) {
        let w = s.objects.type_frame[a];
        let f = ((w >> 10) & 0xc) + 2;
        s.objects.type_frame[a] = (w & 0x3ff) + (f << 10);
    }
    crate::main_loop::after_turn_refresh(s);
    s.combat.hit = s.ui.input.selection;
    let adjacent = (-1..=1).contains(&ddx) && (-1..=1).contains(&ddy);
    let flies = wt != 0x28 && wt != 0x2f && (!adjacent || MISSILE_WEAPONS.contains(&wt));
    if flies && interferes(s, attacker) {
        return 1;
    }
    let mut bolts: i16 = 0;
    if wt == 0x32 {
        bolts = objects::count_of_kind(&mut s.objects, attacker, 0x38).min(3);
    }
    let mut dmg = leaves::damage_of(s, weapon);
    if dmg < 0 {
        dmg = 1;
    }
    let mut res: i16 = -1;
    let mut target = leaves::owner(s, s.combat.hit);
    if wt == 0x150 || wt == 0x39 {
        staff_or_wand(s, attacker, weapon, wt, target);
        return 0;
    }
    if s.combat.hit >= 0 && target < 0x100 {
        res = i16::from(to_hit(s, attacker, target, weapon));
        if res == 0 && flies {
            res = -1;
            x = x.wrapping_add(s.rng.range(0, 2) as i16 - 1);
            y = y.wrapping_add(s.rng.range(0, 2) as i16 - 1);
            s.combat.hit = leaves::actor_at(s, attacker, x, y);
            let _ = leaves::owner(s, s.combat.hit);
        }
    }
    if flies && missile(s, attacker, weapon, &mut x, &mut y) == 0 {
        res = -1;
    }
    target = leaves::owner(s, s.combat.hit);
    if res == -1 {
        res = 1;
        if s.combat.hit >= 0 && target < 0x100 {
            res = i16::from(to_hit(s, attacker, target, weapon));
        }
    }
    if wt == 0x32 {
        for k in 0..bolts.max(0) as usize {
            if k > 0 {
                let (sx, sy) = (s.combat.spread.0[k], s.combat.spread.1[k]);
                s.combat.hit = leaves::actor_at(s, attacker, sx, sy);
                target = leaves::owner(s, s.combat.hit);
                res = i16::from(to_hit(s, attacker, target, weapon));
            }
            if res != 0 && s.combat.hit >= 0 && target != attacker {
                apply_damage(s, attacker, target, dmg);
            }
        }
        return 0;
    }
    if res != 0 && s.combat.hit >= 0 && target != attacker {
        let i = objects::monster_index(&s.objects, kind(s, target));
        if s.combat.tables.flags(i) & 0x0080 != 0 && ![0x30, 0x32, 0x36].contains(&wt) {
            dmg = (dmg + 1) >> 1;
        }
        if wt != 0x5b {
            apply_damage(s, attacker, target, dmg);
        }
        if wt == 0x30 {
            say(s, SHATTERED, vec![]);
            objects::remove(s, weapon);
            s.actors.work.prompt = 1;
        }
    }
    if wt == 0x31 {
        let (ax, ay) = pos(s, attacker);
        let tile = s.tables.basetile.base.get(0x31).copied().unwrap_or(0);
        let (mut xs, mut ys) = ([x], [y]);
        present::fly(s, ax, ay, &mut xs, &mut ys, 1, 100, tile, present::flight::RETURN);
    }
    0
}

/// 0x124232's staff (0x39) and wand (0x150) branch.
fn staff_or_wand(s: &mut State, attacker: i16, weapon: i16, wt: u16, target: i16) {
    let a = attacker as usize;
    let quality = s.objects.qq[weapon as usize] >> 8;
    if quality == 0x81 && kind(s, attacker) == 0x175 {
        let (ox, oy) = pos(s, attacker);
        let c = s.map.centre;
        objects::move_to(s, attacker, c.x, c.y, u16::from(c.z));
        if super::mode::scatter(s, attacker, false) {
            say(s, WISP, vec![]);
            s.actors.work.prompt = 1;
        } else {
            objects::move_to(s, attacker, ox as u16, oy as u16, u16::from(c.z));
        }
        return;
    }
    if wt == 0x39 {
        let charge = (quality as i16) / 16 + 1;
        let magic = i16::from(s.actors.attrs.magic[a]);
        if magic < charge || i16::from(s.actors.attrs.level[a]) < charge {
            return;
        }
        s.actors.attrs.magic[a] = (magic - charge) as u8;
    }
    let saved = s.actors.party.leader_index;
    s.actors.party.leader_index = 0x10;
    s.ui.input.selection = target;
    if target >= 0 {
        let (tx, ty) = pos(s, target);
        s.ui.input.target = (tx, ty);
    }
    s.ui.text.on = false;
    let outcome = super::seams::cast_from_item(s, quality, attacker);
    s.ui.text.on = true;
    if quality == 0x50 && outcome == 0xfe {
        let n = name(s, s.ui.input.selection);
        say(s, CHARMED, vec![n]);
    } else if quality == 0x45 && outcome == 0 {
        let n = name(s, s.ui.input.selection);
        say(s, PARALYZED, vec![n]);
    }
    s.actors.party.leader_index = saved;
}

/// Original: 0x122938 (combat: the flight of a missile weapon from `attacker` toward (x, y),
/// then what happens to the missile; (x, y) becomes where it came down)
///
/// The tile: an arrow (bows), a bolt (crossbows), 0x188 / 0x189 for 0x4f / 0x50, 0x18f for a
/// ship's cannon, 0x18e for 0x21 or a creature's natural ranged attack, the weapon's own tile
/// when it is thrown. A triple crossbow fires up to three bolts at the target and the two cells
/// beside it (A4-0x6929). Afterwards: one arrow / bolt (three for the triple crossbow) is used
/// up; a thrown weapon (0x24..0x26) farther than one tile is destroyed on an actor, sinks in
/// water / a hole, or lands on the tile it reached; flaming oil (0x53) starts a fire field
/// (0x13d) on dry ground; 0x5b strikes the actor it hit; 0x4f / 0x50 vanish 1 time in 100.
/// Returns 1.
pub fn missile(s: &mut State, attacker: i16, weapon: i16, x: &mut i16, y: &mut i16) -> i16 {
    let wt = kind(s, weapon);
    let range = leaves::range(s, weapon);
    let base = |s: &State, t: u16| s.tables.basetile.base.get(usize::from(t)).copied();
    let tile = if is_bow(wt) {
        base(s, 0x37).unwrap_or(0)
    } else if is_crossbow(wt) {
        base(s, 0x38).unwrap_or(0)
    } else if wt == 0x4f {
        0x188
    } else if wt == 0x50 {
        0x189
    } else if wt == 0x19c {
        0x18f
    } else if wt == 0x21 || (range > 1 && attacker == weapon) {
        0x18e
    } else if attacker != weapon {
        base(s, wt)
            .unwrap_or(0)
            .wrapping_add(s.objects.type_frame[weapon as usize] >> 10)
    } else {
        0
    };
    let (ax, ay) = pos(s, attacker);
    if wt == 0x32 {
        let dx = x.wrapping_sub(ax);
        let dy = y.wrapping_sub(ay);
        let i = (i32::from(dy) + 5) * 11 + i32::from(dx) + 5;
        let b = usize::try_from(i)
            .ok()
            .and_then(|i| s.combat.tables.spread.get(i).copied())
            .unwrap_or(0);
        let d = &s.objects.defs;
        let ddx = |k: u8| i16::from(d.dir_dx.get(usize::from(k)).copied().unwrap_or(0));
        let ddy = |k: u8| i16::from(d.dir_dy.get(usize::from(k)).copied().unwrap_or(0));
        let (hi, lo) = (b >> 4, b & 0xf);
        s.combat.spread = (
            [*x, x.wrapping_add(ddx(hi)), x.wrapping_add(ddx(lo))],
            [*y, y.wrapping_add(ddy(hi)), y.wrapping_add(ddy(lo))],
        );
        let n = objects::count_of_kind(&mut s.objects, attacker, 0x38).min(3);
        s.combat.shooter = attacker;
        let (mut xs, mut ys) = s.combat.spread;
        present::fly(s, ax, ay, &mut xs, &mut ys, n, range, tile, 0);
        s.combat.spread = (xs, ys);
        s.combat.shooter = -1;
        *x = xs[0];
        *y = ys[0];
    } else {
        shoot(s, attacker, x, y, range, tile);
    }
    let mut used = false;
    let z = u16::from(s.map.centre.z);
    match wt {
        0x24..=0x26 => {
            let far = x.wrapping_sub(ax).wrapping_abs() > 1 || y.wrapping_sub(ay).wrapping_abs() > 1;
            if far {
                let mut thrown = weapon;
                let mut c = objects::first_child(&mut s.objects, attacker);
                while c >= 0 {
                    if s.objects.obj_status[c as usize] & 0x18 == 0x10 && kind(s, c) == wt {
                        thrown = c;
                        break;
                    }
                    c = objects::next_child(&mut s.objects);
                }
                used = true;
                let h = s.combat.hit;
                if (0..0x100).contains(&h) {
                    objects::remove(s, thrown);
                } else {
                    let (lx, ly) = s.combat.landing;
                    let t = crate::actors::seams::tile_at(s, lx as u16, ly as u16, z);
                    let f1 = s.tables.tileflags.flags1.get(usize::from(t)).copied();
                    let f1 = f1.unwrap_or(0);
                    let wet = (f1 & 1 != 0 && f1 & 2 != 0) || (0xfc..=0xff).contains(&t);
                    let lost = wet
                        && objects::begin_at(&mut s.objects, &s.tables, lx as u16, ly as u16, z)
                            < 0;
                    if lost {
                        objects::remove(s, thrown);
                    } else {
                        objects::move_to(s, thrown, lx as u16, ly as u16, z);
                        s.objects.obj_status[thrown as usize] |= 1;
                    }
                    objects::build_draw_lists(s);
                }
            }
        }
        0x29 | 0x2a | 0x32 | 0x36 => {
            let ammo = ammo_type(wt).unwrap_or(0x38);
            let a = objects::find_child(&mut s.objects, attacker, ammo, -1);
            let n = if wt == 0x32 { 3 } else { 1 };
            if a >= 0 {
                let q = s.objects.qq[a as usize] & 0xff;
                if q > n {
                    s.objects.qq[a as usize] = (s.objects.qq[a as usize] & 0xff00) | (q - n);
                } else {
                    objects::remove(s, a);
                }
                used = true;
            }
        }
        0x53 => {
            used = true;
            objects::take_from_owner(s, attacker, 0x53, 1);
            let (lx, ly) = s.combat.landing;
            let t = crate::actors::seams::tile_at(s, lx as u16, ly as u16, z);
            let f1 = s.tables.tileflags.flags1.get(usize::from(t)).copied();
            if f1.unwrap_or(0) & 1 == 0 {
                objects::create_on_map(&mut s.objects, 0x13d, 0x20, 0, lx as u16, ly as u16, z);
                objects::build_draw_lists(s);
            }
        }
        0x5b => {
            objects::take_from_owner(s, attacker, 0x5b, 1);
            let h = s.combat.hit;
            if (0..0x100).contains(&h) {
                super::seams::weapon_spell(s, h, attacker);
            }
        }
        0x4f | 0x50 => {
            if s.rng.range(1, 100) == 0x25 {
                objects::remove(s, weapon);
                let wand = with_wand();
                say(s, VANISHED, vec![wand]);
                s.actors.work.prompt = 1;
            }
        }
        _ => {}
    }
    let p = &s.ui.panel;
    if used && p.mode == 0x92 {
        let m = s.actors.party.members.get(p.member.max(0) as usize).copied();
        if m.map(i16::from) == Some(attacker) {
            s.ui.panel.dirty = true;
        }
    }
    1
}

/// "wand" (a PC-relative string of hunk 13), as a message argument.
fn with_wand() -> Text {
    let b = crate::engine::with_current_io(|io| crate::main_loop::game_string(io, WAND));
    Text::Str(b.unwrap_or_default())
}

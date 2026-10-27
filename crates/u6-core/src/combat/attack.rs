//! The Attack command and one hit: weapon list, cross-hair range, to-hit roll, missiles,
//! ammunition, special weapons, damage.

use std::cell::RefCell;

use crate::engine::Io;
use crate::geom::{HALF_H, HALF_W, RULE_HALF};
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
/// PC-relative strings of hunk 13: the dash and the space after the command word, the
/// name-with-full-stop format, the newline, the word for a wand, and the charm and paralysis
/// reports after a spellbook's or spell object's cast (asm 0x124730, 0x124760).
const DASH: u32 = 0x0012_1bca;
const SPACE: u32 = 0x0012_1bcc;
const NAMED: u32 = 0x0012_1bc2;
const NEWLINE: u32 = 0x0012_1bc8;
const WAND: u32 = 0x0012_1b8e;
const CHARMED: u32 = 0x0012_1ba0;
const PARALYZED: u32 = 0x0012_1bb0;

/// Port-only (spec 08, slice K2): the strings Attack passes to `printf` as its format (queued
/// through [`say`]), for the locale layer's catalogue ([`crate::locale::strings`]). The wand's
/// word, which goes in as an argument, is a label ([`SHOWN_AT`]).
pub(crate) const PRINTED: &[u32] = &[
    ATTACK_WORD,
    NOT_ABOARD,
    WHAT,
    SHIP_CANNONS,
    BARE_HANDS,
    WITH,
    NO_ARROWS,
    NO_BOLTS,
    PASS,
    NOTHING,
    INTERFERES,
    OUT_OF_RANGE,
    NEXT_WEAPON,
    SHATTERED,
    WISP,
    VANISHED,
    DASH,
    SPACE,
    NAMED,
    NEWLINE,
    CHARMED,
    PARALYZED,
];

/// Port-only (spec 16 item 1, 2026-10-27): the wand's word the vanished line takes as its `%s`,
/// a label of the locale layer's catalogue at its own address ([`crate::locale::strings`]): read
/// for display ([`with_wand`]), on a wrapping line, so with no cell budget.
pub(crate) const SHOWN_AT: &[u32] = &[WAND];
/// Cross-hair confirmed (the key read's code 0x8e).
const CONFIRMED: u16 = crate::ui::wait::REPEAT;

fn leader(s: &State) -> i16 {
    crate::actors::party::leader_actor(s) as i16
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

/// The cross-hair range for `weapon`; a cursor further out goes back to the centre (the
/// leader's view cell, ([`HALF_W`], [`HALF_H`]); (5, 5) in Original, spec 13 Decision 4).
fn aim(s: &mut State, weapon: i16) {
    let r = leaves::range(s, weapon);
    s.ui.input.range = r as i8;
    let (cx, cy) = s.ui.input.cursor;
    let (hw, hh) = (HALF_W as i16, HALF_H as i16);
    if leaves::cell_distance(s, i16::from(cx), i16::from(cy), hw, hh) > i16::from(r as i8) {
        s.ui.input.cursor = (HALF_W as i8, HALF_H as i8);
    }
}

/// Original: 0x1249de (combat: list the weapons the leader has readied (up to 8 that do damage)
/// and print the first (A4-0x76e5 with its name), bare hands (A4-0x76f6), or a ship's cannons
/// (A4-0x7709))
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
/// The command word (A4-0x769b); refused aboard ship unless the vessel has cannons (0x19c),
/// with the line at A4-0x744d. Otherwise the command waits for a target (A4-0x7a32 = 1,
/// A4-0x7a2e = 0x81), the weapons are listed and the cross-hair limited to the first one's
/// range. A confirmed target that is not an object in a container or inventory is attacked
/// with each weapon in turn ([`attack_with_weapon`]; after a hit with a spellbook or spell object
/// its cast, [`cast_item`], then the rest of 0x124a98, [`next_weapon`]), the cross-hair asked
/// again between weapons; anything else gets the question line A4-0x7694. The leader pays 10
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
            if cast_item(state, io).await {
                next_weapon(&mut state.borrow_mut());
                present::play_effects(state, io).await;
            }
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
/// A bow or crossbow without arrows / bolts says so (A4-0x76dc / A4-0x76cc). The target is
/// echoed: the pass line (A4-0x76bd) for the attacker itself, the object's name, the empty-cell
/// line (A4-0x76b6), or the map tile's name; unless it is the attacker: the object there
/// ([`leaves::actor_at`]) and the hit ([`hit`]): the interference line (A4-0x75f2) when a foe
/// next to the attacker blocks a missile, the range line (A4-0x7512; the attack target is
/// forgotten). Then the next weapon ([`next_weapon`]). Port: a hit with a spellbook or spell
/// object leaves its cast for [`attack`] to run ([`cast_item`]); the next weapon waits for it
/// then, as the original's tail runs after the cast inside the hit.
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
            // Port (spec 08, slice K3): shown, so the locale pack's object name when it has one.
            let look = crate::objects::shown_look(s, u16::from(t)).into_bytes();
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
    if s.combat.item_cast.is_none() {
        next_weapon(s);
    }
}

/// Original: 0x124a98's tail (asm 0x124c20..0x124ca4: the next weapon)
///
/// A4-0x68e4 + 1: that weapon's range on the cross-hair and its name (A4-0x76ac); after the
/// last, the cursor off (A4-0x7a32 = 0, 0x1051a6).
pub(crate) fn next_weapon(s: &mut State) {
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
/// an adjacent foe (1). A spellbook (0x39, [`crate::magic::SPELLBOOK`]) or spell object (0x150)
/// casts the spell its quality names instead (a wisp 0x175 teleports with quality 0x81;
/// [`item_spell`], the cast run by the caller once the hit has returned, [`cast_item`]).
/// Otherwise the to-hit roll ([`to_hit`]); a missed
/// missile lands on a random neighbouring tile; the missile flies ([`missile`]); the damage is the
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
        item_spell(s, attacker, weapon, wt, target);
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
        present::fly(
            s,
            ax,
            ay,
            &mut xs,
            &mut ys,
            1,
            100,
            tile,
            present::flight::RETURN,
        );
    }
    0
}

/// A spellbook's or spell object's cast its hit (0x124232) leaves for the async code that called
/// the hit: the attacker, the item and its quality (the spell), the selection with its location,
/// and what the NPC worktype does after the hit, in order ([`super::npc::After`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemCast {
    pub attacker: i16,
    pub item: i16,
    pub spell: u8,
    pub selection: i16,
    pub target: (i16, i16),
    pub after: Vec<super::npc::After>,
}

/// 0x124232's spellbook (0x39, [`crate::magic::SPELLBOOK`]) and spell object (0x150) branch (asm
/// 0x124560..0x124774).
///
/// A wisp (0x175) whose item has quality 0x81 jumps from the view centre
/// ([`super::mode::scatter`]) or goes back. Otherwise a spellbook needs the circle of its spell
/// (quality / 16 + 1) in both the attacker's magic and level, else nothing happens, and pays it
/// from the magic (asm 0x12461c..0x124676; a spell object pays nothing). The hit's target (the
/// owner of the object hit, A4-0x2f04) and its location become the selection and the quality's
/// spell is cast ([`cast_item`]). Port: that cast awaits and this hit is synchronous (the Attack
/// command and the NPC worktypes call it), so the cast is recorded here, after the charge as the
/// original takes it, and run by the caller once the hit has returned. What the original does
/// after the hit, and so after the cast, waits for it too: Attack's rest of 0x124a98
/// ([`next_weapon`]); in the NPC worktypes the movement points (0x127162 after each `bsr
/// 0x124232`, asm 0x12539a, 0x126246, 0x1265f0, 0x126dc6, 0x127032, and berserk's 5 at
/// 0x12672a; spells such as 0x07 and 0x64 refill them) and the retreat's roll (0x12691a after
/// the assault at 0x1268ce), queued in order ([`super::npc::After`]). A target
/// of -1 (an empty cell, Attack only) has the original read entry -1 of the location table (the
/// 3 bytes before 0x135588); the port's location is then (-1, -1).
fn item_spell(s: &mut State, attacker: i16, weapon: i16, wt: u16, target: i16) {
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
    if wt == crate::magic::SPELLBOOK {
        let charge = (quality as i16) / 16 + 1;
        let magic = i16::from(s.actors.attrs.magic[a]);
        if magic < charge || i16::from(s.actors.attrs.level[a]) < charge {
            return;
        }
        s.actors.attrs.magic[a] = (magic - charge) as u8;
    }
    let at = if target >= 0 {
        pos(s, target)
    } else {
        (-1, -1)
    };
    s.combat.item_cast = Some(ItemCast {
        attacker,
        item: weapon,
        spell: quality as u8,
        selection: target,
        target: at,
        after: Vec::new(),
    });
}

/// Original: 0x124232's cast from a spellbook or spell object (asm 0x12467a..0x12476e), run by
/// the async code that called the hit once it has returned ([`item_spell`] recorded it); true
/// when there was one
///
/// What was queued before is shown first. The leader index (A4-0x7b7a) becomes 16 and party
/// slot 16 (A4+0x47fc) the attacker, so the spell's caster is the attacker
/// ([`crate::actors::party::roster_byte`]); the selection (A4-0x2f04, A4-0x2f02 / -0x2f00) is the
/// recorded target; with the text off (A4-0x7a4c) 0x11a586(quality, 1) runs the spell at it
/// ([`super::seams::cast_from_item`]). The text then goes on (set to 1 at 0x124702, not put
/// back). With the item's quality read again (asm 0x124708..0x124714), a charm (0x50) with the
/// result byte A4+0x181e at 0xfe is reported with the name of the selection as the spell left
/// it (0x121ba0), a paralysis (0x45) with result 0 likewise (0x121bb0); the leader index is put
/// back (0x12476e). Slot 16 keeps the attacker, as in the original. Then what the NPC worktype
/// does after the hit, in order ([`super::npc::after_cast`]). Port: no report for a selection of
/// -1, whose name the original reads from slot -1.
pub(crate) async fn cast_item(state: &RefCell<State>, io: &Io) -> bool {
    let Some(cast) = state.borrow_mut().combat.item_cast.take() else {
        return false;
    };
    present::play_effects(state, io).await;
    let leader = {
        let mut s = state.borrow_mut();
        let leader = s.actors.party.leader_index;
        s.actors.party.leader_index = 0x10;
        s.actors.party.slot16 = cast.attacker as u8;
        s.ui.input.selection = cast.selection;
        s.ui.input.target = cast.target;
        s.ui.text.on = false;
        leader
    };
    super::seams::cast_from_item(state, io, cast.spell).await;
    {
        let mut s = state.borrow_mut();
        s.ui.text.on = true;
        let quality = s.objects.qq[cast.item as usize] >> 8;
        let report = match (quality, s.magic.result) {
            (0x50, 0xfe) => Some(CHARMED),
            (0x45, 0) => Some(PARALYZED),
            _ => None,
        };
        let sel = s.ui.input.selection;
        if let Some(at) = report
            && sel >= 0
        {
            let n = name(&mut s, sel);
            say(&mut s, at, vec![n]);
        }
        s.actors.party.leader_index = leader;
        for &step in &cast.after {
            super::npc::after_cast(&mut s, cast.attacker, step);
        }
    }
    present::play_effects(state, io).await;
    true
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
        // The 11 × 11 table round the attacker (spec 13 Decision 4: the original's box).
        let (half, side) = (i32::from(RULE_HALF), super::tables::SPREAD_SIDE as i32);
        let i = (i32::from(dy) + half) * side + i32::from(dx) + half;
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
            let far =
                x.wrapping_sub(ax).wrapping_abs() > 1 || y.wrapping_sub(ay).wrapping_abs() > 1;
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
        0x4f | 0x50 if s.rng.range(1, 100) == 0x25 => {
            objects::remove(s, weapon);
            let wand = with_wand(s);
            say(s, VANISHED, vec![wand]);
            s.actors.work.prompt = 1;
        }
        _ => {}
    }
    let p = &s.ui.panel;
    if used && p.mode == 0x92 {
        let m = s
            .actors
            .party
            .members
            .get(p.member.max(0) as usize)
            .copied();
        if m.map(i16::from) == Some(attacker) {
            s.ui.panel.dirty = true;
        }
    }
    1
}

/// The word for a wand (a PC-relative string of hunk 13), as a message argument. Port (spec 16
/// item 1, 2026-10-27): read for display, so the locale pack's label of it when it has one
/// ([`SHOWN_AT`], [`crate::locale::shown`]).
fn with_wand(s: &State) -> Text {
    let b = crate::engine::with_current_io(|io| crate::main_loop::game_string(io, WAND));
    Text::Str(crate::locale::shown(s, b.unwrap_or_default()))
}

/// Spec 16 item 1 (2026-10-27): the wand's word in the vanished line ([`with_wand`]) with a
/// Korean pack holding a form for it, the pack's form; without a pack, the English. A synthetic
/// `game` of the test's own bytes.
#[cfg(test)]
mod tests_words {
    use std::collections::BTreeMap;

    use super::*;

    /// The word [`with_wand`] gives, with a pack or none.
    fn wand_word(pack: bool) -> Vec<u8> {
        let parts: [(u32, &[u8]); 1] = [(WAND, b"Zw1")];
        let mut s = State::empty();
        if pack {
            crate::locale::strings::install_test_forms(&mut s, &parts, &[(b"Zw1", "지팡이")]);
        }
        let game = crate::locale::strings::test_game(&parts);
        let files = crate::files::Files::new(BTreeMap::from([("game".to_string(), game)]));
        let out = std::rc::Rc::new(RefCell::new(Vec::new()));
        let got = out.clone();
        let mut e = crate::engine::Engine::with_game(s, files, move |st, _io| async move {
            let s = st.borrow();
            if let Text::Str(w) = with_wand(&s) {
                *got.borrow_mut() = w;
            }
        });
        for _ in 0..40 {
            if e.finished() {
                break;
            }
            e.tick(&[], 16);
        }
        assert!(e.finished());
        out.take()
    }

    #[test]
    fn the_wands_word_is_the_packs_form_and_else_the_english() {
        assert_eq!(wand_word(true), "지팡이".as_bytes(), "the pack's form");
        assert_eq!(wand_word(false), b"Zw1", "the English");
    }
}

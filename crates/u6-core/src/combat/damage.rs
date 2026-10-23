//! A hit landing: the flash, the damage roll against armour, wounds, death and its remains,
//! experience, and what an attacked bystander or a monster's special does.

use crate::engine::with_current_io;
use crate::geom::{HALF_H, HALF_W, VIEW_H, VIEW_W};
use crate::image::A4;
use crate::main_loop::Pending;
use crate::objects;
use crate::state::State;
use crate::ui::WindowId;

use super::leaves::{self, kind};
use super::present::{self, Text, name, say};

/// Data-hunk messages (A4 − offset) and the PC-relative ones of hunk 13.
const GRAZED: u32 = A4 - 0x7723;
const KILLED: u32 = A4 - 0x7716;
const DRAGGED: u32 = A4 - 0x75c3;
const DISSOLVES: u32 = A4 - 0x75a0;
const ROBBED: u32 = A4 - 0x7585;
const POISONED: u32 = A4 - 0x75af;
const CRITICAL: u32 = A4 - 0x7750;
const HEAVILY: u32 = A4 - 0x7745;
const LIGHTLY: u32 = A4 - 0x773d;
const BARELY: u32 = A4 - 0x7735;
const WOUNDED: u32 = A4 - 0x772e;
const SLIME: u32 = A4 - 0x7760;
const VINE: u32 = A4 - 0x755a;
const BROKEN: u32 = A4 - 0x7361;
const YELLS: u32 = A4 - 0x7318;
/// The name-and-space format of 0x12395a and the three-digit number line of 0x1231be's roster.
const NAME_SPACE: u32 = 0x0012_1b9a;
const HP_NUMBER: u32 = 0x0012_1b94;

fn status(s: &State, a: i16) -> u8 {
    s.actors.attrs.status[a as usize]
}

fn member(s: &State, i: usize) -> i16 {
    i16::from(s.actors.party.members.get(i).copied().unwrap_or(0))
}

/// Original: 0x1230ce (combat: flash a hit: tile 0x101 over the object's window cell, shown at
/// once; the actor is marked as hit (movement flag 0x08) and woken up)
///
/// The flash only for an object in the view (spec 13 Decision 5: its view cell, the centre at
/// ([`HALF_W`], [`HALF_H`]), inside [`VIEW_W`] × [`VIEW_H`]; 0..11 both in Original).
pub fn flash(s: &mut State, target: i16) {
    let t = target as usize;
    if s.combat.flash != 0 {
        let c = s.map.centre;
        let p = s.objects.location[t];
        let col = (p.x as i16)
            .wrapping_sub(c.x as i16)
            .wrapping_add(HALF_W as i16);
        let row = (p.y as i16)
            .wrapping_sub(c.y as i16)
            .wrapping_add(HALF_H as i16);
        if (0..VIEW_W as i16).contains(&col) && (0..VIEW_H as i16).contains(&row) {
            objects::build_draw_lists(s);
            super::seams::add_tile(s, 0x101, col, row, 1);
            with_current_io(|io| crate::gfx::render_window(s, io, true));
            present::capture_window(s, present::FLASH_MS);
            objects::build_draw_lists(s);
            s.map.view_dirty = 1;
        }
    }
    if target < 0x100 {
        s.actors.attrs.movement_flags[t] |= 0x08;
        if s.actors.attrs.status[t] & 0x04 != 0 {
            s.actors.attrs.status[t] &= !0x04;
            s.objects.type_frame[t] = s.actors.attrs.base_type_frame[t];
            s.map.view_dirty = 1;
        }
    }
    s.combat.flash = 1;
}

/// Original: 0x12395a (combat: print how badly an actor is hurt by quarters of its maximum hit
/// points: the critical line (A4-0x7750), or one of three degrees (A4-0x7745 / -0x773d /
/// -0x7735) followed by the wounded word (A4-0x772e))
///
/// A critically hurt actor with intelligence 5 or more that is not in worktype 0, 1, 2, 3, 6
/// or 0x0e starts to flee (worktype 7).
pub fn report_wounds(s: &mut State, actor: i16) {
    let a = actor as usize;
    if kind(s, actor) == 0x1a7 || s.actors.attrs.hp[a] == 0 {
        return;
    }
    let quarters = (i16::from(s.actors.attrs.hp[a]) * 4)
        / crate::actors::leaves::max_hp(s, actor as u16) as i16;
    if quarters < 4 {
        let n = name(s, actor);
        say(s, NAME_SPACE, vec![n]);
        s.actors.work.prompt = 1;
    }
    match quarters {
        0 => {
            say(s, CRITICAL, vec![]);
            let int = crate::actors::leaves::intelligence(s, actor as u16);
            let wt = s.actors.worktype[a];
            if int >= 5 && ![6, 0x0e, 2, 3, 1, 0].contains(&wt) {
                s.actors.worktype[a] = 7;
            }
        }
        1 => say(s, HEAVILY, vec![]),
        2 => say(s, LIGHTLY, vec![]),
        3 => say(s, BARELY, vec![]),
        _ => {}
    }
    if (1..4).contains(&quarters) {
        say(s, WOUNDED, vec![]);
    }
}

/// Original: 0x123d86 (combat: `attacker` deals up to `damage` to `target` and the result is
/// reported)
///
/// The roll is `random(1, damage)` (-1 counts as 1, 1 and 0xff are exact) less `random(1,
/// armour)` for an armoured actor; nothing left is a graze (A4-0x7723). Otherwise an adjacent
/// fighting actor turns on its attacker, the hurt goes through [`hurt`] and the attacker gains
/// the experience it returns. For an actor target then: a bystander the player picked may yell
/// for help ([`yell_for_help`]), the death line (A4-0x7716), or the attacker's specials
/// (dragging under, dissolving or robbing an item, poison), then the wound report.
pub fn apply_damage(s: &mut State, attacker: i16, target: i16, damage: i16) {
    if target < 0 {
        return;
    }
    let target_name = name(s, target);
    let a = attacker as usize;
    if s.objects.type_frame[a] == 0x165 {
        s.objects.type_frame[a] = (s.objects.type_frame[a] & 0x3ff) + 0x400;
        s.map.view_dirty = 1;
    }
    let mut dmg = damage;
    if dmg == -1 {
        dmg = 1;
    } else if dmg > 1 && dmg < 0xff {
        dmg = s.rng.range(1, dmg as u16) as i16;
    }
    let armour = if target < 0x100 {
        leaves::armour(s, target)
    } else {
        0
    };
    if armour > 0 && dmg < 0xff {
        dmg = dmg.wrapping_sub(s.rng.range(1, armour as u16) as i16);
    }
    if dmg <= 0 {
        flash(s, target);
        say(s, GRAZED, vec![target_name.clone()]);
        s.actors.work.prompt = 1;
    } else {
        let t = target as usize;
        if target < 0x100 && s.actors.worktype[t] > 1 && s.actors.worktype[t] < 0x10 {
            let (tp, ap) = (s.objects.location[t], s.objects.location[a]);
            let near = |u: u16, v: u16| (u as i16).wrapping_sub(v as i16).wrapping_abs() < 2;
            if near(tp.x, ap.x) && near(tp.y, ap.y) {
                s.actors.attrs.unknown_18f1[t] = attacker as u8;
            }
        }
        let exp = hurt(s, target, dmg);
        leaves::add_experience(s, attacker, exp);
    }
    if target >= 0x100 {
        return;
    }
    let t = target as usize;
    if s.ui.input.selection == target {
        yell_for_help(s, target, attacker, dmg);
    }
    let tk = kind(s, target);
    if s.actors.attrs.hp[t] == 0 && ![0x1a7, 0x19e, 0x19f, 0x19c].contains(&tk) {
        say(s, KILLED, vec![target_name]);
        s.actors.work.prompt = 1;
        let p = &s.ui.panel;
        if p.mode == 0x90 && member(s, p.member.max(0) as usize) == attacker {
            s.ui.panel.dirty = true;
        }
        return;
    }
    let index = objects::monster_index(&s.objects, kind(s, attacker));
    if index == 8 {
        say(s, DRAGGED, vec![target_name.clone()]);
        s.actors.attrs.movement_flags[t] |= 0x10;
        s.actors.work.prompt = 1;
        s.map.view_dirty = 1;
    }
    if index == 0 {
        let o = leaves::pick_to_take(s, target, 0);
        if o >= 0 {
            let (who, what) = (name(s, target), name(s, o));
            say(s, DISSOLVES, vec![who, what]);
            s.actors.work.prompt = 1;
            objects::remove(s, o);
        }
    }
    if index == 0x17 {
        let o = leaves::pick_to_take(s, target, 1);
        if o >= 0 {
            let who = name(s, target);
            say(s, ROBBED, vec![who]);
            s.actors.work.prompt = 1;
            objects::remove(s, o);
        }
    }
    if s.combat.tables.flags(index) & 0x0040 != 0 && s.rng.range(0, 3) == 0 {
        say(s, POISONED, vec![target_name]);
        s.actors.work.prompt = 1;
        s.actors.attrs.status[t] |= 0x08;
        let p = &s.ui.panel;
        if p.mode == 0x91 || (p.mode == 0x90 && member(s, p.member.max(0) as usize) == target) {
            s.ui.panel.dirty = true;
        }
    }
    if dmg > 0 {
        report_wounds(s, target);
    }
}

/// Original: 0x1231be (combat: take `damage` from an actor or an object; returns the
/// experience a kill is worth)
///
/// Actors: flashed; creature 0x1f cannot be hurt (hit points 0xff); a ship or skiff (0x19e,
/// 0x19f) passes the damage to a random party member; damage of at least its hit points kills
/// it: karma −5 for a neutral, hit points 0, dead (status 0x10), experience = a quarter of the
/// creature's six stats for an evil one, visible, laid down ([`lie_down`]), out of the party,
/// its remains made; a ship (0x19c) becomes a skiff with 10 hit points instead. Survivors lose
/// the hit points; a slime (creature 0x30) divides onto a free neighbouring tile. A party
/// member's numbers are redrawn in the panel. Objects: the tangle vine's root (0x16e) kills its
/// vine actor; a breakable object loses `damage` from its quantity byte and breaks (A4-0x7361,
/// a trap inside goes off, a mirror 0x7b takes frame 2 for 10 karma, others vanish).
pub fn hurt(s: &mut State, actor: i16, damage: i16) -> i16 {
    if damage < 0 {
        return 0;
    }
    let a = actor as usize;
    let t = kind(s, actor);
    let mut exp: i16 = 0;
    if t == 0x1a7 {
        return 0;
    }
    // A4-0x7a4c: text output forced on while the hit is reported (restored at the end).
    let saved = s.ui.text.on;
    s.ui.text.on = true;
    if actor < 0x100 {
        flash(s, actor);
        let index = objects::monster_index(&s.objects, t);
        if index == 0x1f {
            s.actors.attrs.hp[a] = 0xff;
        } else if t == 0x19e || t == 0x19f {
            let size = leaves::party_size(s) as u16;
            let mut victim = member(s, 0);
            if size > 1 {
                let i = s.rng.range(0, size - 1);
                victim = member(s, usize::from(i));
            }
            hurt(s, victim, damage);
            report_wounds(s, victim);
        } else if damage >= i16::from(s.actors.attrs.hp[a]) {
            let (e, settle) = kill(s, actor, t);
            exp = e;
            let leader = member(s, usize::from(s.actors.party.leader_index));
            if settle && leader == actor {
                crate::actors::turn::leader_bookkeeping(s);
            }
        } else {
            s.actors.attrs.hp[a] = s.actors.attrs.hp[a].wrapping_sub(damage as u8);
            if s.actors.worktype[a] == 9 {
                s.actors.worktype[a] = 8;
            }
            if index == 0x30 {
                slime_divides(s, actor);
            }
        }
        if status(s, actor) & 0x80 != 0 {
            party_numbers(s, actor);
        }
    } else {
        hurt_object(s, actor, t, damage);
    }
    s.ui.text.on = saved;
    exp
}

/// 0x1231be's death branch: the experience, and whether the leader bookkeeping follows (not
/// after a ship became a skiff).
fn kill(s: &mut State, actor: i16, t: u16) -> (i16, bool) {
    let a = actor as usize;
    if s.main.attack_target == actor {
        s.main.attack_target = -1;
    }
    if t == 0x19c {
        let p = s.objects.location[a];
        objects::move_monster_parts(s, actor, p.x, p.y, true);
        s.objects.type_frame[a] = 0x19f;
        s.actors.attrs.hp[a] = 10;
        s.map.view_dirty = 1;
        return (0, false);
    }
    if s.actors.attrs.hp[a] == 0 {
        return (0, true);
    }
    let mut exp = 0;
    if status(s, actor) & 0x60 == 0 {
        leaves::lose_karma(s, 5);
    }
    s.actors.attrs.hp[a] = 0;
    s.actors.attrs.status[a] |= 0x10;
    if status(s, actor) & 0x20 != 0 {
        let i = objects::monster_index(&s.objects, kind(s, actor));
        if i >= 0 {
            let tb = &s.combat.tables;
            let sum: i16 = (0..6).map(|n| i16::from(tb.creature_stat(n, i))).sum();
            exp = sum >> 2;
        }
    }
    s.objects.obj_status[a] &= !0x02;
    lie_down(s, actor);
    s.actors.worktype[a] = 0;
    if status(s, actor) & 0x80 != 0 && member(s, 0) != actor {
        leave_party(s, actor);
    }
    super::seams::remains(s, actor);
    s.map.view_dirty = 1;
    (exp, true)
}

/// 0x1231be: a dead member leaves the roster (solo index, leader index, names shift up).
fn leave_party(s: &mut State, actor: i16) {
    let size = leaves::party_size(s);
    let Some(k) = (1..=size.min(15)).find(|&k| member(s, k) == actor) else {
        return;
    };
    let solo = s.actors.tail(crate::actors::tail::SOLO_MEMBER) as i8;
    if i16::from(solo) >= k as i16 {
        let v = solo.wrapping_sub(1) as u8;
        s.actors.set_tail(crate::actors::tail::SOLO_MEMBER, v);
    }
    if usize::from(s.actors.party.leader_index) == k {
        with_current_io(|io| crate::main_loop::end_solo(s, io));
    }
    if usize::from(s.actors.party.leader_index) > k {
        s.actors.party.leader_index -= 1;
    }
    let p = &mut s.actors.party;
    for i in k..size.min(16) {
        p.members[i] = p.members.get(i + 1).copied().unwrap_or(0);
        if i + 1 < p.names.len() {
            p.names[i] = p.names[i + 1].clone();
        }
    }
    p.size -= 1;
    s.ui.panel.dirty = true;
}

/// 0x1231be: the slime divides (A4-0x7760): up to 8 tries of a random neighbouring tile the
/// slime may stand on, a copy of it created there.
fn slime_divides(s: &mut State, actor: i16) {
    let a = actor as usize;
    for _ in 0..8 {
        let p = s.objects.location[a];
        let nx = (p.x as i16).wrapping_add(s.rng.range(0, 2) as i16 - 1);
        let ny = (p.y as i16).wrapping_add(s.rng.range(0, 2) as i16 - 1);
        if crate::actors::walk::can_move_to(s, actor as u16, nx as u16, ny as u16) {
            let z = u16::from(s.map.centre.z);
            let ty = kind(s, actor);
            let new = objects::create_actor(s, ty, nx as u16, ny as u16, z);
            if new >= 0 {
                leaves::copy_actor(s, actor, new);
                say(s, SLIME, vec![]);
                s.actors.work.prompt = 1;
                s.map.view_dirty = 1;
            }
            return;
        }
    }
}

/// 0x1231be: a party member's hit points in the side panel: the stats panel is marked for a
/// redraw; on the roster (0x91) the number is reprinted in place. The colour changes around it
/// (poisoned / low) are not queued: the next panel redraw paints them.
fn party_numbers(s: &mut State, actor: i16) {
    let p = s.ui.panel.clone();
    if p.mode == 0x90 && member(s, p.member.max(0) as usize) == actor {
        s.ui.panel.dirty = true;
        return;
    }
    if p.mode != 0x91 {
        return;
    }
    let first = usize::from(p.roster_first);
    let size = leaves::party_size(s);
    let mut k = first;
    while k < size && k < first + 5 {
        if member(s, k) == actor {
            let q = &mut s.main.pending_print;
            q.push(Pending::Window(WindowId::Status));
            q.push(Pending::Cursor(14, ((k - first) * 2 + 2) as i16));
            let hp = i16::from(s.actors.attrs.hp[actor as usize]);
            say(s, HP_NUMBER, vec![Text::Word(hp)]);
            s.main
                .pending_print
                .push(Pending::Window(WindowId::Messages));
        }
        k += 1;
    }
}

/// 0x1231be on an object.
fn hurt_object(s: &mut State, obj: i16, t: u16, damage: i16) {
    let o = obj as usize;
    if t == 0x16e {
        let v = usize::from(s.objects.qq[o] >> 8);
        s.actors.attrs.hp[v] = 0;
        s.actors.attrs.status[v] |= 0x10;
        s.objects.obj_status[v] &= !0x02;
        s.actors.worktype[v] = 0;
        say(s, VINE, vec![]);
        s.actors.work.prompt = 1;
        return;
    }
    let tf = s.objects.type_frame[o];
    if objects::stack_kind(&s.objects, tf) != 0 || s.objects.qq[o] & 0xff == 0 {
        return;
    }
    flash(s, obj);
    let frame = s.objects.type_frame[o] >> 10;
    if (0x129..=0x12c).contains(&t) && frame >= 0x0c {
        return;
    }
    if t == 0x62 && frame == 3 {
        return;
    }
    let left = s.objects.qq[o] & 0xff;
    if left > damage as u16 {
        s.objects.qq[o] = (s.objects.qq[o] & 0xff00) | (left - damage as u16);
        return;
    }
    let n = name(s, obj);
    say(s, BROKEN, vec![n]);
    s.actors.work.prompt = 1;
    if objects::find_child(&mut s.objects, obj, 0x151, 0x16) != -1 {
        objects::spring_trap(s, obj);
    }
    if t == 0x7b {
        s.objects.type_frame[o] = t + 0x800;
        s.map.view_dirty = 1;
        leaves::lose_karma(s, 10);
    } else {
        objects::remove(s, obj);
        s.map.view_dirty = 1;
    }
}

/// Original: 0x1221da (combat: an actor drops: asleep, paralysed or dead)
///
/// A rider (0x1af) dismounts first; big creatures keep their whole type as the base. The
/// A4-0x68e2 table gives the frame it lies as (a two-part 0x1ae drops its other half when
/// dead), unless the creature's flags have 0x1000. With no lying frame, a dead creature may
/// leave a new monster (index 0x15 → type 0x157) and vanish (flags 0x0800 / 0x1000; 0x1000
/// near the party with a dissolve); vines re-frame (0x177), 0x19b vanishes. A dead one with
/// flags 0x2000 (or type 0x187 / 0x188) leaves blood (0x152, random frame 0..2).
pub fn lie_down(s: &mut State, actor: i16) {
    let a = actor as usize;
    let mut t = kind(s, actor);
    let dead = status(s, actor) & 0x10 != 0;
    if t == 0x1af {
        super::seams::dismount(s, actor);
        t = kind(s, actor);
    } else if t == 0x19b || t >= 0x1aa || t == 0x19d || t == 0x19c {
        s.actors.attrs.base_type_frame[a] = s.objects.type_frame[a];
    }
    let index = objects::monster_index(&s.objects, t);
    let flags = s.combat.tables.flags(index);
    let lying = s.combat.tables.lying.iter().find(|&&(k, _)| k == t);
    match lying {
        Some(&(_, tf)) if flags & 0x1000 == 0 => {
            if t == 0x1ae {
                if dead {
                    let other = objects::owner_of_part(s, actor);
                    objects::remove(s, other);
                    s.objects.type_frame[a] = tf;
                }
            } else {
                s.objects.type_frame[a] = tf;
            }
        }
        _ if dead => {
            if index >= 0 {
                if index == 0x15 {
                    let p = s.objects.location[a];
                    let z = u16::from(s.map.centre.z);
                    objects::spawn_monster(s, 0x157, p.x, p.y, z);
                }
                if flags & 0x0800 != 0 || flags & 0x1000 != 0 {
                    let p = s.objects.location[a];
                    objects::remove(s, actor);
                    let c = s.map.centre;
                    // In the view (spec 13 Decision 5; within 5 in Original).
                    let near = |u: u16, v: u16, half: usize| {
                        (u as i16).wrapping_sub(v as i16).wrapping_abs() <= half as i16
                    };
                    if flags & 0x1000 != 0 && near(p.x, c.x, HALF_W) && near(p.y, c.y, HALF_H) {
                        with_current_io(|io| crate::gfx::render_window(s, io, true));
                        present::capture_window(s, present::WINDOW_MS);
                        objects::build_draw_lists(s);
                        s.gfx.dissolve = 1;
                        with_current_io(|io| crate::gfx::render_window(s, io, true));
                        present::capture_dissolve(s, present::DISSOLVE_MS);
                    }
                }
            }
            if t == 0x177 {
                objects::update_vine_frames(s);
            }
            if t == 0x19b {
                objects::remove(s, actor);
            }
        }
        _ => {}
    }
    if dead && ((index >= 0 && flags & 0x2000 != 0) || t == 0x187 || t == 0x188) {
        let p = s.objects.location[a];
        let z = u16::from(s.map.centre.z);
        let o = objects::create_on_map(&mut s.objects, 0x152, 0x20, 0, p.x, p.y, z);
        if o >= 0 {
            let f = s.rng.range(0, 2);
            let w = &mut s.objects.type_frame[o as usize];
            *w = (*w & 0x3ff) + (f << 10);
        }
    }
    s.map.view_dirty = 1;
}

/// Original: 0x123aec (combat: the party attacked a neutral: karma −5; nearby guards (0x17e,
/// within 32) come to arrest (0x12), other neutrals within 6 that can act flee (0x13) or, in a
/// worktype above 0x86, attack (0x9b))
pub fn rouse_neutrals(s: &mut State) {
    leaves::lose_karma(s, 5);
    let c = s.map.centre;
    for a in 0..0x100i16 {
        let i = a as usize;
        if s.objects.type_frame[i] == 0 || s.objects.location[i].z != c.z {
            continue;
        }
        let st = status(s, a);
        if st & 0x10 != 0 || st & 0x80 != 0 || st & 0x60 != 0 {
            continue;
        }
        if kind(s, a) == 0x17e {
            if leaves::distance(s, a, c.x as i16, c.y as i16) < 0x20 {
                s.actors.worktype[i] = 0x12;
            }
            continue;
        }
        let wt = s.actors.worktype[i];
        if wt == 0x0e || wt == 0 || st & 0x16 != 0 {
            continue;
        }
        if s.actors.attrs.movement_flags[i] & 0x10 != 0 {
            continue;
        }
        if leaves::distance(s, a, c.x as i16, c.y as i16) >= 6 {
            continue;
        }
        if wt < 0x80 {
            s.actors.worktype[i] = 0x13;
        } else if wt > 0x86 {
            if kind(s, a) == 0x188 {
                s.objects.type_frame[i] = s.actors.attrs.base_type_frame[i];
            }
            s.actors.worktype[i] = 0x9b;
        }
    }
}

/// Original: 0x123c56 (combat: a neutral the player attacks: a humanoid hurt by the player
/// yells for help (0x123aec); one on a schedule fights back (0x9b) when the player attacked;
/// one in a worktype below 0x80 takes the side against its attacker's alignment, monsters
/// (from 0xe0) attack (8))
pub fn yell_for_help(s: &mut State, target: i16, attacker: i16, damage: i16) {
    if target >= 0x100 {
        return;
    }
    let t = target as usize;
    if status(s, target) & 0x60 != 0 || status(s, target) & 0x80 != 0 {
        return;
    }
    let index = objects::monster_index(&s.objects, kind(s, target));
    let player = s.actors.worktype[attacker as usize] == 2;
    if player && index >= 0 && s.combat.tables.flags(index) & 0x4000 != 0 && damage > 0 {
        let n = name(s, target);
        say(s, YELLS, vec![n]);
        rouse_neutrals(s);
        s.actors.work.prompt = 1;
    }
    if s.actors.worktype[t] >= 0x80 {
        if player {
            if kind(s, target) == 0x188 {
                s.objects.type_frame[t] = s.actors.attrs.base_type_frame[t];
            }
            s.actors.worktype[t] = 0x9b;
        }
        return;
    }
    let side = if status(s, attacker) & 0x60 == 0x40 {
        0x60
    } else {
        0x40
    };
    s.actors.attrs.status[t] = (s.actors.attrs.status[t] & 0x9f) | side;
    if target >= 0xe0 {
        s.actors.worktype[t] = 8;
    }
}

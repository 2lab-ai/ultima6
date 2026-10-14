//! Spells on one target (0x116f2c) and the effect routines it calls.

use std::cell::RefCell;

use super::seams;
use super::tables::Msg;
use crate::engine::Io;
use crate::main_loop::game_string;
use crate::objects::Slot;
use crate::state::State;
use crate::ui::printf::{Arg, printf};

/// Image strings of hunk 11.
pub(super) const FMT_S: u32 = 0x0011_58b0;
pub(super) const NEWLINE: u32 = 0x0011_58ba;
pub(super) const FMT_S_NL: u32 = 0x0011_58ca;
pub(super) const FMT_NL_S_NL: u32 = 0x0011_58ce;
pub(super) const FMT_NL_S_BANG: u32 = 0x0011_58e4;
const MSG_PARALYZED: u32 = 0x0011_58d4;
const MSG_CHARMED: u32 = 0x0011_58bc;

/// How 0x116f2c is called: the missile tile, the range, the kind of target (1 = an actor, -1
/// = an object, else anything) and whether the target is chosen already.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub tile: u16,
    pub range: i16,
    pub kind: i16,
    pub chosen: bool,
}

pub(super) async fn print_at(state: &RefCell<State>, io: &Io, addr: u32, args: &[Arg<'_>]) {
    let fmt = game_string(io, addr);
    printf(state, io, &fmt, args).await;
}

/// Prints result message `m` with format `fmt` (`%s`, `\n%s\n` or `\n%s!\n`).
pub(super) async fn say(state: &RefCell<State>, io: &Io, fmt: u32, m: Msg) {
    let text = state.borrow().magic.tables.message(m).to_vec();
    print_at(state, io, fmt, &[Arg::Str(&text)]).await;
}

/// The result line after an effect: 0 "Success", 1 "Failed", 2 "No effect", 0xff / 0xfe the
/// effect printed its own.
pub(super) async fn report(state: &RefCell<State>, io: &Io) {
    let r = state.borrow().magic.result;
    let m = match r {
        0 => Msg::Success,
        1 => Msg::Failed,
        2 => Msg::NoEffect,
        _ => return,
    };
    say(state, io, FMT_NL_S_NL, m).await;
}

/// The display name of `obj` (0x10eef8), printed as a format.
pub(super) async fn print_name(state: &RefCell<State>, io: &Io, obj: i16) {
    let name = crate::objects::display_name(&state.borrow(), Slot::from(obj));
    printf(state, io, name.as_bytes(), &[]).await;
}

/// "On whom: " / "On what: " when the caster is a party member (status bit 7), then the
/// target read unless it is chosen, and the target's name or "nothing".
pub(super) async fn prompt_target(
    state: &RefCell<State>,
    io: &Io,
    kind: i16,
    range: i16,
    chosen: bool,
) {
    let member = {
        let s = state.borrow();
        let c = usize::from(super::cast::caster(&s));
        s.actors.attrs.status[c] & 0x80 != 0
    };
    if member {
        let m = if kind == 1 { Msg::OnWhom } else { Msg::OnWhat };
        say(state, io, FMT_S, m).await;
    }
    if !chosen {
        {
            let mut s = state.borrow_mut();
            s.ui.input.target_pending = 1;
            if s.ui.input.targeting != 0 {
                s.ui.input.range = range as i8;
            }
        }
        crate::ui::wait::read_key(state, io).await;
    }
}

/// True when the selection or the location is missing.
pub(super) fn no_target(s: &State) -> bool {
    let i = &s.ui.input;
    i.selection == -1 || i.target.0 == -1 || i.target.1 == -1
}

/// |a − b| as the 16-bit code computes it.
fn dist(a: i16, b: u16) -> i16 {
    a.wrapping_sub(b as i16).wrapping_abs()
}

/// The tile index of object `obj` (base tile of its type plus its frame).
pub(super) fn tile_of(s: &State, obj: Slot) -> usize {
    let tf = s.objects.tf(obj);
    let base = s
        .tables
        .basetile
        .base
        .get(usize::from(tf & 0x3ff))
        .copied()
        .unwrap_or(0);
    usize::from(base.wrapping_add(tf >> 10))
}

/// flags3 (0x13c140) of the tile of `obj`.
pub(super) fn flags3(s: &State, obj: Slot) -> u8 {
    s.tables
        .tileflags
        .flags3
        .get(tile_of(s, obj))
        .copied()
        .unwrap_or(0)
}

/// Original: 0x116f2c (magic: a spell on one target: prompt, missile, range and kind checks,
/// the effect, the result line)
///
/// The prompt and target (see [`prompt_target`]); the caster flashes (0x132338); with a map
/// location the missile flies (0x1228ba). No target: "No effect". A missile that did not
/// arrive: "Blocked!" within the range on both axes, else "Out of range!". An actor spell on
/// an object (slot ≥ 0x100), or an object spell on an actor (except Dispel Field 0x21): "No
/// effect". The target is resolved (0x1240fa), the effect runs and its result is printed.
pub async fn on_target(state: &RefCell<State>, io: &Io, spell: u16, t: Target) {
    prompt_target(state, io, t.kind, t.range, t.chosen).await;
    let (missing, caster) = {
        let s = state.borrow();
        (no_target(&s), super::cast::caster(&s))
    };
    if missing {
        say(state, io, FMT_S_NL, Msg::Nothing).await;
    } else {
        let sel = state.borrow().ui.input.selection;
        print_name(state, io, sel).await;
        print_at(state, io, NEWLINE, &[]).await;
    }
    seams::flash_actor(&mut state.borrow_mut(), io, caster, 0);
    let mut reached = 0;
    let mut target = state.borrow().ui.input.target;
    if target.0 != -1 && target.1 != -1 {
        reached = seams::missile(state, io, caster, t.range, &mut target, t.tile).await;
        state.borrow_mut().ui.input.target = target;
    }
    if no_target(&state.borrow()) {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    if reached == 0 {
        let near = {
            let s = state.borrow();
            let at = s
                .objects
                .location(crate::objects::ObjectId(caster))
                .unwrap_or_default();
            let (tx, ty) = s.ui.input.target;
            dist(tx, at.x) <= t.range && dist(ty, at.y) <= t.range
        };
        let m = if near { Msg::Blocked } else { Msg::OutOfRange };
        say(state, io, FMT_NL_S_BANG, m).await;
        return;
    }
    let sel = state.borrow().ui.input.selection;
    if (t.kind == 1 && sel >= 0x100) || (t.kind == -1 && sel < 0x100 && spell != 0x21) {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    {
        let mut s = state.borrow_mut();
        let sel = seams::target_owner(&mut s, sel);
        s.ui.input.selection = sel;
        s.magic.result = 0;
    }
    effect(state, io, spell, caster).await;
    report(state, io).await;
}

fn set_result(state: &RefCell<State>, r: u8) {
    state.borrow_mut().magic.result = r;
}

/// The view after an object appeared or vanished: dissolve the next present, view flags,
/// refresh (A4-0x6578 = 1, 0x109e74, 0x102ef0).
pub(super) fn redraw_dissolving(s: &mut State, io: &Io) {
    s.gfx.dissolve = 1;
    crate::map::view::update_view(s);
    crate::gfx::view::refresh_view(s, io);
}

/// A random spot within 5 of the leader on both axes (two draws 0..10, x first).
fn near_leader(s: &mut State) -> (u16, u16) {
    let c = s.map.centre;
    let x = c.x.wrapping_add(s.rng.range(0, 10)).wrapping_sub(5);
    let y = c.y.wrapping_add(s.rng.range(0, 10)).wrapping_sub(5);
    (x, y)
}

/// The effect of spell `spell` on the selected target (0x1170b4..0x1176c6).
async fn effect(state: &RefCell<State>, io: &Io, spell: u16, caster: u16) {
    let sel = state.borrow().ui.input.selection;
    let target = sel as u16;
    match spell {
        0x03 => {
            if dispel(&mut state.borrow_mut(), io, target) {
                set_result(state, 2);
            }
        }
        0x04 | 0x08 => douse_ignite(state, io, Slot::from(sel), spell).await,
        0x05 => harm(state, io, spell).await,
        0x06 | 0x34 => {
            seams::flash_actor(&mut state.borrow_mut(), io, target, 1);
            heal(&mut state.borrow_mut(), spell);
        }
        0x11 => magic_arrow(state, io, caster).await,
        0x12 => {
            let r = poison(&mut state.borrow_mut(), caster, target);
            set_result(state, r);
        }
        0x14 => {
            let r = sleep(&mut state.borrow_mut(), caster, target);
            set_result(state, r);
        }
        0x19 => {
            let mut s = state.borrow_mut();
            if flags3(&s, Slot::from(sel)) & 8 == 0 {
                s.magic.result = 2;
            } else {
                // A4-0x7b62 (objlist block bytes 0x22-0x23): the vanished type for Reappear.
                let tf = s.objects.tf(Slot::from(sel)).to_be_bytes();
                s.actors.set_tail(0x22, tf[0]);
                s.actors.set_tail(0x23, tf[1]);
                crate::objects::remove(&mut s, Slot::from(sel));
                redraw_dissolving(&mut s, io);
            }
        }
        0x20 => state.borrow_mut().objects.obj_status[usize::from(target)] |= 0x40,
        0x21 => dispel_field(&mut state.borrow_mut()),
        0x22 => {
            let hit = fireball(state, io, target).await;
            set_result(state, if hit { 0xff } else { 2 });
        }
        0x28 => state.borrow_mut().actors.attrs.status[usize::from(target)] |= 1,
        0x30 => animate(&mut state.borrow_mut(), caster, Slot::from(sel)),
        0x43 => {
            let mut s = state.borrow_mut();
            let kind = s.objects.kind(Slot::from(sel));
            if s.objects.status(Slot::from(sel)) & 2 == 0 && can_vanish(kind) {
                s.objects.obj_status[usize::from(target)] |= 2;
                seams::flash_actor(&mut s, io, target, 1);
            } else {
                s.magic.result = 2;
            }
        }
        0x44 => {
            let exp = {
                let mut s = state.borrow_mut();
                let d = s.rng.range(1, 0x1e) as i16;
                seams::hurt(&mut s, target, d)
            };
            seams::add_experience(&mut state.borrow_mut(), caster, exp);
            print_at(state, io, NEWLINE, &[]).await;
            seams::wound_line(state, io, target).await;
            seams::provoke(&mut state.borrow_mut(), target, caster, 1);
            set_result(state, 0xff);
        }
        0x45 => {
            {
                let mut s = state.borrow_mut();
                s.actors.attrs.status[usize::from(target)] |= 2;
                seams::hit_flash(&mut s, target);
            }
            print_at(state, io, NEWLINE, &[]).await;
            print_name(state, io, sel).await;
            print_at(state, io, MSG_PARALYZED, &[]).await;
        }
        0x50 => charm(state, io, caster, target).await,
        0x51 => clone(&mut state.borrow_mut(), io, Slot::from(sel)),
        0x58 => replicate(&mut state.borrow_mut(), io, Slot::from(sel)),
        0x60 => {
            chain_bolt(state, io, target).await;
            set_result(state, 0xff);
        }
        0x65 => {
            let r = kill(state, io, caster, target).await;
            set_result(state, r);
        }
        _ => {}
    }
}

/// The objects 0x115b50 lets turn invisible: types below 0x1aa except the multi-tile
/// creatures 0x19b, 0x176, 0x19d and 0x177.
///
/// Original: 0x115b50 (magic: may an object of type `kind` turn invisible)
pub fn can_vanish(kind: u16) -> bool {
    kind < 0x1aa && !matches!(kind, 0x19b | 0x176 | 0x19d | 0x177)
}

/// Original: 0x116dcc (magic: Dispel Magic on `actor`: sleep ends (status 0x04, the base type
/// back), poison (0x08) and paralysis (0x02) end, a charm (object status 0x04) ends through
/// 0x121bce; true when there was nothing to dispel)
pub fn dispel(s: &mut State, io: &Io, actor: u16) -> bool {
    seams::flash_actor(s, io, actor, 1);
    let a = usize::from(actor);
    let asleep = s.actors.attrs.status[a] & 4 != 0;
    if asleep {
        s.actors.attrs.status[a] &= !4;
        s.objects.type_frame[a] = s.actors.attrs.base_type_frame[a];
    }
    let st = &mut s.actors.attrs.status;
    let poisoned = st[a] & 8 != 0;
    if poisoned {
        st[a] &= !8;
    }
    let paralysed = st[a] & 2 != 0;
    if paralysed {
        st[a] &= !2;
    }
    let charmed = s.objects.obj_status[a] & 4 != 0;
    if charmed {
        crate::actors::leaves::uncharm(s, actor);
    }
    !(asleep || poisoned || paralysed || charmed)
}

/// Original: 0x116d0e (magic: Heal (6): +random 1..30 hit points up to the maximum (0x122144);
/// Great Heal (0x34): the maximum; nothing on an actor with status bit 0x10 (dead): "No
/// effect")
pub fn heal(s: &mut State, spell: u16) {
    let t = s.ui.input.selection;
    let a = usize::from(t as u16);
    if s.actors.attrs.status[a] & 0x10 != 0 {
        s.magic.result = 2;
        return;
    }
    if spell == 6 {
        let r = s.rng.range(1, 0x1e) as i16;
        let hp = i16::from(s.actors.attrs.hp[a]);
        let max = crate::actors::leaves::max_hp(s, t as u16) as i16;
        s.actors.attrs.hp[a] = if r + hp < max {
            (hp + r) as u8
        } else {
            max as u8
        };
    } else if spell == 0x34 {
        s.actors.attrs.hp[a] = crate::actors::leaves::max_hp(s, t as u16) as u8;
    }
}

/// Creature flags (A4-0x7dbe word, low byte) of the monster type of `target`; the index of a
/// type outside the list is -1, read as 0.
fn creature_bits(s: &State, target: u16) -> (i16, u16) {
    let kind = s.objects.kind(Slot::from(target as i16));
    let idx = crate::objects::monster_index(&s.objects, kind);
    (idx, s.actors.game.creature_flags(idx))
}

/// Original: 0x115c62 (magic: does `target` dodge `caster`'s spell by strength: random 1..30 <
/// (str(target) / 2 − int(caster) + 30) / 2)
pub fn dodges_by_strength(s: &mut State, caster: u16, target: u16) -> bool {
    let r = s.rng.range(1, 0x1e) as i16;
    let t = crate::actors::leaves::strength(s, target) as i16 / 2;
    let c = crate::actors::leaves::intelligence(s, caster) as i16;
    r < (t - c + 0x1e) / 2
}

/// Original: 0x115cbe (magic: the same by dexterity (0x121ff8))
pub fn dodges_by_dexterity(s: &mut State, caster: u16, target: u16) -> bool {
    let r = s.rng.range(1, 0x1e) as i16;
    let t = crate::actors::leaves::dexterity(s, target) as i16 / 2;
    let c = crate::actors::leaves::intelligence(s, caster) as i16;
    r < (t - c + 0x1e) / 2
}

/// A hit of `damage` from the caster on `target` in 0x11669e's order: the damage, the
/// experience, the target's reaction, a newline, the wound line.
async fn hit_then_line(state: &RefCell<State>, io: &Io, caster: u16, target: u16, d: i16) {
    {
        let mut s = state.borrow_mut();
        let exp = seams::hurt(&mut s, target, d);
        seams::add_experience(&mut s, caster, exp);
        seams::provoke(&mut s, target, caster, d);
    }
    print_at(state, io, NEWLINE, &[]).await;
    seams::wound_line(state, io, target).await;
}

/// Original: 0x11669e (magic: Magic Arrow: dodged by dexterity ("Failed"), else 1..10 damage)
pub async fn magic_arrow(state: &RefCell<State>, io: &Io, caster: u16) {
    let (target, d) = {
        let mut s = state.borrow_mut();
        let target = s.ui.input.selection as u16;
        if dodges_by_dexterity(&mut s, caster, target) {
            s.magic.result = 1;
            return;
        }
        (target, s.rng.range(1, 10) as i16)
    };
    hit_then_line(state, io, caster, target, d).await;
    set_result(state, 0xff);
}

/// Original: 0x116706 (magic: a damage spell on the selection: resisted (0x122076) "Failed";
/// else Harm (5) 1..10, Energy Wind (0x62) 1..30, Disable (0x32) all hit points but one)
pub async fn harm(state: &RefCell<State>, io: &Io, spell: u16) {
    let (caster, target, d) = {
        let mut s = state.borrow_mut();
        let caster = super::cast::caster(&s);
        let target = s.ui.input.selection as u16;
        if seams::resists(&mut s, caster, target) {
            s.magic.result = 1;
            return;
        }
        let d = match spell {
            5 => s.rng.range(1, 10) as i16,
            0x62 => s.rng.range(1, 0x1e) as i16,
            0x32 => i16::from(s.actors.attrs.hp[usize::from(target)]) - 1,
            _ => 0,
        };
        (caster, target, d)
    };
    print_at(state, io, NEWLINE, &[]).await;
    {
        let mut s = state.borrow_mut();
        let exp = seams::hurt(&mut s, target, d);
        seams::add_experience(&mut s, caster, exp);
        seams::provoke(&mut s, target, caster, d);
    }
    seams::wound_line(state, io, target).await;
    set_result(state, 0xff);
}

/// Original: 0x11663e (magic: Poison: creatures with flag 0x04 are immune ("No effect"); a
/// strength dodge (0x115c62) "Failed"; else status bit 0x08 and the hit flash (0xfe))
pub fn poison(s: &mut State, caster: u16, target: u16) -> u8 {
    let (_, flags) = creature_bits(s, target);
    if flags & 4 != 0 {
        return 2;
    }
    if dodges_by_strength(s, caster, target) {
        return 1;
    }
    s.actors.attrs.status[usize::from(target)] |= 8;
    seams::hit_flash(s, target);
    0xfe
}

/// Original: 0x1165d2 (magic: Sleep: creatures with flag 0x01 never sleep ("No effect"); a
/// resist (0x122076) "Failed"; else the hit flash, status bit 0x04 and the sleeper lies down
/// (0x1221da), no message (0xfe))
pub fn sleep(s: &mut State, caster: u16, target: u16) -> u8 {
    let (idx, flags) = creature_bits(s, target);
    if idx >= 0 && flags & 1 != 0 {
        return 2;
    }
    if seams::resists(s, caster, target) {
        return 1;
    }
    seams::hit_flash(s, target);
    s.actors.attrs.status[usize::from(target)] |= 4;
    seams::lie_down(s, target);
    0xfe
}

/// Original: 0x116e60 (magic: Fireball on `target`: creatures with flag 0x08 are immune
/// (false); 1..20 damage, doubled for flag 0x10; true when it hit)
pub async fn fireball(state: &RefCell<State>, io: &Io, target: u16) -> bool {
    let caster = {
        let mut s = state.borrow_mut();
        let (idx, flags) = creature_bits(&s, target);
        if idx >= 0 && flags & 8 != 0 {
            return false;
        }
        let mut d = s.rng.range(1, 0x14) as i16;
        if flags & 0x10 != 0 {
            d = d.wrapping_add(d);
        }
        let exp = seams::hurt(&mut s, target, d);
        let caster = super::cast::caster(&s);
        seams::add_experience(&mut s, caster, exp);
        caster
    };
    print_at(state, io, NEWLINE, &[]).await;
    seams::provoke(&mut state.borrow_mut(), target, caster, 1);
    seams::wound_line(state, io, target).await;
    true
}

/// Original: 0x11687e (magic: Kill: the dead (status 0x10) and creatures with flag 0x02 are
/// immune (2); a resist "Failed" (1); else damage equal to its hit points (0xfe))
pub async fn kill(state: &RefCell<State>, io: &Io, caster: u16, target: u16) -> u8 {
    {
        let mut s = state.borrow_mut();
        let (idx, flags) = creature_bits(&s, target);
        let dead = s.actors.attrs.status[usize::from(target)] & 0x10 != 0;
        if dead || (idx >= 0 && flags & 2 != 0) {
            return 2;
        }
        if seams::resists(&mut s, caster, target) {
            return 1;
        }
        let d = i16::from(s.actors.attrs.hp[usize::from(target)]);
        let exp = seams::hurt(&mut s, target, d);
        seams::add_experience(&mut s, caster, exp);
        seams::provoke(&mut s, target, caster, 1);
    }
    seams::wound_line(state, io, target).await;
    0xfe
}

/// Original: 0x1167bc (magic: Charm: a resist "Failed"; an uncharmed target keeps its
/// alignment in its movement flags, gets object status 0x04, the caster's alignment and
/// worktype 8, flashes and "is charmed." is printed; a charmed one is uncharmed (0x121bce))
pub async fn charm(state: &RefCell<State>, io: &Io, caster: u16, target: u16) {
    let charmed = {
        let mut s = state.borrow_mut();
        if seams::resists(&mut s, caster, target) {
            s.magic.result = 1;
            return;
        }
        let t = usize::from(target);
        if s.objects.obj_status[t] & 4 == 0 {
            let a = &mut s.actors.attrs;
            a.movement_flags[t] = (a.status[t] & 0x60) | (a.movement_flags[t] & 0x9f);
            s.objects.obj_status[t] |= 4;
            let a = &mut s.actors.attrs;
            a.status[t] = (a.status[usize::from(caster)] & 0x60) | (a.status[t] & 0x9f);
            s.actors.worktype[t] = 8;
            seams::hit_flash(&mut s, target);
            true
        } else {
            crate::actors::leaves::uncharm(&mut s, target);
            false
        }
    };
    if charmed {
        print_at(state, io, NEWLINE, &[]).await;
        print_name(state, io, target as i16).await;
        print_at(state, io, MSG_CHARMED, &[]).await;
        set_result(state, 0xfe);
    }
}

/// A4-0x756f: "A torch burned out." (Douse on a lit torch).
const MSG_TORCH_OUT: u32 = 0x756f;
/// Objlist block bytes of the keg timer (A4-0x7b2a) and its object (A4-0x7b28).
const KEG_TIMER: usize = 0x5a;
const KEG_OBJECT: usize = 0x5c;

/// Original: 0x116912 (magic: Douse (4) / Ignite (8) a light source `obj`: torches 0x5a,
/// braziers and the like (0xa4, 0x7a, 0xfd, 0xce frames 0..1, 0x91, 0xdf); frame bit 0 = lit)
///
/// Dousing an unlit one or lighting a lit one: "No effect". Lighting the powder keg 0xdf
/// (frame 0) starts the keg timer (3, this object); lighting another sets frame bit 0 and the
/// lit status 0x80; dousing a torch removes it ("A torch burned out."); dousing another keeps
/// frame bit 1 only and clears 0x80. Anything else: "No effect". The view flags follow.
pub async fn douse_ignite(state: &RefCell<State>, io: &Io, obj: Slot, spell: u16) {
    let burned_out = {
        let mut s = state.borrow_mut();
        let tf = s.objects.tf(obj);
        let (kind, frame) = (tf & 0x3ff, tf >> 10);
        let light = matches!(kind, 0x5a | 0xa4 | 0x7a | 0xfd | 0xce | 0x91 | 0xdf)
            && (kind != 0xce || frame < 2);
        let i = obj as usize;
        let mut out = false;
        if !light || (spell == 4 && frame & 1 == 0) || (spell == 8 && frame & 1 != 0) {
            s.magic.result = 2;
        } else if kind == 0xdf && frame == 0 {
            s.objects.type_frame[i] = kind + 0x400;
            s.actors.set_tail_word(KEG_TIMER, 3);
            s.actors.set_tail_word(KEG_OBJECT, obj as u16);
        } else if frame & 1 == 0 {
            s.objects.type_frame[i] = ((frame | 1) << 10) + kind;
            s.objects.obj_status[i] |= 0x80;
        } else if kind == 0x5a {
            crate::objects::remove(&mut s, obj);
            out = true;
        } else {
            s.objects.type_frame[i] = ((frame & 2) << 10) + kind;
            s.objects.obj_status[i] &= 0x7f;
        }
        out
    };
    if burned_out {
        print_at(state, io, NEWLINE, &[]).await;
        let msg = crate::main_loop::a4_string(io, MSG_TORCH_OUT);
        printf(state, io, &msg, &[]).await;
    }
    crate::map::view::update_view(&mut state.borrow_mut());
}

/// Dispel Field (0x21): the first field (0x13d..0x140) at the target goes, with the hit flash
/// on it and the view flags; none there: "No effect".
fn dispel_field(s: &mut State) {
    let (x, y) = s.ui.input.target;
    let z = u16::from(s.map.centre.z);
    let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, x as u16, y as u16, z);
    while o >= 0 {
        if (0x13d..=0x140).contains(&s.objects.kind(o)) {
            crate::objects::remove(s, o);
            seams::hit_flash(s, o as u16);
            crate::map::view::update_view(s);
            return;
        }
        o = crate::objects::next_at(&mut s.objects, &s.tables);
    }
    s.magic.result = 2;
}

/// A new creature takes the caster's alignment (status bits 0x60) and worktype `work`.
fn adopt(s: &mut State, new: Slot, from: u16, work: u8) {
    let Ok(n) = usize::try_from(new) else {
        return;
    };
    let a = &mut s.actors.attrs;
    a.status[n] = (a.status[n] & 0x9f) | (a.status[usize::from(from)] & 0x60);
    s.actors.worktype[n] = work;
}

/// Animate (0x30): an object with a ready slot (0x110f96) becomes a creature of its type at its
/// place (0x131646) on the caster's side with worktype 8, and the object goes; else "No
/// effect". The port skips the writes when no creature could be made (the original writes
/// through slot -1).
fn animate(s: &mut State, caster: u16, obj: Slot) {
    if seams::ready_slot(s, obj) == -1 {
        s.magic.result = 2;
        return;
    }
    let (kind, x, y) = (s.objects.kind(obj), s.objects.x(obj), s.objects.y(obj));
    let z = u16::from(s.map.centre.z);
    let new = crate::objects::spawn_monster(s, kind, x, y, z);
    adopt(s, new, caster, 8);
    crate::objects::remove(s, obj);
}

/// Clone (0x51): up to 8 random spots near the leader until the target could stand there
/// (0x11b1c0); none: "No effect"; else a creature of its type there with its alignment and
/// worktype, shown dissolving.
fn clone(s: &mut State, io: &Io, obj: Slot) {
    let t = obj as u16;
    let mut spot = None;
    for _ in 0..8 {
        let (x, y) = near_leader(s);
        if crate::actors::walk::can_move_to(s, t, x, y) {
            spot = Some((x, y));
            break;
        }
    }
    let Some((x, y)) = spot else {
        s.magic.result = 2;
        return;
    };
    let z = u16::from(s.map.centre.z);
    let new = crate::objects::spawn_monster(s, s.objects.kind(obj), x, y, z);
    if let Ok(n) = usize::try_from(new) {
        let a = &mut s.actors.attrs;
        a.status[n] = (a.status[n] & 0x9f) | (a.status[usize::from(t)] & 0x60);
        s.actors.worktype[n] = s.actors.worktype[usize::from(t)];
    }
    redraw_dissolving(s, io);
}

/// Replicate (0x58): an object whose tile has flags3 bit 3, copied (quantity 1, ok to take) to
/// the first of up to 8 random spots near the leader where it may lie (0x11b1c0); copies that
/// may not lie there are removed again. No tile flag or no spot: "Failed".
fn replicate(s: &mut State, io: &Io, obj: Slot) {
    if flags3(s, obj) & 8 == 0 {
        s.magic.result = 1;
        return;
    }
    for _ in 0..8 {
        let (x, y) = near_leader(s);
        let z = u16::from(s.map.centre.z);
        let loc = crate::objects::pack_xyz(x, y, z).to_packed();
        let tf = s.objects.tf(obj).to_be_bytes();
        let new =
            crate::objects::create_from_record(s, [0, loc[0], loc[1], loc[2], tf[0], tf[1], 0, 1]);
        crate::objects::mark_takeable(&mut s.objects, new);
        if crate::actors::walk::can_move_to(s, new as u16, x, y) {
            redraw_dissolving(s, io);
            return;
        }
        crate::objects::remove(s, new);
    }
    s.magic.result = 1;
}

/// Original: 0x116a70 (magic: the actor nearest `from` that it can see (0x1224ea), alive, in
/// view of the leader (strictly within 5 on both axes), not `from`; ties broken by random
/// 0..1; -1 if none)
pub fn nearest_seen(s: &mut State, from: u16) -> i16 {
    let (fx, fy) = (
        s.objects.x(from as Slot) as i16,
        s.objects.y(from as Slot) as i16,
    );
    let c = s.map.centre;
    let (cx, cy) = (c.x as i16, c.y as i16);
    let (mut best, mut found) = (0x7fffi16, -1i16);
    for a in 0..0x100u16 {
        let i = usize::from(a);
        if s.objects.type_frame[i] == 0
            || s.actors.attrs.status[i] & 0x10 != 0
            || a == from
            || !crate::actors::leaves::can_see(s, from, a)
        {
            continue;
        }
        let (x, y) = (s.objects.x(a as Slot) as i16, s.objects.y(a as Slot) as i16);
        if x <= cx - 5 || x >= cx + 5 || y <= cy - 5 || y >= cy + 5 {
            continue;
        }
        let d = (x - fx)
            .wrapping_mul(x - fx)
            .wrapping_add((y - fy).wrapping_mul(y - fy));
        if d <= 0 || d > best {
            continue;
        }
        if d == best {
            if s.rng.range(0, 1) == 0 {
                found = a as i16;
            }
        } else {
            best = d;
            found = a as i16;
        }
    }
    found
}

/// Original: 0x116bd0 (magic: Chain Bolt: 1..30 damage on `target`, then up to six jumps to
/// the nearest seen actor (0x116a70) by missile 0x188 (0x1228ba), 1..30 each; it stops when no
/// one is near or a missile does not arrive)
pub async fn chain_bolt(state: &RefCell<State>, io: &Io, target: u16) {
    let caster = super::cast::caster(&state.borrow());
    let exp = {
        let mut s = state.borrow_mut();
        let d = s.rng.range(1, 0x1e) as i16;
        seams::hurt(&mut s, target, d)
    };
    print_at(state, io, NEWLINE, &[]).await;
    {
        let mut s = state.borrow_mut();
        seams::add_experience(&mut s, caster, exp);
        seams::provoke(&mut s, target, caster, 1);
    }
    seams::wound_line(state, io, target).await;
    let mut from = target;
    for _ in 0..6 {
        let next = nearest_seen(&mut state.borrow_mut(), from);
        if next == -1 {
            return;
        }
        let mut at = {
            let s = state.borrow();
            (s.objects.x(next) as i16, s.objects.y(next) as i16)
        };
        if seams::missile(state, io, from, 8, &mut at, 0x188).await == 0 {
            return;
        }
        from = next as u16;
        {
            let mut s = state.borrow_mut();
            let d = s.rng.range(1, 0x1e) as i16;
            let exp = seams::hurt(&mut s, from, d);
            seams::add_experience(&mut s, caster, exp);
        }
        print_at(state, io, NEWLINE, &[]).await;
        seams::wound_line(state, io, from).await;
    }
}

/// Original: 0x115c06 (magic: a strength contest of `a` against `b` (the Move command's push):
/// random 1..30 < (str(b) / 2 − str(a) + 30) / 2)
pub fn strength_contest(s: &mut State, a: u16, b: u16) -> bool {
    let r = s.rng.range(1, 0x1e) as i16;
    let t = crate::actors::leaves::strength(s, b) as i16 / 2;
    let c = crate::actors::leaves::strength(s, a) as i16;
    r < (t - c + 0x1e) / 2
}

/// Original: 0x118a4e (magic: Fear on one actor: an actor that is not good (alignment bits
/// 0x60 ≠ 0x40), does not resist the caster (0x122076) and is not a creature with flag 0x02
/// flees (worktype 0x13))
pub fn frighten(s: &mut State, actor: u16) {
    let (idx, flags) = creature_bits(s, actor);
    let a = usize::from(actor);
    if actor >= 0x100 || s.actors.attrs.status[a] & 0x60 == 0x40 {
        return;
    }
    let caster = super::cast::caster(s);
    if !seams::resists(s, caster, actor) && (idx < 0 || flags & 2 == 0) {
        s.actors.worktype[a] = 0x13;
    }
}

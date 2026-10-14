//! Regression tests on synthetic data for the merge-gate findings: the spells on a full object
//! pool or without a free temporary-actor slot, and the actor tables read or written with an
//! object slot (≥ 0x100). Each case is one the original reaches with an out-of-table index
//! (see the routines' docs); the port refuses it visibly and leaves the tables alone.

use super::tests::{run, world};
use crate::objects::Slot;
use crate::rng::Rng;
use crate::state::State;

/// The objects of type `kind` at (x, y).
fn kinds_at(s: &mut State, x: u16, y: u16, kind: u16) -> Vec<Slot> {
    let mut out = Vec::new();
    let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, x, y, 0);
    while o >= 0 {
        if s.objects.kind(o) == kind {
            out.push(o);
        }
        o = crate::objects::next_at(&mut s.objects, &s.tables);
    }
    out
}

/// No object slot left: 0x10a79a answers -1 to every create.
fn fill_pool(s: &mut State) {
    s.objects.free.1 = 0;
}

/// The eight directions north first, clockwise.
fn compass(s: &mut State) {
    s.objects.defs.dir_dx = vec![0, 1, 1, 1, 0, -1, -1, -1];
    s.objects.defs.dir_dy = vec![-1, -1, 0, 1, 1, 1, 0, -1];
}

/// An object of type `kind` at (x, y) whose base tile is `tile` (frame 0).
fn object_on_tile(s: &mut State, kind: u16, tile: u16, x: u16, y: u16) -> Slot {
    if s.tables.basetile.base.len() < 0x400 {
        s.tables.basetile.base = vec![0; 0x400];
    }
    s.tables.basetile.base[usize::from(kind)] = tile;
    let o = crate::objects::create_on_map(&mut s.objects, kind, 0, 1, x, y, 0);
    assert!(o >= 0x100, "an object slot");
    o
}

/// The selection and the location set as 0x1225c0 leaves them for a chosen target.
fn select(s: &mut State, o: Slot) {
    s.ui.input.selection = o;
    s.ui.input.target = (s.objects.x(o) as i16, s.objects.y(o) as i16);
}

/// M1: Animate (0x30) on an object that can be readied, with no temporary-actor slot free:
/// no creature, so the object stays and the result is `Msg::NoEffect` (2).
#[test]
fn animate_without_a_free_actor_slot_keeps_the_object() {
    let mut s = world();
    // Base tile 0x200: 0x110f96 readies it in slot 0.
    let o = object_on_tile(&mut s, 0x80, 0x200, 101, 100);
    select(&mut s, o);
    s.objects.free_actors.1 = 0;
    let s = run(s, &[], |st, io| async move {
        super::dispatch(&st, &io, 0x30, true).await;
    });
    assert_eq!(s.objects.type_frame[o as usize], 0x80, "the object stays");
    assert_eq!(s.magic.result, 2, "no effect");
}

/// M2: Heal on an object slot (a potion aimed at an object, 0x116d0e reads the selection raw)
/// or on no selection writes no hit points and draws nothing.
#[test]
fn heal_outside_the_actor_slots_writes_nothing() {
    let mut s = world();
    let o = object_on_tile(&mut s, 0x80, 0x10, 101, 100);
    for sel in [o, 0xbff, -1] {
        s.ui.input.selection = sel;
        let (hp, rng) = (s.actors.attrs.hp.clone(), s.rng);
        super::heal(&mut s, 6);
        super::heal(&mut s, 0x34);
        assert_eq!(s.actors.attrs.hp, hp, "selection {sel:#x}");
        assert_eq!(s.rng, rng, "selection {sel:#x}: no draw");
    }
}

/// M3: Reappear with the object pool full: nothing comes back and the vanished type is kept.
#[test]
fn reappear_on_a_full_pool_keeps_the_vanished_type() {
    let mut s = world();
    s.ui.input.target = (104, 100);
    s.actors.set_tail_word(0x22, 0x0055);
    fill_pool(&mut s);
    let mut s = run(s, &[], |st, io| async move {
        super::area::reappear(&st, &io, 1).await;
    });
    assert!(kinds_at(&mut s, 104, 100, 0x55).is_empty());
    assert_eq!(s.actors.tail_word(0x22), 0x0055, "not forgotten");
}

/// M3: a field spell with the object pool full lays nothing.
#[test]
fn field_on_a_full_pool_lays_nothing() {
    let mut s = world();
    s.ui.input.target = (103, 100);
    fill_pool(&mut s);
    let mut s = run(s, &[], |st, io| async move {
        super::area::field(&st, &io, 0x33, 0x17e, 1).await;
    });
    assert!(kinds_at(&mut s, 103, 100, super::area::FIRE_FIELD).is_empty());
}

/// M3: Web with the object pool full skips every cell.
#[test]
fn web_on_a_full_pool_skips_the_cells() {
    let mut s = world();
    compass(&mut s);
    s.ui.input.target = (104, 100);
    fill_pool(&mut s);
    let mut s = run(s, &[], |st, io| async move {
        super::area::web(&st, &io, 1).await;
    });
    let n: usize = (103..=105)
        .flat_map(|x| (99..=101).map(move |y| (x, y)))
        .map(|(x, y)| kinds_at(&mut s, x, y, super::area::WEB).len())
        .sum();
    assert_eq!(n, 0);
}

/// M3: Replicate (0x58) with the object pool full: no copy, eight spots tried (two draws
/// each, as when no copy may lie anywhere), `Msg::Failed` (1).
#[test]
fn replicate_on_a_full_pool_fails() {
    let mut s = world();
    let o = object_on_tile(&mut s, 0x80, 0x10, 101, 100);
    s.tables.tileflags.flags3 = vec![0; 0x800];
    s.tables.tileflags.flags3[0x10] = 8;
    select(&mut s, o);
    fill_pool(&mut s);
    let s = run(s, &[], |st, io| async move {
        super::dispatch(&st, &io, 0x58, true).await;
    });
    let copies = (0x100..crate::objects::OBJECT_SLOTS)
        .filter(|&i| i != o as usize && s.objects.type_frame[i] == 0x80)
        .count();
    assert_eq!(copies, 0);
    assert_eq!(s.magic.result, 1, "failed");
}

/// M4: the resist check (0x122076) against an object slot: one draw, the object's
/// intelligence read as 0.
#[test]
fn resist_against_an_object_slot_draws_once() {
    let mut s = world();
    s.actors.attrs.intelligence[1] = 4;
    s.rng = Rng::seeded(5);
    let mut r = s.rng;
    let roll = r.range(1, 30) as i16;
    let resisted = super::seams::resists(&mut s, 1, 0x150);
    assert_eq!(s.rng, r, "one draw");
    assert_eq!(resisted, roll < -4 + 15);
}

/// M4: Repel Undead (0x29) where the first object of a tile is an object of an undead type:
/// the resist draw is made, no worktype is written.
#[test]
fn repel_undead_on_an_object_of_an_undead_type() {
    let mut s = world();
    let o = object_on_tile(&mut s, 0x160, 0x10, 101, 100);
    s.objects.defs.monster_types = vec![0x160, 0];
    s.actors.game.creature_flags = vec![0x80];
    let worktypes = s.actors.worktype.clone();
    let before = s.rng;
    let s = run(s, &[], |st, io| async move {
        super::spells::all_foes(&st, &io, 0x29).await;
    });
    let mut r = before;
    r.next_word();
    assert_eq!(s.rng, r, "one draw, for object {o:#x}");
    assert_eq!(s.actors.worktype, worktypes);
}

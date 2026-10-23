//! Unit tests of the Rest code on synthetic data: the party (actors 1, 2) at (20, 20) on open
//! ground (tile flags of the walk tests), no map loaded (every map tile reads 0).

use u6_formats::position::Position;

use super::*;
use crate::engine::{Engine, InputEvent, Key};
use crate::files::Files;
use crate::objects::{IN_INVENTORY, create_in};

const FOOD: u16 = 0x81;

fn camp_world() -> State {
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    crate::actors::walk::tests::creature_tiles(&mut s);
    s.tables.tileflags.weight = vec![0; 1024];
    s.actors.set_tail(tail::SOLO_MEMBER, 0xff);
    s.map.centre = Position { x: 20, y: 20, z: 0 };
    let p = &mut s.actors.party;
    p.size = 2;
    p.members[..2].copy_from_slice(&[1, 2]);
    p.names = vec!["Ann".into(), "Bo".into()];
    for (i, a) in [1usize, 2].into_iter().enumerate() {
        s.objects.type_frame[a] = 0x17a;
        s.actors.attrs.base_type_frame[a] = 0x17a;
        s.objects.location[a] = Position {
            x: 20,
            y: 20 + i as u16,
            z: 0,
        };
        s.actors.attrs.status[a] = 0x80;
        s.actors.attrs.level[a] = 2;
        s.actors.attrs.dexterity[a] = 17;
        s.actors.worktype[a] = 2;
    }
    let t = &mut s.inventory.tables;
    t.food = vec![FOOD];
    // Camp places: member 0 on the centre, member 1 one east; both face south.
    t.camp = vec![0; 48];
    t.camp[1] = 1;
    t.camp[16] = 4;
    t.camp[17] = 4;
    crate::actors::walk::tests::link_objects(&mut s);
    s
}

fn run(s: State, keys: &[Key]) -> Engine {
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        rest(&st, &io).await;
    });
    let ev: Vec<InputEvent> = keys.iter().map(|&k| InputEvent::Key(k)).collect();
    e.tick(&ev, 16);
    for _ in 0..200 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    e
}

#[test]
fn food_is_eaten_from_the_member_or_a_fellow() {
    let mut s = camp_world();
    s.objects.defs.stackable = vec![FOOD];
    s.inventory.tables.food = vec![FOOD, 0x82];
    let apple = create_in(&mut s, FOOD, IN_INVENTORY, 2, 2);
    assert!(is_food(&s, 0x82) && !is_food(&s, 0x83));
    assert!(!eat(&mut s, 1), "member 0 carries nothing");
    assert!(member_eats(&mut s, 0), "Bo shares");
    assert_eq!(s.objects.qq_of(apple) & 0xff, 1);
}

#[test]
fn camp_checks_refuse_in_their_order() {
    let mut s = camp_world();
    assert_eq!(refusal(&mut s), None);
    let mut t = s.clone();
    t.actors.set_tail(tail::COMBAT_MODE, 1);
    assert_eq!(refusal(&mut t), Some(STR_COMBAT));
    let mut t = s.clone();
    t.tables.tileflags.flags1[0] = 0x14;
    assert_eq!(refusal(&mut t), Some(STR_WILDERNESS));
    // The window less its last column (Original 39 tiles from x 0: 38 is in, 39 past it).
    let x0 = geom::window_origin(20);
    let (inside, past) = (
        x0.wrapping_add(geom::WINDOW as u16 - 2) & 0x3ff,
        x0.wrapping_add(geom::WINDOW as u16 - 1) & 0x3ff,
    );
    let mut t = s.clone();
    t.objects.type_frame[50] = 0x17a;
    t.objects.location[50] = Position {
        x: inside,
        y: 30,
        z: 0,
    };
    t.actors.attrs.status[50] = 0x20;
    assert_eq!(refusal(&mut t), Some(STR_FOES));
    t.actors.attrs.status[50] = 0x40;
    assert_eq!(refusal(&mut t), None, "a friendly actor far off");
    t.objects.location[50] = Position {
        x: past,
        y: 30,
        z: 0,
    };
    t.actors.attrs.status[50] = 0x20;
    assert_eq!(
        refusal(&mut t),
        None,
        "a foe past the window less its last column"
    );
    t.actors.attrs.status[50] = 0x40;
    t.objects.location[50] = Position { x: 24, y: 17, z: 0 };
    assert_eq!(refusal(&mut t), Some(STR_OTHERS));
    let mut t = s.clone();
    t.actors.set_tail(tail::SOLO_MEMBER, 0);
    assert_eq!(refusal(&mut t), Some(STR_SOLO));
    let mut t = s.clone();
    t.actors.party.leader_index = 2;
    assert_eq!(
        refusal(&mut t),
        Some(STR_NO_REPAIR),
        "aboard something not a ship"
    );
}

#[test]
fn eight_hours_pass_and_the_fed_party_heals() {
    let mut s = camp_world();
    create_in(&mut s, FOOD, IN_INVENTORY, 3, 1);
    s.actors.attrs.hp[1] = 5;
    s.actors.attrs.hp[2] = 7;
    s.actors.calendar.hour = 20;
    let max = crate::actors::leaves::max_hp(&s, 1);
    let e = run(s, &[Key::Char('8'), Key::Space]);
    assert!(e.finished());
    let s = e.state();
    assert_eq!(s.actors.calendar.hour, 4, "eight hours on");
    assert!(s.actors.attrs.hp[1] > 5 && u16::from(s.actors.attrs.hp[1]) <= max);
    assert!(s.actors.attrs.hp[2] > 7);
    assert_eq!(s.actors.rest_counter, 0x0c);
    assert_eq!(s.actors.movement_points[1], 17);
    assert_eq!(s.objects.type_frame[1], 0x17a, "awake again");
    let fire = (0x101..crate::objects::OBJECT_SLOTS)
        .find(|&i| s.objects.type_frame[i] & 0x3ff == CAMPFIRE)
        .expect("campfire");
    assert_eq!(s.objects.type_frame[fire] >> 10, 0, "the fire is out");
}

#[test]
fn a_bad_hour_key_ends_rest_before_the_camp() {
    let s = camp_world();
    let e = run(s, &[Key::Char('x')]);
    assert!(e.finished());
    let s = e.state();
    assert!(
        (0x101..crate::objects::OBJECT_SLOTS).all(|i| s.objects.type_frame[i] & 0x3ff != CAMPFIRE)
    );
}

#[test]
fn the_hull_mends_five_points_up_to_100() {
    let mut s = camp_world();
    s.objects.type_frame[3] = SHIP;
    s.actors.party.members[2] = 3;
    s.actors.party.leader_index = 2;
    s.actors.attrs.hp[3] = 90;
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        repair_hull(&st, &io).await;
        repair_hull(&st, &io).await;
        repair_hull(&st, &io).await;
    });
    for _ in 0..20 {
        e.tick(&[], 16);
    }
    assert!(e.finished());
    assert_eq!(e.state().actors.attrs.hp[3], 100);
    assert_eq!(e.state().actors.calendar.minute, 15);
}

/// Four hours with both members fed from member 0's pack and wounded; `guard_key` answers the
/// guard question. Returns the hit points of actors 1 and 2 after.
fn rest_four_hours(guard_key: char) -> (u8, u8) {
    let mut s = camp_world();
    create_in(&mut s, FOOD, IN_INVENTORY, 3, 1);
    s.actors.attrs.hp[1] = 5;
    s.actors.attrs.hp[2] = 7;
    let e = run(s, &[Key::Char('4'), Key::Char(guard_key), Key::Space]);
    assert!(e.finished());
    let s = e.state();
    assert_eq!(s.actors.rest_counter, 0x0c, "no ambush: the healing ran");
    (s.actors.attrs.hp[1], s.actors.attrs.hp[2])
}

#[test]
fn the_guard_digit_reaches_every_member_but_the_last() {
    // 0x127ce2..0x127cee: a digit is taken while below '0' + party size, so with two members
    // '1' picks member 0 and '2' falls through to no guard, as in the original.
    let (a, b) = rest_four_hours('1');
    assert_eq!(a, 5, "the guard is not healed");
    assert!(b > 7);
    let (a, b) = rest_four_hours('2');
    assert!(a > 5 && b > 7, "no guard: everybody heals");
}

#[test]
fn only_the_first_member_eats_at_the_meal() {
    // A4-0x77a0 keeps the meal going, and nothing in `game` sets it (its only accesses are the
    // clears at 0x10ed24 and 0x127f10 and the test at 0x127f1a; the channels searched are listed
    // on `meal`), so the meal is one round.
    let mut s = camp_world();
    s.objects.defs.stackable = vec![FOOD];
    let food = create_in(&mut s, FOOD, IN_INVENTORY, 5, 2);
    let e = run(s, &[Key::Char('1'), Key::Char('1'), Key::Space]);
    assert!(e.finished());
    assert_eq!(e.state().objects.qq_of(food) & 0xff, 4, "one item eaten");
}

#[test]
fn an_ambush_places_hostile_attackers_around_the_camp() {
    // Synthetic kinds (the real four are read from `game`): type 0x17a walks on open ground.
    let mut s = camp_world();
    s.actors.game.ambush_kinds = vec![0x17a; 4];
    s.rng = crate::rng::Rng::seeded(7);
    assert!(ambush(&mut s));
    let placed: Vec<usize> = (3..crate::objects::ACTOR_SLOTS)
        .filter(|&a| s.objects.type_frame[a] & 0x3ff == 0x17a)
        .collect();
    assert!(!placed.is_empty() && placed.len() <= 8, "{}", placed.len());
    // The sides just outside the view: west / east HALF_W + 1 / + 2, north / south HALF_H + 2
    // / + 1 (Original 6, 7, 7, 6).
    let (hw, hh) = (HALF_W as i16, HALF_H as i16);
    for &m in &placed {
        let p = s.objects.location[m];
        let (dx, dy) = (p.x as i16 - 20, p.y as i16 - 20);
        assert!(
            dx == -(hw + 1) || dx == hw + 2 || dy == -(hh + 2) || dy == hh + 1,
            "slot {m} at {dx}, {dy}"
        );
        assert_eq!(s.actors.attrs.status[m] & 0x60, 0x20, "slot {m} hostile");
        assert_eq!(s.actors.worktype[m], ATTACK, "slot {m} attacks");
    }
}

#[test]
fn an_ambush_takes_its_kinds_from_the_game() {
    // Without the user's `game` loaded the four kinds are unknown: no monster is placed.
    let mut s = camp_world();
    let before = (0..crate::objects::ACTOR_SLOTS)
        .filter(|&a| s.objects.type_frame[a] != 0)
        .count();
    assert!(!ambush(&mut s));
    let after = (0..crate::objects::ACTOR_SLOTS)
        .filter(|&a| s.objects.type_frame[a] != 0)
        .count();
    assert_eq!(after, before);
}

/// Spec 13 Decision 5: an ambush comes from just outside the view (wide in Wide) and inside the
/// window, for the leader on any cell of its window chunk; each try draws twice (a side, a place
/// along it) and reaches all four sides and the eleven places along them.
#[test]
fn the_ambush_spot_is_outside_the_view_and_inside_the_window() {
    let mut s = camp_world();
    s.rng = crate::rng::Rng::seeded(7);
    let (hw, hh) = (HALF_W as i16, HALF_H as i16);
    let side = geom::WINDOW as i16;
    let mut sides = std::collections::BTreeSet::new();
    let mut along = std::collections::BTreeSet::new();
    for c in 200..208u16 {
        let (ox, oy) = (geom::window_origin(c), geom::window_origin(c + 3));
        for _ in 0..200 {
            let mut r = s.rng;
            r.next_word();
            r.next_word();
            let (cx, cy) = (c as i16, (c + 3) as i16);
            let (x, y) = ambush_spot(&mut s, cx, cy);
            assert_eq!(s.rng, r, "two draws a try");
            let (dx, dy) = (x - cx, y - cy);
            assert!(
                dx.abs() > hw || dy.abs() > hh,
                "({dx}, {dy}) outside the view"
            );
            let (wx, wy) = (x - ox as i16, y - oy as i16);
            assert!(
                (0..side).contains(&wx) && (0..side).contains(&wy),
                "({dx}, {dy}) in the window"
            );
            // North and south lie outside the rows along a west or east side.
            if dy == -(hh + 2) || dy == hh + 1 {
                sides.insert(('y', dy));
                along.insert(dx);
            } else {
                assert!(dx == -(hw + 1) || dx == hw + 2, "({dx}, {dy}) on a side");
                sides.insert(('x', dx));
                along.insert(dy);
            }
        }
    }
    assert_eq!(sides.len(), 4, "every side");
    assert_eq!(
        along.into_iter().collect::<Vec<_>>(),
        (-6..=4).collect::<Vec<_>>()
    );
}

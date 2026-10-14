//! Unit tests of the Rest code on synthetic data: the party (actors 1, 2) at (20, 20) on open
//! ground (tile flags of the walk tests), no map loaded (every map tile reads 0).

use std::cell::RefCell;
use std::rc::Rc;

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
    let mut t = s.clone();
    t.objects.type_frame[50] = 0x17a;
    t.objects.location[50] = Position { x: 38, y: 30, z: 0 };
    t.actors.attrs.status[50] = 0x20;
    assert_eq!(refusal(&mut t), Some(STR_FOES));
    t.actors.attrs.status[50] = 0x40;
    assert_eq!(refusal(&mut t), None, "a friendly actor 18 tiles off");
    t.objects.location[50] = Position { x: 39, y: 30, z: 0 };
    t.actors.attrs.status[50] = 0x20;
    assert_eq!(refusal(&mut t), None, "a foe past the 39-tile square");
    t.actors.attrs.status[50] = 0x40;
    t.objects.location[50] = Position { x: 24, y: 17, z: 0 };
    assert_eq!(refusal(&mut t), Some(STR_OTHERS));
    let mut t = s.clone();
    t.actors.set_tail(tail::SOLO_MEMBER, 0);
    assert_eq!(refusal(&mut t), Some(STR_SOLO));
    let mut t = s.clone();
    t.actors.party.leader_index = 2;
    assert_eq!(refusal(&mut t), Some(STR_NO_REPAIR), "aboard something not a ship");
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
    let text = s.ui.messages.join("|");
    assert!(text.is_empty() || !text.contains("Not"), "{text}");
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

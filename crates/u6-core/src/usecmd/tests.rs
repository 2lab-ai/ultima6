//! Unit tests of the Use handlers on synthetic state (no `game` file: strings print empty and
//! the type tables read empty).

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use u6_formats::position::Position;

use super::{Ux, food, mech, misc};
use crate::engine::{Engine, Io};
use crate::files::Files;
use crate::objects::{IN_CONTAINER, Slot};
use crate::state::State;

/// Object slots the fixtures use.
const A: Slot = 0x200;
const B: Slot = 0x201;
const C: Slot = 0x202;

fn world() -> State {
    let mut s = State::empty();
    s.map.centre = Position {
        x: 100,
        y: 100,
        z: 0,
    };
    s.objects.location[1] = Position {
        x: 100,
        y: 100,
        z: 0,
    };
    s.objects.type_frame[1] = 0x17a;
    s.actors.party.members[0] = 1;
    s.actors.party.size = 1;
    s.actors.party.names = vec!["Avatar".into()];
    s.actors.attrs.status[1] = 0x80;
    s.actors.objlist_tail = vec![0; 0x82];
    s
}

fn put(s: &mut State, slot: Slot, tf: u16, x: u16, y: u16) {
    s.objects.type_frame[slot as usize] = tf;
    s.objects.location[slot as usize] = Position { x, y, z: 0 };
}

fn link(s: &mut State) {
    crate::main_loop::test_support::link_placed_objects(s);
}

/// Runs `game` to its end with no input; returns the engine.
fn run<G, F>(s: State, game: G) -> Engine
where
    G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F,
    F: Future<Output = ()> + 'static,
{
    let mut e = Engine::with_game(s, Files::default(), game);
    for _ in 0..64 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    e
}

// The door, chest, fixed-light, secret-door, lock-pick and torch handlers Use reaches are
// objects::commands' ports since the integration; their tests are in objects/commands/tests.rs.

#[test]
fn eating_takes_one_off_a_counted_stack() {
    let mut s = world();
    s.objects.defs.stackable = vec![0x81];
    put(&mut s, A, 0x81, 100, 99);
    s.objects.qq[A as usize] = 3;
    put(&mut s, B, 0x80, 101, 99);
    link(&mut s);
    let e = run(s, |st, io| async move {
        let ux = Ux::new(&st, &io);
        food::eat(ux, A).await;
        food::eat(ux, B).await;
    });
    assert_eq!(e.state().objects.qq_of(A) & 0xff, 2);
    assert_eq!(e.state().objects.tf(B), 0, "an uncounted food goes");
}

#[test]
fn drink_fan_and_scroll_set_their_globals() {
    let mut s = world();
    put(&mut s, A, 0x73, 100, 99);
    s.actors.wind = 7;
    link(&mut s);
    let e = run(s, |st, io| async move {
        let ux = Ux::new(&st, &io);
        food::drink(ux, A).await;
        food::fan(ux).await;
        food::study_scroll(ux).await;
    });
    let s = e.state();
    assert_eq!(s.objects.tf(A), 0);
    assert_eq!(s.actors.objlist_tail[crate::actors::tail::ALCOHOL], 1);
    assert_eq!(s.actors.wind, 0, "7 + 1 wraps to north");
    assert_eq!(&s.actors.objlist_tail[0x5e..0x60], &[0, 1]);
}

#[test]
fn lever_opens_and_closes_its_portcullis_unless_blocked() {
    let mut s = world();
    put(&mut s, A, 0x10c, 100, 99);
    s.objects.qq[A as usize] = 0x0500;
    put(&mut s, B, 0x12d | 2 << 10, 104, 100);
    s.objects.qq[B as usize] = 0x0500;
    link(&mut s);
    let lever = |s: State| {
        run(s, |st, io| async move {
            mech::lever(Ux::new(&st, &io), A).await;
        })
        .state()
        .clone()
    };
    let gate_at = |s: &mut State| crate::objects::object_of_kind_at(s, 104, 100, 0, 0x136);
    let mut s = lever(s);
    assert_eq!(s.objects.tf(A) >> 10, 1);
    let g = gate_at(&mut s);
    assert!(g >= 0, "a portcullis closes on the marker");
    assert_eq!(
        s.objects.tf(g) >> 10,
        3,
        "frame 3 for a marker with frame bit 1"
    );
    let mut s = lever(s);
    assert_eq!(gate_at(&mut s), -1, "it opens again");
    // An actor under the marker keeps it open.
    s.objects.location[1] = Position {
        x: 104,
        y: 100,
        z: 0,
    };
    link(&mut s);
    let mut s = lever(s);
    assert_eq!(gate_at(&mut s), -1);
    assert_eq!(mech::actor_at(&mut s, 104, 100, 0), 1);
}

#[test]
fn keg_lights_once() {
    let mut s = world();
    put(&mut s, A, 0xdf, 100, 99);
    put(&mut s, B, 0xdf, 101, 99);
    link(&mut s);
    let e = run(s, |st, io| async move {
        let ux = Ux::new(&st, &io);
        mech::light_keg(ux, A).await;
        mech::light_keg(ux, B).await;
    });
    let s = e.state();
    assert_eq!(s.objects.tf(A) >> 10, 1);
    assert_eq!(s.objects.tf(B) >> 10, 0, "Not now: one keg burns at a time");
    assert_eq!(s.actors.tail_word(crate::actors::tail::KEG_TIMER), 3);
    assert_eq!(
        s.actors.tail_word(crate::actors::tail::KEG_OBJECT),
        A as u16
    );
}

#[test]
fn keg_explosion_blows_up_doors_but_not_sealed_ones() {
    let mut s = world();
    put(&mut s, A, 0xdf | 1 << 10, 100, 99);
    put(&mut s, B, 0x12c | 5 << 10, 101, 99);
    put(&mut s, C, 0x12a | 13 << 10, 99, 99);
    link(&mut s);
    crate::objects::commands::explode_keg(&mut s, A);
    assert_eq!(s.objects.tf(A), 0, "the keg goes");
    assert_eq!(s.objects.tf(B), 0, "the door is blown up");
    assert_eq!(
        s.objects.tf(C) & 0x3ff,
        0x12a,
        "a magically locked door stays"
    );
}

#[test]
fn eggs_of_wisps_and_daemons_go_with_their_contents() {
    let mut s = world();
    put(&mut s, A, 0x14f, 110, 100);
    put(&mut s, B, 0x16a, 110, 100);
    put(&mut s, C, 0x14f, 112, 100);
    link(&mut s);
    crate::objects::move_into(&mut s, B, A, IN_CONTAINER);
    misc::clear_eggs(&mut s, 100, 100);
    assert_eq!(s.objects.tf(A), 0);
    assert_eq!(s.objects.tf(B), 0);
    assert_eq!(s.objects.tf(C), 0x14f, "an empty egg stays");
}

#[test]
fn mount_and_dismount_a_horse() {
    let mut s = world();
    s.actors.attrs.base_type_frame[1] = 0x17a;
    link(&mut s);
    crate::objects::commands::mount_horse(&mut s, 1, 100, 100, 2, false);
    assert_eq!(s.objects.tf(1), 0x1af | 2 << 10);
    let rear = crate::objects::object_of_kind_at(&mut s, 100, 100, 0, 0x1af);
    assert!(rear >= 0);
    crate::objects::commands::dismount(&mut s, 1);
    assert_eq!(s.objects.kind(1), 0x17a, "the rider's own type is back");
}

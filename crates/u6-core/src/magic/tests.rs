//! Unit tests on synthetic data (no game files: the tables are filled by hand, the strings
//! the code would read from `game` are missing and print nothing).

use std::cell::RefCell;
use std::rc::Rc;

use super::book::{carried_count, layout, load_book, page_line, quality, set_current_spell};
use super::detect::{carried_object, is_door, magic_tile};
use super::effects::{can_vanish, dispel, heal, nearest_seen, poison, sleep};
use super::input::{letter_bit, spells_before};
use super::{MagicState, SPELL, SPELLBOOK};
use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::objects::{IN_CONTAINER, IN_INVENTORY, READIED, Slot};
use crate::rng::Rng;
use crate::state::State;

/// An empty world with actor 1 at (100, 100) leading a party of one, actors 2..=8 placed
/// (type 0 = absent) by `actors`, slots linked, the test text layer.
pub(super) fn world_with(actors: &[(usize, u16, u16, u16)]) -> State {
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    s.objects.type_frame[1] = 0x17a;
    s.objects.location[1] = crate::objects::pack_xyz(100, 100, 0);
    for &(a, tf, x, y) in actors {
        s.objects.type_frame[a] = tf;
        s.objects.location[a] = crate::objects::pack_xyz(x, y, 0);
    }
    s.actors.party.members[0] = 1;
    s.actors.party.size = 1;
    s.map.centre = s.objects.location[1];
    crate::objects::init_slots(&mut s.objects);
    crate::objects::link_actors(&mut s.objects);
    s
}

pub(super) fn world() -> State {
    world_with(&[])
}

/// A book readied by actor 1 holding `spells`.
fn book(s: &mut State, spells: &[u8]) -> Slot {
    let b = crate::objects::create_in(s, SPELLBOOK, READIED, 0, 1);
    for &n in spells {
        crate::objects::create_in(s, SPELL, IN_CONTAINER, u16::from(n) << 8, b);
    }
    b
}

/// Runs `game` to its end in an engine with no files, typing `keys` one per tick.
pub(super) fn run<F, Fut>(s: State, keys: &[Key], game: F) -> State
where
    F: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> Fut,
    Fut: std::future::Future<Output = ()> + 'static,
{
    let mut e = Engine::with_game(s, Files::default(), game);
    e.tick(&[], 16);
    for &k in keys {
        e.tick(&[InputEvent::Key(k)], 16);
    }
    // Enough display frames for the hit flashes combat holds on screen.
    for _ in 0..600 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    assert!(e.finished());
    e.state().clone()
}

#[test]
fn load_book_collects_both_books_and_finds_the_page() {
    let mut s = world();
    // Circle 1: spells 0..9; circle 2: 0x10 and 0x14 in the second book.
    let b1 = book(&mut s, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let b2 = book(&mut s, &[0x10, 0x14, 3]);
    crate::magic::book::set_quality(&mut s.objects, b1, 9);
    s.magic.books = [b1, b2];
    load_book(&mut s);
    assert_eq!(s.magic.known[0], 0x3ff);
    assert_eq!(s.magic.known[1], 0x11);
    assert_eq!(s.magic.known[2..], [0; 6]);
    assert_eq!(s.magic.current, 9);
    // Spell 9 is the 10th of circle 1: the second page of the circle.
    assert_eq!(s.magic.page_first, 8);
    // The second book had quality 0 and no spell 0 inside: it takes the first new spell met
    // (children are linked right after their container, so the last made comes first).
    assert_eq!(quality(&s.objects, b2), 0x14);
    // Current spell 0x14: one page before it is the whole circle 1.
    set_current_spell(&mut s, 0x14);
    load_book(&mut s);
    assert_eq!(s.magic.page_first, 10);
}

#[test]
fn load_book_spell_0xff_fills_every_circle() {
    let mut s = world();
    let b = book(&mut s, &[0xff, 0x12]);
    s.magic.books = [b, 0];
    load_book(&mut s);
    assert_eq!(s.magic.known, [0x3ff; 8]);
    assert_eq!(
        s.magic.current, 0x12,
        "quality 0 and no spell 0: the first spell"
    );
}

#[test]
fn layout_sets_the_page_arrows() {
    let mut m = MagicState {
        known: [0x3ff, 0x0003, 0, 0, 0, 0, 0, 0],
        page_first: 0,
        ..MagicState::default()
    };
    let (lines, circle) = layout(&mut m);
    assert_eq!(lines, (0..8).collect::<Vec<u16>>());
    assert_eq!(circle, 0);
    assert_eq!((m.prev_page, m.prev_circle), (0xff, 0xff));
    assert_eq!((m.next_page, m.next_circle), (2, 1));
    // The second page of circle 1: back within the circle, on to circle 2.
    m.page_first = 8;
    let (lines, _) = layout(&mut m);
    assert_eq!(lines, [8, 9]);
    assert_eq!((m.prev_page, m.prev_circle), (2, 0xff));
    assert_eq!((m.next_page, m.next_circle), (1, 1));
    // Circle 2's page: back to circle 1, nothing after.
    m.page_first = 10;
    let (lines, circle) = layout(&mut m);
    assert_eq!((lines, circle), (vec![0x10, 0x11], 1));
    assert_eq!((m.prev_page, m.prev_circle), (1, 0));
    assert_eq!(m.next_page, 0xff);
}

#[test]
fn page_line_and_spells_before() {
    let mut m = MagicState::default();
    m.page[..3].copy_from_slice(&[0x10, 0x11, 0x14]);
    assert_eq!(page_line(&m, 0x14), 2);
    assert_eq!(page_line(&m, 0x15), -1);
    let mut s = State::empty();
    s.magic.known = [0x7, 0x3, 0x1, 0, 0, 0, 0, 0];
    assert_eq!(spells_before(&s, 0), 0);
    assert_eq!(spells_before(&s, 2), 5);
    assert_eq!(
        spells_before(&s, 0xff),
        6,
        "past the circles every spell counts"
    );
}

#[test]
fn letter_sets_ignore_order_and_shift_like_the_68000() {
    assert_eq!(letter_bit(b'A'), 1);
    assert_eq!(letter_bit(b'Z'), 1 << 25);
    assert_eq!(
        letter_bit(b'a'),
        0,
        "0x61 - 0x41 = 32: a long shift by 32 gives 0"
    );
}

#[test]
fn carried_counts_stop_at_the_first_object_not_carried() {
    let mut s = world();
    let bag = crate::objects::create_in(&mut s, 0x3c, IN_INVENTORY, 0, 1);
    crate::objects::create_in(&mut s, 0x45, IN_CONTAINER, 3, bag);
    crate::objects::create_in(&mut s, 0x45, IN_INVENTORY, 2, 1);
    crate::objects::create_in(&mut s, 0x46, IN_INVENTORY, 0x0105, 1);
    assert_eq!(carried_count(&s.objects, 1, 0x45, 0), 5, "bags included");
    assert_eq!(
        carried_count(&s.objects, 1, 0x46, 0),
        0,
        "quality 1 is not quality 0"
    );
    assert_eq!(carried_count(&s.objects, 1, 0x46, 1), 5);
    let q = carried_object(&s.objects, 1, 0x46, 0);
    assert!(q > 0, "quality 0 = any");
    assert_eq!(carried_object(&s.objects, 1, 0x46, 2), -1);
    assert_eq!(carried_object(&s.objects, 1, 0x47, 0), -1);
}

/// Reagent table: reagent n is type 0x40 + n; spell 3 needs reagents 0 and 2.
fn reagent_tables(s: &mut State) {
    s.magic.tables.reagents = [0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47];
    s.magic.tables.masks = vec![0; 0x80];
    s.magic.tables.masks[3] = 0b101;
    s.objects.defs.stackable = s.magic.tables.reagents.to_vec();
}

#[test]
fn use_reagents_takes_one_of_each_and_counts_the_line() {
    let mut s = world();
    reagent_tables(&mut s);
    for kind in [0x40, 0x41, 0x42] {
        crate::objects::create_in(&mut s, kind, IN_INVENTORY, 4, 1);
    }
    s.magic.page_reagents = [4, 4, 0, 0, 0, 0, 0, 0];
    super::cast::use_reagents(&mut s, 1, 3, 1);
    let n = |s: &State, k| carried_count(&s.objects, 1, k, 0);
    assert_eq!((n(&s, 0x40), n(&s, 0x41), n(&s, 0x42)), (3, 4, 3));
    assert_eq!(s.magic.page_reagents[..2], [4, 3]);
    // Help (7) leaves the count alone; a line past the page is not written.
    super::cast::use_reagents(&mut s, 1, 7, 1);
    super::cast::use_reagents(&mut s, 1, 3, 0x1a5);
    assert_eq!(s.magic.page_reagents[..2], [4, 3]);
    assert_eq!(n(&s, 0x40), 2);
}

#[test]
fn typed_syllables_find_the_spell_by_their_set() {
    let mut s = world();
    s.magic.known[0] = 1 << 9;
    s.magic.tables.words = vec![Vec::new(); 0x80];
    s.magic.tables.words[9] = b"IL".to_vec();
    s.magic.typed_mask = letter_bit(b'L') | letter_bit(b'I');
    let s = run(s, &[], |st, io| async move {
        let n = super::find_typed_spell(&st, &io).await;
        assert_eq!(n, 9);
        st.borrow_mut().magic.typed_mask = letter_bit(b'L');
        assert_eq!(super::find_typed_spell(&st, &io).await, -1);
    });
    assert_eq!(s.magic.typed_mask, 0);
    assert_eq!(s.ui.input.targeting, 1);
}

#[test]
fn book_cursor_steps_lines_and_pages() {
    let mut s = world();
    s.magic.page[..3].copy_from_slice(&[0, 1, 2]);
    s.magic.page_lines = 3;
    s.magic.current = 1;
    s.magic.next_page = 0xff;
    s.magic.prev_page = 2;
    let s = run(s, &[], |st, io| async move {
        let step = |d: i16| {
            st.borrow_mut().ui.input.direction = d;
        };
        step(2);
        assert_eq!(super::book_cursor_step(&st, &io).await, 0, "no next page");
        step(6);
        assert_eq!(super::book_cursor_step(&st, &io).await, 0xa3);
        assert_eq!(st.borrow().magic.page_moved, 1);
        step(4);
        assert_eq!(super::book_cursor_step(&st, &io).await, 0);
        assert_eq!(st.borrow().magic.current, 2);
        assert_eq!(super::book_cursor_step(&st, &io).await, 0);
        assert_eq!(st.borrow().magic.current, 2, "the last line stays");
        step(0);
        super::book_cursor_step(&st, &io).await;
        super::book_cursor_step(&st, &io).await;
        super::book_cursor_step(&st, &io).await;
    });
    assert_eq!(s.magic.current, 0, "the first line stays");
}

#[test]
fn type_checks() {
    assert!(can_vanish(0x17a));
    assert!(!can_vanish(0x19b) && !can_vanish(0x177) && !can_vanish(0x1aa));
    let mut s = world();
    s.magic.tables.doors = [0x12c, 0x12d, 0x12e, 0x12f];
    assert!(is_door(&s, 0x12e) && !is_door(&s, 0x62));
    s.tables.basetile.base = vec![0; 0x400];
    s.tables.basetile.base[0x113] = 0x200;
    s.magic.tables.magic_tiles = vec![0x203];
    let p = crate::objects::create_in(&mut s, 0x113 | 3 << 10, IN_INVENTORY, 0, 1);
    let q = crate::objects::create_in(&mut s, 0x113 | 2 << 10, IN_INVENTORY, 0, 1);
    assert!(magic_tile(&s, p) && !magic_tile(&s, q));
}

#[test]
fn dispel_clears_each_effect_and_reports_nothing_to_dispel() {
    let s = run(world(), &[], |st, io| async move {
        let mut s = st.borrow_mut();
        assert!(dispel(&mut s, &io, 1), "nothing set: no effect");
        s.actors.attrs.base_type_frame[1] = 0x17a;
        s.objects.type_frame[1] = 0x153;
        s.actors.attrs.status[1] = 0x04 | 0x08 | 0x02;
        assert!(!dispel(&mut s, &io, 1));
        assert_eq!(s.actors.attrs.status[1], 0);
        assert_eq!(s.objects.type_frame[1], 0x17a, "the sleeper's base type");
    });
    assert_eq!(s.actors.attrs.status[1], 0);
}

#[test]
fn heal_and_great_heal() {
    let mut s = world();
    s.actors.attrs.level[1] = 2;
    s.ui.input.selection = 1;
    s.actors.attrs.hp[1] = 10;
    s.rng = Rng::seeded(3);
    let mut r = s.rng;
    let roll = r.range(1, 0x1e) as u8;
    heal(&mut s, 6);
    assert_eq!(s.actors.attrs.hp[1], (10 + roll).min(60));
    assert_eq!(s.rng, r, "one draw");
    s.actors.attrs.hp[1] = 59;
    heal(&mut s, 6);
    assert_eq!(s.actors.attrs.hp[1], 60, "capped at 30 × level");
    s.actors.attrs.hp[1] = 1;
    heal(&mut s, 0x34);
    assert_eq!(s.actors.attrs.hp[1], 60);
    s.actors.attrs.status[1] = 0x10;
    s.actors.attrs.hp[1] = 1;
    heal(&mut s, 0x34);
    assert_eq!(
        (s.actors.attrs.hp[1], s.magic.result),
        (1, 2),
        "the dead: no effect"
    );
}

#[test]
fn poison_and_sleep_immunities_and_draws() {
    let mut s = world();
    s.objects.type_frame[2] = 0x160;
    s.objects.defs.monster_types = vec![0x160, 0];
    s.actors.game.creature_flags = vec![0x01 | 0x04];
    assert_eq!(poison(&mut s, 1, 2), 2);
    assert_eq!(sleep(&mut s, 1, 2), 2);
    assert_eq!(s.rng, Rng::new(), "immune: no draw");
    s.actors.game.creature_flags = vec![0];
    s.actors.attrs.intelligence[1] = 30;
    s.actors.attrs.intelligence[2] = 1;
    s.actors.attrs.strength[2] = 1;
    // Sleep: random 1..30 < 1 / 2 - 30 + 15 never holds.
    assert_eq!(sleep(&mut s, 1, 2), 0xfe);
    assert_eq!(s.actors.attrs.status[2] & 4, 4);
    // Poison: random 1..30 < (0 - 30 + 30) / 2 = 0 never holds.
    let before = s.rng;
    assert_eq!(poison(&mut s, 1, 2), 0xfe);
    assert_eq!(s.actors.attrs.status[2] & 8, 8);
    let mut r = before;
    r.next_word();
    assert_eq!(s.rng, r, "one draw");
}

#[test]
fn douse_and_ignite_light_sources() {
    let mut s = world();
    let c = s.map.centre;
    let torch = crate::objects::create_on_map(&mut s.objects, 0x5a, 0, 0, c.x + 1, c.y, 0);
    let keg = crate::objects::create_on_map(&mut s.objects, 0xdf, 0, 0, c.x + 2, c.y, 0);
    let rock = crate::objects::create_on_map(&mut s.objects, 0x30, 0, 0, c.x + 3, c.y, 0);
    let s = run(s, &[], move |st, io| async move {
        let fire = |o, spell| super::effects::douse_ignite(&st, &io, o, spell);
        fire(torch, 4).await;
        assert_eq!(st.borrow().magic.result, 2, "dousing an unlit torch");
        st.borrow_mut().magic.result = 0;
        fire(torch, 8).await;
        assert_eq!(st.borrow().objects.tf(torch), 0x5a | 1 << 10);
        assert_eq!(st.borrow().objects.status(torch) & 0x80, 0x80, "lit");
        fire(keg, 8).await;
        fire(rock, 8).await;
        assert_eq!(st.borrow().magic.result, 2, "not a light source");
        fire(torch, 4).await;
    });
    assert_eq!(
        s.objects.type_frame[torch as usize], 0,
        "a doused torch is gone"
    );
    assert_eq!(s.objects.tf(keg), 0xdf | 1 << 10);
    assert_eq!(s.actors.tail_word(0x5a), 3, "the keg timer");
    assert_eq!(s.actors.tail_word(0x5c), keg as u16);
}

#[test]
fn nearest_seen_breaks_ties_with_one_draw() {
    let mut s = world();
    for (a, x) in [(5u16, 102u16), (6, 98)] {
        s.objects.type_frame[usize::from(a)] = 0x178;
        s.objects.location[usize::from(a)] = crate::objects::pack_xyz(x, 100, 0);
    }
    s.objects.type_frame[7] = 0x178;
    s.objects.location[7] = crate::objects::pack_xyz(104, 100, 0);
    s.objects.type_frame[8] = 0x178;
    s.objects.location[8] = crate::objects::pack_xyz(105, 100, 0);
    let before = s.rng;
    let n = nearest_seen(&mut s, 1);
    let mut r = before;
    let pick6 = r.range(0, 1) == 0;
    assert_eq!(n, if pick6 { 6 } else { 5 });
    assert_eq!(
        s.rng, r,
        "one draw for the one tie; 105 is outside the view"
    );
    s.actors.attrs.status[5] = 0x10;
    s.actors.attrs.status[6] = 0x10;
    assert_eq!(nearest_seen(&mut s, 1), 7, "the dead are skipped");
}

#[test]
fn resurrect_puts_the_actor_on_the_first_free_ring_cell() {
    let mut s = world_with(&[(3, 0x153, 7, 7)]);
    s.actors.attrs.base_type_frame[3] = 0x178;
    s.actors.attrs.status[3] = 0x10 | 0x08;
    s.actors.attrs.exp[3] = 200;
    s.actors.karma = 50;
    let c = s.map.centre;
    let body = crate::objects::create_on_map(&mut s.objects, 0x153, 0, 3 << 8, c.x, c.y, 0);
    let a = super::resurrect(&mut s, body);
    assert_eq!(a, 3);
    assert_eq!(s.actors.attrs.status[3], 0);
    assert_eq!(s.actors.attrs.hp[3], 1);
    assert_eq!(s.objects.type_frame[3], 0x178);
    assert_eq!(s.actors.attrs.exp[3], 100, "karma 50: half the experience");
    let at = s.objects.location(crate::objects::ObjectId(3)).unwrap();
    // An empty map lets it stand anywhere: the first cell of ring 1 (the top edge from the
    // left) is taken.
    assert_eq!((at.x, at.y), (c.x - 1, c.y - 1));
    assert_eq!(s.objects.type_frame[body as usize], 0, "the body is gone");
}

#[test]
fn resist_and_dodge_checks_draw_one_number() {
    let mut s = world();
    s.rng = Rng::seeded(9);
    let mut r = s.rng;
    super::seams::resists(&mut s, 1, 1);
    r.next_word();
    assert_eq!(s.rng, r);
    super::effects::dodges_by_dexterity(&mut s, 1, 1);
    super::effects::dodges_by_strength(&mut s, 1, 1);
    r.next_word();
    r.next_word();
    assert_eq!(s.rng, r);
}

/// A caster with one book holding Light (9, circle 1, reagent 7) and the page drawn.
fn caster_with_light(s: &mut State) {
    s.magic.tables.reagents = [0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47];
    s.magic.tables.masks = vec![0; 0x80];
    s.magic.tables.masks[9] = 0x80;
    s.magic.tables.names = vec![Vec::new(); 0x80];
    s.magic.tables.words = vec![Vec::new(); 0x80];
    s.objects.defs.stackable = s.magic.tables.reagents.to_vec();
    book(s, &[9]);
    s.actors.attrs.level[1] = 1;
    s.actors.attrs.magic[1] = 5;
}

#[test]
fn cast_checks_level_points_and_reagents_in_order() {
    // Return in the book casts the current spell; without the reagent: refused, nothing spent.
    let mut s = world();
    caster_with_light(&mut s);
    let s = run(s, &[Key::Enter], |st, io| async move {
        st.borrow_mut().ui.input.targeting = 3;
        st.borrow_mut().ui.input.target_command = 0x82;
        st.borrow_mut().ui.input.target_pending = 1;
        super::cast(&st, &io).await;
    });
    assert_eq!(s.magic.current, 9);
    assert_eq!(s.actors.attrs.magic[1], 5, "no reagents: no points spent");
    assert_eq!(s.actors.timers[0], 0);
    // With the reagent: one ash and one point go, Light's timer runs.
    let mut s = world();
    caster_with_light(&mut s);
    crate::objects::create_in(&mut s, 0x47, IN_INVENTORY, 2, 1);
    let s = run(s, &[Key::Enter], |st, io| async move {
        st.borrow_mut().ui.input.targeting = 3;
        st.borrow_mut().ui.input.target_command = 0x82;
        st.borrow_mut().ui.input.target_pending = 1;
        super::cast(&st, &io).await;
    });
    assert_eq!(s.actors.attrs.magic[1], 4);
    assert_eq!(carried_count(&s.objects, 1, 0x47, 0), 1);
    assert_eq!(s.actors.timers[0], 100);
    assert_eq!(s.ui.panel.mode, s.magic.saved_panel, "the panel is back");
    // Level 0 below circle 1: refused first.
    let mut s = world();
    caster_with_light(&mut s);
    crate::objects::create_in(&mut s, 0x47, IN_INVENTORY, 2, 1);
    s.actors.attrs.level[1] = 0;
    let s = run(s, &[Key::Enter], |st, io| async move {
        st.borrow_mut().ui.input.targeting = 3;
        super::cast(&st, &io).await;
    });
    assert_eq!((s.actors.attrs.magic[1], s.actors.timers[0]), (5, 0));
}

#[test]
fn negate_magic_and_no_book_refuse() {
    let mut s = world();
    caster_with_light(&mut s);
    s.actors.timers[13] = 3;
    let s = run(s, &[], |st, io| async move { super::cast(&st, &io).await });
    assert_ne!(s.ui.panel.mode, super::BOOK_PANEL, "the book never opened");
    let s = run(
        world(),
        &[],
        |st, io| async move { super::cast(&st, &io).await },
    );
    assert_eq!(s.magic.books, [0, 0]);
}

#[test]
fn page_codes_turn_the_page() {
    let mut s = world();
    s.magic.known = [0x3ff, 0x3, 0, 0, 0, 0, 0, 0];
    s.magic.tables.names = vec![Vec::new(); 0x80];
    s.magic.page_first = 0;
    let s = run(s, &[], |st, io| async move {
        super::draw_book_page(&st, &io).await;
        assert_eq!(st.borrow().magic.next_page, 2);
        super::page_key(&st, &io, 0xa4).await;
        assert_eq!(st.borrow().magic.page_first, 8, "more of circle 1");
        super::page_key(&st, &io, 0xa4).await;
        assert_eq!(st.borrow().magic.page_first, 10, "circle 2");
        super::page_key(&st, &io, 0xa3).await;
        assert_eq!(st.borrow().magic.page_first, 8, "the last page of circle 1");
        st.borrow_mut().magic.page_moved = 1;
        super::page_key(&st, &io, 0xa3).await;
    });
    assert_eq!(s.magic.page_first, 0);
    assert_eq!(s.magic.current, 0, "a moved page selects its first line");
    assert_eq!(s.magic.page_lines, 8);
}

#[test]
fn wind_change_takes_the_last_direction_read() {
    // Not a direction: the no-direction word (0x115a8a), and the wind still takes the last
    // direction read (A4-0x2efc).
    let mut s = world();
    s.ui.input.direction = 5;
    let s = run(s, &[Key::Char('x')], |st, io| async move {
        super::spells::wind_change(&st, &io).await;
    });
    assert_eq!(s.actors.wind, 5);
}

#[test]
fn confuse_and_fear_act_on_the_first_object_of_each_tile() {
    let mut s = world_with(&[(5, 0x178, 101, 100), (6, 0x178, 99, 100)]);
    s.actors.attrs.status[5] = 0x20;
    s.actors.attrs.status[6] = 0x40;
    s.actors.attrs.intelligence[1] = 30;
    s.actors.attrs.intelligence[5] = 1;
    let s = run(s, &[], |st, io| async move {
        super::spells::all_foes(&st, &io, 0x52).await;
    });
    assert_eq!((s.actors.worktype[5], s.actors.attrs.status[5]), (8, 0));
    assert_eq!(s.actors.worktype[6], 0, "good actors are not confused");
    let mut s = world_with(&[(5, 0x178, 101, 100)]);
    s.actors.attrs.intelligence[1] = 30;
    s.actors.attrs.intelligence[5] = 1;
    let s = run(s, &[], |st, io| async move {
        super::spells::all_foes(&st, &io, 0x63).await;
    });
    assert_eq!(s.actors.worktype[5], 0x13, "Fear: flee");
}

#[test]
fn xray_shows_the_caster_flash_before_its_refreshes() {
    // 0x119200 calls the flash 0x132338 before it sets A4-0x794c and starts the 100 refreshes:
    // by the first display frame with every view tile shown, the flash and the window after
    // it are on the screen, none left for after the command.
    let s = world();
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        super::spells::xray(&st, &io, 0x49).await;
    });
    let mut at_reveal = None;
    for _ in 0..400 {
        e.tick(&[], 16);
        {
            let s = e.state();
            if at_reveal.is_none() && s.map.reveal_all == 1 {
                at_reveal = Some((s.combat.shown, s.combat.effects.len()));
            }
        }
        if e.finished() {
            break;
        }
    }
    assert!(e.finished());
    assert_eq!(at_reveal, Some((2, 0)), "(frames shown, frames left)");
}

#[test]
fn the_caster_flash_is_shown_before_the_spell_goes_on() {
    // Locate, Peer and the timed spells of 0x11a586 call the window flash 0x115b7c right after
    // the caster's 0x132338, and 0x115b7c draws straight to the screen: the caster's flash and
    // the window after it are on the screen when the flash returns.
    let got = Rc::new(RefCell::new(None));
    let out = Rc::clone(&got);
    run(world(), &[], move |st, io| async move {
        super::spells::flash_actor(&st, &io, 1, 0).await;
        let s = st.borrow();
        *out.borrow_mut() = Some((s.combat.shown, s.combat.effects.len()));
    });
    assert_eq!(*got.borrow(), Some((2, 0)), "(frames shown, frames left)");
}

//! Unit tests of the inventory code on synthetic data: tiles = 4 × type + frame, weights by
//! type, actor 1 on the map with its objects created inside it.

use std::cell::RefCell;
use std::rc::Rc;

use u6_formats::basetile::BaseTiles;
use u6_formats::tileflag::TileFlags;

use super::*;
use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::objects::{
    IN_CONTAINER, IN_INVENTORY, READIED, Slot, WHERE_MASK, create_in, init_slots,
};
use crate::state::State;

/// Object types of the synthetic world (tile = 4 × type).
const SWORD: u16 = 0x90; // tile 0x240: in the weapon-tile list
const SHIELD: u16 = 0x82; // tile 0x208
const HALBERD: u16 = 0x8a; // tile 0x228: two-handed
const RING: u16 = 0x96; // tile 0x258
const ARMOUR: u16 = 0x84; // tile 0x210
const BAG: u16 = 0x30;
const APPLE: u16 = 0x41; // weighed per ten
const ARROWS: u16 = 0x3c; // stackable
const GOLD: u16 = 0x58;
const TORCH: u16 = 0x5a;
const AMULET: u16 = 0x4c;
const ROCK: u16 = 0x31; // weight byte 0xff

pub(super) fn world() -> State {
    let mut s = State::empty();
    s.ui = crate::main_loop::test_support::test_ui();
    s.tables.basetile = BaseTiles {
        base: (0..1024).map(|t| t * 4).collect(),
    };
    let mut weight = vec![0u8; 1024];
    for (t, w) in [
        (SWORD, 30),
        (SHIELD, 40),
        (HALBERD, 60),
        (RING, 1),
        (ARMOUR, 100),
        (BAG, 5),
        (APPLE, 3),
        (ARROWS, 1),
        (GOLD, 1),
        (TORCH, 10),
        (AMULET, 2),
        (ROCK, 0xff),
    ] {
        weight[usize::from(t)] = w;
    }
    s.tables.tileflags = TileFlags {
        flags1: vec![0; 2048],
        flags2: vec![0; 2048],
        weight,
        flags3: vec![0; 2048],
    };
    s.objects.defs.stackable = vec![ARROWS, TORCH];
    s.objects.defs.nothing = "nothing".into();
    let t = &mut s.inventory.tables;
    t.hand_tiles = vec![0x240, 0x244];
    t.counted = vec![GOLD, APPLE];
    // Synthetic digits: 0 a checker (rows 101 010 101 010 101), 1 the left column (100 × 5).
    t.glyphs = vec![0xaaaa, 0x9248];
    // Synthetic columns, 4 per digit count: the units at 12, further digits 6, 4 or 3 apart.
    t.columns = vec![
        0, 0, 0, 0, 12, 0, 0, 0, 12, 6, 0, 0, 12, 8, 4, 0, 12, 9, 6, 3,
    ];
    init_slots(&mut s.objects);
    crate::objects::link_actors(&mut s.objects);
    let p = &mut s.actors.party;
    p.size = 2;
    p.members[..2].copy_from_slice(&[1, 2]);
    p.names = vec!["Ann".into(), "Bo".into()];
    s.actors.attrs.strength[1] = 20;
    s.actors.attrs.status[1] = 0x80;
    s.objects.type_frame[1] = 0x178;
    s.ui.panel.mode = 0x92;
    s
}

/// An object of type `ty` with `status` and quality / quantity `qq` inside `owner`.
pub(super) fn give(s: &mut State, ty: u16, status: u8, qq: u16, owner: Slot) -> Slot {
    let o = create_in(s, ty, status, qq, owner);
    assert!(o > 0, "slot for {ty:#x}");
    o
}

/// Runs `game` to its end (at most 40 ticks), `keys` queued on the first tick.
fn run<G, F>(s: State, keys: &[Key], game: G) -> Engine
where
    G: FnOnce(Rc<RefCell<State>>, Rc<Io>) -> F,
    F: std::future::Future<Output = ()> + 'static,
{
    let mut e = Engine::with_game(s, Files::default(), game);
    let ev: Vec<InputEvent> = keys.iter().map(|&k| InputEvent::Key(k)).collect();
    e.tick(&ev, 16);
    for _ in 0..40 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    e
}

#[test]
fn ready_slot_follows_the_tile_ranges() {
    let mut s = world();
    // (type, frame, slot): helm tile 0x200; boots 0x218 + 2; amulet 0x214 + 5 = 0x219.
    let cases = [
        (SWORD, 0, 2),
        (SHIELD, 0, 5),
        (HALBERD, 0, 8),
        (RING, 0, 9),
        (ARMOUR, 0, 4),
        (0x80, 0, 0),
        (0x86, 2, 7),
        (0x85, 5, 1),
        (BAG, 0, -1),
    ];
    for (ty, frame, want) in cases {
        let o = give(&mut s, ty | frame << 10, IN_INVENTORY, 1, 1);
        assert_eq!(ready_slot(&s, o), want, "type {ty:#x} frame {frame}");
    }
}

#[test]
fn object_weight_counts_stacks_and_tenths() {
    let mut s = world();
    let gold = give(&mut s, GOLD, IN_INVENTORY, 250, 1);
    let apples = give(&mut s, APPLE, IN_INVENTORY, 7, 1);
    let arrows = give(&mut s, ARROWS, IN_INVENTORY, 12, 1);
    let torch = give(&mut s, TORCH, IN_INVENTORY, 5, 1);
    let sword = give(&mut s, SWORD, IN_INVENTORY, 0, 1);
    let rock = give(&mut s, ROCK, IN_INVENTORY, 0, 1);
    // Gold: the word count, per ten (250 × 1 / 10).
    assert_eq!(object_weight(&s, gold), 25);
    // Apples are no stack: 1 × 3 / 10 = 0.
    assert_eq!(object_weight(&s, apples), 0);
    assert_eq!(object_weight(&s, arrows), 12);
    assert_eq!(object_weight(&s, torch), 50);
    s.objects.type_frame[torch as usize] |= 1 << 10;
    assert_eq!(object_weight(&s, torch), 10, "a lit torch counts one");
    assert_eq!(object_weight(&s, sword), 30);
    assert_eq!(object_weight(&s, rock), 10000);
    assert!(counted_type(&s, GOLD as i16) && !counted_type(&s, SWORD as i16));
}

#[test]
fn stones_round_half_up_and_show_any_weight() {
    assert_eq!(stones(0), 0);
    assert_eq!(stones(3), 1);
    assert_eq!(stones(14), 1);
    assert_eq!(stones(15), 2);
    assert_eq!(stones(200), 20);
}

#[test]
fn carried_weight_fills_the_equipment_and_counts_the_list() {
    let mut s = world();
    let halberd = give(&mut s, HALBERD, READIED, 0, 1);
    let _r1 = give(&mut s, RING, READIED, 0, 1);
    let _r2 = give(&mut s, RING, READIED, 0, 1);
    let bag = give(&mut s, BAG, IN_INVENTORY, 0, 1);
    let _inside = give(&mut s, ARROWS, IN_CONTAINER, 20, bag);
    let _sword = give(&mut s, SWORD, IN_INVENTORY, 0, 1);
    carried_weight(&mut s, 1);
    let inv = &s.inventory;
    assert_eq!(inv.equip_weight, 62);
    // The bag 5 with its 20 arrows, and the sword 30.
    assert_eq!(inv.carried_weight, 55);
    assert_eq!(s.ui.panel.inventory_count, 2);
    assert_eq!(inv.equip(2), halberd);
    assert_eq!(inv.equip(5), 1, "both hands taken");
    assert!(inv.equip(3) > 1 && inv.equip(6) > 1 && inv.equip(3) != inv.equip(6));
    assert!(s.ui.panel.member_flag(0));
    assert_eq!(contents_weight(&s, bag), 20);
}

#[test]
fn direction_to_reads_the_cells_around_the_origin() {
    let o = (0x58, 0x58);
    assert_eq!(direction_to(o, 0x58, 0x58), -1);
    assert_eq!(direction_to(o, 0x58, 0x48), 0);
    assert_eq!(direction_to(o, 0x68, 0x48), 1);
    assert_eq!(direction_to(o, 0x68, 0x58), 2);
    assert_eq!(direction_to(o, 0x48, 0x68), 5);
    // Farther: straight when one axis is at least twice the other, else the diagonal.
    assert_eq!(direction_to(o, 0x58 + 80, 0x58 + 30), 2);
    assert_eq!(direction_to(o, 0x58 - 50, 0x58 + 40), 5);
    assert_eq!(direction_to(o, 0x58, 0), 0);
}

#[test]
fn pointer_hit_tests_select_list_and_equipment_slots() {
    let mut s = world();
    s.inventory.cells[5] = 0x123;
    s.inventory.set_equip(5, 1);
    s.inventory.set_equip(2, 0x150);
    s.ui.input.pointer = (0xf8 + 16 + 3, 0x20 + 16 + 3);
    assert!(list_slot_at(&mut s));
    assert_eq!((s.ui.input.selection, s.ui.input.target.0), (0x123, 6));
    s.ui.input.pointer = (0xf8, 0x20);
    assert!(list_slot_at(&mut s));
    assert_eq!(s.ui.input.selection, -1, "empty cell");
    s.ui.input.pointer = (0xb2, 0x2a);
    assert!(equipment_slot_at(&mut s));
    assert_eq!((s.ui.input.selection, s.ui.input.target.0), (0x150, 2));
    s.ui.input.pointer = (0xe2, 0x2a);
    assert!(equipment_slot_at(&mut s));
    assert_eq!(
        s.ui.input.selection, -1,
        "the both-hands marker selects nothing"
    );
    s.ui.input.pointer = (0, 0);
    assert!(!equipment_slot_at(&mut s) && !list_slot_at(&mut s));
}

#[test]
fn ready_and_unready_move_a_weapon_and_change_the_weights() {
    let mut s = world();
    let sword = give(&mut s, SWORD, IN_INVENTORY, 0, 1);
    let _arrows = give(&mut s, ARROWS, IN_INVENTORY, 3, 1);
    carried_weight(&mut s, 1);
    assert_eq!(
        (s.inventory.equip_weight, s.inventory.carried_weight),
        (0, 33)
    );
    s.ui.input.selection = sword;
    let e = run(s, &[], |st, io| async move {
        ready(&st, &io).await;
    });
    let s = {
        let s = e.state();
        assert!(e.finished());
        assert_eq!(s.objects.place(sword), READIED);
        assert_eq!(s.inventory.equip(2), sword);
        assert_eq!(
            (s.inventory.equip_weight, s.inventory.carried_weight),
            (30, 3)
        );
        assert_eq!(s.ui.panel.inventory_count, 1);
        let mut s = s.clone();
        s.ui.input.selection = sword;
        s
    };
    let e = run(s, &[], |st, io| async move {
        unready(&st, &io).await;
    });
    let s = e.state();
    assert_eq!(s.objects.place(sword), IN_INVENTORY);
    assert_eq!(s.inventory.equip(2), 0);
    assert_eq!(
        (s.inventory.equip_weight, s.inventory.carried_weight),
        (0, 33)
    );
    assert_eq!(s.ui.panel.inventory_count, 2);
}

#[test]
fn ready_refuses_heavy_unreadyable_and_occupied() {
    let mut s = world();
    s.actors.attrs.strength[1] = 8; // 80 tenths; 70 readied already
    let armour = give(&mut s, ARMOUR, IN_INVENTORY, 0, 1);
    let bag = give(&mut s, BAG, IN_INVENTORY, 0, 1);
    let ring = give(&mut s, RING, IN_INVENTORY, 0, 1);
    let _shield = give(&mut s, SHIELD, READIED, 0, 1);
    let sword = give(&mut s, SWORD, READIED, 0, 1);
    let second = give(&mut s, SWORD, IN_INVENTORY, 0, 1);
    carried_weight(&mut s, 1);
    for (obj, why) in [
        (armour, "heavy"),
        (bag, "no slot"),
        (second, "no hand free"),
    ] {
        let mut t = s.clone();
        t.ui.input.selection = obj;
        let e = run(t, &[], |st, io| async move { ready(&st, &io).await });
        assert_eq!(e.state().objects.place(obj), IN_INVENTORY, "{why}");
    }
    s.ui.input.selection = ring;
    let e = run(s, &[], |st, io| async move { ready(&st, &io).await });
    assert_eq!(e.state().objects.place(ring), READIED);
    assert_eq!(e.state().inventory.equip(3), ring);
    assert_eq!(e.state().inventory.equip(2), sword);
}

#[test]
fn the_amulet_of_submission_stays_and_a_lit_torch_burns_out() {
    let mut s = world();
    let amulet = give(&mut s, AMULET, READIED, 0, 1);
    let torch = give(&mut s, TORCH | 1 << 10, READIED, 1, 1);
    carried_weight(&mut s, 1);
    let mut t = s.clone();
    t.ui.input.selection = amulet;
    let e = run(t, &[], |st, io| async move { unready(&st, &io).await });
    assert_eq!(e.state().objects.place(amulet), READIED);
    s.ui.input.selection = torch;
    let e = run(s, &[], |st, io| async move { unready(&st, &io).await });
    assert_eq!(e.state().objects.type_frame[torch as usize], 0, "removed");
}

#[test]
fn a_number_is_plotted_digit_by_digit_into_the_icon() {
    let mut s = world();
    s.ui.tables.colours.text = 6;
    let e = run(s, &[], |st, io| async move {
        tile_with_number(&mut st.borrow_mut(), &io, 0, 10, 32, 40);
        io.present();
    });
    let fb = e.frame();
    // 10 is two digits (columns 12, 6): the units (glyph 0, the checker) at 12, the tens
    // (glyph 1, the left column) at 6, rows 11..=15.
    let lit = |x: usize, y: usize| fb.get(32 + x, 40 + y) == Some(6);
    assert!(lit(12, 11) && !lit(13, 11) && lit(14, 11), "top row of 0");
    assert!(!lit(12, 12) && lit(13, 12) && !lit(14, 12));
    assert!(
        lit(12, 15) && !lit(13, 15) && lit(14, 15),
        "bottom row of 0"
    );
    assert!(
        (11..=15).all(|y| lit(6, y) && !lit(7, y) && !lit(8, y)),
        "the 1's column"
    );
    assert!(!lit(6, 10) && !lit(6, 16) && !lit(12, 10) && !lit(12, 16));
}

/// Runs `f` on a thread of its own and returns its answer; fails when it has not answered within
/// five seconds (the owner walks these tests guard spun forever) or when it panicked.
#[allow(clippy::disallowed_methods)] // a test thread, so a walk that never ends fails the test
fn ends<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(v) => v,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("{what}: the owner walk did not end")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("{what}: panicked"),
    }
}

/// A bag on the map with an object of type `held` inside: its location bytes, read as a parent
/// word, name the bag itself (x = the slot's low 10 bits, y = the rest).
fn bag_on_the_map(s: &mut State, held: u16) -> (Slot, Slot) {
    let bag = give(s, BAG, IN_INVENTORY, 0, 1);
    let sword = give(s, held, IN_CONTAINER, 0, bag);
    s.objects.obj_status[bag as usize] &= !WHERE_MASK;
    s.objects
        .set_xyz(bag, bag as u16 & 0x3ff, bag as u16 >> 10, 0);
    assert_eq!(
        s.objects.parent(bag),
        bag,
        "the bag's coordinates read as itself"
    );
    (bag, sword)
}

/// Two bags inside each other (only a broken save has this) with an object of type `held` in
/// the first.
fn bags_in_a_cycle(s: &mut State, held: u16) -> (Slot, Slot) {
    let a = give(s, BAG, IN_INVENTORY, 0, 1);
    let b = give(s, BAG, IN_CONTAINER, 0, a);
    let sword = give(s, held, IN_CONTAINER, 0, a);
    let st = &mut s.objects.obj_status[a as usize];
    *st = (*st & !WHERE_MASK) | IN_CONTAINER;
    s.objects.set_parent(a, b);
    (a, sword)
}

#[test]
fn the_weight_line_of_a_container_no_actor_holds_is_drawn() {
    for (case, setup) in [
        (
            "a bag on the map",
            bag_on_the_map as fn(&mut State, u16) -> (Slot, Slot),
        ),
        ("bags in a cycle", bags_in_a_cycle),
    ] {
        let (finished, listed) = ends(case, move || {
            let mut s = world();
            let (bag, sword) = setup(&mut s, SWORD);
            s.ui.panel.container = bag as u16;
            s.ui.panel.open_container = bag as u16;
            let e = run(s, &[], |st, io| async move { draw_list(&st, &io).await });
            (e.finished(), e.state().inventory.cells[0] == sword)
        });
        assert!(finished && listed, "{case}: the list is drawn");
    }
}

/// A one-hand sword and a two-handed halberd: the refusal comes before the both-hands arm, so
/// the halberd leaves no marker in slot 5 and no two-handed flag on the member.
#[test]
fn ready_refuses_an_object_no_actor_holds() {
    for (case, setup) in [
        (
            "a bag on the map",
            bag_on_the_map as fn(&mut State, u16) -> (Slot, Slot),
        ),
        ("bags in a cycle", bags_in_a_cycle),
    ] {
        for (weapon, held) in [("sword", SWORD), ("halberd", HALBERD)] {
            let (finished, place, hands, flag) = ends(case, move || {
                let mut s = world();
                let (_, obj) = setup(&mut s, held);
                carried_weight(&mut s, 1);
                s.ui.input.selection = obj;
                let e = run(s, &[], |st, io| async move { ready(&st, &io).await });
                let s = e.state();
                let inv = &s.inventory;
                let flag = s.ui.panel.member_flag(s.ui.panel.member);
                (
                    e.finished(),
                    s.objects.place(obj),
                    (inv.equip(2), inv.equip(5)),
                    flag,
                )
            });
            assert!(finished, "{case}, {weapon}: Ready ends");
            assert_eq!(
                (place, hands, flag),
                (IN_CONTAINER, (0, 0), false),
                "{case}, {weapon}: nothing readied, no both-hands marker or flag"
            );
        }
    }
}

/// The parent word of a container on the map with y bit 5 set (what 0x9f, back to the parent,
/// shows) is 0x8000 or more: the panel draws an object figure, no actor's.
#[test]
fn the_figure_of_a_container_word_past_0x8000_is_drawn() {
    let mut s = world();
    s.ui.panel.container = 0x8001;
    let e = run(s, &[], |st, io| async move { draw_list(&st, &io).await });
    assert!(e.finished());
    assert_eq!(e.state().inventory.cells, [0; CELLS]);
}

#[test]
fn the_holder_is_the_actor_at_the_end_of_the_chain() {
    let mut s = world();
    let outer = give(&mut s, BAG, IN_INVENTORY, 0, 1);
    let inner = give(&mut s, BAG, IN_CONTAINER, 0, outer);
    let sword = give(&mut s, SWORD, READIED, 0, 1);
    assert_eq!(weight::holder(&s, inner), Some(1), "two bags deep");
    assert_eq!(weight::holder(&s, sword), Some(1));
    assert_eq!(weight::holder(&s, 1), Some(1), "an actor holds itself");
    assert_eq!(weight::holder(&s, -1), None);
    let mut m = s.clone();
    let (bag, inside) = bag_on_the_map(&mut m, SWORD);
    assert_eq!(
        (weight::holder(&m, bag), weight::holder(&m, inside)),
        (None, None)
    );
    let (a, inside) = bags_in_a_cycle(&mut s, SWORD);
    assert_eq!(
        (weight::holder(&s, a), weight::holder(&s, inside)),
        (None, None)
    );
    // A parent word of 0x8000 or more (y bit 5 in a map location) ends the walk too.
    let st = &mut s.objects.obj_status[inside as usize];
    *st = (*st & !WHERE_MASK) | IN_CONTAINER;
    s.objects.set_parent(inside, -0x7fff);
    assert_eq!(weight::holder(&s, inside), None);
}

/// The ring of invisibility (0x102) on the ring tile, actor 1 at the view centre, the click
/// regions of the shipped `game` (map window, command bar, side panel, the screen) and the
/// inventory panel of member 0 (mode 0x92).
fn panel_world() -> (State, Slot) {
    use crate::ui::pointer::{PointerState, PointerTables};
    let mut s = world();
    s.tables.basetile.base[0x102] = 0x258;
    s.ui.pointer = PointerState::from_tables(PointerTables {
        regions: vec![
            [8, 8, 167, 167],
            [8, 176, 167, 191],
            [168, 8, 311, 103],
            [0, 0, 319, 199],
        ],
        shapes: Vec::new(),
        walk_cell: (-1, -1),
        shown: false,
    });
    s.ui.panel.member = 0;
    s.ui.panel.container = 1;
    s.main.reading = 1;
    let ring = give(&mut s, 0x102, IN_INVENTORY, 0, 1);
    (s, ring)
}

/// A click at (x, y) in the panel (press and release), then the key `a`, read by 0x10790a
/// (which runs Ready 0xa7 and Unready 0xa8 itself). The frames shown and still recorded when
/// the key comes back, and the actor's status.
fn click_then_key(s: State, x: i16, y: i16) -> (u64, usize, u8) {
    let got = Rc::new(RefCell::new(None));
    let out = Rc::clone(&got);
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        let k = crate::ui::wait::read_key(&st, &io).await;
        let s = st.borrow();
        *out.borrow_mut() = Some((k, s.combat.shown, s.combat.effects.len()));
    });
    let ev = |b| InputEvent::Pointer { x, y, buttons: b };
    e.tick(&[ev(1), ev(0)], 16);
    e.tick(&[InputEvent::Key(Key::Char('a'))], 16);
    for _ in 0..60 {
        if e.finished() {
            break;
        }
        e.tick(&[], 16);
    }
    let (k, shown, left) = got.borrow().expect("the key was read");
    assert_eq!(k, 0x41, "the key after the click");
    (shown, left, e.state().objects.obj_status[1])
}

#[test]
fn a_ring_readied_from_the_panel_flashes_before_the_next_key() {
    let (mut s, ring) = panel_world();
    s.inventory.cells[0] = ring;
    let (shown, left, status) = click_then_key(s, 0xf8 + 8, 0x20 + 8);
    assert_ne!(status & 2, 0, "Ready made the actor invisible");
    assert_eq!(
        (shown, left),
        (2, 0),
        "the flash and the window after it are shown, none left for after the command"
    );
}

#[test]
fn a_ring_taken_off_from_the_panel_flashes_before_the_next_key() {
    let (mut s, ring) = panel_world();
    s.objects.obj_status[ring as usize] =
        (s.objects.obj_status[ring as usize] & !WHERE_MASK) | READIED;
    s.objects.obj_status[1] |= 2;
    s.inventory.set_equip(3, ring);
    // Equipment slot 3 (the first ring): x 0xb0..0xc0, y 0x38..0x48.
    let (shown, left, status) = click_then_key(s, 0xb8, 0x40);
    assert_eq!(status & 2, 0, "Unready made the actor visible");
    assert_eq!(
        (shown, left),
        (2, 0),
        "the flash and the window after it are shown, none left for after the command"
    );
}

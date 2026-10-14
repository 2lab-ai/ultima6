//! Unit tests of the inventory code on synthetic data: tiles = 4 × type + frame, weights by
//! type, actor 1 on the map with its objects created inside it.

use std::cell::RefCell;
use std::rc::Rc;

use u6_formats::basetile::BaseTiles;
use u6_formats::tileflag::TileFlags;

use super::*;
use crate::engine::{Engine, InputEvent, Io, Key};
use crate::files::Files;
use crate::objects::{IN_CONTAINER, IN_INVENTORY, READIED, Slot, create_in, init_slots};
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
    t.glyphs = vec![0xf6de, 0x4924];
    t.columns = vec![
        3, 0, 0, 0, 7, 0, 0, 0, 9, 4, 0, 0, 11, 7, 3, 0, 13, 9, 5, 1,
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
    assert_eq!(s.ui.input.selection, -1, "the both-hands marker selects nothing");
    s.ui.input.pointer = (0, 0);
    assert!(!equipment_slot_at(&mut s) && !list_slot_at(&mut s));
}

#[test]
fn ready_and_unready_move_a_weapon_and_change_the_weights() {
    let mut s = world();
    let sword = give(&mut s, SWORD, IN_INVENTORY, 0, 1);
    let _arrows = give(&mut s, ARROWS, IN_INVENTORY, 3, 1);
    carried_weight(&mut s, 1);
    assert_eq!((s.inventory.equip_weight, s.inventory.carried_weight), (0, 33));
    s.ui.input.selection = sword;
    let e = run(s, &[], |st, io| async move {
        ready(&st, &io).await;
    });
    let s = {
        let s = e.state();
        assert!(e.finished());
        assert_eq!(s.objects.place(sword), READIED);
        assert_eq!(s.inventory.equip(2), sword);
        assert_eq!((s.inventory.equip_weight, s.inventory.carried_weight), (30, 3));
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
    assert_eq!((s.inventory.equip_weight, s.inventory.carried_weight), (0, 33));
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
    for (obj, why) in [(armour, "heavy"), (bag, "no slot"), (second, "no hand free")] {
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
    // "10" (two digits: columns 9, 4): units (glyph 0 = 0xf6de) at 9, tens (0x4924) at 4.
    let lit = |x: usize, y: usize| fb.get(32 + x, 40 + y) == Some(6);
    assert!(lit(9, 11) && lit(10, 11) && lit(11, 11), "top row of 0");
    assert!(lit(9, 12) && !lit(10, 12) && lit(11, 12));
    assert!(lit(5, 11) && !lit(4, 11) && !lit(6, 11), "the 1's stem");
    assert!(lit(5, 15) && !lit(5, 16) && !lit(5, 10));
}

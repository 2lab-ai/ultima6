//! The pointer on synthetic data: the engine's pointer queue, the click map and its region
//! handlers, the pointer shape and sprite, the tables read from a synthetic `game`.

use super::*;
use crate::engine::Pointer;
use crate::ui::click;
use crate::ui::pointer::{PointerState, PointerTables, Shape};

/// The regions of the shipped `game` (map window, command bar, side panel, the screen).
fn regions() -> Vec<[i16; 4]> {
    vec![
        [8, 8, 167, 167],
        [8, 176, 167, 191],
        [168, 8, 311, 103],
        [0, 0, 319, 199],
    ]
}

/// Ten shapes: shape n is n + 1 rows of value 1 in column 0, its top left at (-n, -1).
fn shapes() -> Vec<Shape> {
    (0..10)
        .map(|n| Shape {
            dx: -(n as i16),
            dy: -1,
            rows: vec![std::array::from_fn(|c| u8::from(c == 0)); n + 1],
        })
        .collect()
}

/// The synthetic ui state with the pointer tables, the pointer origin (88, 88), view centre
/// (100, 100).
fn st() -> State {
    let mut s = state();
    s.ui.pointer = PointerState::from_tables(PointerTables {
        regions: regions(),
        shapes: shapes(),
        walk_cell: (-1, -1),
        shown: false,
    });
    s.inventory.pointer_origin = (88, 88);
    s.map.centre.x = 100;
    s.map.centre.y = 100;
    s.actors.party.size = 4;
    s
}

/// The click at (x, y) with `buttons`, as a press (or a held button).
fn at(s: &mut State, x: i16, y: i16, buttons: u16, pressed: bool) {
    s.ui.input.pointer = (x, y);
    s.ui.pointer.buttons = buttons;
    s.ui.pointer.pressed = pressed;
}

/// Runs `click` on `s` (the click map is async only for the inventory's prints).
fn click_code(s: State) -> (u16, State) {
    let code = Rc::new(RefCell::new(0));
    let out = Rc::clone(&code);
    let e = run(s, &[], 4, move |st, io| async move {
        *out.borrow_mut() = click::click(&st, &io).await;
    });
    let c = *code.borrow();
    (c, e.state().clone())
}

#[test]
fn the_queue_hands_one_button_change_per_sample() {
    let ev = |x, y, b| InputEvent::Pointer { x, y, buttons: b };
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = Rc::clone(&seen);
    let mut e = Engine::with_game(
        State::empty(),
        Files::default(),
        move |_st, io| async move {
            for _ in 0..4 {
                out.borrow_mut().push((io.take_pointer(), io.key_ready()));
            }
            let k = io.next_key().await;
            out.borrow_mut().push((io.pointer(), k == Key::Char('k')));
        },
    );
    // Moves, a press, a drag, a release, then a key.
    e.tick(
        &[
            ev(5, 5, 0),
            ev(400, -3, 0),
            ev(10, 20, 1),
            ev(12, 22, 1),
            ev(12, 22, 0),
            InputEvent::Key(Key::Char('k')),
        ],
        16,
    );
    let p = |x, y, buttons| Pointer { x, y, buttons };
    assert_eq!(
        *seen.borrow(),
        [
            // The two moves together, clamped to the screen.
            (p(319, 0, 0), false),
            // The press, with the drag after it.
            (p(12, 22, 1), false),
            // The release; now the key is next.
            (p(12, 22, 0), true),
            // A key in front: nothing taken.
            (p(12, 22, 0), true),
            (p(12, 22, 0), true),
        ]
    );
    let mut e = Engine::with_game(State::empty(), Files::default(), |_st, io| async move {
        let _ = io.next_key().await;
    });
    e.tick(&[ev(7, 8, 2), InputEvent::Key(Key::Space)], 16);
    assert!(
        e.finished(),
        "a pointer event before a key does not block the key wait"
    );
    assert_eq!(e.io().pointer(), p(7, 8, 2));
}

#[test]
fn command_bar_left_gives_the_command_right_underlines_it() {
    let mut s = st();
    at(&mut s, 8 + 3 * 16 + 5, 180, 1, true);
    assert_eq!(click_code(s.clone()).0, 0x84, "the fourth icon: Look");
    at(&mut s, 8 + 3 * 16 + 5, 180, 1, false);
    assert_eq!(
        click_code(s.clone()).0,
        0,
        "a held button does nothing on the bar"
    );
    at(&mut s, 8 + 9 * 16, 191, 2, true);
    let (code, after) = click_code(s);
    assert_eq!(code, 0);
    assert_eq!(
        globals::last_command(&after.actors),
        0x8a,
        "underlined only"
    );
}

#[test]
fn map_clicks_walk_target_and_repeat() {
    let mut s = st();
    s.main.reading = 1;
    // South of the origin: walk south; the walk target is the clicked location.
    at(&mut s, 88, 104, 1, true);
    let (code, a) = click_code(s.clone());
    assert_eq!((code, a.ui.input.direction), (0x80, 4));
    assert_eq!(a.actors.work.mouse_target, (100, 101));
    assert_eq!(a.ui.pointer.walk_cell, (5, 6));
    // Held, last step blocked the same way: nothing; another direction walks.
    let mut b = a.clone();
    at(&mut b, 88, 104, 1, false);
    b.actors.work.step_blocked = 1;
    assert_eq!(click_code(b.clone()).0, 0);
    at(&mut b, 120, 88, 1, false);
    let (code, c) = click_code(b);
    assert_eq!((code, c.ui.input.direction), (0x80, 2));
    // The origin's own cell: the press passes, a hold does nothing.
    at(&mut s, 90, 90, 1, true);
    assert_eq!(click_code(s.clone()).0, 0x8f);
    at(&mut s, 90, 90, 1, false);
    assert_eq!(click_code(s.clone()).0, 0);
    // A target pending: the press confirms the cell.
    let mut t = s.clone();
    t.ui.input.target_pending = 1;
    at(&mut t, 24, 40, 1, true);
    let (code, t) = click_code(t);
    assert_eq!((code, t.ui.input.target), (0x8e, (96, 97)));
    // The right button with a command underlined repeats it at the pointer.
    globals::set_last_command(&mut s.actors, 0x84);
    at(&mut s, 40, 40, 2, true);
    assert_eq!(click_code(s.clone()).0, 0xa9);
    // Outside every window region: cancel.
    at(&mut s, 2, 195, 1, true);
    let (code, u) = click_code(s);
    assert_eq!((code, u.ui.input.target), (0x8f, (-1, -1)));
}

#[test]
fn a_held_walk_keeps_its_target_while_far() {
    let mut s = st();
    s.main.reading = 1;
    s.actors.work.mouse_target = (104, 96);
    s.ui.pointer.walk_cell = (9, 1);
    at(&mut s, 9 * 16 + 4, 16 + 4, 1, false);
    assert_eq!(click::walk_direction(&mut s), 1);
    assert_eq!(
        s.actors.work.mouse_target,
        (104, 96),
        "same cell, still far: kept"
    );
    // Not reading a command key: no target.
    s.main.reading = 0;
    click::walk_direction(&mut s);
    assert_eq!(s.actors.work.mouse_target, (-1, -1));
}

#[test]
fn panel_clicks_by_mode() {
    let mut s = st();
    // Roster: a row right of 0xc0 = stats, on the figures = inventory, arrows scroll.
    s.ui.panel.mode = 0x91;
    at(&mut s, 0xe0, 0x2c + 4, 1, true);
    let (code, a) = click_code(s.clone());
    assert_eq!((code, a.ui.input.selection), (0x90, 1));
    at(&mut s, 0xb8, 0x18 + 0x30, 1, true);
    assert_eq!(click_code(s.clone()).0, 0x92, "the fourth member's figure");
    at(&mut s, 0xb8, 0x18 + 0x40, 1, true);
    assert_eq!(click_code(s.clone()).0, 0, "no fifth member");
    s.ui.panel.roster_first = 1;
    at(&mut s, 0xa9, 0x20, 1, true);
    assert_eq!(click_code(s.clone()).0, 0x93);
    s.actors.party.size = 8;
    at(&mut s, 0xa9, 0x60, 1, true);
    assert_eq!(click_code(s.clone()).0, 0x94);
    // Stats: the bottom row only.
    s.ui.panel.mode = 0x90;
    for (x, code) in [
        (0xb4, 0x9a),
        (0xc4, 0x91),
        (0xd4, 0x92),
        (0xe4, 0x99),
        (0xf4, 0),
    ] {
        at(&mut s, x, 0x5c, 1, true);
        assert_eq!(click_code(s.clone()).0, code, "stats icon at {x:#x}");
    }
    at(&mut s, 0xc4, 0x50, 1, true);
    assert_eq!(click_code(s.clone()).0, 0);
    // A held button does nothing in a panel.
    at(&mut s, 0xc4, 0x5c, 1, false);
    assert_eq!(click_code(s).0, 0);
}

#[test]
fn spellbook_lines_and_tabs_need_the_magic_state() {
    let mut s = st();
    s.ui.panel.mode = 0xa1;
    at(&mut s, 0xc0, 0x18, 1, true);
    let (code, a) = click_code(s.clone());
    assert_eq!(
        (code, a.ui.input.selection, a.ui.input.target),
        (0, 1, (-1, -1))
    );
    at(&mut s, 0xb4, 0x5c, 1, true);
    assert_eq!(click_code(s).0, 0, "no tab without a page");
}

#[test]
fn inventory_clicks_scroll_go_back_and_step_the_combat_mode() {
    let mut s = st();
    s.ui.panel.mode = 0x92;
    s.ui.panel.inventory_first = 4;
    s.ui.panel.inventory_count = 20;
    for (x, y, code) in [
        (0xf4, 0x24, 0x95),
        (0xf4, 0x4c, 0x96),
        (0xf4, 0x30, 0),
        (0x114, 0x14, 0x9f),
    ] {
        at(&mut s, x, y, 1, true);
        assert_eq!(click_code(s.clone()).0, code, "({x:#x}, {y:#x})");
    }
    s.ui.panel.inventory_count = 16;
    at(&mut s, 0xf4, 0x4c, 1, true);
    assert_eq!(
        click_code(s.clone()).0,
        0,
        "12 or fewer left: no scroll down"
    );
    // The bottom row: the stats panel's icons, the second panel being the stats.
    at(&mut s, 0xd4, 0x5c, 1, true);
    assert_eq!(click_code(s.clone()).0, 0x90);
    // A target pending: the figure does not go back, it is the target (0x8e); Move waiting:
    // a list cell does nothing.
    let mut t = s.clone();
    t.ui.input.target_pending = 1;
    at(&mut t, 0x114, 0x14, 1, true);
    assert_eq!(click_code(t).0, 0x8e);
    let mut m = s.clone();
    m.ui.input.target_command = 0x87;
    at(&mut m, 0xfc, 0x24, 1, true);
    assert_eq!(click_code(m).0, 0);
    // The fifth icon steps a companion's combat mode, 8 → 2; in party combat it is their
    // worktype too.
    s.ui.panel.member = 1;
    s.actors.party.members[1] = 5;
    s.actors.attrs.combat_mode[5] = 8;
    s.actors.set_tail(crate::actors::tail::COMBAT_MODE, 1);
    at(&mut s, 0xf4, 0x5c, 1, true);
    let (code, a) = click_code(s.clone());
    assert_eq!(
        (code, a.actors.attrs.combat_mode[5], a.actors.worktype[5]),
        (0, 2, 2)
    );
    // In a conversation nothing in the panel acts.
    s.converse.active = true;
    at(&mut s, 0xf4, 0x24, 1, true);
    assert_eq!(click_code(s).0, 0);
}

#[test]
fn the_shape_follows_the_sample_and_the_sprite_its_offsets() {
    let mut s = st();
    let shape = |s: &mut State, x, y| {
        s.ui.pointer.at = (x, y);
        pointer::update_shape(s);
        s.ui.pointer.shape
    };
    assert_eq!(shape(&mut s, 200, 50), 0, "outside the map: the arrow");
    assert_eq!(
        shape(&mut s, 90, 90),
        1,
        "the origin's cell: the cross-hair"
    );
    assert_eq!(shape(&mut s, 88, 20), 2, "north");
    assert_eq!(shape(&mut s, 20, 20), 9, "north-west");
    s.ui.input.target_pending = 1;
    assert_eq!(
        shape(&mut s, 200, 50),
        1,
        "a target pending: the cross-hair"
    );
    // Hidden: the sprite sits on line 0xc9 (below the screen); shown: at the pointer.
    s.ui.pointer.shape = 3;
    let p = Pointer {
        x: 100,
        y: 60,
        buttons: 0,
    };
    let hidden = pointer::sprite(&s.ui.pointer, p).unwrap();
    assert_eq!((hidden.x, hidden.y, hidden.rows.len()), (97, 0xc8, 4));
    pointer::show(&mut s);
    let sp = pointer::vblank(&mut s, p).unwrap();
    assert_eq!((sp.x, sp.y), (100 - 1, 59), "shape 1 at the target pending");
    let mut fb = Framebuffer::new();
    sp.draw(&mut fb);
    assert_eq!(
        (fb.get(99, 59), fb.get(99, 60), fb.get(100, 59)),
        (Some(17), Some(17), Some(0))
    );
    // Off the screen edge: clipped.
    let edge = pointer::sprite(
        &s.ui.pointer,
        Pointer {
            x: 0,
            y: 199,
            buttons: 0,
        },
    )
    .unwrap();
    edge.draw(&mut fb);
    assert_eq!(fb.get(0, 198), Some(0), "column -1 is off the screen");
    assert_eq!(
        pointer::sprite(&PointerState::default(), p),
        None,
        "no tables, no sprite"
    );
}

#[test]
fn the_event_wait_takes_a_click_then_its_release() {
    let mut s = st();
    s.main.reading = 1;
    let codes = Rc::new(RefCell::new(Vec::new()));
    let out = Rc::clone(&codes);
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        for _ in 0..3 {
            let k = wait::read_key(&st, &io).await;
            out.borrow_mut().push(k);
        }
    });
    let ev = |x, y, b| InputEvent::Pointer { x, y, buttons: b };
    // The Talk icon, then a press held south of the Avatar (the walk repeats), then a key.
    e.tick(&[ev(44, 184, 1), ev(44, 184, 0), ev(88, 104, 1)], 16);
    for _ in 0..4 {
        e.tick(&[], 16);
    }
    assert_eq!(*codes.borrow(), [0x83, 0x80, 0x80]);
    assert_eq!(e.state().ui.pointer.press_code, 0x80);
    assert!(e.state().actors.work.mouse_walk == 1, "held");
}

#[test]
fn pointer_tables_from_a_synthetic_game() {
    // Hunk 28 (sprite data), hunk 33 (the A4 data), hunk 34 (the shape tables) at their bases.
    let mut h28 = vec![0u8; 0x83c];
    // Hunk 33 as long as the shipped one, so hunk 34 follows it at 0x161ec8.
    let mut h33 = vec![0u8; 0xe7d8];
    let mut h34 = vec![0u8; 0x668];
    let put = |h: &mut Vec<u8>, base: u32, addr: u32, b: &[u8]| {
        let o = (addr - base) as usize;
        h[o..o + b.len()].copy_from_slice(b);
    };
    let a4 = |off: i32| crate::image::A4.wrapping_add_signed(off);
    for (i, r) in regions().iter().enumerate() {
        let b: Vec<u8> = r.iter().flat_map(|v| v.to_be_bytes()).collect();
        put(&mut h33, 0x1536f0, a4(-0x79c2 + 8 * i as i32), &b);
    }
    put(&mut h33, 0x1536f0, a4(-0x79c6), &[0xff, 0xff, 0xff, 0xfe]);
    // Every shape: two rows, data at 0x1517f6: row 0 = plane A bit 15, row 1 = both planes bit 14.
    put(
        &mut h28,
        0x1512c8,
        0x1517f6 + 4,
        &[0x80, 0, 0, 0, 0x40, 0, 0x40, 0],
    );
    for i in 0..10u32 {
        put(
            &mut h34,
            0x161ec8,
            0x1624b2 + 4 * i,
            &0x1517f6u32.to_be_bytes(),
        );
        put(
            &mut h34,
            0x161ec8,
            0x1624da + 2 * i,
            &0x0200u16.to_be_bytes(),
        );
        put(
            &mut h34,
            0x161ec8,
            0x1624ee + 2 * i,
            &(-(i as i16)).to_be_bytes(),
        );
        put(&mut h34, 0x161ec8, 0x162502 + 2 * i, &(-2i16).to_be_bytes());
    }
    let game = crate::image::SyntheticGame::new()
        .place_at(28, 0x1512c8)
        .loaded(true, h28)
        .place_at(33, 0x1536f0)
        .loaded(true, h33)
        .loaded(false, h34)
        .build();
    let t = PointerTables::load(&game).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(t.regions, regions());
    assert_eq!(
        (t.walk_cell, t.shown, t.shapes.len()),
        ((-1, -2), false, 10)
    );
    let s = &t.shapes[4];
    assert_eq!((s.dx, s.dy, s.rows.len()), (-4, -2, 2));
    assert_eq!(
        (s.rows[0][0], s.rows[0][1], s.rows[1][1], s.rows[1][0]),
        (1, 0, 3, 0)
    );
    assert!(PointerTables::load(&[0; 8]).is_err());
}

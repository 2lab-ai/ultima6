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
fn spellbook_lines_and_tabs_read_the_magic_page() {
    let mut s = st();
    s.ui.panel.mode = 0xa1;
    s.magic.page_lines = 0;
    at(&mut s, 0xc0, 0x18, 1, true);
    let (code, a) = click_code(s.clone());
    assert_eq!(
        (code, a.ui.input.selection, a.ui.input.target),
        (0, 1, (-1, -1)),
        "line 1 of an empty page"
    );
    s.magic.page_lines = 3;
    let (code, a) = click_code(s.clone());
    assert_eq!((code, a.ui.input.selection), (0x8e, 1), "line 1 of three");
    at(&mut s, 0xc0, 0x38, 1, true);
    assert_eq!(click_code(s.clone()).0, 0, "line 5 of three");
    let m = &mut s.magic;
    (m.prev_circle, m.prev_page, m.next_page, m.next_circle) = (0xff, 0xff, 0xff, 0xff);
    for x in [0xb4, 0xc4, 0xd4, 0xe4] {
        at(&mut s, x, 0x5c, 1, true);
        assert_eq!(
            click_code(s.clone()).0,
            0,
            "no tab at {x:#x} without a page"
        );
    }
    (s.magic.prev_page, s.magic.next_circle) = (2, 3);
    at(&mut s, 0xc4, 0x5c, 1, true);
    let (code, a) = click_code(s.clone());
    assert_eq!(
        (code, a.ui.input.target),
        (0xa3, (-1, -1)),
        "the previous page"
    );
    at(&mut s, 0xe4, 0x5c, 1, true);
    assert_eq!(click_code(s.clone()).0, 0xa5, "the next circle");
    at(&mut s, 0xd4, 0x5c, 1, true);
    assert_eq!(click_code(s).0, 0, "no next page");
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
    // The Talk icon, then a press held south of the hero (the walk repeats a pace later).
    e.tick(&[ev(44, 184, 1), ev(44, 184, 0), ev(88, 104, 1)], 16);
    for _ in 0..HELD_WALK_MS_TEST.div_ceil(16) + 1 {
        e.tick(&[], 16);
    }
    assert_eq!(*codes.borrow(), [0x83, 0x80, 0x80]);
    assert_eq!(e.state().ui.pointer.press_code, 0x80);
    assert!(e.state().actors.work.mouse_walk == 1, "held");
}

/// The codes `reads` command-key reads give, each with the engine clock (ms) it came at, while
/// the press south of the hero (88, 104) is held `held_ms` and then released, at host ticks of
/// `tick_ms`; `after_ms` follow. The press goes in at the first tick. Whether the game finished
/// (all reads done).
fn held_walk(tick_ms: u32, reads: usize, held_ms: u32, after_ms: u32) -> (Vec<(u64, u16)>, bool) {
    held_walk_at(&[tick_ms], reads, held_ms, after_ms, |_| None)
}

/// As [`held_walk`], at host ticks that cycle through `ticks` (ms each): the press goes in at
/// the first tick, the release at the tick that ends exactly `held_ms` after it (that tick is
/// shortened when the cycle would pass it), so every tick pattern delivers the same hold. After
/// the n-th read (from 0) the game waits `step(n)` ms when that is `Some` (a step whose own waits
/// take that long).
fn held_walk_at(
    ticks: &[u32],
    reads: usize,
    held_ms: u32,
    after_ms: u32,
    step: fn(usize) -> Option<u32>,
) -> (Vec<(u64, u16)>, bool) {
    let up = InputEvent::Pointer {
        x: 88,
        y: 104,
        buttons: 0,
    };
    press_and_events(ticks, reads, &[(held_ms, up)], after_ms, step)
}

/// The press south of the hero (88, 104) at the first tick of `ticks` (cycled, ms each), then
/// each of `events` at the tick that ends exactly its ms after the press (that tick shortened
/// when the cycle would pass it; in order), then `after_ms`. The codes the command-key reads
/// gave, each with the engine clock (ms) it came at, and whether all `reads` were done; after
/// the n-th read the game waits `step(n)` ms when that is `Some`.
fn press_and_events(
    ticks: &[u32],
    reads: usize,
    events: &[(u32, InputEvent)],
    after_ms: u32,
    step: fn(usize) -> Option<u32>,
) -> (Vec<(u64, u16)>, bool) {
    let (codes, finished) = press_and_walk(ticks, reads, events, after_ms, step);
    let codes = codes.iter().map(|&(t, k, _)| (t, k)).collect();
    (codes, finished)
}

/// As [`press_and_events`], each code with the walk direction it set (A4-0x2efc).
fn press_and_walk(
    ticks: &[u32],
    reads: usize,
    events: &[(u32, InputEvent)],
    after_ms: u32,
    step: fn(usize) -> Option<u32>,
) -> (Vec<(u64, u16, i16)>, bool) {
    let mut s = st();
    s.main.reading = 1;
    let codes = Rc::new(RefCell::new(Vec::new()));
    let out = Rc::clone(&codes);
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        for n in 0..reads {
            let k = wait::read_key(&st, &io).await;
            let dir = st.borrow().ui.input.direction;
            out.borrow_mut().push((io.now_ms(), k, dir));
            if let Some(ms) = step(n) {
                io.sleep(ms).await;
            }
        }
    });
    let mut cycle = ticks.iter().copied().cycle();
    let mut tick = || cycle.next().unwrap_or(16);
    let press = InputEvent::Pointer {
        x: 88,
        y: 104,
        buttons: 1,
    };
    e.tick(&[press], tick());
    let pressed = e.io().now_ms();
    for &(at, ev) in events {
        let at = pressed + u64::from(at);
        loop {
            let (now, t) = (e.io().now_ms(), u64::from(tick()));
            if now + t >= at {
                e.tick(&[ev], u32::try_from(at - now).unwrap_or(u32::MAX));
                break;
            }
            e.tick(&[], t as u32);
        }
    }
    let end = e.io().now_ms() + u64::from(after_ms);
    while e.io().now_ms() < end {
        e.tick(&[], tick());
    }
    let c = codes.borrow().clone();
    (c, e.finished())
}

#[test]
fn a_held_walk_repeats_at_the_originals_pace() {
    let pace = u64::from(HELD_WALK_MS_TEST);
    // Held for 2.5 paces: the press, then a repeat one pace after each step, nothing after the
    // release.
    let hold = HELD_WALK_MS_TEST * 5 / 2;
    let (codes, finished) = held_walk(16, 12, hold, 2 * HELD_WALK_MS_TEST);
    let times: Vec<u64> = codes.iter().map(|&(t, _)| t).collect();
    assert!(
        codes.iter().all(|&(_, k)| k == 0x80),
        "walk codes: {codes:?}"
    );
    assert_eq!(times.len(), 3, "the press and two repeats: {times:?}");
    for (n, &t) in times.iter().enumerate() {
        let due = times[0] + pace * n as u64;
        assert!(
            t >= due && t < due + 16,
            "each step n paces after the press, to the tick: {times:?}"
        );
    }
    assert!(!finished, "the reads wait for input after the release");
    // A press and its release one tick apart, or held for less than a pace: one walk.
    for held in [16, HELD_WALK_MS_TEST - 32] {
        let (codes, _) = held_walk(16, 12, held, 2 * HELD_WALK_MS_TEST);
        assert_eq!(codes.len(), 1, "held {held} ms walks once: {codes:?}");
    }
}

/// The pace in ms as the test helpers take it.
const HELD_WALK_MS_TEST: u32 = crate::ui::target::HELD_WALK_MS;

/// The tick patterns the cadence tests run at (ms each, cycled): 16 and 8, 20 and 10 (a pace is
/// a whole number of them), and a 60 Hz host's 17, 17, 16.
const TICKS: [&[u32]; 5] = [&[16], &[8], &[20], &[10], &[17, 17, 16]];

#[test]
fn a_held_walk_counts_the_same_steps_at_any_host_tick() {
    // Spec 03 §1: the same input over the same time gives the same game at any tick size. Each
    // hold is delivered exactly (the release `hold` ms after the press at every pattern), and
    // step n comes exactly when n paces after the press is before the release. The holds sit on
    // and one tick either side of whole paces (1320, 1980; at 16 ms ticks, 1328 and 1984 put the
    // release in the tick that passes the due time) and run past ten paces (a repeat restarted
    // from the tick it woke at loses a step there: 672 ms a step at 16 ms, 664 at 8).
    let holds = [
        300, 1304, 1312, 1320, 1328, 1336, 1344, 1650, 1964, 1980, 1984, 1996, 6650, 10_000,
    ];
    for hold in holds {
        let steps: Vec<usize> = TICKS
            .iter()
            .map(|ticks| {
                held_walk_at(ticks, 64, hold, 2 * HELD_WALK_MS_TEST, |_| None)
                    .0
                    .len()
            })
            .collect();
        println!("held {hold} ms: steps at 16 / 8 / 20 / 10 / 17,17,16 ms ticks {steps:?}");
        let expect = 1 + (hold - 1) as usize / HELD_WALK_MS_TEST as usize;
        assert_eq!(steps, [expect; 5], "held {hold} ms");
    }
}

#[test]
fn every_hold_around_a_whole_pace_walks_by_the_hold_alone() {
    // Each millisecond within 20 ms of one to four paces, at every tick pattern: whichever tick
    // the release and the due time fall in, step n comes exactly when the press plus n paces is
    // before the release.
    let pace = HELD_WALK_MS_TEST;
    let mut wrong = Vec::new();
    for hold in (1..=4).flat_map(|k| k * pace - 20..=k * pace + 20) {
        let expect = 1 + (hold - 1) as usize / pace as usize;
        for ticks in TICKS {
            let got = held_walk_at(ticks, 16, hold, pace, |_| None).0.len();
            if got != expect {
                wrong.push((hold, ticks, got, expect));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "(hold, ticks, steps, expected): {wrong:?}"
    );
}

#[test]
fn a_move_before_the_release_does_not_hide_it_from_the_due_sample() {
    // The button stays down while the pointer moves within the same cell 668 ms after the press
    // (a held move: the web host queues it apart from the release), then the release comes 20
    // ms before the second repeat's due time (1320 ms), or 20 ms after it. Whether the move
    // came in the tick that woke the first repeat or is still queued behind it at the due
    // sample, that sample sees the release behind it: two steps before the due time, three after.
    let ev = |y, b| InputEvent::Pointer {
        x: 88,
        y,
        buttons: b,
    };
    for (release, expect) in [(1300u32, 2usize), (1340, 3)] {
        let steps: Vec<usize> = TICKS
            .iter()
            .map(|ticks| {
                let events = [(668, ev(105, 1)), (release, ev(105, 0))];
                press_and_events(ticks, 64, &events, 2 * HELD_WALK_MS_TEST, |_| None)
                    .0
                    .len()
            })
            .collect();
        println!(
            "move at 668, release at {release}: steps at 16 / 8 / 20 / 10 / 17,17,16 ms ticks {steps:?}"
        );
        assert_eq!(steps, [expect; 5], "release at {release} ms");
    }
    // Holds within 24 ms of one and two paces, a held move 1 or 9 ms before the release, or
    // half a pace or a pace after the press: the steps follow the hold alone.
    let pace = HELD_WALK_MS_TEST;
    let mut wrong = Vec::new();
    for hold in (1..=2).flat_map(|k| (k * pace - 24..=k * pace + 24).step_by(4)) {
        let expect = 1 + (hold - 1) as usize / pace as usize;
        for at in [hold - 1, hold - 9, pace / 2, pace + 8] {
            if at >= hold {
                continue;
            }
            for ticks in TICKS {
                let events = [(at, ev(105, 1)), (hold, ev(105, 0))];
                let got = press_and_events(ticks, 16, &events, pace, |_| None).0.len();
                if got != expect {
                    wrong.push((hold, at, ticks, got, expect));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "(hold, move, ticks, steps, expected): {wrong:?}"
    );
}

#[test]
fn a_due_sample_walks_where_the_pointer_was_at_its_due_time() {
    // Held south of the hero, the pointer moves within its cell 650 ms after the press, then
    // into the cell east of the hero at 662 ms, past the first repeat's due time (660 ms). At
    // 10 ms ticks a tick lands on 660, before the second move; at the others the tick that
    // passes 660 queues it, behind the first move. Either way the repeat walks south, where the
    // pointer was at 660, and the next one east.
    let ev = |x, y, b| InputEvent::Pointer { x, y, buttons: b };
    let events = [
        (650, ev(88, 105, 1)),
        (662, ev(120, 88, 1)),
        (1500, ev(120, 88, 0)),
    ];
    let walks: Vec<Vec<(u16, i16)>> = TICKS
        .iter()
        .map(|ticks| {
            let (codes, _) = press_and_walk(ticks, 16, &events, HELD_WALK_MS_TEST, |_| None);
            codes.iter().map(|&(_, k, dir)| (k, dir)).collect()
        })
        .collect();
    println!("(code, direction) at 16 / 8 / 20 / 10 / 17,17,16 ms ticks: {walks:?}");
    for (ticks, walk) in TICKS.iter().zip(&walks) {
        assert_eq!(walk, &[(0x80, 4), (0x80, 4), (0x80, 2)], "ticks {ticks:?}");
    }
    // The move east at each millisecond from 655 to 670 ms, behind a move at 600 or 656: east
    // from the first repeat on when it came by 660, else from the second.
    let mut wrong = Vec::new();
    for first in [600, 656] {
        for east in (655..=670).filter(|&m| m > first) {
            let events = [
                (first, ev(88, 105, 1)),
                (east, ev(120, 88, 1)),
                (1500, ev(120, 88, 0)),
            ];
            let expect = if east <= HELD_WALK_MS_TEST { 2 } else { 4 };
            for ticks in TICKS {
                let (codes, _) = press_and_walk(ticks, 16, &events, HELD_WALK_MS_TEST, |_| None);
                let dirs: Vec<i16> = codes.iter().map(|&(_, _, dir)| dir).collect();
                if dirs != [4, expect, 2] {
                    wrong.push((first, east, ticks, dirs));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "(first move, east move, ticks, directions): {wrong:?}"
    );
}

#[test]
fn a_bounded_take_stops_at_its_count_or_a_button_change() {
    let ev = |x, y, b| InputEvent::Pointer { x, y, buttons: b };
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = Rc::clone(&seen);
    let mut e = Engine::with_game(
        State::empty(),
        Files::default(),
        move |_st, io| async move {
            for n in [0, 1, 5, 5, 5] {
                out.borrow_mut()
                    .push((io.take_pointer_upto(n), io.pending_events()));
            }
        },
    );
    // Three held moves, a release, a move, then a key.
    e.tick(
        &[
            ev(1, 1, 1),
            ev(2, 2, 1),
            ev(3, 3, 1),
            ev(4, 4, 0),
            ev(5, 5, 0),
            InputEvent::Key(Key::Char('k')),
        ],
        16,
    );
    let p = |x, y, buttons| Pointer { x, y, buttons };
    assert_eq!(
        *seen.borrow(),
        [
            // None taken.
            (p(0, 0, 0), 6),
            // Only the first move.
            (p(1, 1, 1), 5),
            // The other moves; stops at the release.
            (p(3, 3, 1), 3),
            // The release and the move after it; stops at the key.
            (p(5, 5, 0), 1),
            // A key in front: nothing.
            (p(5, 5, 0), 1),
        ]
    );
}

#[test]
fn a_key_in_the_tick_that_passes_a_due_time_comes_after_its_step() {
    // A key typed 672 ms after the press, while the button is still down: at 16 ms ticks it
    // arrives in the tick that passes the first repeat's due time (660 ms), at 8 ms ticks a tick
    // after it. Either way the repeat samples at 660 ms, before the key: the step, then the key.
    let ev = |b| InputEvent::Pointer {
        x: 88,
        y: 104,
        buttons: b,
    };
    let seqs: Vec<Vec<u16>> = [16u32, 8]
        .iter()
        .map(|&tick| {
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
            e.tick(&[ev(1)], tick);
            let press = e.io().now_ms();
            let key = [InputEvent::Key(Key::Char('a'))];
            while !e.finished() && e.io().now_ms() < press + 3000 {
                let typed = e.io().now_ms() + u64::from(tick) == press + 672;
                e.tick(if typed { &key } else { &[] }, tick);
            }
            let c = codes.borrow().clone();
            println!("{tick} ms ticks: codes {c:?}");
            c
        })
        .collect();
    assert_eq!(seqs[0], [0x80, 0x80, 0x41], "16 ms ticks");
    assert_eq!(seqs[1], seqs[0], "8 ms ticks");
}

#[test]
fn a_held_walk_samples_whole_paces_after_the_press() {
    // The n-th repeat is due n paces after the press, on the engine clock: a host tick only
    // rounds each wake up to the tick at or after it, and that rounding never adds up.
    let pace = u64::from(HELD_WALK_MS_TEST);
    for ticks in TICKS {
        let (codes, _) = held_walk_at(ticks, 64, 10_000, 2 * HELD_WALK_MS_TEST, |_| None);
        let press = codes[0].0;
        let longest = u64::from(*ticks.iter().max().unwrap_or(&16));
        let late: Vec<u64> = codes
            .iter()
            .enumerate()
            .map(|(n, &(t, _))| t - (press + pace * n as u64).min(t))
            .collect();
        println!(
            "ticks {ticks:?}: {} steps in 10 s, ms after each due time {late:?}",
            codes.len()
        );
        for (n, &(t, code)) in codes.iter().enumerate() {
            let due = press + pace * n as u64;
            assert!(
                code == 0x80 && t >= due && t < due + longest,
                "ticks {ticks:?}: step {n} at {t} ms, due at {due}: {codes:?}"
            );
        }
        assert_eq!(codes.len(), 16, "ticks {ticks:?}: steps in 10 s");
    }
}

#[test]
fn a_step_that_waits_keeps_the_cadence_or_samples_once_it_is_done() {
    // The step after the second code waits itself. 100 ms counts toward its pace, and so does
    // 640 ms, which ends within a 16 ms tick before the next due time: every step still comes a
    // whole number of paces after the press. 1.5 paces outlasts it: the next sample comes on the
    // next frame after the wait, then a pace apart (no catch-up burst).
    let pace = u64::from(HELD_WALK_MS_TEST);
    fn short(n: usize) -> Option<u32> {
        (n == 1).then_some(100)
    }
    fn nearly(n: usize) -> Option<u32> {
        (n == 1).then_some(640)
    }
    fn long(n: usize) -> Option<u32> {
        (n == 1).then_some(HELD_WALK_MS_TEST * 3 / 2)
    }
    for tick in [16u32, 8] {
        let tk = u64::from(tick);
        for (wait, step) in [(100, short as fn(usize) -> Option<u32>), (640, nearly)] {
            let (codes, _) = held_walk_at(&[tick], 64, 6000, 2 * HELD_WALK_MS_TEST, step);
            let times: Vec<u64> = codes.iter().map(|&(t, _)| t).collect();
            println!("{tick} ms ticks, the second step waits {wait} ms: steps at {times:?}");
            assert!(codes.iter().all(|&(_, k)| k == 0x80), "{codes:?}");
            assert_eq!(times.len(), 10, "{times:?}");
            for (n, &t) in times.iter().enumerate() {
                let due = times[0] + pace * n as u64;
                assert!(t >= due && t < due + tk, "step {n} due at {due}: {times:?}");
            }
        }
        let (codes, _) = held_walk_at(&[tick], 64, 6000, 2 * HELD_WALK_MS_TEST, long);
        let times: Vec<u64> = codes.iter().map(|&(t, _)| t).collect();
        println!("{tick} ms ticks, the second step waits 990 ms: steps at {times:?}");
        assert!(codes.iter().all(|&(_, k)| k == 0x80), "{codes:?}");
        let done = times[1] + u64::from(HELD_WALK_MS_TEST * 3 / 2);
        assert!(
            times[2] > done && times[2] <= done + 2 * tk,
            "the sample after the long step comes on the next frame: {times:?}"
        );
        for (n, &t) in times.iter().enumerate().skip(2) {
            let due = times[2] + pace * (n as u64 - 2);
            assert!(t >= due && t < due + tk, "a pace apart from it: {times:?}");
        }
        assert!(times.len() >= 6, "{times:?}");
    }
}

/// What [`walk_and_prompt`] saw: the codes the command loop's reads gave, each with the engine
/// clock (ms) it came at; the time the prompt ended and how many codes it dropped; the state.
struct Walked {
    codes: Vec<(u64, u16)>,
    prompt: Option<(u64, usize)>,
    state: State,
}

/// The command loop's reads (A4-0x7a2c set) at 16 ms ticks: `events[t]` go in on tick t. When
/// `prompt` is set, after the second code a loop shaped like the y/n prompt (A4-0x7a2c clear)
/// reads until `A`; its reads are not listed.
fn walk_and_prompt(events: &[&[InputEvent]], ticks: usize, prompt: bool) -> Walked {
    let mut s = st();
    s.main.reading = 1;
    let codes = Rc::new(RefCell::new(Vec::new()));
    let ended = Rc::new(RefCell::new(None));
    let (out, end) = (Rc::clone(&codes), Rc::clone(&ended));
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        for n in 0..64 {
            st.borrow_mut().main.reading = 1;
            let k = wait::read_key(&st, &io).await;
            out.borrow_mut().push((io.now_ms(), k));
            if prompt && n == 1 {
                st.borrow_mut().main.reading = 0;
                let mut dropped = 0;
                while wait::read_key(&st, &io).await != 0x41 {
                    dropped += 1;
                }
                *end.borrow_mut() = Some((io.now_ms(), dropped));
            }
        }
    });
    for t in 0..ticks {
        e.tick(events.get(t).copied().unwrap_or(&[]), 16);
    }
    let codes = codes.borrow().clone();
    let prompt = *ended.borrow();
    Walked {
        codes,
        prompt,
        state: e.state().clone(),
    }
}

#[test]
fn a_hold_that_gives_no_walk_keeps_the_frame_pace() {
    // A walk south, then the pointer dragged onto the hero's own cell with the button still
    // down: the click map gives nothing there (0x1065f8, not a press), so no step is paced. Once
    // the pace from the last step is over, every pass of the event wait is a frame again: the
    // key poll spins its cursor, a key typed is read at once, the release ends the hold.
    let ev = |x, y, b| InputEvent::Pointer { x, y, buttons: b };
    let (press, drag) = ([ev(88, 104, 1)], [ev(90, 90, 1)]);
    let pace_ticks = HELD_WALK_MS_TEST.div_ceil(16) as usize + 2;
    let key = [InputEvent::Key(Key::Char('a'))];
    let release = [ev(90, 90, 0)];
    let mut events: Vec<&[InputEvent]> = vec![&press, &drag];
    events.resize(pace_ticks, &[]);
    let spin_from = events.len();
    events.resize(spin_from + 20, &[]);
    let before = walk_and_prompt(&events, spin_from, false).state;
    let spun = walk_and_prompt(&events, spin_from + 20, false).state;
    let frames = spun
        .ui
        .input
        .cursor_frame
        .wrapping_sub(before.ui.input.cursor_frame);
    println!("held on the own cell: {frames} cursor frames in 20 ticks");
    assert!(
        frames >= 8,
        "the key poll runs a frame a pass: {frames} in 20 ticks"
    );
    let typed = events.len();
    events.push(&key);
    let codes = walk_and_prompt(&events, typed + 3, false).codes;
    let at = 16 * (typed as u64 + 1);
    println!("codes {codes:?}, the key went in at {at} ms");
    assert_eq!(codes.first().map(|c| c.1), Some(0x80), "the press walks");
    assert!(
        codes.iter().any(|&(t, k)| k == 0x41 && t <= at + 32),
        "the key is read within two frames: {codes:?}"
    );
    let released = events.len();
    events.push(&release);
    let after = walk_and_prompt(&events, released + 1, false).state;
    assert_eq!(
        after.actors.work.mouse_walk, 0,
        "the release is seen at its tick"
    );
}

#[test]
fn a_walk_after_a_prompt_keeps_its_pace_without_a_burst() {
    // The second step asks a question (the y/n prompt reads with A4-0x7a2c clear) while the
    // button stays down. In the prompt the held repeat runs a frame a pass and the prompt drops
    // its walk codes. Answered within the pace, the next step comes when it was due, a pace after
    // the second; answered later, on the next frame after the answer. Then a pace apart.
    let ev = |b| InputEvent::Pointer {
        x: 88,
        y: 104,
        buttons: b,
    };
    let key = [InputEvent::Key(Key::Char('a'))];
    let press = [ev(1)];
    let pace = u64::from(HELD_WALK_MS_TEST);
    let second = (HELD_WALK_MS_TEST / 16) as usize + 2;
    for answer in [
        second + 6,
        second + 3 * (HELD_WALK_MS_TEST / 16) as usize / 2,
    ] {
        let mut events: Vec<&[InputEvent]> = vec![&press];
        events.resize(answer, &[]);
        events.push(&key);
        let Walked { codes, prompt, .. } = walk_and_prompt(&events, answer + 4 * second, true);
        let (ended, dropped) = prompt.expect("the prompt got its key");
        let times: Vec<u64> = codes.iter().map(|&(t, _)| t).collect();
        println!(
            "prompt answered at {ended} ms ({dropped} walk codes dropped): steps at {times:?}"
        );
        assert!(codes.iter().all(|&(_, k)| k == 0x80), "{codes:?}");
        assert!(
            dropped >= 3,
            "a frame a pass in the prompt: {dropped} reads"
        );
        assert!(
            times.len() >= 4,
            "the walk goes on after the prompt: {times:?}"
        );
        let due = times[0] + 2 * pace;
        if ended < due {
            assert!(
                times[2] >= due && times[2] < due + 16,
                "the step due a pace after the second: {times:?}, the prompt ended at {ended}"
            );
        } else {
            assert!(
                times[2] > ended && times[2] <= ended + 32,
                "the next frame after the answer: {times:?}, the prompt ended at {ended}"
            );
        }
        // Each wake rounds its due time up to a tick: steps are a pace apart to the tick.
        for w in times.windows(2) {
            let gap = w[1] - w[0];
            assert!(gap + 16 > pace, "no two steps within a pace: {times:?}");
        }
        for w in times[2..].windows(2) {
            assert!(w[1] - w[0] < pace + 16, "a pace apart: {times:?}");
        }
    }
}

/// A loop shaped like the y/n and more prompts (0x10790a until one of its keys, here `A`) with
/// a press held on the map: `events[t]` go in on tick t; `reading` = A4-0x7a2c (clear in a
/// prompt, set while the command loop reads, where the walk repeat is paced). The key it read
/// and how many reads, or `None` while it still waits; the loop gives up after 64 reads.
fn prompt_under_held_press(
    events: &[&[InputEvent]],
    ticks: usize,
    reading: u16,
) -> Option<(u16, usize)> {
    let mut s = st();
    s.main.reading = reading;
    let got = Rc::new(RefCell::new(None));
    let out = Rc::clone(&got);
    let mut e = Engine::with_game(s, Files::default(), move |st, io| async move {
        let mut n = 0;
        let k = loop {
            let k = wait::read_key(&st, &io).await;
            n += 1;
            if k == 0x41 || n == 64 {
                break k;
            }
        };
        *out.borrow_mut() = Some((k, n));
    });
    for t in 0..ticks {
        e.tick(events.get(t).copied().unwrap_or(&[]), 16);
    }
    *got.borrow()
}

#[test]
fn a_prompt_survives_a_held_map_press() {
    let ev = |b| InputEvent::Pointer {
        x: 88,
        y: 104,
        buttons: b,
    };
    let key = InputEvent::Key(Key::Char('a'));
    // Room for one paced repeat of the command loop's walk.
    let ticks = (HELD_WALK_MS_TEST / 16) as usize + 12;
    for reading in [0, 1] {
        // Held for five ticks, released, then the key: the prompt reads it.
        let (press, release, k) = ([ev(1)], [ev(0)], [key]);
        let events: [&[InputEvent]; 8] = [&press, &[], &[], &[], &[], &release, &[], &k];
        let got = prompt_under_held_press(&events, ticks, reading);
        assert!(
            matches!(got, Some((0x41, n)) if n <= 8),
            "reading {reading}: the prompt got its key after the release: {got:?}"
        );
        // The key typed while the button is still down, then the release behind it in the
        // queue: the key is read (the release cannot be taken before it).
        let events: [&[InputEvent]; 6] = [&press, &[], &k, &[], &release, &[]];
        let got = prompt_under_held_press(&events, ticks, reading);
        assert!(
            matches!(got, Some((0x41, n)) if n <= 8),
            "reading {reading}: the prompt got the key typed during the hold: {got:?}"
        );
    }
}

/// The inventory panel showing `container`; the back icon (0x114, 0x14) clicked and released,
/// read by 0x10790a (which handles 0x9f itself). The container shown afterwards.
fn back_from(s: State, container: u16) -> u16 {
    let mut s = s;
    s.ui.panel.mode = 0x92;
    s.ui.panel.container = container;
    // Not dirty: no full panel redraw (0x111c30 starts again from the actor) after the code.
    s.ui.panel.dirty = false;
    let mut e = Engine::with_game(s, Files::default(), |st, io| async move {
        let _ = wait::read_key(&st, &io).await;
    });
    let ev = |b| InputEvent::Pointer {
        x: 0x114,
        y: 0x14,
        buttons: b,
    };
    e.tick(&[ev(1), ev(0)], 16);
    for _ in 0..4 {
        e.tick(&[], 16);
    }
    e.state().ui.panel.container
}

#[test]
fn back_from_a_container_goes_to_its_parent_or_from_the_map_to_the_actor() {
    let mut s = st();
    s.actors.party.members[0] = 1;
    // A bag (slot 0x150) inside a chest (slot 0x9d2): the location word holds the chest.
    let (bag, chest) = (0x150usize, 0x9d2u16);
    s.objects.obj_status[bag] = crate::objects::IN_CONTAINER;
    s.objects.location[bag].x = chest & 0x3ff;
    s.objects.location[bag].y = chest >> 10;
    assert_eq!(
        back_from(s.clone(), bag as u16),
        chest,
        "inside: its container"
    );
    // The same bag lying on the map: the location word is its coordinates, not a slot; back
    // goes to the panel's actor.
    s.objects.obj_status[bag] = 0;
    s.objects.location[bag].x = 0x123;
    s.objects.location[bag].y = 0x2c5;
    assert_eq!(
        back_from(s.clone(), bag as u16),
        1,
        "on the map: the member's actor"
    );
    // An open container (an actor not in the party) is the panel's actor then.
    s.ui.panel.open_container = 0x40;
    assert_eq!(back_from(s, bag as u16), 0x40, "on the map: the open actor");
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

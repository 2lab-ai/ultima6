//! The raw-key map 0x151d00, the key bridge and the key read.

use super::*;
use crate::ui::input::{Qualifiers, key_code, map_raw, raw_events};
use crate::ui::target::LEADER_CELL;

#[test]
fn raw_map_takes_the_column_of_the_held_qualifier() {
    let t = tables().keymap;
    let mut q = Qualifiers::default();
    assert_eq!(map_raw(&mut q, &t, 0x20), 0x61);
    // Releases give nothing; a qualifier press gives nothing and is remembered.
    assert_eq!(map_raw(&mut q, &t, 0x20 | 0x80), 0);
    assert_eq!(map_raw(&mut q, &t, 0x60), 0);
    assert_eq!(map_raw(&mut q, &t, 0x20), 0x41);
    // Control wins over shift; alt is used only without both.
    assert_eq!(map_raw(&mut q, &t, 0x63), 0);
    assert_eq!(map_raw(&mut q, &t, 0x20), 0x01);
    for raw in [0x63 | 0x80, 0x60 | 0x80, 0x64] {
        assert_eq!(map_raw(&mut q, &t, raw), 0);
    }
    assert_eq!(map_raw(&mut q, &t, 0x21), 0x8e);
    assert_eq!(map_raw(&mut q, &t, 0x64 | 0x80), 0);
    // Caps lock upper-cases letters only.
    map_raw(&mut q, &t, 0x62);
    assert_eq!(map_raw(&mut q, &t, 0x21), 0x53);
    assert_eq!(map_raw(&mut q, &t, 0x01), 0x31);
}

#[test]
fn alt_and_keypad_digits_build_a_code_given_on_release() {
    let t = tables().keymap;
    let mut q = Qualifiers::default();
    map_raw(&mut q, &t, 0x65);
    // Keypad 1 then 2 under alt map to 0x14f and 0x150: digits 1 and 2.
    assert_eq!(map_raw(&mut q, &t, 0x1d), 0);
    assert_eq!(map_raw(&mut q, &t, 0x1e), 0);
    assert_eq!(q.number, 12);
    assert_eq!(map_raw(&mut q, &t, 0x65 | 0x80), 12);
    assert_eq!(q.number, 0);
    assert_eq!(map_raw(&mut q, &t, 0x65 | 0x80), 0);
}

#[test]
fn bridge_finds_characters_in_the_game_table() {
    let t = tables().keymap;
    assert_eq!(raw_events(&t, Key::Char('a')), [0x20]);
    assert_eq!(raw_events(&t, Key::Char('S')), [0x60, 0x21, 0xe0]);
    assert_eq!(raw_events(&t, Key::Char('!')), [0x60, 0x01, 0xe0]);
    assert!(raw_events(&t, Key::Char('~')).is_empty());
    let mut q = Qualifiers::default();
    assert_eq!(key_code(&mut q, &t, Key::Char('S')), 0x53);
    assert_eq!(q, Qualifiers::default(), "shift released again");
    assert_eq!(key_code(&mut q, &t, Key::Up), 0x148);
    assert_eq!(key_code(&mut q, &t, Key::Enter), 0x0d);
    assert_eq!(key_code(&mut q, &t, Key::Escape), 0x1b);
}

/// A control character (Ctrl-A = 0x01 here) comes from the control column of the game's table,
/// wrapped in a control press and release.
#[test]
fn bridge_sends_control_characters_with_control_held() {
    let t = tables().keymap;
    assert_eq!(raw_events(&t, Key::Char('\u{1}')), [0x63, 0x20, 0xe3]);
    let mut q = Qualifiers::default();
    assert_eq!(key_code(&mut q, &t, Key::Char('\u{1}')), 0x01);
    assert_eq!(q, Qualifiers::default(), "control released again");
}

/// Reads one command key and stores it in `vars[0]`, `keys` given one per take ([`run_each`]).
fn read_one(s: State, keys: &[Key]) -> Engine {
    run_each(s, keys, 8, |st, io| async move {
        let k = wait::read_key(&st, &io).await;
        st.borrow_mut().converse.ints[0] = k;
    })
}

/// [`run_each`] fails when the game leaves a key queued past the budget (a game that reads
/// none), rather than queue the next behind it, where a drained take would drop it.
#[test]
#[should_panic(expected = "not taken in 8 ticks")]
fn keys_one_per_take_fail_when_a_key_is_not_taken() {
    run_each(
        state(),
        &[Key::Char('a'), Key::Char('s')],
        8,
        |_st, io| async move {
            io.sleep(u32::MAX).await;
        },
    );
}

#[test]
fn key_read_upcases_letters_and_maps_arrows_to_walk() {
    let e = read_one(state(), &[Key::Char('a')]);
    assert_eq!(e.state().converse.ints[0], u16::from(b'A'));
    let e = read_one(state(), &[Key::Right]);
    assert_eq!(e.state().converse.ints[0], wait::WALK);
    assert_eq!(e.state().ui.input.direction, 2);
    // Without a key the read keeps waiting (and yields every tick).
    let e = read_one(state(), &[]);
    assert!(!e.finished());
}

/// Spec 21: a diagonal key (the PC keypad's, which travel presses) reads as the walk in its
/// direction, as an arrow does; no raw key of the table gives it.
#[test]
fn key_read_maps_the_diagonal_keys_to_walk() {
    for (key, dir) in [
        (Key::UpRight, 1),
        (Key::DownRight, 3),
        (Key::DownLeft, 5),
        (Key::UpLeft, 7),
    ] {
        let e = read_one(state(), &[key]);
        assert_eq!(e.state().converse.ints[0], wait::WALK, "{key:?}");
        assert_eq!(e.state().ui.input.direction, dir, "{key:?}");
        assert!(raw_events(&tables().keymap, key).is_empty(), "{key:?}");
    }
}

#[test]
fn panel_keys_are_handled_inside_the_key_read() {
    let mut s = state();
    s.actors.party.size = 3;
    s.actors.party.members[..3].copy_from_slice(&[1, 2, 3]);
    s.actors.party.names = vec!["Ann".into(), "Bo".into(), "Cy".into()];
    s.ui.panel.mode = 0x90;
    s.ui.panel.dirty = false;
    // `+` steps to the next member and `*` switches stats -> inventory: both are handled
    // inside 0x10790a (panel redrawn), the read never returns.
    let e = read_one(s.clone(), &[Key::Char('+'), Key::Char('+'), Key::Char('+')]);
    assert!(!e.finished());
    let st = e.state();
    assert_eq!(
        (st.ui.panel.mode, st.ui.panel.member),
        (0x90, 2),
        "stops at the last"
    );
    assert!(!st.ui.panel.dirty, "redrawn");
    drop(st);
    let e = read_one(s.clone(), &[Key::Char('*')]);
    assert_eq!(e.state().ui.panel.mode, 0x92);
    // `/` goes to the roster; there `-` at the top of the list is an ordinary key again.
    let e = read_one(s, &[Key::Char('/'), Key::Char('-')]);
    assert!(e.finished());
    assert_eq!(e.state().ui.panel.mode, 0x91);
    assert_eq!(e.state().converse.ints[0], u16::from(b'-'));
}

#[test]
fn cross_hair_moves_within_range_and_the_command_letter_confirms() {
    let mut s = state();
    s.map.centre = u6_formats::position::Position {
        x: 100,
        y: 200,
        z: 0,
    };
    let input = &mut s.ui.input;
    // As the command loop sets it up for Talk (0x104330): cursor mode 1 on the leader's cell
    // (Original (5, 5)), range 7.
    input.target_pending = 1;
    input.target_command = 0x83;
    input.targeting = 1;
    input.cursor = LEADER_CELL;
    input.range = 7;
    s.ui.tables.keymap[0x14] = [0x74, 0x54, 0x14, 0];
    let e = read_one(s.clone(), &[Key::Right, Key::Up, Key::Char('t')]);
    let st = e.state();
    assert_eq!(st.converse.ints[0], wait::REPEAT, "T confirms like Return");
    assert_eq!(st.ui.input.targeting, 0);
    let (lx, ly) = (i16::from(LEADER_CELL.0), i16::from(LEADER_CELL.1));
    assert_eq!(st.ui.input.pointer, pointer::cell_pixel(lx + 1, ly - 1));
    // The view cell east and north of the leader's (Original (6, 4)) around the centre (100,
    // 200) is map (101, 199).
    assert_eq!(st.ui.input.target, (101, 199));
    assert_eq!(st.ui.input.selection, -1);
    drop(st);
    // Range -1 (Get, Move, Use): a direction selects the neighbouring cell at once.
    s.ui.input.range = -1;
    let e = read_one(s, &[Key::Up]);
    let st = e.state();
    assert_eq!(st.converse.ints[0], wait::REPEAT);
    assert_eq!(st.ui.input.target, (100, 199));
}

/// Spec 13 Decision 4: the cross-hair stays in the original's 11 × 11 window round the leader.
/// Moved nine cells east (the Wide view goes on past it) it stops [`RULE_HALF`] east of the
/// leader's cell, and confirmed it targets the map location there.
///
/// [`RULE_HALF`]: crate::geom::RULE_HALF
#[test]
fn the_cross_hair_moved_nine_cells_east_stops_at_the_rule_half() {
    use crate::geom::RULE_HALF;
    let mut s = state();
    s.map.centre = u6_formats::position::Position {
        x: 100,
        y: 200,
        z: 0,
    };
    let input = &mut s.ui.input;
    input.target_pending = 1;
    input.target_command = 0x83;
    input.targeting = 1;
    input.cursor = LEADER_CELL;
    input.range = 7;
    s.ui.tables.keymap[0x14] = [0x74, 0x54, 0x14, 0];
    let mut keys = vec![Key::Right; 9];
    keys.push(Key::Char('t'));
    let e = run_each(s, &keys, 40, |st, io| async move {
        let k = wait::read_key(&st, &io).await;
        st.borrow_mut().converse.ints[0] = k;
    });
    assert!(e.finished(), "the confirm ends the read");
    let st = e.state();
    let (lx, ly) = (i16::from(LEADER_CELL.0), i16::from(LEADER_CELL.1));
    assert_eq!(st.converse.ints[0], wait::REPEAT, "T confirms");
    assert_eq!(
        st.ui.input.cursor,
        ((lx + RULE_HALF) as i8, ly as i8),
        "stopped at the rule's edge"
    );
    assert_eq!(st.ui.input.pointer, pointer::cell_pixel(lx + RULE_HALF, ly));
    assert_eq!(
        st.ui.input.target,
        (100 + RULE_HALF, 200),
        "its map location"
    );
}

#[test]
fn escape_drops_the_cross_hair_and_keeps_waiting() {
    let mut s = state();
    let input = &mut s.ui.input;
    input.target_pending = 1;
    input.targeting = 1;
    input.cursor = LEADER_CELL;
    input.range = 7;
    let e = read_one(s, &[Key::Escape]);
    assert!(!e.finished());
    assert_eq!(e.state().ui.input.targeting, 0);
}

/// Runs the line editor for at most `max` characters; the result goes to the converse buffer 0
/// and the count to word variable 0 (test scratch).
fn edit(keys: &[Key], max: i16) -> Engine {
    run(state(), keys, 12, move |st, io| async move {
        let (n, line) = keyread::read_line(&st, &io, max).await;
        io.present();
        let mut s = st.borrow_mut();
        s.converse.ints[0] = n as u16;
        s.converse.buffers[0] = line;
    })
}

#[test]
fn line_editor_echoes_backspaces_and_ends_on_return_or_escape() {
    let (a, s, bs, ret) = (Key::Char('a'), Key::Char('s'), Key::Backspace, Key::Enter);
    let e = edit(&[a, s, bs, a, ret], 8);
    assert!(e.finished());
    assert_eq!(e.state().converse.buffers[0], b"aa");
    assert_eq!(e.state().converse.ints[0], 2);
    assert_eq!(scroll_row(&e, 14).trim_end(), "aa", "echoed, the s erased");
    // At most `max` characters are taken.
    let e = edit(&[a, a, a, ret], 2);
    assert_eq!(e.state().converse.buffers[0], b"aa");
    // Escape erases the echo and gives -1.
    let e = edit(&[a, s, Key::Escape], 8);
    assert_eq!(e.state().converse.ints[0], 0xffff);
    assert!(e.state().ui.input.line_cancelled);
    assert_eq!(scroll_row(&e, 14).trim_end(), "");
    let st = e.state();
    assert!(st.ui.input.upcase && !st.ui.input.line_input && st.ui.input.drain);
}

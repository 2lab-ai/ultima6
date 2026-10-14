//! The event wait 0x106bbc: keys to commands and directions, the cross-hair and the panel
//! cursor, the panel keys. Everything but the pointer (mouse) part is ported; the port has no
//! pointer device, so the click mapping 0x10658e is behind a seam that reports no buttons.

use std::cell::RefCell;

pub use super::cursor::panel_cursor;
use super::cursor::{select_command, target_from_pointer};
use super::keyread::idle_poll;
use super::print::put_raw;
use super::seams;
use crate::engine::Io;
use crate::state::State;
use crate::todo_stub;

const TAB: u16 = 0x09;
const RETURN: u16 = 0x0d;
const ESCAPE: u16 = 0x1b;
const SPACE: u16 = 0x20;
/// First function key code (F1 = 0x13b): F1 + n picks party member n in the panel.
const F1: u16 = 0x13b;

/// Alt + letter codes (0x100 + PC scan code) that select a command (0x106ca0 switch).
const COMMAND_KEYS: [(u16, u8); 10] = [
    (0x113, 0x89),
    (0x114, 0x83),
    (0x116, 0x88),
    (0x11e, 0x81),
    (0x120, 0x86),
    (0x122, 0x85),
    (0x126, 0x84),
    (0x12e, 0x82),
    (0x130, 0x8a),
    (0x132, 0x87),
];
/// Cursor-key codes and the direction each gives (0 = north, clockwise).
const DIRECTION_KEYS: [(u16, i16); 8] = [
    (0x148, 0),
    (0x149, 1),
    (0x14d, 2),
    (0x151, 3),
    (0x150, 4),
    (0x14f, 5),
    (0x14b, 6),
    (0x147, 7),
];

/// 0x14f12c (copy the pointer position and buttons) — the port has no pointer device:
/// buttons are never down. Seam for the platform (an `InputEvent` for the mouse).
fn pointer_buttons() -> u16 {
    0
}

/// Original: 0x106bbc (ui: wait for the next input event, map it to a game code) [partial]
///
/// Commands, directions, the cross-hair, the panel cursor and the panel keys are handled here;
/// the result is non-zero. Not ported: the pointer (mouse) branch (no pointer device: the
/// click map 0x10658e is a declared stub that the keyboard never reaches) and the inline spell
/// syllable entry and spellbook keys (Cast, outside M1; declared stubs).
pub async fn wait_event(state: &RefCell<State>, io: &Io) -> u16 {
    if state.borrow().ui.input.accept {
        put_raw(state, io, -1).await;
    }
    let mut key: u16 = 0;
    {
        let mut s = state.borrow_mut();
        let rows = i16::from(s.ui.text.win().height);
        s.ui.text.more_rows = rows;
    }
    loop {
        if key != 0 {
            return key;
        }
        if pointer_buttons() & 3 != 0 {
            // Contract: map a click to a code (0 = none). Unreachable: no pointer device.
            key = todo_stub!("0x10658e map a click to a command", 0);
            continue;
        }
        key = idle_poll(state, io).await;
        let (keep_cursor, accept, line, targeting);
        (key, keep_cursor, accept, line, targeting) = map_key(&mut state.borrow_mut(), io, key);
        if !keep_cursor {
            panel_cursor(state, io, 0);
            state.borrow_mut().ui.input.targeting = 0;
        }
        if !accept || (line && key == RETURN) {
            continue;
        }
        let targeting = if keep_cursor { targeting } else { 0 };
        key = if targeting != 0 {
            targeting_key(state, io, key).await
        } else {
            tab_key(state, io, key)
        };
        key = panel_key(state, io, key);
    }
}

/// 0x106c6c..0x106de6: the key from the poll: case, commands, directions; and the flags the
/// rest of the iteration tests.
fn map_key(s: &mut State, io: &Io, key: u16) -> (u16, bool, bool, bool, u8) {
    let mut key = key;
    let input = &mut s.ui.input;
    if key != 0 {
        input.key_marks = (-1, -1);
    }
    if input.upcase && key < 0x100 && (key as u8).is_ascii_lowercase() {
        key -= 0x20;
    }
    if let Some(&(_, cmd)) = COMMAND_KEYS.iter().find(|(k, _)| *k == key) {
        select_command(s, io, cmd);
        key = 0;
    } else if let Some(&(_, dir)) = DIRECTION_KEYS.iter().find(|(k, _)| *k == key) {
        s.ui.input.direction = dir;
        key = super::wait::WALK;
    }
    let input = &s.ui.input;
    let keep_cursor = input.target_pending != 0 || input.cursor_by_tab;
    (
        key,
        keep_cursor,
        input.accept,
        input.line_input,
        input.targeting,
    )
}

/// 0x107604: Tab starts the cursor: the panel cursor in the inventory panel, else the
/// cross-hair (not in panel modes 0x9e / 0xa1).
fn tab_key(state: &RefCell<State>, io: &Io, key: u16) -> u16 {
    let mode = {
        let s = state.borrow();
        let mode = s.ui.panel.mode;
        if key != TAB || mode == 0x9e || s.ui.panel.locked {
            return key;
        }
        mode
    };
    if mode == 0x92 {
        state.borrow_mut().ui.input.targeting = 2;
        panel_cursor(state, io, 1);
    } else if mode != 0xa1 {
        state.borrow_mut().ui.input.targeting = 1;
    }
    let mut s = state.borrow_mut();
    if s.ui.input.targeting == 0 {
        return key;
    }
    start_cross_hair(&mut s);
    0
}

/// 0x107640..0x10765c: cross-hair at the window centre (5, 5), range 7, started by Tab.
fn start_cross_hair(s: &mut State) {
    let input = &mut s.ui.input;
    input.cursor = (5, 5);
    input.range = 7;
    input.cursor_by_tab = true;
    seams::present_map(s, 1);
}

/// 0x10765e..0x1078c4: keys of the panel modes: `*` / `/` switch between stats, inventory and
/// roster, `+` / `-` step the member (or scroll the roster), F1.. pick a member.
fn panel_key(state: &RefCell<State>, io: &Io, key: u16) -> u16 {
    let mut s = state.borrow_mut();
    let (mode, size) = (s.ui.panel.mode, u16::from(s.actors.party.size));
    if s.ui.panel.locked {
        return key;
    }
    let member_key = (F1..F1 + size).contains(&key);
    let input = &mut s.ui.input;
    match mode {
        0x92 | 0x90 => {
            let other = if mode == 0x92 { 0x90 } else { 0x92 };
            match key {
                0x2a => {
                    input.selection = -1;
                    other
                }
                0x2f | 0x144 => 0x91,
                0x2b => 0x99,
                0x2d => 0x9a,
                0x60 | 0x7e if mode == 0x92 => {
                    input.pointer = (0xf8, 0x60);
                    drop(s);
                    // Contract: act on the inventory slot under the pointer (the panel's click
                    // handler). Fallback: nothing happens.
                    todo_stub!("0x10619c inventory click at the pointer", ());
                    let _ = io;
                    0
                }
                _ if member_key => {
                    input.selection = (key - F1) as i16;
                    mode
                }
                _ => key,
            }
        }
        0x91 => {
            let first = u16::from(s.ui.panel.roster_first);
            match key {
                0x2b if (first as i16) < size as i16 - 5 => 0x94,
                0x2d if first != 0 => 0x93,
                _ if member_key => {
                    s.ui.input.selection = (key - F1) as i16;
                    0x92
                }
                _ => key,
            }
        }
        0xa1 => match key {
            SPACE | ESCAPE => {
                input.target = (-1, -1);
                0x8f
            }
            RETURN => {
                // Contract: cast the spell under the spellbook cursor (magic, not in M1).
                // Fallback: no spell; the key is consumed.
                todo_stub!("0x115da4 spellbook select", 0)
            }
            0x80 => {
                // Contract: move the spellbook cursor (0x106812). Fallback: consumed.
                todo_stub!("0x106812 spellbook cursor step", 0)
            }
            _ => key,
        },
        _ => key,
    }
}

/// The command letter that confirms a cursor started by that command (0x106e1e..0x106e86):
/// A, C (cross-hair only), T, L, G, D, M, U.
fn confirms(cmd: u16, targeting: u8, key: u16) -> bool {
    let letter = match cmd {
        0x81 => b'A',
        0x82 if targeting == 1 => b'C',
        0x83 => b'T',
        0x84 => b'L',
        0x85 => b'G',
        0x86 => b'D',
        0x87 => b'M',
        0x88 => b'U',
        _ => return false,
    };
    key == u16::from(letter)
}

/// 0x106e1e..0x107602: a key while a cursor is up.
async fn targeting_key(state: &RefCell<State>, io: &Io, key: u16) -> u16 {
    let (cmd, targeting) = {
        let s = state.borrow();
        (s.ui.input.target_command, s.ui.input.targeting)
    };
    let key = if confirms(cmd, targeting, key) {
        RETURN
    } else {
        key
    };
    if cmd == 0x82 && targeting == 3 {
        // Contract: enter a spell syllable (Cast, not in M1). Fallback: consumed.
        return todo_stub!("0x10674e spell syllable entry", 0);
    }
    match key {
        TAB => {
            if targeting == 1 {
                let redraw = {
                    let mut s = state.borrow_mut();
                    s.ui.input.targeting = 2;
                    let (leader, size) = (s.actors.party.leader_index, s.actors.party.size);
                    let p = &mut s.ui.panel;
                    let redraw = p.mode != 0x92;
                    if redraw {
                        p.member = if leader == size { 0 } else { i16::from(leader) };
                        p.mode = 0x92;
                        p.dirty = true;
                    }
                    redraw
                };
                if redraw {
                    Box::pin(super::panels::redraw_panel(state, io)).await;
                }
                panel_cursor(state, io, 1);
            } else {
                let (mode, locked) = {
                    let s = state.borrow();
                    (s.ui.panel.mode, s.ui.panel.locked)
                };
                if mode != 0x9e && !locked {
                    panel_cursor(state, io, 0);
                    state.borrow_mut().ui.input.targeting = 1;
                }
            }
            seams::present_map(&mut state.borrow_mut(), 1);
            0
        }
        SPACE | ESCAPE => {
            panel_cursor(state, io, 0);
            state.borrow_mut().ui.input.targeting = 0;
            0
        }
        RETURN => confirm(state, io, targeting),
        super::wait::WALK => step(state, io, targeting),
        _ => member_key(state, key, targeting),
    }
}

/// Return with a cursor up (0x107218..0x1072c8).
fn confirm(state: &RefCell<State>, io: &Io, targeting: u8) -> u16 {
    if targeting == 1 {
        let mut s = state.borrow_mut();
        let mut key = super::wait::REPEAT;
        let last = super::globals::last_command(&s.actors);
        let input = &mut s.ui.input;
        if input.target_pending == 0 {
            if input.cursor_by_tab && last != super::cursor::NO_ICON {
                key = 0xa9;
            }
            input.cursor_by_tab = false;
        }
        input.targeting = 0;
        let (cx, cy) = input.cursor;
        input.pointer = (i16::from(cx) << 4, i16::from(cy) << 4);
        target_from_pointer(&mut s);
        return key;
    }
    let mode = state.borrow().ui.panel.mode;
    if targeting != 2 || (mode != 0x92 && mode != 0x90) {
        // 0x107272: falls through to the panel keys with the Return still in D7.
        return RETURN;
    }
    panel_cursor(state, io, 2);
    // Contract: act on the inventory / stats slot under the pointer (0x10619c / 0x10613a) and
    // give the resulting code, 0 for none. Fallback: none.
    let key: u16 = if mode == 0x92 {
        todo_stub!("0x10619c inventory click at the pointer", 0)
    } else {
        todo_stub!("0x10613a stats panel click at the pointer", 0)
    };
    let mut s = state.borrow_mut();
    if key == 0 && s.ui.input.target_pending == 1 && target_from_pointer(&mut s) != 0 {
        return super::wait::REPEAT;
    }
    key
}

/// A direction with a cursor up (0x1072cc..0x10755c): the cross-hair moves within its range
/// (range -1: the step selects at once); the panel cursor moves over the panel's slots.
fn step(state: &RefCell<State>, io: &Io, targeting: u8) -> u16 {
    let mut s = state.borrow_mut();
    let dir = s.ui.input.direction.rem_euclid(8) as usize;
    let (dx, dy) = s.ui.tables.step[dir];
    let mode = s.ui.panel.mode;
    if targeting == 1 {
        let (cx, cy) = s.ui.input.cursor;
        let (nx, ny) = (i16::from(cx) + i16::from(dx), i16::from(cy) + i16::from(dy));
        let range = s.ui.input.range;
        if (0..11).contains(&nx)
            && (0..11).contains(&ny)
            && seams::distance(nx, ny, 5, 5) <= i16::from(range)
        {
            s.ui.input.cursor = (nx as i8, ny as i8);
            seams::present_map(&mut s, 1);
            return 0;
        }
        if range != -1 {
            return 0;
        }
        s.ui.input.pointer = (nx << 4, ny << 4);
        target_from_pointer(&mut s);
        return super::wait::REPEAT;
    }
    if targeting != 2 || (mode != 0x92 && mode != 0x90) {
        // 0x1074fa: the walk code goes on to the panel keys unchanged.
        return super::wait::WALK;
    }
    let (pc, pr) = s.ui.input.panel_cursor;
    let mut nx = i16::from(pc) + i16::from(dx);
    drop(s);
    if mode == 0x90 {
        nx = nx.clamp(0, 3);
        panel_cursor(state, io, 0);
        state.borrow_mut().ui.input.panel_cursor = (nx as i8, 3);
        panel_cursor(state, io, 1);
        return 0;
    }
    let ny = i16::from(pr) + i16::from(dy);
    if (0..7).contains(&nx) && (0..3).contains(&ny) {
        panel_cursor(state, io, 0);
        let (mut x, mut y) = (nx, ny);
        if (x, y) == (1, 1) {
            // The centre slot is the member figure: step over it.
            x += i16::from(dx);
            y += i16::from(dy);
        }
        state.borrow_mut().ui.input.panel_cursor = (x as i8, y as i8);
        panel_cursor(state, io, 1);
        return 0;
    }
    if (nx <= 2 || nx >= 7) && ny <= 2 {
        return 0;
    }
    let mut s = state.borrow_mut();
    let p = &s.ui.panel;
    let shown = i16::from(p.inventory_count) - i16::from(p.inventory_first);
    let (first, no_rows) = (p.inventory_first, p.no_rows);
    let south = s.ui.input.direction == 4;
    if ny > 2 && shown > 12 && south {
        s.ui.input.panel_cursor.0 = nx as i8;
        return 0x96;
    }
    if ny < 0 && first != 0 {
        s.ui.input.panel_cursor.0 = nx as i8;
        return 0x95;
    }
    drop(s);
    if ny < 0 && !no_rows {
        panel_cursor(state, io, 0);
        state.borrow_mut().ui.input.panel_cursor.1 = -1;
        panel_cursor(state, io, 1);
    } else if ny > 2 && !no_rows {
        panel_cursor(state, io, 0);
        state.borrow_mut().ui.input.panel_cursor = (nx.clamp(0, 4) as i8, 3);
        panel_cursor(state, io, 1);
    }
    0
}

/// A digit with the cross-hair up (0x107560..0x107602): `1` + n targets party member n.
fn member_key(state: &RefCell<State>, key: u16, targeting: u8) -> u16 {
    let mut s = state.borrow_mut();
    let size = s.actors.party.size;
    let n = key.wrapping_sub(u16::from(b'1'));
    if key < u16::from(b'1') || n >= u16::from(size) || targeting != 1 {
        return key;
    }
    let actor = s.actors.party.members[usize::from(n).min(15)];
    let at = s
        .objects
        .location(crate::objects::ObjectId(u16::from(actor)));
    let leader_index = s.actors.party.leader_index;
    let centre = s.map.centre;
    let input = &mut s.ui.input;
    input.target_void = false;
    input.targeting = 0;
    input.selection = i16::from(actor);
    input.target = if leader_index == size {
        (centre.x as i16, centre.y as i16)
    } else {
        let p = at.unwrap_or_default();
        (p.x as i16, p.y as i16)
    };
    super::wait::REPEAT
}

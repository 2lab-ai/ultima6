//! The event wait 0x106bbc: the pointer (clicks, [`super::click`]), keys to commands and
//! directions, the cross-hair and the panel cursor, the panel keys.

use std::cell::RefCell;

pub use super::cursor::panel_cursor;
use super::cursor::{select_command, target_from_pointer};
use super::keyread::idle_poll;
use super::pointer;
use super::print::put_raw;
use super::seams;
use crate::engine::Io;
use crate::state::State;

const TAB: u16 = 0x09;
const RETURN: u16 = 0x0d;
const ESCAPE: u16 = 0x1b;
const SPACE: u16 = 0x20;
/// First function key code (F1 = 0x13b): F1 + n picks party member n in the panel.
const F1: u16 = 0x13b;
/// [replaced] Milliseconds from a held walk's step to its next sample: 33 PAL frames (20 ms each),
/// from the oracle (tools/oracle/pace.mjs: game-first disks, the throne room with every actor
/// outside the party passed over and the fight broken off, a press held three cells south, 6
/// boots). In 4 boots the steps came 25..41 frames apart (median 29, mean 31.7, alternating about
/// 27 and 38), the first 1..12 frames after the press and the second 28..37; in the other 2,
/// whose followers were still forming up, 26..108 (median 72). 33 frames from the press puts the
/// port's cells per held time within one of those 4 boots at every hold up to their 8 free cells
/// (100 ms: 1, 1 s: 2, 4 s: 6), so no separate first-repeat delay is needed.
pub(crate) const HELD_WALK_MS: u32 = 660;

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

/// Original: 0x106bbc (ui: wait for the next input event, map it to a game code)
///
/// Each round samples the pointer first: while a button is down the click map gives the code
/// ([`pointer_click`]); else a key: commands, directions, the cross-hair, the panel cursor and
/// the panel keys. The result is non-zero. The inline spell syllable entry and the spellbook
/// keys are magic's ([`crate::magic::spell_key`], [`crate::magic::book_panel_key`]).
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
        key = pointer_click(state, io).await;
        if key != 0 {
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
        key = if crate::magic::takes_book_key(&state.borrow(), key) {
            crate::magic::book_panel_key(state, io, key).await
        } else {
            panel_key(state, io, key).await
        };
    }
}

/// 0x106be6..0x106c5e: the pointer sample (0x14f12c copies the handler's position 0x1623b8 /
/// 0x1623ba and its button word 0x161bd4; the port takes the queued events up to the next button
/// change, [`Io::take_pointer`]); with a button down the click map 0x10658e gives the code.
/// A4-0x7a34 marks the press (the first sample with a button down, A4-0x7a36 then set); while
/// held only the press's code repeats. With no button down both clear.
///
/// A held button (A4-0x7a36 set and still down) repeats at once in the original: nothing in
/// 0x106bbc or 0x14f12c waits, the input.device handler runs beside the game and the release
/// shows in the next sample. The port's handler is the engine's queue, which fills only between
/// polls, so a held repeat first waits one display frame and samples again (a prompt that drops
/// the walk code then still sees the release). A key queued in front of the pointer events holds
/// them back (input in order), the release perhaps among them, so a button still held behind a
/// key gives way to the key poll.
///
/// [replaced] What paces a held walk in the original is the step itself: it redraws the view
/// (more than once while the followers form up) and every redraw costs the 68000 its CPU and
/// blitter time, about 25 frames in the oracle, so the next sample comes [`HELD_WALK_MS`] after
/// the step. The port draws a step at once, so where the walk code is a step (the command loop
/// reads it, A4-0x7a2c, and the last step went through, A4-0x6a90 clear) the repeat waits that
/// long on the engine clock (wall time, the same at any host frame rate) before it samples
/// again. A prompt that drops the code keeps the frame pace. Not reproduced: 0x14f12c reads the
/// handler's button word 0x161bd4 once per pass of this wait, so the original misses a click
/// shorter than a pass (in the oracle 10 of 18 presses of 5 frames); the queue keeps every press.
async fn pointer_click(state: &RefCell<State>, io: &Io) -> u16 {
    if sample(state, io) != 0 && state.borrow().actors.work.mouse_walk != 0 {
        let pace = {
            let s = state.borrow();
            s.main.reading == 1
                && s.ui.pointer.press_code == super::wait::WALK
                && s.actors.work.step_blocked == 0
        };
        let due = io.now_ms() + if pace { u64::from(HELD_WALK_MS) } else { 0 };
        io.vblank().await;
        if let Some(rest) = due.checked_sub(io.now_ms()).filter(|&ms| ms > 0) {
            io.sleep(u32::try_from(rest).unwrap_or(u32::MAX)).await;
        }
        if sample(state, io) != 0 && io.key_ready() {
            return 0;
        }
    }
    {
        let mut s = state.borrow_mut();
        let ptr = &s.ui.pointer;
        let buttons = u16::from(ptr.left) | u16::from(ptr.right);
        if buttons & 3 == 0 {
            s.ui.pointer.pressed = false;
            s.actors.work.mouse_walk = 0;
            return 0;
        }
        let pressed = s.actors.work.mouse_walk == 0;
        s.actors.work.mouse_walk = 1;
        s.ui.pointer.pressed = pressed;
        s.ui.pointer.buttons = buttons;
        s.ui.input.pointer = s.ui.pointer.at;
    }
    let code = super::click::click(state, io).await;
    let mut s = state.borrow_mut();
    let ptr = &mut s.ui.pointer;
    if ptr.pressed {
        ptr.press_code = code;
        code
    } else if ptr.press_code == code {
        code
    } else {
        0
    }
}

/// 0x106be6: the handler's pointer into the sample (0x14f12c); the buttons as 0x106bbc reads
/// them (1 left, 2 right; both down reads as neither).
fn sample(state: &RefCell<State>, io: &Io) -> u16 {
    let p = io.take_pointer();
    let mut s = state.borrow_mut();
    pointer::sample(&mut s, p);
    u16::from(s.ui.pointer.left) | u16::from(s.ui.pointer.right)
}

/// 0x106c6c..0x106de6: the key from the poll: case, commands, directions; and the flags the
/// rest of the iteration tests.
fn map_key(s: &mut State, io: &Io, key: u16) -> (u16, bool, bool, bool, u8) {
    let mut key = key;
    if key != 0 {
        s.actors.work.mouse_target = (-1, -1);
    }
    let input = &mut s.ui.input;
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
        if key != TAB || mode == 0x9e || s.converse.active {
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
    start_cross_hair(&mut s, io);
    0
}

/// 0x107640..0x10765c: cross-hair at the window centre (5, 5), range 7, started by Tab.
fn start_cross_hair(s: &mut State, io: &Io) {
    let input = &mut s.ui.input;
    input.cursor = (5, 5);
    input.range = 7;
    input.cursor_by_tab = true;
    seams::present_map(s, io, 1);
}

/// 0x10765e..0x1078c4: keys of the panel modes: `*` / `/` switch between stats, inventory and
/// roster, `+` / `-` step the member (or scroll the roster), F1.. pick a member; `` ` `` / `~`
/// click the inventory's combat-mode icon (0x10619c with the pointer at (0xf8, 0x60)).
async fn panel_key(state: &RefCell<State>, io: &Io, key: u16) -> u16 {
    let combat_icon = {
        let mut s = state.borrow_mut();
        let hit = !s.converse.active && s.ui.panel.mode == 0x92 && matches!(key, 0x60 | 0x7e);
        if hit {
            s.ui.input.pointer = (0xf8, 0x60);
        }
        hit
    };
    if combat_icon {
        Box::pin(super::click::inventory_click(state, io)).await;
        return 0;
    }
    panel_key_in(&mut state.borrow_mut(), key)
}

/// [`panel_key`] for every key but the combat-mode icon's.
fn panel_key_in(s: &mut State, key: u16) -> u16 {
    let (mode, size) = (s.ui.panel.mode, u16::from(s.actors.party.size));
    if s.converse.active {
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
            // Return and a direction go to crate::magic::book_panel_key (wait_event).
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
        return Box::pin(crate::magic::spell_key(state, io, key)).await;
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
                    (s.ui.panel.mode, s.converse.active)
                };
                if mode != 0x9e && !locked {
                    panel_cursor(state, io, 0);
                    state.borrow_mut().ui.input.targeting = 1;
                }
            }
            seams::present_map(&mut state.borrow_mut(), io, 1);
            0
        }
        SPACE | ESCAPE => {
            panel_cursor(state, io, 0);
            state.borrow_mut().ui.input.targeting = 0;
            0
        }
        RETURN => confirm(state, io, targeting).await,
        super::wait::WALK => step(state, io, targeting),
        _ => member_key(state, key, targeting),
    }
}

/// Return with a cursor up (0x107218..0x1072c8).
async fn confirm(state: &RefCell<State>, io: &Io, targeting: u8) -> u16 {
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
    let key: u16 = if mode == 0x92 {
        Box::pin(super::click::inventory_click(state, io)).await
    } else {
        super::click::stats_click(&mut state.borrow_mut())
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
            && seams::distance(&s, nx, ny, 5, 5) <= i16::from(range)
        {
            s.ui.input.cursor = (nx as i8, ny as i8);
            seams::present_map(&mut s, io, 1);
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

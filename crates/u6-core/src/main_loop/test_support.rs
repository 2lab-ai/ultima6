//! Fixtures shared by the modules' tests (integration, WU-P7).

use crate::objects::{NONE, OBJECT_SLOTS, Slot};
use crate::state::State;

/// Test fixtures place objects by writing the slot tables; this links them into the object list
/// the way load state and the region load leave it, keeping their slot numbers: the slots reset
/// (0x10ed44), the actors linked (0x10edb4), every other placed slot taken off the free chain and
/// inserted at its list position, then the window around the centre (0x108d10's origin, the
/// grid 0x10dea4). Safe to run again after a fixture moves or adds objects.
pub(crate) fn link_placed_objects(s: &mut State) {
    let placed: Vec<Slot> = (0x101..OBJECT_SLOTS as Slot)
        .filter(|&i| s.objects.type_frame[i as usize] != 0)
        .collect();
    crate::objects::init_slots(&mut s.objects);
    crate::objects::link_actors(&mut s.objects);
    let o = &mut s.objects;
    for &slot in &placed {
        let (mut prev, mut c) = (NONE, o.free.0);
        while c >= 0 && c != slot {
            prev = c;
            c = o.next_of(c);
        }
        if c == slot {
            let n = o.next_of(slot);
            if prev < 0 {
                o.free.0 = n;
            } else {
                o.set_next(prev, n);
            }
            o.free.1 -= 1;
        }
        let p = o.list_pos(o.x(slot), o.y(slot), true);
        o.set_next(slot, o.next_of(p));
        o.set_next(p, slot);
    }
    let c = s.map.centre;
    let (ox, oy) = (c.x.wrapping_sub(16) & 0x3f8, c.y.wrapping_sub(16) & 0x3f8);
    s.map.window_origin = [ox, oy];
    s.objects.set_window(ox, oy, c.z);
}

/// A text layer for the tests: every printable character on a raw key (plain or shifted),
/// Return, Escape, space and backspace on theirs; the message scroll the whole screen (40 × 25
/// cells), scrolling, no wrap and no capitals, so each scroll line is one line of output.
pub(crate) fn test_ui() -> crate::ui::UiState {
    use crate::ui::tables::{Colours, Initial, RAW_KEYS, UiTables};
    let mut keymap = vec![[0u16; 4]; RAW_KEYS];
    let printable: Vec<u16> = (0x21..0x7f).collect();
    for (row, pair) in printable.chunks(2).enumerate() {
        keymap[row] = [pair[0], pair.get(1).copied().unwrap_or(0), 0, 0];
    }
    keymap[0x40] = [0x20; 4];
    keymap[0x41] = [0x08; 4];
    keymap[0x44] = [0x0d; 4];
    keymap[0x45] = [0x1b; 4];
    let t = UiTables {
        keymap,
        keyword_end: b" .,".to_vec(),
        colours: Colours {
            text: 1,
            paper: 2,
            alert: 3,
            poisoned: 4,
            command: 5,
        },
        initial: Initial {
            text_on: true,
            cursor_glyph: 5,
            accept: true,
            upcase: true,
            target_command: crate::ui::globals::NO_COMMAND,
            panel_mode: 0x91,
            ..Initial::default()
        },
        ..UiTables::default()
    };
    let mut ui = crate::ui::UiState::from_parts(None, t.clone());
    ui.text.main_windows(&t);
    ui.text
        .open(&t, crate::ui::WindowId::Messages, 0, 0, 0x27, 0x18);
    ui.text.win_mut().flags = crate::ui::text::SCROLL;
    // As the command loop's key read leaves it before a command: a whole window before a more
    // prompt.
    ui.text.more_rows = 25;
    ui
}

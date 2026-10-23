//! Fixtures shared by the modules' tests (integration, WU-P7).

use crate::engine::Io;
use crate::objects::{NONE, OBJECT_SLOTS, Slot};
use crate::state::State;

/// The load-state steps of game's main (`game_main`, 0x1351d8) after its screen, in its order,
/// for the receipts that boot by hand: A4+0x64ce = 1, the save mode at its data-hunk value
/// (A4-0x637a, which the block reads check), 0x105350's tail ([`super::finish_load_state`]),
/// one tile substitution per animdata slot (0x152cd2). The start window's egg stays unhatched
/// (A4-0x7afb set around the region load, which 0x1321dc / 0x1319b2 check), so its monsters are
/// not foes in a receipt's view.
pub(crate) fn load_state_like_main(s: &mut State, io: &Io) {
    s.main.flag_64ce = 1;
    s.objects.save_mode = s.objects.defs.save_mode;
    s.map.flag_7afb = 1;
    super::finish_load_state(s, io);
    s.map.flag_7afb = 0;
    let anim = s.tables.anim.clone();
    for slot in anim.slots.iter().take(usize::from(anim.count)) {
        crate::gfx::anim::substitute(&mut s.gfx, slot.tile_to_animate, slot.first_anim_frame);
    }
}

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
    let (ox, oy) = (
        crate::geom::window_origin(c.x),
        crate::geom::window_origin(c.y),
    );
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

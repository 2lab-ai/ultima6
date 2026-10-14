//! The command-bar underline 0x106506, the panel cursor 0x106932 and the pointer target
//! 0x105f7c.

use std::cell::RefCell;

use super::globals::{last_command, set_last_command};
use super::seams;
use super::text::fill_rect;
use crate::engine::Io;
use crate::objects::ObjectId;
use crate::state::State;

/// No command underlined (A4-0x7b09).
pub const NO_ICON: u8 = 0xff;

/// The command-bar icon column of command `cmd` (0x81..0x8a): x = (cmd - 0x81) × 16 + 8.
fn icon_x(cmd: u8) -> i32 {
    (i32::from(cmd) - 0x81) * 16 + 8
}

/// Original: 0x106506 (ui: underline the icon of command `cmd` in the command bar, removing the
/// previous underline; 0xff = none)
///
/// The underline is the pixel row y = 192 under the 16-pixel icon, in the command colour
/// A4-0x6572; the old one is painted over in the paper colour.
pub fn select_command(s: &mut State, io: &Io, cmd: u8) {
    let (paper, ink) = (s.ui.tables.colours.paper, s.ui.tables.colours.command);
    let old = last_command(&s.actors);
    if old != NO_ICON {
        let x = icon_x(old);
        io.draw(|fb| fill_rect(fb, x, 0xc0, x + 0xf, 0xc0, paper));
    }
    set_last_command(&mut s.actors, cmd);
    if cmd != NO_ICON {
        let x = icon_x(cmd);
        io.draw(|fb| fill_rect(fb, x, 0xc0, x + 0xf, 0xc0, ink));
    }
}

/// Original: 0x105f7c (ui: what the pointer is on) [partial]
///
/// In the map window, the location and an actor there; in the inventory panel, the slot.
///
/// Returns 0x9d for an actor (A4-0x2f04 = its slot), 0x9b / 0x9c for an inventory or equipment
/// slot (0x111e3c / 0x111eec, [`crate::inventory`]), else 0. Map cells are 16 pixels, the
/// window 11 × 11 cells centred on the view centre.
pub fn target_from_pointer(s: &mut State) -> u16 {
    let ui = &mut s.ui;
    ui.input.target_void = false;
    let (px, py) = ui.input.pointer;
    if px < 0xa8 {
        let c = s.map.centre;
        let (col, row) = (px >> 4, py >> 4);
        let tx = (col + c.x as i16) - 5;
        let ty = (row + c.y as i16) - 5;
        s.ui.input.target = (tx, ty);
        let (occupied, tile) = seams::window_cell(s, col, row);
        if occupied || tile >= 0x200 {
            let leader = s.actors.party.leader().map_or(0, |a| a.0 as u8);
            let who = seams::actor_at(s, leader, tx, ty);
            s.ui.input.selection = who;
            return 0x9d;
        }
        s.ui.input.selection = -1;
        s.ui.input.target_void = tile == 0xff || tile == 0x1bc;
        return 0;
    }
    if s.ui.panel.mode == 0x92 {
        s.ui.input.target = (-1, -1);
        if crate::inventory::list_slot_at(s) {
            return 0x9b;
        }
        if crate::inventory::equipment_slot_at(s) {
            return 0x9c;
        }
        return 0;
    }
    let ui = &mut s.ui;
    ui.input.target = (-1, -1);
    ui.input.selection = -1;
    0
}

/// Original: 0x106932 (ui: the panel cursor: `mode` 1 draws it, 0 restores what it covered, 2
/// moves the pointer onto it; only while the cursor mode is the panel's and the panel shows a
/// member's stats or inventory)
pub fn panel_cursor(state: &RefCell<State>, io: &Io, mode: i16) {
    panel_cursor_in(&mut state.borrow_mut(), io, mode);
}

/// [`panel_cursor`] on a borrowed state (the map-window render 0x102c90 calls it from inside a
/// synchronous step).
pub fn panel_cursor_in(s: &mut State, io: &Io, mode: i16) {
    let size = i16::from(s.actors.party.size);
    let (cursor_x, cursor_y) = (s.ui.tables.cursor_x, s.ui.tables.cursor_y);
    let ui = &mut s.ui;
    let pm = ui.panel.mode;
    if ui.input.targeting != 2 || (pm != 0x92 && pm != 0x90) {
        return;
    }
    let icons = &mut ui.panel.icons;
    let member = ui.panel.member;
    if member == 0 {
        icons[0] = 0x19b;
        icons[4] = 0x19b;
    } else {
        icons[0] = 0x183;
        icons[4] = 0x187;
    }
    icons[3] = if member == size - 1 { 0x19b } else { 0x184 };
    icons[2] = if pm == 0x90 { 0x182 } else { 0x181 };
    let pc = &mut ui.input.panel_cursor;
    if pm == 0x90 {
        pc.1 = 3;
        if pc.0 > 3 {
            pc.0 = 3;
        }
    }
    let (col, row) = *pc;
    let (mut x, mut y) = (0i16, 0i16);
    if row < 0 {
        ui.input.pointer = (0x110, 0x10);
        (x, y) = (0x110, 0x10);
    } else if row > 2 {
        if col > 4 {
            ui.input.panel_cursor.0 = 4;
        }
        let col = ui.input.panel_cursor.0;
        x = i16::from(col) * 16 + 0xb0;
        y = 0x58;
        ui.input.pointer = (x, y);
    } else if col < 3 {
        ui.input.pointer = (i16::from(col) * 0x18 + 0xb0, i16::from(row) * 0x14 + 0x18);
    } else {
        ui.input.pointer = (i16::from(col) * 16 + 0xc8, i16::from(row) * 16 + 0x20);
    }
    let (col, row) = ui.input.panel_cursor;
    if (0..3).contains(&row) {
        let c = col.clamp(0, 6) as usize;
        x = cursor_x[c] as i16;
        y = i16::from(cursor_y[row as usize][c]);
    }
    match mode {
        1 => io.draw(|fb| seams::blit_tile(s, fb, 0x16d, x, y)),
        0 => restore_under_cursor(s, io, (col, row), (x, y)),
        2 => s.ui.input.pointer = (x, y),
        _ => {}
    }
}

/// 0x106ab0..0x106ba4: redraw what the panel cursor covered.
fn restore_under_cursor(s: &mut State, io: &Io, (col, row): (i8, i8), (x, y): (i16, i16)) {
    let code = target_from_pointer(s);
    let sel = s.ui.input.selection;
    if row > 2 {
        let tile = s.ui.panel.icons[col.clamp(0, 4) as usize];
        io.draw(|fb| seams::blit_tile(s, fb, tile, x, y));
    } else if (code == 0x9b || code == 0x9c) && sel >= 0 {
        if row < 0 {
            let ty = s.objects.obj_type(ObjectId(sel as u16));
            let base = s
                .tables
                .basetile
                .base
                .get(usize::from(ty))
                .copied()
                .unwrap_or(0);
            let tile = base.wrapping_add(seams::facing_frame(ty, 4) as u16);
            super::panels::tile_on_blank(s, io, tile, 0x19b, x, y);
        } else {
            crate::inventory::draw_icon(s, io, sel, x, y);
        }
    } else {
        let p = &s.ui.panel;
        let readied = p.open_container == 0 && p.member_flag(p.member);
        let open = p.open_container != 0 && p.container_flag;
        let tile = if col == 2 && row == 1 && (readied || open) {
            0x185
        } else {
            0x19a
        };
        io.draw(|fb| seams::blit_tile(s, fb, tile, x, y));
    }
}

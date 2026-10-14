//! The command-key read 0x10790a: panel-navigation codes are handled here and never reach the
//! caller.

use std::cell::RefCell;

use super::panels::redraw_panel;
use super::target::{panel_cursor, wait_event};
use crate::engine::Io;
use crate::objects::ObjectId;
use crate::state::State;
use crate::todo_stub;

/// Codes the command loop dispatches (0x113.. / 0x147.. are mapped to these by 0x106bbc).
pub const WALK: u16 = 0x80;
/// Repeat the last targeted command (Return in the cross-hair, A4-0x7974).
pub const REPEAT: u16 = 0x8e;

/// What one panel code did (0x10790a's jump table at 0x107948).
enum Panel {
    /// Handled; redraw the panel if it is dirty and read again.
    Again,
    /// Return this code to the caller.
    Deliver(u16),
}

fn panel_code(s: &mut State, code: u16) -> Panel {
    let size = i16::from(s.actors.party.size);
    let ui = &mut s.ui;
    let (p, sel) = (&mut ui.panel, ui.input.selection);
    match code {
        0x90 | 0x92 => {
            p.mode = code;
            if sel >= 0 {
                p.member = sel;
            }
            p.dirty = true;
        }
        0x91 => {
            ui.input.targeting = 0;
            p.mode = 0x91;
            p.member = 0;
            p.dirty = true;
        }
        0x93 => {
            p.roster_first = p.roster_first.wrapping_sub(1);
            p.dirty = true;
        }
        0x94 => {
            p.roster_first = p.roster_first.wrapping_add(1);
            p.dirty = true;
        }
        0x99 => {
            if p.member < size - 1 {
                p.member += 1;
                p.dirty = true;
                ui.input.selection = -1;
            }
        }
        0x9a => {
            if p.member > 0 {
                p.member -= 1;
                p.dirty = true;
                ui.input.selection = -1;
            }
        }
        0xa6 => {}
        0xa9 => {
            let last = u16::from(super::globals::last_command(&s.actors));
            if ![0x82, 0x89, 0xaa].contains(&last) {
                ui.input.repeat = true;
            }
            return Panel::Deliver(last);
        }
        _ => return Panel::Deliver(code),
    }
    Panel::Again
}

/// 0x9f (the container shown: back to its parent, or to the roster for a member) and 0xa0 (the
/// selection becomes the container shown).
async fn open_container(state: &RefCell<State>, io: &Io, code: u16) -> Panel {
    let shown = {
        let mut s = state.borrow_mut();
        let shown = s.ui.panel.container;
        if code == 0x9f && shown < 0x100 {
            let p = &mut s.ui.panel;
            p.mode = 0x91;
            p.member = 0;
            p.dirty = true;
            return Panel::Again;
        }
        let parent = if code == 0x9f {
            // The location word of a contained object holds its container (bytes 0..1 of the
            // 3-byte entry at 0x135588: x and the low 6 bits of y).
            let at = s.objects.location(ObjectId(shown)).unwrap_or_default();
            at.x | ((at.y & 0x3f) << 10)
        } else {
            s.ui.input.selection as u16
        };
        s.ui.panel.container = parent;
        s.ui.panel.inventory_first = 0;
        parent
    };
    crate::inventory::panel_contents(state, io, shown as i16).await;
    panel_cursor(state, io, 1);
    Panel::Again
}

/// Original: 0x10790a (ui: read a command key; the panel codes 0x90..0xa9 are handled here)
pub async fn read_key(state: &RefCell<State>, io: &Io) -> u16 {
    {
        let mut s = state.borrow_mut();
        if s.ui.input.repeat {
            s.ui.input.repeat = false;
            return REPEAT;
        }
    }
    loop {
        let code = wait_event(state, io).await;
        let outcome = if (0x90..0xaa).contains(&code) {
            match code {
                0x95 => {
                    crate::inventory::list_up(state, io).await;
                    panel_cursor(state, io, 1);
                    Panel::Again
                }
                0x96 => {
                    crate::inventory::list_down(state, io).await;
                    panel_cursor(state, io, 1);
                    Panel::Again
                }
                0x9f | 0xa0 => open_container(state, io, code).await,
                0xa2..=0xa5 => {
                    // Contract: page the spellbook (magic); M1 has no spellbook open.
                    todo_stub!("0x116218 spellbook page", ());
                    Panel::Again
                }
                0xa7 => {
                    Box::pin(crate::inventory::ready(state, io)).await;
                    panel_cursor(state, io, 1);
                    Panel::Again
                }
                0xa8 => {
                    Box::pin(crate::inventory::unready(state, io)).await;
                    panel_cursor(state, io, 1);
                    Panel::Again
                }
                0x97 | 0x98 | 0x9b..=0x9e | 0xa1 => Panel::Deliver(code),
                _ => panel_code(&mut state.borrow_mut(), code),
            }
        } else {
            Panel::Deliver(code)
        };
        match outcome {
            Panel::Deliver(code) => return code,
            Panel::Again => redraw_panel(state, io).await,
        }
    }
}

//! The portrait and equipment panel 0x128ce8 (Look, Talk, the 0x9e panel mode) and the pointer
//! direction 0x105016 that turns the leader toward a talk target.

use std::cell::RefCell;

use super::panel::draw_equipment;
use super::weight;
use crate::converse::{Cx, StrRef, actor_name, draw_portrait};
use crate::engine::Io;
use crate::main_loop::game_string;
use crate::objects::{READIED, Slot, display_name, first_child, next_child};
use crate::state::State;
use crate::ui::WindowId;
use crate::ui::panels::{figure_frame, select_window, set_cursor, sky_bar};
use crate::ui::printf::{Arg, printf};
use crate::ui::text::fill_rect;

/// `%s` newline (0x1283de, code hunk 15).
const STR_NAME_LINE: u32 = 0x0012_83de;
/// Readied types the panel ignores (0x151, 0x150).
const UNSHOWN: [u16; 2] = [0x151, 0x150];
/// Types whose quality byte names the portrait to show (0x128e62).
const PORTRAIT_BY_QUALITY: [u16; 4] = [0x189, 0x18d, 0x18e, 0x18f];
/// Monster types shown with their portrait like people (0x128e90).
const WITH_PORTRAIT: [u16; 3] = [0x175, 0x17e, 0x16b];

/// Original: 0x128ce8 (objects: an actor's portrait, readied equipment and name in the side
/// panel; waits for a key unless a conversation runs)
///
/// A party member opens as its member (panel member = its index), anyone else as an open
/// container. Readied objects (but 0x150 / 0x151) fill the equipment slots as 0x1112cc does.
/// Nothing is drawn for a temporary actor (0xe0..) with nothing readied unless its type has a
/// portrait. People and the 0x175 / 0x17e / 0x16b monsters show their portrait (beside the
/// equipment when they have some); others with equipment the figure frame and the equipment;
/// types 0x189 / 0x18d..0x18f the portrait their quality names. The name goes centred on row 10
/// (the converse name once the actor has been talked to); panel mode 0x9e while the key is read;
/// then the messages window, no open container, the old panel mode.
pub async fn show_actor(state: &RefCell<State>, io: &Io, actor: Slot) {
    let a = actor as usize & 0xff;
    let (saved_mode, equipped, show, special, portrait) = {
        let mut s = state.borrow_mut();
        let saved_mode = s.ui.panel.mode;
        s.ui.panel.container_flag = false;
        let in_party = s.actors.attrs.status[a] & 0x80 != 0;
        let mut member = 0usize;
        if in_party {
            s.ui.panel.open_container = 0;
            if let Some(i) = (0..16).find(|&i| Slot::from(s.actors.party.members[i]) == actor) {
                s.ui.panel.member = i as i16;
                member = i;
            }
        } else {
            s.ui.panel.open_container = actor as u16;
        }
        s.inventory.cells[super::EQUIP..].fill(0);
        let mut equipped = false;
        let mut c = first_child(&mut s.objects, actor);
        while c >= 0 {
            let ty = s.objects.kind(c);
            if s.objects.place(c) == READIED && !UNSHOWN.contains(&ty) {
                equipped = true;
                let mut slot = weight::ready_slot(&s, c);
                let inv = &s.inventory;
                if slot == 8 {
                    slot = 2;
                    s.inventory.set_equip(5, 1);
                    if in_party {
                        s.ui.panel.member_flags[member] = 1;
                    } else {
                        s.ui.panel.container_flag = true;
                    }
                } else if slot == 2 && inv.equip(2) != 0 {
                    slot = 5;
                } else if slot == 5 && inv.equip(5) != 0 {
                    slot = 2;
                } else if slot == 9 {
                    slot = if inv.equip(3) != 0 { 6 } else { 3 };
                }
                s.inventory.set_equip(slot, c);
            }
            c = next_child(&mut s.objects);
        }
        let ty = s.objects.kind(actor);
        let portrait = if PORTRAIT_BY_QUALITY.contains(&ty) {
            (s.objects.qq_of(actor) >> 8) as i16
        } else {
            0
        };
        let special = portrait == 0 && WITH_PORTRAIT.contains(&ty);
        let show = equipped || actor < 0xe0 || portrait != 0 || special;
        (saved_mode, equipped, show, special, portrait)
    };
    let saved_member = state.borrow().ui.panel.member;
    if show {
        {
            let s = state.borrow();
            let paper = s.ui.tables.colours.paper;
            io.draw(|fb| fill_rect(fb, 0xa8, 6, 0x137, 0x67, paper));
        }
        select_window(state, io, WindowId::Status).await;
        let mut s = state.borrow_mut();
        sky_bar(&mut s, io);
        let person = actor < 0xe0 || special;
        if equipped && person {
            draw_portrait(&s, io, actor, 0xf8, 0x18);
            figure_frame(&s, io, 0xc0, 0x28);
            draw_equipment(&s, io, 0, 8);
        } else if equipped {
            figure_frame(&s, io, 0xe0, 0x28);
            draw_equipment(&s, io, 0x20, 8);
        } else if person {
            draw_portrait(&s, io, actor, 0xd8, 0x18);
        } else if portrait != 0 {
            draw_portrait(&s, io, portrait, 0xd8, 0x18);
        }
        s.ui.panel.dirty = true;
    }
    state.borrow_mut().ui.panel.member = saved_member;
    if show {
        let who = if actor == 0 { 0x82 } else { actor };
        let met = state.borrow().actors.attrs.talk_flags[who as usize & 0xff] != 0;
        let name = if portrait != 0 || !met {
            display_name(&state.borrow(), who).into_bytes()
        } else {
            let r = actor_name(&Cx::new(state, io), who).await;
            let s = state.borrow();
            match r {
                StrRef::PartyName(i) => s
                    .actors
                    .party
                    .names
                    .get(usize::from(i))
                    .map_or_else(Vec::new, |n| n.as_bytes().to_vec()),
                _ => s.converse.name_buf.clone(),
            }
        };
        let d = 0x11 - name.len() as i16;
        let col = if d < 0 { (d + 1) >> 1 } else { d >> 1 };
        set_cursor(state, io, col, 10).await;
        let fmt = game_string(io, STR_NAME_LINE);
        printf(state, io, &fmt, &[Arg::Str(&name)]).await;
        let wait = {
            let mut s = state.borrow_mut();
            s.ui.panel.mode = 0x9e;
            !s.converse.active
        };
        if wait {
            Box::pin(crate::ui::wait::read_key(state, io)).await;
        }
    }
    select_window(state, io, WindowId::Messages).await;
    let mut s = state.borrow_mut();
    let p = &mut s.ui.panel;
    p.open_container = 0;
    p.container_flag = false;
    p.mode = saved_mode;
}

/// Original: 0x105016 (ui: the direction 0..7 (north, clockwise) of pixel (x, y) seen from the
/// pointer origin A4-0x7a26 / -0x7a25, -1 for the origin's own cell)
///
/// Within 24 pixels on both axes: the 3 × 3 cells of 16 pixels around the origin. Farther: a
/// straight direction when one axis is at least twice the other (or the other is 0), else the
/// diagonal of the quadrant.
pub fn direction_to(origin: (u8, u8), x: i16, y: i16) -> i16 {
    let (ox, oy) = (i16::from(origin.0), i16::from(origin.1));
    let (dx, dy) = ((x - ox).abs(), (y - oy).abs());
    if x >= ox - 0x18 && x < ox + 0x18 && y >= oy - 0x18 && y < oy + 0x18 {
        let row = |a: i16, b: i16, c: i16| {
            if y < oy - 8 {
                a
            } else if y < oy + 8 {
                b
            } else {
                c
            }
        };
        return if x < ox - 8 {
            row(7, 6, 5)
        } else if x < ox + 8 {
            row(0, -1, 4)
        } else {
            row(1, 2, 3)
        };
    }
    let straight = if dx > dy {
        (dy == 0 || dx / dy >= 2).then_some(if x >= ox { 2 } else { 6 })
    } else {
        (dx == 0 || dy / dx >= 2).then_some(if y >= oy { 4 } else { 0 })
    };
    straight.unwrap_or(match (x >= ox, y >= oy) {
        (true, true) => 3,
        (true, false) => 1,
        (false, true) => 5,
        (false, false) => 7,
    })
}

/// The step of Talk's start (0x114afa) after the target is chosen: the direction of the pointer
/// (0x105016 of A4-0x2f0a / -0x2f08); when there is one the leader faces it (actors 0x11ba5e)
/// and the view flags are rebuilt (map 0x109e74).
pub fn face_pointer(s: &mut State) {
    let (px, py) = s.ui.input.pointer;
    let dir = direction_to(s.inventory.pointer_origin, px, py);
    if dir >= 0 {
        let leader = s.actors.party.leader().map_or(0, |l| l.0);
        crate::actors::walk::face(s, leader, dir as u16);
        crate::map::view::update_view(s);
    }
}

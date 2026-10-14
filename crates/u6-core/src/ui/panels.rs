//! The side panel (screen pixels 168..312 × 6..103, the status window): the redraw 0x103f64 and
//! its three modes — party roster 0x1109e0, member stats 0x110d2c, inventory 0x111c30 — with the
//! combat-mode line 0x110c76 and the tile helpers they use.

use std::cell::RefCell;

use super::cursor::panel_cursor;
use super::globals::solo_member;
use super::printf::{Arg, printf, text_out};
use super::seams;
use super::text::{WindowId, fill_rect};
use crate::engine::Io;
use crate::state::State;
use crate::todo_stub;

/// The panel area 0x1109e0 / 0x110d2c / 0x111c30 clear (inclusive pixels).
const PANEL: (i32, i32, i32, i32) = (0xa8, 6, 0x138, 0x67);
/// Width of the status window in cells (0x110d2c centres names on 17).
const STATUS_WIDTH: i16 = 0x11;
/// The bottom row of panel icons: y, and x of the first.
const ICON_ROW_Y: i16 = 0x58;

/// Original: 0x107dd8 (ui: make `id` the current window; the pending line of the old one is
/// flushed before, the new one's colours become the glyph colours, then flushed again)
pub async fn select_window(state: &RefCell<State>, io: &Io, id: WindowId) {
    super::print::put_raw(state, io, -1).await;
    state.borrow_mut().ui.text.current = id;
    super::print::put_raw(state, io, -1).await;
}

/// Original: 0x107e00 (ui: move the current window's cursor, clamped to 0..=width / 0..=height,
/// flushing the pending line before and after)
pub async fn set_cursor(state: &RefCell<State>, io: &Io, col: i16, row: i16) {
    let (col, row) = {
        let s = state.borrow();
        let w = s.ui.text.win();
        (
            col.max(0).min(i16::from(w.width)),
            row.max(0).min(i16::from(w.height)),
        )
    };
    super::print::put_raw(state, io, -1).await;
    {
        let mut s = state.borrow_mut();
        let w = s.ui.text.win_mut();
        w.col = col as u8;
        w.row = row as u8;
    }
    super::print::put_raw(state, io, -1).await;
}

/// Clears the panel area to the paper colour (0x152176 + 0x1521f4 with A4-0x656c).
fn clear_panel(s: &State, io: &Io) {
    let paper = s.ui.tables.colours.paper;
    let (x0, y0, x1, y1) = PANEL;
    io.draw(|fb| fill_rect(fb, x0, y0, x1, y1, paper));
}

/// Original: 0x105c6a (ui: draw `tile` over `bg` at pixel (x, y); a forwarding wrapper of 0x10598a)
pub fn tile_on_blank_at(s: &State, io: &Io, tile: u16, bg: u16, x: i16, y: i16) {
    tile_on_blank(s, io, tile, bg, x, y);
}

/// Original: 0x10598a (ui: compose `tile` over `bg` at the top left of the other screen buffer and
/// copy the 16 × 16 result to (x, y))
pub fn tile_on_blank(s: &State, io: &Io, tile: u16, bg: u16, x: i16, y: i16) {
    io.with_buffers(|back, front| {
        seams::blit_tile_back(s, back, bg, 0, 0);
        seams::blit_tile_back(s, back, tile, 0, 0);
        seams::copy_rect(back, front, 0, 0, 0xf, 0xf, x, y);
    });
}

/// Original: 0x133fe0 (ui: the 2 × 2 tiles 0x170..0x173 of the inventory figure frame at (x, y))
pub fn figure_frame(s: &State, io: &Io, x: i16, y: i16) {
    io.draw(|fb| {
        seams::blit_tile(s, fb, 0x170, x, y);
        seams::blit_tile(s, fb, 0x171, x + 0x10, y);
        seams::blit_tile(s, fb, 0x172, x, y + 0x10);
        seams::blit_tile(s, fb, 0x173, x + 0x10, y + 0x10);
    });
}

/// The cell that centres `len` characters in the status window (`(17 - len) / 2`, rounded
/// toward zero as `asr` after the +1 of negative values).
fn centred(len: usize) -> i16 {
    half(STATUS_WIDTH - len as i16)
}

/// `d / 2` rounded toward zero (the `+1` before `asr` the compiler emits for negative values).
fn half(d: i16) -> i16 {
    if d < 0 { (d + 1) >> 1 } else { d >> 1 }
}

/// The roster name of party member `i` (14-byte names at A4+0x470c).
fn name(s: &State, i: i16) -> Vec<u8> {
    let names = &s.actors.party.names;
    usize::try_from(i)
        .ok()
        .and_then(|i| names.get(i))
        .map_or_else(Vec::new, |n| n.as_bytes().to_vec())
}

/// The actor slot of party member `i` (A4+0x47ec).
fn member_actor(s: &State, i: i16) -> u8 {
    usize::try_from(i)
        .ok()
        .and_then(|i| s.actors.party.members.get(i))
        .copied()
        .unwrap_or(0)
}

/// Original: 0x103f64 (ui: redraw the side panel for the panel mode, if it is marked dirty)
pub async fn redraw_panel(state: &RefCell<State>, io: &Io) {
    let mode = {
        let mut s = state.borrow_mut();
        if !s.ui.panel.dirty {
            return;
        }
        let solo = solo_member(&s.actors);
        if solo >= 0 {
            s.ui.panel.member = i16::from(solo);
        }
        s.ui.panel.mode
    };
    let member = state.borrow().ui.panel.member;
    match mode {
        0x91 => {
            let first = state.borrow().ui.panel.roster_first;
            roster(state, io, i16::from(first)).await;
        }
        0x90 => stats(state, io, member).await,
        0x92 => inventory(state, io).await,
        0x9e => {
            // Contract: draw the contents panel of member `member`'s actor (objects). Fallback:
            // the panel keeps its old picture.
            todo_stub!("0x128ce8 container contents panel", ());
        }
        _ => {}
    }
    panel_cursor(state, io, 1);
    state.borrow_mut().ui.panel.dirty = false;
}

/// The ink of a member's hit points: the poisoned colour (status bit 3), the alert colour below
/// 10 hit points, else unchanged (0x110ad6 / 0x110e70).
fn hp_ink(s: &mut State, actor: u8) {
    let (status, hp) = (
        s.actors.attrs.status[usize::from(actor)],
        s.actors.attrs.hp[usize::from(actor)],
    );
    let c = s.ui.tables.colours;
    let w = s.ui.text.win_mut();
    if status & 0x08 != 0 {
        w.fg = c.poisoned;
    } else if hp < 10 {
        w.fg = c.alert;
    }
}

/// Original: 0x1109e0 (ui: party roster panel: up to five members from `first`, each with its
/// figure, hit points and name; arrows when the list scrolls)
pub async fn roster(state: &RefCell<State>, io: &Io, first: i16) {
    {
        let mut s = state.borrow_mut();
        clear_panel(&s, io);
        sky_bar(&mut s, io);
    }
    select_window(state, io, WindowId::Screen).await;
    let size = i16::from(state.borrow().actors.party.size);
    if first != 0 {
        set_cursor(state, io, 0x15, 3).await;
        text_out(state, io, 0x18).await;
    }
    if first < 16 && size > first + 5 {
        set_cursor(state, io, 0x15, 0x0c).await;
        text_out(state, io, 0x19).await;
    }
    select_window(state, io, WindowId::Status).await;
    let mut i = first;
    while i < size && i < first + 5 {
        let k = i - first;
        let (actor, hp, fmt) = {
            let s = state.borrow();
            let actor = member_actor(&s, i);
            let ty = s.actors.attrs.base_type_frame[usize::from(actor)] & 0x3ff;
            let base = s
                .tables
                .basetile
                .base
                .get(usize::from(ty))
                .copied()
                .unwrap_or(0);
            let tile = base.wrapping_add(seams::facing_frame(ty, 4) as u16);
            tile_on_blank_at(&s, io, tile, 0x19b, 0xb0, k * 16 + 0x18);
            let hp = s.actors.attrs.hp[usize::from(actor)];
            (actor, hp, s.ui.tables.panel.roster_hp.clone())
        };
        set_cursor(state, io, 0x0e, k * 2 + 2).await;
        hp_ink(&mut state.borrow_mut(), actor);
        printf(state, io, &fmt, &[Arg::Word(u16::from(hp))]).await;
        {
            let mut s = state.borrow_mut();
            let ink = s.ui.tables.colours.text;
            s.ui.text.win_mut().fg = ink;
        }
        set_cursor(state, io, 3, k * 2 + 3).await;
        let (fmt, n) = {
            let s = state.borrow();
            (s.ui.tables.panel.name_line.clone(), name(&s, i))
        };
        printf(state, io, &fmt, &[Arg::Str(&n)]).await;
        i += 1;
    }
    select_window(state, io, WindowId::Messages).await;
}

/// Original: 0x110c76 (ui: the combat-mode name of the shown member, centred on row 11 of the
/// status window from column 10)
pub async fn combat_mode_line(state: &RefCell<State>, io: &Io) {
    let saved = state.borrow().ui.text.current;
    select_window(state, io, WindowId::Status).await;
    let (name, fmt) = {
        let s = state.borrow();
        let paper = s.ui.tables.colours.paper;
        io.draw(|fb| fill_rect(fb, 0x100, 0x60, 0x137, 0x67, paper));
        let actor = member_actor(&s, s.ui.panel.member);
        let mode = s.actors.attrs.combat_mode[usize::from(actor)];
        let name = s.ui.tables.combat_mode_name(mode).to_vec();
        (name, s.ui.tables.panel.plain.clone())
    };
    let col = half(8 - name.len() as i16) + 10;
    set_cursor(state, io, col, 0x0b).await;
    printf(state, io, &fmt, &[Arg::Str(&name)]).await;
    select_window(state, io, saved).await;
}

/// One `printf` at a cell of the current window.
async fn at(state: &RefCell<State>, io: &Io, col: i16, row: i16, fmt: &[u8], args: &[Arg<'_>]) {
    set_cursor(state, io, col, row).await;
    printf(state, io, fmt, args).await;
}

/// The bottom icons of the stats and inventory panels: previous member (not for the first),
/// the panel tiles `mid`, next member (not for the last).
fn icon_row(s: &State, io: &Io, member: i16, mid: [u16; 2]) {
    let size = i16::from(s.actors.party.size);
    io.draw(|fb| {
        if member > 0 {
            seams::blit_tile(s, fb, 0x183, 0xb0, ICON_ROW_Y);
        }
        seams::blit_tile(s, fb, mid[0], 0xc0, ICON_ROW_Y);
        seams::blit_tile(s, fb, mid[1], 0xd0, ICON_ROW_Y);
        if member < size - 1 {
            seams::blit_tile(s, fb, 0x184, 0xe0, ICON_ROW_Y);
        }
    });
}

/// Original: 0x110d2c (ui: member stats panel: name, portrait, STR / DEX / INT, magic, health,
/// level / experience, the bottom icons)
pub async fn stats(state: &RefCell<State>, io: &Io, member: i16) {
    let (n, actor, p) = {
        let s = state.borrow();
        clear_panel(&s, io);
        (
            name(&s, member),
            member_actor(&s, member),
            s.ui.tables.panel.clone(),
        )
    };
    select_window(state, io, WindowId::Status).await;
    at(
        state,
        io,
        centred(n.len()),
        0,
        &p.name_line,
        &[Arg::Str(&n)],
    )
    .await;
    seams::draw_portrait(&state.borrow(), io, actor, 0xb0, 0x10);
    let (st, dx, it, mp, mmp, hp, mhp, lv, xp) = {
        let s = state.borrow();
        let a = &s.actors.attrs;
        let i = usize::from(actor);
        (
            a.strength[i],
            a.dexterity[i],
            a.intelligence[i],
            a.magic[i],
            seams::max_magic(&s, actor),
            a.hp[i],
            seams::max_hp(&s, actor),
            seams::level(&s, actor),
            a.exp[i],
        )
    };
    let w = |v: u8| Arg::Word(u16::from(v));
    at(state, io, 10, 2, &p.attributes[0], &[w(st)]).await;
    at(state, io, 10, 3, &p.attributes[1], &[w(dx)]).await;
    at(state, io, 10, 4, &p.attributes[2], &[w(it)]).await;
    at(state, io, 10, 6, &p.magic, &[]).await;
    at(state, io, 10, 7, &p.magic_value, &[w(mp), Arg::Word(mmp)]).await;
    at(state, io, 10, 8, &p.health, &[]).await;
    set_cursor(state, io, 10, 9).await;
    hp_ink(&mut state.borrow_mut(), actor);
    printf(state, io, &p.hp_value, &[w(hp)]).await;
    {
        let mut s = state.borrow_mut();
        let ink = s.ui.tables.colours.text;
        s.ui.text.win_mut().fg = ink;
    }
    printf(state, io, &p.hp_max, &[w(mhp)]).await;
    at(state, io, 10, 10, &p.level_exp, &[]).await;
    at(
        state,
        io,
        10,
        11,
        &p.level_exp_value,
        &[w(lv), Arg::Word(xp)],
    )
    .await;
    {
        let s = state.borrow();
        let shown = s.ui.panel.member;
        icon_row(&s, io, shown, [0x180, 0x182]);
    }
    select_window(state, io, WindowId::Messages).await;
}

/// Original: 0x111c30 (inventory: inventory panel) [partial]
///
/// Name, figure frame, the weight labels, the bottom icons; the equipment and the list are
/// drawn by 0x111be0 (not ported: declared stub).
pub async fn inventory(state: &RefCell<State>, io: &Io) {
    let (open, member, n, p) = {
        let mut s = state.borrow_mut();
        s.ui.panel.inventory_first = 0;
        clear_panel(&s, io);
        let member = s.ui.panel.member;
        (
            s.ui.panel.open_container,
            member,
            name(&s, member),
            s.ui.tables.panel.clone(),
        )
    };
    select_window(state, io, WindowId::Status).await;
    if open == 0 {
        at(
            state,
            io,
            centred(n.len()),
            0,
            &p.name_line,
            &[Arg::Str(&n)],
        )
        .await;
    }
    figure_frame(&state.borrow(), io, 0xc0, 0x20);
    at(state, io, 0, 9, &p.weights[0], &[]).await;
    at(state, io, 9, 9, &p.weights[1], &[]).await;
    {
        let mut s = state.borrow_mut();
        if open == 0 {
            icon_row(&s, io, member, [0x180, 0x181]);
            if member != 0 {
                io.draw(|fb| seams::blit_tile(&s, fb, 0x187, 0xf0, ICON_ROW_Y));
            }
        }
        let shown = if open == 0 {
            u16::from(member_actor(&s, member))
        } else {
            open
        };
        s.ui.panel.container = shown;
        if let Some(f) = usize::try_from(member)
            .ok()
            .and_then(|m| s.ui.panel.member_flags.get_mut(m))
        {
            *f = 0;
        }
    }
    // Contract: draw the equipment of the shown actor or container and its inventory list.
    // Fallback: the panel shows the frame and labels only.
    todo_stub!("0x111be0 inventory panel contents", ());
    combat_mode_line(state, io).await;
    select_window(state, io, WindowId::Messages).await;
}

/// Bytes of the objlist block at A4-0x7b84 the sky bar reads.
const TAIL_ECLIPSE: usize = 0x21;
const TAIL_MOONS: [(usize, usize); 2] = [(0x7d, 0x7c), (0x7f, 0x7e)];
/// Object type whose base tile is the first moon phase (0x13a9d2 = basetile 0x13a940 + 2 × 0x49).
const MOON_TYPE: usize = 0x49;

/// Original: 0x133b28 (ui: the sky bar over the panel in roster mode)
///
/// Sun or eclipse by the hour, the two moons by phase and position; a stone ceiling underground.
///
/// Composed at the top of the other screen buffer and copied to (176, 6). The tile blits and
/// the copy are `gfx/` seams; the final call 0x103632 (main) is a seam too.
pub fn sky_bar(s: &mut State, io: &Io) {
    let mode = s.ui.panel.mode;
    if mode != 0x91 && mode != 0x9e {
        return;
    }
    let cal = s.actors.calendar;
    let tail = |i: usize| s.actors.objlist_tail.get(i).copied().unwrap_or(0);
    let eclipse = (cal.day == 1 && cal.month.is_multiple_of(3)) || tail(TAIL_ECLIPSE) != 0;
    let moons = TAIL_MOONS.map(|(pos, phase)| (tail(pos), tail(phase)));
    let heights = s.ui.tables.sky_heights;
    let moon_base = s.tables.basetile.base.get(MOON_TYPE).copied().unwrap_or(0);
    let z = s.map.centre.z;
    let st = &*s;
    io.with_buffers(|back, front| {
        for i in 0..9 {
            seams::blit_tile_back(st, back, 0x19b, i * 16, 4);
        }
        if z == 0 || z == 5 {
            let hour = cal.hour;
            if hour > 4 && hour < 20 {
                let tile = if eclipse {
                    0x16b
                } else if hour == 5 || hour == 19 {
                    0x169
                } else {
                    0x16a
                };
                let p = usize::from(19 - hour);
                seams::blit_tile_back(
                    st,
                    back,
                    tile,
                    (19 - i16::from(hour)) * 8,
                    i16::from(heights[p]),
                );
            }
            if !eclipse {
                for (pos, phase) in moons {
                    if pos <= 14 {
                        let tile = moon_base.wrapping_add(u16::from(phase));
                        let y = i16::from(heights[usize::from(pos)]);
                        seams::blit_tile_back(st, back, tile, i16::from(pos) * 8, y);
                    }
                }
            }
            for i in 0..9 {
                seams::blit_tile_back(st, back, 0x160 + i as u16, i * 16, 4);
            }
        } else {
            seams::blit_tile_back(st, back, 0x174, 0, 4);
            for i in 1..8 {
                seams::blit_tile_back(st, back, 0x175, i * 16, 4);
            }
            seams::blit_tile_back(st, back, 0x176, 0x80, 4);
        }
        seams::copy_rect(back, front, 0, 4, 0x87, 0x13, 0xb0, 6);
    });
    seams::sky_bar_tail(s);
}

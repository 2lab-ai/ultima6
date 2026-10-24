//! The side panel (screen pixels 168..312 × 6..103, the status window): the redraw 0x103f64 and
//! its three modes — party roster 0x1109e0, member stats 0x110d2c, inventory 0x111c30 — with the
//! combat-mode line 0x110c76 and the tile helpers they use. Wide (spec 14): the three views drawn
//! into their slots below the UI at every redraw, and the presses in those slots.

use std::cell::RefCell;

use super::cursor::panel_cursor;
use super::globals::solo_member;
use super::printf::{Arg, printf, text_out};
use super::seams;
use super::text::{WindowId, fill_rect};
use crate::engine::Io;
use crate::state::State;

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
    io.draw(|fb| fill_rect(fb, x0, y0, x1, y1, paper, s.ui.paper.as_ref()));
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

/// The roster name of party member `i` (14-byte names at A4+0x470c). Spec 14 Design 6: the page
/// titles the actor windows with the shown member's ([`panel_view`]); it is shown, never logged.
pub fn name(s: &State, i: i16) -> Vec<u8> {
    let names = &s.actors.party.names;
    usize::try_from(i)
        .ok()
        .and_then(|i| names.get(i))
        .map_or_else(Vec::new, |n| n.as_bytes().to_vec())
}

/// The actor slot of party member `i` (A4+0x47ec).
pub fn member_actor(s: &State, i: i16) -> u8 {
    usize::try_from(i)
        .ok()
        .and_then(|i| s.actors.party.members.get(i))
        .copied()
        .unwrap_or(0)
}

/// Spec 14 Design 7: what the page's actor windows show: the panel mode (the active view while
/// it is one of the three and no conversation runs), the party member the panel shows, that
/// member's actor slot, and whether a conversation runs (its portrait is in the panel area,
/// Design 6). The windows' titles carry the member's name ([`panel_name`]).
pub fn panel_view(s: &State) -> (u16, i16, u8, bool) {
    let member = s.ui.panel.member;
    (
        s.ui.panel.mode,
        member,
        member_actor(s, member),
        s.converse.active,
    )
}

/// Spec 14 Design 7: the roster name of the member the panel shows ([`name`]).
pub fn panel_name(s: &State) -> Vec<u8> {
    name(s, s.ui.panel.member)
}

/// Original: 0x103f64 (ui: redraw the side panel for the panel mode, if it is marked dirty)
///
/// Wide (spec 14 Design 2): in the roster, stats or inventory mode all three views are drawn,
/// each into its slot ([`redraw_views`]). Not while a conversation runs (Design 6): its portrait
/// stays in the panel area, the panel stays dirty, and the redraw after the Talk command draws
/// it.
pub async fn redraw_panel(state: &RefCell<State>, io: &Io) {
    let Some(mode) = redraw_mode(state) else {
        return;
    };
    #[cfg(feature = "wide")]
    if let Some(slot) = slot_of(mode) {
        if !state.borrow().converse.active {
            redraw_views(state, io, mode, slot).await;
        }
        return;
    }
    draw_mode(state, io, mode).await;
    panel_cursor(state, io, 1);
    state.borrow_mut().ui.panel.dirty = false;
}

/// The panel mode 0x103f64 draws, the solo member made the one shown; `None` when the panel is
/// not dirty.
fn redraw_mode(state: &RefCell<State>) -> Option<u16> {
    let mut s = state.borrow_mut();
    if !s.ui.panel.dirty {
        return None;
    }
    let solo = solo_member(&s.actors);
    if solo >= 0 {
        s.ui.panel.member = i16::from(solo);
    }
    Some(s.ui.panel.mode)
}

/// 0x103f64's draw of panel mode `mode` (nothing for a mode it has no case for).
async fn draw_mode(state: &RefCell<State>, io: &Io, mode: u16) {
    let member = state.borrow().ui.panel.member;
    match mode {
        0x91 => {
            let first = state.borrow().ui.panel.roster_first;
            roster(state, io, i16::from(first)).await;
        }
        0x90 => stats(state, io, member).await,
        0x92 => inventory(state, io).await,
        0x9e => {
            let actor = member_actor(&state.borrow(), member);
            Box::pin(crate::inventory::show_actor(state, io, i16::from(actor))).await;
        }
        _ => {}
    }
}

/// The views of the slots, by slot ([`crate::geom::SLOTS`]): roster, stats, inventory.
#[cfg(feature = "wide")]
pub const VIEWS: [u16; 3] = [0x91, 0x90, 0x92];

/// The slot of view `mode`, if it is one of [`VIEWS`].
#[cfg(feature = "wide")]
pub fn slot_of(mode: u16) -> Option<usize> {
    VIEWS.iter().position(|&v| v == mode)
}

/// Spec 14 Design 2 (Wide): 0x103f64 in the roster, stats or inventory mode `mode` (slot
/// `slot`). The two other views are drawn into the panel area first, each with its mode set, and
/// copied into its slot; then `mode`'s as the original draws it, its cursor, its copy.
///
/// The other views leave the state as a draw of `mode` alone would, but for the inventory
/// context (Design 3: the page and the container, which the inventory view keeps): the panel
/// area is cleared by each view and drawn last by `mode`'s; the pending line is flushed once
/// before them, as `mode`'s first window switch would, and what they write besides is put back
/// after each ([`Drawn`]). The inventory view's cells are kept for a press in its slot.
#[cfg(feature = "wide")]
async fn redraw_views(state: &RefCell<State>, io: &Io, mode: u16, slot: usize) {
    super::print::put_raw(state, io, -1).await;
    for (other, &view) in VIEWS.iter().enumerate() {
        if other == slot {
            continue;
        }
        let saved = Drawn::of(&state.borrow());
        state.borrow_mut().ui.panel.mode = view;
        draw_mode(state, io, view).await;
        copy_to_slot(io, other);
        let mut s = state.borrow_mut();
        if view == 0x92 {
            let cells = super::globals::ViewCells::of(&s);
            s.ui.panel.slots.inventory = cells;
        }
        saved.put(&mut s);
    }
    draw_mode(state, io, mode).await;
    panel_cursor(state, io, 1);
    copy_to_slot(io, slot);
    let mut s = state.borrow_mut();
    if mode == 0x92 {
        let cells = super::globals::ViewCells::of(&s);
        s.ui.panel.slots.inventory = cells;
    }
    s.ui.panel.dirty = false;
}

/// What a view's draw writes outside the panel area and the inventory context (spec 14 Design
/// 2), as it was before: the text layer (windows, cursors, inks, the line; 0x107dd8, 0x107e00,
/// 0x107e5a), the speech tap's marks of the line's cells, the plot colour and buffer (0x1059d4),
/// the child cursor (0x10aea4 / 0x10aed6), the wind (the sky bar's 0x103632), the random state,
/// the inventory view's cells, count, weights and both-hands flags (0x1112cc, 0x1118c6,
/// 0x111c30), and the panel mode.
#[cfg(feature = "wide")]
struct Drawn {
    text: super::TextState,
    speech: crate::speech::SpeechTap,
    plot: (u16, bool),
    children: crate::objects::ChildCursor,
    wind: u8,
    rng: crate::Rng,
    cells: super::globals::ViewCells,
    mode: u16,
}

#[cfg(feature = "wide")]
impl Drawn {
    fn of(s: &State) -> Drawn {
        Drawn {
            text: s.ui.text.clone(),
            speech: s.speech.clone(),
            plot: (s.gfx.plot_colour, s.gfx.plot_back),
            children: s.objects.children,
            wind: s.actors.wind,
            rng: s.rng,
            cells: super::globals::ViewCells::of(s),
            mode: s.ui.panel.mode,
        }
    }

    fn put(self, s: &mut State) {
        s.ui.text = self.text;
        s.speech = self.speech;
        (s.gfx.plot_colour, s.gfx.plot_back) = self.plot;
        s.objects.children = self.children;
        s.actors.wind = self.wind;
        s.rng = self.rng;
        self.cells.put(s);
        s.ui.panel.mode = self.mode;
    }
}

/// Each slot lies a multiple of 8 pixels from the panel area on both axes, so the text copied
/// into it stays on the HD font's 8-pixel grid (spec 14 rev 3, `u6_render::hd` `glyph_cell`).
const _: () = assert!(
    slots_on_grid(),
    "the slots on the panel's 8-pixel text grid"
);

/// The check of the assertion above.
const fn slots_on_grid() -> bool {
    let slots = crate::geom::SLOTS;
    let mut i = 0;
    while i < slots.len() {
        let (dx, dy) = (slots[i].0 as i32 - PANEL.0, slots[i].1 as i32 - PANEL.1);
        if dx % 8 != 0 || dy % 8 != 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// Spec 14 Design 2: the side panel area (144 × 98 from [`PANEL`]'s top left) copied into slot
/// `slot`, each pixel with its source ([`u6_render::Framebuffer::copy_px_within`]).
#[cfg(feature = "wide")]
fn copy_to_slot(io: &Io, slot: usize) {
    use crate::geom::{SLOT_H, SLOT_W, SLOTS};
    use u6_render::WIDTH;
    let (x0, y0) = (PANEL.0 as usize, PANEL.1 as usize);
    let (sx, sy) = (usize::from(SLOTS[slot].0), usize::from(SLOTS[slot].1));
    io.draw(|fb| {
        for dy in 0..usize::from(SLOT_H) {
            for dx in 0..usize::from(SLOT_W) {
                fb.copy_px_within((y0 + dy) * WIDTH + x0 + dx, (sy + dy) * WIDTH + sx + dx);
            }
        }
    });
}

/// Spec 14 Design 4 (Wide): a draw into the panel area outside a redraw (the panel cursor, the
/// combat-mode line) reaches the slot of the view shown there. Not while a slot press holds
/// another view's mode (the panel area then shows the panel's own view, and the press ends with
/// a redraw), nor while a conversation runs (the panel area holds its portrait or the
/// inventory it opened, Design 6).
#[cfg(feature = "wide")]
pub(crate) fn mirror_view(s: &State, io: &Io) {
    if s.ui.panel.slots.press.is_some() || s.converse.active {
        return;
    }
    if let Some(slot) = slot_of(s.ui.panel.mode) {
        copy_to_slot(io, slot);
    }
}

/// Spec 14 Design 5 (Wide): the start of a press in slot `slot`, the pointer in it. Ignored
/// (false) unless it is a press, the panel mode is one of [`VIEWS`], no conversation runs
/// (Design 6) and no other slot press is still being handled. Else: the pointer moved to the
/// same place in the panel area, the panel mode the slot's view, for the inventory slot while
/// another view is the panel's the inventory state its last draw left (the cells its click
/// reads, as the panel's own would after that draw), and the press recorded to be put back
/// ([`end_slot_press`]).
#[cfg(feature = "wide")]
pub(crate) fn begin_slot_press(s: &mut State, slot: usize) -> bool {
    let mode = s.ui.panel.mode;
    if !s.ui.pointer.pressed
        || slot_of(mode).is_none()
        || s.converse.active
        || s.ui.panel.slots.press.is_some()
    {
        return false;
    }
    let view = VIEWS[slot];
    let (sx, sy) = crate::geom::SLOTS[slot];
    let pointer = s.ui.input.pointer;
    s.ui.input.pointer = (
        pointer.0 - sx as i16 + PANEL.0 as i16,
        pointer.1 - sy as i16 + PANEL.1 as i16,
    );
    let cells = (view == 0x92 && mode != 0x92).then(|| {
        let live = super::globals::ViewCells::of(s);
        s.ui.panel.slots.inventory.clone().put(s);
        live
    });
    s.ui.panel.mode = view;
    s.ui.panel.slots.press = Some(super::globals::SlotPress {
        mode,
        view,
        pointer,
        cells,
        claimed: false,
    });
    true
}

/// The key read takes the slot press that gave the code it just read, if one did (true): the
/// press is put back once that code is handled ([`end_slot_press`]). A press taken by an outer
/// key read is not this one's.
#[cfg(feature = "wide")]
pub(crate) fn claim_slot_press(s: &mut State) -> bool {
    match &mut s.ui.panel.slots.press {
        Some(p) if !p.claimed => {
            p.claimed = true;
            true
        }
        _ => false,
    }
}

/// Spec 14 Design 5 (Wide): the end of a slot press, its code handled: the panel mode back
/// unless the code changed it (the codes switch the panel as today), the inventory state the
/// press replaced back, the pointer back where the press was, the panel dirty.
#[cfg(feature = "wide")]
pub(crate) fn end_slot_press(s: &mut State) {
    let Some(p) = s.ui.panel.slots.press.take() else {
        return;
    };
    if s.ui.panel.mode == p.view {
        s.ui.panel.mode = p.mode;
    }
    if let Some(cells) = p.cells {
        cells.put(s);
    }
    s.ui.input.pointer = p.pointer;
    s.ui.panel.dirty = true;
}

/// A redraw of the current view alone, as 0x103f64 draws it in Original: what spec 14 Design
/// 2's redraw is compared with.
#[cfg(all(test, feature = "wide"))]
pub(crate) async fn redraw_alone(state: &RefCell<State>, io: &Io) {
    let Some(mode) = redraw_mode(state) else {
        return;
    };
    draw_mode(state, io, mode).await;
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
        let map = s.ui.paper.as_ref();
        io.draw(|fb| fill_rect(fb, 0x100, 0x60, 0x137, 0x67, paper, map));
        let actor = member_actor(&s, s.ui.panel.member);
        let mode = s.actors.attrs.combat_mode[usize::from(actor)];
        let name = s.ui.tables.combat_mode_name(mode).to_vec();
        (name, s.ui.tables.panel.plain.clone())
    };
    let col = half(8 - name.len() as i16) + 10;
    set_cursor(state, io, col, 0x0b).await;
    printf(state, io, &fmt, &[Arg::Str(&name)]).await;
    select_window(state, io, saved).await;
    #[cfg(feature = "wide")]
    mirror_view(&state.borrow(), io);
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

/// Original: 0x111c30 (inventory: inventory panel)
///
/// Name, figure frame, the weight labels, the bottom icons; the equipment, the list and the
/// weights are drawn by 0x111be0 ([`crate::inventory::panel_contents`]).
///
/// Wide (spec 14 Design 3): the page and the container shown stay while the member shown (or
/// the open container) does: a bag opened in the panel (0xa0) stays open and a scrolled list on
/// its page through every redraw. When it changes (0x90, 0x92, 0x99, 0x9a, 0x91, a solo member,
/// Drop's or Tab's leader, a conversation) they start again as the original's
/// ([`kept_container`]).
pub async fn inventory(state: &RefCell<State>, io: &Io) {
    let (open, member, n, p) = {
        let mut s = state.borrow_mut();
        first_row(&mut s);
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
    let shown = {
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
        #[cfg(feature = "wide")]
        let (base, shown) = (shown, kept_container(&mut s, shown));
        s.ui.panel.container = shown;
        if let Some(f) = usize::try_from(member)
            .ok()
            .and_then(|m| s.ui.panel.member_flags.get_mut(m))
        {
            *f = 0;
        }
        // A kept bag: the equipment, the readied weight and the both-hands flag are the
        // member's, as 0x111c30 left them before the bag opened (0x9f / 0xa0 recount the bag
        // only, which keeps them).
        #[cfg(feature = "wide")]
        if shown != base {
            crate::inventory::recount(&mut s, base as i16);
        }
        shown
    };
    crate::inventory::panel_contents(state, io, shown as i16).await;
    combat_mode_line(state, io).await;
    select_window(state, io, WindowId::Messages).await;
}

/// 0x111c30 lists from the first row. Wide (spec 14 Design 3): the row is kept, and starts again
/// with the container ([`kept_container`]).
fn first_row(s: &mut State) {
    if !cfg!(feature = "wide") {
        s.ui.panel.inventory_first = 0;
    }
}

/// Spec 14 Design 3 (Wide): the container the inventory view shows for `base`, the member's actor
/// or the open container: the panel's container as the last draw left it, the page kept, while
/// `base` is the last draw's and that container is `base` itself, or still the object the last
/// draw showed (the same type and frame word in its slot) held in `base` ([`held_in`]). Else
/// `base`, from the first page: the member shown changed, there was no last draw (the first,
/// after a Restore or a Save), the container left `base` (taken by another actor, left on the
/// map), or its slot was freed or handed to another object (its block written back as the
/// region moved, and read again). The container shown and its word are recorded.
#[cfg(feature = "wide")]
fn kept_container(s: &mut State, base: u16) -> u16 {
    let kept = s.ui.panel.container;
    let o = &s.objects;
    let slots = &s.ui.panel.slots;
    let same_object = || o.tf(kept as crate::objects::Slot) == slots.tf && held_in(o, kept, base);
    let keep = slots.base == Some(base) && (kept == base || same_object());
    let shown = if keep {
        kept
    } else {
        s.ui.panel.inventory_first = 0;
        base
    };
    s.ui.panel.slots.base = Some(base);
    s.ui.panel.slots.tf = s.objects.tf(shown as crate::objects::Slot);
    shown
}

/// `obj` is `base`, or its chain of containers reaches `base` as `inventory::weight::holder`
/// walks it (`inventory/weight.rs`), through objects that are there (a type and frame word not
/// 0: a slot freed by a block's write-back keeps the rest of its entry). False when the chain
/// ends elsewhere: at another actor, on the map, at a freed slot, or at the walk's bound (a
/// chain that comes back on itself).
#[cfg(feature = "wide")]
fn held_in(o: &crate::objects::Objects, obj: u16, base: u16) -> bool {
    use crate::objects::{ACTOR_SLOTS, OBJECT_SLOTS, Slot};
    let mut c = obj as Slot;
    for _ in 0..OBJECT_SLOTS {
        if c == base as Slot {
            return true;
        }
        if c < ACTOR_SLOTS as Slot || o.place(c) == 0 || o.tf(c) == 0 {
            return false;
        }
        c = o.parent(c);
    }
    false
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

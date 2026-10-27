//! The inventory panel's contents (hunk 8): equipment cells around the figure, the 4 × 3 list,
//! its scroll arrows and the weight lines, with the icon and number drawing of hunk 2.

use std::cell::RefCell;

use super::{CELLS, EQUIP, weight};
use crate::engine::Io;
use crate::gfx::blit::{blit_back, blit_front, copy_rect, plot, set_plot_colour};
use crate::main_loop::game_string;
use crate::objects::{IN_CONTAINER, IN_INVENTORY, Slot, first_child, next_child, stack_kind};
use crate::state::State;
use crate::ui::WindowId;
use crate::ui::panels::{select_window, set_cursor, tile_on_blank, tile_on_blank_at};
use crate::ui::printf::{Arg, printf, text_out};
use crate::ui::text::fill_rect;

/// Image strings 0x1118c6 prints (code hunk 8): six blanks, `%d%/%ds`.
const STR_BLANK: u32 = 0x0011_0bfc;
const STR_WEIGHT: u32 = 0x0011_0c04;

/// Port-only (spec 16 item 1, 2026-10-27): the strings the weight line ([`draw_list`]'s 0x1118c6)
/// passes to `printf` as its format, for the locale layer's catalogue
/// ([`crate::locale::strings`]): the blank that clears each value's field (pushed at 0x111b5c and
/// 0x111b96; no word of its own) and the values' format (0x111b86, 0x111bc6), each at
/// status-window cells (2, 9) and (0xb, 9) after the blank cleared 6 cells there: the format
/// within its English's cells, as the panels' formats ([`crate::ui::tables::PRINTED`]).
pub(crate) const PRINTED: &[u32] = &[STR_BLANK, STR_WEIGHT];
/// Background, empty-cell and both-hands tiles.
const BLANK: u16 = 0x19b;
const EMPTY: u16 = 0x19a;
const BOTH_HANDS: u16 = 0x185;
/// Object type whose icon shows its quality as a circled number (0x105b14).
const NUMBERED: u16 = 0x40;
/// Cells of the paper doll, slot 0..7, as offsets from the panel origin (0x1113fe's jump table).
const DOLL: [(i16, i16); 8] = [
    (0xc8, 0x10),
    (0xb0, 0x18),
    (0xb0, 0x28),
    (0xb0, 0x38),
    (0xe0, 0x18),
    (0xe0, 0x28),
    (0xe0, 0x38),
    (0xc8, 0x40),
];

/// The tile an object shows (base tile of its type + frame).
pub(crate) fn object_tile(s: &State, obj: Slot) -> u16 {
    let o = &s.objects;
    let base = s
        .tables
        .basetile
        .base
        .get(usize::from(o.kind(obj)))
        .copied()
        .unwrap_or(0);
    base.wrapping_add(o.tf(obj) >> 10)
}

/// The panel shows a both-hands weapon: the member's flag A4+0x1370, or A4+0x1382 when a
/// container (or a non-member) is open.
fn both_hands(s: &State) -> bool {
    let p = &s.ui.panel;
    if p.open_container == 0 {
        p.member_flag(p.member)
    } else {
        p.container_flag
    }
}

/// Original: 0x111be0 (ui: the panel contents of `obj`: clear the list cells (and for an actor
/// the equipment), recount with 0x1112cc, redraw with 0x1118c6)
pub async fn panel_contents(state: &RefCell<State>, io: &Io, obj: Slot) {
    recount(&mut state.borrow_mut(), obj);
    draw_list(state, io).await;
}

/// The part of 0x111be0 before the drawing: the list cells (and for an actor the equipment)
/// cleared and refilled by 0x1112cc. Synchronous callers run this and leave the drawing
/// (0x1118c6) to the next panel redraw.
pub fn recount(s: &mut State, obj: Slot) {
    let inv = &mut s.inventory;
    inv.cells[..EQUIP].fill(0);
    if obj < 0x100 {
        inv.cells[EQUIP..CELLS].fill(0);
    }
    weight::carried_weight(s, obj);
}

/// Original: 0x1113fe (ui: the eight equipment cells, offset by (x0, y0))
///
/// A readied object over the blank tile; an empty slot the empty-cell tile; slot 5 the
/// both-hands tile while a two-handed weapon is readied (with the marker 1 there or not).
pub fn draw_equipment(s: &State, io: &Io, x0: i16, y0: i16) {
    for (slot, &(dx, dy)) in DOLL.iter().enumerate() {
        let obj = s.inventory.equip(slot as i16);
        let (x, y) = (x0 + dx, y0 + dy);
        let marked = slot == 5 && both_hands(s);
        if marked {
            io.draw(|fb| blit_front(&s.gfx, fb, BOTH_HANDS, x as u16, y as u16));
        } else if obj != 0 {
            tile_on_blank_at(s, io, object_tile(s, obj), BLANK, x, y);
        } else {
            io.draw(|fb| blit_front(&s.gfx, fb, EMPTY, x as u16, y as u16));
        }
    }
}

/// Original: 0x1059d4 (ui: digit `d` of the 3 × 5 font at column `x`, rows 11..15, plotted into
/// the other screen buffer in the text colour)
///
/// The glyph word is read top row first, three bits per row, from bit 15. A digit outside the
/// 24 glyphs (the original reads beyond the table) plots nothing.
pub fn digit(s: &mut State, io: &Io, d: i16, x: i16) {
    s.gfx.plot_back = true;
    let mut bits = usize::try_from(d)
        .ok()
        .and_then(|i| s.inventory.tables.glyphs.get(i))
        .copied()
        .unwrap_or(0);
    let colour = s.ui.tables.colours.text;
    set_plot_colour(&mut s.gfx, u16::from(colour));
    let gfx = &s.gfx;
    io.draw_back(|back| {
        for y in 11..16 {
            for px in x..x + 3 {
                if bits & 0x8000 != 0 {
                    plot(gfx, back, px as u16, y);
                }
                bits <<= 1;
            }
        }
    });
    s.gfx.plot_back = false;
}

/// Original: 0x105a3e (ui: `tile` over the blank tile with the number `n` in its bottom rows,
/// composed in the other buffer at (0, 0) and copied to (x, y))
///
/// Up to four digits, units first, at the columns of A4-0x79da for that many digits; 10000 and
/// above (or a negative word) show no number.
pub fn tile_with_number(s: &mut State, io: &Io, tile: u16, n: i16, x: i16, y: i16) {
    {
        let gfx = &s.gfx;
        io.draw_back(|back| {
            blit_back(gfx, back, BLANK, 0, 0);
            blit_back(gfx, back, tile, 0, 0);
        });
    }
    let digits: i16 = match n {
        _ if n < 10 => 1,
        _ if n < 100 => 2,
        _ if n < 1000 => 3,
        _ if n < 10000 => 4,
        _ => 0,
    };
    let mut v = n;
    for i in 0..digits {
        let col = s
            .inventory
            .tables
            .columns
            .get((digits * 4 + i) as usize)
            .copied()
            .unwrap_or(0);
        digit(s, io, v % 10, i16::from(col));
        v /= 10;
    }
    io.with_buffers(|back, front| copy_rect(back, front, 0, 0, 0xf, 0xf, x as u16, y as u16));
}

/// Original: 0x105b14 (ui: an inventory object's icon at (x, y): gold and stacks with their
/// count, type 0x40 with its quality as circle number, others over the blank tile)
///
/// The stack kind is asked of the whole type + frame word (0x10cef2), so only frame 0 of a
/// stackable type shows a count.
pub fn draw_icon(s: &mut State, io: &Io, obj: Slot, x: i16, y: i16) {
    let o = &s.objects;
    let kind = stack_kind(o, o.tf(obj));
    let tile = object_tile(s, obj);
    let qq = o.qq_of(obj);
    if o.kind(obj) == NUMBERED {
        {
            let gfx = &s.gfx;
            io.draw_back(|back| {
                blit_back(gfx, back, BLANK, 0, 0);
                blit_back(gfx, back, tile, 0, 0);
            });
        }
        digit(s, io, (qq >> 8) as i16 + 9, 7);
        io.with_buffers(|back, front| copy_rect(back, front, 0, 0, 0xf, 0xf, x as u16, y as u16));
    } else if kind == 4 || kind == 2 {
        io.draw(|fb| blit_front(&s.gfx, fb, BLANK, x as u16, y as u16));
        let n = if kind == 4 {
            qq as i16
        } else {
            i16::from(qq as u8)
        };
        tile_with_number(s, io, tile, n, x, y);
    } else {
        tile_on_blank(s, io, tile, BLANK, x, y);
    }
}

/// The list cell `k` (0..12) in pixels.
fn cell_xy(k: i16) -> (i16, i16) {
    ((k & 3) * 16 + 0xf8, (k >> 2) * 16 + 0x20)
}

/// An inventory or container entry (not readied) is listed.
fn listed(s: &State, c: Slot) -> bool {
    let p = s.objects.place(c);
    p == IN_INVENTORY || p == IN_CONTAINER
}

/// Original: 0x1118c6 (ui: the equipment, the figure, the 4 × 3 list from the first row, its
/// arrows and the weight lines of the shown actor or container, in the status window)
///
/// Not in roster mode unless A4+0x1820 is set. The first row is pulled back by 4 while it is 4
/// or more and the list no longer reaches past it. The weights are those 0x1112cc left, as whole
/// stones over the strength of the actor that ultimately holds the container (×2 for the
/// total); over 0 when no actor holds it (a container on the map, a broken chain: the original
/// reads coordinates as parents there; `weight::holder`).
pub async fn draw_list(state: &RefCell<State>, io: &Io) {
    let saved = {
        let s = state.borrow();
        if s.ui.panel.mode == 0x91 && !s.ui.panel.no_rows {
            return;
        }
        s.ui.text.current
    };
    select_window(state, io, WindowId::Status).await;
    let (count, first) = {
        let mut s = state.borrow_mut();
        let paper = s.ui.tables.colours.paper;
        let map = s.ui.paper.as_ref();
        io.draw(|fb| fill_rect(fb, 0xf8, 0x10, 0x137, 0x4f, paper, map));
        draw_equipment(&s, io, 0, 0);
        let container = s.ui.panel.container as Slot;
        let mut c = first_child(&mut s.objects, container);
        let count = s.ui.panel.inventory_count;
        let p = &mut s.ui.panel;
        if p.inventory_first >= 4 && u16::from(p.inventory_first) + 8 >= u16::from(count) {
            p.inventory_first -= 4;
        }
        let first = p.inventory_first;
        let mut skipped = 0u8;
        // The original walks on past the last child (-1) forever if the first row lay beyond the
        // list; the clamp above keeps it inside, and the port stops at -1.
        while skipped < first && c >= 0 {
            if listed(&s, c) {
                skipped += 1;
            }
            c = next_child(&mut s.objects);
        }
        // The original's signed test (0x111988) sends a word of 0x8000 or more (the parent word
        // of a container on the map, 0x9f) to the actor table at a negative index; the port draws
        // it as an object.
        let figure = if (0..0x100).contains(&container) {
            let ty = s
                .actors
                .attrs
                .base_type_frame
                .get(container as usize)
                .copied()
                .unwrap_or(0)
                & 0x3ff;
            let base = s
                .tables
                .basetile
                .base
                .get(usize::from(ty))
                .copied()
                .unwrap_or(0);
            base.wrapping_add(crate::objects::actor_frame(ty, 4) as u16)
        } else {
            object_tile(&s, container)
        };
        tile_on_blank_at(&s, io, figure, BLANK, 0x110, 0x10);
        let mut k = 0i16;
        while c >= 0 && k < 12 {
            if listed(&s, c) {
                let (x, y) = cell_xy(k);
                draw_icon(&mut s, io, c, x, y);
                s.inventory.cells[k as usize] = c;
                k += 1;
            }
            c = next_child(&mut s.objects);
        }
        while k < 12 {
            s.inventory.cells[k as usize] = 0;
            let (x, y) = cell_xy(k);
            io.draw(|fb| blit_front(&s.gfx, fb, EMPTY, x as u16, y as u16));
            k += 1;
        }
        (count, first)
    };
    set_cursor(state, io, 8, 8).await;
    let down = u16::from(count) > u16::from(first) + 12;
    text_out(state, io, if down { 0x19 } else { 0x20 }).await;
    set_cursor(state, io, 8, 3).await;
    text_out(state, io, if first == 0 { 0x20 } else { 0x18 }).await;
    let (strength, equip, total) = {
        let s = state.borrow();
        let strength = weight::holder(&s, s.ui.panel.container as Slot)
            .and_then(|a| s.actors.attrs.strength.get(a as usize))
            .copied()
            .unwrap_or(0);
        let inv = &s.inventory;
        (
            i16::from(strength),
            weight::stones(inv.equip_weight),
            weight::stones(inv.carried_weight.wrapping_add(inv.equip_weight)),
        )
    };
    let (blank, fmt) = (game_string(io, STR_BLANK), game_string(io, STR_WEIGHT));
    let w = |v: i16| Arg::Word(v as u16);
    set_cursor(state, io, 2, 9).await;
    printf(state, io, &blank, &[]).await;
    set_cursor(state, io, 2, 9).await;
    printf(state, io, &fmt, &[w(equip), w(strength)]).await;
    set_cursor(state, io, 0xb, 9).await;
    printf(state, io, &blank, &[]).await;
    set_cursor(state, io, 0xb, 9).await;
    printf(state, io, &fmt, &[w(total), w(strength.wrapping_mul(2))]).await;
    select_window(state, io, saved).await;
}

/// Original: 0x111dca (ui: list one row (4) further down when more follow)
///
/// Selects the status window first; without a further row it returns with the status window
/// still current, as the original does.
pub async fn list_down(state: &RefCell<State>, io: &Io) {
    select_window(state, io, WindowId::Status).await;
    let more = {
        let mut s = state.borrow_mut();
        let p = &mut s.ui.panel;
        let more = u16::from(p.inventory_first) + 4 < u16::from(p.inventory_count);
        if more {
            p.inventory_first = p.inventory_first.wrapping_add(4);
        }
        more
    };
    if more {
        draw_list(state, io).await;
        select_window(state, io, WindowId::Messages).await;
    }
}

/// Original: 0x111e0a (ui: list one row (4) back up unless at the top; same window rule as
/// [`list_down`])
pub async fn list_up(state: &RefCell<State>, io: &Io) {
    select_window(state, io, WindowId::Status).await;
    let back = {
        let mut s = state.borrow_mut();
        let p = &mut s.ui.panel;
        let back = p.inventory_first >= 4;
        if back {
            p.inventory_first -= 4;
        }
        back
    };
    if back {
        draw_list(state, io).await;
        select_window(state, io, WindowId::Messages).await;
    }
}

/// Original: 0x111e3c (ui: the list cell or the figure under the pointer; true on a hit)
///
/// A list cell selects its object (-1 for an empty cell), target x = cell + 1. The figure
/// selects the shown container: for an actor the target is the view centre, else target x = 0.
pub fn list_slot_at(s: &mut State) -> bool {
    let (px, py) = s.ui.input.pointer;
    let container = s.ui.panel.container as i16;
    let centre = s.map.centre;
    let input = &mut s.ui.input;
    if (0xf8..0x138).contains(&px) && (0x20..0x50).contains(&py) {
        let k = ((px - 0xf8) >> 4) + (((py - 0x20) >> 4) << 2);
        let obj = s.inventory.cells.get(k as usize).copied().unwrap_or(0);
        input.selection = if obj == 0 { -1 } else { obj };
        input.target.0 = k + 1;
        return true;
    }
    if (0x110..0x120).contains(&px) && (0x10..0x20).contains(&py) {
        if container >= 0x100 {
            input.target.0 = 0;
        } else {
            input.target = (centre.x as i16, centre.y as i16);
        }
        input.selection = container;
        return true;
    }
    input.selection = -1;
    false
}

/// Original: 0x111eec (ui: the equipment cell under the pointer; true on a hit)
///
/// Selects the slot's object, target x = the slot; an empty slot or the both-hands marker (1)
/// selects -1.
pub fn equipment_slot_at(s: &mut State) -> bool {
    let (px, py) = s.ui.input.pointer;
    let slot = if (0xc8..0xd8).contains(&px) && (0x10..0x20).contains(&py) {
        Some(0)
    } else if (0xb0..0xc0).contains(&px) && (0x18..0x48).contains(&py) {
        Some(((py - 0x18) >> 4) + 1)
    } else if (0xe0..0xf0).contains(&px) && (0x18..0x48).contains(&py) {
        Some(((py - 0x18) >> 4) + 4)
    } else if (0xc8..0xd8).contains(&px) && (0x40..0x50).contains(&py) {
        Some(7)
    } else {
        None
    };
    let Some(slot) = slot else {
        s.ui.input.selection = -1;
        return false;
    };
    let obj = s.inventory.equip(slot);
    let input = &mut s.ui.input;
    input.target.0 = slot;
    input.selection = if obj == 0 || obj == 1 { -1 } else { obj };
    true
}

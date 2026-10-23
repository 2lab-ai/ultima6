//! Clicks: the click map 0x10658e that the event wait 0x106bbc runs while a button is down, and
//! the handlers of its regions: the map window (walk 0x105ebe, or the target), the command bar,
//! the side panel by its mode (stats 0x10613a, roster 0x106092, inventory 0x10619c, spellbook
//! 0x106474) and the rest of the screen (cancel).
//!
//! Every handler reads the click at A4-0x2f0a / -0x2f08 ([`InputState::pointer`]) and the
//! buttons of the sample (A4-0x2f06, [`PointerState::buttons`]); most act only on the press
//! (A4-0x7a34), a held button repeats the map walk.
//!
//! [`InputState::pointer`]: super::InputState::pointer
//! [`PointerState::buttons`]: super::pointer::PointerState::buttons

use std::cell::RefCell;

use super::cursor::{NO_ICON, select_command, target_from_pointer};
use super::globals::last_command;
use super::seams;
use super::target::LEADER_CELL;
use crate::engine::Io;
use crate::geom::{HALF_H, HALF_W, RULE_HALF};
use crate::objects::find_child;
use crate::state::State;

/// Button bits of A4-0x2f06's low byte.
const LEFT: u16 = 1;
const RIGHT: u16 = 2;
/// Object types 0x10619c tests: the type that is readied though it is in the container set
/// A4-0x670c, and the chest (frame bit 1 locked, frame 1 closed; a trap is an object of type
/// 0x151, quality 0x16, inside it).
const READY_ANYWAY: u16 = 0x39;
const CHEST: u16 = 0x62;
const TRAP: i16 = 0x151;
const TRAP_QUALITY: i16 = 0x16;
/// The Look command: its target may be any cell of the view (spec 13 Decision 5).
const LOOK: u16 = 0x84;
/// Image addresses of the strings 0x10619c prints (PC-relative in hunk 2).
const MSG_LOCKED: u32 = 0x104fe6;
const FMT_PROMPT: u32 = 0x104ff4;

/// Original: 0x10658e (ui: the code of a click, by the first region A4-0x79c2 that holds the
/// pointer: 0 the map window, 1 the command bar, 2 the side panel, 3 the rest of the screen;
/// 0 = nothing). In Wide region 0 is the shown map beside the UI ([`super::pointer::region`]),
/// and the slots of the side panel's views have a region each (spec 14 Design 5,
/// [`super::pointer::SLOT_REGIONS`]).
pub async fn click(state: &RefCell<State>, io: &Io) -> u16 {
    let region = {
        let s = state.borrow();
        let (x, y) = s.ui.input.pointer;
        region_at(&s, x, y)
    };
    match region {
        Some(0) => map_click(&mut state.borrow_mut()),
        Some(1) => command_click(&mut state.borrow_mut(), io),
        Some(2) => panel_click(state, io).await,
        Some(3) => {
            let mut s = state.borrow_mut();
            if !s.ui.pointer.pressed {
                return 0;
            }
            s.ui.input.target = (-1, -1);
            0x8f
        }
        #[cfg(feature = "wide")]
        Some(i) if super::pointer::SLOT_REGIONS.contains(&i) => {
            slot_click(state, io, i - super::pointer::SLOT_REGIONS.start).await
        }
        _ => 0,
    }
}

/// The side panel's click region (A4-0x79c2 entry 2).
const SIDE_PANEL: usize = 2;

/// The first click region (A4-0x79c2, [`super::pointer::region`]) that holds pixel (x, y), as
/// 0x10658e scans them; `None` when none does. The slots' regions (Wide) come after the side
/// panel's (region 2).
pub(crate) fn region_at(s: &State, x: i16, y: i16) -> Option<usize> {
    let tables = &s.ui.pointer.tables;
    let n = tables.regions.len();
    let after_panel = n.min(SIDE_PANEL + 1);
    (0..after_panel)
        .chain(super::pointer::SLOT_REGIONS)
        .chain(after_panel..n)
        .find(|&i| {
            super::pointer::region(tables, i)
                .is_some_and(|r| (r[0]..=r[2]).contains(&x) && (r[1]..=r[3]).contains(&y))
        })
}

/// Spec 14 Design 5 (Wide): a press in slot `slot` acts as the same press in the side panel with
/// the slot's view shown: [`super::panels::begin_slot_press`] maps the pointer and sets the
/// view, then [`panel_click`] (the press check, the right-click repeat, the view's handler, the
/// target-pending confirmation). A code goes to the key read, which handles it with that view
/// and then puts the press back; with no code (nothing hit, or the combat-mode line or a message
/// drawn in place) the press is put back here and the panel redrawn, as the slots show it.
/// While the panel shows none of the three views a press is ignored (Design 6).
#[cfg(feature = "wide")]
async fn slot_click(state: &RefCell<State>, io: &Io, slot: usize) -> u16 {
    if !super::panels::begin_slot_press(&mut state.borrow_mut(), slot) {
        return 0;
    }
    let code = panel_click(state, io).await;
    if code == 0 {
        super::panels::end_slot_press(&mut state.borrow_mut());
        super::panels::redraw_panel(state, io).await;
    }
    code
}

/// 0x1065f8..0x106678: the map window. Waiting for a target: the press confirms the cell under
/// the pointer (0x8e). Otherwise the right button with a command underlined repeats it on the
/// cell (0xa9); any other click walks toward the pointer (0x80, A4-0x2efc = the direction), and
/// on the origin's own cell the press passes a turn (0x8f). A held button keeps walking unless
/// the last step was blocked in the same direction.
///
/// A confirm or a repeat takes only a cell the cross-hair could reach ([`in_reach`], spec 13
/// Decision 4); on any other the press gives nothing and the wait goes on.
fn map_click(s: &mut State) -> u16 {
    let pressed = s.ui.pointer.pressed;
    if s.ui.input.target_pending != 0 {
        if !pressed || !in_reach(s, s.ui.input.target_command) {
            return 0;
        }
        target_from_pointer(s);
        return 0x8e;
    }
    let last = last_command(&s.actors);
    if last != NO_ICON && s.ui.pointer.buttons & RIGHT != 0 {
        if !pressed || !in_reach(s, u16::from(last)) {
            return 0;
        }
        target_from_pointer(s);
        return 0xa9;
    }
    let dir = walk_direction(s);
    if dir == -1 {
        return if pressed { 0x8f } else { 0 };
    }
    if !pressed && s.actors.work.step_blocked != 0 && s.ui.input.direction == dir {
        return 0;
    }
    s.ui.input.direction = dir;
    super::wait::WALK
}

/// Spec 13 Decision 4: a map click picks a target for command `cmd` (0x81..0x8a) only in the
/// original's 11 × 11 window round the leader ([`RULE_HALF`] each way, on each axis, of the
/// leader's view cell [`LEADER_CELL`]), as the cross-hair moves; Look (0x84) names any cell of
/// the view (Decision 5). In Original that window is the whole view, so every map click passes.
fn in_reach(s: &State, cmd: u16) -> bool {
    if cmd == LOOK {
        return true;
    }
    let (px, py) = s.ui.input.pointer;
    let (col, row) = super::pointer::view_cell(px, py);
    let (lx, ly) = (i16::from(LEADER_CELL.0), i16::from(LEADER_CELL.1));
    let rule = -RULE_HALF..=RULE_HALF;
    rule.contains(&(col - lx)) && rule.contains(&(row - ly))
}

/// 0x10667c..0x1066b0: the command bar. A left press gives the command of the icon under the
/// pointer (0x81 + (x - 8) / 16); a right press only underlines it (0x106506).
fn command_click(s: &mut State, io: &Io) -> u16 {
    if !s.ui.pointer.pressed {
        return 0;
    }
    let cmd = ((s.ui.input.pointer.0 - 8) >> 4) + 0x81;
    if s.ui.pointer.buttons & LEFT != 0 {
        return cmd as u16;
    }
    select_command(s, io, cmd as u8);
    0
}

/// 0x1066b4..0x106730: the side panel, on the press. With a command underlined, no target
/// pending and the right button: repeat the command on what the pointer is on (0xa9). Else
/// the handler of the panel mode; an inventory click that gives nothing while a target is
/// pending confirms what the pointer is on, if anything (0x8e).
async fn panel_click(state: &RefCell<State>, io: &Io) -> u16 {
    let mode = {
        let mut s = state.borrow_mut();
        if !s.ui.pointer.pressed {
            return 0;
        }
        let repeat = last_command(&s.actors) != NO_ICON
            && s.ui.input.target_pending == 0
            && s.ui.pointer.buttons & RIGHT != 0;
        if repeat {
            target_from_pointer(&mut s);
            return 0xa9;
        }
        s.ui.panel.mode
    };
    match mode {
        0x90 => stats_click(&mut state.borrow_mut()),
        0x91 => roster_click(&mut state.borrow_mut()),
        0x92 => {
            let code = inventory_click(state, io).await;
            let mut s = state.borrow_mut();
            if code == 0 && s.ui.input.target_pending == 1 && target_from_pointer(&mut s) != 0 {
                return super::wait::REPEAT;
            }
            code
        }
        0xa1 => spellbook_click(&mut state.borrow_mut()),
        _ => 0,
    }
}

/// Original: 0x105ebe (ui: the walk direction of a map-window click, seen from the pointer origin
/// (0x105016); while the loop reads a command key (A4-0x7a2c) it also notes the clicked map
/// location as the walk target A4-0x7a2a / -0x7a28, which a held button keeps while the party
/// is still two or more cells from it along a shared row or column and the cell is the same)
///
/// The clicked view cell is the leader's ([`HALF_W`], [`HALF_H`]) plus its offset from it, over
/// the whole view (spec 13 Decision 5; Original `cx + col - 5`).
pub fn walk_direction(s: &mut State) -> i16 {
    let (px, py) = s.ui.input.pointer;
    let (col, row) = super::pointer::view_cell(px, py);
    let dir = crate::inventory::direction_to(s.inventory.pointer_origin, px, py);
    if s.main.reading == 0 {
        s.actors.work.mouse_target = (-1, -1);
        return dir;
    }
    let c = s.map.centre;
    let (cx, cy) = (c.x as i16, c.y as i16);
    let (tx, ty) = s.actors.work.mouse_target;
    let near = |a: i16, b: i16| a.wrapping_sub(b).wrapping_abs() < 2;
    let renew = s.ui.pointer.pressed
        || tx == -1
        || (tx == cx && near(ty, cy))
        || (ty == cy && near(tx, cx))
        || s.ui.pointer.walk_cell != (col, row);
    if renew {
        s.actors.work.mouse_target = (cx + col - HALF_W as i16, cy + row - HALF_H as i16);
        s.ui.pointer.walk_cell = (col, row);
    }
    dir
}

/// The bottom row of a member's panel (y ≥ 0x58): the icons at x 0xb0, 0xc0, 0xd0, 0xe0 give
/// the previous member (0x9a), the roster (0x91), the other panel (`other`, the selection
/// cleared) and the next member (0x99).
fn member_icons(s: &mut State, other: u16) -> u16 {
    match s.ui.input.pointer.0 {
        0xb0..=0xbf => 0x9a,
        0xc0..=0xcf => 0x91,
        0xd0..=0xdf => {
            s.ui.input.selection = -1;
            other
        }
        0xe0..=0xef => 0x99,
        _ => 0,
    }
}

/// Original: 0x10613a (ui: a click in a member's stats panel: only its bottom row of icons acts;
/// the third one opens the inventory)
pub fn stats_click(s: &mut State) -> u16 {
    if s.ui.input.pointer.1 <= 0x57 {
        return 0;
    }
    member_icons(s, 0x92)
}

/// Original: 0x106092 (ui: a click in the party roster: right of x 0xc0 a member's row opens
/// their stats (0x90), on the figures (x 0xb1..=0xc0) their inventory (0x92), the member in
/// A4-0x2f04; at the left edge the arrows scroll the list up (0x93, rows 0x18..=0x27) and down
/// (0x94, from row 0x58) while there is more)
pub fn roster_click(s: &mut State) -> u16 {
    let (x, y) = s.ui.input.pointer;
    if y <= 0x18 {
        return 0;
    }
    let first = i16::from(s.ui.panel.roster_first);
    let size = i16::from(s.actors.party.size);
    let pick = |s: &mut State, code: u16, top: i16| {
        let member = ((y - top) >> 4) + first;
        s.ui.input.selection = member;
        if member < size { code } else { 0 }
    };
    if x > 0xc0 {
        pick(s, 0x90, 0x1c)
    } else if x > 0xb0 {
        pick(s, 0x92, 0x18)
    } else if (0x18..0x28).contains(&y) {
        if first != 0 { 0x93 } else { 0 }
    } else if y >= 0x58 && first < size - 5 {
        0x94
    } else {
        0
    }
}

/// Original: 0x106474 (ui: a click in the spellbook: a line of the page (x > 0xb8, rows
/// 0x10..=0x4f, 8 pixels each) casts its spell (0x8e, A4-0x2f04 = the line); the bottom row's
/// four tabs (x 0xb0..=0xef, rows 0x58..=0x67) page the book (0xa2 + tab) where the page has
/// one; either way no map target)
pub fn spellbook_click(s: &mut State) -> u16 {
    let (x, y) = s.ui.input.pointer;
    let tabs = (0x58..0x68).contains(&y) && (0xb0..0xf0).contains(&x);
    let mut code = 0;
    if tabs {
        let tab = (x - 0xb0) >> 4;
        if seams::spellbook_tab(s, tab as usize) != 0xff {
            code = tab as u16 + 0xa2;
        }
    } else if x > 0xb8 && y > 0xf && y < 0x50 {
        let line = (y - 0x10) >> 3;
        s.ui.input.selection = line;
        code = if line < i16::from(seams::spellbook_lines(s)) {
            0x8e
        } else {
            0
        };
    } else {
        return 0;
    }
    s.ui.input.target = (-1, -1);
    code
}

/// What 0x10619c does after its synchronous checks.
enum Inventory {
    Code(u16),
    /// The combat mode changed: redraw its line (0x110c76).
    CombatMode,
    /// The locked line (0x104fe6) and the prompt.
    Locked,
    /// Open the chest (0xa0) after springing its trap (0x12ba36, its lines and pictures shown
    /// there) and printing the prompt (0x106438..0x10645c).
    Trap,
}

/// Original: 0x10619c (ui: a click in the inventory panel; nothing while a conversation runs)
///
/// The fifth icon of the bottom row (x 0xf0..=0xff) steps a companion's combat mode (2..=8;
/// in party combat it is also their worktype unless charmed) and redraws its line. The other
/// icons are the stats panel's ([`member_icons`], the second panel being the stats, 0x90). The
/// arrows at x 0xf0..=0xf7 scroll the list up (0x95, rows 0x20..=0x27, if scrolled) and down
/// (0x96, rows 0x48..=0x4f, if more than 12 are left); the container figure goes back to the
/// parent (0x9f) when no target is pending. Else, unless a target is pending or Move waits:
/// a list object is readied (0xa7) or, if it is a container (A4-0x670c set, not type 0x39),
/// opened (0xa0); a locked chest says so, a trapped one springs first. An equipped object is
/// taken off (0xa8).
///
/// The click runs inside a key read (a print's more prompt too), so the trap sets the
/// interrupted print's queued lines, frames and held newline aside, plays its own lines and
/// frames in their order before the prompt and puts the interrupted ones back after it
/// ([`crate::combat::present::interrupt`]): the original prints and draws the trap
/// synchronously and the interrupted lines after the more key.
pub async fn inventory_click(state: &RefCell<State>, io: &Io) -> u16 {
    let then = inventory_checks(&mut state.borrow_mut());
    match then {
        Inventory::Code(code) => code,
        Inventory::CombatMode => {
            super::panels::combat_mode_line(state, io).await;
            0
        }
        Inventory::Locked => {
            let msg = seams::game_string(io, MSG_LOCKED);
            super::printf(state, io, &msg, &[]).await;
            prompt(state, io).await;
            super::print::put_raw(state, io, -1).await;
            0
        }
        Inventory::Trap => {
            let chest = state.borrow().ui.input.selection;
            let outer = crate::combat::present::interrupt(state, io).await;
            seams::spring_trap(&mut state.borrow_mut(), chest);
            crate::combat::play_effects(state, io).await;
            prompt(state, io).await;
            crate::combat::present::resume(state, io, outer).await;
            0xa0
        }
    }
}

/// The prompt 0x10619c prints after a message: newline, the leader's name, the cursor.
async fn prompt(state: &RefCell<State>, io: &Io) {
    let fmt = seams::game_string(io, FMT_PROMPT);
    let name = crate::main_loop::leader_name(&state.borrow());
    super::printf(state, io, &fmt, &[super::Arg::Str(&name)]).await;
}

/// The synchronous part of [`inventory_click`] (0x10619c up to its prints).
fn inventory_checks(s: &mut State) -> Inventory {
    if s.converse.active {
        return Inventory::Code(0);
    }
    let (x, y) = s.ui.input.pointer;
    let member = s.ui.panel.member;
    if member != 0 && (0xf0..0x100).contains(&x) && (0x58..0x68).contains(&y) {
        next_combat_mode(s, member);
        return Inventory::CombatMode;
    }
    if y >= 0x58 {
        return Inventory::Code(member_icons(s, 0x90));
    }
    let p = &s.ui.panel;
    let pending = s.ui.input.target_pending != 0;
    if (0xf0..0xf8).contains(&x) {
        let more = i16::from(p.inventory_count) - i16::from(p.inventory_first) > 12;
        let code = match y {
            0x20..=0x27 if p.inventory_first != 0 => 0x95,
            0x48..=0x4f if more => 0x96,
            _ => 0,
        };
        return Inventory::Code(code);
    }
    if (0x110..0x120).contains(&x) && (0x10..0x20).contains(&y) && !pending {
        return Inventory::Code(0x9f);
    }
    if pending || s.ui.input.target_command == 0x87 {
        return Inventory::Code(0);
    }
    let code = target_from_pointer(s);
    let obj = s.ui.input.selection;
    if obj == -1 {
        return Inventory::Code(0);
    }
    if code == 0x9c {
        return Inventory::Code(0xa8);
    }
    if code != 0x9b {
        return Inventory::Code(0);
    }
    let kind = s.objects.kind(obj);
    let frame = s.objects.tf(obj) >> 10;
    if !crate::objects::in_type_set_670c(&s.objects, kind) || kind == READY_ANYWAY {
        return Inventory::Code(0xa7);
    }
    if kind == CHEST && frame & 2 != 0 {
        return Inventory::Locked;
    }
    if kind == CHEST && frame == 1 && find_child(&mut s.objects, obj, TRAP, TRAP_QUALITY) != -1 {
        return Inventory::Trap;
    }
    Inventory::Code(0xa0)
}

/// 0x1061de..0x106268: the next combat mode of party member `member` (after 8 comes 2); in
/// party combat a companion who is not charmed (status bit 2) takes it as their worktype.
fn next_combat_mode(s: &mut State, member: i16) {
    let actor = super::panels::member_actor(s, member);
    let a = usize::from(actor);
    let Some(mode) = s.actors.attrs.combat_mode.get_mut(a) else {
        return;
    };
    *mode = mode.wrapping_add(1);
    if *mode == 9 {
        *mode = 2;
    }
    let mode = *mode;
    let leader = i16::from(s.actors.party.leader_index);
    if s.actors.combat_mode()
        && leader != member
        && s.objects.status(i16::from(actor)) & 4 == 0
        && let Some(w) = s.actors.worktype.get_mut(a)
    {
        *w = mode;
    }
}

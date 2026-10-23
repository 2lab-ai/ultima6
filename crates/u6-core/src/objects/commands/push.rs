//! Move (0x12ae04): push an object or actor one tile on the map (0x12acc0, 0x115c06).
//! Moving a carried object to another member or container is the other half of 0x12ae04 and
//! stays a stub point here; the outermost-holder helper it calls (0x12adc4) comes with it.

use std::cell::RefCell;

use super::drop::{fits_in, surface_at};
use super::get::{P_NOT_POSSIBLE, message};
use super::seams;
use super::text::{print, print_at, putc};
use super::world::S_LINE;
use crate::engine::Io;
use crate::objects::iter::begin_at;
use crate::objects::moves::{move_into, move_to};
use crate::objects::{IN_CONTAINER, Slot, shown_name};
use crate::state::State;
use crate::todo_stub;
use crate::ui::pointer::LEADER_PIXEL;
use crate::ui::printf::Arg;

const S_BANG_LINE: u32 = 0x0012_85ac; // "\n%s!\n"
const S_NL_LINE_NL: u32 = 0x0012_859a; // "\n%s\n"
const S_TO: u32 = 0x0012_85fe;
const S_NOWHERE: u32 = 0x0012_8602;
const S_DIR_LINE: u32 = 0x0012_860c; // "%s.\n"

/// Port-only (spec 08, slice K2): the strings this module passes to `printf` as its format,
/// for the locale layer's catalogue ([`crate::locale::strings`]).
pub(super) const PRINTED: &[u32] = &[S_BANG_LINE, S_NL_LINE_NL, S_TO, S_NOWHERE, S_DIR_LINE];
const P_NOTHING: i32 = -0x6c58;
const P_OUT_OF_RANGE: i32 = -0x6c60;
const P_BLOCKED: i32 = -0x6c5c;
const P_FAILED: i32 = -0x6c68;
/// Original: A4-0x61be (pointers to the 8 direction names)
const DIR_NAMES: i32 = -0x61be;
/// The command code a direction key gives (walk).
const WALK: u16 = 0x80;
/// A chair: pushed against something, or not facing the push, it only turns.
const CHAIR: u16 = 0xdd;

/// The frame pixel of map location (x, y) with the view centred on (cx, cy): the leader's
/// pixel (A4-0x7a26 / -0x7a25 at rest, [`LEADER_PIXEL`]) plus 16 per cell. Words: Wide's map
/// pixels pass 255, where the original's bytes would wrap (spec 13 Design 3).
fn view_pixel(x: u16, y: u16, cx: u16, cy: u16) -> (u16, u16) {
    (
        (x.wrapping_sub(cx) << 4).wrapping_add(LEADER_PIXEL.0),
        (y.wrapping_sub(cy) << 4).wrapping_add(LEADER_PIXEL.1),
    )
}

fn leader(s: &State) -> Slot {
    let p = &s.actors.party;
    Slot::from(p.members[usize::from(p.leader_index).min(15)])
}

/// Original: 0x12acc0 (objects: is the push of `obj` from (x, y) toward `dir` blocked)
///
/// Not blocked when the object may stand on the tile ahead (0x11b1c0) or a surface is there
/// (0x12a050); a diagonal push then also needs one of the two tiles beside the diagonal free.
pub fn push_blocked(s: &mut State, obj: Slot, x: u16, y: u16, dir: usize) -> bool {
    let d = &s.objects.defs;
    let at = |i: usize, d: &crate::objects::ObjectDefs| {
        (
            x.wrapping_add_signed(d.dx(i)),
            y.wrapping_add_signed(d.dy(i)),
        )
    };
    let (nx, ny) = at(dir, d);
    if !seams::may_stand(s, obj, nx, ny) && !surface_at(s, nx, ny) {
        return true;
    }
    if dir & 1 == 0 {
        return false;
    }
    let d = &s.objects.defs;
    let (a, b) = (at(dir.wrapping_sub(1), d), at(dir + 1, d));
    let free =
        |s: &mut State, (x, y): (u16, u16)| seams::may_stand(s, obj, x, y) || surface_at(s, x, y);
    !(free(s, a) || free(s, b))
}

/// The end of a Move that went nowhere or failed: no target pending, the pointer origin back on
/// the centre cell, the pointer shape for it (0x1051a6, [`crate::ui::pointer::update_shape`]).
fn reset_pointer(s: &mut State) {
    s.ui.input.target_pending = 0;
    s.inventory.pointer_origin = LEADER_PIXEL;
    crate::ui::pointer::update_shape(s);
}

/// Original: 0x12ae04 (objects: the Move command on the confirmed target) [partial]
///
/// Nothing selected or the shown container: the nothing message (A4-0x6c58). The name. On the
/// map: within one tile of the leader (else the out-of-range message, A4-0x6c60), the leader
/// turns to it, the direction prompt (0x1285fe) and a direction key (any other key: the
/// nowhere line, 0x128602), the direction's name. A weightless thing or a dragon part: the
/// not-possible message. An actor resists (magic 0x115c06), and the leader or an actor lying
/// down (frame & 3 = 3, not type 0x153) cannot be pushed: the failed message (A4-0x6c68). Else
/// 5 movement points, and it goes into a container ahead it fits in (weight below 0xff), or,
/// unless blocked (the blocked message, A4-0x6c5c; 0x12acc0),
/// one tile on; a chair turns to face the push instead when it does not already. Carried
/// objects (moving them to someone else) are a stub point.
pub async fn push(state: &RefCell<State>, io: &Io) {
    let sel = state.borrow().ui.input.selection;
    if sel == -1 || sel as u16 == state.borrow().ui.panel.container {
        let msg = message(&state.borrow(), P_NOTHING);
        print_at(state, io, S_LINE, &[Arg::Str(&msg)]).await;
        return;
    }
    let name = shown_name(&state.borrow(), sel).into_bytes();
    print(state, io, &name, &[]).await;
    putc(state, io, b'\n').await;
    if state.borrow().objects.place(sel) != 0 {
        // Contract: move a carried object to another party member or into a container
        // (the to-whom prompt, weight, the can't-carry line, container rules, spellbook /
        // vortex cube).
        // Fallback: nothing moves, no movement points.
        todo_stub!("0x12ae04 Move a carried object to someone", ());
        let mut s = state.borrow_mut();
        s.ui.panel.dirty = false;
        s.ui.input.target_pending = 0;
        return;
    }
    let (cx, cy, (tx, ty)) = {
        let s = state.borrow();
        (s.map.centre.x, s.map.centre.y, s.ui.input.target)
    };
    let far = |t: i16, c: u16| t.wrapping_sub(c as i16).wrapping_abs() > 1;
    if far(tx, cx) || far(ty, cy) {
        let msg = message(&state.borrow(), P_OUT_OF_RANGE);
        print_at(state, io, S_BANG_LINE, &[Arg::Str(&msg)]).await;
        return;
    }
    let (ox, oy) = {
        let mut s = state.borrow_mut();
        let (px, py) = s.ui.input.pointer;
        let dir = seams::direction_of(&s, px, py);
        if dir != -1 {
            let l = leader(&s) as u16;
            crate::actors::walk::face(&mut s, l, dir as u16);
            crate::map::view::update_view(&mut s);
        }
        let (ox, oy) = (s.objects.x(sel), s.objects.y(sel));
        s.ui.input.target_pending = 0;
        s.inventory.pointer_origin = view_pixel(ox, oy, cx, cy);
        (ox, oy)
    };
    print_at(state, io, S_TO, &[]).await;
    if crate::ui::wait::read_key(state, io).await != WALK {
        print_at(state, io, S_NOWHERE, &[]).await;
        reset_pointer(&mut state.borrow_mut());
        return;
    }
    let dir = state.borrow().ui.input.direction;
    let dir_name = {
        let s = state.borrow();
        let at = super::GameData::a4(DIR_NAMES).wrapping_add_signed(i32::from(dir) * 4);
        s.objcmds.data.pointed(at)
    };
    print_at(state, io, S_DIR_LINE, &[Arg::Str(&dir_name)]).await;
    let pusher = leader(&state.borrow());
    let (wt, kind) = {
        let s = state.borrow();
        let wt = seams::weight(&s, sel).wrapping_add(seams::contents_weight(&s, sel));
        (wt, s.objects.kind(sel))
    };
    let weightless = state
        .borrow()
        .tables
        .tileflags
        .weight
        .get(usize::from(kind))
        .copied()
        .unwrap_or(0)
        == 0;
    if weightless || kind == 0x19b {
        let msg = message(&state.borrow(), P_NOT_POSSIBLE);
        print_at(state, io, S_NL_LINE_NL, &[Arg::Str(&msg)]).await;
        reset_pointer(&mut state.borrow_mut());
        finish(state);
        return;
    }
    if sel < 0x100 {
        let failed = {
            let mut s = state.borrow_mut();
            crate::magic::strength_contest(&mut s, pusher as u16, sel as u16)
                || sel == pusher
                || (s.objects.tf(sel) >> 10 & 3 == 3 && kind != 0x153)
        };
        if failed {
            let msg = message(&state.borrow(), P_FAILED);
            print_at(state, io, S_NL_LINE_NL, &[Arg::Str(&msg)]).await;
            reset_pointer(&mut state.borrow_mut());
            return;
        }
    }
    let blocked = {
        let mut s = state.borrow_mut();
        let s = &mut *s;
        crate::actors::walk::spend_mp(s, pusher as u16, 5);
        let d = usize::try_from(dir).unwrap_or(0);
        let nx = ox.wrapping_add_signed(s.objects.defs.dx(d));
        let ny = oy.wrapping_add_signed(s.objects.defs.dy(d));
        let z = u16::from(s.map.centre.z);
        let here = begin_at(&mut s.objects, &s.tables, nx, ny, z);
        if here != -1 && fits_in(s, here, sel) && wt < 0xff {
            move_into(s, sel, here, IN_CONTAINER);
            false
        } else if push_blocked(s, sel, ox, oy, d) {
            true
        } else {
            if !turn_chair(s, sel, dir) {
                move_to(s, sel, nx, ny, z);
            }
            false
        }
    };
    if blocked {
        let msg = message(&state.borrow(), P_BLOCKED);
        print_at(state, io, S_NL_LINE_NL, &[Arg::Str(&msg)]).await;
        let mut s = state.borrow_mut();
        turn_chair(&mut s, sel, dir);
        reset_pointer(&mut s);
        return;
    }
    {
        let mut s = state.borrow_mut();
        crate::map::view::update_view(&mut s);
        reset_pointer(&mut s);
    }
    finish(state);
}

/// A chair not facing the push direction turns to it (frame (dir >> 1) & 3); true when it did.
fn turn_chair(s: &mut State, obj: Slot, dir: i16) -> bool {
    if s.objects.kind(obj) != CHAIR || s.objects.tf(obj) >> 10 == (dir >> 1) as u16 {
        return false;
    }
    let t = &mut s.objects.type_frame[obj as usize];
    *t = (*t & 0x3ff) + ((((dir >> 1) & 3) as u16) << 10);
    true
}

/// 0x12ba24: the panel is not redrawn on its own, no target pending.
fn finish(state: &RefCell<State>) {
    let mut s = state.borrow_mut();
    s.ui.panel.dirty = false;
    s.ui.input.target_pending = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{HALF_H, HALF_W, MAP_X, MAP_Y, VIEW_H, VIEW_W};

    /// Spec 13 Design 3: the pointer origin Move puts on the pushed object is the centre of its
    /// view cell. For the view's east-most and south-most cells that is past 255 in Wide, and no
    /// byte wraps it: from the east-most cell the leader is due west, from the south-most due
    /// north.
    #[test]
    fn the_push_pixel_of_the_east_most_view_cell_does_not_wrap() {
        let (cx, cy) = (500u16, 400u16);
        let (hw, hh) = (HALF_W as u16, HALF_H as u16);
        let east = view_pixel(cx + hw, cy, cx, cy);
        assert_eq!(east, (MAP_X + (VIEW_W as u16 - 1) * 16 + 8, LEADER_PIXEL.1));
        let south = view_pixel(cx, cy + hh, cx, cy);
        assert_eq!(
            south,
            (LEADER_PIXEL.0, MAP_Y + (VIEW_H as u16 - 1) * 16 + 8)
        );
        let north_west = view_pixel(cx - hw, cy - hh, cx, cy);
        assert_eq!(north_west, (MAP_X + 8, MAP_Y + 8), "the view's first cell");
        assert_eq!(view_pixel(cx, cy, cx, cy), LEADER_PIXEL, "the leader's own");
        let (lx, ly) = (LEADER_PIXEL.0 as i16, LEADER_PIXEL.1 as i16);
        assert_eq!(crate::inventory::direction_to(east, lx, ly), 6, "west");
        assert_eq!(crate::inventory::direction_to(south, lx, ly), 0, "north");
        if cfg!(feature = "wide") {
            assert!(
                east.0 > 0xff && east.0 < u6_render::WIDTH as u16,
                "past a byte"
            );
        }
    }
}

//! Calls from the object commands into routines other modules own and have not ported yet.
//!
//! Each seam names its owner and the original, keeps the `todo_stub!` hit count, and states
//! its fallback. Where an accepted scenario depends on the exact result (the egg hatch on the
//! walk path places monsters with 0x121e88 and its RNG draws), the fallback is the original's
//! behaviour written out here from its code, so the dispatcher can swap in the owner's port
//! without a behaviour change.

use crate::objects::Slot;
use crate::objects::moves::move_to;
use crate::state::State;
use crate::todo_stub;

/// Owner: combat (hunk 13). Original 0x121e88 (put a new monster on a free tile near where it
/// stands, out of the view when `out_of_view`).
///
/// Contract: up to 8 tries of x + (1..=4) + (1..=4) − 5, y the same (four RNG draws per try,
/// in that order); with `out_of_view` a tile within 5 of the view centre on both axes is
/// skipped; the first tile the actor may stand on (actors 0x11b1c0) gets it (0x10c9d2) and the
/// result is true; false when no try fits or `m` < 0. Fallback = that behaviour (the egg hatch
/// at game start depends on its draws and positions).
pub(crate) fn scatter(s: &mut State, m: Slot, out_of_view: bool) -> bool {
    todo_stub!("0x121e88 place a new monster near its tile", ());
    if m < 0 {
        return false;
    }
    let (x, y, z) = (s.objects.x(m), s.objects.y(m), u16::from(s.objects.z(m)));
    for _ in 0..8 {
        let a = s.rng.range(1, 4);
        let b = s.rng.range(1, 4);
        let nx = a.wrapping_add(b).wrapping_add(x).wrapping_sub(5);
        let c = s.rng.range(1, 4);
        let d = s.rng.range(1, 4);
        let ny = c.wrapping_add(d).wrapping_add(y).wrapping_sub(5);
        let centre = s.map.centre;
        let (cx, cy) = (centre.x as i16, centre.y as i16);
        let near = |v: u16, c: i16| (c.wrapping_sub(5)..=c.wrapping_add(5)).contains(&(v as i16));
        if out_of_view && near(nx, cx) && near(ny, cy) {
            continue;
        }
        if crate::actors::walk::can_move_to(s, m as u16, nx, ny) {
            move_to(s, m, nx, ny, z);
            return true;
        }
    }
    false
}

/// Owner: ui (hunk 2). Original 0x105016 (direction 0..7 of the view pixel (x, y) from the
/// reference pixel A4-0x7a26 / -0x7a25, -1 at it).
///
/// Contract: pure. Within 24 pixels on both axes the 16-pixel cell around the reference
/// decides (8 neighbours, -1 inside); farther, the dominant axis decides unless both differ by
/// less than a factor of 2, then the diagonal. Fallback = that computation (the egg warning
/// names the direction).
pub(crate) fn direction_of(s: &State, x: i16, y: i16) -> i16 {
    todo_stub!("0x105016 direction of a view pixel", ());
    let (rx, ry) = (i16::from(s.objcmds.pointer_ref.0), i16::from(s.objcmds.pointer_ref.1));
    let (ax, ay) = ((x - rx).abs(), (y - ry).abs());
    let far = x < rx - 0x18 || x >= rx + 0x18 || y < ry - 0x18 || y >= ry + 0x18;
    if far {
        let mut d = -1;
        if ay < ax {
            if ay == 0 || ax / ay > 1 {
                d = if x < rx { 6 } else { 2 };
            }
        } else if ax == 0 || ay / ax > 1 {
            d = if y < ry { 0 } else { 4 };
        }
        if d == -1 {
            d = match (x < rx, y < ry) {
                (true, true) => 7,
                (true, false) => 5,
                (false, true) => 1,
                (false, false) => 3,
            };
        }
        return d;
    }
    let col = if x < rx - 8 {
        0
    } else if x < rx + 8 {
        1
    } else {
        2
    };
    let row = if y < ry - 8 {
        0
    } else if y < ry + 8 {
        1
    } else {
        2
    };
    [[7, 6, 5], [0, -1, 4], [1, 2, 3]][col][row]
}

/// Owner: combat (hunk 13). Original 0x121b08 (distance from actor `a` to (x, y): the larger of
/// |dx|, |dy|).
///
/// Contract: pure. Fallback = that computation.
pub(crate) fn distance(s: &State, a: u16, x: u16, y: u16) -> i16 {
    todo_stub!("0x121b08 distance from an actor to a location", ());
    let slot = a as Slot;
    let dx = (s.objects.x(slot).wrapping_sub(x) as i16).wrapping_abs();
    let dy = (s.objects.y(slot).wrapping_sub(y) as i16).wrapping_abs();
    dx.max(dy)
}

/// Owner: ui (hunk 8). Original 0x1118c6 (redraw the inventory panel after a carried object
/// changed). Fallback: the panel is marked for the next 0x103f64 redraw instead.
pub(crate) fn redraw_inventory(s: &mut State) {
    todo_stub!("0x1118c6 redraw the inventory panel", ());
    s.ui.panel.dirty = true;
}

/// Owner: combat (hunk 13). Original 0x1231be (hurt actor `a` by `damage`: hit points, death,
/// splitting slimes). Fallback: nothing (no damage).
pub(crate) fn hurt(_s: &mut State, _a: Slot, _damage: i16) {
    todo_stub!("0x1231be hurt an actor", ())
}

/// Owner: combat (hunk 13). Original 0x12395a (print actor `a`'s wound level: "critical!",
/// "heavily wounded" ...). Fallback: nothing printed.
pub(crate) fn wound_text(_s: &mut State, _a: Slot) {
    todo_stub!("0x12395a print the wound level", ())
}

/// Owner: combat (hunk 13). Original 0x1230ce (flash actor `a` hit: redraw with the hit
/// colour). Fallback: nothing.
pub(crate) fn flash(_s: &mut State, _a: Slot) {
    todo_stub!("0x1230ce flash a hit actor", ())
}

/// Owner: ui (hunk 18). Original 0x1331b4 (explosion of `tile` around (x, y): animate it and
/// return the slots it reached, -1-terminated in the original). Fallback: nobody reached.
pub(crate) fn blast(_s: &mut State, _x: u16, _y: u16, _tile: u16) -> Vec<Slot> {
    todo_stub!("0x1331b4 explosion animation and its victims", Vec::new())
}

/// Owner: magic (hunk 11). Original 0x116526 (the object of `kind` and `quality` that `owner`
/// carries, -1). Fallback: the first direct child of that kind and quality (0x10b18c).
pub(crate) fn carried(s: &mut State, owner: Slot, kind: u16, quality: i16) -> Slot {
    todo_stub!("0x116526 object of a kind an actor carries", ());
    crate::objects::find_child(&mut s.objects, owner, kind as i16, quality)
}

/// Owner: combat (hunk 13). Original 0x1240fa (the actor a horse or monster part `obj`
/// belongs to; `obj` itself for an actor). Fallback: `obj` (the rider passed is the actor).
pub(crate) fn rider(_s: &State, obj: Slot) -> Slot {
    todo_stub!("0x1240fa actor of a part", obj)
}

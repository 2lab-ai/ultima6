//! Calls from the spell code into routines other units own: combat (hunk 13: damage, missiles,
//! experience, hostility, the wound line), a few graphics / ui helpers and object routines
//! that are not ported yet. Each is the narrowest signature the spells need, with a stub
//! contract and an explicit fallback; the dispatcher points them at the owners' ports.

use std::cell::RefCell;

use crate::engine::Io;
use crate::state::State;
use crate::todo_stub;

/// Stand-in for graphics 0x1521f4 (fill the inclusive pixel rectangle in the plot colour,
/// 0x152174; XOR when A4-0x617c is set, into the back buffer when A4-0x6179 is set), drawn
/// pixel by pixel through the ported plot 0x152188, which applies both flags. Owner: gfx.
pub(crate) fn fill_rect(s: &mut State, io: &Io, x0: u16, y0: u16, x1: u16, y1: u16) {
    let (xa, xb) = (x0.min(x1), x0.max(x1));
    let (ya, yb) = (y0.min(y1), y0.max(y1));
    let gfx = &s.gfx;
    let draw = |fb: &mut u6_render::Framebuffer| {
        for y in ya..=yb {
            for x in xa..=xb {
                crate::gfx::blit::plot(gfx, fb, x, y);
            }
        }
    };
    if gfx.plot_back {
        io.draw_back(draw);
    } else {
        io.draw(draw);
    }
}

/// [ui] 0x1324ae (redraw the map-window border tiles 0x1b0..0x1b7 that the rectangle (x0, y0)
/// ..(x1, y1) touches, into the back buffer). Stub contract: no result. Fallback: the border
/// is not redrawn (the window flash of 0x115b7c XORs twice, so the border ends as it was).
pub(crate) fn redraw_border(_s: &mut State, _x0: u16, _y0: u16, _x1: u16, _y1: u16) {
    todo_stub!("0x1324ae redraw the window border over a rectangle", ())
}

/// [ui] 0x132338 (flash an actor on the map: when `actor` is on the map and in view, its tile
/// is drawn with effect `mode` (0x10e596(2)) and the window presented dissolving, then redrawn
/// plain). Stub contract: no result. Fallback: no flash.
pub(crate) fn flash_actor(_s: &mut State, _io: &Io, _actor: u16, _mode: u16) {
    todo_stub!("0x132338 flash an actor on the map", ())
}

/// [combat] 0x1228ba (a missile of tile `tile` from `from` toward (x, y) within `range`; the
/// flight may stop short and move (x, y)). Stub contract: non-zero when it reached the target.
/// Fallback: 1, the target unchanged (the spell goes on as if nothing were in the way).
pub(crate) async fn missile(
    _state: &RefCell<State>,
    _io: &Io,
    _from: u16,
    _range: i16,
    _target: &mut (i16, i16),
    _tile: u16,
) -> i16 {
    todo_stub!("0x1228ba fly a missile to the target", 1)
}

/// [combat] 0x1240fa (the object a target stands for: a part of a multi-tile creature gives
/// its head, a horse its rider). Stub contract: object in, object out. Fallback: unchanged.
pub(crate) fn target_owner(_s: &mut State, obj: i16) -> i16 {
    todo_stub!("0x1240fa the actor a target object stands for", obj)
}

/// [combat] 0x122076 (does `target` resist `caster`'s spell: random 1..30 < int(target) / 2 −
/// int(caster) + 15). Stub contract: true = resisted; one RNG draw. Fallback: the leaf as read
/// at 0x122076 (the draw and the compare, intelligence from 0x122048), so the draw order holds
/// until combat owns it.
pub(crate) fn resists(s: &mut State, caster: u16, target: u16) -> bool {
    let r = s.rng.range(1, 0x1e) as i16;
    let t = crate::actors::leaves::intelligence(s, target) as i16 / 2;
    let c = crate::actors::leaves::intelligence(s, caster) as i16;
    todo_stub!("0x122076 resist a spell", r < t - c + 0xf)
}

/// [combat] 0x1231be (hurt `target` by `damage`: hit points, death, splitting slimes; prints).
/// Stub contract: the experience the hit is worth. Fallback: the hit points drop, never below
/// 1 (death and its messages are combat's); 0 experience.
pub(crate) fn hurt(s: &mut State, target: u16, damage: i16) -> i16 {
    if let Some(hp) = s.actors.attrs.hp.get_mut(usize::from(target)) {
        *hp = (i16::from(*hp) - damage.max(0)).clamp(1, 0xff) as u8;
    }
    todo_stub!("0x1231be hurt an actor (magic seam)", 0)
}

/// [combat] 0x122104 (add `exp` experience to `actor`, at most 9999). Stub contract: no
/// result. Fallback: the leaf as read at 0x122104.
pub(crate) fn add_experience(s: &mut State, actor: u16, exp: i16) {
    if let Some(e) = s.actors.attrs.exp.get_mut(usize::from(actor)) {
        *e = if *e < 9999u16.wrapping_sub(exp as u16) {
            e.wrapping_add(exp as u16)
        } else {
            9999
        };
    }
    todo_stub!("0x122104 add experience", ())
}

/// [combat] 0x123c56 (`target` turns on `attacker` after a hit of `damage`; may print "yells
/// for help"). Stub contract: no result. Fallback: no change of attitude.
pub(crate) fn provoke(_s: &mut State, _target: u16, _attacker: u16, _damage: i16) {
    todo_stub!("0x123c56 a hit target turns hostile", ())
}

/// [combat] 0x12395a (print the wound line of `actor`: "grazed", "barely wounded" ...). Stub
/// contract: prints; no result. Fallback: nothing printed.
pub(crate) async fn wound_line(_state: &RefCell<State>, _io: &Io, _actor: u16) {
    todo_stub!("0x12395a print the wound level", ())
}

/// [combat] 0x1230ce (the hit flash on `actor` and its redraw; wakes and uncharms a
/// paralysed sleeper's status bit 2). Stub contract: no result. Fallback: none.
pub(crate) fn hit_flash(_s: &mut State, _actor: u16) {
    todo_stub!("0x1230ce flash a hit actor (magic seam)", ())
}

/// [combat] 0x1221da (an actor put to sleep lies down). Stub contract: no result. Fallback:
/// none (the sleep status bit is set by the spell).
pub(crate) fn lie_down(_s: &mut State, _actor: u16) {
    todo_stub!("0x1221da lie down (magic seam)", ())
}

/// [objects] 0x12bbc0 (lock (`lock`, stack words 1, 0) or magically unlock (0, 1) the door
/// `obj`; D1 = 0). Stub contract: no result. Fallback: nothing changes.
pub(crate) fn door(_s: &mut State, _obj: i16, _lock: bool) {
    todo_stub!("0x12bbc0 lock or unlock a door (magic seam)", ())
}

/// [objects] 0x12bdfa (the same for a chest, type 0x62). Stub contract and fallback as
/// [`door`].
pub(crate) fn chest(_s: &mut State, _obj: i16, _lock: bool) {
    todo_stub!("0x12bdfa lock or unlock a chest (magic seam)", ())
}

/// [ui] 0x110f96 (the equipment slot object `obj` is readied in, -1 when it cannot be
/// readied). Stub contract: pure lookup by type. Fallback 0 (a slot): Animate goes ahead.
pub(crate) fn ready_slot(_s: &State, _obj: i16) -> i16 {
    todo_stub!("0x110f96 ready slot of an object (magic seam)", 0)
}

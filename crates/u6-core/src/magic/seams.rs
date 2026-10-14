//! Calls from the spell code into routines other units own: combat (hunk 13: damage, missiles,
//! experience, hostility, the wound line), a few graphics / ui helpers and object routines
//! that are not ported yet. Each is the narrowest signature the spells need, with a stub
//! contract and an explicit fallback; the dispatcher points them at the owners' ports.

use std::cell::RefCell;

use crate::engine::Io;
use crate::objects::Slot;
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

/// Bytes of the user's `game` at `A4 + off` (zeros where the file or the address is missing):
/// the small A4 tables the spells read at run time (spec 03 §5).
pub(crate) fn a4_bytes(io: &Io, off: i32, len: usize) -> Vec<u8> {
    io.files()
        .get("game")
        .and_then(|g| {
            let img = crate::image::GameImage::parse(g).ok()?;
            img.a4_bytes(off, len).ok().map(<[u8]>::to_vec)
        })
        .unwrap_or_else(|| vec![0; len])
}

/// [objects] 0x131646 (a monster of `kind` at (x, y, z) with its stats, parts and equipment;
/// -1 when no temporary-actor slot is free). Calls the objects port; while that is its stub
/// (-1 always) the fallback is a bare temporary actor through the ported 0x10bec8 at the place
/// (no stats, parts or equipment), so the summoning spells have a creature to place. Once the
/// objects port is in, a -1 means no free slot and 0x10bec8 finds none either.
pub(crate) fn spawn(s: &mut State, kind: u16, x: u16, y: u16, z: u16) -> Slot {
    let m = crate::objects::spawn_monster(s, kind, x, y, z);
    if m >= 0 {
        return m;
    }
    todo_stub!(
        "0x131646 spawn a monster (magic seam: bare actor)",
        crate::objects::create_actor(s, kind, x, y, z)
    )
}

/// The tile under (x, y) on the leader's level and its flags2 byte.
fn flags2_at(s: &State, x: i16, y: i16) -> u8 {
    let z = u16::from(s.map.centre.z);
    let t = crate::actors::seams::tile_at(s, x as u16, y as u16, z);
    s.tables
        .tileflags
        .flags2
        .get(usize::from(t))
        .copied()
        .unwrap_or(0)
}

/// flags2 of the tile object `o` shows at the cell being walked (its part, A4-0x78dc).
fn object_flags2(s: &State, o: Slot) -> u8 {
    let w = s.objects.type_frame.get(o as usize).copied().unwrap_or(0);
    let base = s.tables.basetile.base.get(usize::from(w & 0x3ff)).copied();
    let t = base
        .unwrap_or(0)
        .wrapping_add(w >> 10)
        .wrapping_sub(s.objects.at.part);
    s.tables
        .tileflags
        .flags2
        .get(usize::from(t))
        .copied()
        .unwrap_or(0)
}

/// An actor a flight lists (not the lying frame 0 of 0x165, movement flag 0x10 clear).
fn listed(s: &State, o: Slot) -> bool {
    let Ok(i) = usize::try_from(o) else {
        return false;
    };
    let w = s.objects.type_frame[i];
    i < 0x100
        && !(w & 0x3ff == 0x165 && w >> 10 == 0)
        && s.actors.attrs.movement_flags[i] & 0x10 == 0
}

/// `a / n` rounded half up (n > 0).
fn div_round(a: i16, n: i16) -> i16 {
    (i32::from(a) * 2 + i32::from(n)).div_euclid(i32::from(n) * 2) as i16
}

/// The fallback flight: a straight tile line per missile from (x0, y0), stopped past `range`
/// on either axis, before a map tile with flags2 bit 5 (bit 3 lets it through next to the
/// start) or on an object whose tile has it; the actors met, in slot order (A4+0x186e is a bit
/// set walked by slot). xs / ys become where each stopped; 1 when all arrived.
fn paths(
    s: &mut State,
    x0: i16,
    y0: i16,
    xs: &mut [i16],
    ys: &mut [i16],
    range: i16,
) -> (i16, Vec<i16>) {
    let z = u16::from(s.map.centre.z);
    let mut passed = [false; 0x100];
    let mut reached = 1;
    for (tx, ty) in xs.iter_mut().zip(ys.iter_mut()) {
        let (dx, dy) = (*tx - x0, *ty - y0);
        let n = dx.abs().max(dy.abs());
        let (mut ex, mut ey) = (x0, y0);
        let mut stopped = false;
        for k in 1..=n {
            let (x, y) = (x0 + div_round(dx * k, n), y0 + div_round(dy * k, n));
            let (ax, ay) = ((x - x0).abs(), (y - y0).abs());
            let f2 = flags2_at(s, x, y);
            if ax > range || ay > range || (f2 & 0x20 != 0 && !(ax <= 1 && ay <= 1 && f2 & 8 != 0))
            {
                stopped = true;
                break;
            }
            (ex, ey) = (x, y);
            let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, x as u16, y as u16, z);
            while o >= 0 {
                if listed(s, o) {
                    passed[o as usize] = true;
                }
                if object_flags2(s, o) & 0x20 != 0 {
                    break;
                }
                o = crate::objects::next_at(&mut s.objects, &s.tables);
            }
            if o >= 0 && k < n {
                stopped = true;
                break;
            }
        }
        if stopped {
            reached = 0;
        }
        (*tx, *ty) = (ex, ey);
    }
    let list = (0..0x100)
        .filter(|&a| passed[a])
        .map(|a| a as i16)
        .collect();
    (reached, list)
}

/// [ui] 0x132706 (fly a missile of `tile` from (x0, y0) toward each (xs[i], ys[i]) within
/// `range`; `flags` as the original's word; xs / ys become where each stopped). Stub contract:
/// 1 when every missile arrived, and the actors the flights passed (A4+0x186e). Fallback: the
/// tile paths of [`paths`], nothing drawn.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fly(
    s: &mut State,
    _io: &Io,
    x0: i16,
    y0: i16,
    xs: &mut [i16],
    ys: &mut [i16],
    range: i16,
    _tile: u16,
    _flags: u16,
) -> (i16, Vec<i16>) {
    todo_stub!(
        "0x132706 fly missiles (magic seam)",
        paths(s, x0, y0, xs, ys, range)
    )
}

/// [ui] 0x1331b4 (a blast of 16 sprites of `tile` from (x, y) out to the A4-0x63e6 / -0x63d6
/// offsets, range 3; the actors reached: those the flights passed, then those on (x, y) up to
/// the first object that stops missiles, at most 0x3f). Stub contract: the list. Fallback: the
/// [`paths`] of the 16 flights, then the (x, y) part as read at 0x133238.
pub(crate) fn blast(s: &mut State, io: &Io, x: i16, y: i16, _tile: u16) -> Vec<i16> {
    todo_stub!("0x1331b4 blast (magic seam)", {
        let (bx, by) = (a4_bytes(io, -0x63e6, 16), a4_bytes(io, -0x63d6, 16));
        let mut xs: Vec<i16> = bx.iter().map(|&d| x + i16::from(d as i8)).collect();
        let mut ys: Vec<i16> = by.iter().map(|&d| y + i16::from(d as i8)).collect();
        let (_, mut list) = paths(s, x, y, &mut xs, &mut ys, 3);
        list.truncate(0x3e);
        let z = u16::from(s.map.centre.z);
        let mut o = crate::objects::begin_at(&mut s.objects, &s.tables, x as u16, y as u16, z);
        while list.len() < 0x3f && o >= 0 {
            if listed(s, o) {
                list.push(o);
            }
            if object_flags2(s, o) & 0x20 != 0 {
                break;
            }
            o = crate::objects::next_at(&mut s.objects, &s.tables);
        }
        list
    })
}

/// [ui] 0x13410a (the map overview: the 40 × 40 window cache at 4 pixels per tile, shown until
/// a key). Stub contract: draws, waits for a key, restores the view flags. Fallback: nothing
/// shown and no key read.
pub(crate) async fn overview(_state: &RefCell<State>, _io: &Io) {
    todo_stub!("0x13410a map overview (magic seam)", ())
}

/// [ui] 0x1118c6 (redraw the inventory panel: equipment, figure, list). Stub contract: draws.
/// Fallback: the panel is marked for the next redraw (A4-0x7afe).
pub(crate) fn redraw_inventory(s: &mut State) {
    todo_stub!(
        "0x1118c6 redraw the inventory panel (magic seam)",
        s.ui.panel.dirty = true
    )
}

/// [ui] 0x111be0 (draw the inventory of `actor` in the panel). Stub contract: draws. Fallback:
/// the panel is marked for the next redraw.
pub(crate) fn inventory_of(s: &mut State, _actor: u16) {
    todo_stub!(
        "0x111be0 inventory panel contents (magic seam)",
        s.ui.panel.dirty = true
    )
}

/// [objects] 0x12e3f6 (work the drawbridge crank `obj`). Stub contract: raises or lowers the
/// bridge of its quality, prints. Fallback: nothing.
pub(crate) async fn crank(_state: &RefCell<State>, _io: &Io, _obj: Slot) {
    todo_stub!("0x12e3f6 drawbridge crank (magic seam)", ())
}

/// [objects] 0x12e538 (switch the lever `obj`). Stub contract: opens or closes its
/// portcullises, prints. Fallback: nothing.
pub(crate) async fn lever(_state: &RefCell<State>, _io: &Io, _obj: Slot) {
    todo_stub!("0x12e538 switch a lever (magic seam)", ())
}

/// [objects] 0x12acc0 (is the push of `obj` from (x, y) toward `dir` blocked). Stub contract:
/// true = blocked. Fallback: the first test of its contract, blocked unless `obj` may stand on
/// the tile ahead (0x11b1c0).
pub(crate) fn push_blocked(s: &mut State, obj: Slot, x: u16, y: u16, dir: u16) -> bool {
    let d = usize::from(dir & 7);
    let nx = x.wrapping_add_signed(s.objects.defs.dx(d));
    let ny = y.wrapping_add_signed(s.objects.defs.dy(d));
    todo_stub!(
        "0x12acc0 push blocked (magic seam)",
        !crate::actors::walk::can_move_to(s, obj as u16, nx, ny)
    )
}

/// [inventory] 0x1122dc (what taking `obj` from `actor` meets first: 1 = the amulet of
/// submission stays, with its message; 2 = a lit torch burns out; else 0). Stub contract:
/// prints its own lines. Fallback: 0.
pub(crate) async fn unready_check(
    _state: &RefCell<State>,
    _io: &Io,
    _obj: Slot,
    _actor: Slot,
) -> i16 {
    todo_stub!("0x1122dc unready check (magic seam)", 0)
}

/// [inventory] 0x11236c (a readied ring of invisibility or 0x51 `obj` leaves its effect on
/// `actor`). Stub contract: 3 when one did, else 0. Fallback: 0.
pub(crate) fn release_effect(_s: &mut State, _obj: Slot, _actor: Slot) -> i16 {
    todo_stub!("0x11236c release a ring's effect (magic seam)", 0)
}

/// [inventory] 0x1110f6 (weight of `obj` in tenths of a stone, its count included). Stub
/// contract: pure. Fallback: the weight byte of its type (0x13a540), one item.
pub(crate) fn object_weight(s: &State, obj: Slot) -> i16 {
    let kind = usize::from(s.objects.kind(obj));
    todo_stub!(
        "0x1110f6 object weight (magic seam)",
        i16::from(s.tables.tileflags.weight.get(kind).copied().unwrap_or(0))
    )
}

/// [inventory] 0x11121e (the weights `actor` carries, left at A4+0x1342 / +0x1344: readied,
/// inventory). Stub contract: both words. Fallback: (0, 0).
pub(crate) fn carried_weights(_s: &mut State, _actor: u16) -> (i16, i16) {
    todo_stub!("0x11121e carried weights (magic seam)", (0, 0))
}

/// [objects] 0x128b98 (buf, 0): the decimal number typed; -1 empty, -2 not a number. Fallback:
/// mode 0 as the original (digits accumulate × 10 in a word, any other byte -2, empty -1; the
/// escape flag A4-0x7a44 taken as clear), as the converse seam reads it.
pub(crate) fn parse_number(text: &[u8]) -> i16 {
    todo_stub!("0x128b98 parse a number (magic seam)", {
        if text.is_empty() {
            return -1;
        }
        let mut v: i16 = 0;
        for &b in text {
            if !b.is_ascii_digit() {
                return -2;
            }
            v = v.wrapping_mul(10).wrapping_add(i16::from(b - b'0'));
        }
        v
    })
}

/// [gfx] 0x152800 / 0x152876 / 0x15265a / 0x151fe6 (the hail-stone sprites of 0x133562: save
/// the background under each stone, draw sprite 7 there, copy the changed rectangle, put the
/// backgrounds back). Stub contract: draws `stones` (window pixels). Fallback: nothing drawn.
pub(crate) fn draw_hail(_s: &mut State, _io: &Io, _stones: &[(i16, i16)]) {
    todo_stub!("0x152876 hail-stone sprites (magic seam)", ())
}

/// [gfx] the hit mark of 0x133562: tile 0x101 over window pixel (x, y) (0x152ffa), shown and
/// put back. Stub contract: draws. Fallback: nothing drawn.
pub(crate) fn hail_hit(_s: &mut State, _io: &Io, _x: i16, _y: i16) {
    todo_stub!("0x152ffa hail hit mark (magic seam)", ())
}

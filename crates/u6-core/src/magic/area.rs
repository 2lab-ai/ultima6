//! The spells aimed at a map location: Reappear 0x118494, the mass spells 0x118602, the fields
//! 0x11883c, Web 0x11971a, Hail Storm 0x119e8c and the winds 0x119f26, with the hunk-18 flight
//! patterns only they use (0x13331c, 0x1334a6, 0x133562; ui functions ported here because the
//! spells are their only callers).

use std::cell::RefCell;

use super::effects::{FMT_NL_S_NL, FMT_S, NEWLINE, print_at, say};
use super::seams;
use super::tables::Msg;
use crate::engine::Io;
use crate::objects::Slot;
use crate::state::State;

/// Objlist block bytes 0x22-0x23 (A4-0x7b62): the type Vanish took away, for Reappear.
const VANISHED: usize = 0x22;
/// The field objects 0x11883c lays: fire, poison, sleep, energy.
pub const FIRE_FIELD: u16 = 0x13d;
pub const POISON_FIELD: u16 = 0x13e;
pub const ENERGY_FIELD: u16 = 0x13f;
pub const SLEEP_FIELD: u16 = 0x140;
/// The web object Web lays.
pub const WEB: u16 = 0x35;

/// The start of the location spells: "Location: " when the caster is a party member (status
/// bit 7), the target read unless chosen (A4-0x7a32 = 1, 0x10790a), a newline, the caster's
/// flash. True when a map location is set (A4-0x2f02 / -0x2f00).
pub(super) async fn location(state: &RefCell<State>, io: &Io, pre: i16) -> bool {
    let (caster, member) = {
        let s = state.borrow();
        let c = super::cast::caster(&s);
        (c, s.actors.attrs.status[usize::from(c)] & 0x80 != 0)
    };
    if member {
        say(state, io, FMT_S, Msg::Location).await;
    }
    if pre == 0 {
        state.borrow_mut().ui.input.target_pending = 1;
        crate::ui::wait::read_key(state, io).await;
    }
    print_at(state, io, NEWLINE, &[]).await;
    seams::flash_actor(&mut state.borrow_mut(), io, caster, 0);
    let (x, y) = state.borrow().ui.input.target;
    x != -1 && y != -1
}

/// The missile 0x1228ba from the caster to the target, range 8; the target becomes where it
/// stopped. True when it got there.
async fn missile_to_target(state: &RefCell<State>, io: &Io, tile: u16) -> bool {
    let (caster, mut target) = {
        let s = state.borrow();
        (super::cast::caster(&s), s.ui.input.target)
    };
    let r = seams::missile(state, io, caster, 8, &mut target, tile).await;
    state.borrow_mut().ui.input.target = target;
    r != 0
}

/// A new object of type-and-frame `tf` (status 0, quantity 1) at (x, y, z) through 0x10bc4c.
pub(super) fn new_object(s: &mut State, tf: u16, x: u16, y: u16, z: u16) -> Slot {
    let loc = crate::objects::pack_xyz(x, y, z).to_packed();
    let [hi, lo] = tf.to_be_bytes();
    crate::objects::create_from_record(s, [0, loc[0], loc[1], loc[2], hi, lo, 0, 1])
}

/// The new object `new` stays at (x, y) when it may lie there (0x11b1c0), shown dissolving;
/// else it goes again. False when it went or was never made (-1: the object pool is full).
fn lay(s: &mut State, io: &Io, new: Slot, x: u16, y: u16) -> bool {
    if new < 0 {
        return false;
    }
    if crate::actors::walk::can_move_to(s, new as u16, x, y) {
        super::effects::redraw_dissolving(s, io);
        true
    } else {
        crate::objects::remove(s, new);
        false
    }
}

/// Original: 0x118494 (magic: Reappear: the type Vanish took (A4-0x7b62) back at a location)
///
/// The location prompt; no location or nothing vanished: "No effect". The missile 0x17f flies;
/// short of the target: "Blocked". Then the object comes back there (quantity 1, ok to take,
/// 0x128b0e) if it may lie there (0x11b1c0): shown dissolving, "Success", the type forgotten;
/// else it goes again and "No effect".
///
/// With the object pool full (0x10bc4c: -1) the original passes the -1 on to 0x128b0e and
/// 0x11b1c0 (0x118598, 0x1185a6); the port reports "No effect" and keeps the type.
pub async fn reappear(state: &RefCell<State>, io: &Io, pre: i16) {
    let at = location(state, io, pre).await;
    let tf = state.borrow().actors.tail_word(VANISHED);
    if !at || tf == 0 {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    if !missile_to_target(state, io, 0x17f).await {
        say(state, io, FMT_NL_S_NL, Msg::Blocked).await;
        return;
    }
    let placed = {
        let mut s = state.borrow_mut();
        let (x, y) = s.ui.input.target;
        let z = u16::from(s.map.centre.z);
        let new = new_object(&mut s, tf, x as u16, y as u16, z);
        if new >= 0 {
            crate::objects::mark_takeable(&mut s.objects, new);
        }
        lay(&mut s, io, new, x as u16, y as u16)
    };
    if placed {
        say(state, io, FMT_NL_S_NL, Msg::Success).await;
        state.borrow_mut().actors.set_tail_word(VANISHED, 0);
    } else {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
    }
}

/// Original: 0x118602 (magic: a mass spell on the actors a blast at a location reaches)
///
/// The location prompt; none: "No effect". The missile `missile` flies from the caster; short:
/// "Blocked". The blast of `blast_tile` (0x1331b4) lists the actors it reaches; on each actor
/// slot below 0x100: Mass Awaken (0x25) clears the sleep bit and gives the base type back,
/// Mass Sleep (0x26) 0x1165d2, Mass Dispel (0x36) 0x116dcc, Explosion (0x41) 0x116e60 with the
/// result 0xff, Mass Protect (0x55) status bit 0x01, Mass Curse (0x66) object status 0x40, Mass
/// Invisibility (0x67) object status 0x02 with the flash where 0x115b50 allows, Mass Charm
/// (0x73) 0x1167bc, Mass Kill (0x74) 0x11687e (their own results are dropped). Explosion and
/// Mass Kill then blow up the doors and kegs there (0x12d9ee). "Success" unless the result byte
/// (A4+0x181e, not reset here) is 0xff.
pub async fn area(
    state: &RefCell<State>,
    io: &Io,
    spell: u16,
    missile: u16,
    blast_tile: u16,
    pre: i16,
) {
    if !location(state, io, pre).await {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    if !missile_to_target(state, io, missile).await {
        say(state, io, FMT_NL_S_NL, Msg::Blocked).await;
        return;
    }
    let (caster, (x, y)) = {
        let s = state.borrow();
        (super::cast::caster(&s), s.ui.input.target)
    };
    let hit = seams::blast(&mut state.borrow_mut(), io, x, y, blast_tile);
    for a in hit.into_iter().take_while(|&a| a >= 0) {
        if a < 0x100 {
            on_actor(state, io, spell, caster, a as u16).await;
        }
    }
    if spell == 0x41 || spell == 0x74 {
        crate::objects::blast_doors(&mut state.borrow_mut(), x as u16, y as u16);
    }
    if state.borrow().magic.result != 0xff {
        say(state, io, FMT_NL_S_NL, Msg::Success).await;
    }
}

/// One actor under 0x118602.
async fn on_actor(state: &RefCell<State>, io: &Io, spell: u16, caster: u16, a: u16) {
    let i = usize::from(a);
    match spell {
        0x25 => {
            let mut s = state.borrow_mut();
            s.actors.attrs.status[i] &= !4;
            s.objects.type_frame[i] = s.actors.attrs.base_type_frame[i];
        }
        0x26 => {
            super::effects::sleep(&mut state.borrow_mut(), caster, a);
        }
        0x36 => {
            super::effects::dispel(&mut state.borrow_mut(), io, a);
        }
        0x41 => {
            super::effects::fireball(state, io, a).await;
            state.borrow_mut().magic.result = 0xff;
        }
        0x55 => state.borrow_mut().actors.attrs.status[i] |= 1,
        0x66 => state.borrow_mut().objects.obj_status[i] |= 0x40,
        0x67 => {
            let mut s = state.borrow_mut();
            if s.objects.obj_status[i] & 2 == 0
                && super::effects::can_vanish(s.objects.kind(a as Slot))
            {
                s.objects.obj_status[i] |= 2;
                seams::flash_actor(&mut s, io, a, 1);
            }
        }
        0x73 => super::effects::charm(state, io, caster, a).await,
        0x74 => {
            super::effects::kill(state, io, caster, a).await;
        }
        _ => {}
    }
}

/// Original: 0x11883c (magic: a field spell: Fire Field (0x33), Poison Field (0x37), Sleep Field
/// (0x38), Energy Field (0x40) at a location)
///
/// The location prompt; none: "No effect". The missile `missile`; short: "Blocked". The field
/// object of the spell (0x13d, 0x13e, 0x140, 0x13f; status 0, quantity 1) is made there
/// (0x10bc4c) and kept when it may lie there (0x11b1c0): shown dissolving, "Success"; else it
/// goes again and "Not possible". Dispel Field (0x21, 0x116f2c) takes these types away.
///
/// With the object pool full (0x10bc4c: -1) the original passes the -1 on to 0x11b1c0
/// (0x1189f6); the port lays nothing and reports "Not possible".
pub async fn field(state: &RefCell<State>, io: &Io, spell: u16, missile: u16, pre: i16) {
    if !location(state, io, pre).await {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    if !missile_to_target(state, io, missile).await {
        say(state, io, FMT_NL_S_NL, Msg::Blocked).await;
        return;
    }
    let kind = match spell {
        0x33 => FIRE_FIELD,
        0x37 => POISON_FIELD,
        0x38 => SLEEP_FIELD,
        0x40 => ENERGY_FIELD,
        _ => return,
    };
    let placed = {
        let mut s = state.borrow_mut();
        let (x, y) = s.ui.input.target;
        let z = u16::from(s.map.centre.z);
        let new = new_object(&mut s, kind, x as u16, y as u16, z);
        lay(&mut s, io, new, x as u16, y as u16)
    };
    let m = if placed {
        Msg::Success
    } else {
        Msg::NotPossible
    };
    say(state, io, FMT_NL_S_NL, m).await;
}

/// The location of object `o` as words.
fn xy(s: &State, o: u16) -> (i16, i16) {
    (s.objects.x(o as Slot) as i16, s.objects.y(o as Slot) as i16)
}

/// Original: 0x1334a6 (ui: the Web flight: nine missiles of the web tile (base tile of 0x35)
/// from `actor` to (x, y) and its eight neighbours (A4-0x61ce / -0x61c6), range 7, flags 0xc;
/// 1 when all arrived (0x132706))
pub fn web_flight(s: &mut State, io: &Io, actor: u16, x: i16, y: i16) -> i16 {
    let d = &s.objects.defs;
    let mut xs: Vec<i16> = (0..8).map(|i| x.wrapping_add(d.dx(i))).collect();
    let mut ys: Vec<i16> = (0..8).map(|i| y.wrapping_add(d.dy(i))).collect();
    xs.push(x);
    ys.push(y);
    let tile = s
        .tables
        .basetile
        .base
        .get(usize::from(WEB))
        .copied()
        .unwrap_or(0);
    let (ax, ay) = xy(s, actor);
    seams::fly(s, io, ax, ay, &mut xs, &mut ys, 7, tile, 0xc).0
}

/// Original: 0x11971a (magic: Web: webs (0x35) on the location and its eight neighbours)
///
/// The location prompt; none: "No effect". The Web flight (0x1334a6); not all arrived:
/// "Blocked". Then for x − 1..=x + 1 (columns outside), y − 1..=y + 1 a web is made at (0, 0) on
/// the level (0x10bc4c) and moved to the cell (0x10c9d2) where it may lie (0x11b1c0), else it
/// goes again; the view flags, "Success". A web the full object pool cannot make (0x10bc4c: -1)
/// leaves its cell empty (the original passes the -1 on to 0x11b1c0, 0x119818).
pub async fn web(state: &RefCell<State>, io: &Io, pre: i16) {
    if !location(state, io, pre).await {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    let reached = {
        let mut s = state.borrow_mut();
        let caster = super::cast::caster(&s);
        let (x, y) = s.ui.input.target;
        web_flight(&mut s, io, caster, x, y)
    };
    if reached == 0 {
        say(state, io, FMT_NL_S_NL, Msg::Blocked).await;
        return;
    }
    {
        let mut s = state.borrow_mut();
        let (tx, ty) = s.ui.input.target;
        let z = u16::from(s.map.centre.z);
        for x in tx - 1..=tx + 1 {
            for y in ty - 1..=ty + 1 {
                let w = new_object(&mut s, WEB, 0, 0, z);
                if w < 0 {
                    continue;
                }
                if crate::actors::walk::can_move_to(&mut s, w as u16, x as u16, y as u16) {
                    crate::objects::move_to(&mut s, w, x as u16, y as u16, z);
                } else {
                    crate::objects::remove(&mut s, w);
                }
            }
        }
        crate::map::view::update_view(&mut s);
    }
    say(state, io, FMT_NL_S_NL, Msg::Success).await;
}

/// Original: 0x119e8c (magic: Hail Storm: the location prompt; none: "No effect"; else the
/// storm 0x133562 there and "Success")
pub async fn hail_storm(state: &RefCell<State>, io: &Io, pre: i16) {
    if !location(state, io, pre).await {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    let (x, y) = state.borrow().ui.input.target;
    hail(&mut state.borrow_mut(), io, x, y);
    crate::combat::play_effects(state, io).await;
    say(state, io, FMT_NL_S_NL, Msg::Success).await;
}

/// One hail stone of 0x133562: where it lands and where it is (pixels of the map).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stone {
    land: (i16, i16),
    at: (i16, i16),
}

/// Original: 0x133562 (ui: a hail storm over tile (x, y): up to six stones falling at once,
/// random 10..30 of them; returns the experience the hits were worth)
///
/// Each round a stone starts with chance 1 in 8 (random 0..7 = 0, drawn only while stones are
/// left and a place is free): it lands at x × 16 + 1..30 + 1..30 − 23 (two draws), the same
/// for y, and starts 50 pixels up and left of there. Every fourth round the stones are drawn
/// (sprite 7, tile 0x18e). Each round every falling stone moves a pixel down and right; one
/// that lands draws random 400..500 (unused) and hurts each actor on its tile by 1 (0x1231be,
/// after clearing the combat word A4-0x69a4, which combat owns), showing tile 0x101 over the
/// ones in the window. The view is refreshed at the end. The code never waits (the Amiga draws as
/// fast as it runs), so the port records each copy to the screen as a frame and the caller
/// shows them ([`crate::combat::play_effects`]), the refreshed window last.
pub fn hail(s: &mut State, io: &Io, x: i16, y: i16) -> i16 {
    let mut stones: [Option<Stone>; 6] = [None; 6];
    let mut exp: i16 = 0;
    let mut frame = 0;
    let mut left = s.rng.range(10, 0x1e) as i16;
    seams::hail_start(s);
    let mut falling = false;
    let z = u16::from(s.map.centre.z);
    while left != 0 || falling {
        let free = stones.iter().position(Option::is_none);
        if left != 0
            && let Some(i) = free
            && s.rng.range(0, 7) == 0
        {
            let lx = (x << 4) + s.rng.range(1, 0x1e) as i16 + s.rng.range(1, 0x1e) as i16 - 0x17;
            let ly = (y << 4) + s.rng.range(1, 0x1e) as i16 + s.rng.range(1, 0x1e) as i16 - 0x17;
            stones[i] = Some(Stone {
                land: (lx, ly),
                at: (lx - 0x32, ly - 0x32),
            });
            left -= 1;
        }
        frame += 1;
        falling = false;
        if frame >= 4 {
            let c = s.map.centre;
            let (ox, oy) = ((c.x as i16 - 5) << 4, (c.y as i16 - 5) << 4);
            let shown: Vec<(i16, i16)> = stones
                .iter()
                .flatten()
                .map(|st| (st.at.0 - ox, st.at.1 - oy))
                .filter(|&(px, py)| px > 0xf && px < 0x98 && py > 0xf && py < 0x98)
                .collect();
            seams::draw_hail(s, io, &shown);
            frame = 0;
        }
        for slot in stones.iter_mut() {
            let Some(st) = slot else { continue };
            st.at = (st.at.0 + 1, st.at.1 + 1);
            if st.at != st.land {
                falling = true;
                continue;
            }
            let (tx, ty) = (st.land.0 >> 4, st.land.1 >> 4);
            *slot = None;
            s.rng.range(400, 500);
            let mut o =
                crate::objects::begin_at(&mut s.objects, &s.tables, tx as u16, ty as u16, z);
            while o >= 0 {
                if o < 0x100 {
                    exp = exp.wrapping_add(seams::hurt(s, o as u16, 1));
                    let c = s.map.centre;
                    let px = ((tx - c.x as i16 + 5) << 4) + 8;
                    let py = ((ty - c.y as i16 + 5) << 4) + 8;
                    if (8..0xa8).contains(&px) && (8..0xa8).contains(&py) {
                        seams::hail_hit(s, io, px, py);
                    }
                }
                o = crate::objects::next_at(&mut s.objects, &s.tables);
            }
        }
    }
    crate::gfx::view::refresh_view(s, io);
    crate::combat::present::capture_window(s, 0);
    exp
}

/// Original: 0x13331c (ui: the wind flight: five missiles of `tile` from `actor` toward (x, y),
/// the two directions the A4-0x6929 grid gives for the offset (high and low nibble of the byte
/// at A4-0x6924 + 11 dy + dx) spreading them: (x, y), one step either way, two steps either
/// way; range 7, flags 6 (0x132706); returns the actors passed (A4+0x186e))
pub fn wind_flight(s: &mut State, io: &Io, actor: u16, x: i16, y: i16, tile: u16) -> Vec<i16> {
    let (ax, ay) = xy(s, actor);
    let off = 11 * i32::from(y.wrapping_sub(ay)) + i32::from(x.wrapping_sub(ax));
    let dirs = seams::a4_bytes(io, -0x6924 + off, 1)[0];
    let (d1, d2) = (usize::from(dirs >> 4), usize::from(dirs & 0xf));
    let d = &s.objects.defs;
    let (x1, y1) = (x + d.dx(d1), y + d.dy(d1));
    let (x2, y2) = (x + d.dx(d2), y + d.dy(d2));
    let mut xs = [x, x1, x2, x1 + d.dx(d1), x2 + d.dx(d2)];
    let mut ys = [y, y1, y2, y1 + d.dy(d1), y2 + d.dy(d2)];
    seams::fly(s, io, ax, ay, &mut xs, &mut ys, 7, tile, 6).1
}

/// Original: 0x119f26 (magic: the winds: a fan of five missiles of `tile` toward a location,
/// the spell on every actor it passes)
///
/// The location prompt; none, or the caster's own place: "No effect". The wind flight
/// (0x13331c); on each actor slot below 0x100: Flame Wind (0x53) 0x116e60, Poison Wind (0x57)
/// 0x11663e, Energy Wind (0x62) random 1..30 damage (0x1231be), its experience to the caster
/// (0x122104), the wound line (0x12395a) and the hit's reaction (0x123c56, 1), Death Wind
/// (0x71) 0x11687e. No result line.
pub async fn wind(state: &RefCell<State>, io: &Io, spell: u16, tile: u16, pre: i16) {
    let at = location(state, io, pre).await;
    let (caster, target, own) = {
        let s = state.borrow();
        let c = super::cast::caster(&s);
        (c, s.ui.input.target, xy(&s, c))
    };
    if !at || target == own {
        say(state, io, FMT_NL_S_NL, Msg::NoEffect).await;
        return;
    }
    let hit = wind_flight(
        &mut state.borrow_mut(),
        io,
        caster,
        target.0,
        target.1,
        tile,
    );
    for a in hit.into_iter().take_while(|&a| a >= 0) {
        if a >= 0x100 {
            continue;
        }
        let a = a as u16;
        match spell {
            0x53 => {
                super::effects::fireball(state, io, a).await;
            }
            0x57 => {
                super::effects::poison(&mut state.borrow_mut(), caster, a);
            }
            0x62 => {
                {
                    let mut s = state.borrow_mut();
                    let d = s.rng.range(1, 0x1e) as i16;
                    let exp = seams::hurt(&mut s, a, d);
                    seams::add_experience(&mut s, caster, exp);
                }
                seams::wound_line(state, io, a).await;
                seams::provoke(&mut state.borrow_mut(), a, caster, 1);
            }
            0x71 => {
                super::effects::kill(state, io, caster, a).await;
            }
            _ => {}
        }
    }
}

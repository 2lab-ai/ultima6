//! Calls from the spell code into routines other units own: combat (hunk 13: damage, missiles,
//! experience, hostility, the wound line; hunk 18: the flights), objects (hunk 15), Use's
//! levers, inventory (hunk 8) and a few graphics helpers. Each is the narrowest signature the
//! spells need; since the M2 integration the ported ones call their owner (graphics 0x1521f4
//! and the hail sprites 0x15265a / 0x152800 / 0x152876 since the pointer merge), the rest
//! (0x132338) keep a stub contract and an explicit fallback.

use std::cell::RefCell;

use crate::engine::Io;
use crate::objects::Slot;
use crate::state::State;
use crate::todo_stub;

/// [gfx] 0x1521f4 (fill the inclusive pixel rectangle in the plot colour, 0x152174; XOR when
/// A4-0x617c is set, into the back buffer when A4-0x6179 is set):
/// [`crate::gfx::blit::fill_rect`].
pub(crate) fn fill_rect(s: &mut State, io: &Io, x0: u16, y0: u16, x1: u16, y1: u16) {
    let gfx = &s.gfx;
    let draw = |fb: &mut u6_render::Framebuffer| {
        crate::gfx::blit::fill_rect(gfx, fb, x0, y0, x1, y1);
    };
    if gfx.plot_back {
        io.draw_back(draw);
    } else {
        io.draw(draw);
    }
}

/// [ui] 0x1324ae (redraw the map-window border tiles 0x1b0..0x1b7 that the rectangle (x0, y0)
/// ..(x1, y1) touches, into the back buffer): [`crate::combat::present::draw_borders`].
pub(crate) fn redraw_border(s: &mut State, x0: u16, y0: u16, x1: u16, y1: u16) {
    crate::combat::present::draw_borders(s, x0 as i16, y0 as i16, x1 as i16, y1 as i16);
}

/// [ui] 0x132338 (flash an actor on the map: when `actor` is on the map and in view, its tile
/// is drawn with effect `mode` (0x10e596(2)) and the window presented dissolving, then redrawn
/// plain). Stub contract: no result. Fallback: no flash.
pub(crate) fn flash_actor(_s: &mut State, _io: &Io, _actor: u16, _mode: u16) {
    todo_stub!("0x132338 flash an actor on the map", ())
}

/// [combat] 0x1228ba (a missile of tile `tile` from `from` toward (x, y) within `range`; the
/// flight may stop short and move (x, y); non-zero when it reached the target):
/// [`crate::combat::attack::shoot`], then its recorded frames and lines played
/// ([`crate::combat::play_effects`]).
pub(crate) async fn missile(
    state: &RefCell<State>,
    io: &Io,
    from: u16,
    range: i16,
    target: &mut (i16, i16),
    tile: u16,
) -> i16 {
    let reached = {
        let mut s = state.borrow_mut();
        let (mut x, mut y) = *target;
        let r = crate::combat::attack::shoot(&mut s, from as i16, &mut x, &mut y, range, tile);
        *target = (x, y);
        r
    };
    crate::combat::play_effects(state, io).await;
    reached
}

/// [combat] 0x1240fa (the object a target stands for: a part of a multi-tile creature gives
/// its head, a horse its rider): [`crate::combat::leaves::owner`].
pub(crate) fn target_owner(s: &mut State, obj: i16) -> i16 {
    crate::combat::leaves::owner(s, obj)
}

/// [combat] 0x122076 (does `target` resist `caster`'s spell: random 1..30 < int(target) / 2 −
/// int(caster) + 15; true = resisted): [`crate::combat::leaves::contest`].
pub(crate) fn resists(s: &mut State, caster: u16, target: u16) -> bool {
    crate::combat::leaves::contest(s, caster as i16, target as i16)
}

/// [combat] 0x1231be (hurt `target` by `damage`: hit points, death, splitting slimes; queues
/// its lines; the experience the hit is worth): [`crate::combat::damage::hurt`].
pub(crate) fn hurt(s: &mut State, target: u16, damage: i16) -> i16 {
    crate::combat::damage::hurt(s, target as i16, damage)
}

/// [combat] 0x122104 (add `exp` experience to `actor`, at most 9999):
/// [`crate::combat::leaves::add_experience`].
pub(crate) fn add_experience(s: &mut State, actor: u16, exp: i16) {
    crate::combat::leaves::add_experience(s, actor as i16, exp);
}

/// [combat] 0x123c56 (`target` turns on `attacker` after a hit of `damage`; may yell for
/// help): [`crate::combat::damage::yell_for_help`].
pub(crate) fn provoke(s: &mut State, target: u16, attacker: u16, damage: i16) {
    crate::combat::damage::yell_for_help(s, target as i16, attacker as i16, damage);
}

/// [combat] 0x12395a (print the wound line of `actor`: "grazed", "barely wounded" ...):
/// [`crate::combat::damage::report_wounds`], then the lines and frames it queued.
pub(crate) async fn wound_line(state: &RefCell<State>, io: &Io, actor: u16) {
    crate::combat::damage::report_wounds(&mut state.borrow_mut(), actor as i16);
    crate::combat::play_effects(state, io).await;
}

/// [combat] 0x1230ce (the hit flash on `actor` and its redraw):
/// [`crate::combat::damage::flash`].
pub(crate) fn hit_flash(s: &mut State, actor: u16) {
    crate::combat::damage::flash(s, actor as i16);
}

/// [combat] 0x1221da (an actor put to sleep lies down): [`crate::combat::damage::lie_down`].
pub(crate) fn lie_down(s: &mut State, actor: u16) {
    crate::combat::damage::lie_down(s, actor as i16);
}

/// [objects] 0x12bbc0 (lock (`lock`, stack words 1, 0) or magically unlock (0, 1) the door
/// `obj`; D1 = 0, no key): [`crate::objects::commands::door`].
pub(crate) fn door(s: &mut State, obj: i16, lock: bool) {
    crate::objects::commands::door(s, Slot::from(obj), false, lock, !lock);
}

/// [objects] 0x12bdfa (the same for a chest, type 0x62): [`crate::objects::commands::chest`].
pub(crate) fn chest(s: &mut State, obj: i16, lock: bool) {
    crate::objects::commands::chest(s, Slot::from(obj), false, lock, !lock);
}

/// [inventory] 0x110f96 (the equipment slot object `obj` readies into, -1 when it cannot be
/// readied): [`crate::inventory::ready_slot`].
pub(crate) fn ready_slot(s: &State, obj: i16) -> i16 {
    crate::inventory::ready_slot(s, obj)
}

/// Bytes of the user's `game` at `A4 + off` (zeros where the file or the address is missing):
/// the small A4 tables the spells read at run time (spec 03 §5).
pub(crate) fn a4_bytes(io: &Io, off: i32, len: usize) -> Vec<u8> {
    io.game_image()
        .and_then(|img| img.a4_bytes(off, len).ok().map(<[u8]>::to_vec))
        .unwrap_or_else(|| vec![0; len])
}

/// [objects] 0x131646 (a monster of `kind` at (x, y, z) with its stats, parts and equipment;
/// -1 when no temporary-actor slot is free): [`crate::objects::spawn_monster`].
pub(crate) fn spawn(s: &mut State, kind: u16, x: u16, y: u16, z: u16) -> Slot {
    crate::objects::spawn_monster(s, kind, x, y, z)
}

/// [ui] 0x132706 (fly a missile of `tile` from (x0, y0) toward each (xs[i], ys[i]) within
/// `range`; `flags` as the original's word; xs / ys become where each stopped):
/// [`crate::combat::present::fly`]; 1 when every missile arrived, and the actors the flights
/// passed (A4+0x186e without its -1 terminator).
#[allow(clippy::too_many_arguments)]
pub(crate) fn fly(
    s: &mut State,
    _io: &Io,
    x0: i16,
    y0: i16,
    xs: &mut [i16],
    ys: &mut [i16],
    range: i16,
    tile: u16,
    flags: u16,
) -> (i16, Vec<i16>) {
    let n = xs.len().min(ys.len()) as i16;
    let reached = crate::combat::present::fly(s, x0, y0, xs, ys, n, range, tile, flags);
    let passed = s
        .combat
        .passed
        .iter()
        .copied()
        .take_while(|&a| a >= 0)
        .collect();
    (reached, passed)
}

/// [ui] 0x1331b4 (a blast of 16 sprites of `tile` from (x, y); the actors reached):
/// [`crate::combat::present::blast`] (which reads the offsets A4-0x63e6 / -0x63d6 itself),
/// without its -1 terminator.
pub(crate) fn blast(s: &mut State, _io: &Io, x: i16, y: i16, tile: u16) -> Vec<i16> {
    let mut hit = crate::combat::present::blast(s, x, y, tile);
    hit.retain(|&a| a >= 0);
    hit
}

/// [inventory] 0x111be0 (the inventory of `actor` in the panel: recount, then 0x1118c6):
/// [`crate::inventory::panel_contents`].
pub(crate) async fn inventory_of(state: &RefCell<State>, io: &Io, actor: u16) {
    crate::inventory::panel_contents(state, io, actor as Slot).await;
}

/// [objects] 0x12e3f6 (work the drawbridge crank `obj`): [`crate::usecmd::drawbridge`].
pub(crate) async fn crank(state: &RefCell<State>, io: &Io, obj: Slot) {
    crate::usecmd::drawbridge(crate::usecmd::Ux::new(state, io), obj).await;
}

/// [objects] 0x12e538 (switch the lever `obj`): [`crate::usecmd::lever`].
pub(crate) async fn lever(state: &RefCell<State>, io: &Io, obj: Slot) {
    crate::usecmd::lever(crate::usecmd::Ux::new(state, io), obj).await;
}

/// [objects] 0x12acc0 (is the push of `obj` from (x, y) toward `dir` blocked; true =
/// blocked): [`crate::objects::commands::push_blocked`].
pub(crate) fn push_blocked(s: &mut State, obj: Slot, x: u16, y: u16, dir: u16) -> bool {
    crate::objects::commands::push_blocked(s, obj, x, y, usize::from(dir & 7))
}

/// [inventory] 0x1122dc (what taking `obj` meets first: 1 = the amulet of submission stays,
/// with its message; 2 = a lit torch burns out; else 0): [`crate::inventory::unready_check`]
/// (which reads only the object; the victim is not an argument of the original's check).
pub(crate) async fn unready_check(state: &RefCell<State>, io: &Io, obj: Slot, _actor: Slot) -> i16 {
    crate::inventory::unready_check(state, io, obj).await
}

/// [inventory] 0x11236c (a readied ring of invisibility or 0x51 `obj` leaves its effect on
/// `actor`; 3 when one did, else 0): [`crate::inventory::release_effect`].
pub(crate) fn release_effect(s: &mut State, io: &Io, obj: Slot, actor: Slot) -> i16 {
    crate::inventory::release_effect(s, io, obj, actor)
}

/// [inventory] 0x1110f6 (weight of `obj` in tenths of a stone, its count included):
/// [`crate::inventory::object_weight`].
pub(crate) fn object_weight(s: &State, obj: Slot) -> i16 {
    crate::inventory::object_weight(s, obj)
}

/// [inventory] 0x11121e (the weights `actor` carries, left at A4+0x1342 / +0x1344: readied,
/// inventory): [`crate::inventory::weights`], then both words.
pub(crate) fn carried_weights(s: &mut State, actor: u16) -> (i16, i16) {
    crate::inventory::weights(s, actor as Slot);
    (s.inventory.equip_weight, s.inventory.carried_weight)
}

/// [objects] 0x128b98 (buf, 0): the decimal number typed; -1 empty, -2 not a number:
/// [`crate::objects::commands::parse_answer`] in mode 0.
pub(crate) fn parse_number(s: &State, text: &[u8]) -> i16 {
    crate::objects::commands::parse_answer(s, text, 0)
}

/// Sprite slot of the hail stone (0x1335ba) and of the hit mark's saved background (0x13398a);
/// slots 0..=5 save the backgrounds under the stones.
const STONE_SLOT: u16 = 7;
const HIT_SLOT: u16 = 6;
/// The stone's object tile and its turning centre (0x1335a0..0x1335c2: angle 0).
const STONE_TILE: u16 = 0x18e;
const STONE_CENTRE: i16 = 0x7f;
/// The hit mark's tile (0x1339a2).
const HIT_TILE: u16 = 0x101;
/// The changed rectangle A4+0x1900.. when nothing changed (0x1335ca..0x1335dc).
const NO_RECT: [i16; 4] = [0xa8, 0xa8, 7, 7];

/// [gfx] the start of 0x133562 (0x1335ac..0x1335dc): sprite 7 built from tile 0x18e, unturned
/// (0x15265a, [`crate::gfx::sprite::prepare`]), and no changed rectangle (combat's
/// A4+0x1900.. words, [`crate::combat::CombatState::dirty`]).
pub(crate) fn hail_start(s: &mut State) {
    let c = STONE_CENTRE;
    crate::gfx::sprite::prepare(&mut s.gfx, STONE_SLOT, STONE_TILE, 0, c, c);
    s.combat.dirty = NO_RECT;
}

/// The copy of the changed back-buffer rectangle to the screen (0x151fe6, whole bytes of 8
/// pixels), recorded as a frame that [`crate::combat::play_effects`] shows for `hold` vblanks.
fn show_rect(s: &mut State, io: &Io, [x0, y0, x1, y1]: [i16; 4], hold: u8) {
    let w = |v: i16| v as u16;
    io.with_buffers(|back, front| {
        crate::gfx::blit::copy_rect(back, front, w(x0), w(y0), w(x1), w(y1), w(x0), w(y0));
    });
    let (cx0, cx1) = ((x0 as usize) & !7, (x1 as usize) | 7);
    crate::combat::present::capture(s, cx0, y0 as usize, cx1, y1 as usize, hold);
}

/// [gfx] the stones of 0x133562 (0x1336f8..0x133892), window pixels `stones`: the background
/// under stone k saved in sprite slot k (0x152800), the stone (sprite 7) drawn over each
/// (0x152876), every one growing the changed rectangle (0x13242e,
/// [`crate::combat::present::extend_dirty`]); the rectangle copied to the screen (0x151fe6) and
/// reset; the backgrounds put back (0x152876 on a saved slot), growing it again for the next
/// copy. Slot k is the stone's place among those shown (the original numbers them by their
/// array index; both loops use the same one, so the pixels are the same).
pub(crate) fn draw_hail(s: &mut State, io: &Io, stones: &[(i16, i16)]) {
    let w = |v: i16| v as u16;
    for (k, &(x, y)) in stones.iter().enumerate() {
        let gfx = &mut s.gfx;
        io.draw_back(|back| crate::gfx::sprite::save(gfx, back, k as u16, w(x), w(y)));
        crate::combat::present::extend_dirty(s, x, y);
    }
    for &(x, y) in stones {
        let gfx = &s.gfx;
        io.draw_back(|back| crate::gfx::sprite::draw(gfx, back, STONE_SLOT, w(x), w(y)));
        crate::combat::present::extend_dirty(s, x, y);
    }
    let rect = s.combat.dirty;
    if rect[0] <= rect[2] && rect[1] <= rect[3] {
        show_rect(s, io, rect, 1);
    }
    s.combat.dirty = NO_RECT;
    for (k, &(x, y)) in stones.iter().enumerate() {
        let gfx = &s.gfx;
        io.draw_back(|back| crate::gfx::sprite::draw(gfx, back, k as u16, w(x), w(y)));
        crate::combat::present::extend_dirty(s, x, y);
    }
}

/// [gfx] the hit mark of 0x133562 (0x133984..0x1339ea) at window pixel (x, y): the background
/// saved in sprite slot 6 (0x152800), tile 0x101 over it (0x152ffa), the 16 × 16 cell copied to
/// the screen (0x151fe6), the background put back (0x152876) and the rectangle grown
/// (0x13242e), so the next stones' copy erases the mark from the screen.
pub(crate) fn hail_hit(s: &mut State, io: &Io, x: i16, y: i16) {
    let w = |v: i16| v as u16;
    let gfx = &mut s.gfx;
    io.draw_back(|back| crate::gfx::sprite::save(gfx, back, HIT_SLOT, w(x), w(y)));
    let gfx = &s.gfx;
    io.draw_back(|back| crate::gfx::blit::blit_back(gfx, back, HIT_TILE, w(x - 8), w(y - 8)));
    show_rect(s, io, [x - 8, y - 8, x + 7, y + 7], 0);
    let gfx = &s.gfx;
    io.draw_back(|back| crate::gfx::sprite::draw(gfx, back, HIT_SLOT, w(x), w(y)));
    crate::combat::present::extend_dirty(s, x, y);
}

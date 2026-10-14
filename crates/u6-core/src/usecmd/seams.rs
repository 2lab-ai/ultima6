//! Calls from Use into code it does not own. Each is the narrowest signature Use needs; ported
//! owners are called directly, the others keep a `todo_stub!` with an explicit fallback and a
//! one-line contract. Ported (integration M2): combat (hunk 13) = 0x1240fa, 0x1228ba, 0x1231be,
//! 0x1230ce, 0x12395a, 0x1221da, 0x121d78, 0x122fbe; ui (hunk 18, with combat) = 0x1331b4;
//! objects commands (hunk 15) = 0x12948a (0x129544 is called there directly); ui = 0x107cba;
//! magic (hunk 11) = 0x1165a4, 0x11a470, 0x116d0e, 0x119200, 0x11a586, 0x119afc, 0x115b7c,
//! 0x116526. Not ported: ui = 0x105016, 0x1118c6, 0x11121e, 0x111be0, 0x132338, the weight
//! words; sound = 0x110878, 0x110912, 0x100fa4; graphics = 0x1623be; the ending = 0x133d1e,
//! 0x13513a.

use crate::engine::Io;
use crate::objects::{NONE, Slot};
use crate::state::State;
use crate::todo_stub;

// ---- combat -----------------------------------------------------------------------------------

/// 0x1240fa(obj): the actor an object stands for (the rider of a horse 0x1a9, the head of a
/// multi-tile part), -1 for none: [`crate::combat::leaves::owner`].
pub(crate) fn actor_of(s: &mut State, obj: Slot) -> Slot {
    crate::combat::leaves::owner(s, obj)
}

/// 0x1231be(actor, damage): deal damage: [`crate::combat::damage::hurt`].
pub(crate) fn damage(s: &mut State, actor: Slot, amount: i16) {
    crate::combat::damage::hurt(s, actor, amount);
}

/// 0x1230ce(actor): show an actor hit / poisoned (flash): [`crate::combat::damage::flash`].
pub(crate) fn hit_flash(s: &mut State, actor: Slot) {
    crate::combat::damage::flash(s, actor);
}

/// 0x12395a(actor): after damage, print how badly `actor` is hurt:
/// [`crate::combat::damage::report_wounds`].
pub(crate) fn report_wounds(s: &mut State, actor: Slot) {
    crate::combat::damage::report_wounds(s, actor);
}

/// 0x1221da(actor): an actor drops (asleep, paralysed, dead: its lying frame):
/// [`crate::combat::damage::lie_down`].
pub(crate) fn status_frame(s: &mut State, actor: Slot) {
    crate::combat::damage::lie_down(s, actor);
}

/// 0x1228ba(from, range, &x, &y, tile): a missile flies from `from` towards (x, y); returns
/// its result (1 when it got there, 0 when something stopped it) and where it ended
/// (A4+0x185e / +0x1860): [`crate::combat::attack::shoot`].
pub(crate) fn missile(
    s: &mut State,
    from: Slot,
    range: i16,
    x: i16,
    y: i16,
    tile: u16,
) -> (i16, i16, i16) {
    let (mut x, mut y) = (x, y);
    let r = crate::combat::attack::shoot(s, from, &mut x, &mut y, range, tile);
    (r, x, y)
}

/// 0x121d78: leave combat mode: [`crate::combat::mode::break_off`].
pub(crate) fn end_combat(s: &mut State) {
    crate::combat::mode::break_off(s);
}

/// 0x122fbe(from, to): actor `to` becomes a copy of actor `from` (flags, type, attributes;
/// boarding makes actor 0 a copy of the first member, 0x11cd78 passes D0 = A4+0x47ec,
/// D1 = 0): [`crate::combat::leaves::copy_actor`].
pub(crate) fn copy_actor(s: &mut State, from: Slot, to: Slot) {
    crate::combat::leaves::copy_actor(s, from, to);
}

// ---- magic ------------------------------------------------------------------------------------

/// 0x1165a4(type): one of the 4 door types at A4-0x6b54: [`crate::magic::is_door`].
pub(crate) fn is_door(s: &State, ty: u16) -> bool {
    crate::magic::is_door(s, ty)
}

/// 0x11a470: the sextant line (latitude, longitude): [`crate::magic::locate`].
pub(crate) async fn sextant(state: &std::cell::RefCell<State>, io: &Io) {
    crate::magic::locate(state, io).await;
}

/// 0x116d0e(6): the heal effect a potion casts on the selection: [`crate::magic::heal`].
pub(crate) fn heal(s: &mut State, n: i16) {
    crate::magic::heal(s, n as u16);
}

/// 0x119200(0x49): x-ray vision (the potion passes the X-ray spell number):
/// [`crate::magic::xray`].
pub(crate) async fn xray(state: &std::cell::RefCell<State>, io: &Io) {
    crate::magic::xray(state, io, 0x49).await;
}

/// 0x11a586(spell, 0): cast a spell from an object (wand), its target not chosen yet:
/// [`crate::magic::dispatch`].
pub(crate) async fn cast_from_object(state: &std::cell::RefCell<State>, io: &Io, spell: u8) {
    crate::magic::dispatch(state, io, u16::from(spell), false).await;
}

/// 0x119afc(x, y, 0): the free view (Wizard Eye) from (x, y) without the flashes:
/// [`crate::magic::wizard_eye`].
pub(crate) async fn gate_travel(state: &std::cell::RefCell<State>, io: &Io, x: i16, y: i16) {
    crate::magic::wizard_eye(state, io, x, y, 0).await;
}

/// 0x115b7c(1): the shrine's flash after a mantra: [`crate::magic::flash_window`].
pub(crate) async fn shrine_flash(state: &std::cell::RefCell<State>, io: &Io) {
    crate::magic::flash_window(state, io, 1).await;
}

/// 0x116526(actor, type, 0): the object of `kind` the actor carries at any depth, -1:
/// [`crate::magic::carried_object`].
pub(crate) fn find_carried(s: &mut State, actor: Slot, kind: u16) -> Slot {
    crate::magic::carried_object(&s.objects, actor, kind as i16, 0)
}

// ---- converse ---------------------------------------------------------------------------------

/// 0x11304a(type, quality): the first party member on screen who carries an object of `kind`
/// (and `quality`, 0 = any), -1. Converse ports it in a private module; composed here from its
/// public parts: on screen (converse 0x1125b8's helper) and the carried-object test (0x116526).
pub(crate) fn member_with(s: &mut State, kind: u16, quality: i16) -> Slot {
    for i in 0..usize::from(s.actors.party.size) {
        let m = Slot::from(super::member(s, i));
        if crate::converse::on_screen(s, m)
            && crate::magic::carried_object(&s.objects, m, kind as i16, quality) != -1
        {
            return m;
        }
    }
    NONE
}

// ---- objects commands (hunk 15) ---------------------------------------------------------------

/// 0x12948a(obj): the first non-actor object at `obj`'s tile that is not `obj` and whose tile
/// has no flags3 bit 4, -1: [`crate::objects::commands::object_under`].
pub(crate) fn object_under(s: &mut State, obj: Slot) -> Slot {
    crate::objects::commands::object_under(s, obj)
}

// ---- ui ---------------------------------------------------------------------------------------

/// 0x105016(px, py): the direction 0..7 from the view centre to the pointer pixel, -1 on the
/// centre. Fallback -1 (the leader keeps facing as it is).
pub(crate) fn pointer_direction(_s: &State, _px: i16, _py: i16) -> i16 {
    todo_stub!("0x105016 direction to the pointer (usecmd seam)", -1)
}

/// 0x1118c6: redraw the inventory panel now. Fallback: the panel is redrawn at the next 0x103f64
/// (A4-0x7afe set).
pub(crate) fn redraw_inventory(s: &mut State) {
    todo_stub!("0x1118c6 redraw the inventory panel (usecmd seam)", ());
    s.ui.panel.dirty = true;
}

/// 0x111be0: refill the inventory panel's object list. Fallback: redrawn at the next 0x103f64.
pub(crate) fn inventory_contents(s: &mut State) {
    todo_stub!("0x111be0 inventory panel contents (usecmd seam)", ());
    s.ui.panel.dirty = true;
}

/// 0x11121e(actor): recompute the readied equipment words and carried weight (A4+0x1342 /
/// +0x1344). Fallback: none.
pub(crate) fn ready_changed(_s: &mut State, _actor: Slot) {
    todo_stub!("0x11121e equipment and weight words (usecmd seam)", ())
}

/// A4+0x1342 + A4+0x1344 after 0x11121e(actor): the weight the actor carries (tenths). Fallback:
/// 0 (anything can be carried).
pub(crate) fn carried_weight(_s: &mut State, _actor: Slot) -> i16 {
    todo_stub!("0x11121e carried weight (usecmd seam)", 0)
}

/// 0x132338(actor): flash the actor in the view. Fallback: none.
pub(crate) fn flash_actor(_s: &mut State, _actor: Slot) {
    todo_stub!("0x132338 flash an actor (usecmd seam)", ())
}

/// 0x1331b4(tile, x, y): explosion animation; the actors and objects it hit:
/// [`crate::combat::present::blast`] (which takes (x, y, tile)), without its -1 terminator.
pub(crate) fn blast(s: &mut State, tile: u16, x: i16, y: i16) -> Vec<Slot> {
    let mut hit = crate::combat::present::blast(s, x, y, tile);
    hit.retain(|&a| a >= 0);
    hit
}

/// 0x107cba: read a line of text (returns its bytes). Fallback: an empty line.
pub(crate) async fn read_line(state: &std::cell::RefCell<State>, io: &Io, max: i16) -> Vec<u8> {
    crate::ui::keyread::read_line(state, io, max).await.1
}

// ---- sound, graphics, the ending --------------------------------------------------------------

/// 0x110878(song): play song N, 0 = stop. Fallback: no sound.
pub(crate) fn play_song(_s: &mut State, _song: i16) {
    todo_stub!("0x110878 play song (usecmd seam)", ())
}

/// 0x110912: pick and play the area song. Fallback: no sound.
pub(crate) fn area_song(_s: &mut State) {
    todo_stub!("0x110912 pick and play the area song (usecmd seam)", ())
}

/// A4-0x35cc: the music module is loaded (instruments play only then). Fallback: false (the
/// port plays no music yet).
pub(crate) fn music_loaded(_s: &State) -> bool {
    todo_stub!(
        "0x110878 music module loaded A4-0x35cc (usecmd seam)",
        false
    )
}

/// Module byte A4-0x35cc + 0x3d2 = `note`, instrument `instrument`, then 0x100fa4 plays it.
/// Fallback: no sound.
pub(crate) fn play_instrument(_s: &mut State, _instrument: u8, _note: u8) {
    todo_stub!("0x100fa4 play an instrument note (usecmd seam)", ())
}

/// 0x100fa4 with the sample at 0x1512c8 (the machine's sound). Fallback: no sound.
pub(crate) fn play_sound(_s: &mut State) {
    todo_stub!("0x100fa4 play a sound effect (usecmd seam)", ())
}

/// 0x1623be(100): the copper effect of the machine. Fallback: none.
pub(crate) fn screen_effect(_io: &Io) {
    todo_stub!("0x1623be screen effect (usecmd seam)", ())
}

/// 0x133d1e then 0x13513a: the ending sequence after the Codex vanished. Fallback: the game
/// goes on.
pub(crate) fn ending(_s: &mut State) {
    todo_stub!("0x13513a the ending (usecmd seam)", ())
}

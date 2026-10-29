//! Calls from Use into code it does not own. Each is the narrowest signature Use needs; ported
//! owners are called directly, the others keep a `todo_stub!` with an explicit fallback and a
//! one-line contract. Ported (integration M2): combat (hunk 13) = 0x1240fa, 0x1228ba, 0x1231be,
//! 0x1230ce, 0x12395a, 0x1221da, 0x121d78, 0x122fbe; ui (hunk 18, with combat) = 0x1331b4;
//! objects commands (hunk 15) = 0x12948a (0x129544 is called there directly); ui = 0x107cba,
//! 0x132338 (in gfx);
//! magic (hunk 11) = 0x1165a4, 0x11a470, 0x116d0e, 0x119200, 0x11a586, 0x119afc, 0x115b7c,
//! 0x116526; inventory (hunk 8) = 0x105016, 0x1118c6, 0x11121e, 0x111be0 and the weight words.
//! sound = 0x110878, 0x110912, 0x100fa4; converse = 0x11304a (called there directly). Ported
//! here: graphics = 0x1623be (a wait only Use calls). Not ported: the ending =
//! 0x133d1e, 0x13513a.

use crate::engine::Io;
use crate::objects::Slot;
use crate::state::State;

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

// ---- objects commands (hunk 15) ---------------------------------------------------------------

/// 0x12948a(obj): the first non-actor object at `obj`'s tile that is not `obj` and whose tile
/// has no flags3 bit 4, -1: [`crate::objects::commands::object_under`].
pub(crate) fn object_under(s: &mut State, obj: Slot) -> Slot {
    crate::objects::commands::object_under(s, obj)
}

// ---- ui ---------------------------------------------------------------------------------------

/// 0x105016(px, py): the direction 0..7 from the pointer origin to the pointer pixel, -1 on the
/// origin's cell: [`crate::inventory::direction_to`].
pub(crate) fn pointer_direction(s: &State, px: i16, py: i16) -> i16 {
    crate::inventory::direction_to(s.inventory.pointer_origin, px, py)
}

/// 0x1118c6: redraw the inventory list. Port: Use's callers are synchronous here and 0x1118c6
/// prints its weight lines, so the panel is marked (A4-0x7afe) and the command loop's 0x103f64
/// redraws it before the next key.
pub(crate) fn redraw_inventory(s: &mut State) {
    s.ui.panel.dirty = true;
}

/// 0x111be0: refill the inventory panel of the actor or container it shows: the count and the
/// cells now ([`crate::inventory::recount`]), the drawing at the next panel redraw.
pub(crate) fn inventory_contents(s: &mut State) {
    let shown = s.ui.panel.container as Slot;
    crate::inventory::recount(s, shown);
    s.ui.panel.dirty = true;
}

/// 0x11121e(actor): the readied and carried weight words (A4+0x1342 / +0x1344) and the list
/// count: [`crate::inventory::weights`].
pub(crate) fn ready_changed(s: &mut State, actor: Slot) {
    crate::inventory::weights(s, actor);
}

/// A4+0x1342 + A4+0x1344 after 0x11121e(actor): the weight the actor carries (tenths).
pub(crate) fn carried_weight(s: &mut State, actor: Slot) -> i16 {
    crate::inventory::weights(s, actor);
    s.inventory
        .equip_weight
        .wrapping_add(s.inventory.carried_weight)
}

/// 0x132338(actor, 1): flash the actor in the view, bright (D1 = 1 at both calls in 0x12d526):
/// [`crate::gfx::flash_actor`] on the polling engine's buffers.
pub(crate) fn flash_actor(s: &mut State, actor: Slot) {
    crate::engine::with_current_io(|io| crate::gfx::flash_actor(s, io, actor as u16, true));
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

/// 0x110878(song): play song N, 0 = stop: [`crate::sound::play_song`].
pub(crate) fn play_song(s: &mut State, song: i16) {
    crate::sound::play_song(s, song as u16);
}

/// 0x110912: pick and play the area song: [`crate::sound::play_area_song`].
pub(crate) fn area_song(s: &mut State) {
    crate::sound::play_area_song(s);
}

/// A4-0x35cc: the module buffer is allocated (instruments play only then). The setup
/// allocates it once (0x110878 reads into it); the port always has it.
pub(crate) fn music_loaded(_s: &State) -> bool {
    true
}

/// Module byte A4-0x35cc + 0x3d2 = `note`, then 0x100fa4 plays the buffer (asm
/// 0x12c900..0x12c908): [`crate::sound::poke_module_buffer`],
/// [`crate::sound::play_module_buffer`].
pub(crate) fn play_instrument(s: &mut State, note: u8) {
    crate::sound::poke_module_buffer(s, 0x3d2, note);
    crate::sound::play_module_buffer(s);
}

/// 0x100fa4 with the module at 0x1512c8 (the machine's sound effect):
/// [`crate::sound::play_effect_module`].
pub(crate) fn play_sound(s: &mut State) {
    crate::sound::play_effect_module(s);
}

/// Original: 0x1623be (graphics: wait `n` + 1 vertical blanks)
///
/// A `dbf` loop on D0 around the VBlank wait 0x1622be (which keeps D0); the machine 0x12cfe4
/// passes 100 while its sound effect plays.
pub(crate) async fn wait_vblanks(io: &Io, n: u16) {
    for _ in 0..=n {
        io.vblank().await;
    }
}

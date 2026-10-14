//! Calls from Use into code it does not own. Each is the narrowest signature Use needs; ported
//! owners are called directly, the others keep a `todo_stub!` with an explicit fallback and a
//! one-line contract. Owners: combat (hunk 13) = 0x1240fa, 0x1228ba, 0x1231be, 0x1230ce,
//! 0x12395a, 0x1221da, 0x121d78, 0x122fbe; magic (hunks 2, 8, 11) = 0x1165a4, 0x11a470,
//! 0x116d0e, 0x119200, 0x11a586, 0x119afc; objects commands (hunk 15) = 0x12948a, 0x129544; ui =
//! 0x105016, 0x1118c6, 0x11121e, 0x111be0, 0x13410a, 0x132338, 0x1331b4, 0x107cba, the hand
//! and weight words; sound = 0x110878, 0x110912, 0x100fa4; graphics = 0x1623be; the ending =
//! 0x133d1e, 0x13513a.

use crate::engine::Io;
use crate::objects::{NONE, Slot};
use crate::state::State;
use crate::todo_stub;

// ---- combat -----------------------------------------------------------------------------------

/// 0x1240fa(obj): the actor an object stands for (the rider of a horse 0x1a9, the head of a
/// multi-tile part). Fallback: `obj` itself (right for every single-tile object).
pub(crate) fn actor_of(_s: &mut State, obj: Slot) -> Slot {
    if obj < 0 {
        return NONE;
    }
    todo_stub!("0x1240fa actor an object stands for (usecmd seam)", obj)
}

/// 0x1231be(actor, damage): deal damage. Fallback: none dealt.
pub(crate) fn damage(_s: &mut State, _actor: Slot, _amount: i16) {
    todo_stub!("0x1231be damage an actor (usecmd seam)", ())
}

/// 0x1230ce(actor): show an actor hit / poisoned (flash). Fallback: nothing shown.
pub(crate) fn hit_flash(_s: &mut State, _actor: Slot) {
    todo_stub!("0x1230ce actor hit flash (usecmd seam)", ())
}

/// 0x12395a: after damage, check deaths. Fallback: none.
pub(crate) fn check_deaths(_s: &mut State) {
    todo_stub!("0x12395a check deaths after damage (usecmd seam)", ())
}

/// 0x1221da(actor): put an actor to sleep / refresh its status frame. Fallback: none.
pub(crate) fn status_frame(_s: &mut State, _actor: Slot) {
    todo_stub!("0x1221da actor status frame (usecmd seam)", ())
}

/// 0x1228ba(from, range, &x, &y, tile): a missile flies from `from` towards (x, y); returns
/// non-zero when something stopped it, and where it stopped (A4+0x185e / +0x1860). Fallback:
/// (0, x, y), it arrives unstopped.
pub(crate) fn missile(
    _s: &mut State,
    _from: Slot,
    _range: i16,
    x: i16,
    y: i16,
    _tile: u16,
) -> (i16, i16, i16) {
    todo_stub!("0x1228ba missile flight (usecmd seam)", (0, x, y))
}

/// 0x121d78: leave combat mode. Fallback: none.
pub(crate) fn end_combat(_s: &mut State) {
    todo_stub!("0x121d78 break off combat (usecmd seam)", ())
}

/// 0x122fbe: party members' combat positions after boarding. Fallback: none.
pub(crate) fn reset_formation(_s: &mut State) {
    todo_stub!("0x122fbe reset formation (usecmd seam)", ())
}

// ---- magic ------------------------------------------------------------------------------------

/// 0x1165a4(type): one of the 4 door types at A4-0x6b54 (magic's copy of the list). Fallback:
/// the same lookup in the user's `game`.
pub(crate) fn is_door(io: &Io, ty: u16) -> bool {
    todo_stub!(
        "0x1165a4 door type list (usecmd seam)",
        super::defs::a4_words(io, -0x6b54, 4).contains(&ty)
    )
}

/// 0x11a470: the sextant line (latitude, longitude). Fallback: nothing printed.
pub(crate) async fn sextant(_state: &std::cell::RefCell<State>, _io: &Io) {
    todo_stub!("0x11a470 sextant coordinates (usecmd seam)", ())
}

/// 0x116d0e(n): the heal effect a potion casts (n = 6). Fallback: none healed.
pub(crate) fn heal(_s: &mut State, _n: i16) {
    todo_stub!("0x116d0e heal an actor (usecmd seam)", ())
}

/// 0x119200: x-ray vision. Fallback: none.
pub(crate) fn xray(_s: &mut State) {
    todo_stub!("0x119200 x-ray vision (usecmd seam)", ())
}

/// 0x11a586(spell): cast a spell from an object (wand). Fallback: no spell.
pub(crate) async fn cast_from_object(_state: &std::cell::RefCell<State>, _io: &Io, _spell: u8) {
    todo_stub!("0x11a586 spell target and effect (usecmd seam)", ())
}

/// 0x119afc: the gate-travel spell to a latitude / longitude. Fallback: no travel.
pub(crate) fn gate_travel(_s: &mut State) {
    todo_stub!("0x119afc gate travel (usecmd seam)", ())
}

// ---- objects commands (hunk 15) ---------------------------------------------------------------

/// 0x12948a(obj): the first non-actor object at `obj`'s tile that is not `obj` and whose tile
/// has no flags3 bit 4, -1. Fallback: -1 ("nothing").
pub(crate) fn object_under(_s: &mut State, _obj: Slot) -> Slot {
    todo_stub!("0x12948a object beneath (usecmd seam)", NONE)
}

/// 0x129544(obj, x, y): search: secret doors, and the contents of `obj` spill to its tile.
/// Fallback: nothing found, nothing moved.
pub(crate) async fn search(_state: &std::cell::RefCell<State>, _io: &Io, _obj: Slot, _x: i16, _y: i16) {
    todo_stub!("0x129544 search here (usecmd seam)", ())
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

/// A4+0x1364 / A4+0x136a: the objects in the panel member's two hands (0x11121e keeps them).
/// Fallback: both hands free.
pub(crate) fn hands(_s: &State) -> [Slot; 2] {
    todo_stub!("0x11121e hand slots A4+0x1364/0x136a (usecmd seam)", [0, 0])
}

/// Writes hand `i` (A4+0x1364 + 6 i). Fallback: not kept.
pub(crate) fn set_hand(_s: &mut State, _i: usize, _obj: Slot) {
    todo_stub!("0x11121e set a hand slot (usecmd seam)", ())
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

/// 0x13410a: the gem view (the whole map, waits for a key). Fallback: nothing shown.
pub(crate) async fn peer_map(_state: &std::cell::RefCell<State>, _io: &Io) {
    todo_stub!("0x13410a peer at the map (usecmd seam)", ())
}

/// 0x132338(actor): flash the actor in the view. Fallback: none.
pub(crate) fn flash_actor(_s: &mut State, _actor: Slot) {
    todo_stub!("0x132338 flash an actor (usecmd seam)", ())
}

/// 0x1331b4(tile, x, y): explosion animation; the actors and objects it hit, -1-terminated.
/// Fallback: nothing hit.
pub(crate) fn blast(_s: &mut State, _tile: u16, _x: i16, _y: i16) -> Vec<Slot> {
    todo_stub!("0x1331b4 explosion animation (usecmd seam)", Vec::new())
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

/// 0x100fa4: MED: play the instrument note set at A4-0x35cc + 0x3d2. Fallback: no sound.
pub(crate) fn play_note(_s: &mut State) {
    todo_stub!("0x100fa4 play an instrument note (usecmd seam)", ())
}

/// 0x1623be: the copper effect of the machine. Fallback: none.
pub(crate) fn screen_effect(_io: &Io) {
    todo_stub!("0x1623be screen effect (usecmd seam)", ())
}

/// 0x133d1e then 0x13513a: the ending sequence after the Codex vanished. Fallback: the game
/// goes on.
pub(crate) fn ending(_s: &mut State) {
    todo_stub!("0x13513a the ending (usecmd seam)", ())
}

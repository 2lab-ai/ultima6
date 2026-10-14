//! Calls from the object commands into routines other modules own.
//!
//! Ported owners are called directly (integration M2: combat 0x121e88, 0x121b08, 0x1231be,
//! 0x12395a, 0x1230ce, 0x1331b4, 0x1240fa, 0x122f3e, 0x12267a, 0x123aec, 0x101eb0, 0x1228ba;
//! actors 0x11b1c0; magic 0x116526, 0x115da4, 0x116028, 0x116218 and the book globals;
//! inventory 0x105016, 0x1110c8, 0x1110f6, 0x1111ca, 0x11121e, 0x111be0, 0x1118c6, 0x1122dc,
//! 0x11236c, 0x111fe0; sound 0x110878, 0x110912).

use crate::engine::Io;
use crate::objects::Slot;
use crate::state::State;

/// Combat 0x121e88 (put a new monster on a free tile near where it stands, out of the view when
/// `out_of_view`): [`crate::combat::mode::scatter`].
pub(crate) fn scatter(s: &mut State, m: Slot, out_of_view: bool) -> bool {
    crate::combat::mode::scatter(s, m, out_of_view)
}

/// Inventory 0x105016 (direction 0..7 of the view pixel (x, y) from the pointer origin
/// A4-0x7a26 / -0x7a25, -1 at it): [`crate::inventory::direction_to`].
pub(crate) fn direction_of(s: &State, x: i16, y: i16) -> i16 {
    crate::inventory::direction_to(s.inventory.pointer_origin, x, y)
}

/// Combat 0x121b08 (distance from actor `a` to (x, y): the larger of |dx|, |dy|):
/// [`crate::combat::leaves::distance`].
pub(crate) fn distance(s: &State, a: u16, x: u16, y: u16) -> i16 {
    crate::combat::leaves::distance(s, a as i16, x as i16, y as i16)
}

/// Inventory 0x1118c6 (redraw the inventory list after a carried object changed). Port: the
/// callers here are synchronous and 0x1118c6 prints its weight lines, so the panel is marked
/// (A4-0x7afe) and the command loop's 0x103f64 redraws it before the next key.
pub(crate) fn redraw_inventory(s: &mut State) {
    s.ui.panel.dirty = true;
}

/// Combat 0x1231be (hurt actor `a` by `damage`: hit points, death, splitting slimes):
/// [`crate::combat::damage::hurt`].
pub(crate) fn hurt(s: &mut State, a: Slot, damage: i16) {
    crate::combat::damage::hurt(s, a, damage);
}

/// Combat 0x12395a (print actor `a`'s wound level): [`crate::combat::damage::report_wounds`].
pub(crate) fn wound_text(s: &mut State, a: Slot) {
    crate::combat::damage::report_wounds(s, a);
}

/// Combat 0x1230ce (flash actor `a` hit): [`crate::combat::damage::flash`].
pub(crate) fn flash(s: &mut State, a: Slot) {
    crate::combat::damage::flash(s, a);
}

/// Ui (hunk 18) 0x1331b4 (explosion of `tile` around (x, y): animated, the slots it reached):
/// [`crate::combat::present::blast`], without its -1 terminator.
pub(crate) fn blast(s: &mut State, x: u16, y: u16, tile: u16) -> Vec<Slot> {
    let mut hit = crate::combat::present::blast(s, x as i16, y as i16, tile);
    hit.retain(|&a| a >= 0);
    hit
}

/// Magic 0x116526 (the object of `kind` and `quality` (0 = any) that `owner` carries at any
/// depth, -1): [`crate::magic::carried_object`].
pub(crate) fn carried(s: &mut State, owner: Slot, kind: u16, quality: i16) -> Slot {
    crate::magic::carried_object(&s.objects, owner, kind as i16, quality)
}

/// Combat 0x1240fa (the actor a horse or monster part `obj` belongs to; `obj` itself for an
/// actor): [`crate::combat::leaves::owner`].
pub(crate) fn rider(s: &mut State, obj: Slot) -> Slot {
    crate::combat::leaves::owner(s, obj)
}

/// Combat 0x122f3e (damage `obj` does: a monster's own byte, else its weapon's, -1 when not a
/// weapon): [`crate::combat::leaves::damage_of`].
pub(crate) fn damage(s: &State, obj: Slot) -> i16 {
    crate::combat::leaves::damage_of(s, obj)
}

/// Combat 0x12267a (the points of damage armour `obj` absorbs, 0):
/// [`crate::combat::leaves::armour_of`].
pub(crate) fn armour(s: &State, obj: Slot) -> i16 {
    crate::combat::leaves::armour_of(s, obj)
}

/// Inventory 0x1110f6 (weight of `obj` in tenths of a stone, its count included):
/// [`crate::inventory::object_weight`].
pub(crate) fn weight(s: &State, obj: Slot) -> i16 {
    crate::inventory::object_weight(s, obj)
}

/// Inventory 0x1111ca (weight of everything inside `obj`): [`crate::inventory::contents_weight`].
pub(crate) fn contents_weight(s: &State, obj: Slot) -> i16 {
    crate::inventory::contents_weight(s, obj)
}

/// Magic's A4+0x17f4 / +0x17f6 (the readied books) set to (`book`, 0): Look opens that book.
pub(crate) fn open_spellbook(s: &mut State, book: Slot) {
    s.magic.books = [book, 0];
}

/// Magic 0x115da4 then 0x116028 (the books' spells and page, the book drawn in the side
/// panel): [`crate::magic::load_book`], [`crate::magic::draw_book`]; the page itself is
/// 0x116218 ([`crate::magic::draw_book_page`], awaited by the caller).
pub(crate) fn show_spellbook(s: &mut State, io: &crate::engine::Io) {
    crate::magic::load_book(s);
    crate::magic::draw_book(s, io);
}

/// Magic's A4+0x17f8 (byte per spellbook row: the spell shown there; read past the 8 rows as
/// the bytes that follow, here 0).
pub(crate) fn spell_in_slot(s: &State, row: Slot) -> u8 {
    usize::try_from(row)
        .ok()
        .and_then(|r| s.magic.page.get(r).copied())
        .unwrap_or(0)
}

/// Sound 0x110878(0) (music off): [`crate::sound::play_song`].
pub(crate) fn music_off(s: &mut State) {
    crate::sound::play_song(s, 0);
}

/// Sound 0x110912 (pick and play the area song): [`crate::sound::play_area_song`].
pub(crate) fn music_on(s: &mut State) {
    crate::sound::play_area_song(s);
}

/// Inventory 0x1110c8 (`kind` is one of the 9 types at A4-0x71ca, weighed in tenths):
/// [`crate::inventory::counted_type`].
pub(crate) fn weighed_in_tenths(s: &State, kind: u16) -> bool {
    crate::inventory::counted_type(s, kind as i16)
}

/// Inventory 0x11121e (what `actor` holds: A4+0x1342 readied weight, A4+0x1344 the rest,
/// A4+0x1340 their count): [`crate::inventory::weights`].
pub(crate) fn sum_inventory(s: &mut State, actor: Slot) {
    crate::inventory::weights(s, actor);
}

/// Inventory 0x111be0 (lay out `actor`'s inventory panel): its count and cells now
/// ([`crate::inventory::recount`]); the drawing (0x1118c6) waits for the loop's panel redraw
/// (see [`redraw_inventory`]).
pub(crate) fn inventory_contents(s: &mut State, actor: Slot) {
    crate::inventory::recount(s, actor);
    s.ui.panel.dirty = true;
}

/// Combat 0x123aec (the party wronged a neutral: karma −5, the guards and townsfolk nearby turn
/// on the party): [`crate::combat::damage::rouse_neutrals`].
pub(crate) fn turn_hostile(s: &mut State) {
    crate::combat::damage::rouse_neutrals(s);
}

/// Main (hunk 1) 0x101eb0 (lower the karma A4-0x7b7c by `n`, to 0):
/// [`crate::combat::leaves::lose_karma`].
pub(crate) fn lose_karma(s: &mut State, n: u8) {
    crate::combat::leaves::lose_karma(s, i16::from(n));
}

/// Inventory 0x1122dc (a readied object that cannot simply be put down: the amulet 0x4c with
/// its message gives 1, a lit torch burns out and gives 2, else 0):
/// [`crate::inventory::unready_check`].
pub(crate) async fn cannot_drop(state: &std::cell::RefCell<State>, io: &Io, obj: Slot) -> i16 {
    crate::inventory::unready_check(state, io, obj).await
}

/// Inventory 0x11236c (putting down readied `obj` held by `owner` ends its magic; 3 when it
/// did, else 0): [`crate::inventory::release_effect`]. Its flash is recorded; Drop plays it
/// with the flight before the panel is redrawn ([`super::drop_object`]).
pub(crate) fn end_worn_magic(s: &mut State, io: &Io, obj: Slot, owner: Slot) -> i16 {
    crate::inventory::release_effect(s, io, obj, owner)
}

/// Inventory 0x111fe0 (the equipment slot of A4+0x1360 that holds `obj`, -1):
/// [`crate::inventory::equipped_index`].
pub(crate) fn equipment_slot(s: &State, obj: Slot) -> i16 {
    crate::inventory::equipped_index(s, obj)
}

/// Combat 0x1228ba (the path from `actor` to (x, y) within `range`, `tile` flying along it, ui
/// 0x132706; (x, y) end where it stops; false when it did not get there):
/// [`crate::combat::attack::shoot`].
pub(crate) fn throw_path(
    s: &mut State,
    actor: Slot,
    range: i16,
    x: &mut i16,
    y: &mut i16,
    tile: u16,
) -> bool {
    crate::combat::attack::shoot(s, actor, x, y, range, tile) != 0
}

/// Actors 0x11b1c0 (may `slot` stand at (x, y)): [`crate::actors::walk::can_move_to`], for an
/// object slot too (Drop and Move ask it for the object), as the original calls it.
pub(crate) fn may_stand(s: &mut State, slot: Slot, x: u16, y: u16) -> bool {
    crate::actors::walk::can_move_to(s, slot as u16, x, y)
}

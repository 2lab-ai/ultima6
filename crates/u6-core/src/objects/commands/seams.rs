//! Calls from the object commands into routines other modules own.
//!
//! Ported owners are called directly (integration M2: combat 0x121e88, 0x121b08, 0x1231be,
//! 0x12395a, 0x1230ce, 0x1331b4, 0x1240fa, 0x122f3e, 0x12267a, 0x123aec, 0x101eb0, 0x1228ba;
//! actors 0x11b1c0). The others name their owner and the original, keep the `todo_stub!` hit
//! count, and state their fallback; where an accepted scenario depends on the exact result the
//! fallback is the original's behaviour written out here from its code.

use crate::objects::Slot;
use crate::state::State;
use crate::todo_stub;

/// Combat 0x121e88 (put a new monster on a free tile near where it stands, out of the view when
/// `out_of_view`): [`crate::combat::mode::scatter`].
pub(crate) fn scatter(s: &mut State, m: Slot, out_of_view: bool) -> bool {
    crate::combat::mode::scatter(s, m, out_of_view)
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
    let (rx, ry) = (
        i16::from(s.objcmds.pointer_ref.0),
        i16::from(s.objcmds.pointer_ref.1),
    );
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

/// Combat 0x121b08 (distance from actor `a` to (x, y): the larger of |dx|, |dy|):
/// [`crate::combat::leaves::distance`].
pub(crate) fn distance(s: &State, a: u16, x: u16, y: u16) -> i16 {
    crate::combat::leaves::distance(s, a as i16, x as i16, y as i16)
}

/// Owner: ui (hunk 8). Original 0x1118c6 (redraw the inventory panel after a carried object
/// changed). Fallback: the panel is marked for the next 0x103f64 redraw instead.
pub(crate) fn redraw_inventory(s: &mut State) {
    todo_stub!("0x1118c6 redraw the inventory panel", ());
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

/// Original: A4-0x71ca (9 words: types whose weight 0x1110f6 reads in tenths)
const TENTHS_TYPES: i32 = -0x71ca;

/// Owner: ui / inventory (hunk 8). Original 0x1110f6 (weight of `obj` in tenths of a stone:
/// the type's weight byte × its count (gold-like stacks their word, quantity stacks their
/// quantity byte, a lit (frame 1) torch 1), the 9 types of A4-0x71ca a tenth of that; 0xff
/// reads 10000). The quotient's remainder sits in the high word, which no caller here uses.
///
/// Contract: pure. Fallback = that computation (Get and Look weigh with it).
pub(crate) fn weight(s: &State, obj: Slot) -> i16 {
    todo_stub!("0x1110f6 weight of an object", ());
    let o = &s.objects;
    let kind = o.kind(obj);
    let count = match crate::objects::stack_kind(o, kind) {
        4 => o.qq_of(obj),
        2 if kind == 0x5a && o.tf(obj) >> 10 == 1 => 1,
        2 => o.qq_of(obj) & 0xff,
        _ => 1,
    };
    let unit = u16::from(
        s.tables
            .tileflags
            .weight
            .get(usize::from(kind))
            .copied()
            .unwrap_or(0),
    );
    let tenths = (0..9).any(|i| s.objcmds.data.a4_word(TENTHS_TYPES + 2 * i) == kind);
    let w = if tenths {
        ((unit.wrapping_mul(count)) as i16) / 10
    } else {
        unit.wrapping_mul(count) as i16
    };
    if w == 0xff { 10000 } else { w }
}

/// Owner: ui / inventory (hunk 8). Original 0x1111ca (weight of everything directly after
/// `obj` in the list while it is inside something: the contents of a container; stops at the
/// first entry 0x10c928 rejects).
///
/// Contract: pure. Fallback = that computation with [`weight`].
pub(crate) fn contents_weight(s: &State, obj: Slot) -> i16 {
    todo_stub!("0x1111ca weight of a container's contents", ());
    let o = &s.objects;
    let mut total: i16 = 0;
    let mut c = o.next_of(obj);
    while c >= 0 && o.place(c) != 0 {
        if !o.contained_in(c, obj) {
            return total;
        }
        total = total.wrapping_add(weight(s, c));
        c = o.next_of(c);
    }
    total
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

/// Owner: sound. Original 0x110878(0) (music off). Fallback: nothing (no music).
pub(crate) fn music_off() {
    todo_stub!("0x110878 play song 0 (music off)", ())
}

/// Owner: sound. Original 0x110912 (pick and play the area song). Fallback: nothing.
pub(crate) fn music_on() {
    todo_stub!("0x110912 pick and play the area song", ())
}

/// Owner: ui / inventory (hunk 8). Original 0x1110c8 (`kind` is one of the 9 types at
/// A4-0x71ca, weighed in tenths). Contract: pure. Fallback = that lookup.
pub(crate) fn weighed_in_tenths(s: &State, kind: u16) -> bool {
    todo_stub!("0x1110c8 type weighed in tenths", ());
    (0..9).any(|i| s.objcmds.data.a4_word(TENTHS_TYPES + 2 * i) == kind)
}

/// Owner: ui / inventory (hunk 8). Original 0x11121e (sum what `actor` holds: A4+0x1342 the
/// weight bytes of its readied objects (cleared first for an actor), A4+0x1344 the weight of
/// the rest with their contents, A4+0x1340 how many of those).
///
/// Contract: writes the three globals ([`super::ObjCommands::inventory_weights`], the panel's
/// count). Fallback = that computation (Get's "too heavy" test reads it).
pub(crate) fn sum_inventory(s: &mut State, actor: Slot) {
    todo_stub!("0x11121e sum an actor's inventory weights", ());
    s.ui.panel.inventory_count = 0;
    let mut readied = s.objcmds.inventory_weights.0;
    let mut carried = 0i16;
    if actor < 0x100 {
        readied = 0;
    }
    let mut c = crate::objects::first_child(&mut s.objects, actor);
    while c >= 0 {
        if s.objects.place(c) == crate::objects::READIED {
            let k = usize::from(s.objects.kind(c));
            let w = s.tables.tileflags.weight.get(k).copied().unwrap_or(0);
            readied = readied.wrapping_add(i16::from(w));
        } else {
            let w = weight(s, c).wrapping_add(contents_weight(s, c));
            carried = carried.wrapping_add(w);
            s.ui.panel.inventory_count = s.ui.panel.inventory_count.wrapping_add(1);
        }
        c = crate::objects::next_child(&mut s.objects);
    }
    s.objcmds.inventory_weights = (readied, carried);
}

/// Owner: ui / inventory (hunk 8). Original 0x111be0 (lay out `actor`'s inventory panel).
/// Fallback: the panel is marked for the next redraw.
pub(crate) fn inventory_contents(s: &mut State, _actor: Slot) {
    todo_stub!("0x111be0 inventory panel contents", ());
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

/// Owner: ui / inventory (hunk 8). Original 0x1122dc (a readied object that cannot simply be
/// put down: the amulet 0x4c prints its message (A4-0x6c3c) and gives 1; a lit torch goes out
/// of the world with its message (A4-0x756f) and gives 2; else 0).
///
/// Contract: as described (the text is queued). Fallback = that behaviour.
pub(crate) fn cannot_drop(s: &mut State, obj: Slot) -> i16 {
    todo_stub!("0x1122dc readied object that cannot be dropped", ());
    if s.objects.place(obj) != crate::objects::READIED {
        return 0;
    }
    match (s.objects.kind(obj), s.objects.tf(obj) >> 10) {
        (0x4c, _) => {
            let msg = s.objcmds.data.pointed(super::GameData::a4(-0x6c3c));
            super::text::queue_at(s, 0x0011_0c62, vec![crate::main_loop::PrintArg::Str(msg)]);
            1
        }
        (0x5a, 1) => {
            crate::objects::moves::remove(s, obj);
            super::text::queue_at(s, 0x0011_0c68, Vec::new());
            let fmt = super::text::a4_string(s, -0x756f);
            super::text::queue(s, fmt, Vec::new());
            2
        }
        _ => 0,
    }
}

/// Owner: ui / inventory (hunk 8). Original 0x11236c (putting down readied `obj` held by
/// `owner` ends its magic: a ring 0x102 makes the owner visible (status bit 1 cleared), 0x51
/// clears A4-0x7b65; both redraw (0x132338) and give 3; else 0).
///
/// Contract: as described. Fallback = that behaviour, the redraw left to the next frame.
pub(crate) fn end_worn_magic(s: &mut State, obj: Slot, owner: Slot) -> i16 {
    todo_stub!("0x11236c readied object's magic ends", ());
    if s.objects.place(obj) != crate::objects::READIED {
        return 0;
    }
    match s.objects.kind(obj) {
        0x102 => {
            if let Some(b) = s.objects.obj_status.get_mut(owner as usize) {
                *b &= 0xfd;
            }
            3
        }
        0x51 => {
            if let Some(b) = s.actors.objlist_tail.get_mut(0x1f) {
                *b = 0;
            }
            3
        }
        _ => 0,
    }
}

/// Owner: ui / inventory (hunk 8). Original 0x111fe0 (the equipment slot of A4+0x1360 that
/// holds `obj`, -1). Contract: pure. Fallback = that search.
pub(crate) fn equipment_slot(s: &State, obj: Slot) -> i16 {
    todo_stub!("0x111fe0 equipment slot of an object", ());
    s.objcmds
        .equipment
        .iter()
        .position(|&o| o == obj)
        .map_or(-1, |i| i as i16)
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

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

/// Original: A4-0x7e7e (bytes by monster index: a monster's own damage), A4-0x6800 / -0x67b6
/// (37 weapon types and their damage), A4-0x6884 / -0x6850 (26 armour types and what they
/// absorb).
const MONSTER_DAMAGE: i32 = -0x7e7e;
const WEAPON_TYPES: i32 = -0x6800;
const WEAPON_DAMAGE: i32 = -0x67b6;
const ARMOUR_TYPES: i32 = -0x6884;
const ARMOUR_VALUE: i32 = -0x6850;

/// Owner: combat (hunk 13). Original 0x122f3e (damage `obj` does: an actor of a monster type
/// its monster byte, else the weapon table's byte for its type, -1 when not a weapon).
///
/// Contract: pure table lookup. Fallback = that lookup (Look prints it).
pub(crate) fn damage(s: &State, obj: Slot) -> i16 {
    todo_stub!("0x122f3e damage of a weapon or monster", ());
    let d = &s.objcmds.data;
    let kind = s.objects.kind(obj);
    let index = if obj < 0x100 {
        crate::objects::monster_index(&s.objects, kind)
    } else {
        -1
    };
    if index >= 0 {
        return i16::from(d.a4_byte(MONSTER_DAMAGE + i32::from(index)));
    }
    (0..0x25)
        .find(|&i| d.a4_word(WEAPON_TYPES + 2 * i) == kind)
        .map_or(-1, |i| i16::from(d.a4_byte(WEAPON_DAMAGE + i)))
}

/// Owner: combat (hunk 13). Original 0x12267a (the points of damage armour `obj` absorbs, 0).
///
/// Contract: pure table lookup. Fallback = that lookup (Look prints it).
pub(crate) fn armour(s: &State, obj: Slot) -> i16 {
    todo_stub!("0x12267a armour value of an object", ());
    let d = &s.objcmds.data;
    let kind = s.objects.kind(obj);
    (0..0x1a)
        .find(|&i| d.a4_word(ARMOUR_TYPES + 2 * i) == kind)
        .map_or(0, |i| i16::from(d.a4_byte(ARMOUR_VALUE + i)))
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

/// Owner: magic. A4+0x17f4 / +0x17f6 (the spellbook object Look opens and its page) set to
/// (`book`, 0). Fallback: not recorded (magic keeps them).
pub(crate) fn open_spellbook(_s: &mut State, _book: Slot) {
    todo_stub!("0x115da4 spellbook globals A4+0x17f4", ())
}

/// Owner: magic (hunk 11). Originals 0x115da4, 0x116028, 0x116218 (lay out and draw the open
/// spellbook's page). Fallback: nothing drawn; the key read that follows still runs.
pub(crate) fn show_spellbook(_s: &mut State) {
    todo_stub!("0x116218 draw the spellbook page", ())
}

/// Owner: magic. A4+0x17f8 (byte per spellbook row: the spell shown there). Fallback 0.
pub(crate) fn spell_in_slot(_s: &State, _row: Slot) -> u8 {
    todo_stub!("0x116028 spell on a spellbook row", 0)
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

/// Owner: combat (hunk 13). Original 0x123aec (actor `a` turns on the party). Fallback:
/// nothing.
pub(crate) fn turn_hostile(_s: &mut State, _a: Slot) {
    todo_stub!("0x123aec turn hostile", ())
}

/// Owner: main (hunk 1). Original 0x101eb0 (lower the karma A4-0x7b7c by `n`, to 0).
/// Contract: pure arithmetic. Fallback = that.
pub(crate) fn lose_karma(s: &mut State, n: u8) {
    todo_stub!("0x101eb0 lower the karma", ());
    let k = &mut s.actors.karma;
    *k = (*k).saturating_sub(n);
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

/// Owner: ui (hunk 18) via combat 0x1228ba. Original 0x1228ba → 0x132706 (the path from
/// `actor` to (x, y) within `range`, with `tile` the thrown object flying along it; (x, y)
/// end where it stops; false when nothing gets there). Fallback: true, (x, y) unchanged.
pub(crate) fn throw_path(
    _s: &mut State,
    _actor: Slot,
    _range: i16,
    _x: &mut i16,
    _y: &mut i16,
    _tile: u16,
) -> bool {
    todo_stub!("0x1228ba path and flight of a thrown object", true)
}

/// Owner: actors. Original 0x11b1c0 (may `slot` stand at (x, y)). For an actor this is
/// [`crate::actors::walk::can_move_to`]. Drop and Move ask it for objects too, where the
/// original reads the actor tables past their 256 entries (A4+0x3d0c + slot lands in later
/// tables) and the port's tables end: for a slot ≥ 0x100 the fallback is the tile test alone,
/// a passable map tile with nothing impassable on it (0x11b9ae).
pub(crate) fn may_stand(s: &mut State, slot: Slot, x: u16, y: u16) -> bool {
    if (0..crate::objects::ACTOR_SLOTS as Slot).contains(&slot) {
        return crate::actors::walk::can_move_to(s, slot as u16, x, y);
    }
    todo_stub!("0x11b1c0 for an object slot", ());
    !crate::actors::walk::blocked_at(s, x, y)
}

/// Owner: ui (hunk 18). Original 0x13410a (clear and rebuild the 40 × 40 view-flag and light
/// grids around the leader). Fallback: the view flags are rebuilt by 0x109e74 instead.
pub(crate) fn rebuild_view_light(s: &mut State) {
    todo_stub!("0x13410a rebuild the view and light grids", ());
    crate::map::view::update_view(s);
}

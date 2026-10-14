//! Object names (look.lzd), stack kinds and quantities.
//!
//! The strings come back raw: plural markers (`\` plural-only letters, `/` singular-only) are
//! resolved by the message printer (ui 0x108376, from the count a `#` directive names), not here.

use super::iter::tile_of;
use super::{ACTOR_SLOTS, Objects, Slot};
use crate::state::{State, Tables};

/// Gold: 0x10cef2 kind 4 (the quantity is the whole quality / quantity word).
pub const GOLD: u16 = 0x58;

/// Original: 0x10cef2 (objects: how a type is counted: 4 gold, 2 by quantity byte, 0 single)
///
/// Compares the whole word it is given with 0x58 and with the 21 stackable types, so callers
/// that pass a type + frame word only match frame 0.
pub fn stack_kind(o: &Objects, word: u16) -> u16 {
    if word == GOLD {
        4
    } else if o.defs.stackable.contains(&word) {
        2
    } else {
        0
    }
}

/// Original: 0x10cf2c (objects: how many items `slot` stands for)
///
/// Gold: the quality / quantity word; a quantity stack: the quantity byte; else 1. The kind is
/// taken from the whole type + frame word.
pub fn quantity(o: &Objects, slot: Slot) -> u16 {
    match stack_kind(o, o.tf(slot)) {
        4 => o.qq_of(slot),
        2 => o.qq_of(slot) & 0xff,
        _ => 1,
    }
}

/// Original: 0x10ee76 (objects: look.lzd string of a tile, "nothing" when empty)
///
/// The string of the first entry whose tile is ≥ `tile`. Past the last entry the walk reads on
/// through the terminator (tile 0x800) and the bytes after it, as the original does in its
/// look buffer; past the buffer the string is empty.
pub fn look_text(s: &State, tile: u16) -> String {
    let text = raw_look(&s.tables, tile);
    if text.is_empty() {
        s.objects.defs.nothing_lower.clone()
    } else {
        text
    }
}

fn raw_look(t: &Tables, tile: u16) -> String {
    let tile = tile as i16;
    if let Some(e) = t.look.entries.iter().find(|e| e.tile as i16 >= tile) {
        return e.text.clone();
    }
    // The terminator word and the decoded bytes after it, read as further entries.
    let b = &t.look.trailer;
    let mut at = 0;
    while at + 2 <= b.len() {
        let entry = i16::from_le_bytes([b[at], b[at + 1]]);
        let end = b[at + 2..]
            .iter()
            .position(|&c| c == 0)
            .map_or(b.len(), |n| at + 2 + n);
        if entry >= tile {
            return b[at + 2..end].iter().map(|&c| char::from(c)).collect();
        }
        at = end + 1;
    }
    String::new()
}

/// Original: 0x10eef8 (objects: display name of an object or actor)
///
/// -1 → "Nothing". A party member (actor status bit 7) gets its party name; an actor with
/// status bits 0x14 is named after its base type (the type it shows when not polymorphed or
/// asleep); everything else after the look string of its tile. For slots ≥ 0x100 the original
/// reads the actor-status test past its 256-byte table; both branches it can take there name
/// the tile, as here.
pub fn display_name(s: &State, slot: Slot) -> String {
    if slot < 0 {
        return s.objects.defs.nothing.clone();
    }
    let o = &s.objects;
    let t = &s.tables;
    let actor = slot < ACTOR_SLOTS as Slot;
    let status = if actor {
        s.actors
            .attrs
            .status
            .get(slot as usize)
            .copied()
            .unwrap_or(0)
    } else {
        0
    };
    if status & 0x80 != 0 {
        let name = look_text(s, tile_of(t, o.tf(slot)));
        let party = &s.actors.party;
        for i in 0..usize::from(party.size) {
            if party.members.get(i).map(|&m| Slot::from(m)) == Some(slot) {
                return party.names.get(i).cloned().unwrap_or_default();
            }
        }
        return name;
    }
    if actor && status & 0x14 != 0 {
        let base = s.actors.attrs.base_type_frame[slot as usize];
        let start = t.basetile.base.get(usize::from(base & 0x3ff)).copied();
        let tile = start
            .unwrap_or(0)
            .wrapping_add(((base as i16) >> 10) as u16);
        return look_text(s, tile);
    }
    look_text(s, tile_of(t, o.tf(slot)))
}

/// Original: 0x12904c (objects: article of a tile from flags3 bits 6-7) [partial]
///
/// The original prints "a ", "an " or "the " (none for 0); the port returns the text so the
/// Look path can print it.
pub fn article(s: &State, tile: u16) -> &str {
    let f3 = s.tables.tileflags.flags3.get(usize::from(tile)).copied();
    match f3.unwrap_or(0) & 0xc0 {
        0x40 => &s.objects.defs.articles[0],
        0x80 => &s.objects.defs.articles[1],
        0xc0 => &s.objects.defs.articles[2],
        _ => "",
    }
}

/// Original: 0x129382 (objects: count or article in front of a name, and the count) [partial]
///
/// Gold prints its word and a quantity stack its quantity byte through the count format (a
/// lit (status 0x80) quantity object takes the article path); anything else prints its tile's
/// article and counts 1. Returns the prefix with the count filled in, and the count.
pub fn name_prefix(s: &State, slot: Slot) -> (String, u16) {
    let o = &s.objects;
    let fill = |n: u16| o.defs.count_format.replace("%d", &n.to_string());
    match stack_kind(o, o.kind(slot)) {
        4 => (fill(o.qq_of(slot)), o.qq_of(slot)),
        2 if o.status(slot) & 0x80 == 0 => (fill(o.qq_of(slot) & 0xff), o.qq_of(slot) & 0xff),
        _ => {
            let tile = tile_of(&s.tables, o.tf(slot));
            (article(s, tile).to_string(), 1)
        }
    }
}

/// What the Look path prints for `slot`: the prefix of 0x129382 and the 0x10eef8 name with its
/// plural markers resolved for the count, as the printer 0x108376 does (`\\` letters only for a
/// count ≠ 1, `/` letters only for 1).
pub fn described_name(s: &State, slot: Slot) -> String {
    let (prefix, count) = name_prefix(s, slot);
    let name = display_name(s, slot);
    format!(
        "{prefix}{}",
        u6_formats::look::resolve_plural(&name, count != 1)
    )
}

/// Original: 0x10d2d8 (objects: how many of type `kind` `owner` holds, at any depth)
pub fn count_of_kind(o: &mut Objects, owner: Slot, kind: u16) -> i16 {
    let mut total: i16 = 0;
    let mut c = super::iter::first_child(o, owner);
    while c >= 0 && o.place(c) != 0 {
        if !o.contained_in(c, owner) {
            return total;
        }
        if o.kind(c) == kind {
            total = total.wrapping_add(quantity(o, c) as i16);
        }
        c = o.next_of(c);
    }
    total
}

//! Object names (look.lzd), stack kinds and quantities.
//!
//! The strings come back raw: plural markers (`\` plural-only letters, `/` singular-only) are
//! resolved by the message printer (ui 0x108376, from the count a `#` directive names), not here.
//!
//! Port (spec 08, slice K3): [`look_text`] and [`display_name`] are the English the game's logic
//! reads (a name compared, a panel's layout from its length). Where a name is read to be shown,
//! [`shown_look`] and [`shown_name`] give the locale pack's object name instead when it has one
//! ([`crate::locale::look_at`]), and [`korean_name`] says whether no article goes before it.

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

/// Original: 0x10ee76 (objects: look.lzd string of a tile, the nothing word when empty)
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

/// The index of the entry 0x10ee76 reads for `tile`: the first whose tile is ≥ it; `None` when
/// the walk goes on past the entries.
fn look_entry(t: &Tables, tile: u16) -> Option<usize> {
    let tile = tile as i16;
    t.look.entries.iter().position(|e| e.tile as i16 >= tile)
}

fn raw_look(t: &Tables, tile: u16) -> String {
    if let Some(i) = look_entry(t, tile) {
        return t.look.entries[i].text.clone();
    }
    let tile = tile as i16;
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

/// Port-only (spec 08, slice K3): a name as it is shown: the English, or the locale pack's object
/// name ([`shown_name`], [`shown_look`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    /// The English the game reads (a look string as decoded, a party name, the nothing word).
    English(String),
    /// The pack's text (UTF-8), its plural markers as its language reads them.
    Pack(Vec<u8>),
}

impl Shown {
    /// Its bytes as the name's printers took [`display_name`]'s (`String::into_bytes`).
    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            Shown::English(t) => t.into_bytes(),
            Shown::Pack(b) => b,
        }
    }

    /// Its bytes with the English one per character (0x10eef8's string as it is in look.lzd), as
    /// Use prints a name; the pack's text as it is.
    pub fn into_latin1(self) -> Vec<u8> {
        match self {
            Shown::English(t) => t.chars().map(|c| c as u32 as u8).collect(),
            Shown::Pack(b) => b,
        }
    }
}

/// What 0x10eef8 names a slot by: a word of its own (the nothing word, a party name) or the look
/// string of a tile.
enum Named {
    Word(String),
    Tile(u16),
}

/// Original: 0x10eef8 (objects: display name of an object or actor)
///
/// -1 → the capitalised nothing word. A party member (actor status bit 7) gets its party
/// name; an actor with
/// status bits 0x14 is named after its base type (the type it shows when not polymorphed or
/// asleep); everything else after the look string of its tile. For slots ≥ 0x100 the original
/// reads the actor-status test past its 256-byte table; both branches it can take there name
/// the tile, as here.
///
/// Port (spec 08, slice K3): always the English, for the game's logic; a name read to be shown is
/// [`shown_name`].
pub fn display_name(s: &State, slot: Slot) -> String {
    match named(s, slot) {
        Named::Word(w) => w,
        Named::Tile(tile) => look_text(s, tile),
    }
}

/// Port-only (spec 08, slice K3): [`display_name`] where it is read to be shown: the locale pack's
/// object name for the tile it names when the pack has it ([`shown_look`]); a party name and the
/// nothing word as they are.
pub fn shown_name(s: &State, slot: Slot) -> Shown {
    match named(s, slot) {
        Named::Word(w) => Shown::English(w),
        Named::Tile(tile) => shown_look(s, tile),
    }
}

/// Port-only (spec 08, slice K3): [`look_text`] where it is read to be shown: the locale pack's
/// text for the entry the tile reads when the pack has it (counted, [`crate::locale::look_at`]),
/// else the English. A read past the entries (the trailer) and the nothing word are no unit.
pub fn shown_look(s: &State, tile: u16) -> Shown {
    match pack_look(s, tile, true) {
        Some(text) => Shown::Pack(text),
        None => Shown::English(look_text(s, tile)),
    }
}

/// The pack's text for the look entry `tile` reads, counted when `count`.
fn pack_look(s: &State, tile: u16, count: bool) -> Option<Vec<u8>> {
    let i = look_entry(&s.tables, tile)?;
    let english = crate::locale::texts::latin1(&s.tables.look.entries[i].text);
    crate::locale::look_at(s, i, &english, count)
}

/// Port-only (spec 08, slice K3): whether the name [`shown_name`] gives `slot` is a Korean pack's
/// (then no article goes before it; spec 08 §1). Counts nothing.
pub fn korean_name(s: &State, slot: Slot) -> bool {
    match named(s, slot) {
        Named::Word(_) => false,
        Named::Tile(tile) => korean_look(s, tile),
    }
}

/// Port-only (spec 08, slice K3): whether the look text of `tile` shows from a Korean pack.
/// Counts nothing.
pub fn korean_look(s: &State, tile: u16) -> bool {
    look_entry(&s.tables, tile).is_some_and(|i| {
        let english = crate::locale::texts::latin1(&s.tables.look.entries[i].text);
        crate::locale::look_is_korean(s, i, &english)
    })
}

/// 0x10eef8's choice of what names `slot` (see [`display_name`]).
fn named(s: &State, slot: Slot) -> Named {
    if slot < 0 {
        return Named::Word(s.objects.defs.nothing.clone());
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
        let party = &s.actors.party;
        for i in 0..usize::from(party.size) {
            if party.members.get(i).map(|&m| Slot::from(m)) == Some(slot) {
                return Named::Word(party.names.get(i).cloned().unwrap_or_default());
            }
        }
        return Named::Tile(tile_of(t, o.tf(slot)));
    }
    if actor && status & 0x14 != 0 {
        let base = s.actors.attrs.base_type_frame[slot as usize];
        let start = t.basetile.base.get(usize::from(base & 0x3ff)).copied();
        let tile = start
            .unwrap_or(0)
            .wrapping_add(((base as i16) >> 10) as u16);
        return Named::Tile(tile);
    }
    Named::Tile(tile_of(t, o.tf(slot)))
}

/// The text 0x12904c prints for `tile` (flags3 bits 6-7: one of the three articles or none), as a
/// string for [`described_name`]. The port of 0x12904c itself, which prints it, is
/// [`crate::objects::commands::article`].
///
/// Port (spec 08): in Korean ([`crate::ui::Language`]) there is no article: the text is empty.
pub fn article_text(s: &State, tile: u16) -> &str {
    if s.ui.text.language == crate::ui::Language::Korean {
        return "";
    }
    let f3 = s.tables.tileflags.flags3.get(usize::from(tile)).copied();
    match f3.unwrap_or(0) & 0xc0 {
        0x40 => &s.objects.defs.articles[0],
        0x80 => &s.objects.defs.articles[1],
        0xc0 => &s.objects.defs.articles[2],
        _ => "",
    }
}

/// The text 0x129382 prints in front of `slot`'s name, and the count: gold its word and a
/// quantity stack its quantity byte through the count format (a lit (status 0x80) quantity
/// object takes the article path); anything else its tile's article and 1. The port of
/// 0x129382 itself, which prints it, is [`crate::objects::commands::name_prefix`].
pub fn name_prefix_text(s: &State, slot: Slot) -> (String, u16) {
    let o = &s.objects;
    let fill = |n: u16| o.defs.count_format.replace("%d", &n.to_string());
    match stack_kind(o, o.kind(slot)) {
        4 => (fill(o.qq_of(slot)), o.qq_of(slot)),
        2 if o.status(slot) & 0x80 == 0 => (fill(o.qq_of(slot) & 0xff), o.qq_of(slot) & 0xff),
        _ => {
            let tile = tile_of(&s.tables, o.tf(slot));
            (article_text(s, tile).to_string(), 1)
        }
    }
}

/// What the Look path prints for `slot`: the prefix of 0x129382 and the 0x10eef8 name with its
/// plural markers resolved for the count, as the printer 0x108376 does (`\\` letters only for a
/// count ≠ 1, `/` letters only for 1; in Korean, neither: [`drop_plural`]).
pub fn described_name(s: &State, slot: Slot) -> String {
    let (prefix, count) = name_prefix_text(s, slot);
    let name = display_name(s, slot);
    let name = match s.ui.text.language {
        crate::ui::Language::English => u6_formats::look::resolve_plural(&name, count != 1),
        crate::ui::Language::Korean => drop_plural(&name),
    };
    format!("{prefix}{name}")
}

/// Port-only (spec 08): the plural markers of `text` as the Korean printer reads them: each `/`
/// or `\\` and the ASCII letters after it print nothing.
fn drop_plural(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' || c == '/' {
            while chars.next_if(char::is_ascii_alphabetic).is_some() {}
        } else {
            out.push(c);
        }
    }
    out
}

/// Port-only (spec 08, slice K3): [`drop_plural`] of a pack's text (UTF-8, checked by its
/// reader), for an object name a Korean pack gives ([`crate::locale::look_at`]).
pub(crate) fn drop_plural_bytes(text: &[u8]) -> Vec<u8> {
    drop_plural(&String::from_utf8_lossy(text)).into_bytes()
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

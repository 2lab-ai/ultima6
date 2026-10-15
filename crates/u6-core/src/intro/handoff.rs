//! The hand-off from `u` to `game`: the new character goes into the save's objlist (0x105700),
//! then u exits with code 1 (0x10361a) and the launcher runs `game`, which loads that objlist.
//!
//! The port's files are read-only; a written `savegame/` file goes where `game`'s Save puts
//! its own (the save path of [`crate::saveload`]: the platform stores it after the tick, and
//! the next boot lays it over the user's files). [`apply`] writes the objlist there and loads
//! it into the game state as `game`'s load state 0x105350 reads `savegame/objlist`.

use crate::state::State;

use super::state::IntroState;

/// Objlist bytes 0x105700 writes (formats.md §6; actor 1 = the hero).
const STR: usize = 0x901;
const DEX: usize = 0xa01;
const INT: usize = 0xb01;
const EXP: usize = 0xc02;
const HP: usize = 0xe01;
const NAME: usize = 0xf00;
const LEVEL: usize = 0xff2;
const MAGIC: usize = 0x13f2;
const KARMA: usize = 0x1bf9;
const GENDER: usize = 0x1c71;
const PORTRAIT: usize = 0x1c72;
/// Bytes of the objlist the Amiga reads and writes.
const LEN: usize = 0x1c73;

/// Why 0x105700 wrote nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteError {
    /// `savegame/objlist` is not in the user's files (the original asks for the disk).
    Missing,
    /// The portrait byte is set: the save already holds a character.
    InProgress,
}

/// Original: u 0x105700 (creation: write the character into `savegame/objlist`) — the objlist
/// bytes, or `Err` when the save already holds a character (portrait byte nonzero; 0x105700
/// returns 1 and writes nothing, and its callers show passage 0x3e6 about it)
///
/// Strength, dexterity, intelligence, magic = 2 × intelligence, the sex (1 = female), the karma
/// byte, the 14 name bytes, experience (big-endian word), hit points (at most 255), level and
/// portrait + 1. The first 0x1c73 bytes are written back over the file, the rest is kept.
pub fn write_objlist(st: &IntroState) -> Result<Vec<u8>, WriteError> {
    let mut b = super::load::load_raw(&st.files, b"savegame/objlist").ok_or(WriteError::Missing)?;
    if b.len() < LEN {
        b.resize(LEN, 0);
    }
    if b[PORTRAIT] != 0 {
        return Err(WriteError::InProgress);
    }
    let h = &st.hero;
    b[STR] = h.str as u8;
    b[DEX] = h.dex as u8;
    b[INT] = h.int as u8;
    b[MAGIC] = (h.int as u8).wrapping_mul(2);
    b[GENDER] = u8::from(h.sex_char() == b'F');
    b[KARMA] = h.karma;
    b[NAME..NAME + 14].copy_from_slice(&h.name);
    b[EXP + 1] = (h.exp % 0x100) as u8;
    b[EXP] = (h.exp / 0x100) as u8;
    b[HP] = h.hp.min(0xff) as u8;
    b[LEVEL] = h.level as u8;
    b[PORTRAIT] = (h.portrait_item as u8).wrapping_add(1);
    Ok(b)
}

/// The objlist `u` wrote, into the save path and the game state: it becomes the session's
/// `savegame/objlist` ([`crate::saveload::SaveLoad::objlist`], a new save generation, so the
/// platform stores it as it stores a Save), and the tables are read from it as 0x105350 reads
/// them (the 25 tables, then the party's position), so `game` starts with that character.
pub fn apply(s: &mut State, objlist: &[u8]) {
    s.saveload.objlist = Some(objlist.to_vec());
    s.saveload.generation += 1;
    crate::saveload::objlist::decode(s, objlist);
    s.place_party();
}

/// The character fields of a written objlist, for receipts: (name, sex byte, portrait byte,
/// strength, dexterity, intelligence, magic, experience, hit points, level, karma).
pub fn summary(objlist: &[u8]) -> String {
    let at = |i: usize| objlist.get(i).copied().unwrap_or(0);
    let name: String = objlist
        .get(NAME..NAME + 14)
        .unwrap_or(&[])
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| char::from(c))
        .collect();
    format!(
        "name {name:?} @0xf00; sex 0x1c71={} portrait 0x1c72={} str 0x901={} dex 0xa01={} \
         int 0xb01={} magic 0x13f2={} exp 0xc02={} hp 0xe01={} level 0xff2={} karma 0x1bf9={}",
        at(GENDER),
        at(PORTRAIT),
        at(STR),
        at(DEX),
        at(INT),
        at(MAGIC),
        u16::from_be_bytes([at(EXP), at(EXP + 1)]),
        at(HP),
        at(LEVEL),
        at(KARMA),
    )
}

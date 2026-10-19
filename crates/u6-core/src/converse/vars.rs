//! Script variables: words at A4-0x3006 and string-slot pointers at A4-0x2fbe.
//!
//! A string slot holds a pointer in the original; the port keeps what it points at
//! ([`StrRef`]) and reads through it when the slot is used, so a slot that points at a buffer
//! sees later writes to that buffer as the original does.
//!
//! Port (spec 08, slice K2): what a slot points at always holds the English, which every logic
//! read takes (a comparison, a search, a copy's bytes). A copy of a table string the locale pack
//! has also keeps the pack's text beside it ([`set_shown`]), and a read for display
//! ([`shown_var`]: the escape layer's `$n`, op `b5`'s slot form) shows that text instead; the
//! forms of address, the times of day and `$Y`'s word ([`super::text::Shown::Label`]) show the
//! pack's label. A write to the place drops its shown text.

use super::{Msg, VAR_COUNT};
use crate::state::State;
use crate::ui::Language;

/// What a string slot points at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StrRef {
    /// Never set (the BSS long is 0).
    #[default]
    Null,
    /// One of the ten 100-byte buffers at A4+0x140b (slots 0..9 point there).
    Buffer(u8),
    /// A message in the code (the forms of address, the times of day, the word at 0x1129ce).
    Text(Msg),
    /// The NPC-name buffer 0x112768 (slot 0x17).
    NpcName,
    /// A party member's 14-byte name at A4+0x470c + 14 × slot.
    PartyName(u8),
    /// 0x112b20's name buffer A4+0x13ba.
    NameBuf,
    /// The input buffer A4+0x1388 (slot 0x23, `$Z`).
    Input,
}

/// String slots 0x1125b8 and 0x114afa set.
pub(crate) const SLOT_GENDER: usize = 0x10;
pub(crate) const SLOT_NPC_NAME: usize = 0x17;
pub(crate) const SLOT_PLAYER: usize = 0x19;
pub(crate) const SLOT_TIME: usize = 0x1d;
pub(crate) const SLOT_Y: usize = 0x22;
pub(crate) const SLOT_Z: usize = 0x23;

/// The bytes `r` points at (no NUL).
pub(crate) fn resolve(state: &State, r: StrRef) -> Vec<u8> {
    let c = &state.converse;
    match r {
        StrRef::Null => Vec::new(),
        StrRef::Buffer(i) => c.buffers.get(usize::from(i)).cloned().unwrap_or_default(),
        StrRef::Text(m) => c
            .text
            .as_ref()
            .map(|t| t.get(m).to_vec())
            .unwrap_or_default(),
        StrRef::NpcName => c.npc_name.clone(),
        StrRef::PartyName(i) => state
            .actors
            .party
            .names
            .get(usize::from(i))
            .map(|s| s.chars().map(|ch| ch as u32 as u8).collect())
            .unwrap_or_default(),
        StrRef::NameBuf => c.name_buf.clone(),
        StrRef::Input => c.input.clone(),
    }
}

/// Copies `bytes` to where `r` points (`strcpy`). Writes through a pointer at code text or at
/// address 0 would land in the original's code or low memory; the port drops them. Port (spec
/// 08, slice K2): the place's shown text goes ([`set_shown`]).
pub(crate) fn write(state: &mut State, r: StrRef, bytes: &[u8]) {
    let c = &mut state.converse;
    c.shown.retain(|(at, ..)| *at != r);
    match r {
        StrRef::Null | StrRef::Text(_) => {}
        StrRef::Buffer(i) => {
            if let Some(b) = c.buffers.get_mut(usize::from(i)) {
                *b = bytes.to_vec();
            }
        }
        StrRef::NpcName => c.npc_name = bytes.to_vec(),
        StrRef::NameBuf => c.name_buf = bytes.to_vec(),
        StrRef::Input => c.input = bytes.to_vec(),
        StrRef::PartyName(i) => {
            let names = &mut state.actors.party.names;
            let i = usize::from(i);
            if names.len() <= i {
                names.resize(i + 1, String::new());
            }
            names[i] = bytes.iter().map(|&b| char::from(b)).collect();
        }
    }
}

/// The most bytes the place `r` points at holds, for a copy of a table string's translation
/// (spec 08, slice K2): a buffer's ([`super::disasm::catalog::SLOT_BUFFER_CAPACITY`]); `None`
/// for every other place, whose size the port does not know (a translation is not shown there).
pub(crate) fn capacity(r: StrRef) -> Option<usize> {
    match r {
        StrRef::Buffer(_) => Some(super::disasm::catalog::SLOT_BUFFER_CAPACITY as usize),
        _ => None,
    }
}

/// Port-only (spec 08, slice K2): the place `r` points at, which holds a copy of a table string,
/// shows `text` (in `language`'s grammar) instead, until the next write to it.
pub(crate) fn set_shown(state: &mut State, r: StrRef, language: Language, text: Vec<u8>) {
    let c = &mut state.converse;
    c.shown.retain(|(at, ..)| *at != r);
    c.shown.push((r, language, text));
}

/// Op `a6`'s slot form: the English of `from` copied to where `to` points ([`write`]), and,
/// when `from` shows a table string's translation, `to` shows it too: only where the English was
/// written and the translation fits, as for the copy of the table string itself (a place
/// [`capacity`] knows, the translation within it). The shown text is taken before the write, so
/// a copy onto the same place (a slot onto itself, or onto another slot that points there) keeps
/// it.
pub(crate) fn copy(state: &mut State, from: StrRef, to: StrRef) {
    let text = resolve(state, from);
    let shown = state
        .converse
        .shown
        .iter()
        .find(|(at, ..)| *at == from)
        .cloned();
    write(state, to, &text);
    if let Some((_, language, translation)) = shown
        && capacity(to).is_some_and(|c| translation.len() <= c)
    {
        set_shown(state, to, language, translation);
    }
}

/// Port-only (spec 08, slice K2): what the place `r` points at shows, for a read for display: its
/// shown text with the pack's language, a label as the locale pack has it (counted,
/// [`crate::locale::shown`]), else its bytes (`None`: English). `quiet` counts nothing (the copy
/// of the escape layer `printf`'s cell count runs).
pub(crate) fn shown_ref(state: &State, r: StrRef, quiet: bool) -> (Option<Language>, Vec<u8>) {
    if let Some((_, language, text)) = state.converse.shown.iter().find(|(at, ..)| *at == r) {
        return (Some(*language), text.clone());
    }
    let english = resolve(state, r);
    if let StrRef::Text(m) = r
        && super::text::is_label(m)
    {
        let found = if quiet {
            crate::locale::label_quiet(state, &english)
                .map(|t| (state.locale.pack.as_ref().map(|p| p.language), t))
        } else {
            crate::locale::exe_string(state, &english).map(|(l, t)| (Some(l), t))
        };
        if let Some((language, text)) = found {
            return (language, text);
        }
    }
    (None, english)
}

/// Port-only (spec 08, slice K2): the text string slot `idx` shows ([`shown_ref`], counted): the
/// escape layer's `$n`.
pub fn shown_var(state: &State, idx: u8) -> Vec<u8> {
    shown_ref(state, slot(state, idx), false).1
}

/// [`shown_var`] counting nothing: the copy of the escape layer `printf`'s cell count runs.
pub(crate) fn shown_var_quiet(state: &State, idx: u8) -> Vec<u8> {
    shown_ref(state, slot(state, idx), true).1
}

/// The text of string slot `idx` (`$0`..`$Z` = 0..35); out of range: empty.
pub fn string_var(state: &State, idx: u8) -> Vec<u8> {
    let r = slot(state, idx);
    resolve(state, r)
}

/// The pointer of string slot `idx` (out of range: [`StrRef::Null`]).
pub(crate) fn slot(state: &State, idx: u8) -> StrRef {
    state
        .converse
        .strs
        .get(usize::from(idx))
        .copied()
        .unwrap_or_default()
}

/// Sets string slot `idx` (out of range: dropped; the original would overwrite the words that
/// follow the table).
pub(crate) fn set_slot(state: &mut State, idx: usize, r: StrRef) {
    if let Some(s) = state.converse.strs.get_mut(idx) {
        *s = r;
    }
}

/// Word variable `idx` (`#0`..`#Z` = 0..35); out of range: 0 (the original would read the
/// string-slot table that follows; the shipped scripts never do).
pub fn int_var(state: &State, idx: u8) -> u16 {
    state
        .converse
        .ints
        .get(usize::from(idx))
        .copied()
        .unwrap_or(0)
}

/// Sets word variable `idx` (out of range: dropped).
pub(crate) fn set_int(state: &mut State, idx: usize, v: u16) {
    if idx < VAR_COUNT
        && let Some(w) = state.converse.ints.get_mut(idx)
    {
        *w = v;
    }
}

/// The variable index a `$`/`#` letter names: `0`..`9` → 0..9, `A`..`Z` → 10..35, anything
/// else 0 (0x108376: lowercase letters are folded to upper case first).
pub fn var_letter(c: u8) -> u8 {
    match c.to_ascii_uppercase() {
        d @ b'0'..=b'9' => d - b'0',
        l @ b'A'..=b'Z' => l - b'A' + 10,
        _ => 0,
    }
}

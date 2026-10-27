//! Conversations: Talk 0x1127c4, the script interpreter 0x1130a2, script and portrait loading
//! 0x133e5e / 0x133d3e (WU-P6).
//!
//! Domain state = [`ConverseState`]: the 0x2800-byte script buffer and its read position, the
//! conversation flags, the input and scratch buffers and the script variables (36 words and 36
//! string slots). Entry points: [`talk`] (the T command, 0x1127c4), [`begin`] (0x1125b8, also
//! the spell path), [`load_script`] (0x133e5e), [`load_portrait`] / [`draw_portrait`]
//! (0x133d3e). Calls into modules that are not ported yet go through `seams`.
//!
//! Script shape (read off 0x114afa / 0x1148f0): `ff npc name f1 look-text f2 greeting`, then a
//! loop of input ops (`f7` ask, `f8` ask-key, `f9`/`fa`/`fb`/`fc` input into a variable, `cb`
//! wait) each followed by text with keyword blocks `ef kw,kw f6 answer … ee`. Inside text,
//! bytes < 0x80 print, 0x80..0xee are ops (0x11404c); 0x80-up values inside expressions are
//! evaluated by 0x1130a2 up to `a7`/`a8`. Multi-byte operands are little-endian.
//!
//! Original functions this module will own (milestone 1):
//!
//! - 0x1125b8 [converse] converse text substitutions (forms of address, time of day)
//! - 0x1127c4 [converse] Talk command: target, shrine/statue cases, start talk
//! - 0x1128d0 [converse] unnamed; 1 callees
//! - 0x112b0a [converse] leaf; callers 2, from 1 subsystems
//! - 0x112b20 [converse] uses string 0x112984
//! - 0x112ba4 [converse] unnamed; 1 callees
//! - 0x112bf2 [converse] leaf; callers 7, from 2 subsystems
//! - 0x112d72 [converse] step of 0x114afa (start conversation: checks, load ~)
//! - 0x112daa [converse] unnamed; 2 callees
//! - 0x112eb8 [converse] step of 0x1130a2 (converse interpreter main loop (i~)
//! - 0x11304a [converse] unnamed; 2 callees
//! - 0x1130a2 [converse] converse interpreter main loop (input, keyword match)
//! - 0x113f8a [converse] step of 0x11404c (converse opcode executor)
//! - 0x11404c [converse] converse opcode executor
//! - 0x1148f0 [converse] print converse text, control chars
//! - 0x1149d6 [converse] step of 0x1148f0 (print converse text, control chars)
//! - 0x114afa [converse] start conversation: checks, load script, greeting
//! - 0x11519e [converse] step of 0x1130a2 (converse interpreter main loop (i~)
//! - 0x133d3e [converse] load an NPC portrait (portrait.a)
//! - 0x133e5e [converse] load an NPC's converse.a script block

pub mod disasm;
mod eval;
mod exec;
mod party;
mod run;
mod script;
pub(crate) mod seams;
pub(crate) mod start;
pub mod text;
mod vars;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_locale;
#[cfg(test)]
mod tests_ops;
#[cfg(test)]
mod tests_real;
#[cfg(test)]
mod tests_speech;
#[cfg(test)]
mod tests_strings;
#[cfg(test)]
mod tests_typing;

use std::cell::RefCell;

pub(crate) use party::member_with_object;
pub use party::{actor_name, join_party, leave_party, on_screen};
pub(crate) use script::script_item;
pub use script::{
    PORTRAIT_LETTERS, SCRIPT_LEN, archive_name, draw_portrait, load_portrait, load_script,
    portrait_pixels, portrait_ref, portraits,
};
pub use start::{begin, converse, talk};
pub use text::{ConverseText, Msg};
pub(crate) use vars::shown_var_quiet;
pub use vars::{StrRef, int_var, shown_var, string_var, var_letter};

use crate::engine::Io;
use crate::state::State;

/// Script variables: 36 words (A4-0x3006) and 36 string slots (A4-0x2fbe). The original indexes
/// both with a full byte; word index 0x24 and up would alias the string table there.
pub const VAR_COUNT: usize = 0x24;
/// The ten 100-byte string buffers string slots 0..9 point at (A4+0x140b).
pub const STRING_BUFFERS: usize = 10;
/// Line-input limit the converse code passes to 0x107cba (`moveq #0x32, d0`).
pub const INPUT_MAX: usize = 0x32;

/// Conversation interpreter state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConverseState {
    /// Original: 0x144240 (0x2800-byte script buffer, the end of BSS hunk 23; a load overwrites
    /// only the bytes it reads, the rest keeps the previous script)
    pub script: Vec<u8>,
    /// Original: A4+0x1408 (word: script read position; `adda.w` makes it signed)
    pub pc: u16,
    /// Original: A4+0x1402 (word: the NPC being talked to; op operand 0xeb means this one)
    pub npc: i16,
    /// Original: A4+0x1404 (word: the actor who talks)
    pub talker: i16,
    /// Original: A4+0x13f6 (long: one shared saved read position)
    pub saved_pc: i32,
    /// Original: A4+0x13fe (long: read position of the look text after `f1`)
    pub look_pc: i32,
    /// Original: A4+0x140a (byte: a keyword block answered the current input)
    pub answered: bool,
    /// Original: A4-0x71b6 (byte: set by op 0xb6, ends the conversation)
    pub bye: bool,
    /// Original: A4-0x71b5 (byte: a conversation is running)
    pub active: bool,
    /// Original: A4+0x1821 (byte: set only around the magic subsystem's call 0x119dbc → 0x1125b8)
    pub by_spell: bool,
    /// Original: A4+0x1388 (the player's input, NUL not stored; also `$Z`)
    pub input: Vec<u8>,
    /// Original: A4+0x13ec (keyword / allowed-key scratch buffer)
    pub scratch: Vec<u8>,
    /// Original: A4+0x13ba (name buffer of 0x112b20)
    pub name_buf: Vec<u8>,
    /// Original: 0x112768 (the NPC-name buffer in code hunk 9, string slot 0x17)
    pub npc_name: Vec<u8>,
    /// Original: A4-0x3006 (word variables `#0`..`#Z`)
    pub ints: Vec<u16>,
    /// Original: A4-0x2fbe (string-slot pointers `$0`..`$Z`)
    pub strs: Vec<StrRef>,
    /// Original: A4+0x140b (ten 100-byte buffers string slots 0..9 point at)
    pub buffers: Vec<Vec<u8>>,
    /// Port-only cache: the messages, read from `game` on the first conversation.
    pub text: Option<ConverseText>,
    /// Port-only: the read position left the buffer; every scan loop stops on it (the original
    /// would read past its buffer).
    pub fault: bool,
    /// Port-only: state of the `put_char` seam fallback (a stand-in for 0x108376).
    pub out: OutFallback,
    /// Port-only: ops run since the last forced yield (bounded-work checkpoint, spec 03 §1).
    pub steps: u32,
    /// Port-only (spec 08): the archive letter and item of the script last loaded into
    /// `script` (the locale layer's key), `None` before a load.
    pub script_key: Option<(u8, u16)>,
    /// Port-only (spec 08, slice K2): the places that hold a copy of a table string the locale
    /// pack has, with the pack's language and text, which a read for display shows
    /// ([`vars::shown_var`]); logic reads the English.
    pub shown: Vec<(StrRef, crate::ui::Language, Vec<u8>)>,
}

impl Default for ConverseState {
    fn default() -> Self {
        Self {
            script: Vec::new(),
            pc: 0,
            npc: 0,
            talker: 0,
            saved_pc: 0,
            look_pc: 0,
            answered: false,
            bye: false,
            active: false,
            by_spell: false,
            input: Vec::new(),
            scratch: Vec::new(),
            name_buf: Vec::new(),
            npc_name: Vec::new(),
            ints: vec![0; VAR_COUNT],
            strs: vec![StrRef::Null; VAR_COUNT],
            buffers: vec![Vec::new(); STRING_BUFFERS],
            text: None,
            fault: false,
            out: OutFallback::default(),
            steps: 0,
            script_key: None,
            shown: Vec::new(),
        }
    }
}

/// Port-only: what the stand-in for the message-scroll character output keeps between
/// characters (0x108376 keeps it in A4-0x796f, A4-0x7a4e and A4-0x7a54).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutFallback {
    /// Pending prefix: `#` `$` `%` `/` `@` `\` `^`, or 0.
    pub prefix: u8,
    /// Inside `&`..`&` (text in the gargoyles' tongue, shown only while A4-0x7b26 is set).
    pub gargoyle_tongue: bool,
    /// The last `#` variable was not 1 (`/` and `\\` pick singular / plural letters).
    pub plural: bool,
}

/// What converse code runs against: the shared state and the platform view. Every helper
/// borrows the state only inside one synchronous step, never across an `.await`.
#[derive(Clone, Copy)]
pub struct Cx<'a> {
    pub state: &'a RefCell<State>,
    pub io: &'a Io,
}

impl<'a> Cx<'a> {
    pub fn new(state: &'a RefCell<State>, io: &'a Io) -> Self {
        Self { state, io }
    }

    /// Runs `f` on the converse state (one short borrow).
    pub(crate) fn cv<R>(&self, f: impl FnOnce(&mut ConverseState) -> R) -> R {
        f(&mut self.state.borrow_mut().converse)
    }

    /// Runs `f` on the whole state (one short borrow).
    pub(crate) fn st<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.state.borrow_mut())
    }

    /// The bytes of a message (empty before the table is loaded).
    pub(crate) fn msg(&self, m: Msg) -> Vec<u8> {
        self.cv(|c| {
            c.text
                .as_ref()
                .map(|t| t.get(m).to_vec())
                .unwrap_or_default()
        })
    }
}

//! The converse messages, read at run time from the user's `game` executable (spec 03 §5 "data,
//! not constants"): the original prints string literals that live in its code hunks 9 and 10
//! and passes file names from its data hunk 33 (plus the word-end table of hunk 2 that the
//! message-scroll stand-in uses). This module holds only their image addresses; the hunks'
//! bytes come from the shared hunk loader [`crate::image::GameImage`].
//!
//! Version check: 35 hunks, and hunks 2, 9, 10 and 33 at their image bases
//! (`re/notes/game-layout.md`, "`game` segments"); every message must lie in its hunk's loaded
//! bytes (their length is the hunk header's) and end in a NUL there.

/// One message the converse code prints or uses, named after its role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Msg {
    Milady,
    Milord,
    Morning,
    Afternoon,
    Evening,
    Shrine,
    Statue,
    Nothing,
    Delimiters,
    Undefined,
    SelectItem,
    DivideError,
    ErrorOccured,
    You,
    UnknownCommand,
    ControlChar,
    Bye,
    Newline,
    NotOnScreen,
    NotInSolo,
    DeepMoan,
    NoResponseNpc,
    NoResponse,
    TalkingToYourself,
    FunnyNoResponse,
    YouSeeFirst,
    YouSay,
    TwoNewlines,
    FormatNumber,
    FormatString,
    FormatChar,
    KeyCommandExpected,
    LogicalError,
    Look,
    YouSee,
    ConverseFile,
    PortraitFile,
    WordEnds,
}

/// Image address of every message (the `lea`/`pea` (d16,PC) or A4-relative operand that names
/// it, resolved by hand from the code of the function in brackets).
pub use crate::string_registry::CONVERSATION_MESSAGES as MESSAGES;

/// Port-only (spec 08, slice K2): how the converse code shows a message, for the locale layer's
/// catalogue ([`crate::locale::strings`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// Passed to `printf` as its format (`seams::print`).
    Printed,
    /// Set as a string slot ([`super::StrRef::Text`]) and shown through `$n`: the forms of
    /// address (`$G`), the times of day (`$T`) and the word `$Y` names the hero by. The locale
    /// pack's text is shown where a slot is read for display ([`super::vars::shown`]), never where
    /// logic reads it (a comparison, a search, a copy).
    Label,
}

/// Port-only (spec 08, slice K2): the messages the converse code shows, and how. Left out: the
/// ones logic reads (the word delimiters, `bye` for an empty input, the `look` word, the
/// keyword-end bytes), the file names, and the newlines and one-conversion formats (shorter
/// than the catalogue's least).
pub use crate::string_registry::CONVERSATION_SHOWN as SHOWN;

/// Port-only (spec 08, slice K2): whether `msg` is shown through a string slot ([`Shown::Label`]).
pub fn is_label(msg: Msg) -> bool {
    SHOWN
        .iter()
        .any(|&(m, how)| m == msg && how == Shown::Label)
}

/// The messages, without their NUL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConverseText {
    texts: Vec<(Msg, Vec<u8>)>,
}

impl ConverseText {
    /// Builds a text table from explicit strings (tests; tools that bring their own text).
    pub fn from_pairs(pairs: impl IntoIterator<Item = (Msg, Vec<u8>)>) -> Self {
        let mut texts: Vec<(Msg, Vec<u8>)> = pairs.into_iter().collect();
        texts.sort();
        Self { texts }
    }

    /// The bytes of `msg` (empty if the table lacks it).
    pub fn get(&self, msg: Msg) -> &[u8] {
        self.texts
            .binary_search_by(|(m, _)| m.cmp(&msg))
            .map_or(&[][..], |i| &self.texts[i].1)
    }

    /// Reads every message out of the decrunched `game` executable.
    pub fn from_game(game: &[u8]) -> Result<Self, String> {
        crate::string_registry::conversation(game).map(Self::from_pairs)
    }
}

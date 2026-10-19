//! The converse messages, read at run time from the user's `game` executable (spec 03 §5 "data,
//! not constants"): the original prints string literals that live in its code hunks 9 and 10
//! and passes file names from its data hunk 33 (plus the word-end table of hunk 2 that the
//! message-scroll stand-in uses). This module holds only their image addresses; the hunks'
//! bytes come from the shared hunk loader [`crate::image::GameImage`].
//!
//! Version check: 35 hunks, and hunks 2, 9, 10 and 33 at their image bases
//! (`re/notes/game-layout.md`, "`game` segments"); every message must lie in its hunk's loaded
//! bytes (their length is the hunk header's) and end in a NUL there.

use crate::image::GameImage;

/// Hunk count of the `game` executable this table was measured on.
const GAME_HUNKS: usize = 35;
/// (hunk, image base) of the hunks the messages live in.
const HUNKS: [(usize, u32); 4] = [
    (2, 0x0010_4fb0),
    (9, 0x0011_25b8),
    (10, 0x0011_28d0),
    (33, 0x0015_36f0),
];

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
pub const MESSAGES: [(Msg, u32); 38] = [
    (Msg::Milady, 0x0011_2758),             // [0x1125b8]
    (Msg::Milord, 0x0011_2760),             // [0x1125b8]
    (Msg::Morning, 0x0011_2790),            // [0x1125b8]
    (Msg::Afternoon, 0x0011_2798),          // [0x1125b8]
    (Msg::Evening, 0x0011_27a2),            // [0x1125b8]
    (Msg::Shrine, 0x0011_27aa),             // [0x1127c4]
    (Msg::Statue, 0x0011_27b2),             // [0x1127c4]
    (Msg::Nothing, 0x0011_27ba),            // [0x1127c4]
    (Msg::Delimiters, 0x0011_2982),         // [0x1128d0] strtok delimiters
    (Msg::Undefined, 0x0011_2984),          // [0x112b20]
    (Msg::SelectItem, 0x0011_2992),         // [0x1130a2] op 0xc0
    (Msg::DivideError, 0x0011_29a2),        // [0x1130a2] op 0x93
    (Msg::ErrorOccured, 0x0011_29be),       // [0x1130a2] stack left over
    (Msg::You, 0x0011_29ce),                // [0x11404c] op 0xdf
    (Msg::UnknownCommand, 0x0011_29d2),     // [0x11404c]
    (Msg::ControlChar, 0x0011_29e4),        // [0x1148f0]
    (Msg::Bye, 0x0011_2a06),                // [0x1149d6] empty input
    (Msg::Newline, 0x0011_2a0a),            // [0x1149d6] [0x114afa]
    (Msg::NotOnScreen, 0x0011_2a0c),        // [0x114afa]
    (Msg::NotInSolo, 0x0011_2a1e),          // [0x114afa]
    (Msg::DeepMoan, 0x0011_2a32),           // [0x114afa]
    (Msg::NoResponseNpc, 0x0011_2a4a),      // [0x114afa]
    (Msg::NoResponse, 0x0011_2a5a),         // [0x114afa]
    (Msg::TalkingToYourself, 0x0011_2a68),  // [0x114afa]
    (Msg::FunnyNoResponse, 0x0011_2a80),    // [0x114afa]
    (Msg::YouSeeFirst, 0x0011_2a96),        // [0x114afa]
    (Msg::YouSay, 0x0011_2aa2),             // [0x114afa]
    (Msg::TwoNewlines, 0x0011_2aac),        // [0x114afa]
    (Msg::FormatNumber, 0x0011_2ab0),       // [0x114afa]
    (Msg::FormatString, 0x0011_2ab6),       // [0x114afa]
    (Msg::FormatChar, 0x0011_2abc),         // [0x114afa]
    (Msg::KeyCommandExpected, 0x0011_2ac0), // [0x114afa]
    (Msg::LogicalError, 0x0011_2ae2),       // [0x114afa]
    (Msg::Look, 0x0011_2afa),               // [0x114afa]
    (Msg::YouSee, 0x0011_2b00),             // [0x114afa]
    (Msg::ConverseFile, 0x0015_5344),       // [0x133e5e] A4-0x63ac
    (Msg::PortraitFile, 0x0015_5339),       // [0x133d3e] A4-0x63b7
    (Msg::WordEnds, 0x0010_4ffe),           // [0x108376] bytes that end an `@` keyword
];

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
pub const SHOWN: [(Msg, Shown); 27] = [
    (Msg::Milady, Shown::Label),
    (Msg::Milord, Shown::Label),
    (Msg::Morning, Shown::Label),
    (Msg::Afternoon, Shown::Label),
    (Msg::Evening, Shown::Label),
    (Msg::You, Shown::Label),
    (Msg::Shrine, Shown::Printed),
    (Msg::Statue, Shown::Printed),
    (Msg::Nothing, Shown::Printed),
    (Msg::Undefined, Shown::Printed),
    (Msg::SelectItem, Shown::Printed),
    (Msg::DivideError, Shown::Printed),
    (Msg::ErrorOccured, Shown::Printed),
    (Msg::UnknownCommand, Shown::Printed),
    (Msg::ControlChar, Shown::Printed),
    (Msg::NotOnScreen, Shown::Printed),
    (Msg::NotInSolo, Shown::Printed),
    (Msg::DeepMoan, Shown::Printed),
    (Msg::NoResponseNpc, Shown::Printed),
    (Msg::NoResponse, Shown::Printed),
    (Msg::TalkingToYourself, Shown::Printed),
    (Msg::FunnyNoResponse, Shown::Printed),
    (Msg::YouSeeFirst, Shown::Printed),
    (Msg::YouSay, Shown::Printed),
    (Msg::KeyCommandExpected, Shown::Printed),
    (Msg::LogicalError, Shown::Printed),
    (Msg::YouSee, Shown::Printed),
];

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
        let img = GameImage::parse(game).map_err(|e| format!("game: {e}"))?;
        if img.segment_count() != GAME_HUNKS {
            return Err(format!(
                "game: {} hunks, expected {GAME_HUNKS}",
                img.segment_count()
            ));
        }
        for &(n, base) in &HUNKS {
            if img.segment_base(n) != Some(base) {
                return Err(format!("game: hunk {n} is not at {base:#x}"));
            }
        }
        let mut pairs = Vec::with_capacity(MESSAGES.len());
        for (msg, addr) in MESSAGES {
            // The last known hunk that starts at or below the address holds it, if anywhere.
            let (n, base) = HUNKS
                .iter()
                .copied()
                .filter(|&(_, base)| addr >= base)
                .max_by_key(|&(_, base)| base)
                .ok_or_else(|| format!("{msg:?}: {addr:#x} outside the known hunks"))?;
            let hunk = img.loaded(n).unwrap_or_default();
            let bytes = hunk
                .get((addr - base) as usize..)
                .filter(|b| !b.is_empty())
                .ok_or_else(|| format!("{msg:?}: {addr:#x} past the loaded bytes of hunk {n}"))?;
            let end = bytes
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| format!("{msg:?}: no NUL inside hunk {n}"))?;
            pairs.push((msg, bytes[..end].to_vec()));
        }
        Ok(Self::from_pairs(pairs))
    }
}

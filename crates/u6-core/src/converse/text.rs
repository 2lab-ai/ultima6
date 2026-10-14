//! The converse messages, read at run time from the user's `game` executable (spec 03 §5 "data,
//! not constants"): the original prints string literals that live in its code hunks 9 and 10
//! and passes file names from its data hunk 33 (plus the word-end table of hunk 2 that the
//! message-scroll stand-in uses). This module holds only their image addresses
//! and a minimal AmigaDOS hunk walker that finds those hunks' bytes in the file.
//!
//! Version check: 35 hunks, hunk 2 = CODE 0x3a28 bytes, hunk 9 = CODE 0x318 bytes, hunk 10 = CODE 0x2958 bytes, hunk 33 =
//! DATA with 0x4f0c bytes loaded (`re/notes/game-layout.md`, "`game` segments"); every message
//! must end in a NUL inside its hunk.

/// `HUNK_HEADER`, `HUNK_CODE`, `HUNK_DATA`, `HUNK_BSS`, `HUNK_RELOC32`, `HUNK_END`.
const HUNK_HEADER: u32 = 0x3f3;
const HUNK_CODE: u32 = 0x3e9;
const HUNK_DATA: u32 = 0x3ea;
const HUNK_BSS: u32 = 0x3eb;
const HUNK_RELOC32: u32 = 0x3ec;
const HUNK_END: u32 = 0x3f2;
/// Hunk count of the `game` executable this table was measured on.
const GAME_HUNKS: usize = 35;
/// (hunk, image base, loaded bytes) of the three hunks the messages live in.
const HUNKS: [(usize, u32, usize); 4] = [
    (2, 0x0010_4fb0, 0x3a28),
    (9, 0x0011_25b8, 0x318),
    (10, 0x0011_28d0, 0x2958),
    (33, 0x0015_36f0, 0x4f0c),
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
        let hunks = hunk_ranges(game)?;
        if hunks.len() != GAME_HUNKS {
            return Err(format!(
                "game: {} hunks, expected {GAME_HUNKS}",
                hunks.len()
            ));
        }
        for &(n, _, len) in &HUNKS {
            let got = hunks[n].as_ref().map(|r| r.len());
            if got != Some(len) {
                return Err(format!(
                    "game: hunk {n} holds {got:?} bytes, expected {len:#x}"
                ));
            }
        }
        let mut pairs = Vec::with_capacity(MESSAGES.len());
        for (msg, addr) in MESSAGES {
            let (n, base, len) = HUNKS
                .iter()
                .copied()
                .find(|&(_, base, len)| addr >= base && ((addr - base) as usize) < len)
                .ok_or_else(|| format!("{msg:?}: {addr:#x} outside the known hunks"))?;
            let range = hunks[n].clone().unwrap_or_default();
            let bytes = &game[range][(addr - base) as usize..len];
            let end = bytes
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| format!("{msg:?}: no NUL inside hunk {n}"))?;
            pairs.push((msg, bytes[..end].to_vec()));
        }
        Ok(Self::from_pairs(pairs))
    }
}

fn long(data: &[u8], at: usize) -> Result<u32, String> {
    at.checked_add(4)
        .and_then(|end| data.get(at..end))
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("game: truncated at {at:#x}"))
}

/// File ranges of the loaded bytes of each CODE/DATA hunk (`None` for BSS), in hunk order.
fn hunk_ranges(f: &[u8]) -> Result<Vec<Option<std::ops::Range<usize>>>, String> {
    if long(f, 0)? != HUNK_HEADER {
        return Err("game: not an AmigaDOS executable".into());
    }
    let mut at = 4;
    // Resident library names: (length in longs, name) until a 0 length.
    loop {
        let n = long(f, at)? as usize;
        at += 4;
        if n == 0 {
            break;
        }
        at += n * 4;
    }
    let (first, last) = (long(f, at + 4)? as usize, long(f, at + 8)? as usize);
    let count = last
        .checked_sub(first)
        .map(|d| d + 1)
        .ok_or("game: bad hunk table")?;
    at += 12 + count * 4;
    let mut hunks = Vec::with_capacity(count);
    while hunks.len() < count {
        let id = long(f, at)? & 0x3fff_ffff;
        at += 4;
        match id {
            HUNK_CODE | HUNK_DATA => {
                let len = long(f, at)? as usize * 4;
                at += 4;
                if at + len > f.len() {
                    return Err(format!("game: hunk {} truncated", hunks.len()));
                }
                hunks.push(Some(at..at + len));
                at += len;
            }
            HUNK_BSS => {
                at += 4;
                hunks.push(None);
            }
            other => return Err(format!("game: unexpected block {other:#x} at {at:#x}")),
        }
        // Relocation blocks, then HUNK_END.
        loop {
            match long(f, at)? & 0x3fff_ffff {
                HUNK_RELOC32 => {
                    at += 4;
                    loop {
                        let n = long(f, at)? as usize;
                        at += 4;
                        if n == 0 {
                            break;
                        }
                        at += 4 + n * 4;
                    }
                }
                HUNK_END => {
                    at += 4;
                    break;
                }
                other => return Err(format!("game: unexpected block {other:#x} at {at:#x}")),
            }
        }
    }
    Ok(hunks)
}

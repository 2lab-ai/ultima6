//! Made-up files of the shapes the exporters read, for the tests: a conversation library of one
//! script, and an executable of the hunk layout the conversation text is read from. No byte of
//! them is the game's: every text is the caller's tag and a number.

use u6_core::converse::text::MESSAGES;
use u6_core::image::IMAGE_BASE;

const HUNK_CODE: u32 = 0x3E9;
const HUNK_BSS: u32 = 0x3EB;
const HUNK_END: u32 = 0x3F2;
const HUNK_HEADER: u32 = 0x3F3;
/// The hunks of the executable the engine knows.
const GAME_HUNKS: usize = 35;
/// The hunks that hold the conversation's messages, at the image bases the engine checks
/// (`u6_core::converse::text`'s layout: addresses of the loaded image, not its content).
const HUNKS: [(usize, u32); 4] = [
    (2, 0x0010_4fb0),
    (9, 0x0011_25b8),
    (10, 0x0011_28d0),
    (33, 0x0015_36f0),
];

/// A conversation script: a header with a name (`Nm`), a look text, `text` as the greeting, and
/// one keyword block that answers anything.
pub fn script(text: &str) -> Vec<u8> {
    [
        &b"\xff\x05Nm\xf1look\xf2"[..],
        text.as_bytes(),
        b"\xf7\xef*\xf6Ok\xee\xff",
    ]
    .concat()
}

/// A conversation library (`converse.a` or `converse.b`) of two items: none, then `script`
/// stored raw.
pub fn converse(script: &[u8]) -> Vec<u8> {
    [
        &0u32.to_le_bytes()[..],
        &8u32.to_le_bytes(),
        &[0; 4],
        script,
    ]
    .concat()
}

/// An executable (`game`) of 35 hunks with the message hunks at their bases, each message the
/// tag and its number (cut to end before the next message starts), zeros elsewhere: its
/// conversation text reads, and the messages of 3 bytes or more are catalogued.
pub fn game(tag: &str) -> Vec<u8> {
    let mut starts: Vec<u32> = MESSAGES.iter().map(|&(_, at)| at).collect();
    starts.sort_unstable();
    let mut hunks: Vec<(usize, u32, Vec<u8>)> = HUNKS
        .iter()
        .map(|&(n, base)| (n, base, Vec::new()))
        .collect();
    for (i, &(_, at)) in MESSAGES.iter().enumerate() {
        let room = starts
            .iter()
            .find(|&&s| s > at)
            .map_or(32, |&next| (next - at - 1) as usize);
        let mut text = format!("{tag} {i}").into_bytes();
        text.truncate(room);
        // The last hunk that starts at or below the message holds it.
        let (_, base, bytes) = hunks
            .iter_mut()
            .filter(|h| h.1 <= at)
            .max_by_key(|h| h.1)
            .expect("every message lies in a known hunk");
        let off = (at - *base) as usize;
        if bytes.len() < off + text.len() + 1 {
            bytes.resize(off + text.len() + 1, 0);
        }
        bytes[off..off + text.len()].copy_from_slice(&text);
    }
    // (kind, bytes, size) per segment, laid out from IMAGE_BASE, each base aligned to 8.
    let mut segments: Vec<(u32, Vec<u8>, u32)> = Vec::new();
    let mut at = IMAGE_BASE;
    let mut push = |segments: &mut Vec<_>, kind, bytes: Vec<u8>, size: u32| {
        segments.push((kind, bytes, size));
        at = (at + size + 7) & !7;
        at
    };
    let mut next = IMAGE_BASE;
    for (k, (n, base, mut bytes)) in hunks.into_iter().enumerate() {
        // A hunk followed by the next one reaches its base (nothing lies between them).
        if let Some(&(m, after)) = HUNKS.get(k + 1)
            && m == n + 1
        {
            bytes.resize((after - base) as usize, 0);
        }
        bytes.resize(bytes.len().next_multiple_of(4), 0);
        while segments.len() + 1 < n {
            next = push(&mut segments, HUNK_BSS, Vec::new(), 0);
        }
        if segments.len() < n {
            next = push(&mut segments, HUNK_BSS, Vec::new(), base - next);
        }
        assert_eq!(next, base, "hunk {n} lands at its base");
        let size = bytes.len() as u32;
        next = push(&mut segments, HUNK_CODE, bytes, size);
    }
    while segments.len() < GAME_HUNKS {
        push(&mut segments, HUNK_BSS, Vec::new(), 0);
    }
    let mut f = Vec::new();
    let long = |f: &mut Vec<u8>, v: u32| f.extend_from_slice(&v.to_be_bytes());
    let n = segments.len() as u32;
    for v in [HUNK_HEADER, 0, n, 0, n - 1] {
        long(&mut f, v);
    }
    for (_, _, size) in &segments {
        long(&mut f, size / 4);
    }
    for (kind, bytes, size) in &segments {
        long(&mut f, *kind);
        if *kind == HUNK_BSS {
            long(&mut f, size / 4);
        } else {
            long(&mut f, bytes.len() as u32 / 4);
            f.extend_from_slice(bytes);
        }
        long(&mut f, HUNK_END);
    }
    f
}

//! The text runner 0x1148f0, keyword blocks 0x1149d6 and the keyword test 0x1128d0.

use std::future::Future;
use std::pin::Pin;

use super::eval::YIELD_EVERY;
use super::exec::exec;
use super::seams;
use super::{Cx, Msg};

/// `b0` (jump), `d3`, `d4` carry 4 / 1 / 2 operand bytes the skip loops step over (`d2`'s four
/// are not skipped by the original, nor here).
pub(crate) fn operand_len(op: u8) -> i16 {
    match op {
        0xb0 => 4,
        0xd3 => 1,
        0xd4 => 2,
        _ => 0,
    }
}

/// Original: 0x1128d0 (does any word of the input match a keyword)
///
/// `input` is split with `strtok` at the delimiter bytes; a word matches when its first `len`
/// bytes equal the keyword's, upper case folded to lower and `?` in the keyword matching any
/// byte. Bytes past a word's end are what `strtok` left in its copy of the input (the NUL it
/// wrote, then the rest), as in the original.
pub(crate) fn match_keyword(keyword: &[u8], input: &[u8], len: usize, delims: &[u8]) -> bool {
    let mut buf = input.to_vec();
    buf.push(0);
    let low = |c: u8| if c.is_ascii_uppercase() { c + 0x20 } else { c };
    let mut at = 0;
    while let Some(start) = strtok(&mut buf, &mut at, delims) {
        let ok = (0..len).all(|i| {
            let k = keyword.get(i).copied().unwrap_or(0);
            let t = buf.get(start + i).copied().unwrap_or(0);
            low(k) == low(t) || k == b'?'
        });
        if ok {
            return true;
        }
    }
    false
}

/// `strtok` over a NUL-terminated buffer: the start of the next token, NUL written after it.
fn strtok(buf: &mut [u8], at: &mut usize, delims: &[u8]) -> Option<usize> {
    let is_delim = |b: u8| b != 0 && delims.contains(&b);
    while *at < buf.len() && is_delim(buf[*at]) {
        *at += 1;
    }
    if *at >= buf.len() || buf[*at] == 0 {
        return None;
    }
    let start = *at;
    while *at < buf.len() && buf[*at] != 0 && !is_delim(buf[*at]) {
        *at += 1;
    }
    if *at < buf.len() && buf[*at] != 0 {
        buf[*at] = 0;
        *at += 1;
    }
    Some(start)
}

/// Original: 0x1148f0 (print converse text, run ops, keyword blocks)
///
/// Runs from the read position: bytes 0x20..0x7f and `\n` print through 0x108376, another
/// control byte prints "Try to print out a control char" and stops; `ef` starts a keyword block
/// (0x1149d6) unless an answer was already given, in which case the block is skipped to `ee`;
/// other bytes 0x80..0xee are ops (0x11404c, `npc` = the NPC talked to). Stops before 0, `ee`
/// and every byte from `f0` (input ops, `ff`), and after an op that set "bye".
pub(crate) fn run_text<'a>(cx: &'a Cx<'a>, npc: i16) -> Pin<Box<dyn Future<Output = ()> + 'a>> {
    Box::pin(async move {
        let mut b = cx.cv(|c| c.next());
        loop {
            if b == 0 || b == 0xee || b >= 0xf0 || cx.cv(|c| c.fault) {
                break;
            }
            if b < 0x80 {
                if b < 0x20 && b != b'\n' {
                    let m = cx.msg(Msg::ControlChar);
                    seams::print(cx, &m).await;
                    break;
                }
                seams::put_char(cx, b).await;
            } else if b == 0xef {
                if !cx.cv(|c| c.answered) {
                    keywords(cx, npc).await;
                } else {
                    cx.cv(|c| while c.next() != 0xee && !c.fault {});
                }
            } else {
                exec(cx, b, npc).await;
            }
            if cx.cv(|c| c.bye) {
                break;
            }
            checkpoint(cx).await;
            b = cx.cv(|c| c.next());
        }
        cx.cv(|c| c.advance(-1));
    })
}

/// Bounded-work checkpoint (spec 03 §1): a script that jumps without waiting for input yields
/// one VBlank every [`YIELD_EVERY`] steps. Simulation order is unchanged.
pub(crate) async fn checkpoint(cx: &Cx<'_>) {
    let due = cx.cv(|c| {
        c.steps += 1;
        c.steps >= YIELD_EVERY
    });
    if due {
        cx.cv(|c| c.steps = 0);
        cx.io.vblank().await;
    }
}

/// Original: 0x1149d6 (keyword block: match the input, run the answer)
///
/// Empty input counts as "bye". Keywords are read up to `,` or `f6`; the first one that matches
/// a word of the input (0x1128d0, full keyword length), or that starts with `*`, skips to `f6`,
/// prints a newline, marks the input answered and runs the answer text. Without a match the
/// rest of the block is skipped up to `ef` / `ff` (left to be read again) or past `ee`.
async fn keywords(cx: &Cx<'_>, npc: i16) {
    let (delims, newline) = (cx.msg(Msg::Delimiters), cx.msg(Msg::Newline));
    let bye = cx.msg(Msg::Bye);
    cx.cv(|c| {
        if c.input.is_empty() {
            c.input = bye;
        }
    });
    let mut more = true;
    while more {
        let (kw, matched) = cx.cv(|c| {
            let mut kw = Vec::new();
            let last = loop {
                let b = c.next();
                if b == b',' || b == 0xf6 || c.fault {
                    break b;
                }
                kw.push(b);
            };
            more = last == b',';
            if !more {
                c.advance(-1);
            }
            let len = kw.iter().position(|&b| b == 0).unwrap_or(kw.len());
            let matched = match_keyword(&kw, &c.input, len, &delims);
            (kw, matched)
        });
        if matched || kw.first() == Some(&b'*') {
            cx.cv(|c| while c.next() != 0xf6 && !c.fault {});
            seams::print(cx, &newline).await;
            cx.cv(|c| c.answered = true);
            run_text(cx, npc).await;
            return;
        }
        if cx.cv(|c| c.fault) {
            return;
        }
    }
    cx.cv(|c| {
        loop {
            let b = c.next();
            c.advance(operand_len(b));
            if b == 0xef || b == 0xff {
                c.advance(-1);
                break;
            }
            if b == 0xee || c.fault {
                break;
            }
        }
    });
}

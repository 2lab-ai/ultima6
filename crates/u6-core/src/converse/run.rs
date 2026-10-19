//! The text runner 0x1148f0, keyword blocks 0x1149d6 and the keyword test 0x1128d0.

use std::future::Future;
use std::pin::Pin;

use super::eval::YIELD_EVERY;
use super::exec::exec;
use super::seams;
use super::{Cx, Msg};
use crate::speech::Kind;

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
/// control byte prints the line 0x1129e4 and stops; `ef` starts a keyword block
/// (0x1149d6) unless an answer was already given, in which case the block is skipped to `ee`;
/// other bytes 0x80..0xee are ops (0x11404c, `npc` = the NPC talked to). Stops before 0, `ee`
/// and every byte from `f0` (input ops, `ff`), and after an op that set "bye".
///
/// Port (spec 08 Design 1): a text run (the text bytes between two bytes that are not text) is a
/// unit of text, and so is what one op or keyword block puts (a table string put a byte at a
/// time): where it ends, what the text layer still holds is let out and a brace `@{…}` it opened
/// and left open is closed ([`seams::end_unit`]), so neither joins or colours the next.
///
/// Port (spec 09 Design 1): `kind` is what the caller runs (the look text, or the NPC's words);
/// the text bytes are put with the speech tap's flag up for [`Kind::Speech`], and the keyword
/// blocks and ops it runs take the same kind.
///
/// Port (spec 08, slice K1): where a run starts (the first text byte here, or after a byte that
/// is not text) the installed locale pack may have it: its translation is put instead
/// ([`translated`]), and the byte after the run starts the next one (the next part of a run a
/// path enters in the middle). Ops and the bytes that stop the runner are read as before.
pub(crate) fn run_text<'a>(
    cx: &'a Cx<'a>,
    npc: i16,
    kind: Kind,
) -> Pin<Box<dyn Future<Output = ()> + 'a>> {
    Box::pin(async move {
        // Every unit here begins after this; each ends before the next begins.
        let opened = seams::braces(cx);
        let mut starts = true;
        let mut b = cx.cv(|c| c.next());
        loop {
            #[cfg(test)]
            super::disasm::trace::run_byte(cx, b);
            if b == 0 || b == 0xee || b >= 0xf0 || cx.cv(|c| c.fault) {
                break;
            }
            if b < 0x80 {
                if b < 0x20 && b != b'\n' {
                    seams::end_unit(cx, opened).await;
                    let m = cx.msg(Msg::ControlChar);
                    seams::print(cx, &m).await;
                    break;
                }
                if !(starts && translated(cx, kind).await) {
                    starts = false;
                    seams::put_speech(cx, kind, b).await;
                }
            } else {
                starts = true;
                // The text run before it ends here, and what it puts ends with it.
                seams::end_unit(cx, opened).await;
                if b == 0xef {
                    if !cx.cv(|c| c.answered) {
                        keywords(cx, npc, kind).await;
                    } else {
                        cx.cv(|c| while c.next() != 0xee && !c.fault {});
                    }
                } else {
                    exec(cx, b, npc, kind).await;
                }
                seams::end_unit(cx, opened).await;
            }
            if cx.cv(|c| c.bye) {
                break;
            }
            checkpoint(cx).await;
            b = cx.cv(|c| c.next());
        }
        seams::end_unit(cx, opened).await;
        cx.cv(|c| c.advance(-1));
    })
}

/// Port-only (spec 08, slice K1): the run that starts at the byte just read, from the installed
/// locale pack ([`crate::locale::run_at`]). Its translation is put, with the pack's grammar
/// ([`crate::locale::Pack::language`]) for those puts only, instead of the run's bytes, which are
/// read past one by one, each with the checkpoint an English byte takes (the last one is the
/// caller's): a byte of the translation is put before each source byte's checkpoint while both
/// last, so the identity pack puts and yields exactly as English does. False, and nothing done,
/// when the pack does not have the run.
async fn translated(cx: &Cx<'_>, kind: Kind) -> bool {
    let at = cx.cv(|c| c.pos()) - 1;
    let Some((pack, key)) = cx.st(|s| crate::locale::run_at(s, at)) else {
        return false;
    };
    let Some(run) = pack.run(key) else {
        return false;
    };
    let len = run.len as usize;
    let was = cx.st(|s| std::mem::replace(&mut s.ui.text.language, pack.language));
    for i in 0..len.max(run.text.len()) {
        if (1..len).contains(&i) {
            let _b = cx.cv(|c| c.next());
            #[cfg(test)]
            super::disasm::trace::run_byte(cx, _b);
        }
        if let Some(&t) = run.text.get(i) {
            seams::put_speech(cx, kind, t).await;
        }
        if i + 1 < len {
            checkpoint(cx).await;
        }
    }
    cx.st(|s| s.ui.text.language = was);
    true
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
async fn keywords(cx: &Cx<'_>, npc: i16, kind: Kind) {
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
            run_text(cx, npc, kind).await;
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

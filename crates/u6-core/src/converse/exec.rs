//! The op executor 0x11404c.

use super::eval::{SCRIPT_BASE, eval, table_element};
use super::party::{actor_name, ix, on_screen, resolve_npc, set_status, status, wake_type};
use super::run::operand_len;
use super::seams;
use super::vars::{self, SLOT_Y, StrRef};
use super::{Cx, Msg};
use crate::speech::Kind;

/// Skips to just after one of `stops`, stepping over jump / literal operands (0x11404c ops
/// 0xa1, 0xa3).
fn skip_to(cx: &Cx<'_>, stops: &[u8]) {
    cx.cv(|c| {
        loop {
            let b = c.next();
            if stops.contains(&b) || c.fault {
                break;
            }
            c.advance(operand_len(b));
        }
    });
}

/// `eval()` then 0x112b0a: an actor operand.
async fn actor(cx: &Cx<'_>) -> i16 {
    let v = eval(cx).await as i16;
    cx.st(|s| resolve_npc(s, v))
}

/// Original: 0x11404c (converse op executor)
///
/// Runs op `op` (already read) with `npc` = the NPC talked to. Result 0, 1 for a malformed
/// operand or an `a1` whose condition is -1, -1 (and the line 0x1129d2) for an unknown op;
/// 0x1148f0 ignores it. Operand expressions are read with 0x1130a2, actor operands then go
/// through 0x112b0a.
///
/// Port (spec 09 Design 1): `kind` is what the text runner runs; op `0xb5` prints as speech of
/// that kind, and op `0xbf` makes its actor the speaker of what follows.
pub(crate) async fn exec(cx: &Cx<'_>, op: u8, npc: i16, kind: Kind) -> i16 {
    match op {
        0x9c => {
            let a = actor(cx).await;
            cx.st(|s| {
                let flags = ix(a)
                    .and_then(|i| s.actors.attrs.movement_flags.get(i))
                    .copied();
                seams::mount(s, a, flags.unwrap_or(0) & 7);
            });
        }
        0x9e => sleep_until_five(cx).await,
        0xa1 => {
            let v = eval(cx).await as i16;
            if v == -1 {
                return 1;
            }
            if v == 0 {
                cx.cv(|c| {
                    loop {
                        let b = c.next();
                        if b == 0xa3 || b == 0xa2 || c.fault {
                            break;
                        }
                        c.advance(operand_len(b));
                    }
                });
            }
        }
        0xa2 | 0xa7 => {}
        0xa3 => skip_to(cx, &[0xa2]),
        0xa4 | 0xa5 => {
            let a = actor(cx).await;
            let bit = eval(cx).await as u16;
            cx.st(|s| {
                if let Some(f) = ix(a).and_then(|i| s.actors.attrs.talk_flags.get_mut(i)) {
                    let mask = 1u16.checked_shl(u32::from(bit & 0x3f)).unwrap_or(0) as u8;
                    *f = if op == 0xa4 { *f | mask } else { *f & !mask };
                }
            });
        }
        0xa6 => return assign(cx).await,
        0xb0 => cx.cv(|c| {
            let p = c.pos();
            let target = i32::from(c.word_at(p) as i16);
            c.set_pos(target - SCRIPT_BASE);
        }),
        0xb5 => return print_op(cx, kind).await,
        0xb6 => cx.cv(|c| c.bye = true),
        0xb9 | 0xba => {
            let a = actor(cx).await;
            let ty = eval(cx).await as i16;
            let quality = eval(cx).await as u16;
            let quantity = eval(cx).await as u16;
            let packed = (quality << 8) | quantity;
            cx.st(|s| {
                if op == 0xba {
                    seams::remove_object(s, a, ty, packed);
                } else {
                    seams::add_object(s, a, ty, packed);
                }
            });
        }
        0xbe => {
            let v = eval(cx).await as i16;
            let (member, other) = cx.st(|s| {
                if v == 1 {
                    (Some(u16::from(s.actors.party.leader_index)), 0)
                } else if status(s, npc) & 0x80 != 0 {
                    let i = (0..16)
                        .find(|&i| s.actors.party.members.get(i).copied() == Some(npc as u8));
                    (i.map(|i| i as u16), 0)
                } else {
                    (None, npc)
                }
            });
            seams::inventory_panel(cx, npc, member, other).await;
        }
        0xbf => {
            let a = actor(cx).await;
            cx.st(|s| crate::speech::portrait(s, a));
            seams::portrait_panel(cx, a).await;
        }
        0xc4 => {
            let v = eval(cx).await as u16;
            cx.st(|s| {
                let k = (u16::from(s.actors.karma).wrapping_add(v)) as u8;
                s.actors.karma = k.min(99);
            });
        }
        0xc5 => {
            let v = eval(cx).await as i16;
            cx.st(|s| {
                let k = i16::from(s.actors.karma);
                s.actors.karma = if k > v { (k - v) as u8 } else { 0 };
            });
        }
        0xc8 => {
            let a = actor(cx).await;
            let dest = eval(cx).await as i16;
            cx.st(|s| seams::move_into(s, a, dest, 0x10));
        }
        0xc9 => {
            let ty = eval(cx).await as i16;
            let quality = eval(cx).await as i16;
            let from = actor(cx).await;
            let to = actor(cx).await;
            cx.st(|s| {
                let obj = seams::find_object(s, from, ty, quality);
                seams::move_into(s, obj, to, 0x10);
            });
        }
        0xcb => seams::wait_key(cx).await,
        0xcd => {
            let a = actor(cx).await;
            let wt = eval(cx).await as i16;
            cx.st(|s| {
                if let Some(w) = ix(a).and_then(|i| s.actors.worktype.get_mut(i)) {
                    *w = wt as u8;
                }
                if wt == 0x91 {
                    set_status(s, a, status(s, a) | 4);
                    seams::worktype_91(s, a);
                }
                if wt != 0x95 {
                    wake_type(s, a);
                }
            });
        }
        0xd0 => {
            let n = eval(cx).await as i16;
            let mut i: i16 = 0;
            while i < n.wrapping_mul(20) {
                seams::refresh_view(cx).await;
                i += 1;
            }
        }
        0xd6 => {
            let a = actor(cx).await;
            let body = cx.st(|s| {
                let mut o = seams::object_link(s, a);
                while o >= 0 && super::party::obj_type(s, o) != 0x153 {
                    o = seams::object_link(s, o);
                }
                o
            });
            Box::pin(crate::magic::resurrect_and_rejoin(cx.state, cx.io, body)).await;
        }
        0xd8 | 0xdf => {
            let a = actor(cx).await;
            let r = if op == 0xdf && a == 1 {
                StrRef::Text(Msg::You)
            } else {
                actor_name(cx, a).await
            };
            cx.st(|s| vars::set_slot(s, SLOT_Y, r));
        }
        0xd9 => {
            let a = actor(cx).await;
            cx.st(|s| {
                let hp = seams::max_hp(s, a) as u8;
                if let Some(h) = ix(a).and_then(|i| s.actors.attrs.hp.get_mut(i)) {
                    *h = hp;
                }
            });
        }
        0xdb => {
            let a = actor(cx).await;
            cx.st(|s| set_status(s, a, status(s, a) & !8));
        }
        _ => {
            let m = cx.msg(Msg::UnknownCommand);
            seams::print(cx, &m).await;
            return -1;
        }
    }
    0
}

/// Op 0xb5: print a string. `d2 <table> <index expr>` prints string `index` of a table (bytes
/// through 0x108376; the read position comes back after the expression), `d5` prints nothing,
/// any other byte is followed by `slot b3`: string slot `slot` through printf, then one byte
/// (the expression's `a7`) is skipped. Both printing forms are speech of `kind` (spec 09).
///
/// Port (spec 08, slice K2): a table string the locale pack has ([`crate::locale::table_at`],
/// the table reader's hook) is put as its translation, in the pack's grammar, while its source
/// bytes are read past one by one as English reads them (a byte of the translation put after
/// each source byte read, while both last, so the identity pack reads and puts as English
/// does). The slot form prints what the slot shows ([`vars::shown_var`]: a copied table
/// string's translation, a label's), as a format no executable string is looked up in.
async fn print_op(cx: &Cx<'_>, kind: Kind) -> i16 {
    let form = cx.cv(|c| c.next());
    if form == 0xd2 {
        let back = table_element(cx, true).await;
        cx.cv(|c| c.saved_pc = back);
        #[cfg(test)]
        super::disasm::trace::print_start(cx);
        let read = |cx: &Cx<'_>| {
            let b = cx.cv(|c| c.next());
            #[cfg(test)]
            {
                use super::disasm::trace::{Read, table_byte};
                table_byte(cx.cv(|c| c.pos()) - 1, b, Read::Print);
            }
            b
        };
        let hit = cx.st(|s| crate::locale::table_at(s, s.converse.pos(), None));
        if let Some((len, language, text)) = hit {
            let was = cx.st(|s| std::mem::replace(&mut s.ui.text.language, language));
            for i in 0..len.max(text.len()) {
                if i < len {
                    read(cx);
                }
                if let Some(&t) = text.get(i) {
                    seams::put_speech(cx, kind, t).await;
                }
            }
            // The NUL that ends the source.
            read(cx);
            cx.st(|s| s.ui.text.language = was);
        } else {
            loop {
                let b = read(cx);
                if b == 0 || cx.cv(|c| c.fault) {
                    break;
                }
                seams::put_speech(cx, kind, b).await;
            }
        }
        cx.cv(|c| c.set_pos(c.saved_pc));
        return 0;
    }
    if form == 0xd5 {
        return 0;
    }
    let (slot, tag) = cx.cv(|c| (c.next(), c.next()));
    if tag != 0xb3 {
        return 1;
    }
    let (language, text) = cx.st(|s| vars::shown_ref(s, vars::slot(s, slot), false));
    let was = language.map(|l| cx.st(|s| std::mem::replace(&mut s.ui.text.language, l)));
    seams::print_slot_speech(cx, kind, &text).await;
    if let Some(was) = was {
        cx.st(|s| s.ui.text.language = was);
    }
    cx.cv(|c| c.advance(1));
    0
}

/// Op 0xa6: assignment `a6 <target> a8 <value> a7`. Targets: `d2 <table> <index>` (a word in
/// the script's table is overwritten), `var b2` (word variable), `var <other>` (string slot;
/// the value is then `d2 <table> <index>` = a table string, or `src b3` = another slot).
///
/// Port (spec 08, slice K2): the slot takes the table string's English bytes as in English (what
/// logic reads); when the locale pack has the string ([`crate::locale::table_at`], the table
/// reader's hook) and its translation fits the place the slot points at
/// ([`vars::capacity`]), the place shows the translation ([`vars::set_shown`]); one that does not
/// fit stays English (a miss). A copy into a place whose size the port does not know (not one of
/// the ten buffers) is not looked up. A copy from another slot shows what that slot shows, on
/// the same terms: where its English is written and the translation fits ([`vars::copy`]).
async fn assign(cx: &Cx<'_>) -> i16 {
    let first = cx.cv(|c| c.next());
    let (var, kind, at) = if first == 0xd2 {
        let back = table_element(cx, false).await;
        cx.cv(|c| {
            let at = c.pos();
            c.set_pos(back);
            c.saved_pc = back;
            (0, 0, at)
        })
    } else {
        (first, cx.cv(|c| c.next()), 0)
    };
    if cx.cv(|c| c.next()) != 0xa8 {
        return 1;
    }
    if kind == 0 {
        let v = eval(cx).await as u16;
        cx.cv(|c| {
            c.set_byte(at, v as u8);
            c.set_byte(at + 1, (v >> 8) as u8);
        });
        #[cfg(test)]
        super::disasm::trace::table_write(at, v);
        return 0;
    }
    if kind == 0xb2 {
        let v = eval(cx).await as u16;
        cx.st(|s| vars::set_int(s, usize::from(var), v));
        return 0;
    }
    let dst = cx.st(|s| vars::slot(s, var));
    if cx.cv(|c| c.next()) == 0xd2 {
        let back = table_element(cx, true).await;
        cx.st(|s| {
            s.converse.saved_pc = back;
            // A place the port drops writes to (code text, address 0) shows nothing either.
            let hit = (!matches!(dst, StrRef::Null | StrRef::Text(_)))
                .then(|| crate::locale::table_at(s, s.converse.pos(), Some(vars::capacity(dst)?)))
                .flatten();
            let mut text = Vec::new();
            loop {
                let b = s.converse.next();
                #[cfg(test)]
                super::disasm::trace::table_byte(
                    s.converse.pos() - 1,
                    b,
                    super::disasm::trace::Read::Copy,
                );
                if b == 0 || s.converse.fault {
                    break;
                }
                text.push(b);
            }
            vars::write(s, dst, &text);
            if let Some((_, language, shown)) = hit {
                vars::set_shown(s, dst, language, shown);
            }
            s.converse.set_pos(s.converse.saved_pc);
        });
        return 0;
    }
    let (src, tag) = cx.cv(|c| {
        c.advance(-1);
        (c.next(), c.next())
    });
    if tag != 0xb3 {
        return 1;
    }
    cx.st(|s| {
        let from = vars::slot(s, src);
        vars::copy(s, from, dst);
    });
    0
}

/// Op 0x9e: heal the party members on screen, redraw the panels, then advance the clock in
/// hours until 5 o'clock (a full day when it is 5 already).
async fn sleep_until_five(cx: &Cx<'_>) {
    let size = cx.st(|s| s.actors.party.size);
    for i in 0..i16::from(size) {
        let here = cx.st(|s| {
            let m = s.actors.party.members.get(i as usize).copied().unwrap_or(0);
            on_screen(s, i16::from(m))
        });
        if here {
            seams::heal_member(cx, i).await;
        }
    }
    cx.st(|s| {
        seams::refresh_panels(s);
        seams::resting(s, true);
    });
    if cx.st(|s| s.actors.calendar.hour) == 5 {
        seams::clock_tick(cx, 60).await;
    }
    while cx.st(|s| s.actors.calendar.hour) != 5 {
        seams::clock_tick(cx, 60).await;
    }
    cx.st(|s| seams::resting(s, false));
    seams::refresh_view(cx).await;
}

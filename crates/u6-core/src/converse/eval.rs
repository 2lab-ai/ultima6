//! The expression evaluator 0x1130a2 and its table helper 0x113f8a.

use super::party::{
    ix, join_party, leave_party, member_with_object, obj_type, on_screen, resolve_npc, status,
};
use super::run::match_keyword;
use super::seams;
use super::vars;
use super::{Cx, Msg};
use crate::state::State;

/// Script addresses are relative to A4+0x13fa, a longword nothing in the image writes (0).
pub(crate) const SCRIPT_BASE: i32 = 0;
/// Slots of the evaluator's stack frame array (-0x28(A5), 10 longs).
const STACK: usize = 10;
/// Value 0x1130a2 pushes for "not found" (0xc0, 0xc3, 0xc7).
pub(crate) const NOT_FOUND: u32 = 0x8001;
/// String-slot pointers on the stack: the port pushes this tag | slot instead of an address.
pub(crate) const STR_TAG: u32 = 0x8000_0000;
/// Ops between forced yields of a running script (bounded work, spec 03 §1).
pub(crate) const YIELD_EVERY: u32 = 4096;

/// The evaluator's ten-long stack and its index (a signed word in D6). Reads outside the array
/// give 0 and writes there are dropped (the original would hit its other locals).
struct Stack {
    v: [u32; STACK],
    sp: i16,
}

impl Stack {
    fn get(&self, at: i16) -> u32 {
        usize::try_from(at)
            .ok()
            .and_then(|i| self.v.get(i))
            .copied()
            .unwrap_or(0)
    }
    fn set(&mut self, at: i16, x: u32) {
        if let Some(s) = usize::try_from(at).ok().and_then(|i| self.v.get_mut(i)) {
            *s = x;
        }
    }
    /// The operand `back` slots below the op (1 = top value).
    fn arg(&self, back: i16) -> u32 {
        self.get(self.sp - back)
    }
    /// Stores `x` `back` slots below the op and drops the op and everything above `x`.
    fn reduce(&mut self, back: i16, x: u32) {
        self.set(self.sp - back, x);
        self.sp -= back;
    }
}

fn w(x: u32) -> i16 {
    x as u16 as i16
}

fn sext(x: i16) -> u32 {
    i32::from(x) as u32
}

/// Text of a value used as a string: a tagged string slot, else empty (the original would read
/// memory at that address).
fn str_of(s: &State, v: u32) -> Vec<u8> {
    if v & STR_TAG != 0 {
        vars::resolve(s, vars::slot(s, (v & 0xff) as u8))
    } else {
        Vec::new()
    }
}

/// Original: 0x1130a2 (converse expression evaluator)
///
/// Reads operands and ops up to `a7` or `a8` (consumed) and returns the bottom stack slot.
/// `d2`/`d3`/`d4` prefix a 32/8/16-bit little-endian literal; every other byte is pushed as
/// itself and, if it names an op, the op replaces itself and its operands with the result.
/// Comparisons are unsigned 32-bit; `86` compares strings without case once a `b3` string slot
/// was read in this expression. More than one value left prints the error line 0x1129be.
pub(crate) async fn eval(cx: &Cx<'_>) -> u32 {
    let mut st = Stack {
        v: [0; STACK],
        sp: 0,
    };
    let mut strings = false;
    let mut op = cx.cv(|c| c.next());
    while op != 0xa7 && op != 0xa8 && !cx.cv(|c| c.fault) {
        let literal = cx.cv(|c| match op {
            0xd2 => {
                let p = c.pos();
                c.advance(4);
                Some(c.long_at(p))
            }
            0xd3 => Some(u32::from(c.next())),
            0xd4 => {
                let p = c.pos();
                c.advance(2);
                Some(u32::from(c.word_at(p)))
            }
            _ => None,
        });
        match literal {
            Some(v) => st.set(st.sp, v),
            None => {
                st.set(st.sp, u32::from(op));
                apply(cx, &mut st, op, &mut strings).await;
            }
        }
        st.sp += 1;
        op = cx.cv(|c| c.next());
    }
    if st.sp > 1 {
        let m = cx.msg(Msg::ErrorOccured);
        seams::print(cx, &m).await;
    }
    st.get(0)
}

/// One op of 0x1130a2 on the stack (op value at `sp`).
async fn apply(cx: &Cx<'_>, st: &mut Stack, op: u8, strings: &mut bool) {
    let (a, b) = (st.arg(2), st.arg(1));
    #[cfg(test)]
    if matches!(op, 0xb4 | 0xb7 | 0xc3) {
        let at = st.sp - 2;
        super::disasm::trace::table_address(at >= 0 && (at as usize) < STACK);
    }
    let bool32 = |c: bool| u32::from(c);
    match op {
        0x81 => st.reduce(2, bool32(a > b)),
        0x82 => st.reduce(2, bool32(a >= b)),
        0x83 => st.reduce(2, bool32(a < b)),
        0x84 => st.reduce(2, bool32(a <= b)),
        0x85 => st.reduce(2, bool32(a != b)),
        0x86 => {
            let eq = if *strings {
                cx.st(|s| stricmp(&str_of(s, b), &str_of(s, a)) == 0)
            } else {
                a == b
            };
            st.reduce(2, bool32(eq));
        }
        0x90 => st.reduce(2, a.wrapping_add(b)),
        0x91 => st.reduce(2, a.wrapping_sub(b)),
        0x92 => st.reduce(2, a.wrapping_mul(b)),
        0x93 => {
            if b == 0 {
                let m = cx.msg(Msg::DivideError);
                seams::print(cx, &m).await;
                st.sp -= 2;
            } else {
                st.reduce(2, divide(a, b));
            }
        }
        0x94 => st.reduce(2, bool32(a != 0 || b != 0)),
        0x95 => st.reduce(2, bool32(a != 0 && b != 0)),
        0x9a => {
            let v = cx.st(|s| {
                let n = resolve_npc(s, w(b));
                let (w1, w2) = seams::carried_weights(s, n);
                let cap = (i16::from(attr(&s.actors.attrs.strength, n))).wrapping_mul(200);
                let load = w1.wrapping_add(w2).wrapping_mul(10);
                if cap > load {
                    cap.wrapping_sub(load)
                } else {
                    0
                }
            });
            st.reduce(1, sext(v));
        }
        0x9b => {
            let v = cx.st(|s| {
                let ty = w(a);
                let weight = s.tables.tileflags.weight.get(ix(ty).unwrap_or(usize::MAX));
                let weight = i16::from(weight.copied().unwrap_or(0));
                let n = w(b).wrapping_mul(weight);
                if seams::counted_type(s, ty) {
                    n
                } else {
                    n.wrapping_mul(10)
                }
            });
            st.reduce(2, sext(v));
        }
        0x9d => {
            let v = cx.st(|s| obj_type(s, resolve_npc(s, w(b))) == 0x1af);
            st.reduce(1, bool32(v));
        }
        0x9f => {
            let c3 = st.arg(3);
            let v = cx.st(|s| seams::has_object(s, resolve_npc(s, w(c3)), w(a), w(b)) != -1);
            st.reduce(3, bool32(v));
        }
        0xa0 => {
            let v = cx.st(|s| s.rng.range(u16::from(a as u8), u16::from(b as u8)));
            st.reduce(2, sext(v as i16));
        }
        0xab => {
            let v = cx.st(|s| {
                let n = resolve_npc(s, w(a));
                u16::from(attr(&s.actors.attrs.talk_flags, n))
                    .checked_shr(b & 0x3f)
                    .unwrap_or(0)
                    & 1
            });
            st.reduce(2, u32::from(v));
        }
        0xb2 => {
            let v = cx.st(|s| vars::int_var(s, b as u8));
            st.reduce(1, u32::from(v));
        }
        0xb3 => {
            st.reduce(1, STR_TAG | (b & 0xff));
            *strings = true;
        }
        0xb4 => {
            let v = cx.cv(|c| {
                let at = (a as i32).wrapping_add(i32::from(b as u8) * 2) - SCRIPT_BASE;
                let v = c.word_at(at as i16 as i32);
                #[cfg(test)]
                super::disasm::trace::table_word(at as i16 as i32, v);
                c.saved_pc = c.pos();
                v as i16
            });
            st.reduce(2, sext(v));
        }
        0xb7 => {
            let v = index_of_string(cx, a, b as u8);
            st.reduce(2, sext(v));
        }
        0xbb => {
            let v = cx.st(|s| seams::object_count(s, resolve_npc(s, w(a)), w(b)));
            st.reduce(2, sext(v));
        }
        0xbc | 0xbd => st.reduce(2, 100),
        0xc0 => {
            let m = cx.msg(Msg::SelectItem);
            seams::print(cx, &m).await;
            let (obj, other) = seams::select_item(cx).await;
            let v = if obj == -1 || other == -1 {
                NOT_FOUND
            } else {
                sext(obj)
            };
            st.set(st.sp, v);
        }
        0xc1 => {
            let v = cx.st(|s| {
                let loc = ix(w(b)).and_then(|i| s.objects.location.get(i)).copied();
                let packed = loc.unwrap_or_default().to_packed();
                let p = s.converse.pos() + 1;
                let operand = i16::from(s.converse.byte_at(p));
                let r = resolve_npc(s, operand);
                s.converse.set_byte(p, r as u8);
                i16::from_le_bytes([packed[0], packed[1]])
            });
            st.reduce(1, sext(v));
        }
        0xc2 => {
            let v = cx.st(|s| obj_type(s, b as u16 as i16));
            st.reduce(1, u32::from(v));
        }
        0xc3 => {
            let v = index_of_word(cx, a, b as u8);
            st.reduce(2, v);
        }
        0xc6 => {
            let v = cx.st(|s| {
                let n = resolve_npc(s, w(b));
                let f = status(s, n);
                f & 0x80 != 0 && on_screen(s, n) && f & 0x16 == 0
            });
            st.reduce(1, bool32(v));
        }
        0xc7 => {
            let v = cx.st(|s| member_with_object(s, w(a), w(b)));
            st.reduce(2, if v == -1 { NOT_FOUND } else { sext(v) });
        }
        0xca => {
            let n = cx.st(|s| resolve_npc(s, w(b)));
            let v = join_party(cx, n).await;
            st.reduce(1, sext(v));
        }
        0xcc => {
            let v = cx.st(|s| {
                let n = resolve_npc(s, w(b));
                leave_party(s, n)
            });
            st.reduce(1, sext(v));
        }
        0xd7 => {
            let v = cx.st(|s| on_screen(s, resolve_npc(s, w(b))));
            st.reduce(1, bool32(v));
        }
        0xda => {
            let v = cx.st(|s| {
                let n = resolve_npc(s, w(b));
                i16::from(attr(&s.actors.attrs.hp, n)) < seams::max_hp(s, n)
            });
            st.reduce(1, bool32(v));
        }
        0xdc => {
            let v = cx.st(|s| status(s, resolve_npc(s, w(b))) & 8);
            st.reduce(1, u32::from(v));
        }
        0xdd => {
            let v = cx.st(|s| party_member(s, w(a), w(b)));
            st.reduce(2, u32::from(v));
        }
        0xe0 => {
            let v = cx.st(|s| {
                let n = resolve_npc(s, w(a));
                let e = ix(n).and_then(|i| s.actors.attrs.exp.get_mut(i));
                e.map_or(0, |e| {
                    *e = e.wrapping_add(b as u16);
                    if *e > 9999 {
                        *e = 9999;
                    }
                    *e
                })
            });
            st.reduce(2, u32::from(v));
        }
        0xe1 => {
            let v = cx.st(|s| {
                let n = resolve_npc(s, w(a));
                if let Some(l) = ix(n).and_then(|i| s.actors.attrs.level.get_mut(i)) {
                    *l = l.wrapping_add(b as u8);
                }
                seams::level(s, n)
            });
            st.reduce(2, sext(v));
        }
        0xe2..=0xe4 => {
            let v = cx.st(|s| {
                let n = resolve_npc(s, w(a));
                let attrs = &mut s.actors.attrs;
                let table = match op {
                    0xe2 => &mut attrs.strength,
                    0xe3 => &mut attrs.intelligence,
                    _ => &mut attrs.dexterity,
                };
                ix(n).and_then(|i| table.get_mut(i)).map_or(0, |x| {
                    *x = x.wrapping_add(b as u8);
                    if *x > 30 {
                        *x = 30;
                    }
                    *x
                })
            });
            st.reduce(2, u32::from(v));
        }
        _ => {}
    }
}

fn attr(v: &[u8], a: i16) -> u8 {
    ix(a).and_then(|i| v.get(i)).copied().unwrap_or(0)
}

/// Op 0xdd: party member `index`; with `all` = 0 counted among the members on screen only.
fn party_member(s: &State, index: i16, all: i16) -> u8 {
    let p = &s.actors.party;
    if index >= i16::from(p.size) {
        return 0;
    }
    let members = |i: usize| p.members.get(i).copied().unwrap_or(0);
    if all != 0 {
        return members(index as usize);
    }
    let mut list: Vec<u8> = (0..usize::from(p.size))
        .filter(|&i| on_screen(s, i16::from(members(i))))
        .map(|i| i as u8)
        .collect();
    list.resize(usize::from(p.size).max(list.len()), 0xff);
    match list.get(index as usize) {
        Some(&0xff) | None => 0,
        Some(&i) => members(usize::from(i)),
    }
}

/// Op 0xb7: index of the first entry of the string table at script address `table` (NUL-ended
/// strings up to `b8`) whose first min(len, 4) characters match a word of string slot `slot`;
/// past the end: the entry count. The read position comes back.
fn index_of_string(cx: &Cx<'_>, table: u32, slot: u8) -> i16 {
    cx.st(|s| {
        let hay = vars::string_var(s, slot);
        let delims = s
            .converse
            .text
            .as_ref()
            .map(|t| t.get(Msg::Delimiters).to_vec());
        let delims = delims.unwrap_or_default();
        let c = &mut s.converse;
        c.saved_pc = c.pos();
        c.set_pos(table as i32 - SCRIPT_BASE);
        let mut n: i16 = 0;
        let mut end = false;
        loop {
            if c.fault {
                break;
            }
            if c.next() == 0xb8 {
                #[cfg(test)]
                super::disasm::trace::table_end(c.pos() - 1);
                end = true;
                break;
            }
            c.advance(-1);
            n += 1;
            let mut entry = Vec::new();
            loop {
                let b = c.next();
                #[cfg(test)]
                super::disasm::trace::table_byte(
                    c.pos() - 1,
                    b,
                    super::disasm::trace::Read::Search,
                );
                if b == 0 || c.fault {
                    break;
                }
                entry.push(b);
            }
            let len = entry.len().min(4);
            if match_keyword(&entry, &hay, len, &delims) {
                break;
            }
        }
        c.set_pos(c.saved_pc);
        if end { n } else { n - 1 }
    })
}

/// Op 0xc3: index of word variable `var`'s value in the word table at script address `table`
/// (ends at a word whose low byte is `b8`, then [`NOT_FOUND`]). The read position comes back.
fn index_of_word(cx: &Cx<'_>, table: u32, var: u8) -> u32 {
    cx.st(|s| {
        let key = vars::int_var(s, var);
        let c = &mut s.converse;
        c.saved_pc = c.pos();
        c.set_pos(table as i32 - SCRIPT_BASE);
        let mut n: i16 = 0;
        let found = loop {
            let p = c.pos();
            if c.byte_at(p) == 0xb8 || c.fault {
                #[cfg(test)]
                if !c.fault {
                    super::disasm::trace::table_end(p);
                }
                break false;
            }
            n += 1;
            let w = c.word_at(p);
            #[cfg(test)]
            super::disasm::trace::table_word(p, w);
            if w == key {
                break true;
            }
            c.advance(2);
        };
        c.set_pos(c.saved_pc);
        if found { sext(n - 1) } else { NOT_FOUND }
    })
}

/// Original: 0x11519e (32-bit division: a jump to the runtime's unsigned divide 0x101c9a)
pub(crate) fn divide(a: u32, b: u32) -> u32 {
    a / b
}

/// Case-insensitive compare of the runtime's 0x101724 (`a`..`z` folded to upper case).
pub(crate) fn stricmp(a: &[u8], b: &[u8]) -> i32 {
    let up = |c: u8| if c.is_ascii_lowercase() { c - 0x20 } else { c };
    let mut i = 0;
    loop {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        let d = i32::from(up(x)) - i32::from(up(y));
        if d != 0 || y == 0 {
            return d;
        }
        i += 1;
    }
}

/// Original: 0x113f8a (read a table address, then select an element)
///
/// Reads a 32-bit table address. Followed by `b1`: the read position moves to the table and the
/// position after `b1` is returned. Otherwise an index expression follows (0x1130a2); the read
/// position moves to element `index` of the table (strings: skip `index` NUL-ended strings;
/// words: `index` × 2 bytes) and the position after the expression is returned, minus 1 for
/// words (so an `a8` that ended the index is read again by the caller).
pub(crate) async fn table_element(cx: &Cx<'_>, strings: bool) -> i32 {
    let (addr, ends) = cx.cv(|c| {
        let p = c.pos();
        let addr = c.long_at(p) as i32;
        c.advance(4);
        let b1 = c.next() == 0xb1;
        if !b1 {
            c.advance(-1);
        }
        (addr, b1)
    });
    if ends {
        return cx.cv(|c| {
            let back = c.pos();
            c.set_pos(addr - SCRIPT_BASE);
            back
        });
    }
    let index = eval(cx).await as i16;
    cx.cv(|c| {
        let mut back = c.pos();
        c.set_pos(addr - SCRIPT_BASE);
        if strings {
            let mut left = index;
            while left > 0 && !c.fault {
                if c.next() == 0 {
                    left -= 1;
                }
            }
        } else {
            c.advance(index.wrapping_mul(2));
            back -= 1;
        }
        back
    })
}

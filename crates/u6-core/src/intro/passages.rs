//! The passages of `u`: 0x10ac08 lays out numbered text pieces (story, gypsy, questions,
//! messages) into the passage bitmap.
//!
//! 0x10ac08 is one switch over the passage number: each case stores the scroll frame's position
//! and item, the text box's size and where it goes on screen, fills the box, appends its strings
//! (string literals of `u`, the player's name, the volume name of the disk prompt) to the text
//! buffer and sometimes changes the alignment. The cases are straight-line code of a dozen
//! instruction forms, and the port reads them out of the user's `u` at run time: it decodes the
//! compare chain at 0x10ac7a to find a case, then steps through the case with a small evaluator
//! of exactly those forms (a value for D0, A0, a byte stack for the pushed arguments). The
//! table stays in the user's file; no passage text or layout is in the port.

use crate::engine::Io;

use super::exe::{A4, UExe};
use super::state::IntroState;
use super::text::{self, Arg, TextCtx};

/// Where 0x10ac08 starts its compare chain (`move.l d7,d0` then `tst.w d0`).
const CHAIN: u32 = 0x0010_ac7a;
/// 0x10f898 (fill a view) and 0x1142f0 (`printf` appended to the buffer), the two calls the
/// cases make.
const FILL_VIEW: u32 = 0x0010_f898;
const PRINT_MORE: u32 = 0x0011_42f0;
/// The code after the switch: `movea.l -$a(a5),a0` (buffer start for the flush).
const TAIL_OPCODE: [u16; 2] = [0x206d, 0xfff6];
/// Bound on the instructions of one case (the longest has about 40).
const MAX_STEPS: usize = 400;

/// The case address of passage `n`, or the default case; `None` when the code at 0x10ac7a is
/// not the expected chain.
pub fn case_address(exe: &UExe, n: u16) -> Option<u32> {
    if exe.word(CHAIN) != 0x2007 {
        return None;
    }
    let mut pc = CHAIN + 2;
    let mut acc: u16 = 0;
    for _ in 0..512 {
        let op = exe.word(pc);
        match op {
            // tst.w d0
            0x4a40 => pc += 2,
            // subi.w #imm,d0
            0x0440 => {
                acc = acc.wrapping_add(exe.word(pc + 2));
                pc += 4;
            }
            // subq.w #q,d0
            _ if op & 0xf1ff == 0x5140 => {
                let q = (op >> 9) & 7;
                acc = acc.wrapping_add(if q == 0 { 8 } else { q });
                pc += 2;
            }
            // beq.w
            0x6700 => {
                let target = pc.wrapping_add(2).wrapping_add_signed(i32::from(exe.word(pc + 2) as i16));
                if acc == n {
                    return Some(target);
                }
                pc += 4;
            }
            // bra.w: the default case
            0x6000 => {
                return Some(pc.wrapping_add(2).wrapping_add_signed(i32::from(exe.word(pc + 2) as i16)));
            }
            _ => return None,
        }
    }
    None
}

/// A `jsr`/`bsr` target through its `jmp abs.l` stub.
fn resolve(exe: &UExe, target: u32) -> u32 {
    if exe.word(target) == 0x4ef9 {
        (u32::from(exe.word(target + 2)) << 16) | u32::from(exe.word(target + 4))
    } else {
        target
    }
}

/// The evaluator's machine: D0, A0, the stack bytes the case pushed (top = start).
struct Machine {
    d0: u32,
    a0: u32,
    stack: Vec<u8>,
}

impl Machine {
    fn push(&mut self, bytes: &[u8]) {
        self.stack.splice(0..0, bytes.iter().copied());
    }

    fn pop(&mut self, n: usize) {
        self.stack.drain(..n.min(self.stack.len()));
    }

    fn word_at(&self, i: usize) -> u16 {
        let b = |k: usize| self.stack.get(k).copied().unwrap_or(0);
        u16::from_be_bytes([b(i), b(i + 1)])
    }

    fn long_at(&self, i: usize) -> u32 {
        (u32::from(self.word_at(i)) << 16) | u32::from(self.word_at(i + 2))
    }
}

/// The string at a pushed address: the player's name buffer, else a string of `u`.
fn string_at(st: &IntroState, addr: u32) -> Vec<u8> {
    if addr == A4 + 0x2bd8 {
        st.hero.name.clone()
    } else {
        st.exe.cstr(addr)
    }
}

/// The arguments a format reads from the stack after its own pointer.
fn args_for(st: &IntroState, m: &Machine, fmt: &[u8]) -> Vec<Arg> {
    let mut at = 4;
    let mut args = Vec::new();
    let mut it = fmt.iter().copied().take_while(|&b| b != 0);
    while let Some(c) = it.next() {
        if c != b'%' {
            continue;
        }
        match it.next() {
            Some(b'c' | b'd' | b'u' | b'x' | b'B' | b'F' | b'X' | b'Y') => {
                args.push(Arg::Word(m.word_at(at)));
                at += 2;
            }
            Some(b'D' | b'U') => {
                args.push(Arg::Long(m.long_at(at)));
                at += 4;
            }
            Some(b's') => {
                args.push(Arg::Str(string_at(st, m.long_at(at))));
                at += 4;
            }
            _ => {}
        }
    }
    args
}

/// A word store of a case into the data hunk.
fn store(st: &mut IntroState, off: u16, v: u16) {
    let v16 = v as i16;
    let p = &mut st.views.passage;
    match off {
        0x2cfc => st.frame.x = v16,
        0x2cfe => st.frame.y = v16,
        0x2d00 => st.frame.item = v,
        0x2d0a => p.x1 = v16,
        0x2d0c => p.x0 = v16,
        0x2d0e => p.y0 = v16,
        0x2d10 => p.y1 = v16,
        0x2d1c => st.views.passage_dst.x0 = v16,
        0x2d1e => st.views.passage_dst.y0 = v16,
        _ => {
            crate::todo_stub!("u 0x10ac08 passage store to an unexpected global", ());
        }
    }
}

/// A word load of a case from the data hunk.
fn load(st: &IntroState, off: u16) -> u16 {
    match off {
        0x08d2 => st.volume,
        _ => crate::todo_stub!("u 0x10ac08 passage load of an unexpected global", 0),
    }
}

/// Runs the case at `pc` on the local context; stops at the branch to the tail or at the tail.
fn run_case(st: &mut IntroState, ctx: &mut TextCtx, mut pc: u32, d6: u32) {
    let mut m = Machine {
        d0: 0,
        a0: 0,
        stack: Vec::new(),
    };
    for _ in 0..MAX_STEPS {
        let exe = &st.exe;
        let op = exe.word(pc);
        let w1 = exe.word(pc + 2);
        let w2 = exe.word(pc + 4);
        if [op, w1] == TAIL_OPCODE {
            return;
        }
        match op {
            // moveq #n,d0
            _ if op & 0xff00 == 0x7000 => {
                m.d0 = (op as u8 as i8) as i32 as u32;
                pc += 2;
            }
            // move.l d6,d0
            0x2006 => {
                m.d0 = d6;
                pc += 2;
            }
            // move.w d0,off(a4)
            0x3940 => {
                store(st, w1, m.d0 as u16);
                pc += 4;
            }
            // move.w #imm,off(a4)
            0x397c => {
                store(st, w2, w1);
                pc += 6;
            }
            // clr.w off(a4)
            0x426c => {
                store(st, w1, 0);
                pc += 4;
            }
            // move.w off(a4),d0
            0x302c => {
                m.d0 = (m.d0 & 0xffff_0000) | u32::from(load(st, w1));
                pc += 4;
            }
            // ext.l d0
            0x48c0 => {
                m.d0 = (m.d0 as u16 as i16) as i32 as u32;
                pc += 2;
            }
            // asl.l #2,d0
            0xe580 => {
                m.d0 = m.d0.wrapping_shl(2);
                pc += 2;
            }
            // lea off(a4),a0
            0x41ec => {
                m.a0 = A4.wrapping_add_signed(i32::from(w1 as i16));
                pc += 4;
            }
            // move.l (a0,d0.l),(a7): an entry of the volume-name pointer table
            0x2eb0 if w1 == 0x0800 => {
                let p = if m.a0 == A4 + 0x8ac {
                    let i = (m.d0 / 4) as usize;
                    st.exe.volume_ptrs.get(i).copied().unwrap_or(0)
                } else {
                    0
                };
                m.pop(4);
                m.push(&p.to_be_bytes());
                pc += 4;
            }
            // pea d(pc)
            0x487a => {
                let a = (pc + 2).wrapping_add_signed(i32::from(w1 as i16));
                m.push(&a.to_be_bytes());
                pc += 4;
            }
            // pea off(a4)
            0x486c => {
                let a = A4.wrapping_add_signed(i32::from(w1 as i16));
                m.push(&a.to_be_bytes());
                pc += 4;
            }
            // subq.w #2,a7
            0x554f => {
                m.push(&[0, 0]);
                pc += 2;
            }
            // move.w d0,-(a7)
            0x3f00 => {
                m.push(&(m.d0 as u16).to_be_bytes());
                pc += 2;
            }
            // move.w d0,(a7)
            0x3e80 => {
                let w = (m.d0 as u16).to_be_bytes();
                m.pop(2);
                m.push(&w);
                pc += 2;
            }
            // addq.w #q,a7
            _ if op & 0xf1ff == 0x504f => {
                let q = (op >> 9) & 7;
                m.pop(usize::from(if q == 0 { 8 } else { q }));
                pc += 2;
            }
            // lea n(a7),a7
            0x4fef => {
                m.pop(usize::from(w1));
                pc += 4;
            }
            // move.b #imm,-2(a5): the alignment
            0x1b7c if w2 == 0xfffe => {
                ctx.align = w1 as u8;
                pc += 6;
            }
            // jsr d(pc)
            0x4eba => {
                let target = resolve(exe, (pc + 2).wrapping_add_signed(i32::from(w1 as i16)));
                pc += 4;
                match target {
                    FILL_VIEW => {
                        let v = st.views.passage;
                        if m.a0 == A4 + 0x2d02 {
                            super::mem::fill_view(&mut st.video.chip, &v, m.d0 as u8);
                        }
                    }
                    PRINT_MORE => {
                        let fmt = st.exe.cstr(m.long_at(0));
                        let args = args_for(st, &m, &fmt);
                        if let Some(font) = st.font.as_ref() {
                            text::print_more(&mut st.video.chip, ctx, font, &fmt, &args);
                        }
                    }
                    _ => {
                        crate::todo_stub!("u 0x10ac08 passage calls an unexpected routine", ());
                        return;
                    }
                }
            }
            // bra.w / bra.b: to the tail
            0x6000 => return,
            _ if op & 0xff00 == 0x6000 => return,
            _ => {
                crate::todo_stub!("u 0x10ac08 passage instruction not decoded", ());
                return;
            }
        }
    }
}

/// Original: u 0x10ac08 (show passage `n`: its text laid into the passage bitmap A4+0x2d02,
/// with the scroll frame and the screen position in the globals 0x10f4da uses)
///
/// A copy of main's context with the cursor at 0, ink 0, paper 0x1e, justified and an empty
/// buffer; waiting keys are dropped; the destination view starts as a copy of A4+0x17e4 and
/// the box at (0, 0). After the case, the buffer is flushed through the word wrapper into the
/// box. Nothing reaches the screen until 0x10f4da.
pub fn passage(st: &mut IntroState, io: &Io, n: u16) {
    let mut ctx = st.text.clone();
    st.keys.flush(io);
    st.views.passage_dst = st.views.draw;
    ctx.x = 0;
    ctx.y = 0;
    st.views.passage.x0 = 0;
    st.views.passage.y0 = 0;
    ctx.align = 3;
    ctx.fg = 0;
    ctx.bg = 0x1e;
    ctx.buf.clear();
    let d6 = 0x1e;
    if let Some(pc) = case_address(&st.exe, n) {
        run_case(st, &mut ctx, pc, d6);
    }
    ctx.view = st.views.passage;
    let buf = ctx.buf.clone();
    if let Some(font) = st.font.as_ref() {
        text::wrap(&mut st.video.chip, &mut ctx, font, &buf);
    }
}

/// Original: u 0x10f4da (passage: draw the scroll frame into the screen's drawing buffer and
/// copy the passage text onto it)
pub fn show_passage(st: &mut IntroState) {
    let planes = st.views.screen.planes;
    st.views.passage_dst.planes = planes;
    st.views.draw.planes = planes;
    let (v, f) = (st.views.draw, st.frame);
    super::mem::draw_item(&mut st.video.chip, &v, f.x, f.y, st.blocks, f.item);
    let (src, dst) = (st.views.passage, st.views.passage_dst);
    super::mem::copy_view(&mut st.video.chip, &src, &dst);
}

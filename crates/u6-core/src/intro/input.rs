//! Keyboard input of `u` (hunks 14 and 18).
//!
//! The input.device handler 0x114d30 maps each raw key event through 0x114f28 (u's own table at
//! A4+0x13e5, four columns per raw code, plus caps lock through the C type table A4+0x1611) and
//! queues the code in a 100-entry ring (0x113ed0); the program polls the ring (0x113f0e) and
//! takes from it (0x113f20). The port turns each engine [`Key`] into the raw codes a keyboard
//! sends (the bridge of [`crate::ui::input::raw_events`], on u's table) when the program polls,
//! so the codes come from u's table in arrival order. Mouse events never arrive: the engine has
//! no mouse, so both buttons read as released.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use crate::engine::{Io, Key};

use super::exe::{A4, UExe};

/// Raw codes per table (0x114f28 handles 0..0x5f, 0x60..0x67 are the qualifiers).
const RAW_KEYS: usize = 0x60;
/// Ring size of 0x113ed0 (indices wrap after 99).
const RING: usize = 100;

/// Original: A4+0x1566..0x1574 (qualifier words 0x114f28 keeps)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Qualifiers {
    /// Original: A4+0x1566 (left shift)
    pub lshift: bool,
    /// Original: A4+0x1568 (right shift)
    pub rshift: bool,
    /// Original: A4+0x1574 (caps lock)
    pub caps: bool,
    /// Original: A4+0x1572 (control)
    pub ctrl: bool,
    /// Original: A4+0x156a (left alt)
    pub lalt: bool,
    /// Original: A4+0x156c (right alt)
    pub ralt: bool,
    /// Original: A4+0x156e / A4+0x1570 (Amiga keys)
    pub amiga: [bool; 2],
}

/// The keyboard tables of `u` and the input state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keyboard {
    /// Original: A4+0x13e5 (per raw code: plain, shifted, control, alt)
    pub table: Vec<[u16; 4]>,
    /// Original: A4+0x1611 (C type table: bit 0 upper, bit 1 lower, bit 2 digit)
    pub ctype: Vec<u8>,
    pub q: Qualifiers,
    /// Original: A4+0x2d56 (the 100-word ring), A4+0xb40 (read index), A4+0xb41 (write index)
    ring: [u16; RING],
    read: u8,
    write: u8,
}

impl Keyboard {
    pub fn new(exe: &UExe) -> Self {
        let table = (0..RAW_KEYS)
            .map(|r| {
                std::array::from_fn(|c| u16::from(exe.byte(A4 + 0x13e5 + 4 * r as u32 + c as u32)))
            })
            .collect();
        Self {
            table,
            ctype: exe.bytes(A4 + 0x1611, 256),
            q: Qualifiers::default(),
            ring: [0; RING],
            read: 0,
            write: 0,
        }
    }

    /// Original: u 0x114f28 (input: raw key code → character, tracking the qualifiers; 0 =
    /// nothing)
    pub fn map_raw(&mut self, raw: u16) -> u16 {
        let down = raw <= 0x7f;
        let raw = raw & 0x7f;
        if raw >= 0x60 {
            let q = &mut self.q;
            match raw {
                0x60 => q.lshift = down,
                0x61 => q.rshift = down,
                0x62 => q.caps = down,
                0x63 => q.ctrl = down,
                0x64 => q.lalt = down,
                0x65 => q.ralt = down,
                0x66 => q.amiga[0] = down,
                0x67 => q.amiga[1] = down,
                _ => {}
            }
            return 0;
        }
        if !down {
            return 0;
        }
        let e = self.table.get(usize::from(raw)).copied().unwrap_or([0; 4]);
        let q = self.q;
        let code = if q.ctrl {
            e[2]
        } else if q.lshift || q.rshift {
            e[1]
        } else if q.lalt || q.ralt {
            e[3]
        } else {
            e[0]
        };
        let t = self.ctype.get(usize::from(code)).copied().unwrap_or(0);
        if q.caps && t & 3 != 0 && t & 2 != 0 {
            return code - 0x20;
        }
        code
    }

    /// Original: u 0x113ed0 (input: queue a nonzero code unless the ring is full)
    pub fn queue(&mut self, code: u16) {
        let next = if self.write >= 99 { 0 } else { self.write + 1 };
        if self.read == next || code == 0 {
            return;
        }
        self.ring[usize::from(self.write)] = code;
        self.write = next;
    }

    /// The engine's queued keys, as the handler would have queued them.
    ///
    /// Replaces: u 0x114d30 (the input.device handler; its mouse moves and the Alt + Help
    /// BEAMCON0 keys have no source in the engine)
    fn pump(&mut self, io: &Io) {
        while let Some(key) = try_key(io) {
            for raw in crate::ui::input::raw_events(&self.table, key) {
                let code = self.map_raw(raw);
                self.queue(code);
            }
        }
    }

    /// Original: u 0x113f0e (input: is a code waiting?)
    pub fn ready(&mut self, io: &Io) -> bool {
        self.pump(io);
        self.read != self.write
    }

    /// Original: u 0x113f20 (input: take the next code; the caller has seen [`Keyboard::ready`])
    ///
    /// The original spins until a code arrives; every caller asks [`Keyboard::ready`] first, so
    /// the port returns 0 instead of spinning on an empty ring.
    pub fn take(&mut self, io: &Io) -> u16 {
        self.pump(io);
        if self.read == self.write {
            return 0;
        }
        let code = self.ring[usize::from(self.read)];
        self.read = if self.read >= 99 { 0 } else { self.read + 1 };
        code
    }

    /// Drops every waiting code (the `while ready: take` loops of 0x10ac08 and 0x1041b0).
    pub fn flush(&mut self, io: &Io) {
        while self.ready(io) {
            self.take(io);
        }
    }
}

/// The next engine key if one is queued, without waiting.
pub fn try_key(io: &Io) -> Option<Key> {
    let mut next = pin!(io.next_key());
    match next.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(k) => Some(k),
        Poll::Pending => None,
    }
}

/// Original: u 0x101bf8 (C runtime `toupper` of a byte)
pub const fn to_upper(c: u16) -> u16 {
    if c >= 0x61 && c <= 0x7a { c - 0x20 } else { c }
}

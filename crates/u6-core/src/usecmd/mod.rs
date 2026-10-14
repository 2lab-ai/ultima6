//! The Use command 0x1305e0 and its per-object handlers (hunk 16, plus the hunk-15 and hunk-12
//! helpers only Use reaches).
//!
//! Entry point: [`use_command`], what the command loop runs for code 0x88 once the cross-hair
//! or panel cursor confirmed a target (A4-0x2f04 selection, A4-0x2f02 / -0x2f00 map cell). It
//! checks the target ([`dispatch`]), then switches on the object type to one handler; every
//! handler that ran ends with the leader paying 5 movement points and a view refresh. Handlers
//! print through the async `printf` (a more prompt may wait), so they are `async` and take the
//! shared state cell; synchronous helpers other modules call too (kegs, horses) queue their
//! prints (`main_loop::queue_print`) and the Use path flushes them in order.
//!
//! The tables Use reads live in the user's `game` and are read per call ([`defs`]); no domain
//! state of its own is kept (the globals it touches belong to ui, objects, actors and map).
//! Calls into modules that are not ported or are owned elsewhere go through [`seams`].
//!
//! Original functions this module owns (hunk 16 unless noted; 0x12dcbe and 0x1301e2 of this
//! hunk were ported with objects):
//!
//! - 0x1305e0 Use command: target checks, dispatch by type, end of the command
//! - 0x130546 type refused aboard ship
//! - 0x11d0a4 / 0x11cd78 (hunk 12) leave / board a vessel
//! - 0x12dd1a [objects] drawbridge piece at a tile (with 0x12de3a, not ported)
//! - 0x12ddae [objects] tile in the 5-byte list at A4-0x6614 (with 0x12de3a, not ported)
//! - 0x12ddda [objects] place a drawbridge piece (with 0x12de3a, not ported)
//! - 0x12de3a [objects] raise / lower the drawbridge (stub point)
//! - 0x12ff50 [objects] serpent figure (stub point)
//!
//! Ported with an `Original:` marker each: 0x12c8b8 0x12cfe4 0x12d096 0x12d204 0x12d336
//! 0x12d3c4 0x12d452 0x12d4bc 0x12d526 0x12d7d4 0x12d960 0x12dd6e 0x12e3f6 (partial) 0x12e538
//! 0x12e77c 0x12e912 0x12e924 0x12eb12 0x12eca6 0x12ed4a 0x12ef7a 0x12f0a4 0x12f0d2 0x12f1e6
//! 0x12f3fa 0x12f502 0x12f7ea 0x12fb0a 0x12fca2 0x12fdf6 0x130380 0x1303f4 0x13051a 0x130526.
//!
//! The hunk-15 helpers Use calls (the type list 0x128b6a, doors and chests 0x12bbc0 / 0x12bdfa,
//! the rolls 0x12c062 / 0x12c092, keys 0x12c0c2, torches 0x12c326, the gem 0x12c54a, fixed
//! lights 0x12c594, the frame toggle 0x12c68a, search 0x129544) and the shared world helpers
//! (kegs 0x12db7e / 0x12d9ee, horses 0x12f6ee / 0x12f8fe) are `objects::commands`' ports; the
//! type switch calls them there (integration M2: one implementation per address).

mod cases;
pub mod defs;
mod dispatch;
mod food;
mod mech;
mod misc;
pub(crate) mod seams;
mod travel;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_real;

use std::cell::{RefCell, RefMut};

pub use dispatch::{refused_aboard, use_command};
pub(crate) use mech::{drawbridge, lever};

use crate::engine::Io;
use crate::image::A4;
use crate::main_loop::{flush_prints, game_string, pointed_string};
use crate::objects::Slot;
use crate::state::State;
use crate::ui::printf::{Arg, printf, text_out};

/// The state cell and the platform view every handler gets.
#[derive(Clone, Copy)]
pub(crate) struct Ux<'a> {
    pub state: &'a RefCell<State>,
    pub io: &'a Io,
}

impl<'a> Ux<'a> {
    pub fn new(state: &'a RefCell<State>, io: &'a Io) -> Self {
        Self { state, io }
    }

    /// One short borrow of the whole state.
    pub fn s(&self) -> RefMut<'a, State> {
        self.state.borrow_mut()
    }

    /// printf 0x108658 of the string at image address `fmt`.
    pub async fn print(&self, fmt: u32, args: &[Arg<'_>]) {
        let text = game_string(self.io, fmt);
        printf(self.state, self.io, &text, args).await;
    }

    /// printf 0x108658 of the string at A4 - `off` (a data-hunk string).
    pub async fn print_a4(&self, off: u32) {
        self.print(A4.wrapping_sub(off), &[]).await;
    }

    /// printf of the format at `fmt` with the message the pointer at A4 - `ptr` names (the
    /// shared Use messages: not usable, no effect, ...).
    pub async fn say(&self, fmt: u32, ptr: u32) {
        let msg = pointed_string(self.io, A4.wrapping_sub(ptr));
        self.print(fmt, &[Arg::Str(&msg)]).await;
    }

    /// printf of raw bytes as the format (a name returned by 0x10eef8).
    pub async fn print_bytes(&self, text: &[u8]) {
        printf(self.state, self.io, text, &[]).await;
    }

    /// 0x108376: one character through the escape layer.
    pub async fn put(&self, c: u8) {
        text_out(self.state, self.io, i16::from(c)).await;
    }

    /// printf of the display name 0x10eef8 of `slot`.
    pub async fn name(&self, slot: Slot) {
        let name = crate::objects::display_name(&self.s(), slot);
        let bytes: Vec<u8> = name.chars().map(|c| c as u32 as u8).collect();
        self.print_bytes(&bytes).await;
    }

    /// Prints what synchronous code queued, in order.
    pub async fn flush(&self) {
        flush_prints(self.state, self.io).await;
    }

    /// 0x10790a: the next command key.
    pub async fn key(&self) -> u16 {
        crate::ui::wait::read_key(self.state, self.io).await
    }

    /// 0x102ef0 then the queued prints (synchronous refresh inside an async handler).
    pub fn refresh(&self) {
        crate::gfx::refresh_view(&mut self.s(), self.io);
    }
}

/// 0x10ad1a: the first object at (x, y, z), -1 (starts the location cursor).
pub(crate) fn first_at(s: &mut State, x: u16, y: u16, z: u16) -> Slot {
    crate::objects::begin_at(&mut s.objects, &s.tables, x, y, z)
}

/// 0x10aaca: the next object of the location cursor, -1.
pub(crate) fn next_here(s: &mut State) -> Slot {
    crate::objects::next_at(&mut s.objects, &s.tables)
}

/// `|a - b|` on words, as the original's `sub.w` / `neg.w`.
pub(crate) fn dist(a: i16, b: i16) -> i16 {
    a.wrapping_sub(b).wrapping_abs()
}

/// Actor of party member `i` (A4+0x47ec, unchecked index as the original reads it).
pub(crate) fn member(s: &State, i: usize) -> u8 {
    s.actors.party.members.get(i).copied().unwrap_or(0)
}

/// The actor of the leader slot (A4+0x47ec + A4-0x7b7a).
pub(crate) fn leader(s: &State) -> u8 {
    member(s, usize::from(s.actors.party.leader_index))
}

/// The party is aboard a vessel (leader index = party size).
pub(crate) fn aboard(s: &State) -> bool {
    s.actors.party.leader_index == s.actors.party.size
}

/// flags3 of the tile `slot` shows (0x13c140[basetile[type] + frame]).
pub(crate) fn flags3_of(s: &State, slot: Slot) -> u8 {
    let tf = s.objects.tf(slot);
    let base = s
        .tables
        .basetile
        .base
        .get(usize::from(tf & 0x3ff))
        .copied()
        .unwrap_or(0);
    let tile = base.wrapping_add(tf >> 10);
    s.tables
        .tileflags
        .flags3
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
}

/// flags1 of the tile `slot` shows (0x13b140).
pub(crate) fn flags1_of(s: &State, slot: Slot) -> u8 {
    let tf = s.objects.tf(slot);
    let base = s
        .tables
        .basetile
        .base
        .get(usize::from(tf & 0x3ff))
        .copied()
        .unwrap_or(0);
    let tile = base.wrapping_add(tf >> 10);
    s.tables
        .tileflags
        .flags1
        .get(usize::from(tile))
        .copied()
        .unwrap_or(0)
}

/// Sets the 6-bit frame of `slot`, type kept.
pub(crate) fn set_frame(s: &mut State, slot: Slot, frame: u16) {
    if let Some(w) = usize::try_from(slot)
        .ok()
        .and_then(|i| s.objects.type_frame.get_mut(i))
    {
        *w = (*w & 0x3ff).wrapping_add(frame << 10);
    }
}

/// Sets the whole type + frame word of `slot`.
pub(crate) fn set_tf(s: &mut State, slot: Slot, tf: u16) {
    if let Some(w) = usize::try_from(slot)
        .ok()
        .and_then(|i| s.objects.type_frame.get_mut(i))
    {
        *w = tf;
    }
}

/// The quantity byte (low byte of A4+0x250c) of `slot`.
pub(crate) fn quantity(s: &State, slot: Slot) -> u8 {
    s.objects.qq_of(slot) as u8
}

/// The quality byte (high byte of A4+0x250c) of `slot`.
pub(crate) fn quality(s: &State, slot: Slot) -> u8 {
    (s.objects.qq_of(slot) >> 8) as u8
}

/// Sets the quantity byte of `slot`.
pub(crate) fn set_quantity(s: &mut State, slot: Slot, q: u8) {
    if let Some(w) = usize::try_from(slot)
        .ok()
        .and_then(|i| s.objects.qq.get_mut(i))
    {
        *w = (*w & 0xff00) | u16::from(q);
    }
}

/// Sets the quality byte of `slot`.
pub(crate) fn set_quality(s: &mut State, slot: Slot, q: u8) {
    if let Some(w) = usize::try_from(slot)
        .ok()
        .and_then(|i| s.objects.qq.get_mut(i))
    {
        *w = (*w & 0x00ff) | u16::from(q) << 8;
    }
}

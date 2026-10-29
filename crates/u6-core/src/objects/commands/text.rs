//! Text output of the commands: printf 0x108658 and puts 0x108376 of strings read from the
//! user's `game` ([`GameData`](super::GameData)).
//!
//! Synchronous world code queues its lines ([`queue`]); the async handlers print at once, after
//! flushing what their synchronous callees queued, so the scroll keeps the original's order.

use std::cell::RefCell;

use crate::engine::Io;
use crate::main_loop::PrintArg;
use crate::state::State;
use crate::ui::printf::{Arg, printf};

/// The game string at image address `addr`.
pub(crate) fn string(s: &State, addr: crate::string_registry::Text) -> Vec<u8> {
    addr.data(&s.objcmds.data)
}

/// The game string at `A4 + off`.
pub(crate) fn a4_string(s: &State, off: crate::string_registry::Text) -> Vec<u8> {
    off.data(&s.objcmds.data)
}

/// Queues printf of `fmt` for the command loop (synchronous callers).
pub(crate) fn queue(s: &mut State, fmt: Vec<u8>, args: Vec<PrintArg>) {
    crate::main_loop::queue_print(s, fmt, args);
}

/// Queues printf of the game string at `addr`.
pub(crate) fn queue_at(s: &mut State, addr: crate::string_registry::Text, args: Vec<PrintArg>) {
    let fmt = string(s, addr);
    queue(s, fmt, args);
}

/// printf 0x108658 of `fmt`, after the lines synchronous callees queued.
pub(crate) async fn print(state: &RefCell<State>, io: &Io, fmt: &[u8], args: &[Arg<'_>]) {
    crate::main_loop::flush_prints(state, io).await;
    printf(state, io, fmt, args).await;
}

/// printf of the game string at image address `addr`.
pub(crate) async fn print_at(
    state: &RefCell<State>,
    io: &Io,
    addr: crate::string_registry::Text,
    args: &[Arg<'_>],
) {
    let fmt = string(&state.borrow(), addr);
    print(state, io, &fmt, args).await;
}

/// 0x108376: one character through the escape layer (the commands print their newlines so).
pub(crate) async fn putc(state: &RefCell<State>, io: &Io, c: u8) {
    crate::main_loop::flush_prints(state, io).await;
    crate::ui::printf::text_out(state, io, i32::from(c)).await;
}

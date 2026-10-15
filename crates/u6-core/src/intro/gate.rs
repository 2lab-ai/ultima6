//! The end of the opening (0x1041b0 from 0x104b1c to 0x10558e): the obsidian stone among the
//! standing stones, the red moongate that rises from them, and the walk into the gate
//! (`intro_3.new`, palette 1 with colour cycling).
//!
//! `intro_3.new`: item 0 the circle of stones at night, 3 the obsidian stone lying in it, 1 the
//! hand that picks it up, 5 the circle lit red by the gate, 4 the stones in front, 2 the red
//! moongate (drawn through the room view A4+0x25f6, clipped at y 0x65 so it rises out of the
//! ground), 6 the hand holding the stone close up (the view tilts down to it), 7..0x1a the
//! hero seen from behind, smaller step by step, walking into the gate.

use crate::engine::Io;

use super::load;
use super::mem::{self, Ptr, View};
use super::passages::{passage, show_passage};
use super::screen::{check_key, show, swap};
use super::state::IntroState;

/// Draws item `item` of `file` on the drawing view.
fn put(st: &mut IntroState, file: Ptr, x: i16, y: i16, item: u16) {
    let v = st.views.draw;
    mem::draw_item(&mut st.video.chip, &v, x, y, file, item);
}

/// Draws item `item` of `file` on the room view (the gate's clip).
fn put_room(st: &mut IntroState, file: Ptr, x: i16, y: i16, item: u16) {
    let v = st.views.room;
    mem::draw_item(&mut st.video.chip, &v, x, y, file, item);
}

/// The SAS/C clamp of the opening's loops: `v` stops at `to` once it reaches it from `above`
/// (true) or below.
fn clamp_at(v: i16, to: i16, above: bool) -> i16 {
    if (above && v <= to) || (!above && v >= to) {
        to
    } else {
        v
    }
}

/// Fills the screen view black and swaps, twice (0x104b56 and others).
async fn black(st: &mut IntroState, io: &Io) {
    for _ in 0..2 {
        let s = st.views.screen;
        mem::fill_view(&mut st.video.chip, &s, 0);
        swap(st, io).await;
    }
}

/// Waits for a key the way the opening spins on 0x1039b8, one frame per poll.
async fn until_key(st: &mut IntroState, io: &Io) {
    while check_key(st, io) == 0 {
        st.video.wait_frame(io).await;
    }
}

/// The gate scene's three layers: the red circle 5 at (0, `scene_y`), the stones in front 4 at
/// (0x5e, `front_y`), the gate 2 at (`gx`, `gy`) through the room view.
fn gate_layers(st: &mut IntroState, file: Ptr, scene_y: i16, front_y: i16, gx: i16, gy: i16) {
    put(st, file, 0, scene_y, 5);
    put(st, file, 0x5e, front_y, 4);
    put_room(st, file, gx, gy, 2);
}

/// One frame of the view tilting down to the hand: the layers `d` pixels up, the bottom `d + 1`
/// rows cleared (the drawing view's y0 moved down for the fill), the hand 6 at (0x9b, 200 − 2d).
fn tilt_frame(st: &mut IntroState, file: Ptr, d: i16) {
    gate_layers(st, file, -d, 0x66 - d, 0x7c, 5 - d);
    st.views.draw.y0 = 0xc7 - d;
    let v = st.views.draw;
    mem::fill_view(&mut st.video.chip, &v, 0);
    st.views.draw.y0 = 0;
    put(st, file, 0x9b, 0xc8 - 2 * d, 6);
}

/// 0x104b1c..0x10558e of 0x1041b0: the moongate and the portal. Returns when the opening
/// reaches 0x10558e (its end, or Escape).
///
/// Passages 9 .. 0xf, each until a key: the stones at night (9), the hand picks the stone up
/// (0xa), the gate rises and the hand goes down (0xb), the view tilts down to the stone in the
/// hand and back (0xc, 0xd), the gate quivers (0xe, 0xf), then the hero walks into it and the
/// gate sinks into the ground; the empty circle stays until a key.
pub(super) async fn gate(st: &mut IntroState, io: &Io) {
    check_key(st, io);
    if st.skip != 0 {
        return;
    }
    for v in [st.views.screen, st.views.draw] {
        mem::fill_view(&mut st.video.chip, &v, 0);
    }
    // 0x107822(2): an empty routine of `u`.
    let file = st.files_ptr;
    load::load_into(&mut st.video.chip, &st.files, b"intro_3.new", file);
    black(st, io).await;
    st.video.set_palette(&st.exe, 1);
    st.video.cycling = 1;
    check_key(st, io);
    if st.skip != 0 {
        return;
    }
    passage(st, io, 9);
    put(st, file, 0, 0, 0);
    swap(st, io).await;
    st.views.draw.planes = st.views.screen.planes;
    check_key(st, io);
    if st.skip != 0 {
        return;
    }
    for _ in 0..3 {
        put(st, file, 0, 0, 0);
        show(st, io).await;
        if check_key(st, io) != 0 {
            break;
        }
    }
    if st.skip != 0 {
        return;
    }
    loop {
        put(st, file, 0, 0, 0);
        show_passage(st);
        show(st, io).await;
        if check_key(st, io) != 0 {
            break;
        }
    }
    if st.skip != 0 {
        return;
    }
    st.views.screen.y1 = 0x46;
    black(st, io).await;
    st.views.screen.y1 = 0xc7;
    st.views.room = View {
        y1: 0x65,
        ..st.views.draw
    };
    passage(st, io, 0xa);
    // The hand picks the stone up.
    let step = 8;
    let mut y: i16 = 0xc7;
    let mut done = false;
    while !done {
        done = y == 0x54;
        put(st, file, 0, 0, 0);
        put(st, file, 0xbd, y, 1);
        put(st, file, 0x96, 0x64, 3);
        show_passage(st);
        show(st, io).await;
        if check_key(st, io) != 0 {
            y = 0x54 + step;
        }
        if st.skip != 0 {
            return;
        }
        y = clamp_at(y - step, 0x54, true);
    }
    passage(st, io, 0xb);
    until_key(st, io).await;
    // The gate rises.
    let step = 4;
    let mut gy: i16 = 0x65;
    let mut done = false;
    while !done {
        done = gy == 6;
        put(st, file, 0, 0, 5);
        put(st, file, 0x5e, 0x66, 4);
        put(st, file, 0xbd, 0x54, 1);
        put_room(st, file, 0x7c, gy, 2);
        show_passage(st);
        show(st, io).await;
        if check_key(st, io) != 0 {
            gy = step + 6;
        }
        if st.skip != 0 {
            return;
        }
        gy = clamp_at(gy - step, 6, true);
    }
    // The hand goes down (no Escape check in this loop).
    let step = 8;
    let mut y: i16 = 0x54;
    let mut done = false;
    while !done {
        done = y == 0xcc;
        put(st, file, 0, 0, 5);
        put(st, file, 0x5e, 0x66, 4);
        put(st, file, 0xbd, y, 1);
        put_room(st, file, 0x7c, 5, 2);
        show_passage(st);
        show(st, io).await;
        if check_key(st, io) != 0 {
            y = 0xcc - step;
        }
        y = clamp_at(y + step, 0xcc, false);
    }
    until_key(st, io).await;
    if st.skip != 0 {
        return;
    }
    if !hand_and_walk(st, io, file).await {
        return;
    }
    sink_and_wait(st, io, file).await;
}

/// The gate scene with the gate at its rest position, shown with the passage until a key;
/// `tremble` = the rand mask of the gate's y (0 = still, 1 or 3 = quivering, x by 1 too).
/// False on Escape.
async fn gate_until_key(st: &mut IntroState, io: &Io, file: Ptr, tremble: i16) -> bool {
    loop {
        put(st, file, 0, 0, 5);
        put(st, file, 0x5e, 0x66, 4);
        let (gx, gy) = if tremble == 0 {
            (0x7c, 5)
        } else {
            let gx = (st.rand() & 1) + 0x7c;
            (gx, (st.rand() & tremble) + 5)
        };
        put_room(st, file, gx, gy, 2);
        show_passage(st);
        show(st, io).await;
        if check_key(st, io) != 0 {
            break;
        }
    }
    st.skip == 0
}

/// 0x104ef2..0x10548c: the view tilts down to the hand and back (passages 0xc, 0xd), the gate
/// quivers (0xe, 0xf), the hero walks into it. False on Escape.
async fn hand_and_walk(st: &mut IntroState, io: &Io, file: Ptr) -> bool {
    let step = 4;
    let mut d: i16 = 0;
    let mut done = false;
    while !done {
        done = d == 0x3f;
        tilt_frame(st, file, d);
        show(st, io).await;
        if check_key(st, io) != 0 {
            done = true;
        }
        if st.skip != 0 {
            return false;
        }
        d = clamp_at(d + step, 0x3f, false);
    }
    passage(st, io, 0xc);
    loop {
        gate_layers(st, file, -0x3f, 0x27, 0x7c, -0x3a);
        st.views.draw.y0 = 0x87;
        let v = st.views.draw;
        mem::fill_view(&mut st.video.chip, &v, 0);
        st.views.draw.y0 = 0;
        put(st, file, 0x9b, 0x4a, 6);
        show_passage(st);
        show(st, io).await;
        if check_key(st, io) != 0 {
            break;
        }
    }
    if st.skip != 0 {
        return false;
    }
    let v = st.views.draw;
    mem::fill_view(&mut st.video.chip, &v, 0);
    let mut d: i16 = 0x3f;
    let mut done = false;
    while !done {
        done = d == 0;
        tilt_frame(st, file, d);
        show(st, io).await;
        if check_key(st, io) != 0 {
            done = true;
        }
        if st.skip != 0 {
            return false;
        }
        d = clamp_at(d - step, 0, true);
    }
    passage(st, io, 0xd);
    if !gate_until_key(st, io, file, 0).await {
        return false;
    }
    passage(st, io, 0xe);
    if !gate_until_key(st, io, file, 1).await {
        return false;
    }
    passage(st, io, 0xf);
    if !gate_until_key(st, io, file, 3).await {
        return false;
    }
    // The walk: the pan view takes the room view's size at the origin (unused after).
    let r = st.views.room;
    let p = &mut st.views.pan;
    p.x1 = 0;
    p.x0 = 0;
    p.y1 = r.y1 - r.y0;
    p.x1 = r.x1 - r.x0;
    let mut n: i16 = 0;
    let mut done = false;
    while !done {
        done = n == 0x3b;
        put(st, file, 0, 0, 5);
        put(st, file, 0x5e, 0x66, 4);
        let gx = (st.rand() & 1) + 0x7c;
        let gy = (st.rand() & 3) + 5;
        put_room(st, file, gx, gy, 2);
        put(st, file, -2, 0x12 - n, (n / 3 + 7) as u16);
        show(st, io).await;
        if check_key(st, io) != 0 {
            done = true;
        }
        if st.skip != 0 {
            return false;
        }
        n = clamp_at(n + 1, 0x3b, false);
    }
    gate_layers(st, file, 0, 0x66, 0x7c, 5);
    check_key(st, io);
    if st.skip != 0 {
        return false;
    }
    put(st, file, -2, -0x2a, 0x1a);
    show(st, io).await;
    check_key(st, io);
    if st.skip != 0 {
        return false;
    }
    if st.w25c6 == 0x13 {
        // 0x10fa3a: the room view takes the planes of the screen view, then of the drawing view.
        st.views.room.planes = st.views.screen.planes;
        st.views.room.planes = st.views.draw.planes;
    }
    check_key(st, io);
    st.skip == 0
}

/// 0x105498..0x10558e: the gate sinks back into the ground, then the empty circle stays until a
/// key.
async fn sink_and_wait(st: &mut IntroState, io: &Io, file: Ptr) {
    let step = 4;
    let mut gy: i16 = 5;
    let mut done = false;
    while !done {
        done = gy == 0x65;
        gate_layers(st, file, 0, 0x66, 0x7c, gy);
        show(st, io).await;
        if check_key(st, io) != 0 {
            gy = 0x65 - step;
        }
        if st.skip != 0 {
            return;
        }
        gy = clamp_at(gy + step, 0x65, false);
    }
    let v = st.views.draw;
    mem::fill_view(&mut st.video.chip, &v, 0);
    while check_key(st, io) == 0 {
        put(st, file, 0, 0, 0);
        put(st, file, 0x96, 0x64, 3);
        show(st, io).await;
    }
}

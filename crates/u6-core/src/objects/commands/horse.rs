//! Horses (hunk 16, 0x12f6ee / 0x12f8fe) and the typed-answer parser (hunk 15, 0x128b98).

use super::seams;
use super::text::queue_at;
use crate::objects::iter::{begin_at, next_at};
use crate::objects::moves::{create_from_record, move_to, remove};
use crate::objects::{Slot, actor_frame, pack_xyz, spawn_monster};
use crate::state::State;

/// The mounted horse (front and rear halves) and the free horse a dismount leaves.
const MOUNTED: u16 = 0x1af;
const HORSE: u16 = 0x1ae;
/// "\nRear horse???\n", "\nNo room!\n" (hunk 16).
const S_REAR_HORSE: u32 = 0x0012_ce32;
const S_NO_ROOM: u32 = 0x0012_cbd4;

fn z(s: &State) -> u16 {
    u16::from(s.map.centre.z)
}

fn refresh(s: &mut State) {
    crate::engine::with_current_io(|io| crate::gfx::refresh_view(s, io));
}

/// Original: 0x12f6ee (objects: put `rider` on a horse at (x, y) facing `frame`)
///
/// The rider moves to (x, y) and becomes the mounted horse's front (type 0x1af, `frame`); the
/// view centre follows. Behind it (direction `frame & 6` back) the rear half is the object
/// already there re-typed (`here`) or a new map object 0x1af with frame + 8 (0x10bc4c). Then the
/// region and the view are rebuilt (0x108f90, 0x102ef0).
pub fn mount_horse(s: &mut State, rider: Slot, x: u16, y: u16, frame: u16, here: bool) {
    let lz = z(s);
    move_to(s, rider, x, y, lz);
    if let Some(t) = s.objects.type_frame.get_mut(rider as usize) {
        *t = frame << 10 | MOUNTED;
    }
    s.map.centre.x = x;
    s.map.centre.y = y;
    let d = usize::from(frame & 6);
    let bx = x.wrapping_add_signed(s.objects.defs.dx(d).wrapping_neg());
    let by = y.wrapping_add_signed(s.objects.defs.dy(d).wrapping_neg());
    if here {
        let o = begin_at(&mut s.objects, &s.tables, bx, by, lz);
        if let Some(t) = s.objects.type_frame.get_mut(o as usize) {
            *t = (*t >> 10) << 10 | MOUNTED;
        }
    } else {
        let [b0, b1, b2] = pack_xyz(bx, by, lz).to_packed();
        let [t0, t1] = ((frame + 8) << 10 | MOUNTED).to_be_bytes();
        create_from_record(s, [0, b0, b1, b2, t0, t1, 0, 0]);
    }
    crate::map::region::refresh_region(s);
    refresh(s);
}

/// Original: 0x12f8fe (objects: `rider` gets off its horse)
///
/// The rider (the actor of a horse part, combat 0x1240fa) takes back its base type with the
/// frame of its facing (0x10a4d6); the rear half behind it goes ("Rear horse???" when there is
/// none). A new horse (0x1ae, 0x131646, made at 0, 0) is placed on the first tile around the
/// rider it may stand on (0x11b1c0; columns, then rows), else "No room!" and it is removed.
pub fn dismount(s: &mut State, rider: Slot) {
    let rider = seams::rider(s, rider);
    let (x, y) = (s.objects.x(rider), s.objects.y(rider));
    let d = usize::from((s.objects.tf(rider) >> 10) & 6);
    let i = rider as usize;
    let base = s.actors.attrs.base_type_frame[i] & 0x3ff;
    let dir = i16::from(s.actors.attrs.movement_flags[i] & 7);
    let f = actor_frame(base, dir) as u16;
    s.objects.type_frame[i] = f << 10 | base;
    let rx = x.wrapping_add_signed(s.objects.defs.dx(d).wrapping_neg());
    let ry = y.wrapping_add_signed(s.objects.defs.dy(d).wrapping_neg());
    let lz = z(s);
    let mut c = begin_at(&mut s.objects, &s.tables, rx, ry, lz);
    while c >= 0 && s.objects.kind(c) != MOUNTED {
        c = next_at(&mut s.objects, &s.tables);
    }
    if c >= 0 {
        remove(s, c);
    } else {
        queue_at(s, S_REAR_HORSE, Vec::new());
    }
    let lz = z(s);
    let horse = spawn_monster(s, HORSE, 0, 0, lz);
    let mut placed = false;
    'cols: for i in 0..3u16 {
        for j in 0..3u16 {
            if i == 1 && j == 1 {
                continue;
            }
            let (nx, ny) = (
                x.wrapping_add(i).wrapping_sub(1),
                y.wrapping_add(j).wrapping_sub(1),
            );
            if crate::actors::walk::can_move_to(s, horse as u16, nx, ny) {
                let lz = z(s);
                move_to(s, horse, nx, ny, lz);
                placed = true;
                break 'cols;
            }
        }
    }
    if !placed {
        queue_at(s, S_NO_ROOM, Vec::new());
        remove(s, horse);
    }
    refresh(s);
}

/// Original: 0x128b98 (objects: parse a typed answer: a number, or with `mode` 1 / 2 a number
/// and a hemisphere letter)
///
/// Digits accumulate (word arithmetic). At the first other character: mode 1 gives the number
/// for S, its negative for N, 100 for any other letter; mode 2 the same with E / W; mode 0
/// -2. At the end of the text: 100 in modes 1 and 2; in mode 0 the number, or for an empty
/// line -1 (-2 when the line editor was left by Escape, A4-0x7a44). Letters compare upper-cased
/// (the C library's lower-case class).
pub fn parse_answer(s: &State, text: &[u8], mode: i16) -> i16 {
    let mut n: i16 = 0;
    let len = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    for &c in &text[..len] {
        if c.is_ascii_digit() {
            n = n.wrapping_mul(10).wrapping_add(i16::from(c - b'0'));
            continue;
        }
        let up = c.to_ascii_uppercase();
        return match (mode, up) {
            (1, b'S') | (2, b'E') => n,
            (1, b'N') | (2, b'W') => n.wrapping_neg(),
            (1 | 2, _) => 100,
            _ => -2,
        };
    }
    if mode != 0 {
        return 100;
    }
    match (len, s.ui.input.line_cancelled) {
        (0, false) => -1,
        (0, true) => -2,
        _ => n,
    }
}

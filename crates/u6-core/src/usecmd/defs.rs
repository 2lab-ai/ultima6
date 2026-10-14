//! Tables and strings Use reads out of the user's `game` (spec 03 §5, "data, not constants"),
//! read per call through the shared hunk loader. Only their image addresses live here; a file
//! whose image lacks them yields empty tables (every type then counts as not usable).

use crate::engine::Io;
use crate::image::A4;
use crate::main_loop::pointed_string;

/// Original: A4-0x65aa (10 words: types 0x130546 refuses while aboard ship)
pub const ABOARD_REFUSED: (i32, usize) = (-0x65aa, 10);
/// Original: A4-0x61be (8 pointers: "North" .. "Northwest" by direction)
pub const DIRECTION_NAMES: i32 = -0x61be;
/// Original: A4-0x61ce / A4-0x61c6 (8 signed bytes each: dx / dy of the 8 directions)
pub const DIR_DX: i32 = -0x61ce;
pub const DIR_DY: i32 = -0x61c6;
/// Original: A4-0x661c / A4-0x6618 (4 signed bytes each: cannon shot dx / dy by its frame)
pub const CANNON_DX: i32 = -0x661c;
pub const CANNON_DY: i32 = -0x6618;
/// Original: A4-0x71b4 (pointers by a wand's quality: 0-terminated spell-number lists)
pub const WAND_SPELLS: i32 = -0x71b4;
/// Original: A4-0x70b8 (pointers by spell number: spell names)
pub const SPELL_NAMES: i32 = -0x70b8;
/// Original: A4-0x7dbd (2 bytes per monster kind; byte 1 = resistances, 0x04 poison, 0x80
/// sleep)
pub const MONSTER_FLAGS: i32 = -0x7dbd;
/// Original: A4-0x31f7 (byte per character: class bits, 0x04 = digit)
pub const CHAR_CLASS: i32 = -0x31f7;

/// Message pointers in the data hunk (A4 - n): `"%s"` arguments of the Use messages.
pub const MSG_SUCCESS: u32 = 0x6c6c;
pub const MSG_FAILED: u32 = 0x6c68;
pub const MSG_NO_EFFECT: u32 = 0x6c64;
pub const MSG_OUT_OF_RANGE: u32 = 0x6c60;
pub const MSG_NOTHING: u32 = 0x6c58;
pub const MSG_DONE: u32 = 0x6c50;
pub const MSG_ON_WHOM: u32 = 0x6c48;
pub const MSG_NOT_USABLE: u32 = 0x6c40;

fn a4(off: i32) -> u32 {
    (A4 as i32).wrapping_add(off) as u32
}

/// `len` bytes at image address `addr` of the user's `game` (BSS reads 0), empty on failure.
pub fn bytes(io: &Io, addr: u32, len: usize) -> Vec<u8> {
    io.game_image()
        .and_then(|img| img.read_zero_filled(addr, len).ok())
        .unwrap_or_default()
}

/// `n` big-endian words at A4 + `off`.
pub fn a4_words(io: &Io, off: i32, n: usize) -> Vec<u16> {
    bytes(io, a4(off), 2 * n)
        .chunks(2)
        .map(|w| u16::from_be_bytes([w[0], w[1]]))
        .collect()
}

/// Byte `i` at A4 + `off` (0 when the file cannot be read).
pub fn a4_byte(io: &Io, off: i32, i: usize) -> u8 {
    bytes(io, a4(off).wrapping_add(i as u32), 1)
        .first()
        .copied()
        .unwrap_or(0)
}

/// Signed byte `i` at A4 + `off`.
pub fn a4_sbyte(io: &Io, off: i32, i: usize) -> i8 {
    a4_byte(io, off, i) as i8
}

/// The string the pointer at A4 + `off` + 4 `i` names.
pub fn a4_pointed(io: &Io, off: i32, i: usize) -> Vec<u8> {
    pointed_string(io, a4(off).wrapping_add(4 * i as u32))
}

/// The bytes the pointer at A4 + `off` + 4 `i` names, up to the first 0 (at most 64).
pub fn a4_pointed_list(io: &Io, off: i32, i: usize) -> Vec<u8> {
    let Some(img) = io.game_image() else {
        return Vec::new();
    };
    let Ok(p) = img.pointer(a4(off).wrapping_add(4 * i as u32)) else {
        return Vec::new();
    };
    let list = img.read_zero_filled(p, 64).unwrap_or_default();
    list.into_iter().take_while(|&b| b != 0).collect()
}

/// The name of direction `dir` (0 = north, clockwise).
pub fn direction_name(io: &Io, dir: i16) -> Vec<u8> {
    a4_pointed(io, DIRECTION_NAMES, (dir & 7) as usize)
}

/// (dx, dy) of direction `dir` (0 = north, clockwise).
pub fn direction(io: &Io, dir: i16) -> (i16, i16) {
    let i = (dir & 7) as usize;
    (
        i16::from(a4_sbyte(io, DIR_DX, i)),
        i16::from(a4_sbyte(io, DIR_DY, i)),
    )
}

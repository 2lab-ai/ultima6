//! Keyboard input: the raw-key → game-code map 0x151d00 with its qualifier state, and the bridge
//! from the engine's logical [`Key`] events to Amiga raw key codes.
//!
//! The original input.device handler 0x151b08 maps every raw event through 0x151d00 and queues
//! the result in a 50-entry ring (0x14f0ac); the game polls it (0x134e7c). The port's engine
//! queue replaces handler and ring ([`crate::Io::next_key`]); each [`Key`] is one press. The
//! bridge turns it into the raw codes a keyboard would send and runs them through the ported
//! map, so the codes the game sees come from its own table (a UK layout: shift-3 is 0xa3).
//!
//! Game codes are the PC codes the game was written for: ASCII, or 0x100 + scan code for keys
//! without one (0x148 up, 0x150 down, 0x14b left, 0x14d right, 0x147 / 0x149 / 0x14f / 0x151 the
//! diagonals, 0x113.. Alt + letter).

use crate::engine::Key;

/// Raw code bit of a key release.
pub const RELEASE: u16 = 0x80;
/// First qualifier raw code (left shift); 0x60..=0x67 are the qualifiers.
pub const QUALIFIERS: u16 = 0x60;
const LSHIFT: u16 = 0x60;

/// Original: A4-0x3272..-0x3262 (qualifier words 0x151d00 keeps, and the Alt + keypad number)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Qualifiers {
    /// Original: A4-0x3272 (left shift held)
    pub lshift: bool,
    /// Original: A4-0x3270 (right shift)
    pub rshift: bool,
    /// Original: A4-0x3266 (control)
    pub ctrl: bool,
    /// Original: A4-0x3264 (caps lock)
    pub caps: bool,
    /// Original: A4-0x326e (left alt)
    pub lalt: bool,
    /// Original: A4-0x326c (right alt)
    pub ralt: bool,
    /// Original: A4-0x326a (left Amiga)
    pub lamiga: bool,
    /// Original: A4-0x3268 (right Amiga)
    pub ramiga: bool,
    /// Original: A4-0x3262 (number typed on the keypad while Alt is held)
    pub number: i16,
}

/// Original: 0x151d00 (input: raw key code → game code, tracking the qualifiers; 0 = nothing to queue)
///
/// `table` is the 0x60-entry map A4-0x3572 ([plain, shifted, control, alt] per raw code). A
/// release of Alt delivers the keypad number typed while it was held. With caps lock, letters
/// are upper-cased (the original consults the C type table at the code; for codes ≥ 0x100 that
/// reads past the table into zeroed memory, i.e. no change, as here). With Alt held, keypad
/// codes 0x14b..0x151 build the number instead (0x14d and 0x14e only multiply it by ten and
/// still come through, as in the original's jump table 0x151e76).
pub fn map_raw(q: &mut Qualifiers, table: &[[u16; 4]], raw: u16) -> u16 {
    let down = raw <= 0x7f;
    let raw = raw & 0x7f;
    if raw >= QUALIFIERS {
        match raw {
            0x60 => q.lshift = down,
            0x61 => q.rshift = down,
            0x62 => q.caps = down,
            0x63 => q.ctrl = down,
            0x64 | 0x65 => {
                if raw == 0x64 {
                    q.lalt = down;
                } else {
                    q.ralt = down;
                }
                if !down && q.number != 0 {
                    return std::mem::take(&mut q.number) as u16;
                }
            }
            0x66 => q.lamiga = down,
            _ => q.ramiga = down,
        }
        return 0;
    }
    if !down {
        return 0;
    }
    let entry = table.get(usize::from(raw)).copied().unwrap_or([0; 4]);
    let mut code = if q.ctrl {
        entry[2]
    } else if q.lshift || q.rshift {
        entry[1]
    } else if q.lalt || q.ralt {
        entry[3]
    } else {
        entry[0]
    };
    if q.caps && code < 0x80 && (code as u8).is_ascii_lowercase() {
        code -= 0x20;
    }
    if (q.lalt || q.ralt) && code > 0x14a && code < 0x152 {
        q.number = q.number.wrapping_mul(10);
        let add = match code {
            0x14b => 4,
            0x14c => 5,
            0x14f => 1,
            0x150 => 2,
            0x151 => 3,
            _ => return code,
        };
        q.number = q.number.wrapping_add(add);
        return 0;
    }
    code
}

/// Raw codes of the keys the engine names (Amiga keyboard hardware codes).
const RAW_UP: u16 = 0x4c;
const RAW_DOWN: u16 = 0x4d;
const RAW_RIGHT: u16 = 0x4e;
const RAW_LEFT: u16 = 0x4f;
const RAW_RETURN: u16 = 0x44;
const RAW_ESCAPE: u16 = 0x45;
const RAW_SPACE: u16 = 0x40;
const RAW_BACKSPACE: u16 = 0x41;

/// The raw codes a press of `key` sends: named keys by their hardware code; a character by the
/// first raw key whose plain entry in the game's own table is that character, else the first
/// whose shifted entry is (wrapped in a left-shift press and release). A character the table
/// cannot produce sends nothing.
pub fn raw_events(table: &[[u16; 4]], key: Key) -> Vec<u16> {
    let raw = match key {
        Key::Up => RAW_UP,
        Key::Down => RAW_DOWN,
        Key::Right => RAW_RIGHT,
        Key::Left => RAW_LEFT,
        Key::Enter => RAW_RETURN,
        Key::Escape => RAW_ESCAPE,
        Key::Space => RAW_SPACE,
        Key::Backspace => RAW_BACKSPACE,
        Key::Char(c) => {
            let code = u32::from(c);
            let find = |col: usize| {
                table
                    .iter()
                    .position(|e| u32::from(e[col]) == code && code != 0)
                    .map(|r| r as u16)
            };
            if let Some(r) = find(0) {
                return vec![r];
            }
            return match find(1) {
                Some(r) => vec![LSHIFT, r, LSHIFT | RELEASE],
                None => Vec::new(),
            };
        }
    };
    vec![raw]
}

/// The game code one key press produces: its raw codes through [`map_raw`], last non-zero.
///
/// Replaces: 0x151c60 (open input.device, install the handler), 0x151cdc (remove it), 0x14f0ac
/// (queue a code in the 50-entry ring): the platform queues [`Key`] events in the engine.
pub fn key_code(q: &mut Qualifiers, table: &[[u16; 4]], key: Key) -> u16 {
    raw_events(table, key)
        .into_iter()
        .map(|raw| map_raw(q, table, raw))
        .fold(0, |last, code| if code != 0 { code } else { last })
}

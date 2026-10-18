//! The script buffer, script loading 0x133e5e and portrait loading 0x133d3e.

use u6_formats::lzw;
use u6_formats::portrait::PORTRAIT_LEN;

use super::seams;
use super::{ConverseState, Msg};
use crate::engine::Io;
use crate::files::Files;
use crate::objects::ObjectId;
use crate::state::State;

/// Size of the script buffer 0x144240 (it ends BSS hunk 23 at 0x146a40); also the read limit of
/// a stored (not packed) script.
pub const SCRIPT_LEN: usize = 0x2800;
/// Byte 9 of `converse.?` / `portrait.?` is the archive letter the loaders patch in.
const NAME_LETTER: usize = 9;
/// Object types that talk although they are not actor slots < 0xe0, and the script item each
/// uses in `converse.b` / portrait slot (0x133e5e, 0x133d3e).
const SPECIAL_TALKERS: [(u16, u16, i16); 3] = [
    (0x175, 0x66, 0xc0),
    (0x17e, 0x67, 0xc1),
    (0x16b, 0x68, 0xc2),
];
/// Objlist-block offset of the hero's portrait number (A4-0x7b03, objlist 0x1C72, 1-based).
const TAIL_HERO_PORTRAIT: usize = 0x81;

impl ConverseState {
    /// The byte at signed position `pos`. Outside the buffer the port sets `fault` and yields 0.
    pub(crate) fn byte_at(&mut self, pos: i32) -> u8 {
        match usize::try_from(pos).ok().and_then(|p| self.script.get(p)) {
            Some(&b) => b,
            None => {
                self.fault = true;
                0
            }
        }
    }

    /// Writes one script byte (ops 0xa6 and 0xc1 patch the script); outside the buffer: dropped.
    pub(crate) fn set_byte(&mut self, pos: i32, v: u8) {
        match usize::try_from(pos)
            .ok()
            .and_then(|p| self.script.get_mut(p))
        {
            Some(b) => *b = v,
            None => self.fault = true,
        }
    }

    /// Little-endian word at `pos`.
    pub(crate) fn word_at(&mut self, pos: i32) -> u16 {
        u16::from(self.byte_at(pos)) | u16::from(self.byte_at(pos + 1)) << 8
    }

    /// Little-endian longword at `pos`.
    pub(crate) fn long_at(&mut self, pos: i32) -> u32 {
        (0..4).fold(0, |v, i| v | u32::from(self.byte_at(pos + i)) << (8 * i))
    }

    /// The read position as the signed word the 68000 adds to the buffer address.
    pub(crate) fn pos(&self) -> i32 {
        i32::from(self.pc as i16)
    }

    /// Sets the read position (word: the high bits are dropped as `move.w` does).
    pub(crate) fn set_pos(&mut self, pos: i32) {
        self.pc = pos as u16;
    }

    /// `script[pc++]`.
    pub(crate) fn next(&mut self) -> u8 {
        let b = self.byte_at(self.pos());
        self.pc = self.pc.wrapping_add(1);
        b
    }

    /// Moves the read position by `n` (word arithmetic).
    pub(crate) fn advance(&mut self, n: i16) {
        self.pc = self.pc.wrapping_add(n as u16);
    }
}

/// Where 0x133e5e writes: the script buffer 0x144240, or another buffer (0x112b20 reads a
/// script into the portrait area 0x13c940 to take the name out of it).
pub enum ScriptDst<'a> {
    Main,
    Other(&'a mut Vec<u8>),
}

/// Archive letter and item of `npc`'s script, or `None` for an object that has none.
fn script_item(state: &State, npc: i16) -> Option<(u8, u16)> {
    let ty = obj_type(state, npc);
    if npc >= 0xe0 {
        return SPECIAL_TALKERS
            .iter()
            .find(|&&(t, _, _)| t == ty)
            .map(|&(_, item, _)| (b'b', item));
    }
    Some(if npc > 0x62 {
        (b'b', (npc - 0x63) as u16)
    } else {
        (b'a', npc as u16)
    })
}

fn obj_type(state: &State, id: i16) -> u16 {
    u16::try_from(id).map_or(0, |id| state.objects.obj_type(ObjectId(id)))
}

/// The file name with the archive letter patched in.
fn archive(name: &[u8], letter: u8) -> String {
    let mut n = name.to_vec();
    if let Some(b) = n.get_mut(NAME_LETTER) {
        *b = letter;
    }
    n.iter().map(|&b| char::from(b)).collect()
}

/// Little-endian u32 at `at`, or `None` past the end (a short read).
fn le32(f: &[u8], at: usize) -> Option<usize> {
    let b = f.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

/// Original: 0x133e5e (load an NPC's converse.a script block)
///
/// Actor slots below 0x63 use item `npc` of `converse.a`, 0x63..0xe0 item `npc - 0x63` of
/// `converse.b`; slots from 0xe0 talk only as object type 0x175 / 0x17e / 0x16b (items 0x66,
/// 0x67, 0x68 of `converse.b`). An offset of 0 is an empty item (result 0). The item's first
/// longword is the unpacked size: 0 or ≥ 0x2800 means the next 0x2800 file bytes are read as
/// they are, otherwise they are a U6-LZW stream. Loading into the script buffer stops the music
/// first. Result: `npc` (non-zero) once loaded. Port divergences: a missing archive or a stream
/// that does not decode gives 0 (the NPC does not answer) where the original's disk-prompting open would
/// not fail; nothing is written past the destination buffer.
pub fn load_script(state: &mut State, files: &Files, npc: i16, dst: ScriptDst<'_>) -> i16 {
    let Some((letter, item)) = script_item(state, npc) else {
        return 0;
    };
    let name = state
        .converse
        .text
        .as_ref()
        .map(|t| t.get(Msg::ConverseFile).to_vec());
    let Some(file) = name.and_then(|n| files.get(&archive(&n, letter))) else {
        return 0;
    };
    let off = le32(file, usize::from(item) * 4).unwrap_or(0);
    if off == 0 {
        return 0;
    }
    let main = matches!(dst, ScriptDst::Main);
    if main {
        seams::play_song(state, 0);
    }
    let size = le32(file, off).unwrap_or(0);
    let bytes: Vec<u8> = if size == 0 || size >= SCRIPT_LEN {
        let start = (off + 4).min(file.len());
        file[start..(start + SCRIPT_LEN).min(file.len())].to_vec()
    } else {
        match lzw::decode(&file[off..]) {
            Ok(b) => b,
            Err(_) => return 0,
        }
    };
    let buf = match dst {
        ScriptDst::Main => &mut state.converse.script,
        ScriptDst::Other(b) => b,
    };
    if buf.len() < SCRIPT_LEN {
        buf.resize(SCRIPT_LEN, 0);
    }
    let n = bytes.len().min(buf.len());
    buf[..n].copy_from_slice(&bytes[..n]);
    #[cfg(test)]
    if main {
        super::disasm::trace::loaded(letter, item);
    }
    npc
}

/// The loading half of 0x133d3e ([`draw_portrait`]): 56×64 palette indices of `npc`'s portrait, or `None` (hero without a
/// portrait number, empty item, missing file). Object types 0x175 / 0x17e / 0x16b at slots from
/// 0xe0 use portrait slots 0xc0..0xc2; the hero (actor 1) uses item `n - 1` of `portrait.z`
/// where `n` is the objlist portrait byte; any other slot `s` uses item `s - 1` (slot 0: item 0)
/// of `portrait.a`, from 0x62 on item `s - 1 - 0x62` of `portrait.b`.
pub fn load_portrait(state: &State, files: &Files, npc: i16) -> Option<Vec<u8>> {
    let (letter, item) = portrait_ref(state, npc)?;
    portrait_pixels(state, files, letter, item)
}

/// The portrait [`load_portrait`] loads for `npc`: the archive letter (`portrait.<letter>`) and
/// the item in it; `None` for a hero without a portrait number.
pub fn portrait_ref(state: &State, npc: i16) -> Option<(u8, u16)> {
    let mut slot = npc;
    if slot >= 0xe0 {
        let ty = obj_type(state, npc);
        if let Some(&(_, _, s)) = SPECIAL_TALKERS.iter().find(|&&(t, _, _)| t == ty) {
            slot = s;
        }
    }
    let (letter, item) = if slot == 1 {
        let n = state
            .actors
            .objlist_tail
            .get(TAIL_HERO_PORTRAIT)
            .copied()
            .unwrap_or(0);
        (b'z', i16::from(n) - 1)
    } else {
        let s = if slot != 0 { slot - 1 } else { slot };
        if s >= 0x62 {
            (b'b', s - 0x62)
        } else {
            (b'a', s)
        }
    };
    u16::try_from(item).ok().map(|item| (letter, item))
}

/// Port-only (spec 07 H2): the archives [`portrait_ref`] names, `a` and `b` for the slots and `z`
/// for the hero.
pub const PORTRAIT_LETTERS: [u8; 3] = [b'a', b'b', b'z'];

/// Port-only (spec 07 H2): every portrait the game's archives hold, as (letter, item, its 56×64
/// palette indices): each item of the three archives' tables that is not empty and whose pixels
/// read ([`portrait_pixels`]), each read once. None when the portrait file's name is too short to
/// carry the archive letter. The HD tool packs these; the page counts them.
pub fn portraits(state: &State, files: &Files) -> Vec<(u8, u16, Vec<u8>)> {
    use u6_formats::u6lib::{EntryWidth, U6Lib};
    let Some(text) = state.converse.text.as_ref() else {
        return Vec::new();
    };
    let name = text.get(Msg::PortraitFile).to_vec();
    if name.len() <= NAME_LETTER {
        return Vec::new();
    }
    let mut out = Vec::new();
    for letter in PORTRAIT_LETTERS {
        let Some(file) = files.get(&archive(&name, letter)) else {
            continue;
        };
        let Ok(lib) = U6Lib::parse(file, EntryWidth::U32) else {
            continue;
        };
        for (item, it) in lib.items.iter().enumerate() {
            let (Some(off), Ok(item)) = (le32(file, item * 4), u16::try_from(item)) else {
                continue;
            };
            if it.is_empty() || off == 0 {
                continue;
            }
            if let Some(px) = portrait_pixels(state, files, letter, item) {
                out.push((letter, item, px));
            }
        }
    }
    out
}

/// The 56×64 palette indices of portrait `item` of `portrait.<letter>` ([`portrait_ref`]), or
/// `None` (missing file, empty item).
pub fn portrait_pixels(state: &State, files: &Files, letter: u8, item: u16) -> Option<Vec<u8>> {
    let name = state
        .converse
        .text
        .as_ref()?
        .get(Msg::PortraitFile)
        .to_vec();
    let file = files.get(&archive(&name, letter))?;
    let off = le32(file, usize::from(item) * 4)?;
    let pixels = lzw::decode(file.get(off..)?).ok()?;
    (pixels.len() >= PORTRAIT_LEN).then(|| pixels[..PORTRAIT_LEN].to_vec())
}

/// Original: 0x133d3e (load an NPC portrait (portrait.a))
///
/// Loads `npc`'s portrait ([`load_portrait`]) and hands it (56×64; the original writes the
/// 0x38 / 0x40 size words in front of the pixels) to the bitmap blit 0x1533e0 at (`x`, `y`).
/// The source plane names it by its (letter, item) ([`u6_render::portrait_id`]).
pub fn draw_portrait(state: &State, io: &Io, npc: i16, x: i16, y: i16) {
    let Some((letter, item)) = portrait_ref(state, npc) else {
        return;
    };
    if let Some(pixels) = portrait_pixels(state, io.files(), letter, item) {
        let src = u6_render::portrait_id(letter, item).map_or(
            crate::gfx::blit::BitmapSource::Classic,
            crate::gfx::blit::BitmapSource::Portrait,
        );
        seams::blit_portrait(state, io, &pixels, src, x, y);
    }
}

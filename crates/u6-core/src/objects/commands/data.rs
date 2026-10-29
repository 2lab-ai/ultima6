//! The user's `game` executable as the loaded program holds it, for the strings and tables the
//! object commands read (spec 03 §5, data not constants): every hunk at its image address from
//! 0x100000 (re/notes/game-layout.md), BSS read as 0 and every RELOC32 longword relocated as
//! the AmigaDOS loader leaves it. Built once at load from the dropped file; nothing of it is
//! compiled in.

use std::fmt;
use std::rc::Rc;

use crate::image::{A4, GameImage, IMAGE_BASE};

/// Hunks of the measured `game` and the image address of hunk 33 (the merged data hunk); a
/// file with another layout is refused, as `objects::defs` does.
const GAME_HUNKS: usize = 35;
const HUNK33_BASE: u32 = 0x0015_36f0;

/// The loaded image, shared (cloning a `State` does not copy it).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct GameData {
    flat: Rc<[u8]>,
}

impl fmt::Debug for GameData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GameData({} bytes)", self.flat.len())
    }
}

impl GameData {
    /// Maps `game` (the decrunched hunk file) into one flat image.
    pub fn from_game(game: &[u8]) -> Result<Self, String> {
        let img = GameImage::parse(game).map_err(|e| e.to_string())?;
        if img.segment_count() != GAME_HUNKS || img.segment_base(33) != Some(HUNK33_BASE) {
            return Err("unknown game version (hunk layout differs from the measured one)".into());
        }
        let mut flat = Vec::new();
        for n in 0..img.segment_count() {
            let base = img.segment_base(n).unwrap_or(IMAGE_BASE);
            let at = (base - IMAGE_BASE) as usize;
            let loaded = img.loaded(n).unwrap_or(&[]);
            let end = (n + 1 < img.segment_count())
                .then(|| img.segment_base(n + 1))
                .flatten()
                .map_or(at + loaded.len(), |b| (b - IMAGE_BASE) as usize);
            flat.resize(end.max(at + loaded.len()), 0);
            flat[at..at + loaded.len()].copy_from_slice(loaded);
            for off in (0..loaded.len().saturating_sub(3)).step_by(2) {
                let addr = base + off as u32;
                let raw = &loaded[off..off + 4];
                let raw = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
                // Only relocated longwords change; an unrelocated neighbour overlapping one
                // must not write its file bytes back over it.
                match img.pointer(addr) {
                    Ok(p) if p != raw => {
                        flat[at + off..at + off + 4].copy_from_slice(&p.to_be_bytes())
                    }
                    _ => {}
                }
            }
        }
        Ok(Self { flat: flat.into() })
    }

    /// A synthetic image for tests: `bytes` placed at image address `addr`.
    #[cfg(test)]
    pub fn with_bytes(parts: &[(u32, &[u8])]) -> Self {
        let mut flat = Vec::new();
        for &(addr, bytes) in parts {
            let at = (addr - IMAGE_BASE) as usize;
            if flat.len() < at + bytes.len() {
                flat.resize(at + bytes.len(), 0);
            }
            flat[at..at + bytes.len()].copy_from_slice(bytes);
        }
        Self { flat: flat.into() }
    }

    /// Byte at image address `addr` (0 outside the image).
    pub fn byte(&self, addr: u32) -> u8 {
        addr.checked_sub(IMAGE_BASE)
            .and_then(|o| self.flat.get(o as usize))
            .copied()
            .unwrap_or(0)
    }

    /// Big-endian word at `addr`.
    pub fn word(&self, addr: u32) -> u16 {
        u16::from_be_bytes([self.byte(addr), self.byte(addr.wrapping_add(1))])
    }

    /// Big-endian longword at `addr` (a pointer reads relocated).
    pub fn long(&self, addr: u32) -> u32 {
        u32::from(self.word(addr)) << 16 | u32::from(self.word(addr.wrapping_add(2)))
    }

    /// The NUL-terminated string at `addr` (at most 256 bytes; empty outside the image).
    pub fn cstr(&self, addr: u32) -> Vec<u8> {
        (0..256u32)
            .map(|i| self.byte(addr.wrapping_add(i)))
            .take_while(|&b| b != 0)
            .collect()
    }

    /// The string the pointer at `addr` names.
    pub fn pointed(&self, addr: crate::string_registry::Text) -> Vec<u8> {
        addr.data(self)
    }

    /// Image address `A4 + off`.
    pub const fn a4(off: i32) -> u32 {
        A4.wrapping_add_signed(off)
    }

    /// Byte at `A4 + off`.
    pub fn a4_byte(&self, off: i32) -> u8 {
        self.byte(Self::a4(off))
    }

    /// Word at `A4 + off`.
    pub fn a4_word(&self, off: i32) -> u16 {
        self.word(Self::a4(off))
    }

    /// True once a `game` file was mapped.
    pub fn is_loaded(&self) -> bool {
        !self.flat.is_empty()
    }
}

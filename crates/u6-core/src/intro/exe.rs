//! The user's `u` executable as the loaded program sees it: every string, table and initial
//! global the intro reads comes from here at run time (spec 03 §5, "data, not constants").
//!
//! `u` is a SAS/C small-data program like `game`: hunks laid out from 0x00100000 (the same
//! loader as [`crate::image::GameImage`]), the small-data base A4 = the start of its data hunk
//! 22, image address 0x00116798 (`lea $116798.l,a4` at the entry 0x100008). The port copies the
//! loaded image once (BSS reads 0) and resolves the few stored pointers it needs through the
//! relocation table when it builds the copy.

use u6_formats::FormatError;

use crate::image::{GameImage, IMAGE_BASE};

/// Small-data base register A4 of `u`: `A4 + off` = image `0x116798 + off`.
pub const A4: u32 = 0x0011_6798;

/// The loaded image of `u`, with the stored pointers it needs resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UExe {
    /// Image bytes from [`IMAGE_BASE`] to the end of the last hunk (BSS and gaps read 0).
    flat: Vec<u8>,
    /// Original: A4+0xa0 (pointers to the song file names, index 1..=12 = 0x1036bc's argument)
    pub song_names: Vec<Vec<u8>>,
    /// Original: A4+0x8ac (pointers to the volume names the disk prompt 999 names), relocated
    pub volume_ptrs: Vec<u32>,
}

fn bad(detail: String) -> FormatError {
    FormatError::Invalid {
        what: "u hunk file",
        offset: 0,
        detail,
    }
}

impl UExe {
    /// Maps the decrunched `u` load file. Fails when it is not a hunk file or has no data hunk
    /// at A4.
    pub fn parse(file: &[u8]) -> Result<Self, FormatError> {
        let img = GameImage::parse(file)?;
        let n = img.segment_count();
        if n == 0 {
            return Err(bad("no hunks".into()));
        }
        // Each hunk: its loaded bytes at its base, then its BSS part (reads 0) up to its size,
        // found as the first address past the loaded bytes the image no longer maps.
        let mut parts = Vec::with_capacity(n);
        let mut top = IMAGE_BASE;
        for i in 0..n {
            let base = img.segment_base(i).unwrap_or(IMAGE_BASE);
            let loaded = img.loaded(i).unwrap_or(&[]);
            let next = img.segment_base(i + 1).unwrap_or(u32::MAX);
            let mut end = base + loaded.len() as u32;
            while end < next && img.read_zero_filled(end, 1).is_ok() {
                end += 1;
            }
            top = top.max(end);
            parts.push((base, loaded));
        }
        let mut flat = vec![0u8; (top - IMAGE_BASE) as usize];
        for &(base, loaded) in &parts {
            let o = (base - IMAGE_BASE) as usize;
            flat[o..o + loaded.len()].copy_from_slice(loaded);
        }
        // Relocate as the loader does: a longword with a RELOC32 entry gets its target hunk's
        // base added (the `jmp abs.l` stubs of the code and the stored pointers then read as
        // image addresses). Relocations sit on even offsets and never overlap.
        for &(base, loaded) in &parts {
            for off in (0..loaded.len().saturating_sub(3)).step_by(2) {
                let at = base + off as u32;
                let raw = u32::from_be_bytes([
                    loaded[off],
                    loaded[off + 1],
                    loaded[off + 2],
                    loaded[off + 3],
                ]);
                if let Ok(v) = img.pointer(at)
                    && v != raw
                {
                    let o = (at - IMAGE_BASE) as usize;
                    flat[o..o + 4].copy_from_slice(&v.to_be_bytes());
                }
            }
        }
        if img.segment_base(22) != Some(A4) {
            return Err(bad(format!(
                "data hunk 22 at {:?}, expected {A4:#x}",
                img.segment_base(22)
            )));
        }
        let pointers = |table: u32, count: usize| -> Vec<u32> {
            (0..count)
                .map(|i| img.pointer(table + 4 * i as u32).unwrap_or(0))
                .collect()
        };
        let song_names = pointers(A4 + 0xa0, 13)
            .into_iter()
            .map(|p| img.cstr(p).unwrap_or_default())
            .collect();
        Ok(Self {
            song_names,
            volume_ptrs: pointers(A4 + 0x8ac, 4),
            flat,
        })
    }

    /// The byte at image address `addr` (0 outside the image).
    pub fn byte(&self, addr: u32) -> u8 {
        addr.checked_sub(IMAGE_BASE)
            .and_then(|o| self.flat.get(o as usize))
            .copied()
            .unwrap_or(0)
    }

    /// Big-endian word at image address `addr`.
    pub fn word(&self, addr: u32) -> u16 {
        u16::from_be_bytes([self.byte(addr), self.byte(addr.wrapping_add(1))])
    }

    /// Big-endian signed word at `A4 + off`. The sum wraps as the 68000's 32-bit address sum
    /// does, so an `off` that is a negative index as `u32` reads below A4 (0 outside the image).
    pub fn a4_i16(&self, off: u32) -> i16 {
        self.word(A4.wrapping_add(off)) as i16
    }

    /// The `n` signed words from `A4 + off` on.
    pub fn a4_words(&self, off: u32, n: usize) -> Vec<i16> {
        (0..n).map(|i| self.a4_i16(off + 2 * i as u32)).collect()
    }

    /// `n` bytes from image address `addr` on (the address wraps past 0xffffffff, as
    /// [`UExe::word`]'s does).
    pub fn bytes(&self, addr: u32, n: usize) -> Vec<u8> {
        (0..n)
            .map(|i| self.byte(addr.wrapping_add(i as u32)))
            .collect()
    }

    /// The NUL-terminated string at image address `addr` (at most 512 bytes, no NUL).
    ///
    /// A passage case of `u` passes its own pointers here (the volume-name table A4+0x8ac, a
    /// pushed format). The address sum wraps like [`UExe::bytes`]; it cannot wrap in practice,
    /// since a byte outside the image reads 0 and ends the string before the sum gets past
    /// 0xffffffff.
    pub fn cstr(&self, addr: u32) -> Vec<u8> {
        (0..512)
            .map(|i| self.byte(addr.wrapping_add(i)))
            .take_while(|&b| b != 0)
            .collect()
    }

    /// A 32-colour palette of the table at A4+0xb92 (64 bytes each; 0x113fb2 / 0x113fce /
    /// 0x113fea index it).
    pub fn palette(&self, n: u16) -> [u16; 32] {
        let base = A4 + 0xb92 + u32::from(n) * 64;
        std::array::from_fn(|i| self.word(base + 2 * i as u32))
    }

    /// A synthetic image for tests: `flat` laid out from [`IMAGE_BASE`].
    #[cfg(test)]
    pub(crate) fn from_flat(flat: Vec<u8>) -> Self {
        Self {
            flat,
            song_names: Vec::new(),
            volume_ptrs: Vec::new(),
        }
    }

    /// Writes bytes into a synthetic image (tests).
    #[cfg(test)]
    pub(crate) fn poke(&mut self, addr: u32, data: &[u8]) {
        let o = (addr - IMAGE_BASE) as usize;
        if self.flat.len() < o + data.len() {
            self.flat.resize(o + data.len(), 0);
        }
        self.flat[o..o + data.len()].copy_from_slice(data);
    }
}

//! Read-only view of the user's `game` executable as the loaded memory image (spec 03 §5, "data,
//! not constants"): the one hunk loader every module reads its tables through.
//!
//! The modules need strings and small tables that live inside `game` (panel format strings, the
//! raw-key table, colours, walk tables, object definitions, converse messages). They are read
//! here at run time from the user's own PowerPacker-decrunched `game`, never compiled in. Addresses are image addresses as in the RE
//! notes: AmigaDOS hunk segments laid out one after another from 0x00100000, each base aligned
//! to 8 bytes (re/notes/game-layout.md, `tools/hunk2raw.py`). Bytes are the file's (not
//! relocated); a stored pointer is resolved with [`GameImage::pointer`], which applies the
//! segment's RELOC32 entry for that longword as the loader would.

use std::borrow::Cow;

use u6_formats::FormatError;

/// Load address of hunk 0 in the RE image.
pub const IMAGE_BASE: u32 = 0x0010_0000;
/// SAS/C small-data base register A4 (subsystems.md): `A4 + off` = image `0x15B6F0 + off`.
pub const A4: u32 = 0x0015_B6F0;

const HUNK_CODE: u32 = 0x3E9;
const HUNK_DATA: u32 = 0x3EA;
const HUNK_BSS: u32 = 0x3EB;
const HUNK_RELOC32: u32 = 0x3EC;
const HUNK_END: u32 = 0x3F2;
const HUNK_HEADER: u32 = 0x3F3;
/// Bits 30/31 of a size or type longword carry memory flags.
const TYPE_MASK: u32 = 0x3FFF_FFFF;

/// One segment: its image base, allocated size and the loaded bytes (file range).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Segment {
    base: u32,
    size: u32,
    file_start: usize,
    loaded: usize,
}

/// The executable mapped by image address.
#[derive(Debug)]
pub struct GameImage<'a> {
    file: &'a [u8],
    segments: Cow<'a, [Segment]>,
    /// RELOC32 entries: (image address of the longword, target segment), sorted.
    relocs: Cow<'a, [(u32, usize)]>,
}

/// What [`GameImage::parse`] works out of a file (the segment table and the sorted RELOC32
/// entries), kept so the same file can be viewed again without parsing it
/// ([`GameImage::with_layout`]; the engine keeps one per run, `Io::game_image`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageLayout {
    segments: Vec<Segment>,
    relocs: Vec<(u32, usize)>,
}

fn bad(offset: usize, detail: impl Into<String>) -> FormatError {
    FormatError::Invalid {
        what: "game hunk file",
        offset,
        detail: detail.into(),
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn long(&mut self) -> Result<u32, FormatError> {
        let b = self
            .data
            .get(self.pos..self.pos + 4)
            .ok_or(FormatError::Truncated {
                what: "game hunk file",
                offset: self.pos,
                needed: 4,
                len: self.data.len(),
            })?;
        self.pos += 4;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn skip_longs(&mut self, n: u32) -> Result<(), FormatError> {
        let bytes = usize::try_from(n)
            .ok()
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| bad(self.pos, "block length overflows"))?;
        if self.pos + bytes > self.data.len() {
            return Err(bad(
                self.pos,
                format!("block of {bytes} bytes runs past the end"),
            ));
        }
        self.pos += bytes;
        Ok(())
    }
}

impl<'a> GameImage<'a> {
    /// Maps a decrunched AmigaDOS load file (HUNK_HEADER, then CODE / DATA / BSS blocks with
    /// RELOC32 and END). Fails on any other block type or a truncated file.
    pub fn parse(file: &'a [u8]) -> Result<Self, FormatError> {
        let mut r = Reader { data: file, pos: 0 };
        if r.long()? != HUNK_HEADER {
            return Err(bad(0, "no HUNK_HEADER"));
        }
        // Resident library names: a zero longword ends the list (the game has none).
        loop {
            let n = r.long()?;
            if n == 0 {
                break;
            }
            r.skip_longs(n)?;
        }
        let _table_size = r.long()?;
        let first = r.long()?;
        let last = r.long()?;
        if last < first || last - first > 255 {
            return Err(bad(r.pos, format!("hunk range {first}..={last}")));
        }
        let mut segments = Vec::new();
        let mut base = IMAGE_BASE;
        for _ in first..=last {
            let word = r.long()?;
            if word & 0xC000_0000 == 0xC000_0000 {
                r.long()?; // extra memory attribute longword
            }
            let size = (word & TYPE_MASK)
                .checked_mul(4)
                .ok_or_else(|| bad(r.pos, "hunk size overflows"))?;
            segments.push(Segment {
                base,
                size,
                file_start: 0,
                loaded: 0,
            });
            base = base
                .checked_add(size)
                .and_then(|b| b.checked_add(7))
                .ok_or_else(|| bad(r.pos, "image overflows"))?
                & !7;
        }
        let mut relocs = Vec::new();
        for seg in &mut segments {
            loop {
                let at = r.pos;
                match r.long()? & TYPE_MASK {
                    HUNK_CODE | HUNK_DATA => {
                        let n = r.long()?;
                        seg.file_start = r.pos;
                        r.skip_longs(n)?;
                        seg.loaded = r.pos - seg.file_start;
                        if seg.loaded > seg.size as usize {
                            return Err(bad(at, "loaded bytes exceed the hunk size"));
                        }
                    }
                    HUNK_BSS => {
                        r.long()?;
                    }
                    HUNK_RELOC32 => loop {
                        let n = r.long()?;
                        if n == 0 {
                            break;
                        }
                        let target = r.long()?.wrapping_sub(first) as usize;
                        for _ in 0..n {
                            let off = r.long()?;
                            relocs.push((seg.base.wrapping_add(off), target));
                        }
                    },
                    HUNK_END => break,
                    other => return Err(bad(at, format!("unsupported block {other:#x}"))),
                }
            }
        }
        relocs.sort_unstable();
        if let Some(&(at, t)) = relocs.iter().find(|&&(_, t)| t >= segments.len()) {
            return Err(bad(0, format!("reloc at {at:#x} targets hunk {t}")));
        }
        Ok(GameImage {
            file,
            segments: Cow::Owned(segments),
            relocs: Cow::Owned(relocs),
        })
    }

    /// The parsed layout, to view the same file again later ([`GameImage::with_layout`]).
    pub fn into_layout(self) -> ImageLayout {
        ImageLayout {
            segments: self.segments.into_owned(),
            relocs: self.relocs.into_owned(),
        }
    }

    /// `file` viewed through a layout [`GameImage::parse`] made of it earlier.
    pub fn with_layout(file: &'a [u8], layout: &'a ImageLayout) -> Self {
        GameImage {
            file,
            segments: Cow::Borrowed(&layout.segments),
            relocs: Cow::Borrowed(&layout.relocs),
        }
    }

    /// Number of segments (hunks) in the file.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Image base of segment `n`.
    pub fn segment_base(&self, n: usize) -> Option<u32> {
        self.segments.get(n).map(|s| s.base)
    }

    /// The loaded bytes of segment `n` (a CODE or DATA hunk; empty for BSS).
    pub fn loaded(&self, n: usize) -> Option<&'a [u8]> {
        let s = self.segments.get(n)?;
        self.file.get(s.file_start..s.file_start + s.loaded)
    }

    /// `len` bytes at image address `addr` as the loaded program holds them: the allocated part
    /// of a segment past its loaded bytes (BSS) reads 0. Fails outside every segment.
    pub fn read_zero_filled(&self, addr: u32, len: usize) -> Result<Vec<u8>, FormatError> {
        (0..len)
            .map(|i| {
                let a = addr.wrapping_add(i as u32);
                let seg = self
                    .segments
                    .iter()
                    .find(|s| a >= s.base && a < s.base + s.size)
                    .ok_or_else(|| bad(0, format!("address {a:#x} is in no hunk")))?;
                let off = (a - seg.base) as usize;
                Ok(if off < seg.loaded {
                    self.file[seg.file_start + off]
                } else {
                    0
                })
            })
            .collect()
    }

    /// File offset of image address `addr`, if it lies in a segment's loaded bytes.
    pub fn file_offset(&self, addr: u32) -> Option<usize> {
        let seg = self
            .segments
            .iter()
            .find(|s| addr >= s.base && addr < s.base + s.size)?;
        let off = (addr - seg.base) as usize;
        (off < seg.loaded).then(|| seg.file_start + off)
    }

    /// `len` loaded bytes at image address `addr` (all inside one segment's loaded part).
    pub fn bytes(&self, addr: u32, len: usize) -> Result<&'a [u8], FormatError> {
        let start = self
            .file_offset(addr)
            .ok_or_else(|| bad(0, format!("image address {addr:#x} is not loaded data")))?;
        let end_addr = addr + len.saturating_sub(1) as u32;
        match self.file_offset(end_addr) {
            Some(end) if end + 1 == start + len => Ok(&self.file[start..start + len]),
            _ => Err(bad(
                start,
                format!("{len} bytes at {addr:#x} leave the segment"),
            )),
        }
    }

    /// The NUL-terminated string at image address `addr` (without the NUL; at most 256 bytes).
    pub fn cstr(&self, addr: u32) -> Result<Vec<u8>, FormatError> {
        let start = self
            .file_offset(addr)
            .ok_or_else(|| bad(0, format!("string at {addr:#x} is not loaded data")))?;
        let rest = &self.file[start..];
        let n = rest
            .iter()
            .take(256)
            .position(|&b| b == 0)
            .ok_or_else(|| bad(start, format!("string at {addr:#x} has no NUL")))?;
        Ok(rest[..n].to_vec())
    }

    /// The pointer stored at image address `addr`, relocated: the file holds an offset into the
    /// target segment, the loader adds that segment's base. A longword without a RELOC32 entry
    /// is returned as stored.
    pub fn pointer(&self, addr: u32) -> Result<u32, FormatError> {
        let b = self.bytes(addr, 4)?;
        let raw = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        Ok(match self.relocs.binary_search_by_key(&addr, |&(a, _)| a) {
            Ok(i) => raw.wrapping_add(self.segments[self.relocs[i].1].base),
            Err(_) => raw,
        })
    }

    /// Big-endian word at `A4 + off`.
    pub fn a4_word(&self, off: i32) -> Result<u16, FormatError> {
        let b = self.bytes(A4.wrapping_add_signed(off), 2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    /// `len` bytes at `A4 + off`.
    pub fn a4_bytes(&self, off: i32, len: usize) -> Result<&'a [u8], FormatError> {
        self.bytes(A4.wrapping_add_signed(off), len)
    }
}

/// Builds hunk files for tests: segments in order, each CODE / DATA with its bytes or BSS with a
/// size; RELOC32 is not written.
#[cfg(test)]
pub(crate) struct SyntheticGame {
    segments: Vec<(u32, Vec<u8>, u32)>,
}

#[cfg(test)]
impl SyntheticGame {
    pub(crate) fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// A CODE (`data` false) or DATA hunk holding `bytes` (a multiple of 4).
    pub(crate) fn loaded(mut self, data: bool, bytes: Vec<u8>) -> Self {
        let kind = if data { HUNK_DATA } else { HUNK_CODE };
        let size = bytes.len() as u32;
        self.segments.push((kind, bytes, size));
        self
    }

    /// A BSS hunk of `size` bytes (a multiple of 4).
    pub(crate) fn bss(mut self, size: u32) -> Self {
        self.segments.push((HUNK_BSS, Vec::new(), size));
        self
    }

    /// The segment sizes that put segment `n` at image address `base` after the ones so far
    /// (one BSS gap), then `count` empty BSS hunks up to `n`.
    pub(crate) fn place_at(mut self, n: usize, base: u32) -> Self {
        let mut at = IMAGE_BASE;
        for (_, _, size) in &self.segments {
            at = (at + size + 7) & !7;
        }
        while self.segments.len() + 1 < n {
            self.segments.push((HUNK_BSS, Vec::new(), 0));
        }
        if self.segments.len() < n {
            self.segments.push((HUNK_BSS, Vec::new(), base - at));
        }
        self
    }

    pub(crate) fn build(&self) -> Vec<u8> {
        let mut f = Vec::new();
        let l = |f: &mut Vec<u8>, v: u32| f.extend_from_slice(&v.to_be_bytes());
        let n = self.segments.len() as u32;
        for v in [HUNK_HEADER, 0, n, 0, n - 1] {
            l(&mut f, v);
        }
        for (_, _, size) in &self.segments {
            l(&mut f, size / 4);
        }
        for (kind, bytes, size) in &self.segments {
            l(&mut f, *kind);
            if *kind == HUNK_BSS {
                l(&mut f, size / 4);
            } else {
                l(&mut f, bytes.len() as u32 / 4);
                f.extend_from_slice(bytes);
            }
            l(&mut f, HUNK_END);
        }
        f
    }
}

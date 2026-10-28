//! Spec 18 (saved with the game, unit C3), port-only, no original address: the remembered sight
//! as the file a Save writes beside the savegame (`crate::saveload::SIGHT_FILE`) and load state
//! reads back with the save it belongs to.
//!
//! Version 1, little-endian, fixed widths:
//! - a header of [`HEADER`] bytes: [`MAGIC`], [`VERSION`] (u32), the length (u32) and FNV-1a 64
//!   digest (u64) of the `savegame/objlist` bytes the Save wrote with it, the game hour of the
//!   last sweep (u64, [`NO_SWEEP`] for none), then the counts of copies, cells and layers (u32
//!   each);
//! - each copy in slot order ([`COPY`] bytes): its record (u16), its 256 pixels, its 256
//!   transparency flags as 32 bytes of bits (pixel i is bit i mod 8 of byte i / 8);
//! - each cell in increasing key order ([`cell_key`]): its key (u32), the minute of its last sight
//!   (u32), its layer count (u16, at least 1), then each layer ([`LAYER`] bytes): its record
//!   (u16), masktype, overlay frame (a byte each) and copy slot (u16, [`NO_COPY`] for none).
//!
//! That is what [`SightMemory`] needs for the same picture and the same fading; the rest is
//! rebuilt: the index of copies by content from the copies (in slot order, as they were made),
//! the last-version index empty (it is keyed by the records' versions, which start again at 0 in
//! a new engine), the scratch empty. A file that is malformed in any way, of another version, or
//! of another objlist is not read at all ([`SightFileError`]).

use std::collections::HashMap;

use super::{
    Frozen, Layer, MAX_CELLS, MAX_COPIES, Remembered, RenderCache, SightMemory, content_hash,
};
use crate::map::{DUNGEON_MASK, SURFACE_MASK};
use u6_formats::gfx::{TILE_COUNT, TILE_H, TILE_W};
use u6_formats::map::DUNGEON_LEVELS;

/// The file's first 8 bytes, read as a little-endian u64: `u6sight` and a zero byte.
pub const MAGIC: u64 = 0x0074_6867_6973_3675;
/// The format this reads and writes.
pub const VERSION: u32 = 1;
/// The most layers a file holds, all cells together: twice [`MAX_CELLS`]. Five game days of the
/// Wide walk stacked 876 of 169 762 cells (spec 18, Cost).
pub const MAX_SAVED_LAYERS: usize = 2 * MAX_CELLS;
/// The header's bytes.
const HEADER: usize = 44;
/// A copy's pixels, a record's 16 × 16.
const PIXELS: usize = 256;
/// A copy's bytes of transparency bits.
const BITS: usize = 32;
const _: () = assert!(
    PIXELS == TILE_W * TILE_H && BITS * 8 == PIXELS,
    "a record's pixels"
);
/// A copy's bytes: its record, its pixels, its transparency bits.
const COPY: usize = 2 + PIXELS + BITS;
/// A cell's bytes before its layers: its key, its minute, its layer count.
const CELL: usize = 10;
/// A layer's bytes.
const LAYER: usize = 6;
/// The copy slot of a layer without a copy.
const NO_COPY: u16 = 0xffff;
/// The sweep hour of a memory never swept.
const NO_SWEEP: u64 = u64::MAX;
/// The largest file: [`MAX_COPIES`] copies, [`MAX_CELLS`] cells and [`MAX_SAVED_LAYERS`]
/// layers, 7 421 996 bytes.
pub const MAX_SIGHT_FILE_BYTES: usize =
    HEADER + COPY * MAX_COPIES + CELL * MAX_CELLS + LAYER * MAX_SAVED_LAYERS;

const _: () = assert!(MAX_COPIES <= NO_COPY as usize, "a slot is never NO_COPY");

/// Why a sight file is not read (counts and indices only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SightFileError {
    /// Shorter than its header.
    Short,
    /// Not [`MAGIC`].
    Magic,
    /// Another version than [`VERSION`].
    Version(u32),
    /// Written with another `savegame/objlist` (its length or digest).
    OtherSave,
    /// A count past its bound, or fewer layers than cells.
    Counts,
    /// Not exactly the bytes its counts make.
    Length,
    /// Copy `n`: its record past the records.
    Copy(usize),
    /// Cell `n`: its key not above the one before or out of range, or no layer.
    Cell(usize),
    /// Layer `n`: its record past the records or its slot past the copies, or the layers not the
    /// header's count.
    Layer(usize),
}

/// The FNV-1a 64 digest of `bytes` (the copies' content hash over them alone).
fn digest(bytes: &[u8]) -> u64 {
    content_hash(bytes, &[])
}

/// Whether `k` names a cell of the world: a level 0..=5, x and y within its mask (the surface's
/// 1024 cells a side, a dungeon's 256).
fn key_in_range(k: u32) -> bool {
    let (z, x, y) = (k >> 20, k >> 10 & 0x3ff, k & 0x3ff);
    let mask = u32::from(if z == 0 { SURFACE_MASK } else { DUNGEON_MASK });
    z as usize <= DUNGEON_LEVELS && x <= mask && y <= mask
}

impl RenderCache {
    /// Spec 18 (saved with the game): the remembered sight as the file a Save writes beside the
    /// save whose `savegame/objlist` is `objlist` ([`saved`](self)); `None` when it holds no
    /// cell (nothing to keep: the switch off, or nothing seen yet).
    pub fn sight_file(&self, objlist: &[u8]) -> Option<Vec<u8>> {
        encode(&self.memory, objlist)
    }

    /// Spec 18 (saved with the game): `memory` as the remembered sight, read by load state right
    /// after it forgot the old one ([`RenderCache::forget_sight`]); the window memo goes with
    /// the old one (its key names copies by slot).
    pub fn set_sight(&mut self, memory: SightMemory) {
        self.memory = memory;
        self.window = None;
    }
}

/// The layers a cell is written with: its own, `None` for a stacked cell whose stack is missing
/// (it shows nothing) or that has more layers than a count holds (neither happens in play).
fn written<'a>(m: &'a SightMemory, k: u32, c: &'a Remembered) -> Option<&'a [Layer]> {
    m.layers_of(k, c).filter(|l| u16::try_from(l.len()).is_ok())
}

/// The keys of the cells the file holds, in increasing order: all of them while their layers fit
/// [`MAX_SAVED_LAYERS`]; past it, the newest sights first, as the cap keeps them (of one minute,
/// the highest keys), until the next would not fit.
fn kept(m: &SightMemory) -> Vec<u32> {
    let mut cells: Vec<(u32, u32, usize)> = (m.cells.iter())
        .filter_map(|(&k, c)| written(m, k, c).map(|l| (k, c.seen, l.len())))
        .collect();
    let total: usize = cells.iter().map(|c| c.2).sum();
    if total > MAX_SAVED_LAYERS {
        cells.sort_unstable_by_key(|c| std::cmp::Reverse((c.1, c.0)));
        let mut sum = 0;
        let fit = cells
            .iter()
            .take_while(|c| {
                sum += c.2;
                sum <= MAX_SAVED_LAYERS
            })
            .count();
        cells.truncate(fit);
    }
    let mut keys: Vec<u32> = cells.into_iter().map(|c| c.0).collect();
    keys.sort_unstable();
    keys
}

/// The file of memory `m` bound to `objlist` (the module docs); `None` when it holds no cell.
fn encode(m: &SightMemory, objlist: &[u8]) -> Option<Vec<u8>> {
    if m.cells.is_empty() {
        return None;
    }
    let keys = kept(m);
    let cells: Vec<(u32, &Remembered, &[Layer])> = (keys.iter())
        .filter_map(|k| {
            let c = m.cells.get(k)?;
            Some((*k, c, written(m, *k, c)?))
        })
        .collect();
    let layers: usize = cells.iter().map(|c| c.2.len()).sum();
    let size = HEADER + COPY * m.frozen.len() + CELL * cells.len() + LAYER * layers;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(objlist.len() as u32).to_le_bytes());
    out.extend_from_slice(&digest(objlist).to_le_bytes());
    out.extend_from_slice(&m.swept.unwrap_or(NO_SWEEP).to_le_bytes());
    for n in [m.frozen.len(), cells.len(), layers] {
        out.extend_from_slice(&(n as u32).to_le_bytes());
    }
    for f in &m.frozen {
        out.extend_from_slice(&f.record.to_le_bytes());
        out.extend((0..PIXELS).map(|i| f.pixels.get(i).copied().unwrap_or(0)));
        out.extend((0..BITS).map(|b| {
            (0..8).fold(0u8, |bits, i| {
                let clear = f.transparent.get(b * 8 + i).copied().unwrap_or(false);
                bits | u8::from(clear) << i
            })
        }));
    }
    for (k, c, l) in cells {
        out.extend_from_slice(&k.to_le_bytes());
        out.extend_from_slice(&c.seen.to_le_bytes());
        out.extend_from_slice(&(l.len() as u16).to_le_bytes());
        for layer in l {
            out.extend_from_slice(&layer.record.to_le_bytes());
            out.extend([layer.masktype, layer.frame]);
            out.extend_from_slice(&layer.frozen.unwrap_or(NO_COPY).to_le_bytes());
        }
    }
    debug_assert_eq!(out.len(), size, "the file's bytes are its counts'");
    Some(out)
}

/// A cursor over a file whose length was checked against its counts first.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let (head, rest) = self.0.split_at(N);
        self.0 = rest;
        head.try_into().unwrap_or([0; N])
    }

    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take())
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take())
    }

    fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take())
    }
}

/// Spec 18 (saved with the game): the remembered sight a sight file holds, when it was written
/// with `objlist` (the `savegame/objlist` load state has just read) and is well formed in every
/// part ([`SightFileError`] otherwise: then nothing of it is read). The reading build's cap
/// applies at once ([`SightMemory::cap`]: a file of Original's 121 cells a render read in Wide's
/// 777).
pub fn read_sight(file: &[u8], objlist: &[u8]) -> Result<SightMemory, SightFileError> {
    if file.len() < HEADER {
        return Err(SightFileError::Short);
    }
    let mut b = Bytes(file);
    if b.u64() != MAGIC {
        return Err(SightFileError::Magic);
    }
    let version = b.u32();
    if version != VERSION {
        return Err(SightFileError::Version(version));
    }
    let (len, hash) = (b.u32(), b.u64());
    if usize::try_from(len).ok() != Some(objlist.len()) || hash != digest(objlist) {
        return Err(SightFileError::OtherSave);
    }
    let swept = b.u64();
    let [copies, cells, layers] = [b.u32(), b.u32(), b.u32()].map(|n| n as usize);
    if copies > MAX_COPIES || cells > MAX_CELLS || layers > MAX_SAVED_LAYERS || layers < cells {
        return Err(SightFileError::Counts);
    }
    if file.len() != HEADER + COPY * copies + CELL * cells + LAYER * layers {
        return Err(SightFileError::Length);
    }
    let mut m = SightMemory::default();
    for n in 0..copies {
        let record = b.u16();
        if usize::from(record) >= TILE_COUNT {
            return Err(SightFileError::Copy(n));
        }
        let pixels = b.take::<PIXELS>().to_vec();
        let bits = b.take::<BITS>();
        let transparent = (0..PIXELS)
            .map(|i| bits[i / 8] >> (i % 8) & 1 != 0)
            .collect();
        m.frozen.push(Frozen {
            record,
            pixels,
            transparent,
        });
    }
    for (slot, f) in m.frozen.iter().enumerate() {
        let hash = content_hash(&f.pixels, &f.transparent);
        m.by_content
            .entry((f.record, hash))
            .or_default()
            .push(slot as u16);
    }
    m.cells = HashMap::with_capacity(cells);
    let (mut last, mut read) = (None, 0usize);
    for n in 0..cells {
        let (k, seen, count) = (b.u32(), b.u32(), usize::from(b.u16()));
        if last.is_some_and(|l| k <= l) || !key_in_range(k) || count == 0 {
            return Err(SightFileError::Cell(n));
        }
        last = Some(k);
        if read + count > layers {
            return Err(SightFileError::Layer(read));
        }
        let mut stack = Vec::with_capacity(count);
        for _ in 0..count {
            let (record, [masktype, frame], slot) = (b.u16(), b.take::<2>(), b.u16());
            let frozen = (slot != NO_COPY).then_some(slot);
            if usize::from(record) >= TILE_COUNT || frozen.is_some_and(|s| usize::from(s) >= copies)
            {
                return Err(SightFileError::Layer(read));
            }
            stack.push(Layer {
                record,
                masktype,
                frame,
                frozen,
            });
            read += 1;
        }
        let first = stack[0];
        let stacked = count > 1;
        m.cells.insert(
            k,
            Remembered {
                seen,
                first,
                stacked,
            },
        );
        if stacked {
            m.stacks.insert(k, stack);
        }
    }
    if read != layers {
        return Err(SightFileError::Layer(read));
    }
    m.swept = (swept != NO_SWEEP).then_some(swept);
    m.cap();
    Ok(m)
}

#[cfg(test)]
mod tests {
    use u6_testkit::Quiet;

    use super::super::{BOXES, cell_key, minute32};
    use super::*;

    /// A memory as the tests compare it: its cells (key, minute, layers) in key order, its copies
    /// in slot order, the index by content, the hour of its last sweep.
    type Picture = (
        Vec<(u32, u32, Vec<Layer>)>,
        Vec<(u16, Vec<u8>, Vec<bool>)>,
        Vec<((u16, u64), Vec<u16>)>,
        Option<u64>,
    );

    fn picture(m: &SightMemory) -> Picture {
        let mut cells: Vec<(u32, u32, Vec<Layer>)> = (m.cells.iter())
            .map(|(&k, c)| (k, c.seen, m.layers_of(k, c).quiet().to_vec()))
            .collect();
        cells.sort_unstable_by_key(|c| c.0);
        let copies = (m.frozen.iter())
            .map(|f| (f.record, f.pixels.clone(), f.transparent.clone()))
            .collect();
        let mut index: Vec<((u16, u64), Vec<u16>)> =
            m.by_content.iter().map(|(&k, v)| (k, v.clone())).collect();
        index.sort_unstable();
        (cells, copies, index, m.swept)
    }

    fn layer(record: u16, frozen: Option<u16>) -> Layer {
        Layer {
            record,
            masktype: 4,
            frame: 3,
            frozen,
        }
    }

    /// Bytes a test binds its files to (no game's).
    const OBJLIST: &[u8] = &[7; 0x1c73];

    /// A memory of every part the file holds: one-layer cells on the surface and below (at the
    /// far corners of their levels' masks), a stacked cell, a layer drawn from a copy, copies of
    /// two contents (some pixels transparent), a sweep hour.
    fn sample() -> SightMemory {
        let mut m = SightMemory::default();
        m.fill_copies(3, 17);
        m.frozen[1].transparent[5] = true;
        m.frozen[1].transparent[250] = true;
        // The index as freeze keeps it: the content changed above, so made again.
        m.by_content.clear();
        for (slot, f) in m.frozen.iter().enumerate() {
            let hash = content_hash(&f.pixels, &f.transparent);
            m.by_content
                .entry((f.record, hash))
                .or_default()
                .push(slot as u16);
        }
        m.remember((0, 0, 0), vec![layer(1, None)], 10);
        m.remember((0, 0x3ff, 0x3ff), vec![layer(2047, Some(2))], 20_000);
        m.remember((5, 0xff, 0xff), vec![layer(9, None), layer(17, Some(1))], 7);
        m.remember((2, 3, 4), vec![layer(9, None)], u64::from(u32::MAX));
        m.swept = Some(123_456);
        m
    }

    /// Spec 18 (saved with the game): a memory written and read back is the same memory, part for
    /// part, and writes the same bytes again; the reading drops nothing a picture needs.
    #[test]
    fn the_file_reads_back_the_memory_it_was_written_from() {
        let m = sample();
        let file = encode(&m, OBJLIST).quiet();
        let back = read_sight(&file, OBJLIST).quiet();
        assert!(picture(&back) == picture(&m), "the same memory");
        assert!(back.stacks.len() == 1 && back.latest.is_empty() && back.scratch.is_empty());
        assert!(
            encode(&back, OBJLIST).quiet() == file,
            "the same bytes again"
        );
        let want = HEADER + COPY * 3 + CELL * 4 + LAYER * 5;
        assert_eq!(file.len(), want, "the bytes its counts make");
        assert!(
            encode(&SightMemory::default(), OBJLIST).is_none(),
            "nothing remembered: none"
        );
    }

    /// Spec 18 (the binding): a file is read only with the objlist bytes it was written with: one
    /// byte changed, one byte more or less, and none, are another save's.
    #[test]
    fn a_file_of_another_objlist_is_not_read() {
        let file = encode(&sample(), OBJLIST).quiet();
        let mut other = OBJLIST.to_vec();
        other[0x100] ^= 1;
        for o in [&other[..], &OBJLIST[1..], &[OBJLIST, &[0]].concat(), &[]] {
            assert_eq!(read_sight(&file, o).err(), Some(SightFileError::OtherSave));
        }
        assert!(read_sight(&file, OBJLIST).is_ok());
    }

    /// The file of `m` with the u32 at `at` set to `v`.
    fn with_u32(file: &[u8], at: usize, v: u32) -> Vec<u8> {
        let mut f = file.to_vec();
        f[at..at + 4].copy_from_slice(&v.to_le_bytes());
        f
    }

    /// The offset of cell `n`'s key in [`sample`]'s file (the cells after the three copies; the
    /// cells before it of one layer, but for the stacked one, the last).
    fn cell_at(n: usize) -> usize {
        HEADER + COPY * 3 + (CELL + LAYER) * n
    }

    /// Spec 18 (malformed or unknown: ignored): every malformed file is refused whole, for its
    /// reason, and none panics: shorter than the header, any prefix of a good file, the wrong
    /// magic, another version, counts past the bounds (the cap's cells, the copies, the layers)
    /// or fewer layers than cells, a byte more, keys out of order or out of range (a level past
    /// 5; past a dungeon's 256 cells), a cell of no layer, a layer's record past the 2048 or its
    /// slot past the copies, a copy's record past them, the layers not the header's count.
    #[test]
    fn a_malformed_file_is_refused_whole() {
        use SightFileError as E;
        let file = encode(&sample(), OBJLIST).quiet();
        let read = |f: &[u8]| read_sight(f, OBJLIST).err();
        for n in 0..file.len() {
            let want = if n < HEADER { E::Short } else { E::Length };
            assert_eq!(read(&file[..n]), Some(want), "a prefix of {n} bytes");
        }
        assert_eq!(read(&[file.as_slice(), &[0]].concat()), Some(E::Length));
        let mut f = file.clone();
        f[0] ^= 0x20;
        assert_eq!(read(&f), Some(E::Magic));
        assert_eq!(read(&with_u32(&file, 8, 2)), Some(E::Version(2)));
        assert_eq!(read(&with_u32(&file, 8, 0)), Some(E::Version(0)));
        let (copies, cells, layers) = (32, 36, 40);
        for (at, n) in [
            (copies, MAX_COPIES + 1),
            (cells, MAX_CELLS + 1),
            (layers, MAX_SAVED_LAYERS + 1),
            (layers, 3),
        ] {
            assert_eq!(
                read(&with_u32(&file, at, n as u32)),
                Some(E::Counts),
                "{at} = {n}"
            );
        }
        assert_eq!(
            read(&with_u32(&file, cells, 5)),
            Some(E::Length),
            "a cell more"
        );
        let key = |n: usize, k: u32| with_u32(&file, cell_at(n), k);
        assert_eq!(read(&key(1, 0)), Some(E::Cell(1)), "the key before's");
        assert_eq!(read(&key(0, 6 << 20)), Some(E::Cell(0)), "level 6");
        assert_eq!(
            read(&key(0, cell_key((2, 0x100, 0)))),
            Some(E::Cell(0)),
            "x past a dungeon"
        );
        assert_eq!(
            read(&key(0, cell_key((2, 0, 0x100)))),
            Some(E::Cell(0)),
            "y past a dungeon"
        );
        let mut f = file.clone();
        f[cell_at(0) + 8..cell_at(0) + 10].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(read(&f), Some(E::Cell(0)), "no layer");
        let mut f = file.clone();
        f[cell_at(0) + 8..cell_at(0) + 10].copy_from_slice(&2u16.to_le_bytes());
        assert!(
            matches!(read(&f), Some(E::Layer(_) | E::Cell(_))),
            "a layer count off"
        );
        let rec = cell_at(0) + CELL;
        let mut f = file.clone();
        f[rec..rec + 2].copy_from_slice(&2048u16.to_le_bytes());
        assert_eq!(
            read(&f),
            Some(E::Layer(0)),
            "a layer's record past the 2048"
        );
        let mut f = file.clone();
        f[rec + 4..rec + 6].copy_from_slice(&3u16.to_le_bytes());
        assert_eq!(read(&f), Some(E::Layer(0)), "a slot past the copies");
        let mut f = file.clone();
        f[HEADER + COPY..HEADER + COPY + 2].copy_from_slice(&2048u16.to_le_bytes());
        assert_eq!(read(&f), Some(E::Copy(1)), "a copy's record past the 2048");
        // Corruption anywhere past the header: refused or read, never a panic.
        for at in HEADER..file.len() {
            for bit in [0x01u8, 0x80] {
                let mut f = file.clone();
                f[at] ^= bit;
                let _ = read_sight(&f, OBJLIST);
            }
        }
    }

    /// Spec 18 (the read keeps the reading build's cap): a file holding `MAX_CELLS` cells, the
    /// most a file may (a build with fewer cells a render than this one), is capped at once to
    /// seven eighths, the oldest first, so the next render's cells fit.
    #[test]
    fn a_file_past_this_builds_cap_is_capped_at_the_read() {
        let mut m = SightMemory::default();
        m.fill_synthetic(MAX_CELLS - 7, 0, 0, 1, 50);
        m.fill_synthetic(7, 0, 1000, 1, 10);
        let file = encode(&m, OBJLIST).quiet();
        let back = read_sight(&file, OBJLIST).quiet();
        assert_eq!(back.cells.len(), MAX_CELLS - MAX_CELLS / 8, "capped");
        assert!(
            back.cells.len() + BOXES <= MAX_CELLS,
            "the next render fits"
        );
        assert!(
            back.cells.values().all(|c| c.seen == minute32(50)),
            "the oldest went first"
        );
    }

    /// Spec 18 (the byte cost): the file of a full memory, the cap's most cells after a render
    /// (`MAX_CELLS` less a render's) of one layer each and the copy store full: 16 bytes a cell
    /// and 290 a copy, under [`MAX_SIGHT_FILE_BYTES`], the largest a file can be; with the owner's
    /// 255 copies (a constant of his game) the cells are nearly all of it.
    #[test]
    fn the_file_of_a_full_memory_is_bounded() {
        let mut m = SightMemory::default();
        let cells = MAX_CELLS - BOXES;
        m.fill_synthetic(cells, 0, 0, 1, 50);
        m.fill_copies(MAX_COPIES, 9);
        let full = encode(&m, OBJLIST).quiet().len();
        assert_eq!(full, HEADER + COPY * MAX_COPIES + (CELL + LAYER) * cells);
        assert!(full <= MAX_SIGHT_FILE_BYTES);
        let mut owner = SightMemory::default();
        owner.fill_synthetic(cells, 0, 0, 1, 50);
        owner.fill_copies(255, 9);
        let usual = encode(&owner, OBJLIST).quiet().len();
        println!(
            "sight file: {full} bytes for {cells} cells and {MAX_COPIES} copies, {usual} with 255 \
             copies; at most {MAX_SIGHT_FILE_BYTES}"
        );
        assert_eq!(MAX_SIGHT_FILE_BYTES, 7_421_996);
    }

    /// Spec 18 (the bounds): a memory with more layers than a file holds is written without its
    /// oldest sights (of one minute, the lowest keys first), whole cells, until its layers fit;
    /// the file reads back.
    #[test]
    fn past_the_files_layers_the_oldest_sights_are_left_out() {
        let mut m = SightMemory::default();
        let three = vec![layer(1, None), layer(2, None), layer(3, None)];
        let n = MAX_SAVED_LAYERS / 3 + 2;
        for i in 0..n {
            let at = (0, (i % 1024) as u16, (i / 1024) as u16);
            m.remember(at, three.clone(), 100 + (i % 4) as u64);
        }
        let file = encode(&m, OBJLIST).quiet();
        let back = read_sight(&file, OBJLIST).quiet();
        assert_eq!(
            back.cells.len(),
            MAX_SAVED_LAYERS / 3,
            "the whole cells that fit"
        );
        let oldest_kept = back.cells.values().map(|c| c.seen).min().quiet();
        let left: Vec<(u32, u32)> = (m.cells.iter())
            .filter(|(k, _)| !back.cells.contains_key(k))
            .map(|(&k, c)| (c.seen, k))
            .collect();
        assert!(!left.is_empty(), "some left out");
        let first_kept = (back.cells.iter())
            .filter(|(_, c)| c.seen == oldest_kept)
            .map(|(&k, _)| k)
            .min()
            .quiet();
        assert!(
            left.iter()
                .all(|&(seen, k)| seen < oldest_kept || (seen == oldest_kept && k < first_kept)),
            "the oldest, of one minute the lowest keys, left out"
        );
    }
}

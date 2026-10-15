//! Minimal zip reader (stored and deflate entries), mirroring fflate's `unzipSync`
//! as used by the former TypeScript intake (removed after the Rust port reached parity): find the end-of-central-directory record,
//! walk the central directory, ask the filter per entry name *before* touching its
//! data, then locate the data through the local header. CRCs are not checked
//! (fflate does not either). Zip64 sizes and offsets are honoured.
//!
//! A zip too large to copy whole is read in slices instead: the caller reads the end of the
//! file from [`tail_start`]; [`directory`] walks the central directory with the same checks as
//! [`read_zip`] (naming an earlier offset to read from when the directory or the Zip64 end
//! record starts before what was read) and returns the byte range of each entry the filter
//! takes, from its local header to the end of its data; the caller reads those ranges, and
//! [`Rebuilt`] checks each against its central entry and assembles a zip of only those
//! entries, which [`read_zip`] then reads like the zip they came from.

use crate::Error;

const EOCD: u32 = 0x0605_4b50;
const EOCD64: u32 = 0x0606_4b50;
const EOCD64_LOCATOR: u32 = 0x0706_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;
const MAX_COMMENT_SCAN: usize = 65_558;
const SENTINEL32: u32 = u32::MAX;
/// The end-of-central-directory scan, the Zip64 locator (20 bytes) and the Zip64 end record
/// (56 bytes) before it.
const TAIL: u64 = MAX_COMMENT_SCAN as u64 + 20 + 56;
/// The longest local extra field: a slice's range allows for it, up to the next record.
const MAX_LOCAL_EXTRA: u64 = 0xffff;

/// Local-file-header or empty-archive signature at the start.
pub fn is_zip(data: &[u8]) -> bool {
    data.len() >= 4 && data[0] == 0x50 && data[1] == 0x4b && (data[2] == 3 || data[2] == 5)
}

fn invalid() -> Error {
    Error::new("invalid zip data")
}

fn bytes<const N: usize>(d: &[u8], at: usize) -> Result<[u8; N], Error> {
    let end = at.checked_add(N).ok_or_else(invalid)?;
    d.get(at..end)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(invalid)
}

fn u16le(d: &[u8], at: usize) -> Result<usize, Error> {
    Ok(usize::from(u16::from_le_bytes(bytes(d, at)?)))
}

fn u32le(d: &[u8], at: usize) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(bytes(d, at)?))
}

fn u64le(d: &[u8], at: usize) -> Result<u64, Error> {
    Ok(u64::from_le_bytes(bytes(d, at)?))
}

fn to_usize(v: u64) -> Result<usize, Error> {
    usize::try_from(v).map_err(|_| invalid())
}

/// Entry names: UTF-8 when general-purpose flag bit 11 is set, else ISO 8859-1.
fn entry_name(raw: &[u8], utf8: bool) -> String {
    if utf8 {
        String::from_utf8_lossy(raw).into_owned()
    } else {
        raw.iter().copied().map(char::from).collect()
    }
}

/// Why a walk over part of a file stopped: the zip is invalid, or the walk needs the file's
/// bytes from an offset before the part it has.
enum Stop {
    Invalid(Error),
    Need(u64),
}

impl From<Error> for Stop {
    fn from(err: Error) -> Self {
        Stop::Invalid(err)
    }
}

/// A walk over the whole file never needs more.
fn whole(stop: Stop) -> Error {
    match stop {
        Stop::Invalid(err) => err,
        Stop::Need(_) => invalid(),
    }
}

/// The bytes of a file from offset `base` to its end (the whole file: `base` 0), read at file
/// offsets.
#[derive(Clone, Copy)]
struct View<'a> {
    d: &'a [u8],
    base: u64,
}

impl<'a> View<'a> {
    /// `n` bytes at file offset `at`; `Need(at)` when `at` lies before the bytes held.
    fn at(&self, at: u64, n: usize) -> Result<&'a [u8], Stop> {
        let i = at.checked_sub(self.base).ok_or(Stop::Need(at))?;
        let i = to_usize(i)?;
        let end = i.checked_add(n).ok_or_else(invalid)?;
        Ok(self.d.get(i..end).ok_or_else(invalid)?)
    }

    fn u16(&self, at: u64) -> Result<usize, Stop> {
        Ok(u16le(self.at(at, 2)?, 0)?)
    }

    fn u32(&self, at: u64) -> Result<u32, Stop> {
        Ok(u32le(self.at(at, 4)?, 0)?)
    }

    fn u64(&self, at: u64) -> Result<u64, Stop> {
        Ok(u64le(self.at(at, 8)?, 0)?)
    }
}

/// The number of central entries and the offset of the first, from the end-of-central-directory
/// record of a file of `size` bytes (found within its last 65,558 bytes) and, when that record
/// holds a Zip64 sentinel and a Zip64 locator precedes it, the Zip64 end record.
fn end_records(v: &View, size: u64) -> Result<(u64, u64), Stop> {
    if size < 22 {
        return Err(invalid().into());
    }
    let mut e = size - 22;
    while v.u32(e)? != EOCD {
        if e == 0 || size - e > MAX_COMMENT_SCAN as u64 {
            return Err(invalid().into());
        }
        e -= 1;
    }
    let mut count = v.u16(e + 8)? as u64;
    if count == 0 {
        return Ok((0, 0));
    }
    let mut at = u64::from(v.u32(e + 16)?);
    let zip64 = at == u64::from(SENTINEL32) || count == 0xffff;
    if zip64 && e >= 20 && v.u32(e - 20)? == EOCD64_LOCATOR {
        let z = v.u64(e - 12)?;
        if v.u32(z)? == EOCD64 {
            count = v.u64(z + 32)?;
            at = v.u64(z + 48)?;
        }
    }
    Ok((count, at))
}

/// One central-directory record, its Zip64 values applied.
struct Central<'a> {
    name: String,
    raw_name: &'a [u8],
    flags: usize,
    method: usize,
    crc: u32,
    csize: u64,
    full: u64,
    local: u64,
    /// Where the next record starts.
    next: u64,
}

fn central_entry<'a>(v: &View<'a>, o: u64) -> Result<Central<'a>, Stop> {
    if v.u32(o)? != CENTRAL {
        return Err(invalid().into());
    }
    let flags = v.u16(o + 8)?;
    let method = v.u16(o + 10)?;
    let crc = v.u32(o + 16)?;
    let mut csize = u64::from(v.u32(o + 20)?);
    let mut full = u64::from(v.u32(o + 24)?);
    let name_len = v.u16(o + 28)?;
    let extra_len = v.u16(o + 30)?;
    let comment_len = v.u16(o + 32)?;
    let mut local = u64::from(v.u32(o + 42)?);
    let name_at = o + 46;
    let raw_name = v.at(name_at, name_len)?;
    let name = entry_name(raw_name, flags & 0x800 != 0);
    let extra = v.at(name_at + name_len as u64, extra_len)?;
    zip64_fields(extra, &mut full, &mut csize, &mut local)?;
    Ok(Central {
        name,
        raw_name,
        flags,
        method,
        crc,
        csize,
        full,
        local,
        next: name_at + (name_len + extra_len + comment_len) as u64,
    })
}

/// Every entry whose name passes `keep`, as (full entry name, bytes), in
/// central-directory order. A repeated name replaces the earlier bytes in place.
pub fn read_zip(data: &[u8], keep: impl Fn(&str) -> bool) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let v = View { d: data, base: 0 };
    let (count, mut at) = end_records(&v, data.len() as u64).map_err(whole)?;
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for _ in 0..count {
        let c = central_entry(&v, at).map_err(whole)?;
        at = c.next;
        if !keep(&c.name) {
            continue;
        }
        let l = to_usize(c.local)?;
        if u32le(data, l)? != LOCAL {
            return Err(invalid());
        }
        let start = l + 30 + u16le(data, l + 26)? + u16le(data, l + 28)?;
        let end = start.checked_add(to_usize(c.csize)?).ok_or_else(invalid)?;
        let body = data.get(start..end).ok_or_else(invalid)?;
        let bytes = match c.method {
            0 => body.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(body, to_usize(c.full)?)
                .map_err(|err| Error::new(format!("{}: inflate failed: {err}", c.name)))?,
            m => return Err(Error::new(format!("unknown compression type {m}"))),
        };
        match out.iter_mut().find(|(n, _)| *n == c.name) {
            Some(slot) => slot.1 = bytes,
            None => out.push((c.name, bytes)),
        }
    }
    Ok(out)
}

/// Zip64 extended information (tag 0x0001): 64-bit values for whichever of
/// uncompressed size, compressed size and local offset hold the 32-bit sentinel.
fn zip64_fields(
    extra: &[u8],
    full: &mut u64,
    csize: &mut u64,
    local: &mut u64,
) -> Result<(), Error> {
    let sentinel = u64::from(SENTINEL32);
    if *full != sentinel && *csize != sentinel && *local != sentinel {
        return Ok(());
    }
    let mut i = 0;
    while i + 4 <= extra.len() {
        let tag = u16le(extra, i)?;
        let size = u16le(extra, i + 2)?;
        if tag == 1 {
            let mut f = i + 4;
            for v in [full, csize, local] {
                if *v == sentinel {
                    *v = u64le(extra, f)?;
                    f += 8;
                }
            }
            return Ok(());
        }
        i += 4 + size;
    }
    Ok(())
}

/// Where the first read of a zip of `size` bytes read in slices starts: the end-of-central-
/// directory record with the longest comment, the Zip64 locator and the Zip64 end record before
/// it (the whole file when it is shorter).
pub fn tail_start(size: u64) -> u64 {
    size.saturating_sub(TAIL)
}

/// One entry [`directory`] found: the range of the zip that holds its local header and data, and
/// what its central record says (the values [`Rebuilt`] checks the local header against and
/// writes into the zip it assembles).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slice {
    /// The range in the zip, from the local header to the end of the data: exact when the next
    /// record (another local header or the central directory) follows the data, else allowing
    /// for the longest local extra field (the local header's own lengths are read only from the
    /// slice).
    pub start: u64,
    pub end: u64,
    /// The entry's name as [`read_zip`] reports it.
    pub name: String,
    /// What the entry declares unpacked: the inflate limit.
    pub full: u64,
    raw_name: Vec<u8>,
    flags: usize,
    method: usize,
    crc: u32,
    csize: u64,
}

impl Slice {
    pub fn len(&self) -> u64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.end == self.start
    }
}

/// What [`directory`] read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Directory {
    /// The central directory (or the Zip64 end record) starts before the bytes given: read the
    /// file again from this offset (always before the one given).
    Need(u64),
    /// The entries the filter takes, in central-directory order.
    Entries(Vec<Slice>),
}

/// Walks the central directory of a zip of `size` bytes from `suffix`, its bytes from offset
/// `from` to the end, with [`read_zip`]'s checks, and returns the range of each entry whose name
/// passes `keep` (or the earlier offset to read from). Fails when the end records or the central
/// directory do not read, or an entry's local header and data cannot fit before the end of the
/// file.
pub fn directory(
    size: u64,
    from: u64,
    suffix: &[u8],
    keep: impl Fn(&str) -> bool,
) -> Result<Directory, Error> {
    if from.checked_add(suffix.len() as u64) != Some(size) {
        return Err(invalid());
    }
    let v = View {
        d: suffix,
        base: from,
    };
    match walk(&v, size, keep) {
        Ok(slices) => Ok(Directory::Entries(slices)),
        Err(Stop::Need(at)) => Ok(Directory::Need(at)),
        Err(Stop::Invalid(err)) => Err(err),
    }
}

fn walk(v: &View, size: u64, keep: impl Fn(&str) -> bool) -> Result<Vec<Slice>, Stop> {
    let (count, mut at) = end_records(v, size)?;
    // The records an entry's data ends before: every local header and the central directory.
    let mut records = vec![at];
    let mut kept = Vec::new();
    for _ in 0..count {
        let c = central_entry(v, at)?;
        at = c.next;
        records.push(c.local);
        if keep(&c.name) {
            kept.push(c);
        }
    }
    records.sort_unstable();
    let mut slices = Vec::with_capacity(kept.len());
    for c in kept {
        let least = c
            .local
            .checked_add(30 + c.raw_name.len() as u64)
            .and_then(|header_end| header_end.checked_add(c.csize))
            .ok_or_else(invalid)?;
        if least > size {
            return Err(invalid().into());
        }
        let mut end = size.min(least.saturating_add(MAX_LOCAL_EXTRA));
        if let Some(&next) = records.get(records.partition_point(|&r| r < least)) {
            end = end.min(next);
        }
        slices.push(Slice {
            start: c.local,
            end,
            name: c.name,
            full: c.full,
            raw_name: c.raw_name.to_vec(),
            flags: c.flags,
            method: c.method,
            crc: c.crc,
            csize: c.csize,
        });
    }
    Ok(slices)
}

/// A zip of only the entries [`directory`] found, assembled from their slices: each entry's local
/// header and data as the zip holds them, then a central directory for them (the central
/// records' flags, method, CRC and sizes, at the new offsets) and its end record. [`read_zip`]
/// reads it as it would read those entries of the zip they came from.
#[derive(Debug, Default)]
pub struct Rebuilt {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

impl Rebuilt {
    /// Room for `bytes` of slices.
    pub fn with_capacity(bytes: usize) -> Self {
        Rebuilt {
            out: Vec::with_capacity(bytes),
            ..Rebuilt::default()
        }
    }

    /// Adds the entry `slice` names from `bytes`, the zip's bytes `slice.start..slice.end`, once
    /// its local header agrees with the central record: the local signature, the same name and
    /// compression method, and the data inside the slice. Fails, adding nothing, otherwise.
    pub fn add(&mut self, slice: &Slice, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() as u64 != slice.len() || u32le(bytes, 0)? != LOCAL {
            return Err(invalid());
        }
        let method = u16le(bytes, 8)?;
        let (name_len, extra_len) = (u16le(bytes, 26)?, u16le(bytes, 28)?);
        let name = bytes.get(30..30 + name_len).ok_or_else(invalid)?;
        if name != slice.raw_name.as_slice() || method != slice.method {
            return Err(invalid());
        }
        let data_end = (30 + name_len + extra_len)
            .checked_add(to_usize(slice.csize)?)
            .ok_or_else(invalid)?;
        let entry = bytes.get(..data_end).ok_or_else(invalid)?;
        // 32-bit fields only (no Zip64 record here): a sentinel would read as one.
        let field = |v: u64| match u32::try_from(v) {
            Ok(v) if v != SENTINEL32 => Ok(v),
            _ => Err(invalid()),
        };
        let offset = field(self.out.len() as u64)?;
        let (csize, full) = (field(slice.csize)?, field(slice.full)?);
        let count = self.count.checked_add(1).filter(|&n| n < 0xffff);
        let count = count.ok_or_else(invalid)?;
        field((self.out.len() + entry.len()) as u64)?;
        self.out.extend_from_slice(entry);
        let c = &mut self.central;
        c.extend_from_slice(&CENTRAL.to_le_bytes());
        c.extend_from_slice(&[20, 0, 20, 0]); // versions made by and needed
        for v in [slice.flags, slice.method, 0, 0] {
            c.extend_from_slice(&(v as u16).to_le_bytes()); // flags, method, time, date
        }
        for v in [slice.crc, csize, full] {
            c.extend_from_slice(&v.to_le_bytes());
        }
        c.extend_from_slice(&(slice.raw_name.len() as u16).to_le_bytes());
        c.extend_from_slice(&[0; 12]); // extra, comment, disk, attributes
        c.extend_from_slice(&offset.to_le_bytes());
        c.extend_from_slice(&slice.raw_name);
        self.count = count;
        Ok(())
    }

    /// The zip: the entries added, their central directory and its end record.
    pub fn finish(self) -> Result<Vec<u8>, Error> {
        let Rebuilt {
            mut out,
            central,
            count,
        } = self;
        let at = u32::try_from(out.len()).map_err(|_| invalid())?;
        let len = u32::try_from(central.len()).map_err(|_| invalid())?;
        if at == SENTINEL32 {
            return Err(invalid());
        }
        out.extend_from_slice(&central);
        out.extend_from_slice(&EOCD.to_le_bytes());
        out.extend_from_slice(&[0; 4]); // disk numbers
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&[0; 2]); // comment length
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENTINEL: u32 = u32::MAX;

    fn put16(out: &mut Vec<u8>, v: usize) {
        out.extend_from_slice(&(v as u16).to_le_bytes());
    }

    fn put32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn put64(out: &mut Vec<u8>, v: u64) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    /// A synthetic zip of (name, data, deflate) entries; each local header carries `pad` bytes of
    /// an unknown extra field. `zip64` writes the sizes and offsets as Zip64 values (sentinels,
    /// extended-information fields, the Zip64 end record and locator). Returns the zip and each
    /// entry's range, from its local header to the end of its data.
    fn make(
        entries: &[(&str, &[u8], bool)],
        pad: usize,
        zip64: bool,
    ) -> (Vec<u8>, Vec<(u64, u64)>) {
        let (mut out, mut central, mut ranges) = (Vec::new(), Vec::new(), Vec::new());
        for &(name, data, deflate) in entries {
            let body = if deflate {
                miniz_oxide::deflate::compress_to_vec(data, 6)
            } else {
                data.to_vec()
            };
            let (method, local) = (if deflate { 8 } else { 0 }, out.len() as u64);
            let field = |v: u64| if zip64 { SENTINEL } else { v as u32 };
            put32(&mut out, LOCAL);
            put16(&mut out, 20);
            put16(&mut out, 0x800);
            put16(&mut out, method);
            put32(&mut out, 0); // DOS time + date
            put32(&mut out, 0); // CRC, not checked
            put32(&mut out, field(body.len() as u64));
            put32(&mut out, field(data.len() as u64));
            put16(&mut out, name.len());
            put16(&mut out, if pad > 0 { 4 + pad } else { 0 });
            out.extend_from_slice(name.as_bytes());
            if pad > 0 {
                put16(&mut out, 0xcafe);
                put16(&mut out, pad);
                out.resize(out.len() + pad, 0);
            }
            out.extend_from_slice(&body);
            ranges.push((local, out.len() as u64));

            let mut extra = Vec::new();
            if zip64 {
                put16(&mut extra, 1);
                put16(&mut extra, 24);
                put64(&mut extra, data.len() as u64);
                put64(&mut extra, body.len() as u64);
                put64(&mut extra, local);
            }
            put32(&mut central, CENTRAL);
            put16(&mut central, 20);
            put16(&mut central, 20);
            put16(&mut central, 0x800);
            put16(&mut central, method);
            put32(&mut central, 0);
            put32(&mut central, 0);
            put32(&mut central, field(body.len() as u64));
            put32(&mut central, field(data.len() as u64));
            put16(&mut central, name.len());
            put16(&mut central, extra.len());
            central.extend_from_slice(&[0; 6]); // comment, disk, internal attributes
            put32(&mut central, 0); // external attributes
            put32(&mut central, field(local));
            central.extend_from_slice(name.as_bytes());
            central.extend_from_slice(&extra);
        }
        let at = out.len() as u64;
        out.extend_from_slice(&central);
        let n = entries.len();
        if zip64 {
            let z = out.len() as u64;
            put32(&mut out, EOCD64);
            put64(&mut out, 44);
            put16(&mut out, 45);
            put16(&mut out, 45);
            put32(&mut out, 0);
            put32(&mut out, 0);
            put64(&mut out, n as u64);
            put64(&mut out, n as u64);
            put64(&mut out, central.len() as u64);
            put64(&mut out, at);
            put32(&mut out, EOCD64_LOCATOR);
            put32(&mut out, 0);
            put64(&mut out, z);
            put32(&mut out, 1);
        }
        put32(&mut out, EOCD);
        put32(&mut out, 0);
        put16(&mut out, if zip64 { 0xffff } else { n });
        put16(&mut out, if zip64 { 0xffff } else { n });
        put32(
            &mut out,
            if zip64 {
                SENTINEL
            } else {
                central.len() as u32
            },
        );
        put32(&mut out, if zip64 { SENTINEL } else { at as u32 });
        put16(&mut out, 0);
        (out, ranges)
    }

    fn disks(name: &str) -> bool {
        name.ends_with(".hfe") || name.ends_with(".adf")
    }

    /// The directory read as the page reads it: the tail, then from each offset it names. Returns
    /// the slices and the number of reads.
    fn read_directory(zip: &[u8]) -> Result<(Vec<Slice>, usize), Error> {
        let size = zip.len() as u64;
        let mut from = tail_start(size);
        for reads in 1.. {
            match directory(size, from, &zip[from as usize..], disks)? {
                Directory::Need(at) => {
                    assert!(at < from, "an earlier offset");
                    from = at;
                }
                Directory::Entries(slices) => return Ok((slices, reads)),
            }
        }
        unreachable!()
    }

    fn piece<'a>(zip: &'a [u8], s: &Slice) -> &'a [u8] {
        &zip[s.start as usize..s.end as usize]
    }

    fn rebuild(zip: &[u8], slices: &[Slice]) -> Result<Vec<u8>, Error> {
        let mut r = Rebuilt::default();
        for s in slices {
            r.add(s, piece(zip, s))?;
        }
        r.finish()
    }

    fn ranges(slices: &[Slice]) -> Vec<(u64, u64)> {
        slices.iter().map(|s| (s.start, s.end)).collect()
    }

    /// The central directory's offset, from the end record of a zip without a comment.
    fn central_at(zip: &[u8]) -> usize {
        u32le(zip, zip.len() - 22 + 16).unwrap() as usize
    }

    #[test]
    fn the_directory_names_the_wanted_entries_ranges_and_the_rebuilt_zip_reads_the_same() {
        let scan = vec![7u8; 5000];
        let one: Vec<u8> = (0..3000u32).map(|i| (i * 7) as u8).collect();
        let two = b"second disk ".repeat(200);
        for pad in [0, 9] {
            let (zip, exact) = make(
                &[
                    ("scans/a.jxl", &scan, false),
                    ("d/Disk 1.hfe", &one, false),
                    ("notes.txt", b"x", false),
                    ("Disk 2.adf", &two, true),
                ],
                pad,
                false,
            );
            let (slices, reads) = read_directory(&zip).unwrap();
            assert_eq!(reads, 1, "a small zip's directory is in the tail");
            assert_eq!(
                ranges(&slices),
                [exact[1], exact[3]],
                "exact ranges, pad {pad}"
            );
            assert_eq!(slices[0].name, "d/Disk 1.hfe");
            assert_eq!((slices[0].full, slices[1].full), (3000, two.len() as u64));
            let rebuilt = rebuild(&zip, &slices).unwrap();
            assert!(is_zip(&rebuilt));
            assert_eq!(
                read_zip(&rebuilt, disks).unwrap(),
                read_zip(&zip, disks).unwrap()
            );
            let all = read_zip(&rebuilt, |_| true).unwrap();
            assert_eq!(all.len(), 2, "only the wanted entries");
        }
        let (zip, _) = make(&[("notes.txt", b"x", false)], 0, false);
        let (slices, _) = read_directory(&zip).unwrap();
        let rebuilt = rebuild(&zip, &slices).unwrap();
        assert!(slices.is_empty() && is_zip(&rebuilt));
        assert_eq!(read_zip(&rebuilt, disks).unwrap(), []);
    }

    #[test]
    fn a_directory_before_the_bytes_read_is_named_then_read() {
        let (zip, exact) = make(
            &[("Disk 1.hfe", b"one", false), ("Disk 2.hfe", b"two", false)],
            0,
            false,
        );
        let size = zip.len() as u64;
        let from = size - 22; // the end record only
        let at = exact[1].1; // the central directory follows the last entry
        let read = |from: u64| directory(size, from, &zip[from as usize..], disks).unwrap();
        assert_eq!(read(from), Directory::Need(at));
        let Directory::Entries(slices) = read(at) else {
            panic!("the directory");
        };
        assert_eq!(ranges(&slices), exact);
        // A directory longer than the tail: a second read from where it starts.
        let names: Vec<String> = (0..1500)
            .map(|i| format!("flux/track-{i:04}.raw"))
            .collect();
        let mut entries: Vec<(&str, &[u8], bool)> = names
            .iter()
            .map(|n| (n.as_str(), &b"f"[..], false))
            .collect();
        entries.push(("Disk 3.adf", b"three", false));
        let (zip, exact) = make(&entries, 0, false);
        assert!(zip.len() - central_at(&zip) > TAIL as usize);
        let (slices, reads) = read_directory(&zip).unwrap();
        assert_eq!((reads, ranges(&slices)), (2, vec![exact[1500]]));
        assert_eq!((tail_start(10), tail_start(1 << 30)), (0, (1 << 30) - TAIL));
    }

    #[test]
    fn zip64_sizes_offsets_and_end_records() {
        let disk = b"zip64 disk ".repeat(100);
        let (zip, exact) = make(
            &[("scan.bin", &[1; 300], false), ("Disk 1.adf", &disk, true)],
            5,
            true,
        );
        let (slices, _) = read_directory(&zip).unwrap();
        assert_eq!(ranges(&slices), [exact[1]]);
        assert_eq!(slices[0].full, disk.len() as u64);
        let size = zip.len() as u64;
        let read = |from: u64| directory(size, from, &zip[from as usize..], disks).unwrap();
        // From the locator and the end record only: the Zip64 record, then the directory.
        let from = size - 22 - 20;
        assert_eq!(read(from), Directory::Need(from - 56));
        assert_eq!(read(from - 56), Directory::Need(exact[1].1));
        let rebuilt = rebuild(&zip, &slices).unwrap();
        assert_eq!(
            read_zip(&rebuilt, disks).unwrap(),
            [("Disk 1.adf".to_string(), disk)]
        );
        assert_eq!(
            read_zip(&zip, disks).unwrap(),
            read_zip(&rebuilt, disks).unwrap()
        );
    }

    #[test]
    fn a_truncated_or_inconsistent_tail_is_an_error() {
        let (zip, _) = make(&[("Disk 1.hfe", b"one", false)], 0, false);
        let size = zip.len() as u64;
        let (e, at) = (zip.len() - 22, central_at(&zip));
        let read = |z: &[u8]| directory(z.len() as u64, 0, z, disks);
        assert!(
            directory(size, 0, &zip[..zip.len() - 1], disks).is_err(),
            "not to the end"
        );
        assert!(directory(size, 1, &zip, disks).is_err(), "past the end");
        let cut = &zip[..zip.len() - 10];
        let mut far = zip.clone();
        far[e + 16..e + 20].copy_from_slice(&(size as u32 + 100).to_le_bytes());
        let mut unsigned = zip.clone();
        unsigned[at] ^= 1;
        let mut more = zip.clone();
        more[e + 8] = 2; // two records counted, one there
        let mut short_name = zip.clone();
        short_name[at + 28] = 0xff; // a name running into the end record and past the end
        for (what, z) in [
            ("the end record cut", cut),
            ("the directory past the end", &far[..]),
            ("a central record without its signature", &unsigned[..]),
            ("more records counted than there are", &more[..]),
            ("a name past the end", &short_name[..]),
        ] {
            assert_eq!(read(z), Err(invalid()), "{what}");
            assert_eq!(
                read_zip(z, disks),
                Err(invalid()),
                "{what}: read_zip agrees"
            );
        }
    }

    #[test]
    fn an_entry_range_past_the_end_of_the_file_is_an_error() {
        let (zip, _) = make(&[("Disk 1.hfe", b"one", false)], 0, false);
        let at = central_at(&zip);
        let mut long = zip.clone();
        long[at + 20..at + 24].copy_from_slice(&(zip.len() as u32).to_le_bytes());
        let mut far = zip.clone();
        far[at + 42..at + 46].copy_from_slice(&(zip.len() as u32 - 40).to_le_bytes());
        for (what, z) in [
            ("data past the end", &long),
            ("a local header past the end", &far),
        ] {
            assert_eq!(
                directory(z.len() as u64, 0, z, disks),
                Err(invalid()),
                "{what}"
            );
        }
        let (zip64, _) = make(&[("Disk 1.hfe", b"one", false)], 0, true);
        let mut huge = zip64.clone();
        let at = u64le(&zip64, zip64.len() - 22 - 20 - 56 + 48).unwrap() as usize;
        let local = at + 46 + "Disk 1.hfe".len() + 4 + 16; // the Zip64 field's local offset
        huge[local..local + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        let size = huge.len() as u64;
        assert_eq!(
            directory(size, 0, &huge, disks),
            Err(invalid()),
            "a Zip64 offset at the top"
        );
    }

    #[test]
    fn a_local_header_that_disagrees_with_its_central_record_is_an_error_and_adds_nothing() {
        let (zip, _) = make(
            &[
                ("Disk 1.hfe", b"first", false),
                ("Disk 2.hfe", b"second", false),
            ],
            0,
            false,
        );
        let (slices, _) = read_directory(&zip).unwrap();
        let s = &slices[1];
        let edited = |edit: &dyn Fn(&mut Vec<u8>)| {
            let mut p = piece(&zip, s).to_vec();
            edit(&mut p);
            p
        };
        let mut r = Rebuilt::default();
        for (what, p) in [
            ("no local signature", edited(&|p| p[0] ^= 1)),
            ("another name", edited(&|p| p[30 + 5] = b'3')),
            ("another method", edited(&|p| p[8] = 8)),
            ("an extra field past the slice", edited(&|p| p[28] = 200)),
            (
                "a slice shorter than its range",
                edited(&|p| p.truncate(p.len() - 1)),
            ),
            ("a slice longer than its range", edited(&|p| p.push(0))),
        ] {
            assert_eq!(r.add(s, &p), Err(invalid()), "{what}");
        }
        r.add(&slices[0], piece(&zip, &slices[0])).unwrap();
        let rebuilt = r.finish().unwrap();
        assert_eq!(
            read_zip(&rebuilt, disks).unwrap(),
            [("Disk 1.hfe".to_string(), b"first".to_vec())]
        );
    }

    #[test]
    fn the_inflate_limit_and_repeated_names_carry_over() {
        let data = b"inflate me ".repeat(50);
        let (mut zip, _) = make(
            &[
                ("Disk 1.adf", &data, true),
                ("x/Disk 2.adf", b"a", false),
                ("Disk 1.adf", b"again", false),
            ],
            0,
            false,
        );
        let (slices, _) = read_directory(&zip).unwrap();
        let same = read_zip(&rebuild(&zip, &slices).unwrap(), disks).unwrap();
        assert_eq!(same, read_zip(&zip, disks).unwrap());
        assert_eq!(
            same[0],
            ("Disk 1.adf".to_string(), b"again".to_vec()),
            "the later copy, first place"
        );
        // The first entry declares 10 bytes unpacked: inflating stops there, read whole or sliced.
        let at = central_at(&zip);
        zip[at + 24..at + 28].copy_from_slice(&10u32.to_le_bytes());
        let (slices, _) = read_directory(&zip).unwrap();
        assert_eq!(slices[0].full, 10);
        let whole = read_zip(&zip, disks).unwrap_err();
        assert!(
            whole.to_string().starts_with("Disk 1.adf: inflate failed"),
            "{whole}"
        );
        assert_eq!(
            read_zip(&rebuild(&zip, &slices).unwrap(), disks).unwrap_err(),
            whole
        );
    }
}

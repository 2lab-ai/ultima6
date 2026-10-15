//! Minimal zip reader (stored and deflate entries), after fflate's `unzipSync` as used by the
//! former TypeScript intake (removed after the Rust port reached parity): find the
//! end-of-central-directory record, walk the central directory, ask the filter per entry name
//! *before* touching its data, then locate the data through the local header. CRCs are not
//! checked (fflate does not either). Zip64 sizes and offsets are honoured.
//!
//! A zip too large to copy whole is read in slices instead: the caller reads the end of the
//! file from [`tail_start`]; [`directory`] walks the central directory (naming an earlier offset
//! to read from when the directory or the Zip64 end record starts before what was read) and
//! returns the byte range of each entry the filter takes, from its local header to the end of
//! its data; the caller reads those ranges, and [`Rebuilt`] checks each and assembles a zip of
//! only those entries, which [`read_zip`] then reads like the zip they came from.
//!
//! Whole and sliced take the same zips: [`read_zip`] is [`directory`] over the whole file and
//! the same check of each entry's range ([`Slice`]), so both refuse alike:
//! - more than [`MAX_ENTRIES`] central records, or a central directory starting more than
//!   [`MAX_DIRECTORY_BYTES`] before the end of the file (both before any record is read);
//! - an entry the filter takes with a method other than stored or deflate, or 4 GiB or more
//!   packed or declared unpacked;
//! - an entry whose local header and data do not fit before the end of the file;
//! - a local header without its signature, with another name or method than its central
//!   record, or whose data runs into the next record (another local header or the central
//!   directory) or more than the longest extra field past where it would end without one;
//! - a Zip64 field too short for the values it replaces.
//!
//! The one difference left: [`Rebuilt`] refuses to grow past 4 GiB (its offsets are 32-bit),
//! which the sliced reader's caller never reaches (the page reads at most 512 MiB in all).

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

/// The most central records a zip may hold: the most an end record without Zip64 counts (it
/// writes 0xffff for "see the Zip64 record"), so a zip of the entries taken ([`Rebuilt`])
/// always holds them.
pub const MAX_ENTRIES: u64 = 0xfffe;
/// The most bytes from the first central record to the end of the file: the central directory
/// and the end records and comment after it (65,534 records with 200-byte names fit).
pub const MAX_DIRECTORY_BYTES: u64 = 16 << 20;

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

/// The end-of-central-directory record of a file of `size` bytes, scanning back from its end
/// over the longest comment: the first signature whose comment length reaches the end of the
/// file, else (trailing bytes after the comment) the first signature found.
fn end_record(v: &View, size: u64) -> Result<u64, Stop> {
    let mut e = size - 22;
    let mut first = None;
    loop {
        if v.u32(e)? == EOCD {
            if e + 22 + v.u16(e + 20)? as u64 == size {
                return Ok(e);
            }
            first.get_or_insert(e);
        }
        if e == 0 || size - e > MAX_COMMENT_SCAN as u64 {
            return first.ok_or_else(|| invalid().into());
        }
        e -= 1;
    }
}

/// A central directory or Zip64 end record at `at` lies in the file and at most
/// [`MAX_DIRECTORY_BYTES`] before its end.
fn near_end(at: u64, size: u64) -> Result<(), Stop> {
    match size.checked_sub(at) {
        Some(n) if n <= MAX_DIRECTORY_BYTES => Ok(()),
        _ => Err(invalid().into()),
    }
}

/// The number of central entries and the offset of the first, from the end-of-central-directory
/// record of a file of `size` bytes and, when that record holds a Zip64 sentinel and a Zip64
/// locator precedes it, the Zip64 end record. Refuses more than [`MAX_ENTRIES`] records and a
/// directory (or Zip64 record) more than [`MAX_DIRECTORY_BYTES`] before the end.
fn end_records(v: &View, size: u64) -> Result<(u64, u64), Stop> {
    if size < 22 {
        return Err(invalid().into());
    }
    let e = end_record(v, size)?;
    let mut count = v.u16(e + 8)? as u64;
    if count == 0 {
        return Ok((0, 0));
    }
    let mut at = u64::from(v.u32(e + 16)?);
    let zip64 = at == u64::from(SENTINEL32) || count == 0xffff;
    if zip64 && e >= 20 && v.u32(e - 20)? == EOCD64_LOCATOR {
        let z = v.u64(e - 12)?;
        near_end(z, size)?;
        if v.u32(z)? == EOCD64 {
            count = v.u64(z + 32)?;
            at = v.u64(z + 48)?;
        }
    }
    if count > MAX_ENTRIES {
        return Err(invalid().into());
    }
    near_end(at, size)?;
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

/// Zip64 extended information (tag 0x0001): 64-bit values for whichever of uncompressed size,
/// compressed size and local offset hold the 32-bit sentinel, each inside the field.
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
            let end = i + 4 + size;
            let mut f = i + 4;
            for v in [full, csize, local] {
                if *v == sentinel {
                    if f + 8 > end {
                        return Err(invalid());
                    }
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

/// Every entry whose name passes `keep`, as (full entry name, bytes), in
/// central-directory order. A repeated name replaces the earlier bytes in place.
pub fn read_zip(data: &[u8], keep: impl Fn(&str) -> bool) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let Directory::Entries(slices) = directory(data.len() as u64, 0, data, keep)? else {
        return Err(invalid()); // the whole file holds every offset
    };
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for s in slices {
        let range = data.get(to_usize(s.start)?..to_usize(s.end)?);
        let (_, body) = s.local(range.ok_or_else(invalid)?)?;
        let bytes = match s.method {
            0 => body.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(body, to_usize(s.full)?)
                .map_err(|err| Error::new(format!("{}: inflate failed: {err}", s.name)))?,
            m => return Err(unknown_method(m)),
        };
        match out.iter_mut().find(|(n, _)| *n == s.name) {
            Some(slot) => slot.1 = bytes,
            None => out.push((s.name, bytes)),
        }
    }
    Ok(out)
}

fn unknown_method(m: usize) -> Error {
    Error::new(format!("unknown compression type {m}"))
}

/// Where the first read of a zip of `size` bytes read in slices starts: the end-of-central-
/// directory record with the longest comment, the Zip64 locator and the Zip64 end record before
/// it (the whole file when it is shorter).
pub fn tail_start(size: u64) -> u64 {
    size.saturating_sub(TAIL)
}

/// One entry [`directory`] found: the range of the zip that holds its local header and data, and
/// what its central record says (the values its local header is checked against, and that
/// [`Rebuilt`] writes into the zip it assembles).
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
    /// What the entry declares unpacked: the inflate limit of a deflated entry.
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

    /// The most reading the entry yields: a stored entry its data, a deflated one what it
    /// declares (inflating stops there).
    pub fn unpacked(&self) -> u64 {
        if self.method == 0 {
            self.csize
        } else {
            self.full
        }
    }

    /// The local header and data, and the data, of the entry in `bytes` (the zip's bytes
    /// `start..end`), once the local header agrees with the central record: the local
    /// signature, the same name and compression method, and the data inside the range.
    fn local<'a>(&self, bytes: &'a [u8]) -> Result<(&'a [u8], &'a [u8]), Error> {
        if bytes.len() as u64 != self.len() || u32le(bytes, 0)? != LOCAL {
            return Err(invalid());
        }
        let method = u16le(bytes, 8)?;
        let (name_len, extra_len) = (u16le(bytes, 26)?, u16le(bytes, 28)?);
        let name = bytes.get(30..30 + name_len).ok_or_else(invalid)?;
        if name != self.raw_name.as_slice() || method != self.method {
            return Err(invalid());
        }
        let start = 30 + name_len + extra_len;
        let end = start
            .checked_add(to_usize(self.csize)?)
            .ok_or_else(invalid)?;
        let entry = bytes.get(..end).ok_or_else(invalid)?;
        Ok((entry, &entry[start..]))
    }
}

/// What [`directory`] read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Directory {
    /// The central directory (or the Zip64 end record) starts before the bytes given: read the
    /// file again from this offset (always before the one given, and at most
    /// [`MAX_DIRECTORY_BYTES`] before the end).
    Need(u64),
    /// The entries the filter takes, in central-directory order.
    Entries(Vec<Slice>),
}

/// Walks the central directory of a zip of `size` bytes from `suffix`, its bytes from offset
/// `from` to the end, and returns the range of each entry whose name passes `keep` (or the
/// earlier offset to read from). Fails when the end records or the central directory do not
/// read or pass the limits, or a taken entry cannot be read (see the module doc).
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
    // The records an entry's data ends before: every local header and the central directory
    // (`count` is at most MAX_ENTRIES).
    let mut records = Vec::with_capacity(count as usize + 1);
    records.push(at);
    let mut kept = Vec::new();
    for _ in 0..count {
        let c = central_entry(v, at)?;
        at = c.next;
        records.push(c.local);
        if keep(&c.name) {
            if c.method != 0 && c.method != 8 {
                return Err(unknown_method(c.method).into());
            }
            if c.csize >= u64::from(SENTINEL32) || c.full >= u64::from(SENTINEL32) {
                return Err(invalid().into());
            }
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
/// reads it as it reads those entries of the zip they came from.
#[derive(Debug, Default)]
pub struct Rebuilt {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

impl Rebuilt {
    /// Room for the zip of the entries `slices` name: their ranges, a central record each and
    /// the end record, so assembling it never reallocates (the caller bounds the ranges first).
    pub fn for_slices(slices: &[Slice]) -> Self {
        let room: u64 = slices.iter().map(Slice::len).sum();
        let central: u64 = slices.iter().map(|s| 46 + s.raw_name.len() as u64).sum();
        let reserve = |n: u64| Vec::with_capacity(usize::try_from(n).unwrap_or(0));
        Rebuilt {
            out: reserve(room + central + 22),
            central: reserve(central),
            count: 0,
        }
    }

    /// Adds the entry `slice` names from `bytes`, the zip's bytes `slice.start..slice.end`, once
    /// its local header agrees with the central record (as [`read_zip`] checks it). Fails,
    /// adding nothing, otherwise.
    pub fn add(&mut self, slice: &Slice, bytes: &[u8]) -> Result<(), Error> {
        let (entry, _) = slice.local(bytes)?;
        // 32-bit fields only (no Zip64 record here): a sentinel would read as one.
        let field = |v: u64| match u32::try_from(v) {
            Ok(v) if v != SENTINEL32 => Ok(v),
            _ => Err(invalid()),
        };
        let offset = field(self.out.len() as u64)?;
        let (csize, full) = (field(slice.csize)?, field(slice.full)?);
        if u64::from(self.count) >= MAX_ENTRIES {
            return Err(invalid());
        }
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
        self.count += 1;
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

    /// One entry of a synthetic zip: its name bytes, its data, and its method (8 deflates the
    /// data; any other is written as is, stored).
    #[derive(Clone)]
    struct E<'a> {
        name: &'a [u8],
        data: &'a [u8],
        method: usize,
    }

    fn stored<'a>(name: &'a str, data: &'a [u8]) -> E<'a> {
        E {
            name: name.as_bytes(),
            data,
            method: 0,
        }
    }

    fn deflated<'a>(name: &'a str, data: &'a [u8]) -> E<'a> {
        E {
            method: 8,
            ..stored(name, data)
        }
    }

    /// How a synthetic zip is written: `pad` bytes of an unknown extra field in each local
    /// header, the sizes and offsets as Zip64 values (sentinels, extended-information fields,
    /// the Zip64 end record and locator), the archive comment.
    #[derive(Clone, Copy, Default)]
    struct Opts<'a> {
        pad: usize,
        zip64: bool,
        comment: &'a [u8],
    }

    /// A synthetic zip of `entries` (general-purpose flag bit 11 set: UTF-8 names). Returns the
    /// zip and each entry's range, from its local header to the end of its data.
    fn write(entries: &[E], o: Opts) -> (Vec<u8>, Vec<(u64, u64)>) {
        let (mut out, mut central, mut ranges) = (Vec::new(), Vec::new(), Vec::new());
        for e in entries {
            let body = if e.method == 8 {
                miniz_oxide::deflate::compress_to_vec(e.data, 6)
            } else {
                e.data.to_vec()
            };
            let local = out.len() as u64;
            let field = |v: u64| if o.zip64 { SENTINEL } else { v as u32 };
            put32(&mut out, LOCAL);
            put16(&mut out, 20);
            put16(&mut out, 0x800);
            put16(&mut out, e.method);
            put32(&mut out, 0); // DOS time + date
            put32(&mut out, 0); // CRC, not checked
            put32(&mut out, field(body.len() as u64));
            put32(&mut out, field(e.data.len() as u64));
            put16(&mut out, e.name.len());
            put16(&mut out, if o.pad > 0 { 4 + o.pad } else { 0 });
            out.extend_from_slice(e.name);
            if o.pad > 0 {
                put16(&mut out, 0xcafe);
                put16(&mut out, o.pad);
                out.resize(out.len() + o.pad, 0);
            }
            out.extend_from_slice(&body);
            ranges.push((local, out.len() as u64));

            let mut extra = Vec::new();
            if o.zip64 {
                put16(&mut extra, 1);
                put16(&mut extra, 24);
                put64(&mut extra, e.data.len() as u64);
                put64(&mut extra, body.len() as u64);
                put64(&mut extra, local);
            }
            put32(&mut central, CENTRAL);
            put16(&mut central, 20);
            put16(&mut central, 20);
            put16(&mut central, 0x800);
            put16(&mut central, e.method);
            put32(&mut central, 0);
            put32(&mut central, 0);
            put32(&mut central, field(body.len() as u64));
            put32(&mut central, field(e.data.len() as u64));
            put16(&mut central, e.name.len());
            put16(&mut central, extra.len());
            central.extend_from_slice(&[0; 6]); // comment, disk, internal attributes
            put32(&mut central, 0); // external attributes
            put32(&mut central, field(local));
            central.extend_from_slice(e.name);
            central.extend_from_slice(&extra);
        }
        let at = out.len() as u64;
        out.extend_from_slice(&central);
        let n = entries.len();
        let (count, cd_len, cd_at) = if o.zip64 {
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
            (0xffff, SENTINEL, SENTINEL)
        } else {
            (n, central.len() as u32, at as u32)
        };
        put32(&mut out, EOCD);
        put32(&mut out, 0);
        put16(&mut out, count);
        put16(&mut out, count);
        put32(&mut out, cd_len);
        put32(&mut out, cd_at);
        put16(&mut out, o.comment.len());
        out.extend_from_slice(o.comment);
        (out, ranges)
    }

    fn make(entries: &[E], pad: usize, zip64: bool) -> (Vec<u8>, Vec<(u64, u64)>) {
        write(
            entries,
            Opts {
                pad,
                zip64,
                ..Opts::default()
            },
        )
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
        let mut r = Rebuilt::for_slices(slices);
        for s in slices {
            r.add(s, piece(zip, s))?;
        }
        r.finish()
    }

    type Read = Result<Vec<(String, Vec<u8>)>, Error>;

    /// `zip` read whole and read in slices (the directory, the slices, the rebuilt zip): the two
    /// must agree; returns what they read.
    fn both(zip: &[u8], what: &str) -> Read {
        let whole = read_zip(zip, disks);
        let sliced = read_directory(zip)
            .and_then(|(slices, _)| rebuild(zip, &slices))
            .and_then(|rebuilt| read_zip(&rebuilt, disks));
        assert_eq!(sliced, whole, "{what}: sliced and whole agree");
        whole
    }

    fn entries(list: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
        list.iter()
            .map(|&(n, d)| (n.to_string(), d.to_vec()))
            .collect()
    }

    fn ranges(slices: &[Slice]) -> Vec<(u64, u64)> {
        slices.iter().map(|s| (s.start, s.end)).collect()
    }

    /// The central directory's offset, from the end record of a zip without a comment.
    fn central_at(zip: &[u8]) -> usize {
        u32le(zip, zip.len() - 22 + 16).unwrap() as usize
    }

    #[test]
    fn the_directory_names_the_wanted_entries_ranges_and_both_paths_read_their_bytes() {
        let scan = vec![7u8; 5000];
        let one: Vec<u8> = (0..3000u32).map(|i| (i * 7) as u8).collect();
        let two = b"second disk ".repeat(200);
        for pad in [0, 9] {
            let (zip, exact) = make(
                &[
                    stored("scans/a.jxl", &scan),
                    stored("d/Disk 1.hfe", &one),
                    stored("notes.txt", b"x"),
                    deflated("Disk 2.adf", &two),
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
            assert_eq!(
                (slices[0].unpacked(), slices[1].unpacked()),
                (3000, two.len() as u64)
            );
            let got = both(&zip, "two disks");
            assert_eq!(
                got,
                Ok(entries(&[("d/Disk 1.hfe", &one), ("Disk 2.adf", &two)]))
            );
            let rebuilt = rebuild(&zip, &slices).unwrap();
            assert!(is_zip(&rebuilt));
            assert_eq!(
                read_zip(&rebuilt, |_| true).unwrap().len(),
                2,
                "only the wanted entries"
            );
        }
        let (zip, _) = make(&[stored("notes.txt", b"x")], 0, false);
        let (slices, _) = read_directory(&zip).unwrap();
        let rebuilt = rebuild(&zip, &slices).unwrap();
        assert!(slices.is_empty() && is_zip(&rebuilt));
        assert_eq!(both(&zip, "no disks"), Ok(Vec::new()));
    }

    #[test]
    fn a_directory_before_the_bytes_read_is_named_then_read() {
        let (zip, exact) = make(
            &[stored("Disk 1.hfe", b"one"), stored("Disk 2.hfe", b"two")],
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
        let mut list: Vec<E> = names.iter().map(|n| stored(n, b"f")).collect();
        list.push(stored("Disk 3.adf", b"three"));
        let (zip, exact) = make(&list, 0, false);
        assert!(zip.len() - central_at(&zip) > TAIL as usize);
        let (slices, reads) = read_directory(&zip).unwrap();
        assert_eq!((reads, ranges(&slices)), (2, vec![exact[1500]]));
        assert_eq!(
            both(&zip, "a long directory"),
            Ok(entries(&[("Disk 3.adf", b"three")]))
        );
        assert_eq!((tail_start(10), tail_start(1 << 30)), (0, (1 << 30) - TAIL));
    }

    #[test]
    fn zip64_sizes_offsets_and_end_records() {
        let disk = b"zip64 disk ".repeat(100);
        let (zip, exact) = make(
            &[stored("scan.bin", &[1; 300]), deflated("Disk 1.adf", &disk)],
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
        assert_eq!(both(&zip, "zip64"), Ok(entries(&[("Disk 1.adf", &disk)])));
    }

    /// Comments: empty, short, the longest, and one holding a false end record (its comment
    /// length does not reach the end of the file): both paths read the entries.
    #[test]
    fn archive_comments_including_a_false_end_record_inside_one() {
        let disk = b"commented disk".to_vec();
        let mut fake = b"before ".to_vec();
        fake.extend_from_slice(&EOCD.to_le_bytes());
        fake.extend_from_slice(&[
            0, 0, 0, 0, 9, 0, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0x40, 0, 0, 0,
        ]);
        fake.extend_from_slice(b" after");
        let longest = vec![b'c'; 0xffff];
        for (what, comment) in [
            ("none", &b""[..]),
            ("short", b"hello"),
            ("the longest", &longest),
            ("a false end record", &fake),
        ] {
            let opts = Opts {
                comment,
                ..Opts::default()
            };
            let (zip, _) = write(
                &[stored("scan.bin", &[1; 50]), stored("Disk 1.adf", &disk)],
                opts,
            );
            assert_eq!(
                both(&zip, what),
                Ok(entries(&[("Disk 1.adf", &disk)])),
                "{what}"
            );
        }
        // Trailing bytes after the comment: the first end record found, as before.
        let (mut zip, _) = make(&[stored("Disk 1.adf", &disk)], 0, false);
        zip.extend_from_slice(b"trailing");
        assert_eq!(
            both(&zip, "trailing bytes"),
            Ok(entries(&[("Disk 1.adf", &disk)]))
        );
    }

    /// Names: undecodable UTF-8 (lossy) and, without flag bit 11, ISO 8859-1; a method neither
    /// stored nor deflate is refused on an entry the filter takes and ignored on one it skips.
    #[test]
    fn names_as_decoded_and_methods_the_filter_takes() {
        let bad = E {
            name: b"Disk \xff.adf",
            ..stored("", b"bad name")
        };
        let (zip, _) = make(std::slice::from_ref(&bad), 0, false);
        assert_eq!(
            both(&zip, "lossy"),
            Ok(entries(&[("Disk \u{fffd}.adf", b"bad name")]))
        );
        let (mut latin, _) = make(&[bad], 0, false);
        let at = central_at(&latin);
        latin[at + 9] = 0; // flag bit 11 clear in the central record
        assert_eq!(
            both(&latin, "latin-1"),
            Ok(entries(&[("Disk \u{ff}.adf", b"bad name")]))
        );
        let odd = |name| E {
            method: 12,
            ..stored(name, b"bzip2?")
        };
        let (zip, _) = make(&[odd("scan.bin"), stored("Disk 1.adf", b"d")], 0, false);
        assert_eq!(both(&zip, "skipped"), Ok(entries(&[("Disk 1.adf", b"d")])));
        let (zip, _) = make(&[odd("Disk 1.adf")], 0, false);
        let err = both(&zip, "taken").unwrap_err();
        assert_eq!(err.to_string(), "unknown compression type 12");
    }

    /// Every refusal, read whole and read in slices: the same error from both.
    fn refused(zip: &[u8], what: &str) {
        assert_eq!(both(zip, what), Err(invalid()), "{what}");
    }

    #[test]
    fn a_truncated_or_inconsistent_tail_is_refused_by_both_paths() {
        let (zip, _) = make(&[stored("Disk 1.hfe", b"one")], 0, false);
        let size = zip.len() as u64;
        let (e, at) = (zip.len() - 22, central_at(&zip));
        assert!(
            directory(size, 0, &zip[..zip.len() - 1], disks).is_err(),
            "not to the end"
        );
        assert!(directory(size, 1, &zip, disks).is_err(), "past the end");
        let cut = zip[..zip.len() - 10].to_vec();
        let mut far = zip.clone();
        far[e + 16..e + 20].copy_from_slice(&(size as u32 + 100).to_le_bytes());
        let mut unsigned = zip.clone();
        unsigned[at] ^= 1;
        let mut more = zip.clone();
        more[e + 8] = 2; // two records counted, one there
        let mut long_name = zip.clone();
        long_name[at + 28] = 0xff; // a name running into the end record and past the end
        for (what, z) in [
            ("the end record cut", &cut),
            ("the directory past the end", &far),
            ("a central record without its signature", &unsigned),
            ("more records counted than there are", &more),
            ("a name past the end", &long_name),
        ] {
            refused(z, what);
        }
    }

    #[test]
    fn an_entry_range_past_the_end_of_the_file_is_refused_by_both_paths() {
        let (zip, _) = make(&[stored("Disk 1.hfe", b"one")], 0, false);
        let at = central_at(&zip);
        let mut long = zip.clone();
        long[at + 20..at + 24].copy_from_slice(&(zip.len() as u32).to_le_bytes());
        let mut far = zip.clone();
        far[at + 42..at + 46].copy_from_slice(&(zip.len() as u32 - 40).to_le_bytes());
        refused(&long, "data past the end");
        refused(&far, "a local header past the end");
        let (zip64, _) = make(&[stored("Disk 1.hfe", b"one")], 0, true);
        let at = u64le(&zip64, zip64.len() - 22 - 20 - 56 + 48).unwrap() as usize;
        let field = at + 46 + "Disk 1.hfe".len() + 4; // the Zip64 values: full, packed, local
        let mut top = zip64.clone();
        top[field + 16..field + 24].copy_from_slice(&u64::MAX.to_le_bytes());
        refused(&top, "a Zip64 offset at the top");
        let mut huge = zip64.clone();
        huge[field..field + 8].copy_from_slice(&(4u64 << 30).to_le_bytes());
        refused(&huge, "4 GiB declared unpacked");
    }

    /// A Zip64 field shorter than the values it replaces (its size word 8 for three sentinels;
    /// the bytes after it are another field's), and one cut by the end of the extra field.
    #[test]
    fn malformed_zip64_fields_are_refused_by_both_paths() {
        let (zip64, _) = make(&[stored("Disk 1.hfe", b"one")], 0, true);
        let at = u64le(&zip64, zip64.len() - 22 - 20 - 56 + 48).unwrap() as usize;
        let size_word = at + 46 + "Disk 1.hfe".len() + 2;
        let mut short = zip64.clone();
        short[size_word] = 8;
        refused(&short, "a field too short for its sentinels");
        let mut cut = zip64.clone();
        cut[at + 30] = 4 + 16; // the extra field ends after two of the three values
        refused(&cut, "a field cut by the end of the extra field");
    }

    #[test]
    fn a_local_header_that_disagrees_with_its_central_record_is_refused_by_both_paths() {
        let (zip, exact) = make(
            &[
                stored("Disk 1.hfe", b"first"),
                stored("Disk 2.hfe", b"second"),
            ],
            0,
            false,
        );
        let second = exact[1].0 as usize;
        let edited = |edit: &dyn Fn(&mut Vec<u8>)| {
            let mut z = zip.clone();
            edit(&mut z);
            z
        };
        for (what, z) in [
            ("no local signature", edited(&|z| z[second] ^= 1)),
            ("another name", edited(&|z| z[second + 30 + 5] = b'3')),
            ("another method", edited(&|z| z[second + 8] = 8)),
            (
                "an extra field running into the directory",
                edited(&|z| z[second + 28] = 200),
            ),
            (
                "data running into the next local header",
                edited(&|z| z[28] = 3),
            ),
        ] {
            refused(&z, what);
        }
        // Rebuilt adds nothing of a refused slice.
        let (slices, _) = read_directory(&zip).unwrap();
        let mut r = Rebuilt::for_slices(&slices);
        let mut p = piece(&zip, &slices[1]).to_vec();
        p[30 + 5] = b'3';
        assert_eq!(r.add(&slices[1], &p), Err(invalid()));
        p.truncate(p.len() - 1);
        assert_eq!(
            r.add(&slices[1], &p),
            Err(invalid()),
            "a slice shorter than its range"
        );
        r.add(&slices[0], piece(&zip, &slices[0])).unwrap();
        let rebuilt = r.finish().unwrap();
        assert_eq!(
            read_zip(&rebuilt, disks),
            Ok(entries(&[("Disk 1.hfe", b"first")]))
        );
    }

    /// The record limit: [`MAX_ENTRIES`] records read; one more is refused by both paths, and a
    /// Zip64 count over it is refused before any record is read (a sliced read is not sent
    /// back for the directory).
    #[test]
    fn a_directory_over_the_record_limit_is_refused_before_any_record_is_read() {
        let names: Vec<String> = (0..MAX_ENTRIES + 1).map(|i| format!("{i:05}")).collect();
        let mut list: Vec<E> = names.iter().map(|n| stored(n, b"")).collect();
        list[0] = stored("Disk 1.adf", b"disk");
        let (at_limit, _) = make(&list[..MAX_ENTRIES as usize], 0, false);
        assert_eq!(
            both(&at_limit, "at the limit"),
            Ok(entries(&[("Disk 1.adf", b"disk")]))
        );
        let (over, _) = make(&list, 0, false);
        refused(&over, "one record over the limit");
        let (mut zip64, _) = make(&[stored("Disk 1.adf", b"disk")], 0, true);
        let z = zip64.len() - 22 - 20 - 56;
        zip64[z + 32..z + 40].copy_from_slice(&(MAX_ENTRIES + 1).to_le_bytes());
        refused(&zip64, "a Zip64 count over the limit");
        let size = zip64.len() as u64;
        let from = z as u64; // the Zip64 record onwards: the directory starts before it
        assert_eq!(
            directory(size, from, &zip64[z..], disks),
            Err(invalid()),
            "not Need"
        );
    }

    /// The directory limit: a central directory more than [`MAX_DIRECTORY_BYTES`] before the
    /// end of the file is refused before it is read; one just inside it is read.
    #[test]
    fn a_directory_too_far_before_the_end_is_refused_before_it_is_read() {
        let (zip, _) = make(&[stored("Disk 1.adf", b"disk")], 0, false);
        let e = zip.len() - 22;
        let at = central_at(&zip);
        let with_gap = |gap: usize| {
            let mut z = zip[..e].to_vec();
            z.resize(e + gap, 0); // bytes after the directory, before the end record
            z.extend_from_slice(&zip[e..]);
            z
        };
        let near = with_gap(MAX_DIRECTORY_BYTES as usize - (zip.len() - at));
        assert_eq!(near.len() - at, MAX_DIRECTORY_BYTES as usize);
        assert_eq!(
            both(&near, "at the limit"),
            Ok(entries(&[("Disk 1.adf", b"disk")]))
        );
        let far = with_gap(MAX_DIRECTORY_BYTES as usize - (zip.len() - at) + 1);
        refused(&far, "one byte over the limit");
        let size = far.len() as u64;
        let from = tail_start(size);
        let tail = &far[from as usize..];
        assert_eq!(
            directory(size, from, tail, disks),
            Err(invalid()),
            "not Need"
        );
    }

    /// The zip [`Rebuilt`] assembles fits the room [`Rebuilt::for_slices`] reserved for it.
    #[test]
    fn the_rebuilt_zip_fits_its_reserved_room() {
        let (zip, _) = make(
            &[
                stored("Disk 1.adf", &[1; 3000]),
                deflated("d/Disk 2.hfe", &[2; 9000]),
            ],
            7,
            true,
        );
        let (slices, _) = read_directory(&zip).unwrap();
        let mut r = Rebuilt::for_slices(&slices);
        let room = r.out.capacity();
        for s in &slices {
            r.add(s, piece(&zip, s)).unwrap();
        }
        let rebuilt = r.finish().unwrap();
        assert_eq!(rebuilt.capacity(), room, "no reallocation");
        assert!(rebuilt.len() <= room);
    }

    #[test]
    fn the_inflate_limit_and_repeated_names_carry_over() {
        let data = b"inflate me ".repeat(50);
        let (mut zip, _) = make(
            &[
                deflated("Disk 1.adf", &data),
                stored("x/Disk 2.adf", b"a"),
                stored("Disk 1.adf", b"again"),
            ],
            0,
            false,
        );
        let same = both(&zip, "repeated").unwrap();
        assert_eq!(
            same,
            entries(&[("Disk 1.adf", b"again"), ("x/Disk 2.adf", b"a")])
        );
        // The first entry declares 10 bytes unpacked: inflating stops there, read whole or sliced.
        let at = central_at(&zip);
        zip[at + 24..at + 28].copy_from_slice(&10u32.to_le_bytes());
        let (slices, _) = read_directory(&zip).unwrap();
        assert_eq!(slices[0].unpacked(), 10);
        let err = both(&zip, "over its declared size").unwrap_err();
        assert!(
            err.to_string().starts_with("Disk 1.adf: inflate failed"),
            "{err}"
        );
    }
}

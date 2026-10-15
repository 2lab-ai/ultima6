//! Minimal zip reader (stored and deflate entries), after fflate's `unzipSync` as used by the
//! former TypeScript intake (removed after the Rust port reached parity): find the
//! end-of-central-directory record, walk the central directory, ask the filter per entry name
//! *before* touching its data, then locate the data through the local header. Every entry read
//! ([`entry`], [`read_zip`]) is checked against its central record: stored or deflated, its
//! bytes must be exactly the uncompressed size it declares, and its CRC-32 must match. Zip64
//! sizes and offsets are honoured.
//!
//! A zip too large to copy whole is read in slices instead: the caller reads the end of the
//! file from [`tail_start`]; [`directory`] walks the central directory (naming an earlier offset
//! to read from when the directory or the Zip64 end record starts before what was read) and
//! returns the byte range of each entry the filter takes, from its local header to the end of
//! its data; the caller reads those ranges, and [`Rebuilt`] checks each and assembles a zip of
//! only those entries, which [`read_zip`] then reads like the zip they came from.
//!
//! Whole and sliced take the same zips: [`read_zip`] is [`directory`] over the whole file and
//! the same check of each entry's range ([`Slice`]), so both refuse alike (each of these is
//! stricter than the reader before slicing):
//! - more than [`MAX_ENTRIES`] central records, or a central directory starting more than
//!   [`MAX_DIRECTORY_BYTES`] before the end of its end record (both before any record is read),
//!   or whose records run past the start of its end records;
//! - an entry the filter takes with a method other than stored or deflate, or 4 GiB or more
//!   packed or declared unpacked;
//! - an entry whose local header and data do not fit before the end of the file;
//! - an entry whose local header and data, at their shortest (no local extra field), would
//!   run into the next record: any record starting after its local header and before that end
//!   (another entry's local header, the central directory, the end records); the whole zip is
//!   refused even when that record is one the filter skips (a skipped entry's local offset
//!   pointing into a taken entry's data);
//! - a local header without its signature, with another name or method than its central
//!   record, or whose data runs past the next record or more than the longest extra field
//!   past where it would end without one;
//! - a Zip64 field too short for the values it replaces;
//! - an end record whose comment would run past the end of the file (it is not a record; see
//!   the rule below).
//!
//! The end record read, one rule for both paths (every case is a row of the test
//! `the_end_record_read_by_one_rule`):
//! 1. the candidates are the signatures within the last 65,558 bytes whose comment fits in the
//!    file (it starts at the end of the 22-byte record and does not run past the end of the file);
//!    each names its directory: its own fields or, when they hold a Zip64 sentinel and a Zip64
//!    locator precedes it, the Zip64 end record the locator names; that directory is the one
//!    walked if it is chosen;
//! 2. a candidate inside the comment of another that fits as well or better (by 3 and 4) is
//!    comment data, not a record;
//! 3. a candidate whose directory holds records (at most [`MAX_ENTRIES`]), ends where its end
//!    records start and starts with a central record fits best;
//! 4. then one whose comment ends exactly at the end of the file;
//! 5. of the candidates that fit best, the last in the file (the first found scanning back).
//!
//! All candidates are weighed in one pass. When some need bytes before those read, the sliced
//! reader asks once for the lowest of them, each within [`MAX_DIRECTORY_BYTES`] of the end of
//! the candidate's record, so within that and 64 KiB of the end of the file. What a candidate
//! names is known from the tail (its Zip64 end record, or its directory) or from its Zip64 end
//! record (the directory there), and every candidate with records within the limits has the
//! start of its directory read while it is weighed, so the walk of the one chosen reads nothing
//! more: a sliced read from [`tail_start`] reads the end of the file at most three times (the
//! tail, once from the lowest Zip64 end record or directory the tail names, once from the
//! lowest directory a Zip64 end record names). The end-record bytes cannot tell the real zip
//! from any record whose directory starts with a central record and ends at it, a prefix of the
//! real directory included (a record in a central record's comment field), or a whole zip
//! planted in entry data or in the comment: such a record whose comment holds the real record
//! is read instead of it (the outer wins), on both paths alike. A planted record that is not
//! such a one (no records, no central record where its directory should start, its directory
//! ending elsewhere) does not win over the real one. The zip [`Rebuilt`] assembles is
//! read at its own end record ([`REBUILT_COMMENT`]), so the choice made on the original holds.
//!
//! The one difference left: [`Rebuilt`] refuses to grow past 4 GiB (its offsets are 32-bit),
//! which the sliced reader's caller never reaches (the page reads at most 512 MiB in all).

use std::collections::HashMap;

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
/// The comment of a [`Rebuilt`] zip: the longest, zeros, so the end-record scan over that zip
/// covers only its own end record.
pub const REBUILT_COMMENT: usize = 0xffff;

/// The most central records a zip may hold: the most an end record without Zip64 counts (it
/// writes 0xffff for "see the Zip64 record"), so a zip of the entries taken ([`Rebuilt`])
/// always holds them.
pub const MAX_ENTRIES: u64 = 0xfffe;
/// The most bytes from the first central record to the end of the end-of-central-directory
/// record that names it: the central directory and the end records (the comment and any bytes
/// after it not counted, so a zip and the zip of its entries [`Rebuilt`] assembles, which adds
/// a comment, are held to the same limit; 65,534 records with 200-byte names fit).
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

/// How well an end-of-central-directory candidate explains the file, best first (module doc):
/// its directory holds records, ends where its end records start and starts with a central
/// record; its comment reaches the end of the file exactly; neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Fit {
    Listed = 0,
    ToEnd = 1,
    Found = 2,
}

/// The central directory an end record names: its number of records, its length, where the
/// first starts, and where the end records start (the Zip64 end record when it is read, else
/// the end-of-central-directory record).
#[derive(Clone, Copy)]
struct Ends {
    count: u64,
    len: u64,
    at: u64,
    records: u64,
}

/// An end-of-central-directory candidate: where it starts, where its comment ends, its fit, and
/// the directory it names (what is walked when it is chosen).
struct Candidate {
    at: u64,
    end: u64,
    fit: Fit,
    ends: Ends,
}

/// Whether the file holds the signature `sig` at `at` (false past its end); `Need(at)` when `at`
/// lies before the bytes held.
fn signed(v: &View, at: u64, size: u64, sig: u32) -> Result<bool, Stop> {
    if at.checked_add(4).is_none_or(|end| end > size) {
        return Ok(false);
    }
    Ok(v.u32(at)? == sig)
}

/// The candidate at `e` of a file of `size` bytes, or `None` when its comment does not fit in
/// the file (it is not a record). Its directory: the record's own fields or, when they hold a
/// Zip64 sentinel and a Zip64 locator precedes the record, the Zip64 end record it names (the
/// one reading for ranking and walking). `Need` when the Zip64 end record or the directory it
/// would be listed by (each at most [`MAX_DIRECTORY_BYTES`] before the end) lies before the
/// bytes held.
fn candidate(v: &View, e: u64, size: u64) -> Result<Option<Candidate>, Stop> {
    let end = e + 22 + v.u16(e + 20)? as u64;
    if end > size {
        return Ok(None);
    }
    let mut ends = Ends {
        count: v.u16(e + 8)? as u64,
        len: u64::from(v.u32(e + 12)?),
        at: u64::from(v.u32(e + 16)?),
        records: e,
    };
    let zip64 = ends.at == u64::from(SENTINEL32) || ends.count == 0xffff;
    if zip64 && e >= 20 + 56 && v.u32(e - 20)? == EOCD64_LOCATOR {
        let z = v.u64(e - 12)?;
        if z <= e - 20 - 56 && near_end(z, e + 22).is_ok() && signed(v, z, size, EOCD64)? {
            ends = Ends {
                count: v.u64(z + 32)?,
                len: v.u64(z + 40)?,
                at: v.u64(z + 48)?,
                records: z,
            };
        }
    }
    // Whether the directory starts with a central record is read for every candidate within
    // the limits (one over them is refused once chosen, before any of it is read), so the
    // bytes the walk of the chosen one reads are held by then.
    let headed = (1..=MAX_ENTRIES).contains(&ends.count)
        && near_end(ends.at, e + 22).is_ok()
        && signed(v, ends.at, size, CENTRAL)?;
    let listed = headed && ends.at.checked_add(ends.len) == Some(ends.records);
    let fit = match () {
        _ if listed => Fit::Listed,
        _ if end == size => Fit::ToEnd,
        _ => Fit::Found,
    };
    Ok(Some(Candidate {
        at: e,
        end,
        fit,
        ends,
    }))
}

/// The end-of-central-directory record of a file of `size` bytes, by the rule in the module doc.
/// Every candidate in the scan is weighed in one pass; when any needs bytes before those held
/// (a Zip64 end record, a directory), the walk asks once for the lowest of them (and for the
/// whole scan first, when the bytes held do not cover it).
fn end_record(v: &View, size: u64) -> Result<Candidate, Stop> {
    if v.base > tail_start(size) {
        return Err(Stop::Need(tail_start(size)));
    }
    let low = size.saturating_sub(MAX_COMMENT_SCAN as u64);
    let mut found = Vec::new();
    let mut need: Option<u64> = None;
    let mut e = size - 22;
    loop {
        if v.u32(e)? == EOCD {
            match candidate(v, e, size) {
                Ok(Some(c)) => found.push(c),
                Ok(None) => {}
                Err(Stop::Need(z)) => need = Some(need.map_or(z, |n| n.min(z))),
                Err(stop) => return Err(stop),
            }
        }
        if e == low {
            break;
        }
        e -= 1;
    }
    if let Some(z) = need {
        return Err(Stop::Need(z));
    }
    found.reverse(); // by position
    // Bytes inside the comment of a candidate that fits as well or better are comment data:
    // `reach[f]`, the furthest comment end of the candidates of fit `f` or better that start at
    // least a record before the one weighed.
    let mut reach = [0u64; 3];
    let mut before = 0;
    let mut best: Option<usize> = None;
    for (i, x) in found.iter().enumerate() {
        while let Some(y) = found.get(before).filter(|y| y.at + 22 <= x.at) {
            for r in &mut reach[y.fit as usize..] {
                *r = (*r).max(y.end);
            }
            before += 1;
        }
        if reach[x.fit as usize] > x.at {
            continue;
        }
        // The best fit; of equals, the last in the file.
        if best.is_none_or(|b| x.fit <= found[b].fit) {
            best = Some(i);
        }
    }
    match best {
        Some(b) => Ok(found.swap_remove(b)),
        None => Err(invalid().into()),
    }
}

/// A central directory or Zip64 end record at `at` lies at most [`MAX_DIRECTORY_BYTES`] before
/// `end`, the end of the end-of-central-directory record that names it (its comment and any
/// bytes after it not counted).
fn near_end(at: u64, end: u64) -> Result<(), Stop> {
    match end.checked_sub(at) {
        Some(n) if n <= MAX_DIRECTORY_BYTES => Ok(()),
        _ => Err(invalid().into()),
    }
}

/// The [`Ends`] of a file of `size` bytes: the directory of the end record [`end_record`] chose,
/// as it ranked it. Refuses more than [`MAX_ENTRIES`] records and a directory more than
/// [`MAX_DIRECTORY_BYTES`] before the end, before any central record is read.
fn end_records(v: &View, size: u64) -> Result<Ends, Stop> {
    if size < 22 {
        return Err(invalid().into());
    }
    let chosen = end_record(v, size)?;
    let ends = chosen.ends;
    if ends.count == 0 {
        return Ok(Ends {
            at: ends.records,
            ..ends
        });
    }
    if ends.count > MAX_ENTRIES {
        return Err(invalid().into());
    }
    near_end(ends.at, chosen.at + 22)?;
    Ok(ends)
}

/// One central-directory record, its Zip64 values applied (its name as stored: decoded only
/// for the filter, and for the output of the entries taken).
struct Central<'a> {
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
    let extra = v.at(name_at + name_len as u64, extra_len)?;
    zip64_fields(extra, &mut full, &mut csize, &mut local)?;
    Ok(Central {
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
    // Each name decoded once; a repeated one takes the first one's place (a name -> place
    // index over those names, no copies of them).
    let names: Vec<String> = slices.iter().map(Slice::name).collect();
    let mut first: HashMap<&str, usize> = HashMap::with_capacity(names.len());
    let place: Vec<usize> = names
        .iter()
        .enumerate()
        .map(|(i, n)| *first.entry(n.as_str()).or_insert(i))
        .collect();
    drop(first);
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut at: Vec<usize> = vec![usize::MAX; slices.len()];
    for ((i, s), name) in slices.iter().enumerate().zip(names) {
        let bytes = entry(data, s)?;
        if place[i] == i {
            at[i] = out.len();
            out.push((name, bytes));
        } else {
            out[at[place[i]]].1 = bytes;
        }
    }
    Ok(out)
}

/// The bytes of the entry `s` of the zip `data` (the whole file, as [`directory`] was given
/// it from offset 0): its local header checked, inflated up to the size it declares, then held
/// to exactly that size and to its CRC-32.
pub fn entry(data: &[u8], s: &Slice) -> Result<Vec<u8>, Error> {
    let name = s.name();
    let range = data.get(to_usize(s.start)?..to_usize(s.end)?);
    let (_, body) = s.local(range.ok_or_else(invalid)?)?;
    let bytes = match s.method {
        0 => body.to_vec(),
        8 => miniz_oxide::inflate::decompress_to_vec_with_limit(body, to_usize(s.full)?)
            .map_err(|err| Error::new(format!("{name}: inflate failed: {err}")))?,
        m => return Err(unknown_method(m)),
    };
    // Stored or inflated, the bytes are the uncompressed size the central record declares.
    if bytes.len() as u64 != s.full {
        return Err(Error::new(format!(
            "{name}: holds another size than it declares"
        )));
    }
    if crc32fast::hash(&bytes) != s.crc {
        return Err(Error::new(format!("{name}: CRC-32 mismatch")));
    }
    Ok(bytes)
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

    /// The CRC-32 its central record declares (checked by [`entry`]).
    pub fn crc(&self) -> u32 {
        self.crc
    }

    /// The entry's name as [`read_zip`] reports it.
    pub fn name(&self) -> String {
        entry_name(&self.raw_name, self.flags & 0x800 != 0)
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
    let Ends {
        count,
        mut at,
        records: ends,
        ..
    } = end_records(v, size)?;
    // The records an entry's data ends before: every local header, the central directory and
    // the end records (`count` is at most MAX_ENTRIES).
    let mut records = Vec::with_capacity(count as usize + 2);
    records.extend([at, ends]);
    let mut kept = Vec::new();
    for _ in 0..count {
        let c = central_entry(v, at)?;
        at = c.next;
        // The directory lies before its end records (the limit is measured to them).
        if at > ends {
            return Err(invalid().into());
        }
        records.push(c.local);
        if keep(&entry_name(c.raw_name, c.flags & 0x800 != 0)) {
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
        // The first record after this local header: the entry must end before it.
        let mut end = size.min(least.saturating_add(MAX_LOCAL_EXTRA));
        if let Some(&next) = records.get(records.partition_point(|&r| r <= c.local)) {
            if next < least {
                return Err(invalid().into());
            }
            end = end.min(next);
        }
        slices.push(Slice {
            start: c.local,
            end,
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
///
/// The end record is chosen once, on the zip they came from: the end record of this zip carries
/// a comment of [`REBUILT_COMMENT`] zero bytes, so the end-record scan over it (the last 65,558
/// bytes) sees nothing of the entries, and its own record is the only candidate there. Nothing
/// planted in an entry's data, at offsets that would match only in this zip, is ever weighed.
#[derive(Debug, Default)]
pub struct Rebuilt {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

impl Rebuilt {
    /// The zip of the entries `slices` name, and its central directory while it is assembled:
    /// (their ranges, a central record each and the end record; the central records).
    fn room(slices: &[Slice]) -> (u64, u64) {
        let ranges = slices
            .iter()
            .fold(0u64, |sum, s| sum.saturating_add(s.len()));
        let central = slices
            .iter()
            .map(|s| 46 + s.raw_name.len() as u64)
            .sum::<u64>();
        (
            ranges.saturating_add(central + 22 + REBUILT_COMMENT as u64),
            central,
        )
    }

    /// What assembling the zip of the entries `slices` name holds at most: the room
    /// [`Rebuilt::for_slices`] reserves. The caller bounds it before reserving it.
    pub fn holds(slices: &[Slice]) -> u64 {
        let (zip, central) = Rebuilt::room(slices);
        zip.saturating_add(central)
    }

    /// Room for the zip of the entries `slices` name ([`Rebuilt::holds`]), so assembling it
    /// never reallocates.
    pub fn for_slices(slices: &[Slice]) -> Self {
        let (zip, central) = Rebuilt::room(slices);
        let reserve = |n: u64| Vec::with_capacity(usize::try_from(n).unwrap_or(0));
        Rebuilt {
            out: reserve(zip),
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
        out.extend_from_slice(&(REBUILT_COMMENT as u16).to_le_bytes());
        out.resize(out.len() + REBUILT_COMMENT, 0);
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
    ///
    /// `descriptor`: a data descriptor after the data (with its signature or not, Zip64 sizes or
    /// not), flag bit 3 set and the local CRC and sizes 0, as a streaming writer leaves them.
    /// `local64`: the local sizes as sentinels and a Zip64 field in the local header. `share`:
    /// no local header of its own; its central record names entry `share.0`'s local header
    /// plus `share.1` bytes.
    #[derive(Clone)]
    struct E<'a> {
        name: &'a [u8],
        data: &'a [u8],
        method: usize,
        descriptor: Option<(bool, bool)>,
        local64: bool,
        share: Option<(usize, u64)>,
    }

    fn stored<'a>(name: &'a str, data: &'a [u8]) -> E<'a> {
        E {
            name: name.as_bytes(),
            data,
            method: 0,
            descriptor: None,
            local64: false,
            share: None,
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
        let (mut out, mut central, mut ranges) = (Vec::new(), Vec::new(), Vec::<(u64, u64)>::new());
        for e in entries {
            let body = if e.method == 8 {
                miniz_oxide::deflate::compress_to_vec(e.data, 6)
            } else {
                e.data.to_vec()
            };
            let crc = crc32fast::hash(e.data);
            let field = |v: u64| if o.zip64 { SENTINEL } else { v as u32 };
            let flags = 0x800 | if e.descriptor.is_some() { 8 } else { 0 };
            let local = match e.share {
                Some((k, plus)) => {
                    let (at, end) = ranges[k];
                    ranges.push((at + plus, end));
                    at + plus
                }
                None => {
                    let local = out.len() as u64;
                    let (csize, full) = (body.len() as u64, e.data.len() as u64);
                    let sizes = match () {
                        _ if e.descriptor.is_some() => [0, 0],
                        _ if e.local64 => [SENTINEL, SENTINEL],
                        _ => [field(csize), field(full)],
                    };
                    let mut extra = Vec::new();
                    if e.local64 {
                        put16(&mut extra, 1);
                        put16(&mut extra, 16);
                        put64(&mut extra, full);
                        put64(&mut extra, csize);
                    }
                    if o.pad > 0 {
                        put16(&mut extra, 0xcafe);
                        put16(&mut extra, o.pad);
                        extra.resize(extra.len() + o.pad, 0);
                    }
                    put32(&mut out, LOCAL);
                    put16(&mut out, 20);
                    put16(&mut out, flags);
                    put16(&mut out, e.method);
                    put32(&mut out, 0); // DOS time + date
                    put32(&mut out, if e.descriptor.is_some() { 0 } else { crc });
                    put32(&mut out, sizes[0]);
                    put32(&mut out, sizes[1]);
                    put16(&mut out, e.name.len());
                    put16(&mut out, extra.len());
                    out.extend_from_slice(e.name);
                    out.extend_from_slice(&extra);
                    out.extend_from_slice(&body);
                    ranges.push((local, out.len() as u64));
                    if let Some((signed, zip64)) = e.descriptor {
                        if signed {
                            put32(&mut out, 0x0807_4b50);
                        }
                        put32(&mut out, crc);
                        if zip64 {
                            put64(&mut out, csize);
                            put64(&mut out, full);
                        } else {
                            put32(&mut out, csize as u32);
                            put32(&mut out, full as u32);
                        }
                    }
                    local
                }
            };

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
            put16(&mut central, flags);
            put16(&mut central, e.method);
            put32(&mut central, 0);
            put32(&mut central, crc);
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

    /// `zip` read whole, and read in slices (the directory, the slices, the rebuilt zip).
    fn paths(zip: &[u8]) -> (Read, Read) {
        let whole = read_zip(zip, disks);
        let sliced = read_directory(zip)
            .and_then(|(slices, _)| rebuild(zip, &slices))
            .and_then(|rebuilt| read_zip(&rebuilt, disks));
        (whole, sliced)
    }

    #[test]
    fn crc_and_inflated_size_are_checked_on_both_paths() {
        let disk: Vec<u8> = (0..4000u32).map(|i| (i * 7 % 251) as u8).collect();
        let (zip, exact) = make(&[stored("Disk 1.adf", &disk)], 0, false);
        assert_eq!(both(&zip, "intact"), Ok(entries(&[("Disk 1.adf", &disk)])));
        let refused = |z: &[u8], what: &str, why: &str| {
            let (whole, sliced) = paths(z);
            for (path, r) in [("whole", whole), ("sliced", sliced)] {
                let e = match r {
                    Ok(read) => panic!("{what} ({path}) read {} entries", read.len()),
                    Err(e) => e.to_string(),
                };
                assert!(e.contains(why), "{what} ({path}): {e}");
            }
        };
        // One data bit flipped: the CRC no longer matches.
        let mut flipped = zip.clone();
        flipped[exact[0].0 as usize + 30 + 10 + 100] ^= 0x04;
        refused(&flipped, "a flipped bit", "CRC-32");
        // The central record declaring another uncompressed size: stored (its data is the
        // packed size) and deflated (it inflates to what it holds).
        for (entry, delta) in [
            (stored("Disk 1.adf", &disk), 1i64),
            (stored("Disk 1.adf", &disk), -1),
            (deflated("Disk 1.adf", &disk), 1),
            (deflated("Disk 1.adf", &disk), -1),
        ] {
            let (mut z, _) = make(&[entry], 0, false);
            let cd = central_at(&z);
            let full = u32::from_le_bytes(z[cd + 24..cd + 28].try_into().unwrap());
            let full = (i64::from(full) + delta) as u32;
            z[cd + 24..cd + 28].copy_from_slice(&full.to_le_bytes());
            let why = if delta < 0 && z[cd + 10] == 8 {
                "inflate failed"
            } else {
                "another size"
            };
            refused(&z, &format!("declared size {delta:+}"), why);
        }
        // A wrong CRC in the central record alone.
        let (mut z, _) = make(&[deflated("Disk 1.adf", &disk)], 0, false);
        let cd = central_at(&z);
        z[cd + 16] ^= 1;
        refused(&z, "a changed central CRC", "CRC-32");
        // A genuine CRC of 0 (an empty entry) reads, on both paths.
        let (z, _) = make(
            &[stored("Disk 2.adf", b""), stored("Disk 1.adf", &disk)],
            0,
            false,
        );
        assert_eq!(crc32fast::hash(b""), 0);
        assert_eq!(
            both(&z, "an empty entry"),
            Ok(entries(&[("Disk 2.adf", b""), ("Disk 1.adf", &disk)]))
        );
        // Complete slices given to the rebuilt zip in another order: the same entries, checked.
        let two: Vec<u8> = disk.iter().rev().copied().collect();
        let (z, _) = make(
            &[stored("Disk 1.adf", &disk), deflated("Disk 2.adf", &two)],
            0,
            false,
        );
        let (mut slices, _) = read_directory(&z).unwrap();
        slices.reverse();
        let rebuilt = rebuild(&z, &slices).unwrap();
        let mut got = read_zip(&rebuilt, disks).unwrap();
        got.sort();
        assert_eq!(got, entries(&[("Disk 1.adf", &disk), ("Disk 2.adf", &two)]));
    }

    /// [`paths`]: the two must agree; returns what they read.
    fn both(zip: &[u8], what: &str) -> Read {
        let (whole, sliced) = paths(zip);
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
            assert_eq!(slices[0].name(), "d/Disk 1.hfe");
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
        let read = |from: u64| directory(size, from, &zip[from as usize..], disks).unwrap();
        // Bytes that do not cover the end-record scan: read from the tail first.
        assert_eq!(read(size - 22), Directory::Need(tail_start(size)));
        let Directory::Entries(slices) = read(tail_start(size)) else {
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
        // From the locator and the end record only: the whole tail first.
        assert_eq!(read(size - 22 - 20), Directory::Need(tail_start(size)));
        assert_eq!(both(&zip, "zip64"), Ok(entries(&[("Disk 1.adf", &disk)])));
        // A Zip64 directory longer than the tail: the Zip64 records in the tail, a second read
        // from where the directory starts.
        let names: Vec<String> = (0..1500)
            .map(|i| format!("flux/track-{i:04}.raw"))
            .collect();
        let mut list: Vec<E> = names.iter().map(|n| stored(n, b"f")).collect();
        list.push(deflated("Disk 1.adf", &disk));
        let (zip, exact) = make(&list, 0, true);
        let (slices, reads) = read_directory(&zip).unwrap();
        assert_eq!((reads, ranges(&slices)), (2, vec![exact[1500]]));
        assert_eq!(
            both(&zip, "a long Zip64 directory"),
            Ok(entries(&[("Disk 1.adf", &disk)]))
        );
    }

    /// An end record with no directory (counts, size and offset 0) and comment length `comment_len`.
    fn false_record(comment_len: usize) -> Vec<u8> {
        let mut r = EOCD.to_le_bytes().to_vec();
        r.extend_from_slice(&[0; 16]); // disks, counts, directory size and offset: all 0
        r.extend_from_slice(&(comment_len as u16).to_le_bytes());
        r
    }

    fn put_eocd(out: &mut Vec<u8>, count: usize, len: u32, at: u32, comment_len: usize) {
        put32(out, EOCD);
        put32(out, 0);
        put16(out, count);
        put16(out, count);
        put32(out, len);
        put32(out, at);
        put16(out, comment_len);
    }

    /// A whole stored zip of one entry (`name`, `data`) as it would lie at file offset `p0`: its
    /// local header, its central record and its end record with a comment length.
    fn inner(p0: u64, name: &[u8], data: &[u8], comment_len: usize) -> Vec<u8> {
        let mut p = Vec::new();
        let crc = crc32fast::hash(data);
        put32(&mut p, LOCAL);
        p.extend_from_slice(&[20, 0, 0, 8, 0, 0, 0, 0, 0, 0]); // version, flags, method, time
        put32(&mut p, crc);
        put32(&mut p, data.len() as u32);
        put32(&mut p, data.len() as u32);
        put16(&mut p, name.len());
        put16(&mut p, 0);
        p.extend_from_slice(name);
        p.extend_from_slice(data);
        let cd = p0 + p.len() as u64;
        put32(&mut p, CENTRAL);
        p.extend_from_slice(&[20, 0, 20, 0, 0, 8, 0, 0, 0, 0, 0, 0]);
        put32(&mut p, crc);
        put32(&mut p, data.len() as u32);
        put32(&mut p, data.len() as u32);
        put16(&mut p, name.len());
        p.extend_from_slice(&[0; 12]);
        put32(&mut p, p0 as u32);
        p.extend_from_slice(name);
        let e = p0 + p.len() as u64;
        put_eocd(&mut p, 1, (e - cd) as u32, cd as u32, comment_len);
        p
    }

    /// Zip64 end records as they would lie at `p0`: a Zip64 end record counting one record whose
    /// directory "ends" at it (offset 0, length `p0`), its locator, an end record without Zip64
    /// sentinels (no records, comment length `comment_len`).
    fn z64_empty(p0: u64, comment_len: usize) -> Vec<u8> {
        let mut p = Vec::new();
        put32(&mut p, EOCD64);
        put64(&mut p, 44);
        p.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        for v in [1, 1, p0, 0] {
            put64(&mut p, v);
        }
        put32(&mut p, EOCD64_LOCATOR);
        put32(&mut p, 0);
        put64(&mut p, p0);
        put32(&mut p, 1);
        put_eocd(&mut p, 0, 0, 0, comment_len);
        p
    }

    /// As [`z64_empty`], with a central record (naming the real disk's local header at 0) where
    /// the Zip64 directory starts.
    fn z64_over_central(p0: u64, comment_len: usize) -> Vec<u8> {
        let mut p = inner(0, b"Disk 1.adf", b"the disk", 0);
        let local = 30 + 10 + 8;
        let central = p[local..p.len() - 22].to_vec();
        p = central;
        let z = p0 + p.len() as u64;
        put32(&mut p, EOCD64);
        put64(&mut p, 44);
        p.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        for v in [1, 1, z - p0, p0] {
            put64(&mut p, v);
        }
        put32(&mut p, EOCD64_LOCATOR);
        put32(&mut p, 0);
        put64(&mut p, z);
        put32(&mut p, 1);
        put_eocd(&mut p, 0, 0, 0, comment_len);
        p
    }

    /// opus XF: two entries; the directory; the Zip64 end record; 200,000 bytes; its locator;
    /// the end record (Zip64 sentinel count, its 32-bit fields also ending at it) whose comment
    /// holds a false record after a Zip64 locator naming a place in the gap.
    fn far_zip64(disk: &[u8]) -> Row {
        let filler = vec![7u8; 300_000];
        let (base, _) = make(
            &[stored("Disk 1.adf", disk), stored("scan.bin", &filler)],
            0,
            false,
        );
        let cd = central_at(&base);
        let cd_len = base.len() - 22 - cd;
        let mut z = base[..cd + cd_len].to_vec();
        let z2 = z.len() as u64;
        put32(&mut z, EOCD64);
        put64(&mut z, 44);
        z.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        for v in [2, 2, cd_len as u64, cd as u64] {
            put64(&mut z, v);
        }
        let z1 = z.len() as u64 + 100_000;
        z.resize(z.len() + 200_000, 0);
        put32(&mut z, EOCD64_LOCATOR);
        put32(&mut z, 0);
        put64(&mut z, z2);
        put32(&mut z, 1);
        let r = z.len() as u64;
        let mut comment = Vec::new();
        put32(&mut comment, EOCD64_LOCATOR);
        put32(&mut comment, 0);
        put64(&mut comment, z1);
        put32(&mut comment, 1);
        comment.extend_from_slice(&false_record(0));
        put_eocd(
            &mut z,
            0xffff,
            (r - cd as u64) as u32,
            cd as u32,
            comment.len(),
        );
        z.extend_from_slice(&comment);
        Row {
            what: "opus XF: Zip64 records far from their locator",
            record: Some(r as usize),
            read: Ok(entries(&[("Disk 1.adf", disk)])),
            reads: Some(3),
            zip: z,
        }
    }

    /// One row of the end-record table: the file, the record the rule must read (`None`: the
    /// file is refused), what both paths read, and the reads the sliced path takes.
    struct Row {
        what: &'static str,
        zip: Vec<u8>,
        record: Option<usize>,
        read: Read,
        reads: Option<usize>,
    }

    /// The end-record rule (module doc), every known case one row: both paths must agree and
    /// read the record named, the entries named, in the reads named. The rows of planted
    /// records come from the reviews (gpt6 r2/r3, opus r2/r3).
    #[test]
    fn the_end_record_read_by_one_rule() {
        // Rows whose fixture is not what it says (checked as they are made, not asserted).
        let mut setup: Vec<String> = Vec::new();
        let disk = b"the disk".to_vec();
        let pair = [stored("scan.bin", &[1; 50]), stored("Disk 1.adf", &disk)];
        let read_disk = || Ok(entries(&[("Disk 1.adf", &disk)]));
        let with = |comment: &[u8], zip64: bool| {
            let opts = Opts {
                comment,
                zip64,
                ..Opts::default()
            };
            write(&pair, opts).0
        };
        let mut rows = Vec::new();
        let plain = |what, zip: Vec<u8>, record, reads| Row {
            what,
            record: Some(record),
            read: read_disk(),
            reads: Some(reads),
            zip,
        };

        let zip = with(b"", false);
        rows.push(plain("no comment", zip.clone(), zip.len() - 22, 1));
        let zip = with(b"hello", false);
        rows.push(plain("a short comment", zip.clone(), zip.len() - 27, 1));
        let zip = with(&[b'c'; 0xffff], false);
        let record = zip.len() - 22 - 0xffff;
        rows.push(plain("a 65,535-byte comment", zip, record, 2));
        let mut zip = with(b"", false);
        zip.extend_from_slice(b"trailing");
        rows.push(plain(
            "trailing bytes after the real record",
            zip.clone(),
            zip.len() - 30,
            1,
        ));
        let zip = with(b"zip64 note", true);
        rows.push(plain(
            "Zip64 records and a comment",
            zip.clone(),
            zip.len() - 32,
            1,
        ));

        // opus r2: trailing bytes, and a record planted in a stored disk's data whose comment
        // reaches the end of the file (it would hold the real record in its comment).
        let mut body = vec![5u8; 1000];
        body[500..522].copy_from_slice(&false_record(0));
        let (mut zip, exact) = make(&[stored("Disk 1.adf", &body)], 0, false);
        zip.extend_from_slice(b"trailing");
        let planted = exact[0].0 as usize + 30 + 10 + 500;
        let reach = (zip.len() - planted - 22) as u16;
        zip[planted + 20..planted + 22].copy_from_slice(&reach.to_le_bytes());
        let body = zip[exact[0].0 as usize + 40..][..1000].to_vec();
        // The data changed: its CRC in the local header and the central record follows.
        let crc = crc32fast::hash(&body).to_le_bytes();
        let local = exact[0].0 as usize;
        zip[local + 14..local + 18].copy_from_slice(&crc);
        let central = exact[0].1 as usize;
        zip[central + 16..central + 20].copy_from_slice(&crc);
        rows.push(Row {
            what: "opus r2: planted in a stored disk's data, its comment reaching the end",
            record: Some(zip.len() - 30),
            read: Ok(entries(&[("Disk 1.adf", &body)])),
            reads: Some(1),
            zip,
        });

        // gpt6 r2: an empty record in the real comment, directory offset 0, its comment
        // reaching the end of the file.
        let mut comment = b"note ".to_vec();
        comment.extend_from_slice(&false_record(6));
        comment.extend_from_slice(b"abcdef");
        let zip = with(&comment, false);
        let record = zip.len() - 22 - comment.len();
        rows.push(plain(
            "gpt6 r2: an empty record in the comment, offset 0",
            zip,
            record,
            1,
        ));

        // gpt6 r3: an empty record in the real comment whose directory offset is its own
        // position (an empty directory that ends where it starts), comment not to the end.
        let mut comment = b"note ".to_vec();
        comment.extend_from_slice(&false_record(0));
        comment.extend_from_slice(b"more of the comment");
        let mut zip = with(&comment, false);
        let record = zip.len() - 22 - comment.len();
        let planted = record + 22 + 5;
        zip[planted + 16..planted + 20].copy_from_slice(&(planted as u32).to_le_bytes());
        rows.push(plain(
            "gpt6 r3: an empty record in the comment at its own offset",
            zip,
            record,
            1,
        ));

        // A record in the real comment that lists records and whose directory, also in the
        // comment, ends where it starts: comment data all the same (the real record fits as well
        // and holds it in its comment).
        let mut comment = vec![b'd'; 46];
        let mut listed = false_record(0);
        listed[8..12].copy_from_slice(&[1, 0, 1, 0]); // one record
        listed[12..16].copy_from_slice(&46u32.to_le_bytes());
        comment.extend_from_slice(&listed);
        let mut zip = with(&comment, false);
        let record = zip.len() - 22 - comment.len();
        let planted = record + 22 + 46;
        zip[planted + 16..planted + 20].copy_from_slice(&((planted - 46) as u32).to_le_bytes());
        rows.push(plain("a listing record in the comment", zip, record, 1));

        // A record counting no records over the real directory, in trailing bytes (after the
        // real record's comment, so not inside it): a directory listing none fits no better
        // than a signature; the real record is read.
        let mut zip = with(b"", false);
        let real = zip.len() - 22;
        let cd = central_at(&zip);
        let planted = zip.len();
        put_eocd(&mut zip, 0, (planted - cd) as u32, cd as u32, 0);
        rows.push(plain(
            "a record counting none over the real directory",
            zip,
            real,
            1,
        ));

        // An empty record in trailing bytes at its own offset, its comment reaching the end.
        let mut zip = with(b"", false);
        let planted = zip.len();
        zip.extend_from_slice(&false_record(0));
        zip[planted + 16..planted + 20].copy_from_slice(&(planted as u32).to_le_bytes());
        rows.push(plain(
            "an empty record in trailing bytes at its own offset",
            zip,
            planted - 22,
            1,
        ));

        // opus r3: the comment full of false records, each after a Zip64 locator naming a Zip64
        // end record before the bytes the first read holds (the later the record, the higher
        // its offset): one more read, from the lowest, not one per record.
        let fakes = 1560;
        let mut comment = Vec::new();
        for j in 0..fakes {
            comment.extend_from_slice(&EOCD64_LOCATOR.to_le_bytes());
            comment.extend_from_slice(&0u32.to_le_bytes());
            comment.extend_from_slice(&(1000 + 64 * j as u64).to_le_bytes());
            comment.extend_from_slice(&1u32.to_le_bytes());
            let mut fake = false_record(0);
            fake[8..12].copy_from_slice(&[0xff; 4]); // the Zip64 sentinel count: follow the locator
            comment.extend_from_slice(&fake);
        }
        let filler = vec![9u8; 120_000];
        let opts = Opts {
            comment: &comment,
            ..Opts::default()
        };
        let (zip, _) = write(
            &[stored("scan.bin", &filler), stored("Disk 1.adf", &disk)],
            opts,
        );
        if 1000 + 64 * fakes as u64 > tail_start(zip.len() as u64) {
            setup.push("opus r3: the Zip64 end records named are not before the tail".to_string());
        }
        let record = zip.len() - 22 - comment.len();
        rows.push(plain(
            "opus r3: 1,560 false records after Zip64 locators",
            zip,
            record,
            2,
        ));

        // The rebuilt zip (opus XRB0, XRB1; gpt6): a record planted in the kept disk's data at
        // the offsets the zip of the kept entries had before it carried its own comment, so it
        // would be the best fit there: the end record is chosen once, on the original, and both
        // paths read the real disk.
        let planted_in_rebuilt = |what, payload: &dyn Fn(u64, usize) -> Vec<u8>, after: bool| {
            let head = b"disk head ".repeat(10);
            let p0 = 30 + 10 + head.len() as u64; // the kept disk's local header at 0 there
            let len = head.len() + payload(p0, 0).len();
            let planted = p0 + payload(p0, 0).len() as u64 - 22;
            let end = 30 + 10 + len as u64 + 46 + 10 + 22; // without the zero comment
            let mut data = head.clone();
            data.extend_from_slice(&payload(p0, (end - planted - 22) as usize));
            let filler = vec![3u8; 1000];
            let mut list = vec![stored("scan.bin", &filler), stored("Disk 1.adf", &data)];
            if after {
                list.push(stored("scan 2.bin", &filler));
            }
            let (zip, _) = make(&list, 0, false);
            Row {
                what,
                record: Some(zip.len() - 22),
                read: Ok(entries(&[("Disk 1.adf", &data)])),
                reads: Some(1),
                zip,
            }
        };
        let inner_disk = |p0: u64, clen: usize| inner(p0, b"Disk 2.adf", b"planted", clen);
        rows.push(planted_in_rebuilt(
            "opus XRB0: a zip planted at rebuilt offsets",
            &inner_disk,
            false,
        ));
        rows.push(planted_in_rebuilt(
            "opus XRB1: Zip64 records planted at rebuilt offsets",
            &z64_empty,
            false,
        ));
        rows.push(planted_in_rebuilt(
            "gpt6: matching only once skipped entries are dropped",
            &inner_disk,
            true,
        ));

        // A whole zip planted in the last entry's data, with a real directory listing a disk, its
        // comment holding the real record (opus XR1): the end-record bytes cannot tell it from
        // the real one; both paths read the planted (outer) one, as the module doc says.
        let fake = b"the planted disk".to_vec();
        let in_last = |payload: &[u8]| {
            make(
                &[stored("Disk 1.adf", &disk), stored("scan.bin", payload)],
                0,
                false,
            )
        };
        let at_scan = |exact: &[(u64, u64)]| exact[1].0 + 30 + 8;
        let (probe, exact) = in_last(&inner(0, b"Disk 1.adf", &fake, 0));
        let p0 = at_scan(&exact);
        let planted = p0 + inner(0, b"Disk 1.adf", &fake, 0).len() as u64 - 22;
        let clen = (probe.len() as u64 - planted - 22) as usize;
        let (zip, _) = in_last(&inner(p0, b"Disk 1.adf", &fake, clen));
        rows.push(Row {
            what: "opus XR1: a whole zip planted in entry data, holding the real record",
            record: Some(planted as usize),
            read: Ok(entries(&[("Disk 1.adf", &fake)])),
            reads: Some(1),
            zip,
        });

        // Planted in entry data, their comments holding the real record, but not complete
        // zips: the real one is read. opus XR2: Zip64 records whose directory "ends" at them but
        // holds no record (the end record carries no Zip64 sentinel); the same with a central
        // record there; a 32-bit record declaring a directory that is not there.
        let planted_record = |what, payload: &dyn Fn(u64, usize) -> Vec<u8>| {
            let (probe, exact) = in_last(&payload(0, 0));
            let p0 = at_scan(&exact);
            let planted = p0 + payload(0, 0).len() as u64 - 22;
            let clen = (probe.len() as u64 - planted - 22) as usize;
            let (zip, _) = in_last(&payload(p0, clen));
            plain(what, zip.clone(), zip.len() - 22, 1)
        };
        rows.push(planted_record(
            "opus XR2: Zip64 records without a sentinel",
            &z64_empty,
        ));
        rows.push(planted_record(
            "Zip64 records over a central record, without a sentinel",
            &|p0, clen| z64_over_central(p0, clen),
        ));
        rows.push(planted_record(
            "a declared directory that is not there",
            &|p0, clen| {
                let mut p = vec![0u8; 46]; // where the directory should be: no central record
                put_eocd(&mut p, 1, 46, p0 as u32, clen);
                p
            },
        ));

        // opus XF: the real record's Zip64 end record 200 KB before its locator, its directory
        // before that, a false record after a Zip64 locator in the comment: three reads (the
        // tail, from the Zip64 end record, from the directory).
        rows.push(far_zip64(&disk));

        // Of equal fits, the last: a whole zip planted in entry data whose comment does not hold
        // the real record (both list records; neither holds the other).
        let (probe, exact) = in_last(&inner(0, b"Disk 1.adf", &fake, 0));
        let p0 = at_scan(&exact);
        let (zip, _) = in_last(&inner(p0, b"Disk 1.adf", &fake, 0));
        if zip.len() != probe.len() {
            setup.push("of two listing records: the planted zip moved".to_string());
        }
        rows.push(plain(
            "of two listing records, the last",
            zip.clone(),
            zip.len() - 22,
            1,
        ));

        // A comment ending at the end of the file over a record that does not: an empty zip
        // whose disk-number fields read as a signature, the second record inside the first's
        // 22 bytes (not in its comment), its comment fitting short of the end.
        let mut zip = false_record(40);
        zip[4..8].copy_from_slice(&EOCD.to_le_bytes());
        zip.extend_from_slice(&[b'c', b'c', 3, 0]); // the inner signature's comment length: 3
        zip.resize(22 + 40, b'c');
        rows.push(Row {
            what: "a comment to the end over one short of it",
            zip,
            record: Some(0),
            read: Ok(Vec::new()),
            reads: Some(1),
        });

        // opus XN1: the chosen record does not list (bytes between its directory and it); a
        // false record in its comment names Zip64 records naming a directory that is not
        // there. Every directory start is read while the candidates are weighed: two reads.
        let filler = vec![7u8; 300_000];
        let (base, _) = make(
            &[stored("Disk 1.adf", &disk), stored("scan.bin", &filler)],
            0,
            false,
        );
        let cd = central_at(&base);
        let cd_len = base.len() - 22 - cd;
        let mut z = base[..cd + cd_len].to_vec();
        let (a1, z1) = (z.len() as u64 + 50, z.len() as u64 + 100);
        z.resize(z1 as usize, 0);
        put32(&mut z, EOCD64);
        put64(&mut z, 44);
        z.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        for v in [1, 1, z1 - a1, a1] {
            put64(&mut z, v);
        }
        z.resize(z.len() + 250_000, 0);
        let mut comment = Vec::new();
        put32(&mut comment, EOCD64_LOCATOR);
        put32(&mut comment, 0);
        put64(&mut comment, z1);
        put32(&mut comment, 1);
        put_eocd(&mut comment, 0, 0, SENTINEL, 0);
        let real = z.len();
        put_eocd(&mut z, 2, cd_len as u32, cd as u32, comment.len());
        z.extend_from_slice(&comment);
        rows.push(Row {
            what: "opus XN1: a chosen record that does not list",
            record: Some(real),
            read: Ok(entries(&[("Disk 1.adf", &disk)])),
            reads: Some(2),
            zip: z,
        });

        // opus XN2: a record in the last central record's comment field, listing the directory
        // up to it (a prefix: the skipped first entry only), its comment holding the real
        // record: the end-record bytes cannot tell it from a real one; both paths read it.
        let (zip, _) = make(
            &[stored("scan.bin", &[1; 50]), stored("Disk 1.adf", &disk)],
            0,
            false,
        );
        let cd = central_at(&zip);
        let second = cd + 46 + "scan.bin".len();
        let mut z = zip[..second + 46 + "Disk 1.adf".len()].to_vec();
        z[second + 32..second + 34].copy_from_slice(&22u16.to_le_bytes());
        let planted = z.len();
        put_eocd(&mut z, 1, (planted - cd) as u32, cd as u32, 0);
        let real = z.len();
        put_eocd(&mut z, 2, (real - cd) as u32, cd as u32, 0);
        let reach = (z.len() - planted - 22) as u16;
        z[planted + 20..planted + 22].copy_from_slice(&reach.to_le_bytes());
        rows.push(Row {
            what: "opus XN2: a record listing a prefix of the directory",
            record: Some(planted),
            read: Ok(Vec::new()),
            reads: Some(1),
            zip: z,
        });

        // opus XN4: a directory and end record just under the directory limit, every entry
        // taken: the zip of the entries (its comment past the limit) reads as the original.
        let names: Vec<String> = (0..279).map(|i| format!("{i:0>59996}.adf")).collect();
        let list: Vec<E> = names.iter().map(|n| stored(n, b"d")).collect();
        let (zip, _) = make(&list, 0, false);
        if zip.len() - central_at(&zip) > MAX_DIRECTORY_BYTES as usize {
            setup.push("opus XN4: the directory is not under the limit".to_string());
        }
        let all: Vec<(String, Vec<u8>)> =
            names.iter().map(|n| (n.clone(), b"d".to_vec())).collect();
        rows.push(Row {
            what: "opus XN4: a directory near the limit, every entry taken",
            record: Some(zip.len() - 22),
            read: Ok(all),
            reads: Some(2),
            zip,
        });

        // opus XN5: the end record inside the last central record's name (the rest of the name
        // its comment), the directory up to it within the limit but its records running past it
        // (as the zip of its entries would carry them past the limit): both paths refuse it.
        let names: Vec<String> = (0..280).map(|i| format!("{i:0>59996}.adf")).collect();
        let list: Vec<E> = names.iter().map(|n| stored(n, b"d")).collect();
        let (mut zip, exact) = make(&list, 0, false);
        let cd = central_at(&zip);
        let last_central = zip.len() - 22 - (46 + 60_000);
        let last_local = exact[279].0 as usize;
        let k = 100; // where in the last name the end record starts
        let e = last_central + 46 + k;
        let mut record = Vec::new();
        put_eocd(
            &mut record,
            280,
            (e - cd) as u32,
            cd as u32,
            60_000 - k - 22,
        );
        zip.truncate(zip.len() - 22); // the file ends with the last name
        zip[e..e + 22].copy_from_slice(&record);
        zip[last_local + 30 + k..][..22].copy_from_slice(&record); // the local name agrees
        if e + 22 - cd > MAX_DIRECTORY_BYTES as usize
            || zip.len() - cd <= MAX_DIRECTORY_BYTES as usize
        {
            setup.push("opus XN5: not a directory within the limit up to its record".to_string());
        }
        rows.push(Row {
            what: "opus XN5: the end record inside the last central record's name",
            record: Some(e),
            read: Err(invalid()),
            reads: None,
            zip,
        });

        // The comment-length field: longer than the bytes after the record (the comment does not
        // fit: no record, refused), shorter (trailing bytes after the comment: read).
        let mut zip = with(b"hello", false);
        let at = zip.len() - 27;
        zip[at + 20] = 9;
        rows.push(Row {
            what: "a comment length longer than the bytes that follow",
            zip,
            record: None,
            read: Err(invalid()),
            reads: None,
        });
        let mut zip = with(b"hello", false);
        zip[at + 20] = 2;
        rows.push(plain(
            "a comment length shorter than the bytes that follow",
            zip,
            at,
            1,
        ));

        // Every row is checked to its end, a panic in one recorded as its failure, and all the
        // failures are reported together.
        let mut failed = setup;
        for row in &rows {
            let checked = std::panic::catch_unwind(|| {
                let size = row.zip.len() as u64;
                let view = View {
                    d: &row.zip,
                    base: 0,
                };
                let chosen = end_record(&view, size).ok().map(|c| c.at as usize);
                let (whole, sliced) = paths(&row.zip);
                let reads = read_directory(&row.zip).ok().map(|(_, n)| n);
                [
                    ("the record", chosen == row.record),
                    ("the paths agree", whole == sliced),
                    ("what is read", whole == row.read),
                    ("the reads", reads == row.reads),
                ]
            });
            match checked {
                Ok(checks) => {
                    for (what, ok) in checks {
                        if !ok {
                            failed.push(format!("{}: {what}", row.what));
                        }
                    }
                }
                Err(_) => failed.push(format!("{}: panicked", row.what)),
            }
        }
        for line in &failed {
            eprintln!("row failed: {line}");
        }
        assert!(
            failed.is_empty(),
            "{} row checks failed (listed above)",
            failed.len()
        );
    }

    /// Shapes other writers make, read alike by both paths and as the fixture holds them: data
    /// descriptors (12, 16 and 24 bytes), local Zip64 fields (with and without a descriptor),
    /// two central records naming one local header (both taken, or one the filter skips). And
    /// the one refusal among such shapes: a skipped record whose local offset points into a taken
    /// entry's data (module doc).
    #[test]
    fn shapes_other_writers_make_read_alike_on_both_paths() {
        let scan = vec![1u8; 500];
        let one = vec![5u8; 3000];
        let two = b"deflated disk ".repeat(300);
        let base = || {
            vec![
                stored("scans/a.bin", &scan),
                stored("d/Disk 1.adf", &one),
                deflated("Disk 2.hfe", &two),
            ]
        };
        let expected = || Ok(entries(&[("d/Disk 1.adf", &one), ("Disk 2.hfe", &two)]));
        let each = |f: &dyn Fn(&mut E)| {
            let mut v = base();
            v.iter_mut().for_each(f);
            v
        };
        let zip64 = Opts {
            zip64: true,
            ..Opts::default()
        };
        let plain = Opts::default();
        let cases: Vec<(&str, Vec<E>, Opts)> = vec![
            (
                "a 12-byte descriptor",
                each(&|e| e.descriptor = Some((false, false))),
                plain,
            ),
            (
                "a 16-byte descriptor",
                each(&|e| e.descriptor = Some((true, false))),
                plain,
            ),
            (
                "a 24-byte descriptor, Zip64",
                each(&|e| e.descriptor = Some((true, true))),
                zip64,
            ),
            (
                "a local Zip64 field, Zip64",
                each(&|e| e.local64 = true),
                zip64,
            ),
            (
                "a local Zip64 field and a descriptor, Zip64",
                each(&|e| {
                    e.local64 = true;
                    e.descriptor = Some((true, true));
                }),
                zip64,
            ),
        ];
        for (what, list, opts) in cases {
            let (zip, _) = write(&list, opts);
            assert_eq!(both(&zip, what), expected(), "{what}");
        }
        let mut shared = base();
        shared.push(E {
            share: Some((1, 0)),
            ..stored("d/Disk 1.adf", &one)
        });
        let (zip, _) = write(&shared, plain);
        assert_eq!(
            both(&zip, "one local header, two taken records"),
            expected()
        );
        let mut skipped = base();
        skipped.push(E {
            share: Some((1, 0)),
            ..stored("scans/b.bin", &one)
        });
        let (zip, _) = write(&skipped, plain);
        let what = "one local header, a skipped record";
        assert_eq!(both(&zip, what), expected(), "{what}");
        let mut inside = base();
        inside.push(E {
            share: Some((1, 30 + 12 + 100)),
            ..stored("scans/c.bin", b"x")
        });
        let (zip, _) = write(&inside, plain);
        refused(
            &zip,
            "a skipped record's local offset inside a taken entry's data",
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
        // Rebuilt adds nothing of a refused slice. Bytes between the last entry and the central
        // directory give its range room past its data, so a slice one byte short or long still
        // holds the entry: only the length check refuses it.
        let gapped = gap_before_directory(&zip, 16);
        let (slices, _) = read_directory(&gapped).unwrap();
        let last = &slices[1];
        assert_eq!(last.end, exact[1].1 + 16, "the range runs to the directory");
        let mut r = Rebuilt::for_slices(&slices);
        let mut renamed = piece(&gapped, last).to_vec();
        renamed[30 + 5] = b'3';
        assert_eq!(r.add(last, &renamed), Err(invalid()), "another name");
        let short = &piece(&gapped, last)[..last.len() as usize - 1];
        assert_eq!(
            r.add(last, short),
            Err(invalid()),
            "a slice shorter than its range"
        );
        let long = [piece(&gapped, last), &[0]].concat();
        assert_eq!(
            r.add(last, &long),
            Err(invalid()),
            "a slice longer than its range"
        );
        r.add(&slices[0], piece(&gapped, &slices[0])).unwrap();
        let rebuilt = r.finish().unwrap();
        assert_eq!(
            read_zip(&rebuilt, disks),
            Ok(entries(&[("Disk 1.hfe", b"first")]))
        );
    }

    /// `zip` (no comment) with `n` zero bytes between its last entry and its central directory.
    fn gap_before_directory(zip: &[u8], n: usize) -> Vec<u8> {
        let at = central_at(zip);
        let mut z = zip[..at].to_vec();
        z.resize(at + n, 0);
        z.extend_from_slice(&zip[at..]);
        let e = z.len() - 22;
        z[e + 16..e + 20].copy_from_slice(&((at + n) as u32).to_le_bytes());
        z
    }

    /// An entry whose sizes (central and local alike) run into the next record: the next entry's
    /// local header, the central directory (also by a Zip64 size). Both paths refuse it, though
    /// its local header agrees with its central record.
    #[test]
    fn an_entry_running_into_the_next_record_is_refused_by_both_paths() {
        let grow = |zip: &mut Vec<u8>, local: usize, central: usize| {
            for at in [local + 18, central + 20] {
                let v = u32le(zip, at).unwrap() + 1;
                zip[at..at + 4].copy_from_slice(&v.to_le_bytes());
            }
        };
        let list = [
            stored("Disk 1.hfe", b"first"),
            stored("Disk 2.hfe", b"second"),
        ];
        let (mut zip, exact) = make(&list, 0, false);
        let at = central_at(&zip);
        grow(&mut zip, 0, at);
        refused(&zip, "into the next local header");
        let (mut zip, _) = make(&list, 0, false);
        grow(&mut zip, exact[1].0 as usize, at + 46 + 10);
        refused(&zip, "into the central directory");
        let (mut zip64, _) = make(&list[1..], 0, true);
        let at = central_start(&zip64);
        let field = at + 46 + 10 + 4; // the Zip64 values: full, packed, local
        let packed = u64le(&zip64, field + 8).unwrap() + 1;
        zip64[field + 8..field + 16].copy_from_slice(&packed.to_le_bytes());
        refused(&zip64, "a Zip64 size into the central directory");
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
        // One record over, every record there and valid, without and with Zip64 end records.
        for zip64 in [false, true] {
            let (over, _) = make(&list, 0, zip64);
            refused(&over, "one record over the limit");
            let size = over.len() as u64;
            let from = tail_start(size); // the directory starts before the tail
            assert!(central_start(&over) < from as usize);
            let tail = &over[from as usize..];
            assert_eq!(
                directory(size, from, tail, disks),
                Err(invalid()),
                "not Need"
            );
        }
        // Every record taken: the later copy of a name wins in the first one's place, in time.
        let kept: Vec<(String, String)> = (0..MAX_ENTRIES)
            .map(|i| (format!("{}.adf", i % 40_000), i.to_string()))
            .collect();
        let list: Vec<E> = kept.iter().map(|(n, d)| stored(n, d.as_bytes())).collect();
        let (zip, _) = make(&list, 0, false);
        let read = both(&zip, "every record taken").unwrap();
        assert_eq!(read.len(), 40_000);
        assert_eq!(
            read[0],
            ("0.adf".to_string(), b"40000".to_vec()),
            "the later copy"
        );
        assert_eq!(read[39_999], ("39999.adf".to_string(), b"39999".to_vec()));
    }

    /// The central directory's offset, from the end record or its Zip64 record.
    fn central_start(zip: &[u8]) -> usize {
        let at = central_at(zip);
        if at != SENTINEL as usize {
            return at;
        }
        u64le(zip, zip.len() - 22 - 20 - 56 + 48).unwrap() as usize
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

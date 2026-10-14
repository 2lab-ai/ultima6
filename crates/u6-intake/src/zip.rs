//! Minimal zip reader (stored and deflate entries), mirroring fflate's `unzipSync`
//! as used by `web/src/intake/index.ts`: find the end-of-central-directory record,
//! walk the central directory, ask the filter per entry name *before* touching its
//! data, then locate the data through the local header. CRCs are not checked
//! (fflate does not either). Zip64 sizes and offsets are honoured.

use crate::Error;

const EOCD: u32 = 0x0605_4b50;
const EOCD64: u32 = 0x0606_4b50;
const EOCD64_LOCATOR: u32 = 0x0706_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;
const MAX_COMMENT_SCAN: usize = 65_558;
const SENTINEL32: u32 = u32::MAX;

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

/// Every entry whose name passes `keep`, as (full entry name, bytes), in
/// central-directory order. A repeated name replaces the earlier bytes in place.
pub fn read_zip(data: &[u8], keep: impl Fn(&str) -> bool) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let len = data.len();
    if len < 22 {
        return Err(invalid());
    }
    let mut e = len - 22;
    while u32le(data, e)? != EOCD {
        if e == 0 || len - e > MAX_COMMENT_SCAN {
            return Err(invalid());
        }
        e -= 1;
    }
    let mut count = u16le(data, e + 8)? as u64;
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut at = u64::from(u32le(data, e + 16)?);
    let zip64 = at == u64::from(SENTINEL32) || count == 0xffff;
    if zip64 && e >= 20 && u32le(data, e - 20)? == EOCD64_LOCATOR {
        let z = to_usize(u64le(data, e - 12)?)?;
        if u32le(data, z)? == EOCD64 {
            count = u64le(data, z + 32)?;
            at = u64le(data, z + 48)?;
        }
    }

    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for _ in 0..count {
        let o = to_usize(at)?;
        if u32le(data, o)? != CENTRAL {
            return Err(invalid());
        }
        let flags = u16le(data, o + 8)?;
        let method = u16le(data, o + 10)?;
        let mut csize = u64::from(u32le(data, o + 20)?);
        let mut full = u64::from(u32le(data, o + 24)?);
        let name_len = u16le(data, o + 28)?;
        let extra_len = u16le(data, o + 30)?;
        let comment_len = u16le(data, o + 32)?;
        let mut local = u64::from(u32le(data, o + 42)?);
        let name_at = o + 46;
        let raw_name = data.get(name_at..name_at + name_len).ok_or_else(invalid)?;
        let name = entry_name(raw_name, flags & 0x800 != 0);
        let extra = data
            .get(name_at + name_len..name_at + name_len + extra_len)
            .ok_or_else(invalid)?;
        zip64_fields(extra, &mut full, &mut csize, &mut local)?;
        at = (name_at + name_len + extra_len + comment_len) as u64;

        if !keep(&name) {
            continue;
        }
        let l = to_usize(local)?;
        if u32le(data, l)? != LOCAL {
            return Err(invalid());
        }
        let start = l + 30 + u16le(data, l + 26)? + u16le(data, l + 28)?;
        let end = start.checked_add(to_usize(csize)?).ok_or_else(invalid)?;
        let body = data.get(start..end).ok_or_else(invalid)?;
        let bytes = match method {
            0 => body.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(body, to_usize(full)?)
                .map_err(|err| Error::new(format!("{name}: inflate failed: {err}")))?,
            m => return Err(Error::new(format!("unknown compression type {m}"))),
        };
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = bytes,
            None => out.push((name, bytes)),
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

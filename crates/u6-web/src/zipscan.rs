//! What the disk-image entries of a zip declare uncompressed, read from its central directory
//! without inflating anything ([`crate::MAX_UNZIPPED_BYTES`]). Same record layout and entry
//! filter as `u6_intake::zip::read_zip` and its caller (`.hfe` / `.adf`, no `__MACOSX/`, no
//! `._` files); an archive this cannot read is left to the intake, which reports it.

const EOCD: u32 = 0x0605_4b50;
const EOCD64: u32 = 0x0606_4b50;
const EOCD64_LOCATOR: u32 = 0x0706_4b50;
const CENTRAL: u32 = 0x0201_4b50;
/// End-of-central-directory record plus the longest comment.
const MAX_COMMENT_SCAN: usize = 65_558;
const SENTINEL32: u32 = u32::MAX;

fn u16le(d: &[u8], at: usize) -> Option<usize> {
    let b = d.get(at..at.checked_add(2)?)?;
    Some(usize::from(u16::from_le_bytes([b[0], b[1]])))
}

fn u32le(d: &[u8], at: usize) -> Option<u32> {
    let b = d.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}

fn u64le(d: &[u8], at: usize) -> Option<u64> {
    let b = d.get(at..at.checked_add(8)?)?;
    Some(u64::from_le_bytes(b.try_into().ok()?))
}

fn ends_with_ci(name: &str, suffix: &str) -> bool {
    let (n, s) = (name.as_bytes(), suffix.as_bytes());
    n.len() >= s.len() && n[n.len() - s.len()..].eq_ignore_ascii_case(s)
}

/// The entries the intake inflates.
fn is_disk_entry(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name);
    (ends_with_ci(name, ".hfe") || ends_with_ci(name, ".adf"))
        && !name.starts_with("__MACOSX/")
        && !base.starts_with("._")
}

/// The sum of the declared uncompressed sizes of the disk-image entries of the zip `data`, or
/// `None` when `data` is not a zip this can walk.
pub fn declared_disk_bytes(data: &[u8]) -> Option<u64> {
    if !(data.len() >= 4 && data[..2] == *b"PK" && (data[2] == 3 || data[2] == 5)) {
        return None;
    }
    let len = data.len();
    let mut e = len.checked_sub(22)?;
    while u32le(data, e)? != EOCD {
        if e == 0 || len - e > MAX_COMMENT_SCAN {
            return None;
        }
        e -= 1;
    }
    let mut count = u16le(data, e + 8)? as u64;
    let mut at = u64::from(u32le(data, e + 16)?);
    if (at == u64::from(SENTINEL32) || count == 0xffff)
        && e >= 20
        && u32le(data, e - 20)? == EOCD64_LOCATOR
    {
        let z = usize::try_from(u64le(data, e - 12)?).ok()?;
        if u32le(data, z)? == EOCD64 {
            count = u64le(data, z + 32)?;
            at = u64le(data, z + 48)?;
        }
    }
    let mut total = 0u64;
    for _ in 0..count {
        let o = usize::try_from(at).ok()?;
        if u32le(data, o)? != CENTRAL {
            return None;
        }
        let mut full = u64::from(u32le(data, o + 24)?);
        let name_len = u16le(data, o + 28)?;
        let extra_len = u16le(data, o + 30)?;
        let comment_len = u16le(data, o + 32)?;
        let name_at = o + 46;
        let name = String::from_utf8_lossy(data.get(name_at..name_at + name_len)?);
        let extra = data.get(name_at + name_len..name_at + name_len + extra_len)?;
        if full == u64::from(SENTINEL32) {
            full = zip64_size(extra).unwrap_or(full);
        }
        at = (name_at + name_len + extra_len + comment_len) as u64;
        if is_disk_entry(&name) {
            total = total.saturating_add(full);
        }
    }
    Some(total)
}

/// The 64-bit uncompressed size of a Zip64 extended-information field (tag 1; it comes first).
fn zip64_size(extra: &[u8]) -> Option<u64> {
    let mut i = 0;
    while i + 4 <= extra.len() {
        let (tag, size) = (u16le(extra, i)?, u16le(extra, i + 2)?);
        if tag == 1 {
            return u64le(extra, i + 4);
        }
        i += 4 + size;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stored zip of `entries` (name, declared size, bytes) with an optional Zip64 size.
    fn zip(entries: &[(&str, u32, &[u8])], zip64: Option<u64>) -> Vec<u8> {
        let (mut out, mut central) = (Vec::new(), Vec::new());
        for &(name, size, body) in entries {
            let local = out.len() as u32;
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[0; 14]);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(body);
            let extra: Vec<u8> = match zip64 {
                Some(v) if size == u32::MAX => {
                    let mut x = vec![1, 0, 8, 0];
                    x.extend_from_slice(&v.to_le_bytes());
                    x
                }
                _ => Vec::new(),
            };
            central.extend_from_slice(&CENTRAL.to_le_bytes());
            central.extend_from_slice(&[0; 16]);
            central.extend_from_slice(&(body.len() as u32).to_le_bytes());
            central.extend_from_slice(&size.to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&(extra.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 10]);
            central.extend_from_slice(&local.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
            central.extend_from_slice(&extra);
        }
        let at = out.len() as u32;
        let n = entries.len() as u16;
        out.extend_from_slice(&central);
        out.extend_from_slice(&EOCD.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn sums_what_the_disk_entries_declare() {
        let z = zip(
            &[
                ("d/Disk 1.HFE", 2_000_000, b"x"),
                ("scan.jxl", 900_000_000, b"y"),
                ("__MACOSX/d/Disk 1.hfe", 70_000_000, b"z"),
                ("d/._Disk 2.adf", 70_000_000, b"z"),
                ("Disk 2.adf", 901_120, b"w"),
            ],
            None,
        );
        assert_eq!(declared_disk_bytes(&z), Some(2_901_120));
        let big = zip(&[("Disk.adf", u32::MAX, b"q")], Some(5 << 30));
        assert_eq!(declared_disk_bytes(&big), Some(5 << 30), "Zip64 size");
    }

    #[test]
    fn not_a_zip_is_none() {
        assert_eq!(declared_disk_bytes(b"PP20...."), None);
        assert_eq!(declared_disk_bytes(b"PK\x03\x04 truncated"), None);
        assert_eq!(declared_disk_bytes(&zip(&[], None)), Some(0));
    }
}

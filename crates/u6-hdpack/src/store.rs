//! The zip the writer makes: stored (uncompressed) entries, since the images are compressed
//! already; a fixed date (1980-01-01), so the same images give the same bytes; no Zip64, since
//! a pack is far below its limits (at most [`super::MAX_FILE_BYTES`] in a few thousand
//! entries). [`u6_intake::zip`] reads it.

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
/// Version 2.0: what a stored entry needs.
const VERSION: u16 = 20;
/// MS-DOS date of 1980-01-01 (day 1, month 1, year 0).
const DOS_DATE: u16 = (1 << 5) | 1;

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Both headers' common run: version needed, flags, method, time, date, CRC-32, sizes, name
/// length, extra length.
fn common(out: &mut Vec<u8>, crc: u32, len: u32, name: &str) {
    for v in [VERSION, 0, 0, 0, DOS_DATE] {
        put16(out, v);
    }
    for v in [crc, len, len] {
        put32(out, v);
    }
    put16(out, name.len() as u16);
    put16(out, 0);
}

/// A zip of `files` (name, bytes), in that order.
pub(crate) fn write_zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(data);
        let len = data.len() as u32;
        put32(&mut out, LOCAL);
        common(&mut out, crc, len, name);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        put32(&mut central, CENTRAL);
        put16(&mut central, VERSION);
        common(&mut central, crc, len, name);
        // Comment length, disk, internal and external attributes, then the local offset.
        for v in [0u16, 0, 0] {
            put16(&mut central, v);
        }
        put32(&mut central, 0);
        put32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let at = out.len() as u32;
    let size = central.len() as u32;
    out.extend_from_slice(&central);
    put32(&mut out, END);
    let count = files.len() as u16;
    for v in [0, 0, count, count] {
        put16(&mut out, v);
    }
    put32(&mut out, size);
    put32(&mut out, at);
    put16(&mut out, 0);
    out
}

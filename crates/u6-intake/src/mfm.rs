//! Amiga trackdisk MFM decoder: one revolution of raw cells -> 11 x 512-byte sectors.
//!
//! Sector on disk (after the 0xAAAA 0xAAAA gap): sync 0x4489 0x4489, then raw MFM
//! longs in odd/even split form — info (format/track/sector/sectors-to-gap),
//! 16 label bytes, header checksum, data checksum, 512 data bytes. Each split
//! value v is stored as odd = (v >> 1) & 0x55555555 and even = v & 0x55555555,
//! and checksums are the XOR of the raw longs masked with 0x55555555.

use std::collections::BTreeMap;

use crate::hfe::HfeImage;

pub const SECTOR_SIZE: usize = 512;
pub const SECTORS_PER_TRACK: usize = 11;
pub const ADF_CYLINDERS: usize = 80;
pub const ADF_HEADS: usize = 2;
/// 901,120 bytes.
pub const ADF_SIZE: usize = ADF_CYLINDERS * ADF_HEADS * SECTORS_PER_TRACK * SECTOR_SIZE;

const SYNC: u32 = 0x4489_4489;
const MASK: u32 = 0x5555_5555;
// Cells from the start of the info long to the end of the data area:
// info 2x32, label 8x32, header sum 2x32, data sum 2x32, data 2x4096.
const SECTOR_CELLS: usize = 64 + 256 + 64 + 64 + 8192;

#[derive(Clone, Debug, Default)]
pub struct TrackDecode {
    /// Sector number -> 512 bytes, first copy with good checksums.
    pub sectors: BTreeMap<usize, Vec<u8>>,
    pub syncs: usize,
    pub bad_header: usize,
    pub bad_data: usize,
    pub wrong_track: usize,
}

/// Expand LSB-first cell bytes to one cell per byte, appending `wrap` cells from the start (index wrap).
fn expand_cells(cells: &[u8], wrap: usize) -> Vec<u8> {
    let n = cells.len() * 8;
    let mut out = Vec::with_capacity(n + wrap.min(n));
    for &b in cells {
        out.extend((0..8).map(|i| (b >> i) & 1));
    }
    out.extend_from_within(..wrap.min(n));
    out
}

fn raw_long(bits: &[u8], at: usize) -> u32 {
    bits[at..at + 32]
        .iter()
        .fold(0u32, |v, &b| (v << 1) | u32::from(b))
}

fn join_odd_even(odd: u32, even: u32) -> u32 {
    ((odd & MASK) << 1) | (even & MASK)
}

/// Decode one side of one cylinder. `expected_track` = cyl * 2 + head; sectors
/// whose header names a different track are counted and dropped.
pub fn decode_amiga_track(cells: &[u8], expected_track: usize) -> TrackDecode {
    let mut result = TrackDecode::default();
    let revolution = cells.len() * 8;
    let bits = expand_cells(cells, SECTOR_CELLS + 64);
    let mut shift = 0u32;
    let mut k = 0;
    while k < revolution {
        shift = (shift << 1) | u32::from(bits[k]);
        k += 1;
        if shift != SYNC {
            continue;
        }
        let p = k;
        if p + SECTOR_CELLS > bits.len() {
            break;
        }
        result.syncs += 1;

        let info_odd = raw_long(&bits, p);
        let info_even = raw_long(&bits, p + 32);
        let mut hsum = info_odd ^ info_even;
        for i in 0..8 {
            hsum ^= raw_long(&bits, p + 64 + i * 32);
        }
        hsum &= MASK;
        let hstored = join_odd_even(raw_long(&bits, p + 320), raw_long(&bits, p + 352));
        if hsum != hstored {
            result.bad_header += 1;
            continue;
        }
        let info = join_odd_even(info_odd, info_even);
        let format = info >> 24;
        let track = ((info >> 16) & 0xff) as usize;
        let sector = ((info >> 8) & 0xff) as usize;
        if format != 0xff || track != expected_track || sector >= SECTORS_PER_TRACK {
            result.wrong_track += 1;
            continue;
        }

        let dstored = join_odd_even(raw_long(&bits, p + 384), raw_long(&bits, p + 416));
        let data_at = p + 448;
        let mut data = vec![0u8; SECTOR_SIZE];
        let mut dsum = 0u32;
        for (i, out) in data.chunks_exact_mut(4).enumerate() {
            let odd = raw_long(&bits, data_at + i * 32);
            let even = raw_long(&bits, data_at + 4096 + i * 32);
            dsum ^= odd ^ even;
            out.copy_from_slice(&join_odd_even(odd, even).to_be_bytes());
        }
        if dsum & MASK != dstored {
            result.bad_data += 1;
            continue;
        }
        result.sectors.entry(sector).or_insert(data);
        // Resume scanning after this sector's data area.
        k = p + SECTOR_CELLS;
        shift = 0;
    }
    result
}

#[derive(Clone, Debug)]
pub struct DiskDecode {
    pub adf: Vec<u8>,
    pub found: usize,
    pub total: usize,
    /// Logical block numbers (cyl*22 + head*11 + sector) with no good copy.
    pub missing: Vec<usize>,
    pub bad_header: usize,
    pub bad_data: usize,
    pub wrong_track: usize,
}

/// Decode cylinders 0..79 of an HFE image into an 880 KB ADF; missing sectors stay zero.
pub fn decode_amiga_disk(hfe: &HfeImage) -> DiskDecode {
    let mut out = DiskDecode {
        adf: vec![0u8; ADF_SIZE],
        found: 0,
        total: ADF_CYLINDERS * ADF_HEADS * SECTORS_PER_TRACK,
        missing: Vec::new(),
        bad_header: 0,
        bad_data: 0,
        wrong_track: 0,
    };
    for cyl in 0..ADF_CYLINDERS {
        for head in 0..ADF_HEADS {
            let track = cyl * ADF_HEADS + head;
            let cells = hfe.tracks.get(cyl).and_then(|t| t.get(head));
            let dec = cells.map(|c| decode_amiga_track(c, track));
            if let Some(d) = &dec {
                out.bad_header += d.bad_header;
                out.bad_data += d.bad_data;
                out.wrong_track += d.wrong_track;
            }
            for s in 0..SECTORS_PER_TRACK {
                let lba = track * SECTORS_PER_TRACK + s;
                match dec.as_ref().and_then(|d| d.sectors.get(&s)) {
                    Some(data) => {
                        out.adf[lba * SECTOR_SIZE..(lba + 1) * SECTOR_SIZE].copy_from_slice(data);
                        out.found += 1;
                    }
                    None => out.missing.push(lba),
                }
            }
        }
    }
    out
}

//! HxC Floppy Emulator HFE v1 ("HXCPICFE") container reader.
//!
//! Layout: a 512-byte header, a track look-up table at block `track_list_offset`
//! (4 bytes per cylinder: data offset in 512-byte blocks + byte length covering
//! both sides), then per-cylinder track data. Within each 512-byte block the
//! first 256 bytes belong to side 0 and the next 256 bytes to side 1. Each byte
//! carries 8 raw flux cells, least-significant bit first.

use crate::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HfeHeader {
    pub revision: u8,
    pub cylinders: u8,
    pub sides: u8,
    pub track_encoding: u8,
    pub bit_rate_kbps: u16,
    pub rpm: u16,
    pub interface_mode: u8,
    pub track_list_offset: u16,
}

#[derive(Clone, Debug)]
pub struct HfeImage {
    pub header: HfeHeader,
    /// `tracks[cyl][side]` = raw cell bytes for one revolution (LSB-first bits).
    pub tracks: Vec<Vec<Vec<u8>>>,
}

const MAGIC: &[u8; 8] = b"HXCPICFE";
const BLOCK: usize = 512;
const HALF: usize = 256;

pub fn is_hfe(data: &[u8]) -> bool {
    data.starts_with(MAGIC)
}

fn byte(data: &[u8], at: usize) -> u8 {
    data.get(at).copied().unwrap_or(0)
}

fn u16le(data: &[u8], at: usize) -> u16 {
    u16::from(byte(data, at)) | (u16::from(byte(data, at + 1)) << 8)
}

pub fn parse_hfe(data: &[u8]) -> Result<HfeImage, Error> {
    if !is_hfe(data) {
        return Err(Error::new("not an HFE v1 image (missing HXCPICFE magic)"));
    }
    if data.len() < BLOCK {
        return Err(Error::new("HFE image truncated (header)"));
    }
    let header = HfeHeader {
        revision: byte(data, 8),
        cylinders: byte(data, 9),
        sides: byte(data, 10),
        track_encoding: byte(data, 11),
        bit_rate_kbps: u16le(data, 12),
        rpm: u16le(data, 14),
        interface_mode: byte(data, 16),
        track_list_offset: u16le(data, 18),
    };
    if header.revision != 0 {
        return Err(Error::new(format!(
            "unsupported HFE revision {}",
            header.revision
        )));
    }
    if header.sides < 1 || header.sides > 2 {
        return Err(Error::new(format!("bad HFE side count {}", header.sides)));
    }

    let cylinders = usize::from(header.cylinders);
    let lut = usize::from(header.track_list_offset) * BLOCK;
    if lut + cylinders * 4 > data.len() {
        return Err(Error::new("HFE image truncated (track table)"));
    }

    let mut tracks = Vec::with_capacity(cylinders);
    for cyl in 0..cylinders {
        let offset = usize::from(u16le(data, lut + cyl * 4)) * BLOCK;
        let length = usize::from(u16le(data, lut + cyl * 4 + 2));
        let blocks = length.div_ceil(BLOCK);
        if offset + blocks * BLOCK > data.len() {
            return Err(Error::new(format!(
                "HFE image truncated (cylinder {cyl} data)"
            )));
        }
        let per_side = length >> 1;
        let mut sides = Vec::with_capacity(usize::from(header.sides));
        for side in 0..usize::from(header.sides) {
            let mut out = vec![0u8; per_side];
            for (blk, chunk) in out.chunks_mut(HALF).enumerate() {
                let from = offset + blk * BLOCK + side * HALF;
                // In range by the truncation check above; clamp anyway (untrusted input).
                let src = data.get(from..).unwrap_or(&[]);
                let n = chunk.len().min(src.len());
                chunk[..n].copy_from_slice(&src[..n]);
            }
            sides.push(out);
        }
        tracks.push(sides);
    }
    Ok(HfeImage { header, tracks })
}

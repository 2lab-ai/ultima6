//! Test-only Amiga trackdisk MFM encoder and HFE v1 writer.

const MASK: u32 = 0x5555_5555;

#[derive(Default)]
pub struct CellWriter {
    pub cells: Vec<u8>,
    last_data: u8,
}

impl CellWriter {
    /// Raw cells, MSB first (sync words, gap patterns).
    pub fn raw(&mut self, word: u32, bits: u32) {
        for i in (0..bits).rev() {
            self.cells.push(((word >> i) & 1) as u8);
        }
        self.last_data = self.cells.last().copied().unwrap_or(0);
    }

    /// MFM-encode data bits (MSB first) with correct clock cells.
    pub fn data(&mut self, value: u32, bits: u32) {
        for i in (0..bits).rev() {
            let d = ((value >> i) & 1) as u8;
            self.cells.push(u8::from(self.last_data == 0 && d == 0));
            self.cells.push(d);
            self.last_data = d;
        }
    }

    /// A 32-bit value in odd/even split form: two raw MFM longs.
    pub fn split(&mut self, value: u32) {
        self.data(odd_bits(value), 16);
        self.data(even_bits(value), 16);
    }
}

/// Data bits 31,29,..,1 of v as a 16-bit number (MSB first).
fn odd_bits(v: u32) -> u32 {
    (1..32)
        .step_by(2)
        .rev()
        .fold(0, |r, i| (r << 1) | ((v >> i) & 1))
}

/// Data bits 30,28,..,0 of v as a 16-bit number (MSB first).
fn even_bits(v: u32) -> u32 {
    (0..31)
        .step_by(2)
        .rev()
        .fold(0, |r, i| (r << 1) | ((v >> i) & 1))
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

#[derive(Clone)]
pub struct SectorSpec {
    pub track: u32,
    pub sector: u32,
    pub data: Vec<u8>,
    /// Corrupt the stored data checksum (simulates a bad read).
    pub bad_data: bool,
}

pub fn write_sector(w: &mut CellWriter, s: &SectorSpec, sectors_to_gap: u32) {
    w.data(0, 16); // 0xAAAA 0xAAAA preamble
    w.raw(0x4489, 16);
    w.raw(0x4489, 16);
    let info = (0xff << 24) | (s.track << 16) | (s.sector << 8) | sectors_to_gap;
    let label = [0u32; 4];
    let mut hsum = ((info >> 1) & MASK) ^ (info & MASK);
    for l in label {
        hsum ^= ((l >> 1) & MASK) ^ (l & MASK);
    }
    let mut dsum = 0;
    for i in (0..512).step_by(4) {
        let v = be32(&s.data, i);
        dsum ^= ((v >> 1) & MASK) ^ (v & MASK);
    }
    if s.bad_data {
        dsum ^= 1;
    }
    w.data(odd_bits(info), 16);
    w.data(even_bits(info), 16);
    for l in label {
        w.data(odd_bits(l), 16);
    }
    for l in label {
        w.data(even_bits(l), 16);
    }
    w.split(hsum);
    w.split(dsum);
    for i in (0..512).step_by(4) {
        w.data(odd_bits(be32(&s.data, i)), 16);
    }
    for i in (0..512).step_by(4) {
        w.data(even_bits(be32(&s.data, i)), 16);
    }
}

pub const TRACK_CELLS: usize = 100_464;

/// One revolution: the sectors then 0xAAAA gap padding to `total_cells` (multiple of 8).
pub fn write_track(sectors: &[SectorSpec], total_cells: usize) -> Vec<u8> {
    let mut w = CellWriter::default();
    for (i, s) in sectors.iter().enumerate() {
        write_sector(&mut w, s, (sectors.len() - i) as u32);
    }
    while w.cells.len() < total_cells {
        w.data(0, 1);
    }
    w.cells.truncate(total_cells);
    w.cells
}

/// Pack cells LSB-first into bytes (HFE bit order).
pub fn pack_lsb_first(cells: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; cells.len().div_ceil(8)];
    for (i, &c) in cells.iter().enumerate() {
        if c != 0 {
            out[i >> 3] |= 1 << (i & 7);
        }
    }
    out
}

/// HFE v1 file: header, LUT at block 1, per-cylinder data interleaved per side in 256-byte halves.
pub fn write_hfe(tracks: &[Vec<Vec<u8>>], bit_rate_kbps: u16) -> Vec<u8> {
    let cylinders = tracks.len();
    let sides = tracks.first().map_or(0, Vec::len);
    let lens: Vec<usize> = tracks
        .iter()
        .map(|t| t.iter().map(Vec::len).max().unwrap_or(0))
        .collect();
    let blocks_for = |len: usize| (len * 2).div_ceil(512);
    let total = 1024 + lens.iter().map(|&l| blocks_for(l) * 512).sum::<usize>();
    let mut out = vec![0u8; total];
    out[..8].copy_from_slice(b"HXCPICFE");
    out[9] = cylinders as u8;
    out[10] = sides as u8;
    out[11] = 1;
    out[12..14].copy_from_slice(&bit_rate_kbps.to_le_bytes());
    out[16] = 7;
    out[18] = 1;
    let mut block = 2usize;
    for (cyl, track) in tracks.iter().enumerate() {
        let len = lens[cyl];
        out[512 + cyl * 4..512 + cyl * 4 + 2].copy_from_slice(&(block as u16).to_le_bytes());
        out[512 + cyl * 4 + 2..512 + cyl * 4 + 4]
            .copy_from_slice(&((len * 2) as u16).to_le_bytes());
        for (s, side) in track.iter().enumerate() {
            for (k, chunk) in side[..len.min(side.len())].chunks(256).enumerate() {
                let at = block * 512 + k * 512 + s * 256;
                out[at..at + chunk.len()].copy_from_slice(chunk);
            }
        }
        block += blocks_for(len);
    }
    out
}

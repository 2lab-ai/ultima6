// Test-only Amiga trackdisk MFM encoder and HFE v1 writer, so the decoder can be
// round-tripped without game data.

const MASK = 0x55555555;

export class CellWriter {
  cells: number[] = [];
  private lastData = 0;

  /** Raw cells, MSB first (sync words, gap patterns). */
  raw(word: number, bits: number): void {
    for (let i = bits - 1; i >= 0; i--) this.cells.push((word >>> i) & 1);
    this.lastData = this.cells[this.cells.length - 1] ?? 0;
  }

  /** MFM-encode data bits (MSB first) with correct clock cells. */
  data(value: number, bits: number): void {
    for (let i = bits - 1; i >= 0; i--) {
      const d = (value >>> i) & 1;
      this.cells.push(this.lastData === 0 && d === 0 ? 1 : 0, d);
      this.lastData = d;
    }
  }

  /** A 32-bit value in odd/even split form: two raw MFM longs. */
  split(value: number): void {
    this.data(oddBits(value), 16);
    this.data(evenBits(value), 16);
  }
}

/** Data bits 31,29,..,1 of v as a 16-bit number (MSB first). */
function oddBits(v: number): number {
  let r = 0;
  for (let i = 31; i >= 1; i -= 2) r = (r << 1) | ((v >>> i) & 1);
  return r;
}

/** Data bits 30,28,..,0 of v as a 16-bit number (MSB first). */
function evenBits(v: number): number {
  let r = 0;
  for (let i = 30; i >= 0; i -= 2) r = (r << 1) | ((v >>> i) & 1);
  return r;
}

function be32(b: Uint8Array, at: number): number {
  return (((b[at] ?? 0) << 24) | ((b[at + 1] ?? 0) << 16) | ((b[at + 2] ?? 0) << 8) | (b[at + 3] ?? 0)) >>> 0;
}

export interface SectorSpec {
  track: number;
  sector: number;
  data: Uint8Array;
  /** Corrupt the stored data checksum (simulates a bad read). */
  badData?: boolean;
}

export function writeSector(w: CellWriter, s: SectorSpec, sectorsToGap: number): void {
  w.data(0, 16); // 0xAAAA 0xAAAA preamble
  w.raw(0x4489, 16);
  w.raw(0x4489, 16);
  const info = ((0xff << 24) | (s.track << 16) | (s.sector << 8) | sectorsToGap) >>> 0;
  const label = [0, 0, 0, 0];
  let hsum = ((info >>> 1) & MASK) ^ (info & MASK);
  for (const l of label) hsum ^= ((l >>> 1) & MASK) ^ (l & MASK);
  let dsum = 0;
  for (let i = 0; i < 512; i += 4) {
    const v = be32(s.data, i);
    dsum ^= ((v >>> 1) & MASK) ^ (v & MASK);
  }
  if (s.badData) dsum ^= 1;
  w.data(oddBits(info), 16);
  w.data(evenBits(info), 16);
  for (const l of label) w.data(oddBits(l), 16);
  for (const l of label) w.data(evenBits(l), 16);
  w.split(hsum >>> 0);
  w.split(dsum >>> 0);
  for (let i = 0; i < 512; i += 4) w.data(oddBits(be32(s.data, i)), 16);
  for (let i = 0; i < 512; i += 4) w.data(evenBits(be32(s.data, i)), 16);
}

/** One revolution: 11 sectors then 0xAAAA gap padding to `totalCells` (multiple of 8). */
export function writeTrack(sectors: SectorSpec[], totalCells = 100_464): number[] {
  const w = new CellWriter();
  sectors.forEach((s, i) => writeSector(w, s, sectors.length - i));
  while (w.cells.length < totalCells) w.data(0, 1);
  return w.cells.slice(0, totalCells);
}

/** Pack cells LSB-first into bytes (HFE bit order). */
export function packLsbFirst(cells: number[]): Uint8Array {
  const out = new Uint8Array(Math.ceil(cells.length / 8));
  cells.forEach((c, i) => {
    if (c) out[i >> 3] = (out[i >> 3] ?? 0) | (1 << (i & 7));
  });
  return out;
}

/** HFE v1 file: header, LUT at block 1, per-cylinder data interleaved per side in 256-byte halves. */
export function writeHfe(tracks: Uint8Array[][], bitRateKbps = 250): Uint8Array {
  const cylinders = tracks.length;
  const sides = tracks[0]?.length ?? 0;
  const lens = tracks.map((t) => Math.max(...t.map((s) => s.length)));
  const blocksFor = (len: number): number => Math.ceil((len * 2) / 512);
  const total = 1024 + lens.reduce((a, l) => a + blocksFor(l) * 512, 0);
  const out = new Uint8Array(total);
  out.set(new TextEncoder().encode("HXCPICFE"), 0);
  out[8] = 0;
  out[9] = cylinders;
  out[10] = sides;
  out[11] = 1;
  out[12] = bitRateKbps & 0xff;
  out[13] = bitRateKbps >> 8;
  out[16] = 7;
  out[18] = 1;
  let block = 2;
  tracks.forEach((track, cyl) => {
    const len = lens[cyl] ?? 0;
    out[512 + cyl * 4] = block & 0xff;
    out[512 + cyl * 4 + 1] = block >> 8;
    out[512 + cyl * 4 + 2] = (len * 2) & 0xff;
    out[512 + cyl * 4 + 3] = (len * 2) >> 8;
    track.forEach((side, s) => {
      for (let i = 0; i < len; i += 256) {
        out.set(side.subarray(i, Math.min(i + 256, len)), block * 512 + (i / 256) * 512 + s * 256);
      }
    });
    block += blocksFor(len);
  });
  return out;
}

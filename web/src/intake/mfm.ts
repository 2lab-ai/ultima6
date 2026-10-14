// Amiga trackdisk MFM decoder: one revolution of raw cells → 11 × 512-byte sectors.
//
// Sector on disk (after the 0xAAAA 0xAAAA gap): sync 0x4489 0x4489, then raw MFM
// longs in odd/even split form — info (format/track/sector/sectors-to-gap),
// 16 label bytes, header checksum, data checksum, 512 data bytes. Each split
// value v is stored as odd = (v >> 1) & 0x55555555 and even = v & 0x55555555,
// and checksums are the XOR of the raw longs masked with 0x55555555.

import type { HfeImage } from "./hfe.js";

export const SECTOR_SIZE = 512;
export const SECTORS_PER_TRACK = 11;
export const ADF_CYLINDERS = 80;
export const ADF_HEADS = 2;
export const ADF_SIZE = ADF_CYLINDERS * ADF_HEADS * SECTORS_PER_TRACK * SECTOR_SIZE; // 901,120

const SYNC = 0x44894489;
const MASK = 0x55555555;
// Cells from the start of the info long to the end of the data area:
// info 2×32, label 8×32, header sum 2×32, data sum 2×32, data 2×4096.
const SECTOR_CELLS = 64 + 256 + 64 + 64 + 8192;

export interface TrackDecode {
  /** sector number → 512 bytes, first copy with good checksums. */
  sectors: Map<number, Uint8Array>;
  syncs: number;
  badHeader: number;
  badData: number;
  wrongTrack: number;
}

/** Expand LSB-first cell bytes to one cell per byte, appending `wrap` cells from the start (index wrap). */
function expandCells(cells: Uint8Array, wrap: number): Uint8Array {
  const n = cells.length * 8;
  const out = new Uint8Array(n + Math.min(wrap, n));
  for (let i = 0; i < cells.length; i++) {
    const b = cells[i] ?? 0;
    const o = i * 8;
    out[o] = b & 1;
    out[o + 1] = (b >> 1) & 1;
    out[o + 2] = (b >> 2) & 1;
    out[o + 3] = (b >> 3) & 1;
    out[o + 4] = (b >> 4) & 1;
    out[o + 5] = (b >> 5) & 1;
    out[o + 6] = (b >> 6) & 1;
    out[o + 7] = (b >> 7) & 1;
  }
  out.copyWithin(n, 0, out.length - n);
  return out;
}

function rawLong(bits: Uint8Array, at: number): number {
  let v = 0;
  for (let i = 0; i < 32; i++) v = (v << 1) | (bits[at + i] ?? 0);
  return v >>> 0;
}

function joinOddEven(odd: number, even: number): number {
  return (((odd & MASK) << 1) | (even & MASK)) >>> 0;
}

/**
 * Decode one side of one cylinder. `expectedTrack` = cyl * 2 + head; sectors
 * whose header names a different track are counted and dropped.
 */
export function decodeAmigaTrack(cells: Uint8Array, expectedTrack: number): TrackDecode {
  const result: TrackDecode = {
    sectors: new Map(),
    syncs: 0,
    badHeader: 0,
    badData: 0,
    wrongTrack: 0,
  };
  const revolution = cells.length * 8;
  const bits = expandCells(cells, SECTOR_CELLS + 64);
  let shift = 0;
  for (let k = 0; k < revolution; k++) {
    shift = ((shift << 1) | (bits[k] ?? 0)) >>> 0;
    if (shift !== SYNC) continue;
    const p = k + 1;
    if (p + SECTOR_CELLS > bits.length) break;
    result.syncs++;

    const infoOdd = rawLong(bits, p);
    const infoEven = rawLong(bits, p + 32);
    let hsum = infoOdd ^ infoEven;
    for (let i = 0; i < 8; i++) hsum ^= rawLong(bits, p + 64 + i * 32);
    hsum = (hsum & MASK) >>> 0;
    const hstored = joinOddEven(rawLong(bits, p + 320), rawLong(bits, p + 352));
    if (hsum !== hstored) {
      result.badHeader++;
      continue;
    }
    const info = joinOddEven(infoOdd, infoEven);
    const format = info >>> 24;
    const track = (info >>> 16) & 0xff;
    const sector = (info >>> 8) & 0xff;
    if (format !== 0xff || track !== expectedTrack || sector >= SECTORS_PER_TRACK) {
      result.wrongTrack++;
      continue;
    }

    const dstored = joinOddEven(rawLong(bits, p + 384), rawLong(bits, p + 416));
    const dataAt = p + 448;
    const data = new Uint8Array(SECTOR_SIZE);
    let dsum = 0;
    for (let i = 0; i < SECTOR_SIZE / 4; i++) {
      const odd = rawLong(bits, dataAt + i * 32);
      const even = rawLong(bits, dataAt + 4096 + i * 32);
      dsum ^= odd ^ even;
      const v = joinOddEven(odd, even);
      data[i * 4] = v >>> 24;
      data[i * 4 + 1] = (v >>> 16) & 0xff;
      data[i * 4 + 2] = (v >>> 8) & 0xff;
      data[i * 4 + 3] = v & 0xff;
    }
    if (((dsum & MASK) >>> 0) !== dstored) {
      result.badData++;
      continue;
    }
    if (!result.sectors.has(sector)) result.sectors.set(sector, data);
    // Resume scanning after this sector's data area.
    k = p + SECTOR_CELLS - 1;
    shift = 0;
  }
  return result;
}

export interface DiskDecode {
  adf: Uint8Array;
  found: number;
  total: number;
  /** Logical block numbers (cyl*22 + head*11 + sector) with no good copy. */
  missing: number[];
  badHeader: number;
  badData: number;
  wrongTrack: number;
}

/** Decode cylinders 0..79 of an HFE image into an 880 KB ADF; missing sectors stay zero. */
export function decodeAmigaDisk(hfe: HfeImage): DiskDecode {
  const adf = new Uint8Array(ADF_SIZE);
  const out: DiskDecode = {
    adf,
    found: 0,
    total: ADF_CYLINDERS * ADF_HEADS * SECTORS_PER_TRACK,
    missing: [],
    badHeader: 0,
    badData: 0,
    wrongTrack: 0,
  };
  for (let cyl = 0; cyl < ADF_CYLINDERS; cyl++) {
    for (let head = 0; head < ADF_HEADS; head++) {
      const track = cyl * ADF_HEADS + head;
      const cells = hfe.tracks[cyl]?.[head];
      const dec = cells ? decodeAmigaTrack(cells, track) : null;
      if (dec) {
        out.badHeader += dec.badHeader;
        out.badData += dec.badData;
        out.wrongTrack += dec.wrongTrack;
      }
      for (let s = 0; s < SECTORS_PER_TRACK; s++) {
        const lba = track * SECTORS_PER_TRACK + s;
        const data = dec?.sectors.get(s);
        if (data) {
          adf.set(data, lba * SECTOR_SIZE);
          out.found++;
        } else {
          out.missing.push(lba);
        }
      }
    }
  }
  return out;
}

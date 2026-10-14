// HxC Floppy Emulator HFE v1 ("HXCPICFE") container reader.
//
// Layout: a 512-byte header, a track look-up table at block `trackListOffset`
// (4 bytes per cylinder: data offset in 512-byte blocks + byte length covering
// both sides), then per-cylinder track data. Within each 512-byte block the
// first 256 bytes belong to side 0 and the next 256 bytes to side 1. Each byte
// carries 8 raw flux cells, least-significant bit first.

export interface HfeHeader {
  revision: number;
  cylinders: number;
  sides: number;
  trackEncoding: number;
  bitRateKbps: number;
  rpm: number;
  interfaceMode: number;
  trackListOffset: number;
}

export interface HfeImage {
  header: HfeHeader;
  /** tracks[cyl][side] = raw cell bytes for one revolution (LSB-first bits). */
  tracks: Uint8Array[][];
}

const MAGIC = "HXCPICFE";
const BLOCK = 512;
const HALF = 256;

export function isHfe(data: Uint8Array): boolean {
  if (data.length < 8) return false;
  for (let i = 0; i < 8; i++) {
    if (data[i] !== MAGIC.charCodeAt(i)) return false;
  }
  return true;
}

function u16le(data: Uint8Array, at: number): number {
  return (data[at] ?? 0) | ((data[at + 1] ?? 0) << 8);
}

export function parseHfe(data: Uint8Array): HfeImage {
  if (!isHfe(data)) throw new Error("not an HFE v1 image (missing HXCPICFE magic)");
  if (data.length < BLOCK) throw new Error("HFE image truncated (header)");
  const header: HfeHeader = {
    revision: data[8] ?? 0,
    cylinders: data[9] ?? 0,
    sides: data[10] ?? 0,
    trackEncoding: data[11] ?? 0,
    bitRateKbps: u16le(data, 12),
    rpm: u16le(data, 14),
    interfaceMode: data[16] ?? 0,
    trackListOffset: u16le(data, 18),
  };
  if (header.revision !== 0) throw new Error(`unsupported HFE revision ${header.revision}`);
  if (header.sides < 1 || header.sides > 2) throw new Error(`bad HFE side count ${header.sides}`);

  const lut = header.trackListOffset * BLOCK;
  if (lut + header.cylinders * 4 > data.length) throw new Error("HFE image truncated (track table)");

  const tracks: Uint8Array[][] = [];
  for (let cyl = 0; cyl < header.cylinders; cyl++) {
    const offset = u16le(data, lut + cyl * 4) * BLOCK;
    const length = u16le(data, lut + cyl * 4 + 2);
    const blocks = Math.ceil(length / BLOCK);
    if (offset + blocks * BLOCK > data.length) {
      throw new Error(`HFE image truncated (cylinder ${cyl} data)`);
    }
    const perSide = length >> 1;
    const sides: Uint8Array[] = [];
    for (let side = 0; side < header.sides; side++) {
      const out = new Uint8Array(perSide);
      for (let i = 0, blk = 0; i < perSide; i += HALF, blk++) {
        const n = Math.min(HALF, perSide - i);
        const from = offset + blk * BLOCK + side * HALF;
        out.set(data.subarray(from, from + n), i);
      }
      sides.push(out);
    }
    tracks.push(sides);
  }
  return { header, tracks };
}

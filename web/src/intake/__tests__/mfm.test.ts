import { describe, expect, it } from "vitest";
import { parseHfe } from "../hfe.js";
import { ADF_SIZE, decodeAmigaDisk, decodeAmigaTrack } from "../mfm.js";
import { packLsbFirst, writeHfe, writeTrack, type SectorSpec } from "./mfm-encoder.js";

/** Deterministic per-block payload: every byte depends on (lba, offset). */
function payload(lba: number): Uint8Array {
  const out = new Uint8Array(512);
  let x = (lba * 2654435761) >>> 0 || 1;
  for (let i = 0; i < 512; i++) {
    x ^= x << 13;
    x ^= x >>> 17;
    x ^= x << 5;
    x >>>= 0;
    out[i] = x & 0xff;
  }
  return out;
}

function sectorsFor(track: number): SectorSpec[] {
  return Array.from({ length: 11 }, (_, s) => ({ track, sector: s, data: payload(track * 11 + s) }));
}

function rotate<T>(arr: T[], by: number): T[] {
  return [...arr.slice(by), ...arr.slice(0, by)];
}

describe("decodeAmigaTrack", () => {
  it("recovers all 11 sectors from an encoded revolution", () => {
    const cells = packLsbFirst(writeTrack(sectorsFor(5)));
    const dec = decodeAmigaTrack(cells, 5);
    expect(dec.badHeader + dec.badData + dec.wrongTrack).toBe(0);
    expect([...dec.sectors.keys()].sort((a, b) => a - b)).toEqual([...Array(11).keys()]);
    for (let s = 0; s < 11; s++) expect(dec.sectors.get(s)).toEqual(payload(5 * 11 + s));
  });

  it("recovers a sector that straddles the index", () => {
    // Rotate so the index falls inside sector 3's data area.
    const cells = rotate(writeTrack(sectorsFor(9)), 3 * 8704 + 4000);
    const dec = decodeAmigaTrack(packLsbFirst(cells), 9);
    expect(dec.sectors.size).toBe(11);
    expect(dec.sectors.get(3)).toEqual(payload(9 * 11 + 3));
  });

  it("finds sectors at any bit alignment", () => {
    const cells = rotate(writeTrack(sectorsFor(0)), 5);
    expect(decodeAmigaTrack(packLsbFirst(cells), 0).sectors.size).toBe(11);
  });

  it("drops a sector with a bad data checksum and counts it", () => {
    const specs = sectorsFor(2);
    specs[4] = { ...specs[4]!, badData: true };
    const dec = decodeAmigaTrack(packLsbFirst(writeTrack(specs)), 2);
    expect(dec.badData).toBe(1);
    expect(dec.sectors.has(4)).toBe(false);
    expect(dec.sectors.size).toBe(10);
  });

  it("drops a sector whose header checksum fails", () => {
    const cells = writeTrack(sectorsFor(2));
    // First sector: 64 preamble+sync cells, then info; flip a data cell in the label area.
    const at = 64 + 64 + 1;
    cells[at] = cells[at] ? 0 : 1;
    const dec = decodeAmigaTrack(packLsbFirst(cells), 2);
    expect(dec.badHeader).toBe(1);
    expect(dec.sectors.has(0)).toBe(false);
  });

  it("rejects headers that name another track", () => {
    const dec = decodeAmigaTrack(packLsbFirst(writeTrack(sectorsFor(7))), 8);
    expect(dec.sectors.size).toBe(0);
    expect(dec.wrongTrack).toBe(11);
  });
});

describe("HFE → ADF", () => {
  it("decodes a synthetic 80-cylinder HFE into the expected ADF", () => {
    const tracks: Uint8Array[][] = [];
    for (let cyl = 0; cyl < 83; cyl++) {
      tracks.push([0, 1].map((head) => packLsbFirst(writeTrack(sectorsFor(cyl * 2 + head)))));
    }
    const hfe = parseHfe(writeHfe(tracks));
    expect(hfe.header).toMatchObject({ cylinders: 83, sides: 2, trackEncoding: 1, bitRateKbps: 250 });
    expect(hfe.tracks[0]?.[1]).toEqual(tracks[0]?.[1]);
    const dec = decodeAmigaDisk(hfe);
    expect(dec.found).toBe(1760);
    expect(dec.missing).toEqual([]);
    const want = new Uint8Array(ADF_SIZE);
    for (let lba = 0; lba < 1760; lba++) want.set(payload(lba), lba * 512);
    expect(dec.adf).toEqual(want);
  });

  it("zero-fills and lists missing sectors", () => {
    const tracks: Uint8Array[][] = [];
    for (let cyl = 0; cyl < 80; cyl++) {
      tracks.push(
        [0, 1].map((head) => {
          const t = cyl * 2 + head;
          const specs = sectorsFor(t).filter((s) => !(t === 3 && s.sector === 6));
          return packLsbFirst(writeTrack(specs));
        }),
      );
    }
    const dec = decodeAmigaDisk(parseHfe(writeHfe(tracks)));
    expect(dec.found).toBe(1759);
    expect(dec.missing).toEqual([3 * 11 + 6]);
    expect(dec.adf.subarray((3 * 11 + 6) * 512, (3 * 11 + 7) * 512).every((b) => b === 0)).toBe(true);
  });

  it("rejects non-HFE input and truncated images", () => {
    expect(() => parseHfe(new Uint8Array(1024))).toThrow(/HXCPICFE/);
    const one = writeHfe([[packLsbFirst(writeTrack(sectorsFor(0))), packLsbFirst(writeTrack(sectorsFor(1)))]]);
    expect(() => parseHfe(one.subarray(0, one.length - 600))).toThrow(/truncated/);
  });
});

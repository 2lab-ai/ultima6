import { describe, expect, it } from "vitest";
import { ADF_SIZE } from "../mfm.js";
import { readOfs } from "../ofs.js";
import { amigaHash, ImageBuilder, type Node } from "./ofs-builder.js";

const BS = 512;

function bytes(n: number, seed: number): Uint8Array {
  const out = new Uint8Array(n);
  for (let i = 0; i < n; i++) out[i] = (i * 31 + seed * 7 + (i >> 8)) & 0xff;
  return out;
}

const text = (s: string): Uint8Array => new TextEncoder().encode(s);

function tree(): Node[] {
  return [
    { name: "game", data: bytes(1000, 1) },
    { name: "big", data: bytes(40_000, 2) }, // > 72 data blocks: needs an extension block
    { name: "empty", data: new Uint8Array(0) },
    { name: "disk.info", data: bytes(488, 3) }, // exactly one OFS data block
    { name: "s", children: [{ name: "startup-sequence", data: text("ultima6\n") }] },
    { name: "Trashcan", children: [] },
    // "an" and "ba" (like "game" and "disk.info") share a hash slot → exercises hash_chain.
    { name: "an", data: text("first") },
    { name: "ba", data: text("second") },
  ];
}

describe("readOfs", () => {
  it("chained names really collide", () => {
    expect(amigaHash("an")).toBe(amigaHash("ba"));
    expect(amigaHash("game")).toBe(amigaHash("disk.info"));
  });

  for (const ffs of [false, true]) {
    it(`reads files, subdirectories, extension blocks and hash chains (${ffs ? "FFS" : "OFS"})`, () => {
      const img = new ImageBuilder(ffs).build("DATA1", tree());
      const vol = readOfs(img);
      expect(vol.name).toBe("DATA1");
      expect(vol.dosType).toBe(ffs ? "DOS1" : "DOS0");
      expect(vol.warnings).toEqual([]);
      expect(vol.dirs.sort()).toEqual(["Trashcan", "s"]);
      const want = new Map<string, Uint8Array>([
        ["game", bytes(1000, 1)],
        ["big", bytes(40_000, 2)],
        ["empty", new Uint8Array(0)],
        ["disk.info", bytes(488, 3)],
        ["s/startup-sequence", text("ultima6\n")],
        ["an", text("first")],
        ["ba", text("second")],
      ]);
      expect([...vol.files.keys()].sort()).toEqual([...want.keys()].sort());
      for (const [k, v] of want) expect(vol.files.get(k), k).toEqual(v);
    });
  }

  it("reports a bad data block checksum but still returns the file", () => {
    const b = new ImageBuilder(false);
    const img = b.build("INSTALL", [{ name: "game", data: bytes(100, 9) }]);
    img[883 * BS + 100] = (img[883 * BS + 100] ?? 0) ^ 0xff; // block 882 = header, 883 = data
    const vol = readOfs(img);
    expect(vol.warnings).toEqual(["game: bad data block checksum at 883"]);
    expect(vol.files.get("game")?.length).toBe(100);
  });

  it("rejects images without a DOS boot block or root block", () => {
    expect(() => readOfs(new Uint8Array(ADF_SIZE))).toThrow(/DOS signature/);
    const img = new Uint8Array(ADF_SIZE);
    img.set([0x44, 0x4f, 0x53, 0], 0);
    expect(() => readOfs(img)).toThrow(/root block/);
  });
});

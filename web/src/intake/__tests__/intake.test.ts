/// <reference types="node" />
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { zipSync } from "fflate";
import { describe, expect, it } from "vitest";
import { intake } from "../index.js";
import { packLsbFirst, writeHfe, writeTrack } from "./mfm-encoder.js";
import { ImageBuilder } from "./ofs-builder.js";
import { crunchPP20 } from "./pp20-encoder.js";

const text = (s: string): Uint8Array => new TextEncoder().encode(s);
const GAME_TEXT = text("The Avatar has returned. ".repeat(40));

function volume(name: string): Uint8Array {
  return new ImageBuilder(false).build(name, [
    { name: "game", data: crunchPP20(GAME_TEXT) },
    { name: "plain", data: text("not crunched") },
    { name: "s", children: [{ name: "startup-sequence", data: text("ultima6\n") }] },
  ]);
}

/** Encode an ADF image as an HFE: 83 cylinders like the archive.org dumps. */
function adfToHfe(adf: Uint8Array): Uint8Array {
  const tracks: Uint8Array[][] = [];
  for (let cyl = 0; cyl < 83; cyl++) {
    tracks.push(
      [0, 1].map((head) => {
        const t = cyl * 2 + head;
        const specs = Array.from({ length: 11 }, (_, s) => ({
          track: t,
          sector: s,
          data: cyl < 80 ? adf.subarray((t * 11 + s) * 512, (t * 11 + s + 1) * 512) : new Uint8Array(512),
        }));
        return packLsbFirst(writeTrack(specs));
      }),
    );
  }
  return writeHfe(tracks);
}

describe("intake", () => {
  it("zip of HFE disks → ADFs, volume-prefixed files, PP20 decrunched; scans and flux ignored", async () => {
    const d1 = volume("INSTALL");
    const d2 = volume("DATA1");
    const zip = zipSync(
      {
        "Ultima VI Disk 2.hfe": adfToHfe(d2),
        "Ultima VI Disk 1.hfe": adfToHfe(d1),
        "AA_UVI.jpg": text("\xff\xd8 not really a jpeg"),
        "Pauline/Data Disk 1/track00.0.hxcstream": text("flux"),
      },
      { level: 0 },
    );
    const r = await intake([{ name: "ultima-vi.zip", data: zip }]);
    expect(r.disks.map((d) => [d.label, d.volume])).toEqual([
      ["Ultima VI Disk 1.hfe", "INSTALL"],
      ["Ultima VI Disk 2.hfe", "DATA1"],
    ]);
    expect(r.disks[0]?.adf).toEqual(d1);
    expect(r.disks[1]?.adf).toEqual(d2);
    expect([...r.files.keys()].sort()).toEqual([
      "DATA1/game",
      "DATA1/plain",
      "DATA1/s/startup-sequence",
      "INSTALL/game",
      "INSTALL/plain",
      "INSTALL/s/startup-sequence",
    ]);
    expect(r.files.get("INSTALL/game")).toEqual(GAME_TEXT);
    expect(r.files.get("DATA1/plain")).toEqual(text("not crunched"));
    expect(r.report).toContain(
      "Ultima VI Disk 1.hfe: HFE 83 cyl x 2 sides, 250 kbps, encoding 1; sectors 1760/1760 good, " +
        "bad header checksums 0, bad data checksums 0, wrong-track headers 0",
    );
    expect(r.report).toContain("Ultima VI Disk 2.hfe: volume DATA1 (DOS0), 3 files");
    expect(r.report).toContain("PP20: decrunched 2 file(s); 6 file(s) total");
  });

  it("accepts a dropped ADF directly", async () => {
    const adf = volume("DATA2");
    const r = await intake([{ name: "disk3.adf", data: adf }]);
    expect(r.disks).toHaveLength(1);
    expect(r.disks[0]?.adf).toBe(adf);
    expect(r.files.get("DATA2/game")).toEqual(GAME_TEXT);
  });

  it("passes loose files through, decrunching PP20 and keeping undecodable ones raw", async () => {
    const broken = crunchPP20(GAME_TEXT).subarray(0, 24);
    broken.set([0, 0x10, 0, 3], broken.length - 4); // claims 4 KiB output from a few bits
    const r = await intake([
      { name: "DATA1/game", data: crunchPP20(GAME_TEXT) },
      { name: "DATA1\\map", data: text("map bytes") },
      { name: "./broken", data: broken },
    ]);
    expect(r.disks).toEqual([]);
    expect(r.files.get("DATA1/game")).toEqual(GAME_TEXT);
    expect(r.files.get("DATA1/map")).toEqual(text("map bytes"));
    expect(r.files.get("broken")).toBe(broken);
    expect(r.report.some((l) => l.startsWith("broken: PP20 decrunch failed, kept crunched"))).toBe(true);
    expect(r.report).toContain("PP20: decrunched 1 file(s), 1 failed; 3 file(s) total");
  });

  it("reports an unreadable disk instead of throwing", async () => {
    const hfe = adfToHfe(volume("INSTALL")).subarray(0, 4096);
    const r = await intake([{ name: "bad.hfe", data: hfe }]);
    expect(r.disks).toEqual([]);
    expect(r.report[0]).toMatch(/^bad\.hfe: HFE unreadable: .*truncated/);
  });
});

describe("browser-safe sources", () => {
  it("library code under src/intake uses no Node APIs", () => {
    const dir = new URL("..", import.meta.url).pathname;
    const sources = readdirSync(dir).filter((f) => f.endsWith(".ts"));
    expect(sources.length).toBeGreaterThan(4);
    for (const f of sources) {
      const src = readFileSync(join(dir, f), "utf8");
      expect(src, f).not.toMatch(/from\s+["']node:|require\(|\bprocess\.|\bBuffer\b|reference types="node"/);
    }
  });
});

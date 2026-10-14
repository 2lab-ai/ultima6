/// <reference types="node" />
// Integration against the user's own game data. Runs only where
// ~/Games/ultima6 exists (never in CI: the data is not ours to ship).
// Oracles: greaseweazle ADFs (img/), amitools xdftool unpack (files/), and
// tools/pp20.py output (unpacked/).

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { beforeAll, describe, expect, it } from "vitest";
import { intake, readOfs, type IntakeResult } from "../index.js";

const GAME = "/Users/zhugehyuk/Games/ultima6";
const ZIP = join(GAME, "original", "ultima-vi.zip");
const SIDE_CAR = /\.(blkdev|bootcode|xdfmeta)$/;

function sha256(data: Uint8Array): string {
  return createHash("sha256").update(data).digest("hex");
}

/** Every regular file under `root`, keyed by its path relative to `root`. */
function listTree(root: string): Map<string, string> {
  const out = new Map<string, string>();
  const walk = (dir: string): void => {
    for (const name of readdirSync(dir)) {
      const full = join(dir, name);
      if (statSync(full).isDirectory()) walk(full);
      else out.set(relative(root, full), full);
    }
  };
  walk(root);
  return out;
}

describe.skipIf(!existsSync(ZIP))("intake on the real ultima-vi.zip", () => {
  let result: IntakeResult;
  let ms = 0;

  beforeAll(async () => {
    const data = new Uint8Array(readFileSync(ZIP));
    const t0 = performance.now();
    result = await intake([{ name: "ultima-vi.zip", data }]);
    ms = performance.now() - t0;
    console.log(`intake(ultima-vi.zip, ${data.length} bytes): ${ms.toFixed(0)} ms`);
    for (const line of result.report) console.log(`  ${line}`);
  });

  it("decodes three disks, every sector good", () => {
    expect(result.disks.map((d) => d.label)).toEqual([
      "Ultima VI Disk 1.hfe",
      "Ultima VI Disk 2.hfe",
      "Ultima VI Disk 3.hfe",
    ]);
    expect(result.disks.map((d) => d.volume)).toEqual(["INSTALL", "DATA1", "DATA2"]);
    const sectorLines = result.report.filter((l) => l.includes("sectors "));
    expect(sectorLines).toHaveLength(3);
    for (const l of sectorLines) expect(l).toContain("sectors 1760/1760 good");
  });

  it("each ADF equals the greaseweazle image byte for byte", () => {
    for (const disk of result.disks) {
      const n = /Disk (\d)\.hfe$/.exec(disk.label)?.[1];
      const want = readFileSync(join(GAME, "img", `disk${n}.adf`));
      expect(sha256(disk.adf), disk.label).toBe(sha256(want));
    }
  });

  it("OFS file set and bytes equal xdftool unpack", () => {
    for (const [i, disk] of result.disks.entries()) {
      const vol = readOfs(disk.adf);
      expect(vol.warnings, disk.label).toEqual([]);
      const root = join(GAME, "files", `d${i + 1}`, vol.name);
      const want = listTree(root);
      expect([...vol.files.keys()].sort(), disk.label).toEqual([...want.keys()].sort());
      for (const [path, full] of want) {
        const got = vol.files.get(path);
        expect(got && sha256(got), `${vol.name}/${path}`).toBe(sha256(readFileSync(full)));
      }
    }
  });

  it("intake files equal the decrunched tree (PP20 via tools/pp20.py)", () => {
    const want = new Map<string, string>();
    for (const d of ["d1", "d2", "d3"]) {
      const root = join(GAME, "unpacked", d);
      for (const [path, full] of listTree(root)) {
        if (!SIDE_CAR.test(path)) want.set(path, full);
      }
    }
    expect([...result.files.keys()].sort()).toEqual([...want.keys()].sort());
    let packed = 0;
    for (const [path, full] of want) {
      const got = result.files.get(path);
      expect(got && sha256(got), path).toBe(sha256(readFileSync(full)));
    }
    for (const line of result.report) {
      const m = /^PP20: decrunched (\d+) file/.exec(line);
      if (m) packed = Number(m[1]);
    }
    expect(packed).toBeGreaterThan(0);
    expect(result.report.some((l) => l.includes("decrunch failed"))).toBe(false);
  });

  it("decodes the zip in well under the test budget", () => {
    expect(ms).toBeLessThan(60_000);
  });
});

/// <reference types="node" />
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { decrunchPP20, isPP20 } from "../pp20.js";
import { crunchPP20 } from "./pp20-encoder.js";

// Python reference implementation in this repo (read-only use).
const REFERENCE = new URL("../../../../tools/pp20.py", import.meta.url).pathname;

function lcg(n: number, seed: number): Uint8Array {
  const out = new Uint8Array(n);
  let x = seed >>> 0;
  for (let i = 0; i < n; i++) {
    x = (Math.imul(x, 1103515245) + 12345) >>> 0;
    out[i] = x >>> 24;
  }
  return out;
}

const VECTORS: Record<string, Uint8Array> = {
  "one byte": new Uint8Array([0x42]),
  "two equal bytes": new Uint8Array([7, 7]),
  "text with repeats": new TextEncoder().encode(
    "Ultima VI: The False Prophet. The Avatar returns to Britannia. ".repeat(12),
  ),
  "long run (length extension chunks)": new Uint8Array(2000).fill(0xaa),
  "random (literal runs only)": lcg(700, 1),
  "mixed far/near matches": (() => {
    const base = lcg(1500, 7);
    const out = new Uint8Array(4000);
    for (let i = 0; i < out.length; i++) out[i] = i % 1700 < 1500 ? (base[i % 1500] ?? 0) : i & 0xff;
    return out;
  })(),
};

describe("decrunchPP20", () => {
  for (const [name, plain] of Object.entries(VECTORS)) {
    it(`round-trips ${name}`, () => {
      const packed = crunchPP20(plain);
      expect(isPP20(packed)).toBe(true);
      expect(decrunchPP20(packed)).toEqual(plain);
    });
  }

  it("compresses repetitive data (the encoder really emits matches)", () => {
    const plain = VECTORS["text with repeats"]!;
    expect(crunchPP20(plain).length).toBeLessThan(plain.length / 4);
  });

  it("rejects non-PP20 input and truncated streams", () => {
    expect(() => decrunchPP20(new TextEncoder().encode("FORM0000ILBM"))).toThrow(/not a PP20/);
    const packed = crunchPP20(VECTORS["text with repeats"]!);
    const cut = new Uint8Array([...packed.subarray(0, 8), ...packed.subarray(packed.length - 20)]);
    expect(() => decrunchPP20(cut)).toThrow(/PP20/);
  });

  it.skipIf(!existsSync(REFERENCE))("agrees with tools/pp20.py on every vector", () => {
    const dir = mkdtempSync(join(tmpdir(), "pp20-"));
    try {
      for (const [i, plain] of Object.values(VECTORS).entries()) {
        const src = join(dir, `v${i}.pp`);
        const dst = join(dir, `v${i}.out`);
        writeFileSync(src, crunchPP20(plain));
        execFileSync("python3", ["-I", REFERENCE, src, dst], { stdio: "pipe" });
        expect(new Uint8Array(readFileSync(dst))).toEqual(decrunchPP20(new Uint8Array(readFileSync(src))));
      }
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

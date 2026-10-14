// Test-only PowerPacker PP20 cruncher (greedy LZ, not size-optimal). It emits the
// exact token grammar the decruncher reads, so round trips and the Python
// reference can be checked without game data.

export const OFFSET_LENS = [9, 10, 11, 11] as const;
const WINDOW = 1 << 11;

class BitStream {
  bits: number[] = [];
  put(value: number, n: number): void {
    for (let i = n - 1; i >= 0; i--) this.bits.push(Math.floor(value / 2 ** i) % 2);
  }
}

/** Longest usable match for R[j..] with distance d = offset + 1, honouring per-length offset widths. */
function bestMatch(r: Uint8Array, j: number): { len: number; dist: number } {
  let best = { len: 0, dist: 0 };
  const maxDist = Math.min(j, WINDOW);
  for (let d = 1; d <= maxDist; d++) {
    let len = 0;
    while (j + len < r.length && len < 300 && r[j + len] === r[j + len - d]) len++;
    // Offset field width depends on length class: 2→9 bits, 3→10, 4+→11 (or 7 for short offsets).
    while (len >= 2) {
      const limit = len === 2 ? 1 << OFFSET_LENS[0] : len === 3 ? 1 << OFFSET_LENS[1] : 1 << OFFSET_LENS[2];
      if (d - 1 < limit) break;
      len = len - 1;
    }
    if (len >= 2 && len > best.len) best = { len, dist: d };
  }
  return best;
}

function putLiterals(bs: BitStream, lits: number[]): void {
  let rem = lits.length - 1;
  while (rem >= 3) {
    bs.put(3, 2);
    rem -= 3;
  }
  bs.put(rem, 2);
  for (const b of lits) bs.put(b, 8);
}

function putMatch(bs: BitStream, len: number, dist: number): void {
  const offset = dist - 1;
  if (len <= 4) {
    const x = len - 2;
    bs.put(x, 2);
    bs.put(offset, OFFSET_LENS[x] ?? 0);
    return;
  }
  bs.put(3, 2);
  if (offset < 128) {
    bs.put(0, 1);
    bs.put(offset, 7);
  } else {
    bs.put(1, 1);
    bs.put(offset, OFFSET_LENS[3]);
  }
  let rem = len - 5;
  while (rem >= 7) {
    bs.put(7, 3);
    rem -= 7;
  }
  bs.put(rem, 3);
}

export function crunchPP20(plain: Uint8Array): Uint8Array {
  // The decruncher fills its output from the end, so compress the reversed bytes.
  const r = plain.slice().reverse();
  const bs = new BitStream();
  let lits: number[] = [];
  let j = 0;
  while (j < r.length) {
    const m = bestMatch(r, j);
    if (m.len < 2) {
      lits.push(r[j] ?? 0);
      j++;
      continue;
    }
    bs.put(lits.length > 0 ? 0 : 1, 1);
    if (lits.length > 0) putLiterals(bs, lits);
    lits = [];
    putMatch(bs, m.len, m.dist);
    j += m.len;
  }
  if (lits.length > 0) {
    bs.put(0, 1);
    putLiterals(bs, lits);
  }

  // Pad at the front of the read order; the trailer's skip count discards it.
  const skip = (8 - (bs.bits.length % 8)) % 8;
  const stream = [...Array<number>(skip).fill(0), ...bs.bits];
  const srcLen = stream.length / 8;
  const src = new Uint8Array(srcLen);
  stream.forEach((bit, k) => {
    if (bit) src[srcLen - 1 - (k >> 3)] = (src[srcLen - 1 - (k >> 3)] ?? 0) | (1 << (k & 7));
  });
  const n = plain.length;
  const out = new Uint8Array(8 + srcLen + 4);
  out.set([0x50, 0x50, 0x32, 0x30, ...OFFSET_LENS], 0);
  out.set(src, 8);
  out.set([(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff, skip], 8 + srcLen);
  return out;
}

// PowerPacker "PP20" decruncher — port of tools/pp20.py (algorithm per Stuart
// Caie's ppdepack).
//
// File: "PP20", 4 offset-length bytes, crunched stream, then 4 trailer bytes:
// 24-bit big-endian output length and the number of bits to skip first.
// The stream is consumed from its last byte backwards, each byte LSB first,
// and the output is written from its end backwards.
//
// The Python reference keeps an unbounded integer bit buffer; here a bit cursor
// reads the same stream order without 32-bit overflow.

export function isPP20(data: Uint8Array): boolean {
  return data.length >= 4 && data[0] === 0x50 && data[1] === 0x50 && data[2] === 0x32 && data[3] === 0x30;
}

export function decrunchPP20(data: Uint8Array): Uint8Array {
  if (!isPP20(data)) throw new Error("not a PP20 file");
  if (data.length < 12) throw new Error("PP20 file truncated");
  const offsetLens = data.subarray(4, 8);
  const srcLen = data.length - 12;
  const src = data.subarray(8, 8 + srcLen);
  const n = data.length;
  const destLen = ((data[n - 4] ?? 0) << 16) | ((data[n - 3] ?? 0) << 8) | (data[n - 2] ?? 0);
  const skipBits = data[n - 1] ?? 0;
  const out = new Uint8Array(destLen);
  const totalBits = srcLen * 8;
  let cursor = 0;
  let pos = destLen;

  const read = (count: number): number => {
    if (cursor + count > totalBits) throw new Error("PP20: out of source bits");
    let v = 0;
    for (let i = 0; i < count; i++, cursor++) {
      v = v * 2 + (((src[srcLen - 1 - (cursor >> 3)] ?? 0) >> (cursor & 7)) & 1);
    }
    return v;
  };

  read(skipBits);
  while (pos > 0) {
    if (read(1) === 0) {
      let todo = 1;
      for (;;) {
        const x = read(2);
        todo += x;
        if (x !== 3) break;
      }
      if (todo > pos) throw new Error("PP20: literal run overflow");
      for (let i = 0; i < todo; i++) out[--pos] = read(8);
      if (pos === 0) break;
    }
    let x = read(2);
    let offBits = offsetLens[x] ?? 0;
    let todo = x + 2;
    let offset: number;
    if (x === 3) {
      if (read(1) === 0) offBits = 7;
      offset = read(offBits);
      do {
        x = read(3);
        todo += x;
      } while (x === 7);
    } else {
      offset = read(offBits);
    }
    if (pos + offset >= destLen) throw new Error("PP20: match overflow");
    if (todo > pos) throw new Error("PP20: match run overflow");
    for (let i = 0; i < todo; i++) {
      pos--;
      out[pos] = out[pos + 1 + offset] ?? 0;
    }
  }
  return out;
}

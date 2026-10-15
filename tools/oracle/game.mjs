// The original `game` in the oracle's memory: its hunk table, where each hunk was loaded (the
// seglist), and saving / restoring a settled emulator state. Used by fight.mjs and timeit.mjs.
import { readFileSync, writeFileSync } from 'node:fs';
import * as o from './lib.mjs';

const GAME_BIN = process.env.U6_GAME_BIN || '/Users/zhugehyuk/Games/ultima6/re/game.bin';

// Hunk table of `game` (re/notes/game-layout.md): image base and size of each of the 35 hunks.
// The game's A4 = hunk 33 + 0x8000. Globals read: A4-0x7a2c (the command loop reads a command
// key), A4-0x7b78/-0x7b76 (view centre), A4+0x460c (hit points); objects' locations = hunk 23.
export const HUNKS = [
  [0x100000, 0x1eb0], [0x101eb0, 0x3100], [0x104fb0, 0x3a28], [0x1089d8, 0x10dc], [0x109ab8, 0x964],
  [0x10a420, 0x4c60], [0x10f080, 0x171c], [0x1107a0, 0x240], [0x1109e0, 0x1bd4], [0x1125b8, 0x318],
  [0x1128d0, 0x2958], [0x115228, 0x5f94], [0x11b1c0, 0x6944], [0x121b08, 0x5770], [0x127278, 0x113c],
  [0x1283b8, 0x4500], [0x12c8b8, 0x4880], [0x131138, 0x1200], [0x132338, 0x2238], [0x134570, 0xf8],
  [0x134668, 0x38], [0x1346a0, 0x868], [0x134f08, 0x680], [0x135588, 0x114b8], [0x146a40, 0x7530],
  [0x14df70, 0xf00], [0x14ee70, 0x454], [0x14f2c8, 0x2000], [0x1512c8, 0x83c], [0x151b08, 0x3d4],
  [0x151ee0, 0x16f0], [0x1535d0, 0x4], [0x1535d8, 0x118], [0x1536f0, 0xe7d8], [0x161ec8, 0x668],
];
export const CODE = HUNKS.map((_, i) => i).filter((i) => ![6, 20, 23, 24, 25, 27, 28, 31, 33].includes(i));
const IMG = readFileSync(GAME_BIN);

// The first 48-byte window of hunk `h` with no long that looks relocated (in the image range).
function signature(h) {
  const [base, size] = HUNKS[h];
  const b = IMG.subarray(base - 0x100000, base - 0x100000 + size);
  const rel = (i) => { const v = b.readUInt32BE(i); return v >= 0x100000 && v < 0x163000; };
  for (let off = 0; off + 52 < b.length; off += 2) {
    const w = b.subarray(off, off + 48);
    if (new Set(w).size < 20) continue;
    let ok = true;
    for (let i = Math.max(0, off - 2); i < off + 48 && ok; i += 2) if (rel(i)) ok = false;
    if (ok) return { off, bytes: [...w] };
  }
  throw new Error(`no reloc-free window in hunk ${h}`);
}

const findAll = (page, bytes) => page.evaluate((b) => {
  const out = [];
  const first = (b[0] << 8) | b[1];
  for (let a = 0; a < 0x400000; a += 2) {
    if (Module._wasm_peek16(a) !== first) continue;
    let ok = true;
    for (let i = 2; i < b.length && ok; i += 2) if (Module._wasm_peek16(a + i) !== ((b[i] << 8) | b[i + 1])) ok = false;
    if (ok) out.push(a);
  }
  return out;
}, bytes);
const peekBytes = (page, a, n) => page.evaluate(([a, n]) => Array.from({ length: n }, (_, i) => Module._wasm_peek(a + i)), [a, n]);

// Runtime base of every hunk: hunk 0 by its signature, then the seglist (the long before a
// hunk's data is the BPTR of the next segment's link; its data starts 4 bytes after the link).
// The copy is the live one when three later code hunks hold their own signatures.
export async function hunkBases(page, log = () => {}) {
  const s0 = signature(0);
  for (const hit of await findAll(page, s0.bytes)) {
    const b = [hit - s0.off];
    for (let i = 1; i < HUNKS.length; i++) b.push((await o.peekL(page, b[i - 1] - 4)) * 4 + 4);
    let ok = true;
    for (const h of [13, 18, 30]) {
      const s = signature(h);
      if ((await peekBytes(page, b[h] + s.off, 16)).join() !== s.bytes.slice(0, 16).join()) ok = false;
    }
    log(`hunk 0 copy at ${hit.toString(16)}: ${ok ? 'live' : 'not the seglist'}`);
    if (ok) return b;
  }
  throw new Error('no live copy of the game');
}

export const loadSnapshot = (page, file) => page.evaluate((b64) => {
  const bin = atob(b64), u8 = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  wasm_halt();
  const r = wasm_loadfile('fight.vAmiga', u8);
  wasm_run();
  return r;
}, readFileSync(file).toString('base64'));
export async function saveSnapshot(page, file) {
  const b64 = await page.evaluate(() => {
    const s = JSON.parse(wasm_take_user_snapshot());
    const v = new Uint8Array(Module.HEAPU8.buffer, s.address, s.size).slice();
    wasm_delete_user_snapshot();
    let t = '';
    for (let i = 0; i < v.length; i += 0x8000) t += String.fromCharCode.apply(null, v.subarray(i, i + 0x8000));
    return btoa(t);
  });
  writeFileSync(file, Buffer.from(b64, 'base64'));
}


// The runtime address of image address `img` (bases from hunkBases).
export function runtime(bases, img) {
  let i = HUNKS.length - 1;
  while (HUNKS[i][0] > img) i--;
  return bases[i] + img - HUNKS[i][0];
}

// The castle without the fight: every actor in the assault worktype (8: the start's gargoyles;
// worktypes at 0x139188 = hunk 23 + 0x3c00) gets status bit 0x10 (A4+0x3d0c), which the world
// turn 0x1213de passes over (as pace.mjs's poke), then `b` breaks off combat; the castle's
// people go on with their own worktypes and schedules.
export async function townSetup(page, bases, a4) {
  const peekN = (a, n) => page.evaluate(([a, n]) => Array.from({ length: n }, (_, i) => Module._wasm_peek(a + i)), [a, n]);
  const worktype = await peekN(bases[23] + 0x3c00, 256);
  const status = await peekN(a4 + 0x3d0c, 256);
  const assault = worktype.flatMap((w, i) => (w === 8 ? [i] : []));
  await o.poke(page, a4 + 0x3d0c, status.map((v, i) => (assault.includes(i) ? v | 0x10 : v)));
  // `b` held until the command loop takes it (A4-0x7a2c cleared): a short tap can fall in one of
  // the idle loop's redraws and be missed.
  await page.evaluate(() => wasm_key(0x35, 1));
  let taken = false;
  for (let i = 0; i < 40 && !taken; i++) {
    await o.waitFrames(page, 2);
    taken = (await o.peekW(page, a4 - 0x7a2c)) === 0;
  }
  await page.evaluate(() => wasm_key(0x35, 0));
  await o.waitFrames(page, 400);
  const party = await peekN(a4 + 0x47ec, (await o.peekW(page, a4 + 0x4b00)) >> 8);
  const after = await peekN(bases[23] + 0x3c00, 256);
  const wts = party.map((a) => after[a].toString(16)).join(',');
  if (!taken || party.slice(1).some((a) => after[a] >= 2 && after[a] < 0x10)) {
    throw new Error(`town: combat not broken off (b taken ${taken}; party worktypes ${wts})`);
  }
  return `assault actors passed over: ${assault.map((a) => a.toString(16)).join(',')}; b taken; party worktypes ${wts}`;
}

// CPU cycles of a PAL frame (313 lines × 227 colour clocks × 2): the emulator's CPU clock read as
// PAL frames is monotonic, also across the point where its frame counter steps (which is not
// line 0, so frame + line / 313 can run backwards there).
export const CYCLES_PER_FRAME = 142102;
export const cpuFrames = (page) => page.evaluate(() => Number(Module._wasm_get_cpu_cycles())).then((c) => c / CYCLES_PER_FRAME);

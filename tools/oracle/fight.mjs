// Fight pace of the original: boot the game-first floppies (as pace.mjs), take party steps in
// the start fight and record every emulated frame: the command loop's reading flag, the view
// centre, hashes of the map view, the message scroll and the party panel, the actors that moved
// or lost hit points, and a CPU sample (where the game is and who called it), so each frame of a
// turn can be put down to what the game was doing (fight.py).
// usage: node fight.mjs OUTDIR [KEY ...]   keys = lib.mjs KEY names, one party step each
//                                          (default UP RIGHT LEFT RIGHT LEFT RIGHT LEFT)
//   env U6_FIGHT_SNAPSHOT=file  start from a snapshot fight.mjs saved instead of booting
//       U6_FIGHT_SAVE=file      save the settled start there (a full boot takes ~3 min)
//       U6_GAME_BIN = flattened game image (hunk table and signatures)
// The CPU sample: vAmiga's debugger console runs a command between two frames, where the
// VBlank interrupt has just been taken (PC = the AROS level-3 handler, registers still the
// interrupted task's); the interrupted PC is in its exception frame on the supervisor stack
// (`m.w $3fffe0`), and the user stack, copied at the end of the frame before (it does not change
// in between), gives the return addresses of the callers. Output (keep it outside the repo):
// OUTDIR/turns.json ({bases, a4, turns: [{key, pressedAt, rows}]}) and fight.log.
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import * as o from './lib.mjs';

const [out = join(o.SHOTS_DIR, `fight-${Date.now()}`), ...keyArgs] = process.argv.slice(2);
const KEYS = keyArgs.length ? keyArgs : ['UP', 'RIGHT', 'LEFT', 'RIGHT', 'LEFT', 'RIGHT', 'LEFT'];
const ADF = process.env.U6_ADF_DIR || join(o.ORACLE_HOME, 'adf-gamefirst');
const GAME_BIN = process.env.U6_GAME_BIN || '/Users/zhugehyuk/Games/ultima6/re/game.bin';
mkdirSync(out, { recursive: true });
const LOG = join(out, 'fight.log');
const log = (s) => { appendFileSync(LOG, `${new Date().toISOString()} ${s}\n`); console.log(s); };

// Hunk table of `game` (re/notes/game-layout.md): image base and size of each of the 35 hunks.
// The game's A4 = hunk 33 + 0x8000. Globals read: A4-0x7a2c (the command loop reads a command
// key), A4-0x7b78/-0x7b76 (view centre), A4+0x460c (hit points); objects' locations = hunk 23.
const HUNKS = [
  [0x100000, 0x1eb0], [0x101eb0, 0x3100], [0x104fb0, 0x3a28], [0x1089d8, 0x10dc], [0x109ab8, 0x964],
  [0x10a420, 0x4c60], [0x10f080, 0x171c], [0x1107a0, 0x240], [0x1109e0, 0x1bd4], [0x1125b8, 0x318],
  [0x1128d0, 0x2958], [0x115228, 0x5f94], [0x11b1c0, 0x6944], [0x121b08, 0x5770], [0x127278, 0x113c],
  [0x1283b8, 0x4500], [0x12c8b8, 0x4880], [0x131138, 0x1200], [0x132338, 0x2238], [0x134570, 0xf8],
  [0x134668, 0x38], [0x1346a0, 0x868], [0x134f08, 0x680], [0x135588, 0x114b8], [0x146a40, 0x7530],
  [0x14df70, 0xf00], [0x14ee70, 0x454], [0x14f2c8, 0x2000], [0x1512c8, 0x83c], [0x151b08, 0x3d4],
  [0x151ee0, 0x16f0], [0x1535d0, 0x4], [0x1535d8, 0x118], [0x1536f0, 0xe7d8], [0x161ec8, 0x668],
];
const CODE = HUNKS.map((_, i) => i).filter((i) => ![6, 20, 23, 24, 25, 27, 28, 31, 33].includes(i));
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
async function hunkBases(page) {
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

const loadSnapshot = (page, file) => page.evaluate((b64) => {
  const bin = atob(b64), u8 = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  wasm_halt();
  const r = wasm_loadfile('fight.vAmiga', u8);
  wasm_run();
  return r;
}, readFileSync(file).toString('base64'));
async function saveSnapshot(page, file) {
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

// In-page recorder, after every emulated frame: the key of the step goes down at a frame
// boundary and up once the game has taken it (reading cleared) or after 50 frames; then one row
// [frame, reading, frames skipped, view hash, message hash, panel hash, actors moved,
// [actor, hp before, hp after], sample, centre]. sample = [kind, pc, usp, a5, code words on the
// user stack from usp up: [offset, address]]; kind U = the CPU was in user mode, SU = the VBlank
// interrupt had just taken it from user mode (pc from the exception frame), SS / S? otherwise.
async function installRecorder(page, a4, bases) {
  await page.evaluate(([a4, bases, code]) => {
    const P = (a) => (Module._wasm_peek16(a) << 16) >> 16;
    const W = (a) => Module._wasm_peek16(a);
    const R = window.__fight = { rows: [], on: false, key: null, down: false, pressedAt: null };
    const f = () => Module._wasm_frame_info() >>> 2;
    let last = f();
    // FNV-1a of lores rectangle (x0, y0)..(x1, y1) of the texture (picture origin 93, 44).
    const hash = (x0, y0, x1, y1) => {
      const v = new Uint32Array(Module.HEAPU8.buffer, Module._wasm_pixel_buffer(), 1824 * 313);
      let h = 0x811c9dc5;
      for (let y = 44 + y0; y < 44 + y1; y++) for (let x = 93 + x0; x < 93 + x1; x++) { h ^= v[y * 1824 + x * 4]; h = Math.imul(h, 0x01000193); }
      return h >>> 0;
    };
    const inCode = (a) => code.some((i) => a >= bases[i] && a < bases[i] + 0x7000);
    let prevLoc = null, prevHp = null, stack = null, stackLo = 0, lastUsp = 0;
    const hook = () => {
      const now = f();
      if (now === last) return;
      const gap = now - last - 1;
      last = now;
      const reading = P(a4 - 0x7a2c);
      if (R.key !== null && !R.down) { wasm_key(R.key, 1); R.down = true; R.pressedAt = now; }
      else if (R.down && (reading === 0 || now - R.pressedAt > 50)) { wasm_key(R.key, 0); R.down = false; R.key = null; }
      if (!R.on) return;
      // The console's answer to the commands sent at the end of the previous frame.
      const txt = wasm_retro_shell_get_text();
      let sample = null;
      const m = /PC : 0x([0-9a-f]+)[\s\S]*?USP : 0x([0-9a-f]+)[\s\S]*?A4 - A7 : 0x([0-9a-f]+) 0x([0-9a-f]+)[\s\S]*?Flags : (\S+)[\s\S]*?3fffe0:\s+(\w+) (\w+) (\w+) (\w+)/.exec(txt);
      if (m && stack) {
        let pc = parseInt(m[1], 16), kind = 'U';
        const usp = parseInt(m[2], 16), a5 = parseInt(m[4], 16);
        if (m[5].includes('S')) {
          kind = parseInt(m[1], 16) !== 0xfe8f1a ? 'S?' : (parseInt(m[7], 16) & 0x2000 ? 'SS' : 'SU');
          pc = ((parseInt(m[8], 16) << 16) | parseInt(m[9], 16)) >>> 0;
        }
        const words = [];
        for (let a = usp; a < stackLo + stack.length * 2 - 4 && words.length < 24; a += 2) {
          const i = (a - stackLo) >> 1;
          if (i < 0) continue;
          const v = ((stack[i] << 16) | stack[i + 1]) >>> 0;
          if (inCode(v)) words.push([a - usp, v]);
        }
        sample = [kind, pc, usp, a5, words];
        lastUsp = usp;
      }
      stackLo = ((lastUsp || 0x358b60) - 0x400) & ~1;
      stack = new Uint16Array(0x1400);
      for (let i = 0; i < stack.length; i++) stack[i] = W(stackLo + 2 * i);
      wasm_retro_shell('clear'); wasm_retro_shell('r cpu'); wasm_retro_shell('m.w $3fffe0');
      const loc = new Uint8Array(768), hp = new Uint8Array(256);
      for (let i = 0; i < 768; i++) loc[i] = Module._wasm_peek(bases[23] + i);
      for (let i = 0; i < 256; i++) hp[i] = Module._wasm_peek(a4 + 0x460c + i);
      const moved = [], hurt = [];
      if (prevLoc) {
        for (let a = 0; a < 256; a++) {
          if ([0, 1, 2].some((j) => loc[3 * a + j] !== prevLoc[3 * a + j])) moved.push(a);
          if (hp[a] !== prevHp[a]) hurt.push([a, prevHp[a], hp[a]]);
        }
      }
      prevLoc = loc; prevHp = hp;
      R.rows.push([now, reading, gap, hash(0, 0, 176, 176), hash(176, 112, 312, 192), hash(176, 8, 312, 104), moved, hurt, sample, [P(a4 - 0x7b78), P(a4 - 0x7b76)]]);
    };
    for (const k of ['_wasm_draw_one_frame', '_wasm_execute']) {
      const orig = Module[k];
      Module[k] = (...a) => { const r = orig(...a); hook(); return r; };
    }
  }, [a4, bases, CODE]);
}

// One step: the key held until the game takes it, then rows until the command loop has read
// again for 60 frames after the turn.
async function step(page, key) {
  await page.evaluate((k) => { const R = window.__fight; R.rows = []; R.on = true; R.key = k; }, o.KEY[key]);
  const f0 = await o.frame(page);
  for (let seen = false; ;) {
    await o.waitFrames(page, 5);
    const r = await page.evaluate(() => window.__fight.rows.map((x) => x[1]));
    if (r.includes(0)) seen = true;
    if (seen && r.length - 1 - r.lastIndexOf(0) >= 60) break;
    if ((await o.frame(page)) - f0 > 4000) { log(`${key}: no prompt within 4000 frames`); break; }
  }
  return page.evaluate(() => { const R = window.__fight; R.on = false; return { rows: R.rows, pressedAt: R.pressedAt }; });
}

const h = await o.launch({ log: (s) => appendFileSync(LOG, `${new Date().toISOString()} ${s}\n`) });
const { page } = h;
try {
  await o.setFloppyCount(page, 4);
  for (const [i, v] of ['PROGRAM', 'POPULACE', 'PLAYER', 'PORTRAIT'].entries()) await o.insert(page, join(ADF, `${v}.adf`), i);
  if (process.env.U6_FIGHT_SNAPSHOT) {
    log(`snapshot ${process.env.U6_FIGHT_SNAPSHOT}: ${await loadSnapshot(page, process.env.U6_FIGHT_SNAPSHOT) || 'loaded'}`);
    await o.waitFrames(page, 20);
  } else {
    await o.reset(page);
    await o.ensureRunning(page);
    await o.waitFrames(page, 700);
    log(`palFix ${JSON.stringify(await o.palFix(page))}`);
    await o.waitGame(page, log);
    if (process.env.U6_FIGHT_SAVE) { await saveSnapshot(page, process.env.U6_FIGHT_SAVE); log(`saved ${process.env.U6_FIGHT_SAVE}`); }
  }
  const bases = await hunkBases(page);
  const a4 = bases[33] + 0x8000;
  log(`hunk bases ${bases.map((b) => b.toString(16)).join(',')}; A4 ${a4.toString(16)}; centre ${await o.peekW(page, a4 - 0x7b78)},${await o.peekW(page, a4 - 0x7b76)}`);
  await page.evaluate(() => wasm_retro_shell('debugger'));
  await o.waitFrames(page, 3);
  await installRecorder(page, a4, bases);
  const turns = [];
  for (const key of KEYS) {
    const r = await step(page, key);
    const zero = r.rows.filter((x) => x[1] === 0).map((x) => x[0]);
    turns.push({ key, ...r });
    log(`${key}: pressed at ${r.pressedAt}, read ${zero[0]}, prompt after ${zero.at(-1)}: ${zero.at(-1) - zero[0] + 1} frames; frames skipped ${r.rows.reduce((s, x) => s + x[2], 0)}`);
    writeFileSync(join(out, 'turns.json'), JSON.stringify({ bases, a4, turns }));
  }
} finally {
  await o.close(h);
}

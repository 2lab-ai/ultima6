// Fight pace of the original: boot the game-first floppies (as pace.mjs), take party steps in
// the start fight and record every emulated frame: the command loop's reading flag, the view
// centre, hashes of the map view, the message scroll and the party panel, the actors that moved
// or lost hit points, and a CPU sample (where the game is and who called it), so each frame of a
// turn can be put down to what the game was doing (fight.py).
// usage: node fight.mjs OUTDIR [KEY ...]   keys = lib.mjs KEY names, one party step each
//                                          (default UP RIGHT LEFT RIGHT LEFT RIGHT LEFT)
//   env U6_FIGHT_SNAPSHOT=file  start from a snapshot fight.mjs saved instead of booting
//       U6_FIGHT_SAVE=file      save the settled start there (a full boot takes ~3 min)
//       U6_FIGHT_SETUP=town     no fight: the start's assault actors passed over, combat broken
//                               off (game.mjs townSetup), so the steps are the castle's world turns
//       U6_GAME_BIN = flattened game image (hunk table and signatures)
// The CPU sample: vAmiga's debugger console runs a command between two frames, where the
// VBlank interrupt has just been taken (PC = the AROS level-3 handler, registers still the
// interrupted task's); the interrupted PC is in its exception frame on the supervisor stack
// (`m.w $3fffe0`), and the user stack, copied at the end of the frame before (it does not change
// in between), gives the return addresses of the callers. Output (keep it outside the repo):
// OUTDIR/turns.json ({bases, a4, turns: [{key, pressedAt, rows}]}) and fight.log.
import { appendFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import * as o from './lib.mjs';
import { CODE, hunkBases, loadSnapshot, saveSnapshot, townSetup } from './game.mjs';

const [out = join(o.SHOTS_DIR, `fight-${Date.now()}`), ...keyArgs] = process.argv.slice(2);
const KEYS = keyArgs.length ? keyArgs : ['UP', 'RIGHT', 'LEFT', 'RIGHT', 'LEFT', 'RIGHT', 'LEFT'];
const ADF = process.env.U6_ADF_DIR || join(o.ORACLE_HOME, 'adf-gamefirst');
mkdirSync(out, { recursive: true });
const LOG = join(out, 'fight.log');
const log = (s) => { appendFileSync(LOG, `${new Date().toISOString()} ${s}\n`); console.log(s); };

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
          // 0xfe8f1a: the level-3 autovector entry of the pinned AROS ROM (20260820, §1).
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
  const bases = await hunkBases(page, log);
  const a4 = bases[33] + 0x8000;
  log(`hunk bases ${bases.map((b) => b.toString(16)).join(',')}; A4 ${a4.toString(16)}; centre ${await o.peekW(page, a4 - 0x7b78)},${await o.peekW(page, a4 - 0x7b76)}`);
  if (process.env.U6_FIGHT_SETUP === 'town') log(`town: ${await townSetup(page, bases, a4)}`);
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

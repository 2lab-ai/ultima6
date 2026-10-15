// Walk pace of a held mouse button in the original: boot the game-first floppies (as play.mjs),
// find the game's data hunk in memory, put the pointer on a map cell beside the hero and hold
// the left button for N PAL frames, pressed and released at frame boundaries; the view centre
// (A4-0x7b78/-0x7b76, where the leader is) is read after every emulated frame, so each step's
// frame is known.
// usage: node pace.mjs OUTDIR [HOLD ...]     holds in PAL frames (default 5 10 25 50; 50 = 1 s)
//   env U6_PACE_SETUPS=start,quiet,b,poke  start states (default start; see setup())
//       U6_PACE_RUNS=N  trials per hold (default 3, each pressed at a later frame)
//       U6_PACE_CELLS='dx,dy;...'  cells to try in turn while a press walks nowhere
//       (default 0,3;0,-3;3,0;-3,0;0,2;2,0); U6_PACE_SHOTS=1 lores shots
//       U6_GAME_BIN = flattened game image (for the data-hunk signature)
// One boot = one time seed: the settled start is snapshotted, each setup is applied to it and
// snapshotted, and every trial starts from its setup's snapshot. Output (keep it outside the
// repo): OUTDIR/pace.json, one row per trial (setup, hold, cell, frames of the steps from the
// press, when the press was sampled, a timeline of the reading/held flags and view redraws),
// and pace.log.
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import * as o from './lib.mjs';

const [out = join(o.SHOTS_DIR, `pace-${Date.now()}`), ...holdArgs] = process.argv.slice(2);
const HOLDS = (holdArgs.length ? holdArgs : ['5', '10', '25', '50']).map(Number);
const RUNS = Number(process.env.U6_PACE_RUNS || 3);
const CELLS = (process.env.U6_PACE_CELLS || '0,3;0,-3;3,0;-3,0;0,2;2,0')
  .split(';').map((c) => c.split(',').map(Number));
const ADF = process.env.U6_ADF_DIR || join(o.ORACLE_HOME, 'adf-gamefirst');
const GAME_BIN = process.env.U6_GAME_BIN || '/Users/zhugehyuk/Games/ultima6/re/game.bin';
mkdirSync(out, { recursive: true });
const LOG = join(out, 'pace.log');
const log = (s) => { appendFileSync(LOG, `${new Date().toISOString()} ${s}\n`); console.log(s); };

// Image layout of `game` (re/notes/game-layout.md): hunk 33 = DATA at 0x1536f0, its loaded
// length read from the hunk header (tools/hunk2raw.py's JSON next to the image); SAS/C A4 =
// hunk base + 0x8000. Globals read (A4-relative, words):
const IMAGE_BASE = 0x100000, HUNK33 = 0x1536f0, A4_OFF = 0x8000;
const CENTRE = -0x7b78; // x, y, z (map/mod.rs)
const HELD = -0x7a36; // 1 while a button stays down after its press (0x106bbc)
const READING = -0x7a2c; // 1 while the command loop waits for a command key
const PRESSED = -0x7a34; // 1 at the sample where the buttons went down
const PRESS_CODE = -0x3076; // the click map's code for the press (a held button repeats only it)
const SAMPLE = 0x6198; // buttons (left byte, right byte), then x, y of the last pointer sample
const STATUS = 0x3d0c; // actor status bytes (objlist 0x0800); 0x1213de skips an actor with bit 0x10
const PARTY = 0x47ec, PARTY_SIZE = 0x4b00; // party member actor numbers, party size (bytes)
// Start states, each from the settled start: start = as booted (the start fight), b = `b`
// (break off combat), poke = status bit 0x10 on the actors outside the party, quiet = both.
const SETUPS = (process.env.U6_PACE_SETUPS || 'start').split(',');
const SHOTS = process.env.U6_PACE_SHOTS === '1'; // lores shots of the start and after each trial
const shot = async (page, name) => { if (SHOTS) await o.shot(page, name, { dir: out }); };

// poke: every actor outside the party gets status bit 0x10, so the world turn 0x1213de passes
// them over (the port's tests do the same); b: the game's own break-off-combat command. Both
// (quiet): a step is the party's move and the view alone.
async function setup(page, a4, name) {
  if (name === 'poke' || name === 'quiet') {
    const size = (await page.evaluate((a) => Module._wasm_peek16(a), a4 + PARTY_SIZE)) >> 8;
    const party = await page.evaluate(([a, n]) => Array.from({ length: n }, (_, i) => Module._wasm_peek(a + i)), [a4 + PARTY, size]);
    const status = await page.evaluate((a) => Array.from({ length: 256 }, (_, i) => Module._wasm_peek(a + i)), a4 + STATUS);
    await o.poke(page, a4 + STATUS, status.map((v, i) => (party.includes(i) ? v : v | 0x10)));
    log(`${name}: party ${party.join(',')}; status bit 0x10 on ${256 - party.length} actors`);
  }
  if (name === 'b' || name === 'quiet') {
    await o.tap(page, 'b');
    log(`${name}: b`);
  }
  if (name !== 'start') await idle(page, a4, 100, 20000);
}

// A 48-byte window of hunk 33's loaded bytes that holds no relocated long (no long in the image
// range, at any even offset) and is not filler: its runtime copy is byte-equal to the image.
function signature() {
  const img = readFileSync(GAME_BIN);
  const seg = JSON.parse(readFileSync(GAME_BIN.replace(/\.bin$/, '.json'), 'utf8')).segments[33];
  if (parseInt(seg.base, 16) !== HUNK33) throw new Error('hunk 33 is not where game-layout.md has it');
  const h = img.subarray(HUNK33 - IMAGE_BASE, HUNK33 - IMAGE_BASE + seg.loaded_bytes);
  const looksRelocated = (i) => { const v = h.readUInt32BE(i); return v >= IMAGE_BASE && v < 0x163000; };
  for (let off = 0x100; off + 52 < h.length; off += 2) {
    const w = h.subarray(off, off + 48);
    if (new Set(w).size < 24) continue;
    let ok = true;
    for (let i = off - 2; i < off + 48 && ok; i += 2) if (looksRelocated(i)) ok = false;
    if (ok) return { off, bytes: [...w] };
  }
  throw new Error('no reloc-free signature window in hunk 33');
}

// Runtime A4: every copy of the window in RAM (chip 0..2 MB, fast 2..4 MB); the live one is the
// copy whose pointer sample follows a mouse move.
async function findA4(page) {
  const sig = signature();
  const hits = await page.evaluate((b) => {
    const out = [];
    const first = (b[0] << 8) | b[1];
    for (let a = 0; a < 0x400000; a += 2) {
      if (Module._wasm_peek16(a) !== first) continue;
      let ok = true;
      for (let i = 2; i < b.length && ok; i += 2) if (Module._wasm_peek16(a + i) !== ((b[i] << 8) | b[i + 1])) ok = false;
      if (ok) out.push(a);
    }
    return out;
  }, sig.bytes);
  const cands = hits.map((a) => a - sig.off + A4_OFF);
  log(`signature at hunk 33 +${sig.off.toString(16)}: copies ${hits.map((a) => a.toString(16)).join(',')}`);
  for (const a4 of cands) {
    const before = await sampleAt(page, a4);
    await moveBy(page, 20, 10);
    const after = await sampleAt(page, a4);
    log(`A4 candidate ${a4.toString(16)}: sample ${JSON.stringify(before)} -> ${JSON.stringify(after)}`);
    if (after.x !== before.x || after.y !== before.y) return a4;
  }
  throw new Error('no live copy of the data hunk');
}

// The Amiga's mouse counters are 8 bits per axis, read once a frame, so a move goes in as steps
// of at most 32 counts, one per frame (the recorder applies them); then 4 frames for the sample.
async function moveBy(page, dx, dy) {
  await page.evaluate(([dx, dy]) => {
    const R = window.__pace;
    for (let n = Math.ceil(Math.max(Math.abs(dx), Math.abs(dy), 1) / 32), i = 0; i < n; i++) {
      R.moves.push([Math.round((dx * (i + 1)) / n) - Math.round((dx * i) / n), Math.round((dy * (i + 1)) / n) - Math.round((dy * i) / n)]);
    }
  }, [dx, dy]);
  await page.waitForFunction(() => window.__pace.moves.length === 0, null, { polling: 50, timeout: 60000 });
  await o.waitFrames(page, 4);
}

const peekS = (page, a) => page.evaluate((a) => (Module._wasm_peek16(a) << 16) >> 16, a);
async function sampleAt(page, a4) {
  return { x: await peekS(page, a4 + SAMPLE + 2), y: await peekS(page, a4 + SAMPLE + 4) };
}
async function centre(page, a4) {
  return [await peekS(page, a4 + CENTRE), await peekS(page, a4 + CENTRE + 2), await peekS(page, a4 + CENTRE + 4)];
}

// Pointer to game pixel (x, y): to the corner (the handler clamps at 0, 0), then by the measured
// counts per pixel, corrected until the sample is there.
let scale = null;
async function pointTo(page, a4, x, y) {
  await moveBy(page, -700, -500);
  if (!scale) {
    await moveBy(page, 100, 60);
    const s = await sampleAt(page, a4);
    scale = { x: 100 / s.x, y: 60 / s.y };
    log(`mouse counts per game pixel ${JSON.stringify(scale)} (100,60 -> ${JSON.stringify(s)})`);
    await moveBy(page, -700, -500);
  }
  await moveBy(page, Math.round(x * scale.x), Math.round(y * scale.y));
  for (let i = 0; i < 6; i++) {
    const s = await sampleAt(page, a4);
    if (s.x === x && s.y === y) return s;
    await moveBy(page, Math.round((x - s.x) * scale.x), Math.round((y - s.y) * scale.y));
  }
  return sampleAt(page, a4);
}

// In-page recorder: after every emulated frame (the UI's rAF and catch-up calls), one row
// [frame, cx, cy, cz, held, left, reading] once A4 is known; queued mouse moves (one per frame)
// and scheduled button changes go in at frame boundaries.
async function installRecorder(page) {
  await page.evaluate(([CENTRE, HELD, READING, SAMPLE, PRESSED, PRESS_CODE]) => {
    const P = (a) => (Module._wasm_peek16(a) << 16) >> 16;
    const R = window.__pace = { rows: [], plan: [], moves: [], on: false, gaps: 0, a4: 0 };
    const f = () => Module._wasm_frame_info() >>> 2;
    let last = f();
    const hook = () => {
      const now = f();
      if (now === last) return;
      if (now !== last + 1) R.gaps++;
      last = now;
      const m = R.moves.shift();
      if (m) wasm_mouse(1, m[0], m[1]);
      for (const ev of R.plan) if (ev.at <= now && ev.done === undefined) { ev.done = now; wasm_mouse_button(1, 1, ev.down); }
      const a4 = R.a4;
      if (!R.on) return;
      // FNV-1a of the map view (lores pixels 100..262 x 50..205 of the texture): when it was redrawn
      const v = new Uint32Array(Module.HEAPU8.buffer, Module._wasm_pixel_buffer(), 1824 * 313);
      let hsh = 0x811c9dc5;
      for (let y = 50; y < 205; y++) for (let x = 100; x < 262; x++) { hsh ^= v[y * 1824 + x * 4]; hsh = Math.imul(hsh, 0x01000193); }
      R.rows.push([now, P(a4 + CENTRE), P(a4 + CENTRE + 2), P(a4 + CENTRE + 4), P(a4 + HELD), P(a4 + SAMPLE) >> 8, P(a4 + READING), hsh >>> 0, P(a4 + PRESSED), P(a4 + PRESS_CODE)]);
    };
    for (const k of ['_wasm_draw_one_frame', '_wasm_execute']) {
      const orig = Module[k];
      Module[k] = (...a) => { const r = orig(...a); hook(); return r; };
    }
  }, [CENTRE, HELD, READING, SAMPLE, PRESSED, PRESS_CODE]);
}

// Snapshots kept in the page by name ('base' = the settled start, then one per setup).
const snapshot = (page, name) => page.evaluate((name) => {
  const s = JSON.parse(wasm_take_user_snapshot());
  (window.__snaps ??= {})[name] = new Uint8Array(Module.HEAPU8.buffer, s.address, s.size).slice();
  wasm_delete_user_snapshot();
  return window.__snaps[name].length;
}, name);
const restore = (page, name) => page.evaluate((name) => {
  wasm_halt();
  const r = wasm_loadfile('pace.vAmiga', window.__snaps[name]);
  wasm_run();
  return r;
}, name);

// Waits until the command loop reads keys and the view centre has kept still `frames` frames.
async function idle(page, a4, frames = 40, timeout = 2000) {
  const f0 = await o.frame(page);
  let c = JSON.stringify(await centre(page, a4)), since = f0;
  for (;;) {
    await o.waitFrames(page, 5);
    const f = await o.frame(page), n = JSON.stringify(await centre(page, a4));
    if (n !== c || (await peekS(page, a4 + READING)) !== 1) { c = n; since = f; }
    if (f - since >= frames) return;
    if (f - f0 > timeout) throw new Error('the game did not come back to the prompt');
  }
}

// One trial: press at the next frame boundary, release `hold` frames later, record until the
// centre has kept still 100 frames after the release. Steps = frames where the centre changed.
async function trial(page, a4, hold) {
  await page.evaluate((hold) => {
    const R = window.__pace, now = Module._wasm_frame_info() >>> 2;
    R.rows = [];
    R.plan = [{ at: now + 1, down: 1 }, { at: now + 1 + hold, down: 0 }];
    R.on = true;
  }, hold);
  for (;;) {
    await o.waitFrames(page, 10);
    const r = await page.evaluate(() => {
      const R = window.__pace, rel = R.plan[1].done;
      if (rel === undefined) return null;
      let lastMove = rel;
      for (let i = 1; i < R.rows.length; i++) if (R.rows[i].slice(1, 4).join() !== R.rows[i - 1].slice(1, 4).join()) lastMove = Math.max(lastMove, R.rows[i][0]);
      const now = Module._wasm_frame_info() >>> 2;
      return now - lastMove >= 100 && R.rows.at(-1)[6] === 1 ? { rows: R.rows, plan: R.plan, gaps: R.gaps } : null;
    });
    if (r) {
      await page.evaluate(() => { window.__pace.on = false; });
      const press = r.plan[0].done, release = r.plan[1].done;
      const steps = [];
      for (let i = 1; i < r.rows.length; i++) {
        const [fr, x, y] = r.rows[i], [, px, py] = r.rows[i - 1];
        if (x !== px || y !== py) steps.push({ frame: fr - press, dx: x - px, dy: y - py });
      }
      const seen = r.rows.find((row) => row[4] === 1);
      // Timeline relative to the press: reading flag changes (r0/r1), held flag, view redraws.
      const tl = [];
      for (let i = 1; i < r.rows.length; i++) {
        const [fr, , , , held, , rd, hh] = r.rows[i], p = r.rows[i - 1];
        if (rd !== p[6]) tl.push(`${fr - press}:r${rd}`);
        if (held !== p[4]) tl.push(`${fr - press}:h${held}`);
        if (hh !== p[7]) tl.push(`${fr - press}:v`);
        if (r.rows[i][8] !== p[8]) tl.push(`${fr - press}:p${r.rows[i][8]}`);
        if (r.rows[i][9] !== p[9]) tl.push(`${fr - press}:c${(r.rows[i][9] & 0xffff).toString(16)}`);
      }
      return { hold, press, release: release - press, held_seen: seen ? seen[0] - press : null, steps, gaps: r.gaps, timeline: tl.join(' ') };
    }
  }
}

const h = await o.launch({ log: (s) => appendFileSync(LOG, `${new Date().toISOString()} ${s}\n`) });
const { page } = h;
const results = [];
try {
  await o.setFloppyCount(page, 4);
  for (const [i, v] of ['PROGRAM', 'POPULACE', 'PLAYER', 'PORTRAIT'].entries()) await o.insert(page, join(ADF, `${v}.adf`), i);
  await o.reset(page);
  await o.ensureRunning(page);
  log(`worker build: ${await page.evaluate(() => Module._wasm_is_worker_built())}`);
  await o.waitFrames(page, 700);
  log(`palFix ${JSON.stringify(await o.palFix(page))}`);
  await o.waitGame(page, log);
  await installRecorder(page);
  const a4 = await findA4(page);
  await page.evaluate((a4) => { window.__pace.a4 = a4; }, a4);
  log(`A4 = ${a4.toString(16)}; centre ${await centre(page, a4)}`);
  await idle(page, a4);
  log(`base snapshot ${await snapshot(page, 'base')} bytes at frame ${await o.frame(page)}`);
  for (const name of SETUPS) {
    await restore(page, 'base');
    await o.waitFrames(page, 5);
    await setup(page, a4, name);
    await shot(page, name);
    log(`${name} snapshot ${await snapshot(page, name)} bytes; centre ${await centre(page, a4)}`);
    for (const hold of HOLDS) {
      for (let run = 0; run < RUNS; run++) {
        for (const [dx, dy] of CELLS) {
          await restore(page, name);
          await o.waitFrames(page, 5);
          const at = await pointTo(page, a4, 88 + 16 * dx, 88 + 16 * dy);
          await idle(page, a4, 20 + 7 * run); // a different press phase per run
          const c0 = await centre(page, a4);
          const row = { setup: name, ...(await trial(page, a4, hold)), cell: [dx, dy], pointer: at, from: c0, run };
          await shot(page, `${name}-h${hold}-r${run}-c${dx}_${dy}`);
          log(`${name} hold ${hold} run ${run} cell ${dx},${dy}: ${row.steps.length} steps at ${row.steps.map((s) => s.frame).join(',')} (held seen +${row.held_seen}, gaps ${row.gaps})`);
          results.push(row);
          writeFileSync(join(out, 'pace.json'), JSON.stringify(results, null, 1));
          // A press the game never sampled (it reads the handler's button word, 0x14f12c, so a
          // click inside one of its redraws is missed) walks nowhere in any direction: no retry.
          if (row.steps.length || row.held_seen === null) break;
        }
      }
    }
  }
} finally {
  await o.close(h);
}

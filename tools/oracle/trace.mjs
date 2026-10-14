// Exact routine times in the original's fight (or town) steps, by the debugger: a breakpoint at
// each traced routine's entry; at a stop there the return address ([A7]) gets a breakpoint of
// its own, and the stop there with A7 four bytes higher ends that call. The frame counter and
// the beam line (313 a frame) at both stops give its length in PAL frames; the emulator stands
// still while stopped, so the stops cost the game nothing. Checks what fight.py estimates from
// one CPU sample per frame (counts and lengths).
// usage: node trace.mjs OUTDIR SNAPSHOT [KEY ...]   (default UP RIGHT LEFT RIGHT LEFT RIGHT LEFT)
//   env U6_FIGHT_SETUP=town: the town setup of fight.mjs (no fight) before the steps
// Output: OUTDIR/trace.json ({steps: [{key, read, prompt, calls: [{fn, at, frames, depth}]}]}).
import { appendFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import * as o from './lib.mjs';
import { hunkBases, loadSnapshot, runtime, townSetup } from './game.mjs';

const [out, snap, ...keyArgs] = process.argv.slice(2);
if (!out || !snap) throw new Error('usage: node trace.mjs OUTDIR SNAPSHOT [KEY ...]');
const KEYS = keyArgs.length ? keyArgs : ['UP', 'RIGHT', 'LEFT', 'RIGHT', 'LEFT', 'RIGHT', 'LEFT'];
const ADF = process.env.U6_ADF_DIR || join(o.ORACLE_HOME, 'adf-gamefirst');
mkdirSync(out, { recursive: true });
const log = (s) => { appendFileSync(join(out, 'trace.log'), `${new Date().toISOString()} ${s}\n`); console.log(s); };
const hex = (v) => `$${v.toString(16)}`;
// The traced routines (image addresses).
const TRACED = {
  0x1213de: 'world turn', 0x1200b0: 'NPC turn', 0x102f2a: 'refresh after a turn',
  0x132706: 'flight', 0x1230ce: 'hit flash', 0x108658: 'printf', 0x10eef8: 'object name',
};

const h = await o.launch({ log: (s) => appendFileSync(join(out, 'trace.log'), `${s}\n`) });
const { page } = h;
// The last console prompt: beam line and PC, and the console text.
const prompt = async () => {
  const t = await page.evaluate(() => wasm_retro_shell_get_text());
  const m = [...t.matchAll(/\((\d+),(\d+)\) \$([0-9a-f]+):/g)].at(-1);
  return m ? { v: Number(m[1]), pc: parseInt(m[3], 16) } : null;
};
try {
  await o.setFloppyCount(page, 4);
  for (const [i, v] of ['PROGRAM', 'POPULACE', 'PLAYER', 'PORTRAIT'].entries()) await o.insert(page, join(ADF, `${v}.adf`), i);
  await loadSnapshot(page, snap);
  await o.waitFrames(page, 30);
  const bases = await hunkBases(page, log);
  const a4 = bases[33] + 0x8000;
  if (process.env.U6_FIGHT_SETUP === 'town') log(`town: ${await townSetup(page, bases, a4)}`);
  // Key held until the game takes it (A4-0x7a2c cleared), as fight.mjs; the reading flag kept.
  await page.evaluate((a4) => {
    const R = window.__trace = { key: null, down: false, at: 0, reading: [] };
    const f = () => Module._wasm_frame_info() >>> 2;
    let last = f();
    const hook = () => {
      const now = f();
      if (now === last) return;
      last = now;
      const r = (Module._wasm_peek16(a4 - 0x7a2c) << 16) >> 16;
      R.reading.push([now, r]);
      if (R.reading.length > 400) R.reading.shift();
      if (R.key !== null && !R.down) { wasm_key(R.key, 1); R.down = true; R.at = now; }
      else if (R.down && (r === 0 || now - R.at > 50)) { wasm_key(R.key, 0); R.down = false; R.key = null; }
    };
    for (const k of ['_wasm_draw_one_frame', '_wasm_execute']) {
      const orig = Module[k];
      Module[k] = (...a) => { const v = orig(...a); hook(); return v; };
    }
  }, a4);
  await page.evaluate(() => wasm_retro_shell('debugger'));
  await o.waitFrames(page, 3);
  const entries = new Map(Object.keys(TRACED).map((a) => [runtime(bases, Number(a)), Number(a)]));
  for (const a of entries.keys()) await page.evaluate((c) => wasm_retro_shell(c), `break at ${hex(a)}`);
  await o.waitFrames(page, 3);
  const returns = new Set();
  const steps = [];
  for (const key of KEYS) {
    await page.evaluate((k) => { window.__trace.key = k; window.__trace.reading = []; }, o.KEY[key]);
    const calls = [], open = [];
    let seenZero = false, read = null, done = null, stuck = 0, last = null, strays = 0;
    for (;;) {
      const f1 = await o.frame(page);
      await new Promise((r) => setTimeout(r, 40));
      const f2 = await o.frame(page);
      const p = f1 === f2 ? await prompt() : null;
      const hit = p && (entries.has(p.pc) || returns.has(p.pc));
      if (!hit) {
        const rd = await page.evaluate(() => window.__trace.reading);
        const z = rd.filter(([, r]) => r === 0);
        if (z.length) { seenZero = true; read ??= z[0][0]; }
        if (seenZero && rd.length && rd.at(-1)[1] === 1 && rd.at(-1)[0] - (z.at(-1)?.[0] ?? rd.at(-1)[0]) >= 60) { done = rd.at(-1)[0]; break; }
        // Stopped where nothing is traced (after a one-instruction step): run on.
        if (f1 === f2 && p && ++stuck > 3) {
          if (++strays > 50) throw new Error(`stopped at ${JSON.stringify(p)} again and again, not a traced address`);
          await o.shellKeys(page, 'clear');
          await o.shellKeys(page, 'g');
          stuck = 0;
        }
        if (f1 !== f2) stuck = 0;
        continue;
      }
      stuck = 0;
      if (last && last.pc === p.pc && last.f === f2 && last.v === p.v) {
        // The same stop again (a breakpoint under the PC): one instruction past it, then on.
        await o.shellKeys(page, 's');
        await o.shellKeys(page, 'g');
        continue;
      }
      last = { pc: p.pc, f: f2, v: p.v };
      const regs = await o.shellKeys(page, 'r cpu');
      const a7 = parseInt(/A4 - A7 : (?:0x[0-9a-f]+ ){3}0x([0-9a-f]+)/.exec(regs)[1], 16);
      const d0 = parseInt(/D0 - D3 : 0x([0-9a-f]+)/.exec(regs)[1], 16);
      const at = f2 + p.v / 313;
      if (entries.has(p.pc)) {
        const ret = await o.peekL(page, a7);
        if (!returns.has(ret) && !entries.has(ret)) { await o.shellKeys(page, `break at ${hex(ret)}`); returns.add(ret); }
        const fn = entries.get(p.pc);
        // An NPC turn's actor (D0) and its worktype then (0x139188 = hunk 23 + 0x3c00).
        const actor = fn === 0x1200b0 ? d0 & 0xff : null;
        const worktype = actor === null ? null : await page.evaluate((a) => Module._wasm_peek(a), bases[23] + 0x3c00 + actor);
        open.push({ fn, a7, at, depth: open.length, actor, worktype });
      }
      if (returns.has(p.pc)) {
        const k = open.findLastIndex((c) => c.a7 + 4 === a7);
        if (k >= 0) {
          const c = open[k];
          open.length = k;
          calls.push({ fn: c.fn, at: c.at, frames: at - c.at, depth: c.depth, actor: c.actor, worktype: c.worktype });
        }
      }
      await o.shellKeys(page, 'clear');
      await o.shellKeys(page, 'g');
    }
    if (open.length) throw new Error(`${key}: calls still open at the prompt: ${JSON.stringify(open)}`);
    steps.push({ key, read, prompt: done, calls });
    const by = {};
    for (const c of calls) (by[TRACED[c.fn]] ??= []).push(c.frames);
    log(`${key}: read ${read}, prompt ${done}: ${Object.entries(by).map(([k, v]) => `${k} ${v.length}× ${v.reduce((a, b) => a + b, 0).toFixed(1)}`).join(', ')}`);
    writeFileSync(join(out, 'trace.json'), JSON.stringify({ bases, steps }, null, 1));
  }
} finally {
  await o.close(h);
}

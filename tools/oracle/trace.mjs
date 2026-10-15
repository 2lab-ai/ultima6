// Exact routine times in the original's fight (or town) steps, by the debugger: a breakpoint at
// each traced routine's entry; at a stop there the return address ([A7]) gets a breakpoint of
// its own, and the stop there with A7 four bytes higher ends that call. The CPU's clock at both
// stops gives its length in PAL frames (142,102 cycles a frame); the emulator stands still while
// stopped, so the stops cost the game nothing. Checks what fight.py estimates from
// one CPU sample per frame (counts and lengths).
// usage: node trace.mjs OUTDIR SNAPSHOT [KEY ...]   (default UP RIGHT LEFT RIGHT LEFT RIGHT LEFT)
//   env U6_FIGHT_SETUP=town: the town setup of fight.mjs (no fight) before the steps
// Output: OUTDIR/trace.json ({bases, keys, fresh, steps: [{key, read, back, prompt, moved, calls:
// [{fn, at, frames, depth, combat, followers, strict, moved, spent}]}], complete}), written after
// every step; `complete` turns true only once every key's step is in (trace.py refuses a file
// without it). `fresh`: no command ran between the snapshot (the settled start after the boot)
// and the first key, so the first step's world turn is the first since the boot. A step's `read`
// is the first frame the command loop's reading flag A4-0x7a2c was clear, `back` the first frame
// it was set again (the next key read: key to prompt = back - read), `prompt` the frame the step
// was taken as done (60 frames after), `moved` whether the leader stands elsewhere at the prompt.
// An NPC turn has `combat` (its worktype at entry runs a combat routine), a followers' pass the
// count of members it may move, `strict` (its D1: 1 from Pass 0x104c0a, 0 from the party step)
// and `moved` (a member stands elsewhere at its return), a party step `spent` (the leader's
// movement points at its return differ from its entry). Spec 01 §Rights: what the tool reads
// out of the game's memory (actors, places, points, worktypes) is compared here and written
// only as these booleans and counts, in trace.json and in the log alike.
import { appendFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import * as o from './lib.mjs';
import { cpuFrames, hunkBases, loadSnapshot, runtime, townSetup } from './game.mjs';
import { closeAt, stopPrompt } from './tracecore.mjs';

const [out, snap, ...keyArgs] = process.argv.slice(2);
if (!out || !snap) throw new Error('usage: node trace.mjs OUTDIR SNAPSHOT [KEY ...]');
const KEYS = keyArgs.length ? keyArgs : ['UP', 'RIGHT', 'LEFT', 'RIGHT', 'LEFT', 'RIGHT', 'LEFT'];
const ADF = process.env.U6_ADF_DIR || join(o.ORACLE_HOME, 'adf-gamefirst');
mkdirSync(out, { recursive: true });
const log = (s) => { appendFileSync(join(out, 'trace.log'), `${new Date().toISOString()} ${s}\n`); console.log(s); };
const hex = (v) => `$${v.toString(16)}`;
// A step that has not come back to the prompt by then is rejected (the start fight's longest
// step is ~540 frames; a traced one runs ~30 wall seconds a frame-hundred).
const STEP_FRAMES = 6000;
// The worktypes 0x1200b0 runs through a combat routine (its dispatch 0x120184..0x120248; the
// port's actors/npc.rs runs_combat_routine).
const COMBAT_ROUTINES = new Set([3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 0x13, 0x9a, 0x9b]);
const STEP_WALL_MS = 40 * 60000;
// The traced routines (image addresses): the party step 0x11d4ee, with the followers' pass it
// ends in (0x11c910), is the walk command's own work before the world turn; the world turn
// 0x1213de begins with its setup, 0x121056 (also called after each NPC turn), 0x120b64 and
// 0x120a1c, before its first pick, and when no one near has points left refills them and
// advances the clock a minute (0x10368a through 0x1219b8).
const TRACED = {
  0x1213de: 'world turn', 0x1200b0: 'NPC turn', 0x102f2a: 'refresh after a turn',
  0x132706: 'flight', 0x1230ce: 'hit flash', 0x108658: 'printf', 0x10eef8: 'object name',
  0x11d4ee: 'party step', 0x11c910: 'follow',
  0x121056: 'turn bookkeeping', 0x120b64: 'party centre', 0x120a1c: 'schedules',
  0x10368a: 'clock minute',
};

const h = await o.launch({ log: (s) => appendFileSync(join(out, 'trace.log'), `${s}\n`) });
const { page } = h;
// The console prompt printed since the last resume (`mark` = the console's length right after
// it; tracecore.mjs stopPrompt).
let mark = 0;
const prompt = async () => stopPrompt(await page.evaluate(() => wasm_retro_shell_get_text()), mark);
// Runs on (`g`) or steps (`s`) from a stop, and marks the console there: typed and marked in
// one page call, so no frame can run (and stop) between the command and the mark.
const resume = async (cmd) => {
  mark = await page.evaluate((cmd) => {
    for (const ch of `clear\n${cmd}\n`) Module._wasm_retro_shell_press_key(ch.charCodeAt(0));
    return wasm_retro_shell_get_text().length;
  }, cmd);
};
try {
  await o.setFloppyCount(page, 4);
  for (const [i, v] of ['PROGRAM', 'POPULACE', 'PLAYER', 'PORTRAIT'].entries()) await o.insert(page, join(ADF, `${v}.adf`), i);
  await loadSnapshot(page, snap);
  await o.waitFrames(page, 30);
  const bases = await hunkBases(page, log);
  const a4 = bases[33] + 0x8000;
  // Compared only, never written (see the header): the leader (the party list A4+0x47ec at the
  // leader's index A4-0x7b7a) and its location bytes (3 at hunk 23 + 3 actor) as one string, its
  // movement points (A4+0x430c), and the location bytes of every party member (A4+0x4b00 of
  // them).
  const leaderPlace = () => page.evaluate(([a4, locs]) => {
    const actor = Module._wasm_peek(a4 + 0x47ec + Module._wasm_peek(a4 - 0x7b7a));
    return [actor, ...[0, 1, 2].map((i) => Module._wasm_peek(locs + 3 * actor + i))].join();
  }, [a4, bases[23]]);
  const leaderPoints = () => page.evaluate((a4) => {
    const actor = Module._wasm_peek(a4 + 0x47ec + Module._wasm_peek(a4 - 0x7b7a));
    return Module._wasm_peek(a4 + 0x430c + actor);
  }, a4);
  const partyPlaces = () => page.evaluate(([a4, locs]) => {
    const out = [];
    for (let i = 0; i < Module._wasm_peek(a4 + 0x4b00); i++) {
      const m = Module._wasm_peek(a4 + 0x47ec + i);
      out.push(m, ...[0, 1, 2].map((k) => Module._wasm_peek(locs + 3 * m + k)));
    }
    return out.join();
  }, [a4, bases[23]]);
  const fresh = process.env.U6_FIGHT_SETUP !== 'town';
  if (!fresh) log(`town: ${await townSetup(page, bases, a4)}`);
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
  // The console keeps a bounded scrollback: cleared here (and at every resume), so a stop's
  // prompt always lies past the mark.
  await page.evaluate(() => wasm_retro_shell('clear'));
  await o.waitFrames(page, 3);
  mark = await page.evaluate(() => wasm_retro_shell_get_text().length);
  log(`console cleared: ${mark} chars`);
  const returns = new Set();
  const steps = [];
  for (const key of KEYS) {
    await page.evaluate((k) => { window.__trace.key = k; window.__trace.reading = []; }, o.KEY[key]);
    const calls = [], open = [];
    let seenZero = false, read = null, back = null, done = null, stuck = 0, last = null, strays = 0;
    const from = await leaderPlace();
    const startFrame = await o.frame(page), startWall = Date.now();
    for (;;) {
      if ((await o.frame(page)) - startFrame > STEP_FRAMES || Date.now() - startWall > STEP_WALL_MS) {
        throw new Error(`${key}: no prompt within ${STEP_FRAMES} frames / ${STEP_WALL_MS / 60000} min`);
      }
      const f1 = await o.frame(page);
      await new Promise((r) => setTimeout(r, 40));
      const f2 = await o.frame(page);
      const p = f1 === f2 ? await prompt() : null;
      const hit = p && (entries.has(p.pc) || returns.has(p.pc));
      if (process.env.U6_TRACE_DEBUG && Date.now() - (globalThis.__dbg ?? 0) > 15000) {
        globalThis.__dbg = Date.now();
        const t = await page.evaluate(() => wasm_retro_shell_get_text());
        // The console's length only: its text disassembles the game's code.
        log(`debug: frames ${f1} ${f2}, prompt ${JSON.stringify(p)}, mark ${mark}, text ${t.length}, open ${open.length}, calls ${calls.length}`);
      }
      if (!hit) {
        const rd = await page.evaluate(() => window.__trace.reading);
        const z = rd.filter(([, r]) => r === 0);
        if (z.length) { seenZero = true; read ??= z[0][0]; }
        if (seenZero && rd.length && rd.at(-1)[1] === 1 && rd.at(-1)[0] - (z.at(-1)?.[0] ?? rd.at(-1)[0]) >= 60) {
          done = rd.at(-1)[0];
          back = rd.find(([f, r]) => f > z.at(-1)[0] && r === 1)[0];
          break;
        }
        // Stopped where nothing is traced (after a one-instruction step): run on.
        if (f1 === f2 && p && ++stuck > 3) {
          if (++strays > 50) throw new Error(`stopped at ${JSON.stringify(p)} again and again, not a traced address`);
          await resume('g');
          stuck = 0;
        }
        if (f1 !== f2) stuck = 0;
        continue;
      }
      stuck = 0;
      if (last && last.pc === p.pc && last.f === f2 && last.v === p.v) {
        // The same stop again (a breakpoint under the PC): one instruction past it, then on.
        await resume('s');
        await resume('g');
        continue;
      }
      last = { pc: p.pc, f: f2, v: p.v };
      const regs = await o.shellKeys(page, 'r cpu');
      const a7 = parseInt(/A4 - A7 : (?:0x[0-9a-f]+ ){3}0x([0-9a-f]+)/.exec(regs)[1], 16);
      const [, d0, d1] = /D0 - D3 : 0x([0-9a-f]+) 0x([0-9a-f]+)/.exec(regs).map((v) => parseInt(v, 16));
      const at = await cpuFrames(page);
      if (entries.has(p.pc)) {
        const ret = await o.peekL(page, a7);
        if (!returns.has(ret) && !entries.has(ret)) { await o.shellKeys(page, `break at ${hex(ret)}`); returns.add(ret); }
        const fn = entries.get(p.pc);
        // An NPC turn (actor in D0): whether its worktype then (0x139188 = hunk 23 + 0x3c00) runs a
        // combat routine.
        const combat = fn !== 0x1200b0 ? null
          : COMBAT_ROUTINES.has(await page.evaluate((a) => Module._wasm_peek(a), bases[23] + 0x3c00 + (d0 & 0xff)));
        // The followers' pass (leader in D0): how many members besides the leader are in
        // worktype 1, the ones it may move (the party list A4+0x47ec, its size A4+0x4b00).
        const followers = fn !== 0x11c910 ? null : await page.evaluate(([a4, wt, leader]) => {
          let n = 0;
          for (let i = 0; i < Module._wasm_peek(a4 + 0x4b00); i++) {
            const m = Module._wasm_peek(a4 + 0x47ec + i);
            if (m !== leader && Module._wasm_peek(wt + m) === 1) n++;
          }
          return n;
        }, [a4, bases[23] + 0x3c00, d0 & 0xff]);
        // The followers' pass's D1 (Pass's strict pass is 1) and where the members stand; the
        // party step's leader points. Kept to compare at the return, not written.
        const strict = fn === 0x11c910 ? ((d1 & 0xffff) !== 0 ? 1 : 0) : null;
        const places = fn === 0x11c910 ? await partyPlaces() : null;
        const points = fn === 0x11d4ee ? await leaderPoints() : null;
        open.push({ fn, ret, a7, at, depth: open.length, combat, followers, strict, places, points });
      }
      if (returns.has(p.pc)) {
        // The youngest open call returns, or none does (tracecore.mjs closeAt); an older one
        // returning with younger ones open ends the trace.
        let top;
        try {
          top = closeAt(open, p.pc, a7, (fn) => TRACED[fn]);
        } catch (e) {
          throw new Error(`${key}: ${e.message}`);
        }
        if (top) {
          const moved = top.places === null ? null : (await partyPlaces()) !== top.places;
          const spent = top.points === null ? null : (await leaderPoints()) !== top.points;
          calls.push({ fn: top.fn, at: top.at, frames: at - top.at, depth: top.depth, combat: top.combat, followers: top.followers, strict: top.strict, moved, spent });
        }
      }
      await resume('g');
    }
    if (open.length) throw new Error(`${key}: calls still open at the prompt: ${open.map((c) => TRACED[c.fn]).join(', ')}`);
    const moved = (await leaderPlace()) !== from;
    steps.push({ key, read, back, prompt: done, moved, calls });
    const by = {};
    for (const c of calls) (by[TRACED[c.fn]] ??= []).push(c.frames);
    log(`${key}: read ${read}, back ${back}, prompt ${done}, leader ${moved ? 'moved' : 'stayed'}: ${Object.entries(by).map(([k, v]) => `${k} ${v.length}× ${v.reduce((a, b) => a + b, 0).toFixed(1)}`).join(', ')}`);
    writeFileSync(join(out, 'trace.json'), JSON.stringify({ bases, keys: KEYS, fresh, steps, complete: false }, null, 1));
  }
  writeFileSync(join(out, 'trace.json'), JSON.stringify({ bases, keys: KEYS, fresh, steps, complete: true }, null, 1));
  log(`complete: ${steps.length} steps`);
} finally {
  await o.close(h);
}

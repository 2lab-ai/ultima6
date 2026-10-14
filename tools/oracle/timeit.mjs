// The original's time for one drawing routine, in PAL frames: from a settled start (a snapshot
// fight.mjs saved, U6_FIGHT_SAVE) the debugger stops at the entry of the prompt's idle render
// 0x102c90 (D0 = 1: framed and copied to the screen, called from the key wait 0x105c8e), then
// the routine runs in its place (`g` to it, so its rts goes back to the render's caller) and a
// breakpoint at that return address stops it again; the frames between the two stops, with the
// beam lines (313 a frame) as the fraction, are its time. D0 = 1 at that entry is a valid
// argument for both targets below (circle 1, the Avatar), D1 the caller's (non-zero: bright).
// usage: node timeit.mjs OUTDIR SNAPSHOT NAME
//   NAME = render (0x102c90 itself), dissolve (0x102c90 with A4-0x6578 = 1: the window copied
//   pixel by pixel, 0x1520e2), actor (the actor flash 0x132338), window (the window flash
//   0x115b7c). One routine per run: a stopped debugger does not take the next snapshot.
import { appendFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import * as o from './lib.mjs';
import { hunkBases, loadSnapshot, runtime } from './game.mjs';

const [out, snap, name] = process.argv.slice(2);
const TARGETS = { render: null, dissolve: null, actor: 0x132338, window: 0x115b7c };
if (!out || !snap || !(name in TARGETS)) throw new Error('usage: node timeit.mjs OUTDIR SNAPSHOT render|dissolve|actor|window');
mkdirSync(out, { recursive: true });
const log = (s) => { appendFileSync(join(out, 'timeit.log'), `${new Date().toISOString()} ${s}\n`); console.log(s); };
const ADF = process.env.U6_ADF_DIR || join(o.ORACLE_HOME, 'adf-gamefirst');
const hex = (v) => `$${v.toString(16)}`;

// The frame at which the emulator stopped at one of `pcs` (the frame counter keeps still and
// the console's last prompt is there; a host that lags can hold the counter still while the
// emulator runs, so the counter alone is no proof).
async function stopped(page, pcs) {
  for (let i = 0; i < 2400; i++) {
    const f = await o.frame(page);
    await new Promise((r) => setTimeout(r, 250));
    if ((await o.frame(page)) === f && pcs.includes((await where(page)).pc)) return f;
  }
  throw new Error(`the debugger did not stop at ${pcs.map(hex).join(' / ')}`);
}
// The last prompt of the console: beam line and PC.
async function where(page) {
  const t = await page.evaluate(() => wasm_retro_shell_get_text());
  const m = [...t.matchAll(/\((\d+),(\d+)\) \$([0-9a-f]+):/g)].at(-1);
  return { v: Number(m[1]), pc: parseInt(m[3], 16) };
}

const h = await o.launch({ log: (s) => appendFileSync(join(out, 'timeit.log'), `${s}\n`) });
const { page } = h;
try {
  await o.setFloppyCount(page, 4);
  for (const [i, v] of ['PROGRAM', 'POPULACE', 'PLAYER', 'PORTRAIT'].entries()) await o.insert(page, join(ADF, `${v}.adf`), i);
  await loadSnapshot(page, snap);
  await o.waitFrames(page, 30);
  const bases = await hunkBases(page, log);
  const a4 = bases[33] + 0x8000;
  await page.evaluate(() => wasm_retro_shell('debugger'));
  await o.waitFrames(page, 3);
  await page.evaluate((c) => { wasm_retro_shell('clear'); wasm_retro_shell(c); }, `break at ${hex(runtime(bases, 0x102c90))}`);
  const f0 = await stopped(page, [runtime(bases, 0x102c90)]);
  const regs = await o.shellKeys(page, 'r cpu');
  const a7 = parseInt(/A4 - A7 : (?:0x[0-9a-f]+ ){3}0x([0-9a-f]+)/.exec(regs)[1], 16);
  const d01 = /D0 - D3 : 0x([0-9a-f]+) 0x([0-9a-f]+)/.exec(regs);
  const entry = await where(page);
  if (entry.pc !== runtime(bases, 0x102c90) || parseInt(d01[1], 16) !== 1 || /Flags : \S*S/.test(regs)) {
    throw new Error(`not stopped at the idle render's entry with D0 = 1 in user mode: ${hex(entry.pc)}, D0 ${d01[1]}`);
  }
  const ret = await o.peekL(page, a7);
  await o.shellKeys(page, 'break delete 0');
  if (name === 'dissolve') await o.shellKeys(page, `w.w 1 ${hex(a4 - 0x6578)}`);
  await o.shellKeys(page, `break at ${hex(ret)}`);
  const b0 = await where(page);
  const target = TARGETS[name] === null ? null : runtime(bases, TARGETS[name]);
  await o.shellKeys(page, 'clear');
  let f1, b1;
  if (target === null) {
    await o.shellKeys(page, 'g');
  } else {
    // `g <address>` moves the PC there and stops; a plain `g` runs on.
    await o.shellKeys(page, `g ${hex(target)}`);
    await stopped(page, [target]);
    await o.shellKeys(page, 'clear');
    await o.shellKeys(page, 'g');
  }
  f1 = await stopped(page, [ret]);
  b1 = await where(page);
  await o.shellKeys(page, 'clear');
  const back = await o.shellKeys(page, 'r cpu');
  const a7b = parseInt([...back.matchAll(/A4 - A7 : (?:0x[0-9a-f]+ ){3}0x([0-9a-f]+)/g)].at(-1)[1], 16);
  if (b1.pc !== ret || a7b !== a7 + 4) {
    throw new Error(`${name}: did not come back to the render's caller: stopped at ${hex(b1.pc)} (want ${hex(ret)}), A7 ${hex(a7b)} (want ${hex(a7 + 4)})`);
  }
  const frames = f1 - f0 + (b1.v - b0.v) / 313;
  log(`${name}: stopped at 0x102c90 (D0 ${d01[1]}, D1 ${d01[2]}) frame ${f0} line ${b0.v}; back at ${hex(b1.pc)} (the caller ${hex(ret)}) frame ${f1} line ${b1.v}: ${frames.toFixed(2)} PAL frames`);
  await o.shot(page, `${name}-after`, { dir: out });
} finally {
  await o.close(h);
}

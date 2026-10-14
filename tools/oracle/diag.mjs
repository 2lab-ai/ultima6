// Boot diagnostics: boot the oracle disk set, run for N frames, then report what the machine is
// doing: frame/hash/DMA, colour histogram of the texture, RetroShell inspector output.
// usage: node diag.mjs OUTDIR FRAMES [retroshell-cmd ...]
//   env U6_SHOT_EVERY=1 saves a lores shot every 250 frames; U6_MACHINE as for boot.mjs; U6_PRE='cmd;cmd' + U6_PRE_AT=frames (RetroShell before the run); U6_DISKS='PROGRAM,POPULACE,PLAYER,PORTRAIT' (DH0..DH3; names under disks/ or absolute paths; .adf paths go to DF0..DF3)
import { appendFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import * as o from './lib.mjs';

const out = process.argv[2];
const frames = Number(process.argv[3] || 1500);
const cmds = process.argv.slice(4);
mkdirSync(out, { recursive: true });
const LOG = join(out, 'diag.log');
const log = (s) => { appendFileSync(LOG, `${new Date().toISOString()} ${s}\n`); if (!s.startsWith('[')) console.log(s); };

const h = await o.launch({ log });
const { page } = h;
try {
  if (process.env.U6_MACHINE) log(`configure ${JSON.stringify(await o.configure(page, process.env.U6_MACHINE))}`);
  const vols = (process.env.U6_DISKS || 'PROGRAM,POPULACE,PLAYER,PORTRAIT').split(',');
  const adf = vols.some((v) => v.endsWith('.adf'));
  if (adf) await o.setFloppyCount(page, 4);
  for (const [i, v] of vols.entries()) await o.insert(page, v.startsWith('/') ? v : join(o.ORACLE_HOME, 'disks', `${v}.hdf`), i);
  if (adf) await o.reset(page); // ADF inserts do not power-cycle; HDF inserts do
  // U6_PRE='cmd;cmd' = RetroShell commands after U6_PRE_AT frames (e.g. catchpoints; arm them
  // after AROS's own boot-time CPU/FPU probes, which raise vectors 4/8/11 in the ROM).
  if (process.env.U6_PRE_AT) await o.waitFrames(page, Number(process.env.U6_PRE_AT));
  let pre = '';
  for (const c of (process.env.U6_PRE || '').split(';').filter(Boolean)) {
    await o.shell(page, 'clear');
    await o.shell(page, c);
    await page.waitForTimeout(300);
    pre += `\n### ${c}\n${(await o.shell(page, '')).trim()}\n`;
  }
  if (pre) writeFileSync(join(out, 'pre.txt'), pre);
  let kicked; try { kicked = await o.ensureRunning(page); } catch (e) { kicked = `paused? ${e.message.split("\n")[0]}`; }
  log(`kicked=${kicked} machine=${JSON.stringify(await o.machine(page))}`);
  const step = 250;
  for (let f = 0; f < frames; f += step) {
    try { await o.waitFrames(page, step, { deadlineMs: 20000 }); }
    catch (e) { log(`stopped: ${e.message}`); break; }
    log(`f+${f + step} frame=${await o.frame(page)} texhash=${await o.texHash(page)} unmapped=${await o.unmapped(page)} dma=${JSON.stringify(await o.activity(page))}`);
    if (process.env.U6_SHOT_EVERY === '1') for (const w of await o.shot(page, `f${String(f + step).padStart(5, '0')}`, { dir: out, modes: ['lores'] })) log(`shot ${w.file} ${w.sha256}`);
  }
  for (const w of await o.shot(page, 'final', { dir: out, modes: ['lores', 'hires', 'tex'] })) log(`shot ${w.file} ${w.sha256}`);
  const rgba = await o.grab(page);
  const hist = new Map();
  for (let i = 0; i < rgba.length; i += 16) {
    const k = (rgba[i] << 16) | (rgba[i + 1] << 8) | rgba[i + 2];
    hist.set(k, (hist.get(k) || 0) + 1);
  }
  log(`colours ${hist.size}: ${[...hist.entries()].sort((a, b) => b[1] - a[1]).slice(0, 12).map(([k, n]) => `#${k.toString(16).padStart(6, '0')}x${n}`).join(' ')}`);
  let text = '';
  for (const c of cmds) text += `\n### ${c}\n${(await o.shellKeys(page, c)).trim()}\n`;
  if (text) { writeFileSync(join(out, 'retroshell.txt'), text); log(`retroshell -> ${join(out, 'retroshell.txt')}`); }
} finally {
  await o.close(h);
}

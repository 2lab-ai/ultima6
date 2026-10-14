// Boot the built oracle disk set (PROGRAM/POPULACE/PLAYER/PORTRAIT on DH0-DH3) and capture.
// usage: node boot.mjs OUTDIR [step ...]
//   step = wait:FRAMES | shot:NAME | key:KEYNAME | type:TEXT | click | stable:FRAMES | status
//   env U6_MACHINE='AGNUS_REVISION=OCS,CHIP_RAM=512,SLOW_RAM=512,FAST_RAM=0' sets the machine first
//   default steps: wait:500 shot:t10 wait:1000 shot:t30 wait:1500 shot:t60 status
import { appendFileSync } from 'node:fs';
import { join } from 'node:path';
import * as o from './lib.mjs';

const out = process.argv[2] || join(o.SHOTS_DIR, `boot-${Date.now()}`);
const steps = process.argv.length > 3 ? process.argv.slice(3) : ['wait:500', 'shot:t10', 'wait:1000', 'shot:t30', 'wait:1500', 'shot:t60', 'status'];
const DISKS = join(o.ORACLE_HOME, 'disks');
const LOG = join(o.ORACLE_HOME, 'boot.log');
const log = (s) => { appendFileSync(LOG, `${new Date().toISOString()} ${s}\n`); if (!s.startsWith('[')) console.log(s); };

const h = await o.launch({ log });
const { page } = h;
try {
  if (process.env.U6_MACHINE) log(`configure ${process.env.U6_MACHINE} -> ${JSON.stringify(await o.configure(page, process.env.U6_MACHINE))}`);
  let n = 0;
  for (const vol of ['PROGRAM', 'POPULACE', 'PLAYER', 'PORTRAIT']) await o.insert(page, join(DISKS, `${vol}.hdf`), n++);
  log(`kicked=${await o.ensureRunning(page)}`);
  log(`booted machine=${JSON.stringify(await o.machine(page))}`);
  for (const s of steps) {
    const [op, ...rest] = s.split(':');
    const arg = rest.join(':');
    if (op === 'wait') await o.waitFrames(page, Number(arg));
    else if (op === 'stable') log(`stable ${JSON.stringify(await o.waitStable(page, { frames: Number(arg) }))}`);
    else if (op === 'key') await o.tap(page, arg);
    else if (op === 'type') await o.type(page, arg.replace(/\\n/g, '\n'));
    else if (op === 'click') await o.click(page, Number(arg || 1));
    else if (op === 'shot') for (const w of await o.shot(page, arg, { dir: out, modes: ['lores', 'hires'] })) log(`shot ${w.file} ${w.sha256}`);
    else if (op === 'status') log(`status frame=${await o.frame(page)} texhash=${await o.texHash(page)} unmapped=${await o.unmapped(page)} activity=${JSON.stringify(await o.activity(page))}`);
    else throw new Error(`unknown step ${s}`);
  }
} finally {
  await o.close(h);
}

// Build the oracle's disk set from the original ADFs, using the game's own installer under AROS:
//   1. blank 16 MB FFS HDF on DH0, disk1/2/3 in DF0/1/2, boot -> installu6 (disk1 startup-sequence)
//   2. answer  h <RET>  DH0:ULTIMA6 <RET>  y <RET>  and wait for the installer window to close
//   3. export DH0 -> disks/u6-installed.hdf, extract its tree -> installed/ (amifs.py)
//   4. four volume HDFs PROGRAM/POPULACE/PLAYER/PORTRAIT, each = installed ULTIMA6/* + ultcli;
//      PROGRAM boots with s/startup-sequence "ultcli". (AROS's boot shell has no Assign, so the
//      game's PROGRAM:/PORTRAIT:/... paths are satisfied by volume names instead.)
// usage: node build.mjs        (outputs under $U6_ORACLE_HOME)
import { appendFileSync } from 'node:fs';
import { rm, mkdir, readFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as o from './lib.mjs';
import { DATA_ROOT } from '../data-paths.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const GAMES = process.env.U6_GAMES || DATA_ROOT;
const IMG = join(GAMES, 'img');
const DISKS = join(o.ORACLE_HOME, 'disks');
const INSTALLED = join(o.ORACLE_HOME, 'installed');
const SHOTS = join(o.SHOTS_DIR, 'build');
const LOG = join(o.ORACLE_HOME, 'build.log');
const log = (s) => { appendFileSync(LOG, `${new Date().toISOString()} ${s}\n`); if (!s.startsWith('[')) console.log(s); };

// Texture hash of the empty AROS "Workbench Screen" left when the installer's window closes
// (observed identical in independent runs, 2026-10-14).
const WB_EMPTY = 34535989;

const h = await o.launch({ log });
const { page } = h;
try {
  await mkdir(DISKS, { recursive: true });
  log(`blank ${JSON.stringify(await o.makeHdf(page, join(DISKS, 'u6-blank.hdf'), { mb: 16, name: 'U6HD' }))}`);
  await o.insert(page, join(DISKS, 'u6-blank.hdf'), 0); // HDF insert power-cycles the machine
  await o.setFloppyCount(page, 3);
  for (const d of [0, 1, 2]) await o.insert(page, join(IMG, `disk${d + 1}.adf`), d);
  await o.reset(page);
  log(`machine ${JSON.stringify(await o.machine(page))}`);

  await o.waitFrames(page, 1000);
  log(`prompt ${JSON.stringify(await o.waitStable(page, { frames: 250, timeoutFrames: 6000 }))}`);
  await o.shot(page, '01-hd-or-floppy', { dir: SHOTS, modes: ['hires'] });
  await o.type(page, 'h\n');
  await o.waitStable(page, { frames: 150, timeoutFrames: 3000 });
  await o.shot(page, '02-where', { dir: SHOTS, modes: ['hires'] });
  await o.type(page, 'DH0:ULTIMA6\n');
  await o.waitStable(page, { frames: 150, timeoutFrames: 3000 });
  await o.shot(page, '03-create', { dir: SHOTS, modes: ['hires'] });
  await o.type(page, 'y\n');
  log(`install done ${JSON.stringify(await o.waitHash(page, WB_EMPTY, { timeoutFrames: 50 * 1200 }))}`);

  log(`installed ${JSON.stringify(await o.exportHd(page, 0, join(DISKS, 'u6-installed.hdf')))}`);
  await rm(INSTALLED, { recursive: true, force: true });
  execFileSync('python3', ['-I', join(HERE, 'amifs.py'), join(DISKS, 'u6-installed.hdf'), 'x', INSTALLED]);

  // ultcli is not installed by the HD path; decrunch it from disk1 with the repo's PP20 tool.
  const ultcli = join(o.ORACLE_HOME, 'extra', 'ultcli');
  await mkdir(dirname(ultcli), { recursive: true });
  execFileSync('python3', ['-I', join(HERE, '..', 'pp20.py'), join(GAMES, 'files', 'd1', 'INSTALL', 'ultcli'), ultcli]);

  const base = await o.filesFromDir(join(INSTALLED, 'ULTIMA6'));
  for (const vol of ['PROGRAM', 'POPULACE', 'PLAYER', 'PORTRAIT']) {
    const files = { ...base, ultcli: await readFile(ultcli) };
    if (vol === 'PROGRAM') files['s/startup-sequence'] = 'ultcli\n';
    log(`${vol} ${JSON.stringify(await o.makeHdf(page, join(DISKS, `${vol}.hdf`), { mb: 8, name: vol, files }))}`);
  }
} finally {
  await o.close(h);
}

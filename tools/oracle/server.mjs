// Oracle control server: owns one headless vAmigaWeb instance and exposes it on
// http://127.0.0.1:$U6_ORACLE_PORT (default 8765). Drive it with ctl.sh or curl.
//
//   GET /status                         frame, machine config, ROM, drives with disks
//   GET /drives?n=4                     connect DF0..DF(n-1)
//   GET /insert?drive=0&path=/abs.adf   insert ADF into DFn
//   GET /eject?drive=0
//   GET /export?drive=0&path=/abs.adf   write DFn back out as ADF
//   GET /reset
//   GET /key?k=RETURN[&hold=4][&mods=LSHIFT,CTRL]   tap one key (KEY name or raw code)
//   GET /down?k=..  /up?k=..            hold / release
//   GET /type?text=...                  type text (\n = RETURN), US keymap
//   GET /wait?frames=N                  wait N emulated frames
//   GET /stable?frames=150[&timeout=30000]   wait until picture unchanged for N frames
//   GET /warp?on=1|0
//   GET /mouse?dx=10&dy=-5               relative mouse move (port 1)
//   GET /click?button=1[&hold=4]         mouse button 1 left, 2 middle, 3 right
//   GET /until?hash=N[&timeout=frames]   wait for an exact picture hash (see /status texhash)
//   GET /shot?name=x[&modes=lores,hires,tex,page]   capture into $U6_SHOTS_DIR
//   GET /shell?cmd=...                  vAmiga RetroShell command, returns console text
//   GET /mkhdf?path=/abs.hdf[&mb=10][&name=U6HD][&src=/hostdir][&add=rel=/hostfile]...[&startup=line\\nline]
//                                       build FFS HDF from a host dir + extra files (+ s/startup-sequence)
//   GET /exporthd?hd=0&path=/abs.hdf    write DHn back out as HDF
//   GET /tree?hd=0&dir=/abs/dir         copy DHn's files to a host directory
//   GET /quit
import { createServer } from 'node:http';
import { appendFileSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import * as o from './lib.mjs';

const PORT = Number(process.env.U6_ORACLE_PORT || 8765);
const LOG = process.env.U6_ORACLE_LOG || join(o.ORACLE_HOME, 'server.log');
const log = (s) => appendFileSync(LOG, `${new Date().toISOString()} ${s}\n`);

const h = await o.launch({ log, headless: process.env.U6_HEADFUL !== '1' });
const { page } = h;
log(`ready rom=${JSON.stringify(await o.romInfo(page))}`);

const num = (v, d) => (v === null || v === undefined || v === '' ? d : Number(v));
const keyOf = (k) => (/^(0x[0-9a-f]+|\d\d+)$/i.test(k) ? Number(k) : k);

let queue = Promise.resolve();
const handlers = {
  async status() {
    const disks = [];
    for (let d = 0; d < 4; d++) disks.push(await o.hasDisk(page, d));
    return { texhash: await o.texHash(page), frame: await o.frame(page), running: await o.running(page), activity: await o.activity(page), machine: await o.machine(page), rom: await o.romInfo(page), disks };
  },
  drives: (q) => o.setFloppyCount(page, num(q.get('n'), 4)),
  insert: (q) => o.insert(page, q.get('path'), num(q.get('drive'), 0)),
  eject: (q) => o.eject(page, num(q.get('drive'), 0)),
  export: (q) => o.exportDisk(page, num(q.get('drive'), 0), q.get('path')),
  reset: () => o.reset(page),
  key: (q) => o.tap(page, keyOf(q.get('k')), { hold: num(q.get('hold'), 4), gap: num(q.get('gap'), 4), mods: q.get('mods') ? q.get('mods').split(',') : [] }),
  down: (q) => page.evaluate((c) => wasm_key(c, 1), typeof keyOf(q.get('k')) === 'number' ? keyOf(q.get('k')) : o.KEY[q.get('k')]),
  up: (q) => page.evaluate((c) => wasm_key(c, 0), typeof keyOf(q.get('k')) === 'number' ? keyOf(q.get('k')) : o.KEY[q.get('k')]),
  type: (q) => o.type(page, q.get('text'), { hold: num(q.get('hold'), 4), gap: num(q.get('gap'), 4) }),
  wait: (q) => o.waitFrames(page, num(q.get('frames'), 50)),
  stable: (q) => o.waitStable(page, { frames: num(q.get('frames'), 150), timeoutFrames: num(q.get('timeout'), 30000) }),
  warp: (q) => o.setWarp(page, q.get('on') === '1'),
  mouse: (q) => o.mouseMove(page, num(q.get('dx'), 0), num(q.get('dy'), 0)),
  click: (q) => o.click(page, num(q.get('button'), 1), { hold: num(q.get('hold'), 4) }),
  until: (q) => o.waitHash(page, num(q.get('hash')), { timeoutFrames: num(q.get('timeout'), 60000) }),
  shot: (q) => o.shot(page, q.get('name') || `f${Date.now()}`, { modes: (q.get('modes') || 'lores').split(',') }),
  shell: (q) => o.shell(page, q.get('cmd')),
  async mkhdf(q) {
    const files = q.get('src') ? await o.filesFromDir(q.get('src')) : {};
    for (const extra of q.getAll('add')) { const [rel, host] = extra.split('='); files[rel] = await readFile(host); }
    if (q.get('startup')) files['s/startup-sequence'] = q.get('startup').replace(/\\n/g, '\n') + '\n';
    return o.makeHdf(page, q.get('path'), { mb: num(q.get('mb'), 10), name: q.get('name') || 'U6HD', files });
  },
  exporthd: (q) => o.exportHd(page, num(q.get('hd'), 0), q.get('path')),
  tree: (q) => o.exportTree(page, num(q.get('hd'), 0), q.get('dir')),
};

const server = createServer((req, res) => {
  const url = new URL(req.url, 'http://x');
  const cmd = url.pathname.slice(1);
  if (cmd === 'quit') {
    res.end('{"ok":true}\n');
    log('quit');
    server.close();
    o.close(h).then(() => process.exit(0));
    return;
  }
  const fn = handlers[cmd];
  if (!fn) { res.writeHead(404); res.end(`{"error":"unknown command ${cmd}"}\n`); return; }
  // Serialize: one emulator, one command at a time.
  queue = queue.then(async () => {
    try {
      const result = await fn(url.searchParams);
      log(`${cmd} ${url.search} -> ok`);
      res.end(JSON.stringify({ ok: true, result: result ?? null }) + '\n');
    } catch (e) {
      log(`${cmd} ${url.search} -> ${e.message}`);
      res.writeHead(500);
      res.end(JSON.stringify({ ok: false, error: e.message }) + '\n');
    }
  });
});
server.listen(PORT, '127.0.0.1', () => log(`listening 127.0.0.1:${PORT}`));

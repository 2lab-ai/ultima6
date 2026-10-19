// The memory of the page in headless Chromium before a drop of the user's archive.org zip: JS
// heap and ArrayBuffer backing stores (CDP Runtime.getHeapUsage), the wasm memory and the
// renderer's resident size (ps); then the drop, timed to its status, and whether the game
// started. Output rule (spec 01 §Rights): only measures taken before the user's files are
// imported, the tool's own timing and pass/fail; what the import takes (its memory rises with
// the size of his files) and the page's status text are not printed. Optionally a full-page
// screenshot after the drop.
//
//   node tools/web/measure-memory.mjs [--root site] [--shot out.png]
import { execFile } from 'node:child_process';
import { writeFile } from 'node:fs/promises';
import { promisify } from 'node:util';
import * as h from './harness.mjs';

const run = promisify(execFile);
const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : fallback;
};
const MiB = (n) => Math.round((n / 2 ** 20) * 10) / 10;

if (!h.haveZip()) {
  console.log(`skip: ${h.ZIP} not present`);
  process.exit(0);
}
const env = await h.setup(arg('root', h.SITE));
try {
  const { context, page } = await h.openPage(env);
  await page.goto(env.server.url);
  const done = () => page.evaluate(() => /Started\./.test(document.getElementById('status').textContent));
  await page.waitForFunction(() => globalThis.__u6mem?.() > 0 && !/Loading engine/.test(document.getElementById('status').textContent));
  const cdp = await context.newCDPSession(page);
  const info = await (await env.browser.newBrowserCDPSession()).send('SystemInfo.getProcessInfo');
  const renderers = info.processInfo.filter((p) => p.type === 'renderer').map((p) => p.id);
  const rss = async () => {
    let max = 0;
    for (const pid of renderers) {
      try {
        max = Math.max(max, Number((await run('ps', ['-o', 'rss=', '-p', String(pid)])).stdout.trim()) * 1024);
      } catch {
        // gone
      }
    }
    return max;
  };
  await cdp.send('HeapProfiler.collectGarbage');
  const base = { heap: (await cdp.send('Runtime.getHeapUsage')), rss: await rss(), wasm: await page.evaluate(() => __u6mem()) };
  const t0 = Date.now();
  await h.dropFiles(page, [h.ZIP]);
  await page.waitForFunction(() => /Started\.|failed|Could not/.test(document.getElementById('status').textContent), null, { timeout: 120_000 });
  const ms = Date.now() - t0;
  const ok = await done();
  const result = {
    root: arg('root', 'site'),
    before_the_drop_MiB: {
      wasm_memory: MiB(base.wasm),
      js_heap_used: MiB(base.heap.usedSize),
      array_buffer_backing: MiB(base.heap.backingStorageSize ?? 0),
      renderer_rss: MiB(base.rss),
    },
    import_ms: ms,
    started: ok ? 'pass' : 'fail',
  };
  console.log(JSON.stringify(result, null, 2));
  const shot = arg('shot');
  if (shot) {
    await h.sleep(2000);
    await writeFile(shot, await page.screenshot({ fullPage: true }));
  }
} finally {
  await env.close();
}

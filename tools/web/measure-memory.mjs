// Peak memory of a drop of the user's archive.org zip in headless Chromium: JS heap and
// ArrayBuffer backing stores (CDP Runtime.getHeapUsage, sampled while the import runs), the
// wasm memory (its size never shrinks, so the size after the start is the peak), and the
// renderer's resident size (ps, sampled). Optionally a full-page screenshot after the drop.
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
  const peak = { heap: base.heap.usedSize, backing: base.heap.backingStorageSize ?? 0, rss: base.rss };
  let sampling = true;
  const sampler = (async () => {
    while (sampling) {
      try {
        const u = await cdp.send('Runtime.getHeapUsage');
        peak.heap = Math.max(peak.heap, u.usedSize);
        peak.backing = Math.max(peak.backing, u.backingStorageSize ?? 0);
      } catch {
        // page busy or gone
      }
      peak.rss = Math.max(peak.rss, await rss());
      await h.sleep(20);
    }
  })();
  const t0 = Date.now();
  await h.dropFiles(page, [h.ZIP]);
  await page.waitForFunction(() => /Started\.|failed|Could not/.test(document.getElementById('status').textContent), null, { timeout: 120_000 });
  await h.sleep(300);
  sampling = false;
  await sampler;
  const ms = Date.now() - t0;
  const ok = await done();
  const wasm = await page.evaluate(() => __u6mem());
  await cdp.send('HeapProfiler.collectGarbage');
  const after = await cdp.send('Runtime.getHeapUsage');
  const result = {
    root: arg('root', 'site'),
    started: ok,
    import_ms: ms,
    wasm_memory_MiB: { before: MiB(base.wasm), after_start: MiB(wasm) },
    js_heap_used_MiB: { before: MiB(base.heap.usedSize), peak: MiB(peak.heap), after_gc: MiB(after.usedSize) },
    array_buffer_backing_MiB: { before: MiB(base.heap.backingStorageSize ?? 0), peak: MiB(peak.backing), after_gc: MiB(after.backingStorageSize ?? 0) },
    renderer_rss_MiB: { before: MiB(base.rss), peak: MiB(peak.rss) },
    status: (await page.textContent('#status')).split('\n').slice(-3).join(' | '),
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

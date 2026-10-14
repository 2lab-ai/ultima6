// Shared pieces of the page tests: the local server, a page with test gates, file drops, the
// IndexedDB view, frame comparisons. Game files are read from the user's local copy at run
// time and never written anywhere.
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { serve } from './serve.mjs';

export const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
export const SITE = join(REPO, 'site');
export const ORIGINAL = '/Users/zhugehyuk/Games/ultima6/original';
export const ZIP = join(ORIGINAL, 'ultima-vi.zip');
export const HFES = [1, 2, 3].map((n) => join(ORIGINAL, `Ultima VI Disk ${n}.hfe`));
export const haveHfes = () => HFES.every((p) => existsSync(p));
export const haveZip = () => existsSync(ZIP);

// Runs in the page before its scripts: named gates the test holds and releases, an event log,
// CSP violations, the default-prevented state of every key, file-read hooks.
export function pageHooks() {
  const holds = new Set();
  const waiting = new Map();
  const log = [];
  window.__log = log;
  window.__csp = [];
  window.__keys = [];
  window.__fail = new Set();
  window.__hold = (name) => holds.add(name);
  window.__release = (name) => {
    holds.delete(name);
    for (const go of waiting.get(name) ?? []) go();
    waiting.delete(name);
  };
  window.__gate = async (name) => {
    if (!holds.has(name)) return;
    await new Promise((go) => waiting.set(name, [...(waiting.get(name) ?? []), go]));
  };
  try {
    for (const name of JSON.parse(sessionStorage.getItem('__holds') ?? '[]')) holds.add(name);
    sessionStorage.removeItem('__holds');
  } catch {
    // An opaque origin (about:blank) has no session storage.
  }
  document.addEventListener('securitypolicyviolation', (e) => {
    window.__csp.push(`${e.violatedDirective} ${e.blockedURI}`);
  });
  window.addEventListener('keydown', (e) => window.__keys.push([e.code, e.defaultPrevented]));
  // A file named window.__bigName claims 600 MiB (the size check runs before any read).
  const size = Object.getOwnPropertyDescriptor(Blob.prototype, 'size').get;
  Object.defineProperty(Blob.prototype, 'size', {
    configurable: true,
    get() {
      return window.__bigName && this.name === window.__bigName ? 600 * 2 ** 20 : size.call(this);
    },
  });
  const read = Blob.prototype.arrayBuffer;
  Blob.prototype.arrayBuffer = async function arrayBuffer() {
    log.push(`read:start:${this.name}`);
    await window.__gate('read');
    if (window.__fail.has('read')) throw new DOMException('test: unreadable', 'NotReadableError');
    const bytes = await read.call(this);
    log.push(`read:end:${this.name}`);
    return bytes;
  };
}

// site/store.js behind gates: each call logs, waits for its gate, then runs the real one.
const STORE_WRAPPER = `
import * as real from './store.js?real';
export const collect = real.collect;
const log = (e) => window.__log.push(e);
export async function saveFiles(files) {
  log('save:start'); await window.__gate('save');
  const kept = await real.saveFiles(files); log('save:end:' + kept); return kept;
}
export async function loadFiles() {
  log('load:start'); await window.__gate('load');
  const files = await real.loadFiles(); log('load:end:' + (files ? files.length : 0)); return files;
}
export async function forgetFiles() {
  log('forget:start'); await window.__gate('forget');
  if (window.__fail.has('forget')) { log('forget:fail'); throw new Error('test: storage refused the delete'); }
  await real.forgetFiles(); log('forget:end');
}
export async function storeSaves(files, replace) {
  log('saves:store:start:' + files.length); await window.__gate('saves');
  await real.storeSaves(files, replace); log('saves:store:end');
}
export async function loadSaves() {
  const files = await real.loadSaves(); log('saves:load:' + files.length); return files;
}
export async function forgetSaves() {
  log('saves:forget:start'); await real.forgetSaves(); log('saves:forget:end');
}
`;

// Appended to the wasm-bindgen glue (module scope): the wasm memory size, a trap switch.
const GLUE_HOOKS = `
globalThis.__u6mem = () => (wasm ? wasm.memory.buffer.byteLength : 0);
globalThis.__ticks = 0;
{
  const tick = Game.prototype.tick;
  Game.prototype.tick = function (ms) {
    globalThis.__ticks += 1;
    if (globalThis.__trap) throw new WebAssembly.RuntimeError('unreachable (test trap)');
    return tick.call(this, ms);
  };
}
`;

/// Starts the server and a headless Chromium; `close()` stops both.
export async function setup(root = SITE) {
  const server = await serve({ root });
  const browser = await chromium.launch();
  return { server, browser, close: async () => { await browser.close(); await server.close(); } };
}

/// A fresh context (empty storage) and page with the hooks; `gated` adds the store and glue
/// wrappers (the page itself is unchanged).
export async function openPage(env, { gated = true } = {}) {
  const context = await env.browser.newContext({ viewport: { width: 1024, height: 900 } });
  await context.addInitScript(pageHooks);
  if (gated) {
    await context.route(/\/store\.js(\?real)?$/, async (route) => {
      if (route.request().url().endsWith('?real')) return route.continue();
      const response = await route.fetch();
      await route.fulfill({ response, body: STORE_WRAPPER });
    });
    await context.route(/\/pkg\/u6_web\.js$/, async (route) => {
      const response = await route.fetch();
      await route.fulfill({ response, body: (await response.text()) + GLUE_HOOKS });
    });
  }
  const page = await context.newPage();
  page.on('pageerror', (err) => console.error(`  page error: ${err.message}`));
  return { context, page };
}

export async function load(page, env) {
  const response = await page.goto(env.server.url);
  await waitSettled(page);
  return response;
}

/// Waits until no operation is queued (the restore at load included).
export async function waitSettled(page, timeout = 60_000) {
  await page.waitForFunction(() => document.body.dataset.ops === '0', null, { timeout });
}

/// Drops `paths` (local files) plus `extra` ({ name, text } or { name, bytes: number[] }) on the
/// drop zone, as a user drop.
export async function dropFiles(page, paths, extra = []) {
  await page.evaluate(() => {
    if (document.getElementById('__t')) return;
    const input = Object.assign(document.createElement('input'), { type: 'file', id: '__t', multiple: true });
    input.style.display = 'none';
    document.body.append(input);
  });
  await page.setInputFiles('#__t', paths);
  await page.evaluate((extra) => {
    const dt = new DataTransfer();
    for (const file of document.getElementById('__t').files) dt.items.add(file);
    for (const { name, text, bytes } of extra) {
      dt.items.add(new File([bytes ? new Uint8Array(bytes) : text], name));
    }
    document.getElementById('drop').dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }));
  }, extra);
}

/// The paths of the stored save files (sorted), [] when there are none.
export function savedPaths(page) {
  return page.evaluate(() => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('files');
    req.onerror = () => fail(req.error);
    req.onsuccess = () => {
      const db = req.result;
      const get = db.transaction('files').objectStore('files')
        .getAll(IDBKeyRange.bound('save:', 'save:\uffff'));
      get.onsuccess = () => { db.close(); done(get.result.map((f) => f.path).sort()); };
      get.onerror = () => fail(get.error);
    };
  }));
}

/// The paths kept in IndexedDB, or null.
export function keptPaths(page) {
  return page.evaluate(() => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('files');
    req.onerror = () => fail(req.error);
    req.onsuccess = () => {
      const db = req.result;
      const get = db.transaction('files').objectStore('files').get('extracted');
      get.onsuccess = () => { db.close(); done(get.result ? get.result.map((f) => f.path) : null); };
      get.onerror = () => fail(get.error);
    };
  }));
}

export const state = (page) => page.evaluate(() => ({
  state: document.body.dataset.state,
  ops: Number(document.body.dataset.ops),
  files: Number(document.getElementById('screen').dataset.files),
  status: document.getElementById('status').textContent,
  focused: document.activeElement?.id ?? document.activeElement?.tagName,
  pickerDisabled: document.getElementById('picker').disabled,
  restartHidden: document.getElementById('restart').hidden,
  saves: Number(document.getElementById('screen').dataset.saves),
  leader: document.getElementById('screen').dataset.leader,
  saved: Number(document.body.dataset.saved ?? 0),
  audio: document.body.dataset.audio,
  audioChunks: Number(document.body.dataset.audioChunks ?? 0),
}));

export const log = (page) => page.evaluate(() => [...window.__log]);

export async function waitLog(page, entry, timeout = 30_000) {
  await page.waitForFunction((e) => window.__log.some((l) => l.startsWith(e)), entry, { timeout });
}

/// The canvas pixels (RGBA) at backing-store size.
export function frame(page) {
  return page.evaluate(() => {
    const c = document.getElementById('screen');
    return Array.from(c.getContext('2d').getImageData(0, 0, c.width, c.height).data);
  });
}

/// The horizontal shift (in pixels, multiples of 16) that best maps `before` onto `after` in
/// the map window (11 x 11 tiles from (8, 8)), and how well each shift matches.
export function mapShift(before, after, width = 320) {
  const scores = {};
  for (let s = -64; s <= 64; s += 16) {
    let same = 0;
    let n = 0;
    for (let y = 8; y < 8 + 176; y += 2) {
      for (let x = 8 + 64; x < 8 + 176 - 64; x += 2) {
        const a = (y * width + x + s) * 4;
        const b = (y * width + x) * 4;
        n += 1;
        if (before[a] === after[b] && before[a + 1] === after[b + 1] && before[a + 2] === after[b + 2]) same += 1;
      }
    }
    scores[s] = same / n;
  }
  const best = Object.entries(scores).sort((p, q) => q[1] - p[1])[0];
  return { shift: Number(best[0]), score: best[1], still: scores[0] };
}

export const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

// Shared pieces of the page tests: the local server, a page with test gates, file drops, the
// IndexedDB view, frame comparisons. Game files are read from the user's local copy at run
// time and never written anywhere.
import assert from 'node:assert/strict';
import { existsSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';
import { chromium } from 'playwright';
import { serve } from './serve.mjs';

export const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
export const SITE = join(REPO, 'site');
export const ORIGINAL = '/Users/zhugehyuk/Games/ultima6/original';
export const ZIP = join(ORIGINAL, 'ultima-vi.zip');
export const HFES = [1, 2, 3].map((n) => join(ORIGINAL, `Ultima VI Disk ${n}.hfe`));
export const haveHfes = () => HFES.every((p) => existsSync(p));
export const haveZip = () => existsSync(ZIP);
// The user's disks as tools/pp20.py left them: one directory per volume under d1..d3.
export const UNPACKED = join(ORIGINAL, '..', 'unpacked');
const SIDE_CARS = ['.blkdev', '.bootcode', '.xdfmeta'];

/// How many game files an intake of the user's disks keeps: the files of the decrunched volumes
/// (unpacked/d1..d3, xdftool's side-car files left out), counted at run time like the Rust
/// intake test does; `undefined` without that copy (the count is the user's data, not ours), or
/// with `U6_TEST_NO_UNPACKED` set (to run the tests' run-time baseline path).
export function gameFileCount() {
  if (process.env.U6_TEST_NO_UNPACKED || !existsSync(UNPACKED)) return undefined;
  const count = (dir) => readdirSync(dir, { withFileTypes: true }).reduce((n, e) => {
    if (e.isDirectory()) return n + count(join(dir, e.name));
    return n + (SIDE_CARS.some((x) => e.name.endsWith(x)) ? 0 : 1);
  }, 0);
  return ['d1', 'd2', 'd3'].reduce((n, d) => n + count(join(UNPACKED, d)), 0);
}

/// The objlist and the 69 block files a whole save holds (u6-core `objects::BLOCK_COUNT`).
export const WHOLE_SAVE = 1 + 69;

/// The save path of surface block `i` (`savegame/objblk` + column + row letter, as u6-core
/// `saveload::block_file`).
export const blockFile = (i) => `savegame/objblk${'abcdefgh'[i & 7]}${'abcdefgh'[i >> 3]}`;

/// Deep equality for values that hold content of the user's files (save bytes, names): when
/// they differ the error names `label` and, per differing path (object keys are file paths),
/// whether the lengths differ and the first differing index, never the values nor the lengths
/// (spec 01 §Rights: the length of a list of the user's records or paths is a count of them;
/// assert's own message would print both). A prefix is named without an index (it would be the
/// shorter length), equal elements in sequences of two kinds as such. Strings, arrays and typed
/// arrays count as sequences.
export function assertSame(actual, expected, label) {
  if (isDeepStrictEqual(actual, expected)) return;
  const found = [];
  difference(actual, expected, '', found);
  assert.fail(`${label}: ${found.slice(0, 8).join('; ')}`);
}

const isSequence = (v) => typeof v === 'string' || Array.isArray(v) || ArrayBuffer.isView(v);
const isRecord = (v) => typeof v === 'object' && v !== null && !isSequence(v);
const kind = (v) => {
  if (v === null || v === undefined) return String(v);
  if (isSequence(v)) return typeof v === 'string' ? 'a string' : 'a sequence';
  return isRecord(v) ? 'an object' : `a ${typeof v}`;
};

/// Where `a` and `b` differ, as content-free entries pushed to `out`.
function difference(a, b, path, out) {
  const at = path ? `${path}: ` : '';
  if (isSequence(a) && isSequence(b) && typeof a === typeof b) {
    const n = Math.min(a.length, b.length);
    let i = 0;
    while (i < n && isDeepStrictEqual(a[i], b[i])) i += 1;
    const nested = i < n && typeof a !== 'string' && (isRecord(a[i]) || isSequence(a[i]))
      && (isRecord(b[i]) || isSequence(b[i]));
    if (nested) difference(a[i], b[i], `${path}[${i}]`, out);
    else if (i < n) out.push(`${at}lengths ${a.length === b.length ? 'equal' : 'differ'}, first difference at index ${i}`);
    else if (a.length !== b.length) out.push(`${at}lengths differ, one is a prefix of the other`);
    else out.push(`${at}the same elements, different kinds`);
  } else if (isRecord(a) && isRecord(b)) {
    for (const k of [...new Set([...Object.keys(a), ...Object.keys(b)])].sort()) {
      const p = path ? `${path}.${k}` : k;
      if (!(k in a)) out.push(`${p}: missing`);
      else if (!(k in b)) out.push(`${p}: not expected`);
      else if (!isDeepStrictEqual(a[k], b[k])) difference(a[k], b[k], p, out);
    }
  } else {
    out.push(`${at}${kind(a)} and ${kind(b)}`);
  }
}

// The error classes a failure is shown by; any other name (a page can set its own) shows as
// `Error`.
const ERROR_NAMES = new Set(['Error', 'TypeError', 'RangeError', 'ReferenceError', 'SyntaxError',
  'TimeoutError', 'AbortError', 'RuntimeError', 'AssertionError']);
const HERE = join(dirname(fileURLToPath(import.meta.url)), '/');
// The files whose places a failure shows: the runner's own (not every path under tools/web).
const RUNNER_FILES = new Set(['test-web.mjs', 'harness.mjs']);
// A stack frame's place: node writes a module's as a file URL, Playwright's rewritten stacks a
// plain path.
const FRAME = /^\s+at (?:async )?(?:[^()]*\()?((?:file:\/\/)?\/[^\s()]+):(\d+):(\d+)\)?$/;
const framePath = (place) => {
  try { return place.startsWith('file:') ? fileURLToPath(place) : place; } catch { return ''; }
};

/// The kind of `err` as one of the fixed names above (`Error` for a thrown value that is not
/// an Error).
export const errorName = (err) => (err instanceof Error && ERROR_NAMES.has(err.name) ? err.name : 'Error');

/// What the runner shows of a failure (spec 01 §Rights): an error's message can carry the
/// user's data (an assertion's compared values; a page, Playwright or helper error quoting the
/// page's text, a selector or a status line), so the lines are a fixed head and the places in
/// these test files only. The head is an assertion's own label (the first line of the test's
/// message: node appends the compared values after it) or, for any other error, its kind. The
/// places are read from an Error's stack after its message (whose lines could look like
/// frames; a stack that does not hold the message shows none) and shown as file:line:column of
/// the runner's own files only. A thrown value that is not an Error shows no places: its
/// `stack`, if any, is anyone's text.
export function failureLines(err) {
  let head = `${errorName(err)} (its message is not shown)`;
  if (err instanceof assert.AssertionError) {
    head = err.generatedMessage
      ? 'AssertionError (the compared values are not shown)'
      : `AssertionError: ${String(err.message).split('\n')[0]}`;
  }
  let stack = err instanceof Error && typeof err.stack === 'string' ? err.stack : '';
  const message = err instanceof Error && typeof err.message === 'string' ? err.message : '';
  if (message) {
    const end = stack.indexOf(message);
    stack = end >= 0 ? stack.slice(end + message.length) : '';
  }
  const places = [];
  for (const line of stack.split('\n')) {
    const m = FRAME.exec(line);
    const path = m ? framePath(m[1]) : '';
    const file = path.startsWith(HERE) ? path.slice(HERE.length) : '';
    if (RUNNER_FILES.has(file) && places.length < 5) places.push(`at ${file}:${m[2]}:${m[3]}`);
  }
  return [head, ...places];
}

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
  log('saves:forget:start'); await window.__gate('saves-forget');
  await real.forgetSaves(); log('saves:forget:end');
}
`;

// Appended to the wasm-bindgen glue (module scope): the wasm memory size, a trap switch, the
// steps run (`__ticks`: the helpers' ticks are the page's fixed steps of 20 ms), the save
// generation of the game stepped last (whether or not the page stores its Saves), the engine
// clock and the button changes the game took on it, the keys the game takes itself
// (`__owned`, keys_owned after the step), the keys the page sent it (`__sent`, their DOM `key`)
// and, after each frame's report (`ran`), the game's speed (`__speed`, 0 = Max) and its lost
// time (`__dropped`, `__clamped`). The clock (`__clock`) is the ms of every step of a started
// game summed: the engine adds a step's ms before the game runs in it, so an event queued
// before a step reaches the game at the clock after that step (as the Rust tests' press:
// `now_ms()` after the tick that took it). `__buttons` holds [buttons, clock] for each change
// of the buttons the page queued, at the clock of the step that took it: a press's delivered
// hold is its release's clock less its own, whatever the wall time between them. `__presents`
// counts the page's presents, `__converted` those that converted a new frame (frame_version
// moved: spec 06 O1); `__shownGame` is the game that presented last. `__pointerAt` is the last
// game pixel the page queued for the pointer, `__presentMs` the wall ms the presents took.
const GLUE_HOOKS = `
globalThis.__u6mem = () => (wasm ? wasm.memory.buffer.byteLength : 0);
globalThis.__ticks = 0;
globalThis.__clock = 0;
globalThis.__buttons = [];
globalThis.__sent = [];
globalThis.__presents = 0;
globalThis.__converted = 0;
globalThis.__pointerAt = null;
globalThis.__presentMs = 0;
{
  const step = Game.prototype.step;
  const ran = Game.prototype.ran;
  const present = Game.prototype.present;
  const pointer = Game.prototype.pointer;
  const keyDown = Game.prototype.key_down;
  let queued = [];
  let held = 0;
  Game.prototype.pointer = function (x, y, buttons) {
    if (this.started()) queued.push(buttons & 3);
    globalThis.__pointerAt = [x, y];
    return pointer.call(this, x, y, buttons);
  };
  Game.prototype.key_down = function (code, key, ctrl) {
    globalThis.__sent.push(key);
    return keyDown.call(this, code, key, ctrl);
  };
  Game.prototype.step = function (ms) {
    globalThis.__ticks += 1;
    if (globalThis.__trap) throw new WebAssembly.RuntimeError('unreachable (test trap)');
    if (this.started()) globalThis.__clock += ms;
    step.call(this, ms);
    for (const b of queued) {
      if (b !== held) globalThis.__buttons.push([b, globalThis.__clock]);
      held = b;
    }
    queued = [];
    globalThis.__saveGen = this.save_generation();
    globalThis.__owned = this.keys_owned();
  };
  Game.prototype.present = function () {
    const before = this.frame_version();
    const t = performance.now();
    present.call(this);
    globalThis.__presentMs += performance.now() - t;
    globalThis.__presents += 1;
    if (this.frame_version() !== before) globalThis.__converted += 1;
    globalThis.__shownGame = this;
  };
  Game.prototype.ran = function (k) {
    ran.call(this, k);
    globalThis.__speed = this.speed();
    globalThis.__dropped = this.dropped_ms();
    globalThis.__clamped = this.clamped_ms();
  };
}
`;

// Appended to the glue with `gameFirst`: a start runs `game` at once (Game.start_game_first, as
// `u6-native --game-first`), so the tests of the queue, the storage and the keys need no
// character from `u`. The page itself is unchanged.
const GAME_FIRST = `
Game.prototype.start = Game.prototype.start_game_first;
`;

/// Starts the server and a headless Chromium; `close()` stops both.
export async function setup(root = SITE) {
  const server = await serve({ root });
  const browser = await chromium.launch();
  return { server, browser, close: async () => { await browser.close(); await server.close(); } };
}

/// A fresh context (empty storage) and page with the hooks; `gated` adds the store and glue
/// wrappers; `gameFirst` (default) starts `game` without the launcher's `u` (the page itself is
/// unchanged).
export async function openPage(env, { gated = true, gameFirst = true } = {}) {
  const context = await env.browser.newContext({ viewport: { width: 1024, height: 900 } });
  await context.addInitScript(pageHooks);
  if (gated) {
    await context.route(/\/store\.js(\?real)?$/, async (route) => {
      if (route.request().url().endsWith('?real')) return route.continue();
      const response = await route.fetch();
      await route.fulfill({ response, body: STORE_WRAPPER });
    });
  }
  if (gated || gameFirst) {
    const extra = (gated ? GLUE_HOOKS : '') + (gameFirst ? GAME_FIRST : '');
    await context.route(/\/pkg\/u6_web\.js$/, async (route) => {
      const response = await route.fetch();
      await route.fulfill({ response, body: (await response.text()) + extra });
    });
  }
  const page = await context.newPage();
  // A page error's text can quote the game's data (an engine panic names its values): its kind
  // only.
  page.on('pageerror', (err) => console.error(`  page error: ${errorName(err)}`));
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

/// The stored save files ({ path, bytes: number[] }), sorted by path.
export function savedFiles(page) {
  return page.evaluate(() => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('files');
    req.onerror = () => fail(req.error);
    req.onsuccess = () => {
      const db = req.result;
      const get = db.transaction('files').objectStore('files')
        .getAll(IDBKeyRange.bound('save:', 'save:\uffff'));
      get.onsuccess = () => {
        db.close();
        done(get.result.map((f) => ({ path: f.path, bytes: [...f.bytes] }))
          .sort((a, b) => (a.path < b.path ? -1 : Number(a.path > b.path))));
      };
      get.onerror = () => fail(get.error);
    };
  }));
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
  program: document.getElementById('screen').dataset.program,
  name: document.getElementById('screen').dataset.name,
}));

export const log = (page) => page.evaluate(() => [...window.__log]);

export async function waitLog(page, entry, timeout = 30_000) {
  await page.waitForFunction((e) => window.__log.some((l) => l.startsWith(e)), entry, { timeout });
}

/// The canvas pixels (RGBA), one per game pixel (320 x 200): with HD on (a canvas S times that,
/// spec 07) the top-left device pixel of each game pixel's S x S block, read in the page. That
/// sub-pixel is MMPX's where the pixel shows HD art, not always the game's colour: it serves
/// comparisons of frames with each other (a scroll, a change); a check of the game's colours
/// reads the game's RGBA frame (`Game.frame_rgba`) instead.
export function frame(page) {
  return page.evaluate(() => {
    const c = document.getElementById('screen');
    const s = c.width / 320;
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
    const out = new Array(320 * 200 * 4);
    for (let y = 0; y < 200; y += 1) {
      for (let x = 0; x < 320; x += 1) {
        const from = (y * s * c.width + x * s) * 4;
        for (let k = 0; k < 4; k += 1) out[(y * 320 + x) * 4 + k] = d[from + k];
      }
    }
    return out;
  });
}

/// The shift along one axis (in pixels, multiples of 16; `dx` or `dy`, the other 0) that best
/// maps `before` onto `after` in the map window (11 x 11 tiles from (8, 8)), how well it matches
/// and how well no shift does (`still`). A step east shows as `dx` 16, south as `dy` 16. A shift
/// along x is compared over the window's middle 48 columns, one along y over its middle 48 rows,
/// so a shift of up to 4 cells stays inside the window; no shift over the columns.
export function mapShift(before, after, width = 320) {
  const whole = [8, 8 + 176];
  const middle = [8 + 64, 8 + 176 - 64];
  const score = (dx, dy) => {
    const [xs, ys] = dy === 0 ? [middle, whole] : [whole, middle];
    let same = 0;
    let n = 0;
    for (let y = ys[0]; y < ys[1]; y += 2) {
      for (let x = xs[0]; x < xs[1]; x += 2) {
        const a = ((y + dy) * width + x + dx) * 4;
        const b = (y * width + x) * 4;
        n += 1;
        if (before[a] === after[b] && before[a + 1] === after[b + 1] && before[a + 2] === after[b + 2]) same += 1;
      }
    }
    return same / n;
  };
  const still = score(0, 0);
  let best = { dx: 0, dy: 0, score: still };
  for (let s = -64; s <= 64; s += 16) {
    for (const [dx, dy] of s === 0 ? [] : [[s, 0], [0, s]]) {
      const v = score(dx, dy);
      if (v > best.score) best = { dx, dy, score: v };
    }
  }
  return { ...best, still };
}

export const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

/// The directions a step tries, in this order, by arrow key and by the cell it walks to from the
/// hero's (dx, dy): east, west, south, north.
export const STEPS = [
  { key: 'ArrowRight', dx: 1, dy: 0 },
  { key: 'ArrowLeft', dx: -1, dy: 0 },
  { key: 'ArrowDown', dx: 0, dy: 1 },
  { key: 'ArrowUp', dx: 0, dy: -1 },
];
/// How many times a step tries the four directions, a pass between two rounds.
export const STEP_ROUNDS = 4;
/// Where a killed hero wakes, as the page writes the leader: (0x133, 0x160, 0), the start cell
/// itself (u6-core `map::travel::resurrect_party`, 0x1095bc; pinned by its test
/// `resurrection_wakes_the_party_in_the_castle`, crates/u6-core/src/map/travel.rs:685, the cell
/// at :701-702). The page shows no death, so a leader there is only on an allowed wake
/// destination.
export const WAKE_CELL = '307,352,0';

/// What a key or click toward `step` ({ dx, dy }) did, from the leader at `from` to `to` (as the
/// page writes them: x,y,z): 'blocked' (no cell walked); 'wake cell' (the leader ended on
/// `WAKE_CELL` from another cell, the allowed wake destination: the turn after the step may
/// have killed the hero, so not even a step into it from beside it counts as walked); 'walked'
/// (the one cell toward `step`). Anything else fails as a walk of the wrong cells.
export function stepResult(from, to, { dx, dy }) {
  if (to === from) return 'blocked';
  if (to === WAKE_CELL) return 'wake cell';
  const [fx, fy, fz] = from.split(',').map(Number);
  const [tx, ty, tz] = to.split(',').map(Number);
  assert.ok(tx - fx === dx && ty - fy === dy && tz === fz, 'a step walks the one cell toward its key, or ends on the allowed wake destination');
  return 'walked';
}

/// The page's side of a step (`stepOnce`): wait for the prompt, read the leader, read the frame,
/// do `step` (its arrow, then until the game took it and shows the prompt again), pass. A test
/// drives `stepOnce` with its own: `act` a click (the mouse tests' `clickStep`), or all of them
/// a stub (the no-page test of the tries).
export const pageSteps = (page) => ({
  prompt: () => waitPrompt(page),
  leader: async () => (await state(page)).leader,
  frame: () => frame(page),
  act: async (step) => {
    await page.keyboard.press(step.key);
    await waitTaken(page);
    await waitPrompt(page);
  },
  pass: () => passTurn(page),
});

/// One step from the prompt: east, else west, south, north (`STEPS`), by arrow (or `io.act`). The
/// seed is the clock's and every frame at the prompt draws from the game's RNG (its idle
/// animation), so each boot's start fight goes its own way and can leave a gargoyle on any side of
/// the hero. A blocked step costs no movement points, so the world turn after it picks the hero
/// again at once: no NPC turn runs and the next direction meets the fight as it was. A gargoyle in
/// assault beside the hero strikes him every turn and stays while it fights; passes mostly end that
/// (in boots of seeds that start with gargoyles east and west of him, the old east-or-west step
/// walked at its first call in 29 of 45, after 2 to 6 passes, at 1x and at a 4x CPU throttle), but
/// a hero they kill wakes on the start cell (`WAKE_CELL`) out of combat, so the members no longer
/// strike back, between the same gargoyles, and from then on east and west stay blocked on every
/// turn (seen with gargoyles north, east and west of the start cell, across the hero's deaths). A
/// member's cell is open (the step swaps them), and the start fight has fewer foes than the hero
/// has sides (at most three near him at the first prompt, over 10000 seeds). Only after a round of
/// four blocked does a pass spend the hero's turn, so the fight moves on, up to `STEP_ROUNDS`
/// rounds; after a try that ended on the wake cell the next direction is tried from there. With
/// `withFrame` the frame before each try is read (a whole canvas over the protocol, at S 4 1280 x
/// 800: only for a check of the step's scroll). Returns the step (key, dx, dy), the leader before
/// and after and, with `withFrame`, the frame before it. After the bound it fails with each try's
/// key and outcome and the passes between, in order (`what` names it: a click step).
export async function stepOnce(page, { withFrame = false, io = pageSteps(page), what = 'step' } = {}) {
  const tried = [];
  for (let round = 0; round < STEP_ROUNDS; round += 1) {
    if (round > 0) {
      await io.pass();
      tried.push('pass');
    }
    for (const step of STEPS) {
      await io.prompt();
      const from = await io.leader();
      const before = withFrame ? await io.frame() : undefined;
      await io.act(step);
      const to = await io.leader();
      const result = stepResult(from, to, step);
      tried.push(`${step.key} ${result}`);
      if (result === 'walked') return { ...step, from, to, before };
    }
  }
  assert.fail(`no ${what} in ${STEP_ROUNDS} rounds of the four directions: ${tried.join(', ')}`);
}

/// At the prompt, a pass (Space on the focused canvas): the hero's turn is spent, so the world
/// turn runs the NPCs' turns (the start fight's foes take theirs), then the prompt again.
export async function passTurn(page) {
  await waitPrompt(page);
  await page.keyboard.press('Space');
  await waitTaken(page);
  await waitPrompt(page);
}

// Each wait's own id: a wait counts its looks from zero (a count left by an earlier wait would
// pass a new one at its first look, which can see the frame drawn before a key just pressed).
let waits = 0;

/// Waits until the game shows its prompt (waiting for a command key, nothing queued) for a few
/// frames in a row, counted from this call: the world turns after a command (the start fight's)
/// have run. The first look is at once, the others one per frame, after the page's frame.
export async function waitPrompt(page, timeout = 60_000) {
  waits += 1;
  await page.waitForFunction((id) => {
    if (window.__promptWait !== id) {
      window.__promptWait = id;
      window.__promptRun = 0;
    }
    const at = document.getElementById('screen').dataset.prompt === '1';
    window.__promptRun = at ? window.__promptRun + 1 : 0;
    return window.__promptRun >= 5;
  }, waits, { timeout, polling: 'raf' });
}

/// Waits `n` frames of the page. The page's frame callback runs before this wait's in every
/// frame, so after them a running game had `n` more frames (at 1×, the steps their wall time is
/// worth: about one in most frames at 50–60 Hz); a stopped or ended one stepped in none, which
/// is what the checks of a stopped loop count on.
export async function waitFrames(page, n, timeout = 60_000) {
  if (n <= 0) return;
  waits += 1;
  await page.waitForFunction(({ id, count }) => {
    if (window.__frameWait !== id) {
      window.__frameWait = id;
      window.__frameRun = 0;
      return false; // the first look runs at once, between frames
    }
    window.__frameRun += 1;
    return window.__frameRun >= count;
  }, { id: waits, count: n }, { timeout, polling: 'raf' });
}

/// Waits until the running program is `program` (u, game or ended).
export async function waitProgram(page, program, timeout = 60_000) {
  await page.waitForFunction((p) => document.getElementById('screen').dataset.program === p,
    program, { timeout, polling: 'raf' });
}

/// Waits until the canvas shows a colour other than black.
export async function waitVisible(page, timeout = 30_000) {
  await page.waitForFunction(() => {
    const c = document.getElementById('screen');
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
    for (let i = 0; i < d.length; i += 4) if (d[i] || d[i + 1] || d[i + 2]) return true;
    return false;
  }, null, { timeout, polling: 'raf' });
}

/// Waits until the engine ran `n` more ticks, the page's fixed steps of 20 ms: gated pages count
/// them in the glue; elsewhere `n` frames of the page's loop (at 1×, as much engine time as
/// `n` frames of the display).
export async function waitTicks(page, n, timeout = 60_000) {
  const from = await page.evaluate(() => globalThis.__ticks);
  if (from === undefined) {
    await waitFrames(page, n, timeout);
    return;
  }
  await page.waitForFunction((t) => globalThis.__ticks >= t, from + n, { timeout, polling: 'raf' });
}

/// Waits until the game took every key and pointer event given to the page so far: a step
/// hands them to the engine and the game read them all (`data-keys` 0). The page writes
/// `data-keys` after each frame, so a count read before one is the frame's from before the
/// input.
export async function waitTaken(page, timeout = 60_000) {
  await waitTicks(page, 1, timeout);
  await page.waitForFunction(() => document.getElementById('screen').dataset.keys === '0',
    null, { timeout, polling: 'raf' });
}

/// Presses `key` on the focused canvas, waits until the game took it, then `settle` ticks: the
/// scripted keys of the Rust tests (`Run::keys`), in the page's steps.
export async function press(page, key, settle) {
  await page.keyboard.press(key);
  await waitTaken(page);
  await waitTicks(page, settle);
}

/// A pixel of the canvas (320 x 200 game pixels) as a page position through its CSS box.
export async function screenPoint(page, x, y) {
  const box = await page.locator('#screen').boundingBox();
  return [box.x + ((x + 0.5) * box.width) / 320, box.y + ((y + 0.5) * box.height) / 200];
}

/// Clicks game pixel (x, y), waits until the game took the press and the release, then
/// `settle` ticks: `press` for the mouse.
export async function click(page, x, y, settle) {
  await page.mouse.click(...(await screenPoint(page, x, y)));
  await waitTaken(page);
  await waitTicks(page, settle);
}

/// Opt-in virtual display for the rate tests (spec 06 Acceptance 1): from this call on, the
/// page's requestAnimationFrame and performance.now run on a virtual clock at `hz` frames a
/// second that moves only while the test runs frames (`runFrames`). Each frame moves it on by
/// 1000 / hz ms and calls the callbacks queued, with that time; a step takes no virtual time, so
/// a frame's budget never runs out. Playwright's own polls keep the real frames (its utility
/// script took them at the page's first evaluate, before this). The virtual clock starts at the
/// real one: a loop already running moves over at its next real frame, which this waits for
/// (that frame is less than one interval before the first virtual one); with `loop: false`
/// (before a start) the loop starts on the virtual display. `displayCost` makes the page's own
/// clock reads cost time (a slow host).
export async function virtualDisplay(page, hz, { loop = true } = {}) {
  await page.evaluate((rate) => {
    const v = { hz: rate, base: performance.now(), frames: 0, queue: [], cost: 0, spent: 0 };
    v.now = () => v.base + (v.frames * 1000) / v.hz;
    // The frames' callbacks: the frame's time, the clock's cost so far reset.
    v.run = () => {
      v.frames += 1;
      v.spent = 0;
      const due = v.queue;
      v.queue = [];
      for (const callback of due) callback(v.now());
      return due.length;
    };
    window.__display = v;
    window.requestAnimationFrame = (callback) => {
      v.queue.push(callback);
      return v.queue.length;
    };
    performance.now = () => {
      const t = v.now() + v.spent;
      v.spent += v.cost;
      return t;
    };
  }, hz);
  if (loop) await page.waitForFunction(() => window.__display.queue.length > 0, null, { timeout: 10_000 });
}

/// On the virtual display: every read of performance.now in a frame moves it on by `ms` (the
/// page's steps then take time: the frame's budget holds fewer of them); 0 makes them free again.
export function displayCost(page, ms) {
  return page.evaluate((cost) => { window.__display.cost = cost; }, ms);
}

/// On the virtual display: `ms` pass with no frame (a stall, a hidden tab); the next frame comes
/// that much later.
export function displayStall(page, ms) {
  return page.evaluate((wait) => { window.__display.base += wait; }, ms);
}

/// Sets the virtual display's rate to `hz` from its current time on.
export function displayRate(page, hz) {
  return page.evaluate((rate) => {
    const v = window.__display;
    v.base = v.now();
    v.frames = 0;
    v.hz = rate;
  }, hz);
}

/// Runs `n` frames of the virtual display (one evaluate, nothing else runs between them);
/// returns how many frame callbacks it called and the leader positions the page showed, each
/// change once (the first is the one before these frames).
export function runFrames(page, n) {
  return page.evaluate((count) => {
    const v = window.__display;
    const c = document.getElementById('screen');
    const seen = [c.dataset.leader];
    let called = 0;
    for (let i = 0; i < count; i += 1) {
      called += v.run();
      if (c.dataset.leader !== seen.at(-1)) seen.push(c.dataset.leader);
    }
    return { called, seen };
  }, n);
}

/// On the virtual display: runs frames until the engine ran `step` steps in all (`__ticks`), at
/// most `max`; returns the steps it stopped at (more than `step` when the frame that reached it
/// ran several).
export function runFramesToStep(page, step, max) {
  return page.evaluate(([want, limit]) => {
    const v = window.__display;
    for (let i = 0; i < limit && globalThis.__ticks < want; i += 1) v.run();
    return globalThis.__ticks;
  }, [step, max]);
}

/// On the virtual display: runs frames until the game took every input and shows its prompt for
/// five frames in a row (`waitPrompt`), at most `max`; returns the frames run and the leader
/// positions the page showed, each change once (the first is the one before these frames).
export async function runFramesToPrompt(page, max) {
  const out = await page.evaluate((limit) => {
    const v = window.__display;
    const c = document.getElementById('screen');
    const seen = [c.dataset.leader];
    let run = 0;
    let frames = 0;
    while (frames < limit && run < 5) {
      v.run();
      frames += 1;
      if (c.dataset.leader !== seen.at(-1)) seen.push(c.dataset.leader);
      run = c.dataset.prompt === '1' && c.dataset.keys === '0' ? run + 1 : 0;
    }
    return { frames, seen, prompt: run >= 5 };
  }, max);
  if (!out.prompt) throw new Error(`no prompt within ${max} virtual frames`);
  return out;
}

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
export async function storePack(bytes) {
  log('pack:store:start'); await window.__gate('pack-store');
  const kept = await real.storePack(bytes); log('pack:store:end:' + kept); return kept;
}
export async function loadPack() {
  const bytes = await real.loadPack(); log('pack:load:' + (bytes ? 1 : 0)); return bytes;
}
export async function forgetPack() {
  log('pack:forget:start'); await window.__gate('pack-forget');
  await real.forgetPack(); log('pack:forget:end');
}
export async function forgetBundles() {
  log('bundles:forget:start'); await window.__gate('bundles-forget');
  await real.forgetBundles(); log('bundles:forget:end');
}
export async function storeVoicePack(pack, source) {
  log('voicepack:store:start'); await window.__gate('voicepack-store');
  if (window.__fail.has('voicepack-store')) { log('voicepack:store:fail'); throw new DOMException('test: no room', 'QuotaExceededError'); }
  await real.storeVoicePack(pack, source); log('voicepack:store:end');
}
export async function loadVoicePack() {
  const pack = await real.loadVoicePack(); log('voicepack:load:' + (pack ? 1 : 0)); return pack;
}
export async function forgetVoicePack() {
  log('voicepack:forget:start'); await window.__gate('voicepack-forget');
  await real.forgetVoicePack(); log('voicepack:forget:end');
}
export async function storeLocalePack(bytes, source) {
  log('locale:store:start'); await window.__gate('locale-store');
  if (window.__fail.has('locale-store')) { log('locale:store:fail'); return false; }
  const kept = await real.storeLocalePack(bytes, source); log('locale:store:end:' + kept); return kept;
}
export async function loadLocalePack() {
  const kept = await real.loadLocalePack(); log('locale:load:' + (kept ? 1 : 0)); return kept;
}
export async function forgetLocalePack() {
  log('locale:forget:start'); await window.__gate('locale-forget');
  if (window.__fail.has('locale-forget')) { log('locale:forget:fail'); throw new Error('test: storage refused the delete'); }
  await real.forgetLocalePack(); log('locale:forget:end');
}
`;

// Appended to the wasm-bindgen glue (module scope): the wasm memory size, a trap switch, the
// steps run (`__ticks`: the helpers' ticks are the page's fixed steps of 20 ms), the save
// generation of the game stepped last (whether or not the page stores its Saves), the engine
// clock and the button changes the game took on it, the keys the game takes itself
// (`__owned`, keys_owned after the step), the keys the page sent it (`__sent`, their DOM `key`,
// and in order with them `text:` and the text of each Game.text_input: the tests' own words)
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
// `__starts` has one entry per game started in the page (a Game.start that returned, or the
// start_game_first GAME_FIRST puts in its place), in order: `locale`, what its set_locale gave
// (null: none, 'on', 'mismatch'), and `stepsBefore`, the steps any game ran between its start
// and that set_locale (0: before its first step).
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
  const textInput = Game.prototype.text_input;
  Game.prototype.text_input = function (text) {
    globalThis.__sent.push('text:' + text);
    return textInput.call(this, text);
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
  globalThis.__starts = [];
  // Both starts (GAME_FIRST, appended after this, makes start the second).
  for (const name of ['start', 'start_game_first']) {
    const start = Game.prototype[name];
    Game.prototype[name] = function (seed) {
      const report = start.call(this, seed);
      this.__start = { locale: null, stepsBefore: null, ticks: globalThis.__ticks };
      globalThis.__starts.push(this.__start);
      return report;
    };
  }
  const setLocale = Game.prototype.set_locale;
  Game.prototype.set_locale = function (pack) {
    const why = setLocale.call(this, pack);
    if (this.__start) {
      this.__start.locale = why ? 'mismatch' : 'on';
      this.__start.stepsBefore = globalThis.__ticks - this.__start.ticks;
    }
    return why;
  };
}
`;

// Appended to the glue with `gated` (spec 09): `globalThis.__speech`, utterances the test made
// (its own text), handed to the page by Game.take_speech after what the game's speech tap queued,
// as the page's player gets them; `__speechCalls` counts the calls that returned any.
const SPEECH_HOOK = `
globalThis.__speech = [];
globalThis.__speechCalls = 0;
{
  const take = Game.prototype.take_speech;
  Game.prototype.take_speech = function () {
    const real = take.call(this);
    if (globalThis.__speech.length === 0) {
      if (real) globalThis.__speechCalls += 1;
      return real;
    }
    globalThis.__speechCalls += 1;
    return JSON.stringify([...(real ? JSON.parse(real) : []), ...globalThis.__speech.splice(0)]);
  };
}
`;

// Appended to the glue with `gameFirst`: a start runs `game` at once (Game.start_game_first, as
// `u6-native --game-first`), so the tests of the queue, the storage and the keys need no
// character from `u`. The page itself is unchanged.
const GAME_FIRST = `
Game.prototype.start = Game.prototype.start_game_first;
`;

// Page init script: `window.__packFiles(images)` and `window.__zipFiles(files)`, a synthetic HD
// pack (spec 07 HD1, `u6-hdpack/1`) built in the page from images the test made, the way
// u6-hdpack's writer lays it out: `images` are { kind: 1 tile | 2 portrait | 3 bitmap, id, letter
// (portraits; else 0), hash (16 hex digits: the classic image's content key), width, height,
// rgba: Uint8Array } (×4 sizes); `__packFiles` resolves to its files [name, bytes] in the
// writer's order (manifest.json, index.bin, atlas-N.webp, portrait/L-N.webp, bitmap/ROLE.webp),
// each picture lossless WebP from the browser's own encoder (quality 1: VP8L), so a test can
// change one before `__zipFiles` stores them in a zip (each entry's CRC-32 its bytes' unless
// `crcOf` names a file whose CRC-32 is taken from other bytes). No game data: the pictures are
// the test's; the hashes are keys the page itself computed.
export function packKit() {
  const table = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc32 = (bytes) => {
    let c = 0xffffffff;
    for (let i = 0; i < bytes.length; i++) c = table[(c ^ bytes[i]) & 255] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const webp = async (w, h, rgba) => {
    const canvas = new OffscreenCanvas(w, h);
    const ctx = canvas.getContext('2d');
    ctx.putImageData(new ImageData(new Uint8ClampedArray(rgba), w, h), 0, 0);
    const blob = await canvas.convertToBlob({ type: 'image/webp', quality: 1 });
    return new Uint8Array(await blob.arrayBuffer());
  };
  const ROLES = ['paper', 'spellbook'];
  window.__packFiles = async (images) => {
    const key = (im) => [im.kind, im.kind === 2 ? im.letter : 0, im.id];
    const sorted = [...images].sort((a, b) => {
      const [x, y] = [key(a), key(b)];
      return x[0] - y[0] || x[1] - y[1] || x[2] - y[2];
    });
    const atlases = [];
    let tiles = 0;
    const entries = sorted.map((im) => {
      let cell = [0xffff, 0, 0];
      if (im.kind === 1) {
        const [atlas, k] = [Math.floor(tiles / 1024), tiles % 1024];
        tiles += 1;
        cell = [atlas, k % 32, Math.floor(k / 32)];
        if (atlases.length === atlas) atlases.push({ rows: 0, tiles: [] });
        atlases[atlas].rows = Math.max(atlases[atlas].rows, cell[2] + 1);
        atlases[atlas].tiles.push([im, cell]);
      }
      let alpha = false;
      for (let i = 3; i < im.rgba.length; i += 4) if (im.rgba[i] !== 255) alpha = true;
      return { im, cell, alpha };
    });
    const index = new Uint8Array(16 + 24 * entries.length);
    const view = new DataView(index.buffer);
    index.set(new TextEncoder().encode('U6HDIDX\0'), 0);
    view.setUint32(8, 1, true);
    view.setUint32(12, entries.length, true);
    let base = 0xcbf29ce484222325n;
    const fnv = (b) => {
      base = ((base ^ BigInt(b)) * 0x100000001b3n) & 0xffffffffffffffffn;
    };
    entries.forEach(({ im, cell, alpha }, i) => {
      const at = 16 + 24 * i;
      const [kind, letter, id] = key(im);
      view.setUint8(at, kind);
      view.setUint8(at + 1, alpha ? 1 : 0);
      view.setUint16(at + 2, id, true);
      view.setUint8(at + 4, letter);
      [cell[0], cell[1], cell[2], im.width, im.height].forEach((v, k) => view.setUint16(at + 6 + 2 * k, v, true));
      view.setBigUint64(at + 16, BigInt(`0x${im.hash}`), true);
      for (const b of [kind, letter, id & 255, id >> 8]) fnv(b);
      for (let k = 0; k < 8; k++) fnv(index[at + 16 + k]);
    });
    const manifest = {
      format: 'u6-hdpack/1', tile: 16, scale: 4, derived: true,
      generator: { name: 'test', version: '0', engine: 'none', date: '2026-10-16' },
      base: base.toString(16).padStart(16, '0'), images: entries.length,
    };
    const files = [['manifest.json', new TextEncoder().encode(JSON.stringify(manifest))], ['index.bin', index]];
    for (const [n, a] of atlases.entries()) {
      const rgba = new Uint8Array(2048 * a.rows * 64 * 4);
      for (const [im, [, col, row]] of a.tiles) {
        for (let y = 0; y < 64; y++) {
          rgba.set(im.rgba.subarray(y * 256, y * 256 + 256), ((row * 64 + y) * 2048 + col * 64) * 4);
        }
      }
      files.push([`atlas-${n}.webp`, await webp(2048, a.rows * 64, rgba)]);
    }
    for (const { im } of entries.filter((e) => e.im.kind !== 1)) {
      const name = im.kind === 2 ? `portrait/${String.fromCharCode(im.letter)}-${im.id}.webp` : `bitmap/${ROLES[im.id]}.webp`;
      files.push([name, await webp(im.width, im.height, im.rgba)]);
    }
    return files;
  };
  window.__zipFiles = (files, { crcOf } = {}) => {
    const u16 = (v) => [v & 255, (v >>> 8) & 255];
    const u32 = (v) => [v & 255, (v >>> 8) & 255, (v >>> 16) & 255, (v >>> 24) & 255];
    const parts = [];
    const dir = [];
    let offset = 0;
    for (const [name, bytes] of files) {
      const n = new TextEncoder().encode(name);
      const crc = crc32(crcOf && crcOf[0] === name ? crcOf[1] : bytes);
      const common = [...u16(20), ...u16(0), ...u16(0), ...u16(0), ...u16(33), ...u32(crc), ...u32(bytes.length), ...u32(bytes.length), ...u16(n.length), ...u16(0)];
      const head = new Uint8Array([...u32(0x04034b50), ...common, ...n]);
      parts.push(head, bytes);
      dir.push(...u32(0x02014b50), ...u16(20), ...common, ...u16(0), ...u16(0), ...u16(0), ...u32(0), ...u32(offset), ...n);
      offset += head.length + bytes.length;
    }
    const end = [...u32(0x06054b50), ...u16(0), ...u16(0), ...u16(files.length), ...u16(files.length), ...u32(dir.length), ...u32(offset), ...u16(0)];
    const out = new Uint8Array(offset + dir.length + end.length);
    let at = 0;
    for (const p of [...parts, new Uint8Array(dir), new Uint8Array(end)]) {
      out.set(p, at);
      at += p.length;
    }
    return out;
  };
}

/// Starts the server and a headless Chromium; `close()` stops both. The server answers /packs/*
/// as the site's static files there, from `packs` (name -> body, a string or bytes the test made;
/// nothing on disk; a name it does not hold is a 404), each answer held while `holdPack` holds
/// its name, and logs each of those requests in `packRequests` as [path, the status answered].
export async function setup(root = SITE) {
  const packs = new Map();
  const packRequests = [];
  const packHolds = new Map();
  const server = await serve({
    root,
    packs: async (request) => {
      const path = new URL(request.url).pathname;
      const name = decodeURIComponent(path.slice('/packs/'.length));
      const hold = packHolds.get(name);
      if (hold) {
        hold.arrived();
        await hold.gate;
      }
      const body = packs.get(name);
      const response = body === undefined ? new Response('not found', { status: 404 }) : new Response(body);
      packRequests.push([path, response.status]);
      return response;
    },
  });
  const browser = await chromium.launch();
  // Chromium with WebGPU on this machine's GPU (spec 09's voices): launched once, when a test asks.
  let gpu = null;
  const gpuBrowser = () => (gpu ??= chromium.launch({ channel: 'chromium' }));
  const browsers = async () => [browser, ...(gpu ? [await gpu] : [])];
  return {
    server, browser, gpuBrowser, browsers, packs, packRequests, packHolds,
    close: async () => {
      for (const b of await browsers()) await b.close();
      await server.close();
    },
  };
}

/// Holds the answer to every request for /packs/`name` until `release()` (the body is read from
/// `env.packs` then); `asked` resolves once such a request came.
export function holdPack(env, name) {
  let arrived;
  let release;
  const asked = new Promise((done) => { arrived = done; });
  const gate = new Promise((done) => { release = done; });
  env.packHolds.set(name, { arrived, gate, release });
  return {
    asked,
    release: () => {
      env.packHolds.delete(name);
      release();
    },
  };
}

/// The voice model as tools/speech/fetch-models.sh hosts it under site/models/ (spec 09 V2).
export const MODELS = join(SITE, 'models');
export const haveModels = () => existsSync(join(MODELS, 'supertonic3')) && existsSync(join(MODELS, 'onnxruntime-web'));

/// The version of the chooser's record (site/app.js SETUP_VERSION).
export const SETUP_VERSION = 3;

/// The choice a page starts with unless a test asks for the chooser (`openPage`'s `setup`).
export const CHOSEN = { lang: 'en' };

/// A fresh context (empty storage) and page with the hooks; `gated` adds the store and glue
/// wrappers; `gameFirst` (default) starts `game` without the launcher's `u` (the page itself is
/// unchanged); `setup` is the chooser's choice the page finds kept (`CHOSEN`: no chooser), as a
/// record of this version, or a string kept as it is (a record of an older page), null for a
/// first visit (the chooser shows); `gpu`: in the Chromium with WebGPU; `locale`: the
/// browser's language (navigator.language), en-US unless a test asks, whatever the machine's;
/// `layout`: the screen layout the page finds kept (spec 10 Design 7: `original` unless a test
/// asks for `hd` or `wide`, so every click test keeps its 320 x 200 box and the Original build;
/// null keeps none, the page's default: the Wide build, spec 13), kept only while the page keeps
/// none (a choice the page made survives a reload);
/// `font`: the font the page finds kept (spec 11 Design 5: `pixel` or `hd`; null, the default,
/// keeps none: the page's default, the HD font), kept likewise only while the page keeps none.
export async function openPage(env, { gated = true, gameFirst = true, setup = CHOSEN, gpu = false, locale = 'en-US', layout = 'original', font = null } = {}) {
  const browser = gpu ? await env.gpuBrowser() : env.browser;
  const context = await browser.newContext({ viewport: { width: 1024, height: 900 }, locale });
  await context.addInitScript(pageHooks);
  if (setup) {
    await context.addInitScript((value) => {
      try {
        if (localStorage.getItem('u6.setup') === null) localStorage.setItem('u6.setup', value);
      } catch {
        // An opaque origin (about:blank) has no storage.
      }
    }, typeof setup === 'string' ? setup : JSON.stringify({ ...setup, version: SETUP_VERSION }));
  }
  if (layout) {
    await context.addInitScript((value) => {
      try {
        if (localStorage.getItem('u6.layout') === null) localStorage.setItem('u6.layout', value);
      } catch {
        // An opaque origin (about:blank) has no storage.
      }
    }, layout);
  }
  if (font) {
    await context.addInitScript((value) => {
      try {
        if (localStorage.getItem('u6.font') === null) localStorage.setItem('u6.font', value);
      } catch {
        // An opaque origin (about:blank) has no storage.
      }
    }, font);
  }
  if (gated) {
    await context.route(/\/store\.js(\?real)?$/, async (route) => {
      if (route.request().url().endsWith('?real')) return route.continue();
      const response = await route.fetch();
      await route.fulfill({ response, body: STORE_WRAPPER });
    });
  }
  if (gated || gameFirst) {
    const extra = (gated ? GLUE_HOOKS + SPEECH_HOOK : '') + (gameFirst ? GAME_FIRST : '');
    // Either build's glue (spec 13 Decision 2: site/pkg, site/pkg-wide).
    await context.route(/\/pkg(-wide)?\/u6_web\.js$/, async (route) => {
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

/// `openPage` with the Wide screen kept (spec 13: the Wide build, site/pkg-wide), in a viewport
/// of `width` x `height`.
export async function openWide(env, { width = 1400, height = 900, ...rest } = {}) {
  const opened = await openPage(env, { ...rest, layout: 'wide' });
  await opened.page.setViewportSize({ width, height });
  return opened;
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

/// The bytes of the HD pack kept in IndexedDB (`hd:pack`), as a byte count, or null when none.
export function keptPackBytes(page) {
  return page.evaluate(() => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('files');
    req.onerror = () => fail(req.error);
    req.onsuccess = () => {
      const db = req.result;
      const get = db.transaction('files').objectStore('files').get('hd:pack');
      get.onsuccess = () => { db.close(); done(get.result ? get.result.length : null); };
      get.onerror = () => fail(get.error);
    };
  }));
}

/// The NPC bundles kept in IndexedDB (spec 09 Design 5): their keys (sorted) and the bundle
/// objects by key. They hold game text: compared in the test, never printed.
export function keptBundles(page) {
  return page.evaluate(() => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('files');
    req.onerror = () => fail(req.error);
    req.onsuccess = () => {
      const db = req.result;
      const store = db.transaction('files').objectStore('files');
      const range = IDBKeyRange.bound('npc-bundle:', 'npc-bundle:\uffff');
      const keys = store.getAllKeys(range);
      const values = store.getAll(range);
      values.onsuccess = () => {
        db.close();
        done({ keys: [...keys.result].sort(), byKey: Object.fromEntries(keys.result.map((k, i) => [k, values.result[i]])) });
      };
      values.onerror = () => fail(values.error);
    };
  }));
}

/// The voice pack records kept in IndexedDB (spec 09 Design 4): their keys (sorted) and the
/// objects by key.
export function keptVoicePacks(page) {
  return page.evaluate(() => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('files');
    req.onerror = () => fail(req.error);
    req.onsuccess = () => {
      const db = req.result;
      const store = db.transaction('files').objectStore('files');
      const range = IDBKeyRange.bound('voice-pack:', 'voice-pack:￿');
      const keys = store.getAllKeys(range);
      const values = store.getAll(range);
      values.onsuccess = () => {
        db.close();
        done({ keys: [...keys.result].sort(), byKey: Object.fromEntries(keys.result.map((k, i) => [k, values.result[i]])) });
      };
      values.onerror = () => fail(values.error);
    };
  }));
}

/// The source kept with the Korean pack (spec 08 K6: the record `locale:ko:source`, the listed
/// file it came from), or null when there is none.
export function keptLocaleSource(page) {
  return page.evaluate(() => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('files');
    req.onerror = () => fail(req.error);
    req.onsuccess = () => {
      const db = req.result;
      const get = db.transaction('files').objectStore('files').get('locale:ko:source');
      get.onsuccess = () => {
        db.close();
        done(get.result ?? null);
      };
      get.onerror = () => fail(get.error);
    };
  }));
}

/// The Korean pack kept in the page's IndexedDB (spec 08: the record `locale:ko`), its bytes as a
/// plain array, or null.
export function keptLocalePack(page) {
  return page.evaluate(() => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('files');
    req.onerror = () => fail(req.error);
    req.onsuccess = () => {
      const db = req.result;
      const get = db.transaction('files').objectStore('files').get('locale:ko');
      get.onsuccess = () => {
        db.close();
        done(get.result instanceof Uint8Array ? [...get.result] : null);
      };
      get.onerror = () => fail(get.error);
    };
  }));
}

/// Replaces, in the page's kept game files (the record `extracted`), the bytes of every file of
/// bare name `name` (case ignored) by `bytes`, in the page: the user's other files never leave
/// it. Returns how many were replaced.
export function replaceKept(page, name, bytes) {
  return page.evaluate(({ name, bytes }) => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onerror = () => fail(req.error);
    req.onsuccess = () => {
      const db = req.result;
      const tx = db.transaction('files', 'readwrite');
      const store = tx.objectStore('files');
      const get = store.get('extracted');
      let n = 0;
      get.onsuccess = () => {
        const files = get.result ?? [];
        for (const f of files) {
          if (f.path.split('/').pop().toLowerCase() === name) {
            f.bytes = new Uint8Array(bytes);
            n += 1;
          }
        }
        store.put(files, 'extracted');
      };
      tx.oncomplete = () => {
        db.close();
        done(n);
      };
      tx.onerror = () => fail(tx.error);
    };
  }), { name, bytes });
}

/// The conversation scripts the kept game files hold, read in the page from their converse.a
/// and converse.b (each the first by case-folded path among those of that bare name, as the
/// engine's file lookup takes it) as a U6Lib of 32-bit offsets is read: an item is a script when
/// its offset is not 0 (u6-formats `U6Lib::parse`, `ConverseLib`). As `<letter>/<item>`, sorted;
/// they name the user's scripts: compared in the test, never printed.
export function keptScripts(page) {
  return page.evaluate(() => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('files');
    req.onerror = () => fail(req.error);
    req.onsuccess = () => {
      const db = req.result;
      const get = db.transaction('files').objectStore('files').get('extracted');
      get.onerror = () => fail(get.error);
      get.onsuccess = () => {
        db.close();
        const files = (get.result ?? []).map((f) => ({ fold: f.path.toLowerCase(), bytes: f.bytes }))
          .sort((x, y) => (x.fold < y.fold ? -1 : Number(x.fold > y.fold)));
        const out = [];
        for (const letter of ['a', 'b']) {
          const file = files.find((f) => f.fold.split(/[/:\\]/).pop() === `converse.${letter}`);
          if (!file) continue;
          const v = new DataView(file.bytes.buffer, file.bytes.byteOffset, file.bytes.byteLength);
          const at = (i) => v.getUint32(i * 4, true) & 0xffffff;
          let min = v.byteLength;
          for (let i = 0; (i + 1) * 4 <= min; i += 1) if (at(i) !== 0 && at(i) < min) min = at(i);
          for (let i = 0; i < Math.floor(min / 4); i += 1) if (at(i) !== 0) out.push(`${letter}/${i}`);
        }
        done(out.sort());
      };
    };
  }));
}

/// Keeps `files` ({ path, bytes: number[] }) in IndexedDB as the page's kept game files, as
/// site/store.js keeps them.
export function keepFiles(page, files) {
  return page.evaluate((list) => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('files');
    req.onerror = () => fail(req.error);
    req.onsuccess = () => {
      const db = req.result;
      const tx = db.transaction('files', 'readwrite');
      tx.objectStore('files').put(list.map(({ path, bytes }) => ({ path, bytes: new Uint8Array(bytes) })), 'extracted');
      tx.oncomplete = () => { db.close(); done(); };
      tx.onerror = () => fail(tx.error);
    };
  }), files);
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

/// Spec 17: the context menu the gated page's game gives a right press at game pixel (x, y) now
/// (`Game.context_menu` on `__shownGame`, read only), parsed; null for none.
export const contextMenuAt = (page, x, y) => page.evaluate(([gx, gy]) => {
  const json = globalThis.__shownGame?.context_menu(gx, gy) ?? '';
  return json ? JSON.parse(json) : null;
}, [x, y]);

/// Spec 17: `contextMenuAt` for each game pixel of `points` ([x, y] each), in one call.
export const contextMenus = (page, points) => page.evaluate((list) => list.map(([gx, gy]) => {
  const json = globalThis.__shownGame?.context_menu(gx, gy) ?? '';
  return json ? JSON.parse(json) : null;
}), points);

/// Presses the right button at game pixel (x, y) and waits until the page's context menu shows;
/// the button stays down.
export async function rightPress(page, x, y) {
  await page.mouse.move(...(await screenPoint(page, x, y)));
  await page.mouse.down({ button: 'right' });
  await page.locator('#context-menu').waitFor({ state: 'visible', timeout: 10_000 });
}

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
/// at :702-703). The page shows no death, so a leader there is only on an allowed wake
/// destination.
export const WAKE_CELL = '307,352,0';

/// What a key or click toward `step` ({ dx, dy }) did, from the leader at `from` to `to` (as the
/// page writes them: x,y,z): 'blocked' (no cell walked); 'walked' (the one cell toward `step`);
/// 'wake cell' (the leader ended on `WAKE_CELL`, the allowed wake destination, from a cell not
/// beside it that way: the turn after the step killed the hero). Anything else fails as a walk
/// of the wrong cells. A step that ends on the wake cell from the cell beside it, toward it, is
/// a walk: a blocked step runs no NPC turn (it costs the leader no points, and the world turn
/// picks him again at once: pinned outside a fight by u6-core's test
/// `a_blocked_step_takes_one_pick_and_keeps_the_leaders_points`,
/// crates/u6-core/src/actors/walk.rs:1015; in a fight that rests on the scheduler picking the
/// same actor again over the state the blocked step left unchanged, `actors::turn::pick_actor`,
/// turn.rs:402-407, not on a test), so a turn that kills him comes only after a step that moved
/// him there, and the cell is the one walked to; only a move the game refused after its cost
/// (0x10c9d2, `StepResult::Mismatch`), with a death in its turn, would end there unwalked.
/// Checked the other way round, every walk into the wake cell would go uncounted: a step from
/// beside it walking in then back out (to where it began), or none at all where that is the one
/// way out.
export function stepResult(from, to, { dx, dy }) {
  if (to === from) return 'blocked';
  const [fx, fy, fz] = from.split(',').map(Number);
  const [tx, ty, tz] = to.split(',').map(Number);
  if (tx - fx === dx && ty - fy === dy && tz === fz) return 'walked';
  assert.ok(to === WAKE_CELL, 'a step walks the one cell toward its key, or ends on the allowed wake destination');
  return 'wake cell';
}

/// The page's side of a step (`stepOnce`): wait for the prompt, read the leader, read the frame,
/// count the inputs the page handed the game (`sent`: the keys passed to `Game.key_down`,
/// `__sent`, on a gated page; else the keys the canvas took for the game, the `__keys` entries
/// it prevented), do `step` (its arrow, then until the game took it and shows the prompt again),
/// pass. A test drives `stepOnce` with its own: `act` a click (the mouse tests' `clickStep`), or
/// all of them a stub (the no-page test of the tries).
export const pageSteps = (page) => ({
  prompt: () => waitPrompt(page),
  leader: async () => (await state(page)).leader,
  frame: () => frame(page),
  sent: () => page.evaluate(() => globalThis.__sent?.length ?? window.__keys.filter(([, taken]) => taken).length),
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
/// rounds; after a try that ended on the wake cell the next direction is tried from there. A walk
/// that ends on the cell the call started on ('back': the hero killed in a pass, woken beside it,
/// then walking that way) is not the step either, so the step always leaves the leader elsewhere
/// than it found him. With `withFrame` the frame before each try is read (a whole canvas over the
/// protocol, 1280 x 800 at S 4: only for a check of the step's scroll). Returns the step (key, dx,
/// dy), the leader before and after and, with `withFrame`, the frame before it. After the bound it
/// fails with each try's key, outcome and the inputs the page sent the game for it (`io.sent`: 0
/// for a key that never reached the game, '?' where they are not counted) and the passes
/// between, in order (`what` names it: a click step).
export async function stepOnce(page, { withFrame = false, io = pageSteps(page), what = 'step' } = {}) {
  const tried = [];
  let start;
  for (let round = 0; round < STEP_ROUNDS; round += 1) {
    if (round > 0) {
      await io.pass();
      tried.push('pass');
    }
    for (const step of STEPS) {
      await io.prompt();
      const from = await io.leader();
      start ??= from;
      const before = withFrame ? await io.frame() : undefined;
      const sentBefore = io.sent ? await io.sent() : null;
      await io.act(step);
      const to = await io.leader();
      const sentAfter = io.sent ? await io.sent() : null;
      let result = stepResult(from, to, step);
      if (result === 'walked' && to === start) result = 'back';
      const sent = sentBefore === null || sentAfter === null ? '?' : sentAfter - sentBefore;
      tried.push(`${step.key} ${result} (sent ${sent})`);
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

/// A pixel of the canvas (320 x 200 game pixels) as a page position through its CSS box; with
/// the HD layout (spec 10) or the Wide layout (spec 13) in effect through the page's own map of
/// it (`__u6Layout.toCanvas`: the centre of what shows that pixel; a pixel the layout does not
/// show fails).
export async function screenPoint(page, x, y) {
  const box = await page.locator('#screen').boundingBox();
  const at = await page.evaluate(([gx, gy]) => {
    if (!['hd', 'wide'].includes(document.body.dataset.layout)) return null;
    const c = document.getElementById('screen');
    const p = globalThis.__u6Layout.toCanvas(gx, gy);
    return p ? [p[0] / c.width, p[1] / c.height] : [NaN, NaN];
  }, [x, y]);
  if (at) {
    if (!Number.isFinite(at[0])) throw new Error('the HD layout does not show that pixel');
    return [box.x + at[0] * box.width, box.y + at[1] * box.height];
  }
  return [box.x + ((x + 0.5) * box.width) / 320, box.y + ((y + 0.5) * box.height) / 200];
}

/// Canvas pixel (cx, cy) of the canvas's own size (the HD layout's 1280 x 720, the Wide
/// layout's 1920 x 1080) as a page position through its CSS box.
export async function canvasPoint(page, cx, cy) {
  const box = await page.locator('#screen').boundingBox();
  const [w, h] = await page.evaluate(() => {
    const c = document.getElementById('screen');
    return [c.width, c.height];
  });
  return [box.x + (cx * box.width) / w, box.y + (cy * box.height) / h];
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

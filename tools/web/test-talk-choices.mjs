import assert from 'node:assert/strict';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { existsSync, realpathSync } from 'node:fs';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import * as h from './harness.mjs';
import { fingerprint, hash64, hex64, latin1, packBytes } from '../i18n/translate.mjs';
// The fixture's text and pack are synthetic. Owner-supplied disks are read only by the
// shared harness/data-paths, kept in memory, and never printed or written by this runner.
const args = process.argv.slice(2);
const usage = `Usage: node tools/web/test-talk-choices.mjs --engine|--browser [--site PATH]
  --engine   Run the real wasm engine in Node; no browser, server, or screenshots.
  --browser  Run Chromium through the existing harness; tests the HD/Wide compositor.
  --site     Use an already-built site directory (default: this checkout's site).
  --help     Print this help without reading game files or launching anything.
Set U6_DATA_ROOT/U6_ORIGINAL_DIR as documented in tools/data-paths.mjs.
U6_REQUIRE_ARCHIVE=1 makes absent local disks a failure.
Optional U6_FRAMES_DIR writes browser screenshots outside every git worktree.
See tools/web/test-talk-choices.md for coverage and remaining browser validation.`;
if (args.length === 0 || args.includes('--help')) {
  console.log(usage);
  process.exit(0);
}
const engineMode = args.includes('--engine');
assert.ok(engineMode !== args.includes('--browser'), 'select exactly one execution mode');
let site = h.SITE;
for (let i = 0; i < args.length; i += 1) {
  if (['--engine', '--browser'].includes(args[i])) continue;
  assert.equal(args[i], '--site', 'known command option');
  assert.ok(args[i + 1] && !args[i + 1].startsWith('--'), 'site path supplied');
  site = resolve(args[++i]);
}
const SITE = site;
const OUT = process.env.U6_FRAMES_DIR || null;
// Resolve links before checking any optional frame path; also reject the common checkout.
function privateDirectory(dir) {
  let at = resolve(dir);
  const missing = [];
  while (!existsSync(at)) { missing.unshift(basename(at)); at = dirname(at); }
  const real = join(realpathSync(at), ...missing);
  const common = execFileSync('git', ['-C', h.REPO, 'rev-parse', '--path-format=absolute', '--git-common-dir'],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  for (const root of [h.REPO, dirname(common)]) {
    const rel = relative(realpathSync(root), real);
    assert.ok(rel === '..' || rel.startsWith(`..${sep}`) || isAbsolute(rel), 'frames outside the checkout');
  }
  for (let dir = realpathSync(at); ; dir = dirname(dir)) {
    assert.ok(!existsSync(join(dir, '.git')), 'frames outside every git worktree');
    if (dirname(dir) === dir) break;
  }
  return real;
}
const frames = OUT && !engineMode ? privateDirectory(OUT) : null;
if (frames) process.umask(0o077);
if (!h.haveHfes()) { console.log('skip - local disks unavailable'); process.exit(0); }
// Blank space between the page arrows, visible in both HD and Wide conversation panels.
const REST = [244, 100];
const KEYWORDS = ['alphabet', 'alphanumeric', 'beta', 'gamma', 'delta', 'epsilon', 'zeta', 'theta'];
function fixture() {
  const script = [0xff, 0x02, ...latin1('Zed'), 0xf1];
  const texts = [];
  const run = (en, ko) => {
    const offset = script.length;
    script.push(...latin1(en));
    texts.push({ offset, len: en.length, hash: hex64(hash64(latin1(en))), text: ko });
  };
  run('a test figure', '시험 인형');
  script.push(0xf2);
  run('Hello.', '안녕하세요.');
  const read = script.length;
  script.push(0xf7);
  const jump = () => script.push(0xb0, read & 255, read >> 8, 0, 0);
  const block = (k) => script.push(0xef, ...latin1(k), 0xf6);
  block('name');
  run(KEYWORDS.map((k) => `@${k}`).join(' ') + '.', KEYWORDS.map((k) => `@{${k}}`).join(' ') + '.');
  jump();
  block('job');
  run('Choose an answer.', '답을 선택하세요.');
  script.push(0xf8, ...latin1('yn'));
  block('y'); run('Yes accepted.', '예 선택 완료.'); jump();
  block('n'); run('No accepted.', '아니오 선택 완료.'); jump();
  block('bye'); run('Bye.', '안녕히 가세요.'); script.push(0xb6);
  for (let i = 0; i < KEYWORDS.length; i += 1) {
    block(KEYWORDS[i]); run(`Token ${i} accepted.`, `항목 ${i} 선택 완료.`); jump();
  }
  block('*'); run('Unknown.', '알 수 없습니다.'); jump();
  const items = 0x63;
  const table = [], body = [], named = [], runs = [], aliases = [];
  for (let i = 0; i < items; i += 1) {
    const at = i === 0 ? 0 : items * 4 + body.length;
    if (i > 0) {
      body.push(0, 0, 0, 0, ...script);
      named.push({ archive: 'a', item: i, hash: hex64(hash64(script)) });
      for (const t of texts) runs.push({ archive: 'a', item: i, ...t });
      for (const [keyword, word] of [['name', '이름'], ['job', '직업'], ['bye', '종료']]) {
        aliases.push({ archive: 'a', item: i, keyword, words: [word] });
      }
    }
    table.push(at & 255, (at >> 8) & 255, (at >> 16) & 255, 0);
  }
  return { archive: [...table, ...body], pack: [...packBytes({ base: fingerprint(named), runs, aliases })] };
}
const synthetic = fixture();
const BUILD_FILES = ['app.js', 'pkg/u6_web.js', 'pkg/u6_web_bg.wasm', 'pkg-wide/u6_web.js', 'pkg-wide/u6_web_bg.wasm'];
const digest = async () => Object.fromEntries(await Promise.all(BUILD_FILES.map(async (path) => [
  path, createHash('sha256').update(await readFile(join(SITE, path))).digest('hex'),
])));
const before = await digest();
let env = null;
const modules = new Map();
async function enginePage(layout, ko) {
  const dir = layout === 'wide' ? 'pkg-wide' : 'pkg';
  let loaded = modules.get(dir);
  if (!loaded) {
    const api = await import(pathToFileURL(join(SITE, dir, 'u6_web.js')).href);
    const wasm = api.initSync({ module: await readFile(join(SITE, dir, 'u6_web_bg.wasm')) });
    loaded = { api, wasm };
    modules.set(dir, loaded);
  }
  const { api, wasm } = loaded;
  const intake = new api.Game();
  const game = new api.Game();
  try {
    for (const path of h.HFES) intake.load_file(basename(path), await readFile(path));
    intake.start_game_first(17);
    let replaced = 0;
    for (let i = 0; i < intake.kept_count(); i += 1) {
      const path = intake.kept_path(i);
      const fixture = basename(path).toLowerCase() === 'converse.a';
      const bytes = fixture ? new Uint8Array(synthetic.archive)
        : new Uint8Array(wasm.memory.buffer, intake.kept_ptr(i), intake.kept_len(i)).slice();
      if (fixture) replaced += 1;
      game.load_file(path, bytes);
    }
    assert.equal(replaced, 1, 'one synthetic archive installed');
  } finally { intake.free(); }
  game.start_game_first(17);
  if (ko) {
    const pack = api.LocalePack.load(new Uint8Array(synthetic.pack));
    try { assert.equal(game.set_locale(pack), '', 'synthetic locale applied'); }
    finally { pack.free(); }
  }
  game.set_hd(true, 4);
  const page = { game, clock: 0, node: true };
  await prompt(page);
  return { page, render: { engine: true, layout, width: game.width(), hd: game.hd_on(), font: 'pixel', ko } };
}
function tick(page, n = 4) {
  for (let i = 0; i < n; i += 1) { page.clock += 20; page.game.set_clock(page.clock); page.game.step(20); }
  page.game.present();
}
function key(page, key) {
  const code = key === ' ' ? 'Space' : /^[0-9]$/.test(key) ? `Digit${key}` : `Key${key.toUpperCase()}`;
  page.game.key_down(code, key, false, page.clock);
  page.game.key_up(code);
  tick(page);
}
async function prompt(page) {
  if (!page.node) return h.waitPrompt(page);
  for (let i = 0; i < 5000; i += 1) {
    tick(page);
    if (page.game.command_prompt() && page.game.keys_pending() === 0) return;
  }
  assert.fail('engine reached command prompt');
}
async function close(page) { if (page.node) page.game.free(); else await page.context().close(); }
const receipts = [];
let current = 'startup';
let lastPage = null;
const glyphs = (page) => page.node ? Array.from(page.game.hd_glyphs())
  : page.evaluate(() => Array.from(globalThis.__shownGame.hd_glyphs()));
function rectText(g, x0, y0, x1, y1) {
  let out = '';
  for (let y = y0; y < y1; y += 8) {
    for (let x = x0; x < x1; x += 8) {
      let code = 32;
      for (let i = 0; i < g.length; i += 5) if (g[i] === x && g[i + 1] === y) code = g[i + 2] < 0x10000 ? g[i + 2] : 32;
      out += String.fromCharCode(code);
    }
    out += '\n';
  }
  return out;
}
const grid = (g) => Array.from({ length: 9 }, (_, i) => rectText(g,
  176 + i % 3 * 48, 72 + Math.floor(i / 3) * 8,
  216 + i % 3 * 48, 80 + Math.floor(i / 3) * 8).trim());
const move = async (page, x, y, buttons = 0) => {
  if (page.node) { page.game.pointer(x, y, buttons); tick(page); }
  else { await page.mouse.move(...await h.screenPoint(page, x, y)); await h.waitFrames(page, 3); }
};
const click = async (page, x, y) => {
  if (page.node) { page.game.pointer(x, y, 1); page.game.pointer(x, y, 0); tick(page); }
  else { await page.mouse.click(...await h.screenPoint(page, x, y)); await h.waitFrames(page, 3); }
};
const targets = (page) => page.node ? Array.from(page.game.talk_target())
  : page.evaluate(() => Array.from(globalThis.__shownGame.talk_target()));
const press = async (page, at, down) => {
  if (page.node) { page.game.pointer(...at, down ? 1 : 0); tick(page); }
  else { await page.mouse[down ? 'down' : 'up'](); await h.waitFrames(page, 3); }
};
function choiceInk(g, at) {
  const inks = [];
  for (let i = 0; i < g.length; i += 5) {
    if (g[i] >= at[0] - 20 && g[i] < at[0] + 20 && g[i + 1] === at[1] - 4 && g[i + 2] !== 32) inks.push(g[i + 3]);
  }
  assert.ok(inks.length > 0 && inks.every((ink) => ink === inks[0]), 'choice has one text ink');
  return inks[0];
}
async function reading(page) {
  for (let i = 0; i < 160; i += 1) {
    if (page.node) tick(page); else await h.waitFrames(page, 2);
    const r = page.node ? { r: page.game.reading_by_click(), q: page.game.keys_pending(), owner: page.game.keys_owned() }
      : await page.evaluate(() => ({ r: globalThis.__shownGame.reading_by_click(), q: globalThis.__shownGame.keys_pending(), owner: globalThis.__owned }));
    if (r.r && r.q === 0) return;
    if (r.owner === 1 && !r.r) { if (page.node) key(page, ' '); else await page.keyboard.press('Space'); }
  }
  assert.fail('the expected choice reader reached');
}
async function talk(page) {
  await prompt(page);
  if (page.node) { key(page, 't'); tick(page); key(page, '2'); }
  else { await page.focus('#screen'); await page.keyboard.press('t'); await h.waitTaken(page); await page.keyboard.press('2'); }
  await reading(page);
  await move(page, ...REST);
}
async function headerPixels(page, save) {
  if (page.node) {
    const rgba = page.game.frame_rgba(), width = page.game.width(), pixels = [];
    for (let y = 16; y < 72; y += 1) for (let x = 176; x < 312; x += 1) {
      pixels.push(...rgba.slice((y * width + x) * 4, (y * width + x + 1) * 4));
    }
    if (save) { page.header = pixels; return true; }
    return pixels.length === page.header.length && pixels.every((v, i) => v === page.header[i]);
  }
  return page.evaluate((keep) => {
    const canvas = document.getElementById('screen');
    const a = globalThis.__u6Layout.toCanvas(176, 16), b = globalThis.__u6Layout.toCanvas(311, 71);
    if (!a || !b) return false;
    // toCanvas gives pixel centres. Include both half-pixels at the rectangle's edges.
    const sx = (b[0] - a[0]) / 135, sy = (b[1] - a[1]) / 55;
    const x = Math.floor(a[0] - sx / 2), y = Math.floor(a[1] - sy / 2);
    const w = Math.ceil(b[0] + sx / 2) - x, h = Math.ceil(b[1] + sy / 2) - y;
    const data = Array.from(canvas.getContext('2d').getImageData(x, y, w, h).data);
    if (keep) { globalThis.__choiceHeader = data; return true; }
    return data.length === globalThis.__choiceHeader.length && data.every((v, i) => v === globalThis.__choiceHeader[i]);
  }, save);
}
async function shot(page, label) {
  if (!frames || page.node) return;
  await h.waitFrames(page, 3);
  await mkdir(frames, { recursive: true, mode: 0o700 });
  await page.locator('#screen').screenshot({ path: join(frames, `${current}-${label}.png`) });
}
// A bounded, scalar-only witness: no input values, text, pixels, or game records.
async function watchChoicePress(page) {
  if (page.node) return;
  await page.evaluate(() => {
    const game = globalThis.__shownGame;
    const node = (el) => ['screen', 'ime', 'settings'].includes(el?.id) ? el.id : 'other';
    const trace = [];
    const record = (kind, details = {}) => {
      trace.push({ kind, ...details, focus: node(document.activeElement),
        targets: Array.from(game.talk_target()) });
      if (trace.length > 64) trace.shift();
    };
    const pointer = game.pointer;
    game.pointer = function (x, y, buttons) {
      record('engine-pointer', { x, y, buttons });
      return pointer.call(this, x, y, buttons);
    };
    const events = ['pointerdown', 'pointerup', 'pointercancel', 'gotpointercapture', 'lostpointercapture', 'blur', 'focus'];
    const listen = (event) => record(event.type, { target: node(event.target), related: node(event.relatedTarget) });
    for (const type of events) document.addEventListener(type, listen, true);
    globalThis.__choicePressWitness = () => {
      record('observed', { selectionCollapsed: window.getSelection()?.isCollapsed ?? true });
      game.pointer = pointer;
      for (const type of events) document.removeEventListener(type, listen, true);
      return trace;
    };
  });
}
async function finishChoicePress(page) {
  if (page.node) return;
  const trace = await page.evaluate(() => {
    const finish = globalThis.__choicePressWitness;
    delete globalThis.__choicePressWitness;
    return finish ? finish() : null;
  });
  if (trace) console.log(`# choice press ${current}: ${JSON.stringify(trace)}`);
}
async function target(page, x, y) { await move(page, x, y); return (await targets(page))[0]; }
async function visibleChoiceTargets(page) {
  const ids = [];
  for (let i = 0; i < 9; i += 1) {
    ids.push(await target(page, 196 + i % 3 * 48, 76 + Math.floor(i / 3) * 8));
  }
  await move(page, ...REST);
  return ids;
}
async function find(page, id) {
  for (let p = 0; p < 3; p += 1) {
    for (let i = 0; i < 9; i += 1) {
      const at = [196 + i % 3 * 48, 76 + Math.floor(i / 3) * 8];
      if (await target(page, ...at) === id) return at;
    }
    if (await target(page, 308, 100) !== 0x20001) break;
    await click(page, 308, 100);
  }
  assert.fail('choice stable ID available');
}
async function select(page, id) {
  await click(page, ...await find(page, id));
  await reading(page);
  await move(page, ...REST);
}
async function cancelledChoicePresses(page) {
  if (page.node) return;
  // Real DOM focus and pointer capture, plus the browser's cancellation notifications.
  // Each interrupted press stays cancelled when the held pointer returns to the same choice.
  for (const kind of ['page-focus', 'window-blur', 'pointercancel', 'lostcapture']) {
    const at = await find(page, 3);
    await page.focus('#screen');
    await move(page, ...at);
    const before = await message(page);
    await page.evaluate(() => document.getElementById('screen').addEventListener('pointerdown',
      (event) => { globalThis.__choicePointerId = event.pointerId; }, { once: true }));
    await press(page, at, true);
    assert.equal((await targets(page))[1], 3, `${kind}: a valid choice is held`);
    // Capture is acquired on the next pointer event, so move while still on the same label.
    await move(page, at[0] + 1, at[1], 1);
    if (kind === 'page-focus') await page.focus('#controls summary');
    else if (kind === 'window-blur') await page.evaluate(() => window.dispatchEvent(new Event('blur')));
    else if (kind === 'pointercancel') await page.evaluate(() => document.getElementById('screen')
      .dispatchEvent(new PointerEvent('pointercancel', { pointerId: globalThis.__choicePointerId, buttons: 0 })));
    else await page.evaluate(() => document.getElementById('screen').releasePointerCapture(globalThis.__choicePointerId));
    await move(page, ...at, 1);
    assert.equal((await targets(page))[1], -1, `${kind}: the held choice is cancelled`);
    await press(page, at, false);
    assert.equal(await message(page), before, `${kind}: release submits no answer`);
  }
  await page.focus('#screen');
  await h.waitFrames(page, 3);
}
async function open(layout, ko) {
  if (engineMode) return enginePage(layout, ko);
  const { page } = await h.openPage(env, { layout, font: 'hd', setup: { lang: ko ? 'ko' : 'en' } });
  lastPage = page;
  await page.setViewportSize({ width: 1400, height: 950 });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await h.waitSettled(page);
  await h.waitPrompt(page);
  if (ko) {
    await h.dropFiles(page, [], [{ name: 'synthetic-choices.u6lp', bytes: synthetic.pack }]);
    await h.waitSettled(page);
    await h.waitPrompt(page);
  }
  assert.equal(await h.replaceKept(page, 'converse.a', synthetic.archive), 1, 'archive fixture installed');
  await page.reload();
  await h.waitSettled(page);
  await h.waitPrompt(page);
  const render = await page.evaluate(() => ({
    layout: document.body.dataset.layout, fontOptions: document.querySelectorAll('input[name="font"]').length,
    hd: globalThis.__shownGame.hd_on(), listed: globalThis.__shownGame.hd_glyphs().length, width: globalThis.__shownGame.width(),
    starts: globalThis.__starts.map((s) => s.locale),
  }));
  assert.equal(render.layout, layout);
  assert.equal(render.fontOptions, 0, 'no font choice remains');
  assert.ok(render.hd && render.listed > 0, 'pixel text retains actual source-plane diagnostics');
  if (ko) assert.ok(render.starts.includes('on'), 'synthetic locale active in browser');
  return { page, render };
}
const message = async (page) => rectText(await glyphs(page), 176, 112, 312, 192).replace(/\s/g, '');
async function scenario(page, ko) {
  await talk(page);
  const initial = grid(await glyphs(page));
  const unusedInk = choiceInk(await glyphs(page), [244, 76]);
  assert.equal(initial.filter(Boolean).length, 3, 'three initial choices');
  assert.equal(await target(page, 180, 100), -1, 'no previous page arrow initially');
  assert.equal(await target(page, 308, 100), -1, 'no page arrow initially');
  await move(page, ...REST);
  await shot(page, 'initial');
  await select(page, 1);
  assert.equal(grid(await glyphs(page)).filter(Boolean).length, 9, 'full three column grid');
  assert.equal(await target(page, 308, 100), 0x20001, 'overflow page arrow');
  await move(page, ...REST);
  await shot(page, 'overflow');
  assert.ok(await headerPixels(page, true), 'header snapshot captured');
  const a = await find(page, 3);
  let preview = rectText(await glyphs(page), 176, 16, 312, 72).replace(/\s/g, '');
  assert.ok(preview.includes('alphabet'), 'first full preview');
  await shot(page, 'full-preview-a');
  const label = (g, at) => rectText(g, at[0] - 20, at[1] - 4, at[0] + 20, at[1] + 4).trim();
  const aLabel = label(await glyphs(page), a);
  const b = await find(page, 4);
  assert.equal(aLabel, label(await glyphs(page), b), 'same-prefix labels identically abbreviated');
  assert.ok(aLabel.endsWith('...'), 'long label has clear ellipsis');
  preview = rectText(await glyphs(page), 176, 16, 312, 72).replace(/\s/g, '');
  assert.ok(preview.includes('alphanumeric'), 'second full preview');
  await shot(page, 'full-preview-b');
  await move(page, ...REST);
  assert.ok(await headerPixels(page, false), 'portrait and name pixels fully restored');

  // Two visually identical short labels are separate stable targets. Releasing on the
  // other label cancels; selecting each later must execute that word's distinct response.
  const beforeCancel = await message(page);
  await watchChoicePress(page);
  await move(page, ...a);
  await press(page, a, true);
  await finishChoicePress(page);
  assert.equal((await targets(page))[1], 3, 'pressed stable identity');
  await move(page, ...b, 1);
  await press(page, b, false);
  assert.equal((await targets(page))[1], -1, 'release cancels the pending press');
  assert.equal(await message(page), beforeCancel, 'cancelled press submits no answer');
  await cancelledChoicePresses(page);
  await select(page, 3);
  assert.ok((await message(page)).includes(ko ? '항목0선택완료.' : 'Token0accepted.'), 'first exact word selected');
  assert.notEqual(await target(page, ...a), 3, 'used word moves from its old visual slot');
  await select(page, 4);
  assert.ok((await message(page)).includes(ko ? '항목1선택완료.' : 'Token1accepted.'), 'second exact word selected');
  const pageOneTargets = await visibleChoiceTargets(page);
  const pageOneGrid = grid(await glyphs(page));
  assert.equal(pageOneTargets[0], 2, 'exit remains fixed on page one');
  await click(page, 308, 100);
  assert.equal(await target(page, 196, 76), 2, 'exit remains fixed on page two');
  const usedAt = await find(page, 3);
  await move(page, ...REST);
  const usedInk = choiceInk(await glyphs(page), usedAt);
  const rgb = [usedInk >> 16 & 255, usedInk >> 8 & 255, usedInk & 255];
  assert.notEqual(usedInk, unusedInk, 'used word has a different text ink');
  // The original Amiga palette is coarse: permit two of its 17-level channel steps.
  assert.ok(Math.max(...rgb) - Math.min(...rgb) <= 34 && rgb[0] > 45 && rgb[0] < 240, 'used ink is neutral grey');
  await shot(page, 'page-two-used-grey');
  const pageTwoTargets = await visibleChoiceTargets(page);
  const pageTwoGrid = grid(await glyphs(page));
  const beforePaging = await message(page);
  assert.notDeepEqual(pageTwoTargets, pageOneTargets, 'overflow shows a distinct set of choices');
  assert.equal(await target(page, 180, 100), 0x20000, 'previous page arrow is a real hit target');
  await click(page, 180, 100);
  assert.deepEqual(await visibleChoiceTargets(page), pageOneTargets, 'left click restores the same page-one choice IDs');
  assert.deepEqual(grid(await glyphs(page)), pageOneGrid, 'left click restores the page-one labels');
  assert.equal(await target(page, 196, 76), 2, 'exit stays fixed after paging left');
  assert.equal(await message(page), beforePaging, 'paging left submits no conversation answer');
  await move(page, ...REST);
  await shot(page, 'page-one-return');
  assert.equal(await target(page, 308, 100), 0x20001, 'next page arrow remains a real hit target');
  await click(page, 308, 100);
  assert.deepEqual(await visibleChoiceTargets(page), pageTwoTargets, 'right click restores the same page-two choice IDs');
  assert.deepEqual(grid(await glyphs(page)), pageTwoGrid, 'right click restores the page-two labels');
  assert.equal(await target(page, 196, 76), 2, 'exit stays fixed after paging right again');
  assert.equal(await target(page, ...usedAt), 3, 'used choice retains its page-two location');
  await move(page, ...REST);
  assert.equal(choiceInk(await glyphs(page), usedAt), usedInk, 'page round trip preserves the used grey ink');
  assert.equal(await message(page), beforePaging, 'page round trip submits no conversation answer');
  await shot(page, 'page-two-return-used-grey');
  await select(page, 3);
  assert.ok((await message(page)).includes(ko ? '항목0선택완료.' : 'Token0accepted.'), 'used choice reselectable');
  await shot(page, 'reselected');

  await select(page, 0);
  assert.ok((await message(page)).includes(ko ? '답을선택하세요.' : 'Chooseananswer.'), 'question remains visible');
  await shot(page, 'yes-no');
  const yes = ko ? 260 : 252, no = ko ? 292 : 300;
  for (const [x, id, answer, label] of [
    [yes, 0x30001, ko ? '예선택완료.' : 'Yesaccepted.', 'yes'],
    [no, 0x30000, ko ? '아니오선택완료.' : 'Noaccepted.', 'no'],
  ]) {
    assert.equal(await target(page, x, 188), id, 'matching restricted answer target');
    await click(page, x, 188);
    await reading(page);
    await move(page, ...REST);
    assert.ok((await message(page)).includes(answer), 'restricted click executes matching answer');
    await shot(page, `${label}-result`);
    if (label === 'yes') await select(page, 0);
  }
  await click(page, ...await find(page, 2));
  await prompt(page);
  await talk(page);
  assert.deepEqual(grid(await glyphs(page)), initial, 'new conversation resets choices');
  assert.equal(choiceInk(await glyphs(page), [244, 76]), unusedInk, 'new conversation clears used ink');
  assert.equal(await target(page, 180, 100), -1, 'no previous page arrow after reset');
  assert.equal(await target(page, 308, 100), -1, 'no overflow after reset');
  await move(page, ...REST);
  await shot(page, 'reset-reentry');
  if (ko && !page.node) {
    assert.equal(await page.evaluate(() => document.activeElement?.id), 'ime', 'Korean line retains IME focus');
    const cdp = await page.context().newCDPSession(page);
    try {
      await cdp.send('Input.imeSetComposition', { text: '이름', selectionStart: 2, selectionEnd: 2 });
      await cdp.send('Input.insertText', { text: '이름' });
      await page.keyboard.press('Enter');
      await reading(page);
      assert.equal(grid(await glyphs(page)).filter(Boolean).length, 9, 'typed Korean alias discovers the same choices');
      assert.equal(await target(page, 308, 100), 0x20001, 'typed Korean alias reaches overflow');
      await select(page, 0);
      assert.equal(await target(page, 260, 188), 0x30001, 'click after typed Korean reaches the real yes/no reader');
      await click(page, 260, 188);
      await reading(page);
      assert.ok((await message(page)).includes('예선택완료.'), 'yes click after typed Korean is accepted');
    } finally { await cdp.detach(); }
  }
}
try {
  if (!engineMode) env = await h.setup(SITE);
  for (const layout of ['hd', 'wide']) for (const ko of [false, true]) {
    current = `synthetic-${ko ? 'ko' : 'en'}-${layout}`;
    console.log(`start ${current}`);
    const { page, render } = await open(layout, ko);
    lastPage = page;
    await scenario(page, ko);
    receipts.push({ case: current, render, checks: [
      'initial', 'overflow', 'same-prefix-preview', 'stable-id-cancel', 'exact-input',
      'used-grey', 'page-left', 'page-right-return', 'reselect', 'yes', 'no', 'reset-reentry',
      ...(!page.node ? ['toolbar-focus-cancel', 'window-blur-cancel', 'pointercancel', 'lostcapture', ...(ko ? ['typed-korean-then-click'] : [])] : []),
    ] });
    console.log(`pass ${current}`);
    await close(page);
    lastPage = null;
  }
} catch (err) {
  if (lastPage && !lastPage.node) await shot(lastPage, 'failure').catch(() => {});
  const places = String(err.stack || '').split('\n').filter((line) => /^\s*at /.test(line) && line.includes('/test-talk-choices.mjs:')).map((line) => line.replace(/file:\/\/[^ )]+\/test-talk-choices.mjs:/, 'test-talk-choices.mjs:'));
  console.error([...h.failureLines(err), ...places].join('\n'));
  process.exitCode = 1;
} finally {
  if (lastPage) await close(lastPage);
  if (env) await env.close();
  assert.deepEqual(await digest(), before, 'tested build unchanged');
  if (frames) {
    await mkdir(frames, { recursive: true, mode: 0o700 });
    await writeFile(join(frames, 'choices-receipt.json'), JSON.stringify({ mode: 'browser', builds: before, receipts }, null, 2));
  }
  console.log(`# tested build SHA-256: ${JSON.stringify(before)}`);
  console.log(`# ${engineMode ? 'engine' : 'browser'} choice tests: ${receipts.length}/4 passed`);
}
// h.setup can fail after opening its server but before returning a close handle.
// Finish this runner after all receipts are written, also in that setup-failure case.
process.exit(process.exitCode || 0);

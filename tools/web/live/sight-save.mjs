// Spec 18: run the real launcher, Save a visible remembered sight, reload, choose Journey
// Onward, and compare the same pixels on the same cell. Uses a fresh browser context only.
// Game files stay in that context; screenshots must be written outside the repository.
//
// node tools/web/live/sight-save.mjs --url http://127.0.0.1:8787 --out /tmp/u6-sight
// Optional: --zip PATH (or U6_ARCHIVE), --layout original|wide. No files are uploaded.
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { existsSync, realpathSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { homedir, tmpdir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import * as h from '../harness.mjs';

const options = new Map();
for (let i = 2; i < process.argv.length; i += 2) {
  const key = process.argv[i];
  assert.ok(['--url', '--out', '--zip', '--layout'].includes(key) && process.argv[i + 1], 'named option and value required');
  options.set(key, process.argv[i + 1]);
}
const url = options.get('--url');
assert.ok(url && ['http:', 'https:'].includes(new URL(url).protocol), '--url is required');
function realDir(dir) {
  let at = resolve(dir);
  const rest = [];
  while (!existsSync(at) && dirname(at) !== at) {
    rest.unshift(basename(at));
    at = dirname(at);
  }
  return join(realpathSync(at), ...rest);
}
const out = realDir(options.get('--out') ?? join(tmpdir(), 'ultima6-sight-save'));
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const common = execFileSync('git', ['-C', repo, 'rev-parse', '--path-format=absolute', '--git-common-dir'],
  { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
assert.ok(isAbsolute(common) && existsSync(common), 'the main checkout root is known');
for (const root of [repo, dirname(common)]) {
  const within = relative(realpathSync(root), out);
  assert.ok(within === '..' || within.startsWith(`..${sep}`) || isAbsolute(within),
    'screenshots must stay outside the repository and its main checkout');
}
const zip = options.get('--zip') ?? process.env.U6_ARCHIVE ?? join(homedir(), 'Games/ultima6/original/ultima-vi.zip');
assert.ok(existsSync(zip), 'the local game archive is required');
const layout = options.get('--layout') ?? 'original';
assert.ok(['original', 'wide'].includes(layout), 'layout is original or wide');
await mkdir(out, { recursive: true });

// Observation only: retain the presented Game and count actual engine steps for bounded key
// waits. In particular, do not replace start with start_game_first: that masks the reload bug
// in the former live check (a reload starts u; the player must choose Journey Onward).
const observer = `\n{
  globalThis.__ticks = 0;
  const step = Game.prototype.step, present = Game.prototype.present;
  Game.prototype.step = function(ms) { const result = step.call(this, ms); globalThis.__ticks += 1; return result; };
  Game.prototype.present = function() { const result = present.call(this); globalThis.__shownGame = this; return result; };
}\n`;
let browser;
let page;
let phase = 'launch';
try {
  browser = await chromium.launch();
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, locale: 'en-US' });
  await context.route(/\/pkg(-wide)?\/u6_web\.js$/, async (route) => {
    const response = await route.fetch();
    await route.fulfill({ response, body: (await response.text()) + observer });
  });
  await context.addInitScript((chosen) => {
    localStorage.setItem('u6.speed', 'max');
    localStorage.setItem('u6.layout', chosen);
    localStorage.setItem('u6.memory', 'on');
  }, layout);
  page = await context.newPage();
  let errors = 0;
  page.on('pageerror', () => { errors += 1; });
  const shot = async (name) => page.locator('#screen').screenshot({ path: join(out, `${name}.png`) });

  async function acceptSetup() {
    await page.waitForFunction(() => {
      const setup = document.getElementById('setup');
      return (setup && !setup.hidden && !document.getElementById('setup-ok').disabled)
        || (document.body.dataset.state === 'running' && globalThis.__shownGame);
    }, null, { timeout: 120_000 });
    if (await page.locator('#setup').isVisible()) {
      await page.check('input[name="setup-lang"][value="en"]');
      await page.check('input[name="setup-hd"][value=""]');
      await page.uncheck('#setup-voices');
      await page.click('#setup-ok');
    }
    await h.waitSettled(page, 120_000);
  }

  async function menu() {
    await h.waitProgram(page, 'u', 120_000);
    await h.waitVisible(page, 120_000);
    await page.focus('#screen');
    await h.press(page, 'Escape', 120);
  }

  async function createCharacter() {
    await menu();
    await h.press(page, 'c', 120);
    for (const c of 'Tester') await h.press(page, c, 30);
    for (const [key, ticks] of [['Enter', 60], ['m', 60], ['p', 30], ['c', 60]]) await h.press(page, key, ticks);
    for (let i = 0; i < 5; i += 1) await h.press(page, 'Space', 200);
    for (let i = 0; i < 7; i += 1) await h.press(page, i % 2 ? 'b' : 'a', 220);
    await h.press(page, 'Space', 200);
    for (let i = 0; i < 4; i += 1) await h.press(page, 'Space', 120);
    await h.waitTicks(page, 60);
    await h.press(page, 'Escape', 200);
    await h.waitProgram(page, 'game', 120_000);
    await h.waitPrompt(page, 120_000);
  }

  const leader = () => page.evaluate(() => [...globalThis.__shownGame.leader_position()].slice(0, 3));
  async function walkSouth(cells) {
    const [sx, sy] = await leader();
    for (let tries = 0; tries < 120; tries += 1) {
      phase = `door route attempt ${tries + 1}`;
      await h.waitPrompt(page, 120_000);
      const [x, y] = await leader();
      if (x === sx && y === sy + cells) return;
      await page.keyboard.press(y < sy + cells ? 'ArrowDown' : x < sx ? 'ArrowRight' : x > sx ? 'ArrowLeft' : 'ArrowUp');
      await h.waitTaken(page, 120_000);
      await h.waitPrompt(page, 120_000);
      const [nx, ny] = await leader();
      if (nx === x && ny === y) await h.passTurn(page);
      if (tries % 10 === 9) console.log(`door route: ${tries + 1} attempts`);
    }
    assert.fail('the door route did not finish');
  }

  async function useSouth() {
    await h.waitPrompt(page, 120_000);
    for (const key of ['u', 'ArrowDown', 'Enter']) await h.press(page, key, 20);
    await h.waitPrompt(page, 120_000);
  }

  // Content is retained only in the isolated browser. Only counts cross back into the log.
  async function rememberPixels() {
    return page.evaluate(() => {
      const g = globalThis.__shownGame;
      const f = g.frame_rgba();
      const w = g.width(), rows = f.length / 4 / w;
      const [px, py, pw, ph] = g.hd_pointer();
      const pixels = [];
      for (let y = 0; y < rows; y += 1) {
        for (let x = 0; x < w; x += 1) {
          if (x >= px && x < px + pw && y >= py && y < py + ph) continue;
          if (((g.hd_source_at(x, y)[5] ?? 0) & 0x80) === 0) continue;
          const i = (y * w + x) * 4;
          pixels.push([x, y, f[i], f[i + 1], f[i + 2]]);
        }
      }
      globalThis.__sightReceipt = pixels;
      return { on: g.sight_memory(), count: pixels.length, grey: pixels.filter((p) => p[2] === p[3] && p[3] === p[4]).length, lit: pixels.filter((p) => p[2] + p[3] + p[4] > 0).length };
    });
  }

  phase = 'initial setup';
  await page.goto(url, { waitUntil: 'load' });
  await acceptSetup();
  await page.setInputFiles('#picker', zip);
  await h.waitSettled(page, 300_000);
  phase = 'character creation';
  await createCharacter();
  await page.focus('#screen');
  console.log('PASS real launcher and character creation');
  await walkSouth(10);
  console.log('PASS door route');
  phase = 'open and close door';
  await useSouth();
  await h.waitTicks(page, 2);
  await useSouth();
  await h.waitTicks(page, 2);
  const at = await leader();
  phase = 'Save';
  const savedBefore = await page.evaluate(() => Number(document.body.dataset.saved ?? 0));
  await h.press(page, 'Control+s', 30);
  await h.press(page, 'y', 60);
  await page.waitForFunction((before) => Number(document.body.dataset.saved ?? 0) > before,
    savedBefore, { timeout: 120_000 });
  await h.waitSettled(page, 120_000);
  const before = await rememberPixels();
  console.log(`before reload: ${JSON.stringify(before)}`);
  assert.ok(before.on && before.count >= 256 && before.grey === before.count && before.lit > 0, 'a visible grey remembered sight before the Save reload');
  // Keep the pixel baseline in this context's sessionStorage across the reload, never on disk.
  await page.evaluate(() => sessionStorage.setItem('sight-receipt', JSON.stringify(globalThis.__sightReceipt)));
  const stored = await page.evaluate(() => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onerror = () => fail(new Error('save database open'));
    req.onsuccess = () => {
      const db = req.result;
      const get = db.transaction('files').objectStore('files').get('save:savegame/u6sight');
      get.onsuccess = () => { db.close(); done(Boolean(get.result)); };
      get.onerror = () => fail(new Error('saved sight read'));
    };
  }));
  assert.ok(stored, 'the Save stored its remembered sight');
  await shot('before-reload');

  phase = 'reload and Journey Onward';
  await page.reload({ waitUntil: 'load' });
  await acceptSetup();
  await menu();
  await h.press(page, 'j', 30);
  await h.waitProgram(page, 'game', 120_000);
  await h.waitPrompt(page, 120_000);
  assert.ok(JSON.stringify(await leader()) === JSON.stringify(at), 'Journey Onward resumed on the saved cell');
  const after = await page.evaluate(() => {
    const old = JSON.parse(sessionStorage.getItem('sight-receipt'));
    const g = globalThis.__shownGame, f = g.frame_rgba(), w = g.width();
    let named = 0, same = 0;
    for (const [x, y, r, green, b] of old) {
      const i = (y * w + x) * 4;
      if (((g.hd_source_at(x, y)[5] ?? 0) & 0x80) !== 0) named += 1;
      if (f[i] === r && f[i + 1] === green && f[i + 2] === b) same += 1;
    }
    return { count: old.length, named, same };
  });
  assert.ok(after.named === after.count && after.same === after.count, 'every sampled remembered pixel returned with its saved grey');
  assert.equal(errors, 0, 'no page errors');
  await shot('after-reload');
  console.log(`PASS ${layout}: ${before.count} remembered samples before Save, ${after.named} named and ${after.same} identical after reload and Journey Onward; no page errors`);

  // Negative control in this disposable context: an original-format save with no sidecar must
  // not acquire the earlier memory. The game position is unchanged and those pixels go black.
  phase = 'negative control';
  await page.evaluate(() => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onerror = () => fail(new Error('control database open'));
    req.onsuccess = () => {
      const db = req.result;
      const tx = db.transaction('files', 'readwrite');
      tx.objectStore('files').delete('save:savegame/u6sight');
      tx.oncomplete = () => { db.close(); done(); };
      tx.onerror = () => fail(new Error('control sidecar deletion'));
    };
  }));
  await page.reload({ waitUntil: 'load' });
  await acceptSetup();
  await menu();
  await h.press(page, 'j', 30);
  await h.waitProgram(page, 'game', 120_000);
  await h.waitPrompt(page, 120_000);
  assert.ok(JSON.stringify(await leader()) === JSON.stringify(at), 'the control resumed on the saved cell');
  const control = await page.evaluate(() => {
    const old = JSON.parse(sessionStorage.getItem('sight-receipt'));
    const g = globalThis.__shownGame, f = g.frame_rgba(), w = g.width();
    let black = 0, named = 0;
    for (const [x, y] of old) {
      const i = (y * w + x) * 4;
      if (f[i] + f[i + 1] + f[i + 2] === 0) black += 1;
      if (((g.hd_source_at(x, y)[5] ?? 0) & 0x80) !== 0) named += 1;
    }
    return { black, named, count: old.length };
  });
  assert.ok(control.black === control.count && control.named === 0, 'without the sidecar, every sampled pixel is black and no memory is drawn');
  assert.equal(errors, 0, 'no page errors in the control');
  await shot('without-sidecar');
  console.log(`PASS ${layout} negative control: ${control.black} samples black, ${control.named} remembered without the stored sight`);
} catch (error) {
  console.error(`FAIL sight-save at ${phase}: ${h.failureLines(error).join(' | ')}`);
  if (page) {
    await page.locator('#screen').screenshot({ path: join(out, 'failure.png') }).catch(() => {});
    const state = await page.evaluate(() => {
      const c = document.getElementById('screen');
      return { state: document.body.dataset.state, saved: document.body.dataset.saved, program: c?.dataset.program, prompt: c?.dataset.prompt, keys: c?.dataset.keys, setup: !document.getElementById('setup')?.hidden };
    }).catch(() => null);
    if (state) console.error(JSON.stringify(state));
  }
  process.exitCode = 1;
} finally {
  if (browser) await browser.close();
}

// Spec 16 E1: local build, owner's archive + runtime end-condition save, real launcher.
// The fixture is produced by ending::tests::write_runtime_ending_fixture. It is not a user's
// completed save. Original bytes/art stay in the local browser and private output directory.
// node tools/web/live/ending.mjs --fixture /private/fixture --out /private/receipt --layout original|wide [--zip PATH]
import assert from 'node:assert/strict';
import { mkdir, readFile, readdir } from 'node:fs/promises';
import { existsSync, realpathSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { serve } from '../serve.mjs';
import * as h from '../harness.mjs';

const args = new Map();
for (let i = 2; i < process.argv.length; i += 2) {
  assert.ok(['--fixture', '--out', '--layout', '--zip'].includes(process.argv[i]) && process.argv[i + 1]);
  args.set(process.argv[i], process.argv[i + 1]);
}
const fixture = args.get('--fixture'), output = args.get('--out');
assert.ok(fixture && output, 'fixture and private output required');
await mkdir(output, { recursive: true });
const out = realpathSync(output);
for (let p = out;; p = dirname(p)) {
  assert.ok(!existsSync(join(p, '.git')), 'receipt images must stay outside every Git checkout');
  if (dirname(p) === p) break;
}
const layout = args.get('--layout') ?? 'original';
assert.ok(['original', 'wide'].includes(layout));
const zip = args.get('--zip') ?? process.env.U6_ARCHIVE ?? join(homedir(), 'Games/ultima6/original/ultima-vi.zip');
const extra = [];
for (const name of await readdir(join(fixture, 'savegame'))) {
  extra.push({ name: `PLAYER/SAVEGAME/${name}`, bytes: [...await readFile(join(fixture, 'savegame', name))] });
}
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
// Observation only: retain the actual presented game and count its engine steps. The launcher,
// save loading and Use command all run through the unchanged page and engine entry points.
const observer = `\n{
  globalThis.__ticks = 0;
  const step = Game.prototype.step, present = Game.prototype.present;
  Game.prototype.step = function(ms) { const r = step.call(this, ms); globalThis.__ticks += 1; return r; };
  Game.prototype.present = function() { const r = present.call(this); globalThis.__shownGame = this; return r; };
}\n`;
let server, browser, page;
let phase = 'launch';
try {
  server = await serve({ root: join(repo, 'site') });
  browser = await chromium.launch();
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, locale: 'en-US' });
  await context.route(/\/pkg(-wide)?\/u6_web\.js$/, async route => {
    const response = await route.fetch();
    await route.fulfill({ response, body: (await response.text()) + observer });
  });
  await context.addInitScript(choice => {
    localStorage.setItem('u6.layout', choice);
    localStorage.setItem('u6.speed', 'max');
  }, layout);
  page = await context.newPage();
  let errors = 0;
  page.on('pageerror', () => errors++);
  await page.goto(server.url);
  phase = 'settings';
  await h.waitSettled(page, 120_000);
  await h.chooseSetting(page, 'language', 'en', {close:true});
  await h.chooseSetting(page, 'pack', '', {close:true});
  await page.reload();
  await h.waitSettled(page, 120_000);
  await h.dropFiles(page, [zip], extra);
  // The explicit new page start applies the language before importing the test fixture.
  async function journey() {
    // Setup/restore may replace the running launcher; do not send keys to that old instance.
    await h.waitSettled(page, 120_000);
    await h.waitProgram(page, 'u', 120_000);
    // The launcher program exists before its opening accepts input. Wait for its first
    // visible frame, then verify the main menu's own key claim before choosing Journey.
    // These are observations only: the actual Escape and J still take the normal path.
    await h.waitVisible(page, 120_000);
    await page.focus('#screen');
    await h.press(page, 'Escape', 120);
    await page.waitForFunction(() => globalThis.__shownGame.program() === 'u'
      && globalThis.__shownGame.keys_owned() === 5, null, { timeout: 120_000 });
    console.log(`PASS ${layout}: actual launcher menu ready before Journey input`);
    await h.press(page, 'j', 180);
    await h.waitProgram(page, 'game', 120_000);
    await h.waitPrompt(page, 120_000);
    const pos = await page.evaluate(() => [...__shownGame.leader_position()].slice(0, 3));
    assert.ok(pos[0] === 0x39b && pos[1] === 0x354 && pos[2] === 0,
      'constructed save starts beside completed shrine');
  }
  phase = 'journey';
  await journey();
  phase = 'use';
  for (const key of ['u', 'ArrowDown', 'Enter']) await h.press(page, key, 30);
  await h.press(page, 'Space', 60);
  await h.waitProgram(page, 'end', 120_000);
  console.log(`PASS ${layout}: real launcher, loaded runtime save, Use cube -> end`);
  const settleOrEnd = async n => {
    const from = await page.evaluate(() => __ticks);
    await page.waitForFunction(t => __ticks >= t || __shownGame.finished(), from + n, { timeout: 120_000 });
  };
  let shots = 0;
  for (let n = 0; n < 100; n++) {
    phase = `ending ${n}`;
    const program = await page.evaluate(() => __shownGame.program());
    if (program === 'ended') break;
    assert.equal(program, 'end');
    await page.locator('#screen').screenshot({ path: join(out, `scene-${String(n).padStart(2, '0')}.png`) });
    shots++;
    if (n % 3 === 1) {
      const box = await page.locator('#screen').boundingBox();
      await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
    } else {
      await page.keyboard.press('Space');
    }
    await settleOrEnd(180);
  }
  assert.equal(await page.evaluate(() => __shownGame.program()), 'ended', 'final input exits');
  await page.locator('#screen').screenshot({ path: join(out, 'final.png') });
  assert.equal(errors, 0, 'no page errors');
  assert.ok(shots >= 8, 'multiple scenes captured');
  console.log(`PASS ${layout}: ${shots} scene receipts, keyboard + mouse, final key -> ended, no page errors`);

  // A new browser game reads the same unchanged fixture after reload, through u again.
  phase = 'reload';
  await page.reload();
  await journey();
  assert.equal(errors, 0, 'no page errors after reload');
  console.log(`PASS ${layout}: reload re-enters the saved game; ending did not overwrite the save`);
} catch (error) {
  console.error(`FAIL ${phase}: ${error?.name ?? 'Error'}`);
  if (page) {
    await page.locator('#screen').screenshot({ path: join(out, 'failure.png') }).catch(() => {});
    console.error(await page.evaluate(() => ({
      state: document.body.dataset.state,
      program: globalThis.__shownGame?.program(),
      prompt: globalThis.__shownGame?.at_prompt?.(),
      ticks: globalThis.__ticks,
    })).catch(() => ({})));
  }
  process.exitCode = 1;
} finally {
  await browser?.close();
  await server?.close();
}

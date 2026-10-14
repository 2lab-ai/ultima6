// Browser tests of site/ under its real headers (tools/web/serve.mjs applies site/_headers):
// the operation queue (rapid imports, import during the restore, forget during a save), error
// handling (read and forget failures, an engine trap and Restart), keyboard focus, the CSP,
// saved games (stored after a Save, resumed on reload, forgotten, imported as a folder) and
// the sound output (AudioContext after a gesture, chunks into the worklet).
// Gates hold file reads and storage calls at chosen points, so every interleaving is forced,
// not timed. Tests that need the user's disks skip when they are absent.
//
//   node tools/web/test-web.mjs [name-filter]      (build first: bash scripts/build-web.sh)
import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import * as h from './harness.mjs';

const MARKER = { name: 'port-test-marker.txt', text: 'not a game file' };
const tests = [];
const test = (name, fn, { needs } = {}) => tests.push({ name, fn, needs });

async function started(page, files) {
  await h.waitSettled(page);
  const s = await h.state(page);
  assert.equal(s.state, 'running', s.status);
  if (files !== undefined) assert.equal(s.files, files, s.status);
  return s;
}

test('headers and CSP: the page runs under its policy, third-party requests are blocked', async (env) => {
  const { page } = await h.openPage(env, { gated: false });
  const doc = await h.load(page, env);
  for (const response of [doc, await page.request.get(`${env.server.url}pkg/u6_web_bg.wasm`)]) {
    const hd = response.headers();
    assert.match(hd['content-security-policy'], /connect-src 'self'/);
    assert.match(hd['content-security-policy'], /script-src 'self' 'wasm-unsafe-eval'/);
    assert.equal(hd['x-content-type-options'], 'nosniff');
    assert.equal(hd['referrer-policy'], 'no-referrer');
    assert.equal(hd['permissions-policy'], 'camera=(), microphone=(), geolocation=()');
    assert.equal(hd['cache-control'], 'public, max-age=0, must-revalidate');
  }
  const s = await h.state(page);
  assert.equal(s.state, 'idle', s.status);
  assert.equal(s.pickerDisabled, false);
  const egress = await page.evaluate(() => fetch('https://example.com/').then(() => 'fetched', (e) => `blocked: ${e}`));
  assert.match(egress, /^blocked/);
  const csp = await page.evaluate(() => window.__csp);
  assert.deepEqual(csp, ['connect-src https://example.com/'], 'only the probe was refused');
  if (h.haveHfes()) {
    await page.setInputFiles('#picker', h.HFES);
    await started(page, 130);
    assert.deepEqual(await page.evaluate(() => window.__csp), csp, 'the game boots without violations');
  }
});

test('keyboard only: Tab to the picker, choose, play on the canvas, Tab out', async (env) => {
  const { page } = await h.openPage(env, { gated: false });
  await h.load(page, env);
  await page.keyboard.press('Tab');
  assert.equal((await h.state(page)).focused, 'screen', 'the canvas is in the tab order');
  await page.keyboard.press('ArrowDown');
  assert.deepEqual(await page.evaluate(() => window.__keys.at(-1)), ['ArrowDown', false], 'no game: arrows stay the page\'s');
  await page.keyboard.press('Tab');
  assert.equal((await h.state(page)).focused, 'picker');
  const [chooser] = await Promise.all([page.waitForEvent('filechooser'), page.keyboard.press('Space')]);
  await chooser.setFiles(h.HFES);
  const s = await started(page, 130);
  assert.equal(s.focused, 'screen', 'focus lands on the canvas after the import');
  assert.equal(await page.evaluate(() => getComputedStyle(document.activeElement).outlineColor), 'rgb(255, 209, 102)');
  await h.sleep(2500); // the boot reaches the prompt
  const before = await h.frame(page);
  for (let i = 0; i < 2; i += 1) {
    await page.keyboard.press('ArrowRight');
    await h.sleep(3000); // each step's world turn plays the start fight's hits
  }
  assert.deepEqual(await page.evaluate(() => window.__keys.at(-1)), ['ArrowRight', true], 'the canvas took the arrow');
  const walked = h.mapShift(before, await h.frame(page));
  assert.ok(walked.shift >= 16 && walked.score > 0.6, `the map scrolled east: ${JSON.stringify(walked)}`);
  await page.keyboard.press('Tab');
  assert.notEqual((await h.state(page)).focused, 'screen', 'Tab leaves the canvas');
  const still = await h.frame(page);
  await page.keyboard.press('ArrowRight');
  await h.sleep(600);
  assert.deepEqual(await page.evaluate(() => window.__keys.at(-1)), ['ArrowRight', false]);
  const after = h.mapShift(still, await h.frame(page));
  assert.equal(after.shift, 0, `no walk without focus: ${JSON.stringify(after)}`);
}, { needs: h.haveHfes });

test('two rapid imports: one after the other, the second one runs and is kept', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.evaluate(() => window.__hold('read'));
  await h.dropFiles(page, h.HFES);
  await h.dropFiles(page, h.HFES, [MARKER]);
  await h.waitLog(page, 'read:start');
  assert.equal((await h.state(page)).ops, 2, 'both imports queued');
  assert.equal((await h.log(page)).filter((e) => e.startsWith('read:start')).length, 1, 'the second waits');
  await page.evaluate(() => window.__release('read'));
  await started(page, 131);
  const log = await h.log(page);
  const firstSave = log.indexOf('save:end:true');
  const secondRead = log.findIndex((e, i) => i > 0 && e.startsWith('read:start') && log.slice(0, i).includes('save:end:true'));
  assert.ok(firstSave > 0 && secondRead > firstSave, `the second import starts after the first is kept: ${log}`);
  const kept = await h.keptPaths(page);
  assert.equal(kept.length, 131);
  assert.ok(kept.includes(MARKER.name), 'storage holds the second import');
  assert.match((await h.state(page)).status, /The 131 game files are kept/);
}, { needs: h.haveHfes });

test('an import during the restore waits for it, then replaces the restored game', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await h.dropFiles(page, h.HFES);
  await started(page, 130);
  await page.evaluate(() => sessionStorage.setItem('__holds', '["load"]'));
  await page.reload();
  await h.waitLog(page, 'load:start');
  assert.equal((await h.state(page)).pickerDisabled, true, 'no picker during the restore');
  await h.dropFiles(page, h.HFES, [MARKER]);
  const during = await h.state(page);
  assert.equal(during.ops, 2, 'the drop is queued behind the restore');
  assert.ok(!(await h.log(page)).some((e) => e.startsWith('read:')), 'nothing read before the restore');
  await page.evaluate(() => window.__release('load'));
  await started(page, 131);
  const log = await h.log(page);
  assert.ok(log.indexOf('load:end:130') < log.findIndex((e) => e.startsWith('read:start')), String(log));
  assert.equal((await h.keptPaths(page)).length, 131);
  assert.equal((await h.state(page)).pickerDisabled, false);
}, { needs: h.haveHfes });

test('forget during a pending save runs after the save and reports only once committed', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.evaluate(() => { window.__hold('save'); window.__hold('forget'); });
  await h.dropFiles(page, h.HFES);
  await h.waitLog(page, 'save:start');
  await page.click('#forget');
  let s = await h.state(page);
  assert.equal(s.ops, 2);
  assert.match(s.status, /will be removed after the current step/);
  await page.evaluate(() => window.__release('save'));
  await h.waitLog(page, 'forget:start');
  const log = await h.log(page);
  assert.ok(log.indexOf('save:end:true') < log.indexOf('forget:start'), String(log));
  assert.equal((await h.keptPaths(page)).length, 130, 'the save committed first');
  assert.doesNotMatch((await h.state(page)).status, /were removed/, 'no success before the delete');
  await page.evaluate(() => window.__release('forget'));
  await h.waitSettled(page);
  s = await h.state(page);
  assert.equal(await h.keptPaths(page), null, 'storage empty');
  assert.match(s.status, /The kept game files were removed/);
  assert.equal(s.state, 'running');
  assert.equal(s.files, 130, 'the game on screen keeps running');
}, { needs: h.haveHfes });

test('read and forget failures are shown and the page stays usable', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.evaluate(() => window.__fail.add('read'));
  await h.dropFiles(page, [], [MARKER]);
  await h.waitSettled(page);
  let s = await h.state(page);
  assert.match(s.status, /Loading the files failed: test: unreadable/);
  assert.equal(s.state, 'idle');
  await page.evaluate(() => { window.__fail.delete('read'); window.__fail.add('forget'); });
  await page.click('#forget');
  await h.waitSettled(page);
  s = await h.state(page);
  assert.match(s.status, /Removing the kept files failed: test: storage refused the delete/);
  await page.evaluate(() => window.__fail.delete('forget'));
  await h.dropFiles(page, [], [MARKER]);
  await h.waitSettled(page);
  s = await h.state(page);
  assert.match(s.status, /Could not start the game:[\s\S]*not found in the dropped files/);
  assert.equal(await h.keptPaths(page), null, 'a failed start keeps nothing');
});

test('inputs over 512 MiB together are refused before anything is read', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.evaluate(() => { window.__bigName = 'big.zip'; });
  await h.dropFiles(page, [], [{ name: 'big.zip', text: 'PK' }, MARKER]);
  await h.waitSettled(page);
  const s = await h.state(page);
  assert.match(s.status, /600 MiB together; this page takes at most 512 MiB/);
  assert.ok(!(await h.log(page)).some((e) => e.startsWith('read:')), 'nothing read');
  assert.equal(s.state, 'idle');
});

test('an engine that does not load is reported; the controls stay off', async (env) => {
  const { context, page } = await h.openPage(env);
  await context.route(/\/pkg\/u6_web_bg\.wasm$/, (route) => route.fulfill({ status: 404, body: 'gone' }));
  await page.goto(env.server.url);
  await h.waitSettled(page);
  const s = await h.state(page);
  assert.equal(s.state, 'failed');
  assert.match(s.status, /^Loading the engine failed: the engine could not load/);
  assert.equal(s.pickerDisabled, true);
  assert.equal(await page.evaluate(() => document.getElementById('forget').disabled), true);
});

test('an engine trap stops the loop; Restart loads the kept files again', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await h.dropFiles(page, h.HFES);
  await started(page, 130);
  await page.evaluate(() => { globalThis.__trap = true; });
  await page.waitForFunction(() => document.body.dataset.state === 'crashed');
  let s = await h.state(page);
  assert.equal(s.restartHidden, false);
  assert.match(s.status, /internal error[\s\S]*Restart/);
  const ticks = await page.evaluate(() => globalThis.__ticks);
  await h.sleep(500);
  assert.equal(await page.evaluate(() => globalThis.__ticks), ticks, 'not ticked again');
  await page.click('#restart');
  await page.waitForLoadState('load');
  await started(page, 130);
  s = await h.state(page);
  assert.equal(s.focused, 'screen', 'the user asked: focus on the canvas');
  assert.equal(s.restartHidden, true);
}, { needs: h.haveHfes });

test('A on the canvas: Attack asks for a target, a stray key ends it, then the arrow walks (screenshot)', async (env) => {
  const { page } = await h.openPage(env, { gated: false });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  await h.waitPrompt(page);
  await page.keyboard.press('a');
  await h.sleep(800);
  await page.keyboard.press('q'); // confirms no target: "what?", the turn (the start fight) runs
  await h.waitPrompt(page);
  await writeFile('/tmp/claude-501/m2-attack-what.png', await page.locator('#screen').screenshot());
  const { key, before } = await h.stepSideways(page);
  const walked = h.mapShift(before, await h.frame(page));
  const want = key === 'ArrowRight' ? 16 : -16;
  assert.ok(walked.shift === want && walked.score > 0.5, `one step (${key}) after A: ${JSON.stringify(walked)}`);
}, { needs: h.haveHfes });

test('Ctrl+S and Ctrl+R on the canvas are the game\'s Save and Restore, not the browser\'s', async (env) => {
  const { page } = await h.openPage(env, { gated: false });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  await h.sleep(2500);
  for (const key of ['Control+s', 'Control+r']) {
    await page.keyboard.press(key);
    await h.sleep(300);
    const code = key === 'Control+s' ? 'KeyS' : 'KeyR';
    assert.deepEqual(await page.evaluate(() => window.__keys.at(-1)), [code, true], `${key} taken by the canvas`);
    await page.keyboard.press('n'); // the question's answer: no
    await h.sleep(600);
  }
  await page.keyboard.press('Control+l');
  assert.deepEqual(await page.evaluate(() => window.__keys.at(-1)), ['KeyL', false], 'other Ctrl shortcuts stay the browser\'s');
}, { needs: h.haveHfes });

// Walks one step (east or west), then saves (Ctrl-S, Y) and waits until the save is stored;
// returns the leader's position before and after the step.
async function walkAndSave(page) {
  await h.waitPrompt(page); // the boot reaches the prompt
  const start = (await h.state(page)).leader;
  const { to: moved } = await h.stepSideways(page);
  await page.keyboard.press('Control+s');
  await h.sleep(400);
  await page.keyboard.press('y');
  await page.waitForFunction(() => document.body.dataset.saved === '1', null, { timeout: 60_000 });
  assert.equal((await h.state(page)).leader, moved, 'the save is where the party stands');
  await h.waitSettled(page);
  assert.match((await h.state(page)).status, /Saved in this browser \(\d+ files\)/);
  return { start, moved };
}

test('a save (Ctrl-S, Y) is kept; a reload resumes it there; forgetting it starts over', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  const { start, moved } = await walkAndSave(page);
  const saved = await h.savedPaths(page);
  assert.ok(saved.includes('savegame/objlist') && saved.length > 1, String(saved));
  assert.ok((await h.log(page)).includes('saves:store:end'), 'stored through the queue');
  await page.reload();
  let s = await started(page, 130);
  assert.equal(s.saves, saved.length, 'the stored save is laid over the game files');
  assert.equal(s.leader, moved, 'resumed where it was saved');
  assert.match(s.status, /the game saved in this browser, resumed/);
  await page.click('#forget-save');
  await h.waitSettled(page);
  assert.deepEqual(await h.savedPaths(page), [], 'the save is gone');
  assert.equal((await h.keptPaths(page)).length, 130, 'the game files stay');
  assert.match((await h.state(page)).status, /The saved game was removed/);
  await page.reload();
  s = await started(page, 130);
  assert.equal(s.saves, 0);
  assert.equal(s.leader, start, 'a new game');
  assert.match(s.status, /a new game \(no game saved in this browser\)/);
}, { needs: h.haveHfes });

test('an original savegame folder dropped becomes the saved game', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  const { moved } = await walkAndSave(page);
  // The stored files, as a PLAYER disk's savegame/ folder would hold them.
  const folder = await page.evaluate(() => new Promise((done, fail) => {
    const req = indexedDB.open('ultima6-port', 1);
    req.onerror = () => fail(req.error);
    req.onsuccess = () => {
      const get = req.result.transaction('files').objectStore('files')
        .getAll(IDBKeyRange.bound('save:', 'save:\uffff'));
      get.onsuccess = () => {
        req.result.close();
        done(get.result.map((f) => ({ name: `PLAYER/${f.path.toUpperCase()}`, bytes: [...f.bytes] })));
      };
    };
  }));
  await page.click('#forget-save');
  await h.waitSettled(page);
  assert.deepEqual(await h.savedPaths(page), []);
  await h.dropFiles(page, [], folder);
  const s = await started(page, 130);
  assert.match(s.status, /The saved game \(\d+ files\) was imported/);
  assert.equal(s.saves, folder.length);
  assert.equal(s.leader, moved, 'the imported save resumes');
  assert.equal((await h.savedPaths(page)).length, folder.length, 'one record per file, any case');
}, { needs: h.haveHfes });

test('zip: a click starts the sound, the worklet gets chunks, Mute; save and reload (receipt)', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await h.dropFiles(page, [h.ZIP]);
  await started(page);
  assert.equal((await h.state(page)).audio, 'off', 'no sound before a gesture');
  await page.click('#screen');
  await page.waitForFunction(() => Number(document.body.dataset.audioChunks ?? 0) > 5, null, { timeout: 15_000 });
  let s = await h.state(page);
  assert.equal(s.audio, 'running');
  assert.deepEqual(await page.evaluate(() => window.__csp), [], 'the worklet loads under the CSP');
  console.log(`  receipt: audioContext.state=${s.audio}, worklet chunks=${s.audioChunks}`);
  await page.click('#mute');
  assert.equal(await page.getAttribute('#mute', 'aria-pressed'), 'true');
  await page.click('#mute');
  assert.equal(await page.getAttribute('#mute', 'aria-pressed'), 'false');
  await page.click('#screen');
  const { moved } = await walkAndSave(page);
  await writeFile('/tmp/claude-501/int-b-web-saved.png', await page.locator('#screen').screenshot());
  await page.reload();
  s = await started(page);
  await h.sleep(1500);
  await writeFile('/tmp/claude-501/int-b-web-restored.png', await page.locator('#screen').screenshot());
  console.log(`  receipt: saved at ${moved}, after the reload ${s.leader} (${s.saves} save files)`);
  assert.equal(s.leader, moved, 'restored position');
}, { needs: h.haveZip });

const filter = process.argv[2];
const env = await h.setup();
let failed = 0;
let ran = 0;
try {
  for (const t of tests) {
    if (filter && !t.name.includes(filter)) continue;
    if (t.needs && !t.needs()) {
      console.log(`skip - ${t.name} (the user's disks are not present)`);
      continue;
    }
    ran += 1;
    try {
      await t.fn(env);
      console.log(`ok - ${t.name}`);
    } catch (err) {
      failed += 1;
      console.log(`not ok - ${t.name}\n  ${String(err.stack ?? err).split('\n').slice(0, 6).join('\n  ')}`);
    } finally {
      for (const context of env.browser.contexts()) await context.close();
    }
  }
} finally {
  await env.close();
}
console.log(`# web tests: ${ran} ran, ${ran - failed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);

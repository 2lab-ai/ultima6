// Browser tests of site/ under its real headers (tools/web/serve.mjs applies site/_headers):
// the operation queue (rapid imports, import during the restore, forget during a save), error
// handling (read and forget failures, an engine trap and Restart), keyboard focus, the CSP,
// saved games (stored after a Save, resumed on reload, forgotten, imported as a folder, a
// replaced game's Save never stored over its replacement's, a Save after a forget stored
// whole, a folder file over the save limit left out), the sound output (AudioContext after a
// gesture, chunks into the worklet; a game whose MED tables fail the checks plays silent) and the
// launcher's sequence as a player meets it (the opening, the menu, a character created and
// kept, the menu's J after a reload, Quit) and the mouse on the canvas (the first press is
// both the sound's gesture and exactly one game click). Most tests start `game` at once
// (harness `gameFirst`); the launcher tests run the page as it is.
// Gates hold file reads and storage calls at chosen points, so every interleaving is forced,
// not timed. Tests that need the user's disks skip when they are absent.
//
//   node tools/web/test-web.mjs [name-filter]      (build first: bash scripts/build-web.sh)
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
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

// No page: the harness's content-free comparison (synthetic values) names paths and lengths only.
test('assertSame: a mismatch names the label, the paths, the lengths and the index, never the values', async () => {
  const failure = (f) => {
    try { f(); } catch (err) { return err.message; }
    return null;
  };
  h.assertSame({ 'savegame/objlist': [1, 2] }, { 'savegame/objlist': [1, 2] }, 'equal');
  const store = failure(() => h.assertSame(
    { 'savegame/objblkaa': [7, 83, 9], 'savegame/objlist': [5] },
    { 'savegame/objblkaa': [7, 88, 9] },
    'the store',
  ));
  assert.equal(store, 'the store: savegame/objblkaa: lengths 3 and 3, first difference at index 1; savegame/objlist: not expected');
  const name = failure(() => h.assertSame('Zhuge', 'Zhugx', 'the name'));
  assert.equal(name, 'the name: lengths 5 and 5, first difference at index 4');
  const list = failure(() => h.assertSame([{ path: 'a', bytes: [1] }], [{ path: 'a', bytes: [2, 3] }], 'files'));
  assert.equal(list, 'files: [0].bytes: lengths 1 and 2, first difference at index 0');
  for (const leak of ['83', '88', 'Zhug']) {
    assert.ok(![store, name, list].some((m) => m.includes(leak)), `a message shows ${leak}`);
  }
});

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
  // From the prompt, one step east (west when the start fight blocks it), then the prompt again.
  const { key, before } = await h.stepSideways(page);
  assert.deepEqual(await page.evaluate(() => window.__keys.at(-1)), [key, true], 'the canvas took the arrow');
  const walked = h.mapShift(before, await h.frame(page));
  const want = key === 'ArrowRight' ? 16 : -16;
  assert.ok(walked.shift === want && walked.score > 0.5, `one step (${key}) scrolled the map: ${JSON.stringify(walked)}`);
  await page.keyboard.press('Tab');
  assert.notEqual((await h.state(page)).focused, 'screen', 'Tab leaves the canvas');
  const at = (await h.state(page)).leader;
  const still = await h.frame(page);
  await page.keyboard.press('ArrowRight');
  assert.deepEqual(await page.evaluate(() => window.__keys.at(-1)), ['ArrowRight', false], 'the page kept the arrow');
  await h.waitPrompt(page); // the game still waits for a command, nothing queued
  assert.equal(await page.evaluate(() => document.getElementById('screen').dataset.keys), '0');
  assert.equal((await h.state(page)).leader, at, 'no walk without focus');
  const after = h.mapShift(still, await h.frame(page));
  assert.equal(after.shift, 0, `the map stayed: ${JSON.stringify(after)}`);
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
  await page.keyboard.press('q'); // confirms no target: the no-target word, the turn (the start fight) runs
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

// Stored save files as a PLAYER disk's savegame/ folder would hold them (upper case).
const asFolder = (files) => files.map(({ path, bytes }) => ({ name: `PLAYER/${path.toUpperCase()}`, bytes }));
// The files of a folder or a store, by lower-cased name.
const byName = (files) => Object.fromEntries(files.map(({ name, path, bytes }) =>
  [(path ?? name.slice('PLAYER/'.length)).toLowerCase(), bytes]));

test('an original savegame folder dropped becomes the saved game', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  const { moved } = await walkAndSave(page);
  const folder = asFolder(await h.savedFiles(page));
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

test('a Save of the game an import replaces, made while the import reads: the imported save wins', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  const { moved } = await walkAndSave(page);
  const folder = asFolder(await h.savedFiles(page));
  const { to: later } = await h.stepSideways(page);
  assert.notEqual(later, moved);
  await page.evaluate(() => window.__hold('read'));
  const from = (await h.log(page)).length;
  await h.dropFiles(page, [], folder);
  await h.waitLog(page, 'read:start');
  // The game on screen saves (its second Save) while the import is held in its file reads.
  await page.focus('#screen');
  await h.press(page, 'Control+s', 30);
  await h.press(page, 'y', 60);
  await page.waitForFunction(() => globalThis.__saveGen === 2, null, { timeout: 30_000 });
  await page.evaluate(() => window.__release('read'));
  let s = await started(page, 130);
  const stores = (await h.log(page)).slice(from).filter((e) => e.startsWith('saves:store:start'));
  assert.deepEqual(stores, [`saves:store:start:${folder.length}`], 'only the import stored');
  h.assertSame(byName(await h.savedFiles(page)), byName(folder), 'the store holds the imported files');
  assert.match(s.status, /The saved game \(\d+ files\) was imported/);
  assert.equal(s.leader, moved, 'the imported save resumes');
  assert.equal(s.saved, 1, 'the replaced game\'s Save was not stored');
  await page.reload();
  s = await started(page, 130);
  assert.equal(s.leader, moved, 'a reload resumes the imported save');
}, { needs: h.haveHfes });

test('an import that does not start leaves the stored save; the game on screen saves on', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  await walkAndSave(page);
  const stored = await h.savedFiles(page);
  // A folder with only the objlist, and no game files: the import cannot start.
  const folder = asFolder(stored.filter(({ path }) => path === 'savegame/objlist'));
  assert.ok(folder.length === 1 && stored.length > 1, String(stored.length));
  await h.dropFiles(page, [], [...folder, MARKER]);
  await h.waitSettled(page);
  let s = await h.state(page);
  assert.match(s.status, /Could not start the game/);
  h.assertSame(byName(await h.savedFiles(page)), byName(stored), 'the stored save is as it was');
  await page.focus('#screen');
  await h.press(page, 'Control+s', 30);
  await h.press(page, 'y', 60);
  await page.waitForFunction(() => document.body.dataset.saved === '2', null, { timeout: 60_000 });
  await h.waitSettled(page);
  s = await h.state(page);
  assert.match(s.status, /Saved in this browser/, 'the game on screen still stores its Saves');
}, { needs: h.haveHfes });

test('a Save after the saved game was forgotten stores the whole game; a reload resumes that world', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  await walkAndSave(page);
  // The game on screen starts from a save holding a block its Saves near the start never
  // rewrite (synthetic: the far block 63 empty).
  const folder = [...asFolder(await h.savedFiles(page)), { name: 'PLAYER/SAVEGAME/OBJBLKHH', bytes: [0, 0] }];
  await h.dropFiles(page, [], folder);
  let s = await started(page, 130);
  assert.equal(s.saves, folder.length);
  // A Save taken while the forget runs is not stored after the delete.
  await page.evaluate(() => window.__hold('saves-forget'));
  await page.click('#forget-save');
  await h.waitLog(page, 'saves:forget:start');
  await page.focus('#screen');
  await h.press(page, 'Control+s', 30);
  await h.press(page, 'y', 60);
  await page.waitForFunction(() => globalThis.__saveGen === 1, null, { timeout: 30_000 });
  await page.waitForFunction(() => document.body.dataset.ops === '2', null, { timeout: 10_000 });
  await page.evaluate(() => window.__release('saves-forget'));
  await h.waitSettled(page);
  assert.deepEqual(await h.savedPaths(page), [], 'the Save taken before the forget was not stored');
  assert.match((await h.state(page)).status, /The saved game was removed/);
  // The next Save stores the whole game: the objlist and all 69 blocks, the far block included.
  const { to: later } = await h.stepSideways(page);
  await h.press(page, 'Control+s', 30);
  await h.press(page, 'y', 60);
  await page.waitForFunction(() => document.body.dataset.saved === '2', null, { timeout: 60_000 });
  await h.waitSettled(page);
  const stored = byName(await h.savedFiles(page));
  assert.equal(Object.keys(stored).length, 70, 'the objlist and every block');
  h.assertSame(stored['savegame/objblkhh'], [0, 0], 'the far block as the game had it');
  await page.reload();
  s = await started(page, 130);
  assert.equal(s.saves, 70, 'the whole save is laid over the game files');
  assert.equal(s.leader, later, 'resumed where it was saved');
}, { needs: h.haveHfes });

test('a Save of a game that did not own the stored save, then the forget: it is not stored; the next Save is, whole', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  // No kept files: a drop of only a savegame folder starts nothing, it becomes the stored save
  // and the game on screen no longer owns it (synthetic: the far block 63 empty).
  await page.click('#forget');
  await h.waitSettled(page);
  assert.equal(await h.keptPaths(page), null);
  await h.dropFiles(page, [], [{ name: 'PLAYER/SAVEGAME/OBJBLKHH', bytes: [0, 0] }]);
  await h.waitSettled(page);
  let s = await h.state(page);
  assert.equal(s.state, 'running', s.status);
  assert.match(s.status, /The game on screen is not saved in this browser any more/);
  const lower = async () => (await h.savedPaths(page)).map((p) => p.toLowerCase());
  assert.deepEqual(await lower(), ['savegame/objblkhh']);
  // The game on screen saves: not stored, it does not own the store.
  await page.focus('#screen');
  await h.waitPrompt(page);
  await h.press(page, 'Control+s', 30);
  await h.press(page, 'y', 60);
  await page.waitForFunction(() => globalThis.__saveGen === 1, null, { timeout: 30_000 });
  await h.waitSettled(page);
  assert.deepEqual(await lower(), ['savegame/objblkhh'], 'the Save was not stored');
  assert.equal((await h.state(page)).saved, 0);
  // The forget: that earlier Save stays out of the store on the ticks after it.
  const from = (await h.log(page)).length;
  await page.click('#forget-save');
  await h.waitSettled(page);
  await h.waitTicks(page, 10);
  await h.waitSettled(page);
  s = await h.state(page);
  const stores = (await h.log(page)).slice(from).filter((e) => e.startsWith('saves:store:start'));
  assert.deepEqual(stores, [], 'nothing stored after the forget');
  assert.deepEqual(await h.savedPaths(page), [], 'the store stays empty');
  assert.equal(s.saved, 0);
  assert.match(s.status, /The saved game was removed from this browser; the next start is a new game/);
  // A Save after the forget is stored, whole: the objlist and every block.
  await page.focus('#screen');
  await h.waitPrompt(page);
  await h.press(page, 'Control+s', 30);
  await h.press(page, 'y', 60);
  await page.waitForFunction(() => document.body.dataset.saved === '2', null, { timeout: 60_000 });
  await h.waitSettled(page);
  assert.equal((await h.savedPaths(page)).length, 70, 'the objlist and every block');
}, { needs: h.haveHfes });

test('a savegame folder file over the save limit is left out before it is read', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  // Synthetic bytes: more than a block file can hold (the count word and 0xc00 records).
  await h.dropFiles(page, [], [{ name: 'PLAYER/SAVEGAME/objblkAA', bytes: new Array(30_000).fill(0) }]);
  await h.waitSettled(page);
  const s = await h.state(page);
  assert.match(s.status, /Left out of the saved game \(a save file holds at most \d+ bytes/);
  assert.match(s.status, /Nothing was imported/);
  assert.ok(!(await h.log(page)).some((e) => e.startsWith('read:')), 'nothing read');
  assert.deepEqual(await h.savedPaths(page), [], 'nothing stored');
});

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

// In the page: the files the last start kept (IndexedDB), with a copy of `game` whose MED period
// table (image 0x101248, hunk 0 + 0x1248; u6-core sound/tables.rs) is zeroed, dropped as the
// extracted files. The patch is a synthetic edit of the user's own bytes, in page memory only;
// it checks that the place holds a period table first and prints nothing of it. Returns the
// number of files dropped.
function dropWithoutMedTables(page) {
  return page.evaluate(async () => {
    const kept = await new Promise((done, fail) => {
      const req = indexedDB.open('ultima6-port', 1);
      req.onerror = () => fail(req.error);
      req.onsuccess = () => {
        const db = req.result;
        const get = db.transaction('files').objectStore('files').get('extracted');
        get.onsuccess = () => { db.close(); done(get.result); };
        get.onerror = () => fail(get.error);
      };
    });
    const game = kept.find((f) => f.path.split('/').pop().toLowerCase() === 'game');
    if (!game) throw new Error('no game among the kept files');
    const bytes = game.bytes.slice();
    const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    // The hunk header as u6-core image.rs reads it, up to the first block of hunk 0.
    let pos = 0;
    const long = () => { const x = v.getUint32(pos); pos += 4; return x; };
    if (long() !== 0x3f3) throw new Error('game: no HUNK_HEADER');
    for (let n = long(); n !== 0; n = long()) pos += 4 * n;
    long();
    const first = long();
    const last = long();
    for (let i = first; i <= last; i += 1) if (((long() & 0xc0000000) >>> 0) === 0xc0000000) long();
    const type = long() & 0x3fffffff;
    if (type !== 0x3e9 && type !== 0x3ea) throw new Error('game: hunk 0 holds no bytes');
    long();
    const at = pos + (0x101248 - 0x100000);
    const word = (i) => v.getUint16(at + 2 * i);
    for (let i = 1; i < 60; i += 1) {
      if (word(i) >= word(i - 1)) throw new Error('game: no period table at the expected place');
    }
    bytes.fill(0, at, at + 2 * 96);
    const dt = new DataTransfer();
    for (const f of kept) dt.items.add(new File([f === game ? bytes : f.bytes], f.path));
    document.getElementById('drop').dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }));
    return kept.length;
  });
}

test('a game whose MED tables fail the checks starts and plays silent: sound on at its boot, and after a reload from the first press', async (env) => {
  const { page } = await h.openPage(env);
  const silent = [];
  page.on('console', (m) => { if (m.type() === 'warning' && m.text().startsWith('sound is off')) silent.push(m.text()); });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  await page.click('#screen');
  await page.waitForFunction(() => document.body.dataset.audio === 'running', null, { timeout: 15_000 });
  assert.equal(silent.length, 0, 'the user\'s game has its audio machine');
  // The sound is on: the boot of the new game finds no audio machine for it.
  const dropped = await dropWithoutMedTables(page);
  let s = await started(page);
  assert.match(s.status, /Started\./, 'the dropped files started');
  assert.equal(silent.length, 1, `the boot said the sound is off: ${silent}`);
  let ticks = await page.evaluate(() => globalThis.__ticks);
  let step = await h.stepSideways(page);
  assert.notEqual(step.to, step.from, 'the game plays');
  assert.ok(await page.evaluate(() => globalThis.__ticks) > ticks, 'the engine ticks');
  // The patched files are the kept ones now: a reload restores them before any gesture, and
  // the first press starts the AudioContext, which finds no audio machine for the game.
  await page.reload();
  s = await started(page);
  assert.equal(s.audio, 'off', 'no sound before a gesture');
  assert.equal(silent.length, 1);
  await page.click('#screen');
  for (let i = 0; i < 150 && silent.length < 2; i += 1) await h.sleep(100);
  assert.equal(silent.length, 2, `the gesture said the sound is off: ${silent}`);
  s = await h.state(page);
  assert.equal(s.state, 'running', s.status);
  ticks = await page.evaluate(() => globalThis.__ticks);
  step = await h.stepSideways(page);
  assert.notEqual(step.to, step.from, 'the game plays on');
  assert.ok(await page.evaluate(() => globalThis.__ticks) > ticks, 'the engine ticks');
  console.log(`  receipt: ${dropped} files dropped with the period table zeroed; sound off at the boot and at the gesture, the game walks after each`);
}, { needs: h.haveHfes });

// The creation from the menu (u6-core intro tests `create_from_menu`): C, the name, male, the
// second portrait, the gypsy's seven questions answered A, B alternately, Escape ending the
// introduction story; settles in ticks, after the game took each key.
async function createCharacter(page, name, shot) {
  await h.press(page, 'c', 120);
  for (const c of name) await h.press(page, c, 30);
  await h.waitTicks(page, 10);
  await h.press(page, 'Enter', 60);
  await h.press(page, 'm', 60);
  await h.press(page, 'p', 30);
  await h.press(page, 'c', 60);
  for (let i = 0; i < 3; i += 1) await h.press(page, 'Space', 200);
  await h.press(page, 'Space', 120);
  await h.press(page, 'Space', 120);
  await shot('question');
  for (let q = 0; q < 7; q += 1) await h.press(page, q % 2 === 0 ? 'a' : 'b', 220);
  await h.press(page, 'Space', 200);
  for (let i = 0; i < 4; i += 1) await h.press(page, 'Space', 120);
  await h.waitTicks(page, 60);
  await h.press(page, 'Escape', 200);
}

const RECEIPTS = '/tmp/claude-501/int-intro';

test('zip as a player: the opening, Escape, the menu, a character created and played; a reload journeys onward with it (receipt)', async (env) => {
  await mkdir(RECEIPTS, { recursive: true });
  const { page } = await h.openPage(env, { gameFirst: false });
  const shot = async (name) => writeFile(`${RECEIPTS}/web-${name}.png`, await page.locator('#screen').screenshot());
  await h.load(page, env);
  await h.dropFiles(page, [h.ZIP]);
  await started(page);
  await h.waitProgram(page, 'u');
  await h.waitVisible(page);
  await shot('01-opening');
  await page.focus('#screen');
  await h.press(page, 'Escape', 120);
  assert.equal((await h.state(page)).program, 'u');
  await shot('02-menu');
  await createCharacter(page, 'Zhuge', shot);
  await h.waitProgram(page, 'game');
  await h.waitPrompt(page);
  let s = await h.state(page);
  h.assertSame(s.name, 'Zhuge', 'the game plays the created character');
  await shot('03-game');
  await page.waitForFunction(() => document.body.dataset.saved === '1', null, { timeout: 60_000 });
  await h.waitSettled(page);
  assert.deepEqual(await h.savedPaths(page), ['savegame/objlist'], 'u wrote the objlist, kept like a save');
  await page.reload();
  s = await started(page);
  assert.equal(s.saves, 1, 'the stored objlist is laid over the game files');
  await h.waitProgram(page, 'u');
  await h.waitVisible(page);
  await page.focus('#screen');
  await h.press(page, 'Escape', 120);
  await shot('04-menu-after-reload');
  await h.press(page, 'j', 30);
  await h.waitProgram(page, 'game');
  await h.waitPrompt(page);
  s = await h.state(page);
  h.assertSame(s.name, 'Zhuge', 'J in the menu plays the stored character');
  await shot('05-journey-onward');
  console.log(`  receipt: created ${s.name}; after the reload J in the menu plays ${s.name} (${s.saves} save file)`);
}, { needs: h.haveZip });

test('zip: Quit in the menu ends the game; the page says so and Restart starts it again', async (env) => {
  const { page } = await h.openPage(env, { gameFirst: false });
  await h.load(page, env);
  await h.dropFiles(page, [h.ZIP]);
  await started(page);
  await h.waitProgram(page, 'u');
  await h.waitVisible(page);
  await page.focus('#screen');
  await h.press(page, 'Escape', 120);
  await page.keyboard.press('Control+q');
  await page.waitForFunction(() => document.body.dataset.state === 'ended', null, { timeout: 30_000 });
  let s = await h.state(page);
  assert.equal(s.program, 'ended');
  assert.match(s.status, /The game has ended\. Restart starts it again/);
  assert.equal(s.restartHidden, false);
  const ticks = await page.evaluate(() => globalThis.__ticks);
  await h.sleep(500);
  assert.equal(await page.evaluate(() => globalThis.__ticks), ticks, 'not ticked after the end');
  await page.click('#restart');
  await page.waitForLoadState('load');
  s = await started(page);
  assert.equal(s.restartHidden, true);
  await h.waitProgram(page, 'u');
}, { needs: h.haveZip });

// A pixel of the canvas (320 x 200 game pixels) as a page position through its CSS box.
async function screenPoint(page, x, y) {
  const box = await page.locator('#screen').boundingBox();
  return [box.x + ((x + 0.5) * box.width) / 320, box.y + ((y + 0.5) * box.height) / 200];
}

// The message scroll's pixels (x 168..311, y 104..191) differ between two frames.
function scrollChanged(before, after) {
  for (let y = 104; y < 192; y += 1) {
    for (let x = 168; x < 312; x += 1) {
      const i = (y * 320 + x) * 4;
      if (before[i] !== after[i] || before[i + 1] !== after[i + 1] || before[i + 2] !== after[i + 2]) return true;
    }
  }
  return false;
}

const OUT = '/tmp/claude-501/int-mouse';
const promptNow = (page) => page.evaluate(() => document.getElementById('screen').dataset.prompt);

// Clicks the map cell `dx` cells east of the Avatar (its window cell (5, 5) is centred on
// game pixel (88, 88)), then waits for the prompt again; the leader before and after.
async function clickCell(page, dx) {
  await h.waitPrompt(page);
  const from = (await h.state(page)).leader;
  await page.mouse.click(...(await screenPoint(page, 88 + 16 * dx, 88)));
  await page.waitForFunction(() => document.getElementById('screen').dataset.prompt === '0',
    null, { timeout: 10_000 }).catch(() => {});
  await h.waitPrompt(page);
  return { from, to: (await h.state(page)).leader };
}

// Look by clicks: the Look icon, then the Avatar's own cell. Look on a person shows the portrait
// panel and waits for a key or a click; a click on the Avatar's cell answers it (0x8f, nothing
// else happens). Returns the frame with the description in the scroll.
async function lookAtAvatar(page, shot) {
  await page.mouse.click(...(await screenPoint(page, 64, 184))); // the fourth icon: Look
  await page.waitForFunction(() => document.getElementById('screen').dataset.prompt === '0', null, { timeout: 10_000 });
  if (shot) await writeFile(`${OUT}/web-look-target.png`, await page.locator('#screen').screenshot());
  await page.mouse.click(...(await screenPoint(page, 88, 88))); // Look's target: the Avatar
  await h.sleep(1500);
  assert.equal(await promptNow(page), '0', 'the portrait waits for a key or a click');
  const described = await h.frame(page);
  if (shot) await writeFile(`${OUT}/web-look.png`, await page.locator('#screen').screenshot());
  const at = (await h.state(page)).leader;
  await page.mouse.click(...(await screenPoint(page, 88, 88)));
  await h.waitPrompt(page);
  assert.equal((await h.state(page)).leader, at, 'the click that closes the portrait does not walk');
  return described;
}

// One step by clicks: the cell east of the Avatar, else (blocked by the start fight) west,
// up to the harness's `STEP_TRIES` tries (as `stepSideways`); asserts the step is one cell
// along x.
async function clickStep(page) {
  for (let i = 0; i < h.STEP_TRIES; i += 1) {
    const dx = i % 2 === 0 ? 1 : -1;
    const { from, to } = await clickCell(page, dx);
    if (to === from) continue;
    const [fx, fy, fz] = from.split(',').map(Number);
    const [tx, ty, tz] = to.split(',').map(Number);
    assert.deepEqual([tx - fx, ty - fy, tz - fz], [dx, 0, 0], `one cell ${dx > 0 ? 'east' : 'west'}: ${from} -> ${to}`);
    return { dx, from, to };
  }
  throw new Error(`${h.STEP_TRIES} click steps blocked in a row`);
}

test('the mouse: the Look icon, then Look on the Avatar, then clicks walk; the keys still work', async (env) => {
  const { page } = await h.openPage(env, { gated: false });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  await h.waitPrompt(page);
  const before = await h.frame(page);
  const described = await lookAtAvatar(page, false);
  assert.ok(scrollChanged(before, described), 'Look printed in the scroll');
  assert.equal((await h.state(page)).focused, 'screen', 'the click focused the canvas');
  await clickStep(page);
  const { from, to } = await h.stepSideways(page);
  assert.notEqual(to, from, 'the arrows still walk after the mouse');
}, { needs: h.haveHfes });

// Holds the left button 300 ms at game pixel (x, y); the engine ticks run during the hold (a
// page whose tick never returns answers no evaluate: that is a frozen tab, not a slow one).
async function holdOnMap(page, x, y) {
  const ticks = () => Promise.race([
    page.evaluate(() => globalThis.__ticks),
    h.sleep(5000).then(() => { throw new Error('the tab froze during the hold'); }),
  ]);
  await page.mouse.move(...(await screenPoint(page, x, y)));
  const t0 = await ticks();
  await page.mouse.down();
  await h.sleep(300);
  const t1 = await ticks();
  await page.mouse.up();
  return t1 - t0;
}

test('the mouse: a press held 300 ms on the map walks and yields; held through the exit question it walks nowhere and N gets the prompt back', async (env) => {
  const { page } = await h.openPage(env); // gated: the glue counts the ticks
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  await h.waitPrompt(page);
  // Three cells south of the Avatar, at the prompt.
  const ran = await holdOnMap(page, 88, 88 + 48);
  assert.ok(ran >= 5, `the engine ticked through the hold: ${ran} ticks`);
  await h.waitPrompt(page);
  // Ctrl-Q: the exit question waits for Y or N; a held map press is no answer.
  await page.keyboard.press('Control+q');
  await page.waitForFunction(() => document.getElementById('screen').dataset.prompt === '0', null, { timeout: 10_000 });
  await h.sleep(500);
  const asked = (await h.state(page)).leader;
  const during = await holdOnMap(page, 88 + 48, 88);
  assert.ok(during >= 5, `the engine ticked through the hold in the question: ${during} ticks`);
  await h.sleep(500);
  assert.equal(await promptNow(page), '0', 'still asking after the release');
  assert.equal((await h.state(page)).leader, asked, 'the question took no walk');
  await page.keyboard.press('n');
  await h.waitPrompt(page);
  assert.equal((await h.state(page)).leader, asked, 'N: the prompt again, in place');
  console.log(`  receipt: held 300 ms at the prompt = ${ran} ticks; in the exit question = ${during} ticks, no step, N back to the prompt`);
}, { needs: h.haveHfes });

// Holds the left button `ms` (a real press: mouse down, wait, up) three cells south of the
// Avatar at the prompt; the cells the leader moved until the prompt is back (the page's leader
// read every frame) and the ms after the press at which each step showed.
async function heldWalk(page, ms) {
  await h.waitPrompt(page);
  await page.mouse.move(...(await screenPoint(page, 88, 88 + 48)));
  await page.evaluate(() => {
    const c = document.getElementById('screen');
    const seen = [[performance.now(), c.dataset.leader]];
    window.__walk = seen;
    const poll = () => {
      if (window.__walk !== seen) return;
      if (c.dataset.leader !== seen.at(-1)[1]) seen.push([performance.now(), c.dataset.leader]);
      requestAnimationFrame(poll);
    };
    requestAnimationFrame(poll);
  });
  await page.mouse.down();
  await h.sleep(ms);
  await page.mouse.up();
  await page.waitForFunction(() => document.getElementById('screen').dataset.keys === '0', null, { timeout: 30_000 });
  await h.waitPrompt(page);
  const seen = await page.evaluate(() => { const s = window.__walk; window.__walk = null; return s; });
  return { steps: seen.length - 1, at: seen.slice(1).map(([t]) => Math.round(t - seen[0][0])) };
}

// The original's steps for the same holds in the same start (the throne-room fight), measured
// in the oracle with tools/oracle/pace.mjs: a 5-frame (100 ms) hold walks one cell when the game
// samples it, a 50-frame (1 s) hold walks one cell (its first world turn takes 420..470 frames).
test('the mouse: a press held on the map walks at the original\'s pace: 100 ms one cell, 1 s the original\'s count', async (env) => {
  const { page } = await h.openPage(env, { gated: false });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, 130);
  const click = await heldWalk(page, 100);
  assert.equal(click.steps, 1, `a 100 ms press walks one cell: ${JSON.stringify(click)}`);
  const held = await heldWalk(page, 1000);
  assert.ok(held.steps >= 1 && held.steps <= 2, `a 1 s press walks the original's one cell, within one: ${JSON.stringify(held)}`);
  console.log(`  receipt: held 100 ms = ${click.steps} cell; held 1 s = ${held.steps} cells, at ms ${JSON.stringify(held.at)} after the press`);
}, { needs: h.haveHfes });

test('zip: the first press on the canvas is one game click and starts the sound; Look, walk, Mute (receipt)', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await h.dropFiles(page, [h.ZIP]);
  await started(page);
  await h.waitPrompt(page);
  assert.equal((await h.state(page)).audio, 'off', 'no sound before the first press');
  await mkdir(OUT, { recursive: true });
  const before = await h.frame(page);
  // The page's first press: the Look icon. The game takes it once (Look waits for its target;
  // a second Look while it waits would be the no-target word and bring the prompt back), and
  // the same press starts the AudioContext.
  await page.mouse.click(...(await screenPoint(page, 64, 184)));
  await page.waitForFunction(() => document.getElementById('screen').dataset.prompt === '0', null, { timeout: 10_000 });
  await page.waitForFunction(() => document.body.dataset.audio === 'running', null, { timeout: 15_000 });
  await h.sleep(1500);
  assert.equal(await promptNow(page), '0', 'Look still waits for its target: the press came once');
  await page.keyboard.press('Escape'); // no target: the no-target word, the prompt again
  await h.waitPrompt(page);
  const described = await lookAtAvatar(page, true);
  assert.ok(scrollChanged(before, described), 'Look printed in the scroll');
  const { from, to } = await clickStep(page);
  await h.sleep(1000);
  assert.equal((await h.state(page)).leader, to, 'one step per click, none after it');
  await writeFile(`${OUT}/web-walk.png`, await page.locator('#screen').screenshot());
  await page.waitForFunction(() => Number(document.body.dataset.audioChunks ?? 0) > 5, null, { timeout: 15_000 });
  const s = await h.state(page);
  assert.equal(s.audio, 'running');
  await page.click('#mute');
  assert.equal(await page.getAttribute('#mute', 'aria-pressed'), 'true');
  assert.equal((await h.state(page)).leader, to, 'Mute is not a game click');
  await page.click('#mute');
  assert.equal(await page.getAttribute('#mute', 'aria-pressed'), 'false');
  console.log(`  receipt: first press -> Look waits, audioContext.state=${s.audio}, chunks=${s.audioChunks}; click walk ${from} -> ${to}`);
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

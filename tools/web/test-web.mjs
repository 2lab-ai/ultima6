// Browser tests of site/ under its real headers (tools/web/serve.mjs applies site/_headers):
// the operation queue (rapid imports, import during the restore, forget during a save), error
// handling (read and forget failures, an engine trap and Restart), keyboard focus, the CSP,
// saved games (stored after a Save, resumed on reload, forgotten, imported as a folder, a
// replaced game's Save never stored over its replacement's, a Save after a forget stored
// whole, a folder file over the save limit left out), the sound output (AudioContext after a
// gesture, chunks into the worklet; a game whose MED tables fail the checks plays silent) and the
// launcher's sequence as a player meets it (the opening, the menu, a character created and
// kept, the menu's J after a reload, Quit), the mouse on the canvas (the first press is
// both the sound's gesture and exactly one game click) and the fixed step with its speed
// control (spec 06: the steps follow the clock on a virtual 60 Hz and 144 Hz display and on
// the real one, the speed keys and the keys the game keeps, the badge, the buttons, the stored
// choice, a held press at 1x whatever the speed, the music above 4x). Most tests start `game` at once
// (harness `gameFirst`); the launcher tests run the page as it is.
// Gates hold file reads and storage calls at chosen points, so every interleaving is forced,
// not timed. The game is read once it reached what a check asserts, by its own signals (the
// prompt, the key queue, the program, its ticks: the page's fixed steps of 20 ms), never after a
// fixed sleep: a loaded host draws fewer frames per second, a frame moves the game at most
// 250 ms (the pace's cap) and a host past its frame budget drops steps, so a sleep there
// covers less of the game. Tests that need the user's disks skip when they are absent.
//
//   node tools/web/test-web.mjs [name-filter]      (build first: bash scripts/build-web.sh)
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import * as h from './harness.mjs';

const MARKER = { name: 'port-test-marker.txt', text: 'not a game file' };
// The game files an import of the user's disks keeps: counted from their decrunched copy, else
// the count the first started import showed, a run-time baseline every later import and
// storage check compares with (no count of the user's files is written here). `FILES` and
// `WITH_MARKER` (one more: the marker) stand for that count in `started` and `kept`.
let baseline = h.gameFileCount();
const FILES = Symbol('the game files');
const WITH_MARKER = Symbol('the game files and the marker');
const count = (want) => {
  if (want === FILES) return baseline;
  return want === WITH_MARKER ? baseline + 1 : want;
};
const tests = [];
const test = (name, fn, { needs } = {}) => tests.push({ name, fn, needs });

// Output rule (spec 01 §Rights): what a test prints, its receipts and its failure messages,
// holds labels, indices, counts of what the test itself made and pass/fail only; the page's
// status text, positions, counts of the user's files and the lengths of their records or paths
// (a count of them) are compared, never printed. A failure shows its label or kind and its
// places in these files, never its message (the runner, h.failureLines).

// The page once a start finished (nothing queued) and the started game ran a frame. The page
// writes the canvas fields (leader, prompt, program, name, keys) only in its frame callback,
// and a read can come before the first frame: they would be unset, or the replaced game's.
async function started(page, files) {
  await h.waitSettled(page);
  await h.waitFrames(page, 1);
  const s = await h.state(page);
  assert.ok(s.state === 'running', 'the game runs');
  if (files !== undefined) {
    if (baseline === undefined && files === FILES) baseline = s.files;
    if (baseline === undefined && files === WITH_MARKER) baseline = s.files - 1;
    assert.ok(typeof files === 'number' || (Number.isInteger(baseline) && baseline > 0), 'a count of game files');
    assert.ok(s.files === count(files), 'the files the start kept');
  }
  return s;
}

// The kept paths are as many as `want` stands for (`FILES` or `WITH_MARKER`); before any
// started import the baseline is the count of the game on screen.
async function kept(page, want, label) {
  if (baseline === undefined) {
    const files = (await h.state(page)).files;
    baseline = want === WITH_MARKER ? files - 1 : files;
    assert.ok(Number.isInteger(baseline) && baseline > 0, 'a count of game files');
  }
  const paths = await h.keptPaths(page);
  assert.ok(paths !== null && paths.length === count(want), label);
  return paths;
}

// The page once the game waits at its prompt (its boot and the start fight's turns ran): where
// a started or resumed game's party stands is read here.
async function atPrompt(page) {
  await h.waitPrompt(page);
  return h.state(page);
}

const promptNow = (page) => page.evaluate(() => document.getElementById('screen').dataset.prompt);

// No page: the harness's content-free comparison (synthetic values) names paths, whether the
// lengths differ and the index only; a prefix without an index (it would be the shorter
// length), equal elements of two kinds as such.
test('assertSame: a mismatch names the label, the paths, equal or differing lengths and the index, never the values', async () => {
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
  assert.equal(store, 'the store: savegame/objblkaa: lengths equal, first difference at index 1; savegame/objlist: not expected');
  const name = failure(() => h.assertSame('Zhuge', 'Zhugx', 'the name'));
  assert.equal(name, 'the name: lengths equal, first difference at index 4');
  const list = failure(() => h.assertSame([{ path: 'a', bytes: [1] }], [{ path: 'a', bytes: [2, 3] }], 'files'));
  assert.equal(list, 'files: [0].bytes: lengths differ, first difference at index 0');
  const kinds = failure(() => h.assertSame({ a: 'xyz', b: [1, 2, 3, 4] }, { a: 7, b: null }, 'kinds'));
  assert.equal(kinds, 'kinds: a: a string and a number; b: a sequence and null');
  const prefix = failure(() => h.assertSame({ files: ['a', 'b', 'c'] }, { files: ['a', 'b', 'c', 'd', 'e'] }, 'paths'));
  assert.equal(prefix, 'paths: files: lengths differ, one is a prefix of the other');
  const shorter = failure(() => h.assertSame('Zhug', 'Zhuge', 'the name'));
  assert.equal(shorter, 'the name: lengths differ, one is a prefix of the other');
  const typed = failure(() => h.assertSame(new Uint8Array([1, 2]), [1, 2], 'bytes'));
  assert.equal(typed, 'bytes: the same elements, different kinds');
  for (const leak of ['83', '88', 'Zhug', '3', '5']) {
    assert.ok(![store, name, list, kinds, prefix, shorter, typed].some((m) => m.includes(leak)), `a message shows ${leak}`);
  }
  assert.ok(![prefix, shorter, typed].some((m) => /\d/.test(m)), 'a prefix or a kind shows no number');
});

// No page: the held walk's pace floor on synthetic step times (ms after the press, floor 500),
// so the check is exercised even when a boot walks one cell.
test('the held-walk pace floor: a late first step does not let the next come sooner; a step at the pace passes', async () => {
  const f = 500;
  for (const [shown, label] of [[[], 'no step'], [[0], 'the press only'], [[12, 660, 1320], 'a repeat each pace'],
    [[400, 900], 'a late first step, the next a floor after it'], [[0, 500, 1000], 'repeats at the floor itself']]) {
    assert.ok(notSooner(shown, f), `passes: ${label}`);
  }
  for (const [shown, label] of [[[500, 500], 'two steps at once after a late first'],
    [[400, 700], 'a late first step, the next a short gap after it'], [[0, 499], 'a repeat before the floor'],
    [[0, 660, 1000], 'a fast third step'], [[-100, 450], 'a step before the press, the next within its floor of the press']]) {
    assert.ok(!notSooner(shown, f), `fails: ${label}`);
  }
});

// The runner's failure lines (h.failureLines) on errors whose text holds a sentinel: plain,
// multi-line with lines shaped as frames of this directory, a thrown string, a page-chosen name,
// assertions with a label and generated, Playwright's own (a page's thrown error, a timeout on
// a selector), a frame of another file in this directory, a thrown object with a stack and an
// Error whose stack lost its message. None shows the sentinel; each shows its fixed head, and
// places in this file only where they are an Error's frames after its message.
test('the runner shows a failure by its label or kind and its places here, never its message', async (env) => {
  const S = 'QX7SENTINELZ9';
  const caught = async (f) => {
    try { await f(); } catch (err) { return err; }
    throw new Error('no failure');
  };
  const here = (lines) => lines.slice(1).some((l) => /^at test-web\.mjs:\d+:\d+$/.test(l));
  const fake = new URL(`${S}.mjs`, import.meta.url).href;
  const named = new Error(S);
  named.name = S;
  const thisFile = new URL(import.meta.url).pathname;
  const other = new Error('m');
  other.stack = `Error: m\n    at f (${new URL(fake).pathname}:3:3)\n    at ${thisFile}:5:5`;
  const object = { name: 'TypeError', message: S, stack: `    at ${new URL(fake).pathname}:1:1\n    at ${thisFile}:4:4` };
  const lost = new Error(S);
  lost.stack = `    at ${thisFile}:7:7`;
  const page = await (await env.browser.newContext()).newPage();
  const cases = [
    ['plain', await caught(() => { throw new Error(S); }), 'Error (its message is not shown)'],
    ['multi-line', await caught(() => { throw new TypeError(`a\n    at ${S} (${fake}:1:1)\n    at ${new URL(fake).pathname}:2:2\n${S}`); }), 'TypeError (its message is not shown)'],
    ['a page-chosen name', named, 'Error (its message is not shown)'],
    ['a labelled assertion', await caught(() => assert.equal(S, `${S}-other`, 'the label')), 'AssertionError: the label'],
    ['a generated assertion', await caught(() => assert.deepEqual([S], [`${S}-other`])), 'AssertionError (the compared values are not shown)'],
    ['a page error', await caught(() => page.evaluate((s) => { throw new Error(s); }, S)), 'Error (its message is not shown)'],
    ['a timeout', await caught(() => page.waitForSelector(`#${S}`, { timeout: 50 })), 'TimeoutError (its message is not shown)'],
    ['another file here', other, 'Error (its message is not shown)'],
    ['an object with a stack', await caught(() => { throw object; }), 'Error (its message is not shown)', false],
    ['a stack without its message', lost, 'Error (its message is not shown)', false],
    ['a string', await caught(() => { throw S; }), 'Error (its message is not shown)', false],
  ];
  for (const [what, err, head, placed = true] of cases) {
    const lines = h.failureLines(err);
    assert.ok(lines[0] === head, `${what}: the head`);
    assert.ok(!lines.join('\n').includes(S), `${what}: no message text`);
    assert.ok(lines.slice(1).every((l) => /^at [\w./-]+\.mjs:\d+:\d+$/.test(l)), `${what}: places only`);
    if (placed) assert.ok(here(lines), `${what}: a place in this file`);
    else assert.ok(lines.length === 1, `${what}: no place`);
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
  assert.ok(s.state === 'idle', 'idle before a game');
  assert.equal(s.pickerDisabled, false);
  const egress = await page.evaluate(() => fetch('https://example.com/').then(() => 'fetched', (e) => `blocked: ${e}`));
  assert.match(egress, /^blocked/);
  const csp = await page.evaluate(() => window.__csp);
  assert.deepEqual(csp, ['connect-src https://example.com/'], 'only the probe was refused');
  if (h.haveHfes()) {
    await page.setInputFiles('#picker', h.HFES);
    await started(page, FILES);
    await h.waitPrompt(page); // the boot ran to the prompt
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
  const s = await started(page, FILES);
  assert.equal(s.focused, 'screen', 'focus lands on the canvas after the import');
  assert.equal(await page.evaluate(() => getComputedStyle(document.activeElement).outlineColor), 'rgb(255, 209, 102)');
  // From the prompt, one step east (west when the start fight blocks it), then the prompt again.
  const { key, before } = await h.stepSideways(page);
  assert.deepEqual(await page.evaluate(() => window.__keys.at(-1)), [key, true], 'the canvas took the arrow');
  const walked = h.mapShift(before, await h.frame(page));
  const want = key === 'ArrowRight' ? 16 : -16;
  assert.ok(walked.shift === want && walked.score > 0.5, `one step (${key}) scrolled the map`);
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
  assert.ok(after.shift === 0, 'the map stayed');
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
  await started(page, WITH_MARKER);
  const log = await h.log(page);
  const firstSave = log.indexOf('save:end:true');
  const secondRead = log.findIndex((e, i) => i > 0 && e.startsWith('read:start') && log.slice(0, i).includes('save:end:true'));
  assert.ok(firstSave > 0 && secondRead > firstSave, 'the second import starts after the first is kept');
  const paths = await kept(page, WITH_MARKER, 'storage holds the second import\'s files');
  assert.ok(paths.includes(MARKER.name), 'storage holds the second import');
  const status = (await h.state(page)).status;
  assert.ok(new RegExp(`The ${paths.length} game files are kept`).test(status), 'the kept count shown');
}, { needs: h.haveHfes });

test('an import during the restore waits for it, then replaces the restored game', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await h.dropFiles(page, h.HFES);
  await started(page, FILES);
  await page.evaluate(() => sessionStorage.setItem('__holds', '["load"]'));
  await page.reload();
  await h.waitLog(page, 'load:start');
  assert.equal((await h.state(page)).pickerDisabled, true, 'no picker during the restore');
  await h.dropFiles(page, h.HFES, [MARKER]);
  const during = await h.state(page);
  assert.equal(during.ops, 2, 'the drop is queued behind the restore');
  assert.ok(!(await h.log(page)).some((e) => e.startsWith('read:')), 'nothing read before the restore');
  await page.evaluate(() => window.__release('load'));
  await started(page, WITH_MARKER);
  const log = await h.log(page);
  const restored = log.indexOf(`load:end:${count(FILES)}`);
  assert.ok(restored >= 0 && restored < log.findIndex((e) => e.startsWith('read:start')), 'the restore ends before the read');
  await kept(page, WITH_MARKER, 'the import replaced the restored files');
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
  assert.ok(log.indexOf('save:end:true') < log.indexOf('forget:start'), 'the save before the forget');
  await kept(page, FILES, 'the save committed first');
  assert.doesNotMatch((await h.state(page)).status, /were removed/, 'no success before the delete');
  await page.evaluate(() => window.__release('forget'));
  await h.waitSettled(page);
  s = await h.state(page);
  assert.equal(await h.keptPaths(page), null, 'storage empty');
  assert.match(s.status, /The kept game files were removed/);
  assert.equal(s.state, 'running');
  assert.ok(s.files === count(FILES), 'the game on screen keeps running');
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
  await started(page, FILES);
  await page.evaluate(() => { globalThis.__trap = true; });
  await page.waitForFunction(() => document.body.dataset.state === 'crashed');
  let s = await h.state(page);
  assert.equal(s.restartHidden, false);
  assert.match(s.status, /internal error[\s\S]*Restart/);
  const ticks = await page.evaluate(() => globalThis.__ticks);
  await h.waitFrames(page, 30); // a running loop would step in most of them
  assert.equal(await page.evaluate(() => globalThis.__ticks), ticks, 'not ticked again');
  await page.click('#restart');
  await page.waitForLoadState('load');
  await started(page, FILES);
  s = await h.state(page);
  assert.equal(s.focused, 'screen', 'the user asked: focus on the canvas');
  assert.equal(s.restartHidden, true);
}, { needs: h.haveHfes });

test('A on the canvas: Attack asks for a target, a stray key ends it, then the arrow walks (screenshot)', async (env) => {
  const { page } = await h.openPage(env, { gated: false });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  await h.press(page, 'a', 30);
  assert.equal(await promptNow(page), '0', 'Attack waits for its target');
  await h.press(page, 'q', 0); // confirms no target: the no-target word, the turn (the start fight) runs
  await h.waitPrompt(page);
  await writeFile('/tmp/claude-501/m2-attack-what.png', await page.locator('#screen').screenshot());
  const { key, before } = await h.stepSideways(page);
  const walked = h.mapShift(before, await h.frame(page));
  const want = key === 'ArrowRight' ? 16 : -16;
  assert.ok(walked.shift === want && walked.score > 0.5, `one step (${key}) after A`);
}, { needs: h.haveHfes });

test('Ctrl+S and Ctrl+R on the canvas are the game\'s Save and Restore, not the browser\'s', async (env) => {
  const { page } = await h.openPage(env, { gated: false });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  for (const key of ['Control+s', 'Control+r']) {
    await h.press(page, key, 30);
    const code = key === 'Control+s' ? 'KeyS' : 'KeyR';
    assert.deepEqual(await page.evaluate(() => window.__keys.at(-1)), [code, true], `${key} taken by the canvas`);
    assert.equal(await promptNow(page), '0', `${key}: the game asks its question`);
    await h.press(page, 'n', 0); // the question's answer: no
    await h.waitPrompt(page);
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
  await h.press(page, 'Control+s', 30);
  await h.press(page, 'y', 60);
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
  await started(page, FILES);
  const { start, moved } = await walkAndSave(page);
  const saved = await h.savedPaths(page);
  assert.ok(saved.includes('savegame/objlist') && saved.length > 1, 'the objlist and blocks saved');
  assert.ok((await h.log(page)).includes('saves:store:end'), 'stored through the queue');
  await page.reload();
  let s = await started(page, FILES);
  assert.equal(s.saves, saved.length, 'the stored save is laid over the game files');
  assert.equal((await atPrompt(page)).leader, moved, 'resumed where it was saved');
  assert.match(s.status, /the game saved in this browser, resumed/);
  await page.click('#forget-save');
  await h.waitSettled(page);
  assert.deepEqual(await h.savedPaths(page), [], 'the save is gone');
  await kept(page, FILES, 'the game files stay');
  assert.match((await h.state(page)).status, /The saved game was removed/);
  await page.reload();
  s = await started(page, FILES);
  assert.equal(s.saves, 0);
  assert.equal((await atPrompt(page)).leader, start, 'a new game');
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
  await started(page, FILES);
  const { moved } = await walkAndSave(page);
  const folder = asFolder(await h.savedFiles(page));
  await page.click('#forget-save');
  await h.waitSettled(page);
  assert.deepEqual(await h.savedPaths(page), []);
  await h.dropFiles(page, [], folder);
  const s = await started(page, FILES);
  assert.match(s.status, /The saved game \(\d+ files\) was imported/);
  assert.equal(s.saves, folder.length);
  assert.equal((await atPrompt(page)).leader, moved, 'the imported save resumes');
  assert.equal((await h.savedPaths(page)).length, folder.length, 'one record per file, any case');
}, { needs: h.haveHfes });

test('a Save of the game an import replaces, made while the import reads: the imported save wins', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
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
  let s = await started(page, FILES);
  const stores = (await h.log(page)).slice(from).filter((e) => e.startsWith('saves:store:start'));
  assert.deepEqual(stores, [`saves:store:start:${folder.length}`], 'only the import stored');
  h.assertSame(byName(await h.savedFiles(page)), byName(folder), 'the store holds the imported files');
  assert.match(s.status, /The saved game \(\d+ files\) was imported/);
  assert.equal(s.saved, 1, 'the replaced game\'s Save was not stored');
  assert.equal((await atPrompt(page)).leader, moved, 'the imported save resumes');
  await page.reload();
  await started(page, FILES);
  assert.equal((await atPrompt(page)).leader, moved, 'a reload resumes the imported save');
}, { needs: h.haveHfes });

test('an import that does not start leaves the stored save; the game on screen saves on', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await walkAndSave(page);
  const stored = await h.savedFiles(page);
  // A folder with only the objlist, and no game files: the import cannot start.
  const folder = asFolder(stored.filter(({ path }) => path === 'savegame/objlist'));
  assert.ok(folder.length === 1 && stored.length > 1, 'an objlist-only folder of a larger save');
  await h.dropFiles(page, [], [...folder, MARKER]);
  await h.waitSettled(page);
  let s = await h.state(page);
  assert.match(s.status, /Could not start the game/);
  h.assertSame(byName(await h.savedFiles(page)), byName(stored), 'the stored save is as it was');
  await page.focus('#screen');
  await h.waitPrompt(page);
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
  await started(page, FILES);
  await walkAndSave(page);
  // The game on screen starts from a save holding a block its Saves near the start never
  // rewrite (synthetic: empty; the last block the first Save left out).
  const first = await h.savedFiles(page);
  const listed = new Set(first.map(({ path }) => path.toLowerCase()));
  const far = [...Array(64).keys()].reverse().map(h.blockFile).find((p) => !listed.has(p));
  assert.ok(far, 'a Save near the start leaves blocks out');
  const folder = [...asFolder(first), { name: `PLAYER/${far.toUpperCase()}`, bytes: [0, 0] }];
  await h.dropFiles(page, [], folder);
  let s = await started(page, FILES);
  assert.equal(s.saves, folder.length);
  // A Save taken while the forget runs is not stored after the delete.
  await page.evaluate(() => window.__hold('saves-forget'));
  await page.click('#forget-save');
  await h.waitLog(page, 'saves:forget:start');
  await page.focus('#screen');
  await h.waitPrompt(page); // the started game's boot and the start fight ran
  await h.press(page, 'Control+s', 30);
  await h.press(page, 'y', 60);
  await page.waitForFunction(() => globalThis.__saveGen === 1, null, { timeout: 30_000 });
  await page.waitForFunction(() => document.body.dataset.ops === '2', null, { timeout: 10_000 });
  await page.evaluate(() => window.__release('saves-forget'));
  await h.waitSettled(page);
  assert.deepEqual(await h.savedPaths(page), [], 'the Save taken before the forget was not stored');
  assert.match((await h.state(page)).status, /The saved game was removed/);
  // The next Save stores the whole game: the objlist and all 69 blocks, the far one included.
  const { to: later } = await h.stepSideways(page);
  await h.press(page, 'Control+s', 30);
  await h.press(page, 'y', 60);
  await page.waitForFunction(() => document.body.dataset.saved === '2', null, { timeout: 60_000 });
  await h.waitSettled(page);
  const stored = byName(await h.savedFiles(page));
  assert.equal(Object.keys(stored).length, h.WHOLE_SAVE, 'the objlist and every block');
  h.assertSame(stored[far], [0, 0], 'the far block as the game had it');
  await page.reload();
  s = await started(page, FILES);
  assert.equal(s.saves, h.WHOLE_SAVE, 'the whole save is laid over the game files');
  assert.equal((await atPrompt(page)).leader, later, 'resumed where it was saved');
}, { needs: h.haveHfes });

test('a Save of a game that did not own the stored save, then the forget: it is not stored; the next Save is, whole', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  // No kept files: a drop of only a savegame folder starts nothing, it becomes the stored save
  // and the game on screen no longer owns it (synthetic: one empty block file).
  await page.click('#forget');
  await h.waitSettled(page);
  assert.equal(await h.keptPaths(page), null);
  await h.dropFiles(page, [], [{ name: 'PLAYER/SAVEGAME/OBJBLKHH', bytes: [0, 0] }]);
  await h.waitSettled(page);
  let s = await h.state(page);
  assert.ok(s.state === 'running', 'the game runs on');
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
  assert.equal((await h.savedPaths(page)).length, h.WHOLE_SAVE, 'the objlist and every block');
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
  await started(page);
  s = await atPrompt(page);
  await writeFile('/tmp/claude-501/int-b-web-restored.png', await page.locator('#screen').screenshot());
  console.log(`  receipt: saved, after the reload at the saved place: ${s.leader === moved}`);
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
  await started(page, FILES);
  await page.click('#screen');
  await page.waitForFunction(() => document.body.dataset.audio === 'running', null, { timeout: 15_000 });
  assert.equal(silent.length, 0, 'the user\'s game has its audio machine');
  // The sound is on: the boot of the new game finds no audio machine for it.
  const dropped = await dropWithoutMedTables(page);
  assert.ok(dropped === count(FILES), 'every kept file dropped again');
  let s = await started(page);
  assert.match(s.status, /Started\./, 'the dropped files started');
  assert.equal(silent.length, 1, 'the boot said the sound is off');
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
  assert.equal(silent.length, 2, 'the gesture said the sound is off');
  s = await h.state(page);
  assert.ok(s.state === 'running', 'the game runs');
  ticks = await page.evaluate(() => globalThis.__ticks);
  step = await h.stepSideways(page);
  assert.notEqual(step.to, step.from, 'the game plays on');
  assert.ok(await page.evaluate(() => globalThis.__ticks) > ticks, 'the engine ticks');
  console.log('  receipt: the files dropped with the period table zeroed; sound off at the boot and at the gesture, the game walks after each');
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
  console.log(`  receipt: created ${s.name}; after the reload J in the menu plays ${s.name}`);
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
  await h.waitFrames(page, 30); // a running loop would step in most of them
  assert.equal(await page.evaluate(() => globalThis.__ticks), ticks, 'not ticked after the end');
  await page.click('#restart');
  await page.waitForLoadState('load');
  s = await started(page);
  assert.equal(s.restartHidden, true);
  await h.waitProgram(page, 'u');
}, { needs: h.haveZip });

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

// Clicks the map cell `dx` cells east of the hero (its window cell (5, 5) is centred on
// game pixel (88, 88)), then waits until the game took the click and shows the prompt again;
// the leader before and after.
async function clickCell(page, dx) {
  await h.waitPrompt(page);
  const from = (await h.state(page)).leader;
  await h.click(page, 88 + 16 * dx, 88, 0);
  await h.waitPrompt(page);
  return { from, to: (await h.state(page)).leader };
}

// Look by clicks: the Look icon, then the hero's own cell. Look on a person shows the portrait
// panel and waits for a key or a click; a click on the hero's cell answers it (0x8f, nothing
// else happens). Returns the frame with the description in the scroll, read 90 ticks after the
// game took the target click (1.8 s of the game's time on a gated page, 90 frames elsewhere).
async function lookAtHero(page, shot) {
  await h.click(page, 64, 184, 30); // the fourth icon: Look
  assert.equal(await promptNow(page), '0', 'Look waits for its target');
  if (shot) await writeFile(`${OUT}/web-look-target.png`, await page.locator('#screen').screenshot());
  await h.click(page, 88, 88, 90); // Look's target: the hero
  assert.equal(await promptNow(page), '0', 'the portrait waits for a key or a click');
  const described = await h.frame(page);
  if (shot) await writeFile(`${OUT}/web-look.png`, await page.locator('#screen').screenshot());
  const at = (await h.state(page)).leader;
  await h.click(page, 88, 88, 0);
  await h.waitPrompt(page);
  assert.equal((await h.state(page)).leader, at, 'the click that closes the portrait does not walk');
  return described;
}

// One step by clicks: the cell east of the hero, else (blocked by the start fight) west,
// up to the harness's `STEP_TRIES` tries, a pass after each blocked one (as `stepSideways`:
// after a blocked step the world turn picks the hero again at once, no NPC moves); asserts the
// step is one cell along x.
async function clickStep(page) {
  for (let i = 0; i < h.STEP_TRIES; i += 1) {
    const dx = i % 2 === 0 ? 1 : -1;
    const { from, to } = await clickCell(page, dx);
    if (to === from) {
      await h.passTurn(page); // the click focused the canvas
      continue;
    }
    const [fx, fy, fz] = from.split(',').map(Number);
    const [tx, ty, tz] = to.split(',').map(Number);
    assert.ok(tx - fx === dx && ty === fy && tz === fz, `one cell ${dx > 0 ? 'east' : 'west'}`);
    return { dx, from, to };
  }
  throw new Error(`${h.STEP_TRIES} click steps blocked in a row`);
}

test('the mouse: the Look icon, then Look on the hero, then clicks walk; the keys still work', async (env) => {
  const { page } = await h.openPage(env, { gated: false });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  const before = await h.frame(page);
  const described = await lookAtHero(page, false);
  assert.ok(scrollChanged(before, described), 'Look printed in the scroll');
  assert.equal((await h.state(page)).focused, 'screen', 'the click focused the canvas');
  await clickStep(page);
  const { from, to } = await h.stepSideways(page);
  assert.notEqual(to, from, 'the arrows still walk after the mouse');
}, { needs: h.haveHfes });

// Holds the left button at game pixel (x, y) for 300 ms, longer (up to 30 s) until five engine
// ticks ran (a loaded host draws fewer frames in 300 ms); returns the ticks run during the
// hold, which the caller checks. A page whose step never returns answers no evaluate: that is
// a frozen tab, not a slow one; a loop that stopped answers with a count that does not grow.
async function holdOnMap(page, x, y) {
  const ticks = () => Promise.race([
    page.evaluate(() => globalThis.__ticks),
    h.sleep(5000).then(() => { throw new Error('the tab froze during the hold'); }),
  ]);
  await page.mouse.move(...(await h.screenPoint(page, x, y)));
  const t0 = await ticks();
  await page.mouse.down();
  await h.sleep(300);
  let t1 = await ticks();
  for (const end = Date.now() + 30_000; t1 - t0 < 5 && Date.now() < end; t1 = await ticks()) {
    await h.sleep(20);
  }
  await page.mouse.up();
  return t1 - t0;
}

test('the mouse: a press held 300 ms on the map walks and yields; held through the exit question it walks nowhere and N gets the prompt back', async (env) => {
  const { page } = await h.openPage(env); // gated: the glue counts the ticks
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  // Three cells south of the hero, at the prompt.
  const ran = await holdOnMap(page, 88, 88 + 48);
  assert.ok(ran >= 5, `the engine ticked through the hold: ${ran} ticks`);
  await h.waitPrompt(page);
  // Ctrl-Q: the exit question waits for Y or N; a held map press is no answer.
  await h.press(page, 'Control+q', 30);
  assert.equal(await promptNow(page), '0', 'the exit question waits');
  const asked = (await h.state(page)).leader;
  const during = await holdOnMap(page, 88 + 48, 88);
  assert.ok(during >= 5, `the engine ticked through the hold in the question: ${during} ticks`);
  await h.waitTicks(page, 30);
  assert.equal(await promptNow(page), '0', 'still asking after the release');
  assert.equal((await h.state(page)).leader, asked, 'the question took no walk');
  await page.keyboard.press('n');
  await h.waitPrompt(page);
  assert.equal((await h.state(page)).leader, asked, 'N: the prompt again, in place');
  console.log(`  receipt: held 300 ms at the prompt = ${ran} ticks; in the exit question = ${during} ticks, no step, N back to the prompt`);
}, { needs: h.haveHfes });

// The original's fastest held walk: the shortest gap between two steps of one held press, 25
// PAL frames (20 ms each), measured in the oracle with tools/oracle/pace.mjs (re/notes/oracle.md
// §4 "Held-walk pace", the table's Min over runs pace/run-b1..b6). The test's own figure, not
// the port's constant: no repeat may come sooner than the original's did. The port's exact
// pace rule is pinned by the Rust tests (ui/tests/pointer_tests.rs).
const ORIGINAL_FASTEST_REPEAT_MS = 25 * 20;

// Whether the steps of one held walk, shown at `shown` ms after the press (the first is the
// press's own step), keep the original's pace floor `floor` ms: step n at least n floors after
// the press, and each repeat at least a floor after the step before it (a late first step does
// not let the next one come sooner).
function notSooner(shown, floor) {
  return shown.every((t, n) => t >= n * floor && (n === 0 || t - shown[n - 1] >= floor));
}

// Holds the left button at game pixel (x, y) from the prompt until the game ran `ms` on its
// clock since it took the press, then releases (the oracle's holds are frames of the game's
// time; a wall-time hold reaches a loaded page's engine shorter, a frame moving it at most
// 250 ms). The glue stamps the press and the release with the clock of the step that took
// them (h.openPage, gated). Returns the cells the leader moved until the prompt is back (the
// page's leader read every frame), the engine ms after the press at which each showed (at or
// after the step that walked it) and the hold the game took: the release's clock less the
// press's.
async function heldWalk(page, ms, [x, y]) {
  await h.waitPrompt(page);
  await page.mouse.move(...(await h.screenPoint(page, x, y)));
  await page.evaluate(() => {
    const c = document.getElementById('screen');
    const seen = [[globalThis.__clock, c.dataset.leader]];
    window.__walk = seen;
    globalThis.__buttons.length = 0;
    const poll = () => {
      if (window.__walk !== seen) return;
      if (c.dataset.leader !== seen.at(-1)[1]) seen.push([globalThis.__clock, c.dataset.leader]);
      requestAnimationFrame(poll);
    };
    requestAnimationFrame(poll);
  });
  await page.mouse.down();
  await page.waitForFunction((hold) => {
    const press = globalThis.__buttons.find(([b]) => b === 1);
    return press !== undefined && globalThis.__clock >= press[1] + hold;
  }, ms, { timeout: 60_000, polling: 'raf' });
  await page.mouse.up();
  await h.waitTaken(page);
  await h.waitPrompt(page);
  const { seen, buttons } = await page.evaluate(() => {
    const walk = window.__walk;
    window.__walk = null;
    return { seen: walk, buttons: [...globalThis.__buttons] };
  });
  const press = buttons.findIndex(([b]) => b === 1);
  const release = buttons.findIndex(([b], i) => i > press && b === 0);
  assert.ok(press >= 0 && release > press, 'the game took the press and its release');
  const pressed = buttons[press][1];
  return {
    steps: seen.length - 1,
    shown: seen.slice(1).map(([t]) => t - pressed),
    hold: buttons[release][1] - pressed,
  };
}

// The cells three away from the hero's (its window cell (5, 5) is centred on game pixel
// (88, 88)): south, north, east, west.
const HOLD_CELLS = [[88, 88 + 48], [88, 88 - 48], [88 + 48, 88], [88 - 48, 88]];

// A held walk from the boot as it is: the seed is the clock's, so in some boots the start
// fight puts a foe in the cell beside the hero the press walks toward. The game takes that
// press (waitTaken) and walks no cell; a blocked step costs no movement points, so the world
// turn after it picks the hero again at once and no NPC moves (the prompt is back, not after
// the fight's turns): the boot is as fresh for the next direction, up to the four of them (as
// `stepSideways` tries the other way). The receipt names the direction that walked, so a walk
// broken in one direction shows there although the retry passes.
async function freshHeldWalk(page, ms) {
  for (const [i, cell] of HOLD_CELLS.entries()) {
    const walk = await heldWalk(page, ms, cell);
    if (walk.steps > 0) return { ...walk, direction: i };
  }
  assert.fail('a held press walks a cell in one of the four directions');
}

// The original's steps for the same holds in the same start (the throne-room fight as booted),
// measured in the oracle with tools/oracle/pace.mjs, each from a fresh boot: a 5-frame (100 ms)
// hold walks one cell when the game samples it, a 50-frame (1 s) hold walks one cell (its first
// world turn takes 422..473 frames). So each hold here starts from a fresh boot too and is
// timed on the engine clock; no repeat comes sooner than the original's fastest (with one cell
// per hold, the usual boot, there is no repeat to time and the receipt says so).
test('the mouse: a press held on the map walks at the original\'s pace: 100 ms one cell, 1 s the original\'s count', async (env) => {
  const boot = async () => {
    const opened = await h.openPage(env); // gated: the glue keeps the engine clock
    await h.load(opened.page, env);
    await opened.page.setInputFiles('#picker', h.HFES);
    await started(opened.page, FILES);
    return opened;
  };
  const first = await boot();
  const click = await freshHeldWalk(first.page, 100);
  await first.context.close();
  const second = await boot();
  const held = await freshHeldWalk(second.page, 1000);
  // Before the checks, so a failing run shows what the game took.
  const line = (ms, w) => `${ms} ms held ${w.hold} ms on the engine clock, ${w.steps} cell${w.steps === 1 ? '' : 's'} (direction ${w.direction})`;
  const repeats = Math.max(0, click.steps - 1) + Math.max(0, held.steps - 1);
  const timed = repeats === 0
    ? 'no repeat to time against the original\'s fastest (one cell each)'
    : `${repeats} repeat${repeats === 1 ? '' : 's'} timed against the original's fastest`;
  console.log(`  receipt: from fresh boots, ${line(100, click)}; ${line(1000, held)}; ${timed}`);
  // Harness guards: the release reached the game after the asked hold (heldWalk waits for the
  // clock) and within the original's fastest repeat after it (a release held back by host load
  // would otherwise read as a pace fault).
  const late = 'the harness released within the original\'s fastest repeat';
  assert.ok(click.hold >= 100 && held.hold >= 1000, 'the harness: no early release reached the game');
  assert.ok(click.hold < ORIGINAL_FASTEST_REPEAT_MS, late);
  assert.ok(held.hold < 1000 + ORIGINAL_FASTEST_REPEAT_MS, `${late} after the 1 s hold`);
  assert.ok(click.steps === 1, 'a 100 ms press walks one cell');
  assert.ok(held.steps >= 1 && held.steps <= 2, 'a 1 s press from a fresh boot walks the original\'s one cell, within one');
  const floor = ORIGINAL_FASTEST_REPEAT_MS;
  assert.ok(notSooner(click.shown, floor) && notSooner(held.shown, floor), 'no repeat sooner than the original\'s fastest');
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
  await h.click(page, 64, 184, 0);
  await page.waitForFunction(() => document.body.dataset.audio === 'running', null, { timeout: 15_000 });
  await h.waitTicks(page, 90); // a second Look through the sound's start would show by now
  assert.equal(await promptNow(page), '0', 'Look still waits for its target: the press came once');
  await page.keyboard.press('Escape'); // no target: the no-target word, the prompt again
  await h.waitPrompt(page);
  const described = await lookAtHero(page, true);
  assert.ok(scrollChanged(before, described), 'Look printed in the scroll');
  const { from, to } = await clickStep(page);
  await h.waitTicks(page, 60);
  assert.equal((await h.state(page)).leader, to, 'one step per click, none after it');
  await writeFile(`${OUT}/web-walk.png`, await page.locator('#screen').screenshot());
  await page.waitForFunction(() => Number(document.body.dataset.audioChunks ?? 0) > 5, null, { timeout: 15_000 });
  const s = await h.state(page);
  assert.equal(s.audio, 'running');
  await page.click('#mute');
  assert.equal(await page.getAttribute('#mute', 'aria-pressed'), 'true');
  await h.waitTicks(page, 30); // a click the game took would have walked by now
  assert.equal((await h.state(page)).leader, to, 'Mute is not a game click');
  await page.click('#mute');
  assert.equal(await page.getAttribute('#mute', 'aria-pressed'), 'false');
  console.log(`  receipt: first press -> Look waits, audioContext.state=${s.audio}, chunks=${s.audioChunks}; the click walked ${from !== to}`);
}, { needs: h.haveZip });

// --- the fixed step and the speed (spec 06) -------------------------------------------------

const STEP_MS = 20; // one PAL frame: every step the page runs (u6-core pacer::STEP_MS)

// What the page shows of the speed: data-speed (1, 1.5, .. 8, max), the badge's state and text,
// the Speed group's label.
const speedShown = (page) => page.evaluate(() => ({
  speed: document.getElementById('screen').dataset.speed,
  badge: !document.getElementById('speed-badge').hidden,
  text: document.getElementById('speed-badge').textContent,
  label: document.getElementById('speed-label').textContent,
}));

// What the glue saw on a gated page (h.openPage): the steps run and the engine clock, the
// game's speed (0 = Max) and its lost time after the last frame, the keys the game keeps, the
// keys sent to it.
const glue = (page) => page.evaluate(() => ({
  steps: globalThis.__ticks,
  clock: globalThis.__clock,
  speed: globalThis.__speed,
  lost: globalThis.__dropped + globalThis.__clamped,
  owned: globalThis.__owned,
  sent: globalThis.__sent.length,
  lastSent: globalThis.__sent.at(-1),
}));

// Waits until the running game took speed `x` (the glue reads it after each frame).
const engineSpeed = (page, x) => page.waitForFunction((want) => globalThis.__speed === want, x,
  { timeout: 10_000 });

// A gated page (gameFirst) with the user's disks started; the canvas focused.
async function bootGame(env) {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await page.focus('#screen');
  return page;
}

// On the real clock, about `ms` of wall time from one display frame to a later one: the wall
// time between them, the steps run and the clock advanced in it, and whether any time was lost
// (dropped_ms or clamped_ms moved).
function realRun(page, ms) {
  return page.evaluate(async (wait) => {
    const read = (t) => ({ t, steps: globalThis.__ticks, clock: globalThis.__clock, lost: globalThis.__dropped + globalThis.__clamped });
    const a = read(await new Promise(requestAnimationFrame));
    await new Promise((done) => setTimeout(done, wait));
    const b = read(await new Promise(requestAnimationFrame));
    return { wall: b.t - a.t, steps: b.steps - a.steps, clock: b.clock - a.clock, lost: b.lost !== a.lost };
  }, ms);
}

test('the steps follow the clock: 10 virtual seconds are 500 steps of 20 ms at 60 Hz and at 144 Hz, Max the step cap; 2 s of the real clock (receipt)', async (env) => {
  const page = await bootGame(env);
  // The real clock first: the count is asserted only when no time was lost (spec 06 Acceptance 1).
  const real = await realRun(page, 2000);
  const want = Math.floor(real.wall / STEP_MS);
  assert.ok(real.clock === real.steps * STEP_MS, 'real clock: every step is 20 ms');
  if (real.lost) {
    console.log(`  receipt: real clock ${real.wall.toFixed(0)} ms, ${real.steps} steps (want ${want}): time was lost (dropped or clamped), the count is not asserted`);
  } else {
    assert.ok(Math.abs(real.steps - want) <= 2, 'real clock: the steps the wall time is worth, within two');
    console.log(`  receipt: real clock ${real.wall.toFixed(0)} ms, ${real.steps} steps (want ${want})`);
  }
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2); // the first virtual frame's time is about one interval after the last real one
  const receipts = [];
  for (const hz of [60, 144]) {
    await h.displayRate(page, hz);
    await h.runFrames(page, 1);
    const before = await glue(page);
    const { called } = await h.runFrames(page, 10 * hz);
    const after = await glue(page);
    const steps = after.steps - before.steps;
    assert.ok(called === 10 * hz, `${hz} Hz: the loop ran in every virtual frame`);
    assert.ok(Math.abs(steps - 500) <= 1, `${hz} Hz: 10 virtual seconds are 500 steps, within one`);
    assert.ok(after.clock - before.clock === steps * STEP_MS, `${hz} Hz: the engine clock is the steps x 20 ms`);
    assert.ok(after.lost === before.lost, `${hz} Hz: nothing lost`);
    receipts.push(`${hz} Hz ${steps} steps`);
  }
  // Max: on a virtual clock a step takes no time, so every frame runs the step cap (64).
  await h.displayRate(page, 60);
  for (let i = 0; i < 7; i += 1) await page.keyboard.press('Equal');
  assert.equal((await speedShown(page)).speed, 'max', 'seven presets up: Max');
  await h.runFrames(page, 1);
  const before = await glue(page);
  await h.runFrames(page, 60);
  const after = await glue(page);
  assert.equal(after.speed, 0, 'the game runs at Max');
  assert.ok(after.steps - before.steps === 64 * 60, 'Max: the step cap in every frame');
  const shown = await speedShown(page);
  assert.equal(shown.label, 'Max 76.8×', 'Max shows the speed reached (64 steps of 20 ms a 60 Hz frame)');
  assert.ok(shown.badge && shown.text === shown.label, 'the badge shows it');
  console.log(`  receipt: 10 virtual seconds: ${receipts.join(', ')}; Max at 60 Hz: ${after.steps - before.steps} steps a virtual second, "${shown.label}"`);
}, { needs: h.haveHfes });

test('at 4x the engine clock runs about four times as fast per wall second as at 1x (receipt)', async (env) => {
  const page = await bootGame(env);
  const one = await realRun(page, 2000);
  for (let i = 0; i < 4; i += 1) await page.keyboard.press('Equal');
  assert.equal((await speedShown(page)).speed, '4', 'four presets up');
  await engineSpeed(page, 4);
  const four = await realRun(page, 2000);
  const ratio = (four.clock / four.wall) / (one.clock / one.wall);
  const line = `1x ${one.clock} engine ms in ${one.wall.toFixed(0)} ms, 4x ${four.clock} in ${four.wall.toFixed(0)} ms: ratio ${ratio.toFixed(2)}`;
  if (one.lost || four.lost) {
    console.log(`  receipt: ${line}; time was lost (dropped or clamped), the ratio is not asserted`);
  } else {
    assert.ok(ratio > 3.6 && ratio < 4.4, 'the 4x clock is four times the 1x one, within 10%');
    console.log(`  receipt: ${line}`);
  }
}, { needs: h.haveHfes });

// A keydown on the canvas as another layout sends it (synthetic: the character on another
// key); whether the page took it (prevented its default).
const layoutKey = (page, key, code, shiftKey = false) => page.evaluate(([k, c, shift]) =>
  !document.getElementById('screen').dispatchEvent(new KeyboardEvent('keydown', {
    key: k, code: c, shiftKey: shift, bubbles: true, cancelable: true,
  })), [key, code, shiftKey]);

test('the speed keys at the prompt: + and = faster, - slower, the keypad always, another layout\'s too; the badge shows it; the choice survives a reload, an invalid one is 1x', async (env) => {
  const page = await bootGame(env);
  await h.waitPrompt(page);
  let g = await glue(page);
  assert.equal(g.owned, 0, 'at the prompt the game keeps none of - + =');
  let shown = await speedShown(page);
  assert.ok(shown.speed === '1' && !shown.badge, 'the default is 1x, no badge');
  const sent = g.sent;
  for (const [key, want] of [['+', '1.5'], ['Equal', '2'], ['Minus', '1.5'], ['NumpadAdd', '2'], ['NumpadSubtract', '1.5']]) {
    await page.keyboard.press(key);
    shown = await speedShown(page);
    assert.equal(shown.speed, want, `${key}: the speed`);
    assert.ok(shown.badge && shown.text === `${want}×` && shown.label === shown.text, `${key}: the badge and the label show it`);
    const [, prevented] = await page.evaluate(() => window.__keys.at(-1));
    assert.ok(prevented, `${key}: taken by the page`);
  }
  for (const [key, code, shift, want] of [['+', 'BracketRight', false, '2'], ['-', 'Slash', false, '1.5'], ['=', 'Digit0', true, '2']]) {
    assert.ok(await layoutKey(page, key, code, shift), `${key} on ${code}: taken by the page`);
    assert.equal((await speedShown(page)).speed, want, `${key} on ${code}: the speed`);
  }
  await engineSpeed(page, 2);
  g = await glue(page);
  assert.equal(g.sent, sent, 'no speed key reached the game');
  for (let i = 0; i < 2; i += 1) await page.keyboard.press('Minus');
  shown = await speedShown(page);
  assert.ok(shown.speed === '1' && shown.badge, 'back at 1x the badge stays a moment');
  await page.waitForFunction(() => document.getElementById('speed-badge').hidden, null, { timeout: 5000 });
  for (let i = 0; i < 3; i += 1) await page.keyboard.press('Equal');
  await engineSpeed(page, 3);
  await page.reload();
  await started(page, FILES);
  shown = await speedShown(page);
  assert.ok(shown.speed === '3' && shown.badge, 'the choice survives a reload, the badge up while not 1x');
  await engineSpeed(page, 3);
  for (const [stored, want, speed] of [['max', 'max', 0], ['fast', '1', 1], ['2.50', '1', 1]]) {
    await page.evaluate((v) => localStorage.setItem('u6.speed', v), stored);
    await page.reload();
    await started(page, FILES);
    assert.equal((await speedShown(page)).speed, want, `stored ${stored}: the speed`);
    await engineSpeed(page, speed);
  }
}, { needs: h.haveHfes });

test('in the stats panel - and + are the game\'s and the speed stays; the keypad and = still change it', async (env) => {
  const page = await bootGame(env);
  await h.waitPrompt(page);
  // The roster's first member, right of the figures: their stats panel (ui click.rs roster_click).
  await h.click(page, 240, 36, 0);
  await page.waitForFunction(() => globalThis.__owned === 3, null, { timeout: 10_000 });
  for (const [key, char] of [['+', '+'], ['Minus', '-']]) {
    const before = (await glue(page)).sent;
    await h.press(page, key, 0);
    const g = await glue(page);
    assert.ok(g.sent === before + 1 && g.lastSent === char, `${char}: the game got it`);
    assert.equal((await speedShown(page)).speed, '1', `${char}: the speed stays`);
    assert.equal(g.owned, 3, `${char}: the panel is still open`);
  }
  const sent = (await glue(page)).sent;
  for (const [key, want] of [['NumpadAdd', '1.5'], ['NumpadSubtract', '1'], ['Equal', '1.5']]) {
    await page.keyboard.press(key);
    assert.equal((await speedShown(page)).speed, want, `${key} in the panel: the speed`);
  }
  await engineSpeed(page, 1.5);
  assert.equal((await glue(page)).sent, sent, 'the keypad and = did not reach the game');
}, { needs: h.haveHfes });

// On the virtual display: holds the left button on game pixel `cell` for `frames` frames, then
// releases and runs frames until the prompt is back. Returns the hold the game took (its
// release's clock less its press's) and the cells the leader moved, during the hold and after.
async function virtualHold(page, cell, frames) {
  await page.mouse.move(...(await h.screenPoint(page, ...cell)));
  await page.evaluate(() => { globalThis.__buttons.length = 0; });
  await page.mouse.down();
  const held = await h.runFrames(page, frames);
  await page.mouse.up();
  const after = await h.runFramesToPrompt(page, 60 * 60);
  const seen = [...held.seen, ...after.seen.slice(1)];
  const buttons = await page.evaluate(() => [...globalThis.__buttons]);
  const press = buttons.findIndex(([b]) => b === 1);
  const release = buttons.findIndex(([b], i) => i > press && b === 0);
  assert.ok(press >= 0 && release > press, 'the game took the press and its release');
  return { hold: buttons[release][1] - buttons[press][1], cells: seen.length - 1 };
}

// A virtual hold that walks: the start fight can block a direction (as `freshHeldWalk`), so the
// next of the four is tried (a blocked step costs no movement points, the prompt is back at once).
async function virtualHeldWalk(page, frames) {
  for (const [i, cell] of HOLD_CELLS.entries()) {
    const walk = await virtualHold(page, cell, frames);
    if (walk.cells > 0) return { ...walk, direction: i };
  }
  assert.fail('a held press walks a cell in one of the four directions');
}

test('a press held 150 ms at 8x walks as at 1x: the game takes it at 1x (virtual 60 Hz display, receipt)', async (env) => {
  const page = await bootGame(env);
  await h.waitPrompt(page);
  await h.virtualDisplay(page, 60);
  await h.runFramesToPrompt(page, 600);
  const one = await virtualHeldWalk(page, 9); // 9 frames at 60 Hz: 150 ms
  for (let i = 0; i < 6; i += 1) await page.keyboard.press('Equal');
  assert.equal((await speedShown(page)).speed, '8', 'six presets up: 8x');
  const eight = await virtualHeldWalk(page, 9);
  console.log(`  receipt: 150 ms held: 1x ${one.hold} ms on the engine clock, ${one.cells} cell (direction ${one.direction}); 8x ${eight.hold} ms, ${eight.cells} cell (direction ${eight.direction})`);
  assert.equal((await glue(page)).speed, 8, 'the game ran at 8x');
  assert.ok(one.cells === 1 && eight.cells === 1, 'one cell at either speed');
  assert.ok(eight.hold <= 160 && Math.abs(eight.hold - one.hold) <= STEP_MS, 'the 8x hold is the 1x one (150 ms of frames), within a step');
}, { needs: h.haveHfes });

test('"behind" shows while the frame budget drops steps (a slow host at 8x), not after a stall past the cap; it clears at 1x (virtual 60 Hz display)', async (env) => {
  const page = await bootGame(env);
  const behind = () => page.evaluate(() => ({
    data: document.getElementById('screen').dataset.behind,
    shown: !document.getElementById('behind').hidden,
  }));
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  assert.deepEqual(await behind(), { data: '0', shown: false }, 'not behind at 1x');
  // A stall: the frame after it is cut at the cap (clamped), which is not behind.
  await h.displayStall(page, 5000);
  await h.runFrames(page, 1);
  const stalled = await page.evaluate(() => [globalThis.__clamped, globalThis.__dropped]);
  assert.ok(stalled[0] > 0 && stalled[1] === 0, 'the stall was clamped, nothing dropped');
  assert.deepEqual(await behind(), { data: '0', shown: false }, 'a stall is not behind');
  // 8x on a host whose clock reads cost 3 ms: the 8 ms budget of a 60 Hz frame holds 3 steps,
  // 8x wants 6.67.
  for (let i = 0; i < 6; i += 1) await page.keyboard.press('Equal');
  await h.displayCost(page, 3);
  const g = await glue(page);
  await h.runFrames(page, 60);
  const after = await glue(page);
  assert.ok(after.steps - g.steps === 3 * 60, 'the budget held three steps a frame');
  assert.ok(await page.evaluate(() => globalThis.__dropped) > 0, 'steps dropped');
  assert.deepEqual(await behind(), { data: '1', shown: true }, 'behind');
  // Back at 1x the budget holds the pace again; "behind" goes a second after the last drop.
  for (let i = 0; i < 6; i += 1) await page.keyboard.press('Minus');
  const dropped = await page.evaluate(() => globalThis.__dropped);
  await h.runFrames(page, 61);
  assert.ok(await page.evaluate(() => globalThis.__dropped) === dropped, 'nothing dropped at 1x');
  assert.deepEqual(await behind(), { data: '0', shown: false }, 'not behind after a second at 1x');
}, { needs: h.haveHfes });

test('the Speed buttons change the speed by click; above 4x the music fades out and comes back; muted it stays silent at any speed', async (env) => {
  const page = await bootGame(env);
  const gain = (want) => page.waitForFunction((g) => document.body.dataset.gain === g, want, { timeout: 10_000 });
  await page.click('#faster');
  assert.equal((await speedShown(page)).speed, '1.5', 'the + button');
  await engineSpeed(page, 1.5);
  await page.click('#slower');
  assert.equal((await speedShown(page)).speed, '1', 'the - button');
  await engineSpeed(page, 1);
  await page.click('#mute'); // the gesture that starts the sound, muted
  await page.waitForFunction(() => document.body.dataset.audio === 'running', null, { timeout: 15_000 });
  await gain('0.00');
  await page.click('#mute');
  await gain('1.00');
  for (let i = 0; i < 5; i += 1) await page.click('#faster');
  assert.equal((await speedShown(page)).speed, '6', 'up to 6x');
  await gain('0.00');
  await page.click('#slower');
  await page.click('#slower');
  assert.equal((await speedShown(page)).speed, '3', 'down to 3x');
  await gain('1.00');
  await page.click('#mute');
  await gain('0.00');
  for (let i = 0; i < 3; i += 1) await page.click('#faster');
  assert.equal((await speedShown(page)).speed, '8', 'muted, up to 8x');
  await page.click('#slower');
  await page.click('#slower');
  assert.equal((await speedShown(page)).speed, '4', 'muted, back to 4x');
  await h.waitFrames(page, 20);
  assert.equal(await page.evaluate(() => document.body.dataset.gain), '0.00', 'muted at 4x: silent');
  await page.click('#mute');
  await gain('1.00');
}, { needs: h.haveHfes });

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
      // A fixed head (an assertion's own label, else the error's kind) and the places in these
      // files: no message text (h.failureLines).
      console.log(`not ok - ${t.name}\n  ${h.failureLines(err).join('\n  ')}`);
    } finally {
      for (const context of env.browser.contexts()) await context.close();
    }
  }
} finally {
  await env.close();
}
console.log(`# web tests: ${ran} ran, ${ran - failed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);

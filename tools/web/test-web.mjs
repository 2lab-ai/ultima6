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
// choice, a held press at 1x whatever the speed, the music above 4x, a frame converted and
// drawn only when it changed). Most tests start `game` at once (harness `gameFirst`); the
// launcher tests run the page as it is.
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
  await page.evaluate(() => { window.__bigName = 'big.adf'; });
  await h.dropFiles(page, [], [{ name: 'big.adf', text: 'PK' }, MARKER]);
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
// `stepSideways` tries the other way). Which direction walked depends on the boot's game state,
// so the receipt does not name it.
async function freshHeldWalk(page, ms) {
  for (const cell of HOLD_CELLS) {
    const walk = await heldWalk(page, ms, cell);
    if (walk.steps > 0) return walk;
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
  const line = (ms, w) => `${ms} ms held ${w.hold} ms on the engine clock, ${w.steps} cell${w.steps === 1 ? '' : 's'}`;
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

// --- a large zip is read in slices ----------------------------------------------------------
// A dropped zip over the slicing threshold (u6-web `zip_sliced`) is not copied into wasm whole:
// the page reads its first bytes, its directory and its disk-image entries with Blob.slice. What
// the page read from a dropped zip is counted in the page itself (`countZipReads`); for the
// user's zip the counts are compared with its size and never printed (they are lengths of the
// user's data), for a zip a test made they are printed. A zip larger than the page could hold is
// a virtual file (`virtualFiles`): only the parts a test made exist, and a read of any other
// byte is a forbidden request that fails.

// The bound on the wasm memory after a start from the user's zip (our measurement of the
// engine's memory, not a value of the game's files): the base copied the whole zip in.
const ZIP_START_MEMORY_MIB = 48;
const MiB = (n) => Math.round((n / 2 ** 20) * 10) / 10;

// Page init script, before `countZipReads`: dropped files that claim another size
// (`window.__claims`: name -> bytes) and virtual zips (`window.__virtual`: name -> { size,
// pieces: [[offset, bytes]] }). A slice of a virtual zip is made of its pieces and zeros in its
// last bytes (the tail the first read of a sliced zip takes, u6-intake `tail_start`); a slice
// that touches any other byte is a forbidden request: it is recorded in `window.__forbidden` and
// throws, so the import fails.
function virtualFiles() {
  window.__claims = {};
  window.__virtual = {};
  window.__forbidden = [];
  const TAIL = 65_558 + 20 + 56;
  const virtual = (b) => (b instanceof File ? window.__virtual[b.name] : undefined);
  const size = Object.getOwnPropertyDescriptor(Blob.prototype, 'size').get;
  Object.defineProperty(Blob.prototype, 'size', {
    configurable: true,
    get() {
      const claim = (b) => (b instanceof File ? window.__claims[b.name] : undefined);
      return virtual(this)?.size ?? claim(this) ?? size.call(this);
    },
  });
  const slice = Blob.prototype.slice;
  Blob.prototype.slice = function sliceVirtual(start = 0, end = undefined) {
    const v = virtual(this);
    if (!v) return slice.call(this, start, end);
    const stop = end ?? v.size;
    const spans = [...v.pieces.map(([at, b]) => [at, at + b.length]), [Math.max(0, v.size - TAIL), v.size]];
    for (let at = start; at < stop;) {
      const span = spans.find(([a, b]) => a <= at && at < b);
      if (!span) {
        window.__forbidden.push([start, stop]);
        throw new Error('a forbidden request');
      }
      at = span[1];
    }
    const out = new Uint8Array(stop - start);
    for (const [at, b] of v.pieces) {
      const from = Math.max(start, at);
      const to = Math.min(stop, at + b.length);
      if (from < to) out.set(b.subarray(from - at, to - at), from - start);
    }
    return new Blob([out]);
  };
}

// Page init script: the reads of dropped zips (File objects named *.zip), whole and of the slices
// taken from them, and the zip's size.
function countZipReads() {
  const parts = new WeakSet();
  const count = { whole: 0, sliced: 0, slices: 0, size: 0 };
  window.__zipReads = count;
  const isZip = (b) => b instanceof File && /\.zip$/i.test(b.name);
  const slice = Blob.prototype.slice;
  Blob.prototype.slice = function sliceCounted(...args) {
    const part = slice.apply(this, args);
    if (isZip(this)) {
      parts.add(part);
      count.size = this.size;
    }
    return part;
  };
  const read = Blob.prototype.arrayBuffer;
  Blob.prototype.arrayBuffer = function arrayBufferCounted() {
    if (parts.has(this)) {
      count.sliced += this.size;
      count.slices += 1;
    } else if (isZip(this)) {
      count.whole += this.size;
      count.size = this.size;
    }
    return read.call(this);
  };
}

const zipReads = (page) => page.evaluate(() => ({ ...window.__zipReads, forbidden: window.__forbidden.length }));

// Page init script: `window.__makeZip(entries)`, a zip of synthetic bytes. Each entry is
// { name, length, fill, method, csize, full, localName }: `length` bytes of `fill` as its data,
// `method` (default stored), the packed and unpacked sizes its records declare (default
// `length`), and `localName` (same length) in its local header instead of `name` (a local header
// that disagrees with its central record). CRCs are 0 (the intake does not check them). Returns
// { head, dir, end(at) }: the local headers and data, the central directory, and the end record
// for a directory at `at`.
function zipKit() {
  const u16 = (v) => [v & 255, (v >>> 8) & 255];
  const u32 = (v) => [v & 255, (v >>> 8) & 255, (v >>> 16) & 255, (v >>> 24) & 255];
  window.__makeZip = (entries) => {
    const head = [];
    const dir = [];
    let offset = 0;
    for (const e of entries) {
      const name = [...new TextEncoder().encode(e.name)];
      const local = e.localName ? [...new TextEncoder().encode(e.localName)] : name;
      const method = e.method ?? 0;
      const sizes = [...u32(0), ...u32(e.csize ?? e.length), ...u32(e.full ?? e.length)];
      const header = [...u32(0x04034b50), ...u16(20), ...u16(0), ...u16(method), ...u32(0), ...sizes,
        ...u16(local.length), ...u16(0), ...local];
      head.push(new Uint8Array(header), new Uint8Array(e.length).fill(e.fill ?? 0));
      dir.push(...u32(0x02014b50), ...u16(20), ...u16(20), ...u16(0), ...u16(method), ...u32(0),
        ...sizes, ...u16(name.length), ...u16(0), ...u16(0), ...u16(0), ...u16(0), ...u32(0),
        ...u32(offset), ...name);
      offset += header.length + e.length;
    }
    const joined = new Uint8Array(offset);
    let at = 0;
    for (const part of head) {
      joined.set(part, at);
      at += part.length;
    }
    const end = (cd) => new Uint8Array([...u32(0x06054b50), ...u16(0), ...u16(0), ...u16(entries.length),
      ...u16(entries.length), ...u32(dir.length), ...u32(cd), ...u16(0)]);
    return { head: joined, dir: new Uint8Array(dir), end };
  };
}

// In the page: drops the files `files` on the drop zone, in this order. Each is { zip, name }
// (a zip of the entries `zip`, see `zipKit`), { virtual, size, name } (the zip of the entries
// `virtual` as a file of `size` bytes: its entries at the start, with the zeros an entry's range
// may run on when no record follows it, the longest local extra field, its directory and end
// record at the end), or { claims, name } (a file of a few bytes that claims `claims` bytes).
function dropMade(page, files) {
  return page.evaluate((files) => {
    const dt = new DataTransfer();
    for (const f of files) {
      if (f.zip) {
        const z = window.__makeZip(f.zip);
        dt.items.add(new File([z.head, z.dir, z.end(z.head.length)], f.name));
      } else if (f.virtual) {
        const z = window.__makeZip(f.virtual);
        const at = f.size - 22 - z.dir.length;
        const head = new Uint8Array(z.head.length + 0xffff);
        head.set(z.head);
        const tail = new Uint8Array([...z.dir, ...z.end(at)]);
        window.__virtual[f.name] = { size: f.size, pieces: [[0, head], [at, tail]] };
        dt.items.add(new File(['virtual'], f.name));
      } else {
        window.__claims[f.name] = f.claims;
        dt.items.add(new File(['claims more'], f.name));
      }
    }
    document.getElementById('drop').dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }));
  }, files);
}

async function zipPage(env) {
  const { context, page } = await h.openPage(env);
  for (const script of [virtualFiles, countZipReads, zipKit]) await context.addInitScript(script);
  await h.load(page, env);
  return page;
}

const FILLER = { name: 'scans/filler.bin', length: 17 * 2 ** 20, fill: 1 }; // over the threshold
const DISK = { name: 'disks/Made 1.adf', length: 4096, fill: 2 };
const NOTHING_KEPT = 'nothing kept';

test('a large zip (synthetic) is read in slices: its directory and its disk image only; one whose local header disagrees is refused and nothing is kept', async (env) => {
  const page = await zipPage(env);
  await dropMade(page, [{ name: 'made.zip', zip: [FILLER, DISK] }]);
  await h.waitSettled(page);
  let s = await h.state(page);
  let r = await zipReads(page);
  console.log(`  receipt: the made zip (${r.size} bytes): ${r.whole} bytes read whole, ${r.sliced} in ${r.slices} slices`);
  assert.ok(r.whole === 0, 'the zip is not read whole');
  assert.ok(r.slices === 3 && r.sliced > 4096 && r.sliced < 2 ** 20, 'its first bytes, its directory and the disk image only');
  assert.match(s.status, /^Could not start the game:\nmade\.zip: zip, 1 disk image\(s\): Made 1\.adf\n/);
  assert.equal(s.state, 'idle');
  await dropMade(page, [{ name: 'bent.zip', zip: [FILLER, { ...DISK, localName: 'disks/Made 2.adf' }] }]);
  await h.waitSettled(page);
  s = await h.state(page);
  r = await zipReads(page);
  assert.ok(r.whole === 0, 'the bent zip is not read whole either');
  assert.equal(s.status, 'Loading the files failed: bent.zip: zip unreadable: invalid zip data');
  assert.equal(s.state, 'idle');
  assert.equal(await h.keptPaths(page), null, NOTHING_KEPT);
});

test('a zip of 600 MiB with a small disk image is taken (only what is read counts); one whose entry would take the drop past 512 MiB is refused before that entry is read', async (env) => {
  const page = await zipPage(env);
  const size = 600 * 2 ** 20;
  await dropMade(page, [{ name: 'big.zip', virtual: [DISK], size }]);
  await h.waitSettled(page);
  let s = await h.state(page);
  let r = await zipReads(page);
  console.log(`  receipt: the virtual zip (${r.size} bytes): ${r.whole} bytes read whole, ${r.sliced} in ${r.slices} slices, ${r.forbidden} forbidden requests`);
  assert.ok(r.size === size && r.whole === 0 && r.forbidden === 0, 'read in slices, nothing else asked for');
  assert.match(s.status, /^Could not start the game:\nbig\.zip: zip, 1 disk image\(s\): Made 1\.adf\n/);
  // Deflated, declaring 1 MiB unpacked: its 550 MiB of data would pass the input limit.
  const packed = { name: 'disks/Made 1.adf', length: 2, method: 8, csize: 550 * 2 ** 20, full: 2 ** 20 };
  await dropMade(page, [{ name: 'packed.zip', virtual: [packed], size }]);
  await h.waitSettled(page);
  s = await h.state(page);
  r = await zipReads(page);
  console.log(`  receipt: the packed zip: ${r.slices - 3} slices read, ${r.forbidden} forbidden requests`);
  assert.ok(r.slices === 3 + 2 && r.forbidden === 0, 'its first bytes and its tail, not the entry');
  assert.match(s.status, /^Loading the files failed: The dropped files are \d+ MiB together; this page takes at most 512 MiB \(a zip over 16 MiB counts only what is read of it/);
  assert.equal(s.state, 'idle');
  assert.equal(await h.keptPaths(page), null, NOTHING_KEPT);
});

test('a zip read in slices and a file read whole that fit alone but not together: the file is refused before it is read', async (env) => {
  const page = await zipPage(env);
  // The file alone fits under 512 MiB; with what was read of the zip it does not.
  const claims = 512 * 2 ** 20 - 1024;
  await dropMade(page, [{ name: 'made.zip', zip: [FILLER, DISK] }, { name: 'big.bin', claims }]);
  await h.waitSettled(page);
  const s = await h.state(page);
  const r = await zipReads(page);
  assert.ok(r.whole === 0 && r.slices >= 2, 'the zip read in slices first');
  assert.match(s.status, /^The dropped files are 512 MiB together; this page takes at most 512 MiB/);
  assert.ok(!(await h.log(page)).includes('read:start:big.bin'), 'the file is not read');
  assert.equal(s.state, 'idle');
  assert.equal(await h.keptPaths(page), null, NOTHING_KEPT);
});

test('the user\'s zip is read in slices: the game starts, the wasm memory after the start stays under the bound, and a reload starts from the kept files (receipt)', async (env) => {
  const page = await zipPage(env);
  const idle = await page.evaluate(() => __u6mem());
  await h.dropFiles(page, [h.ZIP]);
  await started(page, FILES);
  await h.waitPrompt(page);
  const memory = await page.evaluate(() => __u6mem());
  const r = await zipReads(page);
  console.log(`  receipt: wasm memory ${MiB(idle)} MiB before the drop, ${MiB(memory)} MiB after the start (bound ${ZIP_START_MEMORY_MIB} MiB)`);
  assert.ok(r.whole === 0, 'the zip is not read whole');
  assert.ok(r.slices >= 2 && r.sliced > 0 && r.sliced < r.size / 10, 'at most a tenth of the zip is read');
  assert.ok(memory < ZIP_START_MEMORY_MIB * 2 ** 20, `the wasm memory after the start is under ${ZIP_START_MEMORY_MIB} MiB`);
  await kept(page, FILES, 'the game files are kept');
  await page.reload();
  const s = await started(page, FILES);
  assert.match(s.status, /Started from the files kept in this browser/);
  await h.waitPrompt(page);
  const reloaded = await page.evaluate(() => __u6mem());
  console.log(`  receipt: after the reload the game started from the kept files; wasm memory ${MiB(reloaded)} MiB`);
}, { needs: h.haveZip });

// --- the fixed step and the speed (spec 06) -------------------------------------------------

const STEP_MS = 20; // one PAL frame: every step the page runs (u6-core pacer::STEP_MS)

// What the page shows of the speed: data-speed (1, 1.5, .. 8, max), the badge's state and text,
// the Speed group's label as shown and its announced part (the live region: the speed's name).
const speedShown = (page) => page.evaluate(() => ({
  speed: document.getElementById('screen').dataset.speed,
  badge: !document.getElementById('speed-badge').hidden,
  text: document.getElementById('speed-badge').textContent,
  label: document.getElementById('speed-shown').textContent,
  announced: document.getElementById('speed-label').textContent,
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

// A gated page (gameFirst) with the user's disks started; the canvas focused. `init` (with its
// `arg`) runs in the page before its scripts.
async function bootGame(env, init, arg) {
  const { context, page } = await h.openPage(env);
  if (init) await context.addInitScript(init, arg);
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
  assert.equal(shown.announced, 'Max', 'the live label announces Max, not each measure of it');
  const a11y = await page.evaluate(() => [document.getElementById('speed-badge').getAttribute('aria-hidden'),
    document.getElementById('speed-label').getAttribute('aria-live'), document.getElementById('speed-reached').getAttribute('aria-hidden')]);
  assert.deepEqual(a11y, ['true', 'polite', 'true'], 'the badge and the reached speed are hidden from assistive technology, the label is live');
  console.log(`  receipt: 10 virtual seconds: ${receipts.join(', ')}; Max at 60 Hz: ${after.steps - before.steps} steps a virtual second, "${shown.label}"`);
}, { needs: h.haveHfes });

// Presses = or - on the canvas until the page shows speed `name` (1, 1.5, .. 8, max).
async function speedTo(page, name) {
  const names = ['1', '1.5', '2', '3', '4', '6', '8', 'max'];
  for (let i = 0; i < names.length; i += 1) {
    const at = names.indexOf((await speedShown(page)).speed);
    if (at === names.indexOf(name)) return;
    await page.keyboard.press(at < names.indexOf(name) ? 'Equal' : 'Minus');
  }
  assert.fail(`the speed reaches ${name}`);
}

test('at 2x, 4x and 8x 10 virtual seconds are exactly that many times 500 steps at 60 Hz and 144 Hz; on the real clock 4x runs about four times 1x (receipt)', async (env) => {
  const page = await bootGame(env);
  const one = await realRun(page, 2000);
  await speedTo(page, '4');
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
  // The same rule without the host: on a virtual display the budget never runs out.
  await h.virtualDisplay(page, 60);
  const counts = [];
  for (const hz of [60, 144]) {
    await h.displayRate(page, hz);
    for (const speed of [2, 4, 8]) {
      await speedTo(page, String(speed));
      await h.runFrames(page, 1);
      const before = await glue(page);
      await h.runFrames(page, 10 * hz);
      const after = await glue(page);
      const steps = after.steps - before.steps;
      assert.equal(after.speed, speed, `${hz} Hz: the game runs at ${speed}x`);
      assert.ok(Math.abs(steps - speed * 500) <= 1, `${hz} Hz at ${speed}x: 10 virtual seconds are ${speed * 500} steps, within one`);
      assert.ok(after.lost === before.lost, `${hz} Hz at ${speed}x: nothing lost`);
      counts.push(`${hz} Hz ${speed}x ${steps}`);
    }
  }
  console.log(`  receipt: 10 virtual seconds: ${counts.join(', ')} steps`);
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
    assert.equal(shown.announced, `${want}×`, `${key}: the live label has it`);
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
  for (const cell of HOLD_CELLS) {
    const walk = await virtualHold(page, cell, frames);
    if (walk.cells > 0) return walk;
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
  const line = (w) => `${w.hold} ms on the engine clock, ${w.cells} cell`;
  console.log(`  receipt: 150 ms held: 1x ${line(one)}; 8x ${line(eight)}`);
  assert.equal((await glue(page)).speed, 8, 'the game ran at 8x');
  // 150 ms of frames is 7 or 8 steps between the press and the release (the step's remainder
  // at the press), so the game holds 140..160 ms at either speed.
  for (const [w, at] of [[one, '1x'], [eight, '8x']]) {
    assert.ok(w.hold >= 150 - STEP_MS / 2 && w.hold <= 150 + STEP_MS / 2, `${at}: the game held the button 150 ms, within half a step either way`);
  }
  assert.ok(one.cells === 1 && eight.cells === 1, 'one cell at either speed');
  assert.ok(Math.abs(eight.hold - one.hold) <= STEP_MS, 'the 8x hold is the 1x one, within a step');
}, { needs: h.haveHfes });

// On the virtual display: runs `n` frames and returns, per frame, the steps it ran, the
// conversions of the game's frame and the canvas draws in it.
const framesSeen = (page, n) => page.evaluate((count) => {
  const v = window.__display;
  const out = [];
  for (let i = 0; i < count; i += 1) {
    const a = [globalThis.__ticks, globalThis.__converted, window.__draws];
    v.run();
    out.push([globalThis.__ticks - a[0], globalThis.__converted - a[1], window.__draws - a[2]]);
  }
  return out;
}, n);

// Counts the canvas draws (putImageData) in `window.__draws`.
function countDraws() {
  const put = CanvasRenderingContext2D.prototype.putImageData;
  window.__draws = 0;
  CanvasRenderingContext2D.prototype.putImageData = function (...args) {
    window.__draws += 1;
    return put.apply(this, args);
  };
}

test('a display frame converts the game\'s frame at most once however many steps ran, and draws the canvas only for a conversion (virtual 60 Hz display, spec 06 O1)', async (env) => {
  const page = await bootGame(env, countDraws);
  await h.waitPrompt(page);
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  const receipts = [];
  for (const speed of ['1', '8']) {
    await speedTo(page, speed);
    await h.runFrames(page, 1);
    const seen = await framesSeen(page, 300);
    const sum = (k) => seen.reduce((t, f) => t + f[k], 0);
    seen.forEach(([steps, converted, draws], i) => {
      assert.ok(converted <= 1 && draws <= 1, `${speed}x frame ${i}: at most one conversion and one draw`);
      assert.ok(draws === converted, `${speed}x frame ${i}: the canvas drawn for a conversion, never without one`);
      assert.ok(steps > 0 || converted === 0, `${speed}x frame ${i}: no step, no conversion`);
    });
    assert.ok(sum(1) > 0, `${speed}x: the game's frames reach the canvas`);
    if (speed === '8') assert.ok(sum(0) > 4 * sum(1), '8x: several steps a frame, one conversion');
    receipts.push(`${speed}x ${sum(0)} steps, ${sum(1)} conversions, ${sum(2)} draws, at most ${Math.max(...seen.map((f) => f[0]))} steps and ${Math.max(...seen.map((f) => f[1]))} conversion a frame`);
  }
  console.log(`  receipt: 300 virtual frames: ${receipts.join('; ')}`);
}, { needs: h.haveHfes });

// Whether the canvas shows the frame of the game that presented last (its HD frame with HD on,
// else its RGBA frame), compared in the page by an FNV-1a hash of each (the frame is the user's
// data: only the equality leaves the page).
const canvasShowsGame = (page) => page.evaluate(() => {
  const hash = (d) => {
    let x = 0x811c9dc5;
    for (let i = 0; i < d.length; i += 1) x = Math.imul(x ^ d[i], 0x01000193) >>> 0;
    return x;
  };
  const c = document.getElementById('screen');
  const g = globalThis.__shownGame;
  return hash(c.getContext('2d').getImageData(0, 0, c.width, c.height).data) === hash(g.hd_on() ? g.hd_frame_rgba() : g.frame_rgba());
});

test('a new game put on screen repaints the canvas with its own frame: an import over the running game (spec 06 O1)', async (env) => {
  const page = await bootGame(env, countDraws);
  await h.waitPrompt(page);
  assert.ok(await canvasShowsGame(page), 'the first game\'s frame on the canvas');
  const before = await page.evaluate(() => {
    globalThis.__firstGame = globalThis.__shownGame;
    return window.__draws;
  });
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitFrames(page, 3);
  const after = await page.evaluate(() => ({
    draws: window.__draws, other: globalThis.__shownGame !== globalThis.__firstGame,
  }));
  assert.ok(after.other, 'the new game presents');
  assert.ok(after.draws > before, 'the canvas is drawn again');
  assert.ok(await canvasShowsGame(page), 'the new game\'s frame on the canvas');
  await h.waitPrompt(page);
  assert.ok(await canvasShowsGame(page), 'and at its prompt');
}, { needs: h.haveHfes });

test('"behind" shows while the frame budget drops steps (a slow host at 8x), not after a stall past the cap; it clears at 1x (virtual 60 Hz display)', async (env) => {
  const page = await bootGame(env);
  const behind = () => page.evaluate(() => ({
    data: document.getElementById('screen').dataset.behind,
    shown: !document.getElementById('behind').hidden,
  }));
  const lost = () => page.evaluate(() => ({ clamped: globalThis.__clamped, dropped: globalThis.__dropped }));
  await h.virtualDisplay(page, 60);
  // A second at 1x lets go of any "behind" from the real frames before (a loaded host).
  await h.runFrames(page, 61);
  assert.deepEqual(await behind(), { data: '0', shown: false }, 'not behind at 1x');
  // A stall: the frame after it is cut at the cap (clamped), which is not behind.
  const before = await lost();
  await h.displayStall(page, 5000);
  await h.runFrames(page, 1);
  const stalled = await lost();
  assert.ok(stalled.clamped > before.clamped, 'the stall was clamped');
  assert.ok(stalled.dropped === before.dropped, 'the stall dropped nothing');
  assert.deepEqual(await behind(), { data: '0', shown: false }, 'a stall is not behind');
  // 8x on a host whose clock reads cost 3 ms: the 8 ms budget of a 60 Hz frame holds 3 steps,
  // 8x wants 6.67.
  for (let i = 0; i < 6; i += 1) await page.keyboard.press('Equal');
  await h.displayCost(page, 3);
  const g = await glue(page);
  await h.runFrames(page, 60);
  const after = await glue(page);
  assert.ok(after.steps - g.steps === 3 * 60, 'the budget held three steps a frame');
  assert.ok((await lost()).dropped > stalled.dropped, 'steps dropped');
  assert.deepEqual(await behind(), { data: '1', shown: true }, 'behind');
  // Back at 1x the budget holds the pace again; "behind" goes a second after the last drop.
  for (let i = 0; i < 6; i += 1) await page.keyboard.press('Minus');
  const dropped = await page.evaluate(() => globalThis.__dropped);
  await h.runFrames(page, 61);
  assert.ok(await page.evaluate(() => globalThis.__dropped) === dropped, 'nothing dropped at 1x');
  assert.deepEqual(await behind(), { data: '0', shown: false }, 'not behind after a second at 1x');
}, { needs: h.haveHfes });

test('Max is bounded by the frame budget and by the step cap, and is never behind (virtual displays, clock reads that cost time)', async (env) => {
  const page = await bootGame(env);
  for (let i = 0; i < 7; i += 1) await page.keyboard.press('Equal');
  await engineSpeed(page, 0);
  await h.virtualDisplay(page, 60);
  const receipts = [];
  // Costs that do not divide the budget: a step's check is never `budget < budget`.
  for (const [hz, cost] of [[60, 1], [144, 1], [240, 1.1], [60, 0.1]]) {
    await h.displayCost(page, cost);
    await h.displayRate(page, hz);
    await h.runFrames(page, 120); // the page's measure of the frame interval settles on the display's
    const before = await glue(page);
    const dropped = await page.evaluate(() => globalThis.__dropped);
    await h.runFrames(page, hz);
    const after = await glue(page);
    // Spec 06 §Design 2: the budget is half the frame interval, at least 3 ms; a step starts
    // only while the frame is within it (the first always), at most 64 a frame.
    const budget = Math.max(0.5 * (1000 / hz), 3);
    const allowed = Math.min(Math.ceil(budget / cost), 64);
    const steps = after.steps - before.steps;
    assert.equal(after.speed, 0, `${hz} Hz: the game runs at Max`);
    assert.ok(steps === allowed * hz, `${hz} Hz, clock reads of ${cost} ms: the steps the budget and the cap allow in every frame`);
    assert.ok(await page.evaluate(() => globalThis.__dropped) === dropped, `${hz} Hz: Max drops nothing`);
    assert.equal((await speedShown(page)).speed, 'max');
    assert.ok(await page.evaluate(() => document.getElementById('screen').dataset.behind) === '0', `${hz} Hz: not behind`);
    receipts.push(`${hz} Hz at ${cost} ms a read: ${steps / hz} a frame`);
  }
  console.log(`  receipt: Max ${receipts.join('; ')}`);
}, { needs: h.haveHfes });

test('a change of speed in the page forgets the debt: steps owed at 8x on a slow host are not run at 1x (virtual 60 Hz display)', async (env) => {
  const page = await bootGame(env);
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  for (let i = 0; i < 6; i += 1) await page.keyboard.press('Equal');
  await h.displayCost(page, 3); // the 8 ms budget of a 60 Hz frame holds 3 steps; 8x wants 6.67
  await h.runFrames(page, 60);
  const behind = await page.evaluate(() => [globalThis.__speed, globalThis.__dropped, document.getElementById('screen').dataset.behind]);
  assert.ok(behind[0] === 8 && behind[1] > 0 && behind[2] === '1', 'behind at 8x: steps owed past the cap, dropped');
  for (let i = 0; i < 6; i += 1) await page.keyboard.press('Minus');
  assert.equal((await speedShown(page)).speed, '1');
  const before = await glue(page);
  const first = await h.runFrames(page, 1);
  const one = (await glue(page)).steps - before.steps;
  await h.runFrames(page, 59);
  const after = await glue(page);
  const steps = after.steps - before.steps;
  console.log(`  receipt: after 1 s behind at 8x, the first second at 1x: ${steps} steps (want 50), ${one} in its first frame`);
  assert.ok(first.called === 1 && one <= 1, 'the first frame at 1x runs no old debt');
  assert.ok(Math.abs(steps - 50) <= 1, 'the second at 1x is 50 steps, within one');
  assert.equal(after.speed, 1, 'the game runs at 1x');
}, { needs: h.haveHfes });

test('a game put on screen starts its clock at the boot: a new import during a running loop at 8x runs no catch-up steps in its first frame (virtual 60 Hz display)', async (env) => {
  const page = await bootGame(env);
  await h.virtualDisplay(page, 60);
  await speedTo(page, '8');
  await h.runFrames(page, 2);
  // Time passes with no frame before the new game is on screen, as the synchronous intake
  // takes it; then the same disks imported again replace the game.
  await h.displayStall(page, 1000);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  const before = await glue(page);
  await h.runFrames(page, 1);
  const after = await glue(page);
  const steps = after.steps - before.steps;
  console.log(`  receipt: the new game's first frame at 8x: ${steps} steps (its own 16.7 ms: 6)`);
  assert.equal(after.speed, 8, 'the new game runs at the chosen 8x');
  assert.ok(steps <= 7, 'the first frame runs its own interval at 8x (6.67 steps), no catch-up');
  // The new game's own counters (its Pacer starts at 0; `before` still holds the old game's).
  const lost = await page.evaluate(() => [globalThis.__clamped, globalThis.__dropped]);
  assert.deepEqual(lost, [0, 0], 'the new game clamped and dropped nothing');
}, { needs: h.haveHfes });

// The canvas pixels as one FNV-1a hash, computed in the page (the frame is the user's data:
// only whether two hashes are equal is used, never the hash).
const canvasHash = (page) => page.evaluate(() => {
  const c = document.getElementById('screen');
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  let x = 0x811c9dc5;
  for (let i = 0; i < d.length; i += 1) x = Math.imul(x ^ d[i], 0x01000193) >>> 0;
  return x;
});

test('the opening\'s frame at a step is the same on a 60 Hz and a 144 Hz display (virtual displays, one seed)', async (env) => {
  const STEPS = [50, 200, 500, 1000];
  const seedTime = Date.now(); // both boots seed the game from this clock (app.js clockSeed)
  const frames = {};
  for (const hz of [60, 144]) {
    const { context, page } = await h.openPage(env, { gameFirst: false });
    await context.addInitScript((t) => { Date.now = () => t; }, seedTime);
    await h.load(page, env);
    await h.virtualDisplay(page, hz, { loop: false }); // the loop starts on it, at the boot
    await page.setInputFiles('#picker', h.HFES);
    await started(page, FILES);
    frames[hz] = [];
    for (const n of STEPS) {
      const at = await h.runFramesToStep(page, n, 4 * n + 10);
      assert.ok(at === n, `${hz} Hz: one step at a time up to step ${n}`);
      assert.equal((await h.state(page)).program, 'u', `${hz} Hz: the opening runs at step ${n}`);
      frames[hz].push(await canvasHash(page));
    }
    await context.close();
  }
  assert.ok(new Set(frames[60]).size > 1, 'the opening moves between these steps');
  const same = STEPS.map((n, i) => `${n} ${frames[60][i] === frames[144][i] ? 'same' : 'differs'}`);
  console.log(`  receipt: the opening's frame at 60 Hz and 144 Hz, by step: ${same.join(', ')}`);
  STEPS.forEach((n, i) => assert.ok(frames[60][i] === frames[144][i], `step ${n}: the same frame at 60 Hz and 144 Hz`));
}, { needs: h.haveHfes });

// No game: the controls only. The widest speed text and "behind" shown.
test('layout: the Speed group sits on Mute\'s line at 1280 and 1024 px; at 375 px the page does not scroll sideways', async (env) => {
  const { page } = await h.openPage(env, { gated: false });
  await h.load(page, env);
  for (const width of [1280, 1024, 375]) {
    await page.setViewportSize({ width, height: 900 });
    const m = await page.evaluate(() => {
      document.getElementById('speed-label').textContent = 'Max';
      document.getElementById('speed-reached').textContent = ' 307.2×';
      document.getElementById('speed-badge').textContent = 'Max 307.2×';
      document.getElementById('behind').hidden = false;
      document.getElementById('speed-badge').hidden = false;
      const box = (id) => document.getElementById(id).getBoundingClientRect();
      const [mute, group, behind] = [box('mute'), box('speed-group'), box('behind')];
      return {
        sameLine: Math.abs((mute.top + mute.bottom) / 2 - (group.top + group.bottom) / 2) < 2 && group.left > mute.right,
        inside: behind.right <= document.getElementById('drop').getBoundingClientRect().right,
        sideways: document.documentElement.scrollWidth > document.documentElement.clientWidth,
      };
    });
    if (width >= 1024) assert.ok(m.sameLine, `${width} px: Speed on Mute's line`);
    assert.ok(m.inside, `${width} px: the group inside the drop zone`);
    assert.ok(!m.sideways, `${width} px: no sideways scroll`);
  }
});

// Before the page's scripts: every scheduling call on an AudioParam (the page's only one is its
// gain) logged as [method, ...arguments].
function logGainSchedule() {
  const log = [];
  window.__gainLog = log;
  for (const name of ['cancelScheduledValues', 'setTargetAtTime', 'setValueAtTime']) {
    const real = AudioParam.prototype[name];
    AudioParam.prototype[name] = function (...args) {
      log.push([name, ...args]);
      return real.apply(this, args);
    };
  }
}

// The fade's time constant (site/app.js FADE_S): ~95% of the way in 50 ms.
const FADE_TAU_S = 0.05 / 3;

test('the Speed buttons change the speed by click; the music fades out above 4x and back at 4x or below, a quick reversal included; Mute is 0 at once at any speed', async (env) => {
  const page = await bootGame(env, logGainSchedule);
  const gain = (want) => page.waitForFunction((g) => document.body.dataset.gain === g, want, { timeout: 10_000 });
  // The schedule since the last call, each change as [kind, value, at]: kind 'fade' (a cancel
  // and setTargetAtTime with the fade's time constant), 'set' (a cancel and setValueAtTime),
  // anything else 'other'.
  const schedule = () => page.evaluate((tau) => {
    const log = window.__gainLog.splice(0);
    const out = [];
    for (let i = 0; i < log.length; i += 1) {
      const [name, value, at, constant] = log[i];
      const cancelled = i > 0 && log[i - 1][0] === 'cancelScheduledValues' && log[i - 1][1] === at;
      if (name === 'cancelScheduledValues') continue;
      if (name === 'setTargetAtTime' && cancelled && Math.abs(constant - tau) < 1e-9) out.push(['fade', value, at]);
      else if (name === 'setValueAtTime' && cancelled) out.push(['set', value, at]);
      else out.push(['other', value, at]);
    }
    return out;
  }, FADE_TAU_S);
  const kinds = (changes) => changes.map(([kind, value]) => `${kind} ${value}`);
  const click = async (id, times = 1) => { for (let i = 0; i < times; i += 1) await page.click(`#${id}`); };
  await click('faster');
  assert.equal((await speedShown(page)).speed, '1.5', 'the + button');
  await engineSpeed(page, 1.5);
  await click('slower');
  assert.equal((await speedShown(page)).speed, '1', 'the - button');
  await engineSpeed(page, 1);
  await click('mute'); // the gesture that starts the sound, muted
  await page.waitForFunction(() => document.body.dataset.audio === 'running', null, { timeout: 15_000 });
  await gain('0.00');
  await schedule();
  await click('mute');
  assert.deepEqual(kinds(await schedule()), ['fade 1'], 'unmuted at 1x: the fade back');
  await gain('1.00');
  await click('faster', 4);
  assert.equal((await speedShown(page)).speed, '4');
  assert.deepEqual(kinds(await schedule()), ['fade 1', 'fade 1', 'fade 1', 'fade 1'], 'up to 4x: loud');
  await click('faster');
  assert.equal((await speedShown(page)).speed, '6', 'above 4x');
  assert.deepEqual(kinds(await schedule()), ['fade 0'], 'above 4x: the fade out');
  await gain('0.00');
  await click('slower');
  assert.equal((await speedShown(page)).speed, '4', 'back at 4x');
  assert.deepEqual(kinds(await schedule()), ['fade 1'], 'at 4x: the fade back');
  await gain('1.00');
  // A quick reversal: up and down again in one task; the second cancels the first's fade.
  await page.evaluate(() => {
    document.getElementById('faster').click();
    document.getElementById('slower').click();
  });
  const reversal = await schedule();
  assert.deepEqual(kinds(reversal), ['fade 0', 'fade 1'], 'a quick reversal: out, then back, the out cancelled');
  assert.ok(reversal[1][2] - reversal[0][2] < 0.05, 'the reversal comes within the fade');
  await h.waitFrames(page, 10);
  await gain('1.00');
  // Muted: 0 at once, at any speed, and no fade back while muted.
  await click('mute');
  assert.deepEqual(kinds(await schedule()), ['set 0'], 'Mute at 4x: 0 at once');
  await click('faster', 2);
  assert.equal((await speedShown(page)).speed, '8', 'muted, up to 8x');
  await click('slower', 3);
  assert.equal((await speedShown(page)).speed, '3', 'muted, down to 3x');
  assert.deepEqual(kinds(await schedule()), ['set 0', 'set 0', 'set 0', 'set 0', 'set 0'], 'muted, 8x and back to 3x: 0 at once each time, no fade');
  await h.waitFrames(page, 20);
  assert.equal(await page.evaluate(() => document.body.dataset.gain), '0.00', 'muted at 3x: silent');
  await click('mute');
  assert.deepEqual(kinds(await schedule()), ['fade 1'], 'unmuted at 3x: the fade back');
  await gain('1.00');
}, { needs: h.haveHfes });

// --- HD (spec 07 HD0) ------------------------------------------------------------------------
// The HD button beside Mute: on by default and kept in localStorage; with it on the canvas is
// the HD frame at 320·S × 200·S (S 4 when its box holds 3 device pixels per game pixel, else 2)
// in the same CSS box, off it is the RGBA frame at 320 × 200 as before; the pointer maps onto
// the game's 320 × 200 either way. The frames are compared in the page, by equality only.

// What the page shows of HD: the button, the canvas size and box, the stored choice.
const hdShown = (page) => page.evaluate(() => {
  const c = document.getElementById('screen');
  const box = c.getBoundingClientRect();
  let stored = null;
  try {
    stored = localStorage.getItem('u6.hd');
  } catch {
    stored = 'unreadable';
  }
  return {
    pressed: document.getElementById('hd').getAttribute('aria-pressed'),
    width: c.width,
    height: c.height,
    box: [Math.round(box.width), Math.round(box.height)],
    stored,
  };
});

// Page init script: the media queries the page makes, in `window.__mediaQueries`.
function watchMedia() {
  const made = [];
  window.__mediaQueries = made;
  const real = window.matchMedia.bind(window);
  window.matchMedia = (query) => {
    const q = real(query);
    made.push(q);
    return q;
  };
}

// A gated page (gameFirst) on the user's disks, started, the canvas focused; every request to
// another origin counted in `foreign`.
async function bootHd(env, foreign, init = watchMedia) {
  const { context, page } = await h.openPage(env);
  context.on('request', (r) => {
    const url = r.url();
    if (!url.startsWith(env.server.url) && !url.startsWith('data:') && !url.startsWith('blob:')) foreign.push(url);
  });
  if (init) await context.addInitScript(init);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await page.focus('#screen');
  return page;
}

test('HD: on by default and kept across a reload; the button switches it at once with no game step; off the canvas is the RGBA frame at 320 x 200, on the HD frame at 320·S x 200·S in the same box (S 2 in a smaller box, S 4 at twice the pixel ratio, S 2 again back on the first display); no foreign request', async (env) => {
  const foreign = [];
  const page = await bootHd(env, foreign);
  await h.waitPrompt(page);
  let shown = await hdShown(page);
  assert.ok(shown.pressed === 'true' && shown.stored === null, 'on by default, nothing stored');
  assert.ok(shown.width === 1280 && shown.height === 800, 'S 4: the box holds 3 device pixels per game pixel');
  assert.ok(await canvasShowsGame(page), 'the canvas is the HD frame');
  const box = shown.box;
  // The switch on a frozen display: the canvas changes with no frame of the game run.
  await h.virtualDisplay(page, 60);
  const ticks = await page.evaluate(() => globalThis.__ticks);
  const press = () => page.evaluate(() => document.getElementById('hd').click());
  await press();
  shown = await hdShown(page);
  assert.ok(shown.pressed === 'false' && shown.stored === 'off', 'off, stored');
  assert.ok(shown.width === 320 && shown.height === 200, 'off: 320 x 200');
  assert.deepEqual(shown.box, box, 'the same box');
  assert.ok(await canvasShowsGame(page), 'off: the RGBA frame, byte for byte');
  await press();
  shown = await hdShown(page);
  assert.ok(shown.pressed === 'true' && shown.stored === 'on' && shown.width === 1280, 'on again');
  assert.deepEqual(shown.box, box, 'the same box');
  assert.ok(await canvasShowsGame(page), 'on: the HD frame at once');
  assert.equal(await page.evaluate(() => globalThis.__ticks), ticks, 'no game step ran');
  await h.runFrames(page, 30);
  assert.ok(await canvasShowsGame(page), 'and after frames');
  // Kept across reloads; anything but off stored is on.
  await press();
  await page.reload();
  await started(page, FILES);
  await h.waitPrompt(page);
  shown = await hdShown(page);
  assert.ok(shown.pressed === 'false' && shown.width === 320, 'off survives a reload');
  assert.ok(await canvasShowsGame(page), 'the RGBA frame after the reload');
  for (const [stored, want] of [['on', 1280], ['dark', 1280]]) {
    await page.evaluate((v) => localStorage.setItem('u6.hd', v), stored);
    await page.reload();
    await started(page, FILES);
    await h.waitPrompt(page);
    shown = await hdShown(page);
    assert.ok(shown.pressed === 'true' && shown.width === want, `stored ${stored}: on`);
    assert.ok(await canvasShowsGame(page), `stored ${stored}: the HD frame`);
  }
  // A box under 3 device pixels per game pixel: S 2; the canvas follows a resize.
  await page.setViewportSize({ width: 700, height: 900 });
  await page.waitForFunction(() => document.getElementById('screen').width === 640, null, { timeout: 10_000 });
  shown = await hdShown(page);
  assert.ok(shown.height === 400 && shown.box[0] < 960, 'S 2 in a smaller box');
  await h.waitFrames(page, 2);
  assert.ok(await canvasShowsGame(page), 'S 2: the HD frame');
  // The same box on a display of twice the pixel ratio (the window moved to another screen):
  // S 4, with no resize of the box. Headless Chromium changes the ratio under the emulation
  // but fires no media-query change for it, so the test fires the one a display change fires
  // on the page's own query (`watchMedia`).
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Emulation.setDeviceMetricsOverride', { width: 700, height: 900, deviceScaleFactor: 2, mobile: false });
  assert.equal(await page.evaluate(() => devicePixelRatio), 2);
  assert.equal(await page.evaluate(() => document.getElementById('screen').width), 640, 'no resize: S 2 still');
  const fired = await page.evaluate(() => {
    const ratio = window.__mediaQueries.filter((q) => q.media.includes('dppx'));
    for (const q of ratio) q.dispatchEvent(new Event('change'));
    return ratio.length;
  });
  assert.equal(fired, 1, 'the page watches one pixel-ratio query');
  await page.waitForFunction(() => document.getElementById('screen').width === 1280, null, { timeout: 10_000 });
  assert.deepEqual((await hdShown(page)).box, shown.box, 'the same box');
  await h.waitFrames(page, 2);
  assert.ok(await canvasShowsGame(page), 'the pixel ratio doubled: S 4');
  // Back to the first display: the query the page armed for the new ratio fires, S 2 again.
  await cdp.send('Emulation.setDeviceMetricsOverride', { width: 700, height: 900, deviceScaleFactor: 1, mobile: false });
  assert.equal(await page.evaluate(() => devicePixelRatio), 1);
  const armed = await page.evaluate(() => {
    const ratio = window.__mediaQueries.filter((q) => q.media.includes('dppx'));
    ratio.at(-1).dispatchEvent(new Event('change'));
    return [ratio.length, ratio.at(-1).media.includes('2dppx')];
  });
  assert.deepEqual(armed, [2, true], 'one query armed per ratio, the second for the doubled one');
  await page.waitForFunction(() => document.getElementById('screen').width === 640, null, { timeout: 10_000 });
  assert.deepEqual((await hdShown(page)).box, shown.box, 'the same box');
  await h.waitFrames(page, 2);
  assert.ok(await canvasShowsGame(page), 'the pixel ratio back: S 2');
  await cdp.send('Emulation.clearDeviceMetricsOverride');
  assert.deepEqual(await page.evaluate(() => window.__csp), [], 'no CSP violation');
  assert.equal(foreign.length, 0, 'no request to another origin');
}, { needs: h.haveHfes });

// Page init script: every read and write of the page's storage throws (a browser that refuses
// it), and the page's media queries are kept (`watchMedia`).
function storageThrows() {
  for (const name of ['getItem', 'setItem', 'removeItem']) {
    Storage.prototype[name] = () => {
      throw new DOMException('test: storage refused', 'SecurityError');
    };
  }
}

test('HD: the button works from the keyboard (Space, Enter); with storage that throws HD is on, switches, and nothing is kept across a reload; no page error', async (env) => {
  const foreign = [];
  const page = await bootHd(env, foreign, storageThrows);
  const errors = [];
  page.on('pageerror', () => errors.push(1));
  await h.waitPrompt(page);
  let shown = await hdShown(page);
  assert.ok(shown.stored === 'unreadable', 'the storage throws');
  assert.ok(shown.pressed === 'true' && shown.width === 1280, 'on by default');
  await page.focus('#hd');
  await page.keyboard.press('Space');
  shown = await hdShown(page);
  assert.ok(shown.pressed === 'false' && shown.width === 320, 'Space: off');
  assert.ok(await canvasShowsGame(page), 'off: the RGBA frame');
  await page.keyboard.press('Enter');
  shown = await hdShown(page);
  assert.ok(shown.pressed === 'true' && shown.width === 1280, 'Enter: on');
  assert.ok(await canvasShowsGame(page), 'on: the HD frame');
  await page.keyboard.press('Space');
  assert.equal((await hdShown(page)).pressed, 'false', 'off again');
  await page.reload();
  await started(page, FILES);
  await h.waitPrompt(page);
  shown = await hdShown(page);
  assert.ok(shown.pressed === 'true' && shown.width === 1280, 'nothing kept: on after the reload');
  assert.ok(await canvasShowsGame(page), 'the HD frame');
  assert.equal(errors.length, 0, 'no page error');
  assert.equal(foreign.length, 0, 'no request to another origin');
}, { needs: h.haveHfes });

test('HD: a pointer on a game pixel reaches the game at that pixel with HD off, at 2x and at 4x; a click on the Look icon is Look in each', async (env) => {
  const foreign = [];
  const page = await bootHd(env, foreign);
  await h.waitPrompt(page);
  const POINTS = [[0, 0], [319, 199], [88, 88], [160, 100], [17, 183], [303, 9]];
  const modes = [['4x', 1024, 1280], ['2x', 700, 640], ['off', 1024, 320]];
  for (const [mode, width, canvasWidth] of modes) {
    await page.setViewportSize({ width, height: 900 });
    if (mode === 'off') await page.click('#hd');
    await page.waitForFunction((w) => document.getElementById('screen').width === w, canvasWidth, { timeout: 10_000 });
    await h.waitFrames(page, 2);
    for (const [x, y] of POINTS) {
      await page.mouse.move(...(await h.screenPoint(page, x, y)));
      await page.waitForFunction(([px, py]) => {
        const at = globalThis.__pointerAt;
        return at !== null && at[0] === px && at[1] === py;
      }, [x, y], { timeout: 5000 }).catch(() => assert.fail(`${mode}: (${x}, ${y}) queued at that pixel`));
    }
    await page.focus('#screen');
    await h.click(page, 64, 184, 30); // the fourth icon: Look
    assert.equal(await promptNow(page), '0', `${mode}: Look waits for its target`);
    await page.keyboard.press('Escape');
    await h.waitPrompt(page);
  }
  assert.equal(foreign.length, 0, 'no request to another origin');
}, { needs: h.haveHfes });

// Page init script: per display frame of the page's loop (its `frame` callback), the wall ms
// of the presents (the glue's `__presentMs`) and the canvas draws (putImageData) in it, and
// whether it drew, in `window.__hdTimes` while that is an array.
function timeHdFrames() {
  window.__hdTimes = null;
  let drawMs = 0;
  let draws = 0;
  const put = CanvasRenderingContext2D.prototype.putImageData;
  CanvasRenderingContext2D.prototype.putImageData = function (...args) {
    const t = performance.now();
    put.apply(this, args);
    drawMs += performance.now() - t;
    draws += 1;
  };
  const raf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (callback) => raf((now) => {
    if (callback.name !== 'frame' || !Array.isArray(window.__hdTimes)) return callback(now);
    const [p, d, n] = [globalThis.__presentMs, drawMs, draws];
    const out = callback(now);
    window.__hdTimes.push([globalThis.__presentMs - p + drawMs - d, draws - n, now]);
    return out;
  });
}

const quantile = (sorted, q) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];

// The H1 no-pack baseline: MMPX for every image. Spec 07 Acceptance 5 with the owner's full
// pack loaded is H2's receipt.
test('HD performance, H1 no-pack baseline (spec 07 Acceptance 5 with a full pack is H2\'s): a walk in the castle at 4x on the real display, the present and the canvas draw per display frame at S 4 and S 2: median at most 4 ms, p95 at most 8 ms (receipt)', async (env) => {
  const { context, page } = await h.openPage(env);
  await context.addInitScript(timeHdFrames);
  await h.load(page, env);
  await h.dropFiles(page, [h.ZIP]);
  await started(page);
  await h.waitPrompt(page);
  await page.focus('#screen');
  await speedTo(page, '4');
  await engineSpeed(page, 4);
  const receipts = [];
  for (const [s, width] of [[4, 1024], [2, 700]]) {
    await page.setViewportSize({ width, height: 900 });
    await page.waitForFunction((w) => document.getElementById('screen').width === w, 320 * s, { timeout: 10_000 });
    await h.waitFrames(page, 10);
    await page.evaluate(() => { window.__hdTimes = []; });
    const end = Date.now() + 8000;
    // Steps both ways and a pass between (the start fight's foes move on, a blocked step
    // passes). The leader's places are compared here, never printed: the party must walk.
    const places = new Set([(await h.state(page)).leader]);
    for (let i = 0; Date.now() < end; i += 1) {
      await page.keyboard.press(['ArrowRight', 'ArrowRight', 'ArrowLeft', 'ArrowLeft', 'Space', 'ArrowDown', 'ArrowUp'][i % 7]);
      await h.sleep(60);
      places.add((await h.state(page)).leader);
    }
    assert.ok(places.size >= 3, `S ${s}: the party walked during the timed frames`);
    const times = await page.evaluate(() => {
      const t = window.__hdTimes;
      window.__hdTimes = null;
      return t;
    });
    const all = times.map((f) => f[0]).sort((a, b) => a - b);
    const drawn = times.filter((f) => f[1] > 0).map((f) => f[0]).sort((a, b) => a - b);
    const hz = (times.length - 1) / ((times.at(-1)[2] - times[0][2]) / 1000);
    const [median, p95] = [quantile(all, 0.5), quantile(all, 0.95)];
    receipts.push(`H1 no-pack baseline, S ${s}: ${times.length} frames at ${hz.toFixed(0)} Hz, ${drawn.length} drawn; per frame median ${median.toFixed(2)} ms, p95 ${p95.toFixed(2)} ms, max ${all.at(-1).toFixed(2)} ms; drawn frames median ${quantile(drawn, 0.5).toFixed(2)} ms, p95 ${quantile(drawn, 0.95).toFixed(2)} ms`);
    assert.ok(drawn.length >= 30, `S ${s}: the walk changed the frame`);
    assert.ok(median <= 4, `S ${s}: median at most 4 ms`);
    assert.ok(p95 <= 8, `S ${s}: p95 at most 8 ms`);
  }
  for (const r of receipts) console.log(`  receipt: ${r}`);
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

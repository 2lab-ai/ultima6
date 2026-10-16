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
// drawn only when it changed), and HD (spec 07: the switch and its storage, the S× canvas and
// its scaling, the pointer, the MMPX notice the site ships, the performance baseline; an HD
// pack dropped, shown, kept, forgotten and refused, its performance with a full pack). Most
// tests start `game` at once (harness `gameFirst`); the launcher tests run the page as it is.
// Gates hold file reads and storage calls at chosen points, so every interleaving is forced,
// not timed. The game is read once it reached what a check asserts, by its own signals (the
// prompt, the key queue, the program, its ticks: the page's fixed steps of 20 ms), never after a
// fixed sleep: a loaded host draws fewer frames per second, a frame moves the game at most
// 250 ms (the pace's cap) and a host past its frame budget drops steps, so a sleep there
// covers less of the game. Tests that need the user's disks skip when they are absent.
//
//   node tools/web/test-web.mjs [name-filter]      (build first: bash scripts/build-web.sh)
import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { mkdir, readFile, stat, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
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

// No page: the harness's step bookkeeping on synthetic frames and positions, so every direction
// is exercised whichever one a boot walks: the map's scroll for a step along either axis, and
// what a key toward a cell did (a step into the wake cell from beside it, toward it, is a walk:
// h.stepResult says why; ending there from anywhere else is the allowed wake destination).
test('a step\'s bookkeeping: the scroll of a step east, west, south or north; blocked, walked (into the wake cell too), on the allowed wake destination; any other cell fails', async () => {
  const shown = (cx, cy) => {
    const f = new Array(320 * 200 * 4).fill(0);
    for (let y = 0; y < 200; y += 1) {
      for (let x = 0; x < 320; x += 1) {
        const v = Math.imul(x + 16 * cx, 73856093) ^ Math.imul(y + 16 * cy, 19349663);
        f.fill(v & 255, (y * 320 + x) * 4, (y * 320 + x) * 4 + 3);
      }
    }
    return f;
  };
  const before = shown(0, 0);
  for (const step of h.STEPS) {
    const walked = h.mapShift(before, shown(step.dx, step.dy));
    assert.ok(walked.dx === 16 * step.dx && walked.dy === 16 * step.dy && walked.score === 1, `the scroll of ${step.key}`);
    assert.ok(walked.still < 0.5, `${step.key}: not still`);
  }
  const still = h.mapShift(before, before);
  assert.ok(still.dx === 0 && still.dy === 0 && still.still === 1, 'no step: no scroll');
  const [east, , south] = h.STEPS;
  assert.equal(h.stepResult('300,300,0', '300,300,0', east), 'blocked');
  assert.equal(h.stepResult('300,300,0', '301,300,0', east), 'walked');
  assert.equal(h.stepResult('300,300,0', '300,301,0', south), 'walked');
  assert.equal(h.stepResult('300,300,0', h.WAKE_CELL, east), 'wake cell');
  const [wx, wy, wz] = h.WAKE_CELL.split(',').map(Number);
  assert.equal(h.stepResult(`${wx - 1},${wy},${wz}`, h.WAKE_CELL, east), 'walked', 'a step east into the wake cell from beside it is a walk');
  assert.equal(h.stepResult(`${wx},${wy - 1},${wz}`, h.WAKE_CELL, east), 'wake cell', 'from beside it the other way: the wake destination');
  assert.equal(h.stepResult(h.WAKE_CELL, `${wx},${wy + 1},${wz}`, south), 'walked', 'a step out of it is');
  assert.equal(h.stepResult(h.WAKE_CELL, h.WAKE_CELL, east), 'blocked');
  for (const [to, label] of [['302,300,0', 'two cells'], ['299,300,0', 'the other way'], ['301,301,0', 'a diagonal'], ['301,300,1', 'another level']]) {
    assert.throws(() => h.stepResult('300,300,0', to, east), assert.AssertionError, `fails: ${label}`);
  }
});

// No page: h.stepOnce's tries driven by a stub of the page's side (`io`: the leader, the frame,
// the step, the pass) on a synthetic grid: the cells a step may enter (`open`, asked with the
// passes so far), the leader moved by each step it allows. Pinned: the order of the directions
// (east and west blocked, south walks: an east-or-west step fails it), a pass only after a round
// of four blocked, the bound (16 tries, 3 passes) and its failure, naming each try's key and
// outcome; a try that ends on the wake cell, then the next direction from there; a walk into the
// wake cell from beside it is the step (counted otherwise, the step would walk in and back out
// or, where that is the one way out, not at all); a walk back onto the cell the call started on
// (the hero killed in a pass, woken beside it) is not; the frame read only with `withFrame`; the
// prompt waited before every try; the label's count of what the page sent for each try (0 for a
// key lost on the way, '?' with no count); a failure of the step itself (the game never took the
// input) not retried.
test('a step\'s tries: east, west, south, north, a pass after four blocked, 4 rounds; the wake cell; never back where it began; the frame only when asked; a step that fails stops it', async () => {
  const stub = ({ at = '300,300,0', open = () => false, wakeOn = null, wakeOnPass = null, failOn = null, lostOn = null } = {}) => {
    const io = { at, passes: 0, acts: [], frames: 0, prompts: 0, keys: 0 };
    Object.assign(io, {
      prompt: async () => { io.prompts += 1; },
      leader: async () => io.at,
      frame: async () => { io.frames += 1; return [io.acts.length]; },
      sent: async () => io.keys,
      act: async (step) => {
        io.acts.push(step.key);
        if (io.acts.length === lostOn) return; // the key never reached the game: nothing moves
        io.keys += 1;
        if (io.acts.length === failOn) throw new Error('test: the input was not taken');
        if (io.acts.length === wakeOn) {
          io.at = h.WAKE_CELL;
          return;
        }
        const [x, y, z] = io.at.split(',').map(Number);
        const to = `${x + step.dx},${y + step.dy},${z}`;
        if (open(to, io.passes)) io.at = to;
      },
      pass: async () => {
        io.passes += 1;
        if (io.passes === wakeOnPass) io.at = h.WAKE_CELL;
      },
    });
    return io;
  };
  const keys = h.STEPS.map((s) => s.key);
  // East and west blocked, south open: the third try walks, no pass.
  let io = stub({ open: (to) => to === '300,301,0' });
  let got = await h.stepOnce(null, { io });
  assert.deepEqual([got.key, got.dx, got.dy, got.from, got.to], ['ArrowDown', 0, 1, '300,300,0', '300,301,0'], 'south after east and west blocked');
  assert.deepEqual(io.acts, keys.slice(0, 3), 'east, west, then south');
  assert.ok(io.passes === 0 && io.frames === 0 && got.before === undefined, 'no pass; no frame read without withFrame');
  assert.equal(io.prompts, io.acts.length, 'the prompt before every try');
  // Every side blocked until the second pass: a pass after each round of four, then east.
  io = stub({ open: (to, passes) => passes >= 2 && to === '301,300,0' });
  got = await h.stepOnce(null, { io, withFrame: true });
  assert.deepEqual(io.acts, [...keys, ...keys, 'ArrowRight'], 'two rounds of four, then east');
  assert.ok(io.passes === 2 && got.key === 'ArrowRight', 'a pass between rounds only');
  assert.ok(io.frames === io.acts.length && got.before[0] === io.acts.length - 1, 'with withFrame: the frame before each try, the walked one returned');
  assert.equal(io.prompts, io.acts.length, 'the prompt before every try, after the passes too');
  // Never open: fails after 4 rounds (16 tries, 3 passes), naming every try and pass in order.
  io = stub();
  const bound = await h.stepOnce(null, { io, what: 'test step' }).then(() => null, (err) => err);
  assert.ok(bound instanceof assert.AssertionError, 'an assertion: the runner shows its label');
  assert.ok(io.acts.length === h.STEP_ROUNDS * h.STEPS.length && io.passes === h.STEP_ROUNDS - 1, '16 tries, 3 passes');
  const round = keys.map((k) => `${k} blocked (sent 1)`).join(', ');
  assert.equal(bound.message, `no test step in 4 rounds of the four directions: ${[round, round, round, round].join(', pass, ')}`);
  assert.equal(io.prompts, io.acts.length, 'the prompt before every try of the bound');
  // A key lost on the way reads as blocked; the label tells it by what the page sent for it.
  io = stub({ lostOn: 1 });
  const lost = await h.stepOnce(null, { io, what: 'test step' }).then(() => null, (err) => err);
  assert.ok(lost.message.startsWith('no test step in 4 rounds of the four directions: ArrowRight blocked (sent 0), ArrowLeft blocked (sent 1),'), 'a lost key: sent 0');
  // With no count (an io without `sent`): '?'.
  io = stub();
  delete io.sent;
  const uncounted = await h.stepOnce(null, { io, what: 'test step' }).then(() => null, (err) => err);
  assert.ok(uncounted.message.startsWith('no test step in 4 rounds of the four directions: ArrowRight blocked (sent ?),'), 'not counted: ?');
  // The second try ends on the wake cell: the next direction from there (south) walks.
  const [wx, wy, wz] = h.WAKE_CELL.split(',').map(Number);
  io = stub({ wakeOn: 2, open: (to) => to === `${wx},${wy + 1},${wz}` });
  got = await h.stepOnce(null, { io });
  assert.deepEqual(io.acts, keys.slice(0, 3), 'east blocked, west ends on the wake cell, then south');
  assert.ok(got.from === h.WAKE_CELL && got.key === 'ArrowDown', 'the step walked from the wake cell');
  // East of the wake cell with only the way into it open (the gargoyles of the woken hero on
  // its other sides): west into it is the step, at the second try.
  const beside = `${wx + 1},${wy},${wz}`;
  io = stub({ at: beside, open: (to) => to === h.WAKE_CELL });
  got = await h.stepOnce(null, { io });
  assert.deepEqual(io.acts, ['ArrowRight', 'ArrowLeft'], 'east blocked, west into the wake cell');
  assert.ok(got.from === beside && got.to === h.WAKE_CELL && io.passes === 0, 'a walk into the wake cell is the step');
  // The same cell, every side blocked until the hero dies in the first pass and wakes beside it:
  // east from the wake cell back onto it is not the step ('back'), west from it then is.
  io = stub({ at: beside, wakeOnPass: 1, open: (to, passes) => passes >= 1 && (to === beside || to === h.WAKE_CELL) });
  got = await h.stepOnce(null, { io, what: 'test step' });
  assert.deepEqual(io.acts, [...keys, 'ArrowRight', 'ArrowLeft'], 'a round, the pass, east back, then west');
  assert.ok(got.from === beside && got.to === h.WAKE_CELL, 'the leader ends elsewhere than he began');
  // A try that ends on the wake cell from afar, then nothing open: the outcomes named in order.
  io = stub({ wakeOn: 1 });
  const named = await h.stepOnce(null, { io, what: 'test step' }).then(() => null, (err) => err);
  assert.ok(named instanceof assert.AssertionError && named.message.startsWith('no test step in 4 rounds of the four directions: ArrowRight wake cell (sent 1), ArrowLeft blocked (sent 1), ArrowDown blocked (sent 1), ArrowUp blocked (sent 1), pass,'), 'the outcomes named: wake cell, blocked, pass');
  // A step whose input the game never took fails as it did, not as a blocked try.
  io = stub({ failOn: 1 });
  const failed = await h.stepOnce(null, { io }).then(() => null, (err) => err);
  assert.ok(failed instanceof Error && failed.message === 'test: the input was not taken' && io.acts.length === 1, 'the failure of the step itself, at once');
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
  // The chooser wait is armed here, before the Tabs, so that the key that opens the chooser
  // comes well after the arming. Playwright turns on Chromium's chooser interception when the
  // first 'filechooser' listener is added, by a protocol call nothing awaits; a chooser the
  // page opens before the interception took effect is not intercepted, headless dismisses it
  // and no event comes. That the page round trips after the arming are answered after the
  // interception is observed, not documented: armed in the same Promise.all as the key, Space
  // failed 6 of 30 and Enter 21 of 26 under 10x CPU throttling; with one read between the
  // arming and the key, the interception was answered before the key was sent in 170 of 170
  // runs (Enter 0 of 100 failed, under 10x Enter 0 of 40 and Space 0 of 30). Here six round
  // trips (the Tabs, the arrow, the reads) lie between them (under 10x, 30 of 30 answered
  // before the key; the arming to the chooser took 2.1 s at most, well inside the wait's
  // default 30 s), so keep the arming above the Tabs.
  // Enter, not Space: Space clicks on its keyup, an input event of its own, and the keydown's
  // user activation lapses within seconds (a Space held 4 s opened the chooser, 6 s did not);
  // a loaded host has delivered the keyup over 6 s after the keydown, and the chooser was
  // refused. Enter clicks in its keydown's own task. When a check fails before the wait is
  // awaited, the `.catch` keeps its rejection (the page is closed) from going unhandled and
  // ending the run; `await chosen` still throws.
  const chosen = page.waitForEvent('filechooser');
  chosen.catch(() => {});
  await page.keyboard.press('Tab');
  assert.equal((await h.state(page)).focused, 'screen', 'the canvas is in the tab order');
  await page.keyboard.press('ArrowDown');
  assert.deepEqual(await page.evaluate(() => window.__keys.at(-1)), ['ArrowDown', false], 'no game: arrows stay the page\'s');
  await page.keyboard.press('Tab');
  assert.equal((await h.state(page)).focused, 'picker');
  await page.keyboard.press('Enter');
  const chooser = await chosen;
  await chooser.setFiles(h.HFES);
  const s = await started(page, FILES);
  assert.equal(s.focused, 'screen', 'focus lands on the canvas after the import');
  assert.equal(await page.evaluate(() => getComputedStyle(document.activeElement).outlineColor), 'rgb(255, 209, 102)');
  // From the prompt, one step east (another way when the start fight blocks it), then the
  // prompt again.
  const { key, dx, dy, before } = await h.stepOnce(page, { withFrame: true });
  assert.deepEqual(await page.evaluate(() => window.__keys.at(-1)), [key, true], 'the canvas took the arrow');
  const walked = h.mapShift(before, await h.frame(page));
  assert.ok(walked.dx === 16 * dx && walked.dy === 16 * dy && walked.score > 0.5, `one step (${key}) scrolled the map`);
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
  assert.ok(after.dx === 0 && after.dy === 0, 'the map stayed');
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
  const { key, dx, dy, before } = await h.stepOnce(page, { withFrame: true });
  const walked = h.mapShift(before, await h.frame(page));
  assert.ok(walked.dx === 16 * dx && walked.dy === 16 * dy && walked.score > 0.5, `one step (${key}) after A`);
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

// Walks one step (h.stepOnce), then saves (Ctrl-S, Y) and waits until the save is stored;
// returns the leader's position before and after the step.
async function walkAndSave(page) {
  await h.waitPrompt(page); // the boot reaches the prompt
  const start = (await h.state(page)).leader;
  const { to: moved } = await h.stepOnce(page);
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
  const { to: later } = await h.stepOnce(page);
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
  const { to: later } = await h.stepOnce(page);
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
  let step = await h.stepOnce(page);
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
  step = await h.stepOnce(page);
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

// One step by clicks: `h.stepOnce` with a click instead of each arrow, on the map cell `dx`
// east and `dy` south of the hero (its window cell (5, 5) is centred on game pixel (88, 88)),
// then until the game took it and shows the prompt again: the same directions, outcomes and
// bound (a pass after a round of four blocked: the clicks focused the canvas).
async function clickStep(page) {
  const act = async ({ dx, dy }) => {
    await h.click(page, 88 + 16 * dx, 88 + 16 * dy, 0);
    await h.waitPrompt(page);
  };
  // What a click sent: the button changes the game took (`__buttons`, a gated page's glue: a
  // press and its release), not counted on an ungated page.
  const sent = () => page.evaluate(() => globalThis.__buttons?.length ?? null);
  return h.stepOnce(page, { io: { ...h.pageSteps(page), act, sent }, what: 'click step' });
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
  const { from, to } = await h.stepOnce(page);
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
// `h.stepOnce` tries the next). Which direction walked depends on the boot's game state, so the
// receipt does not name it.
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
// that disagrees with its central record). Each CRC is the CRC-32 of the entry's data (the
// intake checks it for every entry it reads), except for an entry of more than 16 MiB, never one
// the intake reads here, whose CRC is 0. Returns { head, dir, end(at) }: the local headers and
// data, the central directory, and the end record for a directory at `at`.
function zipKit() {
  const u16 = (v) => [v & 255, (v >>> 8) & 255];
  const u32 = (v) => [v & 255, (v >>> 8) & 255, (v >>> 16) & 255, (v >>> 24) & 255];
  const table = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crcOf = (fill, length) => {
    if (length > 16 << 20) return 0;
    let c = 0xffffffff;
    for (let i = 0; i < length; i++) c = table[(c ^ fill) & 255] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  window.__makeZip = (entries) => {
    const head = [];
    const dir = [];
    let offset = 0;
    for (const e of entries) {
      const name = [...new TextEncoder().encode(e.name)];
      const local = e.localName ? [...new TextEncoder().encode(e.localName)] : name;
      const method = e.method ?? 0;
      const crc = crcOf(e.fill ?? 0, e.length);
      const sizes = [...u32(crc), ...u32(e.csize ?? e.length), ...u32(e.full ?? e.length)];
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
  // The drop classifier's read of its end (spec 07 H2: is it an HD pack?), then the import's.
  assert.ok(r.slices === 1 + 3 && r.sliced > 4096 && r.sliced < 2 ** 20, 'its end for the classifier, then its first bytes, its directory and the disk image only');
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
  // Each drop: the classifier's read of the end (spec 07 H2), then the import's.
  console.log(`  receipt: the packed zip: ${r.slices - 4} slices read, ${r.forbidden} forbidden requests`);
  assert.ok(r.slices === (1 + 3) + (1 + 2) && r.forbidden === 0, 'its end for the classifier, then its first bytes and its tail, not the entry');
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
    rendering: getComputedStyle(c).imageRendering,
  };
});

// The stylesheet's sharp scaling (pixelated, or crisp-edges where the browser takes it).
const SHARP = ['pixelated', 'crisp-edges'];

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
  assert.ok(shown.box[0] < 1280 && shown.rendering === 'auto', 'more canvas pixels than box pixels: smooth');
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
  assert.ok(SHARP.includes(shown.rendering), 'off: sharp');
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
  assert.ok(shown.box[0] >= 640 && SHARP.includes(shown.rendering), 'S 2, a larger box: sharp');
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
  assert.ok(SHARP.includes((await hdShown(page)).rendering), 'S 4 in a box of more device pixels: sharp');
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
  assert.ok(SHARP.includes((await hdShown(page)).rendering), 'back: sharp');
  await cdp.send('Emulation.clearDeviceMetricsOverride');
  assert.deepEqual(await page.evaluate(() => window.__csp), [], 'no CSP violation');
  assert.equal(foreign.length, 0, 'no request to another origin');
}, { needs: h.haveHfes });

test('the MMPX notice ships with the site: NOTICE.txt is the repository\'s NOTICE, as text, linked from the HD paragraph', async (env) => {
  const { page } = await h.openPage(env, { gated: false });
  await h.load(page, env);
  const served = await page.request.get(`${env.server.url}NOTICE.txt`);
  assert.equal(served.status(), 200);
  assert.match(served.headers()['content-type'], /^text\/plain/);
  const repo = await readFile(join(h.REPO, 'NOTICE'));
  assert.ok(Buffer.from(await served.body()).equals(repo), 'the repository\'s NOTICE, byte for byte');
  const text = repo.toString('utf8');
  for (const part of ['MMPX', 'Copyright 2020 Morgan McGuire & Mara Gagiu', 'Permission is hereby granted, free of charge']) {
    assert.ok(text.includes(part), `the notice holds: ${part}`);
  }
  const link = await page.evaluate(() => {
    const a = document.querySelector('#hd-credits a[href="NOTICE.txt"]');
    return a ? new URL(a.href).pathname : null;
  });
  assert.equal(link, '/NOTICE.txt', 'linked from the HD paragraph');
});

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
// pack loaded is H2's receipt. Not measured here: a change of the palette (the opening's fades)
// runs MMPX again on every image shown, since an HD image is made in the frame's colours; the
// fades lie outside Acceptance 5's walk, and in `game` the palette does not change.
test('HD performance, H1 no-pack baseline (spec 07 Acceptance 5 with a full pack is H2\'s): a walk in the castle at 4x on the real display, the present and the canvas draw per display frame at S 4 and S 2, all frames and the drawn ones: median at most 4 ms, p95 at most 8 ms (receipt)', async (env) => {
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
    // One step after another until the time is up (h.stepOnce: east, else west, south, north,
    // a pass after a round of four blocked; each one the one cell walked): the party must walk.
    // No frame read before each try (`withFrame` off): a canvas read at S 4 inside the timed
    // window would halve the steps.
    let walked = 0;
    while (Date.now() < end) {
      await h.stepOnce(page);
      walked += 1;
    }
    assert.ok(walked >= 2, `S ${s}: the party walked during the timed frames`);
    const times = await page.evaluate(() => {
      const t = window.__hdTimes;
      window.__hdTimes = null;
      return t;
    });
    const all = times.map((f) => f[0]).sort((a, b) => a - b);
    const drawn = times.filter((f) => f[1] > 0).map((f) => f[0]).sort((a, b) => a - b);
    const hz = (times.length - 1) / ((times.at(-1)[2] - times[0][2]) / 1000);
    const [median, p95] = [quantile(all, 0.5), quantile(all, 0.95)];
    receipts.push(`H1 no-pack baseline, S ${s}: ${times.length} frames at ${hz.toFixed(0)} Hz, ${drawn.length} drawn, ${walked} steps walked; per frame median ${median.toFixed(2)} ms, p95 ${p95.toFixed(2)} ms, max ${all.at(-1).toFixed(2)} ms; drawn frames median ${quantile(drawn, 0.5).toFixed(2)} ms, p95 ${quantile(drawn, 0.95).toFixed(2)} ms`);
    assert.ok(drawn.length >= 30, `S ${s}: the walk changed the frame`);
    assert.ok(median <= 4, `S ${s}: median at most 4 ms`);
    assert.ok(p95 <= 8, `S ${s}: p95 at most 8 ms`);
    // The frames that drew: a walk that pressed fewer keys has more idle frames, which would
    // lower the all-frames figures alone.
    assert.ok(quantile(drawn, 0.5) <= 4, `S ${s}: drawn frames median at most 4 ms`);
    assert.ok(quantile(drawn, 0.95) <= 8, `S ${s}: drawn frames p95 at most 8 ms`);
  }
  for (const r of receipts) console.log(`  receipt: ${r}`);
  console.log('  receipt: note: a palette change (the opening\'s fades) runs MMPX again on every image shown; fades are outside this walk');
}, { needs: h.haveZip });


// --- the HD pack (spec 07 H2) ---------------------------------------------------------------
// A synthetic pack the test builds in the page (h.packKit) for one tile record the game shows
// at a command icon: its key is the one the page itself computes for the record now
// (Game.hd_image_key), its picture the test's pattern. Positions, ids, keys and pixels stay in
// the page; only booleans, the test's own counts and hashes compared for equality leave it.

// Page init script: `window.__icon()` finds a command icon, a 16 x 16 block of the command bar
// whose every pixel is drawn from one tile record at its own (u, v), unflagged (kept in
// `window.__cell`); `window.__packTile(variant)` builds the test's pack for that record
// (variant: 'good', 'other' (another pattern), or a breakage: 'crc' (a picture's bytes changed,
// its CRC-32 not), 'size' (the picture of another size than its index says), 'entry' (a file a
// pack does not have), 'index' (an index over the entry limit)); `window.__dropPack(variants)`
// drops one file per variant ('marker': a game file); `window.__cellShows(s, variant)` whether
// the canvas shows the cell as the pack image of `variant` at scale `s` (each sub-pixel over
// the pixel's classic colour, at 2x box-averaged by alpha: u6-render's rule), and
// `window.__cellHash()` a hash of the cell's canvas block (compared, never printed).
function hdPackPage() {
  const pattern = (variant) => (x, y) => (variant === 'other'
    ? [255 - x * 2, 99, y * 3, (x * y) % 7 === 0 ? 0 : 255]
    : [(x * 4) & 255, (y * 4) & 255, 77, (x + y) % 9 === 0 ? 0 : 255]);
  const rgbaOf = (w, h, f) => {
    const out = new Uint8Array(w * h * 4);
    for (let i = 0; i < w * h; i++) out.set(f(i % w, Math.floor(i / w)), i * 4);
    return out;
  };
  window.__icon = () => {
    const g = globalThis.__shownGame;
    for (let y0 = 176; y0 <= 184; y0++) {
      for (let x0 = 0; x0 <= 304; x0++) {
        const s = g.hd_source_at(x0, y0);
        if (s[0] !== 1 || s[3] !== 0 || s[4] !== 0 || s[5] !== 0) continue;
        let full = true;
        for (let k = 0; k < 256 && full; k++) {
          const t = g.hd_source_at(x0 + (k % 16), y0 + Math.floor(k / 16));
          full = t[0] === 1 && t[1] === s[1] && t[3] === k % 16 && t[4] === Math.floor(k / 16) && t[5] === 0;
        }
        if (full) {
          window.__cell = { x0, y0, id: s[1] };
          return true;
        }
      }
    }
    return false;
  };
  window.__packTile = async (variant) => {
    const g = globalThis.__shownGame;
    const { id } = window.__cell;
    const tile = { kind: 1, id, letter: 0, hash: g.hd_image_key(1, id), width: 64, height: 64, rgba: rgbaOf(64, 64, pattern(variant)) };
    const files = await window.__packFiles([tile]);
    const at = (name) => files.findIndex(([n]) => n === name);
    if (variant === 'crc') {
      const original = files[at('atlas-0.webp')][1];
      const changed = original.slice();
      changed[changed.length >> 1] ^= 0x40;
      files[at('atlas-0.webp')][1] = changed;
      return window.__zipFiles(files, { crcOf: ['atlas-0.webp', original] });
    }
    if (variant === 'size') {
      // A lossless picture (its VP8L chunk checked) four rows short of the atlas the index
      // names, so only its size is wrong.
      const canvas = new OffscreenCanvas(2048, 60);
      canvas.getContext('2d').fillRect(0, 0, 2048, 60);
      const blob = await canvas.convertToBlob({ type: 'image/webp', quality: 1 });
      const bytes = new Uint8Array(await blob.arrayBuffer());
      const text = String.fromCharCode(...bytes.subarray(0, 4096));
      if (!text.includes('VP8L') || text.includes('VP8 ')) throw new Error('not a lossless WebP');
      files[at('atlas-0.webp')][1] = bytes;
    }
    if (variant === 'entry') files.push(['notes.txt', new TextEncoder().encode('not a pack file')]);
    if (variant === 'index') new DataView(files[at('index.bin')][1].buffer).setUint32(12, 5000, true);
    if (variant === 'manifest') {
      // The manifest's CRC-32 taken from other bytes: it does not read, so the zip is not a
      // pack (spec 07 H2's rule) and goes to the import.
      return window.__zipFiles(files, { crcOf: ['manifest.json', new Uint8Array(1)] });
    }
    return window.__zipFiles(files);
  };
  // A pack of one made-up record (no game needed) whose manifest's CRC-32 is wrong.
  window.__brokenPack = async () => {
    const tile = { kind: 1, id: 1, letter: 0, hash: '0000000000000001', width: 64, height: 64, rgba: rgbaOf(64, 64, pattern('good')) };
    return window.__zipFiles(await window.__packFiles([tile]), { crcOf: ['manifest.json', new Uint8Array(1)] });
  };
  window.__dropPack = async (variants) => {
    const dt = new DataTransfer();
    for (const [i, v] of variants.entries()) {
      if (v === 'marker') dt.items.add(new File(['not a game file'], 'port-test-marker.txt'));
      else if (v === 'claim') {
        // A zip of a disk image and a manifest.json that names the pack format but is cut short:
        // not a pack (it does not parse), so the import takes it.
        const files = [['disks/Made 1.adf', new Uint8Array(4096).fill(2)],
          ['manifest.json', new TextEncoder().encode('{"format": "u6-hdpack/1", "tile"')]];
        dt.items.add(new File([window.__zipFiles(files)], 'claim.zip'));
      } else dt.items.add(new File([await window.__packTile(v)], `test-pack-${i}.zip`));
    }
    document.getElementById('drop').dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }));
  };
  const block = () => {
    const c = document.getElementById('screen');
    const s = c.width / 320;
    const { x0, y0 } = window.__cell;
    return { s, d: c.getContext('2d').getImageData(x0 * s, y0 * s, 16 * s, 16 * s).data };
  };
  window.__cellShows = (want, variant) => {
    const { s, d } = block();
    if (s !== want) return false;
    const g = globalThis.__shownGame;
    const frame = g.frame_rgba();
    const { x0, y0 } = window.__cell;
    const p = pattern(variant);
    const down = (X, Y) => {
      const c = [0, 0, 0];
      let a = 0;
      for (const [dx, dy] of [[0, 0], [1, 0], [0, 1], [1, 1]]) {
        const q = p(2 * X + dx, 2 * Y + dy);
        for (let k = 0; k < 3; k++) c[k] += q[k] * q[3];
        a += q[3];
      }
      if (a === 0) return [0, 0, 0, 0];
      const ch = (v) => Math.floor((v + Math.floor(a / 2)) / a);
      return [ch(c[0]), ch(c[1]), ch(c[2]), Math.floor((a + 2) / 4)];
    };
    for (let Y = 0; Y < 16 * s; Y++) {
      for (let X = 0; X < 16 * s; X++) {
        const at = ((y0 + Math.floor(Y / s)) * 320 + x0 + Math.floor(X / s)) * 4;
        const k = [frame[at], frame[at + 1], frame[at + 2], 255];
        const c = s === 4 ? p(X, Y) : down(X, Y);
        const want4 = c[3] === 255 ? [c[0], c[1], c[2], 255] : c[3] === 0 ? k
          : [0, 1, 2].map((i) => Math.floor((c[i] * c[3] + k[i] * (255 - c[3]) + 127) / 255)).concat(255);
        const i = (Y * 16 * s + X) * 4;
        for (let n = 0; n < 4; n++) if (d[i + n] !== want4[n]) return false;
      }
    }
    return true;
  };
  window.__cellHash = () => {
    const { d } = block();
    let x = 0x811c9dc5;
    for (let i = 0; i < d.length; i += 1) x = Math.imul(x ^ d[i], 0x01000193) >>> 0;
    return x;
  };
}

// What the page says of the HD pack: data-hd-pack, data-hd-matched, the forget button's state.
const packShown = (page) => page.evaluate(() => ({
  pack: document.body.dataset.hdPack,
  matched: document.body.dataset.hdMatched,
  forgetEnabled: !document.getElementById('forget-hd').disabled,
}));

// The bytes the loaded pack's pictures take decoded (data-hd-pack-bytes), 0 without one.
const packBytes = (page) => page.evaluate(() => Number(document.body.dataset.hdPackBytes ?? 0));

// A gated page (gameFirst) with the pack kit, the user's disks started, every request to another
// origin counted in `foreign`, the command icon found; `init` runs in the page first.
async function bootPack(env, foreign, init) {
  const { context, page } = await h.openPage(env);
  context.on('request', (r) => {
    const url = r.url();
    if (!url.startsWith(env.server.url) && !url.startsWith('data:') && !url.startsWith('blob:')) foreign.push(url);
  });
  if (init) await context.addInitScript(init);
  await context.addInitScript(h.packKit);
  await context.addInitScript(hdPackPage);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  return page;
}

// The cell's hash at each scale (the canvas at 1024 px: S 4; at 700 px: S 2), ending at S 4.
async function cellHashes(page) {
  const out = {};
  for (const [s, width] of [[2, 700], [4, 1024]]) {
    await page.setViewportSize({ width, height: 900 });
    await page.waitForFunction((w) => document.getElementById('screen').width === w, 320 * s, { timeout: 10_000 });
    await h.waitFrames(page, 2);
    out[s] = await page.evaluate(() => window.__cellHash());
  }
  return out;
}

// Whether the cell shows the pack's image of `variant` at S 4 and at S 2 (ending at S 4).
async function cellShowsPack(page, variant = 'good') {
  const shown = [];
  for (const [s, width] of [[4, 1024], [2, 700], [4, 1024]]) {
    await page.setViewportSize({ width, height: 900 });
    await page.waitForFunction((w) => document.getElementById('screen').width === w, 320 * s, { timeout: 10_000 });
    await h.waitFrames(page, 2);
    shown.push(await page.evaluate(([want, v]) => window.__cellShows(want, v), [s, variant]));
  }
  return shown.every(Boolean);
}

test('HD pack: a dropped pack starts no game and touches no kept file; its counts show; the HD frame shows its image at a command icon at S 4 and S 2, at once with no step; a reload loads it before the game starts; Forget the HD pack: MMPX again at once and nothing stored; no foreign request', async (env) => {
  const foreign = [];
  const page = await bootPack(env, foreign);
  assert.equal(await page.evaluate(() => window.__icon()), true, 'a command icon drawn from one record');
  assert.deepEqual(await packShown(page), { pack: 'none', matched: '', forgetEnabled: true });
  const mmpx = await cellHashes(page);
  assert.equal(await page.evaluate(() => window.__cellShows(4, 'good')), false, 'MMPX before the pack');
  await page.evaluate(() => { window.__g0 = globalThis.__shownGame; });
  const keptBefore = await kept(page, FILES, 'the game files are kept');
  const from = (await h.log(page)).length;
  // The drop on a frozen display: the pack shows with no game step.
  await h.virtualDisplay(page, 60);
  const ticks = await page.evaluate(() => globalThis.__ticks);
  await page.evaluate(() => window.__dropPack(['good']));
  await page.waitForFunction(() => document.body.dataset.ops === '0' && document.body.dataset.hdPack !== 'none', null, { timeout: 30_000, polling: 50 });
  assert.equal(await page.evaluate(() => globalThis.__ticks), ticks, 'no game step ran');
  assert.equal(await page.evaluate(() => window.__cellShows(4, 'good')), true, 'S 4: the pack image, at once');
  let s = await h.state(page);
  assert.match(s.status, /^HD pack loaded: 1 of its 1 tiles, 0 of its 0 portraits and 0 of its 0 panel pictures match the game on screen; everything else is drawn by MMPX\. It is kept in this browser for the next visit\.$/);
  assert.deepEqual(await packShown(page), { pack: '1,0,0', matched: '1,0,0', forgetEnabled: true });
  assert.ok(s.state === 'running' && await page.evaluate(() => globalThis.__shownGame === window.__g0), 'the same game runs');
  const log = (await h.log(page)).slice(from);
  assert.ok(log.includes('pack:store:end:true'), 'the pack is stored');
  assert.ok(!log.some((e) => e.startsWith('save:') || e.startsWith('saves:')), 'no game file or save touched');
  h.assertSame(await h.keptPaths(page), keptBefore, 'the kept game files are as they were');
  assert.ok(await h.keptPackBytes(page) > 0, 'the pack is kept');
  await page.reload();
  // Live display again: S 4 and S 2 show the pack.
  s = await started(page, FILES);
  await h.waitPrompt(page);
  assert.equal(await page.evaluate(() => window.__icon()), true);
  const reloaded = await h.log(page);
  const packAt = reloaded.indexOf('pack:load:1');
  assert.ok(packAt >= 0 && packAt < reloaded.indexOf('load:start'), 'the pack is loaded before the game is started');
  assert.deepEqual(await packShown(page), { pack: '1,0,0', matched: '1,0,0', forgetEnabled: true });
  assert.ok(await cellShowsPack(page), 'after the reload: the pack image at S 4 and S 2');
  assert.ok(await canvasShowsGame(page), 'the canvas is the game\'s HD frame');
  // Forget: MMPX again at once, storage empty, and after a reload too.
  await h.virtualDisplay(page, 60);
  await page.click('#forget-hd');
  await page.waitForFunction(() => document.body.dataset.ops === '0' && document.body.dataset.hdPack === 'none', null, { timeout: 30_000, polling: 50 });
  assert.equal(await page.evaluate(() => window.__cellHash()), mmpx[4], 'S 4: MMPX again, at once');
  assert.match((await h.state(page)).status, /^The HD pack was removed from this page and this browser; HD shows MMPX\.$/);
  assert.equal(await h.keptPackBytes(page), null, 'nothing stored');
  await page.reload();
  await started(page, FILES);
  await h.waitPrompt(page);
  assert.equal(await page.evaluate(() => window.__icon()), true);
  assert.deepEqual(await packShown(page), { pack: 'none', matched: '', forgetEnabled: true });
  assert.deepEqual(await cellHashes(page), mmpx, 'after the reload: MMPX at S 2 and S 4');
  assert.ok((await h.log(page)).includes('pack:load:0'), 'no pack loaded');
  assert.deepEqual(await page.evaluate(() => window.__csp), [], 'no CSP violation');
  assert.equal(foreign.length, 0, 'no request to another origin');
}, { needs: h.haveHfes });

test('HD pack: a broken pack (a CRC-32, a picture\'s size, a file it does not have, an index over the limit) is refused with nothing changed; a zip whose manifest does not read or parse goes to the import, which starts and stores nothing, the game, its kept files and the kept pack as they were; a drop of a pack with a game file, or of two packs, is refused; another pack replaces the first', async (env) => {
  const foreign = [];
  const page = await bootPack(env, foreign);
  assert.equal(await page.evaluate(() => window.__icon()), true);
  await page.evaluate(() => window.__dropPack(['good']));
  await page.waitForFunction(() => document.body.dataset.ops === '0' && document.body.dataset.hdPack === '1,0,0', null, { timeout: 30_000 });
  const keptPack = await h.keptPackBytes(page);
  const keptFiles = await kept(page, FILES, 'the game files are kept');
  await page.evaluate(() => { window.__g0 = globalThis.__shownGame; });
  const unchanged = async (what) => {
    assert.deepEqual(await packShown(page), { pack: '1,0,0', matched: '1,0,0', forgetEnabled: true }, `${what}: the pack loaded`);
    assert.equal(await h.keptPackBytes(page), keptPack, `${what}: the pack kept`);
    h.assertSame(await h.keptPaths(page), keptFiles, `${what}: the kept game files`);
    assert.ok(await page.evaluate(() => globalThis.__shownGame === window.__g0), `${what}: the same game`);
    assert.equal((await h.state(page)).state, 'running', `${what}: it runs`);
    assert.ok(await cellShowsPack(page), `${what}: the pack's image still shown`);
  };
  const REFUSED = {
    crc: /^The HD pack was not loaded: it does not read as a zip \(its directory, or an entry's size or CRC-32, is wrong\)\.$/,
    size: /^The HD pack was not loaded: a picture is not a lossless WebP of the size its index gives it\.$/,
    entry: /^The HD pack was not loaded: it holds a file an HD pack does not have, or one twice\.$/,
    index: /^The HD pack was not loaded: it is over the format's limits \(4096 images, 150 MB decoded\)\.$/,
  };
  // The entry a pack does not have is seen in the directory: refused before the file is read
  // whole; the others need the reader.
  const WHOLE = { crc: true, size: true, entry: false, index: true };
  for (const [variant, why] of Object.entries(REFUSED)) {
    const from = (await h.log(page)).length;
    await page.evaluate((v) => window.__dropPack([v]), variant);
    await h.waitSettled(page);
    assert.match((await h.state(page)).status, why, `${variant}: refused`);
    assert.equal((await h.log(page)).slice(from).includes('read:start:test-pack-0.zip'), WHOLE[variant], `${variant}: read whole or not`);
    assert.ok(!(await h.log(page)).slice(from).some((e) => e.startsWith('pack:') || e.startsWith('save:')), `${variant}: no storage call`);
    await unchanged(variant);
  }
  // Zips whose manifest.json does not read or parse are not packs (spec 07 H2's rule): a pack
  // whose manifest's CRC-32 is wrong, and a disk image beside a manifest cut short. The import
  // takes them, starts nothing from them (no game files in them) and stores nothing; the game
  // on screen, its kept files and the kept pack stay as they were.
  for (const variant of ['manifest', 'claim']) {
    const from = (await h.log(page)).length;
    await page.evaluate((v) => window.__dropPack([v]), variant);
    await h.waitSettled(page);
    assert.match((await h.state(page)).status, /^Could not start the game:/, `${variant}: the import refused it`);
    const log = (await h.log(page)).slice(from);
    assert.ok(log.some((e) => e.startsWith('read:start:')), `${variant}: the import read it`);
    assert.ok(!log.some((e) => e.startsWith('pack:') || e.startsWith('save:') || e.startsWith('saves:store')), `${variant}: nothing stored`);
    await unchanged(variant);
  }
  for (const [variants, why] of [
    [['good', 'marker'], /^The drop holds an HD pack and other files: drop the HD pack on its own\. Nothing was loaded\.$/],
    [['other', 'good'], /^The drop holds more than one HD pack: drop one at a time\. Nothing was loaded\.$/],
  ]) {
    const from = (await h.log(page)).length;
    await page.evaluate((v) => window.__dropPack(v), variants);
    await h.waitSettled(page);
    assert.match((await h.state(page)).status, why, `${variants}: refused`);
    const log = (await h.log(page)).slice(from);
    assert.ok(!log.some((e) => e.startsWith('pack:') || e.startsWith('save:') || e === 'read:start:port-test-marker.txt'), `${variants}: nothing read past the classifier, nothing stored`);
    await unchanged(variants.join('+'));
  }
  // Another pack replaces the first, in the page and in storage.
  await page.evaluate(() => window.__dropPack(['other']));
  await h.waitSettled(page);
  assert.match((await h.state(page)).status, /^HD pack loaded: 1 of its 1 tiles/);
  assert.ok(await cellShowsPack(page, 'other'), 'the second pack\'s image');
  assert.ok(await h.keptPackBytes(page) > 0);
  await page.reload();
  await started(page, FILES);
  await h.waitPrompt(page);
  assert.equal(await page.evaluate(() => window.__icon()), true);
  assert.ok(await cellShowsPack(page, 'other'), 'the second pack is the one kept');
  assert.equal(foreign.length, 0, 'no request to another origin');
}, { needs: h.haveHfes });

// Page init script: every IndexedDB open throws (a browser that refuses the storage).
function indexedDbThrows() {
  IDBFactory.prototype.open = () => {
    throw new DOMException('test: storage refused', 'SecurityError');
  };
}

test('HD pack with storage that throws: the pack is loaded and shown for this visit, the status says it is not kept; after a reload there is none; Forget the HD pack says the browser did not remove it; no page error', async (env) => {
  const foreign = [];
  const errors = [];
  const page = await bootPack(env, foreign, indexedDbThrows);
  page.on('pageerror', () => errors.push(1));
  assert.equal(await page.evaluate(() => window.__icon()), true);
  const mmpx = await cellHashes(page);
  await page.evaluate(() => window.__dropPack(['good']));
  await page.waitForFunction(() => document.body.dataset.ops === '0' && document.body.dataset.hdPack === '1,0,0', null, { timeout: 30_000 });
  // The storage that refuses the pack refuses the removal of an older one too: the status says
  // what may stay.
  assert.match((await h.state(page)).status, /^HD pack loaded: 1 of its 1 tiles.* \(It could not be kept in this browser: it works until the page is left, and an HD pack kept here before, if any, stays and comes back on the next visit\.\)$/);
  assert.ok(await cellShowsPack(page), 'shown for this visit');
  await page.reload();
  await h.waitSettled(page);
  assert.deepEqual(await packShown(page), { pack: 'none', matched: '', forgetEnabled: true }, 'nothing kept');
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  assert.equal(await page.evaluate(() => window.__icon()), true);
  assert.deepEqual(await cellHashes(page), mmpx, 'MMPX after the reload');
  await page.evaluate(() => window.__dropPack(['good']));
  await page.waitForFunction(() => document.body.dataset.ops === '0' && document.body.dataset.hdPack === '1,0,0', null, { timeout: 30_000 });
  await page.click('#forget-hd');
  await h.waitSettled(page);
  assert.match((await h.state(page)).status, /^The HD pack is off on this page, but this browser did not remove it: /);
  assert.deepEqual(await packShown(page), { pack: 'none', matched: '', forgetEnabled: true });
  assert.deepEqual(await cellHashes(page), mmpx, 'MMPX again');
  assert.equal(errors.length, 0, 'no page error');
  assert.equal(foreign.length, 0, 'no request to another origin');
}, { needs: h.haveHfes });

// Page init script: a put of the HD pack's record fails as a full disk does, while
// `window.__quota` says how: 'throw' (the put throws at once) or 'abort' (the put's request is
// made and its transaction then aborts: the request's error and the abort come later); the rest
// of the storage works.
function packQuota() {
  const put = IDBObjectStore.prototype.put;
  IDBObjectStore.prototype.put = function quotaPut(value, key) {
    const pack = key === 'hd:pack';
    if (pack && window.__quota === 'throw') throw new DOMException('test: quota', 'QuotaExceededError');
    const req = put.call(this, value, key);
    if (pack && window.__quota === 'abort') this.transaction.abort();
    return req;
  };
}

test('HD pack: a new pack the browser cannot keep (its put throws at once, or its transaction aborts later: a full disk) works for this visit, and the pack kept before is removed, not brought back on the next visit', async (env) => {
  const foreign = [];
  const page = await bootPack(env, foreign, packQuota);
  assert.equal(await page.evaluate(() => window.__icon()), true);
  const mmpx = await cellHashes(page);
  for (const mode of ['throw', 'abort']) {
    await page.evaluate(() => window.__dropPack(['good']));
    await page.waitForFunction(() => document.body.dataset.ops === '0' && /^HD pack loaded: /.test(document.getElementById('status').textContent), null, { timeout: 30_000 });
    assert.ok(await h.keptPackBytes(page) > 0, `${mode}: the first pack is kept`);
    await page.evaluate((m) => { window.__quota = m; }, mode);
    await page.evaluate(() => window.__dropPack(['other']));
    await h.waitSettled(page);
    assert.match((await h.state(page)).status, /^HD pack loaded: 1 of its 1 tiles.* \(It could not be kept in this browser: it works until the page is left, and no HD pack is kept for the next visit\.\)$/, `${mode}: the status`);
    assert.ok(await cellShowsPack(page, 'other'), `${mode}: the new pack shows for this visit`);
    assert.equal(await h.keptPackBytes(page), null, `${mode}: the pack kept before is removed`);
    await page.reload();
    await started(page, FILES);
    await h.waitPrompt(page);
    assert.equal(await page.evaluate(() => window.__icon()), true);
    assert.deepEqual(await packShown(page), { pack: 'none', matched: '', forgetEnabled: true }, `${mode}: no pack comes back`);
    assert.deepEqual(await cellHashes(page), mmpx, `${mode}: MMPX after the reload`);
  }
  assert.equal(foreign.length, 0, 'no request to another origin');
}, { needs: h.haveHfes });

test('HD pack: a zip of the owner\'s disk images with a manifest.json that mentions the pack format without declaring it (no format) starts the game as before; that manifest on its own goes to the import too, not refused as a pack', async (env) => {
  const foreign = [];
  const { context, page } = await h.openPage(env);
  context.on('request', (r) => {
    const url = r.url();
    if (!url.startsWith(env.server.url) && !url.startsWith('data:') && !url.startsWith('blob:')) foreign.push(url);
  });
  await context.addInitScript(h.packKit);
  await h.load(page, env);
  await page.evaluate(() => {
    const input = Object.assign(document.createElement('input'), { type: 'file', id: '__disks', multiple: true });
    input.style.display = 'none';
    document.body.append(input);
  });
  await page.setInputFiles('#__disks', h.HFES);
  const MENTION = '{"name": "game", "description": "supports u6-hdpack/1"}';
  const dropZip = (withDisks) => page.evaluate(async ([disks, manifest]) => {
    const files = [];
    if (disks) {
      for (const f of document.getElementById('__disks').files) files.push([f.name, new Uint8Array(await f.arrayBuffer())]);
    }
    files.push(['manifest.json', new TextEncoder().encode(manifest)]);
    const dt = new DataTransfer();
    dt.items.add(new File([window.__zipFiles(files)], disks ? 'disks.zip' : 'notes.zip'));
    document.getElementById('drop').dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }));
  }, [withDisks, MENTION]);
  await dropZip(true);
  const s = await started(page, FILES);
  assert.match(s.status, /Started\./, 'the game started from the zip');
  assert.deepEqual(await packShown(page), { pack: 'none', matched: '', forgetEnabled: true }, 'no pack');
  await page.evaluate(() => { window.__g0 = globalThis.__shownGame; });
  await dropZip(false);
  await h.waitSettled(page);
  assert.match((await h.state(page)).status, /^Could not start the game:/, 'on its own: imported (no game files in it)');
  assert.ok(await page.evaluate(() => globalThis.__shownGame === window.__g0), 'the game on screen runs on');
  assert.deepEqual(await packShown(page), { pack: 'none', matched: '', forgetEnabled: true }, 'no pack');
  assert.equal(foreign.length, 0, 'no request to another origin');
}, { needs: h.haveHfes });

test('HD pack: a pack whose manifest does not read (its CRC-32) dropped on a page with no game is not taken for a pack: the import refuses it, nothing is started, kept or stored', async (env) => {
  const foreign = [];
  const { context, page } = await h.openPage(env);
  context.on('request', (r) => {
    const url = r.url();
    if (!url.startsWith(env.server.url) && !url.startsWith('data:') && !url.startsWith('blob:')) foreign.push(url);
  });
  await context.addInitScript(h.packKit);
  await context.addInitScript(hdPackPage);
  await h.load(page, env);
  const from = (await h.log(page)).length;
  await page.evaluate(async () => {
    const dt = new DataTransfer();
    dt.items.add(new File([await window.__brokenPack()], 'broken-pack.zip'));
    document.getElementById('drop').dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }));
  });
  await h.waitSettled(page);
  const s = await h.state(page);
  assert.match(s.status, /^Could not start the game:/, 'the import refused it');
  assert.equal(s.state, 'idle', 'nothing started');
  assert.deepEqual(await packShown(page), { pack: 'none', matched: '', forgetEnabled: true }, 'no pack');
  assert.equal(await h.keptPackBytes(page), null, 'no pack kept');
  assert.equal(await h.keptPaths(page), null, 'no game files kept');
  const log = (await h.log(page)).slice(from);
  assert.ok(log.includes('read:start:broken-pack.zip'), 'the import read it');
  assert.ok(!log.some((e) => e.startsWith('pack:') || e.startsWith('save:') || e.startsWith('saves:store')), 'nothing stored');
  assert.equal(foreign.length, 0, 'no request to another origin');
});

test('HD pack on a page with no game: the drop loads it and starts nothing (no import, no game files kept); the owner\'s files dropped next start a game that shows it, and a reload brings both back', async (env) => {
  // The pack is made on another page whose game computes the record's key; only its bytes (the
  // test's own pictures and the key) pass to the cold page.
  const foreign = [];
  const maker = await bootPack(env, foreign);
  assert.equal(await maker.evaluate(() => window.__icon()), true);
  const bytes = await maker.evaluate(async () => [...await window.__packTile('good')]);
  await maker.context().close();
  const { context, page } = await h.openPage(env);
  context.on('request', (r) => {
    const url = r.url();
    if (!url.startsWith(env.server.url) && !url.startsWith('data:') && !url.startsWith('blob:')) foreign.push(url);
  });
  await context.addInitScript(h.packKit);
  await context.addInitScript(hdPackPage);
  await h.load(page, env);
  assert.equal((await h.state(page)).state, 'idle');
  await h.dropFiles(page, [], [{ name: 'cold-pack.zip', bytes }]);
  await page.waitForFunction(() => document.body.dataset.ops === '0' && document.body.dataset.hdPack === '1,0,0', null, { timeout: 30_000 });
  const s = await h.state(page);
  assert.match(s.status, /^HD pack loaded: 1 tiles, 0 portraits and 0 panel pictures; it is used once a game runs\. It is kept in this browser for the next visit\.$/);
  assert.equal(s.state, 'idle', 'no game started');
  assert.equal(await h.keptPaths(page), null, 'no game files kept');
  assert.ok(!(await h.log(page)).some((e) => e.startsWith('save:')), 'no import ran');
  assert.ok(await h.keptPackBytes(page) > 0, 'the pack is kept');
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  assert.equal(await page.evaluate(() => window.__icon()), true);
  assert.deepEqual(await packShown(page), { pack: '1,0,0', matched: '1,0,0', forgetEnabled: true });
  assert.ok(await cellShowsPack(page), 'the game started after shows the pack');
  await page.reload();
  await started(page, FILES);
  await h.waitPrompt(page);
  assert.equal(await page.evaluate(() => window.__icon()), true);
  assert.ok(await cellShowsPack(page), 'after the reload: the kept files and the kept pack');
  assert.equal(foreign.length, 0, 'no request to another origin');
}, { needs: h.haveHfes });

// The owner's full-size pack for spec 07 Acceptance 5: every classic image of the owner's files
// as MMPX ×4 (so every image the game draws is a pack hit), written outside the repository by
//   scripts/cargo.sh run -p u6-hdtool --release --example scaler_pack -- --game <the zip> --out <this>
// The test skips without it.
const SCALER_PACK = join(h.ORIGINAL, '..', 'hd', 'scaler', 'u6-hdpack-scaler.zip');
const havePack = () => h.haveZip() && existsSync(SCALER_PACK);
// The wasm memory budget with a full pack (spec 07 H2, our bound, not a value of the game's
// files): the zip-start bound of a session without a pack, plus the pack's decoded pictures, plus
// one atlas decoded while it loads (2048 x 2048 RGBA) and the pack file copied in for the load.
const ATLAS_MIB = (2048 * 2048 * 4) / 2 ** 20;

// Drops `path` on the page and waits for the status of the pack load; returns its wall ms.
async function dropPackFile(page, path) {
  const t0 = Date.now();
  await page.evaluate(() => { document.getElementById('status').textContent = ''; });
  await h.dropFiles(page, [path]);
  await page.waitForFunction(() => document.body.dataset.ops === '0' && /^HD pack loaded: /.test(document.getElementById('status').textContent), null, { timeout: 120_000 });
  return Date.now() - t0;
}

test('HD performance with a full pack (spec 07 Acceptance 5): the owner\'s scaler pack (every record, every portrait and both panel pictures) dropped on a game started from the zip, every portrait and panel picture and nearly every tile matched, the frames composed from it; a walk in the castle at 4x on the real display, the present and the canvas draw per display frame at S 4 and S 2, all frames and the drawn ones: median at most 4 ms, p95 at most 8 ms; the load time and the wasm memory within the budget, and again after the same pack is dropped a second time (both held until the second is checked) (receipt)', async (env) => {
  const { context, page } = await h.openPage(env);
  await context.addInitScript(timeHdFrames);
  await h.load(page, env);
  await h.dropFiles(page, [h.ZIP]);
  await started(page);
  await h.waitPrompt(page);
  await h.waitFrames(page, 30);
  const before = await page.evaluate(() => __u6mem());
  const file = (await stat(SCALER_PACK)).size;
  const made = () => page.evaluate(() => globalThis.__shownGame.hd_cache_counts());
  const made0 = await made();
  const loadMs = await dropPackFile(page, SCALER_PACK);
  const after = await page.evaluate(() => __u6mem());
  const shown = await packShown(page);
  const pack = shown.pack.split(',').map(Number);
  const matched = shown.matched.split(',').map(Number);
  // The fixture is complete: every record (the format's 2048), every portrait the game's
  // archives hold (the engine's count), both panel pictures.
  const portraits = await page.evaluate(() => globalThis.__shownGame.hd_portraits());
  assert.ok(pack[0] === 2048 && portraits > 0 && pack[1] === portraits && pack[2] === 2, 'the scaler pack holds every image');
  assert.ok(matched[0] >= 0.9 * pack[0], 'nearly every tile matches (the merged shore records do not)');
  assert.ok(matched[1] === pack[1] && matched[2] === pack[2], 'every portrait and panel picture matches');
  // The frame composed at the load is the pack's: its images made from the pack; MMPX only for
  // what the pack cannot match (a merged shore record in a state not cached yet: the game runs
  // on while the pack loads).
  const made1 = await made();
  assert.ok(made1[1] - made0[1] > 0, 'HD images made from the pack');
  assert.ok(made1[1] - made0[1] > made1[0] - made0[0], 'more of them from the pack than by MMPX');
  await page.focus('#screen');
  await speedTo(page, '4');
  await engineSpeed(page, 4);
  const receipts = [];
  for (const [s, width] of [[4, 1024], [2, 700]]) {
    await page.setViewportSize({ width, height: 900 });
    await page.waitForFunction((w) => document.getElementById('screen').width === w, 320 * s, { timeout: 10_000 });
    await h.waitFrames(page, 20);
    const at0 = await made();
    await page.evaluate(() => { window.__hdTimes = []; });
    const end = Date.now() + 8000;
    // As the H1 baseline walks: one step after another until the time is up (h.stepOnce, no
    // frame read before each try inside the timed window).
    let walked = 0;
    while (Date.now() < end) {
      await h.stepOnce(page);
      walked += 1;
    }
    assert.ok(walked >= 2, `S ${s}: the party walked during the timed frames`);
    const times = await page.evaluate(() => {
      const t = window.__hdTimes;
      window.__hdTimes = null;
      return t;
    });
    const at1 = await made();
    const all = times.map((f) => f[0]).sort((a, b) => a - b);
    const drawn = times.filter((f) => f[1] > 0).map((f) => f[0]).sort((a, b) => a - b);
    const hz = (times.length - 1) / ((times.at(-1)[2] - times[0][2]) / 1000);
    const [median, p95] = [quantile(all, 0.5), quantile(all, 0.95)];
    const [drawnMedian, drawnP95] = [quantile(drawn, 0.5), quantile(drawn, 0.95)];
    const fromPack = at1[1] - at0[1];
    const byMmpx = at1[0] - at0[0];
    const share = fromPack + byMmpx > 0 ? (100 * fromPack) / (fromPack + byMmpx) : 100;
    receipts.push(`full pack, S ${s}: ${times.length} frames at ${hz.toFixed(0)} Hz, ${drawn.length} drawn, ${walked} steps walked; per frame median ${median.toFixed(2)} ms, p95 ${p95.toFixed(2)} ms, max ${all.at(-1).toFixed(2)} ms; drawn frames median ${drawnMedian.toFixed(2)} ms, p95 ${drawnP95.toFixed(2)} ms; HD images made during the walk: ${Math.round(share)}% from the pack`);
    assert.ok(drawn.length >= 30, `S ${s}: the walk changed the frame`);
    assert.ok(share >= 95, `S ${s}: the walk's HD images are the pack's`);
    assert.ok(median <= 4 && drawnMedian <= 4, `S ${s}: median at most 4 ms, all frames and drawn ones`);
    assert.ok(p95 <= 8 && drawnP95 <= 8, `S ${s}: p95 at most 8 ms, all frames and drawn ones`);
  }
  const walked = await page.evaluate(() => __u6mem());
  const decoded = await packBytes(page);
  const limit = (ZIP_START_MEMORY_MIB + ATLAS_MIB) * 2 ** 20 + decoded + file;
  // The same pack dropped again: it is checked and decoded while the first is set (a refusal
  // must change nothing), so both are held at once for a moment.
  const againMs = await dropPackFile(page, SCALER_PACK);
  await h.waitFrames(page, 10);
  const again = await page.evaluate(() => __u6mem());
  const limitAgain = limit + decoded;
  console.log(`  receipt: the full pack loaded in ${loadMs} ms (drop to status: read, checked, decoded, set, stored), again in ${againMs} ms; matched: tiles ${Math.round((100 * matched[0]) / pack[0])}%, portraits ${Math.round((100 * matched[1]) / pack[1])}%, panel pictures ${Math.round((100 * matched[2]) / pack[2])}%`);
  console.log(`  receipt: wasm memory ${MiB(before)} MiB after the zip start, ${MiB(after)} MiB after the pack load, ${MiB(walked)} MiB after the walks (budget ${MiB(limit)} MiB: the zip-start bound + the pack's pictures decoded + one atlas + the file), ${MiB(again)} MiB after the second load (budget ${MiB(limitAgain)} MiB: and a second pack decoded)`);
  for (const r of receipts) console.log(`  receipt: ${r}`);
  assert.ok(walked <= limit, 'the wasm memory within the budget');
  assert.ok(again <= limitAgain, 'after a second load, within its budget');
  // The same pack again; its matches as at the first load (the shore merge rewrote its records
  // in between, so the tiles matched are not compared exactly).
  const second = await packShown(page);
  assert.ok(second.pack === shown.pack && second.forgetEnabled === shown.forgetEnabled, 'the second load: the same pack');
  const matchedAgain = second.matched.split(',').map(Number);
  assert.ok(matchedAgain[0] >= 0.9 * pack[0] && matchedAgain[1] === pack[1] && matchedAgain[2] === pack[2], 'the second load: matched as the first');
}, { needs: havePack });

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

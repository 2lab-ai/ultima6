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
// pack dropped, shown, kept, forgotten and refused, its performance with a full pack), and the
// first-run chooser (the page's language, a pack the site lists, NPC voices), the page itself
// (one toolbar, the drop line, Settings' "HD on" and forget buttons, the controls, the notices)
// and fullscreen, and the HD layout (spec 10: the 1280 x 720 canvas composed from the x4 frame,
// the game's paper round its regions, clicks through it, the letterbox, fullscreen at any size,
// the setting) and the HD font on it
// (spec 11: the font file, the text drawn in runs over the listed cells, the pixel font as before,
// the choice), and the Wide layout (spec 13: the Wide build site/pkg-wide on a fresh page, the
// map covering the 1920 x 1080 canvas, the framed windows over it, the pointer through them, the
// letterbox, fullscreen, the compositor's time and scale, the setting across the builds; spec 14:
// the actor windows, the active one marked, the side panel's own window in another mode or a
// conversation), the remembered sight (spec 18: on in the page, off in the harness, grey on
// the canvas where the engine's frame remembers a cell out of sight), and the system's pointer
// (hidden over the canvas while a game runs where the engine's sprite is under the mouse, in
// every layout, windowed and fullscreen; kept in the launcher, with the engine's pointer hidden,
// through a press the context menu takes, over the speed badge, in the layouts' gaps, after a
// context-menu entry or a touch moved the game's pointer and once the game stopped; right again
// under a still mouse).
// Most tests start `game` at once (harness `gameFirst`); the launcher tests run the
// page as it is. Every test but the HD layout's, the HD font's and the Wide layout's runs on the
// Original layout of the Original build (harness `openPage`); the HD layout's and the Wide
// layout's keep the pixel font (`bootLayout`, `bootWide`, on harness `openWide`).
// Gates hold file reads and storage calls at chosen points, so every interleaving is forced,
// not timed. The game is read once it reached what a check asserts, by its own signals (the
// prompt, the key queue, the program, its ticks: the page's fixed steps of 20 ms), never after a
// fixed sleep: a loaded host draws fewer frames per second, a frame moves the game at most
// 250 ms (the pace's cap) and a host past its frame budget drops steps, so a sleep there
// covers less of the game. Tests that need the user's disks skip when they are absent.
//
//   node tools/web/test-web.mjs [name-filter]      (build first: bash scripts/build-web.sh)
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, realpathSync } from 'node:fs';
import { copyFile, mkdir, mkdtemp, readFile, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
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
// (a count of them), and the sizes of anything made from them (an export, the bundles, a pack's
// entries or bytes, a share of them) are compared, never printed. A failure shows its label or
// kind and its places in these files, never its message (the runner, h.failureLines).

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

// Presses one of the forget buttons: they sit in Settings, in its "Kept in this browser" group
// (closed by default): Settings opened first when it is not, the group opened.
async function forgetVia(page, id) {
  if (await page.evaluate(() => document.getElementById('setup').hidden)) await page.click('#settings');
  await page.evaluate(() => { document.getElementById('kept').open = true; });
  await page.click(id);
}

// Switches Settings' "HD on" and closes Settings again (its OK, nothing else chosen).
async function switchHd(page) {
  await page.click('#settings');
  await page.click('#hd');
  await page.click('#setup-ok');
  await h.waitSettled(page);
}

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
  // From the canvas: the toolbar under it, then the drop line's picker.
  const order = [];
  for (let i = 0; i < 8 && order.at(-1) !== 'picker'; i += 1) {
    await page.keyboard.press('Tab');
    order.push((await h.state(page)).focused);
  }
  assert.deepEqual(order, ['fullscreen', 'mute', 'voices', 'slower', 'faster', 'settings', 'picker'], 'the toolbar, then the picker');
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
  await forgetVia(page, '#forget');
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
  await forgetVia(page, '#forget');
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
  await forgetVia(page, '#forget-save');
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
  await forgetVia(page, '#forget-save');
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
  await forgetVia(page, '#forget-save');
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
  await forgetVia(page, '#forget');
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
  await forgetVia(page, '#forget-save');
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

// Spec 17: where the frames of the open context menu go (Original, S 4, and at phone width),
// for the dispatcher to read: the directory U6_FRAMES_DIR names, read at each call; unset, no
// frame is written and the checks still run. The frames show the user's game text, so a
// directory inside the repository or inside its main checkout (a linked worktree lies in it),
// resolved through the links of the part that exists, is refused before any screenshot: the test
// fails and nothing is made. Nothing of a frame is ever printed.
// `dir` absolute, its existing part resolved through its links (the rest as named).
function realDir(dir) {
  let at = resolve(dir);
  const rest = [];
  while (!existsSync(at) && dirname(at) !== at) {
    rest.unshift(basename(at));
    at = dirname(at);
  }
  return join(realpathSync(at), ...rest);
}
// The main checkout's root: the parent of git's common directory (the repository itself when
// it is the main checkout); null when git cannot say (it fails, or gives no existing absolute
// path), and then every use of it fails (the guard would check the worktree alone without saying
// so).
const MAIN_ROOT = (() => {
  try {
    const common = execFileSync('git', ['-C', h.REPO, 'rev-parse', '--path-format=absolute', '--git-common-dir'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
    return isAbsolute(common) && existsSync(common) ? dirname(common) : null;
  } catch {
    return null;
  }
})();
// MAIN_ROOT, or a failure saying git could not give it.
function mainRoot() {
  assert.ok(MAIN_ROOT !== null && existsSync(MAIN_ROOT), 'the main checkout root (git rev-parse --git-common-dir) known');
  return MAIN_ROOT;
}
// `dir` is the repository or the main checkout, or lies inside one of them.
function insideRepo(dir) {
  const real = realDir(dir);
  return [h.REPO, mainRoot()].some((root) => {
    const rel = relative(realpathSync(root), real);
    return !(rel === '..' || rel.startsWith(`..${sep}`) || isAbsolute(rel));
  });
}
// Writes `locator`'s screenshot as `name` in `dir` (U6_FRAMES_DIR now by default); true when
// written, false with no directory.
async function keepFrame(locator, name, dir = process.env.U6_FRAMES_DIR ?? null) {
  if (!dir) return false;
  assert.ok(!insideRepo(dir), 'the frames directory outside the repository and its main checkout');
  const real = realDir(dir);
  const shot = await locator.screenshot();
  await mkdir(real, { recursive: true });
  await writeFile(join(real, name), shot);
  return true;
}
test('the context menu (spec 17): keepFrame refuses a frames directory inside the repository (a child named ..frames too), inside its main checkout or named through a link into it, before any screenshot and making nothing, and writes the frame into one outside it', async () => {
  const shots = { n: 0 };
  const fake = { screenshot: async () => { shots.n += 1; return Buffer.from([1, 2, 3]); } };
  const tag = `u6-frames-${process.pid}`;
  const main = mainRoot();
  console.log(`  receipt: the main checkout is ${main === h.REPO ? 'the repository itself' : 'another directory than the worktree'}`);
  const link = await mkdtemp(join(tmpdir(), 'u6-frames-'));
  try {
    await symlink(h.REPO, join(link, 'in'));
    const refused = [
      ['the repository itself', h.REPO, null],
      ['a directory in it, not made yet', join(h.REPO, 'tools', tag, 'frames'), join(h.REPO, 'tools', tag)],
      ['a child named ..frames', join(h.REPO, `..frames-${tag}`, 'x'), join(h.REPO, `..frames-${tag}`)],
      ['in the main checkout', join(main, tag, 'frames'), join(main, tag)],
      ['a main checkout child named ..frames', join(main, `..frames-${tag}`), join(main, `..frames-${tag}`)],
      ['through a link into it', join(link, 'in', tag), join(h.REPO, tag)],
    ];
    for (const [what, dir, made] of refused) {
      assert.ok(insideRepo(dir), `${what}: inside`);
      await assert.rejects(keepFrame(fake, 'frame.png', dir), { name: 'AssertionError', message: /outside the repository and its main checkout/ }, `${what}: refused by the guard`);
      assert.equal(shots.n, 0, `${what}: no screenshot`);
      if (made) assert.ok(!existsSync(made), `${what}: nothing made`);
    }
    assert.equal(await keepFrame(fake, 'frame.png', null), false, 'no directory: nothing');
    assert.equal(shots.n, 0, 'no directory: no screenshot');
    const out = join(link, 'out', 'frames');
    assert.ok(!insideRepo(out) && !insideRepo(tmpdir()), 'outside');
    assert.equal(await keepFrame(fake, 'frame.png', out), true, 'outside: written');
    assert.equal(shots.n, 1, 'one screenshot');
    assert.deepEqual([...(await readFile(join(out, 'frame.png')))], [1, 2, 3], 'the frame in it');
  } finally {
    await rm(link, { recursive: true, force: true });
  }
});

const USE = 0x88;
const LOOK = 0x84;
// The game pixel at the middle of view cell (col, row) of the Original map window (11 x 11 cells
// of 16 pixels, cell c from x 16c, the window clipped to (8, 8)..(167, 167): the edge cells show
// half); the leader's is (5, 5) at (88, 88). The edge cells' middle is their shown half's.
const viewCell = (col, row) => [Math.min(8 + 16 * col, 164), Math.min(8 + 16 * row, 164)].map((v) => Math.max(v, 12));
const VIEW = Array.from({ length: 121 }, (_, i) => [i % 11, Math.floor(i / 11)]);
// Doors (0x129..0x12c), closed while their frame has bit 2 (a section's `kind`: [type, frame]).
const isClosedDoor = ([type, frame]) => type >= 0x129 && type <= 0x12c && (frame & 4) !== 0;

// Walks the leader by arrows until a closed door in view has its Use enabled (next to the
// leader): toward the nearest closed door seen so far (its map cell kept from when it was in
// view), each step the direction that brings it nearest among the cells not yet stood on, else
// back the way it came; a door given up after 30 steps; south while no door was seen (the start's
// nearest doors the leader can reach lie south of it). Returns its view cell and the door's object.
// The trail it prints is counts only.
async function besideClosedDoor(page) {
  const DIRS = [['ArrowRight', 1, 0], ['ArrowLeft', -1, 0], ['ArrowDown', 0, 1], ['ArrowUp', 0, -1]];
  const seen = new Map();
  const given = new Set();
  const visited = new Set();
  const path = [];
  let goalSteps = 0;
  let goalId = null;
  let steps = 0;
  const leader = async () => (await h.state(page)).leader.split(',').map(Number);
  for (let tries = 0; tries < 120; tries += 1) {
    await h.waitPrompt(page);
    const [lx, ly] = await leader();
    visited.add(`${lx},${ly}`);
    const menus = await h.contextMenus(page, VIEW.map(([c, r]) => viewCell(c, r)));
    for (let i = 0; i < VIEW.length; i += 1) {
      for (const s of menus[i]?.sections ?? []) {
        const use = s.entries.find((e) => e.code === USE);
        if (!s.kind || !isClosedDoor(s.kind) || !use) continue;
        if (use.enabled) {
          console.log(`  receipt: a closed door reached in ${steps} steps, ${seen.size} seen`);
          return { col: VIEW[i][0], row: VIEW[i][1], object: s.object };
        }
        seen.set(s.object, [lx + VIEW[i][0] - 5, ly + VIEW[i][1] - 5]);
      }
    }
    const dist = ([x, y], [gx, gy]) => Math.abs(x - gx) + Math.abs(y - gy);
    const open = [...seen].filter(([id]) => !given.has(id));
    const goal = open.sort((p, q) => dist([lx, ly], p[1]) - dist([lx, ly], q[1]))[0];
    if (goal?.[0] !== goalId) [goalId, goalSteps] = [goal?.[0] ?? null, 0];
    const target = goal ? goal[1] : [lx, ly + 20];
    const order = DIRS.map(([key, dx, dy]) => ({ key, dx, dy, d: dist([lx + dx, ly + dy], target) }))
      .filter((m) => !visited.has(`${lx + m.dx},${ly + m.dy}`))
      .sort((p, q) => p.d - q.d);
    let moved = false;
    for (const m of order) {
      await page.keyboard.press(m.key);
      await h.waitTaken(page);
      await h.waitPrompt(page);
      const [nx, ny] = await leader();
      if (nx !== lx || ny !== ly) {
        path.push(DIRS.find(([k]) => k === m.key));
        moved = true;
        break;
      }
      visited.add(`${lx + m.dx},${ly + m.dy}`);
    }
    if (!moved && path.length > 0) {
      const [key] = path.pop();
      const backKey = { ArrowRight: 'ArrowLeft', ArrowLeft: 'ArrowRight', ArrowDown: 'ArrowUp', ArrowUp: 'ArrowDown' }[key];
      await page.keyboard.press(backKey);
      await h.waitTaken(page);
      await h.waitPrompt(page);
    } else if (!moved) {
      await h.passTurn(page);
    }
    steps += 1;
    goalSteps += 1;
    if (goalId !== null && goalSteps >= 30) given.add(goalId);
  }
  return assert.fail(`no closed door reached (${seen.size} seen, ${given.size} given up)`);
}

// The section of `object` in the menu of view cell (col, row) now.
async function sectionAt(page, col, row, object) {
  const menu = await h.contextMenuAt(page, ...viewCell(col, row));
  return menu?.sections.find((s) => s.object === object) ?? null;
}

// The gated page started on the user's disks, at the prompt, the canvas focused.
async function menuPage(env, viewport = null) {
  const opened = await h.openPage(env); // gated: the glue counts what the game took
  if (viewport) await opened.page.setViewportSize(viewport);
  await h.load(opened.page, env);
  await opened.page.setInputFiles('#picker', h.HFES);
  await started(opened.page, FILES);
  await opened.page.focus('#screen');
  await h.waitPrompt(opened.page);
  return opened;
}

test('the context menu (spec 17): a right press at the prompt opens it with the focus, and held and moved over the map it gives the game no press; Escape and a press outside close it and give the game nothing (no key, no button, no walk); on a closed door beside the leader it shows the door\'s section, and Use chosen there opens the door (its frame, through the engine) (screenshot)', async (env) => {
  const { page } = await menuPage(env);
  const menu = page.locator('#context-menu');
  const buttons = () => page.evaluate(() => globalThis.__buttons.length);
  const sent = () => page.evaluate(() => globalThis.__sent.length);
  const leader = async () => (await h.state(page)).leader;
  const inMenu = () => page.evaluate(() => document.getElementById('context-menu').contains(document.activeElement));

  const [b0, at0] = [await buttons(), await leader()];
  await h.rightPress(page, 88, 88);
  for (const [x, y] of [[104, 88], [120, 104], [56, 72], [88, 120]]) {
    await page.mouse.move(...(await h.screenPoint(page, x, y)), { steps: 3 });
  }
  await page.mouse.up({ button: 'right' });
  await h.waitTicks(page, 20);
  assert.equal(await buttons(), b0, 'the held right button gave the game no press');
  assert.equal(await leader(), at0, 'no walk');
  assert.ok(await menu.isVisible() && await inMenu(), 'open, with the focus');
  assert.ok(await menu.locator('[role="menuitem"]').count() > 0, 'entries');

  // No turn: a turn at the prompt needs an event the game takes (the loop waits in its key read),
  // and none reached it: no key, no button; the hero where he was, the prompt still up.
  const s0 = await sent();
  await page.keyboard.press('Escape');
  await menu.waitFor({ state: 'hidden' });
  await h.waitTicks(page, 30);
  assert.equal(await sent(), s0, 'Escape gave the game no key');
  assert.equal(await buttons(), b0, 'nor a button');
  assert.equal(await leader(), at0, 'the hero where he was');
  await h.waitPrompt(page);
  assert.equal((await h.state(page)).focused, 'screen', 'the canvas has the focus again');

  await h.rightPress(page, 88, 88);
  await page.mouse.up({ button: 'right' });
  const box = await page.locator('#status').boundingBox();
  await page.mouse.click(box.x + 4, box.y + 4);
  await menu.waitFor({ state: 'hidden' });
  await h.waitTicks(page, 10);
  assert.equal(await buttons(), b0, 'the press outside gave the game nothing');

  const door = await besideClosedDoor(page);
  const before = await sectionAt(page, door.col, door.row, door.object);
  await h.rightPress(page, ...viewCell(door.col, door.row));
  await page.mouse.up({ button: 'right' });
  const heads = await menu.locator('.head').allTextContents();
  assert.ok(heads.some((x) => x.length > 0), 'the door\'s section, named');
  const use = menu.locator(`[role="menuitem"][data-code="${USE}"]`);
  assert.equal(await use.count(), 1, 'Use offered');
  assert.equal(await use.getAttribute('aria-disabled'), null, 'and it can run');
  await keepFrame(page.locator('#screen-box'), 'menu.png');
  const s1 = await sent();
  await use.click();
  await menu.waitFor({ state: 'hidden' });
  await h.waitTaken(page);
  await h.waitPrompt(page);
  assert.equal(await sent(), s1, 'no key: the icon and the target, by the mouse');
  const after = await sectionAt(page, door.col, door.row, door.object);
  assert.ok(after && after.kind[0] === before.kind[0], 'the same door');
  assert.equal(after.kind[1], before.kind[1] ^ 4, 'the door opened: its frame lost bit 2');
}, { needs: h.haveHfes });

test('the context menu (spec 17): after a press outside closes it (a press and release off the canvas, as a touch gives them, with no move between), the next left press on the canvas, with no move before it, reaches the game (its press and its release, in that order, and nothing before them)', async (env) => {
  const { page } = await menuPage(env);
  const menu = page.locator('#context-menu');
  // The game's button changes ([buttons, clock] each, harness `__buttons`).
  const changes = () => page.evaluate(() => globalThis.__buttons.map(([b, at]) => [b, at]));
  const before = await changes();
  await h.rightPress(page, 88, 88);
  await page.mouse.up({ button: 'right' });
  // The press outside, on the status line, its release there too; the mouse stays on the canvas.
  await page.evaluate(() => {
    const at = document.getElementById('status');
    const ev = (type, b) => new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 1, buttons: b, button: 0 });
    at.dispatchEvent(ev('pointerdown', 1));
    at.dispatchEvent(ev('pointerup', 0));
  });
  await menu.waitFor({ state: 'hidden' });
  await h.waitTicks(page, 10);
  assert.deepEqual(await changes(), before, 'the right press and the press outside: no button change');
  await page.mouse.down();
  await h.waitTicks(page, 3);
  await page.mouse.up();
  await h.waitTaken(page);
  const added = (await changes()).slice(before.length);
  assert.deepEqual(added.map(([b]) => b), [1, 0], 'the press, then its release, and nothing else');
  assert.ok(added[1][1] >= added[0][1], 'the release after the press');
}, { needs: h.haveHfes });

// The open menu's rect and the visible part of the screen box (its rect within the viewport),
// in CSS pixels, rounded.
const menuRects = (page) => page.evaluate(() => {
  const r = (b) => [b.left, b.top, b.width, b.height].map(Math.round);
  const box = document.getElementById('screen-box').getBoundingClientRect();
  const left = Math.max(box.left, 0);
  const top = Math.max(box.top, 0);
  const right = Math.min(box.right, innerWidth);
  const bottom = Math.min(box.bottom, innerHeight);
  const m = document.getElementById('context-menu');
  return { menu: r(m.getBoundingClientRect()), visible: [left, top, right - left, bottom - top].map(Math.round), viewport: [innerWidth, innerHeight], scrolls: m.scrollHeight > m.clientHeight };
});
const inside = ({ menu: [x, y, w, h], visible: [vx, vy, vw, vh] }) => x >= vx && y >= vy && x + w <= vx + vw && y + h <= vy + vh;

// Of `candidates` ({ n: its rows, row: its view row, ... }) the largest menu, of those the lowest
// cell's: rows first, then the row; null for none.
function largestLowest(candidates) {
  let best = null;
  for (const c of candidates) {
    if (!best || c.n > best.n || (c.n === best.n && c.row > best.row)) best = c;
  }
  return best;
}
// The largest menu (most rows) among the view cells whose screen point lies inside the viewport,
// of those the lowest cell's (where a menu must flip up), and that point.
async function lowestLargestMenu(page) {
  const [width, height] = await page.evaluate(() => [innerWidth, innerHeight]);
  const menus = await h.contextMenus(page, VIEW.map(([c, r]) => viewCell(c, r)));
  const candidates = [];
  for (let i = 0; i < VIEW.length; i += 1) {
    const m = menus[i];
    if (!m) continue;
    const at = viewCell(...VIEW[i]);
    const [sx, sy] = await h.screenPoint(page, ...at);
    if (sx < 4 || sy < 4 || sx >= width - 4 || sy >= height - 4) continue;
    const n = m.sections.reduce((k, x) => k + 1 + x.entries.length, 0);
    candidates.push({ n, row: VIEW[i][1], sections: m.sections.length, at });
  }
  return largestLowest(candidates);
}
test('the context menu (spec 17): the placement checks pick the largest menu, rows first: a higher cell with more rows beats a lower one with fewer, and among equal rows the lowest cell wins', async () => {
  const pick = (cells) => largestLowest(cells.map(([n, row]) => ({ n, row })));
  assert.deepEqual(pick([[5, 9], [8, 2]]), { n: 8, row: 2 }, 'more rows beat a lower cell');
  assert.deepEqual(pick([[8, 2], [5, 9]]), { n: 8, row: 2 }, 'in either order');
  assert.deepEqual(pick([[6, 3], [6, 7], [6, 5]]), { n: 6, row: 7 }, 'equal rows: the lowest cell');
  assert.equal(pick([]), null, 'none');
});

test('the context menu (spec 17): a menu stays inside the part of the screen box the viewport shows, flipped up near its bottom and scrolling inside when taller, at phone width (390 x 844), on a phone held sideways (844 x 390, the box taller than the viewport) and in fullscreen (receipt: each menu rect beside the visible box and the viewport) (screenshot)', async (env) => {
  const { page } = await menuPage(env, { width: 390, height: 844 });
  const menu = page.locator('#context-menu');
  const check = async (where) => {
    const big = await lowestLargestMenu(page);
    assert.ok(big, `${where}: a menu in view`);
    await h.rightPress(page, ...big.at);
    await page.mouse.up({ button: 'right' });
    const rects = await menuRects(page);
    console.log(`  receipt: ${where}: ${big.sections} section(s), ${big.n} rows; menu ${rects.menu} in visible ${rects.visible} (viewport ${rects.viewport}), scrolls ${rects.scrolls ? 1 : 0}`);
    if (where === 'phone') await keepFrame(page.locator('#screen-box'), 'menu-phone.png');
    assert.ok(inside(rects), `${where}: inside the visible screen box`);
    await page.keyboard.press('Escape');
    await menu.waitFor({ state: 'hidden' });
    await h.waitTicks(page, 2);
  };
  await check('phone');
  await page.setViewportSize({ width: 844, height: 390 });
  await h.waitFrames(page, 5);
  await check('phone sideways');
  await page.setViewportSize({ width: 1024, height: 700 });
  await page.click('#fullscreen');
  await page.waitForFunction(() => document.fullscreenElement !== null || document.getElementById('screen-box').classList.contains('full'));
  await h.waitFrames(page, 5);
  await check('fullscreen');
}, { needs: h.haveHfes });

// The rect of the open menu's last entry, as `menuRects` gives the menu's.
const lastEntryRect = (page) => page.evaluate(() => {
  const items = document.querySelectorAll('#context-menu [role="menuitem"]');
  const b = items[items.length - 1].getBoundingClientRect();
  return [b.left, b.top, b.width, b.height].map(Math.round);
});

test('the context menu (spec 17): a menu taller than the part of the screen box the viewport shows scrolls inside it: its list taller than its box, the last entry out of sight until the list is scrolled to its end, then inside the visible box (receipt: the heights)', async (env) => {
  const { page } = await menuPage(env, { width: 844, height: 390 });
  const menu = page.locator('#context-menu');
  for (const height of [220, 180, 150, 120, 100]) {
    await page.setViewportSize({ width: 844, height });
    await page.evaluate(() => document.getElementById('screen-box').scrollIntoView({ block: 'start' }));
    await h.waitFrames(page, 3);
    const big = await lowestLargestMenu(page);
    if (!big) continue;
    await h.rightPress(page, ...big.at);
    await page.mouse.up({ button: 'right' });
    const rects = await menuRects(page);
    const sizes = await page.evaluate(() => {
      const m = document.getElementById('context-menu');
      return [m.scrollHeight, m.clientHeight];
    });
    const unseen = await lastEntryRect(page);
    // Taller by at least its last entry: that entry out of sight until the list scrolls.
    if (sizes[0] <= sizes[1] || inside({ menu: unseen, visible: rects.visible })) {
      await page.keyboard.press('Escape');
      await menu.waitFor({ state: 'hidden' });
      continue;
    }
    await page.evaluate(() => {
      const m = document.getElementById('context-menu');
      m.scrollTop = m.scrollHeight;
    });
    await h.waitFrames(page, 2);
    const seen = await lastEntryRect(page);
    console.log(`  receipt: viewport height ${height}: ${big.n} rows; list ${sizes[0]} in a box of ${sizes[1]}; menu ${rects.menu} in visible ${rects.visible}`);
    assert.ok(inside(rects), 'the menu inside the visible screen box');
    assert.ok(sizes[0] > sizes[1], 'its list taller than its box');
    assert.ok(!inside({ menu: unseen, visible: rects.visible }), 'the last entry out of sight before the scroll');
    assert.ok(inside({ menu: seen, visible: rects.visible }), 'the last entry inside the visible box after it');
    assert.ok(await menu.isVisible(), 'still open');
    return;
  }
  assert.fail('no menu taller than the visible part of the screen box');
}, { needs: h.haveHfes });

test('the context menu (spec 17): where the visible part of the screen box is narrower than the menu\'s 10em minimum, the menu is no wider than that part and stays inside it (receipt: the widths)', async (env) => {
  const { page } = await menuPage(env, { width: 120, height: 600 });
  const menu = page.locator('#context-menu');
  const big = await lowestLargestMenu(page);
  assert.ok(big, 'a menu in view');
  await h.rightPress(page, ...big.at);
  await page.mouse.up({ button: 'right' });
  const rects = await menuRects(page);
  const tenEm = await page.evaluate(() => 10 * parseFloat(getComputedStyle(document.getElementById('context-menu')).fontSize));
  console.log(`  receipt: menu ${rects.menu} in visible ${rects.visible}, 10em ${Math.round(tenEm)}`);
  assert.ok(rects.visible[2] < tenEm, 'the visible part narrower than 10em');
  assert.ok(rects.menu[2] <= rects.visible[2], 'the menu no wider than it');
  assert.ok(inside(rects), 'inside the visible screen box');
  assert.ok(await menu.isVisible(), 'open');
}, { needs: h.haveHfes });

test('the context menu (spec 17): a key that closes it gives the canvas the focus back at the release of that physical key (its code) only: not at the release of another key, at a release of that key giving another character; a blur of the window while that release is awaited forgets it, so a later release takes no focus', async (env) => {
  const { page } = await menuPage(env);
  const menu = page.locator('#context-menu');
  const focused = async () => (await h.state(page)).focused;
  const open = async () => {
    await h.rightPress(page, 88, 88);
    await page.mouse.up({ button: 'right' });
    await page.waitForFunction(() => document.getElementById('context-menu').contains(document.activeElement));
  };
  // A key event made in the page: on the focused element (`at` 'focus') or on the window.
  const key = (type, at, init) => page.evaluate(([type, at, init]) => {
    const to = at === 'window' ? window : document.activeElement;
    to.dispatchEvent(new KeyboardEvent(type, { bubbles: true, cancelable: true, ...init }));
  }, [type, at, init]);
  await open();
  await key('keydown', 'focus', { key: 'Escape', code: 'Escape' });
  await menu.waitFor({ state: 'hidden' });
  assert.notEqual(await focused(), 'screen', 'not at the press');
  await key('keyup', 'window', { key: 'Escape', code: 'KeyQ' });
  assert.notEqual(await focused(), 'screen', 'another key let go: not yet');
  await key('keyup', 'window', { key: 'Unidentified', code: 'Escape' });
  assert.equal(await focused(), 'screen', 'that key let go, another character: the canvas has it');

  await open();
  await key('keydown', 'focus', { key: 'Escape', code: 'Escape' });
  await menu.waitFor({ state: 'hidden' });
  await page.evaluate(() => window.dispatchEvent(new Event('blur')));
  await key('keyup', 'window', { key: 'Escape', code: 'Escape' });
  assert.notEqual(await focused(), 'screen', 'after a blur its release takes no focus');
}, { needs: h.haveHfes });

test('the context menu (spec 17): a key let go just before a right press, with no step of the game between (the release queued), still gives the menu, an entry of it able to run', async (env) => {
  const { page } = await menuPage(env);
  const menu = page.locator('#context-menu');
  await page.keyboard.down('x');
  await h.waitTaken(page);
  await h.waitPrompt(page);
  // The game's steps held off from here: the release and the press meet the same queue.
  await page.evaluate(() => {
    const g = globalThis.__shownGame;
    const [step, up] = [g.step, g.key_up];
    globalThis.__held = true;
    g.step = function (ms) {
      if (!globalThis.__held) step.call(this, ms);
    };
    g.key_up = function (code) {
      globalThis.__releaseQueued = up.call(this, code);
      return globalThis.__releaseQueued;
    };
  });
  const ticks = await page.evaluate(() => globalThis.__ticks);
  await page.keyboard.up('x');
  const at = await h.screenPoint(page, 88, 88);
  await page.mouse.move(...at);
  await page.mouse.down({ button: 'right' });
  await menu.waitFor({ state: 'visible', timeout: 10_000 });
  const seen = await page.evaluate(() => ({
    ticks: globalThis.__ticks,
    queued: globalThis.__releaseQueued,
    runnable: [...document.querySelectorAll('#context-menu [role="menuitem"]')]
      .filter((x) => x.getAttribute('aria-disabled') !== 'true').length,
  }));
  await page.evaluate(() => {
    globalThis.__held = false;
  });
  await page.mouse.up({ button: 'right' });
  console.log(`  receipt: steps between ${seen.ticks - ticks}, release queued ${seen.queued ? 1 : 0}, entries able to run ${seen.runnable}`);
  assert.equal(seen.ticks, ticks, 'no step of the game between the release and the press');
  assert.equal(seen.queued, true, 'the key was held: its release queued');
  assert.ok(seen.runnable > 0, 'the menu shown, an entry able to run');
}, { needs: h.haveHfes });

test('the context menu (spec 17): the underlined command is its first entry: Look on any cell, and Rest and Combat (only there); the engine names it (underlined)', async (env) => {
  const { page } = await menuPage(env);
  const menu = page.locator('#context-menu');
  const iconX = (code) => (code - 0x81) * 16 + 16;
  for (const code of [LOOK, 0x89, 0x8a]) {
    // A right press on the command bar's icon underlines it (0x106506); the bar has no menu.
    await page.mouse.move(...(await h.screenPoint(page, iconX(code), 184)));
    await page.mouse.down({ button: 'right' });
    await page.mouse.up({ button: 'right' });
    await h.waitTaken(page);
    await h.waitPrompt(page);
    assert.equal((await h.contextMenuAt(page, ...viewCell(6, 5)))?.underlined, code, `${code}: underlined`);
    await h.rightPress(page, ...viewCell(6, 5));
    await page.mouse.up({ button: 'right' });
    const first = await menu.locator('[role="menuitem"]').first().getAttribute('data-code');
    assert.equal(Number(first), code, `${code}: the first entry`);
    const all = await menu.locator(`[role="menuitem"][data-code="${code}"]`).count();
    assert.equal(all, 1, `${code}: once`);
    await page.keyboard.press('Escape');
    await menu.waitFor({ state: 'hidden' });
  }
}, { needs: h.haveHfes });

// Spec 19 O1b at the page: `key` held from the prompt on the focused canvas as a keyboard sends
// it (Playwright's keyboard.down again is a keydown with `repeat` true): the press, the first
// repeat 225 ms later, then one every 30 ms (the owner's macOS repeat) until the game ran `ms`
// on its clock since the press; then, by `how`, the keyup on the canvas ('canvas'), the focus
// moved to the Mute button first and the keyup there ('focus': the canvas's blur lets every held
// key go, spec 19 O1c, so the release is the blur), or the window's blur with the key still down
// ('blur': the page lets every held key go; the keyup comes only after `check`, and in any case,
// so it cannot stand in for the blur). The page writes the leader and `data-keys` in its frame
// callback after the steps, and the release comes between frames, so the window's listener
// (capture; a keyup, or a blur of the window itself or of the canvas) reads them as of the last
// step before the release reached the game. The walk: the observed position changes of the
// leader before the release (`held`) and the cells after it (`afterCells`, read every frame
// until the prompt is back), the repeats the canvas saw, the keys queued at the release and
// once the step after it ran. `check(walk)` runs on a walk that moved while held.
async function heldKey(page, key, ms, how, check) {
  await h.waitPrompt(page);
  await page.evaluate(() => {
    const c = document.getElementById('screen');
    const stop = new AbortController();
    const seen = { repeats: 0, leaders: [c.dataset.leader], release: null, stop };
    window.__held = seen;
    const look = () => {
      if (c.dataset.leader !== seen.leaders.at(-1)) seen.leaders.push(c.dataset.leader);
    };
    c.addEventListener('keydown', (e) => {
      if (e.repeat) seen.repeats += 1;
    }, { signal: stop.signal });
    const mark = (e) => {
      if (seen.release !== null || (e.type === 'blur' && e.target !== window && e.target !== c)) return;
      look();
      seen.release = { at: seen.leaders.length, keys: Number(c.dataset.keys), ticks: globalThis.__ticks };
    };
    for (const type of ['keyup', 'blur']) window.addEventListener(type, mark, { signal: stop.signal, capture: true });
    const poll = () => {
      if (window.__held !== seen) return;
      look();
      requestAnimationFrame(poll);
    };
    requestAnimationFrame(poll);
  });
  const clock = () => page.evaluate(() => globalThis.__clock);
  await page.keyboard.down(key);
  const t0 = await clock();
  await h.sleep(225);
  while ((await clock()) - t0 < ms) {
    await page.keyboard.down(key);
    await h.sleep(30);
  }
  if (how === 'focus') await page.evaluate(() => document.getElementById('mute').focus());
  if (how === 'blur') await page.evaluate(() => window.dispatchEvent(new Event('blur')));
  else await page.keyboard.up(key);
  let failed = null;
  try {
    const ticks = await page.evaluate(() => window.__held.release.ticks);
    await page.waitForFunction((t) => globalThis.__ticks > t, ticks, { timeout: 60_000, polling: 'raf' });
    await h.waitFrames(page, 1);
    const keysAfter = Number(await page.evaluate(() => document.getElementById('screen').dataset.keys));
    await h.waitPrompt(page);
    const seen = await page.evaluate(() => {
      const s = window.__held;
      window.__held = null;
      s.stop.abort();
      if (document.getElementById('screen').dataset.leader !== s.leaders.at(-1)) {
        s.leaders.push(document.getElementById('screen').dataset.leader);
      }
      return { repeats: s.repeats, leaders: s.leaders, at: s.release.at, keys: s.release.keys };
    });
    const walk = {
      held: seen.at - 1,
      afterCells: seen.leaders.slice(seen.at),
      repeats: seen.repeats,
      keysAtRelease: seen.keys,
      keysAfter,
    };
    if (walk.held > 0) check(walk);
    return walk;
  } catch (err) {
    failed = err;
    throw err;
  } finally {
    // The key up after a blur, the focus back on the canvas: each runs whatever happened above.
    // A cleanup that fails beside a failure in flight is logged (its kind only) and attached to
    // it, never thrown in its place; alone, it is the failure.
    const cleanups = [];
    if (how === 'blur') cleanups.push(() => page.keyboard.up(key));
    if (how === 'focus') cleanups.push(() => page.focus('#screen'));
    let cleanupError = null;
    for (const cleanup of cleanups) {
      try {
        await cleanup();
      } catch (err) {
        cleanupError ??= err;
      }
    }
    if (cleanupError !== null) {
      if (failed === null) throw cleanupError;
      console.error(`  the ${how} cleanup failed too: ${h.errorName(cleanupError)}`);
      if (failed instanceof Error) failed.cleanupError = cleanupError;
    }
  }
}

// `heldKey` from the prompt in the first direction whose press walks (a foe beside the hero
// blocks a way; as `h.stepOnce`, a pass after a round of four blocked).
async function heldWalkingKey(page, ms, how, check) {
  for (let round = 0; round < h.STEP_ROUNDS; round += 1) {
    if (round > 0) await h.passTurn(page);
    for (const { key } of h.STEPS) {
      const walk = await heldKey(page, key, ms, how, check);
      if (walk.held > 0) return walk;
    }
  }
  assert.fail(`no held arrow walked in ${h.STEP_ROUNDS} rounds of the four directions`);
}

// The start fight as booted: an arrow that walks is held 1 s; the step's turn (the foes' turns
// after it) outlasts the hold, so the repeats wait in the queue when the key is let go. The
// press was taken at the prompt, so all that waits is repeats and the release takes them all
// back: no step after it (the drain alone would give one, the newest repeat's). So when the
// release is heard on the canvas, after the focus moved to another control, and when the window
// lost the focus with no keyup. The fight can still kill the hero in the turns after the release
// (he wakes on `h.WAKE_CELL`, as `h.stepResult` allows), so a position change after it is
// allowed there only. The Rust tests (ui/tests/held_key.rs) pin no step at all after the
// release in the quiet castle.
test('a held arrow on the canvas: the browser\'s repeats wait while the turn runs, and its release takes them back at the next step, so the hero walks no cell after it: released on the canvas, on another control the focus moved to, or by the window\'s blur (spec 19 O1b)', async (env) => {
  const { page } = await h.openPage(env); // gated: the glue keeps the engine clock and its ticks
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  for (const how of ['canvas', 'focus', 'blur']) {
    await heldWalkingKey(page, 1000, how, (walk) => {
      const woke = walk.afterCells.filter((cell) => cell === h.WAKE_CELL).length;
      console.log(`  receipt (${how}): held 1 s, ${walk.repeats} repeats sent; observed position changes ${walk.held} while held, ${walk.afterCells.length} after the release (${woke} onto the wake cell); keys queued ${walk.keysAtRelease} at the release, ${walk.keysAfter} once it was taken`);
      assert.ok(walk.repeats > 0, `${how}: the canvas saw the browser's repeats`);
      assert.ok(walk.keysAtRelease > 0, `${how}: the repeats waited while the turn ran`);
      assert.equal(walk.keysAfter, 0, `${how}: the release took them back`);
      assert.ok(walk.afterCells.every((cell) => cell === h.WAKE_CELL), `${how}: no step after the release (a change only onto the wake cell)`);
    });
  }
}, { needs: h.haveHfes });

// The held walk's minimum interval (spec 19 O1c, `MIN_STEP_MS` in u6-core), in wall ms.
const MIN_STEP_MS = 150;

// Spec 19 O1c (round 4 item 4): watches a held walk at the page through the game's own step()
// and set_clock() (the instance's, so the glue's count still runs) and the canvas's keydown. Per
// step, and once before the first: the engine clock, the leader, whether the prompt is up, the
// page's clock (the frame's time, the timeline of the press's `timeStamp`), the presses the command
// prompt has taken (`prompt_takes`: the instrumentation that says which step took the press) and
// the keys pending. The keydown records the press's `timeStamp` and the index of the first step
// after it (`pressIndex`: every step from it on is after the press, whatever the clocks say).
function watchWalk(page) {
  return page.evaluate(() => {
    const g = globalThis.__shownGame;
    const c = document.getElementById('screen');
    const log = { steps: [], repeats: 0, pressedAt: null, pressIndex: null, now: null, stop: new AbortController() };
    window.__k2 = log;
    c.addEventListener('keydown', (e) => {
      if (e.repeat) log.repeats += 1;
      else if (log.pressIndex === null) {
        log.pressedAt = e.timeStamp;
        log.pressIndex = log.steps.length;
      }
    }, { signal: log.stop.signal });
    g.set_clock = function (now) {
      log.now = now;
      Object.getPrototypeOf(this).set_clock.call(this, now);
    };
    const row = (game) => [globalThis.__clock, game.leader_position().join(','), game.at_prompt(), log.now, game.prompt_takes(), game.keys_pending()];
    g.step = function (ms) {
      Object.getPrototypeOf(this).step.call(this, ms);
      log.steps.push(row(this));
    };
    log.steps.push(row(g));
  });
}

// The log `watchWalk` kept, its hooks taken off.
function unwatchWalk(page) {
  return page.evaluate(() => {
    const k2 = window.__k2;
    window.__k2 = null;
    k2.stop.abort();
    delete globalThis.__shownGame.step;
    delete globalThis.__shownGame.set_clock;
    return { steps: k2.steps, repeats: k2.repeats, pressedAt: k2.pressedAt, pressIndex: k2.pressIndex };
  });
}

// What a watched hold did (`released`: the steps logged when the key went up). The steps of the
// leader while held (each with the step() calls from the prompt coming back to it and the engine
// ms since the step before) and after (the cells they ended on); the repeats the canvas saw; and,
// from the press on (D1', spec 19 O1c), by the instrumentation: the step that took the press
// itself (the first prompt take from `pressIndex` on), then each paced press taken (the page's
// clock, its gap from the take before, and the wait from the prompt coming back), the longest
// frame interval of the hold, and the late steps: one that ran with the prompt up and a paced
// press due (the minimum interval past the press and past the last take) and took nothing,
// with what the step saw (for a failure: the press's time, the frame's, the last take's, the
// prompt before and the keys pending).
function walkReceipt(log, released) {
  const { steps, pressedAt, pressIndex } = log;
  const held = [];
  const after = [];
  let prompt = 0;
  let last = steps[0]?.[0] ?? 0;
  const paced = [];
  const lateAt = [];
  let late = 0;
  let pressTaken = false;
  let lastTake = null;
  let promptBack = null;
  let longestFrame = 0;
  for (let i = 1; i < steps.length; i += 1) {
    const [clock, leader, atPrompt, now, takes, pending] = steps[i];
    const before = steps[i - 1];
    if (leader !== before[1]) {
      if (i < released) held.push({ fromPrompt: i - prompt, ms: clock - last, leader });
      else after.push(leader);
      last = clock;
    }
    if (atPrompt) prompt = i;
    if (pressIndex === null || i < pressIndex || i >= released) continue;
    if (before[3] !== null && now !== before[3]) longestFrame = Math.max(longestFrame, now - before[3]);
    if (takes !== before[4]) {
      // The prompt took a press: the key's own first, then each a paced one.
      if (pressTaken) paced.push({ at: now, gap: now - lastTake, wait: promptBack === null ? 0 : now - promptBack });
      pressTaken = true;
      lastTake = now;
      promptBack = null;
      continue;
    }
    if (!pressTaken) continue;
    if (atPrompt && promptBack === null) promptBack = now;
    if (!before[2] || !atPrompt) continue;
    if (now - pressedAt >= MIN_STEP_MS && now - lastTake >= MIN_STEP_MS) {
      late += 1;
      if (lateAt.length < 3) {
        lateAt.push({ step: i, pressedAt: Math.round(pressedAt), frame: Math.round(now), lastTake: Math.round(lastTake), promptBefore: before[2], pending, pressTaken });
      }
    }
  }
  return { held, after, repeats: log.repeats, pressTaken, paced, late, lateAt, longestFrame };
}

// One line for a receipt of `walkReceipt`.
function walkLine(key, w) {
  const walked = w.held.slice(1).filter((s) => s.leader !== h.WAKE_CELL);
  const gaps = w.paced.map((p) => Math.round(p.gap)).join(', ') || '-';
  return `${key} held, ${w.repeats} browser repeats; ${w.held.length} steps while held, engine ms between them ${walked.map((s) => s.ms).join(', ') || '-'}, step() calls from the prompt to each ${walked.map((s) => s.fromPrompt).join(', ') || '-'}; the press taken: ${w.pressTaken ? 'yes' : 'no'}; ${w.paced.length} paced presses taken, wall ms from the take before ${gaps}; the longest frame ${Math.round(w.longestFrame)} ms; ${w.late} steps left the prompt up with a press due${w.late > 0 ? ` (${JSON.stringify(w.lateAt)})` : ''}; ${w.after.length} position changes after the keyup`;
}

// Spec 19 O1c at the page, on the browser's own clock (no virtual display): `key` pressed once on
// the focused canvas and held with no browser repeat at all (Playwright sends none) until the
// leader stepped three times, or the game ran 30 s on its clock, or 2 s with no step at all (the
// way blocked), then the keyup; `walkReceipt` of it.
async function heldNoRepeat(page, key) {
  await h.waitPrompt(page);
  await watchWalk(page);
  await page.keyboard.down(key);
  const t0 = await page.evaluate(() => globalThis.__clock);
  let released = 0;
  try {
    await page.waitForFunction((from) => {
      const s = window.__k2.steps;
      let n = 0;
      for (let i = 1; i < s.length; i += 1) if (s[i][1] !== s[i - 1][1]) n += 1;
      const ran = globalThis.__clock - from;
      return n >= 3 || ran >= 30_000 || (n === 0 && ran >= 2_000);
    }, t0, { timeout: 120_000, polling: 'raf' });
  } finally {
    released = await page.evaluate(() => window.__k2.steps.length);
    await page.keyboard.up(key);
  }
  await h.waitTaken(page);
  await h.waitPrompt(page);
  return walkReceipt(await unwatchWalk(page), released);
}

// The start fight as booted, on the browser's own clock, arrows (left first) held with no browser
// repeat until one walks twice: at every command prompt the held arrow is pressed again as soon
// as the minimum interval allows (no step runs with the prompt up and a paced press due and leaves
// it up, whether its steps walk or the fight blocks them: D1', the prompt up at most one step in
// a row once due), the press itself is taken on every hold, at least one command prompt over all
// the holds is taken by a paced press (the opportunity: the key down past the interval), the
// paced presses at least the interval apart from the take before, and nothing after the keyup.
// The fight's geometry decides whether any way is open twice (a hero killed wakes on
// `h.WAKE_CELL`, between the same foes, and may be blocked every way, `h.stepOnce`'s note). A
// position change after a keyup is allowed only onto the wake cell.
test('a held arrow with no browser repeat walks on at every command prompt once the minimum interval allows, and stops at its keyup (spec 19 O1c)', async (env) => {
  const { page } = await h.openPage(env); // gated: the glue keeps the engine clock and its ticks
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  const keys = ['ArrowLeft', ...h.STEPS.map((s) => s.key).filter((k) => k !== 'ArrowLeft')];
  const holds = [];
  for (let round = 0; round < h.STEP_ROUNDS && !holds.some((w) => w.held.length >= 2); round += 1) {
    if (round > 0) await h.passTurn(page);
    for (const key of keys) {
      const w = await heldNoRepeat(page, key);
      holds.push({ ...w, key });
      if (w.held.length >= 2) break;
    }
  }
  for (const w of holds) console.log(`  receipt: ${walkLine(w.key, w)}`);
  const taken = holds.reduce((n, w) => n + w.paced.length, 0);
  assert.ok(taken >= 1, `a command prompt taken by a paced press over the holds (${taken})`);
  for (const w of holds) {
    assert.equal(w.repeats, 0, `${w.key}: no browser repeat was sent`);
    assert.ok(w.pressTaken, `${w.key}: the press itself taken`);
    assert.equal(w.late, 0, `${w.key}: pressed again at every prompt once due`);
    assert.ok(w.paced.every((p) => p.gap >= MIN_STEP_MS), `${w.key}: the paced presses the interval after the take before`);
    assert.ok(w.after.every((cell) => cell === h.WAKE_CELL), `${w.key}: no step after the keyup (a change only onto the wake cell)`);
  }
  assert.ok(holds.some((w) => w.held.length >= 1), 'a held arrow walked');
}, { needs: h.haveHfes });

// Spec 19 O1c (round 4 items 6 and 11) at Max on the virtual display (64 steps a frame, a step
// taking no time: the fight's turn ends within a few frames, well inside the minimum interval):
// an arrow (left first) pressed once with no browser repeat and held 90 frames (1.5 s), then let
// up. At least two paced presses are taken, so the interval, not the turn, sets the gaps; each
// is 150 ms after the take before within one frame (both ways: removing the interval makes them
// a turn apart, slowing it a frame or more longer), none is late, and nothing walks after the
// keyup (a change only onto the wake cell).
test('at Max a held arrow walks one step per minimum interval, no sooner and no later (spec 19 O1c)', async (env) => {
  const { page } = await menuPage(env);
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  for (let i = 0; i < 7; i += 1) await page.keyboard.press('Equal');
  assert.equal((await speedShown(page)).speed, 'max', 'seven presets up: Max');
  await h.runFramesToPrompt(page, 5000);
  const frame = 1000 / 60;
  const keys = ['ArrowLeft', ...h.STEPS.map((s) => s.key).filter((k) => k !== 'ArrowLeft')];
  const holds = [];
  for (const key of keys) {
    await h.runFramesToPrompt(page, 5000);
    await watchWalk(page);
    await page.keyboard.down(key);
    await h.runFrames(page, 90);
    const released = await page.evaluate(() => window.__k2.steps.length);
    await page.keyboard.up(key);
    await h.runFramesToPrompt(page, 5000);
    const w = walkReceipt(await unwatchWalk(page), released);
    holds.push({ ...w, key });
    console.log(`  receipt (Max): ${walkLine(key, w)}`);
    if (w.paced.length >= 2) break;
  }
  const w = holds.at(-1);
  assert.ok(w.paced.length >= 2, `two paced presses or more in one hold (${w.paced.length})`);
  assert.equal(w.late, 0, 'none late');
  assert.ok(w.paced.every((p) => p.gap >= MIN_STEP_MS - 0.01 && p.gap <= MIN_STEP_MS + frame + 0.01), `every paced press 150 ms after the take before, within a frame: ${w.paced.map((p) => Math.round(p.gap)).join(', ')}`);
  for (const x of holds) assert.ok(x.after.every((cell) => cell === h.WAKE_CELL), `${x.key}: no step after the keyup`);
}, { needs: h.haveHfes });

// Spec 19 O1c (round 4 items 2 and 10), the page's modifier chords on the virtual display, at the
// prompt with nothing else held: Alt down, the down arrow pressed (the chord is not the game's),
// Alt let up, then ten repeats of the arrow (Playwright sends a held key's next down as its
// repeat), three frames apart, and 60 frames more; the same with Ctrl (the arrow with Ctrl down
// is no game key). The prompt takes no press and the leader stays: the repeats are taken back at
// the command prompt, and the arrow, first seen by a repeat, does not walk.
test('an arrow first pressed with Alt or Ctrl does not walk once the modifier is let up, though its repeats come (spec 19 O1c)', async (env) => {
  const { page } = await menuPage(env);
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  const state = () => page.evaluate(() => ({ takes: globalThis.__shownGame.prompt_takes(), leader: document.getElementById('screen').dataset.leader }));
  for (const modifier of ['Alt', 'Control']) {
    await h.runFramesToPrompt(page, 5000);
    const repeats = await page.evaluate(() => {
      const n = { repeats: 0 };
      window.__k2mod = n;
      document.getElementById('screen').addEventListener('keydown', (e) => {
        if (e.repeat) n.repeats += 1;
      });
      return 0;
    });
    const before = await state();
    await page.keyboard.down(modifier);
    await page.keyboard.down('ArrowDown');
    await h.runFrames(page, 3);
    await page.keyboard.up(modifier);
    for (let i = 0; i < 10; i += 1) {
      await page.keyboard.down('ArrowDown');
      await h.runFrames(page, 3);
    }
    await h.runFrames(page, 60);
    const after = await state();
    const sent = await page.evaluate(() => window.__k2mod.repeats);
    await page.keyboard.up('ArrowDown');
    console.log(`  receipt (${modifier}): ${sent - repeats} repeats of the arrow reached the canvas; prompt takes ${after.takes - before.takes}; leader ${after.leader === before.leader ? 'stayed' : 'moved'}`);
    assert.ok(sent >= 10, `${modifier}: the arrow's repeats came`);
    assert.equal(after.takes - before.takes, 0, `${modifier}: the prompt took no press`);
    assert.equal(after.leader, before.leader, `${modifier}: the leader stayed`);
  }
}, { needs: h.haveHfes });

// Spec 19 O1c (round 4 item 10): Alt pauses a held walk at the page and its keyup resumes it, on
// the virtual display: an arrow (down first, else the first whose press the prompt takes) pressed
// at the prompt, Alt down while its turn runs; once the prompt is back (Alt still down) 30 frames
// (500 ms) pass with no press taken; Alt let up, the paced press is taken within three frames (at
// 1× a 60 Hz frame can run no step; it was due long before).
test('Alt down pauses a held arrow at the prompt and its keyup resumes it at once (spec 19 O1c)', async (env) => {
  const { page } = await menuPage(env);
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  const takes = () => page.evaluate(() => globalThis.__shownGame.prompt_takes());
  await h.runFramesToPrompt(page, 5000);
  const t0 = await takes();
  await page.keyboard.down('ArrowDown');
  await h.runFrames(page, 3);
  const t1 = await takes();
  assert.equal(t1 - t0, 1, 'the press taken');
  await page.keyboard.down('Alt');
  await h.runFramesToPrompt(page, 5000);
  const t2 = await takes();
  await h.runFrames(page, 30);
  const t3 = await takes();
  await page.keyboard.up('Alt');
  const { called } = await h.runFrames(page, 3);
  const t4 = await takes();
  await page.keyboard.up('ArrowDown');
  console.log(`  receipt: takes at the press ${t1 - t0}; with Alt down until the prompt ${t2 - t1}, over 30 frames at the prompt ${t3 - t2}; after Alt's keyup, in ${called} frames ${t4 - t3}`);
  assert.equal(t2 - t1, 0, 'none while Alt is down, before the prompt');
  assert.equal(t3 - t2, 0, 'none at the prompt while Alt is down');
  assert.equal(t4 - t3, 1, 'the paced press at once after the keyup');
}, { needs: h.haveHfes });

// Spec 19 O1c (round 4 item 5): the virtual display stamps each event once, at its dispatch, and
// input can land between two frames: with the input offset at 10 ms the arrow's press reaches
// the game with the time of the frame last run plus 10 (`__keyAt`), and its event, kept and read
// again five frames later, still has that time.
test('the virtual display stamps an event once, between frames when asked (harness, spec 19 O1c)', async (env) => {
  const { page } = await menuPage(env);
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  await h.runFramesToPrompt(page, 5000);
  await page.evaluate(() => {
    document.getElementById('screen').addEventListener('keydown', (e) => {
      window.__k2kept = e;
    }, { once: true });
  });
  await h.displayInputAt(page, 10);
  const frameAt = await page.evaluate(() => window.__display.now());
  await page.keyboard.down('ArrowDown');
  const first = await page.evaluate(() => ({ at: globalThis.__keyAt, stamp: window.__k2kept.timeStamp }));
  await h.runFrames(page, 5);
  const again = await page.evaluate(() => window.__k2kept.timeStamp);
  await h.displayInputAt(page, 0);
  await page.keyboard.up('ArrowDown');
  console.log(`  receipt: the frame at ${frameAt.toFixed(1)} ms; the press stamped ${(first.at - frameAt).toFixed(1)} ms after it, read again five frames later ${(again - frameAt).toFixed(1)}`);
  assert.equal(first.at, frameAt + 10, 'the game got the event time, 10 ms after the frame');
  assert.equal(first.stamp, first.at, 'the event and the game agree');
  assert.equal(again, first.at, 'fixed: the same five frames later');
}, { needs: h.haveHfes });

// Spec 19 O1c with the context menu (spec 17), on the virtual display (frames run only when the
// test runs them): an arrow (down first, else the first that walks) pressed once and held with
// no browser repeat until the leader stepped; frames then run until the prompt is up between
// two of them (the held arrow would be pressed again in the next step), and the map is
// right-pressed there with the arrow still down. The menu opens with the focus, and the canvas's
// blur lets every held key go, so 600 frames later (10 s of the game, several turns) the leader is where it
// was; the arrow let up (its keyup takes nothing) and the menu closed with Escape, still there.
test('the context menu opened while an arrow is held lets it go: the hero walks no further under the menu or after it (spec 19 O1c, spec 17)', async (env) => {
  const { page } = await menuPage(env);
  const menu = page.locator('#context-menu');
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  const leader = () => page.evaluate(() => document.getElementById('screen').dataset.leader);
  const keys = ['ArrowDown', ...h.STEPS.map((s) => s.key).filter((k) => k !== 'ArrowDown')];
  let used = null;
  for (let round = 0; round < h.STEP_ROUNDS && used === null; round += 1) {
    if (round > 0) {
      await page.keyboard.press('Space');
      await h.runFramesToPrompt(page, 5000);
    }
    for (const key of keys) {
      await h.runFramesToPrompt(page, 5000);
      await page.keyboard.down(key);
      const { seen } = await h.runFrames(page, 30);
      if (seen.length > 1) {
        used = key;
        break;
      }
      await page.keyboard.up(key);
    }
  }
  assert.ok(used !== null, 'a held arrow walked in one of the four directions');
  const frames = await page.evaluate((limit) => {
    const v = window.__display;
    const c = document.getElementById('screen');
    for (let i = 0; i < limit; i += 1) {
      v.run();
      if (c.dataset.prompt === '1') return i + 1;
    }
    return -1;
  }, 5000);
  assert.ok(frames > 0, 'the prompt came back with the arrow still down');
  const at = await leader();
  await h.rightPress(page, 88, 88);
  await page.mouse.up({ button: 'right' });
  const under = await h.runFrames(page, 600);
  const open = await menu.isVisible();
  await page.keyboard.up(used);
  await page.keyboard.press('Escape');
  await menu.waitFor({ state: 'hidden' });
  const after = await h.runFrames(page, 120);
  const queued = await page.evaluate(() => document.getElementById('screen').dataset.keys);
  console.log(`  receipt: ${used} held, the prompt back ${frames} frames after its step; position changes while the menu was open ${under.seen.length - 1}, after it ${after.seen.length - 1}; keys queued at the end ${queued}`);
  assert.ok(open, 'the menu stayed open');
  assert.equal(under.seen.length - 1, 0, 'no step under the menu');
  assert.equal(after.seen.length - 1, 0, 'nor after it');
  assert.equal(await leader(), at, 'the hero where the menu opened');
  assert.equal(queued, '0', 'nothing queued');
}, { needs: h.haveHfes });

// Spec 19 O1c (review M2): the focus leaves the canvas while an arrow is held, as a Tab or a
// click on a toolbar control moves it (here to the Mute button), on the virtual display: an
// arrow (down first, else the first that walks) pressed once with no browser repeat until the
// leader stepped, ten frames more (the turn in flight), then the focus moved; the canvas's blur
// lets every held key go, so 1200 frames later (20 s of the game: the fight's turn in flight and
// more) the leader is where the blur found it, and the game's own step() (watched as
// `heldNoRepeat` watches it) shows the command prompt back and never taken down again: no key
// read after the blur, where the held arrow would have been pressed at the first; the arrow let
// up afterwards takes nothing.
test('the focus leaving the canvas while an arrow is held lets it go: the hero walks no further after the canvas\'s blur (spec 19 O1c)', async (env) => {
  const { page } = await menuPage(env);
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  const leader = () => page.evaluate(() => document.getElementById('screen').dataset.leader);
  const keys = ['ArrowDown', ...h.STEPS.map((s) => s.key).filter((k) => k !== 'ArrowDown')];
  let used = null;
  for (let round = 0; round < h.STEP_ROUNDS && used === null; round += 1) {
    if (round > 0) {
      await page.keyboard.press('Space');
      await h.runFramesToPrompt(page, 5000);
    }
    for (const key of keys) {
      await h.runFramesToPrompt(page, 5000);
      await page.keyboard.down(key);
      const { seen } = await h.runFrames(page, 30);
      if (seen.length > 1) {
        used = key;
        break;
      }
      await page.keyboard.up(key);
    }
  }
  assert.ok(used !== null, 'a held arrow walked in one of the four directions');
  await h.runFrames(page, 10);
  await page.evaluate(() => {
    const g = globalThis.__shownGame;
    const log = { prompts: 0, taken: 0, up: g.at_prompt() };
    window.__k2focus = log;
    g.step = function (ms) {
      Object.getPrototypeOf(this).step.call(this, ms);
      const up = this.at_prompt();
      if (up) log.prompts += 1;
      else if (log.up) log.taken += 1;
      log.up = up;
    };
  });
  await page.focus('#mute');
  const at = await leader();
  const after = await h.runFrames(page, 1200);
  const reads = await page.evaluate(() => {
    const log = window.__k2focus;
    window.__k2focus = null;
    delete globalThis.__shownGame.step;
    return log;
  });
  await page.keyboard.up(used);
  const late = await h.runFrames(page, 120);
  const queued = await page.evaluate(() => document.getElementById('screen').dataset.keys);
  console.log(`  receipt: ${used} held, the focus moved to Mute; after the blur ${reads.prompts} steps with the prompt up, ${reads.taken} prompts taken down; position changes after the blur ${after.seen.length - 1}, after the keyup ${late.seen.length - 1}; keys queued at the end ${queued}`);
  assert.ok(reads.prompts > 0, 'the prompt came back after the blur (the held arrow\'s chance)');
  assert.equal(reads.taken, 0, 'no key read after the canvas\'s blur');
  assert.equal(after.seen.length - 1, 0, 'no step after the canvas\'s blur');
  assert.equal(late.seen.length - 1, 0, 'nor after the keyup');
  assert.equal(await leader(), at, 'the hero where the blur found him');
  assert.equal(queued, '0', 'nothing queued');
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
// engine's memory, not a value of the game's files): the base copied the whole zip in. The
// memory once his files are in holds them, so it is checked against the bound, never printed.
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
  const under = (n) => (n < ZIP_START_MEMORY_MIB * 2 ** 20 ? 'pass' : 'fail');
  console.log(`  receipt: wasm memory ${MiB(idle)} MiB before the drop; after the start under the bound (${ZIP_START_MEMORY_MIB} MiB): ${under(memory)}`);
  assert.ok(r.whole === 0, 'the zip is not read whole');
  assert.ok(r.slices >= 2 && r.sliced > 0 && r.sliced < r.size / 10, 'at most a tenth of the zip is read');
  assert.ok(memory < ZIP_START_MEMORY_MIB * 2 ** 20, `the wasm memory after the start is under ${ZIP_START_MEMORY_MIB} MiB`);
  await kept(page, FILES, 'the game files are kept');
  await page.reload();
  const s = await started(page, FILES);
  assert.match(s.status, /Started from the files kept in this browser/);
  await h.waitPrompt(page);
  const reloaded = await page.evaluate(() => __u6mem());
  // Checked, not shown: the print was the only place a reload's memory showed.
  console.log(`  receipt: after the reload the game started from the kept files; wasm memory under the bound (${ZIP_START_MEMORY_MIB} MiB): ${under(reloaded)}`);
  assert.ok(reloaded < ZIP_START_MEMORY_MIB * 2 ** 20, `the wasm memory after the reload is under ${ZIP_START_MEMORY_MIB} MiB`);
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
// with the pointer over it, else its RGBA frame), compared in the page by an FNV-1a hash of each
// (the frame is the user's data: only the equality leaves the page). Spec 11 Design 7: the HD
// frame is composed without the pointer sprite, which the page draws over it pixelated at the
// frame's scale: the test stamps Game.hd_pointer_rgba's pixels that are not clear at its box.
const canvasShowsGame = (page) => page.evaluate(() => {
  const hash = (d) => {
    let x = 0x811c9dc5;
    for (let i = 0; i < d.length; i += 1) x = Math.imul(x ^ d[i], 0x01000193) >>> 0;
    return x;
  };
  const c = document.getElementById('screen');
  const g = globalThis.__shownGame;
  const want = g.hd_on() ? g.hd_frame_rgba() : g.frame_rgba();
  if (g.hd_on()) {
    const s = g.hd_scale();
    const [x, y, w, hh] = g.hd_pointer();
    const rgba = g.hd_pointer_rgba();
    const fw = g.width() * s;
    for (let i = 0; i < w * hh; i += 1) {
      if (rgba[4 * i + 3] === 0) continue;
      for (let k = 0; k < s * s; k += 1) {
        const [px, py] = [(x + (i % w)) * s + (k % s), (y + Math.floor(i / w)) * s + Math.floor(k / s)];
        want.set(rgba.subarray(4 * i, 4 * i + 4), (py * fw + px) * 4);
      }
    }
  }
  return hash(c.getContext('2d').getImageData(0, 0, c.width, c.height).data) === hash(want);
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
test('layout: the Speed group sits on Mute\'s line at 1280 and 1024 px, inside the toolbar; at 375 px the page does not scroll sideways', async (env) => {
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
        inside: behind.right <= document.getElementById('playback').getBoundingClientRect().right,
        sideways: document.documentElement.scrollWidth > document.documentElement.clientWidth,
      };
    });
    if (width >= 1024) assert.ok(m.sameLine, `${width} px: Speed on Mute's line`);
    assert.ok(m.inside, `${width} px: the group inside the toolbar`);
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

// What the page shows of HD: Settings' "HD on" (as 'true' or 'false'), the canvas size and box,
// the stored choice.
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
    pressed: String(document.getElementById('hd').checked),
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

test('HD: on by default and kept across a reload; the button switches it at once with no game step; off the canvas is the RGBA frame at 320 x 200, on the HD frame at 320·S x 200·S in the same box (S 2 in a smaller box, S 4 at twice the pixel ratio, S 2 again back on the first display); the pointer drawn over the HD frame (spec 11 Design 7): moved and at rest, and moved with the game frozen, the canvas the HD frame with the pointer over it exactly; no foreign request', async (env) => {
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
  // The pointer moved (spec 11 Design 7: the HD frame is composed without it, and the page draws it
  // over the frame): at rest at two places apart, the canvas each time the HD frame with the
  // pointer over it, exactly.
  const pointerNow = () => page.evaluate(() => [...globalThis.__shownGame.hd_pointer()]);
  const rests = [];
  for (const [gx, gy] of [[60, 60], [250, 150]]) {
    await page.mouse.move(...(await h.screenPoint(page, gx, gy)));
    await h.runFrames(page, 10);
    rests.push(await pointerNow());
    assert.ok(await canvasShowsGame(page), `the pointer at rest at (${gx}, ${gy}): the HD frame with the pointer over it`);
  }
  const apart = ([ax, ay, aw, ah], [bx, by, bw, bh]) => ax >= bx + bw || bx >= ax + aw || ay >= by + bh || by >= ay + ah;
  assert.ok(rests.every((p) => p[2] > 0) && rests[0][4] !== rests[1][4] && apart(rests[0], rests[1]), 'precondition: the pointer moved, its boxes apart');
  // The same move with the game frozen, so no rect the compositions wrote repaints anything: the
  // page's read of the pointer moved by the test's own offset (the pointer's own pixels), and the
  // canvas is the HD frame with the pointer at its new box, so under its old box the HD frame was
  // put back; then the pointer back where it is.
  await freezeGame(page, true);
  await page.evaluate(() => {
    const g = globalThis.__shownGame;
    const [x, y, w, hh, v] = [...Object.getPrototypeOf(g).hd_pointer.call(g)];
    const shift = x >= 160 ? -80 : 80;
    g.hd_pointer = () => Uint32Array.from([x + shift, y, w, hh, (v + 1) >>> 0]);
  });
  await h.runFrames(page, 2);
  assert.ok(await canvasShowsGame(page), 'the pointer moved, the game frozen: the HD frame put back under its old box, the pointer at its new one');
  await page.evaluate(() => { delete globalThis.__shownGame.hd_pointer; });
  await h.runFrames(page, 2);
  assert.ok(await canvasShowsGame(page), 'the pointer back: the HD frame with the pointer over it');
  await freezeGame(page, false);
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

test('the MMPX notice ships with the site: NOTICE.txt is the repository\'s NOTICE, as text, linked from the footer\'s notices', async (env) => {
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
    const a = document.querySelector('#notices a[href="NOTICE.txt"]');
    return a ? new URL(a.href).pathname : null;
  });
  assert.equal(link, '/NOTICE.txt', 'linked from the footer');
});

// Page init script: every read and write of the page's storage throws (a browser that refuses
// it), and the page's media queries are kept (`watchMedia`). The one read let through is the
// screen layout the harness keeps (h.openPage: Original, spec 10 Design 7), so the page stays
// on the layout these checks of 07's canvas are about; the HD layout with storage that throws
// is checked on its own (`layoutStorageThrows`).
function storageThrows() {
  const read = Storage.prototype.getItem;
  for (const name of ['getItem', 'setItem', 'removeItem']) {
    Storage.prototype[name] = function (key) {
      if (name === 'getItem' && key === 'u6.layout') return read.call(this, key);
      throw new DOMException('test: storage refused', 'SecurityError');
    };
  }
}

test('HD: Settings\' "HD on" works from the keyboard (Space); with storage that throws HD is on, switches, and nothing is kept across a reload; no page error', async (env) => {
  const foreign = [];
  const page = await bootHd(env, foreign, storageThrows);
  const errors = [];
  page.on('pageerror', () => errors.push(1));
  await h.waitPrompt(page);
  let shown = await hdShown(page);
  assert.ok(shown.stored === 'unreadable', 'the storage throws');
  assert.ok(shown.pressed === 'true' && shown.width === 1280, 'on by default');
  await page.click('#settings');
  await page.focus('#hd');
  await page.keyboard.press('Space');
  shown = await hdShown(page);
  assert.ok(shown.pressed === 'false' && shown.width === 320, 'Space: off');
  assert.ok(await canvasShowsGame(page), 'off: the RGBA frame');
  await page.keyboard.press('Space');
  shown = await hdShown(page);
  assert.ok(shown.pressed === 'true' && shown.width === 1280, 'Space again: on');
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
    if (mode === 'off') await switchHd(page);
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
  await forgetVia(page, '#forget-hd');
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
  await forgetVia(page, '#forget-hd');
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

test('HD pack: a zip of the owner\'s disk images with a manifest.json that only mentions the pack format (its root format is not the pack\'s: it has none) starts the game as before; that manifest on its own goes to the import too, not refused as a pack', async (env) => {
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

test('HD pack: a zip of the owner\'s disk images beside a manifest.json that declares the pack\'s format at its root but does not read (its CRC-32 wrong, as the broken-pack test breaks it) is not a pack: the game starts from it, no pack is set or kept, no foreign request', async (env) => {
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
  await page.evaluate(async () => {
    const files = [];
    for (const f of document.getElementById('__disks').files) files.push([f.name, new Uint8Array(await f.arrayBuffer())]);
    files.push(['manifest.json', new TextEncoder().encode('{"format": "u6-hdpack/1"}')]);
    const zip = window.__zipFiles(files, { crcOf: ['manifest.json', new Uint8Array(1)] });
    const dt = new DataTransfer();
    dt.items.add(new File([zip], 'disks.zip'));
    document.getElementById('drop').dispatchEvent(new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true }));
  });
  const s = await started(page, FILES);
  assert.match(s.status, /Started\./, 'the game started from the zip');
  assert.deepEqual(await packShown(page), { pack: 'none', matched: '', forgetEnabled: true }, 'no pack set');
  assert.equal(await h.keptPackBytes(page), null, 'no pack kept');
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

// --- the first-run chooser and the site's packs -----------------------------------------------
// The chooser (the page's language, HD: MMPX or a pack the site lists, and NPC voices), the
// site's packs under /packs/ (static files, asked with no key) and the catalog of the page's own
// text (site/i18n.js). The packs are the test's: made-up records (h.packKit), served by the test
// server from memory (h.setup). No game data.

// No page: nothing stands in front of /packs/: no Pages Function and no routing for one.
test('the site\'s packs are static files: no Pages Function (functions/) and no routing for one (site/_routes.json)', async () => {
  assert.ok(!existsSync(join(h.REPO, 'functions')), 'no Function');
  assert.ok(!existsSync(join(h.SITE, '_routes.json')), 'no routing for one');
});

// No page: both languages hold the same keys, each a string in both or a function of as many
// values in both, and every key index.html names is there.
test('the page\'s text: English and Korean hold the same keys, of the same shape; every key index.html names is in the catalog', async () => {
  const { TEXT } = await import('../../site/i18n.js');
  assert.deepEqual(Object.keys(TEXT), ['en', 'ko']);
  assert.deepEqual(Object.keys(TEXT.ko), Object.keys(TEXT.en), 'the same keys in the same order');
  for (const [key, en] of Object.entries(TEXT.en)) {
    const ko = TEXT.ko[key];
    assert.equal(typeof ko, typeof en, `${key}: the same kind`);
    if (typeof en === 'function') assert.equal(ko.length, en.length, `${key}: as many values`);
    else assert.ok(en.length > 0 && ko.length > 0 && ko !== en, `${key}: a text in each`);
  }
  const html = await readFile(join(h.SITE, 'index.html'), 'utf8');
  const named = [...html.matchAll(/data-i18n(?:-title|-label)?="([^"]+)"/g)].map((m) => m[1]);
  assert.ok(named.length >= 20, 'index.html names its labels');
  for (const key of named) assert.ok(typeof TEXT.en[key] === 'string', `index.html's ${key} is in the catalog`);
});

// --- the page (the owner's "쓰잘대기 없는 내용 빼주고", 2026-10-21) and fullscreen ---------------

// The page as a visitor sees it: the toolbar's controls, every button shown outside Settings and
// the Voices terms, the drop line, the footer's links, the controls' list.
const pageShown = (page) => page.evaluate(() => {
  const shownNow = (el) => el.offsetParent !== null || el.getClientRects().length > 0;
  const drop = document.getElementById('drop');
  const controls = document.getElementById('controls');
  return {
    title: document.title,
    h1: document.querySelector('h1').textContent.trim(),
    toolbars: document.querySelectorAll('[role="toolbar"]').length,
    toolbar: [...document.getElementById('playback').children].map((e) => e.id),
    afterScreen: document.getElementById('screen-box').nextElementSibling?.id ?? null,
    buttons: [...document.querySelectorAll('button')].filter((b) => shownNow(b) && !b.closest('#setup, #voice-terms')).map((b) => b.id),
    gone: ['limits', 'npc-bundles', 'forget-npc-bundles', 'game-strings', 'hd-credits', 'voice-credits', 'locale-note']
      .filter((id) => document.getElementById(id) !== null),
    hint: document.querySelectorAll('.hint').length,
    hdInSettings: document.getElementById('hd').type === 'checkbox' && document.getElementById('setup').contains(document.getElementById('hd')),
    kept: [...document.querySelectorAll('#kept button')].map((b) => b.id),
    keptOpen: document.getElementById('kept').open,
    drop: drop.hidden ? null : drop.querySelector('label[for="picker"]').textContent.replace(/\s+/g, ' ').trim(),
    notices: [...document.querySelectorAll('#notices a')].map((a) => new URL(a.href).pathname),
    controlsOpen: controls.open,
    controls: [...controls.querySelectorAll('li')].map((li) => li.textContent.trim()),
    controlsTitle: controls.querySelector('summary').textContent.trim(),
  };
});

const CONTROL_KEYS = ['keyWalk', 'keyCommands', 'keyCombat', 'keyPass', 'keySave', 'keySpeed', 'keyOpening'];

test('the page: titled "Ultima VI"; under the game screen one toolbar of exactly Fullscreen, Mute, Voices (and its chip), the Speed group and Settings, the only buttons shown outside Settings; no "What this version does", hint, NPC bundles, Game strings, HD button or page note; Settings holds "HD on" and a closed "Kept in this browser" group of the five forget buttons; the footer links the MMPX notice and the voice model\'s licence and notice; the controls are closed and list the keys in the page\'s language', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const { LICENSE_URL, NOTICE_URL } = await import('../../site/voice/model.js');
  const { page } = await h.openPage(env);
  await h.load(page, env);
  const p = await pageShown(page);
  assert.deepEqual([p.title, p.h1], ['Ultima VI', 'Ultima VI'], 'the title');
  assert.equal(p.toolbars, 1, 'one toolbar');
  assert.equal(p.afterScreen, 'playback', 'right under the game screen');
  assert.deepEqual(p.toolbar, ['fullscreen', 'mute', 'voices', 'voice-chip', 'speed-group', 'settings'], 'its controls, in order');
  assert.deepEqual(p.buttons, ['fullscreen', 'mute', 'voices', 'slower', 'faster', 'settings'], 'the only buttons shown');
  assert.deepEqual(p.gone, [], 'the removed parts are gone');
  assert.equal(p.hint, 0, 'no hint paragraph');
  assert.ok(p.hdInSettings, '"HD on" is a checkbox in Settings');
  assert.deepEqual(p.kept, ['forget', 'forget-save', 'forget-hd', 'forget-ko', 'forget-voices'], 'the forget buttons, in Settings\' group');
  assert.equal(p.keptOpen, false, 'the group closed');
  const [licence, notice] = [LICENSE_URL, NOTICE_URL].map((u) => u.pathname.slice(h.SITE.length));
  assert.deepEqual(p.notices, ['/NOTICE.txt', licence, notice], 'the footer: MMPX\'s notice, the voice model\'s licence and notice');
  assert.equal(p.controlsOpen, false, 'the controls closed');
  assert.equal(p.controlsTitle, TEXT.en.controls);
  assert.deepEqual(p.controls, CONTROL_KEYS.map((k) => TEXT.en[k]), 'the keys, in English');
  await page.click('#settings');
  await page.check('input[name="setup-lang"][value="ko"]');
  const ko = await pageShown(page);
  assert.equal(ko.controlsTitle, TEXT.ko.controls);
  assert.deepEqual(ko.controls, CONTROL_KEYS.map((k) => TEXT.ko[k]), 'and in Korean');
  console.log(`  receipt: toolbar ${p.toolbar.length} controls, ${p.buttons.length} buttons shown, ${p.kept.length} forget buttons in Settings, ${p.notices.length} notice links: pass`);
});

test('the drop line: shown with no game files kept, one sentence in the page\'s language with the picker; hidden once files are kept (made-up kept files: no game starts from them), while a drop on the page still loads a voice pack; Forget the kept files shows it again; Settings\' "HD on" switches HD and is kept across a reload', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const { page } = await h.openPage(env);
  await h.load(page, env);
  assert.equal((await pageShown(page)).drop, TEXT.en.dropLine, 'no files: the drop line');
  await h.keepFiles(page, [{ path: 'U6/other', bytes: [1, 2, 3] }]);
  await page.reload();
  await h.waitSettled(page);
  assert.equal((await pageShown(page)).drop, null, 'files kept: no drop line');
  const pack = await voicePack({ [PACK_A]: VOICE_A });
  await h.dropFiles(page, [], [{ name: 'voices.u6vp', text: JSON.stringify(pack) }]);
  await voicePackIs(page, 1);
  await forgetVia(page, '#forget');
  await h.waitSettled(page);
  assert.equal((await pageShown(page)).drop, TEXT.en.dropLine, 'forgotten: the drop line again');
  assert.equal(await page.evaluate(() => document.getElementById('hd').checked), true, 'HD on by default');
  await page.click('#hd');
  assert.equal(await page.evaluate(() => localStorage.getItem('u6.hd')), 'off', 'off, kept');
  await page.reload();
  await h.waitSettled(page);
  assert.equal(await page.evaluate(() => document.getElementById('hd').checked), false, 'off after the reload');
});

// The fullscreen as the page shows it: the fullscreen element, data-fullscreen, the toolbar
// button's label and state, the canvas's box and the viewport, the focus, the Exit button, the
// screen box's class.
const fullShown = (page) => page.evaluate(() => {
  const c = document.getElementById('screen').getBoundingClientRect();
  const button = document.getElementById('fullscreen');
  return {
    element: document.fullscreenElement?.id ?? null,
    mode: document.body.dataset.fullscreen,
    label: button.textContent,
    pressed: button.getAttribute('aria-pressed'),
    rect: [c.left, c.top, c.width, c.height].map((v) => Math.round(v * 10) / 10),
    viewport: [innerWidth, innerHeight],
    focus: document.activeElement?.id ?? null,
    exit: !document.getElementById('fullscreen-exit').hidden,
    full: document.getElementById('screen-box').classList.contains('full'),
  };
});

// The canvas as large as fits the viewport with the game's aspect ratio, centred.
function fitsScreen({ rect: [left, top, width, height], viewport: [vw, vh] }) {
  return Math.abs(width / height - 1.6) < 0.01 && width <= vw + 0.5 && height <= vh + 0.5
    && (Math.abs(width - vw) < 1 || Math.abs(height - vh) < 1)
    && Math.abs(left - (vw - width) / 2) < 1 && Math.abs(top - (vh - height) / 2) < 1;
}

const waitFull = (page, mode) => page.waitForFunction((m) => document.body.dataset.fullscreen === m, mode, { timeout: 10_000 });

test('fullscreen: the toolbar\'s Fullscreen puts the screen box in the browser\'s fullscreen, the canvas as large as fits with the game\'s aspect ratio, centred, with the focus; the button reads Exit fullscreen; the Exit button shows on entering and on a pointer move and hides 2 s after; the Exit button, the toolbar\'s button and the browser\'s own way out each leave, the page says so and the canvas has the focus again; without requestFullscreen the box fills the viewport by its class instead, left the same way', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  let { context, page } = await h.openPage(env);
  await h.load(page, env);
  await page.click('#fullscreen');
  await waitFull(page, 'on');
  let f = await fullShown(page);
  assert.equal(f.element, 'screen-box', 'the screen box is fullscreen');
  assert.ok(f.label === TEXT.en.exitFullscreen && f.pressed === 'true', 'the button reads Exit fullscreen');
  assert.ok(fitsScreen(f), 'the canvas fits the screen with the game\'s aspect ratio, centred');
  assert.equal(f.focus, 'screen', 'the canvas has the focus');
  assert.ok(f.exit, 'the Exit button on entering');
  const seen = f;
  await page.waitForFunction(() => document.getElementById('fullscreen-exit').hidden, null, { timeout: 5000 });
  await page.mouse.move(200, 200);
  await page.mouse.move(220, 210);
  assert.ok((await fullShown(page)).exit, 'a pointer move shows it again');
  await page.click('#fullscreen-exit');
  await waitFull(page, 'off');
  f = await fullShown(page);
  assert.ok(f.element === null && f.label === TEXT.en.fullscreen && f.pressed === 'false' && !f.exit, 'the Exit button leaves');
  assert.ok(!fitsScreen(f), 'the canvas back in the page');
  assert.equal(f.focus, 'screen', 'left by the Exit button: the canvas has the focus back');
  await page.click('#fullscreen');
  await waitFull(page, 'on');
  await page.evaluate(() => document.getElementById('fullscreen').click());
  await waitFull(page, 'off');
  assert.equal((await fullShown(page)).element, null, 'the toolbar\'s button leaves');
  assert.equal((await fullShown(page)).focus, 'screen', 'left by the toolbar\'s button: the canvas has the focus back');
  await page.click('#fullscreen');
  await waitFull(page, 'on');
  await page.evaluate(() => document.exitFullscreen()); // as the browser's Escape or F11 does
  await waitFull(page, 'off');
  assert.equal((await fullShown(page)).label, TEXT.en.fullscreen, 'the browser\'s own way out is shown');
  const lock = await page.evaluate(() => typeof navigator.keyboard?.lock === 'function');
  await context.close();
  // No requestFullscreen (iPhone Safari): the page's own, by a class.
  ({ context, page } = await h.openPage(env));
  await context.addInitScript(() => { delete Element.prototype.requestFullscreen; });
  await h.load(page, env);
  await page.click('#fullscreen');
  await waitFull(page, 'css');
  f = await fullShown(page);
  assert.ok(f.element === null && f.full, 'the box by its class, not the browser\'s fullscreen');
  assert.ok(fitsScreen(f), 'the canvas fits the viewport the same way');
  assert.ok(f.focus === 'screen' && f.exit && f.label === TEXT.en.exitFullscreen, 'the focus, the Exit button, the label');
  await page.click('#fullscreen-exit');
  await waitFull(page, 'off');
  assert.ok(!(await fullShown(page)).full, 'the Exit button leaves it');
  assert.equal((await fullShown(page)).focus, 'screen', 'the class left by the Exit button: the canvas has the focus back');
  await page.click('#fullscreen');
  await waitFull(page, 'css');
  await page.evaluate(() => document.getElementById('fullscreen').click());
  await waitFull(page, 'off');
  console.log(`  receipt: fullscreen canvas ${seen.rect.join(',')} in a viewport ${seen.viewport.join('x')}; css fallback ${f.rect.join(',')} in ${f.viewport.join('x')}; keyboard lock ${lock ? 'offered' : 'not offered'} by this Chromium: pass`);
});

test('the chooser\'s note on the game\'s text, in 한글: with no Korean pack at all (none listed, none kept) it points at the README\'s "Packs from your own copy"; with one listed it shows nothing, also while that pack is still downloading and once it is loaded; in English it shows nothing', async (env) => {
  sitePacks(env, { 'index.json': koListing(null) });
  let { context, page } = await h.openPage(env, { setup: { lang: 'ko' } });
  await h.load(page, env);
  await page.click('#settings');
  await listed(page, 0);
  let c = await chooser(page);
  assert.equal(c.note, await noteFor('ko'), 'none listed or kept: the note');
  assert.equal(c.noteLink, README_PACKS, 'pointing at the README');
  await page.check('input[name="setup-lang"][value="en"]');
  assert.equal((await chooser(page)).note, null, 'in English: none');
  await context.close();
  const { pack, runs, scripts } = await koFixture();
  sitePacks(env, { 'index.json': koListing(KO_FILE), [KO_FILE]: Uint8Array.from(pack) });
  const held = h.holdPack(env, KO_FILE);
  ({ context, page } = await h.openPage(env, { setup: null }));
  await page.goto(env.server.url);
  await held.asked;
  c = await chooser(page);
  assert.ok(c.shown && c.lang === 'ko', 'the first run\'s chooser, on 한글');
  assert.equal(c.note, null, 'a pack listed, still downloading: no note');
  held.release();
  await koPackIs(page, `${runs},${scripts}`);
  assert.equal((await chooser(page)).note, null, 'loaded: no note');
  await context.close();
});

test('fullscreen in a game in 한글 with the Korean pack (the user\'s disks, a synthetic conversation): the hidden input that takes Korean is inside the fullscreen element, the canvas has the focus, and while the game reads the player\'s words the input has it; composed syllables and Enter reach the game as on the page; left by the Exit button, the canvas has the focus again and a key reaches the game', async (env) => {
  const { page } = await h.openPage(env, { setup: { lang: 'ko' } });
  await h.load(page, env);
  const { archive, pack } = await koTypingFixture();
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: pack }]);
  await h.waitSettled(page);
  await h.waitPrompt(page);
  assert.equal(await h.replaceKept(page, 'converse.a', archive), 1, 'one archive replaced');
  await page.reload();
  await h.waitSettled(page);
  await h.waitPrompt(page);
  await page.click('#fullscreen');
  await waitFull(page, 'on');
  const inside = () => page.evaluate(() => ({
    ime: document.fullscreenElement?.contains(document.getElementById('ime')) ?? false,
    at: document.activeElement?.id ?? null,
  }));
  assert.deepEqual(await inside(), { ime: true, at: 'screen' }, 'the hidden input inside the fullscreen element, the canvas focused');
  await page.keyboard.press('t');
  await h.waitTaken(page);
  await page.keyboard.press('2');
  await page.waitForFunction(() => globalThis.__owned === 7 && document.getElementById('screen').dataset.keys === '0', null, { timeout: 30_000 });
  await h.waitFrames(page, 2);
  assert.deepEqual(await inside(), { ime: true, at: 'ime' }, 'the line read: the input has the focus, in fullscreen');
  await page.evaluate(() => { globalThis.__sent.length = 0; });
  const cdp = await page.context().newCDPSession(page);
  for (const [steps, syllable] of [[['ㅅ', '시'], '시'], [['ㅎ', '혀', '험'], '험']]) {
    for (const text of steps) await cdp.send('Input.imeSetComposition', { text, selectionStart: text.length, selectionEnd: text.length });
    await cdp.send('Input.insertText', { text: syllable });
  }
  await cdp.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 });
  await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 });
  await h.waitPrompt(page);
  assert.deepEqual(await page.evaluate(() => globalThis.__sent), ['text:시', 'text:험', 'Enter'], 'the committed syllables, then Enter');
  assert.equal((await fullShown(page)).element, 'screen-box', 'still fullscreen');
  // Left by a click on the Exit button (shown by a pointer move): the canvas has the focus back
  // and a key pressed then reaches the game.
  await page.mouse.move(300, 300);
  await page.mouse.move(320, 310);
  await page.click('#fullscreen-exit');
  await waitFull(page, 'off');
  assert.equal((await fullShown(page)).focus, 'screen', 'left: the canvas has the focus back');
  await page.evaluate(() => { globalThis.__sent.length = 0; });
  await page.keyboard.press('ArrowRight');
  await page.waitForFunction(() => globalThis.__sent.length > 0, null, { timeout: 10_000 });
  assert.deepEqual(await page.evaluate(() => globalThis.__sent), ['ArrowRight'], 'left: a key pressed then reaches the game');
}, { needs: h.haveHfes });

// What the chooser shows: whether it is on screen and above the game screen, its entries
// (each label's text; the HD entries shown), the HD entry checked, NPC voices checked and the
// voice model's terms shown, OK's label, its status line, the note on the game's text, the
// choice, the kept pack's source and the key an older page kept.
const chooser = (page) => page.evaluate(() => {
  const setup = document.getElementById('setup');
  const box = setup.getBoundingClientRect();
  const labels = (name) => [...setup.querySelectorAll(`input[name="${name}"]`)].map((i) => i.parentElement.textContent.trim());
  const note = document.getElementById('setup-note');
  return {
    shown: !setup.hidden && box.height > 0,
    first: box.bottom <= document.getElementById('screen').getBoundingClientRect().top,
    langs: labels('setup-lang'),
    lang: setup.querySelector('input[name="setup-lang"]:checked')?.value ?? null,
    hd: [...setup.querySelectorAll('input[name="setup-hd"]')].filter((i) => !i.parentElement.hidden)
      .map((i) => i.parentElement.textContent.trim()),
    checked: setup.querySelector('input[name="setup-hd"]:checked')?.value ?? null,
    markup: setup.querySelectorAll('#setup-hd b').length,
    voices: document.getElementById('setup-voices').checked,
    terms: !document.getElementById('setup-terms').hidden,
    ok: document.getElementById('setup-ok').textContent,
    status: document.getElementById('setup-status').textContent,
    note: note.hidden ? null : note.textContent.replace(/\s+/g, ' ').trim(),
    noteLink: document.getElementById('setup-note-link').href,
    stored: JSON.parse(localStorage.getItem('u6.setup')),
    source: localStorage.getItem('u6.hdSource'),
    key: localStorage.getItem('u6.packKey'),
  };
});

// The README section the chooser's note points at, and the note in `lang` (shown only with no
// Korean pack at all: none kept, none listed).
const README_PACKS = 'https://github.com/2lab-ai/ultima6#packs-from-your-own-copy';
async function noteFor(lang) {
  const { TEXT } = await import('../../site/i18n.js');
  return `${TEXT[lang].gameText} ${TEXT[lang].packsReadme}`;
}

// Waits until the chooser's HD entries hold the list the site gave (`n` packs).
const listed = (page, n) => page.waitForFunction((want) => document.getElementById('setup-hd').dataset.packs === want,
  String(n), { timeout: 10_000 });

// The tests' pack: `tiles` made-up tile records (no game: their keys match nothing), built in a
// page the way u6-hdpack writes one (h.packKit); its bytes.
async function madePack(env, tiles = 1) {
  const { context, page } = await h.openPage(env, { gated: false });
  await context.addInitScript(h.packKit);
  await h.load(page, env);
  const bytes = await page.evaluate(async (n) => {
    const rgba = new Uint8Array(64 * 64 * 4);
    for (let i = 0; i < 64 * 64; i += 1) rgba.set([(i * 4) & 255, (i >> 4) & 255, 77, 255], i * 4);
    const records = Array.from({ length: n }, (_, k) => ({ kind: 1, id: k + 1, letter: 0, hash: String(k + 1).padStart(16, '0'), width: 64, height: 64, rgba }));
    return [...window.__zipFiles(await window.__packFiles(records))];
  }, tiles);
  await context.close();
  return Uint8Array.from(bytes);
}

// The site's /packs/ for a test: `files` (name -> body), its request log emptied.
function sitePacks(env, files) {
  env.packs.clear();
  for (const [name, body] of Object.entries(files)) env.packs.set(name, body);
  env.packRequests.length = 0;
}

// The site's listing for the first-run tests: two HD packs (the first is the default), a Korean
// pack and a voice pack, each the tests' own.
async function fullSite(env, { hd = true } = {}) {
  const ko = await koFixture();
  const voices = await voicePack({ [PACK_A]: VOICE_A, [PACK_B]: VOICE_B });
  const hdPack = hd ? await madePack(env) : null;
  sitePacks(env, {
    'index.json': JSON.stringify({
      packs: hd ? [{ file: 'test-pack.zip', label: 'Test pack' }, { file: 'other.zip', label: 'Other' }] : [],
      ko: { file: KO_FILE, label: 'Korean (test)' },
      voices: { file: VOICES_FILE, label: 'Voices (test)' },
    }),
    ...(hd ? { 'test-pack.zip': hdPack } : {}),
    [KO_FILE]: Uint8Array.from(ko.pack),
    [VOICES_FILE]: JSON.stringify(voices),
  });
  return { ko, voices, hdPack };
}

// The voice model's terms as the chooser shows them: the restrictions' text and the paths the
// licence and the notice link to.
const setupTermsShown = (page) => page.evaluate(() => ({
  text: document.getElementById('setup-restrictions').textContent,
  licence: new URL(document.getElementById('setup-license').href).pathname,
  notice: new URL(document.getElementById('setup-notice').href).pathname,
}));

test('first visit: the chooser shows first, above the game screen, on 한글 (an en-US browser), with the first HD pack the site lists and NPC voices checked, the voice model\'s use restrictions shown verbatim with the licence and the notice linked and OK reading as their acceptance (in English too); unchecking NPC voices hides them and OK reads as OK; the listing is asked once a visit with no key, the listed Korean and voice packs downloaded once before any press; OK loads the HD pack, keeps the choice, asks nothing of /models/ and accepts nothing; a reload downloads nothing and skips the chooser; Settings shows the choice kept, NPC voices unchecked while the voices are off', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const { RESTRICTIONS } = await import('../../site/voice/terms.js');
  const { LICENSE_URL, NOTICE_URL } = await import('../../site/voice/model.js');
  const { ko, hdPack } = await fullSite(env);
  const { context, page } = await h.openPage(env, { setup: null });
  await context.addInitScript(voiceProbe);
  const requests = modelRequests(context);
  await h.load(page, env);
  await listed(page, 2);
  await koPackIs(page, `${ko.runs},${ko.scripts}`);
  await voicePackIs(page, 2);
  let c = await chooser(page);
  assert.ok(c.shown && c.first, 'the chooser shows, above the game screen');
  assert.deepEqual(c.langs, ['English', '한글']);
  assert.equal(c.lang, 'ko', 'on 한글');
  assert.equal((await pageText(page)).lang, 'ko', 'the page in 한글');
  assert.deepEqual(c.hd, [TEXT.ko.mmpx, 'Test pack', 'Other'], 'MMPX and the packs listed');
  assert.equal(c.checked, 'test-pack.zip', 'the first pack listed checked');
  assert.ok(c.voices && c.terms, 'NPC voices checked, the terms shown');
  assert.equal(c.ok, TEXT.ko.acceptStart, 'OK reads as acceptance');
  const shown = await setupTermsShown(page);
  const [licence, notice] = [LICENSE_URL, NOTICE_URL].map((u) => u.pathname.slice(h.SITE.length));
  assert.equal(shown.text, RESTRICTIONS, 'the restrictions shown verbatim');
  assert.ok(shown.licence === licence && shown.notice === notice, 'the licence and the notice linked');
  assert.equal(c.stored, null, 'nothing kept yet');
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200], [`/packs/${VOICES_FILE}`, 200], [`/packs/${KO_FILE}`, 200]],
    'the listing once, the voice pack and the Korean pack once each, before any press');
  await page.check('input[name="setup-lang"][value="en"]');
  assert.equal((await chooser(page)).ok, TEXT.en.acceptStart, 'in English too');
  await page.check('input[name="setup-lang"][value="ko"]');
  await page.uncheck('#setup-voices');
  c = await chooser(page);
  assert.ok(!c.voices && !c.terms, 'unchecked: the terms hidden');
  assert.equal(c.ok, TEXT.ko.ok, 'and OK reads as OK');
  await page.click('#setup-ok');
  await packIs(page, '1,0,0');
  c = await chooser(page);
  assert.ok(!c.shown, 'OK closes it');
  assert.deepEqual(c.stored, { lang: 'ko', version: h.SETUP_VERSION }, 'the choice kept');
  assert.equal(c.source, 'test-pack.zip', 'the listed HD pack loaded');
  assert.equal(await h.keptPackBytes(page), hdPack.length, 'and kept');
  assert.deepEqual(env.packRequests.slice(3), [['/packs/test-pack.zip', 200]], 'downloaded once');
  let v = await voiceShown(page);
  assert.ok(v.state === 'off' && !v.chip, 'voices off');
  assert.deepEqual(await page.evaluate(() => [window.__workers, localStorage.getItem('u6.voiceTerms'), localStorage.getItem('u6.voices')]),
    [0, null, null], 'no worker, nothing accepted');
  assert.deepEqual(requests, [], 'nothing asked of /models/');
  env.packRequests.length = 0;
  await page.reload();
  await packIs(page, '1,0,0');
  await koPackIs(page, `${ko.runs},${ko.scripts}`);
  assert.ok(!(await chooser(page)).shown, 'a kept choice skips the chooser');
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200]], 'a reload: the listing only, every pack kept');
  await page.click('#settings');
  await listed(page, 2);
  c = await chooser(page);
  assert.ok(c.shown && c.lang === 'ko' && c.checked === 'test-pack.zip', 'Settings: on the choice kept');
  assert.ok(!c.voices && !c.terms && c.ok === TEXT.ko.ok, 'NPC voices unchecked while the voices are off');
  assert.deepEqual(requests, [], 'still nothing asked of /models/');
  console.log('  receipt: 1 listing a visit, 3 packs downloaded once each, 0 model requests: pass');
});

test('first visit with the voice model: OK as the chooser opens (NPC voices checked; a listing that names no HD pack: MMPX checked) accepts the model\'s use restrictions as the Voices terms\' Accept does (the revision accepted kept, the voices on) and starts the voices to ready, the model asked of /models/ only after OK; Settings then shows NPC voices checked with the terms; unchecked there, OK turns the voices off', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const { REVISION } = await import('../../site/voice/model.js');
  await fullSite(env, { hd: false });
  const { context, page } = await h.openPage(env, { setup: null });
  await context.addInitScript(voiceProbe);
  const requests = modelRequests(context);
  await h.load(page, env);
  await listed(page, 0);
  await voicePackIs(page, 2);
  let c = await chooser(page);
  assert.ok(c.shown && c.lang === 'ko', 'the chooser on 한글');
  assert.equal(c.checked, '', 'MMPX checked: the listing names no HD pack');
  assert.ok(c.voices && c.terms && c.ok === TEXT.ko.acceptStart, 'NPC voices checked, the terms shown, OK as acceptance');
  assert.deepEqual(requests, [], 'nothing asked of /models/ before OK');
  const gpu = await page.evaluate(async () => Boolean(await navigator.gpu?.requestAdapter()));
  await page.click('#setup-ok');
  await waitVoice(page, () => ['ready', 'failed'].includes(document.body.dataset.voice), null);
  const v = await voiceShown(page);
  assert.ok(v.state === 'ready' && v.chip, 'the voices ready');
  assert.equal(v.path, gpu ? 'gpu' : 'cpu', 'where this browser runs them');
  assert.deepEqual(await page.evaluate(() => [localStorage.getItem('u6.voiceTerms'), localStorage.getItem('u6.voices')]),
    [REVISION, 'on'], 'the acceptance kept, voices on');
  assert.ok(requests.length > 0, 'the model asked of /models/ after OK');
  c = await chooser(page);
  assert.ok(!c.shown, 'OK closes the chooser');
  assert.deepEqual(c.stored, { lang: 'ko', version: h.SETUP_VERSION }, 'and keeps the choice');
  assert.deepEqual([koRequests(env).length, voiceRequests(env).length], [1, 1], 'the Korean and the voice pack downloaded once each');
  await page.click('#settings');
  c = await chooser(page);
  assert.ok(c.voices && c.terms && c.ok === TEXT.ko.acceptStart, 'Settings: NPC voices checked while on, the terms shown');
  await page.uncheck('#setup-voices');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  assert.equal((await voiceShown(page)).state, 'off', 'unchecked: OK turns the voices off');
  assert.equal(await page.evaluate(() => localStorage.getItem('u6.voices')), 'off', 'and keeps them off');
  console.log(`  receipt: voices ready on the ${v.path} from the chooser's OK, ${requests.length} model requests after it, 0 before: pass`);
}, { needs: h.haveModels });

// The chooser's OK, the HD entry checked and what is kept, read in the page.
const setupOkState = (page) => page.evaluate(() => ({
  shown: !document.getElementById('setup').hidden,
  disabled: document.getElementById('setup-ok').disabled,
  checked: document.querySelector('#setup-hd input:checked')?.value ?? null,
  stored: localStorage.getItem('u6.setup'),
}));

test('the chooser waits for the site\'s listing: on a first visit while the listing is held, OK is disabled with no HD entry checked and a press does nothing (nothing kept, the chooser open once the queue is done); once the listing is in, the first pack listed is checked, OK is enabled and loads it; a listing that fails (a 404, a body that does not read) enables OK with MMPX checked', async (env) => {
  const hdPack = await madePack(env);
  sitePacks(env, {
    'index.json': JSON.stringify({ packs: [{ file: 'test-pack.zip', label: 'Test pack' }, { file: 'other.zip', label: 'Other' }] }),
    'test-pack.zip': hdPack,
  });
  let hold = h.holdPack(env, 'index.json');
  let { context, page } = await h.openPage(env, { setup: null });
  await page.goto(env.server.url);
  await hold.asked;
  await page.waitForFunction(() => !document.getElementById('setup').hidden);
  let s = await setupOkState(page);
  assert.ok(s.shown && s.disabled, 'the listing held: the chooser shows, OK disabled');
  assert.equal(s.checked, null, 'no HD entry checked yet');
  await page.uncheck('#setup-voices'); // no voices turned on here
  await page.evaluate(() => document.getElementById('setup-ok').click());
  hold.release();
  await listed(page, 2);
  await h.waitSettled(page);
  s = await setupOkState(page);
  assert.ok(s.shown && s.stored === null, 'the press while disabled did nothing: nothing kept, the chooser open');
  assert.ok(!s.disabled && s.checked === 'test-pack.zip', 'the listing in: the first pack listed checked, OK enabled');
  await page.click('#setup-ok');
  await packIs(page, '1,0,0');
  const c = await chooser(page);
  assert.ok(!c.shown && c.source === 'test-pack.zip', 'OK loads it');
  assert.deepEqual(c.stored, { lang: 'ko', version: h.SETUP_VERSION }, 'and keeps the choice');
  await context.close();
  for (const [what, files] of [['a 404', {}], ['a body that does not read', { 'index.json': 'not JSON' }]]) {
    sitePacks(env, files);
    hold = h.holdPack(env, 'index.json');
    ({ context, page } = await h.openPage(env, { setup: null }));
    await page.goto(env.server.url);
    await hold.asked;
    await page.waitForFunction(() => !document.getElementById('setup').hidden);
    s = await setupOkState(page);
    assert.ok(s.disabled && s.checked === null, `${what}: OK disabled while the listing is held`);
    hold.release();
    await listed(page, 0);
    s = await setupOkState(page);
    assert.ok(!s.disabled && s.checked === '', `${what}: then OK enabled, MMPX checked`);
    await context.close();
  }
  console.log('  receipt: OK disabled while the listing was held (3 visits), a press then kept nothing; the listed pack loaded after; 2 failed listings: MMPX: pass');
});

test('the pack list: asked once a visit with no key, the packs listed by their labels as text (a file that is not a plain name left out), MMPX checked in Settings with no pack active; an old link\'s ?k= is taken off the address bar and opens no chooser, and the key an older page kept is removed', async (env) => {
  sitePacks(env, {
    'index.json': JSON.stringify({ packs: [{ file: 'test-pack.zip', label: '<b>Test</b> pack' }, { file: '../up.zip', label: 'Up' },
      { file: 'plain.zip' }] }),
  });
  const { context, page } = await h.openPage(env);
  await context.addInitScript(() => {
    try {
      if (sessionStorage.getItem('test:seeded') === null) {
        sessionStorage.setItem('test:seeded', '1');
        localStorage.setItem('u6.packKey', 'an-old-key');
      }
    } catch {
      // An opaque origin (about:blank) has no storage.
    }
  });
  await h.load(page, env);
  let c = await chooser(page);
  assert.ok(!c.shown, 'a current choice: no chooser');
  assert.equal(c.key, null, 'the key an older page kept is removed');
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200]], 'the listing asked once, with no key');
  await page.goto(`${env.server.url}?k=an-old-key&x=1#here`);
  await h.waitSettled(page);
  c = await chooser(page);
  assert.equal(page.url(), `${env.server.url}?x=1#here`, 'an old link\'s ?k= taken off the address bar');
  assert.ok(!c.shown && c.key === null, 'it opens no chooser and is not kept');
  await page.click('#settings');
  await listed(page, 2);
  c = await chooser(page);
  assert.deepEqual(c.hd, ['MMPX (no pack)', '<b>Test</b> pack', 'plain.zip'], 'the packs by their labels (a file without one by its name)');
  assert.equal(c.markup, 0, 'a label is text, not markup');
  assert.equal(c.checked, '', 'MMPX, the HD now, checked');
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200], ['/packs/index.json', 200]], 'once a visit');
});

test('a listed pack chosen: downloaded, its progress shown, loaded and kept as a dropped pack (no game needed); the chooser closes; a reload brings it back with no chooser and no download; one the site does not have leaves the chooser open with the reason and nothing loaded; MMPX then forgets it', async (env) => {
  const bytes = await madePack(env);
  sitePacks(env, {
    'index.json': JSON.stringify({ packs: [{ file: 'test-pack.zip', label: 'Test pack' }, { file: 'gone.zip', label: 'Gone' }] }),
    'test-pack.zip': bytes,
  });
  const { page } = await h.openPage(env, { setup: null });
  await h.load(page, env);
  // A first visit opens on 한글 with NPC voices checked; this test reads the English labels and
  // turns no voices on.
  await page.check('input[name="setup-lang"][value="en"]');
  await page.uncheck('#setup-voices');
  await listed(page, 2);
  assert.deepEqual((await chooser(page)).hd, ['MMPX (no pack)', 'Test pack', 'Gone']);
  await page.evaluate(() => {
    window.__setupLines = [];
    const line = document.getElementById('setup-status');
    new MutationObserver(() => window.__setupLines.push(line.textContent)).observe(line, { childList: true, characterData: true, subtree: true });
  });
  await page.check('input[name="setup-hd"][value="gone.zip"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  let c = await chooser(page);
  assert.ok(c.shown, 'not loaded: the chooser stays open');
  assert.equal(c.status, 'The HD pack was not loaded: it could not be downloaded (HTTP 404).', 'with the reason');
  assert.equal(c.stored, null, 'nothing kept');
  assert.deepEqual(await packShown(page), { pack: 'none', matched: '', forgetEnabled: true }, 'no pack');
  await page.check('input[name="setup-hd"][value="test-pack.zip"]');
  await page.click('#setup-ok');
  await page.waitForFunction(() => document.body.dataset.ops === '0' && document.body.dataset.hdPack === '1,0,0', null, { timeout: 30_000 });
  c = await chooser(page);
  const lines = await page.evaluate(() => window.__setupLines);
  const mib = (bytes.length / 2 ** 20).toFixed(1);
  assert.ok(lines.some((l) => l === `Downloading the HD pack: ${mib} of ${mib} MiB…`), 'the download\'s progress, of its size');
  assert.ok(lines.includes('Checking the HD pack…'), 'then the check');
  assert.ok(!c.shown, 'loaded: the chooser closes');
  assert.match((await h.state(page)).status, /^HD pack loaded: 1 tiles, 0 portraits and 0 panel pictures; it is used once a game runs\. It is kept in this browser for the next visit\.$/);
  assert.deepEqual(c.stored, { lang: 'en', version: h.SETUP_VERSION }, 'the choice is kept');
  assert.equal(c.source, 'test-pack.zip', 'and where the kept pack came from');
  assert.equal(await h.keptPackBytes(page), bytes.length, 'the pack is kept as a dropped one is');
  assert.equal((await h.state(page)).state, 'idle', 'no game started');
  assert.deepEqual(env.packRequests.slice(1), [['/packs/gone.zip', 404], ['/packs/test-pack.zip', 200]], 'each download');
  env.packRequests.length = 0;
  await page.reload();
  await page.waitForFunction(() => document.body.dataset.ops === '0' && document.body.dataset.hdPack === '1,0,0', null, { timeout: 30_000 });
  assert.ok(!(await chooser(page)).shown, 'a reload: no chooser');
  // The listing is asked at every restore (the voice pack it may name, spec 09).
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200]], 'the listing only, no download: the kept pack');
  await page.click('#settings');
  await listed(page, 2);
  assert.equal((await chooser(page)).checked, 'test-pack.zip', 'Settings: the pack chosen is checked');
  await page.check('input[name="setup-hd"][value=""]');
  await page.click('#setup-ok');
  await page.waitForFunction(() => document.body.dataset.ops === '0' && document.body.dataset.hdPack === 'none', null, { timeout: 30_000 });
  c = await chooser(page);
  assert.ok(!c.shown && c.source === null, 'MMPX: no pack, no source kept');
  assert.equal(await h.keptPackBytes(page), null, 'the pack forgotten');
});

// The pack on the page by its counts (data-hd-pack: one tile for the listed pack, two for the
// dropped one), once nothing is queued.
const packIs = (page, counts) => page.waitForFunction((want) => document.body.dataset.ops === '0'
  && document.body.dataset.hdPack === want, counts, { timeout: 30_000 });

test('a dropped pack still loads; the chooser then shows it as the active pack ("Dropped pack" checked), OK with nothing changed keeps it, and after a reload it is shown so again; MMPX then removes it', async (env) => {
  const dropped = await madePack(env, 2);
  sitePacks(env, {});
  const { page } = await h.openPage(env);
  await h.load(page, env);
  assert.ok(!(await chooser(page)).shown, 'MMPX kept: no chooser');
  await h.dropFiles(page, [], [{ name: 'dropped-pack.zip', bytes: [...dropped] }]);
  await packIs(page, '2,0,0');
  assert.equal((await chooser(page)).source, ':dropped', 'a dropped pack kept as one');
  await page.click('#settings');
  await listed(page, 0);
  let c = await chooser(page);
  assert.deepEqual(c.hd, ['MMPX (no pack)', 'Dropped pack'], 'the dropped pack is an entry');
  assert.equal(c.checked, ':dropped', 'and the one checked');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  assert.ok(!(await chooser(page)).shown, 'OK closes it');
  assert.equal(await page.evaluate(() => document.body.dataset.hdPack), '2,0,0', 'OK unchanged: the dropped pack stays');
  assert.equal(await h.keptPackBytes(page), dropped.length, 'and stays kept');
  await page.reload();
  await packIs(page, '2,0,0');
  await page.click('#settings');
  await listed(page, 0);
  c = await chooser(page);
  assert.ok(c.hd.includes('Dropped pack') && c.checked === ':dropped', 'after a reload: "Dropped pack" checked');
  await page.check('input[name="setup-hd"][value=""]');
  await page.click('#setup-ok');
  await packIs(page, 'none');
  c = await chooser(page);
  assert.ok(!c.shown && c.source === null, 'MMPX removes the dropped pack');
  assert.equal(await h.keptPackBytes(page), null, 'from the browser too');
  await page.click('#settings');
  await listed(page, 0);
  c = await chooser(page);
  assert.ok(c.hd.length === 1 && c.checked === '', 'no pack: MMPX only, checked');
  assert.deepEqual(env.packRequests, [['/packs/index.json', 404], ['/packs/index.json', 404]], 'the listing only, once a visit');
});

test('a listed pack chosen, then another pack dropped: the chooser shows the dropped one as active, and the listed pack chosen again is downloaded and replaces it', async (env) => {
  const [listedPack, dropped] = [await madePack(env, 1), await madePack(env, 2)];
  sitePacks(env, {
    'index.json': JSON.stringify({ packs: [{ file: 'test-pack.zip', label: 'Test pack' }] }),
    'test-pack.zip': listedPack,
  });
  const { page } = await h.openPage(env, { setup: null });
  await h.load(page, env);
  await page.check('input[name="setup-lang"][value="en"]'); // a first visit opens on 한글
  await page.uncheck('#setup-voices'); // and with NPC voices checked: none turned on here
  await listed(page, 1);
  await page.check('input[name="setup-hd"][value="test-pack.zip"]');
  await page.click('#setup-ok');
  await packIs(page, '1,0,0');
  assert.equal((await chooser(page)).source, 'test-pack.zip');
  await h.dropFiles(page, [], [{ name: 'dropped-pack.zip', bytes: [...dropped] }]);
  await packIs(page, '2,0,0');
  await page.click('#settings');
  await listed(page, 1);
  let c = await chooser(page);
  assert.deepEqual(c.hd, ['MMPX (no pack)', 'Dropped pack', 'Test pack']);
  assert.equal(c.checked, ':dropped', 'the dropped pack is the active one, not the listed one chosen before');
  env.packRequests.length = 0;
  await page.check('input[name="setup-hd"][value="test-pack.zip"]');
  await page.click('#setup-ok');
  await packIs(page, '1,0,0');
  c = await chooser(page);
  assert.ok(!c.shown && c.source === 'test-pack.zip', 'the listed pack is active again');
  assert.deepEqual(env.packRequests, [['/packs/test-pack.zip', 200]], 'downloaded again');
  assert.equal(await h.keptPackBytes(page), listedPack.length, 'and kept in place of the dropped one');
  await page.click('#settings');
  await listed(page, 1);
  c = await chooser(page);
  assert.ok(c.checked === 'test-pack.zip' && !c.hd.includes('Dropped pack'), 'the listed one checked, no dropped entry');
  env.packRequests.length = 0;
  await page.click('#setup-ok');
  await h.waitSettled(page);
  assert.deepEqual(env.packRequests, [], 'the active listed pack again: no download');
});

// The page's own text as shown: the language of the page, buttons and labels, the status line.
const pageText = (page) => page.evaluate(() => {
  const text = (id) => document.getElementById(id).textContent.trim();
  return {
    lang: document.documentElement.lang,
    forget: text('forget'),
    forgetHd: text('forget-hd'),
    settings: text('settings'),
    mute: text('mute'),
    fullscreen: text('fullscreen'),
    drop: document.querySelector('label[for="picker"]').textContent.replace(/\s+/g, ' ').trim(),
    canvas: document.getElementById('screen').getAttribute('aria-label'),
    status: text('status'),
    mmpx: document.querySelector('#setup-hd input[value=""]').parentElement.textContent.trim(),
  };
});

test('한글: a first visit opens on 한글 and English is one click away; the page\'s labels, buttons and status lines switch to Korean at once and the chooser says the game\'s text is still English; the choice survives a reload; English switches back', async (env) => {
  sitePacks(env, {});
  const { page } = await h.openPage(env, { setup: null });
  await h.load(page, env);
  assert.equal((await pageText(page)).lang, 'ko', 'a first visit: 한글');
  await page.uncheck('#setup-voices'); // no voices turned on here
  await page.check('input[name="setup-lang"][value="en"]');
  let shown = await pageText(page);
  assert.deepEqual({ lang: shown.lang, forget: shown.forget, mute: shown.mute, status: shown.status },
    { lang: 'en', forget: 'Forget the kept files', mute: 'Mute', status: 'No files loaded yet: drop or choose your Ultima VI files.' });
  const english = shown;
  await page.check('input[name="setup-lang"][value="ko"]');
  shown = await pageText(page);
  assert.deepEqual(shown, {
    lang: 'ko',
    forget: '보관한 파일 지우기',
    forgetHd: 'HD 팩 지우기',
    settings: '설정',
    mute: '소리 끄기',
    fullscreen: '전체화면',
    drop: '울티마 VI 파일(archive.org의 zip)을 여기에 놓거나 고르세요. 파일은 이 브라우저 밖으로 나가지 않습니다.',
    canvas: '게임 화면. 클릭하거나 Tab으로 옮겨 오면 화살표 키로 걷고 글자 키는 게임 명령입니다. Tab을 누르면 빠져나갑니다.',
    status: '아직 불러온 파일이 없습니다: 울티마 VI 파일을 놓거나 고르세요.',
    mmpx: 'MMPX (팩 없음)',
  }, 'Korean at once, the status line too');
  // With no Korean pack at all (none kept, none listed: spec 08): the game's text is English, and
  // one line in the chooser points at the README's way to make one; no path of the owner's data.
  const c = await chooser(page);
  assert.equal(c.note, await noteFor('ko'), 'the game\'s text is still English');
  assert.equal(c.noteLink, README_PACKS, 'it points at the README\'s "Packs from your own copy"');
  assert.ok(!/~|\/Users|Games|tools\/i18n/.test(c.note), 'at no path of data or tool');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  assert.deepEqual((await chooser(page)).stored, { lang: 'ko', version: h.SETUP_VERSION });
  await forgetVia(page, '#forget');
  await h.waitSettled(page);
  assert.equal((await h.state(page)).status, '보관한 게임 파일을 이 브라우저에서 지웠습니다.', 'a status line of app.js in Korean');
  await page.click('#mute');
  assert.equal((await pageText(page)).mute, '소리 켜기', 'a button\'s state in Korean');
  await page.click('#mute');
  await page.reload();
  await h.waitSettled(page);
  shown = await pageText(page);
  assert.ok(!(await chooser(page)).shown, 'no chooser after the reload');
  assert.ok(shown.lang === 'ko' && shown.forget === '보관한 파일 지우기' && shown.status === '아직 불러온 파일이 없습니다: 울티마 VI 파일을 놓거나 고르세요.',
    'Korean after the reload');
  await page.click('#settings');
  await page.check('input[name="setup-lang"][value="en"]');
  assert.deepEqual(await pageText(page), english, 'English again, as it was');
  assert.equal((await chooser(page)).note, null, 'no note in English');
});

// The owner's full-size pack for spec 07 Acceptance 5: every classic image of the owner's files
// as MMPX ×4 (so every image the game draws is a pack hit), written outside the repository by
//   scripts/cargo.sh run -p u6-hdtool --release --example scaler_pack -- --game <the zip> --out <this>
// The test skips without it.
const SCALER_PACK = join(h.ORIGINAL, '..', 'hd', 'scaler', 'u6-hdpack-scaler.zip');
const havePack = () => h.haveZip() && existsSync(SCALER_PACK);
// The wasm memory budget with a full pack (spec 07 H2): the zip-start bound of a session without
// a pack, plus the pack's decoded pictures, plus one atlas decoded while it loads (2048 x 2048
// RGBA) and the pack file copied in for the load. It holds two sizes of the owner's pack (decoded
// and its file), so it is checked, never printed.
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
  const file = (await stat(SCALER_PACK)).size;
  const made = () => page.evaluate(() => globalThis.__shownGame.hd_cache_counts());
  const made0 = await made();
  const loadMs = await dropPackFile(page, SCALER_PACK);
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
  // The share of the pack's entries matched and the budgets (they add the pack's decoded size and
  // its file's) are sizes of the owner's data, and so is the wasm memory once his zip is in: the
  // checks are shown, not the figures.
  console.log(`  receipt: the full pack loaded in ${loadMs} ms (drop to status: read, checked, decoded, set, stored), again in ${againMs} ms; matched: every portrait and panel picture, at least 90% of the tiles: pass`);
  const within = (ok) => (ok ? 'pass' : 'fail');
  console.log(`  receipt: wasm memory after the walks within the budget (the zip-start bound + the pack's pictures decoded + one atlas + the file): ${within(walked <= limit)}; after the second load within its budget (and a second pack decoded): ${within(again <= limitAgain)}`);
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

// --- voices (spec 09 V2, V3) -------------------------------------------------------------------

// The test's own lines (no game text): a 60-character one, a long one to cancel, a sentinel.
const LINE_60 = 'The quick brown fox jumps over the lazy dog near the rivers.';
const LONG_LINE = 'Here is a long line of the test that goes on for a while, so that its speech is '
  + 'still playing when a key cancels it. It has a second sentence as well, and a third one after '
  + 'that, which keeps the voice busy for many seconds more.';
const SENTINEL = 'Zq9Sentinel';
const MODEL_DIR = () => join(h.MODELS, 'supertonic3', '724fb5abbf5502583fb520898d45929e62f02c0b');

// One utterance as the engine's speech tap hands it to the page (Game.take_speech), made up.
const utterance = (turn, text, lang = 'en') => ({
  seq: turn, turn, speaker: 9, script: null, text, lang, traits: { type: 0, worktype: 0, alignment: 0 },
});

// Spec 09 Order 6: the test's own line in Hangul (made-up words) and one in English.
const VOICE_KO = '몰칸 브레딘 숩, 타례 고닌.';
const VOICE_EN = 'Zorvel quint plosk varrow.';

// Once the worker has ended every line sent to it: the Korean line and the English one, in one
// turn (`turn`, newer than any before), each reach the worker's `speak` with its own language and
// its exact text, and are spoken out.
async function saysEachLanguage(page, turn, timeout) {
  await waitVoice(page, () => window.__replies.length === window.__speaks.length, null, timeout);
  const from = await page.evaluate(() => [window.__speaks.length, window.__replies.length,
    Number(document.body.dataset.voiceDropped)]);
  await page.evaluate((list) => window.__speech.push(...list), [utterance(turn, VOICE_KO, 'ko'), utterance(turn, VOICE_EN)]);
  await waitVoice(page, ([, r, d]) => window.__replies.length >= r + 2 || Number(document.body.dataset.voiceDropped) > d,
    from, timeout);
  const sent = await page.evaluate(([s]) => window.__speaks.slice(s).map((m) => [m.lang, m.text]), from);
  assert.deepEqual(sent, [['ko', VOICE_KO], ['en', VOICE_EN]], 'each line to the worker with its language and its exact text');
  assert.deepEqual(await page.evaluate(([, r]) => window.__replies.slice(r), from), ['done', 'done'], 'both spoken out');
}

// Before the page's scripts: the Workers made, each line sent to them (`__speaks`: its seed,
// steps, profile, language and text, the test's own) and how each utterance ended in the worker
// (`done` or `cancelled`, in order), the storage persist calls, and each scheduled audio source's
// start and stop (seconds after the context's time then) and each linear ramp of a gain [value,
// at]; the samples of every buffer filled (`__pcm`, for the ASR of the test's own line).
function voiceProbe() {
  window.__workers = 0;
  window.__speaks = [];
  window.__replies = [];
  window.__persist = 0;
  window.__starts = [];
  window.__stops = [];
  window.__ramps = [];
  window.__pcm = [];
  const RealWorker = window.Worker;
  window.Worker = function Worker(...args) {
    window.__workers += 1;
    const worker = new RealWorker(...args);
    worker.addEventListener('message', (e) => {
      if (e.data?.type === 'done' || e.data?.type === 'cancelled') window.__replies.push(e.data.type);
    });
    const post = worker.postMessage.bind(worker);
    worker.postMessage = (m, ...rest) => {
      if (m?.type === 'speak') {
        window.__speaks.push({ seed: m.seed, steps: m.steps, profile: m.profile, lang: m.lang, text: m.text });
      }
      return post(m, ...rest);
    };
    return worker;
  };
  window.Worker.prototype = RealWorker.prototype;
  if (navigator.storage?.persist) {
    const persist = navigator.storage.persist.bind(navigator.storage);
    navigator.storage.persist = () => {
      window.__persist += 1;
      return persist();
    };
  }
  const start = AudioBufferSourceNode.prototype.start;
  AudioBufferSourceNode.prototype.start = function (when = 0, ...rest) {
    window.__starts.push([when - this.context.currentTime, performance.now()]);
    return start.call(this, when, ...rest);
  };
  const stop = AudioScheduledSourceNode.prototype.stop;
  AudioScheduledSourceNode.prototype.stop = function (when = 0) {
    window.__stops.push(when - this.context.currentTime);
    return stop.call(this, when);
  };
  const ramp = AudioParam.prototype.linearRampToValueAtTime;
  AudioParam.prototype.linearRampToValueAtTime = function (value, at) {
    window.__ramps.push([value, at]);
    return ramp.call(this, value, at);
  };
  const copy = AudioBuffer.prototype.copyToChannel;
  AudioBuffer.prototype.copyToChannel = function (source, ...rest) {
    window.__pcm.push({ rate: this.sampleRate, samples: Array.from(source) });
    return copy.call(this, source, ...rest);
  };
}

const voiceShown = (page) => page.evaluate(() => {
  const d = document.body.dataset;
  return {
    state: d.voice, error: d.voiceError, path: d.voicePath, gain: d.voiceGain, cold: Number(d.voiceColdMs),
    refetched: Number(d.voiceRefetched), spoken: Number(d.voiceSpoken), first: Number(d.voiceFirstMs),
    cancels: Number(d.voiceCancels), fade: Number(d.voiceFadeMs), playing: Number(d.voicePlaying),
    dropped: Number(d.voiceDropped), distinct: Number(d.voiceDistinct), chip: !document.getElementById('voice-chip').hidden,
    pressed: document.getElementById('voices').getAttribute('aria-pressed'),
    terms: !document.getElementById('voice-terms').hidden,
    status: document.getElementById('status').textContent,
  };
});

const waitVoice = (page, fn, arg, timeout = 180_000) => page.waitForFunction(fn, arg, { timeout, polling: 100 });

// The page's own line for `key` (its catalog, English), to compare the status line with.
async function line(key, ...args) {
  const { TEXT } = await import('../../site/i18n.js');
  const text = TEXT.en[key];
  return typeof text === 'function' ? text(...args) : text;
}

// The requests under /models/ of `context` (its pages and their workers), by path.
function modelRequests(context) {
  const seen = [];
  context.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (path.startsWith('/models/')) seen.push(path);
  });
  return seen;
}

// Every console message of the page and its workers, as text (to look for the sentinel in; never
// printed).
function consoleText(page) {
  const seen = [];
  page.on('console', (m) => seen.push(m.text()));
  page.on('worker', (w) => w.on('console', (m) => seen.push(m.text())));
  page.on('pageerror', (err) => seen.push(String(err?.message ?? err)));
  return seen;
}

test('hosting: every file under site/models is at most 24 MiB, each part and each whole file matches its manifest; LICENSE.txt and NOTICE beside the parts; the terms the page shows are the licence\'s Attachment A verbatim; _headers unchanged', async () => {
  const { RESTRICTIONS } = await import('../../site/voice/terms.js');
  const { MODEL_BASE, ORT_BASE } = await import('../../site/voice/model.js');
  let files = 0;
  for (const base of [MODEL_BASE, ORT_BASE]) {
    const dir = base.pathname;
    const manifest = JSON.parse(await readFile(join(dir, 'manifest.json'), 'utf8'));
    assert.ok(existsSync(join(dir, 'LICENSE.txt')) && existsSync(join(dir, 'NOTICE')), 'LICENSE.txt and NOTICE beside the parts');
    assert.ok((await readFile(join(dir, 'NOTICE'))).equals(await readFile(join(dir, 'NOTICE.txt'))), 'NOTICE.txt is the notice');
    assert.equal(manifest.partBytes, 24 * 2 ** 20);
    for (const f of manifest.files) {
      const whole = createHash('sha256');
      let size = 0;
      for (const p of f.parts) {
        const bytes = await readFile(join(dir, p.path));
        assert.ok(bytes.length === p.size && bytes.length <= manifest.partBytes, `${p.path}: its size, at most 24 MiB`);
        assert.equal(createHash('sha256').update(bytes).digest('hex'), p.sha256, `${p.path}: its sha256`);
        whole.update(bytes);
        size += bytes.length;
        files += 1;
      }
      assert.ok(size === f.size && whole.digest('hex') === f.sha256, `${f.name}: its parts joined are the file`);
    }
  }
  const licence = await readFile(join(MODEL_DIR(), 'LICENSE.txt'), 'utf8');
  assert.equal(RESTRICTIONS, licence.slice(licence.indexOf('\nAttachment A\n') + 1).trimEnd(), 'the terms are Attachment A verbatim');
  const notice = await readFile(join(MODEL_DIR(), 'NOTICE'), 'utf8');
  assert.ok(notice.includes(RESTRICTIONS) && /Served in parts/.test(notice), 'the notice passes the restrictions on and explains the parts');
  const headers = await readFile(join(h.SITE, '_headers'), 'utf8');
  assert.equal(createHash('sha256').update(headers).digest('hex'), '2ec9a0edfc178fbaafb9c17233967c812fc1cdd34a4e825819c923d764cecfbb', '_headers unchanged');
  console.log(`  receipt: ${files} hosted parts checked`);
}, { needs: h.haveModels });

test('voices off by default: no worker and nothing from /models/; Voices shows the use restrictions first and Cancel downloads nothing; the game runs on', async (env) => {
  const { context, page } = await h.openPage(env);
  await context.addInitScript(voiceProbe);
  const requests = modelRequests(context);
  const { RESTRICTIONS } = await import('../../site/voice/terms.js');
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  let v = await voiceShown(page);
  assert.ok(v.state === 'off' && !v.chip && v.pressed === 'false' && !v.terms, 'off by default');
  await page.click('#voices');
  v = await voiceShown(page);
  assert.ok(v.state === 'terms' && v.terms, 'the terms first');
  const shown = await page.evaluate(() => ({
    text: document.getElementById('voice-restrictions').textContent,
    licence: new URL(document.getElementById('voice-license').href).pathname,
    notice: new URL(document.getElementById('voice-notice').href).pathname,
    storage: document.getElementById('voice-storage').textContent.length > 0,
    link: new URL(document.getElementById('voice-terms-link').href).pathname,
  }));
  assert.equal(shown.text, RESTRICTIONS, 'the restrictions shown verbatim');
  const { LICENSE_URL, NOTICE_URL } = await import('../../site/voice/model.js');
  const [licence, notice] = [LICENSE_URL, NOTICE_URL].map((u) => u.pathname.slice(h.SITE.length));
  assert.ok(shown.licence === licence && shown.notice === notice && shown.link === notice, 'the licence and the notice linked');
  if (h.haveModels()) {
    // Served as text (a host types a file by its extension; `nosniff` keeps it so).
    for (const path of [licence, notice]) {
      const served = await page.request.get(new URL(path.slice(1), env.server.url).href);
      assert.ok(served.ok() && served.headers()['content-type'].startsWith('text/plain'), `${path.split('/').pop()}: served as text`);
    }
  }
  assert.ok(shown.storage, 'the storage line');
  await page.click('#voice-decline');
  v = await voiceShown(page);
  assert.ok(v.state === 'off' && !v.terms, 'Cancel: off');
  const probe = await page.evaluate(() => ({ workers: window.__workers, kept: [localStorage.getItem('u6.voiceTerms'), localStorage.getItem('u6.voices')] }));
  assert.equal(probe.workers, 0, 'no worker made');
  assert.deepEqual(probe.kept, [null, null], 'nothing accepted, voices not on');
  assert.deepEqual(requests, [], 'nothing requested from /models/');
  await h.waitTicks(page, 20);
  assert.equal((await h.state(page)).state, 'running', 'the game runs on');
}, { needs: h.haveHfes });

// A page in the Chromium with WebGPU, the game started from the user's disks, the probe on, and
// its requests under /models/ and console text recorded; null when that Chromium has no adapter.
async function gpuGame(env) {
  const { context, page } = await h.openPage(env, { gpu: true });
  await context.addInitScript(voiceProbe);
  const requests = modelRequests(context);
  const text = consoleText(page);
  await h.load(page, env);
  if (!(await page.evaluate(async () => Boolean(await navigator.gpu?.requestAdapter())))) return null;
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  return { context, page, requests, text };
}

// Voices on from the button (the terms accepted first when shown), until ready.
async function voicesOn(page) {
  await page.click('#voices');
  if (await page.evaluate(() => !document.getElementById('voice-terms').hidden)) await page.click('#voice-accept');
  await waitVoice(page, () => ['ready', 'failed'].includes(document.body.dataset.voice), null);
  return voiceShown(page);
}

// The words of `text`, lower case.
const words = (text) => text.toLowerCase().replace(/[^a-z0-9' ]+/g, ' ').split(/\s+/).filter(Boolean);

// The samples the page put into buffers since the last call, as one 16-bit WAV at their rate.
async function capturedWav(page) {
  const chunks = await page.evaluate(() => window.__pcm.splice(0));
  const rate = chunks[0]?.rate ?? 44100;
  const samples = chunks.flatMap((c) => c.samples);
  const wav = Buffer.alloc(44 + samples.length * 2);
  wav.write('RIFF', 0);
  wav.writeUInt32LE(36 + samples.length * 2, 4);
  wav.write('WAVEfmt ', 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20);
  wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(rate, 24);
  wav.writeUInt32LE(rate * 2, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write('data', 36);
  wav.writeUInt32LE(samples.length * 2, 40);
  samples.forEach((x, i) => wav.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(x * 32767))), 44 + i * 2));
  return wav;
}

// mlx-whisper (the owner's local ASR) on `wav`: the words it heard, or null without it.
const WHISPER = join(process.env.HOME ?? '', '.local/share/tour-tts/.venv/bin/mlx_whisper');
async function heard(wav) {
  if (!existsSync(WHISPER)) return null;
  const { mkdtemp, rm } = await import('node:fs/promises');
  const { tmpdir } = await import('node:os');
  const { execFile } = await import('node:child_process');
  const dir = await mkdtemp(join(tmpdir(), 'u6-voice-asr-'));
  try {
    await writeFile(join(dir, 'line.wav'), wav);
    await new Promise((done, fail) => execFile(WHISPER, [join(dir, 'line.wav'), '--model', 'mlx-community/whisper-large-v3-turbo',
      '--language', 'en', '--output-format', 'txt', '--output-dir', dir, '--verbose', 'False'],
    { timeout: 300_000 }, (err) => (err ? fail(err) : done())));
    return words(await readFile(join(dir, 'line.txt'), 'utf8'));
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

test('voices with WebGPU: accepted, downloaded, checked and warmed up to ready (cold start reported); a 60-character line sounds within 1 s and ASR hears it; a key cancels with an 80 ms fade, and so does a newer turn; a Korean line and an English one reach the worker each in its language and are spoken; Mute silences the voice gain; above 4x nothing is said; a reload starts from the cache with nothing downloaded; a corrupted cached part is fetched again; Forget voices empties the cache (receipt)', async (env) => {
  const g = await gpuGame(env);
  if (!g) {
    console.log('  receipt: skipped: this Chromium has no WebGPU adapter');
    return;
  }
  const { page, requests } = g;
  const t0 = Date.now();
  let v = await voicesOn(page);
  const opt = Date.now() - t0;
  assert.ok(v.state === 'ready', 'ready after the download, the checks and the warm-up');
  assert.ok(v.chip && v.pressed === 'true' && v.refetched === 0, 'the chip shows; nothing fetched twice');
  assert.equal(await page.evaluate(() => window.__persist), 1, 'persistent storage asked once');
  const parts = requests.filter((p) => /\.part\d+$|\.json$/.test(p) && !p.endsWith('manifest.json'));
  assert.equal(new Set(parts).size, parts.length, 'each part requested once');
  console.log(`  receipt: cold start (opt-in to ready) ${opt} ms; the worker's load ${v.cold} ms`);
  // A 60-character line: first audio within 1 s of the page getting it.
  assert.equal(LINE_60.length, 60);
  await page.evaluate(() => window.__pcm.splice(0));
  await page.evaluate((u) => window.__speech.push(u), utterance(1, LINE_60));
  await waitVoice(page, () => document.body.dataset.voiceSpoken === '1', null, 30_000);
  v = await voiceShown(page);
  console.log(`  receipt: first audio of the 60-character line ${v.first} ms after it reached the page`);
  assert.ok(v.first >= 0 && v.first <= 1000, 'first audio within 1 s');
  await waitVoice(page, () => document.body.dataset.voicePlaying === '0', null, 30_000);
  const asr = await heard(await capturedWav(page));
  if (asr === null) console.log('  receipt: ASR skipped (no mlx_whisper here)');
  else {
    const want = words(LINE_60);
    // The longest run of the line's words the ASR heard in order (their LCS).
    const lcs = want.map(() => asr.map(() => 0));
    want.forEach((w, i) => asr.forEach((a, j) => {
      const up = i > 0 ? lcs[i - 1][j] : 0;
      const left = j > 0 ? lcs[i][j - 1] : 0;
      const diag = i > 0 && j > 0 ? lcs[i - 1][j - 1] : 0;
      lcs[i][j] = w === a ? diag + 1 : Math.max(up, left);
    }));
    const hit = asr.length > 0 ? lcs[want.length - 1][asr.length - 1] : 0;
    console.log(`  receipt: ASR heard ${hit} of the line's ${want.length} words in order (${asr.length} heard)`);
    assert.ok(hit >= want.length - 1 && asr.length <= want.length + 1, 'ASR hears the line');
  }
  // A key cancels: Escape on the canvas while a long line plays.
  await page.focus('#screen');
  await page.evaluate((u) => window.__speech.push(u), utterance(2, LONG_LINE));
  await waitVoice(page, () => document.body.dataset.voiceSpoken === '2', null, 30_000);
  await page.evaluate(() => {
    window.__stops.length = 0;
    window.__ramps.length = 0;
  });
  await page.keyboard.press('Escape');
  await waitVoice(page, () => document.body.dataset.voiceCancels === '1', null, 5_000);
  let cut = await page.evaluate(() => ({ stops: [...window.__stops], ramps: [...window.__ramps] }));
  v = await voiceShown(page);
  assert.equal(v.fade, 80, 'an 80 ms fade');
  assert.ok(cut.stops.length >= 1 && cut.stops.every((s) => Math.abs(s - 0.08) < 0.02), 'every source stops 80 ms on');
  assert.ok(cut.ramps.length >= 1 && cut.ramps.every(([value]) => value === 0), 'its gain ramps to 0');
  await waitVoice(page, () => document.body.dataset.voicePlaying === '0', null, 5_000);
  // A newer turn cancels the old speech, and its own line is said.
  await page.evaluate((u) => window.__speech.push(u), utterance(3, LONG_LINE));
  await waitVoice(page, () => document.body.dataset.voiceSpoken === '3', null, 30_000);
  await page.evaluate(() => {
    window.__stops.length = 0;
  });
  await page.evaluate((u) => window.__speech.push(u), utterance(4, 'And now the next turn.'));
  await waitVoice(page, () => document.body.dataset.voiceCancels === '2' && document.body.dataset.voiceSpoken === '4', null, 30_000);
  cut = await page.evaluate(() => [...window.__stops]);
  assert.ok(cut.length >= 1 && cut.every((s) => Math.abs(s - 0.08) < 0.02), 'the newer turn fades the old speech out');
  // A Korean line and an English one reach the worker each in its language (spec 09 Order 6).
  await saysEachLanguage(page, 5, 30_000);
  // Mute: the voice gain 0 with the music's, and back.
  await page.click('#mute');
  assert.equal((await voiceShown(page)).gain, '0.00', 'muted: the voice gain 0');
  await page.click('#mute');
  assert.equal((await voiceShown(page)).gain, '1.00', 'unmuted: 1');
  // Above 4x nothing is synthesized.
  for (let i = 0; i < 5; i += 1) await page.click('#faster');
  assert.equal(await page.evaluate(() => document.getElementById('screen').dataset.speed), '6');
  const before = await voiceShown(page);
  await page.evaluate((u) => window.__speech.push(u), utterance(6, LINE_60));
  await waitVoice(page, (n) => Number(document.body.dataset.voiceDropped) > n, before.dropped, 10_000);
  assert.equal((await voiceShown(page)).spoken, before.spoken, 'at 6x: not said');
  for (let i = 0; i < 5; i += 1) await page.click('#slower');
  // A reload: voices come back from the cache, nothing downloaded, no line on the status.
  requests.length = 0;
  await page.reload();
  await h.waitSettled(page);
  await waitVoice(page, () => ['ready', 'failed'].includes(document.body.dataset.voice), null);
  v = await voiceShown(page);
  assert.ok(v.state === 'ready' && v.refetched === 0, 'ready again from the cache');
  assert.ok(requests.every((p) => !/\.part\d+$/.test(p)), 'no part downloaded');
  assert.notEqual(v.status, await line('voiceReady'), 'the start from the cache says nothing');
  // A cached part that no longer checks out is fetched again.
  await page.evaluate(async () => {
    const cache = await caches.open((await caches.keys()).find((n) => n.startsWith('u6-voice-')));
    const key = (await cache.keys()).find((r) => r.url.endsWith('vocoder.onnx.part1'));
    await cache.put(key, new Response(new Uint8Array([1, 2, 3])));
  });
  requests.length = 0;
  await page.click('#voices');
  assert.equal((await voiceShown(page)).state, 'off', 'Voices off');
  v = await voicesOn(page);
  assert.ok(v.state === 'ready' && v.refetched === 1, 'ready, the corrupted part fetched again');
  assert.deepEqual(requests.filter((p) => /\.part\d+$/.test(p)).map((p) => p.split('/').pop()), ['vocoder.onnx.part1'], 'only that part');
  // Forget voices: the cache emptied, the acceptance forgotten, off.
  await forgetVia(page, '#forget-voices');
  await waitVoice(page, (want) => document.getElementById('status').textContent === want, await line('voiceForgot'), 10_000);
  const left = await page.evaluate(async () => ({
    caches: (await caches.keys()).filter((n) => n.startsWith('u6-voice-')).length,
    terms: localStorage.getItem('u6.voiceTerms'),
  }));
  assert.ok(left.caches === 0 && left.terms === null && (await voiceShown(page)).state === 'off', 'nothing kept');
}, { needs: () => h.haveHfes() && h.haveModels() });

// A page in the default headless Chromium, which offers no WebGPU adapter, the game started from
// the user's disks, the probe on, and its requests under /models/ and console text recorded; null
// when it has one.
async function cpuGame(env) {
  const { context, page } = await h.openPage(env);
  await context.addInitScript(voiceProbe);
  const requests = modelRequests(context);
  const text = consoleText(page);
  await h.load(page, env);
  if (await page.evaluate(async () => Boolean(await navigator.gpu?.requestAdapter()))) return null;
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  return { context, page, requests, text };
}

// How many of `line`'s words `asr` heard in order (their longest common subsequence).
function heardInOrder(line, asr) {
  const want = words(line);
  const lcs = want.map(() => asr.map(() => 0));
  want.forEach((w, i) => asr.forEach((a, j) => {
    const up = i > 0 ? lcs[i - 1][j] : 0;
    const left = j > 0 ? lcs[i][j - 1] : 0;
    const diag = i > 0 && j > 0 ? lcs[i - 1][j - 1] : 0;
    lcs[i][j] = w === a ? diag + 1 : Math.max(up, left);
  }));
  return { hit: asr.length > 0 ? lcs[want.length - 1][asr.length - 1] : 0, of: want.length };
}

test('voices without WebGPU: on the CPU, a runtime that does not start turns voices off with its own content-free line while the game runs on; accepted, downloaded, checked and warmed up to ready on the CPU, which the ready line and the chip say (cold start reported); a 60-character line plays and ASR hears it (first audio reported); the game runs on while a long line is made; a key cancels with an 80 ms fade, and so does a newer turn, the worker dropping the line it makes; a Korean line and an English one reach the worker each in its language and are spoken; Forget voices empties the cache and the acceptance (receipt)', async (env) => {
  const ortPart = /onnxruntime-web\/1\.30\.0\/ort-wasm-simd-threaded\.asyncify\.wasm\.part0$/;
  // The CPU runtime that does not start: the worker's WebAssembly refuses to instantiate,
  // installed before the first part arrives.
  const f = await cpuGame(env);
  if (!f) {
    console.log('  receipt: skipped: this Chromium has a WebGPU adapter');
    return;
  }
  let installed;
  const hooked = new Promise((done) => { installed = done; });
  f.page.on('worker', async (w) => {
    await w.evaluate(() => {
      WebAssembly.instantiate = () => Promise.reject(new Error('test: no wasm'));
      WebAssembly.instantiateStreaming = WebAssembly.instantiate;
    });
    installed();
  });
  await f.context.route(ortPart, async (r) => {
    await hooked;
    await r.continue();
  });
  let v = await voicesOn(f.page);
  assert.ok(v.state === 'failed' && v.error === 'wasm' && !v.chip && v.path === '', 'the CPU runtime that does not start: off, with its kind');
  assert.equal(v.status, await line('voiceFailed', await line('voiceErrWasm')), 'the line names only the kind');
  assert.deepEqual(await f.page.evaluate(() => [localStorage.getItem('u6.voiceTerms'), localStorage.getItem('u6.voices')]),
    ['724fb5abbf5502583fb520898d45929e62f02c0b', 'off'], 'the acceptance kept, voices off');
  await h.waitTicks(f.page, 20);
  assert.equal((await h.state(f.page)).state, 'running', 'the game runs on');
  await f.context.close();

  // Ready on the CPU.
  const { page } = await cpuGame(env);
  const t0 = Date.now();
  v = await voicesOn(page);
  const opt = Date.now() - t0;
  assert.ok(v.state === 'ready' && v.path === 'cpu', 'ready on the CPU after the download, the checks and the warm-up');
  assert.ok(v.chip && v.pressed === 'true' && v.refetched === 0, 'the chip shows; nothing fetched twice');
  const chip = await page.evaluate(() => [!document.getElementById('voice-gpu').hidden, !document.getElementById('voice-cpu').hidden]);
  assert.deepEqual(chip, [false, true], 'the chip says CPU');
  assert.equal(v.status, await line('voiceReady', 'cpu'), 'the ready line says CPU');
  console.log(`  receipt: CPU cold start (opt-in to ready) ${opt} ms; the worker's load ${v.cold} ms`);
  // A 60-character line plays, and ASR hears it.
  await page.evaluate(() => window.__pcm.splice(0));
  const steps = await page.evaluate(() => document.body.dataset.voiceSteps);
  await page.evaluate((u) => window.__speech.push(u), utterance(1, LINE_60));
  await waitVoice(page, () => document.body.dataset.voiceSpoken === '1', null, 60_000);
  v = await voiceShown(page);
  console.log(`  receipt: CPU first audio of the 60-character line ${v.first} ms after it reached the page (${steps} steps)`);
  assert.ok(v.first >= 0, 'it sounds');
  await waitVoice(page, () => document.body.dataset.voicePlaying === '0' && window.__replies.length === 1, null, 60_000);
  const asr = await heard(await capturedWav(page));
  if (asr === null) console.log('  receipt: ASR skipped (no mlx_whisper here)');
  else {
    const { hit, of } = heardInOrder(LINE_60, asr);
    console.log(`  receipt: ASR heard ${hit} of the line's ${of} words in order (${asr.length} heard)`);
    assert.ok(hit >= of - 1 && asr.length <= of + 1, 'ASR hears the line');
  }
  // A key cancels: Escape on the canvas while a long line plays and its later chunks are made;
  // the game runs on meanwhile.
  const longer = `${LONG_LINE} ${LONG_LINE}`;
  await page.focus('#screen');
  await page.evaluate((u) => window.__speech.push(u), utterance(2, longer));
  await waitVoice(page, () => document.body.dataset.voiceSpoken === '2', null, 60_000);
  await h.waitTicks(page, 20);
  assert.equal((await h.state(page)).state, 'running', 'the game runs on while the line is made');
  await page.evaluate(() => {
    window.__stops.length = 0;
    window.__ramps.length = 0;
  });
  await page.keyboard.press('Escape');
  await waitVoice(page, () => document.body.dataset.voiceCancels === '1', null, 5_000);
  let cut = await page.evaluate(() => ({ stops: [...window.__stops], ramps: [...window.__ramps] }));
  v = await voiceShown(page);
  assert.equal(v.fade, 80, 'an 80 ms fade');
  assert.ok(cut.stops.length >= 1 && cut.stops.every((s) => Math.abs(s - 0.08) < 0.02), 'every source stops 80 ms on');
  assert.ok(cut.ramps.length >= 1 && cut.ramps.every(([value]) => value === 0), 'its gain ramps to 0');
  await waitVoice(page, () => window.__replies.length === 2, null, 30_000);
  assert.equal(await page.evaluate(() => window.__replies[1]), 'cancelled', 'the worker drops the line it makes');
  await waitVoice(page, () => document.body.dataset.voicePlaying === '0', null, 5_000);
  // A newer turn cancels the old speech, and its own line is said.
  await page.evaluate((u) => window.__speech.push(u), utterance(3, longer));
  await waitVoice(page, () => document.body.dataset.voiceSpoken === '3', null, 60_000);
  await page.evaluate(() => {
    window.__stops.length = 0;
  });
  await page.evaluate((u) => window.__speech.push(u), utterance(4, 'And now the next turn.'));
  await waitVoice(page, () => document.body.dataset.voiceCancels === '2' && document.body.dataset.voiceSpoken === '4', null, 60_000);
  cut = await page.evaluate(() => [...window.__stops]);
  assert.ok(cut.length >= 1 && cut.every((s) => Math.abs(s - 0.08) < 0.02), 'the newer turn fades the old speech out');
  await waitVoice(page, () => window.__replies.length === 4, null, 60_000);
  assert.deepEqual(await page.evaluate(() => window.__replies.slice(2)), ['cancelled', 'done'], 'the worker drops the old line and says the new one');
  const floor = Number(await page.evaluate(() => document.body.dataset.voiceSteps));
  console.log(`  receipt: CPU steps after four lines ${floor}`);
  assert.ok(floor >= 4 && floor <= 16, 'the steps stay within the CPU floor (4) and 16');
  // A Korean line and an English one reach the worker each in its language (spec 09 Order 6).
  await saysEachLanguage(page, 5, 120_000);
  // Forget voices: the cache emptied, the acceptance forgotten, off.
  await forgetVia(page, '#forget-voices');
  await waitVoice(page, (want) => document.getElementById('status').textContent === want, await line('voiceForgot'), 10_000);
  const left = await page.evaluate(async () => ({
    caches: (await caches.keys()).filter((n) => n.startsWith('u6-voice-')).length,
    terms: localStorage.getItem('u6.voiceTerms'),
  }));
  assert.ok(left.caches === 0 && left.terms === null && (await voiceShown(page)).state === 'off', 'nothing kept');
}, { needs: () => h.haveHfes() && h.haveModels() });

// Spec 09 Acceptance 6 and the sentinel: each failure turns voices off with a line that names only
// its kind, and the game runs on.
async function failsWith(env, code, arrange) {
  const g = await gpuGame(env);
  if (!g) return null;
  await arrange(g);
  const v = await voicesOn(g.page);
  assert.ok(v.state === 'failed' && v.error === code && !v.chip, `${code}: off, with its kind`);
  assert.equal(v.status, await line('voiceFailed', await line({
    download: 'voiceErrDownload', hash: 'voiceErrHash', quota: 'voiceErrQuota', gpu: 'voiceErrGpu',
  }[code])), `${code}: the line names only the kind`);
  await h.waitTicks(g.page, 20);
  assert.equal((await h.state(g.page)).state, 'running', `${code}: the game runs on`);
  return g;
}

test('voices fail recoverably: a part that does not download, a part whose bytes are wrong, a storage quota denial and a lost GPU device each turn voices off with a content-free line while the game runs on; a sentinel line said never reaches the console, the page\'s data or its text', async (env) => {
  const ort = /onnxruntime-web\/1\.30\.0\/ort-wasm-simd-threaded\.asyncify\.wasm\.part0$/;
  const failed = await failsWith(env, 'download', ({ context }) => context.route(ort, (r) => r.fulfill({ status: 404, body: '' })));
  if (failed === null) {
    console.log('  receipt: skipped: this Chromium has no WebGPU adapter');
    return;
  }
  await failsWith(env, 'hash', ({ context }) => context.route(ort, (r) => r.fulfill({ status: 200, body: Buffer.alloc(16) })));
  // A quota denial: the worker's Cache.put refuses, installed before the first part arrives.
  await failsWith(env, 'quota', ({ context, page }) => {
    let installed;
    const ready = new Promise((done) => { installed = done; });
    page.on('worker', async (w) => {
      await w.evaluate(() => {
        Cache.prototype.put = () => Promise.reject(new DOMException('test: no room', 'QuotaExceededError'));
      });
      installed();
    });
    return context.route(ort, async (r) => {
      await ready;
      await r.continue();
    });
  });
  // The lost device, with the sentinel said first.
  const g = await gpuGame(env);
  const { page, text } = g;
  assert.equal((await voicesOn(page)).state, 'ready');
  await page.evaluate((u) => window.__speech.push(u), utterance(1, `Remember the word ${SENTINEL} for later.`));
  await waitVoice(page, () => document.body.dataset.voiceSpoken === '1', null, 30_000);
  const [worker] = page.workers().filter((w) => w.url().endsWith('/voice/worker.js'));
  await worker.evaluate(() => self.onmessage({ data: { type: 'lose-device' } }));
  await waitVoice(page, () => document.body.dataset.voice === 'failed', null, 30_000);
  const v = await voiceShown(page);
  assert.ok(v.error === 'gpu' && v.status === await line('voiceFailed', await line('voiceErrGpu')), 'the lost device: off, with its kind');
  await h.waitTicks(page, 20);
  assert.equal((await h.state(page)).state, 'running', 'the game runs on');
  const where = await page.evaluate(() => JSON.stringify({
    body: { ...document.body.dataset }, canvas: { ...document.getElementById('screen').dataset },
    text: document.body.innerText, storage: { ...localStorage },
  }));
  assert.ok(!where.includes(SENTINEL), 'not in the page\'s data, text or storage');
  assert.ok(!text.some((m) => m.includes(SENTINEL)), 'not in the console of the page or its worker');
}, { needs: () => h.haveHfes() && h.haveModels() });

// Spec 09 Order 6: the worker's text preparation (site/voice/worker.js, its text section, from
// core.py's UnicodeProcessor) run alone, with a made-up index table in place of the model's: it
// holds the printable ASCII and every Hangul jamo (U+1100..U+11FF), as the model's does. Nothing
// is downloaded: the section reads the table only (`S.indexer`).
test('voice text: a Hangul line prepares to its jamo, in order, in <ko>…</ko> with no character dropped; a character the index table lacks is left out; an English line in <en>', async () => {
  const { runInNewContext } = await import('node:vm');
  const source = await readFile(join(h.SITE, 'voice', 'worker.js'), 'utf8');
  const from = source.indexOf('// --- text (core.py UnicodeProcessor)');
  const to = source.indexOf('// --- loading');
  assert.ok(from >= 0 && to > from, 'the worker\'s text section');
  const indexer = new Array(0x1200).fill(-1);
  let next = 0;
  for (const [a, b] of [[0x20, 0x7e], [0x1100, 0x11ff]]) {
    for (let c = a; c <= b; c += 1) {
      indexer[c] = next;
      next += 1;
    }
  }
  const { preprocess } = runInNewContext(`${source.slice(from, to)}\n;({ preprocess })`, { S: { indexer } });
  const jamo = VOICE_KO.normalize('NFKD');
  assert.ok(jamo.length > VOICE_KO.length && [...jamo].every((c) => indexer[c.codePointAt(0)] >= 0), 'the line is jamo the table holds');
  const ko = preprocess(VOICE_KO, 'ko');
  assert.equal(ko, `<ko>${jamo}</ko>`, 'every jamo kept, in order');
  assert.equal(ko.slice('<ko>'.length, -'</ko>'.length).normalize('NFC'), VOICE_KO, 'the syllables, composed again');
  assert.equal(preprocess('몰칸ж 숩.', 'ko'), `<ko>${'몰칸 숩.'.normalize('NFKD')}</ko>`, 'a character the table lacks: left out');
  assert.equal(preprocess(VOICE_EN, 'en'), `<en>${VOICE_EN}</en>`, 'an English line in <en>');
});

// --- voice profiles (spec 09 Design 4) -----------------------------------------------------

// The engine's own names the type-class table may use (spec 09 Rights), read from its source:
// the object types converse/start.rs names, and the worktypes actors/npc.rs runs through a
// combat routine.
async function engineNames() {
  const start = await readFile(join(h.REPO, 'crates/u6-core/src/converse/start.rs'), 'utf8');
  const types = new Set();
  for (const name of ['SHRINE', 'STATUES', 'HORSES', 'TALKING_OBJECTS']) {
    const m = start.match(new RegExp(`const ${name}: [^=]+= ([^;]+);`));
    assert.ok(m, `start.rs names ${name}`);
    for (const x of m[1].match(/0x[0-9a-f]+/g)) types.add(Number(x));
  }
  const npc = await readFile(join(h.REPO, 'crates/u6-core/src/actors/npc.rs'), 'utf8');
  const m = npc.match(/fn runs_combat_routine\(wt: u8\) -> bool \{\s*matches!\(wt, ([^)]+)\)/);
  assert.ok(m, 'actors/npc.rs names the combat worktypes');
  const combat = new Set();
  for (const arm of m[1].split('|')) {
    const [a, b = a] = arm.trim().split('..=').map(Number);
    for (let x = a; x <= b; x += 1) combat.add(x);
  }
  return { types, combat };
}

// Whether `p` is a profile within spec 09 Design 4's ranges.
function inRange(p, STYLES) {
  const sum = p.mix.reduce((s, e) => s + e.w, 0);
  return p.mix.length >= 1 && p.mix.length <= 3 && p.mix.every((e) => STYLES.includes(e.style) && e.w > 0)
    && new Set(p.mix.map((e) => e.style)).size === p.mix.length && Math.abs(sum - 1) < 1e-12
    && p.speed >= 0.9 && p.speed <= 1.15 && p.pitch >= -2 && p.pitch <= 2
    && Number.isInteger(p.steps) && p.steps >= 1 && p.steps <= 16 && p.seedSalt === p.seedSalt >>> 0;
}

test('voice profiles: deterministic; within the ranges for every object type the table names, every worktype, every alignment and many speakers; neighbouring speakers (n+1: the other sex, n+2: another leading preset) differ; the table keyed only by the engine\'s names; a pack\'s profile wins, checked into the ranges', async () => {
  const P = await import('../../site/voice/profile.js');
  const { STYLES } = await import('../../site/voice/model.js');
  // The table's keys: only what the engine names.
  const { types, combat } = await engineNames();
  assert.ok([...P.TYPE_CLASSES.keys()].every((t) => types.has(t)), 'every type key is an object type start.rs names');
  assert.ok([...P.TYPE_CLASSES.values()].every((c) => Object.hasOwn(P.CLASSES, c)), 'every type maps to a class');
  assert.deepEqual(P.WORKTYPE_BUCKETS.map((b) => b.name), ['combat'], 'one worktype bucket: the combat routines');
  assert.deepEqual([...P.WORKTYPE_BUCKETS[0].worktypes].sort((a, b) => a - b), [...combat].sort((a, b) => a - b), 'its worktypes are runs_combat_routine\'s');
  assert.ok([...P.ALIGNMENT_BUCKETS.keys()].every((a) => [0, 1, 2, 3].includes(a)), 'the alignment buckets are the 2-bit field\'s');
  // Deterministic, and within the ranges.
  const kinds = [...P.TYPE_CLASSES.keys(), 0, 1, 0x17f];
  const speakers = [-32768, -3, -2, -1, 0, 1, 2, 3, 9, 0xdf, 0xe0, 0xff, 0x100, 1000, 32767];
  let checked = 0;
  for (const type of kinds) {
    for (let worktype = 0; worktype < 256; worktype += 1) {
      for (let alignment = 0; alignment < 4; alignment += 1) {
        for (const speaker of speakers) {
          const u = { speaker, traits: { type, worktype, alignment } };
          const p = P.profileFor(u);
          assert.ok(inRange(p, STYLES), 'within the ranges');
          assert.deepEqual(P.profileFor(JSON.parse(JSON.stringify(u))), p, 'deterministic');
          checked += 1;
        }
      }
    }
  }
  for (const speaker of [undefined, null, 1.5, 'x', NaN]) {
    assert.ok(inRange(P.profileFor({ speaker, traits: undefined }), STYLES), 'any speaker: within the ranges');
  }
  assert.ok(inRange(P.profileFor(), STYLES), 'no utterance: within the ranges');
  // Neighbours differ: n and n+1 share no preset (the other sex), n and n+2 lead with another.
  const traits = { type: 0, worktype: 0, alignment: 0 };
  let pairs = 0;
  for (let n = -64; n < 1024; n += 1) {
    const [a, b, c] = [n, n + 1, n + 2].map((speaker) => P.profileFor({ speaker, traits }));
    const styles = new Set(a.mix.map((e) => e.style));
    assert.ok(b.mix.every((e) => !styles.has(e.style)), 'n and n+1: no preset in common');
    assert.notEqual(a.mix[0].style, c.mix[0].style, 'n and n+2: another leading preset');
    assert.ok(a.pitch !== b.pitch && a.pitch !== c.pitch, 'neighbours: another pitch');
    pairs += 2;
  }
  // A pack's profile wins, checked into the ranges; another actor keeps the default.
  const pack = new Map([[5, { mix: [{ style: 'F2', w: 3 }, { style: 'nope', w: 1 }], speed: 9, pitch: -9, steps: 99, seedSalt: 7 }]]);
  assert.deepEqual(P.profileFor({ speaker: 5, traits }, pack), { mix: [{ style: 'F2', w: 1 }], speed: 1.15, pitch: -2, steps: 16, seedSalt: 7, vary: { speed: 0, pitch: 0 } }, 'the pack\'s, checked (no vary: 0/0)');
  assert.deepEqual(P.profileFor({ speaker: 6, traits }).vary, { speed: 0, pitch: 0 }, 'the default: no line varies');
  assert.deepEqual(P.profileFor({ speaker: 6, traits }, pack), P.profileFor({ speaker: 6, traits }), 'not in the pack: the default');
  console.log(`  receipt: ${checked} profiles within the ranges; ${pairs} neighbouring pairs differ`);
});

test('voice mixer: one preset at weight 1 is that preset\'s bytes (alone at any weight, or twice); the weights normalise (scaled weights mix the same bytes); at most three presets, the heaviest; every mixed token has length 1; nothing valid: the default voice', async () => {
  const P = await import('../../site/voice/profile.js');
  const { STYLES, VOICE } = await import('../../site/voice/model.js');
  const presets = {};
  const raw = {};
  for (const s of STYLES) {
    const json = JSON.parse(await readFile(join(MODEL_DIR(), 'voice_styles', `${s}.json`), 'utf8'));
    presets[s] = P.presetOf(json);
    raw[s] = { ttl: Float32Array.from(json.style_ttl.data.flat(Infinity)), dp: Float32Array.from(json.style_dp.data.flat(Infinity)) };
  }
  const bytes = (a) => Buffer.from(a.buffer, a.byteOffset, a.byteLength);
  const same = (x, y) => bytes(x.ttl).equals(bytes(y.ttl)) && bytes(x.dp).equals(bytes(y.dp));
  for (const s of STYLES) {
    assert.ok(same(P.mixStyles(presets, [{ style: s, w: 1 }]), raw[s]), 'weight 1: the preset\'s bytes');
    assert.ok(same(P.mixStyles(presets, [{ style: s, w: 0.3 }]), raw[s]), 'alone at 0.3: the preset\'s bytes');
    assert.ok(same(P.mixStyles(presets, [{ style: s, w: 0.5 }, { style: s, w: 0.5 }]), raw[s]), 'twice: joined, the preset\'s bytes');
  }
  // The weights normalise.
  assert.ok(same(P.mixStyles(presets, [{ style: 'F1', w: 2 }, { style: 'M3', w: 6 }]),
    P.mixStyles(presets, [{ style: 'F1', w: 0.25 }, { style: 'M3', w: 0.75 }])), 'scaled weights: the same bytes');
  const sum = (m) => m.reduce((s, e) => s + e.w, 0);
  assert.ok(Math.abs(sum(P.checkedMix([{ style: 'F1', w: 2 }, { style: 'M3', w: 6 }])) - 1) < 1e-12, 'they sum to 1');
  // At most three: the heaviest.
  const five = [{ style: 'F1', w: 0.1 }, { style: 'F2', w: 0.3 }, { style: 'F3', w: 0.2 }, { style: 'F4', w: 0.25 }, { style: 'F5', w: 0.15 }];
  const kept = P.checkedMix(five);
  assert.deepEqual(kept.map((e) => e.style), ['F2', 'F4', 'F3'], 'the three heaviest');
  assert.ok(Math.abs(sum(kept) - 1) < 1e-12, 'normalised');
  assert.ok(same(P.mixStyles(presets, five), P.mixStyles(presets, five.filter((e) => ['F2', 'F4', 'F3'].includes(e.style)))), 'five mix as their three heaviest');
  // A mix is a new voice: every token of length 1, none of its presets'.
  const mixed = P.mixStyles(presets, [{ style: 'F1', w: 0.5 }, { style: 'M2', w: 0.3 }, { style: 'M5', w: 0.2 }]);
  for (const [a, dim] of [[mixed.ttl, 256], [mixed.dp, 16]]) {
    for (let at = 0; at < a.length; at += dim) {
      let n = 0;
      for (let d = 0; d < dim; d += 1) n += a[at + d] ** 2;
      assert.ok(Math.abs(Math.sqrt(n) - 1) < 1e-5, 'a mixed token has length 1');
    }
  }
  assert.ok(['F1', 'M2', 'M5'].every((s) => !same(mixed, raw[s])), 'the mix is none of its presets');
  // Nothing valid: the default voice.
  for (const mix of [[], null, [{ style: 'X9', w: 1 }], [{ style: 'F1', w: 0 }], [{ style: 'F1', w: -1 }], [{ style: 'F1', w: NaN }]]) {
    assert.deepEqual(P.checkedMix(mix), [{ style: VOICE, w: 1 }], 'nothing valid: the default voice');
  }
  assert.ok(same(P.mixStyles(presets, null), raw[VOICE]), 'the default voice\'s bytes');
}, { needs: h.haveModels });

// The median F0 (Hz) of `samples` at `rate`: the samples averaged by fours, then per 40 ms frame
// (10 ms on) loud enough (a tenth of the loudest frame's energy), the first lag between 70 and
// 400 Hz whose normalised autocorrelation is within 0.9 of the frame's highest, if that is above
// 0.6. Null with no voiced frame.
function medianF0(samples, rate) {
  const x = new Float64Array(Math.floor(samples.length / 4));
  for (let i = 0; i < x.length; i += 1) x[i] = (samples[4 * i] + samples[4 * i + 1] + samples[4 * i + 2] + samples[4 * i + 3]) / 4;
  const r = rate / 4;
  const win = Math.round(r * 0.04);
  const hop = Math.round(r * 0.01);
  const [lo, hi] = [Math.floor(r / 400), Math.ceil(r / 70)];
  const frames = [];
  for (let at = 0; at + win + hi < x.length; at += hop) {
    let e = 0;
    for (let i = 0; i < win; i += 1) e += x[at + i] ** 2;
    frames.push([at, e]);
  }
  const loudest = Math.max(0, ...frames.map(([, e]) => e));
  const f0 = [];
  for (const [at, e] of frames) {
    if (e < loudest / 10) continue;
    const corr = [];
    for (let lag = lo; lag <= hi; lag += 1) {
      let ab = 0;
      let bb = 0;
      for (let i = 0; i < win; i += 1) {
        ab += x[at + i] * x[at + i + lag];
        bb += x[at + i + lag] ** 2;
      }
      corr.push(ab / Math.sqrt(e * bb + 1e-12));
    }
    const best = Math.max(...corr);
    if (best < 0.6) continue;
    const k = corr.findIndex((c, i) => c >= 0.9 * best && (i === 0 || c >= corr[i - 1]) && (i === corr.length - 1 || c >= corr[i + 1]));
    f0.push(r / (lo + k));
  }
  if (f0.length === 0) return null;
  f0.sort((a, b) => a - b);
  return f0[f0.length >> 1];
}

const semitones = (a, b) => 12 * Math.log2(a / b);

// The flat cosine distance of two voices' style_ttl.
function styleDistance(a, b) {
  let ab = 0;
  let aa = 0;
  let bb = 0;
  for (let i = 0; i < a.ttl.length; i += 1) {
    ab += a.ttl[i] * b.ttl[i];
    aa += a.ttl[i] ** 2;
    bb += b.ttl[i] ** 2;
  }
  return 1 - ab / Math.sqrt(aa * bb);
}

test('voices per NPC (CPU): two neighbouring speakers say one line in two voices (median F0 at least 2 semitones apart, or their styles a cosine distance of 0.1); the same speaker says it again with the same samples; each line reaches the worker with the profile profileFor gives; the page counts the distinct voices and nothing more; pitch moves the voice but not its pace (the worker alone: at +2 and -2 semitones the rate moves by the pitch and the length stays, +2 being the line made slower, sample for sample; cost reported); no speaker, trait or profile value reaches the console, the page\'s data, its storage or its text', async (env) => {
  const P = await import('../../site/voice/profile.js');
  const { STYLES } = await import('../../site/voice/model.js');
  const g = await cpuGame(env);
  if (!g) {
    console.log('  receipt: skipped: this Chromium has a WebGPU adapter');
    return;
  }
  const { page, text } = g;
  assert.ok((await voicesOn(page)).state === 'ready', 'ready');
  // Speakers the test made: distinctive numbers (the sentinels), neighbours.
  const A = { speaker: 29436, traits: { type: 27963, worktype: 0x9a, alignment: 2 } };
  const B = { speaker: 29437, traits: { type: 0x17e, worktype: 0x85, alignment: 0 } };
  const W = { speaker: 40, traits: { type: 0, worktype: 0, alignment: 0 } };
  let turn = 0;
  // `who` says `line` in a turn of its own; its samples (one rate) once the worker is done.
  async function say(who, line) {
    turn += 1;
    const replies = await page.evaluate(() => window.__replies.length);
    await page.evaluate(() => window.__pcm.splice(0));
    await page.evaluate((u) => window.__speech.push(u), { ...utterance(turn, line), ...who });
    await waitVoice(page, (n) => window.__replies.length > n && document.body.dataset.voicePlaying === '0', replies, 60_000);
    const chunks = await page.evaluate(() => window.__pcm.splice(0));
    assert.ok(chunks.length > 0 && chunks.every((c) => c.rate === chunks[0].rate), 'samples at one rate');
    const speak = await page.evaluate(() => window.__speaks.at(-1));
    return { rate: chunks[0].rate, samples: chunks.flatMap((c) => c.samples), speak };
  }
  await say(W, 'A first line, to settle the steps.');
  const a1 = await say(A, LINE_60);
  const b = await say(B, LINE_60);
  let a2 = await say(A, LINE_60);
  for (let i = 0; i < 2 && a2.speak.steps !== a1.speak.steps; i += 1) a2 = await say(A, LINE_60);
  // The profiles the worker got: profileFor's, the seed the speaker's and the text's.
  const [pA, pB] = [P.profileFor(A), P.profileFor(B)];
  assert.deepEqual(a1.speak.profile, pA, 'A: its profile');
  assert.deepEqual(b.speak.profile, pB, 'B: its profile');
  assert.ok(a1.speak.seed === a2.speak.seed && a1.speak.seed !== b.speak.seed, 'the seed: the speaker\'s and the line\'s');
  // The same speaker, the same line, the same steps: the same samples.
  assert.equal(a2.speak.steps, a1.speak.steps, 'A\'s lines at the same steps');
  assert.ok(a1.rate === a2.rate && a1.samples.length === a2.samples.length && a1.samples.every((s, i) => s === a2.samples[i]), 'A again: the same samples');
  // Two voices: F0 apart, or the styles apart.
  const presets = {};
  for (const s of STYLES) presets[s] = P.presetOf(JSON.parse(await readFile(join(MODEL_DIR(), 'voice_styles', `${s}.json`), 'utf8')));
  const [fA, fB] = [medianF0(a1.samples, a1.rate), medianF0(b.samples, b.rate)];
  const apart = Math.abs(semitones(fA, fB));
  const distance = styleDistance(P.mixStyles(presets, pA.mix), P.mixStyles(presets, pB.mix));
  console.log(`  receipt: neighbours' median F0 ${apart.toFixed(2)} semitones apart; style cosine distance ${distance.toFixed(3)}`);
  assert.ok(fA !== null && fB !== null && (apart >= 2 || distance >= 0.1), 'two voices');
  assert.equal((await voiceShown(page)).distinct, 3, 'three distinct voices spoken');
  // Nothing of who spoke reaches the page: the speakers, A's type, the profiles' presets, paces,
  // pitches and weights (worktypes and alignments are too short to tell from a count).
  const sentinels = [String(A.speaker), String(B.speaker), String(A.traits.type),
    ...[pA, pB].flatMap((p) => [String(p.speed), String(p.pitch), ...p.mix.flatMap((e) => [e.style, String(e.w)])])];
  const where = await page.evaluate(() => JSON.stringify({
    body: { ...document.body.dataset }, canvas: { ...document.getElementById('screen').dataset },
    text: document.body.innerText, storage: { ...localStorage },
  }));
  const found = (s, t) => new RegExp(`(^|[^0-9A-Za-z.])${s.replace(/[.\-]/g, '\\$&')}($|[^0-9A-Za-z])`).test(t);
  assert.ok(sentinels.every((s) => !found(s, where)), 'not in the page\'s data, text or storage');
  assert.ok(sentinels.every((s) => !text.some((m) => found(s, m))), 'not in the console of the page or its worker');

  // The worker alone (the model cached in this context), one voice: at 0, +2 and -2 semitones,
  // then at 0 at the pace +2 synthesizes at (its speed over its rate's ratio).
  await page.click('#voices');
  assert.equal((await voiceShown(page)).state, 'off', 'the page\'s voices off');
  const runs = await page.evaluate(async (line) => {
    const worker = new Worker(new URL('/voice/worker.js', location.href), { type: 'module' });
    const next = (want) => new Promise((done, fail) => {
      const got = [];
      worker.onmessage = (e) => {
        if (e.data.type === 'audio') got.push(e.data);
        else if (e.data.type === 'error') fail(new Error(e.data.code));
        else if (e.data.type === want) done(got);
      };
    });
    const loaded = next('ready');
    worker.postMessage({ type: 'load', cachedOnly: true });
    await loaded;
    const out = [];
    for (const [pitch, speed] of [[0, 1.05], [2, 1.05], [-2, 1.05], [0, null]]) {
      const done = next('done');
      worker.postMessage({ type: 'speak', id: out.length + 1, text: line, lang: 'en', seed: 7, steps: 4,
        profile: { mix: [{ style: 'M1', w: 1 }], speed: speed ?? (1.05 * out[0].rate) / out[1].rate, pitch, steps: 16, seedSalt: 0 } });
      const audio = await done;
      out.push({ pitch, rate: audio[0].sampleRate, seconds: audio.reduce((s, m) => s + m.pcm.length / m.sampleRate, 0),
        synthMs: audio.reduce((s, m) => s + m.synthMs, 0), samples: audio.flatMap((m) => Array.from(m.pcm)) });
    }
    worker.terminate();
    return out;
  }, LINE_60);
  const [r0, up, down, slow] = runs;
  for (const r of [up, down]) {
    assert.ok(Math.abs(r.rate / r0.rate - 2 ** (r.pitch / 12)) < 1e-4, `${r.pitch} st: the rate moved by it`);
    assert.ok(Math.abs(r.seconds / r0.seconds - 1) < 0.01, `${r.pitch} st: the same length`);
    console.log(`  receipt: CPU pitch ${r.pitch > 0 ? '+' : ''}${r.pitch} st: length ${(100 * (r.seconds / r0.seconds - 1)).toFixed(2)}%, synthesis ${Math.round(r.synthMs)} ms against ${Math.round(r0.synthMs)} ms at 0 (${r0.seconds.toFixed(2)} s of audio, 4 steps)`);
  }
  // +2 is the line made at the slower pace, sample for sample, played 2^(2/12) times faster.
  assert.ok(slow.rate === r0.rate && slow.samples.length === up.samples.length && slow.samples.every((x, i) => x === up.samples[i]), '+2 st: the slower line\'s samples');
  console.log(`  receipt: +2 st is the line made ${(100 * (slow.seconds / r0.seconds - 1)).toFixed(1)}% longer, the same samples, at a ${(up.rate / r0.rate).toFixed(4)} times higher rate`);
}, { needs: () => h.haveHfes() && h.haveModels() });

// --- the voice pack (spec 09 Design 4) -----------------------------------------------------

const PACK_SENTINEL = 'Zq5VoicePackSentinel';
// The test's own actors (distinctive numbers) and voices: weights, paces and pitches that are
// exact binary fractions, so the check leaves them as they are.
const PACK_A = 29473;
const PACK_B = 29474;
const PACK_C = 29475; // in no pack
const VOICE_A = { mix: [{ style: 'F4', w: 0.625 }, { style: 'M2', w: 0.375 }], speed: 0.9375, pitch: 1.8125, steps: 12, seedSalt: 2718281 };
const VOICE_B = { mix: [{ style: 'M5', w: 1 }], speed: 1.125, pitch: -1.6875, steps: 16, seedSalt: 0 };
// A voice as the page reads it: a /1 pack's with vary 0/0 (spec 15 Design 4).
const NO_VARY = { speed: 0, pitch: 0 };
const read1 = (v) => ({ ...v, vary: NO_VARY });
// A voice of a /2 pack whose lines vary (spec 15 Design 5), the test's own numbers.
const VOICE_V = { mix: [{ style: 'F3', w: 0.75 }, { style: 'F5', w: 0.25 }], speed: 1.0625, pitch: 0.4375, steps: 16, seedSalt: 1618033, vary: { speed: 0.09375, pitch: 1.125 } };

// A u6.voice-pack/1 of `profiles` (actor to profile) as the tool writes it, `extra` laid over it.
async function voicePack(profiles, extra = {}) {
  const P = await import('../../site/voice/profile.js');
  return { format: P.PACK_FORMAT, model: 'test-model', revision: P.RANGES_REVISION, profiles, ...extra };
}

// A u6.voice-pack/2 likewise (each profile with its vary).
async function voicePack2(profiles, extra = {}) {
  const P = await import('../../site/voice/profile.js');
  return { format: P.PACK_FORMAT_2, model: 'test-model', revision: P.RANGES_REVISION, profiles, ...extra };
}

test('voice pack check (no page): a pack as the tool writes it reads (/1, its voices with vary 0/0, and /2, each with its vary), its actors\' profiles from it; every other form is refused by a code that names only the kind (format, keys, shape, revision, range), a /2 voice\'s vary among them', async () => {
  const P = await import('../../site/voice/profile.js');
  const good = await voicePack({ [PACK_A]: VOICE_A, [PACK_B]: VOICE_B });
  const read = P.readPack(structuredClone(good));
  assert.ok(read.ok && read.pack.size === 2, 'it reads');
  assert.deepEqual(read.pack.get(PACK_A), read1(VOICE_A), 'A: its profile, as it is, no vary');
  const traits = { type: 0, worktype: 0, alignment: 0 };
  assert.deepEqual(P.profileFor({ speaker: PACK_B, traits }, read.pack), read1(VOICE_B), 'profileFor: the pack\'s');
  const good2 = await voicePack2({ [PACK_A]: VOICE_V, [PACK_B]: read1(VOICE_B) });
  const read2 = P.readPack(structuredClone(good2));
  assert.ok(read2.ok && read2.pack.size === 2, 'a /2 pack reads');
  assert.deepEqual(read2.pack.get(PACK_A), VOICE_V, 'A: its profile and its vary, as they are');
  assert.deepEqual(P.profileFor({ speaker: PACK_B, traits }, read2.pack), read1(VOICE_B), 'B: vary 0/0');
  assert.deepEqual(P.profileFor({ speaker: PACK_C, traits }, read.pack), P.profileFor({ speaker: PACK_C, traits }), 'not in it: the default');
  assert.ok(P.readPack(await voicePack({})).ok, 'an empty pack reads');
  assert.match(P.RANGES_REVISION, /^[0-9a-f]{8}$/, 'the revision: 8 hex digits');
  const a = (change) => {
    const p = structuredClone(good);
    change(p, p.profiles[PACK_A]);
    return p;
  };
  const refused = [
    ['format', null], ['format', []], ['format', 'x'], ['format', a((p) => { p.format = 'u6.voice-pack/3'; })],
    ['shape', a((p) => { p.format = P.PACK_FORMAT_2; })], ['keys', a((p, v) => { v.vary = NO_VARY; })],
    ['keys', a((p) => { p[PACK_SENTINEL] = 1; })], ['keys', a((p, v) => { v[PACK_SENTINEL] = 1; })],
    ['keys', a((p, v) => { v.mix[0][PACK_SENTINEL] = 1; })],
    ['shape', a((p) => { delete p.model; })], ['shape', a((p, v) => { delete v.seedSalt; })],
    ['shape', a((p) => { p.profiles = [VOICE_A]; })], ['shape', a((p) => { p.profiles.x = VOICE_B; })],
    ['shape', a((p) => { p.profiles['01'] = VOICE_B; })], ['shape', a((p) => { p.profiles[65536] = VOICE_B; })],
    ['shape', a((p) => { p.profiles['-1'] = VOICE_B; })], ['shape', a((p, v) => { v.mix = 'F1'; })],
    ['shape', a((p, v) => { v.mix[0] = 'F4'; })], ['shape', a((p) => { p.model = 7; })],
    ['revision', a((p) => { p.revision = p.revision === 'ffffffff' ? '00000000' : 'ffffffff'; })],
    ['range', a((p, v) => { v.speed = 1.5; })], ['range', a((p, v) => { v.pitch = -2.5; })], ['range', a((p, v) => { v.steps = 17; })],
    ['range', a((p, v) => { v.steps = 1.5; })], ['range', a((p, v) => { v.seedSalt = -1; })], ['range', a((p, v) => { v.speed = '1'; })],
    ['range', a((p, v) => { v.mix = []; })], ['range', a((p, v) => { v.mix[0].style = 'X9'; })],
    ['range', a((p, v) => { v.mix[0].w = 0; })], ['range', a((p, v) => { v.mix[0].w = 0.5; })],
    ['range', a((p, v) => { v.mix = [{ style: 'F1', w: 0.5 }, { style: 'F1', w: 0.5 }]; })],
    ['range', a((p, v) => { v.mix = ['F1', 'F2', 'F3', 'F5'].map((style) => ({ style, w: 0.25 })); })],
    ['range', a((p) => { for (let n = 0; n <= P.PACK_MAX; n += 1) p.profiles[n] = VOICE_B; })],
  ];
  // A /2 pack's vary: exactly {speed, pitch}, each finite, 0 to VARY's.
  const v2 = (vary) => {
    const p = structuredClone(good2);
    p.profiles[PACK_A].vary = vary;
    return p;
  };
  refused.push(
    ['shape', v2(undefined)], ['shape', v2(null)], ['shape', v2([0, 0])], ['shape', v2({ speed: 0 })],
    ['keys', v2({ ...NO_VARY, [PACK_SENTINEL]: 0 })],
    ['range', v2({ speed: -0.0625, pitch: 0 })], ['range', v2({ speed: P.VARY.speed.max + 0.0625, pitch: 0 })],
    ['range', v2({ speed: 0, pitch: -0.5 })], ['range', v2({ speed: 0, pitch: P.VARY.pitch.max + 0.5 })],
    ['range', v2({ speed: '0', pitch: 0 })],
  );
  for (const [code, value] of refused) {
    const r = P.readPack(value);
    assert.ok(!r.ok && r.code === code, `refused as ${code}`);
    assert.ok(!JSON.stringify(r).includes(PACK_SENTINEL), 'the code names no key');
  }
  console.log(`  receipt: ${refused.length} malformed packs refused, each by its kind`);
});

// Before the page's scripts (and before voiceProbe, which wraps it): a voice Worker that runs no
// model, answering a load with ready (on the CPU) and each line with done, so the player sends
// each line with its profile (the probe records it) without the 400 MB model. While the page's
// `window.__fakeAudio` is set, one sample of audio goes before each done, so the player counts
// the line spoken (data-voice-spoken, data-voice-distinct).
function fakeVoiceWorker() {
  window.Worker = class FakeVoiceWorker extends EventTarget {
    postMessage(m) {
      const reply = (data) => setTimeout(() => {
        const e = new MessageEvent('message', { data });
        this.dispatchEvent(e);
        this.onmessage?.(e);
      }, 0);
      if (m?.type === 'load') reply({ type: 'ready', ms: 1, refetched: 0, path: 'cpu' });
      else if (m?.type === 'speak') {
        if (window.__fakeAudio) reply({ type: 'audio', id: m.id, pcm: new Float32Array(1), sampleRate: 44100, synthMs: 0, audioSec: 1 });
        reply({ type: 'done', id: m.id });
      }
    }

    terminate() {}
  };
}

test('voice pack on the page (synthetic): a dropped voices.u6vp gives its actors their profiles (the lines sent to the worker carry the pack\'s), the voices kept for the session forgotten; kept under voice-pack: and back after a reload; told by its format under another name (/1 or /2); a /2 pack\'s varying voice says each line at lineProfile\'s speed and pitch (two lines apart, one line again alike); a malformed one, one too large and one with other files are refused with a content-free line and change nothing; storage that refuses it keeps none; Forget voices removes it; the sentinel and the actors never reach the page\'s data, text, storage or console', async (env) => {
  const P = await import('../../site/voice/profile.js');
  const { context, page } = await h.openPage(env);
  await context.addInitScript(fakeVoiceWorker);
  await context.addInitScript(voiceProbe);
  const text = consoleText(page);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  assert.equal((await voicesOn(page)).state, 'ready', 'voices on (no model)');
  const traits = { type: 0, worktype: 0, alignment: 0 };
  let turn = 0;
  // What the worker is sent for `line` of `speaker`, in a turn of its own.
  async function speakSent(speaker, line = 'A line of the test.') {
    turn += 1;
    const n = await page.evaluate(() => window.__speaks.length);
    await page.evaluate((u) => window.__speech.push(u), { ...utterance(turn, line), speaker, traits });
    await waitVoice(page, (k) => window.__speaks.length > k, n, 30_000);
    return page.evaluate(() => window.__speaks.at(-1));
  }
  // The profile the worker is sent for a line of `speaker`.
  const profileSent = async (speaker) => (await speakSent(speaker)).profile;
  const drop = async (name, body) => {
    await h.dropFiles(page, [], [{ name, text: body }]);
    await h.waitSettled(page);
    return (await h.state(page)).status;
  };
  const shownPack = () => page.evaluate(() => document.body.dataset.voicePack);
  const defaultOf = (speaker) => P.profileFor({ speaker, traits });
  assert.deepEqual(await profileSent(PACK_A), defaultOf(PACK_A), 'before the pack: the default voice, kept for the session');
  assert.equal(await shownPack(), '0');

  // The pack: A and B take its voices, the voice kept for A forgotten; C keeps the default.
  const pack = await voicePack({ [PACK_A]: VOICE_A, [PACK_B]: VOICE_B });
  assert.equal(await drop('voices.u6vp', JSON.stringify(pack)), await line('voicePack', 2), 'a count only');
  assert.equal(await shownPack(), '2');
  assert.deepEqual(await profileSent(PACK_A), read1(VOICE_A), 'A: the pack\'s voice (the session\'s pin forgotten)');
  assert.deepEqual(await profileSent(PACK_B), read1(VOICE_B), 'B: the pack\'s voice');
  assert.deepEqual(await profileSent(PACK_C), defaultOf(PACK_C), 'C, not in it: the default');
  let kept = await h.keptVoicePacks(page);
  assert.deepEqual(kept.keys, ['voice-pack:voices'], 'kept under its key');
  assert.deepEqual(kept.byKey['voice-pack:voices'], pack, 'the pack kept');

  // Refused, nothing changed: the pack in use and the one kept stay.
  const big = JSON.stringify(pack).replace('{', `{${' '.repeat(1 << 20)}`); // over 1 MiB
  for (const [name, body, why] of [
    ['voices.u6vp', JSON.stringify({ ...pack, [PACK_SENTINEL]: 1 }), 'voicePackErrKeys'],
    ['voices.u6vp', JSON.stringify(await voicePack({ [PACK_A]: { ...VOICE_A, [PACK_SENTINEL]: 1 } })), 'voicePackErrKeys'],
    ['voices.u6vp', JSON.stringify(await voicePack({ [PACK_A]: { ...VOICE_A, speed: 2 } })), 'voicePackErrRange'],
    ['voices.u6vp', JSON.stringify(await voicePack({ [PACK_SENTINEL]: VOICE_A })), 'voicePackErrShape'],
    ['voices.u6vp', JSON.stringify({ ...pack, revision: 'ffffffff' === P.RANGES_REVISION ? '00000000' : 'ffffffff' }), 'voicePackErrRevision'],
    ['voices.u6vp', JSON.stringify({ ...pack, format: 'u6.voice-pack/3' }), 'voicePackErrFormat'],
    ['voices.u6vp', JSON.stringify({ ...pack, format: P.PACK_FORMAT_2 }), 'voicePackErrShape'],
    ['voices.u6vp', JSON.stringify(await voicePack2({ [PACK_A]: { ...VOICE_V, vary: { speed: 0.5, pitch: 0 } } })), 'voicePackErrRange'],
    ['voices.u6vp', `not JSON ${PACK_SENTINEL}`, 'voicePackErrJson'],
    ['voices.u6vp', big, 'voicePackErrLarge'],
  ]) {
    assert.equal(await drop(name, body), await line('voicePackRefused', await line(why)), `${why}: refused, by its kind`);
    assert.equal(await shownPack(), '2', `${why}: the pack in use stays`);
  }
  await h.dropFiles(page, [], [{ name: 'voices.u6vp', text: JSON.stringify(pack) }, MARKER]);
  await h.waitSettled(page);
  assert.equal((await h.state(page)).status, await line('dropVoicePackAndFiles'), 'with other files: refused');
  assert.equal((await h.state(page)).state, 'running', 'the game runs on');
  kept = await h.keptVoicePacks(page);
  assert.deepEqual(kept.byKey['voice-pack:voices'], pack, 'the pack kept stays');
  assert.deepEqual(await profileSent(PACK_A), read1(VOICE_A), 'A: still the pack\'s');

  // Told by its format, under another name: B's voice for A now.
  const other = await voicePack({ [PACK_A]: VOICE_B });
  assert.equal(await drop('renamed.json', `\n  ${JSON.stringify(other)}`), await line('voicePack', 1), 'another name: by its format');
  assert.deepEqual(await profileSent(PACK_A), read1(VOICE_B), 'A: the new pack\'s');
  assert.deepEqual(await profileSent(PACK_B), defaultOf(PACK_B), 'B, not in the new pack: the default');

  // A /2 pack, told by its format under another name too: A's voice varies by line. Each line
  // reaches the worker at lineProfile's speed and pitch from the line's seed (the speaker's and
  // the text's: the seed sent, its salt taken out), two lines apart, one line again alike.
  const varying = await voicePack2({ [PACK_A]: VOICE_V, [PACK_B]: read1(VOICE_B) });
  assert.equal(await drop('varying.json', JSON.stringify(varying)), await line('voicePack', 2), 'a /2 pack under another name: by its format');
  assert.equal(await shownPack(), '2');
  // Spoken (one sample each): the page counts A's voice once, however many profiles its lines
  // are said in.
  await page.evaluate(() => { window.__fakeAudio = true; });
  const heard = await voiceShown(page);
  const lines = [];
  for (const text of ['The first line of the test.', 'Another line of the test.', 'A third line of the test.', 'The first line of the test.']) {
    const sent = await speakSent(PACK_A, text);
    await waitVoice(page, (n) => Number(document.body.dataset.voiceSpoken) > n, heard.spoken + lines.length, 10_000);
    const lineSeed = (sent.seed ^ VOICE_V.seedSalt) >>> 0;
    assert.deepEqual(sent.profile, P.lineProfile(VOICE_V, lineSeed), 'the line\'s profile: lineProfile\'s');
    assert.deepEqual(sent.profile.mix, VOICE_V.mix, 'its voice\'s presets');
    assert.ok(Math.abs(sent.profile.speed - VOICE_V.speed) <= VOICE_V.vary.speed && Math.abs(sent.profile.pitch - VOICE_V.pitch) <= VOICE_V.vary.pitch, 'within its vary');
    lines.push(sent);
  }
  await page.evaluate(() => { window.__fakeAudio = false; });
  assert.ok(lines[0].profile.speed !== lines[1].profile.speed && lines[0].profile.pitch !== lines[1].profile.pitch, 'two lines: two speeds and pitches');
  assert.equal(new Set(lines.map((l) => JSON.stringify(l.profile))).size, 3, 'three lines\' profiles');
  assert.deepEqual([lines[3].seed, lines[3].profile], [lines[0].seed, lines[0].profile], 'the same line again: alike');
  const spoken = await voiceShown(page);
  console.log(`  receipt: a varying voice's ${spoken.spoken - heard.spoken} lines spoken in ${new Set(lines.map((l) => JSON.stringify(l.profile))).size} profiles: distinct voices +${spoken.distinct - heard.distinct}`);
  assert.equal(spoken.spoken, heard.spoken + 4, 'four lines spoken');
  assert.equal(spoken.distinct, heard.distinct + 1, 'one voice more, not one per line\'s profile');
  assert.deepEqual(await profileSent(PACK_B), read1(VOICE_B), 'B, vary 0/0: its voice as it is');
  // The same /2 pack as voices.u6vp (as the tool writes it): its varying voice again.
  assert.equal(await drop('voices.u6vp', JSON.stringify(varying)), await line('voicePack', 2), 'a /2 voices.u6vp');
  const again = await speakSent(PACK_A, 'Another line of the test.');
  assert.deepEqual(again.profile, lines[1].profile, 'the line\'s profile as before');
  assert.deepEqual((await h.keptVoicePacks(page)).byKey['voice-pack:voices'], varying, 'the /2 pack kept');
  // The /1 pack again, for what follows.
  assert.equal(await drop('renamed.json', JSON.stringify(other)), await line('voicePack', 1));

  // Storage that refuses it: in use for this visit, none kept.
  await page.evaluate(() => window.__fail.add('voicepack-store'));
  assert.equal(await drop('voices.u6vp', JSON.stringify(pack)), await line('voicePackNotKept', 2, await line('voicePackOldGone')));
  assert.deepEqual((await h.keptVoicePacks(page)).keys, [], 'none kept');
  assert.deepEqual(await profileSent(PACK_B), read1(VOICE_B), 'in use for this visit');
  await page.evaluate(() => window.__fail.delete('voicepack-store'));
  assert.equal(await drop('voices.u6vp', JSON.stringify(pack)), await line('voicePack', 2));
  const stored = await h.log(page);
  assert.ok(['voicepack:store:end', 'voicepack:store:fail', 'voicepack:forget:end'].every((e) => stored.includes(e)), 'through the store');

  // A reload: the kept pack is back before anyone speaks.
  await page.reload();
  await started(page, FILES);
  await waitVoice(page, () => document.body.dataset.voice === 'ready', null, 30_000);
  await page.click('#mute');
  await page.click('#mute');
  await waitVoice(page, () => document.body.dataset.voiceGain === '1.00', null, 30_000);
  assert.equal(await shownPack(), '2', 'back after a reload');
  turn = 0;
  assert.deepEqual(await profileSent(PACK_A), read1(VOICE_A), 'A: the kept pack\'s');

  // Forget voices: the pack goes with the model, off the player and out of storage.
  await forgetVia(page, '#forget-voices');
  await waitVoice(page, (want) => document.getElementById('status').textContent === want, await line('voiceForgot'), 10_000);
  assert.deepEqual((await h.keptVoicePacks(page)).keys, [], 'forgotten');
  assert.equal(await shownPack(), '0');
  assert.equal((await voicesOn(page)).state, 'ready', 'voices on again');
  assert.deepEqual(await profileSent(PACK_A), defaultOf(PACK_A), 'A: the default again');
  const log = await h.log(page);
  assert.ok(log.includes('voicepack:load:1') && log.includes('voicepack:forget:end'), 'restored, then forgotten, through the store');

  // Nothing of it reaches the page: the sentinel, the actors, the salt, a pack's weights.
  const sentinels = [PACK_SENTINEL, String(PACK_A), String(PACK_B), String(VOICE_A.seedSalt), String(VOICE_A.pitch), String(VOICE_B.pitch),
    String(VOICE_V.seedSalt), String(VOICE_V.vary.speed)];
  const where = await page.evaluate(() => JSON.stringify({
    body: { ...document.body.dataset }, canvas: { ...document.getElementById('screen').dataset },
    text: document.body.innerText, storage: { ...localStorage }, csp: window.__csp,
  }));
  assert.ok(sentinels.every((s) => !where.includes(s)), 'not in the page\'s data, text or storage');
  assert.ok(sentinels.every((s) => !text.some((m) => m.includes(s))), 'not in the console');
  assert.deepEqual(await page.evaluate(() => window.__csp), [], 'no CSP report');
}, { needs: h.haveHfes });

test('Forget voices removes the voice pack even when the model\'s caches cannot be removed (their list refused, or a delete): the line says not everything was removed, the pack is off the page and out of storage, and a reload does not bring it back', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  const pack = JSON.stringify(await voicePack({ [PACK_A]: VOICE_A, [PACK_B]: VOICE_B }));
  const dropPack = async () => {
    await h.dropFiles(page, [], [{ name: 'voices.u6vp', text: pack }]);
    await h.waitSettled(page);
    assert.equal((await h.state(page)).status, await line('voicePack', 2), 'the pack loaded');
    assert.deepEqual((await h.keptVoicePacks(page)).keys, ['voice-pack:voices'], 'and kept');
  };
  const failed = await line('voiceForgetFailed');
  for (const [what, refuse] of [
    ['the caches\' list refused', () => { caches.keys = () => Promise.reject(new DOMException('test: refused', 'SecurityError')); }],
    ['a cache\'s delete refused', async () => {
      await caches.open('u6-voice-test');
      caches.delete = () => Promise.reject(new DOMException('test: refused', 'SecurityError'));
    }],
  ]) {
    await dropPack();
    await page.evaluate(refuse);
    await forgetVia(page, '#forget-voices');
    await waitVoice(page, (want) => document.getElementById('status').textContent === want, failed, 10_000);
    await h.waitSettled(page);
    assert.deepEqual((await h.keptVoicePacks(page)).keys, [], `${what}: the pack out of storage`);
    assert.equal(await page.evaluate(() => document.body.dataset.voicePack), '0', `${what}: and off the page`);
    await page.evaluate(() => { delete caches.keys; delete caches.delete; });
  }
  await page.reload();
  await h.waitSettled(page);
  assert.equal(await page.evaluate(() => document.body.dataset.voicePack), '0', 'a reload does not bring it back');
  assert.ok((await h.log(page)).includes('voicepack:load:0'), 'none kept');
});

// --- the site's voice pack (spec 09 Design 4, as spec 08 K6's Korean pack) -------------------
// The site's listing naming a voice pack, served from memory by the test server (sitePacks); the
// packs are the tests' own (made-up actors and profiles), under made-up file names.

const VOICES_FILE = 'voices-test-1.u6vp';
const VOICES_NEWER = 'voices-test-2.u6vp';
const voicesListing = (file) => JSON.stringify({ packs: [], ...(file ? { voices: { file, label: 'Voices (test)' } } : {}) });
// The requests the page made for a voice pack of the site (the listing's are not).
const voiceRequests = (env) => env.packRequests.filter(([path]) => path.endsWith('.u6vp'));
// Waits until nothing is queued and the page's voice pack has `n` voices (data-voice-pack).
const voicePackIs = (page, n) => page.waitForFunction((want) => document.body.dataset.ops === '0'
  && document.body.dataset.voicePack === want, String(n), { timeout: 30_000 });

// Nothing of the tests' packs (the sentinel, the actors, the salt, the pitches) in the page's
// data, text or storage, nor in the console.
async function noVoicePackValues(page, words) {
  const where = await pageWords(page);
  for (const s of [PACK_SENTINEL, String(PACK_A), String(PACK_B), String(VOICE_A.seedSalt), String(VOICE_A.pitch), String(VOICE_B.pitch)]) {
    assert.ok(!where.includes(s), 'nothing of the pack in the page\'s data, text or storage');
    assert.ok(!words.some((m) => m.includes(s)), 'nor in the console');
  }
}

test('the site\'s voice pack (synthetic, no game): with a listing that names one, the page downloads it once (no key) and loads it as a dropped one (data-voice-pack its count, kept under voice-pack: with the file it came from), with no line of its own; a reload with the same listing downloads nothing; a listing that names another file downloads and loads that one', async (env) => {
  const pack = await voicePack({ [PACK_A]: VOICE_A, [PACK_B]: VOICE_B });
  const newer = await voicePack({ [PACK_A]: VOICE_B });
  sitePacks(env, { 'index.json': voicesListing(VOICES_FILE), [VOICES_FILE]: JSON.stringify(pack) });
  const { page } = await h.openPage(env);
  const words = consoleText(page);
  await page.goto(env.server.url);
  await voicePackIs(page, 2);
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200], [`/packs/${VOICES_FILE}`, 200]],
    'the listing, then the pack, each once');
  assert.equal((await h.state(page)).status, await line('noFilesYet'), 'no line of its own');
  let kept = await h.keptVoicePacks(page);
  assert.deepEqual(kept.keys, ['voice-pack:source', 'voice-pack:voices'], 'kept under voice-pack:');
  assert.deepEqual(kept.byKey['voice-pack:voices'], pack, 'the pack kept');
  assert.equal(kept.byKey['voice-pack:source'], VOICES_FILE, 'with the file it came from');
  env.packRequests.length = 0;
  await page.reload();
  await voicePackIs(page, 2);
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200]], 'a reload: the listing only, the kept pack loaded');
  // A newer pack put on the site under another name: the next visit takes it, unasked.
  sitePacks(env, { 'index.json': voicesListing(VOICES_NEWER), [VOICES_NEWER]: JSON.stringify(newer) });
  await page.reload();
  await voicePackIs(page, 1);
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200], [`/packs/${VOICES_NEWER}`, 200]],
    'the other file downloaded, once');
  kept = await h.keptVoicePacks(page);
  assert.deepEqual(kept.byKey['voice-pack:voices'], newer, 'kept in place of the one before');
  assert.equal(kept.byKey['voice-pack:source'], VOICES_NEWER, 'with its file');
  await noVoicePackValues(page, words);
  console.log(`  receipt: voice pack requests ${voiceRequests(env).length} after the newer listing, 1 visit with no download: pass`);
});

test('the site\'s voice pack and a dropped one (synthetic, no game, in 한글): a dropped voice pack is never replaced by the listed one (later visits download nothing); after Forget voices the listed one is not downloaded again in the visit; the next visit downloads it', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const listedPack = await voicePack({ [PACK_A]: VOICE_A, [PACK_B]: VOICE_B });
  const dropped = await voicePack({ [PACK_A]: VOICE_B });
  sitePacks(env, { 'index.json': voicesListing(VOICES_FILE), [VOICES_FILE]: JSON.stringify(listedPack) });
  const { page } = await h.openPage(env, { setup: { lang: 'ko' } });
  const words = consoleText(page);
  await h.load(page, env);
  await h.dropFiles(page, [], [{ name: 'voices.u6vp', text: JSON.stringify(dropped) }]);
  await voicePackIs(page, 1);
  // The drop came after the first visit's restore had downloaded the listed pack.
  assert.deepEqual(voiceRequests(env), [[`/packs/${VOICES_FILE}`, 200]], 'the first visit downloaded the listed pack');
  env.packRequests.length = 0;
  await page.reload();
  await voicePackIs(page, 1);
  await page.reload();
  await voicePackIs(page, 1);
  assert.deepEqual(voiceRequests(env), [], 'the dropped pack stays: nothing downloaded');
  let kept = await h.keptVoicePacks(page);
  assert.deepEqual(kept.keys, ['voice-pack:voices'], 'kept with no source');
  assert.deepEqual(kept.byKey['voice-pack:voices'], dropped, 'the dropped pack stays kept');
  await forgetVia(page, '#forget-voices');
  await waitVoice(page, (want) => document.getElementById('status').textContent === want, TEXT.ko.voiceForgot, 10_000);
  await h.waitSettled(page);
  assert.equal(await page.evaluate(() => document.body.dataset.voicePack), '0', 'forgotten');
  assert.deepEqual((await h.keptVoicePacks(page)).keys, [], 'and out of storage');
  await page.click('#settings');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  assert.deepEqual(voiceRequests(env), [], 'after Forget voices: not downloaded in the visit');
  await page.reload();
  await voicePackIs(page, 2);
  assert.deepEqual(voiceRequests(env), [[`/packs/${VOICES_FILE}`, 200]], 'the next visit after a Forget loads it');
  kept = await h.keptVoicePacks(page);
  assert.deepEqual([kept.byKey['voice-pack:voices'], kept.byKey['voice-pack:source']], [listedPack, VOICES_FILE], 'kept with its file');
  await noVoicePackValues(page, words);
  console.log('  receipt: a dropped voice pack kept over the listed one, 0 downloads; after Forget voices 0 in the visit, 1 the next visit: pass');
});

test('the site\'s voice pack, then a dropped one (synthetic, no game): the drop keeps no source, so the next visit with a listing that names another file downloads nothing and the dropped pack stays', async (env) => {
  const listedPack = await voicePack({ [PACK_A]: VOICE_A, [PACK_B]: VOICE_B });
  const dropped = await voicePack({ [PACK_A]: VOICE_B });
  sitePacks(env, { 'index.json': voicesListing(VOICES_FILE), [VOICES_FILE]: JSON.stringify(listedPack) });
  const { page } = await h.openPage(env);
  await page.goto(env.server.url);
  await voicePackIs(page, 2);
  await h.dropFiles(page, [], [{ name: 'voices.u6vp', text: JSON.stringify(dropped) }]);
  await voicePackIs(page, 1);
  let kept = await h.keptVoicePacks(page);
  assert.deepEqual(kept.keys, ['voice-pack:voices'], 'the drop kept in place of the site\'s pack, with no source');
  assert.deepEqual(kept.byKey['voice-pack:voices'], dropped, 'the dropped pack kept');
  sitePacks(env, { 'index.json': voicesListing(VOICES_NEWER), [VOICES_NEWER]: JSON.stringify(listedPack) });
  await page.reload();
  await voicePackIs(page, 1);
  assert.deepEqual(voiceRequests(env), [], 'a listing that names another file: nothing downloaded');
  kept = await h.keptVoicePacks(page);
  assert.deepEqual([kept.keys, kept.byKey['voice-pack:voices']], [['voice-pack:voices'], dropped], 'the dropped pack stays kept');
  console.log('  receipt: the site\'s voice pack then a drop, a renamed listing, 0 downloads: pass');
});

test('the site\'s voice pack not loaded (synthetic, no game): a listing that names none asks for no pack; an entry that is not a plain *.u6vp name is not asked for; one the site does not have (404), one that is not JSON, one the check refuses and one over the limit change nothing (the pack from before stays in use and kept, with its source), each warns with its kind only and is asked once only in the visit', async (env) => {
  const pack = await voicePack({ [PACK_A]: VOICE_A, [PACK_B]: VOICE_B });
  sitePacks(env, { 'index.json': voicesListing(null) });
  let { context, page } = await h.openPage(env);
  await h.load(page, env);
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200]], 'a listing with no voice pack: the listing only');
  assert.equal(await page.evaluate(() => document.body.dataset.voicePack), '0', 'no pack');
  await context.close();
  sitePacks(env, { 'index.json': voicesListing(VOICES_FILE), [VOICES_FILE]: JSON.stringify(pack) });
  ({ context, page } = await h.openPage(env));
  const words = consoleText(page);
  await page.goto(env.server.url);
  await voicePackIs(page, 2);
  for (const file of ['../voices-up.u6vp', 'voices-test.json']) {
    sitePacks(env, { 'index.json': voicesListing(file), [file]: JSON.stringify(pack) });
    await page.reload();
    await voicePackIs(page, 2);
    assert.deepEqual(env.packRequests, [['/packs/index.json', 200]], 'not a plain *.u6vp name: the listing only');
  }
  const big = JSON.stringify(pack).replace('{', `{${' '.repeat(1 << 20)}`); // over 1 MiB
  const cases = [
    ['a 404', 'voices-gone.u6vp', undefined, 'the site\'s voice pack was not downloaded: HTTP 404'],
    ['not JSON', 'voices-bad-1.u6vp', `not JSON ${PACK_SENTINEL}`, 'the site\'s voice pack is not used: json'],
    ['a key the check refuses', 'voices-bad-2.u6vp', JSON.stringify({ ...pack, [PACK_SENTINEL]: 1 }), 'the site\'s voice pack is not used: keys'],
    ['over the limit', 'voices-bad-3.u6vp', big, 'the site\'s voice pack is not used: large'],
  ];
  for (const [what, file, body, warned] of cases) {
    sitePacks(env, { 'index.json': voicesListing(file), ...(body === undefined ? {} : { [file]: body }) });
    await page.reload();
    await voicePackIs(page, 2);
    assert.deepEqual(voiceRequests(env), [[`/packs/${file}`, body === undefined ? 404 : 200]], `${what}: asked once`);
    assert.equal((await h.state(page)).status, await line('noFilesYet'), `${what}: no line`);
    const kept = await h.keptVoicePacks(page);
    assert.deepEqual([kept.byKey['voice-pack:voices'], kept.byKey['voice-pack:source']], [pack, VOICES_FILE], `${what}: the pack from before stays kept, with its source`);
    assert.ok(words.includes(warned), `${what}: a warning that names its kind only`);
    await page.click('#settings');
    await page.click('#setup-ok');
    await h.waitSettled(page);
    assert.equal(voiceRequests(env).length, 1, `${what}: not asked again in the visit`);
  }
  await noVoicePackValues(page, words);
  console.log(`  receipt: no voice pack listed 0 pack requests, 2 entries of another form 0 pack requests, ${cases.length} refusals 1 request each, nothing changed: pass`);
});

// --- NPC bundles an older page kept (spec 09 Design 5) --------------------------------------

// FNV-1a 64 of `bytes` as 16 lower-case hex digits (the catalog's and the bundles' hash).
function fnv64(bytes) {
  let h = 0xcbf29ce484222325n;
  for (const b of bytes) h = ((h ^ BigInt(b)) * 0x100000001b3n) & 0xffffffffffffffffn;
  return h.toString(16).padStart(16, '0');
}

// Puts made-up records where an older page kept its NPC bundles (npc-bundle:<archive>/<item>).
const seedBundles = (page, n) => page.evaluate((count) => new Promise((done, fail) => {
  const req = indexedDB.open('ultima6-port', 1);
  req.onerror = () => fail(req.error);
  req.onsuccess = () => {
    const db = req.result;
    const tx = db.transaction('files', 'readwrite');
    for (let i = 1; i <= count; i += 1) tx.objectStore('files').put({ format: 'u6.npc-bundle/1', archive: 'a', item: i }, `npc-bundle:a/${i}`);
    tx.oncomplete = () => { db.close(); done(); };
    tx.onerror = () => fail(tx.error);
  };
}), n);

test('NPC bundles an older page kept (made-up records under npc-bundle:) are removed at the restore, once and with no line; a later visit removes nothing again; the page has no NPC bundles or Game strings button', async (env) => {
  const { page } = await h.openPage(env);
  await h.load(page, env);
  assert.ok(await page.evaluate(() => ['npc-bundles', 'forget-npc-bundles', 'game-strings'].every((id) => !document.getElementById(id))),
    'no NPC bundles, Forget NPC bundles or Game strings button');
  await page.evaluate(() => localStorage.removeItem('u6.npcBundlesGone')); // as an older page left it
  await seedBundles(page, 3);
  assert.equal((await h.keptBundles(page)).keys.length, 3, 'the older page\'s records');
  await page.reload();
  await h.waitSettled(page);
  assert.equal((await h.keptBundles(page)).keys.length, 0, 'removed at the restore');
  assert.equal((await h.log(page)).filter((e) => e === 'bundles:forget:end').length, 1, 'once');
  assert.equal((await h.state(page)).status, await line('noFilesYet'), 'with no line');
  await seedBundles(page, 2);
  await page.reload();
  await h.waitSettled(page);
  assert.ok(!(await h.log(page)).some((e) => e.startsWith('bundles:')), 'a later visit: no removal');
  assert.equal((await h.keptBundles(page)).keys.length, 2, 'nothing touched');
});

// --- the Korean pack (spec 08, slice K1) ---------------------------------------------------

const KO_SENTINEL = 'Zq9PageSentinel';
const KO_LINE = '시험이로구려';
const KO_LOOK = 'a figure of tests';
const KO_GREET = `"Hail, ${KO_SENTINEL}! I am made for tests."`;
const KO_LOOK_KO = '시험 인형';
const KO_GREET_KO = `"어서 오시오, 그대여! 나는 ${KO_LINE}."`;

// A synthetic converse.a (no game bytes): items 1..0x62, every one the test's script `ff 02
// "Zed" f1 <look> f2 <greeting> b6`, stored raw, so any conversation archive `a` loads talks it;
// and the Korean pack over it, made with the translation tool's own writer (tools/i18n), each
// script's two runs in Korean. `other`: the pack's runs keyed to bytes that are not these.
// `items`: the archive's items (a smaller one makes a pack of other counts); `greeting` and
// `greetingKo`: the greeting and its Korean, KO_GREET's and KO_GREET_KO unless a test makes its own.
async function koFixture(items = 0x63, { greeting = KO_GREET, greetingKo = KO_GREET_KO } = {}) {
  const { fingerprint, hash64, hex64, latin1, packBytes } = await import('../i18n/translate.mjs');
  const script = [0xff, 0x02, ...latin1('Zed'), 0xf1, ...latin1(KO_LOOK), 0xf2, ...latin1(greeting), 0xb6];
  const table = [];
  const body = [];
  for (let i = 0; i < items; i += 1) {
    const at = i === 0 ? 0 : items * 4 + body.length;
    if (i > 0) body.push(0, 0, 0, 0, ...script);
    table.push(at & 255, (at >> 8) & 255, (at >> 16) & 255, 0);
  }
  const look = 6;
  const greet = look + KO_LOOK.length + 1;
  const runs = [];
  const named = [];
  for (let item = 1; item < items; item += 1) {
    named.push({ archive: 'a', item, hash: hex64(hash64(script)) });
    runs.push({ archive: 'a', item, offset: look, len: KO_LOOK.length, hash: hex64(hash64(latin1(KO_LOOK))), text: KO_LOOK_KO });
    runs.push({ archive: 'a', item, offset: greet, len: greeting.length, hash: hex64(hash64(latin1(greeting))), text: greetingKo });
  }
  const pack = [...packBytes({ base: fingerprint(named), runs })];
  return { archive: [...table, ...body], pack, runs: runs.length, scripts: items - 1 };
}

// The Hangul glyphs of `text` (site/fonts/galmuri7-hangul.bin, committed): one 8-row bitmap per
// distinct syllable.
async function hangulGlyphs(text) {
  const table = await readFile(join(h.SITE, 'fonts', 'galmuri7-hangul.bin'));
  const out = new Map();
  for (const c of text) {
    const code = c.codePointAt(0);
    if (code >= 0xac00 && code <= 0xd7a3) out.set(c, [...table.subarray((code - 0xac00) * 8, (code - 0xac00) * 8 + 8)]);
  }
  return out;
}

// Which of `glyphs` the message window of the game's RGBA frame shows as a cell (8 x 8 at a cell
// of the window, cols 0x16..0x26, rows 0x0e..0x17): each cell read as ink (a pixel of another
// colour than the window's most common one, its paper) and paper. Compared in the page's frame,
// never printed.
async function cellsShown(page, glyphs) {
  const rgba = await page.evaluate(() => [...window.__shownGame.frame_rgba()]);
  const px = (x, y) => rgba.slice((y * 320 + x) * 4, (y * 320 + x) * 4 + 3).join(',');
  const counts = new Map();
  for (let y = 0x0e * 8; y < 0x18 * 8; y += 1) for (let x = 0x16 * 8; x < 0x27 * 8; x += 1) counts.set(px(x, y), (counts.get(px(x, y)) ?? 0) + 1);
  const paper = [...counts].sort((a, b) => b[1] - a[1])[0][0];
  const found = new Set();
  let inked = 0;
  for (let row = 0x0e; row < 0x18; row += 1) {
    for (let col = 0x16; col < 0x27; col += 1) {
      const bits = [];
      for (let dy = 0; dy < 8; dy += 1) {
        let b = 0;
        for (let dx = 0; dx < 8; dx += 1) if (px(col * 8 + dx, row * 8 + dy) !== paper) b |= 0x80 >> dx;
        bits.push(b);
      }
      if (bits.some((b) => b !== 0)) inked += 1;
      for (const [c, g] of glyphs) if (g.every((b, i) => b === bits[i])) found.add(c);
    }
  }
  return { found, inked };
}

const localeShown = (page) => page.evaluate(() => {
  const c = document.getElementById('screen').dataset;
  return { pack: document.body.dataset.localePack, locale: c.locale, hits: Number(c.localeHits ?? 0), misses: Number(c.localeMisses ?? 0) };
});

// Everything a page shows or keeps outside its frame and its IndexedDB, as one text: its data,
// its text, its local storage, its CSP reports.
const pageWords = (page) => page.evaluate(() => JSON.stringify({
  body: { ...document.body.dataset }, canvas: { ...document.getElementById('screen').dataset },
  text: document.body.innerText, storage: { ...localStorage }, csp: window.__csp,
}));

test('Korean pack (synthetic, no game): a dropped ko.u6lp is checked by the engine\'s reader, kept under locale:ko and shown as counts; a reload brings it back; a malformed one, one over the limit and a drop of a pack with another file are refused with a line that names only the rule, nothing changed; storage that refuses it: for this visit only, the one before removed; Forget the Korean pack removes it; no line of it reaches the console, the page\'s data, text or storage', async (env) => {
  const { page } = await h.openPage(env, { setup: { lang: 'ko' } });
  const words = consoleText(page);
  await h.load(page, env);
  const { TEXT } = await import('../../site/i18n.js');
  const ko = TEXT.ko;
  const { pack, runs, scripts } = await koFixture();
  assert.equal((await localeShown(page)).pack, 'none');
  assert.equal(await localeNoteShown(page), await noteFor('ko'), 'no pack at all: the chooser\'s line pointing at the README');
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: pack }]);
  await h.waitSettled(page);
  assert.equal((await localeShown(page)).pack, `${runs},${scripts}`, 'its counts');
  assert.equal((await h.state(page)).status, `${ko.koPackLoaded(runs, scripts)}${ko.koPackKept}`, 'the line, counts only');
  assert.deepEqual(await h.keptLocalePack(page), pack, 'kept under locale:ko, byte for byte');
  assert.equal(await localeNoteShown(page), null, 'the line goes with a pack');
  assert.equal((await h.keptPaths(page)), null, 'no game file kept, no game started');
  await page.reload();
  await h.waitSettled(page);
  assert.equal((await localeShown(page)).pack, `${runs},${scripts}`, 'back after a reload');
  assert.ok((await h.log(page)).includes('locale:load:1'));
  // Refusals: nothing changes.
  const broken = pack.slice(0, pack.length - 3);
  const marked = [...Buffer.from(`U6LP\n{"format":"u6.locale-pack/1","language":"ko","tool":"${KO_SENTINEL}\u0001"}`)];
  for (const [bytes, line] of [
    [broken, ko.koPackRefused('its contents are not those of a Korean pack')],
    [marked, ko.koPackRefused('its contents are not those of a Korean pack')],
    [[...Buffer.from('PK\u0003\u0004 not a pack')], ko.koPackRefused('the file is not a Korean pack (it does not start with U6LP)')],
  ]) {
    await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes }]);
    await h.waitSettled(page);
    assert.equal((await h.state(page)).status, line, 'refused with the rule\'s line');
    assert.deepEqual(await h.keptLocalePack(page), pack, 'the kept pack as it was');
    assert.equal((await localeShown(page)).pack, `${runs},${scripts}`, 'and the page\'s');
  }
  await page.evaluate(() => { window.__bigName = 'big.u6lp'; });
  await h.dropFiles(page, [], [{ name: 'big.u6lp', bytes: [1] }]);
  await h.waitSettled(page);
  assert.equal((await h.state(page)).status, ko.koPackTooLarge(16), 'over the limit, before it is read');
  assert.ok(!(await h.log(page)).includes('read:start:big.u6lp'), 'not read');
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: pack }, MARKER]);
  await h.waitSettled(page);
  assert.equal((await h.state(page)).status, ko.dropKoPackAndFiles, 'a pack with another file: refused');
  // Storage that refuses the new pack: it works for this visit, and the one before goes.
  await page.evaluate(() => window.__fail.add('locale-store'));
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: pack }]);
  await h.waitSettled(page);
  assert.equal((await h.state(page)).status, `${ko.koPackLoaded(runs, scripts)}${ko.koPackNotKept(ko.koPackOldGone)}`);
  assert.equal(await h.keptLocalePack(page), null, 'the one kept before removed');
  assert.equal((await localeShown(page)).pack, `${runs},${scripts}`, 'on the page for this visit');
  await page.evaluate(() => window.__fail.delete('locale-store'));
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: pack }]);
  await h.waitSettled(page);
  await forgetVia(page, '#forget-ko');
  await h.waitSettled(page);
  assert.equal((await h.state(page)).status, ko.koPackForgot);
  assert.equal(await h.keptLocalePack(page), null, 'out of storage');
  assert.equal((await localeShown(page)).pack, 'none', 'and off the page');
  await page.reload();
  await h.waitSettled(page);
  assert.equal((await localeShown(page)).pack, 'none', 'not back after a reload');
  const where = await pageWords(page);
  for (const w of [KO_SENTINEL, KO_LINE, KO_GREET_KO]) {
    assert.ok(!where.includes(w), 'no line in the page\'s data, text or storage');
    assert.ok(!words.some((m) => m.includes(w)), 'nor in the console');
  }
  assert.deepEqual(await page.evaluate(() => window.__csp), [], 'no CSP report');
});

test('Korean pack in a game (the user\'s disks, a synthetic conversation): in 한글 a game started with a pack made for other files runs in English and says the conversation part differs; with the kept converse.a the test\'s own (in the page), the conversation shows the pack\'s Korean in the Hangul cells while every run it starts is the pack\'s; English at the chooser restarts it in English and leaves the save; Forget the Korean pack restarts it in English; no line reaches the console, the page\'s data or text', async (env) => {
  const { page } = await h.openPage(env, { setup: { lang: 'ko' } });
  const words = consoleText(page);
  await h.load(page, env);
  const { TEXT } = await import('../../site/i18n.js');
  const ko = TEXT.ko;
  const { archive, pack, runs, scripts } = await koFixture();
  const glyphs = await hangulGlyphs(KO_LOOK_KO + KO_GREET_KO);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  assert.equal((await localeShown(page)).locale, 'off', 'no pack: English');
  // The pack, over the user's own converse.a: the game restarts and refuses it.
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: pack }]);
  await h.waitSettled(page);
  await h.waitPrompt(page);
  assert.equal((await localeShown(page)).locale, 'mismatch', 'made for other files: not used');
  assert.ok((await h.state(page)).status.includes(ko.koPackMismatch('its conversation part was made from other game files')), 'the line names the part');
  // The test's converse.a in place of the user's, in the page; a reload starts from it.
  assert.equal(await h.replaceKept(page, 'converse.a', archive), 1, 'one archive replaced');
  await page.reload();
  await h.waitSettled(page);
  await h.waitPrompt(page);
  let shown = await localeShown(page);
  assert.deepEqual([shown.pack, shown.locale], [`${runs},${scripts}`, 'on'], 'the pack on the game');
  assert.ok((await h.state(page)).status.includes(ko.koPackOn(runs, scripts)), 'its line, counts only');
  const talkTo2 = async () => {
    await page.focus('#screen');
    await page.keyboard.press('t');
    await h.waitTaken(page);
    await page.keyboard.press('2');
    await h.waitTaken(page);
    await h.waitPrompt(page);
  };
  await talkTo2();
  shown = await localeShown(page);
  assert.deepEqual([shown.hits, shown.misses], [2, 0], 'the look text and the greeting from the pack, nothing missed');
  const korean = await cellsShown(page, glyphs);
  assert.equal(korean.found.size, glyphs.size, 'every syllable of the pack\'s lines in a cell of the message window');
  const saved = await h.savedFiles(page);
  // English at the chooser: the game restarts in English, the save as it was.
  await page.click('#settings');
  await page.check('input[name="setup-lang"][value="en"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await h.waitPrompt(page);
  assert.equal((await localeShown(page)).locale, 'off', 'English: the pack not on the game');
  h.assertSame(await h.savedFiles(page), saved, 'the save untouched');
  await talkTo2();
  let english = await cellsShown(page, glyphs);
  assert.ok(english.found.size === 0 && english.inked > 0, 'the conversation in English: no Korean cell');
  // 한글 again, then Forget the Korean pack: English again.
  await page.click('#settings');
  await page.check('input[name="setup-lang"][value="ko"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await h.waitPrompt(page);
  assert.equal((await localeShown(page)).locale, 'on', '한글: on again');
  await forgetVia(page, '#forget-ko');
  await h.waitSettled(page);
  await h.waitPrompt(page);
  shown = await localeShown(page);
  assert.deepEqual([shown.pack, shown.locale], ['none', 'off'], 'forgotten: English');
  assert.ok((await h.state(page)).status.startsWith(ko.koPackForgot), 'its line');
  await talkTo2();
  english = await cellsShown(page, glyphs);
  assert.ok(english.found.size === 0 && english.inked > 0, 'English again after the forget');
  const where = await pageWords(page);
  for (const w of [KO_SENTINEL, KO_LINE, KO_GREET_KO, KO_GREET]) {
    assert.ok(!where.includes(w), 'no line in the page\'s data, text or storage');
    assert.ok(!words.some((m) => m.includes(w)), 'nor in the console');
  }
}, { needs: h.haveHfes });

// --- the site's Korean pack (spec 08, slice K6) -------------------------------------------
// The site's listing (/packs/index.json) naming a Korean pack, served from memory by the test
// server (sitePacks); the packs are koFixture's (made-up lines, no game data), under made-up file
// names.

const KO_FILE = 'ko-test-1.u6lp';
const KO_NEWER = 'ko-test-2.u6lp';
const koListing = (file) => JSON.stringify({ packs: [], ...(file ? { ko: { file, label: 'Korean (test)' } } : {}) });
// The requests the page made for a Korean pack of the site (the listing's are not).
const koRequests = (env) => env.packRequests.filter(([path]) => path.endsWith('.u6lp'));
// Waits until nothing is queued and the page's Korean pack has `counts` (data-locale-pack).
const koPackIs = (page, counts) => page.waitForFunction((want) => document.body.dataset.ops === '0'
  && document.body.dataset.localePack === want, counts, { timeout: 30_000 });
const koSourceKept = (page) => h.keptLocaleSource(page);
// The chooser's note on the game's text (shown or not with the chooser), as `chooser` reads it.
const localeNoteShown = (page) => page.evaluate(() => {
  const p = document.getElementById('setup-note');
  return p.hidden ? null : p.textContent.replace(/\s+/g, ' ').trim();
});

// Nothing of the pack's lines in the page's data, text or storage, nor in the console.
async function noPackWords(page, words) {
  const where = await pageWords(page);
  for (const w of [KO_SENTINEL, KO_LINE, KO_GREET_KO]) {
    assert.ok(!where.includes(w), 'no line in the page\'s data, text or storage');
    assert.ok(!words.some((m) => m.includes(w)), 'nor in the console');
  }
}

test('the site\'s Korean pack (synthetic, no game): with a listing that names one, a page in 한글 downloads it once (no key) and loads it as a dropped one (its counts, its line, kept under locale:ko with the file it came from), the chooser\'s note saying it came from the site; OK and a reload with the same listing download nothing; a listing that names another file downloads and loads that one', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const ko = TEXT.ko;
  const first = await koFixture();
  const newer = await koFixture(0x11);
  sitePacks(env, { 'index.json': koListing(KO_FILE), [KO_FILE]: Uint8Array.from(first.pack) });
  const { page } = await h.openPage(env, { setup: { lang: 'ko' } });
  const words = consoleText(page);
  await page.goto(env.server.url);
  await koPackIs(page, `${first.runs},${first.scripts}`);
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200], [`/packs/${KO_FILE}`, 200]],
    'the listing, then the pack, each once');
  assert.equal((await h.state(page)).status, `${ko.koPackLoaded(first.runs, first.scripts)}${ko.koPackKept}\n${ko.noFilesYet}`,
    'the line of a dropped pack, counts only');
  assert.deepEqual(await h.keptLocalePack(page), first.pack, 'kept under locale:ko, byte for byte');
  assert.equal(await koSourceKept(page), KO_FILE, 'with the file it came from');
  assert.equal(await localeNoteShown(page), null, 'the line pointing at the tool goes');
  await page.click('#settings');
  let c = await chooser(page);
  assert.ok(c.shown && c.lang === 'ko', 'Settings opens the chooser, on 한글');
  assert.equal(c.note, null, 'with the site\'s pack the chooser has no note');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  assert.equal(koRequests(env).length, 1, 'OK in 한글: not downloaded again');
  env.packRequests.length = 0;
  await page.reload();
  await koPackIs(page, `${first.runs},${first.scripts}`);
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200]], 'a reload: the listing only, the kept pack loaded');
  c = await chooser(page);
  assert.ok(!c.shown && c.note === null, 'no chooser, and no note');
  // A newer pack put on the site under another name: the next visit takes it, unasked.
  sitePacks(env, { 'index.json': koListing(KO_NEWER), [KO_NEWER]: Uint8Array.from(newer.pack) });
  await page.reload();
  await koPackIs(page, `${newer.runs},${newer.scripts}`);
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200], [`/packs/${KO_NEWER}`, 200]],
    'the other file downloaded, once');
  assert.deepEqual(await h.keptLocalePack(page), newer.pack, 'kept in place of the one before');
  assert.equal(await koSourceKept(page), KO_NEWER, 'with its file');
  await noPackWords(page, words);
  console.log(`  receipt: pack requests ${koRequests(env).length} after the newer listing, 2 visits with no download: pass`);
});

test('the site\'s Korean pack and a dropped one (synthetic, no game): a dropped pack is never replaced by the listed one (a later visit downloads nothing); after Forget the listed one is not loaded in the visit, not at an OK in 한글, until the page goes to 한글 again; after another Forget the next visit loads it', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const ko = TEXT.ko;
  const listedPack = await koFixture();
  const dropped = await koFixture(0x11);
  sitePacks(env, { 'index.json': koListing(KO_FILE), [KO_FILE]: Uint8Array.from(listedPack.pack) });
  const { page } = await h.openPage(env, { setup: { lang: 'ko' } });
  const words = consoleText(page);
  await h.load(page, env);
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: dropped.pack }]);
  await koPackIs(page, `${dropped.runs},${dropped.scripts}`);
  // The drop came after the first visit's restore had loaded the listed pack.
  assert.deepEqual(koRequests(env), [[`/packs/${KO_FILE}`, 200]], 'the first visit downloaded the listed pack');
  env.packRequests.length = 0;
  await page.reload();
  await koPackIs(page, `${dropped.runs},${dropped.scripts}`);
  assert.deepEqual(koRequests(env), [], 'the dropped pack stays: nothing downloaded');
  assert.deepEqual(await h.keptLocalePack(page), dropped.pack, 'and stays kept');
  assert.equal((await chooser(page)).note, null, 'a dropped pack: no note');
  await forgetVia(page, '#forget-ko');
  await h.waitSettled(page);
  assert.equal((await localeShown(page)).pack, 'none', 'forgotten');
  assert.equal(await localeNoteShown(page), null, 'the site still lists one: no note');
  await page.click('#settings');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  assert.deepEqual(koRequests(env), [], 'after Forget, OK in 한글: still nothing downloaded');
  await page.click('#settings');
  await page.check('input[name="setup-lang"][value="en"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await page.click('#settings');
  await page.check('input[name="setup-lang"][value="ko"]');
  await page.click('#setup-ok');
  await koPackIs(page, `${listedPack.runs},${listedPack.scripts}`);
  assert.deepEqual(koRequests(env), [[`/packs/${KO_FILE}`, 200]], '한글 again: the listed pack downloaded');
  assert.equal(await koSourceKept(page), KO_FILE);
  await forgetVia(page, '#forget-ko');
  await h.waitSettled(page);
  assert.equal(await koSourceKept(page), null, 'Forget removes its source too');
  env.packRequests.length = 0;
  await page.reload();
  await koPackIs(page, `${listedPack.runs},${listedPack.scripts}`);
  assert.deepEqual(koRequests(env), [[`/packs/${KO_FILE}`, 200]], 'the next visit after a Forget loads it');
  await noPackWords(page, words);
  console.log('  receipt: a dropped pack kept over the listed one, 0 downloads; after Forget 0, then 1 at 한글 again, 1 the next visit: pass');
});

test('the site\'s Korean pack not loaded (synthetic, no game): a listing that names none asks for no pack; in English no Korean pack is asked for; one the site does not have (404) and one that does not read leave the game\'s text English with the page\'s line for that case, asked once only in the visit', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const ko = TEXT.ko;
  const { pack } = await koFixture();
  sitePacks(env, { 'index.json': koListing(null) });
  let { context, page } = await h.openPage(env, { setup: { lang: 'ko' } });
  await h.load(page, env);
  await page.click('#settings');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200]], 'a listing with no Korean pack: the listing only');
  assert.equal((await localeShown(page)).pack, 'none');
  assert.equal(await localeNoteShown(page), await noteFor('ko'), 'no Korean pack at all: the chooser\'s note points at the README');
  await context.close();
  sitePacks(env, { 'index.json': koListing(KO_FILE), [KO_FILE]: Uint8Array.from(pack) });
  ({ context, page } = await h.openPage(env, { setup: { lang: 'en' } }));
  await page.goto(env.server.url);
  await h.waitSettled(page);
  await page.click('#settings');
  await listed(page, 0);
  await page.click('#setup-ok');
  await h.waitSettled(page);
  assert.deepEqual(koRequests(env), [], 'English: no Korean pack asked for');
  assert.equal((await localeShown(page)).pack, 'none');
  await context.close();
  // A listing that names a file the site does not have.
  sitePacks(env, { 'index.json': koListing('ko-gone.u6lp') });
  ({ context, page } = await h.openPage(env, { setup: { lang: 'ko' } }));
  const words = consoleText(page);
  await page.goto(env.server.url);
  await h.waitSettled(page);
  assert.deepEqual(koRequests(env), [['/packs/ko-gone.u6lp', 404]], 'asked once');
  assert.equal((await localeShown(page)).pack, 'none', 'no pack');
  assert.equal(await h.keptLocalePack(page), null, 'none kept');
  assert.equal((await h.state(page)).status, ko.noFilesYet, 'nothing else changes');
  assert.equal(await localeNoteShown(page), null, 'one is listed: no note');
  await page.click('#settings');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await page.click('#settings');
  await page.check('input[name="setup-lang"][value="en"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await page.click('#settings');
  await page.check('input[name="setup-lang"][value="ko"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  assert.equal(koRequests(env).length, 1, 'not asked again in the visit, 한글 again included');
  // A pack that does not read: the reader's line, nothing kept; asked once in the next visit.
  sitePacks(env, { 'index.json': koListing(KO_FILE), [KO_FILE]: Uint8Array.from(pack.slice(0, pack.length - 3)) });
  await page.reload();
  await h.waitSettled(page);
  assert.deepEqual(koRequests(env), [[`/packs/${KO_FILE}`, 200]], 'downloaded once');
  assert.equal((await h.state(page)).status, `${ko.koPackRefused('its contents are not those of a Korean pack')}\n${ko.noFilesYet}`,
    'refused with the reader\'s line');
  assert.equal((await localeShown(page)).pack, 'none', 'no pack');
  assert.equal(await h.keptLocalePack(page), null, 'none kept');
  assert.equal(await koSourceKept(page), null, 'no source kept');
  assert.equal(await localeNoteShown(page), null, 'one is listed: no note');
  await page.click('#settings');
  await page.check('input[name="setup-lang"][value="en"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await page.click('#settings');
  await page.check('input[name="setup-lang"][value="ko"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  assert.equal(koRequests(env).length, 1, 'not asked again in the visit');
  await noPackWords(page, words);
  console.log('  receipt: no Korean pack listed 0 pack requests, English 0 pack requests, 404 1 request, unreadable 1 request: pass');
});

test('the site\'s Korean pack in a game (the user\'s disks, a synthetic conversation): a game running in English, then 한글 at the chooser: the listed pack is downloaded and the game restarts with it, its line counts only; a reload starts the game with the kept pack and downloads nothing', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const ko = TEXT.ko;
  const { archive, pack, runs, scripts } = await koFixture();
  sitePacks(env, { 'index.json': koListing(KO_FILE), [KO_FILE]: Uint8Array.from(pack) });
  const { page } = await h.openPage(env, { setup: { lang: 'en' } });
  const words = consoleText(page);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  assert.equal(await h.replaceKept(page, 'converse.a', archive), 1, 'one archive replaced');
  await page.goto(env.server.url);
  await h.waitSettled(page);
  await h.waitPrompt(page);
  assert.equal((await localeShown(page)).locale, 'off', 'English');
  assert.deepEqual(koRequests(env), [], 'English: no Korean pack asked for');
  const saved = await h.savedFiles(page);
  await page.click('#settings');
  await page.check('input[name="setup-lang"][value="ko"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await h.waitPrompt(page);
  let shown = await localeShown(page);
  assert.deepEqual([shown.pack, shown.locale], [`${runs},${scripts}`, 'on'], '한글: the game restarted with the pack');
  const status = (await h.state(page)).status;
  assert.ok(status.startsWith(`${ko.koPackLoaded(runs, scripts)}${ko.koPackKept}\n`) && status.includes(ko.koPackOn(runs, scripts)),
    'the lines of a dropped pack, counts only');
  assert.deepEqual(koRequests(env), [[`/packs/${KO_FILE}`, 200]], 'downloaded once');
  h.assertSame(await h.savedFiles(page), saved, 'the save untouched');
  env.packRequests.length = 0;
  await page.reload();
  await h.waitSettled(page);
  await h.waitPrompt(page);
  shown = await localeShown(page);
  assert.deepEqual([shown.pack, shown.locale], [`${runs},${scripts}`, 'on'], 'a reload: the game starts with the kept pack');
  assert.ok((await h.state(page)).status.includes(ko.koPackOn(runs, scripts)), 'its line, counts only');
  assert.deepEqual(koRequests(env), [], 'nothing downloaded');
  await noPackWords(page, words);
  console.log('  receipt: 1 download, the game on it after the switch and after a reload, 0 downloads on the reload: pass');
}, { needs: h.haveHfes });

test('the first-run language: with no choice kept the chooser shows on 한글, whatever the browser\'s language, and English is one click away; a kept choice wins in each case', async (env) => {
  sitePacks(env, { 'index.json': koListing(null) });
  // [what, the browser's language, the choice kept, the language wanted]
  const cases = [
    ['an en-US browser', 'en-US', null, 'ko'],
    ['a ko-KR browser', 'ko-KR', null, 'ko'],
    ['an en-US browser, English kept', 'en-US', { lang: 'en' }, 'en'],
    ['a ko-KR browser, English kept', 'ko-KR', { lang: 'en' }, 'en'],
    ['an en-US browser, 한글 kept', 'en-US', { lang: 'ko' }, 'ko'],
  ];
  for (const [what, locale, stored, want] of cases) {
    const { context, page } = await h.openPage(env, { setup: stored, locale });
    await h.load(page, env);
    const c = await chooser(page);
    assert.equal((await pageText(page)).lang, want, `${what}: the page in ${want}`);
    if (stored === null) {
      assert.ok(c.shown, `${what}: the chooser shows first`);
      assert.equal(c.lang, want, `${what}: on ${want}`);
      await page.uncheck('#setup-voices'); // no voices turned on here
      await page.click('#setup-ok');
      await h.waitSettled(page);
      assert.deepEqual((await chooser(page)).stored, { lang: want, version: h.SETUP_VERSION }, `${what}: OK keeps it`);
    } else {
      assert.ok(!c.shown, `${what}: no chooser`);
      assert.deepEqual(c.stored, { ...stored, version: h.SETUP_VERSION }, `${what}: the choice kept as it was`);
    }
    await context.close();
  }
  console.log(`  receipt: ${cases.length} first-run cases: pass`);
});

test('a choice kept by an older page (SETUP_VERSION 2, or none) counts as none once, for every visitor: the chooser opens on 한글 (over English kept) with NPC voices checked and the listed packs loaded, its OK keeps the current record and the next visit has no chooser; a current record with English stands (no chooser, English, no Korean pack asked for)', async (env) => {
  const { pack, runs, scripts } = await koFixture();
  const listing = { 'index.json': koListing(KO_FILE), [KO_FILE]: Uint8Array.from(pack) };
  for (const [what, record] of [['version 2', '{"lang":"en","version":2}'], ['no version', '{"lang":"en"}']]) {
    sitePacks(env, listing);
    const { context, page } = await h.openPage(env, { setup: record, locale: 'en-US' });
    await h.load(page, env);
    await koPackIs(page, `${runs},${scripts}`);
    let c = await chooser(page);
    assert.ok(c.shown, `${what}: the chooser shows`);
    assert.equal(c.lang, 'ko', `${what}: on 한글`);
    assert.equal((await pageText(page)).lang, 'ko', `${what}: the page in 한글`);
    assert.ok(c.voices && c.terms, `${what}: NPC voices checked, the terms shown`);
    assert.deepEqual(koRequests(env), [[`/packs/${KO_FILE}`, 200]], `${what}: the listed Korean pack loaded before any press, once`);
    assert.deepEqual(c.stored, JSON.parse(record), `${what}: the older record untouched until OK`);
    await page.uncheck('#setup-voices'); // no voices turned on here
    await page.click('#setup-ok');
    await h.waitSettled(page);
    c = await chooser(page);
    assert.deepEqual(c.stored, { lang: 'ko', version: h.SETUP_VERSION }, `${what}: OK keeps the current record`);
    await page.reload();
    await koPackIs(page, `${runs},${scripts}`);
    assert.ok(!(await chooser(page)).shown, `${what}: the next visit: no chooser`);
    assert.equal(koRequests(env).length, 1, `${what}: and no second download`);
    await context.close();
  }
  sitePacks(env, listing);
  const { context, page } = await h.openPage(env, { setup: { lang: 'en' } });
  await h.load(page, env);
  const c = await chooser(page);
  assert.ok(!c.shown, 'a current record: no chooser');
  assert.equal((await pageText(page)).lang, 'en', 'English');
  assert.deepEqual(koRequests(env), [], 'no Korean pack asked for');
  await context.close();
  console.log('  receipt: 2 older records 1 chooser and 1 download each, a current record 0 downloads: pass');
});

// Runs in the page: while the test holds `ko-source` (window.__fail), every write of the Korean
// pack's source record (a put or a delete of locale:ko:source) throws inside its transaction,
// as a request the storage refuses.
function sourceWritesThrow() {
  for (const name of ['put', 'delete']) {
    const real = IDBObjectStore.prototype[name];
    IDBObjectStore.prototype[name] = function write(...args) {
      const key = name === 'put' ? args[1] : args[0];
      if (window.__fail?.has('ko-source') && key === 'locale:ko:source') {
        throw new DOMException('test: refused', 'QuotaExceededError');
      }
      return real.apply(this, args);
    };
  }
}

// Sets the source record kept with the Korean pack to `value` in the page (undefined: none).
const setKeptSource = (page, value) => page.evaluate((v) => new Promise((done, fail) => {
  const req = indexedDB.open('ultima6-port', 1);
  req.onerror = () => fail(req.error);
  req.onsuccess = () => {
    const db = req.result;
    const tx = db.transaction('files', 'readwrite');
    if (v === undefined) tx.objectStore('files').delete('locale:ko:source');
    else tx.objectStore('files').put(v, 'locale:ko:source');
    tx.oncomplete = () => { db.close(); done(); };
    tx.onerror = () => fail(tx.error);
  };
}), value);

test('the site\'s Korean pack and its source (synthetic, no game): the pack and the file it came from are kept in one transaction, so with every write of the source refused a drop after the site\'s pack is not kept and the site\'s pack stays with its own source, never the dropped bytes under the site\'s name; a kept pack whose source is missing or not a file name counts as dropped: a listing that names another file downloads nothing and the pack stays, byte for byte', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const ko = TEXT.ko;
  const hosted = await koFixture();
  const dropped = await koFixture(0x11);
  sitePacks(env, { 'index.json': koListing(KO_FILE), [KO_FILE]: Uint8Array.from(hosted.pack) });
  const { context, page } = await h.openPage(env, { setup: { lang: 'ko' } });
  await context.addInitScript(sourceWritesThrow);
  await page.goto(env.server.url);
  await koPackIs(page, `${hosted.runs},${hosted.scripts}`);
  await page.evaluate(() => window.__fail.add('ko-source'));
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: dropped.pack }]);
  await koPackIs(page, `${dropped.runs},${dropped.scripts}`);
  assert.equal((await h.state(page)).status, `${ko.koPackLoaded(dropped.runs, dropped.scripts)}${ko.koPackNotKept(ko.koPackOldStays)}`,
    'the drop works for this visit, the pack kept before stays');
  assert.deepEqual(await h.keptLocalePack(page), hosted.pack, 'the site\'s pack kept as it was');
  assert.equal(await koSourceKept(page), KO_FILE, 'with its own source');
  await page.evaluate(() => window.__fail.delete('ko-source'));
  // Its source missing, then not a file name: the kept pack counts as dropped.
  for (const [what, value] of [['missing', undefined], ['not a file name', '../x.u6lp'], ['not text', 7]]) {
    await setKeptSource(page, value);
    sitePacks(env, { 'index.json': koListing(KO_NEWER), [KO_NEWER]: Uint8Array.from(dropped.pack) });
    await page.reload();
    await h.waitSettled(page);
    assert.deepEqual(koRequests(env), [], `a source ${what}: nothing downloaded`);
    assert.equal((await localeShown(page)).pack, `${hosted.runs},${hosted.scripts}`, `a source ${what}: the kept pack loaded`);
    assert.deepEqual(await h.keptLocalePack(page), hosted.pack, `a source ${what}: and kept, byte for byte`);
    assert.equal((await chooser(page)).note, null, `a source ${what}: a pack kept, no note`);
  }
  console.log('  receipt: a refused source write kept the pair whole; 3 sources missing or unreadable, 0 downloads: pass');
});

test('the site\'s Korean pack, then a dropped one (synthetic, no game): the drop keeps no source, so the next visit with a listing that names another file downloads nothing and the dropped pack stays, byte for byte', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const ko = TEXT.ko;
  const hosted = await koFixture();
  const dropped = await koFixture(0x11);
  const newer = await koFixture(0x21);
  sitePacks(env, { 'index.json': koListing(KO_FILE), [KO_FILE]: Uint8Array.from(hosted.pack) });
  const { page } = await h.openPage(env, { setup: { lang: 'ko' } });
  await page.goto(env.server.url);
  await koPackIs(page, `${hosted.runs},${hosted.scripts}`);
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: dropped.pack }]);
  await koPackIs(page, `${dropped.runs},${dropped.scripts}`);
  assert.deepEqual(await h.keptLocalePack(page), dropped.pack, 'the drop kept in place of the site\'s pack');
  assert.equal(await koSourceKept(page), null, 'with no source');
  assert.equal((await chooser(page)).note, null, 'a dropped pack: no note');
  sitePacks(env, { 'index.json': koListing(KO_NEWER), [KO_NEWER]: Uint8Array.from(newer.pack) });
  await page.reload();
  await h.waitSettled(page);
  assert.deepEqual(koRequests(env), [], 'a listing that names another file: nothing downloaded');
  assert.equal((await localeShown(page)).pack, `${dropped.runs},${dropped.scripts}`, 'the dropped pack loaded');
  assert.deepEqual(await h.keptLocalePack(page), dropped.pack, 'and kept, byte for byte');
  console.log('  receipt: the site\'s pack then a drop, a renamed listing, 0 downloads: pass');
});

test('a returning visit after an older page (the user\'s disks, a synthetic conversation): a choice kept by an older page in English, the kept game files, an HD pack chosen from the listing and no Korean pack kept; the listing names a Korean pack and another HD pack first: before any press, the chooser is open on 한글 with the HD pack chosen still checked, and the game it started behind it already runs with the Korean pack, downloaded once; OK keeps the current record, the HD pack and the Korean pack, nothing downloaded again', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const ko = TEXT.ko;
  const { archive, pack, runs, scripts } = await koFixture();
  const hd = await madePack(env);
  const hdEntry = { file: 'test-pack.zip', label: 'Test pack' };
  // An older page: the game files kept, an HD pack chosen from the listing (no Korean pack listed
  // yet), its choice kept as that page kept it (no version).
  sitePacks(env, { 'index.json': JSON.stringify({ packs: [hdEntry] }), 'test-pack.zip': hd });
  const { context, page } = await h.openPage(env, { setup: '{"lang":"en"}' });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  assert.equal(await h.replaceKept(page, 'converse.a', archive), 1, 'one archive replaced');
  await page.goto(env.server.url);
  await h.waitSettled(page);
  await listed(page, 1);
  // The older record opened the chooser as a first run: English, the HD pack, no voices.
  await page.check('input[name="setup-lang"][value="en"]');
  await page.check('input[name="setup-hd"][value="test-pack.zip"]');
  await page.uncheck('#setup-voices');
  await page.click('#setup-ok');
  await packIs(page, '1,0,0');
  await page.evaluate(() => localStorage.setItem('u6.setup', '{"lang":"en"}'));
  assert.equal((await chooser(page)).source, 'test-pack.zip', 'the HD pack chosen from the listing');
  assert.equal(await h.keptLocalePack(page), null, 'no Korean pack kept');
  // The visit: the listing now names a Korean pack, and another HD pack before the one chosen
  // (the first run's default for a visitor with none); no press at all.
  sitePacks(env, { 'index.json': JSON.stringify({ packs: [{ file: 'first.zip', label: 'First' }, hdEntry],
    ko: { file: KO_FILE, label: 'Korean (test)' } }), 'test-pack.zip': hd, [KO_FILE]: Uint8Array.from(pack) });
  await page.reload();
  await h.waitSettled(page);
  await h.waitPrompt(page);
  await listed(page, 2);
  // The games the visit started, in order (h's glue: each Game.start, and what set_locale gave
  // it): one, with the Korean pack from before its first step; no English start, no restart.
  const starts = () => page.evaluate(() => globalThis.__starts.map(({ locale, stepsBefore }) => ({ locale, stepsBefore })));
  assert.deepEqual(await starts(), [{ locale: 'on', stepsBefore: 0 }],
    'before any press: one game started, with the Korean pack before its first step');
  let c = await chooser(page);
  assert.ok(c.shown && c.lang === 'ko', 'the chooser open on 한글');
  assert.equal(c.checked, 'test-pack.zip', 'the HD pack still checked, not the first listed');
  const shown = await localeShown(page);
  assert.deepEqual([shown.pack, shown.locale], [`${runs},${scripts}`, 'on'], 'the game behind it runs with the Korean pack');
  assert.ok((await h.state(page)).status.includes(ko.koPackOn(runs, scripts)), 'its line, counts only');
  assert.equal(await page.evaluate(() => document.body.dataset.hdPack), '1,0,0', 'and the HD pack');
  assert.deepEqual(env.packRequests, [['/packs/index.json', 200], [`/packs/${KO_FILE}`, 200]],
    'the listing and the Korean pack, once each; the HD pack not downloaded again');
  assert.deepEqual(c.stored, { lang: 'en' }, 'the older record untouched until OK');
  await page.uncheck('#setup-voices'); // no voices turned on here
  await page.click('#setup-ok');
  await h.waitSettled(page);
  c = await chooser(page);
  assert.ok(!c.shown && c.source === 'test-pack.zip', 'OK: the HD pack stays the one chosen');
  assert.deepEqual(c.stored, { lang: 'ko', version: h.SETUP_VERSION }, 'and keeps the current record');
  assert.equal((await localeShown(page)).locale, 'on', 'the game still on the Korean pack');
  assert.equal((await starts()).length, 1, 'OK: no restart');
  assert.equal(env.packRequests.length, 2, 'nothing downloaded again');
  console.log('  receipt: 1 game start in the visit, on the Korean pack from its first step, 1 pack download, the HD choice kept: pass');
}, { needs: h.haveHfes });

// --- typing Korean in a conversation (spec 08 Design 4, slice K5a) ------------------------

const KO_KEYWORD = 'quiz';
const KO_ALIAS = '시험';
const KO_ANSWER = '"Asked, and so answered."';
const KO_ANSWER_KO = '"묻기에 답하였노라."';
const KO_NOPE = '"No such thing."';

// koFixture's archive with the player's words after the greeting: `f7`, a keyword block
// `quiz` whose answer ends the conversation, and after the block a line for no match (left out
// of the pack: a miss); the pack holds the look text, the greeting and the answer in Korean, and
// gives `quiz` the alias 시험 in every script (its aliases part spliced in here: the translation
// tool writes none in K5a).
async function koTypingFixture() {
  const { fingerprint, hash64, hex64, latin1, packBytes } = await import('../i18n/translate.mjs');
  const head = [0xff, 0x02, ...latin1('Zed'), 0xf1, ...latin1(KO_LOOK), 0xf2, ...latin1(KO_GREET)];
  const answer = head.length + 2 + KO_KEYWORD.length + 1;
  const script = [...head, 0xf7, 0xef, ...latin1(KO_KEYWORD), 0xf6, ...latin1(KO_ANSWER), 0xb6, 0xee, ...latin1(KO_NOPE), 0xb6];
  const items = 0x63;
  const table = [];
  const body = [];
  for (let i = 0; i < items; i += 1) {
    const at = i === 0 ? 0 : items * 4 + body.length;
    if (i > 0) body.push(0, 0, 0, 0, ...script);
    table.push(at & 255, (at >> 8) & 255, (at >> 16) & 255, 0);
  }
  const look = 6;
  const greet = look + KO_LOOK.length + 1;
  const runs = [];
  const named = [];
  const aliases = [];
  for (let item = 1; item < items; item += 1) {
    named.push({ archive: 'a', item, hash: hex64(hash64(script)) });
    runs.push({ archive: 'a', item, offset: look, len: KO_LOOK.length, hash: hex64(hash64(latin1(KO_LOOK))), text: KO_LOOK_KO });
    runs.push({ archive: 'a', item, offset: greet, len: KO_GREET.length, hash: hex64(hash64(latin1(KO_GREET))), text: KO_GREET_KO });
    runs.push({ archive: 'a', item, offset: answer, len: KO_ANSWER.length, hash: hex64(hash64(latin1(KO_ANSWER))), text: KO_ANSWER_KO });
    aliases.push({ archive: 'a', item, keyword: KO_KEYWORD, words: [KO_ALIAS] });
  }
  const made = Buffer.from(packBytes({ base: fingerprint(named), runs }));
  const json = JSON.parse(made.subarray(5).toString('utf8'));
  json.aliases = aliases;
  const pack = [...made.subarray(0, 5), ...Buffer.from(JSON.stringify(json), 'utf8')];
  return { archive: [...table, ...body], pack };
}

test('Korean typed in a conversation (the user\'s disks, a synthetic conversation, slice K5a): in 한글 with the pack, while the game reads the player\'s words the page gives its hidden input the focus (Shift+Tab leaves it for the canvas, Tab comes back); a scripted IME sequence there (a composition Escape cancels, a syllable composed and committed, a second with Backspace inside it, a third composed with Enter during its composition, then its commit) gives the game the committed syllables only, then Enter; they echo in the message window and the keyword\'s alias with a particle answers with the pack\'s line; the focus is back on the canvas when the line ends; outside a line Tab from the canvas passes the input by, and a line read while the input already has the focus (given by script before the conversation, the keys sent to it) takes the committed syllables and Enter once each; in English there is no hidden input and the canvas takes the keys as before; nothing typed reaches the console, the page\'s data or text', async (env) => {
  const { page } = await h.openPage(env, { setup: { lang: 'ko' } });
  const words = consoleText(page);
  await h.load(page, env);
  const { archive, pack } = await koTypingFixture();
  const glyphs = await hangulGlyphs(`${KO_ALIAS}을${KO_ANSWER_KO}혐`);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: pack }]);
  await h.waitSettled(page);
  await h.waitPrompt(page);
  assert.equal(await h.replaceKept(page, 'converse.a', archive), 1, 'one archive replaced');
  await page.reload();
  await h.waitSettled(page);
  await h.waitPrompt(page);
  assert.equal((await localeShown(page)).locale, 'on', 'the pack on the game');
  const focus = () => page.evaluate(() => ({ ime: document.getElementById('ime') !== null, at: document.activeElement?.id ?? null }));
  // Talk to member 2 up to the player's words: the line the game reads.
  const talkTo2 = async () => {
    await page.focus('#screen');
    await page.keyboard.press('t');
    await h.waitTaken(page);
    await page.keyboard.press('2');
    await page.waitForFunction(() => globalThis.__owned === 7 && document.getElementById('screen').dataset.keys === '0', null, { timeout: 30_000 });
    await h.waitFrames(page, 2);
  };
  await talkTo2();
  assert.deepEqual(await focus(), { ime: true, at: 'ime' }, 'the hidden input has the focus while the line is read');
  // Shift+Tab from it: the canvas keeps the focus (no trap); Tab gives the input it again.
  await page.keyboard.press('Shift+Tab');
  await h.waitFrames(page, 2);
  assert.deepEqual(await focus(), { ime: true, at: 'screen' }, 'Shift+Tab leaves the input for the canvas');
  await page.keyboard.press('Tab');
  await h.waitFrames(page, 2);
  assert.deepEqual(await focus(), { ime: true, at: 'ime' }, 'Tab comes back to it');
  await page.evaluate(() => { globalThis.__sent.length = 0; });
  const cdp = await page.context().newCDPSession(page);
  const compose = (text) => cdp.send('Input.imeSetComposition', { text, selectionStart: text.length, selectionEnd: text.length });
  const press = async (key, vk) => {
    await cdp.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key, code: key, windowsVirtualKeyCode: vk });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key, code: key, windowsVirtualKeyCode: vk });
  };
  await compose('ㄱ');
  await press('Escape', 27); // while composing: the composition goes, the line stays
  await h.waitFrames(page, 2);
  assert.ok(await page.evaluate(() => globalThis.__owned === 7 && document.activeElement?.id === 'ime'), 'the line still read');
  await compose('ㅅ');
  await compose('시');
  await cdp.send('Input.insertText', { text: '시' });
  await compose('ㅎ');
  await compose('혀');
  await compose('혐');
  await press('Backspace', 8); // inside the syllable: the IME's, which takes the ㅁ back
  await compose('혀');
  await compose('험');
  await cdp.send('Input.insertText', { text: '험' });
  await compose('ㅇ');
  await compose('으');
  await compose('을');
  await press('Enter', 13); // during the composition: after its commit
  await cdp.send('Input.insertText', { text: '을' });
  await h.waitPrompt(page);
  assert.deepEqual(await page.evaluate(() => globalThis.__sent), ['text:시', 'text:험', 'text:을', 'Enter'],
    'the committed syllables only, then Enter');
  const shown = await localeShown(page);
  assert.deepEqual([shown.hits, shown.misses], [3, 0], 'the look text, the greeting and the alias\'s answer from the pack, nothing missed');
  const korean = await cellsShown(page, glyphs);
  for (const c of `${KO_ALIAS}을`) assert.ok(korean.found.has(c), 'the typed syllables echoed in Hangul cells');
  assert.ok(!korean.found.has('혐'), 'nothing of a composition reached the game');
  assert.ok([...KO_ANSWER_KO].filter((c) => glyphs.has(c)).every((c) => korean.found.has(c)), 'the answer in Korean');
  assert.deepEqual(await focus(), { ime: true, at: 'screen' }, 'the focus back on the canvas once the line ended');
  // At the prompt the input is out of the tab order: Tab from the canvas does not reach it.
  await page.focus('#screen');
  await page.keyboard.press('Tab');
  await h.waitFrames(page, 2);
  assert.notEqual((await focus()).at, 'ime', 'Tab from the canvas at the prompt passes the hidden input by');
  // The input given the focus before the conversation (by script): the keys reach the game
  // through it, and the line read then takes what is committed there and the Enter, once each.
  await page.focus('#ime');
  await page.keyboard.press('t');
  await h.waitTaken(page);
  await page.keyboard.press('2');
  await page.waitForFunction(() => globalThis.__owned === 7 && document.getElementById('screen').dataset.keys === '0', null, { timeout: 30_000 });
  await h.waitFrames(page, 2);
  assert.deepEqual(await focus(), { ime: true, at: 'ime' }, 'the input keeps the focus into the line');
  await page.evaluate(() => { globalThis.__sent.length = 0; });
  await compose('ㅅ');
  await compose('시');
  await cdp.send('Input.insertText', { text: '시' });
  await compose('험');
  await cdp.send('Input.insertText', { text: '험' });
  await press('Enter', 13);
  await h.waitPrompt(page);
  assert.deepEqual(await page.evaluate(() => globalThis.__sent), ['text:시', 'text:험', 'Enter'],
    'the committed syllables and the Enter, once each');
  const again = await localeShown(page);
  assert.deepEqual([again.hits, again.misses], [6, 0], 'the alias answered from the pack again, nothing missed');
  assert.deepEqual(await focus(), { ime: true, at: 'screen' }, 'and the focus back on the canvas');
  // English at the chooser: no hidden input; the canvas takes the keys of the line.
  await page.click('#settings');
  await page.check('input[name="setup-lang"][value="en"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await h.waitPrompt(page);
  assert.equal((await localeShown(page)).locale, 'off', 'English: the pack not on the game');
  await talkTo2();
  assert.deepEqual(await focus(), { ime: false, at: 'screen' }, 'no hidden input in English');
  await page.keyboard.type(KO_KEYWORD);
  await page.keyboard.press('Enter');
  await h.waitPrompt(page);
  const english = await cellsShown(page, glyphs);
  assert.ok(english.found.size === 0 && english.inked > 0, 'the English answer, no Korean cell');
  const where = await pageWords(page);
  for (const w of [`${KO_ALIAS}을`, KO_ALIAS, KO_ANSWER_KO]) {
    assert.ok(!where.includes(w), 'nothing typed in the page\'s data, text or storage');
    assert.ok(!words.some((m) => m.includes(w)), 'nor in the console');
  }
}, { needs: h.haveHfes });

// --- the Korean pack's table strings and game strings (spec 08, slice K2) ---------------

const KO_TABLE = 'a stone of tests';
const KO_TABLE_KO = '시험 돌';
// The made-up syllable every game string of the pack shows (none of the fixture's lines has it).
const KO_WORD = '뷁';

// `en` with every `%` directive and every other escape kept in order, the line breaks at its ends
// kept, and the rest one made-up `word`.
function madeUp(en, word) {
  const lead = /^\n*/.exec(en)[0];
  const trail = /\n*$/.exec(en.slice(lead.length))[0];
  const body = en.slice(lead.length, en.length - trail.length);
  let kept = '';
  for (let i = 0; i < body.length; i += 1) {
    const c = body[i];
    if (c === '%') {
      const d = /^%(?:[1-9][0-9]*\$)?[-0]*[0-9]*[\s\S]?/.exec(body.slice(i))[0];
      kept += d;
      i += d.length - 1;
    } else if ('$#^'.includes(c)) {
      kept += body.slice(i, i + 2);
      i += 1;
    } else if ('*&`/\\<>'.includes(c)) kept += c;
  }
  return lead + word + kept + trail;
}

// The game strings of the user's `game` (the archive's path, or the disk images' paths, copied into
// a folder of the test's own) as u6-export makes them (tools/export: the export the page's Game
// strings button made, byte for byte), parsed. Built once a run; its file is written into a
// temporary folder outside the repository, removed after. An error names only the step.
let exporter = null;
async function nativeStrings(game) {
  const { execFile } = await import('node:child_process');
  const run = (file, args, cwd) => new Promise((done, fail) => execFile(file, args, { cwd, maxBuffer: 1 << 24 },
    (err) => (err ? fail(new Error(`${basename(file)} ${args[0]}: exit ${err.code}`)) : done())));
  exporter ??= run('bash', ['scripts/cargo.sh', 'build', '--release', '-p', 'u6-export'], h.REPO)
    .then(() => join(process.env.CARGO_TARGET_DIR ?? join(h.REPO, 'target'), 'release', 'u6-export'));
  const bin = await exporter;
  const dir = await mkdtemp(join(tmpdir(), 'u6-strings-'));
  try {
    let input = game;
    if (Array.isArray(game)) {
      input = join(dir, 'disks');
      await mkdir(input);
      for (const p of game) await copyFile(p, join(input, basename(p)));
    }
    const out = join(dir, 'out', 'strings.json');
    await run(bin, ['strings', '--game', input, '--out', out], dir);
    return JSON.parse(await readFile(out, 'utf8'));
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

// The K1 fixture with a table string: every item of the synthetic converse.a is `ff 02 "Zed" f1
// <look> f2 <greeting> b5 d2 <table> b1 b6 <table string> 0` (op b5 prints the table's first
// string after the greeting); the pack: each script's two runs and its table string in Korean,
// and every game string of `exported` (the native export, made at test time) as the made-up
// syllable within the tool's gates (counted when one fails them and is left out).
async function koStringsFixture(exported) {
  const { fingerprint, hash64, hex64, latin1, packBytes, stringGate } = await import('../i18n/translate.mjs');
  const head = [0xff, 0x02, ...latin1('Zed'), 0xf1, ...latin1(KO_LOOK), 0xf2, ...latin1(KO_GREET)];
  const tableAt = head.length + 8;
  const script = [...head, 0xb5, 0xd2, tableAt & 255, tableAt >> 8, 0, 0, 0xb1, 0xb6, ...latin1(KO_TABLE), 0];
  const items = 0x63;
  const table = [];
  const body = [];
  for (let i = 0; i < items; i += 1) {
    const at = i === 0 ? 0 : items * 4 + body.length;
    if (i > 0) body.push(0, 0, 0, 0, ...script);
    table.push(at & 255, (at >> 8) & 255, (at >> 16) & 255, 0);
  }
  const look = 6;
  const greet = look + KO_LOOK.length + 1;
  const runs = [];
  const tables = [];
  const named = [];
  for (let item = 1; item < items; item += 1) {
    named.push({ archive: 'a', item, hash: hex64(hash64(script)) });
    runs.push({ archive: 'a', item, offset: look, len: KO_LOOK.length, hash: hex64(hash64(latin1(KO_LOOK))), text: KO_LOOK_KO });
    runs.push({ archive: 'a', item, offset: greet, len: KO_GREET.length, hash: hex64(hash64(latin1(KO_GREET))), text: KO_GREET_KO });
    tables.push({ archive: 'a', item, offset: tableAt, len: KO_TABLE.length, hash: hex64(hash64(latin1(KO_TABLE))), capacity: null, text: KO_TABLE_KO });
  }
  const strings = [];
  let refused = 0;
  for (const e of exported.strings) {
    const ko = madeUp(e.text, KO_WORD);
    if (stringGate(e.text, ko) === null) strings.push({ hash: e.hash, len: e.len, text: ko });
    else refused += 1;
  }
  const pack = [...packBytes({ base: fingerprint(named), runs, tables, strings })];
  return { archive: [...table, ...body], pack, runs: runs.length, scripts: items - 1, tables: tables.length, strings: strings.length, refused };
}

const stringCounts = (page) => page.evaluate(() => {
  const c = document.getElementById('screen').dataset;
  return {
    pack: document.body.dataset.localePackStrings,
    tableHits: Number(c.localeTableHits ?? 0), tableMisses: Number(c.localeTableMisses ?? 0),
    textHits: Number(c.localeTextHits ?? 0), textMisses: Number(c.localeTextMisses ?? 0),
  };
});

test('Korean pack with game strings in a game (the user\'s disks, a synthetic conversation with a table string): the native string export (u6-export, the page\'s former Game strings) gives the catalogued strings of the game file as one u6.string-bundle/1 file; a pack made at test time from that export (every string the made-up syllable within the tool\'s gates) and the test\'s conversation shows the syllable in the message window at a command key and the table string\'s Korean in the conversation; Forget the Korean pack: English again; no string reaches the console, the page\'s data or text', async (env) => {
  const { page } = await h.openPage(env, { setup: { lang: 'ko' } });
  const words = consoleText(page);
  await h.load(page, env);
  const { TEXT } = await import('../../site/i18n.js');
  const ko = TEXT.ko;
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  const before = await pageWords(page);
  // The export, from the same disks.
  const exported = await nativeStrings(h.HFES);
  assert.equal(exported.format, 'u6.string-bundle/1');
  assert.ok(exported.strings.length > 0, 'strings');
  // Each 3 bytes or more, a label (a wind's name) 1 or more (u6_core::locale::strings).
  assert.ok(exported.strings.every((e) => fnv64([...Buffer.from(e.text, 'latin1')]) === e.hash && e.len >= (e.kind === 'label' ? 1 : 3)), 'each keyed by its hash');
  const fx = await koStringsFixture(exported);
  assert.ok(fx.strings > 0 && fx.refused < fx.strings, 'strings kept within the gates, fewer refused than kept');
  // The test's converse.a in place of the user's, then the pack: the game restarts with it.
  assert.equal(await h.replaceKept(page, 'converse.a', fx.archive), 1, 'one archive replaced');
  await page.reload();
  await h.waitSettled(page);
  await h.waitPrompt(page);
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: fx.pack }]);
  await h.waitSettled(page);
  await h.waitPrompt(page);
  let shown = await localeShown(page);
  assert.deepEqual([shown.pack, shown.locale], [`${fx.runs},${fx.scripts}`, 'on'], 'the pack on the game');
  assert.equal((await stringCounts(page)).pack, `${fx.tables},${fx.strings}`, 'its strings, counted');
  assert.ok((await h.state(page)).status.includes(ko.koPackOn(fx.runs, fx.scripts, fx.tables, fx.strings)), 'its line, counts only');
  const word = await hangulGlyphs(KO_WORD);
  // A command key: its name and the loop's lines from the pack.
  await page.focus('#screen');
  await page.keyboard.press('l');
  await h.waitTaken(page);
  let cells = await cellsShown(page, word);
  assert.equal(cells.found.size, 1, 'the made-up syllable in a cell of the message window');
  // The cross-hair's own cell (the leader): his portrait, closed by a key; then the prompt.
  await page.keyboard.press('Enter');
  await h.waitTicks(page, 90);
  await page.keyboard.press('Space');
  await h.waitPrompt(page);
  // The conversation: its runs and its table string from the pack.
  await page.keyboard.press('t');
  await h.waitTaken(page);
  await page.keyboard.press('2');
  await h.waitTaken(page);
  await h.waitPrompt(page);
  const table = await hangulGlyphs(KO_TABLE_KO);
  cells = await cellsShown(page, table);
  assert.equal(cells.found.size, table.size, 'every syllable of the table string in the window');
  const counts = await stringCounts(page);
  assert.ok(counts.tableHits >= 1 && counts.tableMisses === 0, 'the table string from the pack, none missed');
  assert.ok(counts.textHits > 0, 'game strings from the pack');
  // Forget the Korean pack: the game restarts in English.
  await forgetVia(page, '#forget-ko');
  await h.waitSettled(page);
  await h.waitPrompt(page);
  shown = await localeShown(page);
  assert.deepEqual([shown.pack, shown.locale], ['none', 'off'], 'forgotten: English');
  assert.equal((await stringCounts(page)).pack, 'none');
  await page.focus('#screen');
  await page.keyboard.press('l');
  await h.waitTaken(page);
  cells = await cellsShown(page, word);
  assert.ok(cells.found.size === 0 && cells.inked > 0, 'the command in English: no made-up syllable');
  await page.keyboard.press('Enter');
  await h.waitTicks(page, 90);
  await page.keyboard.press('Space');
  await h.waitPrompt(page);
  await page.keyboard.press('t');
  await h.waitTaken(page);
  await page.keyboard.press('2');
  await h.waitTaken(page);
  // The English is longer: the window fills and waits at its more prompt, the table string shown.
  await h.waitTicks(page, 300);
  cells = await cellsShown(page, table);
  assert.ok(cells.found.size === 0 && cells.inked > 0, 'the conversation in English: no syllable of the table string');
  await page.keyboard.press('Space');
  await h.waitPrompt(page);
  const where = await pageWords(page);
  // A game string of a few common words can be the page's own text too: only one the page did not
  // show before the export counts.
  const long = exported.strings.map((e) => e.text.trim()).filter((t) => t.length >= 8 && !before.includes(t));
  for (const w of [KO_SENTINEL, KO_TABLE, KO_TABLE_KO, ...long]) {
    assert.ok(!where.includes(w), 'no string in the page\'s data, text or storage');
    assert.ok(!words.some((m) => m.includes(w)), 'nor in the console');
  }
  assert.ok(!where.includes(KO_WORD), 'nor the pack\'s text');
}, { needs: h.haveHfes });

// --- the Korean pack's object names and books (spec 08, slice K3) -----------------------------

// The made-up syllable every object name and book of the pack shows (not K2's, so the Look
// label of a K2 pack never stands in for it; none of the fixture's lines has it).
const KO_NAME = '꽑';

// `e` (an object name or a book of the export) as the tool's gates take it: the line breaks at
// its ends, every `%` directive and every other escape kept in order, the runes as they are, a
// name's plural markers and their letters dropped (spec 08 §1), the rest one made-up `word`.
function madeUpText(e, word, withoutPlural) {
  const en = e.kind === 'look' ? withoutPlural(e.text) : e.text;
  const lead = /^\n*/.exec(en)[0];
  const trail = /\n*$/.exec(en.slice(lead.length))[0];
  const body = en.slice(lead.length, en.length - trail.length);
  let kept = '';
  for (let i = 0; i < body.length; i += 1) {
    const c = body[i];
    if (c === '<') {
      const end = body.indexOf('>', i);
      const to = end < 0 ? body.length : end + 1;
      kept += body.slice(i, to);
      i = to - 1;
    } else if (c === '%') {
      const d = /^%(?:[1-9][0-9]*\$)?[-0]*[0-9]*[\s\S]?/.exec(body.slice(i))[0];
      kept += d;
      i += d.length - 1;
    } else if ('$#^'.includes(c)) {
      kept += body.slice(i, i + 2);
      i += 1;
    } else if ('*&`/\\@'.includes(c)) kept += c;
  }
  return lead + word + kept + trail;
}

// The pack of the export's object names and books, every one the made-up syllable within the
// tool's gates (counted when one fails them and is left out), made with the tool's own writer.
async function koTextsFixture(exported) {
  const { bookGate, fingerprint, lookGate, packBytes, withoutPlural } = await import('../i18n/translate.mjs');
  const look = [];
  const books = [];
  let refused = 0;
  for (const e of exported.texts ?? []) {
    const ko = madeUpText(e, KO_NAME, withoutPlural);
    const why = e.kind === 'look' ? lookGate(e.text, ko) : bookGate(e.text, ko);
    if (why === null) (e.kind === 'look' ? look : books).push({ index: e.index, len: e.len, hash: e.hash, text: ko });
    else refused += 1;
  }
  const pack = [...packBytes({ base: fingerprint([]), runs: [], look, books })];
  return { pack, look: look.length, books: books.length, refused };
}

const textCounts = (page) => page.evaluate(() => {
  const c = document.getElementById('screen').dataset;
  return {
    pack: document.body.dataset.localePackTexts,
    lookHits: Number(c.localeLookHits ?? 0), lookMisses: Number(c.localeLookMisses ?? 0),
    bookHits: Number(c.localeBookHits ?? 0), bookMisses: Number(c.localeBookMisses ?? 0),
  };
});

// Look one step away from the leader (the cross-hair moved by `dir`, confirmed with Enter), the
// window's cells read once it has printed; then the window let go and the prompt back.
async function lookAt(page, dir) {
  await page.focus('#screen');
  await page.keyboard.press('l');
  await h.waitTaken(page);
  await page.keyboard.press(dir);
  await h.waitTaken(page);
  await page.keyboard.press('Enter');
  await h.waitTaken(page);
  await h.waitTicks(page, 120);
  const cells = await cellsShown(page, await hangulGlyphs(KO_NAME));
  for (let i = 0; i < 3; i += 1) {
    await page.keyboard.press('Space');
    await h.waitTaken(page);
  }
  await h.waitPrompt(page);
  return cells;
}

test('Korean pack with object names and books in a game (the user\'s disks): the native string export also gives the object names and the books and signs of the files; a pack made at test time from that export (every name and book the made-up syllable within the tool\'s gates) shows the syllable as the name Look gives the thing next to the leader; Forget the Korean pack: English again; no name or book reaches the console, the page\'s data or text', async (env) => {
  const { page } = await h.openPage(env, { setup: { lang: 'ko' } });
  const words = consoleText(page);
  await h.load(page, env);
  const { TEXT } = await import('../../site/i18n.js');
  const ko = TEXT.ko;
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  const before = await pageWords(page);
  const exported = await nativeStrings(h.HFES);
  const texts = exported.texts ?? [];
  const looks = texts.filter((e) => e.kind === 'look');
  const books = texts.filter((e) => e.kind === 'book');
  assert.ok(looks.length > 0 && books.length > 0 && looks.length + books.length === texts.length, 'object names and books');
  assert.ok(texts.every((e) => fnv64([...Buffer.from(e.text, 'latin1')]) === e.hash && e.len === Buffer.from(e.text, 'latin1').length), 'each keyed by its hash');
  const fx = await koTextsFixture(exported);
  assert.ok(fx.look > 0 && fx.books > 0, 'object names and books kept within the gates');
  // How many the gates refused is a count of the user's texts: checked, not shown.
  assert.ok(fx.refused < fx.look + fx.books, 'fewer refused by the gates than kept');
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: fx.pack }]);
  await h.waitSettled(page);
  await h.waitPrompt(page);
  let shown = await localeShown(page);
  assert.deepEqual([shown.pack, shown.locale], ['0,0', 'on'], 'the pack on the game');
  assert.equal((await textCounts(page)).pack, `${fx.look},${fx.books}`, 'its names and books, counted');
  assert.ok((await h.state(page)).status.includes(ko.koPackOn(0, 0, 0, 0, fx.look, fx.books)), 'its line, counts only');
  // Look one step away, a direction at a time, until a thing's name (not a party member's) shows.
  let dir = null;
  for (const d of ['ArrowUp', 'ArrowLeft', 'ArrowRight', 'ArrowDown']) {
    const cells = await lookAt(page, d);
    if (cells.found.size === 1) {
      dir = d;
      break;
    }
  }
  assert.ok(dir !== null, 'the made-up syllable as a name Look gives');
  let counts = await textCounts(page);
  const [hits, missed] = [counts.lookHits, counts.lookMisses];
  assert.ok(hits >= 1, `object names from the pack (${hits}, missed ${missed})`);
  // Forget the Korean pack: the game restarts in English.
  await forgetVia(page, '#forget-ko');
  await h.waitSettled(page);
  await h.waitPrompt(page);
  shown = await localeShown(page);
  assert.deepEqual([shown.pack, shown.locale], ['none', 'off'], 'forgotten: English');
  assert.equal((await textCounts(page)).pack, 'none');
  const english = await lookAt(page, dir);
  assert.ok(english.found.size === 0 && english.inked > 0, 'Look in English: no made-up syllable');
  counts = await textCounts(page);
  assert.deepEqual([counts.lookHits, counts.bookHits], [0, 0], 'nothing from a pack');
  const where = await pageWords(page);
  const long = texts.map((e) => e.text.trim()).filter((t) => t.length >= 10 && !before.includes(t));
  assert.ok(long.length > 0, 'texts to look for');
  for (const w of long) {
    assert.ok(!where.includes(w), 'no name or book in the page\'s data, text or storage');
    assert.ok(!words.some((m) => m.includes(w)), 'nor in the console');
  }
  assert.ok(!where.includes(KO_NAME), 'nor the pack\'s text');
  // The direction is not named: which one shows a thing is where the user's map puts it.
  console.log(`  receipt: object names and books kept within the gates, fewer refused than kept: pass; Look one step away: names from the pack ${hits}, missed ${missed}`);
}, { needs: h.haveHfes });

// --- the Korean pack's strings of the u program (spec 08, slice K4) ---------------------------

// The made-up syllable every string of u in the pack shows (not K2's or K3's; none of the
// fixture's lines has it).
const KO_U = '퀣';

// A string of u of the export as the tool's gate takes it: every printf code and every line break
// kept where it is, each line's words one made-up `word`.
function madeUpU(en, word) {
  let out = '';
  let worded = false;
  for (let i = 0; i < en.length; i += 1) {
    const c = en[i];
    if (c === '%') {
      out += en.slice(i, i + 2);
      i += 1;
    } else if (c === '\n') {
      out += c;
      worded = false;
    } else if (!worded && /[A-Za-z]/.test(c)) {
      out += word;
      worded = true;
    }
  }
  return out;
}

// The pack of the export's strings of u, every one the made-up syllable within the tool's gate,
// the keys of its read included (counted when one fails it and is left out: a prompt whose
// English shows a key standing alone, which the syllable replaces), made with the tool's own
// writer.
async function koProgramFixture(exported) {
  const { fingerprint, packBytes, uGate } = await import('../i18n/translate.mjs');
  const u = [];
  let refused = 0;
  for (const e of exported.u ?? []) {
    const ko = madeUpU(e.text, KO_U);
    if (uGate(e.text, ko, e.keys) === null) u.push({ hash: e.hash, len: e.len, text: ko });
    else refused += 1;
  }
  return { pack: [...packBytes({ base: fingerprint([]), runs: [], u })], u: u.length, refused };
}

const programCounts = (page) => page.evaluate(() => {
  const c = document.getElementById('screen').dataset;
  return {
    pack: document.body.dataset.localePackProgram,
    hits: Number(c.localeProgramHits ?? 0), misses: Number(c.localeProgramMisses ?? 0),
  };
});

// Whether the game's RGBA frame shows one of `glyphs` anywhere, as `u` draws a syllable: 8 pixels
// wide, its cell's rows 0..6 in a line, ink where the glyph has a bit and paper elsewhere (two
// colours). Compared in the page, never printed. Right after a restart the game that presented
// last is the one the restart freed until the new one presents: not yet shown.
const glyphOnScreen = (page, glyphs) => page.evaluate((rows) => {
  let f;
  try {
    f = window.__shownGame.frame_rgba();
  } catch {
    return false;
  }
  const at = (x, y) => (y * 320 + x) * 4;
  const same = (i, j) => f[i] === f[j] && f[i + 1] === f[j + 1] && f[i + 2] === f[j + 2];
  for (const g of rows) {
    let ink = null;
    let paper = null;
    for (let r = 0; r < 7 && (ink === null || paper === null); r += 1) {
      for (let c = 0; c < 8; c += 1) {
        if (g[r] & (0x80 >> c)) ink ??= [r, c];
        else paper ??= [r, c];
      }
    }
    for (let y = 0; y + 7 <= 200; y += 1) {
      for (let x = 0; x + 8 <= 320; x += 1) {
        const i = at(x + ink[1], y + ink[0]);
        const p = at(x + paper[1], y + paper[0]);
        if (same(i, p)) continue;
        let ok = true;
        for (let r = 0; r < 7 && ok; r += 1) {
          for (let c = 0; c < 8 && ok; c += 1) ok = same(at(x + c, y + r), g[r] & (0x80 >> c) ? i : p);
        }
        if (ok) return true;
      }
    }
  }
  return false;
}, [...glyphs.values()]);

// Steps of the opening until the made-up syllable shows (true) or `ticks` have passed (false).
async function waitGlyph(page, glyphs, ticks) {
  for (let waited = 0; waited < ticks; waited += 20) {
    if (await glyphOnScreen(page, glyphs)) return true;
    await h.waitTicks(page, 20);
  }
  return glyphOnScreen(page, glyphs);
}

test('Korean pack with the strings of u in a new game (the user\'s zip): the native string export also gives the strings of u, each of its own kind; a pack made at test time from that export (every string of u the made-up syllable within the tool\'s gate) shows the syllable in the first passage the opening shows, in the Hangul font; Forget the Korean pack: the opening in English; no string of u reaches the console, the page\'s data or text', async (env) => {
  const { page } = await h.openPage(env, { gameFirst: false, setup: { lang: 'ko' } });
  const words = consoleText(page);
  await h.load(page, env);
  const { TEXT } = await import('../../site/i18n.js');
  const ko = TEXT.ko;
  await h.dropFiles(page, [h.ZIP]);
  await started(page);
  await h.waitProgram(page, 'u');
  const before = await pageWords(page);
  const exported = await nativeStrings(h.ZIP);
  const u = exported.u ?? [];
  const kinds = new Set(['opening', 'story', 'gypsy', 'creation', 'transfer']);
  assert.ok(u.length > 0 && u.every((e) => kinds.has(e.kind)), 'strings of u, each of a kind of u');
  assert.ok(!u.some((e) => ['message', 'format', 'label', 'look', 'book'].includes(e.kind)), 'no kind of the game\'s strings or texts');
  assert.ok(['opening', 'story', 'gypsy', 'creation'].every((k) => u.some((e) => e.kind === k)), 'the opening, the story, the gypsy and creation');
  assert.ok(u.every((e) => fnv64([...Buffer.from(e.text, 'latin1')]) === e.hash && e.len === Buffer.from(e.text, 'latin1').length && e.len >= 3), 'each keyed by its hash');
  assert.ok(u.every((e) => e.keys === undefined || /^[A-Z]+$/.test(e.keys)), 'keys, when a key read follows, upper-case letters');
  assert.ok(u.some((e) => e.kind === 'gypsy' && e.keys !== undefined), 'the gypsy\'s questions with the keys she waits for');
  const fx = await koProgramFixture(exported);
  assert.ok(fx.u > 0 && fx.refused < fx.u, 'strings of u kept within the gate');
  // The pack: the game restarts with it, from the opening.
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: fx.pack }]);
  await h.waitSettled(page);
  await h.waitProgram(page, 'u');
  const shown = await localeShown(page);
  assert.deepEqual([shown.pack, shown.locale], ['0,0', 'on'], 'the pack on the game');
  assert.equal((await programCounts(page)).pack, String(fx.u), 'its strings of u, counted');
  assert.ok((await h.state(page)).status.includes(ko.koPackOn(0, 0, 0, 0, 0, 0, fx.u)), 'its line, counts only');
  const glyph = await hangulGlyphs(KO_U);
  const t0 = await page.evaluate(() => globalThis.__ticks);
  assert.ok(await waitGlyph(page, glyph, 1500), 'the made-up syllable in the first passage');
  const took = (await page.evaluate(() => globalThis.__ticks)) - t0;
  const counts = await programCounts(page);
  assert.ok(counts.hits >= 1, 'strings of u from the pack');
  // Forget the Korean pack: the game restarts in English, the same opening without the syllable.
  await forgetVia(page, '#forget-ko');
  await h.waitSettled(page);
  await h.waitProgram(page, 'u');
  const off = await localeShown(page);
  assert.deepEqual([off.pack, off.locale], ['none', 'off'], 'forgotten: English');
  assert.equal((await programCounts(page)).pack, 'none');
  assert.ok(!(await waitGlyph(page, glyph, took + 200)), 'the opening in English: no made-up syllable');
  await h.waitVisible(page);
  assert.deepEqual([(await programCounts(page)).hits], [0], 'nothing from a pack');
  const where = await pageWords(page);
  const long = u.map((e) => e.text.replace(/%[\s\S]/g, '').trim()).filter((t) => t.length >= 12 && !before.includes(t));
  assert.ok(long.length > 0, 'strings to look for');
  for (const w of long) {
    assert.ok(!where.includes(w), 'no string of u in the page\'s data, text or storage');
    assert.ok(!words.some((m) => m.includes(w)), 'nor in the console');
  }
  assert.ok(!where.includes(KO_U), 'nor the pack\'s text');
  console.log(`  receipt: the syllable shown ${took} steps after the start; strings of u put from the pack by then ${counts.hits}, missed ${counts.misses}`);
}, { needs: h.haveZip });

// --- the HD layout (spec 10) -------------------------------------------------------------------
// With HD on and the screen layout HD 16:9 (the page's default; h.openPage keeps Original unless
// a test asks for HD), the canvas is 1280 x 720, composed from the x4 frame: in game the map
// window and the command bar one to one, the date line, the side panel and the message scroll
// beside them at one scale, on the game's paper; every other screen the whole frame letterboxed.
// Clicks go through the page's map of it (`__u6Layout`, h.screenPoint). Frames are compared in
// the page, by equality only; the receipts print sizes, scales, places and counts the tests made.

// The parts of the game's screen the layout shows, as classic rects [x, y, width, height] (spec 10
// Measured facts, from the engine's code): the test's own reading, checked against the page's.
const LAYOUT_PARTS = {
  map: [8, 8, 160, 160],
  commands: [8, 176, 160, 17],
  date: [16, 168, 152, 8],
  panel: [168, 6, 144, 98],
  messages: [176, 112, 136, 80],
};

// A gated page (gameFirst) with the HD layout kept, in a `width` x `height` window, on the
// user's disks, started at its prompt with the canvas focused. The layout's tests compare the
// canvas with the x4 frame, so they keep the pixel font (`font`; spec 11: with the HD font the
// page draws characters over cells that are paper in that frame), unless a test asks. `init`, a
// page init script the test adds before the page loads.
async function bootLayout(env, { width = 1400, height = 900, font = 'pixel', init = null } = {}) {
  const { context, page } = await h.openPage(env, { layout: 'hd', font });
  if (init) await context.addInitScript(init);
  await page.setViewportSize({ width, height });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await page.focus('#screen');
  await h.waitPrompt(page);
  return { context, page };
}

// What the page shows of the layout: the body's layout, what the canvas shows, its size, its box
// (CSS px, its place, and device px) and scaling, the compositor's scale (0 with HD off), the
// stored choice.
const layoutShown = (page) => page.evaluate(() => {
  const c = document.getElementById('screen');
  const box = c.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const round = (v) => Math.round(v * 100) / 100;
  const g = globalThis.__shownGame;
  let stored = null;
  try {
    stored = localStorage.getItem('u6.layout');
  } catch {
    stored = 'unreadable';
  }
  return {
    layout: document.body.dataset.layout,
    view: globalThis.__u6Layout.view,
    size: [c.width, c.height],
    box: [box.width, box.height].map(round),
    at: [box.left, box.top].map(round),
    device: [box.width * dpr, box.height * dpr].map(round),
    viewport: [innerWidth, innerHeight],
    rendering: getComputedStyle(c).imageRendering,
    scale: g && g.hd_on() ? g.hd_scale() : 0,
    stored,
  };
});

// The page's layout table.
const layoutTable = (page) => page.evaluate(() => {
  const L = globalThis.__u6Layout;
  return { regions: L.regions, whole: L.whole, rest: L.rest, size: L.size };
});

// The canvas pixel `p` shows in region `r` (its classic rect `from`, its canvas rect `to`), the
// point clamped into the canvas rect: the nearest pixel of that region.
function regionPixel({ from, to }, [px, py]) {
  const at = (v, o, n, m) => Math.min(n - 1, Math.max(0, Math.floor(((v - o) * n) / m)));
  return [from[0] + at(px, to[0], from[2], to[2]), from[1] + at(py, to[1], from[3], to[3])];
}

// Page script: puts `window.__pointerArea()` in the page: the canvas rects the page draws the
// pointer in now (spec 11 Design 7: the HD frame is composed without the game's pointer sprite,
// and with HD on the page draws it over the frame through the layout, Game.hd_pointer), which the
// comparisons of the canvas with the frame it is drawn from leave out. Through each region whose
// classic rect the pointer's box meets (in `game`), the letterboxed frame (every other screen of
// the HD layout or Wide) or the frame as it is (07's canvas): its box mapped as that maps the
// frame's pixels, out to whole canvas pixels and clipped to it; none with HD off (the classic
// frame holds the sprite). `window.__inPointer(rects, x, y)`: canvas pixel (x, y) is in one.
const pointerAreaHook = (page) => page.evaluate(() => {
  window.__pointerArea = () => {
    const c = document.getElementById('screen');
    const g = globalThis.__shownGame;
    const L = globalThis.__u6Layout;
    if (!g || !g.hd_on()) return [];
    const [x, y, w, hh] = [...g.hd_pointer()];
    const frame = [0, 0, g.width(), g.height()];
    let regions = [{ from: frame, to: [0, 0, c.width, c.height] }];
    if (L.view === 'game') regions = L.regions;
    else if (L.view === 'whole') regions = [{ from: [0, 0, 320, 200], to: L.whole }];
    const area = [];
    for (const { from, to } of regions) {
      if (w === 0 || x >= from[0] + from[2] || x + w <= from[0] || y >= from[1] + from[3] || y + hh <= from[1]) continue;
      const [sx, sy] = [to[2] / from[2], to[3] / from[3]];
      const x0 = Math.max(to[0], Math.floor(to[0] + (x - from[0]) * sx));
      const y0 = Math.max(to[1], Math.floor(to[1] + (y - from[1]) * sy));
      const x1 = Math.min(to[0] + to[2], Math.ceil(to[0] + (x + w - from[0]) * sx));
      const y1 = Math.min(to[1] + to[3], Math.ceil(to[1] + (y + hh - from[1]) * sy));
      if (x0 < x1 && y0 < y1) area.push([x0, y0, x1 - x0, y1 - y0]);
    }
    return area;
  };
  window.__inPointer = (rects, px, py) => rects.some(([x, y, w, hh]) => px >= x && px < x + w && py >= y && py < y + hh);
});

// Per one-to-one region of the canvas (a canvas rect the size of its x4 rect), the pixels that
// differ from the x4 frame's and the pixels compared, read in the page; equality only. The
// pointer's place is left out (`pointerAreaHook`).
const oneToOne = async (page) => (await pointerAreaHook(page), page.evaluate(() => {
  const c = document.getElementById('screen');
  const g = globalThis.__shownGame;
  const w4 = g.width() * 4;
  const frame = g.hd_frame_rgba();
  const shown = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  const pointer = window.__pointerArea();
  const out = {};
  for (const { name, from, to } of globalThis.__u6Layout.regions) {
    if (to[2] !== 4 * from[2] || to[3] !== 4 * from[3]) continue;
    let differ = 0;
    let compared = 0;
    for (let y = 0; y < to[3]; y += 1) {
      for (let x = 0; x < to[2]; x += 1) {
        if (window.__inPointer(pointer, to[0] + x, to[1] + y)) continue;
        compared += 1;
        const a = ((to[1] + y) * c.width + to[0] + x) * 4;
        const b = ((4 * from[1] + y) * w4 + 4 * from[0] + x) * 4;
        if (shown[a] !== frame[b] || shown[a + 1] !== frame[b + 1] || shown[a + 2] !== frame[b + 2]
          || shown[a + 3] !== frame[b + 3]) differ += 1;
      }
    }
    out[name] = { differ, compared };
  }
  return out;
}));

// Per region named (a region of the game screen, or `whole`: the letterboxed frame), what the
// canvas shows of the x4 frame where a scaled copy is that colour whatever its filter: the game
// pixels of the region whose 3 x 3 neighbourhood in the x4 frame is one flat colour (`compared`;
// `lit` those not black, `colours` how many colours), and how many of them the canvas, at the
// centre of what shows each, does not show within 2 of each channel (`differ`). Read in the
// page, the display frozen by the caller; counts the receipts never print (they are the user's
// picture). The pointer's place is left out (`pointerAreaHook`).
const flatShown = async (page, names) => (await pointerAreaHook(page), page.evaluate((wanted) => {
  const c = document.getElementById('screen');
  const L = globalThis.__u6Layout;
  const g = globalThis.__shownGame;
  const f = g.hd_frame_rgba();
  const [w, hh] = [g.width(), g.height()];
  const w4 = w * 4;
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  const rects = Object.fromEntries(L.regions.map((r) => [r.name, r.from]));
  rects.whole = [0, 0, w, hh];
  const pointer = window.__pointerArea();
  const out = {};
  for (const name of wanted) {
    const [fx, fy, fw, fh] = rects[name];
    let compared = 0;
    let lit = 0;
    let differ = 0;
    const colours = new Set();
    for (let gy = Math.max(fy, 1); gy < Math.min(fy + fh, hh - 1); gy += 1) {
      for (let gx = Math.max(fx, 1); gx < Math.min(fx + fw, w - 1); gx += 1) {
        const ref = (4 * gy * w4 + 4 * gx) * 4;
        let one = true;
        for (let y = 4 * (gy - 1); y < 4 * (gy + 2) && one; y += 1) {
          for (let x = 4 * (gx - 1); x < 4 * (gx + 2); x += 1) {
            const i = (y * w4 + x) * 4;
            if (f[i] !== f[ref] || f[i + 1] !== f[ref + 1] || f[i + 2] !== f[ref + 2]) {
              one = false;
              break;
            }
          }
        }
        if (!one) continue;
        const [cx, cy] = L.toCanvas(gx, gy).map(Math.floor);
        if (window.__inPointer(pointer, cx, cy)) continue;
        const i = (cy * c.width + cx) * 4;
        compared += 1;
        colours.add((f[ref] << 16) | (f[ref + 1] << 8) | f[ref + 2]);
        if (f[ref] + f[ref + 1] + f[ref + 2] > 0) lit += 1;
        if ([0, 1, 2].some((k) => Math.abs(d[i + k] - f[ref + k]) > 2)) differ += 1;
      }
    }
    out[name] = { compared, lit, differ, colours: colours.size };
  }
  return out;
}, names));

// The least flat pixels a scaled region of the game screen must offer the check (its paper
// round the text and the pictures gives many more at the boot).
const FLAT_MIN = { date: 8, panel: 100, messages: 100 };

// A hash of the canvas's pixels in canvas rect [x, y, w, h], read in the page.
const rectHash = (page, rect) => page.evaluate(([x, y, w, hh]) => {
  const d = document.getElementById('screen').getContext('2d').getImageData(x, y, w, hh).data;
  let v = 0x811c9dc5;
  for (let i = 0; i < d.length; i += 1) v = Math.imul(v ^ d[i], 0x01000193) >>> 0;
  return v;
}, rect);

// Page script, once the game presented: every pointer event the page gives the game on screen
// from now, as [x, y, buttons], in `window.__pointer` (the game's own method underneath).
const recordPointer = (page) => page.evaluate(() => {
  const g = globalThis.__shownGame;
  const real = Object.getPrototypeOf(g).pointer;
  window.__pointer = [];
  g.pointer = function (x, y, buttons) {
    window.__pointer.push([x, y, buttons]);
    return real.call(this, x, y, buttons);
  };
});
const pointerSince = (page, n) => page.evaluate((from) => window.__pointer.slice(from), n);
const pointerCount = (page) => page.evaluate(() => window.__pointer.length);

// Presses and releases the left button at canvas pixel `p` (the mouse moved there first), then
// until the game took both: the pointer events the page gave it for that.
async function canvasClick(page, p, settle = 0) {
  const n = await pointerCount(page);
  await page.mouse.click(...(await h.canvasPoint(page, p[0] + 0.5, p[1] + 0.5)));
  await h.waitTaken(page);
  await h.waitTicks(page, settle);
  return pointerSince(page, n);
}

test('HD layout (spec 10 Acceptance 1): in game in a 1400 x 900 window the canvas is 1280 x 720 in a box of as many CSS px, scaled sharp, the compositor at S 4; the map window and the command bar on it equal their rects of the x4 frame pixel for pixel; the date line, the side panel and the message scroll at one scale of 3.5..3.75 in the column beside them, centred, showing the x4 frame\'s colours, the largest that puts every rect on whole pixels; every rect inside the canvas, margins at least 4 px and the same all round; every canvas pixel of a region maps to a game pixel of its rect and back; no page error', async (env) => {
  const errors = [];
  const { page } = await bootLayout(env);
  page.on('pageerror', () => errors.push(1));
  const shown = await layoutShown(page);
  assert.ok(shown.layout === 'hd' && shown.view === 'game', 'the HD layout in game');
  assert.deepEqual(shown.size, [1280, 720], 'the canvas 1280 x 720');
  assert.deepEqual(shown.box, [1280, 720], 'in a box of as many CSS px');
  assert.equal(shown.scale, 4, 'the compositor at S 4');
  assert.ok(SHARP.includes(shown.rendering), 'one to one: scaled sharp');
  const { regions } = await layoutTable(page);
  assert.deepEqual(regions.map((r) => r.name), Object.keys(LAYOUT_PARTS), 'five regions, in order');
  for (const r of regions) {
    assert.deepEqual(r.from, LAYOUT_PARTS[r.name], `${r.name}: its classic rect`);
    const [x, y, w, hh] = r.to;
    assert.ok(r.to.every(Number.isInteger), `${r.name}: on whole pixels`);
    assert.ok(x >= 0 && y >= 0 && x + w <= 1280 && y + hh <= 720, `${r.name}: inside the canvas`);
  }
  for (const [i, a] of regions.entries()) {
    for (const b of regions.slice(i + 1)) {
      const apart = a.to[0] + a.to[2] <= b.to[0] || b.to[0] + b.to[2] <= a.to[0]
        || a.to[1] + a.to[3] <= b.to[1] || b.to[1] + b.to[3] <= a.to[1];
      assert.ok(apart, `${a.name} and ${b.name} do not overlap`);
    }
  }
  const [map, commands, ...column] = regions;
  for (const r of [map, commands]) assert.ok(r.to[2] === 4 * r.from[2] && r.to[3] === 4 * r.from[3], `${r.name}: x4, one to one`);
  assert.ok(commands.to[1] > map.to[1] && commands.to[0] === map.to[0], 'the command bar under the map window');
  const s = column[0].to[2] / column[0].from[2];
  assert.ok(s >= 3.5 && s <= 3.75, 'the column\'s scale in 3.5..3.75');
  for (const r of column) assert.ok(r.to[2] === s * r.from[2] && r.to[3] === s * r.from[3], `${r.name}: at the column's scale`);
  const m = map.to[0];
  assert.ok(m >= 4, 'margins at least 4 px');
  assert.ok(map.to[1] === m && commands.to[1] - (map.to[1] + map.to[3]) === m && 720 - (commands.to[1] + commands.to[3]) === m,
    'the left column: the same margin at the top, between and at the bottom');
  const [x0, x1] = [map.to[0] + map.to[2] + m, 1280 - m];
  for (const r of column) {
    assert.ok(r.to[0] >= x0 && r.to[0] + r.to[2] <= x1, `${r.name}: in the column`);
    assert.equal(r.to[0] - x0, x1 - (r.to[0] + r.to[2]), `${r.name}: centred in the column`);
  }
  assert.ok(column[0].to[1] === m && 720 - (column[2].to[1] + column[2].to[3]) === m, 'the column: the same margin at the top and the bottom');
  const gaps = [0, 1].map((i) => column[i + 1].to[1] - (column[i].to[1] + column[i].to[3]));
  assert.ok(gaps.every((g) => g >= 4) && Math.abs(gaps[0] - gaps[1]) <= 1, 'the date line, the panel, the scroll in order, at least 4 px apart, the gaps within a pixel of each other');
  // The largest scale: none above it in 3.5..3.75 (on a 1/400 grid) puts every column rect on
  // whole pixels and fits the column's height with margins and gaps of at least 4 px.
  const height = column.reduce((n, r) => n + r.from[3], 0);
  for (let k = Math.round(s * 400) + 1; k <= 3.75 * 400; k += 1) {
    const t = k / 400;
    const whole = column.every((r) => Number.isInteger(t * r.from[2]) && Number.isInteger(t * r.from[3]));
    assert.ok(!whole || t * height + 4 * 4 > 720, 'no larger scale of the range fits on whole pixels');
  }
  // The canvas as drawn, the display frozen: the one-to-one regions against the x4 frame.
  await h.virtualDisplay(page, 60);
  const same = await oneToOne(page);
  assert.deepEqual(Object.keys(same), ['map', 'commands'], 'the map window and the command bar one to one');
  for (const [name, { differ }] of Object.entries(same)) assert.equal(differ, 0, `${name}: the x4 frame's pixels`);
  // The scaled regions (the column): the x4 frame's colours where a scaled copy shows them.
  const flat = await flatShown(page, column.map((r) => r.name));
  for (const r of column) {
    assert.ok(flat[r.name].compared >= FLAT_MIN[r.name], `${r.name}: flat pixels to compare`);
    assert.equal(flat[r.name].differ, 0, `${r.name}: the x4 frame's colours at the column's scale`);
  }
  assert.ok(flat.panel.colours >= 2, 'the panel: more than one colour compared');
  // Every canvas pixel of a region: the game pixel it reaches is in that region's rect, and that
  // pixel's centre is shown within half a game pixel of it; the corners and centre of each rect
  // go there and back; a game pixel outside the regions is not shown.
  const trips = await page.evaluate(() => {
    const L = globalThis.__u6Layout;
    let bad = 0;
    let swept = 0;
    let round = 0;
    for (const { from: [fx, fy, fw, fh], to: [x, y, w, hh] } of L.regions) {
      const [sx, sy] = [w / fw, hh / fh];
      for (let py = y; py < y + hh; py += 1) {
        for (let px = x; px < x + w; px += 1) {
          swept += 1;
          const [gx, gy] = L.toGame(px + 0.5, py + 0.5);
          const inside = gx >= fx && gx < fx + fw && gy >= fy && gy < fy + fh;
          const back = inside ? L.toCanvas(gx, gy) : null;
          if (!back || Math.abs(back[0] - px - 0.5) > sx / 2 || Math.abs(back[1] - py - 0.5) > sy / 2) bad += 1;
        }
      }
      for (const [gx, gy] of [[fx, fy], [fx + fw - 1, fy], [fx, fy + fh - 1], [fx + fw - 1, fy + fh - 1], [fx + (fw >> 1), fy + (fh >> 1)]]) {
        round += 1;
        const p = L.toCanvas(gx, gy);
        const g = L.toGame(p[0], p[1]);
        if (g[0] !== gx || g[1] !== gy) bad += 1;
      }
    }
    const hidden = [[0, 0], [319, 199], [4, 100], [170, 106]].filter(([gx, gy]) => L.toCanvas(gx, gy) !== null).length;
    return { bad, swept, round, hidden };
  });
  assert.equal(trips.bad, 0, 'canvas pixels and game pixels map both ways');
  assert.equal(trips.hidden, 0, 'a game pixel outside the regions is not shown');
  assert.equal(errors.length, 0, 'no page error');
  const compared = Object.values(same).map((r) => r.compared).join(' and ');
  console.log(`  receipt: canvas 1280x720 in a ${shown.box.join('x')} box (${shown.rendering}); map window and command bar ${compared} pixels compared with the x4 frame, ${Object.values(same).reduce((n, r) => n + r.differ, 0)} differ; the date line, the panel and the scroll show the x4 frame's colours; the column at ${s}, margins ${m} px, the column's gaps ${gaps.join(' and ')} px; ${trips.swept} canvas pixels and ${trips.round} round trips mapped: pass`);
}, { needs: h.haveHfes });

test('HD layout (spec 10 Acceptance 2): clicks through the layout reach the game at the pixel they show: a map cell walks toward it or is Look\'s target, each of the ten command icons selects its command (a right press underlines that icon alone), Look waits for its target and Escape takes it back, the party roster\'s figure opens that member\'s panel and the canvas shows the new panel; in a gap or a margin a move with no button reaches the nearest pixel of the nearest region, a press, its release and a move with a button held the rest of the screen, the original\'s click there: at the prompt a pass (no walk, no command), while Look waits for its target a cancel', async (env) => {
  const { page } = await bootLayout(env);
  await recordPointer(page);
  const { regions, rest } = await layoutTable(page);
  const [map, commands, date] = regions;
  const at = (name) => regions.find((r) => r.name === name);
  // The rest of the screen: in none of the regions' classic rects.
  assert.ok(Object.values(LAYOUT_PARTS).every(([x, y, w, hh]) => rest[0] < x || rest[0] >= x + w || rest[1] < y || rest[1] >= y + hh),
    'the rest of the screen lies outside every region');
  // Moves with no button in gaps and margins: the nearest pixel of the nearest region.
  const gapMoves = [
    [map, [map.to[0] + map.to[2] + 2, 300], 'between the map window and the column'],
    [map, [300, map.to[1] + map.to[3] + 1], 'between the map window and the command bar'],
    [commands, [300, commands.to[1] - 1], 'between the command bar and the map window'],
    [map, [1, 1], 'in the top left margin'],
    [date, [960, date.to[1] + date.to[3] + 6], 'under the date line'],
    [at('messages'), [1279, 719], 'in the bottom right margin'],
  ];
  for (const [region, p, label] of gapMoves) {
    const n = await pointerCount(page);
    await page.mouse.move(...(await h.canvasPoint(page, p[0] + 0.5, p[1] + 0.5)));
    await page.waitForFunction((k) => window.__pointer.length > k, n, { timeout: 5000 });
    const sent = await pointerSince(page, n);
    assert.deepEqual(sent.at(-1), [...regionPixel(region, [p[0] + 0.5, p[1] + 0.5]), 0], `a move ${label}: the nearest pixel of the ${region.name}`);
  }
  // A press and its release in gaps and a margin, at the prompt: the rest of the screen; the
  // original takes it as a pass (ui/click.rs region 3, the command loop's 0x8f), so the world
  // turn runs and the prompt comes back with the hero walked nowhere (a foe of the start fight
  // that kills him in that turn wakes him on the start cell: h.WAKE_CELL).
  const gapPresses = [
    [[map.to[0] + map.to[2] + 2, 300], 'between the map window and the column'],
    [[300, map.to[1] + map.to[3] + 1], 'between the map window and the command bar'],
    [[1, 1], 'in the top left margin'],
  ];
  for (const [p, label] of gapPresses) {
    await h.waitPrompt(page);
    const from = (await h.state(page)).leader;
    const sent = await canvasClick(page, p);
    const press = sent.find(([, , b]) => b === 1);
    const release = sent.findLast(([, , b]) => b === 0);
    assert.ok(press && release, `a press ${label}: the press and the release reached the game`);
    assert.deepEqual([press.slice(0, 2), release.slice(0, 2)], [rest, rest], `a press ${label}: the rest of the screen`);
    await h.waitPrompt(page);
    const to = (await h.state(page)).leader;
    assert.ok(to === from || to === h.WAKE_CELL, `a press ${label}: no walk`);
    assert.equal(await promptNow(page), '1', `a press ${label}: no command waits`);
  }
  // A press on the map (the hero's own cell: a pass), then a move into a gap with the button
  // still held: the rest of the screen (no walk target there), and the release there too.
  await h.waitPrompt(page);
  let mark = await pointerCount(page);
  await page.mouse.move(...(await h.screenPoint(page, 88, 88)));
  await page.mouse.down();
  const gap = [map.to[0] + map.to[2] + 2, 500];
  await page.mouse.move(...(await h.canvasPoint(page, gap[0] + 0.5, gap[1] + 0.5)));
  await page.waitForFunction((k) => window.__pointer.slice(k).some(([, , b]) => b === 1), mark, { timeout: 5000 });
  let held = await pointerSince(page, mark);
  const down = held.findIndex(([, , b]) => b === 1);
  assert.deepEqual(held[down], [88, 88, 1], 'the press on the map at its pixel');
  await page.waitForFunction((k) => window.__pointer.length > k, mark + down + 1, { timeout: 5000 });
  held = await pointerSince(page, mark);
  assert.deepEqual(held[down + 1], [...rest, 1], 'the held move into the gap: the rest of the screen, the button held');
  await page.mouse.up();
  await h.waitTaken(page);
  held = await pointerSince(page, mark);
  assert.deepEqual(held.at(-1), [...rest, 0], 'its release there: the rest of the screen');
  await h.waitPrompt(page);
  // The command icons (x = 8 + 16 i, y 176..191): a right press on each underlines it and no
  // other (ui/cursor.rs: row 192 under it in the command colour, one flat colour, the same for
  // every icon; the old underline painted over in paper), and runs nothing. Read with the
  // pointer moved off the bar (its arrow would lie over the row), in the page.
  const underlined = (sel) => page.evaluate((i) => {
    const c = document.getElementById('screen');
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
    const L = globalThis.__u6Layout;
    const rows = Array.from({ length: 10 }, (_, n) => Array.from({ length: 16 }, (_, k) => {
      const [x, y] = L.toCanvas(8 + 16 * n + k, 192).map(Math.floor);
      const j = (y * c.width + x) * 4;
      return `${d[j]},${d[j + 1]},${d[j + 2]}`;
    }));
    const inked = (row) => row.every((p) => p === window.__ink);
    window.__ink ??= rows[i][0];
    return { inked: inked(rows[i]), others: rows.filter((row, n) => n !== i && inked(row)).length };
  }, sel);
  for (let i = 0; i < 10; i += 1) {
    const n = await pointerCount(page);
    await page.mouse.click(...(await h.screenPoint(page, 8 + 16 * i + 8, 184)), { button: 'right' });
    await h.waitTaken(page);
    await page.mouse.move(...(await h.screenPoint(page, 88, 40)));
    await h.waitTaken(page);
    await h.waitTicks(page, 3);
    const press = (await pointerSince(page, n)).find(([, , b]) => b === 2);
    assert.deepEqual(press, [8 + 16 * i + 8, 184, 2], `icon ${i}: the right press at its pixel`);
    const u = await underlined(i);
    assert.ok(u.inked && u.others === 0, `icon ${i}: underlined, alone`);
    assert.equal(await promptNow(page), '1', `icon ${i}: the right press runs nothing`);
  }
  // Look (the fourth icon) by a left click waits for its target; Escape takes it back.
  await h.click(page, 64, 184, 30);
  assert.equal(await promptNow(page), '0', 'Look waits for its target');
  await page.keyboard.press('Escape');
  await h.waitPrompt(page);
  // Look's target on a map cell, the hero's own (window cell (5, 5), centred on game pixel
  // (88, 88)): the description's portrait waits for a key or a click; a click on that cell
  // answers it and walks nowhere (as `lookAtHero`).
  await h.click(page, 64, 184, 30);
  const here = (await h.state(page)).leader;
  mark = await pointerCount(page);
  await h.click(page, 88, 88, 90);
  assert.deepEqual((await pointerSince(page, mark)).find(([, , b]) => b === 1), [88, 88, 1], 'Look\'s target pressed at the hero\'s pixel');
  assert.equal(await promptNow(page), '0', 'Look on the hero: the portrait waits');
  await h.click(page, 88, 88, 0);
  await h.waitPrompt(page);
  assert.equal((await h.state(page)).leader, here, 'the click that closes the portrait does not walk');
  // Look again; a press in a gap cancels it (region 3: the target cleared) and walks nowhere.
  await h.click(page, 64, 184, 30);
  assert.equal(await promptNow(page), '0', 'Look waits for its target again');
  const looking = (await h.state(page)).leader;
  const cancel = await canvasClick(page, [map.to[0] + map.to[2] + 2, 300]);
  assert.deepEqual(cancel.find(([, , b]) => b === 1).slice(0, 2), rest, 'the press in the gap: the rest of the screen');
  await h.waitPrompt(page);
  const after = (await h.state(page)).leader;
  assert.ok(after === looking || after === h.WAKE_CELL, 'the cancel walks nowhere');
  // A map cell: clicks walk (one step toward a cell beside the hero, `clickStep`).
  const walked = await clickStep(page);
  assert.notEqual(walked.to, walked.from, 'a click on the map walks');
  // The roster's first member, on the figures (x 0xb1..0xc0): their inventory panel, whose - and
  // + the game then takes (ui/click.rs roster_click).
  // The pointer over the figure first, so the panel's canvas before and after the click differs
  // by what the game drew in the panel only (its arrow is there both times).
  const panel = at('panel');
  await page.mouse.move(...(await h.screenPoint(page, 184, 32)));
  await h.waitTaken(page);
  await h.waitTicks(page, 5);
  const roster = await rectHash(page, panel.to);
  mark = await pointerCount(page);
  await h.click(page, 184, 32, 0);
  assert.deepEqual((await pointerSince(page, mark)).find(([, , b]) => b === 1), [184, 32, 1], 'the press on the figure at its pixel');
  await page.waitForFunction(() => globalThis.__owned === 3, null, { timeout: 10_000 });
  // The panel's new picture reached the canvas over the frame shown before (the dirty rect's
  // path: a scaled region drawn again whole): it changed, and it is the x4 frame's, the display
  // frozen.
  await h.waitTicks(page, 10);
  await h.virtualDisplay(page, 60);
  assert.notEqual(await rectHash(page, panel.to), roster, 'the panel\'s canvas changed');
  const now = (await flatShown(page, ['panel'])).panel;
  assert.ok(now.compared >= FLAT_MIN.panel, 'the new panel: flat pixels to compare');
  assert.equal(now.differ, 0, 'the new panel: the x4 frame\'s colours');
  console.log(`  receipt: ${gapMoves.length} gap moves at the nearest region pixel; ${gapPresses.length} gap presses at the rest of the screen, no walk; a held move into a gap and its release at the rest of the screen; 10 icons underlined by a right press; Look on the hero's cell; Look cancelled by a gap press; a map click walked; the roster figure opened the member's panel, drawn again on the canvas from the x4 frame: pass`);
}, { needs: h.haveHfes });

test('HD layout (spec 10 Acceptance 3): the opening and the menu show the whole x4 frame letterboxed, 1152 x 720 centred with the sides dark, the frame\'s colours where it shows them; their clicks still land, in the opening and in the menu: a press reaches the game at the pixel it shows (on a dark side at the nearest edge) and the game takes it; the keys still work', async (env) => {
  const { page } = await h.openPage(env, { gameFirst: false, layout: 'hd' });
  await page.setViewportSize({ width: 1400, height: 900 });
  await h.load(page, env);
  await h.dropFiles(page, [h.ZIP]);
  await started(page);
  await h.waitProgram(page, 'u');
  await h.waitTicks(page, 60);
  let shown = await layoutShown(page);
  assert.ok(shown.layout === 'hd' && shown.view === 'whole', 'the opening: the whole frame');
  assert.deepEqual(shown.size, [1280, 720], 'on the 1280 x 720 canvas');
  const { whole } = await layoutTable(page);
  assert.deepEqual(whole, [64, 0, 1152, 720], 'letterboxed: 1152 x 720 (16:10), centred');
  // The sides: one dark colour, the same on both.
  const sides = () => page.evaluate(([x, w]) => {
    const c = document.getElementById('screen');
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
    const first = `${d[0]},${d[1]},${d[2]}`;
    let off = 0;
    let n = 0;
    for (let y = 0; y < c.height; y += 1) {
      for (const [a, b] of [[0, x], [x + w, c.width]]) {
        for (let px = a; px < b; px += 1) {
          n += 1;
          const i = (y * c.width + px) * 4;
          if (`${d[i]},${d[i + 1]},${d[i + 2]}` !== first) off += 1;
        }
      }
    }
    return { off, n, dark: d[0] + d[1] + d[2] < 3 * 48 };
  }, [whole[0], whole[2]]);
  let side = await sides();
  assert.ok(side.off === 0 && side.dark, 'the opening: the sides dark');
  // A click while the opening runs: it lands at the pixel it shows and the game takes it.
  await recordPointer(page);
  let mark = await pointerCount(page);
  await page.mouse.click(...(await h.screenPoint(page, 120, 80)));
  await h.waitTaken(page);
  assert.deepEqual((await pointerSince(page, mark)).find(([, , b]) => b === 1), [120, 80, 1], 'a press in the opening lands there');
  assert.ok((await h.state(page)).program === 'u' && (await layoutShown(page)).view === 'whole', 'still the opening, letterboxed');
  await page.focus('#screen');
  await h.press(page, 'Escape', 120);
  assert.equal((await h.state(page)).program, 'u', 'Escape: the menu');
  shown = await layoutShown(page);
  assert.equal(shown.view, 'whole', 'the menu: the whole frame');
  side = await sides();
  assert.ok(side.off === 0 && side.dark, 'the menu: the sides dark');
  // Clicks: at game pixels through the page's map, and on the dark sides.
  const points = [[160, 100], [0, 0], [319, 199], [40, 150], [300, 20]];
  for (const [gx, gy] of points) {
    mark = await pointerCount(page);
    await page.mouse.click(...(await h.screenPoint(page, gx, gy)));
    await h.waitTaken(page);
    const press = (await pointerSince(page, mark)).find(([, , b]) => b === 1);
    assert.deepEqual(press, [gx, gy, 1], `a press at (${gx}, ${gy}) lands there`);
  }
  // On a side: the nearest edge pixel of the frame (the point clamped into the letterbox).
  const edge = ([px, py]) => [
    Math.min(319, Math.max(0, Math.floor(((px - whole[0]) * 320) / whole[2]))),
    Math.min(199, Math.max(0, Math.floor(((py - whole[1]) * 200) / whole[3]))),
  ];
  for (const [p, label] of [[[20, 360], 'the left side'], [[1260, 700], 'the right side']]) {
    const want = edge([p[0] + 0.5, p[1] + 0.5]);
    assert.ok(want[0] === (p[0] < 640 ? 0 : 319), `${label}: an edge pixel`);
    const sent = await canvasClick(page, p);
    assert.deepEqual(sent.find(([, , b]) => b === 1), [...want, 1], `a press on ${label}: the nearest edge pixel`);
  }
  assert.equal((await h.state(page)).program, 'u', 'still the menu');
  // The frame where it shows: game pixels whose x4 block and its neighbours' are one colour show
  // that colour (the scaled frame there is that colour whatever the filter), the display frozen.
  await h.virtualDisplay(page, 60);
  const content = (await flatShown(page, ['whole'])).whole;
  assert.ok(content.lit >= 20, 'the menu shows colours to compare');
  assert.equal(content.differ, 0, 'the frame\'s colours where the letterbox shows them');
  // How many of the frame's pixels were one colour is the user's picture: compared, not printed.
  console.log(`  receipt: the opening and the menu letterboxed at ${whole.join(',')}, sides of one dark colour (${side.n} pixels); ${points.length + 3} presses landed (one in the opening); the frame's colours where the letterbox shows them: pass`);
}, { needs: h.haveZip });

test('HD layout (spec 10 Acceptance 4): fullscreen keeps 16:9 at any size, centred on black, the canvas still 1280 x 720: at 1920 x 1080 the whole screen, smooth; at 2560 x 1440 the whole screen, sharp (x2); at 1728 x 1117 1728 x 972 with bars, smooth; a key reaches the game in each; the page\'s own fullscreen (no requestFullscreen) the same', async (env) => {
  const sizes = [[1920, 1080, 'smooth'], [2560, 1440, 'sharp'], [1728, 1117, 'smooth']];
  const lines = [];
  const check = async (page, mode, label) => {
    for (const [w, hh, want] of sizes) {
      await page.setViewportSize({ width: w, height: hh });
      await page.click('#fullscreen');
      await waitFull(page, mode);
      await page.waitForFunction(([vw]) => Math.abs(document.getElementById('screen').getBoundingClientRect().width - Math.min(vw, (innerHeight * 16) / 9)) < 1, [w], { timeout: 10_000 });
      await h.waitFrames(page, 2);
      const s = await layoutShown(page);
      const [bw, bh] = s.box;
      assert.ok(Math.abs(bw / bh - 16 / 9) < 0.01, `${label} ${w}x${hh}: 16:9`);
      assert.ok(bw <= w + 0.5 && bh <= hh + 0.5 && (Math.abs(bw - w) < 1 || Math.abs(bh - hh) < 1), `${label} ${w}x${hh}: as large as fits`);
      assert.ok(Math.abs(s.at[0] - (w - bw) / 2) < 1 && Math.abs(s.at[1] - (hh - bh) / 2) < 1, `${label} ${w}x${hh}: centred`);
      assert.deepEqual(s.size, [1280, 720], `${label} ${w}x${hh}: the canvas stays 1280 x 720`);
      const mode2 = SHARP.includes(s.rendering) ? 'sharp' : s.rendering === 'auto' ? 'smooth' : s.rendering;
      assert.equal(mode2, want, `${label} ${w}x${hh}: ${want}`);
      const before = (await glue(page)).sent;
      await h.press(page, 'Space', 2);
      const g = await glue(page);
      const key = g.sent === before + 1 && g.lastSent === ' ';
      assert.ok(key, `${label} ${w}x${hh}: a key reaches the game`);
      lines.push(`${label} ${w}x${hh}: box ${bw}x${bh} at ${s.at.join(',')}, ${mode2}, a key reached the game`);
      // The Exit button shows while the pointer moves (it hides 2 s after).
      await page.mouse.move(200, 200);
      await page.mouse.move(220, 210);
      await page.click('#fullscreen-exit');
      await waitFull(page, 'off');
      await h.waitPrompt(page);
    }
  };
  let { context, page } = await bootLayout(env);
  await check(page, 'on', 'fullscreen');
  await context.close();
  ({ context, page } = await h.openPage(env, { layout: 'hd' }));
  await context.addInitScript(() => { delete Element.prototype.requestFullscreen; });
  await page.setViewportSize({ width: 1400, height: 900 });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await page.focus('#screen');
  await h.waitPrompt(page);
  await check(page, 'css', 'the page\'s own');
  for (const line of lines) console.log(`  receipt: ${line}`);
}, { needs: h.haveHfes });

// Page init script: every read and write of the page's storage throws, the layout's too.
function layoutStorageThrows() {
  for (const name of ['getItem', 'setItem', 'removeItem']) {
    Storage.prototype[name] = () => {
      throw new DOMException('test: storage refused', 'SecurityError');
    };
  }
}

test('HD layout (spec 10 Acceptance 5, Design 6): Settings\' "Screen layout" (Wide, HD 16:9, Original 16:10, in the page\'s language) switches the game on screen at once and is kept; any other kept value is HD; Original with HD on is 07\'s canvas (1280 x 800 at S 4 in a 16:10 box); HD off with the HD layout chosen is the page as before (the 320 x 200 frame, a 16:10 box, the pointer through the box); after an engine trap a switch changes neither the canvas nor its box; with storage that throws the layout is HD, switches for the visit and nothing is kept; no page error', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const errors = [];
  let { context, page } = await bootLayout(env, { width: 1024 });
  page.on('pageerror', () => errors.push(1));
  let s = await layoutShown(page);
  assert.ok(s.layout === 'hd' && s.view === 'game' && s.size[0] === 1280 && s.size[1] === 720, 'HD 16:9');
  const column = await page.evaluate(() => {
    const main = document.querySelector('main');
    const pad = getComputedStyle(main);
    return main.clientWidth - parseFloat(pad.paddingLeft) - parseFloat(pad.paddingRight);
  });
  assert.ok(column < 1280 && Math.abs(s.box[0] - column) < 0.5 && Math.abs(s.box[1] - (column * 9) / 16) < 0.5, 'a 16:9 box as wide as the column');
  assert.equal(s.rendering, 'auto', 'more canvas pixels than box pixels: smooth');
  await page.click('#settings');
  const choice = () => page.evaluate(() => ({
    title: document.getElementById('layout-title').textContent,
    labels: [...document.querySelectorAll('#layout-choice label')].map((l) => l.textContent.replace(/\s+/g, ' ').trim()),
    values: [...document.querySelectorAll('input[name="layout"]')].map((i) => [i.value, i.checked]),
    inSettings: document.getElementById('setup').contains(document.getElementById('layout-choice')),
  }));
  let c = await choice();
  assert.ok(c.inSettings, 'in Settings');
  assert.deepEqual([c.title, c.labels], [TEXT.en.layout, [TEXT.en.layoutWide, 'HD 16:9', TEXT.en.layoutOriginal]], 'its words in English');
  assert.deepEqual(c.values, [['wide', false], ['hd', true], ['original', false]], 'HD 16:9 checked');
  await page.check('input[name="layout"][value="original"]');
  s = await layoutShown(page);
  assert.ok(s.layout === 'original' && s.view === 'frame' && s.stored === 'original', 'Original at once, kept');
  assert.ok(s.size[0] === 1280 && s.size[1] === 800 && s.scale === 4, '07\'s canvas: the x4 frame');
  assert.deepEqual(s.box, [960, 600], 'in a 16:10 box');
  assert.ok(await canvasShowsGame(page), 'the HD frame as it is');
  await page.check('input[name="layout"][value="hd"]');
  s = await layoutShown(page);
  assert.ok(s.layout === 'hd' && s.view === 'game' && s.size[1] === 720 && s.stored === 'hd', 'HD 16:9 again at once, kept');
  await page.check('input[name="setup-lang"][value="ko"]');
  c = await choice();
  assert.deepEqual([c.title, c.labels], [TEXT.ko.layout, [TEXT.ko.layoutWide, 'HD 16:9', TEXT.ko.layoutOriginal]], 'its words in Korean');
  await page.check('input[name="setup-lang"][value="en"]');
  // HD off: the page as before, whatever the layout chosen.
  await page.click('#hd');
  s = await layoutShown(page);
  assert.ok(s.layout === 'original' && s.view === 'frame' && s.size[0] === 320 && s.size[1] === 200, 'HD off: the 320 x 200 frame');
  assert.deepEqual(s.box, [960, 600], 'in a 16:10 box');
  assert.ok(await canvasShowsGame(page), 'the RGBA frame');
  const identity = await page.evaluate(() => [globalThis.__u6Layout.toGame(88.5, 88.5), globalThis.__u6Layout.toCanvas(88, 88)]);
  assert.deepEqual(identity, [[88, 88], [88.5, 88.5]], 'the page\'s map: the canvas is the frame');
  await page.mouse.move(...(await h.screenPoint(page, 88, 88)));
  await page.waitForFunction(() => globalThis.__pointerAt?.[0] === 88 && globalThis.__pointerAt[1] === 88, null, { timeout: 5000 });
  await page.click('#hd');
  s = await layoutShown(page);
  assert.ok(s.layout === 'hd' && s.view === 'game' && s.size[1] === 720, 'HD on: the HD layout again');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  // Kept across a reload, as chosen; another value is HD (spec 13: `wide` is the Wide build's).
  for (const [kept, want] of [['original', 'original'], ['hd', 'hd'], ['other', 'hd']]) {
    await page.evaluate((v) => localStorage.setItem('u6.layout', v), kept);
    await page.reload();
    await started(page, FILES);
    await h.waitPrompt(page);
    s = await layoutShown(page);
    assert.equal(s.layout, want, `kept ${kept}: ${want}`);
    assert.ok(want === 'hd' ? s.size[1] === 720 && s.view === 'game' : s.size[1] === 800 && s.view === 'frame', `kept ${kept}: its canvas`);
  }
  // After an engine trap nothing calls into the engine again: a switch of the layout or of HD
  // leaves the canvas as it was, in the box it had, until the reload (the choice still kept).
  await page.evaluate(() => { globalThis.__trap = true; });
  await page.waitForFunction(() => document.body.dataset.state === 'crashed');
  const trapped = await canvasHash(page);
  await page.click('#settings');
  await page.check('input[name="layout"][value="original"]');
  s = await layoutShown(page);
  assert.ok(s.layout === 'hd' && s.size[0] === 1280 && s.size[1] === 720 && s.stored === 'original', 'trapped: Original kept, the canvas and its box as they were');
  await page.click('#hd');
  s = await layoutShown(page);
  assert.ok(s.layout === 'hd' && s.size[0] === 1280 && s.size[1] === 720, 'trapped: HD off leaves them too');
  assert.equal(await canvasHash(page), trapped, 'trapped: the canvas not drawn again');
  await context.close();
  // Storage that throws: HD 16:9, Original for the visit, nothing kept.
  ({ context, page } = await h.openPage(env, { layout: null }));
  await context.addInitScript(layoutStorageThrows);
  page.on('pageerror', () => errors.push(1));
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  s = await layoutShown(page);
  assert.ok(s.stored === 'unreadable' && s.layout === 'hd' && s.size[1] === 720, 'storage that throws: HD 16:9');
  await page.click('#settings');
  await page.check('input[name="layout"][value="original"]');
  s = await layoutShown(page);
  assert.ok(s.layout === 'original' && s.size[1] === 800, 'Original for the visit');
  await page.reload();
  await started(page, FILES);
  await h.waitPrompt(page);
  s = await layoutShown(page);
  assert.ok(s.layout === 'hd' && s.size[1] === 720, 'nothing kept: HD 16:9 after a reload');
  assert.equal(errors.length, 0, 'no page error');
}, { needs: h.haveHfes });

// The paper round the regions (spec 10 Design 2, follow-up L2), the test's own reading, checked
// against the page's: the game's paper, its outer band (6 game rows at the top and the bottom, 8
// columns at either side: in game the source plane shows the paper bitmap alone there, the
// pointer's sprite apart) 9-sliced to the canvas's edges from the x4 frame: `from` a rect of the
// band in game pixels, `to` canvas pixels, `flip` mirrored across x or y (the right edge, the
// bottom edge and the bottom-right corner are the left edge, the top edge and the bottom-left
// corner mirrored); inside the band, the parchment in one colour, each channel's median of the
// x4 frame's top-left pixel of the three game pixels `samples`.
const LAYOUT_PAPER = {
  pieces: [
    { name: 'top-left', from: [0, 0, 8, 6], to: [0, 0, 32, 24], flip: null },
    { name: 'top', from: [8, 0, 304, 6], to: [32, 0, 1216, 24], flip: null },
    { name: 'top-right', from: [312, 0, 8, 6], to: [1248, 0, 32, 24], flip: null },
    { name: 'left', from: [0, 6, 8, 188], to: [0, 24, 32, 672], flip: null },
    { name: 'right', from: [0, 6, 8, 188], to: [1248, 24, 32, 672], flip: 'x' },
    { name: 'bottom-left', from: [0, 194, 8, 6], to: [0, 696, 32, 24], flip: null },
    { name: 'bottom', from: [8, 0, 304, 6], to: [32, 696, 1216, 24], flip: 'y' },
    { name: 'bottom-right', from: [0, 194, 8, 6], to: [1248, 696, 32, 24], flip: 'x' },
  ],
  parchment: [32, 24, 1216, 672],
  samples: [[204, 104], [240, 104], [276, 104]],
};
// The page's paint round the regions before the paper (spec 10 v1): a dark fill and a 1 px frame
// round each region, as RGB.
const LAYOUT_V1 = [[0x0b, 0x0b, 0x0f], [0x3a, 0x3f, 0x4c]];

// Page script: the canvas outside the game's regions (the gaps and the margins) against the paper
// composed by the test from the x4 frame as it is now, by LAYOUT_PAPER's rects (the same draws:
// the unscaled pieces sharp, the stretched edges smooth, mirrored by a transform; then the
// parchment's colour from the frame's samples). Per pixel compared: `differ`, not that paper;
// `v1`, the old dark fill's or frame's colour; the unscaled pieces also read straight against the
// frame's pixels, mirrored by index (`direct` of `directCompared`), and the parchment against its
// colour (`parchmentDiffer` of `parchment`). `touched`: the game pixels of the pieces' sources
// whose source is not the paper bitmap alone (the engine's kind 3, id 0: u6-render BITMAP_PAPER),
// `covered` the samples not the paper alone; `fill` the parchment's colour, `uncovered` the
// colours of the samples that are the paper alone, and `hash` the pixels compared, for checks
// that compare them (never printed: they are the user's picture).
const paperShown = (page) => page.evaluate((own) => {
  const c = document.getElementById('screen');
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  const g = globalThis.__shownGame;
  const L = globalThis.__u6Layout;
  const [w4, h4] = [g.width() * 4, g.height() * 4];
  const f = g.hd_frame_rgba();
  const x4 = document.createElement('canvas');
  [x4.width, x4.height] = [w4, h4];
  x4.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(f), w4, h4), 0, 0);
  const e = document.createElement('canvas');
  [e.width, e.height] = [c.width, c.height];
  const ctx = e.getContext('2d');
  for (const { from, to: [x, y, w, hh], flip } of own.pieces) {
    const source = from.map((v) => v * 4);
    ctx.save();
    ctx.imageSmoothingEnabled = source[2] !== w || source[3] !== hh;
    ctx.imageSmoothingQuality = 'high';
    ctx.translate(flip === 'x' ? x + w : x, flip === 'y' ? y + hh : y);
    ctx.scale(flip === 'x' ? -1 : 1, flip === 'y' ? -1 : 1);
    ctx.drawImage(x4, ...source, 0, 0, w, hh);
    ctx.restore();
  }
  const at = ([gx, gy], k) => f[(4 * gy * w4 + 4 * gx) * 4 + k];
  const colour = (p) => (at(p, 0) << 16) | (at(p, 1) << 8) | at(p, 2);
  const fill = [0, 1, 2].map((k) => own.samples.map((p) => at(p, k)).sort((a, b) => a - b)[1]);
  ctx.fillStyle = `rgb(${fill.join(', ')})`;
  ctx.fillRect(...own.parchment);
  const ex = ctx.getImageData(0, 0, c.width, c.height).data;
  const inRect = (px, py, [x, y, w, hh]) => px >= x && px < x + w && py >= y && py < y + hh;
  const r = { compared: 0, differ: 0, v1: 0, direct: 0, directCompared: 0, parchment: 0, parchmentDiffer: 0, hash: 0x811c9dc5 };
  for (let py = 0; py < c.height; py += 1) {
    for (let px = 0; px < c.width; px += 1) {
      if (L.regions.some(({ to }) => inRect(px, py, to))) continue;
      const i = (py * c.width + px) * 4;
      r.compared += 1;
      if (d[i] !== ex[i] || d[i + 1] !== ex[i + 1] || d[i + 2] !== ex[i + 2] || d[i + 3] !== ex[i + 3]) r.differ += 1;
      if (own.v1.some(([a, b, q]) => d[i] === a && d[i + 1] === b && d[i + 2] === q)) r.v1 += 1;
      for (let k = 0; k < 4; k += 1) r.hash = Math.imul(r.hash ^ d[i + k], 0x01000193) >>> 0;
      if (inRect(px, py, own.parchment)) {
        r.parchment += 1;
        if (d[i] !== fill[0] || d[i + 1] !== fill[1] || d[i + 2] !== fill[2]) r.parchmentDiffer += 1;
        continue;
      }
      const piece = own.pieces.find(({ to }) => inRect(px, py, to));
      if (!piece || piece.to[2] !== 4 * piece.from[2] || piece.to[3] !== 4 * piece.from[3]) continue;
      const [dx, dy] = [px - piece.to[0], py - piece.to[1]];
      const sx = 4 * piece.from[0] + (piece.flip === 'x' ? piece.to[2] - 1 - dx : dx);
      const sy = 4 * piece.from[1] + (piece.flip === 'y' ? piece.to[3] - 1 - dy : dy);
      const j = (sy * w4 + sx) * 4;
      r.directCompared += 1;
      if (d[i] !== f[j] || d[i + 1] !== f[j + 1] || d[i + 2] !== f[j + 2]) r.direct += 1;
    }
  }
  const paper = (gx, gy) => {
    const s = g.hd_source_at(gx, gy);
    return s[0] === 3 && s[1] === 0;
  };
  const sources = new Set(own.pieces.map(({ from }) => from.join(',')));
  r.touched = 0;
  for (const key of sources) {
    const [x, y, w, hh] = key.split(',').map(Number);
    for (let gy = y; gy < y + hh; gy += 1) for (let gx = x; gx < x + w; gx += 1) r.touched += paper(gx, gy) ? 0 : 1;
  }
  r.covered = own.samples.filter(([gx, gy]) => !paper(gx, gy)).length;
  r.fill = (fill[0] << 16) | (fill[1] << 8) | fill[2];
  r.uncovered = [...new Set(own.samples.filter(([gx, gy]) => paper(gx, gy)).map(colour))];
  return r;
}, { ...LAYOUT_PAPER, v1: LAYOUT_V1 });

// The canvas round the regions is the paper as the x4 frame shows it now (`paperShown`).
function paperAsFrame(s, label) {
  assert.ok(s.compared > 0 && s.differ === 0, `${label}: the gaps and margins the 9-slice of the x4 frame and the parchment, pixel for pixel`);
  assert.ok(s.directCompared > 0 && s.direct === 0, `${label}: the unscaled pieces the x4 frame's pixels, mirrored where they are`);
  assert.ok(s.parchment > 0 && s.parchmentDiffer === 0, `${label}: the parchment one colour, each channel the samples' median`);
  assert.equal(s.v1, 0, `${label}: no pixel of the old dark fill or frame`);
}

// Page init script: the engine's wasm memory (the instance whose exports hold the HD frame) kept
// in `window.__wasmMemory`, so a test can write pixels of the x4 frame where the page reads them.
function wasmMemoryHook() {
  const keep = (result) => {
    const instance = result instanceof WebAssembly.Instance ? result : result.instance;
    if (instance && 'game_hd_frame_ptr' in instance.exports) window.__wasmMemory = instance.exports.memory;
    return result;
  };
  const streaming = WebAssembly.instantiateStreaming;
  if (streaming) WebAssembly.instantiateStreaming = (...args) => streaming.apply(WebAssembly, args).then(keep);
  const instantiate = WebAssembly.instantiate;
  WebAssembly.instantiate = (...args) => instantiate.apply(WebAssembly, args).then(keep);
}

// Page script: the x4 frame in the engine's memory at the game pixels `at` (the top-left x4 pixel
// of each, the one the page reads for the parchment): with `colour` (RGB, the test's own) those
// pixels kept in the page and that colour written there; with null, the kept pixels put back.
// The engine writes a block of its frame again only where the screen changes, so the paper's
// pixels keep what is written until then.
const writeFramePixels = (page, at, colour) => page.evaluate(([points, rgb]) => {
  const g = globalThis.__shownGame;
  const view = new Uint8Array(window.__wasmMemory.buffer, g.hd_frame_ptr(), g.hd_frame_len());
  const w4 = g.width() * 4;
  if (rgb !== null) window.__framePixelsHeld = [];
  points.forEach(([gx, gy], n) => {
    const i = (4 * gy * w4 + 4 * gx) * 4;
    if (rgb === null) {
      view.set(window.__framePixelsHeld[n], i);
      return;
    }
    window.__framePixelsHeld.push(view.slice(i, i + 3));
    view.set([(rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255], i);
  });
}, [at, colour]);

test('HD layout (spec 10 Design 2, follow-up L2): in game the canvas round the regions is the game\'s paper from the x4 frame: its outer band (in game only the paper there, the source plane says, the pointer apart) 9-sliced to the canvas\'s edges, the corners and the top and bottom edges unscaled, the side edges stretched, the right and bottom ones mirrored from the left and top; inside it the parchment in one colour, the median of three pixels of the frame; no pixel of the old dark fill or frames; the pieces tile the canvas; the rects that paint it whole outside every region; the pointer\'s sprite never in it (spec 11 Design 7: where the game starts it, moved into the map, the cross-hair at the map\'s left edge, the band as the frame shows it and the same throughout); a source of the band changed in the frame (by the test) and a repaint under the pointer meeting the band\'s sources paints it whole with that change; two samples changed in the frame (by the test) paint it whole, the parchment their new median, and put back whole again; the sprite over one sample leaves the parchment\'s colour with no whole repaint; the regions still the x4 frame\'s; no page error', async (env) => {
  const errors = [];
  const { page } = await bootLayout(env, { init: wasmMemoryHook });
  page.on('pageerror', () => errors.push(1));
  assert.deepEqual(await page.evaluate(() => globalThis.__u6Layout.paper), LAYOUT_PAPER, 'the paper\'s pieces, the parchment and its samples: the test\'s reading');
  // The pieces and the parchment tile the canvas: every canvas pixel in exactly one of them.
  const cover = new Uint8Array(1280 * 720);
  for (const [x, y, w, hh] of [...LAYOUT_PAPER.pieces.map((p) => p.to), LAYOUT_PAPER.parchment]) {
    for (let py = y; py < y + hh; py += 1) for (let px = x; px < x + w; px += 1) cover[py * 1280 + px] += 1;
  }
  assert.ok(cover.every((n) => n === 1), 'the band\'s pieces and the parchment tile the canvas');
  // The samples are the paper's own: outside every region, inside the band's inner edge.
  for (const [gx, gy] of LAYOUT_PAPER.samples) {
    assert.ok(!Object.values(LAYOUT_PARTS).some(([x, y, w, hh]) => gx >= x && gx < x + w && gy >= y && gy < y + hh), 'a sample outside the regions');
  }
  // What paints the canvas whole (the page's PAPER_SOURCES, x4 frame pixels): every rect outside
  // every region's classic rect, so a change in the regions alone never does.
  const { paperSources, regions } = await page.evaluate(() => ({ paperSources: globalThis.__u6Layout.paperSources, regions: globalThis.__u6Layout.regions }));
  const meets = ([ax, ay, aw, ah], [bx, by, bw, bh]) => ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah;
  assert.ok(paperSources.length > 0 && paperSources.every((r) => regions.every(({ from }) => !meets(r, from.map((v) => 4 * v)))), 'every rect that paints the paper whole outside every region');
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  // At the boot, no pointer event yet: the game's sprite where it starts it, over the band's top
  // left in the shown frame (its sources there not the paper alone), and the canvas the paper as
  // the x4 frame shows it: the x4 frame is composed without the sprite (spec 11 Design 7), and the
  // page draws the pointer through the regions only.
  let s = await paperShown(page);
  assert.ok(s.touched > 0, 'the boot: the pointer\'s sprite over the band where the game starts it, in the shown frame');
  paperAsFrame(s, 'the boot');
  const boot = s.hash;
  // The pointer into the map: its sprite leaves the band, which is the paper alone; the band as
  // at the boot (the sprite was never in it).
  await page.mouse.move(...(await h.screenPoint(page, 88, 88)));
  await h.runFrames(page, 10);
  s = await paperShown(page);
  assert.ok(s.touched === 0 && s.covered === 0, 'in game, the pointer in the map: the band and the samples the paper alone');
  paperAsFrame(s, 'the pointer in the map');
  assert.equal(s.hash, boot, 'the band as at the boot: the x4 frame holds no sprite');
  const clean = s;
  // From here, the rects the page paints again and its whole repaints, through frames after a move
  // of the pointer (`moveSeen`): the rects as `dirtySeen` tells them (a move of the pointer alone
  // paints again under its old and new boxes, `recordDirty`), whether any met a source of the
  // band, and the whole repaints since.
  const bandRects = LAYOUT_PAPER.pieces.map(({ from }) => from.map((v) => 4 * v));
  const startSeeing = async () => {
    await recordDirty(page);
    await countWholePaints(page);
  };
  const moveSeen = async () => {
    const seen = await dirtySeen(page, bandRects[0]);
    for (const r of bandRects) seen.meets ||= (await dirtySeen(page, r)).meets;
    return { ...seen, whole: await page.evaluate(() => window.__wholePaints) };
  };
  const partial = (m) => m.rects > 0 && !m.full && !m.meets && m.sameGame && m.view === 'game';
  // Two samples changed in the frame alone: the test writes its own colour into the engine's x4
  // frame at two of them (no image of the paper changing, so nothing else of the frame does),
  // then the pointer moves in the map, a partial repaint that meets no source of the band. The
  // page finds the samples' median not the parchment it shows and paints whole, the parchment
  // the new median; the pixels put back, the next such repaint whole again, the paper as before.
  const OWN = 0xff00ff;
  assert.notEqual(clean.fill, OWN, 'precondition: the test\'s colour not the parchment\'s');
  const two = LAYOUT_PAPER.samples.slice(0, 2);
  await writeFramePixels(page, two, OWN);
  await startSeeing();
  await page.mouse.move(...(await h.screenPoint(page, 100, 100)));
  await h.runFrames(page, 10);
  let m = await moveSeen();
  assert.ok(partial(m), 'precondition: the pointer moved in the map, a partial repaint meeting no source of the band');
  assert.ok(m.whole > 0, 'two samples changed: the canvas painted whole');
  s = await paperShown(page);
  paperAsFrame(s, 'two samples changed');
  assert.equal(s.fill, OWN, 'two samples changed: the parchment their new median');
  await writeFramePixels(page, two, null);
  await startSeeing();
  await page.mouse.move(...(await h.screenPoint(page, 88, 88)));
  await h.runFrames(page, 10);
  m = await moveSeen();
  assert.ok(partial(m), 'precondition: the pointer moved back, a partial repaint meeting no source of the band');
  assert.ok(m.whole > 0, 'the samples put back: the canvas painted whole again');
  s = await paperShown(page);
  paperAsFrame(s, 'the samples put back');
  assert.equal(s.hash, clean.hash, 'the samples put back: the paper as with the pointer in the map before');
  // The sprite over one sample in the shown frame (the arrow from the side panel's foot): the
  // parchment as before, with no whole repaint (a sample is not among the rects that paint the
  // canvas whole, and the x4 frame, without the sprite, keeps the median the canvas shows).
  await startSeeing();
  let over = null;
  for (const [gx, gy] of [[240, 103], [240, 101], [239, 102], [238, 100], [236, 99]]) {
    await page.mouse.move(...(await h.screenPoint(page, gx, gy)));
    await h.runFrames(page, 10);
    over = await paperShown(page);
    if (over.covered > 0) break;
  }
  assert.equal(over.covered, 1, 'the pointer\'s sprite over one sample');
  paperAsFrame(over, 'the sprite over a sample');
  assert.ok(over.uncovered.length === 1 && over.fill === over.uncovered[0] && over.fill === clean.fill, 'the parchment the colour of the samples that are the paper alone, as before');
  m = await moveSeen();
  assert.ok(partial(m), 'precondition: the pointer moved to the sample, partial repaints meeting no source of the band');
  assert.equal(m.whole, 0, 'the sprite over one sample: no whole repaint');
  // Look waits for its target (the cross-hair): at the map's left edge it reaches into the band's
  // left strip in the shown frame, which the right edge mirrors; the x4 frame holds no sprite, so
  // the band stays as it was.
  await page.mouse.move(...(await h.screenPoint(page, 88, 88)));
  await h.runFrames(page, 10);
  await page.focus('#screen');
  await page.keyboard.press('KeyL');
  for (let k = 0; k < 40 && await page.evaluate(() => document.getElementById('screen').dataset.keys !== '0'); k += 1) await h.runFrames(page, 5);
  await page.mouse.move(...(await h.screenPoint(page, 8, 88)));
  await h.runFrames(page, 10);
  s = await paperShown(page);
  assert.ok(s.touched > 0, 'Look: the cross-hair at the map\'s left edge over the band, in the shown frame');
  paperAsFrame(s, 'the cross-hair over the band');
  assert.equal(s.hash, clean.hash, 'the cross-hair over the band\'s source: the band as before (the x4 frame holds no sprite)');
  // A source of the band changed: the test's own colour written into the x4 frame at the top
  // piece's game pixel (100, 0) (unscaled on the canvas's top row, outside every region, and
  // mirrored on its bottom row), and the next rect the page takes widened to it, as a composition
  // that wrote there gives it (`recordDirty`'s `__widenTake`). Then the cross-hair moved down the
  // map's left edge: the rect the page paints again meets the band's sources (PAPER_SOURCES), so
  // the canvas is painted whole and the band shows the change. The pixel is put back the same way
  // afterwards.
  const corner = [[100, 0]];
  const cornerRect = [400, 0, 4, 4];
  await writeFramePixels(page, corner, OWN);
  await startSeeing();
  await page.evaluate((r) => { window.__widenTake = r; }, cornerRect);
  await page.mouse.move(...(await h.screenPoint(page, 8, 100)));
  await h.runFrames(page, 10);
  m = await moveSeen();
  assert.ok(m.rects > 0 && m.meets && !m.full && m.sameGame && m.view === 'game', 'precondition: a partial repaint whose rect (the widened take; the pointer\'s move only makes the draw) meets a source of the band');
  s = await paperShown(page);
  paperAsFrame(s, 'a source of the band changed');
  assert.notEqual(s.hash, clean.hash, 'a source of the band changed: the band painted again with it');
  assert.ok(m.whole > 0, 'a source of the band changed: the canvas painted whole');
  await writeFramePixels(page, corner, null);
  await page.evaluate((r) => { window.__widenTake = r; }, cornerRect);
  await page.keyboard.press('Escape');
  for (let k = 0; k < 40 && await page.evaluate(() => document.getElementById('screen').dataset.keys !== '0'); k += 1) await h.runFrames(page, 5);
  await page.mouse.move(...(await h.screenPoint(page, 88, 88)));
  await h.runFrames(page, 10);
  s = await paperShown(page);
  assert.equal(s.touched, 0, 'the cross-hair gone: the band the paper alone');
  paperAsFrame(s, 'the target taken back');
  assert.equal(s.hash, clean.hash, 'the paper as with the pointer in the map before');
  // The regions over the paper: the one-to-one ones still the x4 frame's pixels.
  for (const [name, { differ }] of Object.entries(await oneToOne(page))) assert.equal(differ, 0, `${name}: the x4 frame's pixels`);
  assert.equal(errors.length, 0, 'no page error');
  console.log(`  receipt: ${LAYOUT_PAPER.pieces.length} pieces of the band and the parchment tile the canvas; ${s.compared} canvas pixels round the regions the paper from the x4 frame (${s.directCompared} of them read straight from the frame), none the old fill or frame; the pointer's sprite never in it (at the boot, moved into the map, the cross-hair at the map's edge); a source of the band changed: painted whole with it; two samples changed in the frame: painted whole with their median, and again once put back; the sprite over a sample: the parchment's colour kept, no whole repaint: pass`);
}, { needs: h.haveHfes });

// --- the HD font (spec 11) ----------------------------------------------------------------------
// With the HD layout and the HD font the compositor leaves each letter, syllable or whole space
// cell its paper in the x4 frame and lists it (Game.hd_glyphs), and the page draws the text over
// the cells' places on the canvas in runs (Design 3). The listed codes are the user's text (but
// those of the test's own conversation): the checks tell them apart by kind (ASCII, a Hangul
// syllable, a space) and compare in the page; the receipts print no code and no count of them.

const FONT_PATH = '/fonts/gowun-batang.woff2';

// The test's own conversation for the font (koFixture's script with this greeting): two spaces
// between two words (a run after them keeps its column), a word the font kerns (AVAW: drawn
// without kerning, Design 3), a keyword (`@`: the game's highlight colour when its help mode is
// on), and its Korean with the same (`@{…}`: the pack's keyword).
const FONT_GREET = '"Hail,  friend! AVAW? Ask of @tests."';
const FONT_GREET_KO = '"어서 오시오,  나그네여! @{시험}을 물으시오."';

// Every answer to a request for the font file in `context`, as [its origin, its status, its
// content type].
function fontAnswers(context) {
  const seen = [];
  context.on('response', (r) => {
    const url = new URL(r.url());
    if (url.pathname === FONT_PATH) seen.push([url.origin, r.status(), r.headers()['content-type']]);
  });
  return seen;
}

// The font as the page shows it: data-font and data-font-file, the compositor's font mode and
// whether it lists any cell, the stored choice, the Font radios' values and checks.
const fontShown = (page) => page.evaluate(() => {
  const g = globalThis.__shownGame;
  let stored = null;
  try {
    stored = localStorage.getItem('u6.font');
  } catch {
    stored = 'unreadable';
  }
  return {
    font: document.body.dataset.font,
    file: document.body.dataset.fontFile,
    mode: g ? g.hd_font() : null,
    listed: g ? g.hd_glyphs().length > 0 : false,
    stored,
    radios: [...document.querySelectorAll('input[name="font"]')].map((i) => [i.value, i.checked]),
  };
});

const fontIs = (page, font) => page.waitForFunction((f) => document.body.dataset.font === f
  && document.body.dataset.fontFile !== 'loading', font, { timeout: 30_000 });

// Page init script: the test's own reading of Design 3 (`window.__fontRunsOf(list, from)`, the
// page's is not used): the listed cells of `list` (Game.hd_glyphs) in the classic rect `from`,
// in runs: one row, each cell the next one's left neighbour, one paper; one space joins the
// cells either side, two or more end the run, so does a cell not listed; a space the player typed
// (0x20000 | 0x20, rev 3) joins them whatever the spaces beside it; the spaces at a run's ends
// left out. Each run: its row, its cells ({ x, code, ink }, a space's ink null), whether it
// follows two or more spaces (`after`), its inks; and `parts`, the texts the page is to draw it
// in (one per stretch of one ink, a space going with the stretch it stands in). A listed symbol
// (0x10000 | its glyph: the key wait's cursor, the more prompt's mark; Design 2 rev 3) ends the
// cells before it; when a run of the same cells stands before it, after its last cell or after
// the spaces that follow it, it is that run's `caret` ({ x, code: its glyph, ink, spaces: the
// spaces between, typed or not }), else it is one of `runs.symbols` ({ x, y, code, ink }), drawn
// at its own cell. With `rows` (Game.hd_symbol_rows, read with `list`) each symbol also has its
// 8 rows (`rows`, bit 7 leftmost), else null. Also every `fillText` on the game's canvas is kept in
// `window.__fills` while that is an array, and every `fillRect` on it (a whole repaint's
// background: `paint` with no dirty rect) is counted in `window.__wholePaints`.
function fontRunsHook() {
  window.__fontRunsOf = (list, [fx, fy, fw, fh], rows = null) => {
    const runs = [];
    runs.symbols = [];
    let chain = null;
    let symbols = 0; // the symbols of the whole list so far: their rows' place in `rows`
    const close = () => {
      if (chain?.run) runs.push(chain.run);
      if (chain) chain.run = null;
    };
    for (let i = 0; i < list.length; i += 5) {
      const [x, y, code, ink, paper] = [list[i], list[i + 1], list[i + 2], list[i + 3], list[i + 4]];
      const symbol = code >= 0x10000 && code !== 0x20020;
      const own = symbol && rows && rows.length >= 8 * symbols + 8 ? rows.slice(8 * symbols, 8 * symbols + 8) : null;
      if (symbol) symbols += 1;
      if (x + 8 <= fx || y + 8 <= fy || x >= fx + fw || y >= fy + fh) continue;
      if (!chain || chain.y !== y || chain.next !== x || chain.paper !== paper) {
        close();
        chain = { y, paper, spaces: 0, gap: 0, run: null, last: null };
      }
      chain.next = x + 8;
      if (code === 0x20 || code === 0x20020) {
        chain.gap += 1;
        if (code === 0x20) chain.spaces += 1;
        if (chain.spaces === 2) close();
        continue;
      }
      if (symbol) {
        const glyph = code & 0xffff;
        if (chain.last) chain.last.caret = { x, code: glyph, ink, spaces: chain.gap, rows: own };
        else runs.symbols.push({ x, y, code: glyph, ink, rows: own });
        close();
        chain = null;
        continue;
      }
      if (!chain.run) {
        chain.run = { y, cells: [], after: chain.spaces >= 2, caret: null };
        chain.last = chain.run;
      } else {
        for (let k = chain.gap; k >= 1; k -= 1) chain.run.cells.push({ x: x - 8 * k, code: 0x20, ink: null });
      }
      chain.spaces = 0;
      chain.gap = 0;
      chain.run.cells.push({ x, code, ink });
    }
    close();
    for (const run of runs) {
      run.inks = new Set(run.cells.filter((c) => c.ink !== null).map((c) => c.ink));
      run.parts = [];
      let ink = null;
      for (const c of run.cells) {
        const ch = String.fromCharCode(c.code);
        if (c.ink === null || c.ink === ink) run.parts[run.parts.length - 1] += ch;
        else {
          run.parts.push(ch);
          ink = c.ink;
        }
      }
    }
    return runs;
  };
  // The test's own measure of where a run of `__fontRunsOf` (in the classic rect `from` that the
  // canvas rect `to` shows) is drawn (Design 3) and its caret (Design 2 rev 3): its span, its text,
  // the narrowing `k`, the pen; its ink's left and right ends and the pen's end (`inkStart`,
  // `inkEnd`, `penEnd`), a space's advance as drawn (`space`); with a caret, where its cell's left
  // edge belongs (`caretLeft`: the pen's end after the run's text and the spaces between, their
  // advances measured here) and the caret's box (`caret`): its cell's 8 x 8 game pixels from there,
  // each edge on the nearest whole canvas pixel. And `__symbolBox`: a symbol's own cell there,
  // its edges likewise.
  window.__runGeometry = (run, from, to) => {
    const [sx, sy] = [to[2] / from[2], to[3] / from[3]];
    const ch = 8 * sy;
    window.__measure ??= document.createElement('canvas').getContext('2d');
    const mc = window.__measure;
    mc.font = `${0.85 * ch}px "U6 Gowun Batang"`;
    mc.fontKerning = 'none';
    const [x0, x1] = [run.cells[0].x, run.cells.at(-1).x + 8];
    const span = [to[0] + (x0 - from[0]) * sx, to[1] + (run.y - from[1]) * sy, (x1 - x0) * sx, ch];
    const text = run.cells.map((k) => String.fromCharCode(k.code)).join('');
    const m = mc.measureText(text);
    const lead = Math.max(0, m.actualBoundingBoxLeft);
    const natural = Math.max(m.width, lead + m.actualBoundingBoxRight);
    const room = span[2] - 2;
    const k = natural > room ? room / natural : 1;
    const pen = span[0] + 1 + lead * k;
    const out = {
      span, text, k, pen,
      inkStart: pen - m.actualBoundingBoxLeft * k,
      inkEnd: pen + m.actualBoundingBoxRight * k,
      penEnd: pen + m.width * k,
      space: mc.measureText(' ').width * k,
      caretLeft: null,
      caret: null,
    };
    if (run.caret) {
      const left = pen + mc.measureText(text + ' '.repeat(run.caret.spaces)).width * k;
      const [l, t] = [Math.round(left), Math.round(span[1])];
      out.caretLeft = left;
      out.caret = [l, t, Math.round(left + 8 * sx) - l, Math.round(span[1] + ch) - t];
    }
    return out;
  };
  // Where the page drew a symbol (`mark`: its rows and its ink) on the canvas pixels `d` (RGBA, of
  // width `cw`), read from the pixels alone: the first cell's left edge from `x0` to `x1` (canvas
  // px) at which, on the cell rows from `top`, the centre of each of its set pixels' blocks
  // (sx x sy canvas px each) is its ink (within 8 a channel); then its ink's observed left edge,
  // the first column of its leftmost set pixel's block that is its ink, walking left from that
  // block's centre (at most one block). `{ edge, lead }`, `lead` the canvas px of the glyph's own
  // blank columns left of its ink; null when no place shows it.
  window.__symbolEdge = (d, cw, mark, [x0, x1], top, sx, sy) => {
    const rows = mark?.rows;
    if (!rows) return null;
    const cols = rows.reduce((m, r) => m | r, 0);
    if (cols === 0) return null;
    const u0 = Math.clz32(cols) - 24;
    const v0 = rows.findIndex((r) => r & (0x80 >> u0));
    const [r, g, b] = [(mark.ink >> 16) & 255, (mark.ink >> 8) & 255, mark.ink & 255];
    const ink = (x, y) => {
      const j = (y * cw + x) * 4;
      return Math.max(Math.abs(d[j] - r), Math.abs(d[j + 1] - g), Math.abs(d[j + 2] - b)) <= 8;
    };
    const set = [];
    for (let v = 0; v < 8; v += 1) for (let u = 0; u < 8; u += 1) if (rows[v] & (0x80 >> u)) set.push([u, v]);
    for (let left = Math.floor(x0); left <= Math.ceil(x1); left += 1) {
      if (!set.every(([u, v]) => ink(Math.floor(left + (u + 0.5) * sx), Math.floor(top + (v + 0.5) * sy)))) continue;
      const y = Math.floor(top + (v0 + 0.5) * sy);
      let x = Math.floor(left + (u0 + 0.5) * sx);
      for (let n = 0; n < Math.ceil(sx) && ink(x - 1, y); n += 1) x -= 1;
      return { edge: x, lead: u0 * sx };
    }
    return null;
  };
  window.__symbolBox = (cell, from, to) => {
    const [sx, sy] = [to[2] / from[2], to[3] / from[3]];
    const [left, top] = [to[0] + (cell.x - from[0]) * sx, to[1] + (cell.y - from[1]) * sy];
    const [l, t] = [Math.round(left), Math.round(top)];
    return [l, t, Math.round(left + 8 * sx) - l, Math.round(top + 8 * sy) - t];
  };
  const real = CanvasRenderingContext2D.prototype.fillText;
  CanvasRenderingContext2D.prototype.fillText = function fillText(text, ...rest) {
    if (Array.isArray(window.__fills) && this.canvas.id === 'screen') window.__fills.push(String(text));
    return real.call(this, text, ...rest);
  };
  window.__wholePaints = 0;
  const rect = CanvasRenderingContext2D.prototype.fillRect;
  CanvasRenderingContext2D.prototype.fillRect = function fillRect(...args) {
    if (this.canvas.id === 'screen') window.__wholePaints += 1;
    return rect.apply(this, args);
  };
}

// Page script: the runs as the canvas shows them now, region by region of the game screen,
// against the regions composed again from the x4 frame as it is (the page's own copies, rect for
// rect: one to one, or scaled smooth; right after a whole composition, a change of the font,
// every region was copied from that frame). A run's span: its first cell's left edge to its last
// cell's right edge, over its cell rows, through the region's map; its ink: the pixels in its
// span that differ from the composition. Where a run's ink belongs, by the test's own measure of
// Design 3: its text (the joined spaces in it) measured in the HD font at 0.85 of the cell's
// height, no kerning; the pen 1 pixel inside the span's left edge and on by the ink the first
// character puts left of it; the run at that width, or narrowed to the span less 1 pixel at
// each end; so its ink box from the pen less the measured left reach to the pen plus the right
// reach (each scaled as the run is). Per region: `off`, the pixels that differ outside every
// span (of `offCompared`); the runs, their cells, syllables and ASCII; how many runs show ink,
// stand on whole pixels, have their ink where it belongs within FONT_TOL pixels at each end
// (`fits`: natural spacing, the inset, the narrowing); the runs of four or more ASCII and no
// syllable (`latin`); of the runs with a syllable, those whose ink stands at least 0.6 of the
// cell's height and is centred down it (within a sixth); of the runs after two or more spaces,
// those that fit (so start at their own first cell); of the runs of two inks, those showing both
// (a pixel within 48 a channel of each) and whose second ink starts where the text before it
// ends (within FONT_TOL + 1: its first pixel of that colour); `unlike`: the runs whose span
// differs from the canvas `fontPixelShot` kept (the pixel font's). A listed symbol's box (Design 2
// rev 3: a run's caret at its pen's end and the spaces' advances, `__runGeometry`; a symbol after
// no run at its own cell, `__symbolBox`) is left out of the runs' ink and of `off`: `carets` and
// `symbols` count them, `caretsPlaced` and `symbolsPlaced` those whose ink's observed left edge on
// the canvas (`__symbolEdge`) is where it belongs, its glyph's blank columns on, within a canvas
// pixel. The canvas, the spans and the symbols' boxes are kept in the page (`window.__fontSeen`)
// for `fontAgain`. The pointer's place is left out (`pointerAreaHook`). Counts only.
const FONT_TOL = 2;
const fontRuns = async (page) => (await pointerAreaHook(page), page.evaluate((tol) => {
  const c = document.getElementById('screen');
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  const g = globalThis.__shownGame;
  const [w4, h4] = [g.width() * 4, g.height() * 4];
  const x4 = document.createElement('canvas');
  [x4.width, x4.height] = [w4, h4];
  x4.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(g.hd_frame_rgba()), w4, h4), 0, 0);
  const e = document.createElement('canvas');
  [e.width, e.height] = [c.width, c.height];
  const ctx = e.getContext('2d');
  for (const { from, to } of globalThis.__u6Layout.regions) {
    const source = from.map((v) => v * 4);
    ctx.imageSmoothingEnabled = !(to[2] === source[2] && to[3] === source[3]);
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(x4, ...source, ...to);
  }
  const f = ctx.getImageData(0, 0, c.width, c.height).data;
  const shot = window.__pixelShot;
  const list = g.hd_glyphs();
  const rows = g.hd_symbol_rows?.() ?? null;
  const inked = (j) => d[j] !== f[j] || d[j + 1] !== f[j + 1] || d[j + 2] !== f[j + 2] || d[j + 3] !== f[j + 3];
  const near = (j, v) => Math.max(Math.abs(d[j] - ((v >> 16) & 255)), Math.abs(d[j + 1] - ((v >> 8) & 255)), Math.abs(d[j + 2] - (v & 255))) <= 48;
  const out = {};
  const spans = [];
  const symbolBoxes = [];
  const pointer = window.__pointerArea();
  const mc = document.createElement('canvas').getContext('2d');
  for (const { name, from, to } of globalThis.__u6Layout.regions) {
    const [sx, sy] = [to[2] / from[2], to[3] / from[3]];
    const ch = 8 * sy;
    mc.font = `${0.85 * ch}px "U6 Gowun Batang"`;
    mc.fontKerning = 'none';
    const r = {
      off: 0, offCompared: 0, runs: 0, cells: 0, hangul: 0, ascii: 0, inked: 0, fits: 0, whole: 0,
      latin: 0, syllabic: 0, tall: 0, upright: 0, after: 0, afterFits: 0, twoInk: 0, bothInks: 0, placed: 0, unlike: 0,
      carets: 0, caretsPlaced: 0, symbols: 0, symbolsPlaced: 0,
    };
    const mask = new Uint8Array(c.width * c.height);
    // The symbols' boxes (a caret after its run, a symbol alone at its own cell): kept out of the
    // runs' ink and of `off`; each placed where its ink's observed left edge (`__symbolEdge`, read
    // from the canvas from the run's first cell to the symbol's own cell's right edge) is where it
    // belongs, the glyph's own blank columns on, within a canvas pixel (Design 2 rev 3).
    const held = new Uint8Array(c.width * c.height);
    const runs = window.__fontRunsOf(list, from, rows);
    const boxes = [
      ...runs.filter((run) => run.caret).map((run) => {
        const geo = window.__runGeometry(run, from, to);
        const reach = [geo.span[0], to[0] + (run.caret.x + 8 - from[0]) * sx];
        return [geo.caret, run.caret, geo.caretLeft, geo.span[1], reach, 'carets'];
      }),
      ...runs.symbols.map((m) => {
        const box = window.__symbolBox(m, from, to);
        const [left, top] = [to[0] + (m.x - from[0]) * sx, to[1] + (m.y - from[1]) * sy];
        return [box, m, left, top, [left - 8 * sx, left + 8 * sx], 'symbols'];
      }),
    ];
    for (const [[bx, by, bw, bh], m, left, top, reach, kind] of boxes) {
      for (let y = Math.max(by, to[1]); y < Math.min(by + bh, to[1] + to[3]); y += 1) {
        for (let x = Math.max(bx, to[0]); x < Math.min(bx + bw, to[0] + to[2]); x += 1) held[y * c.width + x] = 1;
      }
      const seen = window.__symbolEdge(d, c.width, m, reach, top, sx, sy);
      r[kind] += 1;
      r[`${kind}Placed`] += seen && Math.abs(seen.edge - (left + seen.lead)) <= 1 ? 1 : 0;
    }
    for (const run of runs) {
      const [x0, x1] = [run.cells[0].x, run.cells.at(-1).x + 8];
      const span = [to[0] + (x0 - from[0]) * sx, to[1] + (run.y - from[1]) * sy, (x1 - x0) * sx, ch];
      const [left, top] = span;
      const [ya, yb] = [Math.max(span[1], to[1]), Math.min(span[1] + ch, to[1] + to[3])];
      const [xa, xb] = [Math.max(span[0], to[0]), Math.min(span[0] + span[2], to[0] + to[2])];
      let [minX, minY, maxX, maxY] = [Infinity, Infinity, -Infinity, -Infinity];
      const found = new Map(); // ink -> the first x showing it
      let unlike = false;
      for (let y = ya; y < yb; y += 1) {
        for (let x = xa; x < xb; x += 1) {
          mask[y * c.width + x] = 1;
          if (window.__inPointer(pointer, x, y) || held[y * c.width + x]) continue;
          const j = (y * c.width + x) * 4;
          if (shot && (shot[j] !== d[j] || shot[j + 1] !== d[j + 1] || shot[j + 2] !== d[j + 2])) unlike = true;
          if (!inked(j)) continue;
          [minX, minY, maxX, maxY] = [Math.min(minX, x), Math.min(minY, y), Math.max(maxX, x), Math.max(maxY, y)];
          for (const v of run.inks) if (near(j, v) && !(found.get(v) <= x)) found.set(v, x);
        }
      }
      const letters = run.cells.filter((k) => k.code !== 0x20);
      const syllables = letters.filter((k) => k.code >= 0xac00 && k.code <= 0xd7a3).length;
      const ink = maxX >= minX;
      const text = run.cells.map((k) => String.fromCharCode(k.code)).join('');
      const m = mc.measureText(text);
      const lead = Math.max(0, m.actualBoundingBoxLeft);
      const natural = Math.max(m.width, lead + m.actualBoundingBoxRight);
      const room = span[2] - 2;
      const k = natural > room ? room / natural : 1;
      const pen = left + 1 + lead * k;
      const fits = ink && Math.abs(minX - (pen - m.actualBoundingBoxLeft * k)) <= tol
        && Math.abs(maxX + 1 - (pen + m.actualBoundingBoxRight * k)) <= tol;
      r.runs += 1;
      r.cells += letters.length;
      r.hangul += syllables;
      r.ascii += letters.length - syllables;
      r.inked += ink ? 1 : 0;
      r.fits += fits ? 1 : 0;
      r.whole += span.every(Number.isInteger) ? 1 : 0;
      r.unlike += unlike ? 1 : 0;
      if (syllables === 0 && letters.length >= 4) r.latin += 1;
      if (syllables > 0) {
        r.syllabic += 1;
        r.tall += ink && maxY - minY + 1 >= 0.6 * ch ? 1 : 0;
        r.upright += ink && Math.abs((minY + maxY + 1) / 2 - (top + ch / 2)) <= ch / 6 ? 1 : 0;
      }
      if (run.after) {
        r.after += 1;
        r.afterFits += fits ? 1 : 0;
      }
      if (run.inks.size > 1) {
        r.twoInk += 1;
        r.bothInks += found.size === run.inks.size ? 1 : 0;
        // The second ink's part: from its first cell to the next cell of another ink.
        const a = run.cells.findIndex((q) => q.ink !== null && q.ink !== run.cells[0].ink);
        const second = run.cells[a].ink;
        let b = a + 1;
        while (b < run.cells.length && (run.cells[b].ink === null || run.cells[b].ink === second)) b += 1;
        const at = pen + (mc.measureText(text.slice(0, a)).width - mc.measureText(text.slice(a, b)).actualBoundingBoxLeft) * k;
        r.placed += found.has(second) && Math.abs(found.get(second) - at) <= tol + 1 ? 1 : 0;
      }
      spans.push({ key: `${name},${run.y},${run.cells.map((k) => `${k.x}:${k.code}:${k.ink}`).join(' ')}`, rect: [xa, ya, xb - xa, yb - ya] });
    }
    for (const [box] of boxes) symbolBoxes.push({ name, rect: box });
    const [x, y, w, hh] = to;
    for (let yy = y; yy < y + hh; yy += 1) {
      for (let xx = x; xx < x + w; xx += 1) {
        if (mask[yy * c.width + xx] || held[yy * c.width + xx] || window.__inPointer(pointer, xx, yy)) continue;
        r.offCompared += 1;
        if (inked((yy * c.width + xx) * 4)) r.off += 1;
      }
    }
    out[name] = r;
  }
  window.__fontSeen = { pixels: new Uint8ClampedArray(d), spans, symbolBoxes };
  return out;
}, FONT_TOL));

// Page script: the spans `fontRuns` kept, against the canvas now: how many of those whose run is
// still listed the same (its cells, codes and inks: `same`) show other pixels than then
// (`differ`), outside the symbols' boxes it kept; per region whether any pixel of it outside those
// spans and boxes changed (`moved`), and whether any pixel of those boxes did (`turned`: the key
// wait's cursor, drawn as a caret or at its own cell, turns).
const fontAgain = (page) => page.evaluate(() => {
  const c = document.getElementById('screen');
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  const { pixels, spans, symbolBoxes } = window.__fontSeen;
  const list = globalThis.__shownGame.hd_glyphs();
  const now = new Set();
  for (const { name, from } of globalThis.__u6Layout.regions) {
    for (const run of window.__fontRunsOf(list, from)) now.add(`${name},${run.y},${run.cells.map((k) => `${k.x}:${k.code}:${k.ink}`).join(' ')}`);
  }
  const changed = (j) => d[j] !== pixels[j] || d[j + 1] !== pixels[j + 1] || d[j + 2] !== pixels[j + 2];
  const on = new Uint8Array(c.width * c.height);
  const boxed = new Uint8Array(c.width * c.height);
  const turned = {};
  for (const { name } of globalThis.__u6Layout.regions) turned[name] = false;
  for (const { name, rect: [x, y, w, hh] } of symbolBoxes) {
    for (let yy = Math.max(0, y); yy < Math.min(c.height, y + hh); yy += 1) {
      for (let xx = Math.max(0, x); xx < Math.min(c.width, x + w); xx += 1) {
        boxed[yy * c.width + xx] = 1;
        if (changed((yy * c.width + xx) * 4)) turned[name] = true;
      }
    }
  }
  let same = 0;
  let differ = 0;
  for (const { key, rect: [x, y, w, hh] } of spans) {
    let off = false;
    for (let yy = y; yy < y + hh; yy += 1) {
      for (let xx = x; xx < x + w; xx += 1) {
        on[yy * c.width + xx] = 1;
        if (!boxed[yy * c.width + xx] && changed((yy * c.width + xx) * 4)) off = true;
      }
    }
    if (!now.has(key)) continue;
    same += 1;
    differ += off ? 1 : 0;
  }
  const moved = {};
  for (const { name, to: [x, y, w, hh] } of globalThis.__u6Layout.regions) {
    moved[name] = false;
    for (let yy = y; yy < y + hh && !moved[name]; yy += 1) {
      for (let xx = x; xx < x + w; xx += 1) {
        if (!on[yy * c.width + xx] && !boxed[yy * c.width + xx] && changed((yy * c.width + xx) * 4)) {
          moved[name] = true;
          break;
        }
      }
    }
  }
  return { same, differ, moved, turned };
});

// Page script: the canvas as it is now kept in the page (`window.__pixelShot`, for `fontRuns`).
const fontPixelShot = (page) => page.evaluate(() => {
  const c = document.getElementById('screen');
  window.__pixelShot = new Uint8ClampedArray(c.getContext('2d').getImageData(0, 0, c.width, c.height).data);
});

// Settings' "Font" set to `value` (Settings opened first when it is not); the canvas is drawn
// again at once, with no game step.
async function chooseFont(page, value) {
  if (await page.evaluate(() => document.getElementById('setup').hidden)) await page.click('#settings');
  await page.check(`input[name="font"][value="${value}"]`);
}

// Talks to the party's second member (the test's conversation in place of the user's) and waits
// for the prompt. The greeting's keyword shows in the game's highlight colour only in its help
// mode, which the user's files start on or off: when no run of the scroll has two inks the help
// mode is switched (Ctrl-H) and the member talked to again.
async function fontTalk(page) {
  const talk = async () => {
    await page.focus('#screen');
    for (const key of ['t', '2']) {
      await page.keyboard.press(key);
      await h.waitTaken(page);
    }
    await h.waitPrompt(page);
  };
  const twoInks = () => page.evaluate(() => {
    const L = globalThis.__u6Layout;
    const from = L.regions.find((r) => r.name === 'messages').from;
    return window.__fontRunsOf(globalThis.__shownGame.hd_glyphs(), from).some((run) => run.inks.size > 1);
  });
  await talk();
  if (await twoInks()) return;
  await page.keyboard.press('Control+KeyH');
  await h.waitTaken(page);
  await h.waitPrompt(page);
  await talk();
}

// The pixel font in effect (no cell listed) and the canvas exactly the x4 frame's regions; then
// the HD font in one paint that draws each run once, part by part (the page's fillText calls
// against the test's reading of the runs, in order); every run inked within its span and its
// cell rows, the rest of the canvas exactly the x4 frame's regions, every run unlike the pixel
// font's; `regions`: the regions checked closer, each with the kinds of run it must show
// (`latin`, `syllabic`, `after`, `twoInk`). Returns what `fontRuns` read with the HD font.
async function fontOnOff(page, regions) {
  await chooseFont(page, 'pixel');
  let s = await fontShown(page);
  assert.ok(s.font === 'pixel' && s.mode === false && !s.listed && s.stored === 'pixel', 'the pixel font at once, kept: no cell listed');
  for (const [name, r] of Object.entries(await fontRuns(page))) {
    assert.equal(r.runs, 0, `${name}: no run drawn`);
    assert.ok(r.offCompared > 0 && r.off === 0, `${name}: the pixel font: the canvas is the x4 frame's region exactly`);
  }
  await fontPixelShot(page);
  await page.evaluate(() => { window.__fills = []; });
  await chooseFont(page, 'hd');
  const fills = await page.evaluate(() => {
    const L = globalThis.__u6Layout;
    const list = globalThis.__shownGame.hd_glyphs();
    const want = L.regions.flatMap(({ from }) => window.__fontRunsOf(list, from).flatMap((run) => run.parts));
    const got = window.__fills;
    window.__fills = null;
    return { calls: got.length, want: want.length, same: got.length === want.length && got.every((t, i) => t === want[i]) };
  });
  s = await fontShown(page);
  assert.ok(s.font === 'hd' && s.mode === true && s.listed && s.stored === 'hd', 'the HD font at once, kept: cells listed');
  assert.ok(fills.want > 0 && fills.same, 'one paint draws each run once, part by part, as the runs read');
  const runs = await fontRuns(page);
  for (const [name, kinds] of Object.entries(regions)) {
    const r = runs[name];
    assert.ok(r.runs > 0 && r.cells > 0, `${name}: runs listed`);
    for (const kind of kinds) assert.ok(r[kind] > 0, `${name}: a run of kind ${kind}`);
    assert.equal(r.inked, r.runs, `${name}: every run shows its ink`);
    assert.equal(r.whole, r.runs, `${name}: every span on whole canvas pixels`);
    assert.equal(r.fits, r.runs, `${name}: every run's ink where the font puts it: from 1 px inside its first cell, at the font's natural width without kerning (or narrowed to its cells)`);
    assert.ok(r.tall === r.syllabic && r.upright === r.syllabic, `${name}: every run of syllables at the region's scale, centred down its cells`);
    assert.equal(r.afterFits, r.after, `${name}: every run after two or more spaces at its own first cell`);
    assert.ok(r.bothInks === r.twoInk && r.placed === r.twoInk, `${name}: every run of two inks shows both, the second where the text before it ends`);
    assert.equal(r.unlike, r.runs, `${name}: every run unlike the pixel font's`);
  }
  for (const [name, r] of Object.entries(runs)) {
    assert.ok(r.offCompared > 0, `${name}: pixels off the runs to compare`);
    assert.equal(r.off, 0, `${name}: the runs' ink within their spans and cell rows, the rest the x4 frame's region exactly`);
  }
  return { runs, fills };
}

test('HD font (spec 11 Acceptance 2, English): in game on the HD layout (a 1400 x 900 window, the user\'s disks, a synthetic conversation) the text is in the pixel font until the font file is in, which comes from this origin (200, font/woff2, no CSP report); with the pixel font chosen in Settings no cell is listed and the canvas is the x4 frame\'s regions exactly, as before; with the HD font one paint draws each run of the side panel and the message scroll once, in its inks: its ink within its cells where the font puts it (from 1 px inside its first cell, at the font\'s natural width without kerning, the second ink where the text before it ends), a run after two spaces at its own cell, a keyword in its own colour, the rest of the canvas exactly the x4 frame\'s; frames that redraw the regions draw each run once (its pixels as before); at the game\'s own end (Ctrl-Q, Y) its last frame letterboxed still shows the runs; no page error', async (env) => {
  const errors = [];
  const { context, page } = await h.openPage(env, { layout: 'hd' });
  await context.addInitScript(fontRunsHook);
  const answers = fontAnswers(context);
  page.on('pageerror', () => errors.push(1));
  // The font file's answer held until the game runs: the pixel font until it is in.
  let release;
  const held = new Promise((done) => { release = done; });
  await context.route((url) => url.pathname === FONT_PATH, async (route) => {
    await held;
    await route.continue();
  });
  await page.setViewportSize({ width: 1400, height: 900 });
  // The document's load event waits for the font file: the page is used from its DOM on.
  await page.goto(env.server.url, { waitUntil: 'domcontentloaded' });
  await h.waitSettled(page);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  let s = await fontShown(page);
  assert.ok(s.font === 'pixel' && s.file === 'loading' && s.mode === false && !s.listed, 'the font file not in yet: the pixel font, no cell listed');
  release();
  await fontIs(page, 'hd');
  s = await fontShown(page);
  assert.ok(s.file === 'loaded' && s.mode === true && s.listed && s.stored === null, 'the HD font by default, loaded, cells listed');
  assert.ok(answers.length >= 1 && answers.every(([origin, status, type]) => origin === new URL(env.server.url).origin && status === 200 && type === 'font/woff2'), 'the font file from this origin, 200, font/woff2');
  assert.deepEqual(await page.evaluate(() => window.__csp), [], 'no CSP report');
  // The test's conversation in place of the user's (in English no pack is taken).
  const { archive } = await koFixture(0x63, { greeting: FONT_GREET, greetingKo: FONT_GREET_KO });
  assert.equal(await h.replaceKept(page, 'converse.a', archive), 1, 'one archive replaced');
  await page.reload();
  await h.waitSettled(page);
  await h.waitPrompt(page);
  await fontIs(page, 'hd');
  await fontTalk(page);
  await h.virtualDisplay(page, 60);
  await fontOnOff(page, { messages: ['latin', 'after', 'twoInk'], panel: ['latin'] });
  // Frames at the prompt: the message scroll's cursor turns (listed as a symbol, Design 2 rev 3:
  // drawn as the caret of the prompt's run, its own cell the paper), so the regions are copied
  // again and their runs drawn again: each span as it was outside the caret's box, and nothing of
  // the scroll outside the spans and the box changed.
  const atPrompt = await fontRuns(page);
  assert.ok(atPrompt.messages.carets === 1 && atPrompt.messages.caretsPlaced === 1, 'the cursor drawn as the caret of the prompt\'s run, in its ink, where the prompt\'s text ends');
  let turned = 0;
  for (let k = 0; k < 12; k += 1) {
    await h.runFrames(page, 10);
    const again = await fontAgain(page);
    if (again.turned.messages) turned += 1;
    assert.ok(again.same > 0, 'the runs still listed');
    assert.equal(again.differ, 0, 'each run drawn once over its copied region: its pixels as before');
    assert.ok(!again.moved.messages, 'nothing of the scroll outside its runs and the caret changed');
  }
  assert.ok(turned > 0, 'the message scroll drawn again (its cursor turned, as the caret)');
  s = await fontShown(page);
  assert.equal(s.font, 'hd', 'still the HD font');
  // The game's own end (Ctrl-Q, Y: the command loop ends, and so does the launcher, in `game`):
  // the page shows its last frame letterboxed, the runs listed still drawn there, each span
  // (its cells' rect through the letterbox) showing the run's first ink.
  await page.focus('#screen');
  for (const key of ['Control+KeyQ', 'KeyY']) {
    await page.keyboard.press(key);
    for (let k = 0; k < 40 && await page.evaluate(() => document.getElementById('screen').dataset.keys !== '0'); k += 1) await h.runFrames(page, 5);
    await h.runFrames(page, 30);
  }
  for (let k = 0; k < 40 && await page.evaluate(() => document.body.dataset.state !== 'ended'); k += 1) await h.runFrames(page, 10);
  const ended = await page.evaluate(() => {
    const c = document.getElementById('screen');
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
    const L = globalThis.__u6Layout;
    const [x, y, w, hh] = L.whole;
    const [sx, sy] = [w / 320, hh / 200];
    const runs = window.__fontRunsOf(globalThis.__shownGame.hd_glyphs(), [0, 0, 320, 200]);
    let inked = 0;
    for (const run of runs) {
      const v = run.cells[0].ink;
      const ink = [(v >> 16) & 255, (v >> 8) & 255, v & 255];
      const [px, py] = [x + run.cells[0].x * sx, y + run.y * sy];
      const pw = (run.cells.at(-1).x + 8 - run.cells[0].x) * sx;
      let found = false;
      for (let yy = Math.floor(py); yy < Math.ceil(py + 8 * sy) && !found; yy += 1) {
        for (let xx = Math.floor(px); xx < Math.ceil(px + pw); xx += 1) {
          const j = (yy * c.width + xx) * 4;
          if (Math.max(...[0, 1, 2].map((n) => Math.abs(d[j + n] - ink[n]))) <= 48) {
            found = true;
            break;
          }
        }
      }
      inked += found ? 1 : 0;
    }
    return { state: document.body.dataset.state, view: L.view, runs: runs.length, inked };
  });
  assert.ok(ended.state === 'ended' && ended.view === 'whole', 'the game ended: its last frame letterboxed');
  assert.ok(ended.runs > 0 && ended.inked === ended.runs, 'every run listed still drawn, in its ink');
  assert.equal(errors.length, 0, 'no page error');
  console.log('  receipt: the font file from this origin (200, font/woff2), no CSP report; the pixel font: the canvas the x4 frame\'s regions exactly; the HD font: one paint, each run of the panel and the scroll drawn once in its parts, inked within its cells from its first cell, letters at the font\'s width, a run after two spaces at its column, the keyword in its colour, the rest exactly the x4 frame\'s; 120 frames, the scroll drawn again: each run drawn once; the game\'s own end: its runs drawn in the letterbox: pass');
}, { needs: h.haveHfes });

test('HD font (spec 11, after a trap): a font file that comes in after the engine trapped is shown as loaded on the page (data-font-file), while the canvas, not drawn again, keeps the pixel font (data-font)', async (env) => {
  const { context, page } = await h.openPage(env, { layout: 'hd' });
  let release;
  const held = new Promise((done) => { release = done; });
  await context.route((url) => url.pathname === FONT_PATH, async (route) => {
    await held;
    await route.continue();
  });
  await page.setViewportSize({ width: 1400, height: 900 });
  await page.goto(env.server.url, { waitUntil: 'domcontentloaded' });
  await h.waitSettled(page);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  assert.deepEqual([(await fontShown(page)).font, (await fontShown(page)).file], ['pixel', 'loading'], 'the font file not in yet');
  await page.evaluate(() => { globalThis.__trap = true; });
  await page.waitForFunction(() => document.body.dataset.state === 'crashed');
  const trapped = await canvasHash(page);
  release();
  await page.waitForFunction(() => document.body.dataset.fontFile !== 'loading', null, { timeout: 30_000 });
  const s = await page.evaluate(() => [document.body.dataset.font, document.body.dataset.fontFile]);
  assert.deepEqual(s, ['pixel', 'loaded'], 'the file loaded; the canvas keeps the pixel font');
  assert.equal(await canvasHash(page), trapped, 'the canvas not drawn again');
}, { needs: h.haveHfes });

// Page script: the canvas's pixels in rect [x, y, w, h] that differ from the x4 frame's at the
// same place of a one-to-one region `name` (its canvas rect `to` shows the x4 rect of `from`),
// the pointer's place left out (`pointerAreaHook`).
const oneToOneOff = async (page, name, rect) => (await pointerAreaHook(page), page.evaluate(([n, [x, y, w, hh]]) => {
  const c = document.getElementById('screen');
  const d = c.getContext('2d').getImageData(x, y, w, hh).data;
  const g = globalThis.__shownGame;
  const f = g.hd_frame_rgba();
  const w4 = g.width() * 4;
  const { from, to } = globalThis.__u6Layout.regions.find((r) => r.name === n);
  const pointer = window.__pointerArea();
  let off = 0;
  for (let yy = 0; yy < hh; yy += 1) {
    for (let xx = 0; xx < w; xx += 1) {
      if (window.__inPointer(pointer, x + xx, y + yy)) continue;
      const a = (yy * w + xx) * 4;
      const b = ((4 * from[1] + y - to[1] + yy) * w4 + 4 * from[0] + x - to[0] + xx) * 4;
      if (d[a] !== f[b] || d[a + 1] !== f[b + 1] || d[a + 2] !== f[b + 2]) off += 1;
    }
  }
  return off;
}, [name, rect]));

// Page script: from now on, the rect of the game on screen's x4 frame each of the page's draws
// paints again (the HD layout's `paint` gets it as its dirty rect) is kept in `window.__dirty`,
// one per draw that paints, and that game in `window.__dirtyGame`. A draw reads the pointer first
// (Game.hd_pointer, once) and then takes the rect the compositions wrote (Game.hd_take_dirty);
// the rect it paints is the box round that rect and, when the pointer's version moved (spec 11
// Design 7), the pointer's old and new boxes at x4. So each entry is that union, kept as the page
// keeps it: the rect taken alone, the pointer's boxes alone, or both in one. A test that wrote
// pixels of the x4 frame itself (`writeFramePixels`) sets `window.__widenTake` to the x4 rect it
// wrote: the next take is widened to it (and that rect cleared), as a composition that wrote
// there would give it, so the page copies those pixels to the canvas it paints from.
const recordDirty = (page) => page.evaluate(() => {
  const g = globalThis.__shownGame;
  const real = Object.getPrototypeOf(g);
  window.__dirty = [];
  window.__dirtyGame = g;
  const union = (a, b) => {
    if (!a || !b) return a ?? b;
    const [x0, y0] = [Math.min(a[0], b[0]), Math.min(a[1], b[1])];
    const [x1, y1] = [Math.max(a[0] + a[2], b[0] + b[2]), Math.max(a[1] + a[3], b[1] + b[3])];
    return [x0, y0, x1 - x0, y1 - y0];
  };
  const box = (p) => (p[2] > 0 ? [p[0], p[1], p[2], p[3]].map((v) => 4 * v) : null);
  let last = [...real.hd_pointer.call(g)];
  let moved = null; // the pointer's old and new boxes, read by this draw
  g.hd_pointer = function hdPointer() {
    const p = real.hd_pointer.call(this);
    if (p[4] !== last[4]) {
      moved = union(box(last), box(p));
      last = [...p];
    }
    return p;
  };
  window.__widenTake = null;
  g.hd_take_dirty = function hdTakeDirty() {
    let rect = real.hd_take_dirty.call(this);
    if (window.__widenTake) {
      rect = Uint32Array.from(union(rect.length > 0 ? [...rect] : null, window.__widenTake));
      window.__widenTake = null;
    }
    const painted = union(rect.length > 0 ? [...rect] : null, moved);
    moved = null;
    if (painted) window.__dirty.push(painted);
    return rect;
  };
});

// Page script: the rects `recordDirty` kept (one per draw that painted: the union it painted
// again): how many, whether any is the whole x4 frame, whether any meets the x4 rect `rect`;
// whether the game on screen and the view are still the ones then (so each of those rects
// reached `paint` as its dirty rect: a partial repaint).
const dirtySeen = (page, rect) => page.evaluate(([x, y, w, hh]) => {
  const g = globalThis.__shownGame;
  const [w4, h4] = [g.width() * 4, g.height() * 4];
  const meets = ([a, b, c, d]) => a < x + w && x < a + c && b < y + hh && y < b + d;
  return {
    rects: window.__dirty.length,
    full: window.__dirty.some(([a, b, c, d]) => a === 0 && b === 0 && c === w4 && d === h4),
    meets: window.__dirty.some(meets),
    sameGame: g === window.__dirtyGame,
    view: globalThis.__u6Layout.view,
  };
}, rect);

// Page script: from now on, every `fillRect` on the game's canvas (a whole repaint's background:
// `paint` with no dirty rect) is counted in `window.__wholePaints`, from 0 (the wrapper put in once,
// as `fontRunsHook` puts it; then only the count reset).
const countWholePaints = (page) => page.evaluate(() => {
  if (window.__wholePaints === undefined) {
    const rect = CanvasRenderingContext2D.prototype.fillRect;
    CanvasRenderingContext2D.prototype.fillRect = function fillRect(...args) {
      if (this.canvas.id === 'screen') window.__wholePaints += 1;
      return rect.apply(this, args);
    };
  }
  window.__wholePaints = 0;
});

// Page script: a hash of the game on screen's x4 frame in its rect [x, y, w, h].
const x4Hash = (page, rect) => page.evaluate(([x, y, w, hh]) => {
  const g = globalThis.__shownGame;
  const f = g.hd_frame_rgba();
  const w4 = g.width() * 4;
  let v = 0x811c9dc5;
  for (let yy = y; yy < y + hh; yy += 1) {
    for (let xx = x; xx < x + w; xx += 1) {
      for (let k = 0; k < 4; k += 1) v = Math.imul(v ^ f[(yy * w4 + xx) * 4 + k], 0x01000193) >>> 0;
    }
  }
  return v;
}, rect);

test('HD font (spec 11, a one-to-one region): a run listed in the map window (made up by the test: its own four letters and colours added to the list the page reads) is drawn there once and kept through a repaint the pointer causes elsewhere in the window; once it is no longer listed, the next such repaint copies the window again whole, and nothing of the run stays; preconditions asserted: the game lists no cell in the window, and through those frames nothing else repaints the run\'s place (its x4 pixels unchanged, no dirty rect meets it, each a partial repaint, no whole repaint of the canvas)', async (env) => {
  const { page } = await bootLayout(env, { font: null });
  await fontIs(page, 'hd');
  await page.mouse.move(...(await h.screenPoint(page, 150, 150)));
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 10);
  // The test's precondition: the game lists no cell in the map window (else the window is copied
  // whole for those, and the made-up run below would not be what decides it).
  const mapCells = () => page.evaluate(() => {
    const [fx, fy, fw, fh] = globalThis.__u6Layout.regions.find((r) => r.name === 'map').from;
    const g = globalThis.__shownGame;
    const list = Object.getPrototypeOf(g).hd_glyphs.call(g);
    let n = 0;
    for (let i = 0; i < list.length; i += 5) {
      if (list[i] + 8 > fx && list[i + 1] + 8 > fy && list[i] < fx + fw && list[i + 1] < fy + fh) n += 1;
    }
    return n;
  });
  assert.equal(await mapCells(), 0, 'precondition: the game lists no cell in the map window');
  // The made-up run: 'TEST' in the map window's row 136 from x 16 (its lower left, away from the
  // pointer and from the tiles that move at the start), ink and paper of the test's own.
  const [RUN_X, RUN_Y] = [16, 136];
  await page.evaluate(([rx, ry]) => {
    const g = globalThis.__shownGame;
    const real = Object.getPrototypeOf(g).hd_glyphs;
    g.hd_glyphs = function hdGlyphs() {
      const list = real.call(this);
      return window.__fake ? Uint32Array.from([...list, ...window.__fake]) : list;
    };
    window.__fake = [0x54, 0x45, 0x53, 0x54].flatMap((code, i) => [rx + 8 * i, ry, code, 0xff00ff, 0x00ff00]);
  }, [RUN_X, RUN_Y]);
  const { from, to } = (await layoutTable(page)).regions.find((r) => r.name === 'map');
  const span = [to[0] + (RUN_X - from[0]) * 4, to[1] + (RUN_Y - from[1]) * 4, 32 * 4, 32];
  // A whole paint (the font off and on again) draws it; then Settings closed again (its OK,
  // nothing else chosen). Settings (#setup) is a section in the page's flow above the screen's box
  // (site/index.html: its rules, and the section before #screen-box), so while it is open the
  // canvas sits lower, and `h.screenPoint` maps a game pixel through the canvas's box as it is
  // then: the pointer's moves below could land past the viewport's bottom. Each target is checked
  // inside the viewport.
  await chooseFont(page, 'pixel');
  await chooseFont(page, 'hd');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  const inView = async (gx, gy) => {
    const [px, py] = await h.screenPoint(page, gx, gy);
    const [w, hh] = await page.evaluate(() => [innerWidth, innerHeight]);
    assert.ok(px >= 0 && px < w && py >= 0 && py < hh, `precondition: game pixel (${gx}, ${gy}) inside the viewport`);
    return [px, py];
  };
  assert.ok(await oneToOneOff(page, 'map', span) > 0, 'the made-up run drawn in the map window');
  await page.evaluate(([x, y, w, hh]) => {
    window.__spanShot = [...document.getElementById('screen').getContext('2d').getImageData(x, y, w, hh).data];
  }, span);
  const sameAsShot = () => page.evaluate(([x, y, w, hh]) => {
    const d = document.getElementById('screen').getContext('2d').getImageData(x, y, w, hh).data;
    return d.every((v, i) => v === window.__spanShot[i]);
  }, span);
  // The pointer's version (Game.hd_pointer): the page draws the pointer over the x4 frame and
  // repaints under its old and new boxes when it moves (spec 11 Design 7).
  const version = () => page.evaluate(() => globalThis.__shownGame.hd_pointer()[4]);
  // A repaint the pointer causes far from the run, the run still listed: drawn once again.
  let v = await version();
  await page.mouse.move(...(await inView(150, 100)));
  await h.runFrames(page, 10);
  assert.notEqual(await version(), v, 'the pointer moved: its version moved');
  assert.ok(await sameAsShot(), 'the run still listed: its pixels as before (drawn once)');
  // No longer listed: the next repaint copies the window whole, the run gone. Preconditions for
  // that to rest on the whole copy alone: through those frames the x4 frame under the run's place
  // stays as it was (no animated tile there), no rect the page copies meets it, every one reaches
  // `paint` as a partial repaint (the same game and view, none the whole frame), no paint draws
  // the whole canvas (whatever the rects, `compose` repaints whole for another game or view, a
  // fresh x4 frame), and the game still lists no cell in the window.
  const under = [4 * RUN_X, 4 * RUN_Y, 4 * 32, 4 * 8];
  const before = await x4Hash(page, under);
  await recordDirty(page);
  await countWholePaints(page);
  await page.evaluate(() => { window.__fake = null; });
  v = await version();
  await page.mouse.move(...(await inView(150, 150)));
  await h.runFrames(page, 10);
  assert.notEqual(await version(), v, 'the pointer moved again: its version moved');
  const dirty = await dirtySeen(page, under);
  assert.ok(dirty.rects > 0 && !dirty.full && dirty.sameGame && dirty.view === 'game', 'precondition: the frames after it repainted part of the frame');
  assert.equal(await page.evaluate(() => window.__wholePaints), 0, 'precondition: no whole repaint of the canvas through those frames');
  assert.ok(!dirty.meets, 'precondition: no rect the page copied meets the run\'s place');
  assert.equal(await x4Hash(page, under), before, 'precondition: the x4 frame under the run\'s place unchanged');
  assert.equal(await mapCells(), 0, 'precondition: still no cell of the game in the map window');
  assert.equal(await oneToOneOff(page, 'map', span), 0, 'no longer listed: the run\'s place the x4 frame\'s again');
  console.log('  receipt: a made-up run in the map window drawn, kept once through a pointer repaint, and gone after the next once no longer listed, its place repainted by nothing else (no game cell in the window, its x4 pixels unchanged, no dirty rect meeting it, no whole repaint): pass');
}, { needs: h.haveHfes });

test('HD font (spec 11 Acceptance 2, 한글): in game in 한글 with a Korean pack (the user\'s disks, a synthetic conversation) the message scroll lists the pack\'s syllables and the game\'s letters; with the HD font one paint draws each run once, in its inks, where the font puts it (from 1 px inside its first cell, at the font\'s natural width without kerning, the second ink where the text before it ends): syllables at the scroll\'s scale centred down their cells, a run after two spaces at its own cell, the keyword in its own colour, the rest of the canvas exactly the x4 frame\'s; a change of the scroll\'s text alone (Look at the prompt, the pointer in the map; precondition asserted: no rect the page copied met a source of the paper\'s band, nor did the x4 frame there change), a partial repaint (its dirty rects meet the scroll, none the whole frame), drawn over the scroll copied again, nothing of the runs before left; with the pixel font the canvas is the x4 frame\'s regions exactly; no line reaches the console, the page\'s data or text', async (env) => {
  const { context, page } = await h.openPage(env, { setup: { lang: 'ko' }, layout: 'hd' });
  await context.addInitScript(fontRunsHook);
  const words = consoleText(page);
  const answers = fontAnswers(context);
  await page.setViewportSize({ width: 1400, height: 900 });
  await h.load(page, env);
  const { archive, pack } = await koFixture(0x63, { greeting: FONT_GREET, greetingKo: FONT_GREET_KO });
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.dropFiles(page, [], [{ name: 'ko.u6lp', bytes: pack }]);
  await h.waitSettled(page);
  await h.waitPrompt(page);
  assert.equal(await h.replaceKept(page, 'converse.a', archive), 1, 'one archive replaced');
  await page.reload();
  await h.waitSettled(page);
  await h.waitPrompt(page);
  assert.equal((await localeShown(page)).locale, 'on', 'the pack on the game');
  await fontIs(page, 'hd');
  await fontTalk(page);
  const shown = await localeShown(page);
  assert.ok(shown.hits >= 2 && shown.misses === 0, 'the conversation from the pack');
  assert.ok(answers.length >= 1 && answers.every(([, status]) => status === 200), 'the font file, 200');
  await h.virtualDisplay(page, 60);
  // The pack's syllables (the test's own lines) are among the scroll's listed cells.
  const wanted = [...new Set([...(KO_LOOK_KO + FONT_GREET_KO)].filter((ch) => /[가-힣]/.test(ch)).map((ch) => ch.codePointAt(0)))];
  const listed = await page.evaluate((codes) => {
    const g = globalThis.__shownGame.hd_glyphs();
    const seen = new Set();
    for (let i = 0; i < g.length; i += 5) if (g[i] >= 176 && g[i + 1] >= 112) seen.add(g[i + 2]);
    return codes.filter((c) => seen.has(c)).length;
  }, wanted);
  assert.equal(listed, wanted.length, 'every syllable of the pack\'s lines listed in the message scroll');
  await fontOnOff(page, { messages: ['syllabic', 'latin', 'after', 'twoInk'], panel: ['latin'] });
  // A change of the scroll's text on its own (L at the prompt: Look, its name printed, waiting
  // for a target, no turn): a repaint of the regions the change touched, the scroll's runs not as
  // before; the scroll copied again shows its new runs where the font puts them and nothing of
  // the old ones. The pointer first into the map, settled: until the first move the game keeps its
  // sprite over the paper's band where it starts it, and Look changes it there, which the page
  // paints again under the pointer's box (spec 11 Design 7), a rect over the band's sources that
  // rightly paints the canvas whole (spec 10 Design 2). Precondition: through
  // the Look no rect the page copied meets a source of the band (its pieces' `from`, the x4
  // frame's rects that paint the paper whole) and the x4 frame under them stays as it was, so a
  // whole repaint could only be the page's own.
  await page.mouse.move(...(await h.screenPoint(page, 88, 88)));
  await h.runFrames(page, 10);
  await page.evaluate(() => {
    const from = globalThis.__u6Layout.regions.find((r) => r.name === 'messages').from;
    window.__keysBefore = window.__fontRunsOf(globalThis.__shownGame.hd_glyphs(), from).map((run) => `${run.y}:${run.cells.map((k) => `${k.x}:${k.code}`).join(' ')}`).join('|');
  });
  const bandSources = await page.evaluate(() => globalThis.__u6Layout.paper.pieces.map(({ from }) => from.map((v) => 4 * v)));
  const bandHashes = async () => {
    const v = [];
    for (const r of bandSources) v.push(await x4Hash(page, r));
    return v;
  };
  const bandBefore = await bandHashes();
  await recordDirty(page);
  await page.evaluate(() => { window.__wholePaints = 0; });
  await page.focus('#screen');
  await page.keyboard.press('KeyL');
  const scrollChanged = () => page.evaluate(() => {
    const from = globalThis.__u6Layout.regions.find((r) => r.name === 'messages').from;
    return window.__fontRunsOf(globalThis.__shownGame.hd_glyphs(), from).map((run) => `${run.y}:${run.cells.map((k) => `${k.x}:${k.code}`).join(' ')}`).join('|') !== window.__keysBefore;
  });
  for (let k = 0; k < 40 && !(await scrollChanged()); k += 1) await h.runFrames(page, 5);
  await h.runFrames(page, 10);
  assert.ok(await scrollChanged(), 'the scroll\'s runs changed');
  // The repaint was partial: each rect the page copied since reached `paint` as its dirty rect
  // (the same game and view), none the whole frame, some met the scroll, and no paint drew the
  // whole canvas (its background, `fillRect`).
  const scroll = (await layoutTable(page)).regions.find((r) => r.name === 'messages').from.map((n) => 4 * n);
  const dirty = await dirtySeen(page, scroll);
  const bandMet = [];
  for (const r of bandSources) bandMet.push((await dirtySeen(page, r)).meets);
  assert.ok(bandSources.length > 0 && !bandMet.some(Boolean), 'precondition: through the Look no rect the page copied met a source of the paper\'s band');
  assert.deepEqual(await bandHashes(), bandBefore, 'precondition: through the Look the x4 frame under the band\'s sources unchanged');
  assert.ok(dirty.rects > 0 && dirty.meets && !dirty.full && dirty.sameGame && dirty.view === 'game', 'the repaint partial: the dirty rects met the scroll, none the whole frame');
  assert.equal(await page.evaluate(() => window.__wholePaints), 0, 'the repaint partial: no whole repaint of the canvas');
  const partial = (await fontRuns(page)).messages;
  assert.ok(partial.runs > 0 && partial.inked === partial.runs && partial.fits === partial.runs, 'after the partial repaint every run of the scroll where the font puts it');
  assert.ok(partial.offCompared > 0 && partial.off === 0, 'and nothing of the runs before: the rest of the scroll the x4 frame\'s exactly');
  await noPackWords(page, words);
  const where = await pageWords(page);
  assert.ok(!where.includes('나그네여') && !words.some((m) => m.includes('나그네여')), 'no line of the test\'s conversation in the page\'s data, text or console');
  console.log(`  receipt: in 한글 the ${wanted.length} syllables of the test's lines listed in the message scroll; one paint, each run drawn once in its parts, syllables at the scroll's scale from their first cell, a run after two spaces at its column, the keyword in its colour; a change of the scroll alone drawn over it copied again, nothing of the runs before left; the pixel font: the canvas the x4 frame's regions exactly: pass`);
}, { needs: h.haveHfes });

// --- the pointer over the HD text (spec 11 Design 7) --------------------------------------------
// The HD frame is composed without the game's pointer sprite and the page draws the pointer over
// it, last (Game.hd_pointer, Game.hd_pointer_rgba). The pointer's box, its pixels and the places
// they land are the user's (the sprite's shape is read from the game): they stay in the page, and
// only booleans and the tests' own counts compared leave it.

// Page script: the game on screen frozen (`on`: its steps do nothing, so its frames stay as they
// are while the page goes on drawing them) or running again.
const freezeGame = (page, on) => page.evaluate((frozen) => {
  const g = globalThis.__shownGame;
  if (frozen) g.step = () => {};
  else delete g.step;
}, on);

// Page script: the canvas kept in the page as `window.__shots[name]`.
const shotCanvas = (page, name) => page.evaluate((n) => {
  const c = document.getElementById('screen');
  window.__shots ??= {};
  window.__shots[n] = new Uint8ClampedArray(c.getContext('2d').getImageData(0, 0, c.width, c.height).data);
}, name);

// Page script: the pointer as the game gives it now (Game.hd_pointer, Game.hd_pointer_rgba) kept
// in the page as `window.__pointer`, with `area`, the canvas rects it can reach through the
// layout as it shows now (`pointerAreaHook`). Whether it has a box, and whether the area is on
// the canvas.
const keepPointer = async (page) => (await pointerAreaHook(page), page.evaluate(() => {
  const g = globalThis.__shownGame;
  const [x, y, w, hh, version] = [...g.hd_pointer()];
  const area = window.__pointerArea();
  window.__pointer = { box: [x, y, w, hh], version, rgba: [...g.hd_pointer_rgba()], area };
  return { box: w > 0, area: area.length > 0 };
}));

// Page script: of the pixels of the pointer kept (`keepPointer`) that are not clear, those whose
// game pixel the layout shows (`__u6Layout.toCanvas`: where the region in front shows it), and of
// those how many the canvas shows in the pointer pixel's own colour at the centre of that place.
const pointerColours = (page) => page.evaluate(() => {
  const c = document.getElementById('screen');
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  const L = globalThis.__u6Layout;
  const { box: [x, y, w], rgba } = window.__pointer;
  let [seen, same] = [0, 0];
  for (let i = 0; i < rgba.length / 4; i += 1) {
    if (rgba[4 * i + 3] === 0) continue;
    const at = L.toCanvas(x + (i % w), y + Math.floor(i / w));
    if (!at) continue;
    seen += 1;
    const j = (Math.floor(at[1]) * c.width + Math.floor(at[0])) * 4;
    if ([0, 1, 2].every((k) => d[j + k] === rgba[4 * i + k])) same += 1;
  }
  return { seen, same };
});

// Page script: of the canvas shots `a` and `b` (`shotCanvas`), the pixels compared and those that
// differ, inside the area of the pointer kept (`keepPointer`), or with `inside` false outside it.
const shotsDiffer = (page, a, b, inside) => page.evaluate(([na, nb, within]) => {
  const c = document.getElementById('screen');
  const [p, q] = [window.__shots[na], window.__shots[nb]];
  const mask = new Uint8Array(c.width * c.height);
  for (const [x, y, w, hh] of window.__pointer.area) {
    for (let yy = y; yy < y + hh; yy += 1) mask.fill(1, yy * c.width + x, yy * c.width + x + w);
  }
  let [compared, differ] = [0, 0];
  for (let i = 0; i < mask.length; i += 1) {
    if ((mask[i] === 1) !== within) continue;
    compared += 1;
    const j = 4 * i;
    if (p[j] !== q[j] || p[j + 1] !== q[j + 1] || p[j + 2] !== q[j + 2] || p[j + 3] !== q[j + 3]) differ += 1;
  }
  return { compared, differ };
}, [a, b, inside]);

// The canvas painted whole with no game step (the font off and on again, a whole composition
// each), then Settings closed with its OK (nothing else chosen).
async function wholeRepaint(page) {
  await chooseFont(page, 'pixel');
  await chooseFont(page, 'hd');
  await fontIs(page, 'hd');
  await page.click('#setup-ok');
  await h.waitSettled(page);
}

// Spec 11 Acceptance 5, the page (`what`: the layout's label), on a game at the prompt after the
// test's own conversation (`fontTalk`), with the HD font, on the virtual display. The test's own
// line in the message scroll (its word AVAW, FONT_GREET), the pointer brought to rest over it:
// every cell of the line still listed (the cell under the pointer too: spec 11 Design 7, the HD
// frame without the sprite), and the canvas shows the pointer's own colours where its pixels
// land. The game frozen (the same frame throughout): the same frame painted again with the
// pointer away (the page's read of Game.hd_pointer made empty) and then whole is the canvas
// with the pointer outside its box, and inside it the pointer's place repainted when it went is
// that whole repaint's; with the pointer back, the canvas as before. Then the pointer moved off
// into the map: its old box (and the rest) as a whole repaint of the same frame.
async function pointerOverText(page, what) {
  const target = await page.evaluate(() => {
    const from = globalThis.__u6Layout.regions.find((r) => r.name === 'messages').from;
    const runs = window.__fontRunsOf(globalThis.__shownGame.hd_glyphs(), from);
    const run = runs.find((r) => r.cells.map((k) => String.fromCharCode(k.code)).join('').includes('AVAW'));
    if (!run) return null;
    window.__run = { y: run.y, xs: run.cells.map((k) => k.x), key: run.cells.map((k) => `${k.x}:${k.code}`).join(' ') };
    const at = run.cells.find((k) => k.code === 0x56);
    return [at.x + 4, run.y + 4];
  });
  assert.ok(target, `${what}: precondition: the test's own line listed in the message scroll`);
  const runListed = () => page.evaluate(() => {
    const from = globalThis.__u6Layout.regions.find((r) => r.name === 'messages').from;
    return window.__fontRunsOf(globalThis.__shownGame.hd_glyphs(), from)
      .some((r) => r.y === window.__run.y && r.cells.map((k) => `${k.x}:${k.code}`).join(' ') === window.__run.key);
  });
  await page.mouse.move(...(await h.screenPoint(page, ...target)));
  await h.runFrames(page, 10);
  await freezeGame(page, true);
  await h.runFrames(page, 2);
  const kept = await keepPointer(page);
  assert.ok(kept.box && kept.area, `${what}: the pointer at rest in the message scroll, on the canvas`);
  const over = await page.evaluate(() => {
    const [x, y, w, hh] = window.__pointer.box;
    const { y: ry, xs } = window.__run;
    return xs.some((cx) => x < cx + 8 && cx < x + w && y < ry + 8 && ry < y + hh);
  });
  assert.ok(over, `${what}: precondition: the pointer's box over the line`);
  assert.ok(await runListed(), `${what}: every cell of the line listed with the pointer over it, as with it away`);
  const colours = await pointerColours(page);
  assert.ok(colours.seen > 0 && colours.same === colours.seen, `${what}: the canvas shows the pointer's own colours where its pixels land`);
  await shotCanvas(page, 'over');
  // The same frame with the pointer away: the page's read of the pointer made empty (its version
  // moved), the pointer's place painted again; then the canvas painted whole.
  await page.evaluate(() => {
    const v = window.__pointer.version;
    globalThis.__shownGame.hd_pointer = () => Uint32Array.from([0, 0, 0, 0, (v + 1) >>> 0]);
  });
  await h.runFrames(page, 2);
  await shotCanvas(page, 'gone');
  await wholeRepaint(page);
  await shotCanvas(page, 'away');
  let d = await shotsDiffer(page, 'over', 'away', false);
  assert.ok(d.compared > 0 && d.differ === 0, `${what}: outside the pointer's box the canvas is the same frame painted with the pointer away`);
  d = await shotsDiffer(page, 'over', 'away', true);
  assert.ok(d.compared > 0 && d.differ > 0, `${what}: inside it, the pointer`);
  d = await shotsDiffer(page, 'gone', 'away', true);
  assert.equal(d.differ, 0, `${what}: the pointer gone: its box as the whole repaint`);
  await page.evaluate(() => { delete globalThis.__shownGame.hd_pointer; });
  await h.runFrames(page, 2);
  await shotCanvas(page, 'back');
  for (const inside of [true, false]) {
    d = await shotsDiffer(page, 'over', 'back', inside);
    assert.equal(d.differ, 0, `${what}: the pointer back: the canvas as before`);
  }
  // Moved off, into the map (its centre, away from the scroll), at rest: its old box, and the
  // rest of the canvas, as the same frame painted whole.
  await freezeGame(page, false);
  const centre = await page.evaluate(() => {
    const [x, y, w, hh] = globalThis.__u6Layout.regions.find((r) => r.name === 'map').from;
    return [x + Math.floor(w / 2), y + Math.floor(hh / 2)];
  });
  await page.mouse.move(...(await h.screenPoint(page, ...centre)));
  await h.runFrames(page, 10);
  await freezeGame(page, true);
  await h.runFrames(page, 2);
  const off = await page.evaluate(() => {
    const [x, y, w, hh, v] = [...globalThis.__shownGame.hd_pointer()];
    const [ox, oy, ow, oh] = window.__pointer.box;
    return { moved: v !== window.__pointer.version && w > 0, apart: x >= ox + ow || ox >= x + w || y >= oy + oh || oy >= y + hh };
  });
  assert.ok(off.moved && off.apart, `${what}: precondition: the pointer moved off, its new box apart from the old`);
  await shotCanvas(page, 'moved');
  await wholeRepaint(page);
  await shotCanvas(page, 'whole');
  for (const [inside, where] of [[true, 'its old box'], [false, 'the rest']]) {
    d = await shotsDiffer(page, 'moved', 'whole', inside);
    assert.ok(d.compared > 0 && d.differ === 0, `${what}: after the pointer moved off, ${where} as a whole repaint`);
  }
  await freezeGame(page, false);
}

// The page with the test's own conversation (koFixture with FONT_GREET in place of the user's),
// at the prompt after it, with the HD font, on the virtual display.
async function talkedWithFont(context, page, env) {
  await context.addInitScript(fontRunsHook);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  await fontIs(page, 'hd');
  const { archive } = await koFixture(0x63, { greeting: FONT_GREET, greetingKo: FONT_GREET_KO });
  assert.equal(await h.replaceKept(page, 'converse.a', archive), 1, 'one archive replaced');
  await page.reload();
  await h.waitSettled(page);
  await h.waitPrompt(page);
  await fontIs(page, 'hd');
  await fontTalk(page);
  await h.virtualDisplay(page, 60);
}

test('HD font (spec 11 Acceptance 5, the HD layout): the pointer at rest over the test\'s own line of the message scroll (the user\'s disks, a synthetic conversation, the HD font): every cell of the line listed, the canvas showing the pointer\'s own colours where its pixels land; the game frozen, outside its box the canvas is the same frame painted with the pointer away, its box when it went as a whole repaint, the canvas as before with it back; moved off into the map, its old box and the rest as a whole repaint of the same frame; no page error', async (env) => {
  const errors = [];
  const { context, page } = await h.openPage(env, { layout: 'hd' });
  page.on('pageerror', () => errors.push(1));
  await page.setViewportSize({ width: 1400, height: 900 });
  await talkedWithFont(context, page, env);
  assert.equal(await page.evaluate(() => globalThis.__u6Layout.view), 'game', 'the game\'s regions');
  await pointerOverText(page, 'HD layout');
  assert.equal(errors.length, 0, 'no page error');
  console.log('  receipt: the HD layout: the pointer over the test\'s line, every cell listed, its colours where its pixels land; outside its box the frame as painted with it away; its old box as a whole repaint after it went and after it moved off: pass');
}, { needs: h.haveHfes });

test('Wide layout (spec 11 Acceptance 5): the pointer at rest over the test\'s own line of the message scroll\'s window (the user\'s disks, a synthetic conversation, the HD font): every cell of the line listed, the canvas showing the pointer\'s own colours where its pixels land; the game frozen, outside its box the canvas is the same frame painted with the pointer away, its box when it went as a whole repaint, the canvas as before with it back; moved off into the map, its old box and the rest as a whole repaint of the same frame; no page error', async (env) => {
  const errors = [];
  const { context, page } = await h.openWide(env, { width: 1400, height: 900 });
  page.on('pageerror', () => errors.push(1));
  await talkedWithFont(context, page, env);
  assert.equal(await page.evaluate(() => globalThis.__u6Layout.view), 'game', 'the map and the windows');
  await pointerOverText(page, 'Wide');
  assert.equal(errors.length, 0, 'no page error');
  console.log('  receipt: Wide: the pointer over the test\'s line in the message scroll\'s window, every cell listed, its colours where its pixels land; outside its box the frame as painted with it away; its old box as a whole repaint after it went and after it moved off: pass');
}, { needs: h.haveHfes });

// --- the caret and the typed spaces (spec 11 Design 2 and 3, rev 3) ------------------------------
// The game's reply prompt in koTypingFixture's conversation (its archive alone, in English: the
// canvas takes the keys), with the HD font: the key wait's cursor after the prompt (the game's own
// text, never compared outside the page) and what the player types there (a made-up word). The
// checks are geometry in canvas pixels by the test's own reading of the runs (`__fontRunsOf`,
// `__runGeometry`); only booleans and the test's counts leave the page.

const CARET_WORD = 'qux';

// Page script: the composition of the canvas rect `to` from the classic rect `from` of the x4
// frame as it is (smooth unless one to one, as the page copies it), as RGBA of the whole canvas.
function composedHook() {
  window.__composed = (pairs) => {
    const c = document.getElementById('screen');
    const g = globalThis.__shownGame;
    const [w4, h4] = [g.width() * 4, g.height() * 4];
    const x4 = document.createElement('canvas');
    [x4.width, x4.height] = [w4, h4];
    x4.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(g.hd_frame_rgba()), w4, h4), 0, 0);
    const e = document.createElement('canvas');
    [e.width, e.height] = [c.width, c.height];
    const ctx = e.getContext('2d');
    for (const { from, to } of pairs) {
      const source = from.map((v) => v * 4);
      ctx.imageSmoothingEnabled = !(to[2] === source[2] && to[3] === source[3]);
      ctx.imageSmoothingQuality = 'high';
      ctx.drawImage(x4, ...source, ...to);
    }
    return ctx.getImageData(0, 0, c.width, c.height).data;
  };
}

// Page script: where the reply prompt is (`where`: the region of that name, or 'whole', the
// letterbox), kept in the page as `window.__reply`: its row, the row of the listed symbol of the
// key wait's cursor (Design 2 rev 3), or with none listed (before rev 3) the last row listed in
// the rect; and its last letter, the rightmost cell of the rightmost run on that row (the prompt:
// nothing is typed yet). Whether the cursor is listed there.
const replyMark = (page, where) => page.evaluate((w) => {
  const L = globalThis.__u6Layout;
  const { from } = w === 'whole' ? { from: [0, 0, 320, 200] } : L.regions.find((r) => r.name === w);
  const list = globalThis.__shownGame.hd_glyphs();
  const runs = window.__fontRunsOf(list, from);
  const withCaret = runs.find((r) => r.caret);
  const row = withCaret ? withCaret.y : Math.max(...runs.map((r) => r.y));
  const prompt = runs.filter((r) => r.y === row).at(-1);
  if (!prompt) return { found: false };
  window.__reply = { row, last: prompt.cells.at(-1).x };
  return { found: true, listed: Boolean(withCaret) };
}, where);

// Page script: the reply prompt's row (`replyMark`) as the canvas shows it now, in the rect
// `where` names (`row`: the row it stands in now, by default where it was marked). Of the run
// that holds the prompt's last letter: whether the cursor's symbol is listed as its caret
// (`caret`) and the spaces between (`spaces`); whether the made-up word's letters follow that
// letter after two spaces in the same run (`joined`), those two listed as typed (`typed`); a
// space's advance as drawn, measured here (`space`). What the canvas shows, read from its pixels:
// `edge`, the caret's ink's observed left edge (`__symbolEdge`, searched from the run's first cell
// to the cursor's own cell's right edge; null when the cursor is not listed or not found), and
// `want`, where it belongs (the pen's end after the run's text and the spaces between, the
// glyph's own blank columns on: `lead`); `gap`, from the prompt's ink end (the test's measure,
// held to the canvas by `fontRuns`' `fits`) to that edge (before rev 3, to the first pixel right
// of the prompt that is ink: a pixel the page drew, or near the run's ink in the x4 frame);
// `wordInk`, the made-up word's first observed ink (the first column right of the prompt's ink
// end and FONT_TOL with a pixel the page drew or near the ink), and `wordWant`, where its first
// letter's ink belongs (the pen's end after the prompt and the two spaces, less that letter's
// own left side bearing), and `promptEnd`, the pen's end after the prompt; whether the cursor's own cell is the paper alone (`ownPaper`: no pixel
// near its ink in the x4 frame there, and the canvas there outside the caret's box the
// composition's).
const replySeen = async (page, where, row = null) => (await pointerAreaHook(page), page.evaluate(([w, wanted, word, tol]) => {
  const L = globalThis.__u6Layout;
  const g = globalThis.__shownGame;
  const c = document.getElementById('screen');
  const pair = w === 'whole' ? { from: [0, 0, 320, 200], to: L.whole } : L.regions.find((r) => r.name === w);
  const { from, to } = pair;
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  const f = window.__composed([pair]);
  const pointer = window.__pointerArea();
  const y = wanted ?? window.__reply.row;
  const last = window.__reply.last;
  const list = g.hd_glyphs();
  const runs = window.__fontRunsOf(list, from, g.hd_symbol_rows?.() ?? null);
  const run = runs.find((r) => r.y === y && r.cells.some((k) => k.x === last && k.code !== 0x20));
  if (!run) return { found: false };
  const geo = window.__runGeometry(run, from, to);
  const at = run.cells.findIndex((k) => k.x === last);
  const mc = window.__measure;
  const inkEnd = geo.pen + mc.measureText(geo.text.slice(0, at + 1)).actualBoundingBoxRight * geo.k;
  const promptEnd = geo.pen + mc.measureText(geo.text.slice(0, at + 1)).width * geo.k;
  const ink = run.cells[at].ink;
  const near = (j, v) => Math.max(Math.abs(d[j] - ((v >> 16) & 255)), Math.abs(d[j + 1] - ((v >> 8) & 255)), Math.abs(d[j + 2] - (v & 255))) <= 48;
  const drawn = (j) => d[j] !== f[j] || d[j + 1] !== f[j + 1] || d[j + 2] !== f[j + 2];
  const [sx, sy] = [to[2] / from[2], to[3] / from[3]];
  const top = Math.round(to[1] + (y - from[1]) * sy);
  const bottom = Math.round(to[1] + (y + 8 - from[1]) * sy);
  let first = Infinity;
  for (let x = Math.ceil(inkEnd) + tol; x < to[0] + to[2] && first === Infinity; x += 1) {
    for (let yy = top; yy < bottom; yy += 1) {
      const j = (yy * c.width + x) * 4;
      if (window.__inPointer(pointer, x, yy)) continue;
      if (drawn(j) || near(j, ink)) {
        first = x;
        break;
      }
    }
  }
  const typedAt = (x) => {
    for (let i = 0; i < list.length; i += 5) if (list[i] === x && list[i + 1] === y) return list[i + 2] === 0x20020;
    return false;
  };
  const letter = run.cells.find((k) => k.x === last + 24);
  const joined = Boolean(letter) && letter.code === word.charCodeAt(0);
  const wordWant = geo.pen + (mc.measureText(geo.text.slice(0, at + 3)).width - mc.measureText(word[0]).actualBoundingBoxLeft) * geo.k;
  // The cursor's own cell: the listed symbol's on the row, else (before rev 3) the cell after the
  // prompt.
  let own = null;
  for (let i = 0; i < list.length; i += 5) {
    if (list[i + 1] === y && list[i + 2] >= 0x10000 && list[i + 2] !== 0x20020) own = { x: list[i], y };
  }
  const cell = window.__symbolBox(own ?? { x: last + 8, y }, from, to);
  const seen = run.caret ? window.__symbolEdge(d, c.width, run.caret, [geo.span[0], cell[0] + cell[2]], geo.span[1], sx, sy) : null;
  const inBox = (b, x, yy) => b && x >= b[0] && x < b[0] + b[2] && yy >= b[1] && yy < b[1] + b[3];
  const box = geo.caret;
  const x4 = g.hd_frame_rgba();
  const w4 = g.width() * 4;
  let ownPaper = own !== null;
  for (let v = 0; v < 32 && ownPaper; v += 1) {
    for (let u = 0; u < 32; u += 1) {
      const j = ((own.y * 4 + v) * w4 + own.x * 4 + u) * 4;
      const sym = run.caret ? run.caret.ink : ink;
      if (Math.max(Math.abs(x4[j] - ((sym >> 16) & 255)), Math.abs(x4[j + 1] - ((sym >> 8) & 255)), Math.abs(x4[j + 2] - (sym & 255))) <= 48) {
        ownPaper = false;
        break;
      }
    }
  }
  for (let yy = cell[1]; yy < cell[1] + cell[3] && ownPaper; yy += 1) {
    for (let x = cell[0]; x < cell[0] + cell[2]; x += 1) {
      if (inBox(box, x, yy) || window.__inPointer(pointer, x, yy)) continue;
      if (drawn((yy * c.width + x) * 4)) {
        ownPaper = false;
        break;
      }
    }
  }
  const lead = seen ? seen.lead : 0;
  return {
    found: true,
    caret: Boolean(run.caret),
    spaces: run.caret ? run.caret.spaces : null,
    joined,
    typed: typedAt(last + 8) && typedAt(last + 16),
    space: geo.space,
    edge: seen ? seen.edge : null,
    want: run.caret ? geo.caretLeft + lead : null,
    lead,
    gap: (seen ? seen.edge : first) - inkEnd,
    wordInk: first,
    wordWant,
    promptEnd,
    ownPaper,
    box,
  };
}, [where, row, CARET_WORD, FONT_TOL]));

// Page script: the runs of the region `where` that hold the made-up word typed after the
// prompt's last letter (`replyMark`: that letter three cells left of the word's first, in the same
// run, the two cells between listed as typed): how many, and the highest row among them.
const typedLines = (page, where) => page.evaluate(([w, word]) => {
  const { from } = globalThis.__u6Layout.regions.find((r) => r.name === w);
  const list = globalThis.__shownGame.hd_glyphs();
  const typed = new Set();
  for (let i = 0; i < list.length; i += 5) if (list[i + 2] === 0x20020) typed.add(`${list[i]},${list[i + 1]}`);
  let n = 0;
  let top = Infinity;
  for (const run of window.__fontRunsOf(list, from)) {
    const codes = run.cells.map((k) => k.code);
    for (let a = 0; a + word.length <= codes.length; a += 1) {
      if (![...word].every((ch, i) => codes[a + i] === ch.charCodeAt(0))) continue;
      const x = run.cells[a].x;
      const before = run.cells.find((k) => k.x === x - 24);
      if (before && before.code !== 0x20 && typed.has(`${x - 16},${run.y}`) && typed.has(`${x - 8},${run.y}`)) {
        n += 1;
        top = Math.min(top, run.y);
      }
    }
  }
  return { n, top };
}, [where, CARET_WORD]);

// The frame shown letterboxed, as on every other screen (the game frozen, and its program read
// as another by the page: the same frame through `paint`'s whole view), while `fn` runs; then the
// game's regions and its steps again.
async function letterboxed(page, fn) {
  await freezeGame(page, true);
  await page.evaluate(() => { globalThis.__shownGame.program = () => 'u'; });
  await h.runFrames(page, 2);
  const view = await page.evaluate(() => globalThis.__u6Layout.view);
  const out = await fn();
  await page.evaluate(() => { delete globalThis.__shownGame.program; });
  await freezeGame(page, false);
  await h.runFrames(page, 2);
  return { view, out };
}

// On the virtual display: frames run until `done` (a page function) holds, at most `max` runs
// of five; whether it held.
async function framesUntil(page, done, max = 80) {
  for (let k = 0; k < max; k += 1) {
    if (await page.evaluate(done)) return true;
    await h.runFrames(page, 5);
  }
  return page.evaluate(done);
}

// On the virtual display: presses `keys` on the canvas one by one, each taken by the game (the
// key queue empty), then frames for the page.
async function typeKeys(page, keys) {
  await page.focus('#screen');
  for (const key of keys) {
    await page.keyboard.press(key);
    await h.runFrames(page, 2);
    assert.ok(await framesUntil(page, () => document.getElementById('screen').dataset.keys === '0'), `precondition: the game took ${key}`);
  }
  await h.runFrames(page, 4);
}

// With `U6_SHOTS` set (a directory), the canvas rect of region `name` saved there as
// `<label>.png`, for a report's before and after (never in the gate).
async function keepShot(page, name, label) {
  const dir = process.env.U6_SHOTS;
  if (!dir) return;
  const url = await page.evaluate((n) => {
    const c = document.getElementById('screen');
    const { to } = globalThis.__u6Layout.regions.find((r) => r.name === n);
    const e = document.createElement('canvas');
    [e.width, e.height] = [to[2], to[3]];
    e.getContext('2d').drawImage(c, ...to, 0, 0, to[2], to[3]);
    return e.toDataURL('image/png');
  }, name);
  await writeFile(join(dir, `${label}.png`), Buffer.from(url.split(',')[1], 'base64'));
}

// Spec 11 rev 3 (Design 2: the caret; Design 3: typed spaces) on a page in `what`'s layout at the
// test's conversation, talked to up to the player's words (the line read), with the HD font, the
// pointer in the map. Every check is kept and the test fails naming each one not met (a build
// before rev 3 fails several of them, each named).
// (i) Nothing typed: the cursor listed, the prompt's caret: its ink from within one space
// advance (and the spaces between, each one more) of the prompt's ink end, its own cell the paper
// alone, its box showing its ink.
// (ii) Two spaces and the made-up word typed: one run with the prompt, the two spaces listed as
// typed, the word's ink at most two space advances after the prompt's last letter.
// With `letterbox`, (i) and (ii) also on the same frames letterboxed.
// Then: two spaces typed after the word, the caret after them; Backspace over one, the caret one
// space nearer (each: the caret's box where the test measures it, its ink in it, the scroll's
// other pixels the composition's: `fontRuns`); letters typed to the window's end, the echo
// wrapping: the cursor at the next row's first cell, drawn there alone, and the typed line still
// one run; (iii) Return: the conversation answers and ends, the scroll moves the typed line up,
// and it keeps its joined form.
async function caretFlow(page, what, letterbox) {
  const misses = [];
  const check = (ok, label) => {
    if (!ok) misses.push(`${what}: ${label}`);
  };
  const map = await page.evaluate(() => {
    const [x, y, w, hh] = globalThis.__u6Layout.regions.find((r) => r.name === 'map').from;
    return [x + Math.floor(w / 2), y + Math.floor(hh / 2)];
  });
  await page.mouse.move(...(await h.screenPoint(page, ...map)));
  await h.runFrames(page, 10);
  const mark = await replyMark(page, 'messages');
  assert.ok(mark.found, `${what}: precondition: the reply prompt listed in the message scroll`);
  await keepShot(page, 'messages', `${what}-prompt`);
  // The caret's observed ink edge where it belongs (`replySeen`: the pen's end after the run's
  // text and the spaces between, the glyph's own blank columns on): within a canvas pixel either
  // way (the page puts each block's edge on the nearest whole pixel).
  const placed = (r) => r.found && r.edge !== null && r.edge >= r.want - 1 && r.edge <= r.want + 1;
  const prompt = async (where, label) => {
    const r = await replySeen(page, where);
    check(r.found && r.caret, `${label}: the cursor listed as the prompt's caret`);
    check(placed(r), `${label}: the caret's observed ink edge at the prompt's pen end (its glyph's blank columns on), within a pixel`);
    check(r.found && r.gap - r.lead <= ((r.spaces ?? 0) + 1) * r.space + FONT_TOL, `${label}: the caret's ink from within one space advance (and the spaces between) of the prompt's ink end, its glyph's blank columns aside`);
    check(r.found && r.ownPaper, `${label}: the cursor's own cell the paper alone`);
    if (where !== 'whole') {
      const runs = (await fontRuns(page))[where];
      check(runs.carets === 1 && runs.caretsPlaced === 1 && runs.off === 0 && runs.fits === runs.runs, `${label}: the scroll's other pixels the composition's, every run where the font puts it`);
    }
  };
  await prompt('messages', '(i) the regions');
  if (letterbox) {
    const { view } = await letterboxed(page, () => prompt('whole', '(i) the letterbox'));
    assert.equal(view, 'whole', `${what}: precondition: the frame letterboxed`);
  }
  await typeKeys(page, ['Space', 'Space', ...CARET_WORD]);
  await keepShot(page, 'messages', `${what}-typed`);
  const typed = async (where, label) => {
    const r = await replySeen(page, where);
    check(r.found && r.joined, `${label}: the prompt and the word typed after two spaces one run`);
    check(r.found && r.typed, `${label}: the two spaces listed as typed`);
    check(r.found && r.wordInk >= r.wordWant - FONT_TOL && r.wordInk <= r.wordWant + FONT_TOL, `${label}: the word's observed first ink where the pen's end after the prompt and two space advances puts it (within the font's fringe)`);
    check(r.found && r.wordInk - r.promptEnd <= 2 * r.space + FONT_TOL, `${label}: the word's first ink at most two space advances after the prompt's pen end`);
    return r;
  };
  const right = await typed('messages', '(ii) the regions');
  check(placed(right), 'the caret after the word: its observed ink edge at the word\'s pen end, within a pixel');
  if (letterbox) await letterboxed(page, () => typed('whole', '(ii) the letterbox'));
  // Spaces typed after the word, then Backspace over one: the caret's observed edge where the pen's
  // end and the spaces put it, moved on by one measured space advance per space (within a pixel:
  // each edge is on the nearest whole pixel). The cursor turns between the reads: each edge is
  // taken less its glyph's own blank columns (the cell's left edge as drawn).
  const caretAfter = async (spaces, label) => {
    const r = await replySeen(page, 'messages');
    const runs = (await fontRuns(page)).messages;
    check(r.found && r.caret && r.spaces === spaces, `${label}: the caret after ${spaces} typed space(s)`);
    check(placed(r), `${label}: its observed ink edge where the pen's end and the spaces put it, within a pixel`);
    check(runs.carets === 1 && runs.caretsPlaced === 1 && runs.off === 0 && runs.fits === runs.runs, `${label}: the scroll's other pixels the composition's, every run where the font puts it`);
    return r;
  };
  const moved = (a, b, n, label) => check(a.edge !== null && b.edge !== null && Math.abs((b.edge - b.lead) - (a.edge - a.lead) - n * a.space) <= 1, `${label}: the caret's observed edge moved by ${n} measured space advance(s), within a pixel`);
  await typeKeys(page, ['Space', 'Space']);
  const two = await caretAfter(2, 'trailing typed spaces');
  moved(right, two, 2, 'trailing typed spaces');
  await typeKeys(page, ['Backspace']);
  const one = await caretAfter(1, 'Backspace over a typed space');
  moved(two, one, -1, 'Backspace over a typed space');
  // Letters to the window's end: the echo wraps to the next row's first cell (the scroll moves
  // the line up when it was the last row).
  const left = await page.evaluate(() => {
    const { from } = globalThis.__u6Layout.regions.find((r) => r.name === 'messages');
    return { right: from[0] + from[2], first: from[0] };
  });
  const { last, row } = await page.evaluate(() => window.__reply);
  const cursorX = last + 8 * (1 + 2 + CARET_WORD.length + 1);
  await typeKeys(page, Array((left.right - cursorX) / 8).fill('z'));
  const wrapped = await page.evaluate(([first, y0]) => {
    const g = globalThis.__shownGame;
    const L = globalThis.__u6Layout;
    const { from } = L.regions.find((r) => r.name === 'messages');
    const runs = window.__fontRunsOf(g.hd_glyphs(), from);
    const alone = runs.symbols.filter((s) => s.x === first && s.y >= y0);
    return { alone: alone.length, y: alone[0]?.y ?? null };
  }, [left.first, row]);
  check(wrapped.alone === 1, 'the echo wrapped: the cursor at the next row\'s first cell, drawn alone there');
  const after = (await fontRuns(page)).messages;
  check(after.symbols === 1 && after.symbolsPlaced === 1 && after.off === 0, 'the cursor alone at its own cell, its observed ink edge there; the scroll\'s other pixels the composition\'s');
  const line = wrapped.y === row ? row - 8 : row;
  const up = await replySeen(page, 'messages', line);
  check(up.found && up.joined && up.typed, 'after the wrap (and a scroll) the typed line still one run');
  // (iii) Return: the conversation answers and ends; the typed line, moved up, keeps its form.
  await typeKeys(page, ['Enter']);
  assert.ok(await framesUntil(page, () => document.getElementById('screen').dataset.prompt === '1', 200), `${what}: precondition: the conversation ended, the prompt back`);
  await h.runFrames(page, 10);
  const kept = await typedLines(page, 'messages');
  check(kept.n === 1 && kept.top < line, '(iii) after Return and the scroll the typed line keeps its joined form');
  return misses;
}

// The page with the test's conversation (koTypingFixture's archive in place of the user's,
// English: no pack) talked to up to the player's words (the reply prompt, the line read), then
// on the virtual display.
async function atReplyPrompt(context, page, env, font = true) {
  await context.addInitScript(fontRunsHook);
  await context.addInitScript(composedHook);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  if (font) await fontIs(page, 'hd');
  const { archive } = await koTypingFixture();
  assert.equal(await h.replaceKept(page, 'converse.a', archive), 1, 'one archive replaced');
  await page.reload();
  await h.waitSettled(page);
  await h.waitPrompt(page);
  if (font) await fontIs(page, 'hd');
  await page.focus('#screen');
  await page.keyboard.press('t');
  await h.waitTaken(page);
  await page.keyboard.press('2');
  await page.waitForFunction(() => globalThis.__owned === 7 && document.getElementById('screen').dataset.keys === '0', null, { timeout: 30_000 });
  await h.waitFrames(page, 2);
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 10);
}

test('HD font (spec 11 rev 3, the HD layout): at the reply prompt of the test\'s conversation (the user\'s disks, a made-up word typed) the key wait\'s cursor is drawn as the prompt\'s caret, within one space advance of its ink, its own cell the paper; two typed spaces and the word make one run with the prompt, the word at most two space advances after it; the same letterboxed; typed spaces before the caret move it on, Backspace back; the echo wrapping puts the cursor alone at the next row\'s first cell; after Return and the scroll the typed line keeps its joined form; no page error', async (env) => {
  const errors = [];
  const { context, page } = await h.openPage(env, { layout: 'hd' });
  page.on('pageerror', () => errors.push(1));
  await page.setViewportSize({ width: 1400, height: 900 });
  await atReplyPrompt(context, page, env);
  assert.equal(await page.evaluate(() => globalThis.__u6Layout.view), 'game', 'the game\'s regions');
  const misses = await caretFlow(page, 'HD layout', true);
  assert.ok(misses.length === 0, `every check met; not met: ${misses.join('; ')}`);
  assert.equal(errors.length, 0, 'no page error');
  console.log('  receipt: the HD layout and its letterbox: the cursor the prompt\'s caret within a space of its ink, its cell the paper; two typed spaces and a word one run, the word two spaces on; the caret after typed spaces and back after Backspace; the wrap\'s cursor alone at its cell; the typed line joined after the scroll: pass');
}, { needs: h.haveHfes });

test('Wide layout (spec 11 rev 3): at the reply prompt of the test\'s conversation in the message scroll\'s window (the user\'s disks, a made-up word typed) the key wait\'s cursor is drawn as the prompt\'s caret, within one space advance of its ink, its own cell the paper; two typed spaces and the word make one run with the prompt, the word at most two space advances after it; the same letterboxed; typed spaces before the caret move it on, Backspace back; the echo wrapping puts the cursor alone at the next row\'s first cell; after Return and the scroll the typed line keeps its joined form; no page error', async (env) => {
  const errors = [];
  const { context, page } = await h.openWide(env, { width: 1920, height: 1080 });
  page.on('pageerror', () => errors.push(1));
  await atReplyPrompt(context, page, env);
  assert.equal(await page.evaluate(() => globalThis.__u6Layout.view), 'game', 'the map and the windows');
  const misses = await caretFlow(page, 'Wide', true);
  assert.ok(misses.length === 0, `every check met; not met: ${misses.join('; ')}`);
  assert.equal(errors.length, 0, 'no page error');
  console.log('  receipt: Wide and its letterbox: the cursor the prompt\'s caret within a space of its ink, its cell the paper; two typed spaces and a word one run, the word two spaces on; the caret after typed spaces and back after Backspace; the wrap\'s cursor alone at its cell; the typed line joined after the scroll: pass');
}, { needs: h.haveHfes });

// The test's own greeting with a key wait inside it (`*`: the game flushes the line and waits for a
// key with the more prompt's mark, glyphs 1..4, right after the text).
const ARROW_GREET = '"Zq9 qux!*Abc xyz."';

// The page with the test's conversation (koFixture with ARROW_GREET in place of the user's,
// English) talked to up to the wait inside the greeting, on the virtual display, the pointer in
// the map; whether the more prompt's mark is listed in the message scroll.
async function atMoreMark(context, page, env) {
  await context.addInitScript(fontRunsHook);
  await context.addInitScript(composedHook);
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await h.waitPrompt(page);
  await fontIs(page, 'hd');
  const { archive } = await koFixture(0x63, { greeting: ARROW_GREET, greetingKo: FONT_GREET_KO });
  assert.equal(await h.replaceKept(page, 'converse.a', archive), 1, 'one archive replaced');
  await page.reload();
  await h.waitSettled(page);
  await h.waitPrompt(page);
  await fontIs(page, 'hd');
  await h.virtualDisplay(page, 60);
  const map = await page.evaluate(() => {
    const [x, y, w, hh] = globalThis.__u6Layout.regions.find((r) => r.name === 'map').from;
    return [x + Math.floor(w / 2), y + Math.floor(hh / 2)];
  });
  await page.mouse.move(...(await h.screenPoint(page, ...map)));
  await typeKeys(page, ['t']);
  await page.focus('#screen');
  await page.keyboard.press('2');
  return framesUntil(page, () => {
    const L = globalThis.__u6Layout;
    const { from } = L.regions.find((r) => r.name === 'messages');
    const runs = window.__fontRunsOf(globalThis.__shownGame.hd_glyphs(), from);
    return runs.some((r) => r.caret && r.caret.code >= 1 && r.caret.code <= 4) || runs.symbols.some((m) => m.code >= 1 && m.code <= 4);
  }, 200);
}

// Page script: the more prompt's mark in the message scroll as the canvas shows it: listed as the
// caret of the run before it (`caret`), its observed ink edge (`__symbolEdge`, searched from the
// run's first cell to the mark's own cell's right edge) and where it belongs (`want`: the pen's end
// after the run's text and the spaces between, the glyph's own blank columns on), and the run's
// last letter's cell (`last`).
const moreMarkSeen = (page) => page.evaluate(() => {
  const L = globalThis.__u6Layout;
  const g = globalThis.__shownGame;
  const c = document.getElementById('screen');
  const { from, to } = L.regions.find((r) => r.name === 'messages');
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  const runs = window.__fontRunsOf(g.hd_glyphs(), from, g.hd_symbol_rows?.() ?? null);
  const run = runs.find((r) => r.caret && r.caret.code >= 1 && r.caret.code <= 4);
  if (!run) return { caret: false };
  const geo = window.__runGeometry(run, from, to);
  const [sx, sy] = [to[2] / from[2], to[3] / from[3]];
  const own = window.__symbolBox({ x: run.caret.x, y: run.y }, from, to);
  const seen = window.__symbolEdge(d, c.width, run.caret, [geo.span[0], own[0] + own[2]], geo.span[1], sx, sy);
  return {
    caret: true,
    adjacent: run.caret.x === run.cells.at(-1).x + 8 * (run.caret.spaces + 1),
    edge: seen ? seen.edge : null,
    want: seen ? geo.caretLeft + seen.lead : null,
    own: own[0],
  };
});

// Spec 11 rev 3 on the more prompt's mark (`what`: the layout's label): at the wait inside the
// test's greeting the mark is listed as the caret of the greeting's run, its observed ink edge at
// the run's pen end (the glyph's blank columns on) within a canvas pixel, left of its own cell; the
// scroll's other pixels the composition's; a key ends the wait.
async function moreMarkFlow(page, what) {
  const r = await moreMarkSeen(page);
  const misses = [];
  const check = (ok, label) => {
    if (!ok) misses.push(`${what}: ${label}`);
  };
  check(r.caret && r.adjacent, 'the more prompt\'s mark listed as the caret of the run it follows');
  check(r.caret && r.edge !== null && r.edge >= r.want - 1 && r.edge <= r.want + 1, 'its observed ink edge at the run\'s pen end (its glyph\'s blank columns on), within a pixel');
  check(r.caret && r.edge !== null && r.edge < r.own, 'drawn left of its own cell');
  const runs = (await fontRuns(page)).messages;
  check(runs.carets === 1 && runs.caretsPlaced === 1 && runs.off === 0 && runs.fits === runs.runs, 'the scroll\'s other pixels the composition\'s, every run where the font puts it');
  await typeKeys(page, ['Space']);
  return misses;
}

test('HD font (spec 11 rev 3, the more prompt\'s mark, the HD layout): at a key wait inside the test\'s greeting (the user\'s disks) the more prompt\'s mark is drawn as the caret of the text it follows, its observed ink edge at the text\'s pen end within a canvas pixel; no page error', async (env) => {
  const errors = [];
  const { context, page } = await h.openPage(env, { layout: 'hd' });
  page.on('pageerror', () => errors.push(1));
  await page.setViewportSize({ width: 1400, height: 900 });
  const listed = await atMoreMark(context, page, env);
  const misses = listed ? await moreMarkFlow(page, 'HD layout') : ['HD layout: the more prompt\'s mark listed in the message scroll'];
  assert.ok(misses.length === 0, `every check met; not met: ${misses.join('; ')}`);
  assert.equal(errors.length, 0, 'no page error');
  console.log('  receipt: the HD layout: the more prompt\'s mark the caret of its text, its ink edge at the pen end within a pixel: pass');
}, { needs: h.haveHfes });

test('Wide layout (spec 11 rev 3, the more prompt\'s mark): at a key wait inside the test\'s greeting in the message scroll\'s window (the user\'s disks) the more prompt\'s mark is drawn as the caret of the text it follows, its observed ink edge at the text\'s pen end within a canvas pixel; no page error', async (env) => {
  const errors = [];
  const { context, page } = await h.openWide(env, { width: 1920, height: 1080 });
  page.on('pageerror', () => errors.push(1));
  const listed = await atMoreMark(context, page, env);
  const misses = listed ? await moreMarkFlow(page, 'Wide') : ['Wide: the more prompt\'s mark listed in the message scroll'];
  assert.ok(misses.length === 0, `every check met; not met: ${misses.join('; ')}`);
  assert.equal(errors.length, 0, 'no page error');
  console.log('  receipt: Wide: the more prompt\'s mark the caret of its text, its ink edge at the pen end within a pixel: pass');
}, { needs: h.haveHfes });

test('Original layout (spec 11 rev 3): with the HD font chosen and HD on, at the reply prompt of the test\'s conversation with two spaces and a made-up word typed, nothing is listed and the canvas is the HD frame exactly (the page draws nothing over it but the pointer), the same frame before and after the font chosen again (pixel, then HD); no page error', async (env) => {
  const errors = [];
  const { context, page } = await h.openPage(env, { layout: 'original' });
  page.on('pageerror', () => errors.push(1));
  await atReplyPrompt(context, page, env, false);
  await typeKeys(page, ['Space', 'Space', ...CARET_WORD]);
  await freezeGame(page, true);
  await h.runFrames(page, 2);
  const s = await fontShown(page);
  assert.ok(s.font === 'pixel' && s.mode === false && !s.listed, 'the Original layout: the pixel font, nothing listed');
  const frameSeen = async () => (await pointerAreaHook(page), page.evaluate(() => {
    const c = document.getElementById('screen');
    const g = globalThis.__shownGame;
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
    const f = g.hd_frame_rgba();
    const pointer = window.__pointerArea();
    let [compared, differ] = [0, 0];
    let v = 0x811c9dc5;
    for (let i = 0; i < d.length; i += 4) {
      const p = i / 4;
      if (window.__inPointer(pointer, p % c.width, Math.floor(p / c.width))) continue;
      compared += 1;
      if (d[i] !== f[i] || d[i + 1] !== f[i + 1] || d[i + 2] !== f[i + 2]) differ += 1;
      for (let k = 0; k < 3; k += 1) v = Math.imul(v ^ d[i + k], 0x01000193) >>> 0;
    }
    return { hd: g.hd_on(), size: c.width === g.width() * g.hd_scale(), compared, differ, hash: v };
  }));
  const before = await frameSeen();
  assert.ok(before.hd && before.size && before.compared > 0 && before.differ === 0, 'the canvas is the HD frame exactly');
  await chooseFont(page, 'pixel');
  await chooseFont(page, 'hd');
  await page.click('#setup-ok');
  await h.runFrames(page, 2);
  const after = await frameSeen();
  assert.ok(after.differ === 0 && after.hash === before.hash, 'the same frame before and after the font chosen again');
  await freezeGame(page, false);
  assert.equal(errors.length, 0, 'no page error');
  console.log('  receipt: the Original layout at the reply prompt with typed spaces: nothing listed, the canvas the HD frame exactly, the same before and after the font chosen again: pass');
}, { needs: h.haveHfes });

test('HD font (spec 11 Acceptance 3, Design 4-5): the first-run chooser and Settings offer "Font: HD font / Pixel font" in the page\'s language, the HD font checked; the font file is asked for once it is wanted (the HD layout, the HD font: from this origin, 200, font/woff2, no CSP report), not with the pixel font or the Original layout; a choice is kept at once and across a reload, any other kept value is the HD font; with storage that throws the HD font, switched for the visit, nothing kept; a font file that does not load leaves the pixel font (synthetic, no game)', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const origin = new URL(env.server.url).origin;
  let { context, page } = await h.openPage(env, { setup: null, layout: null });
  let answers = fontAnswers(context);
  await h.load(page, env);
  const choice = () => page.evaluate(() => ({
    title: document.getElementById('font-title').textContent,
    labels: [...document.querySelectorAll('#font-choice label')].map((l) => l.textContent.replace(/\s+/g, ' ').trim()),
    inSetup: document.getElementById('setup').contains(document.getElementById('font-choice')),
    open: !document.getElementById('setup').hidden,
  }));
  let c = await choice();
  assert.ok(c.open && c.inSetup, 'the first-run chooser holds it');
  assert.deepEqual([c.title, c.labels], [TEXT.ko.font, [TEXT.ko.fontHd, TEXT.ko.fontPixel]], 'its words in Korean (the first run)');
  await fontIs(page, 'hd');
  let s = await fontShown(page);
  assert.deepEqual(s.radios, [['hd', true], ['pixel', false]], 'the HD font checked');
  assert.ok(s.file === 'loaded' && s.stored === null, 'the HD font loaded before any game, nothing kept yet');
  assert.ok(answers.length === 1 && answers[0][0] === origin && answers[0][1] === 200 && answers[0][2] === 'font/woff2', 'the font file asked for once, from this origin, 200, font/woff2');
  assert.deepEqual(await page.evaluate(() => window.__csp), [], 'no CSP report');
  await page.check('input[name="setup-lang"][value="en"]');
  c = await choice();
  assert.deepEqual([c.title, c.labels], [TEXT.en.font, [TEXT.en.fontHd, TEXT.en.fontPixel]], 'its words in English');
  // The chooser's choice: kept at once, and after OK and a reload (no chooser then) in Settings.
  await page.uncheck('#setup-voices');
  await page.check('input[name="font"][value="pixel"]');
  s = await fontShown(page);
  assert.ok(s.font === 'pixel' && s.stored === 'pixel', 'the pixel font, kept at once');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await page.reload();
  await h.waitSettled(page);
  assert.equal(await page.evaluate(() => document.getElementById('setup').hidden), true, 'no chooser after its OK');
  await page.click('#settings');
  s = await fontShown(page);
  assert.ok(s.font === 'pixel' && s.file === 'none', 'kept: the pixel font, no font file asked for');
  assert.deepEqual(s.radios, [['hd', false], ['pixel', true]], 'Settings shows it');
  assert.equal(answers.length, 1, 'the pixel font: nothing asked for');
  await page.check('input[name="font"][value="hd"]');
  await fontIs(page, 'hd');
  assert.ok(answers.length === 2 && answers[1][1] === 200, 'the HD font chosen: the file asked for, 200');
  await page.reload();
  await h.waitSettled(page);
  await fontIs(page, 'hd');
  s = await fontShown(page);
  assert.ok(s.stored === 'hd' && s.radios[0][1], 'kept: the HD font');
  // Any other kept value is the HD font; the Original layout asks for no font file.
  for (const [kept, layout, want, file] of [['wide', 'hd', 'hd', 'loaded'], ['hd', 'original', 'pixel', 'none'], ['pixel', 'original', 'pixel', 'none']]) {
    await page.evaluate(([f, l]) => {
      localStorage.setItem('u6.font', f);
      localStorage.setItem('u6.layout', l);
    }, [kept, layout]);
    const n = answers.length;
    await page.reload();
    await h.waitSettled(page);
    await fontIs(page, want);
    s = await fontShown(page);
    assert.ok(s.file === file && s.radios[kept === 'pixel' ? 1 : 0][1], `kept ${kept} with ${layout}: ${want}, the file ${file}`);
    assert.equal(answers.length, n + (file === 'none' ? 0 : 1), `kept ${kept} with ${layout}: the file asked for ${file === 'none' ? 'not' : 'once'}`);
  }
  await context.close();
  // Storage that throws: the HD font, the pixel font for the visit, nothing kept.
  ({ context, page } = await h.openPage(env, { layout: null }));
  await context.addInitScript(layoutStorageThrows);
  answers = fontAnswers(context);
  await h.load(page, env);
  await fontIs(page, 'hd');
  s = await fontShown(page);
  assert.ok(s.stored === 'unreadable' && s.radios[0][1], 'storage that throws: the HD font');
  await chooseFont(page, 'pixel');
  assert.equal((await fontShown(page)).font, 'pixel', 'the pixel font for the visit');
  await page.reload();
  await h.waitSettled(page);
  await fontIs(page, 'hd');
  assert.ok((await fontShown(page)).radios[0][1], 'nothing kept: the HD font after a reload');
  await context.close();
  // A font file the site does not answer (404): the pixel font for the visit, a content-free line.
  ({ context, page } = await h.openPage(env, { layout: null }));
  const words = consoleText(page);
  await context.route((url) => url.pathname === FONT_PATH, (route) => route.fulfill({ status: 404, body: 'not found' }));
  await h.load(page, env);
  await page.waitForFunction(() => document.body.dataset.fontFile === 'failed', null, { timeout: 30_000 });
  s = await fontShown(page);
  assert.ok(s.font === 'pixel' && s.radios[0][1], 'the file not there: the pixel font, the HD font still chosen');
  assert.ok(words.some((m) => m.startsWith('the HD font did not load')), 'the line says so');
  console.log('  receipt: the chooser\'s and Settings\' Font in Korean and English; the font file from this origin (200) only while the HD font and the HD layout are in effect; the choice kept at once and across reloads; storage that throws: for the visit; a file not there: the pixel font: pass');
});

// --- the Wide layout (spec 13, spec 14) -------------------------------------------------------
// The Wide build (site/pkg-wide: the UI's 320 x 200 at the frame's top left, the map beside it,
// under the UI the side panel's three views in their slots, spec 14) on a 1920 x 1080 canvas: the
// map covering it, framed windows over it. The test's own copy of spec 13's Geometry (Wide
// column), of spec 10's region rects and of spec 14's window rects; the window rects are checked
// against where spec 13 Design 5 and spec 14 Design 7 put them, read from the page's table. The
// frame's size is read from the engine (Game.width / height); spec 14's 912 x 408 is asserted
// once, as its contract. What these tests check of the map is that the canvas shows the frame's
// pixels there, whatever they are, and where the pointer goes.
const WIDE_GEOM = { mapX: 320, mapY: 0, halfW: 18, halfH: 10, shown: [328, 8, 576, 320] };
const WIDE_CANVAS = [1920, 1080];
// Spec 14 Design 1: the Wide frame (the contract).
const WIDE_FRAME = [912, 408];
// The windows: `from` (frame pixels), the size of their `to` (canvas pixels) and where they go:
// spec 10's regions at spec 10's sizes (the date line, the message scroll, the command bar), spec
// 14's actor windows and the side panel's own window at 3 (site/app.js ACTOR_S); `corner`, with
// `under`, and `leftOf`, `rightOf`, `below` as site/app.js's WIDE_WINDOWS, `view` the panel mode
// each shows, `other` the side panel's own (shown only in another mode or a conversation),
// `named` the shown member's name in its title.
const WIDE_PARTS = {
  date: { from: [16, 168, 152, 8], size: [532, 28], corner: 'top-right', title: 'windowDate' },
  party: { from: [8, 206, 144, 98], size: [432, 294], corner: 'top-right', under: 'date', view: 0x91, title: 'windowParty' },
  stats: { from: [160, 206, 144, 98], size: [432, 294], leftOf: 'party', view: 0x90, named: true, title: 'windowAttributes' },
  doll: { from: [8, 320, 72, 88], size: [216, 264], below: 'stats', view: 0x92, named: true, title: 'windowDoll' },
  backpack: { from: [80, 320, 72, 88], size: [216, 264], rightOf: 'doll', view: 0x92, named: true, title: 'windowBackpack' },
  messages: { from: [176, 112, 136, 80], size: [476, 280], corner: 'bottom-right', title: 'windowMessages' },
  commands: { from: [8, 176, 160, 17], size: [640, 68], corner: 'bottom-left', title: 'windowCommands' },
  other: { from: [168, 6, 144, 98], size: [432, 294], corner: 'top-left', other: true, title: 'windowPanel' },
};
// The windows the page lists in one of the actor views' modes, in its order (the side panel's own
// last, after them, in another mode or a conversation).
const WIDE_NAMES = ['date', 'party', 'stats', 'doll', 'backpack', 'messages', 'commands'];
const ACTOR_MODES = [0x90, 0x91, 0x92];
const WIDE_EDGE = 8; // the windows' frames this far from the canvas's edges (Design 5, brief W5)
const WIDE_GAP = 8; // a window this far from the one it is placed by (site/app.js WINDOW_GAP)
// A window's frame: its border this wide, its title strip this tall above its content
// (site/app.js WINDOW_BORDER, WINDOW_TITLE).
const WIDE_BORDER = 4;
const WIDE_TITLE = 20;
// A title keeps this many columns at its strip's right end clear (site/app.js WINDOW_TITLE_PAD
// less a fringe): the strip's colour alone there.
const TITLE_CLEAR = 4;


// A gated page of the Wide build (`openWide`; with `fresh`, a page that keeps no screen choice:
// the page's default), the pixel font kept (as `bootLayout`: a listed cell is flat paper in the
// HD frame), the user's disks started in `game`, the canvas focused. `init` runs in the page
// before its scripts. Also: the paths of the engine glue the page asked for.
async function bootWide(env, { fresh = false, init = null, width = 1400, height = 900 } = {}) {
  const { context, page } = fresh
    ? await h.openPage(env, { layout: null, font: 'pixel' })
    : await h.openWide(env, { width, height, font: 'pixel' });
  if (fresh) await page.setViewportSize({ width, height });
  if (init) await context.addInitScript(init);
  const glues = [];
  page.on('request', (r) => {
    const path = new URL(r.url()).pathname;
    if (path.endsWith('/u6_web.js')) glues.push(path);
  });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  await page.focus('#screen');
  await h.waitPrompt(page);
  return { context, page, glues };
}

// The page's Wide table and what it shows: the layout, the view, the canvas, its box's
// rendering, the frame's and the compositor's sizes, the regions and the windows.
const wideShown = (page) => page.evaluate(() => {
  const c = document.getElementById('screen');
  const L = globalThis.__u6Layout;
  const g = globalThis.__shownGame;
  return {
    layout: document.body.dataset.layout,
    wide: L.wide,
    view: L.view,
    size: [c.width, c.height],
    frame: [g.width(), g.height()],
    hd: g.hd_on(),
    scale: g.hd_on() ? g.hd_scale() : 0,
    layoutScale: L.scale,
    regions: L.regions,
    windows: L.windows,
    whole: L.whole,
    rest: L.rest,
  };
});

// Whether rects (x, y, w, h) `a` and `b` share a pixel.
const meets = ([ax, ay, aw, ah], [bx, by, bw, bh]) => ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah;
const holds = ([ax, ay, aw, ah], [bx, by, bw, bh]) => bx >= ax && by >= ay && bx + bw <= ax + aw && by + bh <= ay + ah;

// Page script, the display frozen by the caller: the canvas against the frame it is drawn from
// (`hd`: the HD frame at the layout's scale, else the classic frame). Per window, its content's
// pixels where the frame is one colour round the game pixel shown (3 x 3 game pixels inside the
// frame; at the centre of what shows it, where no window in front covers it): `compared`,
// `differ` (more than 2 off in a channel), `dimDiffer` (more than 2 off the frame's colour at
// half over the border's, a dimmed window's); a one-to-one window (the command bar at x4) every
// pixel (`exact`, `exactDiffer`). The map likewise at a grid of canvas pixels not under a window
// (`map`). The frames: per window, the colours of its border (the outer ring; `ringInk` its
// corner's), of its title strip's last `titleClear` columns (no title there; `barInk`) and the
// title strip's colours besides them (the name's ink). The pointer's place is left out
// (`pointerAreaHook`). Counts and the page's own window colours only.
const wideCheck = async (page, hd, titleClear = TITLE_CLEAR) => (await pointerAreaHook(page), page.evaluate(([onHd, clear]) => {
  const c = document.getElementById('screen');
  const L = globalThis.__u6Layout;
  const g = globalThis.__shownGame;
  const s = onHd ? g.hd_scale() : 1;
  const f = onHd ? g.hd_frame_rgba() : g.frame_rgba();
  const fw = g.width() * s;
  const fh = g.height() * s;
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  const px = (x, y) => [0, 1, 2].map((k) => d[(y * c.width + x) * 4 + k]);
  const fr = (x, y) => [0, 1, 2].map((k) => f[(y * fw + x) * 4 + k]);
  // The frame's colour of game pixel (gx, gy) when its 3 x 3 game pixels are one colour there
  // (all of them in the frame).
  const flat = (gx, gy) => {
    if (gx < 1 || gy < 1 || (gx + 2) * s > fw || (gy + 2) * s > fh) return null;
    const ref = fr(gx * s, gy * s);
    for (let y = (gy - 1) * s; y < (gy + 2) * s; y += 1) {
      for (let x = (gx - 1) * s; x < (gx + 2) * s; x += 1) {
        const p = fr(x, y);
        if (p[0] !== ref[0] || p[1] !== ref[1] || p[2] !== ref[2]) return null;
      }
    }
    return ref;
  };
  const off = (a, b) => [0, 1, 2].some((k) => Math.abs(a[k] - b[k]) > 2);
  const pointer = window.__pointerArea();
  const out = { windows: {}, map: { compared: 0, differ: 0 } };
  for (const w of L.windows) {
    const r = { compared: 0, differ: 0, dimDiffer: 0, exact: 0, exactDiffer: 0 };
    const [x0, y0, ww, wh] = w.from;
    const paper = px(w.outer[0], w.outer[1]);
    for (let gy = y0 + 1; gy < y0 + wh - 1; gy += 1) {
      for (let gx = x0 + 1; gx < x0 + ww - 1; gx += 1) {
        const want = flat(gx, gy);
        const place = want ? L.toCanvas(gx, gy) : null;
        if (!place) continue; // not flat, or under a window in front of this one
        const [cx, cy] = place.map(Math.floor);
        if (window.__inPointer(pointer, cx, cy)) continue;
        const got = px(cx, cy);
        r.compared += 1;
        if (off(got, want)) r.differ += 1;
        if (off(got, [0, 1, 2].map((k) => (want[k] + paper[k]) / 2))) r.dimDiffer += 1;
      }
    }
    if (w.to[2] === s * ww && w.to[3] === s * wh) {
      for (let y = 0; y < w.to[3]; y += 1) {
        for (let x = 0; x < w.to[2]; x += 1) {
          if (window.__inPointer(pointer, w.to[0] + x, w.to[1] + y)) continue;
          r.exact += 1;
          const a = px(w.to[0] + x, w.to[1] + y);
          const b = fr(s * x0 + x, s * y0 + y);
          if (a[0] !== b[0] || a[1] !== b[1] || a[2] !== b[2]) r.exactDiffer += 1;
        }
      }
    }
    const key = (p) => p.join(',');
    const [ox, oy, ow, oh] = w.outer;
    const ring = new Set();
    for (let x = ox; x < ox + ow; x += 1) ring.add(key(px(x, oy))).add(key(px(x, oy + oh - 1)));
    for (let y = oy; y < oy + oh; y += 1) ring.add(key(px(ox, y))).add(key(px(ox + ow - 1, y)));
    const [bx, by, bw, bh] = w.bar;
    const plain = new Set();
    for (let y = by; y < by + bh; y += 1) for (let x = bx + bw - clear; x < bx + bw; x += 1) plain.add(key(px(x, y)));
    const ink = new Set();
    for (let y = by; y < by + bh; y += 1) for (let x = bx; x < bx + 120; x += 1) if (!plain.has(key(px(x, y)))) ink.add(key(px(x, y)));
    r.ring = ring.size;
    r.plain = plain.size;
    r.ink = ink.size;
    r.ringInk = paper;
    r.barInk = px(bx + bw - 1, by + Math.floor(bh / 2));
    out.windows[w.name] = r;
  }
  const map = L.regions.find((r) => r.name === 'map');
  for (let cy = 4; cy < c.height; cy += 24) {
    for (let cx = 4; cx < c.width; cx += 24) {
      if (L.windows.some(({ outer: [x, y, w, h] }) => cx >= x - 2 && cx < x + w + 2 && cy >= y - 2 && cy < y + h + 2)) continue;
      const gx = map.from[0] + Math.floor(((cx + 0.5 - map.to[0]) * map.from[2]) / map.to[2]);
      const gy = map.from[1] + Math.floor(((cy + 0.5 - map.to[1]) * map.from[3]) / map.to[3]);
      const want = flat(gx, gy);
      if (!want || window.__inPointer(pointer, cx, cy)) continue;
      out.map.compared += 1;
      if (off(px(cx, cy), want)) out.map.differ += 1;
    }
  }
  return out;
}, [hd, titleClear]));

// The windows of `shown` are where spec 13 Design 5 and spec 14 Design 7 put them (WIDE_PARTS):
// content the size the test gives it, in a frame whose title strip is above it; at a corner the
// frame WIDE_EDGE from the canvas's edges there (under the window `under` names, WIDE_GAP under
// it), beside (`leftOf`, `rightOf`: the tops in line) or under (`below`: the left edges in line)
// the window named, WIDE_GAP from it; none meeting another, nor the leader's cell (the map's
// centre, by the test's own geometry: view cell (HALF_W, HALF_H) at frame (MAP_X, MAP_Y) plus 16
// a cell, the shown rect scaled by 1080 / 320 from canvas x -12); with `label`. In one of the
// actor views' modes the windows are WIDE_NAMES; with `other`, the side panel's own after them
// (placed and meeting none too).
function windowsPlaced(shown, label, { other = false } = {}) {
  const names = shown.windows.map((w) => w.name);
  assert.deepEqual(names, other ? [...WIDE_NAMES, 'other'] : WIDE_NAMES, `${label}: the windows`);
  const [cw, ch] = WIDE_CANVAS;
  const at = Object.fromEntries(shown.windows.map((w) => [w.name, w]));
  for (const w of shown.windows) {
    const part = WIDE_PARTS[w.name];
    assert.deepEqual(w.from, part.from, `${label}: ${w.name} shows its rect`);
    assert.deepEqual(w.to.slice(2), part.size, `${label}: ${w.name} at its size`);
    assert.ok(holds(w.outer, w.to) && holds(w.outer, w.bar), `${label}: ${w.name}'s frame holds its title strip and its content`);
    assert.ok(w.bar[1] + w.bar[3] <= w.to[1] && w.bar[0] === w.to[0] && w.bar[2] === w.to[2] && w.bar[3] >= 12, `${label}: ${w.name}'s title strip above its content, as wide`);
    const [ox, oy, ow, oh] = w.outer;
    assert.ok(ox >= 0 && oy >= 0 && ox + ow <= cw && oy + oh <= ch, `${label}: ${w.name} inside the canvas`);
    if (part.corner) {
      const right = part.corner.endsWith('right');
      const bottom = part.corner.startsWith('bottom');
      assert.equal(right ? cw - (ox + ow) : ox, WIDE_EDGE, `${label}: ${w.name} ${WIDE_EDGE} px from the ${right ? 'right' : 'left'} edge`);
      if (bottom) assert.equal(ch - (oy + oh), WIDE_EDGE, `${label}: ${w.name} ${WIDE_EDGE} px from the bottom`);
      else if (part.under) assert.equal(oy, at[part.under].outer[1] + at[part.under].outer[3] + WIDE_GAP, `${label}: ${w.name} under ${part.under}`);
      else assert.equal(oy, WIDE_EDGE, `${label}: ${w.name} ${WIDE_EDGE} px from the top`);
    } else if (part.leftOf || part.rightOf) {
      const [bx, by, bw] = at[part.leftOf ?? part.rightOf].outer;
      assert.deepEqual([ox, oy], [part.leftOf ? bx - WIDE_GAP - ow : bx + bw + WIDE_GAP, by], `${label}: ${w.name} beside ${part.leftOf ?? part.rightOf}`);
    } else {
      const [bx, by, , bh] = at[part.below].outer;
      assert.deepEqual([ox, oy], [bx, by + bh + WIDE_GAP], `${label}: ${w.name} under ${part.below}`);
    }
  }
  for (let i = 0; i < shown.windows.length; i += 1) {
    for (let j = i + 1; j < shown.windows.length; j += 1) {
      assert.ok(!meets(shown.windows[i].outer, shown.windows[j].outer), `${label}: ${shown.windows[i].name} and ${shown.windows[j].name} do not overlap`);
    }
  }
  const g = WIDE_GEOM;
  const k = WIDE_CANVAS[1] / g.shown[3];
  const [lx, ly] = [g.mapX + g.halfW * 16, g.mapY + g.halfH * 16];
  const leader = [-12 + (lx - g.shown[0]) * k, (ly - g.shown[1]) * k, 16 * k, 16 * k];
  for (const w of shown.windows) assert.ok(!meets(w.outer, leader), `${label}: ${w.name} leaves the leader's cell clear`);
}

// The map region of `shown` covers the canvas: spec 13's shown rect scaled by the larger of the
// canvas's ratios to it (1080 / 320), centred: 12 px of it past either side.
function mapCovers(shown, label) {
  const map = shown.regions.find((r) => r.name === 'map');
  assert.deepEqual(map.from, WIDE_GEOM.shown, `${label}: the map is spec 13's shown rect`);
  const [cw, ch] = WIDE_CANVAS;
  const k = Math.max(cw / map.from[2], ch / map.from[3]);
  assert.equal(k, 3.375, `${label}: the cover scale`);
  assert.deepEqual(map.to, [(cw - map.from[2] * k) / 2, (ch - map.from[3] * k) / 2, map.from[2] * k, map.from[3] * k], `${label}: scaled to cover, centred`);
  assert.ok(map.to[0] === -12 && map.to[0] + map.to[2] === cw + 12 && map.to[1] === 0 && map.to[3] === ch, `${label}: 12 px cropped at either side, the whole height`);
}

test('Wide layout (spec 13 A1, A3, A4, Design 2 and 5; spec 14 Design 1 and 7): a fresh page (no screen kept) loads the Wide build (site/pkg-wide only: the frame 912 x 408, read from the engine) and shows the Wide layout: the canvas 1920 x 1080 in a 16:9 box, the compositor at the layout\'s scale; the map (spec 13\'s shown rect) scaled to cover the canvas, the canvas the frame\'s colours there; the date line, the party, the attributes, the paper doll, the backpack, the message scroll and the command bar in framed windows over it, each titled, at its default place (the date line, the scroll and the command bar at their corners 8 px from the edges, the party under the date line, the attributes left of it, the doll and the backpack side by side under them), at its size, none overlapping nor over the leader\'s cell, showing the frame\'s colours (the command bar one to one with the x4 frame); HD off: the same layout from the classic frame, pixelated (each pixel the frame\'s); every window\'s centre and every pixel of a window maps to a game pixel of its rect and back; no page error', async (env) => {
  const errors = [];
  const { page, glues } = await bootWide(env, { fresh: true });
  page.on('pageerror', () => errors.push(1));
  assert.ok(glues.length > 0 && glues.every((p) => p === '/pkg-wide/u6_web.js'), 'the Wide build\'s glue, and no other');
  let s = await wideShown(page);
  assert.ok(s.layout === 'wide' && s.wide && s.view === 'game', 'the Wide layout in game');
  // The frame as the engine has it: spec 14's size, its contract; every window's rect in it.
  assert.deepEqual(s.frame, WIDE_FRAME, 'the Wide frame (spec 14 Design 1)');
  assert.ok(s.windows.every(({ from: [x, y, w, hh] }) => x + w <= s.frame[0] && y + hh <= s.frame[1]), 'every window\'s rect in the frame');
  assert.deepEqual(s.size, WIDE_CANVAS, 'the canvas 1920 x 1080');
  assert.ok(s.hd && s.scale === s.layoutScale && [2, 4].includes(s.scale), 'HD at the layout\'s scale');
  const box = await page.evaluate(() => {
    const r = document.getElementById('screen').getBoundingClientRect();
    return [r.width, r.height];
  });
  assert.ok(Math.abs(box[0] / box[1] - 16 / 9) < 0.01, 'a 16:9 box');
  mapCovers(s, 'HD on');
  windowsPlaced(s, 'HD on');
  // Every pixel of a window's content maps to a game pixel of its rect and back to that pixel's
  // place; a window's centre to its rect's centre.
  const mapped = await page.evaluate(() => {
    const L = globalThis.__u6Layout;
    let bad = 0;
    let n = 0;
    const centres = {};
    for (const { name, from: [fx, fy, fw, fh], to: [x, y, w, hh] } of L.windows) {
      for (let py = y; py < y + hh; py += 1) {
        for (let px = x; px < x + w; px += 1) {
          const [gx, gy] = L.toGame(px + 0.5, py + 0.5);
          n += 1;
          if (gx < fx || gx >= fx + fw || gy < fy || gy >= fy + fh) bad += 1;
          const [bx, by] = L.toCanvas(gx, gy);
          if (Math.abs(bx - px - 0.5) > w / fw / 2 + 1e-9 || Math.abs(by - py - 0.5) > hh / fh / 2 + 1e-9) bad += 1;
        }
      }
      centres[name] = L.toGame(x + w / 2, y + hh / 2);
    }
    return { bad, n, centres };
  });
  assert.ok(mapped.n > 0 && mapped.bad === 0, 'every window pixel maps to a pixel of its rect and back');
  for (const name of WIDE_NAMES) {
    const { from } = WIDE_PARTS[name];
    assert.deepEqual(mapped.centres[name], [from[0] + Math.floor(from[2] / 2), from[1] + Math.floor(from[3] / 2)], `${name}: its centre is its rect's centre`);
  }
  // What the canvas shows, the display frozen.
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  let shown = await wideCheck(page, true);
  for (const [name, r] of Object.entries(shown.windows)) {
    assert.ok(r.compared > 0 && r.differ === 0, `HD on: ${name} shows the frame's colours`);
    assert.ok(r.ring === 1 && r.plain === 1 && r.ink > 0, `HD on: ${name} framed in one colour, titled on a strip of one colour`);
  }
  // One to one at S 4 only (the command bar's 640 x 68 is its rect at x4).
  if (s.layoutScale === 4) assert.ok(shown.windows.commands.exact === 640 * 68 && shown.windows.commands.exactDiffer === 0, 'HD on: the command bar one to one with the x4 frame');
  // At least 50 flat places on the map's grid (the map as the engine draws it now: textured
  // tiles leave fewer than a black map does).
  assert.ok(shown.map.compared > 50 && shown.map.differ === 0, 'HD on: the map, scaled to cover, the frame\'s colours');
  // HD off: the same layout, from the classic frame, pixelated.
  await page.click('#settings');
  await page.click('#hd');
  await h.runFrames(page, 2);
  s = await wideShown(page);
  assert.ok(s.layout === 'wide' && s.view === 'game' && !s.hd && s.size[0] === 1920 && s.size[1] === 1080, 'HD off: the Wide layout still');
  mapCovers(s, 'HD off');
  windowsPlaced(s, 'HD off');
  shown = await wideCheck(page, false);
  for (const [name, r] of Object.entries(shown.windows)) {
    assert.ok(r.compared > 0 && r.differ === 0, `HD off: ${name} shows the classic frame's colours`);
  }
  assert.ok(shown.map.compared > 50 && shown.map.differ === 0, 'HD off: the map, the classic frame\'s colours');
  // Pixelated: every canvas pixel of a window is the classic pixel it shows (those whose centre
  // falls within 0.15 of a game pixel's edge left out: the nearest sample there is the scaler's
  // rounding).
  const exact = await page.evaluate(() => {
    const c = document.getElementById('screen');
    const L = globalThis.__u6Layout;
    const g = globalThis.__shownGame;
    const f = g.frame_rgba();
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
    let differ = 0;
    let n = 0;
    for (const { from, to: [x, y, w, hh] } of L.windows) {
      for (let py = y; py < y + hh; py += 2) {
        for (let px = x; px < x + w; px += 2) {
          const [u, v] = [((px + 0.5 - x) * from[2]) / w, ((py + 0.5 - y) * from[3]) / hh];
          if ([u, v].some((q) => q - Math.floor(q) < 0.15 || q - Math.floor(q) > 0.85)) continue;
          const [gx, gy] = L.toGame(px + 0.5, py + 0.5);
          const a = (py * c.width + px) * 4;
          const b = (gy * g.width() + gx) * 4;
          n += 1;
          if (d[a] !== f[b] || d[a + 1] !== f[b + 1] || d[a + 2] !== f[b + 2]) differ += 1;
        }
      }
    }
    return { differ, n };
  });
  assert.ok(exact.n > 0 && exact.differ === 0, 'HD off: every window pixel the classic pixel it shows (pixelated)');
  await page.click('#hd');
  await h.runFrames(page, 2);
  assert.ok((await wideShown(page)).hd, 'HD on again');
  assert.equal(errors.length, 0, 'no page error');
  console.log(`  receipt: the Wide build from a fresh page; frame ${s.frame.join(' x ')}; canvas 1920 x 1080, map cover ${s.regions[0].to.join(',')}; windows (frame / content) ${s.windows.map((w) => `${w.name} ${w.outer.join(',')} / ${w.to.join(',')}`).join('; ')}; the frame's colours in ${s.windows.length} windows and on the map with HD on and off${s.layoutScale === 4 ? ', the command bar one to one at x4' : ''}: pass`);
}, { needs: h.haveHfes });

test('Wide layout (spec 13 Design 5, spec 14 Design 7): the pointer through the windows and the map: a move on a window\'s frame or title strip with no button reaches the nearest pixel of its content; a click on a title strip is a gap (its press and release the rest of the screen); a click on the map 15 cells east of the leader reaches the game at its cell\'s centre (MAP_X + (HALF_W + 15) 16 + 8, the leader\'s row); a click at each window\'s centre (the actor windows\' included) reaches the game pixel at its frame rect\'s centre, which shows there', async (env) => {
  const { page } = await bootWide(env);
  await recordPointer(page);
  const s = await wideShown(page);
  const at = Object.fromEntries(s.windows.map((w) => [w.name, w]));
  const nearest = (w, [px, py]) => regionPixel(w, [px, py]);
  // The rest of the screen in Wide: in none of the click map's map (Original's window and the
  // shown map), command bar and side panel; a window's rect is not near it (its arrow, 16 px
  // wide at most, shows in none; the drag test reads its pixels).
  const near = [s.rest[0] - 16, s.rest[1] - 16, 48, 48];
  assert.ok([LAYOUT_PARTS.map, LAYOUT_PARTS.commands, LAYOUT_PARTS.panel, WIDE_GEOM.shown].every((r) => !meets(r, [...s.rest, 1, 1])), 'the rest pixel: the rest of the screen');
  assert.ok(s.windows.every((w) => !meets(w.from, near)) && !meets(WIDE_GEOM.shown, near), 'no window and not the map near the rest pixel');
  // Moves with no button on a frame and on a title strip: the nearest pixel of that content.
  const moves = [
    [at.date, [at.date.outer[0] + 1, at.date.outer[1] + 1], 'the date line\'s frame, top left'],
    [at.party, [at.party.bar[0] + 40, at.party.bar[1] + 5], 'the party\'s title strip'],
    [at.doll, [at.doll.outer[0] + 1, at.doll.to[1] + 100], 'the paper doll\'s left border'],
    [at.messages, [at.messages.outer[0] + at.messages.outer[2] - 2, at.messages.to[1] + 29], 'the message scroll\'s right border'],
    [at.commands, [at.commands.to[0] + 102, at.commands.outer[1] + at.commands.outer[3] - 2], 'the command bar\'s bottom border'],
  ];
  for (const [w, p, label] of moves) {
    const n = await pointerCount(page);
    await page.mouse.move(...(await h.canvasPoint(page, p[0] + 0.5, p[1] + 0.5)));
    await page.waitForFunction((k) => window.__pointer.length > k, n, { timeout: 5000 });
    const sent = await pointerSince(page, n);
    assert.deepEqual(sent.at(-1), [...nearest(w, [p[0] + 0.5, p[1] + 0.5]), 0], `a move on ${label}: the nearest pixel of its content`);
  }
  // A click on each title strip: a gap, the rest of the screen (the original's click there).
  for (const w of s.windows) {
    await h.waitPrompt(page);
    const p = [w.bar[0] + Math.floor(w.bar[2] / 2), w.bar[1] + Math.floor(w.bar[3] / 2)];
    const sent = await canvasClick(page, p);
    const press = sent.find(([, , b]) => b === 1);
    const release = sent.findLast(([, , b]) => b === 0);
    assert.ok(press && release, `${w.name}'s title strip: the press and the release reached the game`);
    assert.deepEqual([press.slice(0, 2), release.slice(0, 2)], [s.rest, s.rest], `${w.name}'s title strip: a gap, the rest of the screen`);
  }
  // The map: the cell 15 east of the leader, at its centre, through the test's own geometry
  // (spec 13: the leader at view cell (HALF_W, HALF_H), view cell (0, 0) at frame (MAP_X,
  // MAP_Y); the shown rect scaled by 1080 / 320 from canvas x -12).
  await h.waitPrompt(page);
  const g = WIDE_GEOM;
  const want = [g.mapX + (g.halfW + 15) * 16 + 8, g.mapY + g.halfH * 16 + 8];
  const k = 1080 / g.shown[3];
  const cell = [-12 + (want[0] - g.shown[0] + 0.5) * k, (want[1] - g.shown[1] + 0.5) * k];
  const viaPage = await page.evaluate(([x, y]) => globalThis.__u6Layout.toCanvas(x, y), want);
  assert.ok(viaPage && Math.abs(viaPage[0] - cell[0]) < 1e-6 && Math.abs(viaPage[1] - cell[1]) < 1e-6, 'the page shows that cell where the geometry puts it, not under a window');
  let n = await pointerCount(page);
  await page.mouse.click(...(await h.canvasPoint(page, cell[0], cell[1])));
  await h.waitTaken(page);
  let press = (await pointerSince(page, n)).find(([, , b]) => b === 1);
  assert.deepEqual(press, [...want, 1], 'the map click 15 cells east of the leader reaches its cell\'s centre');
  // Each window's centre: the game pixel at its rect's centre, clicked at the middle of what
  // shows it (the date line and the scroll first; the actor windows and the command bar may
  // start something in the game, which this test does not wait for; the command bar last).
  const centres = ['date', 'messages', 'party', 'stats', 'doll', 'backpack', 'commands'];
  for (const name of centres) {
    const { from } = WIDE_PARTS[name];
    const centre = [from[0] + Math.floor(from[2] / 2), from[1] + Math.floor(from[3] / 2)];
    const w = at[name];
    const [cx, cy] = await page.evaluate(([x, y]) => globalThis.__u6Layout.toCanvas(x, y), centre);
    assert.ok(Math.abs(cx - (w.to[0] + w.to[2] / 2)) <= w.to[2] / from[2] && Math.abs(cy - (w.to[1] + w.to[3] / 2)) <= w.to[3] / from[3], `${name}: that pixel shows at the window's centre`);
    n = await pointerCount(page);
    await page.mouse.click(...(await h.screenPoint(page, ...centre)));
    await h.waitTaken(page);
    press = (await pointerSince(page, n)).find(([, , b]) => b === 1);
    assert.deepEqual(press, [...centre, 1], `a click at the centre of ${name} reaches its rect's centre`);
  }
  console.log(`  receipt: ${moves.length} frame and title moves at the nearest content pixel; ${s.windows.length} title-strip clicks at the rest of the screen; the map click 15 cells east at (${want.join(', ')}); ${centres.length} window centres at their rects' centres: pass`);
}, { needs: h.haveHfes });

// The windows of `shown` as spec 14 Design 6 marks them for the side panel's mode `mode` and
// whether a conversation runs (`talking`): in one of the actor views' modes, no conversation,
// WIDE_NAMES; in another or in a conversation the side panel's own after them (each at its default
// place, `windowsPlaced`); the active windows those whose view is the mode (both halves of the
// inventory), else the side panel's own; with the side panel's own the actor windows dimmed, none
// else; with `label`.
function marksFollow(shown, mode, label, talking = false) {
  const other = !ACTOR_MODES.includes(mode) || talking;
  windowsPlaced(shown, label, { other });
  for (const w of shown.windows) {
    const part = WIDE_PARTS[w.name];
    assert.equal(w.view, part.view ?? null, `${label}: ${w.name}'s view`);
    assert.equal(w.active, other ? w.name === 'other' : part.view === mode, `${label}: ${w.name} active or not`);
    assert.equal(w.dimmed, other && part.view !== undefined, `${label}: ${w.name} dimmed or not`);
  }
}

// Page script: per window shown, whether its title is its English name (`names`, by window) with
// the shown member's name (Game.panel_name) after it in the windows `named` lists, when the
// panel shows a member by name (in one of the actor views' modes); the titles kept in the page
// for `titlesKept`: booleans only (the name stays in the page).
const titlesNamed = (page, names, named) => page.evaluate(([byWindow, withName]) => {
  const member = globalThis.__shownGame.panel_name();
  window.__titlesKept = globalThis.__u6Layout.windows.map((w) => [w.name, w.title]);
  return globalThis.__u6Layout.windows.every((w) => w.title === (withName.includes(w.name) && member
    ? `${byWindow[w.name]} — ${member}` : byWindow[w.name]));
}, [names, named]);
// Page script: whether every window `titlesNamed` read last shows the same title now (another mode
// or a conversation: the actor windows keep their last views, and their titles): a boolean.
const titlesKept = (page) => page.evaluate(() => {
  const now = Object.fromEntries(globalThis.__u6Layout.windows.map((w) => [w.name, w.title]));
  return window.__titlesKept.length > 0 && window.__titlesKept.every(([name, title]) => now[name] === title);
});

// The canvas's marks in `check` (`wideCheck`, the display frozen) for the windows of `shown`: the
// active windows' border and title strip each of one colour, the others' of another, darker; the
// content of a dimmed window the frame's colours at half over its paper (the border's ink), of any
// other the frame's colours; with `label`.
function inksFollow(check, shown, label) {
  const luma = ([r, g, b]) => 0.2126 * r + 0.7152 * g + 0.0722 * b;
  const active = shown.windows.filter((w) => w.active).map((w) => check.windows[w.name]);
  const rest = shown.windows.filter((w) => !w.active).map((w) => check.windows[w.name]);
  assert.ok(active.length > 0 && rest.length > 0, `${label}: active windows and others`);
  for (const key of ['barInk', 'ringInk']) {
    const a = new Set(active.map((r) => r[key].join(',')));
    const b = new Set(rest.map((r) => r[key].join(',')));
    assert.ok(a.size === 1 && b.size === 1, `${label}: one ${key} for the active windows, one for the others`);
    assert.ok(luma(active[0][key]) > luma(rest[0][key]), `${label}: the active windows' ${key} lighter`);
  }
  for (const w of shown.windows) {
    const r = check.windows[w.name];
    assert.ok(r.ring === 1 && r.plain === 1 && r.ink > 0, `${label}: ${w.name} framed in one colour, titled on a strip of one colour`);
    if (w.dimmed) assert.ok(r.compared > 0 && r.dimDiffer === 0 && r.differ > 0, `${label}: ${w.name} dimmed, the frame's colours at half over its paper`);
    else assert.ok(r.compared > 0 && r.differ === 0, `${label}: ${w.name} the frame's colours`);
  }
}

test('Wide layout (spec 14 Design 6 and 7): the actor windows follow the side panel\'s mode as the engine reports it (Game.panel_view, read at each paint): at the prompt the mode is one of the three views and its window is the active one, no side panel window; Look on the leader (its portrait, another mode): the side panel\'s own window at its default place (top left, meeting none), active, the party, the attributes, the paper doll and the backpack dimmed, a press in the dimmed party reaching the game at its frame rect\'s pixel, a press at the side panel window\'s content centre (at the test\'s own canvas coordinates) at the frame pixel (240, 55); back at the prompt as before; Talk to a party member (a conversation, the mode the roster\'s): the side panel\'s own window shown and active, the actor windows dimmed, until the conversation ends. The mode (the page\'s read of it set to each of 0x91, 0x90, 0x92, another, and 0x91 in a conversation; the display frozen): the active window\'s border and title strip lighter, one colour among the active ones (both halves of the inventory), the others another; dimmed windows the frame\'s colours at half over their paper, the rest the frame\'s; each title its name, the shown member\'s name after it in the attributes, the doll and the backpack, none when no member is shown, in another mode and in a conversation the name of their last views\' member; the titles\' words in English and Korean (Party / 파티, Attributes / 능력치, Paper doll / 인형, Backpack / 배낭, Panel / 패널)', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const keys = ['windowParty', 'windowAttributes', 'windowDoll', 'windowBackpack', 'windowPanel'];
  assert.deepEqual(keys.map((k) => TEXT.en[k]), ['Party', 'Attributes', 'Paper doll', 'Backpack', 'Panel'], 'the English titles');
  assert.deepEqual(keys.map((k) => TEXT.ko[k]), ['파티', '능력치', '인형', '배낭', '패널'], 'the Korean titles');
  const names = Object.fromEntries(Object.entries(WIDE_PARTS).map(([name, { title }]) => [name, TEXT.en[title]]));
  const named = Object.entries(WIDE_PARTS).filter(([, p]) => p.named).map(([name]) => name);
  const errors = [];
  const { page } = await bootWide(env);
  page.on('pageerror', () => errors.push(1));
  const panel = () => page.evaluate(() => [[...globalThis.__shownGame.panel_view()], globalThis.__u6Layout.panel]);
  // At the prompt: one of the three views, as the engine has it.
  let [real, read] = await panel();
  assert.ok(real.length === 4 && ACTOR_MODES.includes(real[0]) && real[3] === 0, 'at the prompt the panel shows one of the three views, no conversation');
  assert.deepEqual(read, real, 'the page read the engine\'s view');
  const start = real[0];
  let s = await wideShown(page);
  marksFollow(s, start, 'at the prompt');
  assert.ok(await titlesNamed(page, names, named), 'at the prompt: the titles');
  // Look on the leader: the portrait (another mode) until a key or a click.
  const look = async () => {
    await page.focus('#screen');
    await page.keyboard.press('l');
    await h.waitTaken(page);
    await page.keyboard.press('Enter');
    await page.waitForFunction((three) => {
      const p = globalThis.__u6Layout.panel;
      return p !== null && p.length === 4 && !three.includes(p[0]);
    }, ACTOR_MODES, { timeout: 30_000 });
  };
  await look();
  [real, read] = await panel();
  assert.deepEqual(read, real, 'Look: the page read the engine\'s view');
  s = await wideShown(page);
  marksFollow(s, real[0], 'Look');
  assert.ok(await titlesKept(page), 'Look: the actor windows\' titles as at the prompt');
  // A press in the dimmed party: the game pixel there, as any press (the engine ignores it).
  await recordPointer(page);
  const party = WIDE_PARTS.party.from;
  const centre = [party[0] + Math.floor(party[2] / 2), party[1] + Math.floor(party[3] / 2)];
  let n = await pointerCount(page);
  await page.mouse.click(...(await h.screenPoint(page, ...centre)));
  await h.waitTaken(page);
  const press = (await pointerSince(page, n)).find(([, , b]) => b === 1);
  assert.deepEqual(press, [...centre, 1], 'a press in the dimmed party reaches its pixel');
  // A press at the side panel's own window's content centre (the portrait again should that press
  // have closed it), at canvas coordinates of the test's own: the window at (WIDE_EDGE, WIDE_EDGE),
  // its content WIDE_BORDER in and under a title strip WIDE_TITLE tall, at its size over its rect;
  // the centre of what shows the rect's centre pixel, (168 + 72, 6 + 49).
  await h.waitTaken(page);
  if (!(await page.evaluate(() => globalThis.__u6Layout.windows.some((w) => w.name === 'other')))) await look();
  const other = WIDE_PARTS.other;
  const k = other.size[0] / other.from[2];
  const want = [240, 55];
  const point = [WIDE_EDGE + WIDE_BORDER + (want[0] - other.from[0] + 0.5) * k,
    WIDE_EDGE + WIDE_BORDER + WIDE_TITLE + (want[1] - other.from[1] + 0.5) * k];
  const shownAt = await page.evaluate(([x, y]) => globalThis.__u6Layout.toCanvas(x, y), want);
  assert.ok(shownAt && Math.abs(shownAt[0] - point[0]) < 1e-6 && Math.abs(shownAt[1] - point[1]) < 1e-6, 'the side panel\'s centre pixel shows where the test puts it');
  n = await pointerCount(page);
  await page.mouse.click(...(await h.canvasPoint(page, ...point)));
  await h.waitTaken(page);
  const panelPress = (await pointerSince(page, n)).find(([, , b]) => b === 1);
  assert.deepEqual(panelPress, [...want, 1], 'a press at the side panel\'s own window\'s centre reaches the frame pixel there');
  // The portrait closed (by that press, else a key), back at the prompt.
  await h.waitTicks(page, 30);
  if (await promptNow(page) !== '1') await page.keyboard.press('Space');
  await h.waitPrompt(page);
  await page.waitForFunction((three) => three.includes(globalThis.__u6Layout.panel?.[0]), ACTOR_MODES, { timeout: 30_000 });
  [real] = await panel();
  s = await wideShown(page);
  marksFollow(s, real[0], 'back at the prompt');
  assert.ok(await titlesNamed(page, names, named), 'back at the prompt: the titles');
  // Talk to the party's second member (the real engine): the conversation flag, whatever the mode;
  // the side panel's own window while it runs. Ended at its line by an empty one (the game's
  // goodbye), a key at each page of text before it.
  await page.focus('#screen');
  await page.keyboard.press('t');
  await h.waitTaken(page);
  await page.keyboard.press('2');
  await page.waitForFunction(() => globalThis.__u6Layout.panel?.[3] === 1, null, { timeout: 30_000 });
  [real, read] = await panel();
  assert.deepEqual(read, real, 'Talk: the page read the engine\'s view');
  s = await wideShown(page);
  marksFollow(s, real[0], `Talk (the mode ${ACTOR_MODES.includes(real[0]) ? 'one of the three' : 'another'})`, true);
  assert.ok(await titlesKept(page), 'Talk: the actor windows\' titles as before it (their last views\' member)');
  const talkMode = real[0];
  // The conversation ended by its state: each key only once the game took the one before (nothing
  // queued) and waits for input: at the line it reads (it owns every key) an empty line, the
  // game's goodbye; reading no line for 100 of its steps, a page of the scroll held for a key:
  // Space.
  const talking = () => page.evaluate(() => globalThis.__shownGame.panel_view()[3] === 1);
  for (let i = 0; i < 20 && await talking(); i += 1) {
    await h.waitTaken(page);
    const from = await page.evaluate(() => globalThis.__ticks);
    await page.waitForFunction((t0) => globalThis.__shownGame.panel_view()[3] !== 1
      || (globalThis.__owned === 7 && document.getElementById('screen').dataset.keys === '0')
      || globalThis.__ticks >= t0 + 100, from, { timeout: 60_000, polling: 'raf' });
    if (!(await talking())) break;
    await page.keyboard.press(await page.evaluate(() => globalThis.__owned === 7) ? 'Enter' : 'Space');
  }
  assert.equal(await talking(), false, 'the conversation ended');
  await h.waitPrompt(page);
  await page.waitForFunction((three) => {
    const p = globalThis.__u6Layout.panel;
    return p?.[3] === 0 && three.includes(p[0]);
  }, ACTOR_MODES, { timeout: 30_000 });
  [real] = await panel();
  s = await wideShown(page);
  marksFollow(s, real[0], 'the conversation over');
  // Each mode through the page's read (the engine's own view left as it is), the display frozen:
  // the canvas's marks and the titles (a member's name the test's own).
  await h.virtualDisplay(page, 60);
  const views = [
    [0x91, 0, 0, 'Qa', 'the roster'],
    [0x90, 1, 0, 'Qb', 'the stats'],
    [0x92, 2, 0, 'Qc', 'the inventory'],
    [0x9e, 1, 0, 'Qd', 'another mode'],
    [0x91, 1, 1, 'Qe', 'a conversation'],
    [0x90, -1, 0, '', 'the stats, no member'],
  ];
  let dimCompared = [];
  let slotsName = '';
  for (const [mode, member, talking, name, label] of views) {
    await page.evaluate(([v, who]) => {
      const g = globalThis.__shownGame;
      g.panel_view = () => Int32Array.from(v);
      g.panel_name = () => who;
    }, [[mode, member, 3, talking], name]);
    await h.runFrames(page, 4);
    assert.deepEqual(await page.evaluate(() => globalThis.__u6Layout.panel), [mode, member, 3, talking], `${label}: the page read it`);
    s = await wideShown(page);
    marksFollow(s, mode, label, talking === 1);
    // The actor windows' titles: the name read in the last of their views (kept in another mode
    // and in a conversation, with the views).
    if (ACTOR_MODES.includes(mode) && talking === 0) slotsName = name;
    const titled = await page.evaluate(() => Object.fromEntries(globalThis.__u6Layout.windows.map((w) => [w.name, w.title])));
    for (const w of Object.keys(WIDE_PARTS).filter((k) => titled[k] !== undefined)) {
      assert.equal(titled[w], named.includes(w) && slotsName ? `${names[w]} — ${slotsName}` : names[w], `${label}: ${w}'s title`);
    }
    inksFollow(await wideCheck(page, true), s, label);
    if (s.windows.some((w) => w.dimmed)) dimCompared = s.windows.filter((w) => w.dimmed).map((w) => w.name);
  }
  // The dimmed windows compared with the frame: all four actor windows.
  assert.deepEqual(dimCompared, ['party', 'stats', 'doll', 'backpack'], 'the dimmed windows compared');
  // The engine's own view again.
  await page.evaluate(() => {
    const g = globalThis.__shownGame;
    delete g.panel_view;
    delete g.panel_name;
  });
  await h.runFrames(page, 4);
  s = await wideShown(page);
  marksFollow(s, real[0], 'the engine\'s view again');
  inksFollow(await wideCheck(page, true), s, 'the engine\'s view again');
  assert.equal(errors.length, 0, 'no page error');
  console.log(`  receipt: at the prompt one of the three views, its window active; Look (another mode): the side panel's own window shown and active, 4 actor windows dimmed, a press in the dimmed party at its pixel, one at the side panel window's centre at (240, 55); Talk (the real engine, the mode ${ACTOR_MODES.includes(talkMode) ? 'one of the three' : 'another'}): the side panel's own window shown and active until the conversation ended; ${views.length} views through the page's read: the active mark lighter, the dimmed windows at half, the titles with the member's name: pass`);
}, { needs: h.haveHfes });

// Page script, the display frozen by the caller: the canvas of the letterboxed screen against the
// frame's rect `crop` (frame pixels) scaled into the canvas rect `rect`, by the test's own
// arithmetic: at a grid of canvas points over `rect` (every 9th pixel, and the rect's first and
// last two columns and rows), the frame pixel the point's centre falls in. `hd`: the HD frame
// (scaled smoothly: only points whose 3 x 3 frame pixels round it are one colour, within 2 a
// channel); else the classic frame (pixelated: every point not within 0.15 of a frame pixel's
// edge, exactly). The pointer's place is left out (`pointerAreaHook`). Counts only: `compared`,
// `lit` (not black), `differ`.
const wholeCheck = async (page, crop, rect, hd) => (await pointerAreaHook(page), page.evaluate(([[fx, fy, fw, fh], [x, y, w, hh], onHd]) => {
  const c = document.getElementById('screen');
  const g = globalThis.__shownGame;
  const s = onHd ? g.hd_scale() : 1;
  const f = onHd ? g.hd_frame_rgba() : g.frame_rgba();
  const [W, H] = [g.width(), g.height()];
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  const fr = (gx, gy) => [0, 1, 2].map((k) => f[((gy * s) * W * s + gx * s) * 4 + k]);
  const flat = (gx, gy) => {
    const ref = fr(gx, gy);
    for (let v = (gy - 1) * s; v < (gy + 2) * s; v += 1) {
      for (let u = (gx - 1) * s; u < (gx + 2) * s; u += 1) {
        if (u < 0 || v < 0 || u >= W * s || v >= H * s) return null;
        const i = (v * W * s + u) * 4;
        if (f[i] !== ref[0] || f[i + 1] !== ref[1] || f[i + 2] !== ref[2]) return null;
      }
    }
    return ref;
  };
  const cols = new Set([x, x + 1, x + w - 2, x + w - 1]);
  const rows = new Set([y, y + 1, y + hh - 2, y + hh - 1]);
  for (let v = x + 4; v < x + w; v += 9) cols.add(v);
  for (let v = y + 4; v < y + hh; v += 9) rows.add(v);
  const r = { compared: 0, lit: 0, differ: 0 };
  const pointer = window.__pointerArea();
  for (const cy of rows) {
    for (const cx of cols) {
      if (window.__inPointer(pointer, cx, cy)) continue;
      const [u, v] = [((cx + 0.5 - x) * fw) / w, ((cy + 0.5 - y) * fh) / hh];
      if (!onHd && [u, v].some((q) => q - Math.floor(q) < 0.15 || q - Math.floor(q) > 0.85)) continue;
      const [gx, gy] = [fx + Math.floor(u), fy + Math.floor(v)];
      if (gx < 0 || gy < 0 || gx >= W || gy >= H) continue;
      const want = onHd ? flat(gx, gy) : fr(gx, gy);
      if (!want) continue;
      const i = (cy * c.width + cx) * 4;
      r.compared += 1;
      if (want[0] + want[1] + want[2] > 0) r.lit += 1;
      if ([0, 1, 2].some((k) => Math.abs(d[i + k] - want[k]) > (onHd ? 2 : 0))) r.differ += 1;
    }
  }
  return r;
}, [crop, rect, hd]));

test('Wide layout (spec 13 Design 5): every other screen (the opening, the menu) shows the UI\'s 320 x 200 scaled to the canvas\'s height, 1728 x 1080 centred, the sides dark; a press lands at the pixel it shows, on a side at the nearest edge; the page\'s map shows no frame pixel outside the UI\'s 320 x 200 there; in the menu, the display frozen, the canvas inside the letterbox is the frame\'s 320 x 200 crop scaled by 1080 / 200 (the test\'s own arithmetic), with HD off pixel for pixel (edges included) and with HD on where the frame is flat; the same check with the whole frame as the source or another scale finds pixels that differ', async (env) => {
  const { page } = await h.openWide(env, { gameFirst: false });
  await h.load(page, env);
  await h.dropFiles(page, [h.ZIP]);
  await started(page);
  await h.waitProgram(page, 'u');
  await h.waitTicks(page, 60);
  const s = await wideShown(page);
  assert.ok(s.layout === 'wide' && s.view === 'whole', 'the opening: the UI\'s screen');
  assert.deepEqual(s.size, WIDE_CANVAS, 'on the 1920 x 1080 canvas');
  assert.deepEqual(s.whole, [96, 0, 1728, 1080], 'scaled to the canvas\'s height, centred');
  const sides = await page.evaluate(([x, w]) => {
    const c = document.getElementById('screen');
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
    const first = `${d[0]},${d[1]},${d[2]}`;
    let off = 0;
    for (let y = 0; y < c.height; y += 3) {
      for (const [a, b] of [[0, x], [x + w, c.width]]) {
        for (let px = a; px < b; px += 3) {
          const i = (y * c.width + px) * 4;
          if (`${d[i]},${d[i + 1]},${d[i + 2]}` !== first) off += 1;
        }
      }
    }
    return { off, dark: d[0] + d[1] + d[2] < 3 * 48 };
  }, [s.whole[0], s.whole[2]]);
  assert.ok(sides.off === 0 && sides.dark, 'the sides dark');
  await recordPointer(page);
  for (const [gx, gy] of [[160, 100], [0, 0], [319, 199]]) {
    const mark = await pointerCount(page);
    await page.mouse.click(...(await h.screenPoint(page, gx, gy)));
    await h.waitTaken(page);
    assert.deepEqual((await pointerSince(page, mark)).find(([, , b]) => b === 1), [gx, gy, 1], `a press at (${gx}, ${gy}) lands there`);
  }
  const sent = await canvasClick(page, [20, 500]);
  assert.deepEqual(sent.find(([, , b]) => b === 1).slice(0, 2), [0, Math.floor(((500.5 - 0) * 200) / 1080)], 'a press on the left side: the nearest edge pixel');
  assert.equal((await h.state(page)).program, 'u', 'still in u');
  // The page's map shows only the UI's 320 x 200 here: no canvas place for a frame pixel past it
  // (the frame's last pixel: its size read from the engine).
  const places = await page.evaluate(([fw, fh]) => [[0, 0], [319, 199], [320, 0], [0, 200], [319, 200], [fw - 1, fh - 1], [-1, 5], [5, -1]]
    .map(([x, y]) => globalThis.__u6Layout.toCanvas(x, y)), s.frame);
  assert.ok(places[0] && places[1], 'the UI\'s corner pixels show');
  assert.ok(places.slice(2).every((p) => p === null), 'no place for a frame pixel outside the UI\'s 320 x 200');
  // The menu, frozen: the letterbox is the UI's crop scaled, by the test's own arithmetic.
  await page.focus('#screen');
  await h.press(page, 'Escape', 120);
  assert.equal((await h.state(page)).program, 'u', 'Escape: the menu');
  const k = 1080 / 200;
  const rect = [(1920 - 320 * k) / 2, 0, 320 * k, 1080];
  assert.deepEqual(rect, s.whole, 'the letterbox where the test puts it');
  await page.click('#settings');
  await page.click('#hd');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 3);
  const results = {};
  for (const [hd, label] of [[false, 'HD off'], [true, 'HD on']]) {
    if (hd) {
      await page.click('#settings');
      await page.click('#hd');
      await page.click('#setup-ok');
      await h.runFrames(page, 3);
    }
    assert.equal(await page.evaluate(() => globalThis.__shownGame.hd_on()), hd, `${label}: as switched`);
    const right = await wholeCheck(page, [0, 0, 320, 200], rect, hd);
    assert.ok(right.compared >= (hd ? 200 : 2000) && right.lit >= (hd ? 50 : 500), `${label}: the menu offers lit pixels to compare`);
    assert.equal(right.differ, 0, `${label}: the letterbox is the frame's 320 x 200 crop scaled by 1080 / 200`);
    const wrongCrop = await wholeCheck(page, [0, 0, ...s.frame], rect, hd);
    const wrongScale = await wholeCheck(page, [0, 0, 320, 200], [96, 0, 1600, 1000], hd);
    assert.ok(wrongCrop.differ > 0 && wrongScale.differ > 0, `${label}: the check tells the whole frame or another scale from it`);
    results[label] = [right, wrongCrop, wrongScale];
  }
  console.log(`  receipt: the opening shows the UI's screen at 96,0,1728,1080 with dark sides; 4 presses landed; frame pixels outside the UI have no canvas place; the menu's letterbox the UI crop scaled (the grid with the rect's first and last two columns and rows): ${Object.entries(results).map(([l, [r, a, b]]) => `${l} ${r.compared} compared, 0 differ (whole frame as source: ${a.differ} differ; scale 5: ${b.differ} differ)`).join('; ')}: pass`);
}, { needs: h.haveZip });

test('Wide layout (spec 13 A5): fullscreen keeps 16:9 at any size, centred on black, the canvas still 1920 x 1080: at 1920 x 1080 the whole screen, sharp (one to one); at 2560 x 1440 the whole screen, smooth; at 1728 x 1117 1728 x 972 with bars, smooth; a key reaches the game in each', async (env) => {
  const { page } = await bootWide(env);
  const sizes = [[1920, 1080, 'sharp'], [2560, 1440, 'smooth'], [1728, 1117, 'smooth']];
  const lines = [];
  for (const [w, hh, want] of sizes) {
    await page.setViewportSize({ width: w, height: hh });
    await page.click('#fullscreen');
    await waitFull(page, 'on');
    await page.waitForFunction(([vw]) => Math.abs(document.getElementById('screen').getBoundingClientRect().width - Math.min(vw, (innerHeight * 16) / 9)) < 1, [w], { timeout: 10_000 });
    await h.waitFrames(page, 2);
    const s = await layoutShown(page);
    const [bw, bh] = s.box;
    assert.ok(Math.abs(bw / bh - 16 / 9) < 0.01, `${w}x${hh}: 16:9`);
    assert.ok(bw <= w + 0.5 && bh <= hh + 0.5 && (Math.abs(bw - w) < 1 || Math.abs(bh - hh) < 1), `${w}x${hh}: as large as fits`);
    assert.ok(Math.abs(s.at[0] - (w - bw) / 2) < 1 && Math.abs(s.at[1] - (hh - bh) / 2) < 1, `${w}x${hh}: centred`);
    assert.deepEqual(s.size, WIDE_CANVAS, `${w}x${hh}: the canvas stays 1920 x 1080`);
    const mode = SHARP.includes(s.rendering) ? 'sharp' : s.rendering === 'auto' ? 'smooth' : s.rendering;
    assert.equal(mode, want, `${w}x${hh}: ${want}`);
    const before = (await glue(page)).sent;
    await h.press(page, 'Space', 2);
    const g = await glue(page);
    assert.ok(g.sent === before + 1 && g.lastSent === ' ', `${w}x${hh}: a key reaches the game`);
    lines.push(`${w}x${hh}: box ${bw}x${bh}, ${mode}`);
    await page.mouse.move(200, 200);
    await page.mouse.move(220, 210);
    await page.click('#fullscreen-exit');
    await waitFull(page, 'off');
    await h.waitPrompt(page);
  }
  console.log(`  receipt: ${lines.join('; ')}; a key reached the game in each: pass`);
}, { needs: h.haveHfes });

// A timing: it needs a lightly loaded runner. On a machine busy with other work the median grows
// with the load (three runs of 60 are taken and the least kept, which only the load raises), and
// a failure prints the medians measured: read them against the machine's load before choosing 2.
test('Wide layout (spec 13 Design 6, spec 14 Design 1): the compositor\'s time per frame at S 4 on a game frame (a whole composition of the Wide frame, its size read from the engine: spec 14\'s 912 x 408, the median of 60, the least of up to three runs of 60: the machine\'s load only adds; a lightly loaded runner) and the scale the Wide layout asks: while it asks 4 that median is within 8 ms (above, it is to ask 2); asking 2, the median is printed (receipt, with the frame\'s size)', async (env) => {
  // The page's own frames run between the runs (the display stays real: a virtual one would
  // stop the clock the runs read).
  const { page } = await bootWide(env);
  const { layoutScale: scale, frame } = await wideShown(page);
  assert.deepEqual(frame, WIDE_FRAME, 'spec 14\'s frame composed');
  const medians = [];
  for (let run = 0; run < 3; run += 1) {
    medians.push(await page.evaluate(() => {
      const g = globalThis.__shownGame;
      const times = [];
      for (let i = 0; i < 60; i += 1) {
        g.set_hd(true, 2);
        const t0 = performance.now();
        g.set_hd(true, 4);
        times.push(performance.now() - t0);
      }
      times.sort((a, b) => a - b);
      return (times[29] + times[30]) / 2;
    }));
    if (Math.min(...medians) <= 8) break;
  }
  await page.evaluate((s) => globalThis.__shownGame.set_hd(true, s), scale);
  await h.waitFrames(page, 2);
  const median = Math.min(...medians);
  assert.ok(median > 0, 'a time measured');
  const shown = await wideShown(page);
  assert.equal(shown.scale, scale, 'the game composes at the layout\'s scale');
  assert.ok([2, 4].includes(scale), 'the Wide layout asks 2 or 4');
  // One-sided (brief W5 item 5): S 4 holds while the median is within 8 ms; S 2, once chosen,
  // needs no time (the receipt says the median at S 4).
  const runs = medians.map((v) => v.toFixed(2)).join(', ');
  if (scale === 4) assert.ok(median <= 8, `S 4: the median ${median.toFixed(2)} ms (runs: ${runs}) within 8 ms (above it on a lightly loaded runner, the Wide layout is to ask 2: site/app.js WIDE_S)`);
  console.log(`  receipt: the compositor at S 4, a whole composition of the Wide frame (${frame.join(' x ')}) on a game frame: median ${median.toFixed(2)} ms (runs: ${medians.map((m) => m.toFixed(2)).join(', ')}); the Wide layout asks S ${scale}`);
}, { needs: h.haveHfes });

// Waits for the page's next load after `act` (a choice that loads the page again), without
// waiting on the old document.
const loadAfter = (page, act) => Promise.all([page.waitForEvent('load', { timeout: 60_000 }), act()]);

test('Wide layout (spec 13 Decision 2): the chooser\'s and Settings\' "Screen layout" offers Wide (first), HD 16:9 and Original 16:10, in the page\'s language, Wide checked in the Wide build; after a Save in the Wide build, HD 16:9 is only noted (no load, the radio on it) until OK, which keeps hd and loads the page again with the Original build (site/pkg) and spec 10\'s layout, the kept files and the stored save as they were (nothing new saved) and the save resumed; Wide again (at OK) loads the Wide build, the save resumed there too; with storage that throws, Wide from the Original build at OK keeps the page as it is (the radio back on HD 16:9) and says why', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const { page, glues } = await bootWide(env);
  const choice = () => page.evaluate(() => ({
    labels: [...document.querySelectorAll('#layout-choice label')].map((l) => l.textContent.replace(/\s+/g, ' ').trim()),
    values: [...document.querySelectorAll('input[name="layout"]')].map((i) => [i.value, i.checked]),
  }));
  await page.click('#settings');
  let c = await choice();
  assert.deepEqual(c.labels, [TEXT.en.layoutWide, 'HD 16:9', TEXT.en.layoutOriginal], 'its words in English, Wide first');
  assert.deepEqual(c.values, [['wide', true], ['hd', false], ['original', false]], 'Wide checked');
  await page.check('input[name="setup-lang"][value="ko"]');
  c = await choice();
  assert.deepEqual(c.labels, [TEXT.ko.layoutWide, 'HD 16:9', TEXT.ko.layoutOriginal], 'its words in Korean');
  assert.equal(TEXT.ko.layoutWide, '와이드 (넓은 지도)', 'the Korean name of Wide');
  await page.check('input[name="setup-lang"][value="en"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await page.focus('#screen');
  const { moved } = await walkAndSave(page);
  await page.click('#settings');
  const keptBefore = await h.keptPaths(page);
  const savedBefore = await h.savedFiles(page);
  const n = glues.length;
  // The choice is noted; OK loads the page again.
  let loads = 0;
  page.on('load', () => { loads += 1; });
  await page.check('input[name="layout"][value="hd"]');
  await h.waitFrames(page, 10);
  assert.equal(loads, 0, 'HD 16:9 chosen: no load before OK');
  assert.equal((await choice()).values.find(([v]) => v === 'hd')[1], true, 'the radio on HD 16:9 until OK');
  await loadAfter(page, () => page.click('#setup-ok', { noWaitAfter: true }));
  await started(page, FILES);
  await h.waitPrompt(page);
  let s = await layoutShown(page);
  assert.ok(glues.length > n && glues.slice(n).every((p) => p === '/pkg/u6_web.js'), 'the page loaded again with the Original build\'s glue');
  assert.ok(s.layout === 'hd' && s.view === 'game' && s.stored === 'hd' && s.size[0] === 1280 && s.size[1] === 720 && s.scale === 4, 'HD 16:9: spec 10\'s layout, kept');
  assert.equal(await page.evaluate(() => globalThis.__u6Layout.wide), false, 'not the Wide layout');
  assert.deepEqual(await h.keptPaths(page), keptBefore, 'the kept files as they were');
  h.assertSame(await h.savedFiles(page), savedBefore, 'the stored save as it was: nothing new saved');
  assert.ok(savedBefore.length > 1, 'a save was stored');
  assert.equal((await atPrompt(page)).leader, moved, 'the save resumes in the Original build');
  // Original 16:10 within the Original build: at once, no new load.
  const m = glues.length;
  await page.click('#settings');
  await page.check('input[name="layout"][value="original"]');
  s = await layoutShown(page);
  assert.ok(s.layout === 'original' && s.view === 'frame' && s.stored === 'original' && glues.length === m, 'Original 16:10 at once, in the same build');
  // Wide again (at OK): the Wide build.
  await page.check('input[name="layout"][value="wide"]');
  await loadAfter(page, () => page.click('#setup-ok', { noWaitAfter: true }));
  await started(page, FILES);
  await h.waitPrompt(page);
  const w = await wideShown(page);
  assert.ok(w.layout === 'wide' && w.wide && w.size[0] === 1920 && glues.slice(m).every((p) => p === '/pkg-wide/u6_web.js'), 'Wide: the Wide build again');
  assert.equal((await atPrompt(page)).leader, moved, 'the save resumes in the Wide build');
  h.assertSame(await h.savedFiles(page), savedBefore, 'the stored save still as it was');
  await page.context().close();
  // Storage that throws: the Original build (HD 16:9); Wide cannot be kept across the load it
  // needs, so nothing happens but a line.
  const { context, page: p2 } = await h.openPage(env, { layout: null });
  await context.addInitScript(layoutStorageThrows);
  await h.load(p2, env);
  await p2.setInputFiles('#picker', h.HFES);
  await started(p2, FILES);
  await h.waitPrompt(p2);
  let reloads = 0;
  p2.on('load', () => { reloads += 1; });
  await p2.click('#settings');
  await p2.click('input[name="layout"][value="wide"]');
  // No voices: storage that throws keeps no choice, so the chooser is a first run's (NPC voices
  // ticked), and their download's line would follow.
  await p2.uncheck('#setup-voices');
  await p2.click('#setup-ok');
  await h.waitSettled(p2);
  await h.waitFrames(p2, 10);
  const after = await p2.evaluate(() => ({
    layout: document.body.dataset.layout,
    checked: document.querySelector('input[name="layout"]:checked')?.value,
    status: document.getElementById('status').textContent,
    lang: document.documentElement.lang,
  }));
  assert.ok(reloads === 0 && after.layout === 'hd' && after.checked === 'hd', 'storage that throws: no load, HD 16:9 still, the radio back');
  // The page's language: 한글 here (storage that throws keeps no chooser's choice: a first run).
  assert.equal(after.status, TEXT[after.lang].layoutNotKept, 'the line says why, in the page\'s language');
  console.log('  receipt: Wide, HD 16:9, Original 16:10 in English and Korean; a Save in Wide, then Wide -> HD 16:9 noted, loaded at OK into the Original build with spec 10\'s layout, kept files and save unchanged, the save resumed; Original 16:10 at once; HD -> Wide at OK loaded the Wide build, the save resumed; storage that throws: no load, a line: pass');
}, { needs: h.haveHfes });

test('Wide layout (spec 13 Decision 2, the first-run chooser): on a first visit (the chooser open on the Wide build) English, NPC voices unticked and HD 16:9 load nothing until OK; OK keeps the chooser\'s choices first and then loads the page once into the Original build: in English, the chooser not shown again, the voices off and nothing asked of /models/, HD 16:9 kept', async (env) => {
  const { context, page } = await h.openPage(env, { setup: null, layout: null });
  const glues = [];
  const models = [];
  page.on('request', (r) => {
    const path = new URL(r.url()).pathname;
    if (path.endsWith('/u6_web.js')) glues.push(path);
    if (path.startsWith('/models/')) models.push(path);
  });
  let loads = 0;
  page.on('load', () => { loads += 1; });
  await h.load(page, env);
  await page.waitForFunction(() => !document.getElementById('setup').hidden);
  assert.ok(glues.length === 1 && glues[0] === '/pkg-wide/u6_web.js', 'a first visit: the Wide build');
  await page.check('input[name="setup-lang"][value="en"]');
  await page.uncheck('#setup-voices');
  await page.check('input[name="layout"][value="hd"]');
  await h.waitFrames(page, 10);
  assert.equal(loads, 1, 'no load before OK');
  await loadAfter(page, () => page.click('#setup-ok', { noWaitAfter: true }));
  await h.waitSettled(page);
  await h.waitFrames(page, 20);
  const after = await page.evaluate(() => ({
    lang: document.documentElement.lang,
    open: !document.getElementById('setup').hidden,
    layout: document.body.dataset.layout,
    wide: globalThis.__u6Layout.wide,
    stored: localStorage.getItem('u6.layout'),
    setup: JSON.parse(localStorage.getItem('u6.setup')),
    voices: localStorage.getItem('u6.voices'),
    voice: document.body.dataset.voice ?? null,
  }));
  assert.equal(loads, 2, 'one load at OK');
  assert.deepEqual(glues, ['/pkg-wide/u6_web.js', '/pkg/u6_web.js'], 'then the Original build');
  assert.ok(after.layout === 'hd' && !after.wide && after.stored === 'hd', 'HD 16:9, kept');
  assert.ok(after.lang === 'en' && after.setup?.lang === 'en', 'in English, kept');
  assert.equal(after.open, false, 'the chooser not shown again');
  assert.ok(after.voices !== 'on' && models.length === 0, 'the voices off, nothing asked of /models/');
  await context.close();
  console.log(`  receipt: a first visit chose English, no voices and HD 16:9; one load at OK into the Original build, the chooser's choices kept, voices off (voice state ${after.voice}): pass`);
});

// Before the page's scripts: a voice Worker that runs no model (as fakeVoiceWorker) and keeps,
// in the tab's session storage across its documents, each load it was asked for
// (`__voiceLoads`: [the document's number from 1, whether from the cache only]) and each voice
// state the page showed (`__voiceStates`). From the cache only it answers as the real worker does
// when the model is not there (notcached); with downloads allowed, in the tab's first document a
// download that goes on (progress, never ready), as the 400 MB model's would; in a later one,
// ready. `window.__voiceLog()` reads both lists. With `sessionThrows`, the page's sessionStorage
// throws on access (as in some privacy modes; localStorage works): the log keeps the tab's real
// one, taken before. With `cacheSilent`, a load from the cache only gets no answer (as one still
// reading the cache): its tab does not fail, so it does not turn the voices off for the site.
function voiceLoadsLog({ sessionThrows = false, cacheSilent = false } = {}) {
  const store = sessionStorage;
  if (sessionThrows) {
    Object.defineProperty(window, 'sessionStorage', {
      configurable: true,
      get() {
        throw new DOMException('test: session storage refused', 'SecurityError');
      },
    });
  }
  const list = (key) => JSON.parse(store.getItem(key) ?? '[]');
  window.__voiceLog = () => ({ loads: list('__voiceLoads'), states: list('__voiceStates') });
  const doc = Number(store.getItem('__voiceDocs') ?? 0) + 1;
  store.setItem('__voiceDocs', String(doc));
  window.Worker = class FakeVoiceWorker extends EventTarget {
    postMessage(m) {
      const reply = (data) => setTimeout(() => {
        const e = new MessageEvent('message', { data });
        this.dispatchEvent(e);
        this.onmessage?.(e);
      }, 0);
      if (m?.type === 'load') {
        store.setItem('__voiceLoads', JSON.stringify([...list('__voiceLoads'), [doc, Boolean(m.cachedOnly)]]));
        if (m.cachedOnly) {
          if (!cacheSilent) reply({ type: 'error', code: 'notcached' });
        } else if (doc === 1) reply({ type: 'progress', got: 1 << 20, total: 400 << 20 });
        else reply({ type: 'ready', ms: 1, refetched: 0, path: 'cpu' });
      } else if (m?.type === 'speak') reply({ type: 'done', id: m.id });
    }

    terminate() {}
  };
  document.addEventListener('DOMContentLoaded', () => {
    const note = () => {
      const states = list('__voiceStates');
      const v = document.body.dataset.voice;
      if (v !== undefined && states.at(-1) !== v) store.setItem('__voiceStates', JSON.stringify([...states, v]));
    };
    note();
    new MutationObserver(note).observe(document.body, { attributes: true, attributeFilter: ['data-voice'] });
  });
}

test('Wide layout (spec 13 Decision 2, the first-run chooser with voices): NPC voices ticked and HD 16:9 at the first run (the model stubbed: a download that goes on), OK starts the download and loads the page once into the Original build; there the voices go on with downloads allowed (not from the cache only) and are ready, the voices kept on; no state failed, no load from the cache only; the mark the switch left is taken', async (env) => {
  const { context, page } = await h.openPage(env, { setup: null, layout: null });
  await context.addInitScript(voiceLoadsLog);
  let loads = 0;
  page.on('load', () => { loads += 1; });
  await h.load(page, env);
  await page.waitForFunction(() => !document.getElementById('setup').hidden);
  await page.check('input[name="setup-lang"][value="en"]');
  assert.equal(await page.isChecked('#setup-voices'), true, 'NPC voices ticked (a first run)');
  await page.check('input[name="layout"][value="hd"]');
  await loadAfter(page, () => page.click('#setup-ok', { noWaitAfter: true }));
  await h.waitSettled(page);
  await page.waitForFunction(() => document.body.dataset.voice === 'ready', null, { timeout: 30_000 });
  await h.waitFrames(page, 10);
  const after = await page.evaluate(() => ({
    loads: JSON.parse(sessionStorage.getItem('__voiceLoads') ?? '[]'),
    states: JSON.parse(sessionStorage.getItem('__voiceStates') ?? '[]'),
    mark: sessionStorage.getItem('u6-voice-download-after-reload'),
    voices: localStorage.getItem('u6.voices'),
    layout: document.body.dataset.layout,
    wide: globalThis.__u6Layout.wide,
    error: document.body.dataset.voiceError,
  }));
  assert.equal(loads, 2, 'one load at OK');
  assert.ok(after.layout === 'hd' && !after.wide, 'the Original build, HD 16:9');
  // The first document's load may not be asked yet when the page loads again (the storage's
  // persist is asked first): the second's goes on with downloads allowed either way.
  assert.ok(after.loads.every(([, cached]) => !cached), 'no load from the cache only');
  assert.ok(after.loads.some(([d]) => d === 2), 'the page loaded again asked for the model, downloads allowed');
  assert.ok(!after.states.includes('failed') && after.error === '', 'never failed');
  assert.equal(after.states.at(-1), 'ready', 'the voices ready');
  assert.equal(after.voices, 'on', 'the voices kept on');
  assert.equal(after.mark, null, 'the mark taken');
  await context.close();
  console.log(`  receipt: voices ticked at the first run with HD 16:9: the voices started, one load at OK, then the model asked for with downloads allowed (loads ${after.loads.map(([d, c]) => `${d}:${c ? 'cache' : 'download'}`).join(', ')}), ready, kept on (states ${after.states.join(' > ')}): pass`);
});

// The status line and the chooser's line, and the chooser's button as it reads.
const switchLines = (page) => page.evaluate(() => ({
  lines: [document.getElementById('status').textContent, document.getElementById('setup-status').textContent],
  ok: document.getElementById('setup-ok').textContent,
  lang: document.documentElement.lang,
}));

test('Wide layout (spec 13 Decision 2, the switch line names the button): on a first visit (the Wide build, the chooser in Korean, NPC voices ticked), a choice across the builds shows its line on the status line and the chooser\'s naming the button as it reads ("Accept and start" while NPC voices is ticked, else "OK"); unticking and ticking NPC voices names the button anew, in Korean and in English, and so does a change of language', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const { context, page } = await h.openPage(env, { setup: null, layout: null });
  await h.load(page, env);
  await page.waitForFunction(() => !document.getElementById('setup').hidden);
  assert.equal(await page.isChecked('#setup-voices'), true, 'NPC voices ticked (a first run)');
  const names = async (lang, button, label) => {
    const s = await switchLines(page);
    assert.ok(s.lang === lang && s.ok === TEXT[lang][button], `${label}: the button`);
    assert.ok(s.lines.every((line) => line.includes(s.ok)), `${label}: the line names the button as it reads`);
    const want = TEXT[lang].layoutSwitchAtOk('hd', TEXT[lang][button]);
    assert.deepEqual(s.lines, [want, want], `${label}: the line, twice`);
  };
  await page.check('input[name="layout"][value="hd"]');
  await names('ko', 'acceptStart', 'Korean, NPC voices ticked');
  await page.uncheck('#setup-voices');
  await names('ko', 'ok', 'Korean, unticked');
  await page.check('#setup-voices');
  await names('ko', 'acceptStart', 'Korean, ticked again');
  await page.check('input[name="setup-lang"][value="en"]');
  await names('en', 'acceptStart', 'English, ticked');
  await page.uncheck('#setup-voices');
  await names('en', 'ok', 'English, unticked');
  await page.check('input[name="setup-lang"][value="ko"]');
  await names('ko', 'ok', 'Korean again, unticked');
  await context.close();
  console.log('  receipt: the switch line named the button as it read in 6 states (Korean and English, NPC voices ticked and unticked, after a change of the tick and of the language): pass');
});

test('Wide layout (spec 13 Decision 2, the switch line kept by Settings): on a first visit (the Wide build), with a switch to the other build noted, Settings pressed again keeps the line on the status line and the chooser\'s (naming the button as Settings sets it: NPC voices ticked again, a first run), the radio still on the screen chosen; with none noted, Settings empties the chooser\'s line as before (a pack that failed to download said why there)', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  sitePacks(env, { 'index.json': JSON.stringify({ packs: [{ file: 'gone.zip', label: 'Gone' }] }) });
  const { context, page } = await h.openPage(env, { setup: null, layout: null });
  await h.load(page, env);
  await page.waitForFunction(() => !document.getElementById('setup').hidden);
  await listed(page, 1);
  await page.check('input[name="setup-lang"][value="en"]');
  await page.check('input[name="layout"][value="hd"]');
  const line = TEXT.en.layoutSwitchAtOk('hd', TEXT.en.acceptStart);
  assert.deepEqual((await switchLines(page)).lines, [line, line], 'HD 16:9 noted: its line');
  await page.click('#settings');
  assert.deepEqual((await switchLines(page)).lines, [line, line], 'Settings again: the line kept');
  assert.equal(await page.isChecked('input[name="layout"][value="hd"]'), true, 'the radio still on HD 16:9');
  await page.uncheck('#setup-voices');
  await page.click('#settings');
  let s = await switchLines(page);
  assert.ok(await page.isChecked('#setup-voices') && s.ok === TEXT.en.acceptStart, 'Settings ticks NPC voices again (a first run)');
  assert.deepEqual(s.lines, [line, line], 'the line kept, naming the button as Settings set it');
  // Called off; a pack that is not there chosen, NPC voices unticked: OK says why on the chooser's
  // line and leaves the chooser open; Settings again empties it.
  await page.check('input[name="layout"][value="wide"]');
  assert.deepEqual((await switchLines(page)).lines, ['', ''], 'called off: the line gone');
  await page.uncheck('#setup-voices');
  await page.check('#setup-hd input[value="gone.zip"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  s = await switchLines(page);
  assert.ok(!(await page.evaluate(() => document.getElementById('setup').hidden)), 'the pack not downloaded: the chooser open');
  assert.ok(s.lines[1] !== '' && s.lines[1] === s.lines[0], 'its reason on the chooser\'s line');
  await page.click('#settings');
  assert.equal((await switchLines(page)).lines[1], '', 'no switch noted: Settings empties the chooser\'s line');
  await context.close();
  console.log('  receipt: a switch noted kept its line (twice) through 2 presses of Settings, the radio on HD 16:9; with none noted Settings emptied the chooser\'s line: pass');
});

test('Wide layout (spec 13 Decision 2, the switch line on the chooser only): on a first visit (the Wide build), HD 16:9 and the site\'s pack chosen, OK (the pack\'s answer held: its download\'s line on the status line), Settings pressed again leaves the status line as it is and puts the switch line on the chooser\'s line; then a Forget voices admitted behind it (its store held) and the pack let go: the chooser closes and the switch waits (its line on the status line), Settings again leaves that line and puts the switch line on the chooser\'s; the Forget let go, the page loads once into the Original build', async (env) => {
  const { TEXT } = await import('../../site/i18n.js');
  const hdPack = await madePack(env);
  sitePacks(env, { 'index.json': JSON.stringify({ packs: [{ file: 'test-pack.zip', label: 'Test pack' }] }), 'test-pack.zip': hdPack });
  const { context, page } = await h.openPage(env, { setup: null, layout: null });
  let loads = 0;
  page.on('load', () => { loads += 1; });
  await h.load(page, env);
  await page.waitForFunction(() => !document.getElementById('setup').hidden);
  await listed(page, 1);
  await page.check('input[name="setup-lang"][value="en"]');
  await page.uncheck('#setup-voices');
  await page.check('input[name="layout"][value="hd"]');
  await page.check('#setup-hd input[value="test-pack.zip"]');
  const pack = h.holdPack(env, 'test-pack.zip');
  await page.click('#setup-ok');
  await pack.asked;
  // The download's line (the chooser's operation, mirrored on the chooser's line).
  const download = (await switchLines(page)).lines[0];
  assert.ok(download !== '' && download !== TEXT.en.layoutSwitchAtOk('hd', TEXT.en.ok), 'the download\'s line on the status line');
  await page.click('#settings');
  let s = await switchLines(page);
  assert.equal(s.lines[0], download, 'Settings during the download: the status line as it was');
  // A first run still (OK has not kept the choices): Settings ticks NPC voices again.
  assert.equal(s.lines[1], TEXT.en.layoutSwitchAtOk('hd', TEXT.en.acceptStart), 'the switch line on the chooser\'s line, naming the button');
  // The wait: a Forget voices admitted while the chooser's operation is held, its store held.
  await page.evaluate(() => {
    window.__hold('voicepack-forget');
    document.getElementById('kept').open = true;
  });
  await page.click('#forget-voices');
  await page.waitForFunction(() => document.body.dataset.ops === '2', null, { timeout: 30_000 });
  pack.release();
  await page.waitForFunction(() => document.getElementById('setup').hidden, null, { timeout: 30_000 });
  await page.waitForFunction((want) => document.getElementById('status').textContent === want, TEXT.en.layoutSwitchWaiting, { timeout: 30_000 });
  assert.equal(loads, 1, 'no load while the Forget is held');
  await page.click('#settings');
  s = await switchLines(page);
  assert.equal(s.lines[0], TEXT.en.layoutSwitchWaiting, 'Settings while the switch waits: its line on the status line as it was');
  assert.equal(s.lines[1], TEXT.en.layoutSwitchAtOk('hd', TEXT.en.ok), 'the switch line on the chooser\'s line, naming the button');
  await loadAfter(page, () => page.evaluate(() => window.__release('voicepack-forget')));
  await h.waitSettled(page);
  const after = await page.evaluate(() => ({ layout: document.body.dataset.layout, wide: globalThis.__u6Layout.wide }));
  assert.equal(loads, 2, 'one load, once the Forget is done');
  assert.ok(after.layout === 'hd' && !after.wide, 'the Original build, HD 16:9');
  await context.close();
  console.log('  receipt: Settings pressed during the chooser\'s download and during the switch\'s wait left the status line as it was (2 of 2) and put the switch line on the chooser\'s line; one load after the wait: pass');
});

test('Wide layout (spec 13 Decision 2, the voice mark without session storage): where sessionStorage throws, NPC voices ticked and HD 16:9 at the first run (the model stubbed: a download that goes on), OK loads the page once into the Original build; there the voices go on with downloads allowed (the mark the switch left in the tab\'s history entry) and are ready; the mark is taken (gone from the entry)', async (env) => {
  const { context, page } = await h.openPage(env, { setup: null, layout: null });
  await context.addInitScript(voiceLoadsLog, { sessionThrows: true });
  let loads = 0;
  page.on('load', () => { loads += 1; });
  await h.load(page, env);
  await page.waitForFunction(() => !document.getElementById('setup').hidden);
  const refused = () => page.evaluate(() => {
    try {
      void window.sessionStorage;
      return false;
    } catch {
      return true;
    }
  });
  assert.equal(await refused(), true, 'the page\'s sessionStorage throws');
  await page.check('input[name="setup-lang"][value="en"]');
  assert.equal(await page.isChecked('#setup-voices'), true, 'NPC voices ticked (a first run)');
  await page.check('input[name="layout"][value="hd"]');
  await loadAfter(page, () => page.click('#setup-ok', { noWaitAfter: true }));
  await h.waitSettled(page);
  assert.equal(await refused(), true, 'the page loaded again: its sessionStorage throws too');
  await page.waitForFunction(() => window.__voiceLog().loads.some(([d]) => d === 2), null, { timeout: 30_000 });
  let log = await page.evaluate(() => window.__voiceLog());
  assert.ok(log.loads.filter(([d]) => d === 2).every(([, cached]) => !cached), 'the page loaded again asked for the model with downloads allowed');
  await page.waitForFunction(() => ['ready', 'failed'].includes(document.body.dataset.voice), null, { timeout: 30_000 });
  log = await page.evaluate(() => window.__voiceLog());
  const after = await page.evaluate(() => ({
    mark: history.state?.u6Marks ?? null,
    voices: localStorage.getItem('u6.voices'),
    layout: document.body.dataset.layout,
    wide: globalThis.__u6Layout.wide,
    error: document.body.dataset.voiceError,
  }));
  assert.equal(loads, 2, 'one load at OK');
  assert.ok(after.layout === 'hd' && !after.wide, 'the Original build, HD 16:9');
  assert.ok(log.loads.every(([, cached]) => !cached), 'no load from the cache only');
  assert.ok(!log.states.includes('failed') && after.error === '', 'never failed');
  assert.equal(log.states.at(-1), 'ready', 'the voices ready');
  assert.equal(after.voices, 'on', 'the voices kept on');
  assert.equal(after.mark, null, 'the mark taken');
  await context.close();
  console.log(`  receipt: sessionStorage throwing, voices ticked at the first run with HD 16:9: one load at OK, then the model asked for with downloads allowed (loads ${log.loads.map(([d, c]) => `${d}:${c ? 'cache' : 'download'}`).join(', ')}), ready (states ${log.states.join(' > ')}), the mark taken: pass`);
});

// Before the page's scripts: the page's sessionStorage throws on access (as in some privacy
// modes); localStorage works.
function sessionStorageThrows() {
  Object.defineProperty(window, 'sessionStorage', {
    configurable: true,
    get() {
      throw new DOMException('test: session storage refused', 'SecurityError');
    },
  });
}

// Holds `page`'s next request for app.js (the script of the document it loads next) until
// `release()`; `asked` resolves once it came. Another page of the context loads meanwhile, so it
// runs before that document does.
async function holdScript(page) {
  let arrived;
  let release;
  const asked = new Promise((done) => { arrived = done; });
  const gate = new Promise((done) => { release = done; });
  await page.route(/\/app\.js$/, async (route) => {
    arrived();
    await gate;
    await route.continue();
  });
  return { asked, release };
}

test('Wide layout (spec 13 Decision 2, the voice mark is the tab\'s): where sessionStorage throws in every tab, the switch (NPC voices ticked and HD 16:9 at the first run, the model stubbed: a download that goes on) leaves its mark for its own tab only: a second tab of the site, loaded before the first tab\'s page has loaded again, starts the voices from the cache only (the stub does not answer it: a failure there would turn the voices off for the site) and holds no mark; the first tab\'s mark is in its own history entry (not its window.name) while it loads again; loaded again, it asks for the model with downloads allowed and is ready, the mark taken', async (env) => {
  const { context, page: a } = await h.openPage(env, { setup: null, layout: null });
  await context.addInitScript(voiceLoadsLog, { sessionThrows: true, cacheSilent: true });
  await h.load(a, env);
  await a.waitForFunction(() => !document.getElementById('setup').hidden);
  await a.check('input[name="setup-lang"][value="en"]');
  assert.equal(await a.isChecked('#setup-voices'), true, 'NPC voices ticked (a first run)');
  await a.check('input[name="layout"][value="hd"]');
  const script = await holdScript(a);
  const reloaded = a.waitForEvent('load', { timeout: 120_000 });
  reloaded.catch(() => {}); // awaited below; a failure before that closes the page under it
  await a.click('#setup-ok', { noWaitAfter: true });
  await script.asked;
  // The first tab's next document is in, its script held: the mark in the tab's history entry.
  const held = await a.evaluate(() => ({ at: history.state?.u6Marks?.['u6-voice-download-after-reload'], name: window.name }));
  assert.ok(Number.isFinite(held.at) && held.name === '', 'the first tab loading again: its mark in its history entry, not its name');
  const b = await context.newPage();
  await h.load(b, env);
  await b.waitForFunction(() => window.__voiceLog().loads.length > 0, null, { timeout: 30_000 });
  const other = await b.evaluate(() => ({ loads: window.__voiceLog().loads, mark: history.state?.u6Marks ?? null }));
  assert.deepEqual(other.loads, [[1, true]], 'the second tab: the voices from the cache only');
  assert.equal(other.mark, null, 'the second tab: no mark');
  script.release();
  await reloaded;
  await h.waitSettled(a);
  await a.waitForFunction(() => window.__voiceLog().loads.some(([d]) => d === 2), null, { timeout: 30_000 });
  assert.ok((await a.evaluate(() => window.__voiceLog().loads)).filter(([d]) => d === 2).every(([, cached]) => !cached), 'the first tab loaded again: its load with downloads allowed');
  await a.waitForFunction(() => ['ready', 'failed'].includes(document.body.dataset.voice), null, { timeout: 30_000 });
  const first = await a.evaluate(() => ({
    loads: window.__voiceLog().loads,
    mark: history.state?.u6Marks ?? null,
    voice: document.body.dataset.voice,
    layout: document.body.dataset.layout,
    wide: globalThis.__u6Layout.wide,
  }));
  assert.ok(first.layout === 'hd' && !first.wide, 'the first tab loaded again: the Original build, HD 16:9');
  assert.ok(first.loads.some(([d]) => d === 2) && first.loads.every(([, cached]) => !cached), 'the first tab loaded again: the model asked for with downloads allowed');
  assert.equal(first.voice, 'ready', 'the voices ready');
  assert.equal(first.mark, null, 'the mark taken');
  await context.close();
  console.log(`  receipt: sessionStorage throwing in 2 tabs: the second tab loaded during the first's reload started from the cache only (loads ${other.loads.map(([d, c]) => `${d}:${c ? 'cache' : 'download'}`).join(', ')}); the first, loaded again, went on downloading (loads ${first.loads.map(([d, c]) => `${d}:${c ? 'cache' : 'download'}`).join(', ')}), ready: pass`);
});

test('Restart where sessionStorage throws in every tab: its focus mark is the tab\'s: a second tab of the site, loaded before the restarted tab\'s page has loaded again, starts the kept game without moving its focus to the canvas and holds no mark; the restarted tab\'s mark is in its own history entry (not its window.name) while it loads again; loaded again, it has the focus on the canvas, the mark taken', async (env) => {
  const { context, page: a } = await h.openPage(env);
  await context.addInitScript(sessionStorageThrows);
  await h.load(a, env);
  await h.dropFiles(a, h.HFES);
  await started(a, FILES);
  await a.evaluate(() => { globalThis.__trap = true; });
  await a.waitForFunction(() => document.body.dataset.state === 'crashed');
  const script = await holdScript(a);
  const reloaded = a.waitForEvent('load', { timeout: 120_000 });
  reloaded.catch(() => {}); // awaited below; a failure before that closes the page under it
  await a.click('#restart', { noWaitAfter: true });
  await script.asked;
  const held = await a.evaluate(() => ({ at: history.state?.u6Marks?.['u6-focus-after-reload'], name: window.name }));
  assert.ok(Number.isFinite(held.at) && held.name === '', 'the restarted tab loading again: its mark in its history entry, not its name');
  const b = await context.newPage();
  await h.load(b, env);
  await started(b, FILES);
  assert.notEqual((await h.state(b)).focused, 'screen', 'the second tab: the focus not moved to the canvas');
  assert.equal(await b.evaluate(() => history.state?.u6Marks ?? null), null, 'the second tab: no mark');
  script.release();
  await reloaded;
  await started(a, FILES);
  assert.equal((await h.state(a)).focused, 'screen', 'the restarted tab: the focus on the canvas');
  assert.equal(await a.evaluate(() => history.state?.u6Marks ?? null), null, 'the mark taken');
  await context.close();
  console.log('  receipt: sessionStorage throwing in 2 tabs: the second tab loaded during the Restart\'s reload left the focus where it was; the restarted tab put it on the canvas: pass');
}, { needs: h.haveHfes });

test('the tab\'s fallback marks (where sessionStorage throws, in the page\'s history entry: history.state.u6Marks): a mark is taken only while 0 to 59.999 s old and removed whatever it holds, the entry\'s other marks and other keys kept: 0 s and 59.999 s old taken (the voices with downloads allowed), and after a reload of the tab not taken again; 61 s old, exactly 60 s old, a future time, a time that is not a number and an object that turns into no number ignored (the voices from the cache only), no page error; marks that do not read ignored and removed; a state that is not an object ignored and left as it is', async (env) => {
  const { REVISION } = await import('../../site/voice/model.js');
  const NOW = 1_800_000_000_000;
  const KEY = 'u6-voice-download-after-reload';
  const OTHER = { 'u6-test-other': NOW };
  const planted = (value) => ({ u6Marks: { [KEY]: value, ...OTHER }, test: 'kept' });
  const rest = { u6Marks: OTHER, test: 'kept' };
  // [label, the entry's state before, whether the voices start from the cache only, its state
  // after, whether the tab is loaded again (the mark found no more)]
  const cases = [
    ['0 s old', planted(NOW), false, rest, true],
    ['59.999 s old', planted(NOW - 59_999), false, rest, false],
    ['61 s old', planted(NOW - 61_000), true, rest, false],
    ['exactly 60 s old', planted(NOW - 60_000), true, rest, false],
    ['a future time', planted(NOW + 1_000), true, rest, false],
    ['not a number', planted(String(NOW)), true, rest, false],
    ['an object that turns into no number', planted({ toString: null, valueOf: null }), true, rest, false],
    ['marks that do not read', { u6Marks: 'not marks', test: 'kept' }, true, { test: 'kept' }, false],
    ['a state that is not an object', 'a state of another kind', true, 'a state of another kind', false],
  ];
  for (const [label, before, cachedOnly, after, again] of cases) {
    const { context, page } = await h.openPage(env);
    let errors = 0;
    page.on('pageerror', () => { errors += 1; });
    // The clock fixed in every document; the state planted in the tab's first only (the tab's
    // real session storage remembers it: this runs before voiceLoadsLog makes it throw).
    await context.addInitScript(([revision, now, state]) => {
      try {
        localStorage.setItem('u6.voices', 'on');
        localStorage.setItem('u6.voiceTerms', revision);
      } catch {
        return; // An opaque origin (about:blank) has no storage.
      }
      Date.now = () => now;
      if (sessionStorage.getItem('test:planted') === null) {
        sessionStorage.setItem('test:planted', '1');
        history.replaceState(state, '');
      }
    }, [REVISION, NOW, before]);
    await context.addInitScript(voiceLoadsLog, { sessionThrows: true });
    await h.load(page, env);
    await page.waitForFunction(() => window.__voiceLog().loads.length > 0, null, { timeout: 30_000 });
    let seen = await page.evaluate(() => ({ loads: window.__voiceLog().loads, state: history.state }));
    assert.deepEqual(seen.loads, [[1, cachedOnly]], `${label}: ${cachedOnly ? 'ignored, the voices from the cache only' : 'taken, downloads allowed'}`);
    assert.deepEqual(seen.state, after, `${label}: the entry's state after`);
    if (again) {
      await page.reload();
      await h.waitSettled(page);
      await page.waitForFunction(() => window.__voiceLog().loads.length > 1, null, { timeout: 30_000 });
      seen = await page.evaluate(() => ({ loads: window.__voiceLog().loads, state: history.state }));
      assert.deepEqual(seen.loads, [[1, cachedOnly], [2, true]], `${label}: loaded again, the mark not taken again`);
      assert.deepEqual(seen.state, after, `${label}: the entry's state after the second load`);
    }
    assert.equal(errors, 0, `${label}: no page error`);
    await context.close();
  }
  console.log(`  receipt: ${cases.length} cases of the tab's fallback mark, each taken or ignored and removed as listed, one loaded again: pass`);
});

test('Restart\'s mark is taken as the page loads (where sessionStorage throws): with the engine not loading, the page fails and the mark is gone from its history entry, none left for a later load', async (env) => {
  const { context, page } = await h.openPage(env);
  await context.addInitScript(sessionStorageThrows);
  await context.addInitScript(() => {
    history.replaceState({ u6Marks: { 'u6-focus-after-reload': Date.now() } }, '');
  });
  await page.route(/\/pkg(-wide)?\/u6_web\.js$/, (route) => route.abort());
  await h.load(page, env);
  await page.waitForFunction(() => document.body.dataset.state === 'failed', null, { timeout: 30_000 });
  const marks = await page.evaluate(() => history.state?.u6Marks ?? null);
  assert.equal(marks, null, 'the engine failed: the mark taken all the same');
  await context.close();
  console.log('  receipt: an engine that did not load, the focus mark taken as the page loaded: pass');
});

test('a mark in sessionStorage that it refuses to remove is not taken (it would count again on the next load): the voices kept on start from the cache only, the mark left', async (env) => {
  const { REVISION } = await import('../../site/voice/model.js');
  const { context, page } = await h.openPage(env);
  await context.addInitScript((revision) => {
    try {
      localStorage.setItem('u6.voices', 'on');
      localStorage.setItem('u6.voiceTerms', revision);
      const session = sessionStorage;
      session.setItem('u6-voice-download-after-reload', '1');
      const remove = Storage.prototype.removeItem;
      Storage.prototype.removeItem = function removeItem(key) {
        if (this === session) throw new DOMException('test: removal refused', 'SecurityError');
        return remove.call(this, key);
      };
    } catch {
      // An opaque origin (about:blank) has no storage.
    }
  }, REVISION);
  await context.addInitScript(voiceLoadsLog);
  await h.load(page, env);
  await page.waitForFunction(() => window.__voiceLog().loads.length > 0, null, { timeout: 30_000 });
  const seen = await page.evaluate(() => ({
    loads: window.__voiceLog().loads,
    mark: sessionStorage.getItem('u6-voice-download-after-reload'),
  }));
  assert.deepEqual(seen.loads, [[1, true]], 'not taken: the voices from the cache only');
  assert.equal(seen.mark, '1', 'the mark left (its removal refused)');
  await context.close();
  console.log('  receipt: a session mark whose removal was refused: 1 load, from the cache only: pass');
});

test('Wide layout (spec 13 Decision 2, one switch): two choices across the builds before OK load the page once, into the last; a choice across the builds shows a line (on the status line and the chooser\'s, in the page\'s language) that OK loads the page again with that screen and what the game has not saved is lost, gone when the choice is called off; a choice across and back (or to the other layout of the build that runs) before OK loads nothing, the latter at once; after OK the switch waits for every operation admitted before the page loads again, with a line that it waits: a Save held in its store when the switch starts, then a second Save admitted while it waits, held too: no load when the first is done, one load only once the second is, and the second resumes after it', async (env) => {
  const hdPack = await madePack(env);
  const listing = { 'index.json': JSON.stringify({ packs: [{ file: 'test-pack.zip', label: 'Test pack' }] }), 'test-pack.zip': hdPack };
  sitePacks(env, listing);
  const { page, glues } = await bootWide(env);
  let loads = 0;
  page.on('load', () => { loads += 1; });
  const stored = () => page.evaluate(() => localStorage.getItem('u6.layout'));
  const { TEXT } = await import('../../site/i18n.js');
  const lines = () => page.evaluate(() => [document.getElementById('status').textContent, document.getElementById('setup-status').textContent]);
  // Two choices across the builds: HD 16:9, then Original 16:10; one load, into Original. Each
  // says so; back to Wide (the build that runs), the line goes.
  await page.click('#settings');
  await listed(page, 1);
  await page.check('input[name="layout"][value="hd"]');
  assert.deepEqual(await lines(), [TEXT.en.layoutSwitchAtOk('hd', TEXT.en.ok), TEXT.en.layoutSwitchAtOk('hd', TEXT.en.ok)], 'HD 16:9 across the builds: the line, twice');
  await page.check('input[name="setup-lang"][value="ko"]');
  assert.deepEqual(await lines(), [TEXT.ko.layoutSwitchAtOk('hd', TEXT.ko.ok), TEXT.ko.layoutSwitchAtOk('hd', TEXT.ko.ok)], 'in Korean');
  await page.check('input[name="setup-lang"][value="en"]');
  await page.check('input[name="layout"][value="wide"]');
  assert.deepEqual(await lines(), ['', ''], 'called off: the line gone');
  await page.check('input[name="layout"][value="hd"]');
  await page.check('input[name="layout"][value="original"]');
  assert.deepEqual(await lines(), [TEXT.en.layoutSwitchAtOk('original', TEXT.en.ok), TEXT.en.layoutSwitchAtOk('original', TEXT.en.ok)], 'Original 16:10: its line');
  assert.equal(await stored(), 'wide', 'noted only: nothing kept before OK');
  let n = glues.length;
  await loadAfter(page, () => page.click('#setup-ok', { noWaitAfter: true }));
  await started(page, FILES);
  await h.waitPrompt(page);
  await h.waitFrames(page, 30);
  let s = await layoutShown(page);
  assert.equal(loads, 1, 'one load');
  assert.ok(s.layout === 'original' && s.stored === 'original' && glues.slice(n).every((p) => p === '/pkg/u6_web.js'), 'into the last choice: Original 16:10, the Original build');
  // Across and back: Wide, then Original 16:10 (the build and layout that run): no load.
  await page.click('#settings');
  await page.check('input[name="layout"][value="wide"]');
  await page.check('input[name="layout"][value="original"]');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await h.waitFrames(page, 30);
  assert.equal(loads, 1, 'across and back: no load');
  // Wide, then HD 16:9 (the other layout of this build): no load, HD 16:9 at once.
  await page.click('#settings');
  await page.check('input[name="layout"][value="wide"]');
  await page.check('input[name="layout"][value="hd"]');
  s = await layoutShown(page);
  assert.ok(s.layout === 'hd' && s.stored === 'hd', 'HD 16:9 at once, the switch called off');
  await page.click('#setup-ok');
  await h.waitSettled(page);
  await h.waitFrames(page, 30);
  assert.equal(loads, 1, 'no load');
  // The switch's wait: Wide and the site's pack chosen, the pack's answer held, OK (the
  // chooser's operation waits on the pack); a first Save (after a step) admitted behind it, its
  // store held; the pack let go: the chooser closes and the switch starts, waiting on the first
  // Save. THEN a second Save (after another step), admitted while the switch waits, its store
  // held as well. The first let go (and the hold kept for the second): no load; the second let
  // go: one load. A wait on the queue's tail as it was when the switch started would load the
  // page between the two.
  const stores = async (kind) => (await h.log(page)).filter((e) => e.startsWith(`saves:store:${kind}`)).length;
  const save = async () => {
    await page.focus('#screen');
    const { to } = await h.stepOnce(page);
    await h.press(page, 'Control+s', 30);
    await h.press(page, 'y', 60);
    return to;
  };
  await page.click('#settings');
  await listed(page, 1);
  await page.check('input[name="layout"][value="wide"]');
  await page.check('#setup-hd input[value="test-pack.zip"]');
  const pack = h.holdPack(env, 'test-pack.zip');
  await page.click('#setup-ok');
  await pack.asked;
  await page.evaluate(() => window.__hold('saves'));
  const [ends0, starts0] = [await stores('end'), await stores('start')];
  const first = await save();
  pack.release();
  await page.waitForFunction((k) => window.__log.filter((e) => e.startsWith('saves:store:start')).length > k, starts0, { timeout: 30_000 });
  await page.waitForFunction(() => document.getElementById('setup').hidden, null, { timeout: 30_000 });
  assert.equal((await lines())[0], TEXT.en.layoutSwitchWaiting, 'the switch waits on the first Save: its line');
  const second = await save();
  assert.notEqual(second, first, 'the second Save after another step');
  await page.waitForFunction(() => document.body.dataset.ops === '2', null, { timeout: 30_000 });
  assert.equal(loads, 1, 'both Saves admitted, no load yet');
  await page.evaluate(() => {
    window.__release('saves');
    window.__hold('saves');
  });
  await page.waitForFunction(([e, k]) => window.__log.filter((x) => x.startsWith('saves:store:end')).length > e
    && window.__log.filter((x) => x.startsWith('saves:store:start')).length > k + 1, [ends0, starts0], { timeout: 30_000 });
  await h.waitFrames(page, 30);
  assert.equal(loads, 1, 'the first Save done: no load (the second is admitted)');
  assert.equal(await stores('end'), ends0 + 1, 'the second Save still held');
  assert.equal((await lines())[0], TEXT.en.layoutSwitchWaiting, 'the switch waits again (on the second): its line');
  n = glues.length;
  await loadAfter(page, () => page.evaluate(() => window.__release('saves')));
  await started(page, FILES);
  await h.waitPrompt(page);
  assert.equal(loads, 2, 'one load, after the second Save');
  const w = await wideShown(page);
  assert.ok(w.layout === 'wide' && w.wide && glues.slice(n).every((p) => p === '/pkg-wide/u6_web.js'), 'the Wide build');
  assert.equal((await atPrompt(page)).leader, second, 'the second Save resumes');
  assert.ok((await h.savedPaths(page)).includes('savegame/objlist'), 'it is in storage');
  console.log('  receipt: the switch line in English and Korean, gone when called off; HD then Original before OK: one load into Original; Wide and back, Wide then HD: no load; the switch waited (its line) on a held Save, then on a second Save admitted during the wait: no load after the first, one load after the second, which resumed: pass');
}, { needs: h.haveHfes });

test('Wide layout (spec 13 Design 5, drag): the date line dragged by its title strip follows the pointer (in front of the others) and stays whole on the canvas; its press and release reach the game as a gap\'s; the pointer then reaches its content where it is; its place is kept (u6.layout.windows) and comes back after a reload; the windows not dragged stay at their corners', async (env) => {
  const { page } = await bootWide(env);
  await recordPointer(page);
  let s = await wideShown(page);
  const date = s.windows.find((w) => w.name === 'date');
  // From the middle of its title strip, 600 canvas px left and 300 down, in steps (the box is
  // 1280 CSS px for 1920 canvas px: 400 and 200 CSS px).
  const box = await page.locator('#screen').boundingBox();
  const k = box.width / WIDE_CANVAS[0];
  const start = await h.canvasPoint(page, date.bar[0] + date.bar[2] / 2 + 0.5, date.bar[1] + date.bar[3] / 2 + 0.5);
  const n = await pointerCount(page);
  await page.mouse.move(...start);
  await page.mouse.down();
  for (let i = 1; i <= 4; i += 1) await page.mouse.move(start[0] - 150 * k * i, start[1] + 75 * k * i);
  // Mid-drag, the game's pointer at the rest pixel with the button held: its frame kept, to read
  // the arrow there against a frame with the pointer elsewhere (below).
  await h.waitTaken(page);
  await page.evaluate(() => { window.__restFrame = globalThis.__shownGame.frame_rgba(); });
  await page.mouse.up();
  await h.waitTaken(page);
  const sent = await pointerSince(page, n);
  const press = sent.find(([, , b]) => b === 1);
  const release = sent.findLast(([, , b]) => b === 0);
  assert.deepEqual([press?.slice(0, 2), release?.slice(0, 2)], [s.rest, s.rest], 'the press and the release: a gap\'s');
  s = await wideShown(page);
  const moved = s.windows.at(-1);
  assert.equal(moved.name, 'date', 'the dragged window in front');
  assert.deepEqual(moved.outer.slice(0, 2), [date.outer[0] - 600, date.outer[1] + 300], 'moved with the pointer');
  assert.deepEqual([moved.to[0] - moved.outer[0], moved.to[1] - moved.outer[1]], [date.to[0] - date.outer[0], date.to[1] - date.outer[1]], 'its content with it');
  const kept = await page.evaluate(() => JSON.parse(localStorage.getItem('u6.layout.windows')));
  assert.deepEqual(kept.date, moved.outer.slice(0, 2), 'its place kept');
  // The canvas shows the frame's colours there, the map where it was.
  await h.waitFrames(page, 2);
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  const shown = await wideCheck(page, true);
  assert.ok(shown.windows.date.compared > 0 && shown.windows.date.differ === 0, 'the moved date line shows the frame\'s colours');
  assert.ok(shown.map.compared > 50 && shown.map.differ === 0, 'the map where it was');
  // A press in its content where it is now: the pixel it shows.
  const centre = [92, 172];
  const m = await pointerCount(page);
  await page.mouse.click(...(await h.screenPoint(page, ...centre)));
  await h.runFrames(page, 4);
  await page.waitForFunction((c) => window.__pointer.length > c, m, { timeout: 5000 });
  assert.deepEqual((await pointerSince(page, m)).find(([, , b]) => b === 1), [...centre, 1], 'a press in the moved date line reaches its pixel');
  // The arrow the game drew at the rest pixel during the drag: the frame pixels round the rest
  // pixel that differ from this frame (the pointer in the date line now). Some, and none of them
  // has a place on the canvas (no window shows it, nor the map); the arrow at the date line's
  // pixel, read the same way, has.
  const arrows = await page.evaluate(([rest, here]) => {
    const g = globalThis.__shownGame;
    const [a, b, w] = [window.__restFrame, g.frame_rgba(), g.width()];
    const L = globalThis.__u6Layout;
    const drawn = ([px, py]) => {
      const out = [];
      for (let y = Math.max(0, py - 16); y < py + 24; y += 1) {
        for (let x = Math.max(0, px - 16); x < px + 24; x += 1) {
          const i = (y * w + x) * 4;
          if (a[i] !== b[i] || a[i + 1] !== b[i + 1] || a[i + 2] !== b[i + 2]) out.push([x, y]);
        }
      }
      return out;
    };
    const atRest = drawn(rest);
    const atHere = drawn(here);
    return {
      rest: atRest.length,
      restShown: atRest.filter(([x, y]) => L.toCanvas(x, y) !== null).length,
      here: atHere.length,
      hereShown: atHere.filter(([x, y]) => L.toCanvas(x, y) !== null).length,
    };
  }, [s.rest, centre]);
  assert.ok(arrows.rest > 0 && arrows.restShown === 0, 'the arrow at the rest pixel during the drag: drawn, and nowhere on the canvas');
  assert.ok(arrows.here > 0 && arrows.hereShown > 0, 'the arrow in the date line: on the canvas (the check sees an arrow that shows)');
  // Past the canvas's edge: it stays whole on it; the pointer leaves its title strip there (over
  // the map, then off the canvas), and every event of the drag is still the rest of the screen,
  // never a held map pixel.
  const right = await h.canvasPoint(page, moved.bar[0] + 10.5, moved.bar[1] + 10.5);
  const e0 = await pointerCount(page);
  await page.mouse.move(...right);
  await page.mouse.down();
  await page.mouse.move(right[0] + 300 * k, right[1] - 300 * k);
  await page.mouse.move(right[0] + 3000, right[1] - 3000);
  await page.mouse.up();
  await h.runFrames(page, 4);
  await page.waitForFunction((c) => window.__pointer.some(([, , b], i) => i > c && b === 0), e0, { timeout: 5000 });
  const drag = (await pointerSince(page, e0)).slice(1);
  assert.ok(drag.length >= 3 && drag.every(([x, y]) => x === s.rest[0] && y === s.rest[1]), 'past the edge: every event of the drag the rest of the screen');
  assert.ok(drag.some(([, , b]) => b === 1) && drag.at(-1)[2] === 0, 'past the edge: held, then released');
  s = await wideShown(page);
  const edge = s.windows.find((w) => w.name === 'date');
  assert.deepEqual(edge.outer.slice(0, 2), [WIDE_CANVAS[0] - edge.outer[2], 0], 'dragged past the corner: whole on the canvas, at its edge');
  // After a reload: where it was left; the others at their corners.
  await page.reload();
  await started(page, FILES);
  await h.waitPrompt(page);
  s = await wideShown(page);
  assert.deepEqual(s.windows.find((w) => w.name === 'date').outer.slice(0, 2), edge.outer.slice(0, 2), 'its place after a reload');
  // The others at their corners (checked with the date line where it was first).
  const corners = s.windows.filter((w) => w.name !== 'date');
  windowsPlaced({ ...s, windows: [date, ...corners] }, 'the others');
  console.log(`  receipt: the date line dragged by its title strip to ${moved.outer.slice(0, 2).join(',')}, then past the corner to ${edge.outer.slice(0, 2).join(',')}, kept across a reload; its press and release a gap's (the rest pixel ${s.rest.join(',')}: the arrow there ${arrows.rest} frame pixels, none on the canvas; in the date line ${arrows.hereShown} on it); a press in it there at its pixel: pass`);
}, { needs: h.haveHfes });

// Window record `w` (the page's table) with its frame's top-left corner at canvas pixel (x, y),
// its title strip and content with it.
const placedAt = (w, [x, y]) => {
  const [dx, dy] = [x - w.outer[0], y - w.outer[1]];
  const move = ([rx, ry, rw, rh]) => [rx + dx, ry + dy, rw, rh];
  return { ...w, outer: move(w.outer), bar: move(w.bar), to: move(w.to) };
};

test('Wide layout (spec 14 Design 7, memory): places kept before spec 14 (the date line moved, a place for the old side panel window, no place for the actor windows) leave every window without a place at its default place (the side panel\'s own window too, whatever the old window\'s place) and the date line where it was kept; the paper doll dragged by its title strip follows the pointer, in front, its place kept and back after a reload, the others where they were', async (env) => {
  const old = { date: [700, 400], panel: [1400, 72], messages: [1428, 764], commands: [8, 976] };
  const { page } = await bootWide(env);
  await page.evaluate((kept) => localStorage.setItem('u6.layout.windows', kept), JSON.stringify(old));
  await page.reload();
  await started(page, FILES);
  await h.waitPrompt(page);
  let s = await wideShown(page);
  const at = (name) => s.windows.find((w) => w.name === name);
  assert.deepEqual(at('date').outer.slice(0, 2), old.date, 'the date line where it was kept');
  // The others at their default places (checked with the date line at its own).
  const dateHome = placedAt(at('date'), [WIDE_CANVAS[0] - WIDE_EDGE - at('date').outer[2], WIDE_EDGE]);
  const home = (shown) => ({ ...shown, windows: shown.windows.map((w) => (w.name === 'date' ? dateHome : w)) });
  windowsPlaced(home(s), 'kept before spec 14');
  // The side panel's own window (shown in another mode, through the page's read) at its default
  // place too.
  await page.evaluate(() => {
    const g = globalThis.__shownGame;
    g.panel_view = () => Int32Array.from([0x9e, 0, 0, 0]);
  });
  await page.waitForFunction(() => globalThis.__u6Layout.windows.some((w) => w.name === 'other'), null, { timeout: 10_000 });
  s = await wideShown(page);
  windowsPlaced(home(s), 'kept before spec 14, another mode', { other: true });
  await page.evaluate(() => { delete globalThis.__shownGame.panel_view; });
  await page.waitForFunction(() => !globalThis.__u6Layout.windows.some((w) => w.name === 'other'), null, { timeout: 10_000 });
  // The paper doll dragged by its title strip, 300 canvas px left and 200 down, in steps.
  s = await wideShown(page);
  const doll = at('doll');
  const box = await page.locator('#screen').boundingBox();
  const k = box.width / WIDE_CANVAS[0];
  const start = await h.canvasPoint(page, doll.bar[0] + 30.5, doll.bar[1] + doll.bar[3] / 2 + 0.5);
  await page.mouse.move(...start);
  await page.mouse.down();
  for (let i = 1; i <= 4; i += 1) await page.mouse.move(start[0] - 75 * k * i, start[1] + 50 * k * i);
  await page.mouse.up();
  await h.waitTaken(page);
  s = await wideShown(page);
  const moved = s.windows.at(-1);
  assert.equal(moved.name, 'doll', 'the dragged doll in front');
  assert.deepEqual(moved.outer.slice(0, 2), [doll.outer[0] - 300, doll.outer[1] + 200], 'moved with the pointer');
  const kept = await page.evaluate(() => JSON.parse(localStorage.getItem('u6.layout.windows')));
  assert.deepEqual(kept.doll, moved.outer.slice(0, 2), 'its place kept');
  assert.ok(!Object.hasOwn(kept, 'panel') && WIDE_NAMES.every((name) => Array.isArray(kept[name])) && Array.isArray(kept.other), 'every window\'s place kept, the old window\'s gone');
  // After a reload: the doll and the date line where they were left, the others at home.
  await page.reload();
  await started(page, FILES);
  await h.waitPrompt(page);
  s = await wideShown(page);
  assert.deepEqual(at('doll').outer.slice(0, 2), moved.outer.slice(0, 2), 'the doll\'s place after a reload');
  assert.deepEqual(at('date').outer.slice(0, 2), old.date, 'the date line\'s after a reload');
  windowsPlaced(home({ ...s, windows: s.windows.map((w) => (w.name === 'doll' ? doll : w)) }), 'the others after a reload');
  console.log(`  receipt: spec 13's kept places: the date line kept, ${WIDE_NAMES.length} windows and the side panel's own at their default places; the doll dragged to ${moved.outer.slice(0, 2).join(',')}, kept across a reload: pass`);
}, { needs: h.haveHfes });

// --- the remembered sight (spec 18) --------------------------------------------------------------
// The harness keeps it off (`openPage`'s `memory`), so every other test compares the map with the
// original's black; here it is on. The start's room shows nothing a step hides, so the party walks
// to the room's door ten cells south of the start cell and opens and closes it (Use, the door
// south): what it showed through the door is out of sight then, in view.

/// The party ten cells south of where it is: arrow keys toward that cell (south first), a pass when
/// a key walked nothing (a gargoyle of the start fight, or a wall beside the column), bounded; a
/// hero killed by the fight wakes on the start cell and walks on from there.
async function walkSouth(page, cells) {
  const at = async () => (await atPrompt(page)).leader.split(',').map(Number);
  const [sx, sy] = await at();
  const to = [sx, sy + cells];
  for (let tries = 0; tries < 120; tries += 1) {
    const [x, y] = await at();
    if (x === to[0] && y === to[1]) return;
    const key = y < to[1] ? 'ArrowDown' : x < to[0] ? 'ArrowRight' : x > to[0] ? 'ArrowLeft' : 'ArrowUp';
    await page.keyboard.press(key);
    await h.waitTaken(page);
    const [nx, ny] = await at();
    if (nx === x && ny === y) await h.passTurn(page);
  }
  assert.fail(`the party did not reach the cell ${cells} south of the start`);
}

/// Use on the cell south of the leader: U, the cross-hair one cell down, Enter; then the prompt.
async function useSouth(page) {
  await h.waitPrompt(page);
  for (const key of ['u', 'ArrowDown', 'Enter']) await h.press(page, key, 20);
  await h.waitPrompt(page);
}

/// Keeps in the page the game's RGBA frame and the canvas as they are now (`memoryShown`'s
/// `before`: taken while the cells it checks were in sight).
function keepBefore(page) {
  return page.evaluate(() => {
    const c = document.getElementById('screen');
    globalThis.__memBefore = {
      frame: globalThis.__shownGame.frame_rgba().slice(),
      canvas: c.getContext('2d').getImageData(0, 0, c.width, c.height).data.slice(),
    };
  });
}

/// The remembered sight as the page shows it now, the display frozen: over the shown map (8..168
/// both ways in Original), the pixels the engine's frame names the remembered sight
/// (`hd_source_at` flags 0x80; a cell's top-left pixel read first, then its 256 when it has it).
/// Of them, in the game's RGBA frame and on the canvas (every device pixel of a pixel's S x S
/// block) apart: how many are grey (red = green = blue), how many are not black, how many grey
/// levels they take, and how many are `grey` (site's K, u6-render `grey`) of what the same
/// place showed in the frame or on the canvas kept by `keepBefore` (`before`). `blackout`
/// ('frame' or 'canvas') makes the remembered pixels black in that copy first: the negative
/// controls, which its checks must fail. The pixels under the pointer the page draws over the
/// HD frame (`hd_pointer`) are left out, and so are the cells that show themselves.
function memoryShown(page, { before = false, blackout = null } = {}) {
  return page.evaluate(([useBefore, blackout]) => {
    const g = globalThis.__shownGame;
    const c = document.getElementById('screen');
    const s = c.width / 320;
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data.slice();
    const f = g.frame_rgba().slice();
    const was = useBefore ? globalThis.__memBefore : null;
    const memory = (x, y) => ((g.hd_source_at(x, y)[5] ?? 0) & 0x80) !== 0;
    const [px, py, pw, ph] = g.hd_pointer();
    const pointer = (x, y) => x >= px && x < px + pw && y >= py && y < py + ph;
    const grey = (a, i) => {
      const v = (((77 * a[i] + 150 * a[i + 1] + 29 * a[i + 2] + 128) >> 8) * 115 + 128) >> 8;
      return [v, v, v];
    };
    const pixels = [];
    for (let cy = 0; cy < 11; cy += 1) {
      for (let cx = 0; cx < 11; cx += 1) {
        const [x0, y0] = [Math.max(8, cx * 16), Math.max(8, cy * 16)];
        const [x1, y1] = [Math.min(168, cx * 16 + 16), Math.min(168, cy * 16 + 16)];
        if (x0 >= x1 || y0 >= y1 || !memory(x0, y0)) continue;
        for (let y = y0; y < y1; y += 1) {
          for (let x = x0; x < x1; x += 1) if (!pointer(x, y) && memory(x, y)) pixels.push([x, y]);
        }
      }
    }
    // The device pixels of game pixel (x, y)'s block on the canvas, as byte offsets.
    const block = (x, y) => Array.from({ length: s * s }, (_, k) =>
      ((y * s + Math.floor(k / s)) * c.width + x * s + (k % s)) * 4);
    for (const [x, y] of pixels) {
      if (blackout === 'frame') f.fill(0, (y * 320 + x) * 4, (y * 320 + x) * 4 + 3);
      if (blackout === 'canvas') for (const i of block(x, y)) d.fill(0, i, i + 3);
    }
    const isGrey = (a, i) => a[i] === a[i + 1] && a[i + 1] === a[i + 2];
    const lit = (a, i) => a[i] + a[i + 1] + a[i + 2] > 0;
    const same = (a, i, want) => want[0] === a[i] && want[1] === a[i + 1] && want[2] === a[i + 2];
    const out = { flagged: pixels.length, s, hd: g.hd_on(), on: g.sight_memory(), places: pixels };
    const frame = { grey: 0, lit: 0, expected: 0, levels: new Set() };
    const canvas = { grey: 0, lit: 0, expected: 0, levels: new Set() };
    for (const [x, y] of pixels) {
      const i = (y * 320 + x) * 4;
      if (isGrey(f, i)) frame.grey += 1;
      if (lit(f, i)) frame.lit += 1;
      frame.levels.add(f[i]);
      if (was && same(f, i, grey(was.frame, i))) frame.expected += 1;
      const b = block(x, y);
      if (b.every((j) => isGrey(d, j))) canvas.grey += 1;
      if (b.some((j) => lit(d, j))) canvas.lit += 1;
      for (const j of b) canvas.levels.add(d[j]);
      if (was && b.every((j) => same(d, j, grey(was.canvas, j)))) canvas.expected += 1;
    }
    for (const [name, m] of [['frame', frame], ['canvas', canvas]]) out[name] = { ...m, levels: m.levels.size };
    return out;
  }, [before, blackout]);
}

/// What `memoryShown` must hold of the remembered sight, in the game's frame and on the canvas
/// each: some of it (a cell's worth), every pixel grey, most of it not black, at least 3 grey
/// levels, and nine in ten pixels the grey of what they showed in sight (the rest: an animated
/// tile, an actor that stood there). Returns the failed checks' labels.
function memoryChecks(m) {
  const failed = [];
  if (m.flagged < 256) failed.push('a cell\'s worth');
  for (const name of ['frame', 'canvas']) {
    const v = m[name];
    if (v.grey !== m.flagged) failed.push(`${name}: grey`);
    if (v.lit * 2 <= m.flagged) failed.push(`${name}: not black`);
    if (v.levels < 3) failed.push(`${name}: grey levels`);
    if (v.expected * 10 < m.flagged * 9) failed.push(`${name}: the grey of its look in sight`);
  }
  return failed;
}

test('remembered sight (spec 18): on in the page by default and kept off across a reload, off in the harness; with it on at S 4, what the room\'s door showed is grey once the door is closed: every pixel the engine\'s frame remembers is grey in the game\'s frame and on the canvas, not black, the grey of its look in sight in each; switched off, those pixels are the original\'s black; a copy with them blacked out (the frame\'s, the canvas\'s) fails the checks', async (env) => {
  const { page } = await h.openPage(env, { memory: 'on' });
  await h.load(page, env);
  await page.setInputFiles('#picker', h.HFES);
  await started(page, FILES);
  let m = await memoryShown(page);
  assert.ok(m.on && m.hd && m.s === 4, 'on, HD on at S 4');
  assert.equal(m.flagged, 0, 'nothing remembered at the start');
  await walkSouth(page, 10);
  await useSouth(page);
  await h.waitFrames(page, 2);
  await keepBefore(page);
  await useSouth(page);
  // What the canvas shows, the display frozen (no step between the canvas and the frame read).
  await h.virtualDisplay(page, 60);
  await h.runFrames(page, 2);
  m = await memoryShown(page, { before: true });
  assert.deepEqual(memoryChecks(m), [], 'the remembered sight shown');
  for (const where of ['frame', 'canvas']) {
    const control = memoryChecks(await memoryShown(page, { before: true, blackout: where }));
    assert.ok(control.includes(`${where}: not black`) && control.includes(`${where}: the grey of its look in sight`), `the control: the ${where} blacked out fails`);
    assert.ok(control.every((label) => label.startsWith(where)), `the control: only the ${where}'s checks fail`);
  }
  const line = (v) => `${v.grey} grey, ${v.lit} not black, ${v.levels} levels, ${v.expected} the grey of their look in sight`;
  console.log(`  receipt: spec 18's remembered sight: ${m.flagged} pixels at S ${m.s}; frame ${line(m.frame)}; canvas ${line(m.canvas)}: pass`);
  // Off: the same pixels (their places from the reading above: the engine's frame no longer
  // names them) are the original's black in the RGBA frame and on the canvas.
  await page.evaluate(() => globalThis.__shownGame.set_sight_memory(false));
  await h.runFrames(page, 4);
  const off = await page.evaluate((places) => {
    const g = globalThis.__shownGame;
    const c = document.getElementById('screen');
    const s = c.width / 320;
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
    const f = g.frame_rgba();
    let black = 0;
    for (const [x, y] of places) {
      const i = (y * 320 + x) * 4;
      const j = (y * s * c.width + x * s) * 4;
      if (f[i] + f[i + 1] + f[i + 2] === 0 && d[j] + d[j + 1] + d[j + 2] === 0) black += 1;
    }
    return { black, on: g.sight_memory(), named: places.filter(([x, y]) => (g.hd_source_at(x, y)[5] ?? 0) & 0x80).length };
  }, m.places);
  assert.ok(!off.on && off.named === 0, 'off: nothing drawn from the remembered sight');
  assert.equal(off.black, m.places.length, 'off: those pixels black');
  // Every test page closes with its context; the harness's default and the page's own follow.
  const { page: plain } = await h.openPage(env);
  await h.load(plain, env);
  await plain.setInputFiles('#picker', h.HFES);
  await started(plain, FILES);
  // The game the page presented last, once there is one (a reload starts the page again).
  const kept = async (p) => {
    await p.waitForFunction(() => globalThis.__shownGame?.started(), null, { timeout: 60_000 });
    return p.evaluate(() => [globalThis.__shownGame.sight_memory(), localStorage.getItem('u6.memory')]);
  };
  assert.deepEqual(await kept(plain), [false, 'off'], 'the harness keeps it off');
  const { page: fresh } = await h.openPage(env, { memory: null });
  await h.load(fresh, env);
  await fresh.setInputFiles('#picker', h.HFES);
  await started(fresh, FILES);
  assert.deepEqual(await kept(fresh), [true, null], 'nothing kept: on, the page\'s default');
  await fresh.evaluate(() => localStorage.setItem('u6.memory', 'off'));
  await fresh.reload();
  await started(fresh, FILES);
  assert.deepEqual(await kept(fresh), [false, 'off'], 'kept off: off after a reload');
}, { needs: h.haveHfes });

// The system's pointer over the game screen: the game draws its own (the engine's sprite over
// the frame; with HD on the page draws it over the HD frame, spec 11 Design 7), so while a game
// runs the stylesheet hides the system's over the canvas where the engine's sprite is under the
// mouse (index.html, data-pointer). The tests keep the mouse's own place apart from the page's
// (`trackMouse`: its last pointer event of type mouse, as `window.__testMouse`), and every read
// after a settle checks the one claim the page makes: data-pointer `game` only where the engine's
// sprite is drawn with its hot spot at the mouse's game pixel (Game.game_pointer; with HD on
// also inside the box the page draws, Game.hd_pointer), with HD on and off.

// Page script: the mouse's client point as its last pointer event (of type mouse) left it, in
// `window.__testMouse` (null before one), for the cursor checks (`cursorOn`); and
// `window.__mousePixel()`, where that point is: `on` the canvas, and the game pixel it reaches
// through the layout in effect (`__u6Layout.toGame`), null with no mouse.
const trackMouse = (page) => page.evaluate(() => {
  window.__testMouse = null;
  for (const kind of ['pointerdown', 'pointermove', 'pointerup']) {
    window.addEventListener(kind, (e) => {
      if (e.pointerType === 'mouse') window.__testMouse = [e.clientX, e.clientY];
    }, { capture: true });
  }
  window.__mousePixel = () => {
    const m = window.__testMouse;
    if (!m) return null;
    const c = document.getElementById('screen');
    const box = c.getBoundingClientRect();
    const on = m[0] >= box.left && m[0] < box.right && m[1] >= box.top && m[1] < box.bottom;
    const [px, py] = [((m[0] - box.left) * c.width) / box.width, ((m[1] - box.top) * c.height) / box.height];
    return { on, pixel: globalThis.__u6Layout.toGame(px, py) };
  };
});
const mousePixel = (page) => page.evaluate(() => window.__mousePixel());

// What the page shows of the system's pointer: the body's state, the canvas's program, its
// computed cursor and data-pointer, the computed cursor of the first element each selector of
// `selectors` names, and `holds`: with data-pointer `game`, whether the engine's sprite is drawn
// (Game.game_pointer: shown) with its hot spot at the mouse's game pixel (`__mousePixel`, on
// the canvas), with HD on also inside the pointer's box the page draws (Game.hd_pointer); false
// for `game` with no mouse tracked; null for another value (nothing claimed).
const cursorOn = (page, ...selectors) => page.evaluate((list) => {
  const c = document.getElementById('screen');
  const pointer = c.dataset.pointer ?? null;
  let holds = null;
  if (pointer === 'game') {
    const g = globalThis.__shownGame;
    const m = window.__mousePixel?.() ?? null;
    holds = false;
    if (m && m.on) {
      const [gx, gy] = m.pixel;
      const [ex, ey, shown] = g.game_pointer();
      holds = shown === 1 && ex === gx && ey === gy;
      if (holds && g.hd_on()) {
        const [x, y, w, hh] = g.hd_pointer();
        holds = w > 0 && gx >= x && gx < x + w && gy >= y && gy < y + hh;
      }
    }
  }
  return {
    state: document.body.dataset.state,
    program: c.dataset.program ?? null,
    canvas: getComputedStyle(c).cursor,
    pointer,
    holds,
    of: list.map((s) => getComputedStyle(document.querySelector(s)).cursor),
  };
}, selectors);

// The cursor once settled (`frames` frames on), the claim checked (`cursorOn`'s `holds` not
// false): data-pointer `game` only with the engine's sprite drawn at the mouse, HD on or off.
async function settled(page, label, frames = 2) {
  await h.waitFrames(page, frames);
  const c = await cursorOn(page);
  assert.ok(c.holds !== false, `${label}: data-pointer game only where the game's pointer holds the mouse's pixel`);
  return c;
}

// Waits until the canvas's data-pointer is `want` (`showGamePointer`), then settles (`settled`,
// 3 frames: it holds there), with no pointer event; returns the cursor then.
async function settledCursor(page, want, label = `data-pointer ${want}`) {
  await page.waitForFunction((w) => document.getElementById('screen').dataset.pointer === w, want, { timeout: 10_000 });
  return settled(page, label, 3);
}

// The game pixel the engine placed its sprite at in the last step, once nothing is queued
// (Game.game_pointer), is (gx, gy).
const engineAt = (page, gx, gy) => page.waitForFunction(([x, y]) => {
  const [ex, ey, , queued] = globalThis.__shownGame.game_pointer();
  return queued === 0 && ex === x && ey === y;
}, [gx, gy], { timeout: 10_000 });

// The mouse moved to game pixel (gx, gy) (`h.screenPoint`: through the layout in effect), once
// the engine took it (`engineAt`) and frames drew it: with HD on its box (frame pixels) and
// pixels, of those the layout shows the ones the canvas shows in their own colour (`keepPointer`,
// `pointerColours`; none with HD off), and the cursor settled (`settled`).
async function mouseTo(page, gx, gy) {
  await page.mouse.move(...(await h.screenPoint(page, gx, gy)));
  await engineAt(page, gx, gy);
  const cursor = await settled(page, `the mouse at (${gx}, ${gy})`);
  if (!(await page.evaluate(() => globalThis.__shownGame.hd_on()))) return { cursor };
  const kept = await keepPointer(page);
  const colours = await pointerColours(page);
  const { box, rgba } = await page.evaluate(() => window.__pointer);
  return { kept, colours, box, rgba, cursor };
}

// The game's pointer drawn at the mouse at game pixel `a`, then `b` (`mouseTo`), the system's
// hidden over the canvas at each: a box at each holding the mouse's pixel, the second the first
// moved by the mouse's own move, every pixel of it the layout shows on the canvas in its own
// colour. Returns the second reading.
async function pointerFollows(page, a, b, label) {
  const first = await mouseTo(page, ...a);
  const second = await mouseTo(page, ...b);
  for (const [r, [gx, gy], at] of [[first, a, 'first'], [second, b, 'second']]) {
    assert.ok(r.cursor.state === 'running' && r.cursor.canvas === 'none' && r.cursor.pointer === 'game', `${label}, ${at} point: the system's pointer hidden over the canvas`);
    const [x, y, w, hh] = r.box;
    assert.ok(w > 0 && hh > 0 && gx >= x && gx < x + w && gy >= y && gy < y + hh, `${label}, ${at} point: the game's pointer at the mouse`);
    assert.ok(r.kept.area && r.colours.seen > 0 && r.colours.same === r.colours.seen, `${label}, ${at} point: the canvas shows the game's pointer there`);
  }
  const moved = [second.box[0] - first.box[0], second.box[1] - first.box[1]];
  assert.ok(moved[0] === b[0] - a[0] && moved[1] === b[1] - a[1] && second.box[2] === first.box[2], `${label}: the game's pointer moved as the mouse did`);
  return second;
}

// Two game pixels in the message scroll (spec 10's region, a window in Wide; out of the map, so
// the arrow shows at both), and one more for fullscreen.
const SCROLL_POINTS = [[200, 140], [260, 170]];
const FULL_POINT = [220, 150];

// Fullscreen (the browser's): the system's pointer still hidden over the canvas, the game's at the
// mouse.
async function fullscreenCursor(page, label) {
  await page.click('#fullscreen');
  await waitFull(page, 'on');
  await h.waitFrames(page, 2);
  const r = await mouseTo(page, ...FULL_POINT);
  const [x, y, w, hh] = r.box;
  const [gx, gy] = FULL_POINT;
  assert.ok(r.cursor.canvas === 'none' && r.cursor.pointer === 'game', `${label} fullscreen: the system's pointer hidden over the canvas`);
  assert.ok(w > 0 && gx >= x && gx < x + w && gy >= y && gy < y + hh && r.colours.seen > 0 && r.colours.same === r.colours.seen, `${label} fullscreen: the game's pointer at the mouse`);
  await page.evaluate(() => document.exitFullscreen());
  await waitFull(page, 'off');
}

// The client point of the mouse as the tests keep it (`trackMouse`).
const testMouse = (page) => page.evaluate(() => window.__testMouse);

// Settings' "HD on" switched to `on` by the page itself (no mouse move), once the game shows it.
async function hdTo(page, on) {
  await page.evaluate((want) => {
    if (globalThis.__shownGame.hd_on() !== want) document.getElementById('hd').click();
  }, on);
  await page.waitForFunction((want) => globalThis.__shownGame.hd_on() === want, on, { timeout: 10_000 });
}

test('the system\'s pointer (the launcher): kept over the canvas while `u` runs, in the opening and in the menu: `u` takes no mouse (its pointer stays where it is) and the game\'s own is not drawn over it (no HD pointer at either of two mouse places)', async (env) => {
  const { page } = await h.openPage(env, { gameFirst: false });
  await h.load(page, env);
  await h.dropFiles(page, [h.ZIP]);
  await started(page);
  await h.waitProgram(page, 'u');
  await h.waitVisible(page);
  await page.focus('#screen');
  const check = async (label) => {
    for (const [gx, gy] of SCROLL_POINTS) {
      await page.mouse.move(...(await h.screenPoint(page, gx, gy)));
      await h.waitTicks(page, 2);
      await h.waitFrames(page, 1);
      const c = await cursorOn(page);
      const box = await page.evaluate(() => [...globalThis.__shownGame.hd_pointer()]);
      assert.ok(c.state === 'running' && c.program === 'u', `${label}: \`u\` runs`);
      assert.equal(box[2], 0, `${label}: no game pointer drawn at the mouse`);
      assert.equal(c.canvas, 'auto', `${label}: the canvas keeps the system's pointer`);
    }
  };
  await check('the opening');
  await h.press(page, 'Escape', 120);
  await check('the menu');
  console.log(`  receipt: the launcher: the system's pointer kept over the canvas at ${2 * SCROLL_POINTS.length} mouse places in the opening and the menu: pass`);
}, { needs: h.haveZip });

test('the system\'s pointer (`game` with the engine\'s pointer hidden): J in the launcher\'s menu with no character made, the game waiting on its message with its pointer hidden (no sprite drawn, the HD pointer empty) though given the mouse\'s place: the canvas keeps the system\'s pointer, with HD on and off', async (env) => {
  const { page } = await h.openPage(env, { gameFirst: false });
  await h.load(page, env);
  await h.dropFiles(page, [h.ZIP]);
  await started(page);
  await h.waitProgram(page, 'u');
  await h.waitVisible(page);
  await page.focus('#screen');
  await trackMouse(page);
  await h.press(page, 'Escape', 120);
  await h.press(page, 'j', 30);
  await h.waitProgram(page, 'game');
  await h.waitFrames(page, 10);
  const checks = [];
  for (const hd of [true, false]) {
    await hdTo(page, hd);
    for (const [gx, gy] of hd ? SCROLL_POINTS : SCROLL_POINTS.toReversed()) {
      const label = `HD ${hd ? 'on' : 'off'}, hidden at (${gx}, ${gy})`;
      await page.mouse.move(...(await h.screenPoint(page, gx, gy)));
      await engineAt(page, gx, gy);
      const c = await settled(page, label);
      const [, , shown] = await page.evaluate(() => [...globalThis.__shownGame.game_pointer()]);
      const box = await page.evaluate(() => [...globalThis.__shownGame.hd_pointer()]);
      assert.ok(c.state === 'running' && c.program === 'game', `${label}: precondition, \`game\` runs`);
      assert.ok(shown === 0 && box[2] === 0, `${label}: precondition, the engine's pointer hidden, no sprite drawn`);
      assert.ok(c.pointer === 'page' && c.canvas === 'auto', `${label}: the canvas keeps the system's pointer`);
      checks.push(c);
    }
  }
  console.log(`  receipt: \`game\` with its pointer hidden: the system's pointer kept at ${checks.length} mouse places the engine took, HD on and off: pass`);
}, { needs: h.haveZip });

test('the system\'s pointer (Original layout): hidden over the canvas while the game runs, where the game draws its own at the mouse (read from the HD pointer, and with HD off from the classic frame), windowed and fullscreen; kept while the press that opened the context menu is held, and while the press outside that closed it is held with the mouse back on the game\'s pointer (it does not follow the mouse then); a move after it hides it again', async (env) => {
  const { page } = await menuPage(env);
  await trackMouse(page);
  let c = await cursorOn(page);
  assert.equal(c.state, 'running', 'the game runs');
  const r = await pointerFollows(page, ...SCROLL_POINTS, 'Original, HD on');
  // HD off: the classic frame (the engine's sprite over it) and the canvas hold the game's
  // pointer at the same box, in the colours the HD pointer gave; the system's still hidden.
  await page.evaluate(() => document.getElementById('hd').click());
  await page.waitForFunction(() => !globalThis.__shownGame.hd_on(), null, { timeout: 10_000 });
  await h.waitFrames(page, 2);
  const off = await page.evaluate(([[x, y, w, hh], rgba]) => {
    const g = globalThis.__shownGame;
    const f = g.frame_rgba();
    const cv = document.getElementById('screen');
    const d = cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data;
    let [seen, frame, canvas] = [0, 0, 0];
    for (let i = 0; i < w * hh; i += 1) {
      if (rgba[4 * i + 3] === 0) continue;
      seen += 1;
      const j = ((y + Math.floor(i / w)) * g.width() + x + (i % w)) * 4;
      if ([0, 1, 2].every((k) => f[j + k] === rgba[4 * i + k])) frame += 1;
      if ([0, 1, 2].every((k) => d[j + k] === rgba[4 * i + k])) canvas += 1;
    }
    return { seen, frame, canvas, size: [cv.width, cv.height], cursor: getComputedStyle(cv).cursor };
  }, [r.box, r.rgba]);
  assert.deepEqual(off.size, [320, 200], 'HD off: the canvas is the classic frame');
  assert.ok(off.seen > 0 && off.frame === off.seen && off.canvas === off.seen, 'HD off: the classic frame and the canvas show the game\'s pointer at the mouse');
  assert.equal(off.cursor, 'none', 'HD off: the system\'s pointer hidden over the canvas');
  await page.evaluate(() => document.getElementById('hd').click());
  await page.waitForFunction(() => globalThis.__shownGame.hd_on(), null, { timeout: 10_000 });
  await fullscreenCursor(page, 'Original');
  // The press that opened the context menu, held: none of it reaches the game.
  await h.waitPrompt(page);
  await h.rightPress(page, 88, 88);
  c = await settled(page, 'the opening press held');
  assert.ok(c.pointer === 'page' && c.canvas === 'auto', 'the press that opened the menu: the canvas keeps the system\'s pointer');
  await page.mouse.move(...(await h.screenPoint(page, 120, 104)), { steps: 3 });
  c = await settled(page, 'held and moved');
  assert.ok(c.pointer === 'page' && c.canvas === 'auto', 'held and moved: the canvas keeps the system\'s pointer');
  await page.mouse.up({ button: 'right' });
  // A plain game pixel off the menu, given to the game; then the press outside that closes the
  // menu, held (none of it reaches the game), the mouse moved away and back to that pixel: the
  // game's pointer is there but does not follow the mouse, so the system's shows.
  const plain = SCROLL_POINTS[1];
  const q = await h.screenPoint(page, ...plain);
  const off2 = await page.evaluate(([x, y]) => document.elementFromPoint(x, y) === document.getElementById('screen'), q);
  assert.ok(off2, 'precondition: that pixel is off the menu');
  await mouseTo(page, ...plain);
  await page.mouse.down();
  await page.locator('#context-menu').waitFor({ state: 'hidden', timeout: 10_000 });
  await page.mouse.move(q[0] - 40, q[1] - 30, { steps: 3 });
  await page.mouse.move(...q, { steps: 3 });
  c = await settled(page, 'the closing press held, back on the game\'s pointer', 4);
  const [ex, ey] = await page.evaluate(() => [...globalThis.__shownGame.game_pointer()]);
  assert.ok(ex === plain[0] && ey === plain[1], 'precondition: the game\'s pointer under the mouse again');
  assert.ok(c.pointer === 'page' && c.canvas === 'auto', 'the closing press held: the canvas keeps the system\'s pointer');
  await page.mouse.up();
  await pointerFollows(page, ...SCROLL_POINTS, 'Original, after the menu');
  console.log('  receipt: Original: the system\'s pointer hidden over the canvas at 6 points (2 with HD on, 1 with it off, 1 fullscreen, 2 after the menu), the game\'s at the mouse at each; kept through the opening press and the held closing press: pass');
}, { needs: h.haveHfes });

test('the system\'s pointer (HD layout): hidden over the canvas where a region of the game shows the game\'s own pointer at the mouse, windowed and fullscreen; in a gap between the regions and on the paper round them (the game\'s pointer at the nearest edge there) the canvas keeps it', async (env) => {
  const { page } = await bootLayout(env);
  await trackMouse(page);
  assert.equal((await layoutShown(page)).view, 'game', 'the HD layout shows the game\'s regions');
  await pointerFollows(page, ...SCROLL_POINTS, 'HD layout');
  const gaps = [[670, 300, 'between the map and the side panel'], [2, 360, 'the paper at the left edge']];
  for (const [cx, cy, label] of gaps) {
    await page.mouse.move(...(await h.canvasPoint(page, cx + 0.5, cy + 0.5)));
    const c = await settledCursor(page, 'page', label);
    assert.equal(c.canvas, 'auto', `${label}: the system's pointer`);
  }
  await pointerFollows(page, ...SCROLL_POINTS.toReversed(), 'HD layout, back from the gaps');
  await fullscreenCursor(page, 'HD layout');
  console.log(`  receipt: HD layout: the system's pointer hidden at 5 points with the game's at the mouse, kept at ${gaps.length} gaps: pass`);
}, { needs: h.haveHfes });

test('the system\'s pointer (Wide layout): hidden over the canvas where the map or a window\'s content shows the game\'s own pointer at the mouse, windowed and fullscreen; on a window\'s frame and title strip, and while a window is dragged by it with the mouse off the strip on that window\'s content (the game\'s pointer at the rest of the screen then), the canvas keeps it', async (env) => {
  const { page } = await bootWide(env);
  await trackMouse(page);
  assert.equal((await wideShown(page)).view, 'game', 'the Wide layout shows the map and the windows');
  await pointerFollows(page, ...SCROLL_POINTS, 'Wide');
  const w = (await wideShown(page)).windows.find((win) => win.name === 'messages');
  const gaps = [
    [w.bar[0] + Math.floor(w.bar[2] / 2), w.bar[1] + Math.floor(w.bar[3] / 2), 'the message scroll\'s title strip'],
    [w.outer[0] + w.outer[2] - 2, w.to[1] + 29, 'the message scroll\'s right border'],
  ];
  for (const [cx, cy, label] of gaps) {
    await page.mouse.move(...(await h.canvasPoint(page, cx + 0.5, cy + 0.5)));
    const c = await settledCursor(page, 'page', label);
    assert.equal(c.canvas, 'auto', `${label}: the system's pointer`);
  }
  // A drag by the title strip down past the canvas's bottom edge: the window stops there, the
  // mouse goes on onto its content, the drag still held (the game's pointer at the rest of the
  // screen).
  const [sx, sy] = gaps[0];
  await page.mouse.move(...(await h.canvasPoint(page, sx + 0.5, sy + 0.5)));
  await page.mouse.down();
  await page.mouse.move(...(await h.canvasPoint(page, sx + 0.5, sy + 60.5)), { steps: 4 });
  const held = await settled(page, 'the drag held off the strip', 3);
  const moved = (await wideShown(page)).windows.find((win) => win.name === 'messages');
  const onContent = sy + 60.5 >= moved.to[1] && sy + 60.5 < moved.to[1] + moved.to[3];
  const rest = await page.evaluate(() => [...globalThis.__shownGame.game_pointer()].slice(0, 2));
  assert.ok(onContent && moved.to[1] + moved.to[3] + 4 >= 1080 - 8, 'precondition: the window held at the bottom edge, the mouse on its content');
  assert.deepEqual(rest, (await wideShown(page)).rest, 'precondition: the drag gives the game the rest of the screen');
  assert.ok(held.pointer === 'page' && held.canvas === 'auto', 'a window dragged, the mouse off its strip: the system\'s pointer');
  await page.mouse.up();
  await pointerFollows(page, ...SCROLL_POINTS.toReversed(), 'Wide, after the drag');
  await fullscreenCursor(page, 'Wide');
  console.log(`  receipt: Wide: the system's pointer hidden at 5 points with the game's at the mouse, kept at ${gaps.length} gaps and through a drag off the strip: pass`);
}, { needs: h.haveHfes });

test('the system\'s pointer (the speed badge): over the badge, which lets the pointer through to the canvas but covers the game\'s pointer there, the canvas shows it, the game still given the move; off the badge it is hidden again; the badge hidden under a still mouse (back at 1x) hides it with no move', async (env) => {
  const { page } = await menuPage(env);
  await trackMouse(page);
  await pointerFollows(page, ...SCROLL_POINTS, 'before the badge');
  await page.keyboard.press('NumpadAdd');
  await page.locator('#speed-badge').waitFor({ state: 'visible', timeout: 10_000 });
  const b = await page.locator('#speed-badge').boundingBox();
  const at = [b.x + b.width / 2, b.y + b.height / 2];
  const was = await page.evaluate(() => globalThis.__shownGame.hd_pointer()[4]);
  await page.mouse.move(...at);
  await page.waitForFunction((v) => globalThis.__shownGame.hd_pointer()[4] !== v, was, { timeout: 10_000 });
  let c = await settledCursor(page, 'page', 'over the badge');
  const under = await page.evaluate(([x, y]) => {
    const cv = document.getElementById('screen');
    const box = cv.getBoundingClientRect();
    const [px, py] = [Math.floor(((x - box.left) * 320) / box.width), Math.floor(((y - box.top) * 200) / box.height)];
    const [bx, by, bw, bh] = globalThis.__shownGame.hd_pointer();
    return { canvasHit: document.elementFromPoint(x, y) === cv, given: bw > 0 && px >= bx && px < bx + bw && py >= by && py < by + bh };
  }, at);
  assert.ok(under.canvasHit, 'the badge lets the pointer through: the canvas is under the mouse');
  assert.ok(under.given, 'the game was given the move: its pointer under the badge');
  assert.equal(c.canvas, 'auto', 'over the badge: the system\'s pointer');
  await pointerFollows(page, ...SCROLL_POINTS, 'off the badge');
  await page.mouse.move(...at);
  await settledCursor(page, 'page', 'over the badge again');
  await page.keyboard.press('NumpadSubtract');
  await page.locator('#speed-badge').waitFor({ state: 'hidden', timeout: 10_000 });
  c = await settledCursor(page, 'game', 'the badge hidden');
  assert.equal(c.canvas, 'none', 'the badge hidden under the still mouse: the system\'s pointer hidden, no move');
  console.log('  receipt: the speed badge: the system\'s pointer over it, the game given the move under it; hidden off it, and again once the badge hid under a still mouse: pass');
}, { needs: h.haveHfes });

test('the system\'s pointer under a still mouse (HD layout): a context menu closed by Escape with the mouse on the game\'s pointer leaves it hidden there, and with the mouse on an entry (the game\'s pointer where the press left it) shows it; HD switched off, and fullscreen, each put another game pixel under the mouse, so the canvas shows it, and back hides it, with no move each time', async (env) => {
  const { page } = await bootLayout(env);
  await trackMouse(page);
  const top = (p) => page.evaluate(([x, y]) => {
    const el = document.elementFromPoint(x, y);
    return { menu: document.getElementById('context-menu').contains(el), cursor: getComputedStyle(el).cursor };
  }, p);
  // The menu opened over the game's pointer and closed with the mouse still there: hidden.
  await mouseTo(page, 88, 88);
  const press = await testMouse(page);
  await h.rightPress(page, 88, 88);
  await page.mouse.up({ button: 'right' });
  await page.keyboard.press('Escape');
  await page.locator('#context-menu').waitFor({ state: 'hidden', timeout: 10_000 });
  let c = await settledCursor(page, 'game', 'a menu closed on the game\'s pointer');
  const still = await top(press);
  assert.ok(!still.menu && still.cursor === 'none' && c.canvas === 'none', 'a menu closed by Escape, the mouse on the game\'s pointer: the system\'s pointer hidden');
  // The menu opened again, the mouse onto its first entry: the menu takes the move (the game is
  // not given it); Escape: the game's pointer where the press left it, the system's shown.
  await h.rightPress(page, 88, 88);
  await page.mouse.up({ button: 'right' });
  const item = await page.locator('#context-menu [role="menuitem"]').first().boundingBox();
  const on = [item.x + item.width / 2, item.y + item.height / 2];
  await page.mouse.move(...on);
  await h.waitFrames(page, 3);
  const open = await top(on);
  assert.ok(open.menu && open.cursor !== 'none', 'precondition: the mouse on the menu, its own cursor');
  await page.keyboard.press('Escape');
  await page.locator('#context-menu').waitFor({ state: 'hidden', timeout: 10_000 });
  c = await settled(page, 'a menu closed on its entry', 3);
  const closed = await top(on);
  assert.ok(!closed.menu && c.pointer === 'page' && closed.cursor === 'auto', 'a menu closed by Escape, the mouse on its entry, no move: the canvas under the mouse shows the system\'s pointer');
  await pointerFollows(page, SCROLL_POINTS[1], SCROLL_POINTS[0], 'HD layout');
  // A change under the still mouse: the game pixel under it (`__mousePixel`, through the layout
  // then in effect) another, the system's pointer shown; undone, the same pixel again, hidden.
  const same = (p, [gx, gy]) => p.on && p.pixel[0] === gx && p.pixel[1] === gy;
  const away = async (change, back, label) => {
    assert.ok(same(await mousePixel(page), SCROLL_POINTS[0]), `${label}: precondition, the mouse over the game's pointer`);
    await change();
    c = await settledCursor(page, 'page', `${label}, no move`);
    assert.ok(!same(await mousePixel(page), SCROLL_POINTS[0]), `${label}: another game pixel under the mouse`);
    assert.equal(c.canvas, 'auto', `${label}, no move: the system's pointer (the game's is not under it)`);
    await back();
    c = await settledCursor(page, 'game', `${label} undone, no move`);
    assert.ok(same(await mousePixel(page), SCROLL_POINTS[0]), `${label} undone: the game's pointer's pixel under the mouse again`);
    assert.equal(c.canvas, 'none', `${label} undone, no move: the system's pointer hidden (the game's under it again)`);
  };
  const hd = (on) => async () => {
    await hdTo(page, on);
    await page.waitForFunction((w) => document.body.dataset.layout === (w ? 'hd' : 'original'), on, { timeout: 10_000 });
  };
  // The toolbar's Fullscreen clicked by the page itself (no mouse move): the browser's or the
  // page's own, as the browser allows; then out again.
  const full = (on) => async () => {
    await page.evaluate(() => document.getElementById('fullscreen').click());
    await page.waitForFunction((w) => (document.body.dataset.fullscreen !== 'off') === w, on, { timeout: 10_000 });
  };
  await away(hd(false), hd(true), 'HD off');
  await away(full(true), full(false), 'fullscreen');
  console.log('  receipt: under a still mouse: a menu closed on the game\'s pointer (hidden) and on its entry (shown), HD off and on again, fullscreen in and out, each the cursor it needs by the next frames: pass');
}, { needs: h.haveHfes });

test('the system\'s pointer and a context-menu entry (opus M1): Look chosen by a click, the mouse moved one game pixel at once and then still, both before a step of the game (the virtual display); once the prompt is back the game\'s pointer is where the entry\'s target press (queued after the move) left it, not under the mouse, and the canvas keeps the system\'s pointer', async (env) => {
  const { page } = await menuPage(env);
  await trackMouse(page);
  const target = viewCell(6, 5);
  await h.rightPress(page, ...target);
  await page.mouse.up({ button: 'right' });
  const item = await page.locator(`#context-menu [data-code="${LOOK}"]`).boundingBox();
  const [mx, my] = [item.x + item.width / 2, item.y + item.height / 2];
  const one = await page.evaluate(() => document.getElementById('screen').getBoundingClientRect().width / 320);
  // No step of the game from the click to the move: the move is queued before the entry's
  // target press, which its command queues once it reads the target.
  await h.virtualDisplay(page, 60);
  await page.mouse.click(mx, my);
  await page.locator('#context-menu').waitFor({ state: 'hidden', timeout: 10_000 });
  await page.mouse.move(mx + one, my);
  await h.runFrames(page, 60);
  // A portrait (someone on the cell: the game's people walk) waits for a key: Space, which
  // moves no pointer, until the prompt is back.
  let back = false;
  for (let i = 0; i < 4 && !back; i += 1) {
    back = await h.runFramesToPrompt(page, 240).then(() => true, () => false);
    if (!back) await page.keyboard.press('Space');
  }
  assert.ok(back, 'precondition: the prompt back');
  const c = await settled(page, 'the prompt back', 3);
  // The game's pointer drawn, seen on the canvas, at the target's pixel; the mouse elsewhere.
  const m = await mousePixel(page);
  const [ex, ey, shown] = await page.evaluate(() => [...globalThis.__shownGame.game_pointer()]);
  const kept = await keepPointer(page);
  const colours = await pointerColours(page);
  const [x, y, w, hh] = (await page.evaluate(() => window.__pointer)).box;
  assert.ok(shown === 1 && kept.box && kept.area && colours.seen > 0 && colours.same === colours.seen, 'precondition: the game\'s pointer drawn and seen on the canvas');
  assert.ok(ex === target[0] && ey === target[1] && target[0] >= x && target[0] < x + w && target[1] >= y && target[1] < y + hh, 'precondition: at the entry\'s target');
  assert.ok(m.on && (m.pixel[0] !== target[0] || m.pixel[1] !== target[1]), 'precondition: the mouse on the canvas, off the target');
  assert.ok(c.pointer === 'page' && c.canvas === 'auto', 'the game\'s pointer elsewhere: the canvas keeps the system\'s pointer');
  console.log('  receipt: a context-menu entry\'s target press after the mouse\'s move: the system\'s pointer kept where the game\'s is not: pass');
}, { needs: h.haveHfes });

test('the system\'s pointer and a touch (gpt MUST 1): the mouse still over the game\'s pointer (hidden), then a touch moves the game\'s pointer elsewhere on the canvas: the mouse\'s place is the mouse\'s own, so the canvas shows the system\'s pointer again, with HD on and off; the window\'s blur forgets the mouse too', async (env) => {
  const { page } = await menuPage(env);
  await trackMouse(page);
  const touch = (gx, gy) => h.screenPoint(page, gx, gy).then(([x, y]) => page.evaluate(([cx, cy]) => {
    const init = { pointerType: 'touch', pointerId: 7, isPrimary: true, clientX: cx, clientY: cy, buttons: 0, bubbles: true, cancelable: true, composed: true };
    document.getElementById('screen').dispatchEvent(new PointerEvent('pointermove', init));
  }, [x, y]));
  let r;
  let c;
  for (const hd of [true, false]) {
    const label = `HD ${hd ? 'on' : 'off'}`;
    const [mouse, touched] = hd ? SCROLL_POINTS : SCROLL_POINTS.toReversed();
    await hdTo(page, hd);
    r = await mouseTo(page, ...mouse);
    assert.ok(r.cursor.pointer === 'game' && r.cursor.canvas === 'none', `${label}: precondition, the still mouse over the game's pointer, the system's hidden`);
    await touch(...touched);
    await engineAt(page, ...touched);
    c = await settled(page, `${label}, after the touch`, 4);
    assert.ok(c.pointer === 'page' && c.canvas === 'auto', `${label}: the touch moved the game's pointer off the still mouse: the system's pointer shows`);
  }
  // The mouse back over the game's pointer, then the window's blur: the mouse's place forgotten
  // (it may come back elsewhere), the system's pointer shown until it moves again.
  await hdTo(page, true);
  r = await mouseTo(page, ...SCROLL_POINTS[0]);
  assert.ok(r.cursor.pointer === 'game', 'precondition: hidden again over the game\'s pointer');
  await page.evaluate(() => window.dispatchEvent(new Event('blur')));
  c = await settled(page, 'after the blur', 3);
  assert.ok(c.pointer === 'page' && c.canvas === 'auto', 'the window\'s blur: the system\'s pointer shows');
  console.log('  receipt: a touch elsewhere moved the game\'s pointer, the still mouse keeps the system\'s pointer, HD on and off; a blur forgets the mouse: pass');
}, { needs: h.haveHfes });

test('the system\'s pointer once the game stops: hidden over the game\'s pointer while it runs, then shown though data-pointer is as the last frame left it: after the game ended (Ctrl-Q, Y; the program then `ended`) and after an engine trap (the program still `game`: the page\'s state alone says it stopped)', async (env) => {
  const stops = [
    ['ended', async (page) => {
      await page.keyboard.press('Control+q');
      await h.waitTicks(page, 30);
      await page.keyboard.press('y');
    }],
    ['crashed', (page) => page.evaluate(() => { globalThis.__trap = true; })],
  ];
  for (const [state, stop] of stops) {
    const { page } = await menuPage(env);
    await trackMouse(page);
    const r = await mouseTo(page, ...SCROLL_POINTS[0]);
    assert.ok(r.cursor.pointer === 'game' && r.cursor.canvas === 'none', `${state}: precondition, hidden while it runs`);
    await stop(page);
    await page.waitForFunction((s) => document.body.dataset.state === s, state, { timeout: 30_000 });
    const c = await cursorOn(page);
    assert.ok(c.pointer === 'game', `${state}: precondition, data-pointer as the last frame left it`);
    assert.ok(c.program === (state === 'ended' ? 'ended' : 'game'), `${state}: precondition, the program the canvas says`);
    assert.equal(c.canvas, 'auto', `${state}: the system's pointer shows`);
  }
  console.log(`  receipt: the system's pointer shown once the game ${stops.map(([s]) => s).join(' and once it ')}: pass`);
}, { needs: h.haveHfes });

test('the system\'s pointer and the Exit button (opus S1): in fullscreen, the Original layout filling a 16:10 screen so the Exit button lies over the canvas, a press held on the canvas and moved onto the button (the canvas holds the pointer captured, the game given each move: its pointer under the button) shows the system\'s pointer', async (env) => {
  const { page } = await menuPage(env, { width: 1280, height: 800 });
  await trackMouse(page);
  await page.click('#fullscreen');
  await waitFull(page, 'on');
  await h.waitFrames(page, 2);
  await mouseTo(page, ...SCROLL_POINTS[0]);
  await page.mouse.down();
  const exit = page.locator('#fullscreen-exit');
  await exit.waitFor({ state: 'visible', timeout: 10_000 });
  const b = await exit.boundingBox();
  const at = [b.x + b.width / 2, b.y + b.height / 2];
  await page.mouse.move(...at, { steps: 4 });
  const m = await mousePixel(page);
  await engineAt(page, ...m.pixel);
  const c = await settled(page, 'a held press over the Exit button', 3);
  const over = await page.evaluate(([x, y]) => {
    const button = document.getElementById('fullscreen-exit');
    const r = button.getBoundingClientRect();
    const box = document.getElementById('screen').getBoundingClientRect();
    return {
      top: document.elementFromPoint(x, y) === button && !button.hidden,
      inside: r.left >= box.left && r.right <= box.right && r.top >= box.top && r.bottom <= box.bottom,
      shown: globalThis.__shownGame.game_pointer()[2],
    };
  }, at);
  assert.ok(m.on && over.top && over.inside && over.shown === 1, 'precondition: the Exit button over the canvas under the mouse, the game\'s pointer drawn under it');
  assert.ok(c.pointer === 'page' && c.canvas === 'auto', 'a held press over the Exit button: the system\'s pointer shows');
  await page.mouse.up();
  await page.evaluate(() => document.exitFullscreen());
  await waitFull(page, 'off');
  console.log('  receipt: a press held on the canvas moved onto the Exit button over it: the system\'s pointer shown: pass');
}, { needs: h.haveHfes });

test('the system\'s pointer, the stylesheet\'s scope: only the canvas hides it: on a first visit (the chooser) the canvas, the chooser and its OK keep it; in fullscreen, the canvas hiding it over the game\'s pointer, the letterbox round the canvas and the Exit button keep it', async (env) => {
  const { page: first } = await h.openPage(env, { setup: null });
  await h.load(first, env);
  assert.ok(!(await first.evaluate(() => document.getElementById('setup').hidden)), 'precondition: the chooser shows');
  const c = await cursorOn(first, '#screen', '#setup', '#setup-ok');
  assert.ok(c.of.every((v) => v !== 'none'), 'a first visit: the canvas, the chooser and its OK keep it');
  const { page } = await menuPage(env);
  await trackMouse(page);
  await page.click('#fullscreen');
  await waitFull(page, 'on');
  const r = await mouseTo(page, ...FULL_POINT);
  assert.ok(r.cursor.pointer === 'game' && r.cursor.canvas === 'none', 'precondition: the canvas hides it over the game\'s pointer');
  await page.locator('#fullscreen-exit').waitFor({ state: 'visible', timeout: 10_000 });
  const scope = await page.evaluate(() => {
    const box = document.getElementById('screen').getBoundingClientRect();
    const bar = document.elementFromPoint(innerWidth / 2, box.top / 2);
    return {
      letterbox: box.top > 0 && bar?.id === 'screen-box',
      bar: bar ? getComputedStyle(bar).cursor : null,
      exit: getComputedStyle(document.getElementById('fullscreen-exit')).cursor,
    };
  });
  assert.ok(scope.letterbox, 'precondition: the letterbox above the canvas is the screen box');
  assert.ok(scope.bar !== 'none' && scope.exit !== 'none', 'fullscreen: the letterbox and the Exit button keep it');
  await page.evaluate(() => document.exitFullscreen());
  await waitFull(page, 'off');
  console.log('  receipt: the stylesheet hides the system\'s pointer on the canvas only: the chooser, its OK, the letterbox and the Exit button keep it: pass');
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
    // Each test starts with a site that lists nothing (every visit asks /packs/index.json), and
    // answers nothing held.
    env.packs.clear();
    env.packRequests.length = 0;
    for (const hold of env.packHolds.values()) hold.release();
    env.packHolds.clear();
    try {
      await t.fn(env);
      console.log(`ok - ${t.name}`);
    } catch (err) {
      failed += 1;
      // A fixed head (an assertion's own label, else the error's kind) and the places in these
      // files: no message text (h.failureLines).
      console.log(`not ok - ${t.name}\n  ${h.failureLines(err).join('\n  ')}`);
    } finally {
      for (const b of await env.browsers()) for (const context of b.contexts()) await context.close();
    }
  }
} finally {
  await env.close();
}
console.log(`# web tests: ${ran} ran, ${ran - failed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);

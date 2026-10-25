// Tests of tools/speech/profile.mjs (spec 09 Design 4, spec 08's upload boundary) on synthetic
// bundles the test writes (no game data), with a counting fake transport, in a directory of its
// own (never ~/Games/ultima6):
//
// - zero requests: no terminal and no --send, --dry-run (with --send too), and on a terminal a
//   refusal, an empty answer, the input's end, Ctrl-C at the prompt and SIGINT; the preview is
//   the exact first payload, 0600 in a 0700 directory; a typed yes sends (so the prompt is the
//   gate, not a dead end);
// - with --send: the pack's shape, every actor of every script profiled covered and checked by
//   the page's readPack, an invalid answer asked again once then counted as a failure, the
//   coverage counts reconciled, the files 0600; a SIGINT while sending sends nothing more;
// - two runs on one output directory: the second refuses while the first holds the lock, so the
//   preview the first printed is what its yes sends; the writer's temporary file is made
//   exclusively (a planted symlink is not followed);
// - the CLI itself in a child process against a counting HTTP server (the real transport): no
//   terminal and no --send sends nothing, --send sends and writes the pack, a 307 or 308 to
//   another server is not followed; and on a real
//   terminal (a pty, through Python's pty module where python3 is there), Ctrl-C at the prompt
//   sends nothing;
// - nothing the tool prints holds a name, a line, an actor number or a script's item (the numbers
//   read past the preview's path, whose pid and random part may hold one by chance);
// - spec 15 Design 4-5: the voice pack /2 (read with each voice's vary, every bad vary refused, a
//   /1 pack still read with vary 0/0, the ranges' revision unchanged), a line's profile
//   (lineProfile: each side of its motion cut to the room the range leaves, toward a side with
//   none the other way, the voice's base kept, so no line is held at a range's end), the voice
//   derived from a speaker (deriveProfile: the preset families, the
//   medians by age and sex, the offsets, the spread, vary by manner) and --speakers runs on a /1
//   and a /2 speaker file the test writes (no request, the pack /2, the coverage's counts).
//
//   node --test tools/speech/profile.test.mjs
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { EventEmitter } from 'node:events';
import { existsSync } from 'node:fs';
import { chmod, lstat, mkdir, mkdtemp, readdir, readFile, readlink, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { PassThrough } from 'node:stream';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { STYLES } from '../../site/voice/model.js';
import {
  checkedProfile, draw, lineProfile, PACK_FORMAT, PACK_FORMAT_2, PITCH, RANGES_REVISION, readPack, SPEED, unit, VARY,
} from '../../site/voice/profile.js';
import * as translate from '../i18n/translate.mjs';
import {
  AGE_MEDIANS, deriveProfile, familyOf, LEAD_WEIGHT, MANNER_OFFSETS, MANNER_VARY, main, planOf, PRESET_FAMILIES, PREVIEW,
  SPEAKER_DEFAULTS, SPEAKERS_MODEL, speakerOutputsOf, speakerPlanOf, SPREAD, STATION_OFFSETS, SYSTEM, traitsOf, writePrivate,
} from './profile.mjs';

const TOOL = join(dirname(fileURLToPath(import.meta.url)), 'profile.mjs');
const SENTINEL = 'Zq6VoiceSentinel';
const LINE = 'Zq6LineSentinel';
// Distinctive numbers: the actors and items of the fixture, never to be printed.
const ACTORS = { good: [31337, 31338], again: [31339], bad: [31341], skipped: [31343], silent: [31345] };
const ITEMS = [7001, 7002, 7003, 7004, 7005, 7006];

const text = (t) => [{ kind: 'text', key: ['a', 1, 0], text: t }];
// A u6.npc-bundle/1 as the page's exporter writes it, with the test's own name and lines.
function bundle(archive, item, actors, name, { refused = null, lines = [] } = {}) {
  return {
    format: 'u6.npc-bundle/1', archive, item, hash: '0123456789abcdef', refused, actors,
    portraits: [], name, locale: 'en',
    narration: { look: text(`a person called ${name}`) },
    speech: { greeting: [...text(`Hello, I am ${name}. ${LINE}`), { kind: 'var', at: 1, op: 'b5' }] },
    turns: [{ input: null, segments: [], topics: lines.map((l) => ({ at: 0, keywords: ['job'], requires: [], answer: text(l) })) }],
    traits: [], strings: [],
  };
}

// The fixture: two scripts that answer well (one of them only when asked again), one whose answers
// never parse, one refused and one no actor uses.
const BUNDLES = [
  bundle('a', ITEMS[0], ACTORS.good, `${SENTINEL}Good`, { lines: ['I guard the gate.', 'Away with you!'] }),
  bundle('a', ITEMS[1], ACTORS.again, `${SENTINEL}Again`, { lines: ['Ask me twice.'] }),
  bundle('b', ITEMS[2], ACTORS.bad, `${SENTINEL}Bad`, { lines: ['I never answer well.'] }),
  bundle('b', ITEMS[3], ACTORS.skipped, `${SENTINEL}Refused`, { refused: 'test refusal' }),
  bundle('b', ITEMS[4], [], `${SENTINEL}Unused`),
];

const GOOD = { mix: [{ style: 'M3', w: 0.7 }, { style: 'M5', w: 0.3 }], speed: 1.2, pitch: -1.5, note: `gruff ${SENTINEL}` };

// A counting fake transport: the answer for each request from its NPC's name (the bad one is
// never JSON; the "again" one is prose the first time).
function fakeTransport() {
  const bodies = [];
  const asked = new Map();
  const send = async (body) => {
    bodies.push(body);
    const user = JSON.parse(body).messages[0].content;
    const who = /Name: (\S+)/.exec(user)[1];
    asked.set(who, (asked.get(who) ?? 0) + 1);
    let content = JSON.stringify(GOOD);
    if (who.endsWith('Bad')) content = 'I would rather not say.';
    if (who.endsWith('Again') && asked.get(who) === 1) content = '';
    return { status: 200, json: { type: 'message', content: [{ type: 'text', text: content }], usage: { input_tokens: 100, output_tokens: 20 } } };
  };
  return { send, bodies };
}

// A terminal for the prompt: what the test types, as a TTY stream readline takes keys from.
function terminal() {
  const input = new PassThrough();
  input.isTTY = true;
  input.setRawMode = () => input;
  return input;
}

// The output, kept as text.
function output() {
  const out = new PassThrough();
  out.text = '';
  out.on('data', (d) => { out.text += d; });
  return out;
}

async function fixture() {
  const dir = await mkdtemp(join(tmpdir(), 'u6-voice-profile-'));
  const bundles = join(dir, 'ultima6-npc-bundles.json');
  await writeFile(bundles, JSON.stringify(BUNDLES));
  return { dir, bundles, out: join(dir, 'voice') };
}

// The path of a preview the tool printed: its name holds the run's pid and a random part, either
// of which may hold an actor number or an item of the fixture by chance.
const PREVIEW_PATH = /\S*first-request-[0-9]+-[0-9a-f]{12}\.json/g;

// Nothing the tool printed names an NPC, a line, a note, an actor or an item (the numbers read past
// the preview's path).
function nothingTold(printed) {
  assert.ok(!printed.includes(SENTINEL) && !printed.includes(LINE), 'no name, line or note printed');
  const told = printed.replace(PREVIEW_PATH, '');
  for (const n of [...Object.values(ACTORS).flat(), ...ITEMS]) {
    assert.ok(!new RegExp(`(^|[^0-9])${n}([^0-9]|$)`).test(told), 'no actor number or item printed');
  }
}

const mode = async (path) => (await stat(path)).mode & 0o777;

// The path of the preview the run printed (its last), or null.
const previewPrinted = (printed) => [...printed.matchAll(/The exact first request: (.+) \((\d+) bytes\)\./g)].at(-1)?.[1] ?? null;

// The preview the run printed: the first request's exact bytes, 0600 in a 0700 directory, a name
// of the run's own, among the previews there (other runs' previews are left alone).
async function previewChecked(out, printed, first = null) {
  const path = previewPrinted(printed);
  assert.ok(path !== null && dirname(path) === join(out, 'preview'), 'the preview\'s path printed');
  assert.match(path.split('/').at(-1), /^first-request-[0-9]+-[0-9a-f]{12}\.json$/, 'a name of the run\'s own');
  assert.ok((await readdir(join(out, 'preview'))).filter((n) => PREVIEW.test(n)).includes(path.split('/').at(-1)), 'the preview is there');
  assert.equal(await mode(join(out, 'preview')), 0o700, 'the preview directory is 0700');
  assert.equal(await mode(out), 0o700, 'the output directory is 0700');
  assert.equal(await mode(path), 0o600, 'the preview is 0600');
  const preview = await readFile(path, 'utf8');
  const body = JSON.parse(preview);
  assert.ok(body.system === SYSTEM && body.messages.length === 1 && body.messages[0].content.includes(`${SENTINEL}Good`), 'the first request');
  assert.deepEqual(Object.keys(body), ['model', 'max_tokens', 'system', 'messages'], 'the model, the cap, the instruction and the NPC\'s text: nothing else');
  if (first !== null) assert.equal(preview, first, 'the preview is the exact first payload sent');
  return preview;
}

test('the check of what the tool printed reads past the preview\'s path, whose pid and random part may hold an actor number or an item of the fixture; a number printed elsewhere still fails it', () => {
  const printed = `The exact first request: /tmp/u6-voice-profile-x/voice/preview/first-request-${ITEMS[0]}-0${ACTORS.good[0]}${ITEMS[1]}ab.json (10 bytes).\n`;
  nothingTold(printed);
  assert.throws(() => nothingTold(`${printed}Actor ${ACTORS.good[0]}.\n`), 'an actor number elsewhere');
  assert.throws(() => nothingTold(`${printed}Item ${ITEMS[1]}.\n`), 'an item elsewhere');
});

test('zero requests: no terminal and no --send', async () => {
  const f = await fixture();
  const t = fakeTransport();
  const out = output();
  const code = await main([f.bundles, '--out', f.out], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() });
  assert.equal(code, 1);
  assert.equal(t.bodies.length, 0, 'no request');
  assert.match(out.text, /Route: llmux on this machine \(http:\/\/127\.0\.0\.1:3456\), then OpenAI \(model gpt-6-astra\)\./);
  assert.match(out.text, /Nothing was sent: there is no terminal to ask on; add --send to send\./);
  await previewChecked(f.out, out.text);
  assert.ok(!existsSync(join(f.out, 'voices.u6vp')), 'no pack');
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

test('zero requests: --dry-run, alone and with --send', async () => {
  const f = await fixture();
  for (const args of [['--dry-run'], ['--dry-run', '--send']]) {
    const t = fakeTransport();
    const out = output();
    const code = await main([f.bundles, '--out', f.out, ...args], { transport: t.send, stdout: out, isTTY: true, stdin: terminal(), signals: new EventEmitter() });
    assert.equal(code, 0, `${args}: done`);
    assert.equal(t.bodies.length, 0, `${args}: no request`);
    assert.match(out.text, /Dry run: nothing was sent\./);
    assert.match(out.text, /The exact first request: .*first-request-[0-9]+-[0-9a-f]{12}\.json \(\d+ bytes\)\./);
    await previewChecked(f.out, out.text);
    nothingTold(out.text);
  }
  await rm(f.dir, { recursive: true });
});

// On a terminal: what is typed at the prompt (`keys`, or a SIGINT when null) and the exit code.
for (const [what, keys, want] of [
  ['a refusal', 'no\r', 1],
  ['an empty answer', '\r', 1],
  ['"yes" with another case', 'Yes\r', 1],
  ['the input\'s end', '', 1],
  ['Ctrl-C at the prompt', '\x03', 130],
  ['SIGINT', null, 130],
]) {
  test(`zero requests on a terminal: ${what}`, async () => {
    const f = await fixture();
    const t = fakeTransport();
    const out = output();
    const stdin = terminal();
    const signals = new EventEmitter();
    const run = main([f.bundles, '--out', f.out], { transport: t.send, stdout: out, isTTY: true, stdin, signals });
    await new Promise((done) => { const wait = () => (out.text.includes('Type yes') ? done() : setTimeout(wait, 5)); wait(); });
    await previewChecked(f.out, out.text);
    if (keys === null) signals.emit('SIGINT');
    else if (keys === '') stdin.end();
    else stdin.write(keys);
    assert.equal(await run, want, 'the exit code');
    assert.equal(t.bodies.length, 0, 'no request');
    assert.match(out.text, /Nothing was sent/);
    assert.equal(signals.listenerCount('SIGINT'), 0, 'its SIGINT handler gone');
    nothingTold(out.text);
    await rm(f.dir, { recursive: true });
  });
}

test('on a terminal, a typed yes sends; the preview is the exact first payload', async () => {
  const f = await fixture();
  const t = fakeTransport();
  const out = output();
  const stdin = terminal();
  const run = main([f.bundles, '--out', f.out, '--jobs', '1'], { transport: t.send, stdout: out, isTTY: true, stdin, signals: new EventEmitter() });
  await new Promise((done) => { const wait = () => (out.text.includes('Type yes') ? done() : setTimeout(wait, 5)); wait(); });
  stdin.write('yes\r');
  assert.equal(await run, 3, 'written, with a failure');
  assert.ok(t.bodies.length > 0, 'sent');
  await previewChecked(f.out, out.text, t.bodies[0]);
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

// Waits until `out` shows the prompt.
const prompted = (out) => new Promise((done) => { const wait = () => (out.text.includes('Type yes') ? done() : setTimeout(wait, 5)); wait(); });

test('two runs on one output directory: while the first waits at its prompt the second refuses (a dry run or --send) and leaves its preview alone; the preview the first printed is byte for byte what it sends after yes; the lock goes with the run, and one left by a run that is gone is taken over', async () => {
  const f = await fixture();
  // The second run's bundles: another first NPC, so an overwritten preview would differ.
  const other = join(f.dir, 'other.json');
  await writeFile(other, JSON.stringify([bundle('a', 1, [9], `${SENTINEL}Other`, { lines: ['Another line.'] })]));
  const t = fakeTransport();
  const out = output();
  const stdin = terminal();
  const first = main([f.bundles, '--out', f.out, '--jobs', '1'], { transport: t.send, stdout: out, isTTY: true, stdin, signals: new EventEmitter() });
  await prompted(out);
  const path = previewPrinted(out.text);
  const shown = await readFile(path, 'utf8');
  for (const args of [['--dry-run'], ['--send']]) {
    const t2 = fakeTransport();
    const out2 = output();
    const code = await main([other, '--out', f.out, ...args], { transport: t2.send, stdout: out2, isTTY: false, signals: new EventEmitter() });
    assert.equal(code, 2, `${args}: refused`);
    assert.match(out2.text, /another run of this tool holds .*\.lock .*nothing was sent\./, `${args}: says why`);
    assert.equal(t2.bodies.length, 0, `${args}: no request`);
    assert.equal(previewPrinted(out2.text), null, `${args}: no preview of its own`);
    assert.equal(await readFile(path, 'utf8'), shown, `${args}: the first run's preview unchanged`);
  }
  stdin.write('yes\r');
  assert.equal(await first, 3);
  assert.equal(t.bodies[0], shown, 'what the first run sent first is the preview it printed, byte for byte');
  assert.ok(!existsSync(join(f.out, '.lock')), 'the lock goes with the run');
  // A lock left by a run that is gone (a pid no process has now) is taken over.
  const gone = spawn(process.execPath, ['-e', '']);
  await new Promise((done) => gone.on('close', done));
  await writeFile(join(f.out, '.lock'), `${gone.pid}\n`);
  const out3 = output();
  assert.equal(await main([f.bundles, '--out', f.out, '--dry-run'], { stdout: out3, isTTY: false, signals: new EventEmitter() }), 0, 'a stale lock taken over');
  await previewChecked(f.out, out3.text);
  // A run never removes another run's preview: had two runs both taken over one stale lock, the
  // one shown to the owner by the first would still be there, as printed.
  assert.notEqual(previewPrinted(out3.text), path, 'a preview of its own');
  assert.equal(await readFile(path, 'utf8'), shown, 'the first run\'s preview is left as printed');
  nothingTold(out.text + out3.text);
  await rm(f.dir, { recursive: true });
});

test('the writer makes its temporary file exclusively: a symlink planted at its name is not followed (its target unchanged) nor is a file planted there written; the file written is 0600', async () => {
  const f = await fixture();
  await mkdir(f.out, { recursive: true });
  const target = join(f.dir, 'target.txt');
  await writeFile(target, 'the target, untouched');
  await chmod(target, 0o644);
  const path = join(f.out, 'voices.u6vp');
  const planted = `${path}.part-${process.pid}-planted`;
  const kept = `${path}.part-${process.pid}-kept`;
  await symlink(target, planted);
  await writeFile(kept, 'an old file');
  await chmod(kept, 0o644);
  const names = ['planted', 'kept', 'fresh'];
  const bytes = await writePrivate(path, 'the new text', () => names.shift());
  assert.equal(await readFile(target, 'utf8'), 'the target, untouched', 'the symlink\'s target unchanged');
  assert.equal(await mode(target), 0o644, 'and its mode');
  assert.equal(bytes, 12);
  assert.deepEqual(names, [], 'past both planted names');
  assert.ok((await lstat(planted)).isSymbolicLink() && (await readlink(planted)) === target, 'the symlink left as it was');
  assert.ok((await readFile(kept, 'utf8')) === 'an old file' && (await mode(kept)) === 0o644, 'the planted file left as it was');
  assert.equal(await readFile(path, 'utf8'), 'the new text');
  assert.equal(await mode(path), 0o600, 'the file written is 0600');
  assert.ok(!existsSync(`${path}.part-${process.pid}-fresh`), 'its temporary file renamed');
  await rm(f.dir, { recursive: true });
});

// A directory the test made outside the output directory, as a planted link would name it: its
// mode and what it holds, to compare after a run.
async function decoy(dir) {
  await mkdir(dir, { recursive: true });
  await chmod(dir, 0o755);
  await writeFile(join(dir, 'kept.txt'), 'the owner\'s own file');
  return async () => ({ mode: (await stat(dir)).mode & 0o777, names: (await readdir(dir)).sort(), kept: await readFile(join(dir, 'kept.txt'), 'utf8') });
}

test('a symbolic link (or a file) where the output directory or its preview directory goes is refused before any request, with a line that names the rule only; the link\'s target keeps its mode and contents', async () => {
  for (const where of ['output', 'preview', 'file']) {
    const f = await fixture();
    const target = join(f.dir, 'elsewhere');
    const seen = await decoy(target);
    const before = await seen();
    if (where === 'output') await symlink(target, f.out);
    else if (where === 'file') await writeFile(f.out, 'not a directory');
    else {
      await mkdir(f.out, { recursive: true });
      await symlink(target, join(f.out, 'preview'));
    }
    const t = fakeTransport();
    const out = output();
    const code = await main([f.bundles, '--out', f.out, '--send'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() });
    assert.equal(code, 2, `${where}: refused`);
    assert.equal(t.bodies.length, 0, `${where}: no request`);
    const dir = where === 'preview' ? 'preview directory' : 'output directory';
    assert.ok(out.text.includes(`profile: the ${dir} is a symbolic link or not a directory: nothing was sent.`), `${where}: the rule's line`);
    assert.equal(previewPrinted(out.text), null, `${where}: no preview`);
    assert.deepEqual(await seen(), before, `${where}: the target's mode and contents unchanged`);
    if (where === 'preview') assert.ok(!existsSync(join(f.out, '.lock')), 'the lock goes with the run');
    nothingTold(out.text);
    await rm(f.dir, { recursive: true });
  }
});

test('with --send: the pack covers every actor of the scripts profiled, checked as the page checks it; an invalid answer is asked again once, then counted; the coverage reconciles; the files are 0600', async () => {
  const f = await fixture();
  const t = fakeTransport();
  const out = output();
  const code = await main([f.bundles, '--out', f.out, '--send', '--model', 'test-model'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() });
  assert.equal(code, 3, 'written, with a failure');
  // Requests: Good once, Again twice (empty, then good), Bad twice (prose both times).
  assert.equal(t.bodies.length, 5, 'five requests');
  const names = t.bodies.map((b) => /Name: (\S+)/.exec(JSON.parse(b).messages[0].content)[1]).sort();
  assert.deepEqual(names, [`${SENTINEL}Again`, `${SENTINEL}Again`, `${SENTINEL}Bad`, `${SENTINEL}Bad`, `${SENTINEL}Good`]);
  assert.ok(t.bodies.every((b) => JSON.parse(b).model === 'test-model'), 'the model named');
  // The pack.
  const pack = JSON.parse(await readFile(join(f.out, 'voices.u6vp'), 'utf8'));
  assert.deepEqual(Object.keys(pack), ['format', 'model', 'revision', 'profiles']);
  assert.ok(pack.format === PACK_FORMAT && pack.model === 'test-model' && pack.revision === RANGES_REVISION);
  assert.deepEqual(Object.keys(pack.profiles).map(Number).sort(), [...ACTORS.good, ...ACTORS.again].sort(), 'every actor of the scripts profiled');
  const read = readPack(pack);
  assert.ok(read.ok && read.pack.size === 3, 'the page\'s check passes');
  // GOOD's numbers into the ranges: speed 1.2 to 1.15, steps the most, no salt.
  assert.deepEqual(pack.profiles[ACTORS.good[0]], { mix: [{ style: 'M3', w: 0.7 }, { style: 'M5', w: 0.3 }], speed: 1.15, pitch: -1.5, steps: 16, seedSalt: 0 });
  assert.ok(!JSON.stringify(pack).includes(SENTINEL), 'no note or name in the pack');
  // The notes: one per script profiled.
  const notes = JSON.parse(await readFile(join(f.out, 'notes.json'), 'utf8'));
  assert.equal(notes.format, 'u6.voice-notes/1');
  assert.deepEqual(notes.notes.map((n) => [n.archive, n.item, n.note]), [['a', ITEMS[0], `gruff ${SENTINEL}`], ['a', ITEMS[1], `gruff ${SENTINEL}`]]);
  // The coverage: every script read is profiled, failed or skipped, with its reason.
  const c = JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8'));
  assert.deepEqual(c.scripts, { read: 5, profiled: 2, failed: 1, skipped: 2 });
  assert.deepEqual(c.actors, { inScripts: 5, profiled: 3, inFailed: 1, inSkipped: 1, repeated: 0 });
  assert.equal(c.unexplained, 0);
  assert.deepEqual(c.failures, [{ archive: 'b', item: ITEMS[2], actors: 1, reason: 'invalid' }]);
  assert.deepEqual(c.skipped, [{ archive: 'b', item: ITEMS[3], actors: 1, reason: 'refused' }, { archive: 'b', item: ITEMS[4], actors: 0, reason: 'no-actors' }]);
  assert.ok(!JSON.stringify(c).includes(SENTINEL), 'counts and numbers only');
  for (const name of ['voices.u6vp', 'notes.json', 'coverage.json']) assert.equal(await mode(join(f.out, name)), 0o600, `${name} is 0600`);
  // What it printed: counts, timings, paths and sizes.
  assert.match(out.text, /Sent: 5 requests \(2 asked again\) in [0-9.]+ s; tokens: input 500, output 100 \(5 answers counted\)\./);
  assert.match(out.text, /Profiles: 2 scripts, 3 actors; failed 1 scripts \(invalid 1\); skipped 2; unexplained 0\./);
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

// Spec 08's slice K2 adds optional fields to a bundle's table strings (`slots`, `capacity`,
// `inner`): bundles that carry them are read as before, request for request, byte for byte.
test('bundles whose table strings carry the optional slots, capacity and inner fields are read as before: the same requests, the same pack', async () => {
  const run = async (bundles) => {
    const dir = await mkdtemp(join(tmpdir(), 'u6-voice-profile-k2-'));
    const path = join(dir, 'ultima6-npc-bundles.json');
    await writeFile(path, JSON.stringify(bundles));
    const t = fakeTransport();
    const out = output();
    const code = await main([path, '--out', join(dir, 'voice'), '--send', '--model', 'test-model', '--jobs', '1'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() });
    const pack = await readFile(join(dir, 'voice', 'voices.u6vp'), 'utf8');
    await rm(dir, { recursive: true });
    return { code, bodies: t.bodies, pack };
  };
  const plain = await run(BUNDLES);
  const marked = BUNDLES.map((b) => ({
    ...b,
    strings: [
      { key: [b.archive, b.item, 300], table: 300, index: 0, text: 'red' },
      { key: [b.archive, b.item, 304], table: 300, index: 1, text: 'blue', slots: [1], capacity: 99, inner: [306] },
    ],
  }));
  const k2 = await run(marked);
  assert.equal(k2.code, plain.code);
  assert.deepEqual(k2.bodies, plain.bodies, 'the same requests');
  assert.equal(k2.pack, plain.pack, 'the same pack');
});

test('SIGINT while sending: the request in flight is dropped and none more is sent; what was answered is written', async () => {
  const f = await fixture();
  const signals = new EventEmitter();
  const bodies = [];
  // The first request is answered; the second waits until it is aborted.
  const transport = (body, { signal }) => {
    bodies.push(body);
    if (bodies.length === 1) return Promise.resolve({ status: 200, json: { content: [{ type: 'text', text: JSON.stringify(GOOD) }] } });
    setTimeout(() => signals.emit('SIGINT'), 10);
    return new Promise((_, fail) => signal.addEventListener('abort', () => fail(signal.reason)));
  };
  const out = output();
  const code = await main([f.bundles, '--out', f.out, '--send', '--jobs', '1'], { transport, stdout: out, isTTY: false, signals });
  assert.equal(code, 130, 'interrupted');
  assert.equal(bodies.length, 2, 'none sent after the interrupt');
  const c = JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8'));
  assert.deepEqual(c.scripts, { read: 5, profiled: 1, failed: 2, skipped: 2 });
  assert.ok(c.failures.every((x) => x.reason === 'interrupted') && c.unexplained === 0, 'the rest counted as interrupted');
  assert.ok(readPack(JSON.parse(await readFile(join(f.out, 'voices.u6vp'), 'utf8'))).pack.size === 2, 'the answered script\'s actors');
  assert.equal(signals.listenerCount('SIGINT'), 0, 'its SIGINT handler gone');
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

test('each request is bounded: the name, the description and lines up to the budget', async () => {
  const { npcText, TEXT_BUDGET, LINE_MAX } = await import('./profile.mjs');
  const long = Array.from({ length: 40 }, (_, i) => `${'word '.repeat(60)}${i}`);
  const npc = npcText(bundle('a', 1, [1], 'Nm', { lines: long }));
  assert.ok(npc.chars <= TEXT_BUDGET, 'within the budget');
  assert.ok(npc.text.split('\n').every((l) => l.length <= LINE_MAX + 16), 'each line cut');
  assert.ok(npc.text.startsWith('Name: Nm\nDescribed as: a person called Nm\nWhat they say:\n- Hello, I am Nm.'), 'the name, the description, the greeting first');
  assert.ok(!npc.text.includes('39'), 'the lines past the budget left out');
});

// --- the CLI in a child process, against a counting HTTP server -------------------------------

// A local server counting what reaches it, answering each POST with GOOD; `close()`.
async function countingServer() {
  const seen = [];
  const server = createServer((req, res) => {
    let body = '';
    req.on('data', (d) => { body += d; });
    req.on('end', () => {
      seen.push({ method: req.method, url: req.url, body });
      res.setHeader('content-type', 'application/json');
      res.end(JSON.stringify({ type: 'message', content: [{ type: 'text', text: JSON.stringify(GOOD) }] }));
    });
  });
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  return { seen, url: `http://127.0.0.1:${server.address().port}`, close: () => new Promise((done) => server.close(done)) };
}

// Runs the CLI with `args` (stdin a pipe: no terminal); its exit code and output.
function cli(args) {
  return new Promise((done) => {
    const child = spawn(process.execPath, [TOOL, ...args], { stdio: ['pipe', 'pipe', 'pipe'] });
    let printed = '';
    child.stdout.on('data', (d) => { printed += d; });
    child.stderr.on('data', (d) => { printed += d; });
    child.on('close', (code) => done({ code, printed }));
    child.stdin.end();
  });
}

test('the CLI: no terminal and no --send reaches no server; --dry-run neither; --send does, and writes the pack', async () => {
  const f = await fixture();
  const server = await countingServer();
  try {
    let r = await cli([f.bundles, '--out', f.out, '--llmux', server.url]);
    assert.equal(r.code, 1);
    assert.equal(server.seen.length, 0, 'no request reached the server');
    nothingTold(r.printed);
    r = await cli([f.bundles, '--out', f.out, '--llmux', server.url, '--dry-run']);
    assert.equal(r.code, 0);
    assert.equal(server.seen.length, 0, 'no request reached the server');
    const preview = await previewChecked(f.out, r.printed);
    r = await cli([f.bundles, '--out', f.out, '--llmux', server.url, '--send', '--jobs', '2']);
    assert.equal(r.code, 0, 'every script profiled');
    assert.equal(server.seen.length, 3, 'one request per script to profile');
    assert.ok(server.seen.every((s) => s.method === 'POST' && s.url === '/v1/messages'), 'llmux\'s Messages call');
    assert.ok(server.seen.some((s) => s.body === preview), 'the preview is one of the requests sent, byte for byte');
    const pack = readPack(JSON.parse(await readFile(join(f.out, 'voices.u6vp'), 'utf8')));
    assert.ok(pack.ok && pack.pack.size === 4, 'the pack: every actor of the scripts profiled');
    nothingTold(r.printed);
    r = await cli([f.bundles, '--out', f.out, '--llmux', 'http://192.0.2.1:3456', '--send']);
    assert.equal(r.code, 2, 'llmux on this machine only');
    assert.equal(server.seen.length, 3);
  } finally {
    await server.close();
    await rm(f.dir, { recursive: true });
  }
});

// A pty for a child process: Python's pty.spawn, which copies a pipe to it (null without python3).
const PTY = spawnSync('python3', ['-c', 'import pty'], { stdio: 'ignore' }).status === 0
  ? ['python3', '-c', 'import pty, sys; sys.exit(pty.spawn(sys.argv[1:]) >> 8)'] : null;

// A local server answering every request with `status` and a Location on `to`.
async function redirectingServer(status, to) {
  const seen = [];
  const server = createServer((req, res) => {
    req.resume();
    req.on('end', () => {
      seen.push(req.url);
      res.writeHead(status, { location: `${to}/v1/messages` });
      res.end();
    });
  });
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  return { seen, url: `http://127.0.0.1:${server.address().port}`, close: () => new Promise((done) => server.close(done)) };
}

test('the CLI: llmux answering 307 or 308 to another server: the request is not followed (the other server gets none), it is a failure, and none more is sent', async () => {
  const f = await fixture();
  const elsewhere = await countingServer();
  try {
    for (const status of [307, 308]) {
      const llmux = await redirectingServer(status, elsewhere.url);
      try {
        const r = await cli([f.bundles, '--out', f.out, '--llmux', llmux.url, '--send', '--jobs', '1']);
        assert.equal(elsewhere.seen.length, 0, `${status}: the place it names got no request`);
        assert.equal(llmux.seen.length, 1, `${status}: one request, not tried again, none after it`);
        assert.equal(r.code, 3, `${status}: no profile made`);
        assert.match(r.printed, new RegExp(`The route refused a request \\(http-${status}\\): no request was sent after it\\.`));
        nothingTold(r.printed);
      } finally {
        await llmux.close();
      }
    }
  } finally {
    await elsewhere.close();
    await rm(f.dir, { recursive: true });
  }
});

test('the CLI on a real terminal (a pty): Ctrl-C at the prompt reaches no server', { skip: PTY === null }, async () => {
  const f = await fixture();
  const server = await countingServer();
  try {
    const child = spawn(PTY[0], [...PTY.slice(1), process.execPath, TOOL, f.bundles, '--out', f.out, '--llmux', server.url], { stdio: ['pipe', 'pipe', 'pipe'] });
    let printed = '';
    const prompted = new Promise((done) => {
      child.stdout.on('data', (d) => {
        printed += d;
        if (printed.includes('Type yes')) done();
      });
    });
    const closed = new Promise((done) => child.on('close', (code) => done(code)));
    await prompted;
    child.stdin.write('\x03');
    assert.equal(await closed, 130, 'the exit code of an interrupt');
    assert.match(printed, /Nothing was sent \(interrupted\)\./, 'interrupted at the prompt');
    assert.equal(server.seen.length, 0, 'no request reached the server');
    nothingTold(printed);
  } finally {
    await server.close();
    await rm(f.dir, { recursive: true });
  }
});

// --- spec 15 Design 4-5: the pack /2, a line's profile, the voices from a speaker profile -----

// A made-up voice of a /2 pack (exact binary fractions: the check leaves them as they are).
const VOICE_2 = { mix: [{ style: 'F3', w: 0.75 }, { style: 'F1', w: 0.25 }], speed: 1.0625, pitch: 0.5, steps: 16, seedSalt: 0, vary: { speed: 0.0625, pitch: 1.25 } };
const VOICE_1 = { mix: [{ style: 'M4', w: 1 }], speed: 0.9375, pitch: -1.5, steps: 12, seedSalt: 3 };
const ZERO = { speed: 0, pitch: 0 };
const pack2 = (profiles) => ({ format: PACK_FORMAT_2, model: 'test-model', revision: RANGES_REVISION, profiles });
const inRanges = (p) => p.speed >= SPEED.min && p.speed <= SPEED.max && p.pitch >= PITCH.min && p.pitch <= PITCH.max;

test('the voice pack /2: it reads, each voice with its vary; every bad vary refuses the pack by a code that names only the kind; a /1 pack still reads, its voices with vary 0/0, and a /1 voice with a vary is refused; the ranges\' revision is unchanged', () => {
  const read = readPack(structuredClone(pack2({ 7: VOICE_2, 8: { ...VOICE_1, vary: ZERO } })));
  assert.ok(read.ok && read.pack.size === 2, 'a /2 pack reads');
  assert.deepEqual(read.pack.get(7), VOICE_2, 'its voice, its vary, as they are');
  assert.deepEqual(read.pack.get(8), { ...VOICE_1, vary: ZERO });
  const one = readPack({ format: PACK_FORMAT, model: 'test-model', revision: RANGES_REVISION, profiles: { 9: VOICE_1 } });
  assert.ok(one.ok && one.pack.size === 1, 'a /1 pack still reads');
  assert.deepEqual(one.pack.get(9), { ...VOICE_1, vary: ZERO }, 'its voice with vary 0/0');
  // The ranges' revision: the value of main 4320314 (VARY is not in it).
  assert.equal(RANGES_REVISION, 'b6fb2a8d', 'the revision unchanged');
  const vary = (v) => pack2({ 7: { ...VOICE_2, vary: v } });
  const refused = [
    ['format', { ...pack2({ 7: VOICE_2 }), format: 'u6.voice-pack/3' }],
    ['keys', { format: PACK_FORMAT, model: 'test-model', revision: RANGES_REVISION, profiles: { 7: VOICE_2 } }],
    ['shape', pack2({ 7: VOICE_1 })],
    ['shape', vary(null)], ['shape', vary([0, 0])], ['shape', vary(0.1)], ['shape', vary({ speed: 0.1 })], ['shape', vary({ pitch: 0.1 })],
    ['keys', vary({ speed: 0, pitch: 0, x: 0 })],
    ['range', vary({ speed: -0.01, pitch: 0 })], ['range', vary({ speed: VARY.speed.max + 0.01, pitch: 0 })],
    ['range', vary({ speed: 0, pitch: -0.5 })], ['range', vary({ speed: 0, pitch: VARY.pitch.max + 0.01 })],
    ['range', vary({ speed: NaN, pitch: 0 })], ['range', vary({ speed: 0, pitch: Infinity })],
    ['range', vary({ speed: '0.1', pitch: 0 })], ['range', vary({ speed: 0, pitch: null })],
  ];
  for (const [code, value] of refused) {
    const r = readPack(value);
    assert.ok(!r.ok && r.code === code, `refused as ${code}`);
    assert.deepEqual(Object.keys(r), ['ok', 'code'], 'the code alone');
  }
  // checkedProfile keeps a vary, into its range (none: 0/0).
  assert.deepEqual(checkedProfile({ ...VOICE_1, vary: { speed: 9, pitch: -1 } }).vary, { speed: VARY.speed.max, pitch: 0 });
  assert.deepEqual(checkedProfile(VOICE_1).vary, ZERO);
  assert.deepEqual(VARY, { speed: { max: 0.15 }, pitch: { max: 1.5 } });
});

test('a line\'s profile: deterministic per seed; vary 0/0 gives the voice itself; two lines of a mad voice are said at two speeds; always within the ranges', () => {
  const mad = deriveProfile({ age: 'adult', sex: 'male', station: 'commoner', manner: 'mad' }, 40);
  assert.deepEqual(mad.vary, MANNER_VARY.mad);
  assert.deepEqual(lineProfile(mad, 12345), lineProfile(structuredClone(mad), 12345), 'the same seed: the same profile');
  const [a, b] = [lineProfile(mad, 0x9e3779b9), lineProfile(mad, 0x12345678)];
  assert.notEqual(a.speed, b.speed, 'two lines: two speeds');
  assert.notEqual(a.pitch, b.pitch, 'and two pitches');
  assert.deepEqual(a.mix, mad.mix, 'the voice\'s presets');
  assert.ok(Math.abs(a.speed - mad.speed) <= mad.vary.speed && Math.abs(a.pitch - mad.pitch) <= mad.vary.pitch, 'within its vary');
  const plain = deriveProfile({ age: 'adult', sex: 'male', station: 'commoner', manner: 'plain' }, 40);
  assert.equal(lineProfile(plain, 0x9e3779b9), plain, 'vary 0/0: the voice itself');
  assert.equal(lineProfile(VOICE_1, 5), VOICE_1, 'no vary: the voice itself');
  // Within the ranges, from voices at their edges.
  const speeds = new Set();
  for (const edge of [{ speed: SPEED.max, pitch: PITCH.max }, { speed: SPEED.min, pitch: PITCH.min }, { speed: 1, pitch: 0 }]) {
    const v = checkedProfile({ ...VOICE_2, ...edge, vary: { speed: VARY.speed.max, pitch: VARY.pitch.max } });
    for (let seed = 0; seed < 2000; seed += 1) {
      const p = lineProfile(v, (seed * 2654435761) >>> 0);
      assert.ok(inRanges(p), 'within the ranges');
      speeds.add(p.speed);
    }
  }
  assert.ok(speeds.size > 1000, 'the lines spread');
  // A worked case: base speed 0.91, vary 0.05. A line drawn at a = -1 (its seed's first draw 0)
  // goes down by the room there, 0.01, to 0.90; one at a = -0.5 half of it, to 0.905 (not held at
  // 0.90); one at a = +1 (less 2^-31: the draw's most) up the whole vary, to 0.96. The pitch
  // (vary 0) stays.
  const DOWN = 3568425090;
  const HALF = 4105105844;
  const UP = 3997851825;
  assert.ok(draw(DOWN, 1) === 0 && draw(HALF, 1) === 0x40000000 && draw(UP, 1) === 0xffffffff, 'the seeds\' first draws');
  const slow = checkedProfile({ ...VOICE_2, speed: 0.91, pitch: 0.5, vary: { speed: 0.05, pitch: 0 } });
  const down = lineProfile(slow, DOWN);
  const up = lineProfile(slow, UP);
  assert.ok(Math.abs(down.speed - 0.9) < 1e-12 && down.speed >= SPEED.min, 'a = -1: at 0.90 (the room below, 0.01)');
  assert.ok(Math.abs(up.speed - 0.96) < 1e-9, 'a = +1: at 0.96 (the whole vary)');
  assert.ok(Math.abs(lineProfile(slow, HALF).speed - 0.905) < 1e-12, 'a = -0.5: at 0.905 (half the room below)');
  assert.ok(down.pitch === 0.5 && up.pitch === 0.5, 'vary 0: the pitch stays');
  assert.equal(slow.speed, 0.91, 'the voice itself unchanged');
  // A base on the range's end: speed 0.90, vary 0.05. A line drawn down (a = -0.5), where there is
  // no room, moves the same distance up: 0.925, as one drawn up (a = +0.5) does.
  const HALF_UP = 4215774887;
  assert.equal(draw(HALF_UP, 1), 0xc0000000, 'its seed\'s first draw');
  const floor = checkedProfile({ ...VOICE_2, speed: 0.9, pitch: 0.5, vary: { speed: 0.05, pitch: 0 } });
  assert.ok(Math.abs(lineProfile(floor, HALF).speed - 0.925) < 1e-12, 'a = -0.5 at the bottom: up to 0.925');
  assert.ok(Math.abs(lineProfile(floor, HALF_UP).speed - 0.925) < 1e-12, 'a = +0.5: up to 0.925');
  assert.ok(lineProfile(floor, DOWN).speed > SPEED.min, 'a = -1 at the bottom: off the end');
});

const STATIONS = Object.keys(STATION_OFFSETS);
const MANNERS = Object.keys(MANNER_OFFSETS);
const AGES = Object.keys(AGE_MEDIANS);
const SEXES = ['female', 'male'];
const ACTORS_N = 256;

test('the derivation\'s tables: the enums of the speaker profile, every number as spec 15 Design 4 gives it', { skip: translate.STATIONS ? false : 'the enums wait on unit S1 (u6.speakers/2)' }, () => {
  assert.deepEqual(STATIONS, translate.STATIONS, 'the stations');
  assert.deepEqual(MANNERS, translate.MANNERS, 'the manners');
  assert.deepEqual(Object.keys(MANNER_VARY), translate.MANNERS, 'vary for every manner');
  assert.deepEqual(AGES, translate.AGES, 'the ages');
  assert.deepEqual(translate.SEXES, [...SEXES, 'none'], 'the sexes');
});

test('deriveProfile: each station and sex leads with its family\'s presets (a young speaker by age, an elderly woman likewise), blending another of the voice\'s sex at 0.6-0.8; a plain commoner gives the medians by age and sex within the spread; the offsets by station and manner; the jester faster and higher than a plain speaker of the same age and sex; vary by manner; neighbours differ; every voice within the ranges and a /2 pack of them reads', () => {
  const leads = (speaker) => new Set(Array.from({ length: ACTORS_N }, (_, n) => deriveProfile(speaker, n).mix[0].style));
  // The lead presets of each family, all of them used across the actors.
  for (const sex of SEXES) {
    for (const station of STATIONS) {
      const family = PRESET_FAMILIES[sex][station];
      assert.deepEqual([...leads({ sex, station, manner: 'plain', age: 'adult' })].sort(), [...family].sort(), `${sex} ${station}: its family's leads`);
    }
    for (const age of ['child', 'youth']) {
      for (const station of STATIONS) {
        assert.deepEqual([...leads({ sex, station, age })].sort(), [...PRESET_FAMILIES[sex].young].sort(), `${sex} ${age} ${station}: the young's`);
      }
    }
  }
  // The tables, against spec 15 Design 4's literals written out.
  assert.deepEqual(PRESET_FAMILIES, {
    male: {
      young: ['M1'],
      royalty: ['M2', 'M5'], lord: ['M2', 'M5'], beyond: ['M2', 'M5'],
      commoner: ['M3', 'M1'],
      lowborn: ['M4'],
    },
    female: {
      young: ['F2', 'F4'],
      elder: ['F5', 'F3'],
      royalty: ['F5', 'F3'], lord: ['F5', 'F3'], beyond: ['F5', 'F3'],
      commoner: ['F1', 'F4'], lowborn: ['F1', 'F4'],
    },
  });
  assert.deepEqual(LEAD_WEIGHT, { min: 0.6, max: 0.8 });
  assert.deepEqual(SPREAD, { speed: 0.02, pitch: 0.3 });
  assert.deepEqual(MANNER_VARY, {
    plain: { speed: 0, pitch: 0 },
    jester: { speed: 0.04, pitch: 0.8 },
    mad: { speed: 0.1, pitch: 1.2 },
    pompous: { speed: 0, pitch: 0 },
    drunk: { speed: 0.05, pitch: 0.4 },
    sly: { speed: 0, pitch: 0 },
    gruff: { speed: 0, pitch: 0 },
    timid: { speed: 0, pitch: 0 },
    pious: { speed: 0, pitch: 0 },
  });
  for (const station of STATIONS) {
    assert.deepEqual([...leads({ sex: 'female', station, age: 'elder' })].sort(), ['F3', 'F5'], `an elderly woman (${station}): F5 or F3`);
    assert.deepEqual([...leads({ sex: 'male', station, age: 'elder' })].sort(), [...PRESET_FAMILIES.male[station]].sort(), `an elderly man (${station}): by station`);
  }
  // `none`, a /1 entry and no entry: the actor number's parity, and the defaults.
  for (const speaker of [{ sex: 'none' }, { age: 'adult', class: 'commoner', note: '' }, null]) {
    assert.equal(traitsOf(speaker, 7).sex, 'female');
    assert.equal(traitsOf(speaker, 8).sex, 'male');
    assert.deepEqual(familyOf(traitsOf(speaker, 7)), PRESET_FAMILIES.female[SPEAKER_DEFAULTS.station]);
  }
  assert.deepEqual(deriveProfile(null, 9), deriveProfile({ sex: 'female', age: 'adult', station: 'commoner', manner: 'plain' }, 9), 'no entry: the defaults');
  assert.deepEqual(deriveProfile({ age: 'elder', class: 'noble', note: '' }, 10), deriveProfile({ sex: 'male', age: 'elder', station: 'commoner', manner: 'plain' }, 10), 'a /1 entry: its age, the other defaults');
  // The blend.
  for (let n = 0; n < ACTORS_N; n += 1) {
    const p = deriveProfile({ sex: n % 3 === 0 ? 'female' : 'male', station: STATIONS[n % 5], age: AGES[n % 4] }, n);
    assert.equal(p.mix.length, 2, 'two presets');
    assert.ok(p.mix[0].w >= LEAD_WEIGHT.min - 1e-12 && p.mix[0].w <= LEAD_WEIGHT.max + 1e-12, 'the lead at 0.6-0.8');
    assert.ok(p.mix[1].style[0] === p.mix[0].style[0] && p.mix[1].style !== p.mix[0].style, 'another of the voice\'s sex');
  }
  // The medians by age and sex: a plain commoner within the spread of them, their mean the median.
  assert.deepEqual(AGE_MEDIANS, {
    child: { female: { speed: 1.1, pitch: 2 }, male: { speed: 1.1, pitch: 2 } },
    youth: { female: { speed: 0.98, pitch: 0.5 }, male: { speed: 1.07, pitch: 0.9 } },
    adult: { female: { speed: 1, pitch: -0.5 }, male: { speed: 0.98, pitch: -1 } },
    elder: { female: { speed: 0.945, pitch: -1 }, male: { speed: 0.96, pitch: -1 } },
  });
  const eps = 1e-9;
  for (const age of AGES) {
    for (const sex of SEXES) {
      const m = AGE_MEDIANS[age][sex];
      const ps = Array.from({ length: ACTORS_N }, (_, n) => deriveProfile({ sex, age, station: 'commoner', manner: 'plain' }, n));
      for (const p of ps) {
        assert.ok(Math.abs(p.speed - m.speed) <= SPREAD.speed + eps, `${age} ${sex}: the speed within the spread`);
        const top = Math.min(PITCH.max, m.pitch + SPREAD.pitch);
        assert.ok(p.pitch >= m.pitch - SPREAD.pitch - eps && p.pitch <= top + eps, `${age} ${sex}: the pitch within the spread (into the range)`);
      }
      const mean = (k) => ps.reduce((x, p) => x + p[k], 0) / ps.length;
      assert.ok(Math.abs(mean('speed') - m.speed) < 0.004, `${age} ${sex}: the mean speed the median`);
      if (m.pitch + SPREAD.pitch <= PITCH.max) assert.ok(Math.abs(mean('pitch') - m.pitch) < 0.06, `${age} ${sex}: the mean pitch the median`);
    }
  }
  // The offsets: one actor (one spread), each station and manner against a plain commoner.
  assert.deepEqual(STATION_OFFSETS, {
    royalty: { speed: -0.05, pitch: -0.5 }, lord: { speed: -0.03, pitch: -0.3 }, commoner: { speed: 0, pitch: 0 },
    lowborn: { speed: 0.02, pitch: 0 }, beyond: { speed: -0.06, pitch: 0 },
  });
  assert.deepEqual(MANNER_OFFSETS, {
    plain: { speed: 0, pitch: 0 }, jester: { speed: 0.1, pitch: 1 }, mad: { speed: 0.03, pitch: 0 },
    pompous: { speed: -0.04, pitch: -0.4 }, drunk: { speed: -0.07, pitch: 0 }, sly: { speed: 0.02, pitch: 0 },
    gruff: { speed: 0.01, pitch: -0.3 }, timid: { speed: -0.02, pitch: 0.4 }, pious: { speed: -0.03, pitch: 0 },
  });
  const base = { sex: 'female', age: 'adult', station: 'commoner', manner: 'plain' };
  for (let n = 0; n < 32; n += 1) {
    const plain = deriveProfile(base, n);
    for (const [k, table] of [['station', STATION_OFFSETS], ['manner', MANNER_OFFSETS]]) {
      for (const [v, off] of Object.entries(table)) {
        const p = deriveProfile({ ...base, [k]: v }, n);
        assert.ok(Math.abs(p.speed - plain.speed - off.speed) < 1e-9 && Math.abs(p.pitch - plain.pitch - off.pitch) < 1e-9, `${k} ${v}: its offsets`);
        if (k === 'manner') assert.deepEqual(p.mix, plain.mix, `${v}: the manner moves the pace and pitch, not the presets`);
      }
    }
  }
  // The jester: faster and higher than a plain speaker of the same age, sex and station (a child's
  // plain pitch is at the top of the range already: faster only).
  for (const age of AGES) {
    for (const sex of SEXES) {
      for (const station of STATIONS) {
        for (let n = 0; n < 64; n += 1) {
          const plain = deriveProfile({ sex, age, station, manner: 'plain' }, n);
          const jester = deriveProfile({ sex, age, station, manner: 'jester' }, n);
          assert.ok(jester.speed > plain.speed, `${age} ${sex} ${station}: the jester faster`);
          if (age !== 'child') assert.ok(jester.pitch > plain.pitch, `${age} ${sex} ${station}: the jester higher`);
        }
      }
    }
  }
  // Vary by manner (MANNER_VARY pinned above).
  for (const manner of MANNERS) {
    const v = deriveProfile({ ...base, manner }, 3).vary;
    assert.deepEqual(v, MANNER_VARY[manner], `${manner}: its vary`);
    assert.equal(v.speed > 0 && v.pitch > 0, ['jester', 'mad', 'drunk'].includes(manner), `${manner}: varies or not`);
  }
  // Neighbours differ; every voice within the ranges, and a /2 pack of them reads.
  const profiles = {};
  let actor = 0;
  for (const sex of SEXES) {
    for (const age of AGES) {
      for (const station of STATIONS) {
        const mixes = new Set();
        for (let n = 0; n < 64; n += 1) {
          const speaker = { sex, age, station, manner: MANNERS[n % MANNERS.length] };
          const [a, b] = [deriveProfile(speaker, n), deriveProfile(speaker, n + 1)];
          assert.notDeepEqual(a.mix, b.mix, `${sex} ${age} ${station}: neighbours, two mixes`);
          // Two paces and pitches too, but where the range holds one of them at its edge.
          const edge = (x, r) => x === r.min || x === r.max;
          assert.ok(a.speed !== b.speed || edge(a.speed, SPEED), `${sex} ${age} ${station}: neighbours, two paces`);
          assert.ok(a.pitch !== b.pitch || edge(a.pitch, PITCH), `${sex} ${age} ${station}: neighbours, two pitches`);
          assert.ok(inRanges(a), 'within the ranges');
          mixes.add(JSON.stringify(a.mix));
          if (n < 16) { // a pack holds PACK_MAX voices at most
            profiles[actor] = a;
            actor += 1;
          }
        }
        assert.equal(mixes.size, 64, `${sex} ${age} ${station}: no two actors share a mix`);
      }
    }
  }
  const read = readPack(JSON.parse(JSON.stringify(pack2(profiles))));
  assert.ok(read.ok && read.pack.size === actor, 'a /2 pack of them reads');
  assert.ok(STYLES.length === 10);
  console.log(`  receipt: ${2 * AGES.length * STATIONS.length * 64} derived voices within the ranges, ${actor} of them read as a /2 pack`);

  // The varying voices' lines (site/voice/profile.js lineProfile): every station, varying manner,
  // age and sex, actors 0 to 999, 200 lines each. A voice's base is the round-2 sum (the median,
  // the offsets and the spread, into the ranges), untouched by its lines; each line is
  // base + s * |a| * min(vary, the room on side s), a and b the line's two draws, s a's side or,
  // where that side has no room, the other; a line is at a range's end only where |a| is 1 and
  // that side's room is within vary; each voice says its 200 lines at 200 speeds (and pitches),
  // one whose base is at a range's end ("one-sided") all off that end.
  const VARYING = MANNERS.filter((m) => MANNER_VARY[m].speed > 0 || MANNER_VARY[m].pitch > 0);
  assert.deepEqual(VARYING, ['jester', 'mad', 'drunk']);
  const into = (x, r) => Math.min(r.max, Math.max(r.min, x));
  // The side a draw `t` moves a line of `base` to (-1 or 1), and the room there.
  const side = (base, t, r) => (t < 0 ? (base - r.min > 0 ? -1 : 1) : (r.max - base > 0 ? 1 : -1));
  const room = (base, s, r) => Math.max(0, s < 0 ? base - r.min : r.max - base);
  const sum = (t, n) => {
    const m = AGE_MEDIANS[t.age][t.sex];
    const [st, ma] = [STATION_OFFSETS[t.station], MANNER_OFFSETS[t.manner]];
    const spread = (k, size) => size * (2 * unit(draw(n, k)) - 1);
    return { speed: m.speed + st.speed + ma.speed + spread(14, SPREAD.speed), pitch: m.pitch + st.pitch + ma.pitch + spread(15, SPREAD.pitch) };
  };
  const counted = { voices: 0, lines: 0, misses: 0, bases: 0, atEnd: 0, heldAtEnd: 0, duplicates: 0, oneSided: 0, onEnd: 0 };
  for (const station of STATIONS) {
    for (const manner of VARYING) {
      const v = MANNER_VARY[manner];
      for (const age of AGES) {
        for (const sex of SEXES) {
          for (let n = 0; n < 1000; n += 1) {
            const t = { sex, age, station, manner };
            const p = deriveProfile(t, n);
            const raw = sum(t, n);
            if (p.speed !== into(raw.speed, SPEED) || p.pitch !== into(raw.pitch, PITCH)) counted.bases += 1;
            const before = JSON.stringify(p);
            const seen = { speed: new Set(), pitch: new Set() };
            const end = { speed: p.speed === SPEED.min || p.speed === SPEED.max, pitch: p.pitch === PITCH.min || p.pitch === PITCH.max };
            if (end.speed || end.pitch) counted.oneSided += 1;
            for (let seed = 0; seed < 200; seed += 1) {
              const line = (Math.imul(seed + 1, 2654435761) ^ n) >>> 0;
              const l = lineProfile(p, line);
              const draws = { speed: 2 * unit(draw(line, 1)) - 1, pitch: 2 * unit(draw(line, 2)) - 1 };
              for (const [k, r] of [['speed', SPEED], ['pitch', PITCH]]) {
                const d = draws[k];
                const s = side(p[k], d, r);
                const want = Math.min(r.max, Math.max(r.min, p[k] + s * Math.abs(d) * Math.min(v[k], room(p[k], s, r))));
                if (l[k] !== want) counted.misses += 1;
                if (l[k] === r.min || l[k] === r.max) {
                  counted.atEnd += 1;
                  if (Math.abs(d) !== 1 || room(p[k], s, r) > v[k]) counted.heldAtEnd += 1;
                  if (end[k] && l[k] === p[k]) counted.onEnd += 1;
                }
                seen[k].add(l[k]);
              }
              counted.lines += 1;
            }
            for (const k of ['speed', 'pitch']) counted.duplicates += 200 - seen[k].size;
            if (JSON.stringify(p) !== before) counted.bases += 1;
            counted.voices += 1;
          }
        }
      }
    }
  }
  assert.equal(counted.misses, 0, 'every line: base + s * |a| * min(vary, room on side s)');
  assert.equal(counted.heldAtEnd, 0, 'a line at a range\'s end only at |a| = 1 with that side\'s room within vary');
  assert.equal(counted.onEnd, 0, 'a one-sided voice: no line at its end');
  assert.equal(counted.duplicates, 0, 'each voice: its lines all apart');
  assert.equal(counted.bases, 0, 'each base the round-2 sum, untouched by its lines');
  assert.ok(counted.oneSided > 0, 'voices at a range\'s end met too');
  console.log(`  receipt: ${counted.voices} varying voices (${counted.oneSided} at a range's end), ${counted.lines} lines as the room allows; ${counted.atEnd} line values at a range's end; ${counted.duplicates} duplicates`);

  // A voice whose vary is 0 is as before: the hash of every station, age, sex and still manner,
  // actors 0 to 255 (the six manners with vary 0), taken on round 2's code.
  const STILL = ['plain', 'pompous', 'sly', 'gruff', 'timid', 'pious'];
  assert.deepEqual(STILL, MANNERS.filter((m) => !VARYING.includes(m)));
  const still = [];
  for (const station of STATIONS) {
    for (const age of AGES) {
      for (const sex of SEXES) {
        for (const manner of STILL) {
          for (let n = 0; n < 256; n += 1) still.push(deriveProfile({ sex, age, station, manner }, n));
        }
      }
    }
  }
  assert.equal(still.length, 61440);
  assert.equal(createHash('sha256').update(JSON.stringify(still)).digest('hex'),
    'a439d8f6f4f9a86f6a3a504bba7a2d3d5287d1830aa20fbea655d2876ae28df9', 'the voices with vary 0 unchanged');
});

test('speakerOutputsOf (the speaker profile\'s values as tools/i18n/translate.mjs readSpeakers gives them, /2 and /1): each actor its derived voice, the pack /2; the counts by station, manner, sex and age, the means, the varying, the distinct mixes and the scripts not listed', () => {
  const plan = {
    scripts: 4,
    jobs: [
      { archive: 'a', item: 1, actors: [10, 11] },
      { archive: 'a', item: 2, actors: [13] },
      { archive: 'b', item: 3, actors: [14, 10] },
    ],
    skipped: [{ archive: 'b', item: 4, actors: [15], reason: 'refused' }],
  };
  const lord = { age: 'elder', class: 'noble', note: '', sex: 'male', station: 'lord', manner: 'pompous', nameKo: null };
  const one = { age: 'youth', class: 'thief', note: '' };
  const speakers = new Map([['a:1', lord], ['b:3', one], ['b:9', lord]]);
  const { pack, coverage: c } = speakerOutputsOf(plan, speakers, 'u6.speakers/2');
  assert.deepEqual(Object.keys(pack), ['format', 'model', 'revision', 'profiles']);
  assert.ok(pack.format === PACK_FORMAT_2 && pack.model === SPEAKERS_MODEL && pack.revision === RANGES_REVISION);
  assert.deepEqual(pack.profiles, { 10: deriveProfile(lord, 10), 11: deriveProfile(lord, 11), 13: deriveProfile(null, 13), 14: deriveProfile(one, 14) }, 'each actor its voice (10 from its first script)');
  assert.ok(readPack(JSON.parse(JSON.stringify(pack))).ok, 'the pack reads');
  assert.deepEqual(c.scripts, { read: 4, profiled: 3, listed: 2, unlisted: 1, skipped: 1 });
  assert.deepEqual(c.actors, { inScripts: 6, profiled: 4, inSkipped: 1, repeated: 1, defaulted: { unlisted: 1, unstated: 1 } });
  assert.deepEqual(c.by.station, { royalty: 0, lord: 2, commoner: 2, lowborn: 0, beyond: 0 });
  assert.deepEqual(c.by.manner, { plain: 2, jester: 0, mad: 0, pompous: 2, drunk: 0, sly: 0, gruff: 0, timid: 0, pious: 0 });
  assert.deepEqual(c.by.sex, { female: 1, male: 3 }, '13 by parity female; 14 by parity male');
  assert.deepEqual(c.by.age, { child: 0, youth: 1, adult: 1, elder: 2 });
  assert.equal(c.mean.station.royalty, null);
  assert.ok(Math.abs(c.mean.station.lord.speed - (pack.profiles[10].speed + pack.profiles[11].speed) / 2) < 1e-4);
  assert.equal(c.varying, 0);
  assert.equal(c.distinctMixes, 4);
  assert.deepEqual(c.unlisted, [{ archive: 'a', item: 2, actors: 1 }]);
  assert.deepEqual(c.skipped, [{ archive: 'b', item: 4, actors: 1, reason: 'refused' }]);
  assert.equal(c.format, 'u6.voice-coverage/2');
  assert.equal(c.speakers, 'u6.speakers/2');
});

// A speaker file the test writes: `entries` ({archive, item, ...} of its format) under `format`.
const speakerFile = (format, entries) => JSON.stringify({ format, model: 'test-model', speakers: entries });
// A /1 entry and a /2 entry of the fixture's scripts (made-up classes and notes).
const v1Entry = (archive, item, age) => ({ archive, item, age, class: 'commoner', note: 'a made-up note' });
const v2Entry = (archive, item, age, sex, station, manner) => ({ ...v1Entry(archive, item, age), sex, station, manner, name_ko: null });

// Runs main with --speakers on the fixture: the code, the output, the requests (none expected).
async function speakersRun(f, speakers, extra = []) {
  const path = join(f.dir, 'speakers.json');
  await writeFile(path, speakers);
  const t = fakeTransport();
  const out = output();
  const code = await main([f.bundles, '--speakers', path, '--out', f.out, ...extra], { transport: t.send, stdout: out, isTTY: true, stdin: terminal(), signals: new EventEmitter() });
  return { code, printed: out.text, requests: t.bodies.length };
}

// The pack and coverage a --speakers run wrote (0600), the notes.json planted before it unchanged.
async function speakersWritten(f) {
  for (const name of ['voices.u6vp', 'coverage.json']) assert.equal(await mode(join(f.out, name)), 0o600, `${name} is 0600`);
  assert.equal(await readFile(join(f.out, 'notes.json'), 'utf8'), 'the notes, untouched', 'notes.json left as it is');
  assert.ok(!existsSync(join(f.out, 'preview')) && !existsSync(join(f.out, '.lock')), 'no preview; the lock gone');
  return {
    pack: JSON.parse(await readFile(join(f.out, 'voices.u6vp'), 'utf8')),
    coverage: JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8')),
  };
}

async function speakersFixture() {
  const f = await fixture();
  await mkdir(f.out, { recursive: true });
  await writeFile(join(f.out, 'notes.json'), 'the notes, untouched');
  return f;
}

test('--speakers with a /1 speaker file: no request (no route, no preview, no prompt); the pack /2 of every actor of the scripts to profile, each its derived voice (a listed script\'s age, the defaults for the rest), read by readPack; the coverage\'s counts; notes.json left as it is; counts printed only', async () => {
  const f = await speakersFixture();
  const r = await speakersRun(f, speakerFile('u6.speakers/1', [v1Entry('a', ITEMS[0], 'elder'), v1Entry('b', ITEMS[2], 'child')]));
  assert.equal(r.code, 0, 'done');
  assert.equal(r.requests, 0, 'no request');
  assert.ok(!/Route|Type yes|first request/.test(r.printed), 'no route, no prompt, no preview');
  const { pack, coverage: c } = await speakersWritten(f);
  assert.equal(pack.format, PACK_FORMAT_2);
  const read = readPack(pack);
  assert.ok(read.ok && read.pack.size === 4, 'every actor of the scripts to profile');
  const elder = v1Entry('a', ITEMS[0], 'elder');
  assert.deepEqual(pack.profiles[ACTORS.good[0]], deriveProfile({ age: elder.age, class: elder.class, note: elder.note }, ACTORS.good[0]));
  assert.deepEqual(pack.profiles[ACTORS.again[0]], deriveProfile(null, ACTORS.again[0]), 'not listed: the defaults');
  assert.deepEqual(pack.profiles[ACTORS.bad[0]], deriveProfile({ age: 'child' }, ACTORS.bad[0]));
  assert.deepEqual(c.scripts, { read: 5, profiled: 3, listed: 2, unlisted: 1, skipped: 2 });
  assert.deepEqual(c.actors.defaulted, { unlisted: 1, unstated: 3 });
  assert.deepEqual(c.by.station.commoner, 4);
  assert.deepEqual(c.by.age, { child: 1, youth: 0, adult: 1, elder: 2 });
  assert.deepEqual(c.by.sex, { female: 3, male: 1 }, 'by the actors\' parity');
  assert.equal(c.speakers, 'u6.speakers/1');
  assert.deepEqual(c.unlisted, [{ archive: 'a', item: ITEMS[1], actors: 1 }]);
  assert.match(r.printed, /Bundles: 5 scripts read from .* \(\d+ bytes\): 3 to voice \(4 actors\), 2 skipped \(refused 1, no actors 1\)\./);
  assert.match(r.printed, /Speakers: 2 scripts listed \(u6\.speakers\/1\); of the 3 to voice 2 listed, 1 not \(their 1 actors take the defaults\); 3 actors of listed scripts with no station given\./);
  assert.match(r.printed, /Voices: 4 actors \(0 repeated\), 4 distinct mixes, 0 whose lines vary\./);
  assert.match(r.printed, /by station: royalty 0, lord 0, commoner 4, lowborn 0, beyond 0/);
  assert.match(r.printed, /Wrote .*voices\.u6vp \(\d+ bytes\), .*coverage\.json \(\d+ bytes\)\./);
  nothingTold(r.printed);
  await rm(f.dir, { recursive: true });
});

// translate.mjs readSpeakers reads u6.speakers/2 from unit S1 on (spec 15 Design 1; it exports
// SPEAKERS_FORMAT_2 with it): before it, a /2 file is refused there, and this test waits.
const S1_READER = translate.SPEAKERS_FORMAT_2 === 'u6.speakers/2' ? false : 'waits on unit S1 (translate.mjs readSpeakers of u6.speakers/2)';

test('--speakers with a /2 speaker file (unit S1\'s reader): each listed script\'s sex, station and manner give its actors\' voices, a mad one\'s lines vary; the pack reads as /2; the coverage counts by station, manner, sex and age', { skip: S1_READER }, async () => {
  const f = await speakersFixture();
  const lord = v2Entry('a', ITEMS[0], 'elder', 'male', 'lord', 'pompous');
  const mad = v2Entry('b', ITEMS[2], 'adult', 'female', 'lowborn', 'mad');
  const r = await speakersRun(f, speakerFile('u6.speakers/2', [lord, mad, v2Entry('b', 7099, 'adult', 'none', 'beyond', 'pious')]));
  assert.equal(r.code, 0, 'done');
  assert.equal(r.requests, 0, 'no request');
  const { pack, coverage: c } = await speakersWritten(f);
  const read = readPack(pack);
  assert.ok(pack.format === PACK_FORMAT_2 && read.ok && read.pack.size === 4, 'the pack reads as /2');
  const traits = (e) => ({ age: e.age, sex: e.sex, station: e.station, manner: e.manner });
  for (const a of ACTORS.good) assert.deepEqual(pack.profiles[a], deriveProfile(traits(lord), a), 'the lord\'s voice');
  assert.deepEqual(pack.profiles[ACTORS.bad[0]], deriveProfile(traits(mad), ACTORS.bad[0]), 'the madwoman\'s');
  assert.deepEqual(pack.profiles[ACTORS.bad[0]].vary, MANNER_VARY.mad, 'her lines vary');
  assert.deepEqual(c.scripts, { read: 5, profiled: 3, listed: 2, unlisted: 1, skipped: 2 });
  assert.deepEqual(c.by.station, { royalty: 0, lord: 2, commoner: 1, lowborn: 1, beyond: 0 });
  assert.deepEqual(c.by.manner, { plain: 1, jester: 0, mad: 1, pompous: 2, drunk: 0, sly: 0, gruff: 0, timid: 0, pious: 0 });
  assert.deepEqual(c.by.sex, { female: 2, male: 2 });
  assert.deepEqual(c.by.age, { child: 0, youth: 0, adult: 2, elder: 2 });
  assert.equal(c.varying, 1);
  assert.deepEqual(c.actors.defaulted, { unlisted: 1, unstated: 0 });
  assert.equal(c.speakers, 'u6.speakers/2');
  nothingTold(r.printed);
  await rm(f.dir, { recursive: true });
});

test('--speakers refuses with nothing written: a request option with it (no request either), a speakers file that is missing, not JSON or not a speaker profile; the line names the rule only', async () => {
  const f = await speakersFixture();
  for (const extra of [['--send'], ['--dry-run'], ['--model', 'x'], ['--llmux', 'http://127.0.0.1:1'], ['--jobs', '1'], ['--timeout', '5']]) {
    const r = await speakersRun(f, speakerFile('u6.speakers/1', []), extra);
    assert.equal(r.code, 2, `${extra[0]}: a usage error`);
    assert.equal(r.requests, 0, `${extra[0]}: no request`);
    assert.ok(r.printed.includes(`profile: --speakers makes no request: ${extra[0]} does not go with it`), `${extra[0]}: says why`);
  }
  const missing = output();
  assert.equal(await main([f.bundles, '--speakers', join(f.dir, 'none.json'), '--out', f.out], { stdout: missing, isTTY: false, signals: new EventEmitter() }), 2);
  assert.match(missing.text, /profile: the speakers file cannot be read \(ENOENT\): nothing was written\./);
  for (const [body, why] of [
    [`not JSON ${SENTINEL}`, /the speakers file does not read as JSON: nothing was written\./],
    [JSON.stringify({ format: 'u6.speakers/9', model: 'm', speakers: [] }), /the speakers file is not u6\.speakers\/[12]: nothing was written\./],
    [speakerFile('u6.speakers/1', [{ ...v1Entry('a', ITEMS[0], 'adult'), age: SENTINEL }]), /the speakers file is not u6\.speakers\/[12].*: nothing was written\./],
  ]) {
    const r = await speakersRun(f, body);
    assert.equal(r.code, 2, 'refused');
    assert.match(r.printed, why);
    nothingTold(r.printed);
  }
  assert.ok(!existsSync(join(f.out, 'voices.u6vp')) && !existsSync(join(f.out, 'coverage.json')), 'nothing written');
  assert.equal(await readFile(join(f.out, 'notes.json'), 'utf8'), 'the notes, untouched');
  await rm(f.dir, { recursive: true });
});

// A script whose bundle has actors but no name and no text: nothing to send, a voice all the same.
const silentBundle = () => ({
  ...bundle('b', ITEMS[5], ACTORS.silent, ''), narration: { look: [] }, speech: { greeting: [] }, turns: [],
});

test('--speakers voices every script with actors the exporter did not refuse, one with no name and no text too (the model\'s plan still skips it, its printed line as before): its actor in the pack, from the speaker file or the defaults', async () => {
  const value = [...BUNDLES, silentBundle()];
  // The model's plan: the silent script skipped as before (nothing to send).
  const model = planOf(value, 'test-model');
  assert.deepEqual(model.skipped.map((x) => [x.item, x.reason]), [[ITEMS[3], 'refused'], [ITEMS[4], 'no-actors'], [ITEMS[5], 'no-text']]);
  assert.ok(!model.jobs.some((j) => j.item === ITEMS[5]), 'no request for it');
  // The speakers' plan: it is voiced; refused and actorless scripts are still skipped.
  const voiced = speakerPlanOf(value);
  assert.deepEqual(voiced.jobs.map((j) => j.item), [ITEMS[0], ITEMS[1], ITEMS[2], ITEMS[5]]);
  assert.deepEqual(voiced.skipped.map((x) => [x.item, x.reason]), [[ITEMS[3], 'refused'], [ITEMS[4], 'no-actors']]);
  assert.ok(voiced.jobs.every((j) => Object.keys(j).join() === 'archive,item,actors'), 'no text and no request body');
  const f = await speakersFixture();
  await writeFile(f.bundles, JSON.stringify(value));
  // The model's dry run prints its line as before (no text 1).
  const dry = output();
  assert.equal(await main([f.bundles, '--out', join(f.dir, 'model'), '--dry-run'], { stdout: dry, isTTY: false, signals: new EventEmitter() }), 0);
  assert.match(dry.text, /: 3 to profile \(4 actors\), 3 skipped \(refused 1, no actors 1, no text 1\)\./);
  // Listed (/1) and not listed: its actor voiced either way.
  for (const entries of [[v1Entry('b', ITEMS[5], 'elder')], []]) {
    const r = await speakersRun(f, speakerFile('u6.speakers/1', entries));
    assert.equal(r.code, 0);
    assert.equal(r.requests, 0, 'no request');
    const { pack, coverage: c } = await speakersWritten(f);
    const a = ACTORS.silent[0];
    assert.deepEqual(pack.profiles[a], deriveProfile(entries.length > 0 ? { age: 'elder', class: 'commoner', note: 'a made-up note' } : null, a), 'its actor voiced');
    assert.ok(readPack(pack).ok && readPack(pack).pack.size === 5, 'every actor of the scripts voiced');
    assert.deepEqual(c.scripts, { read: 6, profiled: 4, listed: entries.length, unlisted: 4 - entries.length, skipped: 2 });
    assert.match(r.printed, /: 4 to voice \(5 actors\), 2 skipped \(refused 1, no actors 1\)\./);
    nothingTold(r.printed);
  }
  nothingTold(dry.text);
  await rm(f.dir, { recursive: true });
});

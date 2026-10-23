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
//   read past the preview's path, whose pid and random part may hold one by chance).
//
//   node --test tools/speech/profile.test.mjs
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { existsSync } from 'node:fs';
import { chmod, lstat, mkdir, mkdtemp, readdir, readFile, readlink, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { PassThrough } from 'node:stream';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { PACK_FORMAT, RANGES_REVISION, readPack } from '../../site/voice/profile.js';
import { main, PREVIEW, SYSTEM, writePrivate } from './profile.mjs';

const TOOL = join(dirname(fileURLToPath(import.meta.url)), 'profile.mjs');
const SENTINEL = 'Zq6VoiceSentinel';
const LINE = 'Zq6LineSentinel';
// Distinctive numbers: the actors and items of the fixture, never to be printed.
const ACTORS = { good: [31337, 31338], again: [31339], bad: [31341], skipped: [31343] };
const ITEMS = [7001, 7002, 7003, 7004, 7005];

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

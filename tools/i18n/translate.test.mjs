// Tests of tools/i18n/translate.mjs (spec 08 Design 6 and Rights, slice K1) on synthetic bundles
// the test writes (no game data), with a counting fake transport, in a directory of its own
// (never ~/Games/ultima6):
//
// - zero requests: no terminal and no --send, --dry-run (with --send too), and on a terminal a
//   refusal, an empty answer, another case of yes, the input's end, Ctrl-C at the prompt and
//   SIGINT; the preview is the exact first payload, 0600 in a 0700 directory, under a name of the
//   run's own; a typed yes sends (so the prompt is the gate, not a dead end);
// - two runs on one output directory: the second refuses while the first holds the lock and
//   leaves its preview alone; the pack is written through a new file, never through a link;
// - with --send: one request per script, the pack's form (the magic, the JSON the engine reads,
//   each run's hash that of its source, the fingerprint of the scripts translated), a line that
//   fails the gates asked again once in a request of its own, an answer that never reads left
//   English and counted, the coverage receipt reconciled, the files 0600;
// - the gates one by one, the escapes as the disassembler splits them, the fingerprint's test
//   vector shared with the engine's test;
// - the CLI itself in a child process against a counting HTTP server (the real transport): no
//   terminal and no --send sends nothing, --dry-run neither, --send does and writes the pack, a
//   307 or 308 to another server is not followed;
// - nothing the tool prints holds a name, a line, a script's item or a hash.
//
//   node --test tools/i18n/*.test.mjs
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { existsSync } from 'node:fs';
import { chmod, lstat, mkdir, mkdtemp, readdir, readFile, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { PassThrough } from 'node:stream';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  CLASSES, COVERAGE_FORMAT, MAX_TOKENS, PACK_FORMAT, braceWords, fingerprint, gate, hash64, hex64,
  keywordsOf, main, packBytes, placeholders, speakerClass, unitsOf,
} from './translate.mjs';

const TOOL = join(dirname(fileURLToPath(import.meta.url)), 'translate.mjs');
const SENTINEL = 'Zq8NameSentinel';
const LINE = 'Zq8LineSentinel';
const ITEMS = [7001, 7002, 7003, 7004];
const HASHES = ['00000000000a7001', '00000000000a7002', '00000000000b7003', '00000000000b7004'];

const seg = (archive, item, offset, text, part) => ({ kind: 'text', key: [archive, item, offset], text, ...(part === undefined ? {} : { part_of: part }) });

// A u6.npc-bundle/1 as the page's exporter writes it, with the test's own name and lines (the
// keys are made-up offsets in script order).
function bundle(archive, item, hash, name, { refused = null, look = 'a guard of the gate', lines = [] } = {}) {
  const at = (n) => 100 + n * 40;
  return {
    format: 'u6.npc-bundle/1', archive, item, hash, refused, actors: [5], portraits: [], name, locale: 'en',
    narration: { look: refused ? [] : [seg(archive, item, 20, look)] },
    speech: { greeting: refused ? [] : [seg(archive, item, at(0), `"Hail, $G! I am $N. ${LINE}"\n`)] },
    turns: refused ? [] : [{
      input: { at: 90, kind: 'ask' }, segments: [],
      topics: lines.map((l, i) => ({ at: 0, keywords: ['job'], requires: [], answer: [seg(archive, item, at(i + 1), l)] })),
    }],
    traits: [], strings: [],
  };
}

const BUNDLES = [
  bundle('a', ITEMS[0], HASHES[0], `${SENTINEL}Good`, {
    lines: ['My @job is to guard.*', 'I have #0 coin\\s, <runes> here.', 'The first part ', 'and the rest.'],
  }),
  bundle('a', ITEMS[1], HASHES[1], `${SENTINEL}Again`, { look: 'a merchant of bread', lines: ['Buy from me, $P.'] }),
  bundle('b', ITEMS[2], HASHES[2], `${SENTINEL}Bad`, { lines: ['I never answer well.'] }),
  bundle('b', ITEMS[3], HASHES[3], `${SENTINEL}Refused`, { refused: 'test refusal' }),
];
// The third and fourth lines of the first script are the two parts of one run.
BUNDLES[0].turns[0].topics[2].answer[0].part_of = 100 + 3 * 40;
BUNDLES[0].turns[0].topics[3].answer[0].part_of = 100 + 3 * 40;

// The fake translator's Korean for `en`: every escape and the runic text kept, the line breaks at
// its ends kept, each keyword `@word` as `말(@{word})`, every other letter a syllable.
function fakeKo(en) {
  let out = '';
  let runic = false;
  for (let i = 0; i < en.length; i += 1) {
    const c = en[i];
    if (c === '<') runic = true;
    if (c === '>') runic = false;
    if (runic || c === '>' || c === '\n' || c === ' ' || '.,!?"\''.includes(c)) out += c;
    else if ('$#^'.includes(c)) {
      out += c + (en[i + 1] ?? '');
      i += 1;
    } else if (c === '@') {
      const word = /^[a-z]+/i.exec(en.slice(i + 1))[0];
      out += `말(@{${word}})`;
      i += word.length;
    } else if ('*&`/\\%'.includes(c)) {
      out += c;
      if ('/\\'.includes(c)) {
        const letters = /^[a-z]*/i.exec(en.slice(i + 1))[0];
        out += letters;
        i += letters.length;
      }
    } else out += '가';
  }
  return out;
}

const answer = (lines) => ({
  status: 200,
  json: { type: 'message', content: [{ type: 'text', text: JSON.stringify({ lines }) }], usage: { input_tokens: 100, output_tokens: 20 } },
});

// A counting fake transport: the answer for each request from its NPC's name (the bad one is
// never JSON; the "again" one drops an escape the first time, so that line fails the gates).
function fakeTransport() {
  const bodies = [];
  const asked = new Map();
  const send = async (body) => {
    bodies.push(body);
    const user = JSON.parse(JSON.parse(body).messages[0].content);
    const who = user.npc.name;
    asked.set(who, (asked.get(who) ?? 0) + 1);
    if (who.endsWith('Bad')) {
      return { status: 200, json: { type: 'message', content: [{ type: 'text', text: 'I would rather not.' }] } };
    }
    const lines = user.lines.map((l) => ({ n: l.n, ko: fakeKo(l.en) }));
    if (who.endsWith('Again') && asked.get(who) === 1) {
      const last = lines.at(-1);
      last.ko = last.ko.replace('$P', '그대');
    }
    return answer(lines);
  };
  return { send, bodies, asked };
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
  const dir = await mkdtemp(join(tmpdir(), 'u6-i18n-translate-'));
  const bundles = join(dir, 'ultima6-npc-bundles.json');
  await writeFile(bundles, JSON.stringify(BUNDLES));
  return { dir, bundles, out: join(dir, 'ko') };
}

// Nothing the tool printed names an NPC, a line, a script's item or a hash.
function nothingTold(printed) {
  assert.ok(!printed.includes(SENTINEL) && !printed.includes(LINE), 'no name or line printed');
  for (const n of ITEMS) assert.ok(!new RegExp(`(^|[^0-9])${n}([^0-9]|$)`).test(printed), 'no item printed');
  for (const h of HASHES) assert.ok(!printed.includes(h), 'no hash printed');
  assert.ok(!/[가-힣]{3}/.test(printed.replace(/한글/g, '')), 'no Korean line printed');
}

const mode = async (path) => (await stat(path)).mode & 0o777;
const previewPrinted = (printed) => [...printed.matchAll(/The exact first request: (.+) \((\d+) bytes\)\./g)].at(-1)?.[1] ?? null;

// The preview the run printed: the first request's exact bytes, 0600 in a 0700 directory, a name
// of the run's own.
async function previewChecked(out, printed, first = null) {
  const path = previewPrinted(printed);
  assert.ok(path !== null && dirname(path) === join(out, 'preview'), 'the preview\'s path printed');
  assert.match(path.split('/').at(-1), /^first-request-[0-9]+-[0-9a-f]{12}\.json$/, 'a name of the run\'s own');
  assert.equal(await mode(join(out, 'preview')), 0o700, 'the preview directory is 0700');
  assert.equal(await mode(out), 0o700, 'the output directory is 0700');
  assert.equal(await mode(path), 0o600, 'the preview is 0600');
  const preview = await readFile(path, 'utf8');
  const body = JSON.parse(preview);
  assert.deepEqual(Object.keys(body), ['model', 'max_tokens', 'system', 'messages'], 'the model, the cap, the instruction and the lines: nothing else');
  assert.equal(body.max_tokens, MAX_TOKENS);
  assert.ok(body.system.includes('사극체') && body.system.includes('그대'), 'the style guide goes with it');
  const user = JSON.parse(body.messages[0].content);
  assert.equal(user.npc.name, `${SENTINEL}Good`, 'the first script chosen');
  if (first !== null) assert.equal(preview, first, 'the preview is the exact first payload sent');
  return preview;
}

const prompted = (out) => new Promise((done) => { const wait = () => (out.text.includes('Type yes') ? done() : setTimeout(wait, 5)); wait(); });

test('zero requests: no terminal and no --send', async () => {
  const f = await fixture();
  const t = fakeTransport();
  const out = output();
  const code = await main([f.bundles, '--first', '3', '--out', f.out], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() });
  assert.equal(code, 1);
  assert.equal(t.bodies.length, 0, 'no request');
  assert.match(out.text, /Route: llmux on this machine \(http:\/\/127\.0\.0\.1:3456\), then OpenAI \(model gpt-6-astra\)\./);
  assert.match(out.text, /Nothing was sent: there is no terminal to ask on; add --send to send\./);
  await previewChecked(f.out, out.text);
  assert.ok(!existsSync(join(f.out, 'ko.u6lp')), 'no pack');
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

test('zero requests: --dry-run, alone and with --send', async () => {
  const f = await fixture();
  for (const args of [['--dry-run'], ['--dry-run', '--send']]) {
    const t = fakeTransport();
    const out = output();
    const code = await main([f.bundles, '--item', `a:${ITEMS[0]}`, '--out', f.out, ...args], { transport: t.send, stdout: out, isTTY: true, stdin: terminal(), signals: new EventEmitter() });
    assert.equal(code, 0, `${args}: done`);
    assert.equal(t.bodies.length, 0, `${args}: no request`);
    assert.match(out.text, /Dry run: nothing was sent\./);
    await previewChecked(f.out, out.text);
    nothingTold(out.text);
  }
  await rm(f.dir, { recursive: true });
});

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
    const run = main([f.bundles, '--item', `a:${ITEMS[0]}`, '--out', f.out], { transport: t.send, stdout: out, isTTY: true, stdin, signals });
    await prompted(out);
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

test('on a terminal, a typed yes sends exactly the one script named; the preview is the exact first payload', async () => {
  const f = await fixture();
  const t = fakeTransport();
  const out = output();
  const stdin = terminal();
  const run = main([f.bundles, '--item', `a:${ITEMS[0]}`, '--out', f.out], { transport: t.send, stdout: out, isTTY: true, stdin, signals: new EventEmitter() });
  await prompted(out);
  stdin.write('yes\r');
  assert.equal(await run, 0, 'every line translated');
  assert.equal(t.bodies.length, 1, 'one request: the script named');
  await previewChecked(f.out, out.text, t.bodies[0]);
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

test('two runs on one output directory: the second refuses while the first holds the lock and leaves its preview alone; the pack is written through a new file, never through a link planted at its name', async () => {
  const f = await fixture();
  const t = fakeTransport();
  const out = output();
  const stdin = terminal();
  const first = main([f.bundles, '--item', `a:${ITEMS[0]}`, '--out', f.out], { transport: t.send, stdout: out, isTTY: true, stdin, signals: new EventEmitter() });
  await prompted(out);
  const path = previewPrinted(out.text);
  const shown = await readFile(path, 'utf8');
  for (const args of [['--dry-run'], ['--send']]) {
    const t2 = fakeTransport();
    const out2 = output();
    const code = await main([f.bundles, '--first', '2', '--out', f.out, ...args], { transport: t2.send, stdout: out2, isTTY: false, signals: new EventEmitter() });
    assert.equal(code, 2, `${args}: refused`);
    assert.match(out2.text, /another run of a tool holds .*\.lock .*nothing was sent\./, `${args}: says why`);
    assert.equal(t2.bodies.length, 0, `${args}: no request`);
    assert.equal(previewPrinted(out2.text), null, `${args}: no preview of its own`);
    assert.equal(await readFile(path, 'utf8'), shown, `${args}: the first run's preview unchanged`);
  }
  // A link planted where the pack goes: the write replaces the name, the link's target untouched.
  const target = join(f.dir, 'elsewhere.txt');
  await writeFile(target, 'untouched');
  await symlink(target, join(f.out, 'ko.u6lp'));
  stdin.write('yes\r');
  assert.equal(await first, 0);
  assert.equal(t.bodies[0], shown, 'what it sent first is the preview it printed, byte for byte');
  assert.equal(await readFile(target, 'utf8'), 'untouched', 'the link\'s target is not written');
  assert.ok((await lstat(join(f.out, 'ko.u6lp'))).isFile(), 'the pack is a file of its own');
  assert.equal(await mode(join(f.out, 'ko.u6lp')), 0o600);
  assert.ok(!existsSync(join(f.out, '.lock')), 'the lock goes with the run');
  assert.ok((await readdir(f.out)).every((n) => !n.includes('.part-')), 'no temporary file left');
  nothingTold(out.text);
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
    const code = await main([f.bundles, '--item', `a:${ITEMS[0]}`, '--out', f.out, '--send'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() });
    assert.equal(code, 2, `${where}: refused`);
    assert.equal(t.bodies.length, 0, `${where}: no request`);
    const dir = where === 'preview' ? 'preview directory' : 'output directory';
    assert.ok(out.text.includes(`translate: the ${dir} is a symbolic link or not a directory: nothing was sent.`), `${where}: the rule's line`);
    assert.equal(previewPrinted(out.text), null, `${where}: no preview`);
    assert.deepEqual(await seen(), before, `${where}: the target's mode and contents unchanged`);
    if (where === 'preview') assert.ok(!existsSync(join(f.out, '.lock')), 'the lock goes with the run');
    nothingTold(out.text);
    await rm(f.dir, { recursive: true });
  }
});

// The pack file's JSON (after the magic and its line feed).
async function packOf(out) {
  const bytes = await readFile(join(out, 'ko.u6lp'));
  assert.equal(bytes.subarray(0, 5).toString('latin1'), 'U6LP\n', 'the magic and a line feed');
  return JSON.parse(bytes.subarray(5).toString('utf8'));
}

test('with --send: one request per script; the pack holds the lines that pass the gates, each with its source\'s length and hash, over the fingerprint of the scripts translated; a failed line is asked again once on its own; an answer that never reads leaves its script English, counted; the coverage reconciles; the files are 0600', async () => {
  const f = await fixture();
  const t = fakeTransport();
  const out = output();
  const code = await main([f.bundles, '--first', '9', '--out', f.out, '--send', '--jobs', '1'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() });
  assert.equal(code, 3, 'written, with lines left English');
  const asked = Object.fromEntries([...t.asked].map(([k, v]) => [k.replace(SENTINEL, ''), v]));
  assert.deepEqual(asked, { Good: 1, Again: 2, Bad: 2 }, 'each script once; the failed line again; the bad answer again (the transport\'s retry)');
  const retry = JSON.parse(JSON.parse(t.bodies.find((b, i) => i > 0 && b.includes(`${SENTINEL}Again`) && b !== t.bodies[1])).messages[0].content);
  assert.equal(retry.lines.length, 1, 'the second request for Again holds only the failed line');
  const pack = await packOf(f.out);
  assert.deepEqual(Object.keys(pack), ['format', 'language', 'tool', 'base', 'converse']);
  assert.equal(pack.format, PACK_FORMAT);
  assert.equal(pack.language, 'ko');
  const units = [0, 1].flatMap((i) => unitsOf(BUNDLES[i]).map((u) => ({ ...u, item: ITEMS[i] })));
  assert.equal(pack.converse.length, units.length, 'every line of the two good scripts');
  for (const r of pack.converse) {
    const u = units.find((x) => r.archive === 'a' && x.item === r.item && x.offset === r.offset);
    assert.ok(u, 'a run of the bundles');
    assert.equal(r.len, Buffer.from(u.text, 'latin1').length);
    assert.equal(r.hash, hex64(hash64(Buffer.from(u.text, 'latin1'))));
    assert.equal(r.text, fakeKo(u.text));
    assert.equal(gate(u.text, r.text), null);
  }
  assert.equal(pack.base.converse, fingerprint([{ archive: 'a', item: ITEMS[0], hash: HASHES[0] }, { archive: 'a', item: ITEMS[1], hash: HASHES[1] }]), 'the fingerprint of the scripts translated');
  const sorted = [...pack.converse].sort((x, y) => x.item - y.item || x.offset - y.offset);
  assert.deepEqual(pack.converse, sorted, 'in key order');
  const coverage = JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8'));
  assert.equal(coverage.format, COVERAGE_FORMAT);
  const bad = unitsOf(BUNDLES[2]).length;
  assert.deepEqual(coverage.scripts, { read: 4, usable: 3, chosen: 3, translated: 2 });
  assert.deepEqual(coverage.units, { found: units.length + bad, translated: units.length, leftEnglish: bad, byReason: { invalid: bad }, unexplained: 0 });
  assert.ok(JSON.stringify(coverage).includes(HASHES[2]) && !JSON.stringify(coverage).includes(SENTINEL), 'hashes, no text');
  for (const name of ['ko.u6lp', 'coverage.json']) assert.equal(await mode(join(f.out, name)), 0o600, `${name} is 0600`);
  assert.match(out.text, /Lines: \d+ found, \d+ translated, \d+ left English \(invalid \d+\); scripts translated 2 of 3\./);
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

test('a line whose @{…} shows a word its English keywords do not hold fails the gates, is asked again once on its own, then is left English and counted (keywords)', async () => {
  const f = await fixture();
  const b = bundle('a', ITEMS[0], HASHES[0], `${SENTINEL}Baker`, { lines: ['Buy my @bread here.'] });
  await writeFile(f.bundles, JSON.stringify([b]));
  const asked = [];
  const send = async (body) => {
    const user = JSON.parse(JSON.parse(body).messages[0].content);
    asked.push(user.lines.map((l) => l.n));
    return answer(user.lines.map((l) => ({ n: l.n, ko: l.en.includes('@bread') ? '내 빵(@{food})을 사시오.' : fakeKo(l.en) })));
  };
  const out = output();
  const code = await main([f.bundles, '--item', `a:${ITEMS[0]}`, '--out', f.out, '--send'], { transport: send, stdout: out, isTTY: false, signals: new EventEmitter() });
  assert.equal(code, 3, 'written, a line left English');
  const n = unitsOf(b).findIndex((u) => u.text.includes('@bread'));
  assert.equal(asked.length, 2, 'the script, then the failed line again');
  assert.deepEqual(asked[1], [n], 'the second request holds only that line');
  const pack = await packOf(f.out);
  assert.equal(pack.converse.length, unitsOf(b).length - 1, 'every other line in the pack');
  assert.ok(pack.converse.every((r) => !r.text.includes('@{food}')), 'not the wrong keyword');
  const coverage = JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8'));
  assert.deepEqual(coverage.units.byReason, { keywords: 1 }, 'counted by its reason');
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

test('the speaker class: guessed from the name and description (commoner when nothing tells), or the owner\'s --class; the request names it and its register', async () => {
  assert.equal(speakerClass(BUNDLES[0]), 'guard');
  assert.equal(speakerClass(BUNDLES[1]), 'merchant');
  assert.equal(speakerClass({ name: 'X', narration: { look: [seg('a', 1, 1, 'a tall man')] } }), 'commoner');
  assert.equal(speakerClass({ name: 'X', narration: { look: [seg('a', 1, 1, 'a winged gargoyle')] } }), 'gargoyle');
  const f = await fixture();
  const out = output();
  assert.equal(await main([f.bundles, '--item', `a:${ITEMS[0]}`, '--class', 'spirit', '--out', f.out, '--dry-run'], { stdout: out, isTTY: false, signals: new EventEmitter() }), 0);
  const user = JSON.parse(JSON.parse(await readFile(previewPrinted(out.text), 'utf8')).messages[0].content);
  assert.deepEqual([user.npc.class, user.npc.register], ['spirit', CLASSES.spirit]);
  assert.deepEqual(user.lines.map((l) => l.joins ?? false), [false, false, false, false, false, true], 'the second part of the split run joins');
  const bad = output();
  assert.equal(await main([f.bundles, '--item', `a:${ITEMS[0]}`, '--class', 'emperor', '--out', f.out], { stdout: bad, isTTY: false, signals: new EventEmitter() }), 2);
  for (const args of [[], ['--item', 'c:1'], ['--item', 'a:1', '--first', '1'], ['--first', '0']]) {
    const o = output();
    assert.equal(await main([f.bundles, ...args, '--out', f.out], { stdout: o, isTTY: false, signals: new EventEmitter() }), 2, `${args}: a usage error`);
  }
  const missing = output();
  assert.equal(await main([f.bundles, '--item', 'a:1', '--out', f.out], { stdout: missing, isTTY: false, signals: new EventEmitter() }), 2);
  assert.match(missing.text, /the script named is not in the bundles, is refused or has no lines/);
  nothingTold(out.text + bad.text + missing.text);
  await rm(f.dir, { recursive: true });
});

test('the gates: the escapes\' multiset, Hangul and ASCII only, the josa markers and every @{ closed as the text layer reads them, the runic text and the line breaks at the ends', () => {
  assert.deepEqual(placeholders('Hi $G, #0 coin\\s @job* <ab> `x &h& %y ^c $'), ['$G', '#0', '\\', '@', '*', '<', '>', '`', '&', '&', '%', '^c', '$']);
  assert.deepEqual(placeholders('$$x'), ['$', '$x'], 'an escape after a prefix is no argument');
  assert.deepEqual(placeholders('$N{이/가} {(으)로}'), ['$N'], 'a josa marker\'s / is no plural marker');
  assert.deepEqual(placeholders('@{이/가}'), ['@', '/'], 'after @ its { opens a brace');
  assert.deepEqual(keywordsOf('Ask of @Rune2, @job. @@x @'), ['rune2', 'job', '', 'x', ''], 'the letters and digits after each @');
  assert.deepEqual(braceWords('@{Job} 그리고 @{a{이/가}} @x'), ['job', 'a{이/가}'], 'what each @{ holds, a marker\'s } not closing it');
  const ok = [
    ['Hi $N.', '$N{이/가} 왔소.'],
    ['My @job.', '내 일(@{job})이오.'],
    ['#0 coin\\s', '#0닢\\s'],
    ['a <rune> b', '가 <rune> 나'],
    ['&hidden }& a', '&숨은 }& 가'],
    ['\nTwo lines\n', '\n두 줄\n'],
    ['@a', '@{a}{이/가}'],
    ['Buy @bread', '빵(@{Bread})을 사시오'],
    ['@job or @name?', '이름(@{name})이오, 일(@{job})이오?'],
  ];
  for (const [en, ko] of ok) assert.equal(gate(en, ko), null, `${en} -> ${ko}`);
  const bad = [
    ['Hi $N.', '그대 왔소.', 'escapes'],
    ['Hi.', '안녕*', 'escapes'],
    ['Hi.', '안녕 中', 'characters'],
    ['Hi.', '안녕ㄱ', 'characters'],
    ['Hi.', '안녕\t', 'characters'],
    ['Hi.', '안녕{이}', 'markers'],
    ['Hi.', '안녕}', 'markers'],
    ['My @job.', '내 @일이오.', 'braces'],
    ['My @job.', '내 @{일이오.', 'braces'],
    ['Buy @bread', '빵(@{food})을 사시오', 'keywords'],
    ['My @job.', '내 일(@{jobs})이오.', 'keywords'],
    ['@job @job', '일(@{job}) 이름(@{name})', 'keywords'],
    ['@a', '@{a{이/가}}', 'keywords'],
    ['@job', '일(@{일})', 'keywords'],
    ['@a @b', '@{가 @{나}}', 'braces'],
    ['a <rune>', '가 <룬>', 'runic'],
    ['Hi.\n', '안녕.', 'line-breaks'],
    ['Hi.', '\n안녕.', 'line-breaks'],
    ['Hi.', '', 'empty'],
    ['Hi.', '가'.repeat(0x2800 + 1), 'long'],
  ];
  for (const [en, ko, why] of bad) assert.equal(gate(en, ko), why, `${en} -> ${ko}`);
});

test('the hashes and the pack form agree with the engine: FNV-1a 64, the fingerprint\'s test vector (crates/u6-core/src/locale/tests.rs), the runs in key order after U6LP and a line feed', () => {
  assert.equal(hex64(hash64([])), 'cbf29ce484222325');
  assert.equal(hex64(hash64(Buffer.from('a'))), 'af63dc4c8601ec8c');
  assert.equal(fingerprint([{ archive: 'a', item: 3, hash: '0000000000000007' }]), '200c9bb3451c6be0');
  assert.equal(
    fingerprint([{ archive: 'b', item: 1, hash: '0000000000000009' }, { archive: 'a', item: 3, hash: '0000000000000007' }]),
    fingerprint([{ archive: 'a', item: 3, hash: '0000000000000007' }, { archive: 'b', item: 1, hash: '0000000000000009' }, { archive: 'a', item: 3, hash: '0000000000000007' }]),
    'of the set',
  );
  const bytes = packBytes({ base: '0123456789abcdef', runs: [
    { archive: 'b', item: 1, offset: 4, len: 2, hash: '0000000000000001', text: '가' },
    { archive: 'a', item: 9, offset: 8, len: 2, hash: '0000000000000002', text: '나' },
    { archive: 'a', item: 9, offset: 2, len: 2, hash: '0000000000000003', text: '다' },
  ] });
  assert.equal(bytes.subarray(0, 5).toString('latin1'), 'U6LP\n');
  const json = JSON.parse(bytes.subarray(5).toString('utf8'));
  assert.deepEqual(json.converse.map((r) => `${r.archive}${r.item}:${r.offset}`), ['a9:2', 'a9:8', 'b1:4']);
  assert.deepEqual(Object.keys(json.converse[0]), ['archive', 'item', 'offset', 'len', 'hash', 'text']);
});

// --- the CLI in a child process --------------------------------------------------------------

// A local HTTP server that counts requests and answers each as `answer` says ({status, json,
// headers}); `url` once listening.
async function server(answerFor) {
  const seen = [];
  const s = createServer((req, res) => {
    let body = '';
    req.on('data', (d) => { body += d; });
    req.on('end', () => {
      seen.push({ method: req.method, path: req.url, body });
      const a = answerFor(body, seen.length);
      res.writeHead(a.status, { 'content-type': 'application/json', ...(a.headers ?? {}) });
      res.end(a.json === undefined ? '' : JSON.stringify(a.json));
    });
  });
  await new Promise((done) => s.listen(0, '127.0.0.1', done));
  return { seen, url: `http://127.0.0.1:${s.address().port}`, close: () => new Promise((done) => s.close(done)) };
}

// Runs the tool's CLI with `args`, stdin not a terminal; its exit code and output.
function cli(args) {
  return new Promise((done) => {
    const child = spawn(process.execPath, [TOOL, ...args], { stdio: ['ignore', 'pipe', 'pipe'] });
    let text = '';
    child.stdout.on('data', (d) => { text += d; });
    child.stderr.on('data', (d) => { text += d; });
    child.on('close', (code) => done({ code, text }));
  });
}

// The fake translator's answer to a request body, as llmux would give it.
function translated(body) {
  const user = JSON.parse(JSON.parse(body).messages[0].content);
  return answer(user.lines.map((l) => ({ n: l.n, ko: fakeKo(l.en) }))).json;
}

test('the CLI: no terminal and no --send reaches no server; --dry-run neither; --send does, on POST /v1/messages, and writes the pack', async () => {
  const f = await fixture();
  const s = await server((body) => ({ status: 200, json: translated(body) }));
  try {
    const none = await cli([f.bundles, '--item', `a:${ITEMS[0]}`, '--out', f.out, '--llmux', s.url]);
    assert.equal(none.code, 1);
    assert.equal(s.seen.length, 0, 'no request without --send');
    const dry = await cli([f.bundles, '--item', `a:${ITEMS[0]}`, '--out', f.out, '--llmux', s.url, '--dry-run']);
    assert.equal(dry.code, 0);
    assert.equal(s.seen.length, 0, 'no request on a dry run');
    const sent = await cli([f.bundles, '--item', `a:${ITEMS[0]}`, '--out', f.out, '--llmux', s.url, '--send']);
    assert.equal(sent.code, 0, 'sent and written');
    assert.deepEqual(s.seen.map((r) => [r.method, r.path]), [['POST', '/v1/messages']], 'one Messages call');
    assert.equal(s.seen[0].body, await readFile(previewPrinted(sent.text), 'utf8'), 'the preview is what was sent');
    assert.ok((await packOf(f.out)).converse.length > 0, 'the pack');
    assert.match(sent.text, /Sent: 1 requests \(0 asked again\) in \d+\.\d s/);
    nothingTold(none.text + dry.text + sent.text);
  } finally {
    await s.close();
    await rm(f.dir, { recursive: true });
  }
});

test('the CLI: llmux answering 307 or 308 to another server: the request is not followed (the other server gets none), it is a failure, and none more is sent', async () => {
  for (const status of [307, 308]) {
    const f = await fixture();
    const other = await server(() => ({ status: 200, json: {} }));
    const s = await server(() => ({ status, headers: { location: `${other.url}/v1/messages` } }));
    try {
      const r = await cli([f.bundles, '--first', '3', '--out', f.out, '--llmux', s.url, '--send', '--jobs', '1']);
      assert.equal(other.seen.length, 0, `${status}: the other server got nothing`);
      assert.equal(s.seen.length, 1, `${status}: one request, none after the refusal`);
      assert.match(r.text, new RegExp(`The route refused a request \\(http-${status}\\): no request was sent after it\\.`));
      assert.ok(!existsSync(join(f.out, 'ko.u6lp')), `${status}: no pack`);
      nothingTold(r.text);
    } finally {
      await s.close();
      await other.close();
      await rm(f.dir, { recursive: true });
    }
  }
});

test('the CLI refuses an llmux address off this machine before reading anything', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'u6-i18n-route-'));
  await mkdir(join(dir, 'x'));
  const r = await cli([join(dir, 'missing.json'), '--first', '1', '--out', join(dir, 'ko'), '--llmux', 'http://example.com:3456', '--send']);
  assert.equal(r.code, 2);
  assert.match(r.text, /--llmux is the address of llmux on this machine/);
  assert.ok(!existsSync(join(dir, 'ko')), 'nothing made');
  await rm(dir, { recursive: true });
});

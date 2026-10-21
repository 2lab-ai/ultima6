// Tests of tools/i18n/translate.mjs for spec 08's slice K2 (the game strings of the page's second
// export, the conversations' table strings, the merge into the pack already there, --name) on
// synthetic inputs the test writes (no game data), with a counting fake transport or a counting
// local server, in a directory of its own (never ~/Games/ultima6):
//
// - zero requests with --strings (alone and with bundles): no terminal and no --send, --dry-run
//   (with --send too), and on a terminal a refusal, an empty answer, another case of yes, the
//   input's end, Ctrl-C and SIGINT; a symbolic link (or a file) where the output or preview
//   directory goes; another run holding the lock (its preview left alone); the CLI against a
//   counting server without --send and with --dry-run, and a 307 or 308 not followed; a pack
//   already there that does not read, that is English, or that other game files made;
// - with --send: game strings in bounded requests, each checked by the line's gates, the same `%`
//   directives in order and the reader's length; table strings with the script's runs, a copied
//   one held to its capacity and to no marker, one read from inside left English; the merge
//   keeps what the pack had (K1's runs byte for byte) and replaces what it translates again;
//   --name picks one bundle by its NPC's name, case folded, and refuses none or two by count;
// - the reader is the engine's (readPack), the directives are printf's (the engine's test
//   vectors), and nothing the tool prints holds a name, a line, a string, an item or a hash.
//
//   node --test tools/i18n/*.test.mjs
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { existsSync } from 'node:fs';
import { chmod, mkdir, mkdtemp, readdir, readFile, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { PassThrough } from 'node:stream';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  MAX_PACK_BYTES, MAX_RUNS, MAX_STRING_TEXT_BYTES, PackReadError, STRINGS_PER_REQUEST, directivesOf,
  duplicateMembers, fingerprint, hash64, hex64, latin1, main, packBytes, readPack, stringGate, stringsPlanOf,
  tableGate, tablesOf, unitsOf,
} from './translate.mjs';

const TOOL = join(dirname(fileURLToPath(import.meta.url)), 'translate.mjs');
const SENTINEL = 'Zq8StrSentinel';
const NAME = 'Zq8NpcSentinel';
const ITEMS = [7101, 7102];
const HASHES = ['00000000000c7101', '00000000000c7102'];

const hashOf = (text) => hex64(hash64(latin1(text)));
const str = (text, kind) => ({ hash: hashOf(text), len: latin1(text).length, kind, text });

// The page's string export (u6.string-bundle/1) as the engine writes it, with the test's own text.
const STRINGS = [
  str(`\nThou seest %s, ${SENTINEL}.\n`, 'format'),
  str('Naught is there.', 'message'),
  str('Seek', 'label'),
  str('%d stone/s of weight.\n', 'format'),
  str('%s=\n%>', 'format'),
];
const EXPORT = { format: 'u6.string-bundle/1', locale: 'en', strings: STRINGS };

const seg = (archive, item, offset, text) => ({ kind: 'text', key: [archive, item, offset], text });
// A bundle with two runs and table strings: one printed, one copied into slot 1 (capacity 99), one
// an op reads from inside, and an empty one.
function bundle(archive, item, hash, name) {
  return {
    format: 'u6.npc-bundle/1', archive, item, hash, refused: null, actors: [5], portraits: [], name, locale: 'en',
    narration: { look: [seg(archive, item, 20, 'a keeper of tests')] },
    speech: { greeting: [seg(archive, item, 60, `"Hail! I am ${SENTINEL}."`)] },
    turns: [],
    traits: [],
    strings: [
      { key: [archive, item, 200], table: 200, index: 0, text: 'a red stone' },
      { key: [archive, item, 212], table: 200, index: 1, text: 'gold', slots: [1], capacity: 9 },
      { key: [archive, item, 217], table: 200, index: 2, text: 'the rest of it', inner: [221] },
      { key: [archive, item, 232], table: 200, index: 3, text: '' },
    ],
  };
}
const BUNDLES = [bundle('a', ITEMS[0], HASHES[0], `${NAME}Keeper`), bundle('a', ITEMS[1], HASHES[1], `${NAME}Other`)];

// The fake translator's Korean for `en`: the line breaks at its ends, every `%` directive and every
// other escape kept in order, each other letter or word a syllable.
function fakeKo(en) {
  let out = '';
  for (let i = 0; i < en.length; i += 1) {
    const c = en[i];
    if (c === '%') {
      const d = /^%(?:[1-9][0-9]*\$)?[-0]*[0-9]*[\s\S]?/.exec(en.slice(i))[0];
      out += d;
      i += d.length - 1;
    } else if ('$#^'.includes(c)) {
      out += c + (en[i + 1] ?? '');
      i += 1;
    } else if ('/\\'.includes(c)) {
      const letters = /^[a-z]*/i.exec(en.slice(i + 1))[0];
      out += c + letters;
      i += letters.length;
    } else if ('\n *&`<>.,:!"'.includes(c)) out += c;
    else if (/[a-z]/i.test(c) && !/[a-z]/i.test(en[i - 1] ?? '')) out += '가';
    else if (!/[a-z]/i.test(c)) out += '가';
  }
  return out;
}

const answer = (lines) => ({
  status: 200,
  json: { type: 'message', content: [{ type: 'text', text: JSON.stringify({ lines }) }], usage: { input_tokens: 10, output_tokens: 5 } },
});

// A counting fake transport for both kinds of request; `tamper(u, first)` may change a line's
// Korean (the first time it is asked, or again).
function fakeTransport(tamper = () => null) {
  const bodies = [];
  const asked = new Map();
  const send = async (body) => {
    bodies.push(body);
    const user = JSON.parse(JSON.parse(body).messages[0].content);
    const list = user.strings ?? user.lines;
    const lines = list.map((l) => {
      const key = l.en;
      asked.set(key, (asked.get(key) ?? 0) + 1);
      return { n: l.n, ko: tamper(l, asked.get(key) === 1) ?? fakeKo(l.en) };
    });
    return answer(lines);
  };
  return { send, bodies, asked };
}

function terminal() {
  const input = new PassThrough();
  input.isTTY = true;
  input.setRawMode = () => input;
  return input;
}

function output() {
  const out = new PassThrough();
  out.text = '';
  out.on('data', (d) => { out.text += d; });
  return out;
}

async function fixture({ strings = EXPORT, bundles = BUNDLES } = {}) {
  const dir = await mkdtemp(join(tmpdir(), 'u6-i18n-strings-'));
  const s = join(dir, 'ultima6-strings.json');
  const b = join(dir, 'ultima6-npc-bundles.json');
  await writeFile(s, JSON.stringify(strings));
  await writeFile(b, JSON.stringify(bundles));
  return { dir, strings: s, bundles: b, out: join(dir, 'ko') };
}

// Nothing the tool printed names an NPC, a line, a string, a script's item or a hash.
function nothingTold(printed) {
  assert.ok(!printed.includes(SENTINEL) && !printed.includes(NAME), 'no name, line or string printed');
  for (const s of STRINGS) {
    assert.ok(!printed.includes(s.hash), 'no hash printed');
    if (s.text.trim().length >= 6) assert.ok(!printed.includes(s.text.trim()), 'no string printed');
  }
  for (const n of ITEMS) assert.ok(!new RegExp(`(^|[^0-9])${n}([^0-9]|$)`).test(printed), 'no item printed');
  for (const h of HASHES) assert.ok(!printed.includes(h), 'no hash printed');
  assert.ok(!/[가-힣]{2}/.test(printed.replace(/한글/g, '')), 'no Korean line printed');
}

const mode = async (path) => (await stat(path)).mode & 0o777;
const previewPrinted = (printed) => [...printed.matchAll(/The exact first request: (.+) \((\d+) bytes\)\./g)].at(-1)?.[1] ?? null;
const prompted = (out) => new Promise((done) => { const wait = () => (out.text.includes('Type yes') ? done() : setTimeout(wait, 5)); wait(); });

// The two invocations with game strings: alone, and with a script of the bundles.
const VARIANTS = [
  ['strings alone', (f) => ['--strings', f.strings]],
  ['strings and a script', (f) => [f.bundles, '--item', `a:${ITEMS[0]}`, '--strings', f.strings]],
];

for (const [name, args] of VARIANTS) {
  test(`zero requests (${name}): no terminal and no --send; --dry-run alone and with --send`, async () => {
    const f = await fixture();
    const t = fakeTransport();
    const out = output();
    assert.equal(await main([...args(f), '--out', f.out], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() }), 1);
    assert.match(out.text, /Nothing was sent: there is no terminal to ask on; add --send to send\./);
    for (const extra of [['--dry-run'], ['--dry-run', '--send']]) {
      assert.equal(await main([...args(f), '--out', f.out, ...extra], { transport: t.send, stdout: out, isTTY: true, stdin: terminal(), signals: new EventEmitter() }), 0);
    }
    assert.equal(t.bodies.length, 0, 'no request');
    const path = previewPrinted(out.text);
    assert.equal(await mode(path), 0o600);
    assert.equal(await mode(join(f.out, 'preview')), 0o700);
    assert.equal(await mode(f.out), 0o700);
    assert.ok(!existsSync(join(f.out, 'ko.u6lp')), 'no pack');
    nothingTold(out.text);
    await rm(f.dir, { recursive: true });
  });

  for (const [what, keys, want] of [
    ['a refusal', 'no\r', 1],
    ['an empty answer', '\r', 1],
    ['"yes" with another case', 'YES\r', 1],
    ['the input\'s end', '', 1],
    ['Ctrl-C at the prompt', '\x03', 130],
    ['SIGINT', null, 130],
  ]) {
    test(`zero requests on a terminal (${name}): ${what}`, async () => {
      const f = await fixture();
      const t = fakeTransport();
      const out = output();
      const stdin = terminal();
      const signals = new EventEmitter();
      const run = main([...args(f), '--out', f.out], { transport: t.send, stdout: out, isTTY: true, stdin, signals });
      await prompted(out);
      if (keys === null) signals.emit('SIGINT');
      else if (keys === '') stdin.end();
      else stdin.write(keys);
      assert.equal(await run, want);
      assert.equal(t.bodies.length, 0, 'no request');
      assert.equal(signals.listenerCount('SIGINT'), 0);
      nothingTold(out.text);
      await rm(f.dir, { recursive: true });
    });
  }

  test(`zero requests (${name}): a symbolic link or a file where the output or preview directory goes; another run holding the lock, its preview left alone`, async () => {
    for (const where of ['output', 'preview', 'file']) {
      const f = await fixture();
      const target = join(f.dir, 'elsewhere');
      await mkdir(target);
      await chmod(target, 0o755);
      if (where === 'output') await symlink(target, f.out);
      else if (where === 'file') await writeFile(f.out, 'not a directory');
      else {
        await mkdir(f.out, { recursive: true });
        await symlink(target, join(f.out, 'preview'));
      }
      const t = fakeTransport();
      const out = output();
      assert.equal(await main([...args(f), '--out', f.out, '--send'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() }), 2, where);
      assert.equal(t.bodies.length, 0, `${where}: no request`);
      assert.match(out.text, /is a symbolic link or not a directory: nothing was sent\./);
      assert.equal(await mode(target), 0o755, `${where}: the target untouched`);
      assert.deepEqual(await readdir(target), []);
      nothingTold(out.text);
      await rm(f.dir, { recursive: true });
    }
    const f = await fixture();
    const t = fakeTransport();
    const out = output();
    const stdin = terminal();
    const first = main([...args(f), '--out', f.out], { transport: t.send, stdout: out, isTTY: true, stdin, signals: new EventEmitter() });
    await prompted(out);
    const shown = await readFile(previewPrinted(out.text), 'utf8');
    const out2 = output();
    assert.equal(await main([...args(f), '--out', f.out, '--send'], { transport: t.send, stdout: out2, isTTY: false, signals: new EventEmitter() }), 2);
    assert.match(out2.text, /another run of a tool holds .*\.lock .*nothing was sent\./);
    assert.equal(t.bodies.length, 0);
    assert.equal(await readFile(previewPrinted(out.text), 'utf8'), shown, 'the first run\'s preview unchanged');
    stdin.write('no\r');
    assert.equal(await first, 1);
    nothingTold(out.text + out2.text);
    await rm(f.dir, { recursive: true });
  });
}

// The pack file's JSON (after the magic and its line feed).
async function packOf(out) {
  const bytes = await readFile(join(out, 'ko.u6lp'));
  readPack(bytes);
  return JSON.parse(bytes.subarray(5).toString('utf8'));
}

// A pack already in `out`, as a K1 run left it: one run of the first bundle's script.
async function k1Pack(out, { base = fingerprint([{ archive: 'a', item: ITEMS[0], hash: HASHES[0] }]), hash = hashOf('a keeper of tests') } = {}) {
  await mkdir(out, { recursive: true, mode: 0o700 });
  const runs = [{ archive: 'a', item: ITEMS[0], offset: 20, len: 17, hash, text: '시험 지기' }];
  const bytes = packBytes({ base, runs, tool: 'tools/i18n/translate.mjs/1' });
  await writeFile(join(out, 'ko.u6lp'), bytes, { mode: 0o600 });
  return { bytes, runs };
}

test('with --send, game strings alone: bounded requests; each string through the gates (a changed directive asked again once, then left English and counted); merged into the pack there, whose runs stay byte for byte', async () => {
  const many = [...STRINGS, ...Array.from({ length: STRINGS_PER_REQUEST + 3 }, (_, i) => str(`Line ${i} of the many.`, 'message'))];
  const f = await fixture({ strings: { ...EXPORT, strings: many } });
  const k1 = await k1Pack(f.out);
  const t = fakeTransport((l) => (l.en.startsWith('%d stone') ? '%s 돌/s.\n' : null));
  const out = output();
  const code = await main(['--strings', f.strings, '--out', f.out, '--send', '--jobs', '1'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() });
  assert.equal(code, 3, 'written, a string left English');
  const asks = t.bodies.map((b) => JSON.parse(JSON.parse(b).messages[0].content).strings.length);
  assert.ok(asks.every((n) => n <= STRINGS_PER_REQUEST), 'no request over the bound');
  assert.deepEqual(asks, [STRINGS_PER_REQUEST, many.length - STRINGS_PER_REQUEST, 1], 'two batches, then the failed string alone');
  const user = JSON.parse(JSON.parse(t.bodies[0]).messages[0].content);
  assert.deepEqual(Object.keys(user.strings[0]), ['n', 'en', 'kind']);
  assert.ok(JSON.parse(t.bodies[0]).system.includes('사극체'), 'the guide goes with it');
  const pack = await packOf(f.out);
  assert.deepEqual(Object.keys(pack), ['format', 'language', 'tool', 'base', 'converse', 'strings']);
  assert.deepEqual(pack.converse, k1.runs, 'the runs the pack had, as they were');
  assert.equal(pack.base.converse, fingerprint([{ archive: 'a', item: ITEMS[0], hash: HASHES[0] }]), 'its conversation part unchanged');
  assert.equal(pack.strings.length, many.length - 1);
  for (const s of pack.strings) {
    const en = many.find((m) => m.hash === s.hash);
    assert.ok(en && s.len === en.len && stringGate(en.text, s.text) === null);
  }
  const coverage = JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8'));
  assert.deepEqual(coverage.strings, { read: many.length, found: many.length, translated: many.length - 1, leftEnglish: 1, byReason: { directives: 1 }, unexplained: 0 });
  assert.match(out.text, /Game strings: \d+ found, \d+ translated, 1 left English \(directives 1\)\./);
  assert.match(out.text, /Pack: .* holds 1 lines, 0 table strings and 0 game strings; what this run translates is merged into it\./);
  nothingTold(out.text);
  // Again: what it translates replaces what the pack had; the rest stays.
  const t2 = fakeTransport((l) => (l.en === 'Seek' ? '찾기' : null));
  const one = await fixture({ strings: { ...EXPORT, strings: [STRINGS[2]] } });
  assert.equal(await main(['--strings', one.strings, '--out', f.out, '--send'], { transport: t2.send, stdout: output(), isTTY: false, signals: new EventEmitter() }), 0);
  const again = await packOf(f.out);
  assert.equal(again.strings.length, pack.strings.length);
  assert.equal(again.strings.find((s) => s.hash === STRINGS[2].hash).text, '찾기');
  assert.deepEqual(again.converse, k1.runs);
  await rm(one.dir, { recursive: true });
  await rm(f.dir, { recursive: true });
});

test('a label of 1 or 2 bytes (a wind\'s name) is planned, translated and merged, and the pack reads back; a format or a message that short is refused unasked', async () => {
  const short = [str('Zw', 'label'), str('Z', 'label'), str('Naught is there.', 'message')];
  assert.deepEqual(stringsPlanOf({ ...EXPORT, strings: short }).batches[0].units.map((u) => [u.text, u.len, u.skind]),
    [['Zw', 2, 'label'], ['Z', 1, 'label'], ['Naught is there.', 16, 'message']]);
  assert.equal(stringGate('Zw', '샛바람'), null);
  const f = await fixture({ strings: { ...EXPORT, strings: short } });
  await k1Pack(f.out);
  const t = fakeTransport((l) => (l.en === 'Zw' ? '샛바람' : null));
  const out = output();
  assert.equal(await main(['--strings', f.strings, '--out', f.out, '--send'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() }), 0);
  const pack = await packOf(f.out);
  assert.deepEqual(pack.strings.find((s) => s.hash === hashOf('Zw')), { hash: hashOf('Zw'), len: 2, text: '샛바람' });
  assert.equal(pack.strings.find((s) => s.hash === hashOf('Z')).len, 1);
  nothingTold(out.text);
  // Run again over the pack it wrote: the reader takes the short strings back.
  assert.equal(await main(['--strings', f.strings, '--out', f.out, '--send'], { transport: fakeTransport().send, stdout: output(), isTTY: false, signals: new EventEmitter() }), 0);
  assert.equal((await packOf(f.out)).strings.length, 3);
  for (const kind of ['format', 'message']) {
    const g = await fixture({ strings: { ...EXPORT, strings: [str('Zq', kind), STRINGS[1]] } });
    const t2 = fakeTransport();
    const o = output();
    assert.equal(await main(['--strings', g.strings, '--out', g.out, '--send'], { transport: t2.send, stdout: o, isTTY: false, signals: new EventEmitter() }), 2, kind);
    assert.match(o.text, /translate: 1 entries of the file are not strings of u6\.string-bundle\/1/);
    assert.equal(t2.bodies.length, 0, 'nothing asked');
    await rm(g.dir, { recursive: true });
  }
  await rm(f.dir, { recursive: true });
});

test('with --send, a script with table strings: its runs and its table strings in one request; a copied one held to its capacity and to no marker (asked again once, then left English and counted); one read from inside left English unasked; the pack checks against the bundles again on the next run', async () => {
  const f = await fixture();
  const t = fakeTransport((l, first) => {
    if (l.en === 'gold') return first ? '{이/가}금' : '황금'.repeat(2);
    return null;
  });
  const out = output();
  const code = await main([f.bundles, '--item', `a:${ITEMS[0]}`, '--out', f.out, '--send'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() });
  assert.equal(code, 3, 'written, a string left English');
  const user = JSON.parse(JSON.parse(t.bodies[0]).messages[0].content);
  assert.deepEqual(user.lines.map((l) => [l.en.startsWith('"Hail') ? 'greeting' : l.en, l.table ?? false, l.copied ?? false, l.capacity ?? null]), [
    ['a keeper of tests', false, false, null], ['greeting', false, false, null],
    ['a red stone', true, false, null], ['gold', true, true, 9],
  ], 'the runs, then the table strings; the inner one and the empty one not asked');
  const pack = await packOf(f.out);
  assert.deepEqual(Object.keys(pack), ['format', 'language', 'tool', 'base', 'converse', 'tables']);
  assert.deepEqual(pack.tables.map((x) => [x.offset, x.len, x.hash]), [[200, 11, hashOf('a red stone')]]);
  assert.equal(pack.base.converse, fingerprint([{ archive: 'a', item: ITEMS[0], hash: HASHES[0] }]));
  const coverage = JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8'));
  assert.deepEqual(coverage.tables, { found: 3, translated: 1, leftEnglish: 2, byReason: { capacity: 1, inner: 1 }, unexplained: 0 });
  assert.equal(tableGate({ text: 'gold', copied: true, capacity: 9 }, '{이/가}금'), 'raw');
  assert.equal(tableGate({ text: 'gold', copied: true, capacity: 9 }, '황금황금'), 'capacity');
  assert.equal(tableGate({ text: 'gold', copied: true, capacity: 9 }, '황금'), null);
  // The next run, on the same bundles: the pack there checks, the other script merges in.
  const t2 = fakeTransport();
  assert.equal(await main([f.bundles, '--item', `a:${ITEMS[1]}`, '--out', f.out, '--send'], { transport: t2.send, stdout: output(), isTTY: false, signals: new EventEmitter() }), 3);
  const both = await packOf(f.out);
  assert.equal(both.base.converse, fingerprint(ITEMS.map((item, i) => ({ archive: 'a', item, hash: HASHES[i] }))), 'the fingerprint of both scripts');
  assert.deepEqual([...new Set(both.converse.map((r) => r.item))], ITEMS);
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

test('a pack already there that does not read, that is English, or that other game files made is refused before any request, the pack left as it was', async () => {
  for (const what of ['broken', 'english', 'other files', 'other hash']) {
    const f = await fixture();
    await mkdir(f.out, { recursive: true, mode: 0o700 });
    let bytes;
    if (what === 'broken') bytes = Buffer.from('U6LP\n{"format":');
    else if (what === 'english') bytes = packBytes({ language: 'en', base: fingerprint([]), runs: [] });
    else if (what === 'other files') bytes = (await k1Pack(f.out, { base: fingerprint([{ archive: 'a', item: ITEMS[0], hash: 'ffffffffffffffff' }]) })).bytes;
    else bytes = (await k1Pack(f.out, { hash: hashOf('another look') })).bytes;
    await writeFile(join(f.out, 'ko.u6lp'), bytes);
    const t = fakeTransport();
    const out = output();
    assert.equal(await main([f.bundles, '--item', `a:${ITEMS[1]}`, '--strings', f.strings, '--out', f.out, '--send'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() }), 2, what);
    assert.equal(t.bodies.length, 0, `${what}: no request`);
    assert.equal(previewPrinted(out.text), null, `${what}: no preview`);
    assert.deepEqual(await readFile(join(f.out, 'ko.u6lp')), bytes, `${what}: the pack as it was`);
    assert.match(out.text, what === 'broken' ? /does not read \(its contents are not those of a Korean pack\)/
      : what === 'english' ? /declares another language/ : /was made from other game files/);
    nothingTold(out.text);
    await rm(f.dir, { recursive: true });
  }
});

test('--name picks the one bundle whose NPC is so named, case folded, never printing it; none or two of that name are refused by count', async () => {
  const f = await fixture();
  const out = output();
  assert.equal(await main([f.bundles, '--name', `${NAME}keeper`.toUpperCase(), '--out', f.out, '--dry-run'], { stdout: out, isTTY: false, signals: new EventEmitter() }), 0);
  const user = JSON.parse(JSON.parse(await readFile(previewPrinted(out.text), 'utf8')).messages[0].content);
  assert.equal(user.npc.name, `${NAME}Keeper`);
  for (const [bundles, n] of [[BUNDLES, 0], [[BUNDLES[0], { ...BUNDLES[1], name: `${NAME}Keeper` }], 2]]) {
    await writeFile(f.bundles, JSON.stringify(bundles));
    const o = output();
    assert.equal(await main([f.bundles, '--name', n === 0 ? `${NAME}Nobody` : `${NAME}Keeper`, '--out', f.out, '--send'], { stdout: o, isTTY: false, signals: new EventEmitter() }), 2);
    assert.match(o.text, new RegExp(`translate: ${n} scripts with lines have that name: name one with --item`));
    nothingTold(o.text);
  }
  for (const argv of [['--name', 'x', '--strings', f.strings], [f.bundles, '--name', 'x', '--first', '1']]) {
    const o = output();
    assert.equal(await main([...argv, '--out', f.out], { stdout: o, isTTY: false, signals: new EventEmitter() }), 2, 'a usage error');
  }
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

test('the reader is the engine\'s: a K1 pack and a K2 pack read; each broken rule refuses with the engine\'s sentence; the writer leaves out empty maps; the directives are printf\'s', () => {
  const base = fingerprint([]);
  assert.equal(base, hex64(hash64([])), 'the empty set\'s fingerprint');
  const k1 = packBytes({ base, runs: [{ archive: 'b', item: 1, offset: 4, len: 2, hash: '0000000000000001', text: '가' }] });
  assert.deepEqual(Object.keys(JSON.parse(k1.subarray(5).toString())), ['format', 'language', 'tool', 'base', 'converse']);
  assert.equal(readPack(k1).converse.length, 1);
  const k2 = packBytes({
    base, runs: [],
    tables: [{ archive: 'a', item: 2, offset: 9, len: 3, hash: '0000000000000002', capacity: 6, text: '빨강' }, { archive: 'a', item: 2, offset: 1, len: 3, hash: '0000000000000003', capacity: null, text: '파랑' }],
    strings: [{ hash: 'ffffffffffffffff', len: 4, text: '찾기' }, { hash: '0000000000000004', len: 3, text: '가' }],
  });
  const json = JSON.parse(k2.subarray(5).toString());
  assert.deepEqual(json.tables.map((t) => [t.offset, Object.keys(t)]), [[1, ['archive', 'item', 'offset', 'len', 'hash', 'text']], [9, ['archive', 'item', 'offset', 'len', 'hash', 'capacity', 'text']]]);
  assert.deepEqual(json.strings.map((t) => t.hash), ['0000000000000004', 'ffffffffffffffff']);
  const read = readPack(k2);
  assert.equal(read.tables.length + read.strings.length, 4);
  const file = (o) => Buffer.from(`U6LP\n${JSON.stringify({ format: 'u6.locale-pack/1', language: 'ko', tool: 't/1', base: { converse: base }, converse: [], ...o })}`);
  const table = { archive: 'a', item: 2, offset: 9, len: 3, hash: '0000000000000002', text: '빨강' };
  for (const [bytes, why] of [
    [Buffer.from('U6LQ\n{}'), 'the file is not a Korean pack (it does not start with U6LP)'],
    [file({ extra: 1 }), 'its contents are not those of a Korean pack'],
    [file({ tables: [{ ...table, note: 1 }] }), 'its contents are not those of a Korean pack'],
    [file({ format: 'u6.locale-pack/2' }), 'it is not of this page\'s pack format (u6.locale-pack/1)'],
    [file({ tables: [{ ...table, capacity: 5 }] }), 'a table string in it is longer than the place the game copies it to'],
    [file({ tables: [{ ...table, len: 0 }] }), 'a line in it is keyed outside the conversation scripts'],
    [file({ tables: [table, table] }), 'it holds one line twice'],
    [file({ strings: [{ hash: '0000000000000004', len: 0, text: '가' }] }), 'a game string in it is not one the game prints'],
    [file({ strings: [{ hash: '0000000000000004', len: 3, text: '가'.repeat(MAX_STRING_TEXT_BYTES / 3 + 1) }] }), 'a game string in it is not one the game prints'],
    [file({ strings: [{ hash: '000000000000000G', len: 3, text: '가' }] }), 'a hash in it is not 16 hex digits'],
    [file({ strings: [{ hash: '0000000000000004', len: 3, text: '中' }] }), 'a line in it holds a character the game cannot show'],
  ]) {
    assert.throws(() => readPack(bytes), (err) => err instanceof PackReadError && err.message === why, why);
  }
  // The engine's test vectors (crates/u6-core/src/locale/tests_strings.rs).
  assert.deepEqual(directivesOf('Hi %s, %-5d%% %03x%>'), ['%s', '%-5d', '%%', '%03x', '%>']);
  assert.deepEqual(directivesOf('%2$s then %1$s'), ['%2$s', '%1$s']);
  assert.deepEqual(directivesOf('%0$s %12d'), ['%0$', '%12d']);
  assert.deepEqual(directivesOf('end %'), ['%']);
  assert.equal(stringGate('%s is here.', '%s{이/가} 있소.'), null);
  assert.equal(stringGate('%s is here.', '%d{이/가} 있소.'), 'directives');
  assert.equal(stringGate('%s and %d', '%d 그리고 %s'), 'directives');
  assert.equal(stringGate('\nNaught.', '아무것도.'), 'line-breaks');
  // The bundles' table strings as the tool reads them: none empty, each with its hash.
  const tables = tablesOf(BUNDLES[0]);
  assert.deepEqual(tables.map((x) => [x.offset, x.copied, x.capacity, x.inner]), [[200, false, null, false], [212, true, 9, false], [217, false, null, true]]);
  assert.equal(unitsOf(BUNDLES[0]).length, 2);
});

test('the reader refuses a member given twice, as the engine\'s does (at the root, in an entry, under escapes), and a run never merges into such a pack: refused before any request, the pack left as it was', async () => {
  const base = fingerprint([]);
  const head = `"format":"u6.locale-pack/1","language":"ko","tool":"t/1","base":{"converse":"${base}"}`;
  const string = '{"hash":"0000000000000004","len":3,"text":"가"}';
  const shape = 'its contents are not those of a Korean pack';
  for (const json of [
    `{${head},"converse":[],"converse":[]}`,
    `{${head},"converse":[],"conv\\u0065rse":[]}`,
    `{${head},"converse":[],"strings":[${string}],"strings":[]}`,
    `{${head},"converse":[],"tables":[],"tables":[]}`,
    `{${head},"converse":[],"strings":[{"hash":"0000000000000004","len":3,"text":"가","text":"나"}]}`,
    `{${head.replace('"converse":"', `"converse":"${base}","converse":"`)},"converse":[]}`,
  ]) {
    assert.equal(JSON.parse(json).format, 'u6.locale-pack/1', 'JSON.parse takes it');
    assert.ok(duplicateMembers(json));
    assert.throws(() => readPack(Buffer.from(`U6LP\n${json}`)), (err) => err instanceof PackReadError && err.message === shape);
  }
  // Keys alike in different objects, and a key's text inside a value, are no duplicates.
  assert.ok(!duplicateMembers(`{${head},"converse":[],"strings":[${string},${string.replace('04', '05')}],"x":"\\"converse\\":"}`));
  const f = await fixture();
  await mkdir(f.out, { recursive: true, mode: 0o700 });
  const bytes = Buffer.from(`U6LP\n{${head},"converse":[],"strings":[${string}],"strings":[]}`);
  await writeFile(join(f.out, 'ko.u6lp'), bytes);
  const t = fakeTransport();
  const out = output();
  assert.equal(await main(['--strings', f.strings, '--out', f.out, '--send'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() }), 2);
  assert.equal(t.bodies.length, 0, 'no request');
  assert.equal(previewPrinted(out.text), null, 'no preview');
  assert.match(out.text, /does not read \(its contents are not those of a Korean pack\)/);
  assert.deepEqual(await readFile(join(f.out, 'ko.u6lp')), bytes, 'the pack as it was');
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

// `n` game strings of a pack, each `text`, under made-up hashes.
const madeStrings = (n, text) => Array.from({ length: n }, (_, i) => ({ hash: (0x7000000000000000n + BigInt(i)).toString(16), len: 3, text }));

test('a merge past the reader\'s limits (the size, the count of lines) is not written: the pack there and coverage.json are left as they were, with a line that names the rule only', async () => {
  // The count: a pack of MAX_RUNS game strings, and one more translated.
  const lines = await fixture({ strings: { ...EXPORT, strings: [STRINGS[1]] } });
  const full = packBytes({ base: fingerprint([]), runs: [], strings: madeStrings(MAX_RUNS, '가') });
  // The size: game strings of 200 syllables, padded with ASCII to 5 bytes under the limit.
  const big = await fixture({ strings: { ...EXPORT, strings: [STRINGS[1]] } });
  const text = '가'.repeat(200);
  const one = Buffer.byteLength(JSON.stringify({ hash: '7000000000000000', len: 3, text })) + 1;
  const strings = madeStrings(Math.floor((MAX_PACK_BYTES - 1000) / one), text);
  let deficit = MAX_PACK_BYTES - 5 - packBytes({ base: fingerprint([]), runs: [], strings }).length;
  for (let i = 0; deficit > 0; i += 1) {
    const add = Math.min(deficit, MAX_STRING_TEXT_BYTES - Buffer.byteLength(strings[i].text));
    strings[i] = { ...strings[i], text: strings[i].text + 'a'.repeat(add) };
    deficit -= add;
  }
  const large = packBytes({ base: fingerprint([]), runs: [], strings });
  assert.equal(large.length, MAX_PACK_BYTES - 5);
  for (const [f, bytes, why] of [
    [lines, full, 'it holds more lines than a pack can'],
    [big, large, 'the file is larger than a Korean pack can be'],
  ]) {
    readPack(bytes);
    await mkdir(f.out, { recursive: true, mode: 0o700 });
    await writeFile(join(f.out, 'ko.u6lp'), bytes, { mode: 0o600 });
    await writeFile(join(f.out, 'coverage.json'), 'the coverage of the run before', { mode: 0o600 });
    const t = fakeTransport();
    const out = output();
    assert.equal(await main(['--strings', f.strings, '--out', f.out, '--send'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() }), 2, why);
    assert.equal(t.bodies.length, 1, 'the string was asked');
    assert.ok(out.text.includes(`translate: the merged pack would not read (${why}): the pack there and coverage.json are left as they were; nothing was written.`), why);
    assert.ok((await readFile(join(f.out, 'ko.u6lp'))).equals(bytes), `${why}: the pack as it was`);
    assert.equal(await readFile(join(f.out, 'coverage.json'), 'utf8'), 'the coverage of the run before');
    assert.ok((await readdir(f.out)).every((name) => !name.includes('.part-')), 'no temporary file left');
    nothingTold(out.text);
    await rm(f.dir, { recursive: true });
  }
});

// --- the CLI in a child process --------------------------------------------------------------

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

function cli(args) {
  return new Promise((done) => {
    const child = spawn(process.execPath, [TOOL, ...args], { stdio: ['ignore', 'pipe', 'pipe'] });
    let text = '';
    child.stdout.on('data', (d) => { text += d; });
    child.stderr.on('data', (d) => { text += d; });
    child.on('close', (code) => done({ code, text }));
  });
}

test('the CLI with --strings: no terminal and no --send reaches no server; --dry-run neither; --send does and writes the pack; a 307 or 308 to another server is not followed', async () => {
  const f = await fixture();
  const s = await server((body) => {
    const user = JSON.parse(JSON.parse(body).messages[0].content);
    return answer(user.strings.map((l) => ({ n: l.n, ko: fakeKo(l.en) })));
  });
  try {
    const none = await cli(['--strings', f.strings, '--out', f.out, '--llmux', s.url]);
    assert.equal(none.code, 1);
    const dry = await cli(['--strings', f.strings, '--out', f.out, '--llmux', s.url, '--dry-run']);
    assert.equal(dry.code, 0);
    assert.equal(s.seen.length, 0, 'no request without --send');
    const sent = await cli(['--strings', f.strings, '--out', f.out, '--llmux', s.url, '--send']);
    assert.equal(sent.code, 0, sent.text);
    assert.deepEqual(s.seen.map((r) => [r.method, r.path]), [['POST', '/v1/messages']]);
    assert.equal(s.seen[0].body, await readFile(previewPrinted(sent.text), 'utf8'), 'the preview is what was sent');
    assert.equal((await packOf(f.out)).strings.length, STRINGS.length);
    nothingTold(none.text + dry.text + sent.text);
  } finally {
    await s.close();
    await rm(f.dir, { recursive: true });
  }
  for (const status of [307, 308]) {
    const g = await fixture();
    const other = await server(() => ({ status: 200, json: {} }));
    const r307 = await server(() => ({ status, headers: { location: `${other.url}/v1/messages` } }));
    try {
      const many = { ...EXPORT, strings: [...STRINGS, ...Array.from({ length: STRINGS_PER_REQUEST }, (_, i) => str(`Line ${i} more.`, 'message'))] };
      await writeFile(g.strings, JSON.stringify(many));
      const r = await cli(['--strings', g.strings, '--out', g.out, '--llmux', r307.url, '--send', '--jobs', '1']);
      assert.equal(other.seen.length, 0, `${status}: the other server got nothing`);
      assert.equal(r307.seen.length, 1, `${status}: one request, none after the refusal`);
      assert.match(r.text, new RegExp(`The route refused a request \\(http-${status}\\): no request was sent after it\\.`));
      assert.ok(!existsSync(join(g.out, 'ko.u6lp')), 'no pack');
      nothingTold(r.text);
    } finally {
      await r307.close();
      await other.close();
      await rm(g.dir, { recursive: true });
    }
  }
});

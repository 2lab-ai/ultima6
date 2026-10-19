// Tests of tools/i18n/translate.mjs for spec 08's slice K4 (the strings of the u program the
// page's Game strings download lists under `u`: the opening, the story, the gypsy, character
// creation and the transfer, translated into the pack's `u`) on synthetic inputs the test writes
// (no game data), with a counting fake transport or a local server that counts each connection
// and request as it arrives, in a directory of its own (never ~/Games/ultima6):
//
// - zero requests with the strings of u (all parts, and --only u), through the tool's own
//   transport to that server: no terminal and no --send, --dry-run (with --send too), and on a
//   terminal a refusal, an empty answer, another case of yes, the input's end, Ctrl-C and SIGINT;
//   a symbolic link (or a file) where the output or preview directory goes; another run holding
//   the lock, then one that takes it (the first run's preview left alone); the CLI against that
//   server without --send and with --dry-run, and a 307 or 308 not followed; a file whose strings
//   of u do not read (a hash, a kind, its keys) or --only with a part it does not have;
// - the owner's go on that transport: a typed yes, the preview there (0600, its path and the
//   route printed) and nothing arrived while the prompt waits, the first request the preview read
//   before the answer; with --send, the preview there with those bytes when the first request
//   arrives (checked as it arrives);
// - with --send and --only u: the strings in bounded requests, in the order u prints them, each
//   with its kind and the keys of the read after it; each checked by its gate (u's printf codes,
//   `%B %F %X %Y %P` among them, in order; the keys its English shows standing alone, in order;
//   no josa marker; the line breaks; a Hangul syllable: asked again once, then left English and
//   counted); one with no letter outside its codes left English unasked; merged into the pack
//   there, whose K3 names stay byte for byte, and merged again by a later run;
// - the reader is the engine's for the `u` map (each broken rule refuses with the engine's
//   sentence), and nothing the tool prints holds a string of u, a translation, a Korean syllable
//   but its own label, or a hash.
//
//   node --test tools/i18n/*.test.mjs
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { existsSync, readFileSync } from 'node:fs';
import { chmod, mkdir, mkdtemp, readdir, readFile, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { PassThrough } from 'node:stream';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  MAX_U_BYTES, MAX_U_TEXT_BYTES, PackReadError, U_CHARS, U_KINDS, U_PER_REQUEST, fingerprint, hash64, hex64, keysKept, keysShown,
  latin1, main, packBytes, parseArgs, readPack, uCodesOf, uGate, uHasLetters,
} from './translate.mjs';

const TOOL = join(dirname(fileURLToPath(import.meta.url)), 'translate.mjs');
const SENTINEL = 'Zq8OpeningSentinel';

const hashOf = (text) => hex64(hash64(latin1(text)));
const line = (kind, text) => ({ kind, hash: hashOf(text), len: latin1(text).length, text });

// The page's export (u6.string-bundle/1) with the test's own game string and strings of u.
const STRINGS = [{ hash: hashOf('Naught is there.'), len: 16, kind: 'message', text: 'Naught is there.' }];
const U = [
  line('opening', `%F%BThe storm rages over ${SENTINEL}.\n%P`),
  { ...line('gypsy', 'Wouldst thou A) give alms, or B) keep thy coin?'), keys: 'AB' },
  line('creation', 'By what name art thou called?'),
  line('transfer', 'Strength\nDexterity\nIntelligence\n'),
  line('story', '%s%s'),
];
const EXPORT = { format: 'u6.string-bundle/1', locale: 'en', strings: STRINGS, u: U };

// The fake translator's Korean for `en`: every printf code and every line break kept in order, a
// key letter (A or B before `)`) as it is, each other word a syllable.
function fakeKo(en) {
  let out = '';
  for (let i = 0; i < en.length; i += 1) {
    const c = en[i];
    if (c === '%') {
      out += en.slice(i, i + 2);
      i += 1;
    } else if (/[AB]/.test(c) && en[i + 1] === ')') out += c;
    else if ('\n )?,.'.includes(c)) out += c;
    else if (/[a-z]/i.test(c) && !/[a-z]/i.test(en[i - 1] ?? '')) out += '가';
    else if (!/[a-z]/i.test(c)) out += '가';
  }
  return out;
}

const answer = (lines) => ({
  status: 200,
  json: { type: 'message', content: [{ type: 'text', text: JSON.stringify({ lines }) }], usage: { input_tokens: 10, output_tokens: 5 } },
});

// A counting fake transport; `tamper(l, first)` may change a line's Korean (the first time it is
// asked, or again).
function fakeTransport(tamper = () => null) {
  const bodies = [];
  const asked = new Map();
  const send = async (body) => {
    bodies.push(body);
    const user = JSON.parse(JSON.parse(body).messages[0].content);
    const list = user.lines ?? user.strings ?? user.texts;
    return answer(list.map((l) => {
      asked.set(l.en, (asked.get(l.en) ?? 0) + 1);
      return { n: l.n, ko: tamper(l, asked.get(l.en) === 1) ?? fakeKo(l.en) };
    }));
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

async function fixture(exported = EXPORT) {
  const dir = await mkdtemp(join(tmpdir(), 'u6-i18n-program-'));
  const s = join(dir, 'ultima6-strings.json');
  await writeFile(s, JSON.stringify(exported));
  return { dir, strings: s, out: join(dir, 'ko') };
}

// The one line of the tool with Korean in it: the language's name on the page's menu.
const LABEL = 'Drop ko.u6lp on the page and choose 한글; coverage.json stays here.';

// Nothing the tool printed holds a string of u, its translation (or the fake's syllable) or a hash,
// and no Korean syllable is printed but the label's.
function nothingTold(printed, lines = U) {
  assert.ok(!printed.includes(SENTINEL), 'no string printed');
  for (const t of lines) {
    assert.ok(!printed.includes(t.hash), 'no hash printed');
    for (const part of t.text.split('\n').map((p) => p.trim()).filter((p) => p.length >= 6)) {
      assert.ok(!printed.includes(part), 'no text printed');
    }
    const ko = fakeKo(t.text).trim();
    if (ko.length >= 4) assert.ok(!printed.includes(ko), 'no translation printed');
  }
  assert.ok(!printed.includes('가'), 'not the fake\'s syllable');
  assert.ok(!/[가-힣]/.test(printed.replaceAll(LABEL, '')), 'no Korean printed but the label');
}

const mode = async (path) => (await stat(path)).mode & 0o777;
const previewPrinted = (printed) => [...printed.matchAll(/The exact first request: (.+) \((\d+) bytes\)\./g)].at(-1)?.[1] ?? null;
// The tool's prompt for the owner's go; a tool that does not ask within `ms` fails the test.
const prompted = (out, ms = 10_000) => new Promise((done, fail) => {
  const until = Date.now() + ms;
  const wait = () => {
    if (out.text.includes('Type yes')) done();
    else if (Date.now() > until) fail(new Error('the tool did not ask'));
    else setTimeout(wait, 5);
  };
  wait();
});

// A local server: `arrivals` gets each connection and each request as it arrives (before a body
// ends; `onArrival` is called then), `seen` each request whose body ended, answered by
// `answerFor`.
async function server(answerFor, onArrival = () => {}) {
  const seen = [];
  const arrivals = [];
  const s = createServer((req, res) => {
    arrivals.push('request');
    onArrival();
    let body = '';
    req.on('data', (d) => { body += d; });
    req.on('end', () => {
      seen.push({ method: req.method, path: req.url, body });
      const a = answerFor(body, seen.length);
      res.writeHead(a.status, { 'content-type': 'application/json', ...(a.headers ?? {}) });
      res.end(a.json === undefined ? '' : JSON.stringify(a.json));
    });
  });
  s.on('connection', () => arrivals.push('connection'));
  await new Promise((done) => s.listen(0, '127.0.0.1', done));
  return { seen, arrivals, url: `http://127.0.0.1:${s.address().port}`, close: () => new Promise((done) => s.close(done)) };
}

// The fake translator as a server's answer.
function translator(body) {
  const user = JSON.parse(JSON.parse(body).messages[0].content);
  const list = user.lines ?? user.strings ?? user.texts;
  return answer(list.map((l) => ({ n: l.n, ko: fakeKo(l.en) })));
}

// What reached `s`, once a request started and not awaited has had the time to arrive.
async function arrived(s) {
  await new Promise((done) => setTimeout(done, 50));
  return s.arrivals;
}

const VARIANTS = [
  ['every part', (f) => ['--strings', f.strings]],
  ['--only u', (f) => ['--strings', f.strings, '--only', 'u']],
];

for (const [name, args] of VARIANTS) {
  test(`zero requests (${name}): no terminal and no --send; --dry-run alone and with --send`, async () => {
    const f = await fixture();
    const s = await server(translator);
    try {
      const out = output();
      const argv = [...args(f), '--out', f.out, '--llmux', s.url];
      assert.equal(await main(argv, { stdout: out, isTTY: false, signals: new EventEmitter() }), 1);
      assert.match(out.text, /Nothing was sent: there is no terminal to ask on; add --send to send\./);
      assert.match(out.text, /Opening and creation: 5 strings of u read; 4 to translate, \d+ characters\./);
      for (const extra of [['--dry-run'], ['--dry-run', '--send']]) {
        assert.equal(await main([...argv, ...extra], { stdout: out, isTTY: true, stdin: terminal(), signals: new EventEmitter() }), 0);
      }
      assert.deepEqual(await arrived(s), [], 'nothing reached the server');
      const path = previewPrinted(out.text);
      assert.equal(await mode(path), 0o600);
      assert.equal(await mode(join(f.out, 'preview')), 0o700);
      assert.equal(await mode(f.out), 0o700);
      assert.ok(!existsSync(join(f.out, 'ko.u6lp')), 'no pack');
      nothingTold(out.text);
    } finally {
      await s.close();
      await rm(f.dir, { recursive: true });
    }
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
      const s = await server(translator);
      try {
        const out = output();
        const stdin = terminal();
        const signals = new EventEmitter();
        const run = main([...args(f), '--out', f.out, '--llmux', s.url], { stdout: out, isTTY: true, stdin, signals });
        await prompted(out);
        if (keys === null) signals.emit('SIGINT');
        else if (keys === '') stdin.end();
        else stdin.write(keys);
        assert.equal(await run, want);
        assert.deepEqual(await arrived(s), [], 'nothing reached the server');
        assert.equal(signals.listenerCount('SIGINT'), 0);
        nothingTold(out.text);
      } finally {
        await s.close();
        await rm(f.dir, { recursive: true });
      }
    });
  }

  test(`zero requests (${name}): a symbolic link or a file where the output or preview directory goes; another run holding the lock, then a later run, the first run's preview left alone`, async () => {
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
    const firstPath = previewPrinted(out.text);
    const shown = await readFile(firstPath);
    const out2 = output();
    assert.equal(await main([...args(f), '--out', f.out, '--send'], { transport: t.send, stdout: out2, isTTY: false, signals: new EventEmitter() }), 2);
    assert.match(out2.text, /another run of a tool holds .*\.lock .*nothing was sent\./);
    assert.equal(t.bodies.length, 0);
    assert.deepEqual(await readFile(firstPath), shown, 'the first run\'s preview unchanged');
    stdin.write('no\r');
    assert.equal(await first, 1);
    const out3 = output();
    assert.equal(await main([...args(f), '--out', f.out, '--dry-run'], { transport: t.send, stdout: out3, isTTY: false, signals: new EventEmitter() }), 0);
    const secondPath = previewPrinted(out3.text);
    assert.ok(secondPath !== null && secondPath !== firstPath, 'a preview of its own');
    assert.equal(dirname(secondPath), dirname(firstPath), 'in the same preview directory');
    assert.deepEqual(await readFile(firstPath), shown, 'the first run\'s preview kept, byte for byte');
    assert.equal(await mode(firstPath), 0o600);
    assert.equal(t.bodies.length, 0);
    nothingTold(out.text + out2.text + out3.text);
    await rm(f.dir, { recursive: true });
  });
}

// The pack file's JSON (after the magic and its line feed), read through the tool's reader first.
async function packOf(out) {
  const bytes = await readFile(join(out, 'ko.u6lp'));
  readPack(bytes);
  return JSON.parse(bytes.subarray(5).toString('utf8'));
}

// A pack already in `out`, as a K3 run left it: one object name.
async function k3Pack(out) {
  await mkdir(out, { recursive: true, mode: 0o700 });
  const look = [{ index: 1733, len: 4, hash: hashOf('lamp'), text: '등잔' }];
  const bytes = packBytes({ base: fingerprint([]), runs: [], look, tool: 'tools/i18n/translate.mjs/3' });
  await writeFile(join(out, 'ko.u6lp'), bytes, { mode: 0o600 });
  return { bytes, look };
}

test('with --send and --only u: the strings of u in bounded requests, in their order, each with its kind; each through its gate (asked again once, then left English and counted); one with no letter unasked; merged into the pack there, whose names stay byte for byte, and merged again by a later run', async () => {
  const many = Array.from({ length: U_PER_REQUEST + 3 }, (_, i) => line('story', `The tale goes on, page ${i}.\n`));
  const long = line('story', `${'A long passage of the test. '.repeat(18)}\n`);
  assert.ok(long.len < MAX_U_BYTES);
  const f = await fixture({ ...EXPORT, u: [...U, ...many, long] });
  const k3 = await k3Pack(f.out);
  const t = fakeTransport((l, first) => {
    if (l.en.startsWith('%F%B')) return first ? `%B%F${fakeKo(l.en).slice(4)}` : null;
    if (l.en.startsWith('By what')) return first ? '그대{이/가} 이름은?' : 'name?';
    if (l.en.startsWith('Strength')) return first ? '힘\n재주\n' : null;
    return null;
  });
  const out = output();
  const code = await main(['--strings', f.strings, '--only', 'u', '--out', f.out, '--send', '--jobs', '1'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() });
  assert.equal(code, 3, 'written, strings left English');
  const asks = t.bodies.map((b) => JSON.parse(JSON.parse(b).messages[0].content).lines);
  assert.ok(asks.every((a) => a.length <= U_PER_REQUEST), 'no request over its bound');
  assert.ok(asks.every((a) => a.reduce((n, l) => n + l.en.length, 0) <= Math.max(U_CHARS, long.len)), 'nor over its characters');
  assert.deepEqual(Object.keys(asks[0][0]), ['n', 'en', 'kind']);
  assert.deepEqual(asks[0][1], { n: 1, en: U[1].text, kind: 'gypsy', keys: 'AB' }, 'a prompt of a key read with its keys');
  assert.deepEqual(asks[0].slice(0, 4).map((l) => l.kind), ['opening', 'gypsy', 'creation', 'transfer'], 'in the order u prints them');
  assert.ok(asks.every((a) => a.every((l) => l.en !== '%s%s')), 'one with no letter never asked');
  assert.ok(asks.every((a) => a.every((l) => U_KINDS.includes(l.kind))), 'only strings of u asked (--only)');
  const system = JSON.parse(t.bodies[0]).system;
  assert.ok(system.includes('사극체') && system.includes('%B %F %X %Y %P') && system.includes('gypsy'), 'the instruction and the guide go with it');
  const pack = await packOf(f.out);
  assert.deepEqual(Object.keys(pack), ['format', 'language', 'tool', 'base', 'converse', 'look', 'u']);
  assert.deepEqual(pack.look, k3.look, 'the names the pack had, as they were');
  const ko = (en) => pack.u.find((x) => x.hash === hashOf(en))?.text;
  assert.equal(ko(U[0].text), fakeKo(U[0].text), 'the codes reordered, then the fake\'s');
  assert.equal(ko(U[2].text), undefined, 'a marker, then no Hangul: English');
  assert.equal(ko(U[3].text), fakeKo(U[3].text), 'a line break lost, then the fake\'s');
  assert.ok(ko(U[1].text).includes('A)') && ko(U[1].text).includes('B)'), 'the key letters kept');
  for (const x of pack.u) {
    const en = [...U, ...many, long].find((y) => y.hash === x.hash);
    assert.ok(en && x.len === en.len && uGate(en.text, x.text) === null);
  }
  const coverage = JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8'));
  const found = U.length + many.length + 1;
  assert.deepEqual(coverage.u, { read: found, found, translated: found - 2, leftEnglish: 2, byReason: { nothing: 1, 'no-hangul': 1 }, unexplained: 0 });
  assert.equal(coverage.strings.found, 0, 'no game string chosen');
  assert.match(out.text, /Opening and creation: \d+ found, \d+ translated, 2 left English \(no-hangul 1, nothing 1\)\./);
  assert.match(out.text, /Game strings: 1 read from .* none chosen\./);
  nothingTold(out.text, [...U, ...many, long]);
  // A later run: what it translates again replaces what the pack had; the rest stays.
  const t2 = fakeTransport((l) => (l.en.startsWith('By what') ? '그대 이름은 무엇이오?' : null));
  const f2 = { ...f, strings: join(f.dir, 'again.json') };
  await writeFile(f2.strings, JSON.stringify({ ...EXPORT, u: [U[2]] }));
  const out2 = output();
  assert.equal(await main(['--strings', f2.strings, '--only', 'u', '--out', f.out, '--send'], { transport: t2.send, stdout: out2, isTTY: false, signals: new EventEmitter() }), 0);
  const again = await packOf(f.out);
  assert.equal(again.u.length, pack.u.length + 1);
  assert.equal(again.u.find((x) => x.hash === U[2].hash).text, '그대 이름은 무엇이오?');
  assert.deepEqual(again.look, k3.look);
  assert.match(out2.text, /holds 0 lines, 0 table strings and 0 game strings, 1 object names and 0 books, \d+ strings of u; what this run translates is merged into it\./);
  nothingTold(out2.text);
  await rm(f.dir, { recursive: true });
});

test('the gate is u\'s printf\'s: the codes in order (%B %F %X %Y %P among them), no marker, the line breaks, a Hangul syllable, the lengths', () => {
  assert.deepEqual(uCodesOf('%F%B%X%Ya %s and %d%%%P'), ['%F', '%B', '%X', '%Y', '%s', '%d', '%%', '%P']);
  assert.deepEqual(uCodesOf('50%'), ['%']);
  assert.equal(uGate('%F%BThe storm.\n%P', '%F%B폭풍.\n%P'), null);
  for (const [en, ko, why] of [
    ['%F%BThe storm.\n%P', '%B%F폭풍.\n%P', 'directives'],
    ['%F%BThe storm.\n%P', '%F%B폭풍.\n', 'directives'],
    ['Hail, %s!', '반갑소, %s %s!', 'directives'],
    ['%X%YHere', '%X여기', 'directives'],
    ['Hail, %s!', '%s{이/가} 왔소!', 'raw'],
    ['Hail, %s!', '@{반갑소}, %s!', 'raw'],
    ['A\nB\nC\n', '가\n나\n', 'line-breaks'],
    ['A\nB\nC\n', '가\n나\n다', 'line-breaks'],
    ['\nThe end.', '끝.', 'line-breaks'],
    ['Hail!', 'Hail!', 'no-hangul'],
    ['Hail!', '', 'empty'],
    ['Hail!', '中', 'characters'],
    ['Hail!', '가'.repeat(MAX_U_TEXT_BYTES / 3 + 1), 'long'],
  ]) {
    assert.equal(uGate(en, ko), why, `${en} / ${why}`);
  }
  assert.equal(uGate('A $1 & <b>', '가 $1 & <b>'), null, 'no escape layer: $ & < > are plain characters');
  assert.ok(!uHasLetters('%s%s\n%d') && uHasLetters('%sA'));
});

test('the keys of a prompt: the letters it shows standing alone (not inside a word, not a printf code), kept standing alone in the same order; a key shown only inside a word, or a line without keys, not checked', () => {
  assert.equal(keysShown('Wouldst thou A) give, or B) keep?', 'AB'), 'AB');
  assert.equal(keysShown('B) first, then A) and A) again', 'AB'), 'BA');
  assert.equal(keysShown('(M)ale or (F)emale', 'MF'), 'MF');
  assert.equal(keysShown('N)ew', 'N'), 'N');
  assert.equal(keysShown('Male or Female', 'MF'), '', 'first letters');
  assert.equal(keysShown('%B%F the %A code', 'ABF'), '', 'codes are not keys');
  assert.equal(keysShown('a) lower', 'A'), '', 'upper case only');
  assert.equal(keysShown('%sA) after a name', 'A'), 'A');
  assert.equal(keysShown('가A)나', 'A'), 'A', 'next to Hangul');
  assert.equal(keysShown('AB) together', 'AB'), '', 'a word of two keys');
  const q = 'Wouldst thou A) give alms, or B) keep thy coin?';
  assert.ok(keysKept(q, '그대는 A) 베풀겠는가, B) 지키겠는가?', 'AB'));
  assert.ok(keysKept('Male or Female?', '남(M) 여(F)?', 'MF'), 'added keys are not counted');
  for (const [en, ko, keys, why] of [
    [q, '그대는 A) 베풀겠는가, B) 지키겠는가?', 'AB', null],
    [q, '그대는 A) 베풀겠는가, 지키겠는가?', 'AB', 'keys'],
    [q, '그대는 B) 지키겠는가, A) 베풀겠는가?', 'AB', 'keys'],
    [q, '그대는 SA) 베풀겠는가, B) 지키겠는가?', 'AB', 'keys'],
    [q, '그대는 베풀겠는가, 지키겠는가?', '', null],
    [q, '그대는 베풀겠는가, 지키겠는가?', undefined, null],
    ['Art thou Male or Female?\n%P', '그대는 남자인가 여자인가?\n%P', 'MF', null],
    ['%F%BChoose A) or B).%P', '%F%B가) 나).%P', 'AB', 'keys'],
  ]) {
    assert.equal(uGate(en, ko, keys), why, `${ko} / ${keys}`);
  }
  // The identity (each line as its English) always keeps them: its other gates aside.
  for (const en of [q, '(M)ale or (F)emale', 'N)ew S)ex']) assert.ok(keysKept(en, en, 'ABMFNS'));
});

test('with --send on the tool\'s own transport: a line of a key read that drops or reorders the keys its English shows standing alone is asked again, then left English (keys); the request names each line\'s keys; keys added as (M) and a line without keys pass', async () => {
  const sex = { ...line('creation', 'Art thou Male or Female?\n%P'), keys: 'MF' };
  const then = { ...line('gypsy', 'Then A) or B), seeker?'), keys: 'AB' };
  const story = line('story', 'A) is no key in a story.');
  const f = await fixture({ ...EXPORT, u: [U[1], then, sex, story] });
  const asked = new Map();
  const tamper = (en) => {
    const n = (asked.get(en) ?? 0) + 1;
    asked.set(en, n);
    if (en === U[1].text) return n === 1 ? '그대는 가) 베풀겠는가, B) 지키겠는가?' : '그대는 B) 지키겠는가, A) 베풀겠는가?';
    if (en === then.text) return n === 1 ? '그러면 B) 아니면 A), 구도자여?' : '그러면 A) 아니면 B), 구도자여?';
    if (en === sex.text) return '그대는 남(M)인가 여(F)인가?\n%P';
    if (en === story.text) return '이야기에는 열쇠가 없도다.';
    return fakeKo(en);
  };
  const s = await server((body) => {
    const user = JSON.parse(JSON.parse(body).messages[0].content);
    return answer(user.lines.map((l) => ({ n: l.n, ko: tamper(l.en) })));
  });
  try {
    const out = output();
    const code = await main(['--strings', f.strings, '--only', 'u', '--out', f.out, '--llmux', s.url, '--send'], { stdout: out, isTTY: false, signals: new EventEmitter() });
    assert.equal(code, 3, 'written, one left English');
    const asks = s.seen.map((r) => JSON.parse(JSON.parse(r.body).messages[0].content).lines);
    assert.equal(asks.length, 2, 'the request, then the lines that failed');
    assert.deepEqual(asks[0].map((l) => l.keys), ['AB', 'AB', 'MF', undefined], 'each line\'s keys named, none for a line without');
    assert.deepEqual(asks[1].map((l) => [l.en, l.keys]), [[U[1].text, 'AB'], [then.text, 'AB']], 'asked again with its keys');
    const system = JSON.parse(s.seen[0].body).system;
    assert.ok(system.includes('"keys"') && system.includes('standing alone') && system.includes('(M)'), 'the instruction says what keys are');
    const pack = await packOf(f.out);
    const ko = (en) => pack.u.find((x) => x.hash === hashOf(en))?.text;
    assert.equal(ko(U[1].text), undefined, 'dropped, then reordered: English');
    assert.equal(ko(then.text), '그러면 A) 아니면 B), 구도자여?', 'reordered, then kept');
    assert.equal(ko(sex.text), '그대는 남(M)인가 여(F)인가?\n%P', 'keys added as (M)');
    assert.equal(ko(story.text), '이야기에는 열쇠가 없도다.', 'no keys, nothing checked for them');
    const coverage = JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8'));
    assert.deepEqual(coverage.u.byReason, { keys: 1 });
    assert.match(out.text, /Opening and creation: 4 found, 3 translated, 1 left English \(keys 1\)\./);
    nothingTold(out.text, [U[1], then, sex, story]);
  } finally {
    await s.close();
    await rm(f.dir, { recursive: true });
  }
});

test('on a terminal, a typed yes on the tool\'s own transport: while the prompt waits, the preview is there (0600, its path and the route printed) and nothing reached the server; then the first request is the preview read before the answer, byte for byte', async () => {
  const f = await fixture();
  const s = await server(translator);
  try {
    const out = output();
    const stdin = terminal();
    const run = main(['--strings', f.strings, '--only', 'u', '--out', f.out, '--llmux', s.url], { stdout: out, isTTY: true, stdin, signals: new EventEmitter() });
    await prompted(out);
    const path = previewPrinted(out.text);
    assert.ok(path !== null && existsSync(path), 'the preview, its path printed');
    assert.equal(await mode(path), 0o600);
    assert.ok(out.text.includes(`Route: llmux on this machine (${s.url})`), 'the route printed');
    const shown = await readFile(path, 'utf8');
    assert.deepEqual(await arrived(s), [], 'nothing reached the server while it asks');
    stdin.write('yes\r');
    assert.equal(await run, 3, 'sent and written, the one with no letter left English');
    assert.ok(s.seen.length >= 1);
    assert.equal(s.seen[0].body, shown, 'the first request is the preview');
    assert.ok(existsSync(join(f.out, 'ko.u6lp')));
    nothingTold(out.text);
  } finally {
    await s.close();
    await rm(f.dir, { recursive: true });
  }
});

test('with --send on the tool\'s own transport: when the first request arrives, its preview is already there, holding its bytes', async () => {
  const f = await fixture();
  const out = output();
  const atArrival = [];
  const s = await server(translator, () => {
    const path = previewPrinted(out.text);
    atArrival.push(path !== null && existsSync(path) ? readFileSync(path, 'utf8') : null);
  });
  try {
    const code = await main(['--strings', f.strings, '--only', 'u', '--out', f.out, '--llmux', s.url, '--send'], { stdout: out, isTTY: false, signals: new EventEmitter() });
    assert.equal(code, 3);
    assert.ok(atArrival.length >= 1 && s.seen.length >= 1);
    assert.ok(atArrival[0] !== null, 'the preview there, its path printed, as the first request arrived');
    assert.equal(atArrival[0], s.seen[0].body, 'holding the first request\'s bytes');
    nothingTold(out.text);
  } finally {
    await s.close();
    await rm(f.dir, { recursive: true });
  }
});

test('a file whose strings of u do not read, and --only with a part the file does not have, are refused before any request', async () => {
  for (const [what, bad] of [
    ['a hash', { ...U[0], hash: '0000000000000000' }],
    ['a kind', { ...U[0], kind: 'look' }],
    ['a length', { ...U[0], len: U[0].len + 1 }],
    ['too long', line('story', 'a'.repeat(MAX_U_BYTES + 1))],
    ['keys in lower case', { ...U[1], keys: 'ab' }],
    ['a key twice', { ...U[1], keys: 'AA' }],
    ['keys not a string', { ...U[1], keys: ['A'] }],
  ]) {
    const f = await fixture({ ...EXPORT, u: [bad] });
    const t = fakeTransport();
    const out = output();
    assert.equal(await main(['--strings', f.strings, '--out', f.out, '--send'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() }), 2, what);
    assert.equal(t.bodies.length, 0, `${what}: no request`);
    assert.equal(previewPrinted(out.text), null, `${what}: no preview`);
    assert.match(out.text, /translate: 1 strings of the file are not strings of the u program of u6\.string-bundle\/1/);
    nothingTold(out.text, [U[0]]);
    await rm(f.dir, { recursive: true });
  }
  for (const argv of [['bundles.json', '--first', '1', '--only', 'u'], ['--strings', 'x.json', '--only', 'opening']]) {
    assert.match(parseArgs(argv).error, /--only names parts of the --strings file: strings, look, books, u/);
  }
});

test('the reader is the engine\'s for the strings of u: a K4 pack reads; each broken rule refuses with the engine\'s sentence; the writer leaves out an empty map', () => {
  const base = fingerprint([]);
  const k3 = packBytes({ base, runs: [], look: [{ index: 1, len: 3, hash: '0000000000000002', text: '칼' }] });
  assert.deepEqual(Object.keys(JSON.parse(k3.subarray(5).toString())), ['format', 'language', 'tool', 'base', 'converse', 'look']);
  const k4 = packBytes({ base, runs: [], u: [{ hash: '0000000000000009', len: 5, text: '폭풍' }, { hash: '0000000000000003', len: 3, text: '가' }] });
  const json = JSON.parse(k4.subarray(5).toString());
  assert.deepEqual(Object.keys(json), ['format', 'language', 'tool', 'base', 'converse', 'u']);
  assert.deepEqual(json.u.map((t) => t.hash), ['0000000000000003', '0000000000000009'], 'in hash order');
  assert.equal(readPack(k4).u.length, 2);
  const file = (o) => Buffer.from(`U6LP\n${JSON.stringify({ format: 'u6.locale-pack/1', language: 'ko', tool: 't/1', base: { converse: base }, converse: [], ...o })}`);
  const one = { hash: '0000000000000002', len: 3, text: '가' };
  for (const [bytes, why] of [
    [file({ u: [{ ...one, note: 1 }] }), 'its contents are not those of a Korean pack'],
    [file({ u: [{ ...one, len: 2 }] }), 'an opening or creation string in it is not one the u program prints'],
    [file({ u: [{ ...one, len: 512 }] }), 'an opening or creation string in it is not one the u program prints'],
    [file({ u: [{ ...one, text: '가'.repeat(MAX_U_TEXT_BYTES / 3 + 1) }] }), 'an opening or creation string in it is not one the u program prints'],
    [file({ u: [one, one] }), 'it holds one line twice'],
    [file({ u: [{ ...one, text: '中' }] }), 'a line in it holds a character the game cannot show'],
    [file({ u: [{ ...one, text: `${SENTINEL}\r` }] }), 'a line in it holds a character the game cannot show'],
    [file({ u: [{ ...one, hash: '0123' }] }), 'a hash in it is not 16 hex digits'],
    [file({ u: {} }), 'its contents are not those of a Korean pack'],
    // The engine reads every hash before it checks a name: the hash of a string of u first.
    [file({ look: [{ index: 1, len: 0, hash: '0000000000000002', text: '칼' }], u: [{ ...one, hash: '0123' }] }), 'a hash in it is not 16 hex digits'],
  ]) {
    assert.throws(() => readPack(bytes), (err) => err instanceof PackReadError && err.message === why, why);
  }
  assert.equal(readPack(file({ u: [{ ...one, len: 511, text: '가'.repeat(MAX_U_TEXT_BYTES / 3) }] })).u.length, 1, 'the longest');
});

// --- the CLI in a child process --------------------------------------------------------------

function cli(args) {
  return new Promise((done) => {
    const child = spawn(process.execPath, [TOOL, ...args], { stdio: ['ignore', 'pipe', 'pipe'] });
    let printed = '';
    child.stdout.on('data', (d) => { printed += d; });
    child.stderr.on('data', (d) => { printed += d; });
    child.on('close', (code) => done({ code, text: printed }));
  });
}

test('the CLI with the strings of u: no terminal and no --send reaches no server; --dry-run neither; --send does and writes them; a 307 or 308 to another server is not followed', async () => {
  const f = await fixture();
  const s = await server(translator);
  try {
    const args = ['--strings', f.strings, '--only', 'u', '--out', f.out, '--llmux', s.url];
    const none = await cli(args);
    assert.equal(none.code, 1);
    const dry = await cli([...args, '--dry-run']);
    assert.equal(dry.code, 0);
    assert.deepEqual(await arrived(s), [], 'nothing reached the server without --send');
    const sent = await cli([...args, '--send']);
    assert.equal(sent.code, 3, 'written, the one with no letter left English');
    assert.deepEqual(s.seen.map((r) => [r.method, r.path]), [['POST', '/v1/messages']], 'one request');
    assert.equal(s.seen[0].body, await readFile(previewPrinted(sent.text), 'utf8'), 'the preview is what was sent first');
    const pack = await packOf(f.out);
    assert.equal(pack.u.length, U.length - 1);
    nothingTold(none.text + dry.text + sent.text);
  } finally {
    await s.close();
    await rm(f.dir, { recursive: true });
  }
  for (const status of [307, 308]) {
    const many = Array.from({ length: U_PER_REQUEST + 1 }, (_, i) => line('story', `Page ${i} of the tale.`));
    const g = await fixture({ ...EXPORT, u: [...U, ...many] });
    const other = await server(() => ({ status: 200, json: {} }));
    const redirect = await server(() => ({ status, headers: { location: `${other.url}/v1/messages` } }));
    try {
      const r = await cli(['--strings', g.strings, '--only', 'u', '--out', g.out, '--llmux', redirect.url, '--send', '--jobs', '1']);
      assert.deepEqual(await arrived(other), [], `${status}: nothing reached the other server`);
      assert.equal(redirect.seen.length, 1, `${status}: one request, none after the refusal`);
      assert.match(r.text, new RegExp(`The route refused a request \\(http-${status}\\): no request was sent after it\\.`));
      assert.ok(!existsSync(join(g.out, 'ko.u6lp')), 'no pack');
      nothingTold(r.text);
    } finally {
      await redirect.close();
      await other.close();
      await rm(g.dir, { recursive: true });
    }
  }
});

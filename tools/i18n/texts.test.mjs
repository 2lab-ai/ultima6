// Tests of tools/i18n/translate.mjs for spec 08's slice K3 (the object names and the books and
// signs the page's Game strings download lists, translated into the pack's `look` and `books`) on
// synthetic inputs the test writes (no game data), with a counting fake transport or a local
// server that counts each connection and request as it arrives, in a directory of its own (never
// ~/Games/ultima6):
//
// - zero requests with the names and books (all parts, and --only look,books), through the tool's
//   own transport to that server: no terminal and no --send, --dry-run (with --send too), and on a
//   terminal a refusal, an empty answer, another case of yes, the input's end, Ctrl-C and SIGINT;
//   a symbolic link (or a file) where the output or preview directory goes; another run holding
//   the lock, then one that takes it (the first run's preview left alone); the CLI against that
//   server without --send and with --dry-run, and a 307 or 308 not followed;
//   a pack already there whose names or books other game files made; a file whose texts do not
//   read; --only without --strings or with a part it does not have;
// - with --send: the names and the books in bounded requests (a long book alone), each checked by
//   its gates (a name's plural marker, its josa marker, a book over the game's buffer, an answer
//   with no Hangul: asked again once, then left English and counted), a book of runes only left
//   English unasked; merged into the pack there, whose K2 strings stay byte for byte;
// - the reader is the engine's (each broken rule of the new maps refuses with the engine's
//   sentence), and nothing the tool prints holds a name, a book, a translation, a Korean
//   syllable but its own label, a hash or an index.
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
  BOOK_BYTES, BOOK_CHARS, BOOKS_PER_REQUEST, LOOK_PER_REQUEST, PackReadError, bookGate, fingerprint, hasLetters, hash64,
  hex64, latin1, lookGate, main, packBytes, parseArgs, readPack, withoutPlural,
} from './translate.mjs';

const TOOL = join(dirname(fileURLToPath(import.meta.url)), 'translate.mjs');
const SENTINEL = 'Zq8TextSentinel';

const hashOf = (text) => hex64(hash64(latin1(text)));
const text = (kind, index, t) => ({ kind, index, hash: hashOf(t), len: latin1(t).length, text: t });

// The page's export (u6.string-bundle/1) with the test's own game strings, names and books; the
// names' and books' indices made distinctive (inside the reader's ranges) for nothingTold.
const STRINGS = [{ hash: hashOf('Naught is there.'), len: 16, kind: 'message', text: 'Naught is there.' }];
const LOOK = [
  text('look', 1733, `${SENTINEL} lamp`),
  text('look', 1871, 'kni/fe\\ves'),
  text('look', 1946, 'copper teapot'),
];
const BOOKS = [
  text('book', 197, `Beware the woods, ${SENTINEL}.\n`),
  text('book', 213, 'Sign: 5%% off for <runes>'),
  text('book', 238, '<only runes here>'),
];
const EXPORT = { format: 'u6.string-bundle/1', locale: 'en', strings: STRINGS, texts: [...LOOK, ...BOOKS] };

// The fake translator's Korean for `en` of `kind`: the line breaks at its ends, every `%` directive
// and every other escape kept in order (a name's plural markers and their letters dropped, the
// runes as they are), each other letter or word a syllable.
function fakeKo(en, kind) {
  let out = '';
  const src = kind === 'look' ? withoutPlural(en) : en;
  let runes = false;
  for (let i = 0; i < src.length; i += 1) {
    const c = src[i];
    if (c === '<') runes = true;
    if (runes) {
      out += c;
      if (c === '>') runes = false;
    } else if (c === '%') {
      const d = /^%(?:[1-9][0-9]*\$)?[-0]*[0-9]*[\s\S]?/.exec(src.slice(i))[0];
      out += d;
      i += d.length - 1;
    } else if ('$#^'.includes(c)) {
      out += c + (src[i + 1] ?? '');
      i += 1;
    } else if ('/\\'.includes(c)) {
      const letters = /^[a-z]*/i.exec(src.slice(i + 1))[0];
      out += c + letters;
      i += letters.length;
    } else if ('\n *&`.,:!"'.includes(c)) out += c;
    else if (/[a-z]/i.test(c) && !/[a-z]/i.test(src[i - 1] ?? '')) out += '가';
    else if (!/[a-z]/i.test(c)) out += '가';
  }
  return out;
}

const answer = (lines) => ({
  status: 200,
  json: { type: 'message', content: [{ type: 'text', text: JSON.stringify({ lines }) }], usage: { input_tokens: 10, output_tokens: 5 } },
});

// A counting fake transport; `tamper(l, first)` may change a text's Korean (the first time it is
// asked, or again).
function fakeTransport(tamper = () => null) {
  const bodies = [];
  const asked = new Map();
  const send = async (body) => {
    bodies.push(body);
    const user = JSON.parse(JSON.parse(body).messages[0].content);
    const list = user.texts ?? user.strings ?? user.lines;
    return answer(list.map((l) => {
      asked.set(l.en, (asked.get(l.en) ?? 0) + 1);
      return { n: l.n, ko: tamper(l, asked.get(l.en) === 1) ?? fakeKo(l.en, l.kind) };
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
  const dir = await mkdtemp(join(tmpdir(), 'u6-i18n-texts-'));
  const s = join(dir, 'ultima6-strings.json');
  await writeFile(s, JSON.stringify(exported));
  return { dir, strings: s, out: join(dir, 'ko') };
}

// The one line of the tool with Korean in it: the language's name on the page's menu.
const LABEL = 'Drop ko.u6lp on the page and choose 한글; coverage.json stays here.';

// Nothing the tool printed names a thing, a book, its translation (or the fake's syllable), a
// hash or an index, and no Korean syllable is printed but the label's. An index is looked for as
// a number of its own, with the tool's preview names (its pid, random hex) left out.
function nothingTold(printed) {
  assert.ok(!printed.includes(SENTINEL), 'no name or book printed');
  const numbers = printed.replace(/first-request-\d+-[0-9a-f]+\.json/g, 'first-request.json');
  for (const t of [...LOOK, ...BOOKS]) {
    assert.ok(!printed.includes(t.hash), 'no hash printed');
    assert.ok(!printed.includes(t.text.trim()), 'no text printed');
    assert.ok(!printed.includes(fakeKo(t.text, t.kind).trim()), 'no translation printed');
    assert.ok(!new RegExp(`\\b${t.index}\\b`).test(numbers), 'no index printed');
  }
  assert.ok(!printed.includes('가'), 'not the fake\'s syllable');
  assert.ok(!/[가-힣]/.test(printed.replaceAll(LABEL, '')), 'no Korean printed but the label');
}

const mode = async (path) => (await stat(path)).mode & 0o777;
const previewPrinted = (printed) => [...printed.matchAll(/The exact first request: (.+) \((\d+) bytes\)\./g)].at(-1)?.[1] ?? null;
const prompted = (out) => new Promise((done) => { const wait = () => (out.text.includes('Type yes') ? done() : setTimeout(wait, 5)); wait(); });

const VARIANTS = [
  ['every part', (f) => ['--strings', f.strings]],
  ['--only look,books', (f) => ['--strings', f.strings, '--only', 'look,books']],
];

for (const [name, args] of VARIANTS) {
  // The tool's own transport (no `transport` given) to a local server that counts arrivals.
  test(`zero requests (${name}): no terminal and no --send; --dry-run alone and with --send`, async () => {
    const f = await fixture();
    const s = await server(translator);
    try {
      const out = output();
      const argv = [...args(f), '--out', f.out, '--llmux', s.url];
      assert.equal(await main(argv, { stdout: out, isTTY: false, signals: new EventEmitter() }), 1);
      assert.match(out.text, /Nothing was sent: there is no terminal to ask on; add --send to send\./);
      assert.match(out.text, /Object names and books: 3 names and 3 books read; 3 names and 2 books to translate, \d+ characters\./);
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
    const firstPath = previewPrinted(out.text);
    const shown = await readFile(firstPath);
    const out2 = output();
    assert.equal(await main([...args(f), '--out', f.out, '--send'], { transport: t.send, stdout: out2, isTTY: false, signals: new EventEmitter() }), 2);
    assert.match(out2.text, /another run of a tool holds .*\.lock .*nothing was sent\./);
    assert.equal(t.bodies.length, 0);
    assert.deepEqual(await readFile(firstPath), shown, 'the first run\'s preview unchanged');
    stdin.write('no\r');
    assert.equal(await first, 1);
    // Then a run that takes the lock and writes its own preview: the first one's stays as it was.
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

// The pack file's JSON (after the magic and its line feed).
async function packOf(out) {
  const bytes = await readFile(join(out, 'ko.u6lp'));
  readPack(bytes);
  return JSON.parse(bytes.subarray(5).toString('utf8'));
}

// A pack already in `out`, as a K2 run left it: one game string, and `extra` maps.
async function k2Pack(out, extra = {}) {
  await mkdir(out, { recursive: true, mode: 0o700 });
  const strings = [{ hash: STRINGS[0].hash, len: STRINGS[0].len, text: '아무것도 없소.' }];
  const bytes = packBytes({ base: fingerprint([]), runs: [], strings, tool: 'tools/i18n/translate.mjs/2', ...extra });
  await writeFile(join(out, 'ko.u6lp'), bytes, { mode: 0o600 });
  return { bytes, strings };
}

test('with --send and --only look,books: the names and the books in bounded requests (a long book alone); each through its gates (asked again once, then left English and counted); a book of runes only unasked; merged into the pack there, whose strings stay byte for byte', async () => {
  const names = Array.from({ length: LOOK_PER_REQUEST + 5 }, (_, i) => text('look', 10 + i, `thing number ${i}`));
  const long = text('book', 9, `${'A long page of the test. '.repeat(300)}\n`);
  assert.ok(long.len > BOOK_CHARS && long.len < BOOK_BYTES);
  const many = Array.from({ length: BOOKS_PER_REQUEST + 1 }, (_, i) => text('book', 20 + i, `Page ${i} of the test.`));
  const f = await fixture({ ...EXPORT, texts: [...LOOK, ...names, ...BOOKS, long, ...many] });
  const k2 = await k2Pack(f.out);
  const t = fakeTransport((l, first) => {
    if (l.en === 'kni/fe\\ves') return first ? '칼/s' : '칼';
    if (l.en === 'copper teapot') return first ? '{이/가}주전자' : 'teapot';
    if (l.en.startsWith('Beware')) return first ? `${'가'.repeat(BOOK_BYTES / 3)}\n` : null;
    if (l.en === 'Page 0 of the test.') return 'Page 0.';
    return null;
  });
  const out = output();
  const code = await main(['--strings', f.strings, '--only', 'look,books', '--out', f.out, '--send', '--jobs', '1'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() });
  assert.equal(code, 3, 'written, texts left English');
  const asks = t.bodies.map((b) => JSON.parse(JSON.parse(b).messages[0].content).texts);
  assert.ok(asks.every((a) => a.length <= Math.max(LOOK_PER_REQUEST, BOOKS_PER_REQUEST)), 'no request over its bound');
  assert.ok(asks.some((a) => a.length === 1 && a[0].en === long.text), 'the long book alone');
  assert.ok(asks.every((a) => a.every((l) => l.en !== '<only runes here>')), 'the runes-only book never asked');
  assert.ok(asks.every((a) => a.every((l) => l.kind === 'look' || l.kind === 'book')), 'no game string asked (--only)');
  assert.deepEqual(Object.keys(asks[0][0]), ['n', 'en', 'kind']);
  const system = JSON.parse(t.bodies[0]).system;
  assert.ok(system.includes('사극체') && system.includes('no plural marker'), 'the instruction and the guide go with it');
  const pack = await packOf(f.out);
  assert.deepEqual(Object.keys(pack), ['format', 'language', 'tool', 'base', 'converse', 'strings', 'look', 'books']);
  assert.deepEqual(pack.strings, k2.strings, 'the strings the pack had, as they were');
  assert.equal(pack.look.find((e) => e.index === LOOK[1].index).text, '칼', 'the plural marker refused, then the name');
  assert.ok(!pack.look.some((e) => e.index === LOOK[2].index), 'no Hangul the second time: English');
  assert.equal(pack.look.length, LOOK.length + names.length - 1);
  assert.ok(pack.books.every((b) => Buffer.byteLength(b.text) < BOOK_BYTES), 'every book under the buffer');
  assert.ok(!pack.books.some((b) => b.index === 20), 'an answer with no Hangul: English');
  for (const b of pack.books) {
    const en = [...BOOKS, long, ...many].find((x) => x.index === b.index);
    assert.ok(en && b.len === en.len && b.hash === en.hash && bookGate(en.text, b.text) === null);
  }
  const coverage = JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8'));
  assert.deepEqual(coverage.look, { found: LOOK.length + names.length, translated: LOOK.length + names.length - 1, leftEnglish: 1, byReason: { 'no-hangul': 1 }, unexplained: 0 });
  const books = BOOKS.length + 1 + many.length;
  assert.deepEqual(coverage.books, { found: books, translated: books - 2, leftEnglish: 2, byReason: { nothing: 1, 'no-hangul': 1 }, unexplained: 0 });
  assert.equal(coverage.strings.found, 0, 'no game string chosen');
  assert.match(out.text, /Object names: \d+ found, \d+ translated, 1 left English \(no-hangul 1\)\./);
  assert.match(out.text, /Books: \d+ found, \d+ translated, 2 left English \(no-hangul 1, nothing 1\)\./);
  assert.match(out.text, /Game strings: 1 read from .* none chosen\./);
  nothingTold(out.text);
  // The gates as the coverage named them.
  assert.equal(lookGate('kni/fe\\ves', '칼/s'), 'plural');
  assert.equal(lookGate('copper teapot', '{이/가}주전자'), 'raw');
  assert.equal(lookGate('copper teapot', 'teapot'), 'no-hangul');
  assert.equal(lookGate('50% wet', '50 젖은'), 'escapes');
  assert.equal(lookGate('%s wet', '%d 젖은'), 'directives');
  assert.equal(bookGate('Hi.\n', `${'가'.repeat(BOOK_BYTES / 3)}\n`), 'capacity');
  assert.equal(bookGate('Hi.\n', `${'가'.repeat(BOOK_BYTES / 3 - 1)}\n`), null);
  assert.equal(bookGate('<abc> Hi.', '<abd> 안녕.'), 'runic');
  assert.ok(!hasLetters('<only runes> 12 $1') && hasLetters('a/s') === true && !hasLetters('/s\\es'));
  await rm(f.dir, { recursive: true });
});

test('a pack already there whose names or books other game files made, and a file whose texts do not read, are refused before any request, the pack left as it was', async () => {
  for (const what of ['other look', 'other book', 'bad texts']) {
    const f = await fixture(what === 'bad texts' ? { ...EXPORT, texts: [{ ...LOOK[0], hash: '0000000000000000' }] } : EXPORT);
    const extra = what === 'other look' ? { look: [{ index: 1, len: 3, hash: hashOf('hat'), text: '모자' }] }
      : { books: [{ index: 0, len: 3, hash: hashOf('Hi.'), text: '안녕.' }] };
    const { bytes } = await k2Pack(f.out, extra);
    const t = fakeTransport();
    const out = output();
    assert.equal(await main(['--strings', f.strings, '--out', f.out, '--send'], { transport: t.send, stdout: out, isTTY: false, signals: new EventEmitter() }), 2, what);
    assert.equal(t.bodies.length, 0, `${what}: no request`);
    assert.equal(previewPrinted(out.text), null, `${what}: no preview`);
    assert.deepEqual(await readFile(join(f.out, 'ko.u6lp')), bytes, `${what}: the pack as it was`);
    assert.match(out.text, what === 'bad texts' ? /translate: 1 texts of the file are not object names or books of u6\.string-bundle\/1/
      : /was made from other game files \(its object names or books\)/);
    nothingTold(out.text);
    await rm(f.dir, { recursive: true });
  }
  for (const argv of [['bundles.json', '--first', '1', '--only', 'look'], ['--strings', 'x.json', '--only', 'names'], ['--strings', 'x.json', '--only', '']]) {
    assert.match(parseArgs(argv).error, /--only names parts of the --strings file: strings, look, books/);
  }
});

test('the reader is the engine\'s for the names and the books: a K3 pack reads; each broken rule refuses with the engine\'s sentence; the writer leaves out empty maps', () => {
  const base = fingerprint([]);
  const k2 = packBytes({ base, runs: [], strings: [{ hash: '0000000000000004', len: 3, text: '가' }] });
  assert.deepEqual(Object.keys(JSON.parse(k2.subarray(5).toString())), ['format', 'language', 'tool', 'base', 'converse', 'strings']);
  const k3 = packBytes({
    base, runs: [],
    look: [{ index: 9, len: 3, hash: '0000000000000002', text: '칼' }, { index: 1, len: 3, hash: '0000000000000003', text: '빵' }],
    books: [{ index: 0, len: 4, hash: '0000000000000005', text: '읽노라.' }],
  });
  const json = JSON.parse(k3.subarray(5).toString());
  assert.deepEqual(Object.keys(json), ['format', 'language', 'tool', 'base', 'converse', 'look', 'books']);
  assert.deepEqual(json.look.map((t) => [t.index, Object.keys(t)]), [[1, ['index', 'len', 'hash', 'text']], [9, ['index', 'len', 'hash', 'text']]]);
  const read = readPack(k3);
  assert.equal(read.look.length + read.books.length, 3);
  const file = (o) => Buffer.from(`U6LP\n${JSON.stringify({ format: 'u6.locale-pack/1', language: 'ko', tool: 't/1', base: { converse: base }, converse: [], ...o })}`);
  const name = { index: 1, len: 3, hash: '0000000000000002', text: '칼' };
  const book = { index: 0, len: 3, hash: '0000000000000005', text: '가' };
  for (const [bytes, why] of [
    [file({ look: [{ ...name, note: 1 }] }), 'its contents are not those of a Korean pack'],
    [file({ look: [{ ...name, index: 2048 }] }), 'an object name in it is not one the game\'s look table holds'],
    [file({ look: [{ ...name, len: 0 }] }), 'an object name in it is not one the game\'s look table holds'],
    [file({ look: [{ ...name, len: 257 }] }), 'an object name in it is not one the game\'s look table holds'],
    [file({ look: [{ ...name, text: '가'.repeat(257) }] }), 'an object name in it is not one the game\'s look table holds'],
    [file({ look: [name, name] }), 'it holds one line twice'],
    [file({ look: [{ ...name, text: '中' }] }), 'a line in it holds a character the game cannot show'],
    [file({ look: [{ ...name, hash: '0123' }] }), 'a hash in it is not 16 hex digits'],
    [file({ look: Array.from({ length: 2049 }, (_, i) => ({ ...name, index: i })) }), 'it holds more lines than a pack can'],
    [file({ books: [{ ...book, index: 255 }] }), 'a book in it is not one the game\'s book file holds'],
    [file({ books: [{ ...book, len: BOOK_BYTES + 1 }] }), 'a book in it is not one the game\'s book file holds'],
    [file({ books: [{ ...book, text: 'a'.repeat(BOOK_BYTES) }] }), 'a book in it is longer than the game reads a book'],
    [file({ books: [{ ...book, text: `${SENTINEL}\u0001` }] }), 'a line in it holds a character the game cannot show'],
  ]) {
    assert.throws(() => readPack(bytes), (err) => err instanceof PackReadError && err.message === why, why);
  }
  assert.equal(readPack(file({ books: [{ ...book, text: 'a'.repeat(BOOK_BYTES - 1) }] })).books.length, 1, 'under the buffer');
});

// --- the CLI in a child process --------------------------------------------------------------

// A local server: `arrivals` gets each connection and each request as it arrives (before a
// body ends), `seen` each request whose body ended, answered by `answerFor`.
async function server(answerFor) {
  const seen = [];
  const arrivals = [];
  const s = createServer((req, res) => {
    arrivals.push('request');
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
  const list = user.texts ?? user.strings;
  return answer(list.map((l) => ({ n: l.n, ko: fakeKo(l.en, l.kind) })));
}

// What reached `s`, once a request started and not awaited has had the time to arrive.
async function arrived(s) {
  await new Promise((done) => setTimeout(done, 50));
  return s.arrivals;
}

function cli(args) {
  return new Promise((done) => {
    const child = spawn(process.execPath, [TOOL, ...args], { stdio: ['ignore', 'pipe', 'pipe'] });
    let printed = '';
    child.stdout.on('data', (d) => { printed += d; });
    child.stderr.on('data', (d) => { printed += d; });
    child.on('close', (code) => done({ code, text: printed }));
  });
}

test('the CLI with the names and books: no terminal and no --send reaches no server; --dry-run neither; --send does and writes them; a 307 or 308 to another server is not followed', async () => {
  const f = await fixture();
  const s = await server(translator);
  try {
    const args = ['--strings', f.strings, '--only', 'look,books', '--out', f.out, '--llmux', s.url];
    const none = await cli(args);
    assert.equal(none.code, 1);
    const dry = await cli([...args, '--dry-run']);
    assert.equal(dry.code, 0);
    assert.deepEqual(await arrived(s), [], 'nothing reached the server without --send');
    const sent = await cli([...args, '--send']);
    assert.equal(sent.code, 3, sent.text);
    assert.deepEqual(s.seen.map((r) => [r.method, r.path]), [['POST', '/v1/messages'], ['POST', '/v1/messages']], 'one request of names, one of books');
    assert.equal(s.seen[0].body, await readFile(previewPrinted(sent.text), 'utf8'), 'the preview is what was sent first');
    const pack = await packOf(f.out);
    assert.deepEqual([pack.look.length, pack.books.length], [LOOK.length, BOOKS.length - 1]);
    nothingTold(none.text + dry.text + sent.text);
  } finally {
    await s.close();
    await rm(f.dir, { recursive: true });
  }
  for (const status of [307, 308]) {
    const many = Array.from({ length: LOOK_PER_REQUEST + 1 }, (_, i) => text('look', 30 + i, `thing ${i}`));
    const g = await fixture({ ...EXPORT, texts: [...LOOK, ...many] });
    const other = await server(() => ({ status: 200, json: {} }));
    const redirect = await server(() => ({ status, headers: { location: `${other.url}/v1/messages` } }));
    try {
      const r = await cli(['--strings', g.strings, '--only', 'look', '--out', g.out, '--llmux', redirect.url, '--send', '--jobs', '1']);
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

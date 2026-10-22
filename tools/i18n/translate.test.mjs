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
// - nothing the tool prints holds a name, a line, a script's item or a hash;
// - slice K5b, the keyword aliases (made-up words only): a script's keys from its keyword blocks
//   and the highlighted words that hit each, the request's `topics` (with `known` from the pack
//   there and from the shared keywords' request, asked once before the scripts and never before
//   the owner's yes), a request with no key byte for byte main's for the same style guide, the
//   words of one block's keys (shared only where their English means the same), the
//   alias gate (words that collide as the engine's matcher takes them, with the engine's
//   particles, kept only by keys whose first block is the same, and checked against the words the
//   pack keeps) and its refusals counted while the lines land, a key's several words, the run
//   gate with a script's words (runs and table strings), the pack's `aliases` in the engine's
//   form (a pack without them byte for byte main's; crates/u6-core/src/locale/tool-pack.json is
//   this writer's bytes, which the engine reads and writes back), the merge (new words replace
//   old, an older line that no longer passes dropped and counted, a strings-only run keeping
//   them);
// - the keywords' words per script (made-up words only): a request's `known` the script's own word
//   in the pack there, never another script's, the shared keywords' answer only where it has none
//   (and asked only then), `--fresh-keywords` (no word of the pack sent in a script's first
//   request, every shared key asked, a line asked again naming the word its key keeps, the pack's
//   where the run kept none, the words an answer keeps replacing the pack's, a failed script
//   keeping its own, said in the consent text and the coverage), a one-letter key never shared and
//   a `y` or `n` alone in its first block marked as the reply's yes or no (always sent 예 or 아니
//   as known, a request again too), which yes and no words the alias gate keeps beside keys yes
//   and no of another block, and the guide's yes and no words set by the tool (a marked key and a
//   key `yes` or `no` settle to theirs whatever the answer gives, the word winning a collision
//   with another key's, new or kept by the pack, in any order; a word that would lose not sent as
//   known, so one run cleans a pack; `yes` and `no` never asked in the shared request);
// - spec 12, the register by profile (made-up speakers only): the speaker file's strict reader and
//   its refusals (fixed messages, no request), the register for a class at an age, `--speakers`
//   putting the age, the profile and that register in the request (a script the file does not list
//   by Pass A as an adult, `--class` still winning), a request without it byte for byte main's,
//   `--ages` choosing the scripts of those ages (alone or with `--first`) and its refusals, the
//   guide's age section.
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
  AGES, CLASSES, COVERAGE_FORMAT, InputError, MAX_TOKENS, PACK_FORMAT, PARTICLES, PackReadError, SPEAKERS_FORMAT, SPEAKER_NOTE_MAX,
  aliasesOf, bodyOf, braceWords, braceWordsWanted, collide, compatibleOf, duplicateMembers, fingerprint, firstBlocksOf, gate,
  glossaryOf, hash64, hex64, hits, keyHit, keyOf, keysOf, keywordsOf, knownOf, latin1, main, packBytes, placeholders, planOf,
  readPack, readSpeakers, registerOf, settledOf, settleKeys, speakerClass, systemOf, tableGate, topicsOf, unitsOf, wordGate,
} from './translate.mjs';

const TOOL = join(dirname(fileURLToPath(import.meta.url)), 'translate.mjs');
const SENTINEL = 'Zq8NameSentinel';
const LINE = 'Zq8LineSentinel';
const ITEMS = [7001, 7002, 7003, 7004];
const HASHES = ['00000000000a7001', '00000000000a7002', '00000000000b7003', '00000000000b7004'];

const seg = (archive, item, offset, text, part) => ({ kind: 'text', key: [archive, item, offset], text, ...(part === undefined ? {} : { part_of: part }) });

// A u6.npc-bundle/1 as the page's exporter writes it, with the test's own name and lines (the
// keys are made-up offsets in script order), each topic's keywords `keywords`.
function bundle(archive, item, hash, name, { refused = null, look = 'a guard of the gate', lines = [], keywords = ['job'] } = {}) {
  const at = (n) => 100 + n * 40;
  return {
    format: 'u6.npc-bundle/1', archive, item, hash, refused, actors: [5], portraits: [], name, locale: 'en',
    narration: { look: refused ? [] : [seg(archive, item, 20, look)] },
    speech: { greeting: refused ? [] : [seg(archive, item, at(0), `"Hail, $G! I am $N. ${LINE}"\n`)] },
    turns: refused ? [] : [{
      input: { at: 90, kind: 'ask' }, segments: [],
      topics: lines.map((l, i) => ({ at: 0, keywords, requires: [], answer: [seg(archive, item, at(i + 1), l)] })),
    }],
    traits: [], strings: [],
  };
}

// Slice K5b: two scripts share `job` (below the GLOSSARY_SCRIPTS that asks a key on its own
// first), so each run here sends the scripts' requests only; the shared keywords' request has
// tests of its own below.
const BUNDLES = [
  bundle('a', ITEMS[0], HASHES[0], `${SENTINEL}Good`, {
    lines: ['My @job is to guard.*', 'I have #0 coin\\s, <runes> here.', 'The first part ', 'and the rest.'],
  }),
  bundle('a', ITEMS[1], HASHES[1], `${SENTINEL}Again`, { look: 'a merchant of bread', lines: ['Buy from me, $P.'] }),
  bundle('b', ITEMS[2], HASHES[2], `${SENTINEL}Bad`, { lines: ['I never answer well.'], keywords: ['bye'] }),
  bundle('b', ITEMS[3], HASHES[3], `${SENTINEL}Refused`, { refused: 'test refusal' }),
];
// The third and fourth lines of the first script are the two parts of one run.
BUNDLES[0].turns[0].topics[2].answer[0].part_of = 100 + 3 * 40;
BUNDLES[0].turns[0].topics[3].answer[0].part_of = 100 + 3 * 40;

// The fake translator's Korean word for key `k` (slice K5b): a made-up syllable per letter.
const fakeWord = (k) => [...k].slice(0, 8).map((c) => String.fromCharCode(0xac00 + ((c.charCodeAt(0) * 389) % 11172))).join('');

// The fake translator's Korean for `en`: every escape and the runic text kept, the line breaks at
// its ends kept, each keyword `@word` as `@{<its Korean word>}` where `words` (a Map of the
// English word, case folded, to it) has one, else `말(@{word})`, every other letter a syllable.
function fakeKo(en, words = new Map()) {
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
      const ko = words.get(word.toLowerCase());
      out += ko ? `@{${ko}}` : `말(@{${word}})`;
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

const answer = (lines, keywords) => ({
  status: 200,
  json: {
    type: 'message', content: [{ type: 'text', text: JSON.stringify(keywords ? { lines, keywords } : { lines }) }],
    usage: { input_tokens: 100, output_tokens: 20 },
  },
});

// The fake translator's answer to a script's request `user` (slice K5b): for each key the request
// lists, the word `pick` gives it (its `known` word, else a made-up one); each line with the words
// of the keys its `@`s show.
function fakeAnswer(user, pick = (key) => key.known ?? fakeWord(key.k)) {
  const keywords = [];
  const words = new Map();
  for (const t of user.topics ?? []) {
    for (const key of t.keys) {
      const ko = pick(key);
      if (!keywords.some((x) => x.k === key.k)) keywords.push({ k: key.k, ko });
      for (const w of key.shown) words.set(w, ko);
    }
  }
  return { lines: user.lines.map((l) => ({ n: l.n, ko: fakeKo(l.en, words) })), keywords: user.topics ? keywords : undefined };
}

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
    const { lines, keywords } = fakeAnswer(user);
    if (who.endsWith('Again') && asked.get(who) === 1) {
      const last = lines.at(-1);
      last.ko = last.ko.replace('$P', '그대');
    }
    return answer(lines, keywords);
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
  assert.deepEqual(Object.keys(pack), ['format', 'language', 'tool', 'base', 'converse', 'aliases']);
  assert.equal(pack.format, PACK_FORMAT);
  assert.equal(pack.language, 'ko');
  const units = [0, 1].flatMap((i) => unitsOf(BUNDLES[i]).map((u) => ({ ...u, item: ITEMS[i] })));
  assert.equal(pack.converse.length, units.length, 'every line of the two good scripts');
  const job = fakeWord('job');
  const keyed = { keys: ['job'], words: new Map([['job', job]]) };
  for (const r of pack.converse) {
    const u = units.find((x) => r.archive === 'a' && x.item === r.item && x.offset === r.offset);
    assert.ok(u, 'a run of the bundles');
    assert.equal(r.len, Buffer.from(u.text, 'latin1').length);
    assert.equal(r.hash, hex64(hash64(Buffer.from(u.text, 'latin1'))));
    assert.equal(r.text, fakeKo(u.text, new Map([['job', job]])));
    assert.equal(gate(u.text, r.text, keyed), null);
  }
  assert.ok(pack.converse.some((r) => r.text.includes(`@{${job}}`)), 'a line highlights the Korean word');
  assert.deepEqual(pack.aliases, [0, 1].map((i) => ({ archive: 'a', item: ITEMS[i], keyword: 'job', words: [job] })),
    'each good script\'s key with its Korean word (the bad one\'s none)');
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
  const { lines, keywords } = fakeAnswer(JSON.parse(JSON.parse(body).messages[0].content));
  return answer(lines, keywords).json;
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

// --- slice K5b: the keyword aliases (made-up words only) ---------------------------------------

const hashOf = (text) => hex64(hash64(latin1(text)));
const userOf = (body) => JSON.parse(JSON.parse(body).messages[0].content);
const reply = (obj) => ({ status: 200, json: { type: 'message', content: [{ type: 'text', text: JSON.stringify(obj) }] } });
const ALIASES_PART = 'its keyword aliases part (aliases) holds an entry of another form or for a script its lines do not name';

// A bundle of the keyword cases, every word of it made up: `topics`, each [its keywords as the
// script holds them, its answer's line]; `strings`, its table strings.
function kbundle(archive, item, hash, name, topics, strings = []) {
  return {
    format: 'u6.npc-bundle/1', archive, item, hash, refused: null, actors: [5], portraits: [], name, locale: 'en',
    narration: { look: [seg(archive, item, 20, 'a keeper of words')] },
    speech: { greeting: [seg(archive, item, 60, 'Hail.')] },
    turns: [{
      input: { at: 90, kind: 'ask' }, segments: [],
      topics: topics.map(([keywords, line], i) => ({ at: 0, keywords, requires: [], answer: [seg(archive, item, 100 + i * 40, line)] })),
    }],
    traits: [],
    strings: strings.map((text, i) => ({ key: [archive, item, 2000 + i * 40], table: 2000, index: i, text })),
  };
}

async function kfixture(bundles, strings = null) {
  const dir = await mkdtemp(join(tmpdir(), 'u6-i18n-keywords-'));
  const path = join(dir, 'ultima6-npc-bundles.json');
  await writeFile(path, JSON.stringify(bundles));
  const spath = strings ? join(dir, 'ultima6-strings.json') : null;
  if (strings) await writeFile(spath, JSON.stringify(strings));
  return { dir, bundles: path, strings: spath, out: join(dir, 'ko') };
}

// The tool run with `args` and `send`, stdin no terminal: its exit code and what it printed.
async function run(args, send, extra = {}) {
  const out = output();
  const code = await main(args, { transport: send, stdout: out, isTTY: false, signals: new EventEmitter(), ...extra });
  return { code, text: out.text };
}

// Nothing printed names a script of `bundles`, a hash, a keyword's word or a Korean word.
function quiet(printed, bundles) {
  nothingTold(printed);
  for (const b of bundles) {
    assert.ok(!printed.includes(b.name) && !printed.includes(b.hash), 'no name or hash printed');
    assert.ok(!new RegExp(`(^|[^0-9])${b.item}([^0-9]|$)`).test(printed), 'no item printed');
  }
  assert.ok(!/[가-힣]{2}/.test(printed.replace(/한글/g, '')), 'no Korean word printed');
}

// None of `words` (a fixture's keywords, kept and refused alike, and its alias words, given or
// refused) is in `text` (what the tool printed, or a coverage receipt): an ASCII one as a word of
// its own, a Korean one anywhere, one syllable too.
function noneOf(text, words) {
  const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  for (const w of words.filter((x) => typeof x === 'string' && x.length > 0)) {
    const shown = /^[\x20-\x7e]+$/.test(w) ? new RegExp(`(^|[^A-Za-z0-9])${esc(w)}([^A-Za-z0-9]|$)`, 'i').test(text) : text.includes(w);
    assert.ok(!shown, `a keyword or an alias word of the fixture is shown: ${JSON.stringify(w)}`);
  }
}

// Three scripts that share the key plo (enough for the shared keywords' request), two fl?n, each
// one of its own.
const SHARE_ITEMS = [7601, 7602, 7603];
const sharing = () => SHARE_ITEMS.map((item, i) => kbundle('a', item, `00000000000f${item}`, `${SENTINEL}Share${'xyz'[i]}`, [
  [['plo'], 'Try my @plod.'],
  ...(i < 2 ? [[['fl?n'], 'A @flan.']] : []),
  [[`my${'abc'[i]}`], `Mine is @my${'abc'[i]}.`],
]));
const SHARE_KEYS = ['plo', 'plod', 'fl?n', 'flan', 'mya', 'myb', 'myc'];
// Coverage `keywords.kept` of a run in which no key kept the pack's old word.
const NONE_KEPT = { count: 0, byReason: {}, keys: [] };

test('slice K5b: a script\'s keys are its keywords as the keyword block keys them (up to the NUL, case folded; a * one and one past the form left out), in the blocks\' order, each with the highlighted words that hit it first (a word longer than its key, a ? in it)', () => {
  assert.equal(keyOf('Plo\0xyz'), 'plo', 'up to its NUL, case folded');
  for (const k of ['*', '*plo', 'a,b', '', 'x'.repeat(33), `caf${String.fromCharCode(0xe9)}`]) assert.equal(keyOf(k), null, JSON.stringify(k));
  assert.equal(keyOf('x'.repeat(32)), 'x'.repeat(32));
  assert.ok(hits('plod', 'plo') && hits('flan', 'fl?n') && hits('fl', 'fl?'), 'a longer word; ? anywhere, past the word\'s end too');
  assert.ok(!hits('gr', 'plo') && !hits('grig', 'plo') && !hits('', '?'));
  const b = kbundle('a', 7501, '00000000000e7501', `${SENTINEL}Keys`, [
    [['Plo\0xyz', 'plods'], 'Try my @plod, or @Plods.'],
    [['fl?n'], 'A @flan for thee.'],
    [['*'], 'Naught.'],
    [['zub'], 'The @zubbo.'],
    [['zubbo', 'zub'], 'More @zubbo.'],
    [['quux'], 'Nothing marked here.'],
    [['a,b', 'x'.repeat(33)], 'Odd keys.'],
  ]);
  assert.deepEqual(keysOf(b), {
    topics: [['plo', 'plods'], ['fl?n'], ['zub'], ['zubbo', 'zub'], ['quux']],
    keys: ['plo', 'plods', 'fl?n', 'zub', 'zubbo', 'quux'],
  });
  const [s] = planOf([b], { item: { archive: 'a', item: 7501 } }).scripts;
  assert.deepEqual(Object.fromEntries(s.shown), { plo: ['plod', 'plods'], plods: [], 'fl?n': ['flan'], zub: ['zubbo'], zubbo: [], quux: [] },
    'plods hits plo first; zubbo hits zub first; quux is highlighted nowhere');
  assert.equal(keyHit('zubbo', s.keys), 'zub');
  assert.deepEqual(topicsOf(s, new Map([['zub', '나']])), [
    { keys: [{ k: 'plo', shown: ['plod', 'plods'] }, { k: 'plods', shown: [] }] },
    { keys: [{ k: 'fl?n', shown: ['flan'] }] },
    { keys: [{ k: 'zub', shown: ['zubbo'], known: '나' }] },
    { keys: [{ k: 'zubbo', shown: [] }] },
    { keys: [{ k: 'quux', shown: [] }] },
  ], 'one entry per block with keys, a key two blocks list only in its first (listed twice, it may be answered twice), known where settled');
});

// The request's bytes include the style guide, which this slice changes on purpose: the claim is
// for the same guide (here the literal 'STYLE' both sides).
test('slice K5b: a request of a script with no key is byte for byte main\'s (b0df2aa) for the same input and the same style guide (the guide is part of every request; this slice changes it on purpose), its preview too; with keys the request lists them and its instruction asks a word for each', async () => {
  const NOKEY = {
    format: 'u6.npc-bundle/1', archive: 'a', item: 7301, hash: '00000000000d7301', refused: null, actors: [], portraits: [], name: 'Zq8Plain', locale: 'en',
    narration: { look: [seg('a', 7301, 20, 'a plain test')] },
    speech: { greeting: [seg('a', 7301, 60, 'Hail, $G. Ask of my @wares.')] },
    turns: [{ input: { at: 90, kind: 'ask' }, segments: [], topics: [{ at: 0, keywords: ['*'], requires: [], answer: [seg('a', 7301, 100, 'Naught more.')] }] }],
    traits: [], strings: [{ key: ['a', 7301, 200], table: 200, index: 0, text: 'a cup' }],
  };
  const [s] = planOf([NOKEY], { item: { archive: 'a', item: 7301 } }).scripts;
  assert.deepEqual(s.keys, [], 'a * keyword is no key');
  const body = bodyOf('gpt-6-astra', systemOf('STYLE'), s.bundle, s.cls, s.units.filter((u) => !u.done));
  assert.equal(body.length, 1091);
  assert.equal(hex64(hash64(Buffer.from(body))), '8dfd9fc326baf037', 'main\'s body, byte for byte');
  const f = await kfixture([NOKEY]);
  const r = await run([f.bundles, '--item', 'a:7301', '--out', f.out, '--dry-run'], null, { style: 'STYLE' });
  assert.equal(r.code, 0);
  assert.equal(await readFile(previewPrinted(r.text), 'utf8'), body, 'the preview is that body');
  quiet(r.text, [NOKEY]);
  const keyed = systemOf('STYLE', true);
  assert.notEqual(keyed, systemOf('STYLE'));
  assert.ok(keyed.includes('"topics"') && keyed.includes('"keywords": [{"k": "<keyword>", "ko":') && keyed.includes('@{<the English word>}') && keyed.endsWith('\nSTYLE'));
  await rm(f.dir, { recursive: true });
});

test('slice K5b: the known words: the script\'s own in the pack there (its first word the alias gate takes, never the word other scripts give the key), else the shared keywords\' answer; a key at least 3 scripts hold is asked once before the scripts (the preview), its word reaching every script\'s request; a key every script holding it has a word for is not asked; that request failing is counted and the scripts go on', async () => {
  const al = (item, keyword, words) => ({ archive: 'a', item, keyword, words });
  const there = { aliases: [al(1, 'plo', ['술']), al(2, 'plo', ['술']), al(3, 'plo', ['탁주']), al(1, 'zub', ['abc', '나']), al(2, 'zub', ['가']), al(3, 'quux', ['abc'])] };
  const own = (item, keys) => [...knownOf(there, { bundle: { archive: 'a', item }, keys })];
  assert.deepEqual(own(3, ['plo', 'zub', 'quux']), [['plo', '탁주']], 'its own word, not the one most scripts give; one the gate refuses, or none, gives none');
  assert.deepEqual(own(1, ['plo', 'zub']), [['plo', '술'], ['zub', '나']], 'its first word the gate takes');
  assert.deepEqual(own(1, ['zub']), [['zub', '나']], 'its keys only');
  assert.deepEqual(own(1, ['plo']), [['plo', '술']]);
  assert.deepEqual(own(4, ['plo']), [], 'a script the pack gives no word');
  assert.deepEqual([...knownOf(null, { bundle: { archive: 'a', item: 1 }, keys: ['plo'] })], [], 'no pack');
  const items = SHARE_ITEMS;
  const bundles = sharing();
  const secrets = [...SHARE_KEYS, '술잔', ...['fl?n', 'mya', 'myb', 'myc'].map(fakeWord)];
  // `glossary(user, n)` answers the shared keywords' request (the n-th time it is asked).
  const transport = (glossary) => {
    const bodies = [];
    let n = 0;
    const send = async (body) => {
      bodies.push(body);
      const user = userOf(body);
      if (user.keywords) {
        n += 1;
        return glossary(user, n);
      }
      const { lines, keywords } = fakeAnswer(user);
      return answer(lines, keywords);
    };
    return { send, bodies };
  };
  const f = await kfixture(bundles);
  const t = transport(() => reply({ keywords: [{ k: 'plo', ko: '술잔' }] }));
  const r = await run([f.bundles, '--first', '3', '--out', f.out, '--send', '--jobs', '2'], t.send);
  assert.equal(r.code, 0, 'every line and key given Korean');
  assert.deepEqual(userOf(t.bodies[0]), { keywords: [{ k: 'plo', shown: ['plod'] }] }, 'the key three scripts hold, asked first and alone');
  assert.ok(JSON.parse(t.bodies[0]).system.includes('사극체'), 'the guide goes with it');
  assert.equal(await readFile(previewPrinted(r.text), 'utf8'), t.bodies[0], 'the preview is that request');
  assert.equal(t.bodies.length, 4, 'then one request per script');
  for (const body of t.bodies.slice(1)) {
    const keys = userOf(body).topics.flatMap((x) => x.keys);
    assert.deepEqual(keys.find((x) => x.k === 'plo'), { k: 'plo', shown: ['plod'], known: '술잔' }, 'its word reaches every script');
    assert.ok(keys.every((x) => x.k === 'plo' || x.known === undefined), 'no other key settled');
  }
  assert.match(r.text, /It sends the owner's game text: 4 requests, .*the keywords at least 3 of the scripts share .*the shared keywords first/);
  assert.match(r.text, /Shared keywords: 1 asked before the scripts, 1 given a word\./);
  const pack = await packOf(f.out);
  assert.deepEqual(pack.aliases.filter((a) => a.keyword === 'plo').map((a) => [a.item, a.words]), items.map((i) => [i, ['술잔']]));
  assert.ok(pack.converse.some((x) => x.text.includes('@{술잔}')), 'a line highlights it');
  const coverage = JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8'));
  assert.deepEqual(coverage.glossary, { found: 1, translated: 1, leftEnglish: 0, byReason: {}, unexplained: 0 });
  assert.deepEqual(coverage.keywords, { found: 8, translated: 8, kept: NONE_KEPT, leftEnglish: 0, byReason: {}, unexplained: 0, dropped: 0 });
  quiet(r.text, bundles);
  noneOf(r.text + await readFile(join(f.out, 'coverage.json'), 'utf8'), secrets);
  // Again on the pack it wrote: the pack settles plo (and fl?n), so nothing is asked first.
  const t2 = transport(() => assert.fail('no shared keywords asked'));
  const again = await run([f.bundles, '--first', '3', '--out', f.out, '--send', '--jobs', '2'], t2.send);
  assert.equal(again.code, 0);
  assert.equal(t2.bodies.length, 3);
  for (const body of t2.bodies) {
    const keys = userOf(body).topics.flatMap((x) => x.keys);
    assert.equal(keys.find((x) => x.k === 'plo').known, '술잔', 'known from the pack there');
  }
  quiet(again.text, bundles);
  noneOf(again.text + await readFile(join(f.out, 'coverage.json'), 'utf8'), secrets);
  // A fresh run whose shared keywords' answer never reads: counted; the scripts go without it.
  const g = await kfixture(bundles);
  const t3 = transport(() => reply({ nothing: true }));
  const failed = await run([g.bundles, '--first', '3', '--out', g.out, '--send', '--jobs', '2'], t3.send);
  assert.equal(failed.code, 0, 'the scripts answered their keys themselves');
  assert.equal(t3.bodies.filter((b) => userOf(b).keywords).length, 2, 'asked, then once more (the transport\'s)');
  assert.equal(t3.bodies.filter((b) => userOf(b).npc).length, 3, 'every script still asked');
  assert.ok(t3.bodies.filter((b) => userOf(b).npc).every((b) => userOf(b).topics.flatMap((x) => x.keys).every((x) => x.known === undefined)), 'with no known word');
  assert.deepEqual(JSON.parse(await readFile(join(g.out, 'coverage.json'), 'utf8')).glossary, { found: 1, translated: 0, leftEnglish: 1, byReason: { invalid: 1 }, unexplained: 0 });
  assert.match(failed.text, /Shared keywords: 1 asked before the scripts, 0 given a word \(invalid 1\)\./);
  quiet(failed.text, bundles);
  noneOf(failed.text + await readFile(join(g.out, 'coverage.json'), 'utf8'), secrets);
  await rm(f.dir, { recursive: true });
  await rm(g.dir, { recursive: true });
});

test('slice K5b: the alias gate: a word is 1 to 8 Hangul syllables (no ASCII, no space, not empty); two keys whose words collide (a word of the input the engine takes for both: the word, or it with one particle) keep them only when their first block is the same, else both are refused; the glossary needs that in every script holding both', () => {
  assert.equal(wordGate('한말'), null);
  assert.equal(wordGate('가'.repeat(8)), null);
  for (const [w, why] of [['', 'empty'], [null, 'empty'], ['abc', 'characters'], ['술 잔', 'characters'], ['한a', 'characters'], ['ㅎ말', 'characters'], ['가'.repeat(9), 'long']]) {
    assert.equal(wordGate(w), why, JSON.stringify(w));
  }
  const compatible = compatibleOf([['kah', 'kai'], ['kaf'], ['kag'], ['p', 'q'], ['r']]);
  const { words, failed } = aliasesOf(['kah', 'kai', 'kaf', 'kag', 'p', 'q', 'r'], [
    { k: 'kah', ko: '한말' }, { k: 'kai', ko: '한말' }, { k: 'kaf', ko: '같은말' }, { k: 'kag', ko: '같은말' },
    { k: 'p', ko: '셋' }, { k: 'q', ko: '셋' }, { k: 'r', ko: '셋' }, { k: 'other', ko: '헛말' },
  ], compatible);
  assert.deepEqual(Object.fromEntries(words), { kah: '한말', kai: '한말' }, 'one block\'s keys share a word');
  assert.deepEqual(Object.fromEntries(failed), { kaf: 'shared', kag: 'shared', p: 'shared', q: 'shared', r: 'shared' },
    'two blocks sharing a word: every key with it refused');
  // F1: a collision is what the engine's matcher takes for both, not only the same word.
  assert.ok(collide('달', '달') && collide('달', '달이') && collide('달이', '달'), 'the word, and it with a particle');
  assert.ok(collide('가', '가이여') && collide('가', '가으'), 'with a particle of two syllables; both with one (가으로)');
  assert.ok(!collide('달', '별') && !collide('달', '달빛') && !collide('달이', '달가'));
  const zp = (topics, given) => {
    const r = aliasesOf(['zorb', 'plim'], given, compatibleOf(topics));
    return [Object.fromEntries(r.words), Object.fromEntries(r.failed)];
  };
  assert.deepEqual(zp([['zorb'], ['plim']], [{ k: 'zorb', ko: '달' }, { k: 'plim', ko: '달이' }]), [{}, { zorb: 'shared', plim: 'shared' }],
    'typing 달이 would reach zorb\'s block through 달 and 이');
  assert.deepEqual(zp([['zorb'], ['plim']], [{ k: 'zorb', ko: '달' }, { k: 'plim', ko: '별' }]), [{ zorb: '달', plim: '별' }, {}]);
  // F2: colliding words are kept only when the two keys' first block is the same.
  assert.deepEqual([...firstBlocksOf([['zorb'], ['zorb', 'plim']])], [['zorb', 0], ['plim', 1]]);
  assert.deepEqual(zp([['zorb'], ['zorb', 'plim']], [{ k: 'zorb', ko: '달' }, { k: 'plim', ko: '달' }]), [{}, { zorb: 'shared', plim: 'shared' }],
    'block 1 lists zorb alone: 달 reaches it, while English plim reaches block 2');
  assert.deepEqual(zp([['zorb', 'plim'], ['plim']], [{ k: 'zorb', ko: '달' }, { k: 'plim', ko: '달' }]), [{ zorb: '달', plim: '달' }, {}],
    'their first block the same');
  // The glossary: the same first block in every chosen script that holds both keys.
  const scripts = (lists) => planOf(lists.map((topics, i) => kbundle('a', 7400 + i, `00000000001a740${i}`, `${SENTINEL}G${i}`,
    topics.map((keys) => [keys, 'Naught.']))), { first: lists.length }).scripts;
  const same = [[['zorb', 'plim']], [['zorb', 'plim'], ['zorb']], [['plim', 'zorb']]];
  assert.equal(glossaryOf(scripts(same)).compatible('zorb', 'plim'), true);
  assert.equal(glossaryOf(scripts([...same, [['zorb'], ['zorb', 'plim']]])).compatible('zorb', 'plim'), false, 'one script differs');
  assert.equal(glossaryOf(scripts([[['zorb']], [['zorb']], [['zorb']], [['plim']]])).compatible('zorb', 'plim'), true, 'no script holds both');
});

test('slice K5b: the words of one block\'s keys: the instruction and the guide ask a key\'s word in all its lines, the keys of one block (leading to one reply) sharing a word only when the English words their lines show mean the same thing, else each the word for its own English (a key no line shows, its own keyword\'s), the sharing rule never replacing a key\'s known word; neither asks one word for every key of a block nor lets known override every rule, and the alias gate keeps one block\'s keys with different words', async () => {
  // Whitespace collapsed: how either text is wrapped is no part of the rule.
  const flat = (s) => s.replace(/\s+/g, ' ');
  const keyed = flat(systemOf('STYLE', true));
  assert.ok(keyed.includes('(the keys of one block lead to one reply;') && !keyed.includes('are one answer'), 'a block leads to one reply, not "one answer"');
  assert.ok(keyed.includes('the same word for a key in all its lines, its "known" word where one is given'), 'a key\'s word in all its lines');
  assert.ok(keyed.includes('the keys of one block may share a word only when the English words their lines show mean the same thing'),
    'a block\'s keys share a word only when their English means the same');
  assert.ok(keyed.includes('else each takes the word for its own English (a key no line shows: the word for its own keyword) (each line shows its own key\'s word);'),
    'else each its own, a key no line shows its keyword\'s');
  assert.ok(keyed.includes('(each line shows its own key\'s word); the sharing rule never replaces a key\'s "known" word.'), 'known over the sharing rule');
  assert.ok(!keyed.includes('the same word for the keys of one block'), 'the instruction no longer asks one word for a block');
  assert.ok(!keyed.includes('this is for the words you choose') && !keyed.includes('a key given "known" keeps it'), 'nor lets known override every rule');
  const guide = flat(await readFile(join(dirname(TOOL), 'style-ko.md'), 'utf8'));
  assert.ok(guide.includes('Use a key\'s word in every line of the character; when the request gives `known`, use that word.'), 'the guide: a key\'s word');
  assert.ok(guide.includes('The keys of one keyword block lead to one reply, so they may share a word, but only when the English words their lines show mean the same thing'),
    'the guide: a block\'s keys share a word only when their English means the same');
  assert.ok(guide.includes('otherwise each key takes the word for its own English (a key no line shows, the word for its own keyword)'),
    'the guide: else each its own, a key no line shows its keyword\'s');
  assert.ok(guide.includes('since each line shows its own key\'s word. The sharing rule never replaces a key\'s `known` word, even where another key of its block keeps the same word.'),
    'the guide: known over the sharing rule');
  assert.ok(guide.includes('This holds over `known`: a name key\'s `known` is used only when it is already the name transliterated'), 'the name rule over known still stands');
  assert.ok(!guide.includes('for the keys of one keyword block') && !guide.includes('Use the same word in every line of the character'),
    'the guide no longer asks one word for a block');
  assert.ok(!guide.includes('This is for the words you choose') && !guide.includes('a key given `known` keeps it'), 'nor lets known override every rule');
  const { words, failed } = aliasesOf(['zorb', 'plim'], [{ k: 'zorb', ko: '달' }, { k: 'plim', ko: '별' }], compatibleOf([['zorb', 'plim']]));
  assert.deepEqual([Object.fromEntries(words), Object.fromEntries(failed)], [{ zorb: '달', plim: '별' }, {}], 'one block, two words: both kept');
});

test('slice K5b: the tool\'s particles are the engine\'s (crates/u6-core/src/converse/run.rs PARTICLES), in their order', async () => {
  const source = await readFile(join(dirname(TOOL), '..', '..', 'crates', 'u6-core', 'src', 'converse', 'run.rs'), 'utf8');
  const m = /pub\(crate\) const PARTICLES: \[&str; (\d+)\] = \[([^\]]*)\];/.exec(source);
  assert.ok(m, 'the engine\'s list is found');
  const engine = [...m[2].matchAll(/"([^"]*)"/g)].map((x) => x[1]);
  assert.equal(engine.length, Number(m[1]));
  assert.deepEqual(PARTICLES, engine);
});

test('slice K5b: the run gate with a script\'s words: each @{…} the Korean word of the key its English @ word hits first, or that English word where the key has none or it hits no key, the particle after the brace; a table string the same; without words today\'s English rule', () => {
  const keyed = { keys: ['plo', 'fl?n', 'zub'], words: new Map([['plo', '술'], ['fl?n', '과자']]) };
  assert.deepEqual(braceWordsWanted('My @Plod, @flan, @zubbo and @other.', keyed), [['술'], ['과자'], ['zubbo'], ['other']]);
  const ok = [
    ['My @plod.', '내 @{술}이오.'],
    ['My @Plod.', '내 @{술}을 드시오.'],
    ['A @flan or @plod?', '@{술}{이/가} 아니면 @{과자}?'],
    ['The @zubbo.', '@{zubbo}이오.'],
    ['Ask @other.', '@{other}에게 묻소.'],
  ];
  for (const [en, ko] of ok) assert.equal(gate(en, ko, keyed), null, `${en} -> ${ko}`);
  const bad = [
    ['My @plod.', '내 @{plod}이오.', 'the English word where the key has a word'],
    ['My @plod.', '내 술(@{plod})이오.', 'K1\'s form'],
    ['My @plod.', '내 @{과자}이오.', 'another key\'s word'],
    ['My @plod.', '내 @{술이}오.', 'the particle inside the brace'],
    ['My @plod.', '내 @{술{이/가}} 있소.', 'a marker inside the brace'],
    ['The @zubbo.', '@{나}이오.', 'a Korean word where the key has none'],
  ];
  for (const [en, ko, what] of bad) assert.equal(gate(en, ko, keyed), 'keywords', what);
  assert.equal(gate('My @plod.', '내 @{plod}이오.'), null, 'no words: the English word');
  assert.equal(gate('My @plod.', '내 @{술}이오.'), 'keywords', 'no words: not a Korean one');
  const table = { text: 'a @plod cup', copied: false, capacity: null };
  assert.equal(tableGate(table, '@{술} 한 잔', keyed), null, 'a table string with the Korean word');
  assert.equal(tableGate(table, '@{plod} 한 잔', keyed), 'keywords', 'not with the English one');
  // F4: a key with several words (a pack written by hand): any of them, each brace its own.
  const many = { keys: ['ambo', 'brix'], words: new Map([['ambo', ['가', '나']], ['brix', ['다']]]) };
  assert.deepEqual(braceWordsWanted('@ambo @brix', many), [['가', '나'], ['다']]);
  for (const ko of ['@{가}에게 묻소.', '@{나}에게 묻소.']) assert.equal(gate('Ask of @ambo.', ko, many), null, ko);
  assert.equal(gate('Ask of @ambo.', '@{다}에게 묻소.', many), 'keywords', 'not another key\'s');
  assert.equal(gate('The @ambo and @ambo, @brix.', '@{나}와 @{가}, @{다}.', many), null, 'two of one key\'s words');
  assert.equal(gate('The @ambo and @brix.', '@{다}와 @{가}.', many), null, 'in any order');
  assert.equal(gate('The @ambo and @ambo.', '@{다}와 @{가}.', many), 'keywords');
});

test('slice K5b: the alias gate\'s refusals (ASCII, too long, empty, a space, two blocks one word, missing, given twice) are counted by reason, by hashes only, while every line lands (English braces for those keys); a table string with an aliased key lands with its word; a line asked again lists the keys with a word, each known', async () => {
  const keys = ['kab', 'kac', 'kad', 'kae', 'kaf', 'kag', 'kaj', 'kak'];
  const b = kbundle('a', 7801, '0000000000107801', `${SENTINEL}Refusals`, [
    ...keys.map((k) => [[k], `Thy @${k}.`]),
    [['kah', 'kai'], 'Thy @kah or @kai.'],
    [['kah'], 'The @kah, once more.*'],
  ], ['a @kah cup']);
  const given = [
    { k: 'kab', ko: 'abc' }, { k: 'kac', ko: '가'.repeat(9) }, { k: 'kad', ko: '' }, { k: 'kae', ko: '술 잔' },
    { k: 'kaf', ko: '같은말' }, { k: 'kag', ko: '같은말' }, { k: 'kah', ko: '한말' }, { k: 'kai', ko: '한말' },
    { k: 'kak', ko: '하나' }, { k: 'kak', ko: '둘' }, { k: 'zzz', ko: '헛말' },
  ];
  const kept = new Map([['kah', '한말'], ['kai', '한말']]);
  const bodies = [];
  const send = async (body) => {
    bodies.push(body);
    const user = userOf(body);
    const lines = user.lines.map((l) => ({ n: l.n, ko: fakeKo(l.en, kept) }));
    if (bodies.length === 1) lines.find((l, i) => user.lines[i].en.endsWith('*')).ko = lines.find((l, i) => user.lines[i].en.endsWith('*')).ko.replace('*', '');
    return answer(lines, given);
  };
  const f = await kfixture([b]);
  const r = await run([f.bundles, '--item', 'a:7801', '--out', f.out, '--send'], send);
  assert.equal(r.code, 3, 'written, keys left English');
  assert.equal(bodies.length, 2, 'the script, then the failed line again');
  const retry = userOf(bodies[1]);
  assert.equal(retry.lines.length, 1);
  assert.deepEqual(retry.topics, [
    { keys: [{ k: 'kah', shown: ['kah'], known: '한말' }, { k: 'kai', shown: ['kai'], known: '한말' }] },
  ], 'only the keys with a word, each with it, each once');
  const pack = await packOf(f.out);
  assert.equal(pack.converse.length, unitsOf(b).length, 'every line landed');
  assert.deepEqual(pack.tables.map((x) => x.text), [fakeKo('a @kah cup', kept)]);
  assert.ok(pack.tables[0].text.includes('@{한말}'));
  assert.deepEqual(pack.aliases, [
    { archive: 'a', item: 7801, keyword: 'kah', words: ['한말'] }, { archive: 'a', item: 7801, keyword: 'kai', words: ['한말'] },
  ]);
  const coverage = JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8'));
  assert.deepEqual(coverage.keywords, {
    found: 10, translated: 2, kept: NONE_KEPT, leftEnglish: 8, byReason: { characters: 2, empty: 1, long: 1, missing: 1, repeated: 1, shared: 2 }, unexplained: 0,
    dropped: 0,
  });
  const left = coverage.left.filter((x) => x.category === 'keywords');
  assert.deepEqual(left.map((x) => [x.hash, x.reason]).sort(), [
    ['kab', 'characters'], ['kac', 'long'], ['kad', 'empty'], ['kae', 'characters'], ['kaf', 'shared'], ['kag', 'shared'], ['kaj', 'missing'], ['kak', 'repeated'],
  ].map(([k, why]) => [hashOf(k), why]).sort());
  assert.ok(left.every((x) => x.script === b.hash));
  assert.ok(!keys.some((k) => JSON.stringify(coverage).includes(`"${k}"`)), 'no keyword in the receipt');
  assert.match(r.text, /Keywords: 10 found, 2 given a Korean word, 8 left English \(characters 2, empty 1, long 1, missing 1, repeated 1, shared 2\)\./);
  quiet(r.text, [b]);
  noneOf(r.text + await readFile(join(f.out, 'coverage.json'), 'utf8'), [...keys, 'kah', 'kai', 'zzz', ...given.map((x) => x.ko)]);
  await rm(f.dir, { recursive: true });
});

// The parts of a pack of every kind but aliases (the same inputs main's packBytes was given).
const PARTS = {
  base: '0123456789abcdef',
  runs: [{ archive: 'b', item: 2, offset: 9, len: 4, hash: '0000000000000001', text: '나무' }, { archive: 'a', item: 5, offset: 3, len: 4, hash: '0000000000000002', text: '일(@{job})' }],
  tables: [{ archive: 'a', item: 5, offset: 40, len: 3, hash: '0000000000000003', capacity: 9, text: '잔' }, { archive: 'a', item: 5, offset: 30, len: 3, hash: '0000000000000004', capacity: null, text: '컵' }],
  strings: [{ hash: '00000000000000f5', len: 4, text: '찾기' }],
  look: [{ index: 7, len: 3, hash: '0000000000000006', text: '돌' }],
  books: [{ index: 2, len: 5, hash: '0000000000000007', text: '책이오.' }],
  u: [{ hash: '0000000000000008', len: 5, text: '%s님' }],
};

// The pack whose JSON is crates/u6-core/src/locale/tool-pack.json (the engine's test reads it and
// writes it back): two scripts, a table string, keyword aliases given out of order.
const TOOL_PACK = {
  tool: 'tools/i18n/translate.mjs/5',
  base: fingerprint([{ archive: 'a', item: 3, hash: '00000000000000a3' }, { archive: 'b', item: 7, hash: '00000000000000b7' }]),
  runs: [
    { archive: 'b', item: 7, offset: 12, len: 9, hash: '00000000000000c1', text: '그대의 @{주브}.' },
    { archive: 'a', item: 3, offset: 4, len: 13, hash: '00000000000000c2', text: '내 @{술}을 드시오.' },
  ],
  tables: [{ archive: 'a', item: 3, offset: 40, len: 8, hash: '00000000000000c3', capacity: 30, text: '@{과자} 하나.' }],
  aliases: [
    { archive: 'b', item: 7, keyword: 'zub', words: ['주브'] },
    { archive: 'a', item: 3, keyword: 'plo', words: ['술'] },
    { archive: 'a', item: 3, keyword: 'fl?n', words: ['과자'] },
  ],
};
const TOOL_PACK_FILE = join(dirname(TOOL), '..', '..', 'crates', 'u6-core', 'src', 'locale', 'tool-pack.json');

test('slice K5b: the pack\'s aliases in the engine\'s form: by script then keyword, {archive, item, keyword, words}, read back by the reader; a pack without aliases is byte for byte main\'s; crates/u6-core/src/locale/tool-pack.json is this writer\'s bytes', async () => {
  const k4 = { ...PARTS, tool: 'tools/i18n/translate.mjs/4' };
  for (const pack of [packBytes(k4), packBytes({ ...k4, aliases: [] })]) {
    assert.equal(pack.length, 772);
    assert.equal(hex64(hash64(pack)), '5a35b5cc3dd12650', 'main\'s pack (b0df2aa), byte for byte: no aliases part');
  }
  const bytes = packBytes(TOOL_PACK);
  const json = JSON.parse(bytes.subarray(5).toString('utf8'));
  assert.deepEqual(Object.keys(json), ['format', 'language', 'tool', 'base', 'converse', 'tables', 'aliases']);
  assert.deepEqual(json.aliases.map((a) => Object.keys(a)), [0, 1, 2].map(() => ['archive', 'item', 'keyword', 'words']));
  assert.deepEqual(json.aliases.map((a) => `${a.archive}${a.item}:${a.keyword}`), ['a3:fl?n', 'a3:plo', 'b7:zub'], 'by script, then keyword');
  const read = readPack(bytes);
  assert.deepEqual(read.aliases, json.aliases);
  assert.deepEqual(packBytes({ ...TOOL_PACK, runs: read.converse, tables: read.tables, aliases: read.aliases }), bytes, 'written back the same');
  assert.deepEqual(bytes.subarray(5), await readFile(TOOL_PACK_FILE), 'the committed file is these bytes (after the magic)');
});

test('slice K5b: the reader takes the aliases part as the engine does (ASCII words too) and refuses each entry of another form, a script its lines do not name, a key twice, a member twice inside it, with the part\'s sentence; a part given twice at the root is the shape\'s', () => {
  const runA3 = { archive: 'a', item: 3, offset: 4, len: 2, hash: '0000000000000001', text: '가' };
  const head = { format: PACK_FORMAT, language: 'ko', tool: 't/1', base: { converse: fingerprint([]) }, converse: [runA3] };
  const file = (aliases) => Buffer.from(`U6LP\n${JSON.stringify({ ...head, aliases })}`);
  const raw = (text) => Buffer.from(`U6LP\n${JSON.stringify(head).slice(0, -1)},"aliases":${text}}`);
  const entry = (archive, item, keyword, words) => ({ archive, item, keyword, words });
  const good = entry('a', 3, 'name', ['이름', 'na?e1']);
  assert.deepEqual(readPack(file([good])).aliases, [good]);
  assert.deepEqual(readPack(raw(JSON.stringify([good]))).aliases, [good]);
  assert.deepEqual(readPack(packBytes({ base: fingerprint([]), runs: [runA3] })).aliases, [], 'none when left out');
  const nine = '가'.repeat(9);
  for (const bad of [
    [entry('a', 3, 'Name', ['이름'])], [entry('a', 3, 'na,me', ['이름'])], [entry('a', 3, '', ['이름'])],
    [entry('a', 3, 'name', [])], [entry('a', 3, 'name', ['가', '나', '다', '라', '마'])], [entry('a', 3, 'name', [nine])],
    [entry('a', 3, 'name', ['이 름'])], [entry('a', 3, 'name', ['ㅇ름'])], [entry('a', 3, 'name', [''])],
    [entry('c', 3, 'name', ['이름'])], [entry('a', 4, 'name', ['이름'])], [entry('a', 70000, 'name', ['이름'])], [good, good],
    [{ archive: 'a', item: 3, keyword: 'name' }], [{ ...good, x: 1 }], null, { name: ['이름'] },
  ]) {
    assert.throws(() => readPack(file(bad)), (err) => err instanceof PackReadError && err.message === ALIASES_PART, JSON.stringify(bad));
  }
  for (const twice of [
    '[{"archive":"a","item":3,"keyword":"name","words":[],"words":["이름"]}]',
    '[{"archive":"a","item":3,"keyword":"Name","keyword":"name","words":["이름"]}]',
  ]) {
    assert.ok(duplicateMembers(`{"aliases":${twice}}`));
    assert.throws(() => readPack(raw(twice)), (err) => err instanceof PackReadError && err.message === ALIASES_PART, twice);
  }
  const root = Buffer.from(`U6LP\n${JSON.stringify({ ...head, aliases: [good] }).slice(0, -1)},"aliases":[]}`);
  assert.throws(() => readPack(root), (err) => err instanceof PackReadError && err.message === 'its contents are not those of a Korean pack');
});

test('slice K5b: the merge: a script answered again has its new words replace the old for the same keys, its other keys\' and the other scripts\' aliases staying; an older line of it that no longer passes is dropped and counted; a script whose lines all fail keeps no aliases (no-base); a strings-only run keeps them all', async () => {
  const [X, Y, Z] = [7701, 7702, 7703];
  const bundles = [
    kbundle('a', X, '0000000000117701', `${SENTINEL}MergeX`, [[['plo'], 'Try my @plod.'], [['plo'], 'Ask of my @plod again.'], [['quux'], 'Naught here.']]),
    kbundle('a', Y, '0000000000117702', `${SENTINEL}MergeY`, [[['plo'], 'Thy @plod, friend.']]),
    kbundle('a', Z, '0000000000117703', `${SENTINEL}MergeZ`, [[['zed'], 'Never a @zed.']]),
  ];
  const strings = { format: 'u6.string-bundle/1', locale: 'en', strings: [{ hash: hashOf('Naught is there.'), len: 16, kind: 'message', text: 'Naught is there.' }] };
  const f = await kfixture(bundles, strings);
  // The first run: X and Y take 술 for plo, X 무엇 for quux; every line of Z fails.
  const first = async (body) => {
    const user = userOf(body);
    if (user.npc.name.endsWith('MergeZ')) return answer(user.lines.map((l) => ({ n: l.n, ko: '' })), [{ k: 'zed', ko: '제드' }]);
    const { lines, keywords } = fakeAnswer(user, (key) => ({ plo: '술', quux: '무엇' })[key.k]);
    return answer(lines, keywords);
  };
  const one = await run([f.bundles, '--first', '3', '--out', f.out, '--send'], first);
  assert.equal(one.code, 3);
  const pack1 = await packOf(f.out);
  const alias = (item, keyword, word) => ({ archive: 'a', item, keyword, words: [word] });
  assert.deepEqual(pack1.aliases, [alias(X, 'plo', '술'), alias(X, 'quux', '무엇'), alias(Y, 'plo', '술')], 'none for Z, whose lines are not in the pack');
  const cov1Text = await readFile(join(f.out, 'coverage.json'), 'utf8');
  const cov1 = JSON.parse(cov1Text);
  assert.deepEqual(cov1.keywords.byReason, { 'no-base': 1 });
  const again = 'Ask of my @plod again.';
  assert.ok(pack1.converse.find((r) => r.hash === hashOf(again)).text.includes('@{술}'));
  // The second run, X alone: plo takes 탁주, quux a word the gate refuses, and the second plo line fails.
  const bodies = [];
  const second = async (body) => {
    bodies.push(body);
    const user = userOf(body);
    const { lines, keywords } = fakeAnswer(user, (key) => ({ plo: '탁주', quux: 'abc' })[key.k]);
    lines.forEach((l, i) => { if (user.lines[i].en === again) l.ko = ''; });
    return answer(lines, keywords);
  };
  const two = await run([f.bundles, '--item', `a:${X}`, '--out', f.out, '--send'], second);
  assert.equal(two.code, 3);
  assert.deepEqual(userOf(bodies[0]).topics.flatMap((x) => x.keys).map((x) => [x.k, x.known]), [['plo', '술'], ['quux', '무엇']],
    'known from the pack there (plo once, in the first of its blocks)');
  const pack2 = await packOf(f.out);
  assert.deepEqual(pack2.aliases, [alias(X, 'plo', '탁주'), alias(X, 'quux', '무엇'), alias(Y, 'plo', '술')], 'plo replaced; quux and Y stay');
  assert.ok(!pack2.converse.some((r) => r.item === X && r.hash === hashOf(again)), 'the older line with the old word dropped');
  assert.ok(pack2.converse.find((r) => r.item === X && r.hash === hashOf('Try my @plod.')).text.includes('@{탁주}'));
  assert.deepEqual(pack2.converse.filter((r) => r.item === Y), pack1.converse.filter((r) => r.item === Y), 'Y\'s lines as they were');
  const cov2Text = await readFile(join(f.out, 'coverage.json'), 'utf8');
  const cov2 = JSON.parse(cov2Text);
  assert.equal(cov2.keywords.dropped, 1);
  assert.deepEqual([cov2.keywords.kept, cov2.keywords.leftEnglish], [{ count: 1, byReason: { characters: 1 }, keys: [{ script: bundles[0].hash, hash: hashOf('quux'), reason: 'characters' }] }, 0],
    'quux\'s new word refused: it keeps 무엇, so it is kept, not left English');
  assert.deepEqual(cov2.left.filter((x) => x.hash === hashOf(again)).map((x) => x.reason).sort(), ['empty', 'keywords'], 'this run\'s failure, and the old line dropped');
  assert.match(two.text, /Keywords: 2 found, 1 given a Korean word, 1 kept the pack's word \(characters 1\), 0 left English; 1 lines the pack had left English again/);
  // A strings-only run: the conversation part and the aliases as they were.
  const third = async (body) => answer(userOf(body).strings.map((l) => ({ n: l.n, ko: '아무것도 없소.' })));
  const three = await run(['--strings', f.strings, '--only', 'strings', '--out', f.out, '--send'], third);
  assert.equal(three.code, 0);
  const pack3 = await packOf(f.out);
  assert.deepEqual([pack3.aliases, pack3.converse, pack3.base], [pack2.aliases, pack2.converse, pack2.base]);
  assert.equal(pack3.strings.length, 1);
  quiet(one.text + two.text + three.text, bundles);
  noneOf([one.text, two.text, three.text, cov1Text, cov2Text, await readFile(join(f.out, 'coverage.json'), 'utf8')].join('\n'),
    ['plo', 'plod', 'quux', 'zed', '술', '무엇', '탁주', 'abc', '제드']);
  await rm(f.dir, { recursive: true });
});

test('slice K5b: a new word is checked against the words the pack keeps for the script\'s other keys: one colliding with a kept word of another block is refused (shared), its line with that word is not taken, the kept key keeps its word, and the merged pack holds no two colliding words; a key that keeps its old word is counted kept, not left English, and alone does not make the run exit 3', async () => {
  const V = 7901;
  const b = kbundle('a', V, '0000000000127901', `${SENTINEL}KeptV`, [[['zarp'], 'Ask of the @zarp.'], [['vexl'], 'Ask of the @vexl.']]);
  const [s] = planOf([b], { item: { archive: 'a', item: V } }).scripts;
  const kept = new Map([['zarp', ['가']]]);
  const unit = (given) => {
    const r = aliasesOf(s.keys, given, s.compatible, kept);
    return [Object.fromEntries(r.words), Object.fromEntries(r.failed)];
  };
  assert.deepEqual(unit([{ k: 'vexl', ko: '가' }]), [{}, { zarp: 'missing', vexl: 'shared' }]);
  assert.deepEqual(unit([{ k: 'vexl', ko: '가이' }]), [{}, { zarp: 'missing', vexl: 'shared' }], 'the kept word with a particle');
  assert.deepEqual(unit([{ k: 'zarp', ko: '나' }, { k: 'vexl', ko: '가' }]), [{ zarp: '나', vexl: '가' }, {}], 'a kept word this answer replaces does not count');
  assert.deepEqual(unit([{ k: 'zarp', ko: '나' }, { k: 'zarp', ko: '다' }, { k: 'vexl', ko: '가' }]), [{}, { zarp: 'repeated', vexl: 'shared' }],
    'a key refused here keeps its old word, which the others are checked against');
  const f = await kfixture([b]);
  // The first run: zarp takes 가; vexl's word is refused (empty), its line English.
  const first = async (body) => {
    const { lines, keywords } = fakeAnswer(userOf(body), (key) => ({ zarp: '가' })[key.k]);
    return answer(lines, keywords);
  };
  const one = await run([f.bundles, '--item', `a:${V}`, '--out', f.out, '--send'], first);
  assert.equal(one.code, 3);
  assert.deepEqual((await packOf(f.out)).aliases, [{ archive: 'a', item: V, keyword: 'zarp', words: ['가'] }]);
  // The second run: the answer gives only vexl, 가, and writes 가 in both lines.
  const bodies = [];
  const second = async (body) => {
    bodies.push(body);
    const user = userOf(body);
    return answer(user.lines.map((l) => ({ n: l.n, ko: fakeKo(l.en, new Map([['zarp', '가'], ['vexl', '가']])) })), [{ k: 'vexl', ko: '가' }]);
  };
  const two = await run([f.bundles, '--item', `a:${V}`, '--out', f.out, '--send'], second);
  assert.equal(two.code, 3);
  assert.deepEqual(userOf(bodies[0]).topics, [{ keys: [{ k: 'zarp', shown: ['zarp'], known: '가' }] }, { keys: [{ k: 'vexl', shown: ['vexl'] }] }]);
  assert.deepEqual(userOf(bodies[1]).topics, [{ keys: [{ k: 'zarp', shown: ['zarp'], known: '가' }] }], 'asked again: the key with a word');
  const pack = await packOf(f.out);
  assert.deepEqual(pack.aliases, [{ archive: 'a', item: V, keyword: 'zarp', words: ['가'] }], 'vexl refused; zarp keeps its word');
  const vexl = pack.converse.find((r) => r.hash === hashOf('Ask of the @vexl.'));
  assert.ok(vexl.text.includes('@{vexl}') && !vexl.text.includes('@{가}'), 'the line with 가 for vexl not taken: the English one stays');
  assert.ok(pack.converse.find((r) => r.hash === hashOf('Ask of the @zarp.')).text.includes('@{가}'));
  for (const a of pack.aliases) {
    for (const o of pack.aliases) {
      if (a !== o && a.archive === o.archive && a.item === o.item) assert.ok(!a.words.some((w) => o.words.some((v) => collide(w, v))), 'no two colliding words');
    }
  }
  const coverage = await readFile(join(f.out, 'coverage.json'), 'utf8');
  const kw = JSON.parse(coverage).keywords;
  assert.deepEqual(kw, {
    found: 2, translated: 0, kept: { count: 1, byReason: { missing: 1 }, keys: [{ script: b.hash, hash: hashOf('zarp'), reason: 'missing' }] },
    leftEnglish: 1, byReason: { shared: 1 }, unexplained: 0, dropped: 0,
  }, 'zarp keeps 가: kept, not left English; vexl ends with no word: left');
  assert.deepEqual(JSON.parse(coverage).left.filter((x) => x.category === 'keywords').map((x) => [x.hash, x.reason]), [[hashOf('vexl'), 'shared']]);
  assert.deepEqual(JSON.parse(coverage).units.byReason, { keywords: 1 });
  assert.match(two.text, /Keywords: 2 found, 0 given a Korean word, 1 kept the pack's word \(missing 1\), 1 left English \(shared 1\)\./);
  // A third run: vexl takes 나, zarp is left out again but keeps 가; every line lands, so the run
  // is done (exit 0): a key that keeps its word is no key left English.
  const third = async (body) => {
    const user = userOf(body);
    return answer(user.lines.map((l) => ({ n: l.n, ko: fakeKo(l.en, new Map([['zarp', '가'], ['vexl', '나']])) })), [{ k: 'vexl', ko: '나' }]);
  };
  const three = await run([f.bundles, '--item', `a:${V}`, '--out', f.out, '--send'], third);
  assert.equal(three.code, 0, 'done: nothing left English');
  const coverage3 = await readFile(join(f.out, 'coverage.json'), 'utf8');
  assert.deepEqual(JSON.parse(coverage3).keywords, {
    found: 2, translated: 1, kept: { count: 1, byReason: { missing: 1 }, keys: [{ script: b.hash, hash: hashOf('zarp'), reason: 'missing' }] },
    leftEnglish: 0, byReason: {}, unexplained: 0, dropped: 0,
  });
  assert.deepEqual((await packOf(f.out)).aliases, [
    { archive: 'a', item: V, keyword: 'vexl', words: ['나'] }, { archive: 'a', item: V, keyword: 'zarp', words: ['가'] },
  ]);
  assert.match(three.text, /Keywords: 2 found, 1 given a Korean word, 1 kept the pack's word \(missing 1\), 0 left English\./);
  quiet(one.text + two.text + three.text, [b]);
  noneOf([one.text, two.text, three.text, coverage, coverage3].join('\n'), ['zarp', 'vexl', '가', '가이', '나']);
  await rm(f.dir, { recursive: true });
});

test('slice K5c: a key answered twice with the same word takes it and is counted translated, not repeated; the request lists a key two blocks hold once, in its first block, and a line asked again too', async () => {
  const T = 7961;
  const b = kbundle('a', T, '0000000000147961', `${SENTINEL}Twice`, [
    [['zorb', 'vexl'], 'Ask of the @zorb.'],
    [['plim', 'zorb'], 'The @plim, or the @zorb.*'],
  ]);
  const words = new Map([['zorb', '달'], ['plim', '별']]);
  const given = [{ k: 'zorb', ko: '달' }, { k: 'plim', ko: '별' }, { k: 'zorb', ko: '달' }, { k: 'vexl', ko: 'abc' }];
  const bodies = [];
  const send = async (body) => {
    bodies.push(body);
    const user = userOf(body);
    const lines = user.lines.map((l) => ({ n: l.n, ko: fakeKo(l.en, words) }));
    // The first answer drops the line's escape, so that line is asked again.
    if (bodies.length === 1) for (const l of lines) l.ko = l.ko.replace('*', '');
    return answer(lines, bodies.length === 1 ? given : []);
  };
  const f = await kfixture([b]);
  const r = await run([f.bundles, '--item', `a:${T}`, '--out', f.out, '--send'], send);
  assert.equal(r.code, 3, 'written; vexl left English');
  assert.equal(bodies.length, 2, 'the script, then the failed line again');
  assert.deepEqual(userOf(bodies[0]).topics, [
    { keys: [{ k: 'zorb', shown: ['zorb'] }, { k: 'vexl', shown: [] }] },
    { keys: [{ k: 'plim', shown: ['plim'] }] },
  ], 'zorb once, in its first block');
  assert.deepEqual(userOf(bodies[1]).topics, [
    { keys: [{ k: 'zorb', shown: ['zorb'], known: '달' }] },
    { keys: [{ k: 'plim', shown: ['plim'], known: '별' }] },
  ], 'asked again: the keys with a word, zorb alone in its first block');
  const pack = await packOf(f.out);
  assert.deepEqual(pack.aliases, [
    { archive: 'a', item: T, keyword: 'plim', words: ['별'] }, { archive: 'a', item: T, keyword: 'zorb', words: ['달'] },
  ]);
  assert.equal(pack.converse.length, unitsOf(b).length, 'every line landed');
  for (const en of ['Ask of the @zorb.', 'The @plim, or the @zorb.*']) {
    assert.ok(pack.converse.find((x) => x.hash === hashOf(en)).text.includes('@{달}'), 'each line of zorb highlights its word');
  }
  const coverage = await readFile(join(f.out, 'coverage.json'), 'utf8');
  assert.deepEqual(JSON.parse(coverage).keywords, {
    found: 3, translated: 2, kept: NONE_KEPT, leftEnglish: 1, byReason: { characters: 1 }, unexplained: 0, dropped: 0,
  }, 'zorb translated, not repeated');
  assert.deepEqual(JSON.parse(coverage).left.filter((x) => x.category === 'keywords').map((x) => [x.hash, x.reason]), [[hashOf('vexl'), 'characters']]);
  assert.match(r.text, /Keywords: 3 found, 2 given a Korean word, 1 left English \(characters 1\)\./);
  quiet(r.text, [b]);
  noneOf(r.text + coverage, ['zorb', 'plim', 'vexl', 'abc', '달', '별']);
  await rm(f.dir, { recursive: true });
});

test('slice K5c: the alias gate takes entries giving a key one word as one entry; two different words stay repeated; the collision rules run after, so a word given twice that collides with another key\'s, or with one the pack keeps, is refused shared', () => {
  const compatible = compatibleOf([['zorb'], ['plim']]);
  const gate2 = (given, kept = new Map()) => {
    const r = aliasesOf(['zorb', 'plim'], given, compatible, kept);
    return [Object.fromEntries(r.words), Object.fromEntries(r.failed)];
  };
  const z = (ko) => ({ k: 'zorb', ko });
  assert.deepEqual(gate2([z('달'), { k: 'plim', ko: '별' }, z('달')]), [{ zorb: '달', plim: '별' }, {}], 'the same word twice');
  assert.deepEqual(gate2([z('달'), z('별'), { k: 'plim', ko: '빛' }]), [{ plim: '빛' }, { zorb: 'repeated' }], 'two words');
  assert.deepEqual(gate2([z('달'), z('달'), z('별'), { k: 'plim', ko: '빛' }]), [{ plim: '빛' }, { zorb: 'repeated' }], 'all the same, not most');
  assert.deepEqual(gate2([z('ab'), z('ab'), { k: 'plim', ko: '빛' }]), [{ plim: '빛' }, { zorb: 'characters' }], 'the word gate after');
  assert.deepEqual(gate2([z('달'), z('달'), { k: 'plim', ko: '달이' }]), [{}, { zorb: 'shared', plim: 'shared' }], 'colliding with another key\'s word');
  assert.deepEqual(gate2([z('달'), z('달')], new Map([['plim', ['달']]])), [{}, { zorb: 'shared', plim: 'missing' }], 'colliding with a kept word');
});

test('slice K5b: a key the pack gives several words (a pack written by hand) takes any of them: its line showing the second word is kept when another key\'s word changes; the request\'s known is its first word', async () => {
  const W = 7951;
  const b = kbundle('a', W, '0000000000137951', `${SENTINEL}ManyW`, [[['ambo'], 'The @ambo here.'], [['ambo'], 'Ask of @ambo again.'], [['brix'], 'And @brix.']]);
  const at = (en) => unitsOf(b).find((u) => u.text === en);
  const line = (en, text) => ({ archive: 'a', item: W, offset: at(en).offset, len: latin1(en).length, hash: hashOf(en), text });
  const f = await kfixture([b]);
  await mkdir(f.out, { recursive: true, mode: 0o700 });
  await writeFile(join(f.out, 'ko.u6lp'), packBytes({
    base: fingerprint([{ archive: 'a', item: W, hash: b.hash }]),
    runs: [line('The @ambo here.', '@{가} 여기 있소.'), line('Ask of @ambo again.', '@{나}에게 다시 묻소.'), line('And @brix.', '그리고 @{다}.')],
    aliases: [{ archive: 'a', item: W, keyword: 'ambo', words: ['가', '나'] }, { archive: 'a', item: W, keyword: 'brix', words: ['다'] }],
  }), { mode: 0o600 });
  // The answer: brix takes 라, ambo none (it keeps both its words); the second ambo line fails.
  const bodies = [];
  const send = async (body) => {
    bodies.push(body);
    const user = userOf(body);
    const lines = user.lines.map((l) => ({ n: l.n, ko: l.en === 'Ask of @ambo again.' ? '' : fakeKo(l.en, new Map([['ambo', '가'], ['brix', '라']])) }));
    return answer(lines, [{ k: 'brix', ko: '라' }]);
  };
  const r = await run([f.bundles, '--item', `a:${W}`, '--out', f.out, '--send'], send);
  assert.equal(r.code, 3);
  assert.deepEqual(userOf(bodies[0]).topics.flatMap((x) => x.keys).map((x) => [x.k, x.known]), [['ambo', '가'], ['brix', '다']], 'known: the first word; ambo once');
  const pack = await packOf(f.out);
  assert.deepEqual(pack.aliases, [{ archive: 'a', item: W, keyword: 'ambo', words: ['가', '나'] }, { archive: 'a', item: W, keyword: 'brix', words: ['라'] }]);
  assert.equal(pack.converse.find((x) => x.hash === hashOf('Ask of @ambo again.')).text, '@{나}에게 다시 묻소.', 'the line with ambo\'s second word kept');
  assert.ok(pack.converse.find((x) => x.hash === hashOf('And @brix.')).text.includes('@{라}'));
  const coverage = await readFile(join(f.out, 'coverage.json'), 'utf8');
  assert.equal(JSON.parse(coverage).keywords.dropped, 0, 'nothing dropped');
  quiet(r.text, [b]);
  noneOf(r.text + coverage, ['ambo', 'brix', '가', '나', '다', '라']);
  await rm(f.dir, { recursive: true });
});

test('slice K5b: with the shared keywords\' request first (three scripts share a key), --dry-run, no terminal without --send, and on a terminal a typed no, Ctrl-C and SIGINT send nothing: the counting transport is never called, before the answer either; the preview is that request', async () => {
  const bundles = sharing();
  const wants = { 'dry-run': 0, 'no terminal': 1, no: 1, 'ctrl-c': 130, sigint: 130 };
  for (const how of Object.keys(wants)) {
    const f = await kfixture(bundles);
    const calls = [];
    const send = async (body) => {
      calls.push(body);
      return reply({ keywords: [] });
    };
    const out = output();
    const signals = new EventEmitter();
    const args = [f.bundles, '--first', '3', '--out', f.out, '--jobs', '2'];
    let code;
    if (how === 'dry-run' || how === 'no terminal') {
      code = await main(how === 'dry-run' ? [...args, '--dry-run'] : args, { transport: send, stdout: out, isTTY: false, signals });
    } else {
      const stdin = terminal();
      const running = main(args, { transport: send, stdout: out, isTTY: true, stdin, signals });
      await prompted(out);
      assert.equal(calls.length, 0, `${how}: nothing sent before the answer`);
      if (how === 'sigint') signals.emit('SIGINT');
      else stdin.write(how === 'no' ? 'no\r' : '\x03');
      code = await running;
    }
    assert.equal(code, wants[how], how);
    assert.equal(calls.length, 0, `${how}: the transport never called`);
    assert.deepEqual(userOf(await readFile(previewPrinted(out.text), 'utf8')), { keywords: [{ k: 'plo', shown: ['plod'] }] }, `${how}: the preview`);
    assert.ok(!existsSync(join(f.out, 'ko.u6lp')) && !existsSync(join(f.out, 'coverage.json')), `${how}: nothing written`);
    quiet(out.text, bundles);
    noneOf(out.text, SHARE_KEYS);
    await rm(f.dir, { recursive: true });
  }
});

// --- the keywords' words per script (made-up words only) -------------------------------------

// A pack in `out` over every line of `bundles` (each in made-up Korean, the word `aliases` gives
// a highlighted word's key in its braces: the fixtures highlight each key as itself), with those
// aliases ([archive, item, keyword, word]), over the fingerprint of those scripts.
async function packFor(out, bundles, aliases) {
  const runs = [];
  for (const b of bundles) {
    const words = new Map(aliases.filter(([a, i]) => a === b.archive && i === b.item).map(([, , k, w]) => [k, w]));
    for (const u of unitsOf(b)) runs.push({ archive: b.archive, item: b.item, offset: u.offset, len: latin1(u.text).length, hash: hashOf(u.text), text: fakeKo(u.text, words) });
  }
  await mkdir(out, { recursive: true, mode: 0o700 });
  await writeFile(join(out, 'ko.u6lp'), packBytes({
    base: fingerprint(bundles.map((b) => ({ archive: b.archive, item: b.item, hash: b.hash }))),
    runs,
    aliases: aliases.map(([archive, item, keyword, word]) => ({ archive, item, keyword, words: [word] })),
  }), { mode: 0o600 });
}

// The `topics` of each script request of `bodies`, by its NPC's name (the sentinel taken off);
// the last one when a script is asked twice.
const topicsBy = (bodies) => new Map(bodies.map(userOf).filter((u) => u.npc).map((u) => [u.npc.name.replace(SENTINEL, ''), u.topics]));

// A transport that answers the shared keywords' request with `glossary` (its keywords) and each
// script's as the fake translator does (its known words, else made-up ones); the script `fails`
// names (the end of its NPC's name) never gets an answer that reads.
function wordsTransport(glossary = [], fails = null) {
  const bodies = [];
  const send = async (body) => {
    bodies.push(body);
    const user = userOf(body);
    if (user.keywords) return reply({ keywords: glossary });
    if (fails && user.npc.name.endsWith(fails)) return { status: 200, json: { type: 'message', content: [{ type: 'text', text: 'I would rather not.' }] } };
    const { lines, keywords } = fakeAnswer(user);
    return answer(lines, keywords);
  };
  return { send, bodies };
}

test('keywords per script: each script\'s request names as known its own word in the pack there, never another script\'s: two scripts that give one key different words each get their own back (and keep it), and a key only the other script has a word for is asked with none', async () => {
  const [X, Y] = [8101, 8102];
  const bundles = [
    kbundle('a', X, '0000000000158101', `${SENTINEL}OwnX`, [[['zorb'], 'Ask of the @zorb.'], [['vexl'], 'The @vexl, then.']]),
    kbundle('a', Y, '0000000000158102', `${SENTINEL}OwnY`, [[['zorb'], 'Thy @zorb, friend.'], [['vexl'], 'No @vexl here.']]),
  ];
  const f = await kfixture(bundles);
  await packFor(f.out, bundles, [['a', X, 'zorb', '달'], ['a', Y, 'zorb', '별'], ['a', Y, 'vexl', '빛']]);
  const t = wordsTransport();
  const r = await run([f.bundles, '--first', '2', '--out', f.out, '--send'], t.send);
  assert.equal(r.code, 0);
  assert.equal(t.bodies.length, 2, 'one request per script, no shared keywords');
  const topics = topicsBy(t.bodies);
  assert.deepEqual(topics.get('OwnX'), [{ keys: [{ k: 'zorb', shown: ['zorb'], known: '달' }] }, { keys: [{ k: 'vexl', shown: ['vexl'] }] }],
    'X: its own word for zorb; none for vexl (the other script\'s is not X\'s)');
  assert.deepEqual(topics.get('OwnY'), [{ keys: [{ k: 'zorb', shown: ['zorb'], known: '별' }] }, { keys: [{ k: 'vexl', shown: ['vexl'], known: '빛' }] }],
    'Y: its own words, not the first in order of the two');
  const alias = (item, keyword, word) => ({ archive: 'a', item, keyword, words: [word] });
  assert.deepEqual((await packOf(f.out)).aliases, [alias(X, 'vexl', fakeWord('vexl')), alias(X, 'zorb', '달'), alias(Y, 'vexl', '빛'), alias(Y, 'zorb', '별')],
    'each script\'s word stays its own');
  const coverage = await readFile(join(f.out, 'coverage.json'), 'utf8');
  assert.ok(!('freshKeywords' in JSON.parse(coverage)), 'no flag, no mark');
  assert.doesNotMatch(r.text, /fresh-keywords/);
  quiet(r.text, bundles);
  noneOf(r.text + coverage, ['zorb', 'vexl', '달', '별', '빛', fakeWord('vexl')]);
  await rm(f.dir, { recursive: true });
});

test('keywords per script: a key at least 3 scripts hold is asked before them while one of them has no word for it in the pack there; its answer goes only to the scripts with none, the one with its own word keeping it; once every one has its own it is not asked', async () => {
  const items = [8201, 8202, 8203];
  const bundles = items.map((item, i) => kbundle('a', item, `000000000016${item}`, `${SENTINEL}Fill${'pqr'[i]}`, [
    [['plo'], 'Try my @plo.'], [[`my${'abc'[i]}`], `Mine is @my${'abc'[i]}.`],
  ]));
  const planned = planOf(bundles, { first: 3 }).scripts;
  [planned[0].own, planned[1].own, planned[2].own] = [new Map([['plo', '술']]), new Map([['plo', '술'], ['myb', '내것']]), new Map()];
  assert.deepEqual([...settledOf(planned)], ['myb'], 'plo: one holder has no word of its own; myb: its one holder has');
  const f = await kfixture(bundles);
  await packFor(f.out, [bundles[0]], [['a', items[0], 'plo', '술']]);
  const t = wordsTransport([{ k: 'plo', ko: '술잔' }]);
  const r = await run([f.bundles, '--first', '3', '--out', f.out, '--send', '--jobs', '2'], t.send);
  assert.equal(r.code, 0);
  assert.deepEqual(userOf(t.bodies[0]), { keywords: [{ k: 'plo', shown: ['plo'] }] }, 'asked first: two of the three have no word for it');
  const topics = topicsBy(t.bodies);
  assert.deepEqual(topics.get('Fillp')[0], { keys: [{ k: 'plo', shown: ['plo'], known: '술' }] }, 'its own word, not the shared answer');
  for (const who of ['Fillq', 'Fillr']) assert.deepEqual(topics.get(who)[0], { keys: [{ k: 'plo', shown: ['plo'], known: '술잔' }] }, `${who}: the shared answer`);
  for (const who of ['Fillp', 'Fillq', 'Fillr']) assert.ok(topics.get(who)[1].keys.every((x) => x.known === undefined), `${who}: its own key settled by none`);
  assert.deepEqual((await packOf(f.out)).aliases.filter((a) => a.keyword === 'plo').map((a) => [a.item, a.words]), [[items[0], ['술']], [items[1], ['술잔']], [items[2], ['술잔']]]);
  // Again: every one has its own word now, so nothing is asked first; each gets its own.
  const t2 = wordsTransport([{ k: 'plo', ko: '탁주' }]);
  const again = await run([f.bundles, '--first', '3', '--out', f.out, '--send', '--jobs', '2'], t2.send);
  assert.equal(again.code, 0);
  assert.equal(t2.bodies.length, 3, 'no shared keywords asked');
  const topics2 = topicsBy(t2.bodies);
  assert.deepEqual(['Fillp', 'Fillq', 'Fillr'].map((w) => topics2.get(w)[0].keys[0].known), ['술', '술잔', '술잔']);
  quiet(r.text + again.text, bundles);
  noneOf(r.text + again.text + await readFile(join(f.out, 'coverage.json'), 'utf8'), ['plo', 'mya', 'myb', 'myc', '술', '술잔', '탁주']);
  await rm(f.dir, { recursive: true });
});

test('keywords per script: --fresh-keywords sends no word of the pack there as known in a script\'s first request (only the shared keywords\' answer\'s, every key 3 scripts hold asked though each has a word); the words each answered script\'s answer keeps replace the pack\'s for its keys, a script whose request fails keeps its aliases and lines, a script not chosen its own; the consent text and the coverage say so; the usage names it', async () => {
  const items = [8301, 8302, 8303, 8304];
  const bundles = items.map((item, i) => kbundle('a', item, `000000000017${item}`, `${SENTINEL}Fresh${'pqrs'[i]}`, [
    [['plo'], 'Try my @plo.'], [[`my${'abcd'[i]}`], `Mine is @my${'abcd'[i]}.`],
  ]));
  const [P, Q, R, S] = items;
  const f = await kfixture(bundles);
  await packFor(f.out, bundles, [['a', P, 'plo', '술'], ['a', P, 'mya', '내것'], ['a', Q, 'plo', '탁주'], ['a', R, 'plo', '술'], ['a', R, 'myc', '그것'], ['a', S, 'plo', '약주']]);
  const before = await packOf(f.out);
  const t = wordsTransport([{ k: 'plo', ko: '막걸리' }], 'Freshr');
  const r = await run([f.bundles, '--first', '3', '--out', f.out, '--send', '--fresh-keywords'], t.send);
  assert.equal(r.code, 3, 'R\'s lines left English this run');
  assert.deepEqual(userOf(t.bodies[0]), { keywords: [{ k: 'plo', shown: ['plo'] }] }, 'asked though every script has a word for it');
  const scripts = t.bodies.slice(1).map(userOf);
  assert.deepEqual(scripts.map((u) => u.npc.name.replace(SENTINEL, '')).sort(), ['Freshp', 'Freshq', 'Freshr', 'Freshr'], 'each chosen script (the failed one asked again)');
  for (const u of scripts) {
    const keys = u.topics.flatMap((x) => x.keys);
    assert.deepEqual(keys.map((x) => [x.k, x.known]), [['plo', '막걸리'], [`my${'abc'[['Freshp', 'Freshq', 'Freshr'].indexOf(u.npc.name.replace(SENTINEL, ''))]}`, undefined]],
      'the shared answer only: no word of the pack (술, 탁주, 내것, 그것); every request here a first one (none asked again)');
  }
  const alias = (item, keyword, word) => ({ archive: 'a', item, keyword, words: [word] });
  const pack = await packOf(f.out);
  assert.deepEqual(pack.aliases, [
    alias(P, 'mya', fakeWord('mya')), alias(P, 'plo', '막걸리'), alias(Q, 'myb', fakeWord('myb')), alias(Q, 'plo', '막걸리'),
    alias(R, 'myc', '그것'), alias(R, 'plo', '술'), alias(S, 'plo', '약주'),
  ], 'P and Q answered: their words replace the pack\'s; R failed: its own stay; S not chosen: its own stay');
  for (const item of [R, S]) assert.deepEqual(pack.converse.filter((x) => x.item === item), before.converse.filter((x) => x.item === item), 'its lines as they were');
  assert.ok(pack.converse.find((x) => x.item === P && x.hash === hashOf('Try my @plo.')).text.includes('@{막걸리}'));
  const coverageText = await readFile(join(f.out, 'coverage.json'), 'utf8');
  const coverage = JSON.parse(coverageText);
  assert.equal(coverage.freshKeywords, true, 'the coverage says the flag was on');
  assert.deepEqual(coverage.glossary, { found: 1, translated: 1, leftEnglish: 0, byReason: {}, unexplained: 0 });
  assert.deepEqual(coverage.keywords.kept, { count: 2, byReason: { invalid: 2 }, keys: ['plo', 'myc'].map((k) => ({ script: bundles[2].hash, hash: hashOf(k), reason: 'invalid' })) },
    'R\'s keys keep their old words');
  const consent = r.text.slice(0, r.text.indexOf('The exact first request'));
  const said = /\(--fresh-keywords\): no word of the pack there is sent as known in a script's first request \(a line asked again names the word its key keeps, the pack's where this run kept none\);.* the words a script's answer keeps \(past the alias gate\) replace the pack's for its keys/;
  assert.match(consent, said, 'said before the owner\'s go');
  // Before any request: --dry-run says it too; the usage names it; with no bundles it is refused.
  const g = await kfixture(bundles);
  const dry = await run([g.bundles, '--first', '3', '--out', g.out, '--dry-run', '--fresh-keywords'], null);
  assert.equal(dry.code, 0);
  assert.match(dry.text, said);
  const help = await run(['--help'], null);
  assert.equal(help.code, 0);
  assert.match(help.text, /\[--fresh-keywords\]/);
  const alone = await run(['--strings', join(g.dir, 'none.json'), '--fresh-keywords', '--out', g.out], null);
  assert.equal(alone.code, 2);
  assert.match(alone.text, /--fresh-keywords goes with a bundles file/);
  quiet(r.text + dry.text, bundles);
  noneOf(r.text + dry.text + coverageText, ['plo', 'mya', 'myb', 'myc', 'myd', '술', '탁주', '내것', '그것', '약주', '막걸리', fakeWord('mya'), fakeWord('myb')]);
  await rm(f.dir, { recursive: true });
  await rm(g.dir, { recursive: true });
});

test('keywords per script: a one-letter key is never asked across scripts (nor its highlighted words gathered from them) and never takes another script\'s word; a y or an n its first block lists alone is marked in the request as that block\'s yes or no, a y beside another key and another letter alone are not; a marked key is always sent 예 or 아니 as known, whatever word the pack there has for it; a line asked again keeps the mark and the word; the instruction and the guide say what to answer', async () => {
  const items = [8401, 8402, 8403];
  const [P, Q, R] = items;
  const bundles = [
    kbundle('a', P, `000000000018${P}`, `${SENTINEL}Letterp`, [
      [['plo'], 'Try my @plo.'], [['y'], 'So be it.'], [['n'], 'A @nix to thee.'], [['x'], 'An @xo.'],
    ]),
    kbundle('a', Q, `000000000018${Q}`, `${SENTINEL}Letterq`, [[['plo'], 'Thy @plo.'], [['y'], 'Aye, @yarn.'], [['n'], 'Nay, a @nob.*']]),
    kbundle('a', R, `000000000018${R}`, `${SENTINEL}Letterr`, [[['plo'], 'My @plo.'], [['y', 'yon'], 'Look @yon.'], [['n'], 'Never.']]),
  ];
  const [p, q, r3] = planOf(bundles, { first: 3 }).scripts;
  assert.deepEqual(glossaryOf([p, q, r3]).units, [{ kind: 'keyword', n: 0, k: 'plo', shown: ['plo'] }], 'y and n held by all three: not asked, their words not gathered');
  const only = planOf(bundles.map((b) => ({ ...b, turns: [{ ...b.turns[0], topics: b.turns[0].topics.filter((t) => t.keywords[0].length === 1) }] })), { first: 3 }).scripts;
  assert.equal(glossaryOf(only), null, 'only one-letter keys shared: no shared keywords\' request');
  const f = await kfixture(bundles);
  // P's n has 아니 in the pack; Q's n another word (the defect's kind), its y 예.
  await packFor(f.out, [bundles[0], bundles[1]], [['a', P, 'n', '아니'], ['a', Q, 'n', '나무'], ['a', Q, 'y', '예']]);
  const bodies = [];
  const send = async (body) => {
    bodies.push(body);
    const user = userOf(body);
    if (user.keywords) return reply({ keywords: [{ k: 'plo', ko: '술' }] });
    // The fake translator answers each key its known word (a marked key is always sent 예 or 아니),
    // else a made-up one.
    const { lines, keywords } = fakeAnswer(user);
    // Q's first answer drops its line's escape, so that line is asked again.
    if (user.npc.name.endsWith('Letterq') && user.lines.length > 1) for (const l of lines) l.ko = l.ko.replace('*', '');
    return answer(lines, keywords);
  };
  const r = await run([f.bundles, '--first', '3', '--out', f.out, '--send', '--jobs', '1'], send);
  assert.equal(r.code, 0);
  assert.deepEqual(userOf(bodies[0]), { keywords: [{ k: 'plo', shown: ['plo'] }] }, 'the shared keywords: plo alone');
  const first = new Map(bodies.slice(1, 4).map(userOf).map((u) => [u.npc.name.replace(SENTINEL, ''), u.topics]));
  assert.deepEqual(first.get('Letterp'), [
    { keys: [{ k: 'plo', shown: ['plo'], known: '술' }] },
    { keys: [{ k: 'y', shown: [], known: '예', answer: 'yes' }] },
    { keys: [{ k: 'n', shown: ['nix'], known: '아니', answer: 'no' }] },
    { keys: [{ k: 'x', shown: ['xo'] }] },
  ], 'P: y and n marked, each with its word (y with none in the pack too), x not');
  assert.deepEqual(first.get('Letterq'), [
    { keys: [{ k: 'plo', shown: ['plo'], known: '술' }] },
    { keys: [{ k: 'y', shown: ['yarn'], known: '예', answer: 'yes' }] },
    { keys: [{ k: 'n', shown: ['nob'], known: '아니', answer: 'no' }] },
  ], 'Q: its own y word 예 kept; its own n word (not 아니) replaced by 아니; no word of P\'s; its own highlighted words only');
  assert.deepEqual(first.get('Letterr'), [
    { keys: [{ k: 'plo', shown: ['plo'], known: '술' }] },
    { keys: [{ k: 'y', shown: ['yon'] }, { k: 'yon', shown: [] }] },
    { keys: [{ k: 'n', shown: [], known: '아니', answer: 'no' }] },
  ], 'R: y beside yon is not marked');
  const retry = userOf(bodies.at(-1));
  assert.equal(retry.lines.length, 1, 'Q\'s failed line asked again');
  assert.deepEqual(retry.topics.find((x) => x.keys.some((k) => k.k === 'n')), { keys: [{ k: 'n', shown: ['nob'], known: '아니', answer: 'no' }] }, 'still marked: the word it keeps is 아니');
  const pack = await packOf(f.out);
  assert.deepEqual(pack.aliases.filter((a) => a.keyword === 'n').map((a) => [a.item, a.words]), [[P, ['아니']], [Q, ['아니']], [R, ['아니']]], 'Q\'s old word replaced');
  assert.deepEqual(pack.aliases.filter((a) => a.keyword === 'y').map((a) => [a.item, a.words]), [[P, ['예']], [Q, ['예']], [R, [fakeWord('y')]]]);
  const keyed = systemOf('STYLE', true);
  assert.ok(keyed.includes('"answer": "yes" | "no"'), 'the instruction names the mark');
  assert.ok(keyed.includes('is the player\'s yes or no') && keyed.includes('예 for y and 아니 for n, whatever else is given') && keyed.includes('not a word of its'));
  assert.ok(keyed.includes('a key of another block that means yes or no (yes, no) takes another word') && keyed.includes('(그렇소,') && keyed.includes('아니오)'),
    'a key yes or no elsewhere takes another word');
  const guide = await readFile(join(dirname(TOOL), 'style-ko.md'), 'utf8');
  assert.ok(guide.includes('is the player\'s yes or no') && guide.includes('예 for `y` and 아니 for `n`') && guide.includes('whatever else is given'), 'the guide says it too');
  assert.ok(guide.includes('A key of another block that means yes or no') && guide.includes('(`yes`, `no`) takes another word (그렇소, 아니오)'));
  quiet(r.text, bundles);
  noneOf(r.text + await readFile(join(f.out, 'coverage.json'), 'utf8'), ['plo', 'nix', 'xo', 'yarn', 'nob', 'yon', '아니', '예', '나무', '술', fakeWord('y')]);
  await rm(f.dir, { recursive: true });
});

test('keywords per script: a key marked yes or no and its known word never disagree: in a first request and in a request again it is named 예 or 아니 as known whatever other word known gives it, or with none; a lone y block that is not y\'s first block leaves y unmarked', () => {
  const [s] = planOf([kbundle('a', 8501, '0000000000198501', `${SENTINEL}Marks`, [[['y'], 'So be it.'], [['n'], 'Begone.']])], { first: 1 }).scripts;
  const listed = (known, only = null) => topicsOf(s, new Map(Object.entries(known)), only).flatMap((t) => t.keys);
  const both = [{ k: 'y', shown: [], known: '예', answer: 'yes' }, { k: 'n', shown: [], known: '아니', answer: 'no' }];
  assert.deepEqual(listed({ y: '예', n: '나무' }), both);
  assert.deepEqual(listed({ y: '너', n: '아니' }), both);
  assert.deepEqual(listed({}), both, 'no word known: the fixed one');
  assert.deepEqual(listed({ y: '너', n: '아니' }, new Set(['y', 'n'])), both, 'asked again: the fixed word (the settle gives the key the same), still marked');
  const [lone] = planOf([kbundle('a', 8502, '0000000000198502', `${SENTINEL}Lone`, [[['yea', 'y'], 'Ask of @yea.'], [['y'], 'Aye.'], [['n'], 'Nay.']])], { first: 1 }).scripts;
  assert.deepEqual(topicsOf(lone, new Map()), [{ keys: [{ k: 'yea', shown: ['yea'] }, { k: 'y', shown: [] }] }, { keys: [{ k: 'n', shown: [], known: '아니', answer: 'no' }] }],
    'y first beside yea: unmarked, though a later block lists it alone');
});

test('keywords per script: under --fresh-keywords a line asked again names the word its key keeps: the new one where the answer kept one, the pack\'s where it kept none (its word refused); the merged pack keeps that key\'s old alias and the line lands with it', async () => {
  const V = 8601;
  const b = kbundle('a', V, '0000000000208601', `${SENTINEL}FreshV`, [[['zarp'], 'Ask of the @zarp.'], [['vexl'], 'Ask of the @vexl.*']]);
  const f = await kfixture([b]);
  await packFor(f.out, [b], [['a', V, 'zarp', '가'], ['a', V, 'vexl', '나']]);
  const bodies = [];
  const send = async (body) => {
    bodies.push(body);
    const lines = userOf(body).lines.map((l) => ({ n: l.n, ko: fakeKo(l.en, new Map([['zarp', '다'], ['vexl', '나']])) }));
    if (bodies.length > 1) return answer(lines, []);
    // The first answer: zarp a new word, vexl one the alias gate refuses, the vexl line's escape dropped.
    for (const l of lines) l.ko = l.ko.replace('*', '');
    return answer(lines, [{ k: 'zarp', ko: '다' }, { k: 'vexl', ko: 'abc' }]);
  };
  const r = await run([f.bundles, '--item', `a:${V}`, '--out', f.out, '--send', '--fresh-keywords'], send);
  assert.equal(r.code, 0, 'every line lands; vexl keeps its word');
  assert.equal(bodies.length, 2, 'the script, then its failed line again');
  assert.deepEqual(userOf(bodies[0]).topics, [{ keys: [{ k: 'zarp', shown: ['zarp'] }] }, { keys: [{ k: 'vexl', shown: ['vexl'] }] }], 'the first request: no word of the pack');
  const retry = userOf(bodies[1]);
  assert.equal(retry.lines.length, 1);
  assert.deepEqual(retry.topics, [{ keys: [{ k: 'zarp', shown: ['zarp'], known: '다' }] }, { keys: [{ k: 'vexl', shown: ['vexl'], known: '나' }] }],
    'asked again: zarp\'s new word, vexl\'s old one (the pack\'s: this run kept none)');
  const pack = await packOf(f.out);
  assert.deepEqual(pack.aliases, [{ archive: 'a', item: V, keyword: 'vexl', words: ['나'] }, { archive: 'a', item: V, keyword: 'zarp', words: ['다'] }], 'vexl\'s old alias kept');
  assert.ok(pack.converse.find((x) => x.hash === hashOf('Ask of the @vexl.*')).text.includes('@{나}'), 'the line asked again lands with it');
  const coverageText = await readFile(join(f.out, 'coverage.json'), 'utf8');
  const coverage = JSON.parse(coverageText);
  assert.equal(coverage.freshKeywords, true);
  assert.deepEqual(coverage.keywords, {
    found: 2, translated: 1, kept: { count: 1, byReason: { characters: 1 }, keys: [{ script: b.hash, hash: hashOf('vexl'), reason: 'characters' }] },
    leftEnglish: 0, byReason: {}, unexplained: 0, dropped: 0,
  });
  quiet(r.text, [b]);
  noneOf(r.text + coverageText, ['zarp', 'vexl', '가', '나', '다', 'abc']);
  await rm(f.dir, { recursive: true });
});

test('keywords per script: the yes and no words beside a key yes or no of another block: only the same word, or it with one particle, collides (아니 passes beside 아니오 and 아니요, not beside 아니 or 아니야; 예 beside 그렇소 or 네, not beside 예), so the guide\'s 예 and 아니 pass beside 그렇소 and 아니오', () => {
  const pair = (keys, a, b) => {
    const r = aliasesOf(keys, [{ k: keys[0], ko: a }, { k: keys[1], ko: b }], compatibleOf([[keys[0]], [keys[1]]]));
    return r.words.size === 2 ? 'kept' : [...r.failed.values()].join(',');
  };
  const nNo = Object.fromEntries(['아니오', '아니요', '아니', '아니야'].map((no) => [no, Object.fromEntries(['아니', '아니요', '아니오'].map((n) => [n, pair(['n', 'no'], n, no)]))]));
  assert.deepEqual(nNo, {
    아니오: { 아니: 'kept', 아니요: 'kept', 아니오: 'shared,shared' },
    아니요: { 아니: 'kept', 아니요: 'shared,shared', 아니오: 'kept' },
    아니: { 아니: 'shared,shared', 아니요: 'kept', 아니오: 'kept' },
    아니야: { 아니: 'shared,shared', 아니요: 'kept', 아니오: 'kept' },
  }, 'for no (rows) and n (columns): 오 and 요 are no particles, 야 is');
  const yYes = Object.fromEntries(['그렇소', '예', '네'].map((yes) => [yes, Object.fromEntries(['예', '네'].map((y) => [y, pair(['y', 'yes'], y, yes)]))]));
  assert.deepEqual(yYes, { 그렇소: { 예: 'kept', 네: 'kept' }, 예: { 예: 'shared,shared', 네: 'kept' }, 네: { 예: 'kept', 네: 'shared,shared' } });
  // In one block (y beside yes) the two may share the word: the rule of a block is its own.
  assert.equal(aliasesOf(['y', 'yes'], [{ k: 'y', ko: '예' }, { k: 'yes', ko: '예' }], compatibleOf([['y', 'yes']])).words.size, 2);
});

// The guide's yes and no words, set by the tool (style-ko.md): a script of the made-up `topics`
// planned on its own, and what settling its keys from an answer giving `keywords` keeps and
// refuses.
const planned = (item, topics) => planOf([kbundle('a', item, `000000000021${item}`, `${SENTINEL}Fixed${item}`, topics)], { first: 1 }).scripts[0];
const settled = (s, keywords, existing = null) => {
  settleKeys(s, { ok: true, keywords }, existing);
  return [Object.fromEntries(s.kept), Object.fromEntries(s.refused)];
};

test('the yes and no words set by the tool: an n its first block lists alone, answered 예, settles to 아니, not refused, and its lines are gated with 아니', () => {
  const s = planned(8701, [[['n'], 'A @nix to thee.'], [['zorb'], 'The @zorb.']]);
  assert.deepEqual(settled(s, [{ k: 'n', ko: '예' }, { k: 'zorb', ko: '돌' }]), [{ n: '아니', zorb: '돌' }, {}]);
  assert.deepEqual(s.keyed.words.get('n'), ['아니']);
  assert.equal(gate('A @nix to thee.', fakeKo('A @nix to thee.', new Map([['nix', '예']])), s.keyed), 'keywords', 'a line showing 예 for it fails');
  assert.equal(gate('A @nix to thee.', fakeKo('A @nix to thee.', new Map([['nix', '아니']])), s.keyed), null);
});

test('the yes and no words set by the tool: a y its first block lists alone settles to 예 when the answer gives it none, two words or a word the alias gate refuses, never refused; a y beside another key is no reply and stays missing; a request that failed settles no word', () => {
  const s = planned(8702, [[['y'], 'So be it.'], [['zorb'], 'The @zorb.']]);
  assert.deepEqual(settled(s, [{ k: 'zorb', ko: '돌' }]), [{ y: '예', zorb: '돌' }, {}], 'none given');
  assert.deepEqual(settled(s, [{ k: 'y', ko: '네' }, { k: 'y', ko: '응' }, { k: 'zorb', ko: '돌' }]), [{ y: '예', zorb: '돌' }, {}], 'two words given');
  assert.deepEqual(settled(s, [{ k: 'y', ko: 'abc' }, { k: 'zorb', ko: '돌' }]), [{ y: '예', zorb: '돌' }, {}], 'a word the gate refuses');
  assert.deepEqual(settled(planned(8703, [[['y', 'zorb'], 'The @zorb.']]), [{ k: 'zorb', ko: '돌' }]), [{ zorb: '돌' }, { y: 'missing' }], 'y beside zorb');
  settleKeys(s, { ok: false, reason: 'invalid' });
  assert.deepEqual([Object.fromEntries(s.kept), Object.fromEntries(s.refused)], [{}, { y: 'invalid', zorb: 'invalid' }], 'its keys keep the pack\'s');
});

test('the yes and no words set by the tool: a key yes in a block with three other keys, all answered with one word, settles to 그렇소; the other keys keep that word', () => {
  const s = planned(8704, [[['zorb', 'yes', 'plim', 'vexl'], 'Ask of the @zorb.'], [['grak'], 'A @grak.']]);
  const one = ['zorb', 'yes', 'plim', 'vexl'].map((k) => ({ k, ko: '좋소' }));
  assert.deepEqual(settled(s, [...one, { k: 'grak', ko: '풀' }]), [{ zorb: '좋소', yes: '그렇소', plim: '좋소', vexl: '좋소', grak: '풀' }, {}]);
});

test('the yes and no words set by the tool: another key answered 그렇소 or 아니오 beside a key yes or no of another block is refused (shared), the fixed word kept; a key of the same block may keep it', () => {
  const s = planned(8705, [[['yes', 'plim'], 'Ask of the @plim.'], [['zorb'], 'The @zorb.'], [['no'], 'Begone.'], [['vexl'], 'A @vexl.']]);
  const given = (plim, zorb) => [{ k: 'yes', ko: '좋소' }, { k: 'plim', ko: plim }, { k: 'zorb', ko: zorb }, { k: 'no', ko: '싫소' }, { k: 'vexl', ko: '아니오' }];
  assert.deepEqual(settled(s, given('좋소', '그렇소')), [{ yes: '그렇소', plim: '좋소', no: '아니오' }, { zorb: 'shared', vexl: 'shared' }]);
  assert.deepEqual(settled(s, given('그렇소', '돌')), [{ yes: '그렇소', plim: '그렇소', zorb: '돌', no: '아니오' }, { vexl: 'shared' }], 'plim beside yes');
});

test('the yes and no words set by the tool: a script\'s request names a key no or yes its fixed word as known (아니오, 그렇소), unmarked, whatever word the pack there has for it, a request again too; the pack keeps those words', async () => {
  const V = 8706;
  const b = kbundle('a', V, '0000000000228706', `${SENTINEL}FixKnown`, [[['no', 'zorb'], 'The @zorb.'], [['yes'], 'So be it.']]);
  const f = await kfixture([b]);
  await packFor(f.out, [b], [['a', V, 'no', '싫소'], ['a', V, 'zorb', '돌']]);
  const t = wordsTransport();
  const r = await run([f.bundles, '--item', `a:${V}`, '--out', f.out, '--send'], t.send);
  assert.equal(r.code, 0);
  assert.deepEqual(userOf(t.bodies[0]).topics, [
    { keys: [{ k: 'no', shown: [], known: '아니오' }, { k: 'zorb', shown: ['zorb'], known: '돌' }] },
    { keys: [{ k: 'yes', shown: [], known: '그렇소' }] },
  ], 'no: 아니오, not the pack\'s word; yes: 그렇소 with none in the pack');
  const [s] = planOf([b], { first: 1 }).scripts;
  assert.deepEqual(topicsOf(s, new Map([['no', '싫소'], ['zorb', '돌']]), new Set(['no', 'zorb'])), [
    { keys: [{ k: 'no', shown: [], known: '아니오' }, { k: 'zorb', shown: ['zorb'], known: '돌' }] },
  ], 'asked again: the same');
  const alias = (keyword, word) => ({ archive: 'a', item: V, keyword, words: [word] });
  assert.deepEqual((await packOf(f.out)).aliases, [alias('no', '아니오'), alias('yes', '그렇소'), alias('zorb', '돌')]);
  const coverage = await readFile(join(f.out, 'coverage.json'), 'utf8');
  quiet(r.text, [b]);
  noneOf(r.text + coverage, ['zorb', '싫소', '돌', '아니오', '그렇소']);
  await rm(f.dir, { recursive: true });
});

test('the yes and no words set by the tool: a key yes or no, its word set, is not asked in the shared keywords\' request (an entry the answer gives it is not read); a shared word colliding with its word (another block) reaches no script as known; every script\'s request names yes 그렇소', async () => {
  const items = [8711, 8712, 8713];
  const bundles = items.map((item, i) => kbundle('a', item, `000000000023${item}`, `${SENTINEL}FixShare${'pqr'[i]}`, [
    [['plo'], 'Try my @plo.'], [['yes'], 'So be it.'], [[`my${'abc'[i]}`], `Mine is @my${'abc'[i]}.`],
  ]));
  assert.deepEqual(glossaryOf(planOf(bundles, { first: 3 }).scripts).units, [{ kind: 'keyword', n: 0, k: 'plo', shown: ['plo'] }], 'yes, held by all three, not asked');
  const f = await kfixture(bundles);
  const t = wordsTransport([{ k: 'yes', ko: '좋소' }, { k: 'plo', ko: '그렇소' }]);
  const r = await run([f.bundles, '--first', '3', '--out', f.out, '--send', '--jobs', '1'], t.send);
  assert.equal(r.code, 0);
  assert.deepEqual(userOf(t.bodies[0]), { keywords: [{ k: 'plo', shown: ['plo'] }] }, 'plo alone asked first');
  const coverageText = await readFile(join(f.out, 'coverage.json'), 'utf8');
  assert.deepEqual(JSON.parse(coverageText).glossary, { found: 1, translated: 1, leftEnglish: 0, byReason: {}, unexplained: 0 }, 'plo takes 그렇소 there');
  for (const [who, topics] of topicsBy(t.bodies.slice(1))) {
    assert.deepEqual(topics.slice(0, 2), [{ keys: [{ k: 'plo', shown: ['plo'] }] }, { keys: [{ k: 'yes', shown: [], known: '그렇소' }] }],
      `${who}: plo not named 그렇소 (yes's, another block)`);
  }
  const pack = await packOf(f.out);
  assert.deepEqual(pack.aliases.filter((a) => a.keyword === 'yes').map((a) => [a.item, a.words]), items.map((item) => [item, ['그렇소']]));
  assert.deepEqual(pack.aliases.filter((a) => a.keyword === 'plo').map((a) => [a.item, a.words]), items.map((item) => [item, [fakeWord('plo')]]));
  quiet(r.text, bundles);
  noneOf(r.text + coverageText, ['plo', 'mya', 'myb', 'myc', '좋소', '그렇소', fakeWord('plo')]);
  await rm(f.dir, { recursive: true });
});

test('the yes and no words set by the tool: the word the pack keeps for another key, colliding with a fixed word (another block), is dropped: that key, named no known word and answered none, ends with no word (shared, left English), its line with the English word, the fixed key with its word; the merged pack holds no two colliding words', async () => {
  const V = 8721;
  const b = kbundle('a', V, '0000000000248721', `${SENTINEL}FixKept`, [[['n'], 'A @nix to thee.'], [['zorb'], 'The @zorb.']]);
  const [s] = planOf([b], { first: 1 }).scripts;
  const there = { aliases: [{ archive: 'a', item: V, keyword: 'n', words: ['예'] }, { archive: 'a', item: V, keyword: 'zorb', words: ['아니'] }] };
  assert.deepEqual(settled(s, [], there), [{ n: '아니' }, { zorb: 'shared' }], 'zorb, given none, loses its kept word');
  assert.deepEqual([[...s.lost], [...s.keyed.words]], [['zorb'], [['n', ['아니']]]]);
  const f = await kfixture([b]);
  // The pack of an earlier run: n 예 (the defect's kind), zorb 아니.
  await packFor(f.out, [b], [['a', V, 'n', '예'], ['a', V, 'zorb', '아니']]);
  const bodies = [];
  const send = async (body) => {
    bodies.push(body);
    // The answer gives zorb no word; its line keeps the English one.
    const { lines, keywords } = fakeAnswer(userOf(body), (key) => (key.k === 'zorb' ? undefined : key.known ?? fakeWord(key.k)));
    return answer(lines, keywords.filter((x) => x.ko !== undefined));
  };
  const r = await run([f.bundles, '--item', `a:${V}`, '--out', f.out, '--send'], send);
  assert.equal(r.code, 3, 'a key left English');
  assert.deepEqual(userOf(bodies[0]).topics, [
    { keys: [{ k: 'n', shown: ['nix'], known: '아니', answer: 'no' }] }, { keys: [{ k: 'zorb', shown: ['zorb'] }] },
  ], 'zorb named no known word: its old one collides with n\'s');
  assert.equal(bodies.length, 1, 'every line lands');
  const pack = await packOf(f.out);
  assert.deepEqual(pack.aliases, [{ archive: 'a', item: V, keyword: 'n', words: ['아니'] }]);
  assert.ok(pack.converse.find((x) => x.hash === hashOf('The @zorb.')).text.includes('@{zorb}'), 'its line with the English word');
  assert.ok(pack.converse.find((x) => x.hash === hashOf('A @nix to thee.')).text.includes('@{아니}'));
  for (const a of pack.aliases) {
    for (const o of pack.aliases) if (a !== o) assert.ok(!a.words.some((w) => o.words.some((v) => collide(w, v))), 'no two colliding words');
  }
  const coverageText = await readFile(join(f.out, 'coverage.json'), 'utf8');
  assert.deepEqual(JSON.parse(coverageText).keywords, {
    found: 2, translated: 1, kept: NONE_KEPT, leftEnglish: 1, byReason: { shared: 1 }, unexplained: 0, dropped: 0,
  });
  quiet(r.text, [b]);
  noneOf(r.text + coverageText, ['nix', 'zorb', '예', '아니']);
  await rm(f.dir, { recursive: true });
});

test('the yes and no words set by the tool: one run cleans a pack whose word for another key collides with a fixed word (another block): the request names that key no known word (one certain to be refused), so the answer gives it a new one, kept; a key of the fixed key\'s own block keeps its known word', async () => {
  const V = 8722;
  const b = kbundle('a', V, '0000000000248722', `${SENTINEL}FixClean`, [[['n'], 'A @nix to thee.'], [['zorb'], 'The @zorb.']]);
  const f = await kfixture([b]);
  // The pack of an earlier run: n 예 (the defect's kind), zorb 아니.
  await packFor(f.out, [b], [['a', V, 'n', '예'], ['a', V, 'zorb', '아니']]);
  const t = wordsTransport();
  const r = await run([f.bundles, '--item', `a:${V}`, '--out', f.out, '--send'], t.send);
  assert.equal(r.code, 0, 'done in one run');
  assert.equal(t.bodies.length, 1, 'nothing asked again');
  assert.deepEqual(userOf(t.bodies[0]).topics, [
    { keys: [{ k: 'n', shown: ['nix'], known: '아니', answer: 'no' }] }, { keys: [{ k: 'zorb', shown: ['zorb'] }] },
  ]);
  const pack = await packOf(f.out);
  assert.deepEqual(pack.aliases, [{ archive: 'a', item: V, keyword: 'n', words: ['아니'] }, { archive: 'a', item: V, keyword: 'zorb', words: [fakeWord('zorb')] }]);
  assert.ok(pack.converse.find((x) => x.hash === hashOf('The @zorb.')).text.includes(`@{${fakeWord('zorb')}}`));
  const coverageText = await readFile(join(f.out, 'coverage.json'), 'utf8');
  assert.deepEqual(JSON.parse(coverageText).keywords, {
    found: 2, translated: 2, kept: NONE_KEPT, leftEnglish: 0, byReason: {}, unexplained: 0, dropped: 0,
  });
  // Beside yes in its block a key keeps 그렇소 as known; a key of another block is named none for
  // 그렇소 with a particle.
  const s = planned(8723, [[['yes', 'plim'], 'Ask of the @plim.'], [['vexl'], 'A @vexl.']]);
  assert.deepEqual(topicsOf(s, new Map([['plim', '그렇소'], ['vexl', '그렇소이']])), [
    { keys: [{ k: 'yes', shown: [], known: '그렇소' }, { k: 'plim', shown: ['plim'], known: '그렇소' }] }, { keys: [{ k: 'vexl', shown: ['vexl'] }] },
  ]);
  quiet(r.text, [b]);
  noneOf(r.text + coverageText, ['nix', 'zorb', '예', '아니', fakeWord('zorb')]);
  await rm(f.dir, { recursive: true });
});

test('the yes and no words set by the tool: a kept word colliding with a fixed word is lost first and is no constraint on any key, in whatever order the answer gives the keys', () => {
  const kept = new Map([['a', ['아니야']], ['b', ['돌']]]);
  const of = (given) => {
    const r = aliasesOf(['n', 'a', 'b', 'c'], given, compatibleOf([['n'], ['a'], ['b'], ['c']]), kept, new Map([['n', '아니']]));
    return [Object.fromEntries(r.words), Object.fromEntries(r.failed), [...r.lost]];
  };
  const given = [{ k: 'a', ko: '돌' }, { k: 'c', ko: '아니야도' }];
  // a's new word collides with b's kept one (refused); a's kept 아니야 (아니 with a particle) is
  // lost to n's 아니, so c's 아니야도 (아니야 with a particle) is kept.
  const want = [{ n: '아니', c: '아니야도' }, { a: 'shared', b: 'missing' }, ['a']];
  assert.deepEqual(of(given), want, 'a first');
  assert.deepEqual(of([...given].reverse()), want, 'c first');
});

// --- spec 12: the register by profile (made-up speakers only) ----------------------------------

const NOTE = 'Zq8NoteSentinel';
// A u6.speakers/1 listing `entries`, each [archive, item, age, class, note].
const speakersOf = (entries) => ({
  format: SPEAKERS_FORMAT, model: 'test-model', speakers: entries.map(([archive, item, age, cls, note]) => ({ archive, item, age, class: cls, note })),
});

async function sfixture(bundles, speakers) {
  const f = await kfixture(bundles);
  const path = join(f.dir, 'speakers.json');
  await writeFile(path, typeof speakers === 'string' ? speakers : JSON.stringify(speakers));
  return { ...f, speakers: path };
}

const npcsOf = (bodies) => new Map(bodies.map((b) => userOf(b)).filter((u) => u.npc).map((u) => [u.npc.name.replace(SENTINEL, ''), u.npc]));

test('spec 12: the speaker file\'s reader takes u6.speakers/1 exactly (each age and class one of its enums, a note of at most SPEAKER_NOTE_MAX characters, a script once; its coverage receipt, an object, passed over) and refuses anything else with a fixed message, a count at most', () => {
  const read = readSpeakers(speakersOf([['a', 1, 'child', 'commoner', `${NOTE}a`], ['b', 65535, 'elder', 'mage', '']]));
  assert.deepEqual([...read], [['a:1', { age: 'child', class: 'commoner', note: `${NOTE}a` }], ['b:65535', { age: 'elder', class: 'mage', note: '' }]]);
  assert.equal(readSpeakers(speakersOf([])).size, 0, 'an empty list reads');
  assert.equal(readSpeakers(speakersOf([['a', 1, 'youth', 'noble', 'x'.repeat(SPEAKER_NOTE_MAX)]])).size, 1, 'a note of the most characters');
  const shape = /^the speakers file is not u6\.speakers\/1$/;
  const entries = /^the speakers file is not u6\.speakers\/1: 1 entries are not \{archive, item, age, class, note\} of its forms$/;
  const good = { archive: 'a', item: 1, age: 'child', class: 'commoner', note: NOTE };
  const file = (speakers, extra = {}) => ({ format: SPEAKERS_FORMAT, model: 'm', speakers, ...extra });
  for (const coverage of [{}, { scripts: { read: 1 }, fallback: [{ archive: 'b', item: 9, age: 'missing', class: null }], extra: NOTE }]) {
    assert.deepEqual([...readSpeakers(file([good], { coverage }))], [...readSpeakers(file([good]))], 'the coverage member changes nothing read');
  }
  for (const [what, value, rule] of [
    ['null', null, shape], ['a list', [], shape], ['a string', 'u6.speakers/1', shape],
    ['another format', { ...file([]), format: 'u6.speakers/2' }, shape],
    ['no model', { format: SPEAKERS_FORMAT, speakers: [] }, shape],
    ['a model not text', file([], { model: 1 }), shape],
    ['a member more', file([], { extra: 1 }), shape],
    ['a coverage not an object', file([], { coverage: [] }), shape],
    ['a coverage of null', file([], { coverage: null }), shape],
    ['a coverage of text', file([], { coverage: NOTE }), shape],
    ['speakers not a list', file({}), shape],
    ['an age not of the enum', file([{ ...good, age: 'teen' }]), entries],
    ['an age of another case', file([{ ...good, age: 'Child' }]), entries],
    ['a class not of the enum', file([{ ...good, class: 'emperor' }]), entries],
    ['a class of the prototype', file([{ ...good, class: '__proto__' }]), entries],
    ['an archive not a or b', file([{ ...good, archive: 'c' }]), entries],
    ['an item below 0', file([{ ...good, item: -1 }]), entries],
    ['an item past 65535', file([{ ...good, item: 65536 }]), entries],
    ['an item not whole', file([{ ...good, item: 1.5 }]), entries],
    ['a note not text', file([{ ...good, note: 5 }]), entries],
    ['a note too long', file([{ ...good, note: 'x'.repeat(SPEAKER_NOTE_MAX + 1) }]), entries],
    ['no note', file([{ archive: 'a', item: 1, age: 'child', class: 'commoner' }]), entries],
    ['an entry member more', file([{ ...good, name: NOTE }]), entries],
    ['an entry not an object', file([null]), entries],
    ['a script twice', file([good, { ...good, age: 'elder' }]), /^the speakers file is not u6\.speakers\/1: 1 entries list a script listed before$/],
  ]) {
    assert.throws(() => readSpeakers(value), (err) => err instanceof InputError && rule.test(err.message) && !err.message.includes(NOTE), what);
  }
});

test('spec 12: the register for a class at an age: an adult\'s is the class\'s; a child\'s the same whatever its class, a child\'s speech; a young person\'s 해요체 for the plain stations, a soft 하오체 for the others; an elder\'s the class\'s with an old person\'s turns', () => {
  const classes = Object.keys(CLASSES);
  assert.deepEqual(AGES, ['child', 'youth', 'adult', 'elder']);
  for (const c of classes) {
    assert.equal(registerOf(c, 'adult'), CLASSES[c], `${c}: an adult's is today's`);
    assert.equal(registerOf(c), CLASSES[c], `${c}: no age is an adult's`);
    assert.equal(registerOf(c, 'child'), registerOf('commoner', 'child'), `${c}: a child speaks as a child`);
    assert.ok(registerOf(c, 'elder').startsWith(CLASSES[c]) && registerOf(c, 'elder').includes('~구먼'), `${c}: an elder's`);
    assert.equal(new Set(AGES.map((a) => registerOf(c, a))).size, 4, `${c}: each age its own`);
  }
  assert.ok(registerOf('commoner', 'child').includes('해요체') && registerOf('commoner', 'child').includes('반말'));
  for (const c of ['merchant', 'commoner', 'thief']) assert.ok(registerOf(c, 'youth').startsWith('해요체'), `${c}: a young one's 해요체`);
  for (const c of classes.filter((x) => !['merchant', 'commoner', 'thief'].includes(x))) {
    assert.ok(registerOf(c, 'youth').startsWith('a soft 하오체'), `${c}: a young one's soft 하오체`);
  }
});

test('spec 12: --speakers: a script the file lists takes its class and age, the request\'s NPC naming its age, its profile (the note) and the register for that class at that age; one it does not list Pass A as an adult with no profile; --class still wins for the run; the coverage counts the ages; nothing printed names a note', async () => {
  const f = await sfixture(BUNDLES, speakersOf([
    ['a', ITEMS[0], 'child', 'commoner', `${NOTE}Small`], ['b', ITEMS[2], 'elder', 'spirit', `${NOTE}Old`], ['b', 9999, 'youth', 'noble', ''],
  ]));
  const t = fakeTransport();
  const r = await run([f.bundles, '--first', '9', '--speakers', f.speakers, '--out', f.out, '--send', '--jobs', '1'], t.send);
  assert.equal(r.code, 3, 'written, the bad script left English');
  const npcs = npcsOf(t.bodies);
  const look = (i) => BUNDLES[i].narration.look[0].text;
  assert.deepEqual(npcs.get('Good'), {
    name: `${SENTINEL}Good`, description: look(0), class: 'commoner', age: 'child', profile: `${NOTE}Small`, register: registerOf('commoner', 'child'),
  }, 'the file\'s class (not Pass A\'s guard), age and note');
  assert.deepEqual(Object.keys(npcs.get('Good')), ['name', 'description', 'class', 'age', 'profile', 'register']);
  assert.deepEqual(npcs.get('Again'), { name: `${SENTINEL}Again`, description: look(1), class: 'merchant', age: 'adult', profile: '', register: CLASSES.merchant },
    'not listed: Pass A, an adult');
  assert.deepEqual(npcs.get('Bad'), { name: `${SENTINEL}Bad`, description: look(2), class: 'spirit', age: 'elder', profile: `${NOTE}Old`, register: registerOf('spirit', 'elder') });
  const again = t.bodies.filter((b) => b.includes(`${SENTINEL}Again`)).map((b) => userOf(b).npc);
  assert.equal(again.length, 2, 'a line asked again');
  assert.deepEqual(again[1], again[0], 'a request again names the same speaker');
  const coverage = JSON.parse(await readFile(join(f.out, 'coverage.json'), 'utf8'));
  assert.deepEqual(coverage.ages, {
    child: { chosen: 1, listed: 1, translated: 1 }, youth: { chosen: 0, listed: 0, translated: 0 },
    adult: { chosen: 1, listed: 0, translated: 1 }, elder: { chosen: 1, listed: 1, translated: 0 },
  });
  assert.deepEqual(coverage.perScript.map((p) => [p.class, p.age]), [['commoner', 'child'], ['merchant', 'adult'], ['spirit', 'elder']]);
  assert.match(r.text, /Speakers: 3 scripts listed in .*speakers\.json; of the 3 chosen, 2 listed \(the others by Pass A, as adults\); child 1, youth 0, adult 1, elder 1\./);
  assert.match(r.text, /Scripts by age \(translated of chosen\): child 1 of 1, youth 0 of 0, adult 1 of 1, elder 0 of 1\./);
  assert.ok(!r.text.includes(NOTE) && !JSON.stringify(coverage).includes(NOTE), 'no note printed or in the receipt');
  nothingTold(r.text);
  // --class gives the class for the run; the file's age and note stay.
  const c = await run([f.bundles, '--item', `a:${ITEMS[0]}`, '--speakers', f.speakers, '--class', 'royalty', '--out', join(f.dir, 'c'), '--dry-run'], null);
  assert.equal(c.code, 0);
  const npc = userOf(await readFile(previewPrinted(c.text), 'utf8')).npc;
  assert.deepEqual([npc.class, npc.age, npc.profile, npc.register], ['royalty', 'child', `${NOTE}Small`, registerOf('royalty', 'child')]);
  nothingTold(c.text);
  await rm(f.dir, { recursive: true });
});

test('spec 12: without --speakers the request is byte for byte main\'s (the NPC\'s name, description, class and register only), its plan has no speaker and its coverage no ages; with an empty speaker file every script is an adult and its request names that', async () => {
  const NOKEY = {
    format: 'u6.npc-bundle/1', archive: 'a', item: 7301, hash: '00000000000d7301', refused: null, actors: [], portraits: [], name: 'Zq8Plain', locale: 'en',
    narration: { look: [seg('a', 7301, 20, 'a plain test')] },
    speech: { greeting: [seg('a', 7301, 60, 'Hail, $G. Ask of my @wares.')] },
    turns: [{ input: { at: 90, kind: 'ask' }, segments: [], topics: [{ at: 0, keywords: ['*'], requires: [], answer: [seg('a', 7301, 100, 'Naught more.')] }] }],
    traits: [], strings: [{ key: ['a', 7301, 200], table: 200, index: 0, text: 'a cup' }],
  };
  const [s] = planOf([NOKEY], { item: { archive: 'a', item: 7301 } }).scripts;
  assert.equal(s.speaker, null, 'no speaker without a file');
  const units = s.units.filter((u) => !u.done);
  for (const body of [bodyOf('gpt-6-astra', systemOf('STYLE'), s.bundle, s.cls, units), bodyOf('gpt-6-astra', systemOf('STYLE'), s.bundle, s.cls, units, [], null)]) {
    assert.equal(hex64(hash64(Buffer.from(body))), '8dfd9fc326baf037', 'main\'s body (the K5b test\'s), byte for byte');
  }
  const f = await sfixture([NOKEY], speakersOf([]));
  const plainRun = await run([f.bundles, '--item', 'a:7301', '--out', f.out, '--dry-run'], null, { style: 'STYLE' });
  assert.equal(hex64(hash64(await readFile(previewPrinted(plainRun.text)))), '8dfd9fc326baf037', 'the preview without --speakers');
  assert.ok(!plainRun.text.includes('Speakers:'), 'no speaker line');
  const [e] = planOf([NOKEY], { item: { archive: 'a', item: 7301 }, speakers: new Map() }).scripts;
  assert.deepEqual(e.speaker, { age: 'adult', profile: '', listed: false });
  const listed = await run([f.bundles, '--item', 'a:7301', '--speakers', f.speakers, '--out', join(f.dir, 'b'), '--dry-run'], null, { style: 'STYLE' });
  assert.equal(listed.code, 0);
  assert.deepEqual(userOf(await readFile(previewPrinted(listed.text), 'utf8')).npc,
    { name: 'Zq8Plain', description: 'a plain test', class: 'commoner', age: 'adult', profile: '', register: CLASSES.commoner });
  // A run with --send and no --speakers: the coverage as before.
  const g = await kfixture(BUNDLES.slice(0, 2));
  const t = fakeTransport();
  assert.equal((await run([g.bundles, '--first', '2', '--out', g.out, '--send'], t.send)).code, 0);
  assert.ok(t.bodies.every((b) => JSON.stringify(Object.keys(userOf(b).npc)) === '["name","description","class","register"]'), 'the NPC as before');
  const coverage = JSON.parse(await readFile(join(g.out, 'coverage.json'), 'utf8'));
  assert.ok(!Object.hasOwn(coverage, 'ages'), 'no ages');
  assert.ok(coverage.perScript.every((p) => JSON.stringify(Object.keys(p)) === '["hash","class","units","tables","translated","left"]'), 'each script as before');
  await rm(f.dir, { recursive: true });
  await rm(g.dir, { recursive: true });
});

// Five scripts with a line each and a keyword of their own (so no shared keywords' request): the
// speaker file gives the first four ages, the fifth none (an adult).
const AGE_ITEMS = [7701, 7702, 7703, 7704, 7705];
const aged = () => AGE_ITEMS.map((item, i) => bundle('a', item, `00000000001b${item}`, `${SENTINEL}Age${i}`, { lines: [`Line ${i}.`], keywords: [`own${i}`] }));
const AGED = speakersOf([['a', 7701, 'child', 'commoner', ''], ['a', 7702, 'elder', 'mage', ''], ['a', 7703, 'child', 'thief', ''], ['a', 7704, 'youth', 'noble', '']]);

test('spec 12: --ages chooses the scripts the speaker file gives those ages (a script it does not list an adult), in order: all of them, or the first N with --first; each sent once, nothing else', async () => {
  const speakers = readSpeakers(AGED);
  const items = (o) => planOf(aged(), { speakers, ...o }).scripts.map((s) => s.bundle.item);
  assert.deepEqual(items({ ages: ['child'] }), [7701, 7703]);
  assert.deepEqual(items({ ages: ['child'], first: 1 }), [7701]);
  assert.deepEqual(items({ ages: ['adult'] }), [7705], 'the one not listed');
  assert.deepEqual(items({ ages: ['child', 'youth', 'elder'] }), [7701, 7702, 7703, 7704]);
  assert.deepEqual(items({ ages: ['child', 'youth', 'elder'], first: 2 }), [7701, 7702], 'the first 2 of those ages');
  assert.deepEqual(items({ first: 2 }), [7701, 7702], 'without --ages, --first as before');
  const f = await sfixture(aged(), AGED);
  for (const [args, want] of [[['--ages', 'child'], ['Age0', 'Age2']], [['--ages', 'child,youth,elder', '--first', '3'], ['Age0', 'Age1', 'Age2']],
    [['--first', '3', '--ages', 'adult'], ['Age4']]]) {
    const t = fakeTransport();
    const out = join(f.dir, `out-${args.join('')}`);
    const r = await run([f.bundles, ...args, '--speakers', f.speakers, '--out', out, '--send'], t.send);
    assert.equal(r.code, 0, `${args}: every line translated`);
    assert.deepEqual([...t.asked.keys()].map((k) => k.replace(SENTINEL, '')).sort(), want, `${args}: those scripts, once each`);
    assert.equal(t.bodies.length, want.length);
    nothingTold(r.text);
  }
  const none = await sfixture(aged(), speakersOf([['a', 7701, 'child', 'commoner', '']]));
  const r = await run([none.bundles, '--ages', 'youth', '--speakers', none.speakers, '--out', none.out, '--send'], async () => assert.fail('no request'));
  assert.equal(r.code, 1);
  assert.match(r.text, /0 chosen.*\n.*Nothing to translate: nothing was sent\./s);
  await rm(f.dir, { recursive: true });
  await rm(none.dir, { recursive: true });
});

test('spec 12: --ages and --speakers refused with a fixed message before any request: --ages with no speaker file, an age not of the enum, with --item or --name, a speaker file with no bundles, a speaker file that does not read or check', async () => {
  const f = await sfixture(aged(), AGED);
  const never = async () => assert.fail('no request');
  for (const [args, rule] of [
    [[f.bundles, '--ages', 'child'], /--ages chooses by the ages of a --speakers file: name one/],
    [[f.bundles, '--ages', 'kid', '--speakers', f.speakers], /--ages names ages of the speaker file: child, youth, adult, elder/],
    [[f.bundles, '--ages', '', '--speakers', f.speakers], /--ages names ages of the speaker file/],
    [[f.bundles, '--ages', 'child,', '--speakers', f.speakers], /--ages names ages of the speaker file/],
    [[f.bundles, '--ages', 'child', '--item', 'a:7701', '--speakers', f.speakers], /--ages goes alone or with --first N, not with --item or --name/],
    [[f.bundles, '--ages', 'child', '--name', 'x', '--speakers', f.speakers], /--ages goes alone or with --first N, not with --item or --name/],
    [['--strings', f.bundles, '--speakers', f.speakers], /--speakers and --ages go with a bundles file/],
    [[f.bundles, '--speakers', f.speakers], /name one script with --item or --name, or --first N, or choose by --ages/],
  ]) {
    const r = await run([...args, '--out', f.out, '--send'], never);
    assert.equal(r.code, 2, `${args}: refused`);
    assert.match(r.text, rule, `${args}: says why`);
    assert.ok(!r.text.includes('kid'), 'the refused value is not echoed');
  }
  const bad = [
    ['not JSON', '{', /the speakers file does not read as JSON/],
    ['another format', JSON.stringify({ ...AGED, format: 'u6.voice-notes/1' }), /the speakers file is not u6\.speakers\/1$/m],
    ['an age not of the enum', JSON.stringify(speakersOf([['a', 7701, 'teen', 'commoner', NOTE]])), /the speakers file is not u6\.speakers\/1: 1 entries are not/],
  ];
  for (const [what, text, rule] of bad) {
    const g = await sfixture(aged(), text);
    const r = await run([g.bundles, '--first', '1', '--speakers', g.speakers, '--out', g.out, '--send'], never);
    assert.equal(r.code, 2, `${what}: refused`);
    assert.match(r.text, rule, `${what}: its fixed message`);
    assert.ok(!r.text.includes('Route:') && !existsSync(g.out), `${what}: before the route, the output directory or a preview`);
    assert.ok(!r.text.includes(NOTE) && !r.text.includes('teen'), `${what}: nothing of the file printed`);
    nothingTold(r.text);
    await rm(g.dir, { recursive: true });
  }
  const missing = await run([f.bundles, '--first', '1', '--speakers', join(f.dir, 'none.json'), '--out', f.out, '--send'], never);
  assert.equal(missing.code, 2);
  assert.match(missing.text, /the speakers file cannot be read \(ENOENT\)/);
  await rm(f.dir, { recursive: true });
});

test('spec 12: the guide by age: the ages section (child, youth, adult, elder, made-up examples), the modern endings banned for adult and elder speakers only, a child never saying 그대; the class table and the rest as before', async () => {
  const guide = await readFile(join(dirname(TOOL), 'style-ko.md'), 'utf8');
  const ages = guide.slice(guide.indexOf('## Speaker ages'), guide.indexOf('## Speaker classes'));
  assert.ok(ages.length > 0 && guide.indexOf('## Speaker ages') < guide.indexOf('## Speaker classes'), 'the ages before the classes');
  for (const a of AGES) assert.ok(ages.includes(`- \`${a}\`:`), `${a}'s rule`);
  assert.ok(ages.includes('never 그대') && ages.includes('아저씨, 우리 고양이 못 봤어요?'), 'the child\'s address and example');
  assert.ok(guide.includes('An `adult` or an `elder` speaker never uses modern endings') && !guide.includes('Never use modern endings'), 'the ban scoped');
  assert.ok(guide.includes('No speaker uses modern loanwords or slang.'));
  for (const c of Object.keys(CLASSES)) assert.ok(guide.includes(`| ${c} |`), `${c} still in the table`);
});

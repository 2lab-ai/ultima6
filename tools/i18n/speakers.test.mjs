// Tests of tools/i18n/speakers.mjs (spec 12 Design 1, spec 08's upload boundary) on synthetic
// bundles and notes the test writes (no game data, made-up names and notes), with a counting fake
// transport, in a directory of its own (never ~/Games/ultima6):
//
// - zero requests: no terminal and no --send, --dry-run (with --send too), and on a terminal a
//   refusal, Ctrl-C at the prompt and SIGINT; the preview is the exact first payload, 0600 in a
//   0700 directory, under a name of the run's own; a typed yes sends;
// - with --send: the scripts translate.mjs can translate asked in batches (at most BATCH_SCRIPTS
//   scripts and BATCH_CHARS characters of text; a name cut to NAME_MAX, a script still past
//   SCRIPT_CHARS not sent, Pass A and adult, counted), each with its note; the answer gated to
//   the enums (case and spaces aside), an age not answered `adult`, a class not answered the
//   script's Pass A class, each counted by reason; the file u6.speakers/1, read by translate.mjs's
//   reader and taken by translate.mjs --speakers, its coverage receipt inside it; one file, 0600;
// - an answer that never reads, a route that refuses and a SIGINT while sending write nothing; a
//   SIGINT once the file is staged leaves the one there as it was (not renamed, the stage removed);
// - the notes reader's refusals, a bundles file that is not one, a lock another run holds and a
//   symbolic link where the output directory goes: refused before any request;
// - the CLI itself in a child process against a counting HTTP server (the real transport): no
//   terminal and no --send sends nothing, --dry-run neither, --send does and writes the file, a 307
//   or 308 to another server is not followed;
// - nothing the tool prints holds a name, a note, a script's item or a hash.
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
  BATCH_CHARS, BATCH_SCRIPTS, LOOK_MAX, MAX_TOKENS, NAME_MAX, NOTES_FORMAT, SCRIPT_CHARS, SYSTEM, bodyOf, contentOfBatch,
  gated, main, planOfSpeakers, readNotes, speakersOfAnswer,
} from './speakers.mjs';
import { AGES, CLASSES, InputError, SPEAKERS_FORMAT, SPEAKER_NOTE_MAX, main as translate, readSpeakers, registerOf } from './translate.mjs';

const TOOL = join(dirname(fileURLToPath(import.meta.url)), 'speakers.mjs');
const SENTINEL = 'Zq7SpeakerSentinel';
const NOTE = 'Zq7NoteSentinel';
const LINE = 'Zq7LineSentinel';
const ITEMS = [7101, 7102, 7103, 7104, 7105, 7106, 7107];
const hashOf = (item) => `00000000000c${item}`;

const seg = (archive, item, offset, text) => ({ kind: 'text', key: [archive, item, offset], text });

// A u6.npc-bundle/1 as the page's exporter writes it, with the test's own name, look and line
// (no text at all with `lines: false`: the look text is a run of the script too).
function bundle(archive, item, name, { look = 'a person of the town', refused = null, lines = true } = {}) {
  return {
    format: 'u6.npc-bundle/1', archive, item, hash: hashOf(item), refused, actors: [5], portraits: [], name, locale: 'en',
    narration: { look: lines ? [seg(archive, item, 20, look)] : [] },
    speech: { greeting: lines && !refused ? [seg(archive, item, 60, `"Hail. ${LINE}"`)] : [] },
    turns: [], traits: [], strings: [],
  };
}

// The fixture: five scripts to ask (one answered well, one an elder, one with answers outside the
// enums, one the answer leaves out, one in another case and with spaces), one refused and one with
// no lines (neither asked).
const BUNDLES = [
  bundle('a', ITEMS[0], `${SENTINEL}Kid`, { look: 'a small one' }),
  bundle('a', ITEMS[1], `${SENTINEL}Old`, { look: 'a grey wizard' }),
  bundle('a', ITEMS[2], `${SENTINEL}Odd`, { look: 'a guard of the gate' }),
  bundle('b', ITEMS[3], `${SENTINEL}Lost`, { look: 'a merchant of bread' }),
  bundle('b', ITEMS[4], `${SENTINEL}Shy`),
  bundle('b', ITEMS[5], `${SENTINEL}Refused`, { refused: 'test refusal' }),
  bundle('b', ITEMS[6], `${SENTINEL}Mute`, { lines: false }),
];
const NOTES = {
  format: NOTES_FORMAT, model: 'test-model',
  notes: [
    { archive: 'a', item: ITEMS[0], actors: [5], note: `${NOTE}Kid  is   small` },
    { archive: 'a', item: ITEMS[1], actors: [5], note: `${NOTE}Old` },
    { archive: 'a', item: ITEMS[2], actors: [5], note: `${NOTE}Odd` },
    { archive: 'b', item: ITEMS[4], actors: [5], note: `${NOTE}Shy` },
    { archive: 'b', item: 7999, actors: [5], note: `${NOTE}Nobody` },
  ],
};

// What the fake model answers for a character named ...`who`.
const ANSWERS = {
  Kid: { age: 'child', class: 'commoner' },
  Old: { age: 'elder', class: 'mage' },
  Odd: { age: 'teen', class: 'emperor' },
  Shy: { age: ' Youth ', class: 'MERCHANT' },
};

const userOf = (body) => JSON.parse(JSON.parse(body).messages[0].content);
const reply = (text) => ({ status: 200, json: { type: 'message', content: [{ type: 'text', text }], usage: { input_tokens: 50, output_tokens: 10 } } });

// A counting fake transport: each character's answer from its name (Lost left out); `prose`
// answers that never read.
function fakeTransport({ prose = false } = {}) {
  const bodies = [];
  const send = async (body) => {
    bodies.push(body);
    if (prose) return reply('I would rather not say.');
    const speakers = userOf(body).characters.flatMap((c) => {
      const a = ANSWERS[c.name.replace(SENTINEL, '')] ?? (c.name.includes('Many') ? { age: 'adult', class: 'guard' } : null);
      return a ? [{ n: c.n, ...a }] : [];
    });
    return reply(`\`\`\`json\n${JSON.stringify({ speakers })}\n\`\`\``);
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

async function fixture(bundles = BUNDLES, notes = NOTES) {
  const dir = await mkdtemp(join(tmpdir(), 'u6-i18n-speakers-'));
  const paths = { bundles: join(dir, 'ultima6-npc-bundles.json'), notes: join(dir, 'notes.json') };
  await writeFile(paths.bundles, JSON.stringify(bundles));
  await writeFile(paths.notes, typeof notes === 'string' ? notes : JSON.stringify(notes));
  return { dir, ...paths, out: join(dir, 'speakers') };
}

// The tool run with `args` and `send`, stdin no terminal: its exit code and what it printed.
async function run(args, send, extra = {}) {
  const out = output();
  const code = await main(args, { transport: send, stdout: out, isTTY: false, signals: new EventEmitter(), ...extra });
  return { code, text: out.text };
}

// The path of a preview the tool printed: its name holds the run's pid and a random part, either
// of which may hold a fixture's item number by chance.
const PREVIEW_PATH = /\S*first-request-[0-9]+-[0-9a-f]{12}\.json/g;

// Nothing the tool printed names an NPC, a note, a line, a script's item or a hash (the item
// numbers read past the preview's path).
function nothingTold(printed) {
  assert.ok(!printed.includes(SENTINEL) && !printed.includes(NOTE) && !printed.includes(LINE), 'no name, note or line printed');
  const told = printed.replace(PREVIEW_PATH, '');
  for (const n of [...ITEMS, 7999]) {
    assert.ok(!new RegExp(`(^|[^0-9])${n}([^0-9]|$)`).test(told), 'no item printed');
    assert.ok(!printed.includes(hashOf(n)), 'no hash printed');
  }
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
  assert.deepEqual(Object.keys(body), ['model', 'max_tokens', 'system', 'messages'], 'the model, the cap, the instruction and the text: nothing else');
  assert.equal(body.max_tokens, MAX_TOKENS);
  assert.equal(body.system, SYSTEM);
  assert.equal(userOf(preview).characters[0].name, `${SENTINEL}Kid`, 'the first script');
  if (first !== null) assert.equal(preview, first, 'the preview is the exact first payload sent');
  return preview;
}

const prompted = (out) => new Promise((done) => { const wait = () => (out.text.includes('Type yes') ? done() : setTimeout(wait, 5)); wait(); });

test('zero requests: no terminal and no --send; --dry-run, alone and with --send', async () => {
  const f = await fixture();
  const t = fakeTransport();
  const none = await run([f.bundles, '--notes', f.notes, '--out', f.out], t.send);
  assert.equal(none.code, 1);
  assert.match(none.text, /Route: llmux on this machine \(http:\/\/127\.0\.0\.1:3456\), then OpenAI \(model gpt-6-astra\)\./);
  assert.match(none.text, /Nothing was sent: there is no terminal to ask on; add --send to send\./);
  await previewChecked(f.out, none.text);
  for (const args of [['--dry-run'], ['--dry-run', '--send']]) {
    const r = await run([f.bundles, '--notes', f.notes, '--out', f.out, ...args], t.send);
    assert.equal(r.code, 0, `${args}: done`);
    assert.match(r.text, /Dry run: nothing was sent\./);
    await previewChecked(f.out, r.text);
    nothingTold(r.text);
  }
  assert.equal(t.bodies.length, 0, 'no request');
  assert.ok(!existsSync(join(f.out, 'speakers.json')), 'nothing written');
  nothingTold(none.text);
  await rm(f.dir, { recursive: true });
});

for (const [what, keys, want] of [['a refusal', 'no\r', 1], ['Ctrl-C at the prompt', '\x03', 130], ['SIGINT', null, 130]]) {
  test(`zero requests on a terminal: ${what}`, async () => {
    const f = await fixture();
    const t = fakeTransport();
    const out = output();
    const stdin = terminal();
    const signals = new EventEmitter();
    const running = main([f.bundles, '--notes', f.notes, '--out', f.out], { transport: t.send, stdout: out, isTTY: true, stdin, signals });
    await prompted(out);
    await previewChecked(f.out, out.text);
    if (keys === null) signals.emit('SIGINT');
    else stdin.write(keys);
    assert.equal(await running, want, 'the exit code');
    assert.equal(t.bodies.length, 0, 'no request');
    assert.match(out.text, /Nothing was sent/);
    assert.equal(signals.listenerCount('SIGINT'), 0, 'its SIGINT handler gone');
    assert.ok(!existsSync(join(f.out, '.lock')), 'the lock goes with the run');
    nothingTold(out.text);
    await rm(f.dir, { recursive: true });
  });
}

test('on a terminal, a typed yes sends: one request for the five scripts, the preview its exact bytes; each script its name, description and note, none of its lines; a script refused or with no lines not asked', async () => {
  const f = await fixture();
  const t = fakeTransport();
  const out = output();
  const stdin = terminal();
  const running = main([f.bundles, '--notes', f.notes, '--out', f.out], { transport: t.send, stdout: out, isTTY: true, stdin, signals: new EventEmitter() });
  await prompted(out);
  stdin.write('yes\r');
  assert.equal(await running, 3, 'written, with ages and classes not answered');
  assert.equal(t.bodies.length, 1);
  await previewChecked(f.out, out.text, t.bodies[0]);
  assert.deepEqual(userOf(t.bodies[0]), {
    characters: [
      { n: 0, name: `${SENTINEL}Kid`, description: 'a small one', note: `${NOTE}Kid is small` },
      { n: 1, name: `${SENTINEL}Old`, description: 'a grey wizard', note: `${NOTE}Old` },
      { n: 2, name: `${SENTINEL}Odd`, description: 'a guard of the gate', note: `${NOTE}Odd` },
      { n: 3, name: `${SENTINEL}Lost`, description: 'a merchant of bread' },
      { n: 4, name: `${SENTINEL}Shy`, description: 'a person of the town', note: `${NOTE}Shy` },
    ],
  }, 'the request: no line, no refused script, no script without lines');
  assert.ok(!t.bodies[0].includes(LINE), 'no line of a script is sent');
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

test('with --send: the answer gated to the enums (case and spaces aside); an age not answered is adult, a class not answered the script\'s Pass A class, each counted by reason; one file, u6.speakers/1, its coverage inside it, read by translate.mjs\'s reader, 0600; counts printed, nothing else', async () => {
  const f = await fixture();
  const t = fakeTransport();
  const r = await run([f.bundles, '--notes', f.notes, '--out', f.out, '--send'], t.send);
  assert.equal(r.code, 3, 'written, with ages and classes not answered');
  const text = await readFile(join(f.out, 'speakers.json'), 'utf8');
  const file = JSON.parse(text);
  const { coverage } = file;
  assert.deepEqual(Object.keys(file), ['format', 'model', 'coverage', 'speakers']);
  assert.deepEqual({ ...file, coverage: undefined }, {
    format: SPEAKERS_FORMAT, model: 'gpt-6-astra', coverage: undefined,
    speakers: [
      { archive: 'a', item: ITEMS[0], age: 'child', class: 'commoner', note: `${NOTE}Kid is small` },
      { archive: 'a', item: ITEMS[1], age: 'elder', class: 'mage', note: `${NOTE}Old` },
      { archive: 'a', item: ITEMS[2], age: 'adult', class: 'guard', note: `${NOTE}Odd` },
      { archive: 'b', item: ITEMS[3], age: 'adult', class: 'merchant', note: '' },
      { archive: 'b', item: ITEMS[4], age: 'youth', class: 'merchant', note: `${NOTE}Shy` },
    ],
  }, 'the answers taken; Odd (outside the enums) and Lost (left out) by Pass A as adults');
  assert.equal(readSpeakers(file).size, 5, 'translate.mjs reads it');
  assert.deepEqual(Object.keys(coverage), ['scripts', 'ages', 'classes', 'fallbacks', 'fallback']);
  assert.deepEqual(coverage.scripts, { read: 7, listed: 5, asked: 5, oversize: 0, noted: 4, taken: 3, notesUnused: 1 });
  assert.deepEqual(coverage.ages, { child: 1, youth: 1, adult: 2, elder: 1 });
  assert.deepEqual(coverage.classes, { royalty: 0, noble: 0, mage: 1, scholar: 0, clergy: 0, guard: 1, merchant: 2, commoner: 1, thief: 0, spirit: 0, gargoyle: 0 });
  assert.deepEqual(coverage.fallbacks, { age: { count: 2, byReason: { invalid: 1, missing: 1 } }, class: { count: 2, byReason: { invalid: 1, missing: 1 } } });
  assert.deepEqual(coverage.fallback, [
    { archive: 'a', item: ITEMS[2], age: 'invalid', class: 'invalid' }, { archive: 'b', item: ITEMS[3], age: 'missing', class: 'missing' },
  ]);
  assert.equal(await mode(join(f.out, 'speakers.json')), 0o600, 'speakers.json is 0600');
  assert.equal(await mode(f.out), 0o700);
  assert.deepEqual((await readdir(f.out)).sort(), ['preview', 'speakers.json'], 'one file: no coverage.json, no staged or part file, no lock');
  assert.match(r.text, new RegExp(`Wrote ${join(f.out, 'speakers.json').replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} \\(${Buffer.byteLength(text)} bytes; its coverage member holds the counts and the scripts not answered\\)\\.\n`));
  assert.match(r.text, /Speakers: 5 scripts, 3 given an age or a class by the answers; ages: child 1, youth 1, adult 2, elder 1\./);
  assert.match(r.text, /Classes: royalty 0, noble 0, mage 1, scholar 0, clergy 0, guard 1, merchant 2, commoner 1, thief 0, spirit 0, gargoyle 0\./);
  assert.match(r.text, /Not answered: age 2 \(invalid 1, missing 1\) \(taken as adult\), class 2 \(invalid 1, missing 1\) \(taken by Pass A\)\./);
  assert.match(r.text, /Sent: 1 requests \(0 asked again\) in \d+\.\d s; tokens: input 50, output 10 \(1 answers counted\)\./);
  nothingTold(r.text);
  // translate.mjs takes it: the child's request names its age, its note and a child's register.
  const ko = join(f.dir, 'ko');
  const tr = output();
  assert.equal(await translate([f.bundles, '--ages', 'child', '--speakers', join(f.out, 'speakers.json'), '--out', ko, '--dry-run'],
    { stdout: tr, isTTY: false, signals: new EventEmitter() }), 0);
  const preview = [...tr.text.matchAll(/The exact first request: (.+) \((\d+) bytes\)\./g)].at(-1)[1];
  const { npc } = userOf(await readFile(preview, 'utf8'));
  assert.deepEqual([npc.name, npc.class, npc.age, npc.profile, npc.register], [`${SENTINEL}Kid`, 'commoner', 'child', `${NOTE}Kid is small`, registerOf('commoner', 'child')]);
  await rm(f.dir, { recursive: true });
});

test('every answer as asked: exit 0; the requests in batches of at most BATCH_SCRIPTS scripts (and BATCH_CHARS characters), each numbered from 0, every script in the file once, in order', async () => {
  const many = Array.from({ length: 2 * BATCH_SCRIPTS + 5 }, (_, i) => bundle('a', 7200 + i, `${SENTINEL}Many${i}`));
  const f = await fixture(many, { format: NOTES_FORMAT, model: 'm', notes: [] });
  const t = fakeTransport();
  const r = await run([f.bundles, '--notes', f.notes, '--out', f.out, '--send', '--jobs', '2'], t.send);
  assert.equal(r.code, 0, 'every script answered');
  assert.deepEqual(t.bodies.map((b) => userOf(b).characters.length).sort((x, y) => y - x), [BATCH_SCRIPTS, BATCH_SCRIPTS, 5]);
  for (const b of t.bodies) assert.deepEqual(userOf(b).characters.map((c) => c.n), userOf(b).characters.map((_, i) => i));
  const file = JSON.parse(await readFile(join(f.out, 'speakers.json'), 'utf8'));
  assert.deepEqual(file.speakers.map((s) => s.item), many.map((b) => b.item));
  assert.ok(file.speakers.every((s) => s.age === 'adult' && s.class === 'guard' && s.note === ''));
  assert.match(r.text, /Not answered: none\./);
  nothingTold(r.text);
  assert.ok(!many.some((b) => new RegExp(`(^|[^0-9])${b.item}([^0-9]|$)`).test(r.text.replace(PREVIEW_PATH, ''))), 'no item printed');
  // The characters' budget: long descriptions and notes (cut to LOOK_MAX and SPEAKER_NOTE_MAX).
  const long = Array.from({ length: 12 }, (_, i) => bundle('a', 7300 + i, `${SENTINEL}Long${i}`, { look: 'x'.repeat(LOOK_MAX + 50) }));
  const notes = readNotes({ format: NOTES_FORMAT, notes: long.map((b) => ({ archive: 'a', item: b.item, note: 'y'.repeat(SPEAKER_NOTE_MAX + 50) })) });
  const plan = planOfSpeakers(long, notes, 'm');
  const text = (scripts) => contentOfBatch(scripts).length;
  assert.ok(plan.batches.length > 1 && plan.batches.every((b) => text(b.scripts) <= BATCH_CHARS && b.scripts.length < BATCH_SCRIPTS), 'split by characters');
  assert.ok(text([...plan.batches[0].scripts, plan.batches[1].scripts[0]]) > BATCH_CHARS - BATCH_SCRIPTS, 'a batch as full as the budget lets it');
  assert.ok(plan.batches.every((b) => b.scripts.every((s) => s.description.length === LOOK_MAX && s.note.length === SPEAKER_NOTE_MAX)));
  assert.equal(plan.batches.reduce((n, b) => n + b.scripts.length, 0), 12);
  await rm(f.dir, { recursive: true });
});

test('the batch bound: a name is cut to NAME_MAX; a script whose entry is still past SCRIPT_CHARS is not sent, it is adult and its Pass A class, counted (oversize); no request\'s text passes BATCH_CHARS, nor its body twice that past an empty one\'s', async () => {
  assert.ok(SCRIPT_CHARS + JSON.stringify({ characters: [] }).length <= BATCH_CHARS, 'one script always fits a request');
  const huge = bundle('a', 7400, `${SENTINEL}ManyHuge${'N'.repeat(5000)}`, { look: 'x'.repeat(5000) });
  const escaped = bundle('a', 7401, `${SENTINEL}Escaped`, { look: 'a guard of the gate' });
  const rest = Array.from({ length: 30 }, (_, i) => bundle('b', 7410 + i, `${SENTINEL}Many${i}`, { look: 'z'.repeat(LOOK_MAX) }));
  const notes = {
    format: NOTES_FORMAT, model: 'm',
    notes: [
      { archive: 'a', item: 7400, note: `${NOTE}${'q'.repeat(5000)}` },
      // Characters JSON escapes (six for each): cut to SPEAKER_NOTE_MAX, still past SCRIPT_CHARS.
      { archive: 'a', item: 7401, note: '\u0001'.repeat(SPEAKER_NOTE_MAX) },
      ...rest.map((b) => ({ archive: 'b', item: b.item, note: `${NOTE}${'w'.repeat(SPEAKER_NOTE_MAX)}` })),
    ],
  };
  const f = await fixture([huge, escaped, ...rest], notes);
  const t = fakeTransport();
  const r = await run([f.bundles, '--notes', f.notes, '--out', f.out, '--send', '--jobs', '1'], t.send);
  assert.equal(r.code, 3, 'written, the oversize one counted');
  assert.ok(t.bodies.length > 1, 'more than one request');
  const empty = bodyOf('gpt-6-astra', []).length;
  for (const body of t.bodies) {
    const content = JSON.parse(body).messages[0].content;
    assert.ok(content.length <= BATCH_CHARS, `a request's text is ${content.length} characters`);
    assert.ok(body.length <= empty + 2 * BATCH_CHARS, 'its body within the bound');
    assert.ok(JSON.parse(content).characters.length <= BATCH_SCRIPTS);
    assert.ok(!content.includes(`${SENTINEL}Escaped`), 'the oversize script is not sent');
  }
  const first = userOf(t.bodies[0]).characters[0];
  assert.equal(first.name.length, NAME_MAX, 'the name cut');
  assert.equal(first.name, `${SENTINEL}ManyHuge${'N'.repeat(NAME_MAX - `${SENTINEL}ManyHuge`.length)}`);
  assert.equal(first.description.length, LOOK_MAX);
  assert.equal(first.note.length, SPEAKER_NOTE_MAX);
  const file = JSON.parse(await readFile(join(f.out, 'speakers.json'), 'utf8'));
  const { coverage } = file;
  assert.equal(file.speakers.length, 32, 'every script in the file');
  assert.deepEqual(file.speakers.find((x) => x.item === 7401), { archive: 'a', item: 7401, age: 'adult', class: 'guard', note: '\u0001'.repeat(SPEAKER_NOTE_MAX) },
    'not sent: Pass A, an adult');
  assert.deepEqual(coverage.scripts, { read: 32, listed: 32, asked: 31, oversize: 1, noted: 32, taken: 31, notesUnused: 0 });
  assert.deepEqual(coverage.fallbacks, { age: { count: 1, byReason: { oversize: 1 } }, class: { count: 1, byReason: { oversize: 1 } } });
  assert.ok(file.speakers.filter((x) => x.item !== 7401).every((x) => x.age === 'adult' && x.class === 'guard'), 'the others as answered');
  assert.deepEqual(coverage.fallback.find((x) => x.item === 7401), { archive: 'a', item: 7401, age: 'oversize', class: 'oversize' });
  assert.match(r.text, /31 to ask \(32 with a note\), 1 too long to send \(taken by Pass A, as adults\);/);
  nothingTold(r.text);
  await rm(f.dir, { recursive: true });
});

test('a SIGINT once the file is staged: it is not renamed and the stage is removed, so the speakers.json there stays as it was; with no SIGINT the same run puts it in place, one file', async () => {
  const f = await fixture();
  await mkdir(f.out, { recursive: true });
  await chmod(f.out, 0o700);
  await writeFile(join(f.out, 'speakers.json'), 'the older speakers');
  const t = fakeTransport();
  const signals = new EventEmitter();
  const out = output();
  const stages = [];
  const onStaged = async (stage) => {
    stages.push(stage);
    assert.equal(dirname(stage), f.out, 'staged beside the file');
    assert.match(stage.split('/').at(-1), /^\.staged-[0-9]+-[0-9a-f]{12}-speakers\.json$/);
    assert.equal(JSON.parse(await readFile(stage, 'utf8')).format, SPEAKERS_FORMAT, 'the whole file staged');
    assert.equal(await mode(stage), 0o600);
    signals.emit('SIGINT');
  };
  const code = await main([f.bundles, '--notes', f.notes, '--out', f.out, '--send'], { transport: t.send, stdout: out, isTTY: false, signals, onStaged });
  assert.equal(code, 130);
  assert.equal(t.bodies.length, 1, 'the request was sent');
  assert.equal(stages.length, 1, 'one file staged');
  assert.match(out.text, /Interrupted: nothing was written\./);
  assert.ok(!out.text.includes('Wrote '));
  assert.equal(await readFile(join(f.out, 'speakers.json'), 'utf8'), 'the older speakers', 'speakers.json unchanged');
  assert.deepEqual((await readdir(f.out)).sort(), ['preview', 'speakers.json'], 'no staged, part or lock file left');
  assert.equal(signals.listenerCount('SIGINT'), 0, 'its SIGINT handler gone');
  nothingTold(out.text);
  const again = output();
  assert.equal(await main([f.bundles, '--notes', f.notes, '--out', f.out, '--send'], { transport: t.send, stdout: again, isTTY: false, signals: new EventEmitter() }), 3);
  const file = JSON.parse(await readFile(join(f.out, 'speakers.json'), 'utf8'));
  assert.equal(file.format, SPEAKERS_FORMAT);
  assert.equal(file.coverage.scripts.asked, 5, 'its coverage inside it');
  assert.deepEqual((await readdir(f.out)).sort(), ['preview', 'speakers.json'], 'one file, no coverage.json');
  assert.equal([...again.text.matchAll(/^Wrote /gm)].length, 1);
  assert.match(again.text, /Give it to translate\.mjs: --speakers .*speakers\.json\.\n/);
  nothingTold(again.text);
  await rm(f.dir, { recursive: true });
});

test('the gate: an answer is JSON of {speakers: [{n, age, class}]} (a code fence allowed); each age and class one of its enums, case and spaces aside, else adult or the Pass A class with its reason; the instruction names every age and class', () => {
  assert.equal(speakersOfAnswer('no'), null);
  assert.equal(speakersOfAnswer('{"speakers": []}'), null, 'no entry');
  assert.equal(speakersOfAnswer('{"lines": [{"n": 0}]}'), null);
  const a = speakersOfAnswer('```json\n{"speakers": [{"n": 0, "age": "child", "class": "thief"}, {"n": 0, "age": "elder"}, {"n": "1"}, 5]}\n```');
  assert.deepEqual([...a.entries], [[0, { age: 'child', class: 'thief' }]], 'the first entry for an n; others not of the form dropped');
  assert.deepEqual(gated({ age: 'child', class: 'thief' }, 'guard'), { age: 'child', class: 'thief', ageWhy: null, classWhy: null });
  assert.deepEqual(gated({ age: ' ELDER ', class: 'Noble' }, 'guard'), { age: 'elder', class: 'noble', ageWhy: null, classWhy: null });
  assert.deepEqual(gated({ age: 'teen', class: 'constructor' }, 'guard'), { age: 'adult', class: 'guard', ageWhy: 'invalid', classWhy: 'invalid' });
  assert.deepEqual(gated({ age: 3, class: null }, 'mage'), { age: 'adult', class: 'mage', ageWhy: 'invalid', classWhy: 'invalid' });
  assert.deepEqual(gated(undefined, 'mage'), { age: 'adult', class: 'mage', ageWhy: 'missing', classWhy: 'missing' });
  assert.deepEqual(gated(undefined, 'mage', 'http-500'), { age: 'adult', class: 'mage', ageWhy: 'http-500', classWhy: 'http-500' });
  for (const age of AGES) assert.ok(SYSTEM.includes(`"${age}"`), age);
  for (const c of Object.keys(CLASSES)) assert.ok(SYSTEM.includes(`  "${c}"`), c);
});

test('an answer that never reads (asked again once), a route that refuses and a SIGINT while sending write nothing', async () => {
  const f = await fixture();
  const prose = fakeTransport({ prose: true });
  const r = await run([f.bundles, '--notes', f.notes, '--out', f.out, '--send'], prose.send);
  assert.equal(r.code, 3);
  assert.equal(prose.bodies.length, 2, 'asked again once');
  assert.match(r.text, /Not answered: age 5 \(invalid 5\) .*class 5 \(invalid 5\)/);
  assert.match(r.text, /No answer gave an age or a class: nothing was written\./);
  assert.ok(!existsSync(join(f.out, 'speakers.json')));
  nothingTold(r.text);
  const calls = [];
  const refusing = await run([f.bundles, '--notes', f.notes, '--out', f.out, '--send'], async (body) => {
    calls.push(body);
    return { status: 403, json: null };
  });
  assert.equal(refusing.code, 3);
  assert.equal(calls.length, 1, 'not asked again');
  assert.match(refusing.text, /The route refused a request \(http-403\): no request was sent after it\./);
  assert.ok(!existsSync(join(f.out, 'speakers.json')));
  const signals = new EventEmitter();
  const out = output();
  const hanging = (body, { signal }) => new Promise((_, fail) => {
    signal.addEventListener('abort', () => fail(signal.reason));
    setTimeout(() => signals.emit('SIGINT'), 5);
  });
  assert.equal(await main([f.bundles, '--notes', f.notes, '--out', f.out, '--send'], { transport: hanging, stdout: out, isTTY: false, signals }), 130);
  assert.match(out.text, /Interrupted: nothing was written\./);
  assert.ok(!existsSync(join(f.out, 'speakers.json')) && !existsSync(join(f.out, '.lock')));
  assert.equal(signals.listenerCount('SIGINT'), 0);
  nothingTold(refusing.text + out.text);
  await rm(f.dir, { recursive: true });
});

test('refused before any request, with a fixed message: the notes file missing, not JSON, not u6.voice-notes/1 or giving a script twice; the bundles not a list of bundles; no --notes; another run\'s lock; a symbolic link where the output directory goes', async () => {
  const never = async () => assert.fail('no request');
  const shape = /speakers: the notes file is not u6\.voice-notes\/1$/m;
  for (const [what, notes, rule] of [
    ['not JSON', '{', /speakers: the notes file does not read as JSON/],
    ['another format', { ...NOTES, format: 'u6.speakers/1' }, shape],
    ['notes not a list', { format: NOTES_FORMAT, notes: {} }, shape],
    ['an entry not of the form', { format: NOTES_FORMAT, notes: [{ archive: 'c', item: 1, note: NOTE }, { archive: 'a', item: 1, note: 5 }] },
      /speakers: the notes file is not u6\.voice-notes\/1: 2 entries are not \{archive, item, note\}/],
    ['a script twice', { format: NOTES_FORMAT, notes: [{ archive: 'a', item: 1, note: NOTE }, { archive: 'a', item: 1, note: NOTE }] },
      /speakers: the notes file is not u6\.voice-notes\/1: 1 entries give a script given before/],
  ]) {
    const f = await fixture(BUNDLES, notes);
    const r = await run([f.bundles, '--notes', f.notes, '--out', f.out, '--send'], never);
    assert.equal(r.code, 2, `${what}: refused`);
    assert.match(r.text, rule, `${what}: its message`);
    assert.ok(!existsSync(f.out), `${what}: before the output directory`);
    nothingTold(r.text);
    await rm(f.dir, { recursive: true });
  }
  assert.throws(() => readNotes(null), InputError);
  const f = await fixture();
  for (const [args, rule] of [
    [[f.bundles, '--notes', join(f.dir, 'none.json')], /the notes file cannot be read \(ENOENT\)/],
    [[f.bundles], /no notes file named/],
    [[f.notes, '--notes', f.notes], /the file is not a list of NPC bundles/],
    [[f.bundles, '--notes', f.notes, '--jobs', '9'], /--jobs is 1 to 4/],
    [[f.bundles, '--notes', f.notes, '--llmux', 'http://192.0.2.1:3456'], /--llmux is the address of llmux on this machine/],
  ]) {
    const r = await run([...args, '--out', f.out, '--send'], never);
    assert.equal(r.code, 2, `${args}: refused`);
    assert.match(r.text, rule);
  }
  await mkdir(f.out, { recursive: true });
  await writeFile(join(f.out, '.lock'), `${process.pid}\n`);
  const locked = await run([f.bundles, '--notes', f.notes, '--out', f.out, '--send'], never);
  assert.equal(locked.code, 2);
  assert.match(locked.text, /another run of a tool holds .*\.lock .*nothing was sent\./);
  assert.equal(previewPrinted(locked.text), null, 'no preview');
  await rm(f.out, { recursive: true });
  const target = join(f.dir, 'elsewhere');
  await mkdir(target);
  await symlink(target, f.out);
  const linked = await run([f.bundles, '--notes', f.notes, '--out', f.out, '--send'], never);
  assert.equal(linked.code, 2);
  assert.ok(linked.text.includes('speakers: the output directory is a symbolic link or not a directory: nothing was sent.'));
  assert.deepEqual(await readdir(target), [], 'the link\'s target untouched');
  nothingTold(locked.text + linked.text);
  await rm(f.dir, { recursive: true });
});

// --- the CLI in a child process, against a counting HTTP server -------------------------------

// A local HTTP server that counts requests and answers each as `answerFor` says ({status, json,
// headers}); `url` once listening.
async function server(answerFor) {
  const seen = [];
  const s = createServer((req, res) => {
    let body = '';
    req.on('data', (d) => { body += d; });
    req.on('end', () => {
      seen.push({ method: req.method, path: req.url, body });
      const a = answerFor(body);
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

test('the CLI: no terminal and no --send reaches no server; --dry-run neither; --send does, on POST /v1/messages, and writes the file; a 307 or 308 to another server is not followed', async () => {
  const f = await fixture();
  const answering = await server((body) => {
    const speakers = userOf(body).characters.map((c) => ({ n: c.n, age: 'adult', class: 'commoner' }));
    return { status: 200, json: { type: 'message', content: [{ type: 'text', text: JSON.stringify({ speakers }) }] } };
  });
  try {
    const none = await cli([f.bundles, '--notes', f.notes, '--out', f.out, '--llmux', answering.url]);
    assert.equal(none.code, 1);
    const dry = await cli([f.bundles, '--notes', f.notes, '--out', f.out, '--llmux', answering.url, '--dry-run']);
    assert.equal(dry.code, 0);
    assert.equal(answering.seen.length, 0, 'no request without --send, nor on a dry run');
    const sent = await cli([f.bundles, '--notes', f.notes, '--out', f.out, '--llmux', answering.url, '--send']);
    assert.equal(sent.code, 0, 'sent and written');
    assert.deepEqual(answering.seen.map((r) => [r.method, r.path]), [['POST', '/v1/messages']], 'one Messages call');
    assert.equal(answering.seen[0].body, await readFile(previewPrinted(sent.text), 'utf8'), 'the preview is what was sent');
    assert.equal(readSpeakers(JSON.parse(await readFile(join(f.out, 'speakers.json'), 'utf8'))).size, 5);
    nothingTold(none.text + dry.text + sent.text);
  } finally {
    await answering.close();
  }
  for (const status of [307, 308]) {
    const other = await server(() => ({ status: 200, json: {} }));
    const redirecting = await server(() => ({ status, headers: { location: `${other.url}/v1/messages` } }));
    const out = join(f.dir, `r${status}`);
    try {
      const r = await cli([f.bundles, '--notes', f.notes, '--out', out, '--llmux', redirecting.url, '--send']);
      assert.equal(other.seen.length, 0, `${status}: the other server got nothing`);
      assert.equal(redirecting.seen.length, 1, `${status}: one request, none after the refusal`);
      assert.equal(r.code, 3);
      assert.match(r.text, new RegExp(`The route refused a request \\(http-${status}\\): no request was sent after it\\.`));
      assert.ok(!existsSync(join(out, 'speakers.json')), `${status}: nothing written`);
      nothingTold(r.text);
    } finally {
      await redirecting.close();
      await other.close();
    }
  }
  await rm(f.dir, { recursive: true });
});

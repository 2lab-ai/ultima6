// Tests of tools/i18n/speakers.mjs (spec 12 Design 1, spec 15 Design 1, spec 08's upload
// boundary) on synthetic bundles and notes the test writes (no game data, made-up names, notes and
// Korean names), with a counting fake transport, in a directory of its own (never ~/Games/ultima6):
//
// - zero requests: no terminal and no --send, --dry-run (with --send too), and on a terminal a
//   refusal, Ctrl-C at the prompt and SIGINT; the preview is the exact first payload, 0600 in a
//   0700 directory, under a name of the run's own; a typed yes sends;
// - with --send: the scripts translate.mjs can translate asked in batches (at most BATCH_SCRIPTS
//   scripts and BATCH_CHARS characters of text; a name cut to NAME_MAX, a greeting cleaned and cut
//   to GREETING_MAX, a script still past SCRIPT_CHARS not sent, every field's default, counted),
//   each with its note and its greeting's first lines, none of its other lines; the answer gated
//   to the enums (case and spaces aside) and name_ko to a name's Korean form, a field not answered
//   its default (adult, the Pass A class, none, its class's station, plain, null), each counted
//   by field and reason; the counts printed by age, sex, station and manner; the file
//   u6.speakers/2, read by translate.mjs's reader and taken by translate.mjs --speakers (the
//   station, manner and address in its request), its coverage receipt inside it; one file, 0600;
//   the instruction naming every value of every enum;
// - an answer that never reads, a route that refuses and a SIGINT while sending write nothing; a
//   SIGINT once the file is staged leaves the one there as it was (not renamed, the stage removed);
// - the notes reader's refusals, a bundles file that is not one, a lock another run holds and a
//   symbolic link where the output directory goes: refused before any request;
// - the CLI itself in a child process against a counting HTTP server (the real transport): no
//   terminal and no --send sends nothing, --dry-run neither, --send does and writes the file, a 307
//   or 308 to another server is not followed;
// - nothing the tool prints holds a name, a Korean name, a note, a line, a script's item or a hash.
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
  BATCH_CHARS, BATCH_SCRIPTS, FIELDS, GREETING_MAX, LOOK_MAX, MAX_TOKENS, NAME_MAX, NOTES_FORMAT, SCRIPT_CHARS, SYSTEM, bodyOf, contentOfBatch,
  gated, main, outputsOf, planOfSpeakers, readNotes, speakersOfAnswer, stationOfClass,
} from './speakers.mjs';
import {
  AGES, CLASSES, InputError, MANNERS, SEXES, SPEAKERS_FORMAT_2, SPEAKER_NOTE_MAX, STATIONS, addressOf, main as translate, readSpeakers,
  registerOf,
} from './translate.mjs';

const TOOL = join(dirname(fileURLToPath(import.meta.url)), 'speakers.mjs');
const SENTINEL = 'Zq7SpeakerSentinel';
const NOTE = 'Zq7NoteSentinel';
const LINE = 'Zq7LineSentinel';
const GREETING = 'Zq7GreetingSentinel';
const ITEMS = [7101, 7102, 7103, 7104, 7105, 7106, 7107];
const hashOf = (item) => `00000000000c${item}`;

const seg = (archive, item, offset, text) => ({ kind: 'text', key: [archive, item, offset], text });

// A u6.npc-bundle/1 as the page's exporter writes it, with the test's own name, look, greeting and
// an answer's line (no text at all with `lines: false`: the look text is a run of the script too).
function bundle(archive, item, name, { look = 'a person of the town', refused = null, lines = true, greeting = `"Hail. ${GREETING}"` } = {}) {
  const text = lines && !refused;
  return {
    format: 'u6.npc-bundle/1', archive, item, hash: hashOf(item), refused, actors: [5], portraits: [], name, locale: 'en',
    narration: { look: lines ? [seg(archive, item, 20, look)] : [] },
    speech: { greeting: text && greeting ? [seg(archive, item, 60, greeting)] : [] },
    turns: text ? [{ input: { at: 90, kind: 'ask' }, segments: [], topics: [{ at: 0, keywords: ['*'], requires: [], answer: [seg(archive, item, 100, `Naught. ${LINE}`)] }] }] : [],
    traits: [], strings: [],
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

// What the fake model answers for a character named ...`who` (made-up Korean names): Old leaves
// its manner out, Odd answers every field outside its form, Shy in another case and with spaces.
const ANSWERS = {
  Kid: { age: 'child', class: 'commoner', sex: 'male', station: 'commoner', manner: 'timid', name_ko: '가나 다' },
  Old: { age: 'elder', class: 'mage', sex: 'female', station: 'lord', name_ko: null },
  Odd: { age: 'teen', class: 'emperor', sex: 'other', station: 'noble', manner: 'merry', name_ko: 'Zed' },
  Shy: { age: ' Youth ', class: 'MERCHANT', sex: ' Female ', station: 'LOWBORN', manner: ' Sly ', name_ko: ' 다라 ' },
};
// Every field as asked: a guard of the town, no name of its own.
const MANY = { age: 'adult', class: 'guard', sex: 'male', station: 'commoner', manner: 'plain', name_ko: null };

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
      const a = ANSWERS[c.name.replace(SENTINEL, '')] ?? (c.name.includes('Many') ? MANY : null);
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
  assert.ok(!printed.includes(SENTINEL) && !printed.includes(NOTE) && !printed.includes(LINE) && !printed.includes(GREETING), 'no name, note or line printed');
  assert.ok(!/[가-힣]/.test(printed), 'no Korean name printed');
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

test('on a terminal, a typed yes sends: one request for the five scripts, the preview its exact bytes; each script its name, description, note and greeting, none of its other lines; a script refused or with no lines not asked', async () => {
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
      { n: 0, name: `${SENTINEL}Kid`, description: 'a small one', note: `${NOTE}Kid is small`, greeting: `"Hail. ${GREETING}"` },
      { n: 1, name: `${SENTINEL}Old`, description: 'a grey wizard', note: `${NOTE}Old`, greeting: `"Hail. ${GREETING}"` },
      { n: 2, name: `${SENTINEL}Odd`, description: 'a guard of the gate', note: `${NOTE}Odd`, greeting: `"Hail. ${GREETING}"` },
      { n: 3, name: `${SENTINEL}Lost`, description: 'a merchant of bread', greeting: `"Hail. ${GREETING}"` },
      { n: 4, name: `${SENTINEL}Shy`, description: 'a person of the town', note: `${NOTE}Shy`, greeting: `"Hail. ${GREETING}"` },
    ],
  }, 'the request: the greetings, no other line, no refused script, no script without lines');
  assert.ok(!t.bodies[0].includes(LINE), 'no answer\'s line of a script is sent');
  nothingTold(out.text);
  await rm(f.dir, { recursive: true });
});

test('with --send: the answer gated to the enums (case and spaces aside) and name_ko to a name\'s form; a field not answered takes its default (adult, the Pass A class, none, its class\'s station, plain, null), each counted by field and reason; one file, u6.speakers/2, its coverage inside it, read by translate.mjs\'s reader, 0600; counts printed by age, sex, station and manner, nothing else', async () => {
  const f = await fixture();
  const t = fakeTransport();
  const r = await run([f.bundles, '--notes', f.notes, '--out', f.out, '--send'], t.send);
  assert.equal(r.code, 3, 'written, with ages and classes not answered');
  const text = await readFile(join(f.out, 'speakers.json'), 'utf8');
  const file = JSON.parse(text);
  const { coverage } = file;
  assert.deepEqual(Object.keys(file), ['format', 'model', 'coverage', 'speakers']);
  const none = { sex: 'none', station: 'commoner', manner: 'plain', name_ko: null };
  assert.deepEqual({ ...file, coverage: undefined }, {
    format: SPEAKERS_FORMAT_2, model: 'gpt-6-astra', coverage: undefined,
    speakers: [
      { archive: 'a', item: ITEMS[0], age: 'child', class: 'commoner', note: `${NOTE}Kid is small`, sex: 'male', station: 'commoner', manner: 'timid', name_ko: '가나 다' },
      { archive: 'a', item: ITEMS[1], age: 'elder', class: 'mage', note: `${NOTE}Old`, sex: 'female', station: 'lord', manner: 'plain', name_ko: null },
      { archive: 'a', item: ITEMS[2], age: 'adult', class: 'guard', note: `${NOTE}Odd`, ...none },
      { archive: 'b', item: ITEMS[3], age: 'adult', class: 'merchant', note: '', ...none },
      { archive: 'b', item: ITEMS[4], age: 'youth', class: 'merchant', note: `${NOTE}Shy`, sex: 'female', station: 'lowborn', manner: 'sly', name_ko: '다라' },
    ],
  }, 'the answers taken; Old\'s manner (left out) plain; Odd (outside the forms) and Lost (left out) the defaults, Pass A as adults');
  for (const s of file.speakers) {
    assert.deepEqual(Object.keys(s), ['archive', 'item', 'age', 'class', 'note', 'sex', 'station', 'manner', 'name_ko'], 'each entry in the file\'s order');
  }
  const read = readSpeakers(file);
  assert.equal(read.size, 5, 'translate.mjs reads it');
  assert.deepEqual(read.get(`b:${ITEMS[4]}`), { age: 'youth', class: 'merchant', note: `${NOTE}Shy`, sex: 'female', station: 'lowborn', manner: 'sly', nameKo: '다라' });
  assert.deepEqual(Object.keys(coverage), ['scripts', 'ages', 'classes', 'sexes', 'stations', 'manners', 'named', 'fallbacks', 'fallback']);
  assert.deepEqual(coverage.scripts, { read: 7, listed: 5, asked: 5, oversize: 0, noted: 4, taken: 3, notesUnused: 1 });
  assert.deepEqual(coverage.ages, { child: 1, youth: 1, adult: 2, elder: 1 });
  assert.deepEqual(coverage.classes, { royalty: 0, noble: 0, mage: 1, scholar: 0, clergy: 0, guard: 1, merchant: 2, commoner: 1, thief: 0, spirit: 0, gargoyle: 0 });
  assert.deepEqual(coverage.sexes, { female: 2, male: 1, none: 2 });
  assert.deepEqual(coverage.stations, { royalty: 0, lord: 1, commoner: 3, lowborn: 1, beyond: 0 });
  assert.deepEqual(coverage.manners, { plain: 3, jester: 0, mad: 0, pompous: 0, drunk: 0, sly: 1, gruff: 0, timid: 1, pious: 0 });
  assert.equal(coverage.named, 2);
  const two = { count: 2, byReason: { invalid: 1, missing: 1 } };
  assert.deepEqual(coverage.fallbacks, { age: two, class: two, sex: two, station: two, manner: { count: 3, byReason: { invalid: 1, missing: 2 } }, name_ko: two });
  const all = (why) => Object.fromEntries(FIELDS.map((x) => [x, why]));
  assert.deepEqual(coverage.fallback, [
    { archive: 'a', item: ITEMS[1], ...all(null), manner: 'missing' },
    { archive: 'a', item: ITEMS[2], ...all('invalid') }, { archive: 'b', item: ITEMS[3], ...all('missing') },
  ]);
  assert.equal(await mode(join(f.out, 'speakers.json')), 0o600, 'speakers.json is 0600');
  assert.equal(await mode(f.out), 0o700);
  assert.deepEqual((await readdir(f.out)).sort(), ['preview', 'speakers.json'], 'one file: no coverage.json, no staged or part file, no lock');
  assert.match(r.text, new RegExp(`Wrote ${join(f.out, 'speakers.json').replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} \\(${Buffer.byteLength(text)} bytes; its coverage member holds the counts and the scripts not answered\\)\\.\n`));
  assert.match(r.text, /Speakers: 5 scripts, 3 given a field by the answers; ages: child 1, youth 1, adult 2, elder 1\./);
  assert.match(r.text, /Sexes: female 2, male 1, none 2\. Stations: royalty 0, lord 1, commoner 3, lowborn 1, beyond 0\. Manners: plain 3, jester 0, mad 0, pompous 0, drunk 0, sly 1, gruff 0, timid 1, pious 0\.\n/);
  assert.match(r.text, new RegExp('Not answered: age 2 \\(invalid 1, missing 1\\) \\(taken as adult\\), class 2 \\(invalid 1, missing 1\\) \\(taken by Pass A\\), '
    + 'sex 2 \\(invalid 1, missing 1\\) \\(taken as none\\), station 2 \\(invalid 1, missing 1\\) \\(taken by its class\\), '
    + 'manner 3 \\(invalid 1, missing 2\\) \\(taken as plain\\), name_ko 2 \\(invalid 1, missing 1\\) \\(taken as null\\)\\.'));
  assert.ok(!/Classes:|merchant [0-9]/.test(r.text), 'no count by class printed');
  assert.match(r.text, /Sent: 1 requests \(0 asked again\) in \d+\.\d s; tokens: input 50, output 10 \(1 answers counted\)\./);
  nothingTold(r.text);
  // translate.mjs takes it: the child's request names its age, its note, its station, its manner,
  // a child's register with its manner and a child's words for the hero.
  const ko = join(f.dir, 'ko');
  const tr = output();
  assert.equal(await translate([f.bundles, '--ages', 'child', '--speakers', join(f.out, 'speakers.json'), '--out', ko, '--dry-run'],
    { stdout: tr, isTTY: false, signals: new EventEmitter() }), 0);
  const preview = [...tr.text.matchAll(/The exact first request: (.+) \((\d+) bytes\)\./g)].at(-1)[1];
  const { npc } = userOf(await readFile(preview, 'utf8'));
  assert.deepEqual([npc.name, npc.class, npc.age, npc.profile, npc.station, npc.manner, npc.register, npc.address], [
    `${SENTINEL}Kid`, 'commoner', 'child', `${NOTE}Kid is small`, 'commoner', 'timid', registerOf('commoner', 'child', 'commoner', 'timid'),
    addressOf('commoner', 'child'),
  ]);
  nothingTold(tr.text);
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
  assert.ok(file.speakers.every((s) => s.age === 'adult' && s.class === 'guard' && s.note === '' && s.sex === 'male' && s.station === 'commoner'
    && s.manner === 'plain' && s.name_ko === null));
  assert.match(r.text, /Not answered: none\./);
  nothingTold(r.text);
  assert.ok(!many.some((b) => new RegExp(`(^|[^0-9])${b.item}([^0-9]|$)`).test(r.text.replace(PREVIEW_PATH, ''))), 'no item printed');
  // The characters' budget: long descriptions, notes and greetings (cut to LOOK_MAX,
  // SPEAKER_NOTE_MAX and GREETING_MAX).
  const long = Array.from({ length: 12 }, (_, i) => bundle('a', 7300 + i, `${SENTINEL}Long${i}`, { look: 'x'.repeat(LOOK_MAX + 50), greeting: 'g'.repeat(GREETING_MAX + 50) }));
  const notes = readNotes({ format: NOTES_FORMAT, notes: long.map((b) => ({ archive: 'a', item: b.item, note: 'y'.repeat(SPEAKER_NOTE_MAX + 50) })) });
  const plan = planOfSpeakers(long, notes, 'm');
  const text = (scripts) => contentOfBatch(scripts).length;
  assert.ok(plan.batches.length > 1 && plan.batches.every((b) => text(b.scripts) <= BATCH_CHARS && b.scripts.length < BATCH_SCRIPTS), 'split by characters');
  assert.ok(text([...plan.batches[0].scripts, plan.batches[1].scripts[0]]) > BATCH_CHARS - BATCH_SCRIPTS, 'a batch as full as the budget lets it');
  assert.ok(plan.batches.every((b) => b.scripts.every((s) => s.description.length === LOOK_MAX && s.note.length === SPEAKER_NOTE_MAX
    && s.greeting.length === GREETING_MAX && s.character.greeting === s.greeting)));
  assert.equal(plan.batches.reduce((n, b) => n + b.scripts.length, 0), 12);
  await rm(f.dir, { recursive: true });
});

test('the batch bound: a name is cut to NAME_MAX; a script whose entry is still past SCRIPT_CHARS is not sent, it takes every field\'s default (adult, its Pass A class, none, its class\'s station, plain, null), counted (oversize); no request\'s text passes BATCH_CHARS, nor its body twice that past an empty one\'s', async () => {
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
  assert.deepEqual(file.speakers.find((x) => x.item === 7401), {
    archive: 'a', item: 7401, age: 'adult', class: 'guard', note: '\u0001'.repeat(SPEAKER_NOTE_MAX), sex: 'none', station: 'commoner', manner: 'plain', name_ko: null,
  }, 'not sent: Pass A, an adult, every default');
  assert.deepEqual(coverage.scripts, { read: 32, listed: 32, asked: 31, oversize: 1, noted: 32, taken: 31, notesUnused: 0 });
  assert.deepEqual(coverage.fallbacks, Object.fromEntries(FIELDS.map((x) => [x, { count: 1, byReason: { oversize: 1 } }])));
  assert.ok(file.speakers.filter((x) => x.item !== 7401).every((x) => x.age === 'adult' && x.class === 'guard'), 'the others as answered');
  assert.deepEqual(coverage.fallback.find((x) => x.item === 7401), { archive: 'a', item: 7401, ...Object.fromEntries(FIELDS.map((x) => [x, 'oversize'])) });
  assert.match(r.text, /31 to ask \(32 with a note\), 1 too long to send \(each by Pass A, an adult of its class's station, sex none, manner plain, no Korean name\);/);
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
    assert.equal(JSON.parse(await readFile(stage, 'utf8')).format, SPEAKERS_FORMAT_2, 'the whole file staged');
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
  assert.equal(file.format, SPEAKERS_FORMAT_2);
  assert.equal(file.coverage.scripts.asked, 5, 'its coverage inside it');
  assert.deepEqual((await readdir(f.out)).sort(), ['preview', 'speakers.json'], 'one file, no coverage.json');
  assert.equal([...again.text.matchAll(/^Wrote /gm)].length, 1);
  assert.match(again.text, /Give it to translate\.mjs: --speakers .*speakers\.json\.\n/);
  nothingTold(again.text);
  await rm(f.dir, { recursive: true });
});

test('the gate: an answer is JSON of {speakers: [{n, age, class, sex, station, manner, name_ko}]} (a code fence allowed); each age and class one of its enums, case and spaces aside, else adult or the Pass A class with its reason; the instruction names every age and class', () => {
  assert.equal(speakersOfAnswer('no'), null);
  assert.equal(speakersOfAnswer('{"speakers": []}'), null, 'no entry');
  assert.equal(speakersOfAnswer('{"lines": [{"n": 0}]}'), null);
  const a = speakersOfAnswer('```json\n{"speakers": [{"n": 0, "age": "child", "class": "thief", "extra": 1}, {"n": 0, "age": "elder"}, {"n": "1"}, 5]}\n```');
  assert.deepEqual([...a.entries], [[0, { age: 'child', class: 'thief', sex: undefined, station: undefined, manner: undefined, name_ko: undefined }]],
    'the first entry for an n, its fields only; others not of the form dropped');
  const rest = { sex: 'male', station: 'lord', manner: 'gruff', name_ko: null };
  const taken = { sex: null, station: null, manner: null, name_ko: null };
  assert.deepEqual(gated({ age: 'child', class: 'thief', ...rest }, 'guard'), { age: 'child', class: 'thief', ...rest, why: { age: null, class: null, ...taken } });
  assert.deepEqual(gated({ age: ' ELDER ', class: 'Noble', ...rest }, 'guard'), { age: 'elder', class: 'noble', ...rest, why: { age: null, class: null, ...taken } });
  const g = (entry, passA, failed) => {
    const x = gated(entry, passA, failed);
    return { age: x.age, class: x.class, ageWhy: x.why.age, classWhy: x.why.class };
  };
  assert.deepEqual(g({ age: 'teen', class: 'constructor' }, 'guard'), { age: 'adult', class: 'guard', ageWhy: 'invalid', classWhy: 'invalid' });
  assert.deepEqual(g({ age: 3, class: null }, 'mage'), { age: 'adult', class: 'mage', ageWhy: 'invalid', classWhy: 'invalid' });
  assert.deepEqual(g({ age: 'adult', class: '__proto__' }, 'mage'), { age: 'adult', class: 'mage', ageWhy: null, classWhy: 'invalid' });
  assert.deepEqual(g(undefined, 'mage'), { age: 'adult', class: 'mage', ageWhy: 'missing', classWhy: 'missing' });
  assert.deepEqual(g(undefined, 'mage', 'http-500'), { age: 'adult', class: 'mage', ageWhy: 'http-500', classWhy: 'http-500' });
  for (const age of AGES) assert.ok(SYSTEM.includes(`"${age}"`), age);
  for (const c of Object.keys(CLASSES)) assert.ok(SYSTEM.includes(`  "${c}"`), c);
});

test('spec 15: the gate takes each field to its form, case and spaces aside: a sex, a station or a manner outside its enum (or missing) is none, commoner or plain, a name_ko that is not null nor 1 to 8 Hangul syllables with one space between two words is null, each with its reason; a failed request every field\'s default by its reason', () => {
  for (const sex of SEXES) for (const station of STATIONS) for (const manner of MANNERS) {
    const x = gated({ age: 'adult', class: 'guard', sex: ` ${sex.toUpperCase()} `, station: ` ${station} `, manner: manner.toUpperCase(), name_ko: null }, 'mage');
    assert.deepEqual([x.sex, x.station, x.manner, x.why.sex, x.why.station, x.why.manner], [sex, station, manner, null, null, null], `${sex} ${station} ${manner}`);
  }
  const base = { age: 'adult', class: 'guard', sex: 'female', station: 'beyond', manner: 'pious', name_ko: '가나' };
  for (const [field, value, want, why] of [
    ['sex', 'other', 'none', 'invalid'], ['sex', undefined, 'none', 'missing'], ['sex', null, 'none', 'invalid'], ['sex', 1, 'none', 'invalid'],
    ['station', 'noble', 'commoner', 'invalid'], ['station', '__proto__', 'commoner', 'invalid'], ['station', undefined, 'commoner', 'missing'],
    ['manner', 'merry', 'plain', 'invalid'], ['manner', ['mad'], 'plain', 'invalid'], ['manner', undefined, 'plain', 'missing'],
    ['name_ko', null, null, null], ['name_ko', '가나다라마바사아', '가나다라마바사아', null], ['name_ko', '가나 다라', '가나 다라', null],
    ['name_ko', '  가나 다  ', '가나 다', null],
    ['name_ko', '가나다라마바사아자', null, 'invalid'], ['name_ko', '가나  다', null, 'invalid'], ['name_ko', 'Zed', null, 'invalid'],
    ['name_ko', '가나Zed', null, 'invalid'], ['name_ko', '', null, 'invalid'], ['name_ko', '가ㄴ', null, 'invalid'], ['name_ko', 5, null, 'invalid'],
    ['name_ko', undefined, null, 'missing'], ['name_ko', ['가나'], null, 'invalid'], ['name_ko', '가나\n', null, 'invalid'],
    ['name_ko', '가나\u2028', null, 'invalid'], ['name_ko', '\u00a0가나', null, 'invalid'], ['name_ko', '가나\t', null, 'invalid'],
    // A one-element array holding a valid value is no value (no key coercion).
    ['age', ['adult'], 'adult', 'invalid'], ['class', ['merchant'], 'mage', 'invalid'], ['sex', ['female'], 'none', 'invalid'],
    ['station', ['lord'], 'commoner', 'invalid'], ['manner', ['sly'], 'plain', 'invalid'],
  ]) {
    const entry = { ...base, [field]: value };
    if (value === undefined) delete entry[field];
    const x = gated(entry, 'mage');
    assert.equal(x[field], want, `${field} ${JSON.stringify(value)}: its value`);
    assert.equal(x.why[field], why, `${field} ${JSON.stringify(value)}: its reason`);
    for (const other of FIELDS.filter((f) => f !== field)) assert.equal(x.why[other], null, `${field} ${JSON.stringify(value)}: ${other} taken`);
  }
  assert.deepEqual(gated(undefined, 'mage', 'timeout'), {
    age: 'adult', class: 'mage', sex: 'none', station: 'commoner', manner: 'plain', name_ko: null,
    why: Object.fromEntries(FIELDS.map((f) => [f, 'timeout'])),
  });
  // The written file's counts: a field taken counts the script taken; a script none answered does
  // not.
  const plan = { read: 2, scripts: 2, asked: 2, oversize: 0, noted: 0, unused: 0, list: [
    { archive: 'a', item: 1, cls: 'mage', note: '', batch: 0, n: 0 }, { archive: 'a', item: 2, cls: 'guard', note: '', batch: 0, n: 1 },
  ] };
  const { file, taken } = outputsOf(plan, [{ ok: true, entries: new Map([[0, { manner: 'jester' }]]) }], 'm');
  assert.equal(taken, 1);
  assert.deepEqual(file.speakers.map((x) => [x.age, x.class, x.sex, x.station, x.manner, x.name_ko]), [
    ['adult', 'mage', 'none', 'commoner', 'jester', null], ['adult', 'guard', 'none', 'commoner', 'plain', null],
  ]);
  assert.deepEqual(file.coverage.fallbacks.manner, { count: 1, byReason: { missing: 1 } });
  assert.deepEqual(file.coverage.fallbacks.age, { count: 2, byReason: { missing: 2 } });
  assert.equal(readSpeakers(JSON.parse(JSON.stringify(file))).size, 2, 'it reads back as /2');
});

test('spec 15 (round 3): a station not answered takes the station of the class read for the script (the answer\'s class, else its Pass A class): royalty the crown\'s, noble lord, spirit beyond, thief lowborn, any other commoner; counted by reason as before', () => {
  const want = { royalty: 'royalty', noble: 'lord', spirit: 'beyond', thief: 'lowborn' };
  for (const cls of Object.keys(CLASSES)) {
    const station = want[cls] ?? 'commoner';
    assert.equal(stationOfClass(cls), station, cls);
    const given = gated({ age: 'adult', class: cls, sex: 'male', manner: 'plain', name_ko: null }, 'guard');
    assert.deepEqual([given.station, given.why.station], [station, 'missing'], `${cls}: answered, no station`);
    const passA = gated({ age: 'adult', class: 'emperor', sex: 'male', station: 'noble', manner: 'plain', name_ko: null }, cls);
    assert.deepEqual([passA.class, passA.station, passA.why.station], [cls, station, 'invalid'], `${cls}: by Pass A, a station outside the enum`);
    const failed = gated(undefined, cls, 'timeout');
    assert.deepEqual([failed.station, failed.why.station], [station, 'timeout'], `${cls}: a failed request`);
  }
  assert.equal(gated({ class: 'thief', station: 'lord' }, 'noble').station, 'lord', 'an answered station wins');
  assert.equal(stationOfClass('__proto__'), 'commoner');
  // In the file: an oversize script of a thief's Pass A class is lowborn, counted oversize.
  const plan = { read: 2, scripts: 2, asked: 1, oversize: 1, noted: 0, unused: 0, list: [
    { archive: 'a', item: 1, cls: 'thief', note: '', oversize: true }, { archive: 'a', item: 2, cls: 'guard', note: '', batch: 0, n: 0 },
  ] };
  const { file } = outputsOf(plan, [{ ok: true, entries: new Map([[0, { class: 'spirit' }]]) }], 'm');
  assert.deepEqual(file.speakers.map((x) => [x.class, x.station]), [['thief', 'lowborn'], ['spirit', 'beyond']]);
  assert.deepEqual(file.coverage.fallbacks.station, { count: 2, byReason: { missing: 1, oversize: 1 } });
  assert.deepEqual(file.coverage.stations, { royalty: 0, lord: 0, commoner: 0, lowborn: 1, beyond: 1 });
});

test('spec 15: the instruction asks every field, each enum value named in a line with what it is, the request\'s greeting and name_ko (its form, null for a name no one\'s own); the greeting sent is the bundle\'s, cleaned (escapes an ellipsis, keyword marks and waits gone, one space) and cut to GREETING_MAX; a script with none sends none', () => {
  for (const f of FIELDS) assert.ok(SYSTEM.includes(`"${f}": "<`), `${f} in the answer's form`);
  for (const [list, head] of [[SEXES, '- sex: one of'], [STATIONS, '- station, its rank: one of'], [MANNERS, '- manner, how it speaks: one of'],
    [Object.keys(CLASSES), '- class, its role: one of']]) {
    const lines = SYSTEM.split('\n');
    const at = lines.indexOf(head);
    assert.ok(at >= 0, head);
    assert.deepEqual(lines.slice(at + 1, at + 1 + list.length).map((l) => /^ {2}"([a-z]+)" \(.+\)[,;]$/.exec(l)?.[1]), list, `${head}: one line each, with what it is`);
  }
  assert.ok(SYSTEM.includes('"greeting": "<the') && SYSTEM.includes('"commoner" when nothing tells') && SYSTEM.includes('"plain" when nothing tells'));
  assert.ok(SYSTEM.includes('1 to 8 syllables') && SYSTEM.includes('null for a name that is'));
  // Round 2: the lord is the gentle, the commoner the free townsfolk.
  assert.ok(SYSTEM.includes('\n  "lord" (the gentle: those who govern, such as lords, mayors and the heads of orders, and the knighted and the learned, such as '
    + 'knights, court mages, scholars, high clergy and the hero\'s companions),\n'));
  assert.ok(SYSTEM.includes('\n  "commoner" (free townsfolk and villagers: farmers, craftsmen, shopkeepers, innkeepers, sailors, bards, town healers, soldiers of '
    + 'the ranks),\n'));
  const said = 'Well @met, $G!*  I am   #1 of the  @town.  ';
  const b = [
    bundle('a', 7501, `${SENTINEL}A`, { greeting: said }), bundle('a', 7502, `${SENTINEL}B`, { greeting: 'h'.repeat(GREETING_MAX + 9) }),
    bundle('a', 7503, `${SENTINEL}C`, { greeting: null }),
  ];
  const plan = planOfSpeakers(b, new Map(), 'm');
  const chars = plan.batches[0].scripts.map((s) => s.character);
  assert.equal(chars[0].greeting, 'Well met, …! I am … of the town.');
  assert.equal(chars[1].greeting, 'h'.repeat(GREETING_MAX));
  assert.ok(!Object.hasOwn(chars[2], 'greeting'), 'no greeting, no member');
  assert.ok(!plan.batches[0].body.includes(LINE), 'no answer\'s line');
});

test('an answer that never reads (asked again once), a route that refuses and a SIGINT while sending write nothing', async () => {
  const f = await fixture();
  const prose = fakeTransport({ prose: true });
  const r = await run([f.bundles, '--notes', f.notes, '--out', f.out, '--send'], prose.send);
  assert.equal(r.code, 3);
  assert.equal(prose.bodies.length, 2, 'asked again once');
  assert.match(r.text, /Not answered: age 5 \(invalid 5\) .*class 5 \(invalid 5\)/);
  assert.match(r.text, /No answer gave a field: nothing was written\./);
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
    const speakers = userOf(body).characters.map((c) => ({ n: c.n, ...MANY }));
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

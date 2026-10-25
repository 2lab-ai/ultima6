#!/usr/bin/env node
// The speaker file (spec 12 Design 1; its second form, spec 15 Design 1): reads the NPC bundles
// the page downloads (ultima6-npc-bundles.json, one u6.npc-bundle/1 per conversation script) and
// the personality notes tools/speech/profile.mjs made for the voices (u6.voice-notes/1), asks a
// model for each script's age, class, sex, station and manner and its NPC's own name in Hangul
// from its NPC's name, its description (the look text), its note and the first lines of its
// greeting, and writes the one file tools/i18n/translate.mjs takes with --speakers, its coverage
// receipt inside it:
//
//   <out>/speakers.json   u6.speakers/2: {format, model, coverage, speakers: [{archive, item, age,
//                         class, note, sex, station, manner, name_ko}]}, one entry per script, in
//                         (archive, item) order; `coverage`: the counts, and the scripts an
//                         answer gave a field no value of its form for, by (archive, item), each
//                         such field with a fixed reason (translate.mjs reads past it)
//
// <out> is ~/Games/ultima6/i18n/speakers/ unless --out names another; it and preview/ are made
// 0700, every file in them 0600; either one that is a symbolic link (or not a directory) is
// refused with no request, its target left as it is (tools/speech/profile.mjs privateDir).
//
// The scripts asked are the ones translate.mjs can translate (its planOf: not refused, with
// lines), each with its note when the notes file has one. They go in requests of at most
// BATCH_SCRIPTS scripts whose text (the JSON of the scripts the request carries) is at most
// BATCH_CHARS characters, through the same route and model as translate.mjs: a name is cut to
// NAME_MAX characters, a description to LOOK_MAX, a note to SPEAKER_NOTE_MAX and the greeting (its
// text as profile.mjs reads it, `clean`ed: the printing escapes an ellipsis, the keyword marks and
// waits gone) to GREETING_MAX, and a script whose entry, so cut and encoded, is still past
// SCRIPT_CHARS (its text full of characters JSON escapes) is not sent: it takes every field's
// default, counted (`oversize`). The answer is JSON only, {"speakers": [{n, age, class, sex,
// station, manner, name_ko}]}; one that is not (or empty, or a failed request) is asked again
// once. Each script's age must be one of child, youth, adult, elder, its class one of the guide's
// classes (translate.mjs CLASSES), its sex one of SEXES, its station one of STATIONS and its
// manner one of MANNERS, case and spaces aside, and its name_ko null or a name's Korean form (1 to
// 8 Hangul syllables, one space between two words, ASCII spaces at its ends aside): a field that
// is not (or missing) takes its default, `adult`, the script's Pass A class (translate.mjs
// speakerClass), `none`, the station of the script's class (STATION_OF_CLASS: the class its
// answer gave, else its Pass A class), `plain` or null, each counted by field and reason. The file
// is read back through translate.mjs's own reader (readSpeakers) before it is written; when no
// answer gave a single field nothing is written (a file of guesses only would pass for the
// model's). The file is staged, then renamed into place: a SIGINT before the rename leaves the one
// there as it was.
//
// The upload boundary (spec 08 Rights; tools/speech/profile.mjs's, used as it is): the bundles
// hold the owner's game text, and a request sends some scripts' names, descriptions and the first
// lines of their greetings, with their notes, to llmux on this machine, then to the model's
// provider. Before the first request the tool
// names that route, writes the exact first payload to a file of this run's own,
// <out>/preview/first-request-<pid>-<random>.json, and prints only its path and size, never its
// text. It sends only after a typed `yes` on a terminal, or with --send; anything else (another
// answer, an empty one, Ctrl-C, no terminal and no --send) exits with no request. --dry-run writes
// the preview and exits. A run holds <out>/.lock from before the preview to its last write, so the
// preview the owner reads is what his `yes` sends. llmux's answer is never followed elsewhere: a
// redirect is a failed request, and none more is sent. A request is llmux's Messages call (POST
// /v1/messages) with the model's name, an output cap, this tool's instruction and the scripts'
// text: no other client's prompt or headers.
//
// Rights: what this prints is counts (by age, sex, station and manner, and the fields not answered
// by reason), timings, paths and sizes, and the route (the address and the model's name): never a
// name, a Korean name, a line, a note, a script's (archive, item) or a hash. An error prints its
// kind only.
//
//   node tools/i18n/speakers.mjs <bundles.json> --notes <notes.json> [--send | --dry-run]
//        [--model NAME] [--llmux URL] [--out DIR] [--jobs N] [--timeout SECONDS]
import { randomBytes } from 'node:crypto';
import { rename, unlink } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  NotOwnDir, ask, clean, holdLock, llmuxTransport, local, privateDir, providerOf, sendAll, textOf, writePrivate,
} from '../speech/profile.mjs';
import {
  AGES, CLASSES, DEFAULTS as TRANSLATE, InputError, JOBS_MAX, MANNERS, SEXES, SPEAKERS_FORMAT_2, SPEAKER_NOTE_MAX, STATIONS,
  nameForm, objectOfAnswer, planOf, readJson, readSpeakers,
} from './translate.mjs';

export const DEFAULTS = {
  model: TRANSLATE.model,
  llmux: TRANSLATE.llmux,
  out: join(homedir(), 'Games', 'ultima6', 'i18n', 'speakers'),
  jobs: 2,
  timeoutS: 300,
};
export const MAX_TOKENS = 16000; // the answer's cap (a reasoning model's thinking counts in it)
// Spec 15: an entry now also carries up to GREETING_MAX characters of the greeting and is answered
// with six fields, not two. An entry so cut is at most 80 + 320 + 400 + 400 characters of text and
// some 60 of JSON around it; SCRIPT_CHARS leaves some 340 more for what JSON escapes (a quote mark
// of the greeting is two). A batch of BATCH_SCRIPTS such scripts is at most BATCH_CHARS (some 4000
// tokens in), and its answer some 12 short entries, far under MAX_TOKENS with the model's thinking.
export const BATCH_SCRIPTS = 12;
export const BATCH_CHARS = 12000; // the characters of a request's text: its JSON of the scripts (the user content)
export const SCRIPT_CHARS = 1600; // the characters of one script's entry in it (its comma included)
export const NAME_MAX = 80; // the characters of a name sent
export const LOOK_MAX = 320; // the characters of a description sent
export const GREETING_MAX = 400; // the characters of a greeting's first lines sent
export const NOTES_FORMAT = 'u6.voice-notes/1';
const LOCK = '.lock';

const plain = (x) => x !== null && typeof x === 'object' && !Array.isArray(x);
const finite = (x) => typeof x === 'number' && Number.isFinite(x);
const chars = (text) => [...text].length;
const cut = (text, max) => (chars(text) > max ? [...text].slice(0, max).join('') : text);

// The words that tell each class (made up for the instruction; the guide's classes, in its order).
const CLASS_HINTS = {
  royalty: 'a king, a queen, a prince or a princess',
  noble: 'a lord, a lady, a mayor',
  mage: 'a mage, a wizard, an enchanter',
  scholar: 'a sage, a scribe, an alchemist',
  clergy: 'a priest, a monk, a healer',
  guard: 'a guard, a soldier, a knight, a warrior',
  merchant: 'a shopkeeper, an innkeeper, a smith, a vendor',
  commoner: 'any other of the common folk: a farmer, a servant, a beggar',
  thief: 'a thief, a pirate, a rogue',
  spirit: 'a ghost or a spirit',
  gargoyle: 'a gargoyle',
};

// Spec 15 Design 1: what each sex, station and manner is (made up for the instruction, in the
// enums' order).
const SEX_HINTS = { female: 'a woman or a girl', male: 'a man or a boy', none: 'a shrine, a statue, a thing' };
const STATION_HINTS = {
  royalty: 'the crown',
  lord: 'the gentle: those who govern, such as lords, mayors and the heads of orders, and the knighted and the learned, such as knights, '
    + 'court mages, scholars, high clergy and the hero\'s companions',
  commoner: 'free townsfolk and villagers: farmers, craftsmen, shopkeepers, innkeepers, sailors, bards, town healers, soldiers of the ranks',
  lowborn: 'beggars, thieves, pirates, drunks, outcasts, ruffians',
  beyond: 'spirits, shrines, statues, the otherworldly',
};
const MANNER_HINTS = {
  plain: 'nothing marked',
  jester: 'play, rhyme, puns',
  mad: 'lurching, broken thought',
  pompous: 'inflated, self-important',
  drunk: 'slurred, trailing off',
  sly: 'flattering, hinting',
  gruff: 'short, rough',
  timid: 'hesitant, fearful',
  pious: 'blessing, intoning',
};
// An enum's values for the instruction, one a line, each with its hint; the last ends with `end`.
const enumLines = (list, hints, end) => list.map((v, i) => `  "${v}"${hints[v] ? ` (${hints[v]})` : ''}${i + 1 < list.length ? ',' : end}`);

/// What the model is told (the same for every request).
export const SYSTEM = [
  'You read the characters of a role-playing game, Ultima VI, so that their lines can be translated',
  'into Korean at the speech level of their age, their station and their manner. The request is',
  'JSON: {"characters": [{"n": <number>, "name": "<name>", "description": "<how the game describes',
  'them>", "note": "<a short note on their personality and manner of speech>", "greeting": "<the',
  'first lines they say>"}]} ("note" and "greeting" only where there is one). For each character',
  'tell its age, class, sex, station and manner and its own name in Korean from what the request',
  'says of it, and answer with one JSON object and nothing else:',
  '{"speakers": [{"n": <number>, "age": "<age>", "class": "<class>", "sex": "<sex>", "station": "<station>", "manner": "<manner>", "name_ko": "<its name in Hangul>" or null}]}',
  'with exactly one entry for every character asked, its own n:',
  '- age: "child" (a child), "youth" (a young person not yet grown: an apprentice, a squire, a young',
  '  servant), "adult" or "elder" (an old person); "adult" when nothing tells;',
  '- class, its role: one of',
  ...enumLines(Object.keys(CLASSES), CLASS_HINTS, ';'),
  '- sex: one of',
  ...enumLines(SEXES, SEX_HINTS, ';'),
  '- station, its rank: one of',
  ...enumLines(STATIONS, STATION_HINTS, ';'),
  '  "commoner" when nothing tells;',
  '- manner, how it speaks: one of',
  ...enumLines(MANNERS, MANNER_HINTS, ';'),
  '  "plain" when nothing tells;',
  '- name_ko: the character\'s own name in Hangul, as Korean writes a foreign name (외래어 표기법),',
  '  1 to 8 syllables with one space between two words and nothing else; null for a name that is',
  '  no one\'s own (a guard, a farmer).',
].join('\n');

// --- the notes and the bundles -----------------------------------------------------------------

/// The personality notes (`value`, the parsed u6.voice-notes/1 tools/speech/profile.mjs writes) as
/// a Map of `<archive>:<item>` to its note. An object of that format whose `notes` are each
/// {archive (a or b), item (0 to 65535), note (text)} (their `actors` not read), each script once;
/// anything else throws an InputError with a fixed message (a count at most).
export function readNotes(value) {
  const refused = (why) => new InputError(`the notes file is not ${NOTES_FORMAT}${why ? `: ${why}` : ''}`);
  if (!plain(value) || value.format !== NOTES_FORMAT || !Array.isArray(value.notes)) throw refused('');
  const entry = (n) => plain(n) && ['a', 'b'].includes(n.archive) && Number.isInteger(n.item) && n.item >= 0 && n.item <= 0xffff
    && typeof n.note === 'string';
  const bad = value.notes.filter((n) => !entry(n)).length;
  if (bad > 0) throw refused(`${bad} entries are not {archive, item, note}`);
  const notes = new Map();
  let twice = 0;
  for (const n of value.notes) {
    const key = `${n.archive}:${n.item}`;
    if (notes.has(key)) twice += 1;
    else notes.set(key, n.note.replace(/\s+/g, ' ').trim());
  }
  if (twice > 0) throw refused(`${twice} entries give a script given before`);
  return notes;
}

/// The request's text for `batch` (its user content): the JSON of the scripts' entries.
export const contentOfBatch = (batch) => JSON.stringify({ characters: batch.map((s) => s.character) });

/// The request body for `batch` (the exact bytes sent): a Messages call.
export function bodyOf(model, batch) {
  return JSON.stringify({ model, max_tokens: MAX_TOKENS, system: SYSTEM, messages: [{ role: 'user', content: contentOfBatch(batch) }] });
}

// What a request says of script `s`, numbered `n`.
const entryOf = (s, n) => ({
  n, name: s.name, description: s.description, ...(s.note ? { note: s.note } : {}), ...(s.greeting ? { greeting: s.greeting } : {}),
});
// The characters a request's text holds besides its entries.
const CONTENT_BASE = JSON.stringify({ characters: [] }).length;

/// The scripts of `bundles` (the parsed file) to ask, with `notes` (readNotes's Map): every script
/// translate.mjs can translate (its planOf), in (archive, item) order (`list`), each {archive,
/// item, cls (its Pass A class), name (cut to NAME_MAX), description (cut to LOOK_MAX), note ('',
/// without one; cut to SPEAKER_NOTE_MAX), greeting (its greeting's text, cleaned as profile.mjs
/// reads it, '' without one; cut to GREETING_MAX), chars (its entry's characters in a request, its
/// comma included, its n at the widest)}; one whose chars pass SCRIPT_CHARS is `oversize` and not
/// sent, the others go in batches of at most BATCH_SCRIPTS scripts whose text (contentOfBatch) is
/// at most BATCH_CHARS characters, each script with its `batch` (the index) and `n`, each batch
/// with its `character` entries and its body. `noted`: the scripts with a note; `unused`: the
/// notes of no script of the bundles. Throws translate.mjs's fixed message when the file is not a
/// list of bundles.
export function planOfSpeakers(bundles, notes, model) {
  const { read, scripts } = planOf(bundles, { first: Infinity });
  const known = new Set();
  const list = scripts.map((s) => {
    const b = s.bundle;
    const key = `${b.archive}:${b.item}`;
    known.add(key);
    const name = cut(typeof b.name === 'string' ? clean(b.name) : '', NAME_MAX);
    const description = cut(textOf(b.narration?.look), LOOK_MAX);
    const note = cut(notes.get(key) ?? '', SPEAKER_NOTE_MAX);
    const greeting = cut(textOf(b.speech?.greeting), GREETING_MAX);
    const script = { archive: b.archive, item: b.item, cls: s.cls, name, description, note, greeting };
    const size = JSON.stringify(entryOf(script, BATCH_SCRIPTS)).length + 1;
    return { ...script, chars: size, oversize: size > SCRIPT_CHARS };
  });
  const batches = [];
  let batch = [];
  let used = CONTENT_BASE;
  for (const s of list.filter((x) => !x.oversize)) {
    if (batch.length > 0 && (batch.length >= BATCH_SCRIPTS || used + s.chars > BATCH_CHARS)) {
      batches.push(batch);
      batch = [];
      used = CONTENT_BASE;
    }
    s.batch = batches.length;
    s.n = batch.length;
    batch.push({ ...s, character: entryOf(s, s.n) });
    used += s.chars;
  }
  if (batch.length > 0) batches.push(batch);
  const sent = list.filter((s) => !s.oversize);
  return {
    read,
    list,
    scripts: list.length,
    asked: sent.length,
    oversize: list.length - sent.length,
    noted: list.filter((s) => s.note).length,
    unused: [...notes.keys()].filter((k) => !known.has(k)).length,
    chars: sent.reduce((n, s) => n + s.chars, 0),
    batches: batches.map((b) => ({ scripts: b, body: bodyOf(model, b) })),
  };
}

// --- the answers -------------------------------------------------------------------------------

/// The fields of a u6.speakers/2 entry an answer gives (spec 15), in the file's order.
export const FIELDS = ['age', 'class', 'sex', 'station', 'manner', 'name_ko'];

/// The entries of an answer: {entries: a Map of n to {age, class, sex, station, manner, name_ko} as
/// given (a field it lacks undefined)}, or null when it is not JSON of the answer's form (a code
/// fence around it allowed) or lists no entry.
export function speakersOfAnswer(content) {
  const v = objectOfAnswer(content);
  if (!v || !Array.isArray(v.speakers)) return null;
  const entries = new Map();
  for (const e of v.speakers) {
    if (plain(e) && Number.isInteger(e.n) && !entries.has(e.n)) entries.set(e.n, Object.fromEntries(FIELDS.map((f) => [f, e[f]])));
  }
  return entries.size > 0 ? { entries } : null;
}

const word = (x) => (typeof x === 'string' ? x.trim().toLowerCase() : null);

// A name's Korean form as an answer gives it, the ASCII spaces at its ends aside (no other
// character is trimmed into one, a line terminator included): the form, null for an answer of
// null, or undefined for one that is neither.
const nameOf = (x) => {
  if (x === null) return null;
  const ko = typeof x === 'string' ? x.replace(/^ +| +$/g, '') : undefined;
  return nameForm(ko) ? ko : undefined;
};

/// The station a script takes when its answer gives none, by the class read for it (spec 15
/// round 3): the crown's, a noble's the lord's, a spirit's beyond, a thief's the lowborn's; any
/// other class's (STATION_OTHER) the commoner's.
export const STATION_OF_CLASS = { royalty: 'royalty', noble: 'lord', spirit: 'beyond', thief: 'lowborn' };
export const STATION_OTHER = 'commoner';
export const stationOfClass = (cls) => (Object.hasOwn(STATION_OF_CLASS, cls) ? STATION_OF_CLASS[cls] : STATION_OTHER);

/// A script's fields from its answer's `entry` (undefined when the answer has none, or when its
/// request failed: `failed`, that reason), each gated to its form (an enum's case and spaces
/// aside): an age that is not one is `adult`, a class `passA`, a sex `none`, a station the one of
/// the class read for the script (the answer's class, else `passA`: [`stationOfClass`]), a manner
/// `plain` and a name_ko (null, or a name's Korean form, ASCII spaces at its ends aside) null;
/// {age, class, sex, station, manner, name_ko} and `why`, each field's reason (missing, invalid,
/// or the request's; null for one taken). A field the entry lacks is missing.
export function gated(entry, passA, failed = null) {
  const enumOf = (list) => (x) => (list.includes(word(x)) ? word(x) : undefined);
  const read = {
    age: enumOf(AGES), class: (x) => (Object.hasOwn(CLASSES, word(x) ?? '') ? word(x) : undefined), sex: enumOf(SEXES),
    station: enumOf(STATIONS), manner: enumOf(MANNERS), name_ko: nameOf,
  };
  const defaults = { age: 'adult', class: passA, sex: 'none', station: null, manner: 'plain', name_ko: null };
  const out = {};
  const why = {};
  for (const f of FIELDS) {
    // FIELDS lists the class before the station, so the station's default reads the class taken.
    if (f === 'station') defaults.station = stationOfClass(out.class);
    const given = entry === undefined || !Object.hasOwn(entry, f) || entry[f] === undefined ? undefined : read[f](entry[f]);
    out[f] = given === undefined ? defaults[f] : given;
    why[f] = given !== undefined ? null : failed ?? (entry?.[f] === undefined ? 'missing' : 'invalid');
  }
  return { ...out, why };
}

const byReason = (list) => {
  const counts = {};
  for (const r of list) counts[r] = (counts[r] ?? 0) + 1;
  return { count: list.length, byReason: counts };
};

/// The speaker file (u6.speakers/2), its coverage receipt inside it (`coverage`), of `plan`'s
/// scripts (its `list`, in order) and its batches' `results` (sendAll's, with speakersOfAnswer); a
/// script not sent (`oversize`) takes every field's default, by that reason. `taken`: the scripts
/// an answer gave a field for; `coverage.fallbacks`, each field's defaults by reason.
export function outputsOf(plan, results, model) {
  const speakers = [];
  const fallback = [];
  const whys = Object.fromEntries(FIELDS.map((f) => [f, []]));
  let taken = 0;
  for (const s of plan.list) {
    const r = s.oversize ? { ok: false, reason: 'oversize' } : results[s.batch] ?? { ok: false, reason: 'not-sent' };
    const g = r.ok ? gated(r.entries.get(s.n), s.cls) : gated(undefined, s.cls, r.reason);
    speakers.push({
      archive: s.archive, item: s.item, age: g.age, class: g.class, note: s.note, sex: g.sex, station: g.station, manner: g.manner, name_ko: g.name_ko,
    });
    if (FIELDS.some((f) => g.why[f] === null)) taken += 1;
    for (const f of FIELDS) if (g.why[f] !== null) whys[f].push(g.why[f]);
    if (FIELDS.some((f) => g.why[f] !== null)) fallback.push({ archive: s.archive, item: s.item, ...g.why });
  }
  const count = (key, values) => Object.fromEntries(values.map((v) => [v, speakers.filter((s) => s[key] === v).length]));
  const coverage = {
    scripts: { read: plan.read, listed: plan.scripts, asked: plan.asked, oversize: plan.oversize, noted: plan.noted, taken, notesUnused: plan.unused },
    ages: count('age', AGES),
    classes: count('class', Object.keys(CLASSES)),
    sexes: count('sex', SEXES),
    stations: count('station', STATIONS),
    manners: count('manner', MANNERS),
    named: speakers.filter((s) => s.name_ko !== null).length,
    fallbacks: Object.fromEntries(FIELDS.map((f) => [f, byReason(whys[f])])),
    fallback,
  };
  return { file: { format: SPEAKERS_FORMAT_2, model, coverage, speakers }, taken };
}

// --- the run -----------------------------------------------------------------------------------

const USAGE = 'usage: node tools/i18n/speakers.mjs <bundles.json> --notes <notes.json> [--send | --dry-run] [--model NAME] '
  + `[--llmux URL] [--out DIR] [--jobs 1-${JOBS_MAX}] [--timeout SECONDS]`;

/// The options of `argv`, or {error}.
export function parseArgs(argv) {
  const o = { send: false, dryRun: false, model: DEFAULTS.model, llmux: DEFAULTS.llmux, out: DEFAULTS.out,
    jobs: DEFAULTS.jobs, timeoutS: DEFAULTS.timeoutS, bundles: null, notes: null, help: false };
  const value = (i) => (i + 1 < argv.length ? argv[i + 1] : null);
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--send') o.send = true;
    else if (a === '--dry-run') o.dryRun = true;
    else if (a === '--help' || a === '-h') o.help = true;
    else if (['--model', '--llmux', '--out', '--jobs', '--timeout', '--notes'].includes(a)) {
      const v = value(i);
      if (v === null) return { error: `${a} needs a value` };
      i += 1;
      if (a === '--model') o.model = v;
      else if (a === '--llmux') o.llmux = v;
      else if (a === '--out') o.out = resolve(v);
      else if (a === '--jobs') o.jobs = Number(v);
      else if (a === '--timeout') o.timeoutS = Number(v);
      else o.notes = resolve(v);
    } else if (a.startsWith('--')) return { error: `unknown option ${a}` };
    else if (o.bundles === null) o.bundles = resolve(a);
    else return { error: 'one bundles file only' };
  }
  if (o.help) return o;
  if (o.bundles === null) return { error: 'no bundles file named' };
  if (o.notes === null) return { error: 'no notes file named (--notes, tools/speech/profile.mjs\'s notes.json)' };
  if (!Number.isInteger(o.jobs) || o.jobs < 1 || o.jobs > JOBS_MAX) return { error: `--jobs is 1 to ${JOBS_MAX}` };
  if (!finite(o.timeoutS) || o.timeoutS <= 0) return { error: '--timeout is a number of seconds above 0' };
  if (!/^[\w.:[\]/-]{1,100}$/.test(o.model)) return { error: '--model is a model name' };
  return o;
}

const seconds = (ms) => `${(ms / 1000).toFixed(1)} s`;
const tally = (counts) => Object.entries(counts).map(([k, n]) => `${k} ${n}`).join(', ');
const reasons = (f) => (f.count > 0 ? ` (${Object.entries(f.byReason).sort().map(([r, n]) => `${r} ${n}`).join(', ')})` : '');

/// The tool: returns its exit code (0 done or a dry run, 1 nothing sent, 2 a usage or input
/// error, 3 written with a field not answered, or nothing written since no answer gave one, 130
/// interrupted). `transport` (default: llmux at --llmux), `stdin`, `stdout`, `isTTY`
/// (whether to ask on stdin), `signals` (SIGINT's emitter) and `onStaged` (awaited once the file
/// is staged, with the staged file's path) are the tests' to give.
export async function main(argv, {
  transport = null, stdin = process.stdin, stdout = process.stdout, isTTY = Boolean(process.stdin.isTTY),
  signals = process, now = () => performance.now(), onStaged = null,
} = {}) {
  const say = (line) => stdout.write(`${line}\n`);
  const o = parseArgs(argv);
  if (o.error) {
    say(`speakers: ${o.error}\n${USAGE}`);
    return 2;
  }
  if (o.help) {
    say(USAGE);
    return 0;
  }
  let route;
  try {
    route = new URL(o.llmux);
  } catch {
    route = null;
  }
  if (!route || !['http:', 'https:'].includes(route.protocol) || !local(route)) {
    say('speakers: --llmux is the address of llmux on this machine (127.0.0.1, localhost or [::1])');
    return 2;
  }

  // The notes and the bundles, and what to send of them.
  let plan;
  try {
    const notes = readNotes((await readJson(o.notes, 'notes')).value);
    const { value, size } = await readJson(o.bundles, 'bundles');
    plan = planOfSpeakers(value, notes, o.model);
    say(`Bundles: ${plan.read} scripts read from ${o.bundles} (${size} bytes): ${plan.asked} to ask (${plan.noted} with a note)`
      + `${plan.oversize > 0 ? `, ${plan.oversize} too long to send (each by Pass A, an adult of its class's station, `
        + 'sex none, manner plain, no Korean name)' : ''}; `
      + `notes: ${notes.size} read from ${o.notes}, ${plan.unused} for no script of the bundles.`);
  } catch (err) {
    if (err instanceof InputError) say(`speakers: ${err.message}`);
    else say(`speakers: the input does not read (${err?.name ?? 'Error'})`);
    return 2;
  }
  if (plan.batches.length === 0) {
    say('Nothing to ask: nothing was sent.');
    return 1;
  }

  // The boundary: the route named, the first payload written, and the owner's go.
  say(`Route: llmux on this machine (${route.origin}), then ${providerOf(o.model)} (model ${o.model}).`);
  say(`It sends the owner's game text: ${plan.batches.length} requests, each up to ${BATCH_SCRIPTS} scripts' NPC names, `
    + `descriptions and the first lines of their greetings with their notes in up to ${BATCH_CHARS} characters, ${plan.chars} characters in all; `
    + `up to ${o.jobs} at a time.`);
  let release;
  try {
    await privateDir(o.out);
    release = await holdLock(o.out);
  } catch (err) {
    if (err instanceof NotOwnDir) say(`speakers: ${err.message}: nothing was sent.`);
    else say(`speakers: ${o.out} cannot be used (${err?.code ?? err?.name ?? 'Error'}): nothing was sent.`);
    return 2;
  }
  if (!release) {
    say(`speakers: another run of a tool holds ${join(o.out, LOCK)} (remove it if none runs): nothing was sent.`);
    return 2;
  }
  try {
    return await consentAndSend(o, plan, route, { transport, stdin, stdout, isTTY, signals, now, say, onStaged });
  } finally {
    await release();
  }
}

// From the preview to the file, with `o.out`'s lock held (tools/speech/profile.mjs's order): the
// preview of this run (a name of its own), the owner's go, the requests, the gate, the file.
// SIGINT is heard from the first request to the file's rename: the file is first staged (a new
// file of its own in <out>, through writePrivate), and only when it is staged and no SIGINT came
// is it renamed over speakers.json, one rename; a SIGINT before that removes what was staged, so
// the speakers.json there stays as it was.
async function consentAndSend(o, plan, route, { transport, stdin, stdout, isTTY, signals, now, say, onStaged }) {
  const previewDir = join(o.out, 'preview');
  const preview = join(previewDir, `first-request-${process.pid}-${randomBytes(6).toString('hex')}.json`);
  try {
    await privateDir(previewDir, 'preview directory');
    const bytes = await writePrivate(preview, plan.batches[0].body);
    say(`The exact first request: ${preview} (${bytes} bytes).`);
  } catch (err) {
    if (err instanceof NotOwnDir) say(`speakers: ${err.message}: nothing was sent.`);
    else say(`speakers: the preview cannot be written (${err?.code ?? err?.name ?? 'Error'}): nothing was sent.`);
    return 2;
  }
  if (o.dryRun) {
    say('Dry run: nothing was sent.');
    return 0;
  }
  if (!o.send) {
    if (!isTTY) {
      say('Nothing was sent: there is no terminal to ask on; add --send to send.');
      return 1;
    }
    const answer = await ask(`Type yes to send ${plan.batches.length} requests: `, { stdin, stdout, signals });
    if (answer === 'interrupted') {
      say('\nNothing was sent (interrupted).');
      return 130;
    }
    if (answer !== 'yes') {
      say('Nothing was sent.');
      return 1;
    }
  }

  // The requests: each batch once, asked again once when it fails or its answer does not read.
  const stopper = new AbortController();
  const onSignal = () => stopper.abort();
  signals.on('SIGINT', onSignal);
  try {
    return await sendAndWrite(o, plan, route, stopper.signal, { transport, now, say, onStaged });
  } finally {
    signals.off('SIGINT', onSignal);
  }
}

// The requests, the gate and the file, `stop` aborted by a SIGINT (consentAndSend's).
async function sendAndWrite(o, plan, route, stop, { transport, now, say, onStaged }) {
  const t0 = now();
  const sent = await sendAll(plan.batches, {
    transport: transport ?? llmuxTransport(route),
    jobs: o.jobs,
    timeoutMs: o.timeoutS * 1000,
    stop,
    parse: speakersOfAnswer,
  });
  const c = sent.counts;
  const tokens = c.usage > 0 ? `tokens: input ${c.input}, output ${c.output} (${c.usage} answers counted)` : 'no token counts returned';
  say(`Sent: ${c.requests} requests (${c.retried} asked again) in ${seconds(now() - t0)}; ${tokens}.`);
  if (sent.halted) say(`The route refused a request (${sent.halted}): no request was sent after it.`);
  const interrupted = () => {
    say('Interrupted: nothing was written.');
    return 130;
  };
  if (stop.aborted) return interrupted();
  const { file, taken } = outputsOf(plan, sent.results, o.model);
  const { coverage } = file;
  const f = coverage.fallbacks;
  say(`Speakers: ${plan.scripts} scripts, ${taken} given a field by the answers; ages: ${tally(coverage.ages)}.`);
  say(`Sexes: ${tally(coverage.sexes)}. Stations: ${tally(coverage.stations)}. Manners: ${tally(coverage.manners)}.`);
  const missed = FIELDS.reduce((n, x) => n + f[x].count, 0);
  if (missed === 0) say('Not answered: none.');
  else {
    say(`Not answered: age ${f.age.count}${reasons(f.age)} (taken as adult), class ${f.class.count}${reasons(f.class)} (taken by Pass A), `
      + `sex ${f.sex.count}${reasons(f.sex)} (taken as none), station ${f.station.count}${reasons(f.station)} (taken by its class), `
      + `manner ${f.manner.count}${reasons(f.manner)} (taken as plain), name_ko ${f.name_ko.count}${reasons(f.name_ko)} (taken as null).`);
  }
  if (taken === 0) {
    say('No answer gave a field: nothing was written.');
    return 3;
  }

  // The file, read back through translate.mjs's own reader, then written.
  const text = JSON.stringify(file, null, 1);
  try {
    if (readSpeakers(JSON.parse(text)).size !== file.speakers.length) throw new InputError('size');
  } catch (err) {
    say(`speakers: the file does not read back (${err instanceof InputError ? 'its form' : err?.name ?? 'Error'}): nothing was written.`);
    return 2;
  }
  // The file staged, then renamed into place (one rename) only if no SIGINT came.
  const path = join(o.out, 'speakers.json');
  const stage = join(o.out, `.staged-${process.pid}-${randomBytes(6).toString('hex')}-speakers.json`);
  const unstage = () => unlink(stage).catch(() => {});
  let bytes;
  try {
    bytes = await writePrivate(stage, text);
    await onStaged?.(stage);
  } catch (err) {
    await unstage();
    say(`speakers: the file cannot be written (${err?.code ?? err?.name ?? 'Error'}): nothing was written.`);
    return 2;
  }
  if (stop.aborted) {
    await unstage();
    return interrupted();
  }
  try {
    await rename(stage, path);
  } catch (err) {
    await unstage();
    say(`speakers: the file cannot be put in place (${err?.code ?? err?.name ?? 'Error'}): nothing was written.`);
    return 2;
  }
  say(`Wrote ${path} (${bytes} bytes; its coverage member holds the counts and the scripts not answered).`);
  say(`Give it to translate.mjs: --speakers ${path}.`);
  return missed > 0 ? 3 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const exit = (code) => process.stdout.write('', () => process.exit(code));
  main(process.argv.slice(2)).then(exit, (err) => {
    // Its kind only: a message could quote what was read.
    process.stdout.write(`speakers: failed (${err?.name ?? 'Error'})\n`);
    exit(1);
  });
}

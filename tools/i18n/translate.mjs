#!/usr/bin/env node
// The Korean pack's translator (spec 08 Design 6, slice K1: conversation runs only). It reads the
// NPC bundles the page downloads (ultima6-npc-bundles.json, one u6.npc-bundle/1 per conversation
// script, each text segment keyed to its run), translates the runs of the scripts chosen into
// archaic Korean (사극체, tools/i18n/style-ko.md) through a model, and writes the pack the page
// takes in a drop, beside a coverage receipt:
//
//   <out>/ko.u6lp        the pack: `U6LP`, a line feed, then {format: u6.locale-pack/1, language:
//                        ko, tool, base: {converse: <the fingerprint of its scripts>}, converse:
//                        [{archive, item, offset, len, hash, text}]} (u6_core::locale)
//   <out>/coverage.json  u6.locale-coverage/1: the units found, translated and left English by
//                        reason, scripts and units named by their hashes only
//
// <out> is ~/Games/ultima6/i18n/ko/ unless --out names another; it and preview/ are made 0700,
// every file in them 0600; either one that is a symbolic link (or not a directory) is refused with
// no request, its target left as it is (tools/speech/profile.mjs privateDir). A run writes the pack of the scripts it translated (another run's pack
// there is replaced).
//
// What is chosen: one script, `--item <archive>:<item>` (the archive letter and item the bundle
// has), or the first N scripts with lines in (archive, item) order, `--first N`. One request per
// script: the style guide, the speaker's class and register (guessed from its name and
// description, Pass A of spec 08 §6; `--class` gives it instead), the NPC's name and description,
// and every run of its script in order, escapes kept. The answer is JSON only, {"lines": [{"n",
// "ko"}]}. Each line is checked by the mechanical gates of spec 08 §6 (structure only: the
// escapes' multiset, Hangul syllables and ASCII only, the josa markers and every `@{` closed as the
// text layer reads them, the runic text and the line breaks at its ends kept); a line that fails
// (or that the answer lacks) is asked again once, in a request of the failed lines, then left
// English and counted. A request that fails or whose answer does not read is asked again once.
//
// The upload boundary (spec 08 Rights; tools/speech/profile.mjs's, used as it is): the bundles
// hold the owner's game text, and a request sends one script's of it to llmux on this machine,
// then to the model's provider. Before the first request the tool names that route, writes the
// exact first payload to a file of this run's own, <out>/preview/first-request-<pid>-<random>.json,
// and prints only its path and size, never its text. It sends only after a typed `yes` on a
// terminal, or with --send; anything else (another answer, an empty one, Ctrl-C, no terminal and
// no --send) exits with no request. --dry-run writes the preview and exits. A run holds <out>/.lock
// from before the preview to its last write, so the preview the owner reads is what his `yes`
// sends; another run's preview is never removed. llmux's answer is never followed elsewhere: a
// redirect is a failed request, and none more is sent. A request is llmux's Messages call (POST
// /v1/messages) with the model's name, an output cap, the instruction and the script's lines: no
// other client's prompt or headers.
//
// Rights: what this prints is counts, timings, paths and sizes, and the route (the address and the
// model's name): never a name, a line, a script's (archive, item) or a hash. An error prints its
// kind only.
//
//   node tools/i18n/translate.mjs <bundles.json> (--item a:N | --first N) [--class CLASS]
//        [--send | --dry-run] [--model NAME] [--llmux URL] [--out DIR] [--jobs N]
//        [--timeout SECONDS]
import { randomBytes } from 'node:crypto';
import { readFile, stat } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  NotOwnDir, ask, contentOf, holdLock, llmuxTransport, local, privateDir, providerOf, sendAll,
  writePrivate,
} from '../speech/profile.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));

export const DEFAULTS = {
  model: 'gpt-6-astra',
  llmux: 'http://127.0.0.1:3456',
  out: join(homedir(), 'Games', 'ultima6', 'i18n', 'ko'),
  jobs: 2,
  timeoutS: 900,
};
export const JOBS_MAX = 4;
export const TOOL = 'tools/i18n/translate.mjs/1';
export const MAX_TOKENS = 32000; // the answer's cap (a reasoning model's thinking counts in it)
export const PACK_FORMAT = 'u6.locale-pack/1';
export const COVERAGE_FORMAT = 'u6.locale-coverage/1';
export const MAGIC = 'U6LP';
export const SCRIPT_LEN = 0x2800; // u6_core::converse::SCRIPT_LEN
export const MAX_TEXT_BYTES = 3 * SCRIPT_LEN; // u6_core::locale::MAX_TEXT_BYTES
const BUNDLE_FORMAT = 'u6.npc-bundle/1';
const BUNDLES_MAX = 256 << 20;
const LOCK = '.lock';

/// The speaker classes (spec 08 §7) and each one's register, as the request names it.
export const CLASSES = {
  royalty: '해라체 (~노라, ~느니라, ~하라)',
  noble: '하오체 (~하오, ~이오, ~시오)',
  mage: '하오체 (~하오, ~이오, ~시오)',
  scholar: '하오체 with Sino-Korean diction',
  clergy: '하오체 with Sino-Korean diction',
  guard: 'a clipped 하오체',
  merchant: 'an archaic 합쇼체 (~하옵니다, ~사옵니다)',
  commoner: 'a low, plain 하오체',
  thief: '하게체 or 해라체',
  spirit: '해라체 with archaic forms (~로다, ~도다, ~니라)',
  gargoyle: 'the provisional gargoyle register of the guide',
};

// Pass A: words of a name or description that tell a class, the most telling first.
const CLASS_WORDS = [
  ['royalty', /\b(king|queen|prince|princess|monarch|royal|lord british)\b/i],
  ['gargoyle', /\bgargoyles?\b/i],
  ['spirit', /\b(ghost|spirit|shade|wisp|phantom|apparition)\b/i],
  ['thief', /\b(thief|thieves|pirate|rogue|brigand|cutpurse|scoundrel|bandit)\b/i],
  ['mage', /\b(mage|wizard|sorcer\w*|magician|enchant\w*|witch)\b/i],
  ['clergy', /\b(priest\w*|monk|cleric|healer|abbot|nun)\b/i],
  ['scholar', /\b(scholar|sage|librarian|scribe|philosopher|alchemist|astronomer)\b/i],
  ['guard', /\b(guard|soldier|captain|warrior|knight|fighter|ranger|sentry)\b/i],
  ['merchant', /\b(merchant|shopkeeper|vendor|trader|innkeeper|tavern\w*|barkeep|smith|armou?rer|weaponsmith|baker|butcher|provisioner)\b/i],
  ['noble', /\b(lord|lady|duke|duchess|baron|baroness|noble\w*|mayor)\b/i],
];

const plain = (x) => x !== null && typeof x === 'object' && !Array.isArray(x);
const finite = (x) => typeof x === 'number' && Number.isFinite(x);

/// An input that is not what the tool reads: its message is fixed text (a count at most).
export class InputError extends Error {}

// --- hashes and the pack ----------------------------------------------------------------------

const MASK = 0xffffffffffffffffn;

/// FNV-1a 64 of `bytes` (u6_core::converse::disasm::catalog::hash64), as a BigInt.
export function hash64(bytes) {
  let h = 0xcbf29ce484222325n;
  for (const b of bytes) h = ((h ^ BigInt(b)) * 0x100000001b3n) & MASK;
  return h;
}

/// `v` as 16 lower-case hex digits.
export const hex64 = (v) => v.toString(16).padStart(16, '0');

/// A run's bytes: its text, one character per byte (the bundles' form).
export const latin1 = (text) => Buffer.from(text, 'latin1');

/// The fingerprint of the scripts a converse map names (u6_core::locale::fingerprint): FNV-1a 64
/// over, per distinct (archive, item) in order, the letter, the item (2 bytes LE) and the
/// script's hash (8 bytes LE). `scripts`: [{archive, item, hash: 16 hex digits}].
export function fingerprint(scripts) {
  const seen = new Map();
  for (const s of scripts) seen.set(`${s.archive}:${s.item}`, s);
  const sorted = [...seen.values()].sort((x, y) => (x.archive === y.archive ? x.item - y.item : x.archive < y.archive ? -1 : 1));
  const bytes = [];
  for (const s of sorted) {
    bytes.push(s.archive.charCodeAt(0), s.item & 255, s.item >> 8);
    let h = BigInt(`0x${s.hash}`);
    for (let i = 0; i < 8; i += 1) {
      bytes.push(Number(h & 255n));
      h >>= 8n;
    }
  }
  return hex64(hash64(bytes));
}

/// The pack file (u6_core::locale's form): the magic, a line feed and the JSON object, the runs in
/// key order. `runs`: [{archive, item, offset, len, hash, text}].
export function packBytes({ language = 'ko', tool = TOOL, base, runs }) {
  const sorted = [...runs].sort((x, y) => (x.archive !== y.archive ? (x.archive < y.archive ? -1 : 1)
    : x.item !== y.item ? x.item - y.item : x.offset - y.offset));
  const json = {
    format: PACK_FORMAT,
    language,
    tool,
    base: { converse: base },
    converse: sorted.map((r) => ({ archive: r.archive, item: r.item, offset: r.offset, len: r.len, hash: r.hash, text: r.text })),
  };
  return Buffer.concat([Buffer.from(`${MAGIC}\n`), Buffer.from(JSON.stringify(json), 'utf8')]);
}

// --- the escapes and the gates ---------------------------------------------------------------

const PREFIXED = '$#^';
const SINGLE = '*@&<>`/\\%';
const isEscape = (c) => PREFIXED.includes(c) || SINGLE.includes(c);

const MARKERS = ['{이/가}', '{은/는}', '{을/를}', '{과/와}', '{(으)로}'];

// `text` without its josa markers: the resolver takes a whole marker before the escape layer
// reads anything of it (so its `/` is no plural marker), except right after an `@`, whose `{`
// opens a brace.
function withoutMarkers(text) {
  let out = '';
  for (let i = 0; i < text.length; i += 1) {
    const m = text[i - 1] !== '@' && MARKERS.find((k) => text.startsWith(k, i));
    if (m) i += m.length - 1;
    else out += text[i];
  }
  return out;
}

/// The escapes of `text` in order, as the disassembler splits a run (u6-core disasm run_parts):
/// `$`, `#` and `^` with the next character when it is not an escape itself, and each of
/// `* @ & < > ` / \ %` alone; a josa marker is none of them.
export function placeholders(full) {
  const text = withoutMarkers(full);
  const out = [];
  for (let i = 0; i < text.length; i += 1) {
    const c = text[i];
    if (PREFIXED.includes(c)) {
      const arg = i + 1 < text.length && !isEscape(text[i + 1]) ? text[i + 1] : '';
      out.push(c + arg);
      i += arg.length;
    } else if (SINGLE.includes(c)) {
      out.push(c);
    }
  }
  return out;
}

/// The keywords the `@` escapes of English `text` mark, in order and case folded: the ASCII
/// letters and digits right after each `@`. That is the keyword the escape layer shows for every
/// `@` of the owner's scripts (it runs to a character of the game's keyword-end list, which the
/// tool does not have, or to the next escape prefix; the real-data test
/// `the_tools_keyword_rule_is_the_escape_layers_on_the_owners_runs` checks the two agree).
export function keywordsOf(text) {
  return [...text.matchAll(/@([A-Za-z0-9]*)/g)].map((m) => m[1].toLowerCase());
}

/// The words the `@{…}` braces of a translation hold, in order and case folded: for each `@`
/// followed at once by `{`, its text up to the `}` that closes it (a josa marker's `}` is the
/// marker's, so the marker is part of the word).
export function braceWords(ko) {
  const out = [];
  for (let i = 0; i < ko.length; i += 1) {
    if (ko[i] !== '@' || ko[i + 1] !== '{') continue;
    let j = i + 2;
    let word = '';
    while (j < ko.length && ko[j] !== '}') {
      const m = MARKERS.find((k) => ko.startsWith(k, j));
      word += m ?? ko[j];
      j += m ? m.length : 1;
    }
    out.push(word.toLowerCase());
    i = j;
  }
  return out;
}

// Whether `c` is a character the game can show: ASCII printable, the line feed, a Hangul syllable.
const showable = (c) => c === '\n' || (c >= ' ' && c <= '\x7f') || (c >= '가' && c <= '힣');

// The text between `<` and `>` (drawn in runes), each stretch in order.
function runic(text) {
  const out = [];
  let at = -1;
  for (let i = 0; i < text.length; i += 1) {
    if (text[i] === '<' && at < 0) at = i + 1;
    else if (text[i] === '>' && at >= 0) {
      out.push(text.slice(at, i));
      at = -1;
    }
  }
  if (at >= 0) out.push(text.slice(at));
  return out;
}

const edges = (text) => [/^\n*/.exec(text)[0].length, /\n*$/.exec(text)[0].length];

/// Why `ko` is not a translation of the run `en` the text layer can take, or null: the escapes'
/// multiset, the characters, the josa markers and the `@{` braces as the text layer reads them
/// (hidden `&…&` text is dropped before it is read; `@` is followed at once by `{` and opens a
/// brace, closed by a `}` that is no marker's; every other `{` starts one of the markers), the
/// keywords (the words the braces hold are, as a multiset and case folded, the keywords the
/// English `@`s mark, so the player is shown only words the conversation takes), the runic text,
/// the line breaks at both ends, its length.
export function gate(en, ko) {
  if (typeof ko !== 'string' || (ko.length === 0 && en.length > 0)) return 'empty';
  if (Buffer.byteLength(ko) > MAX_TEXT_BYTES) return 'long';
  if (![...ko].every(showable)) return 'characters';
  const a = placeholders(en).sort();
  const b = placeholders(ko).sort();
  if (a.length !== b.length || a.some((p, i) => p !== b[i])) return 'escapes';
  let hidden = false;
  let brace = false;
  for (let i = 0; i < ko.length; i += 1) {
    const c = ko[i];
    if (c === '&') {
      hidden = !hidden;
    } else if (hidden) {
      continue;
    } else if (c === '@') {
      if (ko[i + 1] !== '{' || brace) return 'braces';
      brace = true;
      i += 1;
    } else if (c === '{') {
      const m = MARKERS.find((k) => ko.startsWith(k, i));
      if (!m) return 'markers';
      i += m.length - 1;
    } else if (c === '}') {
      if (!brace) return 'markers';
      brace = false;
    }
  }
  if (brace) return 'braces';
  const [ka, kb] = [keywordsOf(en).sort(), braceWords(ko).sort()];
  if (ka.length !== kb.length || ka.some((k, i) => k !== kb[i])) return 'keywords';
  const [ra, rb] = [runic(en), runic(ko)];
  if (ra.length !== rb.length || ra.some((r, i) => r !== rb[i])) return 'runic';
  const [ea, eb] = [edges(en), edges(ko)];
  if (ea[0] !== eb[0] || ea[1] !== eb[1]) return 'line-breaks';
  return null;
}

// --- the bundles -----------------------------------------------------------------------------

/// The runs of `bundle` (u6.npc-bundle/1) in script order, each once: {offset, text, joins}
/// (`joins`: a later part of a run a path enters in the middle).
export function unitsOf(bundle) {
  const segments = [...(bundle.narration?.look ?? []), ...(bundle.speech?.greeting ?? [])];
  for (const turn of Array.isArray(bundle.turns) ? bundle.turns : []) {
    segments.push(...(turn?.segments ?? []));
    for (const topic of Array.isArray(turn?.topics) ? turn.topics : []) segments.push(...(topic?.answer ?? []));
  }
  const byOffset = new Map();
  for (const s of segments) {
    if (s?.kind !== 'text' || typeof s.text !== 'string' || !Array.isArray(s.key)) continue;
    const [archive, item, offset] = s.key;
    if (archive !== bundle.archive || item !== bundle.item || !Number.isInteger(offset)) continue;
    if (s.text.length === 0 || [...s.text].some((c) => c.charCodeAt(0) > 0xff)) continue;
    byOffset.set(offset, { offset, text: s.text, joins: Number.isInteger(s.part_of) && s.part_of !== offset });
  }
  return [...byOffset.values()].sort((x, y) => x.offset - y.offset);
}

// The text of a bundle's segments (its look text), its runs joined.
const textOf = (segments) => (Array.isArray(segments) ? segments : [])
  .filter((s) => s?.kind === 'text' && typeof s.text === 'string').map((s) => s.text).join('');

/// Pass A: the speaker's class from its name and description, `commoner` when nothing tells.
export function speakerClass(bundle) {
  const said = `${typeof bundle.name === 'string' ? bundle.name : ''} ${textOf(bundle.narration?.look)}`;
  return CLASS_WORDS.find(([, words]) => words.test(said))?.[0] ?? 'commoner';
}

/// The instruction every request carries: the task, the answer's form, then the style guide.
export function systemOf(style) {
  return [
    'You translate the conversation lines of one character of a role-playing game, Ultima VI,',
    'from its faux-archaic English into archaic Korean (사극체), following the guide below.',
    'The request is JSON: the character (name, description, class and register) and its lines,',
    'each {"n": <number>, "en": "<line>"} (and "joins": true for a line that goes on from the one',
    'before it). Answer with one JSON object and nothing else:',
    '{"lines": [{"n": <number>, "ko": "<the Korean line>"}]}',
    'with exactly one entry for every line asked, its own n.',
    '',
    style,
  ].join('\n');
}

/// The request body for `lines` of `bundle` (the exact bytes sent): a Messages call.
export function bodyOf(model, system, bundle, cls, lines) {
  const npc = {
    name: typeof bundle.name === 'string' ? bundle.name : '',
    description: textOf(bundle.narration?.look),
    class: cls,
    register: CLASSES[cls],
  };
  const user = JSON.stringify({
    npc,
    lines: lines.map((u) => (u.joins ? { n: u.n, en: u.text, joins: true } : { n: u.n, en: u.text })),
  });
  return JSON.stringify({ model, max_tokens: MAX_TOKENS, system, messages: [{ role: 'user', content: user }] });
}

/// The lines of an answer: a Map of n to its Korean text, or null when it is not JSON of the
/// answer's form (a code fence around it allowed).
export function linesOfAnswer(content) {
  const text = typeof content === 'string' ? content.trim() : '';
  const from = text.indexOf('{');
  const to = text.lastIndexOf('}');
  if (from < 0 || to < from) return null;
  let v;
  try {
    v = JSON.parse(text.slice(from, to + 1));
  } catch {
    return null;
  }
  if (!plain(v) || !Array.isArray(v.lines)) return null;
  const lines = new Map();
  for (const l of v.lines) {
    if (plain(l) && Number.isInteger(l.n) && typeof l.ko === 'string' && !lines.has(l.n)) lines.set(l.n, l.ko);
  }
  return lines.size > 0 ? { lines } : null;
}

/// The bundles of `value` (the parsed file) as the scripts to translate: `item` ({archive, item})
/// or the `first` scripts with lines, in (archive, item) order. Throws a fixed message (a count
/// at most) when the file is not a list of bundles or the script named has no lines.
export function planOf(value, { item = null, first = null, cls = null } = {}) {
  if (!Array.isArray(value)) throw new InputError('the file is not a list of NPC bundles');
  const bad = value.filter((b) => !plain(b) || b.format !== BUNDLE_FORMAT || !['a', 'b'].includes(b.archive)
    || !Number.isInteger(b.item) || typeof b.hash !== 'string' || !/^[0-9a-f]{16}$/.test(b.hash)).length;
  if (bad > 0) throw new InputError(`${bad} entries of the file are not ${BUNDLE_FORMAT} bundles`);
  const sorted = [...value].sort((x, y) => (x.archive === y.archive ? x.item - y.item : x.archive < y.archive ? -1 : 1));
  const usable = sorted.filter((b) => (b.refused === null || b.refused === undefined) && unitsOf(b).length > 0);
  let chosen;
  if (item) {
    chosen = usable.filter((b) => b.archive === item.archive && b.item === item.item);
    if (chosen.length === 0) throw new InputError('the script named is not in the bundles, is refused or has no lines');
  } else {
    chosen = usable.slice(0, first);
  }
  const scripts = chosen.map((b) => {
    const units = unitsOf(b).map((u, n) => ({ ...u, n, len: latin1(u.text).length, hash: hex64(hash64(latin1(u.text))) }));
    return { bundle: b, cls: cls ?? speakerClass(b), units };
  });
  return { read: value.length, usable: usable.length, scripts };
}

// --- the run ---------------------------------------------------------------------------------

/// The units of `script` an answer gave that pass the gates: a Map of n to text; and the n of the
/// others, by reason.
function checked(script, answer) {
  const good = new Map();
  const failed = new Map();
  for (const u of script.units) {
    if (u.done) continue;
    const ko = answer?.lines?.get(u.n);
    const why = ko === undefined ? 'missing' : gate(u.text, ko);
    if (why === null) good.set(u.n, ko);
    else failed.set(u.n, why);
  }
  return { good, failed };
}

/// The pack and the coverage receipt of `plan`'s scripts, translated as `results` say.
export function outputsOf(plan, model) {
  const runs = [];
  const named = [];
  const left = [];
  const byReason = {};
  const perScript = [];
  let found = 0;
  let translated = 0;
  for (const s of plan.scripts) {
    let done = 0;
    for (const u of s.units) {
      found += 1;
      if (u.ko !== undefined) {
        done += 1;
        translated += 1;
        runs.push({ archive: s.bundle.archive, item: s.bundle.item, offset: u.offset, len: u.len, hash: u.hash, text: u.ko });
      } else {
        const why = u.why ?? 'not-sent';
        byReason[why] = (byReason[why] ?? 0) + 1;
        left.push({ hash: u.hash, reason: why });
      }
    }
    if (done > 0) named.push({ archive: s.bundle.archive, item: s.bundle.item, hash: s.bundle.hash });
    perScript.push({ hash: s.bundle.hash, class: s.cls, units: s.units.length, translated: done, left: s.units.length - done });
  }
  const coverage = {
    format: COVERAGE_FORMAT,
    model,
    tool: TOOL,
    category: 'conversations',
    scripts: { read: plan.read, usable: plan.usable, chosen: plan.scripts.length, translated: named.length },
    units: { found, translated, leftEnglish: found - translated, byReason, unexplained: 0 },
    perScript,
    left,
  };
  const pack = runs.length > 0 ? packBytes({ base: fingerprint(named), runs }) : null;
  return { pack, coverage, runs: runs.length };
}

const USAGE = 'usage: node tools/i18n/translate.mjs <bundles.json> (--item a:N | --first N) [--class CLASS] '
  + `[--send | --dry-run] [--model NAME] [--llmux URL] [--out DIR] [--jobs 1-${JOBS_MAX}] [--timeout SECONDS]`;

/// The options of `argv`, or {error}.
export function parseArgs(argv) {
  const o = { send: false, dryRun: false, model: DEFAULTS.model, llmux: DEFAULTS.llmux, out: DEFAULTS.out,
    jobs: DEFAULTS.jobs, timeoutS: DEFAULTS.timeoutS, bundles: null, item: null, first: null, cls: null, help: false };
  const value = (i) => (i + 1 < argv.length ? argv[i + 1] : null);
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--send') o.send = true;
    else if (a === '--dry-run') o.dryRun = true;
    else if (a === '--help' || a === '-h') o.help = true;
    else if (['--model', '--llmux', '--out', '--jobs', '--timeout', '--item', '--first', '--class'].includes(a)) {
      const v = value(i);
      if (v === null) return { error: `${a} needs a value` };
      i += 1;
      if (a === '--model') o.model = v;
      else if (a === '--llmux') o.llmux = v;
      else if (a === '--out') o.out = resolve(v);
      else if (a === '--jobs') o.jobs = Number(v);
      else if (a === '--timeout') o.timeoutS = Number(v);
      else if (a === '--first') o.first = Number(v);
      else if (a === '--class') o.cls = v;
      else {
        const m = /^([ab]):([0-9]{1,5})$/.exec(v);
        if (!m) return { error: '--item is <archive>:<item>, the archive a or b' };
        o.item = { archive: m[1], item: Number(m[2]) };
      }
    } else if (a.startsWith('--')) return { error: `unknown option ${a}` };
    else if (o.bundles === null) o.bundles = resolve(a);
    else return { error: 'one bundles file only' };
  }
  if (o.help) return o;
  if (o.bundles === null) return { error: 'no bundles file named' };
  if ((o.item === null) === (o.first === null)) return { error: 'name one script with --item, or --first N' };
  if (o.first !== null && (!Number.isInteger(o.first) || o.first < 1)) return { error: '--first is a number of scripts above 0' };
  if (o.cls !== null && !Object.hasOwn(CLASSES, o.cls)) return { error: `--class is one of ${Object.keys(CLASSES).join(', ')}` };
  if (!Number.isInteger(o.jobs) || o.jobs < 1 || o.jobs > JOBS_MAX) return { error: `--jobs is 1 to ${JOBS_MAX}` };
  if (!finite(o.timeoutS) || o.timeoutS <= 0) return { error: '--timeout is a number of seconds above 0' };
  if (!/^[\w.:[\]/-]{1,100}$/.test(o.model)) return { error: '--model is a model name' };
  return o;
}

const seconds = (ms) => `${(ms / 1000).toFixed(1)} s`;

/// The tool: returns its exit code (0 done or a dry run, 1 nothing sent, 2 a usage or input
/// error, 3 written with lines left English, 130 interrupted). `transport` (default: llmux at
/// --llmux), `stdin`, `stdout`, `isTTY` (whether to ask on stdin), `signals` (SIGINT's emitter)
/// and `style` (the guide's text) are the tests' to give.
export async function main(argv, {
  transport = null, stdin = process.stdin, stdout = process.stdout, isTTY = Boolean(process.stdin.isTTY),
  signals = process, now = () => performance.now(), style = null,
} = {}) {
  const say = (line) => stdout.write(`${line}\n`);
  const o = parseArgs(argv);
  if (o.error) {
    say(`translate: ${o.error}\n${USAGE}`);
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
    say('translate: --llmux is the address of llmux on this machine (127.0.0.1, localhost or [::1])');
    return 2;
  }
  let guide = style;
  if (guide === null) {
    try {
      guide = await readFile(join(HERE, 'style-ko.md'), 'utf8');
    } catch (err) {
      say(`translate: the style guide cannot be read (${err?.code ?? err?.name ?? 'Error'})`);
      return 2;
    }
  }
  const system = systemOf(guide);

  // The bundles, and what to send of them.
  let plan;
  let size;
  try {
    size = (await stat(o.bundles)).size;
    if (size > BUNDLES_MAX) {
      say(`translate: the bundles file is ${size} bytes, more than ${BUNDLES_MAX}`);
      return 2;
    }
    let value;
    try {
      value = JSON.parse(await readFile(o.bundles, 'utf8'));
    } catch (err) {
      say(`translate: ${err?.code ? `the bundles file cannot be read (${err.code})` : 'the bundles file does not read as JSON'}`);
      return 2;
    }
    plan = planOf(value, { item: o.item, first: o.first, cls: o.cls });
  } catch (err) {
    if (err instanceof InputError) say(`translate: ${err.message}`);
    else say(`translate: the bundles do not read (${err?.name ?? 'Error'})`);
    return 2;
  }
  const units = plan.scripts.reduce((n, s) => n + s.units.length, 0);
  const chars = plan.scripts.reduce((n, s) => n + s.units.reduce((m, u) => m + u.text.length, 0), 0);
  say(`Bundles: ${plan.read} scripts read from ${o.bundles} (${size} bytes), ${plan.usable} with lines; `
    + `${plan.scripts.length} chosen, ${units} lines, ${chars} characters.`);
  if (plan.scripts.length === 0) {
    say('Nothing to translate: nothing was sent.');
    return 1;
  }
  const jobOf = (s, lines) => ({ script: s, body: bodyOf(o.model, system, s.bundle, s.cls, lines) });
  const jobs = plan.scripts.map((s) => jobOf(s, s.units));

  // The boundary: the route named, the first payload written, and the owner's go.
  say(`Route: llmux on this machine (${route.origin}), then ${providerOf(o.model)} (model ${o.model}).`);
  say(`It sends the owner's game text: ${jobs.length} requests, each one script's lines with its NPC's name and `
    + `description; up to ${o.jobs} at a time, and a request again for lines that fail the checks.`);
  let release;
  try {
    await privateDir(o.out);
    release = await holdLock(o.out);
  } catch (err) {
    if (err instanceof NotOwnDir) say(`translate: ${err.message}: nothing was sent.`);
    else say(`translate: ${o.out} cannot be used (${err?.code ?? err?.name ?? 'Error'}): nothing was sent.`);
    return 2;
  }
  if (!release) {
    say(`translate: another run of a tool holds ${join(o.out, LOCK)} (remove it if none runs): nothing was sent.`);
    return 2;
  }
  try {
    return await consentAndSend(o, plan, jobs, jobOf, route, { transport, stdin, stdout, isTTY, signals, now, say });
  } finally {
    await release();
  }
}

// From the preview to the last file, with `o.out`'s lock held (tools/speech/profile.mjs's order):
// the preview of this run (a name of its own), the owner's go, the requests, the gates, the
// files.
async function consentAndSend(o, plan, jobs, jobOf, route, { transport, stdin, stdout, isTTY, signals, now, say }) {
  const previewDir = join(o.out, 'preview');
  const preview = join(previewDir, `first-request-${process.pid}-${randomBytes(6).toString('hex')}.json`);
  try {
    await privateDir(previewDir, 'preview directory');
    const bytes = await writePrivate(preview, jobs[0].body);
    say(`The exact first request: ${preview} (${bytes} bytes).`);
  } catch (err) {
    if (err instanceof NotOwnDir) say(`translate: ${err.message}: nothing was sent.`);
    else say(`translate: the preview cannot be written (${err?.code ?? err?.name ?? 'Error'}): nothing was sent.`);
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
    const answer = await ask(`Type yes to send ${jobs.length} requests: `, { stdin, stdout, signals });
    if (answer === 'interrupted') {
      say('\nNothing was sent (interrupted).');
      return 130;
    }
    if (answer !== 'yes') {
      say('Nothing was sent.');
      return 1;
    }
  }

  // The requests: each script once (asked again once when the request fails or its answer does
  // not read), then once more for its lines that failed the gates.
  const stopper = new AbortController();
  const onSignal = () => stopper.abort();
  signals.on('SIGINT', onSignal);
  const t0 = now();
  const counts = { requests: 0, retried: 0, input: 0, output: 0, usage: 0 };
  let halted = '';
  const send = async (list, tries) => {
    const sent = await sendAll(list, {
      transport: transport ?? llmuxTransport(route),
      jobs: o.jobs,
      timeoutMs: o.timeoutS * 1000,
      stop: stopper.signal,
      parse: linesOfAnswer,
      tries,
    });
    for (const k of Object.keys(counts)) counts[k] += sent.counts[k];
    halted ||= sent.halted;
    return sent.results;
  };
  let again = [];
  try {
    const first = await send(jobs, 2);
    first.forEach((r, i) => {
      const s = jobs[i].script;
      const { good, failed } = checked(s, r.ok ? r : null);
      for (const u of s.units) {
        if (good.has(u.n)) Object.assign(u, { ko: good.get(u.n), done: true });
        else u.why = r.ok ? failed.get(u.n) : r.reason;
      }
      // Lines an answer gave that failed the gates (or that it lacked) are asked again once; a
      // request that failed was already asked again.
      const retry = s.units.filter((u) => !u.done);
      if (r.ok && retry.length > 0 && !halted && !stopper.signal.aborted) again.push(jobOf(s, retry));
    });
    say(`  first round: ${jobs.length} requests answered in ${seconds(now() - t0)}; ${again.reduce((n, j) => n + j.script.units.filter((u) => !u.done).length, 0)} lines to ask again`);
    if (again.length > 0) {
      const second = await send(again, 1);
      second.forEach((r, i) => {
        const s = again[i].script;
        const { good, failed } = checked(s, r.ok ? r : null);
        for (const u of s.units) {
          if (u.done) continue;
          if (good.has(u.n)) Object.assign(u, { ko: good.get(u.n), done: true, why: undefined });
          else u.why = r.ok ? failed.get(u.n) : r.reason;
        }
      });
    }
  } finally {
    signals.off('SIGINT', onSignal);
    again = null;
  }
  const ms = now() - t0;
  const tokens = counts.usage > 0 ? `tokens: input ${counts.input}, output ${counts.output} (${counts.usage} answers counted)` : 'no token counts returned';
  say(`Sent: ${counts.requests} requests (${counts.retried} asked again) in ${seconds(ms)}; ${tokens}.`);
  if (halted) say(`The route refused a request (${halted}): no request was sent after it.`);
  const { pack, coverage, runs } = outputsOf(plan, o.model);
  const why = Object.entries(coverage.units.byReason).sort().map(([r, n]) => `${r} ${n}`).join(', ');
  say(`Lines: ${coverage.units.found} found, ${coverage.units.translated} translated, ${coverage.units.leftEnglish} left English`
    + `${why ? ` (${why})` : ''}; scripts translated ${coverage.scripts.translated} of ${coverage.scripts.chosen}.`);
  const written = [];
  if (pack) {
    const path = join(o.out, 'ko.u6lp');
    written.push(`${path} (${await writePrivate(path, pack)} bytes, ${runs} lines)`);
  }
  const cpath = join(o.out, 'coverage.json');
  written.push(`${cpath} (${await writePrivate(cpath, JSON.stringify(coverage, null, 1))} bytes)`);
  say(`Wrote ${written.join(', ')}.`);
  if (pack) say('Drop ko.u6lp on the page and choose 한글; coverage.json stays here.');
  else say('No line was translated: no pack was written.');
  if (stopper.signal.aborted) return 130;
  if (!pack) return 3;
  return coverage.units.leftEnglish > 0 ? 3 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const exit = (code) => process.stdout.write('', () => process.exit(code));
  main(process.argv.slice(2)).then(exit, (err) => {
    // Its kind only: a message could quote what was read.
    process.stdout.write(`translate: failed (${err?.name ?? 'Error'})\n`);
    exit(1);
  });
}

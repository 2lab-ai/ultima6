#!/usr/bin/env node
// The Korean pack's translator (spec 08 Design 6; slice K1: conversation runs; slice K2: the
// conversations' table strings and the game's own messages). It reads the NPC bundles the page
// downloads (ultima6-npc-bundles.json, one u6.npc-bundle/1 per conversation script, each text
// segment keyed to its run, each table string to its offset), and the game strings the page's
// second export writes (ultima6-strings.json, u6.string-bundle/1: the executable's strings the
// engine shows, each keyed by its hash), translates what is chosen into archaic Korean (사극체,
// tools/i18n/style-ko.md) through a model, and writes the pack the page takes in a drop, beside
// a coverage receipt:
//
//   <out>/ko.u6lp        the pack: `U6LP`, a line feed, then {format: u6.locale-pack/1, language:
//                        ko, tool, base: {converse: <the fingerprint of its scripts>}, converse:
//                        [{archive, item, offset, len, hash, text}], and when there are any
//                        tables: [{archive, item, offset, len, hash, capacity?, text}] and
//                        strings: [{hash, len, text}]} (u6_core::locale)
//   <out>/coverage.json  u6.locale-coverage/1: the units found, translated and left English by
//                        reason, by category, scripts and units named by their hashes only
//
// <out> is ~/Games/ultima6/i18n/ko/ unless --out names another; it and preview/ are made 0700,
// every file in them 0600; either one that is a symbolic link (or not a directory) is refused with
// no request, its target left as it is (tools/speech/profile.mjs privateDir).
//
// A run MERGES into the pack already in <out>: that pack is read through the tool's strict reader
// (readPack, the engine's rules) and, when bundles are given, its conversation part is checked
// against them again (each run and table string still the bundles', the fingerprint theirs); one
// that does not read or check is refused with no request. What this run translates replaces what
// the pack had under the same key; the rest stays. A run that translates nothing leaves it as it
// is. The merged pack is read through the same reader before it replaces anything: one the reader
// would refuse (past its size or its count of lines) is not written, the pack there and
// coverage.json left as they were.
//
// What is chosen: from the bundles, one script, `--item <archive>:<item>` (the archive letter and
// item the bundle has) or `--name <text>` (the bundle whose NPC name is that, case folded: one
// match, else a refusal that counts them; the name is never printed), or the first N scripts with
// lines in (archive, item) order, `--first N`; and with `--strings <file>`, every game string.
// One request per script: the style guide, the speaker's class and register (guessed from its
// name and description, Pass A of spec 08 §6; `--class` gives it instead), the NPC's name and
// description, every run of its script in order, escapes kept, and its table strings (a table
// string an op reads from inside it is left English, `inner`). The game strings go in requests of
// at most STRINGS_PER_REQUEST strings and STRING_CHARS characters. The answer is JSON only,
// {"lines": [{"n", "ko"}]}. Each line is checked by the mechanical gates of spec 08 §6 (structure
// only: the escapes' multiset, Hangul syllables and ASCII only, the josa markers and every `@{`
// closed as the text layer reads them, the runic text and the line breaks at its ends kept; for a
// table string the engine copies into a slot, its capacity and no marker; for a game string the
// same `%` directives in the same order and the reader's length); a line that fails (or that the
// answer lacks) is asked again once, in a request of the failed lines, then left English and
// counted. A request that fails or whose answer does not read is asked again once.
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
//   node tools/i18n/translate.mjs [<bundles.json> (--item a:N | --name TEXT | --first N)]
//        [--strings <strings.json>] [--class CLASS] [--send | --dry-run] [--model NAME]
//        [--llmux URL] [--out DIR] [--jobs N] [--timeout SECONDS]
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
export const TOOL = 'tools/i18n/translate.mjs/2';
export const MAX_TOKENS = 32000; // the answer's cap (a reasoning model's thinking counts in it)
export const PACK_FORMAT = 'u6.locale-pack/1';
export const COVERAGE_FORMAT = 'u6.locale-coverage/1';
export const MAGIC = 'U6LP';
export const SCRIPT_LEN = 0x2800; // u6_core::converse::SCRIPT_LEN
export const MAX_TEXT_BYTES = 3 * SCRIPT_LEN; // u6_core::locale::MAX_TEXT_BYTES
export const STRINGS_FORMAT = 'u6.string-bundle/1'; // u6_core::locale::strings::FORMAT
export const MIN_STRING_BYTES = 3; // u6_core::locale::strings::MIN_BYTES
export const MAX_STRING_BYTES = 256; // u6_core::locale::strings::MAX_BYTES
export const MAX_STRING_TEXT_BYTES = 3 * MAX_STRING_BYTES; // u6_core::locale::MAX_STRING_TEXT_BYTES
export const MAX_PACK_BYTES = 16 << 20; // u6_core::locale::MAX_PACK_BYTES
export const MAX_RUNS = 1 << 16; // u6_core::locale::MAX_RUNS (and MAX_STRINGS)
export const STRINGS_PER_REQUEST = 60;
export const STRING_CHARS = 4000; // the English characters of one request of game strings
const BUNDLE_FORMAT = 'u6.npc-bundle/1';
const BUNDLES_MAX = 256 << 20;
const LOCK = '.lock';
const KINDS = ['message', 'format', 'label'];

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

const byKey = (x, y) => (x.archive !== y.archive ? (x.archive < y.archive ? -1 : 1)
  : x.item !== y.item ? x.item - y.item : x.offset - y.offset);

/// The pack file (u6_core::locale's form): the magic, a line feed and the JSON object, the runs and
/// table strings in key order, the game strings in hash order; `tables` and `strings` written only
/// when there are any (a pack of runs only is K1's file). `runs`: [{archive, item, offset, len,
/// hash, text}]; `tables`: the same with `capacity` (or null); `strings`: [{hash, len, text}].
export function packBytes({ language = 'ko', tool = TOOL, base, runs, tables = [], strings = [] }) {
  const json = {
    format: PACK_FORMAT,
    language,
    tool,
    base: { converse: base },
    converse: [...runs].sort(byKey).map((r) => ({ archive: r.archive, item: r.item, offset: r.offset, len: r.len, hash: r.hash, text: r.text })),
  };
  if (tables.length > 0) {
    json.tables = [...tables].sort(byKey).map((t) => ({
      archive: t.archive, item: t.item, offset: t.offset, len: t.len, hash: t.hash,
      ...(Number.isInteger(t.capacity) ? { capacity: t.capacity } : {}), text: t.text,
    }));
  }
  if (strings.length > 0) {
    json.strings = [...strings].sort((x, y) => (x.hash < y.hash ? -1 : x.hash > y.hash ? 1 : 0))
      .map((t) => ({ hash: t.hash, len: t.len, text: t.text }));
  }
  return Buffer.concat([Buffer.from(`${MAGIC}\n`), Buffer.from(JSON.stringify(json), 'utf8')]);
}

/// A pack file that does not read: its message is the rule broken, the engine's fixed sentence
/// (u6_core::locale::PackError), never a value of the file.
export class PackReadError extends Error {}

const exact = (o, required, optional = []) => plain(o)
  && required.every((k) => Object.hasOwn(o, k))
  && Object.keys(o).every((k) => required.includes(k) || optional.includes(k));
const u32 = (x) => Number.isInteger(x) && x >= 0 && x <= 0xffffffff;
const hex16 = (x) => typeof x === 'string' && /^[0-9a-f]{16}$/.test(x);
const textOk = (t, max) => typeof t === 'string' && Buffer.byteLength(t) <= max && [...t].every(showableChar);
const showableChar = (c) => c === '\n' || (c >= ' ' && c <= '\x7f') || (c >= '가' && c <= '힣');

/// Whether the JSON `text` (already parsed) gives one member twice in an object: JSON.parse keeps
/// the last, the engine's reader (serde) refuses the file. Keys are compared decoded
/// ("conv\u0065rse" is "converse").
export function duplicateMembers(text) {
  const stack = [];
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === '"') {
      let j = i + 1;
      while (text[j] !== '"') j += text[j] === '\\' ? 2 : 1;
      const top = stack.at(-1);
      if (top && top.keys && top.expectKey) {
        const key = JSON.parse(text.slice(i, j + 1));
        if (top.keys.has(key)) return true;
        top.keys.add(key);
        top.expectKey = false;
      }
      i = j + 1;
      continue;
    }
    if (c === '{') stack.push({ keys: new Set(), expectKey: true });
    else if (c === '[') stack.push({ keys: null });
    else if (c === '}' || c === ']') stack.pop();
    else if (c === ',' && stack.at(-1)?.keys) stack.at(-1).expectKey = true;
    i += 1;
  }
  return false;
}

/// Reads a pack file as the engine's reader does (u6_core::locale::Pack::parse): the size, the
/// magic, JSON of exactly the pack's fields (each once), the format, the language, the tool, every hash 16
/// hex digits, the limits, each key inside the script buffer and once, each text one the game
/// can show (a table string's within its capacity, a game string's within the reader's length).
/// Returns {language, tool, base, converse, tables, strings}; throws PackReadError.
export function readPack(bytes) {
  const fail = (why) => { throw new PackReadError(why); };
  if (bytes.length > MAX_PACK_BYTES) fail('the file is larger than a Korean pack can be');
  const head = Buffer.from(`${MAGIC}\n`);
  if (bytes.length < head.length || !Buffer.from(bytes.subarray(0, head.length)).equals(head)) {
    fail('the file is not a Korean pack (it does not start with U6LP)');
  }
  const shape = 'its contents are not those of a Korean pack';
  let v;
  let text;
  try {
    text = new TextDecoder('utf-8', { fatal: true }).decode(bytes.subarray(head.length));
    v = JSON.parse(text);
  } catch {
    fail(shape);
  }
  if (duplicateMembers(text)) fail(shape);
  if (!exact(v, ['format', 'language', 'tool', 'base', 'converse'], ['tables', 'strings'])
    || typeof v.format !== 'string' || typeof v.language !== 'string' || typeof v.tool !== 'string'
    || !exact(v.base, ['converse']) || typeof v.base.converse !== 'string' || !Array.isArray(v.converse)
    || (v.tables !== undefined && !Array.isArray(v.tables)) || (v.strings !== undefined && !Array.isArray(v.strings))) fail(shape);
  const runKeys = ['archive', 'item', 'offset', 'len', 'hash', 'text'];
  const keyed = (r, optional = []) => exact(r, runKeys, optional) && typeof r.archive === 'string' && Number.isInteger(r.item)
    && r.item >= 0 && r.item <= 0xffff && u32(r.offset) && u32(r.len) && typeof r.hash === 'string' && typeof r.text === 'string'
    && (r.capacity === undefined || u32(r.capacity));
  const tables = v.tables ?? [];
  const strings = v.strings ?? [];
  if (!v.converse.every((r) => keyed(r)) || !tables.every((t) => keyed(t, ['capacity']))
    || !strings.every((t) => exact(t, ['hash', 'len', 'text']) && typeof t.hash === 'string' && u32(t.len) && typeof t.text === 'string')) fail(shape);
  if (v.format !== PACK_FORMAT) fail(`it is not of this page's pack format (${PACK_FORMAT})`);
  if (!['en', 'ko'].includes(v.language)) fail('it declares a language this page does not have');
  if (v.tool.length === 0 || v.tool.length > 64 || !/^[\x20-\x7e]+$/.test(v.tool)) fail('it does not name the tool that made it');
  if (!hex16(v.base.converse)) fail('a hash in it is not 16 hex digits');
  if (v.converse.length > MAX_RUNS || tables.length > MAX_RUNS || strings.length > MAX_RUNS) fail('it holds more lines than a pack can');
  const seen = new Set();
  const place = (r) => {
    if (!['a', 'b'].includes(r.archive)) fail('a line in it is keyed outside the conversation scripts');
    if (!hex16(r.hash)) fail('a hash in it is not 16 hex digits');
    if (r.len === 0 || r.offset + r.len > SCRIPT_LEN) fail('a line in it is keyed outside the conversation scripts');
    if (!textOk(r.text, MAX_TEXT_BYTES)) fail('a line in it holds a character the game cannot show');
  };
  for (const r of v.converse) {
    place(r);
    const k = `${r.archive}:${r.item}:${r.offset}`;
    if (seen.has(k)) fail('it holds one line twice');
    seen.add(k);
  }
  seen.clear();
  for (const t of tables) {
    place(t);
    if (Number.isInteger(t.capacity) && Buffer.byteLength(t.text) > t.capacity) fail('a table string in it is longer than the place the game copies it to');
    const k = `${t.archive}:${t.item}:${t.offset}`;
    if (seen.has(k)) fail('it holds one line twice');
    seen.add(k);
  }
  seen.clear();
  for (const t of strings) {
    if (!hex16(t.hash)) fail('a hash in it is not 16 hex digits');
    if (t.len < MIN_STRING_BYTES || t.len > MAX_STRING_BYTES || Buffer.byteLength(t.text) > MAX_STRING_TEXT_BYTES) fail('a game string in it is not one the game prints');
    if (!textOk(t.text, MAX_STRING_TEXT_BYTES)) fail('a line in it holds a character the game cannot show');
    if (seen.has(t.hash)) fail('it holds one line twice');
    seen.add(t.hash);
  }
  return { language: v.language, tool: v.tool, base: v.base.converse, converse: v.converse, tables, strings };
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

/// The `%` directives of a format in order, as `printf` reads them with `wide` on
/// (u6_core::locale::strings::directives): `%`, then `N$` (N from 1, its first digit not 0),
/// then `-` and `0` flags, then digits, then one more character (a conversion, or one put as it
/// is: `%%`, `%>`).
export function directivesOf(text) {
  return [...text.matchAll(/%(?:[1-9][0-9]*\$)?[-0]*[0-9]*[\s\S]?/g)].map((m) => m[0]);
}

const hasMarker = (ko) => MARKERS.some((m) => ko.includes(m)) || ko.includes('@{');

/// Why `ko` is not a translation of table string `u` the engine can take, or null: the line's
/// gates, then for a string the engine copies into a slot (printed later as it is, by `$n`) no
/// josa marker or `@{`, and its capacity.
export function tableGate(u, ko) {
  const why = gate(u.text, ko);
  if (why) return why;
  if (u.copied && hasMarker(ko)) return 'raw';
  if (Number.isInteger(u.capacity) && Buffer.byteLength(ko) > u.capacity) return 'capacity';
  return null;
}

/// Why `ko` is not a translation of game string `en` the engine can take, or null: the line's
/// gates, the same `%` directives in the same order, and the reader's length.
export function stringGate(en, ko) {
  const why = gate(en, ko);
  if (why) return why;
  const [a, b] = [directivesOf(en), directivesOf(ko)];
  if (a.length !== b.length || a.some((d, i) => d !== b[i])) return 'directives';
  if (Buffer.byteLength(ko) > MAX_STRING_TEXT_BYTES) return 'long';
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

/// The table strings of `bundle` (its `strings`, keyed to it), each once in offset order: {offset,
/// text, len, hash, capacity, copied, inner} (`copied`: an `a6` copies it into a string slot;
/// `inner`: an op reads from inside it). Empty strings are left out (the engine never looks one up).
export function tablesOf(bundle) {
  const byOffset = new Map();
  for (const s of Array.isArray(bundle.strings) ? bundle.strings : []) {
    if (!plain(s) || typeof s.text !== 'string' || !Array.isArray(s.key)) continue;
    const [archive, item, offset] = s.key;
    if (archive !== bundle.archive || item !== bundle.item || !Number.isInteger(offset)) continue;
    if (s.text.length === 0 || [...s.text].some((c) => c.charCodeAt(0) > 0xff)) continue;
    const bytes = latin1(s.text);
    byOffset.set(offset, {
      offset, text: s.text, len: bytes.length, hash: hex64(hash64(bytes)),
      capacity: Number.isInteger(s.capacity) ? s.capacity : null,
      copied: Array.isArray(s.slots) && s.slots.length > 0,
      inner: Array.isArray(s.inner) && s.inner.length > 0,
    });
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
    'before it; "table": true for a string the script picks at run time, with "copied": true and',
    '"capacity" for one it stores and prints later as it is). Answer with one JSON object and',
    'nothing else:',
    '{"lines": [{"n": <number>, "ko": "<the Korean line>"}]}',
    'with exactly one entry for every line asked, its own n.',
    '',
    style,
  ].join('\n');
}

/// The line of unit `u` as a request lists it.
function lineOf(u) {
  if (u.kind === 'table') {
    return { n: u.n, en: u.text, table: true, ...(u.copied ? { copied: true } : {}), ...(Number.isInteger(u.capacity) ? { capacity: u.capacity } : {}) };
  }
  return u.joins ? { n: u.n, en: u.text, joins: true } : { n: u.n, en: u.text };
}

/// The request body for `lines` of `bundle` (the exact bytes sent): a Messages call.
export function bodyOf(model, system, bundle, cls, lines) {
  const npc = {
    name: typeof bundle.name === 'string' ? bundle.name : '',
    description: textOf(bundle.narration?.look),
    class: cls,
    register: CLASSES[cls],
  };
  const user = JSON.stringify({ npc, lines: lines.map(lineOf) });
  return JSON.stringify({ model, max_tokens: MAX_TOKENS, system, messages: [{ role: 'user', content: user }] });
}

/// The instruction a request of game strings carries: the task, the answer's form, then the guide.
export function stringsSystemOf(style) {
  return [
    'You translate the strings a role-playing game, Ultima VI, prints itself (its command prompts',
    'and their results, the frame of a conversation, a few words it puts inside other lines) from',
    'English into archaic Korean (사극체), following the guide below. The request is JSON: the',
    'strings, each {"n": <number>, "en": "<string>", "kind": "message" | "format" | "label"}.',
    'Keep every % directive of a format exactly, in the same order. Answer with one JSON object',
    'and nothing else:',
    '{"lines": [{"n": <number>, "ko": "<the Korean string>"}]}',
    'with exactly one entry for every string asked, its own n.',
    '',
    style,
  ].join('\n');
}

/// The request body for game strings `units` (the exact bytes sent): a Messages call.
export function stringsBodyOf(model, system, units) {
  const user = JSON.stringify({ strings: units.map((u) => ({ n: u.n, en: u.text, kind: u.skind })) });
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

/// The bundles of `value` (the parsed file) as the scripts to translate: `item` ({archive, item}),
/// the one bundle whose `name` is `name` (case folded), or the `first` scripts with lines, in
/// (archive, item) order. Each script's units: its runs, then its table strings (one an op reads
/// from inside is left English, `inner`). Throws a fixed message (a count at most) when the file is
/// not a list of bundles or the script named has no lines.
export function planOf(value, { item = null, first = null, name = null, cls = null } = {}) {
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
  } else if (name !== null) {
    const want = name.toLowerCase();
    chosen = usable.filter((b) => typeof b.name === 'string' && b.name.toLowerCase() === want);
    if (chosen.length !== 1) throw new InputError(`${chosen.length} scripts with lines have that name: name one with --item`);
  } else {
    chosen = usable.slice(0, first ?? 0);
  }
  const scripts = chosen.map((b) => {
    const runs = unitsOf(b).map((u) => ({ ...u, kind: 'run', len: latin1(u.text).length, hash: hex64(hash64(latin1(u.text))) }));
    const tables = tablesOf(b).map((t) => ({ ...t, kind: 'table', ...(t.inner ? { done: true, why: 'inner' } : {}) }));
    const units = [...runs, ...tables].map((u, n) => ({ ...u, n }));
    return { kind: 'script', bundle: b, cls: cls ?? speakerClass(b), units };
  });
  return { read: value.length, usable: usable.length, scripts, bundles: value };
}

/// The game strings of `value` (the parsed u6.string-bundle/1) as the units to translate, in
/// requests of at most STRINGS_PER_REQUEST strings and STRING_CHARS characters. Throws a fixed
/// message (a count at most) when the file is not of that form or a string's text, length and hash
/// disagree.
export function stringsPlanOf(value) {
  if (!plain(value) || value.format !== STRINGS_FORMAT || !Array.isArray(value.strings)) {
    throw new InputError(`the file is not ${STRINGS_FORMAT}`);
  }
  const ok = (s) => plain(s) && hex16(s.hash) && Number.isInteger(s.len) && KINDS.includes(s.kind) && typeof s.text === 'string'
    && ![...s.text].some((c) => c.charCodeAt(0) > 0xff) && latin1(s.text).length === s.len
    && hex64(hash64(latin1(s.text))) === s.hash && s.len >= MIN_STRING_BYTES && s.len <= MAX_STRING_BYTES;
  const bad = value.strings.filter((s) => !ok(s)).length;
  if (bad > 0) throw new InputError(`${bad} entries of the file are not strings of ${STRINGS_FORMAT}`);
  const unique = [...new Map(value.strings.map((s) => [s.hash, s])).values()];
  const batches = [];
  let batch = [];
  let chars = 0;
  for (const s of unique) {
    if (batch.length > 0 && (batch.length >= STRINGS_PER_REQUEST || chars + s.text.length > STRING_CHARS)) {
      batches.push(batch);
      batch = [];
      chars = 0;
    }
    batch.push({ kind: 'string', n: batch.length, hash: s.hash, len: s.len, text: s.text, skind: s.kind });
    chars += s.text.length;
  }
  if (batch.length > 0) batches.push(batch);
  return { read: value.strings.length, batches: batches.map((units) => ({ kind: 'strings', units })) };
}

// --- the run ---------------------------------------------------------------------------------

/// The units of `script` an answer gave that pass the gates: a Map of n to text; and the n of the
/// others, by reason.
function checked(script, answer) {
  const good = new Map();
  const failed = new Map();
  const gateOf = (u, ko) => (u.kind === 'table' ? tableGate(u, ko) : u.kind === 'string' ? stringGate(u.text, ko) : gate(u.text, ko));
  for (const u of script.units) {
    if (u.done) continue;
    const ko = answer?.lines?.get(u.n);
    const why = ko === undefined ? 'missing' : gateOf(u, ko);
    if (why === null) good.set(u.n, ko);
    else failed.set(u.n, why);
  }
  return { good, failed };
}

/// Whether the pack read from <out> (`existing`, readPack's) was made from the files `bundles` were
/// exported from: each of its runs and table strings is one of theirs (its key, length and hash),
/// and its fingerprint is that of the scripts they name.
export function recheckOf(existing, bundles) {
  const byScript = new Map(bundles.filter(plain).map((b) => [`${b.archive}:${b.item}`, b]));
  const named = new Map();
  const same = (r, unitsOfB) => {
    const b = byScript.get(`${r.archive}:${r.item}`);
    const u = b && unitsOfB(b).find((x) => x.offset === r.offset);
    if (!u || latin1(u.text).length !== r.len || hex64(hash64(latin1(u.text))) !== r.hash) return false;
    named.set(`${r.archive}:${r.item}`, b);
    return true;
  };
  if (!existing.converse.every((r) => same(r, unitsOf))) return false;
  if (!existing.tables.every((t) => same(t, tablesOf))) return false;
  return fingerprint([...named.values()].map((b) => ({ archive: b.archive, item: b.item, hash: b.hash }))) === existing.base;
}

// A category's counts: units found, translated, left English by reason.
function tally(units) {
  const byReason = {};
  let translated = 0;
  for (const u of units) {
    if (u.ko !== undefined) translated += 1;
    else {
      const why = u.why ?? 'not-sent';
      byReason[why] = (byReason[why] ?? 0) + 1;
    }
  }
  return { found: units.length, translated, leftEnglish: units.length - translated, byReason, unexplained: 0 };
}

/// The pack and the coverage receipt of `plan`'s scripts and game strings, translated as their
/// units say, merged into `existing` (readPack's, or null): what this run translated replaces what
/// it had under the same key, the rest stays. The fingerprint is over the scripts the merged runs
/// and table strings name (their hashes from the bundles); with no bundles the conversation part
/// is the existing pack's, unchanged. No pack (null) when this run translated nothing.
export function outputsOf(plan, model, existing = null) {
  const runs = [];
  const tables = [];
  const strings = [];
  const left = [];
  const perScript = [];
  let translatedScripts = 0;
  for (const s of plan.scripts) {
    let done = 0;
    for (const u of s.units) {
      const at = { archive: s.bundle.archive, item: s.bundle.item, offset: u.offset, len: u.len, hash: u.hash };
      if (u.ko !== undefined) {
        done += 1;
        if (u.kind === 'table') tables.push({ ...at, capacity: u.capacity, text: u.ko });
        else runs.push({ ...at, text: u.ko });
      } else {
        left.push({ category: u.kind === 'table' ? 'tables' : 'conversations', hash: u.hash, reason: u.why ?? 'not-sent' });
      }
    }
    if (done > 0) translatedScripts += 1;
    const tablesHere = s.units.filter((u) => u.kind === 'table').length;
    perScript.push({ hash: s.bundle.hash, class: s.cls, units: s.units.length - tablesHere, tables: tablesHere, translated: done, left: s.units.length - done });
  }
  const stringUnits = (plan.strings?.batches ?? []).flatMap((b) => b.units);
  for (const u of stringUnits) {
    if (u.ko !== undefined) strings.push({ hash: u.hash, len: u.len, text: u.ko });
    else left.push({ category: 'strings', hash: u.hash, reason: u.why ?? 'not-sent' });
  }
  const runUnits = plan.scripts.flatMap((s) => s.units.filter((u) => u.kind !== 'table'));
  const tableUnits = plan.scripts.flatMap((s) => s.units.filter((u) => u.kind === 'table'));
  const coverage = {
    format: COVERAGE_FORMAT,
    model,
    tool: TOOL,
    category: 'conversations',
    scripts: { read: plan.read, usable: plan.usable, chosen: plan.scripts.length, translated: translatedScripts },
    units: tally(runUnits),
    tables: tally(tableUnits),
    strings: { read: plan.strings?.read ?? 0, ...tally(stringUnits) },
    perScript,
    left,
  };
  const key = (r) => `${r.archive}:${r.item}:${r.offset}`;
  const newRuns = new Set(runs.map(key));
  const newTables = new Set(tables.map(key));
  const newStrings = new Set(strings.map((t) => t.hash));
  const merged = {
    runs: [...(existing?.converse ?? []).filter((r) => !newRuns.has(key(r))), ...runs],
    tables: [...(existing?.tables ?? []).filter((t) => !newTables.has(key(t))), ...tables],
    strings: [...(existing?.strings ?? []).filter((t) => !newStrings.has(t.hash)), ...strings],
  };
  let base = existing?.base ?? fingerprint([]);
  if (plan.bundles) {
    const hashes = new Map(plan.bundles.filter(plain).map((b) => [`${b.archive}:${b.item}`, b.hash]));
    const named = new Map();
    for (const r of [...merged.runs, ...merged.tables]) named.set(`${r.archive}:${r.item}`, { archive: r.archive, item: r.item, hash: hashes.get(`${r.archive}:${r.item}`) });
    base = fingerprint([...named.values()]);
  }
  const changed = runs.length + tables.length + strings.length > 0;
  const pack = changed ? packBytes({ base, ...merged }) : null;
  return {
    pack, coverage, runs: runs.length, tables: tables.length, strings: strings.length,
    total: { runs: merged.runs.length, tables: merged.tables.length, strings: merged.strings.length },
  };
}

const USAGE = 'usage: node tools/i18n/translate.mjs [<bundles.json> (--item a:N | --name TEXT | --first N)] '
  + '[--strings <strings.json>] [--class CLASS] '
  + `[--send | --dry-run] [--model NAME] [--llmux URL] [--out DIR] [--jobs 1-${JOBS_MAX}] [--timeout SECONDS]`;

/// The options of `argv`, or {error}.
export function parseArgs(argv) {
  const o = { send: false, dryRun: false, model: DEFAULTS.model, llmux: DEFAULTS.llmux, out: DEFAULTS.out,
    jobs: DEFAULTS.jobs, timeoutS: DEFAULTS.timeoutS, bundles: null, strings: null, item: null, first: null,
    name: null, cls: null, help: false };
  const value = (i) => (i + 1 < argv.length ? argv[i + 1] : null);
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--send') o.send = true;
    else if (a === '--dry-run') o.dryRun = true;
    else if (a === '--help' || a === '-h') o.help = true;
    else if (['--model', '--llmux', '--out', '--jobs', '--timeout', '--item', '--first', '--class', '--name', '--strings'].includes(a)) {
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
      else if (a === '--name') o.name = v;
      else if (a === '--strings') o.strings = resolve(v);
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
  if (o.bundles === null && o.strings === null) return { error: 'no bundles file named, and no --strings' };
  const choices = [o.item, o.first, o.name].filter((c) => c !== null).length;
  if (o.bundles !== null && choices !== 1) return { error: 'name one script with --item or --name, or --first N' };
  if (o.bundles === null && choices > 0) return { error: '--item, --name and --first choose from a bundles file' };
  if (o.first !== null && (!Number.isInteger(o.first) || o.first < 1)) return { error: '--first is a number of scripts above 0' };
  if (o.name !== null && o.name.length === 0) return { error: '--name is an NPC\'s name' };
  if (o.cls !== null && !Object.hasOwn(CLASSES, o.cls)) return { error: `--class is one of ${Object.keys(CLASSES).join(', ')}` };
  if (!Number.isInteger(o.jobs) || o.jobs < 1 || o.jobs > JOBS_MAX) return { error: `--jobs is 1 to ${JOBS_MAX}` };
  if (!finite(o.timeoutS) || o.timeoutS <= 0) return { error: '--timeout is a number of seconds above 0' };
  if (!/^[\w.:[\]/-]{1,100}$/.test(o.model)) return { error: '--model is a model name' };
  return o;
}

const seconds = (ms) => `${(ms / 1000).toFixed(1)} s`;

// The JSON of the file at `path` (at most BUNDLES_MAX bytes) and its size; throws InputError with
// a fixed message (its kind, the file's role).
async function readJson(path, what) {
  let size;
  try {
    size = (await stat(path)).size;
  } catch (err) {
    throw new InputError(`the ${what} file cannot be read (${err?.code ?? err?.name ?? 'Error'})`);
  }
  if (size > BUNDLES_MAX) throw new InputError(`the ${what} file is ${size} bytes, more than ${BUNDLES_MAX}`);
  try {
    return { value: JSON.parse(await readFile(path, 'utf8')), size };
  } catch (err) {
    throw new InputError(err?.code ? `the ${what} file cannot be read (${err.code})` : `the ${what} file does not read as JSON`);
  }
}

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
  const stringsSystem = stringsSystemOf(guide);

  // The bundles and the game strings, and what to send of them.
  const plan = { read: 0, usable: 0, scripts: [], bundles: null, strings: null };
  try {
    if (o.bundles !== null) {
      const { value, size } = await readJson(o.bundles, 'bundles');
      Object.assign(plan, planOf(value, { item: o.item, first: o.first, name: o.name, cls: o.cls }));
      const units = plan.scripts.reduce((n, s) => n + s.units.filter((u) => u.kind === 'run').length, 0);
      const tables = plan.scripts.reduce((n, s) => n + s.units.filter((u) => u.kind === 'table').length, 0);
      const chars = plan.scripts.reduce((n, s) => n + s.units.reduce((m, u) => m + u.text.length, 0), 0);
      say(`Bundles: ${plan.read} scripts read from ${o.bundles} (${size} bytes), ${plan.usable} with lines; `
        + `${plan.scripts.length} chosen, ${units} lines, ${tables} table strings, ${chars} characters.`);
    }
    if (o.strings !== null) {
      const { value, size } = await readJson(o.strings, 'game strings');
      plan.strings = stringsPlanOf(value);
      const n = plan.strings.batches.reduce((m, b) => m + b.units.length, 0);
      const chars = plan.strings.batches.reduce((m, b) => m + b.units.reduce((k, u) => k + u.text.length, 0), 0);
      say(`Game strings: ${plan.strings.read} read from ${o.strings} (${size} bytes); ${n} to translate, ${chars} characters.`);
    }
  } catch (err) {
    if (err instanceof InputError) say(`translate: ${err.message}`);
    else say(`translate: the input does not read (${err?.name ?? 'Error'})`);
    return 2;
  }
  const jobOf = (g, units) => ({
    script: g,
    body: g.kind === 'strings' ? stringsBodyOf(o.model, stringsSystem, units) : bodyOf(o.model, system, g.bundle, g.cls, units),
  });
  const groups = [...plan.scripts, ...(plan.strings?.batches ?? [])];
  const jobs = groups.map((g) => jobOf(g, g.units.filter((u) => !u.done))).filter((j) => j.script.units.some((u) => !u.done));
  if (jobs.length === 0) {
    say('Nothing to translate: nothing was sent.');
    return 1;
  }

  // The boundary: the route named, the first payload written, and the owner's go.
  say(`Route: llmux on this machine (${route.origin}), then ${providerOf(o.model)} (model ${o.model}).`);
  const what = [plan.scripts.length > 0 ? 'one script\'s lines with its NPC\'s name and description' : null,
    plan.strings ? `up to ${STRINGS_PER_REQUEST} of the game's own strings` : null].filter(Boolean).join(', or ');
  say(`It sends the owner's game text: ${jobs.length} requests, each ${what}; up to ${o.jobs} at a time, `
    + 'and a request again for lines that fail the checks.');
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
    // The pack already there, merged into: read strictly, checked against the bundles again.
    let existing = null;
    const packPath = join(o.out, 'ko.u6lp');
    try {
      const bytes = await readFile(packPath);
      existing = readPack(bytes);
    } catch (err) {
      if (err instanceof PackReadError) {
        say(`translate: the pack in ${o.out} does not read (${err.message}): move it away to make a new one; nothing was sent.`);
        return 2;
      }
      if (err?.code !== 'ENOENT') {
        say(`translate: the pack in ${o.out} cannot be read (${err?.code ?? err?.name ?? 'Error'}): nothing was sent.`);
        return 2;
      }
    }
    if (existing) {
      if (existing.language !== 'ko') {
        say(`translate: the pack in ${o.out} declares another language than ko: move it away to make a new one; nothing was sent.`);
        return 2;
      }
      if (plan.bundles && !recheckOf(existing, plan.bundles)) {
        say(`translate: the pack in ${o.out} was made from other game files (its conversation part): move it away to make a new one; nothing was sent.`);
        return 2;
      }
      say(`Pack: ${packPath} holds ${existing.converse.length} lines, ${existing.tables.length} table strings and `
        + `${existing.strings.length} game strings; what this run translates is merged into it.`);
    }
    return await consentAndSend(o, plan, jobs, jobOf, existing, route, { transport, stdin, stdout, isTTY, signals, now, say });
  } finally {
    await release();
  }
}

// From the preview to the last file, with `o.out`'s lock held (tools/speech/profile.mjs's order):
// the preview of this run (a name of its own), the owner's go, the requests, the gates, the
// files.
async function consentAndSend(o, plan, jobs, jobOf, existing, route, { transport, stdin, stdout, isTTY, signals, now, say }) {
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

  // The requests: each script and each batch of game strings once (asked again once when the
  // request fails or its answer does not read), then once more for its lines that failed the
  // gates.
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
        if (u.done) continue;
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
  const out = outputsOf(plan, o.model, existing);
  const { coverage } = out;
  const reasons = (c) => {
    const why = Object.entries(c.byReason).sort().map(([r, n]) => `${r} ${n}`).join(', ');
    return why ? ` (${why})` : '';
  };
  if (plan.scripts.length > 0) {
    const c = coverage.units;
    say(`Lines: ${c.found} found, ${c.translated} translated, ${c.leftEnglish} left English${reasons(c)}; `
      + `scripts translated ${coverage.scripts.translated} of ${coverage.scripts.chosen}.`);
    const t = coverage.tables;
    if (t.found > 0) say(`Table strings: ${t.found} found, ${t.translated} translated, ${t.leftEnglish} left English${reasons(t)}.`);
  }
  if (plan.strings) {
    const c = coverage.strings;
    say(`Game strings: ${c.found} found, ${c.translated} translated, ${c.leftEnglish} left English${reasons(c)}.`);
  }
  // The whole merged pack through the reader before anything is replaced: one past the reader's
  // limits (the size, the lines) would be refused by the page and by the next run.
  if (out.pack) {
    try {
      readPack(out.pack);
    } catch (err) {
      if (!(err instanceof PackReadError)) throw err;
      say(`translate: the merged pack would not read (${err.message}): the pack there and coverage.json are left as they were; nothing was written.`);
      return 2;
    }
  }
  const written = [];
  if (out.pack) {
    const path = join(o.out, 'ko.u6lp');
    written.push(`${path} (${await writePrivate(path, out.pack)} bytes: ${out.total.runs} lines, ${out.total.tables} table strings, `
      + `${out.total.strings} game strings; ${out.runs + out.tables + out.strings} from this run)`);
  }
  const cpath = join(o.out, 'coverage.json');
  written.push(`${cpath} (${await writePrivate(cpath, JSON.stringify(coverage, null, 1))} bytes)`);
  say(`Wrote ${written.join(', ')}.`);
  if (out.pack) say('Drop ko.u6lp on the page and choose 한글; coverage.json stays here.');
  else say(existing ? 'No line was translated: the pack there is left as it was.' : 'No line was translated: no pack was written.');
  if (stopper.signal.aborted) return 130;
  if (!out.pack) return 3;
  const leftEnglish = coverage.units.leftEnglish + coverage.tables.leftEnglish + coverage.strings.leftEnglish;
  return leftEnglish > 0 ? 3 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const exit = (code) => process.stdout.write('', () => process.exit(code));
  main(process.argv.slice(2)).then(exit, (err) => {
    // Its kind only: a message could quote what was read.
    process.stdout.write(`translate: failed (${err?.name ?? 'Error'})\n`);
    exit(1);
  });
}

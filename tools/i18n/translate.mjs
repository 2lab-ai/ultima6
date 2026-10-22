#!/usr/bin/env node
// The Korean pack's translator (spec 08 Design 6; slice K1: conversation runs; slice K2: the
// conversations' table strings and the game's own messages; slice K3: the object names and the
// books and signs; slice K4: the u program's opening, story, gypsy, character creation and
// transfer; slice K5b: the keyword aliases). It reads the NPC bundles the page downloads
// (ultima6-npc-bundles.json, one u6.npc-bundle/1 per conversation script, each text segment keyed
// to its run, each table string to its offset), and the game strings the page's second export
// writes (ultima6-strings.json, u6.string-bundle/1: the executable's strings the engine shows,
// each keyed by its hash, its
// `texts`: the object names of look.lzd and the books and signs of book.dat, each keyed by its
// index with its length and hash, and its `u`: the strings the u program prints, each keyed by
// its hash, with its kind, the place u prints it), translates what is chosen into archaic Korean
// (사극체, tools/i18n/style-ko.md) through a model, and writes the pack the page takes in a drop,
// beside a coverage receipt:
//
//   <out>/ko.u6lp        the pack: `U6LP`, a line feed, then {format: u6.locale-pack/1, language:
//                        ko, tool, base: {converse: <the fingerprint of its scripts>}, converse:
//                        [{archive, item, offset, len, hash, text}], and when there are any
//                        tables: [{archive, item, offset, len, hash, capacity?, text}],
//                        strings: [{hash, len, text}], look: [{index, len, hash, text}],
//                        books: [{index, len, hash, text}], u: [{hash, len, text}] and
//                        aliases: [{archive, item, keyword, words}]} (u6_core::locale)
//   <out>/coverage.json  u6.locale-coverage/1: the units found, translated and left English by
//                        reason, by category, scripts and units named by their hashes only
//
// <out> is ~/Games/ultima6/i18n/ko/ unless --out names another; it and preview/ are made 0700,
// every file in them 0600; either one that is a symbolic link (or not a directory) is refused with
// no request, its target left as it is (tools/speech/profile.mjs privateDir).
//
// A run MERGES into the pack already in <out>: that pack is read through the tool's strict reader
// (readPack, the engine's rules) and, when bundles are given, its conversation part is checked
// against them again (each run and table string still the bundles', the fingerprint theirs), and
// when the game strings file lists object names or books, its own are checked against them (each
// one of the file's, at its index, of its length and hash, as the engine checks them at install);
// one that does not read or check is refused with no request. What this run translates replaces
// what the pack had under the same key; the rest stays. A run that translates nothing leaves it as
// it is. The merged pack is read through the same reader before it replaces anything: one the
// reader would refuse (past its size or its count of lines) is not written, the pack there and
// coverage.json left as they were.
//
// What is chosen: from the bundles, one script, `--item <archive>:<item>` (the archive letter and
// item the bundle has) or `--name <text>` (the bundle whose NPC name is that, case folded: one
// match, else a refusal that counts them; the name is never printed), or the first N scripts with
// lines in (archive, item) order, `--first N`; and with `--strings <file>`, every game string,
// object name, book and string of u it lists, or only the parts `--only` names (strings, look,
// books, u).
// One request per script: the style guide, the speaker's class and register (guessed from its
// name and description, Pass A of spec 08 §6; `--class` gives it instead), the NPC's name and
// description, every run of its script in order, escapes kept, and its table strings (a table
// string an op reads from inside it is left English, `inner`).
// Spec 12, the register by profile: `--speakers <file>` (u6.speakers/1, tools/i18n/speakers.mjs's,
// read strictly, its `coverage` receipt passed over: one that does not read is refused with no
// request) gives a script it lists its class (`--class`, given, still wins for the run), its age
// (child, youth, adult or elder) and its personality note; the request's NPC then also names its
// `age` and its `profile` (the note), and its register is the one for that class at that age
// (registerOf). A script the file does not list takes Pass A, `adult` and an empty profile. `--ages a,b` (with `--speakers` only) chooses the
// scripts of those ages, a script not listed counting as adult: all of them, or with `--first N`
// the first N of them. Without `--speakers` every request is byte for byte as before.
// The game strings go in requests of at most STRINGS_PER_REQUEST strings and STRING_CHARS
// characters, the object names in requests of at most LOOK_PER_REQUEST names and LOOK_CHARS
// characters, the books in requests of at most BOOKS_PER_REQUEST books and BOOK_CHARS characters
// (a longer book alone), the strings of u in requests of at most U_PER_REQUEST strings and U_CHARS
// characters, in the order u prints them; a name or a book with no letter to translate (outside
// its runes, escapes and plural markers), or a string of u with none outside its printf codes, is
// left English unasked.
// A script's request also lists its keywords (slice K5b, spec 08 §4): `topics`, one entry per
// keyword block in script order, each key (the keyword as the keyword block keys it, up to its NUL
// and case-folded; a `*` keyword is none) once, in the first block that lists it, with the words
// its lines highlight that hit it (`shown`), the Korean word already settled for it (`known`: the
// script's own in the pack there, its alias's first word the alias gate takes, never a word another
// script gives the key, as the engine looks a word up by script and key; else the shared keywords'
// request's) and, for a key `y` or `n` its first block lists alone, the reply's yes or no it is
// (`answer`: such a key is sent its known word only when that is 예 or 아니, so the mark decides;
// a line of it asked again names the word its key keeps, without the mark when that word is
// another); the answer gives one Korean word per key (`keywords`), checked by the alias gate (1
// to 8 Hangul syllables; one entry per key, or several that give it the same word; two keys whose
// words collide, a word of the input the engine takes for both, refused unless their first block
// is the same, and so is a word colliding with one the pack keeps for another key), and the words
// kept become the pack's `aliases` for that script.
// Before the scripts, the keys at least GLOSSARY_SCRIPTS of the chosen scripts hold (and one of
// them has no word of its own for) are asked once in a request of their own, so a key many NPCs
// have gets one word where they have none; never a key of one letter (a word longer than a key
// hits it: over the scripts it would gather every highlighted word that starts so). With
// `--fresh-keywords` no word of the pack there is sent as `known` in a script's first request
// (only the shared keywords' request's, which then asks every key that many scripts hold), so
// each script answers its keys from its own lines; a line asked again names the word its key
// keeps (the pack's where this run kept none), as in any run; the words a script's answer keeps
// (past the alias gate) replace the pack's for its keys, as any run's do (a key whose word is
// refused or missing, and every key of a script whose request fails, keeps its own); the consent
// text and the coverage (`freshKeywords`) say so. A request of a script with no key is the one of
// K1 to K4, byte for byte, for the same style guide (which goes in every request, and which K5b
// changes).
// The answer is JSON only, {"lines": [{"n", "ko"}]} (and "keywords" for a script with keys).
// Each line is checked by the mechanical gates of spec 08 §6 (structure only: the escapes'
// multiset, Hangul syllables and ASCII only, the josa markers and every `@{` closed as the text
// layer reads them, each `@{…}` the Korean word of the key its English `@` word hits, or that
// English word for a key with no word or for none, the runic text and the line breaks at its
// ends kept; for a table string the engine copies into a slot, its capacity and no marker;
// for a game string the same `%` directives in the same order and the reader's length; for an
// object name no plural marker (the Korean rule drops `/` and `\` with the letters after them,
// so a Korean name holds none and shows one form), no josa marker or `@{` (it is put inside other
// text), the directives, the reader's length and a Hangul syllable; for a book the line's gates,
// the directives, a text under the game's book buffer (0x2800 bytes) and a Hangul syllable; for a
// string of u, which u's own printf prints with no escape layer and no particle resolver, the same
// printf codes in the same order (%B %F %X %Y %P included), no josa marker or `@{`, as many line
// breaks at its start, at its end and in all, for the prompt of a key read the keys its English
// shows standing alone, standing alone in the same order, the reader's length and a Hangul
// syllable); a line that fails (or that the answer lacks) is asked again once, in a request of
// the failed lines, then left English and counted. A request that fails or whose answer does not
// read is asked again once.
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
//   node tools/i18n/translate.mjs [<bundles.json> (--item a:N | --name TEXT | --first N
//        | --ages AGES [--first N]) [--speakers <speakers.json>] [--fresh-keywords]]
//        [--strings <strings.json> [--only strings,look,books,u]] [--class CLASS]
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
export const TOOL = 'tools/i18n/translate.mjs/5';
export const MAX_TOKENS = 32000; // the answer's cap (a reasoning model's thinking counts in it)
export const PACK_FORMAT = 'u6.locale-pack/1';
export const COVERAGE_FORMAT = 'u6.locale-coverage/1';
export const MAGIC = 'U6LP';
export const SCRIPT_LEN = 0x2800; // u6_core::converse::SCRIPT_LEN
export const MAX_TEXT_BYTES = 3 * SCRIPT_LEN; // u6_core::locale::MAX_TEXT_BYTES
export const STRINGS_FORMAT = 'u6.string-bundle/1'; // u6_core::locale::strings::FORMAT
export const MIN_STRING_BYTES = 3; // u6_core::locale::strings::MIN_BYTES (a label's is 1)
export const MAX_STRING_BYTES = 256; // u6_core::locale::strings::MAX_BYTES
export const MAX_STRING_TEXT_BYTES = 3 * MAX_STRING_BYTES; // u6_core::locale::MAX_STRING_TEXT_BYTES
export const MAX_PACK_BYTES = 16 << 20; // u6_core::locale::MAX_PACK_BYTES
export const MAX_RUNS = 1 << 16; // u6_core::locale::MAX_RUNS (and MAX_STRINGS)
export const STRINGS_PER_REQUEST = 60;
export const STRING_CHARS = 4000; // the English characters of one request of game strings
export const LOOK_ENTRIES = 2048; // u6_core::locale::texts::LOOK_ENTRIES
export const MAX_LOOK_BYTES = 256; // u6_core::locale::texts::MAX_LOOK_BYTES
export const MAX_LOOK_TEXT_BYTES = 3 * MAX_LOOK_BYTES; // u6_core::locale::texts::MAX_LOOK_TEXT_BYTES
export const BOOK_BYTES = 0x2800; // u6_core::locale::texts::BOOK_BYTES (a book's text is under it)
export const BOOK_INDICES = 255; // u6_core::locale::texts::BOOK_INDICES
export const LOOK_PER_REQUEST = 80;
export const LOOK_CHARS = 3000; // the English characters of one request of object names
export const BOOKS_PER_REQUEST = 8;
export const BOOK_CHARS = 6000; // the English characters of one request of books
export const MIN_U_BYTES = 3; // u6_core::locale::program::MIN_BYTES
export const MAX_U_BYTES = 511; // u6_core::locale::program::MAX_BYTES
export const MAX_U_TEXT_BYTES = 3 * MAX_U_BYTES; // u6_core::locale::program::MAX_TEXT_BYTES
export const U_KINDS = ['opening', 'story', 'gypsy', 'creation', 'transfer']; // u6_core::locale::program::Role
export const U_PER_REQUEST = 40;
export const U_CHARS = 4000; // the English characters of one request of strings of u
export const PARTS = ['strings', 'look', 'books', 'u'];
export const MAX_ALIASES = 1 << 16; // u6_core::locale::MAX_ALIASES
export const MAX_ALIAS_WORDS = 4; // u6_core::locale::MAX_ALIAS_WORDS
export const MAX_ALIAS_CHARS = 8; // u6_core::locale::MAX_ALIAS_CHARS
export const MAX_KEYWORD_BYTES = 32; // u6_core::locale::MAX_KEYWORD_BYTES
export const GLOSSARY_SCRIPTS = 3; // the chosen scripts a key is in to be asked before them
export const GLOSSARY_KEYS = 64;
export const GLOSSARY_SHOWN = 8;
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

/// The speakers' ages (spec 12): the age decides the speech level first, then the class.
export const AGES = ['child', 'youth', 'adult', 'elder'];
export const SPEAKERS_FORMAT = 'u6.speakers/1';
export const SPEAKER_NOTE_MAX = 400; // the characters of a speaker's note the reader takes
const SPEAKERS_MAX = 1 << 16; // the entries of a speaker file

// The stations whose young speak 해요체 (an apprentice, a young servant); the young of the others
// speak a soft 하오체 (a young noble, a squire). Spec 12 Design 3.
const PLAIN_STATIONS = new Set(['merchant', 'commoner', 'thief']);

/// The register a request names for a speaker of class `cls` at age `age` (spec 12 Design 3, the
/// guide's ages): a child's speech whatever its class; a young person's lighter level by station;
/// the class's level with an old person's turns; an adult's is the class's (CLASSES).
export function registerOf(cls, age = 'adult') {
  if (age === 'child') {
    return 'a child\'s speech: short sentences in easy native words, none of the archaic adult endings; '
      + '해요체 (~요, ~어요) to a grown-up, 반말 to another child; the hero never 그대';
  }
  if (age === 'youth') {
    return PLAIN_STATIONS.has(cls) ? '해요체 (~요, ~어요), a young person\'s lighter level'
      : 'a soft 하오체 (~하오, ~이오), a young person\'s lighter level with fewer archaic endings';
  }
  if (age === 'elder') return `${CLASSES[cls]}, with an old person's turns (~구먼, ~하네, ~일세)`;
  return CLASSES[cls];
}

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
/// table strings in key order, the game strings in hash order, the object names and books in index
/// order, the strings of u in hash order, the keyword aliases by script then keyword; `tables`,
/// `strings`, `look`, `books`, `u` and `aliases` written only when there are any (a pack of runs
/// only is K1's file, one without names and books K2's, one without strings of u K3's, one without
/// aliases K4's). `runs`: [{archive, item, offset, len, hash, text}]; `tables`: the same with
/// `capacity` (or null); `strings` and `u`: [{hash, len, text}]; `look` and `books`: [{index, len,
/// hash, text}]; `aliases`: [{archive, item, keyword, words}].
export function packBytes({ language = 'ko', tool = TOOL, base, runs, tables = [], strings = [], look = [], books = [], u = [], aliases = [] }) {
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
  const byHash = (list) => [...list].sort((x, y) => (x.hash < y.hash ? -1 : x.hash > y.hash ? 1 : 0))
    .map((t) => ({ hash: t.hash, len: t.len, text: t.text }));
  if (strings.length > 0) json.strings = byHash(strings);
  const entries = (list) => [...list].sort((x, y) => x.index - y.index).map((t) => ({ index: t.index, len: t.len, hash: t.hash, text: t.text }));
  if (look.length > 0) json.look = entries(look);
  if (books.length > 0) json.books = entries(books);
  if (u.length > 0) json.u = byHash(u);
  if (aliases.length > 0) {
    const order = (x, y) => (x.archive !== y.archive ? (x.archive < y.archive ? -1 : 1) : x.item !== y.item ? x.item - y.item
      : x.keyword < y.keyword ? -1 : x.keyword > y.keyword ? 1 : 0);
    json.aliases = [...aliases].sort(order).map((a) => ({ archive: a.archive, item: a.item, keyword: a.keyword, words: [...a.words] }));
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

/// Where the JSON `text` (already parsed) gives one member twice in an object: JSON.parse keeps
/// the last, the engine's reader (serde) refuses the file. `aliases`: inside the root's `aliases`
/// value (the engine reads that part on its own, so it is that part's refusal); `outside`:
/// anywhere else (the root's own members included). Keys are compared decoded
/// ("conv\u0065rse" is "converse").
export function duplicatePlaces(text) {
  const found = { outside: false, aliases: false };
  const stack = [];
  let member = null; // the root's member being read
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    if (c === '"') {
      let j = i + 1;
      while (text[j] !== '"') j += text[j] === '\\' ? 2 : 1;
      const top = stack.at(-1);
      if (top && top.keys && top.expectKey) {
        const key = JSON.parse(text.slice(i, j + 1));
        if (top.keys.has(key)) found[top.under === 'aliases' ? 'aliases' : 'outside'] = true;
        top.keys.add(key);
        top.expectKey = false;
        if (stack.length === 1) member = key;
      }
      i = j + 1;
      continue;
    }
    if (c === '{' || c === '[') {
      const under = stack.length === 0 ? null : stack.length === 1 ? member : stack.at(-1).under;
      stack.push(c === '{' ? { keys: new Set(), expectKey: true, under } : { keys: null, under });
    } else if (c === '}' || c === ']') stack.pop();
    else if (c === ',' && stack.at(-1)?.keys) stack.at(-1).expectKey = true;
    i += 1;
  }
  return found;
}

/// Whether the JSON `text` (already parsed) gives one member twice in an object, anywhere.
export function duplicateMembers(text) {
  const { outside, aliases } = duplicatePlaces(text);
  return outside || aliases;
}

/// Slice K5a's rules for the aliases part (u6_core::locale): whether `key` is a key of the
/// aliases (`keyword_ok`: 1 to MAX_KEYWORD_BYTES bytes of printable ASCII, no `,`, no upper case),
/// and whether `word` is an alias word (`alias_word_ok`: 1 to MAX_ALIAS_CHARS characters, each a
/// Hangul syllable or printable ASCII but the space).
export const keywordOk = (key) => typeof key === 'string' && key.length >= 1 && key.length <= MAX_KEYWORD_BYTES
  && /^[\x20-\x2b\x2d-\x40\x5b-\x7e]+$/.test(key);
const aliasWordOk = (w) => typeof w === 'string' && [...w].length >= 1 && [...w].length <= MAX_ALIAS_CHARS
  && [...w].every((c) => (c > ' ' && c < '\x7f') || (c >= '가' && c <= '힣'));
const ALIASES_PART = 'its keyword aliases part (aliases) holds an entry of another form or for a script its lines do not name';

/// Reads a pack file as the engine's reader does (u6_core::locale::Pack::parse): the size, the
/// magic, JSON of exactly the pack's fields (each once), the format, the language, the tool, every hash 16
/// hex digits, the limits, each key inside the script buffer and once, each text one the game
/// can show (a table string's within its capacity, a game string's within the reader's length),
/// each object name keyed inside the look table and each book by a quality, a book's text under the
/// game's book buffer, each string of u within its reader's lengths, each keyword alias of the
/// aliases part's form, for a script the runs or table strings name, its keyword once per script
/// (anything else of that part, a member twice inside it included, is that part's refusal).
/// Returns {language, tool, base, converse, tables, strings, look, books, u, aliases}; throws
/// PackReadError.
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
  const twice = duplicatePlaces(text);
  if (twice.outside) fail(shape);
  if (!exact(v, ['format', 'language', 'tool', 'base', 'converse'], ['tables', 'strings', 'look', 'books', 'u', 'aliases'])
    || typeof v.format !== 'string' || typeof v.language !== 'string' || typeof v.tool !== 'string'
    || !exact(v.base, ['converse']) || typeof v.base.converse !== 'string' || !Array.isArray(v.converse)
    || ['tables', 'strings', 'look', 'books', 'u'].some((k) => v[k] !== undefined && !Array.isArray(v[k]))) fail(shape);
  const runKeys = ['archive', 'item', 'offset', 'len', 'hash', 'text'];
  const keyed = (r, optional = []) => exact(r, runKeys, optional) && typeof r.archive === 'string' && Number.isInteger(r.item)
    && r.item >= 0 && r.item <= 0xffff && u32(r.offset) && u32(r.len) && typeof r.hash === 'string' && typeof r.text === 'string'
    && (r.capacity === undefined || u32(r.capacity));
  const tables = v.tables ?? [];
  const strings = v.strings ?? [];
  const look = v.look ?? [];
  const books = v.books ?? [];
  const u = v.u ?? [];
  const entry = (t) => exact(t, ['index', 'len', 'hash', 'text']) && u32(t.index) && u32(t.len) && typeof t.hash === 'string' && typeof t.text === 'string';
  const hashed = (t) => exact(t, ['hash', 'len', 'text']) && typeof t.hash === 'string' && u32(t.len) && typeof t.text === 'string';
  if (!v.converse.every((r) => keyed(r)) || !tables.every((t) => keyed(t, ['capacity']))
    || !strings.every(hashed) || !look.every(entry) || !books.every(entry) || !u.every(hashed)) fail(shape);
  if (v.format !== PACK_FORMAT) fail(`it is not of this page's pack format (${PACK_FORMAT})`);
  if (!['en', 'ko'].includes(v.language)) fail('it declares a language this page does not have');
  if (v.tool.length === 0 || v.tool.length > 64 || !/^[\x20-\x7e]+$/.test(v.tool)) fail('it does not name the tool that made it');
  if (!hex16(v.base.converse)) fail('a hash in it is not 16 hex digits');
  if (v.converse.length > MAX_RUNS || tables.length > MAX_RUNS || strings.length > MAX_RUNS
    || look.length > LOOK_ENTRIES || books.length > BOOK_INDICES || u.length > MAX_RUNS) fail('it holds more lines than a pack can');
  // The aliases part, read on its own as the engine reads it: its form here, the rest after the
  // other parts (u6_core::locale::Pack::with_aliases comes last).
  const aliasForm = (a) => exact(a, ['archive', 'item', 'keyword', 'words']) && typeof a.archive === 'string'
    && Number.isInteger(a.item) && a.item >= 0 && a.item <= 0xffff && typeof a.keyword === 'string'
    && Array.isArray(a.words) && a.words.every((w) => typeof w === 'string');
  if (v.aliases !== undefined && (twice.aliases || !Array.isArray(v.aliases) || !v.aliases.every(aliasForm))) fail(ALIASES_PART);
  const aliases = v.aliases ?? [];
  if (aliases.length > MAX_ALIASES) fail(ALIASES_PART);
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
    // A pack string holds no kind: one of 1 or 2 bytes is a label's (the engine's reader keeps it).
    if (t.len < 1 || t.len > MAX_STRING_BYTES || Buffer.byteLength(t.text) > MAX_STRING_TEXT_BYTES) fail('a game string in it is not one the game prints');
    if (!textOk(t.text, MAX_STRING_TEXT_BYTES)) fail('a line in it holds a character the game cannot show');
    if (seen.has(t.hash)) fail('it holds one line twice');
    seen.add(t.hash);
  }
  // The engine reads every hash of the names, the books and the strings of u before it checks
  // any of them.
  if (![...look, ...books, ...u].every((t) => hex16(t.hash))) fail('a hash in it is not 16 hex digits');
  seen.clear();
  for (const t of look) {
    if (t.index >= LOOK_ENTRIES || t.len < 1 || t.len > MAX_LOOK_BYTES || Buffer.byteLength(t.text) > MAX_LOOK_TEXT_BYTES) fail('an object name in it is not one the game\'s look table holds');
    if (!textOk(t.text, MAX_LOOK_TEXT_BYTES)) fail('a line in it holds a character the game cannot show');
    if (seen.has(t.index)) fail('it holds one line twice');
    seen.add(t.index);
  }
  seen.clear();
  for (const t of books) {
    if (t.index >= BOOK_INDICES || t.len < 1 || t.len > BOOK_BYTES) fail('a book in it is not one the game\'s book file holds');
    if (Buffer.byteLength(t.text) >= BOOK_BYTES) fail('a book in it is longer than the game reads a book');
    if (!textOk(t.text, BOOK_BYTES)) fail('a line in it holds a character the game cannot show');
    if (seen.has(t.index)) fail('it holds one line twice');
    seen.add(t.index);
  }
  seen.clear();
  for (const t of u) {
    if (t.len < MIN_U_BYTES || t.len > MAX_U_BYTES || Buffer.byteLength(t.text) > MAX_U_TEXT_BYTES) fail('an opening or creation string in it is not one the u program prints');
    if (!textOk(t.text, MAX_U_TEXT_BYTES)) fail('a line in it holds a character the game cannot show');
    if (seen.has(t.hash)) fail('it holds one line twice');
    seen.add(t.hash);
  }
  seen.clear();
  const named = new Set([...v.converse, ...tables].map((r) => `${r.archive}:${r.item}`));
  for (const a of aliases) {
    if (!['a', 'b'].includes(a.archive) || !named.has(`${a.archive}:${a.item}`) || !keywordOk(a.keyword)
      || a.words.length < 1 || a.words.length > MAX_ALIAS_WORDS || !a.words.every(aliasWordOk)) fail(ALIASES_PART);
    const k = `${a.archive}:${a.item}:${a.keyword}`;
    if (seen.has(k)) fail(ALIASES_PART);
    seen.add(k);
  }
  return { language: v.language, tool: v.tool, base: v.base.converse, converse: v.converse, tables, strings, look, books, u, aliases };
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

// --- the keyword aliases (slice K5b, spec 08 §4) ----------------------------------------------

/// The key of a keyword as a bundle holds it (the script's bytes, one character each) as the
/// keyword block keys it (crates/u6-core/src/converse/run.rs `keywords`): its bytes up to a NUL,
/// ASCII upper case folded to lower, `?` kept; null when it is no key an alias can have (not
/// [`keywordOk`]) or it starts with `*` (such a keyword matches any input).
export function keyOf(keyword) {
  if (typeof keyword !== 'string') return null;
  const nul = keyword.indexOf('\0');
  const key = (nul < 0 ? keyword : keyword.slice(0, nul)).replace(/[A-Z]/g, (c) => c.toLowerCase());
  return keywordOk(key) && key[0] !== '*' ? key : null;
}

/// Whether highlighted word `word` (case folded) hits key `key` as the keyword test does
/// (run.rs `match_keyword`): each of the key's bytes is the word's at that place or `?`, so a word
/// may be longer than its key; past the word's end only a `?` matches. An empty word hits none.
export function hits(word, key) {
  if (word.length === 0) return false;
  for (let i = 0; i < key.length; i += 1) if (key[i] !== '?' && key[i] !== word[i]) return false;
  return true;
}

/// The key of `keys` (in the keyword blocks' order) that `word` hits first, as the game's first
/// match takes it, or null.
export const keyHit = (word, keys) => keys.find((k) => hits(word, k)) ?? null;

/// The keys of `bundle`'s keyword blocks: `topics`, the keys of each block that has any, in script
/// order (its turns, then their topics), each once; `keys`, each key once, in the order it first
/// comes (so the first of them a word hits is the game's first match).
export function keysOf(bundle) {
  const topics = [];
  const keys = [];
  for (const turn of Array.isArray(bundle.turns) ? bundle.turns : []) {
    for (const topic of Array.isArray(turn?.topics) ? turn.topics : []) {
      const list = [];
      for (const k of Array.isArray(topic?.keywords) ? topic.keywords : []) {
        const key = keyOf(k);
        if (key !== null && !list.includes(key)) list.push(key);
      }
      if (list.length === 0) continue;
      topics.push(list);
      for (const key of list) if (!keys.includes(key)) keys.push(key);
    }
  }
  return { topics, keys };
}

/// The highlighted English words of `units` that hit each of `keys`: a Map of key to its words
/// (case folded), each once, in the lines' order; a word that hits several keys is its first's.
export function shownOf(units, keys) {
  const shown = new Map(keys.map((k) => [k, []]));
  for (const u of units) {
    for (const w of keywordsOf(u.text)) {
      const k = keyHit(w, keys);
      if (k !== null && !shown.get(k).includes(w)) shown.get(k).push(w);
    }
  }
  return shown;
}

/// The particles (조사) the engine takes after a keyword's alias, longest first
/// (crates/u6-core/src/converse/run.rs `PARTICLES`; a test reads that file and compares).
export const PARTICLES = [
  '에게', '에서', '으로', '이여', '은', '는', '이', '가', '을', '를', '와', '과', '의', '에',
  '로', '도', '만', '께', '아', '야', '여',
];

/// The words of the input the engine takes for alias word `word` (run.rs `match_alias`): the word
/// itself and the word followed by exactly one particle, ASCII case folded.
export const formsOf = (word) => [word, ...PARTICLES.map((p) => word + p)].map((w) => w.replace(/[A-Z]/g, (c) => c.toLowerCase()));

/// Whether two alias words collide: some word of the input the engine takes for one it takes for
/// the other too (달 and 달이: 달이 is 달 with the particle 이).
export function collide(a, b) {
  const forms = new Set(formsOf(a));
  return formsOf(b).some((w) => forms.has(w));
}

/// The first keyword block of `topics` (the script's, in its order) that lists each key: a Map of
/// key to that block's index.
export function firstBlocksOf(topics) {
  const first = new Map();
  topics.forEach((list, i) => list.forEach((k) => { if (!first.has(k)) first.set(k, i); }));
  return first;
}

/// Whether two keys of a script (its `topics`) may keep colliding words, as a function of two
/// keys: only when their first block is the same (the game's first match then reaches that block
/// for the word of either and for the English of either).
export function compatibleOf(topics) {
  const first = firstBlocksOf(topics);
  return (a, b) => first.has(a) && first.get(a) === first.get(b);
}

/// Why `ko` is not a word a player may type for a keyword, or null: 1 to MAX_ALIAS_CHARS Hangul
/// syllables (U+AC00..=U+D7A3) and nothing else (no ASCII, no space; the engine takes ASCII too,
/// the tool gives Korean only).
export function wordGate(ko) {
  if (typeof ko !== 'string' || ko.length === 0) return 'empty';
  if (![...ko].every((c) => c >= '가' && c <= '힣')) return 'characters';
  if ([...ko].length > MAX_ALIAS_CHARS) return 'long';
  return null;
}

/// The words an answer's `keywords` ([{k, ko}]) give `keys` that pass the alias gate: a Map of key
/// to word; and the others' reasons by key: `missing` (none given), `repeated` (given two different
/// words; entries that give a key the same word count as one), the word's ([`wordGate`]), `shared`
/// (a word that collides with another key's ([`collide`]) where `compatible(a, b)` does not let the
/// two keep it: the player could not tell them apart). `kept` (a Map of key to its words): the
/// words the pack there keeps for the script's other keys; a new word colliding with one of a key
/// this answer gives none is refused too (`shared`), and a key so refused keeps its own old words,
/// which the others are then checked against. An entry for a key not asked is not read.
export function aliasesOf(keys, given, compatible, kept = new Map()) {
  const list = (Array.isArray(given) ? given : []).filter((e) => plain(e) && typeof e.k === 'string' && keys.includes(e.k))
    .filter((e, i, all) => all.findIndex((x) => x.k === e.k && x.ko === e.ko) === i);
  const count = new Map();
  for (const e of list) count.set(e.k, (count.get(e.k) ?? 0) + 1);
  const words = new Map();
  const failed = new Map();
  for (const e of list) {
    if (count.get(e.k) > 1) continue;
    const why = wordGate(e.ko);
    if (why) failed.set(e.k, why);
    else words.set(e.k, e.ko);
  }
  for (const k of keys) {
    if ((count.get(k) ?? 0) > 1) failed.set(k, 'repeated');
    else if (!count.has(k)) failed.set(k, 'missing');
  }
  const clash = (k, j, ws) => j !== k && !compatible(k, j) && ws.some((v) => collide(words.get(k), v));
  const shared = [...words.keys()].filter((k) => [...words].some(([j, v]) => clash(k, j, [v])));
  for (const k of shared) {
    words.delete(k);
    failed.set(k, 'shared');
  }
  // Against the words the pack keeps for keys this answer does not replace, until none collides.
  for (let again = true; again;) {
    again = false;
    for (const k of [...words.keys()]) {
      if ([...kept].some(([j, ws]) => !words.has(j) && clash(k, j, ws))) {
        words.delete(k);
        failed.set(k, 'shared');
        again = true;
      }
    }
  }
  return { words, failed };
}

/// The brace words the `@` keywords of English `en` call for, case folded, in order: for each
/// highlighted word, the words it may show, those `keyed.words` gives the key of `keyed.keys` it
/// hits first (a word, or a list: any of a key's words), or the English word itself when that key
/// has none or it hits no key; with no `keyed` (no script, no aliases), the English words.
export function braceWordsWanted(en, keyed = null) {
  const fold = (w) => w.toLowerCase();
  return keywordsOf(en).map((w) => {
    const k = keyed ? keyHit(w, keyed.keys) : null;
    return k !== null && keyed.words.has(k) ? [keyed.words.get(k)].flat().map(fold) : [w];
  });
}

/// Why the braces of `ko` do not hold the words the `@` keywords of `en` call for, or null: one
/// brace word for each highlighted word, one of the words it may show ([`braceWordsWanted`]), in
/// any order. So the player is shown only words the conversation takes.
export function keywordsWhy(en, ko, keyed = null) {
  const wanted = braceWordsWanted(en, keyed);
  const got = braceWords(ko);
  if (wanted.length !== got.length) return 'keywords';
  // A matching of the highlighted words to the brace words (augmenting paths).
  const owner = new Array(got.length).fill(-1);
  const take = (i, seen) => got.some((g, j) => {
    if (seen[j] || !wanted[i].includes(g)) return false;
    seen[j] = true;
    if (owner[j] >= 0 && !take(owner[j], seen)) return false;
    owner[j] = i;
    return true;
  });
  return wanted.every((_, i) => take(i, new Array(got.length).fill(false))) ? null : 'keywords';
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
/// keywords (the words the braces hold are, as a multiset and case folded, the words the English
/// `@`s call for: with `keyed`, a script's keys and their words, the Korean word of the key each
/// English word hits, else the English word, [`keywordsWhy`]; so the player is shown only words
/// the conversation takes), the runic text, the line breaks at both ends, its length.
export function gate(en, ko, keyed = null) {
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
  if (keywordsWhy(en, ko, keyed)) return 'keywords';
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
/// gates (with its script's keys and words, `keyed`), then for a string the engine copies into a
/// slot (printed later as it is, by `$n`) no josa marker or `@{`, and its capacity.
export function tableGate(u, ko, keyed = null) {
  const why = gate(u.text, ko, keyed);
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

const hangul = (ko) => /[가-힣]/.test(ko);

/// `en` without its plural markers: each `/` or `\` and the ASCII letters after it (the Korean
/// rule of spec 08 §1: they print nothing).
export const withoutPlural = (en) => en.replace(/[/\\][A-Za-z]*/g, '');

/// Why `ko` is not a translation of object name `en` the engine can take, or null: no josa marker
/// or `@{` (a name is put inside other text), no plural marker (in Korean they print nothing, so a
/// name holds none and shows one form for any count), the line's gates on the English without its
/// markers, the same `%` directives in order (it is printed as a format), the reader's length,
/// and a Hangul syllable.
export function lookGate(en, ko) {
  if (typeof ko === 'string' && hasMarker(ko)) return 'raw';
  if (typeof ko === 'string' && /[/\\]/.test(ko)) return 'plural';
  const why = gate(withoutPlural(en), ko);
  if (why) return why;
  const [a, b] = [directivesOf(en), directivesOf(ko)];
  if (a.length !== b.length || a.some((d, i) => d !== b[i])) return 'directives';
  if (Buffer.byteLength(ko) > MAX_LOOK_TEXT_BYTES) return 'long';
  if (!hangul(ko)) return 'no-hangul';
  return null;
}

/// Why `ko` is not a translation of book or sign `en` the engine can take, or null: the line's
/// gates, the same `%` directives in order (it is printed as a format), a text under the game's
/// book buffer (BOOK_BYTES), and a Hangul syllable.
export function bookGate(en, ko) {
  const why = gate(en, ko);
  if (why) return why;
  const [a, b] = [directivesOf(en), directivesOf(ko)];
  if (a.length !== b.length || a.some((d, i) => d !== b[i])) return 'directives';
  if (Buffer.byteLength(ko) >= BOOK_BYTES) return 'capacity';
  if (!hangul(ko)) return 'no-hangul';
  return null;
}

/// The printf codes of a string of u in order (u6_core::locale::program::directives): each `%`
/// with the character after it (`%B %F %X %Y %P`, a conversion, or one put as it is: `%%`), a `%`
/// that ends the text alone.
export function uCodesOf(text) {
  return [...text.matchAll(/%[\s\S]?/g)].map((m) => m[0]);
}

/// Whether a string of u has a letter to translate: one outside its printf codes.
export const uHasLetters = (en) => /[A-Za-z]/.test(en.replace(/%[\s\S]?/g, ''));

/// The letters of `keys` (the keys of the read after a string of u, the export's `keys`) that
/// `text` shows standing alone, each once, in the order they first do: an ASCII letter with no
/// ASCII letter right before or after it (`A)`, `(M)`, `N)ew`) and no ASCII whitespace right after
/// it (space, tab, line feed, carriage return: such a letter is a word, the article A, not a key
/// shown); the character after a `%` is a printf code, not a letter
/// (u6_core::locale::program::keys_shown).
export function keysShown(text, keys = '') {
  return standing(text, keys, true);
}

// The letters of `keys` that `text` shows standing alone, as keysShown reads them; a letter
// followed by whitespace counts only when `words` is false (a translation: `A 가시오` shows A).
function standing(text, keys, words) {
  const glyphs = [];
  for (let i = 0; i < text.length; i += 1) {
    if (text[i] === '%') {
      glyphs.push(null);
      i += 1;
    } else glyphs.push(text[i]);
  }
  const letter = (j) => typeof glyphs[j] === 'string' && /^[A-Za-z]$/.test(glyphs[j]);
  const space = (j) => typeof glyphs[j] === 'string' && /^[ \t\n\r]$/.test(glyphs[j]);
  let out = '';
  glyphs.forEach((g, j) => {
    if (g !== null && /^[A-Z]$/.test(g) && keys.includes(g) && !out.includes(g) && !letter(j - 1) && !letter(j + 1)
      && !(words && space(j + 1))) out += g;
  });
  return out;
}

/// Whether `ko` keeps the keys of its English `en`: every letter of `keys` the English shows
/// standing alone stands alone in `ko` too, in the same order (one it shows besides, such as a key
/// added as `(M)`, is not counted; in `ko` a key followed by whitespace still counts, the article
/// rule reads the English only; u6_core::locale::program::keys_kept).
export function keysKept(en, ko, keys = '') {
  const shown = keysShown(en, keys);
  return [...standing(ko, keys, false)].filter((k) => shown.includes(k)).join('') === shown;
}

const lineFeeds = (text) => [...text].filter((c) => c === '\n').length;

/// Why `ko` is not a translation of string `en` of the u program the engine can take, or null. u
/// prints it with its own printf, which has no escape layer (`$ # * @ & < >` are plain characters
/// there) and no particle resolver: the text, the characters the game can show, the same printf
/// codes in the same order (`%B %F %X %Y %P`, the cursor and colour and the flush, included), no
/// josa marker or `@{` (they would print as they are), as many line breaks at its start, at its end
/// and in all (a sheet's rows and a passage's paragraphs), for the prompt of a key read (`keys`,
/// the letters the read takes) the keys its English shows standing alone ([`keysKept`]), the
/// reader's length, at least 3 bytes and a Hangul syllable. The engine refuses a pack whose string
/// breaks the codes or the keys.
export function uGate(en, ko, keys = '') {
  if (typeof ko !== 'string' || ko.length === 0) return 'empty';
  if (Buffer.byteLength(ko) > MAX_U_TEXT_BYTES) return 'long';
  if (Buffer.byteLength(ko) < MIN_U_BYTES) return 'short';
  if (![...ko].every(showable)) return 'characters';
  const [a, b] = [uCodesOf(en), uCodesOf(ko)];
  if (a.length !== b.length || a.some((d, i) => d !== b[i])) return 'directives';
  if (!keysKept(en, ko, keys)) return 'keys';
  if (hasMarker(ko)) return 'raw';
  const [ea, eb] = [edges(en), edges(ko)];
  if (ea[0] !== eb[0] || ea[1] !== eb[1] || lineFeeds(en) !== lineFeeds(ko)) return 'line-breaks';
  if (!hangul(ko)) return 'no-hangul';
  return null;
}

/// Whether `en` (an object name or a book) has a letter to translate: one outside its runes
/// (`<…>`), its escapes and its plural markers.
export function hasLetters(en) {
  const open = withoutPlural(en).replace(/<[^>]*(>|$)/g, '');
  return /[A-Za-z]/.test(open.replace(/[$#^][\s\S]?/g, ''));
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

/// The instruction every request of a script carries: the task, the answer's form, then the style
/// guide; `keyed` for a request that lists the script's keywords (`topics`), whose answer gives a
/// Korean word for each (slice K5b; without it the instruction is K1's to K4's, byte for byte).
export function systemOf(style, keyed = false) {
  const head = [
    'You translate the conversation lines of one character of a role-playing game, Ultima VI,',
    'from its faux-archaic English into archaic Korean (사극체), following the guide below.',
    'The request is JSON: the character (name, description, class and register) and its lines,',
    'each {"n": <number>, "en": "<line>"} (and "joins": true for a line that goes on from the one',
    'before it; "table": true for a string the script picks at run time, with "copied": true and',
  ];
  if (!keyed) {
    return [
      ...head,
      '"capacity" for one it stores and prints later as it is). Answer with one JSON object and',
      'nothing else:',
      '{"lines": [{"n": <number>, "ko": "<the Korean line>"}]}',
      'with exactly one entry for every line asked, its own n.',
      '',
      style,
    ].join('\n');
  }
  return [
    ...head,
    '"capacity" for one it stores and prints later as it is), and the keywords the player types',
    'to it, "topics": one entry per keyword block of the script, in its order, each {"keys":',
    '[{"k": "<keyword>", "shown": [<the English words its lines highlight with @ that this keyword',
    'answers>], "known": "<the Korean word already chosen for it>", "answer": "yes" | "no"}]} (the',
    'keys of one block are one answer; "known" only where a word is chosen; "answer" only on a',
    'one-letter key y or n alone in its block). Answer with one JSON object and nothing else:',
    '{"lines": [{"n": <number>, "ko": "<the Korean line>"}], "keywords": [{"k": "<keyword>", "ko":',
    '"<the Korean word the player types for it>"}]}',
    'with exactly one entry for every line asked, its own n, and one for every keyword listed: a',
    'word of 1 to 8 Hangul syllables and nothing else (no space, no particle, no Latin letter), the',
    'same word for the keys of one block, its "known" word where one is given. A key marked',
    '"answer" is the player\'s yes or no to the question before its block: answer the Korean word a',
    'player types for yes or no, 예 for y and 아니 for n, whatever else is given, not a word of its',
    'lines; a key of another block that means yes or no (yes, no) takes another word (그렇소,',
    '아니오). In the lines, write each English @word that a key\'s "shown" lists (case aside) as',
    '@{<the Korean word you answer for that key>}, its particle after the closing brace (@{일}을),',
    'and every other @word as @{<the English word>}.',
    '',
    style,
  ].join('\n');
}

/// The instruction the request of shared keywords carries (slice K5b): the task, the answer's
/// form, then the style guide.
export function glossarySystemOf(style) {
  return [
    'You choose the Korean words a player of a role-playing game, Ultima VI, types to its',
    'characters for the keywords many of them answer to, in the archaic Korean (사극체) of the',
    'guide below. The request is JSON: {"keywords": [{"k": "<keyword>", "shown": [<the English',
    'words the characters\' lines highlight with @ that this keyword answers>]}]}. Answer with one',
    'JSON object and nothing else:',
    '{"keywords": [{"k": "<keyword>", "ko": "<the Korean word>"}]}',
    'with exactly one entry for every keyword asked: the word the lines will highlight and the',
    'player will type, 1 to 8 Hangul syllables and nothing else (no space, no particle, no Latin',
    'letter), a plain word of the meaning the English words share, a different word for each',
    'keyword unless two mean the same.',
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

/// The request body for `lines` of `bundle` (the exact bytes sent): a Messages call; with
/// `topics` (slice K5b, [`topicsOf`]) the script's keywords too, and `system` is then
/// `systemOf(style, true)`'s. With none it is K1's to K4's body, byte for byte, for the same style.
/// With `speaker` (spec 12: a script's {age, profile} from the speaker file, [`planOf`]) the `npc`
/// also names its `age` and its `profile` (the personality note), and its `register` is the one
/// for that class at that age ([`registerOf`]); without it (null) the body is as before.
export function bodyOf(model, system, bundle, cls, lines, topics = [], speaker = null) {
  const npc = {
    name: typeof bundle.name === 'string' ? bundle.name : '',
    description: textOf(bundle.narration?.look),
    class: cls,
    ...(speaker ? { age: speaker.age, profile: speaker.profile } : {}),
    register: speaker ? registerOf(cls, speaker.age) : CLASSES[cls],
  };
  const user = JSON.stringify(topics.length > 0 ? { npc, lines: lines.map(lineOf), topics } : { npc, lines: lines.map(lineOf) });
  return JSON.stringify({ model, max_tokens: MAX_TOKENS, system, messages: [{ role: 'user', content: user }] });
}

/// The one-letter keys a block that lists one of them alone takes as the player's answer to the
/// question before it: `y` for yes, `n` for no (the game's own test takes any word starting so);
/// and the Korean word such a key takes, the one a player types for yes or no (YES_NO_WORDS).
export const YES_NO = { y: 'yes', n: 'no' };
export const YES_NO_WORDS = { y: '예', n: '아니' };

/// The `topics` of a request of `script` (slice K5b): one entry per keyword block with keys, in
/// script order, each key once, in the first block that lists it (the block the game's first match
/// reaches for it, [`compatibleOf`]; a key listed twice may be answered twice), with the words its
/// lines highlight that hit it (`shown`) and the Korean word `known` (a Map) settles for it, when
/// there is one, and for a key of [`YES_NO`] its first block lists alone, the answer it is
/// (`answer`: yes or no); only the keys of `only` (a Set) when given (a request again lists the
/// keys that have a word, each with that word). The two never disagree: a marked key's known word
/// other than its [`YES_NO_WORDS`] word is left out of a first request (the mark decides), and the
/// mark out of a request again (the word the run gate takes decides: its lines must show it).
export function topicsOf(script, known, only = null) {
  const first = firstBlocksOf(script.topics);
  return script.topics
    .map((list, i) => ({
      keys: list.filter((k) => first.get(k) === i && (only === null || only.has(k)))
        .map((k) => {
          const answer = Object.hasOwn(YES_NO, k) && list.length === 1 ? YES_NO[k] : null;
          const word = known.get(k);
          const agree = answer === null || word === undefined || word === YES_NO_WORDS[k];
          return {
            k, shown: script.shown.get(k) ?? [], ...(word !== undefined && (agree || only !== null) ? { known: word } : {}),
            ...(answer !== null && (agree || only === null) ? { answer } : {}),
          };
        }),
    }))
    .filter((t) => t.keys.length > 0);
}

/// The request body for the shared keywords `units` (the exact bytes sent): a Messages call.
export function glossaryBodyOf(model, system, units) {
  const user = JSON.stringify({ keywords: units.map((u) => ({ k: u.k, shown: u.shown })) });
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

/// The instruction a request of object names and books carries: the task, the answer's form,
/// then the guide.
export function textsSystemOf(style) {
  return [
    'You translate the object names and the books and signs of a role-playing game, Ultima VI,',
    'from its faux-archaic English into archaic Korean (사극체), following the guide below. The',
    'request is JSON: the texts, each {"n": <number>, "en": "<text>", "kind": "look" | "book"}.',
    'A "look" text is the name the game gives a thing: a plain noun phrase, with no honorific, no',
    'article, no particle marker and no plural marker (drop / and \\ with the letters after them:',
    'Korean shows one form for any count). A "book" text is a book or a sign, read in 사극체: keep',
    'every escape character and every % directive exactly, in the same order, the text between <',
    'and > (runes) as it is, and the line breaks at its ends. Answer with one JSON object and',
    'nothing else:',
    '{"lines": [{"n": <number>, "ko": "<the Korean text>"}]}',
    'with exactly one entry for every text asked, its own n.',
    '',
    style,
  ].join('\n');
}

/// The request body for object names and books `units` (the exact bytes sent): a Messages call.
export function textsBodyOf(model, system, units) {
  const user = JSON.stringify({ texts: units.map((u) => ({ n: u.n, en: u.text, kind: u.kind })) });
  return JSON.stringify({ model, max_tokens: MAX_TOKENS, system, messages: [{ role: 'user', content: user }] });
}

/// The instruction a request of the u program's strings carries: the task, the answer's form,
/// then the guide.
export function programSystemOf(style) {
  return [
    'You translate the opening of a role-playing game, Ultima VI, from its faux-archaic English',
    'into archaic Korean (사극체), following the guide below: the story passages, the walk to a',
    'gypsy and her questions, and the prompts of making a character and of bringing one from an',
    'earlier game. The request is JSON: the lines, in the order the game shows them, each',
    '{"n": <number>, "en": "<line>", "kind": "opening" | "story" | "gypsy" | "creation" | "transfer"},',
    'with "keys" for a line after which the game waits for one of those keys. Keep every % code',
    'exactly, in the same order (%s, %d and the codes %B %F %X %Y %P the game reads itself) and',
    'every line break. Of a line\'s keys, keep each one its English shows standing alone (as in',
    'A), (M) or N)ew) standing alone, in Latin letters and in the same order; for one its English',
    'shows only inside a word (Male for M), put the letter after that word\'s Korean as (M).',
    'Answer with one JSON object and nothing else:',
    '{"lines": [{"n": <number>, "ko": "<the Korean line>"}]}',
    'with exactly one entry for every line asked, its own n.',
    '',
    style,
  ].join('\n');
}

/// The request body for the u program's strings `units` (the exact bytes sent): a Messages call.
export function programBodyOf(model, system, units) {
  const user = JSON.stringify({ lines: units.map((u) => ({ n: u.n, en: u.text, kind: u.role, ...(u.keys ? { keys: u.keys } : {}) })) });
  return JSON.stringify({ model, max_tokens: MAX_TOKENS, system, messages: [{ role: 'user', content: user }] });
}

/// The JSON object of an answer (a code fence around it allowed), or null.
export function objectOfAnswer(content) {
  const text = typeof content === 'string' ? content.trim() : '';
  const from = text.indexOf('{');
  const to = text.lastIndexOf('}');
  if (from < 0 || to < from) return null;
  try {
    const v = JSON.parse(text.slice(from, to + 1));
    return plain(v) ? v : null;
  } catch {
    return null;
  }
}

/// The lines of an answer: a Map of n to its Korean text, and its `keywords` as given (slice K5b;
/// [] when it has none, read by the alias gate), or null when it is not JSON of the answer's form
/// (a code fence around it allowed).
export function linesOfAnswer(content) {
  const v = objectOfAnswer(content);
  if (!v || !Array.isArray(v.lines)) return null;
  const lines = new Map();
  for (const l of v.lines) {
    if (plain(l) && Number.isInteger(l.n) && typeof l.ko === 'string' && !lines.has(l.n)) lines.set(l.n, l.ko);
  }
  return lines.size > 0 ? { lines, keywords: Array.isArray(v.keywords) ? v.keywords : [] } : null;
}

/// The keywords of the answer to the shared keywords' request (slice K5b), as given, or null when
/// it is not JSON of that answer's form.
export function keywordsOfAnswer(content) {
  const v = objectOfAnswer(content);
  return v && Array.isArray(v.keywords) && v.keywords.length > 0 ? { keywords: v.keywords } : null;
}

/// The speaker file (spec 12 Design 1: `value`, the parsed u6.speakers/1 tools/i18n/speakers.mjs
/// writes) as a Map of `<archive>:<item>` to {age, class, note}. Strict: an object of exactly
/// {format, model, speakers} and, optionally, `coverage` (speakers.mjs's receipt: an object, not
/// read for the plan), each entry exactly {archive, item, age, class, note}, its archive a or b,
/// its item 0 to 65535, its age one of AGES, its class one of CLASSES and its note at most
/// SPEAKER_NOTE_MAX characters, each script once. Anything else throws an InputError with a fixed
/// message (a count at most).
export function readSpeakers(value) {
  const refused = (why) => new InputError(`the speakers file is not ${SPEAKERS_FORMAT}${why ? `: ${why}` : ''}`);
  if (!exact(value, ['format', 'model', 'speakers'], ['coverage']) || value.format !== SPEAKERS_FORMAT || typeof value.model !== 'string'
    || !Array.isArray(value.speakers) || (Object.hasOwn(value, 'coverage') && !plain(value.coverage))) throw refused('');
  if (value.speakers.length > SPEAKERS_MAX) throw refused(`it lists more than ${SPEAKERS_MAX} scripts`);
  const entry = (s) => exact(s, ['archive', 'item', 'age', 'class', 'note']) && ['a', 'b'].includes(s.archive)
    && Number.isInteger(s.item) && s.item >= 0 && s.item <= 0xffff && AGES.includes(s.age) && Object.hasOwn(CLASSES, s.class)
    && typeof s.note === 'string' && [...s.note].length <= SPEAKER_NOTE_MAX;
  const bad = value.speakers.filter((s) => !entry(s)).length;
  if (bad > 0) throw refused(`${bad} entries are not {archive, item, age, class, note} of its forms`);
  const speakers = new Map();
  let twice = 0;
  for (const s of value.speakers) {
    const key = `${s.archive}:${s.item}`;
    if (speakers.has(key)) twice += 1;
    else speakers.set(key, { age: s.age, class: s.class, note: s.note });
  }
  if (twice > 0) throw refused(`${twice} entries list a script listed before`);
  return speakers;
}

/// The bundles of `value` (the parsed file) as the scripts to translate: `item` ({archive, item}),
/// the one bundle whose `name` is `name` (case folded), or the `first` scripts with lines, in
/// (archive, item) order. Each script's units: its runs, then its table strings (one an op reads
/// from inside is left English, `inner`); its keys (slice K5b, [`keysOf`]) with the words the
/// units asked highlight for each (`shown`, [`shownOf`]). Throws a fixed message (a count at most)
/// when the file is not a list of bundles or the script named has no lines.
/// With `speakers` (spec 12: [`readSpeakers`]'s Map) each script's `speaker` is {age, profile,
/// listed}: a script the file lists takes its age, its note as the profile and its class (unless
/// `cls`, the owner's --class, gives one for the run); one it does not list Pass A, `adult` and
/// no note. `ages` (a list of AGES, with `speakers`) keeps only the scripts of those ages, in
/// order, all of them, or the `first` of them; without `speakers` every script is as before
/// (`speaker` null).
export function planOf(value, { item = null, first = null, name = null, cls = null, speakers = null, ages = null } = {}) {
  if (!Array.isArray(value)) throw new InputError('the file is not a list of NPC bundles');
  const bad = value.filter((b) => !plain(b) || b.format !== BUNDLE_FORMAT || !['a', 'b'].includes(b.archive)
    || !Number.isInteger(b.item) || typeof b.hash !== 'string' || !/^[0-9a-f]{16}$/.test(b.hash)).length;
  if (bad > 0) throw new InputError(`${bad} entries of the file are not ${BUNDLE_FORMAT} bundles`);
  const sorted = [...value].sort((x, y) => (x.archive === y.archive ? x.item - y.item : x.archive < y.archive ? -1 : 1));
  const usable = sorted.filter((b) => (b.refused === null || b.refused === undefined) && unitsOf(b).length > 0);
  const listedOf = (b) => speakers?.get(`${b.archive}:${b.item}`) ?? null;
  let chosen;
  if (item) {
    chosen = usable.filter((b) => b.archive === item.archive && b.item === item.item);
    if (chosen.length === 0) throw new InputError('the script named is not in the bundles, is refused or has no lines');
  } else if (name !== null) {
    const want = name.toLowerCase();
    chosen = usable.filter((b) => typeof b.name === 'string' && b.name.toLowerCase() === want);
    if (chosen.length !== 1) throw new InputError(`${chosen.length} scripts with lines have that name: name one with --item`);
  } else if (speakers !== null && ages !== null) {
    const pool = usable.filter((b) => ages.includes(listedOf(b)?.age ?? 'adult'));
    chosen = pool.slice(0, first ?? pool.length);
  } else {
    chosen = usable.slice(0, first ?? 0);
  }
  const scripts = chosen.map((b) => {
    const runs = unitsOf(b).map((u) => ({ ...u, kind: 'run', len: latin1(u.text).length, hash: hex64(hash64(latin1(u.text))) }));
    const tables = tablesOf(b).map((t) => ({ ...t, kind: 'table', ...(t.inner ? { done: true, why: 'inner' } : {}) }));
    const units = [...runs, ...tables].map((u, n) => ({ ...u, n }));
    const { topics, keys } = keysOf(b);
    const shown = shownOf(units.filter((u) => !u.done), keys);
    const listed = listedOf(b);
    const speaker = speakers === null ? null : { age: listed?.age ?? 'adult', profile: listed?.note ?? '', listed: listed !== null };
    return {
      kind: 'script', bundle: b, cls: cls ?? listed?.class ?? speakerClass(b), units, keys, topics, shown, compatible: compatibleOf(topics), speaker,
    };
  });
  return { read: value.length, usable: usable.length, scripts, bundles: value, speakers: speakers !== null };
}

/// The words the pack there (`existing`, readPack's, or null) settles for `script`'s keys (slice
/// K5b): each key's own word in the script's alias for it (its entry's first word that passes
/// [`wordGate`]; a key with none has none), never a word another script gives the key (the engine
/// looks a word up by script and key, so a key need not have one word over the scripts); a Map of
/// key to word.
export function knownOf(existing, script) {
  const known = new Map();
  for (const [k, words] of scriptWordsOf(existing?.aliases, script.bundle.archive, script.bundle.item)) {
    const w = words.find((x) => wordGate(x) === null);
    if (w !== undefined && script.keys.includes(k)) known.set(k, w);
  }
  return known;
}

/// The keys of `scripts` the shared keywords' request need not ask: those every one of them that
/// holds the key has its own word for (`own`, [`knownOf`]'s; none with --fresh-keywords).
export function settledOf(scripts) {
  const settled = new Set(scripts.flatMap((s) => [...(s.own ?? new Map()).keys()]));
  for (const s of scripts) for (const k of s.keys) if (!s.own?.has(k)) settled.delete(k);
  return settled;
}

/// The request of shared keywords (slice K5b, spec 08 §4): the keys at least GLOSSARY_SCRIPTS of
/// `scripts` hold that `settled` (a Set: [`settledOf`]'s) does not, the most held first (then in
/// key order), at most GLOSSARY_KEYS, each with the words the scripts' lines highlight for it (at
/// most GLOSSARY_SHOWN, in script order); never a key of one letter (a word longer than its key
/// hits it, so over the scripts it would gather every highlighted word that starts so: `y` and `n`
/// are a reply's yes and no, which each script's request marks, [`topicsOf`]); `compatible`,
/// whether two keys may keep colliding words: in every one of them that holds both, the first
/// block of each is the same ([`compatibleOf`]); null when no key is so shared.
export function glossaryOf(scripts, settled = new Set()) {
  const held = new Map();
  for (const s of scripts) {
    for (const k of s.keys) {
      if (k.length === 1) continue;
      const h = held.get(k) ?? { k, scripts: 0, shown: [] };
      h.scripts += 1;
      for (const w of s.shown.get(k) ?? []) if (h.shown.length < GLOSSARY_SHOWN && !h.shown.includes(w)) h.shown.push(w);
      held.set(k, h);
    }
  }
  const chosen = [...held.values()].filter((h) => h.scripts >= GLOSSARY_SCRIPTS && !settled.has(h.k))
    .sort((x, y) => y.scripts - x.scripts || (x.k < y.k ? -1 : x.k > y.k ? 1 : 0))
    .slice(0, GLOSSARY_KEYS);
  if (chosen.length === 0) return null;
  const compatible = (a, b) => scripts.every((s) => !(s.keys.includes(a) && s.keys.includes(b)) || s.compatible(a, b));
  return { kind: 'glossary', units: chosen.map((h, n) => ({ kind: 'keyword', n, k: h.k, shown: h.shown })), compatible };
}

/// The strings of u of `value` (the parsed u6.string-bundle/1, its `u`) as the units to translate,
/// in the order u prints them, in requests of at most U_PER_REQUEST strings and U_CHARS characters;
/// one with no letter outside its printf codes is left English unasked (`nothing`). Throws a fixed
/// message (a count at most) when an entry is not of the form, its kind is not one of u's, its
/// text, length and hash disagree, or its `keys` (the keys of the read after it, optional) are not
/// upper-case letters, each once.
export function programPlanOf(value) {
  const list = value.u ?? [];
  if (!Array.isArray(list)) throw new InputError(`the file is not ${STRINGS_FORMAT}`);
  const keysOk = (k) => k === undefined || (typeof k === 'string' && /^[A-Z]*$/.test(k) && new Set(k).size === k.length);
  const ok = (t) => plain(t) && U_KINDS.includes(t.kind) && hex16(t.hash) && Number.isInteger(t.len)
    && typeof t.text === 'string' && ![...t.text].some((c) => c.charCodeAt(0) > 0xff) && latin1(t.text).length === t.len
    && t.len >= MIN_U_BYTES && t.len <= MAX_U_BYTES && hex64(hash64(latin1(t.text))) === t.hash && keysOk(t.keys);
  const bad = list.filter((t) => !ok(t)).length;
  if (bad > 0) throw new InputError(`${bad} strings of the file are not strings of the u program of ${STRINGS_FORMAT}`);
  const unique = [...new Map(list.map((t) => [t.hash, t])).values()];
  const batches = [];
  const left = [];
  let batch = [];
  let chars = 0;
  for (const t of unique) {
    const unit = { kind: 'program', role: t.kind, hash: t.hash, len: t.len, text: t.text, keys: t.keys ?? '' };
    if (!uHasLetters(t.text)) {
      left.push({ ...unit, n: left.length, done: true, why: 'nothing' });
      continue;
    }
    if (batch.length > 0 && (batch.length >= U_PER_REQUEST || chars + t.text.length > U_CHARS)) {
      batches.push({ kind: 'program', units: batch });
      batch = [];
      chars = 0;
    }
    batch.push({ ...unit, n: batch.length });
    chars += t.text.length;
  }
  if (batch.length > 0) batches.push({ kind: 'program', units: batch });
  if (left.length > 0) batches.push({ kind: 'program', units: left });
  return { read: list.length, batches };
}

/// The object names and books of `value` (the parsed u6.string-bundle/1, its `texts`) of the parts
/// `only` names, as the units to translate: the names in requests of at most LOOK_PER_REQUEST and
/// LOOK_CHARS characters, the books of at most BOOKS_PER_REQUEST and BOOK_CHARS characters (a
/// longer book alone); one with no letter to translate is left English unasked (`nothing`).
/// Throws a fixed message (a count at most) when an entry is not of the form, its text, length
/// and hash disagree, or it is keyed where no pack keys one. `all`: every name and book listed,
/// for the check of the pack already there.
export function textsPlanOf(value, only = PARTS) {
  const list = value.texts ?? [];
  if (!Array.isArray(list)) throw new InputError(`the file is not ${STRINGS_FORMAT}`);
  const ok = (t) => plain(t) && ['look', 'book'].includes(t.kind) && Number.isInteger(t.index) && t.index >= 0
    && t.index < (t.kind === 'look' ? LOOK_ENTRIES : BOOK_INDICES) && hex16(t.hash) && Number.isInteger(t.len)
    && typeof t.text === 'string' && ![...t.text].some((c) => c.charCodeAt(0) > 0xff) && latin1(t.text).length === t.len
    && t.len >= 1 && t.len <= (t.kind === 'look' ? MAX_LOOK_BYTES : BOOK_BYTES) && hex64(hash64(latin1(t.text))) === t.hash;
  const bad = list.filter((t) => !ok(t)).length;
  if (bad > 0) throw new InputError(`${bad} texts of the file are not object names or books of ${STRINGS_FORMAT}`);
  const unique = (kind) => [...new Map(list.filter((t) => t.kind === kind).map((t) => [t.index, t])).values()];
  const all = { look: unique('look'), books: unique('book') };
  const batches = [];
  for (const [part, kind, most, chars] of [['look', 'look', LOOK_PER_REQUEST, LOOK_CHARS], ['books', 'book', BOOKS_PER_REQUEST, BOOK_CHARS]]) {
    if (!only.includes(part)) continue;
    const left = [];
    let batch = [];
    let n = 0;
    for (const t of all[part]) {
      const unit = { kind, index: t.index, hash: t.hash, len: t.len, text: t.text };
      if (!hasLetters(t.text)) {
        left.push({ ...unit, n: left.length, done: true, why: 'nothing' });
        continue;
      }
      if (batch.length > 0 && (batch.length >= most || n + t.text.length > chars)) {
        batches.push({ kind: 'texts', units: batch });
        batch = [];
        n = 0;
      }
      batch.push({ ...unit, n: batch.length });
      n += t.text.length;
    }
    if (batch.length > 0) batches.push({ kind: 'texts', units: batch });
    // Left English unasked: in a group of their own, which sends nothing.
    if (left.length > 0) batches.push({ kind: 'texts', units: left });
  }
  return { read: list.length, batches, all };
}

/// Whether the object names and books of the pack read from <out> (`existing`, readPack's) are
/// those of the files the game strings file was exported from (`all`, textsPlanOf's): each of a
/// kind the file lists is one of its, at that index, of that length and hash (the engine refuses
/// the whole pack at install otherwise).
export function textsRecheckOf(existing, all) {
  const same = (mine, theirs) => theirs.length === 0 || mine.every((t) => theirs.some((u) => u.index === t.index && u.len === t.len && u.hash === t.hash));
  return same(existing.look, all.look) && same(existing.books, all.books);
}

/// The game strings of `value` (the parsed u6.string-bundle/1) as the units to translate, in
/// requests of at most STRINGS_PER_REQUEST strings and STRING_CHARS characters. Throws a fixed
/// message (a count at most) when the file is not of that form or a string's text, length and hash
/// disagree, or one is under MIN_STRING_BYTES but a label (the engine catalogues a label from 1
/// byte: a wind's name is 2).
export function stringsPlanOf(value) {
  if (!plain(value) || value.format !== STRINGS_FORMAT || !Array.isArray(value.strings)) {
    throw new InputError(`the file is not ${STRINGS_FORMAT}`);
  }
  const ok = (s) => plain(s) && hex16(s.hash) && Number.isInteger(s.len) && KINDS.includes(s.kind) && typeof s.text === 'string'
    && ![...s.text].some((c) => c.charCodeAt(0) > 0xff) && latin1(s.text).length === s.len
    && hex64(hash64(latin1(s.text))) === s.hash && s.len >= (s.kind === 'label' ? 1 : MIN_STRING_BYTES)
    && s.len <= MAX_STRING_BYTES;
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
/// others, by reason. A conversation script's runs and table strings are gated with its keys and
/// their words (`script.keyed`, slice K5b); the other texts have none.
function checked(script, answer) {
  const good = new Map();
  const failed = new Map();
  const keyed = script.keyed ?? null;
  const gateOf = (u, ko) => (u.kind === 'table' ? tableGate(u, ko, keyed) : u.kind === 'string' ? stringGate(u.text, ko)
    : u.kind === 'look' ? lookGate(u.text, ko) : u.kind === 'book' ? bookGate(u.text, ko)
      : u.kind === 'program' ? uGate(u.text, ko, u.keys) : gate(u.text, ko, keyed));
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
/// each of its keyword aliases is for a script they name, and its fingerprint is that of those
/// scripts.
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
  if (!(existing.aliases ?? []).every((a) => named.has(`${a.archive}:${a.item}`))) return false;
  return fingerprint([...named.values()].map((b) => ({ archive: b.archive, item: b.item, hash: b.hash }))) === existing.base;
}

// Spec 12: `scripts` by their speaker's age: for each age the scripts chosen, those of them the
// speaker file lists and those `translated` (a predicate) says.
function agesOf(scripts, translated = () => false) {
  return Object.fromEntries(AGES.map((age) => {
    const of = scripts.filter((s) => s.speaker?.age === age);
    return [age, { chosen: of.length, listed: of.filter((s) => s.speaker.listed).length, translated: of.filter(translated).length }];
  }));
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

/// The words the aliases `aliases` (a pack's) give script (`archive`, `item`), as the run gate
/// reads them: a Map of keyword to all its entry's words (a pack written by hand may give a key up
/// to MAX_ALIAS_WORDS; a line may show any of them).
export function scriptWordsOf(aliases, archive, item) {
  const words = new Map();
  for (const a of aliases ?? []) if (a.archive === archive && a.item === item) words.set(a.keyword, [...a.words]);
  return words;
}

/// The word a request names as `known` of a key's `words`: the first the alias gate takes, else
/// the first.
export const firstWord = (words) => words.find((w) => wordGate(w) === null) ?? words[0];

/// Slice K5b: settles `script`'s keys from `r`, the result of its first request (sendAll's: `ok`
/// with the answer's `keywords`, or a failure with its `reason`): `kept`, the words that pass the
/// alias gate ([`aliasesOf`], checked against each other and against the words the pack there,
/// `existing`, keeps for the script's keys this answer does not replace); `refused`, the other
/// keys' reasons; `keyed`, its keys with the words its lines are gated with: the pack there's for
/// the script (all of a key's words), each key this answer kept a word for with that word instead
/// (as the merge leaves them).
export function settleKeys(script, r, existing = null) {
  const theirs = scriptWordsOf(existing?.aliases, script.bundle.archive, script.bundle.item);
  const kept = new Map([...theirs].filter(([k]) => script.keys.includes(k)));
  const { words, failed } = r?.ok ? aliasesOf(script.keys, r.keywords, script.compatible, kept)
    : { words: new Map(), failed: new Map(script.keys.map((k) => [k, r?.reason ?? 'not-sent'])) };
  script.kept = words;
  script.refused = failed;
  script.keyed = { keys: script.keys, words: new Map([...theirs, ...[...words].map(([k, w]) => [k, [w]])]) };
}

/// The pack and the coverage receipt of `plan`'s scripts and game strings, translated as their
/// units say, merged into `existing` (readPack's, or null): what this run translated replaces what
/// it had under the same key, the rest stays. The fingerprint is over the scripts the merged runs
/// and table strings name (their hashes from the bundles); with no bundles the conversation part
/// is the existing pack's, unchanged. The keyword aliases (slice K5b): the words a script answered
/// in this run kept (`kept`, [`settleKeys`]) replace the pack's for the same keys, the others
/// stay; every merged run and table string of a script whose words changed is checked again
/// against the merged words, one that no longer passes dropped (left English, `keywords`); the
/// aliases of a script the merged lines do not name are dropped (`no-base`). Coverage `keywords`:
/// each key of the scripts answered given a word, or kept (its old word stays in the merged pack:
/// `kept`, by reason, keys by hash), or left English (no word in the merged pack: `byReason`, and
/// in `left` by hash); only one left English counts toward exit 3. No pack (null) when this run
/// changed nothing.
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
    perScript.push({
      hash: s.bundle.hash, class: s.cls, ...(s.speaker ? { age: s.speaker.age } : {}), units: s.units.length - tablesHere, tables: tablesHere,
      translated: done, left: s.units.length - done,
    });
  }
  const stringUnits = (plan.strings?.batches ?? []).flatMap((b) => b.units);
  for (const u of stringUnits) {
    if (u.ko !== undefined) strings.push({ hash: u.hash, len: u.len, text: u.ko });
    else left.push({ category: 'strings', hash: u.hash, reason: u.why ?? 'not-sent' });
  }
  const textUnits = (plan.texts?.batches ?? []).flatMap((b) => b.units);
  const look = [];
  const books = [];
  for (const u of textUnits) {
    if (u.ko !== undefined) (u.kind === 'look' ? look : books).push({ index: u.index, len: u.len, hash: u.hash, text: u.ko });
    else left.push({ category: u.kind === 'look' ? 'look' : 'books', hash: u.hash, reason: u.why ?? 'not-sent' });
  }
  const programUnits = (plan.program?.batches ?? []).flatMap((b) => b.units);
  const u = [];
  for (const x of programUnits) {
    if (x.ko !== undefined) u.push({ hash: x.hash, len: x.len, text: x.ko });
    else left.push({ category: 'u', hash: x.hash, reason: x.why ?? 'not-sent' });
  }
  const key = (r) => `${r.archive}:${r.item}:${r.offset}`;
  const newRuns = new Set(runs.map(key));
  const newTables = new Set(tables.map(key));
  const newStrings = new Set(strings.map((t) => t.hash));
  const newLook = new Set(look.map((t) => t.index));
  const newBooks = new Set(books.map((t) => t.index));
  const newU = new Set(u.map((t) => t.hash));
  const merged = {
    runs: [...(existing?.converse ?? []).filter((r) => !newRuns.has(key(r))), ...runs],
    tables: [...(existing?.tables ?? []).filter((t) => !newTables.has(key(t))), ...tables],
    strings: [...(existing?.strings ?? []).filter((t) => !newStrings.has(t.hash)), ...strings],
    look: [...(existing?.look ?? []).filter((t) => !newLook.has(t.index)), ...look],
    books: [...(existing?.books ?? []).filter((t) => !newBooks.has(t.index)), ...books],
    u: [...(existing?.u ?? []).filter((t) => !newU.has(t.hash)), ...u],
  };
  // Slice K5b: the keyword aliases. The words a script answered in this run kept replace the
  // pack's for the same keys; the pack's other aliases stay.
  const script = (x) => `${x.archive}:${x.item}`;
  const aliasKey = (a) => `${script(a)}:${a.keyword}`;
  const keyUnits = [];
  const fresh = [];
  for (const s of plan.scripts) {
    for (const k of s.keys ?? []) {
      const unit = { archive: s.bundle.archive, item: s.bundle.item, script: s.bundle.hash, k };
      if (s.kept?.has(k)) {
        unit.ko = s.kept.get(k);
        fresh.push({ archive: unit.archive, item: unit.item, keyword: k, words: [unit.ko] });
      } else unit.why = s.refused?.get(k) ?? 'not-sent';
      keyUnits.push(unit);
    }
  }
  const before = new Map((existing?.aliases ?? []).map((a) => [aliasKey(a), a]));
  const differs = (a) => JSON.stringify(before.get(aliasKey(a))?.words) !== JSON.stringify(a.words);
  const freshKeys = new Set(fresh.map(aliasKey));
  merged.aliases = [...(existing?.aliases ?? []).filter((a) => !freshKeys.has(aliasKey(a))), ...fresh];
  // Every merged run and table string of a script whose words changed is checked again against
  // the merged words (the run gate's keywords); one that no longer passes is dropped, English.
  const changedScripts = new Set(fresh.filter(differs).map(script));
  const answered = new Map(plan.scripts.map((s) => [script(s.bundle), s]));
  const english = new Map();
  const englishOf = (r, kind) => {
    const s = answered.get(script(r));
    const at = `${script(r)}:${kind}`;
    if (!english.has(at)) english.set(at, new Map((kind === 'table' ? tablesOf : unitsOf)(s.bundle).map((x) => [x.offset, x.text])));
    return english.get(at).get(r.offset);
  };
  let dropped = 0;
  const recheck = (list, kind) => list.filter((r) => {
    if (!changedScripts.has(script(r))) return true;
    const en = englishOf(r, kind);
    const keyed = { keys: answered.get(script(r)).keys, words: scriptWordsOf(merged.aliases, r.archive, r.item) };
    if (en === undefined || keywordsWhy(en, r.text, keyed) === null) return true;
    left.push({ category: kind === 'table' ? 'tables' : 'conversations', hash: r.hash, reason: 'keywords' });
    dropped += 1;
    return false;
  });
  merged.runs = recheck(merged.runs, 'run');
  merged.tables = recheck(merged.tables, 'table');
  // Only a script the merged runs or table strings name keeps aliases (the engine refuses others).
  const hashes = new Map((plan.bundles ?? []).filter(plain).map((b) => [script(b), b.hash]));
  const named = new Set([...merged.runs, ...merged.tables].map(script));
  let unbased = 0;
  merged.aliases = merged.aliases.filter((a) => {
    if (named.has(script(a))) return true;
    const unit = keyUnits.find((x) => script(x) === script(a) && x.k === a.keyword);
    if (!freshKeys.has(aliasKey(a))) unbased += 1;
    if (unit) {
      delete unit.ko;
      unit.why = 'no-base';
    } else {
      left.push({ category: 'keywords', script: hashes.get(script(a)) ?? '', hash: hex64(hash64(latin1(a.keyword))), reason: 'no-base' });
    }
    return false;
  });
  // A key this run gave no word that keeps one in the merged pack (its old word: the answer left
  // it out or its new word was refused) is not English: counted as kept, by reason; one that ends
  // with no word is left English.
  const finalKeys = new Set(merged.aliases.map(aliasKey));
  const kept = { count: 0, byReason: {}, keys: [] };
  let given = 0;
  const leftKeys = {};
  for (const x of keyUnits) {
    const entry = { script: x.script, hash: hex64(hash64(latin1(x.k))), reason: x.why };
    if (x.ko !== undefined) given += 1;
    else if (finalKeys.has(aliasKey({ ...x, keyword: x.k }))) {
      kept.count += 1;
      kept.byReason[x.why] = (kept.byReason[x.why] ?? 0) + 1;
      kept.keys.push(entry);
    } else {
      leftKeys[x.why] = (leftKeys[x.why] ?? 0) + 1;
      left.push({ category: 'keywords', ...entry });
    }
  }
  const aliased = fresh.filter((a) => named.has(script(a)) && differs(a)).length;
  const runUnits = plan.scripts.flatMap((s) => s.units.filter((u) => u.kind !== 'table'));
  const tableUnits = plan.scripts.flatMap((s) => s.units.filter((u) => u.kind === 'table'));
  const coverage = {
    format: COVERAGE_FORMAT,
    model,
    tool: TOOL,
    category: 'conversations',
    scripts: { read: plan.read, usable: plan.usable, chosen: plan.scripts.length, translated: translatedScripts },
    // Spec 12: with a speaker file, the chosen scripts by age, and of them the translated.
    ...(plan.speakers ? { ages: agesOf(plan.scripts, (s) => s.units.some((u) => u.ko !== undefined)) } : {}),
    units: tally(runUnits),
    tables: tally(tableUnits),
    strings: { read: plan.strings?.read ?? 0, ...tally(stringUnits) },
    look: tally(textUnits.filter((u) => u.kind === 'look')),
    books: tally(textUnits.filter((u) => u.kind === 'book')),
    u: { read: plan.program?.read ?? 0, ...tally(programUnits) },
    // A run with --fresh-keywords sent no word of the pack there as known in a script's first
    // request (a line asked again names the word its key keeps, the pack's where it kept none).
    ...(plan.freshKeywords ? { freshKeywords: true } : {}),
    keywords: {
      found: keyUnits.length, translated: given, kept, leftEnglish: keyUnits.length - given - kept.count, byReason: leftKeys, unexplained: 0, dropped,
    },
    glossary: tally(plan.glossary?.units ?? []),
    perScript,
    left,
  };
  let base = existing?.base ?? fingerprint([]);
  if (plan.bundles) {
    const scripts = new Map();
    for (const r of [...merged.runs, ...merged.tables]) scripts.set(script(r), { archive: r.archive, item: r.item, hash: hashes.get(script(r)) });
    base = fingerprint([...scripts.values()]);
  }
  const changed = runs.length + tables.length + strings.length + look.length + books.length + u.length + aliased + dropped + unbased > 0;
  const pack = changed ? packBytes({ base, ...merged }) : null;
  return {
    pack, coverage, runs: runs.length, tables: tables.length, strings: strings.length, look: look.length, books: books.length, u: u.length,
    aliases: aliased,
    total: {
      runs: merged.runs.length, tables: merged.tables.length, strings: merged.strings.length, look: merged.look.length,
      books: merged.books.length, u: merged.u.length, aliases: merged.aliases.length,
    },
  };
}

const USAGE = 'usage: node tools/i18n/translate.mjs [<bundles.json> (--item a:N | --name TEXT | --first N | --ages AGES [--first N]) '
  + '[--speakers <speakers.json>] [--fresh-keywords]] [--strings <strings.json> [--only strings,look,books,u]] [--class CLASS] '
  + `[--send | --dry-run] [--model NAME] [--llmux URL] [--out DIR] [--jobs 1-${JOBS_MAX}] [--timeout SECONDS]`;

/// The options of `argv`, or {error}.
export function parseArgs(argv) {
  const o = { send: false, dryRun: false, model: DEFAULTS.model, llmux: DEFAULTS.llmux, out: DEFAULTS.out,
    jobs: DEFAULTS.jobs, timeoutS: DEFAULTS.timeoutS, bundles: null, strings: null, item: null, first: null,
    name: null, cls: null, only: null, speakers: null, ages: null, freshKeywords: false, help: false };
  const value = (i) => (i + 1 < argv.length ? argv[i + 1] : null);
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--send') o.send = true;
    else if (a === '--dry-run') o.dryRun = true;
    else if (a === '--fresh-keywords') o.freshKeywords = true;
    else if (a === '--help' || a === '-h') o.help = true;
    else if (['--model', '--llmux', '--out', '--jobs', '--timeout', '--item', '--first', '--class', '--name', '--strings', '--only',
      '--speakers', '--ages'].includes(a)) {
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
      else if (a === '--only') o.only = v.split(',');
      else if (a === '--speakers') o.speakers = resolve(v);
      else if (a === '--ages') o.ages = v.split(',');
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
  // Spec 12: --ages chooses by the speaker file's ages, alone or with --first N.
  if (o.ages !== null && o.speakers === null) return { error: '--ages chooses by the ages of a --speakers file: name one' };
  if (o.speakers !== null && o.bundles === null) return { error: '--speakers and --ages go with a bundles file' };
  if (o.freshKeywords && o.bundles === null) return { error: '--fresh-keywords goes with a bundles file' };
  if (o.ages !== null && o.ages.some((a) => !AGES.includes(a))) return { error: `--ages names ages of the speaker file: ${AGES.join(', ')}` };
  if (o.ages !== null && (o.item !== null || o.name !== null)) return { error: '--ages goes alone or with --first N, not with --item or --name' };
  const choices = [o.item, o.first, o.name].filter((c) => c !== null).length;
  if (o.bundles !== null && choices !== 1 && !(o.ages !== null && choices === 0)) {
    return { error: 'name one script with --item or --name, or --first N, or choose by --ages' };
  }
  if (o.bundles === null && choices > 0) return { error: '--item, --name and --first choose from a bundles file' };
  if (o.only !== null && (o.strings === null || o.only.length === 0 || o.only.some((p) => !PARTS.includes(p)))) {
    return { error: `--only names parts of the --strings file: ${PARTS.join(', ')}` };
  }
  if (o.first !== null && (!Number.isInteger(o.first) || o.first < 1)) return { error: '--first is a number of scripts above 0' };
  if (o.name !== null && o.name.length === 0) return { error: '--name is an NPC\'s name' };
  if (o.cls !== null && !Object.hasOwn(CLASSES, o.cls)) return { error: `--class is one of ${Object.keys(CLASSES).join(', ')}` };
  if (!Number.isInteger(o.jobs) || o.jobs < 1 || o.jobs > JOBS_MAX) return { error: `--jobs is 1 to ${JOBS_MAX}` };
  if (!finite(o.timeoutS) || o.timeoutS <= 0) return { error: '--timeout is a number of seconds above 0' };
  if (!/^[\w.:[\]/-]{1,100}$/.test(o.model)) return { error: '--model is a model name' };
  return o;
}

const seconds = (ms) => `${(ms / 1000).toFixed(1)} s`;

/// The JSON of the file at `path` (at most BUNDLES_MAX bytes) and its size; throws InputError with
/// a fixed message (its kind, the file's role).
export async function readJson(path, what) {
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
  const keyedSystem = systemOf(guide, true);
  const glossarySystem = glossarySystemOf(guide);
  const stringsSystem = stringsSystemOf(guide);
  const textsSystem = textsSystemOf(guide);
  const programSystem = programSystemOf(guide);

  // The bundles and the game strings, and what to send of them.
  const plan = {
    read: 0, usable: 0, scripts: [], bundles: null, speakers: false, freshKeywords: o.freshKeywords, strings: null, texts: null, program: null,
  };
  try {
    if (o.bundles !== null) {
      // Spec 12: the speaker file, read strictly before the bundles (one that does not read is
      // refused here, before any request).
      const speakers = o.speakers === null ? null : readSpeakers((await readJson(o.speakers, 'speakers')).value);
      const { value, size } = await readJson(o.bundles, 'bundles');
      Object.assign(plan, planOf(value, { item: o.item, first: o.first, name: o.name, cls: o.cls, speakers, ages: o.ages }));
      const units = plan.scripts.reduce((n, s) => n + s.units.filter((u) => u.kind === 'run').length, 0);
      const tables = plan.scripts.reduce((n, s) => n + s.units.filter((u) => u.kind === 'table').length, 0);
      const chars = plan.scripts.reduce((n, s) => n + s.units.reduce((m, u) => m + u.text.length, 0), 0);
      say(`Bundles: ${plan.read} scripts read from ${o.bundles} (${size} bytes), ${plan.usable} with lines; `
        + `${plan.scripts.length} chosen, ${units} lines, ${tables} table strings, ${chars} characters.`);
      if (speakers !== null) {
        const ages = agesOf(plan.scripts);
        const listed = AGES.reduce((n, a) => n + ages[a].listed, 0);
        say(`Speakers: ${speakers.size} scripts listed in ${o.speakers}${o.ages ? `; chosen by age (${o.ages.join(', ')})` : ''}; `
          + `of the ${plan.scripts.length} chosen, ${listed} listed (the others by Pass A, as adults); `
          + `${AGES.map((a) => `${a} ${ages[a].chosen}`).join(', ')}.`);
      }
    }
    if (o.strings !== null) {
      const { value, size } = await readJson(o.strings, 'game strings');
      const only = o.only ?? PARTS;
      const strings = stringsPlanOf(value);
      plan.texts = textsPlanOf(value, only);
      const program = programPlanOf(value);
      if (only.includes('strings')) plan.strings = strings;
      if (only.includes('u')) plan.program = program;
      const count = (batches, kind) => batches.reduce((m, b) => m + b.units.filter((u) => !u.done && (!kind || u.kind === kind)).length, 0);
      const chars = (batches) => batches.reduce((m, b) => m + b.units.reduce((k, u) => k + (u.done ? 0 : u.text.length), 0), 0);
      say(`Game strings: ${strings.read} read from ${o.strings} (${size} bytes); `
        + (plan.strings ? `${count(strings.batches)} to translate, ${chars(strings.batches)} characters.` : 'none chosen.'));
      const { all, batches } = plan.texts;
      if (all.look.length + all.books.length > 0) {
        say(`Object names and books: ${all.look.length} names and ${all.books.length} books read; ${count(batches, 'look')} names and `
          + `${count(batches, 'book')} books to translate, ${chars(batches)} characters.`);
      }
      if (program.read > 0) {
        say(`Opening and creation: ${program.read} strings of u read; `
          + (plan.program ? `${count(program.batches)} to translate, ${chars(program.batches)} characters.` : 'none chosen.'));
      }
    }
  } catch (err) {
    if (err instanceof InputError) say(`translate: ${err.message}`);
    else say(`translate: the input does not read (${err?.name ?? 'Error'})`);
    return 2;
  }
  // Slice K5b: the Korean word settled for a key before a script's request: the script's own in
  // the pack there (`own`, none with --fresh-keywords), else the shared keywords' answer's
  // (`known`). A request again lists the keys with a word, each with it.
  const known = new Map();
  const jobOf = (g, units, again = false) => {
    if (g.kind !== 'script') {
      return {
        script: g,
        body: g.kind === 'strings' ? stringsBodyOf(o.model, stringsSystem, units)
          : g.kind === 'texts' ? textsBodyOf(o.model, textsSystem, units)
            : g.kind === 'program' ? programBodyOf(o.model, programSystem, units) : glossaryBodyOf(o.model, glossarySystem, units),
      };
    }
    const topics = again ? topicsOf(g, new Map([...g.keyed.words].map(([k, ws]) => [k, firstWord(ws)])), new Set(g.keyed.words.keys()))
      : topicsOf(g, new Map([...known, ...(g.own ?? [])]));
    return { script: g, body: bodyOf(o.model, topics.length > 0 ? keyedSystem : system, g.bundle, g.cls, units, topics, g.speaker) };
  };
  const groups = [...plan.scripts, ...(plan.strings?.batches ?? []), ...(plan.texts?.batches ?? []), ...(plan.program?.batches ?? [])];
  const jobsOf = () => [
    ...(plan.glossary ? [jobOf(plan.glossary, plan.glossary.units)] : []),
    ...groups.map((g) => jobOf(g, g.units.filter((u) => !u.done))).filter((j) => j.script.units.some((u) => !u.done)),
  ];
  if (jobsOf().length === 0) {
    say('Nothing to translate: nothing was sent.');
    return 1;
  }

  // The boundary: the route named, the first payload written, and the owner's go.
  say(`Route: llmux on this machine (${route.origin}), then ${providerOf(o.model)} (model ${o.model}).`);
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
      if (plan.texts && !textsRecheckOf(existing, plan.texts.all)) {
        say(`translate: the pack in ${o.out} was made from other game files (its object names or books): move it away to make a new one; nothing was sent.`);
        return 2;
      }
      say(`Pack: ${packPath} holds ${existing.converse.length} lines, ${existing.tables.length} table strings and `
        + `${existing.strings.length} game strings${existing.look.length + existing.books.length > 0 ? `, ${existing.look.length} object names and ${existing.books.length} books` : ''}`
        + `${existing.u.length > 0 ? `, ${existing.u.length} strings of u` : ''}`
        + `${existing.aliases.length > 0 ? `, ${existing.aliases.length} keyword aliases` : ''}; what this run translates is merged into it.`);
    }
    // Each script's own words in the pack (none with --fresh-keywords), and the keys shared by
    // enough of the scripts that one of them has no word for.
    for (const s of plan.scripts) s.own = o.freshKeywords ? new Map() : knownOf(existing, s);
    plan.glossary = glossaryOf(plan.scripts, settledOf(plan.scripts));
    const jobs = jobsOf();
    const parts = new Set(jobs.map((j) => j.script.kind));
    const npcSaid = plan.speakers ? 'its NPC\'s name, description, age and personality note' : 'its NPC\'s name and description';
    const what = [plan.scripts.length > 0 ? `one script's lines (and its keywords) with ${npcSaid}` : null,
      parts.has('glossary') ? `the keywords at least ${GLOSSARY_SCRIPTS} of the scripts share (up to ${GLOSSARY_KEYS}, each with up to `
        + `${GLOSSARY_SHOWN} of the words their lines highlight)` : null,
      parts.has('strings') ? `up to ${STRINGS_PER_REQUEST} of the game's own strings` : null,
      parts.has('texts') ? `up to ${LOOK_PER_REQUEST} object names or ${BOOKS_PER_REQUEST} books` : null,
      parts.has('program') ? `up to ${U_PER_REQUEST} lines of the opening and character creation` : null].filter(Boolean).join(', or ');
    say(`It sends the owner's game text: ${jobs.length} requests, each ${what}; up to ${o.jobs} at a time, `
      + `${parts.has('glossary') ? 'the shared keywords first, ' : ''}and a request again for lines that fail the checks.`);
    if (o.freshKeywords) {
      say('Keywords afresh (--fresh-keywords): no word of the pack there is sent as known in a script\'s first request (a line '
        + 'asked again names the word its key keeps, the pack\'s where this run kept none); each script answers its keys from its '
        + `own lines, the keys at least ${GLOSSARY_SCRIPTS} of them share asked first; the words a script's answer keeps (past the `
        + 'alias gate) replace the pack\'s for its keys, and a script whose request fails keeps its own.');
    }
    return await consentAndSend(o, plan, jobs, jobOf, existing, known, route, { transport, stdin, stdout, isTTY, signals, now, say });
  } finally {
    await release();
  }
}

// From the preview to the last file, with `o.out`'s lock held (tools/speech/profile.mjs's order):
// the preview of this run (a name of its own), the owner's go, the requests, the gates, the
// files. `known`: the words the shared keywords' answer settles for the keys (it adds to it), each
// script's own words (`own`) going before them in its request.
async function consentAndSend(o, plan, jobs, jobOf, existing, known, route, { transport, stdin, stdout, isTTY, signals, now, say }) {
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

  // The requests: the shared keywords first (slice K5b: their words reach as `known` the request of
  // each script with no word of its own for the key; the scripts go without them when it fails),
  // then each script and each batch of game strings once (asked again once when the request fails
  // or its answer does not read), then once more for its lines that failed the gates.
  const stopper = new AbortController();
  const onSignal = () => stopper.abort();
  signals.on('SIGINT', onSignal);
  const t0 = now();
  const counts = { requests: 0, retried: 0, input: 0, output: 0, usage: 0 };
  let halted = '';
  const send = async (list, tries, parse = linesOfAnswer) => {
    const sent = await sendAll(list, {
      transport: transport ?? llmuxTransport(route),
      jobs: o.jobs,
      timeoutMs: o.timeoutS * 1000,
      stop: stopper.signal,
      parse,
      tries,
    });
    for (const k of Object.keys(counts)) counts[k] += sent.counts[k];
    halted ||= sent.halted;
    return sent.results;
  };
  let again = [];
  try {
    let list = jobs;
    if (plan.glossary) {
      const gl = plan.glossary;
      const [r] = await send([jobs[0]], 2, keywordsOfAnswer);
      const { words, failed } = r.ok ? aliasesOf(gl.units.map((u) => u.k), r.keywords, gl.compatible) : { words: new Map(), failed: new Map() };
      for (const u of gl.units) {
        if (words.has(u.k)) u.ko = words.get(u.k);
        else u.why = r.ok ? failed.get(u.k) : r.reason;
      }
      for (const [k, w] of words) known.set(k, w);
      list = jobs.slice(1).map((j) => (j.script.kind === 'script' ? jobOf(j.script, j.script.units.filter((u) => !u.done)) : j));
    }
    // A route that refused the shared keywords' request is sent nothing more.
    const first = halted ? list.map(() => ({ ok: false, reason: 'not-sent' })) : await send(list, 2);
    first.forEach((r, i) => {
      const s = list[i].script;
      if (s.kind === 'script') settleKeys(s, r, existing);
      const { good, failed } = checked(s, r.ok ? r : null);
      for (const u of s.units) {
        if (u.done) continue;
        if (good.has(u.n)) Object.assign(u, { ko: good.get(u.n), done: true });
        else u.why = r.ok ? failed.get(u.n) : r.reason;
      }
      // Lines an answer gave that failed the gates (or that it lacked) are asked again once; a
      // request that failed was already asked again.
      const retry = s.units.filter((u) => !u.done);
      if (r.ok && retry.length > 0 && !halted && !stopper.signal.aborted) again.push(jobOf(s, retry, true));
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
    if (coverage.ages) {
      say(`Scripts by age (translated of chosen): ${AGES.map((a) => `${a} ${coverage.ages[a].translated} of ${coverage.ages[a].chosen}`).join(', ')}.`);
    }
    const t = coverage.tables;
    if (t.found > 0) say(`Table strings: ${t.found} found, ${t.translated} translated, ${t.leftEnglish} left English${reasons(t)}.`);
    const g = coverage.glossary;
    if (g.found > 0) say(`Shared keywords: ${g.found} asked before the scripts, ${g.translated} given a word${reasons(g)}.`);
    const k = coverage.keywords;
    if (k.found > 0 || k.dropped > 0) {
      const keptOld = k.kept.count > 0 ? `, ${k.kept.count} kept the pack's word${reasons(k.kept)}` : '';
      say(`Keywords: ${k.found} found, ${k.translated} given a Korean word${keptOld}, ${k.leftEnglish} left English${reasons(k)}`
        + `${k.dropped > 0 ? `; ${k.dropped} lines the pack had left English again (their keywords' words changed)` : ''}.`);
    }
  }
  if (plan.strings) {
    const c = coverage.strings;
    say(`Game strings: ${c.found} found, ${c.translated} translated, ${c.leftEnglish} left English${reasons(c)}.`);
  }
  for (const [what, c] of [['Object names', coverage.look], ['Books', coverage.books], ['Opening and creation', coverage.u]]) {
    if (c.found > 0) say(`${what}: ${c.found} found, ${c.translated} translated, ${c.leftEnglish} left English${reasons(c)}.`);
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
    const texts = out.total.look + out.total.books > 0 ? `, ${out.total.look} object names, ${out.total.books} books` : '';
    const program = out.total.u > 0 ? `, ${out.total.u} strings of u` : '';
    const aliases = out.total.aliases > 0 ? `, ${out.total.aliases} keyword aliases` : '';
    written.push(`${path} (${await writePrivate(path, out.pack)} bytes: ${out.total.runs} lines, ${out.total.tables} table strings, `
      + `${out.total.strings} game strings${texts}${program}${aliases}; `
      + `${out.runs + out.tables + out.strings + out.look + out.books + out.u + out.aliases} from this run)`);
  }
  const cpath = join(o.out, 'coverage.json');
  written.push(`${cpath} (${await writePrivate(cpath, JSON.stringify(coverage, null, 1))} bytes)`);
  say(`Wrote ${written.join(', ')}.`);
  if (out.pack) say('Drop ko.u6lp on the page and choose 한글; coverage.json stays here.');
  else say(existing ? 'No line was translated: the pack there is left as it was.' : 'No line was translated: no pack was written.');
  if (stopper.signal.aborted) return 130;
  if (!out.pack) return 3;
  const leftEnglish = coverage.units.leftEnglish + coverage.tables.leftEnglish + coverage.strings.leftEnglish
    + coverage.look.leftEnglish + coverage.books.leftEnglish + coverage.u.leftEnglish + coverage.keywords.leftEnglish
    + coverage.keywords.dropped;
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

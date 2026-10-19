#!/usr/bin/env node
// Personality voices (spec 09 Design 4, Order 5): reads the NPC bundles the page downloads
// (ultima6-npc-bundles.json, one u6.npc-bundle/1 per conversation script), asks a model for each
// script's voice from its NPC's name, description and lines, and writes the local voice pack the
// page takes in a drop, beside the personality notes and a coverage receipt:
//
//   <out>/voices.u6vp     u6.voice-pack/1: {format, model, revision, profiles: {<actor>: profile}}
//                         for every actor of each script profiled (numbers only, no text)
//   <out>/notes.json      the model's short personality note per script (local only; never
//                         dropped on the page)
//   <out>/coverage.json   the scripts read against the profiles made: counts, and the scripts
//                         skipped or failed by (archive, item) with a fixed reason
//
// <out> is ~/Games/ultima6/voice/ unless --out names another; it and preview/ are made 0700,
// every file in them 0600.
//
// The upload boundary (spec 08 Rights, used unchanged): the bundles hold the owner's game text,
// and a request sends one NPC's of it to llmux on this machine, then to the model's provider.
// Before the first request the tool names that route, writes the exact first payload to a file
// of this run's own, <out>/preview/first-request-<pid>-<random>.json, and prints only its path and
// size, never its text. It sends only after a typed `yes` on a terminal, or with --send; anything
// else (another answer, an empty one, Ctrl-C, no terminal and no --send) exits with no request.
// --dry-run writes the preview and exits. A run holds <out>/.lock (made exclusively, holding its
// pid) from before the preview to its last write: a second run on the same <out> refuses while
// the first lives, so the preview the owner reads is the payload his `yes` sends. llmux's answer
// is never followed elsewhere: a redirect is a failed request, and none more is sent. A request is llmux's Messages call (POST /v1/messages: the one llmux serves for every
// account, OpenAI's included) with the model's name, an output cap, this tool's instruction and
// the NPC's text: no other client's prompt or headers.
//
// Each request is bounded: the NPC's name, its description (the look text) and its greeting and
// answers in script order, each line at most LINE_MAX characters, up to TEXT_BUDGET in all. The
// answer must be JSON only, {mix: [{style, w}] (1 to 3 of the ten presets), speed, pitch, note};
// one that is not (or empty, or a failed request) is asked again once, then counted as a failure.
// Its numbers are taken into the ranges by site/voice/profile.js checkedProfile, and the pack is
// checked by readPack, the page's own check, before it is written. At most four requests run at
// a time, each with a timeout.
//
// Rights: what this prints is counts, timings, paths and sizes, and the route (the address and
// the model's name): never a name, a line, a note, an actor number or a script's (archive,
// item). An error prints its kind only (a JSON error quotes its input).
//
//   node tools/speech/profile.mjs <bundles.json> [--send | --dry-run] [--model NAME]
//        [--llmux URL] [--out DIR] [--jobs N] [--timeout SECONDS]
import { randomBytes } from 'node:crypto';
import { constants } from 'node:fs';
import { createInterface } from 'node:readline';
import { chmod, mkdir, open, readFile, rename, stat, unlink } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { STYLES } from '../../site/voice/model.js';
import {
  checkedProfile, MAX_MIX, PACK_FORMAT, PACK_MAX, PITCH, RANGES_REVISION, readPack, SPEED, STEPS,
} from '../../site/voice/profile.js';

export const DEFAULTS = {
  model: 'gpt-6-astra',
  llmux: 'http://127.0.0.1:3456',
  out: join(homedir(), 'Games', 'ultima6', 'voice'),
  jobs: 4,
  timeoutS: 120,
};
export const JOBS_MAX = 4;
export const TEXT_BUDGET = 1600; // characters of an NPC's text in one request
export const LINE_MAX = 320; // characters of one line
export const NOTE_MAX = 200; // characters of a note kept
export const MAX_TOKENS = 2048; // the answer's cap (a reasoning model's thinking counts in it)
const BUNDLE_FORMAT = 'u6.npc-bundle/1';
const BUNDLES_MAX = 256 << 20; // the most bytes of a bundles file read
const NOTES_FORMAT = 'u6.voice-notes/1';
const COVERAGE_FORMAT = 'u6.voice-coverage/1';
export const PREVIEW = /^first-request(-[0-9]+-[0-9a-f]{12})?\.json$/; // a run's preview (and the old fixed name)
const LOCK = '.lock';
// The route refuses every request (and any 3xx: it would send the text elsewhere): none more is
// sent, and this one is not tried again.
const HALTING = new Set([401, 403, 404]);

/// What the model is told (the same for every request).
export const SYSTEM = [
  'You choose a voice for one character of a role-playing game, for an on-device text-to-speech',
  `model that mixes ten preset voices: ${STYLES.filter((s) => s[0] === 'F').join(', ')} (female) and`,
  `${STYLES.filter((s) => s[0] === 'M').join(', ')} (male). Read the character's name, how they are`,
  'described and what they say, and answer with one JSON object and nothing else:',
  '{"mix": [{"style": "<preset>", "w": <weight>}], "speed": <number>, "pitch": <number>, "note": "<text>"}',
  `- mix: 1 to ${MAX_MIX} presets, usually of the character's sex, each with a weight above 0 (the`,
  '  weights are normalised); more than one blends them into a new voice.',
  `- speed: ${SPEED.min} (slow, deliberate) to ${SPEED.max} (quick, excited); ${SPEED.base} is an`,
  '  ordinary pace.',
  `- pitch: ${PITCH.min} (deeper) to +${PITCH.max} (higher) semitones; 0 is the presets' own.`,
  `- note: one short line on the character's personality and manner of speech, under ${NOTE_MAX}`,
  '  characters.',
].join('\n');

const plain = (x) => x !== null && typeof x === 'object' && !Array.isArray(x);

/// An input that is not what the tool reads: its message is fixed text (a count at most).
export class InputError extends Error {}
const finite = (x) => typeof x === 'number' && Number.isFinite(x);

// --- the bundles -------------------------------------------------------------------------------

// A line of the script as the model reads it: the printing escapes the engine fills in at run
// time stand as an ellipsis, the keyword marks and waits go, whitespace is one space.
function clean(text) {
  return text.replace(/@/g, '').replace(/\*/g, ' ').replace(/\$[A-Za-z0-9]|#[0-9]/g, '…')
    .replace(/\s+/g, ' ').trim();
}

// The text of `segments` (u6.npc-bundle/1): its text runs joined, a value printed at run time
// (`var`) as an ellipsis; ops and conditions print nothing.
function textOf(segments) {
  let out = '';
  for (const s of Array.isArray(segments) ? segments : []) {
    if (s?.kind === 'text' && typeof s.text === 'string') out += s.text;
    else if (s?.kind === 'var') out += '…';
  }
  return clean(out);
}

const cut = (line) => (line.length > LINE_MAX ? `${line.slice(0, LINE_MAX - 1)}…` : line);

/// What one request says of `bundle`'s NPC, within TEXT_BUDGET: its name, its description, then
/// its greeting and its answers in script order (each line once). `chars`: the text's length.
export function npcText(bundle) {
  const name = typeof bundle.name === 'string' ? clean(bundle.name) : '';
  const look = cut(textOf(bundle.narration?.look));
  const said = [textOf(bundle.speech?.greeting)];
  for (const turn of Array.isArray(bundle.turns) ? bundle.turns : []) {
    for (const topic of Array.isArray(turn?.topics) ? turn.topics : []) said.push(textOf(topic?.answer));
  }
  let used = name.length + look.length;
  const lines = [];
  const seen = new Set();
  for (const line of said.map(cut)) {
    if (!line || seen.has(line)) continue;
    if (used + line.length > TEXT_BUDGET) break;
    seen.add(line);
    lines.push(line);
    used += line.length;
  }
  const text = [`Name: ${name || '(none)'}`];
  if (look) text.push(`Described as: ${look}`);
  text.push('What they say:', ...(lines.length > 0 ? lines.map((l) => `- ${l}`) : ['(nothing)']));
  return { text: text.join('\n'), chars: used, empty: !name && !look && lines.length === 0 };
}

/// The request body for `text` (the exact bytes sent): a Messages call.
export function bodyOf(model, text) {
  return JSON.stringify({ model, max_tokens: MAX_TOKENS, system: SYSTEM, messages: [{ role: 'user', content: text }] });
}

/// The bundles of `value` (the parsed file) as the jobs to send, in (archive, item) order, and the
/// scripts skipped: refused by the exporter, used by no actor, or with no name and no text. Throws
/// a fixed message (a count only) when it is not a list of bundles.
export function planOf(value, model) {
  if (!Array.isArray(value)) throw new InputError('the file is not a list of NPC bundles');
  const bad = value.filter((b) => !plain(b) || b.format !== BUNDLE_FORMAT || !['a', 'b'].includes(b.archive)
    || !Number.isInteger(b.item) || !Array.isArray(b.actors) || !b.actors.every(Number.isInteger)).length;
  if (bad > 0) throw new InputError(`${bad} entries of the file are not ${BUNDLE_FORMAT} bundles`);
  const sorted = [...value].sort((x, y) => (x.archive === y.archive ? x.item - y.item : x.archive < y.archive ? -1 : 1));
  const jobs = [];
  const skipped = [];
  for (const b of sorted) {
    const key = { archive: b.archive, item: b.item };
    if (b.refused !== null && b.refused !== undefined) {
      skipped.push({ ...key, actors: b.actors, reason: 'refused' });
      continue;
    }
    if (b.actors.length === 0) {
      skipped.push({ ...key, actors: [], reason: 'no-actors' });
      continue;
    }
    const npc = npcText(b);
    if (npc.empty) {
      skipped.push({ ...key, actors: b.actors, reason: 'no-text' });
      continue;
    }
    jobs.push({ ...key, actors: b.actors, chars: npc.chars, body: bodyOf(model, npc.text) });
  }
  return { scripts: value.length, jobs, skipped };
}

// --- the answers -------------------------------------------------------------------------------

/// The answer's text in a Messages answer (its `content` blocks of type text, joined), '' without
/// one.
export function contentOf(json) {
  const content = json?.content;
  if (!Array.isArray(content)) return '';
  return content.map((b) => (b?.type === 'text' && typeof b.text === 'string' ? b.text : '')).join('');
}

/// The profile and note of an answer, or null when it is not one: JSON (a code fence around it
/// allowed) of an object with 1 to MAX_MIX presets of a known name and a weight above 0, a finite
/// speed and pitch and a note. The numbers are taken into the ranges (checkedProfile).
export function profileOfAnswer(content) {
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
  if (!plain(v) || !Array.isArray(v.mix) || v.mix.length < 1 || v.mix.length > MAX_MIX) return null;
  if (!v.mix.every((e) => plain(e) && STYLES.includes(e.style) && finite(e.w) && e.w > 0)) return null;
  if (!finite(v.speed) || !finite(v.pitch) || typeof v.note !== 'string') return null;
  const note = v.note.replace(/\s+/g, ' ').trim().slice(0, NOTE_MAX);
  if (!note) return null;
  const profile = checkedProfile({ mix: v.mix, speed: v.speed, pitch: v.pitch, steps: STEPS.max, seedSalt: 0 });
  return { profile, note };
}

// --- the route ---------------------------------------------------------------------------------

/// The provider a model's name goes to past llmux.
export function providerOf(model) {
  if (/^(gpt-|o[0-9]|codex)/i.test(model)) return 'OpenAI';
  if (/^claude-/i.test(model)) return 'Anthropic';
  if (/^gemini-/i.test(model)) return 'Google';
  if (/^grok-/i.test(model)) return 'xAI';
  return `the provider that serves ${model}`;
}

/// Whether `url` is an address of this machine.
function local(url) {
  return ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname);
}

/// The transport to llmux: a POST of `body` to its Messages call; `{status, json}` (json null when
/// the answer does not parse). Nothing else goes with it: no other client's headers. A redirect is
/// not followed (`redirect: 'manual'`): its 3xx is the answer, so the text never goes to the place
/// it names, on this machine or off it.
export function llmuxTransport(base) {
  const url = new URL('/v1/messages', base);
  return async (body, { signal }) => {
    const response = await fetch(url, {
      method: 'POST', headers: { 'content-type': 'application/json' }, body, signal, redirect: 'manual',
    });
    const text = await response.text();
    let json = null;
    try {
      json = JSON.parse(text);
    } catch {
      // Counted as an empty answer.
    }
    return { status: response.status, json };
  };
}

// --- files -------------------------------------------------------------------------------------

/// `dir` made (with its parents) and set to 0700.
async function privateDir(dir) {
  await mkdir(dir, { recursive: true, mode: 0o700 });
  await chmod(dir, 0o700);
}

// Create a new file only, never through a link: O_CREAT | O_EXCL (a name that exists, a symlink
// included, fails with EEXIST and is left as it is) | O_NOFOLLOW, write only, mode 0600 at
// creation.
const NEW_FILE = constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW;
const random = () => randomBytes(6).toString('hex');

/// `text` written to `path` (mode 0600), through a new file beside it, named `<path>.part-<pid>-
/// <random>` (`suffix` gives the random part: the tests' to give), made exclusively (another name
/// when one exists: nothing there is followed, opened or changed), set to 0600 on its own handle
/// (whatever the umask took), then renamed over `path`. Returns the bytes written.
export async function writePrivate(path, text, suffix = random) {
  let part;
  let f;
  for (let tries = 0; !f; tries += 1) {
    part = `${path}.part-${process.pid}-${suffix()}`;
    try {
      f = await open(part, NEW_FILE, 0o600);
    } catch (err) {
      if (err?.code !== 'EEXIST' || tries >= 8) throw err;
    }
  }
  try {
    await f.chmod(0o600);
    await f.writeFile(text);
  } catch (err) {
    await f.close();
    await unlink(part).catch(() => {});
    throw err;
  }
  await f.close();
  await rename(part, path);
  return Buffer.byteLength(text);
}

/// Whether the process `pid` lives (one of another user's counts: it cannot be told apart).
function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (err) {
    return err?.code === 'EPERM';
  }
}

/// `<dir>/.lock`, made exclusively and holding this process's pid: a release function, or null
/// while another run that lives holds it (one whose process is gone is taken over).
export async function holdLock(dir) {
  const path = join(dir, LOCK);
  for (let tries = 0; tries < 2; tries += 1) {
    try {
      const f = await open(path, NEW_FILE, 0o600);
      try {
        await f.writeFile(`${process.pid}\n`);
      } finally {
        await f.close();
      }
      return async () => unlink(path).catch(() => {});
    } catch (err) {
      if (err?.code !== 'EEXIST') throw err;
    }
    let pid = NaN;
    try {
      pid = Number((await readFile(path, { encoding: 'utf8', flag: constants.O_RDONLY | constants.O_NOFOLLOW })).trim());
    } catch {
      // Not a lock this tool wrote: held.
    }
    if (!Number.isInteger(pid) || pid <= 0 || alive(pid)) return null;
    await unlink(path).catch(() => {});
  }
  return null;
}

// --- the run -----------------------------------------------------------------------------------

/// Sends `jobs` (at most `jobs` at a time, each try `timeoutMs` long) through `transport` until
/// all are answered or `stop` aborts; each is tried twice at most. Returns a result per job
/// ({ok, profile, note} or {ok: false, reason}) and the counts.
export async function sendAll(list, { transport, jobs, timeoutMs, stop, progress }) {
  const results = new Array(list.length);
  const counts = { requests: 0, retried: 0, input: 0, output: 0, usage: 0 };
  let halted = '';
  let next = 0;
  let done = 0;
  async function one(job) {
    let reason = '';
    for (let attempt = 0; attempt < 2; attempt += 1) {
      if (stop.aborted) return { ok: false, reason: 'interrupted' };
      if (halted) return { ok: false, reason: 'not-sent' };
      if (attempt > 0) counts.retried += 1;
      counts.requests += 1;
      let r;
      try {
        r = await transport(job.body, { signal: AbortSignal.any([stop, AbortSignal.timeout(timeoutMs)]) });
      } catch (err) {
        if (stop.aborted) return { ok: false, reason: 'interrupted' };
        reason = err?.name === 'TimeoutError' ? 'timeout' : 'network';
        continue;
      }
      if (r?.status !== 200) {
        reason = `http-${Number(r?.status) || 0}`;
        if (HALTING.has(r?.status) || (r?.status >= 300 && r?.status < 400)) {
          halted = reason;
          return { ok: false, reason };
        }
        continue;
      }
      const usage = r.json?.usage;
      if (plain(usage)) {
        const [input, output] = [usage.input_tokens, usage.output_tokens].map((n) => (finite(n) ? n : 0));
        counts.usage += 1;
        counts.input += input;
        counts.output += output;
      }
      const content = contentOf(r.json);
      const got = profileOfAnswer(content);
      if (got) return { ok: true, ...got };
      reason = content.trim() ? 'invalid' : 'empty';
    }
    return { ok: false, reason };
  }
  const workers = Array.from({ length: Math.min(jobs, list.length) }, async () => {
    while (next < list.length) {
      const i = next;
      next += 1;
      results[i] = await one(list[i]);
      done += 1;
      progress?.(done, results);
    }
  });
  await Promise.all(workers);
  return { results, counts, halted };
}

/// The pack, the notes and the coverage receipt of `plan`'s jobs and their `results`.
export function outputsOf(plan, results, model) {
  const profiles = {};
  const notes = [];
  const failures = [];
  let duplicate = 0;
  plan.jobs.forEach((job, i) => {
    const r = results[i] ?? { ok: false, reason: 'not-sent' };
    if (!r.ok) {
      failures.push({ archive: job.archive, item: job.item, actors: job.actors.length, reason: r.reason });
      return;
    }
    for (const actor of job.actors) {
      if (Object.hasOwn(profiles, actor)) duplicate += 1;
      else profiles[actor] = r.profile;
    }
    notes.push({ archive: job.archive, item: job.item, actors: job.actors, note: r.note });
  });
  const actorsOf = (list) => list.reduce((n, x) => n + (typeof x.actors === 'number' ? x.actors : x.actors.length), 0);
  const profiledScripts = notes.length;
  const coverage = {
    format: COVERAGE_FORMAT,
    model,
    revision: RANGES_REVISION,
    scripts: { read: plan.scripts, profiled: profiledScripts, failed: failures.length, skipped: plan.skipped.length },
    actors: {
      inScripts: actorsOf(plan.jobs) + actorsOf(plan.skipped),
      profiled: Object.keys(profiles).length,
      inFailed: actorsOf(failures),
      inSkipped: actorsOf(plan.skipped),
      repeated: duplicate,
    },
    unexplained: plan.scripts - profiledScripts - failures.length - plan.skipped.length,
    failures,
    skipped: plan.skipped.map((s) => ({ archive: s.archive, item: s.item, actors: s.actors.length, reason: s.reason })),
  };
  return {
    pack: { format: PACK_FORMAT, model, revision: RANGES_REVISION, profiles },
    notes: { format: NOTES_FORMAT, model, notes },
    coverage,
  };
}

const USAGE = 'usage: node tools/speech/profile.mjs <bundles.json> [--send | --dry-run] [--model NAME] '
  + `[--llmux URL] [--out DIR] [--jobs 1-${JOBS_MAX}] [--timeout SECONDS]`;

/// The options of `argv`, or {error}.
export function parseArgs(argv) {
  const o = { send: false, dryRun: false, model: DEFAULTS.model, llmux: DEFAULTS.llmux, out: DEFAULTS.out,
    jobs: DEFAULTS.jobs, timeoutS: DEFAULTS.timeoutS, bundles: null, help: false };
  const value = (i) => (i + 1 < argv.length ? argv[i + 1] : null);
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--send') o.send = true;
    else if (a === '--dry-run') o.dryRun = true;
    else if (a === '--help' || a === '-h') o.help = true;
    else if (['--model', '--llmux', '--out', '--jobs', '--timeout'].includes(a)) {
      const v = value(i);
      if (v === null) return { error: `${a} needs a value` };
      i += 1;
      if (a === '--model') o.model = v;
      else if (a === '--llmux') o.llmux = v;
      else if (a === '--out') o.out = resolve(v);
      else if (a === '--jobs') o.jobs = Number(v);
      else o.timeoutS = Number(v);
    } else if (a.startsWith('--')) return { error: `unknown option ${a}` };
    else if (o.bundles === null) o.bundles = resolve(a);
    else return { error: 'one bundles file only' };
  }
  if (o.help) return o;
  if (o.bundles === null) return { error: 'no bundles file named' };
  if (!Number.isInteger(o.jobs) || o.jobs < 1 || o.jobs > JOBS_MAX) return { error: `--jobs is 1 to ${JOBS_MAX}` };
  if (!finite(o.timeoutS) || o.timeoutS <= 0) return { error: '--timeout is a number of seconds above 0' };
  if (!/^[\w.:[\]/-]{1,100}$/.test(o.model)) return { error: '--model is a model name' };
  return o;
}

// Asks for `yes` on the terminal: 'yes', 'no' (any other answer, an empty one, or the input's
// end) or 'interrupted' (Ctrl-C at the prompt, or SIGINT).
function ask(question, { stdin, stdout, signals }) {
  return new Promise((done) => {
    const rl = createInterface({ input: stdin, output: stdout, terminal: true });
    let settled = false;
    const finish = (answer) => {
      if (settled) return;
      settled = true;
      signals.off('SIGINT', onSignal);
      rl.close();
      done(answer);
    };
    const onSignal = () => finish('interrupted');
    signals.on('SIGINT', onSignal);
    rl.on('SIGINT', onSignal);
    rl.on('close', () => finish('no'));
    rl.question(question, (answer) => finish(answer.trim() === 'yes' ? 'yes' : 'no'));
  });
}

const seconds = (ms) => `${(ms / 1000).toFixed(1)} s`;

/// The tool: returns its exit code (0 done or a dry run, 1 nothing sent, 2 a usage or input
/// error, 3 written with failures, 130 interrupted). `transport` (default: llmux at --llmux),
/// `stdin`, `stdout`, `isTTY` (whether to ask on stdin) and `signals` (SIGINT's emitter) are the
/// tests' to give.
export async function main(argv, {
  transport = null, stdin = process.stdin, stdout = process.stdout, isTTY = Boolean(process.stdin.isTTY),
  signals = process, now = () => performance.now(),
} = {}) {
  const say = (line) => stdout.write(`${line}\n`);
  const o = parseArgs(argv);
  if (o.error) {
    say(`profile: ${o.error}\n${USAGE}`);
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
    say('profile: --llmux is the address of llmux on this machine (127.0.0.1, localhost or [::1])');
    return 2;
  }

  // The bundles, and what to send of them.
  let plan;
  let size;
  try {
    size = (await stat(o.bundles)).size;
    if (size > BUNDLES_MAX) {
      say(`profile: the bundles file is ${size} bytes, more than ${BUNDLES_MAX}`);
      return 2;
    }
    let value;
    try {
      value = JSON.parse(await readFile(o.bundles, 'utf8'));
    } catch (err) {
      say(`profile: ${err?.code ? `the bundles file cannot be read (${err.code})` : 'the bundles file does not read as JSON'}`);
      return 2;
    }
    plan = planOf(value, o.model);
  } catch (err) {
    if (err instanceof InputError) say(`profile: ${err.message}`);
    else say(`profile: ${err?.code ? `the bundles file cannot be read (${err.code})` : `the bundles do not read (${err?.name ?? 'Error'})`}`);
    return 2;
  }
  const reasons = (r) => plan.skipped.filter((s) => s.reason === r).length;
  const actors = plan.jobs.reduce((n, j) => n + j.actors.length, 0);
  const chars = plan.jobs.reduce((n, j) => n + j.chars, 0);
  say(`Bundles: ${plan.scripts} scripts read from ${o.bundles} (${size} bytes): ${plan.jobs.length} to profile `
    + `(${actors} actors), ${plan.skipped.length} skipped (refused ${reasons('refused')}, no actors `
    + `${reasons('no-actors')}, no text ${reasons('no-text')}).`);
  if (plan.jobs.length === 0) {
    say('Nothing to profile: nothing was sent.');
    return 1;
  }

  // The boundary: the route named, the first payload written, and the owner's go.
  say(`Route: llmux on this machine (${route.origin}), then ${providerOf(o.model)} (model ${o.model}).`);
  say(`It sends the owner's game text: ${plan.jobs.length} requests, each one NPC's name and up to `
    + `${TEXT_BUDGET} characters of its lines, ${chars} characters in all; up to ${o.jobs} at a time.`);
  let release;
  try {
    await privateDir(o.out);
    release = await holdLock(o.out);
  } catch (err) {
    say(`profile: ${o.out} cannot be used (${err?.code ?? err?.name ?? 'Error'}): nothing was sent.`);
    return 2;
  }
  if (!release) {
    say(`profile: another run of this tool holds ${join(o.out, LOCK)} (remove it if none runs): nothing was sent.`);
    return 2;
  }
  try {
    return await consentAndSend(o, plan, route, { transport, stdin, stdout, isTTY, signals, now, say });
  } finally {
    await release();
  }
}

// From the preview to the last file, with `o.out`'s lock held: the preview of this run (a name of
// its own; previews of other runs are never removed, so one another run may still be showing the
// owner stays as printed), the owner's go, the requests, the files.
async function consentAndSend(o, plan, route, { transport, stdin, stdout, isTTY, signals, now, say }) {
  const previewDir = join(o.out, 'preview');
  const preview = join(previewDir, `first-request-${process.pid}-${random()}.json`);
  try {
    await privateDir(previewDir);
    const bytes = await writePrivate(preview, plan.jobs[0].body);
    say(`The exact first request: ${preview} (${bytes} bytes).`);
  } catch (err) {
    say(`profile: the preview cannot be written (${err?.code ?? err?.name ?? 'Error'}): nothing was sent.`);
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
    const answer = await ask(`Type yes to send ${plan.jobs.length} requests: `, { stdin, stdout, signals });
    if (answer === 'interrupted') {
      say('\nNothing was sent (interrupted).');
      return 130;
    }
    if (answer !== 'yes') {
      say('Nothing was sent.');
      return 1;
    }
  }

  // The requests.
  const stopper = new AbortController();
  const onSignal = () => stopper.abort();
  signals.on('SIGINT', onSignal);
  const t0 = now();
  let sent;
  try {
    const step = Math.max(1, Math.ceil(plan.jobs.length / 10));
    sent = await sendAll(plan.jobs, {
      transport: transport ?? llmuxTransport(route),
      jobs: o.jobs,
      timeoutMs: o.timeoutS * 1000,
      stop: stopper.signal,
      progress: (done, results) => {
        if (done % step === 0 && done < plan.jobs.length) {
          say(`  ${done} of ${plan.jobs.length} answered (${results.filter((r) => r && !r.ok).length} failed), ${seconds(now() - t0)}`);
        }
      },
    });
  } finally {
    signals.off('SIGINT', onSignal);
  }
  const ms = now() - t0;
  const { pack, notes, coverage } = outputsOf(plan, sent.results, o.model);
  const c = sent.counts;
  const tokens = c.usage > 0 ? `tokens: input ${c.input}, output ${c.output} (${c.usage} answers counted)` : 'no token counts returned';
  say(`Sent: ${c.requests} requests (${c.retried} asked again) in ${seconds(ms)}; ${tokens}.`);
  if (sent.halted) say(`The route refused a request (${sent.halted}): no request was sent after it.`);
  const failed = coverage.scripts.failed;
  const why = [...new Set(coverage.failures.map((f) => f.reason))].sort()
    .map((r) => `${r} ${coverage.failures.filter((f) => f.reason === r).length}`).join(', ');
  say(`Profiles: ${coverage.scripts.profiled} scripts, ${coverage.actors.profiled} actors; failed ${failed} scripts`
    + `${failed > 0 ? ` (${why})` : ''}; skipped ${coverage.scripts.skipped}; unexplained ${coverage.unexplained}.`);

  // The pack, checked by the page's own check, then the files.
  const packText = JSON.stringify(pack);
  const checked = readPack(JSON.parse(packText));
  if (!checked.ok || checked.pack.size !== Object.keys(pack.profiles).length || checked.pack.size > PACK_MAX) {
    say(`profile: the pack does not check out (${checked.ok ? 'size' : checked.code}): nothing was written.`);
    return 2;
  }
  if (checked.pack.size === 0) {
    say('No profile was made: nothing was written.');
    return stopper.signal.aborted ? 130 : 3;
  }
  const written = [];
  for (const [name, text] of [['voices.u6vp', packText], ['notes.json', JSON.stringify(notes, null, 1)],
    ['coverage.json', JSON.stringify(coverage, null, 1)]]) {
    const path = join(o.out, name);
    written.push(`${path} (${await writePrivate(path, text)} bytes)`);
  }
  say(`Wrote ${written.join(', ')}.`);
  say('Drop voices.u6vp on the page; notes.json and coverage.json stay here.');
  if (stopper.signal.aborted) return 130;
  return failed > 0 ? 3 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const exit = (code) => process.stdout.write('', () => process.exit(code));
  main(process.argv.slice(2)).then(exit, (err) => {
    // Its kind only: a message could quote what was read.
    process.stdout.write(`profile: failed (${err?.name ?? 'Error'})\n`);
    exit(1);
  });
}

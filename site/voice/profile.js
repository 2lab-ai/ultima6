// A voice of its own for each NPC (spec 09 Design 4): the profile of who speaks, from the
// committed type-class table and the actor number, and the mixer of the model's ten presets.
// Shared by the player (site/voice/player.js), which picks a speaker's profile, and the worker
// (site/voice/worker.js), which mixes its presets.
//
// A profile is {mix: [{style, w}] (1 to 3 of the presets, the weights summing to 1), speed (0.9
// to 1.15: the model's pace, dividing its predicted duration), pitch (-2..+2 semitones), steps
// (the most denoising steps the voice takes: the page's adaptive count, capped at this),
// seedSalt (mixed into the line's noise seed: 0 keeps the seed of the speaker and the text), vary
// ({speed, pitch}, 0 to VARY's: how far one line may move from that speed and pitch, spec 15
// Design 4-5; 0/0 for a voice that says every line alike)}.
//
// Rights (spec 09 Rights): the table is keyed only by what the engine's code already names: the
// object types converse/start.rs names (SHRINE, STATUES, HORSES, TALKING_OBJECTS), the worktypes
// actors/npc.rs runs through a combat routine (runs_combat_routine) and the alignment bits
// (status & 0x60, combat/npc.rs find_target). Which object types are human, gargoyle or female is
// game knowledge and is not here: the voice's sex comes from the actor number's parity (odd F,
// even M), and the actor number picks the presets, their blend and the pitch, so neighbours
// differ: n and n+1 by sex, n and n+2 by the leading preset.
//
// An NPC always sounds the same: its worktype and alignment are read when it speaks and move with
// its day (a schedule, a fight, a charm), so they set only its pace, by a few hundredths, and the
// player keeps the first profile of each speaker for the session. The voice itself (presets and
// pitch) comes from the actor number and the object type's class.
//
// The local voice pack (`voices.u6vp`, spec 09 Design 4: made by tools/speech/profile.mjs from
// the owner's NPC bundles, dropped on the page) gives `profileFor` a map from actor number to
// profile, its personality voice. `readPack` is its one check, the tool's and the page's: a
// pack whose form, keys or values are not exactly this one's is refused whole, by a code that
// names only the kind. A pack is u6.voice-pack/1 (no vary: each voice 0/0) or /2 (each voice
// with its vary, spec 15 Design 4); the player moves each line by its voice's vary
// (`lineProfile`, spec 15 Design 5).
import { STYLES, VOICE } from './model.js';

/// The ranges of a profile.
export const SPEED = { min: 0.9, max: 1.15, base: 1.05 }; // base: the sample code's pace
export const PITCH = { min: -2, max: 2 };
export const STEPS = { min: 1, max: 16 };
export const MAX_MIX = 3;
/// The most a line moves from its voice's speed and pitch (a profile's `vary`, spec 15 Design 4).
/// Not in RANGES_REVISION: a /1 pack's voices keep their meaning.
export const VARY = { speed: { max: 0.15 }, pitch: { max: 1.5 } };

/// Voice classes: a pitch (semitones) and a pace offset each.
export const CLASSES = {
  plain: { pitch: 0, speed: 0 },
  deep: { pitch: -1, speed: -0.06 },
  low: { pitch: -0.5, speed: -0.03 },
  bright: { pitch: 1, speed: 0.04 },
};

/// The type-class table: the object types converse/start.rs names, each to its class
/// (start.rs:13-18).
export const TYPE_CLASSES = new Map([
  [0x189, 'deep'], // SHRINE
  [0x18d, 'deep'], [0x18e, 'deep'], [0x18f, 'deep'], // STATUES
  [0x1ae, 'low'], [0x1af, 'low'], // HORSES
  [0x17e, 'bright'], [0x175, 'bright'], [0x16b, 'bright'], [0x16a, 'bright'], // TALKING_OBJECTS
]);

/// Worktype buckets, each a pace offset: the worktypes 0x1200b0 runs through a combat routine
/// (actors/npc.rs runs_combat_routine: 3..=11, 13, 14, 0x13, 0x9a, 0x9b). Any other: 0.
export const WORKTYPE_BUCKETS = [
  { name: 'combat', worktypes: [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 0x13, 0x9a, 0x9b], speed: 0.04 },
];

/// Alignment buckets (Traits.alignment: status bits 5..6), each a pace offset: 0 neutral, 1 the
/// evil bit (0x20), 2 the good bit (0x40), 3 chaotic (0x60), as combat/npc.rs find_target reads
/// them.
export const ALIGNMENT_BUCKETS = new Map([[0, 0], [1, -0.03], [2, 0], [3, 0.03]]);

const clamp = (x, lo, hi) => Math.min(hi, Math.max(lo, x));
const finite = (x, fallback) => (typeof x === 'number' && Number.isFinite(x) ? x : fallback);

/// murmur3's finaliser over the actor number (or a line's seed) and a draw index: one 32-bit
/// draw per use.
export function draw(n, k) {
  let h = (Math.imul(n | 0, 0x9e3779b1) ^ Math.imul(k, 0x85ebca77) ^ 0x5bd1e995) | 0;
  h ^= h >>> 16;
  h = Math.imul(h, 0x85ebca6b);
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35);
  h ^= h >>> 16;
  return h >>> 0;
}
/// A draw as a number in [0, 1).
export const unit = (h) => h / 4294967296;

/// `mix` as the mixer takes it: entries of a known preset with a finite weight above 0, the same
/// preset's entries joined, the heaviest `MAX_MIX` kept (ties by name), the weights summing to
/// 1; none: the default voice alone.
export function checkedMix(mix) {
  const by = new Map();
  for (const e of Array.isArray(mix) ? mix : []) {
    const w = finite(e?.w, 0);
    if (!STYLES.includes(e?.style) || !(w > 0)) continue;
    by.set(e.style, (by.get(e.style) ?? 0) + w);
  }
  const kept = [...by].sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1)).slice(0, MAX_MIX);
  const sum = kept.reduce((s, [, w]) => s + w, 0);
  if (!(sum > 0) || !Number.isFinite(sum)) return [{ style: VOICE, w: 1 }];
  return kept.map(([style, w]) => ({ style, w: w / sum }));
}

/// `p` within the ranges (anything missing or out of range: the default's, or the nearest; no
/// vary: 0/0).
export function checkedProfile(p) {
  return {
    mix: checkedMix(p?.mix),
    speed: clamp(finite(p?.speed, SPEED.base), SPEED.min, SPEED.max),
    pitch: clamp(finite(p?.pitch, 0), PITCH.min, PITCH.max),
    steps: clamp(Math.round(finite(p?.steps, STEPS.max)), STEPS.min, STEPS.max),
    seedSalt: finite(p?.seedSalt, 0) >>> 0,
    vary: {
      speed: clamp(finite(p?.vary?.speed, 0), 0, VARY.speed.max),
      pitch: clamp(finite(p?.vary?.pitch, 0), 0, VARY.pitch.max),
    },
  };
}

/// The profile one line is said in (spec 15 Design 5): `profile` (as `checkedProfile` gives it)
/// itself when its vary is 0/0, else its speed moved by vary.speed times a and its pitch by
/// vary.pitch times b, a and b two draws in -1..1 from `seed` (the line's: player.js seedOf of
/// its speaker and text), each into the ranges. The same line always gets the same profile.
export function lineProfile(profile, seed) {
  const vs = finite(profile?.vary?.speed, 0);
  const vp = finite(profile?.vary?.pitch, 0);
  if (vs === 0 && vp === 0) return profile;
  const a = 2 * unit(draw(seed, 1)) - 1;
  const b = 2 * unit(draw(seed, 2)) - 1;
  return {
    ...profile,
    speed: clamp(profile.speed + vs * a, SPEED.min, SPEED.max),
    pitch: clamp(profile.pitch + vp * b, PITCH.min, PITCH.max),
  };
}

/// The profile of an utterance's speaker (`{speaker, traits: {type, worktype, alignment}}`, as
/// Game.take_speech gives it): the pack's for that actor when `pack` (a Map from actor number to
/// profile) has one, else the default: the class of its object type and the pace of its buckets,
/// with the actor number choosing the presets, their blend and the pitch. Deterministic.
export function profileFor({ speaker, traits } = {}, pack = null) {
  const own = pack?.get?.(speaker);
  if (own) return checkedProfile(own);
  const n = Number.isInteger(speaker) ? speaker : 0;
  const cls = CLASSES[TYPE_CLASSES.get(traits?.type) ?? 'plain'];
  const work = WORKTYPE_BUCKETS.find((b) => b.worktypes.includes(traits?.worktype))?.speed ?? 0;
  const align = ALIGNMENT_BUCKETS.get(traits?.alignment) ?? 0;
  const sex = n % 2 !== 0 ? 'F' : 'M';
  const lead = (((Math.floor(n / 2) % 5) + 5) % 5);
  const second = (lead + 1 + (draw(n, 1) % 4)) % 5;
  const w = 0.55 + 0.35 * unit(draw(n, 2));
  return checkedProfile({
    mix: [{ style: `${sex}${lead + 1}`, w }, { style: `${sex}${second + 1}`, w: 1 - w }],
    speed: SPEED.base + cls.speed + work + align + 0.08 * unit(draw(n, 3)) - 0.04,
    pitch: cls.pitch + 2 * unit(draw(n, 4)) - 1,
    steps: STEPS.max,
    seedSalt: 0,
  });
}

/// The voice pack's formats: /1 (profiles without vary) and /2 (each profile with its vary).
export const PACK_FORMAT = 'u6.voice-pack/1';
export const PACK_FORMAT_2 = 'u6.voice-pack/2';
/// The most voices a pack holds (the engine's actor slots are 1 to 255).
export const PACK_MAX = 1024;
/// The revision of the ranges and presets a profile is read in: FNV-1a 32 of them, 8 hex digits.
/// A pack carries the one it was made in, and a page of other ranges refuses it (its numbers
/// would mean other voices).
export const RANGES_REVISION = (() => {
  let h = 0x811c9dc5;
  for (const c of JSON.stringify([SPEED, PITCH, STEPS, MAX_MIX, STYLES])) {
    h ^= c.charCodeAt(0);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, '0');
})();

const PACK_KEYS = ['format', 'model', 'revision', 'profiles'];
const PROFILE_KEYS = ['mix', 'speed', 'pitch', 'steps', 'seedSalt'];
const PROFILE_KEYS_2 = [...PROFILE_KEYS, 'vary'];
const VARY_KEYS = ['speed', 'pitch'];
const ACTOR = /^(0|[1-9][0-9]{0,4})$/;
const plain = (x) => x !== null && typeof x === 'object' && !Array.isArray(x);
const sameKeys = (x, keys) => Object.keys(x).length === keys.length && keys.every((k) => Object.hasOwn(x, k));
const within = (x, lo, hi) => typeof x === 'number' && Number.isFinite(x) && x >= lo && x <= hi;

// Why `p` is not a profile of a pack ('keys', 'shape' or 'range'); '' when it is one: exactly
// the keys of a profile (`keys`: a /1 pack's, or a /2 pack's with vary), 1 to MAX_MIX presets,
// each once, their weights above 0 and summing to 1, every number within its range, and a /2
// profile's vary exactly {speed, pitch}, each finite, 0 to VARY's.
function profileFault(p, keys) {
  if (!plain(p)) return 'shape';
  if (!Object.keys(p).every((k) => keys.includes(k))) return 'keys';
  if (!sameKeys(p, keys) || !Array.isArray(p.mix)) return 'shape';
  if (p.mix.length < 1 || p.mix.length > MAX_MIX) return 'range';
  const styles = new Set();
  let sum = 0;
  for (const e of p.mix) {
    if (!plain(e)) return 'shape';
    if (!Object.keys(e).every((k) => k === 'style' || k === 'w')) return 'keys';
    if (!sameKeys(e, ['style', 'w'])) return 'shape';
    if (!STYLES.includes(e.style) || styles.has(e.style) || !within(e.w, Number.MIN_VALUE, 1)) return 'range';
    styles.add(e.style);
    sum += e.w;
  }
  if (Math.abs(sum - 1) > 1e-6) return 'range';
  if (!within(p.speed, SPEED.min, SPEED.max) || !within(p.pitch, PITCH.min, PITCH.max)) return 'range';
  if (!Number.isInteger(p.steps) || !within(p.steps, STEPS.min, STEPS.max)) return 'range';
  if (!Number.isInteger(p.seedSalt) || !within(p.seedSalt, 0, 0xffffffff)) return 'range';
  if (keys === PROFILE_KEYS_2) {
    if (!plain(p.vary)) return 'shape';
    if (!Object.keys(p.vary).every((k) => VARY_KEYS.includes(k))) return 'keys';
    if (!sameKeys(p.vary, VARY_KEYS)) return 'shape';
    if (!within(p.vary.speed, 0, VARY.speed.max) || !within(p.vary.pitch, 0, VARY.pitch.max)) return 'range';
  }
  return '';
}

/// The voice pack `value` (its JSON, parsed) as `{ok: true, pack}`, `pack` a Map from actor
/// number to its profile (as `checkedProfile` gives it: a /1 pack's voices with vary 0/0), or
/// `{ok: false, code}`: 'format' (not a pack of these formats), 'keys' (a key its format does not
/// have), 'shape' (a key missing, a value of the wrong form, an actor that is not a number from 0
/// to 65535), 'revision' (made in other ranges), 'range' (a voice out of the ranges, or more than
/// PACK_MAX voices). The codes name the kind only, never a key or a value.
export function readPack(value) {
  if (!plain(value) || ![PACK_FORMAT, PACK_FORMAT_2].includes(value.format)) return { ok: false, code: 'format' };
  const keys = value.format === PACK_FORMAT_2 ? PROFILE_KEYS_2 : PROFILE_KEYS;
  if (!Object.keys(value).every((k) => PACK_KEYS.includes(k))) return { ok: false, code: 'keys' };
  if (!sameKeys(value, PACK_KEYS)) return { ok: false, code: 'shape' };
  if (typeof value.model !== 'string' || value.model.length > 200 || typeof value.revision !== 'string'
    || !plain(value.profiles)) return { ok: false, code: 'shape' };
  if (value.revision !== RANGES_REVISION) return { ok: false, code: 'revision' };
  const actors = Object.keys(value.profiles);
  if (actors.length > PACK_MAX) return { ok: false, code: 'range' };
  const pack = new Map();
  for (const actor of actors) {
    if (!ACTOR.test(actor) || Number(actor) > 0xffff) return { ok: false, code: 'shape' };
    const fault = profileFault(value.profiles[actor], keys);
    if (fault) return { ok: false, code: fault };
    pack.set(Number(actor), checkedProfile(value.profiles[actor]));
  }
  return { ok: true, pack };
}

/// A preset as its JSON gives it (`style_ttl` [1, 50, 256], `style_dp` [1, 8, 16]), flat.
export function presetOf(json) {
  const flat = (x) => Float32Array.from(x.data.flat(Infinity));
  const ttl = flat(json.style_ttl);
  const dp = flat(json.style_dp);
  if (ttl.length !== 50 * 256 || dp.length !== 8 * 16) throw new Error('preset');
  return { ttl, dp };
}

// The weighted sum of `arrays`' tokens (`dim` values each), each token scaled back to length 1
// (every preset token has length 1: so is the mixed one).
function blend(arrays, weights, dim) {
  const out = new Float32Array(arrays[0].length);
  const acc = new Float64Array(dim);
  for (let at = 0; at < out.length; at += dim) {
    acc.fill(0);
    for (let i = 0; i < arrays.length; i += 1) {
      for (let d = 0; d < dim; d += 1) acc[d] += weights[i] * arrays[i][at + d];
    }
    let norm = 0;
    for (let d = 0; d < dim; d += 1) norm += acc[d] * acc[d];
    norm = Math.sqrt(norm);
    for (let d = 0; d < dim; d += 1) out[at + d] = norm > 1e-12 ? acc[d] / norm : arrays[0][at + d];
  }
  return out;
}

/// The voice of `mix` (checked as `checkedMix`) from `presets` (name to `presetOf`): one preset,
/// its own arrays untouched; two or three, their weighted sum, renormalised token by token (a
/// valid new voice: spec 09 Measured facts).
export function mixStyles(presets, mix) {
  const m = checkedMix(mix);
  if (m.length === 1) return presets[m[0].style];
  const of = m.map((e) => presets[e.style]);
  const w = m.map((e) => e.w);
  return {
    ttl: blend(of.map((p) => p.ttl), w, 256),
    dp: blend(of.map((p) => p.dp), w, 16),
  };
}

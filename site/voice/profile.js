// A voice of its own for each NPC (spec 09 Design 4): the profile of who speaks, from the
// committed type-class table and the actor number, and the mixer of the model's ten presets.
// Shared by the player (site/voice/player.js), which picks a speaker's profile, and the worker
// (site/voice/worker.js), which mixes its presets.
//
// A profile is {mix: [{style, w}] (1 to 3 of the presets, the weights summing to 1), speed (0.9
// to 1.15: the model's pace, dividing its predicted duration), pitch (-2..+2 semitones), steps
// (the most denoising steps the voice takes: the page's adaptive count, capped at this),
// seedSalt (mixed into the line's noise seed: 0 keeps the seed of the speaker and the text)}.
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
// A later local voice pack (`voices.u6vp`, spec 09 Design 4) gives `profileFor` a map from actor
// number to profile; a profile of the pack is checked into the same ranges.
import { STYLES, VOICE } from './model.js';

/// The ranges of a profile.
export const SPEED = { min: 0.9, max: 1.15, base: 1.05 }; // base: the sample code's pace
export const PITCH = { min: -2, max: 2 };
export const STEPS = { min: 1, max: 16 };
export const MAX_MIX = 3;

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

// murmur3's finaliser over the actor number and a draw index: one 32-bit draw per use.
function draw(n, k) {
  let h = (Math.imul(n | 0, 0x9e3779b1) ^ Math.imul(k, 0x85ebca77) ^ 0x5bd1e995) | 0;
  h ^= h >>> 16;
  h = Math.imul(h, 0x85ebca6b);
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35);
  h ^= h >>> 16;
  return h >>> 0;
}
const unit = (h) => h / 4294967296; // [0, 1)

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

/// `p` within the ranges (anything missing or out of range: the default's, or the nearest).
export function checkedProfile(p) {
  return {
    mix: checkedMix(p?.mix),
    speed: clamp(finite(p?.speed, SPEED.base), SPEED.min, SPEED.max),
    pitch: clamp(finite(p?.pitch, 0), PITCH.min, PITCH.max),
    steps: clamp(Math.round(finite(p?.steps, STEPS.max)), STEPS.min, STEPS.max),
    seedSalt: finite(p?.seedSalt, 0) >>> 0,
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

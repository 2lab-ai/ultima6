// Supertonic-3 on this device (spec 09 Design 3): a module worker that loads the model the site
// hosts (tools/speech/fetch-models.sh) and turns text into speech with onnxruntime-web on WebGPU.
// Its text processing and inference are written from Supertone's MIT sample code (supertonic
// 1.3.1: core.py's UnicodeProcessor and Supertonic.__call__, utils.py's chunk_text; see NOTICE).
//
// Loading: every file is fetched as the parts its manifest lists, each part checked against its
// sha256 and kept in Cache Storage under its versioned URL (one cache per model revision and
// runtime version; older ones are pruned); a cached part that no longer checks out is fetched
// again. The parts are joined and the whole file checked. The runtime's wasm comes the same way
// and is handed to it as `wasmBinary`; its two modules are checked, then imported by URL. One
// thread, WebGPU only, then a one-word synthesis as a warm-up.
//
// Messages in: {type: 'load', cachedOnly} (cachedOnly: a part not cached fails the load instead
// of being downloaded), {type: 'speak', id, text, lang, seed, steps, speed}, {type: 'cancel', id},
// {type: 'lose-device'} (tests: destroys the GPU device, as a lost one). Out: {type: 'progress',
// got, total}, {type: 'stage', stage: 'verifying' | 'starting'}, {type: 'ready', ms, refetched},
// {type: 'audio', id, pcm, sampleRate, audioSec, synthMs}, {type: 'done', id} or
// {type: 'cancelled', id}, {type: 'error', code, id?}. A message out never holds text: the text is
// the user's game text, and an error is one of the codes below, never a runtime's message.
import {
  CACHE_NAME, CACHE_PREFIX, MODEL_BASE, MODEL_FILES, ORT_BASE, ORT_MODULES, ORT_WASM, REVISION,
  ORT_VERSION,
} from './model.js';

// Error codes: no WebGPU adapter; a part not in the cache (cachedOnly); a download that failed;
// a part or file whose hash is wrong; storage that refused for lack of room, or otherwise; the
// runtime or the model that did not start; the GPU device lost; text that could not be spoken.
const CODES = new Set(['nowebgpu', 'notcached', 'download', 'hash', 'quota', 'storage', 'model',
  'gpu', 'speak']);
const SILENCE_S = 0.3; // between the chunks of one text (the sample code's default)
const MAX_CHUNK = 150; // characters per chunk: a long text starts sounding after its first one

let ort = null;
let S = null; // the sessions, the config, the indexer and the voice once loaded
let device = null;
let lost = false;
const cancelled = new Set();
let queue = Promise.resolve();

const post = (m, transfer = []) => self.postMessage(m, transfer);

function coded(code) {
  return Object.assign(new Error(code), { code });
}

// The error's code, `fallback` for any error the code did not raise itself.
const codeOf = (err, fallback) => (CODES.has(err?.code) ? err.code : fallback);

// --- text (core.py UnicodeProcessor) ----------------------------------------------------------

const EMOJI = /[\u{1f600}-\u{1f64f}\u{1f300}-\u{1f5ff}\u{1f680}-\u{1f6ff}\u{1f700}-\u{1f77f}\u{1f780}-\u{1f7ff}\u{1f800}-\u{1f8ff}\u{1f900}-\u{1f9ff}\u{1fa00}-\u{1fa6f}\u{1fa70}-\u{1faff}\u{2600}-\u{26ff}\u{2700}-\u{27bf}\u{1f1e6}-\u{1f1ff}]+/gu;
const SYMBOLS = [['–', '-'], ['‑', '-'], ['—', '-'], ['¯', ' '], ['_', ' '],
  ['“', '"'], ['”', '"'], ['‘', "'"], ['’', "'"], ['´', "'"], ['`', "'"],
  ['[', ' '], [']', ' '], ['|', ' '], ['/', ' '], ['#', ' '], ['→', ' '], ['←', ' ']];
const SPECIAL = /[♥☆♡©\\]/g;
const EXPRESSIONS = [['@', ' at '], ['e.g.,', 'for example, '], ['i.e.,', 'that is, ']];
const SPACING = [[' ,', ','], [' .', '.'], [' !', '!'], [' ?', '?'], [' ;', ';'], [' :', ':'], [" '", "'"]];
const DUPLICATE_QUOTES = /(["'`])\1+/g;
const ENDING = /[.!?;:,'")\]}…。」』】〉》›»]$/u;

// The text as the model reads it, wrapped in its language's tokens; characters the model has no
// index for are left out (the sample code would pass -1).
function preprocess(text, lang) {
  let t = text.normalize('NFKD').replace(EMOJI, '');
  for (const [a, b] of SYMBOLS) t = t.split(a).join(b);
  t = t.replace(SPECIAL, '');
  for (const [a, b] of EXPRESSIONS) t = t.split(a).join(b);
  for (const [a, b] of SPACING) t = t.split(a).join(b);
  t = t.replace(DUPLICATE_QUOTES, '$1');
  t = Array.from(t).filter((c) => indexOf(c) >= 0 || /\s/u.test(c)).join('');
  t = t.replace(/\s+/gu, ' ').trim();
  if (!ENDING.test(t)) t += '.';
  return `<${lang}>${t}</${lang}>`;
}

function indexOf(c) {
  const cp = c.codePointAt(0);
  const v = cp < S.indexer.length ? S.indexer[cp] : -1;
  return typeof v === 'number' ? v : -1;
}

// utils.py chunk_text: paragraphs, then sentences (not after the usual abbreviations), packed
// into chunks of at most `max` characters.
const SENTENCE_END = /(?<!Mr\.)(?<!Mrs\.)(?<!Ms\.)(?<!Dr\.)(?<!Prof\.)(?<!Sr\.)(?<!Jr\.)(?<!Ph\.D\.)(?<!etc\.)(?<!e\.g\.)(?<!i\.e\.)(?<!vs\.)(?<!Inc\.)(?<!Ltd\.)(?<!Co\.)(?<!Corp\.)(?<!St\.)(?<!Ave\.)(?<!Blvd\.)(?<!\b[A-Z]\.)(?<=[.!?])\s+/u;

function chunks(text, max = MAX_CHUNK) {
  const out = [];
  for (const paragraph of text.trim().split(/\n\s*\n+/u).map((p) => p.trim()).filter(Boolean)) {
    let current = '';
    for (const sentence of paragraph.split(SENTENCE_END)) {
      const s = sentence.trim();
      if (!s) continue;
      if (current.length + s.length + 1 <= max) current += (current ? ' ' : '') + s;
      else {
        if (current) out.push(current);
        current = s;
      }
    }
    if (current.trim()) out.push(current.trim());
  }
  return out;
}

// --- loading ------------------------------------------------------------------------------------

async function sha256(bytes) {
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
  return [...digest].map((b) => b.toString(16).padStart(2, '0')).join('');
}

async function manifest(base) {
  let m;
  try {
    const response = await fetch(new URL('manifest.json', base), { cache: 'no-store' });
    if (!response.ok) throw coded('download');
    m = await response.json();
  } catch (err) {
    throw coded(codeOf(err, 'download'));
  }
  if (!Array.isArray(m?.files)) throw coded('hash');
  return m;
}

function entry(m, name) {
  const f = m.files.find((x) => x.name === name);
  if (!f || !Array.isArray(f.parts) || f.parts.length === 0) throw coded('hash');
  return f;
}

// The cache of this model and runtime; every other cache of ours is an older one, removed.
async function openCache() {
  try {
    for (const name of await caches.keys()) {
      if (name.startsWith(CACHE_PREFIX) && name !== CACHE_NAME) await caches.delete(name);
    }
    return await caches.open(CACHE_NAME);
  } catch (err) {
    throw coded(err?.name === 'QuotaExceededError' ? 'quota' : 'storage');
  }
}

// One part's bytes: from the cache if they still check out (else the cached copy is removed and
// counted in `stats.refetched`), from the network otherwise, checked, then cached.
async function part(cache, url, want, stats, cachedOnly) {
  const hit = await cache.match(url);
  if (hit) {
    const bytes = new Uint8Array(await hit.arrayBuffer());
    if (bytes.length === want.size && (await sha256(bytes)) === want.sha256) return bytes;
    await cache.delete(url);
    stats.refetched += 1;
  }
  if (cachedOnly) throw coded('notcached');
  let bytes;
  try {
    const response = await fetch(url);
    if (!response.ok) throw coded('download');
    bytes = new Uint8Array(await response.arrayBuffer());
  } catch (err) {
    throw coded(codeOf(err, 'download'));
  }
  if (bytes.length !== want.size || (await sha256(bytes)) !== want.sha256) throw coded('hash');
  try {
    await cache.put(url, new Response(bytes, { headers: { 'Content-Type': 'application/octet-stream' } }));
  } catch (err) {
    throw coded(err?.name === 'QuotaExceededError' ? 'quota' : 'storage');
  }
  return bytes;
}

// File `name` of manifest `m` at `base`: its parts joined and the whole checked. A whole that
// does not check out has its parts removed from the cache, so the next load fetches them.
async function file(cache, base, m, name, stats, cachedOnly, progress) {
  const f = entry(m, name);
  const parts = [];
  for (const p of f.parts) {
    parts.push(await part(cache, new URL(p.path, base).href, p, stats, cachedOnly));
    progress(p.size);
  }
  const whole = new Uint8Array(f.size);
  let at = 0;
  for (const p of parts) {
    if (at + p.length > whole.length) throw coded('hash');
    whole.set(p, at);
    at += p.length;
  }
  if (at !== f.size || (await sha256(whole)) !== f.sha256) {
    for (const p of f.parts) await cache.delete(new URL(p.path, base).href);
    throw coded('hash');
  }
  return whole;
}

// The runtime's modules: checked against the manifest, then imported (from the same URLs).
async function runtime(m) {
  for (const name of ORT_MODULES) {
    const f = entry(m, name);
    let bytes;
    try {
      const response = await fetch(new URL(f.parts[0].path, ORT_BASE));
      if (!response.ok) throw coded('download');
      bytes = new Uint8Array(await response.arrayBuffer());
    } catch (err) {
      throw coded(codeOf(err, 'download'));
    }
    if ((await sha256(bytes)) !== f.sha256) throw coded('hash');
  }
  try {
    return await import(new URL(entry(m, ORT_MODULES[0]).parts[0].path, ORT_BASE).href);
  } catch {
    throw coded('model');
  }
}

function style(json) {
  const flat = (x) => Float32Array.from(x.data.flat(Infinity));
  const ttl = flat(json.style_ttl);
  const dp = flat(json.style_dp);
  if (ttl.length !== 50 * 256 || dp.length !== 8 * 16) throw coded('model');
  return { ttl, dp };
}

async function load(cachedOnly) {
  const t0 = performance.now();
  if (!self.navigator?.gpu) throw coded('nowebgpu');
  let adapter = null;
  try {
    adapter = await navigator.gpu.requestAdapter();
  } catch {
    adapter = null;
  }
  if (!adapter) throw coded('nowebgpu');
  const [om, mm] = [await manifest(ORT_BASE), await manifest(MODEL_BASE)];
  if (mm.revision !== REVISION || om.revision !== ORT_VERSION) throw coded('hash');
  const cache = await openCache();
  const stats = { refetched: 0 };
  const total = entry(om, ORT_WASM).size + MODEL_FILES.reduce((n, name) => n + entry(mm, name).size, 0);
  let got = 0;
  const progress = (n) => {
    got += n;
    post({ type: 'progress', got, total });
  };
  post({ type: 'progress', got, total });
  const wasm = await file(cache, ORT_BASE, om, ORT_WASM, stats, cachedOnly, progress);
  const bytes = {};
  for (const name of MODEL_FILES) bytes[name] = await file(cache, MODEL_BASE, mm, name, stats, cachedOnly, progress);
  post({ type: 'stage', stage: 'starting' });
  ort = await runtime(om);
  ort.env.logLevel = 'error';
  ort.env.wasm.numThreads = 1;
  ort.env.wasm.wasmPaths = { mjs: new URL(entry(om, ORT_MODULES[1]).parts[0].path, ORT_BASE).href };
  ort.env.wasm.wasmBinary = wasm;
  const json = (name) => JSON.parse(new TextDecoder().decode(bytes[name]));
  let cfg;
  let indexer;
  let voice;
  try {
    cfg = json('onnx/tts.json');
    indexer = json('onnx/unicode_indexer.json');
    voice = style(json(MODEL_FILES[2]));
  } catch {
    throw coded('model');
  }
  const options = { executionProviders: ['webgpu'], graphOptimizationLevel: 'all' };
  const sessions = {};
  try {
    for (const [key, name] of [['dp', 'onnx/duration_predictor.onnx'], ['te', 'onnx/text_encoder.onnx'],
      ['ve', 'onnx/vector_estimator.onnx'], ['voc', 'onnx/vocoder.onnx']]) {
      sessions[key] = await ort.InferenceSession.create(bytes[name], options);
      bytes[name] = null;
    }
  } catch {
    throw coded(lost ? 'gpu' : 'model');
  }
  device = ort.env.webgpu?.device ?? null;
  device?.lost?.then(() => {
    lost = true;
    post({ type: 'error', code: 'gpu' });
  });
  S = {
    ...sessions, indexer, voice, sampleRate: cfg.ae.sample_rate,
    chunk: cfg.ae.base_chunk_size * cfg.ttl.chunk_compress_factor,
    latentDim: cfg.ttl.latent_dim * cfg.ttl.chunk_compress_factor,
  };
  await synthesize('Hello.', 'en', 1, 16, 1.05, null);
  post({ type: 'ready', ms: performance.now() - t0, refetched: stats.refetched });
}

// --- synthesis (core.py Supertonic.__call__) ----------------------------------------------------

// mulberry32: the noise of one text is a function of its seed, so a line repeated sounds the same.
function random(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function gaussian(n, rand) {
  const out = new Float32Array(n);
  for (let i = 0; i < n; i += 1) {
    const u = Math.max(1e-4, rand());
    out[i] = Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * rand());
  }
  return out;
}

const tensor = (type, data, dims) => new ort.Tensor(type, data, dims);
const first = (result, session) => result[session.outputNames[0]];

// One chunk to samples; null once `id` is cancelled (checked between the denoising steps).
async function chunk(text, lang, rand, steps, speed, id) {
  const p = preprocess(text, lang);
  const ids = BigInt64Array.from(Array.from(p), (c) => BigInt(indexOf(c)));
  const n = ids.length;
  const textIds = tensor('int64', ids, [1, n]);
  const textMask = tensor('float32', new Float32Array(n).fill(1), [1, 1, n]);
  const styleTtl = tensor('float32', S.voice.ttl, [1, 50, 256]);
  const dp = await S.dp.run({ text_ids: textIds, style_dp: tensor('float32', S.voice.dp, [1, 8, 16]), text_mask: textMask });
  const seconds = first(dp, S.dp).data[0] / speed;
  const te = await S.te.run({ text_ids: textIds, style_ttl: styleTtl, text_mask: textMask });
  const textEmb = first(te, S.te);
  const wavLen = Math.floor(seconds * S.sampleRate);
  const latentLen = Math.max(1, Math.trunc((seconds * S.sampleRate + S.chunk - 1) / S.chunk));
  const valid = Math.floor((wavLen + S.chunk - 1) / S.chunk);
  const mask = new Float32Array(latentLen);
  mask.fill(1, 0, Math.min(valid, latentLen));
  const noise = gaussian(S.latentDim * latentLen, rand);
  for (let d = 0; d < S.latentDim; d += 1) {
    for (let k = valid; k < latentLen; k += 1) noise[d * latentLen + k] = 0;
  }
  let xt = tensor('float32', noise, [1, S.latentDim, latentLen]);
  const latentMask = tensor('float32', mask, [1, 1, latentLen]);
  const total = tensor('float32', new Float32Array([steps]), [1]);
  for (let step = 0; step < steps; step += 1) {
    if (id !== null && cancelled.has(id)) return null;
    const r = await S.ve.run({
      noisy_latent: xt, text_emb: textEmb, style_ttl: styleTtl, text_mask: textMask,
      latent_mask: latentMask, current_step: tensor('float32', new Float32Array([step]), [1]),
      total_step: total,
    });
    xt = first(r, S.ve);
  }
  if (id !== null && cancelled.has(id)) return null;
  const v = await S.voc.run({ latent: xt });
  return new Float32Array(first(v, S.voc).data.subarray(0, Math.max(1, wavLen)));
}

// `text` chunk by chunk; each chunk's samples are posted as soon as they are made (`id` null: the
// warm-up, posted nowhere). Returns false once cancelled.
async function synthesize(text, lang, seed, steps, speed, id) {
  const rand = random(seed);
  const parts = chunks(text);
  for (let i = 0; i < parts.length; i += 1) {
    const t = performance.now();
    const pcm = await chunk(parts[i], lang, rand, steps, speed, id);
    if (pcm === null) return false;
    if (id === null) continue;
    const gap = i < parts.length - 1 ? SILENCE_S : 0;
    post({ type: 'audio', id, pcm, sampleRate: S.sampleRate, audioSec: pcm.length / S.sampleRate, gap,
      synthMs: performance.now() - t }, [pcm.buffer]);
  }
  return true;
}

async function speak(m) {
  if (!S || lost) {
    post({ type: 'error', code: lost ? 'gpu' : 'speak', id: m.id });
    return;
  }
  try {
    const done = await synthesize(String(m.text), m.lang === 'ko' ? 'ko' : 'en', m.seed >>> 0,
      m.steps, m.speed, m.id);
    post({ type: done ? 'done' : 'cancelled', id: m.id });
  } catch {
    post({ type: 'error', code: lost ? 'gpu' : 'speak', id: m.id });
  } finally {
    cancelled.delete(m.id);
  }
}

self.onmessage = (e) => {
  const m = e.data;
  if (m?.type === 'cancel') {
    cancelled.add(m.id);
  } else if (m?.type === 'lose-device') {
    device?.destroy();
  } else if (m?.type === 'load') {
    queue = queue.then(() => load(Boolean(m.cachedOnly))).catch((err) => {
      post({ type: 'error', code: lost ? 'gpu' : codeOf(err, 'model') });
    });
  } else if (m?.type === 'speak') {
    queue = queue.then(() => speak(m));
  }
};

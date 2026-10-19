// The voice model the page runs (spec 09 Design 2 and 3): the Supertonic-3 revision and the
// onnxruntime-web version tools/speech/fetch-models.sh hosts under site/models/, the files the
// worker loads from them, the revision's ten voices, and the Cache Storage the worker keeps them
// in. Shared by the page (site/voice/player.js, site/voice/profile.js, site/app.js) and the
// worker (site/voice/worker.js).

export const REVISION = '724fb5abbf5502583fb520898d45929e62f02c0b';
export const ORT_VERSION = '1.30.0';
/// The model's directory and the runtime's, relative to this file.
export const MODEL_BASE = new URL(`../models/supertonic3/${REVISION}/`, import.meta.url);
export const ORT_BASE = new URL(`../models/onnxruntime-web/${ORT_VERSION}/`, import.meta.url);
/// The revision's ten voice presets (voice_styles/), all loaded: each NPC speaks in a mix of
/// them (site/voice/profile.js). The default voice, the one its model card starts with, is the
/// warm-up's and that of a profile with no valid mix.
export const STYLES = ['F1', 'F2', 'F3', 'F4', 'F5', 'M1', 'M2', 'M3', 'M4', 'M5'];
export const VOICE = 'M1';
export const styleFile = (name) => `voice_styles/${name}.json`;
/// The model files the worker loads (the manifest's names): the model, then the presets; and
/// the runtime's wasm (its two modules are imported by URL).
export const MODEL_FILES = [
  'onnx/tts.json', 'onnx/unicode_indexer.json',
  'onnx/duration_predictor.onnx', 'onnx/text_encoder.onnx', 'onnx/vector_estimator.onnx',
  'onnx/vocoder.onnx', ...STYLES.map(styleFile),
];
export const ORT_WASM = 'ort-wasm-simd-threaded.asyncify.wasm';
export const ORT_MODULES = ['ort.webgpu.min.mjs', 'ort-wasm-simd-threaded.asyncify.mjs'];
/// Cache Storage: one cache per model revision and runtime version (the parts' URLs carry both
/// too); a cache of this prefix with another name is an older one, pruned.
export const CACHE_PREFIX = 'u6-voice-';
export const CACHE_NAME = `${CACHE_PREFIX}${REVISION.slice(0, 12)}-ort-${ORT_VERSION}`;
/// The model's licence and its notice (the copy served as text), as hosted beside the parts.
export const LICENSE_URL = new URL('LICENSE.txt', MODEL_BASE);
export const NOTICE_URL = new URL('NOTICE.txt', MODEL_BASE);

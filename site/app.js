// Static shell: loads the wasm engine, feeds it dropped files, keys and time, and paints its
// RGBA frames. No game bytes are fetched or uploaded. Dropping files runs the intake
// (zip/HFE/ADF -> game files) and boots the engine; the game files are kept in this browser's
// IndexedDB so a reload starts the game again without a new drop. A large zip (the archive.org
// one: flux streams and scans around three disk images) is not read whole: the engine names the
// parts to read, its directory and its disk images, and the page reads only those (Blob.slice);
// the input limit counts the bytes read of it, not its size. The engine runs the
// original launcher's sequence: `u` (the opening, the main menu, character creation), then
// `game`; when it ends (Quit in the menu) the page says so and offers Restart.
//
// Everything that touches files, the engine or storage (engine load, the restore at page load,
// each import, each forget) is one operation on a single queue: an operation starts only after
// the previous one finished, from its file reads through the boot to the storage commit. Each
// works on its own Game and puts it on screen only once it started; the one it replaces is
// freed then. Keys reach the engine only while the canvas has focus and a game runs; the mouse
// on the canvas is the game's pointer (a press there also focuses it).
//
// Saves: a Save in the game (Ctrl-S, Y), like the character `u` creates, is stored after the
// frame it committed in, as one operation on the same queue (one IndexedDB transaction); every
// start lays the stored save over the game files, so a reload resumes it. Only the game that
// owns the stored save (the one on screen, once it started) stores its Saves, and none is taken
// while an import runs: a replaced game's Save never lands over the save its replacement
// started from. An imported savegame folder becomes the stored save once the game started
// from it. After the stored save is removed, the next Save of the game on screen is stored
// whole (objlist and every block). Sound: the first click or key on the canvas (or the Mute
// button) starts an AudioContext; after each frame the page renders the game's
// audio in wasm a little ahead and posts it to the AudioWorklet (site/audio-worklet.js).
// A press on the canvas is first the game's pointer event, then the gesture that starts the
// sound: the game queues it whatever the AudioContext does, and only once.
//
// Time (spec 06): the engine runs only in fixed steps of one PAL frame (`step_ms`, 20 ms).
// Each display frame the page asks the game how many steps the wall time since the last one
// is worth at the viewer's speed (`pace`), runs them while the frame's budget lasts, reports
// them (`ran`) and shows the result once, if they changed it (`present`, `frame_version`).
// The speed (1x .. 8x, Max) is the viewer's, kept in localStorage; - and = / + on the canvas,
// the keypad - and +, and the Speed buttons change it.
//
// HD (spec 07 HD0): with the HD button on (the default, kept in localStorage) the canvas holds
// the game's HD frame, 320·S × 200·S device pixels (S = 4 when its box holds at least three
// device pixels per game pixel, else 2) in the same CSS box: the pixels the game drew from a
// tile, a portrait or a panel picture are MMPX-magnified, computed in this browser from the
// user's own files; text and everything drawn directly are the classic pixels scaled. Off, the
// canvas is the 320 × 200 frame as before. The pointer maps onto the game's 320 × 200 pixels
// either way. The canvas is scaled into its box pixelated, or smoothly when the HD frame has
// more device pixels than the box.
//
// An HD pack (spec 07 H2: a zip the owner's local tool made from the owner's own files): a dropped
// `.zip` is first classified from its zip directory and its manifest.json alone (the engine names
// the parts to read); a pack never starts or touches a game. The rule: a zip is a pack if and only
// if its root manifest.json reads (within the reader's limit, 64 KiB unpacked, and the probe's for
// its range; its CRC-32 right) and parses as a JSON object whose root `format` is u6-hdpack/1. Any
// other zip goes to the import as before H2, which takes only the disk images in it. A pack whose
// directory already breaks the format is refused before it is read; else it is read whole, checked
// and decoded by the engine (refused whole, with nothing changed, when it does not read), set on
// the game on screen and on every game started after, and kept in IndexedDB so the next visit
// loads it before its first HD frame (storage that refuses it: the pack works for this visit only,
// and the one kept before is removed). "Forget the HD pack" removes it from the page and the
// browser. Its images go in front of MMPX for the images whose classic bytes they were made from.
// A drop that holds a pack and other files is refused as a whole. Loading a pack while another is
// set holds both decoded until the new one is checked (so a refusal changes nothing).
//
// The chooser (first run, and Settings): the page's language (English or 한글: the page's own
// labels, buttons and status lines, from site/i18n.js; the game's text stays English) and HD:
// MMPX, or one of the packs the site lists for the owner. Those are fetched only with the key
// the owner's link carries once (`?k=`, kept in localStorage and taken off the address bar),
// sent as X-Pack-Key on every /packs/ request; the site's Pages Function
// (functions/_middleware.js) answers 404 to any other, so a visitor without the key gets MMPX
// only. A listed pack is downloaded and then loaded and kept exactly as a dropped one. The
// chooser shows the pack active (none, a dropped one, or the listed one it came from), and OK
// makes the entry checked the active one. The choice made is kept in localStorage; while there
// is none the chooser shows first.
//
// Voices (spec 09 V3, site/voice/): off until the visitor presses Voices. The first time, the
// model's use restrictions show (with this browser's room for the site), and only their
// acceptance downloads it; nothing is requested from /models/ before. The worker
// (site/voice/worker.js) runs the model on WebGPU where the browser offers an adapter, else on
// the CPU (slower); the ready line and the chip say which. Once it has the model checked and
// warmed up, the game's speech tap is on (Game.set_voices) and after each frame what NPCs said
// (Game.take_speech) goes to the player (site/voice/player.js) and nowhere else: the
// text is the user's game text, never logged, shown or stored. Each NPC speaks in a voice of its
// own (site/voice/profile.js: from its actor number and the engine's names for its kind, never
// shown either; data-voice-distinct counts the voices heard), or the personality voice a dropped
// voice pack gives its actor (`voices.u6vp`, which the owner's local tool tools/speech/profile.mjs
// makes from the NPC bundles: named so, or a .json dropped alone told by its format; checked
// whole, kept in IndexedDB, shown as a count, never sent; "Forget voices" removes it too). The speech plays through a voice
// gain beside the music's (Mute silences both); Enter, Escape and Space on the canvas, and a newer
// turn, cancel it; above 4x nothing is spoken. While voices are on a chip says the voice is
// generated on this device. The choice and the acceptance are kept in localStorage: the next
// visit starts the voices from this browser's cache only (never downloads unasked). "Forget
// voices" removes the cached model and the acceptance. A failure (download, a part that does not
// check out twice, no room, a runtime that does not start on the CPU, a lost GPU device) turns
// voices off with a line that names only its kind; the game runs on.
//
// NPC bundles (spec 09 Design 5): the "NPC bundles" button reads the game files kept in this
// browser (no game needs to run) and exports, in wasm (NpcBundles), each conversation script's
// NPC name and dialogue as one u6.npc-bundle/1 object; they are kept in IndexedDB (one record per
// script under npc-bundle:) and downloaded as one JSON array, read back from that storage.
// Nothing is uploaded, and the bundles are never logged or shown: the status line gives their
// count. Storage that refuses them still gets the download, with a note. "Forget NPC bundles"
// removes them from this browser.
import init, {
  Game, HdPack, NpcBundles, PackProbe, budget_ms, input_too_large, max_input_bytes,
  max_pack_bytes, max_save_bytes, max_save_file_bytes, step_ms, zip_sliced,
} from './pkg/u6_web.js';
import {
  collect, forgetBundles, forgetFiles, forgetPack, forgetSaves, forgetVoicePack, loadBundles,
  loadFiles, loadPack, loadSaves, loadVoicePack, saveFiles, storeBundles, storePack, storeSaves,
  storeVoicePack,
} from './store.js';
import { TEXT } from './i18n.js';
import { CACHE_PREFIX, REVISION } from './voice/model.js';
import { createVoices } from './voice/player.js';
import { PACK_FORMAT, readPack } from './voice/profile.js';
import { RESTRICTIONS } from './voice/terms.js';

const canvas = document.getElementById('screen');
const drop = document.getElementById('drop');
const picker = document.getElementById('picker');
const forget = document.getElementById('forget');
const forgetSave = document.getElementById('forget-save');
const forgetHd = document.getElementById('forget-hd');
const mute = document.getElementById('mute');
const hdButton = document.getElementById('hd');
const restart = document.getElementById('restart');
const statusLine = document.getElementById('status');
const badge = document.getElementById('speed-badge');
const slower = document.getElementById('slower');
const faster = document.getElementById('faster');
const speedLabel = document.getElementById('speed-label');
const speedReached = document.getElementById('speed-reached');
const behindMark = document.getElementById('behind');
const settings = document.getElementById('settings');
const setup = document.getElementById('setup');
const setupNote = document.getElementById('setup-note');
const setupHd = document.getElementById('setup-hd');
const setupDropped = document.getElementById('setup-dropped');
const setupOk = document.getElementById('setup-ok');
const setupStatus = document.getElementById('setup-status');
const voicesButton = document.getElementById('voices');
const voiceChip = document.getElementById('voice-chip');
const voiceOnGpu = document.getElementById('voice-gpu');
const voiceOnCpu = document.getElementById('voice-cpu');
const voiceTerms = document.getElementById('voice-terms');
const voiceStorage = document.getElementById('voice-storage');
const voiceAccept = document.getElementById('voice-accept');
const voiceDecline = document.getElementById('voice-decline');
const forgetVoices = document.getElementById('forget-voices');
const npcBundles = document.getElementById('npc-bundles');
const forgetNpcBundles = document.getElementById('forget-npc-bundles');

const FOCUS_AFTER_RELOAD = 'u6-focus-after-reload';

let wasm = null; // the wasm exports, once loaded
let game = null; // the running game, or null
let crashed = false; // the engine trapped: nothing calls into it again until a reload

// Public state for scripts and tests: data-ops = queued operations, data-state = what the page
// does, data-saved = the last save generation stored, data-audio / data-audio-chunks = the
// AudioContext state and the chunks the worklet received; on the canvas data-files = game
// files of the running game, data-saves = save files it booted with, data-leader = x,y,z,
// data-prompt = 1 while the game waits for a command key, data-program = u / game / ended,
// data-name = the party leader's name, data-keys = keys not yet taken by the game,
// data-speed = the chosen speed (1, 1.5, .. 8, max), data-behind = 1 while steps are dropped;
// on the body data-gain = the music's gain now (0 muted or above 4x, 1), data-hd-pack = the
// images of the HD pack loaded (tiles,portraits,panels) or none, data-hd-pack-bytes = the bytes
// its pictures take decoded, data-hd-matched = those the game on screen matches (empty without
// a game); data-voice = the voices' state (off, terms, checking, loading, ready, failed),
// data-voice-error = the last failure's code, data-voice-gain = the voice gain, data-voice-path =
// where the model runs once ready (gpu, cpu), data-voice-pack = the voices of the voice pack
// loaded (0 without one), and the player's counts and timings (reflectVoice), never what is
// said.
function reflect(state) {
  if (state) document.body.dataset.state = state;
  const live = game && !crashed;
  canvas.dataset.files = String(live ? game.game_files() : 0);
  canvas.dataset.saves = String(live ? game.save_files() : 0);
}

// --- the page's language ---------------------------------------------------------------------

const SETUP_KEY = 'u6.setup'; // localStorage: the chooser's choice made, {"lang":"en"|"ko"}

// The stored choice, or null (none yet, or storage that cannot be read).
function storedSetup() {
  try {
    const value = JSON.parse(localStorage.getItem(SETUP_KEY));
    return value && typeof value === 'object' && Object.hasOwn(TEXT, value.lang) ? value : null;
  } catch {
    return null;
  }
}

let lang = storedSetup()?.lang ?? 'en';

// The page's text `key` in its language (a function of `args` where the line shows values).
function t(key, ...args) {
  const text = TEXT[lang][key] ?? TEXT.en[key];
  return typeof text === 'function' ? text(...args) : text;
}

// The status line: `text`, also on the chooser's line while a pack it chose loads (`mirror`).
// `say` writes one key's line and remembers it, so a change of language rewrites it.
let mirror = false;
let said = null;
function setStatus(text) {
  said = null;
  statusLine.textContent = text;
  if (mirror) setupStatus.textContent = text;
}

function say(key, ...args) {
  setStatus(t(key, ...args));
  said = [key, args];
}

// Puts the page's text in `next` language: the labels and buttons index.html names by key, the
// buttons whose text is a state, the last line `say` wrote.
function setLang(next) {
  lang = Object.hasOwn(TEXT, next) ? next : 'en';
  document.documentElement.lang = lang;
  for (const el of document.querySelectorAll('[data-i18n]')) el.textContent = t(el.dataset.i18n);
  for (const el of document.querySelectorAll('[data-i18n-title]')) el.title = t(el.dataset.i18nTitle);
  for (const el of document.querySelectorAll('[data-i18n-label]')) el.setAttribute('aria-label', t(el.dataset.i18nLabel));
  setupNote.hidden = lang === 'en';
  mute.textContent = t(audio.muted ? 'unmute' : 'mute');
  showSpeed();
  showStorage();
  if (said) say(said[0], ...said[1]);
}

function message(err) {
  return err?.message ?? String(err);
}

// The low word of the clock in seconds, as the original seeds its RNG from time().
function clockSeed() {
  return Math.floor(Date.now() / 1000) & 0xffff;
}

// --- the operation queue -------------------------------------------------------------------

let tail = Promise.resolve();
let queued = 0;

// Runs `op` after every operation queued before it; a failure is shown (`what`: the key of its
// name) and ends only that one.
function enqueue(what, op) {
  queued += 1;
  document.body.dataset.ops = String(queued);
  const run = async () => {
    try {
      await op();
    } catch (err) {
      console.error(err);
      setStatus(t('failed', t(what), message(err)));
    } finally {
      queued -= 1;
      document.body.dataset.ops = String(queued);
      reflect();
    }
  };
  tail = tail.then(run, run);
  return tail;
}

// --- sound ---------------------------------------------------------------------------------

const AHEAD_S = 0.15; // audio rendered ahead of the worklet
const MAX_RENDER_S = 0.25; // at most per frame (a tab back from the background)
const LOUD_UP_TO = 4; // above this speed (and at Max) the music fades out
const FADE_S = 0.05 / 3; // the fade's time constant: ~95% of the way in 50 ms
const audio = {
  ctx: null, node: null, gain: null, voiceGain: null, ready: false, muted: false,
  queued: 0, at: 0, posted: 0, received: 0, // the worklet's last report, frames posted since
};

function reflectAudio() {
  document.body.dataset.audio = audio.ctx ? audio.ctx.state : 'off';
  document.body.dataset.audioChunks = String(audio.received);
}

// The audio machine of game `g` at the context's rate (each new game gets its own).
function attachAudio(g) {
  if (audio.ready && g && !crashed) g.audio_start(audio.ctx.sampleRate);
}

// From a user gesture: the AudioContext (created once, resumed after that) and its worklet.
async function startAudio() {
  if (audio.ctx) {
    if (audio.ctx.state === 'suspended') await audio.ctx.resume();
    reflectAudio();
    return;
  }
  const ctx = new AudioContext();
  audio.ctx = ctx;
  ctx.onstatechange = reflectAudio;
  reflectAudio();
  await ctx.audioWorklet.addModule('./audio-worklet.js');
  const node = new AudioWorkletNode(ctx, 'u6-audio', {
    numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2],
  });
  const gain = ctx.createGain();
  gain.gain.value = loudness();
  node.connect(gain).connect(ctx.destination);
  // The voices' gain, beside the music's: 0 muted, else 1 (written directly: it is never
  // scheduled; each utterance fades on a gain of its own).
  const voiceGain = ctx.createGain();
  voiceGain.gain.value = audio.muted ? 0 : 1;
  voiceGain.connect(ctx.destination);
  audio.voiceGain = voiceGain;
  voice.attach(ctx, voiceGain);
  reflectVoice();
  node.port.onmessage = (e) => {
    audio.queued = e.data.queued;
    audio.received = e.data.received;
    audio.at = ctx.currentTime;
    audio.posted = 0;
    reflectAudio();
  };
  Object.assign(audio, { node, gain, ready: true });
  attachAudio(game);
  if (ctx.state === 'suspended') await ctx.resume();
  reflectAudio();
}

function gesture() {
  startAudio().catch((err) => console.warn(`sound is off: ${message(err)}`));
}

// Once per frame: keep the worklet AHEAD_S ahead (its last report less what it played since,
// plus what was posted after the report).
function feedAudio() {
  if (!audio.ready || audio.ctx.state !== 'running') return;
  const rate = audio.ctx.sampleRate;
  const held = Math.max(0, audio.queued - (audio.ctx.currentTime - audio.at) * rate) + audio.posted;
  const want = Math.min(Math.ceil(AHEAD_S * rate - held), Math.ceil(MAX_RENDER_S * rate));
  if (want <= 0) return;
  const n = game.audio_render(want);
  if (n === 0) return;
  // Copy before the next call into wasm, which may grow (detach) its memory.
  const chunk = new Float32Array(wasm.memory.buffer, game.audio_ptr(), n).slice();
  audio.node.port.postMessage(chunk, [chunk.buffer]);
  audio.posted += want;
}

// The music's gain: 0 muted (at any speed) or above LOUD_UP_TO, else 1. The music is rendered
// in wall time whatever the speed.
function loudness() {
  const speed = SPEEDS[speedAt];
  return audio.muted || speed === 0 || speed > LOUD_UP_TO ? 0 : 1;
}

// Sets the gain to `loudness()`: muting at once, any other change over ~50 ms. Only through
// the automation timeline: a value written directly does not take while a fade is scheduled.
function applyGain() {
  if (!audio.gain) return;
  const param = audio.gain.gain;
  const now = audio.ctx.currentTime;
  param.cancelScheduledValues(now);
  if (audio.muted) param.setValueAtTime(0, now);
  else param.setTargetAtTime(loudness(), now, FADE_S);
}

// Mute sets both gains: the music's (applyGain) and the voices'.
function setMuted(muted) {
  audio.muted = muted;
  applyGain();
  if (audio.voiceGain) audio.voiceGain.gain.value = muted ? 0 : 1;
  mute.textContent = t(muted ? 'unmute' : 'mute');
  mute.setAttribute('aria-pressed', String(muted));
  reflectVoice();
}

// --- voices (spec 09 V3) -----------------------------------------------------------------------

const VOICES_KEY = 'u6.voices'; // localStorage: 'on' while the visitor has voices on
const TERMS_KEY = 'u6.voiceTerms'; // localStorage: the model revision whose terms were accepted
// The keys that cancel the speech (the more prompt's, and those that answer).
const VOICE_CANCEL_KEYS = new Set(['Enter', 'Escape', ' ']);
const VOICE_ERRORS = {
  notcached: 'voiceErrNotCached', download: 'voiceErrDownload', hash: 'voiceErrHash',
  quota: 'voiceErrQuota', storage: 'voiceErrStorage', model: 'voiceErrModel', wasm: 'voiceErrWasm',
  gpu: 'voiceErrGpu',
};

let voiceState = 'off';
let voiceError = '';
let voiceQuiet = false; // a start from the cache at page load: no progress lines
let voiceFree = null; // this browser's room for the site, once asked (bytes, -1 unknown)

const stored = (key) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};
const remember = (key, value) => {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // No storage: the choice lasts until the page is left.
  }
};
const voicesWanted = () => stored(VOICES_KEY) === 'on';
const termsAccepted = () => stored(TERMS_KEY) === REVISION;

const voice = createVoices({
  workerUrl: new URL('./voice/worker.js', import.meta.url),
  speed: () => SPEEDS[speedAt],
  onState: voiceChanged,
  onStats: reflectVoice,
});

// The voices' state, buttons, chip and counts on the page (data-voice*: states, counts and
// timings only).
function reflectVoice() {
  const on = ['checking', 'loading', 'ready'].includes(voiceState);
  const st = voice.stats;
  const data = document.body.dataset;
  put(data, 'voice', voiceState);
  put(data, 'voiceError', voiceError);
  put(data, 'voiceGain', audio.voiceGain ? audio.voiceGain.gain.value.toFixed(2) : '');
  put(data, 'voicePath', voiceState === 'ready' ? st.path : '');
  put(data, 'voicePack', String(voice.packVoices));
  for (const [key, value] of [['voiceColdMs', st.coldMs], ['voiceRefetched', st.refetched],
    ['voiceSpoken', st.spoken], ['voiceFirstMs', st.firstMs], ['voiceCancels', st.cancels],
    ['voiceFadeMs', st.fadeMs], ['voicePlaying', st.playing], ['voiceSteps', st.steps],
    ['voiceDropped', st.dropped], ['voiceDistinct', st.distinct]]) put(data, key, String(value));
  voicesButton.setAttribute('aria-pressed', String(on));
  put(voiceChip, 'hidden', !on);
  put(voiceOnGpu, 'hidden', !(voiceState === 'ready' && st.path === 'gpu'));
  put(voiceOnCpu, 'hidden', !(voiceState === 'ready' && st.path === 'cpu'));
}

// The player's state changed: loading (its progress on the status line, unless quiet), ready (the
// game's speech tap on), failed (off, with the kind of failure).
function voiceChanged(state, code = '') {
  if (state === 'loading') {
    voiceState = 'loading';
    const st = voice.stats;
    if (!voiceQuiet && st.stage === 'download') say('voiceDownload', st.got, st.total);
    else if (!voiceQuiet && st.stage === 'starting') say('voiceStarting');
  } else if (state === 'ready') {
    voiceState = 'ready';
    voiceError = '';
    if (game && !crashed) game.set_voices(true);
    if (!voiceQuiet) say('voiceReady', voice.stats.path);
    voiceQuiet = false;
  } else if (state === 'failed') {
    voiceOff();
    voiceState = 'failed';
    voiceError = Object.hasOwn(VOICE_ERRORS, code) ? code : 'model';
    say('voiceFailed', t(VOICE_ERRORS[voiceError]));
  }
  reflectVoice();
}

// Voices off: the worker stopped, the speech tap off, the choice kept.
function voiceOff() {
  voice.stop();
  if (game && !crashed) game.set_voices(false);
  remember(VOICES_KEY, 'off');
  voiceState = 'off';
  voiceQuiet = false;
}

// This browser's room for the site, on the terms (unknown: -1).
function showStorage() {
  if (voiceFree !== null) voiceStorage.textContent = t('voiceStorage', voiceFree);
}

// The Voices button: off when on; else the terms the first time, or the model straight away.
function toggleVoices() {
  if (['checking', 'loading', 'ready'].includes(voiceState)) {
    voiceOff();
    say('voiceOff');
    reflectVoice();
  } else if (!termsAccepted()) {
    showTerms();
  } else {
    startVoices();
  }
}

// The model's use restrictions, before its first download, with this browser's room for the
// site. Nothing is downloaded until they are accepted.
function showTerms() {
  voiceTerms.hidden = false;
  document.getElementById('voice-restrictions').textContent = RESTRICTIONS;
  voiceState = 'terms';
  voiceError = '';
  reflectVoice();
  voiceFree = -1;
  showStorage();
  navigator.storage?.estimate?.().then(({ quota, usage }) => {
    voiceFree = Number.isFinite(quota) && Number.isFinite(usage) ? Math.max(0, quota - usage) : -1;
    showStorage();
  }).catch(() => {});
  voiceAccept.focus();
}

function acceptTerms() {
  remember(TERMS_KEY, REVISION);
  voiceTerms.hidden = true;
  startVoices();
}

function declineTerms() {
  voiceTerms.hidden = true;
  voiceState = 'off';
  reflectVoice();
}

// Voices on: the storage asked to persist (a download, not a start from the cache), then the
// worker loads the model, on WebGPU or, without it, on the CPU.
async function startVoices({ cachedOnly = false } = {}) {
  voiceQuiet = cachedOnly;
  voiceState = 'checking';
  voiceError = '';
  remember(VOICES_KEY, 'on');
  reflectVoice();
  if (!cachedOnly) {
    say('voiceChecking');
    try {
      await navigator.storage?.persist?.();
    } catch {
      // Not persisted: the browser may evict the model under pressure; it is fetched again.
    }
  }
  if (voiceState !== 'checking') return;
  voice.load({ cachedOnly });
}

// After a frame: what NPCs said goes to the player, and only there.
function hearSpeech() {
  const said = game.take_speech();
  if (!said) return;
  try {
    voice.say(said);
  } catch {
    // Never shown: the error could carry the text.
  }
}

// "Forget voices": the voices off, the cached model, the acceptance and the voice pack removed
// (the pack off the player at once, out of storage as an operation of the queue, after any drop
// queued before). The model's caches and the pack are removed apart: one that cannot be removed
// leaves the other removed, and the line says that not everything was.
async function forgetVoiceModel() {
  voiceOff();
  remember(VOICES_KEY, null);
  remember(TERMS_KEY, null);
  voiceTerms.hidden = true;
  attachVoicePack(null);
  reflectVoice();
  let modelGone = true;
  try {
    for (const name of await caches.keys()) {
      if (!name.startsWith(CACHE_PREFIX)) continue;
      try {
        await caches.delete(name);
      } catch (err) {
        console.warn(`could not remove a voice model cache: ${errorKind(err)}`);
        modelGone = false;
      }
    }
  } catch (err) {
    console.warn(`could not list the voice model caches: ${errorKind(err)}`);
    modelGone = false;
  }
  let packGone = true;
  await enqueue('opForgetVoices', async () => {
    attachVoicePack(null);
    try {
      await forgetVoicePack();
    } catch (err) {
      console.warn(`could not remove the voice pack: ${errorKind(err)}`);
      packGone = false;
    }
  });
  say(modelGone && packGone ? 'voiceForgot' : 'voiceForgetFailed');
}

// --- the voice pack (spec 09 Design 4) ---------------------------------------------------------

const VOICE_PACK_MAX = 1 << 20; // the most bytes a voice pack file holds
const VOICE_PACK_NAME = /\.u6vp$/i;
const JSON_SPACE = new Set([0x20, 0x09, 0x0a, 0x0d]);
const VOICE_PACK_ERRORS = {
  large: 'voicePackErrLarge', json: 'voicePackErrJson', format: 'voicePackErrFormat',
  keys: 'voicePackErrKeys', shape: 'voicePackErrShape', revision: 'voicePackErrRevision',
  range: 'voicePackErrRange',
};

// An error's kind only (its name): a message could quote what was read.
function errorKind(err) {
  return err?.name ?? 'Error';
}

// The pack's profiles on the player (null: none): the voices kept for the session forgotten.
function attachVoicePack(pack) {
  voice.setPack(pack);
  reflectVoice();
}

// Whether the dropped `item` is a voice pack (`alone`: the only file dropped): any file named
// *.u6vp (its format is checked when it is loaded: one of another is refused), or a *.json file
// dropped alone, of at most VOICE_PACK_MAX bytes and outside a savegame folder, that starts as
// JSON (its first byte that is not JSON whitespace is `{`) and parses to an object whose
// `format` is the pack's. `{pack, value}`: `value` is the object, when it was read. Only such a
// file is read here, so a drop the import refuses unread (over its limit, a save file over the
// save limit) is still not read.
async function probeVoicePack(item, alone) {
  if (VOICE_PACK_NAME.test(item.path)) return { pack: true };
  if (!alone || !/\.json$/i.test(item.path) || SAVE_PATH.test(item.path) || item.file.size > VOICE_PACK_MAX) {
    return { pack: false };
  }
  try {
    const bytes = new Uint8Array(await item.file.arrayBuffer());
    const first = bytes.findIndex((b) => !JSON_SPACE.has(b));
    if (first < 0 || bytes[first] !== 0x7b) return { pack: false };
    const value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
    return value?.format === PACK_FORMAT ? { pack: true, value } : { pack: false };
  } catch {
    return { pack: false }; // not one (or not readable: the import reads it and says what failed)
  }
}

// A dropped voice pack (`value`: its object when the probe read it): read (at most
// VOICE_PACK_MAX bytes), checked whole (profile.js readPack), set on the player and kept in this
// browser. One that does not check out is refused with a line that names only the kind, and
// nothing changes. When the browser does not keep it, the pack works for this visit only and the
// one kept before is removed (else the next visit would bring it back unsaid).
async function loadVoicePackFile(file, value) {
  const refuse = (code) => say('voicePackRefused', t(VOICE_PACK_ERRORS[code]));
  if (value === undefined) {
    if (file.size > VOICE_PACK_MAX) {
      refuse('large');
      return;
    }
    const bytes = new Uint8Array(await file.arrayBuffer());
    try {
      value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
    } catch {
      refuse('json');
      return;
    }
  }
  const read = readPack(value);
  if (!read.ok) {
    refuse(read.code);
    return;
  }
  attachVoicePack(read.pack);
  try {
    await storeVoicePack(value);
    say('voicePack', read.pack.size);
  } catch (err) {
    console.warn(`could not keep the voice pack: ${errorKind(err)}`);
    let old = t('voicePackOldGone');
    try {
      await forgetVoicePack();
    } catch (failed) {
      console.warn(`could not remove the voice pack kept before: ${errorKind(failed)}`);
      old = t('voicePackOldStays');
    }
    say('voicePackNotKept', read.pack.size, old);
  }
}

// The voice pack kept from the last visit, on the player before the first game speaks. One that
// no longer checks out (another version's ranges) is left where it is and not used.
async function restoreVoicePack() {
  const value = await loadVoicePack();
  if (value === null) return;
  const read = readPack(value);
  if (read.ok) attachVoicePack(read.pack);
  else console.warn(`the kept voice pack is not used: ${read.code}`);
}

// --- saves ---------------------------------------------------------------------------------

// The game whose Saves are stored: the one that started from the stored save (`boot`) or was
// on screen when it was removed (`forgetSaved`), or null once the stored save is not its any
// more. `pause` > 0 while an import runs, which may replace it; a Save made meanwhile is kept
// only when the import left this game on screen: it is taken at the first frame after, as a list
// covers every Save since the last one stored.
let owner = null;
let pause = 0;
let saveSeen = 0; // the owner's highest save generation already queued (or, at a forget, made)
let forgets = 0; // removals of the stored save so far: a save job taken before one writes nothing

// After a frame's steps: a Save that committed in them is stored as one operation; the game
// hears of the commit (its next list starts after it) only once the transaction completed. The
// operation writes nothing when its game no longer owns the stored save by then (it was
// replaced) or the stored save was removed after it was taken (it lists only the blocks its
// Saves replaced).
function checkSave() {
  if (pause > 0 || game !== owner) return;
  const gen = game.save_generation();
  if (gen <= saveSeen) return;
  saveSeen = gen;
  const g = game;
  const era = forgets;
  const files = [];
  const n = g.save_collect();
  for (let i = 0; i < n; i += 1) {
    const path = g.save_path(i);
    const ptr = g.save_ptr(i);
    const len = g.save_len(i);
    files.push({ path, bytes: new Uint8Array(wasm.memory.buffer, ptr, len).slice() });
  }
  enqueue('opSave', async () => {
    if (g !== owner || era !== forgets) return;
    await storeSaves(files);
    if (g === game && !crashed) g.save_committed(gen);
    document.body.dataset.saved = String(gen);
    say('saved', files.length);
  });
}

// The save `next` starts from, laid over its game files before the start: `imported` (a
// dropped savegame folder) or else the stored one.
async function attachSaves(next, imported = null) {
  const saves = imported ?? await loadSaves();
  for (const { path, bytes } of saves) next.load_save(path, bytes);
}

// After the game started from `imported`: the folder becomes the stored save. When the browser
// does not keep it, the game's own Saves are not stored either (they hold only the blocks they
// replaced and would land over the old save). Returns the status note.
async function adoptSaves(imported) {
  try {
    await storeSaves(imported, true);
    return '';
  } catch (err) {
    console.warn(`could not keep the imported save: ${message(err)}`);
    owner = null;
    return t('importedSaveNotKept');
  }
}

// --- speed ---------------------------------------------------------------------------------

// The speeds offered, slowest first: u6-core's pacer::PRESETS, then Max (0: as many steps as
// the frame's budget holds). 1x, the original's pace, is the default.
const SPEEDS = [1, 1.5, 2, 3, 4, 6, 8, 0];
const SPEED_KEY = 'u6.speed'; // localStorage: the chosen speed's name
const BADGE_MS = 1500; // the badge stays this long after a change (and always while not at 1x)
const BEHIND_MS = 1000; // "behind" shows while steps were dropped this recently
const REACH_MS = 500; // the speed Max reaches is measured over windows of this much wall time
// The characters that change the speed: their Game.keys_owned bit (the game takes the key
// itself while it is set) and the direction.
const SPEED_CHARS = new Map([['-', [1, -1]], ['+', [2, 1]], ['=', [4, 1]]]);

// The name of speed `i` as stored and reflected: 1, 1.5, .. 8, max.
const speedName = (i) => (SPEEDS[i] === 0 ? 'max' : String(SPEEDS[i]));

// The stored speed; an unreadable or unknown value is 1x.
function storedSpeed() {
  try {
    const name = localStorage.getItem(SPEED_KEY);
    return Math.max(0, SPEEDS.findIndex((_, i) => speedName(i) === name));
  } catch {
    return 0;
  }
}

let speedAt = storedSpeed(); // the chosen speed, an index into SPEEDS
let badgeRecent = false; // the speed changed less than BADGE_MS ago
let badgeTimer = 0;
// Engine ms per wall ms over the frames of the current window, and smoothed over the windows:
// the speed reached (shown at Max).
const reach = { steps: 0, wall: 0, rate: 0 };
// The game's dropped_ms() at the last frame, and the time of the frame it last grew in.
let dropped = 0;
let droppedAt = -Infinity;

function resetReach() {
  Object.assign(reach, { steps: 0, wall: 0, rate: 0 });
}

// A game put on screen runs at the chosen speed, has dropped nothing yet, and its clock starts
// now: the time its start took (the synchronous intake, with no frame) is not owed to it.
function paceGame(g) {
  g.set_speed(SPEEDS[speedAt]);
  last = performance.now();
  dropped = 0;
  droppedAt = -Infinity;
  resetReach();
}

// One preset faster (`by` 1) or slower (-1), within the list: the game takes it at once, the
// badge shows it, the choice is kept for the next visit, the music fades above 4x.
function changeSpeed(by) {
  speedAt = Math.min(Math.max(speedAt + by, 0), SPEEDS.length - 1);
  if (game && !crashed) game.set_speed(SPEEDS[speedAt]);
  resetReach();
  try {
    localStorage.setItem(SPEED_KEY, speedName(speedAt));
  } catch {
    // No storage: the choice lasts until the page is left.
  }
  badgeRecent = true;
  clearTimeout(badgeTimer);
  badgeTimer = setTimeout(() => {
    badgeRecent = false;
    showSpeed();
  }, BADGE_MS);
  applyGain();
  voice.speedChanged();
  showSpeed();
}

// The chosen speed's name as shown (2×, Max) and at Max the speed reached once measured
// (` 5.3×`, shown after the name: Max 5.3×).
function speedText() {
  const speed = SPEEDS[speedAt];
  if (speed !== 0) return [`${speed}×`, ''];
  return [t('max'), reach.rate > 0 ? ` ${reach.rate.toFixed(1)}×` : ''];
}

// Sets `el`'s `key` (a property such as hidden or textContent) only when it changes: the page
// writes these every frame.
function put(el, key, value) {
  if (el[key] !== value) el[key] = value;
}

// The label announces a change of the chosen speed (a live region); the speed Max reaches,
// which moves every REACH_MS, is beside it and not announced.
function showSpeed() {
  const [name, reached] = speedText();
  put(speedLabel, 'textContent', name);
  put(speedReached, 'textContent', reached);
  put(badge, 'textContent', name + reached);
  put(canvas.dataset, 'speed', speedName(speedAt));
  put(badge, 'hidden', SPEEDS[speedAt] === 1 && !badgeRecent);
}

// A speed key (spec 06 §Design 6): 1 faster, -1 slower, 0 none. The keypad's + and - by `code`,
// always; otherwise the character (`key`, so every layout works): = and + faster, - slower,
// each unless the game takes that key now (a panel cycling members, a line being typed, u's
// menu), when it is the game's. With Ctrl no key is a speed key.
function speedKey(e) {
  if (e.ctrlKey) return 0;
  if (e.code === 'NumpadAdd') return 1;
  if (e.code === 'NumpadSubtract') return -1;
  const char = SPEED_CHARS.get(e.key);
  if (!char || (game.keys_owned() & char[0]) !== 0) return 0;
  return char[1];
}

// After a frame's `k` steps over `dt` wall ms, at frame time `now`: the speed reached, the
// "behind" marker (the game's dropped_ms grew in the last BEHIND_MS; time cut from a stall,
// clamped_ms, is not behind), the gain the music has now.
function paced(now, dt, k) {
  if (dt > 0 && dt <= LONG_FRAME_MS) {
    reach.steps += k;
    reach.wall += dt;
  }
  if (reach.wall >= REACH_MS) {
    const rate = (reach.steps * stepMs) / reach.wall;
    reach.rate = reach.rate > 0 ? (reach.rate + rate) / 2 : rate;
    reach.steps = 0;
    reach.wall = 0;
  }
  const d = game.dropped_ms();
  if (d > dropped) droppedAt = now;
  dropped = d;
  const behind = now - droppedAt < BEHIND_MS;
  put(canvas.dataset, 'behind', behind ? '1' : '0');
  put(behindMark, 'hidden', !behind);
  showSpeed();
  if (audio.gain) put(document.body.dataset, 'gain', audio.gain.gain.value.toFixed(2));
}

// --- engine and frames ---------------------------------------------------------------------

// The Pacer's CAP_MS: a longer gap between frames is a stall (a hidden tab), not the display's
// interval.
const LONG_FRAME_MS = 250;
let looping = false;
let last = 0; // the time of the previous frame
let interval = 1000 / 60; // the display's frame interval, smoothed over the frames
let stepMs = 20; // step_ms(), once the engine loaded

let drawnGame = null; // the game whose frame the canvas shows
let drawnVersion = -1; // its frame_version() then
let drawnHd = false; // that frame was its HD frame

// Draws the game's frame on the canvas (its HD frame with HD on, else its RGBA frame), unless
// the canvas already shows that frame of that game (spec 06 O1: `present` converts only a
// changed frame, and so the page draws only a changed one; a start or an HD switch resizes,
// and so clears, the canvas, and its first frame is drawn whole). Over the HD frame it drew
// last, only the rect the compositions since then wrote is drawn.
function draw() {
  const version = game.frame_version();
  if (game === drawnGame && version === drawnVersion) return;
  const hd = game.hd_on();
  const s = hd ? game.hd_scale() : 1;
  const [w, h] = [game.width() * s, game.height() * s];
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
    drawnGame = null; // the resize cleared it
    fitRendering(hd);
  }
  const over = hd && drawnHd && game === drawnGame;
  drawnGame = game;
  drawnVersion = version;
  drawnHd = hd;
  const ctx = canvas.getContext('2d');
  // Re-create the view every frame: wasm memory growth detaches old buffers.
  if (!hd) {
    const pixels = new Uint8ClampedArray(wasm.memory.buffer, game.frame_ptr(), game.frame_len());
    ctx.putImageData(new ImageData(pixels, w, h), 0, 0);
    return;
  }
  const rect = game.hd_take_dirty();
  if (over && rect.length === 0) return;
  const pixels = new Uint8ClampedArray(wasm.memory.buffer, game.hd_frame_ptr(), game.hd_frame_len());
  const image = new ImageData(pixels, w, h);
  if (over) ctx.putImageData(image, 0, 0, rect[0], rect[1], rect[2], rect[3]);
  else ctx.putImageData(image, 0, 0);
}

// --- HD ------------------------------------------------------------------------------------

const HD_KEY = 'u6.hd'; // localStorage: 'off' when the viewer switched HD off

// The stored choice; unreadable storage or any other value is on.
function storedHd() {
  try {
    return localStorage.getItem(HD_KEY) !== 'off';
  } catch {
    return true;
  }
}

let hdOn = storedHd();

// The HD scale for the canvas's box now: 4 when it holds at least 3 device pixels per game
// pixel each way, else 2 (MMPX doubles; there is no 3x).
function hdScale() {
  const box = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  return box.width * dpr >= 3 * 320 && box.height * dpr >= 3 * 200 ? 4 : 2;
}

function showHd() {
  hdButton.setAttribute('aria-pressed', String(hdOn));
}

// How the browser scales the canvas into its box: pixelated (each canvas pixel a sharp block)
// by the stylesheet, but smooth when the HD frame has more device pixels than the box (S 4 in
// a box of 3 to 4 device pixels per game pixel): pixelated would drop one HD row and column in
// four there.
function fitRendering(hd) {
  const box = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const down = canvas.width > box.width * dpr || canvas.height > box.height * dpr;
  const want = hd && down ? 'auto' : '';
  if (canvas.style.imageRendering !== want) canvas.style.imageRendering = want;
}

// The game on screen shows HD as chosen, at the scale for the box now, drawn at once with no
// new game frame (the canvas takes the size of the frame it shows: `draw`).
function applyHd() {
  if (!game || crashed) return;
  game.set_hd(hdOn, hdScale());
  draw();
  fitRendering(game.hd_on());
}

// --- the HD pack -------------------------------------------------------------------------------

let hdPack = null; // the HdPack loaded, set on every game put on screen; null without one

// Where the pack loaded came from: '' (none), DROPPED (a drop, or a pack kept from before the
// chooser) or the name of the listed pack it was downloaded as. The kept pack's source is kept
// beside it (SOURCE_KEY) for the next visit.
const DROPPED = ':dropped';
const SOURCE_KEY = 'u6.hdSource';
const PACK_NAME = /^\w[\w.-]*$/; // a listed pack's file: a plain name
let packSource = '';

// The source of the pack kept in this browser: DROPPED unless a listed name is kept with it.
function keptSource() {
  try {
    const source = localStorage.getItem(SOURCE_KEY);
    return source !== null && PACK_NAME.test(source) ? source : DROPPED;
  } catch {
    return DROPPED;
  }
}

function keepSource(source) {
  try {
    if (source) localStorage.setItem(SOURCE_KEY, source);
    else localStorage.removeItem(SOURCE_KEY);
  } catch {
    // No storage: a pack kept without its source comes back as a dropped one.
  }
}

// The pack's counts and the game's matches on the body (data-hd-pack, data-hd-matched).
function reflectPack() {
  document.body.dataset.hdPack = hdPack ? hdPack.counts().join(',') : 'none';
  document.body.dataset.hdPackBytes = String(hdPack ? hdPack.decoded_bytes() : 0);
  document.body.dataset.hdMatched = hdPack && game && !crashed ? game.hd_pack_matches().join(',') : '';
}

// Whether `file` is an HD pack, and if so why it is refused already ('' when nothing in its
// directory or manifest breaks the format): the engine names the parts to read (the end of the
// file, its zip directory, at most 192 KiB of its manifest.json) and tells from them.
async function probePack(file) {
  const probe = new PackProbe(file.size);
  try {
    for (let part = probe.need(); part.length > 0; part = probe.need()) {
      probe.give(new Uint8Array(await file.slice(part[0], part[1]).arrayBuffer()));
    }
    return { pack: probe.is_pack(), refusal: probe.refusal() };
  } finally {
    probe.free();
  }
}

// The status line of a loaded pack: counts only.
function packStatus() {
  const [tiles, portraits, panels] = hdPack.counts();
  if (!game || crashed) return t('packLoaded', tiles, portraits, panels);
  const [matchedTiles, matchedPortraits, matchedPanels] = game.hd_pack_matches();
  return t('packMatched', matchedTiles, matchedPortraits, matchedPanels, tiles, portraits, panels);
}

// `next` (an HdPack, from `source`) replaces the pack loaded, on the game on screen at once
// (drawn with no step), and on the chooser if it is open.
function attachPack(next, source = '') {
  if (game && !crashed) {
    if (next) game.set_hd_pack(next);
    else game.forget_hd_pack();
    draw();
  }
  hdPack?.free();
  hdPack = next;
  packSource = next ? source : '';
  reflectPack();
  if (!setup.hidden) showHdChoice();
}

// A dropped pack (`refusal`: why its directory or manifest already refuses it, '' if nothing
// does): read whole, checked and decoded by the engine, set on the game on screen, kept in this
// browser. A pack that does not read is refused and nothing changes; one too large or refused by
// its directory is refused before it is read whole. When the browser does not keep the new pack,
// the one kept before is removed too (else the next visit would bring it back unsaid). `source`:
// DROPPED, or the listed name it was downloaded as. Returns whether the pack was loaded.
async function loadPackFile(file, refusal, source = DROPPED) {
  needEngine();
  if (file.size > max_pack_bytes()) {
    say('packTooLarge', Math.floor(max_pack_bytes() / 2 ** 20));
    return false;
  }
  if (refusal) {
    say('packRefused', refusal);
    return false;
  }
  say('packReading');
  let bytes = new Uint8Array(await file.arrayBuffer());
  say('packChecking');
  await new Promise((resolve) => setTimeout(resolve, 0));
  let next;
  try {
    next = HdPack.load(bytes);
  } catch (err) {
    say('packRefused', message(err));
    return false;
  }
  attachPack(next, source);
  const kept = await storePack(bytes);
  bytes = null;
  let note = t('packKept');
  if (kept) keepSource(source);
  if (!kept) {
    let old = t('packOldGone');
    try {
      await forgetPack();
      keepSource('');
    } catch (err) {
      console.warn(`could not remove the HD pack kept before: ${message(err)}`);
      old = t('packOldStays');
    }
    note = t('packNotKept', old);
  }
  setStatus(`${packStatus()}${note}`);
  return true;
}

// The pack kept from the last visit, loaded before the first game is put on screen (so its first
// HD frame has it). One that no longer reads is left where it is and not used.
async function restorePack() {
  const bytes = await loadPack();
  if (!bytes) return;
  try {
    attachPack(HdPack.load(bytes), keptSource());
  } catch (err) {
    console.warn(`the kept HD pack is not used: ${message(err)}`);
  }
}

// "Forget the HD pack": off the page at once (MMPX again), then out of the browser's storage.
async function forgetHdPack() {
  attachPack(null);
  try {
    await forgetPack();
  } catch (err) {
    say('packForgetFailed', message(err));
    return;
  }
  keepSource('');
  say('packForgot');
}

function toggleHd() {
  hdOn = !hdOn;
  try {
    localStorage.setItem(HD_KEY, hdOn ? 'on' : 'off');
  } catch {
    // No storage: the choice lasts until the page is left.
  }
  showHd();
  applyHd();
}

// One display frame: the steps its wall time is worth at the chosen speed (`pace`; the first
// frame's time can precede `last`, which counts 0), run while the frame's budget lasts (half the
// display's interval, at least 3 ms; the first always) and until the sequence ends, then
// reported (`ran`). If any ran, the frame is converted and drawn once, when they changed it;
// the sound, a Save to store and the reflected state once per frame.
function frame(now) {
  if (!game || crashed) {
    looping = false;
    return;
  }
  const dt = now - last;
  last = now;
  if (dt > 0 && dt <= LONG_FRAME_MS) interval += (dt - interval) / 8;
  try {
    const due = game.pace(dt);
    const budget = budget_ms(interval);
    const start = performance.now();
    let k = 0;
    let done = false;
    while (k < due && !done && (k === 0 || performance.now() - start < budget)) {
      game.step(stepMs);
      k += 1;
      done = game.finished();
    }
    game.ran(k);
    const log = game.take_log();
    if (log) console.info(log);
    if (voice.ready) hearSpeech();
    if (k > 0) {
      game.present();
      draw();
    }
    feedAudio();
    checkSave();
    canvas.dataset.leader = game.leader_position().join(',');
    canvas.dataset.prompt = game.at_prompt() ? '1' : '0';
    canvas.dataset.program = game.program();
    canvas.dataset.name = game.leader_name();
    canvas.dataset.keys = String(game.keys_pending());
    paced(now, dt, k);
    if (done || game.finished()) {
      ended();
      return;
    }
  } catch (err) {
    stopOnTrap(err);
    return;
  }
  requestAnimationFrame(frame);
}

// The launcher's sequence ended (Quit in the menu): nothing runs any more; Restart starts it
// again from the kept files.
function ended() {
  looping = false;
  say('ended');
  restart.hidden = false;
  reflect('ended');
}

function startLoop() {
  if (looping) return;
  looping = true;
  last = performance.now();
  requestAnimationFrame(frame);
}

// A trap or exception inside the engine: its state is unknown, so it is never stepped again.
function stopOnTrap(err) {
  console.error(err);
  say('trap', message(err));
  crashed = true;
  looping = false;
  restart.hidden = false;
  reflect('crashed');
}

// Boots `next` (its files loaded; `origin` the key of the line that says where from); on success
// it replaces the running game. Returns the intake report, or null when it did not start (the
// status says why; the caller frees `next`).
async function boot(next, origin) {
  say(origin, next.files_loaded());
  // Let the status paint before the (synchronous) intake runs.
  await new Promise((resolve) => setTimeout(resolve, 0));
  let report;
  try {
    report = next.start(clockSeed());
  } catch (err) {
    say('cannotStart', message(err));
    return null;
  }
  const old = game;
  game = next;
  owner = next;
  next.set_voices(voice.ready);
  voice.newGame();
  old?.free();
  drawnGame = null; // the freed game's frame is not this one's
  drawnVersion = -1;
  saveSeen = 0;
  paceGame(next);
  try {
    attachAudio(next);
  } catch (err) {
    // No audio machine for this game (its MED tables fail the engine's checks): it runs silent.
    console.warn(`sound is off: ${message(err)}`);
  }
  if (hdPack) next.set_hd_pack(hdPack);
  applyHd();
  reflectPack();
  restart.hidden = true;
  reflect('running');
  startLoop();
  return report;
}

// Copies the files `started` booted from out of wasm and stores them; true once committed.
async function keep(started) {
  let files = [];
  try {
    for (let i = 0; i < started.kept_count(); i += 1) {
      const path = started.kept_path(i);
      const ptr = started.kept_ptr(i);
      const len = started.kept_len(i);
      // Copy before the next call into wasm, which may grow (detach) its memory.
      files.push({ path, bytes: new Uint8Array(wasm.memory.buffer, ptr, len).slice() });
    }
    started.release_kept();
    return await saveFiles(files);
  } finally {
    files = null;
  }
}

// --- operations (each runs on the queue) ---------------------------------------------------

function needEngine() {
  if (!wasm) throw new Error(t('engineNotLoaded'));
  if (crashed) throw new Error(t('engineStopped'));
}

// Files of an original `savegame/` folder (the PLAYER disk's saved game).
const SAVE_PATH = /(^|\/)savegame\/[^/]+$/i;

// The save files of a drop, read, in the original's names; a file over the save limits (the
// engine's, which its start also applies to stored files) is left out before it is read.
// Returns the files and the status note on what was left out.
async function readSaves(saveItems) {
  const files = [];
  const left = [];
  let total = 0;
  for (const { path, file } of saveItems) {
    if (file.size > max_save_file_bytes() || total + file.size > max_save_bytes()) {
      left.push(path);
      continue;
    }
    say('reading', path);
    total += file.size;
    files.push({ path: `savegame/${path.split('/').pop()}`, bytes: new Uint8Array(await file.arrayBuffer()) });
  }
  const note = left.length === 0 ? ''
    : t('savesLeftOut', max_save_file_bytes(), max_save_bytes(), left.join(', '));
  return { files, note };
}

// A zip the engine reads in slices (`zip_sliced`): it names each part to read (the end of the
// file, the directory if it starts before that, then each disk-image entry) and checks it, and
// the page reads just that part. The engine collects the zip as one file of its disk images; a
// part that does not check out, or one that would pass the input limit, fails the import before
// it is read.
async function readZipSlices(next, path, file) {
  next.zip_open(path, file.size);
  for (let part = next.zip_need(); part.length > 0; part = next.zip_need()) {
    next.zip_give(new Uint8Array(await file.slice(part[0], part[1]).arrayBuffer()));
  }
}

// An import: `items` ({ path, file }) read one at a time into a new Game, booted, kept. The
// files of a `savegame/` folder among them are the save it starts from, and become the stored
// save (replacing the old one) once it started; a drop of only such a folder resumes it with
// the kept game files. It runs with the Saves paused (`queueImport`). The input limit is
// checked before anything is read for the files read whole, and again before each of them
// with what wasm already holds (the parts read of a zip read in slices included); a zip read in
// slices is checked part by part before each is read (only those parts count).
async function importFiles(dropped) {
  needEngine();
  const total = dropped.reduce(
    (sum, { path, file }) => sum + (zip_sliced(path, file.size) ? 0 : file.size), 0);
  if (dropped.length === 0) {
    say('noFiles');
    return;
  }
  if (total > max_input_bytes()) {
    setStatus(input_too_large(total));
    return;
  }
  const saveItems = dropped.filter(({ path }) => SAVE_PATH.test(path));
  const items = dropped.filter(({ path }) => !SAVE_PATH.test(path));
  let imported = null;
  let lead = '';
  if (saveItems.length > 0) {
    const read = await readSaves(saveItems);
    lead = read.note;
    if (read.files.length > 0) imported = read.files;
    if (items.length === 0) {
      if (imported) {
        await restoreKept(true, `${lead}${t('savesImported', imported.length)}`, imported);
      } else {
        setStatus(`${lead}${t('nothingImported')}`);
      }
      return;
    }
  }
  const next = new Game();
  let started = false;
  try {
    // The files still to be read whole, together.
    let whole = items.reduce(
      (sum, { path, file }) => sum + (zip_sliced(path, file.size) ? 0 : file.size), 0);
    for (const { path, file } of items) {
      if (zip_sliced(path, file.size)) {
        say('reading', path);
        await readZipSlices(next, path, file);
      } else {
        if (next.input_held() + whole > max_input_bytes()) {
          setStatus(input_too_large(next.input_held() + whole));
          return;
        }
        whole -= file.size;
        say('reading', path);
        // The JS copy lives only until wasm has its own (load_file copies it in).
        next.load_file(path, new Uint8Array(await file.arrayBuffer()));
      }
    }
    await attachSaves(next, imported);
    const report = await boot(next, 'bootRead');
    if (report === null) return;
    started = true;
    canvas.focus();
    const saved = imported ? await adoptSaves(imported) : '';
    const kept = (await keep(next)) ? t('filesKept', next.game_files()) : t('filesNotKept');
    const resumed = next.save_files() > 0 ? t('resumed') : '';
    setStatus(`${lead}${report}\n${t('started')}${resumed}${saved}${kept}\n${t('howToPlay')}`);
  } finally {
    if (started) {
      if (!crashed) next.release_kept();
    } else {
      next.free();
    }
  }
}

// The restore: the kept files (if any) and the saved game (`imported`, a dropped folder, or
// the stored one) into a new Game, booted. `focus` when the user asked for it (Restart, an
// imported save); the restore at page load leaves the focus where it is. `note` goes first in
// the status. It runs at page load (no game yet) or inside an import (the Saves paused).
async function restoreKept(focus, note = '', imported = null) {
  needEngine();
  const lead = note ? `${note}\n` : '';
  let kept = await loadFiles();
  if (!kept || kept.length === 0) {
    let after = '';
    if (imported) {
      // No game starts from it: it is the stored save now, and no longer the running game's.
      await storeSaves(imported, true);
      if (game) {
        owner = null;
        after = t('notSavedAnyMore');
      }
    }
    if (lead || after) setStatus(`${lead}${t('noFilesYet')}${after}`);
    else say('noFilesYet');
    return;
  }
  const next = new Game();
  let started = false;
  try {
    for (let i = 0; i < kept.length; i += 1) {
      next.load_file(kept[i].path, kept[i].bytes);
      kept[i] = null;
    }
    kept = null;
    await attachSaves(next, imported);
    const report = await boot(next, 'bootKept');
    if (report === null) return;
    started = true;
    next.release_kept();
    if (focus) canvas.focus();
    const saved = imported ? await adoptSaves(imported) : '';
    const what = t(next.save_files() > 0 ? 'keptResumed' : 'keptNew');
    setStatus(`${lead}${report}\n${t('startedKept', what)}${saved}\n${t('howToPlay')}`);
  } finally {
    if (!started) next.free();
  }
}

// --- NPC bundles (spec 09 Design 5) ------------------------------------------------------

const NPC_FILE = 'ultima6-npc-bundles.json';

// Offers `text` as the download `name` (a JSON file made in this page; nothing is sent).
function download(name, text) {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const link = Object.assign(document.createElement('a'), { href: url, download: name });
  link.hidden = true;
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

// "NPC bundles": the bundles of the kept game files, made in wasm, kept in IndexedDB in place of
// the ones kept before, and downloaded as one JSON array read back from there (from the export
// itself when the storage refuses them).
async function exportNpcBundles() {
  needEngine();
  let kept = await loadFiles();
  if (!kept || kept.length === 0) {
    say('npcNoFiles');
    return;
  }
  const exporter = new NpcBundles();
  let json;
  try {
    for (let i = 0; i < kept.length; i += 1) {
      exporter.add(kept[i].path, kept[i].bytes);
      kept[i] = null;
    }
    kept = null;
    json = exporter.export();
  } finally {
    exporter.free();
  }
  let bundles = JSON.parse(json);
  json = null;
  // A storage error names only its kind, never a bundle.
  let stored = true;
  try {
    await storeBundles(bundles);
  } catch (err) {
    console.warn(`could not keep the NPC bundles: ${message(err)}`);
    stored = false;
  }
  if (stored) {
    try {
      bundles = await loadBundles();
    } catch (err) {
      // Kept, but not read back: the download is the export itself, the same bundles.
      console.warn(`could not read the kept NPC bundles back: ${message(err)}`);
    }
  }
  download(NPC_FILE, JSON.stringify(bundles));
  say(stored ? 'npcExported' : 'npcNotKept', bundles.length, NPC_FILE);
}

async function forgetNpc() {
  await forgetBundles();
  say('npcForgot');
}

async function forgetKept() {
  await forgetFiles();
  say(game ? 'forgotFilesRunning' : 'forgotFiles');
}

// The stored save removed: a save job taken before writes nothing, and the game on screen owns
// the empty store, its next Save listing the whole game (objlist and every block), so a start
// from it is the world that game had and not new-game blocks under its own. A Save it made
// before the forget and not yet stored (made while it did not own the store, or during an
// import just before) is not stored: only its Saves after the forget are.
async function forgetSaved() {
  await forgetSaves();
  forgets += 1;
  if (game && !crashed) {
    game.save_forgotten();
    owner = game;
    saveSeen = game.save_generation();
  }
  say('forgotSave');
}

// Runs `op` (an import, which may replace the running game) with no Save taken meanwhile.
async function pausingSaves(op) {
  pause += 1;
  try {
    return await op();
  } finally {
    pause -= 1;
  }
}

// A drop: a voice pack alone or an HD pack alone is loaded as one; anything else is imported as
// game files. A drop holding a pack and other files (or two packs) is refused as a whole, nothing
// read further.
async function takeDrop(dropped) {
  needEngine();
  const voicePacks = [];
  for (const item of dropped) {
    const told = await probeVoicePack(item, dropped.length === 1);
    if (told.pack) voicePacks.push({ ...item, value: told.value });
  }
  if (voicePacks.length > 0) {
    if (dropped.length > 1) say('dropVoicePackAndFiles');
    else await loadVoicePackFile(voicePacks[0].file, voicePacks[0].value);
    return;
  }
  const packs = [];
  for (const item of dropped) {
    if (!/\.zip$/i.test(item.path)) continue;
    const told = await probePack(item.file);
    if (told.pack) packs.push({ ...item, refusal: told.refusal });
  }
  if (packs.length === 0) {
    await importFiles(dropped);
  } else if (dropped.length > 1) {
    say(packs.length > 1 ? 'dropTwoPacks' : 'dropPackAndFiles');
  } else {
    await loadPackFile(packs[0].file, packs[0].refusal);
  }
}

function queueImport(gather) {
  if (queued > 0) say('waiting');
  enqueue('opLoad', () => pausingSaves(async () => takeDrop(await gather())));
}

// --- the chooser -----------------------------------------------------------------------------

const PACK_KEY = 'u6.packKey'; // localStorage: the key the owner's link brought (`?k=`)

// The key: taken once from the address (`?k=`; empty forgets it), kept, and taken off the
// address bar (and so off the history and a copied link); else the one kept, '' without one.
// `fresh`: the address brought a key other than the one kept.
function takePackKey() {
  let kept = '';
  try {
    kept = localStorage.getItem(PACK_KEY) ?? '';
  } catch {
    // No storage: a key lasts until the page is left.
  }
  const url = new URL(location.href);
  const given = url.searchParams.get('k');
  if (given === null) return { key: kept, fresh: false };
  url.searchParams.delete('k');
  history.replaceState(history.state, '', `${url.pathname}${url.search}${url.hash}`);
  try {
    if (given) localStorage.setItem(PACK_KEY, given);
    else localStorage.removeItem(PACK_KEY);
  } catch {
    // No storage: the key lasts until the page is left.
  }
  return { key: given, fresh: given !== '' && given !== kept };
}

const packKey = takePackKey();

// /packs/`name` with the key, never cached by the browser (it is kept in IndexedDB once
// loaded); null without a key: no request is made.
function packFetch(name) {
  if (!packKey.key) return null;
  return fetch(`/packs/${encodeURIComponent(name)}`, {
    headers: { 'X-Pack-Key': packKey.key }, cache: 'no-store',
  });
}

// The packs the site lists for this visitor (/packs/index.json: {"packs": [{"file", "label"}]}),
// asked once a visit; [] when it lists none (no key, a 404, anything that does not read). A
// file is a plain name; a label is text.
let packList = null;
function listPacks() {
  packList ??= (async () => {
    try {
      const response = await packFetch('index.json');
      if (!response?.ok) return [];
      const { packs } = await response.json();
      return (Array.isArray(packs) ? packs : [])
        .filter((p) => typeof p?.file === 'string' && PACK_NAME.test(p.file))
        .map((p) => ({ file: p.file, label: typeof p.label === 'string' && p.label ? p.label : p.file }));
    } catch {
      return [];
    }
  })();
  return packList;
}

// The chooser's HD entries checked as the pack active now: MMPX for none, "Dropped pack" (shown
// only then) for a dropped one, the listed one it was downloaded as (none checked when that one
// is not listed: OK leaves the HD as it is).
function showHdChoice() {
  setupDropped.hidden = packSource !== DROPPED;
  for (const input of setupHd.querySelectorAll('input')) input.checked = input.value === packSource;
}

// The chooser's listed HD entries, after MMPX (and "Dropped pack"): each pack listed, by its label
// (as text); the active one checked unless another entry was checked meanwhile. Its data-packs
// is the count listed, once the list is in (for scripts and tests).
async function showPacks() {
  const packs = await listPacks();
  for (const row of setupHd.querySelectorAll('label[data-pack]')) row.remove();
  for (const { file, label } of packs) {
    const input = Object.assign(document.createElement('input'), { type: 'radio', name: 'setup-hd', value: file });
    const text = document.createElement('span');
    text.textContent = label;
    const row = document.createElement('label');
    row.dataset.pack = file;
    row.append(input, ' ', text);
    setupHd.append(row);
    if (file === packSource && !setupHd.querySelector('input:checked')) input.checked = true;
  }
  setupHd.dataset.packs = String(packs.length);
}

// Opens the chooser on the language and the HD choice now.
function openSetup() {
  for (const input of setup.querySelectorAll('input[name="setup-lang"]')) input.checked = input.value === lang;
  setupStatus.textContent = '';
  setup.hidden = false;
  showHdChoice();
  showPacks();
}

// Keeps the choice made (the language now) and closes the chooser.
function keepSetup() {
  try {
    localStorage.setItem(SETUP_KEY, JSON.stringify({ lang }));
  } catch {
    // No storage: the choice lasts until the page is left.
  }
  setup.hidden = true;
}

// A listed pack: downloaded with the key (its progress on the status line), then loaded and kept
// as a dropped one (`loadPackFile`). Returns whether it was loaded.
async function downloadPack(name) {
  needEngine();
  const limit = max_pack_bytes();
  const tooLarge = () => say('packTooLarge', Math.floor(limit / 2 ** 20));
  say('packDownload', 0, 0);
  const parts = [];
  try {
    const response = await packFetch(name);
    if (!response?.ok) {
      say('packNotDownloaded', `HTTP ${response?.status ?? 404}`);
      return false;
    }
    const total = Number(response.headers.get('Content-Length')) || 0;
    if (total > limit) {
      await response.body?.cancel();
      tooLarge();
      return false;
    }
    const reader = response.body.getReader();
    let got = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      got += value.length;
      if (got > limit) {
        await reader.cancel();
        tooLarge();
        return false;
      }
      parts.push(value);
      say('packDownload', got, total);
    }
  } catch (err) {
    say('packNotDownloaded', message(err));
    return false;
  }
  const file = new File([new Blob(parts)], name);
  const told = await probePack(file);
  if (!told.pack) {
    say('packNotAPack');
    return false;
  }
  return loadPackFile(file, told.refusal, name);
}

// OK: the language is kept, and the HD entry checked becomes the active pack, on the queue (so
// after a restore or a load queued before): the active pack's own entry (or none checked)
// changes nothing; MMPX removes the active pack, whatever its source; another listed pack is
// downloaded and loaded, the chooser open and saying what the status line says until it is (it
// stays open with the reason when it is not).
function applySetup() {
  const chosen = setupHd.querySelector('input:checked')?.value;
  setupOk.disabled = true;
  enqueue('opPack', async () => {
    mirror = true;
    try {
      if (chosen === undefined || chosen === packSource || chosen === DROPPED) {
        keepSetup();
      } else if (chosen === '') {
        await forgetHdPack();
        keepSetup();
      } else if (await downloadPack(chosen)) {
        keepSetup();
      }
    } catch (err) {
      console.error(err);
      setStatus(t('failed', t('opPack'), message(err)));
    } finally {
      mirror = false;
      setupOk.disabled = false;
    }
  });
}

// --- wiring --------------------------------------------------------------------------------

// Keys go to the engine only from the focused canvas with a game running; everything else
// (Tab, keys of other elements, modifier shortcuts) keeps its browser meaning. Ctrl + the
// letters of the game's control commands (Ctrl-S save, Ctrl-R restore, ...) are game keys
// there, so the browser's own Save page / Reload do not fire. The speed keys (`speedKey`) are
// the page's and never reach the game.
canvas.addEventListener('keydown', (e) => {
  gesture();
  if (!game || crashed || e.metaKey || e.altKey) return;
  if (VOICE_CANCEL_KEYS.has(e.key)) voice.cancel();
  const by = speedKey(e);
  if (by !== 0) {
    e.preventDefault();
    changeSpeed(by);
  } else if (game.key_down(e.code, e.key, e.ctrlKey)) {
    e.preventDefault();
  }
});
mute.addEventListener('click', () => {
  setMuted(!audio.muted);
  gesture();
});
hdButton.addEventListener('click', toggleHd);
settings.addEventListener('click', () => {
  openSetup();
  setup.querySelector('input[name="setup-lang"]:checked')?.focus();
});
for (const input of setup.querySelectorAll('input[name="setup-lang"]')) {
  input.addEventListener('change', () => setLang(input.value));
}
setupOk.addEventListener('click', applySetup);
// A box that grows or shrinks (the window, the zoom) may want the other scale.
new ResizeObserver(() => {
  if (hdOn) applyHd();
}).observe(canvas);
window.addEventListener('resize', () => {
  if (hdOn) applyHd();
});
// A move to a display of another pixel ratio changes the device pixels of the same CSS box
// (no resize): one query per ratio, armed again for the new one.
function watchRatio() {
  matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`).addEventListener('change', () => {
    if (hdOn) applyHd();
    watchRatio();
  }, { once: true });
}
watchRatio();
slower.addEventListener('click', () => changeSpeed(-1));
faster.addEventListener('click', () => changeSpeed(1));
// The pointer: the position through the canvas's CSS box onto the game's 320 x 200 pixels
// (whatever the canvas's own size: HD makes it S times that), the buttons
// as DOM `buttons` (bit 0 left, bit 1 right). The canvas captures a press so the release reaches
// the game even off the canvas; the context menu stays off it while a game runs. A press is, in
// this order, the focus, the game's pointer event (queued for the next step, exactly once) and
// the gesture that starts the sound (the AudioContext is created before its first await, inside
// the press's user activation); the sound starting never delays or drops the event.
function sendPointer(e) {
  if (!game || crashed) return;
  const box = canvas.getBoundingClientRect();
  if (box.width === 0 || box.height === 0) return;
  const x = Math.floor(((e.clientX - box.left) * game.width()) / box.width);
  const y = Math.floor(((e.clientY - box.top) * game.height()) / box.height);
  game.pointer(x, y, e.buttons & 3);
}
canvas.addEventListener('pointerdown', (e) => {
  canvas.focus();
  if (game && !crashed) canvas.setPointerCapture(e.pointerId);
  sendPointer(e);
  gesture();
});
canvas.addEventListener('pointermove', sendPointer);
canvas.addEventListener('pointerup', sendPointer);
canvas.addEventListener('pointercancel', (e) =>
  sendPointer({ clientX: e.clientX, clientY: e.clientY, buttons: 0 }));
canvas.addEventListener('contextmenu', (e) => {
  if (game && !crashed) e.preventDefault();
});

// The whole page accepts drops so a near miss does not open the file in the tab.
window.addEventListener('dragover', (e) => {
  e.preventDefault();
  drop.classList.add('over');
});
window.addEventListener('dragleave', (e) => {
  if (e.relatedTarget === null) drop.classList.remove('over');
});
window.addEventListener('drop', (e) => {
  e.preventDefault();
  drop.classList.remove('over');
  // Entries must be taken synchronously; the DataTransfer is cleared after this handler.
  const entries = [...e.dataTransfer.items]
    .map((item) => item.webkitGetAsEntry?.())
    .filter(Boolean);
  const files = [...e.dataTransfer.files];
  queueImport(async () => {
    if (entries.length === 0) return files.map((file) => ({ path: file.name, file }));
    const items = [];
    for (const entry of entries) await collect(entry, items);
    return items;
  });
});
picker.addEventListener('change', () => {
  const items = [...picker.files].map((file) => ({ path: file.webkitRelativePath || file.name, file }));
  picker.value = '';
  queueImport(async () => items);
});
forget.addEventListener('click', () => {
  if (queued > 0) say('forgetFilesLater');
  enqueue('opForgetFiles', forgetKept);
});
forgetSave.addEventListener('click', () => {
  if (queued > 0) say('forgetSaveLater');
  enqueue('opForgetSave', forgetSaved);
});
forgetHd.addEventListener('click', () => {
  if (queued > 0) say('forgetPackLater');
  enqueue('opForgetPack', forgetHdPack);
});
voicesButton.addEventListener('click', () => {
  gesture();
  toggleVoices();
});
voiceAccept.addEventListener('click', () => {
  gesture();
  acceptTerms();
});
voiceDecline.addEventListener('click', declineTerms);
forgetVoices.addEventListener('click', () => {
  forgetVoiceModel().catch(() => say('voiceForgetFailed'));
});
npcBundles.addEventListener('click', () => {
  if (queued > 0) say('waiting');
  enqueue('opNpcBundles', exportNpcBundles);
});
forgetNpcBundles.addEventListener('click', () => {
  if (queued > 0) say('forgetNpcLater');
  enqueue('opForgetNpc', forgetNpc);
});
restart.addEventListener('click', () => {
  try {
    sessionStorage.setItem(FOCUS_AFTER_RELOAD, '1');
  } catch {
    // No session storage: the reload just does not move the focus.
  }
  location.reload();
});
window.addEventListener('unhandledrejection', (e) => {
  console.error(e.reason);
  say('unexpected', message(e.reason));
});

function takeFocusFlag() {
  try {
    const set = sessionStorage.getItem(FOCUS_AFTER_RELOAD) === '1';
    sessionStorage.removeItem(FOCUS_AFTER_RELOAD);
    return set;
  } catch {
    return false;
  }
}

reflect('loading');
reflectPack();
reflectAudio();
setLang(lang);
say('loadingEngine');
showHd();
canvas.dataset.behind = '0';
reflectVoice();
// Voices turned on in an earlier visit come back from this browser's cache only.
if (voicesWanted() && termsAccepted()) startVoices({ cachedOnly: true });
// The chooser first while no choice is kept, or when the address brought a new key.
if (!storedSetup() || packKey.fresh) openSetup();
enqueue('opEngine', async () => {
  try {
    wasm = await init();
  } catch (err) {
    reflect('failed');
    throw new Error(t('engineFailed', message(err)));
  }
  stepMs = step_ms();
  forget.disabled = false;
  forgetSave.disabled = false;
  forgetHd.disabled = false;
  npcBundles.disabled = false;
  forgetNpcBundles.disabled = false;
  reflect('idle');
});
// The picker opens once the kept files are back, so a choice never races the restore (a drop
// meanwhile is queued after it).
enqueue('opRestore', async () => {
  try {
    await restoreVoicePack();
    if (wasm) await restorePack();
    if (wasm) await restoreKept(takeFocusFlag());
  } finally {
    if (wasm) picker.disabled = false;
    if (!game) reflect(wasm ? 'idle' : 'failed');
  }
});

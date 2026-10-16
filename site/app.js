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
// either way.
import init, {
  Game, budget_ms, input_too_large, max_input_bytes, max_save_bytes, max_save_file_bytes, step_ms,
  zip_sliced,
} from './pkg/u6_web.js';
import {
  collect, forgetFiles, forgetSaves, loadFiles, loadSaves, saveFiles, storeSaves,
} from './store.js';

const canvas = document.getElementById('screen');
const drop = document.getElementById('drop');
const picker = document.getElementById('picker');
const forget = document.getElementById('forget');
const forgetSave = document.getElementById('forget-save');
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

const FOCUS_AFTER_RELOAD = 'u6-focus-after-reload';
const HOW_TO_PLAY = 'Click the game screen or Tab to it, then: Escape skips the opening; in ' +
  'the menu C creates a character, J journeys onward; in the game arrow keys walk, T talks, ' +
  'space passes. The mouse works as in the original: click the map to walk, the command icons, ' +
  'the party and the inventory.';

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
// on the body data-gain = the music's gain now (0 muted or above 4x, 1).
function reflect(state) {
  if (state) document.body.dataset.state = state;
  const live = game && !crashed;
  canvas.dataset.files = String(live ? game.game_files() : 0);
  canvas.dataset.saves = String(live ? game.save_files() : 0);
}

function setStatus(text) {
  statusLine.textContent = text;
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

// Runs `op` after every operation queued before it; a failure is shown and ends only that one.
function enqueue(what, op) {
  queued += 1;
  document.body.dataset.ops = String(queued);
  const run = async () => {
    try {
      await op();
    } catch (err) {
      console.error(err);
      setStatus(`${what} failed: ${message(err)}`);
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
  ctx: null, node: null, gain: null, ready: false, muted: false,
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

function setMuted(muted) {
  audio.muted = muted;
  applyGain();
  mute.textContent = muted ? 'Unmute' : 'Mute';
  mute.setAttribute('aria-pressed', String(muted));
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
  enqueue('Saving the game', async () => {
    if (g !== owner || era !== forgets) return;
    await storeSaves(files);
    if (g === game && !crashed) g.save_committed(gen);
    document.body.dataset.saved = String(gen);
    setStatus(`Saved in this browser (${files.length} files): the next visit resumes this game.`);
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
    return ' (The imported save could not be kept in this browser, nor will this game\'s Saves.)';
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
  showSpeed();
}

// The chosen speed's name as shown (2×, Max) and at Max the speed reached once measured
// (` 5.3×`, shown after the name: Max 5.3×).
function speedText() {
  const speed = SPEEDS[speedAt];
  if (speed !== 0) return [`${speed}×`, ''];
  return ['Max', reach.rate > 0 ? ` ${reach.rate.toFixed(1)}×` : ''];
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

// The game on screen shows HD as chosen, at the scale for the box now, drawn at once with no
// new game frame (the canvas takes the size of the frame it shows: `draw`).
function applyHd() {
  if (!game || crashed) return;
  game.set_hd(hdOn, hdScale());
  draw();
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
  setStatus('The game has ended. Restart starts it again from the files kept in this browser.');
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
  setStatus(`The game stopped on an internal error (${message(err)}). Restart loads the ` +
    'kept files again.');
  crashed = true;
  looping = false;
  restart.hidden = false;
  reflect('crashed');
}

// Boots `next` (its files loaded); on success it replaces the running game. Returns the intake
// report, or null when it did not start (the status says why; the caller frees `next`).
async function boot(next, origin) {
  const n = next.files_loaded();
  setStatus(`${n} file${n === 1 ? '' : 's'} ${origin}; decoding the disks…`);
  // Let the status paint before the (synchronous) intake runs.
  await new Promise((resolve) => setTimeout(resolve, 0));
  let report;
  try {
    report = next.start(clockSeed());
  } catch (err) {
    setStatus(`Could not start the game:\n${message(err)}`);
    return null;
  }
  const old = game;
  game = next;
  owner = next;
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
  applyHd();
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
  if (!wasm) throw new Error('the engine is not loaded');
  if (crashed) throw new Error('the engine stopped; press Restart');
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
    setStatus(`Reading ${path}…`);
    total += file.size;
    files.push({ path: `savegame/${path.split('/').pop()}`, bytes: new Uint8Array(await file.arrayBuffer()) });
  }
  const note = left.length === 0 ? '' : `Left out of the saved game (a save file holds at most ` +
    `${max_save_file_bytes()} bytes, the saved game ${max_save_bytes()} bytes together): ` +
    `${left.join(', ')}.\n`;
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
    setStatus('No files in the drop.');
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
        await restoreKept(true, `${lead}The saved game (${imported.length} files) was imported.`,
          imported);
      } else {
        setStatus(`${lead}Nothing was imported.`);
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
        setStatus(`Reading ${path}…`);
        await readZipSlices(next, path, file);
      } else {
        if (next.input_held() + whole > max_input_bytes()) {
          setStatus(input_too_large(next.input_held() + whole));
          return;
        }
        whole -= file.size;
        setStatus(`Reading ${path}…`);
        // The JS copy lives only until wasm has its own (load_file copies it in).
        next.load_file(path, new Uint8Array(await file.arrayBuffer()));
      }
    }
    await attachSaves(next, imported);
    const report = await boot(next, 'read');
    if (report === null) return;
    started = true;
    canvas.focus();
    const saved = imported ? await adoptSaves(imported) : '';
    const kept = (await keep(next))
      ? ` The ${next.game_files()} game files are kept in this browser for the next visit.`
      : ' (The files could not be kept in this browser; drop them again next time.)';
    const resumed = next.save_files() > 0 ? ' Resumed the game saved in this browser.' : '';
    setStatus(`${lead}${report}\nStarted.${resumed}${saved}${kept}\n${HOW_TO_PLAY}`);
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
        after = ' The game on screen is not saved in this browser any more.';
      }
    }
    setStatus(`${lead}No files loaded yet: drop or choose your Ultima VI files.${after}`);
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
    const report = await boot(next, 'kept from the last visit');
    if (report === null) return;
    started = true;
    next.release_kept();
    if (focus) canvas.focus();
    const saved = imported ? await adoptSaves(imported) : '';
    const what = next.save_files() > 0
      ? 'the game saved in this browser, resumed'
      : 'a new game (no game saved in this browser)';
    setStatus(`${lead}${report}\nStarted from the files kept in this browser: ${what}.${saved}\n` +
      HOW_TO_PLAY);
  } finally {
    if (!started) next.free();
  }
}

async function forgetKept() {
  await forgetFiles();
  setStatus(game
    ? 'The kept game files were removed from this browser. The game on screen runs until ' +
      'the page is left or reloaded.'
    : 'The kept game files were removed from this browser.');
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
  setStatus('The saved game was removed from this browser; the next start is a new game. The ' +
    'game on screen runs on until the page is left or reloaded; a Save in it stores the whole ' +
    'game again.');
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

function queueImport(gather) {
  if (queued > 0) setStatus('Waiting for the previous step to finish…');
  enqueue('Loading the files', () => pausingSaves(async () => importFiles(await gather())));
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
  if (queued > 0) setStatus('The kept files will be removed after the current step…');
  enqueue('Removing the kept files', forgetKept);
});
forgetSave.addEventListener('click', () => {
  if (queued > 0) setStatus('The saved game will be removed after the current step…');
  enqueue('Removing the saved game', forgetSaved);
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
  setStatus(`Unexpected error: ${message(e.reason)}`);
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
reflectAudio();
showSpeed();
showHd();
canvas.dataset.behind = '0';
enqueue('Loading the engine', async () => {
  try {
    wasm = await init();
  } catch (err) {
    reflect('failed');
    throw new Error(`the engine could not load (${message(err)})`);
  }
  stepMs = step_ms();
  forget.disabled = false;
  forgetSave.disabled = false;
  reflect('idle');
});
// The picker opens once the kept files are back, so a choice never races the restore (a drop
// meanwhile is queued after it).
enqueue('Starting the kept files', async () => {
  try {
    if (wasm) await restoreKept(takeFocusFlag());
  } finally {
    if (wasm) picker.disabled = false;
    if (!game) reflect(wasm ? 'idle' : 'failed');
  }
});

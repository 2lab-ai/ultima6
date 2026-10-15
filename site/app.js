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
// tick it committed in, as one operation on the same queue (one IndexedDB transaction); every
// start lays the stored save over the game files, so a reload resumes it. Only the game that
// owns the stored save (the one on screen, once it started) stores its Saves, and none is taken
// while an import runs: a replaced game's Save never lands over the save its replacement
// started from. An imported savegame folder becomes the stored save once the game started
// from it. After the stored save is removed, the next Save of the game on screen is stored
// whole (objlist and every block). Sound: the first click or key on the canvas (or the Mute
// button) starts an AudioContext; after each tick the page renders the game's
// audio in wasm a little ahead and posts it to the AudioWorklet (site/audio-worklet.js).
// A press on the canvas is first the game's pointer event, then the gesture that starts the
// sound: the game queues it whatever the AudioContext does, and only once.
import init, {
  Game, input_too_large, max_input_bytes, max_save_bytes, max_save_file_bytes, zip_sliced,
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
const restart = document.getElementById('restart');
const statusLine = document.getElementById('status');

const MAX_STEP_MS = 100; // a backgrounded tab must not jump the game ahead
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
// data-name = the party leader's name, data-keys = keys not yet taken by the game.
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
const MAX_RENDER_S = 0.25; // at most per tick (a tab back from the background)
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
  gain.gain.value = audio.muted ? 0 : 1;
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

// After a tick: keep the worklet AHEAD_S ahead (its last report less what it played since,
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

function setMuted(muted) {
  audio.muted = muted;
  if (audio.gain) audio.gain.gain.value = muted ? 0 : 1;
  mute.textContent = muted ? 'Unmute' : 'Mute';
  mute.setAttribute('aria-pressed', String(muted));
}

// --- saves ---------------------------------------------------------------------------------

// The game whose Saves are stored: the one that started from the stored save (`boot`) or was
// on screen when it was removed (`forgetSaved`), or null once the stored save is not its any
// more. `pause` > 0 while an import runs, which may replace it; a Save made meanwhile is kept
// only when the import left this game on screen: it is taken at the first tick after, as a list
// covers every Save since the last one stored.
let owner = null;
let pause = 0;
let saveSeen = 0; // the owner's highest save generation already queued (or, at a forget, made)
let forgets = 0; // removals of the stored save so far: a save job taken before one writes nothing

// After a tick: a Save that committed in it is stored as one operation; the game hears of the
// commit (its next list starts after it) only once the transaction completed. The operation
// writes nothing when its game no longer owns the stored save by then (it was replaced) or
// the stored save was removed after it was taken (it lists only the blocks its Saves replaced).
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

// --- engine and frames ---------------------------------------------------------------------

let looping = false;
let last = 0;
let carry = 0;

function draw() {
  // Re-create the view every frame: wasm memory growth detaches old buffers.
  const pixels = new Uint8ClampedArray(wasm.memory.buffer, game.frame_ptr(), game.frame_len());
  canvas.getContext('2d').putImageData(new ImageData(pixels, canvas.width, canvas.height), 0, 0);
}

function frame(now) {
  if (!game || crashed) {
    looping = false;
    return;
  }
  // The first rAF timestamp can precede `last`; a negative step would wrap as u32.
  carry = Math.min(Math.max(0, carry + (now - last)), MAX_STEP_MS);
  last = now;
  const ms = Math.floor(carry);
  carry -= ms;
  try {
    game.tick(ms);
    const log = game.take_log();
    if (log) console.info(log);
    draw();
    feedAudio();
    checkSave();
    canvas.dataset.leader = game.leader_position().join(',');
    canvas.dataset.prompt = game.at_prompt() ? '1' : '0';
    canvas.dataset.program = game.program();
    canvas.dataset.name = game.leader_name();
    canvas.dataset.keys = String(game.keys_pending());
    if (game.finished()) {
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
  carry = 0;
  requestAnimationFrame(frame);
}

// A trap or exception inside the engine: its state is unknown, so it is never ticked again.
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
  saveSeen = 0;
  try {
    attachAudio(next);
  } catch (err) {
    // No audio machine for this game (its MED tables fail the engine's checks): it runs silent.
    console.warn(`sound is off: ${message(err)}`);
  }
  canvas.width = next.width();
  canvas.height = next.height();
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
// checked before anything is read for the files read whole; a zip read in slices is checked
// part by part as it is read (only those parts count).
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
    for (const { path, file } of items) {
      setStatus(`Reading ${path}…`);
      if (zip_sliced(path, file.size)) {
        await readZipSlices(next, path, file);
      } else {
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
// there, so the browser's own Save page / Reload do not fire.
canvas.addEventListener('keydown', (e) => {
  gesture();
  if (!game || crashed || e.metaKey || e.altKey) return;
  if (game.key_down(e.code, e.key, e.ctrlKey)) e.preventDefault();
});
mute.addEventListener('click', () => {
  setMuted(!audio.muted);
  gesture();
});
// The pointer: the position through the canvas's CSS box onto its 320 x 200 pixels, the buttons
// as DOM `buttons` (bit 0 left, bit 1 right). The canvas captures a press so the release reaches
// the game even off the canvas; the context menu stays off it while a game runs. A press is, in
// this order, the focus, the game's pointer event (queued for the next tick, exactly once) and
// the gesture that starts the sound (the AudioContext is created before its first await, inside
// the press's user activation); the sound starting never delays or drops the event.
function sendPointer(e) {
  if (!game || crashed) return;
  const box = canvas.getBoundingClientRect();
  if (box.width === 0 || box.height === 0) return;
  const x = Math.floor(((e.clientX - box.left) * canvas.width) / box.width);
  const y = Math.floor(((e.clientY - box.top) * canvas.height) / box.height);
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
enqueue('Loading the engine', async () => {
  try {
    wasm = await init();
  } catch (err) {
    reflect('failed');
    throw new Error(`the engine could not load (${message(err)})`);
  }
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

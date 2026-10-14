// Static shell: loads the wasm engine, feeds it dropped files, keys and time, and paints its
// RGBA frames. No game bytes are fetched or uploaded. Dropping files runs the intake
// (zip/HFE/ADF -> game files) and boots the engine; the game files are kept in this browser's
// IndexedDB so a reload starts the game again without a new drop.
//
// Everything that touches files, the engine or storage (engine load, the restore at page load,
// each import, each forget) is one operation on a single queue: an operation starts only after
// the previous one finished, from its file reads through the boot to the storage commit. Each
// works on its own Game and puts it on screen only once it started; the one it replaces is
// freed then. Keys reach the engine only while the canvas has focus and a game runs.
import init, { Game, max_input_bytes } from './pkg/u6_web.js';
import { collect, forgetFiles, loadFiles, saveFiles } from './store.js';

const canvas = document.getElementById('screen');
const drop = document.getElementById('drop');
const picker = document.getElementById('picker');
const forget = document.getElementById('forget');
const restart = document.getElementById('restart');
const statusLine = document.getElementById('status');

const MAX_STEP_MS = 100; // a backgrounded tab must not jump the game ahead
const FOCUS_AFTER_RELOAD = 'u6-focus-after-reload';
const HOW_TO_PLAY = 'Click the game screen or Tab to it, then: arrow keys walk, T talks, ' +
  'space passes.';

let wasm = null; // the wasm exports, once loaded
let game = null; // the running game, or null
let crashed = false; // the engine trapped: nothing calls into it again until a reload

// Public state for scripts and tests: data-ops = queued operations, data-state = what the page
// does; data-files on the canvas = game files of the running game.
function reflect(state) {
  if (state) document.body.dataset.state = state;
  canvas.dataset.files = String(game && !crashed ? game.game_files() : 0);
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
  } catch (err) {
    stopOnTrap(err);
    return;
  }
  requestAnimationFrame(frame);
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
  old?.free();
  canvas.width = next.width();
  canvas.height = next.height();
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

// An import: `items` ({ path, file }) read one at a time into a new Game, booted, kept.
async function importFiles(items) {
  needEngine();
  const total = items.reduce((sum, { file }) => sum + file.size, 0);
  if (items.length === 0) {
    setStatus('No files in the drop.');
    return;
  }
  if (total > max_input_bytes()) {
    setStatus(`The dropped files are ${Math.floor(total / 2 ** 20)} MiB together; this page ` +
      `takes at most ${max_input_bytes() / 2 ** 20} MiB (the archive.org zip, the three disks ` +
      'or the extracted files).');
    return;
  }
  const next = new Game();
  let started = false;
  try {
    for (const { path, file } of items) {
      setStatus(`Reading ${path}…`);
      // The JS copy lives only until wasm has its own (load_file copies it in).
      next.load_file(path, new Uint8Array(await file.arrayBuffer()));
    }
    const report = await boot(next, 'read');
    if (report === null) return;
    started = true;
    canvas.focus();
    const kept = (await keep(next))
      ? ` The ${next.game_files()} game files are kept in this browser for the next visit ` +
        '(not a saved game: each visit starts a new game).'
      : ' (The files could not be kept in this browser; drop them again next time.)';
    setStatus(`${report}\nStarted.${kept}\n${HOW_TO_PLAY}`);
  } finally {
    if (started) {
      if (!crashed) next.release_kept();
    } else {
      next.free();
    }
  }
}

// The restore: the kept files (if any) into a new Game, booted. `focus` when the user asked
// for it (Restart); the restore at page load leaves the focus where it is.
async function restoreKept(focus) {
  needEngine();
  let kept = await loadFiles();
  if (!kept || kept.length === 0) {
    setStatus('No files loaded yet: drop or choose your Ultima VI files.');
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
    const report = await boot(next, 'kept from the last visit');
    if (report === null) return;
    started = true;
    next.release_kept();
    if (focus) canvas.focus();
    setStatus(`${report}\nStarted from the files kept in this browser (a new game, not a ` +
      `saved one).\n${HOW_TO_PLAY}`);
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

function queueImport(gather) {
  if (queued > 0) setStatus('Waiting for the previous step to finish…');
  enqueue('Loading the files', async () => importFiles(await gather()));
}

// --- wiring --------------------------------------------------------------------------------

// Keys go to the engine only from the focused canvas with a game running; everything else
// (Tab, keys of other elements, modifier shortcuts) keeps its browser meaning. Ctrl + the
// letters of the game's control commands (Ctrl-S save, Ctrl-R restore, ...) are game keys
// there, so the browser's own Save page / Reload do not fire.
canvas.addEventListener('keydown', (e) => {
  if (!game || crashed || e.metaKey || e.altKey) return;
  if (game.key_down(e.code, e.key, e.ctrlKey)) e.preventDefault();
});
canvas.addEventListener('pointerdown', () => canvas.focus());

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
enqueue('Loading the engine', async () => {
  try {
    wasm = await init();
  } catch (err) {
    reflect('failed');
    throw new Error(`the engine could not load (${message(err)})`);
  }
  forget.disabled = false;
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

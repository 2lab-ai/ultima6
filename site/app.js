// Static shell: loads the wasm engine, feeds it dropped files, keys and time,
// and paints its RGBA frames. No game bytes are fetched or uploaded.
import init, { Game } from './pkg/u6_web.js';

const canvas = document.getElementById('screen');
const drop = document.getElementById('drop');
const picker = document.getElementById('picker');
const statusLine = document.getElementById('status');

const MAX_STEP_MS = 100; // a backgrounded tab must not jump the game ahead

function setStatus(text) {
  statusLine.textContent = text;
}

function isFormControl(target) {
  return target instanceof HTMLElement &&
    (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT|BUTTON)$/.test(target.tagName));
}

function fileOf(entry) {
  return new Promise((resolve, reject) => entry.file(resolve, reject));
}

function readBatch(reader) {
  return new Promise((resolve, reject) => reader.readEntries(resolve, reject));
}

// Walks a dropped file or folder; paths are relative to the drop.
async function collect(entry, out) {
  if (entry.isFile) {
    out.push({ path: entry.fullPath.replace(/^\//, ''), file: await fileOf(entry) });
  } else if (entry.isDirectory) {
    const reader = entry.createReader();
    for (;;) {
      const batch = await readBatch(reader);
      if (batch.length === 0) break;
      for (const child of batch) await collect(child, out);
    }
  }
}

async function main() {
  let wasm;
  try {
    wasm = await init();
  } catch (err) {
    setStatus(`The engine failed to load: ${err}`);
    return;
  }
  const game = new Game();
  const width = game.width();
  const height = game.height();
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext('2d');

  function draw() {
    // Re-create the view every frame: wasm memory growth detaches old buffers.
    const pixels = new Uint8ClampedArray(wasm.memory.buffer, game.frame_ptr(), game.frame_len());
    ctx.putImageData(new ImageData(pixels, width, height), 0, 0);
  }

  async function load(items) {
    try {
      for (const { path, file } of items) {
        game.load_file(path, new Uint8Array(await file.arrayBuffer()));
      }
      const n = game.files_loaded();
      setStatus(`${n} file${n === 1 ? '' : 's'} loaded (in memory, this tab only).`);
    } catch (err) {
      setStatus(`Could not read a file: ${err}`);
    }
  }

  window.addEventListener('keydown', (e) => {
    if (e.ctrlKey || e.metaKey || e.altKey || isFormControl(e.target)) return;
    if (game.key_down(e.code)) e.preventDefault();
  });
  window.addEventListener('keyup', (e) => {
    if (game.key_up(e.code) && !isFormControl(e.target)) e.preventDefault();
  });
  window.addEventListener('blur', () => game.release_all());
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) game.release_all();
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
    (async () => {
      let items = [];
      if (entries.length > 0) {
        for (const entry of entries) await collect(entry, items);
      } else {
        items = files.map((file) => ({ path: file.name, file }));
      }
      await load(items);
    })().catch((err) => setStatus(`Could not read the drop: ${err}`));
  });
  picker.addEventListener('change', () => {
    load([...picker.files].map((file) => ({ path: file.webkitRelativePath || file.name, file })));
  });

  let last = performance.now();
  let carry = 0;
  function frame(now) {
    // The first rAF timestamp can precede `last`; a negative step would wrap as u32.
    carry = Math.min(Math.max(0, carry + (now - last)), MAX_STEP_MS);
    last = now;
    const ms = Math.floor(carry);
    carry -= ms;
    game.tick(ms);
    draw();
    requestAnimationFrame(frame);
  }

  draw();
  setStatus('No files loaded yet.');
  requestAnimationFrame(frame);
}

main();

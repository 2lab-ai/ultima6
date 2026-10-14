// Oracle harness core: vAmigaWeb (wasm vAmiga core + AROS ROM) in headless Chromium.
// Input = raw Amiga keycodes via wasm_key; capture = the core's native RGBA texture,
// read straight out of wasm memory (not the scaled canvas).
import { createServer } from 'node:http';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { deflateSync } from 'node:zlib';
import { extname, join, normalize, dirname } from 'node:path';
import { chromium } from 'playwright';

export const ORACLE_HOME = process.env.U6_ORACLE_HOME || '/Users/zhugehyuk/Games/ultima6/oracle';
export const VAW_DIR = process.env.U6_VAW_DIR || join(ORACLE_HOME, 'vamigaweb');
export const SHOTS_DIR = process.env.U6_SHOTS_DIR || join(ORACLE_HOME, 'shots');

// vAmiga texture geometry (js/vAmiga_canvas.js:1-4): 912 hires positions * TPP 2 texels,
// 313 lines of one PAL field. One lores pixel = 4 texels wide, 1 line tall.
export const TEX_W = 912 * 2;
export const TEX_H = 313;

// Raw Amiga keycodes (same table as vAmigaWeb js/vAmiga_keyboard.js key_translation_map).
export const KEY = {
  '`': 0x00, 1: 0x01, 2: 0x02, 3: 0x03, 4: 0x04, 5: 0x05, 6: 0x06, 7: 0x07, 8: 0x08, 9: 0x09, 0: 0x0a,
  '-': 0x0b, '=': 0x0c, '\\': 0x0d,
  q: 0x10, w: 0x11, e: 0x12, r: 0x13, t: 0x14, y: 0x15, u: 0x16, i: 0x17, o: 0x18, p: 0x19,
  '[': 0x1a, ']': 0x1b,
  a: 0x20, s: 0x21, d: 0x22, f: 0x23, g: 0x24, h: 0x25, j: 0x26, k: 0x27, l: 0x28, ';': 0x29, "'": 0x2a,
  z: 0x31, x: 0x32, c: 0x33, v: 0x34, b: 0x35, n: 0x36, m: 0x37, ',': 0x38, '.': 0x39, '/': 0x3a,
  ' ': 0x40,
  SPACE: 0x40, BACKSPACE: 0x41, TAB: 0x42, KP_ENTER: 0x43, RETURN: 0x44, ESC: 0x45, DEL: 0x46,
  UP: 0x4c, DOWN: 0x4d, RIGHT: 0x4e, LEFT: 0x4f,
  F1: 0x50, F2: 0x51, F3: 0x52, F4: 0x53, F5: 0x54, F6: 0x55, F7: 0x56, F8: 0x57, F9: 0x58, F10: 0x59,
  LSHIFT: 0x60, RSHIFT: 0x61, CAPS: 0x62, CTRL: 0x63, LALT: 0x64, RALT: 0x65, LAMIGA: 0x66, RAMIGA: 0x67,
};
// Characters that need shift on a US Amiga keymap.
const SHIFTED = { ':': ';', '"': "'", '?': '/', '!': '1', '(': '9', ')': '0', '_': '-', '+': '=' };

const MIME = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css',
  '.wasm': 'application/wasm', '.json': 'application/json', '.png': 'image/png', '.svg': 'image/svg+xml',
  '.bin': 'application/octet-stream', '.mp3': 'audio/mpeg', '.wav': 'audio/wav',
};

export function startServer(root = VAW_DIR) {
  const server = createServer(async (req, res) => {
    try {
      const url = new URL(req.url, 'http://x');
      const rel = normalize(decodeURIComponent(url.pathname)).replace(/^(\.\.[/\\])+/, '');
      const file = join(root, rel.endsWith('/') ? rel + 'index.html' : rel);
      if (!file.startsWith(root)) throw new Error('outside root');
      const body = await readFile(file);
      res.writeHead(200, { 'content-type': MIME[extname(file)] || 'application/octet-stream' });
      res.end(body);
    } catch {
      res.writeHead(404);
      res.end();
    }
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server)));
}

// Minimal RGBA PNG encoder (zlib only).
const CRC_TABLE = new Int32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c;
});
function crc32(buf) {
  let c = -1;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}
export function encodePNG(width, height, rgba) {
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0;
    Buffer.from(rgba.buffer, rgba.byteOffset + y * width * 4, width * 4).copy(raw, y * (width * 4 + 1) + 1);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw, { level: 9 })), chunk('IEND', Buffer.alloc(0)),
  ]);
}

export async function launch({ headless = true, log = () => {} } = {}) {
  const server = await startServer();
  const port = server.address().port;
  const browser = await chromium.launch({
    headless,
    args: ['--autoplay-policy=no-user-gesture-required', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
  });
  const context = await browser.newContext({ serviceWorkers: 'block', viewport: { width: 1280, height: 900 } });
  // vAmiga warns once per unmapped access; a runaway CPU emits >100k of these and pushing
  // them over CDP took the page down once. Drop them in-page, keep a count.
  await context.addInitScript(() => {
    window.__u6_unmapped = 0;
    for (const k of ['error', 'warn', 'log']) {
      const orig = console[k].bind(console);
      console[k] = (...a) => {
        if (typeof a[0] === 'string' && /^Warning: Invalid (addr|value)/.test(a[0])) { window.__u6_unmapped++; return; }
        orig(...a);
      };
    }
  });
  const page = await context.newPage();
  page.on('console', (m) => log(`[console.${m.type()}] ${m.text()}`));
  page.on('crash', () => log('[pagecrash]'));
  page.on('pageerror', (e) => log(`[pageerror] ${e.message}`));
  page.on('dialog', async (d) => { log(`[dialog] ${d.type()}: ${d.message()}`); await d.dismiss(); });
  const params = { AROS: true, navbar: false, dialog_on_missing_roms: false, dialog_on_disk: false };
  await page.goto(`http://127.0.0.1:${port}/index.html#${encodeURIComponent(JSON.stringify(params))}`);
  // Ready = both AROS ROM halves installed by fetchOpenROMS (vAmiga_ui.js:701-726), the UI
  // reports MSG_RUN (emulator_currently_runs), and the frame counter is advancing.
  await page.waitForFunction(() => {
    try {
      const r = JSON.parse(wasm_rom_info());
      return r.hasRom === 'true' && r.hasExt === 'true' && emulator_currently_runs === true;
    } catch { return false; }
  }, null, { timeout: 120000, polling: 250 });
  const f = await frame(page);
  await page.waitForFunction((f) => ((Module._wasm_frame_info() >>> 2) - f) > 25, f, { timeout: 60000, polling: 100 });
  return { server, browser, context, page };
}

export async function close(h) {
  await h.browser.close();
  h.server.close();
}

export const romInfo = (page) => page.evaluate(() => JSON.parse(wasm_rom_info()));

export const frame = (page) => page.evaluate(() => Module._wasm_frame_info() >>> 2);
export const running = (page) => page.evaluate(() => emulator_currently_runs === true);

// RetroShell (vAmiga's own console) command; returns the console text after it ran.
export async function shell(page, cmd) {
  return page.evaluate((cmd) => { wasm_retro_shell(cmd); return wasm_retro_shell_get_text(); }, cmd);
}

export const drives = (page) => page.evaluate(() => [0, 1, 2, 3].map((i) => wasm_get_config_item('DRIVE_CONNECT', i)));

// Machine options before boot, e.g. 'AGNUS_REVISION=OCS,CHIP_RAM=512,SLOW_RAM=512,FAST_RAM=0'
// (names as the UI passes them to wasm_configure; RAM/revision changes power the machine off).
export async function configure(page, spec) {
  const out = {};
  for (const kv of spec.split(',').filter(Boolean)) {
    const [k, v] = kv.split('=');
    out[k] = await page.evaluate(([k, v]) => wasm_configure(k, v), [k, v]);
  }
  return out;
}

// Make sure the core is running and the UI's rAF loop drives it (same as vAmiga_ui.js:470).
// Needed after configure(): RAM/revision changes power the machine off.
export async function ensureRunning(page) {
  const f = await frame(page);
  await page.waitForTimeout(1000);
  if ((await frame(page)) - f >= 20) return false; // PAL runs 50 frames/s
  await page.evaluate(() => { wasm_run(); running = true; });
  const g = await frame(page);
  await page.waitForFunction((g) => (Module._wasm_frame_info() >>> 2) - g >= 20, g, { timeout: 10000, polling: 100 });
  return true;
}

export async function setFloppyCount(page, n) {
  return page.evaluate((n) => wasm_configure('floppy_drive_count', String(n)), n);
}

// Insert a disk image into DFn (0-3) or, for .hdf, into HDn.
export async function insert(page, path, drive = 0) {
  const bytes = await readFile(path);
  const name = path.split('/').pop();
  return page.evaluate(([name, b64, drive]) => {
    const bin = atob(b64);
    const u8 = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
    return wasm_loadfile(name, u8, drive);
  }, [name, bytes.toString('base64'), drive]);
}

export const eject = (page, drive) => page.evaluate((d) => wasm_eject_disk(`df${d}`), drive);
export const reset = (page) => page.evaluate(() => wasm_reset());

// Wait n emulated frames. Node-side loop with a wall-clock deadline, so a stalled page shows
// up as an error instead of a silent hang (in-page polling stops when the page stalls).
export async function waitFrames(page, n, { deadlineMs = Math.max(60000, n * 100) } = {}) {
  const t0 = Date.now(), start = await frame(page);
  for (;;) {
    const f = await frame(page);
    if (f - start >= n) return f - start;
    if (Date.now() - t0 > deadlineMs) throw new Error(`waitFrames: ${f - start}/${n} frames after ${deadlineMs} ms (emulation stalled)`);
    await new Promise((r) => setTimeout(r, 50));
  }
}

export const unmapped = (page) => page.evaluate(() => window.__u6_unmapped || 0);

// Press and release one key (raw keycode or KEY name), holding for `hold` frames.
export async function tap(page, key, { hold = 4, gap = 4, mods = [] } = {}) {
  const code = typeof key === 'number' ? key : KEY[key];
  if (code === undefined) throw new Error(`unknown key ${key}`);
  for (const m of mods) await page.evaluate((c) => wasm_key(c, 1), KEY[m]);
  await page.evaluate((c) => wasm_key(c, 1), code);
  await waitFrames(page, hold);
  await page.evaluate((c) => wasm_key(c, 0), code);
  for (const m of mods) await page.evaluate((c) => wasm_key(c, 0), KEY[m]);
  await waitFrames(page, gap);
}

// Type a string on the US keymap (letters lowercase unless uppercase given).
export async function type(page, text, opts = {}) {
  for (const ch of text) {
    if (ch === '\n') { await tap(page, 'RETURN', opts); continue; }
    const lower = ch.toLowerCase();
    if (KEY[lower] !== undefined && ch !== lower) { await tap(page, lower, { ...opts, mods: ['LSHIFT'] }); continue; }
    if (SHIFTED[ch]) { await tap(page, SHIFTED[ch], { ...opts, mods: ['LSHIFT'] }); continue; }
    await tap(page, lower, opts);
  }
}

// Native framebuffer: full 1824x313 RGBA texture of the last completed frame.
export async function grab(page) {
  const b64 = await page.evaluate(([w, h]) => {
    const ptr = Module._wasm_pixel_buffer();
    const view = new Uint8Array(Module.HEAPU8.buffer, ptr, w * h * 4);
    let s = '';
    for (let i = 0; i < view.length; i += 0x8000) s += String.fromCharCode.apply(null, view.subarray(i, i + 0x8000));
    return btoa(s);
  }, [TEX_W, TEX_H]);
  const rgba = new Uint8Array(Buffer.from(b64, 'base64'));
  for (let i = 3; i < rgba.length; i += 4) rgba[i] = 255;
  return rgba;
}

// Downsample the texture horizontally by `step` texels: 4 = lores (456 wide, one value per
// lores pixel), 2 = hires (912 wide). Vertical is already 1 line per row (one field).
export function sample(rgba, step) {
  const w = TEX_W / step;
  const out = new Uint8Array(w * TEX_H * 4);
  for (let y = 0; y < TEX_H; y++)
    for (let x = 0; x < w; x++) {
      const s = (y * TEX_W + x * step) * 4, d = (y * w + x) * 4;
      out[d] = rgba[s]; out[d + 1] = rgba[s + 1]; out[d + 2] = rgba[s + 2]; out[d + 3] = 255;
    }
  return { width: w, height: TEX_H, rgba: out };
}

// Save a capture. modes: 'lores' -> NAME.png (456x313), 'hires' -> NAME.hires.png (912x313),
// 'tex' -> NAME.tex.png (1824x313 raw texture), 'page' -> NAME.page.png (browser viewport).
export async function shot(page, name, { dir = SHOTS_DIR, modes = ['lores'] } = {}) {
  const rgba = await grab(page);
  const written = [];
  const put = async (file, buf) => {
    await mkdir(dirname(file), { recursive: true });
    await writeFile(file, buf);
    written.push({ file, sha256: createHash('sha256').update(buf).digest('hex') });
  };
  for (const m of modes) {
    if (m === 'lores' || m === 'hires') {
      const v = sample(rgba, m === 'lores' ? 4 : 2);
      await put(join(dir, m === 'lores' ? `${name}.png` : `${name}.hires.png`), encodePNG(v.width, v.height, v.rgba));
    } else if (m === 'tex') {
      await put(join(dir, `${name}.tex.png`), encodePNG(TEX_W, TEX_H, rgba));
    } else if (m === 'page') {
      await put(join(dir, `${name}.page.png`), await page.screenshot());
    } else throw new Error(`unknown shot mode ${m}`);
  }
  return written;
}

// Write the current disk in DFn back out as an ADF (wasm_export_disk -> {address,size}).
export async function exportDisk(page, drive, outPath) {
  const b64 = await page.evaluate((d) => {
    const o = JSON.parse(wasm_export_disk(`df${d}`, 0, ''));
    if (!o.size) return '';
    const view = new Uint8Array(Module.HEAPU8.buffer, o.address, o.size);
    let s = '';
    for (let i = 0; i < view.length; i += 0x8000) s += String.fromCharCode.apply(null, view.subarray(i, i + 0x8000));
    Module._wasm_delete_disk();
    return btoa(s);
  }, drive);
  if (!b64) throw new Error(`df${drive}: no disk`);
  const buf = Buffer.from(b64, 'base64');
  await mkdir(dirname(outPath), { recursive: true });
  await writeFile(outPath, buf);
  return { file: outPath, size: buf.length, sha256: createHash('sha256').update(buf).digest('hex') };
}

export const hasDisk = (page, drive) => page.evaluate((d) => !!wasm_has_disk(`df${d}`), drive);
export const setWarp = (page, on) => page.evaluate((on) => wasm_set_warp(on ? 1 : 0), on);

// Machine config as the core reports it (names as used by vAmiga_ui.js:3708-3719).
export async function machine(page) {
  return page.evaluate(() => {
    const out = {};
    for (const k of ['AGNUS.REVISION', 'DENISE.REVISION', 'CPU.REVISION', 'CHIP_RAM', 'SLOW_RAM', 'FAST_RAM'])
      out[k] = wasm_get_config_item(k, 0);
    out.DRIVE_CONNECT = [0, 1, 2, 3].map((i) => wasm_get_config_item('DRIVE_CONNECT', i));
    return out;
  });
}

// FNV-1a over the native texture, computed in-page (no 2.3 MB transfer per poll).
export const texHash = (page) => page.evaluate(([w, h]) => {
  const v = new Uint32Array(Module.HEAPU8.buffer, Module._wasm_pixel_buffer(), w * h);
  let x = 0x811c9dc5;
  for (let i = 0; i < v.length; i += 3) { x ^= v[i]; x = Math.imul(x, 0x01000193); }
  return x >>> 0;
}, [TEX_W, TEX_H]);

export const diskActivity = (page) => page.evaluate(() => Module._wasm_activity(2, 0));

// Wait until the picture has not changed AND disk DMA has been zero for `frames` emulated
// frames (PAL: 50 = 1 s). Disk loads leave the screen static, so the picture alone lies.
export async function waitStable(page, { frames = 150, timeoutFrames = 50 * 600, disk = true } = {}) {
  const f0 = await frame(page);
  let last = await texHash(page), since = f0;
  for (;;) {
    await page.waitForTimeout(100);
    const f = await frame(page), hsh = await texHash(page);
    const busy = disk && (await diskActivity(page)) > 0;
    if (hsh !== last || busy) { last = hsh; since = f; }
    if (f - since >= frames) return { frames: f - f0, hash: hsh };
    if (f - f0 > timeoutFrames) throw new Error(`waitStable: no stable picture within ${timeoutFrames} frames`);
  }
}

const fromB64 = (b64) => Buffer.from(b64, 'base64');
const sha = (buf) => createHash('sha256').update(buf).digest('hex');

// Build an FFS-formatted HDF in-page from `files` ({ 'rel/path': Buffer|string }) via the core's
// folder import (main.cpp: wasm_export_disk('/imported_hd', mb, name) -> HardDrive::importFolder).
export async function makeHdf(page, outPath, { mb = 10, name = 'U6HD', files = {} } = {}) {
  const enc = Object.entries(files).map(([p, d]) => [p, Buffer.from(d).toString('base64')]);
  const b64 = await page.evaluate(([enc, mb, name]) => {
    const root = '/imported_hd';
    const rm = (p) => {
      for (const e of FS.readdir(p)) {
        if (e === '.' || e === '..') continue;
        const q = `${p}/${e}`;
        if (FS.isDir(FS.stat(q).mode)) { rm(q); FS.rmdir(q); } else FS.unlink(q);
      }
    };
    if (!FS.analyzePath(root).exists) FS.mkdir(root); else rm(root);
    for (const [p, d] of enc) {
      const parts = p.split('/');
      let cur = root;
      for (const dir of parts.slice(0, -1)) { cur += `/${dir}`; if (!FS.analyzePath(cur).exists) FS.mkdir(cur); }
      const bin = atob(d), u8 = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
      FS.writeFile(`${root}/${p}`, u8);
    }
    const o = JSON.parse(wasm_export_disk(root, mb, name));
    if (!o.size) return '';
    const view = new Uint8Array(Module.HEAPU8.buffer, o.address, o.size);
    let s = '';
    for (let i = 0; i < view.length; i += 0x8000) s += String.fromCharCode.apply(null, view.subarray(i, i + 0x8000));
    Module._wasm_delete_disk();
    return btoa(s);
  }, [enc, mb, name]);
  if (!b64) throw new Error('makeHdf: core returned no image');
  const buf = fromB64(b64);
  await mkdir(dirname(outPath), { recursive: true });
  await writeFile(outPath, buf);
  return { file: outPath, size: buf.length, sha256: sha(buf) };
}

// Write DHn back out as an HDF.
export async function exportHd(page, idx, outPath) {
  const b64 = await page.evaluate((d) => {
    const o = JSON.parse(wasm_export_disk(`dh${d}`, 0, ''));
    if (!o.size) return '';
    const view = new Uint8Array(Module.HEAPU8.buffer, o.address, o.size);
    let s = '';
    for (let i = 0; i < view.length; i += 0x8000) s += String.fromCharCode.apply(null, view.subarray(i, i + 0x8000));
    Module._wasm_delete_disk();
    return btoa(s);
  }, idx);
  if (!b64) throw new Error(`dh${idx}: no disk`);
  const buf = fromB64(b64);
  await mkdir(dirname(outPath), { recursive: true });
  await writeFile(outPath, buf);
  return { file: outPath, size: buf.length, sha256: sha(buf) };
}

// Copy DHn's file tree to a host directory (core's MutableFileSystem::exportFiles).
export async function exportTree(page, idx, hostDir) {
  const files = await page.evaluate((d) => {
    const root = '/exported_u6';
    const rm = (p) => {
      for (const e of FS.readdir(p)) {
        if (e === '.' || e === '..') continue;
        const q = `${p}/${e}`;
        if (FS.isDir(FS.stat(q).mode)) { rm(q); FS.rmdir(q); } else FS.unlink(q);
      }
    };
    // exportFiles refuses a non-empty target ("Directory is not empty").
    if (!FS.analyzePath(root).exists) FS.mkdir(root); else rm(root);
    const r = wasm_export_as_folder(`dh${d}`, root);
    if (r.startsWith('{"error"')) throw new Error(r);
    const out = [];
    const walk = (p, rel) => {
      for (const e of FS.readdir(p)) {
        if (e === '.' || e === '..') continue;
        const q = `${p}/${e}`, r = rel ? `${rel}/${e}` : e;
        if (FS.isDir(FS.stat(q).mode)) walk(q, r);
        else {
          const u8 = FS.readFile(q);
          let s = '';
          for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode.apply(null, u8.subarray(i, i + 0x8000));
          out.push([r, btoa(s)]);
        }
      }
    };
    walk(root, '');
    return out;
  }, idx);
  const list = [];
  for (const [rel, b64] of files) {
    const buf = fromB64(b64), file = join(hostDir, rel);
    await mkdir(dirname(file), { recursive: true });
    await writeFile(file, buf);
    list.push({ rel, size: buf.length, sha256: sha(buf) });
  }
  return list;
}

// Host directory tree -> { 'rel/path': Buffer } for makeHdf.
export async function filesFromDir(dir, prefix = '') {
  const { readdir } = await import('node:fs/promises');
  const out = {};
  for (const e of await readdir(dir, { withFileTypes: true })) {
    const p = join(dir, e.name), rel = prefix ? `${prefix}/${e.name}` : e.name;
    if (e.isDirectory()) Object.assign(out, await filesFromDir(p, rel));
    else if (e.isFile()) out[rel] = await readFile(p);
  }
  return out;
}

// DMA activity of the last frames as the core reports it (main.cpp wasm_activity ids 0-5).
export const activity = (page) => page.evaluate(() => {
  const n = ['copper', 'blitter', 'disk', 'audio', 'sprite', 'bitplane'], out = {};
  n.forEach((k, i) => { out[k] = Number(Module._wasm_activity(i, 0).toFixed(4)); });
  return out;
});

// Mouse on control port 1: relative move (dx, dy); buttons 1 = left, 2 = middle, 3 = right
// (main.cpp wasm_mouse_button).
export const mouseMove = (page, dx, dy) => page.evaluate(([x, y]) => wasm_mouse(1, x, y), [dx, dy]);
export async function click(page, button = 1, { hold = 4 } = {}) {
  await page.evaluate((b) => wasm_mouse_button(1, b, 1), button);
  await waitFrames(page, hold);
  await page.evaluate((b) => wasm_mouse_button(1, b, 0), button);
}

// Wait for an exact picture (texHash) - boot is deterministic, so known screens recur.
export async function waitHash(page, hash, { timeoutFrames = 50 * 1200 } = {}) {
  const f0 = await frame(page);
  for (;;) {
    if ((await texHash(page)) === hash) return { frames: (await frame(page)) - f0 };
    if ((await frame(page)) - f0 > timeoutFrames) throw new Error(`waitHash ${hash}: not reached in ${timeoutFrames} frames`);
    await page.waitForTimeout(250);
  }
}

// RetroShell by keystrokes: wasm_retro_shell_press_key calls emu->update() itself, so this also
// works while the CPU is paused at a breakpoint/catchpoint (execScript only runs in the run loop).
// Returns only the console text added by this command.
export async function shellKeys(page, cmd) {
  return page.evaluate((cmd) => {
    const before = UTF8ToString(Module._wasm_retro_shell_get_text());
    for (const ch of cmd + '\n') Module._wasm_retro_shell_press_key(ch.charCodeAt(0));
    const after = UTF8ToString(Module._wasm_retro_shell_get_text());
    return after.startsWith(before) ? after.slice(before.length) : after;
  }, cmd);
}

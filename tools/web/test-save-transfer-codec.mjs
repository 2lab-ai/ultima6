// Synthetic save ZIPs against the real browser WASM API. No original files or live game.
// Build both WASMs first; run: node tools/web/test-save-transfer-codec.mjs [original|wide]
import assert from 'node:assert/strict';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { serve } from './serve.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const layouts = process.argv[2] ? [process.argv[2]] : ['original', 'wide'];
assert.ok(layouts.every((layout) => ['original', 'wide'].includes(layout)), 'known layout');
const server = await serve({ root: resolve(repo, 'site') });
let browser;
try {
  browser = await chromium.launch({ headless: true });
  for (const layout of layouts) {
    const page = await browser.newPage();
    await page.route('**/save-transfer-test', (route) => route.fulfill({ contentType: 'text/html', body: '<!doctype html><title>Save transfer codec test</title>' }));
    await page.goto(`${server.url}/save-transfer-test`);
    const result = await page.evaluate(async (layout) => {
      const check = (condition, label) => { if (!condition) throw new Error(label); };
      const { createSaveTransferCodec } = await import('/save-transfer.mjs');
      const wasm = await import(layout === 'wide' ? '/pkg-wide/u6_web.js' : '/pkg/u6_web.js');
      await wasm.default();
      const codec = createSaveTransferCodec(wasm);
      const tests = [];
      const pass = (label) => tests.push(label);
      const rejects = async (label, action, code) => {
        let rejected = false;
        try { await action(); } catch (error) {
          rejected = true;
          check(error.code === code, `${label}: fixed error code`);
          check(!error.message.includes('private-marker'), `${label}: no input text in error`);
        }
        check(rejected, `${label}: must reject`);
        pass(label);
      };
      const equal = (a, b) => a.length === b.length && a.every((byte, i) => byte === b[i]);
      const raw = new Uint8Array(0x1c73);
      for (let i = 0; i < raw.length; i += 1) raw[i] = (i * 31) & 255;
      raw[0xff0] = 1; raw[0x1bfb] = 0;
      const original = raw.slice();
      const createdAt = '2026-10-30T12:00:00.000Z';
      const input = [{ path: 'SAVEGAME/OBJLIST', bytes: raw }, { path: 'savegame/objblkAA', bytes: new Uint8Array([0, 0]) }];
      const pending = codec.encode(input, { createdAt });
      raw[0] ^= 1;
      const zip = await pending;
      const decoded = await codec.decode(zip);
      check(decoded.files.length === 2, 'manifest is separate from payloads');
      check(decoded.files[0].path === 'savegame/objblkaa' && decoded.files[1].path === 'savegame/objlist', 'canonical paths');
      check(equal(decoded.files[1].bytes, original), 'snapshot before await preserves original payload');
      check(decoded.manifest.createdAt === createdAt && decoded.manifest.version === 1, 'manifest metadata');
      pass('byte-exact roundtrip and encode snapshot');
      const again = await codec.encode(decoded.files, { createdAt });
      check(equal(zip, again), 'deterministic stored ZIP');
      pass('deterministic stored ZIP');
      const asFile = await codec.decode(new File([zip], 'private-marker.zip'));
      check(equal(asFile.files[1].bytes, original), 'File byte-exact roundtrip');
      const padded = new Uint8Array(zip.length + 12); padded.set(zip, 7);
      check((await codec.decode(padded.subarray(7, 7 + zip.length))).files.length === 2, 'subarray offsets');
      pass('File and Uint8Array subarray input');

      // Independent simple fixture writer, consumed by the native ZIP/CRC reader.
      const crc = (bytes) => {
        let c = 0xffffffff;
        for (const b of bytes) {
          c ^= b;
          for (let bit = 0; bit < 8; bit += 1) c = (c >>> 1) ^ ((c & 1) ? 0xedb88320 : 0);
        }
        return (c ^ 0xffffffff) >>> 0;
      };
      const archive = (files) => {
        const enc = new TextEncoder();
        const items = files.map(({ path, bytes }) => ({ name: enc.encode(path), bytes }));
        const localSize = items.reduce((n, e) => n + 30 + e.name.length + e.bytes.length, 0);
        const centralSize = items.reduce((n, e) => n + 46 + e.name.length, 0);
        const out = new Uint8Array(localSize + centralSize + 22), v = new DataView(out.buffer);
        const u16 = (at, n) => v.setUint16(at, n, true), u32 = (at, n) => v.setUint32(at, n, true);
        let l = 0, c = localSize;
        for (const { name, bytes } of items) {
          const sum = crc(bytes);
          u32(l, 0x04034b50); u16(l + 4, 20); u16(l + 6, 0x800);
          u32(l + 14, sum); u32(l + 18, bytes.length); u32(l + 22, bytes.length); u16(l + 26, name.length);
          out.set(name, l + 30); out.set(bytes, l + 30 + name.length);
          u32(c, 0x02014b50); u16(c + 4, 20); u16(c + 6, 20); u16(c + 8, 0x800);
          u32(c + 16, sum); u32(c + 20, bytes.length); u32(c + 24, bytes.length); u16(c + 28, name.length); u32(c + 42, l);
          out.set(name, c + 46);
          l += 30 + name.length + bytes.length; c += 46 + name.length;
        }
        u32(c, 0x06054b50); u16(c + 8, items.length); u16(c + 10, items.length);
        u32(c + 12, centralSize); u32(c + 16, localSize);
        return out;
      };
      const withManifest = (manifest, files = decoded.files) => archive([
        { path: 'manifest.json', bytes: new TextEncoder().encode(JSON.stringify(manifest)) }, ...files,
      ]);
      const schemaCases = [
        ['version', (m) => { m.version = 2; }],
        ['game', (m) => { m.game = 'private-marker'; }],
        ['format', (m) => { m.format = 'private-marker'; }],
        ['date', (m) => { m.createdAt = '2026-02-30T00:00:00.000Z'; }],
        ['extra field', (m) => { m.extra = 'private-marker'; }],
        ['entry extra field', (m) => { m.files[0].extra = true; }],
        ['duplicate manifest path', (m) => { m.files[1] = { ...m.files[0] }; }],
        ['uppercase manifest path', (m) => { m.files[0].path = m.files[0].path.toUpperCase(); }],
        ['missing manifest file', (m) => { m.files.pop(); }],
        ['noninteger size', (m) => { m.files[0].size = 1.5; }],
        ['invalid SHA spelling', (m) => { m.files[0].sha256 = 'private-marker'; }],
      ];
      for (const [label, mutate] of schemaCases) {
        const manifest = structuredClone(decoded.manifest); mutate(manifest);
        await rejects(label, () => codec.decode(withManifest(manifest)), 'save-transfer-manifest');
      }
      const badSha = structuredClone(decoded.manifest);
      badSha.files[0].sha256 = '0'.repeat(64);
      await rejects('SHA mismatch', () => codec.decode(withManifest(badSha)), 'save-transfer-integrity');
      const badSize = structuredClone(decoded.manifest); badSize.files[0].size += 1;
      await rejects('manifest size mismatch', () => codec.decode(withManifest(badSize)), 'save-transfer-integrity');
      const changed = decoded.files.map((f) => ({ path: f.path, bytes: f.bytes.slice() }));
      changed[1].bytes[0] ^= 1;
      await rejects('valid CRC with changed content', () => codec.decode(withManifest(decoded.manifest, changed)), 'save-transfer-integrity');
      await rejects('unlisted save payload', () => codec.decode(withManifest(decoded.manifest, [...decoded.files, { path: 'savegame/objblkba', bytes: new Uint8Array([0, 0]) }])), 'save-transfer-manifest');
      const invalidCrc = zip.slice(); invalidCrc[30 + 'manifest.json'.length] ^= 1;
      await rejects('CRC mismatch', () => codec.decode(invalidCrc), 'save-transfer-invalid-zip');
      await rejects('truncated archive', () => codec.decode(zip.subarray(0, zip.length - 1)), 'save-transfer-invalid-zip');
      const trailing = new Uint8Array(zip.length + 1); trailing.set(zip);
      await rejects('trailing data', () => codec.decode(trailing), 'save-transfer-invalid-zip');
      await rejects('unknown ZIP payload', () => codec.decode(withManifest(decoded.manifest, [...decoded.files, { path: 'packs/private-marker', bytes: new Uint8Array([0]) }])), 'save-transfer-path');
      await rejects('casefold duplicate export', () => codec.encode([{ path: 'savegame/objlist', bytes: original }, { path: 'SAVEGAME/OBJLIST', bytes: original }]), 'save-transfer-duplicate');
      await rejects('traversal export', () => codec.encode([{ path: '../savegame/objlist', bytes: original }]), 'save-transfer-path');
      await rejects('backslash export', () => codec.encode([{ path: 'savegame\\objlist', bytes: original }]), 'save-transfer-path');
      await rejects('missing objlist export', () => codec.encode([{ path: 'savegame/objblkaa', bytes: new Uint8Array([0, 0]) }]), 'save-transfer-missing-objlist');
      await rejects('short objlist export', () => codec.encode([{ path: 'savegame/objlist', bytes: new Uint8Array(2) }]), 'save-transfer-invalid-objlist');
      await rejects('invalid block export', () => codec.encode([{ path: 'savegame/objlist', bytes: original }, { path: 'savegame/objblkaa', bytes: new Uint8Array([0, 1]) }]), 'save-transfer-invalid-block');
      let read = false;
      const oversized = new File([], 'private-marker.zip');
      Object.defineProperty(oversized, 'size', { value: 16 * 1024 * 1024 + 1 });
      oversized.arrayBuffer = async () => { read = true; return new ArrayBuffer(0); };
      await rejects('oversized File', () => codec.decode(oversized), 'save-transfer-too-large');
      check(!read, 'oversized File rejected before arrayBuffer');
      const unreadable = new File([], 'private-marker.zip');
      unreadable.arrayBuffer = async () => { throw new Error('private-marker'); };
      await rejects('file read failure', () => codec.decode(unreadable), 'save-transfer-read');
      return { layout, passed: tests.length, failed: 0, tests, nativeWasm: true, originalData: false };
    }, layout);
    console.log(JSON.stringify(result));
    await page.close();
  }
} finally {
  await browser?.close();
  await server.close();
}

// Save ZIPs exported by this engine only. SHA-256 verifies content consistency, not who
// created an archive. This module neither reads original game files nor changes a live game.
const ARCHIVE_LIMIT = 16 * 1024 * 1024;
const MANIFEST_LIMIT = 32 * 1024;
const FILE_LIMIT = 24578;
const SIGHT_LIMIT = 7421996;
const MANIFEST = 'manifest.json';
const MESSAGES = Object.freeze({
  'save-transfer-too-large': 'The save archive exceeds its size limit.',
  'save-transfer-entry-limit': 'The save archive contains too many files.',
  'save-transfer-invalid-zip': 'Choose a save ZIP exported by this engine.',
  'save-transfer-path': 'The archive contains a file that is not a supported save file.',
  'save-transfer-duplicate': 'The archive contains duplicate save files.',
  'save-transfer-manifest': 'The save archive manifest is invalid or unsupported.',
  'save-transfer-missing-objlist': 'The archive is missing the saved game state.',
  'save-transfer-invalid-objlist': 'The saved game state is invalid.',
  'save-transfer-invalid-block': 'A saved world block is invalid.',
  'save-transfer-invalid-sight': 'The saved sight data is invalid.',
  'save-transfer-sight-mismatch': 'Save normally in the original game again: the sight data belongs to another save.',
  'save-transfer-index': 'The save archive file index is invalid.',
  'save-transfer-integrity': 'The save archive contents do not match its manifest.',
  'save-transfer-input': 'Choose a save ZIP exported by this engine.',
  'save-transfer-read': 'The selected save archive could not be read.',
  'save-transfer-crypto': 'Content verification is unavailable in this browser.',
  'save-transfer-config': 'Save transfer is unavailable in this build.',
});

function failure(code) {
  const error = new Error(MESSAGES[code] ?? MESSAGES['save-transfer-input']);
  error.code = Object.hasOwn(MESSAGES, code) ? code : 'save-transfer-input';
  return error;
}

function safe(error) {
  return failure(typeof error === 'string' ? error : error?.code);
}

function pathOf(path) {
  if (typeof path !== 'string' || !/^[\x20-\x7e]+$/.test(path)) throw failure('save-transfer-path');
  const canonical = path.toLowerCase();
  if (!/^savegame\/(?:objlist|u6sight|objblk(?:[a-h][a-h]|[a-e]i))$/.test(canonical)) {
    throw failure('save-transfer-path');
  }
  return canonical;
}

function exactKeys(object, keys) {
  return object !== null && typeof object === 'object' && !Array.isArray(object)
    && Object.keys(object).length === keys.length && keys.every((key) => Object.hasOwn(object, key));
}

function dateIsValid(value) {
  return typeof value === 'string' && /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/.test(value)
    && Number.isFinite(Date.parse(value)) && new Date(value).toISOString() === value;
}

async function sha256(bytes) {
  if (!globalThis.crypto?.subtle) throw failure('save-transfer-crypto');
  try {
    const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
    return [...digest].map((value) => value.toString(16).padStart(2, '0')).join('');
  } catch {
    throw failure('save-transfer-crypto');
  }
}

const CRC_TABLE = Uint32Array.from({ length: 256 }, (_, i) => {
  let value = i;
  for (let bit = 0; bit < 8; bit += 1) value = (value >>> 1) ^ ((value & 1) ? 0xedb88320 : 0);
  return value >>> 0;
});

function crc32(bytes) {
  let value = 0xffffffff;
  for (const byte of bytes) value = (value >>> 8) ^ CRC_TABLE[(value ^ byte) & 255];
  return (value ^ 0xffffffff) >>> 0;
}

// Stored ZIP, no comments/extras/data descriptors. All lengths are bounded before allocation.
function storedZip(files, cap) {
  const encoder = new TextEncoder();
  const entries = files.map(({ path, bytes }) => ({ name: encoder.encode(path), bytes }));
  const localSize = entries.reduce((sum, e) => sum + 30 + e.name.length + e.bytes.length, 0);
  const centralSize = entries.reduce((sum, e) => sum + 46 + e.name.length, 0);
  const total = localSize + centralSize + 22;
  if (total > cap) throw failure('save-transfer-too-large');
  const output = new Uint8Array(total);
  const view = new DataView(output.buffer);
  const u16 = (at, n) => view.setUint16(at, n, true);
  const u32 = (at, n) => view.setUint32(at, n, true);
  let local = 0;
  let central = localSize;
  for (const { name, bytes } of entries) {
    const crc = crc32(bytes);
    u32(local, 0x04034b50); u16(local + 4, 20); u16(local + 6, 0x800);
    u32(local + 14, crc); u32(local + 18, bytes.length); u32(local + 22, bytes.length);
    u16(local + 26, name.length);
    output.set(name, local + 30); output.set(bytes, local + 30 + name.length);
    u32(central, 0x02014b50); u16(central + 4, 20); u16(central + 6, 20); u16(central + 8, 0x800);
    u32(central + 16, crc); u32(central + 20, bytes.length); u32(central + 24, bytes.length);
    u16(central + 28, name.length); u32(central + 42, local);
    output.set(name, central + 46);
    local += 30 + name.length + bytes.length;
    central += 46 + name.length;
  }
  u32(central, 0x06054b50); u16(central + 8, entries.length); u16(central + 10, entries.length);
  u32(central + 12, centralSize); u32(central + 16, localSize);
  return output;
}

export function createSaveTransferCodec({ SaveTransfer, max_save_archive_bytes } = {}) {
  if (typeof SaveTransfer !== 'function' || typeof SaveTransfer.from_zip !== 'function') {
    throw failure('save-transfer-config');
  }
  const supplied = typeof max_save_archive_bytes === 'function' ? max_save_archive_bytes() : max_save_archive_bytes;
  const cap = supplied === undefined ? ARCHIVE_LIMIT : Math.min(supplied, ARCHIVE_LIMIT);
  if (!Number.isSafeInteger(cap) || cap <= 0) throw failure('save-transfer-config');

  async function encode(files, metadata = {}) {
    let transfer;
    try {
      if (!Array.isArray(files)) throw failure('save-transfer-input');
      if (files.length > 71) throw failure('save-transfer-entry-limit');
      if (!exactKeys(metadata, []) && !exactKeys(metadata, ['createdAt'])) throw failure('save-transfer-manifest');
      const createdAt = metadata.createdAt ?? new Date().toISOString();
      if (!dateIsValid(createdAt)) throw failure('save-transfer-manifest');
      const paths = new Set();
      let total = 0;
      const checked = files.map((file) => {
        const path = pathOf(file?.path);
        if (paths.has(path)) throw failure('save-transfer-duplicate');
        paths.add(path);
        const bytes = file.bytes;
        if (!(bytes instanceof Uint8Array)) throw failure('save-transfer-input');
        if (bytes.length > (path === 'savegame/u6sight' ? SIGHT_LIMIT : FILE_LIMIT)) throw failure('save-transfer-too-large');
        total += bytes.length;
        if (total > cap) throw failure('save-transfer-too-large');
        return { path, bytes };
      });
      // Snapshot every input before the first await: caller edits cannot race hashes/ZIP data.
      const snapshot = checked.map(({ path, bytes }) => ({ path, bytes: bytes.slice() }))
        .sort((a, b) => a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
      transfer = new SaveTransfer();
      for (const { path, bytes } of snapshot) transfer.add_file(path, bytes);
      transfer.validate();
      const entries = [];
      for (const { path, bytes } of snapshot) entries.push({ path, size: bytes.length, sha256: await sha256(bytes) });
      const manifest = { format: 'ultima6-save', version: 1, game: 'ultima6-amiga', createdAt, files: entries };
      const manifestBytes = new TextEncoder().encode(JSON.stringify(manifest));
      if (manifestBytes.length > MANIFEST_LIMIT) throw failure('save-transfer-too-large');
      return storedZip([{ path: MANIFEST, bytes: manifestBytes }, ...snapshot], cap);
    } catch (error) {
      throw safe(error);
    } finally {
      transfer?.free();
    }
  }

  async function decode(input) {
    let transfer;
    try {
      let data;
      if (input instanceof Uint8Array) {
        if (input.length > cap) throw failure('save-transfer-too-large');
        data = input;
      } else if (typeof File !== 'undefined' && input instanceof File) {
        if (input.size > cap) throw failure('save-transfer-too-large');
        try { data = new Uint8Array(await input.arrayBuffer()); }
        catch { throw failure('save-transfer-read'); }
        if (data.length > cap || data.length !== input.size) throw failure('save-transfer-too-large');
      } else {
        throw failure('save-transfer-input');
      }
      transfer = SaveTransfer.from_zip(data);
      const files = [];
      let manifestBytes;
      for (let i = 0; i < transfer.count(); i += 1) {
        const path = transfer.file_path(i);
        const bytes = transfer.file_bytes(i);
        if (path === MANIFEST) manifestBytes = bytes;
        else files.push({ path, bytes });
      }
      let manifest;
      try { manifest = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(manifestBytes)); }
      catch { throw failure('save-transfer-manifest'); }
      if (!exactKeys(manifest, ['format', 'version', 'game', 'createdAt', 'files'])
          || manifest.format !== 'ultima6-save' || manifest.version !== 1 || manifest.game !== 'ultima6-amiga'
          || !dateIsValid(manifest.createdAt) || !Array.isArray(manifest.files)
          || manifest.files.length !== files.length || !files.length || files.length > 71) {
        throw failure('save-transfer-manifest');
      }
      const byPath = new Map(files.map((file) => [file.path, file]));
      const seen = new Set();
      for (const entry of manifest.files) {
        if (!exactKeys(entry, ['path', 'size', 'sha256']) || typeof entry.path !== 'string'
            || pathOf(entry.path) !== entry.path || seen.has(entry.path)
            || !Number.isSafeInteger(entry.size) || entry.size < 0
            || typeof entry.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(entry.sha256)) {
          throw failure('save-transfer-manifest');
        }
        seen.add(entry.path);
        const file = byPath.get(entry.path);
        if (!file || file.bytes.length !== entry.size || await sha256(file.bytes) !== entry.sha256) {
          throw failure('save-transfer-integrity');
        }
      }
      return { files, manifest };
    } catch (error) {
      throw safe(error);
    } finally {
      transfer?.free();
    }
  }

  return { encode, decode };
}

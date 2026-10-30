// The game files of the last start, kept in this browser's IndexedDB so a reload can start the
// game without a new drop (the files the engine booted from, never uploaded), and the saved
// game: the `savegame/` files the game's Save writes (the original's names), one record each
// under `save:` + the lower-cased name in the same store, so a later Save replaces only the
// blocks it wrote and an imported folder's case does not make a second copy. The HD pack
// loaded (spec 07 H2) is one record under `hd:pack`, its file's bytes. The NPC bundles an older
// page made here (spec 09 Design 5) lay under `npc-bundle:`; the page now only removes them
// (`forgetBundles`). The voice pack (spec 09 Design 4,
// `u6.voice-pack/1` or `/2`: the owner's personality voices, by actor number; numbers only) is one
// record under `voice-pack:` + `voices`, the pack object checked, and the listed file it came from
// one under `voice-pack:source`, in the pack's own transaction as the Korean pack's source
// below. The Korean pack (spec 08,
// `ko.u6lp`: the owner's translation of the conversation lines, made from his files by
// tools/i18n) is one record under `locale:ko`, its file's bytes, and where it came from (spec 08
// K6: the file name the site's listing gave it) one under `locale:ko:source`, written, read and
// removed in the same transaction as the pack: none for a dropped pack, so a kept pack never
// carries the source of another. The page serializes its own operations; save transfers also
// compare their preview snapshot inside the write transaction to detect changes from other tabs.
// Current pages also guard each Save with the epoch of their attached snapshot. Code already
// loaded from an older release has no such guard and must be reloaded before transferring.

const DB = 'ultima6-port';
const STORE = 'files';
const KEY = 'extracted';

function openDb() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
    req.onblocked = () => reject(new Error('the browser storage is in use by another tab'));
  });
}

// Runs `op` on the store in one transaction; resolves with the request's result once the
// transaction is complete (committed), rejects when it aborts or fails. An `op` that throws
// aborts it: none of the requests it made before is committed.
async function run(mode, op) {
  const db = await openDb();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, mode);
      let req;
      try {
        req = op(tx.objectStore(STORE));
      } catch (err) {
        tx.abort();
        throw err;
      }
      tx.oncomplete = () => resolve(req.result);
      tx.onabort = () => reject(tx.error ?? new Error('the storage transaction was aborted'));
      tx.onerror = () => reject(tx.error ?? req.error);
    });
  } finally {
    db.close();
  }
}

/// Stores `files` ({ path, bytes }) in place of the kept ones; true once committed, false (with
/// a console warning) when the browser does not keep them.
export async function saveFiles(files) {
  try {
    await run('readwrite', (s) => s.put(files, KEY));
    return true;
  } catch (err) {
    console.warn(`could not keep the game files: ${err}`);
    return false;
  }
}

/// The kept files, or null (none, or storage unavailable).
export async function loadFiles() {
  try {
    const files = await run('readonly', (s) => s.get(KEY));
    return Array.isArray(files) ? files : null;
  } catch (err) {
    console.warn(`could not read the kept game files: ${err}`);
    return null;
  }
}

/// Removes the kept files; resolves once the delete is committed, throws when it is not.
export async function forgetFiles() {
  await run('readwrite', (s) => s.delete(KEY));
}

const SAVE = 'save:';
const SAVES = IDBKeyRange.bound(SAVE, `${SAVE}\uffff`);
const SAVE_EPOCH = 'save-epoch';

/// Writes the files ({ path, bytes }) of one Save over the stored ones, all in one transaction;
/// `replace` drops every stored save file first (an imported savegame folder). Callers that
/// attach a snapshot must pass its epoch: a replaced/deleted save invalidates older games,
/// even if their next Save writes only one block. The comparison and writes share one tx.
/// Resolves with { epoch } once committed; replacement starts a new epoch to adopt.
export async function storeSaves(files, replace = false, expectedEpoch = undefined) {
  const copy = snapshotFiles(files);
  checkExpectedEpoch(expectedEpoch);
  return saveTransaction('readwrite', (store, done, guard) => {
    readSaveEpoch(store, guard, (epoch) => {
      compareSaveEpoch(epoch, expectedEpoch);
      const after = replace ? nextSaveEpoch(epoch) : epoch;
      if (replace) {
        store.delete(SAVES);
        store.put(after, SAVE_EPOCH);
      }
      for (const file of copy) store.put(file, SAVE + file.path.toLowerCase());
      done({ epoch: after });
    });
  });
}

/// The stored save files ({ path, bytes }), or [] (none, or storage unavailable).
export async function loadSaves() {
  try {
    const files = await run('readonly', (s) => s.getAll(SAVES));
    return Array.isArray(files) ? files : [];
  } catch (err) {
    console.warn(`could not read the saved game: ${err}`);
    return [];
  }
}

// Save-transfer backups are deliberately outside SAVES: normal engine saves and
// Forget saved game cannot overwrite them. Metadata is separate from payloads so
// listing history never materializes all earlier saves at once.
const BACKUPS = 'save-backup:';
const BACKUP_FILES = 'save-backup-files:';
const BACKUP_ORDER = 'save-backup-order';
const BACKUP_RANGE = IDBKeyRange.bound(BACKUPS, `${BACKUPS}\uffff`);
const BACKUP_ID = /^\d+-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const TRANSFER_PATH = /^savegame\/(?:objlist|u6sight|objblk[a-h][a-h]|objblk[a-e]i)$/;

// Legacy snapshots may contain unknown names. Keep their original path and bytes
// in backups; only a newly imported candidate must use the native allowlist.
function snapshotFiles(files, candidate = false) {
  if (!Array.isArray(files) || (candidate && files.length === 0)) throw new TypeError('invalid saved-game file list');
  const seen = new Set();
  return files.map((file) => {
    if (!file || typeof file.path !== 'string' || !file.path || !(file.bytes instanceof Uint8Array)) {
      throw new TypeError('invalid saved-game record');
    }
    const name = file.path.toLowerCase();
    if (candidate && (!/^[\x20-\x7e]+$/.test(file.path) || !TRANSFER_PATH.test(name))) {
      throw new TypeError('invalid imported saved-game path');
    }
    if (seen.has(name)) throw new TypeError('duplicate saved-game path');
    seen.add(name);
    return { path: candidate ? name : file.path, bytes: new Uint8Array(file.bytes) };
  }).sort((a, b) => {
    const x = a.path.toLowerCase(), y = b.path.toLowerCase();
    return x < y ? -1 : x > y ? 1 : 0;
  });
}

function sameSave(a, b) {
  return a.length === b.length && a.every((file, i) => file.path.toLowerCase() === b[i].path.toLowerCase()
    && file.bytes.length === b[i].bytes.length && file.bytes.every((byte, j) => byte === b[i].bytes[j]));
}

function checkExpectedEpoch(epoch) {
  if (epoch !== undefined && (!Number.isSafeInteger(epoch) || epoch < 0)) {
    throw new TypeError('invalid saved-game epoch');
  }
}

function readSaveEpoch(store, guard, done) {
  const request = store.get(SAVE_EPOCH);
  request.onsuccess = guard(() => {
    const epoch = request.result === undefined ? 0 : request.result;
    checkExpectedEpoch(epoch);
    done(epoch);
  });
}

function compareSaveEpoch(epoch, expectedEpoch) {
  if (expectedEpoch !== undefined && epoch !== expectedEpoch) {
    const error = new Error('the saved game was replaced; reload before saving');
    error.name = 'SaveConflictError';
    throw error;
  }
}

function nextSaveEpoch(epoch) {
  if (epoch === Number.MAX_SAFE_INTEGER) throw new RangeError('saved-game epoch exhausted');
  return epoch + 1;
}

// All request callbacks are synchronous. In particular no hashing/await can let
// the transaction commit between its comparison, backup and replacement.
async function saveTransaction(mode, work) {
  const db = await openDb();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, mode);
      let result, failure;
      const fail = (error) => {
        failure ??= error;
        try { tx.abort(); } catch { /* A failed request may already have aborted it. */ }
      };
      const guard = (fn) => (event) => {
        try { fn(event); } catch (error) { fail(error); }
      };
      tx.oncomplete = () => resolve(result);
      tx.onabort = () => reject(failure ?? tx.error ?? new Error('the storage transaction was aborted'));
      tx.onerror = (event) => { failure ??= event.target.error ?? tx.error; };
      try { work(tx.objectStore(STORE), (value) => { result = value; }, guard); }
      catch (error) { fail(error); }
    });
  } finally {
    db.close();
  }
}

function readCurrentSave(store, guard, done) {
  const files = [];
  const request = store.openCursor(SAVES);
  request.onsuccess = guard(() => {
    const cursor = request.result;
    if (!cursor) { done(snapshotFiles(files)); return; }
    const file = cursor.value;
    if (typeof file?.path !== 'string' || cursor.key !== SAVE + file.path.toLowerCase()) {
      throw new TypeError('inconsistent saved-game record');
    }
    files.push(file);
    cursor.continue();
  });
}

/// The exact saved-game snapshot, with private byte copies. Unlike loadSaves,
/// unavailable or malformed storage rejects; it must never appear to be empty. The epoch and
/// files are read in one transaction, so a game can retain ownership of that exact snapshot.
export async function readSaveSnapshot() {
  return saveTransaction('readonly', (store, done, guard) => {
    readSaveEpoch(store, guard, (epoch) => {
      readCurrentSave(store, guard, (files) => done({ files, epoch }));
    });
  });
}

/// Atomically compare the preview snapshot, preserve the previous save, and
/// install a validated candidate. A change from another tab rejects rather than
/// replacing unpreviewed progress. Identical imports write nothing at all.
export async function replaceSavesWithBackup(files, expectedFiles, expectedEpoch = undefined) {
  const candidate = snapshotFiles(files, true);
  const expected = snapshotFiles(expectedFiles);
  checkExpectedEpoch(expectedEpoch);
  return saveTransaction('readwrite', (store, done, guard) => {
    readSaveEpoch(store, guard, (epoch) => {
      compareSaveEpoch(epoch, expectedEpoch);
      readCurrentSave(store, guard, (current) => {
        if (!sameSave(current, expected)) {
          const error = new Error('the saved game changed after the preview');
          error.name = 'SaveConflictError';
          throw error;
        }
        if (sameSave(candidate, current)) { done({ changed: false, backupId: null, epoch }); return; }
        const after = nextSaveEpoch(epoch);
        const replace = (backupId) => {
          store.delete(SAVES);
          for (const file of candidate) store.put(file, SAVE + file.path);
          store.put(after, SAVE_EPOCH);
          done({ changed: true, backupId, epoch: after });
        };
        if (current.length === 0) { replace(null); return; }
        const order = store.get(BACKUP_ORDER);
        order.onsuccess = guard(() => {
          const before = order.result ?? 0;
          if (!Number.isSafeInteger(before) || before < 0 || before === Number.MAX_SAFE_INTEGER) {
            throw new TypeError('invalid backup order');
          }
          const sequence = before + 1;
          const createdAt = Date.now();
          const id = `${createdAt}-${crypto.randomUUID()}`;
          const totalBytes = current.reduce((n, file) => n + file.bytes.length, 0);
          const metadata = { id, createdAt, sequence, fileCount: current.length, totalBytes };
          checkBackupMetadata(metadata, id);
          // add, never put: an identity collision aborts instead of replacing history.
          store.add(metadata, BACKUPS + id);
          for (const [i, file] of current.entries()) store.add(file, backupFileKey(id, i));
          store.put(sequence, BACKUP_ORDER);
          replace(id);
        });
      });
    });
  });
}

const backupFileKey = (id, index) => `${BACKUP_FILES}${id}:${String(index).padStart(12, '0')}`;

function checkBackupMetadata(value, id) {
  if (!value || !BACKUP_ID.test(id) || value.id !== id || !Number.isSafeInteger(value.createdAt)
      || value.createdAt < 0 || !id.startsWith(`${value.createdAt}-`)
      || !Number.isSafeInteger(value.sequence) || value.sequence < 1
      || !Number.isSafeInteger(value.fileCount) || value.fileCount < 1
      || !Number.isSafeInteger(value.totalBytes) || value.totalBytes < 0) {
    throw new TypeError('invalid saved-game backup metadata');
  }
  return value;
}

/// Backup descriptions only, newest first even if two were made in the same
/// millisecond or the browser's wall clock moved backwards. No pruning occurs.
export async function listSaveBackups() {
  return saveTransaction('readonly', (store, done, guard) => {
    const rows = [];
    const request = store.openCursor(BACKUP_RANGE);
    request.onsuccess = guard(() => {
      const cursor = request.result;
      if (!cursor) {
        rows.sort((a, b) => b.sequence - a.sequence);
        done(rows.map(({ id, createdAt, fileCount, totalBytes }) => ({ id, createdAt, fileCount, totalBytes })));
        return;
      }
      rows.push(checkBackupMetadata(cursor.value, cursor.key.slice(BACKUPS.length)));
      cursor.continue();
    });
  });
}

/// Load one immutable backup with its original paths. Missing, incomplete or
/// unreadable backups reject instead of returning an apparently empty save.
export async function loadSaveBackup(id) {
  if (typeof id !== 'string' || !BACKUP_ID.test(id)) throw new TypeError('invalid saved-game backup id');
  return saveTransaction('readonly', (store, done, guard) => {
    const metadataRequest = store.get(BACKUPS + id);
    metadataRequest.onsuccess = guard(() => {
      if (metadataRequest.result === undefined) {
        const error = new Error('the saved-game backup does not exist');
        error.name = 'SaveBackupNotFoundError';
        throw error;
      }
      const metadata = checkBackupMetadata(metadataRequest.result, id);
      const prefix = `${BACKUP_FILES}${id}:`, files = [];
      const request = store.openCursor(IDBKeyRange.bound(prefix, `${prefix}\uffff`));
      request.onsuccess = guard(() => {
        const cursor = request.result;
        if (!cursor) {
          const copy = snapshotFiles(files);
          if (copy.length !== metadata.fileCount || copy.reduce((n, f) => n + f.bytes.length, 0) !== metadata.totalBytes) {
            throw new TypeError('incomplete saved-game backup');
          }
          done(copy);
          return;
        }
        if (cursor.key !== backupFileKey(id, files.length)) throw new TypeError('inconsistent saved-game backup');
        files.push(cursor.value);
        cursor.continue();
      });
    });
  });
}

/// Removes the active save and invalidates games attached before the deletion. Backups
/// remain immutable. The caller may adopt the returned epoch for its explicit new start.
export async function forgetSaves(expectedEpoch = undefined) {
  checkExpectedEpoch(expectedEpoch);
  return saveTransaction('readwrite', (store, done, guard) => {
    readSaveEpoch(store, guard, (epoch) => {
      compareSaveEpoch(epoch, expectedEpoch);
      const after = nextSaveEpoch(epoch);
      store.delete(SAVES);
      store.put(after, SAVE_EPOCH);
      done({ epoch: after });
    });
  });
}

const PACK = 'hd:pack';

/// Keeps the HD pack's bytes in place of the kept one; true once committed, false (with a
/// console warning) when the browser does not keep it.
export async function storePack(bytes) {
  try {
    await run('readwrite', (s) => s.put(bytes, PACK));
    return true;
  } catch (err) {
    console.warn(`could not keep the HD pack: ${err}`);
    return false;
  }
}

/// The kept HD pack's bytes, or null (none, or storage unavailable).
export async function loadPack() {
  try {
    const bytes = await run('readonly', (s) => s.get(PACK));
    return bytes instanceof Uint8Array ? bytes : null;
  } catch (err) {
    console.warn(`could not read the kept HD pack: ${err}`);
    return null;
  }
}

/// Removes the kept HD pack; resolves once the delete is committed, throws when it is not.
export async function forgetPack() {
  await run('readwrite', (s) => s.delete(PACK));
}

const BUNDLE = 'npc-bundle:';
const BUNDLES = IDBKeyRange.bound(BUNDLE, `${BUNDLE}\uffff`);

/// Removes the NPC bundles an older page kept; resolves once the delete is committed, throws
/// when it is not.
export async function forgetBundles() {
  await run('readwrite', (s) => s.delete(BUNDLES));
}

const VOICE_PACK = 'voice-pack:';
const VOICE_PACKS = IDBKeyRange.bound(VOICE_PACK, `${VOICE_PACK}\uffff`);
const VOICE_PACK_VOICES = `${VOICE_PACK}voices`;
const VOICE_PACK_SOURCE = `${VOICE_PACK}source`;

/// Keeps the voice pack `pack` (the object a voices.u6vp parses to, checked) in place of the kept
/// one, with `source` (the listed file it came from; '' for a dropped one: the source kept before
/// is removed), in one transaction. Resolves once committed, throws when it is not (the one kept
/// before stays).
export async function storeVoicePack(pack, source = '') {
  await run('readwrite', (s) => {
    s.put(pack, VOICE_PACK_VOICES);
    return source ? s.put(source, VOICE_PACK_SOURCE) : s.delete(VOICE_PACK_SOURCE);
  });
}

/// The kept voice pack, read in one transaction: { pack, source } (`source` '' when none is kept
/// or it is not text), or null when none is kept. Throws when the storage cannot be read (the
/// page cannot tell whether a pack is kept).
export async function loadVoicePack() {
  let source = null;
  const pack = await run('readonly', (s) => {
    source = s.get(VOICE_PACK_SOURCE);
    return s.get(VOICE_PACK_VOICES);
  });
  if (pack === undefined || pack === null) return null;
  return { pack, source: typeof source.result === 'string' ? source.result : '' };
}

/// Removes the kept voice pack; resolves once the delete is committed, throws when it is not.
export async function forgetVoicePack() {
  await run('readwrite', (s) => s.delete(VOICE_PACKS));
}

const LOCALE_KO = 'locale:ko';
const LOCALE_KO_SOURCE = 'locale:ko:source';

/// Keeps the Korean pack's bytes in place of the kept one, with `source` (the listed file it
/// came from; '' for a dropped one: the source kept before is removed), in one transaction;
/// true once committed, false (with a console warning that names only the error's kind, and
/// nothing changed) when the browser does not keep them.
export async function storeLocalePack(bytes, source = '') {
  try {
    await run('readwrite', (s) => {
      s.put(bytes, LOCALE_KO);
      return source ? s.put(source, LOCALE_KO_SOURCE) : s.delete(LOCALE_KO_SOURCE);
    });
    return true;
  } catch (err) {
    console.warn(`could not keep the Korean pack: ${err?.name ?? 'Error'}`);
    return false;
  }
}

/// The kept Korean pack, read in one transaction: { bytes, source } (`source` '' when none is
/// kept or it is not text), or null when none is kept. Throws when the storage cannot be read
/// (the page cannot tell whether a pack is kept).
export async function loadLocalePack() {
  let source = null;
  const bytes = await run('readonly', (s) => {
    source = s.get(LOCALE_KO_SOURCE);
    return s.get(LOCALE_KO);
  });
  if (!(bytes instanceof Uint8Array)) return null;
  return { bytes, source: typeof source.result === 'string' ? source.result : '' };
}

/// Removes the kept Korean pack and its source in one transaction; resolves once the delete is
/// committed, throws when it is not.
export async function forgetLocalePack() {
  await run('readwrite', (s) => {
    s.delete(LOCALE_KO);
    return s.delete(LOCALE_KO_SOURCE);
  });
}

function fileOf(entry) {
  return new Promise((resolve, reject) => entry.file(resolve, reject));
}

function readBatch(reader) {
  return new Promise((resolve, reject) => reader.readEntries(resolve, reject));
}

/// Walks a dropped file or folder; paths are relative to the drop.
export async function collect(entry, out) {
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

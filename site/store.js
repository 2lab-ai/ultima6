// The game files of the last start, kept in this browser's IndexedDB so a reload can start the
// game without a new drop (the files the engine booted from, never uploaded), and the saved
// game: the `savegame/` files the game's Save writes (the original's names), one record each
// under `save:` + the lower-cased name in the same store, so a later Save replaces only the
// blocks it wrote and an imported folder's case does not make a second copy. The page calls
// these one at a time (its operation queue), so a read never races a write or a delete.

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
// transaction is complete (committed), rejects when it aborts or fails.
async function run(mode, op) {
  const db = await openDb();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, mode);
      const req = op(tx.objectStore(STORE));
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

/// Writes the files ({ path, bytes }) of one Save over the stored ones, all in one transaction;
/// `replace` drops every stored save file first (an imported savegame folder). Resolves once
/// committed, throws when it is not.
export async function storeSaves(files, replace = false) {
  await run('readwrite', (s) => {
    if (replace) s.delete(SAVES);
    let last = null;
    for (const { path, bytes } of files) last = s.put({ path, bytes }, SAVE + path.toLowerCase());
    return last ?? s.count();
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

/// Removes the saved game; resolves once the delete is committed, throws when it is not.
export async function forgetSaves() {
  await run('readwrite', (s) => s.delete(SAVES));
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

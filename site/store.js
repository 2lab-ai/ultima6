// The game files of the last start, kept in this browser's IndexedDB so a reload can start the
// game without a new drop (the files the engine booted from, never uploaded). Keeping them is
// not a saved game: every start is a new game from the disks. The page calls these one at a
// time (its operation queue), so a read never races a write or a delete.

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

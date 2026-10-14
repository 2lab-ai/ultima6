// The dropped game files, kept in this browser's IndexedDB so a reload can start
// the game without a new drop (only the game files the intake extracted, never
// uploaded). Every call degrades to "not kept" when storage is unavailable.

const DB = 'ultima6-port';
const STORE = 'files';
const KEY = 'extracted';

function openDb() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function run(mode, op) {
  return openDb().then((db) => new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, mode);
    const req = op(tx.objectStore(STORE));
    tx.oncomplete = () => {
      db.close();
      resolve(req?.result);
    };
    tx.onabort = () => {
      db.close();
      reject(tx.error);
    };
  }));
}

/// Stores `files` ({ path, bytes }); true when kept.
export async function saveFiles(files) {
  try {
    await run('readwrite', (s) => s.put(files, KEY));
    return true;
  } catch (err) {
    console.warn(`could not keep the game files: ${err}`);
    return false;
  }
}

/// The kept files, or null.
export async function loadFiles() {
  try {
    const files = await run('readonly', (s) => s.get(KEY));
    return Array.isArray(files) ? files : null;
  } catch (err) {
    console.warn(`could not read the kept game files: ${err}`);
    return null;
  }
}

/// Removes the kept files.
export async function forgetFiles() {
  try {
    await run('readwrite', (s) => s.delete(KEY));
  } catch (err) {
    console.warn(`could not remove the kept game files: ${err}`);
  }
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

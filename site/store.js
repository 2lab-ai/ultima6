// The game files of the last start, kept in this browser's IndexedDB so a reload can start the
// game without a new drop (the files the engine booted from, never uploaded), and the saved
// game: the `savegame/` files the game's Save writes (the original's names), one record each
// under `save:` + the lower-cased name in the same store, so a later Save replaces only the
// blocks it wrote and an imported folder's case does not make a second copy. The HD pack
// loaded (spec 07 H2) is one record under `hd:pack`, its file's bytes. The NPC bundles (spec 09
// Design 5, `u6.npc-bundle/1`: each conversation script's NPC name and dialogue, made in this
// browser from the kept files) are one record per script under `npc-bundle:` + its archive
// letter, `/` and its item, the bundle object itself. The voice pack (spec 09 Design 4,
// `u6.voice-pack/1`: the owner's personality voices, by actor number; numbers only) is one
// record under `voice-pack:` + `voices`, the pack object checked. The Korean pack (spec 08,
// `ko.u6lp`: the owner's translation of the conversation lines, made from his files by
// tools/i18n) is one record under `locale:ko`, its file's bytes, and where it came from (spec 08
// K6: the file name the site's listing gave it) one under `locale:ko:source`, written, read and
// removed in the same transaction as the pack: none for a dropped pack, so a kept pack never
// carries the source of another. The page calls these one at a time (its operation queue), so
// a read never races a write or a delete.

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

/// Keeps `bundles` (u6.npc-bundle/1 objects) in place of the kept ones, one record each under
/// `npc-bundle:<archive>/<item>`, all in one transaction. Resolves once committed, throws when
/// it is not (nothing of it is then kept, and the bundles kept before stay).
export async function storeBundles(bundles) {
  await run('readwrite', (s) => {
    s.delete(BUNDLES);
    let last = null;
    for (const b of bundles) last = s.put(b, `${BUNDLE}${b.archive}/${b.item}`);
    return last ?? s.count();
  });
}

/// The kept NPC bundles, in (archive, item) order; [] when there are none. Throws when storage
/// cannot be read.
export async function loadBundles() {
  const bundles = await run('readonly', (s) => s.getAll(BUNDLES));
  return (Array.isArray(bundles) ? bundles : [])
    .sort((a, b) => (a.archive === b.archive ? a.item - b.item : Number(a.archive > b.archive) * 2 - 1));
}

/// Removes the kept NPC bundles; resolves once the delete is committed, throws when it is not.
export async function forgetBundles() {
  await run('readwrite', (s) => s.delete(BUNDLES));
}

const VOICE_PACK = 'voice-pack:';
const VOICE_PACKS = IDBKeyRange.bound(VOICE_PACK, `${VOICE_PACK}\uffff`);

/// Keeps the voice pack `pack` (the object a dropped voices.u6vp parses to, checked) in place of
/// the kept one. Resolves once committed, throws when it is not (the one kept before stays).
export async function storeVoicePack(pack) {
  await run('readwrite', (s) => s.put(pack, `${VOICE_PACK}voices`));
}

/// The kept voice pack object, or null (none, or storage unavailable).
export async function loadVoicePack() {
  try {
    const pack = await run('readonly', (s) => s.get(`${VOICE_PACK}voices`));
    return pack ?? null;
  } catch (err) {
    console.warn(`could not read the kept voice pack: ${err}`);
    return null;
  }
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

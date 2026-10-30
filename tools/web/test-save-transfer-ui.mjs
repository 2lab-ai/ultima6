// Independent save-transfer UI receipts. Owned disks/saves are runtime-only, never literals.
// Main roundtrips use ordinary Ctrl+S/Y saves, actual file choosers and download events.
// The harness's game-first entry skips character creation only; it does not seed save data.
// Browser coverage is isolated Chromium contexts, not Safari/Firefox or a user profile.
// Run after the WASM build: node tools/web/test-save-transfer-ui.mjs
// Private evidence: U6_SAVE_UI_OUT (default: sibling evidence/save-transfer/ui/<run>).
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { inflateRawSync } from 'node:zlib';
import * as h from './harness.mjs';

const run = new Date().toISOString().replace(/[:.]/g, '-');
const out = resolve(process.env.U6_SAVE_UI_OUT ?? join(h.REPO, '..', 'evidence', 'save-transfer', 'ui', run));
const results = [];
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const manifestName = 'manifest.json';
let env;
let currentCase = 'setup';
let downloadSequence = 0;

// This small ZIP writer/parser is independent of the product's native ZIP and JS codec.
// It is deliberately capable of duplicate/escaping/unknown paths for rejection fixtures.
const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(bytes) {
  let c = 0xffffffff;
  for (const b of bytes) c = crcTable[(c ^ b) & 255] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function zip(entries) {
  const local = [], central = [];
  let offset = 0;
  for (const { name, bytes: value } of entries) {
    const bytes = Buffer.from(value), nameBytes = Buffer.from(name);
    const head = Buffer.alloc(30), dir = Buffer.alloc(46), crc = crc32(bytes);
    head.writeUInt32LE(0x04034b50, 0); head.writeUInt16LE(20, 4);
    head.writeUInt32LE(crc, 14); head.writeUInt32LE(bytes.length, 18);
    head.writeUInt32LE(bytes.length, 22); head.writeUInt16LE(nameBytes.length, 26);
    dir.writeUInt32LE(0x02014b50, 0); dir.writeUInt16LE(20, 4); dir.writeUInt16LE(20, 6);
    dir.writeUInt32LE(crc, 16); dir.writeUInt32LE(bytes.length, 20);
    dir.writeUInt32LE(bytes.length, 24); dir.writeUInt16LE(nameBytes.length, 28);
    dir.writeUInt32LE(offset, 42);
    local.push(head, nameBytes, bytes); central.push(dir, nameBytes);
    offset += head.length + nameBytes.length + bytes.length;
  }
  const directory = Buffer.concat(central), end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10); end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...local, directory, end]);
}
function unzip(bytes) {
  const data = Buffer.from(bytes), end = data.length - 22;
  assert.ok(end >= 0 && data.readUInt32LE(end) === 0x06054b50, 'download has a complete ZIP end');
  const count = data.readUInt16LE(end + 10), entries = [];
  let at = data.readUInt32LE(end + 16);
  for (let i = 0; i < count; i++) {
    assert.equal(data.readUInt32LE(at), 0x02014b50, 'download central entry is valid');
    const method = data.readUInt16LE(at + 10), size = data.readUInt32LE(at + 20);
    const length = data.readUInt32LE(at + 24), n = data.readUInt16LE(at + 28);
    const name = data.subarray(at + 46, at + 46 + n).toString('utf8');
    const local = data.readUInt32LE(at + 42);
    assert.equal(data.readUInt32LE(local), 0x04034b50, 'download local entry is valid');
    const body = local + 30 + data.readUInt16LE(local + 26) + data.readUInt16LE(local + 28);
    const packed = data.subarray(body, body + size);
    assert.ok(method === 0 || method === 8, 'download ZIP uses a supported independent decoder');
    const unpacked = method === 0 ? packed : inflateRawSync(packed);
    assert.equal(unpacked.length, length, 'download entry length matches its directory');
    assert.equal(crc32(unpacked), data.readUInt32LE(at + 16), 'download entry CRC matches');
    entries.push({ name, bytes: Buffer.from(unpacked) });
    at += 46 + n + data.readUInt16LE(at + 30) + data.readUInt16LE(at + 32);
  }
  assert.equal(at, end, 'download directory has no hidden trailing entries');
  return entries;
}
function fingerprints(files) {
  // The engine's original block suffixes are uppercase; transfer paths are ASCII lowercase.
  // Fold only case, never slashes, traversal, prefixes, or any bytes in a save file.
  const records = files.map(f => ({ path: (f.path ?? f.name).toLowerCase(), sha256: sha(Buffer.from(f.bytes)) }));
  assert.equal(new Set(records.map(f => f.path)).size, files.length, 'file comparison rejects duplicate case aliases');
  return records.sort((a, b) => a.path.localeCompare(b.path));
}
const equalFiles = (actual, expected, label) => h.assertSame(fingerprints(actual), fingerprints(expected), label);

async function check(name, body) {
  currentCase = name;
  const began = Date.now();
  const evidence = await body();
  results.push({ case: name, status: 'pass', durationMs: Date.now() - began, ...evidence });
  await writeFile(join(out, 'results.json'), JSON.stringify({ run, mode: 'QA game-first entry; normal saves', results }, null, 2));
  console.log(`PASS ${name}`);
}

async function opened({ width = 1024, height = 900, touch = false, layout = 'original' } = {}) {
  const browser = touch ? { newContext: options => env.browser.newContext({ ...options, hasTouch: true }) } : env.browser;
  const { page, context } = await h.openPage({ ...env, browser }, { layout });
  await page.setViewportSize({ width, height });
  const errors = [];
  page.on('pageerror', error => errors.push(error.name));
  await context.addInitScript(() => { window.__qaDocument = crypto.randomUUID(); });
  // Scheduling/failure hooks only: all controls and real codec/storage remain in the app.
  await context.route('**/app.js', async route => {
    const response = await route.fetch();
    let body = await response.text();
    const stageTail = /(cancelAction: `save-transfer-cancel:\$\{token\}`,\s*lockOnAccept: false,\s*keepPending: true,\s*\}\);)/;
    assert.ok(stageTail.test(body), 'preview scheduling hook targets the transfer stage only');
    body = body.replace(stageTail, "$1\n      await window.__gate('transfer-stage-tail');");
    const reload = 'try { location.reload(); }';
    assert.ok(body.includes(reload), 'reload refusal hook targets the committed-transfer navigation');
    body = body.replace(reload, "try { if (window.__qaRefuseReload) throw new DOMException('synthetic navigation refusal', 'SecurityError'); location.reload(); }");
    await route.fulfill({ response, body });
  });
  // Hold/inject only the save-transfer boundary. All actual storage code still runs normally.
  await context.route(/\/store\.js\?real$/, async route => {
    const response = await route.fetch();
    await route.fulfill({ response, body: (await response.text()) + `
      const qaSnapshot = readSaveSnapshot;
      readSaveSnapshot = async (...args) => {
        if (window.__fail.has('transfer-read')) throw new DOMException('synthetic snapshot refusal', 'SecurityError');
        return qaSnapshot(...args);
      };
      const qaReplace = replaceSavesWithBackup;
      replaceSavesWithBackup = async (...args) => {
        window.__qaApplyCalls = (window.__qaApplyCalls ?? 0) + 1;
        await window.__gate('transfer-apply');
        if (window.__fail.has('transfer-apply')) throw new DOMException('synthetic storage refusal', 'QuotaExceededError');
        return qaReplace(...args);
      };
      const qaBackups = listSaveBackups;
      listSaveBackups = async (...args) => {
        const held = Boolean(window.__qaHoldNextBackupRead);
        if (held) {
          window.__qaHoldNextBackupRead = false;
          window.__qaBackupReadHeld = (window.__qaBackupReadHeld ?? 0) + 1;
          await window.__gate('transfer-backups');
        }
        const result = await qaBackups(...args);
        if (held) window.__qaBackupReadReleased = (window.__qaBackupReadReleased ?? 0) + 1;
        return result;
      };
    ` });
  });
  // No external request or model/pack fetch is part of transfer QA.
  await context.route('**/*', route => {
    const url = new URL(route.request().url());
    if (!['127.0.0.1', 'localhost'].includes(url.hostname) || /^\/(packs|models)\//.test(url.pathname)) return route.abort();
    return route.fallback();
  });
  await h.load(page, env);
  return { page, context, errors, touch };
}

async function startWithDisks(page) {
  const chooser = page.waitForEvent('filechooser');
  await page.locator('label[for="picker"]').click();
  await (await chooser).setFiles(h.HFES);
  await h.waitSettled(page);
  await page.waitForFunction(() => document.body.dataset.state === 'running');
  await h.waitPrompt(page);
  assert.equal((await h.state(page)).program, 'game', 'QA entry reaches the actual game runtime');
}
async function normalSave(page, steps, differentFrom = null) {
  await h.closeSettings(page);
  await page.focus('#screen');
  for (let i = 0; i < steps; i++) await h.stepOnce(page);
  for (let i = 0; i < 6 && (await h.state(page)).leader === differentFrom; i++) await h.stepOnce(page);
  if (differentFrom !== null) assert.notEqual((await h.state(page)).leader, differentFrom, 'second normal save has a distinct position');
  await h.waitPrompt(page);
  const prior = await page.evaluate(() => __shownGame.save_generation());
  await h.press(page, 'Control+s', 30);
  await h.press(page, 'y', 60);
  await page.waitForFunction(n => Number(document.body.dataset.saved) > n, prior, { timeout: 60_000 });
  await h.waitSettled(page); await h.waitPrompt(page);
  const state = await h.state(page), files = await h.savedFiles(page);
  assert.ok(files.some(f => f.path === 'savegame/objlist') && files.length > 1, 'normal save commits objlist and changed blocks');
  return { files, state };
}

async function visibleItem(page, id, { files = false, keyboard = false } = {}) {
  await h.openSettings(page);
  if (files) await activate(page, 'nav-files', { keyboard });
  const seen = new Set();
  for (let i = 0; i < 20; i++) {
    const state = await page.evaluate(id => ({ page: u6Settings.page, item: u6Settings.items.find(i => i.id === id), next: u6Settings.items.find(i => i.id === 'next' && !i.disabled) }), id);
    if (state.item) return state.item;
    if (seen.has(state.page) || !state.next) break;
    seen.add(state.page); await activate(page, 'next', { keyboard });
  }
  assert.fail(`canvas item ${id} is reachable`);
}
async function activate(page, id, { files = false, keyboard = false, touch = false } = {}) {
  const item = await visibleItem(page, id, { files, keyboard });
  assert.ok(!item.disabled, `canvas item ${id} is enabled`);
  if (keyboard) {
    await page.focus('#screen');
    for (let i = 0; i < 40 && await page.evaluate(() => u6Settings.focused) !== id; i++) await page.keyboard.press('Tab');
    assert.equal(await page.evaluate(() => u6Settings.focused), id, 'keyboard reaches requested transfer action');
    await page.keyboard.press('Enter');
  } else {
    const [x, y, w, ht] = item.rect;
    if (touch) await page.touchscreen.tap(x + w / 2, y + ht / 2);
    else await page.mouse.click(x + w / 2, y + ht / 2);
  }
}
async function selectZip(page, bytes, { name = 'qa-save.zip', pregame = false, keyboard = false, touch = false } = {}) {
  const choice = page.waitForEvent('filechooser');
  if (pregame) {
    if (keyboard) { await page.focus('#save-import-start'); await page.keyboard.press('Enter'); }
    else if (touch) await page.locator('#save-import-start').tap();
    else await page.click('#save-import-start');
  } else await activate(page, 'save-import', { files: true, keyboard, touch });
  await (await choice).setFiles({ name, mimeType: 'application/zip', buffer: bytes });
}
const transferState = page => page.evaluate(() => document.body.dataset.saveTransfer);
const awaitTransfer = (page, state) => page.waitForFunction(state => document.body.dataset.saveTransfer === state, state, { timeout: 60_000 });
async function snapshot(page) {
  return page.evaluate(async () => {
    const store = await import('/store.js?real');
    const state = await store.readSaveSnapshot();
    return { files: state.files.map(f => ({ path: f.path, bytes: Array.from(f.bytes) })), epoch: state.epoch, backups: await store.listSaveBackups() };
  });
}
async function anchor(page) {
  await page.evaluate(() => {
    window.__qaGameAnchor = window.__shownGame;
    window.__qaPresentTrace = [];
    const game = window.__shownGame;
    if (game && !game.__qaPresentObserved) {
      game.__qaPresentObserved = true;
      const present = game.present;
      game.present = function (...args) {
        const before = { ticks: __ticks, clock: __clock, sent: __sent.length, generation: this.save_generation(), frameVersion: this.frame_version() };
        const stack = new Error().stack.split('\n').filter(line => line.includes('/app.js:'));
        const result = present.apply(this, args);
        const after = { ticks: __ticks, clock: __clock, sent: __sent.length, generation: this.save_generation(), frameVersion: this.frame_version() };
        window.__qaPresentTrace.push({ menuOpen: Boolean(window.u6Settings?.open), before, after, stack });
        return result;
      };
    }
  });
  assert.equal(await page.evaluate(() => Boolean(window.u6Settings?.open)), true, 'live-state observation starts with settings open');
  const opening = await observed(page);
  // Opening the canvas menu resizes its box. Let ResizeObserver deliver the view-only
  // applyHd/present before measuring the transfer, without permitting game progress here.
  await h.waitFrames(page, 2);
  const settled = await observed(page);
  const { presents: openingPresents, ...openingGame } = opening;
  const { presents: settledPresents, ...settledGame } = settled;
  await writeFile(join(out, `${currentCase}-anchor.json`), JSON.stringify({
    presents: { before: openingPresents, after: settledPresents, delta: settledPresents - openingPresents },
    gameStateUnchanged: JSON.stringify(openingGame) === JSON.stringify(settledGame),
    presentTrace: await page.evaluate(() => window.__qaPresentTrace ?? []),
  }, null, 2));
  h.assertSame(settledGame, openingGame, 'menu layout settling cannot advance or replace the game');
  await page.evaluate(() => { window.__qaPresentTrace = []; });
  return settled;
}
const observed = page => page.evaluate(() => ({ document: __qaDocument, ticks: __ticks, clock: __clock,
  starts: __starts.length, presents: __presents, sameGame: __shownGame === __qaGameAnchor,
  generation: __shownGame?.save_generation(), keys: __shownGame?.keys_pending(),
  leader: document.querySelector('#screen').dataset.leader, sent: __sent.length, buttons: __buttons.length }));
async function unchanged(page, before, label) {
  await h.waitFrames(page, 12);
  const after = await observed(page);
  const counters = Object.fromEntries(['ticks', 'clock', 'starts', 'presents', 'generation', 'keys', 'sent', 'buttons']
    .map(key => [key, { before: before[key], after: after[key], delta: after[key] - before[key] }]));
  await writeFile(join(out, `${currentCase}-observed.json`), JSON.stringify({ label, counters,
    positionChanged: before.leader !== after.leader, documentChanged: before.document !== after.document,
    sameGame: after.sameGame, presentTrace: await page.evaluate(() => window.__qaPresentTrace ?? []),
  }, null, 2));
  h.assertSame(after, before, label);
}
async function download(page, id = 'save-export', options = {}) {
  if (id === 'backup-export') {
    // Menu opening starts an asynchronous backup-list read. Wait for its real UI result;
    // a backup present in IndexedDB alone does not mean the selected button is ready yet.
    await visibleItem(page, id, { files: true, ...options });
    await page.waitForFunction(id => {
      const item = window.u6Settings?.items.find(item => item.id === id);
      return Boolean(item && !item.disabled);
    }, id, { timeout: 10_000 });
  }
  // Unlike an abandoned waitForEvent promise, this waiter can be detached if activation
  // fails. Resolve errors as values so a later page close cannot mask that original error.
  let finish;
  const event = new Promise(resolve => {
    let settled = false;
    const onDownload = item => finish({ item });
    const onClose = () => finish({ error: new Error('page closed before its download event') });
    const timer = setTimeout(() => finish({ error: new Error('browser download event timed out') }), 60_000);
    finish = result => {
      if (settled) return;
      settled = true;
      clearTimeout(timer); page.off('download', onDownload); page.off('close', onClose);
      resolve(result);
    };
    page.on('download', onDownload); page.on('close', onClose);
  });
  try {
    await activate(page, id, { files: true, ...options });
    const { item, error } = await event;
    if (error) throw error;
    const path = join(out, `${currentCase}-${id}-${++downloadSequence}.zip`);
    await item.saveAs(path);
    assert.equal(await item.failure(), null, 'the browser download completes');
    await h.waitSettled(page);
    return readFile(path);
  } finally {
    finish({ error: new Error('download wait cancelled') });
  }
}
function inspectExport(bytes, files) {
  const entries = unzip(bytes), names = entries.map(e => e.name);
  assert.equal(new Set(names).size, entries.length, 'export has unique ZIP entries');
  assert.deepEqual(names.filter(n => !n.startsWith('savegame/')), [manifestName], 'export contains only manifest and save files');
  equalFiles(entries.filter(e => e.name !== manifestName), files, 'download matches committed file hashes exactly');
  const manifest = JSON.parse(entries.find(e => e.name === manifestName).bytes.toString('utf8'));
  assert.equal(manifest.format, 'ultima6-save', 'download manifest declares save format');
  assert.equal(manifest.version, 1, 'download manifest declares supported version');
  assert.equal(manifest.game, 'ultima6-amiga', 'download manifest identifies game edition');
  assert.equal(manifest.files.length, files.length, 'manifest lists exactly the saved files');
  for (const entry of manifest.files) {
    assert.equal(entry.path, entry.path.toLowerCase(), 'manifest uses canonical lowercase paths');
    const file = files.find(file => file.path.toLowerCase() === entry.path);
    assert.ok(file, 'each manifest path names a committed save file');
    assert.equal(entry.size, file.bytes.length, 'manifest file size matches committed bytes');
    assert.equal(entry.sha256, sha(Buffer.from(file.bytes)), 'manifest SHA-256 is independently verified');
  }
  return { entries, manifest };
}
async function screenshot(page, name) {
  await page.screenshot({ path: join(out, `${name}.png`), fullPage: true });
  await page.screenshot({ path: join(out, `${name}-viewport.png`) });
}
async function checkPreview(page, name) {
  await awaitTransfer(page, 'preview');
  const items = await page.evaluate(() => u6Settings.items);
  const accept = items.find(i => i.id === 'confirm-accept'), cancel = items.find(i => i.id === 'confirm-cancel');
  assert.ok(accept && cancel, 'preview has explicit accept and cancel actions');
  assert.match(accept.label, /import|가져오기/i, 'import confirmation identifies import');
  assert.doesNotMatch(accept.label, /delete|삭제/i, 'import confirmation never says delete');
  const accessible = await page.locator('#game-settings-accessibility').textContent();
  assert.match(accessible, /unsaved|저장하지 않은/i, 'assistive technology receives the unsaved-progress warning');
  assert.match(accessible, /backup|백업/i, 'assistive technology receives the backup promise');
  for (const item of items) assert.ok(item.rect[2] >= 44 && item.rect[3] >= 44, 'preview targets are at least 44px');
  const canvas = await page.locator('#screen').boundingBox();
  for (const item of items) {
    const [x, y, w, ht] = item.rect;
    assert.ok(x >= canvas.x - 1 && y >= canvas.y - 1 && x + w <= canvas.x + canvas.width + 1 && y + ht <= canvas.y + canvas.height + 1, 'preview controls stay within the canvas');
  }
  const [ax, ay, aw, ah] = accept.rect, [cx, cy, cw, ch] = cancel.rect;
  assert.ok(ax + aw <= cx || cx + cw <= ax || ay + ah <= cy || cy + ch <= ay, 'confirmation controls do not overlap in horizontal or stacked layouts');
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, 'preview has no horizontal page overflow');
  await screenshot(page, name);
}

try {
  await mkdir(out, { recursive: true });
  assert.ok(h.haveHfes(), 'configured owned disk images are required; no substitute save fixture');
  env = await h.setup();
  const a = await opened();
  let saveA, zipA, parsedA;
  await check('empty-export-no-download', async () => {
    let downloads = 0;
    const onDownload = () => { downloads++; };
    a.page.on('download', onDownload);
    await activate(a.page, 'save-export', { files: true });
    await awaitTransfer(a.page, 'error'); await h.waitSettled(a.page);
    const notice = await a.page.evaluate(() => u6Settings.notice);
    assert.match(notice, /no (?:stored )?saved game|저장.*없/i, 'empty export explains that there is no committed save');
    assert.match(notice, /save[^\n]*first|먼저[^\n]*저장/i, 'empty export explains that normal Save is needed');
    assert.equal(downloads, 0, 'empty export does not download an invalid archive');
    assert.equal((await h.savedFiles(a.page)).length, 0, 'empty export does not invent a save');
    a.page.off('download', onDownload); await h.closeSettings(a.page);
    return { noDownload: true, noSaveCreated: true, readableError: true };
  });
  await check('normal-save-export', async () => {
    await startWithDisks(a.page);
    await h.chooseSetting(a.page, 'memory', 'on', { close: true });
    saveA = await normalSave(a.page, 1);
    assert.ok(saveA.files.some(file => file.path.toLowerCase() === 'savegame/u6sight'), 'normal memory-enabled Save includes its real sight sidecar');
    await h.stepOnce(a.page);
    assert.notEqual((await h.state(a.page)).leader, saveA.state.leader, 'source continues to an unsaved location');
    await h.openSettings(a.page);
    const before = await anchor(a.page);
    zipA = await download(a.page);
    parsedA = inspectExport(zipA, saveA.files);
    await unchanged(a.page, before, 'export never advances or replaces live game');
    equalFiles(await h.savedFiles(a.page), saveA.files, 'export never writes a new save');
    await screenshot(a.page, 'export-from-live-unsaved-game');
    await writeFile(join(out, 'source-fingerprints.json'), JSON.stringify(fingerprints(saveA.files), null, 2));
    return { normalSave: true, rememberedSightEnabledThroughUI: true, realSightSidecar: true, actualDownload: true, committedSnapshotOnly: true, liveStateStable: true, onlySaveEntries: true };
  });
  await check('pending-normal-save-export-drains', async () => {
    await h.closeSettings(a.page); await a.page.focus('#screen'); await h.waitPrompt(a.page);
    const beforeFiles = await h.savedFiles(a.page);
    const generation = await a.page.evaluate(() => __shownGame.save_generation());
    await a.page.evaluate(() => { window.__qaSaveLogStart = __log.length; __hold('saves'); });
    await h.press(a.page, 'Control+s', 30); await h.press(a.page, 'y', 60);
    await a.page.waitForFunction(old => __shownGame.save_generation() > old, generation);
    await a.page.waitForFunction(() => __log.slice(__qaSaveLogStart).some(line => line.startsWith('saves:store:start:')));
    equalFiles(await h.savedFiles(a.page), beforeFiles, 'pending normal Save has not committed yet');
    await h.openSettings(a.page); const before = await anchor(a.page);
    let downloads = 0; const onDownload = () => { downloads++; }; a.page.on('download', onDownload);
    const exported = download(a.page);
    await a.page.waitForFunction(() => Number(document.body.dataset.ops) >= 2);
    await h.waitFrames(a.page, 6);
    assert.equal(downloads, 0, 'export waits behind the pending normal Save');
    await a.page.evaluate(() => __release('saves'));
    const bytes = await exported; a.page.off('download', onDownload);
    const committed = await h.savedFiles(a.page);
    assert.notEqual(sha(Buffer.concat(committed.map(f => Buffer.from(f.bytes)))), sha(Buffer.concat(beforeFiles.map(f => Buffer.from(f.bytes)))), 'the drained Save actually changes stored content');
    inspectExport(bytes, committed);
    await unchanged(a.page, before, 'draining persistence does not advance the paused live game');
    return { pendingNormalSave: true, queuedExportWaited: true, finalCommittedHashesVerified: true };
  });
  const b = await opened();
  let saveB, zipB, staleTab;
  await check('distinct-target-save', async () => {
    await startWithDisks(b.page);
    saveB = await normalSave(b.page, 3, saveA.state.leader);
    await h.openSettings(b.page); zipB = await download(b.page);
    inspectExport(zipB, saveB.files);
    assert.notEqual(sha(zipA), sha(zipB), 'source and target exports are distinct');
    return { normalSave: true, separateBrowser: true, distinctPosition: true };
  });
  await check('export-storage-read-failure', async () => {
    const kept = await snapshot(b.page), before = await anchor(b.page);
    let downloads = 0; const onDownload = () => { downloads++; }; b.page.on('download', onDownload);
    await b.page.evaluate(() => __fail.add('transfer-read'));
    await activate(b.page, 'save-export', { files: true });
    await awaitTransfer(b.page, 'error'); await h.waitSettled(b.page);
    assert.equal(downloads, 0, 'unreadable storage never yields an empty or partial export');
    assert.match(await b.page.evaluate(() => u6Settings.notice), /storage|읽|보관|저장 공간/i, 'read refusal is explained as a storage problem');
    await b.page.evaluate(() => __fail.delete('transfer-read')); b.page.off('download', onDownload);
    const after = await snapshot(b.page);
    equalFiles(after.files, kept.files, 'failed export read leaves save bytes unchanged');
    assert.equal(after.epoch, kept.epoch, 'failed export read leaves save ownership unchanged');
    h.assertSame(after.backups, kept.backups, 'failed export read leaves backup history unchanged');
    await unchanged(b.page, before, 'failed export read preserves the paused game');
    return { snapshotFailureInjected: true, noDownload: true, readableStorageError: true, saveAndPlayPreserved: true };
  });

  // Corrupt structure independently, preserving real save bytes only in this private run.
  const mutatedManifest = change => parsedA.entries.map(e => e.name === manifestName
    ? { name: e.name, bytes: Buffer.from(JSON.stringify(change(structuredClone(parsedA.manifest)))) } : e);
  const invalid = [
    ['not-zip', Buffer.from('synthetic not a ZIP')],
    ['version', zip(mutatedManifest(m => ({ ...m, version: 999 })))],
    ['hash', zip(mutatedManifest(m => ({ ...m, files: m.files.map((file, i) => i === 0
      ? { ...file, sha256: file.sha256 === '0'.repeat(64) ? '1'.repeat(64) : '0'.repeat(64) } : file) })))],
    ['missing-objlist', zip(parsedA.entries.filter(e => e.name !== 'savegame/objlist'))],
    ['duplicate-case', zip([...parsedA.entries, { name: 'SAVEGAME/OBJLIST', bytes: parsedA.entries.find(e => e.name === 'savegame/objlist').bytes }])],
    ['path-escape', zip([...parsedA.entries, { name: '../savegame/objlist', bytes: Buffer.from([0]) }])],
    ['unknown-game-file', zip([...parsedA.entries, { name: 'map', bytes: Buffer.from([0]) }])],
    ['entry-bound', zip(Array.from({ length: 73 }, (_, i) => ({ name: `entry${i}`, bytes: Buffer.alloc(0) })))],
  ];
  for (const [label, bytes] of invalid) await check(`reject-${label}`, async () => {
    const kept = await snapshot(b.page), before = await anchor(b.page);
    await selectZip(b.page, bytes); await awaitTransfer(b.page, 'error'); await h.waitSettled(b.page);
    const after = await snapshot(b.page);
    equalFiles(after.files, kept.files, 'invalid transfer leaves committed save unchanged');
    assert.equal(after.epoch, kept.epoch, 'invalid transfer leaves attached save epoch unchanged');
    h.assertSame(after.backups, kept.backups, 'invalid transfer creates no backup');
    await unchanged(b.page, before, 'invalid transfer never enters game input or starts a new session');
    assert.ok((await b.page.evaluate(() => u6Settings.notice)).length > 10, 'invalid transfer has readable feedback');
    if (label === 'not-zip') await screenshot(b.page, 'invalid-transfer-error');
    return { currentSavePreserved: true, backupsPreserved: true, liveStateStable: true };
  });
  await check('reject-file-bound-before-read', async () => {
    const before = await snapshot(b.page);
    await b.page.evaluate(() => { window.__bigName = 'oversized-save.zip'; });
    await selectZip(b.page, Buffer.from('synthetic bounded test'), { name: 'oversized-save.zip' });
    await awaitTransfer(b.page, 'error'); await h.waitSettled(b.page);
    assert.equal((await h.log(b.page)).includes('read:start:oversized-save.zip'), false, 'oversized archive is rejected before reading');
    await b.page.evaluate(() => { window.__bigName = null; });
    const after = await snapshot(b.page);
    equalFiles(after.files, before.files, 'oversized input leaves B intact');
    assert.equal(after.epoch, before.epoch, 'oversized input leaves save epoch unchanged');
    h.assertSame(after.backups, before.backups, 'oversized input creates no backup');
    return { syntheticClaimedSize: true, rejectedBeforeRead: true, storagePreserved: true };
  });

  await check('cancel-preview-and-reading', async () => {
    for (const method of ['button', 'escape', 'close']) {
      const kept = await snapshot(b.page);
      await selectZip(b.page, zipA); await checkPreview(b.page, `preview-${method}`);
      const before = await anchor(b.page);
      if (method === 'button') await activate(b.page, 'confirm-cancel');
      else if (method === 'escape') await b.page.keyboard.press('Escape');
      else await activate(b.page, 'close');
      await h.waitSettled(b.page);
      const after = await snapshot(b.page);
      equalFiles(after.files, kept.files, 'cancel preview leaves target save intact');
      assert.equal(after.epoch, kept.epoch, 'cancel preview leaves save epoch unchanged');
      h.assertSame(after.backups, kept.backups, 'cancel preview leaves backups unchanged');
      if (method !== 'close') await unchanged(b.page, before, 'cancel preview while menu stays open preserves game');
      else assert.equal(await b.page.evaluate(() => document.activeElement.id), 'screen', 'closing returns focus to game canvas');
      await h.openSettings(b.page);
    }
    await b.page.evaluate(() => __hold('read'));
    await selectZip(b.page, zipA, { name: 'held-cancel.zip' });
    await h.waitLog(b.page, 'read:start:held-cancel.zip');
    await b.page.keyboard.press('Escape');
    await b.page.evaluate(() => __release('read')); await h.waitSettled(b.page);
    assert.equal(await b.page.evaluate(() => u6Settings.open), false, 'cancelled read does not reopen preview');
    equalFiles(await h.savedFiles(b.page), saveB.files, 'cancelled delayed read leaves B intact');
    await h.openSettings(b.page);
    return { previewCancelMethods: 3, delayedReadCancelled: true, focusReturned: true };
  });
  await check('latest-selection-wins', async () => {
    await b.page.evaluate(() => __hold('read'));
    await selectZip(b.page, zipA, { name: 'first-held.zip' });
    await h.waitLog(b.page, 'read:start:first-held.zip');
    await selectZip(b.page, Buffer.from('synthetic invalid replacement'), { name: 'second-invalid.zip' });
    await b.page.evaluate(() => __release('read'));
    await awaitTransfer(b.page, 'error'); await h.waitSettled(b.page);
    assert.equal(await b.page.evaluate(() => u6Settings.items.some(i => i.id === 'confirm-accept')), false, 'stale first read cannot resurrect its confirmation');
    equalFiles(await h.savedFiles(b.page), saveB.files, 'changing file selection never applies stale A');
    return { stalePreviewBlocked: true, BPreserved: true };
  });
  await check('queued-confirm-cancelled-before-transaction', async () => {
    const kept = await snapshot(b.page), appliesBefore = await b.page.evaluate(() => window.__qaApplyCalls ?? 0);
    await b.page.evaluate(() => __hold('transfer-stage-tail'));
    await selectZip(b.page, zipA); await awaitTransfer(b.page, 'preview');
    await activate(b.page, 'confirm-accept');
    await b.page.waitForFunction(() => Number(document.body.dataset.ops) >= 2);
    assert.equal(await b.page.evaluate(() => window.__qaApplyCalls ?? 0), appliesBefore, 'confirmation is queued before any transaction');
    await b.page.keyboard.press('Escape');
    await b.page.evaluate(() => __release('transfer-stage-tail')); await h.waitSettled(b.page);
    assert.equal(await b.page.evaluate(() => window.__qaApplyCalls ?? 0), appliesBefore, 'cancelled queued confirmation never calls replacement');
    const after = await snapshot(b.page);
    equalFiles(after.files, kept.files, 'queued cancellation preserves target B');
    assert.equal(after.epoch, kept.epoch, 'queued cancellation preserves target save epoch');
    h.assertSame(after.backups, kept.backups, 'queued cancellation creates no backup');
    return { previewOperationHeld: true, queuedConfirmCancelled: true, replacementCalls: 0 };
  });
  await check('late-confirm-cannot-unlock-new-commit', async () => {
    const kept = await snapshot(b.page);
    // The old same-save confirmation finishes its transaction, then waits while refreshing
    // backup descriptions. Escape cancels that modal before its original promise resolves.
    await selectZip(b.page, zipB); await awaitTransfer(b.page, 'preview');
    await b.page.evaluate(() => { __hold('transfer-backups'); window.__qaHoldNextBackupRead = true; });
    await activate(b.page, 'confirm-accept');
    await b.page.waitForFunction(() => window.__qaBackupReadHeld === 1);
    await b.page.keyboard.press('Escape');
    await selectZip(b.page, zipA); await awaitTransfer(b.page, 'preview');
    await b.page.evaluate(() => { __hold('transfer-apply'); __fail.add('transfer-apply'); });
    await activate(b.page, 'confirm-accept'); await awaitTransfer(b.page, 'committing');
    assert.equal(await b.page.evaluate(() => u6Settings.busy), true, 'new transaction owns the menu lock');
    await b.page.evaluate(() => __release('transfer-backups'));
    await b.page.waitForFunction(() => window.__qaBackupReadReleased === 1);
    await h.waitFrames(b.page, 6);
    assert.equal(await b.page.evaluate(() => u6Settings.busy), true, 'old cancelled confirmation cannot release the new lock');
    await b.page.keyboard.press('Escape');
    assert.equal(await b.page.evaluate(() => u6Settings.open), true, 'Escape cannot close a committing replacement');
    assert.equal(await transferState(b.page), 'committing', 'late old promise cannot change commit state');
    await screenshot(b.page, 'late-confirm-current-lock');
    // Fail this synthetic held call before the real store to keep B for the normal roundtrip.
    await b.page.evaluate(() => __release('transfer-apply'));
    await awaitTransfer(b.page, 'error'); await h.waitSettled(b.page);
    await b.page.evaluate(() => __fail.delete('transfer-apply'));
    const after = await snapshot(b.page);
    equalFiles(after.files, kept.files, 'failed race fixture leaves target save unchanged');
    assert.equal(after.epoch, kept.epoch, 'failed race fixture leaves save epoch unchanged');
    h.assertSame(after.backups, kept.backups, 'failed race fixture creates no backup');
    return { oldPromiseDelayed: true, newLockPreserved: true, escapeBlockedDuringCommit: true };
  });
  await check('readable-storage-error', async () => {
    await selectZip(b.page, zipA); await awaitTransfer(b.page, 'preview');
    const before = await snapshot(b.page);
    await b.page.evaluate(() => __fail.add('transfer-apply'));
    await activate(b.page, 'confirm-accept'); await awaitTransfer(b.page, 'error'); await h.waitSettled(b.page);
    equalFiles((await snapshot(b.page)).files, before.files, 'storage refusal leaves B intact');
    assert.equal((await snapshot(b.page)).epoch, before.epoch, 'storage refusal leaves save epoch unchanged');
    h.assertSame((await snapshot(b.page)).backups, before.backups, 'storage refusal creates no backup');
    assert.match(await b.page.evaluate(() => u6Settings.notice), /storage|저장/i, 'storage refusal is readable');
    await screenshot(b.page, 'storage-error');
    await b.page.evaluate(() => __fail.delete('transfer-apply'));
    return { boundaryFailureInjected: true, readableError: true, atomicStorageUnitCoverageSeparate: true };
  });
  await check('import-backup-resume', async () => {
    // A real second tab shares this context's IndexedDB and boots B before A is imported.
    staleTab = await b.context.newPage();
    staleTab.on('pageerror', error => b.errors.push(error.name));
    await staleTab.goto(env.server.url); await h.waitSettled(staleTab); await h.waitPrompt(staleTab);
    await h.openSettings(staleTab); await b.page.bringToFront();
    await selectZip(b.page, zipA, { keyboard: true }); await checkPreview(b.page, 'desktop-import-preview');
    const documentBefore = await b.page.evaluate(() => __qaDocument);
    // A second click on the same hit target before the asynchronous commit finishes cannot
    // create a second transaction. Hold the storage boundary, not the game or its state.
    const appliesBefore = await b.page.evaluate(() => window.__qaApplyCalls ?? 0);
    await b.page.evaluate(() => __hold('transfer-apply'));
    const accept = await visibleItem(b.page, 'confirm-accept');
    await activate(b.page, 'confirm-accept', { keyboard: true });
    await awaitTransfer(b.page, 'committing');
    await b.page.mouse.click(accept.rect[0] + accept.rect[2] / 2, accept.rect[1] + accept.rect[3] / 2);
    assert.equal(await b.page.evaluate(() => __qaApplyCalls), appliesBefore + 1, 'double activation invokes storage once');
    await b.page.evaluate(() => __release('transfer-apply'));
    await b.page.waitForFunction(old => __qaDocument !== old, documentBefore, { timeout: 60_000 });
    await h.waitSettled(b.page); await h.waitPrompt(b.page);
    equalFiles(await h.savedFiles(b.page), saveA.files, 'target stored save equals imported A exactly');
    h.assertSame((await h.state(b.page)).leader, saveA.state.leader, 'automatic restart resumes A saved location');
    h.assertSame((await h.state(b.page)).name, saveA.state.name, 'resumed leader identity matches A');
    assert.equal((await h.state(b.page)).program, 'game', 'imported save runs in actual game');
    await screenshot(b.page, 'resumed-game-roster');
    const after = await snapshot(b.page);
    assert.equal(after.backups.length, 1, 'replacing B creates exactly one backup');
    await h.openSettings(b.page);
    const backup = await download(b.page, 'backup-export');
    inspectExport(backup, saveB.files);
    return { automaticReload: true, exactSaveHashes: true, resumedPositionAndIdentity: true, backupCount: 1, actualBackupDownload: true, doubleApplyBlocked: true };
  });
  await check('stale-tab-normal-save-refused', async () => {
    const kept = await snapshot(b.page);
    await staleTab.bringToFront(); await h.closeSettings(staleTab); await staleTab.focus('#screen');
    await h.stepOnce(staleTab);
    const savedBefore = (await h.state(staleTab)).saved;
    const generation = await staleTab.evaluate(() => __shownGame.save_generation());
    await h.press(staleTab, 'Control+s', 30); await h.press(staleTab, 'y', 60);
    await staleTab.waitForFunction(old => __shownGame.save_generation() > old, generation);
    await staleTab.waitForFunction(() => document.body.dataset.saveConflict === 'true');
    await h.waitSettled(staleTab);
    assert.equal((await h.state(staleTab)).saved, savedBefore, 'stale normal Save is not reported as committed');
    assert.match(await staleTab.locator('#status').textContent(), /reload|새로고침/i, 'stale tab explains the required reload');
    assert.equal(await staleTab.locator('#restart').isVisible(), true, 'stale tab offers a visible recovery action');
    const after = await snapshot(b.page);
    equalFiles(after.files, kept.files, 'old tab cannot overwrite imported A with its later normal Save');
    assert.equal(after.epoch, kept.epoch, 'old tab cannot advance the imported save epoch');
    h.assertSame(after.backups, kept.backups, 'old tab cannot modify import backup history');
    await screenshot(staleTab, 'stale-tab-save-conflict');
    await staleTab.close(); await b.page.bringToFront();
    return { sharedContextTwoTabs: true, realNormalSaveAttempt: true, importedSavePreserved: true, noFalseCommit: true, reloadGuidance: true };
  });
  await check('same-import-noop', async () => {
    const kept = await snapshot(b.page), before = await anchor(b.page);
    await selectZip(b.page, zipA); await awaitTransfer(b.page, 'preview');
    await activate(b.page, 'confirm-accept'); await awaitTransfer(b.page, 'same'); await h.waitSettled(b.page);
    await unchanged(b.page, before, 'identical import neither reloads nor advances current game');
    const after = await snapshot(b.page);
    equalFiles(after.files, kept.files, 'identical import leaves files unchanged');
    assert.equal(after.epoch, kept.epoch, 'identical import leaves save epoch unchanged');
    h.assertSame(after.backups, kept.backups, 'identical import creates no backup');
    return { noReload: true, noExtraBackup: true, liveStateStable: true };
  });
  await check('multiple-backup-selection', async () => {
    const firstBackups = (await snapshot(b.page)).backups;
    await selectZip(b.page, zipB); await awaitTransfer(b.page, 'preview');
    const documentBefore = await b.page.evaluate(() => __qaDocument);
    await activate(b.page, 'confirm-accept');
    await b.page.waitForFunction(old => __qaDocument !== old, documentBefore, { timeout: 60_000 });
    await h.waitSettled(b.page); await h.waitPrompt(b.page);
    const after = await snapshot(b.page);
    assert.equal(after.backups.length, 2, 'a second replacement preserves both backups');
    h.assertSame(after.backups.find(item => item.id === firstBackups[0].id), firstBackups[0], 'older backup metadata is unchanged');
    equalFiles(after.files, saveB.files, 'second import uses B and preserves earlier history');
    await h.openSettings(b.page);
    inspectExport(await download(b.page, 'backup-export'), saveA.files);
    let choice = await visibleItem(b.page, 'backup-choice', { files: true });
    assert.equal(choice.options.length, 2, 'canvas offers both recorded backups');
    const newest = choice.value;
    await activate(b.page, 'backup-choice', { keyboard: true });
    choice = await visibleItem(b.page, 'backup-choice', { files: true });
    assert.notEqual(choice.value, newest, 'keyboard changes the selected backup');
    assert.equal(choice.value, firstBackups[0].id, 'older backup remains selectable');
    inspectExport(await download(b.page, 'backup-export'), saveB.files);
    await screenshot(b.page, 'older-backup-selected');
    return { backupCount: 2, originalMetadataPreserved: true, selectedBackupDownloadVerified: true };
  });
  await check('committed-import-reload-refused', async () => {
    const kept = await snapshot(b.page), documentBefore = await b.page.evaluate(() => __qaDocument);
    await b.page.evaluate(() => { window.__qaRefuseReload = true; });
    await selectZip(b.page, zipA); await awaitTransfer(b.page, 'preview');
    await activate(b.page, 'confirm-accept'); await awaitTransfer(b.page, 'imported'); await h.waitSettled(b.page);
    assert.equal(await b.page.evaluate(() => __qaDocument), documentBefore, 'synthetic navigation failure leaves the current document');
    const after = await snapshot(b.page);
    equalFiles(after.files, saveA.files, 'navigation refusal does not undo successful import');
    assert.equal(after.backups.length, kept.backups.length + 1, 'successful import retains its backup despite navigation refusal');
    assert.match(await b.page.evaluate(() => u6Settings.notice), /imported|가져오기.*완료/i, 'message accurately reports the committed import');
    assert.match(await b.page.evaluate(() => u6Settings.notice), /reload|새로고침/i, 'message explains how to resume');
    await screenshot(b.page, 'committed-import-reload-refused');
    await h.closeSettings(b.page);
    await b.page.locator('#restart').click();
    await b.page.waitForFunction(old => __qaDocument !== old, documentBefore, { timeout: 60_000 });
    await h.waitSettled(b.page); await h.waitPrompt(b.page);
    h.assertSame((await h.state(b.page)).leader, saveA.state.leader, 'visible Restart resumes the successfully imported save');
    equalFiles(await h.savedFiles(b.page), saveA.files, 'Restart preserves the committed imported bytes');
    return { reloadFailureInjected: true, commitReportedTruthfully: true, backupPreserved: true, visibleRestartRecovered: true };
  });

  await check('save-before-originals', async () => {
    const c = await opened();
    assert.equal((await h.savedFiles(c.page)).length, 0, 'third context has no seeded save');
    assert.equal(await h.keptPaths(c.page), null, 'third context has no original data');
    await selectZip(c.page, zipA, { pregame: true, keyboard: true }); await checkPreview(c.page, 'pregame-import-preview');
    const documentBefore = await c.page.evaluate(() => __qaDocument);
    await activate(c.page, 'confirm-accept');
    await c.page.waitForFunction(old => __qaDocument !== old, documentBefore); await h.waitSettled(c.page);
    equalFiles(await h.savedFiles(c.page), saveA.files, 'empty browser keeps A before originals');
    assert.equal(await h.keptPaths(c.page), null, 'save ZIP did not contain or create original files');
    await screenshot(c.page, 'save-kept-needs-originals');
    await startWithDisks(c.page);
    h.assertSame((await h.state(c.page)).leader, saveA.state.leader, 'normal disk import resumes earlier imported A');
    await screenshot(c.page, 'save-first-resumed-game');
    assert.deepEqual(c.errors, [], 'save-first context has no page errors'); await c.context.close();
    return { emptyBrowser: true, saveFirst: true, normalOriginalPicker: true, actualGameResumed: true };
  });

  for (const [label, width, height, touch, layout, language] of [
    ['phone-touch', 390, 844, true, 'hd', 'en'],
    ['landscape-pointer', 844, 390, false, 'wide', 'en'],
    ['desktop-korean', 1024, 900, false, 'original', 'ko'],
    ['phone-korean-touch', 390, 844, true, 'hd', 'ko'],
    ['landscape-korean-pointer', 844, 390, false, 'wide', 'ko'],
  ]) {
    await check(label, async () => {
      const c = await opened({ width, height, touch, layout });
      if (language === 'ko') {
        await h.chooseSetting(c.page, 'language', 'ko', { close: true });
        await c.page.waitForFunction(() => u6Settings.fontState === 'ready');
      }
      await selectZip(c.page, Buffer.from('synthetic invalid mobile ZIP'), { pregame: true, touch });
      await awaitTransfer(c.page, 'error'); await h.waitSettled(c.page);
      assert.ok((await c.page.evaluate(() => u6Settings.notice)).length > 10, 'small-screen rejection has readable feedback');
      await screenshot(c.page, `${label}-error`);
      await selectZip(c.page, zipA, { touch }); await checkPreview(c.page, `${label}-preview`);
      await activate(c.page, 'confirm-cancel', { touch });
      assert.equal(await transferState(c.page), 'idle', 'touch/pointer cancellation dismisses import');
      assert.equal((await h.savedFiles(c.page)).length, 0, 'cancelled mobile import stores nothing');
      await activate(c.page, 'close', { touch });
      assert.equal(await c.page.evaluate(() => document.activeElement.id), 'screen', 'mobile close returns canvas focus');
      assert.deepEqual(c.errors, [], 'mobile context has no page errors'); await c.context.close();
      return { touch, width, height, language, actualCanvasControls: true, targets44: true, noOverflow: true, errorRecovery: true, cancelAndFocus: true };
    });
  }
  assert.deepEqual(a.errors, [], 'source context has no page errors');
  assert.deepEqual(b.errors, [], 'target context has no page errors');
  console.log(`PASS save-transfer UI: ${results.length} cases, 0 skips`);
} catch (error) {
  await mkdir(out, { recursive: true });
  const failure = { run, case: currentCase, error: error.name, message: error.message, stack: error.stack, results };
  if (env) {
    const pages = env.browser.contexts().flatMap(context => context.pages());
    await Promise.allSettled(pages.map((page, i) => page.screenshot({ path: join(out, `failure-page-${i + 1}.png`), fullPage: true })));
  }
  await writeFile(join(out, 'failure.json'), JSON.stringify(failure, null, 2));
  try { await writeFile(join(out, '..', 'first-failure.json'), JSON.stringify(failure, null, 2), { flag: 'wx' }); } catch (e) { if (e.code !== 'EEXIST') throw e; }
  console.error(`FAIL ${currentCase}: ${h.errorName(error)} (details retained privately)`);
  process.exitCode = 1;
} finally { if (env) await env.close(); }

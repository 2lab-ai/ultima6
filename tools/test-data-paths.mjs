import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const root = fileURLToPath(new URL('..', import.meta.url));
const base = { ...process.env };
for (const key of ['U6_DATA_ROOT', 'U6_ARCHIVE', 'U6_ORIGINAL_DIR', 'U6_DATA_DIR', 'U6_GAME_BIN', 'U6_ORACLE_HOME', 'U6_REQUIRE_ARCHIVE']) delete base[key];
function paths(overrides) {
  const child = spawnSync(process.execPath, ['--input-type=module', '-e', `
    const p = await import('./tools/data-paths.mjs');
    console.log(JSON.stringify([p.DATA_ROOT,p.ORIGINAL,p.ARCHIVE,p.UNPACKED,p.GAME_BIN,p.ORACLE_HOME]));
  `], { cwd: root, env: { ...base, ...overrides }, encoding: 'utf8' });
  assert.equal(child.status, 0, 'paths resolve in a fresh environment');
  return JSON.parse(child.stdout);
}
const selected = paths({ U6_DATA_ROOT: '/private/fixture data' });
assert.deepEqual(selected, [
  '/private/fixture data', '/private/fixture data/original', '/private/fixture data/original/ultima-vi.zip',
  '/private/fixture data/unpacked', '/private/fixture data/re/game.bin', '/private/fixture data/oracle',
]);
assert.equal(paths({ U6_DATA_ROOT: '/private/root', U6_ARCHIVE: './owned.zip' })[2], resolve(root, 'owned.zip'));
assert.equal(paths({ HOME: '/private/player' })[0], '/private/player/Games/ultima6');
for (const required of ['0', '1']) {
  const child = spawnSync(process.execPath, ['--input-type=module', '-e', `
    const h = await import('./tools/web/harness.mjs');
    if (h.haveZip()) process.exit(9);
  `], { cwd: root, env: { ...base, U6_ARCHIVE: '/private/nonexistent-u6-fixture.zip', U6_REQUIRE_ARCHIVE: required }, encoding: 'utf8' });
  assert.equal(child.status === 0, required === '0', 'a required archive fails instead of skipping');
}
console.log('Portable input paths: 5 cases passed');

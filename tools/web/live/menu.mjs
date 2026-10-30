// Portable replacement for live30-menu/live37-menu (spec 17 engine menu; spec 21 travel).
// Checks the explicit site URL as a new visitor (ko-KR, 1920x1080, the chooser's defaults). The page's
// own wasm glue is served with read-only observers for engine steps and the game that
// presented last (as the test harness does), so the menu's JSON and the leader's position can be
// read. A right press on bare ground opens the menu drawn in the frame (no page element), its
// "Go here" entry walks the party there, and Escape closes a menu. Prints codes and counts only.
// Usage: node tools/web/live/menu.mjs <url> <private-out-prefix>
import { chromium } from 'playwright';
import { mkdir, realpath } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import * as h from '../harness.mjs';
import { ARCHIVE as ZIP } from '../../data-paths.mjs';

const [BASE, output] = process.argv.slice(2);
if (!BASE || !output || !['http:', 'https:'].includes(new URL(BASE).protocol)) {
  throw new Error('Usage: node tools/web/live/menu.mjs <url> <private-out-prefix>');
}
const OUT = resolve(output);
// Screenshots contain owner-supplied content: require an explicit private output outside git.
process.umask(0o077);
await mkdir(dirname(OUT), { recursive: true, mode: 0o700 });
const outputDir = await realpath(dirname(OUT));
for (let dir = outputDir; ; dir = dirname(dir)) {
  if (existsSync(join(dir, '.git'))) throw new Error('Live screenshots must be outside every git worktree');
  if (dirname(dir) === dir) break;
}
if (!h.haveZip()) throw new Error('The local game archive is required; set U6_ARCHIVE');
const GO_HERE = 0x100;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(page, fn, ms, arg) { const t = Date.now(); while (Date.now() - t < ms) { if (await page.evaluate(fn, arg)) return Date.now() - t; await sleep(200); } return -1; }
let ok = true;
let stage = 'opening candidate';
const check = (name, cond) => { console.log(cond ? 'PASS' : 'FAIL', name); if (!cond) ok = false; };
const browser = await chromium.launch();
try {
  const context = await browser.newContext({ viewport: { width: 1920, height: 1080 }, locale: 'ko-KR' });
  await context.route(/\/pkg(-wide)?\/u6_web\.js$/, async (route) => {
    const response = await route.fetch();
    await route.fulfill({ response, body: (await response.text()) + '\n{globalThis.__ticks=0;const s=Game.prototype.step;Game.prototype.step=function(ms){globalThis.__ticks+=1;return s.call(this,ms);};const p=Game.prototype.present;Game.prototype.present=function(){p.call(this);globalThis.__shownGame=this;};}\n' });
  });
  // Max speed (the page's own stored choice) so the start fight ends sooner; travel keeps its 150 ms pace.
  await context.addInitScript(() => { try { localStorage.setItem('u6.speed', 'max'); } catch {} });
  const page = await context.newPage();
  const errors = [];
  page.on('console', (m) => { if (m.type() === 'error') errors.push('console'); });
  page.on('pageerror', () => errors.push('pageerror'));
  await page.goto(BASE, { waitUntil: 'load' });
  await h.waitSettled(page, 120000);
  const artwork = await h.settingsItem(page, 'pack');
  const hosted = artwork.options.find(option => option.value && option.value !== ':dropped');
  if (!hosted) throw new Error('A hosted artwork pack must be listed');
  await h.chooseSetting(page, 'pack', hosted.value, {close:true});
  check('the hosted HD pack loads', await until(page, () => document.body.dataset.hdPack && document.body.dataset.hdPack !== 'none', 300000) >= 0);
  check('the hosted Korean pack loads', await until(page, () => (document.body.dataset.localePack ?? 'none') !== 'none', 180000) >= 0);
  if (!ok) throw new Error('Required hosted packs did not load');
  stage = 'importing the local archive';
  await page.setInputFiles('#picker', ZIP);
  await until(page, () => document.body.dataset.state === 'running', 300000);
  stage = 'launcher';
  await h.waitProgram(page, 'u', 120000);
  await h.waitVisible(page);
  await page.focus('#screen');
  await h.press(page, 'Escape', 120);
  await sleep(1500);
  stage = 'creating a fixture character';
  await h.press(page, 'c', 120);
  for (const c of 'Fixture') await h.press(page, c, 30);
  await h.press(page, 'Enter', 60); await h.press(page, 'm', 60); await h.press(page, 'p', 30); await h.press(page, 'c', 60);
  for (let i = 0; i < 5; i++) await h.press(page, 'Space', 200);
  for (let q = 0; q < 7; q++) await h.press(page, q % 2 === 0 ? 'a' : 'b', 220);
  await h.press(page, 'Space', 200);
  for (let i = 0; i < 4; i++) await h.press(page, 'Space', 120);
  await h.waitTicks(page, 60);
  await h.press(page, 'Escape', 200);
  stage = 'entering the game';
  await h.waitProgram(page, 'game', 120000);
  await h.waitPrompt(page, 120000);
  await sleep(2000);
  await page.focus('#screen');
  check('the observer sees the game', await page.evaluate(() => !!globalThis.__shownGame));
  check('no page element for the menu', await page.evaluate(() => document.getElementById('context-menu') === null));
  // The map region (game pixels, as the page's pointer uses them); the leader stands at its centre
  // and cells are 16 pixels. Go here needs a reachable cell out of a fight: break off combat (b) and
  // pass turns until a cell 2..4 away that the layout shows is offered enabled.
  const regions = await page.evaluate(() => globalThis.__u6Layout.regions.map((r) => ({ name: r.name, from: [...r.from] })));
  const map = regions.find((r) => /map/i.test(r.name));
  check('the layout names its map region', !!map);
  const [mx0, my0, mw, mh] = map.from;
  const [cx0, cy0] = [Math.floor(mx0 + mw / 2), Math.floor(my0 + mh / 2)];
  const cellAt = (dc, dr) => [cx0 + 16 * dc, cy0 + 16 * dr];
  const cells = [];
  for (let dr = -4; dr <= 4; dr++) for (let dc = -4; dc <= 4; dc++) { const d = Math.max(Math.abs(dc), Math.abs(dr)); if (d >= 2) cells.push([dc, dr, d]); }
  const shown = await page.evaluate((pts) => pts.map(([x, y]) => { const p = globalThis.__u6Layout.toCanvas(x, y); return !!p && Number.isFinite(p[0]); }), cells.map(([c, r]) => cellAt(c, r)));
  const visible = cells.filter((_, i) => shown[i]);
  console.log(`visible candidates: ${visible.length} of ${cells.length}`);
  const viewCell = (c, r) => cellAt(c, r);
  // The start is a fight. The combat check comes before the foe check, so with combat on every
  // cell says "combat": fight 20 turns with combat on (the companions attack), break off (b) and
  // look; a foe still in view turns combat back on (b again) for the next 20.
  const look = async () => {
    const menus = await h.contextMenus(page, visible.map(([c, r]) => viewCell(c, r)));
    const why = {}; let found = null;
    visible.forEach(([c, r, d], i) => {
      const g = menus[i]?.go_here;
      if (!g) return;
      if (g.enabled && !found && menus[i].kind === 'map') found = { c, r, d, cell: menus[i].cell };
      if (!g.enabled) why[g.reason] = (why[g.reason] ?? 0) + 1;
    });
    return [found, why, menus.filter(Boolean).length];
  };
  stage = 'finding a reachable route';
  let pick = null; let reasons = {}; let rounds = 0; let combat = true;
  for (; rounds < 25 && !pick; rounds++) {
    await h.waitPrompt(page, 60000);
    let [found, why, n] = await look();
    if (found) { pick = found; reasons = why; break; }
    if (why.combat) { await h.press(page, 'b', 300); await h.waitPrompt(page, 60000); [found, why, n] = await look(); combat = false; }
    reasons = why;
    console.log(`round ${rounds}: menus ${n}, refusals ${JSON.stringify(why)}`);
    if (found) { pick = found; break; }
    if (why.foe && !combat) { await h.press(page, 'b', 300); combat = true; }
    for (let t = 0; t < 20; t++) await h.passTurn(page);
  }
  console.log(`after ${rounds} rounds: refusals ${JSON.stringify(reasons)}`);
  check('a cell offers Go here enabled', !!pick);
  if (pick) {
    stage = 'opening and checking the Korean menu';
    const start = await page.evaluate(() => [...globalThis.__shownGame.leader_position()]);
    check('the chosen route has at least two cells', pick.d >= 2);
    const [sx, sy] = await h.screenPoint(page, ...viewCell(pick.c, pick.r));
    await page.mouse.move(sx, sy); await sleep(300);
    await page.mouse.click(sx, sy, { button: 'right' });
    check('the right press opens a menu drawn by the engine', await until(page, () => (globalThis.__shownGame.menu_view() ?? '') !== '', 10000) >= 0);
    const view = JSON.parse(await page.evaluate(() => globalThis.__shownGame.menu_view()));
    check('the menu uses the loaded Korean font and labels', view.korean === true);
    console.log(`menu rows ${view.rows.length}; entry codes ${view.rows.filter((r) => r.kind === 'entry').map((r) => r.code.toString(16)).join(',')}`);
    check('still no page element for the menu', await page.evaluate(() => document.getElementById('context-menu') === null));
    await page.locator('#screen-box').screenshot({ path: `${OUT}-menu.png` });
    const row = view.rows.findIndex((r) => r.kind === 'entry' && r.code === GO_HERE && r.enabled);
    check('the menu lists Go here, enabled', row >= 0);
    const asked = await page.evaluate(() => globalThis.__shownGame.go_here_asked());
    const [rx, ry] = await h.screenPoint(page, ...h.rowPoint(view, row));
    await page.mouse.move(rx, ry); await sleep(250);
    // An in-page logger: each animation frame, the page clock when the leader's cell changed.
    await page.evaluate(() => {
      const g = globalThis.__shownGame; const log = []; globalThis.__moves = log;
      let last = [...g.leader_position()].join(',');
      const tick = () => { const p = [...globalThis.__shownGame.leader_position()].join(','); if (p !== last) { log.push([performance.now(), p]); last = p; } if (log.length < 64) requestAnimationFrame(tick); };
      requestAnimationFrame(tick);
    });
    stage = 'walking the chosen route';
    const t0 = Date.now();
    await page.mouse.click(rx, ry);
    check('the menu closes on the choice', await until(page, () => (globalThis.__shownGame.menu_view() ?? '') === '', 10000) >= 0);
    const after = await page.evaluate(() => globalThis.__shownGame.go_here_asked());
    console.log(`go_here_asked ${asked} -> ${after}`);
    check('Go here asked the travel once', after === asked + 1);
    const track = [[0, start[0], start[1]]];
    let shotMid = false;
    while (Date.now() - t0 < 60000) {
      const [p, active] = await page.evaluate(() => [[...globalThis.__shownGame.leader_position()], globalThis.__shownGame.travel_active()]);
      const last = track[track.length - 1];
      if (p[0] !== last[1] || p[1] !== last[2]) {
        track.push([Date.now() - t0, p[0], p[1]]);
        if (!shotMid) { shotMid = true; await page.locator('#screen-box').screenshot({ path: `${OUT}-walking.png` }); }
      }
      if (!active && track.length > 1) break;
      await sleep(30);
    }
    await sleep(800);
    await page.locator('#screen-box').screenshot({ path: `${OUT}-arrived.png` });
    const end = track[track.length - 1];
    const gaps = track.slice(2).map((t, i) => t[0] - track[i + 1][0]);
    const dist = pick.cell ? Math.max(Math.abs(end[1] - pick.cell[0]), Math.abs(end[2] - pick.cell[1])) : null;
    const moves = await page.evaluate(() => globalThis.__moves.map(([t]) => t));
    const pageGaps = moves.slice(1).map((t, i) => Math.round(t - moves[i]));
    console.log(`in the page: ${moves.length} moves, gaps between them ${pageGaps.join(', ')} ms (page clock, per animation frame)`);
    check('each step at least 150 ms of the page clock after the one before (one frame of slack)', pageGaps.length > 0 && Math.min(...pageGaps) >= 150 - 17);
    console.log(`leader moved ${track.length - 1} times in ${end[0]} ms; least gap seen from outside ${gaps.length ? Math.min(...gaps) : '-'} ms (sampled); ends ${dist} cells from the target`);
    check('the party walked (at least 2 moves)', track.length - 1 >= 2);
    check('it ends on or next to the chosen cell', dist !== null && dist <= 1);
    check('the travel ended', await page.evaluate(() => !globalThis.__shownGame.travel_active()));
  }
  stage = 'menu escape and reentry';
  // Escape closes an open menu.
  const [ex, ey] = await h.screenPoint(page, ...viewCell(visible[0][0], visible[0][1]));
  await h.waitPrompt(page, 60000);
  await page.mouse.move(ex, ey); await sleep(300);
  await page.mouse.click(ex, ey, { button: 'right' });
  const opened = await until(page, () => (globalThis.__shownGame.menu_view() ?? '') !== '', 10000) >= 0;
  await h.press(page, 'Escape', 200);
  const closed = await until(page, () => (globalThis.__shownGame.menu_view() ?? '') === '', 10000) >= 0;
  check('a second menu opens and Escape closes it', opened && closed);
  console.log(`page errors ${errors.length}`);
  check('no page errors', errors.length === 0);
  console.log(ok ? 'LIVE OK' : 'LIVE FAIL');
  process.exitCode = ok ? 0 : 1;
} catch (error) {
  console.error(`LIVE FAIL at ${stage}: ${h.errorName(error)} (details suppressed; no game content logged)`);
  for (const line of h.failureLines(error).slice(1)) console.error(line);
  process.exitCode = 1;
} finally {
  await browser.close();
}

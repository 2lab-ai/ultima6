// Boot the game floppies (DF0-DF3) under AROS, apply the PAL fix, capture the screens.
// usage: node play.mjs game  OUTDIR [KEY ...]   game-first set -> first in-game frame, settled frame, one shot per KEY
//        node play.mjs intro OUTDIR             installer-faithful set -> title/intro screens until the picture stops
//   env U6_ADF_DIR overrides the disk set (default $U6_ORACLE_HOME/adf-gamefirst or /adf)
//   keys default DOWN DOWN DOWN (south along the throne-room carpet; north is the gargoyle fight)
import { appendFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import * as o from './lib.mjs';

const [mode = 'game', out = join(o.SHOTS_DIR, `play-${Date.now()}`), ...keyArgs] = process.argv.slice(2);
if (!['game', 'intro'].includes(mode)) throw new Error(`unknown mode ${mode}`);
const ADF = process.env.U6_ADF_DIR || join(o.ORACLE_HOME, mode === 'game' ? 'adf-gamefirst' : 'adf');
mkdirSync(out, { recursive: true });
const LOG = join(out, 'play.log');
const log = (s) => { appendFileSync(LOG, `${new Date().toISOString()} ${s}\n`); console.log(s); };
const shots = [];
async function shot(name, what) {
  const w = await o.shot(page, name, { dir: out, modes: ['lores', 'hires'] });
  const row = { name, frame: await o.frame(page), texhash: await o.texHash(page), what, files: w };
  shots.push(row);
  log(`shot ${name} frame=${row.frame} texhash=${row.texhash} ${w.map((x) => `${x.file} ${x.sha256}`).join(' ')}`);
}

// Lores rectangles of the game screen (456x313 capture; picture origin at about x 93, y 44).
const PARTY_PANEL = [270, 70, 410, 135]; // party names + hit points
const MAP_VIEW = [100, 50, 262, 205];

// Map view as lores RGB rows, and the one-tile scroll that best explains the change between two
// views (static walls and floor dominate; fraction of equal pixels in the overlap).
async function mapView() {
  const v = o.sample(await o.grab(page), 4);
  return { w: v.width, px: v.rgba };
}
function viewShift(a, b) {
  const [x0, y0, x1, y1] = MAP_VIEW;
  const eq = (dx, dy) => {
    let same = 0, n = 0;
    for (let y = y0 + 16; y < y1 - 16; y++) for (let x = x0 + 16; x < x1 - 16; x++) {
      const i = (y * a.w + x) * 4, j = ((y + dy) * b.w + (x + dx)) * 4;
      n++; if (a.px[i] === b.px[j] && a.px[i + 1] === b.px[j + 1] && a.px[i + 2] === b.px[j + 2]) same++;
    }
    return same / n;
  };
  let best = { dx: 0, dy: 0, match: eq(0, 0) };
  for (const [dx, dy] of [[0, 16], [0, -16], [16, 0], [-16, 0]]) {
    const m = eq(dx, dy);
    if (m > best.match + 0.1) best = { dx, dy, match: m };
  }
  best.match = Number(best.match.toFixed(3));
  return best;
}

const h = await o.launch({ log: (s) => appendFileSync(LOG, `${new Date().toISOString()} ${s}\n`) });
const { page } = h;
try {
  await o.setFloppyCount(page, 4);
  for (const [i, v] of ['PROGRAM', 'POPULACE', 'PLAYER', 'PORTRAIT'].entries()) log(`DF${i} ${v}: ${await o.insert(page, join(ADF, `${v}.adf`), i) || 'ok'}`);
  await o.reset(page);
  log(`kicked=${await o.ensureRunning(page)} machine=${JSON.stringify(await o.machine(page))}`);
  // graphics.library is up long before the startup-sequence runs ultcli (about frame 1800)
  await o.waitFrames(page, 700);
  log(`palFix ${JSON.stringify(await o.palFix(page))}`);

  if (mode === 'intro') {
    // Shoot every new picture (texhash change) until picture unchanged and disk idle for 2000
    // frames (u loads for a long time behind a black screen before the title).
    let last = await o.texHash(page), n = 0, still = await o.frame(page);
    const start = await o.frame(page);
    while ((await o.frame(page)) - still < 2000 && (await o.frame(page)) - start < 15000) {
      await o.waitFrames(page, 25);
      const t = await o.texHash(page);
      if (t !== last) { last = t; still = await o.frame(page); await shot(`intro-${String(++n).padStart(2, '0')}`, 'picture changed'); }
      else if ((await o.diskActivity(page)) > 0) still = await o.frame(page);
    }
    await shot('intro-final', `unchanged, disk idle for ${(await o.frame(page)) - still} frames; audio ${(await o.activity(page)).audio}`);
    log(`status ${JSON.stringify(await o.activity(page))} unmapped=${await o.unmapped(page)}`);
  } else {
    // 1. first in-game frame = the party panel has text in it
    const t0 = await o.frame(page);
    // AROS console and the empty parchment give <= 5 colours there, names + hit points about 22
    while ((await o.regionColours(page, PARTY_PANEL)) < 12) {
      if ((await o.frame(page)) - t0 > 20000) throw new Error('game UI did not appear within 20000 frames');
      await o.waitFrames(page, 10);
    }
    await shot('ingame-first', 'party panel drawn');
    // 2. settled = the map view has been redrawn since (the first frame can still show the
    //    uninitialised map buffer), and disk idle + party panel unchanged for 250 frames
    const map0 = await o.regionHash(page, MAP_VIEW);
    let ph = await o.regionHash(page, PARTY_PANEL), since = await o.frame(page), redrawn = false;
    while (!redrawn || (await o.frame(page)) - since < 250) {
      await o.waitFrames(page, 25);
      if (!redrawn && (await o.regionHash(page, MAP_VIEW)) !== map0) { redrawn = true; since = await o.frame(page); }
      const nh = await o.regionHash(page, PARTY_PANEL);
      if (nh !== ph || (await o.diskActivity(page)) > 0) { ph = nh; since = await o.frame(page); }
      if ((await o.frame(page)) - t0 > 30000) throw new Error('game view did not settle within 30000 frames');
    }
    await shot('ingame', `settled; map colours ${await o.regionColours(page, MAP_VIEW)}`);
    // 3. one step per key; the game reads raw keys through its input.device handler. A step
    //    scrolls the map view by one 16-pixel tile, so a step is confirmed by the scroll (NPCs
    //    move on their own). The game starts mid-fight (gargoyles in the throne room), and a
    //    party member or gargoyle can block a tile: then the other arrows are tried in turn,
    //    each press being one game turn. The shot records which key moved the party.
    const keys = keyArgs.length ? keyArgs : ['DOWN', 'DOWN', 'DOWN'];
    const ARROWS = ['DOWN', 'LEFT', 'RIGHT', 'UP'];
    for (const [i, k] of keys.entries()) {
      let moved = null;
      const pressed = [];
      for (const key of [k, ...ARROWS.filter((a) => a !== k)]) {
        const before = await mapView();
        await o.tap(page, key);
        pressed.push(key);
        for (let w = 0; w < 10 && !moved; w++) { // 250 frames: companions' and gargoyles' combat turns
          await o.waitFrames(page, 25);
          const sh = viewShift(before, await mapView());
          if (sh.dx || sh.dy) moved = { ...sh, key };
        }
        if (moved) break;
        log(`step ${i + 1}: ${key} did not scroll the view`);
      }
      await o.waitFrames(page, 50);
      await shot(`step-${i + 1}`, `keys ${pressed.join(',')}: ${moved ? `${moved.key} scrolled the view dx=${moved.dx} dy=${moved.dy} (match ${moved.match})` : 'no key scrolled the view'}`);
    }
  }
  writeFileSync(join(out, 'shots.json'), JSON.stringify({ mode, adf: ADF, shots }, null, 1));
} finally {
  await o.close(h);
}

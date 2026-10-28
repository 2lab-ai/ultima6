// Static shell: loads the wasm engine, feeds it dropped files, keys and time, and paints its
// RGBA frames. No game bytes are fetched or uploaded. Dropping files runs the intake
// (zip/HFE/ADF -> game files) and boots the engine; the game files are kept in this browser's
// IndexedDB so a reload starts the game again without a new drop. A large zip (the archive.org
// one: flux streams and scans around three disk images) is not read whole: the engine names the
// parts to read, its directory and its disk images, and the page reads only those (Blob.slice);
// the input limit counts the bytes read of it, not its size. The engine runs the
// original launcher's sequence: `u` (the opening, the main menu, character creation), then
// `game`; when it ends (Quit in the menu) the page says so and offers Restart.
//
// Everything that touches files, the engine or storage (engine load, the restore at page load,
// each import, each forget) is one operation on a single queue: an operation starts only after
// the previous one finished, from its file reads through the boot to the storage commit. Each
// works on its own Game and puts it on screen only once it started; the one it replaces is
// freed then. Keys reach the engine only while the canvas has focus and a game runs; the mouse
// on the canvas is the game's pointer (a press there also focuses it).
//
// Saves: a Save in the game (Ctrl-S, Y), like the character `u` creates, is stored after the
// frame it committed in, as one operation on the same queue (one IndexedDB transaction); every
// start lays the stored save over the game files, so a reload resumes it. Only the game that
// owns the stored save (the one on screen, once it started) stores its Saves, and none is taken
// while an import runs: a replaced game's Save never lands over the save its replacement
// started from. An imported savegame folder becomes the stored save once the game started
// from it. After the stored save is removed, the next Save of the game on screen is stored
// whole (objlist and every block). Sound: the first click or key on the canvas (or the Mute
// button) starts an AudioContext; after each frame the page renders the game's
// audio in wasm a little ahead and posts it to the AudioWorklet (site/audio-worklet.js).
// A press on the canvas is first the game's pointer event, then the gesture that starts the
// sound: the game queues it whatever the AudioContext does, and only once.
//
// Time (spec 06): the engine runs only in fixed steps of one PAL frame (`step_ms`, 20 ms).
// Each display frame the page asks the game how many steps the wall time since the last one
// is worth at the viewer's speed (`pace`), runs them while the frame's budget lasts, reports
// them (`ran`) and shows the result once, if they changed it (`present`, `frame_version`).
// The speed (1x .. 8x, Max) is the viewer's, kept in localStorage; - and = / + on the canvas,
// the keypad - and +, and the Speed buttons change it.
//
// HD (spec 07 HD0): with HD on (Settings' "HD on", the default, kept in localStorage) the canvas holds
// the game's HD frame, 320·S × 200·S device pixels (S = 4 when its box holds at least three
// device pixels per game pixel, else 2) in the same CSS box: the pixels the game drew from a
// tile, a portrait or a panel picture are MMPX-magnified, computed in this browser from the
// user's own files; text and everything drawn directly are the classic pixels scaled. Off, the
// canvas is the 320 × 200 frame as before. The pointer maps onto the game's 320 × 200 pixels
// either way. The canvas is scaled into its box pixelated, or smoothly when the HD frame has
// more device pixels than the box.
//
// Remembered sight (spec 18; on unless `u6.memory` is kept 'off'): each game the page starts
// shows the map cells in view that were seen and are out of sight now as they last looked, their
// ground and still objects in dark grey (Game.set_sight_memory), where the original shows black.
//
// The HD layout (spec 10; Settings' "Screen layout", kept in localStorage, HD 16:9 unless
// Original 16:10 was chosen): with HD on and that layout, the canvas is 1280 × 720 in a 16:9 box
// (at most 1280 CSS px wide on the page) and the compositor runs at S 4. The ×4 frame is kept on
// an offscreen canvas (updated by its dirty rect) and the canvas is drawn from it: in `game`, the
// map window and the command bar one to one, beside them the date line, the side panel and the
// message scroll at one smaller scale (LAYOUT_REGIONS), over a dark fill with a thin frame round
// each; on every other screen the whole frame letterboxed. The pointer goes through the layout
// backwards (`layoutToGame`): a move in a gap reaches the nearest pixel of the nearest region, a
// press or a release there (or a move with a button held) the rest of the screen, the original's
// cancel. globalThis.__u6Layout maps canvas pixels and game pixels both ways for tests, in any
// layout. With Original, or HD off, the page is as without the HD layout.
//
// Wide mode (spec 13): the engine comes in two builds, site/pkg (Original: the 320 × 200 frame)
// and site/pkg-wide (Wide: a 912 × 408 frame, the UI's 320 × 200 at its top left, a 37 × 21 map
// beside it and under the UI the side panel's three views, spec 14), and the page imports one at
// its engine's operation (the frame's size read from the engine): Wide when the kept screen
// (`u6.layout`) is `wide` or none (a new visitor), else Original. The chooser's and Settings'
// "Screen layout" is Wide / HD 16:9 / Original 16:10; a choice across the builds is noted, and
// the chooser's OK, once it kept the chooser's other choices, keeps it and loads the page again,
// once, after the operations admitted before (the game starts from the kept files and its own
// stored save, as at any load). In Wide, HD on or off, the canvas is 1920 × 1080 in the HD
// layout's 16:9 box: the engine's shown map scaled to cover it, and over it framed windows with
// a title each (the date line top right, the message scroll bottom right, the command bar bottom
// left, each the size of its spec 10 region; spec 14's actor windows: the party under the date
// line, the attributes left of it, the paper doll and the backpack under the attributes, the one
// the side panel's mode shows marked, and the side panel itself in a window of its own while its
// mode is another or a conversation runs); the HD frame scaled smoothly (the compositor at
// WIDE_S), with HD off the classic frame pixelated. A window is dragged by its title strip, its
// place kept (`u6.layout.windows`). The pointer reaches a window's content before the map, its
// frame and title strip are gaps as in the HD layout; every other screen shows the UI's 320 ×
// 200 letterboxed.
//
// The HD font (spec 11; the chooser's and Settings' "Font", kept in localStorage, the HD font
// unless the pixel font was chosen): while the HD layout is in effect the page loads the font
// from this origin (site/fonts/gowun-batang.woff2, document.fonts.load) and, once it has it,
// sets the compositor's HD font mode (Game.set_hd_font): each letter or syllable cell of the
// game's text is its paper in the ×4 frame and listed (Game.hd_glyphs, whole spaces too), and
// after the regions are copied the page draws the listed characters in the font over their
// cells' places on the canvas in runs (a row's neighbouring cells, one space joining words, two
// or more ending the run, a space the player typed never): each run one line from its first
// cell's left edge, at 0.85 of the cell's height there, at the font's natural width (narrowed to
// its cells should it be wider), each character in its own ink. The game's symbols (the key
// wait's cursor, the more prompt's mark) are listed too, their cells the paper, with their
// pixels (Game.hd_symbol_rows): one after a run is drawn as its caret, where the run's text and
// the spaces after it end; one after no run at its own cell. Until the font is loaded, with the
// pixel font chosen, with Original or with HD off, the game's own pixel font shows, as before.
//
// The pointer over the HD frame (spec 11 Design 7): the HD frame is composed without the game's
// pointer sprite (the original's is a hardware sprite above the screen), so its blocks and its
// listed cells are the screen as the game drew it, a cell under the pointer included, and a move
// of the pointer alone composes nothing. With HD on the page draws the pointer itself, last
// (Game.hd_pointer: its box in frame pixels and a version; Game.hd_pointer_rgba: its pixels):
// through every region whose frame rect its box meets, as that region maps the frame's pixels,
// pixelated, clipped to the region and to what the paint copied there, after the region's HD font
// characters (in Wide the map's before the windows in front of it, each window's over its
// content, at full opacity); in the whole view likewise, and on 07's canvas (HD on with Original)
// at its scale. When its version moves, the page paints the canvas under its old and new boxes as
// for a dirty rect of the HD frame there (the regions they touch and the runs listed there), then
// the pointer. With HD off the classic frame holds the sprite, as before.
//
// Fullscreen (the toolbar's Fullscreen): #screen-box (the canvas, its badge, the hidden input
// that takes Korean) goes fullscreen, the canvas as large as fits with the game's aspect ratio,
// centred on black, drawn as on the page; the canvas takes the focus, and where the browser has
// the Keyboard Lock API, Escape is locked to the game while it lasts (holding it leaves). An Exit
// button shows at the top right while the pointer moves (hidden after EXIT_SHOWN_MS still). The
// toolbar's button, the Exit button and the browser's own way out (Escape, F11) leave it, and the
// button says which it does. Where the page cannot ask for fullscreen (requestFullscreen missing
// or refused: iPhone Safari), the box fills the viewport by a class instead, left the same way.
//
// The page shows the game screen, the toolbar under it (Fullscreen, Mute, Voices, Speed,
// Settings), the drop line while no game files are kept or loaded (dropping anywhere on the page
// works at any time), the status line, the controls (closed) and the licence notices.
//
// An HD pack (spec 07 H2: a zip the owner's local tool made from the owner's own files): a dropped
// `.zip` is first classified from its zip directory and its manifest.json alone (the engine names
// the parts to read); a pack never starts or touches a game. The rule: a zip is a pack if and only
// if its root manifest.json reads (within the reader's limit, 64 KiB unpacked, and the probe's for
// its range; its CRC-32 right) and parses as a JSON object whose root `format` is u6-hdpack/1. Any
// other zip goes to the import as before H2, which takes only the disk images in it. A pack whose
// directory already breaks the format is refused before it is read; else it is read whole, checked
// and decoded by the engine (refused whole, with nothing changed, when it does not read), set on
// the game on screen and on every game started after, and kept in IndexedDB so the next visit
// loads it before its first HD frame (storage that refuses it: the pack works for this visit only,
// and the one kept before is removed). "Forget the HD pack" removes it from the page and the
// browser. Its images go in front of MMPX for the images whose classic bytes they were made from.
// A drop that holds a pack and other files is refused as a whole. Loading a pack while another is
// set holds both decoded until the new one is checked (so a refusal changes nothing).
//
// The chooser (first run, and Settings): the page's language (English or 한글: the page's own
// labels, buttons and status lines, from site/i18n.js; the game's text stays English), HD: MMPX,
// or one of the packs the site lists under /packs/ (static files, for every visitor), and NPC
// voices. A listed pack is downloaded and then loaded and kept exactly as a dropped one. The
// chooser shows the pack active (none, a dropped one, or the listed one it came from), and OK
// makes the entry checked the active one. The choice made is kept in localStorage; while there
// is none (or one an older page kept, SETUP_VERSION) the chooser shows first, on 한글, with the
// active pack checked or else the first the site lists (MMPX when it lists none), and NPC voices
// checked. While NPC voices is checked the chooser shows the voice model's use restrictions and
// its OK reads as their acceptance: OK records it as the Voices terms' Accept does and starts the
// voices the same way; unchecked, OK turns them off and asks nothing of /models/. Opened from
// Settings it shows the voices as they are (checked while on). An old link's `?k=` (the key the
// packs once needed) is taken off the address bar, and the key an older page kept is removed.
//
// Voices (spec 09 V3, site/voice/): off until the visitor accepts them at the chooser (NPC
// voices) or presses Voices. Before the first download the model's use restrictions show (at
// the chooser, or with this browser's room for the site at Voices), and only their acceptance
// downloads it; nothing is requested from /models/ before. The worker
// (site/voice/worker.js) runs the model on WebGPU where the browser offers an adapter, else on
// the CPU (slower); the ready line and the chip say which. Once it has the model checked and
// warmed up, the game's speech tap is on (Game.set_voices) and after each frame what NPCs said
// (Game.take_speech) goes to the player (site/voice/player.js) and nowhere else: the
// text is the user's game text, never logged, shown or stored. Each NPC speaks in a voice of its
// own (site/voice/profile.js: from its actor number and the engine's names for its kind, never
// shown either; data-voice-distinct counts the voices heard), or the personality voice a dropped
// voice pack gives its actor (`voices.u6vp`, which the owner's local tool tools/speech/profile.mjs
// makes from the NPC bundles: named so, or a .json dropped alone told by its format; checked
// whole, kept in IndexedDB, shown as a count, never sent; "Forget voices" removes it too; the
// site may also list one, slice K6 below). The speech plays through a voice
// gain beside the music's (Mute silences both); Enter, Escape and Space on the canvas, and a newer
// turn, cancel it; above 4x nothing is spoken. While voices are on a chip says the voice is
// generated on this device. The choice and the acceptance are kept in localStorage: the next
// visit starts the voices from this browser's cache only (never downloads unasked). "Forget
// voices" removes the cached model and the acceptance. A failure (download, a part that does not
// check out twice, no room, a runtime that does not start on the CPU, a lost GPU device) turns
// voices off with a line that names only its kind; the game runs on.
//
// NPC bundles (spec 09 Design 5) and the game strings are exported natively from the user's
// files (tools/export, u6-export), no longer on the page: the bundles an older page kept under
// npc-bundle: are removed once, quietly, at the restore (`dropOldBundles`).
//
// The Korean pack (spec 08, slice K1: `ko.u6lp`, which the owner's local tool tools/i18n makes
// from the NPC bundles): a dropped *.u6lp is read whole (at most max_locale_pack_bytes) and
// checked by the engine's strict reader (LocalePack; refused with a line that names only the
// rule broken, nothing changed), kept in IndexedDB under locale:ko and shown as counts only.
// While the page is in 한글 every game it starts takes it (Game.set_locale, before the game's
// first step): checked against that game's files (a mismatch: the game runs in English, and the
// line says which part differs), its conversation lines then show in Korean in the Hangul font;
// every other text stays English. With no pack at all (none kept, none listed), 한글 is the
// page's own text over the game's English, and the chooser has one line pointing at the README's
// "Packs from your own copy" (with a pack, none). Dropping or forgetting the pack, or switching
// the language at the chooser's OK, restarts the game from the kept files (the save untouched)
// when what it shows changes. "Forget the Korean pack" removes it from the page and the browser.
// Slice K2: the pack may also hold table strings and the game's own messages (the command
// prompts and their results, a conversation's frame, the forms of address), which then show in
// Korean too (counts only: data-locale-pack-strings on the body, data-locale-table-hits/misses
// and data-locale-text-hits/misses on the canvas). Slice K3: a pack that holds the names Look
// gives things and the books and signs shows them in Korean (counts only: data-locale-pack-texts
// on the body, data-locale-look-hits/misses and data-locale-book-hits/misses on the canvas).
// Slice K4: a pack that holds the lines of the opening, the introduction story, the gypsy and
// character creation shows them in Korean when a
// new game starts in 한글 (u runs on the game the pack is set on before its first step, as the
// game does; counts only: data-locale-pack-program on the body, data-locale-program-hits/misses
// on the canvas). Slice K5a: with the pack on the game, while the game reads a line (all three
// bits of Game.keys_owned: what the player says, a name), the canvas's keys go through a hidden
// text input (#ime, made only then: in English or without a pack there is none) that takes the
// browser's IME composition: only committed text reaches the game (Game.text_input; the
// conversation's line editor takes Hangul syllables, and the pack's keyword aliases answer as
// their English keywords), Enter during a composition commits it first and then goes, Backspace
// and Escape while composing are the IME's (Escape cancels the composition only), and every
// other key keeps its canvas meaning (the speed keys included). When the line ends the focus
// goes back to the canvas. Nothing typed is logged, kept or shown in the page's text or data.
// Slice K6: the site's listing (/packs/index.json) may also name a Korean pack (`ko`: {file,
// label}). While the page is in 한글 (at the restore, before the first game, and at the
// chooser's OK) the page downloads it and loads it as a dropped one, its file
// name kept with it in the pack's own transaction (store.js: a drop keeps none, and a kept pack
// without a listed name counts as dropped); it is downloaded again only when the listing names
// another file than the kept one came from, never over a dropped one, and not after Forget until
// the page goes to 한글 again or the next visit. A download or a pack that fails is not tried
// again in the visit (the game stays English, with the line the page has for that case). The
// listing may also name a voice pack (`voices`: {file, label}; spec 09 Design 4), which the
// restore downloads in either language and loads as a dropped one, its file name
// kept with it, under the same rules (not after Forget voices in the visit), with no line of its
// own.
import {
  collect, forgetBundles, forgetFiles, forgetLocalePack, forgetPack, forgetSaves, forgetVoicePack,
  loadFiles, loadLocalePack, loadPack, loadSaves, loadVoicePack, saveFiles,
  storeLocalePack, storePack, storeSaves, storeVoicePack,
} from './store.js';
import { TEXT } from './i18n.js';
import { CACHE_PREFIX, REVISION } from './voice/model.js';
import { createVoices } from './voice/player.js';
import { PACK_FORMAT, PACK_FORMAT_2, readPack } from './voice/profile.js';
import { RESTRICTIONS } from './voice/terms.js';

const canvas = document.getElementById('screen');
const drop = document.getElementById('drop');
const picker = document.getElementById('picker');
const forget = document.getElementById('forget');
const forgetSave = document.getElementById('forget-save');
const forgetHd = document.getElementById('forget-hd');
const forgetKo = document.getElementById('forget-ko');
const mute = document.getElementById('mute');
const hdButton = document.getElementById('hd');
const layoutChoices = document.querySelectorAll('input[name="layout"]');
const fontChoices = document.querySelectorAll('input[name="font"]');
const restart = document.getElementById('restart');
const statusLine = document.getElementById('status');
const badge = document.getElementById('speed-badge');
const slower = document.getElementById('slower');
const faster = document.getElementById('faster');
const speedLabel = document.getElementById('speed-label');
const speedReached = document.getElementById('speed-reached');
const behindMark = document.getElementById('behind');
const settings = document.getElementById('settings');
const setup = document.getElementById('setup');
const setupNote = document.getElementById('setup-note');
const setupHd = document.getElementById('setup-hd');
const setupDropped = document.getElementById('setup-dropped');
const setupOk = document.getElementById('setup-ok');
const setupStatus = document.getElementById('setup-status');
const setupVoices = document.getElementById('setup-voices');
const setupTerms = document.getElementById('setup-terms');
const setupRestrictions = document.getElementById('setup-restrictions');
const voicesButton = document.getElementById('voices');
const voiceChip = document.getElementById('voice-chip');
const voiceOnGpu = document.getElementById('voice-gpu');
const voiceOnCpu = document.getElementById('voice-cpu');
const voiceTerms = document.getElementById('voice-terms');
const voiceStorage = document.getElementById('voice-storage');
const voiceAccept = document.getElementById('voice-accept');
const voiceDecline = document.getElementById('voice-decline');
const forgetVoices = document.getElementById('forget-voices');
const screenBox = document.getElementById('screen-box');
const contextMenu = document.getElementById('context-menu');
const fullscreenButton = document.getElementById('fullscreen');
const fullscreenExit = document.getElementById('fullscreen-exit');

// One-shot marks for the page loaded next (`setMark`, `takeMark`): Restart asked for the focus
// on the canvas; the switch to the other build (`switchBuild`) cut a download of the voice model
// the visitor started, so the page it loads goes on with downloads allowed.
const FOCUS_AFTER_RELOAD = 'u6-focus-after-reload';
const VOICE_DOWNLOAD_AFTER_RELOAD = 'u6-voice-download-after-reload';
// Where sessionStorage throws, a mark is kept in the page's own history entry (history.state,
// which a reload keeps, only this site's document writes, and no other tab sees; it is not Web
// Storage): under `MARKS_STATE`, each mark's time, taken only this long after it was set (one
// whose load never came starts nothing later).
const MARKS_STATE = 'u6Marks';
const MARK_FALLBACK_MS = 60_000;

// The engine (wasm-bindgen's glue of one of the two builds, spec 13 Decision 2: `pkg` Original,
// `pkg-wide` Wide), imported at the engine's operation (`ENGINE_URL`); until then none of these.
let init = null;
let Game = null;
let HdPack = null;
let LocalePack = null;
let PackProbe = null;
let budget_ms = null;
let input_too_large = null;
let max_input_bytes = null;
let max_locale_pack_bytes = null;
let max_pack_bytes = null;
let max_save_bytes = null;
let max_save_file_bytes = null;
let step_ms = null;
let zip_sliced = null;
let shown_map = null;

let wasm = null; // the wasm exports, once loaded
let game = null; // the running game, or null
let crashed = false; // the engine trapped: nothing calls into it again until a reload
let leaving = false; // the page is loading again into the other build: the game steps no more

// Public state for scripts and tests: data-ops = queued operations, data-state = what the page
// does, data-saved = the last save generation stored, data-audio / data-audio-chunks = the
// AudioContext state and the chunks the worklet received; on the canvas data-files = game
// files of the running game, data-saves = save files it booted with, data-leader = x,y,z,
// data-prompt = 1 while the game waits for a command key, data-program = u / game / ended,
// data-name = the party leader's name, data-keys = keys not yet taken by the game,
// data-speed = the chosen speed (1, 1.5, .. 8, max), data-behind = 1 while steps are dropped,
// data-pointer = game / page (`showGamePointer`: whether the engine's sprite is under the
// mouse's last place, by the last pointer event or frame); on the body data-gain = the music's
// gain now (0 muted or above 4x, 1), data-hd-pack = the images of the HD pack loaded
// (tiles,portraits,panels) or none, data-hd-pack-bytes = the bytes
// its pictures take decoded, data-hd-matched = those the game on screen matches (empty without
// a game); data-voice = the voices' state (off, terms, checking, loading, ready, failed),
// data-voice-error = the last failure's code, data-voice-gain = the voice gain, data-voice-path =
// where the model runs once ready (gpu, cpu), data-voice-pack = the voices of the voice pack
// loaded (0 without one), and the player's counts and timings (reflectVoice), never what is
// said; data-locale-pack = the lines and conversations of the Korean pack loaded (runs,scripts)
// or none (data-locale-pack-strings = its table strings and game strings), and on the canvas
// data-locale = what the game on screen took of it (on, mismatch, off) with data-locale-hits /
// data-locale-misses = the runs it put from it and those it did not have, and the same for its
// table strings (data-locale-table-hits / -misses), game strings (data-locale-text-hits /
// -misses), object names (data-locale-look-hits / -misses) and books (data-locale-book-hits /
// -misses), and on the body data-locale-pack-texts = the pack's object names and books (counts
// only), data-fullscreen = off, on (the browser's) or css (the page's own), data-layout = the
// screen layout in effect (hd: HD on with the HD layout chosen; else original), data-font = the
// font the game's text is drawn in on the canvas (hd: the HD font, chosen with the HD layout in
// effect and loaded, as the page last set it; after a trap as it was; else pixel),
// data-font-file = the HD font file's state (none: not asked for, loading, loaded, failed).
// The drop line shows only while no game files are kept (`filesKept` false once the storage said
// so) or loaded (a game on screen).
let filesKept = null; // whether this browser keeps game files; null until the restore asked
function reflect(state) {
  if (state) document.body.dataset.state = state;
  const live = game && !crashed;
  canvas.dataset.files = String(live ? game.game_files() : 0);
  canvas.dataset.saves = String(live ? game.save_files() : 0);
  drop.hidden = !(filesKept === false && game === null);
}

// --- the page's language ---------------------------------------------------------------------

const SETUP_KEY = 'u6.setup'; // localStorage: the chooser's choice made, {"lang":"en"|"ko","version":3}
// The record's version: 3 since the packs are public and the first run turns everything on (one
// of 2, from slice K6, or none, was kept before those defaults: `setupStale`).
const SETUP_VERSION = 3;

// The stored choice, or null (none yet, or storage that cannot be read).
function storedSetup() {
  try {
    const value = JSON.parse(localStorage.getItem(SETUP_KEY));
    return value && typeof value === 'object' && Object.hasOwn(TEXT, value.lang) ? value : null;
  } catch {
    return null;
  }
}

let lang = storedSetup()?.lang ?? 'en';

// The page's text `key` in its language (a function of `args` where the line shows values).
function t(key, ...args) {
  const text = TEXT[lang][key] ?? TEXT.en[key];
  return typeof text === 'function' ? text(...args) : text;
}

// The status line: `text`, also on the chooser's line while a pack it chose loads (`mirror`).
// `say` writes one key's line and remembers it, so a change of language rewrites it.
let mirror = false;
let said = null;
// Whether the chooser's line shows the switch line (`showSwitchOnChooser`), which a change of
// OK's label or of the language writes again (`refreshSwitchLine`).
let switchShown = false;
function setStatus(text) {
  said = null;
  statusLine.textContent = text;
  if (mirror) {
    setupStatus.textContent = text;
    switchShown = false;
  }
}

function say(key, ...args) {
  setStatus(t(key, ...args));
  said = [key, args];
}

// Puts the page's text in `next` language: the labels and buttons index.html names by key, the
// buttons whose text is a state, the last line `say` wrote. A switch to 한글 lets the site's
// Korean pack load again after a Forget (spec 08 K6).
function setLang(next) {
  if (next === 'ko' && lang !== 'ko') hostedForgot = false;
  lang = Object.hasOwn(TEXT, next) ? next : 'en';
  document.documentElement.lang = lang;
  for (const el of document.querySelectorAll('[data-i18n]')) el.textContent = t(el.dataset.i18n);
  for (const el of document.querySelectorAll('[data-i18n-title]')) el.title = t(el.dataset.i18nTitle);
  for (const el of document.querySelectorAll('[data-i18n-label]')) el.setAttribute('aria-label', t(el.dataset.i18nLabel));
  reflectLocale();
  repaintWide();
  mute.textContent = t(audio.muted ? 'unmute' : 'mute');
  showSpeed();
  showStorage();
  // The switch line names OK as it reads now (relabelled above).
  if (said && said[0] !== 'layoutSwitchAtOk') say(said[0], ...said[1]);
  refreshSwitchLine();
}

function message(err) {
  return err?.message ?? String(err);
}

// The low word of the clock in seconds, as the original seeds its RNG from time().
function clockSeed() {
  return Math.floor(Date.now() / 1000) & 0xffff;
}

// --- the operation queue -------------------------------------------------------------------

let tail = Promise.resolve();
let queued = 0;

// Runs `op` after every operation queued before it; a failure is shown (`what`: the key of its
// name) and ends only that one.
function enqueue(what, op) {
  queued += 1;
  document.body.dataset.ops = String(queued);
  const run = async () => {
    try {
      await op();
    } catch (err) {
      console.error(err);
      setStatus(t('failed', t(what), message(err)));
    } finally {
      queued -= 1;
      document.body.dataset.ops = String(queued);
      reflect();
    }
  };
  tail = tail.then(run, run);
  return tail;
}

// --- sound ---------------------------------------------------------------------------------

const AHEAD_S = 0.15; // audio rendered ahead of the worklet
const MAX_RENDER_S = 0.25; // at most per frame (a tab back from the background)
const LOUD_UP_TO = 4; // above this speed (and at Max) the music fades out
const FADE_S = 0.05 / 3; // the fade's time constant: ~95% of the way in 50 ms
const audio = {
  ctx: null, node: null, gain: null, voiceGain: null, ready: false, muted: false,
  queued: 0, at: 0, posted: 0, received: 0, // the worklet's last report, frames posted since
};

function reflectAudio() {
  document.body.dataset.audio = audio.ctx ? audio.ctx.state : 'off';
  document.body.dataset.audioChunks = String(audio.received);
}

// The audio machine of game `g` at the context's rate (each new game gets its own).
function attachAudio(g) {
  if (audio.ready && g && !crashed) g.audio_start(audio.ctx.sampleRate);
}

// From a user gesture: the AudioContext (created once, resumed after that) and its worklet.
async function startAudio() {
  if (audio.ctx) {
    if (audio.ctx.state === 'suspended') await audio.ctx.resume();
    reflectAudio();
    return;
  }
  const ctx = new AudioContext();
  audio.ctx = ctx;
  ctx.onstatechange = reflectAudio;
  reflectAudio();
  await ctx.audioWorklet.addModule('./audio-worklet.js');
  const node = new AudioWorkletNode(ctx, 'u6-audio', {
    numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2],
  });
  const gain = ctx.createGain();
  gain.gain.value = loudness();
  node.connect(gain).connect(ctx.destination);
  // The voices' gain, beside the music's: 0 muted, else 1 (written directly: it is never
  // scheduled; each utterance fades on a gain of its own).
  const voiceGain = ctx.createGain();
  voiceGain.gain.value = audio.muted ? 0 : 1;
  voiceGain.connect(ctx.destination);
  audio.voiceGain = voiceGain;
  voice.attach(ctx, voiceGain);
  reflectVoice();
  node.port.onmessage = (e) => {
    audio.queued = e.data.queued;
    audio.received = e.data.received;
    audio.at = ctx.currentTime;
    audio.posted = 0;
    reflectAudio();
  };
  Object.assign(audio, { node, gain, ready: true });
  attachAudio(game);
  if (ctx.state === 'suspended') await ctx.resume();
  reflectAudio();
}

function gesture() {
  startAudio().catch((err) => console.warn(`sound is off: ${message(err)}`));
}

// Once per frame: keep the worklet AHEAD_S ahead (its last report less what it played since,
// plus what was posted after the report).
function feedAudio() {
  if (!audio.ready || audio.ctx.state !== 'running') return;
  const rate = audio.ctx.sampleRate;
  const held = Math.max(0, audio.queued - (audio.ctx.currentTime - audio.at) * rate) + audio.posted;
  const want = Math.min(Math.ceil(AHEAD_S * rate - held), Math.ceil(MAX_RENDER_S * rate));
  if (want <= 0) return;
  const n = game.audio_render(want);
  if (n === 0) return;
  // Copy before the next call into wasm, which may grow (detach) its memory.
  const chunk = new Float32Array(wasm.memory.buffer, game.audio_ptr(), n).slice();
  audio.node.port.postMessage(chunk, [chunk.buffer]);
  audio.posted += want;
}

// The music's gain: 0 muted (at any speed) or above LOUD_UP_TO, else 1. The music is rendered
// in wall time whatever the speed.
function loudness() {
  const speed = SPEEDS[speedAt];
  return audio.muted || speed === 0 || speed > LOUD_UP_TO ? 0 : 1;
}

// Sets the gain to `loudness()`: muting at once, any other change over ~50 ms. Only through
// the automation timeline: a value written directly does not take while a fade is scheduled.
function applyGain() {
  if (!audio.gain) return;
  const param = audio.gain.gain;
  const now = audio.ctx.currentTime;
  param.cancelScheduledValues(now);
  if (audio.muted) param.setValueAtTime(0, now);
  else param.setTargetAtTime(loudness(), now, FADE_S);
}

// Mute sets both gains: the music's (applyGain) and the voices'.
function setMuted(muted) {
  audio.muted = muted;
  applyGain();
  if (audio.voiceGain) audio.voiceGain.gain.value = muted ? 0 : 1;
  mute.textContent = t(muted ? 'unmute' : 'mute');
  mute.setAttribute('aria-pressed', String(muted));
  reflectVoice();
}

// --- voices (spec 09 V3) -----------------------------------------------------------------------

const VOICES_KEY = 'u6.voices'; // localStorage: 'on' while the visitor has voices on
const TERMS_KEY = 'u6.voiceTerms'; // localStorage: the model revision whose terms were accepted
// The keys that cancel the speech (the more prompt's, and those that answer).
const VOICE_CANCEL_KEYS = new Set(['Enter', 'Escape', ' ']);
const VOICE_ERRORS = {
  notcached: 'voiceErrNotCached', download: 'voiceErrDownload', hash: 'voiceErrHash',
  quota: 'voiceErrQuota', storage: 'voiceErrStorage', model: 'voiceErrModel', wasm: 'voiceErrWasm',
  gpu: 'voiceErrGpu',
};

let voiceState = 'off';
let voiceError = '';
let voiceQuiet = false; // a start from the cache at page load: no progress lines
let voiceFree = null; // this browser's room for the site, once asked (bytes, -1 unknown)

const stored = (key) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};
const remember = (key, value) => {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // No storage: the choice lasts until the page is left.
  }
};
const voicesWanted = () => stored(VOICES_KEY) === 'on';
const termsAccepted = () => stored(TERMS_KEY) === REVISION;
const voiceIsOn = () => ['checking', 'loading', 'ready'].includes(voiceState);

const voice = createVoices({
  workerUrl: new URL('./voice/worker.js', import.meta.url),
  speed: () => SPEEDS[speedAt],
  onState: voiceChanged,
  onStats: reflectVoice,
});

// The voices' state, buttons, chip and counts on the page (data-voice*: states, counts and
// timings only).
function reflectVoice() {
  const on = voiceIsOn();
  const st = voice.stats;
  const data = document.body.dataset;
  put(data, 'voice', voiceState);
  put(data, 'voiceError', voiceError);
  put(data, 'voiceGain', audio.voiceGain ? audio.voiceGain.gain.value.toFixed(2) : '');
  put(data, 'voicePath', voiceState === 'ready' ? st.path : '');
  put(data, 'voicePack', String(voice.packVoices));
  for (const [key, value] of [['voiceColdMs', st.coldMs], ['voiceRefetched', st.refetched],
    ['voiceSpoken', st.spoken], ['voiceFirstMs', st.firstMs], ['voiceCancels', st.cancels],
    ['voiceFadeMs', st.fadeMs], ['voicePlaying', st.playing], ['voiceSteps', st.steps],
    ['voiceDropped', st.dropped], ['voiceDistinct', st.distinct]]) put(data, key, String(value));
  voicesButton.setAttribute('aria-pressed', String(on));
  put(voiceChip, 'hidden', !on);
  put(voiceOnGpu, 'hidden', !(voiceState === 'ready' && st.path === 'gpu'));
  put(voiceOnCpu, 'hidden', !(voiceState === 'ready' && st.path === 'cpu'));
}

// The player's state changed: loading (its progress on the status line, unless quiet), ready (the
// game's speech tap on), failed (off, with the kind of failure).
function voiceChanged(state, code = '') {
  if (state === 'loading') {
    voiceState = 'loading';
    const st = voice.stats;
    if (!voiceQuiet && st.stage === 'download') say('voiceDownload', st.got, st.total);
    else if (!voiceQuiet && st.stage === 'starting') say('voiceStarting');
  } else if (state === 'ready') {
    voiceState = 'ready';
    voiceError = '';
    if (game && !crashed) game.set_voices(true);
    if (!voiceQuiet) say('voiceReady', voice.stats.path);
    voiceQuiet = false;
  } else if (state === 'failed') {
    voiceOff();
    voiceState = 'failed';
    voiceError = Object.hasOwn(VOICE_ERRORS, code) ? code : 'model';
    say('voiceFailed', t(VOICE_ERRORS[voiceError]));
  }
  reflectVoice();
}

// Voices off: the worker stopped, the speech tap off, the choice kept.
function voiceOff() {
  voice.stop();
  if (game && !crashed) game.set_voices(false);
  remember(VOICES_KEY, 'off');
  voiceState = 'off';
  voiceQuiet = false;
}

// This browser's room for the site, on the terms (unknown: -1).
function showStorage() {
  if (voiceFree !== null) voiceStorage.textContent = t('voiceStorage', voiceFree);
}

// The Voices button: off when on; else the terms the first time, or the model straight away.
function toggleVoices() {
  if (voiceIsOn()) {
    voiceOff();
    say('voiceOff');
    reflectVoice();
  } else if (!termsAccepted()) {
    showTerms();
  } else {
    startVoices();
  }
}

// The model's use restrictions, before its first download, with this browser's room for the
// site. Nothing is downloaded until they are accepted.
function showTerms() {
  voiceTerms.hidden = false;
  document.getElementById('voice-restrictions').textContent = RESTRICTIONS;
  voiceState = 'terms';
  voiceError = '';
  reflectVoice();
  voiceFree = -1;
  showStorage();
  navigator.storage?.estimate?.().then(({ quota, usage }) => {
    voiceFree = Number.isFinite(quota) && Number.isFinite(usage) ? Math.max(0, quota - usage) : -1;
    showStorage();
  }).catch(() => {});
  voiceAccept.focus();
}

function acceptTerms() {
  remember(TERMS_KEY, REVISION);
  voiceTerms.hidden = true;
  startVoices();
}

function declineTerms() {
  voiceTerms.hidden = true;
  voiceState = 'off';
  reflectVoice();
}

// Voices on: the storage asked to persist (a download, not a start from the cache), then the
// worker loads the model, on WebGPU or, without it, on the CPU.
async function startVoices({ cachedOnly = false } = {}) {
  voiceQuiet = cachedOnly;
  voiceState = 'checking';
  voiceError = '';
  remember(VOICES_KEY, 'on');
  reflectVoice();
  if (!cachedOnly) {
    say('voiceChecking');
    try {
      await navigator.storage?.persist?.();
    } catch {
      // Not persisted: the browser may evict the model under pressure; it is fetched again.
    }
  }
  if (voiceState !== 'checking') return;
  voice.load({ cachedOnly });
}

// After a frame: what NPCs said goes to the player, and only there.
function hearSpeech() {
  const said = game.take_speech();
  if (!said) return;
  try {
    voice.say(said);
  } catch {
    // Never shown: the error could carry the text.
  }
}

// "Forget voices": the voices off, the cached model, the acceptance and the voice pack removed
// (the pack off the player at once, out of storage as an operation of the queue, after any drop
// queued before; the site's pack is not downloaded again in the visit). The model's caches and
// the pack are removed apart: one that cannot be removed leaves the other removed, and the line
// says that not everything was.
async function forgetVoiceModel() {
  voiceHostedForgot = true;
  voiceOff();
  remember(VOICES_KEY, null);
  remember(TERMS_KEY, null);
  voiceTerms.hidden = true;
  attachVoicePack(null);
  reflectVoice();
  let modelGone = true;
  try {
    for (const name of await caches.keys()) {
      if (!name.startsWith(CACHE_PREFIX)) continue;
      try {
        await caches.delete(name);
      } catch (err) {
        console.warn(`could not remove a voice model cache: ${errorKind(err)}`);
        modelGone = false;
      }
    }
  } catch (err) {
    console.warn(`could not list the voice model caches: ${errorKind(err)}`);
    modelGone = false;
  }
  let packGone = true;
  await enqueue('opForgetVoices', async () => {
    attachVoicePack(null);
    voiceSource = '';
    try {
      await forgetVoicePack();
    } catch (err) {
      console.warn(`could not remove the voice pack: ${errorKind(err)}`);
      packGone = false;
    }
  });
  say(modelGone && packGone ? 'voiceForgot' : 'voiceForgetFailed');
}

// --- the voice pack (spec 09 Design 4) ---------------------------------------------------------

const VOICE_PACK_MAX = 1 << 20; // the most bytes a voice pack file holds
const VOICE_PACK_NAME = /\.u6vp$/i;
const JSON_SPACE = new Set([0x20, 0x09, 0x0a, 0x0d]);
const VOICE_PACK_ERRORS = {
  large: 'voicePackErrLarge', json: 'voicePackErrJson', format: 'voicePackErrFormat',
  keys: 'voicePackErrKeys', shape: 'voicePackErrShape', revision: 'voicePackErrRevision',
  range: 'voicePackErrRange',
};

// Where the voice pack kept or loaded came from, as the Korean pack's `koSource`: '' (none),
// DROPPED (a drop, or a pack kept without a listed name, even one that no longer checks out) or
// the file the site's listing named, kept with the pack in its own transaction (store.js). The
// listed one is not tried again in the visit once it failed (`voiceHostedFailed`), nor after
// Forget voices (`voiceHostedForgot`).
let voiceSource = '';
let voiceHostedFailed = false;
let voiceHostedForgot = false;

// An error's kind only (its name): a message could quote what was read.
function errorKind(err) {
  return err?.name ?? 'Error';
}

// The pack's profiles on the player (null: none): the voices kept for the session forgotten.
function attachVoicePack(pack) {
  voice.setPack(pack);
  reflectVoice();
}

// Whether the dropped `item` is a voice pack (`alone`: the only file dropped): any file named
// *.u6vp (its format is checked when it is loaded: one of another is refused), or a *.json file
// dropped alone, of at most VOICE_PACK_MAX bytes and outside a savegame folder, that starts as
// JSON (its first byte that is not JSON whitespace is `{`) and parses to an object whose
// `format` is one of the pack's (/1 or /2). `{pack, value}`: `value` is the object, when it was read. Only such a
// file is read here, so a drop the import refuses unread (over its limit, a save file over the
// save limit) is still not read.
async function probeVoicePack(item, alone) {
  if (VOICE_PACK_NAME.test(item.path)) return { pack: true };
  if (!alone || !/\.json$/i.test(item.path) || SAVE_PATH.test(item.path) || item.file.size > VOICE_PACK_MAX) {
    return { pack: false };
  }
  try {
    const bytes = new Uint8Array(await item.file.arrayBuffer());
    const first = bytes.findIndex((b) => !JSON_SPACE.has(b));
    if (first < 0 || bytes[first] !== 0x7b) return { pack: false };
    const value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
    return [PACK_FORMAT, PACK_FORMAT_2].includes(value?.format) ? { pack: true, value } : { pack: false };
  } catch {
    return { pack: false }; // not one (or not readable: the import reads it and says what failed)
  }
}

// A voice pack from `source` (DROPPED, or the listed file it was downloaded as; `value`: its
// object when the probe read it): read (at most VOICE_PACK_MAX bytes), checked whole (profile.js
// readPack), set on the player and kept in this browser in place of the one before, with its
// source. One that does not check out is refused (`refused`: its kind) and nothing changes.
// When the browser does not keep it, the pack works for this visit only and the one kept before
// is removed (else the next visit would bring it back unsaid; `oldStays` when it could not be).
async function installVoicePack(file, value, source) {
  if (value === undefined) {
    if (file.size > VOICE_PACK_MAX) return { refused: 'large' };
    const bytes = new Uint8Array(await file.arrayBuffer());
    try {
      value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes));
    } catch {
      return { refused: 'json' };
    }
  }
  const read = readPack(value);
  if (!read.ok) return { refused: read.code };
  attachVoicePack(read.pack);
  voiceSource = source;
  try {
    await storeVoicePack(value, PACK_NAME.test(source) ? source : '');
    return { voices: read.pack.size, kept: true };
  } catch (err) {
    console.warn(`could not keep the voice pack: ${errorKind(err)}`);
    let oldStays = false;
    try {
      await forgetVoicePack();
    } catch (failed) {
      console.warn(`could not remove the voice pack kept before: ${errorKind(failed)}`);
      oldStays = true;
    }
    return { voices: read.pack.size, kept: false, oldStays };
  }
}

// A dropped voice pack: installed, with a line that gives its count, or the refusal's that names
// only the kind.
async function loadVoicePackFile(file, value) {
  const got = await installVoicePack(file, value, DROPPED);
  if (got.refused) say('voicePackRefused', t(VOICE_PACK_ERRORS[got.refused]));
  else if (got.kept) say('voicePack', got.voices);
  else say('voicePackNotKept', got.voices, t(got.oldStays ? 'voicePackOldStays' : 'voicePackOldGone'));
}

// The voice pack kept from the last visit, on the player before the first game speaks, with its
// source: one kept without a listed name (a drop, or a source missing or not a plain name) is a
// dropped one. One that no longer checks out (another version's ranges) is left where it is and
// not used. Storage that cannot be read: the site's pack is not tried in this visit (a pack kept
// there may be a dropped one).
async function restoreVoicePack() {
  let kept;
  try {
    kept = await loadVoicePack();
  } catch (err) {
    voiceHostedFailed = true;
    console.warn(`could not read the kept voice pack: ${errorKind(err)}`);
    return;
  }
  if (kept === null) return;
  voiceSource = PACK_NAME.test(kept.source) ? kept.source : DROPPED;
  const read = readPack(kept.pack);
  if (read.ok) attachVoicePack(read.pack);
  else console.warn(`the kept voice pack is not used: ${read.code}`);
}

// The voice pack the site lists (spec 09 Design 4, as slice K6's Korean pack, in either
// language: the voices speak both): downloaded (at most VOICE_PACK_MAX bytes read) and installed
// as a dropped one, when no pack is kept or the one kept came from another listed file; never
// over a dropped one. It has no line of its own (data-voice-pack gives its count). A download
// that fails, or a pack refused, changes nothing, warns with its kind only and is not tried
// again in this visit.
async function hostedVoicePack() {
  if (voiceHostedFailed || voiceHostedForgot) return;
  try {
    const listed = (await packIndex()).voices;
    if (listed === null) return;
    if (voiceSource !== '' && (!PACK_NAME.test(voiceSource) || voiceSource === listed.file)) return;
    const response = await packFetch(listed.file);
    if (!response.ok) {
      voiceHostedFailed = true;
      console.warn(`the site's voice pack was not downloaded: HTTP ${response.status}`);
      return;
    }
    const reader = response.body.getReader();
    const parts = [];
    let got = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      parts.push(value);
      got += value.length;
      if (got > VOICE_PACK_MAX) {
        // Enough to tell it is over the limit: the install refuses it before reading it.
        await reader.cancel();
        break;
      }
    }
    const installed = await installVoicePack(new File(parts, listed.file), undefined, listed.file);
    if (installed.refused) {
      voiceHostedFailed = true;
      console.warn(`the site's voice pack is not used: ${installed.refused}`);
    }
  } catch (err) {
    voiceHostedFailed = true;
    console.warn(`the site's voice pack was not loaded: ${errorKind(err)}`);
  }
}

// --- the Korean pack (spec 08, slice K1) -----------------------------------------------------

const LOCALE_PACK_NAME = /\.u6lp$/i;
let koPack = null; // the LocalePack loaded (kept under locale:ko), or null
let localeShown = 'off'; // what the game on screen took of it: off, on, mismatch
let localeWhy = ''; // the mismatch's sentence (a fixed text of the engine's)
// Where the pack kept or loaded came from (slice K6), as the HD pack's `packSource`: '' (none),
// DROPPED (a drop, or a pack kept without a listed name, even one that no longer reads) or the
// file the site's listing named, kept with the pack in its own transaction (store.js). The
// listed one is not tried again in the visit once it failed (`hostedFailed`), nor after Forget
// until the page goes to 한글 again (`hostedForgot`).
let koSource = '';
let hostedFailed = false;
let hostedForgot = false;
let koListed = null; // whether the site's listing names a Korean pack; null until it is in

// Whether a game started now takes the pack: the page in 한글 with a pack loaded.
const wantLocale = () => lang === 'ko' && koPack !== null;

// The pack's counts on the body, the game's on the canvas, and the chooser's note on the game's
// text: shown in 한글 only while no Korean pack is at hand at all (none loaded or kept, and the
// listing, once in, names none), pointing at the README's way to make one.
function reflectLocale() {
  document.body.dataset.localePack = koPack ? koPack.counts().join(',') : 'none';
  document.body.dataset.localePackStrings = koPack ? koPack.string_counts().join(',') : 'none';
  document.body.dataset.localePackTexts = koPack ? koPack.text_counts().join(',') : 'none';
  document.body.dataset.localePackProgram = koPack ? String(koPack.program_strings()) : 'none';
  const live = game && !crashed;
  canvas.dataset.locale = live ? localeShown : 'off';
  setupNote.hidden = lang !== 'ko' || koPack !== null || koSource !== '' || koListed !== false;
}

// The game's runs put from the pack and those it did not have (counts only), each frame.
function reflectLocaleCounts() {
  if (localeShown === 'off') return;
  const [, hits, misses, tableHits, tableMisses, textHits, textMisses, lookHits, lookMisses, bookHits, bookMisses,
    programHits, programMisses] = game.locale_counts();
  canvas.dataset.localeHits = String(hits);
  canvas.dataset.localeMisses = String(misses);
  canvas.dataset.localeTableHits = String(tableHits);
  canvas.dataset.localeTableMisses = String(tableMisses);
  canvas.dataset.localeTextHits = String(textHits);
  canvas.dataset.localeTextMisses = String(textMisses);
  canvas.dataset.localeLookHits = String(lookHits);
  canvas.dataset.localeLookMisses = String(lookMisses);
  canvas.dataset.localeBookHits = String(bookHits);
  canvas.dataset.localeBookMisses = String(bookMisses);
  canvas.dataset.localeProgramHits = String(programHits);
  canvas.dataset.localeProgramMisses = String(programMisses);
}

// `next`, just started, takes the pack when the page wants it (before its first step).
function attachLocale(next) {
  localeShown = 'off';
  localeWhy = '';
  for (const k of ['localeHits', 'localeMisses', 'localeTableHits', 'localeTableMisses', 'localeTextHits', 'localeTextMisses',
    'localeLookHits', 'localeLookMisses', 'localeBookHits', 'localeBookMisses', 'localeProgramHits', 'localeProgramMisses']) {
    canvas.dataset[k] = '0';
  }
  if (!wantLocale()) return;
  localeWhy = next.set_locale(koPack);
  localeShown = localeWhy ? 'mismatch' : 'on';
}

// The line a start adds about the pack: its counts, or why the game did not take it.
function localeLine() {
  if (localeShown === 'on') {
    const [runs, scripts] = koPack.counts();
    return `\n${t('koPackOn', runs, scripts, ...koPack.string_counts(), ...koPack.text_counts(), koPack.program_strings())}`;
  }
  return localeShown === 'mismatch' ? `\n${t('koPackMismatch', localeWhy)}` : '';
}

// The game on screen again from the kept files (the save as stored), when what it should show
// of the pack is not what it shows, or (`changed`: a new pack) it should show the new one:
// `note` first in the status; else just `note`.
async function restartForLocale(note, changed = false) {
  const differs = (localeShown !== 'off') !== wantLocale();
  if (game && !crashed && (differs || (changed && wantLocale()))) {
    await pausingSaves(() => restoreKept(false, note));
  } else {
    setStatus(note);
  }
}

// A Korean pack from `source` (DROPPED, or the listed file it was downloaded as): read (at most
// max_locale_pack_bytes), checked whole by the engine, kept in this browser in place of the one
// before, with its source. One that does not check out is refused with the rule it breaks and
// nothing changes. When the browser does not keep it, it works for this visit only and the one
// kept before is removed (else the next visit would bring it back unsaid). Returns whether it
// was loaded and its status line (or the refusal's, which the status line shows already).
async function installLocalePack(file, source) {
  needEngine();
  const refuse = (key, ...args) => {
    say(key, ...args);
    return { loaded: false, line: t(key, ...args) };
  };
  const limit = max_locale_pack_bytes();
  if (file.size > limit) return refuse('koPackTooLarge', Math.floor(limit / 2 ** 20));
  let bytes = new Uint8Array(await file.arrayBuffer());
  let next;
  try {
    next = LocalePack.load(bytes);
  } catch (err) {
    return refuse('koPackRefused', message(err));
  }
  koPack?.free();
  koPack = next;
  koSource = source;
  reflectLocale();
  const kept = await storeLocalePack(bytes, PACK_NAME.test(source) ? source : '');
  bytes = null;
  let note = t('koPackKept');
  if (!kept) {
    let old = t('koPackOldGone');
    try {
      await forgetLocalePack();
    } catch (err) {
      console.warn(`could not remove the Korean pack kept before: ${errorKind(err)}`);
      old = t('koPackOldStays');
    }
    note = t('koPackNotKept', old);
  }
  const [runs, scripts] = koPack.counts();
  const line = `${t('koPackLoaded', runs, scripts, ...koPack.string_counts(), ...koPack.text_counts(), koPack.program_strings())}${note}`;
  return { loaded: true, line };
}

// A dropped Korean pack: installed, and the game on screen restarts with it when the page is in
// 한글.
async function loadLocalePackFile(file) {
  const { loaded, line } = await installLocalePack(file, DROPPED);
  if (loaded) await restartForLocale(line, true);
}

// The pack kept from the last visit, loaded before the first game starts, with its source: one
// kept without a listed name (a drop, or a source missing or not a plain name) is a dropped one.
// One that no longer reads is left where it is and not used. Storage that cannot be read: the
// site's pack is not tried in this visit (a pack kept there may be a dropped one).
async function restoreLocalePack() {
  let kept;
  try {
    kept = await loadLocalePack();
  } catch (err) {
    hostedFailed = true;
    console.warn(`could not read the kept Korean pack: ${errorKind(err)}`);
    return;
  }
  if (!kept) return;
  koSource = PACK_NAME.test(kept.source) ? kept.source : DROPPED;
  try {
    koPack = LocalePack.load(kept.bytes);
  } catch (err) {
    console.warn(`the kept Korean pack is not used: ${message(err)}`);
  }
  reflectLocale();
}

// The Korean pack the site lists (slice K6), while the page is in 한글: downloaded (at most
// max_locale_pack_bytes read) and installed as a dropped one, when no pack is kept or the one
// kept came from another listed file; never over a dropped one. Returns the install's result
// (whether it loaded, its line), or null when none was downloaded (nothing changed). A download
// that fails, or a pack refused, is not tried again in this visit.
async function hostedLocalePack() {
  if (lang !== 'ko' || hostedFailed || hostedForgot) return null;
  try {
    const listed = (await packIndex()).ko;
    if (listed === null) return null;
    if (koSource !== '' && (!PACK_NAME.test(koSource) || koSource === listed.file)) return null;
    const limit = max_locale_pack_bytes();
    const response = await packFetch(listed.file);
    if (!response.ok) {
      hostedFailed = true;
      console.warn(`the site's Korean pack was not downloaded: HTTP ${response.status}`);
      return null;
    }
    const reader = response.body.getReader();
    const parts = [];
    let got = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      parts.push(value);
      got += value.length;
      if (got > limit) {
        // Enough to tell it is over the limit: the install refuses it before reading it.
        await reader.cancel();
        break;
      }
    }
    const installed = await installLocalePack(new File(parts, listed.file), listed.file);
    if (!installed.loaded) hostedFailed = true;
    return installed;
  } catch (err) {
    hostedFailed = true;
    console.warn(`the site's Korean pack was not loaded: ${errorKind(err)}`);
    return null;
  }
}

// "Forget the Korean pack": off the page at once, out of the browser's storage, and the game on
// screen restarted in English if it had it. The site's pack is not loaded again until the page
// goes to 한글 again or the next visit.
async function forgetKoPack() {
  koPack?.free();
  koPack = null;
  koSource = '';
  hostedForgot = true;
  reflectLocale();
  let line = t('koPackForgot');
  try {
    await forgetLocalePack();
  } catch (err) {
    console.warn(`could not remove the Korean pack: ${errorKind(err)}`);
    line = t('koPackForgetFailed');
  }
  await restartForLocale(line);
}

// --- saves ---------------------------------------------------------------------------------

// The game whose Saves are stored: the one that started from the stored save (`boot`) or was
// on screen when it was removed (`forgetSaved`), or null once the stored save is not its any
// more. `pause` > 0 while an import runs, which may replace it; a Save made meanwhile is kept
// only when the import left this game on screen: it is taken at the first frame after, as a list
// covers every Save since the last one stored.
let owner = null;
let pause = 0;
let saveSeen = 0; // the owner's highest save generation already queued (or, at a forget, made)
let forgets = 0; // removals of the stored save so far: a save job taken before one writes nothing

// After a frame's steps: a Save that committed in them is stored as one operation; the game
// hears of the commit (its next list starts after it) only once the transaction completed. The
// operation writes nothing when its game no longer owns the stored save by then (it was
// replaced) or the stored save was removed after it was taken (it lists only the blocks its
// Saves replaced).
function checkSave() {
  if (pause > 0 || game !== owner) return;
  const gen = game.save_generation();
  if (gen <= saveSeen) return;
  saveSeen = gen;
  const g = game;
  const era = forgets;
  const files = [];
  const n = g.save_collect();
  for (let i = 0; i < n; i += 1) {
    const path = g.save_path(i);
    const ptr = g.save_ptr(i);
    const len = g.save_len(i);
    files.push({ path, bytes: new Uint8Array(wasm.memory.buffer, ptr, len).slice() });
  }
  enqueue('opSave', async () => {
    if (g !== owner || era !== forgets) return;
    await storeSaves(files);
    if (g === game && !crashed) g.save_committed(gen);
    document.body.dataset.saved = String(gen);
    say('saved', files.length);
  });
}

// The save `next` starts from, laid over its game files before the start: `imported` (a
// dropped savegame folder) or else the stored one.
async function attachSaves(next, imported = null) {
  const saves = imported ?? await loadSaves();
  for (const { path, bytes } of saves) next.load_save(path, bytes);
}

// After the game started from `imported`: the folder becomes the stored save. When the browser
// does not keep it, the game's own Saves are not stored either (they hold only the blocks they
// replaced and would land over the old save). Returns the status note.
async function adoptSaves(imported) {
  try {
    await storeSaves(imported, true);
    return '';
  } catch (err) {
    console.warn(`could not keep the imported save: ${message(err)}`);
    owner = null;
    return t('importedSaveNotKept');
  }
}

// --- speed ---------------------------------------------------------------------------------

// The speeds offered, slowest first: u6-core's pacer::PRESETS, then Max (0: as many steps as
// the frame's budget holds). 1x, the original's pace, is the default.
const SPEEDS = [1, 1.5, 2, 3, 4, 6, 8, 0];
const SPEED_KEY = 'u6.speed'; // localStorage: the chosen speed's name
const BADGE_MS = 1500; // the badge stays this long after a change (and always while not at 1x)
const BEHIND_MS = 1000; // "behind" shows while steps were dropped this recently
const REACH_MS = 500; // the speed Max reaches is measured over windows of this much wall time
// The characters that change the speed: their Game.keys_owned bit (the game takes the key
// itself while it is set) and the direction.
const SPEED_CHARS = new Map([['-', [1, -1]], ['+', [2, 1]], ['=', [4, 1]]]);

// The name of speed `i` as stored and reflected: 1, 1.5, .. 8, max.
const speedName = (i) => (SPEEDS[i] === 0 ? 'max' : String(SPEEDS[i]));

// The stored speed; an unreadable or unknown value is 1x.
function storedSpeed() {
  try {
    const name = localStorage.getItem(SPEED_KEY);
    return Math.max(0, SPEEDS.findIndex((_, i) => speedName(i) === name));
  } catch {
    return 0;
  }
}

let speedAt = storedSpeed(); // the chosen speed, an index into SPEEDS
let badgeRecent = false; // the speed changed less than BADGE_MS ago
let badgeTimer = 0;
// Engine ms per wall ms over the frames of the current window, and smoothed over the windows:
// the speed reached (shown at Max).
const reach = { steps: 0, wall: 0, rate: 0 };
// The game's dropped_ms() at the last frame, and the time of the frame it last grew in.
let dropped = 0;
let droppedAt = -Infinity;

function resetReach() {
  Object.assign(reach, { steps: 0, wall: 0, rate: 0 });
}

// A game put on screen runs at the chosen speed, has dropped nothing yet, and its clock starts
// now: the time its start took (the synchronous intake, with no frame) is not owed to it.
function paceGame(g) {
  g.set_speed(SPEEDS[speedAt]);
  last = performance.now();
  dropped = 0;
  droppedAt = -Infinity;
  resetReach();
}

// One preset faster (`by` 1) or slower (-1), within the list: the game takes it at once, the
// badge shows it, the choice is kept for the next visit, the music fades above 4x.
function changeSpeed(by) {
  speedAt = Math.min(Math.max(speedAt + by, 0), SPEEDS.length - 1);
  if (game && !crashed) game.set_speed(SPEEDS[speedAt]);
  resetReach();
  try {
    localStorage.setItem(SPEED_KEY, speedName(speedAt));
  } catch {
    // No storage: the choice lasts until the page is left.
  }
  badgeRecent = true;
  clearTimeout(badgeTimer);
  badgeTimer = setTimeout(() => {
    badgeRecent = false;
    showSpeed();
  }, BADGE_MS);
  applyGain();
  voice.speedChanged();
  showSpeed();
}

// The chosen speed's name as shown (2×, Max) and at Max the speed reached once measured
// (` 5.3×`, shown after the name: Max 5.3×).
function speedText() {
  const speed = SPEEDS[speedAt];
  if (speed !== 0) return [`${speed}×`, ''];
  return [t('max'), reach.rate > 0 ? ` ${reach.rate.toFixed(1)}×` : ''];
}

// Sets `el`'s `key` (a property such as hidden or textContent) only when it changes: the page
// writes these every frame.
function put(el, key, value) {
  if (el[key] !== value) el[key] = value;
}

// The label announces a change of the chosen speed (a live region); the speed Max reaches,
// which moves every REACH_MS, is beside it and not announced.
function showSpeed() {
  const [name, reached] = speedText();
  put(speedLabel, 'textContent', name);
  put(speedReached, 'textContent', reached);
  put(badge, 'textContent', name + reached);
  put(canvas.dataset, 'speed', speedName(speedAt));
  put(badge, 'hidden', SPEEDS[speedAt] === 1 && !badgeRecent);
}

// A speed key (spec 06 §Design 6): 1 faster, -1 slower, 0 none. The keypad's + and - by `code`,
// always; otherwise the character (`key`, so every layout works): = and + faster, - slower,
// each unless the game takes that key now (a panel cycling members, a line being typed, u's
// menu), when it is the game's. With Ctrl no key is a speed key.
function speedKey(e) {
  if (e.ctrlKey) return 0;
  if (e.code === 'NumpadAdd') return 1;
  if (e.code === 'NumpadSubtract') return -1;
  const char = SPEED_CHARS.get(e.key);
  if (!char || (game.keys_owned() & char[0]) !== 0) return 0;
  return char[1];
}

// After a frame's `k` steps over `dt` wall ms, at frame time `now`: the speed reached, the
// "behind" marker (the game's dropped_ms grew in the last BEHIND_MS; time cut from a stall,
// clamped_ms, is not behind), the gain the music has now.
function paced(now, dt, k) {
  if (dt > 0 && dt <= LONG_FRAME_MS) {
    reach.steps += k;
    reach.wall += dt;
  }
  if (reach.wall >= REACH_MS) {
    const rate = (reach.steps * stepMs) / reach.wall;
    reach.rate = reach.rate > 0 ? (reach.rate + rate) / 2 : rate;
    reach.steps = 0;
    reach.wall = 0;
  }
  const d = game.dropped_ms();
  if (d > dropped) droppedAt = now;
  dropped = d;
  const behind = now - droppedAt < BEHIND_MS;
  put(canvas.dataset, 'behind', behind ? '1' : '0');
  put(behindMark, 'hidden', !behind);
  showSpeed();
  if (audio.gain) put(document.body.dataset, 'gain', audio.gain.gain.value.toFixed(2));
}

// --- engine and frames ---------------------------------------------------------------------

// The Pacer's CAP_MS: a longer gap between frames is a stall (a hidden tab), not the display's
// interval.
const LONG_FRAME_MS = 250;
let looping = false;
let last = 0; // the time of the previous frame
let interval = 1000 / 60; // the display's frame interval, smoothed over the frames
let stepMs = 20; // step_ms(), once the engine loaded

let drawnGame = null; // the game whose frame the canvas shows
let drawnVersion = -1; // its frame_version() then
let drawnHd = false; // that frame was its HD frame

// Draws the game's frame on the canvas (its HD frame with HD on, else its RGBA frame), unless
// the canvas already shows that frame of that game (spec 06 O1: `present` converts only a
// changed frame, and so the page draws only a changed one; a start or an HD switch resizes,
// and so clears, the canvas, and its first frame is drawn whole). Over the HD frame it drew
// last, only the rect the compositions since then wrote is drawn. With the HD layout the frame
// goes to the offscreen ×4 frame and the canvas is composed from it (`compose`), also when only
// the screen changed (the opening's end: the game's regions in place of the whole frame). In
// Wide in `game` the side panel's view is read first (`pollPanel`): when it changed, the canvas
// is composed whole, with no new frame too. With HD on, a move of the pointer alone
// (Game.hd_pointer: the HD frame is composed without it) draws too: the canvas under its old and
// new boxes, then the pointer (spec 11 Design 7). The pointer is read once a draw, here.
function draw() {
  const version = game.frame_version();
  const hd = game.hd_on();
  const view = viewFor(hd);
  if (WIDE && view === REGIONS_VIEW && pollPanel()) wideStale = true;
  const pointer = hd ? game.hd_pointer() : null;
  const pointerMoved = hd && drawnHd && game === drawnGame && pointer[4] !== pointerShown?.[4];
  if (game === drawnGame && version === drawnVersion && view === shownView && !wideStale && !pointerMoved) return;
  if (WIDE) {
    composeWide(view, hd, version, pointer);
    return;
  }
  if (view !== FRAME) {
    compose(view, version, pointer);
    return;
  }
  if (x4) {
    // The dirty rects go to the canvas now: the ×4 frame kept would be stale; its memory goes.
    x4.width = 0;
    x4.height = 0;
    x4 = null;
    x4Game = null;
  }
  if (shownView !== FRAME) drawnGame = null; // the canvas held the layout
  shownView = FRAME;
  const s = hd ? game.hd_scale() : 1;
  const [w, h] = [game.width() * s, game.height() * s];
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
    drawnGame = null; // the resize cleared it
    fitRendering(hd);
  }
  const over = hd && drawnHd && game === drawnGame;
  drawnGame = game;
  drawnVersion = version;
  drawnHd = hd;
  const ctx = canvas.getContext('2d');
  // Re-create the view every frame: wasm memory growth detaches old buffers.
  if (!hd) {
    const pixels = new Uint8ClampedArray(wasm.memory.buffer, game.frame_ptr(), game.frame_len());
    ctx.putImageData(new ImageData(pixels, w, h), 0, 0);
    return;
  }
  const rect = game.hd_take_dirty();
  const before = over ? pointerShown : null;
  const shown = takePointer(pointer);
  const moved = before === null || before[4] !== shown[4];
  if (over && rect.length === 0 && !moved) return;
  const pixels = new Uint8ClampedArray(wasm.memory.buffer, game.hd_frame_ptr(), game.hd_frame_len());
  const image = new ImageData(pixels, w, h);
  if (over) {
    if (rect.length > 0) ctx.putImageData(image, 0, 0, rect[0], rect[1], rect[2], rect[3]);
    // The pointer moved: the HD frame put back under its old and new boxes.
    if (moved) for (const box of [before, shown].map((p) => scaledBox(p, s)).filter(Boolean)) ctx.putImageData(image, 0, 0, ...box);
  } else ctx.putImageData(image, 0, 0);
  // Then the pointer, whole: at a whole scale each canvas pixel takes one of its pixels, so
  // drawing it again over itself changes nothing.
  [pointerShown, pointerOf] = [shown, game];
  paintPointer(ctx, [0, 0, game.width(), game.height()], [0, 0, w, h], [0, 0, w, h]);
}

// --- HD ------------------------------------------------------------------------------------

const HD_KEY = 'u6.hd'; // localStorage: 'off' when the viewer switched HD off

// The stored choice; unreadable storage or any other value is on.
function storedHd() {
  try {
    return localStorage.getItem(HD_KEY) !== 'off';
  } catch {
    return true;
  }
}

let hdOn = storedHd();

const MEMORY_KEY = 'u6.memory'; // localStorage: 'off' when the remembered sight is switched off

// The stored choice of the remembered sight (spec 18); unreadable storage or any other value is
// on.
function storedMemory() {
  try {
    return localStorage.getItem(MEMORY_KEY) !== 'off';
  } catch {
    return true;
  }
}

// The HD scale for the canvas's box now: 4 when it holds at least 3 device pixels per game
// pixel each way, else 2 (MMPX doubles; there is no 3x).
function hdScale() {
  const box = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  return box.width * dpr >= 3 * 320 && box.height * dpr >= 3 * 200 ? 4 : 2;
}

// Settings' "HD on", "Screen layout" (the build switch chosen, while one is pending; none while
// the page loads again into it) and "Font" as the choices are.
function showHd() {
  hdButton.checked = hdOn;
  const screen = switchTo ?? layoutChoice;
  for (const input of layoutChoices) {
    input.checked = input.value === screen;
    input.disabled = switching;
  }
  for (const input of fontChoices) input.checked = input.value === fontChoice;
}

// How the browser scales the canvas into its box: pixelated (each canvas pixel a sharp block)
// by the stylesheet, but smooth when the HD frame has more device pixels than the box (S 4 in
// a box of 3 to 4 device pixels per game pixel): pixelated would drop one HD row and column in
// four there. The HD layout's canvas is pixelated only in a box of a whole number of times its
// 1280 × 720 in device pixels (one to one, ×2, ...), smooth in any other (spec 10 Design 5); the
// Wide layout's likewise of its 1920 × 1080 (spec 13 A5).
function fitRendering(hd) {
  const box = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  let want;
  if (shownView === FRAME) {
    const down = canvas.width > box.width * dpr || canvas.height > box.height * dpr;
    want = hd && down ? 'auto' : '';
  } else {
    const k = Math.round((box.width * dpr) / canvas.width);
    const whole = k >= 1 && Math.abs(box.width * dpr - k * canvas.width) < 0.5
      && Math.abs(box.height * dpr - k * canvas.height) < 0.5;
    want = whole ? '' : 'auto';
  }
  if (canvas.style.imageRendering !== want) canvas.style.imageRendering = want;
}

// The game on screen shows HD as chosen, at the scale for the box now (S 4 with the HD layout,
// WIDE_S in Wide), drawn at once with no new game frame (the canvas takes the size of the frame
// it shows: `draw`). The layout in effect goes on the body first, since it sets the box. The text
// in the HD font while it is chosen and loaded with the HD layout in effect, or HD on in Wide
// (asked for here when it is wanted and not yet), else in the pixel font. After a trap nothing
// changes (the canvas keeps what it showed in the box it had, as for HD on or off) until the
// reload; the font file's state still shows.
function applyHd() {
  if (crashed) {
    reflectFont();
    return;
  }
  showLayout();
  if (fontWanted()) loadFont();
  fontDrawn = fontOn();
  reflectFont();
  if (!game) return;
  game.set_hd(hdOn, WIDE ? WIDE_S : layoutOn() ? LAYOUT_S : hdScale());
  game.set_hd_font(fontDrawn);
  draw();
  fitRendering(game.hd_on());
}

// --- the HD layout (spec 10) -------------------------------------------------------------------

// localStorage: the screen chosen (spec 13 Decision 2): 'wide', 'hd' (HD 16:9) or 'original'
// (Original 16:10).
const LAYOUT_KEY = 'u6.layout';

// The stored choice: 'wide' when none is kept (a new visitor), 'wide' or 'original' as kept,
// else (any other value, storage that cannot be read) 'hd'. Storage that cannot be read keeps
// spec 10's HD: it could not keep a choice of the other build across the reload that loads it.
function storedLayout() {
  try {
    const kept = localStorage.getItem(LAYOUT_KEY);
    if (kept === null) return 'wide';
    return kept === 'wide' || kept === 'original' ? kept : 'hd';
  } catch {
    return 'hd';
  }
}

let layoutChoice = storedLayout();
// The build this page loads, for its life (spec 13 Decision 2): Wide with the Wide screen
// chosen, else Original (HD 16:9 and Original 16:10 are its two layouts, spec 10). Another
// choice across the two reloads the page (`chooseLayout`).
const WIDE = layoutChoice === 'wide';
const ENGINE_URL = WIDE ? './pkg-wide/u6_web.js' : './pkg/u6_web.js';

// The HD layout is in effect: HD on and the HD layout chosen (Original build).
const layoutOn = () => hdOn && layoutChoice === 'hd';

// The game's screen in game pixels (Game.width / height).
const SCREEN_W = 320;
const SCREEN_H = 200;
// The HD layout's canvas, and the compositor's scale while it shows.
const LAYOUT_W = 1280;
const LAYOUT_H = 720;
const LAYOUT_S = 4;
// What the canvas shows: the frame as it is (07's canvas, any layout off), the game's regions
// (`game` with the HD layout) or the whole ×4 frame letterboxed (any other screen with it).
const FRAME = 'frame';
const REGIONS_VIEW = 'game';
const WHOLE_VIEW = 'whole';
// The game screen's regions (spec 10 Design 2): `from`, the classic rect in game pixels (x, y,
// width, height; the screen's parts the engine draws: the map window gfx/view.rs, the command
// bar main_loop.rs and ui/cursor.rs with its underline row, the date line actors/seams.rs, the
// side panel magic/book.rs and ui/panels.rs, the message scroll ui/textstate.rs); `to`, where
// it goes on the 1280 × 720 canvas (canvas pixels, integers). Left, 4 px from the edges with 4
// px between them: the map window and under it the command bar at ×4, one to one with the ×4
// frame. Right, in the column x 648..1276 (4 px from the map and the edge): the date line, the
// side panel and the message scroll at 3.5, each centred in the column, 4 px from the top and
// the bottom with 30 and 31 px between them. 3.5 is the only scale of 3.5..3.75 that puts every
// rect on whole pixels (8 rows and 98 rows both whole need a multiple of 1/2; at 3.75 the panel
// would be 367.5 px tall); the 69 px it leaves in the column's height are odd, hence 30 and 31.
const LAYOUT_REGIONS = [
  { name: 'map', from: [8, 8, 160, 160], to: [4, 4, 640, 640] },
  { name: 'commands', from: [8, 176, 160, 17], to: [4, 648, 640, 68] },
  { name: 'date', from: [16, 168, 152, 8], to: [696, 4, 532, 28] },
  { name: 'panel', from: [168, 6, 144, 98], to: [710, 62, 504, 343] },
  { name: 'messages', from: [176, 112, 136, 80], to: [724, 436, 476, 280] },
];
// The whole ×4 frame (1280 × 800) on every other screen: 1152 × 720 with its 16:10 shape,
// centred (spec 10 Design 3).
const LAYOUT_WHOLE = [64, 0, 1152, 720];
// Where a press, a release or a held move in a gap reaches the game: a pixel of the screen in
// none of the map window, the command bar and the side panel (nor the message scroll), so the
// click map takes it for the rest of the screen, a cancel (ui/click.rs: region 3, 0x8f, the
// target cleared), never a walk or a command; the arrow the game draws from there is off screen
// (the 320 × 200 frame clips it). Not in Wide: there the frame goes on past x 319 and y 199, and
// the arrow from (319, 199) reaches the map's columns at the canvas's left edge (WIDE_REST).
const LAYOUT_REST = [319, 199];
// The letterbox's sides (every screen but the game's).
const LAYOUT_FILL = '#0b0b0f';
// Round the game's regions, the game's own paper (spec 10 Design 2, follow-up L2): `paper.bmp`,
// drawn over the whole screen at the boot (main_loop.rs draw_paper), the windows drawn over it.
// Taken from the ×4 frame at every whole paint, never kept. Its outer band, in game pixels 6 rows
// at the top and the bottom and 8 columns at either side, is only the paper in game: the source
// plane (Game.hd_source_at, read at the prompt and in a conversation) shows the paper bitmap and
// nothing else there, and everywhere outside the regions but the column right of the side panel
// beside its rows (the pointer's sprite apart, where it reaches: the shown frame's plane names it
// classic, but the ×4 frame is composed without it, spec 11 Design 7, so the band never shows
// it; the page draws the pointer through the regions only). The band goes to the canvas's edges
// 9-sliced: `from`, a rect of the band (game pixels); `to`, where it goes (canvas pixels);
// `flip`, mirrored across x or y. The corners at ×4, unscaled; the top edge along the canvas at
// ×4 (the canvas is as wide as the ×4 frame), the left edge stretched down it. The right edge,
// the bottom edge and the bottom-right corner are the left edge, the top edge and the
// bottom-left corner mirrored: the band's own right and bottom strips hold the frame the paper
// draws round the message scroll (they are the paper too, so the source plane cannot tell them
// apart). Every rect here is outside the regions' bounding box, so a change in the regions alone
// leaves the band as drawn.
const LAYOUT_PAPER = [
  { name: 'top-left', from: [0, 0, 8, 6], to: [0, 0, 32, 24] },
  { name: 'top', from: [8, 0, 304, 6], to: [32, 0, 1216, 24] },
  { name: 'top-right', from: [312, 0, 8, 6], to: [1248, 0, 32, 24] },
  { name: 'left', from: [0, 6, 8, 188], to: [0, 24, 32, 672] },
  { name: 'right', from: [0, 6, 8, 188], to: [1248, 24, 32, 672], flip: 'x' },
  { name: 'bottom-left', from: [0, 194, 8, 6], to: [0, 696, 32, 24] },
  { name: 'bottom', from: [8, 0, 304, 6], to: [32, 696, 1216, 24], flip: 'y' },
  { name: 'bottom-right', from: [0, 194, 8, 6], to: [1248, 696, 32, 24], flip: 'x' },
];
// Inside the band (canvas pixels) the parchment, one colour: each channel's median of three
// pixels of the ×4 frame, the top-left pixel of game pixels on the row between the side panel and
// the message scroll, a quarter of the panel's width apart (the parchment there is one colour at
// ×4, with and without the owner's HD pack). One colour, not a patch stretched: no patch of the
// parchment is larger than a row of it, which stretched would streak the whole inside; three, so
// that one of them drawn over alone leaves the median as it was (the pointer's sprite never is:
// the ×4 frame is composed without it, spec 11 Design 7).
const LAYOUT_PARCHMENT = [32, 24, 1216, 672];
const LAYOUT_SAMPLES = [[204, 104], [240, 104], [276, 104]];
// What the band is taken from, in ×4 frame pixels: a dirty rect that meets one of these is
// painted whole (`paint`), so the band never shows a frame before. The samples are not among
// them: a dirty rect is one box round all a take changed, so the map's animation and a change of
// the message scroll in one take span the samples' row, and would paint everything. The
// parchment is checked instead: at every partial paint `paint` takes the samples' median from the
// ×4 frame again and paints whole when it is not the colour the canvas shows (`parchmentShown`).
// One changed sample leaves the median as it was; two changed samples, or the paper's whole
// image, change it.
const PAPER_SOURCES = LAYOUT_PAPER.map(({ from }) => from.map((v) => v * LAYOUT_S));

let shownView = FRAME; // what the canvas shows now
let x4 = null; // the offscreen canvas that holds the ×4 frame while the HD layout shows
let x4Game = null; // the game whose ×4 frame x4 holds as drawn last (null: none, or stale)
let parchmentShown = null; // the parchment's colour the last whole paint drew (`parchmentNow`)
let sentButtons = 0; // the pointer's buttons as last sent to the game
let mouseAt = null; // the mouse's client point as its last pointer event left it, or null
// The canvas pixel mapped last (`layoutToGame`, `pixelAt`) shows no pixel of the game's at that
// place: a gap or a margin round the game's regions, a letterbox side, a Wide window's frame or
// title strip; the game's pointer is then at the nearest edge or off screen (LAYOUT_REST,
// WIDE_REST), not under the system's (`gameUnder`).
let pointerGap = false;

// The layout in effect on the body (data-layout: the stylesheet's box): 'wide' in the Wide build
// (HD on or off), else 'hd' or 'original'.
function showLayout() {
  put(document.body.dataset, 'layout', WIDE ? 'wide' : layoutOn() ? 'hd' : 'original');
}

// What the canvas is to show of the game on screen now (`hd`: its HD frame shows). The Wide
// layout shows the game's windows over its map in `game`, the UI's screen letterboxed on every
// other, HD on or off.
function viewFor(hd) {
  if (WIDE) return game.program() === 'game' ? REGIONS_VIEW : WHOLE_VIEW;
  if (!hd || layoutChoice !== 'hd' || game.hd_scale() !== LAYOUT_S) return FRAME;
  return game.program() === 'game' ? REGIONS_VIEW : WHOLE_VIEW;
}

// Draws the HD frame (version `version`) for `view`: on the offscreen ×4 frame (only the dirty
// rect over the one it holds of this game), then on the canvas, whole when it showed another
// game, another view or nothing yet, else only the regions the dirty rect touches (with the
// pointer's old and new boxes when it moved: spec 11 Design 7; `meta`, Game.hd_pointer as `draw`
// read it).
function compose(view, version, meta) {
  if (canvas.width !== LAYOUT_W || canvas.height !== LAYOUT_H) {
    canvas.width = LAYOUT_W;
    canvas.height = LAYOUT_H;
    drawnGame = null; // the resize cleared it
  }
  const [w, h] = [game.width() * LAYOUT_S, game.height() * LAYOUT_S];
  x4 ??= document.createElement('canvas');
  if (x4.width !== w || x4.height !== h) {
    x4.width = w;
    x4.height = h;
    x4Game = null;
  }
  const whole = game !== drawnGame || view !== shownView;
  const over = x4Game === game;
  const rect = game.hd_take_dirty();
  const fresh = shownView !== view;
  const before = whole ? null : pointerShown;
  const pointer = takePointer(meta);
  // The pointer moved (spec 11 Design 7): the canvas under its old and new boxes, as for a dirty
  // rect of the ×4 frame there.
  const moved = before !== null && before[4] !== pointer[4] ? union(scaledBox(before, LAYOUT_S), scaledBox(pointer, LAYOUT_S)) : null;
  drawnGame = game;
  drawnVersion = version;
  drawnHd = true;
  shownView = view;
  [pointerShown, pointerOf] = [pointer, game];
  if (fresh) fitRendering(true);
  if (over && rect.length === 0 && !whole && !moved) return;
  if (!over || rect.length > 0) {
    // Re-create the view every frame: wasm memory growth detaches old buffers.
    const pixels = new Uint8ClampedArray(wasm.memory.buffer, game.hd_frame_ptr(), game.hd_frame_len());
    const image = new ImageData(pixels, w, h);
    const ctx = x4.getContext('2d');
    if (over) ctx.putImageData(image, 0, 0, rect[0], rect[1], rect[2], rect[3]);
    else ctx.putImageData(image, 0, 0);
    x4Game = game;
  }
  paint(view, whole || !over ? null : union(rect.length > 0 ? [...rect] : null, moved));
}

// The part of rect `a` inside rect `b` (x, y, width, height), or null when they do not meet.
function overlap([ax, ay, aw, ah], [bx, by, bw, bh]) {
  const [x0, y0] = [Math.max(ax, bx), Math.max(ay, by)];
  const [x1, y1] = [Math.min(ax + aw, bx + bw), Math.min(ay + ah, by + bh)];
  return x0 < x1 && y0 < y1 ? [x0, y0, x1 - x0, y1 - y0] : null;
}

// The box round rects `a` and `b` (x, y, width, height; null for none), or null for neither.
function union(a, b) {
  if (!a || !b) return a ?? b;
  const [x0, y0] = [Math.min(a[0], b[0]), Math.min(a[1], b[1])];
  const [x1, y1] = [Math.max(a[0] + a[2], b[0] + b[2]), Math.max(a[1] + a[3], b[1] + b[3])];
  return [x0, y0, x1 - x0, y1 - y0];
}

// --- the pointer over the HD frame (spec 11 Design 7) -------------------------------------------

let pointerShown = null; // the pointer the canvas shows (Game.hd_pointer as drawn last), or null
let pointerOf = null; // the game `pointerShown` is of
let pointerImage = null; // an offscreen canvas holding its pixels (Game.hd_pointer_rgba)

// The pointer `p` (Game.hd_pointer as `draw` read it, once a draw: its box in frame pixels and its
// version) as the canvas is to show it: while it is the one the canvas shows (this game's, at the
// same version), that one, nothing copied or taken again; else `p` itself, its pixels taken into
// `pointerImage` when it has a box. The caller then shows it (`pointerShown`, `pointerOf`).
function takePointer(p) {
  if (pointerOf === game && pointerShown !== null && pointerShown[4] === p[4]) return pointerShown;
  if (p[2] > 0) {
    pointerImage ??= document.createElement('canvas');
    [pointerImage.width, pointerImage.height] = [p[2], p[3]];
    const rgba = new Uint8ClampedArray(game.hd_pointer_rgba());
    pointerImage.getContext('2d').putImageData(new ImageData(rgba, p[2], p[3]), 0, 0);
  }
  return p;
}

// Pointer `p`'s box at `s` frame's pixels per game pixel, or null for none (no pointer, an empty
// box).
const scaledBox = (p, s) => (p && p[2] > 0 ? [p[0] * s, p[1] * s, p[2] * s, p[3] * s] : null);

// The pointer the canvas shows (`pointerShown`) through a region that shows the frame's rect
// `from` (game pixels) on the canvas rect `to`, as the region maps the frame's pixels: pixelated,
// clipped to `to` and to `clip` (the canvas rect the caller copied again just before; the pointer
// is drawn only over pixels copied in the same paint), at full opacity. Nothing when its box does
// not meet `from`.
function paintPointer(ctx, from, to, clip) {
  const p = pointerShown;
  if (!p || p[2] === 0 || !clip || !overlap(p, from)) return;
  const area = overlap(to, clip);
  if (!area) return;
  const [sx, sy] = [to[2] / from[2], to[3] / from[3]];
  ctx.save();
  ctx.beginPath();
  ctx.rect(...area);
  ctx.clip();
  ctx.globalAlpha = 1;
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(pointerImage, 0, 0, p[2], p[3], to[0] + (p[0] - from[0]) * sx, to[1] + (p[1] - from[1]) * sy, p[2] * sx, p[3] * sy);
  ctx.restore();
}

// The canvas from the ×4 frame for `view`: everything (`dirty` null: the letterbox's sides or the
// paper round the regions, and every region), or the regions that rect of the ×4 frame touches (a
// one-to-one region only that part of it, a scaled one whole: a part scaled on its own would not
// meet its neighbours'); then the HD font's characters over what it copied (`paintGlyphs`), and
// last the pointer over what it copied (`paintPointer`). A rect that meets what the band is
// taken from (PAPER_SOURCES) paints everything, and so does any rect when the samples' median in
// the ×4 frame now is not the parchment the canvas shows (a rect that meets a sample alone does
// not: one covered sample leaves the median as it was). A one-to-one region with cells listed in
// it, now or at the paint before, is copied whole too: a run's characters move when one of its
// cells changes, so a part copied alone could keep pixels of the run before.
function paint(view, dirty) {
  const ctx = canvas.getContext('2d');
  const glyphs = game.hd_font() ? listedGlyphs() : [];
  if (view === WHOLE_VIEW) {
    if (!dirty) {
      ctx.fillStyle = LAYOUT_FILL;
      ctx.fillRect(0, 0, LAYOUT_W, LAYOUT_H);
    }
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(x4, 0, 0, x4.width, x4.height, ...LAYOUT_WHOLE);
    // `u` lists no cell (its text is in the chip planes), but the letterbox also shows `game`'s
    // last frame once the game itself ended (Ctrl-Q, Y: the launcher ends in `game`). The whole
    // screen is one region here: runs of two windows side by side do not join, since the
    // windows' borders are not listed cells and so end a run.
    if (glyphs.length > 0) paintGlyphs(ctx, glyphs, [0, 0, SCREEN_W, SCREEN_H], LAYOUT_WHOLE, LAYOUT_WHOLE);
    paintPointer(ctx, [0, 0, SCREEN_W, SCREEN_H], LAYOUT_WHOLE, LAYOUT_WHOLE);
    paintedGlyphs = glyphs;
    return;
  }
  if (dirty && PAPER_SOURCES.some((source) => overlap(source, dirty))) dirty = null;
  if (dirty && parchmentNow() !== parchmentShown) dirty = null;
  if (!dirty) paintPaper(ctx);
  const copies = [];
  for (const { from, to } of LAYOUT_REGIONS) {
    const source = from.map((v) => v * LAYOUT_S);
    const one = to[2] === source[2] && to[3] === source[3];
    const part = dirty ? overlap(source, dirty) : source;
    if (!part) continue;
    ctx.imageSmoothingEnabled = !one;
    ctx.imageSmoothingQuality = 'high';
    let copied = to;
    if (one && !listedIn(glyphs, from) && !listedIn(paintedGlyphs, from)) {
      const [x, y, w, h] = part;
      copied = [to[0] + x - source[0], to[1] + y - source[1], w, h];
      ctx.drawImage(x4, x, y, w, h, ...copied);
    } else {
      ctx.drawImage(x4, ...source, ...to);
    }
    if (glyphs.length > 0) paintGlyphs(ctx, glyphs, from, to, copied);
    copies.push([from, to, copied]);
  }
  // Last, the pointer through every region it reaches, over what was copied (spec 11 Design 7).
  for (const [from, to, copied] of copies) paintPointer(ctx, from, to, copied);
  paintedGlyphs = glyphs;
}

// The paper under and round the game's regions, from the ×4 frame now (LAYOUT_PAPER): the band's
// pieces (one to one sharp, the stretched edges smooth), then the parchment inside it (its
// colour kept in `parchmentShown`).
function paintPaper(ctx) {
  for (const { from, to: [x, y, w, h], flip } of LAYOUT_PAPER) {
    const source = from.map((v) => v * LAYOUT_S);
    ctx.save();
    ctx.imageSmoothingEnabled = source[2] !== w || source[3] !== h;
    ctx.imageSmoothingQuality = 'high';
    ctx.translate(flip === 'x' ? x + w : x, flip === 'y' ? y + h : y);
    ctx.scale(flip === 'x' ? -1 : 1, flip === 'y' ? -1 : 1);
    ctx.drawImage(x4, ...source, 0, 0, w, h);
    ctx.restore();
  }
  parchmentShown = parchmentNow();
  ctx.fillStyle = parchmentShown;
  ctx.fillRect(...LAYOUT_PARCHMENT);
}

// The parchment's colour in the ×4 frame now, as a fill style: each channel's median of the
// samples' pixels (LAYOUT_SAMPLES, the top-left ×4 pixel of each), read from the engine's memory.
function parchmentNow() {
  // Re-create the view every call: wasm memory growth detaches old buffers.
  const pixels = new Uint8ClampedArray(wasm.memory.buffer, game.hd_frame_ptr(), game.hd_frame_len());
  const width = game.width() * LAYOUT_S;
  const median = (k) => LAYOUT_SAMPLES
    .map(([gx, gy]) => pixels[(gy * LAYOUT_S * width + gx * LAYOUT_S) * 4 + k])
    .sort((a, b) => a - b)[1];
  return `rgb(${median(0)}, ${median(1)}, ${median(2)})`;
}

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

// The game pixel canvas pixel (px, py) reaches (fractions inside the pixel), as the canvas shows
// now; `held`: the event presses or releases a button, or moves with one held. The frame as it
// is maps its box onto the game's screen (as `sendPointer` always did); the letterboxed frame
// likewise, a point on its dark sides at the nearest edge. In the game's regions a point in one
// is that pixel of its classic rect; in a gap or a margin, a move with no button the nearest
// pixel of the nearest region (the game's pointer stays where the player sees it), anything
// else the rest of the screen (LAYOUT_REST). `pointerGap` says whether the point was in none
// (a dark side, a gap, a margin).
function layoutToGame(px, py, held = false) {
  if (WIDE && shownView !== FRAME) return wideToGame(px, py, held);
  if (shownView === FRAME) {
    pointerGap = false;
    return [Math.floor((px * SCREEN_W) / canvas.width), Math.floor((py * SCREEN_H) / canvas.height)];
  }
  if (shownView === WHOLE_VIEW) {
    const [x, y, w, h] = LAYOUT_WHOLE;
    pointerGap = px < x || px >= x + w || py < y || py >= y + h;
    return [clamp(Math.floor(((px - x) * SCREEN_W) / w), 0, SCREEN_W - 1),
      clamp(Math.floor(((py - y) * SCREEN_H) / h), 0, SCREEN_H - 1)];
  }
  let near = null;
  let far = Infinity;
  for (const region of LAYOUT_REGIONS) {
    const [x, y, w, h] = region.to;
    const inside = px >= x && px < x + w && py >= y && py < y + h;
    const d = inside ? -1 : Math.hypot(Math.max(x - px, 0, px - x - w), Math.max(y - py, 0, py - y - h));
    if (d < far) [near, far] = [region, d];
  }
  pointerGap = far >= 0;
  if (far >= 0 && held) return [...LAYOUT_REST];
  const [fx, fy, fw, fh] = near.from;
  const [x, y, w, h] = near.to;
  return [clamp(fx + Math.floor(((px - x) * fw) / w), fx, fx + fw - 1),
    clamp(fy + Math.floor(((py - y) * fh) / h), fy, fy + fh - 1)];
}

// The canvas pixel (the centre of what shows game pixel (gx, gy)) as the canvas shows now; null
// for a pixel that does not show (outside the game's regions).
function layoutToCanvas(gx, gy) {
  if (WIDE && shownView !== FRAME) return wideToCanvas(gx, gy);
  if (shownView === FRAME) {
    return [((gx + 0.5) * canvas.width) / SCREEN_W, ((gy + 0.5) * canvas.height) / SCREEN_H];
  }
  if (shownView === WHOLE_VIEW) {
    const [x, y, w, h] = LAYOUT_WHOLE;
    return [x + ((gx + 0.5) * w) / SCREEN_W, y + ((gy + 0.5) * h) / SCREEN_H];
  }
  const region = LAYOUT_REGIONS.find(({ from: [x, y, w, h] }) => gx >= x && gx < x + w && gy >= y && gy < y + h);
  if (!region) return null;
  const [fx, fy, fw, fh] = region.from;
  const [x, y, w, h] = region.to;
  return [x + ((gx - fx + 0.5) * w) / fw, y + ((gy - fy + 0.5) * h) / fh];
}

// For tests, in any layout: canvas pixels to game pixels (`held` as layoutToGame) and back,
// what the canvas shows, the regions (in Wide the map, then the windows), the paper round them
// and the rects of the ×4 frame that paint it whole (`paperSources`, spec 10's), the letterbox
// and the rest of the screen, the canvas's size and the compositor's scale of the layout; in
// Wide (`wide`) the windows shown too, each with its frame (`outer`), its title strip (`bar`), the
// panel mode it shows (`view`), its title as painted (before the fit), whether it is the active
// one and whether it is dimmed, and the side panel's view the page read last (`panel`).
globalThis.__u6Layout = {
  toGame: (px, py, held = false) => layoutToGame(px, py, held),
  toCanvas: (gx, gy) => layoutToCanvas(gx, gy),
  get view() {
    return shownView;
  },
  wide: WIDE,
  get regions() {
    const regions = WIDE ? [...(wideMap ? [wideMap] : []), ...shownWindows()] : LAYOUT_REGIONS;
    return regions.map(({ name, from, to }) => ({ name, from: [...from], to: [...to] }));
  },
  get windows() {
    return shownWindows().map((win) => ({
      name: win.name, from: [...win.from], to: [...win.to], outer: [...win.outer], bar: [...win.bar],
      view: win.view ?? null, title: windowTitle(win), active: windowActive(win), dimmed: windowDimmed(win),
    }));
  },
  // The side panel's view as the page read it at its last paint in `game` (Game.panel_view), or
  // null before one.
  get panel() {
    return panelView === null ? null : [...panelView];
  },
  paper: {
    pieces: LAYOUT_PAPER.map(({ name, from, to, flip }) => ({ name, from: [...from], to: [...to], flip: flip ?? null })),
    parchment: [...LAYOUT_PARCHMENT],
    samples: LAYOUT_SAMPLES.map((p) => [...p]),
  },
  paperSources: PAPER_SOURCES.map((r) => [...r]),
  get whole() {
    return [...(WIDE ? WIDE_WHOLE : LAYOUT_WHOLE)];
  },
  get rest() {
    return [...(WIDE ? WIDE_REST : LAYOUT_REST)];
  },
  get size() {
    return WIDE ? [WIDE_W, WIDE_H] : [LAYOUT_W, LAYOUT_H];
  },
  get scale() {
    return WIDE ? WIDE_S : LAYOUT_S;
  },
};

// The switch to the other build (spec 13 Decision 2): the screen chosen across the builds while
// the chooser (or Settings: the same panel) is open, the latest choice, until its OK
// (`switchBuild`); null for none. `switching`: OK started it and the page is to load again.
let switchTo = null;
let switching = false;

// The chooser's and Settings' "Screen layout" changed. Within the build this page loaded: kept,
// on the game on screen at once, and a switch to the other build chosen before is called off.
// Across the builds: only noted (`switchTo`, the latest choice), for the chooser's OK, which
// switches after it kept the chooser's other choices (`applySetup`); a line says so
// (`showSwitchLine`). While the page is loading again into the other build, nothing (the
// choices are disabled).
function chooseLayout(value) {
  if (switching) return;
  const next = value === 'wide' || value === 'original' ? value : 'hd';
  if ((next === 'wide') !== WIDE) {
    switchTo = next;
    showHd();
    showSwitchLine();
    return;
  }
  switchTo = null;
  showSwitchLine();
  layoutChoice = next;
  try {
    localStorage.setItem(LAYOUT_KEY, layoutChoice);
  } catch {
    // No storage: the choice lasts until the page is left.
  }
  showHd();
  applyHd();
}

// The chooser's OK with a switch to the other build noted (`switchTo`): once (`switching`), the
// page loads again into it after every operation admitted before is done: the queue's tail is
// awaited again until no operation came after it (a Save of the game on screen meanwhile is
// stored first), then the choice is kept and the page loads again at once, the game stepped no
// more (`frame`), with nothing new saved: the game starts from the kept files and its own stored
// save, which carry over (no state of the window or the view is saved). Storage that does not
// keep the choice would bring back the build the page left: the switch is called off, the
// radios show the build that runs, and a line says why.
async function switchBuild() {
  if (switching || switchTo === null) return;
  switching = true;
  showHd();
  // Waiting on an operation other than the chooser's own (the one that calls this): a line says
  // the page loads again once it is done, again after each that came meanwhile.
  for (let seen = null; seen !== tail;) {
    if (seen !== null || queued > 1) say('layoutSwitchWaiting');
    seen = tail;
    await seen;
  }
  const next = switchTo;
  try {
    localStorage.setItem(LAYOUT_KEY, next);
  } catch {
    switchTo = null;
    switching = false;
    showHd();
    showSwitchLine();
    say('layoutNotKept');
    return;
  }
  // A download of the voice model the visitor started (not a start from the cache) goes on in
  // the page loaded next, which would otherwise start the voices from the cache only (as it does
  // with no storage for the mark).
  if (!voiceQuiet && (voiceState === 'checking' || voiceState === 'loading')) setMark(VOICE_DOWNLOAD_AFTER_RELOAD);
  leaving = true;
  location.reload();
}

// What the line of a switch to the other build names: the screen noted (`switchTo`) and OK as it
// reads now ("Accept and start" while NPC voices is ticked: `showVoiceTerms`).
function switchWords() {
  return [switchTo, t(setupOk.dataset.i18n)];
}

// The switch line on the chooser's line only, in the page's language.
function showSwitchOnChooser() {
  setupStatus.textContent = t('layoutSwitchAtOk', ...switchWords());
  switchShown = true;
}

// The line of a switch to the other build noted for OK: on the status line and the chooser's;
// gone from both with the switch called off.
function showSwitchLine() {
  if (switchTo !== null) {
    say('layoutSwitchAtOk', ...switchWords());
    showSwitchOnChooser();
    return;
  }
  if (said?.[0] === 'layoutSwitchAtOk') setStatus('');
  if (switchShown) setupStatus.textContent = '';
  switchShown = false;
}

// The switch line written again where it shows (OK relabelled, another language): on both lines
// while the status line shows it, else on the chooser's only (the status line says something
// else since: a wait, a download).
function refreshSwitchLine() {
  if (said?.[0] === 'layoutSwitchAtOk') showSwitchLine();
  else if (switchShown && switchTo !== null) showSwitchOnChooser();
}

// --- the Wide layout (spec 13) -----------------------------------------------------------------

// The Wide layout's canvas (spec 13 Design 5), 16:9, and the compositor's scale while it shows
// (Design 6: 4 unless its time per frame on a game frame passes 8 ms, measured by the web test
// "Wide layout (spec 13 Design 6)", then 2).
const WIDE_W = 1920;
const WIDE_H = 1080;
const WIDE_S = 4;
// The windows' frames, U7-style in colours of our own (no art of another program's): a dark
// border WINDOW_BORDER wide (a lighter line along its inside), then a title strip WINDOW_TITLE
// tall with the window's name in the page's font (WINDOW_TITLE_PAD in from either end of the
// strip, cut short with an ellipsis where it would reach past that), over the window's content,
// WINDOW_EDGE from the canvas's edges, WINDOW_GAP from the window it is placed by. The active
// window (spec 14 Design 6: the one that shows the side panel's mode) is lighter: its border the
// inside line's ink, its line the title's, its strip `active`. An actor window while the side
// panel's own window shows shows its content at WINDOW_DIM over the border's ink (its paper),
// dimmed.
const WINDOW_BORDER = 4;
const WINDOW_TITLE = 20;
const WINDOW_TITLE_PAD = 6;
const WINDOW_EDGE = 8;
const WINDOW_GAP = 8;
const WINDOW_INK = { border: '#16130f', line: '#6b5a3e', bar: '#2e271d', title: '#ecdfbf', active: '#5c4a2c' };
const WINDOW_DIM = 0.5;
// The page's font for the titles (index.html's body's, read once), at this size in canvas pixels.
const WINDOW_FONT_PX = 13;
let windowFont = '';
// A part of the map drawn again alone takes this many canvas pixels more on every side: a
// smoothed scale reads the source pixels round each it shows, so a changed source pixel changes
// the canvas a little past the rect it maps to.
const WIDE_PAD = 8;
// The actor windows' scale (spec 14 Design 7), content canvas pixels per frame pixel: 3 puts
// each of their rects (144 x 98, 72 x 88) on whole pixels, and at their default places leaves
// the leader's cell, at the canvas's centre, under none of them (at spec 13's 3.5 the attributes
// left of the party reach x 880 and the paper doll under them covers the centre).
const ACTOR_S = 3;

// The windows (spec 13 Design 5, spec 14 Design 7), framed (`outer`, the title strip `bar`, the
// content `to`), each showing the frame's rect `from`: the engine's date line, message scroll
// and command bar, each its spec 10 region (`region`: its `from` at the size of its `to` there,
// the column at 3.5, the command bar at 4, one to one with the ×4 frame); spec 14's actor
// windows, the side panel's three views the engine keeps under the UI (the roster's slot, the
// stats', the inventory's cut at its x 72 into the paper doll and the backpack list, its name
// row left out), and the side panel itself (`other`: shown only while its mode is none of the
// three or a conversation runs), each at ACTOR_S. `title`: the key of its name; `view`: the panel mode whose view it
// shows; `named`: the shown member's name follows the title. The default places, in this order,
// each by the ones before it: `corner` (WINDOW_EDGE from the canvas's edges there; with
// `under`, WINDOW_GAP under that window), `leftOf` and `rightOf` (WINDOW_GAP beside that
// window, the tops in line), `below` (WINDOW_GAP under that window, the left edges in line):
// the date line at the top right, the party under it, the attributes left of the party, the
// paper doll and the backpack side by side under the attributes, the message scroll at the
// bottom right, the command bar at the bottom left, the side panel at the top left.
const WIDE_WINDOWS = [
  { name: 'date', title: 'windowDate', region: 'date', corner: 'top-right' },
  { name: 'party', title: 'windowParty', from: [8, 206, 144, 98], view: 0x91, corner: 'top-right', under: 'date' },
  { name: 'stats', title: 'windowAttributes', from: [160, 206, 144, 98], view: 0x90, named: true, leftOf: 'party' },
  { name: 'doll', title: 'windowDoll', from: [8, 320, 72, 88], view: 0x92, named: true, below: 'stats' },
  { name: 'backpack', title: 'windowBackpack', from: [80, 320, 72, 88], view: 0x92, named: true, rightOf: 'doll' },
  { name: 'messages', title: 'windowMessages', region: 'messages', corner: 'bottom-right' },
  { name: 'commands', title: 'windowCommands', region: 'commands', corner: 'bottom-left' },
  { name: 'other', title: 'windowPanel', from: [168, 6, 144, 98], other: true, corner: 'top-left' },
];
const wideWindows = WIDE ? WIDE_WINDOWS.map((spec) => {
  const region = spec.region ? LAYOUT_REGIONS.find((r) => r.name === spec.region) : null;
  const from = region ? region.from : spec.from;
  const [w, h] = region ? [region.to[2], region.to[3]] : [from[2] * ACTOR_S, from[3] * ACTOR_S];
  return { ...spec, from: [...from], to: [0, 0, w, h], outer: [0, 0, w + 2 * WINDOW_BORDER, h + WINDOW_TITLE + 2 * WINDOW_BORDER], bar: [0, 0, w, WINDOW_TITLE] };
}) : [];
// The side panel's modes the actor windows show (0x90 stats, 0x91 roster, 0x92 inventory).
const ACTOR_VIEWS = new Set(WIDE_WINDOWS.filter((w) => w.view !== undefined).map((w) => w.view));
// What the side panel shows as the engine said at the last paint in `game` (`pollPanel`):
// `panelView` its [mode, member, actor, talking] (Game.panel_view; null before the first),
// `panelName` the shown member's name (Game.panel_name); `slotsName` the name of the member the
// actor windows show: `panelName` at the last paint that read one of their views (in another mode
// or a conversation they keep their last views, Design 6, and their titles with them).
let panelView = null;
let panelName = '';
let slotsName = '';
// The side panel shows what no actor window does (spec 14 Design 6): its mode is none of theirs,
// or a conversation runs (its portrait in the panel area, whatever the mode).
const otherMode = () => panelView !== null && panelView.length > 0
  && (!ACTOR_VIEWS.has(panelView[0]) || panelView[3] === 1);
// The windows shown now, in the order they are painted (the last in front): every window but
// the side panel's own, which shows only while the mode is another or a conversation runs.
const shownWindows = () => wideWindows.filter((win) => !win.other || otherMode());
// Window `win` is the active one: the side panel's own while it shows, else the one that shows the
// side panel's mode (both halves of the inventory).
const windowActive = (win) => (win.other
  ? otherMode()
  : !otherMode() && win.view !== undefined && panelView !== null && win.view === panelView[0]);
// Window `win` is dimmed: an actor window while the side panel's own window shows.
const windowDimmed = (win) => win.view !== undefined && otherMode();
// Window `win`'s title in the page's language: its name, the name of the member it shows after
// it where it shows one (`named`).
const windowTitle = (win) => (win.named && slotsName ? `${t(win.title)} — ${slotsName}` : t(win.title));
// The map (Design 5): the engine's shown rect (`shown_map`) scaled to cover the canvas, centred
// (`placeWide`, once the engine loaded).
let wideMap = null;
// Where a press, a release or a held move in a gap (a window's frame or title strip, a drag)
// reaches the game in Wide (as LAYOUT_REST): the UI's top-left pixel, the rest of the screen for
// the click map (in none of its map, command bar, side panel and the actor views' slots: a
// cancel, at the prompt a pass), whose arrow lies in the UI's paper top left, which no window
// shows (the arrow is 16 px at most; the rects nearest it start at x 168, the side panel's, and
// at y 206, the party's), nor the map.
const WIDE_REST = [0, 0];
// Every other screen (a program other than `game`): the UI's 320 × 200 scaled to the canvas's
// height, centred, the sides dark (as LAYOUT_WHOLE).
const WIDE_WHOLE = [Math.round((WIDE_W - (SCREEN_W * WIDE_H) / SCREEN_H) / 2), 0,
  Math.round((SCREEN_W * WIDE_H) / SCREEN_H), WIDE_H];

// localStorage: where the viewer dragged the windows, {name: [x, y]} (each frame's top-left
// corner on the canvas); a window not named (a window new since the places were kept too), or
// named with anything but two numbers, stays at its default place. A window is dragged by its
// title strip and stays whole on the canvas.
const WINDOWS_KEY = 'u6.layout.windows';
let dragged = null; // the window being dragged: { win, dx, dy } (the press's place in its frame)

let wideSrc = null; // the offscreen canvas of the frame the Wide layout is drawn from
let wideSrcGame = null; // the game whose frame it holds (null: none, or stale)
let wideSrcScale = 0; // that frame's scale: 1 the classic frame (HD off), else the HD frame's
let wideStale = false; // the next paint is whole (the titles changed, a window moved)

// Window `win` with its frame's top-left corner at canvas pixel (x, y).
function placeWindow(win, x, y) {
  win.outer[0] = x;
  win.outer[1] = y;
  win.bar[0] = x + WINDOW_BORDER;
  win.bar[1] = y + WINDOW_BORDER;
  win.to[0] = x + WINDOW_BORDER;
  win.to[1] = y + WINDOW_BORDER + WINDOW_TITLE;
}

// Window `win` moved to canvas pixel (x, y) (its frame's top-left corner), kept whole on the
// canvas.
function moveWindow(win, x, y) {
  placeWindow(win, clamp(Math.round(x), 0, WIDE_W - win.outer[2]), clamp(Math.round(y), 0, WIDE_H - win.outer[3]));
}

// The places the viewer dragged the windows to, as kept ({} for none or storage that cannot be
// read).
function keptWindows() {
  try {
    const kept = JSON.parse(localStorage.getItem(WINDOWS_KEY));
    return kept && typeof kept === 'object' && !Array.isArray(kept) ? kept : {};
  } catch {
    return {};
  }
}

// The windows' places kept (WINDOWS_KEY), the side panel's own window's too while it is hidden.
function keepWindows() {
  try {
    localStorage.setItem(WINDOWS_KEY, JSON.stringify(Object.fromEntries(wideWindows.map((w) => [w.name, w.outer.slice(0, 2)]))));
  } catch {
    // No storage: the places last until the page is left.
  }
}

// The map's region from the engine's shown rect `[x, y, width, height]` (frame pixels), and the
// windows at their default places (WIDE_WINDOWS), or where the viewer dragged them
// (`keptWindows`).
function placeWide([x, y, w, h]) {
  const k = Math.max(WIDE_W / w, WIDE_H / h);
  const [cw, ch] = [Math.round(w * k), Math.round(h * k)];
  wideMap = { name: 'map', from: [x, y, w, h], to: [Math.round((WIDE_W - cw) / 2), Math.round((WIDE_H - ch) / 2), cw, ch] };
  const named = (name) => wideWindows.find((o) => o.name === name).outer;
  for (const { name } of WIDE_WINDOWS) {
    const win = wideWindows.find((o) => o.name === name);
    const [, , ow, oh] = win.outer;
    if (win.leftOf || win.rightOf) {
      const [bx, by, bw] = named(win.leftOf ?? win.rightOf);
      placeWindow(win, win.leftOf ? bx - WINDOW_GAP - ow : bx + bw + WINDOW_GAP, by);
    } else if (win.below) {
      const [bx, by, , bh] = named(win.below);
      placeWindow(win, bx, by + bh + WINDOW_GAP);
    } else {
      const right = win.corner.endsWith('right');
      const bottom = win.corner.startsWith('bottom');
      const above = win.under ? named(win.under) : null;
      const top = above ? above[1] + above[3] + WINDOW_GAP : WINDOW_EDGE;
      placeWindow(win, right ? WIDE_W - WINDOW_EDGE - ow : WINDOW_EDGE, bottom ? WIDE_H - WINDOW_EDGE - oh : top);
    }
  }
  const kept = keptWindows();
  for (const win of wideWindows) {
    const at = kept[win.name];
    if (Array.isArray(at) && at.length === 2 && at.every(Number.isFinite)) moveWindow(win, at[0], at[1]);
  }
}

// A drag of a window by its title strip: a press of the left button on a title strip in `game`
// takes that window, in front of the others; the moves with it held move the window (the canvas
// painted whole); the release (or a cancel) keeps the places. Whether the event (at canvas pixel
// (px, py)) is the drag's: its press, a move or its end, which the game takes as the rest of the
// screen, a gap's, wherever the pointer is (also past a window held at the canvas's edge, so
// never a held walk on the map).
function dragWindow(e, px, py) {
  if (e.type === 'pointerdown') {
    dragged = null;
    if (e.button !== 0 || shownView !== REGIONS_VIEW) return false;
    const win = shownWindows().findLast(({ outer }) => inRect(px, py, outer));
    if (!win || !inRect(px, py, win.bar)) return false;
    wideWindows.splice(wideWindows.indexOf(win), 1);
    wideWindows.push(win);
    dragged = { win, dx: px - win.outer[0], dy: py - win.outer[1] };
    repaintWide();
    return true;
  }
  if (!dragged) return false;
  if (e.type === 'pointermove' && (e.buttons & 1) !== 0) {
    moveWindow(dragged.win, px - dragged.dx, py - dragged.dy);
    repaintWide();
    return true;
  }
  dragged = null;
  keepWindows();
  return true;
}

const inRect = (px, py, [x, y, w, h]) => px >= x && px < x + w && py >= y && py < y + h;

// Draws the frame (version `version`; its HD frame with `hd`, else the classic one) for `view`
// on the 1920 × 1080 canvas: first on the offscreen frame (the HD frame's dirty rect only over
// the one it holds of this game at this scale; the classic frame, which has none, whole), then
// on the canvas, whole when it showed another game, another view, the other frame or nothing
// yet, else the part the dirty rect touches (`paintWide`; with the pointer's old and new boxes
// when it moved over the HD frame: spec 11 Design 7; `meta`, Game.hd_pointer as `draw` read it,
// null with HD off).
function composeWide(view, hd, version, meta) {
  if (canvas.width !== WIDE_W || canvas.height !== WIDE_H) {
    canvas.width = WIDE_W;
    canvas.height = WIDE_H;
    drawnGame = null; // the resize cleared it
  }
  const s = hd ? game.hd_scale() : 1;
  const [w, h] = [game.width() * s, game.height() * s];
  wideSrc ??= document.createElement('canvas');
  if (wideSrc.width !== w || wideSrc.height !== h) {
    wideSrc.width = w;
    wideSrc.height = h;
    wideSrcGame = null;
  }
  const over = wideSrcGame === game && wideSrcScale === s;
  const whole = !over || !hd || game !== drawnGame || view !== shownView || hd !== drawnHd || wideStale;
  const fresh = view !== shownView;
  const rect = hd ? game.hd_take_dirty() : [];
  // With HD the pointer is drawn over the frame (spec 11 Design 7); when it moved, the canvas
  // under its old and new boxes is painted again, as for a dirty rect of the HD frame there.
  const before = whole ? null : pointerShown;
  const pointer = hd ? takePointer(meta) : null;
  const moved = before !== null && before[4] !== pointer[4] ? union(scaledBox(before, s), scaledBox(pointer, s)) : null;
  drawnGame = game;
  drawnVersion = version;
  drawnHd = hd;
  shownView = view;
  wideStale = false;
  [pointerShown, pointerOf] = [pointer, game];
  if (fresh) fitRendering(hd);
  if (!over || !hd || rect.length > 0) {
    // Re-create the view every frame: wasm memory growth detaches old buffers.
    const pixels = hd
      ? new Uint8ClampedArray(wasm.memory.buffer, game.hd_frame_ptr(), game.hd_frame_len())
      : new Uint8ClampedArray(wasm.memory.buffer, game.frame_ptr(), game.frame_len());
    const image = new ImageData(pixels, w, h);
    const ctx = wideSrc.getContext('2d');
    if (over && hd) ctx.putImageData(image, 0, 0, rect[0], rect[1], rect[2], rect[3]);
    else ctx.putImageData(image, 0, 0);
    wideSrcGame = game;
    wideSrcScale = s;
  }
  const dirty = union(rect.length > 0 ? [...rect] : null, moved);
  if (whole) paintWide(view, null);
  else if (dirty) paintWide(view, dirty);
}

// The canvas from the offscreen frame for `view`. Every other screen: the dark sides and the UI's
// screen (WIDE_WHOLE). The game's: within one clip rect of the canvas (all of it with `dirty`
// null; else the box round the canvas rects that show the part of the frame `dirty` touches: of
// the map that part, WIDE_PAD wider, all of it with cells listed in it now or at the paint
// before; of a window shown its whole content) the map, then each window shown over it: its
// frame, its title and its content; then the HD font's characters over each region copied
// (`paintGlyphs`, within the clip: copied again just before), and last the pointer through that
// region (`paintPointer`, over the HD frame only: the classic frame holds the sprite), so a
// window in front covers the map's pointer as it covers the map. The HD frame is scaled smoothly
// (a one-to-one window copied as it is), the classic frame pixelated.
function paintWide(view, dirty) {
  const ctx = canvas.getContext('2d');
  const s = wideSrcScale;
  const smooth = s > 1;
  const glyphs = drawnHd && game.hd_font() ? listedGlyphs() : [];
  if (view === WHOLE_VIEW) {
    ctx.fillStyle = LAYOUT_FILL;
    ctx.fillRect(0, 0, WIDE_W, WIDE_H);
    ctx.imageSmoothingEnabled = smooth;
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(wideSrc, 0, 0, SCREEN_W * s, SCREEN_H * s, ...WIDE_WHOLE);
    if (glyphs.length > 0) paintGlyphs(ctx, glyphs, [0, 0, SCREEN_W, SCREEN_H], WIDE_WHOLE, WIDE_WHOLE);
    paintPointer(ctx, [0, 0, SCREEN_W, SCREEN_H], WIDE_WHOLE, WIDE_WHOLE);
    paintedGlyphs = glyphs;
    return;
  }
  let clip = [0, 0, WIDE_W, WIDE_H];
  if (dirty) {
    const rects = [];
    const map = wideMap;
    const part = overlap(map.from.map((v) => v * s), dirty);
    if (part && (listedIn(glyphs, map.from) || listedIn(paintedGlyphs, map.from))) {
      rects.push(map.to);
    } else if (part) {
      const [kx, ky] = [map.to[2] / (map.from[2] * s), map.to[3] / (map.from[3] * s)];
      const x0 = Math.floor(map.to[0] + (part[0] - map.from[0] * s) * kx) - WIDE_PAD;
      const y0 = Math.floor(map.to[1] + (part[1] - map.from[1] * s) * ky) - WIDE_PAD;
      const x1 = Math.ceil(map.to[0] + (part[0] + part[2] - map.from[0] * s) * kx) + WIDE_PAD;
      const y1 = Math.ceil(map.to[1] + (part[1] + part[3] - map.from[1] * s) * ky) + WIDE_PAD;
      rects.push([x0, y0, x1 - x0, y1 - y0]);
    }
    for (const win of shownWindows()) {
      if (overlap(win.from.map((v) => v * s), dirty)) rects.push(win.to);
    }
    if (rects.length === 0) return;
    const x0 = Math.min(...rects.map((r) => r[0]));
    const y0 = Math.min(...rects.map((r) => r[1]));
    const x1 = Math.max(...rects.map((r) => r[0] + r[2]));
    const y1 = Math.max(...rects.map((r) => r[1] + r[3]));
    clip = overlap([x0, y0, x1 - x0, y1 - y0], clip);
    if (!clip) return;
  }
  ctx.save();
  ctx.beginPath();
  ctx.rect(...clip);
  ctx.clip();
  ctx.imageSmoothingEnabled = smooth;
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(wideSrc, ...wideMap.from.map((v) => v * s), ...wideMap.to);
  if (glyphs.length > 0) paintGlyphs(ctx, glyphs, wideMap.from, wideMap.to, overlap(wideMap.to, clip));
  paintPointer(ctx, wideMap.from, wideMap.to, overlap(wideMap.to, clip));
  for (const win of shownWindows()) {
    if (overlap(win.outer, clip)) paintWindow(ctx, win, s, smooth, glyphs, clip);
  }
  ctx.restore();
  paintedGlyphs = glyphs;
}

// Window `win` on the canvas (within the clip `clip` the caller set): its frame, its title
// (`windowTitle`, fitted to its strip), lighter while it is the active one, its content from the
// offscreen frame at scale `s` (`smooth`: the HD frame, scaled smoothly unless one to one), the
// HD font's characters over the content (`glyphs`); the content and the characters at
// WINDOW_DIM over the border's ink while it is dimmed; then the pointer over the content, at full
// opacity (the original's is a sprite above the screen, never part of a window).
function paintWindow(ctx, win, s, smooth, glyphs, clip) {
  const [x, y, w, h] = win.outer;
  const active = windowActive(win);
  ctx.fillStyle = active ? WINDOW_INK.line : WINDOW_INK.border;
  ctx.fillRect(x, y, w, h);
  ctx.strokeStyle = active ? WINDOW_INK.title : WINDOW_INK.line;
  ctx.lineWidth = 1;
  ctx.strokeRect(x + 1.5, y + 1.5, w - 3, h - 3);
  ctx.fillStyle = active ? WINDOW_INK.active : WINDOW_INK.bar;
  ctx.fillRect(...win.bar);
  ctx.save();
  ctx.beginPath();
  ctx.rect(...win.bar);
  ctx.clip();
  ctx.fillStyle = WINDOW_INK.title;
  windowFont ||= `600 ${WINDOW_FONT_PX}px ${getComputedStyle(document.body).fontFamily}`;
  ctx.font = windowFont;
  ctx.textAlign = 'left';
  ctx.textBaseline = 'middle';
  const title = fitText(ctx, windowTitle(win), win.bar[2] - 2 * WINDOW_TITLE_PAD);
  ctx.fillText(title, win.bar[0] + WINDOW_TITLE_PAD, win.bar[1] + WINDOW_TITLE / 2 + 1);
  ctx.restore();
  const dim = windowDimmed(win);
  if (dim) ctx.globalAlpha = WINDOW_DIM;
  const source = win.from.map((v) => v * s);
  ctx.imageSmoothingEnabled = smooth && (win.to[2] !== source[2] || win.to[3] !== source[3]);
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(wideSrc, ...source, ...win.to);
  const copied = overlap(win.to, clip);
  if (glyphs.length > 0 && copied) paintGlyphs(ctx, glyphs, win.from, win.to, copied);
  if (dim) ctx.globalAlpha = 1;
  paintPointer(ctx, win.from, win.to, copied);
}

// `text` as it fits `room` canvas pixels in the context's font: as it is, else cut short with an
// ellipsis (whole characters).
function fitText(ctx, text, room) {
  if (ctx.measureText(text).width <= room) return text;
  const chars = [...text];
  while (chars.length > 0 && ctx.measureText(`${chars.join('')}…`).width > room) chars.pop();
  return `${chars.join('')}…`;
}

// What the side panel shows now (spec 14 Design 7), read at each paint in `game`: the engine's
// Game.panel_view ([mode, member, actor, talking]) and Game.panel_name (the shown member's name)
// into `panelView` and `panelName`, and in one of the actor windows' views the name into
// `slotsName`; true when either changed (the windows' titles and marks, and whether the side
// panel's own window shows, go with them: the paint is whole).
function pollPanel() {
  const view = [...game.panel_view()];
  const name = game.panel_name();
  const same = panelView !== null && view.length === panelView.length && view.every((v, i) => v === panelView[i]);
  if (same && name === panelName) return false;
  panelView = view;
  panelName = name;
  if (!otherMode()) slotsName = name;
  return true;
}

// The Wide layout painted whole (the titles changed, a window moved), at once with a game
// shown.
function repaintWide() {
  if (!WIDE) return;
  wideStale = true;
  if (game && !crashed && drawnGame === game) {
    drawnVersion = -1;
    draw();
  }
}

// The game pixel canvas pixel (px, py) reaches in the Wide layout (as `layoutToGame`). Every
// other screen: the UI's screen letterboxed, a point on a dark side at the nearest edge. The
// game's: a window shown first (each in front of the map, the one painted last in front; a dimmed
// one too, whose press the engine ignores): in its content that pixel of its frame rect; on its
// frame or its title strip (a gap), a move with no button the nearest pixel of its content,
// anything else the rest of the screen (WIDE_REST); elsewhere the map, at its pixel (it covers
// the canvas). `pointerGap` as there: a dark side, a window's frame or title strip.
function wideToGame(px, py, held) {
  const through = ([fx, fy, fw, fh], [x, y, w, h]) => [
    clamp(fx + Math.floor(((px - x) * fw) / w), fx, fx + fw - 1),
    clamp(fy + Math.floor(((py - y) * fh) / h), fy, fy + fh - 1)];
  if (shownView === WHOLE_VIEW) {
    pointerGap = !inRect(px, py, WIDE_WHOLE);
    return through([0, 0, SCREEN_W, SCREEN_H], WIDE_WHOLE);
  }
  const win = shownWindows().findLast(({ outer }) => inRect(px, py, outer));
  pointerGap = win !== undefined && !inRect(px, py, win.to);
  if (pointerGap && held) return [...WIDE_REST];
  if (win) return through(win.from, win.to);
  return through(wideMap.from, wideMap.to);
}

// The canvas pixel (the centre of what shows game pixel (gx, gy)) in the Wide layout (as
// `layoutToCanvas`); null for a pixel that does not show: on every other screen outside the UI's
// 320 × 200; in `game` outside the windows shown and the map, under a window in front of it, or
// of the map past the canvas's edges.
function wideToCanvas(gx, gy) {
  const at = ([fx, fy, fw, fh], [x, y, w, h]) => [x + ((gx - fx + 0.5) * w) / fw, y + ((gy - fy + 0.5) * h) / fh];
  if (shownView === WHOLE_VIEW) {
    return inRect(gx, gy, [0, 0, SCREEN_W, SCREEN_H]) ? at([0, 0, SCREEN_W, SCREEN_H], WIDE_WHOLE) : null;
  }
  const shown = shownWindows();
  const i = shown.findIndex(({ from }) => inRect(gx, gy, from));
  const [region, before] = i >= 0 ? [shown[i], shown.slice(i + 1)] : [wideMap, shown];
  if (!inRect(gx, gy, region.from)) return null;
  const [cx, cy] = at(region.from, region.to);
  if (!inRect(cx, cy, [0, 0, WIDE_W, WIDE_H])) return null;
  return before.some(({ outer }) => inRect(cx, cy, outer)) ? null : [cx, cy];
}

// --- the HD font (spec 11) ---------------------------------------------------------------------

const FONT_KEY = 'u6.font'; // localStorage: 'pixel' when the viewer chose the pixel font
const FONT_FAMILY = 'U6 Gowun Batang'; // index.html's @font-face: site/fonts/gowun-batang.woff2
const GLYPH_CELL = 8; // a character's cell, in game pixels
const GLYPH_SPACE = 0x20; // a listed space: no ink, it joins the words beside it (Design 3)
const GLYPH_SYMBOL = 0x10000; // | its glyph: a listed symbol, drawn from its rows (Design 2 rev 3)
const GLYPH_TYPED = 0x20000; // | 0x20: a listed space the player typed, never ending a run (rev 3)
const FONT_EM = 0.85; // the font's size: of the height the cell has on the canvas
// A run's ink keeps this far inside its cells at either end, in canvas pixels (it starts this far
// in, and a run is narrowed to its cells less this either side): the rasteriser's anti-aliased
// fringe reaches about a pixel past the ink box the font measures (measured on Chromium, the
// shipped woff2 at 0.85 em: 'A' at the pen, the fringe one pixel left of it).
const GLYPH_INSET = 1;

// The stored choice: 'pixel', else (any other value, none, storage that cannot be read) 'hd'.
function storedFont() {
  try {
    return localStorage.getItem(FONT_KEY) === 'pixel' ? 'pixel' : 'hd';
  } catch {
    return 'hd';
  }
}

let fontChoice = storedFont();
let fontFile = 'none'; // the font file: none (not asked for yet), loading, loaded, failed
let fontDrawn = false; // the game's text is drawn in the HD font (as `applyHd` set it last)
let paintedGlyphs = []; // the cells the last `paint` drew (Game.hd_glyphs then)
let symbolRows = new Uint8Array(0); // the listed symbols' pixels (Game.hd_symbol_rows) with them
// How far below its cell's centre a character's baseline lies, in ems of the font (once loaded:
// half a syllable's ink height less its depth, so the syllables are centred in their cells and
// ASCII stands on the same baseline, where the font puts it beside them).
let glyphDrop = 0;

// The HD font is wanted (chosen, with the HD layout in effect, or HD on in the Wide build: its
// windows show the text as the HD layout's regions do), and drawn (wanted and loaded).
const fontWanted = () => (WIDE ? hdOn : layoutOn()) && fontChoice === 'hd';
const fontOn = () => fontWanted() && fontFile === 'loaded';

// The font in effect and the file's state on the body (data-font, data-font-file).
function reflectFont() {
  put(document.body.dataset, 'font', fontDrawn ? 'hd' : 'pixel');
  put(document.body.dataset, 'fontFile', fontFile);
}

// Asks for the font file once (this origin's, index.html's @font-face) and measures it; then the
// game on screen shows its text in it at once (`applyHd`). A file that does not load leaves the
// pixel font for the visit, with a line that names nothing of the game (so does a browser with
// no document.fonts: the call is in the promise's chain, never thrown at the caller).
function loadFont() {
  if (fontFile !== 'none') return;
  fontFile = 'loading';
  const face = `100px "${FONT_FAMILY}"`;
  Promise.resolve()
    .then(() => document.fonts.load(face, '한'))
    .then((faces) => {
      if (faces.length === 0) throw new Error('no face');
      const ctx = document.createElement('canvas').getContext('2d');
      ctx.font = face;
      const m = ctx.measureText('한');
      glyphDrop = (m.actualBoundingBoxAscent - m.actualBoundingBoxDescent) / 200;
      fontFile = 'loaded';
    })
    .catch(() => {
      fontFile = 'failed';
      console.warn('the HD font did not load: the text stays in the pixel font');
    })
    .finally(applyHd);
}

// Whether `glyphs` (Game.hd_glyphs) lists a cell in the classic rect `from`.
function listedIn(glyphs, from) {
  for (let i = 0; i < glyphs.length; i += 5) {
    if (overlap([glyphs[i], glyphs[i + 1], GLYPH_CELL, GLYPH_CELL], from)) return true;
  }
  return false;
}

// The game's listed cells now (Game.hd_glyphs), the symbols' pixels with them (`symbolRows`).
function listedGlyphs() {
  const glyphs = game.hd_glyphs();
  symbolRows = glyphs.length > 0 ? game.hd_symbol_rows() : new Uint8Array(0);
  return glyphs;
}

// The runs of `glyphs` (Game.hd_glyphs: x, y, code, ink and paper per cell, in raster order)
// whose cells lie in the classic rect `from` (Design 3): listed cells of one row, each the next
// one's left neighbour, of one paper; one listed space joins the cells either side of it, two or
// more end the run (a space the player typed, GLYPH_TYPED, joins them whatever the spaces beside
// it: rev 3), and so does a cell not listed; a run's spaces at either end are left out. Each run:
// its row `y` and its cells [x, code, ink] left to right (a space's ink null: it goes with the
// text it stands in). A listed symbol (GLYPH_SYMBOL: Design 2 rev 3) ends the cells before it:
// after a run of them (directly, or after the spaces that follow it) it is that run's `caret`,
// else one of the returned list's `symbols`; each { x, y, ink, rows (its 8 rows from
// `symbolRows`, bit 7 leftmost), spaces (those between it and the run) }.
function glyphRuns(glyphs, from) {
  const runs = [];
  runs.symbols = [];
  // The cells being read: their row and paper, where the next would be, the spaces since the
  // last character (the game's `spaces`, all of them `gap`), the run open (null after two of the
  // game's spaces) and the last run of them.
  let chain = null;
  let symbols = 0; // the symbols of the whole list so far: their rows' place in `symbolRows`
  const end = () => {
    if (chain?.run) runs.push(chain.run);
    if (chain) chain.run = null;
  };
  for (let i = 0; i < glyphs.length; i += 5) {
    const [x, y, code, ink, paper] = [glyphs[i], glyphs[i + 1], glyphs[i + 2], glyphs[i + 3], glyphs[i + 4]];
    const symbol = code >= GLYPH_SYMBOL && code < GLYPH_TYPED;
    const at = symbol ? 8 * symbols : 0;
    if (symbol) symbols += 1;
    if (!overlap([x, y, GLYPH_CELL, GLYPH_CELL], from)) continue;
    if (!chain || y !== chain.y || x !== chain.next || paper !== chain.paper) {
      end();
      chain = { y, paper, next: x, spaces: 0, gap: 0, run: null, last: null };
    }
    chain.next = x + GLYPH_CELL;
    if (code === GLYPH_SPACE || code === (GLYPH_TYPED | GLYPH_SPACE)) {
      chain.gap += 1;
      if (code === GLYPH_SPACE) chain.spaces += 1;
      if (chain.spaces > 1) end();
      continue;
    }
    if (symbol) {
      const rows = symbolRows.length >= at + 8 ? symbolRows.subarray(at, at + 8) : null;
      const mark = { x, y, ink, rows, spaces: chain.gap };
      if (chain.last) chain.last.caret = mark;
      else runs.symbols.push(mark);
      end();
      chain = null;
      continue;
    }
    if (!chain.run) {
      chain.run = { y, cells: [], caret: null };
      chain.last = chain.run;
    } else {
      for (let k = chain.gap; k >= 1; k -= 1) chain.run.cells.push([x - k * GLYPH_CELL, GLYPH_SPACE, null]);
    }
    chain.spaces = 0;
    chain.gap = 0;
    chain.run.cells.push([x, code, ink]);
  }
  end();
  return runs;
}

// A listed symbol's pixels (`rows`: 8 rows, bit 7 leftmost) in its ink, its cell's top left at
// canvas (x, y), each game pixel sx × sy canvas pixels with its edges on the nearest whole canvas
// pixel (sharp, as the pixel font's blocks): one path, filled once.
function paintSymbol(ctx, { rows, ink }, x, y, sx, sy) {
  if (!rows) return;
  ctx.beginPath();
  for (let v = 0; v < GLYPH_CELL; v += 1) {
    for (let u = 0; u < GLYPH_CELL; u += 1) {
      if ((rows[v] & (0x80 >> u)) === 0) continue;
      const [x0, x1] = [Math.round(x + u * sx), Math.round(x + (u + 1) * sx)];
      const [y0, y1] = [Math.round(y + v * sy), Math.round(y + (v + 1) * sy)];
      ctx.rect(x0, y0, x1 - x0, y1 - y0);
    }
  }
  ctx.fillStyle = `#${ink.toString(16).padStart(6, '0')}`;
  ctx.fill();
}

// The runs of `glyphs` (`glyphRuns`) in the classic rect `from` that the canvas rect `to` shows,
// drawn there over the canvas rect `copied` alone (the part just copied from the ×4 frame, where
// their cells are their paper): each run one line of text from the left edge of its first cell's
// place (its ink from GLYPH_INSET inside that edge), at FONT_EM of the cell's height there, on
// the baseline `glyphDrop` puts, at the font's natural width without kerning, or narrowed to its
// cells' width less the inset at each end when wider (horizontally only: the ink stays within its
// cells); each character in its own ink, the parts of one ink drawn one after another, each where
// the text before it ends. A run's caret (Design 2 rev 3) is drawn after it: its cell's left edge
// where the run's text and the spaces after it end (their advances in the font, narrowed as the
// run is), on the run's row; a symbol after no run at its own cell (`paintSymbol`). A run is
// drawn only over pixels copied again just before (a region copied whole while it has
// characters: `paint`), so never twice over itself (its anti-aliased edges would thicken).
function paintGlyphs(ctx, glyphs, from, to, copied) {
  const [sx, sy] = [to[2] / from[2], to[3] / from[3]];
  const h = GLYPH_CELL * sy;
  const size = FONT_EM * h;
  let ready = false;
  const prepare = () => {
    if (ready) return;
    ctx.save();
    ctx.beginPath();
    ctx.rect(...copied);
    ctx.clip();
    ctx.font = `${size}px "${FONT_FAMILY}"`;
    ctx.textAlign = 'left';
    ctx.textBaseline = 'alphabetic';
    // No kerning (Design 3): a run's width is its characters' advances, so a part drawn on its
    // own stands where the whole text would put it.
    ctx.fontKerning = 'none';
    ready = true;
  };
  const runs = glyphRuns(glyphs, from);
  for (const { y, cells, caret } of runs) {
    const left = to[0] + (cells[0][0] - from[0]) * sx;
    const top = to[1] + (y - from[1]) * sy;
    const span = (cells.at(-1)[0] + GLYPH_CELL - cells[0][0]) * sx;
    // A caret stands within the cells up to its own.
    const reach = caret ? (caret.x + GLYPH_CELL - cells[0][0]) * sx : span;
    if (!overlap([left, top, reach, h], copied)) continue;
    prepare();
    // Every code is under 0x10000: one character of the string per cell. The pen starts
    // GLYPH_INSET inside the span, and further right as far as the first character's ink reaches
    // left of the pen, and the run is narrowed when its width (or its ink's) is wider than the
    // span less an inset either side: the ink, and its anti-aliased fringe, stay in the span.
    const text = cells.map(([, code]) => String.fromCharCode(code)).join('');
    const m = ctx.measureText(text);
    const lead = Math.max(0, m.actualBoundingBoxLeft);
    const natural = Math.max(m.width, lead + m.actualBoundingBoxRight);
    const room = span - 2 * GLYPH_INSET;
    const k = natural > room ? room / natural : 1;
    const pen = left + GLYPH_INSET + lead * k;
    ctx.setTransform(k, 0, 0, 1, pen, top + h / 2 + glyphDrop * size);
    for (let a = 0; a < cells.length;) {
      const ink = cells[a][2];
      let b = a + 1;
      while (b < cells.length && (cells[b][2] === null || cells[b][2] === ink)) b += 1;
      ctx.fillStyle = `#${ink.toString(16).padStart(6, '0')}`;
      ctx.fillText(text.slice(a, b), a === 0 ? 0 : ctx.measureText(text.slice(0, a)).width, 0);
      a = b;
    }
    if (caret) {
      const at = pen + ctx.measureText(text + ' '.repeat(caret.spaces)).width * k;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      paintSymbol(ctx, caret, at, top, sx, sy);
    }
  }
  for (const symbol of runs.symbols) {
    const [left, top] = [to[0] + (symbol.x - from[0]) * sx, to[1] + (symbol.y - from[1]) * sy];
    if (!overlap([left, top, GLYPH_CELL * sx, h], copied)) continue;
    prepare();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    paintSymbol(ctx, symbol, left, top, sx, sy);
  }
  if (ready) ctx.restore();
}

// The chooser's and Settings' "Font" changed: kept, on the game on screen at once.
function chooseFont(value) {
  fontChoice = value === 'pixel' ? 'pixel' : 'hd';
  try {
    localStorage.setItem(FONT_KEY, fontChoice);
  } catch {
    // No storage: the choice lasts until the page is left.
  }
  showHd();
  applyHd();
}

// --- the HD pack -------------------------------------------------------------------------------

let hdPack = null; // the HdPack loaded, set on every game put on screen; null without one

// Where the pack loaded came from: '' (none), DROPPED (a drop, or a pack kept from before the
// chooser) or the name of the listed pack it was downloaded as. The kept pack's source is kept
// beside it (SOURCE_KEY) for the next visit.
const DROPPED = ':dropped';
const SOURCE_KEY = 'u6.hdSource';
const PACK_NAME = /^\w[\w.-]*$/; // a listed pack's file: a plain name
let packSource = '';

// The source of the pack kept in this browser: DROPPED unless a listed name is kept with it.
function keptSource() {
  try {
    const source = localStorage.getItem(SOURCE_KEY);
    return source !== null && PACK_NAME.test(source) ? source : DROPPED;
  } catch {
    return DROPPED;
  }
}

function keepSource(source) {
  try {
    if (source) localStorage.setItem(SOURCE_KEY, source);
    else localStorage.removeItem(SOURCE_KEY);
  } catch {
    // No storage: a pack kept without its source comes back as a dropped one.
  }
}

// The pack's counts and the game's matches on the body (data-hd-pack, data-hd-matched).
function reflectPack() {
  document.body.dataset.hdPack = hdPack ? hdPack.counts().join(',') : 'none';
  document.body.dataset.hdPackBytes = String(hdPack ? hdPack.decoded_bytes() : 0);
  document.body.dataset.hdMatched = hdPack && game && !crashed ? game.hd_pack_matches().join(',') : '';
}

// Whether `file` is an HD pack, and if so why it is refused already ('' when nothing in its
// directory or manifest breaks the format): the engine names the parts to read (the end of the
// file, its zip directory, at most 192 KiB of its manifest.json) and tells from them.
async function probePack(file) {
  const probe = new PackProbe(file.size);
  try {
    for (let part = probe.need(); part.length > 0; part = probe.need()) {
      probe.give(new Uint8Array(await file.slice(part[0], part[1]).arrayBuffer()));
    }
    return { pack: probe.is_pack(), refusal: probe.refusal() };
  } finally {
    probe.free();
  }
}

// The status line of a loaded pack: counts only.
function packStatus() {
  const [tiles, portraits, panels] = hdPack.counts();
  if (!game || crashed) return t('packLoaded', tiles, portraits, panels);
  const [matchedTiles, matchedPortraits, matchedPanels] = game.hd_pack_matches();
  return t('packMatched', matchedTiles, matchedPortraits, matchedPanels, tiles, portraits, panels);
}

// `next` (an HdPack, from `source`) replaces the pack loaded, on the game on screen at once
// (drawn with no step), and on the chooser if it is open.
function attachPack(next, source = '') {
  if (game && !crashed) {
    if (next) game.set_hd_pack(next);
    else game.forget_hd_pack();
    draw();
  }
  hdPack?.free();
  hdPack = next;
  packSource = next ? source : '';
  reflectPack();
  if (!setup.hidden) showHdChoice();
}

// A dropped pack (`refusal`: why its directory or manifest already refuses it, '' if nothing
// does): read whole, checked and decoded by the engine, set on the game on screen, kept in this
// browser. A pack that does not read is refused and nothing changes; one too large or refused by
// its directory is refused before it is read whole. When the browser does not keep the new pack,
// the one kept before is removed too (else the next visit would bring it back unsaid). `source`:
// DROPPED, or the listed name it was downloaded as. Returns whether the pack was loaded.
async function loadPackFile(file, refusal, source = DROPPED) {
  needEngine();
  if (file.size > max_pack_bytes()) {
    say('packTooLarge', Math.floor(max_pack_bytes() / 2 ** 20));
    return false;
  }
  if (refusal) {
    say('packRefused', refusal);
    return false;
  }
  say('packReading');
  let bytes = new Uint8Array(await file.arrayBuffer());
  say('packChecking');
  await new Promise((resolve) => setTimeout(resolve, 0));
  let next;
  try {
    next = HdPack.load(bytes);
  } catch (err) {
    say('packRefused', message(err));
    return false;
  }
  attachPack(next, source);
  const kept = await storePack(bytes);
  bytes = null;
  let note = t('packKept');
  if (kept) keepSource(source);
  if (!kept) {
    let old = t('packOldGone');
    try {
      await forgetPack();
      keepSource('');
    } catch (err) {
      console.warn(`could not remove the HD pack kept before: ${message(err)}`);
      old = t('packOldStays');
    }
    note = t('packNotKept', old);
  }
  setStatus(`${packStatus()}${note}`);
  return true;
}

// The pack kept from the last visit, loaded before the first game is put on screen (so its first
// HD frame has it). One that no longer reads is left where it is and not used.
async function restorePack() {
  const bytes = await loadPack();
  if (!bytes) return;
  try {
    attachPack(HdPack.load(bytes), keptSource());
  } catch (err) {
    console.warn(`the kept HD pack is not used: ${message(err)}`);
  }
}

// "Forget the HD pack": off the page at once (MMPX again), then out of the browser's storage.
async function forgetHdPack() {
  attachPack(null);
  try {
    await forgetPack();
  } catch (err) {
    say('packForgetFailed', message(err));
    return;
  }
  keepSource('');
  say('packForgot');
}

// "HD on" changed: HD as it says, kept, on the game on screen at once.
function toggleHd() {
  hdOn = hdButton.checked;
  try {
    localStorage.setItem(HD_KEY, hdOn ? 'on' : 'off');
  } catch {
    // No storage: the choice lasts until the page is left.
  }
  showHd();
  applyHd();
}

// One display frame: the steps its wall time is worth at the chosen speed (`pace`; the first
// frame's time can precede `last`, which counts 0), run while the frame's budget lasts (half the
// display's interval, at least 3 ms; the first always) and until the sequence ends, then
// reported (`ran`). If any ran, the frame is converted and drawn once, when they changed it;
// the sound, a Save to store and the reflected state once per frame.
function frame(now) {
  if (leaving) {
    looping = false;
    return;
  }
  if (!game || crashed) {
    looping = false;
    followLine();
    return;
  }
  const dt = now - last;
  last = now;
  if (dt > 0 && dt <= LONG_FRAME_MS) interval += (dt - interval) / 8;
  try {
    game.set_clock(now);
    const due = game.pace(dt);
    const budget = budget_ms(interval);
    const start = performance.now();
    let k = 0;
    let done = false;
    while (k < due && !done && (k === 0 || performance.now() - start < budget)) {
      game.step(stepMs);
      k += 1;
      done = game.finished();
    }
    game.ran(k);
    const log = game.take_log();
    if (log) console.info(log);
    if (voice.ready) hearSpeech();
    if (k > 0) {
      game.present();
      draw();
    }
    feedAudio();
    checkSave();
    canvas.dataset.leader = game.leader_position().join(',');
    canvas.dataset.prompt = game.at_prompt() ? '1' : '0';
    put(canvas.dataset, 'program', game.program());
    showGamePointer();
    canvas.dataset.name = game.leader_name();
    canvas.dataset.keys = String(game.keys_pending());
    followLine();
    reflectLocaleCounts();
    paced(now, dt, k);
    if (done || game.finished()) {
      ended();
      return;
    }
  } catch (err) {
    stopOnTrap(err);
    return;
  }
  requestAnimationFrame(frame);
}

// The launcher's sequence ended (Quit in the menu): nothing runs any more; Restart starts it
// again from the kept files.
function ended() {
  looping = false;
  say('ended');
  restart.hidden = false;
  reflect('ended');
}

function startLoop() {
  if (looping) return;
  looping = true;
  last = performance.now();
  requestAnimationFrame(frame);
}

// A trap or exception inside the engine: its state is unknown, so it is never stepped again.
function stopOnTrap(err) {
  console.error(err);
  say('trap', message(err));
  crashed = true;
  looping = false;
  restart.hidden = false;
  reflect('crashed');
}

// Boots `next` (its files loaded; `origin` the key of the line that says where from); on success
// it replaces the running game. Returns the intake report, or null when it did not start (the
// status says why; the caller frees `next`).
async function boot(next, origin) {
  say(origin, next.files_loaded());
  // Let the status paint before the (synchronous) intake runs.
  await new Promise((resolve) => setTimeout(resolve, 0));
  let report;
  try {
    report = next.start(clockSeed());
  } catch (err) {
    say('cannotStart', message(err));
    return null;
  }
  const old = game;
  game = next;
  owner = next;
  next.set_voices(voice.ready);
  next.set_sight_memory(storedMemory());
  voice.newGame();
  old?.free();
  drawnGame = null; // the freed game's frame is not this one's
  drawnVersion = -1;
  saveSeen = 0;
  paceGame(next);
  try {
    attachAudio(next);
  } catch (err) {
    // No audio machine for this game (its MED tables fail the engine's checks): it runs silent.
    console.warn(`sound is off: ${message(err)}`);
  }
  if (hdPack) next.set_hd_pack(hdPack);
  attachLocale(next);
  applyHd();
  reflectPack();
  reflectLocale();
  restart.hidden = true;
  reflect('running');
  startLoop();
  return report;
}

// Copies the files `started` booted from out of wasm and stores them; true once committed.
async function keep(started) {
  let files = [];
  try {
    for (let i = 0; i < started.kept_count(); i += 1) {
      const path = started.kept_path(i);
      const ptr = started.kept_ptr(i);
      const len = started.kept_len(i);
      // Copy before the next call into wasm, which may grow (detach) its memory.
      files.push({ path, bytes: new Uint8Array(wasm.memory.buffer, ptr, len).slice() });
    }
    started.release_kept();
    return await saveFiles(files);
  } finally {
    files = null;
  }
}

// --- operations (each runs on the queue) ---------------------------------------------------

function needEngine() {
  if (!wasm) throw new Error(t('engineNotLoaded'));
  if (crashed) throw new Error(t('engineStopped'));
}

// Files of an original `savegame/` folder (the PLAYER disk's saved game).
const SAVE_PATH = /(^|\/)savegame\/[^/]+$/i;

// The save files of a drop, read, in the original's names; a file over the save limits (the
// engine's, which its start also applies to stored files) is left out before it is read.
// Returns the files and the status note on what was left out.
async function readSaves(saveItems) {
  const files = [];
  const left = [];
  let total = 0;
  for (const { path, file } of saveItems) {
    if (file.size > max_save_file_bytes() || total + file.size > max_save_bytes()) {
      left.push(path);
      continue;
    }
    say('reading', path);
    total += file.size;
    files.push({ path: `savegame/${path.split('/').pop()}`, bytes: new Uint8Array(await file.arrayBuffer()) });
  }
  const note = left.length === 0 ? ''
    : t('savesLeftOut', max_save_file_bytes(), max_save_bytes(), left.join(', '));
  return { files, note };
}

// A zip the engine reads in slices (`zip_sliced`): it names each part to read (the end of the
// file, the directory if it starts before that, then each disk-image entry) and checks it, and
// the page reads just that part. The engine collects the zip as one file of its disk images; a
// part that does not check out, or one that would pass the input limit, fails the import before
// it is read.
async function readZipSlices(next, path, file) {
  next.zip_open(path, file.size);
  for (let part = next.zip_need(); part.length > 0; part = next.zip_need()) {
    next.zip_give(new Uint8Array(await file.slice(part[0], part[1]).arrayBuffer()));
  }
}

// An import: `items` ({ path, file }) read one at a time into a new Game, booted, kept. The
// files of a `savegame/` folder among them are the save it starts from, and become the stored
// save (replacing the old one) once it started; a drop of only such a folder resumes it with
// the kept game files. It runs with the Saves paused (`queueImport`). The input limit is
// checked before anything is read for the files read whole, and again before each of them
// with what wasm already holds (the parts read of a zip read in slices included); a zip read in
// slices is checked part by part before each is read (only those parts count).
async function importFiles(dropped) {
  needEngine();
  const total = dropped.reduce(
    (sum, { path, file }) => sum + (zip_sliced(path, file.size) ? 0 : file.size), 0);
  if (dropped.length === 0) {
    say('noFiles');
    return;
  }
  if (total > max_input_bytes()) {
    setStatus(input_too_large(total));
    return;
  }
  const saveItems = dropped.filter(({ path }) => SAVE_PATH.test(path));
  const items = dropped.filter(({ path }) => !SAVE_PATH.test(path));
  let imported = null;
  let lead = '';
  if (saveItems.length > 0) {
    const read = await readSaves(saveItems);
    lead = read.note;
    if (read.files.length > 0) imported = read.files;
    if (items.length === 0) {
      if (imported) {
        await restoreKept(true, `${lead}${t('savesImported', imported.length)}`, imported);
      } else {
        setStatus(`${lead}${t('nothingImported')}`);
      }
      return;
    }
  }
  const next = new Game();
  let started = false;
  try {
    // The files still to be read whole, together.
    let whole = items.reduce(
      (sum, { path, file }) => sum + (zip_sliced(path, file.size) ? 0 : file.size), 0);
    for (const { path, file } of items) {
      if (zip_sliced(path, file.size)) {
        say('reading', path);
        await readZipSlices(next, path, file);
      } else {
        if (next.input_held() + whole > max_input_bytes()) {
          setStatus(input_too_large(next.input_held() + whole));
          return;
        }
        whole -= file.size;
        say('reading', path);
        // The JS copy lives only until wasm has its own (load_file copies it in).
        next.load_file(path, new Uint8Array(await file.arrayBuffer()));
      }
    }
    await attachSaves(next, imported);
    const report = await boot(next, 'bootRead');
    if (report === null) return;
    started = true;
    canvas.focus();
    const saved = imported ? await adoptSaves(imported) : '';
    const keptNow = await keep(next);
    if (keptNow) filesKept = true;
    reflect();
    const kept = keptNow ? t('filesKept', next.game_files()) : t('filesNotKept');
    const resumed = next.save_files() > 0 ? t('resumed') : '';
    setStatus(`${lead}${report}\n${t('started')}${resumed}${saved}${kept}${localeLine()}\n${t('howToPlay')}`);
  } finally {
    if (started) {
      if (!crashed) next.release_kept();
    } else {
      next.free();
    }
  }
}

// The restore: the kept files (if any) and the saved game (`imported`, a dropped folder, or
// the stored one) into a new Game, booted. `focus` when the user asked for it (Restart, an
// imported save); the restore at page load leaves the focus where it is. `note` goes first in
// the status. It runs at page load (no game yet) or inside an import (the Saves paused).
async function restoreKept(focus, note = '', imported = null) {
  needEngine();
  const lead = note ? `${note}\n` : '';
  let kept = await loadFiles();
  filesKept = Boolean(kept && kept.length > 0);
  reflect();
  if (!kept || kept.length === 0) {
    let after = '';
    if (imported) {
      // No game starts from it: it is the stored save now, and no longer the running game's.
      await storeSaves(imported, true);
      if (game) {
        owner = null;
        after = t('notSavedAnyMore');
      }
    }
    if (lead || after) setStatus(`${lead}${t('noFilesYet')}${after}`);
    else say('noFilesYet');
    return;
  }
  const next = new Game();
  let started = false;
  try {
    for (let i = 0; i < kept.length; i += 1) {
      next.load_file(kept[i].path, kept[i].bytes);
      kept[i] = null;
    }
    kept = null;
    await attachSaves(next, imported);
    const report = await boot(next, 'bootKept');
    if (report === null) return;
    started = true;
    next.release_kept();
    if (focus) canvas.focus();
    const saved = imported ? await adoptSaves(imported) : '';
    const what = t(next.save_files() > 0 ? 'keptResumed' : 'keptNew');
    setStatus(`${lead}${report}\n${t('startedKept', what)}${saved}${localeLine()}\n${t('howToPlay')}`);
  } finally {
    if (!started) next.free();
  }
}

// --- NPC bundles (spec 09 Design 5) ------------------------------------------------------

const BUNDLES_GONE = 'u6.npcBundlesGone'; // localStorage: '1' once the old bundles were removed

// The NPC bundles an older page kept (its "NPC bundles" button; u6-export makes them now) removed
// from this browser once, with no line: a failure only warns with its kind, and it is tried
// again on the next visit.
async function dropOldBundles() {
  if (stored(BUNDLES_GONE) === '1') return;
  try {
    await forgetBundles();
    remember(BUNDLES_GONE, '1');
  } catch (err) {
    console.warn(`could not remove the old NPC bundles: ${errorKind(err)}`);
  }
}

async function forgetKept() {
  await forgetFiles();
  filesKept = false;
  reflect();
  say(game ? 'forgotFilesRunning' : 'forgotFiles');
}

// The stored save removed: a save job taken before writes nothing, and the game on screen owns
// the empty store, its next Save listing the whole game (objlist and every block), so a start
// from it is the world that game had and not new-game blocks under its own. A Save it made
// before the forget and not yet stored (made while it did not own the store, or during an
// import just before) is not stored: only its Saves after the forget are.
async function forgetSaved() {
  await forgetSaves();
  forgets += 1;
  if (game && !crashed) {
    game.save_forgotten();
    owner = game;
    saveSeen = game.save_generation();
  }
  say('forgotSave');
}

// Runs `op` (an import, which may replace the running game) with no Save taken meanwhile.
async function pausingSaves(op) {
  pause += 1;
  try {
    return await op();
  } finally {
    pause -= 1;
  }
}

// A drop: a Korean pack, a voice pack or an HD pack alone is loaded as one; anything else is
// imported as game files. A drop holding a pack and other files (or two packs) is refused as a
// whole, nothing read further. A Korean pack is told by its name (*.u6lp) alone.
async function takeDrop(dropped) {
  needEngine();
  const koPacks = dropped.filter((item) => LOCALE_PACK_NAME.test(item.path));
  if (koPacks.length > 0) {
    if (dropped.length > 1) say('dropKoPackAndFiles');
    else await loadLocalePackFile(koPacks[0].file);
    return;
  }
  const voicePacks = [];
  for (const item of dropped) {
    const told = await probeVoicePack(item, dropped.length === 1);
    if (told.pack) voicePacks.push({ ...item, value: told.value });
  }
  if (voicePacks.length > 0) {
    if (dropped.length > 1) say('dropVoicePackAndFiles');
    else await loadVoicePackFile(voicePacks[0].file, voicePacks[0].value);
    return;
  }
  const packs = [];
  for (const item of dropped) {
    if (!/\.zip$/i.test(item.path)) continue;
    const told = await probePack(item.file);
    if (told.pack) packs.push({ ...item, refusal: told.refusal });
  }
  if (packs.length === 0) {
    await importFiles(dropped);
  } else if (dropped.length > 1) {
    say(packs.length > 1 ? 'dropTwoPacks' : 'dropPackAndFiles');
  } else {
    await loadPackFile(packs[0].file, packs[0].refusal);
  }
}

function queueImport(gather) {
  if (queued > 0) say('waiting');
  enqueue('opLoad', () => pausingSaves(async () => takeDrop(await gather())));
}

// --- the chooser -----------------------------------------------------------------------------

const OLD_KEY = 'u6.packKey'; // localStorage: the key an older page kept for the packs (none needed now)

// An old link's key (`?k=`) taken off the address bar (and so off the history and a copied
// link), and the key an older page kept removed: the packs are asked with none.
function dropOldKey() {
  const url = new URL(location.href);
  if (url.searchParams.has('k')) {
    url.searchParams.delete('k');
    history.replaceState(history.state, '', `${url.pathname}${url.search}${url.hash}`);
  }
  remember(OLD_KEY, null);
}

dropOldKey();

// /packs/`name`, never cached by the browser (it is kept in IndexedDB once loaded).
function packFetch(name) {
  return fetch(`/packs/${encodeURIComponent(name)}`, { cache: 'no-store' });
}

// What the site lists (/packs/index.json: {"packs": [{"file", "label"}], "ko": {"file",
// "label"}, "voices": {"file", "label"}}), asked once a visit: `packs`, the HD packs ([] when it
// lists none: a 404, anything that does not read; a file is a plain name, a label is text),
// `ko`, the Korean pack ({file}: a plain name ending .u6lp; its label is not shown), and
// `voices`, the voice pack ({file}: a plain name ending .u6vp; its label is not shown), each null
// when none is listed or its entry is not of that form.
let packList = null;
function packIndex() {
  packList ??= (async () => {
    try {
      const response = await packFetch('index.json');
      if (!response.ok) return { packs: [], ko: null, voices: null };
      const { packs, ko, voices } = await response.json();
      const listed = {
        packs: (Array.isArray(packs) ? packs : [])
          .filter((p) => typeof p?.file === 'string' && PACK_NAME.test(p.file))
          .map((p) => ({ file: p.file, label: typeof p.label === 'string' && p.label ? p.label : p.file })),
        ko: typeof ko?.file === 'string' && PACK_NAME.test(ko.file) && LOCALE_PACK_NAME.test(ko.file) ? { file: ko.file } : null,
        voices: typeof voices?.file === 'string' && PACK_NAME.test(voices.file) && VOICE_PACK_NAME.test(voices.file)
          ? { file: voices.file } : null,
      };
      return listed;
    } catch {
      return { packs: [], ko: null, voices: null };
    }
  })().then((listed) => {
    koListed = listed.ko !== null;
    reflectLocale();
    return listed;
  });
  return packList;
}

async function listPacks() {
  return (await packIndex()).packs;
}

// Whether the chooser shows the first run's defaults (no choice kept, or one an older page kept:
// `setupStale`), until its OK keeps a choice.
let setupFirst = false;

// OK waits for the chooser's list (`setupListing`: the `showPacks` in flight) and for the OK
// before it, on the queue (`setupApplying`): a press before the list is in would take no HD entry.
let setupListing = 0;
let setupApplying = false;
function reflectSetupOk() {
  setupOk.disabled = setupListing > 0 || setupApplying;
}

// The chooser's HD entries checked as the pack active now: MMPX for none (on the first run none
// yet: `showPacks` checks the first pack listed, once the list is in), "Dropped pack" (shown only
// then) for a dropped one, the listed one it was downloaded as (none checked when that one is
// not listed: OK leaves the HD as it is).
function showHdChoice() {
  setupDropped.hidden = packSource !== DROPPED;
  const active = setupFirst && packSource === '' ? null : packSource;
  for (const input of setupHd.querySelectorAll('input')) input.checked = input.value === active;
}

// The chooser's listed HD entries, after MMPX (and "Dropped pack"): each pack listed, by its label
// (as text); the active one checked unless another entry was checked meanwhile, and on the first
// run with no pack active the first one listed (MMPX when none is). Its data-packs is the count
// listed, once the list is in (for scripts and tests). OK is disabled until then, whatever the
// listing gave (none, when it failed).
async function showPacks() {
  setupListing += 1;
  reflectSetupOk();
  try {
    const packs = await listPacks();
    for (const row of setupHd.querySelectorAll('label[data-pack]')) row.remove();
    for (const { file, label } of packs) {
      const input = Object.assign(document.createElement('input'), { type: 'radio', name: 'setup-hd', value: file });
      const text = document.createElement('span');
      text.textContent = label;
      const row = document.createElement('label');
      row.dataset.pack = file;
      row.append(input, ' ', text);
      setupHd.append(row);
      if (file === packSource && !setupHd.querySelector('input:checked')) input.checked = true;
    }
    if (setupFirst && packSource === '' && !setupHd.querySelector('input:checked')) {
      (setupHd.querySelector('label[data-pack] input') ?? setupHd.querySelector('input[value=""]')).checked = true;
    }
    setupHd.dataset.packs = String(packs.length);
  } finally {
    setupListing -= 1;
    reflectSetupOk();
  }
}

// The chooser's NPC voices as checked: the model's use restrictions shown, and OK read as their
// acceptance, only while it is (a switch line shown names OK as it reads now).
function showVoiceTerms() {
  setupTerms.hidden = !setupVoices.checked;
  setupOk.dataset.i18n = setupVoices.checked ? 'acceptStart' : 'ok';
  setupOk.textContent = t(setupOk.dataset.i18n);
  refreshSwitchLine();
}

// Opens the chooser on the language, the HD choice and the voices now (NPC voices checked on the
// first run, else while the voices are on), its line empty unless a switch to the other build is
// pending (noted, or under way): then the switch line, on the chooser's line only (the status
// line keeps what it says: a wait, a download).
function openSetup() {
  for (const input of setup.querySelectorAll('input[name="setup-lang"]')) input.checked = input.value === lang;
  setup.hidden = false;
  showHdChoice();
  setupVoices.checked = setupFirst || voiceIsOn();
  setupRestrictions.textContent = RESTRICTIONS;
  showVoiceTerms();
  if (switchTo !== null) {
    showSwitchOnChooser();
  } else {
    setupStatus.textContent = '';
    switchShown = false;
  }
  showPacks();
}

// Keeps the choice made (the language now) and closes the chooser.
function keepSetup() {
  try {
    localStorage.setItem(SETUP_KEY, JSON.stringify({ lang, version: SETUP_VERSION }));
  } catch {
    // No storage: the choice lasts until the page is left.
  }
  setupFirst = false;
  setup.hidden = true;
}

// A listed pack: downloaded (its progress on the status line), then loaded and kept as a dropped
// one (`loadPackFile`). Returns whether it was loaded.
async function downloadPack(name) {
  needEngine();
  const limit = max_pack_bytes();
  const tooLarge = () => say('packTooLarge', Math.floor(limit / 2 ** 20));
  say('packDownload', 0, 0);
  const parts = [];
  try {
    const response = await packFetch(name);
    if (!response.ok) {
      say('packNotDownloaded', `HTTP ${response.status}`);
      return false;
    }
    const total = Number(response.headers.get('Content-Length')) || 0;
    if (total > limit) {
      await response.body?.cancel();
      tooLarge();
      return false;
    }
    const reader = response.body.getReader();
    let got = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      got += value.length;
      if (got > limit) {
        await reader.cancel();
        tooLarge();
        return false;
      }
      parts.push(value);
      say('packDownload', got, total);
    }
  } catch (err) {
    say('packNotDownloaded', message(err));
    return false;
  }
  const file = new File([new Blob(parts)], name);
  const told = await probePack(file);
  if (!told.pack) {
    say('packNotAPack');
    return false;
  }
  return loadPackFile(file, told.refusal, name);
}

// OK: the voices as NPC voices says, at once (`applyVoices`); the language is kept, and the HD
// entry checked becomes the active pack, on the queue (so after a restore or a load queued
// before): the active pack's own entry (or none checked) changes nothing; MMPX removes the active
// pack, whatever its source; another listed pack is downloaded and loaded, the chooser open and
// saying what the status line says until it is (it stays open with the reason when it is not).
// Then, the chooser closed and its choices kept, a switch to the other build noted by "Screen
// layout" (`switchBuild`).
function applySetup() {
  const chosen = setupHd.querySelector('input:checked')?.value;
  applyVoices();
  setupApplying = true;
  reflectSetupOk();
  enqueue('opPack', async () => {
    mirror = true;
    try {
      if (chosen === undefined || chosen === packSource || chosen === DROPPED) {
        keepSetup();
      } else if (chosen === '') {
        await forgetHdPack();
        keepSetup();
      } else if (await downloadPack(chosen)) {
        keepSetup();
      }
      // Spec 13 Decision 2: a switch to the other build chosen in the chooser, once it kept
      // the choices above (the language, the voices, the pack): the page loads again into it
      // (which starts the game in the language kept, so none of the restarts below).
      if (setup.hidden && switchTo !== null) {
        switchBuild();
        return;
      }
      // Spec 08 K6: in 한글 the site's Korean pack, when it should be loaded, restarts the game on
      // screen for it as a drop does.
      if (setup.hidden && lang === 'ko') {
        const hosted = await hostedLocalePack();
        if (hosted?.loaded) await restartForLocale(hosted.line, true);
      }
      // Spec 08: a switch of the language the game on screen shows the Korean pack in or not
      // restarts it from the kept files (the save untouched).
      if (setup.hidden && game && !crashed && (localeShown !== 'off') !== wantLocale()) {
        await pausingSaves(() => restoreKept(false));
      }
    } catch (err) {
      console.error(err);
      setStatus(t('failed', t('opPack'), message(err)));
    } finally {
      mirror = false;
      setupApplying = false;
      reflectSetupOk();
    }
  });
}

// NPC voices at OK: checked, the model's use restrictions the chooser shows are accepted and the
// voices start, exactly as at Accept on the Voices terms (the acceptance kept, the model
// downloaded, warmed up, ready; on the CPU without WebGPU), unless they are on already;
// unchecked, voices that are on go off, and nothing is asked of /models/.
function applyVoices() {
  if (setupVoices.checked && !voiceIsOn()) {
    gesture();
    acceptTerms();
  } else if (!setupVoices.checked && voiceIsOn()) {
    voiceOff();
    reflectVoice();
  }
}

// --- typing Korean (spec 08 Design 4, slice K5a) -------------------------------------------

const OWN_LINE = 7; // Game.keys_owned's three bits: a line being typed
// A plain Enter keydown this soon after the Enter a commit sent is taken for the same press,
// should a platform deliver it again once the IME let it go (not measured here: the page tests
// emulate the IME through CDP, which sends no second keydown).
const ENTER_AGAIN_MS = 100;
let ime = null; // the hidden input, while the game on screen has the pack
let lineOpen = false; // the page moved the focus to it for the line being read
let composing = false; // a composition is open in it
let enterAfterCommit = false; // Enter came during the composition: it goes after the commit
let dropCommit = false; // Escape cancelled the composition: a late commit of it is dropped
let enterSentAt = -Infinity; // the event time of the Enter a commit sent
// The focus came back to the canvas from the hidden input by the keyboard (Shift+Tab) during the
// line: it stays there (the canvas takes the keys as in English), so the focus is not trapped.
let keepCanvas = false;

// Committed text to the game (never logged or kept).
function sendText(text) {
  if (lineOpen && text && game && !crashed) game.text_input(text);
}

// Escape while composing: the composition goes, nothing of it reaches the game (setting the
// value cancels it in Chrome without a compositionend; an IME that commits on Escape anyway
// has that commit dropped).
function cancelComposition() {
  composing = false;
  enterAfterCommit = false;
  dropCommit = true;
  ime.value = '';
}

// A key pressed in the hidden input: while composing (or a key the IME takes, keyCode 229)
// the IME's, Enter marked to follow the commit and Escape cancelling the composition; else as
// on the canvas (`gameKey`), but for the second Enter of a press a commit already sent.
function imeKey(e) {
  if (composing || e.isComposing) {
    if (e.key === 'Enter') enterAfterCommit = true;
    else if (e.key === 'Escape') {
      e.preventDefault();
      cancelComposition();
    }
    return;
  }
  if (e.keyCode === 229) return;
  if (e.key === 'Enter' && e.timeStamp - enterSentAt < ENTER_AGAIN_MS) {
    enterSentAt = -Infinity;
    e.preventDefault();
    return;
  }
  gameKey(e);
}

function makeIme() {
  ime = document.createElement('input');
  ime.id = 'ime';
  ime.type = 'text';
  ime.lang = 'ko';
  ime.tabIndex = -1; // in the tab order only while a line is open (`followLine`)
  ime.autocomplete = 'off';
  ime.spellcheck = false;
  ime.setAttribute('autocapitalize', 'off');
  ime.dataset.i18nLabel = 'imeLabel';
  ime.setAttribute('aria-label', t('imeLabel'));
  ime.addEventListener('keydown', imeKey);
  ime.addEventListener('blur', releaseKeys);
  ime.addEventListener('compositionstart', () => {
    composing = true;
    dropCommit = false;
  });
  ime.addEventListener('compositionend', (e) => {
    composing = false;
    if (dropCommit) {
      dropCommit = false;
      return;
    }
    sendText(e.data);
    if (enterAfterCommit && lineOpen && game && !crashed) {
      enterAfterCommit = false;
      enterSentAt = e.timeStamp;
      voice.cancel();
      game.key_down('Enter', 'Enter', false, e.timeStamp);
    }
    ime.value = '';
  });
  // Text put in without a composition (an IME's punctuation, an emulated commit).
  ime.addEventListener('input', (e) => {
    composing = e.isComposing;
    if (e.inputType === 'insertText' && !e.isComposing) sendText(e.data);
    if (!composing) ime.value = '';
  });
  canvas.after(ime);
}

// The line ended (or the input goes): the focus back to the canvas if the input had it, a
// composition left open dropped with it.
function closeLine() {
  const had = document.activeElement === ime;
  lineOpen = false;
  ime.tabIndex = -1;
  composing = false;
  enterAfterCommit = false;
  if (had) canvas.focus({ preventScroll: true });
  keepCanvas = false;
  ime.value = '';
}

// Once per frame: the hidden input exists while the game on screen has the pack, and has the
// focus while the game reads a line and the canvas (or the input) has it, unless the keyboard
// moved it back to the canvas (`keepCanvas`). The line opens with the focus on the canvas or
// already on the input (a script gave it, the keys before the line went through it); the input
// is in the tab order only while the line is open, so Tab from the canvas passes it by outside.
function followLine() {
  const wanted = game && !crashed && localeShown === 'on';
  if (!wanted) {
    if (ime) {
      closeLine();
      ime.remove();
      ime = null;
    }
    return;
  }
  if (!ime) makeIme();
  const line = (game.keys_owned() & OWN_LINE) === OWN_LINE;
  const at = document.activeElement;
  if (line && ((at === canvas && !keepCanvas) || at === ime)) {
    lineOpen = true;
    ime.tabIndex = 0;
    if (at !== ime) ime.focus({ preventScroll: true });
  } else if (!line && lineOpen) {
    closeLine();
  }
}

// --- fullscreen ------------------------------------------------------------------------------

const EXIT_SHOWN_MS = 2000; // the Exit button stays this long after the pointer last moved
let cssFull = false; // the page's own fullscreen (a class), where the browser's is not to be had
let exitTimer = 0;
let shownFull = false; // fullscreen as `showFullscreen` last showed it

const fullNow = () => cssFull || document.fullscreenElement === screenBox;

// The toolbar's button as the fullscreen is (its label, pressed), the class, data-fullscreen;
// out of it, no Exit button. When it has just ended (by any way: the Exit button, the toolbar's,
// the browser's own), the canvas takes the focus back, since keys reach the game only from it
// (`followLine` hands it on to the hidden input while a line is read).
function showFullscreen() {
  const full = fullNow();
  const ended = shownFull && !full;
  shownFull = full;
  fullscreenButton.dataset.i18n = full ? 'exitFullscreen' : 'fullscreen';
  fullscreenButton.textContent = t(fullscreenButton.dataset.i18n);
  fullscreenButton.setAttribute('aria-pressed', String(full));
  screenBox.classList.toggle('full', cssFull);
  document.body.dataset.fullscreen = !full ? 'off' : cssFull ? 'css' : 'on';
  if (!full) {
    clearTimeout(exitTimer);
    fullscreenExit.hidden = true;
  }
  if (ended) canvas.focus({ preventScroll: true });
}

// The Exit button shown for EXIT_SHOWN_MS more, while fullscreen.
function showExit() {
  if (!fullNow()) return;
  fullscreenExit.hidden = false;
  clearTimeout(exitTimer);
  exitTimer = setTimeout(() => {
    fullscreenExit.hidden = true;
  }, EXIT_SHOWN_MS);
}

// The screen box fullscreen: the browser's (Escape locked to the game where it can be), or the
// page's own where the browser's cannot be asked for or is refused; the canvas takes the focus.
async function enterFullscreen() {
  let browsers = false;
  if (typeof screenBox.requestFullscreen === 'function') {
    try {
      await screenBox.requestFullscreen({ navigationUI: 'hide' });
      browsers = true;
    } catch (err) {
      console.warn(`fullscreen refused: ${errorKind(err)}`);
    }
  }
  cssFull = !browsers;
  if (browsers) {
    try {
      await navigator.keyboard?.lock?.(['Escape']);
    } catch {
      // No lock: the browser's Escape leaves fullscreen.
    }
  }
  showFullscreen();
  showExit();
  canvas.focus({ preventScroll: true });
}

// Out of fullscreen, whichever it is (the browser's says so by its fullscreenchange).
async function leaveFullscreen() {
  if (cssFull) {
    cssFull = false;
    showFullscreen();
  } else if (document.fullscreenElement) {
    try {
      await document.exitFullscreen();
    } catch (err) {
      console.warn(`fullscreen not left: ${errorKind(err)}`);
    }
  }
}

// --- wiring --------------------------------------------------------------------------------

// Keys go to the engine only from the focused canvas (or, while a line is typed in 한글 with the
// pack, the hidden input in its place) with a game running; everything else (Tab, keys of other
// elements, modifier shortcuts) keeps its browser meaning. Ctrl + the letters of the game's
// control commands (Ctrl-S save, Ctrl-R restore, ...) are game keys there, so the browser's own
// Save page / Reload do not fire. The speed keys (`speedKey`) are the page's and never reach the
// game. The browser's auto-repeat goes as repeats (spec 19 O1b), which the key's release takes
// back. Each press goes with its event's time (`timeStamp`, the animation frames' timeline: the
// held walk's interval, spec 19 O1c).
function gameKey(e) {
  gesture();
  if (!game || crashed) return;
  if (e.metaKey) {
    // A chord with Meta is not the game's, and its keyup may never come (macOS sends none for a
    // key let go while Cmd is down): every held key is let go (spec 19 O1c).
    releaseKeys();
    return;
  }
  // Spec 19 O1c: while Ctrl or Alt is down a held walk pauses, as in the native window; nothing
  // is let go. A chord with Alt is not the game's (the browser's Alt shortcuts).
  game.set_modifiers(e.ctrlKey || e.altKey);
  if (e.altKey) return;
  if (VOICE_CANCEL_KEYS.has(e.key)) voice.cancel();
  const by = speedKey(e);
  if (by !== 0) {
    e.preventDefault();
    changeSpeed(by);
  } else if (
    e.repeat
      ? game.key_repeat(e.code, e.key, e.ctrlKey, e.timeStamp)
      : game.key_down(e.code, e.key, e.ctrlKey, e.timeStamp)
  ) {
    e.preventDefault();
  }
}
// A key let go (spec 19 O1b): outside the line editor its repeats still queued are taken back,
// so a held key walks no further than the prompt it was let go at. The game releases what that
// physical key gave (`code`), whatever the modifiers are now; a key it never had takes nothing.
// Heard on the window, so a release after the focus moved to another control still counts; when
// the window loses the focus or the page is hidden, a release may never come, and when the canvas
// (or the line input in its place) loses the focus a held key must walk no further (spec 19 O1c:
// the context menu, a toolbar control, Settings, Tab): every held key is let go then.
function gameKeyUp(e) {
  if (!game || crashed) return;
  game.set_modifiers(e.ctrlKey || e.altKey);
  game.key_up(e.code);
}
function releaseKeys() {
  if (game && !crashed) game.release_keys();
}
canvas.addEventListener('keydown', gameKey);
window.addEventListener('keyup', gameKeyUp);
window.addEventListener('blur', releaseKeys);
canvas.addEventListener('blur', releaseKeys);
document.addEventListener('visibilitychange', () => {
  if (document.hidden) releaseKeys();
});
canvas.addEventListener('focus', (e) => {
  keepCanvas = ime !== null && e.relatedTarget === ime && lineOpen;
});
mute.addEventListener('click', () => {
  setMuted(!audio.muted);
  gesture();
});
hdButton.addEventListener('change', toggleHd);
for (const input of layoutChoices) input.addEventListener('change', () => chooseLayout(input.value));
for (const input of fontChoices) input.addEventListener('change', () => chooseFont(input.value));
settings.addEventListener('click', () => {
  openSetup();
  setup.querySelector('input[name="setup-lang"]:checked')?.focus();
});
for (const input of setup.querySelectorAll('input[name="setup-lang"]')) {
  input.addEventListener('change', () => setLang(input.value));
}
setupVoices.addEventListener('change', showVoiceTerms);
setupOk.addEventListener('click', applySetup);
// A box that grows or shrinks (the window, the zoom) may want the other scale, and the Wide
// layout's canvas (HD on or off) the other rendering.
new ResizeObserver(() => {
  if (hdOn || WIDE) applyHd();
}).observe(canvas);
window.addEventListener('resize', () => {
  if (hdOn || WIDE) applyHd();
});
// A move to a display of another pixel ratio changes the device pixels of the same CSS box
// (no resize): one query per ratio, armed again for the new one.
function watchRatio() {
  matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`).addEventListener('change', () => {
    if (hdOn || WIDE) applyHd();
    watchRatio();
  }, { once: true });
}
watchRatio();
slower.addEventListener('click', () => changeSpeed(-1));
faster.addEventListener('click', () => changeSpeed(1));
// The pointer: the position through the canvas's CSS box onto the game's 320 x 200 pixels
// (whatever the canvas's own size: HD makes it S times that), the buttons
// as DOM `buttons` (bit 0 left, bit 1 right). With the HD layout the position goes through the
// canvas's pixels and the layout backwards (`layoutToGame`, held when a button is down or was
// down in the event sent last: a press, a release, a move with a button held); in Wide a press
// on a title strip also drags its window (`dragWindow`). The canvas captures a press so the
// release reaches
// the game even off the canvas; the context menu stays off it while a game runs. A press is, in
// this order, the focus, the game's pointer event (queued for the next step, exactly once) and
// the gesture that starts the sound (the AudioContext is created before its first await, inside
// the press's user activation); the sound starting never delays or drops the event.
// The game pixel client point (x, y) reaches through the canvas's CSS box `box` (whatever the
// canvas's own size), `held` as `layoutToGame`, which sets `pointerGap` (here too for the frame
// as it is: never a gap).
function pixelAt(x, y, box, held) {
  if (shownView === FRAME) {
    pointerGap = false;
    return [Math.floor(((x - box.left) * game.width()) / box.width),
      Math.floor(((y - box.top) * game.height()) / box.height)];
  }
  const px = ((x - box.left) * canvas.width) / box.width;
  const py = ((y - box.top) * canvas.height) / box.height;
  return layoutToGame(px, py, held);
}
// The game pixel of pointer event `e` (null on a canvas with no box), as `sendPointer` sends it.
function pointerPixel(e) {
  const box = canvas.getBoundingClientRect();
  if (box.width === 0 || box.height === 0) return null;
  const buttons = e.buttons & 3;
  if (WIDE && shownView !== FRAME) {
    const px = ((e.clientX - box.left) * canvas.width) / box.width;
    const py = ((e.clientY - box.top) * canvas.height) / box.height;
    if (dragWindow(e, px, py)) return [...WIDE_REST];
  }
  return pixelAt(e.clientX, e.clientY, box, buttons !== 0 || sentButtons !== 0);
}

// The system's pointer over the canvas (index.html: hidden while `game` runs, where the game's
// own is under it): the mouse's last place (`mouseAt`), from the mouse's own pointer events only
// (a touch or a pen moves the game's pointer, never the mouse); forgotten when the mouse leaves
// the window, the window loses the focus or the page is hidden (it may come back elsewhere).
// The page's elements over the canvas that cover the game's pointer while the canvas's cursor
// shows over them: the speed badge, which lets the pointer through to the canvas, and the Exit
// button, under which a press held on the canvas (captured: every press is, `pointerdown`) moves
// on, the canvas's cursor shown and the game given each move (the hidden input never covers it:
// opacity 0; the context menu takes the pointer, a press on the canvas closes it first).
const OVER_CANVAS = [badge, fullscreenExit];
for (const kind of ['pointerdown', 'pointermove', 'pointerup']) {
  window.addEventListener(kind, (e) => {
    if (e.pointerType === 'mouse') mouseAt = [e.clientX, e.clientY];
  }, { capture: true });
}
const forgetMouse = () => {
  mouseAt = null;
  showGamePointer();
};
window.addEventListener('pointerout', (e) => {
  if (e.pointerType === 'mouse' && e.relatedTarget === null) forgetMouse();
}, { capture: true });
window.addEventListener('blur', forgetMouse);
document.addEventListener('visibilitychange', () => {
  if (document.hidden) forgetMouse();
});

// Where the game's pointer is against the system's at client point (x, y), as the page and the
// engine show now: `game`, the engine's sprite drawn (Game.game_pointer: shown) at the very game
// pixel the point reaches, the point on the canvas, under none of OVER_CANVAS, in a part of the
// game (not a gap or a margin of the HD layout, a letterbox side, a Wide window's frame or title
// strip: `pointerGap`), and no press the context menu takes held; `hold`, the same but for a
// pointer event at that pixel queued for the next step (the mouse's own move, which the sprite
// takes at that step: the cursor stays as it is, so a move never flickers it), for one frame at
// most: the frame's end decides again from where the step put the sprite (where a context-menu
// entry's target press, queued at that step after the move, put it elsewhere, `page` from
// then; the cursor was hidden over the sprite's old place for that one frame); else `page`. The
// engine decides where the sprite is, whatever moved it (the page's moves, a context-menu entry's
// presses, a window dragged: the rest of the screen), so a layout, a view or a box changed under
// a still mouse shows the system's pointer until the mouse moves again.
function gameUnder(x, y) {
  if (!game || crashed || leaving || swallowing) return 'page';
  const inside = (r) => x >= r.left && x < r.right && y >= r.top && y < r.bottom;
  const box = canvas.getBoundingClientRect();
  if (!inside(box)) return 'page';
  if (OVER_CANVAS.some((el) => !el.hidden && inside(el.getBoundingClientRect()))) return 'page';
  const [gx, gy] = pixelAt(x, y, box, false);
  if (pointerGap) return 'page';
  const [ex, ey, shown, queued, qx, qy] = game.game_pointer();
  if (!shown) return 'page';
  if (queued) return qx === gx && qy === gy ? 'hold' : 'page';
  return ex === gx && ey === gy ? 'game' : 'page';
}

// The canvas's data-pointer, which the stylesheet reads: `game` while the game's pointer is
// under the system's at the mouse's last place (`gameUnder`), else `page`, as it was while the
// mouse's own move waits for a step. Set at each pointer event the page handles and at the end
// of each frame of the loop, after its steps, so a change under a still mouse (the menu closed by
// a key, a layout, a view or fullscreen in or out, the program, a window or the badge shown or
// hidden, the engine moving or hiding its pointer) shows by the next frame.
function showGamePointer() {
  const under = mouseAt ? gameUnder(...mouseAt) : 'page';
  if (under !== 'hold') put(canvas.dataset, 'pointer', under);
}

function sendPointer(e) {
  if (!game || crashed) return;
  if (swallowing) {
    // The press that opened the context menu, or the outside press that closed it: none of it
    // reaches the game, up to its release.
    if ((e.buttons & 3) === 0) swallowing = false;
    showGamePointer();
    return;
  }
  const at = pointerPixel(e);
  if (!at) return;
  const buttons = e.buttons & 3;
  sentButtons = buttons;
  game.pointer(at[0], at[1], buttons);
  showGamePointer();
}
canvas.addEventListener('pointerdown', (e) => {
  canvas.focus();
  keepCanvas = false; // a press on the canvas gives a line being read the hidden input again
  if (game && !crashed) canvas.setPointerCapture(e.pointerId);
  if (!openContextMenu(e)) sendPointer(e);
  gesture();
});
canvas.addEventListener('pointermove', sendPointer);
canvas.addEventListener('pointerup', sendPointer);
canvas.addEventListener('pointercancel', (e) =>
  sendPointer({ type: e.type, clientX: e.clientX, clientY: e.clientY, buttons: 0 }));
canvas.addEventListener('contextmenu', (e) => {
  if (game && !crashed) e.preventDefault();
});

// --- the context menu (spec 17) -----------------------------------------------------------------
// A right press (the left button up) at the game's prompt on the map, or on an inventory or
// equipment slot, opens a menu of what the commands can do with what is there
// (Game.context_menu, at the press's game pixel); anywhere else, or with no prompt, the press
// goes to the game as before. The menu lies in the screen box at the press, flipped and kept
// inside it, and has the focus: arrows, Home and End move, Enter or Space chooses, a command's
// letter chooses its first entry that can run, Escape or a press outside closes it; none of
// these reach the game, nor does the press that opened it, held and moved, up to its release.
// A choice runs as the original's mouse does (Game.command_at: its icon, then the target once the
// command asks for it); an inventory slot's own action (ready, open, take off) is a left click
// there. The words are the page's; the names are the game's, given at run time.
const COMMAND_WORDS = {
  0x81: 'cmdAttack', 0x82: 'cmdCast', 0x83: 'cmdTalk', 0x84: 'cmdLook', 0x85: 'cmdGet', 0x86: 'cmdDrop',
  0x87: 'cmdMove', 0x88: 'cmdUse', 0x89: 'cmdRest', 0x8a: 'cmdCombat', 0xa0: 'cmdOpen', 0xa7: 'cmdReady',
  0xa8: 'cmdUnready',
};
const COMMAND_LETTERS = { a: 0x81, c: 0x82, t: 0x83, l: 0x84, g: 0x85, d: 0x86, m: 0x87, u: 0x88 };
// An inventory slot's own left-press actions: open, ready, take off.
const SLOT_ACTIONS = new Set([0xa0, 0xa7, 0xa8]);
let swallowing = false; // the pointer's events up to its release are the menu's
let menuAt = null; // the game pixel the open menu is for, null while it is closed

// The reason a disabled entry shows: its code's line ('far' → whyFar).
const reasonLine = (code) => t(`why${code[0].toUpperCase()}${code.slice(1)}`);

function menuItem(entry, name) {
  const item = document.createElement('button');
  item.type = 'button';
  item.setAttribute('role', 'menuitem');
  item.dataset.code = String(entry.code);
  const label = document.createElement('span');
  label.className = 'label';
  label.textContent = t(COMMAND_WORDS[entry.code]);
  item.append(label);
  if (name) {
    const of = document.createElement('span');
    of.className = 'of';
    of.textContent = name;
    item.append(of);
  }
  if (entry.theft) {
    const theft = document.createElement('span');
    theft.className = 'theft';
    theft.textContent = t('theft');
    item.append(theft);
  }
  if (!entry.enabled) {
    item.setAttribute('aria-disabled', 'true');
    const why = document.createElement('span');
    why.className = 'why';
    why.textContent = entry.reason ? reasonLine(entry.reason) : '';
    item.append(why);
  }
  item.addEventListener('click', () => chooseEntry(item));
  return item;
}

// Opens the menu for pointer event `e` when it is a right press the game has a menu for; false
// otherwise (the press then goes to the game).
function openContextMenu(e) {
  if (!game || crashed || (e.buttons & 3) !== 2) return false;
  const at = pointerPixel(e);
  if (!at) return false;
  const json = game.context_menu(at[0], at[1]);
  if (!json) return false;
  const menu = JSON.parse(json);
  contextMenu.replaceChildren();
  // The underlined command first: Rest and Combat only there, any other as its first entry.
  let first = null;
  if (menu.underlined !== null) {
    if (menu.underlined === 0x89 || menu.underlined === 0x8a) {
      first = { code: menu.underlined, enabled: true, reason: null, theft: false };
      contextMenu.append(menuItem(first, ''));
    } else {
      for (const section of menu.sections) {
        const entry = section.entries.find((x) => x.code === menu.underlined);
        if (entry) {
          first = entry;
          contextMenu.append(menuItem(entry, section.name));
          break;
        }
      }
    }
  }
  for (const section of menu.sections) {
    const entries = section.entries.filter((x) => x !== first);
    if (entries.length === 0) continue;
    const head = document.createElement('div');
    head.className = 'head';
    head.setAttribute('role', 'presentation');
    head.textContent = section.name;
    contextMenu.append(head);
    for (const entry of entries) contextMenu.append(menuItem(entry, ''));
  }
  menuAt = at;
  swallowing = true;
  showGamePointer();
  contextMenu.hidden = false;
  placeMenu(e.clientX, e.clientY);
  // No focus for the canvas from the press (its mouse default), the menu takes it, again once the
  // press's own handling is over.
  e.preventDefault();
  focusMenu();
  setTimeout(focusMenu, 0);
  return true;
}

// The open menu's first entry that can run (else its first) has the focus.
function focusMenu() {
  if (menuAt === null || contextMenu.contains(document.activeElement)) return;
  const items = menuItems();
  (items.find((x) => x.getAttribute('aria-disabled') !== 'true') ?? items[0])?.focus({ preventScroll: true });
}

// The menu at client point (x, y): to the right of and below the point, or flipped to the other
// side where it would not fit, kept inside the part of the screen box the viewport shows; at most
// that part's size, scrolling inside when its entries need more. Its style's minimum width gives
// way where that part is narrower (a minimum outweighs a maximum), and it is sized at the box's
// corner before its rect is read, so the width it gets is not the room left where it was last.
function placeMenu(x, y) {
  const box = screenBox.getBoundingClientRect();
  const view = {
    left: Math.max(box.left, 0),
    top: Math.max(box.top, 0),
    right: Math.min(box.right, window.innerWidth),
    bottom: Math.min(box.bottom, window.innerHeight),
  };
  const [vw, vh] = [Math.max(0, view.right - view.left), Math.max(0, view.bottom - view.top)];
  contextMenu.style.left = '0px';
  contextMenu.style.top = '0px';
  contextMenu.style.minWidth = `min(10em, ${vw}px)`;
  contextMenu.style.maxWidth = `${vw}px`;
  contextMenu.style.maxHeight = `${vh}px`;
  const size = contextMenu.getBoundingClientRect();
  const [w, h] = [size.width, size.height];
  let left = x;
  let top = y;
  if (left + w > view.right) left -= w;
  if (top + h > view.bottom) top -= h;
  left = clamp(left, view.left, Math.max(view.left, view.right - w));
  top = clamp(top, view.top, Math.max(view.top, view.bottom - h));
  contextMenu.style.left = `${left - box.left}px`;
  contextMenu.style.top = `${top - box.top}px`;
}

const menuItems = () => [...contextMenu.querySelectorAll('[role="menuitem"]')];

// Closes the menu; the canvas has the focus again unless `refocus` is false. A key that closes it
// (`code`, its DOM `code`) gives the canvas the focus only at its release, so the release never
// reaches the game; a blur of the window forgets it.
let refocusAt = null; // the physical key whose release gives the canvas the focus back
function closeContextMenu(refocus = true, code = null) {
  if (menuAt === null) return;
  menuAt = null;
  contextMenu.hidden = true;
  contextMenu.replaceChildren();
  if (refocus && code !== null) refocusAt = code;
  else if (refocus) canvas.focus({ preventScroll: true });
}
window.addEventListener('keyup', (e) => {
  if (refocusAt === null || e.code !== refocusAt) return;
  refocusAt = null;
  e.stopPropagation();
  canvas.focus({ preventScroll: true });
}, { capture: true });
window.addEventListener('blur', () => {
  refocusAt = null;
});

function chooseEntry(item, key = null) {
  if (menuAt === null || item.getAttribute('aria-disabled') === 'true') return;
  const code = Number(item.dataset.code);
  const [x, y] = menuAt;
  closeContextMenu(true, key);
  if (!game || crashed) return;
  if (SLOT_ACTIONS.has(code)) {
    game.pointer(x, y, 1);
    game.pointer(x, y, 0);
    sentButtons = 0;
  } else {
    game.command_at(code, x, y);
  }
}

contextMenu.addEventListener('keydown', (e) => {
  const items = menuItems();
  const at = items.indexOf(document.activeElement);
  const focus = (i) => items[(i + items.length) % items.length]?.focus({ preventScroll: true });
  const letter = e.key.length === 1 ? COMMAND_LETTERS[e.key.toLowerCase()] : undefined;
  if (e.key === 'ArrowDown') focus(at + 1);
  else if (e.key === 'ArrowUp') focus(at < 0 ? -1 : at - 1);
  else if (e.key === 'Home') focus(0);
  else if (e.key === 'End') focus(-1);
  else if (e.key === 'Escape') closeContextMenu(true, e.code);
  else if (e.key === 'Enter' || e.key === ' ') {
    if (at >= 0) chooseEntry(items[at], e.code);
  } else if (letter !== undefined) {
    const item = items.find((x) => Number(x.dataset.code) === letter && x.getAttribute('aria-disabled') !== 'true');
    if (item) chooseEntry(item, e.code);
  } else if (e.key !== 'Tab') return;
  if (e.key !== 'Tab') {
    e.preventDefault();
    e.stopPropagation();
  }
});
contextMenu.addEventListener('focusout', (e) => {
  if (menuAt !== null && !swallowing && !contextMenu.contains(e.relatedTarget)) closeContextMenu(false);
});
contextMenu.addEventListener('contextmenu', (e) => e.preventDefault());
// A press outside the open menu closes it and goes nowhere else: not to the game, up to its
// release, which the window sees wherever it happens (the press need not be the canvas's).
window.addEventListener('pointerdown', (e) => {
  if (menuAt === null || contextMenu.contains(e.target)) return;
  e.preventDefault();
  e.stopPropagation();
  swallowing = true;
  showGamePointer();
  closeContextMenu();
}, { capture: true });
for (const kind of ['pointerup', 'pointercancel']) {
  window.addEventListener(kind, (e) => {
    if (!swallowing || (kind === 'pointerup' && (e.buttons & 3) !== 0)) return;
    swallowing = false;
    showGamePointer();
    if (e.target === canvas) e.stopPropagation();
  }, { capture: true });
}

// The whole page accepts drops so a near miss does not open the file in the tab.
window.addEventListener('dragover', (e) => {
  e.preventDefault();
  drop.classList.add('over');
});
window.addEventListener('dragleave', (e) => {
  if (e.relatedTarget === null) drop.classList.remove('over');
});
window.addEventListener('drop', (e) => {
  e.preventDefault();
  drop.classList.remove('over');
  // Entries must be taken synchronously; the DataTransfer is cleared after this handler.
  const entries = [...e.dataTransfer.items]
    .map((item) => item.webkitGetAsEntry?.())
    .filter(Boolean);
  const files = [...e.dataTransfer.files];
  queueImport(async () => {
    if (entries.length === 0) return files.map((file) => ({ path: file.name, file }));
    const items = [];
    for (const entry of entries) await collect(entry, items);
    return items;
  });
});
picker.addEventListener('change', () => {
  const items = [...picker.files].map((file) => ({ path: file.webkitRelativePath || file.name, file }));
  picker.value = '';
  queueImport(async () => items);
});
forget.addEventListener('click', () => {
  if (queued > 0) say('forgetFilesLater');
  enqueue('opForgetFiles', forgetKept);
});
forgetSave.addEventListener('click', () => {
  if (queued > 0) say('forgetSaveLater');
  enqueue('opForgetSave', forgetSaved);
});
forgetHd.addEventListener('click', () => {
  if (queued > 0) say('forgetPackLater');
  enqueue('opForgetPack', forgetHdPack);
});
forgetKo.addEventListener('click', () => {
  if (queued > 0) say('forgetKoLater');
  enqueue('opForgetKo', forgetKoPack);
});
voicesButton.addEventListener('click', () => {
  gesture();
  toggleVoices();
});
voiceAccept.addEventListener('click', () => {
  gesture();
  acceptTerms();
});
voiceDecline.addEventListener('click', declineTerms);
forgetVoices.addEventListener('click', () => {
  forgetVoiceModel().catch(() => say('voiceForgetFailed'));
});
fullscreenButton.addEventListener('click', () => {
  if (fullNow()) leaveFullscreen();
  else enterFullscreen();
});
fullscreenExit.addEventListener('click', leaveFullscreen);
document.addEventListener('fullscreenchange', () => {
  if (document.fullscreenElement !== screenBox) navigator.keyboard?.unlock?.();
  showFullscreen();
});
// The Exit button while the pointer moves over the fullscreen box (a touch too).
for (const kind of ['pointermove', 'pointerdown']) screenBox.addEventListener(kind, showExit);
restart.addEventListener('click', () => {
  // With no storage for the mark, the reload just does not move the focus.
  setMark(FOCUS_AFTER_RELOAD);
  location.reload();
});
window.addEventListener('unhandledrejection', (e) => {
  console.error(e.reason);
  say('unexpected', message(e.reason));
});

// Whether the page's history state is an object (or none) the marks can be kept in, its other
// keys with them.
function stateHoldsMarks(state) {
  return state === null || (typeof state === 'object' && !Array.isArray(state));
}

// A copy of the marks this page's history entry keeps ({} with none), or null when its state is
// of another kind or its marks do not read: no mark is set or taken there then.
function stateMarks() {
  const state = history.state;
  if (!stateHoldsMarks(state)) return null;
  if (state === null || !Object.hasOwn(state, MARKS_STATE)) return {};
  const marks = state[MARKS_STATE];
  return marks !== null && typeof marks === 'object' && !Array.isArray(marks) ? { ...marks } : null;
}

// Writes `marks` into this page's history entry, its state's other keys kept (the marks' key gone
// with none), and reads it back: whether the entry holds `key` as `marks` does (that time, or
// none).
function keepStateMarks(marks, key) {
  const next = { ...history.state };
  if (Object.keys(marks).length > 0) next[MARKS_STATE] = marks;
  else delete next[MARKS_STATE];
  history.replaceState(next, '');
  const kept = history.state?.[MARKS_STATE];
  const holds = kept !== null && typeof kept === 'object' && Object.hasOwn(kept, key);
  return Object.hasOwn(marks, key) ? holds && kept[key] === marks[key] : !holds;
}

// Sets the one-shot mark `key` for the page loaded next: in sessionStorage, else (it throws: some
// privacy modes) in the page's history entry with its time; with neither, the next page goes
// without it.
function setMark(key) {
  try {
    sessionStorage.setItem(key, '1');
    return;
  } catch {
    // No session storage: the history entry.
  }
  try {
    const marks = stateMarks();
    if (marks !== null) keepStateMarks({ ...marks, [key]: Date.now() }, key);
  } catch {
    // A history entry that cannot be written: no mark.
  }
}

// Whether the mark `key` was set for this page, taken once (never a throw): sessionStorage's when
// it can be removed (else it would count again), then the history entry's while its time is a
// number 0 to MARK_FALLBACK_MS old and its removal reads back. The entry's is removed whatever it
// holds, the other marks kept; marks that do not read are removed.
function takeMark(key) {
  let set = false;
  try {
    if (sessionStorage.getItem(key) === '1') {
      sessionStorage.removeItem(key);
      set = true;
    }
  } catch {
    // No session storage, or a mark it does not let go: none from there.
  }
  try {
    const marks = stateMarks();
    if (marks === null) {
      if (stateHoldsMarks(history.state)) keepStateMarks({}, key);
    } else if (Object.hasOwn(marks, key)) {
      const at = marks[key];
      delete marks[key];
      const age = typeof at === 'number' && Number.isFinite(at) ? Date.now() - at : NaN;
      if (keepStateMarks(marks, key) && age >= 0 && age < MARK_FALLBACK_MS) set = true;
    }
  } catch {
    // A history entry that cannot be read or written: no mark from there.
  }
  return set;
}

// The mark the switch to the other build left for a voice download it cut, taken once.
function takeVoiceDownload() {
  return takeMark(VOICE_DOWNLOAD_AFTER_RELOAD);
}

function takeFocusFlag() {
  return takeMark(FOCUS_AFTER_RELOAD);
}

// A choice kept before this version counts as none, once: the chooser opens as on a first run
// (on 한글, whatever the browser's language), and its OK keeps the current record.
const setupStale = storedSetup() !== null && storedSetup().version !== SETUP_VERSION;
setupFirst = !storedSetup() || setupStale;
if (setupFirst) lang = 'ko';
reflect('loading');
reflectPack();
reflectAudio();
setLang(lang);
say('loadingEngine');
showHd();
showLayout();
// The HD font asked for at once when it is wanted, so it is in before the first game's frame.
if (fontWanted()) loadFont();
reflectFont();
showFullscreen();
canvas.dataset.behind = '0';
reflectVoice();
// Voices turned on in an earlier visit come back from this browser's cache only; a download the
// switch to the other build cut (`switchBuild`) goes on, downloads allowed.
const voiceDownload = takeVoiceDownload();
if (voicesWanted() && termsAccepted()) startVoices({ cachedOnly: !voiceDownload });
// Restart's mark, taken as the page loads (its age is the reload's; an engine that fails to load
// leaves none behind), is applied once the kept game is back (`opRestore`).
const focusAfterReload = takeFocusFlag();
// The chooser first while no choice of this version is kept.
if (setupFirst) openSetup();
enqueue('opEngine', async () => {
  try {
    ({
      default: init, Game, HdPack, LocalePack, PackProbe, budget_ms, input_too_large, max_input_bytes,
      max_locale_pack_bytes, max_pack_bytes, max_save_bytes, max_save_file_bytes, step_ms, zip_sliced,
      shown_map,
    } = await import(ENGINE_URL));
    wasm = await init();
    if (WIDE) placeWide(shown_map());
  } catch (err) {
    reflect('failed');
    throw new Error(t('engineFailed', message(err)));
  }
  stepMs = step_ms();
  forget.disabled = false;
  forgetSave.disabled = false;
  forgetHd.disabled = false;
  forgetKo.disabled = false;
  reflect('idle');
});
// The picker opens once the kept files are back, so a choice never races the restore (a drop
// meanwhile is queued after it).
enqueue('opRestore', async () => {
  try {
    await dropOldBundles();
    await restoreVoicePack();
    // The site's voice pack (spec 09 Design 4), in either language, before the first game speaks.
    await hostedVoicePack();
    if (wasm) await restorePack();
    if (wasm) await restoreLocalePack();
    // In 한글 the site's Korean pack first (spec 08 K6), so the first game starts with it.
    const hosted = wasm ? await hostedLocalePack() : null;
    if (wasm) await restoreKept(focusAfterReload, hosted?.line ?? '');
  } finally {
    if (wasm) picker.disabled = false;
    // Storage that could not say what it keeps: the drop line shows.
    if (filesKept === null) filesKept = false;
    reflect(game ? undefined : wasm ? 'idle' : 'failed');
  }
});

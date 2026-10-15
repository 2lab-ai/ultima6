//! Browser entry point: collects the dropped files, runs u6-intake, boots `u6_core::Engine`
//! and hands frames back as RGBA8 (see `site/app.js`).
//!
//! JS owns the clock and the DOM. Key callbacks only queue events; [`Game::tick`] delivers them
//! to the engine with the elapsed time, so game code never runs inside an input handler.
//!
//! Sound: once the page has an AudioContext it calls [`Game::audio_start`] with its sample
//! rate; every tick then replays the game's MED calls on the emulated A500 audio machine
//! (`u6_core::sound::AudioDriver`) and [`Game::audio_render`] renders the samples the page
//! posts to its AudioWorklet. Saves: the page lays the stored `savegame/*` files over the
//! new-game files before the boot ([`Game::load_save`], `saveload::attach`), and after a tick
//! whose Save committed ([`Game::save_generation`] moved) it stores what [`Game::save_collect`]
//! lists, then reports the generation committed ([`Game::save_committed`]). The character `u`
//! makes is such a write too (its `savegame/objlist`). A list holds what the Saves since the
//! last commit replaced; after the page removed the stored save ([`Game::save_forgotten`]) the
//! next one holds the whole save.
//!
//! A start runs the launcher's sequence (`u`: the opening and the menu, then `game`);
//! [`Game::start_game_first`] starts `game` at once (development and tests). When the sequence
//! ends (Quit in the menu, the end of the game) [`Game::finished`] turns true and the page says
//! so.
//!
//! Limits (a clear message instead of an exhausted tab): [`MAX_INPUT_BYTES`] for the dropped
//! files together as they are copied into wasm (a zip over [`SLICED_ZIP_BYTES`] is read in
//! slices, [`Game::zip_open`]: only its directory and its disk-image entries are copied in, and
//! only they count), [`MAX_UNZIPPED_BYTES`] for what the zip entries the intake inflates declare,
//! [`MAX_GAME_FILES_BYTES`] for the game files the intake produces, [`MAX_SAVE_FILE_BYTES`] and
//! [`MAX_SAVE_BYTES`] for the stored save (a file over them is left out, and the start's report
//! says so).

mod zipscan;

use std::collections::BTreeMap;

use u6_core::objects::{BLOCK_COUNT, MAX_RECORDS};
use u6_core::saveload::{MemStore, OBJLIST_FILE, attach, block_file, changed_since};
use u6_core::sound::AudioDriver;
use u6_core::{Engine, Files, Framebuffer, InputEvent, Key};
use u6_intake::{InputFile, intake};
use wasm_bindgen::prelude::*;

/// The dropped files together, as copied into wasm: a file read whole counts its size; a zip
/// read in slices ([`zip_sliced`]) counts the bytes read of it, its directory and its disk-image
/// entries, not its size. Every read is checked before it is made: the page checks the sizes of
/// the files it reads whole, before anything is read and again before each one against what
/// wasm already holds ([`Game::input_held`]); [`Game::load_file`] is their backstop, and
/// [`Game::zip_need`] checks each slice before the page reads it.
pub const MAX_INPUT_BYTES: usize = 512 << 20;
/// A dropped zip larger than this is read in slices ([`Game::zip_open`]) instead of whole: the
/// archive.org zip is mostly flux streams and scans the intake never reads. A smaller one is
/// copied in whole, which costs the tab at most this much.
pub const SLICED_ZIP_BYTES: u64 = 16 << 20;
/// What the disk-image entries of the dropped zips (the only entries the intake inflates)
/// declare uncompressed, together; read from the central directories before anything is
/// inflated (u6-intake caps each inflate at its declared size).
pub const MAX_UNZIPPED_BYTES: u64 = 64 << 20;
/// The game files the intake produced, together (the Amiga disks hold 2.4 MiB), checked before
/// the engine boots. The intake itself bounds its total output (decrunched files, disk images
/// and raw files; u6-intake's budget), so this is the page's own limit on what it boots from.
pub const MAX_GAME_FILES_BYTES: usize = 64 << 20;
/// One stored `savegame/` file: the largest the game writes there, a block at its record limit
/// (the count word and 0xc00 records of 8 bytes; 0x10ddd2 clamps the count it reads to 0xc00).
/// `savegame/objlist` is under it: Save writes the 0x1C73 bytes load state reads, and the
/// character `u` makes keeps the user's file at its own length (u 0x105700 writes the first
/// 0x1C73 bytes over it), so the objlist is not capped at 0x1C73.
pub const MAX_SAVE_FILE_BYTES: usize = 2 + MAX_RECORDS * 8;
/// The stored save together: the objlist and the 69 blocks, each at most [`MAX_SAVE_FILE_BYTES`].
pub const MAX_SAVE_BYTES: usize = (1 + BLOCK_COUNT) * MAX_SAVE_FILE_BYTES;

/// [`MAX_INPUT_BYTES`] for the page.
#[wasm_bindgen]
pub fn max_input_bytes() -> f64 {
    MAX_INPUT_BYTES as f64
}

/// [`MAX_SAVE_FILE_BYTES`] for the page (it leaves larger files of a dropped `savegame/` folder
/// out before reading them; [`Game::load_save`] is the backstop).
#[wasm_bindgen]
pub fn max_save_file_bytes() -> f64 {
    MAX_SAVE_FILE_BYTES as f64
}

/// [`MAX_SAVE_BYTES`] for the page.
#[wasm_bindgen]
pub fn max_save_bytes() -> f64 {
    MAX_SAVE_BYTES as f64
}

/// Whether the page reads the dropped file `name` of `size` bytes through [`Game::zip_open`]
/// instead of whole: a `.zip` over [`SLICED_ZIP_BYTES`]. Whether it is a zip is then told from
/// its first bytes, as for a file read whole (one that is not is read whole after all).
#[wasm_bindgen]
pub fn zip_sliced(name: &str, size: f64) -> bool {
    zipscan::ends_with_ci(name, ".zip") && size > SLICED_ZIP_BYTES as f64
}

/// [`too_large`] for the page (its check of the files it reads whole).
#[wasm_bindgen]
pub fn input_too_large(total: f64) -> String {
    too_large(total as u64)
}

/// The message for inputs over [`MAX_INPUT_BYTES`]; it states the rule.
pub fn too_large(total: u64) -> String {
    format!(
        "The dropped files are {} MiB together; this page takes at most {} MiB (a zip over {} \
         MiB counts only what is read of it: its directory and its disk images).",
        total >> 20,
        MAX_INPUT_BYTES >> 20,
        SLICED_ZIP_BYTES >> 20
    )
}

/// The message for disk images that declare more than [`MAX_UNZIPPED_BYTES`] unpacked.
pub fn unpacks_too_much(declared: u64) -> String {
    format!(
        "The disk images in the dropped zip declare {} MiB unpacked; this page unpacks at most \
         {} MiB.",
        declared >> 20,
        MAX_UNZIPPED_BYTES >> 20
    )
}

/// The letters whose control characters the command loop dispatches (Ctrl-E / Ctrl-Q exit,
/// Ctrl-H help, Ctrl-R restore, Ctrl-S save, Ctrl-V version, Ctrl-Z sound;
/// `u6_core::main_loop::commands`). With Ctrl held only these are game keys; every other
/// Ctrl shortcut keeps its browser meaning.
const CONTROL_LETTERS: &str = "ehqrsvz";

/// Maps a DOM `KeyboardEvent` (`code`, `key`, `ctrlKey`) to an engine key: named keys by
/// `code`, printable single characters by `key`, and with Ctrl a letter of
/// [`CONTROL_LETTERS`] by `code` as its control character (Ctrl-S = 0x13).
pub fn key_from_dom(code: &str, key: &str, ctrl: bool) -> Option<Key> {
    if ctrl {
        let letter = code.strip_prefix("Key")?.to_ascii_lowercase();
        let mut chars = letter.chars();
        return match (chars.next(), chars.next()) {
            (Some(c), None) if CONTROL_LETTERS.contains(c) => {
                Some(Key::Char(char::from(c as u8 & 0x1f)))
            }
            _ => None,
        };
    }
    Some(match code {
        "ArrowUp" => Key::Up,
        "ArrowDown" => Key::Down,
        "ArrowLeft" => Key::Left,
        "ArrowRight" => Key::Right,
        "Enter" | "NumpadEnter" => Key::Enter,
        "Escape" => Key::Escape,
        "Space" => Key::Space,
        "Backspace" => Key::Backspace,
        _ => {
            let mut chars = key.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_graphic() => Key::Char(c),
                _ => return None,
            }
        }
    })
}

#[wasm_bindgen]
pub struct Game {
    inputs: Vec<InputFile>,
    /// Bytes copied in for `inputs` and the zip being read ([`MAX_INPUT_BYTES`]).
    input_bytes: usize,
    /// The zip being read in slices ([`Game::zip_open`]) and `input_bytes` before it.
    zip: Option<(zipscan::Sliced, usize)>,
    engine: Option<Engine>,
    pending: Vec<InputEvent>,
    blank: Framebuffer,
    rgba: Vec<u8>,
    log: Vec<String>,
    /// Paths of the game files the last start booted from, until the page has kept them (their
    /// bytes are read in place from the engine's file map, never copied in wasm).
    kept: Vec<String>,
    /// The stored `savegame/*` files the next start lays over the game files.
    saves: BTreeMap<String, Vec<u8>>,
    /// Bytes in `saves`.
    save_bytes: usize,
    /// One line per stored file [`Game::load_save`] left out, for the next start's report.
    save_notes: Vec<String>,
    /// The last save generation the page reported stored.
    saved_gen: u32,
    /// The files [`Game::save_collect`] listed, until [`Game::save_committed`].
    save_out: Vec<(String, Vec<u8>)>,
    /// The next list is the whole save ([`Game::save_forgotten`]); `save_out_whole`: the last
    /// list was.
    save_whole: bool,
    save_out_whole: bool,
    /// The audio machine, once the page started its AudioContext.
    audio: Option<AudioDriver>,
    audio_rate: u32,
    audio_buf: Vec<f32>,
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Game {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Game {
        let mut game = Game {
            inputs: Vec::new(),
            input_bytes: 0,
            zip: None,
            engine: None,
            pending: Vec::new(),
            blank: Framebuffer::new(),
            rgba: Vec::new(),
            log: Vec::new(),
            kept: Vec::new(),
            saves: BTreeMap::new(),
            save_bytes: 0,
            save_notes: Vec::new(),
            saved_gen: 0,
            save_out: Vec::new(),
            save_whole: false,
            save_out_whole: false,
            audio: None,
            audio_rate: 0,
            audio_buf: Vec::new(),
        };
        game.redraw();
        game
    }

    pub fn width(&self) -> u32 {
        self.blank.width() as u32
    }

    pub fn height(&self) -> u32 {
        self.blank.height() as u32
    }

    /// Collects one dropped file (zip, ADF, HFE or an extracted game file) for [`Game::start`].
    /// Takes the bytes over (one copy from JS into wasm, none here). Fails, keeping nothing of
    /// the call, when the files collected so far would pass [`MAX_INPUT_BYTES`].
    pub fn load_file(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), JsError> {
        self.collect(path, bytes).map_err(|msg| JsError::new(&msg))
    }

    pub fn files_loaded(&self) -> u32 {
        self.inputs.len() as u32
    }

    /// The bytes the dropped files hold in wasm so far (the files read whole and what was read of
    /// the zips read in slices): the page checks each file it reads whole against
    /// [`MAX_INPUT_BYTES`] with these before reading it.
    pub fn input_held(&self) -> f64 {
        self.input_bytes as f64
    }

    /// Starts reading the dropped file `path` of `size` bytes in slices (the page does so when
    /// [`zip_sliced`]): [`Game::zip_need`] names each range of it to read and [`Game::zip_give`]
    /// takes its bytes. First its first four bytes: one that does not start as a zip is then read
    /// whole and collected as it is. A zip is collected for [`Game::start`] as a zip of only its
    /// disk-image entries, which the intake reads as it would read them in the dropped zip. Only
    /// the bytes read count toward [`MAX_INPUT_BYTES`]. A file still being read is dropped.
    pub fn zip_open(&mut self, path: &str, size: f64) -> Result<(), JsError> {
        self.open_zip(path, size).map_err(|msg| JsError::new(&msg))
    }

    /// The next range of the open zip to read, as `[start, end]` (`end` exclusive), or empty once
    /// it is read and collected. Fails, keeping nothing of the zip, when reading the range would
    /// take the dropped files past [`MAX_INPUT_BYTES`].
    pub fn zip_need(&mut self) -> Result<Vec<f64>, JsError> {
        let range = self.zip_range().map_err(|msg| JsError::new(&msg))?;
        Ok(range.map_or_else(Vec::new, |(start, end)| vec![start as f64, end as f64]))
    }

    /// The bytes of the range [`Game::zip_need`] named. Fails, keeping nothing of the zip, when
    /// they do not read as that part of a zip (the directory; an entry's local header, which must
    /// agree with its central record, and its data; see `u6_intake::zip`), when its disk images
    /// are more than `u6_intake::zip::MAX_ENTRIES`, yield more than [`MAX_UNZIPPED_BYTES`], or
    /// their ranges would take the dropped files past [`MAX_INPUT_BYTES`] (all checked before
    /// any entry is read).
    pub fn zip_give(&mut self, bytes: Vec<u8>) -> Result<(), JsError> {
        self.zip_bytes(bytes).map_err(|msg| JsError::new(&msg))
    }

    /// Runs the intake over the collected files and boots the engine with RNG seed `seed` (the
    /// original seeds from the clock; the page passes the low word of the time). Returns the
    /// intake report, one line per entry; fails with the report plus the boot error.
    pub fn start(&mut self, seed: u16) -> Result<String, JsError> {
        self.boot(seed, false)
            .map_err(|report| JsError::new(&report))
    }

    /// As [`Game::start`], but `game` starts at once, without the launcher and `u` (the
    /// development entry, as `u6-native --game-first`).
    pub fn start_game_first(&mut self, seed: u16) -> Result<String, JsError> {
        self.boot(seed, true)
            .map_err(|report| JsError::new(&report))
    }

    /// True once the launcher's sequence ended (the engine is not ticked further).
    pub fn finished(&self) -> bool {
        self.engine.as_ref().is_some_and(Engine::finished)
    }

    /// The program that runs: `u`, `game`, `ended`, or empty before a start.
    pub fn program(&self) -> String {
        let Some(e) = &self.engine else {
            return String::new();
        };
        if e.finished() {
            return "ended".into();
        }
        match e.state().main.program {
            u6_core::main_loop::Program::U => "u".into(),
            u6_core::main_loop::Program::Game => "game".into(),
        }
    }

    /// Keys queued and not yet taken by the game (the page's and the engine's queues).
    pub fn keys_pending(&self) -> u32 {
        let queued = self.engine.as_ref().map_or(0, |e| e.io().pending_events());
        (queued + self.pending.len()) as u32
    }

    /// The party leader's name, or empty before a start.
    pub fn leader_name(&self) -> String {
        self.engine.as_ref().map_or_else(String::new, |e| {
            let s = e.state();
            let p = &s.actors.party;
            p.names
                .get(usize::from(p.leader_index))
                .cloned()
                .unwrap_or_default()
        })
    }

    /// Number of game files the running game booted from (the save laid over them not
    /// counted; see [`Game::save_files`]).
    pub fn game_files(&self) -> u32 {
        self.engine
            .as_ref()
            .map_or(0, |e| e.io().files().layer_len("base") as u32)
    }

    /// Collects one stored save file (`savegame/objlist`, `savegame/objblkAA`, ...) for the next
    /// [`Game::start`], which lays them over the game files. A file over
    /// [`MAX_SAVE_FILE_BYTES`], or one that would take the files together past
    /// [`MAX_SAVE_BYTES`], is left out (the start's report names it); a path given again
    /// replaces its file. Returns whether the file was taken.
    pub fn load_save(&mut self, path: &str, bytes: Vec<u8>) -> bool {
        let old = self.saves.get(path).map_or(0, Vec::len);
        let total = self.save_bytes - old + bytes.len();
        if bytes.len() > MAX_SAVE_FILE_BYTES || total > MAX_SAVE_BYTES {
            let limit = if bytes.len() > MAX_SAVE_FILE_BYTES {
                format!("a save file holds at most {MAX_SAVE_FILE_BYTES}")
            } else {
                format!("the saved game holds at most {MAX_SAVE_BYTES} together")
            };
            self.save_notes.push(format!(
                "The saved game's file {path} ({} bytes) is left out: {limit} bytes.",
                bytes.len()
            ));
            return false;
        }
        self.save_bytes = total;
        self.saves.insert(path.to_string(), bytes);
        true
    }

    /// Number of stored save files the running game booted with (0: a new game).
    pub fn save_files(&self) -> u32 {
        self.engine.as_ref().map_or(0, |e| {
            e.io().files().layer_len(u6_core::saveload::SAVE_LAYER) as u32
        })
    }

    /// Writes of `savegame/` files since boot (Saves, 0x105716, and the character `u` wrote).
    pub fn save_generation(&self) -> u32 {
        self.engine
            .as_ref()
            .map_or(0, |e| e.state().saveload.generation)
    }

    /// Lists the files the Saves since the last reported commit wrote (`savegame/objlist` and
    /// the replaced blocks, the original's names); returns how many. Read them with
    /// [`Game::save_path`] / [`Game::save_ptr`] / [`Game::save_len`].
    pub fn save_collect(&mut self) -> u32 {
        let whole = self.save_whole;
        self.save_out = self.engine.as_ref().map_or_else(Vec::new, |e| {
            let s = e.state();
            let files = if whole {
                whole_save(&s, self.saved_gen)
            } else {
                changed_since(&s, self.saved_gen)
            };
            files.into_iter().collect()
        });
        self.save_out_whole = whole;
        self.save_out.len() as u32
    }

    pub fn save_path(&self, i: u32) -> String {
        self.save_out
            .get(i as usize)
            .map(|(p, _)| p.clone())
            .unwrap_or_default()
    }

    /// Address of listed save file `i` in wasm memory; copy `save_len(i)` bytes before the next
    /// call into wasm.
    pub fn save_ptr(&self, i: u32) -> *const u8 {
        self.save_out
            .get(i as usize)
            .map_or(std::ptr::null(), |(_, b)| b.as_ptr())
    }

    pub fn save_len(&self, i: u32) -> usize {
        self.save_out.get(i as usize).map_or(0, |(_, b)| b.len())
    }

    /// The page removed the stored save: the next list ([`Game::save_collect`]) holds the whole
    /// save, `savegame/objlist` and all 69 blocks as the game has them, until one is committed.
    /// A Save lists only the blocks it replaced; over an emptied store those alone would let the
    /// next start take every other block from the new-game files, a world this game never had.
    pub fn save_forgotten(&mut self) {
        self.save_whole = true;
    }

    /// The page stored the files of generation `generation` (all-or-nothing); later lists start
    /// after it. A failed store is not reported, so the next list still has its files.
    pub fn save_committed(&mut self, generation: u32) {
        self.saved_gen = self.saved_gen.max(generation);
        if self.save_out_whole {
            self.save_whole = false;
        }
        self.save_out = Vec::new();
    }

    /// True while the command loop waits for a command key with nothing queued (A4-0x7a2c set,
    /// no key pending): the prompt is up.
    pub fn at_prompt(&self) -> bool {
        self.engine.as_ref().is_some_and(|e| {
            e.state().main.reading == 1 && e.io().pending_events() == 0 && self.pending.is_empty()
        })
    }

    /// The leader's map position (x, y, z), or empty before a start.
    pub fn leader_position(&self) -> Vec<u16> {
        self.engine.as_ref().map_or_else(Vec::new, |e| {
            let s = e.state();
            s.actors.party.leader().map_or_else(Vec::new, |l| {
                let p = s.objects.location[usize::from(l.0)];
                vec![p.x, p.y, u16::from(p.z)]
            })
        })
    }

    /// Starts the audio machine for the running game at the page's `sample_rate`; false before
    /// a start (the page calls it again for each new game).
    pub fn audio_start(&mut self, sample_rate: u32) -> Result<bool, JsError> {
        let Some(engine) = &self.engine else {
            return Ok(false);
        };
        let mut driver =
            AudioDriver::new(engine.io().files()).map_err(|e| JsError::new(&e.to_string()))?;
        driver.sync(&engine.state().sound, engine.io().files());
        self.audio = Some(driver);
        self.audio_rate = sample_rate;
        Ok(true)
    }

    /// Renders `frames` stereo frames (interleaved f32) into the audio buffer; returns the
    /// number of samples (0 without [`Game::audio_start`]). Copy them from [`Game::audio_ptr`]
    /// before the next call into wasm.
    pub fn audio_render(&mut self, frames: u32) -> usize {
        let Some(driver) = &mut self.audio else {
            return 0;
        };
        self.audio_buf.clear();
        self.audio_buf.resize(2 * frames as usize, 0.0);
        driver.render(&mut self.audio_buf, self.audio_rate);
        self.audio_buf.len()
    }

    pub fn audio_ptr(&self) -> *const f32 {
        self.audio_buf.as_ptr()
    }

    /// Number of game files the last start left for the page to keep (so a reload needs no new
    /// drop; each goes back in as a loose file the intake passes through). 0 after
    /// [`Game::release_kept`].
    pub fn kept_count(&self) -> u32 {
        self.kept.len() as u32
    }

    /// Path of kept file `i` (`VOLUME/dir/file`).
    pub fn kept_path(&self, i: u32) -> String {
        self.kept.get(i as usize).cloned().unwrap_or_default()
    }

    /// Address of kept file `i` in wasm memory (0 when absent); the page copies `kept_len(i)`
    /// bytes from there before calling into wasm again.
    pub fn kept_ptr(&self, i: u32) -> *const u8 {
        self.kept_bytes(i).map_or(std::ptr::null(), <[u8]>::as_ptr)
    }

    pub fn kept_len(&self, i: u32) -> usize {
        self.kept_bytes(i).map_or(0, <[u8]>::len)
    }

    /// Forgets the kept paths once the page stored (or gave up on) the files.
    pub fn release_kept(&mut self) {
        self.kept = Vec::new();
    }

    /// Stub points the game reached so far, one `count id` line each (for the console).
    pub fn stub_report(&self) -> String {
        self.engine.as_ref().map_or_else(String::new, |e| {
            e.stub_hits()
                .iter()
                .map(|(id, n)| format!("{n} {id}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    pub fn started(&self) -> bool {
        self.engine.is_some()
    }

    /// Advances the engine by `ms` with the keys queued since the previous tick.
    pub fn tick(&mut self, ms: u32) {
        if let Some(engine) = &mut self.engine {
            engine.tick(&self.pending, ms);
            self.log.extend(engine.take_log());
            if let Some(driver) = &mut self.audio {
                driver.sync(&engine.state().sound, engine.io().files());
            }
        }
        self.pending.clear();
        self.redraw();
    }

    /// Queues a key press. Returns true when it is a game key (the caller should then
    /// `preventDefault` so arrows do not scroll the page and Ctrl-S / Ctrl-R do not save or
    /// reload the page).
    pub fn key_down(&mut self, code: &str, key: &str, ctrl: bool) -> bool {
        match key_from_dom(code, key, ctrl) {
            Some(k) => {
                if self.engine.is_some() {
                    self.pending.push(InputEvent::Key(k));
                }
                true
            }
            None => false,
        }
    }

    /// Queues the pointer at screen pixel (x, y) (the page maps the canvas's CSS box onto its
    /// 320 × 200 pixels) with `buttons` held: bit 0 left, bit 1 right, as DOM `buttons`. Ignored
    /// before the game starts.
    pub fn pointer(&mut self, x: i32, y: i32, buttons: u8) {
        if self.engine.is_some() {
            queue_pointer(&mut self.pending, x, y, buttons);
        }
    }

    /// Engine log lines (first hit of each stub point) since the previous call.
    pub fn take_log(&mut self) -> String {
        std::mem::take(&mut self.log).join("\n")
    }

    /// Current frame as RGBA8, `width * height * 4` bytes (a copy).
    pub fn frame_rgba(&self) -> Vec<u8> {
        self.rgba.clone()
    }

    /// Address of the current RGBA8 frame inside wasm memory. Views over it are
    /// invalidated when memory grows; re-create them every frame.
    pub fn frame_ptr(&self) -> *const u8 {
        self.rgba.as_ptr()
    }

    pub fn frame_len(&self) -> usize {
        self.rgba.len()
    }
}

impl Game {
    /// [`Game::load_file`] without the JS error type.
    fn collect(&mut self, path: &str, data: Vec<u8>) -> Result<(), String> {
        let total = self.input_bytes.saturating_add(data.len());
        if total > MAX_INPUT_BYTES {
            return Err(too_large(total as u64));
        }
        self.input_bytes = total;
        self.inputs.push(InputFile {
            name: path.to_string(),
            data,
        });
        Ok(())
    }

    /// [`Game::zip_open`] without the JS error type.
    fn open_zip(&mut self, path: &str, size: f64) -> Result<(), String> {
        if !(size.is_finite() && size >= 0.0 && size.fract() == 0.0 && size < 2f64.powi(53)) {
            return Err(format!("{path}: not a file size"));
        }
        self.drop_zip();
        let sliced = zipscan::Sliced::new(path, size as u64);
        self.zip = Some((sliced, self.input_bytes));
        Ok(())
    }

    /// [`Game::zip_need`] without the JS error type: `None` once the zip is collected.
    fn zip_range(&mut self) -> Result<Option<(u64, u64)>, String> {
        let Some((sliced, _)) = &self.zip else {
            return Ok(None);
        };
        if let Some((start, end)) = sliced.need() {
            let total = (self.input_bytes as u64).saturating_add(end - start);
            if total > MAX_INPUT_BYTES as u64 {
                self.drop_zip();
                return Err(too_large(total));
            }
            return Ok(Some((start, end)));
        }
        let Some((sliced, held)) = self.zip.take() else {
            return Ok(None);
        };
        let name = sliced.path.clone();
        match sliced.finish() {
            Ok(data) => {
                self.inputs.push(InputFile { name, data });
                Ok(None)
            }
            Err(msg) => {
                self.input_bytes = held;
                Err(msg)
            }
        }
    }

    /// [`Game::zip_give`] without the JS error type.
    fn zip_bytes(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let Some((sliced, _)) = &mut self.zip else {
            return Err("no zip is being read".into());
        };
        let held = self.input_bytes.saturating_add(bytes.len());
        match sliced.give(bytes, held) {
            Ok(()) => {
                self.input_bytes = held;
                Ok(())
            }
            Err(msg) => {
                self.drop_zip();
                Err(msg)
            }
        }
    }

    /// Forgets the zip being read and what it counted.
    fn drop_zip(&mut self) {
        if let Some((_, held)) = self.zip.take() {
            self.input_bytes = held;
        }
    }

    /// Kept file `i`, read in place from the running engine's game files (the base layer: a
    /// save laid over them is never kept as a game file).
    fn kept_bytes(&self, i: u32) -> Option<&[u8]> {
        let path = self.kept.get(i as usize)?;
        let (_, bytes) = self.engine.as_ref()?.io().files().get_in("base", path)?;
        Some(bytes)
    }

    /// [`Game::start`] (or with `game_first` [`Game::start_game_first`]) without the JS error
    /// type (testable off wasm).
    fn boot(&mut self, seed: u16, game_first: bool) -> Result<String, String> {
        let inputs = std::mem::take(&mut self.inputs);
        self.input_bytes = 0;
        self.zip = None;
        // A zip whose directory does not read is refused here, not left to the intake: what its
        // disk images yield could not be checked.
        let mut unzipped = 0u64;
        for f in &inputs {
            let declared = zipscan::declared_disk_bytes(&f.name, &f.data)?;
            unzipped = unzipped.saturating_add(declared.unwrap_or(0));
        }
        if unzipped > MAX_UNZIPPED_BYTES {
            return Err(unpacks_too_much(unzipped));
        }
        let r = intake(inputs);
        drop(r.disks);
        let mut report = r.report.join("\n");
        for note in std::mem::take(&mut self.save_notes) {
            report.push_str(&format!("\n{note}"));
        }
        let total: usize = r.files.values().map(Vec::len).sum();
        if total > MAX_GAME_FILES_BYTES {
            report.push_str(&format!(
                "\nThe game files come to {} MiB; this page takes at most {} MiB.",
                total >> 20,
                MAX_GAME_FILES_BYTES >> 20
            ));
            return Err(report);
        }
        let mut paths: Vec<String> = r.files.keys().cloned().collect();
        let mut files = Files::new(r.files);
        let saves = MemStore {
            files: std::mem::take(&mut self.saves),
        };
        self.save_bytes = 0;
        let stored = attach(&mut files, &saves).map_err(|e| format!("{report}\n{e}"))?;
        if stored > 0 {
            report.push_str(&format!(
                "\nThe saved game ({stored} files) is laid over them."
            ));
        }
        let engine = if game_first {
            Engine::game_first(files, seed)
        } else {
            Engine::with_seed(files, seed)
        };
        match engine {
            Ok(engine) => {
                // A path that differs from another only in case is shadowed (the game's lookups
                // ignore case): the engine never reads it, so it is not kept either.
                let files = engine.io().files();
                paths.retain(|p| files.get_in("base", p).is_some_and(|(key, _)| key == p));
                self.kept = paths;
                self.saved_gen = 0;
                self.save_out = Vec::new();
                self.save_whole = false;
                self.save_out_whole = false;
                self.audio = None;
                self.engine = Some(engine);
                self.pending.clear();
                self.redraw();
                Ok(report)
            }
            Err(e) => {
                report.push_str(&format!("\n{e}"));
                Err(report)
            }
        }
    }

    fn redraw(&mut self) {
        let fb = self.engine.as_ref().map_or(&self.blank, |e| e.frame());
        fb.write_rgba(&mut self.rgba);
    }
}

/// The whole save after a Save since generation `since`: `savegame/objlist` as the last Save (or
/// `u`'s character) wrote it and every block file as the game holds it (the files a Save
/// commits; between Saves they do not change), under the original's names. Empty when nothing
/// was saved since `since`.
fn whole_save(s: &u6_core::State, since: u32) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    if s.saveload.generation <= since {
        return out;
    }
    if let Some(objlist) = &s.saveload.objlist {
        out.insert(OBJLIST_FILE.to_string(), objlist.clone());
    }
    for (i, (_, block)) in s.objects.blocks.iter().enumerate() {
        out.insert(block_file(i), block.to_bytes());
    }
    out
}

/// Queues a pointer event; a move right after one with the same buttons replaces it (the game
/// samples the pointer once per frame and takes a run of moves at once anyway).
fn queue_pointer(pending: &mut Vec<InputEvent>, x: i32, y: i32, buttons: u8) {
    let word = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
    let ev = InputEvent::Pointer {
        x: word(x),
        y: word(y),
        buttons: buttons & 3,
    };
    let same =
        |e: &InputEvent| matches!(*e, InputEvent::Pointer { buttons: b, .. } if b == buttons & 3);
    if pending.last().is_some_and(same) {
        pending.pop();
    }
    pending.push(ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use u6_testkit::{Quiet, assert_same};

    #[test]
    fn pointer_moves_collapse_and_button_changes_queue() {
        let mut q = Vec::new();
        let p = |x, y, buttons| InputEvent::Pointer { x, y, buttons };
        queue_pointer(&mut q, 10, 10, 0);
        queue_pointer(&mut q, 20, 30, 0);
        queue_pointer(&mut q, 20, 30, 1);
        queue_pointer(&mut q, 21, 31, 5);
        queue_pointer(&mut q, 21, 31, 0);
        assert_eq!(q, [p(20, 30, 0), p(21, 31, 1), p(21, 31, 0)]);
        q.push(InputEvent::Key(Key::Space));
        queue_pointer(&mut q, 99_999, -99_999, 2);
        assert_eq!(
            q.last(),
            Some(&p(i16::MAX, i16::MIN, 2)),
            "a key in between keeps both"
        );
        let mut game = Game::new();
        game.pointer(5, 5, 1);
        assert!(game.pending.is_empty(), "no game, nothing queued");
    }

    #[test]
    fn maps_named_codes_and_single_characters() {
        assert_eq!(
            key_from_dom("ArrowRight", "ArrowRight", false),
            Some(Key::Right)
        );
        assert_eq!(
            key_from_dom("NumpadEnter", "Enter", false),
            Some(Key::Enter)
        );
        assert_eq!(key_from_dom("KeyT", "t", false), Some(Key::Char('t')));
        assert_eq!(key_from_dom("KeyT", "T", false), Some(Key::Char('T')));
        assert_eq!(key_from_dom("ShiftLeft", "Shift", false), None);
        assert_eq!(key_from_dom("F1", "F1", false), None);
        // Ctrl: the command loop's control keys only.
        assert_eq!(key_from_dom("KeyS", "s", true), Some(Key::Char('\u{13}')));
        assert_eq!(key_from_dom("KeyR", "r", true), Some(Key::Char('\u{12}')));
        assert_eq!(key_from_dom("KeyL", "l", true), None);
        assert_eq!(key_from_dom("ArrowUp", "ArrowUp", true), None);
    }

    #[test]
    fn before_start_the_frame_is_black_and_keys_are_not_queued() {
        let mut game = Game::new();
        assert_eq!(game.frame_len(), 320 * 200 * 4);
        assert!(game.frame_rgba().chunks(4).all(|p| p == [0, 0, 0, 255]));
        assert!(game.key_down("ArrowUp", "ArrowUp", false));
        assert!(!game.key_down("ShiftLeft", "Shift", false));
        assert!(game.pending.is_empty());
        game.tick(16);
        assert!(!game.started());
    }

    #[test]
    fn inputs_over_the_limit_are_refused_and_not_kept() {
        let mut game = Game::new();
        game.collect("a.adf", vec![0; 1 << 20]).unwrap();
        let err = game.collect("b.zip", vec![0; MAX_INPUT_BYTES]).unwrap_err();
        assert!(err.contains("at most 512 MiB"), "{err}");
        assert_eq!((game.files_loaded(), game.input_bytes), (1, 1 << 20));
    }

    #[test]
    fn start_without_game_files_reports_the_missing_file() {
        let mut game = Game::new();
        game.collect("notes.txt", b"hello".to_vec()).unwrap();
        assert_eq!(game.files_loaded(), 1);
        let report = game.boot(0, false).unwrap_err();
        assert!(
            report.starts_with("PP20: decrunched 0 file(s); 1 file(s) total"),
            "{report}"
        );
        assert!(
            report.ends_with("maptiles.ega: not found in the dropped files"),
            "{report}"
        );
        assert!(!game.started());
        assert_eq!(game.files_loaded(), 0);
    }

    /// Stored save files: one larger than any save file is left out, the files together stop
    /// at a total, a path given again replaces its file; what was left out is in the start's
    /// report (synthetic bytes).
    #[test]
    fn save_files_over_the_caps_are_left_out_and_reported() {
        let mut game = Game::new();
        game.load_save("savegame/objblkAA", vec![0; 1 << 20]);
        game.load_save("savegame/objlist", vec![0; 0x1c73]);
        for _ in 0..3 {
            game.load_save("savegame/objblkAB", vec![0; 20_000]);
        }
        assert!(!game.saves.contains_key("savegame/objblkAA"), "1 MiB");
        assert!(game.saves.contains_key("savegame/objlist"));
        assert!(
            game.saves.contains_key("savegame/objblkAB"),
            "replaced, not added up"
        );
        for i in 0..100 {
            game.load_save(&format!("savegame/x{i:03}"), vec![0; 20_000]);
        }
        let taken = game.saves.len();
        assert!((60..102).contains(&taken), "{taken} files taken");
        let kept: usize = game.saves.values().map(Vec::len).sum();
        assert!(kept < 2 << 20, "{kept} bytes");
        game.collect("notes.txt", b"hello".to_vec()).unwrap();
        let report = game.boot(0, false).unwrap_err();
        let notes = report.lines().filter(|l| l.contains("left out")).count();
        assert_eq!(notes, 1 + 102 - taken, "{report}");
        assert!(report.contains("savegame/objblkAA"), "{report}");
    }

    const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

    /// On the user's zip (skipped when absent; read only): the disk entries declare well under
    /// the unzip limit, the start keeps every game file in place, and those files given back as
    /// loose files start the same game.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn real_zip_keeps_its_game_files_and_restarts_from_them() {
        if !std::path::Path::new(ZIP).exists() {
            eprintln!("skipped: {ZIP} not present");
            return;
        }
        let data = std::fs::read(ZIP).quiet();
        let unzipped = zipscan::declared_disk_bytes("ultima-vi.zip", &data)
            .quiet()
            .quiet();
        assert!(
            unzipped > 0 && unzipped < MAX_UNZIPPED_BYTES,
            "under the unzip limit"
        );
        let mut game = Game::new();
        game.collect("ultima-vi.zip", data).quiet();
        game.boot(1, false).quiet();
        let n = game.kept_count();
        assert!(n == game.game_files());
        let kept: Vec<(String, Vec<u8>)> = (0..n)
            .map(|i| (game.kept_path(i), game.kept_bytes(i).quiet().to_vec()))
            .collect();
        assert!(kept.iter().all(|(_, b)| !b.is_empty()));
        game.release_kept();
        assert!((game.kept_count(), game.kept_len(0)) == (0, 0));
        let mut again = Game::new();
        for (path, bytes) in kept {
            again.collect(&path, bytes).quiet();
        }
        again.boot(1, false).quiet();
        assert!(again.game_files() == n);
        for _ in 0..60 {
            game.tick(16);
            again.tick(16);
        }
        assert!(game.frame_rgba() == again.frame_rgba(), "same first frames");
        assert!(
            game.program() == "u",
            "a start runs the launcher: `u` first"
        );
        assert!(!game.finished());
    }

    /// A Save's files listed for the page, stored, and laid over the same game files (`game`
    /// started at once): the next start resumes where the party stood; the kept game files stay the new-game ones; the
    /// sound machine starts and renders.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn a_save_listed_and_laid_over_restarts_there() {
        if !std::path::Path::new(ZIP).exists() {
            eprintln!("skipped: {ZIP} not present");
            return;
        }
        let data = std::fs::read(ZIP).quiet();
        let mut game = Game::new();
        game.collect("ultima-vi.zip", data.clone()).quiet();
        game.boot(1, true).quiet();
        let ticks = |game: &mut Game, n: u32| {
            for _ in 0..n {
                game.tick(16);
            }
        };
        ticks(&mut game, 120);
        assert!(game.program() == "game");
        assert!((game.save_generation(), game.save_collect()) == (0, 0));
        let start = game.leader_position();
        game.key_down("ArrowRight", "ArrowRight", false);
        ticks(&mut game, 400);
        let moved = game.leader_position();
        assert!(moved != start, "one step east");
        game.key_down("KeyS", "s", true);
        ticks(&mut game, 30);
        game.key_down("KeyY", "y", false);
        ticks(&mut game, 60);
        assert!(game.save_generation() == 1, "Ctrl-S, Y saved");
        assert!(game.leader_position() == moved);
        let n = game.save_collect();
        assert!(n > 1, "objlist and blocks");
        let files: Vec<(String, Vec<u8>)> = (0..n)
            .map(|i| {
                let len = game.save_len(i);
                let bytes = unsafe { std::slice::from_raw_parts(game.save_ptr(i), len) };
                (game.save_path(i), bytes.to_vec())
            })
            .collect();
        assert!(files.iter().any(|(p, _)| p == "savegame/objlist"));
        game.save_committed(1);
        assert!(game.save_collect() == 0, "nothing new after the commit");
        let kept: Vec<(String, Vec<u8>)> = (0..game.kept_count())
            .map(|i| (game.kept_path(i), game.kept_bytes(i).quiet().to_vec()))
            .collect();
        let mut again = Game::new();
        for (path, bytes) in &kept {
            again.collect(path, bytes.clone()).quiet();
        }
        for (path, bytes) in files {
            again.load_save(&path, bytes);
        }
        again.boot(1, true).quiet();
        assert!(again.save_files() == n);
        assert!(
            again.game_files() == game.game_files(),
            "the save is not a game file"
        );
        assert!(again.leader_position() == moved);
        let mut fresh = Game::new();
        for (path, bytes) in kept {
            fresh.collect(&path, bytes).quiet();
        }
        fresh.boot(1, true).quiet();
        assert!(
            fresh.leader_position() == start,
            "the kept files are the new game's"
        );
        assert!(fresh.audio_start(48_000).quiet());
        let mut heard = 0f32;
        for _ in 0..60 {
            fresh.tick(16);
            let n = fresh.audio_render(768);
            assert!(n == 1536);
            heard = heard.max(fresh.audio_buf.iter().fold(0f32, |m, &v| m.max(v.abs())));
        }
        assert!(heard > 0.0, "the boot's song renders");
    }
    /// The stored save removed under a game that started from one: its next Save lists the
    /// whole save (objlist and the 69 blocks), so the next start from it is the world that
    /// game had. A block of the save it started from that no Save rewrote (synthetic: an empty
    /// block, the last one the first Save did not list) is in the list, with that save's bytes,
    /// and the restarted world holds it.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn a_save_after_the_stored_one_was_removed_lists_the_whole_world() {
        if !std::path::Path::new(ZIP).exists() {
            eprintln!("skipped: {ZIP} not present");
            return;
        }
        let ticks = |game: &mut Game, n: u32| {
            for _ in 0..n {
                game.tick(16);
            }
        };
        let save = |game: &mut Game| {
            game.key_down("KeyS", "s", true);
            ticks(game, 30);
            game.key_down("KeyY", "y", false);
            ticks(game, 60);
        };
        let listed = |game: &mut Game| -> BTreeMap<String, Vec<u8>> {
            (0..game.save_collect())
                .map(|i| {
                    let len = game.save_len(i);
                    let bytes = unsafe { std::slice::from_raw_parts(game.save_ptr(i), len) };
                    (game.save_path(i), bytes.to_vec())
                })
                .collect()
        };
        let blocks = |game: &Game| -> Vec<Vec<u8>> {
            let s = game.engine.as_ref().quiet().state();
            s.objects.blocks.iter().map(|(_, b)| b.to_bytes()).collect()
        };
        let mut first = Game::new();
        first
            .collect("ultima-vi.zip", std::fs::read(ZIP).quiet())
            .quiet();
        first.boot(1, true).quiet();
        ticks(&mut first, 120);
        save(&mut first);
        assert!(first.save_generation() == 1);
        let mut stored = listed(&mut first);
        let far_index = (0..BLOCK_COUNT)
            .rev()
            .find(|&i| !stored.contains_key(&u6_core::saveload::block_file(i)))
            .expect("a Save near the start leaves blocks out");
        let far = u6_core::saveload::block_file(far_index);
        stored.insert(far.clone(), vec![0, 0]);
        let kept: Vec<(String, Vec<u8>)> = (0..first.kept_count())
            .map(|i| (first.kept_path(i), first.kept_bytes(i).quiet().to_vec()))
            .collect();
        let start = |save: &BTreeMap<String, Vec<u8>>| {
            let mut game = Game::new();
            for (path, bytes) in &kept {
                game.collect(path, bytes.clone()).quiet();
            }
            for (path, bytes) in save {
                assert!(game.load_save(path, bytes.clone()));
            }
            game.boot(1, true).quiet();
            ticks(&mut game, 120);
            game
        };
        let mut game = start(&stored);
        assert_same!(blocks(&game)[far_index], [0, 0], "the save's far block");
        game.save_forgotten();
        save(&mut game);
        assert!(game.save_generation() == 1);
        let whole = listed(&mut game);
        assert!(
            whole.len() == 1 + BLOCK_COUNT,
            "the objlist and every block"
        );
        let far_bytes = whole.get(&far).expect("the far block is listed");
        assert_same!(far_bytes, [0, 0], "the listed far block");
        let total: usize = whole.values().map(Vec::len).sum();
        assert!(total <= MAX_SAVE_BYTES, "within the save cap");
        game.save_committed(1);
        let again = start(&whole);
        assert_same!(
            blocks(&again),
            blocks(&game),
            "the same world, block for block"
        );
        assert!(again.leader_position() == game.leader_position());
        save(&mut game);
        assert!(
            listed(&mut game).len() < 1 + BLOCK_COUNT,
            "once committed, Saves list changes again"
        );
    }

    use zipscan::tests::{Entry, stored, zip};

    /// A file of `size` bytes made of `pieces` (offset, bytes), zeros in its last bytes the first
    /// read of a sliced file takes (`tail_start(size)..size`), and nothing else: a read that
    /// touches a byte elsewhere, or a range that is reversed or runs past the end of the file, is
    /// a request the reader must never make, and panics. Records each range read.
    struct Virtual {
        size: u64,
        pieces: Vec<(u64, Vec<u8>)>,
        reads: std::cell::RefCell<Vec<(u64, u64)>>,
    }

    impl Virtual {
        fn new(size: u64, pieces: Vec<(u64, Vec<u8>)>) -> Self {
            Virtual {
                size,
                pieces,
                reads: Default::default(),
            }
        }

        fn whole(data: &[u8]) -> Self {
            Virtual::new(data.len() as u64, vec![(0, data.to_vec())])
        }

        fn read(&self, start: u64, end: u64) -> Vec<u8> {
            assert!(
                start <= end && end <= self.size,
                "a request outside the file: {start}..{end}"
            );
            self.reads.borrow_mut().push((start, end));
            let tail = u6_intake::zip::tail_start(self.size)..self.size;
            (start..end)
                .map(|i| {
                    let mut held = self.pieces.iter();
                    let piece = held.find(|(at, b)| (*at..*at + b.len() as u64).contains(&i));
                    match piece {
                        Some((at, b)) => b[(i - at) as usize],
                        None if tail.contains(&i) => 0,
                        None => panic!("a forbidden request: {start}..{end}"),
                    }
                })
                .collect()
        }

        fn reads(&self) -> Vec<(u64, u64)> {
            self.reads.borrow().clone()
        }
    }

    /// Reads `file` into `game` through the slicing calls, as the page does (`zip_sliced` files);
    /// returns the number of reads.
    fn sparse_in(game: &mut Game, path: &str, file: &Virtual) -> Result<usize, String> {
        game.open_zip(path, file.size as f64)?;
        let mut n = 0;
        while let Some((start, end)) = game.zip_range()? {
            game.zip_bytes(file.read(start, end))?;
            n += 1;
        }
        Ok(n)
    }

    fn slice_in(game: &mut Game, path: &str, data: &[u8]) -> Result<usize, String> {
        sparse_in(game, path, &Virtual::whole(data))
    }

    /// The zip `zip` as a file of `size` bytes: its entries at the start (with the zeros an
    /// entry's range may run on when no record follows it, the longest local extra field), its
    /// central directory `gap` bytes before its end record, which is at the end (the record's
    /// directory offset moved there); nothing between. Reading the bytes between is a forbidden
    /// request.
    fn far(zip: &[u8], size: u64, gap: u64) -> Virtual {
        let e = zip.len() - 22;
        let at = u32::from_le_bytes(zip[e + 16..e + 20].try_into().unwrap()) as usize;
        let dir = &zip[at..e];
        let mut end = zip[e..].to_vec();
        let moved = size - 22 - gap - dir.len() as u64;
        end[16..20].copy_from_slice(&(moved as u32).to_le_bytes());
        let mut head = zip[..at].to_vec();
        head.resize(at + 0xffff, 0);
        let pieces = if gap == 0 {
            vec![(0, head), (moved, [dir, &end].concat())]
        } else {
            vec![(0, head), (moved, dir.to_vec()), (size - 22, end)]
        };
        Virtual::new(size, pieces)
    }

    /// A deflated disk-image entry that declares `full` unpacked (its data: an empty deflate
    /// stream, so nothing is ever inflated from it).
    fn declaring(name: &str, full: u32) -> Entry<'_> {
        Entry {
            method: 8,
            full,
            ..stored(name, b"\x03\x00")
        }
    }

    #[test]
    fn large_zips_are_read_in_slices_and_the_message_states_the_rule() {
        let mib = |n: u64| (n << 20) as f64;
        assert!(zip_sliced("ultima-vi.zip", mib(17)) && zip_sliced("a/U.ZIP", mib(600)));
        assert!(
            !zip_sliced("small.zip", mib(16)),
            "a small zip is read whole"
        );
        assert!(!zip_sliced("disk.hfe", mib(600)) && !zip_sliced("ü", mib(600)));
        let msg = too_large(600 << 20);
        assert!(msg.starts_with(
            "The dropped files are 600 MiB together; this page takes at most 512 MiB"
        ));
        assert!(
            msg.contains("a zip over 16 MiB counts only what is read of it"),
            "{msg}"
        );
        assert_eq!(input_too_large(mib(600)), msg);
    }

    /// A zip read in slices gives the intake what the zip read whole gives (the same report:
    /// the same disk-image entries, by the same filter, in the same order); only the first
    /// bytes, the directory and those entries are read, and the rebuilt zip is no larger than
    /// what was read (synthetic bytes).
    #[test]
    fn a_zip_read_in_slices_starts_the_intake_the_whole_zip_starts() {
        let filler = vec![1u8; 2 << 20];
        let (one, two) = (vec![2u8; 4096], vec![3u8; 3000]);
        let data = zip(&[
            stored("scans/filler.bin", &filler),
            stored("d/Disk 1.adf", &one),
            stored("__MACOSX/d/Disk 1.adf", b"mac"),
            stored("d/._Disk 2.adf", b"dot"),
            stored("notes.txt", b"n"),
            stored("Disk 2.HFE", &two),
        ]);
        let mut whole = Game::new();
        whole.collect("u6.zip", data.clone()).unwrap();
        let expected = whole.boot(0, false).unwrap_err();
        assert!(
            expected.starts_with("u6.zip: zip, 2 disk image(s): Disk 1.adf, Disk 2.HFE\n"),
            "{expected}"
        );
        let mut sliced = Game::new();
        let n = slice_in(&mut sliced, "u6.zip", &data);
        assert_eq!(n, Ok(4), "the first bytes, the tail and two entries");
        assert_eq!(sliced.files_loaded(), 1);
        assert!(
            sliced.input_bytes < 80 << 10,
            "{} bytes",
            sliced.input_bytes
        );
        assert!(
            sliced.inputs[0].data.len() <= sliced.input_bytes + u6_intake::zip::REBUILT_COMMENT,
            "the rebuilt zip"
        );
        assert_eq!(sliced.input_held(), sliced.input_bytes as f64);
        assert_eq!(sliced.boot(0, false).unwrap_err(), expected);
    }

    /// A large file named .zip that does not start as one is read whole after its first bytes,
    /// and collected as the whole path collects it (synthetic bytes).
    #[test]
    fn a_large_file_named_zip_that_is_not_one_is_read_whole_like_the_whole_path() {
        let mut data = b"not a zip at all".to_vec();
        data.resize(17 << 20, 7);
        let mut whole = Game::new();
        whole.collect("odd.zip", data.clone()).unwrap();
        let expected = whole.boot(0, false).unwrap_err();
        let file = Virtual::whole(&data);
        let mut sliced = Game::new();
        assert_eq!(sparse_in(&mut sliced, "odd.zip", &file), Ok(2));
        assert_eq!(file.reads(), [(0, 4), (0, data.len() as u64)]);
        assert_eq!(sliced.input_bytes, 4 + data.len());
        assert!(sliced.inputs[0].data == data);
        assert_eq!(sliced.boot(0, false).unwrap_err(), expected);
    }

    /// A zip read whole whose directory does not read is refused before the intake runs, so
    /// what its disk images would yield is never unchecked: an entry declaring more than the
    /// unzip limit ahead of an entry past the end of the file, or ahead of a record the end
    /// record counts and the directory lacks. The control, that entry alone, is refused for what
    /// it declares (synthetic bytes).
    #[test]
    fn a_whole_zip_whose_directory_does_not_read_is_refused_before_the_intake() {
        let big = declaring("Disk 1.adf", 100 << 20);
        let past = Entry {
            csize: 1 << 20, // its data runs past the end of the file
            ..stored("Disk 2.adf", &[2; 64])
        };
        let past_end = zip(&[big, past]);
        let mut counted = zip(&[big]);
        let e = counted.len() - 22;
        counted[e + 8] = 2; // two records counted, one there
        counted[e + 10] = 2;
        for (what, data) in [
            ("an entry past the end", past_end),
            ("a record missing", counted),
        ] {
            let mut game = Game::new();
            game.collect("u6.zip", data).unwrap();
            let err = game.boot(0, false).unwrap_err();
            assert_eq!(err, "u6.zip: zip unreadable: invalid zip data", "{what}");
        }
        let mut game = Game::new();
        game.collect("u6.zip", zip(&[big])).unwrap();
        assert_eq!(
            game.boot(0, false),
            Err(unpacks_too_much(100 << 20)),
            "the control"
        );
    }

    /// A zip of 600 MiB of which only the parts made here exist: taken, as what is read of it
    /// fits; nothing between the parts is ever asked for. Entries whose ranges would pass the
    /// input limit, disk images yielding more than the unzip limit, a directory more than
    /// `MAX_DIRECTORY_BYTES` before the end: refused after the tail read, before any entry or
    /// the directory is read, keeping nothing.
    #[test]
    fn a_sliced_zip_counts_the_bytes_read_and_refuses_past_the_limits_before_reading() {
        let size = 600u64 << 20;
        let disk = [2u8; 4096];
        let ok = far(&zip(&[stored("Disk 1.adf", &disk)]), size, 0);
        let mut game = Game::new();
        game.collect("loose.txt", b"loose".to_vec()).unwrap();
        assert_eq!(sparse_in(&mut game, "big.zip", &ok), Ok(3));
        assert_eq!(ok.reads().len(), 3, "the first bytes, the tail, the entry");
        assert!(game.input_bytes < 1 << 20, "{} bytes", game.input_bytes);
        assert!(
            game.inputs[1].data.len() <= game.input_bytes + u6_intake::zip::REBUILT_COMMENT,
            "the rebuilt zip"
        );
        let report = game.boot(0, false).unwrap_err();
        assert!(
            report.starts_with("big.zip: zip, 1 disk image(s): Disk 1.adf\n"),
            "{report}"
        );

        let packed = Entry {
            csize: 550 << 20, // deflated, declaring 1 MiB unpacked: its data is 550 MiB
            ..declaring("Disk 1.adf", 1 << 20)
        };
        let dir_far = u6_intake::zip::MAX_DIRECTORY_BYTES + 1;
        let unreadable = Some("big.zip: zip unreadable: invalid zip data".to_string());
        let cases = [
            (
                "ranges past the input limit",
                far(&zip(&[packed]), size, 0),
                None,
            ),
            (
                "disk images past the unzip limit",
                far(&zip(&[declaring("Disk 1.adf", 100 << 20)]), size, 0),
                Some(unpacks_too_much(100 << 20)),
            ),
            (
                "a directory too far from the end",
                far(&zip(&[stored("Disk 1.adf", &disk)]), size, dir_far),
                unreadable,
            ),
        ];
        for (what, file, message) in cases {
            let mut game = Game::new();
            game.collect("loose.txt", b"loose".to_vec()).unwrap();
            let err = sparse_in(&mut game, "big.zip", &file).unwrap_err();
            match message {
                Some(m) => assert_eq!(err, m, "{what}"),
                None => assert!(
                    err.contains("this page takes at most 512 MiB"),
                    "{what}: {err}"
                ),
            }
            let tail = (u6_intake::zip::tail_start(size), size);
            assert_eq!(
                file.reads(),
                [(0, 4), tail],
                "{what}: the first bytes and the tail"
            );
            assert_eq!(
                (game.files_loaded(), game.input_bytes),
                (1, 5),
                "{what}: nothing kept"
            );
            assert!(game.zip.is_none(), "{what}");
        }
    }

    /// `zip` (no comment) with Zip64 end records: a Zip64 end record and locator before its end
    /// record, which then holds the sentinels.
    fn with_zip64(zip: &[u8]) -> Vec<u8> {
        let e = zip.len() - 22;
        let field = |at: usize| u32::from_le_bytes(zip[e + at..e + at + 4].try_into().unwrap());
        let count = u64::from(u16::from_le_bytes([zip[e + 8], zip[e + 9]]));
        let (len, at) = (u64::from(field(12)), u64::from(field(16)));
        let mut out = zip[..e].to_vec();
        let z = out.len() as u64;
        out.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
        out.extend_from_slice(&44u64.to_le_bytes());
        out.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // versions, disks
        for v in [count, count, len, at] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&z.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]); // disks, counts
        out.extend_from_slice(&[0xff; 8]); // directory size and offset
        out.extend_from_slice(&[0, 0]);
        out
    }

    /// One record over `MAX_ENTRIES`, every record there and valid (the whole path reads such a
    /// directory to its end), without and with Zip64 end records: refused after the first bytes
    /// and the tail, before the directory (which starts before the tail) is read.
    #[test]
    fn a_valid_directory_over_the_record_limit_is_refused_before_it_is_read() {
        let max = u6_intake::zip::MAX_ENTRIES as usize;
        let names: Vec<String> = (0..max + 1).map(|i| format!("{i:05}")).collect();
        let mut list: Vec<Entry> = names.iter().map(|n| stored(n, b"")).collect();
        list[0] = stored("Disk 1.adf", &[2; 64]);
        let at_limit = zip(&list[..max]);
        let mut game = Game::new();
        assert_eq!(
            slice_in(&mut game, "u6.zip", &at_limit),
            Ok(4),
            "at the limit: read"
        );
        let plain = zip(&list);
        for (what, data) in [("plain", plain.clone()), ("Zip64", with_zip64(&plain))] {
            let file = Virtual::whole(&data);
            let size = file.size;
            let mut game = Game::new();
            let err = sparse_in(&mut game, "u6.zip", &file).unwrap_err();
            assert_eq!(err, "u6.zip: zip unreadable: invalid zip data", "{what}");
            let tail = (u6_intake::zip::tail_start(size), size);
            assert_eq!(file.reads(), [(0, 4), tail], "{what}: not the directory");
            let mut whole = Game::new();
            whole.collect("u6.zip", data).unwrap();
            assert_eq!(whole.boot(0, false).unwrap_err(), err, "{what}: read whole");
        }
    }

    /// The zip assembled from the entries is what the page holds of them: their ranges fit with
    /// what is held, the zip of them (with its central directory) does not. Refused after the
    /// tail, before the entry is read or the zip's room reserved (reading it would be a
    /// forbidden request).
    #[test]
    fn a_sliced_zip_whose_rebuilt_zip_would_pass_the_limit_is_refused_before_reading() {
        let size = 600u64 << 20;
        let max = MAX_INPUT_BYTES as u64;
        let tail = size - u6_intake::zip::tail_start(size);
        let held = 5 + 4 + tail; // the loose file, the first bytes, the tail
        let room = max - held - 10; // the entry's range: from its local header, 64 KiB past its data
        let entry = Entry {
            csize: (room - 0xffff - 40) as u32,
            ..declaring("Disk 1.adf", 1 << 20)
        };
        let file = far(&zip(&[entry]), size, 0);
        let mut game = Game::new();
        game.collect("loose.txt", b"loose".to_vec()).unwrap();
        let err = sparse_in(&mut game, "big.zip", &file).unwrap_err();
        let rebuilt = room + 2 * (46 + 10) + 22 + u6_intake::zip::REBUILT_COMMENT as u64;
        assert_eq!(err, too_large(held + rebuilt));
        assert!(
            held + room <= max && held + rebuilt > max,
            "only the rebuilt zip passes"
        );
        assert_eq!(file.reads(), [(0, 4), (size - tail, size)], "not the entry");
        assert_eq!(
            (game.files_loaded(), game.input_bytes),
            (1, 5),
            "nothing kept"
        );
    }

    /// A file that does not read as the part of a zip it was asked for, an entry past the end of
    /// the file, a local header that disagrees with its central record: a clean error naming the
    /// zip, and nothing of it kept (synthetic bytes).
    #[test]
    fn a_sliced_zip_that_does_not_read_is_refused_and_nothing_is_kept() {
        let disk = vec![2u8; 4096];
        let good = zip(&[stored("Disk 1.adf", &disk), stored("Disk 2.adf", &disk)]);
        let past = zip(&[
            stored("Disk 1.adf", &disk),
            Entry {
                csize: 1 << 20,
                ..stored("Disk 2.adf", &disk)
            },
        ]);
        let mut cut = good.clone();
        cut.truncate(good.len() - 30);
        let second = 30 + 10 + 4096;
        let mut renamed = good.clone();
        renamed[second + 30 + 5] = b'3'; // the second local header names "Disk 3.adf"
        let mut unsigned = good.clone();
        unsigned[second] ^= 1;
        for (what, data) in [
            ("a cut tail", &cut),
            ("an entry past the end", &past),
            ("a local header with another name", &renamed),
            ("a local header without its signature", &unsigned),
        ] {
            let mut game = Game::new();
            game.collect("loose.txt", b"loose".to_vec()).unwrap();
            let err = slice_in(&mut game, "u6.zip", data).unwrap_err();
            assert_eq!(err, "u6.zip: zip unreadable: invalid zip data", "{what}");
            let held = (game.files_loaded(), game.input_bytes);
            assert_eq!(held, (1, 5), "{what}: nothing kept");
            assert!(game.zip.is_none() && game.zip_range() == Ok(None), "{what}");
            let mut whole = Game::new();
            whole.collect("u6.zip", data.clone()).unwrap();
            // Read whole: refused by the check before the intake (the directory) or by the
            // intake (a local header), the same line first either way.
            let err = whole.boot(0, false).unwrap_err();
            let line = err.lines().next();
            assert_eq!(
                line,
                Some("u6.zip: zip unreadable: invalid zip data"),
                "{what}: whole"
            );
        }
        let mut game = Game::new();
        assert!(game.open_zip("u6.zip", f64::NAN).is_err());
        assert!(game.zip_bytes(b"PK".to_vec()).is_err(), "no zip open");
        game.open_zip("u6.zip", good.len() as f64).unwrap();
        assert_eq!(game.zip_range(), Ok(Some((0, 4))));
        assert!(
            game.zip_bytes(b"PK".to_vec()).is_err(),
            "not the range asked for"
        );
        assert!(game.zip.is_none(), "dropped");
    }

    /// On the user's zip (skipped when absent; read only): read in slices, a tenth of it at most
    /// is copied in, and the start keeps the same game files and shows the same frames as the
    /// start from the zip read whole (content-free comparisons; nothing of the zip is printed).
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn the_users_zip_read_in_slices_starts_the_game_the_whole_zip_starts() {
        if !std::path::Path::new(ZIP).exists() {
            eprintln!("skipped: {ZIP} not present");
            return;
        }
        let data = std::fs::read(ZIP).quiet();
        assert!(
            zip_sliced("ultima-vi.zip", data.len() as f64),
            "read in slices"
        );
        let mut sliced = Game::new();
        slice_in(&mut sliced, "ultima-vi.zip", &data).quiet();
        assert!(
            sliced.input_bytes < data.len() / 10,
            "a tenth of the zip at most"
        );
        sliced.boot(1, false).quiet();
        let mut whole = Game::new();
        whole.collect("ultima-vi.zip", data).quiet();
        whole.boot(1, false).quiet();
        let kept = |game: &Game| -> Vec<(String, Vec<u8>)> {
            (0..game.kept_count())
                .map(|i| (game.kept_path(i), game.kept_bytes(i).quiet().to_vec()))
                .collect()
        };
        assert!(sliced.kept_count() > 0);
        assert_same!(kept(&sliced), kept(&whole), "the kept game files");
        for _ in 0..60 {
            sliced.tick(16);
            whole.tick(16);
        }
        assert!(
            sliced.frame_rgba() == whole.frame_rgba(),
            "the same first frames"
        );
    }
}

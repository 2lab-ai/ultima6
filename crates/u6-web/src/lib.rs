//! Browser entry point: collects the dropped files, runs u6-intake, boots `u6_core::Engine`
//! and hands frames back as RGBA8 (see `site/app.js`).
//!
//! JS owns the clock and the DOM. Key callbacks only queue events; [`Game::step`] delivers them
//! to the engine with the step's time, so game code never runs inside an input handler.
//!
//! The fixed step (spec 06): each display frame the page asks [`Game::pace`] how many steps of
//! [`step_ms`] its wall time is worth, runs them with [`Game::step`] while its budget
//! ([`budget_ms`]) lasts, reports them ([`Game::ran`]) and converts the frame once, if the steps
//! changed it ([`Game::present`]; the page draws the canvas only when [`Game::frame_version`]
//! moved). [`Game::set_speed`] picks the speed; [`Game::keys_owned`] says which of `-`, `+` and
//! `=` the game takes itself.
//!
//! HD (spec 07 HD0): with [`Game::set_hd`] on, each present also composes the HD frame
//! ([`Game::hd_frame_ptr`], `WIDTH·S × HEIGHT·S`, S 2 or 4; the frame is 320 × 200, 912 × 408
//! in Wide mode, specs 13 and 14: [`Game::width`], [`Game::height`]) from the published frame
//! (the shown frame without the pointer sprite, spec 11 Design 7) and the images its source plane
//! names (`u6_render::hd::Compositor`), when the frame, its sources or the HD setting changed
//! since the last composition; the page draws the pointer over it ([`Game::hd_pointer`]). Off,
//! nothing of it runs and the RGBA frame, the sprite in it, is as without it. An HD pack (spec 07
//! H2): the page tells a dropped pack from game files with a [`PackProbe`] (its zip directory and
//! manifest only), loads it once as an [`HdPack`] (checked whole, every image decoded: at most
//! [`MAX_PACK_FILE_BYTES`] read, at most `u6_hdpack::MAX_DECODED_BYTES` decoded) and sets it on
//! every game it starts ([`Game::set_hd_pack`]); a pack image whose classic image is the one the
//! game holds now goes in front of MMPX.
//!
//! Sound: once the page has an AudioContext it calls [`Game::audio_start`] with its sample
//! rate; every step then replays the game's MED calls on the emulated A500 audio machine
//! (`u6_core::sound::AudioDriver`) and [`Game::audio_render`] renders the samples the page
//! posts to its AudioWorklet. Saves: the page lays the stored `savegame/*` files over the
//! new-game files before the boot ([`Game::load_save`], `saveload::attach`), and after a frame
//! whose Save committed ([`Game::save_generation`] moved) it stores what [`Game::save_collect`]
//! lists, then reports the generation committed ([`Game::save_committed`]). The character `u`
//! makes is such a write too (its `savegame/objlist`). A list holds what the Saves since the
//! last commit replaced; after the page removed the stored save ([`Game::save_forgotten`]) the
//! next one holds the whole save.
//!
//! Voices (spec 09): with [`Game::set_voices`] on, the engine's speech tap keeps what NPCs say
//! in conversations, and the page takes it after each frame with [`Game::take_speech`] for its
//! on-device synthesis; off (the default), the tap keeps nothing and no frame changes either way.
//!
//! Remembered sight (spec 18): with [`Game::set_sight_memory`] on, the map window shows the cells
//! in view that were seen and are out of sight now in dark grey ([`u6_core::Io::set_sight_memory`]);
//! off (the default, the original's black), the frames are those without it.
//!
//! Korean (spec 08, slice K1): the page loads a dropped `ko.u6lp` once as a [`LocalePack`]
//! (the engine's strict reader: at most [`max_locale_pack_bytes`]) and, in Korean mode, sets it
//! on each game it starts ([`Game::set_locale`]): checked against that game's files, then its
//! conversation runs show the pack's lines, drawn with the Hangul table built in here
//! (`site/fonts/galmuri7-hangul.bin`, Galmuri7, OFL-1.1). The counts it gives are counts only.
//!
//! Slice K2 adds the pack's table strings and game strings, and [`Game::locale_counts`] counts
//! the table strings and game strings shown from the pack. Slice K3 adds its object names and
//! books, and the counts take them too. Slice K4 adds the strings of the `u` program (the
//! opening, the story, the gypsy, character creation, the transfer): a pack's show in `u` (which
//! runs on the game state the pack is set on, before its first step, as the game does), and the
//! counts take them too. The strings a pack is made from, and the NPC bundles (spec 09 Design 5),
//! are exported natively from the user's files (`tools/export`, `u6-export`), not here.
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

use std::rc::Rc;

use u6_core::hd::{HdView, PackCounts};
use u6_core::objects::{BLOCK_COUNT, MAX_RECORDS};
use u6_core::pacer::{self, Pacer, Speed};
use u6_core::saveload::{MemStore, OBJLIST_FILE, attach, block_file, changed_since};
use u6_core::sound::AudioDriver;
use u6_core::{Engine, Files, Framebuffer, InputEvent, Key};
use u6_intake::{InputFile, intake};
use u6_render::hd::{self, Scale};
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

/// The largest HD pack file the page reads (spec 07 HD1): the entries the reader takes
/// (`u6_hdpack::MAX_FILE_BYTES`) and the largest zip directory it reads
/// (`u6_intake::zip::MAX_DIRECTORY_BYTES`).
pub const MAX_PACK_FILE_BYTES: u64 =
    u6_hdpack::MAX_FILE_BYTES + u6_intake::zip::MAX_DIRECTORY_BYTES;

/// [`MAX_PACK_FILE_BYTES`] for the page (it checks a pack's size before it reads it).
#[wasm_bindgen]
pub fn max_pack_bytes() -> f64 {
    MAX_PACK_FILE_BYTES as f64
}

/// Spec 07 HD1: whether a dropped file is an HD pack, read in slices (`u6_hdpack::Probe`: the
/// end of the file, its zip directory if it starts before that, its `manifest.json`). The page
/// reads each range [`PackProbe::need`] names and hands its bytes to [`PackProbe::give`] until
/// [`PackProbe::told`].
#[wasm_bindgen]
pub struct PackProbe(u6_hdpack::Probe);

#[wasm_bindgen]
impl PackProbe {
    /// A file of `size` bytes (a size that is not one: not a pack).
    #[wasm_bindgen(constructor)]
    pub fn new(size: f64) -> PackProbe {
        let ok = size.is_finite() && size >= 0.0 && size.fract() == 0.0 && size < 2f64.powi(53);
        PackProbe(u6_hdpack::Probe::new(if ok { size as u64 } else { 0 }))
    }

    /// The next range to read, `[start, end]` (`end` exclusive), or empty once told.
    pub fn need(&self) -> Vec<f64> {
        self.0
            .need()
            .map_or_else(Vec::new, |(start, end)| vec![start as f64, end as f64])
    }

    /// The bytes of the range [`PackProbe::need`] named.
    pub fn give(&mut self, bytes: &[u8]) {
        self.0.give(bytes);
    }

    /// Whether nothing more is needed.
    pub fn told(&self) -> bool {
        self.0.is_pack().is_some()
    }

    /// Whether the file is an HD pack (false until told).
    pub fn is_pack(&self) -> bool {
        self.0.is_pack() == Some(true)
    }

    /// For a pack whose zip directory or manifest already breaks the format (an entry a pack
    /// does not have, entries over the limit, a manifest that does not read or is not this
    /// format's): why it is refused, the fixed sentence [`HdPack::load`] gives, so the page
    /// refuses it without reading it whole; empty otherwise.
    pub fn refusal(&self) -> String {
        self.0
            .refusal()
            .map_or_else(String::new, |e| refusal(e).into())
    }
}

/// Spec 08 Design 1: the Hangul table (Galmuri7, OFL-1.1, `site/fonts/OFL.txt`), one 8 × 8
/// cell per syllable, as `tools/font` wrote it.
const HANGUL: &[u8] = include_bytes!("../../../site/fonts/galmuri7-hangul.bin");

/// The Hangul table, read from [`HANGUL`]; `None` if it does not read (a syllable is then a
/// blank cell).
fn hangul() -> Option<Rc<u6_formats::font::HangulFont>> {
    u6_formats::font::HangulFont::parse(HANGUL)
        .ok()
        .map(Rc::new)
}

/// Spec 08: the largest Korean pack file the page reads (`u6_core::locale::MAX_PACK_BYTES`).
#[wasm_bindgen]
pub fn max_locale_pack_bytes() -> f64 {
    u6_core::locale::MAX_PACK_BYTES as f64
}

/// Spec 08 (slice K1): a Korean pack the page loaded ([`LocalePack::load`]), kept by the page
/// and set on each game it starts in Korean mode ([`Game::set_locale`]).
#[wasm_bindgen]
pub struct LocalePack {
    pack: Rc<u6_core::locale::Pack>,
}

#[wasm_bindgen]
impl LocalePack {
    /// Reads and checks `bytes` as a pack (`u6_core::locale::Pack::parse`). A refusal is the
    /// fixed sentence of the rule it breaks (no value of the file).
    pub fn load(bytes: Vec<u8>) -> Result<LocalePack, JsError> {
        load_locale(&bytes).map_err(|e| JsError::new(&e))
    }

    /// The lines it holds and the conversations they are in: `[runs, scripts]`.
    pub fn counts(&self) -> Vec<u32> {
        vec![self.pack.runs() as u32, self.pack.scripts() as u32]
    }

    /// Slice K2: the table strings and game strings it holds: `[tables, strings]`.
    pub fn string_counts(&self) -> Vec<u32> {
        vec![self.pack.tables() as u32, self.pack.strings() as u32]
    }

    /// Slice K3: the object names and the books it holds: `[look, books]`.
    pub fn text_counts(&self) -> Vec<u32> {
        vec![self.pack.look_entries() as u32, self.pack.books() as u32]
    }

    /// Slice K4: the strings of the `u` program it holds.
    pub fn program_strings(&self) -> u32 {
        self.pack.program_strings() as u32
    }

    /// The grammar it declares: `en` or `ko`.
    pub fn language(&self) -> String {
        match self.pack.language {
            u6_core::ui::Language::English => "en".into(),
            u6_core::ui::Language::Korean => "ko".into(),
        }
    }
}

/// [`LocalePack::load`] without the JS error type.
fn load_locale(bytes: &[u8]) -> Result<LocalePack, String> {
    let pack = u6_core::locale::Pack::parse(bytes).map_err(|e| e.to_string())?;
    Ok(LocalePack {
        pack: Rc::new(pack),
    })
}

/// Spec 07 H2: an HD pack the page loaded ([`HdPack::load`]): one for the page, set on each game
/// it starts ([`Game::set_hd_pack`]), freed when the page forgets it or loads another (a game it
/// is set on keeps it until [`Game::forget_hd_pack`] or until it is freed).
#[wasm_bindgen]
pub struct HdPack {
    pack: Rc<u6_core::hd::HdPack>,
}

#[wasm_bindgen]
impl HdPack {
    /// Reads and checks `bytes` as an HD pack (`u6_hdpack::read_pack`: the whole of it or
    /// nothing, every image decoded). Fails with the reader's reason, or when `bytes` is over
    /// [`MAX_PACK_FILE_BYTES`].
    pub fn load(bytes: Vec<u8>) -> Result<HdPack, JsError> {
        load_pack(&bytes).map_err(|msg| JsError::new(&msg))
    }

    /// The images the pack holds: tiles, portraits, panel bitmaps.
    pub fn counts(&self) -> Vec<u32> {
        counts(self.pack.counts())
    }

    /// The RGBA bytes its images take decoded.
    pub fn decoded_bytes(&self) -> f64 {
        self.pack.decoded_bytes() as f64
    }
}

/// [`HdPack::load`] without the JS error type. A refusal says which rule of the format the pack
/// breaks, in a fixed sentence: nothing of the pack (its names, its manifest's text) is in it.
fn load_pack(bytes: &[u8]) -> Result<HdPack, String> {
    if bytes.len() as u64 > MAX_PACK_FILE_BYTES {
        return Err(format!(
            "the file is over the {} MiB a pack can be.",
            MAX_PACK_FILE_BYTES >> 20
        ));
    }
    if !u6_intake::zip::is_zip(bytes) {
        return Err(NOT_A_ZIP.into());
    }
    let pack = u6_hdpack::read_pack(bytes).map_err(|e| refusal(&e).to_string())?;
    Ok(HdPack {
        pack: Rc::new(u6_core::hd::HdPack::new(pack)),
    })
}

/// The refusal of a file that does not start as a zip.
const NOT_A_ZIP: &str = "it is not a zip.";

/// The fixed sentence for a pack the reader refuses: which rule of the format it breaks.
fn refusal(e: &u6_hdpack::PackError) -> &'static str {
    use u6_hdpack::PackError;
    match e {
        PackError::Zip(_) => {
            "it does not read as a zip (its directory, or an entry's size or CRC-32, is wrong)."
        }
        PackError::Entry(_) => "it holds a file an HD pack does not have, or one twice.",
        PackError::Files(_) => "its files are not the ones its index names.",
        PackError::Manifest(_) => "its manifest.json is not that of a u6-hdpack/1 pack.",
        PackError::Index(_) => "its index.bin does not read.",
        PackError::Limit(_) => "it is over the format's limits (4096 images, 150 MB decoded).",
        PackError::Image(_) => "a picture is not a lossless WebP of the size its index gives it.",
    }
}

fn counts(c: PackCounts) -> Vec<u32> {
    [c.tiles, c.portraits, c.bitmaps].map(|n| n as u32).to_vec()
}

/// The [`u6_core::Kind`] of the page's number for it (0 classic, 1 tile, 2 portrait, 3 bitmap).
fn kind_of(kind: u8) -> Option<u6_core::Kind> {
    use u6_core::Kind;
    [Kind::Classic, Kind::Tile, Kind::Portrait, Kind::Bitmap]
        .into_iter()
        .find(|&k| k as u8 == kind)
}

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

/// Spec 11 Design 2 (rev 3), [`Game::hd_glyphs`]: the code of a listed symbol is this | its glyph.
const SYMBOL: u32 = 0x1_0000;
/// Spec 11 Design 3 (rev 3), [`Game::hd_glyphs`]: the code of a space the player typed is this |
/// 0x20.
const TYPED: u32 = 0x2_0000;

/// A listed cell's code as [`Game::hd_glyphs`] gives it, from the compositor's
/// ([`u6_render::hd::HdGlyph::code`]): a symbol's and a typed space's in their own ranges
/// ([`SYMBOL`], [`TYPED`]), every other as it is.
fn listed_code(code: u16) -> u32 {
    match code {
        hd::TYPED_SPACE => TYPED | 0x20,
        c => hd::symbol_glyph(c).map_or(u32::from(c), |g| SYMBOL | u32::from(g)),
    }
}

/// [`u6_core::pacer::STEP_MS`] for the page: the `ms` of every [`Game::step`].
#[wasm_bindgen]
pub fn step_ms() -> u32 {
    pacer::STEP_MS
}

/// [`u6_core::pacer::budget_ms`] for the page: the wall milliseconds its steps may take in one
/// display frame, from the measured interval between frames.
#[wasm_bindgen]
pub fn budget_ms(raf_interval_ms: f64) -> f64 {
    pacer::budget_ms(raf_interval_ms)
}

/// Spec 13: the rect of the frame the map is shown in, `[x, y, width, height]` in frame pixels
/// ([`u6_core::geom::SHOWN`]): the game's map window, (8, 8, 160, 160), in Original; beside the
/// UI's 320 columns, (328, 8, 576, 320), in Wide mode, where the page's layout covers its canvas
/// with it.
#[wasm_bindgen]
pub fn shown_map() -> Vec<u32> {
    let (x0, y0, x1, y1) = u6_core::geom::SHOWN;
    [x0, y0, x1 + 1 - x0, y1 + 1 - y0].map(u32::from).to_vec()
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

/// The most characters one [`Game::text_input`] queues: more than a conversation's line holds
/// (the engine also keeps no more text queued than that, `u6_core::engine`'s text room).
const TEXT_CHARS: usize = 64;

#[wasm_bindgen]
pub struct Game {
    inputs: Vec<InputFile>,
    /// Bytes copied in for `inputs` and the zip being read ([`MAX_INPUT_BYTES`]).
    input_bytes: usize,
    /// The zip being read in slices ([`Game::zip_open`]) and `input_bytes` before it.
    zip: Option<(zipscan::Sliced, usize)>,
    engine: Option<Engine>,
    pending: Vec<InputEvent>,
    /// The buttons of the last pointer event [`Game::step`] handed to the engine (0 before
    /// one): the pointer state before the first one queued now ([`queue_pointer`]).
    handed: u8,
    blank: Framebuffer,
    rgba: Vec<u8>,
    /// The engine frame `rgba` holds ([`Engine::frame_version`]); `None` while it holds the
    /// black frame before a start.
    rgba_holds: Option<u64>,
    /// Conversions into `rgba` so far ([`Game::frame_version`]).
    conversions: u64,
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
    /// The steps the page's wall time is worth, at the viewer's speed.
    pacer: Pacer,
    /// The HD frame ([`Game::set_hd`]).
    hd: HdView,
    /// The HD pack set ([`Game::set_hd_pack`]).
    hd_pack: Option<Rc<u6_core::hd::HdPack>>,
    /// Spec 11 Design 7: the pointer the page draws over the HD frame ([`Game::hd_pointer`]): its
    /// box in frame pixels (x, y, width, height; width and height 0 for none), its pixels as
    /// RGBA8, and their version, all as of the last [`Game::present`], [`Game::set_hd`] or start.
    pointer_box: [u32; 4],
    pointer_rgba: Vec<u8>,
    pointer_version: u64,
    /// Voices on ([`Game::set_voices`]): the engine's speech tap keeps what NPCs say.
    voices: bool,
    /// Spec 17: the context-menu choice whose target press waits for its command to read the
    /// target ([`Game::command_at`]).
    run: Option<u6_core::ui::menu::Run>,
    /// Spec 19 O1b: per physical key held (its DOM `code`), each engine key its presses and
    /// repeats were queued as, so its release takes back those ([`Game::key_up`]).
    held: Vec<(String, Key)>,
    /// Spec 18: the remembered sight on ([`Game::set_sight_memory`]).
    sight_memory: bool,
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
            handed: 0,
            blank: Framebuffer::new(),
            rgba: Vec::new(),
            rgba_holds: None,
            conversions: 0,
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
            pacer: Pacer::new(),
            hd: HdView::new(),
            hd_pack: None,
            pointer_box: [0; 4],
            pointer_rgba: Vec::new(),
            pointer_version: 0,
            voices: false,
            run: None,
            held: Vec::new(),
            sight_memory: false,
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

    /// Spec 14 Design 6-7: what the side panel shows, `[mode, member, actor, talking]`: its mode
    /// (0x90 stats, 0x91 roster, 0x92 inventory, another for the other modes), the party index
    /// of the member it shows, that member's actor slot (0 for none), and 1 while a conversation
    /// runs (its portrait in the panel area, whatever the mode), else 0; empty before a start.
    /// The page reads it at each paint in Wide: the window of the mode is the active one, the
    /// side panel's own in another mode or a conversation ([`u6_core::ui::panels::panel_view`]).
    pub fn panel_view(&self) -> Vec<i32> {
        self.engine.as_ref().map_or_else(Vec::new, |e| {
            let (mode, member, actor, talking) = u6_core::ui::panels::panel_view(&e.state());
            vec![
                i32::from(mode),
                i32::from(member),
                i32::from(actor),
                i32::from(talking),
            ]
        })
    }

    /// Spec 14 Design 7: the roster name of the member the side panel shows (the member of
    /// [`Game::panel_view`]; [`u6_core::ui::panels::panel_name`]), for the page's window titles;
    /// empty for none and before a start.
    pub fn panel_name(&self) -> String {
        self.engine.as_ref().map_or_else(String::new, |e| {
            String::from_utf8_lossy(&u6_core::ui::panels::panel_name(&e.state())).into_owned()
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

    /// Spec 17: the context menu of a right press at frame pixel (x, y): `""` unless the prompt
    /// is up ([`Game::at_prompt`], pointer moves queued since the last step let through) and the
    /// pixel is on the map or on an inventory or equipment slot (Wide's included); else JSON
    /// `{kind: "map"|"panel", cell: [x, y] | null, underlined: code | null, sections: [{object,
    /// kind: [type, frame] | null, name, entries: [{code, enabled, reason, theft}]}]}`.
    /// The name is the game's display name (the user's game text, for the page's menu only:
    /// never logged or kept). Reads the game only ([`u6_core::ui::menu::query`]).
    pub fn context_menu(&self, x: i32, y: i32) -> String {
        if !self.prompt_up() {
            return String::new();
        }
        let Some(e) = &self.engine else {
            return String::new();
        };
        let word = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        u6_core::ui::menu::query(&e.state(), e.io(), word(x), word(y))
            .map_or_else(String::new, |m| menu_json(&m))
    }

    /// Spec 17: runs context-menu entry `code` on the target at frame pixel (x, y) as the
    /// original's mouse does: a left press and release on the command's icon now, and for a
    /// command that reads a target (0x81..0x88) a left press and release at (x, y) once it reads
    /// it ([`Game::step`]; dropped if the prompt comes back first). Rest and Combat (0x89, 0x8a)
    /// are the icon press alone. No underline is written. False (nothing queued) unless the
    /// prompt is up and `code` is one of those.
    pub fn command_at(&mut self, code: u16, x: i32, y: i32) -> bool {
        if !self.prompt_up() || !(0x81..=0x8a).contains(&code) {
            return false;
        }
        let word = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        let Some(e) = &self.engine else {
            return false;
        };
        let start = u6_core::ui::menu::Run::start(&e.state(), code, (word(x), word(y)));
        let icon = match start {
            Some((icon, run)) => {
                self.run = Some(run);
                icon
            }
            None => u6_core::ui::menu::icon_press(code),
        };
        self.pending.extend(icon);
        true
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

    /// One engine tick of `ms` (the page's fixed step: [`step_ms`]) with the keys queued since
    /// the previous one: the engine's log taken, the sound machine given the game's calls, the
    /// queue emptied. The RGBA frame is left as it was ([`Game::present`]).
    pub fn step(&mut self, ms: u32) {
        self.step_run();
        if let Some(engine) = &mut self.engine {
            engine.tick(&self.pending, ms);
            self.log.extend(engine.take_log());
            if let Some(driver) = &mut self.audio {
                driver.sync(&engine.state().sound, engine.io().files());
            }
            self.handed = last_buttons(&self.pending).unwrap_or(self.handed);
        }
        self.pending.clear();
    }

    /// Converts the engine's frame into the RGBA frame ([`Game::frame_ptr`]) when it changed
    /// since the last conversion (spec 06 O1, [`Engine::frame_version`]): once per display
    /// frame, after its steps; steps that composed no new frame convert nothing. Before a start
    /// the RGBA frame stays black.
    ///
    /// With HD on ([`Game::set_hd`]) it then composes the HD frame when the published frame's
    /// colours or sources changed since the last composition (a move of the pointer sprite alone
    /// composes nothing, spec 11 Design 7), and takes the pointer the page draws over it
    /// ([`Game::hd_pointer`]).
    pub fn present(&mut self) {
        let version = self.engine.as_ref().map(Engine::frame_version);
        if version.is_some() && version != self.rgba_holds {
            self.redraw();
        }
        if self.engine.is_some() {
            self.compose_hd();
        }
        self.track_pointer();
    }

    /// Changes whenever the frame the page shows was made again: with HD off the RGBA frame
    /// converted ([`Game::present`], a start), with HD on the HD frame composed with a block
    /// changed, and HD switched or its scale changed ([`Game::set_hd`]). The page draws the
    /// canvas only when it moved since its last draw, or with HD on when the pointer it draws
    /// over the HD frame did ([`Game::hd_pointer`]: a move of the pointer alone moves only that).
    /// It never goes back, also across starts.
    pub fn frame_version(&self) -> f64 {
        self.conversions as f64
    }

    /// Spec 07 HD0: shows the HD frame (`on`) at scale `scale` (4 for 4×, any other value 2×),
    /// or the RGBA frame as without HD. A change composes the HD frame at once (when on) and
    /// moves [`Game::frame_version`], with no step: the page draws it right away.
    pub fn set_hd(&mut self, on: bool, scale: u32) {
        if self.hd.set(on, Scale::from_factor(scale)) {
            self.conversions += 1;
            self.compose_hd();
            self.track_pointer();
        }
    }

    /// Whether the page shows the HD frame ([`Game::set_hd`]).
    pub fn hd_on(&self) -> bool {
        self.hd.on()
    }

    /// The HD frame's scale: 2 or 4 ([`Game::set_hd`]; 2 until then).
    pub fn hd_scale(&self) -> u32 {
        self.hd.scale().factor() as u32
    }

    /// Spec 11: the HD frame shows text in the HD font (`on`: each listed cell,
    /// [`Game::hd_glyphs`], is its paper colour, for the page to draw the character over) or in
    /// the pixel font, as before. Off until called. A change with HD on composes the HD frame at
    /// once and moves [`Game::frame_version`], with no step, as [`Game::set_hd`] does; with HD
    /// off nothing is shown differently, so nothing moves until HD is switched on.
    pub fn set_hd_font(&mut self, on: bool) {
        if self.hd.set_font(on) && self.hd.on() {
            self.conversions += 1;
            self.compose_hd();
        }
    }

    /// Whether the HD frame shows text in the HD font ([`Game::set_hd_font`]).
    pub fn hd_font(&self) -> bool {
        self.hd.font()
    }

    /// Spec 11: every cell the HD frame shows in the HD font, every whole space, and every symbol
    /// drawn over the paper, of its last composition (the whole list, not what changed: the page
    /// that copies a rect of the HD frame draws every listed glyph over it again), 5 words per
    /// cell in raster order: the cell's x and y in frame pixels (multiples of 8, of the
    /// [`Game::width`] × [`Game::height`] frame), its code, its ink and its paper as 0xRRGGBB.
    /// The code: printable ASCII but the space, or a Hangul syllable's code point, drawn in the
    /// HD font; 0x20, a space the text writer drew whole, for which nothing is drawn but which a
    /// run of text may cross, its ink equal to its paper; rev 3, [`TYPED`] | 0x20, such a space
    /// the player typed (a run goes on across it whatever the spaces beside it); [`SYMBOL`] | its
    /// glyph (a code of the game's font), a symbol the game drew over the paper (the key wait's
    /// cursor, the more prompt's mark), its cell the paper in the HD frame and its pixels in
    /// [`Game::hd_symbol_rows`]. Empty while HD or the HD font is off. The list changes only
    /// with every block of the cells it adds, drops or changes written, so with
    /// [`Game::frame_version`] moved and those cells in the rect of [`Game::hd_take_dirty`].
    pub fn hd_glyphs(&self) -> Vec<u32> {
        let rgb = |c: u6_core::Rgb| u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b);
        self.hd
            .glyphs()
            .iter()
            .flat_map(|g| {
                [
                    u32::from(g.x),
                    u32::from(g.y),
                    listed_code(g.code),
                    rgb(g.ink),
                    rgb(g.paper),
                ]
            })
            .collect()
    }

    /// Spec 11 Design 2 (rev 3): the pixels of each symbol [`Game::hd_glyphs`] lists ([`SYMBOL`]),
    /// in its order, 8 bytes each: its rows of 8 pixels, bit 7 the leftmost, a set bit a pixel of
    /// its ink. Empty when the list holds none.
    pub fn hd_symbol_rows(&self) -> Vec<u8> {
        if self.hd.glyphs().is_empty() {
            return Vec::new();
        }
        self.hd
            .compositor()
            .symbol_rows()
            .iter()
            .flatten()
            .copied()
            .collect()
    }

    /// Spec 11 Design 7: the pointer the page draws over the HD frame, last, as the original's
    /// hardware sprite is shown above the screen (the HD frame is composed without it):
    /// `[x, y, width, height, version]`, its box in frame pixels (of the [`Game::width`] ×
    /// [`Game::height`] frame), clipped to the frame, and a version that moves whenever the box or
    /// the pixels ([`Game::hd_pointer_rgba`]) changed, also to or from no pointer. The box is
    /// empty (width and height 0, at 0, 0) when there is no sprite, the pointer is hidden, its
    /// box lies outside the frame, HD is off or no game runs; the version is given then too, so
    /// the page sees the pointer go. As of the last [`Game::present`], [`Game::set_hd`] or start.
    pub fn hd_pointer(&self) -> Vec<u32> {
        let mut out = self.pointer_box.to_vec();
        out.push(self.pointer_version as u32);
        out
    }

    /// Spec 11 Design 7: the pointer's pixels in its box ([`Game::hd_pointer`]), width × height
    /// RGBA8 in rows: value v ≠ 0 of the sprite's shape in the shown frame's colour 16 + v
    /// (the colours the RGBA frame shows it in), opaque; value 0 transparent (alpha 0). Empty
    /// with the box.
    pub fn hd_pointer_rgba(&self) -> Vec<u8> {
        self.pointer_rgba.clone()
    }

    /// The game's pointer, for the page to tell whether it lies under the system's (index.html
    /// hides the system's there): `[x, y, shown, queued, qx, qy]`. (x, y), the frame pixel the
    /// engine placed its sprite at in the last step: the input handler's pointer
    /// ([`u6_core::Io::handler_pointer`], the VBlank server's, which no other call moves);
    /// `shown` 1 while the sprite is drawn there ([`Engine::sprite`], the pointer not hidden:
    /// 0x1622d6 parks a hidden one below the frame), else 0, in any program and with HD on or
    /// off. `queued` 1 while a pointer event waits for the next step (the page's
    /// [`Game::pointer`], a context-menu entry's [`Game::command_at`]), (qx, qy) the last one's
    /// pixel as queued; else 0, 0, 0. The queue is as of now: a context-menu entry's target
    /// press joins it only at the step that runs it ([`Game::step`]), after what was queued
    /// before, so (qx, qy) is where the sprite goes at the next step but for that press, which
    /// the next read after that step shows. All 0 before a start.
    pub fn game_pointer(&self) -> Vec<i32> {
        let Some(e) = &self.engine else {
            return vec![0; 6];
        };
        let at = e.io().handler_pointer();
        let shown = e.sprite().is_some() && e.state().ui.pointer.shown;
        let queued = self.pending.iter().rev().find_map(|ev| match *ev {
            InputEvent::Pointer { x, y, .. } => Some((x, y)),
            InputEvent::Key(_)
            | InputEvent::Repeat(_)
            | InputEvent::Release(_)
            | InputEvent::Text(_) => None,
        });
        let (q, qx, qy) = queued.map_or((0, 0, 0), |(x, y)| (1, i32::from(x), i32::from(y)));
        vec![
            i32::from(at.x),
            i32::from(at.y),
            i32::from(shown),
            q,
            qx,
            qy,
        ]
    }

    /// Address of the HD frame in wasm memory: `WIDTH·S × HEIGHT·S` RGBA8 pixels while HD is on
    /// (S = [`Game::hd_scale`]; [`Game::width`] × [`Game::height`] the frame). Views over it are
    /// invalidated when memory grows.
    pub fn hd_frame_ptr(&self) -> *const u8 {
        self.hd.rgba().as_ptr().cast()
    }

    /// Bytes of the HD frame (0 before its first composition).
    pub fn hd_frame_len(&self) -> usize {
        self.hd.rgba().len() * 4
    }

    /// The HD frame as RGBA8 (a copy).
    pub fn hd_frame_rgba(&self) -> Vec<u8> {
        self.hd
            .rgba()
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
    }

    /// Spec 07 H2: `pack`'s images go in front of MMPX in this game's HD frame, for this game and
    /// every one it starts later, in place of the pack set before. With HD on it composes at once
    /// (only the blocks whose HD image changed) and moves [`Game::frame_version`], with no step;
    /// off, nothing of the pack runs until HD is switched on.
    pub fn set_hd_pack(&mut self, pack: &HdPack) {
        self.attach_pack(Some(pack.pack.clone()));
    }

    /// Spec 07 H2: no HD pack any more: MMPX for every image, at once as [`Game::set_hd_pack`].
    pub fn forget_hd_pack(&mut self) {
        self.attach_pack(None);
    }

    /// Spec 08 (slice K1): installs `pack` on the game just started (the page calls it after
    /// [`Game::start`], before the first step), checked against its files: `""` once installed,
    /// else the fixed sentence that says why not (the part that differs, or no game). The pack
    /// stays with this game; a game started later takes one only from its own call.
    pub fn set_locale(&mut self, pack: &LocalePack) -> String {
        let Some(engine) = self.engine.as_mut() else {
            return "no game is running".into();
        };
        match engine.install_locale(pack.pack.clone(), hangul()) {
            Ok(()) => String::new(),
            Err(m) => m.to_string(),
        }
    }

    /// Spec 08: the running game's locale layer, counts only: `[on, runs put from the pack,
    /// runs it did not have, table strings from the pack, table strings it did not have, game
    /// strings from the pack, game strings it did not have, object names from the pack, object
    /// names it did not have, books from the pack, books it did not have, strings of `u` from the
    /// pack, strings of `u` it did not have]` (`on` 1 with a pack installed, else 0 and no counts).
    pub fn locale_counts(&self) -> Vec<u32> {
        self.engine.as_ref().map_or_else(
            || vec![0; 13],
            |e| {
                let l = &e.state().locale;
                let ([th, tm], [xh, xm]) = (l.tables.get(), l.texts.get());
                let ([lh, lm], [bh, bm]) = (l.look.get(), l.books.get());
                let [uh, um] = l.program.get();
                vec![
                    u32::from(l.pack.is_some()),
                    l.hits,
                    l.misses,
                    th,
                    tm,
                    xh,
                    xm,
                    lh,
                    lm,
                    bh,
                    bm,
                    uh,
                    um,
                ]
            },
        )
    }

    /// Whether an HD pack is set.
    pub fn has_hd_pack(&self) -> bool {
        self.hd_pack.is_some()
    }

    /// The images of the pack set that the running game would show now, by kind: tiles,
    /// portraits, panel bitmaps (those whose classic image is the one the pack's image was made
    /// from); empty without a pack or a game.
    pub fn hd_pack_matches(&self) -> Vec<u32> {
        match (&self.hd_pack, &self.engine) {
            (Some(pack), Some(engine)) => counts(pack.matches(engine)),
            _ => Vec::new(),
        }
    }

    /// The portraits the running game's archives hold (`u6_core::converse::portraits`, what
    /// the HD tool packs); 0 before a start. For tests and diagnostics.
    pub fn hd_portraits(&self) -> u32 {
        self.engine.as_ref().map_or(0, |e| {
            u6_core::converse::portraits(&e.state(), e.io().files()).len() as u32
        })
    }

    /// The HD images this game's compositor made so far, by MMPX and from the pack, and the
    /// bytes its cache holds now. For tests and diagnostics.
    pub fn hd_cache_counts(&self) -> Vec<f64> {
        let c = self.hd.compositor().cache();
        vec![c.made() as f64, c.packed() as f64, c.bytes() as f64]
    }

    /// The source of pixel (x, y) of the shown frame (spec 07 HD0's plane): kind (0 classic, 1
    /// tile, 2 portrait, 3 bitmap, 4 glyph), id, version, u, v, flags; empty before a start or
    /// outside the screen. For tests and diagnostics.
    pub fn hd_source_at(&self, x: u32, y: u32) -> Vec<u32> {
        let src = self
            .engine
            .as_ref()
            .and_then(|e| e.frame().src_at(x as usize, y as usize));
        src.map_or_else(Vec::new, |s| {
            vec![
                s.kind as u32,
                u32::from(s.id),
                s.ver,
                u32::from(s.u),
                u32::from(s.v),
                u32::from(s.flags),
            ]
        })
    }

    /// The content key image (`kind`, `id`) has in the running game now (the hash an HD pack
    /// keys its image by), 16 lower-case hex digits; empty when it has none. For tests and
    /// diagnostics.
    pub fn hd_image_key(&self, kind: u8, id: u16) -> String {
        let (Some(engine), Some(kind)) = (&self.engine, kind_of(kind)) else {
            return String::new();
        };
        u6_core::hd::image_key(engine, kind, id).map_or_else(String::new, |k| format!("{k:016x}"))
    }

    /// The rect of the HD frame (x, y, width, height in its pixels) the compositions since the
    /// last call wrote, empty when they wrote nothing: the page that drew the frame before them
    /// redraws only that.
    pub fn hd_take_dirty(&mut self) -> Vec<u32> {
        let s = self.hd.scale().factor() as u32;
        self.hd
            .take_dirty()
            .map_or_else(Vec::new, |r| r.iter().map(|&v| v as u32 * s).collect())
    }

    /// Adds `dt` wall milliseconds since the previous display frame and returns the steps they
    /// are worth at the speed, or at 1× while a pointer button is held
    /// ([`u6_core::pacer::Pacer::due`]); nothing is taken off until [`Game::ran`].
    pub fn pace(&mut self, dt: f64) -> u32 {
        let held = self.pointer_held();
        self.pacer.due(dt, held)
    }

    /// The page ran `k` of the steps [`Game::pace`] gave (0 when its budget allowed none).
    pub fn ran(&mut self, k: u32) {
        self.pacer.ran(k);
    }

    /// Sets the speed: `x` engine milliseconds per wall millisecond, 0 for Max (as many steps
    /// as the frame budget holds). A negative, infinite or NaN `x` is ignored.
    pub fn set_speed(&mut self, x: f64) {
        self.pacer.set_speed(if x == 0.0 {
            Speed::Max
        } else {
            Speed::Finite(x)
        });
    }

    /// The speed [`Game::set_speed`] set, 0 for Max (1 until then).
    pub fn speed(&self) -> f64 {
        match self.pacer.speed() {
            Speed::Finite(x) => x,
            Speed::Max => 0.0,
        }
    }

    /// Wall milliseconds the pace cut so far from frames longer than its cap (a hidden tab, a
    /// debugger pause): lost time, but not the game falling behind.
    pub fn clamped_ms(&self) -> f64 {
        self.pacer.clamped_ms()
    }

    /// Wall milliseconds of steps owed and not run so far (the budget did not hold them): the
    /// page shows "behind" while it grows.
    pub fn dropped_ms(&self) -> f64 {
        self.pacer.dropped_ms()
    }

    /// Which of `-`, `+` and `=` the game takes itself now (as of the last step), as bits: 1 `-`,
    /// 2 `+`, 4 `=`. The page leaves a key whose bit is set to the game instead of changing the
    /// speed: all three while a line is typed (`game`'s line editor, `u`'s name), `-` and `+` in
    /// the stats or inventory panel, `+` / `-` in the roster while it scrolls down / up, `-` and
    /// `=` in `u`'s menu (its songs). 0 before a start and after the end
    /// ([`u6_core::Engine::keys_owned`]).
    pub fn keys_owned(&self) -> u8 {
        self.engine.as_ref().map_or(0, Engine::keys_owned)
    }

    /// Queues a key press: the first of a key held down (a DOM `keydown` whose `repeat` is
    /// false). Returns true when it is a game key (the caller should then `preventDefault` so
    /// arrows do not scroll the page and Ctrl-S / Ctrl-R do not save or reload the page).
    pub fn key_down(&mut self, code: &str, key: &str, ctrl: bool) -> bool {
        self.key_event(InputEvent::Key, code, key, ctrl)
    }

    /// Spec 19 O1b: queues a press the browser's auto-repeat sent (a `keydown` whose `repeat`
    /// is true), as [`Game::key_down`] does a press; the key's release ([`Game::key_up`]) takes
    /// it back while it waits at a key wait that drains.
    pub fn key_repeat(&mut self, code: &str, key: &str, ctrl: bool) -> bool {
        self.key_event(InputEvent::Repeat, code, key, ctrl)
    }

    /// Spec 19 O1b: the physical key `code` (a `keyup`'s) let go: one release per engine key
    /// its presses and repeats were queued as ([`Game::key_down`], [`Game::key_repeat`]),
    /// whatever the modifiers are now (Ctrl let go before S still releases Ctrl-S). Outside the
    /// line editor a release takes the key's queued repeats back, so a held key walks no
    /// further than the prompt it was let go at. Whether `code` was held.
    pub fn key_up(&mut self, code: &str) -> bool {
        let (pending, running) = (&mut self.pending, self.engine.is_some());
        let before = self.held.len();
        self.held.retain(|(held, k)| {
            if held != code {
                return true;
            }
            if running {
                pending.push(InputEvent::Release(*k));
            }
            false
        });
        self.held.len() != before
    }

    /// Spec 19 O1b: every held key let go ([`Game::key_up`]), for when the page can no longer
    /// see their releases (the window lost the focus or the page was hidden).
    pub fn release_keys(&mut self) {
        let codes: Vec<String> = self.held.iter().map(|(code, _)| code.clone()).collect();
        for code in codes {
            self.key_up(&code);
        }
    }

    /// Spec 08 Design 4 (slice K5a): queues the text an input method committed (the page's
    /// hidden input, in 한글 with a Korean pack, while a line is typed): one
    /// [`InputEvent::Text`] per character, in order with the keys, at most [`TEXT_CHARS`] of one
    /// call. The engine's line editor takes a Hangul syllable (the player's words, with the pack)
    /// or printable ASCII (as its key) and drops the rest. Ignored before the game starts.
    /// Nothing of it is logged or kept.
    pub fn text_input(&mut self, text: &str) {
        if self.engine.is_some() {
            self.pending
                .extend(text.chars().take(TEXT_CHARS).map(InputEvent::Text));
        }
    }

    /// Queues the pointer at frame pixel (x, y) (the page maps the canvas onto the frame's
    /// [`Game::width`] × [`Game::height`] pixels) with `buttons` held: bit 0 left, bit 1 right,
    /// as DOM `buttons`. Ignored before the game starts.
    pub fn pointer(&mut self, x: i32, y: i32, buttons: u8) {
        if self.engine.is_some() {
            queue_pointer(&mut self.pending, self.handed, x, y, buttons);
        }
    }

    /// Spec 22 Contract 8: whether the game reads the player's words by click now (a
    /// conversation's read after "you say", op f7, from its start to its end; buttons drawn or
    /// not). The page releases a cancelled press off every target then. False before a start.
    pub fn reading_by_click(&self) -> bool {
        self.engine
            .as_ref()
            .is_some_and(|e| e.state().ui.talk.reading)
    }

    /// Voices on or off (spec 09 Design 1): while on, the engine's speech tap keeps what NPCs
    /// say for [`Game::take_speech`]; off (the default), it keeps nothing. Applies to the game
    /// running and to every game started after.
    pub fn set_voices(&mut self, on: bool) {
        self.voices = on;
        if let Some(engine) = &mut self.engine {
            engine.set_speech(on);
        }
    }

    /// Spec 18: the remembered sight on or off: while on, the map window shows each cell in view
    /// that was seen and is out of sight now as it last showed it, its ground and still objects
    /// in dark grey; off (the default), black, as the original. Applies to the game running (from
    /// its next map window render) and to every game started after.
    pub fn set_sight_memory(&mut self, on: bool) {
        self.sight_memory = on;
        if let Some(engine) = &self.engine {
            engine.io().set_sight_memory(on);
        }
    }

    /// Whether the remembered sight is on ([`Game::set_sight_memory`]).
    pub fn sight_memory(&self) -> bool {
        self.sight_memory
    }

    /// Spec 09 Design 1: the utterances NPCs said since the previous call, as a JSON array of
    /// `{seq, turn, speaker, script: [archive, item] | null, text, lang, traits: {type,
    /// worktype, alignment}}` (empty when there are none). A channel of its own: the text is
    /// the user's game text, for the page's speech only (never logged, shown or stored).
    pub fn take_speech(&mut self) -> String {
        let said = self
            .engine
            .as_mut()
            .map(Engine::take_speech)
            .unwrap_or_default();
        if said.is_empty() {
            String::new()
        } else {
            speech_json(&said)
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
    /// Spec 17: the engine's queue is empty and the page's holds nothing the game reads as an
    /// event: the moves of a button-less pointer change nothing it reads, and a key's release is
    /// applied where the engine queues the input, never one of its events
    /// ([`InputEvent::Release`]).
    fn quiet(&self) -> bool {
        let inert = |ev: &InputEvent| match ev {
            InputEvent::Pointer { buttons, .. } => buttons & 3 == 0,
            InputEvent::Release(_) => true,
            _ => false,
        };
        self.engine
            .as_ref()
            .is_some_and(|e| e.io().pending_events() == 0)
            && self.pending.iter().all(inert)
    }

    /// Spec 17: [`Game::at_prompt`] with the moves of a button-less pointer and the key releases
    /// queued since the last step let through ([`Game::quiet`]): a right press just after either
    /// still gets its menu.
    fn prompt_up(&self) -> bool {
        self.quiet()
            && self
                .engine
                .as_ref()
                .is_some_and(|e| e.state().main.reading == 1)
    }

    /// Spec 17: the held target press of [`Game::command_at`] before a tick: queued once its
    /// command reads the target with the queues quiet ([`Game::quiet`]), dropped once a new
    /// prompt read came, whatever is queued ([`u6_core::ui::menu::Run::step`]).
    fn step_run(&mut self) {
        let (Some(run), Some(e)) = (self.run, &self.engine) else {
            return;
        };
        let step = run.step(&e.state(), self.quiet());
        match step {
            u6_core::ui::menu::Step::Hold => {}
            u6_core::ui::menu::Step::Release(press) => {
                self.pending.extend(press);
                self.run = None;
            }
            u6_core::ui::menu::Step::Drop => self.run = None,
        }
    }

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
            Ok(mut engine) => {
                // Spec 07 HD0: the source plane is kept whether or not HD is shown, so switching
                // HD on loses nothing.
                engine.enable_src_plane();
                // A path that differs from another only in case is shadowed (the game's lookups
                // ignore case): the engine never reads it, so it is not kept either.
                let files = engine.io().files();
                paths.retain(|p| files.get_in("base", p).is_some_and(|(key, _)| key == p));
                self.kept = paths;
                self.put_on_screen(engine);
                Ok(report)
            }
            Err(e) => {
                report.push_str(&format!("\n{e}"));
                Err(report)
            }
        }
    }

    /// Queues `event` of the engine key the DOM key maps to ([`key_from_dom`]) while a game
    /// runs, noting it as held by `code`; whether it is a game key.
    fn key_event(
        &mut self,
        event: fn(Key) -> InputEvent,
        code: &str,
        key: &str,
        ctrl: bool,
    ) -> bool {
        match key_from_dom(code, key, ctrl) {
            Some(k) => {
                if self.engine.is_some() {
                    self.pending.push(event(k));
                    if !self.held.iter().any(|(c, h)| c == code && *h == k) {
                        self.held.push((code.to_string(), k));
                    }
                }
                true
            }
            None => false,
        }
    }

    /// A pointer button is down as the input handler has it: the last pointer event queued
    /// since the previous step (newer than the engine's), else the engine's handler pointer
    /// ([`u6_core::Io::handler_pointer`]). False before a start.
    fn pointer_held(&self) -> bool {
        let buttons = last_buttons(&self.pending).or_else(|| {
            self.engine
                .as_ref()
                .map(|e| e.io().handler_pointer().buttons)
        });
        buttons.is_some_and(|b| b & 3 != 0)
    }

    /// `engine`, just started, becomes the game on screen: no Save of it taken yet, no sound
    /// machine, no queued input, the voices and the remembered sight as set; its first frame
    /// converted and, with HD on, composed whole (its versions start again at 0).
    fn put_on_screen(&mut self, mut engine: Engine) {
        engine.set_speech(self.voices);
        engine.io().set_sight_memory(self.sight_memory);
        self.saved_gen = 0;
        self.save_out = Vec::new();
        self.save_whole = false;
        self.save_out_whole = false;
        self.audio = None;
        self.engine = Some(engine);
        self.pending.clear();
        self.handed = 0;
        self.redraw();
        self.hd.restart();
        self.compose_hd();
        self.track_pointer();
    }

    /// Sets the HD pack (`None`: none): with HD on, composed at once and the page's version
    /// moved, as [`Game::set_hd`] does.
    fn attach_pack(&mut self, pack: Option<Rc<u6_core::hd::HdPack>>) {
        self.hd_pack = pack.clone();
        self.hd.set_pack(pack);
        if self.hd.on() {
            self.conversions += 1;
            self.compose_hd();
        }
    }

    /// Spec 11 Design 7: takes the pointer the page is to draw over the HD frame now
    /// ([`Game::hd_pointer`]): with HD on and a game, [`pointer_pixels`] of its engine, else none;
    /// the version moves when the box or the pixels changed.
    fn track_pointer(&mut self) {
        let (rect, rgba) = match &self.engine {
            Some(e) if self.hd.on() => pointer_pixels(e),
            _ => ([0; 4], Vec::new()),
        };
        if rect != self.pointer_box || rgba != self.pointer_rgba {
            self.pointer_box = rect;
            self.pointer_rgba = rgba;
            self.pointer_version += 1;
        }
    }

    /// With HD on: composes the HD frame from the engine's published frame (black before a start)
    /// when it does not hold that frame's colours and sources at the HD setting now
    /// ([`HdView::present`]).
    fn compose_hd(&mut self) {
        let wrote = self.hd.present(self.engine.as_ref(), &self.blank);
        self.conversions += u64::from(wrote);
    }

    /// Converts the engine's frame (black before a start) into the RGBA frame, whatever it holds
    /// now (a new engine's versions start again at 0, so a start converts here, never through
    /// the version check of [`Game::present`]).
    ///
    /// With HD on the page shows the HD frame, so the conversion alone does not move
    /// [`Game::frame_version`].
    fn redraw(&mut self) {
        let fb = self.engine.as_ref().map_or(&self.blank, |e| e.frame());
        fb.write_rgba(&mut self.rgba);
        self.rgba_holds = self.engine.as_ref().map(Engine::frame_version);
        self.conversions += u64::from(!self.hd.on());
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

/// Spec 11 Design 7: the pointer sprite `e` shows now, for the page to draw over the HD frame:
/// its box (x, y, width, height in frame pixels) clipped to the frame, and its pixels in that box
/// as RGBA8 (value v ≠ 0 in the shown frame's colour 16 + v, [`Engine::sprite_colour`], opaque;
/// value 0 transparent). An empty box and no pixels when there is no sprite, the pointer is
/// hidden (0x1622d6 parks it below the frame; a shape reaching up past its hot spot would still
/// put rows of it into the frame's last lines there, which the RGBA frame shows) or the box lies
/// outside the frame.
fn pointer_pixels(e: &Engine) -> ([u32; 4], Vec<u8>) {
    let none = ([0; 4], Vec::new());
    let Some(sp) = e.sprite() else {
        return none;
    };
    if !e.state().ui.pointer.shown {
        return none;
    }
    // A sprite row is 16 pixels wide (`SpriteFrame::rows`).
    let (x0, y0) = (i32::from(sp.x), i32::from(sp.y));
    let (x1, y1) = (x0 + 16, y0 + sp.rows.len() as i32);
    let (w, h) = (u6_render::WIDTH as i32, u6_render::HEIGHT as i32);
    let (cx0, cy0, cx1, cy1) = (x0.max(0), y0.max(0), x1.min(w), y1.min(h));
    if cx0 >= cx1 || cy0 >= cy1 {
        return none;
    }
    let mut rgba = Vec::with_capacity(((cx1 - cx0) * (cy1 - cy0) * 4) as usize);
    for y in cy0..cy1 {
        let row = &sp.rows[(y - y0) as usize];
        for x in cx0..cx1 {
            let v = row[(x - x0) as usize];
            let c = e.sprite_colour(v).map_or([0; 4], |c| [c.r, c.g, c.b, 255]);
            rgba.extend_from_slice(&c);
        }
    }
    let rect = [cx0, cy0, cx1 - cx0, cy1 - cy0].map(|v| v as u32);
    (rect, rgba)
}

/// The buttons of the last pointer event of `events`.
fn last_buttons(events: &[InputEvent]) -> Option<u8> {
    events.iter().rev().find_map(|e| match *e {
        InputEvent::Pointer { buttons, .. } => Some(buttons),
        InputEvent::Key(_)
        | InputEvent::Repeat(_)
        | InputEvent::Release(_)
        | InputEvent::Text(_) => None,
    })
}

/// Queues a pointer event. Spec 22 Contract 5: a move replaces the move queued last when both
/// keep the same buttons (the game samples the pointer once per frame and takes a run of moves
/// at once anyway): the event queued last is a pointer event with the new one's buttons, and the
/// pointer had them before it too (the pointer event queued before it, else `handed`, the last
/// one the engine was handed). A press or a release is never replaced, so it stays where its own
/// event put it, and the moves after it follow it.
fn queue_pointer(pending: &mut Vec<InputEvent>, handed: u8, x: i32, y: i32, buttons: u8) {
    let word = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
    let buttons = buttons & 3;
    let ev = InputEvent::Pointer {
        x: word(x),
        y: word(y),
        buttons,
    };
    if let Some((InputEvent::Pointer { buttons: last, .. }, before)) = pending.split_last() {
        let before = last_buttons(before).unwrap_or(handed);
        if *last == buttons && before == buttons {
            pending.pop();
        }
    }
    pending.push(ev);
}

/// [`Game::take_speech`]'s JSON for `said`.
fn speech_json(said: &[u6_core::speech::Utterance]) -> String {
    let mut out = String::from("[");
    for (i, u) in said.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let script = u.script.map_or_else(
            || "null".to_string(),
            |(letter, item)| format!("[\"{}\",{item}]", char::from(letter)),
        );
        let t = u.traits;
        out.push_str(&format!(
            "{{\"seq\":{},\"turn\":{},\"speaker\":{},\"script\":{script},\"text\":",
            u.seq, u.turn, u.speaker
        ));
        json_string(&mut out, &u.text);
        out.push_str(&format!(
            ",\"lang\":\"{}\",\"traits\":{{\"type\":{},\"worktype\":{},\"alignment\":{}}}}}",
            u.lang.code(),
            t.obj_type,
            t.worktype,
            t.alignment
        ));
    }
    out.push(']');
    out
}

/// [`Game::context_menu`]'s JSON for `m`.
fn menu_json(m: &u6_core::ui::menu::Menu) -> String {
    use u6_core::ui::menu::Kind;
    let kind = match m.kind {
        Kind::Map => "map",
        Kind::Panel => "panel",
    };
    let cell = m
        .cell
        .map_or_else(|| "null".to_string(), |(x, y)| format!("[{x},{y}]"));
    let underlined = m
        .underlined
        .map_or_else(|| "null".to_string(), |c| c.to_string());
    let mut out =
        format!("{{\"kind\":\"{kind}\",\"cell\":{cell},\"underlined\":{underlined},\"sections\":[");
    for (i, section) in m.sections.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let kind = section
            .kind
            .map_or_else(|| "null".to_string(), |(t, f)| format!("[{t},{f}]"));
        out.push_str(&format!(
            "{{\"object\":{},\"kind\":{kind},\"name\":",
            section.object
        ));
        json_string(&mut out, &section.name);
        out.push_str(",\"entries\":[");
        for (j, e) in section.entries.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            let reason = e
                .reason
                .map_or_else(|| "null".to_string(), |r| format!("\"{}\"", r.code()));
            out.push_str(&format!(
                "{{\"code\":{},\"enabled\":{},\"reason\":{reason},\"theft\":{}}}",
                e.code,
                e.enabled(),
                e.theft
            ));
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    out
}

/// `text` as a JSON string literal: `"` and `\` escaped, control characters as `\u00XX`.
fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use u6_core::geom::{HALF_H, HALF_W};
    use u6_testkit::{Quiet, assert_same};

    /// Spec 22 Contract 5: a run of moves keeping the buttons collapses to its last; a press or
    /// a release is never replaced, so the move after it, the held move after a press included,
    /// is queued behind it (until round 3 of spec 22 the press moved with that held move to
    /// (21, 31), and a release to the hover after it).
    #[test]
    fn pointer_moves_collapse_and_button_changes_queue() {
        let mut q = Vec::new();
        let p = |x, y, buttons| InputEvent::Pointer { x, y, buttons };
        queue_pointer(&mut q, 0, 10, 10, 0);
        queue_pointer(&mut q, 0, 20, 30, 0);
        queue_pointer(&mut q, 0, 20, 30, 1);
        queue_pointer(&mut q, 0, 21, 31, 5);
        queue_pointer(&mut q, 0, 21, 31, 0);
        assert_eq!(q, [p(20, 30, 0), p(20, 30, 1), p(21, 31, 1), p(21, 31, 0)]);
        q.push(InputEvent::Key(Key::Space));
        queue_pointer(&mut q, 0, 99_999, -99_999, 2);
        assert_eq!(
            q.last(),
            Some(&p(i16::MAX, i16::MIN, 2)),
            "a key in between keeps both"
        );
        let mut game = Game::new();
        game.pointer(5, 5, 1);
        assert!(game.pending.is_empty(), "no game, nothing queued");
    }

    /// Spec 22 Contract 5 (gpt c-2): a release and the hover after it in one frame (a click
    /// whose release would otherwise be taken where the hover went), a press and the held move
    /// after it, and a run of moves after either.
    #[test]
    fn a_press_or_release_stays_where_its_event_put_it() {
        let p = |x, y, buttons| InputEvent::Pointer { x, y, buttons };
        let queued = |handed: u8, events: &[(i32, i32, u8)]| {
            let mut q = Vec::new();
            for &(x, y, b) in events {
                queue_pointer(&mut q, handed, x, y, b);
            }
            q
        };
        assert_eq!(
            queued(0, &[(40, 50, 1), (40, 50, 0), (60, 70, 0), (61, 71, 0)]),
            [p(40, 50, 1), p(40, 50, 0), p(61, 71, 0)],
            "release then hover: the release kept, the hover's moves one"
        );
        assert_eq!(
            queued(0, &[(40, 50, 1), (45, 55, 1), (46, 56, 1)]),
            [p(40, 50, 1), p(46, 56, 1)],
            "press then held moves: the press kept, the held moves one"
        );
        assert_eq!(
            queued(0, &[(1, 1, 0), (2, 2, 0), (3, 3, 0)]),
            [p(3, 3, 0)],
            "a run of moves collapses"
        );
        assert_eq!(
            queued(1, &[(1, 1, 1), (2, 2, 1)]),
            [p(2, 2, 1)],
            "held moves after a press the engine has collapse"
        );
        assert_eq!(
            queued(1, &[(-1, -1, 0), (40, 50, 0)]),
            [p(-1, -1, 0), p(40, 50, 0)],
            "a release of a press the engine has: kept before the hover"
        );
    }

    /// Spec 22 Contract 8: the press handed to the engine at one step, its release (the page's
    /// cancelled press, at (-1, -1)) and a hover over the press's place queued before the next:
    /// both reach the engine, the release where it was queued.
    #[test]
    fn a_release_after_a_handed_press_is_not_moved_by_the_hover_after_it() {
        let mut game = Game::new();
        game.engine = Some(Engine::with_game(
            u6_core::State::empty(),
            Files::default(),
            |_, io| async move {
                io.sleep(u32::MAX).await;
            },
        ));
        game.pointer(40, 50, 1);
        game.step(16);
        game.pointer(-1, -1, 0);
        game.pointer(40, 50, 0);
        let p = |x, y, buttons| InputEvent::Pointer { x, y, buttons };
        assert_eq!(game.pending, [p(-1, -1, 0), p(40, 50, 0)]);
        game.step(16);
        game.pointer(41, 51, 0);
        game.pointer(42, 52, 0);
        assert_eq!(game.pending, [p(42, 52, 0)], "moves after it collapse");
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

    /// Bytes of the whole frame at `s`× (1: the classic frame), RGBA8.
    fn frame_bytes(s: usize) -> usize {
        u6_render::WIDTH * s * u6_render::HEIGHT * s * 4
    }

    /// The whole frame at `s`× as [`Game::hd_take_dirty`] gives it.
    fn frame_rect(s: usize) -> Vec<u32> {
        vec![
            0,
            0,
            (u6_render::WIDTH * s) as u32,
            (u6_render::HEIGHT * s) as u32,
        ]
    }

    /// Spec 13: the shown map as the page gets it ([`shown_map`]). Original: the game's map
    /// window. Wide: the view less half a cell, beside the UI's 320 columns and half a cell
    /// inside the frame's right edge and the view's last row (the frame goes on under the UI,
    /// spec 14), 36 cells across and 20 down (fails with `VIEW_W` and `VIEW_H` swapped).
    #[test]
    fn the_shown_map_is_the_view_less_half_a_cell_in_the_frame() {
        let [x, y, w, h] = shown_map()[..] else {
            panic!("four words")
        };
        let (fw, fh) = (u6_render::WIDTH as u32, u6_render::HEIGHT as u32);
        if fw > 320 {
            // The view's rows end above the frame's last (spec 14: the actor views' slots lie
            // under the UI, beside the map's rows).
            let rows = u32::from(u6_core::geom::MAP_Y) + u6_core::geom::VIEW_H as u32 * 16;
            assert_eq!((x, y), (320 + 8, 8), "beside the UI");
            assert_eq!(
                (x + w + 8, y + h + 8),
                (fw, rows),
                "to the frame's right edge and the view's last row"
            );
            assert!(rows <= fh, "the view's rows in the frame");
            assert_eq!((w / 16, h / 16), (36, 20), "cells across and down");
        } else {
            assert_eq!([x, y, w, h], [8, 8, 160, 160], "the map window");
        }
    }

    #[test]
    fn before_start_the_frame_is_black_and_keys_are_not_queued() {
        let mut game = Game::new();
        assert_eq!(game.frame_len(), frame_bytes(1));
        assert!(game.frame_rgba().chunks(4).all(|p| p == [0, 0, 0, 255]));
        assert!(game.key_down("ArrowUp", "ArrowUp", false));
        assert!(!game.key_down("ShiftLeft", "Shift", false));
        game.text_input("가");
        assert!(game.pending.is_empty());
        game.step(16);
        game.present();
        assert!(!game.started());
    }

    /// Spec 19 O1b: a release goes by the physical key. Ctrl-S held into its repeats, Ctrl let
    /// go first (S's next repeat is a plain `s`), then S: both of S's keys are released, and in
    /// `game` with its key waits draining the queue is left with the presses only. A key whose
    /// release the page cannot see is let go by `release_keys`.
    #[test]
    fn a_release_takes_back_the_repeats_of_its_physical_key_whatever_the_modifiers() {
        let (ctrl_s, s) = (Key::Char('\u{13}'), Key::Char('s'));
        let mut state = u6_core::State::empty();
        state.main.program = u6_core::main_loop::Program::Game;
        state.ui.input.drain = true;
        let mut game = Game::new();
        game.engine = Some(Engine::with_game(
            state,
            Files::default(),
            |_, io| async move {
                io.sleep(u32::MAX).await;
            },
        ));
        assert!(game.key_down("KeyS", "s", true));
        assert!(game.key_repeat("KeyS", "s", true));
        assert!(game.key_repeat("KeyS", "s", true));
        assert!(game.key_repeat("KeyS", "s", false));
        assert!(game.key_down("ArrowUp", "ArrowUp", false));
        assert!(game.key_up("KeyS"));
        assert_eq!(
            game.pending[5..],
            [InputEvent::Release(ctrl_s), InputEvent::Release(s)],
            "one release per key S gave"
        );
        assert!(!game.key_up("KeyS"), "released once");
        game.step(20);
        let engine = game.engine.as_ref().quiet();
        assert_eq!(
            engine.io().pending_events(),
            2,
            "the presses of Ctrl-S and the arrow are left"
        );
        assert!(game.key_repeat("ArrowUp", "ArrowUp", false));
        game.release_keys();
        assert_eq!(
            game.pending,
            [InputEvent::Repeat(Key::Up), InputEvent::Release(Key::Up)]
        );
        assert!(game.held.is_empty());
        game.step(20);
        let engine = game.engine.as_ref().quiet();
        assert_eq!(
            engine.io().pending_events(),
            2,
            "the arrow's repeat taken back"
        );
    }

    /// Spec 08 Design 4 (slice K5a): committed text reaches the engine one character per event,
    /// in order with the keys, at most a line of it per call.
    #[test]
    fn text_input_queues_each_character_in_order_with_the_keys() {
        use std::cell::RefCell;
        use u6_core::engine::Typed;
        let seen = Rc::new(RefCell::new(Vec::new()));
        let out = Rc::clone(&seen);
        let mut game = Game::new();
        game.engine = Some(Engine::with_game(
            u6_core::State::empty(),
            Files::default(),
            move |_state, io| async move {
                loop {
                    let t = io.next_typed().await;
                    out.borrow_mut().push(t);
                }
            },
        ));
        game.key_down("KeyA", "a", false);
        game.text_input("가나");
        game.key_down("Enter", "Enter", false);
        assert_eq!(game.keys_pending(), 4);
        game.step(20);
        assert_eq!(
            *seen.borrow(),
            [
                Typed::Key(Key::Char('a')),
                Typed::Text('가'),
                Typed::Text('나'),
                Typed::Key(Key::Enter)
            ]
        );
        game.text_input(&"가".repeat(100));
        assert_eq!(game.keys_pending(), TEXT_CHARS as u32);
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

    /// A game on synthetic state and no files: palette entry v = (v, 3v, 255 - v), then each step
    /// a pixel per generation on row 0, each key taken drawn on row 1 (its character as the
    /// index) and logged as a stub hit, the pointer's cell lit, presented before the frame's
    /// wait.
    fn synthetic_engine() -> Engine {
        Engine::with_game(
            u6_core::State::empty(),
            Files::default(),
            |_state, io| async move {
                io.draw(|fb| {
                    for (i, c) in fb.palette.iter_mut().enumerate() {
                        let v = i as u8;
                        *c = u6_core::Rgb::new(v, v.wrapping_mul(3), 255 - v);
                    }
                });
                let mut typed = 0;
                loop {
                    let p = io.take_pointer();
                    while io.key_ready() {
                        let key = io.next_key().await;
                        let code = match key {
                            Key::Char(c) => c as u8,
                            _ => 0xff,
                        };
                        io.draw(|fb| fb.set(typed % 320, 1, code));
                        u6_core::stub::hit("synthetic.key");
                        typed += 1;
                    }
                    let g = io.generation();
                    io.draw(|fb| {
                        fb.set(g as usize % 320, 0, (g % 251) as u8 + 1);
                        fb.set(p.x as usize, p.y as usize, 0x80 | p.buttons);
                    });
                    io.present();
                    io.vblank().await;
                }
            },
        )
    }

    /// Spec 09: the speech JSON lists every field; the text's quote, backslash, newline and
    /// another control character are escaped, a code point past ASCII is kept; no script is
    /// `null`. Voices set before a start apply to the engine put on screen, and off again to
    /// the running one; without utterances the channel is empty.
    #[test]
    fn take_speech_is_json_and_voices_follow_the_game() {
        use u6_core::speech::{Lang, Traits, Utterance};
        let u = |seq, script, text: &str| Utterance {
            seq,
            turn: 2,
            speaker: 5,
            script,
            text: text.into(),
            lang: Lang::En,
            traits: Traits {
                obj_type: 0x187,
                worktype: 7,
                alignment: 1,
            },
        };
        let json = speech_json(&[
            u(0, Some((b'a', 3)), "A \"b\"\\c\nd\u{1}é"),
            u(1, None, "x"),
        ]);
        assert_eq!(
            json,
            "[{\"seq\":0,\"turn\":2,\"speaker\":5,\"script\":[\"a\",3],\"text\":\"A \\\"b\\\"\\\\c\\u000ad\\u0001é\",\
             \"lang\":\"en\",\"traits\":{\"type\":391,\"worktype\":7,\"alignment\":1}},\
             {\"seq\":1,\"turn\":2,\"speaker\":5,\"script\":null,\"text\":\"x\",\"lang\":\"en\",\
             \"traits\":{\"type\":391,\"worktype\":7,\"alignment\":1}}]"
        );
        let mut game = Game::new();
        assert_eq!(game.take_speech(), "", "no engine: nothing");
        game.set_voices(true);
        game.put_on_screen(synthetic_engine());
        assert!(
            game.engine
                .as_ref()
                .is_some_and(|e| e.state().speech.enabled)
        );
        game.set_voices(false);
        assert!(
            game.engine
                .as_ref()
                .is_some_and(|e| !e.state().speech.enabled)
        );
        game.step(20);
        assert_eq!(game.take_speech(), "", "no utterance: nothing");
    }

    /// Spec 18: the remembered sight is off until set; set before a start it applies to the
    /// engine put on screen, and off again to the running one.
    #[test]
    fn the_sight_memory_follows_the_game() {
        let mut game = Game::new();
        assert!(!game.sight_memory(), "off by default");
        game.set_sight_memory(true);
        game.put_on_screen(synthetic_engine());
        assert!(game.engine.as_ref().is_some_and(|e| e.io().sight_memory()));
        game.set_sight_memory(false);
        assert!(game.engine.as_ref().is_some_and(|e| !e.io().sight_memory()));
        assert!(!game.sight_memory());
    }

    /// Spec 08 (slice K1): a pack reads through the engine's strict reader (a refusal names the
    /// rule only, never what the file holds); set on a game whose files it was made from it is
    /// installed (`wide` on, the built-in Hangul table set, the counts start at 0), on one with
    /// other files it is refused with the part's sentence and nothing changes; with no game it is
    /// refused too.
    #[test]
    fn a_locale_pack_loads_strictly_and_installs_only_on_its_own_files() {
        use u6_core::converse::disasm::catalog::{ScriptUnits, hash64};
        use u6_core::locale::{Pack, RunText, fingerprint};
        const SENTINEL: &str = "Zq9LocaleSentinel";
        let script = [
            &b"\xff\x05Nm\xf1look\xf2Hi "[..],
            SENTINEL.as_bytes(),
            b".\xb6",
        ]
        .concat();
        let lib = |script: &[u8]| {
            [
                &0u32.to_le_bytes()[..],
                &8u32.to_le_bytes(),
                &[0, 0, 0, 0],
                script,
            ]
            .concat()
        };
        let units = ScriptUnits::build(b'a', 1, &script);
        let runs = units.runs.iter().map(|r| {
            (
                (b'a', 1, r.unit.offset),
                RunText {
                    len: r.unit.len,
                    hash: r.unit.hash,
                    text: "가나".as_bytes().to_vec(),
                },
            )
        });
        let pack = Pack::new(
            u6_core::ui::Language::Korean,
            "test/1",
            fingerprint([(b'a', 1, hash64(&script))]),
            runs,
        )
        .quiet();
        let bytes = pack.to_bytes();
        let loaded = load_locale(&bytes).quiet();
        assert_eq!(
            (loaded.counts(), loaded.language()),
            (vec![2, 1], "ko".into())
        );
        // Refusals: fixed sentences, nothing of the file.
        let mut broken = bytes.clone();
        broken.truncate(bytes.len() - 2);
        let e = load_locale(&broken).err().quiet();
        assert_eq!(e, "its contents are not those of a Korean pack");
        let marked = [&b"U6LP\n{\""[..], SENTINEL.as_bytes(), b"\":1}"].concat();
        assert!(!load_locale(&marked).err().quiet().contains(SENTINEL));
        assert!(load_locale(b"PK\x03\x04").is_err());
        // No game: refused.
        let mut game = Game::new();
        assert_eq!(game.set_locale(&loaded), "no game is running");
        assert_eq!(game.locale_counts(), [0; 13]);
        assert_eq!(loaded.string_counts(), [0, 0]);
        assert_eq!(loaded.text_counts(), [0, 0]);
        assert_eq!(loaded.program_strings(), 0);
        // Its own files: installed.
        let engine = |archive: Vec<u8>| {
            let files = Files::new(BTreeMap::from([("U6/converse.a".to_string(), archive)]));
            Engine::with_game(u6_core::State::empty(), files, |_s, io| async move {
                loop {
                    io.vblank().await;
                }
            })
        };
        game.put_on_screen(engine(lib(&script)));
        assert_eq!(game.set_locale(&loaded), "");
        assert_eq!(
            game.locale_counts(),
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        let e = game.engine.as_ref().quiet();
        assert!(e.state().ui.text.wide, "the assembler on");
        assert!(
            e.state().ui.hangul.is_some(),
            "the built-in Hangul table reads"
        );
        // Other files: refused, nothing changed.
        let mut other = script.clone();
        other[10] = b'o';
        game.put_on_screen(engine(lib(&other)));
        assert_eq!(
            game.set_locale(&loaded),
            "its conversation part was made from other game files"
        );
        assert_eq!(game.locale_counts(), [0; 13]);
        let e = game.engine.as_ref().quiet();
        assert!(!e.state().ui.text.wide && e.state().ui.hangul.is_none());
    }

    /// The keys queued reach the game in the step after them (the queues empty, their characters
    /// drawn in the synthetic palette's colours) and its log line comes through; a step alone
    /// leaves the RGBA frame as it was, the present shows it. Before a start the frame stays
    /// black.
    #[test]
    fn a_step_takes_the_keys_and_a_present_shows_it() {
        let mut game = Game::new();
        game.engine = Some(synthetic_engine());
        let first = game.frame_rgba();
        let rgba = |v: u8| [v, v.wrapping_mul(3), 255 - v, 255];
        let pixel = |g: &Game, x: usize, y: usize| {
            let at = (y * u6_render::WIDTH + x) * 4;
            g.frame_rgba()[at..at + 4].to_vec()
        };
        for i in 0..60u32 {
            if i % 7 == 3 {
                game.key_down("KeyT", "t", false);
                game.key_down("Minus", "-", false);
            }
            if i % 5 == 1 {
                game.pointer(i as i32 * 3, 50 + i as i32, (i % 3) as u8);
            }
            game.step([20, 16, 7, 33][i as usize % 4]);
            game.present();
            let log = game.take_log();
            if i == 3 {
                assert_eq!(log, "stub: synthetic.key", "the first key's line");
                assert!(
                    game.pending.is_empty() && game.keys_pending() == 0,
                    "both keys taken"
                );
                assert_eq!(pixel(&game, 0, 1), rgba(b't'), "t drawn");
                assert_eq!(pixel(&game, 1, 1), rgba(b'-'), "- drawn after it");
            } else {
                assert_eq!(log, "", "step {i}: one line per stub id");
            }
        }
        assert_eq!(pixel(&game, 17, 1), rgba(b'-'), "the ninth pair's -");
        assert!(game.frame_rgba() != first, "the frames changed");
        let shown = game.frame_rgba();
        game.step(20);
        assert!(game.frame_rgba() == shown, "a step alone converts nothing");
        game.present();
        assert!(game.frame_rgba() != shown, "the present shows the step");
        let mut idle = Game::new();
        idle.step(20);
        idle.present();
        assert!(idle.frame_rgba() == first, "before a start: black");
    }

    /// A game on synthetic state that presents only when it takes a key (the key's character
    /// drawn on row 1), with a one-shape pointer sprite shown: a step with no key and no pointer
    /// move composes no new frame.
    fn still_engine() -> Engine {
        use u6_core::ui::pointer::{PointerState, PointerTables, Shape};
        let mut s = u6_core::State::empty();
        s.ui.pointer = PointerState::from_tables(PointerTables {
            shapes: vec![Shape {
                dx: -1,
                dy: -2,
                rows: vec![[1; 16]],
            }],
            shown: true,
            ..Default::default()
        });
        Engine::with_game(s, Files::default(), |_state, io| async move {
            io.draw(|fb| {
                for (i, c) in fb.palette.iter_mut().enumerate() {
                    let v = i as u8;
                    *c = u6_core::Rgb::new(v, 255 - v, v.wrapping_mul(5));
                }
            });
            let mut typed = 0;
            loop {
                io.take_pointer();
                while io.key_ready() {
                    if let Key::Char(c) = io.next_key().await {
                        io.draw(|fb| fb.set(typed % 320, 1, c as u8));
                    }
                    typed += 1;
                    io.present();
                }
                io.vblank().await;
            }
        })
    }

    /// Spec 06 O1: a present converts only a frame the steps changed (a key presented, the
    /// pointer sprite moved, the first step), once however many steps ran, and the RGBA frame
    /// after every present is the one the old path (a conversion at every present) gave. The
    /// version the page reads moves with each conversion; a start that fails converts nothing.
    #[test]
    fn a_present_converts_only_a_changed_frame() {
        let mut game = Game::new();
        game.engine = Some(still_engine());
        let mut changes = 0;
        for i in 0..60u32 {
            let key = i % 5 == 2;
            let moved = i % 7 == 3;
            if key {
                game.key_down(
                    "KeyA",
                    &char::from(b'a' + (i % 26) as u8).to_string(),
                    false,
                );
            }
            if moved {
                game.pointer(10 + i as i32, 40, 0);
            }
            if i % 11 == 4 && !moved {
                // The pointer where it already is: the sprite stays.
                let at = game
                    .engine
                    .as_ref()
                    .map(|e| e.io().handler_pointer())
                    .unwrap();
                game.pointer(i32::from(at.x), i32::from(at.y), 0);
            }
            let steps = 1 + i % 3;
            for _ in 0..steps {
                game.step(20);
            }
            let before = game.conversions;
            game.present();
            let expect = u64::from(i == 0 || key || moved);
            assert_eq!(game.conversions - before, expect, "frame {i}: conversions");
            changes += expect;
            let mut old = Vec::new();
            game.engine.as_ref().unwrap().frame().write_rgba(&mut old);
            assert!(game.rgba == old, "frame {i}: the old path's RGBA");
            assert_eq!(game.frame_version(), game.conversions as f64);
        }
        // The first, 12 with a key, 9 with a move, 2 of them with both.
        assert_eq!(changes, 20, "frames that changed");
        let before = game.frame_version();
        game.collect("notes.txt", b"hello".to_vec()).unwrap();
        assert!(game.boot(0, false).is_err());
        game.present();
        assert_eq!(
            game.frame_version(),
            before,
            "a failed start keeps the frame"
        );
    }

    /// The speed: 1 at first; a preset, 0 for Max (the step cap per frame) and back; a negative,
    /// infinite or NaN speed ignored. A frame past the cap is clamped, steps not run are
    /// dropped: two counts.
    #[test]
    fn set_speed_round_trips_and_refuses_what_is_not_a_speed() {
        let mut game = Game::new();
        assert_eq!(game.speed(), 1.0);
        game.set_speed(2.0);
        assert_eq!((game.speed(), game.pace(100.0)), (2.0, 10));
        game.ran(10);
        game.set_speed(0.0);
        assert_eq!(game.speed(), 0.0, "Max");
        assert_eq!(game.pace(16.0), pacer::MAX_STEPS);
        game.ran(5);
        for bad in [-1.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            game.set_speed(bad);
            assert_eq!(game.speed(), 0.0, "{bad} ignored");
        }
        game.set_speed(1.5);
        assert_eq!((game.speed(), game.pace(40.0)), (1.5, 3));
        game.ran(3);
        assert_eq!((game.clamped_ms(), game.dropped_ms()), (0.0, 0.0));
        assert_eq!(game.pace(1_000.0), 18, "250 ms of wall time at 1.5×");
        assert_eq!(
            (game.clamped_ms(), game.dropped_ms()),
            (750.0, 0.0),
            "the stall"
        );
        game.ran(18);
        for _ in 0..30 {
            let n = game.pace(100.0);
            game.ran(n.min(1));
        }
        assert!(game.clamped_ms() == 750.0, "no frame past the cap");
        assert!(
            game.dropped_ms() > 0.0,
            "a budget of one step a frame falls behind"
        );
        assert_eq!((step_ms(), budget_ms(1000.0 / 60.0)), (20, 1000.0 / 120.0));
    }

    /// The pace runs at 1× while a pointer button is down, as the page's queued pointer events
    /// and then the engine's handler have it, at 8× and at Max; released, the speed again.
    #[test]
    fn a_held_button_paces_at_1x() {
        for speed in [8.0, 0.0] {
            let mut game = Game::new();
            game.engine = Some(synthetic_engine());
            game.set_speed(speed);
            let fast = game.pace(100.0);
            game.ran(fast);
            assert!(fast >= 40, "{speed}: {fast} steps free");
            game.pointer(10, 10, 1);
            assert_eq!(game.pace(100.0), 5, "{speed}: the press queued");
            game.ran(5);
            for _ in 0..5 {
                game.step(20);
            }
            assert_eq!(game.pace(100.0), 5, "{speed}: held in the engine");
            game.ran(5);
            game.pointer(12, 10, 1);
            game.pointer(12, 10, 0);
            assert_eq!(game.pace(100.0), fast, "{speed}: the release queued");
            game.ran(fast);
            game.step(20);
            assert_eq!(game.pace(100.0), fast, "{speed}: released");
        }
    }

    /// The keys the game takes: none before a start, at a `game` prompt and after the end;
    /// `game`'s state (a line typed, the panels, the roster per direction) and what `u` claims on
    /// the `Io`, together (synthetic state).
    #[test]
    fn keys_owned_follows_the_running_state() {
        use u6_core::engine::{OWN_ALL, OWN_EQUALS, OWN_MINUS, OWN_PLUS};
        use u6_core::main_loop::Program;
        let owned = |edit: &dyn Fn(&mut u6_core::State), claim: u8| {
            let mut s = u6_core::State::empty();
            s.main.program = Program::Game;
            s.ui.panel.mode = 0x9e;
            edit(&mut s);
            let mut game = Game::new();
            game.engine = Some(Engine::with_game(
                s,
                Files::default(),
                move |_s, io| async move {
                    let _keys = io.own_keys(claim);
                    io.sleep(u32::MAX).await;
                },
            ));
            game.step(20);
            game.keys_owned()
        };
        let roster = |size: u8, first: u8| {
            move |s: &mut u6_core::State| {
                s.ui.panel.mode = 0x91;
                s.actors.party.size = size;
                s.ui.panel.roster_first = first;
            }
        };
        assert_eq!(Game::new().keys_owned(), 0, "before a start");
        assert_eq!(owned(&|_| {}, 0), 0, "at the prompt");
        assert_eq!(owned(&|s| s.ui.input.line_input = true, 0), OWN_ALL);
        assert_eq!(owned(&|s| s.ui.panel.mode = 0x90, 0), OWN_MINUS | OWN_PLUS);
        assert_eq!(owned(&|s| s.ui.panel.mode = 0x92, 0), OWN_MINUS | OWN_PLUS);
        assert_eq!(
            owned(&roster(6, 0), 0),
            OWN_PLUS,
            "six from the first: down only"
        );
        assert_eq!(
            owned(&roster(6, 1), 0),
            OWN_MINUS,
            "six from the second: up only"
        );
        assert_eq!(
            owned(&roster(7, 1), 0),
            OWN_MINUS | OWN_PLUS,
            "seven from the second"
        );
        assert_eq!(owned(&roster(5, 0), 0), 0, "five fit");
        let menu = OWN_MINUS | OWN_EQUALS;
        assert_eq!(
            owned(&|s| s.main.program = Program::U, menu),
            menu,
            "u's menu"
        );
        assert_eq!(
            owned(&|s| s.ui.panel.mode = 0x90, OWN_EQUALS),
            OWN_ALL,
            "the Io's claim and the state together"
        );
        assert_eq!(
            owned(&|s| s.main.program = Program::U, 0),
            0,
            "u's own state, not game's"
        );
        let mut ended = Game::new();
        ended.engine = Some(Engine::with_game(
            u6_core::State::empty(),
            Files::default(),
            |_s, _io| async {},
        ));
        ended.step(20);
        assert!(ended.finished());
        assert_eq!(ended.keys_owned(), 0, "after the end");
    }

    /// Spec 14 Design 6-7: the side panel's view for the page: empty before a start; the mode,
    /// the member, the member's actor slot and whether a conversation runs, and the member's
    /// roster name, as the state holds them (synthetic state and names); no member (a negative
    /// index or one past the roster): actor 0, no name.
    #[test]
    fn panel_view_reads_the_mode_the_member_its_actor_and_name() {
        assert!(Game::new().panel_view().is_empty(), "before a start");
        assert_eq!(Game::new().panel_name(), "", "before a start");
        let view = |mode: u16, member: i16, talking: bool| {
            let mut s = u6_core::State::empty();
            s.ui.panel.mode = mode;
            s.ui.panel.member = member;
            s.converse.active = talking;
            s.actors.party.names = ["Ab", "Cd", "Ef"].map(String::from).to_vec();
            s.actors.party.members[..3].copy_from_slice(&[5, 9, 12]);
            let mut game = Game::new();
            game.engine = Some(Engine::with_game(
                s,
                Files::default(),
                |_s, io| async move {
                    io.sleep(u32::MAX).await;
                },
            ));
            (game.panel_view(), game.panel_name())
        };
        let shown = |mode: u16, member: i16| view(mode, member, false);
        assert_eq!(shown(0x90, 1), (vec![0x90, 1, 9, 0], "Cd".to_string()));
        assert_eq!(shown(0x92, 2), (vec![0x92, 2, 12, 0], "Ef".to_string()));
        assert_eq!(shown(0x91, 0), (vec![0x91, 0, 5, 0], "Ab".to_string()));
        assert_eq!(shown(0x9e, -1), (vec![0x9e, -1, 0, 0], String::new()));
        assert_eq!(
            shown(0x90, 3),
            (vec![0x90, 3, 0, 0], String::new()),
            "past the names"
        );
        assert_eq!(
            view(0x91, 1, true),
            (vec![0x91, 1, 9, 1], "Cd".to_string()),
            "a conversation, the roster's mode kept"
        );
    }

    const ZIP: &str = "/Users/zhugehyuk/Games/ultima6/original/ultima-vi.zip";

    /// On the user's zip (skipped when absent; read only): the disk entries declare well under
    /// the unzip limit, the start keeps every game file in place, and those files given back as
    /// loose files start the same game. The second start runs with HD on at 4× (spec 07 HD0):
    /// the start moves the page's version and composes its first HD frame whole, and the frames
    /// after it are the definition's.
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
        again.set_hd(true, 4);
        again.hd_take_dirty();
        let v = again.frame_version();
        again.boot(1, false).quiet();
        assert!(again.game_files() == n);
        assert!(again.frame_version() > v, "the start moves the version");
        assert_eq!(again.hd_frame_len(), frame_bytes(4));
        assert_eq!(again.hd_take_dirty(), frame_rect(4), "composed whole");
        assert!(
            again.hd_frame_rgba() == expected_hd(&again, Scale::X4),
            "the first HD frame"
        );
        for _ in 0..60 {
            for g in [&mut game, &mut again] {
                g.step(16);
                g.present();
            }
        }
        assert!(game.frame_rgba() == again.frame_rgba(), "same first frames");
        assert!(
            again.hd_frame_rgba() == expected_hd(&again, Scale::X4),
            "the HD frame then"
        );
        assert!(
            game.program() == "u",
            "a start runs the launcher: `u` first"
        );
        assert!(!game.finished());
    }

    /// On the user's zip (skipped when absent; read only): a start gives every buffer the source
    /// plane (spec 07 HD0), the frames name images (counted, never printed), and the frames and
    /// the page's RGBA are those of an engine without the plane, step by step.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn a_start_keeps_the_source_plane_and_the_same_frames() {
        if !std::path::Path::new(ZIP).exists() {
            eprintln!("skipped: {ZIP} not present");
            return;
        }
        let data = std::fs::read(ZIP).quiet();
        let r = intake(vec![InputFile {
            name: "ultima-vi.zip".into(),
            data: data.clone(),
        }]);
        let mut plain = Engine::game_first(Files::new(r.files), 1).quiet();
        let mut game = Game::new();
        game.collect("ultima-vi.zip", data).quiet();
        game.boot(1, true).quiet();
        let mut attributed = 0;
        for step in 0..600u32 {
            game.step(16);
            game.present();
            plain.tick(&[], 16);
            let e = game.engine.as_ref().quiet();
            let src = e.frame().src().quiet();
            attributed = src
                .iter()
                .filter(|s| s.kind != u6_core::Kind::Classic)
                .count();
            assert!(
                e.io().published(|p| p.src().is_some()),
                "the published frame"
            );
            assert!(e.io().draw(|f| f.src().is_some()), "the screen");
            assert_same!(
                e.frame().pixels(),
                plain.frame().pixels(),
                "step {step}: frame"
            );
            assert!(
                e.frame().palette == plain.frame().palette,
                "step {step}: palette"
            );
        }
        let mut rgba = Vec::new();
        plain.frame().write_rgba(&mut rgba);
        assert_same!(game.frame_rgba(), rgba, "the page's RGBA");
        assert!(
            plain.frame().src().is_none(),
            "an engine alone has no plane"
        );
        assert!(attributed > 0, "the last frame names images");
    }

    /// On the user's zip (skipped when absent): a start over a running game converts the new
    /// game's frame at once (its versions start again at 0, the page's version still moves on),
    /// and its presents convert its own frames, as the old always-convert path gives them.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn a_start_over_a_running_game_converts_the_new_games_frame() {
        if !std::path::Path::new(ZIP).exists() {
            eprintln!("skipped: {ZIP} not present");
            return;
        }
        let data = std::fs::read(ZIP).quiet();
        let mut game = Game::new();
        game.collect("ultima-vi.zip", data.clone()).quiet();
        game.boot(1, false).quiet();
        let old_path = |g: &Game| {
            let mut rgba = Vec::new();
            g.engine.as_ref().quiet().frame().write_rgba(&mut rgba);
            rgba
        };
        for _ in 0..300 {
            game.step(20);
            game.present();
        }
        assert!(game.rgba == old_path(&game), "the first game's frame");
        let (first, version) = (game.frame_rgba(), game.frame_version());
        game.collect("ultima-vi.zip", data).quiet();
        game.boot(2, true).quiet();
        assert!(game.frame_version() > version, "the start converted");
        assert!(game.rgba == old_path(&game), "the new game's first frame");
        assert!(game.rgba != first, "not the replaced game's");
        let mut converted = 0;
        for _ in 0..120 {
            game.step(20);
            let v = game.frame_version();
            game.present();
            converted += u32::from(game.frame_version() != v);
            assert!(game.rgba == old_path(&game), "the new game's frames");
        }
        assert!(converted > 0, "the new game's presents convert");
        assert!(
            game.program() == "game",
            "the second start runs game at once"
        );
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
                game.step(16);
                game.present();
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
            fresh.step(16);
            fresh.present();
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
                game.step(16);
                game.present();
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
            for g in [&mut sliced, &mut whole] {
                g.step(16);
                g.present();
            }
        }
        assert!(
            sliced.frame_rgba() == whole.frame_rgba(),
            "the same first frames"
        );
    }

    /// A game on synthetic state for HD: records 1 and 2 hold patterns MMPX changes, record 3
    /// the colours of record 1; a key `a` draws record 1 at the next cell of row 2 and presents,
    /// `c` draws the pixels of the last cell drawn again in the same colours but classic and
    /// presents the sources alone, `d` draws record 1 over that cell again (its colours the same,
    /// its sources the record's) and presents the sources alone, `b` draws record 2 over the
    /// first cell and presents, `m` rewrites record 1 in place as the shore merge does (its
    /// pixels and its version move), draws it at the last cell and presents (the cells drawn
    /// before keep pixels of its old version).
    fn tile_engine() -> Engine {
        tile_engine_on(u6_core::State::empty())
    }

    /// [`tile_engine`] on `state` (its records 1..=3 set as there): the pointer events queued in
    /// front of a key are taken first, so a state with the pointer tables can be pointed at too.
    fn tile_engine_on(mut state: u6_core::State) -> Engine {
        for (r, seed) in [(1usize, 3u32), (2, 7), (3, 3)] {
            let rec = &mut state.gfx.records[r];
            rec.pixels = (0..256u32)
                .map(|i| ((i / 16 + i % 16 + seed * ((i * 7) % 5)) % 4) as u8 + 1)
                .collect();
            rec.transparent = vec![false; 256];
        }
        let mut engine = Engine::with_game(state, Files::default(), |state, io| async move {
            io.draw(|fb| {
                for (i, c) in fb.palette.iter_mut().enumerate() {
                    let v = i as u8;
                    *c = u6_core::Rgb::new(v.wrapping_mul(40), v.wrapping_mul(90), 255 - v);
                }
            });
            io.present();
            let mut cell = 0u16;
            loop {
                io.take_pointer();
                while io.key_ready() {
                    let key = io.next_key().await;
                    let s = state.borrow();
                    match key {
                        Key::Char('a') => {
                            cell += 1;
                            io.draw(|fb| {
                                u6_core::gfx::blit::blit_front(&s.gfx, fb, 1, cell * 16, 32)
                            });
                            io.present();
                        }
                        Key::Char('b') => {
                            io.draw(|fb| u6_core::gfx::blit::blit_front(&s.gfx, fb, 2, 16, 32));
                            io.present();
                        }
                        Key::Char('m') => {
                            drop(s);
                            let mut w = state.borrow_mut();
                            let rec = &mut w.gfx.records[1];
                            for p in rec.pixels.iter_mut() {
                                *p = *p % 4 + 1;
                            }
                            w.gfx.cache.record_changed(1);
                            drop(w);
                            let s = state.borrow();
                            io.draw(|fb| {
                                u6_core::gfx::blit::blit_front(&s.gfx, fb, 1, cell * 16, 32)
                            });
                            io.present();
                        }
                        Key::Char('c') => {
                            io.draw(|fb| {
                                for i in 0..256 {
                                    let (x, y) = (usize::from(cell) * 16 + i % 16, 32 + i / 16);
                                    let p = fb.get(x, y).unwrap_or(0);
                                    fb.set(x, y, p);
                                }
                            });
                            io.present_sources();
                        }
                        Key::Char('d') => {
                            io.draw(|fb| {
                                u6_core::gfx::blit::blit_front(&s.gfx, fb, 1, cell * 16, 32)
                            });
                            io.present_sources();
                        }
                        _ => {}
                    }
                }
                io.vblank().await;
            }
        });
        engine.enable_src_plane();
        engine
    }

    /// The HD frame of the engine's shown frame at `scale` by the definition, as RGBA bytes,
    /// built here pixel by pixel (no compositor): a tile pixel without flags whose version is its
    /// record's now takes its block of MMPX ×S of the record (transparent sub-pixels its classic
    /// colour), every other pixel its classic colour S × S.
    fn expected_hd(game: &Game, scale: Scale) -> Vec<u8> {
        expected_hd_with(game, scale, None)
    }

    /// [`expected_hd`] with the pack `pack` set: a tile pixel's block is of the pack's image
    /// instead when the pack holds one for its record made from the record's bytes now (at 2×
    /// `u6_render::hd::pack_words`, each sub-pixel over the classic colour, both tested on their
    /// own in u6-render).
    fn expected_hd_with(game: &Game, scale: Scale, pack: Option<&u6_hdpack::Pack>) -> Vec<u8> {
        let e = game.engine.as_ref().unwrap();
        let (fb, st) = (e.frame(), e.state());
        let s = scale.factor();
        let rgba = |c: u6_core::Rgb| [c.r, c.g, c.b, 255];
        let w = u6_render::WIDTH;
        let mut out = vec![0u8; frame_bytes(s)];
        let mut made: BTreeMap<u16, Vec<u32>> = BTreeMap::new();
        for y in 0..u6_render::HEIGHT {
            for x in 0..w {
                let src = fb.src_at(x, y).unwrap();
                let classic = rgba(fb.palette[usize::from(fb.get(x, y).unwrap())]);
                let id = usize::from(src.id);
                let hd = src.kind == u6_core::Kind::Tile
                    && src.flags == 0
                    && src.ver == st.gfx.cache.version(id);
                for k in 0..s * s {
                    let (sx, sy) = (x * s + k % s, y * s + k / s);
                    let c = if hd {
                        let words = made.entry(src.id).or_insert_with(|| {
                            let rec = &st.gfx.records[id];
                            let key = u6_hdpack::tile_hash(&rec.pixels, &rec.transparent);
                            let art = pack
                                .and_then(|p| p.get(u6_hdpack::ImageId::Tile(src.id)))
                                .filter(|a| Some(a.hash) == key);
                            if let Some(a) = art {
                                let mut out = Vec::new();
                                u6_render::hd::pack_words(&a.image.pixels, 64, 64, scale, &mut out);
                                return out;
                            }
                            let input: Vec<u32> = (0..256)
                                .map(|i| {
                                    if rec.transparent[i] {
                                        0
                                    } else {
                                        u32::from_le_bytes(rgba(
                                            fb.palette[usize::from(rec.pixels[i])],
                                        ))
                                    }
                                })
                                .collect();
                            let mut out = Vec::new();
                            u6_render::hd::mmpx(&input, 16, 16, scale, &mut out);
                            out
                        });
                        let (u, v) = (usize::from(src.u), usize::from(src.v));
                        let w = words[(v * s + k / s) * 16 * s + u * s + k % s];
                        u6_render::hd::over(w, u32::from_le_bytes(classic)).to_le_bytes()
                    } else {
                        classic
                    };
                    out[(sy * w * s + sx) * 4..][..4].copy_from_slice(&c);
                }
            }
        }
        out
    }

    /// The HD frame's bytes of cell (`col`, row 2) of [`tile_engine`] at scale `s`.
    fn cell_bytes(game: &Game, col: usize, s: usize) -> Vec<u8> {
        let rgba = game.hd_frame_rgba();
        (32 * s..48 * s)
            .flat_map(|y| {
                let at = (y * u6_render::WIDTH * s + col * 16 * s) * 4;
                rgba[at..at + 16 * s * 4].to_vec()
            })
            .collect()
    }

    fn press(game: &mut Game, key: &str) {
        game.key_down(&format!("Key{}", key.to_uppercase()), key, false);
        game.step(20);
        game.present();
    }

    /// Spec 07 HD0 in the platform: off (the default), the compositor never runs and the RGBA
    /// frame is the old path's; on, each present composes when the shown frame's colours or
    /// sources changed (once, and the page's version moves only then), into the frame by the
    /// definition ([`expected_hd`]); a change of the sources alone, either way (an HD cell put
    /// back classic in its colours, then the record's sources put back), is composed on the
    /// source version alone; switching on, a scale change and switching off take effect at once,
    /// with no step.
    #[test]
    fn the_hd_frame_follows_the_shown_frame_and_the_switch() {
        let mut game = Game::new();
        game.put_on_screen(tile_engine());
        for k in ["a", "a", "x"] {
            press(&mut game, k);
        }
        assert!(
            !game.hd_on() && game.hd_frame_len() == 0,
            "off: no HD frame"
        );
        let mut old = Vec::new();
        game.engine.as_ref().unwrap().frame().write_rgba(&mut old);
        assert!(game.rgba == old, "off: the RGBA path");
        assert_eq!(game.frame_version(), game.conversions as f64);
        // On at 4x: composed at once, before any step.
        let v = game.frame_version();
        game.set_hd(true, 4);
        assert!(game.frame_version() > v, "the switch moves the version");
        assert_eq!((game.hd_on(), game.hd_scale()), (true, 4));
        assert_eq!(game.hd_frame_len(), frame_bytes(4));
        assert!(
            game.hd_frame_rgba() == expected_hd(&game, Scale::X4),
            "a new compositor's frame"
        );
        let nearest = |g: &Game, s: usize| -> Vec<u8> {
            let mut out = Vec::new();
            for y in 0..u6_render::HEIGHT * s {
                for x in 0..u6_render::WIDTH * s {
                    let at = ((y / s) * u6_render::WIDTH + x / s) * 4;
                    out.extend_from_slice(&g.rgba[at..at + 4]);
                }
            }
            out
        };
        assert!(
            game.hd_frame_rgba() != nearest(&game, 4),
            "the record's pixels are MMPX'd"
        );
        assert_eq!(
            game.hd_take_dirty(),
            frame_rect(4),
            "the first composition: all"
        );
        // A present with nothing new composes nothing; a key that draws composes once.
        let v = game.frame_version();
        game.step(20);
        game.present();
        assert_eq!(game.frame_version(), v, "no change, no composition");
        assert!(game.hd_take_dirty().is_empty());
        press(&mut game, "a");
        assert_eq!(
            game.frame_version(),
            v + 1.0,
            "one composition, one version"
        );
        assert_eq!(
            game.hd_take_dirty(),
            vec![48 * 4, 32 * 4, 16 * 4, 16 * 4],
            "the cell drawn"
        );
        assert!(game.hd_frame_rgba() == expected_hd(&game, Scale::X4));
        // The same colours, the sources alone changed: composed again on the source version
        // alone, the cell's blocks classic; then the record's sources back, the blocks HD again.
        let hd_cell = cell_bytes(&game, 3, 4);
        for (key, what) in [("c", "classic"), ("d", "the record again")] {
            let e = game.engine.as_ref().unwrap();
            let (fv, sv) = (e.frame_version(), e.src_version());
            let v = game.frame_version();
            press(&mut game, key);
            let e = game.engine.as_ref().unwrap();
            assert_eq!(
                (e.frame_version(), e.src_version()),
                (fv, sv + 1),
                "{what}: a source-only change"
            );
            assert_eq!(
                game.frame_version(),
                v + 1.0,
                "{what}: composed for the sources"
            );
            assert_eq!(
                game.hd_take_dirty(),
                vec![48 * 4, 32 * 4, 16 * 4, 16 * 4],
                "{what}"
            );
            assert!(
                game.hd_frame_rgba() == expected_hd(&game, Scale::X4),
                "{what}"
            );
            let cell = cell_bytes(&game, 3, 4);
            assert_eq!(cell == hd_cell, key == "d", "{what}: the cell's blocks");
        }
        // 2x: composed at once.
        let v = game.frame_version();
        game.set_hd(true, 2);
        assert!(game.frame_version() > v);
        assert_eq!((game.hd_scale(), game.hd_frame_len()), (2, frame_bytes(2)));
        assert!(game.hd_frame_rgba() == expected_hd(&game, Scale::X2));
        game.set_hd(true, 3);
        assert_eq!(game.hd_scale(), 2, "any other scale is 2");
        // Off: the RGBA frame; on again over frames composed meanwhile: exact at once.
        let v = game.frame_version();
        game.set_hd(false, 2);
        assert!(game.frame_version() > v && !game.hd_on());
        let held = game.hd_frame_rgba();
        press(&mut game, "b");
        press(&mut game, "a");
        assert!(game.hd_frame_rgba() == held, "off: not composed");
        let mut old = Vec::new();
        game.engine.as_ref().unwrap().frame().write_rgba(&mut old);
        assert!(game.rgba == old, "off: the RGBA path");
        game.set_hd(true, 2);
        assert!(
            game.hd_frame_rgba() == expected_hd(&game, Scale::X2),
            "on again: exact"
        );
        // A start puts a new engine on screen (the start path's own step): its versions start
        // again at 0, and its first frame is composed whole at once.
        let v = game.frame_version();
        game.hd_take_dirty();
        game.put_on_screen(tile_engine());
        assert!(game.frame_version() > v, "the start moves the version");
        assert_eq!(game.hd_frame_len(), frame_bytes(2));
        assert_eq!(game.hd_take_dirty(), frame_rect(2), "composed whole");
        assert!(
            game.hd_frame_rgba() == expected_hd(&game, Scale::X2),
            "its first frame"
        );
        for k in ["a", "a"] {
            press(&mut game, k);
        }
        assert!(
            game.hd_frame_rgba() == expected_hd(&game, Scale::X2),
            "and its next"
        );
    }

    /// An engine on screen with the plane that draws text cells of a synthetic font (glyph `c` =
    /// eight rows of the byte `c`) on keys: `t` a letter at cell (2, 2), another at (3, 2) and a
    /// space at (4, 2), in ink 3 on paper 4; `r` the first letter again in ink 5; `p` a classic
    /// pixel over it, a classic write of the game's.
    fn text_engine() -> Engine {
        let mut engine = Engine::with_game(
            u6_core::State::empty(),
            Files::default(),
            |_, io| async move {
                let font = u6_formats::font::BitFont {
                    glyphs: (0..=255u8).map(|c| [c; 8]).collect(),
                };
                let glyph = |c: u8, col: usize, fg: u8| {
                    io.draw(|fb| u6_core::ui::text::draw_glyph(fb, &font, col * 8, 16, c, fg, 4));
                };
                io.draw(|fb| {
                    for (i, c) in fb.palette.iter_mut().enumerate() {
                        let v = i as u8;
                        *c = u6_core::Rgb::new(v.wrapping_mul(40), 255 - v, v.wrapping_mul(7));
                    }
                });
                io.present();
                loop {
                    while io.key_ready() {
                        match io.next_key().await {
                            Key::Char('t') => {
                                glyph(b'A', 2, 3);
                                glyph(b'z', 3, 3);
                                glyph(b' ', 4, 3);
                            }
                            Key::Char('r') => glyph(b'A', 2, 5),
                            Key::Char('p') => io.draw(|fb| fb.set(19, 20, 9)),
                            _ => {}
                        }
                        io.present();
                    }
                    io.vblank().await;
                }
            },
        );
        engine.enable_src_plane();
        engine
    }

    /// Spec 11 in the platform: the HD font is off until switched on, and lists nothing while HD
    /// or it is off; switched on with HD on it composes at once (the whole frame, the version
    /// moved, no step) and lists every letter cell, not the space (the synthetic font's space
    /// has ink, so it is not a whole space), as x, y, code, ink, paper; a recoloured letter and
    /// a letter under a classic write change the list with their cells in the rect; off again, the
    /// HD frame is the definition's (the pixel font); with HD off a switch moves nothing until
    /// HD is on again.
    #[test]
    fn the_hd_font_lists_the_letters_shown_and_follows_its_switch() {
        let mut game = Game::new();
        game.put_on_screen(text_engine());
        press(&mut game, "t");
        game.set_hd(true, 2);
        assert!(
            !game.hd_font() && game.hd_glyphs().is_empty(),
            "off by default"
        );
        let e = game.engine.as_ref().unwrap();
        let rgb = |i: usize| {
            let c = e.frame().palette[i];
            u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
        };
        let (ink3, ink5, paper) = (rgb(3), rgb(5), rgb(4));
        game.hd_take_dirty();
        let v = game.frame_version();
        game.set_hd_font(true);
        assert!(
            game.hd_font() && game.frame_version() > v,
            "composed at once"
        );
        assert_eq!(game.hd_take_dirty(), frame_rect(2), "the whole frame");
        let letters = |a: u32| vec![16, 16, 0x41, a, paper, 24, 16, 0x7a, ink3, paper];
        assert_eq!(
            game.hd_glyphs(),
            letters(ink3),
            "the letters, not the space"
        );
        let v = game.frame_version();
        game.set_hd_font(true);
        assert_eq!(game.frame_version(), v, "no change, nothing composed");
        press(&mut game, "r");
        assert_eq!(game.hd_glyphs(), letters(ink5), "the letter recoloured");
        assert_eq!(game.hd_take_dirty(), vec![32, 32, 16, 16], "its cell");
        assert!(game.frame_version() > v);
        press(&mut game, "p");
        assert_eq!(
            game.hd_glyphs(),
            vec![24, 16, 0x7a, ink3, paper],
            "under a classic write"
        );
        assert_eq!(
            game.hd_take_dirty(),
            vec![32, 32, 16, 16],
            "its cell, whole"
        );
        let v = game.frame_version();
        game.set_hd_font(false);
        assert!(!game.hd_font() && game.frame_version() > v);
        assert!(game.hd_glyphs().is_empty(), "off: nothing listed");
        assert!(
            game.hd_frame_rgba() == expected_hd(&game, Scale::X2),
            "off: the pixel font, as before"
        );
        // HD off: the switch moves nothing and nothing is listed, until HD is on again.
        game.set_hd(false, 2);
        let v = game.frame_version();
        game.set_hd_font(true);
        assert!(
            game.hd_font() && game.frame_version() == v,
            "HD off: nothing moves"
        );
        assert!(game.hd_glyphs().is_empty(), "HD off: nothing listed");
        game.set_hd(true, 2);
        assert!(game.frame_version() > v);
        assert_eq!(
            game.hd_glyphs(),
            vec![24, 16, 0x7a, ink3, paper],
            "HD on again"
        );
    }

    /// An engine on screen with the plane that draws, over a paper of colour 4 named in place
    /// (spec 07 HD0), in ink 3: a letter at cell (2, 2), a space the player typed at (3, 2) and
    /// the key wait's symbol of glyph 5 at (4, 2) (spec 11 rev 3; a synthetic font: glyph `c` =
    /// eight rows of the byte `c`, the space blank).
    fn symbol_engine() -> Engine {
        let mut engine = Engine::with_game(
            u6_core::State::empty(),
            Files::default(),
            |_, io| async move {
                use u6_core::ui::text::{self, Paper, TextWindow};
                let mut font = u6_formats::font::BitFont {
                    glyphs: (0..=255u8).map(|c| [c; 8]).collect(),
                };
                font.glyphs[0x20] = [0; 8];
                let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
                let paper = Paper::drawn(
                    &identity,
                    &u6_formats::bitmap::Bitmap {
                        width: 320,
                        height: 200,
                        pixels: vec![4; 320 * 200],
                    },
                );
                let w = TextWindow::new(0, 0, 39, 24, 3, 4, 0);
                let fonts = (Some(&font), None);
                io.draw(|fb| {
                    for (i, c) in fb.palette.iter_mut().enumerate() {
                        let v = i as u8;
                        *c = u6_core::Rgb::new(v.wrapping_mul(40), 255 - v, v.wrapping_mul(7));
                    }
                    text::fill_rect(fb, 0, 0, 319, 199, 4, Some(&paper));
                    text::draw_char(fb, fonts.0, None, &w, 0x41, 2, 2);
                    text::draw_typed(fb, fonts, &w, 0x20, (3, 2));
                    text::draw_symbol(fb, fonts, &w, 5, (4, 2), Some(&paper));
                });
                io.present();
                loop {
                    io.vblank().await;
                }
            },
        );
        engine.enable_src_plane();
        engine
    }

    /// Spec 11 rev 3 in the platform: the list's code of a space the player typed is 0x20000 |
    /// 0x20 and of a symbol 0x10000 | its glyph, a letter's as before; the symbol's rows follow in
    /// [`Game::hd_symbol_rows`]; both empty with the HD font off.
    #[test]
    fn the_hd_glyphs_give_a_typed_space_and_a_symbol_in_their_ranges() {
        let mut game = Game::new();
        game.put_on_screen(symbol_engine());
        game.step(20);
        game.present();
        game.set_hd(true, 2);
        game.set_hd_font(true);
        let e = game.engine.as_ref().unwrap();
        let rgb = |i: usize| {
            let c = e.frame().palette[i];
            u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
        };
        let (ink, paper) = (rgb(3), rgb(4));
        assert_eq!(
            game.hd_glyphs(),
            vec![
                16, 16, 0x41, ink, paper, 24, 16, 0x2_0020, paper, paper, 32, 16, 0x1_0005, ink,
                paper
            ],
            "the letter, the typed space, the symbol"
        );
        assert_eq!(game.hd_symbol_rows(), vec![5; 8], "the symbol's rows");
        game.set_hd_font(false);
        assert!(game.hd_glyphs().is_empty() && game.hd_symbol_rows().is_empty());
    }

    /// The pointer shape of [`pointer_engine`]: three rows, values `(c + r) % 4` in the first
    /// five columns (so 0, clear, inside the box too), its top left at (-2, -1) from the hot spot.
    fn pointer_shape() -> u6_core::ui::pointer::Shape {
        u6_core::ui::pointer::Shape {
            dx: -2,
            dy: -1,
            rows: (0..3)
                .map(|r| std::array::from_fn(|c| if c < 5 { ((c + r) % 4) as u8 } else { 0 }))
                .collect(),
        }
    }

    /// An engine on screen with the plane and the pointer tables of [`pointer_shape`] (shown), a
    /// palette of distinct colours and five letters of the synthetic font at cells 2..=6 of row 2
    /// (ink 3, paper 4); on keys `h` the pointer hidden, `s` shown again, `p` the sprite's
    /// colours 17..=19 changed in the palette and presented.
    fn pointer_engine() -> Engine {
        let mut state = u6_core::State::empty();
        state.ui.pointer =
            u6_core::ui::pointer::PointerState::from_tables(u6_core::ui::pointer::PointerTables {
                shapes: vec![pointer_shape()],
                shown: true,
                ..Default::default()
            });
        let mut engine = Engine::with_game(state, Files::default(), |st, io| async move {
            let font = u6_formats::font::BitFont {
                glyphs: (0..=255u8).map(|c| [c; 8]).collect(),
            };
            io.draw(|fb| {
                for (i, c) in fb.palette.iter_mut().enumerate() {
                    let v = i as u8;
                    *c = u6_core::Rgb::new(v.wrapping_mul(40), 255 - v, v.wrapping_mul(7));
                }
                for (k, col) in (2..7).enumerate() {
                    u6_core::ui::text::draw_glyph(fb, &font, col * 8, 16, b'a' + k as u8, 3, 4);
                }
            });
            io.present();
            loop {
                // The pointer events in front of a key are taken first (input comes in order).
                io.take_pointer();
                while io.key_ready() {
                    match io.next_key().await {
                        Key::Char('h') => st.borrow_mut().ui.pointer.shown = false,
                        Key::Char('s') => st.borrow_mut().ui.pointer.shown = true,
                        Key::Char('p') => {
                            io.draw(|fb| {
                                for v in 17..20u8 {
                                    fb.palette[usize::from(v)] = u6_core::Rgb::new(v, 2 * v, 3 * v);
                                }
                            });
                            io.present();
                        }
                        _ => {}
                    }
                }
                io.vblank().await;
            }
        });
        engine.enable_src_plane();
        engine
    }

    /// The pointer at (x, y), one step and a present.
    fn point(game: &mut Game, x: i32, y: i32) {
        game.pointer(x, y, 0);
        game.step(20);
        game.present();
    }

    /// [`Game::game_pointer`]: where the engine placed its sprite in the last step and whether
    /// it is drawn, with HD off and on; a pointer queued since is given apart (the last one, a
    /// press and its release too) and moves the place only at the next step; hidden, not shown.
    /// All 0 before a start.
    #[test]
    fn the_game_pointer_is_where_the_engine_placed_its_sprite() {
        let mut game = Game::new();
        assert_eq!(game.game_pointer(), vec![0; 6], "before a start: none");
        game.put_on_screen(pointer_engine());
        point(&mut game, 100, 60);
        assert_eq!(
            game.game_pointer(),
            [100, 60, 1, 0, 0, 0],
            "HD off: placed, shown"
        );
        let sp = game.engine.as_ref().unwrap().sprite().cloned().unwrap();
        assert_eq!((sp.x, sp.y), (98, 59), "the sprite drawn from there");
        game.set_hd(true, 2);
        assert_eq!(
            game.game_pointer(),
            [100, 60, 1, 0, 0, 0],
            "HD on: the same"
        );
        game.pointer(140, 80, 0);
        game.pointer(141, 82, 0);
        assert_eq!(
            game.game_pointer(),
            [100, 60, 1, 1, 141, 82],
            "queued: the sprite still where it was, the last queued apart"
        );
        game.step(20);
        assert_eq!(
            game.game_pointer(),
            [141, 82, 1, 0, 0, 0],
            "stepped: placed there"
        );
        game.pointer(5, 6, 1);
        game.pointer(5, 6, 0);
        assert_eq!(
            game.game_pointer()[3..],
            [1, 5, 6],
            "a press and release queued"
        );
        game.step(20);
        press(&mut game, "h");
        assert_eq!(game.game_pointer(), [5, 6, 0, 0, 0, 0], "hidden: not shown");
        press(&mut game, "s");
        assert_eq!(game.game_pointer()[2], 1, "shown again");
    }

    /// Spec 11 Design 7 in the platform: with HD on, the HD frame is composed without the pointer
    /// sprite (the shown frame has it); the pointer is given apart ([`Game::hd_pointer`]): the
    /// sprite's box in frame pixels and its pixels in the shown frame's colours, transparent
    /// where the shape is clear, clipped to the frame at its edges. A move of the pointer alone,
    /// over the letters too, composes nothing (the page's version stays, no dirty rect, the same
    /// list and the same HD frame) and moves the pointer's version; a step with the pointer where
    /// it was moves nothing. Hidden, with HD off and before a start the box is empty, the version
    /// moved each time it went or came back.
    #[test]
    fn the_pointer_is_given_apart_from_the_hd_frame() {
        let (w, h) = (u6_render::WIDTH as u32, u6_render::HEIGHT as u32);
        let shape = pointer_shape();
        let mut game = Game::new();
        assert_eq!(
            game.hd_pointer(),
            vec![0, 0, 0, 0, 0],
            "before a start: none"
        );
        game.put_on_screen(pointer_engine());
        game.set_hd(true, 2);
        game.set_hd_font(true);
        point(&mut game, 100, 60);
        let p = game.hd_pointer();
        assert_eq!(p[..4], [98, 59, 16, 3], "the sprite's box");
        let e = game.engine.as_ref().unwrap();
        let (fb, published) = (e.frame(), e.published_frame());
        let rgba = game.hd_pointer_rgba();
        assert_eq!(rgba.len(), 16 * 3 * 4);
        let mut opaque = 0;
        for (i, px) in rgba.chunks_exact(4).enumerate() {
            let (c, r) = (i % 16, i / 16);
            let v = shape.rows[r][c];
            let (x, y) = (98 + c, 59 + r);
            if v == 0 {
                assert_eq!(px[3], 0, "({c}, {r}) clear: transparent");
                continue;
            }
            opaque += 1;
            let colour = fb.palette[usize::from(16 + v)];
            assert_eq!(px, [colour.r, colour.g, colour.b, 255], "({c}, {r})");
            assert_eq!(fb.get(x, y), Some(16 + v), "the shown frame has it");
            // The HD frame has the published frame's pixel there, classic, not the sprite's.
            let under = published.palette[usize::from(published.get(x, y).unwrap())];
            let at = ((y * 2) * u6_render::WIDTH * 2 + x * 2) * 4;
            assert_eq!(
                game.hd_frame_rgba()[at..at + 4],
                [under.r, under.g, under.b, 255],
                "({c}, {r}): the HD frame without the sprite"
            );
        }
        assert!(opaque > 0 && opaque < 15, "clear pixels inside the box too");
        drop(published);
        // Moves of the pointer alone: nothing composed, the pointer's version moved. The baseline
        // first: the five letters listed.
        let (v, held, list) = (game.frame_version(), game.hd_frame_rgba(), game.hd_glyphs());
        let e = game.engine.as_ref().unwrap();
        let rgb = |i: usize| {
            let c = e.frame().palette[i];
            u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
        };
        let letters: Vec<u32> = (2..7u32)
            .zip(0..)
            .flat_map(|(col, k)| [col * 8, 16, u32::from(b'a') + k, rgb(3), rgb(4)])
            .collect();
        assert_eq!(list, letters, "the baseline: the five letters listed");
        game.hd_take_dirty();
        let mut version = p[4];
        for (x, y, what) in [(140, 80, "a move"), (27, 19, "over the letters")] {
            point(&mut game, x, y);
            let p = game.hd_pointer();
            assert_eq!(
                p[..4],
                [x as u32 - 2, y as u32 - 1, 16, 3],
                "{what}: the box moved"
            );
            assert_eq!(p[4], version + 1, "{what}: the pointer's version moved");
            version = p[4];
            assert_eq!(game.frame_version(), v, "{what}: nothing composed");
            assert!(game.hd_take_dirty().is_empty(), "{what}: no dirty rect");
            assert_eq!(game.hd_glyphs(), list, "{what}: the same letters listed");
            assert!(game.hd_frame_rgba() == held, "{what}: the same HD frame");
        }
        point(&mut game, 27, 19);
        assert_eq!(
            game.hd_pointer()[4],
            version,
            "the same place: nothing moves"
        );
        // At the frame's edges: clipped, its pixels those of the part inside.
        let clip = |game: &Game| {
            let rgba = game.hd_pointer_rgba();
            rgba.chunks_exact(4).map(|px| px[3]).collect::<Vec<u8>>()
        };
        let alpha = |c0: usize, r0: usize, cw: usize, rh: usize| -> Vec<u8> {
            (0..rh * cw)
                .map(|i| shape.rows[r0 + i / cw][c0 + i % cw])
                .map(|v| if v == 0 { 0 } else { 255 })
                .collect()
        };
        point(&mut game, w as i32 - 1, h as i32 - 1);
        assert_eq!(
            game.hd_pointer()[..4],
            [w - 3, h - 2, 3, 2],
            "the far corner"
        );
        assert_eq!(clip(&game), alpha(0, 0, 3, 2));
        point(&mut game, 0, 0);
        assert_eq!(game.hd_pointer()[..4], [0, 0, 14, 2], "the near corner");
        assert_eq!(clip(&game), alpha(2, 1, 14, 2));
        // Hidden, shown again; HD off, on again.
        version = game.hd_pointer()[4];
        let gone = |game: &Game, version: u32, what: &str| {
            let p = game.hd_pointer();
            assert_eq!(p, vec![0, 0, 0, 0, version + 1], "{what}: none");
            assert!(game.hd_pointer_rgba().is_empty(), "{what}: no pixels");
        };
        press(&mut game, "h");
        gone(&game, version, "hidden");
        press(&mut game, "s");
        assert_eq!(game.hd_pointer()[..4], [0, 0, 14, 2], "shown again");
        version = game.hd_pointer()[4];
        game.set_hd(false, 2);
        gone(&game, version, "HD off");
        point(&mut game, 100, 60);
        assert_eq!(
            game.hd_pointer()[4],
            version + 1,
            "HD off: a move gives nothing"
        );
        game.set_hd(true, 2);
        assert_eq!(game.hd_pointer()[..4], [98, 59, 16, 3], "HD on again");
    }

    /// The pointer's pixels [`Game::hd_pointer_rgba`] is to give for the running engine's sprite
    /// whole in the frame ([`pointer_shape`]'s 16 × 3): value v ≠ 0 in the shown frame's colour
    /// 16 + v, opaque; 0 transparent.
    fn expected_pointer(game: &Game) -> Vec<u8> {
        let fb = game.engine.as_ref().unwrap().frame();
        pointer_shape()
            .rows
            .iter()
            .flat_map(|row| row.iter())
            .flat_map(|&v| match v {
                0 => [0; 4],
                v => {
                    let c = fb.palette[usize::from(16 + v)];
                    [c.r, c.g, c.b, 255]
                }
            })
            .collect()
    }

    /// Spec 11 Design 7: the HD frame follows the published frame alone. On [`tile_engine`] with
    /// the pointer shown: a move of the sprite alone composes the shown frame again
    /// ([`Engine::frame_version`]) but leaves the published version, composes no HD block and
    /// moves only the pointer's version; a publish of the sources alone (`c`: the cell drawn
    /// classic in its colours, then `d`: the record again) moves the published version, and the
    /// HD frame is composed for it (the page's version moved, the cell in the rect), the pointer's
    /// version left as it was.
    #[test]
    fn a_publish_of_the_sources_alone_composes_and_a_sprite_move_does_not() {
        let mut state = u6_core::State::empty();
        state.ui.pointer =
            u6_core::ui::pointer::PointerState::from_tables(u6_core::ui::pointer::PointerTables {
                shapes: vec![pointer_shape()],
                shown: true,
                ..Default::default()
            });
        let mut game = Game::new();
        game.put_on_screen(tile_engine_on(state));
        game.set_hd(true, 4);
        for k in ["a", "a"] {
            press(&mut game, k);
        }
        point(&mut game, 200, 120);
        let versions = |game: &Game| {
            let e = game.engine.as_ref().unwrap();
            (e.frame_version(), e.published_version())
        };
        game.hd_take_dirty();
        let (shown, published) = versions(&game);
        let (v, pointer) = (game.frame_version(), game.hd_pointer()[4]);
        point(&mut game, 230, 140);
        assert_eq!(
            versions(&game),
            (shown + 1, published),
            "the sprite moved: the shown frame composed again, the published frame as it was"
        );
        assert_eq!(
            game.frame_version(),
            v,
            "the sprite alone: nothing composed"
        );
        assert!(
            game.hd_take_dirty().is_empty(),
            "the sprite alone: no dirty rect"
        );
        assert_eq!(
            game.hd_pointer()[4],
            pointer + 1,
            "the pointer's version moved"
        );
        for (key, what) in [("c", "classic"), ("d", "the record again")] {
            let (shown, published) = versions(&game);
            let (v, pointer) = (game.frame_version(), game.hd_pointer()[4]);
            press(&mut game, key);
            assert_eq!(
                versions(&game),
                (shown, published + 1),
                "{what}: the sources alone published (the colours as they were)"
            );
            assert_eq!(
                game.frame_version(),
                v + 1.0,
                "{what}: composed for the sources"
            );
            assert_eq!(
                game.hd_take_dirty(),
                vec![32 * 4, 32 * 4, 16 * 4, 16 * 4],
                "{what}: the cell"
            );
            assert_eq!(
                game.hd_pointer()[4],
                pointer,
                "{what}: the pointer as it was"
            );
        }
    }

    /// Spec 11 Design 7: the pointer standing still, its colours changed in the palette (`p`):
    /// the same box, its pixels in the new colours, the version moved; a new engine put on
    /// screen (a start) gives no pointer until its first step, then its own sprite's box and
    /// pixels, each time with the version moved.
    #[test]
    fn the_pointer_follows_its_colours_and_a_new_engine() {
        let mut game = Game::new();
        game.put_on_screen(pointer_engine());
        game.set_hd(true, 2);
        point(&mut game, 100, 60);
        let (before, rgba) = (game.hd_pointer(), game.hd_pointer_rgba());
        assert_eq!(before[..4], [98, 59, 16, 3]);
        assert_eq!(
            rgba,
            expected_pointer(&game),
            "its pixels in the frame's colours"
        );
        press(&mut game, "p");
        let after = game.hd_pointer();
        assert_eq!(after[..4], before[..4], "the colours changed: the same box");
        assert_eq!(
            after[4],
            before[4] + 1,
            "the colours changed: the version moved"
        );
        assert!(
            game.hd_pointer_rgba() != rgba,
            "the pixels in the new colours"
        );
        assert_eq!(game.hd_pointer_rgba(), expected_pointer(&game));
        // A start: a new engine, no sprite before its first step, then its own.
        let version = after[4];
        game.put_on_screen(pointer_engine());
        assert_eq!(
            game.hd_pointer(),
            vec![0, 0, 0, 0, version + 1],
            "the new engine before its first step: no pointer"
        );
        assert!(game.hd_pointer_rgba().is_empty());
        point(&mut game, 140, 80);
        let p = game.hd_pointer();
        assert_eq!(
            p,
            vec![138, 79, 16, 3, version + 2],
            "the new engine's sprite"
        );
        assert_eq!(
            game.hd_pointer_rgba(),
            expected_pointer(&game),
            "its pixels"
        );
    }

    /// A synthetic pack image of a tile: 64 × 64 RGBA of a pattern seeded by `seed`, opaque but
    /// for a transparent stripe and a stripe of alpha between 0 and 255.
    fn pack_tile(r: u16, hash: u64, seed: u8) -> u6_hdpack::PackImage {
        let mut image = u6_hdpack::Rgba::new(64, 64);
        for (i, p) in image.pixels.chunks_exact_mut(4).enumerate() {
            let (x, y) = (i % 64, i / 64);
            let a = match (x + y) % 11 {
                0 => 0,
                1 => 90,
                _ => 255,
            };
            p.copy_from_slice(&[(x as u8).wrapping_mul(seed), (y * 4) as u8, seed, a]);
        }
        u6_hdpack::PackImage {
            id: u6_hdpack::ImageId::Tile(r),
            hash,
            image,
        }
    }

    fn generator() -> u6_hdpack::Generator {
        u6_hdpack::Generator {
            name: "test".into(),
            version: "0".into(),
            engine: "none".into(),
            date: "2026-10-16".into(),
        }
    }

    /// A pack of `images` written and read back as the page reads it.
    fn loaded(images: &[u6_hdpack::PackImage]) -> (HdPack, u6_hdpack::Pack) {
        let bytes = u6_hdpack::write_pack(images, &generator(), true).unwrap();
        let read = u6_hdpack::read_pack(&bytes).unwrap();
        (load_pack(&bytes).unwrap(), read)
    }

    /// Record `r`'s content key in the running game now.
    fn key_of(game: &Game, r: u16) -> u64 {
        let k = game.hd_image_key(1, r);
        assert_eq!(k.len(), 16);
        u64::from_str_radix(&k, 16).unwrap()
    }

    /// Spec 07 H2 in the platform: a pack set on the game puts its image of record 1 (made from
    /// the record's bytes now) in front of MMPX at 4× and 2× (the frame by the definition), its
    /// image of record 2 (made from other bytes) is not used; setting and forgetting the pack
    /// compose at once with no step, write only the cells whose HD image changed, and equal a
    /// game that never had the other; once record 1 is rewritten in place (the shore merge's
    /// path: pixels and version), the pack's image is not used: the cell drawn from its new bytes
    /// shows MMPX of them, the cells drawn before are classic (stale). With HD off nothing of the
    /// pack runs.
    #[test]
    fn an_hd_pack_goes_in_front_of_mmpx_for_the_bytes_it_was_made_from() {
        // Record 1 at cells 2 and 3 of row 2, record 2 at cell 1.
        let mut game = Game::new();
        game.put_on_screen(tile_engine());
        for k in ["a", "a", "a", "b"] {
            press(&mut game, k);
        }
        let (pack, read) = loaded(&[
            pack_tile(1, key_of(&game, 1), 7),
            pack_tile(2, key_of(&game, 2) ^ 1, 9),
        ]);
        assert_eq!(pack.counts(), [2, 0, 0]);
        assert_eq!(pack.decoded_bytes(), (2 * 64 * 64 * 4) as f64);
        // Off: setting the pack composes nothing; the RGBA path as before.
        let v = game.frame_version();
        game.set_hd_pack(&pack);
        assert!(
            game.has_hd_pack() && game.hd_frame_len() == 0,
            "off: no HD frame"
        );
        assert_eq!(game.frame_version(), v, "off: nothing to draw");
        assert_eq!(
            game.hd_pack_matches(),
            [1, 0, 0],
            "record 1 matches, 2 does not"
        );
        for (s, scale) in [(4, Scale::X4), (2, Scale::X2)] {
            game.set_hd(true, s);
            let with = expected_hd_with(&game, scale, Some(&read));
            assert!(
                game.hd_frame_rgba() == with,
                "{s}x: the pack's image of record 1"
            );
            assert!(with != expected_hd(&game, scale), "{s}x: not MMPX's");
            game.hd_take_dirty();
            // Forgotten: MMPX at once, only the cells of record 1 written (cells 2 and 3).
            let v = game.frame_version();
            game.forget_hd_pack();
            assert!(
                game.frame_version() > v,
                "{s}x: the forget is drawn at once"
            );
            assert!(
                game.hd_frame_rgba() == expected_hd(&game, scale),
                "{s}x: MMPX"
            );
            assert_eq!(
                game.hd_take_dirty(),
                vec![32 * s, 32 * s, 32 * s, 16 * s],
                "{s}x: the cells whose HD image changed"
            );
            let mut fresh = Game::new();
            fresh.put_on_screen(tile_engine());
            for k in ["a", "a", "a", "b"] {
                press(&mut fresh, k);
            }
            fresh.set_hd(true, s);
            assert!(
                game.hd_frame_rgba() == fresh.hd_frame_rgba(),
                "{s}x: a game never given it"
            );
            // Set again: the pack's image, as a game given it from the start.
            game.set_hd_pack(&pack);
            assert!(game.hd_frame_rgba() == with, "{s}x: set again");
            fresh.set_hd_pack(&pack);
            assert!(fresh.hd_frame_rgba() == with, "{s}x: a new game given it");
            let [_, packed, _] = game.hd_cache_counts()[..] else {
                panic!("three counts")
            };
            assert!(packed >= 1.0, "{s}x: made from the pack");
        }
        // Record 1 rewritten in place and drawn at the last cell (3): its key moved, so the
        // pack's image (made from its earlier bytes) is not used; the cell shows MMPX of its
        // bytes now and cell 2, drawn before from its old version, is classic.
        let before = cell_bytes(&game, 3, 2);
        press(&mut game, "m");
        assert_eq!(game.hd_pack_matches(), [0, 0, 0], "nothing matches now");
        let now = expected_hd_with(&game, Scale::X2, Some(&read));
        assert!(
            now == expected_hd(&game, Scale::X2),
            "no pack image is used"
        );
        assert!(
            game.hd_frame_rgba() == now,
            "MMPX of the new bytes, stale classic"
        );
        assert!(
            cell_bytes(&game, 3, 2) != before,
            "the last cell is drawn again"
        );
        let rgba = game.rgba.clone();
        let classic: Vec<u8> = (64..96)
            .flat_map(|y| {
                (64..96).flat_map(move |x| {
                    let at = ((y / 2) * u6_render::WIDTH + x / 2) * 4;
                    [at, at + 1, at + 2, at + 3]
                })
            })
            .map(|i| rgba[i])
            .collect();
        assert!(
            cell_bytes(&game, 2, 2) == classic,
            "the old version's cell: classic"
        );
        // Off again: setting or forgetting the pack composes nothing.
        game.set_hd(false, 2);
        let held = game.hd_frame_rgba();
        let v = game.frame_version();
        game.forget_hd_pack();
        game.set_hd_pack(&pack);
        assert_eq!(game.frame_version(), v);
        assert!(game.hd_frame_rgba() == held, "off: not composed");
        // A start keeps the pack on the game (the page sets it on each new one too).
        game.put_on_screen(tile_engine());
        assert!(game.has_hd_pack());
        assert_eq!(game.hd_pack_matches(), [1, 0, 0]);
    }

    /// One pixel by hand (no reference code): record 1's first pixel is colour 1, (40, 90, 254)
    /// in [`tile_engine`]'s palette, drawn at (16, 32). The pack's picture repeats one 2 × 2
    /// block: (200, 10, 0) opaque, (100, 30, 0) opaque, then two transparent pixels. At 4× the
    /// pixel's block is those (the transparent ones the classic colour); at 2× every sub-pixel is
    /// the block's mean, (150, 20, 0) at alpha 128 ((200·255 + 100·255 + 255) / 510 = 150,
    /// (10·255 + 30·255 + 255) / 510 = 20, (510 + 2) / 4 = 128), over the classic colour:
    /// (150·128 + 40·127 + 127) / 255 = 95, (20·128 + 90·127 + 127) / 255 = 55,
    /// (0·128 + 254·127 + 127) / 255 = 127.
    #[test]
    fn a_pack_pixel_by_hand_at_4x_and_2x() {
        let mut game = Game::new();
        game.put_on_screen(tile_engine());
        press(&mut game, "a");
        let mut image = u6_hdpack::Rgba::new(64, 64);
        for (i, p) in image.pixels.chunks_exact_mut(4).enumerate() {
            let (x, y) = (i % 64, i / 64);
            p.copy_from_slice(match (x % 2, y % 2) {
                (0, 0) => &[200, 10, 0, 255],
                (1, 0) => &[100, 30, 0, 255],
                _ => &[0, 0, 0, 0],
            });
        }
        let hash = key_of(&game, 1);
        let (pack, _) = loaded(&[u6_hdpack::PackImage {
            id: u6_hdpack::ImageId::Tile(1),
            hash,
            image,
        }]);
        game.set_hd_pack(&pack);
        let w = u6_render::WIDTH;
        let px = |game: &Game, s: usize, (x, y): (usize, usize)| {
            let at = (y * w * s + x) * 4;
            game.hd_frame_rgba()[at..at + 4].to_vec()
        };
        assert_eq!(&game.rgba[(32 * w + 16) * 4..][..4], [40, 90, 254, 255]);
        game.set_hd(true, 4);
        assert_eq!(px(&game, 4, (64, 128)), [200, 10, 0, 255]);
        assert_eq!(px(&game, 4, (65, 128)), [100, 30, 0, 255]);
        assert_eq!(px(&game, 4, (64, 129)), [40, 90, 254, 255]);
        assert_eq!(px(&game, 4, (65, 129)), [40, 90, 254, 255]);
        game.set_hd(true, 2);
        for (x, y) in [(32, 64), (33, 64), (32, 65), (33, 65)] {
            assert_eq!(px(&game, 2, (x, y)), [95, 55, 127, 255], "({x}, {y})");
        }
    }

    /// The pack's limits and refusals as the page sees them: a pack that does not read is
    /// refused with the reader's reason, a file over [`MAX_PACK_FILE_BYTES`] before it is read,
    /// and the probe tells a pack from a zip of game files.
    #[test]
    fn a_pack_that_does_not_read_is_refused_and_the_probe_tells_one() {
        let bytes = u6_hdpack::write_pack(&[pack_tile(5, 1, 3)], &generator(), true).unwrap();
        let mut bad = bytes.clone();
        let mid = bad.len() / 2;
        bad[mid] ^= 1;
        let refused = load_pack(&bad).err().unwrap();
        assert!(
            refused.starts_with("it does not read as a zip"),
            "{refused}"
        );
        let mut files = u6_intake::zip::read_zip(&bytes, |_| true).unwrap();
        files.push(("notes.txt".into(), b"x".to_vec()));
        let extra = refused_by(&files);
        assert!(extra.starts_with("it holds a file"), "{extra}");
        let huge = vec![0u8; MAX_PACK_FILE_BYTES as usize + 1];
        let over = load_pack(&huge).err().unwrap();
        assert!(over.starts_with("the file is over the 159 MiB"), "{over}");
        assert_eq!(
            load_pack(b"PP20 not a zip").err().unwrap(),
            "it is not a zip."
        );
        let tell = |file: &[u8]| {
            let mut p = PackProbe::new(file.len() as f64);
            while let [start, end] = p.need()[..] {
                p.give(&file[start as usize..end as usize]);
            }
            assert!(p.told());
            p.is_pack()
        };
        assert!(
            tell(&bytes) && tell(&bad),
            "a pack (a broken one is refused by the loader)"
        );
        // A pack whose directory breaks the format is refused by the probe already.
        let mut p = PackProbe::new(0.0);
        assert!(p.refusal().is_empty());
        let entries: Vec<zipscan::tests::Entry> = files
            .iter()
            .map(|(n, b)| zipscan::tests::stored(n, b))
            .collect();
        let extra = zipscan::tests::zip(&entries);
        p = PackProbe::new(extra.len() as f64);
        while let [start, end] = p.need()[..] {
            p.give(&extra[start as usize..end as usize]);
        }
        assert!(p.is_pack());
        assert_eq!(
            p.refusal(),
            "it holds a file an HD pack does not have, or one twice."
        );
        let disks = zipscan::tests::zip(&[zipscan::tests::stored("Disk 1.adf", &[1; 64])]);
        assert!(!tell(&disks) && !tell(b"PP20") && !tell(&[]));
        assert!(!PackProbe::new(f64::NAN).is_pack() && PackProbe::new(-1.0).told());
    }

    /// Why the pack of `files` (stored in a zip as they are) is refused.
    fn refused_by(files: &[(String, Vec<u8>)]) -> String {
        let entries: Vec<zipscan::tests::Entry> = files
            .iter()
            .map(|(n, b)| zipscan::tests::stored(n, b))
            .collect();
        load_pack(&zipscan::tests::zip(&entries)).err().unwrap()
    }

    /// The plane and the keys the page's tests read: the source of a pixel drawn from a record,
    /// and the record's key (empty before a start, outside the screen, or for no such image).
    #[test]
    fn the_source_of_a_pixel_and_the_key_of_its_image() {
        let mut game = Game::new();
        assert!(game.hd_source_at(0, 0).is_empty() && game.hd_image_key(1, 1).is_empty());
        game.put_on_screen(tile_engine());
        press(&mut game, "a");
        let src = game.hd_source_at(16 + 5, 32 + 9);
        assert_eq!(src, vec![1, 1, 0, 5, 9, 0], "record 1 at (5, 9), version 0");
        assert!(game.hd_source_at(u6_render::WIDTH as u32, 0).is_empty());
        assert!(game.hd_source_at(0, u6_render::HEIGHT as u32).is_empty());
        let e = game.engine.as_ref().unwrap();
        let rec = &e.state().gfx.records[1];
        let want = u6_hdpack::tile_hash(&rec.pixels, &rec.transparent).unwrap();
        assert_eq!(game.hd_image_key(1, 1), format!("{want:016x}"));
        assert!(game.hd_image_key(9, 1).is_empty() && game.hd_image_key(3, 7).is_empty());
    }

    /// Before a start the HD frame is the black frame scaled.
    #[test]
    fn before_a_start_the_hd_frame_is_black() {
        let mut game = Game::new();
        game.set_hd(true, 4);
        assert_eq!(game.hd_frame_len(), frame_bytes(4));
        assert!(game.hd_frame_rgba().chunks(4).all(|p| p == [0, 0, 0, 255]));
    }

    /// Spec 17: before a start there is no context menu and no entry runs (nothing queued).
    #[test]
    fn before_a_start_there_is_no_context_menu_and_no_entry_runs() {
        let mut game = Game::new();
        let (x, y) = hero_point();
        assert_eq!(game.context_menu(x, y), "");
        assert!(!game.command_at(0x88, x, y));
        assert!(game.pending.is_empty() && game.run.is_none());
    }

    /// Spec 17: the context menu's JSON: its kind, cell, underlined command and sections, each
    /// entry's code, whether it can run, its reason's code and the theft mark; the name as a JSON
    /// string (made-up names here).
    #[test]
    fn the_context_menus_json() {
        use u6_core::ui::menu::{Entry, Kind, Menu, Reason, Section};
        let entry = |code, reason, theft| Entry {
            code,
            reason,
            theft,
        };
        let map = Menu {
            kind: Kind::Map,
            cell: Some((3, -1)),
            underlined: Some(0x88),
            sections: vec![Section {
                object: 300,
                kind: Some((0x80, 2)),
                name: "a \"b\"".into(),
                entries: vec![
                    entry(0x88, Some(Reason::Far), false),
                    entry(0x85, None, true),
                ],
            }],
        };
        assert_eq!(
            menu_json(&map),
            r#"{"kind":"map","cell":[3,-1],"underlined":136,"sections":[{"object":300,"kind":[128,2],"name":"a \"b\"","entries":[{"code":136,"enabled":false,"reason":"far","theft":false},{"code":133,"enabled":true,"reason":null,"theft":true}]}]}"#
        );
        let panel = Menu {
            kind: Kind::Panel,
            cell: None,
            underlined: None,
            sections: Vec::new(),
        };
        assert_eq!(
            menu_json(&panel),
            r#"{"kind":"panel","cell":null,"underlined":null,"sections":[]}"#
        );
    }

    /// Spec 17, on the user's zip (skipped when absent; read only): at the prompt a right press
    /// on the hero (the view's centre cell, [`cell_point`], so in either build's layout) has a
    /// map menu with Look, also with a pointer move queued since the last step;
    /// `command_at` Look there queues the icon press and,
    /// once Look reads its target, the press on the hero (Look then shows the portrait and waits
    /// for a key: no prompt); with no prompt it refuses. A key gets the prompt back.
    #[test]
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn command_at_runs_look_on_the_hero_through_the_target_read() {
        if !std::path::Path::new(ZIP).exists() {
            eprintln!("skipped: {ZIP} not present");
            return;
        }
        let mut game = Game::new();
        game.collect("ultima-vi.zip", std::fs::read(ZIP).quiet())
            .quiet();
        game.boot(1, true).quiet();
        let prompt = |game: &mut Game| {
            for _ in 0..3000 {
                if game.at_prompt() {
                    return true;
                }
                game.step(20);
            }
            false
        };
        assert!(prompt(&mut game), "the prompt");
        let (hx, hy) = hero_point();
        let menu = game.context_menu(hx, hy);
        assert!(
            menu.starts_with(r#"{"kind":"map""#) && menu.contains(r#""code":132,"enabled":true"#)
        );
        let (mx, my) = cell_point(HALF_W as i32 + 2, HALF_H as i32 + 2);
        game.pointer(mx, my, 0);
        let after_a_move = game.context_menu(hx, hy);
        assert!(after_a_move == menu, "a pointer move queued: the same menu");
        assert!(game.command_at(0x84, hx, hy));
        assert!(
            !game.command_at(0x84, hx, hy),
            "no prompt with the icon queued"
        );
        let mut read = false;
        for _ in 0..200 {
            game.step(20);
            if game.run.is_none() {
                read = !game.at_prompt();
                break;
            }
        }
        for _ in 0..50 {
            game.step(20);
        }
        assert!(
            read && !game.at_prompt(),
            "Look read its target and waits on the portrait"
        );
        game.key_down("Space", " ", false);
        assert!(prompt(&mut game), "the prompt again");
    }

    /// The user's zip booted `game` first, stepped to its prompt (skipped when absent).
    #[allow(clippy::disallowed_methods)] // tests may read files
    fn at_the_prompt() -> Option<Game> {
        if !std::path::Path::new(ZIP).exists() {
            eprintln!("skipped: {ZIP} not present");
            return None;
        }
        let mut game = Game::new();
        game.collect("ultima-vi.zip", std::fs::read(ZIP).quiet())
            .quiet();
        game.boot(1, true).quiet();
        assert!(steps_to_prompt(&mut game), "the prompt");
        Some(game)
    }

    /// Steps `game` (20 ms each, at most 3000) until the prompt is up; false if it never is.
    fn steps_to_prompt(game: &mut Game) -> bool {
        steps_to_prompt_within(game, 3000)
    }

    /// [`steps_to_prompt`] in at most `steps` steps.
    fn steps_to_prompt_within(game: &mut Game, steps: usize) -> bool {
        for _ in 0..steps {
            if game.at_prompt() {
                return true;
            }
            game.step(20);
        }
        false
    }

    /// The frame pixel a test presses for view cell (col, row), laid out from the build's map
    /// geometry ([`u6_core::geom`]: the view's origin `MAP_X`/`MAP_Y`, Original (0, 0), Wide
    /// (320, 0)): the cell's centre, except in the last column and row, whose centre lies past
    /// the shown map's edge (`SHOWN`, the view less half a cell each side; no menu there), so
    /// theirs is 3 pixels inside that edge, still in the same cell.
    fn cell_point(col: i32, row: i32) -> (i32, i32) {
        use u6_core::geom::{MAP_X, MAP_Y, SHOWN};
        let at = |k: i32, origin: u16, edge: u16| {
            (i32::from(origin) + 16 * k + 8).min(i32::from(edge) - 3)
        };
        (at(col, MAP_X, SHOWN.2), at(row, MAP_Y, SHOWN.3))
    }

    /// The hero's cell: [`u6_core::ui::pointer::LEADER_PIXEL`], the centre of the view's centre
    /// cell, the leader's.
    fn hero_point() -> (i32, i32) {
        let (x, y) = u6_core::ui::pointer::LEADER_PIXEL;
        (i32::from(x), i32::from(y))
    }

    /// Spec 17 rounds 2 and 3 and follow-up F0, items 2 and 12, on the user's zip: with the
    /// leader's inventory shown (as Drop's key shows it, 0x92; Drop then let go with Escape),
    /// `context_menu` on every cell of the build's view (Original 11 × 11, Wide 37 × 21, each
    /// pressed at [`cell_point`]), on two inventory slots and on one control point leaves the
    /// running game's state as it was, checked after each query (the checks run on a copy); every
    /// view cell gives a menu, each slot gives one exactly when it holds something (and at least
    /// one does). The slots are the first list cell and an equipment cell, the neck (the cell
    /// lookups 0x111e3c / 0x111eec), inside the side panel's click region (region 2, the UI's own
    /// rect, the same pixels in both builds); the control (0xc0, 0x80) lies below that region and
    /// off the map, and gives none.
    #[test]
    fn context_menu_leaves_the_game_as_it_was() {
        use u6_core::geom::{VIEW_H, VIEW_W};
        use u6_core::ui::pointer::{map_column, region, view_cell};
        let Some(mut game) = at_the_prompt() else {
            return;
        };
        assert!(game.key_down("KeyD", "d", false), "Drop's key");
        for _ in 0..200 {
            game.step(20);
            if !game.at_prompt() {
                break;
            }
        }
        assert!(game.key_up("KeyD"), "Drop's key let go");
        // Escape: the first takes the cursor off (0x106e1e), the next one ends the read.
        for _ in 0..4 {
            if steps_to_prompt_within(&mut game, 50) {
                break;
            }
            assert!(game.key_down("Escape", "Escape", false), "Escape taken");
            game.step(20);
            assert!(game.key_up("Escape"), "Escape let go");
        }
        assert!(steps_to_prompt(&mut game), "the prompt again");
        let before = game.engine.as_ref().quiet().state().clone();
        assert_eq!(before.ui.panel.mode, 0x92, "the inventory shown");
        // After each query, the game's state as it was (a write would show at the query that
        // made it).
        let query = |x: i32, y: i32| {
            let menu = game.context_menu(x, y);
            let same = *game.engine.as_ref().quiet().state() == before;
            assert!(
                same,
                "the game's state as it was after the query at ({x}, {y})"
            );
            !menu.is_empty()
        };
        let mut cells = 0;
        for row in 0..VIEW_H as i32 {
            for col in 0..VIEW_W as i32 {
                let (x, y) = cell_point(col, row);
                let cell = view_cell(x as i16, y as i16);
                assert_eq!(
                    cell,
                    (col as i16, row as i16),
                    "the press point in its own cell"
                );
                cells += usize::from(query(x, y));
            }
        }
        let panel_rect = region(&before.ui.pointer.tables, 2).expect("the side panel's region");
        let in_panel = |x: i32, y: i32| {
            let [x0, y0, x1, y1] = panel_rect.map(i32::from);
            (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
        };
        // What the slots hold: list cell 0, and equipment slot 1 (the neck); an equipment value of
        // 0, or 1 (the both-hands marker), is an empty slot.
        let inv = &before.inventory;
        let neck = inv.equip(1);
        let slots = [
            ((0x100, 0x28), inv.cells[0] != 0),
            ((0xb8, 0x20), neck != 0 && neck != 1),
        ];
        let (mut panel, mut held) = (0, 0);
        for ((x, y), holds) in slots {
            assert!(in_panel(x, y), "a slot inside the side panel's region");
            let menu = query(x, y);
            assert_eq!(
                menu, holds,
                "a menu on slot ({x}, {y}) exactly when it holds something"
            );
            panel += usize::from(menu);
            held += usize::from(holds);
        }
        let (cx, cy) = (0xc0, 0x80);
        assert!(
            !in_panel(cx, cy) && !map_column(cx as i16),
            "the control: off the panel and the map"
        );
        assert!(!query(cx, cy), "the control: no menu");
        println!(
            "menus given: view {cells} of {}, panel {panel} of 2 (slots holding something {held})",
            VIEW_W * VIEW_H
        );
        assert_eq!(cells, VIEW_W * VIEW_H, "a menu on every view cell");
        assert!(held >= 1, "a slot holds something");
    }

    /// Spec 17 Acceptance 5 (round 2, item 10), on the user's zip, through `Game::command_at` and
    /// `Game::step`: the leader starts without a readied spellbook, so the menu shows Cast disabled
    /// as book; forced anyway on a cell two to five steps off that shows it ([`cell_point`],
    /// either build's layout), Cast runs its own refusal before any target
    /// read (its line printed, the prompt back), the held press is dropped, and the leader has not
    /// moved.
    #[test]
    fn cast_forced_without_a_book_refuses_and_drops_its_target_press() {
        let Some(mut game) = at_the_prompt() else {
            return;
        };
        // Cast is offered on a cell with an object (a bare tile gives Look alone), so the cell is
        // the first one whose menu shows Cast disabled as book two to five steps from the hero
        // (inside the original's targeting box in both builds), two east first: what shows Cast
        // two east differs between the builds.
        const BOOK: &str = r#""code":130,"enabled":false,"reason":"book""#;
        let near = (-5..=5i32).flat_map(|r| (-5..=5i32).map(move |c| (c, r)));
        let (dx, dy) = std::iter::once((2, 0))
            .chain(near.filter(|&(c, r)| c.abs().max(r.abs()) >= 2 && (c, r) != (2, 0)))
            .find(|&(c, r)| {
                let (x, y) = cell_point(HALF_W as i32 + c, HALF_H as i32 + r);
                game.context_menu(x, y).matches(BOOK).count() == 1
            })
            .expect("a cell two to five steps off whose menu shows Cast disabled as book once");
        let (x, y) = cell_point(HALF_W as i32 + dx, HALF_H as i32 + dy);
        let at = game.leader_position();
        let lines = game.engine.as_ref().quiet().state().ui.messages.len();
        assert!(game.command_at(0x82, x, y));
        let mut released = 0;
        for _ in 0..600 {
            let before = game.pending.len();
            game.step(20);
            released += usize::from(game.pending.len() > before);
            if game.run.is_none() && game.at_prompt() {
                break;
            }
        }
        let printed = game.engine.as_ref().quiet().state().ui.messages.len() - lines;
        println!(
            "the cell {dx}, {dy} from the hero; presses released: {released}, lines: {printed}"
        );
        assert!(
            game.run.is_none() && game.at_prompt(),
            "the prompt is back, the run over"
        );
        assert_eq!(released, 0, "the target press never went in");
        assert!(printed > 0, "Cast printed its refusal");
        assert_eq!(game.leader_position(), at, "the leader did not move");
    }

    /// Spec 17 round 2, item 0: a key let go just before a right press on the hero ([`hero_point`])
    /// (its release queued, as `key_up` queues it) does not hide the menu, and the entry chosen
    /// then runs: the release is applied where the engine queues the input and is never one of
    /// its events.
    #[test]
    fn a_key_let_go_just_before_a_right_press_keeps_the_menu_and_its_run() {
        let Some(mut game) = at_the_prompt() else {
            return;
        };
        assert!(game.key_down("KeyX", "x", false));
        game.step(20);
        assert!(steps_to_prompt(&mut game), "the prompt after the key");
        let (hx, hy) = hero_point();
        let menu = game.context_menu(hx, hy);
        assert!(!menu.is_empty(), "a menu on the hero");
        assert!(game.key_up("KeyX"), "the key was held");
        let with_release = game.context_menu(hx, hy);
        assert!(
            with_release == menu,
            "the same menu with the release queued"
        );
        assert!(game.command_at(0x84, hx, hy), "the entry runs");
        let mut read = 0;
        for _ in 0..200 {
            game.step(20);
            if game.run.is_none() {
                read = u32::from(!game.at_prompt());
                break;
            }
        }
        assert_eq!(read, 1, "Look read its target");
    }
}
